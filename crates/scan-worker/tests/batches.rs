//! Batches through a worker: every event and page the one the scripted backend gives in
//! process, the errors that end a batch after its pages, progress, and what can't be sent.

mod support;

use std::sync::mpsc;

use scan_worker::scan_device::in_progress::{Level, Tracker, Unit};
use scan_worker::scan_device::{
    Done, Encoding, Event, Format, Paper, Pixels, ScanError, Side, SideKnown, SizeMm, Source,
    Status,
};
use scan_worker::{Error, Limits, ProtocolError};

use support::{
    client_of, is_scan, open, open_in_process, pages, pages_are_pattern_s, scan, scan_in_process,
    settings,
};

/// Each batch through `scenario`'s worker, and through its script in process, gives the same
/// events and ends the same: the worker's `(events, end)` for each.
fn same_as_in_process(
    scenario: &str,
    count: usize,
    batches: &[(scan_worker::scan_device::Settings, u32)],
) -> Vec<(Vec<Event>, Result<Done, Error>)> {
    let mut device = open(&client_of(scenario, count));
    let mut in_process = open_in_process(scenario, count);
    let mut through = Vec::new();
    for (settings, first_sheet) in batches {
        let (events, ended) = scan(&mut device, settings, *first_sheet);
        let (expected, expected_end) = scan_in_process(&mut *in_process, settings, *first_sheet);
        assert!(
            events == expected,
            "{scenario}, {settings:?} from {first_sheet}: the events differ"
        );
        match (&ended, &expected_end) {
            (Ok(done), Ok(expected)) => assert_eq!(done, expected),
            (Err(Error::Scan(error)), Err(expected)) => assert_eq!(error, expected),
            _ => panic!("{scenario}: {ended:?}, where in process {expected_end:?}"),
        }
        through.push((events, ended));
    }
    device.close().expect("closed");
    through
}

/// Each page's sheet and side, in order.
fn sides(events: &[Event]) -> Vec<(u32, Side)> {
    pages(events)
        .iter()
        .map(|page| (page.sheet, page.side))
        .collect()
}

#[test]
fn batches_in_each_format_are_the_fake_s_page_for_page() {
    let formats = [
        Format::Bw1,
        Format::Gray8,
        Format::Gray16,
        Format::Rgb8,
        Format::Rgb16,
    ];
    let batches: Vec<_> = formats
        .iter()
        .map(|format| (settings(Source::Feeder, 75, *format, true), 1))
        .collect();
    // Two new sheets in the feeder for each batch.
    let through = same_as_in_process("scan-more", 2, &batches);
    for ((events, ended), format) in through.iter().zip(formats) {
        assert_eq!(ended.as_ref().unwrap(), &Done::FeederEmpty);
        pages_are_pattern_s(events);
        for page in pages(events) {
            let Pixels::Raw(image) = &page.pixels else {
                panic!("a raw page");
            };
            assert_eq!(image.format(), format);
            assert_eq!(page.dpi, (75.0, 75.0));
            assert_eq!(page.side_known, SideKnown::Inferred);
        }
    }
    // Each batch took its two sheets, front and back, numbered from 1.
    for (events, _) in &through {
        assert_eq!(
            sides(events),
            [
                (1, Side::Front),
                (1, Side::Back),
                (2, Side::Front),
                (2, Side::Back)
            ]
        );
    }
}

