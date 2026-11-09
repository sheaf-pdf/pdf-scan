//! A device model for document scanners: one shape, whatever driver API reaches them.
//!
//! A [`Backend`] lists the devices its driver API finds and opens the one an app stored; the
//! [`Device`] it opens says what it can do ([`Caps`]), scans a batch with the [`Settings`] it's
//! given, sending what happens as [`Event`]s, and opens its driver's own settings window where it
//! has one. Each driver API (SANE, TWAIN, WIA, ...) is a backend behind these two traits, so an app
//! meets the same calls, events and errors whichever one finds a scanner.
//!
//! Everything is synchronous: a call blocks until it has its answer, and a batch runs on a thread
//! of its own until it ends. The types are plain data, with every field public but
//! [`RawImage`]'s, so they can cross a pipe to a process that runs the drivers. The structs and
//! enums that may grow are `#[non_exhaustive]`: a variant or a field added later breaks no caller,
//! and code outside this crate makes the structs by their `new` and then sets their fields.
//!
//! # Devices and their ids
//!
//! A listing gives each device's [`DeviceInfo`]: its [`DeviceId`], its name, maker and model, and
//! the [`Driver`] API it's reached through. An id is the driver's name and the driver's own id for
//! the device, `<driver>:<native id>`, compared whole and read by no one but the backend that
//! made it. An app stores the `DeviceInfo` and gives it back to [`Backend::open`], which checks it
//! against what the driver finds now: where it knowingly opens another device in the stored one's
//! place (the same scanner under a new name), it says so in [`Opened::renamed_from`].
//!
//! # A batch
//!
//! [`Device::scan`] blocks until the batch ends, and sends its events on a bounded channel, a
//! [`SyncSender`](std::sync::mpsc::SyncSender) made by
//! [`sync_channel(n)`](std::sync::mpsc::sync_channel), the caller choosing `n`. In order:
//!
//! 1. [`Event::Applied`] once the settings are set, before the first sheet: the settings the
//!    device took, and what it changed of those asked for ([`Clamped`]);
//! 2. [`Event::Status`], at any point, for what the user should know meanwhile, such as the scanner
//!    warming up;
//! 3. [`Event::Sheet`]`(n)` as sheet `n` begins, counted on from the `first_sheet` given, then an
//!    [`Event::Page`] for each side of it scanned, each page whole and sent once it's complete.
//!
//! It returns how the batch ended, a [`Done`] (the feeder emptied, a flatbed's one page, or
//! cancelled), or a [`ScanError`]. [`ScanError::FeederEmpty`] comes only before any page: after
//! pages, an empty feeder is [`Done::FeederEmpty`]. The pages sent before an error are kept.
//!
//! A `send` waits while the channel is full, so a backend whose events nobody takes holds there,
//! starting no next sheet, and its driver waits with it: pages never pile up past the bound. A
//! `send` that fails, the receiver gone, ends the batch at once as [`Done::Cancelled`], with no
//! sheet started after it.
//!
//! # Cancel and progress
//!
//! [`Device::scan`] takes an [`in_progress::Progress`]; the crate is re-exported as
//! [`in_progress`], so an app names the version the trait does. The caller passes a
//! [`Tracker`](in_progress::Tracker) and cancels the batch through a clone of it, from any thread.
//! A backend checks it between sheets and never within one, so a cancel finishes the sheet in
//! flight, front and back, feeds no more, and the batch ends as [`Done::Cancelled`]. A cancelled
//! tracker stays cancelled, so each batch takes a tracker of its own. Progress is counted in each
//! page's bytes.
//!
//! A backend whose driver delivers pages on a thread of its own gives that thread
//! `progress.child(..)` (owned, `Send`, and cancelled whenever its parent is) and a clone of the
//! `SyncSender`, which waits on a full channel there as it does anywhere.
//!
//! # The raw layout
//!
//! A [`RawImage`] holds a page's pixels in one layout, whatever the driver gave:
//!
//! - rows top down, each right after the one before, with nothing between them, so a row's
//!   length, [`RawImage::stride`], follows from the format and the width;
//! - 0 the least intensity at every format: black in [`Format::Bw1`] and the grays, and in color
//!   none of that sample's red, green or blue;
//! - in [`Format::Bw1`], eight pixels a byte, the leftmost in its most significant bit, each row
//!   rounded up to whole bytes, with the bits past a row's last pixel 0;
//! - in [`Format::Rgb8`] and [`Format::Rgb16`], red, green and blue, in that order, a pixel;
//! - 16-bit samples little-endian, whatever the host's byte order.
//!
//! [`RawImage::new`] checks what a buffer can get wrong: its length, and in `Bw1` the padding
//! bits. The sizes are worked out in checked arithmetic against what the platform's `usize` holds,
//! so a 32-bit process refuses what it can't hold rather than wrapping. [`RawImage::data_len`]
//! gives the length before any byte is read, so a reader caps a page's size and allocates once.
//!
//! # Features
//!
//! - `fake`: `scan_device::fake`, a scripted backend for tests, on every system and with no
//!   driver: devices listed and opened as a script says (a renamed one found by its maker and
//!   model), and batches of sheets whose pages it draws at the settings it applies. For tests
//!   only, never for a shipped build.
//!
//! None is on by default, and a backend for a driver API comes behind a feature of its own.
//!
//! # Example
//!
//! A device listed, opened and scanned through, here the fake one, as an app would: the batch on
//! a thread of its own, its events read as they come, a [`Tracker`](in_progress::Tracker) to
//! cancel it with.
//!
//! ```
//! use std::{sync::mpsc, thread, time::Duration};
//!
//! use scan_device::fake::{FakeBackend, FakeDevice, Sheet};
//! use scan_device::in_progress::Tracker;
//! use scan_device::{Backend, Done, Event, Format, Pixels, Settings, Side, Source};
//!
//! // Three sheets in the feeder of the one scanner the fake finds.
//! let scanner = FakeDevice::new("desk", "Maker", "Model");
//! scanner.load([Sheet::new(), Sheet::new(), Sheet::new()]);
//! let mut backend: Box<dyn Backend> = Box::new(FakeBackend::new().with(scanner));
//!
//! // What the app lists, stores, and opens again later.
//! let stored = backend.list(Duration::from_secs(5))?.remove(0);
//! let opened = backend.open(&stored, Duration::from_secs(5))?;
//! assert_eq!(opened.renamed_from, None);
//! let mut device = opened.device;
//! let caps = device.caps()?;
//! assert!(caps.sources.iter().any(|source| source.source == Source::Feeder && source.duplex));
//!
//! // Both sides of each sheet, in gray at 300 dpi.
//! let mut settings = Settings::new(Source::Feeder, 300, Format::Gray8);
//! settings.duplex = true;
//! let tracker = Tracker::new();
//! let (events, received) = mpsc::sync_channel(2);
//! let batch = {
//!     let tracker = tracker.clone();
//!     thread::spawn(move || device.scan(&settings, 1, &events, &tracker))
//! };
//! // `tracker.cancel()`, from any thread, would finish the sheet in flight and feed no more.
//! let mut pages = Vec::new();
//! for event in received {
//!     match event {
//!         Event::Applied(applied) => assert!(applied.clamped.is_empty()),
//!         Event::Sheet(sheet) => println!("sheet {sheet}"),
//!         Event::Page(page) => {
//!             let Pixels::Raw(image) = &page.pixels else { panic!("a raw page") };
//!             // Letter, 8.5 by 11 inches, at 300 dpi.
//!             assert_eq!((image.width(), image.height()), (2550, 3300));
//!             pages.push((page.sheet, page.side));
//!         }
//!         _ => {}
//!     }
//! }
//! assert_eq!(batch.join().expect("the batch's thread")?, Done::FeederEmpty);
//! assert_eq!(pages[..2], [(1, Side::Front), (1, Side::Back)]);
//! assert_eq!(pages.len(), 6);
//! # Ok::<(), scan_device::ScanError>(())
//! ```
//!
//! # Stability
//!
//! Provisional: 0.0.x and unpublished, while the model is tried against each driver API. It's
//! frozen at 0.1.0 once a second one has been through it.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod caps;
mod device;
mod error;
#[cfg(feature = "fake")]
pub mod fake;
mod image;
mod page;

pub use caps::{Applied, Caps, Clamped, Paper, Resolutions, Settings, SizeMm, Source, SourceCaps};
pub use device::{
    Backend, Device, DeviceId, DeviceInfo, Driver, Opened, WindowClosed, WindowOwner,
};
pub use error::ScanError;
pub use image::{Format, LayoutError, RawImage};
pub use page::{
    Done, Encoded, EncodedKind, Encoding, Event, Pixels, RawPage, Side, SideKnown, Status,
};

/// Progress and cancel: the crate whose [`Progress`](in_progress::Progress) [`Device::scan`]
/// takes, re-exported, so an app uses the version the trait names.
pub use in_progress;
