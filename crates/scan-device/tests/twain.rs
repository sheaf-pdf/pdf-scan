//! End-to-end integration tests for [`scan_device::twain::TwainBackend`] against the TWAIN Working
//! Group's `TWAIN2 Software Scanner` sample source (`Twain_DS_sample01`) in both 64-bit
//! (`x86_64-pc-windows-msvc`) and 32-bit (`i686-pc-windows-msvc`).
//!
//! Each test runs inside its own `rusty-fork` child process so `CAP_XFERCOUNT` environment settings
//! and the process-wide lazy TWAIN `Session` start clean. When `TWAINDSM.dll` or the sample source
//! isn't installed, tests skip cleanly unless `TWAIN_DSM_REQUIRE_SAMPLE=1` is set (as in CI).

#![cfg(all(windows, feature = "twain"))]

use core::ptr::null;
use std::hash::Hash;
use std::io::Write as _;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use scan_device::in_progress::{Tracker, Unit};
use scan_device::twain::{Delivery, TwainBackend};
use scan_device::{
    Applied, Backend, Clamped, Device, DeviceId, DeviceInfo, Done, Driver, Encoding, Event, Format,
    Paper, Pixels, RawImage, ScanError, Settings, Side, SideKnown, SizeMm, Source, WindowOwner,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CW_USEDEFAULT, CreateWindowExW, DestroyWindow, IsWindow, WS_OVERLAPPEDWINDOW,
};

const SKIPPED: i32 = 77;
const DONE: i32 = 78;

const SAMPLE_NAME: &str = "TWAIN2 Software Scanner";
const SAMPLE_MAKER: &str = "TWAIN Working Group";
const SAMPLE_FAMILY: &str = "Software Scan";
const DEADLINE: Duration = Duration::from_secs(20);

#[derive(Debug)]
enum Outcome {
    Done,
    Skip(String),
}

fn host_bits() -> u8 {
    if cfg!(target_pointer_width = "32") {
        32
    } else {
        64
    }
}

fn host_prefix() -> &'static str {
    if cfg!(target_pointer_width = "32") {
        "twain32:"
    } else {
        "twain64:"
    }
}

fn other_bits() -> (u8, &'static str) {
    if cfg!(target_pointer_width = "32") {
        (64, "twain64:")
    } else {
        (32, "twain32:")
    }
}

fn require_sample() -> bool {
    std::env::var("TWAIN_DSM_REQUIRE_SAMPLE").is_ok_and(|v| v == "1")
}

fn fork_test(name: &str, id: impl Hash, env: &[(&str, &str)], body: impl FnOnce() -> Outcome) {
    rusty_fork::fork(
        name,
        id,
        |cmd| {
            for (k, v) in env {
                cmd.env(k, v);
            }
        },
        |child, _| {
            let status = child
                .wait_timeout(Duration::from_secs(120))
                .expect("wait on child")
                .unwrap_or_else(|| panic!("{name}: child timed out after 120 seconds"));
            match status.code() {
                Some(DONE) => {}
                Some(SKIPPED) => {
                    let _ = writeln!(std::io::stderr(), "{name}: skipped");
                }
                Some(0) => panic!("{name}: child exited 0 without running test"),
                _ => panic!("{name}: child failed ({status:?})"),
            }
        },
        || match body() {
            Outcome::Done => std::process::exit(DONE),
            Outcome::Skip(why) => {
                assert!(!require_sample(), "{why}, and TWAIN_DSM_REQUIRE_SAMPLE=1");
                println!("skipped: {why}");
                std::process::exit(SKIPPED);
            }
        },
    )
    .expect("spawn child process");
}

macro_rules! forked {
    ($name:ident, $env:expr, $body:expr) => {
        fork_test(
            rusty_fork::rusty_fork_test_name!($name),
            rusty_fork::rusty_fork_id!(),
            $env,
            $body,
        )
    };
}

