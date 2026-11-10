//! The `sane` backend through SANE's own `test` and `pnm` backends: devices listed, opened and
//! what each can do, an option set by name, a device opened once at a time; the configuration of
//! a worker that opens a stored device, a directory naming its backend then one standing for the
//! system's, never the system's own: aliases opened through it, a stale index gone, and an alias
//! whose target is gone listed after it's opened. And batches: the `test` backend's feeder of ten
//! sheets, again and again; its five formats in the raw layout; a hand scanner's unknown height;
//! three-pass color in each order; the statuses it's made to read; a cancel; a flatbed's one page
//! and Letter cut to its area; and `pnm`'s files, page for page. Each test runs in a child process
//! of its own; where there's no libsane, each says it skipped, or fails where
//! `SANE_DL_REQUIRE_LIBSANE=1`.

#[macro_use]
mod sane_support;

use std::fs;
use std::time::Duration;

use sane_support::{
    Count, Recorder, applied, batch_with, image, installed, order, pages, real_child, settings,
    stand_in_child, test_dir,
};
use scan_device::sane::SaneBackend;
use scan_device::{
    Applied, Backend, Clamped, Device, DeviceId, DeviceInfo, Done, Driver, Event, Format, Paper,
    Resolutions, ScanError, Settings, SizeMm, Source,
};

/// A deadline no listing of these backends comes near.
const SOON: Duration = Duration::from_secs(30);

/// The `DeviceInfo` an app stored for `id`, of `maker` and `model`: its name the id's after
/// `sane:`, as a listing gives it.
fn stored(id: &str, maker: &str, model: &str) -> DeviceInfo {
    DeviceInfo::new(
        DeviceId(id.to_owned()),
        id.strip_prefix("sane:").unwrap_or(id).to_owned(),
        maker.to_owned(),
        model.to_owned(),
        Driver::Sane,
    )
}

/// A `test` backend's device, as its listing gives it.
fn tester(id: &str) -> DeviceInfo {
    stored(id, "Noname", "frontend-tester")
}

/// Each listed device's id.
fn ids(listed: &[DeviceInfo]) -> Vec<&str> {
    listed.iter().map(|device| device.id.0.as_str()).collect()
}

#[test]
fn the_test_backend_is_listed_opened_and_read() {
    let child = real_child("the_test_backend_is_listed_opened_and_read", &["test"], &[]);
    in_child!(the_test_backend_is_listed_opened_and_read, child, || {
        let mut backend = SaneBackend::new();
        let listed = installed(backend.list(SOON)).unwrap();
        assert_eq!(listed, [tester("sane:test:0"), tester("sane:test:1")]);
        let mut opened = backend.open(&listed[0], SOON).unwrap();
        assert_eq!(opened.info, listed[0]);
        assert_eq!(opened.renamed_from, None);
        let caps = opened.device.caps().unwrap();
        assert!(!caps.settings_window);
        // Gray at 1, 8 and 16 bits, the first black and white; color at 8 and 16. A FIXED range of
        // 1 to 1200 dpi. Its scan area, 200 by 200 mm.
        let formats = [
            Format::Bw1,
            Format::Gray8,
            Format::Gray16,
            Format::Rgb8,
            Format::Rgb16,
        ];
        let sources: Vec<_> = caps
            .sources
            .iter()
            .map(|source| {
                (
                    source.source,
                    source.duplex,
                    source.formats.as_slice(),
                    &source.resolutions,
                    source.max_area,
                    source.paper_auto,
                    source.encodings.is_empty(),
                )
            })
            .collect();
        let resolutions = Resolutions::Range {
            min: 1,
            max: 1200,
            step: std::num::NonZeroU32::new(1).unwrap(),
        };
        let area = Some(SizeMm {
            width: 200.0,
            height: 200.0,
        });
        assert_eq!(
            sources,
            [
                (
                    Source::Flatbed,
                    false,
                    &formats[..],
                    &resolutions,
                    area,
                    false,
                    true
                ),
                (
                    Source::Feeder,
                    false,
                    &formats[..],
                    &resolutions,
                    area,
                    false,
                    true
                ),
            ]
        );
        assert_eq!(
            opened.device.settings_window(None),
            Err(ScanError::Unsupported)
        );
    });
}

