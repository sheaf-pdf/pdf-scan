//! The `sane` backend's batches through the fake libsane: the settings set in order and read
//! back for `Applied`, each backend's layout's duplex, sheets and their pages in order (a sheet's
//! event with its front), the frames SANE gives in the raw layout, the feeder's end, the faults
//! and SANE's own cancel, a start refused with the settings, cancels and a slow or gone consumer
//! decided by a rendezvous channel's order, never by timing, and progress in SANE's bytes.
//!
//! The fake's feed is one for every source, and a start that begins a page takes it from the feed
//! whatever it answers: after a start refused, a jam or an open cover, the next start (a retry's
//! or the next batch's) gives the fake's next page, whose bytes are that page's number's. Each
//! test runs in a child process of its own, and the fake's log is checked after it.

#[macro_use]
mod sane_support;

use std::thread;
use std::time::{Duration, Instant};

use sane_support::{
    Count, Fake, FakeLog, Recorder, applied, assert_fake_loaded, batch, batch_with, fake_pixels,
    image, order, pages, settings,
};
use scan_device::in_progress::Tracker;
use scan_device::sane::SaneBackend;
use scan_device::{
    Applied, Backend, Clamped, Device, DeviceId, DeviceInfo, Done, Driver, Event, Format, Paper,
    Resolutions, ScanError, Side, SideKnown, SizeMm, Source,
};

/// A deadline no listing here comes near.
const SOON: Duration = Duration::from_secs(10);

/// The script's one device.
const DEVICE: &str = "device fake:0 Fake Scanner T\n";

/// A source option offering a flatbed and a feeder of one side, at the flatbed; no other option,
/// so pages are 72 dpi, of the format the frames give, with no area.
const FEEDER: &str = "option source STRING 32 soft-select+soft-detect\n\
                      strings Flatbed \"Automatic Document Feeder\"\n\
                      value Flatbed\n";

/// A source option offering a flatbed and a feeder of one side and of both.
const DUPLEX_FEEDER: &str = "option source STRING 16 soft-select+soft-detect\n\
                             strings Flatbed ADF \"ADF Duplex\"\n\
                             value Flatbed\n";

/// The script's device, opened.
fn open(backend: &mut SaneBackend) -> Box<dyn Device> {
    let stored = DeviceInfo::new(
        DeviceId("sane:fake:0".to_owned()),
        "fake:0".to_owned(),
        "Fake".to_owned(),
        "Scanner".to_owned(),
        Driver::Sane,
    );
    let opened = backend.open(&stored, SOON).unwrap();
    assert_fake_loaded();
    opened.device
}

/// The fake's default frame's page `page` (from 1), a back if `back`: 16 by 4 gray pixels.
fn gray_page(page: usize, back: bool) -> (Format, u32, u32, Vec<u8>) {
    (Format::Gray8, 16, 4, fake_pixels(page, back, 0, 16, 4))
}

/// Each page's image, owned, to compare.
fn images(events: &[Event]) -> Vec<(Format, u32, u32, Vec<u8>)> {
    pages(events)
        .into_iter()
        .map(|page| {
            let (format, width, height, data) = image(page);
            (format, width, height, data.to_vec())
        })
        .collect()
}

/// The `sane_start` calls the log holds.
fn starts(log: &FakeLog) -> usize {
    log.calls_of("sane_start").len()
}

/// Fails unless the log's sets of option `before` all come before its first set of `after`.
fn set_before(log: &FakeLog, before: usize, after: usize) {
    let befores = log.positions_of_sets(before);
    let afters = log.positions_of_sets(after);
    assert!(!befores.is_empty(), "option {before} was never set");
    assert!(!afters.is_empty(), "option {after} was never set");
    assert!(
        befores.last() < afters.first(),
        "option {before} was set after option {after}: {:?}",
        log.option_sets()
    );
}

/// A test of a duplex batch on a backend's layout: the profile, the option that turns its duplex
/// on and the value it's set to, its source option where the batch sets it, and its page height.
macro_rules! duplex_on {
    ($test:ident, $profile:literal, $duplex:expr, $source:expr, $page_height:expr) => {
        #[test]
        #[cfg_attr(not(unix), ignore = "loads the fake by SANE's name: none on Windows")]
        fn $test() {
            let script = format!("{DEVICE}profile {}\nduplex\nsheets 2\n", $profile);
            let fake = Fake::new(stringify!($test), &script);
            let ran = in_child!($test, fake.child(), || {
                let mut device = open(&mut SaneBackend::new());
                let mut asked = settings(Source::Feeder, true, 300, Format::Gray8);
                asked.paper = Paper::Size(SizeMm::LETTER);
                let (events, done) = batch(&mut *device, &asked, 5);
                assert_eq!(done, Ok(Done::FeederEmpty));
                assert_eq!(*applied(&events), Applied::new(asked.clone(), Vec::new()));
                assert_eq!(
                    order(&events),
                    [
                        "applied",
                        "sheet 5",
                        "page 5 front",
                        "page 5 back",
                        "sheet 6",
                        "page 6 front",
                        "page 6 back"
                    ]
                );
                // Fronts marked F and backs B, the fake's pages 1 to 4.
                assert_eq!(
                    images(&events),
                    [
                        gray_page(1, false),
                        gray_page(2, true),
                        gray_page(3, false),
                        gray_page(4, true)
                    ]
                );
                for page in pages(&events) {
                    assert_eq!(page.side_known, SideKnown::Inferred);
                    assert_eq!(page.dpi, (300.0, 300.0));
                }
            });
            if ran {
                let log = fake.log();
                log.assert_no_violations();
                let (option, value): (usize, &str) = $duplex;
                assert!(
                    log.option_sets().contains(&(option, value)),
                    "option {option} wasn't set to {value}: {:?}",
                    log.option_sets()
                );
                let source: Option<usize> = $source;
                if let Some(source) = source {
                    set_before(&log, source, $page_height);
                }
                set_before(&log, option, $page_height);
                // Two sheets of two pages, then the empty feeder's start.
                assert_eq!(starts(&log), 5);
            }
        }
    };
}

