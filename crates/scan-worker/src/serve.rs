//! The worker's side: requests read, the backends' calls made in order on one thread, and their
//! answers and a batch's events written.

use std::{
    io::{self, BufReader, Read, Write},
    process::{self, ExitCode},
    sync::{
        Arc, Mutex, MutexGuard, PoisonError,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, RecvTimeoutError, Sender},
    },
    thread,
    time::{Duration, Instant},
};

use scan_device::in_progress::{Tracker, Unit};
use scan_device::{
    Backend, Device, DeviceInfo, Event, Pixels, RawPage, ScanError, Settings, WindowOwner,
};

use crate::wire::{self, Line, PROTOCOL, PageHeader, PixelsHeader, REQUEST_LIMIT, Reply, Request};

/// How long a worker whose input has ended waits for its call in progress to return before it
/// aborts.
const ORPHAN_GRACE: Duration = Duration::from_secs(10);

/// How long a batch's events may pause before its progress is read anyway.
const PROGRESS_POLL: Duration = Duration::from_millis(100);

/// Serves `backends` to the client at the other end of the process's standard input and
/// output, until it says `quit` or its input ends: a worker binary's whole `main`.
///
/// The process's standard input and output are the protocol's (the crate's docs, "The
/// protocol"), so nothing else in the process may write to standard output: a line the client
/// didn't expect is a protocol error to it. Standard error is free for a log, which a client can
/// keep the end of for the errors that end a worker.
///
/// Every call on a backend or the device it opens is made on the calling thread, in the order
/// the requests came, so a driver that must be called from the thread that opened it is. The
/// backends are all one driver API's, since a client starts a worker for each: a listing lists
/// each in turn and joins their devices, and an open goes to each in turn until one doesn't
/// answer [`ScanError::BadId`].
///
/// It returns once the device and the backends are dropped (closing the driver): with success
/// after `quit` or the end of its input, which also cancels a running batch; with 1 where its
/// output failed, the client gone, which ends a running batch at its next event. A client gone
/// while a driver's call never returns leaves nothing to answer, so 10 seconds after its input
/// ends, if that call still hasn't returned, the worker ends its process with
/// [`std::process::abort`], which runs no exit handler beside the stuck driver: a worker never
/// outlives its client by more than that, and the system may keep a crash report or a core of
/// it. A backend's panic isn't caught: the worker ends as a panicking program does, its message
/// on standard error.
///
/// ```no_run
/// use std::process::ExitCode;
///
/// use scan_device::fake::FakeBackend;
///
/// // A worker binary: here with the scripted backend, where an app's has its drivers'.
/// fn main() -> ExitCode {
///     scan_worker::serve(vec![Box::new(FakeBackend::new())])
/// }
/// ```
pub fn serve(backends: Vec<Box<dyn Backend>>) -> ExitCode {
    match run(backends, io::stdin(), io::stdout(), Some(ORPHAN_GRACE)) {
        Exit::Quit | Exit::InputEnded => ExitCode::SUCCESS,
        Exit::OutputFailed => ExitCode::from(1),
    }
}

/// How [`run`] ended, which `serve` exits by.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Exit {
    /// The client said `quit`.
    Quit,
    /// The input ended: the client is gone.
    InputEnded,
    /// A write failed: the client is gone.
    OutputFailed,
}

/// [`serve`]'s core, over any input and output: it returns where `serve` ends the process.
///
/// `orphaned`, where given, is how long after the input ends the reader waits for this call to
/// return before it aborts the process; `None`, in tests, never aborts.
pub(crate) fn run(
    backends: Vec<Box<dyn Backend>>,
    input: impl Read + Send + 'static,
    output: impl Write + Send + 'static,
    orphaned: Option<Duration>,
) -> Exit {
    let out = Arc::new(Output::new(output));
    // First, before any request is read: the reader may write refusals.
    let hello = Reply::Hello {
        protocol: PROTOCOL,
        version: env!("CARGO_PKG_VERSION").to_owned(),
    };
    if out.send(&hello).is_err() {
        out.close();
        return Exit::OutputFailed;
    }
    let state = Arc::new(Mutex::new(State::default()));
    let (commands, received) = mpsc::channel();
    // Held until this call returns: the reader's wait for it, once the input has ended.
    let (alive, returned) = mpsc::channel::<()>();
    {
        let out = Arc::clone(&out);
        let state = Arc::clone(&state);
        thread::spawn(move || {
            read_requests(input, &out, &state, &commands);
            if let Some(grace) = orphaned
                && returned.recv_timeout(grace) == Err(RecvTimeoutError::Timeout)
            {
                process::abort();
            }
        });
    }
    let mut worker = Worker {
        backends,
        device: None,
        out: Arc::clone(&out),
    };
    let ended = loop {
        // The reader gone without a word is the input's end.
        let command = received.recv().unwrap_or(Command::InputEnded);
        let call = match command {
            Command::Call(call) => call,
            Command::Quit => break Exit::Quit,
            Command::InputEnded => break Exit::InputEnded,
        };
        let answer = worker.call(call);
        // Before the answer, so a request the client sends once it has it is taken.
        lock(&state).finish(worker.device.is_some());
        if out.send(&answer).is_err() {
            break Exit::OutputFailed;
        }
    };
    // The driver closed before the client sees the output end.
    drop(worker);
    out.close();
    drop(alive);
    if out.failed() {
        Exit::OutputFailed
    } else {
        ended
    }
}

