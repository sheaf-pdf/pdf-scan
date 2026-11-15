//! The ImageCaptureCore backend through a real worker process. Over the scripted framework
//! (`ica-fake`), on every system: the listing (scanners only, by their ids) and an open by the id
//! alone, the caps of a flatbed and a duplex feeder, a flatbed's page and a duplex batch, every
//! page `pattern`'s byte for byte across the pipe, with warming up and the bytes' progress; a
//! cancel through the client, finishing the sheet in flight, the next batch giving the rest; an
//! error keeping the pages before it; the device removed, and every call after it. And the real
//! backend (`ica`): off macOS `NotInstalled`, after `BadId`; on macOS a listing and an open
//! through the framework, the worker's main thread running its run loop while it waits.
//!
//! The scripted framework is our model of ImageCaptureCore as its documentation reads, not
//! evidence of it: these prove the backend's logic and the worker's over that reading.

mod support;

use std::fs;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use scan_worker::scan_device::in_progress::{Tracker, Unit};
use scan_worker::scan_device::{
    DeviceId, DeviceInfo, Done, Driver, Event, Format, Paper, Pixels, ScanError, Settings, Source,
    Status,
};
use scan_worker::{Client, Error, WorkerCommand};

use support::{AT_MOST, is_scan, outline, pages, pages_are_pattern_s, scan, soon, worker};

/// The scripted office scanner's persistent id, as the test worker's `ica.rs` has it.
const OFFICE: &str = "0x14100000046d0001";
/// The scripted network scanner's.
const NETWORK: &str = "bonjour-office-2";

/// A6, the scripted feeder's smallest document type: 207 by 291 pixels at 50 dpi.
const A6: scan_worker::scan_device::SizeMm = scan_worker::scan_device::SizeMm {
    width: 105.0,
    height: 148.0,
};

fn ica_fake(mode: &str) -> WorkerCommand {
    worker("ica-fake").arg(mode)
}

fn stored(native: &str) -> DeviceInfo {
    DeviceInfo::new(
        DeviceId(format!("ica:{native}")),
        "Stored".to_owned(),
        String::new(),
        String::new(),
        Driver::Ica,
    )
}

fn feeder(duplex: bool, format: Format) -> Settings {
    let mut settings = Settings::new(Source::Feeder, 50, format);
    settings.duplex = duplex;
    settings.paper = Paper::Size(A6);
    settings
}

/// The raw pages' sizes among `events`.
fn sizes(events: &[Event]) -> Vec<(u32, u32)> {
    pages(events)
        .into_iter()
        .map(|page| match &page.pixels {
            Pixels::Raw(image) => (image.width(), image.height()),
            other => panic!("a raw page, not {other:?}"),
        })
        .collect()
}

fn is_driver(ended: &Result<Done, Error>, has: &str) -> bool {
    matches!(
        ended,
        Err(Error::Scan(ScanError::Driver { message, shown: false })) if message.contains(has)
    )
}