/// Lists sources with `backend` and finds `TWAIN2 Software Scanner`, or returns `Err(Outcome::Skip)`
/// if the DSM or sample source isn't installed.
fn find_sample(backend: &mut TwainBackend) -> Result<DeviceInfo, Outcome> {
    let devices = match backend.list(DEADLINE) {
        Ok(devices) => devices,
        Err(ScanError::NotInstalled { .. }) => {
            return Err(Outcome::Skip(
                "TWAINDSM.dll isn't installed: run scripts/twain-sample.ps1 Install".to_owned(),
            ));
        }
        Err(err) => panic!("TwainBackend::list failed: {err}"),
    };
    devices
        .into_iter()
        .find(|d| d.name == SAMPLE_NAME && d.maker == SAMPLE_MAKER)
        .ok_or_else(|| {
            Outcome::Skip(
                "TWAIN2 Software Scanner isn't installed: run scripts/twain-sample.ps1 Install"
                    .to_owned(),
            )
        })
}

fn run_batch(
    device: &mut dyn Device,
    settings: &Settings,
    first_sheet: u32,
    tracker: &Tracker,
) -> (Result<Done, ScanError>, Vec<Event>) {
    let (events_tx, events_rx) = mpsc::sync_channel(4);
    thread::scope(|scope| {
        let handle = scope.spawn(move || device.scan(settings, first_sheet, &events_tx, tracker));
        let events: Vec<Event> = events_rx.iter().collect();
        (handle.join().expect("batch thread"), events)
    })
}

type PageRecord = (u32, RawImage, (f32, f32));

fn extract_applied_and_pages(events: &[Event]) -> (&Applied, Vec<PageRecord>) {
    let Some(Event::Applied(applied)) = events.first() else {
        panic!("expected Event::Applied first, got: {:?}", events.first());
    };
    let mut out = Vec::new();
    for pair in events[1..].as_chunks::<2>().0 {
        match pair {
            [Event::Sheet(sheet), Event::Page(page)] => {
                assert_eq!(page.sheet, *sheet);
                assert_eq!(page.side, Side::Front);
                assert_eq!(page.side_known, SideKnown::Inferred);
                let Pixels::Raw(ref img) = page.pixels else {
                    panic!("expected raw pixels");
                };
                out.push((*sheet, img.clone(), page.dpi));
            }
            other => panic!("unexpected event pair: {other:?}"),
        }
    }
    assert_eq!(
        1 + out.len() * 2,
        events.len(),
        "unexpected trailing event in {events:?}"
    );
    (applied, out)
}