/// A 13 by 3 PBM, its rows' padding bits set, as a scanner's padding might be.
const PBM: &[u8] = b"P4\n13 3\n\xaa\xbf\x55\x47\xff\xff";

#[test]
fn pnm_opens_with_its_file_set_by_name_and_one_device_at_a_time() {
    let test = "pnm_opens_with_its_file_set_by_name_and_one_device_at_a_time";
    let file = test_dir(test).join("page.pbm");
    let child = real_child(test, &["pnm"], &[]).prepare({
        let file = file.clone();
        move |_| fs::write(&file, PBM).expect("couldn't write the PBM")
    });
    in_child!(
        pnm_opens_with_its_file_set_by_name_and_one_device_at_a_time,
        child,
        || {
            let path = file.to_str().expect("the test's path is text");
            let mut backend = SaneBackend::new().with_option("filename", path);
            let listed = installed(backend.list(SOON)).unwrap();
            assert_eq!(
                listed,
                [
                    stored("sane:pnm:0", "Noname", "PNM file reader"),
                    stored("sane:pnm:1", "Noname", "PNM file reader"),
                ]
            );
            let mut opened = backend.open(&listed[0], SOON).unwrap();
            // No source, mode or area option: one flatbed, the file's format as the parameters
            // give it, its resolutions a word list, no largest area.
            let caps = opened.device.caps().unwrap();
            assert_eq!(caps.sources.len(), 1);
            let flatbed = &caps.sources[0];
            assert_eq!(flatbed.source, Source::Flatbed);
            assert!(!flatbed.duplex);
            assert_eq!(flatbed.formats, [Format::Bw1]);
            assert_eq!(
                flatbed.resolutions,
                Resolutions::List(vec![
                    75, 90, 100, 120, 135, 150, 165, 180, 195, 200, 210, 225, 240, 255, 270, 285,
                    300
                ])
            );
            assert_eq!(flatbed.max_area, None);
            // pnm opens one device at a time.
            for again in [&listed[0], &listed[1]] {
                let second = backend.open(again, SOON).map(|opened| opened.info);
                assert_eq!(second, Err(ScanError::Busy), "{}", again.id.0);
            }
            drop(opened);
            backend.open(&listed[1], SOON).unwrap();
        }
    );
}

/// The stand-in's aliases: one whose name names no backend, one whose prefix names none.
const ALIASES: &str = "alias office:scan test:0\nalias \"Desk scanner\" test:1\n";

/// The stand-in's `test.conf`: the `test` backend's own configuration, read from it.
const TEST_CONF: &str = "number_of_devices 2\n";

#[test]
fn an_alias_naming_no_backend_opens_through_the_stand_in_with_none_named() {
    let child = stand_in_child(
        "an_alias_naming_no_backend_opens_through_the_stand_in_with_none_named",
        &[],
        &[("dll.aliases", ALIASES), ("test.conf", TEST_CONF)],
    );
    in_child!(
        an_alias_naming_no_backend_opens_through_the_stand_in_with_none_named,
        child,
        || {
            let mut backend = SaneBackend::new();
            assert_eq!(installed(backend.list(SOON)).unwrap(), []);
            let desk = tester("sane:Desk scanner");
            let opened = backend.open(&desk, SOON).unwrap();
            assert_eq!(opened.info, desk);
            assert_eq!(opened.renamed_from, None);
        }
    );
}

#[test]
fn an_alias_whose_prefix_names_no_backend_opens_through_the_stand_in() {
    let child = stand_in_child(
        "an_alias_whose_prefix_names_no_backend_opens_through_the_stand_in",
        &["office"],
        &[("dll.aliases", ALIASES), ("test.conf", TEST_CONF)],
    );
    in_child!(
        an_alias_whose_prefix_names_no_backend_opens_through_the_stand_in,
        child,
        || {
            let mut backend = SaneBackend::new();
            assert_eq!(installed(backend.list(SOON)).unwrap(), []);
            let office = tester("sane:office:scan");
            let opened = backend.open(&office, SOON).unwrap();
            assert_eq!(opened.info, office);
            assert_eq!(opened.renamed_from, None);
        }
    );
}

