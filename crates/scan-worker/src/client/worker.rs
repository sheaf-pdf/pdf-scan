//! A running worker, from the client's side: its requests written, its answers and a batch's
//! events taken under the watchdog, and its end.

use std::{
    io::{self, Write},
    path::Path,
    process::{Child, ChildStdin, ExitStatus},
    sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender},
    thread,
    time::{Duration, Instant},
};

use scan_device::in_progress::{Progress, Unit};
use scan_device::{
    Caps, DeviceId, DeviceInfo, Done, Driver, Event, Resolutions, Settings, WindowClosed,
    WindowOwner,
};
use tempfile::TempDir;

use super::read::{self, Incoming, Stderr};
use super::{Limits, Timeouts, WorkerCommand, spawn};
use crate::wire::{self, PROTOCOL, Reply, Request};
use crate::{Error, ProtocolError};

/// How often a batch reads its caller's progress for a cancel while the worker is silent.
const CANCEL_POLL: Duration = Duration::from_millis(50);

/// A worker started for one listing or one open device.
pub(super) struct Worker {
    /// The driver API it was started for: every device it gives must be one of it.
    driver: Driver,
    timeouts: Timeouts,
    /// `None` once it has ended: reaped, or handed to a thread that waits for it.
    process: Option<Process>,
    /// `None` once it has ended, or a write to it failed.
    stdin: Option<ChildStdin>,
    /// What its reader hands on, one at a time.
    messages: Receiver<Incoming>,
    stderr: Stderr,
    process_id: u32,
}

/// The worker's process, and its private directory, removed once it has exited.
struct Process {
    child: Child,
    dir: TempDir,
}

/// How a worker's process ended.
enum Reaped {
    /// It exited of itself.
    Exited(ExitStatus),
    /// It was killed, or had been already.
    Killed,
}

impl Worker {
    /// `command` started for `driver`, to list or to open `opens`, in a directory of its own
    /// under `work_dir`; its `hello` taken.
    pub(super) fn start(
        command: &WorkerCommand,
        work_dir: &Path,
        driver: Driver,
        opens: Option<&DeviceId>,
        limits: Limits,
        timeouts: Timeouts,
    ) -> Result<Worker, Error> {
        let not_started = |source| Error::Start {
            program: command.program.clone(),
            source,
        };
        let (mut child, dir) =
            spawn::spawn(command, work_dir, driver, opens).map_err(not_started)?;
        let process_id = child.id();
        let pipes = (child.stdin.take(), child.stdout.take(), child.stderr.take());
        let (sent, messages) = mpsc::sync_channel(0);
        let threads = match pipes {
            (Some(stdin), Some(stdout), Some(stderr)) => {
                read_threads(stdout, stderr, limits, sent).map(|stderr| (stdin, stderr))
            }
            // Each was asked for: never, but by a broken system.
            _ => Err(io::Error::other("the worker's pipes weren't made")),
        };
        let (stdin, stderr) = match threads {
            Ok(started) => started,
            Err(source) => {
                let _ = child.kill();
                reap(Process { child, dir }, timeouts.grace);
                return Err(not_started(source));
            }
        };
        let mut worker = Worker {
            driver,
            timeouts,
            process: Some(Process { child, dir }),
            stdin: Some(stdin),
            messages,
            stderr,
            process_id,
        };
        worker.hello()?;
        Ok(worker)
    }

    pub(super) fn process_id(&self) -> u32 {
        self.process_id
    }

    /// Its first line, its `hello` with this client's protocol, within [`Timeouts::start`].
    fn hello(&mut self) -> Result<(), Error> {
        match self.receive(Some(self.timeouts.start))? {
            Incoming::Message(Reply::Hello { protocol, .. }) if protocol == PROTOCOL => Ok(()),
            Incoming::Message(Reply::Hello { protocol, .. }) => {
                Err(self.broken(ProtocolError::Version {
                    worker: protocol,
                    client: PROTOCOL,
                }))
            }
            Incoming::Message(_) | Incoming::Page(_) => Err(self.broken(ProtocolError::NoHello)),
            Incoming::Broken(error) => Err(self.broken(error)),
            Incoming::End { page_cut_short } => Err(self.crashed(page_cut_short)),
        }
    }