duplex_on!(
    a_duplex_batch_on_fujitsu_s_layout,
    "fujitsu",
    (2, "\"ADF Duplex\""),
    Some(2),
    11
);
duplex_on!(
    a_duplex_batch_on_airscan_s_layout,
    "airscan",
    (2, "\"ADF Duplex\""),
    Some(2),
    11
);
duplex_on!(
    a_duplex_batch_on_brother_s_layout,
    "brother",
    (4, "\"Automatic Document Feeder(left aligned,Duplex)\""),
    Some(4),
    11
);
// One source, never set: duplex is its `ScanMode`.
duplex_on!(
    a_duplex_batch_on_canon_dr_s_layout,
    "canon-dr",
    (3, "\"Duplex\""),
    None,
    12
);
duplex_on!(
    a_duplex_batch_on_lexmark_s_layout,
    "lexmark",
    (3, "[1]"),
    Some(2),
    12
);
duplex_on!(
    a_duplex_batch_on_epson_s_layout,
    "epson",
    (3, "\"Duplex\""),
    Some(2),
    12
);
duplex_on!(
    a_duplex_batch_on_kodak_s_layout,
    "kodak",
    (3, "\"both\""),
    None,
    12
);

#[test]
#[cfg_attr(not(unix), ignore = "loads the fake by SANE's name: none on Windows")]
fn airscan_s_duplex_batch_asked_at_600_dpi_is_set_to_300_and_says_so() {
    let script = format!("{DEVICE}profile airscan\nduplex\nsheets 1\n");
    let fake = Fake::new(
        "airscan_s_duplex_batch_asked_at_600_dpi_is_set_to_300_and_says_so",
        &script,
    );
    let ran = in_child!(
        airscan_s_duplex_batch_asked_at_600_dpi_is_set_to_300_and_says_so,
        fake.child(),
        || {
            let mut device = open(&mut SaneBackend::new());
            // The feeder's caps are its one side's: 75 to 600 dpi.
            let caps = device.caps().unwrap();
            let feeder = caps
                .sources
                .iter()
                .find(|source| source.source == Source::Feeder)
                .unwrap();
            assert!(feeder.duplex);
            assert_eq!(
                feeder.resolutions,
                Resolutions::List(vec![75, 100, 150, 200, 300, 600])
            );
            let mut asked = settings(Source::Feeder, true, 600, Format::Rgb8);
            asked.paper = Paper::Size(SizeMm::LETTER);
            let (events, done) = batch(&mut *device, &asked, 1);
            assert_eq!(done, Ok(Done::FeederEmpty));
            // Both sides allow 300 at most.
            let mut taken = asked.clone();
            taken.dpi = 300;
            assert_eq!(
                *applied(&events),
                Applied::new(taken, vec![Clamped::Dpi(600)])
            );
            for page in pages(&events) {
                assert_eq!(page.dpi, (300.0, 300.0));
            }
            assert_eq!(pages(&events).len(), 2);
        }
    );
    if ran {
        let log = fake.log();
        log.assert_no_violations();
        // The resolution (option 4) set to 300 once `ADF Duplex` allowed no more.
        let resolutions: Vec<&str> = log
            .option_sets()
            .into_iter()
            .filter(|(option, _)| *option == 4)
            .map(|(_, value)| value)
            .collect();
        assert_eq!(resolutions, ["[300]"]);
    }
}

#[test]
#[cfg_attr(not(unix), ignore = "loads the fake by SANE's name: none on Windows")]
fn a_simplex_feeder_s_sheets_come_in_order_each_sheet_with_its_front_then_it_empties() {
    let script = format!("{DEVICE}{FEEDER}sheets 3\n");
    let fake = Fake::new(
        "a_simplex_feeder_s_sheets_come_in_order_each_sheet_with_its_front_then_it_empties",
        &script,
    );
    let ran = in_child!(
        a_simplex_feeder_s_sheets_come_in_order_each_sheet_with_its_front_then_it_empties,
        fake.child(),
        || {
            let mut device = open(&mut SaneBackend::new());
            let asked = settings(Source::Feeder, false, 300, Format::Gray8);
            let tracker = Tracker::new();
            let (events, done) = batch_with(&mut *device, &asked, 1, &tracker, |_| true);
            assert_eq!(done, Ok(Done::FeederEmpty));
            // No resolution option: 72 dpi, said; no area options: the driver's paper.
            let mut taken = asked.clone();
            taken.dpi = 72;
            assert_eq!(
                *applied(&events),
                Applied::new(taken, vec![Clamped::Dpi(300)])
            );
            assert_eq!(
                order(&events),
                [
                    "applied",
                    "sheet 1",
                    "page 1 front",
                    "sheet 2",
                    "page 2 front",
                    "sheet 3",
                    "page 3 front"
                ]
            );
            assert_eq!(
                images(&events),
                [
                    gray_page(1, false),
                    gray_page(2, false),
                    gray_page(3, false)
                ]
            );
            for page in pages(&events) {
                assert_eq!(page.dpi, (72.0, 72.0));
            }
            // The last page's bytes, counted once the batch is back.
            let snapshot = tracker.snapshot();
            assert_eq!((snapshot.total, snapshot.done), (Some(64), 64));
        }
    );
    if ran {
        let log = fake.log();
        log.assert_no_violations();
        // Three pages, then the start that found the feeder empty; the source set to the feeder.
        assert_eq!(starts(&log), 4);
        assert_eq!(log.sets(), ["\"Automatic Document Feeder\""]);
    }
}