#[test]
fn lists_opens_validates_ids_handles_busy_and_restarts_session() {
    forked!(
        lists_opens_validates_ids_handles_busy_and_restarts_session,
        &[("CAP_XFERCOUNT", "1")],
        || {
            // `BadId` is checked before loading `TWAINDSM.dll` (even when `with_dsm_path` names a
            // missing file).
            let mut missing_dsm =
                TwainBackend::new().with_dsm_path(r"C:\definitely-missing\TWAINDSM.dll");
            let (wrong_bits, wrong_prefix) = other_bits();
            for bad in [
                DeviceInfo::new(
                    DeviceId("sane:test:0".to_owned()),
                    "Scanner".to_owned(),
                    "Maker".to_owned(),
                    "Model".to_owned(),
                    Driver::Sane,
                ),
                DeviceInfo::new(
                    DeviceId(format!("{wrong_prefix}{SAMPLE_NAME}")),
                    SAMPLE_NAME.to_owned(),
                    SAMPLE_MAKER.to_owned(),
                    SAMPLE_FAMILY.to_owned(),
                    Driver::Twain { bits: wrong_bits },
                ),
                DeviceInfo::new(
                    DeviceId(host_prefix().to_owned()),
                    String::new(),
                    SAMPLE_MAKER.to_owned(),
                    SAMPLE_FAMILY.to_owned(),
                    Driver::Twain { bits: host_bits() },
                ),
                DeviceInfo::new(
                    DeviceId(format!(
                        "{}123456789012345678901234567890123",
                        host_prefix()
                    )),
                    "TooLong".to_owned(),
                    SAMPLE_MAKER.to_owned(),
                    SAMPLE_FAMILY.to_owned(),
                    Driver::Twain { bits: host_bits() },
                ),
                DeviceInfo::new(
                    DeviceId(format!("{}Scan\0ner", host_prefix())),
                    "Nul".to_owned(),
                    SAMPLE_MAKER.to_owned(),
                    SAMPLE_FAMILY.to_owned(),
                    Driver::Twain { bits: host_bits() },
                ),
            ] {
                assert_eq!(
                    missing_dsm.open(&bad, DEADLINE).unwrap_err(),
                    ScanError::BadId,
                    "expected BadId for {:?}",
                    bad.id
                );
            }

            // A missing explicit DSM path reports `NotInstalled` on `list` and valid-id `open`.
            assert_eq!(
                missing_dsm.list(DEADLINE).unwrap_err(),
                ScanError::NotInstalled {
                    what: "TWAIN".to_owned(),
                }
            );
            drop(missing_dsm);

            let mut backend = TwainBackend::new();
            let sample = match find_sample(&mut backend) {
                Ok(sample) => sample,
                Err(skip) => return skip,
            };
            assert_eq!(sample.driver, Driver::Twain { bits: host_bits() });
            assert_eq!(sample.id.0, format!("{}{SAMPLE_NAME}", host_prefix()));
            assert_eq!(sample.model, SAMPLE_FAMILY);

            // Unlisted name or mismatched maker/family returns `Gone`.
            let mut wrong_maker = sample.clone();
            wrong_maker.maker = "Someone Else".to_owned();
            assert_eq!(
                backend.open(&wrong_maker, DEADLINE).unwrap_err(),
                ScanError::Gone
            );

            let mut wrong_family = sample.clone();
            wrong_family.model = "Other Family".to_owned();
            assert_eq!(
                backend.open(&wrong_family, DEADLINE).unwrap_err(),
                ScanError::Gone
            );

            let unlisted = DeviceInfo::new(
                DeviceId(format!("{}NoSuchScanner", host_prefix())),
                "NoSuchScanner".to_owned(),
                SAMPLE_MAKER.to_owned(),
                SAMPLE_FAMILY.to_owned(),
                Driver::Twain { bits: host_bits() },
            );
            assert_eq!(
                backend.open(&unlisted, DEADLINE).unwrap_err(),
                ScanError::Gone
            );

            // Open the sample; a second `open` returns `Busy`, while `list` still works.
            let opened = backend.open(&sample, DEADLINE).expect("open sample");
            assert_eq!(opened.info, sample);
            assert_eq!(opened.renamed_from, None);

            let mut second = TwainBackend::new();
            assert_eq!(second.open(&sample, DEADLINE).unwrap_err(), ScanError::Busy);
            let listed_while_open = second.list(DEADLINE).expect("list while open");
            assert!(listed_while_open.contains(&sample));

            // Asking for a different DSM file once the system DSM is loaded fails with `Driver`
            // (`LoadError::Elsewhere`).
            let mut elsewhere = TwainBackend::new().with_dsm_path(r"C:\other-folder\TWAINDSM.dll");
            assert!(matches!(
                elsewhere.list(DEADLINE).unwrap_err(),
                ScanError::Driver { shown: false, .. }
            ));

            // Dropping all backends and the open device tears down and joins the STA session; a
            // fresh backend afterward starts a new session cleanly.
            drop(opened);
            drop(elsewhere);
            drop(second);
            drop(backend);

            let mut restarted = TwainBackend::new();
            let sample_again = find_sample(&mut restarted).expect("restarted list");
            let reopened = restarted.open(&sample_again, DEADLINE).expect("reopen");
            assert_eq!(reopened.info, sample);
            Outcome::Done
        }
    );
}