#[test]
fn encoded_pages_are_the_fake_s_as_its_driver_gave_them() {
    // One sheet in the feeder, both its sides in CCITT G4; then the glass, three times.
    let mut g4 = settings(Source::Feeder, 300, Format::Bw1, true);
    g4.encodings = vec![Encoding::CcittG4];
    let mut jpeg = settings(Source::Flatbed, 150, Format::Gray8, false);
    jpeg.encodings = vec![Encoding::Jpeg, Encoding::Png];
    // JPEG doesn't fit 16-bit color, so PNG, the next asked for.
    let mut png = settings(Source::Flatbed, 75, Format::Rgb16, false);
    png.encodings = vec![Encoding::Jpeg, Encoding::Png];
    let mut tiff = settings(Source::Flatbed, 75, Format::Gray16, false);
    tiff.encodings = vec![Encoding::Tiff];
    let through = same_as_in_process("encodings", 1, &[(g4, 1), (jpeg, 3), (png, 7), (tiff, 8)]);
    let kinds: Vec<_> = through
        .iter()
        .flat_map(|(events, _)| pages(events))
        .map(|page| match &page.pixels {
            Pixels::Encoded(encoded) => format!("{:?} {}", encoded.kind, encoded.bytes.len()),
            Pixels::Raw(_) => "raw".to_owned(),
            _ => "unknown".to_owned(),
        })
        .collect();
    assert_eq!(kinds.len(), 5, "{kinds:?}");
    assert!(kinds[0].starts_with("CcittG4"), "{kinds:?}");
    assert!(kinds[1].starts_with("CcittG4"), "{kinds:?}");
    assert!(kinds[2].starts_with("Jpeg"), "{kinds:?}");
    assert!(kinds[3].starts_with("Png"), "{kinds:?}");
    assert!(kinds[4].starts_with("Tiff"), "{kinds:?}");
    // Numbered from each batch's first sheet.
    let sheets: Vec<u32> = through
        .iter()
        .flat_map(|(events, _)| pages(events))
        .map(|page| page.sheet)
        .collect();
    assert_eq!(sheets, [1, 1, 3, 7, 8]);
}

#[test]
fn the_flatbed_gives_its_one_page_numbered_as_asked() {
    let through = same_as_in_process(
        "feeder",
        0,
        &[(settings(Source::Flatbed, 150, Format::Rgb8, false), 5)],
    );
    let (events, ended) = &through[0];
    assert_eq!(ended.as_ref().unwrap(), &Done::OnePage);
    assert_eq!(sides(events), [(5, Side::Front)]);
    pages_are_pattern_s(events);
}

#[test]
fn a_side_the_driver_says_is_said_so() {
    let through = same_as_in_process(
        "reports-sides",
        1,
        &[(settings(Source::Feeder, 100, Format::Gray8, true), 1)],
    );
    let (events, _) = &through[0];
    assert_eq!(sides(events), [(1, Side::Front), (1, Side::Back)]);
    assert!(
        pages(events)
            .iter()
            .all(|page| page.side_known == SideKnown::Reported)
    );
}

#[test]
fn scan_more_numbers_on_from_the_first_sheet_given() {
    let batch = settings(Source::Feeder, 100, Format::Gray8, false);
    let through = same_as_in_process("scan-more", 2, &[(batch.clone(), 1), (batch, 3)]);
    assert_eq!(sides(&through[0].0), [(1, Side::Front), (2, Side::Front)]);
    assert_eq!(sides(&through[1].0), [(3, Side::Front), (4, Side::Front)]);
    for (events, ended) in &through {
        pages_are_pattern_s(events);
        assert_eq!(ended.as_ref().unwrap(), &Done::FeederEmpty);
    }
}

#[test]
fn a_jam_ends_a_batch_after_its_pages_and_continue_goes_on_in_the_same_worker() {
    let duplex = settings(Source::Feeder, 100, Format::Gray8, true);
    let through = same_as_in_process(
        "jams",
        0,
        &[(duplex.clone(), 1), (duplex.clone(), 2), (duplex, 4)],
    );
    // Sheet 2's back jammed midway: the pages before it kept, its front among them.
    assert!(is_scan(&through[0].1, &ScanError::Jammed));
    assert_eq!(
        sides(&through[0].0),
        [(1, Side::Front), (1, Side::Back), (2, Side::Front)]
    );
    // Continue from sheet 2, put back: its front again, then on to sheet 4's front, which
    // jams as it starts, after its sheet is said.
    assert!(is_scan(&through[1].1, &ScanError::Jammed));
    assert_eq!(
        sides(&through[1].0),
        [
            (2, Side::Front),
            (2, Side::Back),
            (3, Side::Front),
            (3, Side::Back)
        ]
    );
    assert_eq!(through[1].0.last(), Some(&Event::Sheet(4)));
    // And from sheet 4, to the feeder's end.
    assert_eq!(through[2].1.as_ref().unwrap(), &Done::FeederEmpty);
    assert_eq!(
        sides(&through[2].0),
        [
            (4, Side::Front),
            (4, Side::Back),
            (5, Side::Front),
            (5, Side::Back)
        ]
    );
    for (events, _) in &through {
        pages_are_pattern_s(events);
    }
}