    /// The devices it lists within `deadline`, each of its driver.
    pub(super) fn list(&mut self, deadline: Duration) -> Result<Vec<DeviceInfo>, Error> {
        self.request(&Request::List { deadline })?;
        match self.answer(Some(deadline.saturating_add(self.timeouts.grace)))? {
            Reply::Devices(devices) => {
                if devices.iter().any(|device| device.driver != self.driver) {
                    return Err(self.broken(ProtocolError::Invalid {
                        what: "a device of another driver API than the worker's",
                    }));
                }
                Ok(devices)
            }
            Reply::Error(error) => Err(Error::Scan(error)),
            other => Err(self.unexpected(&other)),
        }
    }

    /// `stored` opened within `deadline`: the device as found, and the stored id where another
    /// was opened in its place.
    pub(super) fn open(
        &mut self,
        stored: &DeviceInfo,
        deadline: Duration,
    ) -> Result<(DeviceInfo, Option<DeviceId>), Error> {
        self.request(&Request::Open {
            device: stored.clone(),
            deadline,
        })?;
        match self.answer(Some(deadline.saturating_add(self.timeouts.grace)))? {
            Reply::Opened { info, renamed_from } => {
                if info.driver != self.driver {
                    return Err(self.broken(ProtocolError::Invalid {
                        what: "an opened device of another driver API than the stored one's",
                    }));
                }
                Ok((info, renamed_from))
            }
            Reply::Error(error) => Err(Error::Scan(error)),
            other => Err(self.unexpected(&other)),
        }
    }

    /// The open device's caps, checked as the model's rules have them.
    pub(super) fn caps(&mut self) -> Result<Caps, Error> {
        self.request(&Request::Caps)?;
        match self.answer(Some(self.timeouts.silence))? {
            Reply::Caps(caps) => match check_caps(&caps) {
                Ok(()) => Ok(caps),
                Err(error) => Err(self.broken(error)),
            },
            Reply::Error(error) => Err(Error::Scan(error)),
            other => Err(self.unexpected(&other)),
        }
    }

    /// The driver's settings window, untimed.
    pub(super) fn settings_window(
        &mut self,
        owner: Option<WindowOwner>,
    ) -> Result<WindowClosed, Error> {
        self.request(&Request::SettingsWindow { owner })?;
        match self.answer(None)? {
            Reply::Window(closed) => Ok(closed),
            Reply::Error(error) => Err(Error::Scan(error)),
            other => Err(self.unexpected(&other)),
        }
    }

