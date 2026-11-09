//! A scripted backend, for tests: devices that list, open and scan as their script says, on every
//! system, with no driver.
//!
//! For tests only, never for a shipped build: it's behind the `fake` feature. A test builds its
//! script as Rust values: a [`FakeBackend`] holds [`FakeDevice`]s, and a device's feeder and glass
//! hold [`Sheet`]s. A `FakeDevice` is a script and a handle at once: a clone of it shares the
//! device's paper, so a test that hands one clone to the backend keeps another to load paper
//! between batches, or to read how many sheets were fed while a batch is held on another thread.
//!
//! ```
//! use std::{sync::mpsc, time::Duration};
//!
//! use scan_device::fake::{FakeBackend, FakeDevice, Sheet};
//! use scan_device::in_progress::Tracker;
//! use scan_device::{Backend, Done, Event, Format, Settings, Source};
//!
//! let scanner = FakeDevice::new("libusb:001:004", "Maker", "Model");
//! let mut backend = FakeBackend::new().with(scanner.clone());
//! let stored = backend.list(Duration::from_secs(1))?.remove(0);
//! let mut device = backend.open(&stored, Duration::from_secs(1))?.device;
//!
//! scanner.load([Sheet::new(), Sheet::new()]);
//! let (events, received) = mpsc::sync_channel(8);
//! let settings = Settings::new(Source::Feeder, 150, Format::Bw1);
//! let done = device.scan(&settings, 1, &events, &Tracker::new())?;
//! drop(events);
//! assert_eq!(done, Done::FeederEmpty);
//! assert_eq!(scanner.fed(), 2);
//! let pages = received.iter().filter(|event| matches!(event, Event::Page(_))).count();
//! assert_eq!(pages, 2);
//! # Ok::<(), scan_device::ScanError>(())
//! ```
//!
//! # Listing and opening
//!
//! As the backends for real driver APIs do, where a name can change under a device:
//!
//! - [`Backend::list`] gives each listed device's [`DeviceInfo`] (its id `fake:<name>`, its name
//!   its maker and model, [`Driver::Fake`]) once the backend's listing has taken its time
//!   ([`FakeBackend::listing_takes`], none by default). A listing that takes as long as the
//!   deadline or longer waits the deadline out and answers [`ScanError::TimedOut`], so a deadline
//!   of zero always does.
//! - [`Backend::open`] answers, in order: [`ScanError::NotInstalled`] where the backend is
//!   scripted so; [`ScanError::BadId`] for an id that isn't `fake:` and a name of at least one
//!   character; then it lists, under the deadline. The stored id listed with the stored maker and
//!   model opens that device, [`Opened::info`] the listing's. Listed with another maker or model,
//!   it's taken as gone: the name is another device's now. Not listed but scripted
//!   [`unlisted`](FakeDevice::unlisted), it opens by its name, `info` the stored one, unchecked.
//!   Not there at all, it's gone.
//! - **A device taken as gone falls back only from a USB connection's name,**
//!   `fake:libusb:<bus>:<device>` (each number one or more digits), the one form whose numbers a
//!   replugging changes: to the one listed device under a name of that form with the stored maker
//!   and model, if exactly one is, said in [`Opened::renamed_from`]. Anything else, none or
//!   several, or a stored name of another form, is [`ScanError::Gone`].
//! - Once found, a device scripted to [refuse to open](FakeDevice::refuses_open) answers its
//!   error.
//!
//! # Its capabilities
//!
//! By default, a feeder with duplex (every resolution from 50 to 600 dpi, the five formats, Legal
//! its largest paper, which it can also find the size of itself) and a flatbed (75, 150, 300 and
//! 600 dpi, the five formats, 215.9 by 297 mm its largest paper); no encoded forms, no settings
//! window. [`FakeDevice::caps`] gives others.
//!
//! # A batch
//!
//! [`Device::scan`] first clamps the settings to the chosen source's caps, and sends
//! [`Event::Applied`] with each it changed, holding what was asked ([`Clamped`]):
//!
//! - a source the caps lack becomes the first they list;
//! - duplex on a source without it is turned off;
//! - a resolution becomes the nearest the source allows, the higher of two as near: in a list,
//!   its nearest value; in a range, the nearest of `min`, `min + step`, and so on, never past
//!   `max`;
//! - a format the source lacks becomes the first it lists;
//! - a paper size beyond the source's largest is cut to it, each way; one not finite and above 0
//!   becomes the largest; any size, on a source with no largest, becomes [`Paper::Auto`]; and
//!   `Auto` on a source that can't find the size itself becomes its largest paper, or stays `Auto`
//!   where it has none, scanning the sheet's own size;
//! - the encoded forms the source doesn't offer are dropped, the rest kept in the order asked.
//!
//! Then, where the device is scripted to [warm up](FakeDevice::warms_up),
//! `Event::Status(Status::WarmingUp)`. Then, from the feeder, sheet by sheet: the progress
//! checked, and a failed check ending the batch as [`Done::Cancelled`] before another sheet is
//! taken; the sheet's number, counted on from `first_sheet`; the sheet taken from the feeder and
//! [`Event::Sheet`] sent; then its front's page, and its back's where duplex is on, each an
//! [`Event::Page`]. An empty feeder before any sheet is [`ScanError::FeederEmpty`], and after
//! sheets [`Done::FeederEmpty`]. From the flatbed: the progress checked, then the sheet on the
//! glass scanned the same way, numbered `first_sheet` and left where it lies: [`Done::OnePage`].
//!
//! A side's page is the one its sheet was given ([`Sheet::side`]), delivered as given; else one
//! drawn at the settings as applied: the paper's size (under `Auto`, the sheet's own, cut to the
//! source's largest), at the resolution, `mm / 25.4 × dpi` pixels each way, rounded, and at least
//! 1 (Letter at 300 dpi is 2550 by 3300). Where the applied settings ask for an encoded form that
//! fits the page's format (CCITT G4 black and white only, JPEG 8-bit gray or color only, PNG, TIFF
//! and BMP any), the page comes in the first that fits, its bytes a short stand-in naming the
//! form, the sheet and the side: the fake encodes nothing. Else it's raw, drawn by [`pattern`].
//! A page past 512 MiB raw is refused before anything is allocated, as a [`ScanError::Driver`]
//! error, the pages before it kept and its sheet put back at the head of the feeder. Each page's
//! progress is its bytes: `set_total(Some(bytes), Unit::Bytes)`, `set(0)`, then `inc` a row at a
//! time (an encoded page's all at once). Its resolution is the applied one, both ways; its side
//! [`SideKnown::Inferred`], or [`SideKnown::Reported`] where the device is scripted to
//! [report sides](FakeDevice::reports_sides).
//!
//! Every `send` waits while the channel is full, and the device's paper is never locked while it
//! does, nor while the progress is called, so [`FakeDevice::fed`] answers while a batch waits. A
//! `send` that fails, the receiver gone, ends the batch at once as [`Done::Cancelled`].
//!
//! [`Clamped`]: crate::Clamped
//! [`Device::scan`]: crate::Device::scan
//! [`Done::Cancelled`]: crate::Done::Cancelled
//! [`Done::FeederEmpty`]: crate::Done::FeederEmpty
//! [`Done::OnePage`]: crate::Done::OnePage
//! [`Event::Applied`]: crate::Event::Applied
//! [`Event::Page`]: crate::Event::Page
//! [`Event::Sheet`]: crate::Event::Sheet
//! [`Paper::Auto`]: crate::Paper::Auto
//! [`ScanError::Driver`]: crate::ScanError::Driver
//! [`ScanError::FeederEmpty`]: crate::ScanError::FeederEmpty
//! [`SideKnown::Inferred`]: crate::SideKnown::Inferred
//! [`SideKnown::Reported`]: crate::SideKnown::Reported

