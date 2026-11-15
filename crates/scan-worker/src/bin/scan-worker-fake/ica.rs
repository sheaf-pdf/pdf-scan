//! The `ica` and `ica-fake` scenarios: `scan-device`'s ImageCaptureCore backend served in a worker
//! process. `ica` serves the real backend from the worker's `main`, so on macOS it lists through
//! the framework on the process's main thread, as an app's worker does, and elsewhere it answers
//! `NotInstalled`; `ica-fake` serves the backend over the scripted framework, on every system.

use std::{
    env, fs,
    process::ExitCode,
    sync::mpsc::{self, SyncSender},
    thread,
    time::{Duration, Instant},
};

use scan_device::ica::IcaBackend;
use scan_device::ica::fake_ica::{
    DEPTH_1, DEPTH_8, DEPTH_16, DEVICE_CAMERA, DOCUMENT_A4, DOCUMENT_A5, DOCUMENT_A6, FakeBands,
    FakeCall, FakeFault, FakeIcaDevice, FakeIcaHandle, FakeSheet, LOCATION_LOCAL,
    RETURN_SCANNER_FAILED_TO_COMPLETE_SCAN, feeder, flatbed, ica_error, network_props,
    scanner_props,
};
use scan_device::in_progress::Progress;
use scan_device::{
    Backend, Caps, Device, DeviceInfo, Done, Event, Opened, ScanError, Settings, Side,
    WindowClosed, WindowOwner,
};

/// The scripted office scanner's persistent id: its `DeviceId` is `ica:` and this.
pub const OFFICE: &str = "0x14100000046d0001";

/// The scripted network scanner's persistent id.
pub const NETWORK: &str = "bonjour-office-2";

/// The sheet whose `Sheet` the `feeding` scenario holds until its batch is cancelled.
const HELD_AT: u32 = 2;

/// The longest the `feeding` scenario holds a batch for its cancel.
const HOLD: Duration = Duration::from_secs(5);

/// `ica`: serves the real [`IcaBackend::new()`] over `scan_worker::serve`, from `main`.
pub fn main(args: &[String]) -> ExitCode {
    if let Some(other) = args.first() {
        eprintln!("scan-worker-fake: ica: no argument {other:?}");
        return ExitCode::from(2);
    }
    scan_worker::serve(vec![Box::new(IcaBackend::new())])
}

/// `ica-fake <office|jam|feeding|removed>`: serves [`IcaBackend::from_fake`] over a scripted
/// framework holding the office scanner (a flatbed, and a duplex feeder of A6, A5 and A4), a
/// network scanner and a camera, the office scanner's feeder loaded as the scenario says:
///
/// - `office`: two sheets, the first warming the scanner up, its bands of seven rows padded and
///   their 16-bit samples high byte first;
/// - `jam`: four sheets, the third's front failing as a jam would (the framework has no code for
///   a jam: its scan fails to complete);
/// - `feeding`: four sheets, a batch held as sheet 2's `Sheet` comes, until it's cancelled (at
///   most 5 s), the hold signalled by writing `SCAN_WORKER_TEST_SIGNAL_FILE`; as the worker ends,
///   what the scripted scanner was asked and fed, in order, written to
///   `SCAN_WORKER_TEST_SUMMARY_FILE` as a JSON array (`"scan"`, `"fed 1 front"`, `"cancel"`);
/// - `removed`: three sheets, the scanner unplugged two bands into sheet 2's front.
pub fn fake_main(args: &[String]) -> ExitCode {
    let mode = args.first().map_or("office", String::as_str);
    if let Some(other) = args.get(1) {
        eprintln!("scan-worker-fake: ica-fake {mode}: no argument {other:?}");
        return ExitCode::from(2);
    }
    let Some(office) = office(mode) else {
        eprintln!("scan-worker-fake: ica-fake: no mode {mode:?}");
        return ExitCode::from(2);
    };
    let fake = FakeIcaHandle::new();
    fake.add_device(office);
    let mut network = FakeIcaDevice::new(network_props(NETWORK, "Network scanner"));
    network.found_after = Duration::from_millis(1);
    fake.add_device(network);
    let mut camera = scanner_props("camera-1", "Camera");
    camera.type_bits = DEVICE_CAMERA | LOCATION_LOCAL;
    fake.add_device(FakeIcaDevice::new(camera));

    let backend = IcaBackend::from_fake(&fake);
    if mode != "feeding" {
        return scan_worker::serve(vec![Box::new(backend)]);
    }
    let code = scan_worker::serve(vec![Box::new(HeldAtSheet(backend))]);
    if let Some(summary) = env::var_os("SCAN_WORKER_TEST_SUMMARY_FILE") {
        let record: Vec<String> = fake.calls().iter().filter_map(said).collect();
        let _ = fs::write(summary, serde_json::json!(record).to_string());
    }
    code
}

