//! SANE's backend in a worker, as an app runs it: the test worker's `sane` scenario, started by
//! the client.
//!
//! Through the fake libsane, given to the worker by its command's variables: a listing, an open as
//! stored and one renamed, an id no listing gives, caps, a duplex batch's pages byte for byte, a
//! jam keeping the pages before it, and a cancel through the client finishing its sheet with the
//! next batch giving the rest; the fake's log, which every worker that loaded it appends to,
//! checked after each.
//!
//! Through SANE's own `test` backend, its configuration the test's own, given as the command's
//! `SANE_CONFIG_DIR`, which a listing's worker reads and an open's reads after the client's
//! directory, never the system's: a listing and an open of its first device and its feeder of ten
//! sheets, the open's worker reading the command's configuration; a stored device of another
//! model and a stale index gone;
//! an alias opened by a worker whose directory names no backend, and one by a worker whose
//! directory names the alias's prefix; and a batch with SANE's debug output on, which reaches the
//! worker's standard error alone. Where there's no libsane, each says it skipped, or fails where
//! `SANE_DL_REQUIRE_LIBSANE=1`.
//!
//! And, run by name alone, a measurement: 300 dpi Letter pages from the fake through one worker.

mod support;

use std::{
    io::{self, Write},
    num::NonZeroU32,
    sync::mpsc,
    thread,
    time::Instant,
};

use scan_worker::scan_device::in_progress::Tracker;
use scan_worker::scan_device::{
    Applied, Caps, DeviceId, DeviceInfo, Done, Driver, Event, Format, Paper, Pixels, RawPage,
    Resolutions, ScanError, Settings, SizeMm, Source,
};
use scan_worker::{Client, Error, RemoteDevice};

use support::sane::{Fake, fake_pixels, fresh_dir, installed, real_command, sane_client};
use support::{AT_MOST, outline, pages, scan, settings};

/// A device listed, and the `DeviceInfo` an app stored for it: `id` and its name after `sane:`.
fn stored(id: &str, maker: &str, model: &str) -> DeviceInfo {
    DeviceInfo::new(
        DeviceId(id.to_owned()),
        id.strip_prefix("sane:").unwrap_or(id).to_owned(),
        maker.to_owned(),
        model.to_owned(),
        Driver::Sane,
    )
}

/// The scripts' first device.
const DEVICE: &str = "device fake:0 Fake Scanner T\n";

/// The scripts' first device, as a listing gives it.
fn fake_0() -> DeviceInfo {
    stored("sane:fake:0", "Fake", "Scanner")
}

/// A source option offering a flatbed and a feeder of one side, at the flatbed.
const FEEDER: &str = "option source STRING 32 soft-select+soft-detect\n\
                      strings Flatbed \"Automatic Document Feeder\"\n\
                      value Flatbed\n";

/// A source option offering a flatbed and a feeder of one side and of both.
const DUPLEX_FEEDER: &str = "option source STRING 16 soft-select+soft-detect\n\
                             strings Flatbed ADF \"ADF Duplex\"\n\
                             value Flatbed\n";

/// `device` opened through `client`.
fn open(client: &Client, device: &DeviceInfo) -> RemoteDevice {
    client.open(device, AT_MOST).expect("the device opened")
}

/// Each page's image, owned, to compare.
fn images(events: &[Event]) -> Vec<(Format, u32, u32, Vec<u8>)> {
    pages(events).into_iter().map(image).collect()
}

fn image(page: &RawPage) -> (Format, u32, u32, Vec<u8>) {
    let Pixels::Raw(image) = &page.pixels else {
        panic!("not a raw page: {page:?}");
    };
    (
        image.format(),
        image.width(),
        image.height(),
        image.data().to_vec(),
    )
}

/// The fake's default frame's page `page` (from 1), a back if `back`: 16 by 4 gray pixels.
fn gray_page(page: usize, back: bool) -> (Format, u32, u32, Vec<u8>) {
    (Format::Gray8, 16, 4, fake_pixels(page, back, 0, 16, 4))
}