#[test]
fn reads_sample_capabilities_for_memory_and_native_transfers() {
    forked!(
        reads_sample_capabilities_for_memory_and_native_transfers,
        &[("CAP_XFERCOUNT", "1")],
        || {
            let mut backend = TwainBackend::new();
            let sample = match find_sample(&mut backend) {
                Ok(sample) => sample,
                Err(skip) => return skip,
            };

            let mut dev = backend.open(&sample, DEADLINE).expect("open").device;
            let mem_caps = dev.caps().expect("memory caps");
            assert!(mem_caps.settings_window);
            assert!(
                mem_caps
                    .sources
                    .iter()
                    .any(|s| s.source == Source::Flatbed && !s.duplex),
                "expected Flatbed in {mem_caps:?}"
            );
            let feeder = mem_caps
                .sources
                .iter()
                .find(|s| s.source == Source::Feeder)
                .expect("expected Feeder in sample caps");
            assert!(feeder.encodings.is_empty());
            for fmt in [Format::Bw1, Format::Gray8, Format::Rgb8] {
                assert!(
                    feeder.formats.contains(&fmt),
                    "missing {fmt:?} in {:?}",
                    feeder.formats
                );
            }
            let max_area = feeder.max_area.expect("sample reports physical bounds");
            assert!(
                (max_area.width - SizeMm::LEGAL.width).abs() < 0.1
                    && (max_area.height - SizeMm::LEGAL.height).abs() < 0.1,
                "expected ~Legal bounds, got {max_area:?}"
            );
            drop(dev);

            // Under `with_native_transfer`, 16-bit formats are never advertised.
            let mut native_backend = TwainBackend::new().with_native_transfer();
            let mut native_dev = native_backend
                .open(&sample, DEADLINE)
                .expect("open native")
                .device;
            let native_caps = native_dev.caps().expect("native caps");
            for sc in &native_caps.sources {
                assert!(!sc.formats.contains(&Format::Gray16));
                assert!(!sc.formats.contains(&Format::Rgb16));
            }
            Outcome::Done
        }
    );
}

#[test]
fn ten_page_feeder_batch_tracks_sheets_progress_and_reloads_cleanly() {
    forked!(
        ten_page_feeder_batch_tracks_sheets_progress_and_reloads_cleanly,
        &[("CAP_XFERCOUNT", "10")],
        || {
            let mut backend = TwainBackend::new();
            let sample = match find_sample(&mut backend) {
                Ok(sample) => sample,
                Err(skip) => return skip,
            };
            let mut dev = backend.open(&sample, DEADLINE).expect("open").device;

            let mut asked = Settings::new(Source::Feeder, 100, Format::Rgb8);
            asked.paper = Paper::Size(SizeMm::LETTER);

            let tracker = Tracker::new();
            let (done, events) = run_batch(&mut *dev, &asked, 5, &tracker);
            assert_eq!(done, Ok(Done::FeederEmpty));

            let (applied, pages) = extract_applied_and_pages(&events);
            assert_eq!(applied.settings.source, Source::Feeder);
            assert_eq!(applied.settings.dpi, 100);
            assert_eq!(applied.settings.format, Format::Rgb8);
            assert!(
                applied.clamped.is_empty(),
                "unexpected clamps: {:?}",
                applied.clamped
            );
            assert_eq!(pages.len(), 10);

            for (idx, (sheet, img, dpi)) in pages.iter().enumerate() {
                assert_eq!(*sheet, 5 + idx as u32);
                assert_eq!(img.format(), Format::Rgb8);
                assert!(img.width() > 0 && img.height() > 0);
                assert!((dpi.0 - 100.0).abs() < 0.5 && (dpi.1 - 100.0).abs() < 0.5);
            }

            let snap = tracker.snapshot();
            assert_eq!(snap.unit, Unit::Bytes);
            assert!(snap.done > 0);
            assert_eq!(snap.total, Some(snap.done));
            assert_eq!(snap.fraction, Some(1.0));
            assert!(!snap.cancelled);

            // Second consecutive batch on the same open device scans another 10 sheets.
            let tracker2 = Tracker::new();
            let (done2, events2) = run_batch(&mut *dev, &asked, 15, &tracker2);
            assert_eq!(done2, Ok(Done::FeederEmpty));
            let (_, pages2) = extract_applied_and_pages(&events2);
            assert_eq!(pages2.len(), 10);
            assert_eq!(pages2[0].0, 15);
            assert_eq!(pages2[9].0, 24);
            Outcome::Done
        }
    );
}

