//! A running worker, from the client's side: its requests written, its answers and a batch's
//! events taken under the watchdog and checked, its process watched, and its end.

use std::{
    io::{self, Write},
    path::Path,
    process::{Child, ChildStdin, ExitStatus},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, RecvTimeoutError, SyncSender},
    },
    thread,
    time::{Duration, Instant},
};

use scan_device::in_progress::{Progress, Unit};
use scan_device::{
    Applied, Caps, Clamped, DeviceId, DeviceInfo, Done, Driver, EncodedKind, Encoding, Event,
    Paper, Pixels, RawPage, Resolutions, ScanError, Settings, Side, SizeMm, WindowClosed,
    WindowOwner,
};
use tempfile::TempDir;

use super::read::{self, Incoming, Stderr};
use super::spawn::{self, NotStarted};
use super::{Limits, Timeouts, WorkerCommand};
use crate::wire::{self, PROTOCOL, REQUEST_LIMIT, Reply, Request};
use crate::{Error, ProtocolError, Timeout};

/// How often a batch reads its caller's progress for a cancel, and how often any wait on the
/// worker asks whether its process has exited, while the worker is silent.
const TICK: Duration = Duration::from_millis(50);

/// Once the worker's process has exited, how long its reader may go without a message before
/// what it handed on is taken as all of it; and how long the end of its standard error is waited
/// for once it has ended. A process leaves a pipe's buffer at most as it exits, read in a moment,
/// and a process it started may hold its pipes open for good.
const DRAIN: Duration = Duration::from_millis(250);

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
    /// Set by its reader while it reads a page's bytes.
    in_page: Arc<AtomicBool>,
    /// When its process was found to have exited, its status kept by its `Child`; `None` while
    /// it hasn't been.
    exited: Option<Instant>,
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