#[test]
#[cfg_attr(not(unix), ignore = "loads the fake by SANE's name: none on Windows")]
fn a_listing_through_a_worker_gives_sane_s_devices() {
    let script = format!("{DEVICE}device \"fake:caf\\xe9\" \"Caf\\xe9 Scan\" Model T\n");
    let fake = Fake::new("a_listing_through_a_worker_gives_sane_s_devices", &script);
    let listed = fake
        .client(&[])
        .list(Driver::Sane, AT_MOST)
        .expect("a listing");
    // A Latin-1 name's id, each byte the character of its number, and its vendor read as Latin-1.
    assert_eq!(
        listed,
        [
            fake_0(),
            stored("sane:fake:caf\u{e9}", "Caf\u{e9} Scan", "Model")
        ]
    );
    let log = fake.log();
    log.assert_no_violations();
    assert_eq!(
        log.calls_but_options(),
        ["sane_init", "sane_get_devices", "sane_exit"]
    );
}

#[test]
#[cfg_attr(not(unix), ignore = "loads the fake by SANE's name: none on Windows")]
fn a_stored_device_opens_in_a_worker_as_listed_and_a_renamed_one_says_so() {
    let script = format!(
        "{DEVICE}device \"fake:caf\\xe9\" \"Caf\\xe9 Scan\" Model T\n\
         device fake:libusb:001:005 Fake Scanner T\n"
    );
    let fake = Fake::new(
        "a_stored_device_opens_in_a_worker_as_listed_and_a_renamed_one_says_so",
        &script,
    );
    let client = fake.client(&[]);
    // As stored: its info the listing's.
    for listed in [
        fake_0(),
        stored("sane:fake:caf\u{e9}", "Caf\u{e9} Scan", "Model"),
    ] {
        let device = open(&client, &listed);
        assert_eq!(*device.info(), listed);
        assert_eq!(device.renamed_from(), None);
        device.close().expect("closed");
    }
    // A USB connection's name gone, its maker's and model's one other device listed: opened, and
    // said renamed across the pipe.
    let gone = stored("sane:fake:libusb:001:004", "Fake", "Scanner");
    let device = open(&client, &gone);
    assert_eq!(
        *device.info(),
        stored("sane:fake:libusb:001:005", "Fake", "Scanner")
    );
    assert_eq!(device.renamed_from(), Some(&gone.id));
    device.close().expect("closed");
    let log = fake.log();
    log.assert_no_violations();
    // Each name SANE was asked to open, the Latin-1 one as its byte: the fake opens a name only as
    // it lists it, byte for byte.
    assert_eq!(
        log.calls_of("sane_open"),
        [
            "name=\"fake:0\"",
            "name=\"fake:caf\u{fffd}\"",
            "name=\"fake:libusb:001:004\"",
            "name=\"fake:libusb:001:005\""
        ]
    );
    assert_eq!(
        log.calls_but_options()
            .into_iter()
            .filter(|call| *call == "sane_get_devices")
            .count(),
        3,
        "each open lists first"
    );
}

#[test]
#[cfg_attr(not(unix), ignore = "loads the fake by SANE's name: none on Windows")]
fn an_id_no_listing_gives_is_bad_id_through_a_worker_with_nothing_opened() {
    let fake = Fake::new(
        "an_id_no_listing_gives_is_bad_id_through_a_worker_with_nothing_opened",
        DEVICE,
    );
    let client = fake.client(&[]);
    // SANE's testing mode, which replays a file the name gives; a name SANE would hand to a
    // backend the id never named; and another driver's id, given to the SANE worker.
    for id in ["sane:fakeusb:dump.xml", "sane::0", "fake:desk"] {
        let opened = client.open(&stored(id, "Fake", "Scanner"), AT_MOST);
        assert!(
            matches!(opened, Err(Error::Scan(ScanError::BadId))),
            "{id}: {opened:?}"
        );
    }
    // Refused before SANE loads.
    assert!(fake.log().calls().is_empty(), "{:?}", fake.log());
}

/// `source`'s caps: a range of resolutions from `min` to `max`, black and white, gray and color at
/// 8 bits, duplex or not, and its largest area `width` by `height`.
fn fujitsu_s(source: Source, duplex: bool, width: f32, height: f32) -> String {
    format!(
        "{source:?} duplex={duplex} {:?} {:?} {width:.2}x{height:.2} auto=false encodings=[]",
        Resolutions::Range {
            min: 50,
            max: 600,
            step: NonZeroU32::MIN,
        },
        [Format::Bw1, Format::Gray8, Format::Rgb8]
    )
}

