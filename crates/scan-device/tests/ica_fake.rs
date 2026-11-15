//! The ImageCaptureCore backend end to end over the scripted framework, on every system: listings
//! (scanners only, the snapshot at the settle after the local enumeration or at the deadline,
//! `TimedOut` where the local enumeration never ends, duplicates, a late event of an earlier
//! listing), opens (found, `Gone` at the settle, `TimedOut`, a busy or refused session, a session
//! never ready), caps (a flatbed and a duplex feeder, the unit selected back, the unasked select
//! and an answer of another type never taken, a nil answer, a nil unasked answer of another
//! unit never taken, a select never answered), and
//! batches (a flatbed's page, a feeder's simplex and duplex sheets, each format with padded rows
//! and either byte order, every page `pattern`'s byte for byte, sheets numbered from
//! `first_sheet`, a cancel on a rendezvous channel, a cancel finding the next sheet feeding, the
//! receiver dropped, errors keeping the pages before them, the scanner's own cancel, the device
//! removed, an empty feeder, a lying band, a callback's panic, warming up, what the module took).
//!
//! The scripted framework is our model of ImageCaptureCore as its documentation reads, not
//! evidence of it: these prove the backend's logic over that reading.

#![cfg(all(feature = "ica", feature = "fake"))]

use std::sync::mpsc::sync_channel;
use std::thread;
use std::time::{Duration, Instant};

use scan_device::fake::pattern;
use scan_device::ica::IcaBackend;
use scan_device::ica::fake_ica::*;
use scan_device::in_progress::Tracker;
use scan_device::{
    Backend, Clamped, Device, DeviceId, DeviceInfo, Done, Driver, Event, Format, Paper, Pixels,
    ScanError, Settings, Side, SideKnown, SizeMm, Source, Status,
};

const OFFICE: &str = "0x14100000046d0001";
const RESOLUTIONS: &[u64] = &[50, 75, 100, 150, 300];
const DEPTHS: &[u64] = &[1, 8, 16];
const LONG: Duration = Duration::from_secs(10);
const A6: SizeMm = SizeMm {
    width: 105.0,
    height: 148.0,
};

/// A flatbed of 1.1 by 0.9 inches (55 by 45 pixels at 50 dpi), and a duplex feeder of A6, A5 and
/// A4, the flatbed selected as it's instantiated.
fn office() -> FakeIcaDevice {
    FakeIcaDevice::new(scanner_props(OFFICE, "Office scanner"))
        .with_unit(flatbed(RESOLUTIONS, DEPTHS, 1.1, 0.9))
        .with_unit(feeder(
            RESOLUTIONS,
            DEPTHS,
            &[DOCUMENT_A6, DOCUMENT_A5, DOCUMENT_A4],
            true,
            8.5,
            14.0,
        ))
}

fn ica_id(native: &str) -> DeviceId {
    DeviceId(format!("ica:{native}"))
}

fn stored(native: &str) -> DeviceInfo {
    DeviceInfo::new(
        ica_id(native),
        "Stored".to_owned(),
        String::new(),
        String::new(),
        Driver::Ica,
    )
}

/// The office scanner, listed and opened.
fn opened(fake: &FakeIcaHandle) -> Box<dyn Device> {
    let mut backend = IcaBackend::from_fake(fake);
    let listed = backend.list(LONG).expect("a listing");
    let office = listed
        .into_iter()
        .find(|info| info.id == ica_id(OFFICE))
        .expect("the office scanner listed");
    backend.open(&office, LONG).expect("opened").device
}

fn feeder_settings(duplex: bool) -> Settings {
    let mut settings = Settings::new(Source::Feeder, 50, Format::Gray8);
    settings.duplex = duplex;
    settings.paper = Paper::Size(A6);
    settings
}

fn flatbed_settings(format: Format) -> Settings {
    let mut settings = Settings::new(Source::Flatbed, 50, format);
    settings.paper = Paper::Auto;
    settings
}

struct Batch {
    events: Vec<Event>,
    done: Result<Done, ScanError>,
}

/// A batch on a channel of 4, every event taken.
fn scan(device: &mut dyn Device, settings: &Settings, first_sheet: u32) -> Batch {
    scan_with(device, settings, first_sheet, 4, |_, _| true)
}

/// A batch on a channel of `bound`, each event handed to `on` with the batch's tracker as it's
/// taken; `on` answering false drops the receiver.
fn scan_with(
    device: &mut dyn Device,
    settings: &Settings,
    first_sheet: u32,
    bound: usize,
    mut on: impl FnMut(&Event, &Tracker) -> bool,
) -> Batch {
    let tracker = Tracker::new();
    let (events, received) = sync_channel(bound);
    thread::scope(|scope| {
        let batch = {
            let tracker = tracker.clone();
            scope.spawn(move || device.scan(settings, first_sheet, &events, &tracker))
        };
        let mut taken = Vec::new();
        for event in &received {
            let go_on = on(&event, &tracker);
            taken.push(event);
            if !go_on {
                break;
            }
        }
        drop(received);
        Batch {
            events: taken,
            done: batch.join().expect("the batch's thread"),
        }
    })
}

fn pages(events: &[Event]) -> Vec<(u32, Side)> {
    events
        .iter()
        .filter_map(|event| match event {
            Event::Page(page) => Some((page.sheet, page.side)),
            _ => None,
        })
        .collect()
}

