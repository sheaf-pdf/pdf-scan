//! The SANE backend: the scanners SANE finds, on Linux and the other Unix-like systems.
//!
//! [`SaneBackend`] reaches SANE through `sane-dl`, which loads libsane at run time and never links
//! it, so a program built with this backend runs where SANE isn't installed, and hears so as
//! [`ScanError::NotInstalled`] when it lists or opens a device.
//!
//! **Run it in a worker process.** SANE's backends are C code that runs in the process that loads
//! them; some corrupt memory, some crash, and some hang past any cancel. So an app runs this
//! backend in a process of its own (`scan-worker` serves it there, and its client gives the worker
//! that opens a stored device a configuration naming only that device's backend), and lists
//! devices in another than the one that scans, since SANE makes a listing and a scan wait on each
//! other.
//!
//! # Loading SANE
//!
//! SANE is loaded at a backend's first [`list`](crate::Backend::list) or
//! [`open`](crate::Backend::open), never as it's made, so a worker without libsane starts and
//! answers `NotInstalled`; where it isn't there (no library, or a system with none, as Windows),
//! each call tries again. It's loaded once per process, whatever the number of backends: while
//! any backend, or any device one opened, holds it, every other backend made in the process shares
//! it, and once all are gone it ends, so the next one loads it afresh. The library is the one the
//! system's loader finds under SANE's name, by the search the process started with, as `sane-dl`'s
//! `Sane::load` says, and that rests on two rules of the process's environment, which it gives:
//! every entry of the loader's search path absolute, and that path never changed inside a process
//! using SANE. Any other failure to load is [`ScanError::Driver`].
//!
//! # Devices and their ids
//!
//! A listing gives every device SANE finds, network backends' among them, within its deadline;
//! past it the answer is [`ScanError::TimedOut`], and every call after it too while the listing
//! still holds SANE, so the backend is then the caller's to drop, with its process. A device's id
//! is `sane:` and the name SANE gives it, each of the name's bytes the character of its number
//! (Latin-1, the charset SANE's standard gives its strings), so the name goes back to SANE byte for
//! byte. Its name, maker and model, for people, are SANE's name, vendor and model read as UTF-8
//! where they are (some backends send it), else as Latin-1.
//!
//! [`open`](crate::Backend::open) takes the `DeviceInfo` an app stored, and goes in this order:
//!
//! 1. **Ids no listing gives are refused** as [`ScanError::BadId`], before SANE is loaded: a
//!    device of another driver; an id not `sane:`; a character past U+00FF, or a `NUL`; a name
//!    longer than any listing gives (64 KiB); the empty name, and a name starting `:`, which SANE
//!    would hand to a backend the id never named; and the names of SANE's testing modes
//!    (`fakeusb:`, `fakeusbdev:` and `fakeusbout:`), which replay or record a device's traffic
//!    through a file the name gives. A backend's name alone, or with an empty name after it
//!    (`fujitsu:`), stays within that backend, and is taken.
//! 2. **The id's backend is listed, before anything opens,** under the open's deadline. In a
//!    worker whose configuration names that backend alone, that's fast and that backend's; in an
//!    app's own process it's the system's, every backend's, which can take seconds.
//! 3. **The stored name**, byte for byte: listed with the stored maker and model, it's opened, and
//!    [`Opened::info`](crate::Opened::info) is the listing's; listed with another, it's another
//!    device's name now, never opened; not listed, it's opened by name all the same (an alias, or
//!    a name a backend opens but lists another way), `info` the stored one, unchecked, since SANE
//!    says nothing of a device it opens.
//! 4. **Its errors:** a device SANE doesn't have (`INVAL`) goes to the fallback, below; refused
//!    access is [`ScanError::AccessDenied`], a device in use [`ScanError::Busy`], an I/O error
//!    [`ScanError::Gone`], and the rest [`ScanError::Driver`].
//! 5. **The fallback**, for a name that's gone or another device's: only from a USB connection's
//!    name (`<backend>:libusb:<bus>:<device>`, whose numbers a replugging or a reboot gives
//!    again), to the one device listed under another such name of the same backend with the
//!    stored maker and model, if exactly one is, said in
//!    [`Opened::renamed_from`](crate::Opened::renamed_from). Anything else is
//!    [`ScanError::Gone`], for the app to show the list: none, or several; and every other kind
//!    of name, a serial (which survives a replugging), a network address (another machine's
//!    scanner of that model), an index, or an alias.
//!
//! A name given since to another device of the same maker and model opens that device
//! unremarked, which only a name holding the device's serial prevents.
//!
//! # Options set by name
//!
//! [`SaneBackend::with_option`] gives an option the model doesn't carry, by its SANE name and a
//! value, set on every device the backend opens, after it opens and in the order given: where the
//! device has the option and it's active, and skipped otherwise, so a name misspelt sets nothing.
//! The value is read by the option's type (a string's bytes; a boolean from `true`, `false`,
//! `yes`, `no`, `1` or `0`; a number in each of its words; a button pressed), and one that doesn't
//! read, or that the device refuses, fails the open as [`ScanError::Driver`]. These are the
//! app's own choice, never a stored device's or a device's answer: an option can do anything a
//! backend lets it (the `pnm` backend's `filename` reads any file the process can, and the `test`
//! backend's options inject faults).
//!
//! # What a device can do
//!
//! [`Device::caps`] reads the device's options afresh each time, a source at a time: SANE's
//! options change with the source, so it sets the source option to each source's value, reads
//! what that source can do, and sets the option back to the value it found. An option already at
//! the value wanted isn't set, and a source option with one value, or one software can't set,
//! never is, so only the source it's at is read. Where a
//! device names a source, a mode or an option its own way, the backend reads it by tables of the
//! names backends are seen to use, each with where it's seen:
//!
//! | What | Read from | The model |
//! |---|---|---|
//! | The source | `source`, else Samsung's `doc-source`; neither, one flatbed | [`Source`](crate::Source) |
//! | A flatbed | `Flatbed`, `FlatBed`, `Normal`, `Document Table` | `Flatbed` |
//! | A feeder, one side | `ADF`, `ADF Front`, `ADF Simplex`, `Automatic Document Feeder`, and Brother's `Automatic Document Feeder(left aligned)`, `(centrally aligned)` and `(center aligned)`: the first listed | `Feeder` |
//! | A feeder, both sides | `ADF Duplex`, `Duplex ADF`, `Duplex`, `ADF Duplex - Long-Edge Binding`, Brother's `...,Duplex)` (the one matching the one side's, else the first) | the feeder's `duplex` |
//! | Duplex apart from the source | `ScanMode` of `Simplex` or `Duplex`, `adf-mode` or `adf_mode` likewise, `scan-both-sides` or `duplex` as a boolean, `duplex` of `front` or `both`, active once the feeder is the source | the feeder's `duplex` |
//! | Left out | `ADF Back` (the backs alone), `Auto` (a flatbed that never stops), any other value, translated ones among them | not offered |
//! | Formats | `mode`: `Lineart`, `Binary`, `Black & White`, `Black and White - Line Art` (black and white); `Gray`, `Grayscale`, `True Gray`; `Color`, `24bit Color`. With `depth`, gray and color at 8 bits and 16 where it allows them, and gray at 1 as black and white; without, 8 bits. Dithered modes (`Halftone`) left out | [`Format`](crate::Format), in its order |
//! | No `mode` | the parameters before a start: gray at 1, 8 or 16 bits, color at 8 or 16, color at 1 as 8 | that one format |
//! | Resolutions | `resolution`, else `scan-resolution`, else `x-resolution`: a range or a list, in whole dpi; none of these, 72 dpi | [`Resolutions`](crate::Resolutions) |
//! | The largest area | a feeder's `page-width` and `page-height`, where active; else `br-x` and `br-y` less `tl-x` and `tl-y`; in millimetres, or in pixels at the resolution now | `max_area`; no such options, none |
//!
//! Where each name is seen: SANE's standard
//! (<https://gitlab.com/sane-project/standard>) names `source`, `mode`, `depth`, `resolution`
//! and the scan area; the backends' man pages (<https://gitlab.com/sane-project/backends/-/tree/master/doc>:
//! sane-test(5), sane-pnm(5), sane-fujitsu(5)) give `Flatbed`, `Automatic Document Feeder`, the
//! `ADF` sources and `page-width`; the rest are read as frontends read them, from NAPS2
//! (<https://github.com/cyanfish/naps2/tree/master/NAPS2.Sdk/Scan/Internal/Sane>), simple-scan
//! (<https://github.com/GNOME/simple-scan/blob/master/src/scanner.vala>) and libinsane
//! (<https://gitlab.gnome.org/World/OpenPaperwork/libinsane>). Values are matched whole, never by
//! a part of them. A feeder is read through its one-sided value, so where both sides allow less
//! than one (fewer resolutions, say), its caps are the one side's. A resolution range's ends are rounded inward to whole dpi, its step to whole dpi (none, or
//! under 1, is 1; a larger fraction rounded up), and a list's values to the nearest whole dpi,
//! nothing below 1. No source finds the paper's size itself, and none gives encoded pages (SANE 1
//! has none). A device with no source the model can scan from (no format, or no resolution, it
//! can name) is [`ScanError::Unsupported`].
//!
//! SANE has no settings window of a driver's own, so [`Device::settings_window`] is
//! [`ScanError::Unsupported`].