#[test]
#[cfg_attr(not(unix), ignore = "loads the fake by SANE's name: none on Windows")]
fn an_empty_feeder_is_feeder_empty_after_applied_with_no_sheet() {
    let script = format!("{DEVICE}{FEEDER}sheets 0\n");
    let fake = Fake::new(
        "an_empty_feeder_is_feeder_empty_after_applied_with_no_sheet",
        &script,
    );
    let ran = in_child!(
        an_empty_feeder_is_feeder_empty_after_applied_with_no_sheet,
        fake.child(),
        || {
            let mut device = open(&mut SaneBackend::new());
            let asked = settings(Source::Feeder, false, 72, Format::Gray8);
            let (events, done) = batch(&mut *device, &asked, 1);
            assert_eq!(done, Err(ScanError::FeederEmpty));
            assert_eq!(order(&events), ["applied"]);
        }
    );
    if ran {
        let log = fake.log();
        log.assert_no_violations();
        assert_eq!(starts(&log), 1);
    }
}

/// 1-bit color expanded: each bit 255 or 0, a byte of eight pixels a channel, red, green, blue.
fn expanded(data: &[u8], width: usize, stride: usize) -> Vec<u8> {
    let mut pixels = Vec::new();
    for row in data.chunks(stride) {
        for x in 0..width {
            for channel in 0..3 {
                let byte = row[x / 8 * 3 + channel];
                pixels.push(if byte >> (7 - x % 8) & 1 == 1 { 255 } else { 0 });
            }
        }
    }
    pixels
}

/// 16-bit samples in this machine's order, as SANE gives them, made little-endian.
fn little_endian(data: &[u8]) -> Vec<u8> {
    data.chunks(2)
        .flat_map(|sample| u16::from_ne_bytes([sample[0], sample[1]]).to_le_bytes())
        .collect()
}

/// A test of a flatbed's page of one frame: the frame's line, what the page is to be, the format
/// asked.
macro_rules! frame_page {
    ($test:ident, $frame:literal, $format:expr, $expected:expr) => {
        #[test]
        #[cfg_attr(not(unix), ignore = "loads the fake by SANE's name: none on Windows")]
        fn $test() {
            let script = format!("{DEVICE}frame {}\n", $frame);
            let fake = Fake::new(stringify!($test), &script);
            let ran = in_child!($test, fake.child(), || {
                let mut device = open(&mut SaneBackend::new());
                let asked = settings(Source::Flatbed, false, 72, $format);
                let (events, done) = batch(&mut *device, &asked, 1);
                assert_eq!(done, Ok(Done::OnePage));
                assert_eq!(*applied(&events), Applied::new(asked, Vec::new()));
                let expected: (Format, u32, u32, Vec<u8>) = $expected;
                assert_eq!(images(&events), [expected]);
            });
            if ran {
                fake.log().assert_no_violations();
            }
        }
    };
}

frame_page!(
    a_frame_of_1_bit_color_is_expanded_to_8_bits,
    "RGB 1 6 16 4 1",
    Format::Rgb8,
    (
        Format::Rgb8,
        16,
        4,
        expanded(&fake_pixels(1, false, 0, 6, 4), 16, 6)
    )
);
frame_page!(
    a_frame_of_16_bit_gray_is_little_endian,
    "GRAY 1 32 16 4 16",
    Format::Gray16,
    (
        Format::Gray16,
        16,
        4,
        little_endian(&fake_pixels(1, false, 0, 32, 4))
    )
);
frame_page!(
    a_frame_of_16_bit_color_is_little_endian,
    "RGB 1 96 16 4 16",
    Format::Rgb16,
    (
        Format::Rgb16,
        16,
        4,
        little_endian(&fake_pixels(1, false, 0, 96, 4))
    )
);

#[test]
#[cfg_attr(not(unix), ignore = "loads the fake by SANE's name: none on Windows")]
fn a_frame_of_unknown_height_has_no_total_until_it_ends() {
    let script = format!("{DEVICE}frame GRAY 1 16 16 -1 8\nread-max 16\n");
    let fake = Fake::new(
        "a_frame_of_unknown_height_has_no_total_until_it_ends",
        &script,
    );
    let ran = in_child!(
        a_frame_of_unknown_height_has_no_total_until_it_ends,
        fake.child(),
        || {
            let mut device = open(&mut SaneBackend::new());
            let asked = settings(Source::Flatbed, false, 72, Format::Gray8);
            let recorder = Recorder::new();
            let (events, done) = batch_with(&mut *device, &asked, 1, &recorder, |_| true);
            assert_eq!(done, Ok(Done::OnePage));
            // Four lines: its height known once it ends.
            assert_eq!(images(&events), [gray_page(1, false)]);
            assert_eq!(
                recorder.counts(),
                [
                    Count::Total(None),
                    Count::Done(0),
                    Count::Done(16),
                    Count::Done(32),
                    Count::Done(48),
                    Count::Done(64),
                    Count::Total(Some(64)),
                ]
            );
        }
    );
    if ran {
        fake.log().assert_no_violations();
    }
}

#[test]
#[cfg_attr(not(unix), ignore = "loads the fake by SANE's name: none on Windows")]
fn a_page_of_known_height_has_its_total_from_its_first_read() {
    let script = format!("{DEVICE}read-max 32\n");
    let fake = Fake::new(
        "a_page_of_known_height_has_its_total_from_its_first_read",
        &script,
    );
    let ran = in_child!(
        a_page_of_known_height_has_its_total_from_its_first_read,
        fake.child(),
        || {
            let mut device = open(&mut SaneBackend::new());
            let asked = settings(Source::Flatbed, false, 72, Format::Gray8);
            let recorder = Recorder::new();
            let (_, done) = batch_with(&mut *device, &asked, 1, &recorder, |_| true);
            assert_eq!(done, Ok(Done::OnePage));
            assert_eq!(
                recorder.counts(),
                [
                    Count::Total(None),
                    Count::Done(0),
                    Count::Total(Some(64)),
                    Count::Done(32),
                    Count::Done(64),
                ]
            );
        }
    );
    if ran {
        fake.log().assert_no_violations();
    }
}