fn sheets(events: &[Event]) -> Vec<u32> {
    events
        .iter()
        .filter_map(|event| match event {
            Event::Sheet(sheet) => Some(*sheet),
            _ => None,
        })
        .collect()
}

/// The events in order, each by its kind.
fn kinds(events: &[Event]) -> Vec<String> {
    events
        .iter()
        .map(|event| match event {
            Event::Applied(_) => "Applied".to_owned(),
            Event::Sheet(sheet) => format!("Sheet({sheet})"),
            Event::Page(page) => format!("Page({}, {:?})", page.sheet, page.side),
            Event::Status(status) => format!("Status({status:?})"),
            other => format!("{other:?}"),
        })
        .collect()
}

/// Every page is `pattern`'s, byte for byte, for its sheet and side, its side inferred; the
/// pages' sizes.
fn assert_patterns(events: &[Event]) -> Vec<(u32, u32)> {
    let mut sizes = Vec::new();
    for event in events {
        let Event::Page(page) = event else { continue };
        let Pixels::Raw(image) = &page.pixels else {
            panic!("a raw page");
        };
        let drawn = pattern(
            image.format(),
            image.width(),
            image.height(),
            page.sheet,
            page.side,
        )
        .expect("drawn");
        assert!(
            *image == drawn,
            "sheet {} {:?} isn't the pattern",
            page.sheet,
            page.side
        );
        assert_eq!(page.side_known, SideKnown::Inferred);
        sizes.push((image.width(), image.height()));
    }
    sizes
}

fn position(calls: &[FakeCall], call: &FakeCall) -> Option<usize> {
    calls.iter().position(|made| made == call)
}

fn is_driver(result: &Result<impl std::fmt::Debug, ScanError>, has: &str) -> bool {
    matches!(result, Err(ScanError::Driver { message, shown: false }) if message.contains(has))
}

// Listings.

#[test]
fn a_listing_gives_the_scanners_by_their_ids() {
    let fake = FakeIcaHandle::new();
    fake.add_device(office());
    let mut network = FakeIcaDevice::new(network_props("net-1", "Network scanner"));
    network.found_after = Duration::from_millis(1);
    fake.add_device(network);
    // A camera, an object that isn't a scanner device, and a scanner with no id aren't listed.
    let mut camera = scanner_props("camera-1", "Camera");
    camera.type_bits = DEVICE_CAMERA | LOCATION_LOCAL;
    fake.add_device(FakeIcaDevice::new(camera));
    let mut not_a_scanner = scanner_props("odd-1", "Odd");
    not_a_scanner.scanner_class = false;
    fake.add_device(FakeIcaDevice::new(not_a_scanner));
    fake.add_device(FakeIcaDevice::new(DeviceProps {
        persistent_id: None,
        uuid: None,
        ..scanner_props("", "No id")
    }));
    // One with no persistent id is listed by its UUID.
    fake.add_device(FakeIcaDevice::new(DeviceProps {
        persistent_id: None,
        uuid: Some("UUID-2".to_owned()),
        name: None,
        ..scanner_props("", "")
    }));

    let mut backend = IcaBackend::from_fake(&fake);
    let listed = backend.list(LONG).expect("a listing");
    // In the order found: those attached at once, then the network's.
    let ids: Vec<_> = listed.iter().map(|info| info.id.0.as_str()).collect();
    assert_eq!(ids, [&*format!("ica:{OFFICE}"), "ica:UUID-2", "ica:net-1"]);
    assert_eq!(listed[0].name, "Office scanner");
    assert_eq!(listed[1].name, "UUID-2");
    for info in &listed {
        assert_eq!((info.maker.as_str(), info.model.as_str()), ("", ""));
        assert_eq!(info.driver, Driver::Ica);
    }
    let calls = fake.calls();
    assert!(matches!(
        calls[..],
        [FakeCall::Browse(started), FakeCall::StopBrowse(stopped)] if started == stopped
    ));
}

#[test]
fn a_listing_is_the_snapshot_at_the_settle_after_the_local_enumeration() {
    let fake = FakeIcaHandle::new();
    fake.set_bounds(Bounds {
        settle: Duration::from_millis(300),
        ..Bounds::FAKE
    });
    fake.add_device(office());
    // Found on the network after the local enumeration, within the settle.
    let mut network = FakeIcaDevice::new(network_props("net-1", "Network scanner"));
    network.found_after = Duration::from_millis(50);
    fake.add_device(network);
    let start = Instant::now();
    let listed = IcaBackend::from_fake(&fake).list(LONG).expect("a listing");
    let took = start.elapsed();
    assert_eq!(listed.len(), 2);
    assert!(took >= Duration::from_millis(300), "{took:?}");
    assert!(took < Duration::from_secs(5), "{took:?}");
}

#[test]
fn a_deadline_before_the_settle_gives_what_has_been_found() {
    let fake = FakeIcaHandle::new();
    fake.set_bounds(Bounds {
        settle: Duration::from_secs(60),
        ..Bounds::FAKE
    });
    fake.add_device(office());
    let start = Instant::now();
    let listed = IcaBackend::from_fake(&fake)
        .list(Duration::from_millis(200))
        .expect("a listing");
    let took = start.elapsed();
    assert_eq!(listed.len(), 1);
    assert!(took >= Duration::from_millis(200), "{took:?}");
    assert!(took < Duration::from_secs(5), "{took:?}");
}