mod batch;
mod clamp;
mod pattern;

use std::{
    collections::VecDeque,
    num::NonZeroU32,
    sync::{Arc, Mutex, MutexGuard, PoisonError, mpsc::SyncSender},
    thread,
    time::Duration,
};

use in_progress::Progress;

pub use pattern::pattern;

use crate::{
    Backend, Caps, Device, DeviceId, DeviceInfo, Done, Driver, Event, Format, Opened, Pixels,
    Resolutions, ScanError, Settings, Side, SideKnown, SizeMm, Source, SourceCaps, WindowClosed,
    WindowOwner,
};

/// A backend of scripted devices: it lists and opens them as the module's docs say ("Listing and
/// opening").
#[derive(Debug)]
pub struct FakeBackend {
    devices: Vec<FakeDevice>,
    listing_takes: Duration,
    not_installed: bool,
}

impl Default for FakeBackend {
    fn default() -> FakeBackend {
        FakeBackend::new()
    }
}

impl FakeBackend {
    /// A backend with no devices, whose listing takes no time.
    pub fn new() -> FakeBackend {
        FakeBackend {
            devices: Vec::new(),
            listing_takes: Duration::ZERO,
            not_installed: false,
        }
    }

    /// The backend with `device` too, listed unless it's [`FakeDevice::unlisted`].
    ///
    /// # Panics
    ///
    /// Where the backend has a device of the same id already: a script's mistake.
    pub fn with(mut self, device: FakeDevice) -> FakeBackend {
        assert!(
            self.devices
                .iter()
                .all(|other| other.info.id != device.info.id),
            "the fake backend has a device {:?} already",
            device.info.id.0
        );
        self.devices.push(device);
        self
    }