#[test]
fn a_stale_index_of_the_named_backend_is_gone() {
    let child = stand_in_child(
        "a_stale_index_of_the_named_backend_is_gone",
        &["test"],
        &[("dll.aliases", ALIASES), ("test.conf", TEST_CONF)],
    );
    in_child!(a_stale_index_of_the_named_backend_is_gone, child, || {
        let mut backend = SaneBackend::new();
        // That backend's devices alone, as the stand-in's aliases name them.
        let listed = installed(backend.list(SOON)).unwrap();
        assert_eq!(ids(&listed), ["sane:office:scan", "sane:Desk scanner"]);
        let stale = backend.open(&tester("sane:test:9"), SOON);
        assert_eq!(stale.map(|opened| opened.info), Err(ScanError::Gone));
    });
}

#[test]
fn an_alias_naming_no_backend_whose_target_is_gone_is_gone_listed_first() {
    let child = stand_in_child(
        "an_alias_naming_no_backend_whose_target_is_gone_is_gone_listed_first",
        &[],
        &[
            ("dll.aliases", "alias \"Desk scanner\" test:9\n"),
            ("test.conf", TEST_CONF),
        ],
    );
    in_child!(
        an_alias_naming_no_backend_whose_target_is_gone_is_gone_listed_first,
        child,
        || {
            let mut backend = SaneBackend::new();
            let desk = installed(backend.open(&tester("sane:Desk scanner"), SOON));
            assert_eq!(desk.map(|opened| opened.info), Err(ScanError::Gone));
            // SANE added the alias's target's backend as it opened it, and lists it now; the open
            // listed before it opened, so its fallback saw none of it.
            let listed = backend.list(SOON).unwrap();
            assert!(ids(&listed).contains(&"sane:test:0"), "{:?}", ids(&listed));
        }
    );
}

#[test]
fn an_alias_whose_prefix_names_no_backend_and_whose_target_is_gone_is_gone_listed_first() {
    let child = stand_in_child(
        "an_alias_whose_prefix_names_no_backend_and_whose_target_is_gone_is_gone_listed_first",
        &["office"],
        &[
            ("dll.aliases", "alias office:scan test:9\n"),
            ("test.conf", TEST_CONF),
        ],
    );
    in_child!(
        an_alias_whose_prefix_names_no_backend_and_whose_target_is_gone_is_gone_listed_first,
        child,
        || {
            let mut backend = SaneBackend::new();
            let office = installed(backend.open(&tester("sane:office:scan"), SOON));
            assert_eq!(office.map(|opened| opened.info), Err(ScanError::Gone));
            let listed = backend.list(SOON).unwrap();
            assert!(ids(&listed).contains(&"sane:test:0"), "{:?}", ids(&listed));
        }
    );
}

#[test]
#[cfg(windows)]
fn on_windows_sane_isn_t_installed() {
    let not_installed = ScanError::NotInstalled {
        what: "SANE".to_owned(),
    };
    let mut backend = SaneBackend::new();
    assert_eq!(backend.list(SOON), Err(not_installed.clone()));
    // Tried again, and an open the same; a stored id no listing gives is refused first.
    assert_eq!(backend.list(SOON), Err(not_installed.clone()));
    let opened = backend.open(&tester("sane:test:0"), SOON);
    assert_eq!(opened.map(|opened| opened.info), Err(not_installed));
    let bad = backend.open(&tester("sane::0"), SOON);
    assert_eq!(bad.map(|opened| opened.info), Err(ScanError::BadId));
}

/// The `test` backend's configuration for batches: two devices, its reads limited to 64 bytes, as
/// a paced [`Recorder`] needs.
const PACED: &str = "number_of_devices 2\nread-limit true\nread-limit-size 64\n";

/// The `test` backend's own scan area at its first values, 80 by 100 mm: 31 by 39 pixels at 10
/// dpi.
const SMALL: SizeMm = SizeMm {
    width: 80.0,
    height: 100.0,
};

/// `test:0`, opened by `backend`; a skip where there's no SANE.
fn open_tester(backend: &mut SaneBackend) -> Box<dyn Device> {
    installed(backend.open(&tester("sane:test:0"), SOON))
        .unwrap()
        .device
}