#[test]
fn a_listing_whose_local_enumeration_never_ends_times_out() {
    let fake = FakeIcaHandle::new();
    fake.set_enumeration(None);
    fake.add_device(office());
    let start = Instant::now();
    assert_eq!(
        IcaBackend::from_fake(&fake).list(Duration::from_millis(200)),
        Err(ScanError::TimedOut)
    );
    assert!(start.elapsed() < Duration::from_secs(5));
    // The browse stopped all the same.
    assert!(matches!(fake.calls().last(), Some(FakeCall::StopBrowse(_))));
}

#[test]
fn two_devices_of_one_id_are_listed_once_one_attached_first() {
    let fake = FakeIcaHandle::new();
    fake.add_device(FakeIcaDevice::new(network_props("same", "On the network")));
    fake.add_device(FakeIcaDevice::new(scanner_props("same", "Attached")));
    fake.add_device(FakeIcaDevice::new(scanner_props("same", "Attached again")));
    let listed = IcaBackend::from_fake(&fake).list(LONG).expect("a listing");
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].name, "Attached");
}

#[test]
fn a_late_event_of_an_earlier_listing_is_dropped() {
    let fake = FakeIcaHandle::new();
    fake.add_device(office());
    // Its callback comes only after each browse is stopped.
    let mut late = FakeIcaDevice::new(scanner_props("late-1", "Late"));
    late.late = true;
    fake.add_device(late);
    let mut backend = IcaBackend::from_fake(&fake);
    for _ in 0..2 {
        let listed = backend.list(LONG).expect("a listing");
        let ids: Vec<_> = listed.iter().map(|info| info.id.clone()).collect();
        assert_eq!(ids, [ica_id(OFFICE)]);
    }
}

#[test]
fn a_listing_at_the_real_settle_takes_its_two_seconds() {
    let fake = FakeIcaHandle::new();
    fake.set_bounds(Bounds::REAL);
    fake.add_device(office());
    let start = Instant::now();
    let listed = IcaBackend::from_fake(&fake).list(LONG).expect("a listing");
    let took = start.elapsed();
    assert_eq!(listed.len(), 1);
    assert!(took >= Duration::from_secs(2), "{took:?}");
    assert!(took < Duration::from_secs(8), "{took:?}");
}

// Opens.

#[test]
fn a_stored_device_is_opened_by_its_id_and_closed_as_it_is_dropped() {
    let fake = FakeIcaHandle::new();
    fake.add_device(office());
    let mut backend = IcaBackend::from_fake(&fake);
    let opened = backend.open(&stored(OFFICE), LONG).expect("opened");
    assert_eq!(opened.info.name, "Office scanner");
    assert_eq!(opened.info.id, ica_id(OFFICE));
    assert_eq!(opened.renamed_from, None);
    let calls = fake.calls();
    let Some(FakeCall::Browse(browse)) = calls.first().cloned() else {
        panic!("a browse first: {calls:?}");
    };
    let Some(FakeCall::OpenSession(device)) = calls.last().cloned() else {
        panic!("the session last: {calls:?}");
    };
    drop(opened);
    let calls = fake.calls();
    assert_eq!(
        calls[calls.len() - 2..],
        [FakeCall::CloseSession(device), FakeCall::StopBrowse(browse)]
    );
}

#[test]
fn a_device_not_found_is_gone_at_the_settle_well_before_the_deadline() {
    let fake = FakeIcaHandle::new();
    fake.add_device(office());
    let start = Instant::now();
    let opened = IcaBackend::from_fake(&fake).open(&stored("missing"), Duration::from_secs(30));
    assert_eq!(opened.err(), Some(ScanError::Gone));
    assert!(start.elapsed() < Duration::from_secs(5));
}

#[test]
fn an_open_whose_local_enumeration_never_ends_times_out() {
    let fake = FakeIcaHandle::new();
    fake.set_enumeration(None);
    fake.add_device(office());
    let opened = IcaBackend::from_fake(&fake).open(&stored("missing"), Duration::from_millis(200));
    assert_eq!(opened.err(), Some(ScanError::TimedOut));
    assert!(matches!(fake.calls().last(), Some(FakeCall::StopBrowse(_))));
}

#[test]
fn an_open_whose_deadline_comes_before_the_settle_times_out_and_one_after_finds_the_device() {
    // The local enumeration ends at once; the network scanner is found 300 ms into the settle.
    let fake = FakeIcaHandle::new();
    fake.set_bounds(Bounds {
        settle: Duration::from_secs(2),
        ..Bounds::FAKE
    });
    let mut network = FakeIcaDevice::new(network_props("net-1", "Network scanner"));
    network.found_after = Duration::from_millis(300);
    fake.add_device(network);
    // By the listing's rule the open isn't done before the settle, whether or not the device
    // comes: past its deadline then, `TimedOut`, never `Gone`.
    let start = Instant::now();
    let early = IcaBackend::from_fake(&fake).open(&stored("net-1"), Duration::from_millis(100));
    assert_eq!(early.err(), Some(ScanError::TimedOut));
    assert!(start.elapsed() < Duration::from_secs(1));
    let opened = IcaBackend::from_fake(&fake)
        .open(&stored("net-1"), LONG)
        .expect("found within the settle");
    assert_eq!(opened.info.id.0, "ica:net-1");
}