/// The office scanner, its feeder loaded as `mode` says; `None` for a mode there isn't.
fn office(mode: &str) -> Option<FakeIcaDevice> {
    let resolutions = [50, 75, 100, 150, 300];
    let depths = [DEPTH_1, DEPTH_8, DEPTH_16];
    let mut stack = match mode {
        "office" => FakeSheet::stack(1..=2),
        "jam" | "feeding" => FakeSheet::stack(1..=4),
        "removed" => FakeSheet::stack(1..=3),
        _ => return None,
    };
    match mode {
        "office" => stack[0] = FakeSheet::new(1).with_fault(Side::Front, FakeFault::WarmingUp),
        "jam" => {
            stack[2] = FakeSheet::new(3).with_fault(
                Side::Front,
                FakeFault::Error(ica_error(RETURN_SCANNER_FAILED_TO_COMPLETE_SCAN)),
            );
        }
        "removed" => {
            stack[1] =
                FakeSheet::new(2).with_fault(Side::Front, FakeFault::Removed { after_bands: 2 });
        }
        _ => {}
    }
    let mut device = FakeIcaDevice::new(scanner_props(OFFICE, "Office scanner"))
        .with_unit(flatbed(&resolutions, &depths, 1.1, 0.9))
        .with_unit(feeder(
            &resolutions,
            &depths,
            &[DOCUMENT_A6, DOCUMENT_A5, DOCUMENT_A4],
            true,
            8.5,
            14.0,
        ))
        .load(stack);
    if mode == "office" {
        device.bands = FakeBands {
            rows: 7,
            padding: 3,
            big_endian: true,
        };
    }
    Some(device)
}

/// A call the scripted scanner was asked, or a side it fed, in the summary's words; the rest
/// left out.
fn said(call: &FakeCall) -> Option<String> {
    match call {
        FakeCall::RequestScan { .. } => Some("scan".to_owned()),
        FakeCall::CancelScan => Some("cancel".to_owned()),
        FakeCall::Fed { sheet, side } => Some(format!(
            "fed {sheet} {}",
            match side {
                Side::Front => "front",
                Side::Back => "back",
            }
        )),
        _ => None,
    }
}

/// The `feeding` scenario's backend: its devices' batches held as [`HELD_AT`]'s `Sheet` comes.
struct HeldAtSheet(IcaBackend);

impl Backend for HeldAtSheet {
    fn list(&mut self, deadline: Duration) -> Result<Vec<DeviceInfo>, ScanError> {
        self.0.list(deadline)
    }

    fn open(&mut self, stored: &DeviceInfo, deadline: Duration) -> Result<Opened, ScanError> {
        let mut opened = self.0.open(stored, deadline)?;
        opened.device = Box::new(HeldDevice(opened.device));
        Ok(opened)
    }
}

/// A device whose batch's events pass through a rendezvous of their own, held as
/// [`HELD_AT`]'s `Sheet` comes until the batch's progress says it's cancelled: the backend waits
/// at its next `send` meanwhile, so the cancel comes, through the client and the worker's
/// input, while sheet 2's front is whole and its back not yet scanned, as an app's cancel
/// would as it takes the sheet's event.
struct HeldDevice(Box<dyn Device>);

impl Device for HeldDevice {
    fn caps(&mut self) -> Result<Caps, ScanError> {
        self.0.caps()
    }

    fn scan(
        &mut self,
        settings: &Settings,
        first_sheet: u32,
        events: &SyncSender<Event>,
        progress: &dyn Progress,
    ) -> Result<Done, ScanError> {
        let (held, taken) = mpsc::sync_channel(0);
        let forward = events.clone();
        thread::scope(|scope| {
            scope.spawn(move || {
                for event in taken {
                    if event == Event::Sheet(HELD_AT) {
                        hold(progress);
                    }
                    // The worker's drain gone: the backend's next `send` fails, as it would.
                    if forward.send(event).is_err() {
                        break;
                    }
                }
            });
            let done = self.0.scan(settings, first_sheet, &held, progress);
            drop(held);
            done
        })
    }

    fn settings_window(&mut self, owner: Option<WindowOwner>) -> Result<WindowClosed, ScanError> {
        self.0.settings_window(owner)
    }
}

/// The hold: signalled, then until `progress` is cancelled, at most [`HOLD`].
fn hold(progress: &dyn Progress) {
    if let Some(signal) = env::var_os("SCAN_WORKER_TEST_SIGNAL_FILE") {
        let _ = fs::write(signal, b"held");
    }
    let start = Instant::now();
    while !progress.is_cancelled() && start.elapsed() < HOLD {
        thread::sleep(Duration::from_millis(5));
    }
}
