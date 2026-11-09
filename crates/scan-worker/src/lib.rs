//! Scanner drivers run in a process of their own: a worker serving any [`scan_device`] backends
//! over its standard input and output, and the protocol it speaks.
//!
//! A scanner's driver is code its maker wrote, loaded into whatever process calls it: a driver
//! that hangs holds that process's thread for good, one that crashes takes the process with it,
//! and a 32-bit driver loads only into a 32-bit process. Run in a worker process of its own, a
//! driver can hang, crash or need another bitness while the app goes on: the app keeps the
//! pages the worker sent before it failed, ends a worker that went silent, and starts another.
//!
//! The worker's side is [`serve`], the whole `main` of a worker binary, given the backends it
//! serves: every call on them made on its main thread, in order; a batch's events written as
//! they come, each page's bytes after its line, through a channel that holds the backend back
//! while the pipe is slow; a batch's progress and its driver's messages forwarded; a cancel
//! taken at once. This version has the worker's side and the protocol; the client that starts a
//! worker for each device, watches it, and reads its pages comes next.
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
//! the version of this crate it was built with, for a log.
//!
//! ```text
//! {"hello":{"protocol":1,"version":"0.0.0"}}
//! ```
//!
//! A client that speaks another protocol refuses the worker before it writes anything: a worker
//! and its client ship together, so a mismatch is an error, not a negotiation.
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
//! - `{"sheet":N}`, as sheet `N` begins;
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
//! output, and exits with success. The end of its input, its client gone, is a cancel and a
//! `quit`; and should the call in progress not have returned 10 seconds later (a driver's call
//! that never does), the worker aborts. Its output failing, its client gone, ends a running
//! batch at its next event and the worker with 1.
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
//! types, so a change to the model changes the protocol, and [`PROTOCOL`] with it once the two
//! are frozen.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod serve;
mod wire;

pub use serve::serve;
pub use wire::PROTOCOL;

/// The device model whose backends a worker serves and whose types the protocol carries,
/// re-exported, so an app names the version this crate's API does.
pub use scan_device;