/// A batch paced for the `test` backend, every event taken: each page's frames `frame_bytes`
/// long where given.
fn paced(
    device: &mut dyn Device,
    settings: &Settings,
    first_sheet: u32,
    frame_bytes: Option<u64>,
) -> (Vec<Event>, Result<Done, ScanError>) {
    let progress = Recorder::paced(frame_bytes);
    batch_with(device, settings, first_sheet, &progress, |_| true)
}

/// Each page's sheet.
fn sheets(events: &[Event]) -> Vec<u32> {
    pages(events).iter().map(|page| page.sheet).collect()
}

#[test]
fn the_test_backend_s_feeder_gives_10_pages_then_is_empty_then_10_more() {
    let test = "the_test_backend_s_feeder_gives_10_pages_then_is_empty_then_10_more";
    let child = real_child(test, &["test"], &[("test.conf", PACED)]);
    in_child!(
        the_test_backend_s_feeder_gives_10_pages_then_is_empty_then_10_more,
        child,
        || {
            let mut device = open_tester(&mut SaneBackend::new());
            let asked = settings(Source::Feeder, false, 10, Format::Gray8);
            let (events, done) = paced(&mut *device, &asked, 1, None);
            assert_eq!(done, Ok(Done::FeederEmpty));
            // The paper the driver's: the largest area, 200 by 200 mm, said.
            let mut taken = asked.clone();
            taken.paper = Paper::Size(SizeMm {
                width: 200.0,
                height: 200.0,
            });
            assert_eq!(
                *applied(&events),
                Applied::new(taken, vec![Clamped::Paper(Paper::Auto)])
            );
            assert_eq!(sheets(&events), (1..=10).collect::<Vec<_>>());
            for page in pages(&events) {
                // 200 mm at 10 dpi, the whole pixels of 78.7; its first picture solid black.
                let (format, width, height, data) = image(page);
                assert_eq!((format, width, height), (Format::Gray8, 78, 78));
                assert!(data.iter().all(|&byte| byte == 0));
                assert_eq!(page.dpi, (10.0, 10.0));
            }
            let (events, done) = paced(&mut *device, &asked, 11, None);
            assert_eq!(done, Ok(Done::FeederEmpty));
            assert_eq!(sheets(&events), (11..=20).collect::<Vec<_>>());
        }
    );
}

/// The longest run of samples, along the rows, each the one before plus `step`, the samples read
/// as little-endian.
fn longest_run(data: &[u8], width: usize, step: u16) -> usize {
    let samples: Vec<u16> = data
        .chunks(2)
        .map(|sample| u16::from_le_bytes([sample[0], sample[1]]))
        .collect();
    let mut longest = 0;
    for row in samples.chunks(width) {
        let mut run = 0;
        for pair in row.windows(2) {
            run = if pair[1] == pair[0].wrapping_add(step) {
                run + 1
            } else {
                0
            };
            longest = longest.max(run);
        }
    }
    longest
}

#[test]
fn the_test_backend_s_five_formats_in_the_raw_layout() {
    let test = "the_test_backend_s_five_formats_in_the_raw_layout";
    let child = real_child(test, &["test"], &[("test.conf", PACED)]);
    in_child!(
        the_test_backend_s_five_formats_in_the_raw_layout,
        child,
        || {
            let scan = |picture: &str, format: Format, paper: Paper| {
                let mut backend = SaneBackend::new().with_option("test-picture", picture);
                let mut device = open_tester(&mut backend);
                let mut asked = settings(Source::Flatbed, false, 10, format);
                asked.paper = paper;
                let (events, done) = paced(&mut *device, &asked, 1, None);
                assert_eq!(done, Ok(Done::OnePage), "{picture} in {format:?}");
                // As asked, the driver's paper aside: the largest area.
                let applied = applied(&events);
                assert_eq!(
                    (applied.settings.format, applied.settings.dpi),
                    (format, 10)
                );
                let auto = [Clamped::Paper(Paper::Auto)];
                assert_eq!(applied.clamped, auto[..usize::from(paper == Paper::Auto)]);
                let (got, width, height, data) = image(pages(&events)[0]);
                assert_eq!(got, format);
                (width, height, data.to_vec())
            };
            let small = Paper::Size(SMALL);
            // Gray at 1 bit, black and white: 0 black, and the bits past the last pixel 0. 31
            // pixels a row: four bytes, the last bit of each row padding.
            let (width, height, black) = scan("Solid black", Format::Bw1, small);
            assert_eq!((width, height), (31, 39));
            assert!(black.iter().all(|&byte| byte == 0));
            let (_, _, white) = scan("Solid white", Format::Bw1, small);
            assert_eq!(white, [0xff, 0xff, 0xff, 0xfe].repeat(39));
            let (width, height, gray) = scan("Color pattern", Format::Gray8, small);
            assert_eq!((width, height, gray.len()), (31, 39, 31 * 39));
            let (width, height, color) = scan("Color pattern", Format::Rgb8, small);
            assert_eq!((width, height, color.len()), (31, 39, 31 * 39 * 3));
            let (width, height, color16) = scan("Color pattern", Format::Rgb16, small);
            assert_eq!((width, height, color16.len()), (31, 39, 31 * 39 * 6));
            // At 16 bits, little-endian: the pattern's gray at its largest area runs along its rows
            // in steps of 256 read so, where read the other way the steps would be 1.
            let (width, height, gray16) = scan("Color pattern", Format::Gray16, Paper::Auto);
            assert_eq!((width, height, gray16.len()), (78, 78, 78 * 78 * 2));
            assert!(longest_run(&gray16, 78, 256) >= 40);
            assert_eq!(longest_run(&gray16, 78, 1), 0);
        }
    );
}