    /// A batch, its events sent on `events` as they come and its progress counted on
    /// `progress`, under the watchdog; `progress` read for a cancel.
    pub(super) fn scan(
        &mut self,
        settings: &Settings,
        first_sheet: u32,
        events: &SyncSender<Event>,
        progress: &dyn Progress,
    ) -> Result<Done, Error> {
        self.request(&Request::Scan {
            settings: settings.clone(),
            first_sheet,
        })?;
        let mut batch = Batch::default();
        // When the calling thread last began to wait on the worker: after the line before, and
        // after handing on its event.
        let mut since = Instant::now();
        loop {
            if batch.cancelled_at.is_none() && progress.is_cancelled() {
                self.cancel(&mut batch);
            }
            let now = Instant::now();
            // Never where a time is too long to add to the clock's.
            let wait = match batch.deadline(since, self.timeouts) {
                Some(deadline) if now >= deadline => {
                    return Err(self.stopped(now.duration_since(since)));
                }
                Some(deadline) => (deadline - now).min(CANCEL_POLL),
                None => CANCEL_POLL,
            };
            let incoming = match self.messages.recv_timeout(wait) {
                Ok(incoming) => incoming,
                Err(RecvTimeoutError::Timeout) => continue,
                // The reader gone without a word: never, but by a panic there.
                Err(RecvTimeoutError::Disconnected) => Incoming::End {
                    page_cut_short: false,
                },
            };
            let event = match incoming {
                Incoming::Page(page) => Event::Page(page),
                Incoming::Message(Reply::Applied(applied)) => Event::Applied(applied),
                Incoming::Message(Reply::Sheet(sheet)) => Event::Sheet(sheet),
                Incoming::Message(Reply::Status(status)) => Event::Status(status),
                Incoming::Message(Reply::Progress { done, total }) => {
                    if batch.total != Some(total) {
                        progress.set_total(total, Unit::Bytes);
                        batch.total = Some(total);
                    }
                    progress.set(done);
                    since = Instant::now();
                    continue;
                }
                Incoming::Message(Reply::Message { level, text }) => {
                    progress.message(level.into(), &text);
                    since = Instant::now();
                    continue;
                }
                // With the receiver gone, the batch is cancelled, whatever ended it after.
                Incoming::Message(Reply::Done(_)) if batch.receiver_gone => {
                    return Ok(Done::Cancelled);
                }
                Incoming::Message(Reply::Done(done)) => return Ok(done),
                Incoming::Message(Reply::Error(_)) if batch.receiver_gone => {
                    return Ok(Done::Cancelled);
                }
                Incoming::Message(Reply::Error(error)) => return Err(Error::Scan(error)),
                Incoming::Message(Reply::Refused { reason }) => {
                    return Err(self.broken(ProtocolError::Refused { reason }));
                }
                Incoming::Message(other) => return Err(self.unexpected(&other)),
                Incoming::Broken(error) => return Err(self.broken(error)),
                Incoming::End { page_cut_short } => return Err(self.crashed(page_cut_short)),
            };
            if !batch.receiver_gone && events.send(event).is_err() {
                batch.receiver_gone = true;
                if batch.cancelled_at.is_none() {
                    self.cancel(&mut batch);
                }
            }
            since = Instant::now();
        }
    }

    /// `cancel` sent, once, and when.
    fn cancel(&mut self, batch: &mut Batch) {
        batch.cancelled_at = Some(Instant::now());
        // Encodes always: it holds nothing.
        if let Ok(line) = wire::line(&Request::Cancel) {
            self.write(&line);
        }
    }

    /// `request` sent, if it can be: refused at once where the worker has ended
    /// ([`Error::Ended`]) or the request doesn't come back from its own line as it went
    /// ([`Error::Unsendable`]).
    fn request(&mut self, request: &Request) -> Result<(), Error> {
        if self.process.is_none() {
            return Err(Error::Ended);
        }
        let line = sendable(request)?;
        self.write(&line);
        Ok(())
    }

    /// `line` written to the worker's input. A write that fails means the worker has gone,
    /// which its output's end then says: the input is dropped, and nothing more is written.
    fn write(&mut self, line: &[u8]) {
        if let Some(stdin) = &mut self.stdin
            && stdin.write_all(line).and_then(|()| stdin.flush()).is_err()
        {
            self.stdin = None;
        }
    }

    /// The reader's next message, waiting at most `limit` (forever for `None`); past it, the
    /// worker is killed ([`Error::Stopped`]).
    fn receive(&mut self, limit: Option<Duration>) -> Result<Incoming, Error> {
        let received = match limit {
            Some(limit) => self
                .messages
                .recv_timeout(limit)
                .map_err(|error| match error {
                    RecvTimeoutError::Timeout => Some(limit),
                    RecvTimeoutError::Disconnected => None,
                }),
            None => self.messages.recv().map_err(|_| None),
        };
        match received {
            Ok(incoming) => Ok(incoming),
            Err(Some(silence)) => Err(self.stopped(silence)),
            // The reader gone without a word: never, but by a panic there.
            Err(None) => Ok(Incoming::End {
                page_cut_short: false,
            }),
        }
    }