#[test]
fn a_busy_or_refused_session_is_said() {
    for (code, expected) in [
        (RETURN_SCANNER_IN_USE_BY_LOCAL_USER, ScanError::Busy),
        (
            RETURN_CONNECTION_NOT_AUTHORIZED_TO_OPEN_DEVICE,
            ScanError::AccessDenied,
        ),
    ] {
        let fake = FakeIcaHandle::new();
        let mut device = office();
        device.session = FakeSession::Refused(ica_error(code));
        fake.add_device(device);
        let opened = IcaBackend::from_fake(&fake).open(&stored(OFFICE), LONG);
        assert_eq!(opened.err(), Some(expected));
        // Its browse stopped; a session that didn't open isn't closed.
        let calls = fake.calls();
        assert!(matches!(calls.last(), Some(FakeCall::StopBrowse(_))));
        assert!(
            !calls
                .iter()
                .any(|call| matches!(call, FakeCall::CloseSession(_)))
        );
    }
}

#[test]
fn a_session_never_ready_times_out_and_is_closed() {
    for session in [FakeSession::NeverReady, FakeSession::NeverAnswered] {
        let fake = FakeIcaHandle::new();
        let mut device = office();
        device.session = session.clone();
        fake.add_device(device);
        let start = Instant::now();
        let opened = IcaBackend::from_fake(&fake).open(&stored(OFFICE), Duration::from_millis(300));
        assert_eq!(opened.err(), Some(ScanError::TimedOut), "{session:?}");
        assert!(start.elapsed() < Duration::from_secs(5));
        let calls = fake.calls();
        assert!(
            matches!(
                calls[calls.len() - 2..],
                [FakeCall::CloseSession(_), FakeCall::StopBrowse(_)]
            ),
            "{calls:?}"
        );
    }
}

// Caps.

#[test]
fn a_flatbed_and_a_duplex_feeder_are_read_and_the_first_unit_selected_again() {
    let fake = FakeIcaHandle::new();
    fake.add_device(office());
    let mut device = opened(&fake);
    let before = fake.calls().len();
    let caps = device.caps().expect("caps");
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
    let feeder = &caps.sources[1];
    assert_eq!(feeder.source, Source::Feeder);
    assert!(feeder.duplex);
    assert_eq!(feeder.max_area, Some(SizeMm::A4));
    // The flatbed, selected already, read as it was; the feeder selected, then the flatbed again.
    assert_eq!(
        fake.calls()[before..],
        [
            FakeCall::SelectUnit(UNIT_DOCUMENT_FEEDER),
            FakeCall::SelectUnit(UNIT_FLATBED)
        ]
    );
    assert_eq!(device.settings_window(None), Err(ScanError::Unsupported));
}

#[test]
fn the_unasked_select_and_an_answer_of_another_type_are_never_taken_as_the_answer() {
    for unasked in [Unasked::BeforeReady, Unasked::AfterReady] {
        let fake = FakeIcaHandle::new();
        // A transparency unit first, selected as the device is instantiated, its answer after
        // the device is ready, while the flatbed's select waits.
        let mut transparency = flatbed(&[1200], DEPTHS, 1.0, 1.0);
        transparency.unit_type = UNIT_POSITIVE_TRANSPARENCY;
        let mut device = FakeIcaDevice::new(scanner_props(OFFICE, "Office scanner"))
            .with_unit(transparency)
            .with_unit(flatbed(RESOLUTIONS, DEPTHS, 1.1, 0.9))
            .with_unit(feeder(&[300], &[8], &[DOCUMENT_A4], true, 8.5, 14.0));
        device.unasked = unasked;
        fake.add_device(device);
        let mut device = opened(&fake);
        let before = fake.calls().len();
        let caps = device.caps().expect("caps");
        let sources: Vec<_> = caps.sources.iter().map(|source| source.source).collect();
        assert_eq!(sources, [Source::Flatbed, Source::Feeder], "{unasked:?}");
        assert!(!caps.sources[0].duplex);
        assert_eq!(
            caps.sources[0].resolutions,
            scan_device::Resolutions::List(vec![50, 75, 100, 150, 300])
        );
        assert!(caps.sources[1].duplex);
        assert_eq!(
            caps.sources[1].resolutions,
            scan_device::Resolutions::List(vec![300])
        );
        assert_eq!(
            fake.calls()[before..],
            [
                FakeCall::SelectUnit(UNIT_FLATBED),
                FakeCall::SelectUnit(UNIT_DOCUMENT_FEEDER),
                FakeCall::SelectUnit(UNIT_POSITIVE_TRANSPARENCY)
            ]
        );
    }
}