#[test]
fn the_test_backend_as_a_hand_scanner_gives_a_page_of_unknown_height() {
    let test = "the_test_backend_as_a_hand_scanner_gives_a_page_of_unknown_height";
    let conf = format!("{PACED}hand-scanner true\n");
    let child = real_child(test, &["test"], &[("test.conf", &conf)]);
    in_child!(
        the_test_backend_as_a_hand_scanner_gives_a_page_of_unknown_height,
        child,
        || {
            let mut device = open_tester(&mut SaneBackend::new());
            let asked = settings(Source::Flatbed, false, 10, Format::Gray8);
            let progress = Recorder::paced(None);
            let (events, done) = batch_with(&mut *device, &asked, 1, &progress, |_| true);
            assert_eq!(done, Ok(Done::OnePage));
            // Its fixed width, 11 cm.
            let (format, width, height, data) = image(pages(&events)[0]);
            assert_eq!((format, width), (Format::Gray8, 43));
            assert!(height > 0);
            // No total until the page ended, then its bytes.
            let counts = progress.counts();
            let (last, before) = counts.split_last().unwrap();
            assert_eq!(*last, Count::Total(Some(data.len() as u64)));
            assert!(
                before
                    .iter()
                    .all(|count| matches!(count, Count::Total(None) | Count::Done(_))),
                "{counts:?}"
            );
        }
    );
}

#[test]
fn the_test_backend_s_three_pass_color_in_each_order_is_its_one_pass_color() {
    let test = "the_test_backend_s_three_pass_color_in_each_order_is_its_one_pass_color";
    let conf = format!("{PACED}three-pass true\ntest-picture \"Color pattern\"\n");
    let child = real_child(test, &["test"], &[("test.conf", &conf)]);
    in_child!(
        the_test_backend_s_three_pass_color_in_each_order_is_its_one_pass_color,
        child,
        || {
            let mut asked = settings(Source::Flatbed, false, 10, Format::Rgb8);
            asked.paper = Paper::Size(SMALL);
            // A page, and whether it came as three frames of a third of its bytes each: read 64
            // bytes at a time, a frame of 1,209 ends its own read, which one of 3,627 never
            // gives.
            let page = |backend: SaneBackend, frame_bytes: Option<u64>| {
                let mut backend = backend;
                let mut device = open_tester(&mut backend);
                let progress = Recorder::paced(frame_bytes);
                let (events, done) = batch_with(&mut *device, &asked, 1, &progress, |_| true);
                assert_eq!(done, Ok(Done::OnePage));
                let (format, width, height, data) = image(pages(&events)[0]);
                assert_eq!((format, width, height), (Format::Rgb8, 31, 39));
                let third = data.len() as u64 / 3;
                let counts = progress.counts();
                let three = [third, 2 * third]
                    .iter()
                    .all(|&end| counts.contains(&Count::Done(end)));
                (data.to_vec(), three)
            };
            // Three-pass is on from the configuration, and set off by name: an option inactive
            // as the device opens, in gray, and set at the batch, once the mode is color.
            let (one, three) = page(SaneBackend::new().with_option("three-pass", "false"), None);
            assert!(!three);
            let frame_bytes = Some(one.len() as u64 / 3);
            for order in ["RGB", "RBG", "GBR", "GRB", "BRG", "BGR"] {
                let backend = SaneBackend::new().with_option("three-pass-order", order);
                assert_eq!(page(backend, frame_bytes), (one.clone(), true), "{order}");
            }
        }
    );
}