#[test]
#[cfg_attr(not(unix), ignore = "loads the fake by SANE's name: none on Windows")]
fn a_jam_at_sheet_4_keeps_sheets_1_to_3_and_a_second_batch_goes_on_from_sheet_4() {
    let script = format!("{DEVICE}{FEEDER}sheets 10\nat 4 read JAMMED 10\n");
    let fake = Fake::new(
        "a_jam_at_sheet_4_keeps_sheets_1_to_3_and_a_second_batch_goes_on_from_sheet_4",
        &script,
    );
    let ran = in_child!(
        a_jam_at_sheet_4_keeps_sheets_1_to_3_and_a_second_batch_goes_on_from_sheet_4,
        fake.child(),
        || {
            let mut device = open(&mut SaneBackend::new());
            let asked = settings(Source::Feeder, false, 72, Format::Gray8);
            let (events, done) = batch(&mut *device, &asked, 1);
            assert_eq!(done, Err(ScanError::Jammed));
            assert_eq!(
                images(&events),
                [
                    gray_page(1, false),
                    gray_page(2, false),
                    gray_page(3, false)
                ]
            );
            assert_eq!(order(&events).last().unwrap(), "page 3 front");
            // Continue: the fake's next page is its fifth, sheet 4 to the app.
            let (events, done) = batch(&mut *device, &asked, 4);
            assert_eq!(done, Ok(Done::FeederEmpty));
            let sheets: Vec<u32> = pages(&events).iter().map(|page| page.sheet).collect();
            assert_eq!(sheets, [4, 5, 6, 7, 8, 9]);
            let expected: Vec<_> = (5..=10).map(|page| gray_page(page, false)).collect();
            assert_eq!(images(&events), expected);
        }
    );
    if ran {
        fake.log().assert_no_violations();
    }
}

#[test]
#[cfg_attr(not(unix), ignore = "loads the fake by SANE's name: none on Windows")]
fn faults_end_a_batch_as_the_model_names_them_its_pages_before_kept() {
    // The fake's pages, a batch after another: the second's start answers COVER_OPEN; the third
    // fails mid-page with IO_ERROR; the fourth's read is refused; the fifth's start is busy.
    let script = format!(
        "{DEVICE}{FEEDER}at 2 start COVER_OPEN\nat 3 read IO_ERROR 10\n\
         at 4 read ACCESS_DENIED\nat 5 start DEVICE_BUSY\n"
    );
    let fake = Fake::new(
        "faults_end_a_batch_as_the_model_names_them_its_pages_before_kept",
        &script,
    );
    let ran = in_child!(
        faults_end_a_batch_as_the_model_names_them_its_pages_before_kept,
        fake.child(),
        || {
            let mut device = open(&mut SaneBackend::new());
            let asked = settings(Source::Feeder, false, 72, Format::Gray8);
            let (events, done) = batch(&mut *device, &asked, 1);
            assert_eq!(done, Err(ScanError::CoverOpen));
            assert_eq!(images(&events), [gray_page(1, false)]);
            for (fault, sheet) in [
                (ScanError::Gone, 2),
                (ScanError::AccessDenied, 2),
                (ScanError::Busy, 2),
            ] {
                let (events, done) = batch(&mut *device, &asked, sheet);
                assert_eq!(done, Err(fault));
                assert_eq!(order(&events), ["applied"]);
            }
            // The sixth goes through.
            let flatbed = settings(Source::Flatbed, false, 72, Format::Gray8);
            let (events, done) = batch(&mut *device, &flatbed, 2);
            assert_eq!(done, Ok(Done::OnePage));
            assert_eq!(images(&events), [gray_page(6, false)]);
        }
    );
    if ran {
        fake.log().assert_no_violations();
    }
}

#[test]
#[cfg_attr(not(unix), ignore = "loads the fake by SANE's name: none on Windows")]
fn no_docs_from_a_read_after_pages_ends_the_feeder_and_sane_s_own_cancel_is_cancelled() {
    let script = format!("{DEVICE}{FEEDER}at 3 read NO_DOCS\nat 5 read CANCELLED 10\n");
    let fake = Fake::new(
        "no_docs_from_a_read_after_pages_ends_the_feeder_and_sane_s_own_cancel_is_cancelled",
        &script,
    );
    let ran = in_child!(
        no_docs_from_a_read_after_pages_ends_the_feeder_and_sane_s_own_cancel_is_cancelled,
        fake.child(),
        || {
            let mut device = open(&mut SaneBackend::new());
            let asked = settings(Source::Feeder, false, 72, Format::Gray8);
            let (events, done) = batch(&mut *device, &asked, 1);
            assert_eq!(done, Ok(Done::FeederEmpty));
            assert_eq!(pages(&events).len(), 2);
            // A cancel SANE gives (the scanner's button) ends the batch cancelled, the page
            // before kept.
            let (events, done) = batch(&mut *device, &asked, 3);
            assert_eq!(done, Ok(Done::Cancelled));
            assert_eq!(images(&events), [gray_page(4, false)]);
        }
    );
    if ran {
        fake.log().assert_no_violations();
    }
}

#[test]
#[cfg_attr(not(unix), ignore = "loads the fake by SANE's name: none on Windows")]
fn parameters_of_2500_by_0_are_the_driver_s_words_at_the_first_page() {
    let script = format!("{DEVICE}{FEEDER}frame GRAY 1 2500 2500 0 8\n");
    let fake = Fake::new(
        "parameters_of_2500_by_0_are_the_driver_s_words_at_the_first_page",
        &script,
    );
    let ran = in_child!(
        parameters_of_2500_by_0_are_the_driver_s_words_at_the_first_page,
        fake.child(),
        || {
            let mut device = open(&mut SaneBackend::new());
            let asked = settings(Source::Feeder, false, 72, Format::Gray8);
            let (events, done) = batch(&mut *device, &asked, 1);
            match done {
                Err(ScanError::Driver { message, shown }) => {
                    assert!(!shown);
                    assert!(message.contains("parameters"), "{message}");
                }
                other => panic!("not the driver's words: {other:?}"),
            }
            assert_eq!(order(&events), ["applied"]);
        }
    );
    if ran {
        fake.log().assert_no_violations();
    }
}