#[test]
fn a_nil_unasked_answer_of_another_unit_is_never_taken_for_the_unit_asked() {
    for unasked in [Unasked::BeforeReady, Unasked::AfterReady] {
        let fake = FakeIcaHandle::new();
        // A transparency unit selected as the device is instantiated, its answer nil (no unit,
        // no error), after the device is ready coming while the flatbed's select waits: the
        // selected unit read then is the transparency unit's, not the flatbed asked for.
        let mut transparency = flatbed(&[1200], DEPTHS, 1.0, 1.0);
        transparency.unit_type = UNIT_POSITIVE_TRANSPARENCY;
        let mut device = FakeIcaDevice::new(scanner_props(OFFICE, "Office scanner"))
            .with_unit(transparency)
            .with_unit(flatbed(RESOLUTIONS, DEPTHS, 1.1, 0.9))
            .with_unit(feeder(&[300], &[8], &[DOCUMENT_A4], true, 8.5, 14.0));
        device.unasked = unasked;
        device.unasked_nil = true;
        fake.add_device(device);
        let mut device = opened(&fake);
        let caps = device.caps().expect("caps");
        let sources: Vec<_> = caps.sources.iter().map(|source| source.source).collect();
        assert_eq!(sources, [Source::Flatbed, Source::Feeder], "{unasked:?}");
        assert_eq!(
            caps.sources[0].resolutions,
            scan_device::Resolutions::List(vec![50, 75, 100, 150, 300]),
            "{unasked:?}"
        );
    }
}

#[test]
fn a_nil_answer_is_resolved_by_the_selected_unit_s_type() {
    let fake = FakeIcaHandle::new();
    fake.add_device(office().with_select(UNIT_DOCUMENT_FEEDER, FakeSelect::Nil));
    let mut device = opened(&fake);
    let caps = device.caps().expect("caps");
    assert_eq!(caps.sources.len(), 2);
    assert!(caps.sources[1].duplex);
    // And a batch from the feeder, its select answered nil again.
    let batch = scan(device.as_mut(), &feeder_settings(false), 1);
    assert_eq!(batch.done, Err(ScanError::FeederEmpty));
}

#[test]
fn a_select_refused_or_never_answered_is_said() {
    let fake = FakeIcaHandle::new();
    fake.add_device(office().with_select(UNIT_DOCUMENT_FEEDER, FakeSelect::NeverAnswered));
    let mut device = opened(&fake);
    let start = Instant::now();
    let caps = device.caps();
    assert!(
        is_driver(&caps, "didn't answer the select of its document feeder"),
        "{caps:?}"
    );
    // Within the scripted bound, a few hundred milliseconds.
    assert!(start.elapsed() < Duration::from_secs(5));

    let fake = FakeIcaHandle::new();
    fake.add_device(office().with_select(
        UNIT_DOCUMENT_FEEDER,
        FakeSelect::Refused(ica_error(RETURN_SCANNER_IN_USE_BY_LOCAL_USER)),
    ));
    let mut device = opened(&fake);
    assert_eq!(device.caps(), Err(ScanError::Busy));
}

#[test]
fn a_device_with_no_unit_the_model_scans_from_is_unsupported() {
    let fake = FakeIcaHandle::new();
    let mut transparency = flatbed(&[1200], DEPTHS, 1.0, 1.0);
    transparency.unit_type = UNIT_POSITIVE_TRANSPARENCY;
    fake.add_device(
        FakeIcaDevice::new(scanner_props(OFFICE, "Film scanner")).with_unit(transparency),
    );
    let mut device = opened(&fake);
    assert_eq!(device.caps(), Err(ScanError::Unsupported));
}

// Batches.

#[test]
fn a_flatbed_s_page_comes_whole() {
    let fake = FakeIcaHandle::new();
    fake.add_device(office());
    let mut device = opened(&fake);
    let batch = scan(device.as_mut(), &flatbed_settings(Format::Gray8), 1);
    assert_eq!(batch.done, Ok(Done::OnePage));
    assert_eq!(
        kinds(&batch.events),
        ["Applied", "Sheet(1)", "Page(1, Front)"]
    );
    let Event::Applied(applied) = &batch.events[0] else {
        panic!("Applied first");
    };
    // Auto, the whole bed, since the flatbed doesn't find the paper itself.
    assert_eq!(applied.clamped, [Clamped::Paper(Paper::Auto)]);
    assert_eq!(assert_patterns(&batch.events), [(55, 45)]);
    let Event::Page(page) = &batch.events[2] else {
        panic!("a page");
    };
    assert_eq!(page.dpi, (50.0, 50.0));
    // The memory-based scan asked for, in bands of at most 1 MiB.
    assert!(fake.calls().contains(&FakeCall::RequestScan {
        band_bytes: BAND_BYTES
    }));
    assert_eq!(BAND_BYTES, 1 << 20);
}