#[test]
fn ica_worker_lists_opens_reads_caps_and_scans_byte_for_byte() {
    let client = Client::new().worker(Driver::Ica, ica_fake("office"));

    // The scanners alone, the attached one first, by their ids; no maker or model.
    let listed = client.list(Driver::Ica, AT_MOST).expect("a listing");
    let ids: Vec<&str> = listed.iter().map(|info| info.id.0.as_str()).collect();
    assert_eq!(
        ids,
        [format!("ica:{OFFICE}"), format!("ica:{NETWORK}")],
        "the camera isn't listed"
    );
    assert_eq!(listed[0].name, "Office scanner");
    assert_eq!(listed[1].name, "Network scanner");
    for info in &listed {
        assert_eq!(info.driver, Driver::Ica);
        assert_eq!((info.maker.as_str(), info.model.as_str()), ("", ""));
    }

    let mut device = client
        .open(&listed[0], AT_MOST)
        .expect("the office scanner");
    assert_eq!(device.renamed_from(), None);
    assert_eq!(device.info().name, "Office scanner");

    let caps = device.caps().expect("its caps");
    assert!(!caps.settings_window);
    assert_eq!(caps.sources.len(), 2);
    let flatbed = &caps.sources[0];
    assert_eq!(flatbed.source, Source::Flatbed);
    assert!(!flatbed.duplex && !flatbed.paper_auto && flatbed.encodings.is_empty());
    assert_eq!(
        flatbed.formats,
        [
            Format::Bw1,
            Format::Gray8,
            Format::Gray16,
            Format::Rgb8,
            Format::Rgb16
        ]
    );
    let bed = flatbed.max_area.expect("the bed");
    assert!((bed.width - 27.94).abs() < 1e-3 && (bed.height - 22.86).abs() < 1e-3);
    let feeder_caps = &caps.sources[1];
    assert_eq!(feeder_caps.source, Source::Feeder);
    assert!(feeder_caps.duplex && !feeder_caps.paper_auto);
    assert_eq!(
        feeder_caps.max_area,
        Some(scan_worker::scan_device::SizeMm::A4)
    );
    assert!(matches!(
        device.settings_window(None),
        Err(Error::Scan(ScanError::Unsupported))
    ));

    // A flatbed's page: its bands of seven rows, each padded, put together.
    let (events, done) = scan(
        &mut device,
        &Settings::new(Source::Flatbed, 50, Format::Gray8),
        1,
    );
    assert_eq!(done.expect("a flatbed's page"), Done::OnePage);
    assert_eq!(outline(&events), ["applied", "sheet 1", "page 1 front"]);
    pages_are_pattern_s(&events);
    assert_eq!(sizes(&events), [(55, 45)]);
    assert_eq!(pages(&events)[0].dpi, (50.0, 50.0));

    // A duplex batch at 16 bits a sample, sent high byte first: every page the pattern's,
    // little-endian, across the pipe; the warm-up said between pages; the bytes' progress.
    let tracker = Tracker::new();
    let (sent, received) = mpsc::sync_channel(64);
    let done = device
        .scan(&feeder(true, Format::Rgb16), 1, &sent, &tracker)
        .expect("a duplex batch");
    drop(sent);
    let events: Vec<Event> = received.into_iter().collect();
    assert_eq!(done, Done::FeederEmpty);
    assert_eq!(
        outline(&events),
        [
            "applied",
            "sheet 1",
            "page 1 front",
            "status",
            "page 1 back",
            "sheet 2",
            "page 2 front",
            "page 2 back"
        ]
    );
    assert_eq!(events[3], Event::Status(Status::WarmingUp));
    let Event::Applied(applied) = &events[0] else {
        panic!("applied first");
    };
    assert!(applied.clamped.is_empty(), "{:?}", applied.clamped);
    pages_are_pattern_s(&events);
    assert_eq!(sizes(&events), [(207, 291); 4]);
    let page_bytes = 207 * 291 * 6;
    let progress = tracker.snapshot();
    assert_eq!(progress.unit, Unit::Bytes);
    assert_eq!(
        (progress.done, progress.total),
        (page_bytes, Some(page_bytes))
    );

    // The feeder's empty now: the model's empty feeder, before any page.
    let (events, ended) = scan(&mut device, &feeder(false, Format::Gray8), 3);
    assert!(is_scan(&ended, &ScanError::FeederEmpty), "{ended:?}");
    assert_eq!(outline(&events), ["applied"]);

    device.close().expect("the worker closed");
}

#[test]
fn ica_worker_opens_by_the_id_alone_and_refuses_an_id_it_doesn_t_form() {
    let client = Client::new().worker(Driver::Ica, ica_fake("office"));
    // A stored device found by its id, whatever its stored name.
    let device = client.open(&stored(NETWORK), AT_MOST).expect("by its id");
    assert_eq!(device.renamed_from(), None);
    assert_eq!(device.info().name, "Network scanner");
    device.close().expect("the worker closed");
    // Not there: gone once the listing's snapshot would be taken, long before the deadline.
    assert!(matches!(
        client.open(&stored("unplugged"), Duration::from_secs(60)),
        Err(Error::Scan(ScanError::Gone))
    ));
    for bad in ["", "bad\0id"] {
        assert!(
            matches!(
                client.open(&stored(bad), AT_MOST),
                Err(Error::Scan(ScanError::BadId))
            ),
            "{bad:?}"
        );
    }
}