    /// The backend whose listing takes `delay`, in [`Backend::list`] and in [`Backend::open`]
    /// alike. A deadline longer than it gets the listing after it; any other is waited out, and
    /// answered [`ScanError::TimedOut`].
    pub fn listing_takes(mut self, delay: Duration) -> FakeBackend {
        self.listing_takes = delay;
        self
    }

    /// The backend as it is where its driver API isn't installed: [`Backend::list`] and
    /// [`Backend::open`] answer [`ScanError::NotInstalled`].
    pub fn not_installed(mut self) -> FakeBackend {
        self.not_installed = true;
        self
    }

    /// `NotInstalled` where the backend is scripted so.
    fn installed(&self) -> Result<(), ScanError> {
        if self.not_installed {
            return Err(ScanError::NotInstalled {
                what: "the fake driver".to_owned(),
            });
        }
        Ok(())
    }

    /// The listed devices, once the listing has taken its time under `deadline`.
    fn listing(&self, deadline: Duration) -> Result<Vec<&FakeDevice>, ScanError> {
        if self.listing_takes >= deadline {
            thread::sleep(deadline);
            return Err(ScanError::TimedOut);
        }
        thread::sleep(self.listing_takes);
        Ok(self.devices.iter().filter(|device| device.listed).collect())
    }
}

impl Backend for FakeBackend {
    fn list(&mut self, deadline: Duration) -> Result<Vec<DeviceInfo>, ScanError> {
        self.installed()?;
        let listed = self.listing(deadline)?;
        Ok(listed.into_iter().map(FakeDevice::info).collect())
    }

    fn open(&mut self, stored: &DeviceInfo, deadline: Duration) -> Result<Opened, ScanError> {
        self.installed()?;
        if !is_fake_id(&stored.id) {
            return Err(ScanError::BadId);
        }
        let listed = self.listing(deadline)?;
        let by_id = self
            .devices
            .iter()
            .find(|device| device.info.id == stored.id);
        let (device, info, renamed_from) = match by_id {
            Some(device) if device.listed && same_device(&device.info, stored) => {
                (device, device.info.clone(), None)
            }
            Some(device) if !device.listed => (device, stored.clone(), None),
            // Listed as another maker or model, so the name is another device's now; or not
            // there at all.
            _ => {
                let device = renamed(&listed, stored).ok_or(ScanError::Gone)?;
                (device, device.info.clone(), Some(stored.id.clone()))
            }
        };
        if let Some(error) = &device.refuses_open {
            return Err(error.clone());
        }
        Ok(Opened {
            device: Box::new(OpenDevice {
                script: device.clone(),
            }),
            info,
            renamed_from,
        })
    }
}