#[test]
fn a_feeder_scans_its_sheets_simplex_and_duplex() {
    let fake = FakeIcaHandle::new();
    fake.add_device(office().load(FakeSheet::stack(1..=5)));
    let mut device = opened(&fake);

    let batch = scan(device.as_mut(), &feeder_settings(false), 1);
    // A simplex batch takes the whole stack.
    assert_eq!(batch.done, Ok(Done::FeederEmpty));
    assert_eq!(sheets(&batch.events), [1, 2, 3, 4, 5]);
    assert_eq!(assert_patterns(&batch.events), [(207, 291); 5]);
    let Event::Applied(applied) = &batch.events[0] else {
        panic!("Applied first");
    };
    assert!(applied.clamped.is_empty(), "{:?}", applied.clamped);
    let written = fake.calls().into_iter().find_map(|call| match call {
        FakeCall::WriteUnit(write) => Some(write),
        _ => None,
    });
    assert_eq!(
        written,
        Some(UnitWrite {
            duplex: Some(false),
            pixel_type: PIXEL_GRAY,
            bit_depth: DEPTH_8,
            resolution: 50,
            scan_area: None,
            document_type: Some(DOCUMENT_A6),
            orientation: Some(ORIENTATION_AS_READ),
        })
    );

    let fake = FakeIcaHandle::new();
    fake.add_device(office().load(FakeSheet::stack(1..=2)));
    let mut device = opened(&fake);
    let batch = scan(device.as_mut(), &feeder_settings(true), 1);
    assert_eq!(batch.done, Ok(Done::FeederEmpty));
    assert_eq!(
        kinds(&batch.events),
        [
            "Applied",
            "Sheet(1)",
            "Page(1, Front)",
            "Page(1, Back)",
            "Sheet(2)",
            "Page(2, Front)",
            "Page(2, Back)"
        ]
    );
    assert_patterns(&batch.events);
}

#[test]
fn each_format_comes_as_its_pattern_whatever_its_padding_and_byte_order() {
    for format in [
        Format::Bw1,
        Format::Gray8,
        Format::Gray16,
        Format::Rgb8,
        Format::Rgb16,
    ] {
        for (padding, big_endian) in [(0, false), (3, true), (5, false), (1, true)] {
            let fake = FakeIcaHandle::new();
            let mut device = office();
            device.bands = FakeBands {
                rows: 7,
                padding,
                big_endian,
            };
            fake.add_device(device);
            let mut device = opened(&fake);
            let batch = scan(device.as_mut(), &flatbed_settings(format), 1);
            assert_eq!(batch.done, Ok(Done::OnePage), "{format:?}");
            let Event::Applied(applied) = &batch.events[0] else {
                panic!("Applied first");
            };
            assert_eq!(applied.settings.format, format);
            // 55 pixels across, so a black and white row has bits past its last pixel.
            assert_eq!(assert_patterns(&batch.events), [(55, 45)], "{format:?}");
        }
    }
}

#[test]
fn sheets_are_numbered_on_from_first_sheet() {
    let fake = FakeIcaHandle::new();
    fake.add_device(office().load(FakeSheet::stack(5..=6)));
    let mut device = opened(&fake);
    let batch = scan(device.as_mut(), &feeder_settings(true), 5);
    assert_eq!(batch.done, Ok(Done::FeederEmpty));
    assert_eq!(
        pages(&batch.events),
        [
            (5, Side::Front),
            (5, Side::Back),
            (6, Side::Front),
            (6, Side::Back)
        ]
    );
    assert_patterns(&batch.events);
}

#[test]
fn a_cancel_before_the_first_sheet_asks_nothing_of_the_scanner() {
    let fake = FakeIcaHandle::new();
    fake.add_device(office().load(FakeSheet::stack(1..=2)));
    let mut device = opened(&fake);
    let tracker = Tracker::new();
    tracker.cancel();
    let (events, received) = sync_channel(4);
    let done = device.scan(&feeder_settings(false), 1, &events, &tracker);
    assert_eq!(done, Ok(Done::Cancelled));
    drop(events);
    assert_eq!(kinds(&received.iter().collect::<Vec<_>>()), ["Applied"]);
    assert!(
        !fake
            .calls()
            .iter()
            .any(|call| matches!(call, FakeCall::RequestScan { .. }))
    );
    assert_eq!(fake.sheets_left(OFFICE), 2);
}

#[test]
fn a_cancel_on_a_rendezvous_channel_finishes_the_sheet_in_flight() {
    let fake = FakeIcaHandle::new();
    fake.add_device(office().load(FakeSheet::stack(1..=4)));
    let mut device = opened(&fake);
    let batch = scan_with(
        device.as_mut(),
        &feeder_settings(true),
        1,
        0,
        |event, tracker| {
            if *event == Event::Sheet(2) {
                tracker.cancel();
            }
            true
        },
    );
    assert_eq!(batch.done, Ok(Done::Cancelled));
    assert_eq!(
        pages(&batch.events),
        [
            (1, Side::Front),
            (1, Side::Back),
            (2, Side::Front),
            (2, Side::Back)
        ]
    );
    assert_patterns(&batch.events);
    // The cancel after sheet 2's back was fed, and no sheet fed after it.
    let calls = fake.calls();
    let back = position(
        &calls,
        &FakeCall::Fed {
            sheet: 2,
            side: Side::Back,
        },
    )
    .expect("sheet 2's back fed");
    let cancel = position(&calls, &FakeCall::CancelScan).expect("the scan cancelled");
    assert!(back < cancel, "{calls:?}");
    assert!(
        !calls
            .iter()
            .any(|call| matches!(call, FakeCall::Fed { sheet: 3, .. })),
        "{calls:?}"
    );
    assert_eq!(fake.sheets_left(OFFICE), 2);

    // The next batch gives the rest.
    let batch = scan(device.as_mut(), &feeder_settings(true), 3);
    assert_eq!(batch.done, Ok(Done::FeederEmpty));
    assert_eq!(
        pages(&batch.events),
        [
            (3, Side::Front),
            (3, Side::Back),
            (4, Side::Front),
            (4, Side::Back)
        ]
    );
    assert_patterns(&batch.events);
}