/// `caps` in short, its areas to the hundredth of a millimetre, as they cross the pipe.
fn caps_in_short(caps: &Caps) -> Vec<String> {
    caps.sources
        .iter()
        .map(|source| {
            let area = source.max_area.expect("a largest area");
            format!(
                "{:?} duplex={} {:?} {:?} {:.2}x{:.2} auto={} encodings={:?}",
                source.source,
                source.duplex,
                source.resolutions,
                source.formats,
                area.width,
                area.height,
                source.paper_auto,
                source.encodings
            )
        })
        .collect()
}

#[test]
#[cfg_attr(not(unix), ignore = "loads the fake by SANE's name: none on Windows")]
fn caps_through_a_worker_are_the_backend_s() {
    let script = format!("{DEVICE}profile fujitsu\n");
    let fake = Fake::new("caps_through_a_worker_are_the_backend_s", &script);
    let mut device = open(&fake.client(&[]), &fake_0());
    let caps = device.caps().expect("caps");
    assert!(!caps.settings_window);
    assert_eq!(
        caps_in_short(&caps),
        [
            fujitsu_s(Source::Flatbed, false, 215.9, 355.6),
            fujitsu_s(Source::Feeder, true, 221.0, 355.6)
        ]
    );
    device.close().expect("closed");
    let log = fake.log();
    log.assert_no_violations();
    // Found at `ADF Front`, read first; the flatbed read, then set back.
    assert_eq!(log.sets(), ["\"Flatbed\"", "\"ADF Front\""]);
}

#[test]
#[cfg_attr(not(unix), ignore = "loads the fake by SANE's name: none on Windows")]
fn a_duplex_batch_through_a_worker_gives_the_fake_s_pages_byte_for_byte() {
    let script = format!("{DEVICE}profile brother\nduplex\nsheets 3\n");
    let fake = Fake::new(
        "a_duplex_batch_through_a_worker_gives_the_fake_s_pages_byte_for_byte",
        &script,
    );
    let mut device = open(&fake.client(&[]), &fake_0());
    let mut asked = settings(Source::Feeder, 300, Format::Gray8, true);
    asked.paper = Paper::Size(SizeMm::LETTER);
    let (events, ended) = scan(&mut device, &asked, 5);
    assert_eq!(ended.expect("a batch"), Done::FeederEmpty);
    assert_eq!(
        events.first(),
        Some(&Event::Applied(Applied::new(asked.clone(), Vec::new())))
    );
    assert_eq!(
        outline(&events),
        [
            "applied",
            "sheet 5",
            "page 5 front",
            "page 5 back",
            "sheet 6",
            "page 6 front",
            "page 6 back",
            "sheet 7",
            "page 7 front",
            "page 7 back"
        ]
    );
    // Fronts marked F and backs B, the fake's pages 1 to 6, as its formula gives them.
    assert_eq!(
        images(&events),
        (1..=6)
            .map(|page| gray_page(page, page % 2 == 0))
            .collect::<Vec<_>>()
    );
    for page in pages(&events) {
        assert_eq!(page.dpi, (300.0, 300.0));
    }
    device.close().expect("closed");
    let log = fake.log();
    log.assert_no_violations();
    assert!(
        log.sets()
            .contains(&"\"Automatic Document Feeder(left aligned,Duplex)\""),
        "{:?}",
        log.sets()
    );
}

#[test]
#[cfg_attr(not(unix), ignore = "loads the fake by SANE's name: none on Windows")]
fn a_jam_through_a_worker_keeps_the_pages_before_it_and_the_worker_goes_on() {
    let script = format!("{DEVICE}{FEEDER}sheets 5\nat 4 read JAMMED 8\n");
    let fake = Fake::new(
        "a_jam_through_a_worker_keeps_the_pages_before_it_and_the_worker_goes_on",
        &script,
    );
    let mut device = open(&fake.client(&[]), &fake_0());
    let asked = settings(Source::Feeder, 300, Format::Gray8, false);
    let (events, ended) = scan(&mut device, &asked, 1);
    assert!(
        matches!(ended, Err(Error::Scan(ScanError::Jammed))),
        "{ended:?}"
    );
    assert_eq!(
        outline(&events),
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
        (1..=3)
            .map(|page| gray_page(page, false))
            .collect::<Vec<_>>()
    );
    // Continued from sheet 4: the jammed page was taken by its start, so the fake's page 5.
    let (events, ended) = scan(&mut device, &asked, 4);
    assert_eq!(ended.expect("the next batch"), Done::FeederEmpty);
    assert_eq!(outline(&events), ["applied", "sheet 4", "page 4 front"]);
    assert_eq!(images(&events), [gray_page(5, false)]);
    device.close().expect("closed");
    fake.log().assert_no_violations();
}

