//! Devices, the two traits a backend implements, and the driver's settings window.

use std::{fmt, sync::mpsc::SyncSender, time::Duration};

use in_progress::Progress;

use crate::{Caps, Done, Event, ScanError, Settings};

/// A device's id: the driver's name and the driver's own id for it, `<driver>:<native id>`.
///
/// Compared whole, and read by no one but the backend that made it.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DeviceId(pub String);

/// A device as a listing gives it, and as an app stores it and gives it back to
/// [`Backend::open`].
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub struct DeviceInfo {
    /// Its id.
    pub id: DeviceId,
    /// Its name, to show.
    pub name: String,
    /// Its maker, as the driver gives it.
    pub maker: String,
    /// Its model, as the driver gives it.
    pub model: String,
    /// The driver API it's reached through.
    pub driver: Driver,
}

impl DeviceInfo {
    /// The device `id`, named `name`, by `maker`, of `model`, reached through `driver`.
    pub fn new(
        id: DeviceId,
        name: String,
        maker: String,
        model: String,
        driver: Driver,
    ) -> DeviceInfo {
        DeviceInfo {
            id,
            name,
            maker,
            model,
            driver,
        }
    }
}

/// The driver API a device is reached through.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum Driver {
    /// SANE, on Linux and the other Unix-like systems.
    Sane,
    /// TWAIN, on Windows.
    Twain {
        /// The bitness of the driver, 32 or 64: a 32-bit driver runs in a 32-bit process.
        bits: u8,
    },
    /// WIA, on Windows.
    Wia,
    /// ImageCaptureCore, on macOS.
    Ica,
    /// A scripted stand-in, for tests.
    Fake,
}

/// A driver API: lists the devices it finds, and opens one.
///
/// `Send`, so a backend can be made on one thread and used on another.
pub trait Backend: Send {
    /// The devices the driver finds now, a snapshot: those found within `deadline`, or
    /// [`ScanError::TimedOut`] where the listing isn't done by then.
    fn list(&mut self, deadline: Duration) -> Result<Vec<DeviceInfo>, ScanError>;

    /// Opens `device`, the `DeviceInfo` a listing gave and the app stored, under `deadline`.
    ///
    /// An id this backend's driver doesn't form is refused as [`ScanError::BadId`] before the
    /// backend lists or opens anything (a backend whose driver API isn't installed may say so
    /// first, as [`ScanError::NotInstalled`]); past the deadline, the answer is
    /// [`ScanError::TimedOut`]. The backend checks the stored device against what the driver
    /// finds now: where it knowingly opens another device in its place (the same scanner under a
    /// new name, found by its maker and model), it says so in [`Opened::renamed_from`]. The
    /// deadline bounds what the backend can bound; a driver's call that never returns is for
    /// whoever runs the backend to end, with its process.
    fn open(&mut self, device: &DeviceInfo, deadline: Duration) -> Result<Opened, ScanError>;
}

/// A device [`Backend::open`] opened, and what it found.
///
/// `Debug` by hand: the device, a trait object, is left out.
pub struct Opened {
    /// The open device.
    pub device: Box<dyn Device>,
    /// The device as the backend found it, or the stored one where it couldn't check.
    pub info: DeviceInfo,
    /// The stored id, where the backend opened another device in its place; `None` where it
    /// opened the stored one.
    pub renamed_from: Option<DeviceId>,
}

impl fmt::Debug for Opened {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Opened")
            .field("info", &self.info)
            .field("renamed_from", &self.renamed_from)
            .finish_non_exhaustive()
    }
}

/// An open device: what it can do, a batch scanned, and its driver's settings window.
///
/// `Send`, so a device can be opened on one thread and scan on another.
pub trait Device: Send {
    /// What the device can do, per source.
    fn caps(&mut self) -> Result<Caps, ScanError>;

    /// Scans a batch with `settings`, numbering its sheets on from `first_sheet`, and blocks until
    /// it ends.
    ///
    /// The settings are clamped to what the device can do, and said in [`Event::Applied`] before
    /// the first sheet. Then each sheet is [`Event::Sheet`] before its pages (as it begins, or with
    /// its front where the driver says a sheet is there only by scanning it) and each of its pages
    /// an [`Event::Page`], whole, once it's complete; [`Event::Status`] says what the user should
    /// know meanwhile. Each `send` on `events` waits while the channel holds as many events as its
    /// bound, so once that many wait untaken the backend holds at its next `send`, its driver with
    /// it, until one is taken; on a rendezvous channel (`sync_channel(0)`) each `send` waits until
    /// its event is taken. A `send` that fails, the receiver gone, ends the batch at once as
    /// [`Done::Cancelled`].
    ///
    /// `progress` is checked between sheets and never within one: once its check fails, the
    /// sheet in flight is finished, front and back, no other sheet is fed, and the batch ends as
    /// [`Done::Cancelled`]. Each page's bytes are counted on it as they come. The sheet in flight's
    /// events still come after a cancel, and a backend held at a full channel never reaches its
    /// check, so a caller that cancels keeps taking events until `scan` returns, or drops the
    /// receiver to end the batch at its next `send`.
    ///
    /// It returns how the batch ended, or a [`ScanError`]. [`ScanError::FeederEmpty`] comes only
    /// before any page: after pages, an empty feeder ends it as [`Done::FeederEmpty`]. The pages
    /// sent before an error are kept.
    fn scan(
        &mut self,
        settings: &Settings,
        first_sheet: u32,
        events: &SyncSender<Event>,
        progress: &dyn Progress,
    ) -> Result<Done, ScanError>;

    /// Opens the driver's own settings window, owned by `owner`'s window where it gives one, and
    /// answers how it closed: [`WindowClosed::Accepted`], its settings kept and applying to scans
    /// until they're changed, or [`WindowClosed::Dismissed`]. A device without one
    /// ([`Caps::settings_window`] false) answers [`ScanError::Unsupported`].
    fn settings_window(&mut self, owner: Option<WindowOwner>) -> Result<WindowClosed, ScanError>;
}

/// How the driver's settings window closed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum WindowClosed {
    /// Its settings were accepted, and apply to scans until they're changed.
    Accepted,
    /// It was closed without accepting them.
    Dismissed,
}

/// The app's window, to own the driver's: a native handle as an integer (an `HWND` on Windows).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct WindowOwner(pub u64);