#[test]
fn ica_worker_cancel_through_the_client_finishes_the_sheet_and_the_next_batch_gives_the_rest() {
    let dir = tempfile::tempdir().expect("a directory");
    let signal = dir.path().join("held");
    let summary = dir.path().join("summary.json");
    let command = ica_fake("feeding")
        .env("SCAN_WORKER_TEST_SIGNAL_FILE", &signal)
        .env("SCAN_WORKER_TEST_SUMMARY_FILE", &summary);
    let client = Client::new().worker(Driver::Ica, command);
    let mut device = client.open(&stored(OFFICE), AT_MOST).expect("opened");

    let tracker = Tracker::new();
    let cancel = tracker.clone();
    let (sent, received) = mpsc::sync_channel(64);
    let batch = thread::spawn(move || {
        let done = device.scan(&feeder(true, Format::Gray8), 1, &sent, &tracker);
        (device, done)
    });
    // The worker holds the batch as sheet 2's front is whole: the cancel comes then.
    assert!(soon(|| signal.exists()), "the worker never held sheet 2");
    cancel.cancel();
    let (mut device, done) = batch.join().expect("the batch's thread");
    assert_eq!(done.expect("a cancelled batch"), Done::Cancelled);
    let events: Vec<Event> = received.into_iter().collect();
    assert_eq!(
        outline(&events),
        [
            "applied",
            "sheet 1",
            "page 1 front",
            "page 1 back",
            "sheet 2",
            "page 2 front",
            "page 2 back"
        ]
    );
    pages_are_pattern_s(&events);

    // The next batch gives the rest.
    let (events, done) = scan(&mut device, &feeder(true, Format::Gray8), 3);
    assert_eq!(done.expect("the rest"), Done::FeederEmpty);
    assert_eq!(
        outline(&events),
        [
            "applied",
            "sheet 3",
            "page 3 front",
            "page 3 back",
            "sheet 4",
            "page 4 front",
            "page 4 back"
        ]
    );
    pages_are_pattern_s(&events);
    device.close().expect("the worker closed");

    // The scanner: cancelled after sheet 2's back was fed, and no sheet fed before the next scan.
    let record: Vec<String> =
        serde_json::from_str(&fs::read_to_string(&summary).expect("the worker's summary"))
            .expect("a summary");
    assert_eq!(
        record,
        [
            "scan",
            "fed 1 front",
            "fed 1 back",
            "fed 2 front",
            "fed 2 back",
            "cancel",
            "scan",
            "fed 3 front",
            "fed 3 back",
            "fed 4 front",
            "fed 4 back"
        ]
    );
}

#[test]
fn ica_worker_keeps_the_pages_before_an_error_and_the_rest_come_after() {
    let client = Client::new().worker(Driver::Ica, ica_fake("jam"));
    let mut device = client.open(&stored(OFFICE), AT_MOST).expect("opened");
    let (events, ended) = scan(&mut device, &feeder(true, Format::Gray8), 1);
    // ImageCaptureCore has no code for a jam: its scan fails to complete, in its own words.
    assert!(
        is_driver(&ended, "ICReturnScannerFailedToCompleteScan"),
        "{ended:?}"
    );
    assert_eq!(
        outline(&events),
        [
            "applied",
            "sheet 1",
            "page 1 front",
            "page 1 back",
            "sheet 2",
            "page 2 front",
            "page 2 back"
        ]
    );
    pages_are_pattern_s(&events);
    // The sheet that failed stays in the feeder, and the next batch scans it.
    let (events, done) = scan(&mut device, &feeder(true, Format::Gray8), 3);
    assert_eq!(done.expect("the rest"), Done::FeederEmpty);
    assert_eq!(
        outline(&events),
        [
            "applied",
            "sheet 3",
            "page 3 front",
            "page 3 back",
            "sheet 4",
            "page 4 front",
            "page 4 back"
        ]
    );
    pages_are_pattern_s(&events);
    device.close().expect("the worker closed");
}

#[test]
fn ica_worker_says_a_device_removed_mid_page_gone_and_every_call_after() {
    let client = Client::new().worker(Driver::Ica, ica_fake("removed"));
    let mut device = client.open(&stored(OFFICE), AT_MOST).expect("opened");
    let (events, ended) = scan(&mut device, &feeder(false, Format::Gray8), 1);
    assert!(is_scan(&ended, &ScanError::Gone), "{ended:?}");
    // Sheet 1's page kept; sheet 2's, unplugged mid-page, dropped.
    assert_eq!(outline(&events), ["applied", "sheet 1", "page 1 front"]);
    pages_are_pattern_s(&events);
    assert!(matches!(device.caps(), Err(Error::Scan(ScanError::Gone))));
    let (events, ended) = scan(&mut device, &feeder(false, Format::Gray8), 2);
    assert!(is_scan(&ended, &ScanError::Gone), "{ended:?}");
    assert!(events.is_empty(), "{:?}", outline(&events));
    device.close().expect("the worker closed");
}

