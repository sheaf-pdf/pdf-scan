//! Scanner drivers run in a process of their own: a worker serving any [`scan_device`] backends
//! over its standard input and output, the client that starts one for each device, and the
//! protocol between them.
//!
//! A scanner's driver is code its maker wrote, loaded into whatever process calls it: a driver
//! that hangs holds that process's thread for good, one that crashes takes the process with it,
//! and a 32-bit driver loads only into a 32-bit process. Run in a worker process of its own, a
//! driver can hang, crash or need another bitness while the app goes on: the app keeps the
//! pages the worker sent before it failed, ends a worker that went silent, and starts another.
//!
//! Two halves, which ship together:
//!
//! - **The worker's side** is [`serve`], the whole `main` of a worker binary, given the backends
//!   it serves: every call on them made on its main thread, in order; a batch's events written as
//!   they come, each page's bytes after its line, through a channel that holds the backend back
//!   while the pipe is slow; a batch's progress and its driver's messages forwarded; a cancel
//!   taken at once.
//! - **The app's side** is a [`Client`], given the worker to start for each driver API: it makes
//!   the device model's calls through a worker, a [`RemoteDevice`] for each device it opens, with
//!   an [`Error`] of its own that holds the device's [`ScanError`](scan_device::ScanError) beside
//!   the worker's failures.
//!
//! ```no_run
//! use std::{process::ExitCode, sync::mpsc, thread, time::Duration};
//!
//! use scan_worker::scan_device::fake::{FakeBackend, FakeDevice, Sheet};
//! use scan_worker::scan_device::in_progress::Tracker;
//! use scan_worker::scan_device::{Driver, Event, Format, Settings, Source};
//! use scan_worker::{Client, WorkerCommand};
//!
//! fn main() -> ExitCode {
//!     // Run as a worker: the scripted backend, where an app's has its drivers'.
//!     if std::env::args().nth(1).as_deref() == Some("--scan-worker") {
//!         let scanner = FakeDevice::new("desk", "Maker", "Model");
//!         scanner.load([Sheet::new(), Sheet::new()]);
//!         return scan_worker::serve(vec![Box::new(FakeBackend::new().with(scanner))]);
//!     }
//!     // Run as the app, with itself as its worker.
//!     let me = std::env::current_exe().expect("the program's path");
//!     let client = Client::new().worker(Driver::Fake, WorkerCommand::new(me).arg("--scan-worker"));
//!     let stored = client.list(Driver::Fake, Duration::from_secs(5)).expect("a listing").remove(0);
//!     let mut device = client.open(&stored, Duration::from_secs(5)).expect("the device");
//!     let (events, received) = mpsc::sync_channel(2);
//!     let pages = thread::spawn(move || {
//!         received.iter().filter(|event| matches!(event, Event::Page(_))).count()
//!     });
//!     let settings = Settings::new(Source::Feeder, 300, Format::Gray8);
//!     let done = device.scan(&settings, 1, &events, &Tracker::new()).expect("a batch");
//!     drop(events);
//!     println!("{done:?}: {} pages", pages.join().expect("the pages"));
//!     device.close().expect("the device closed");
//!     ExitCode::SUCCESS
//! }
//! ```
//!
//! A SANE worker is built the same way with this crate's `sane` feature, which turns on
//! `scan-device`'s: its `main` serves `scan_worker::scan_device::sane::SaneBackend::new()` where
//! this one serves the scripted backend, and the client is given it for `Driver::Sane`. On
//! Windows, a 64-bit and a 32-bit TWAIN worker are built with the `twain` feature, each serving
//! `scan_worker::scan_device::twain::TwainBackend::new()`, registered for
//! `Driver::Twain { bits: 64 }` and `Driver::Twain { bits: 32 }`, and listed together through
//! [`Client::list_twain`] ([`TwainList`]). Named through the re-export, each backend is one of
//! the `scan-device` this crate's [`serve`] takes; a `scan-device` of another version, a
//! dependency of the app's own, is another crate, whose backends `serve` doesn't take.
//!
//! # The client
//!
//! A listing ([`Client::list`], or [`Client::list_twain`] across both TWAIN bitnesses in
//! parallel) starts a worker of its own, which lists, answers and is quit, so a slow listing
//! never holds a device that scans; an open ([`Client::open`]) starts a worker that keeps the
//! device until [`RemoteDevice::close`], or its drop, asks it to quit. Each is the worker given
//! for the device's [`Driver`](scan_device::Driver) ([`Client::worker`]), a program named by an
//! absolute path, never searched for.
//!
//! Each worker starts in a private directory of its own, made under the work directory
//! ([`Client::work_dir`], the system's temporary directory by default) at once under a random
//! name, on Unix its owner's alone, and removed once the worker has exited. A SANE worker that
//! opens a stored device finds there a SANE configuration naming only the backend the stored id
//! names (the id's text after `sane:` up to its first `:`, each character up to U+00FF a byte;
//! none where the id gives none), which SANE reads first (`SANE_CONFIG_DIR`), so that backend
//! alone loads, whatever else SANE reads; then the directories a listing's worker reads, so each
//! backend finds the same configuration of its own as it did when it listed the device: the
//! `SANE_CONFIG_DIR` the worker's command gives, else the app's, else SANE's own list (the
//! worker's current directory, the same one, then the system's). A listing's directory is empty,
//! and its worker reads what the command or the app gives, or SANE's own list. Whatever the id,
//! the worker's backend is the one to refuse it. On Unix a worker leads a process group of its
//! own, so a terminal's interrupt reaches the app, which decides, and not the worker mid-sheet;
//! on Windows it's started with no console window and placed in a Job Object
//! (`JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`), so any helper processes a driver spawns end with the
//! worker and never outlive the client.
//!
//! A worker inherits the app's environment, with `LC_ALL=C` set first on SANE workers so decimal
//! option formatting is locale-independent; its command's variables are set after it, in order
//! ([`WorkerCommand::env`], which can override `LC_ALL` too), then a SANE open's
//! `SANE_CONFIG_DIR`: so an app gives a worker what its process reads as it starts, such as the
//! system loader's search path that chooses the SANE library it loads, and the configuration of
//! an app's own SANE, without changing its own environment. The client sets each as given, the
//! app's own choice, as its program is; a search path holds absolute entries alone.
//!
//! The client trusts nothing a worker says, since a driver's code runs there: every line is
//! capped before it's read whole ([`Limits::line`]); a page's bytes are capped
//! ([`Limits::page`]) and, raw, checked against its format and size before anything is
//! allocated, then made a page through [`RawImage::new`](scan_device::RawImage::new), which
//! checks them again; a page's `dpi` must be finite and above 0; caps must keep the model's
//! rules (a source at least, each with a format; a list of resolutions not empty and holding no
//! 0, a range's `min` above 0 and not past its `max`; a largest paper finite and above 0); a
//! device listed or opened must be of the worker's driver API, and an open must give the stored
//! device, or another said renamed from the stored id; a batch must keep the model's order (its
//! settings applied once, before its first sheet, with the paper finite and above 0 and each
//! setting said changed with what was asked of it; its sheets numbered on from the first; each
//! page of the sheet begun last, a side of it once, raw or encoded in a form the settings asked
//! for; and the empty feeder an error only before any page); and each message must come in its
//! place. A JSON number past an `f32`'s range reads as an infinity, so sizes are checked for
//! being finite as they're read, not taken as finite for being JSON. A worker that breaks one of
//! these is killed ([`Error::Protocol`]): it can't be trusted to be in step.
//!
//! A request is written only if it can cross: its line no longer than a worker reads (the
//! protocol's 1 MiB), and reading back from it as it was, which a paper size that isn't finite
//! doesn't (JSON writes it `null`). One that can't is [`Error::Unsendable`], nothing sent.
//!
//! On Unix, a write to a pipe whose reader has gone raises `SIGPIPE`, so a worker that ends,
//! or closes its input, just before the client writes to it would end an app that takes the
//! signal's default action. Rust's runtime leaves `SIGPIPE` ignored, so a write fails instead,
//! and the client takes that as the worker's end; an app that sets the default action back (a
//! command-line program may, for `| head`) must ignore it again before it uses a client. The
//! client writes nothing to a worker it has found exited, which narrows the moment but can't
//! close it.
//!
//! ## The watchdog, and cancel
//!
//! Silence is counted from the last line the client read, and only while the calling thread
//! waits on the worker: time it spends handing an event to the caller's full channel is the
//! caller's, so a slow consumer is never taken for a hung driver. A worker silent past its time
//! is killed ([`Error::Stopped`]):
//!
//! | While | Killed after, without a line ([`Timeouts`]) |
//! |---|---|
//! | Starting | `start`, 10 s, for its `hello` |
//! | A batch | `silence`, 60 s, a flatbed's warm-up included |
//! | A batch, once cancelled | `cancel`, 10 s, counted from the later of the cancel and the last line, so the sheet in flight finishes while it sends |
//! | A listing or an open | its deadline, and `grace`, 5 s |
//! | `caps` | `silence` |
//! | The settings window | never: it's the user's, and only its answer, [`StopHandle::stop`], or the worker's end ends the call |
//!
//! A worker writes a progress line only when the count moves, so a driver stuck mid-page goes
//! silent. A batch's caller cancels through its [`Progress`](scan_device::in_progress::Progress),
//! which the client reads every 50 ms and at each message and sends on once: the worker finishes
//! the sheet in flight, front and back, and feeds no more. A caller's receiver gone is a cancel
//! too, the rest of the batch read and dropped. To force-terminate an open device's worker from
//! another thread (for instance when a driver's untimed settings window hangs, or a batch must
//! stop immediately even if the caller's event channel is full), [`RemoteDevice::stop_handle`]
//! hands out a `Send + Sync + Clone` [`StopHandle`]: [`StopHandle::stop`] kills the worker and
//! makes the interrupted call (or the first call after an idle `stop`) answer [`Error::Aborted`],
//! with [`Error::Ended`] thereafter. A listing or an open the backend timed out
//! ([`ScanError::TimedOut`](scan_device::ScanError::TimedOut)) kills its worker at once, since
//! its driver may still be at it.
//!
//! ## When a worker fails
//!
//! Every page a worker sent whole before it failed is the caller's already, sent on as it came.
//! A worker that ends unasked is [`Error::Crashed`], with how it exited (its exit code, or on
//! Unix the signal that ended it: an abort's is `SIGABRT`, where on Windows an abort exits with
//! `0xC0000409`), and with a page its output ended within dropped and said. It's seen as its
//! output ends, or as its process exits: while a call waits on a silent worker, its process is
//! asked after every 50 ms, so a process its driver started, which may hold the worker's pipes
//! open long after, never holds the call. One killed for its silence is [`Error::Stopped`], with
//! which of its times ran out ([`Timeout`]).
//! Both keep the last 8 KiB of what the worker wrote to its standard error, which a thread drains
//! as it comes, for the app's log. A worker that ended is closed: every later call on its device
//! answers [`Error::Ended`]. A worker's end never holds the caller long: asked to quit, it's
//! given [`Timeouts::grace`] to exit, its input kept open meanwhile (its end would be its client
//! gone, which a worker aborts at 10 s later), then killed, and one that can't be reaped even
//! then is left to a thread that waits for it.
//!
//! # The protocol
//!
//! A client writes requests to a worker's standard input, and reads its messages from its
//! standard output: one JSON value a line, in UTF-8, each line ended by `\n` (a newline within a
//! string is escaped, so the line's own is its only one). A message is an object whose one key
//! names it, or the name alone, as a string, where it carries nothing (`"caps"`). The device
//! model's types inside are in [`scan_device`]'s `serde` form (fields and variants by their Rust
//! names, an enum's value tagged by its variant's name: `"Feeder"`, `{"Size":{"width":210.0,
//! "height":297.0}}`), and a deadline `D`, a [`Duration`](std::time::Duration), is
//! `{"secs":5,"nanos":0}`. A `page` line is followed by exactly the bytes it states, and then
//! the next line.
//!
//! ## The version
//!
//! A worker's first line, unasked, is its `hello`: the protocol it speaks, [`PROTOCOL`], and
//! the version of this crate it was built with, which a client doesn't read: it's for whoever
//! reads the exchange.
//!
//! ```text
//! {"hello":{"protocol":1,"version":"0.0.0"}}
//! ```
//!
//! A client that speaks another protocol refuses the worker before it writes anything: a worker
//! and its client ship together, so a mismatch is an error, not a negotiation. Every protocol's
//! `hello` keeps `protocol`, a number, where it is, and a client reads that alone where it can't
//! read the rest, so a worker of any other protocol is refused as one. While this crate is
//! 0.0.x, the protocol stays 1 and may change form without it ([`PROTOCOL`]): a worker and its
//! client come from one build.
//!
//! ## Requests, and their answers
//!
//! | Request | Answered with |
//! |---|---|
//! | `{"list":{"deadline":D}}` | `{"devices":[DeviceInfo, ...]}`, or `{"error":ScanError}` |
//! | `{"open":{"device":DeviceInfo,"deadline":D}}` | `{"opened":{"info":DeviceInfo,"renamed_from":DeviceId}}`, `renamed_from` `null` where the stored device was opened; or `error` |
//! | `"caps"` | `{"caps":Caps}`, or `error` |
//! | `{"scan":{"settings":Settings,"first_sheet":N}}` | the batch's messages, then `{"done":Done}`, or `error` |
//! | `{"settings_window":{"owner":WindowOwner}}`, the owner `null` for none | `{"window":WindowClosed}`, or `error` |
//! | `"cancel"` | nothing: the batch's end says it |
//! | `"quit"` | nothing: the worker's output ends |
//!
//! Each is a call on the model's traits ([`Backend`](scan_device::Backend) and
//! [`Device`](scan_device::Device)), answered as the call returns. A worker given several
//! backends, all of one driver API, lists each in turn and joins their devices, the first error
//! ending the listing; it opens a device through the first that doesn't answer
//! [`ScanError::BadId`](scan_device::ScanError::BadId), which an id another driver formed gets.
//! Each backend in turn is given what's left of the request's one deadline. An error is the
//! model's [`ScanError`](scan_device::ScanError), `{"error":"Jammed"}` or
//! `{"error":{"Driver":{"message":"...","shown":false}}}`.
//!
//! ## A batch
//!
//! Between `scan` and its `done` or `error`, a worker writes the batch's events in the order the
//! backend sends them ([`scan_device`]'s "A batch"), and its progress:
//!
//! - `{"applied":Applied}`, the settings the device took and what it changed;
//! - `{"status":Status}`, such as `{"status":"WarmingUp"}`;
//! - `{"sheet":N}`, sheet `N`, before its pages: as it begins, or with its front where the
//!   driver says a sheet is there only by scanning it;
//! - `{"page":...}` and its bytes, for each page, whole (below);
//! - `{"progress":{"done":B,"total":T}}`: the bytes done of the page in flight, and its total,
//!   `null` while it isn't known;
//! - `{"message":{"level":L,"text":"..."}}`: a message the driver reported, `L` one of
//!   `"info"`, `"warn"` and `"error"`.
//!
//! The progress is read from the batch's [`Tracker`](scan_device::in_progress::Tracker) after
//! each event, every 100 ms while none comes, and once more as the batch ends, and a line is
//! written only when it moved: a `progress` line where the bytes done or the total changed (in
//! bytes alone, the model's unit: a count in another unit isn't carried), a `message` line
//! where a message came. The tracker keeps only the latest message, so messages replaced
//! between two reads are lost. A driver stuck mid-page moves nothing, so its worker goes
//! silent, and its last `progress` line is where the batch stood.
//!
//! The backend's events go through a channel that holds none: each `send` waits until the one
//! before it is written whole, so a slow pipe holds the backend back, and the pages of the
//! sheets before the one in flight are in the pipe before that sheet's next side is read. A
//! page being written as a worker dies is cut short, which a client takes for the end it is,
//! never for a page.
//!
//! `cancel` cancels the batch's tracker: the backend finishes the sheet in flight, front and
//! back, feeds no more, and the batch ends `{"done":"Cancelled"}` after that sheet's pages. A
//! `cancel` written at once after its `scan` is never missed: the worker makes the batch's
//! tracker as it reads the `scan`.
//!
//! ## A page
//!
//! ```text
//! {"page":{"pixels":P,"dpi":[X,Y],"sheet":N,"side":Side,"side_known":SideKnown,"bytes":B}}
//! ```
//!
//! `P` is `{"raw":{"format":Format,"width":W,"height":H}}` or
//! `{"encoded":{"kind":EncodedKind}}`; `X` and `Y` the resolution across and down, in dots per
//! inch. Exactly `B` bytes follow the line: a raw page's data in the model's raw layout, as
//! [`RawImage`](scan_device::RawImage) holds it (rows top down and packed, so
//! [`RawImage::data_len`](scan_device::RawImage::data_len) gives `B` from the format and the
//! size), or an encoded page's bytes as the driver gave them. A reader caps `B`, and checks it
//! against the format and the size, before it reads them, and makes a raw page through
//! [`RawImage::new`](scan_device::RawImage::new), which checks them too. Letter at 300 dpi in
//! color:
//!
//! ```text
//! {"page":{"pixels":{"raw":{"format":"Rgb8","width":2550,"height":3300}},"dpi":[300.0,300.0],"sheet":1,"side":"Front","side_known":"Inferred","bytes":25245000}}
//! ```
//!
//! ## What a worker takes, and when
//!
//! One device a worker, and no listing beside it: `list` and `open` only while no device is
//! open, and `caps`, `scan` and `settings_window` only while one is. One call at a time: while
//! one runs, any other request is refused at once. `cancel` is taken at any time (with no batch
//! running, it does nothing), and so is `quit`, which cancels a running batch first.
//!
//! A request refused is answered `{"refused":{"reason":"..."}}`, the reason for a log, and the
//! worker goes on: one out of its state or while a call runs, a line that isn't a request, and
//! a line past 1 MiB, which is never read whole. A worker also writes `refused` in a batch for an
//! event, or a page's pixels, that the protocol doesn't carry (one the model gained after it),
//! which a client takes as the protocol error it is, rather than losing it unsaid.
//!
//! ## The end
//!
//! After `quit`, a worker drops its device and its backends, so the driver is closed, ends its
//! output, and exits with success; a client keeps the worker's input open until then. The end
//! of its input, its client gone, is a cancel and a `quit`; and should the worker not have
//! finished 10 seconds later (a driver's call, or its close, that never returns), it aborts.
//! Its output failing, its client gone, ends a running batch at its next event and the worker
//! with 1.
//!
//! ## An exchange
//!
//! A device opened, a batch of one sheet in simplex, and the worker quit: the client's lines
//! marked `>`, the worker's `<`, a page's bytes `[...]`.
//!
//! ```text
//! < {"hello":{"protocol":1,"version":"0.0.0"}}
//! > {"open":{"device":{"id":"fake:desk","name":"Maker Model","maker":"Maker","model":"Model","driver":"Fake"},"deadline":{"secs":5,"nanos":0}}}
//! < {"opened":{"info":{"id":"fake:desk","name":"Maker Model","maker":"Maker","model":"Model","driver":"Fake"},"renamed_from":null}}
//! > {"scan":{"settings":{"source":"Feeder","duplex":false,"dpi":150,"format":"Gray8","paper":"Auto","encodings":[]},"first_sheet":1}}
//! < {"applied":{"settings":{"source":"Feeder","duplex":false,"dpi":150,"format":"Gray8","paper":"Auto","encodings":[]},"clamped":[]}}
//! < {"sheet":1}
//! < {"page":{"pixels":{"raw":{"format":"Gray8","width":1275,"height":1650}},"dpi":[150.0,150.0],"sheet":1,"side":"Front","side_known":"Inferred","bytes":2103750}}
//! < [2103750 bytes]
//! < {"progress":{"done":2103750,"total":2103750}}
//! < {"done":"FeederEmpty"}
//! > "quit"
//! ```
//!
//! # Stability
//!
//! Provisional, as the device model is: 0.0.x and unpublished. The protocol carries the model's
//! types, so a change to the model changes the protocol; [`PROTOCOL`] stays 1 until the two are
//! frozen, and changes with every change of form from then on.

#![deny(unsafe_code)]
#![warn(missing_docs)]

mod client;
mod error;
mod serve;
#[cfg(any(unix, windows))]
#[allow(unsafe_code)]
mod sys;
mod wire;

pub use client::{Client, Limits, RemoteDevice, StopHandle, Timeouts, TwainList, WorkerCommand};
pub use error::{Error, ProtocolError, Timeout};
pub use serve::serve;
pub use wire::PROTOCOL;

/// The device model whose backends a worker serves and whose types the protocol carries,
/// re-exported, so an app names the version this crate's API does.
pub use scan_device;