    /// The answer to a request, waiting at most `limit`: a message, but a page or a refusal,
    /// which break the protocol.
    fn answer(&mut self, limit: Option<Duration>) -> Result<Reply, Error> {
        match self.receive(limit)? {
            Incoming::Message(Reply::Refused { reason }) => {
                Err(self.broken(ProtocolError::Refused { reason }))
            }
            Incoming::Message(reply) => Ok(reply),
            Incoming::Page(_) => Err(self.broken(ProtocolError::Unexpected { got: "page" })),
            Incoming::Broken(error) => Err(self.broken(error)),
            Incoming::End { page_cut_short } => Err(self.crashed(page_cut_short)),
        }
    }

    /// `reply` out of its place: the worker killed.
    fn unexpected(&mut self, reply: &Reply) -> Error {
        self.broken(ProtocolError::Unexpected { got: reply.name() })
    }

    /// The protocol broken: the worker killed.
    fn broken(&mut self, error: ProtocolError) -> Error {
        self.kill();
        Error::Protocol(error)
    }

    /// Silent for `silence`: the worker killed.
    fn stopped(&mut self, silence: Duration) -> Error {
        self.kill();
        Error::Stopped {
            silence,
            stderr: self.stderr.tail(self.timeouts.grace),
        }
    }

    /// The output ended unasked: the worker given the grace to exit, and killed if it hasn't.
    fn crashed(&mut self, page_cut_short: bool) -> Error {
        let status = match self.end(false) {
            Reaped::Exited(status) => Some(status),
            Reaped::Killed => None,
        };
        Error::Crashed {
            status,
            page_cut_short,
            stderr: self.stderr.tail(self.timeouts.grace),
        }
    }

    /// The worker killed at once, and reaped.
    pub(super) fn kill(&mut self) {
        self.end(true);
    }

    /// The worker asked to quit, given the grace to exit, and killed if it hasn't; `Ok` where
    /// it has ended already.
    pub(super) fn close(&mut self) -> Result<(), Error> {
        if self.process.is_none() {
            return Ok(());
        }
        if let Ok(line) = wire::line(&Request::Quit) {
            self.write(&line);
        }
        match self.end(false) {
            Reaped::Exited(status) if status.success() => Ok(()),
            Reaped::Exited(status) => Err(Error::Crashed {
                status: Some(status),
                page_cut_short: false,
                stderr: self.stderr.tail(self.timeouts.grace),
            }),
            Reaped::Killed => Err(Error::Stopped {
                silence: self.timeouts.grace,
                stderr: self.stderr.tail(self.timeouts.grace),
            }),
        }
    }

    /// The worker ended: its input closed, which a worker takes as a cancel and a quit; then
    /// killed at once where `kill` says, else given the grace to exit and killed if it hasn't;
    /// reaped, or handed to a thread that waits for it. Every call after it answers
    /// [`Error::Ended`].
    fn end(&mut self, kill: bool) -> Reaped {
        self.stdin = None;
        let Some(mut process) = self.process.take() else {
            return Reaped::Killed;
        };
        if !kill && let Some(status) = wait_for(&mut process.child, self.timeouts.grace) {
            // Its directory removed with it.
            drop(process);
            return Reaped::Exited(status);
        }
        let _ = process.child.kill();
        reap(process, self.timeouts.grace);
        Reaped::Killed
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        let _ = self.close();
    }
}

/// The reader's thread and standard error's started.
fn read_threads(
    stdout: impl io::Read + Send + 'static,
    stderr: impl io::Read + Send + 'static,
    limits: Limits,
    sent: SyncSender<Incoming>,
) -> io::Result<Stderr> {
    let stderr = Stderr::drain(stderr)?;
    thread::Builder::new()
        .name("scan-worker reader".to_owned())
        .spawn(move || read::read_output(stdout, limits, &sent))?;
    Ok(stderr)
}