/// A megabyte's frame: a page longer than any pipe holds, so a worker is held a page or two ahead
/// of its client.
const MEGABYTE_FRAME: &str = "frame GRAY 1 1024 1024 1024 8\n";

/// The fake's megabyte page `page` (from 1), a back if `back`.
fn megabyte_page(page: usize, back: bool) -> (Format, u32, u32, Vec<u8>) {
    (
        Format::Gray8,
        1024,
        1024,
        fake_pixels(page, back, 0, 1024, 1024),
    )
}

#[test]
#[cfg_attr(not(unix), ignore = "loads the fake by SANE's name: none on Windows")]
fn a_cancel_through_the_client_finishes_its_sheet_and_the_next_batch_gives_the_rest() {
    let script = format!("{DEVICE}{DUPLEX_FEEDER}duplex\nsheets 6\n{MEGABYTE_FRAME}");
    let fake = Fake::new(
        "a_cancel_through_the_client_finishes_its_sheet_and_the_next_batch_gives_the_rest",
        &script,
    );
    let mut device = open(&fake.client(&[]), &fake_0());
    let asked = settings(Source::Feeder, 300, Format::Gray8, true);
    // Cancelled as the first page is taken, on a rendezvous channel: where it lands is the pipe's
    // timing, so the sheets before it aren't counted here.
    let tracker = Tracker::new();
    let (events, received) = mpsc::sync_channel(0);
    let cancels = tracker.clone();
    let taker = thread::spawn(move || {
        let mut taken = Vec::new();
        for event in received {
            if matches!(event, Event::Page(_)) {
                cancels.cancel();
            }
            taken.push(event);
        }
        taken
    });
    let ended = device.scan(&asked, 1, &events, &tracker);
    drop(events);
    let first = taker.join().expect("the events' thread");
    assert_eq!(ended.expect("a batch"), Done::Cancelled);
    // Whole sheets, each front then back, from sheet 1, and not the feeder's last.
    let sheets = pages(&first).last().expect("a page").sheet;
    assert!((1..6).contains(&sheets), "{:?}", outline(&first));
    let whole = |from: u32, to: u32| {
        let mut events = vec!["applied".to_owned()];
        for sheet in from..=to {
            events.extend([
                format!("sheet {sheet}"),
                format!("page {sheet} front"),
                format!("page {sheet} back"),
            ]);
        }
        events
    };
    assert_eq!(outline(&first), whole(1, sheets));
    // The next batch, in the same worker, gives the rest: every sheet once, none lost.
    let (rest, ended) = scan(&mut device, &asked, sheets + 1);
    assert_eq!(ended.expect("the next batch"), Done::FeederEmpty);
    assert_eq!(outline(&rest), whole(sheets + 1, 6));
    let both: Vec<_> = images(&first).into_iter().chain(images(&rest)).collect();
    assert_eq!(both.len(), 12);
    for (at, page) in both.iter().enumerate() {
        assert!(
            *page == megabyte_page(at + 1, at % 2 == 1),
            "the {}th page isn't the fake's page {}",
            at + 1,
            at + 1
        );
    }
    device.close().expect("closed");
    let log = fake.log();
    log.assert_no_violations();
    // One start a page, then the empty feeder's.
    assert_eq!(log.calls_of("sane_start").len(), 13);
}

/// The `test` backend's configuration: two devices, its reads limited to 64 bytes, as the
/// scenario's pacing needs.
const PACED: &str = "number_of_devices 2\nread-limit true\nread-limit-size 64\n";

/// A `test.conf` as [`PACED`], with a grid for its first picture, which neither the backend's own
/// defaults nor the system's configuration give, so a page shows it was read.
const PACED_GRID: &str =
    "number_of_devices 2\nread-limit true\nread-limit-size 64\ntest-picture Grid\n";

/// A `test` backend's device, as its listing gives it.
fn tester(id: &str) -> DeviceInfo {
    stored(id, "Noname", "frontend-tester")
}