/// What the reader hands the main thread.
enum Command {
    /// A call to make, taken in the state the worker is in.
    Call(Call),
    /// `quit`: a running batch is cancelled already.
    Quit,
    /// The input ended: a running batch is cancelled already.
    InputEnded,
}

/// A call on a backend or the device, made on the main thread.
enum Call {
    List {
        deadline: Duration,
    },
    Open {
        device: DeviceInfo,
        deadline: Duration,
    },
    Caps,
    Scan {
        settings: Settings,
        first_sheet: u32,
        /// Made by the reader as it read `scan`, so a `cancel` read after it finds it.
        tracker: Tracker,
    },
    SettingsWindow {
        owner: Option<WindowOwner>,
    },
}

/// What the reader and the main thread share: whether a call runs, whether a device is open,
/// and a running batch's tracker.
#[derive(Default)]
struct State {
    busy: bool,
    open: bool,
    batch: Option<Tracker>,
}

impl State {
    /// `request` taken or refused, as the crate's docs say ("What a worker takes, and when"):
    /// the command for the main thread, if there's one, or why it's refused.
    fn admit(&mut self, request: Request) -> Result<Option<Command>, String> {
        let call = match request {
            Request::Cancel => {
                self.cancel();
                return Ok(None);
            }
            Request::Quit => {
                self.cancel();
                return Ok(Some(Command::Quit));
            }
            _ if self.busy => {
                return Err("a call is running, and a worker takes one at a time".to_owned());
            }
            Request::List { deadline } if !self.open => Call::List { deadline },
            Request::Open { device, deadline } if !self.open => Call::Open { device, deadline },
            Request::List { .. } | Request::Open { .. } => {
                return Err(
                    "a device is open, and a worker opens one and lists none beside it".to_owned(),
                );
            }
            Request::Caps if self.open => Call::Caps,
            Request::Scan {
                settings,
                first_sheet,
            } if self.open => {
                let tracker = Tracker::new();
                self.batch = Some(tracker.clone());
                Call::Scan {
                    settings,
                    first_sheet,
                    tracker,
                }
            }
            Request::SettingsWindow { owner } if self.open => Call::SettingsWindow { owner },
            Request::Caps | Request::Scan { .. } | Request::SettingsWindow { .. } => {
                return Err("no device is open".to_owned());
            }
        };
        self.busy = true;
        Ok(Some(Command::Call(call)))
    }

    /// The running batch cancelled, if there's one.
    fn cancel(&self) {
        if let Some(batch) = &self.batch {
            batch.cancel();
        }
    }

    /// A call finished, and whether a device is open after it.
    fn finish(&mut self, open: bool) {
        self.busy = false;
        self.open = open;
        self.batch = None;
    }
}

/// The reader: each request read under the cap, taken or refused, until the input ends, which
/// cancels a running batch and tells the main thread.
fn read_requests(input: impl Read, out: &Output, state: &Mutex<State>, commands: &Sender<Command>) {
    let mut input = BufReader::new(input);
    let mut line = Vec::new();
    // A failed refusal leaves the output failed, which the main thread reports as it ends.
    let refuse = |reason: String| {
        let _ = out.send(&Reply::Refused { reason });
    };
    loop {
        match wire::read_line(&mut input, REQUEST_LIMIT, &mut line) {
            Ok(Line::Read) => {}
            Ok(Line::TooLong) => {
                // Read through, never held; an input that fails there is as gone as one that
                // ends.
                if wire::skip_line(&mut input).is_err() {
                    break;
                }
                refuse(format!("a line past the {REQUEST_LIMIT}-byte cap"));
                continue;
            }
            // An input that fails is as gone as one that ends.
            Ok(Line::End) | Err(_) => break,
        }
        let request = match serde_json::from_slice::<Request>(&line) {
            Ok(request) => request,
            Err(error) => {
                refuse(format!("an unreadable request: {error}"));
                continue;
            }
        };
        let admitted = lock(state).admit(request);
        match admitted {
            // The main thread gone has ended the worker already.
            Ok(Some(command)) => {
                let _ = commands.send(command);
            }
            Ok(None) => {}
            Err(reason) => refuse(reason),
        }
    }
    lock(state).cancel();
    let _ = commands.send(Command::InputEnded);
}