/// Whether `id` is `fake:` and a name of at least one character.
fn is_fake_id(id: &DeviceId) -> bool {
    id.0.strip_prefix("fake:")
        .is_some_and(|name| !name.is_empty())
}

/// Whether `id` names a USB connection, `fake:libusb:<bus>:<device>`, each number one or more
/// ASCII digits.
fn is_usb_name(id: &DeviceId) -> bool {
    let numbers = |part: &str| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit());
    id.0.strip_prefix("fake:libusb:")
        .and_then(|rest| rest.split_once(':'))
        .is_some_and(|(bus, device)| numbers(bus) && numbers(device))
}

/// Whether `found` has `stored`'s maker and model.
fn same_device(found: &DeviceInfo, stored: &DeviceInfo) -> bool {
    found.maker == stored.maker && found.model == stored.model
}

/// The device a stored USB connection's name was renamed to: the one listed under a name of that
/// form with the stored maker and model, where exactly one is.
fn renamed<'a>(listed: &[&'a FakeDevice], stored: &DeviceInfo) -> Option<&'a FakeDevice> {
    if !is_usb_name(&stored.id) {
        return None;
    }
    let mut found = listed
        .iter()
        .filter(|device| is_usb_name(&device.info.id) && same_device(&device.info, stored));
    match (found.next(), found.next()) {
        (Some(device), None) => Some(device),
        _ => None,
    }
}

/// A scripted device, and a handle on its paper.
///
/// A clone shares the device's paper (its feeder, its glass and the count of sheets fed), and
/// keeps the script as it was when cloned: script a device fully before handing it to a
/// [`FakeBackend`].
#[derive(Clone, Debug)]
pub struct FakeDevice {
    info: DeviceInfo,
    caps: Caps,
    listed: bool,
    refuses_open: Option<ScanError>,
    side_known: SideKnown,
    warms_up: bool,
    window: Option<WindowClosed>,
    paper: Arc<Mutex<PaperState>>,
}

/// A device's paper, shared by its clones.
#[derive(Debug)]
struct PaperState {
    feeder: VecDeque<Sheet>,
    glass: Sheet,
    fed: u32,
}

impl FakeDevice {
    /// The device `fake:<name>`, by `maker`, of `model`, its name to show the two together; with
    /// the default caps (the module's docs, "Its capabilities"), listed, its feeder empty and a
    /// plain sheet on its glass.
    ///
    /// # Panics
    ///
    /// Where `name` is empty, which no stored id can open: a script's mistake.
    pub fn new(name: &str, maker: &str, model: &str) -> FakeDevice {
        assert!(
            !name.is_empty(),
            "a fake device's name is at least one character"
        );
        let shown = [maker, model]
            .into_iter()
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join(" ");
        FakeDevice {
            info: DeviceInfo::new(
                DeviceId(format!("fake:{name}")),
                shown,
                maker.to_owned(),
                model.to_owned(),
                Driver::Fake,
            ),
            caps: default_caps(),
            listed: true,
            refuses_open: None,
            side_known: SideKnown::Inferred,
            warms_up: false,
            window: None,
            paper: Arc::new(Mutex::new(PaperState {
                feeder: VecDeque::new(),
                glass: Sheet::new(),
                fed: 0,
            })),
        }
    }

    /// The device as a listing gives it.
    pub fn info(&self) -> DeviceInfo {
        self.info.clone()
    }

    /// The device with `caps` in place of the default ones. Their `settings_window` is the
    /// device's own: true where [`FakeDevice::settings_window`] scripts one, whatever `caps` say.
    ///
    /// # Panics
    ///
    /// On caps the model forbids, a script's mistake: no source; a source with no format; a list
    /// of resolutions that's empty, or holds 0; a range whose `min` is 0 or past its `max`; a
    /// largest paper whose width or height isn't finite and above 0.
    pub fn caps(mut self, caps: Caps) -> FakeDevice {
        check_caps(&caps);
        self.caps = caps;
        self
    }