use std::sync::{Arc, Mutex, PoisonError, Weak};
use std::time::Duration;

use in_progress::Progress;
use sane_dl::{Call, Error, Handle, Sane, Status};

use crate::{
    Backend, Caps, Device, DeviceInfo, Done, Driver, Event, Opened, ScanError, Settings,
    WindowClosed, WindowOwner,
};

mod caps;
mod error;
mod id;
mod options;

/// SANE, as a backend: the scanners SANE finds, listed and opened.
///
/// Loads SANE at its first [`list`](Backend::list) or [`open`](Backend::open), sharing it with
/// every other `SaneBackend` in the process ([the module's docs](self) say how, and what each
/// call does).
#[derive(Debug, Default)]
pub struct SaneBackend {
    /// The options set by name on every device it opens, in order.
    options: Vec<(String, String)>,
    /// SANE, once loaded.
    sane: Option<Arc<Sane>>,
}

impl SaneBackend {
    /// A backend that sets no option by name.
    pub fn new() -> SaneBackend {
        SaneBackend::default()
    }

    /// The backend, setting the option SANE names `name` to `value` on every device it opens,
    /// after the options given before: where the device has the option and it's active, skipped
    /// otherwise, so a name misspelt sets nothing; read by the option's type (a string's bytes;
    /// `true`, `false`, `yes`, `no`, `1` or `0`; a number; a button pressed, whatever `value`
    /// says). A value that doesn't read, or that the device refuses, fails the open.
    ///
    /// For what the model doesn't carry, such as the `pnm` backend's `filename` or a backend's
    /// advanced options: the app's own choice, since an option can do whatever its backend lets it
    /// (`pnm`'s `filename` reads any file the process can).
    #[must_use]
    pub fn with_option(mut self, name: impl Into<String>, value: impl Into<String>) -> SaneBackend {
        self.options.push((name.into(), value.into()));
        self
    }