#[test]
fn the_statuses_the_test_backend_reads_end_a_batch_as_the_model_names_them() {
    let test = "the_statuses_the_test_backend_reads_end_a_batch_as_the_model_names_them";
    let child = real_child(test, &["test"], &[("test.conf", PACED)]);
    in_child!(
        the_statuses_the_test_backend_reads_end_a_batch_as_the_model_names_them,
        child,
        || {
            let asked = settings(Source::Feeder, false, 10, Format::Gray8);
            for (status, ended) in [
                ("SANE_STATUS_JAMMED", ScanError::Jammed),
                ("SANE_STATUS_COVER_OPEN", ScanError::CoverOpen),
                // At the first page's first read: the feeder empty as the batch began.
                ("SANE_STATUS_NO_DOCS", ScanError::FeederEmpty),
                ("SANE_STATUS_IO_ERROR", ScanError::Gone),
                ("SANE_STATUS_ACCESS_DENIED", ScanError::AccessDenied),
            ] {
                let mut backend = SaneBackend::new().with_option("read-return-value", status);
                let mut device = open_tester(&mut backend);
                let (events, done) = paced(&mut *device, &asked, 1, None);
                assert_eq!(done, Err(ended), "{status}");
                assert_eq!(order(&events), ["applied"], "{status}");
            }
        }
    );
}

#[test]
fn a_cancel_as_the_test_backend_s_sheet_3_is_taken_leaves_7_for_the_next_batch() {
    let test = "a_cancel_as_the_test_backend_s_sheet_3_is_taken_leaves_7_for_the_next_batch";
    let child = real_child(test, &["test"], &[("test.conf", PACED)]);
    in_child!(
        a_cancel_as_the_test_backend_s_sheet_3_is_taken_leaves_7_for_the_next_batch,
        child,
        || {
            let mut device = open_tester(&mut SaneBackend::new());
            let mut asked = settings(Source::Feeder, false, 10, Format::Gray8);
            asked.paper = Paper::Size(SMALL);
            let progress = Recorder::paced(None);
            let cancel = progress.tracker.clone();
            // The backend's next `send` waits on the events' thread, so the cancel comes before
            // it checks again.
            let (events, done) = batch_with(&mut *device, &asked, 1, &progress, |event| {
                if *event == Event::Sheet(3) {
                    cancel.cancel();
                }
                true
            });
            assert_eq!(done, Ok(Done::Cancelled));
            assert_eq!(sheets(&events), [1, 2, 3]);
            let (events, done) = paced(&mut *device, &asked, 4, None);
            assert_eq!(done, Ok(Done::FeederEmpty));
            assert_eq!(sheets(&events), (4..=10).collect::<Vec<_>>());
        }
    );
}

#[test]
fn the_test_backend_s_flatbed_gives_one_page_letter_cut_to_its_area() {
    let test = "the_test_backend_s_flatbed_gives_one_page_letter_cut_to_its_area";
    let child = real_child(test, &["test"], &[("test.conf", PACED)]);
    in_child!(
        the_test_backend_s_flatbed_gives_one_page_letter_cut_to_its_area,
        child,
        || {
            let mut device = open_tester(&mut SaneBackend::new());
            let mut asked = settings(Source::Flatbed, false, 10, Format::Gray8);
            asked.paper = Paper::Size(SizeMm::LETTER);
            let (events, done) = paced(&mut *device, &asked, 1, None);
            assert_eq!(done, Ok(Done::OnePage));
            let mut taken = asked.clone();
            taken.paper = Paper::Size(SizeMm {
                width: 200.0,
                height: 200.0,
            });
            assert_eq!(
                *applied(&events),
                Applied::new(taken, vec![Clamped::Paper(asked.paper)])
            );
            assert_eq!(order(&events), ["applied", "sheet 1", "page 1 front"]);
            // The area at the dpi: 200 mm at 10 dpi, 78 whole pixels each way.
            let (_, width, height, _) = image(pages(&events)[0]);
            assert_eq!((width, height), (78, 78));
        }
    );
}