#[test]
fn live_ica_worker_refuses_a_bad_id_then_lists_through_the_framework_or_says_it_isn_t_there() {
    let client = Client::new().worker(Driver::Ica, worker("ica"));
    // On every system, an id the backend doesn't form is refused before the framework is reached.
    assert!(matches!(
        client.open(&stored("bad\0id"), AT_MOST),
        Err(Error::Scan(ScanError::BadId))
    ));

    #[cfg(not(target_os = "macos"))]
    {
        let not_installed = |answer: Result<(), Error>| {
            assert!(
                matches!(
                    &answer,
                    Err(Error::Scan(ScanError::NotInstalled { what })) if what == "ImageCaptureCore"
                ),
                "{answer:?}"
            );
        };
        not_installed(client.list(Driver::Ica, AT_MOST).map(|_| ()));
        not_installed(client.open(&stored(OFFICE), AT_MOST).map(|_| ()));
    }

    #[cfg(target_os = "macos")]
    on_macos::list_and_open(&client);
}

/// The real framework through a worker, whose `serve` makes its calls on the process's main
/// thread, where the backend runs the main run loop while it waits for the framework's callbacks.
/// With no scanner (CI's runner), the listing answers `Ok` and empty, 2 s after the framework's
/// local enumeration ends, and a made-up id is `Gone` then. `SCAN_DEVICE_REQUIRE_ICA=1` (CI's
/// macOS check job) makes each answer required within 20 s of its 30 s deadline; without it, a
/// listing or open the framework never finishes (`TimedOut`) is said as skipped.
#[cfg(target_os = "macos")]
mod on_macos {
    use std::time::{Duration, Instant};

    use scan_worker::scan_device::{Driver, ScanError};
    use scan_worker::{Client, Error};

    use super::stored;

    const DEADLINE: Duration = Duration::from_secs(30);
    const REQUIRED_WITHIN: Duration = Duration::from_secs(20);
    const REQUIRE: &str = "SCAN_DEVICE_REQUIRE_ICA";

    fn required() -> bool {
        std::env::var_os(REQUIRE).is_some_and(|value| value == "1")
    }

    /// `took` within [`REQUIRED_WITHIN`] where [`REQUIRE`] is `1`.
    fn in_time(took: Duration, what: &str) {
        if required() {
            assert!(
                took < REQUIRED_WITHIN,
                "{what} took {took:?}, past {REQUIRED_WITHIN:?}"
            );
        }
    }

    fn skipped(took: Duration, what: &str) {
        assert!(
            !required(),
            "{what} timed out after {took:?} with {REQUIRE}=1"
        );
        eprintln!(
            "skipped: {what} timed out after {:.1} s: the framework's local enumeration never \
             ended here ({REQUIRE}=1 makes this fail)",
            took.as_secs_f32()
        );
    }

    pub fn list_and_open(client: &Client) {
        let start = Instant::now();
        let listed = client.list(Driver::Ica, DEADLINE);
        let took = start.elapsed();
        match listed {
            Ok(devices) => {
                for device in &devices {
                    assert_eq!(device.driver, Driver::Ica, "{device:?}");
                    assert!(device.id.0.starts_with("ica:"), "{device:?}");
                }
                eprintln!(
                    "a listing through the worker: {} scanners in {:.2} s",
                    devices.len(),
                    took.as_secs_f32()
                );
                in_time(took, "the listing through the worker");
            }
            Err(Error::Scan(ScanError::TimedOut)) => {
                skipped(took, "the listing through the worker");
            }
            Err(other) => panic!("the listing through the worker answered {other:?}"),
        }

        let start = Instant::now();
        let opened = client.open(&stored("no-such-scanner-0000"), DEADLINE);
        let took = start.elapsed();
        match opened {
            Err(Error::Scan(ScanError::Gone)) => {
                eprintln!(
                    "a made-up id through the worker: Gone in {:.2} s",
                    took.as_secs_f32()
                );
                in_time(took, "the open through the worker");
            }
            Err(Error::Scan(ScanError::TimedOut)) => {
                skipped(took, "the open through the worker");
            }
            Ok(device) => panic!("a device of a made-up id was opened: {:?}", device.info()),
            Err(other) => panic!("the open through the worker answered {other:?}"),
        }
    }
}