#[test]
fn a_cancel_that_finds_the_next_sheet_feeding_drops_its_page() {
    let fake = FakeIcaHandle::new();
    let mut device = office().load(FakeSheet::stack(1..=4));
    device.feeds_ahead = true;
    fake.add_device(device);
    let mut device = opened(&fake);
    let batch = scan_with(
        device.as_mut(),
        &feeder_settings(true),
        1,
        0,
        |event, tracker| {
            if *event == Event::Sheet(2) {
                tracker.cancel();
            }
            true
        },
    );
    assert_eq!(batch.done, Ok(Done::Cancelled));
    assert_eq!(
        pages(&batch.events),
        [
            (1, Side::Front),
            (1, Side::Back),
            (2, Side::Front),
            (2, Side::Back)
        ]
    );
    // Sheet 3 began feeding as the cancel came; its page was dropped, and it's gone from the
    // feeder.
    let calls = fake.calls();
    let cancel = position(&calls, &FakeCall::CancelScan).expect("the scan cancelled");
    let fed = position(
        &calls,
        &FakeCall::Fed {
            sheet: 3,
            side: Side::Front,
        },
    )
    .expect("sheet 3 fed");
    assert!(cancel < fed, "{calls:?}");
    assert_eq!(fake.sheets_left(OFFICE), 1);
}

#[test]
fn the_receiver_dropped_cancels_the_scan() {
    let fake = FakeIcaHandle::new();
    fake.add_device(office().load(FakeSheet::stack(1..=4)));
    let mut device = opened(&fake);
    let batch = scan_with(
        device.as_mut(),
        &feeder_settings(false),
        1,
        0,
        |event, _| !matches!(event, Event::Page(_)),
    );
    assert_eq!(batch.done, Ok(Done::Cancelled));
    assert_eq!(pages(&batch.events), [(1, Side::Front)]);
    assert!(fake.calls().contains(&FakeCall::CancelScan));
    // Sheet 2 was fed, its page whole as its event's send failed, and not delivered: the model's
    // receiver gone. The device scans the rest after.
    assert_eq!(fake.sheets_left(OFFICE), 2);
    let batch = scan(device.as_mut(), &feeder_settings(false), 2);
    assert_eq!(batch.done, Ok(Done::FeederEmpty));
    assert_eq!(sheets(&batch.events), [2, 3]);
}

#[test]
fn an_error_at_sheet_3_keeps_sheets_1_and_2() {
    let fake = FakeIcaHandle::new();
    let mut stack = FakeSheet::stack(1..=4);
    stack[2] = FakeSheet::new(3).with_fault(
        Side::Front,
        FakeFault::Error(ica_error(RETURN_SCANNER_FAILED_TO_COMPLETE_SCAN)),
    );
    fake.add_device(office().load(stack));
    let mut device = opened(&fake);
    let batch = scan(device.as_mut(), &feeder_settings(true), 1);
    assert!(
        is_driver(&batch.done, "ICReturnScannerFailedToCompleteScan"),
        "{:?}",
        batch.done
    );
    assert_eq!(sheets(&batch.events), [1, 2]);
    assert_eq!(pages(&batch.events).len(), 4);
    assert_patterns(&batch.events);
    // The sheet that failed stays in the feeder, with the one after it.
    assert_eq!(fake.sheets_left(OFFICE), 2);
}

#[test]
fn the_scanner_s_own_cancel_ends_the_batch_cancelled() {
    let fake = FakeIcaHandle::new();
    let mut stack = FakeSheet::stack(1..=3);
    stack[1] = FakeSheet::new(2).with_fault(Side::Front, FakeFault::ScannerCancel);
    fake.add_device(office().load(stack));
    let mut device = opened(&fake);
    let batch = scan(device.as_mut(), &feeder_settings(false), 1);
    assert_eq!(batch.done, Ok(Done::Cancelled));
    assert_eq!(pages(&batch.events), [(1, Side::Front)]);
    // Not the backend's cancel.
    assert!(!fake.calls().contains(&FakeCall::CancelScan));
}

#[test]
fn the_device_removed_mid_page_is_gone() {
    let fake = FakeIcaHandle::new();
    let mut stack = FakeSheet::stack(1..=3);
    stack[1] = FakeSheet::new(2).with_fault(Side::Front, FakeFault::Removed { after_bands: 2 });
    fake.add_device(office().load(stack));
    let mut device = opened(&fake);
    let batch = scan(device.as_mut(), &feeder_settings(false), 1);
    assert_eq!(batch.done, Err(ScanError::Gone));
    assert_eq!(pages(&batch.events), [(1, Side::Front)]);
    // Every call after it is gone too.
    assert_eq!(device.caps(), Err(ScanError::Gone));
    let batch = scan(device.as_mut(), &feeder_settings(false), 2);
    assert_eq!(batch.done, Err(ScanError::Gone));
}