/// A 5 by 3 PGM, every byte its own. At 8 bits a sample, as are all the files here: SANE 1.4.0's
/// `pnm` announces a 16-bit file's frames as 8-bit and sends twice the bytes, so its batch fails
/// as the driver's words, a fault of the backend's.
const PGM: &[u8] = b"P5\n5 3\n255\n\x00\x01\x02\x03\x04\x40\x41\x42\x43\x44\xf0\xf1\xf2\xf3\xff";

/// A 4 by 2 PPM.
const PPM: &[u8] = b"P6\n4 2\n255\n\
    \xff\x00\x00\x00\xff\x00\x00\x00\xff\x10\x20\x30\
    \x01\x02\x03\x80\x90\xa0\xfe\xfd\xfc\x00\x00\x00";

/// A PNM file's pixels, after its header's last line.
fn pixels_of(file: &[u8], header_lines: usize) -> &[u8] {
    let mut at = 0;
    for _ in 0..header_lines {
        at += file[at..].iter().position(|&byte| byte == b'\n').unwrap() + 1;
    }
    &file[at..]
}

#[test]
fn pnm_s_files_come_page_for_page() {
    let test = "pnm_s_files_come_page_for_page";
    let dir = test_dir(test);
    let (pbm, pgm, ppm) = (
        dir.join("page.pbm"),
        dir.join("page.pgm"),
        dir.join("page.ppm"),
    );
    let child = real_child(test, &["pnm"], &[]).prepare({
        let files = [(pbm.clone(), PBM), (pgm.clone(), PGM), (ppm.clone(), PPM)];
        move |_| {
            for (path, bytes) in files {
                fs::write(path, bytes).expect("couldn't write a PNM file");
            }
        }
    });
    in_child!(pnm_s_files_come_page_for_page, child, || {
        let scan = |backend: SaneBackend, format: Format| {
            let mut backend = backend;
            let mut device =
                installed(backend.open(&stored("sane:pnm:0", "Noname", "PNM file reader"), SOON))
                    .unwrap()
                    .device;
            let asked = settings(Source::Flatbed, false, 75, format);
            let (events, done) = batch_with(&mut *device, &asked, 1, &Recorder::new(), |_| true);
            assert_eq!(done, Ok(Done::OnePage));
            // No area options: the driver's paper.
            assert_eq!(*applied(&events), Applied::new(asked, Vec::new()));
            let (got, width, height, data) = image(pages(&events)[0]);
            assert_eq!(got, format);
            (width, height, data.to_vec())
        };
        let file = |path: &std::path::Path| {
            SaneBackend::new()
                .with_option("filename", path.to_str().expect("the test's path is text"))
        };
        // The PBM's 1 black, inverted, the three bits past each row's last pixel cleared.
        let (width, height, data) = scan(file(&pbm), Format::Bw1);
        assert_eq!((width, height), (13, 3));
        let expected: Vec<u8> = pixels_of(PBM, 2)
            .chunks(2)
            .flat_map(|row| [!row[0], !row[1] & 0xf8])
            .collect();
        assert_eq!(data, expected);
        assert_eq!(data, [0x55, 0x40, 0xaa, 0xb8, 0x00, 0x00]);
        let (width, height, data) = scan(file(&pgm), Format::Gray8);
        assert_eq!((width, height), (5, 3));
        assert_eq!(data, pixels_of(PGM, 3));
        let (width, height, data) = scan(file(&ppm), Format::Rgb8);
        assert_eq!((width, height), (4, 2));
        assert_eq!(data, pixels_of(PPM, 3));
        // Three frames, green, blue and red, set by name: the same page.
        let (_, _, three) = scan(file(&ppm).with_option("three-pass", "true"), Format::Rgb8);
        assert_eq!(three, pixels_of(PPM, 3));
    });
}