    /// The device left out of listings: it opens by its stored id alone, with the stored info,
    /// unchecked, as a name a driver opens but lists another way would.
    pub fn unlisted(mut self) -> FakeDevice {
        self.listed = false;
        self
    }

    /// The device refusing to open with `error` once it's found, such as [`ScanError::Busy`] or
    /// [`ScanError::AccessDenied`].
    pub fn refuses_open(mut self, error: ScanError) -> FakeDevice {
        self.refuses_open = Some(error);
        self
    }

    /// The device saying each page's side ([`SideKnown::Reported`]), where by default the side is
    /// worked out from the order ([`SideKnown::Inferred`]).
    pub fn reports_sides(mut self) -> FakeDevice {
        self.side_known = SideKnown::Reported;
        self
    }

    /// The device warming up at each batch: `Event::Status(Status::WarmingUp)` after the
    /// settings are applied.
    pub fn warms_up(mut self) -> FakeDevice {
        self.warms_up = true;
        self
    }

    /// The device with a settings window of its own, which closes as `closes` says each time it's
    /// opened.
    pub fn settings_window(mut self, closes: WindowClosed) -> FakeDevice {
        self.window = Some(closes);
        self
    }

    /// Puts `sheets` in the feeder, after any already there; at any time, between batches or
    /// during one.
    pub fn load(&self, sheets: impl IntoIterator<Item = Sheet>) {
        self.paper().feeder.extend(sheets);
    }

    /// Puts `sheet` on the glass, which the flatbed scans: a plain [`Sheet::new`] until then.
    pub fn glass(&self, sheet: Sheet) {
        self.paper().glass = sheet;
    }

    /// The sheets taken from the feeder so far, by every batch.
    pub fn fed(&self) -> u32 {
        self.paper().fed
    }

    /// The sheets in the feeder now.
    pub fn in_feeder(&self) -> usize {
        self.paper().feeder.len()
    }

    /// The device's paper, locked: never held across a `send`, a call on a progress, or a wait.
    fn paper(&self) -> MutexGuard<'_, PaperState> {
        // Nothing panics while it's held, but a test's thread may have: the state is whole.
        self.paper.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// A feeder with duplex and a flatbed, the five formats on each.
fn default_caps() -> Caps {
    let formats = vec![
        Format::Bw1,
        Format::Gray8,
        Format::Gray16,
        Format::Rgb8,
        Format::Rgb16,
    ];
    let mut feeder = SourceCaps::new(
        Source::Feeder,
        Resolutions::Range {
            min: 50,
            max: 600,
            step: NonZeroU32::MIN,
        },
        formats.clone(),
    );
    feeder.duplex = true;
    feeder.max_area = Some(SizeMm::LEGAL);
    feeder.paper_auto = true;
    let mut flatbed = SourceCaps::new(
        Source::Flatbed,
        Resolutions::List(vec![75, 150, 300, 600]),
        formats,
    );
    flatbed.max_area = Some(SizeMm {
        width: 215.9,
        height: 297.0,
    });
    Caps::new(vec![feeder, flatbed])
}

/// Whether a size's width and height are finite and above 0.
fn is_paper(size: SizeMm) -> bool {
    [size.width, size.height]
        .iter()
        .all(|mm| mm.is_finite() && *mm > 0.0)
}

/// Panics on caps the model forbids ([`FakeDevice::caps`]).
fn check_caps(caps: &Caps) {
    assert!(!caps.sources.is_empty(), "a fake device has a source");
    for source in &caps.sources {
        let name = source.source;
        assert!(!source.formats.is_empty(), "{name:?} has no format");
        match &source.resolutions {
            Resolutions::List(values) => {
                assert!(!values.is_empty(), "{name:?} lists no resolution");
                assert!(!values.contains(&0), "{name:?} lists 0 dpi");
            }
            Resolutions::Range { min, max, .. } => {
                assert!(*min > 0, "{name:?}'s range starts at 0 dpi");
                assert!(min <= max, "{name:?}'s range has its min past its max");
            }
        }
        if let Some(area) = source.max_area {
            assert!(is_paper(area), "{name:?}'s largest paper is {area:?}");
        }
    }
}

/// A sheet of paper, for a feeder or the glass.
///
/// Both its sides are drawn by [`pattern`] at the settings a batch applies, unless a page is given
/// for a side ([`Sheet::side`]).
#[derive(Clone, Debug, PartialEq)]
pub struct Sheet {
    size: SizeMm,
    front: Option<Pixels>,
    back: Option<Pixels>,
}

impl Default for Sheet {
    fn default() -> Sheet {
        Sheet::new()
    }
}

impl Sheet {
    /// A Letter sheet, both its sides drawn.
    pub fn new() -> Sheet {
        Sheet {
            size: SizeMm::LETTER,
            front: None,
            back: None,
        }
    }