/// A source option and a mode option, which a start's `INVAL` makes inactive.
const INVAL_LAYOUT: &str = "option source STRING 32 soft-select+soft-detect\n\
                            strings Flatbed \"Automatic Document Feeder\"\n\
                            value Flatbed\n\
                            option mode STRING 16 soft-select+soft-detect\n\
                            strings Gray Color\n\
                            value Color\n\
                            start-inval-deactivates mode\n";

#[test]
#[cfg_attr(not(unix), ignore = "loads the fake by SANE's name: none on Windows")]
fn a_start_refused_with_the_settings_sets_them_again_and_starts_once_more() {
    let script = format!("{DEVICE}{INVAL_LAYOUT}sheets 2\nat 1 start INVAL\n");
    let fake = Fake::new(
        "a_start_refused_with_the_settings_sets_them_again_and_starts_once_more",
        &script,
    );
    let ran = in_child!(
        a_start_refused_with_the_settings_sets_them_again_and_starts_once_more,
        fake.child(),
        || {
            let mut device = open(&mut SaneBackend::new());
            let asked = settings(Source::Feeder, false, 72, Format::Gray8);
            let (events, done) = batch(&mut *device, &asked, 1);
            assert_eq!(done, Ok(Done::FeederEmpty));
            // One `Applied`, and the page the start once more took, the fake's second, as sheet 1.
            assert_eq!(order(&events), ["applied", "sheet 1", "page 1 front"]);
            assert_eq!(*applied(&events), Applied::new(asked, Vec::new()));
            assert_eq!(images(&events), [gray_page(2, false)]);
        }
    );
    if ran {
        let log = fake.log();
        log.assert_no_violations();
        let refused = log.positions_of("sane_start")[0];
        // The source (option 1) set again after the start refused; the mode (option 2), gone
        // inactive, not.
        assert!(
            log.positions_of_sets(1).iter().any(|&set| set > refused),
            "{:?}",
            log.option_sets()
        );
        assert!(log.positions_of_sets(2).iter().all(|&set| set < refused));
        assert_eq!(starts(&log), 3);
    }
}

#[test]
#[cfg_attr(not(unix), ignore = "loads the fake by SANE's name: none on Windows")]
fn a_start_refused_again_is_settings_refused() {
    let script = format!("{DEVICE}{INVAL_LAYOUT}sheets 3\nat 1 start INVAL\nat 2 start INVAL\n");
    let fake = Fake::new("a_start_refused_again_is_settings_refused", &script);
    let ran = in_child!(
        a_start_refused_again_is_settings_refused,
        fake.child(),
        || {
            let mut device = open(&mut SaneBackend::new());
            let asked = settings(Source::Feeder, false, 72, Format::Gray8);
            let (events, done) = batch(&mut *device, &asked, 1);
            assert_eq!(done, Err(ScanError::SettingsRefused));
            assert_eq!(order(&events), ["applied"]);
        }
    );
    if ran {
        let log = fake.log();
        log.assert_no_violations();
        assert_eq!(starts(&log), 2);
    }
}

#[test]
#[cfg_attr(not(unix), ignore = "loads the fake by SANE's name: none on Windows")]
fn a_start_refused_at_a_duplex_sheet_s_back_ends_the_batch_its_front_kept() {
    // Sheet 1's back is the fake's second page.
    let script = format!("{DEVICE}{DUPLEX_FEEDER}duplex\nsheets 2\nat 2 start INVAL\n");
    let fake = Fake::new(
        "a_start_refused_at_a_duplex_sheet_s_back_ends_the_batch_its_front_kept",
        &script,
    );
    let ran = in_child!(
        a_start_refused_at_a_duplex_sheet_s_back_ends_the_batch_its_front_kept,
        fake.child(),
        || {
            let mut device = open(&mut SaneBackend::new());
            let asked = settings(Source::Feeder, true, 72, Format::Gray8);
            let (events, done) = batch(&mut *device, &asked, 1);
            match done {
                Err(ScanError::Driver { message, shown }) => {
                    assert!(!shown);
                    assert!(message.contains("sheet 1's back"), "{message}");
                }
                other => panic!("not the driver's words: {other:?}"),
            }
            // Never started again mid-sheet, which would give the next sheet's front as this
            // sheet's back: the front alone, kept.
            assert_eq!(order(&events), ["applied", "sheet 1", "page 1 front"]);
            assert_eq!(images(&events), [gray_page(1, false)]);
        }
    );
    if ran {
        let log = fake.log();
        log.assert_no_violations();
        assert_eq!(starts(&log), 2);
        // The settings never set again after the back's start: the source (option 1) among them.
        let refused = log.positions_of("sane_start")[1];
        assert!(
            log.positions_of_sets(1).iter().all(|&set| set < refused),
            "{:?}",
            log.option_sets()
        );
    }
}

/// A source option offering a flatbed and a feeder of one side, at the flatbed, which a start's
/// `INVAL` sets back there.
const INVAL_RESETS: &str = "option source STRING 32 soft-select+soft-detect\n\
                            strings Flatbed \"Automatic Document Feeder\"\n\
                            value Flatbed\n\
                            start-inval-resets source\n";