#[test]
fn memory_and_native_transfers_match_pixel_for_pixel_across_formats_and_deliveries() {
    forked!(
        memory_and_native_transfers_match_pixel_for_pixel_across_formats_and_deliveries,
        &[("CAP_XFERCOUNT", "2")],
        || {
            let mut probe = TwainBackend::new();
            let sample = match find_sample(&mut probe) {
                Ok(sample) => sample,
                Err(skip) => return skip,
            };
            drop(probe);

            for format in [Format::Bw1, Format::Gray8, Format::Rgb8] {
                let mut asked = Settings::new(Source::Feeder, 100, format);
                asked.paper = Paper::Size(SizeMm::LETTER);

                let mut baseline: Option<Vec<RawImage>> = None;
                for delivery in [Delivery::Callback, Delivery::Events] {
                    for native in [false, true] {
                        let mut backend = TwainBackend::new().with_delivery(delivery);
                        if native {
                            backend = backend.with_native_transfer();
                        }
                        let mut dev = backend.open(&sample, DEADLINE).expect("open").device;
                        let tracker = Tracker::new();
                        let (done, events) = run_batch(&mut *dev, &asked, 1, &tracker);
                        assert_eq!(
                            done,
                            Ok(Done::FeederEmpty),
                            "{format:?} {delivery:?} native={native}"
                        );
                        let (_, pages) = extract_applied_and_pages(&events);
                        assert_eq!(pages.len(), 2);
                        let imgs: Vec<RawImage> =
                            pages.into_iter().map(|(_, img, _)| img).collect();
                        assert_eq!(imgs[0].format(), format);

                        if let Some(ref expected) = baseline {
                            assert_eq!(
                                imgs, *expected,
                                "pixel mismatch for {format:?} {delivery:?} native={native}"
                            );
                        } else {
                            baseline = Some(imgs);
                        }
                    }
                }
            }
            Outcome::Done
        }
    );
}

#[test]
fn paper_size_via_imagelayout_crops_dimensions_and_clamping_reports_changes() {
    forked!(
        paper_size_via_imagelayout_crops_dimensions_and_clamping_reports_changes,
        &[("CAP_XFERCOUNT", "1")],
        || {
            let mut backend = TwainBackend::new().with_native_transfer();
            let sample = match find_sample(&mut backend) {
                Ok(sample) => sample,
                Err(skip) => return skip,
            };
            let mut dev = backend.open(&sample, DEADLINE).expect("open").device;

            // 5 x 7 inches = 127.0 x 177.8 mm at 100 dpi -> 500 x 700 pixels, no paper clamp.
            let five_by_seven = SizeMm {
                width: 127.0,
                height: 177.8,
            };
            let mut small = Settings::new(Source::Feeder, 100, Format::Gray8);
            small.paper = Paper::Size(five_by_seven);
            let (done_small, events_small) = run_batch(&mut *dev, &small, 1, &Tracker::new());
            assert_eq!(done_small, Ok(Done::FeederEmpty));
            let (applied_small, pages_small) = extract_applied_and_pages(&events_small);
            assert!(
                applied_small.clamped.is_empty(),
                "expected 5x7 in to apply without clamping, got {:?}",
                applied_small.clamped
            );
            let img_small = &pages_small[0].1;
            assert!(
                img_small.width().abs_diff(500) <= 2 && img_small.height().abs_diff(700) <= 2,
                "expected ~500x700, got {}x{}",
                img_small.width(),
                img_small.height()
            );

            // Clamping across duplex, out-of-range DPI, 16-bit format on native transfer,
            // unsupported Paper::Auto -> Legal, and encodings.
            let mut clamped_ask = Settings::new(Source::Feeder, 12_000, Format::Rgb16);
            clamped_ask.duplex = true;
            clamped_ask.paper = Paper::Auto;
            clamped_ask.encodings = vec![Encoding::Jpeg];

            let (done_full, events_full) = run_batch(&mut *dev, &clamped_ask, 1, &Tracker::new());
            assert_eq!(done_full, Ok(Done::FeederEmpty));
            let (applied_full, pages_full) = extract_applied_and_pages(&events_full);
            assert!(!applied_full.settings.duplex);
            assert_eq!(applied_full.settings.format, Format::Rgb8);
            assert!(applied_full.settings.encodings.is_empty());
            assert!(applied_full.clamped.contains(&Clamped::Duplex(true)));
            assert!(applied_full.clamped.contains(&Clamped::Dpi(12_000)));
            assert!(
                applied_full
                    .clamped
                    .contains(&Clamped::Format(Format::Rgb16))
            );
            assert!(applied_full.clamped.contains(&Clamped::Paper(Paper::Auto)));
            assert!(
                applied_full
                    .clamped
                    .contains(&Clamped::Encodings(vec![Encoding::Jpeg]))
            );

            let img_full = &pages_full[0].1;
            assert!(
                img_full.width() > img_small.width() && img_full.height() > img_small.height(),
                "Legal image ({}x{}) must exceed 5x7 crop ({}x{})",
                img_full.width(),
                img_full.height(),
                img_small.width(),
                img_small.height()
            );
            Outcome::Done
        }
    );
}