/// A client whose SANE worker is the paced `sane` scenario for the `test` backend, with `extra`
/// variables on its command: its configuration names `test`, with `files` beside it (a
/// `test.conf`, `dll.aliases`), which a listing reads, and an open after the client's directory.
fn test_backend(test: &str, files: &[(&str, &str)], extra: &[(&str, &str)]) -> Client {
    let dir = fresh_dir(test);
    let command = real_command(&dir, &["test"], files, &["--paced"]);
    let command = extra
        .iter()
        .fold(command, |command, &(key, value)| command.env(key, value));
    sane_client(command, &dir)
}

/// Gray at 10 dpi from the feeder, the paper the driver's.
fn ten_dpi() -> Settings {
    settings(Source::Feeder, 10, Format::Gray8, false)
}

/// A batch of the `test` backend's whole feeder from sheet 1: ten pages of its 200 mm square at
/// 10 dpi, 78 by 78; and the pages, to read.
fn ten_sheets(device: &mut RemoteDevice) -> Vec<Event> {
    let (events, ended) = scan(device, &ten_dpi(), 1);
    assert_eq!(ended.expect("a batch"), Done::FeederEmpty);
    let sheets: Vec<u32> = pages(&events).iter().map(|page| page.sheet).collect();
    assert_eq!(sheets, (1..=10).collect::<Vec<_>>());
    for page in pages(&events) {
        let (format, width, height, _) = image(page);
        assert_eq!((format, width, height), (Format::Gray8, 78, 78));
        assert_eq!(page.dpi, (10.0, 10.0));
    }
    events
}

#[test]
fn the_test_backend_in_a_worker_lists_opens_and_gives_its_feeder_s_ten_sheets() {
    let test = "the_test_backend_in_a_worker_lists_opens_and_gives_its_feeder_s_ten_sheets";
    let client = test_backend(test, &[("test.conf", PACED_GRID)], &[]);
    let Some(listed) = installed(test, client.list(Driver::Sane, AT_MOST)) else {
        return;
    };
    let listed = listed.expect("a listing");
    assert_eq!(listed, [tester("sane:test:0"), tester("sane:test:1")]);
    let mut device = open(&client, &listed[0]);
    assert_eq!(*device.info(), listed[0]);
    assert_eq!(device.renamed_from(), None);
    let events = ten_sheets(&mut device);
    // The grid, the command's configuration's: so the open's worker read it after the client's
    // directory, and not the system's.
    for page in pages(&events) {
        let (_, _, _, data) = image(page);
        assert!(data.contains(&0) && data.contains(&255), "not a grid");
    }
    device.close().expect("closed");
}

#[test]
fn a_stored_test_device_of_another_model_is_gone_through_a_worker() {
    let test = "a_stored_test_device_of_another_model_is_gone_through_a_worker";
    let client = test_backend(test, &[("test.conf", PACED)], &[]);
    let another = stored("sane:test:0", "Noname", "another-tester");
    let Some(opened) = installed(test, client.open(&another, AT_MOST)) else {
        return;
    };
    assert!(
        matches!(opened, Err(Error::Scan(ScanError::Gone))),
        "{opened:?}"
    );
}

#[test]
fn a_stale_index_of_the_test_backend_is_gone_through_a_worker() {
    let test = "a_stale_index_of_the_test_backend_is_gone_through_a_worker";
    let client = test_backend(test, &[("test.conf", PACED)], &[]);
    let Some(opened) = installed(test, client.open(&tester("sane:test:9"), AT_MOST)) else {
        return;
    };
    assert!(
        matches!(opened, Err(Error::Scan(ScanError::Gone))),
        "{opened:?}"
    );
}

/// The aliases: one whose name names no backend, one whose prefix names none.
const ALIASES: &str = "alias office:scan test:0\nalias \"Desk scanner\" test:1\n";

/// An alias opened through `client`, a worker whose directory the client writes from the id: its
/// info the stored one, and its feeder's ten sheets.
fn alias_opens(test: &str, alias: &str) {
    let client = test_backend(test, &[("dll.aliases", ALIASES), ("test.conf", PACED)], &[]);
    let stored = tester(alias);
    let Some(opened) = installed(test, client.open(&stored, AT_MOST)) else {
        return;
    };
    let mut device = opened.expect("the alias opened");
    assert_eq!(*device.info(), stored);
    assert_eq!(device.renamed_from(), None);
    ten_sheets(&mut device);
    device.close().expect("closed");
}