/// A batch's state, for the watchdog and the cancel.
#[derive(Default)]
struct Batch {
    /// When the cancel was sent.
    cancelled_at: Option<Instant>,
    /// Whether the caller's receiver has gone: the rest of the batch is dropped.
    receiver_gone: bool,
    /// The total last given to the caller's progress.
    total: Option<Option<u64>>,
}

impl Batch {
    /// When the worker, silent since `since`, is killed: [`Timeouts::silence`] after it; and
    /// once cancelled, no later than [`Timeouts::cancel`] after the later of the cancel and
    /// `since`. `None`, never, where both are too long to add to the clock's time (a timeout
    /// of `Duration::MAX`, say).
    fn deadline(&self, since: Instant, timeouts: Timeouts) -> Option<Instant> {
        let silent = since.checked_add(timeouts.silence);
        let cancelled = self
            .cancelled_at
            .and_then(|at| at.max(since).checked_add(timeouts.cancel));
        match (silent, cancelled) {
            (Some(silent), Some(cancelled)) => Some(silent.min(cancelled)),
            (silent, cancelled) => silent.or(cancelled),
        }
    }
}

/// `request`'s line, if it comes back from it as it went: JSON writes a size that isn't finite
/// as `null`, which doesn't.
fn sendable(request: &Request) -> Result<Vec<u8>, Error> {
    let line = wire::line(request).map_err(|_| Error::Unsendable)?;
    let json = line.strip_suffix(b"\n").unwrap_or(&line);
    match serde_json::from_slice::<Request>(json) {
        Ok(back) if back == *request => Ok(line),
        _ => Err(Error::Unsendable),
    }
}

/// `caps` checked as the model's rules have them, which its constructors don't.
fn check_caps(caps: &Caps) -> Result<(), ProtocolError> {
    let invalid = |what| Err(ProtocolError::Invalid { what });
    if caps.sources.is_empty() {
        return invalid("caps with no source");
    }
    for source in &caps.sources {
        if source.formats.is_empty() {
            return invalid("a source with no format");
        }
        match &source.resolutions {
            Resolutions::List(values) if values.is_empty() => {
                return invalid("a source with an empty list of resolutions");
            }
            Resolutions::Range { min, max, .. } if min > max => {
                return invalid("a source whose range of resolutions has its min past its max");
            }
            _ => {}
        }
    }
    Ok(())
}

/// `child`'s exit, waited for at most `limit`, without a handler of the process's signals: its
/// status asked in turn, a pause between growing to 20 ms.
fn wait_for(child: &mut Child, limit: Duration) -> Option<ExitStatus> {
    let start = Instant::now();
    let mut pause = Duration::from_millis(1);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Some(status),
            Ok(None) => {}
            Err(_) => return None,
        }
        let left = limit.saturating_sub(start.elapsed());
        if left.is_zero() {
            return None;
        }
        thread::sleep(pause.min(left));
        pause = (pause * 2).min(Duration::from_millis(20));
    }
}