#[test]
fn flatbed_stops_at_one_page_and_cancel_finishes_sheet_without_poisoning_list() {
    forked!(
        flatbed_stops_at_one_page_and_cancel_finishes_sheet_without_poisoning_list,
        &[("CAP_XFERCOUNT", "10")],
        || {
            let mut backend = TwainBackend::new();
            let sample = match find_sample(&mut backend) {
                Ok(sample) => sample,
                Err(skip) => return skip,
            };
            let mut dev = backend.open(&sample, DEADLINE).expect("open").device;

            // 1. Flatbed returns `Done::OnePage` after 1 page even when `CAP_XFERCOUNT=10`.
            let mut flatbed = Settings::new(Source::Flatbed, 100, Format::Gray8);
            flatbed.paper = Paper::Size(SizeMm::LETTER);
            let (done_fb, events_fb) = run_batch(&mut *dev, &flatbed, 1, &Tracker::new());
            assert_eq!(done_fb, Ok(Done::OnePage));
            let (_, pages_fb) = extract_applied_and_pages(&events_fb);
            assert_eq!(pages_fb.len(), 1);

            // 2. Sheet-granular cancel during a 10-page feeder batch on a rendezvous channel,
            //    plus concurrent `list()` timeout while the STA thread is held mid-send (proving
            //    `ABANDONED` requests don't poison the active session).
            let mut feeder = Settings::new(Source::Feeder, 100, Format::Gray8);
            feeder.paper = Paper::Size(SizeMm::LETTER);
            let tracker = Tracker::new();
            let (events_tx, events_rx) = mpsc::sync_channel(0);

            thread::scope(|scope| {
                let batch_tracker = tracker.clone();
                let dev_ref = &mut *dev;
                let feeder_ref = &feeder;
                let batch =
                    scope.spawn(move || dev_ref.scan(feeder_ref, 1, &events_tx, &batch_tracker));

                assert!(matches!(events_rx.recv().unwrap(), Event::Applied(_)));
                assert_eq!(events_rx.recv().unwrap(), Event::Sheet(1));

                // While sheet 1's `Event::Page` is waiting on the rendezvous channel, a short
                // `list` from another backend times out in the queue without poisoning `Session`.
                let mut lister = TwainBackend::new();
                assert_eq!(
                    lister.list(Duration::from_millis(20)).unwrap_err(),
                    ScanError::TimedOut
                );

                assert!(matches!(events_rx.recv().unwrap(), Event::Page(p) if p.sheet == 1));
                assert_eq!(events_rx.recv().unwrap(), Event::Sheet(2));

                // Cancel while sheet 2's page is waiting to be taken: sheet 2 finishes, sheet 3
                // never starts.
                tracker.cancel();
                assert!(matches!(events_rx.recv().unwrap(), Event::Page(p) if p.sheet == 2));
                assert_eq!(batch.join().expect("batch thread"), Ok(Done::Cancelled));
            });

            // Session and open device remain completely healthy after the queued `list` timeout
            // and sheet cancellation.
            let mut lister = TwainBackend::new();
            assert!(
                lister
                    .list(DEADLINE)
                    .expect("list after cancel")
                    .contains(&sample)
            );
            assert!(dev.caps().is_ok());

            // 3. Pre-cancelled tracker ends `Done::Cancelled` after `Event::Applied` with 0 sheets.
            let pre_cancelled = Tracker::new();
            pre_cancelled.cancel();
            let (done_pre, events_pre) = run_batch(&mut *dev, &feeder, 1, &pre_cancelled);
            assert_eq!(done_pre, Ok(Done::Cancelled));
            assert_eq!(events_pre.len(), 1);
            assert!(matches!(events_pre[0], Event::Applied(_)));

            // 4. Dropped event receiver ends `Done::Cancelled` immediately.
            let (tx, rx) = mpsc::sync_channel(1);
            drop(rx);
            assert_eq!(
                dev.scan(&feeder, 1, &tx, &Tracker::new()),
                Ok(Done::Cancelled)
            );

            Outcome::Done
        }
    );
}