#[test]
fn an_alias_naming_no_backend_opens_in_a_worker_whose_directory_names_none() {
    alias_opens(
        "an_alias_naming_no_backend_opens_in_a_worker_whose_directory_names_none",
        "sane:Desk scanner",
    );
}

#[test]
fn an_alias_whose_prefix_names_no_backend_opens_in_a_worker_whose_directory_names_it() {
    alias_opens(
        "an_alias_whose_prefix_names_no_backend_opens_in_a_worker_whose_directory_names_it",
        "sane:office:scan",
    );
}

#[test]
fn sane_s_debug_output_reaches_no_further_than_the_worker_s_standard_error() {
    let test = "sane_s_debug_output_reaches_no_further_than_the_worker_s_standard_error";
    // SANE's `dll` and the `test` backend saying everything they do: on standard error, so the
    // protocol on standard output is whole, and the batch passes.
    let client = test_backend(
        test,
        &[("test.conf", PACED)],
        &[("SANE_DEBUG_DLL", "255"), ("SANE_DEBUG_TEST", "255")],
    );
    let Some(listed) = installed(test, client.list(Driver::Sane, AT_MOST)) else {
        return;
    };
    let listed = listed.expect("a listing");
    let mut device = open(&client, &listed[0]);
    ten_sheets(&mut device);
    device.close().expect("closed");
}

/// The sheets of each measured batch, both sides read.
const SHEETS: usize = 24;

/// The pages the rate leaves out: the batch's start, and the first sheets'.
const LEFT_OUT: usize = 8;

#[test]
#[ignore = "a measurement, run by name, on Unix: 300 dpi Letter from the fake libsane through a worker"]
fn letter_at_300_dpi_from_the_fake_through_a_worker_measured() {
    let build = if cfg!(debug_assertions) {
        "debug"
    } else {
        "release"
    };
    // Letter at 300 dpi is 2550 by 3300 pixels: color at 8 bits, and gray at 1 bit (319 bytes a
    // row, its last byte six pixels then two bits of padding).
    for (format, frame, page_bytes) in [
        (
            Format::Rgb8,
            "frame RGB 1 7650 2550 3300 8\n",
            2550 * 3 * 3300,
        ),
        (Format::Bw1, "frame GRAY 1 319 2550 3300 1\n", 319 * 3300),
    ] {
        let script = format!("{DEVICE}{DUPLEX_FEEDER}duplex\nsheets {SHEETS}\n{frame}");
        let fake = Fake::new(
            &format!("letter_at_300_dpi_from_the_fake_through_a_worker_measured_{format:?}"),
            &script,
        );
        let mut device = open(&fake.client(&[]), &fake_0());
        let (events, received) = mpsc::sync_channel(2);
        let taken = thread::spawn(move || {
            received
                .iter()
                .filter_map(|event| match event {
                    Event::Page(page) => match &page.pixels {
                        Pixels::Raw(image) => Some((Instant::now(), image.data().len())),
                        _ => panic!("a raw page"),
                    },
                    _ => None,
                })
                .collect::<Vec<_>>()
        });
        let started = Instant::now();
        let ended = device.scan(
            &settings(Source::Feeder, 300, format, true),
            1,
            &events,
            &Tracker::new(),
        );
        drop(events);
        let taken = taken.join().expect("the caller's thread");
        assert_eq!(ended.expect("a batch"), Done::FeederEmpty);
        assert_eq!(taken.len(), 2 * SHEETS);
        assert!(taken.iter().all(|(_, bytes)| *bytes == page_bytes));
        let (start, _) = taken[LEFT_OUT - 1];
        let (end, _) = taken[taken.len() - 1];
        let counted = taken.len() - LEFT_OUT;
        let elapsed = (end - start).as_secs_f64();
        let _ = writeln!(
            io::stderr(),
            "sane throughput: {format:?}, 300 dpi Letter, duplex, {build} build: {:.1} pages a \
             second, {:.1} MB a second, over pages {} to {} ({elapsed:.2} s; the whole batch \
             {:.2} s)",
            counted as f64 / elapsed,
            (counted * page_bytes) as f64 / 1e6 / elapsed,
            LEFT_OUT + 1,
            taken.len(),
            started.elapsed().as_secs_f64(),
        );
        device.close().expect("closed");
        fake.log().assert_no_violations();
    }
}