    /// SANE, loaded at the first call that needs it and kept.
    fn sane(&mut self) -> Result<Arc<Sane>, ScanError> {
        if let Some(sane) = &self.sane {
            return Ok(Arc::clone(sane));
        }
        let sane = shared()?;
        self.sane = Some(Arc::clone(&sane));
        Ok(sane)
    }

    /// A device of `handle`, the options given by name set on it; the handle closed if one fails.
    fn device(&self, sane: Arc<Sane>, mut handle: Handle) -> Result<Box<dyn Device>, ScanError> {
        options::set_by_name(&mut handle, &self.options)?;
        Ok(Box::new(SaneDevice {
            handle,
            _sane: sane,
        }))
    }
}

/// The process's SANE, while a backend or a device holds it: SANE is started once per process,
/// so every backend shares the one loaded.
static LOADED: Mutex<Weak<Sane>> = Mutex::new(Weak::new());

/// The process's SANE: the one a backend or a device holds, else loaded now. Held locked while it
/// loads, so two backends never both load it.
fn shared() -> Result<Arc<Sane>, ScanError> {
    let mut loaded = LOADED.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(sane) = loaded.upgrade() {
        return Ok(sane);
    }
    let sane = Arc::new(Sane::load().map_err(|error| error::load(&error))?);
    *loaded = Arc::downgrade(&sane);
    Ok(sane)
}