#[test]
fn empty_feeder_returns_feeder_empty_after_applied() {
    // In `Twain_DS_sample01` (`CScanner_FreeImage::getDocumentCount`), `atoi("65536")` passes
    // `nCount > 0` as `int` and truncates to `0` on `short` return, deterministically leaving the
    // simulated ADF empty (`CAP_FEEDERLOADED == FALSE`).
    forked!(
        empty_feeder_returns_feeder_empty_after_applied,
        &[("CAP_XFERCOUNT", "65536")],
        || {
            let mut backend = TwainBackend::new();
            let sample = match find_sample(&mut backend) {
                Ok(sample) => sample,
                Err(skip) => return skip,
            };
            let mut dev = backend.open(&sample, DEADLINE).expect("open").device;

            let mut feeder = Settings::new(Source::Feeder, 100, Format::Gray8);
            feeder.paper = Paper::Size(SizeMm::LETTER);
            let (outcome, events) = run_batch(&mut *dev, &feeder, 1, &Tracker::new());
            assert_eq!(outcome, Err(ScanError::FeederEmpty));
            assert_eq!(events.len(), 1);
            assert!(matches!(events[0], Event::Applied(_)));

            // Pre-cancelled tracker still takes precedence over an empty feeder (`Done::Cancelled`).
            let cancelled = Tracker::new();
            cancelled.cancel();
            let (outcome_cancel, events_cancel) = run_batch(&mut *dev, &feeder, 1, &cancelled);
            assert_eq!(outcome_cancel, Ok(Done::Cancelled));
            assert_eq!(events_cancel.len(), 1);

            Outcome::Done
        }
    );
}

#[test]
fn settings_window_rejects_bogus_destroyed_and_same_thread_owner_handles() {
    forked!(
        settings_window_rejects_bogus_destroyed_and_same_thread_owner_handles,
        &[("CAP_XFERCOUNT", "1")],
        || {
            let mut backend = TwainBackend::new();
            let sample = match find_sample(&mut backend) {
                Ok(sample) => sample,
                Err(skip) => return skip,
            };
            let mut dev = backend.open(&sample, DEADLINE).expect("open").device;

            for bogus in [0, 0xFFFF, u64::from(u32::MAX), u64::MAX] {
                assert!(
                    matches!(
                        dev.settings_window(Some(WindowOwner(bogus))),
                        Err(ScanError::Driver { shown: false, .. })
                    ),
                    "expected Driver error for bogus HWND {bogus:#x}"
                );
            }

            // Create a live Win32 window on this calling thread and verify `settings_window`
            // refuses it immediately rather than deadlocking on synchronous cross-thread messages.
            let class_static: Vec<u16> = "STATIC\0".encode_utf16().collect();
            // SAFETY: `"STATIC\0"` is a built-in system window class and NUL-terminated UTF-16; all
            // pointer arguments not used are null.
            let hwnd = unsafe {
                CreateWindowExW(
                    0,
                    class_static.as_ptr(),
                    null(),
                    WS_OVERLAPPEDWINDOW,
                    CW_USEDEFAULT,
                    CW_USEDEFAULT,
                    100,
                    100,
                    core::ptr::null_mut(),
                    core::ptr::null_mut(),
                    core::ptr::null_mut(),
                    null(),
                )
            };
            assert!(!hwnd.is_null(), "CreateWindowExW failed");
            // SAFETY: `hwnd` was just created above.
            assert_ne!(unsafe { IsWindow(hwnd) }, 0);

            let raw_hwnd = hwnd as usize as u64;
            assert!(matches!(
                dev.settings_window(Some(WindowOwner(raw_hwnd))),
                Err(ScanError::Driver { shown: false, .. })
            ));

            // Destroy the window and verify the now-stale handle is also refused cleanly.
            // SAFETY: `hwnd` belongs to this thread and hasn't been destroyed yet.
            let destroyed = unsafe { DestroyWindow(hwnd) };
            assert_ne!(destroyed, 0);

            assert!(matches!(
                dev.settings_window(Some(WindowOwner(raw_hwnd))),
                Err(ScanError::Driver { shown: false, .. })
            ));

            // Open device remains healthy in state 4 afterward.
            assert!(dev.caps().is_ok());
            Outcome::Done
        }
    );
}