/// The main thread's side: the backends, and the device one opened.
struct Worker {
    backends: Vec<Box<dyn Backend>>,
    device: Option<Box<dyn Device>>,
    out: Arc<Output>,
}

impl Worker {
    /// `call` made, and its answer.
    fn call(&mut self, call: Call) -> Reply {
        let answer = match call {
            Call::List { deadline } => self.list(deadline).map(Reply::Devices),
            Call::Open { device, deadline } => self.open(&device, deadline),
            Call::Caps => self.on_device(|device| device.caps().map(Reply::Caps)),
            Call::Scan {
                settings,
                first_sheet,
                tracker,
            } => {
                let out = Arc::clone(&self.out);
                self.on_device(|device| scan(device, &out, &settings, first_sheet, &tracker))
            }
            Call::SettingsWindow { owner } => {
                self.on_device(|device| device.settings_window(owner).map(Reply::Window))
            }
        };
        answer.unwrap_or_else(Reply::Error)
    }

    /// `call` on the open device. The reader takes no call on a device without one, so the
    /// refusal is never written but by a mistake here.
    fn on_device(
        &mut self,
        call: impl FnOnce(&mut dyn Device) -> Result<Reply, ScanError>,
    ) -> Result<Reply, ScanError> {
        match self.device.as_deref_mut() {
            Some(device) => call(device),
            None => Ok(Reply::Refused {
                reason: "no device is open".to_owned(),
            }),
        }
    }

    /// Every backend's devices in turn, each given what's left of the deadline; the first
    /// error ends it.
    fn list(&mut self, deadline: Duration) -> Result<Vec<DeviceInfo>, ScanError> {
        let start = Instant::now();
        let mut devices = Vec::new();
        for backend in &mut self.backends {
            let left = deadline.saturating_sub(start.elapsed());
            devices.extend(backend.list(left)?);
        }
        Ok(devices)
    }

    /// The stored device opened by the first backend that doesn't answer `BadId`, each given
    /// what's left of the deadline.
    fn open(&mut self, stored: &DeviceInfo, deadline: Duration) -> Result<Reply, ScanError> {
        let start = Instant::now();
        for backend in &mut self.backends {
            let left = deadline.saturating_sub(start.elapsed());
            match backend.open(stored, left) {
                Err(ScanError::BadId) => {}
                Err(error) => return Err(error),
                Ok(opened) => {
                    self.device = Some(opened.device);
                    return Ok(Reply::Opened {
                        info: opened.info,
                        renamed_from: opened.renamed_from,
                    });
                }
            }
        }
        Err(ScanError::BadId)
    }
}

/// A batch on `device`, its events drained to `out` as it goes; its end once every event is
/// written.
fn scan(
    device: &mut dyn Device,
    out: &Arc<Output>,
    settings: &Settings,
    first_sheet: u32,
    tracker: &Tracker,
) -> Result<Reply, ScanError> {
    // A rendezvous: each event is taken as it's sent, so one is held at the backend's `send`
    // while one is written, and a slow pipe holds the backend back.
    let (events, taken) = mpsc::sync_channel(0);
    let drainer = {
        let out = Arc::clone(out);
        let tracker = tracker.clone();
        thread::spawn(move || drain(&taken, &out, &tracker))
    };
    let ended = device.scan(settings, first_sheet, &events, tracker);
    drop(events);
    if let Err(panic) = drainer.join() {
        std::panic::resume_unwind(panic);
    }
    ended.map(Reply::Done)
}