#[test]
fn a_double_feed_or_an_open_cover_ends_a_batch_after_its_pages() {
    for (scenario, error) in [
        ("double-feed", ScanError::DoubleFeed),
        ("cover-open", ScanError::CoverOpen),
    ] {
        let simplex = settings(Source::Feeder, 100, Format::Bw1, false);
        let through = same_as_in_process(scenario, 0, &[(simplex.clone(), 1), (simplex, 2)]);
        assert!(
            is_scan(&through[0].1, &error),
            "{scenario}: {:?}",
            through[0].1
        );
        assert_eq!(sides(&through[0].0), [(1, Side::Front)], "{scenario}");
        // Continue: sheet 2, put back, and sheet 3.
        assert_eq!(
            sides(&through[1].0),
            [(2, Side::Front), (3, Side::Front)],
            "{scenario}"
        );
    }
}

#[test]
fn an_empty_feeder_is_an_error_before_any_page() {
    let through = same_as_in_process(
        "feeder",
        0,
        &[(settings(Source::Feeder, 100, Format::Gray8, false), 1)],
    );
    let (events, ended) = &through[0];
    assert!(is_scan(ended, &ScanError::FeederEmpty), "{ended:?}");
    assert!(matches!(events[..], [Event::Applied(_)]), "{events:?}");
}

#[test]
fn a_device_gone_answers_gone_to_every_call_after() {
    let simplex = settings(Source::Feeder, 100, Format::Gray8, false);
    let through = same_as_in_process("gone", 0, &[(simplex.clone(), 1), (simplex.clone(), 2)]);
    assert!(is_scan(&through[0].1, &ScanError::Gone));
    assert_eq!(sides(&through[0].0), [(1, Side::Front)]);
    // The batch after it, refused before any event.
    assert!(is_scan(&through[1].1, &ScanError::Gone));
    assert!(through[1].0.is_empty(), "{:?}", through[1].0);
    // And every other call, the worker going on.
    let mut device = open(&client_of("gone", 0));
    let _ = scan(&mut device, &simplex, 1);
    assert!(matches!(device.caps(), Err(Error::Scan(ScanError::Gone))));
    assert!(matches!(
        device.settings_window(None),
        Err(Error::Scan(ScanError::Gone))
    ));
    assert!(is_scan(&scan(&mut device, &simplex, 1).1, &ScanError::Gone));
    device.close().expect("closed");
}

#[test]
fn warming_up_is_said_before_the_first_sheet() {
    let through = same_as_in_process(
        "warm-up",
        1,
        &[(settings(Source::Feeder, 100, Format::Gray8, false), 1)],
    );
    let (events, _) = &through[0];
    assert!(
        matches!(
            events[..3],
            [
                Event::Applied(_),
                Event::Status(Status::WarmingUp),
                Event::Sheet(1)
            ]
        ),
        "{events:?}"
    );
}