/// A killed `process` reaped within `limit`, its directory removed; or, where it isn't (a
/// process stuck in the system), handed to a thread that waits for it and then removes its
/// directory, so the caller isn't held.
fn reap(mut process: Process, limit: Duration) {
    if wait_for(&mut process.child, limit).is_some() {
        return;
    }
    // Where no thread can be made, the process is dropped unreaped, and its directory removed.
    let _ = thread::Builder::new()
        .name("scan-worker reaper".to_owned())
        .spawn(move || {
            let _ = process.child.wait();
            drop(process.dir);
        });
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU32;

    use scan_device::{Format, Paper, SizeMm, Source, SourceCaps};

    use super::*;

    #[test]
    fn a_request_json_can_t_hold_is_unsendable() {
        let mut settings = Settings::new(Source::Feeder, 300, Format::Gray8);
        assert!(
            sendable(&Request::Scan {
                settings: settings.clone(),
                first_sheet: 1
            })
            .is_ok()
        );
        for size in [
            SizeMm {
                width: f32::NAN,
                height: 297.0,
            },
            SizeMm {
                width: 210.0,
                height: f32::INFINITY,
            },
            SizeMm {
                width: f32::NEG_INFINITY,
                height: 297.0,
            },
        ] {
            settings.paper = Paper::Size(size);
            let scan = Request::Scan {
                settings: settings.clone(),
                first_sheet: 1,
            };
            assert!(
                matches!(sendable(&scan), Err(Error::Unsendable)),
                "{size:?}"
            );
        }
        // The edges JSON holds, sent.
        for width in [f32::MIN_POSITIVE, f32::MAX, -0.0, 1.0e-45] {
            settings.paper = Paper::Size(SizeMm { width, height: 1.0 });
            let scan = Request::Scan {
                settings: settings.clone(),
                first_sheet: 1,
            };
            assert!(sendable(&scan).is_ok(), "{width:?}");
        }
    }

    #[test]
    fn caps_the_model_forbids_are_refused() {
        let range = |min, max| Resolutions::Range {
            min,
            max,
            step: NonZeroU32::MIN,
        };
        let caps = |resolutions, formats| {
            Caps::new(vec![SourceCaps::new(Source::Feeder, resolutions, formats)])
        };
        assert_eq!(check_caps(&caps(range(50, 600), vec![Format::Bw1])), Ok(()));
        assert_eq!(
            check_caps(&caps(range(300, 300), vec![Format::Bw1])),
            Ok(())
        );
        assert_eq!(
            check_caps(&caps(Resolutions::List(vec![300]), vec![Format::Bw1])),
            Ok(())
        );
        for (refused, what) in [
            (Caps::new(Vec::new()), "caps with no source"),
            (caps(range(50, 600), Vec::new()), "a source with no format"),
            (
                caps(Resolutions::List(Vec::new()), vec![Format::Bw1]),
                "a source with an empty list of resolutions",
            ),
            (
                caps(range(601, 600), vec![Format::Bw1]),
                "a source whose range of resolutions has its min past its max",
            ),
        ] {
            assert_eq!(
                check_caps(&refused),
                Err(ProtocolError::Invalid { what }),
                "{refused:?}"
            );
        }
    }

    #[test]
    fn the_watchdog_counts_silence_and_once_cancelled_the_cancel_s_time() {
        let timeouts = Timeouts {
            silence: Duration::from_secs(60),
            cancel: Duration::from_secs(10),
            ..Timeouts::default()
        };
        let since = Instant::now();
        let mut batch = Batch::default();
        assert_eq!(
            batch.deadline(since, timeouts),
            Some(since + Duration::from_secs(60))
        );
        // Cancelled after the last line: counted from the cancel.
        let at = since + Duration::from_secs(5);
        batch.cancelled_at = Some(at);
        assert_eq!(
            batch.deadline(since, timeouts),
            Some(at + Duration::from_secs(10))
        );
        // A line after the cancel: counted from the line.
        let later = since + Duration::from_secs(8);
        assert_eq!(
            batch.deadline(later, timeouts),
            Some(later + Duration::from_secs(10))
        );
        // Never past the silence's.
        let long = Timeouts {
            cancel: Duration::from_secs(600),
            ..timeouts
        };
        assert_eq!(
            batch.deadline(since, long),
            Some(since + Duration::from_secs(60))
        );
        // A time too long for the clock is never, alone; the other still holds.
        let never = Timeouts {
            silence: Duration::MAX,
            cancel: Duration::MAX,
            ..timeouts
        };
        assert_eq!(batch.deadline(since, never), None);
        batch.cancelled_at = None;
        assert_eq!(batch.deadline(since, never), None);
        let silence_never = Timeouts {
            silence: Duration::MAX,
            ..timeouts
        };
        assert_eq!(batch.deadline(since, silence_never), None);
        batch.cancelled_at = Some(at);
        assert_eq!(
            batch.deadline(since, silence_never),
            Some(at + Duration::from_secs(10))
        );
    }
}
