//! The scripted devices the test worker serves, by scenario. The crate's integration tests take
//! this file too, and run the same scripts in their own process, so a page through a worker is
//! compared with the one the scripted backend makes.

use std::{num::NonZeroU32, sync::mpsc::SyncSender, time::Duration};

use scan_device::fake::{FakeBackend, FakeDevice, Fault, Sheet, When};
use scan_device::in_progress::{Level, Progress};
use scan_device::{
    Backend, Caps, Device, DeviceInfo, Done, Encoding, Event, Format, Opened, Resolutions,
    ScanError, Settings, Side, SizeMm, Source, SourceCaps, WindowClosed, WindowOwner,
};

/// What a scenario whose driver fails writes to standard error first, so a test can find it
/// in the error the client gives.
pub const FAILING: &str = "scan-worker-fake: the driver is about to fail";

/// What the `talking` scenario's driver says at each batch, as a warning.
pub const SAID: &str = "the lamp is \"old\": replace it soon";

/// The device most scenarios have: `fake:desk`, by Maker, of Model, the scripted backend's
/// default caps (a feeder with duplex, and a flatbed).
pub fn desk() -> FakeDevice {
    FakeDevice::new("desk", "Maker", "Model")
}

/// `n` plain Letter sheets.
pub fn sheets(n: usize) -> Vec<Sheet> {
    vec![Sheet::new(); n]
}

/// `n` plain sheets, `fault` scripted on sheet `at`'s `side` (counted from 1), to fire `when`.
fn faulted(n: usize, at: usize, side: Side, when: When, fault: Fault) -> Vec<Sheet> {
    let mut sheets = sheets(n);
    sheets[at - 1] = Sheet::new().fault(side, when, fault);
    sheets
}

/// The scenario `name`'s backend, with `count` sheets where it takes a count; `None` for a name
/// that isn't one of these.
pub fn backend(name: &str, count: usize) -> Option<Box<dyn Backend>> {
    let one =
        |device: FakeDevice| -> Box<dyn Backend> { Box::new(FakeBackend::new().with(device)) };
    let loaded = |device: FakeDevice, sheets: Vec<Sheet>| {
        device.load(sheets);
        one(device)
    };
    Some(match name {
        // A feeder of `count` sheets.
        "feeder" => loaded(desk(), sheets(count)),
        // The same, every source offering every encoded form.
        "encodings" => loaded(desk().caps(encoding_caps()), sheets(count)),
        // Five sheets: sheet 2's back jams midway, and sheet 4's front as it starts.
        "jams" => {
            let mut five = sheets(5);
            five[1] = Sheet::new().fault(Side::Back, When::Midway, Fault::Error(ScanError::Jammed));
            five[3] = Sheet::new().fault(Side::Front, When::Start, Fault::Error(ScanError::Jammed));
            loaded(desk(), five)
        }
        // Three sheets, sheet 2 taken with another as it starts.
        "double-feed" => loaded(
            desk(),
            faulted(
                3,
                2,
                Side::Front,
                When::Start,
                Fault::Error(ScanError::DoubleFeed),
            ),
        ),
        // Three sheets, the cover opened as sheet 2 starts.
        "cover-open" => loaded(
            desk(),
            faulted(
                3,
                2,
                Side::Front,
                When::Start,
                Fault::Error(ScanError::CoverOpen),
            ),
        ),
        // Three sheets, the device gone midway through sheet 2's front, and after.
        "gone" => loaded(
            desk(),
            faulted(
                3,
                2,
                Side::Front,
                When::Midway,
                Fault::Error(ScanError::Gone),
            ),
        ),
        // A feeder of `count` sheets, the lamp warming up at each batch.
        "warm-up" => loaded(desk().warms_up(), sheets(count)),
        // A feeder of `count` sheets, whose driver says each page's side.
        "reports-sides" => loaded(desk().reports_sides(), sheets(count)),
        // A device with a settings window of its own, which the user accepts.
        "window" => one(desk().settings_window(WindowClosed::Accepted)),
        // A scanner on a USB connection: stored under one name, it's found under another.
        "renamed" => one(FakeDevice::new("libusb:001:005", "Maker", "Model")),
        // A system without the driver.
        "not-installed" => Box::new(FakeBackend::new().with(desk()).not_installed()),
        // `count` sheets, the driver failing midway through sheet 3's front: hung, crashed, or
        // exited with 42.
        "hang" => loaded(
            desk(),
            faulted(count, 3, Side::Front, When::Midway, Fault::Hang),
        ),
        "crash" => loaded(
            desk(),
            faulted(count, 3, Side::Front, When::Midway, Fault::Crash),
        ),
        "exit" => loaded(
            desk(),
            faulted(count, 3, Side::Front, When::Midway, Fault::Exit(42)),
        ),
        // `count` sheets, the batch held midway through sheet 2's front, or as its back starts,
        // until it's cancelled.
        "await-cancel-midway" => loaded(
            desk(),
            faulted(count, 2, Side::Front, When::Midway, Fault::AwaitCancel),
        ),
        "await-cancel-back" => loaded(
            desk(),
            faulted(count, 2, Side::Back, When::Start, Fault::AwaitCancel),
        ),
        // Every batch finds `count` new sheets in the feeder, as the user puts more in.
        "scan-more" => each_batch(count, None),
        // The same, the driver saying `SAID` as each batch starts.
        "talking" => each_batch(count, Some((Level::Warn, SAID))),
        _ => return None,
    })
}