#[test]
fn progress_reaches_the_caller_s_tracker() {
    let mut device = open(&client_of("feeder", 2));
    let tracker = Tracker::new();
    let (events, received) = mpsc::sync_channel(64);
    let done = device
        .scan(
            &settings(Source::Feeder, 150, Format::Gray8, false),
            1,
            &events,
            &tracker,
        )
        .expect("a batch");
    drop(events);
    assert_eq!(done, Done::FeederEmpty);
    let events: Vec<Event> = received.iter().collect();
    let Some(Pixels::Raw(last)) = pages(&events).last().map(|page| &page.pixels) else {
        panic!("a raw page last");
    };
    // Letter at 150 dpi in gray.
    let bytes = last.data().len() as u64;
    assert_eq!(bytes, 1275 * 1650);
    let snapshot = tracker.snapshot();
    assert_eq!(
        (snapshot.done, snapshot.total, snapshot.unit),
        (bytes, Some(bytes), Unit::Bytes)
    );
    assert!(!snapshot.cancelled);
    device.close().expect("closed");
}

#[test]
fn a_driver_s_message_reaches_the_caller_s_tracker() {
    let mut device = open(&client_of("talking", 1));
    let tracker = Tracker::new();
    let (events, received) = mpsc::sync_channel(64);
    let done = device
        .scan(
            &settings(Source::Feeder, 100, Format::Bw1, false),
            1,
            &events,
            &tracker,
        )
        .expect("a batch");
    drop(events);
    assert_eq!(done, Done::FeederEmpty);
    assert_eq!(pages(&received.iter().collect::<Vec<_>>()).len(), 1);
    let snapshot = tracker.snapshot();
    assert_eq!(
        snapshot.message,
        Some((Level::Warn, support::scenarios::SAID.to_owned()))
    );
    assert_eq!(snapshot.message_count, 1);
    device.close().expect("closed");
}

#[test]
fn a_size_json_can_t_hold_is_unsendable_and_the_worker_goes_on() {
    let mut device = open(&client_of("feeder", 1));
    for width in [f32::NAN, f32::INFINITY] {
        let mut batch = settings(Source::Feeder, 100, Format::Gray8, false);
        batch.paper = Paper::Size(SizeMm {
            width,
            height: 297.0,
        });
        let (events, ended) = scan(&mut device, &batch, 1);
        assert!(matches!(ended, Err(Error::Unsendable)), "{ended:?}");
        assert!(events.is_empty(), "{events:?}");
    }
    // Nothing was sent: the sheet is still there for the next.
    let (events, ended) = scan(
        &mut device,
        &settings(Source::Feeder, 100, Format::Gray8, false),
        1,
    );
    assert_eq!(ended.unwrap(), Done::FeederEmpty);
    assert_eq!(sides(&events), [(1, Side::Front)]);
    pages_are_pattern_s(&events);
    device.close().expect("closed");
}

#[test]
fn a_page_past_the_cap_is_refused_before_it_s_read_and_ends_the_worker() {
    let mut limits = Limits::default();
    limits.page = 1 << 20;
    let mut device = client_of("feeder", 1)
        .limits(limits)
        .open(&support::stored(), support::AT_MOST)
        .expect("opened");
    // The glass at 75 dpi in gray, about half a megabyte, under the cap.
    let small = scan(
        &mut device,
        &settings(Source::Flatbed, 75, Format::Gray8, false),
        1,
    );
    assert_eq!(sides(&small.0), [(1, Side::Front)]);
    // Letter at 150 dpi in gray, 2,103,750 bytes, past it.
    let (events, ended) = scan(
        &mut device,
        &settings(Source::Feeder, 150, Format::Gray8, false),
        2,
    );
    assert!(
        matches!(
            ended,
            Err(Error::Protocol(ProtocolError::PageTooLarge {
                bytes: 2_103_750,
                limit: 1_048_576
            }))
        ),
        "{ended:?}"
    );
    // The events before it kept, the page not among them.
    assert!(
        matches!(events[..], [Event::Applied(_), Event::Sheet(2)]),
        "{events:?}"
    );
    assert!(matches!(device.caps(), Err(Error::Ended)));
    assert!(matches!(
        scan(
            &mut device,
            &settings(Source::Feeder, 75, Format::Gray8, false),
            3
        )
        .1,
        Err(Error::Ended)
    ));
    // Closed already.
    device.close().expect("closed");
}