/// Every device SANE lists within `deadline`, network backends' among them.
fn listing(sane: &Sane, deadline: Duration) -> Result<Vec<id::Listed>, ScanError> {
    let devices = sane
        .devices(false, deadline)
        .map_err(|error| error::map(&error))?;
    Ok(devices
        .into_iter()
        .map(|device| {
            let name = device.name.into_bytes();
            let info = DeviceInfo::new(
                id::id_of(&name),
                id::text(&name),
                id::text(device.vendor.as_bytes()),
                id::text(device.model.as_bytes()),
                Driver::Sane,
            );
            id::Listed { name, info }
        })
        .collect())
}

/// `sane_open` of `name`: its handle, or `None` where SANE has no such device (`INVAL`).
fn open_handle(sane: &Sane, name: &[u8]) -> Result<Option<Handle>, ScanError> {
    match sane.open(name) {
        Ok(handle) => Ok(Some(handle)),
        Err(Error::Status {
            status: Status::INVAL,
            call: Call::Open,
        }) => Ok(None),
        Err(error) => Err(error::map(&error)),
    }
}

impl Backend for SaneBackend {
    fn list(&mut self, deadline: Duration) -> Result<Vec<DeviceInfo>, ScanError> {
        let sane = self.sane()?;
        Ok(listing(&sane, deadline)?
            .into_iter()
            .map(|listed| listed.info)
            .collect())
    }

    fn open(&mut self, stored: &DeviceInfo, deadline: Duration) -> Result<Opened, ScanError> {
        let name = id::name_of(stored)?;
        let sane = self.sane()?;
        let listed = listing(&sane, deadline)?;
        let info = match listed.iter().find(|device| device.name == name) {
            Some(device) if id::same_device(&device.info, stored) => Some(device.info.clone()),
            // Another device's name now: never opened.
            Some(_) => None,
            None => Some(stored.clone()),
        };
        if let Some(info) = info
            && let Some(handle) = open_handle(&sane, &name)?
        {
            return Ok(Opened {
                device: self.device(sane, handle)?,
                info,
                renamed_from: None,
            });
        }
        let renamed = id::fallback(&name, stored, &listed).ok_or(ScanError::Gone)?;
        let handle = open_handle(&sane, &renamed.name)?.ok_or(ScanError::Gone)?;
        Ok(Opened {
            device: self.device(sane, handle)?,
            info: renamed.info.clone(),
            renamed_from: Some(stored.id.clone()),
        })
    }
}

/// An open SANE device.
struct SaneDevice {
    /// Its handle, closed as it's dropped.
    handle: Handle,
    /// The process's SANE, held beside the handle and dropped after it, so the backends' shared
    /// SANE lives as long as this device keeps SANE loaded.
    _sane: Arc<Sane>,
}

impl Device for SaneDevice {
    fn caps(&mut self) -> Result<Caps, ScanError> {
        caps::read(&mut self.handle)
    }

    fn scan(
        &mut self,
        _settings: &Settings,
        _first_sheet: u32,
        _events: &std::sync::mpsc::SyncSender<Event>,
        _progress: &dyn Progress,
    ) -> Result<Done, ScanError> {
        // Batches aren't built yet: until they are, a scan is refused before anything is set.
        Err(ScanError::Unsupported)
    }

    fn settings_window(&mut self, _owner: Option<WindowOwner>) -> Result<WindowClosed, ScanError> {
        Err(ScanError::Unsupported)
    }
}