/// A batch's events written as they come, and its progress after each and at each pause, until
/// the batch's channel closes; or until a write fails, which drops the receiver, so the batch
/// ends at its next `send`.
fn drain(events: &Receiver<Event>, out: &Output, tracker: &Tracker) {
    let mut seen = Seen::default();
    loop {
        match events.recv_timeout(PROGRESS_POLL) {
            Ok(event) => {
                if write_event(out, event).is_err() {
                    return;
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => {
                // The last of it: the main thread's answer reports a failure.
                let _ = seen.poll(out, tracker);
                return;
            }
        }
        if seen.poll(out, tracker).is_err() {
            return;
        }
    }
}

/// An event's line, and a page's bytes after it.
fn write_event(out: &Output, event: Event) -> io::Result<()> {
    match event {
        Event::Applied(applied) => out.send(&Reply::Applied(applied)),
        Event::Sheet(sheet) => out.send(&Reply::Sheet(sheet)),
        Event::Page(page) => write_page(out, &page),
        Event::Status(status) => out.send(&Reply::Status(status)),
        // An event added to the model and not to the protocol: loud at the client, not lost.
        _ => out.send(&Reply::Refused {
            reason: "an event the protocol doesn't carry".to_owned(),
        }),
    }
}

/// A page's line, then its bytes.
fn write_page(out: &Output, page: &RawPage) -> io::Result<()> {
    let (pixels, bytes) = match &page.pixels {
        Pixels::Raw(image) => (
            PixelsHeader::Raw {
                format: image.format(),
                width: image.width(),
                height: image.height(),
            },
            image.data(),
        ),
        Pixels::Encoded(encoded) => (
            PixelsHeader::Encoded { kind: encoded.kind },
            &encoded.bytes[..],
        ),
        _ => {
            return out.send(&Reply::Refused {
                reason: "a page's pixels in a form the protocol doesn't carry".to_owned(),
            });
        }
    };
    let header = PageHeader {
        pixels,
        dpi: page.dpi,
        sheet: page.sheet,
        side: page.side,
        side_known: page.side_known,
        bytes: bytes.len() as u64,
    };
    out.send_with(&Reply::Page(header), bytes)
}

/// The progress last written, and the messages counted, of a batch.
#[derive(Default)]
struct Seen {
    done: u64,
    total: Option<u64>,
    messages: u64,
}

impl Seen {
    /// A `progress` line where the bytes done or the total changed, and a `message` line where
    /// a message came: the latest, those before it since the last poll lost.
    fn poll(&mut self, out: &Output, tracker: &Tracker) -> io::Result<()> {
        let snapshot = tracker.snapshot();
        // Bytes only: a step counted in another unit isn't the model's, and isn't carried.
        if snapshot.unit == Unit::Bytes
            && (snapshot.done, snapshot.total) != (self.done, self.total)
        {
            self.done = snapshot.done;
            self.total = snapshot.total;
            out.send(&Reply::Progress {
                done: self.done,
                total: self.total,
            })?;
        }
        if snapshot.message_count != self.messages {
            self.messages = snapshot.message_count;
            if let Some((level, text)) = snapshot.message {
                out.send(&Reply::Message {
                    level: level.into(),
                    text,
                })?;
            }
        }
        Ok(())
    }
}

/// The worker's output: one writer, locked for each message (a page's line and its bytes under
/// one lock) and flushed after each. A write that fails drops the writer, so nothing follows a
/// message cut short, and every later one fails at once.
struct Output {
    writer: Mutex<Option<Box<dyn Write + Send>>>,
    failed: AtomicBool,
}

impl Output {
    fn new(writer: impl Write + Send + 'static) -> Output {
        Output {
            writer: Mutex::new(Some(Box::new(writer))),
            failed: AtomicBool::new(false),
        }
    }

    /// `reply`'s line.
    fn send(&self, reply: &Reply) -> io::Result<()> {
        self.send_with(reply, &[])
    }

    /// `reply`'s line, then `bytes`.
    fn send_with(&self, reply: &Reply, bytes: &[u8]) -> io::Result<()> {
        let line = wire::line(reply)?;
        let mut writer = lock(&self.writer);
        let Some(open) = writer.as_mut() else {
            return Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "the worker's output is closed",
            ));
        };
        let written = open
            .write_all(&line)
            .and_then(|()| open.write_all(bytes))
            .and_then(|()| open.flush());
        if written.is_err() {
            *writer = None;
            self.failed.store(true, Ordering::Relaxed);
        }
        written
    }

    /// The writer dropped, so the client's reading ends.
    fn close(&self) {
        *lock(&self.writer) = None;
    }

    /// Whether a write failed.
    fn failed(&self) -> bool {
        self.failed.load(Ordering::Relaxed)
    }
}

/// `mutex` locked, whatever thread panicked holding it: what's behind each is whole between
/// statements.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests;