/// How a worker is ended ([`Worker::end`]).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Ending {
    /// Killed at once.
    Kill,
    /// Its output has ended, or its process exited: its input closed, which a worker takes as
    /// a cancel and a quit, and the grace given to exit.
    Ended,
    /// Asked to quit, and given the grace to exit with its input kept open: a worker takes its
    /// input's end for its client gone, and aborts 10 s later if it hasn't finished, so a
    /// driver slow to close would be aborted within a grace longer than that.
    Quit,
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
            spawn::spawn(command, work_dir, driver, opens).map_err(|error| match error {
                NotStarted::Program(source) => not_started(source),
                NotStarted::WorkDir(source) => Error::WorkDir {
                    dir: work_dir.to_owned(),
                    source,
                },
            })?;
        let process_id = child.id();
        let pipes = (child.stdin.take(), child.stdout.take(), child.stderr.take());
        let (sent, messages) = mpsc::sync_channel(0);
        let in_page = Arc::new(AtomicBool::new(false));
        let threads = match pipes {
            (Some(stdin), Some(stdout), Some(stderr)) => {
                read_threads(stdout, stderr, limits, sent, Arc::clone(&in_page))
                    .map(|stderr| (stdin, stderr))
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
            in_page,
            exited: None,
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
        match self.receive(Some(self.timeouts.start), Timeout::Start)? {
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
        let limit = deadline.saturating_add(self.timeouts.grace);
        match self.answer(Some(limit), Timeout::Deadline)? {
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
        let limit = deadline.saturating_add(self.timeouts.grace);
        match self.answer(Some(limit), Timeout::Deadline)? {
            Reply::Opened { info, renamed_from } => {
                if let Err(error) = check_opened(stored, &info, renamed_from.as_ref()) {
                    return Err(self.broken(error));
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
        match self.answer(Some(self.timeouts.silence), Timeout::Silence)? {
            Reply::Caps(caps) => match check_caps(&caps) {
                Ok(()) => Ok(caps),
                Err(error) => Err(self.broken(error)),
            },
            Reply::Error(error) => Err(Error::Scan(error)),
            other => Err(self.unexpected(&other)),
        }
    }

    /// The driver's settings window, untimed: only its answer, or the worker's end, ends it.
    pub(super) fn settings_window(
        &mut self,
        owner: Option<WindowOwner>,
    ) -> Result<WindowClosed, Error> {
        self.request(&Request::SettingsWindow { owner })?;
        match self.answer(None, Timeout::Silence)? {
            Reply::Window(closed) => Ok(closed),
            Reply::Error(error) => Err(Error::Scan(error)),
            other => Err(self.unexpected(&other)),
        }
    }

    /// A batch, its events checked against the model's order and sent on `events` as they
    /// come, and its progress counted on `progress`, under the watchdog; `progress` read for a
    /// cancel.
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
        let mut batch = Batch::new(first_sheet);
        // When the calling thread last began to wait on the worker: after the line before, and
        // after handing on its event.
        let mut since = Instant::now();
        loop {
            if batch.cancelled_at.is_none() && progress.is_cancelled() {
                self.cancel(&mut batch);
            }
            let now = Instant::now();
            // Never where a time is too long to add to the clock's.
            let until = match batch.deadline(since, self.timeouts) {
                Some((deadline, timeout)) if now >= deadline => {
                    return Err(self.stopped(timeout, now.duration_since(since)));
                }
                Some((deadline, _)) => deadline.min(now + TICK),
                None => now + TICK,
            };
            let Some(incoming) = self.next(Some(until)) else {
                continue;
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
                Incoming::Message(Reply::Error(ScanError::FeederEmpty)) if batch.order.paged => {
                    return Err(self.broken(ProtocolError::Invalid {
                        what: "an empty feeder as an error after pages, where the batch is done",
                    }));
                }
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
            if let Err(error) = batch.order.take(&event, settings) {
                return Err(self.broken(error));
            }
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
    /// ([`Error::Ended`]) or the request can't cross ([`Error::Unsendable`]).
    fn request(&mut self, request: &Request) -> Result<(), Error> {
        if self.process.is_none() {
            return Err(Error::Ended);
        }
        let line = sendable(request)?;
        self.write(&line);
        Ok(())
    }

    /// `line` written to the worker's input. A write that fails means the worker has gone,
    /// which its output's end, or its process's exit, then says: the input is dropped, and
    /// nothing more is written. Nothing is written to a process found to have exited: on Unix,
    /// a write to a pipe no process reads raises `SIGPIPE`, which ends an app that hasn't left
    /// it ignored (the crate's docs).
    fn write(&mut self, line: &[u8]) {
        if self.has_exited() {
            self.stdin = None;
        }
        if let Some(stdin) = &mut self.stdin
            && stdin.write_all(line).and_then(|()| stdin.flush()).is_err()
        {
            self.stdin = None;
        }
    }

    /// Whether the worker's process has exited, asked of the system where it isn't known yet:
    /// its status is kept by its `Child`, which reaps it.
    fn has_exited(&mut self) -> bool {
        if self.exited.is_none()
            && let Some(process) = &mut self.process
            && let Ok(Some(_)) = process.child.try_wait()
        {
            self.exited = Some(Instant::now());
        }
        self.exited.is_some()
    }

    /// The reader's next message, waiting until `until` (for good where `None`), and `None`
    /// once it has passed with none. The worker's process is watched meanwhile, every
    /// [`TICK`] it's silent: once it has exited, what the reader still holds is taken, and a
    /// reader quiet for [`DRAIN`] then is the output's end, whatever still holds the pipe
    /// open (a process the worker's driver started, say).
    fn next(&mut self, until: Option<Instant>) -> Option<Incoming> {
        loop {
            let now = Instant::now();
            let mut wait = match self.exited {
                Some(at) => (at + DRAIN).saturating_duration_since(now),
                None => TICK,
            };
            if let Some(until) = until {
                if now >= until {
                    return None;
                }
                wait = wait.min(until - now);
            }
            match self.messages.recv_timeout(wait) {
                Ok(incoming) => {
                    // Quiet counted from its last message.
                    if self.exited.is_some() {
                        self.exited = Some(Instant::now());
                    }
                    return Some(incoming);
                }
                // The reader gone without a word: never, but by a panic there.
                Err(RecvTimeoutError::Disconnected) => {
                    return Some(Incoming::End {
                        page_cut_short: false,
                    });
                }
                Err(RecvTimeoutError::Timeout) => {
                    if let Some(at) = self.exited
                        && at.elapsed() >= DRAIN
                    {
                        return Some(Incoming::End {
                            page_cut_short: self.in_page.load(Ordering::Relaxed),
                        });
                    }
                    self.has_exited();
                }
            }
        }
    }

    /// The reader's next message, waiting at most `limit` (for good for `None`); past it, the
    /// worker is killed ([`Error::Stopped`], `timeout` the time that ran out).
    fn receive(&mut self, limit: Option<Duration>, timeout: Timeout) -> Result<Incoming, Error> {
        // Never where a limit is too long to add to the clock's.
        let until = limit.and_then(|limit| Instant::now().checked_add(limit));
        match self.next(until) {
            Some(incoming) => Ok(incoming),
            None => Err(self.stopped(timeout, limit.unwrap_or_default())),
        }
    }

    /// The answer to a request, waiting at most `limit`: a message, but a page or a refusal,
    /// which break the protocol.
    fn answer(&mut self, limit: Option<Duration>, timeout: Timeout) -> Result<Reply, Error> {
        match self.receive(limit, timeout)? {
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

    /// Silent for `waited`, past `timeout`: the worker killed. One whose process has exited
    /// wasn't silent but ended, and is said so ([`Error::Crashed`]).
    fn stopped(&mut self, timeout: Timeout, waited: Duration) -> Error {
        if self.has_exited() {
            return self.crashed(self.in_page.load(Ordering::Relaxed));
        }
        self.kill();
        Error::Stopped {
            timeout,
            waited,
            stderr: self.stderr.tail(DRAIN),
        }
    }

    /// The output ended unasked, or the process exited: the worker given the grace to exit, and
    /// killed if it hasn't.
    fn crashed(&mut self, page_cut_short: bool) -> Error {
        let status = match self.end(Ending::Ended) {
            Reaped::Exited(status) => Some(status),
            Reaped::Killed => None,
        };
        Error::Crashed {
            status,
            page_cut_short,
            stderr: self.stderr.tail(DRAIN),
        }
    }

    /// The worker killed at once, and reaped.
    pub(super) fn kill(&mut self) {
        self.end(Ending::Kill);
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
        match self.end(Ending::Quit) {
            Reaped::Exited(status) if status.success() => Ok(()),
            Reaped::Exited(status) => Err(Error::Crashed {
                status: Some(status),
                page_cut_short: false,
                stderr: self.stderr.tail(DRAIN),
            }),
            Reaped::Killed => Err(Error::Stopped {
                timeout: Timeout::Exit,
                waited: self.timeouts.grace,
                stderr: self.stderr.tail(DRAIN),
            }),
        }
    }

    /// The worker ended as `ending` says, its input closed; reaped, or handed to a thread that
    /// waits for it. Every call after it answers [`Error::Ended`].
    fn end(&mut self, ending: Ending) -> Reaped {
        if ending != Ending::Quit {
            self.stdin = None;
        }
        let Some(mut process) = self.process.take() else {
            self.stdin = None;
            return Reaped::Killed;
        };
        let exited = match ending {
            Ending::Kill => None,
            Ending::Ended | Ending::Quit => wait_for(&mut process.child, self.timeouts.grace),
        };
        self.stdin = None;
        if let Some(status) = exited {
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
    in_page: Arc<AtomicBool>,
) -> io::Result<Stderr> {
    let stderr = Stderr::drain(stderr)?;
    thread::Builder::new()
        .name("scan-worker reader".to_owned())
        .spawn(move || read::read_output(stdout, limits, &sent, &in_page))?;
    Ok(stderr)
}

/// A batch's state, for the watchdog, the cancel and the model's order.
struct Batch {
    /// When the cancel was sent.
    cancelled_at: Option<Instant>,
    /// Whether the caller's receiver has gone: the rest of the batch is dropped.
    receiver_gone: bool,
    /// The total last given to the caller's progress.
    total: Option<Option<u64>>,
    /// Where the batch stands in the model's order.
    order: Order,
}

impl Batch {
    fn new(first_sheet: u32) -> Batch {
        Batch {
            cancelled_at: None,
            receiver_gone: false,
            total: None,
            order: Order::new(first_sheet),
        }
    }

    /// When the worker, silent since `since`, is killed, and which time that is:
    /// [`Timeouts::silence`] after it; and once cancelled, no later than [`Timeouts::cancel`]
    /// after the later of the cancel and `since`. `None`, never, where both are too long to
    /// add to the clock's time (a timeout of `Duration::MAX`, say).
    fn deadline(&self, since: Instant, timeouts: Timeouts) -> Option<(Instant, Timeout)> {
        let silent = since
            .checked_add(timeouts.silence)
            .map(|at| (at, Timeout::Silence));
        let cancelled = self
            .cancelled_at
            .and_then(|at| at.max(since).checked_add(timeouts.cancel))
            .map(|at| (at, Timeout::Cancel));
        match (silent, cancelled) {
            (Some(silent), Some(cancelled)) if cancelled.0 <= silent.0 => Some(cancelled),
            (Some(silent), Some(_)) => Some(silent),
            (silent, cancelled) => silent.or(cancelled),
        }
    }
}

/// Where a batch stands in the model's order ([`scan_device`]'s "A batch"): its settings applied
/// once, before the first sheet; its sheets numbered on from the first, each page of the sheet
/// begun last, a side of it once; and its pages encoded only in a form the settings asked for.
struct Order {
    applied: bool,
    /// The next sheet's number; `None` past `u32::MAX`.
    next_sheet: Option<u32>,
    /// The sheet begun last, and whether its front and its back have come.
    sheet: Option<(u32, [bool; 2])>,
    /// Whether a page has come.
    paged: bool,
}

impl Order {
    fn new(first_sheet: u32) -> Order {
        Order {
            applied: false,
            next_sheet: Some(first_sheet),
            sheet: None,
            paged: false,
        }
    }

    /// `event`, of a batch asked with `asked`, taken where it's in its place.
    fn take(&mut self, event: &Event, asked: &Settings) -> Result<(), ProtocolError> {
        match event {
            Event::Applied(applied) => {
                if self.applied || self.sheet.is_some() {
                    return Err(ProtocolError::Unexpected { got: "applied" });
                }
                check_applied(applied, asked)?;
                self.applied = true;
            }
            Event::Sheet(sheet) => {
                if !self.applied {
                    return Err(ProtocolError::Unexpected { got: "sheet" });
                }
                if self.next_sheet != Some(*sheet) {
                    return Err(ProtocolError::Invalid {
                        what: "a sheet numbered out of turn",
                    });
                }
                self.next_sheet = sheet.checked_add(1);
                self.sheet = Some((*sheet, [false; 2]));
            }
            Event::Page(page) => {
                let Some((sheet, sides)) = &mut self.sheet else {
                    return Err(ProtocolError::Unexpected { got: "page" });
                };
                if page.sheet != *sheet {
                    return Err(ProtocolError::Invalid {
                        what: "a page of another sheet than the one begun",
                    });
                }
                let side = match page.side {
                    Side::Front => 0,
                    Side::Back => 1,
                };
                if sides[side] {
                    return Err(ProtocolError::Invalid {
                        what: "a second page of one side of a sheet",
                    });
                }
                sides[side] = true;
                check_encoded(page, asked)?;
                self.paged = true;
            }
            _ => {}
        }
        Ok(())
    }
}

/// `applied`, for a batch asked with `asked`: the paper it scans finite and above 0, and each
/// setting it says it changed with what was asked of it.
fn check_applied(applied: &Applied, asked: &Settings) -> Result<(), ProtocolError> {
    if let Paper::Size(size) = applied.settings.paper
        && !is_paper(size)
    {
        return Err(ProtocolError::Invalid {
            what: "settings applied whose paper isn't finite and above 0",
        });
    }
    let as_asked = |clamped: &Clamped| match clamped {
        Clamped::Source(source) => *source == asked.source,
        Clamped::Duplex(duplex) => *duplex == asked.duplex,
        Clamped::Dpi(dpi) => *dpi == asked.dpi,
        Clamped::Format(format) => *format == asked.format,
        Clamped::Paper(paper) => *paper == asked.paper,
        Clamped::Encodings(encodings) => *encodings == asked.encodings,
        // Never read from a line: the model's own, whose variants a line can hold.
        _ => true,
    };
    if !applied.clamped.iter().all(as_asked) {
        return Err(ProtocolError::Invalid {
            what: "a setting said changed with another value than the one asked",
        });
    }
    Ok(())
}

/// `page` raw, or encoded in a form `asked` takes.
fn check_encoded(page: &RawPage, asked: &Settings) -> Result<(), ProtocolError> {
    let Pixels::Encoded(encoded) = &page.pixels else {
        return Ok(());
    };
    let encoding = match encoded.kind {
        EncodedKind::Jpeg => Some(Encoding::Jpeg),
        EncodedKind::Png => Some(Encoding::Png),
        EncodedKind::Tiff => Some(Encoding::Tiff),
        EncodedKind::Bmp => Some(Encoding::Bmp),
        EncodedKind::CcittG4 { .. } => Some(Encoding::CcittG4),
        // Never read from a line: the model's own, whose variants a line can hold.
        _ => None,
    };
    if encoding.is_some_and(|encoding| asked.encodings.contains(&encoding)) {
        Ok(())
    } else {
        Err(ProtocolError::Invalid {
            what: "a page encoded in a form the settings didn't ask for",
        })
    }
}

/// `request`'s line, if it can cross: no longer than a worker reads, and coming back from it as
/// it went, which a size that isn't finite doesn't (JSON writes it `null`).
fn sendable(request: &Request) -> Result<Vec<u8>, Error> {
    let line = wire::line(request).map_err(|_| Error::Unsendable)?;
    let json = line.strip_suffix(b"\n").unwrap_or(&line);
    if json.len() > REQUEST_LIMIT {
        return Err(Error::Unsendable);
    }
    match serde_json::from_slice::<Request>(json) {
        Ok(back) if back == *request => Ok(line),
        _ => Err(Error::Unsendable),
    }
}

/// An open's answer, for `stored`: the device of the stored one's driver API; and either the
/// stored device itself (its id, nothing said renamed), or another in its place, said renamed
/// from the stored id.
fn check_opened(
    stored: &DeviceInfo,
    info: &DeviceInfo,
    renamed_from: Option<&DeviceId>,
) -> Result<(), ProtocolError> {
    let invalid = |what| Err(ProtocolError::Invalid { what });
    if info.driver != stored.driver {
        return invalid("an opened device of another driver API than the stored one's");
    }
    match renamed_from {
        None if info.id != stored.id => {
            invalid("an opened device of another id than the stored one's, not said renamed")
        }
        Some(renamed) if *renamed != stored.id => {
            invalid("an opened device said renamed from another id than the stored one's")
        }
        Some(_) if info.id == stored.id => {
            invalid("the stored device opened, said renamed from its own id")
        }
        _ => Ok(()),
    }
}

/// Whether a size's width and height are finite and above 0, as a paper's are.
fn is_paper(size: SizeMm) -> bool {
    [size.width, size.height]
        .iter()
        .all(|mm| mm.is_finite() && *mm > 0.0)
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
            Resolutions::List(values) if values.contains(&0) => {
                return invalid("a source whose list of resolutions holds 0");
            }
            Resolutions::Range { min: 0, .. } => {
                return invalid("a source whose range of resolutions starts at 0");
            }
            Resolutions::Range { min, max, .. } if min > max => {
                return invalid("a source whose range of resolutions has its min past its max");
            }
            _ => {}
        }
        if source.max_area.is_some_and(|area| !is_paper(area)) {
            return invalid("a source whose largest paper isn't finite and above 0");
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

    use scan_device::{Encoded, Format, RawImage, SideKnown, Source, SourceCaps};

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
    fn a_request_longer_than_a_worker_reads_is_unsendable() {
        let open = |name_bytes: usize| Request::Open {
            device: DeviceInfo::new(
                DeviceId("fake:desk".to_owned()),
                "n".repeat(name_bytes),
                "Maker".to_owned(),
                "Model".to_owned(),
                Driver::Fake,
            ),
            deadline: Duration::from_secs(5),
        };
        // The line's length beside its name's.
        let around = sendable(&open(0)).expect("sendable").len() - 1;
        let at_the_cap = sendable(&open(REQUEST_LIMIT - around)).expect("at the cap");
        assert_eq!(at_the_cap.len(), REQUEST_LIMIT + 1);
        assert!(matches!(
            sendable(&open(REQUEST_LIMIT - around + 1)),
            Err(Error::Unsendable)
        ));
        assert!(matches!(sendable(&open(2 << 20)), Err(Error::Unsendable)));
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
        let with_area = |width, height| {
            let mut caps = caps(range(50, 600), vec![Format::Bw1]);
            caps.sources[0].max_area = Some(SizeMm { width, height });
            caps
        };
        assert_eq!(check_caps(&with_area(215.9, 355.6)), Ok(()));
        assert_eq!(check_caps(&with_area(f32::MIN_POSITIVE, 1.0)), Ok(()));
        let no_paper = "a source whose largest paper isn't finite and above 0";
        for (refused, what) in [
            (Caps::new(Vec::new()), "caps with no source"),
            (caps(range(50, 600), Vec::new()), "a source with no format"),
            (
                caps(Resolutions::List(Vec::new()), vec![Format::Bw1]),
                "a source with an empty list of resolutions",
            ),
            (
                caps(Resolutions::List(vec![300, 0]), vec![Format::Bw1]),
                "a source whose list of resolutions holds 0",
            ),
            (
                caps(range(0, 600), vec![Format::Bw1]),
                "a source whose range of resolutions starts at 0",
            ),
            (
                caps(range(601, 600), vec![Format::Bw1]),
                "a source whose range of resolutions has its min past its max",
            ),
            (with_area(0.0, 297.0), no_paper),
            (with_area(210.0, -297.0), no_paper),
            (with_area(-0.0, 297.0), no_paper),
            (with_area(f32::INFINITY, 297.0), no_paper),
            (with_area(210.0, f32::NAN), no_paper),
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
        let mut batch = Batch::new(1);
        assert_eq!(
            batch.deadline(since, timeouts),
            Some((since + Duration::from_secs(60), Timeout::Silence))
        );
        // Cancelled after the last line: counted from the cancel.
        let at = since + Duration::from_secs(5);
        batch.cancelled_at = Some(at);
        assert_eq!(
            batch.deadline(since, timeouts),
            Some((at + Duration::from_secs(10), Timeout::Cancel))
        );
        // A line after the cancel: counted from the line.
        let later = since + Duration::from_secs(8);
        assert_eq!(
            batch.deadline(later, timeouts),
            Some((later + Duration::from_secs(10), Timeout::Cancel))
        );
        // Never past the silence's.
        let long = Timeouts {
            cancel: Duration::from_secs(600),
            ..timeouts
        };
        assert_eq!(
            batch.deadline(since, long),
            Some((since + Duration::from_secs(60), Timeout::Silence))
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
            Some((at + Duration::from_secs(10), Timeout::Cancel))
        );
    }

    fn desk(id: &str) -> DeviceInfo {
        DeviceInfo::new(
            DeviceId(id.to_owned()),
            "Maker Model".to_owned(),
            "Maker".to_owned(),
            "Model".to_owned(),
            Driver::Fake,
        )
    }

    #[test]
    fn an_open_gives_the_stored_device_or_one_said_renamed_from_it() {
        let stored = desk("fake:desk");
        let other = desk("fake:libusb:001:005");
        let stored_id = Some(&stored.id);
        assert_eq!(check_opened(&stored, &stored, None), Ok(()));
        assert_eq!(check_opened(&stored, &other, stored_id), Ok(()));
        let mut sane = stored.clone();
        sane.driver = Driver::Sane;
        for (info, renamed_from, what) in [
            (
                &sane,
                None,
                "an opened device of another driver API than the stored one's",
            ),
            (
                &other,
                None,
                "an opened device of another id than the stored one's, not said renamed",
            ),
            (
                &other,
                Some(&other.id),
                "an opened device said renamed from another id than the stored one's",
            ),
            (
                &stored,
                stored_id,
                "the stored device opened, said renamed from its own id",
            ),
        ] {
            assert_eq!(
                check_opened(&stored, info, renamed_from),
                Err(ProtocolError::Invalid { what }),
                "{info:?} {renamed_from:?}"
            );
        }
    }

    /// A raw page of `sheet`'s `side`.
    fn page(sheet: u32, side: Side) -> Event {
        let image = RawImage::new(Format::Gray8, 1, 1, vec![0]).unwrap();
        Event::Page(RawPage::new(
            Pixels::Raw(image),
            (300.0, 300.0),
            sheet,
            side,
            SideKnown::Inferred,
        ))
    }

    /// An encoded page of sheet 1's front, of `kind`.
    fn encoded(kind: EncodedKind) -> Event {
        Event::Page(RawPage::new(
            Pixels::Encoded(Encoded {
                kind,
                bytes: b"JPEG".to_vec(),
            }),
            (300.0, 300.0),
            1,
            Side::Front,
            SideKnown::Inferred,
        ))
    }

    /// What `events`, a batch asked with `asked` from `first_sheet`, break, if they break
    /// anything.
    fn order(first_sheet: u32, asked: &Settings, events: &[Event]) -> Result<(), ProtocolError> {
        let mut order = Order::new(first_sheet);
        events.iter().try_for_each(|event| order.take(event, asked))
    }

    #[test]
    fn a_batch_s_events_come_in_the_model_s_order() {
        let asked = Settings::new(Source::Feeder, 300, Format::Gray8);
        let applied = || Event::Applied(Applied::new(asked.clone(), Vec::new()));
        let duplex = [
            applied(),
            Event::Sheet(7),
            page(7, Side::Front),
            page(7, Side::Back),
            Event::Sheet(8),
            // A sheet whose side failed, then its back alone.
            Event::Sheet(9),
            page(9, Side::Back),
        ];
        assert_eq!(order(7, &asked, &duplex), Ok(()));
        // Up to the last sheet there is.
        let last = [
            applied(),
            Event::Sheet(u32::MAX),
            page(u32::MAX, Side::Front),
        ];
        assert_eq!(order(u32::MAX, &asked, &last), Ok(()));
        let unexpected = |got| Err(ProtocolError::Unexpected { got });
        let invalid = |what| Err(ProtocolError::Invalid { what });
        for (events, broken) in [
            (vec![Event::Sheet(1)], unexpected("sheet")),
            (vec![applied(), page(1, Side::Front)], unexpected("page")),
            (vec![applied(), applied()], unexpected("applied")),
            (
                vec![applied(), Event::Sheet(1), applied()],
                unexpected("applied"),
            ),
            (
                vec![applied(), Event::Sheet(2)],
                invalid("a sheet numbered out of turn"),
            ),
            (
                vec![applied(), Event::Sheet(1), Event::Sheet(1)],
                invalid("a sheet numbered out of turn"),
            ),
            (
                vec![applied(), Event::Sheet(1), Event::Sheet(3)],
                invalid("a sheet numbered out of turn"),
            ),
            (
                vec![applied(), Event::Sheet(1), page(2, Side::Front)],
                invalid("a page of another sheet than the one begun"),
            ),
            (
                vec![
                    applied(),
                    Event::Sheet(1),
                    page(1, Side::Back),
                    page(1, Side::Back),
                ],
                invalid("a second page of one side of a sheet"),
            ),
        ] {
            assert_eq!(order(1, &asked, &events), broken, "{events:?}");
        }
        let past_the_last = [applied(), Event::Sheet(u32::MAX), Event::Sheet(0)];
        assert_eq!(
            order(u32::MAX, &asked, &past_the_last),
            invalid("a sheet numbered out of turn")
        );
    }

    #[test]
    fn a_page_comes_encoded_only_in_a_form_asked_for() {
        let mut asked = Settings::new(Source::Feeder, 300, Format::Bw1);
        let applied = Event::Applied(Applied::new(asked.clone(), Vec::new()));
        let g4 = EncodedKind::CcittG4 {
            width: 1,
            height: 1,
            black_is_1: true,
        };
        let unasked = Err(ProtocolError::Invalid {
            what: "a page encoded in a form the settings didn't ask for",
        });
        for kind in [EncodedKind::Jpeg, EncodedKind::Png, g4] {
            let events = [applied.clone(), Event::Sheet(1), encoded(kind)];
            assert_eq!(order(1, &asked, &events), unasked, "{kind:?}");
        }
        asked.encodings = vec![Encoding::CcittG4, Encoding::Png];
        for kind in [EncodedKind::Png, g4] {
            let events = [applied.clone(), Event::Sheet(1), encoded(kind)];
            assert_eq!(order(1, &asked, &events), Ok(()), "{kind:?}");
        }
        let events = [applied, Event::Sheet(1), encoded(EncodedKind::Jpeg)];
        assert_eq!(order(1, &asked, &events), unasked);
    }

    #[test]
    fn settings_applied_scan_a_paper_and_say_what_was_asked() {
        let mut asked = Settings::new(Source::Feeder, 300, Format::Gray8);
        asked.paper = Paper::Size(SizeMm::A4);
        let mut took = asked.clone();
        took.dpi = 200;
        took.paper = Paper::Size(SizeMm::LETTER);
        let clamped = vec![Clamped::Dpi(300), Clamped::Paper(Paper::Size(SizeMm::A4))];
        assert_eq!(
            check_applied(&Applied::new(took.clone(), clamped), &asked),
            Ok(())
        );
        let no_paper = Err(ProtocolError::Invalid {
            what: "settings applied whose paper isn't finite and above 0",
        });
        for size in [
            SizeMm {
                width: f32::INFINITY,
                height: 297.0,
            },
            SizeMm {
                width: 210.0,
                height: 0.0,
            },
            SizeMm {
                width: -210.0,
                height: 297.0,
            },
        ] {
            took.paper = Paper::Size(size);
            assert_eq!(
                check_applied(&Applied::new(took.clone(), Vec::new()), &asked),
                no_paper,
                "{size:?}"
            );
        }
        took.paper = Paper::Auto;
        let not_asked = Err(ProtocolError::Invalid {
            what: "a setting said changed with another value than the one asked",
        });
        for clamped in [
            Clamped::Dpi(600),
            Clamped::Source(Source::Flatbed),
            Clamped::Duplex(true),
            Clamped::Format(Format::Rgb8),
            Clamped::Paper(Paper::Auto),
            Clamped::Paper(Paper::Size(SizeMm {
                width: f32::INFINITY,
                height: 297.0,
            })),
            Clamped::Encodings(vec![Encoding::Png]),
        ] {
            assert_eq!(
                check_applied(&Applied::new(took.clone(), vec![clamped.clone()]), &asked),
                not_asked,
                "{clamped:?}"
            );
        }
    }
}