#[test]
fn an_empty_feeder_and_a_duplex_batch_ending_after_a_front() {
    // Empty: before any page, the model's empty feeder, whether the scan ends with no error or
    // with one while the module finds no paper.
    for answer in [
        None,
        Some(ica_error(RETURN_SCANNER_FAILED_TO_COMPLETE_SCAN)),
    ] {
        let fake = FakeIcaHandle::new();
        let mut device = office();
        device.empty_answer = answer.clone();
        fake.add_device(device);
        let mut device = opened(&fake);
        let batch = scan(device.as_mut(), &feeder_settings(false), 1);
        assert_eq!(batch.done, Err(ScanError::FeederEmpty), "{answer:?}");
        assert_eq!(kinds(&batch.events), ["Applied"]);
    }

    // A duplex batch ending after a front: the feeder emptied, the front kept.
    let fake = FakeIcaHandle::new();
    let mut stack = FakeSheet::stack(1..=2);
    stack[1] = FakeSheet::new(2).with_fault(Side::Front, FakeFault::EndAfter);
    fake.add_device(office().load(stack));
    let mut device = opened(&fake);
    let batch = scan(device.as_mut(), &feeder_settings(true), 1);
    assert_eq!(batch.done, Ok(Done::FeederEmpty));
    assert_eq!(
        pages(&batch.events),
        [(1, Side::Front), (1, Side::Back), (2, Side::Front)]
    );
}

#[test]
fn a_band_whose_header_lies_is_refused_the_pages_before_kept() {
    let fake = FakeIcaHandle::new();
    let mut stack = FakeSheet::stack(1..=3);
    stack[1] = FakeSheet::new(2).with_fault(Side::Front, FakeFault::LyingBand { band: 1 });
    fake.add_device(office().load(stack));
    let mut device = opened(&fake);
    let batch = scan(device.as_mut(), &feeder_settings(false), 1);
    assert!(is_driver(&batch.done, "out of order"), "{:?}", batch.done);
    assert_eq!(pages(&batch.events), [(1, Side::Front)]);
    assert!(fake.calls().contains(&FakeCall::CancelScan));
}

#[test]
fn a_callback_s_panic_is_said() {
    let fake = FakeIcaHandle::new();
    let mut stack = FakeSheet::stack(1..=2);
    stack[0] = FakeSheet::new(1).with_fault(Side::Front, FakeFault::Panic { after_bands: 1 });
    fake.add_device(office().load(stack));
    let mut device = opened(&fake);
    let batch = scan(device.as_mut(), &feeder_settings(false), 1);
    assert!(is_driver(&batch.done, "panicked"), "{:?}", batch.done);
    assert!(pages(&batch.events).is_empty());
    assert!(fake.calls().contains(&FakeCall::CancelScan));
}

#[test]
fn warming_up_is_said_between_pages() {
    let fake = FakeIcaHandle::new();
    let mut stack = FakeSheet::stack(1..=2);
    stack[0] = FakeSheet::new(1).with_fault(Side::Front, FakeFault::WarmingUp);
    fake.add_device(office().load(stack));
    let mut device = opened(&fake);
    let batch = scan(device.as_mut(), &feeder_settings(false), 1);
    assert_eq!(batch.done, Ok(Done::FeederEmpty));
    // The status came after the page's first band; it's said once the page is sent.
    assert_eq!(
        kinds(&batch.events),
        [
            "Applied",
            "Sheet(1)",
            "Page(1, Front)",
            &format!("Status({:?})", Status::WarmingUp),
            "Sheet(2)",
            "Page(2, Front)"
        ]
    );
}

#[test]
fn applied_says_what_the_module_took_and_the_pages_come_so() {
    let fake = FakeIcaHandle::new();
    // A module that keeps 100 dpi at 50, and gray at 16 bits at 8.
    fake.add_device(office().with_write_rule(|unit| {
        if unit.resolution == 100 {
            unit.resolution = 50;
        }
        if unit.pixel_type == PIXEL_GRAY && unit.bit_depth == DEPTH_16 {
            unit.bit_depth = DEPTH_8;
        }
    }));
    let mut device = opened(&fake);
    let mut settings = flatbed_settings(Format::Gray16);
    settings.dpi = 100;
    let batch = scan(device.as_mut(), &settings, 1);
    assert_eq!(batch.done, Ok(Done::OnePage));
    let Event::Applied(applied) = &batch.events[0] else {
        panic!("Applied first");
    };
    assert_eq!(applied.settings.dpi, 50);
    assert_eq!(applied.settings.format, Format::Gray8);
    assert_eq!(
        applied.clamped,
        [
            Clamped::Dpi(100),
            Clamped::Format(Format::Gray16),
            Clamped::Paper(Paper::Auto)
        ]
    );
    // The page is what the module took, at 50 dpi.
    assert_eq!(assert_patterns(&batch.events), [(55, 45)]);
    let Event::Page(page) = &batch.events[2] else {
        panic!("a page");
    };
    assert_eq!(page.dpi, (50.0, 50.0));
}

#[cfg(not(target_os = "macos"))]
#[test]
fn off_macos_the_real_backend_is_not_installed_after_a_bad_id() {
    let mut backend = IcaBackend::new();
    let not_installed = ScanError::NotInstalled {
        what: "ImageCaptureCore".to_owned(),
    };
    assert_eq!(backend.list(LONG), Err(not_installed.clone()));
    assert_eq!(
        backend.open(&stored(""), LONG).err(),
        Some(ScanError::BadId)
    );
    assert_eq!(
        backend.open(&stored(OFFICE), LONG).err(),
        Some(not_installed)
    );
}