#[test]
#[cfg_attr(not(unix), ignore = "loads the fake by SANE's name: none on Windows")]
fn a_setting_found_as_asked_that_a_start_s_inval_moves_is_set_back() {
    let script = format!("{DEVICE}{INVAL_RESETS}sheets 4\nat 2 start INVAL\n");
    let fake = Fake::new(
        "a_setting_found_as_asked_that_a_start_s_inval_moves_is_set_back",
        &script,
    );
    let ran = in_child!(
        a_setting_found_as_asked_that_a_start_s_inval_moves_is_set_back,
        fake.child(),
        || {
            let mut device = open(&mut SaneBackend::new());
            let asked = settings(Source::Feeder, false, 72, Format::Gray8);
            // The first batch sets the feeder, and is cancelled after its first sheet.
            let tracker = Tracker::new();
            let cancel = tracker.clone();
            let (events, done) = batch_with(&mut *device, &asked, 1, &tracker, |event| {
                if *event == Event::Sheet(1) {
                    cancel.cancel();
                }
                true
            });
            assert_eq!(done, Ok(Done::Cancelled));
            assert_eq!(images(&events), [gray_page(1, false)]);
            // The second finds the feeder set, so sets nothing; its start's `INVAL` moves the
            // source back to the flatbed, which is set to the feeder again, and the page started
            // once more.
            let (events, done) = batch(&mut *device, &asked, 2);
            assert_eq!(done, Ok(Done::FeederEmpty));
            assert_eq!(*applied(&events), Applied::new(asked, Vec::new()));
            assert_eq!(
                order(&events)[1..],
                ["sheet 2", "page 2 front", "sheet 3", "page 3 front"]
            );
            assert_eq!(images(&events), [gray_page(3, false), gray_page(4, false)]);
        }
    );
    if ran {
        let log = fake.log();
        log.assert_no_violations();
        // Set once by the first batch, then once after the second's start refused.
        let refused = log.positions_of("sane_start")[1];
        let sets = log.positions_of_sets(1);
        assert_eq!(sets.len(), 2, "{:?}", log.option_sets());
        assert!(
            sets[0] < refused && sets[1] > refused,
            "{:?}",
            log.option_sets()
        );
        assert_eq!(starts(&log), 5);
    }
}

#[test]
#[cfg_attr(not(unix), ignore = "loads the fake by SANE's name: none on Windows")]
fn a_cancel_as_sheet_2_is_taken_finishes_it_and_the_next_batch_gives_the_rest() {
    let script = format!("{DEVICE}{FEEDER}sheets 10\n");
    let fake = Fake::new(
        "a_cancel_as_sheet_2_is_taken_finishes_it_and_the_next_batch_gives_the_rest",
        &script,
    );
    let ran = in_child!(
        a_cancel_as_sheet_2_is_taken_finishes_it_and_the_next_batch_gives_the_rest,
        fake.child(),
        || {
            let mut device = open(&mut SaneBackend::new());
            let asked = settings(Source::Feeder, false, 72, Format::Gray8);
            let tracker = Tracker::new();
            let cancel = tracker.clone();
            // The backend's next `send` waits on this thread, so the cancel comes before it
            // checks again, whatever the timing.
            let (events, done) = batch_with(&mut *device, &asked, 1, &tracker, |event| {
                if *event == Event::Sheet(2) {
                    cancel.cancel();
                }
                true
            });
            assert_eq!(done, Ok(Done::Cancelled));
            assert_eq!(
                order(&events),
                [
                    "applied",
                    "sheet 1",
                    "page 1 front",
                    "sheet 2",
                    "page 2 front"
                ]
            );
            let (events, done) = batch(&mut *device, &asked, 3);
            assert_eq!(done, Ok(Done::FeederEmpty));
            let expected: Vec<_> = (3..=10).map(|page| gray_page(page, false)).collect();
            assert_eq!(images(&events), expected);
        }
    );
    if ran {
        let log = fake.log();
        log.assert_no_violations();
        // Two pages, then eight and the empty feeder's start.
        assert_eq!(starts(&log), 11);
    }
}