    /// The sheet at `size`, which a batch scans where its paper is [`Paper::Auto`], cut to the
    /// source's largest paper.
    ///
    /// # Panics
    ///
    /// Where the width or the height isn't finite and above 0: a script's mistake.
    ///
    /// [`Paper::Auto`]: crate::Paper::Auto
    pub fn size(mut self, size: SizeMm) -> Sheet {
        assert!(is_paper(size), "a sheet of {size:?}");
        self.size = size;
        self
    }

    /// The sheet with `pixels` as `side`'s page, delivered as given, whatever the settings.
    pub fn side(mut self, side: Side, pixels: Pixels) -> Sheet {
        match side {
            Side::Front => self.front = Some(pixels),
            Side::Back => self.back = Some(pixels),
        }
        self
    }

    /// The page given for `side`, if one is.
    fn given(&self, side: Side) -> Option<&Pixels> {
        match side {
            Side::Front => self.front.as_ref(),
            Side::Back => self.back.as_ref(),
        }
    }
}

/// The device a [`FakeBackend`] opens: its script, and through it the device's paper.
struct OpenDevice {
    script: FakeDevice,
}

impl Device for OpenDevice {
    fn caps(&mut self) -> Result<Caps, ScanError> {
        let mut caps = self.script.caps.clone();
        caps.settings_window = self.script.window.is_some();
        Ok(caps)
    }

    fn scan(
        &mut self,
        settings: &Settings,
        first_sheet: u32,
        events: &SyncSender<Event>,
        progress: &dyn Progress,
    ) -> Result<Done, ScanError> {
        batch::scan(&self.script, settings, first_sheet, events, progress)
    }

    fn settings_window(&mut self, _owner: Option<WindowOwner>) -> Result<WindowClosed, ScanError> {
        self.script.window.ok_or(ScanError::Unsupported)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(id: &str) -> DeviceId {
        DeviceId(id.to_owned())
    }

    #[test]
    fn a_fake_id_is_fake_and_a_name() {
        assert!(is_fake_id(&id("fake:a")));
        assert!(is_fake_id(&id("fake::")));
        assert!(!is_fake_id(&id("fake:")));
        assert!(!is_fake_id(&id("sane:fake:a")));
        assert!(!is_fake_id(&id("Fake:a")));
        assert!(!is_fake_id(&id("")));
    }

    #[test]
    fn a_usb_name_is_libusb_and_two_numbers() {
        assert!(is_usb_name(&id("fake:libusb:001:004")));
        assert!(is_usb_name(&id("fake:libusb:1:4")));
        for not in [
            "fake:libusb:001",
            "fake:libusb:001:",
            "fake:libusb::004",
            "fake:libusb:001:004:1",
            "fake:libusb:00a:004",
            "fake:libusb:001:+4",
            "fake:usb:001:004",
            "sane:libusb:001:004",
            "fake:fi-6130dj:12345",
        ] {
            assert!(!is_usb_name(&id(not)), "{not}");
        }
    }

    #[test]
    fn a_device_is_shown_by_its_maker_and_model() {
        assert_eq!(
            FakeDevice::new("a", "Maker", "Model").info().name,
            "Maker Model"
        );
        assert_eq!(FakeDevice::new("a", "", "Model").info().name, "Model");
        assert_eq!(FakeDevice::new("a", "Maker", "").info().name, "Maker");
    }
}