/// The default caps, every source offering every encoded form.
fn encoding_caps() -> Caps {
    let all = vec![
        Encoding::Jpeg,
        Encoding::Png,
        Encoding::Tiff,
        Encoding::Bmp,
        Encoding::CcittG4,
    ];
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
    feeder.encodings = all.clone();
    feeder.max_area = Some(SizeMm::LEGAL);
    feeder.paper_auto = true;
    let mut flatbed = SourceCaps::new(
        Source::Flatbed,
        Resolutions::List(vec![75, 150, 300, 600]),
        formats,
    );
    flatbed.encodings = all;
    flatbed.max_area = Some(SizeMm::A4);
    Caps::new(vec![feeder, flatbed])
}

/// `desk`, finding `more` new sheets in its feeder at each batch, its driver saying `says` as
/// each starts.
fn each_batch(more: usize, says: Option<(Level, &'static str)>) -> Box<dyn Backend> {
    let scanner = desk();
    Box::new(EachBatch {
        backend: FakeBackend::new().with(scanner.clone()),
        scanner,
        more,
        says,
    })
}

/// A backend whose devices find `more` new sheets in the feeder at each batch, their driver
/// saying `says` as each starts.
struct EachBatch {
    backend: FakeBackend,
    scanner: FakeDevice,
    more: usize,
    says: Option<(Level, &'static str)>,
}

impl Backend for EachBatch {
    fn list(&mut self, deadline: Duration) -> Result<Vec<DeviceInfo>, ScanError> {
        self.backend.list(deadline)
    }

    fn open(&mut self, device: &DeviceInfo, deadline: Duration) -> Result<Opened, ScanError> {
        let mut opened = self.backend.open(device, deadline)?;
        opened.device = Box::new(EachBatchDevice {
            device: opened.device,
            scanner: self.scanner.clone(),
            more: self.more,
            says: self.says,
        });
        Ok(opened)
    }
}

/// An [`EachBatch`] backend's device.
struct EachBatchDevice {
    device: Box<dyn Device>,
    scanner: FakeDevice,
    more: usize,
    says: Option<(Level, &'static str)>,
}

impl Device for EachBatchDevice {
    fn caps(&mut self) -> Result<Caps, ScanError> {
        self.device.caps()
    }

    fn scan(
        &mut self,
        settings: &Settings,
        first_sheet: u32,
        events: &SyncSender<Event>,
        progress: &dyn Progress,
    ) -> Result<Done, ScanError> {
        self.scanner.load(sheets(self.more));
        if let Some((level, text)) = self.says {
            progress.message(level, text);
        }
        self.device.scan(settings, first_sheet, events, progress)
    }

    fn settings_window(&mut self, owner: Option<WindowOwner>) -> Result<WindowClosed, ScanError> {
        self.device.settings_window(owner)
    }
}