#[test]
#[cfg_attr(not(unix), ignore = "loads the fake by SANE's name: none on Windows")]
fn a_cancel_as_a_duplex_sheet_s_front_is_taken_still_gives_its_back() {
    let script = format!("{DEVICE}{DUPLEX_FEEDER}duplex\nsheets 4\n");
    let fake = Fake::new(
        "a_cancel_as_a_duplex_sheet_s_front_is_taken_still_gives_its_back",
        &script,
    );
    let ran = in_child!(
        a_cancel_as_a_duplex_sheet_s_front_is_taken_still_gives_its_back,
        fake.child(),
        || {
            let mut device = open(&mut SaneBackend::new());
            let asked = settings(Source::Feeder, true, 72, Format::Gray8);
            let tracker = Tracker::new();
            let cancel = tracker.clone();
            let (events, done) = batch_with(&mut *device, &asked, 1, &tracker, |event| {
                if let Event::Page(page) = event
                    && (page.sheet, page.side) == (2, Side::Front)
                {
                    cancel.cancel();
                }
                true
            });
            assert_eq!(done, Ok(Done::Cancelled));
            assert_eq!(
                order(&events),
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
            // The next batch starts at sheet 3's front, the fake's fifth page.
            let (events, done) = batch(&mut *device, &asked, 3);
            assert_eq!(done, Ok(Done::FeederEmpty));
            assert_eq!(
                order(&events)[1..],
                [
                    "sheet 3",
                    "page 3 front",
                    "page 3 back",
                    "sheet 4",
                    "page 4 front",
                    "page 4 back"
                ]
            );
            assert_eq!(
                images(&events),
                [
                    gray_page(5, false),
                    gray_page(6, true),
                    gray_page(7, false),
                    gray_page(8, true)
                ]
            );
        }
    );
    if ran {
        fake.log().assert_no_violations();
    }
}

#[test]
#[cfg_attr(not(unix), ignore = "loads the fake by SANE's name: none on Windows")]
fn a_consumer_that_stops_taking_holds_the_batch_after_one_page_read_whole() {
    let script = format!("{DEVICE}{FEEDER}sheets 3\n");
    let fake = Fake::new(
        "a_consumer_that_stops_taking_holds_the_batch_after_one_page_read_whole",
        &script,
    );
    let ran = in_child!(
        a_consumer_that_stops_taking_holds_the_batch_after_one_page_read_whole,
        fake.child(),
        || {
            let mut device = open(&mut SaneBackend::new());
            let asked = settings(Source::Feeder, false, 72, Format::Gray8);
            let mut held = false;
            let (events, done) = batch_with(&mut *device, &asked, 1, &Tracker::new(), |event| {
                if *event == Event::Sheet(1) {
                    return true;
                }
                if let Event::Page(page) = event
                    && page.sheet == 1
                {
                    // Taking no more: the backend reads the next page whole, then holds.
                    let started = Instant::now();
                    while fake.log().frames_ended() < 2 {
                        assert!(
                            started.elapsed() < Duration::from_secs(10),
                            "no second page"
                        );
                        thread::sleep(Duration::from_millis(5));
                    }
                    thread::sleep(Duration::from_millis(200));
                    let log = fake.log();
                    assert_eq!(starts(&log), 2, "a start past the page held");
                    assert_eq!(log.frames_ended(), 2);
                    held = true;
                }
                true
            });
            assert!(held);
            assert_eq!(done, Ok(Done::FeederEmpty));
            assert_eq!(pages(&events).len(), 3);
        }
    );
    if ran {
        fake.log().assert_no_violations();
    }
}

#[test]
#[cfg_attr(not(unix), ignore = "loads the fake by SANE's name: none on Windows")]
fn the_receiver_dropped_ends_the_batch_cancelled_with_no_start_after() {
    let script = format!("{DEVICE}{FEEDER}sheets 3\n");
    let fake = Fake::new(
        "the_receiver_dropped_ends_the_batch_cancelled_with_no_start_after",
        &script,
    );
    let ran = in_child!(
        the_receiver_dropped_ends_the_batch_cancelled_with_no_start_after,
        fake.child(),
        || {
            let mut device = open(&mut SaneBackend::new());
            let asked = settings(Source::Feeder, false, 72, Format::Gray8);
            let (events, done) = batch_with(&mut *device, &asked, 1, &Tracker::new(), |event| {
                *event != Event::Sheet(1)
            });
            assert_eq!(done, Ok(Done::Cancelled));
            assert_eq!(order(&events), ["applied", "sheet 1"]);
        }
    );
    if ran {
        let log = fake.log();
        log.assert_no_violations();
        assert_eq!(starts(&log), 1);
    }
}

#[test]
#[cfg_attr(not(unix), ignore = "loads the fake by SANE's name: none on Windows")]
fn a_flatbed_gives_one_page_with_one_start() {
    let script = format!("{DEVICE}{FEEDER}sheets 5\n");
    let fake = Fake::new("a_flatbed_gives_one_page_with_one_start", &script);
    let ran = in_child!(
        a_flatbed_gives_one_page_with_one_start,
        fake.child(),
        || {
            let mut device = open(&mut SaneBackend::new());
            let asked = settings(Source::Flatbed, false, 72, Format::Gray8);
            let (events, done) = batch(&mut *device, &asked, 7);
            assert_eq!(done, Ok(Done::OnePage));
            assert_eq!(order(&events), ["applied", "sheet 7", "page 7 front"]);
            assert_eq!(images(&events), [gray_page(1, false)]);
        }
    );
    if ran {
        let log = fake.log();
        log.assert_no_violations();
        assert_eq!(starts(&log), 1);
        // At the flatbed already: the source isn't set.
        assert!(log.sets().is_empty(), "{:?}", log.sets());
    }
}

#[test]
#[cfg_attr(not(unix), ignore = "loads the fake by SANE's name: none on Windows")]
fn no_sheet_is_numbered_past_u32_max() {
    let script = format!("{DEVICE}{FEEDER}sheets 3\n");
    let fake = Fake::new("no_sheet_is_numbered_past_u32_max", &script);
    let ran = in_child!(no_sheet_is_numbered_past_u32_max, fake.child(), || {
        let mut device = open(&mut SaneBackend::new());
        let asked = settings(Source::Feeder, false, 72, Format::Gray8);
        let (events, done) = batch(&mut *device, &asked, u32::MAX);
        match done {
            Err(ScanError::Driver { message, .. }) => {
                assert!(message.contains("past 4294967295"), "{message}");
            }
            other => panic!("not the driver's words: {other:?}"),
        }
        let last = format!("page {} front", u32::MAX);
        assert_eq!(order(&events)[1..], [format!("sheet {}", u32::MAX), last]);
    });
    if ran {
        let log = fake.log();
        log.assert_no_violations();
        // The sheet after it never started.
        assert_eq!(starts(&log), 1);
    }
}

#[test]
#[cfg_attr(not(unix), ignore = "loads the fake by SANE's name: none on Windows")]
fn a_cancel_as_sheet_u32_max_is_taken_ends_the_batch_cancelled() {
    let script = format!("{DEVICE}{FEEDER}sheets 3\n");
    let fake = Fake::new(
        "a_cancel_as_sheet_u32_max_is_taken_ends_the_batch_cancelled",
        &script,
    );
    let ran = in_child!(
        a_cancel_as_sheet_u32_max_is_taken_ends_the_batch_cancelled,
        fake.child(),
        || {
            let mut device = open(&mut SaneBackend::new());
            let asked = settings(Source::Feeder, false, 72, Format::Gray8);
            let tracker = Tracker::new();
            let cancel = tracker.clone();
            // A cancel is checked before the next sheet is numbered: cancelled, not past the
            // last number.
            let (events, done) = batch_with(&mut *device, &asked, u32::MAX, &tracker, |event| {
                if *event == Event::Sheet(u32::MAX) {
                    cancel.cancel();
                }
                true
            });
            assert_eq!(done, Ok(Done::Cancelled));
            let last = format!("page {} front", u32::MAX);
            assert_eq!(order(&events)[1..], [format!("sheet {}", u32::MAX), last]);
        }
    );
    if ran {
        let log = fake.log();
        log.assert_no_violations();
        assert_eq!(starts(&log), 1);
    }
}

#[test]
#[cfg_attr(not(unix), ignore = "loads the fake by SANE's name: none on Windows")]
fn an_area_in_pixels_is_set_down_the_page_at_y_resolution() {
    // y-resolution takes 600 dpi alone, whatever x-resolution is set to.
    let script = format!(
        "{DEVICE}\
         option x-resolution INT 4 soft-select+soft-detect\nunit DPI\nwords 300\nvalue 300\n\
         option y-resolution INT 4 soft-select+soft-detect\nunit DPI\nwords 600\nvalue 600\n\
         option tl-x INT 4 soft-select+soft-detect\nunit PIXEL\nrange 0 2550 1\n\
         option tl-y INT 4 soft-select+soft-detect\nunit PIXEL\nrange 0 6600 1\n\
         option br-x INT 4 soft-select+soft-detect\nunit PIXEL\nrange 0 2550 1\nvalue 2550\n\
         option br-y INT 4 soft-select+soft-detect\nunit PIXEL\nrange 0 6600 1\nvalue 6600\n"
    );
    let fake = Fake::new(
        "an_area_in_pixels_is_set_down_the_page_at_y_resolution",
        &script,
    );
    let ran = in_child!(
        an_area_in_pixels_is_set_down_the_page_at_y_resolution,
        fake.child(),
        || {
            let mut device = open(&mut SaneBackend::new());
            let mut asked = settings(Source::Flatbed, false, 300, Format::Gray8);
            asked.paper = Paper::Size(SizeMm::LETTER);
            let (events, done) = batch(&mut *device, &asked, 1);
            assert_eq!(done, Ok(Done::OnePage));
            // The whole of Letter: 3300 pixels down at 600 dpi is 139.7 mm.
            assert_eq!(*applied(&events), Applied::new(asked, Vec::new()));
            assert_eq!(pages(&events)[0].dpi, (300.0, 600.0));
        }
    );
    if ran {
        let log = fake.log();
        log.assert_no_violations();
        // Options 1 to 6: x and y, tl-x and tl-y, br-x and br-y.
        let last = |option| {
            log.option_sets()
                .into_iter()
                .rfind(|&(set, _)| set == option)
        };
        assert_eq!(last(5).map(|(_, value)| value), Some("[2550]"));
        assert_eq!(last(6).map(|(_, value)| value), Some("[6600]"));
    }
}

#[test]
#[cfg_attr(not(unix), ignore = "loads the fake by SANE's name: none on Windows")]
fn an_inactive_resolution_is_left_and_an_active_x_resolution_set() {
    let script = format!(
        "{DEVICE}\
         option resolution INT 4 soft-select+soft-detect+inactive\n\
         unit DPI\nwords 100 200 300\nvalue 100\n\
         option x-resolution INT 4 soft-select+soft-detect\nunit DPI\nwords 150 300\nvalue 150\n\
         option y-resolution INT 4 soft-select+soft-detect\nunit DPI\nwords 150 300\nvalue 150\n"
    );
    let fake = Fake::new(
        "an_inactive_resolution_is_left_and_an_active_x_resolution_set",
        &script,
    );
    let ran = in_child!(
        an_inactive_resolution_is_left_and_an_active_x_resolution_set,
        fake.child(),
        || {
            let mut device = open(&mut SaneBackend::new());
            let asked = settings(Source::Flatbed, false, 300, Format::Gray8);
            let (events, done) = batch(&mut *device, &asked, 1);
            assert_eq!(done, Ok(Done::OnePage));
            assert_eq!(*applied(&events), Applied::new(asked, Vec::new()));
            assert_eq!(pages(&events)[0].dpi, (300.0, 300.0));
        }
    );
    if ran {
        let log = fake.log();
        log.assert_no_violations();
        // x-resolution and y-resolution (options 2 and 3) set; the inactive one never.
        assert_eq!(log.option_sets(), [(2, "[300]"), (3, "[300]")]);
    }
}

#[test]
#[cfg_attr(not(unix), ignore = "loads the fake by SANE's name: none on Windows")]
fn a_resolution_read_back_as_0_fails_the_batch_before_applied() {
    let script = format!("{DEVICE}option resolution INT 4 soft-select+soft-detect\nvalue 0\n");
    let fake = Fake::new(
        "a_resolution_read_back_as_0_fails_the_batch_before_applied",
        &script,
    );
    let ran = in_child!(
        a_resolution_read_back_as_0_fails_the_batch_before_applied,
        fake.child(),
        || {
            let mut device = open(&mut SaneBackend::new());
            let asked = settings(Source::Flatbed, false, 300, Format::Gray8);
            let (events, done) = batch(&mut *device, &asked, 1);
            match done {
                Err(ScanError::Driver { message, .. }) => {
                    assert!(message.contains("resolution reads back as 0"), "{message}");
                }
                other => panic!("not the driver's words: {other:?}"),
            }
            assert!(events.is_empty(), "{events:?}");
        }
    );
    if ran {
        let log = fake.log();
        log.assert_no_violations();
        assert_eq!(starts(&log), 0);
    }
}

#[test]
#[cfg_attr(not(unix), ignore = "loads the fake by SANE's name: none on Windows")]
fn an_option_set_by_name_is_set_again_after_the_settings_and_applied_says_what_it_moved() {
    let script = format!(
        "{DEVICE}{FEEDER}option resolution INT 4 soft-select+soft-detect\nunit DPI\n\
         range 50 600 1\nvalue 300\nsheets 1\n"
    );
    let fake = Fake::new(
        "an_option_set_by_name_is_set_again_after_the_settings_and_applied_says_what_it_moved",
        &script,
    );
    let ran = in_child!(
        an_option_set_by_name_is_set_again_after_the_settings_and_applied_says_what_it_moved,
        fake.child(),
        || {
            let mut device = open(&mut SaneBackend::new().with_option("resolution", "150"));
            let asked = settings(Source::Feeder, false, 300, Format::Gray8);
            let (events, done) = batch(&mut *device, &asked, 1);
            assert_eq!(done, Ok(Done::FeederEmpty));
            // The device holds what the option by name set, after the model's 300.
            let mut taken = asked.clone();
            taken.dpi = 150;
            assert_eq!(
                *applied(&events),
                Applied::new(taken, vec![Clamped::Dpi(300)])
            );
            assert_eq!(pages(&events)[0].dpi, (150.0, 150.0));
        }
    );
    if ran {
        let log = fake.log();
        log.assert_no_violations();
        // The resolution (option 2): by name as the device opened; then the batch's 300, then by
        // name again.
        let resolutions: Vec<&str> = log
            .option_sets()
            .into_iter()
            .filter(|(option, _)| *option == 2)
            .map(|(_, value)| value)
            .collect();
        assert_eq!(resolutions, ["[150]", "[300]", "[150]"]);
    }
}
