//! The fake backend's batches that go right: the settings clamped and said, sheets from the
//! feeder or the glass, their pages drawn or given, raw or encoded, the progress, and the end.

use std::{
    num::NonZeroU32,
    sync::{Mutex, mpsc},
    thread,
    time::Duration,
};

use scan_device::{
    Applied, Backend, Caps, Clamped, Device, Done, Encoded, EncodedKind, Encoding, Event, Format,
    LayoutError, Paper, Pixels, RawImage, RawPage, Resolutions, ScanError, Settings, Side,
    SideKnown, SizeMm, Source, SourceCaps, Status,
    fake::{FakeBackend, FakeDevice, Sheet, pattern},
    in_progress::{Level, NoProgress, Progress, Tracker, Unit},
};

/// A deadline no listing here comes near.
const SOON: Duration = Duration::from_secs(5);

/// A device with the default caps.
fn scanner() -> FakeDevice {
    FakeDevice::new("libusb:001:004", "Maker", "Model")
}

/// `device` opened through a backend of its own, sharing its paper.
fn open(device: &FakeDevice) -> Box<dyn Device> {
    let mut backend = FakeBackend::new().with(device.clone());
    backend
        .open(&device.info(), SOON)
        .expect("the device opens")
        .device
}

/// A batch run to its end on a thread of its own, every event kept, with `progress`.
fn run_with(
    device: &mut Box<dyn Device>,
    settings: &Settings,
    first_sheet: u32,
    progress: &dyn Progress,
) -> (Result<Done, ScanError>, Vec<Event>) {
    let (events, received) = mpsc::sync_channel(1);
    thread::scope(|scope| {
        let batch = scope.spawn(move || device.scan(settings, first_sheet, &events, progress));
        let got: Vec<Event> = received.iter().collect();
        (batch.join().expect("the batch's thread"), got)
    })
}

/// A batch run to its end, every event kept, with a tracker of its own.
fn run(
    device: &mut Box<dyn Device>,
    settings: &Settings,
    first_sheet: u32,
) -> (Result<Done, ScanError>, Vec<Event>) {
    run_with(device, settings, first_sheet, &Tracker::new())
}

/// Each event in short: `applied`, `warming up`, `sheet 3`, `page 3 front`.
fn outline(events: &[Event]) -> Vec<String> {
    events
        .iter()
        .map(|event| match event {
            Event::Applied(_) => "applied".to_owned(),
            Event::Status(Status::WarmingUp) => "warming up".to_owned(),
            Event::Sheet(n) => format!("sheet {n}"),
            Event::Page(page) => {
                let side = match page.side {
                    Side::Front => "front",
                    Side::Back => "back",
                };
                format!("page {} {side}", page.sheet)
            }
            other => format!("{other:?}"),
        })
        .collect()
}

/// The batch's `Applied`, its first event.
fn applied(events: &[Event]) -> &Applied {
    match events.first() {
        Some(Event::Applied(applied)) => applied,
        other => panic!("{other:?}"),
    }
}

/// The batch's pages.
fn pages(events: &[Event]) -> Vec<&RawPage> {
    events
        .iter()
        .filter_map(|event| match event {
            Event::Page(page) => Some(page),
            _ => None,
        })
        .collect()
}

/// A page's raw image.
fn raw(page: &RawPage) -> &RawImage {
    match &page.pixels {
        Pixels::Raw(image) => image,
        other => panic!("{other:?}"),
    }
}

/// `n` plain sheets.
fn sheets(n: usize) -> Vec<Sheet> {
    vec![Sheet::new(); n]
}

fn settings(source: Source, dpi: u32, format: Format) -> Settings {
    Settings::new(source, dpi, format)
}

/// Settings on A4, which both the default sources take as it is.
fn on_a4(source: Source, dpi: u32, format: Format) -> Settings {
    let mut settings = Settings::new(source, dpi, format);
    settings.paper = Paper::Size(SizeMm::A4);
    settings
}

#[test]
fn a_feeder_s_sheets_come_one_by_one_until_it_s_empty() {
    let scanner = scanner();
    scanner.load(sheets(3));
    let mut device = open(&scanner);
    let (done, events) = run(&mut device, &settings(Source::Feeder, 75, Format::Gray8), 1);
    assert_eq!(done, Ok(Done::FeederEmpty));
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
    assert_eq!((scanner.fed(), scanner.in_feeder()), (3, 0));
    for page in pages(&events) {
        assert_eq!(page.dpi, (75.0, 75.0));
        assert_eq!(page.side_known, SideKnown::Inferred);
        assert_eq!(raw(page).format(), Format::Gray8);
    }
}

#[test]
fn in_duplex_each_sheet_gives_its_front_then_its_back() {
    for (scanner, known) in [
        (scanner(), SideKnown::Inferred),
        (scanner().reports_sides(), SideKnown::Reported),
    ] {
        scanner.load(sheets(2));
        let mut device = open(&scanner);
        let mut asked = settings(Source::Feeder, 50, Format::Bw1);
        asked.duplex = true;
        let (done, events) = run(&mut device, &asked, 1);
        assert_eq!(done, Ok(Done::FeederEmpty));
        assert!(applied(&events).clamped.is_empty());
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
        assert!(pages(&events).iter().all(|page| page.side_known == known));
        assert_eq!(scanner.fed(), 2);
    }
}

#[test]
fn scan_more_carries_the_sheet_count_on() {
    let scanner = scanner();
    scanner.load(sheets(2));
    let mut device = open(&scanner);
    let asked = settings(Source::Feeder, 50, Format::Bw1);
    let (done, events) = run(&mut device, &asked, 1);
    assert_eq!(done, Ok(Done::FeederEmpty));
    assert_eq!(
        outline(&events)[1..],
        ["sheet 1", "page 1 front", "sheet 2", "page 2 front"]
    );
    // More paper, and the next batch numbered on from where the last ended.
    scanner.load(sheets(2));
    let (done, events) = run(&mut device, &asked, 3);
    assert_eq!(done, Ok(Done::FeederEmpty));
    assert_eq!(
        outline(&events)[1..],
        ["sheet 3", "page 3 front", "sheet 4", "page 4 front"]
    );
    assert_eq!((scanner.fed(), scanner.in_feeder()), (4, 0));
}

#[test]
fn an_empty_feeder_is_an_error_before_any_page_and_comes_after_the_settings() {
    for (scanner, before) in [
        (scanner(), vec!["applied"]),
        (scanner().warms_up(), vec!["applied", "warming up"]),
    ] {
        let mut device = open(&scanner);
        let (done, events) = run(&mut device, &settings(Source::Feeder, 300, Format::Rgb8), 1);
        assert_eq!(done, Err(ScanError::FeederEmpty));
        assert_eq!(outline(&events), before);
        assert_eq!(scanner.fed(), 0);
    }
}

#[test]
fn warming_up_comes_after_the_settings_and_before_the_first_sheet() {
    let scanner = scanner().warms_up();
    scanner.load(sheets(1));
    let mut device = open(&scanner);
    let (done, events) = run(&mut device, &settings(Source::Feeder, 50, Format::Bw1), 1);
    assert_eq!(done, Ok(Done::FeederEmpty));
    assert_eq!(
        outline(&events),
        ["applied", "warming up", "sheet 1", "page 1 front"]
    );
    // At each batch.
    scanner.load(sheets(1));
    let (_, events) = run(&mut device, &settings(Source::Flatbed, 75, Format::Bw1), 2);
    assert_eq!(
        outline(&events),
        ["applied", "warming up", "sheet 2", "page 2 front"]
    );
}

#[test]
fn a_flatbed_scans_its_glass_once_and_takes_nothing_from_the_feeder() {
    let scanner = scanner();
    scanner.load(sheets(2));
    let mut device = open(&scanner);
    let (done, events) = run(
        &mut device,
        &settings(Source::Flatbed, 150, Format::Gray8),
        5,
    );
    assert_eq!(done, Ok(Done::OnePage));
    assert_eq!(outline(&events), ["applied", "sheet 5", "page 5 front"]);
    assert_eq!((scanner.fed(), scanner.in_feeder()), (0, 2));
    // The glass's largest paper, 215.9 by 297 mm, at 150 dpi: its own size is no size the
    // flatbed finds.
    let image = raw(pages(&events)[0]);
    assert_eq!((image.width(), image.height()), (1275, 1754));

    // The sheet on the glass stays there, and is scanned again.
    let given = RawImage::new(Format::Gray8, 2, 1, vec![7, 9]).unwrap();
    scanner.glass(Sheet::new().side(Side::Front, Pixels::Raw(given.clone())));
    for first in [1, 2] {
        let (done, events) = run(
            &mut device,
            &settings(Source::Flatbed, 150, Format::Gray8),
            first,
        );
        assert_eq!(done, Ok(Done::OnePage));
        assert_eq!(pages(&events)[0].pixels, Pixels::Raw(given.clone()));
        assert_eq!(pages(&events)[0].sheet, first);
    }
    assert_eq!(scanner.fed(), 0);
}

#[test]
fn settings_the_source_can_take_are_applied_as_asked() {
    let scanner = scanner();
    scanner.load(sheets(1));
    let mut device = open(&scanner);
    let mut asked = settings(Source::Feeder, 333, Format::Rgb16);
    asked.duplex = true;
    asked.paper = Paper::Size(SizeMm::A4);
    let (_, events) = run(&mut device, &asked, 1);
    let applied = applied(&events);
    assert_eq!(applied.settings, asked);
    assert!(applied.clamped.is_empty());
}

/// One source of `caps`' kind, with every format, at the given resolutions.
fn source(source: Source, resolutions: Resolutions) -> SourceCaps {
    SourceCaps::new(
        source,
        resolutions,
        vec![
            Format::Bw1,
            Format::Gray8,
            Format::Gray16,
            Format::Rgb8,
            Format::Rgb16,
        ],
    )
}

/// The settings `device` applies to `asked`: the batch's `Applied`.
fn clamp(device: &mut Box<dyn Device>, asked: &Settings) -> Applied {
    let (_, events) = run(device, asked, 1);
    applied(&events).clone()
}

#[test]
fn a_source_the_caps_lack_becomes_the_first_they_list() {
    let caps = Caps::new(vec![source(Source::Flatbed, Resolutions::List(vec![300]))]);
    let scanner = FakeDevice::new("a", "M", "N").caps(caps);
    scanner.load(sheets(1));
    let mut device = open(&scanner);
    let (done, events) = run(&mut device, &settings(Source::Feeder, 300, Format::Bw1), 1);
    assert_eq!(done, Ok(Done::OnePage));
    let applied = applied(&events);
    assert_eq!(applied.settings.source, Source::Flatbed);
    assert_eq!(applied.clamped, [Clamped::Source(Source::Feeder)]);
    assert_eq!(scanner.in_feeder(), 1);
}

#[test]
fn duplex_on_a_source_without_it_is_turned_off() {
    let mut device = open(&scanner());
    let mut asked = on_a4(Source::Flatbed, 300, Format::Bw1);
    asked.duplex = true;
    let (_, events) = run(&mut device, &asked, 1);
    assert!(!applied(&events).settings.duplex);
    assert_eq!(applied(&events).clamped, [Clamped::Duplex(true)]);
    assert_eq!(outline(&events), ["applied", "sheet 1", "page 1 front"]);
}

#[test]
fn a_resolution_becomes_the_nearest_the_source_allows_the_higher_of_two() {
    let mut device = open(&scanner());
    // The flatbed's 75, 150, 300 and 600; the feeder's 50 to 600 in steps of 1.
    for (source, asked, taken) in [
        (Source::Flatbed, 200, 150),
        (Source::Flatbed, 225, 300),
        (Source::Flatbed, 226, 300),
        (Source::Flatbed, 0, 75),
        (Source::Flatbed, 1, 75),
        (Source::Flatbed, 601, 600),
        (Source::Flatbed, u32::MAX, 600),
        (Source::Feeder, 0, 50),
        (Source::Feeder, 49, 50),
        (Source::Feeder, 601, 600),
        (Source::Feeder, u32::MAX, 600),
    ] {
        let applied = clamp(&mut device, &on_a4(source, asked, Format::Bw1));
        assert_eq!(applied.settings.dpi, taken, "{source:?} {asked}");
        assert_eq!(applied.clamped, [Clamped::Dpi(asked)], "{source:?} {asked}");
    }
    for (source, asked) in [
        (Source::Flatbed, 150),
        (Source::Feeder, 50),
        (Source::Feeder, 333),
    ] {
        let applied = clamp(&mut device, &on_a4(source, asked, Format::Bw1));
        assert_eq!(applied.settings.dpi, asked);
        assert!(applied.clamped.is_empty());
    }
}

#[test]
fn a_range_whose_max_is_off_its_steps_is_never_passed() {
    // 50, 80 and 110: 100 isn't on the steps, and 110 is past it.
    let range = Resolutions::Range {
        min: 50,
        max: 100,
        step: NonZeroU32::new(30).unwrap(),
    };
    let scanner =
        FakeDevice::new("a", "M", "N").caps(Caps::new(vec![source(Source::Feeder, range)]));
    let mut device = open(&scanner);
    for (asked, taken) in [
        (100, 80),
        (96, 80),
        (u32::MAX, 80),
        (65, 80),
        (64, 50),
        (0, 50),
    ] {
        scanner.load(sheets(1));
        let (done, events) = run(
            &mut device,
            &settings(Source::Feeder, asked, Format::Bw1),
            1,
        );
        assert_eq!(done, Ok(Done::FeederEmpty));
        let applied = applied(&events);
        assert_eq!(applied.settings.dpi, taken, "{asked}");
        assert_eq!(applied.clamped, [Clamped::Dpi(asked)]);
        let dpi = taken as f32;
        assert_eq!(pages(&events)[0].dpi, (dpi, dpi));
    }
    let applied = clamp(&mut device, &settings(Source::Feeder, 80, Format::Bw1));
    assert!(applied.clamped.is_empty());
}

#[test]
fn a_format_the_source_lacks_becomes_the_first_it_lists() {
    let mut feeder = source(Source::Feeder, Resolutions::List(vec![100]));
    feeder.formats = vec![Format::Gray8, Format::Bw1];
    let scanner = FakeDevice::new("a", "M", "N").caps(Caps::new(vec![feeder]));
    scanner.load(sheets(1));
    let mut device = open(&scanner);
    let (_, events) = run(
        &mut device,
        &settings(Source::Feeder, 100, Format::Rgb16),
        1,
    );
    assert_eq!(applied(&events).settings.format, Format::Gray8);
    assert_eq!(applied(&events).clamped, [Clamped::Format(Format::Rgb16)]);
    assert_eq!(raw(pages(&events)[0]).format(), Format::Gray8);
    let applied = clamp(&mut device, &settings(Source::Feeder, 100, Format::Bw1));
    assert!(applied.clamped.is_empty());
}

fn size(width: f32, height: f32) -> SizeMm {
    SizeMm { width, height }
}

#[test]
fn paper_beyond_the_largest_is_cut_to_it_each_way() {
    let mut device = open(&scanner());
    // The feeder's largest is Legal, 215.9 by 355.6 mm.
    for (asked, taken) in [
        (size(300.0, 400.0), SizeMm::LEGAL),
        (size(100.0, 400.0), size(100.0, 355.6)),
        (size(300.0, 100.0), size(215.9, 100.0)),
        (size(f32::MAX, f32::MAX), SizeMm::LEGAL),
    ] {
        let mut settings = settings(Source::Feeder, 50, Format::Bw1);
        settings.paper = Paper::Size(asked);
        let applied = clamp(&mut device, &settings);
        assert_eq!(applied.settings.paper, Paper::Size(taken), "{asked:?}");
        assert_eq!(applied.clamped, [Clamped::Paper(Paper::Size(asked))]);
    }
    for asked in [SizeMm::LEGAL, SizeMm::A4, size(0.1, 0.1)] {
        let mut settings = settings(Source::Feeder, 50, Format::Bw1);
        settings.paper = Paper::Size(asked);
        assert!(
            clamp(&mut device, &settings).clamped.is_empty(),
            "{asked:?}"
        );
    }
}

#[test]
fn paper_not_finite_and_above_0_becomes_the_largest() {
    let mut device = open(&scanner());
    for asked in [
        size(f32::NAN, 100.0),
        size(100.0, f32::NAN),
        size(f32::INFINITY, 100.0),
        size(100.0, f32::NEG_INFINITY),
        size(-1.0, 100.0),
        size(100.0, 0.0),
        size(-0.0, -0.0),
    ] {
        let mut settings = settings(Source::Feeder, 50, Format::Bw1);
        settings.paper = Paper::Size(asked);
        let applied = clamp(&mut device, &settings);
        assert_eq!(
            applied.settings.paper,
            Paper::Size(SizeMm::LEGAL),
            "{asked:?}"
        );
        // What was asked, compared with `is_nan`, since NaN equals nothing.
        let [Clamped::Paper(Paper::Size(said))] = applied.clamped[..] else {
            panic!("{:?}", applied.clamped);
        };
        for (said, asked) in [(said.width, asked.width), (said.height, asked.height)] {
            assert!(
                said == asked || (said.is_nan() && asked.is_nan()),
                "{said} {asked}"
            );
        }
    }
}

#[test]
fn any_size_on_a_source_with_no_largest_becomes_auto() {
    let mut feeder = source(Source::Feeder, Resolutions::List(vec![50]));
    feeder.paper_auto = true;
    let mut device = open(&FakeDevice::new("a", "M", "N").caps(Caps::new(vec![feeder])));
    for asked in [SizeMm::A4, size(f32::NAN, f32::NAN), size(1e9, 1e9)] {
        let mut settings = settings(Source::Feeder, 50, Format::Bw1);
        settings.paper = Paper::Size(asked);
        let applied = clamp(&mut device, &settings);
        assert_eq!(applied.settings.paper, Paper::Auto);
        assert_eq!(applied.clamped.len(), 1);
        assert!(matches!(applied.clamped[0], Clamped::Paper(Paper::Size(_))));
    }
}

#[test]
fn auto_without_size_detection_becomes_the_largest_or_stays_auto_with_none() {
    // The default flatbed finds no size: its largest, 215.9 by 297 mm.
    let mut device = open(&scanner());
    let taken = clamp(&mut device, &settings(Source::Flatbed, 300, Format::Bw1));
    assert_eq!(taken.settings.paper, Paper::Size(size(215.9, 297.0)));
    assert_eq!(taken.clamped, [Clamped::Paper(Paper::Auto)]);

    // A flatbed that says neither: `Auto` kept, and the sheet scanned at its own size.
    let flatbed = source(Source::Flatbed, Resolutions::List(vec![300]));
    let scanner = FakeDevice::new("a", "M", "N").caps(Caps::new(vec![flatbed]));
    scanner.glass(Sheet::new().size(SizeMm::A4));
    let mut device = open(&scanner);
    let (done, events) = run(&mut device, &settings(Source::Flatbed, 300, Format::Bw1), 1);
    assert_eq!(done, Ok(Done::OnePage));
    assert_eq!(applied(&events).settings.paper, Paper::Auto);
    assert!(applied(&events).clamped.is_empty());
    let image = raw(pages(&events)[0]);
    assert_eq!((image.width(), image.height()), (2480, 3508));

    // A feeder that finds the size: `Auto` kept.
    let mut device = open(&scanner_with_sheet(Sheet::new()));
    let taken = clamp(&mut device, &settings(Source::Feeder, 300, Format::Bw1));
    assert_eq!(taken.settings.paper, Paper::Auto);
    assert!(taken.clamped.is_empty());
}

/// A default device with `sheet` in its feeder.
fn scanner_with_sheet(sheet: Sheet) -> FakeDevice {
    let scanner = scanner();
    scanner.load([sheet]);
    scanner
}

#[test]
fn encodings_the_source_doesn_t_offer_are_dropped_the_rest_kept_in_order() {
    let mut feeder = source(Source::Feeder, Resolutions::List(vec![50]));
    feeder.encodings = vec![Encoding::Png, Encoding::Jpeg];
    let mut device = open(&FakeDevice::new("a", "M", "N").caps(Caps::new(vec![feeder])));
    let asked = vec![Encoding::Tiff, Encoding::Jpeg, Encoding::Bmp, Encoding::Png];
    let mut settings = settings(Source::Feeder, 50, Format::Rgb8);
    settings.encodings = asked.clone();
    let applied = clamp(&mut device, &settings);
    assert_eq!(applied.settings.encodings, [Encoding::Jpeg, Encoding::Png]);
    assert_eq!(applied.clamped, [Clamped::Encodings(asked)]);

    settings.encodings = vec![Encoding::Png, Encoding::Jpeg];
    assert!(clamp(&mut device, &settings).clamped.is_empty());
    settings.encodings = vec![Encoding::CcittG4];
    let applied = clamp(&mut device, &settings);
    assert!(applied.settings.encodings.is_empty());
    assert_eq!(
        applied.clamped,
        [Clamped::Encodings(vec![Encoding::CcittG4])]
    );
}

#[test]
fn every_change_is_said_in_the_settings_order() {
    let flatbed = SourceCaps::new(
        Source::Flatbed,
        Resolutions::List(vec![300]),
        vec![Format::Gray8],
    );
    let mut device = open(&FakeDevice::new("a", "M", "N").caps(Caps::new(vec![flatbed])));
    let mut asked = settings(Source::Feeder, 600, Format::Rgb8);
    asked.duplex = true;
    asked.paper = Paper::Size(SizeMm::A4);
    asked.encodings = vec![Encoding::Png];
    let applied = clamp(&mut device, &asked);
    let mut taken = settings(Source::Flatbed, 300, Format::Gray8);
    taken.paper = Paper::Auto;
    assert_eq!(applied.settings, taken);
    assert_eq!(
        applied.clamped,
        [
            Clamped::Source(Source::Feeder),
            Clamped::Duplex(true),
            Clamped::Dpi(600),
            Clamped::Format(Format::Rgb8),
            Clamped::Paper(Paper::Size(SizeMm::A4)),
            Clamped::Encodings(vec![Encoding::Png]),
        ]
    );
}

#[test]
fn wild_settings_are_clamped_without_a_panic() {
    let scanner = scanner();
    let mut device = open(&scanner);
    let papers = [
        Paper::Auto,
        Paper::Size(size(f32::NAN, f32::NAN)),
        Paper::Size(size(f32::INFINITY, f32::NEG_INFINITY)),
        Paper::Size(size(-1.0, -1.0)),
        Paper::Size(size(0.0, 0.0)),
        Paper::Size(size(f32::MAX, f32::MAX)),
        Paper::Size(size(f32::MIN_POSITIVE, f32::MIN_POSITIVE)),
    ];
    for source in [Source::Feeder, Source::Flatbed] {
        for dpi in [0, 1, u32::MAX] {
            for paper in papers {
                if source == Source::Feeder {
                    scanner.load(sheets(1));
                }
                let mut asked = settings(source, dpi, Format::Bw1);
                asked.duplex = true;
                asked.paper = paper;
                asked.encodings = vec![Encoding::CcittG4];
                let (done, events) = run(&mut device, &asked, u32::MAX);
                let taken = &applied(&events).settings;
                assert!(done.is_ok(), "{source:?} {dpi} {paper:?}: {done:?}");
                assert!((50..=600).contains(&taken.dpi), "{taken:?}");
                if let Paper::Size(size) = taken.paper {
                    assert!(
                        size.width > 0.0 && size.width <= SizeMm::LEGAL.width,
                        "{size:?}"
                    );
                    assert!(
                        size.height > 0.0 && size.height <= SizeMm::LEGAL.height,
                        "{size:?}"
                    );
                }
                for page in pages(&events) {
                    let image = raw(page);
                    assert!(image.width() >= 1 && image.height() >= 1);
                }
            }
        }
    }
    // A source that takes any resolution, asked for the largest: refused as a page past what
    // the fake draws, never an overflow.
    let range = Resolutions::Range {
        min: 1,
        max: u32::MAX,
        step: NonZeroU32::new(7).unwrap(),
    };
    let scanner =
        FakeDevice::new("a", "M", "N").caps(Caps::new(vec![source(Source::Feeder, range)]));
    scanner.load(sheets(1));
    let mut device = open(&scanner);
    let (done, events) = run(
        &mut device,
        &settings(Source::Feeder, u32::MAX, Format::Rgb16),
        1,
    );
    assert!(
        matches!(done, Err(ScanError::Driver { shown: false, .. })),
        "{done:?}"
    );
    // The last step not past the range's end.
    assert_eq!(applied(&events).settings.dpi, 1 + (u32::MAX - 1) / 7 * 7);
}

#[test]
fn a_page_is_the_paper_s_size_at_the_resolution() {
    let scanner = scanner();
    scanner.load(sheets(1));
    let mut device = open(&scanner);
    let mut asked = settings(Source::Feeder, 300, Format::Gray8);
    asked.paper = Paper::Size(SizeMm::LETTER);
    let (_, events) = run(&mut device, &asked, 1);
    let page = pages(&events)[0];
    let image = raw(page);
    // Letter, 8.5 by 11 inches.
    assert_eq!((image.width(), image.height()), (2550, 3300));
    assert_eq!(page.dpi, (300.0, 300.0));

    for (paper, dpi, width, height) in [
        (SizeMm::A4, 300, 2480, 3508),
        (SizeMm::LEGAL, 100, 850, 1400),
        (size(0.01, 0.01), 50, 1, 1),
        (size(25.4, 50.8), 600, 600, 1200),
    ] {
        scanner.load(sheets(1));
        let mut asked = settings(Source::Feeder, dpi, Format::Bw1);
        asked.paper = Paper::Size(paper);
        let (_, events) = run(&mut device, &asked, 1);
        let image = raw(pages(&events)[0]);
        assert_eq!(
            (image.width(), image.height()),
            (width, height),
            "{paper:?}"
        );
    }
}

#[test]
fn under_auto_a_page_is_the_sheet_s_own_size_cut_to_the_largest() {
    for (sheet, width, height) in [
        (Sheet::new(), 2550, 3300),
        (Sheet::new().size(SizeMm::A4), 2480, 3508),
        (Sheet::new().size(size(100.0, 100.0)), 1181, 1181),
        // Cut to Legal, the feeder's largest.
        (Sheet::new().size(size(300.0, 500.0)), 2550, 4200),
        (Sheet::new().size(size(100.0, 500.0)), 1181, 4200),
    ] {
        let mut device = open(&scanner_with_sheet(sheet));
        let (_, events) = run(&mut device, &settings(Source::Feeder, 300, Format::Bw1), 1);
        let image = raw(pages(&events)[0]);
        assert_eq!((image.width(), image.height()), (width, height));
    }
}

#[test]
fn a_drawn_page_is_the_pattern_s_for_its_sheet_and_side() {
    let scanner = scanner();
    scanner.load(sheets(2));
    let mut device = open(&scanner);
    let mut asked = settings(Source::Feeder, 50, Format::Rgb8);
    asked.duplex = true;
    let (_, events) = run(&mut device, &asked, 7);
    let pages = pages(&events);
    assert_eq!(pages.len(), 4);
    for page in &pages {
        let expected = pattern(Format::Rgb8, 425, 550, page.sheet, page.side).unwrap();
        assert_eq!(*raw(page), expected, "{} {:?}", page.sheet, page.side);
    }
    assert_ne!(pages[0].pixels, pages[1].pixels);
    assert_ne!(pages[0].pixels, pages[2].pixels);
}

#[test]
fn the_pattern_is_the_same_each_run_and_differs_by_sheet_and_side() {
    for format in [
        Format::Bw1,
        Format::Gray8,
        Format::Gray16,
        Format::Rgb8,
        Format::Rgb16,
    ] {
        let page = |sheet, side| pattern(format, 61, 17, sheet, side).unwrap();
        assert_eq!(page(1, Side::Front), page(1, Side::Front), "{format:?}");
        let drawn = [
            page(1, Side::Front),
            page(1, Side::Back),
            page(2, Side::Front),
            page(2, Side::Back),
            page(u32::MAX, Side::Back),
        ];
        for (i, one) in drawn.iter().enumerate() {
            for other in &drawn[i + 1..] {
                assert_ne!(one, other, "{format:?}");
            }
        }
        // Its rows differ from one another too.
        let image = page(3, Side::Front);
        let rows: Vec<&[u8]> = image.data().chunks_exact(image.stride()).collect();
        assert_ne!(rows[0], rows[1], "{format:?}");
    }
}

#[test]
fn the_pattern_s_black_and_white_padding_bits_are_0() {
    for width in 1..=17 {
        let image = pattern(Format::Bw1, width, 9, 4, Side::Back).unwrap();
        let used = width % 8;
        let padding = if used == 0 { 0 } else { 0xff_u8 >> used };
        for row in image.data().chunks_exact(image.stride()) {
            assert_eq!(row.last().unwrap() & padding, 0, "{width}");
        }
        // Remade from its bytes: the layout holds.
        let again = RawImage::new(Format::Bw1, width, 9, image.data().to_vec());
        assert_eq!(again.as_ref(), Ok(&image));
    }
    // And somewhere a pixel set and one clear.
    let image = pattern(Format::Bw1, 64, 4, 1, Side::Front).unwrap();
    assert!(image.data().iter().any(|&byte| byte != 0));
    assert!(image.data().iter().any(|&byte| byte != 0xff));
}

#[test]
fn the_pattern_refuses_what_a_raw_image_refuses() {
    assert_eq!(
        pattern(Format::Gray8, 0, 10, 1, Side::Front),
        Err(LayoutError::Empty)
    );
    assert_eq!(
        pattern(Format::Rgb16, u32::MAX, u32::MAX, 1, Side::Front),
        Err(LayoutError::TooLarge {
            width: u32::MAX,
            height: u32::MAX
        })
    );
}

#[test]
fn a_page_given_on_a_sheet_is_delivered_as_given() {
    let front = Pixels::Raw(RawImage::new(Format::Rgb16, 3, 2, (0..36).collect()).unwrap());
    let back = Pixels::Encoded(Encoded {
        kind: EncodedKind::Tiff,
        bytes: b"II*\0".to_vec(),
    });
    let scanner = scanner();
    scanner.load([
        Sheet::new()
            .side(Side::Front, front.clone())
            .side(Side::Back, back.clone()),
        Sheet::new().side(Side::Back, back.clone()),
    ]);
    let mut device = open(&scanner);
    // Whatever the settings: another format and size, and an encoding asked.
    let mut asked = settings(Source::Feeder, 50, Format::Gray8);
    asked.duplex = true;
    asked.paper = Paper::Size(SizeMm::A4);
    let (done, events) = run(&mut device, &asked, 1);
    assert_eq!(done, Ok(Done::FeederEmpty));
    let pages = pages(&events);
    assert_eq!(pages[0].pixels, front);
    assert_eq!(pages[1].pixels, back);
    // The side not given, drawn.
    assert_eq!(
        *raw(pages[2]),
        pattern(Format::Gray8, 413, 585, 2, Side::Front).unwrap()
    );
    assert_eq!(pages[3].pixels, back);
    for page in pages {
        assert_eq!(page.dpi, (50.0, 50.0));
    }
}

/// A feeder offering every encoded form, at 50 dpi.
fn encoding_scanner() -> FakeDevice {
    let mut feeder = source(Source::Feeder, Resolutions::List(vec![50]));
    feeder.duplex = true;
    feeder.max_area = Some(SizeMm::LETTER);
    feeder.encodings = vec![
        Encoding::CcittG4,
        Encoding::Jpeg,
        Encoding::Png,
        Encoding::Tiff,
        Encoding::Bmp,
    ];
    FakeDevice::new("a", "M", "N").caps(Caps::new(vec![feeder]))
}

#[test]
fn an_encoded_page_comes_in_the_first_form_asked_that_fits_its_format() {
    let g4 = EncodedKind::CcittG4 {
        width: 425,
        height: 550,
        black_is_1: true,
    };
    for (format, asked, kind, name) in [
        (
            Format::Bw1,
            vec![Encoding::Jpeg, Encoding::CcittG4, Encoding::Png],
            g4,
            "CCITT G4",
        ),
        (
            Format::Gray8,
            vec![Encoding::CcittG4, Encoding::Jpeg],
            EncodedKind::Jpeg,
            "JPEG",
        ),
        (
            Format::Rgb8,
            vec![Encoding::Jpeg, Encoding::Png],
            EncodedKind::Jpeg,
            "JPEG",
        ),
        (
            Format::Gray16,
            vec![Encoding::Jpeg, Encoding::Tiff],
            EncodedKind::Tiff,
            "TIFF",
        ),
        (
            Format::Rgb16,
            vec![Encoding::CcittG4, Encoding::Bmp],
            EncodedKind::Bmp,
            "BMP",
        ),
        (Format::Bw1, vec![Encoding::Png], EncodedKind::Png, "PNG"),
    ] {
        let scanner = encoding_scanner();
        scanner.load(sheets(1));
        let mut device = open(&scanner);
        let mut settings = settings(Source::Feeder, 50, format);
        settings.duplex = true;
        settings.encodings = asked;
        let (done, events) = run(&mut device, &settings, 3);
        assert_eq!(done, Ok(Done::FeederEmpty));
        let pages = pages(&events);
        for (page, side) in pages.iter().zip(["front", "back"]) {
            let Pixels::Encoded(encoded) = &page.pixels else {
                panic!("{format:?}: {:?}", page.pixels);
            };
            assert_eq!(encoded.kind, kind);
            assert_eq!(
                encoded.bytes,
                format!("fake {name} page: sheet 3, {side}").into_bytes()
            );
        }
    }
}

#[test]
fn a_page_is_raw_where_no_form_asked_fits() {
    for (format, asked) in [
        (Format::Rgb16, vec![Encoding::Jpeg, Encoding::CcittG4]),
        (Format::Gray16, vec![Encoding::Jpeg]),
        (Format::Rgb8, vec![Encoding::CcittG4]),
        (Format::Gray8, Vec::new()),
    ] {
        let scanner = encoding_scanner();
        scanner.load(sheets(1));
        let mut device = open(&scanner);
        let mut settings = settings(Source::Feeder, 50, format);
        settings.encodings = asked.clone();
        let (_, events) = run(&mut device, &settings, 1);
        assert_eq!(applied(&events).settings.encodings, asked);
        assert_eq!(
            *raw(pages(&events)[0]),
            pattern(format, 425, 550, 1, Side::Front).unwrap()
        );
    }
}

#[test]
fn a_page_past_512_mib_is_refused_before_it_s_drawn_the_pages_before_kept() {
    let mut feeder = source(
        Source::Feeder,
        Resolutions::Range {
            min: 50,
            max: 2400,
            step: NonZeroU32::new(50).unwrap(),
        },
    );
    feeder.max_area = Some(SizeMm::LEGAL);
    feeder.encodings = vec![Encoding::Png];
    let scanner = FakeDevice::new("a", "M", "N").caps(Caps::new(vec![feeder]));
    let given = Pixels::Raw(RawImage::new(Format::Gray8, 1, 1, vec![0]).unwrap());
    scanner.load([
        Sheet::new().side(Side::Front, given.clone()),
        Sheet::new(),
        Sheet::new(),
    ]);
    let mut device = open(&scanner);
    // Legal at 2400 dpi in 16-bit color: 20,400 by 33,600 pixels, 4,112,640,000 bytes.
    let mut asked = settings(Source::Feeder, 2400, Format::Rgb16);
    asked.paper = Paper::Size(SizeMm::LEGAL);
    let (done, events) = run(&mut device, &asked, 1);
    assert_eq!(
        done,
        Err(ScanError::Driver {
            message: "the fake draws no page past 512 MiB: 20400 by 33600 in Rgb16 is \
                      4112640000 bytes"
                .to_owned(),
            shown: false
        })
    );
    // The page given on sheet 1 delivered, and sheet 2 put back, unscanned.
    assert_eq!(
        outline(&events),
        ["applied", "sheet 1", "page 1 front", "sheet 2"]
    );
    assert_eq!(pages(&events)[0].pixels, given);
    assert_eq!((scanner.fed(), scanner.in_feeder()), (2, 2));
    // Even where it would come encoded.
    asked.encodings = vec![Encoding::Png];
    let (done, events) = run(&mut device, &asked, 2);
    assert!(matches!(done, Err(ScanError::Driver { .. })), "{done:?}");
    assert_eq!(outline(&events), ["applied", "sheet 2"]);
    assert_eq!((scanner.fed(), scanner.in_feeder()), (3, 2));
}

/// A call on [`Recorder`].
#[derive(Clone, Debug, PartialEq)]
enum Call {
    Total(Option<u64>, Unit),
    Set(u64),
    Inc(u64),
}

/// A progress that records each call, and reads the device's paper as each page begins, which a
/// lock held across the call would block.
struct Recorder {
    calls: Mutex<Vec<Call>>,
    device: FakeDevice,
}

impl Recorder {
    fn record(&self, call: Call) {
        self.calls.lock().unwrap().push(call);
    }
}

impl Progress for Recorder {
    fn set_total(&self, total: Option<u64>, unit: Unit) {
        let (fed, read) = mpsc::channel();
        let device = self.device.clone();
        thread::spawn(move || fed.send(device.fed()));
        read.recv_timeout(Duration::from_secs(10))
            .expect("the device's paper is free during a call on the progress");
        self.record(Call::Total(total, unit));
    }
    fn set(&self, done: u64) {
        self.record(Call::Set(done));
    }
    fn inc(&self, n: u64) {
        self.record(Call::Inc(n));
    }
    fn message(&self, _level: Level, _msg: &str) {}
    fn is_cancelled(&self) -> bool {
        false
    }
    fn child(&self, _name: &str, _weight: f64) -> Box<dyn Progress> {
        Box::new(NoProgress)
    }
}

#[test]
fn each_page_s_progress_is_its_bytes_a_row_at_a_time() {
    let scanner = encoding_scanner();
    let given = Pixels::Encoded(Encoded {
        kind: EncodedKind::Png,
        bytes: vec![1; 10],
    });
    scanner.load([Sheet::new().side(Side::Back, given)]);
    let mut device = open(&scanner);
    let recorder = Recorder {
        calls: Mutex::new(Vec::new()),
        device: scanner.clone(),
    };
    // 25 by 20 pixels in black and white: 4 bytes a row.
    let mut asked = settings(Source::Feeder, 50, Format::Bw1);
    asked.duplex = true;
    asked.paper = Paper::Size(size(12.7, 10.16));
    let (done, events) = run_with(&mut device, &asked, 1, &recorder);
    assert_eq!(done, Ok(Done::FeederEmpty));
    let image = raw(pages(&events)[0]);
    assert_eq!((image.width(), image.height(), image.stride()), (25, 20, 4));

    let mut expected = vec![Call::Total(Some(80), Unit::Bytes), Call::Set(0)];
    expected.extend(std::iter::repeat_n(Call::Inc(4), 20));
    expected.extend([
        Call::Total(Some(10), Unit::Bytes),
        Call::Set(0),
        Call::Inc(10),
    ]);
    assert_eq!(*recorder.calls.lock().unwrap(), expected);

    // An encoded page drawn: its stand-in's bytes, at once.
    scanner.load(sheets(1));
    recorder.calls.lock().unwrap().clear();
    asked.duplex = false;
    asked.encodings = vec![Encoding::CcittG4];
    let (_, events) = run_with(&mut device, &asked, 2, &recorder);
    let Pixels::Encoded(encoded) = &pages(&events)[0].pixels else {
        panic!("an encoded page");
    };
    let bytes = encoded.bytes.len() as u64;
    assert_eq!(
        *recorder.calls.lock().unwrap(),
        [
            Call::Total(Some(bytes), Unit::Bytes),
            Call::Set(0),
            Call::Inc(bytes)
        ]
    );
}

#[test]
fn the_tracker_is_finished_at_the_batch_s_end() {
    let scanner = scanner();
    scanner.load(sheets(3));
    let mut device = open(&scanner);
    let tracker = Tracker::new();
    let mut asked = settings(Source::Feeder, 100, Format::Gray16);
    asked.duplex = true;
    let (done, events) = run_with(&mut device, &asked, 1, &tracker);
    assert_eq!(done, Ok(Done::FeederEmpty));
    let last = raw(pages(&events)[5]).data().len() as u64;
    let snapshot = tracker.snapshot();
    assert_eq!(snapshot.fraction, Some(1.0));
    assert_eq!(snapshot.total, Some(last));
    assert_eq!(snapshot.done, last);
    assert_eq!(snapshot.unit, Unit::Bytes);
    assert!(!snapshot.cancelled);
}

#[test]
fn a_cancelled_tracker_ends_the_batch_before_a_sheet_is_taken() {
    let tracker = Tracker::new();
    tracker.cancel();
    for (scanner, source, loaded, before) in [
        (scanner(), Source::Feeder, 2, vec!["applied"]),
        (scanner(), Source::Feeder, 0, vec!["applied"]),
        (scanner(), Source::Flatbed, 1, vec!["applied"]),
        (
            scanner().warms_up(),
            Source::Feeder,
            1,
            vec!["applied", "warming up"],
        ),
    ] {
        scanner.load(sheets(loaded));
        let mut device = open(&scanner);
        let (done, events) = run_with(&mut device, &settings(source, 50, Format::Bw1), 1, &tracker);
        // Even with the feeder empty: cancelled, not empty.
        assert_eq!(done, Ok(Done::Cancelled), "{source:?} {loaded}");
        assert_eq!(outline(&events), before);
        assert_eq!((scanner.fed(), scanner.in_feeder()), (0, loaded));
    }
}

#[test]
fn a_receiver_gone_before_the_batch_ends_it_as_cancelled() {
    let scanner = scanner();
    scanner.load(sheets(2));
    let mut device = open(&scanner);
    let (events, received) = mpsc::sync_channel(1);
    drop(received);
    let done = device.scan(
        &settings(Source::Feeder, 50, Format::Bw1),
        1,
        &events,
        &Tracker::new(),
    );
    assert_eq!(done, Ok(Done::Cancelled));
    assert_eq!((scanner.fed(), scanner.in_feeder()), (0, 2));
}

#[test]
fn no_sheet_is_numbered_past_u32_max() {
    let scanner = scanner();
    scanner.load(sheets(2));
    let mut device = open(&scanner);
    let asked = settings(Source::Feeder, 50, Format::Bw1);
    let (done, events) = run(&mut device, &asked, u32::MAX);
    let Err(ScanError::Driver { message, shown }) = done else {
        panic!("{done:?}");
    };
    assert!(!shown);
    assert!(message.contains("4294967295"), "{message}");
    assert_eq!(
        outline(&events),
        ["applied", "sheet 4294967295", "page 4294967295 front"]
    );
    // The sheet after it left in the feeder.
    assert_eq!((scanner.fed(), scanner.in_feeder()), (1, 1));

    // Where the feeder empties at the last number, the batch ends as it would anywhere.
    let (done, events) = run(&mut device, &asked, u32::MAX);
    assert_eq!(done, Ok(Done::FeederEmpty));
    assert_eq!(
        outline(&events)[1..],
        ["sheet 4294967295", "page 4294967295 front"]
    );
}

/// `read(device)` on a thread of its own, within 10 s: the device's paper isn't locked.
fn within_10_s<T: Send + 'static>(device: &FakeDevice, read: fn(&FakeDevice) -> T) -> T {
    let (sent, got) = mpsc::channel();
    let device = device.clone();
    thread::spawn(move || sent.send(read(&device)));
    got.recv_timeout(Duration::from_secs(10))
        .expect("the device's paper is free while a batch waits to send")
}

#[test]
fn the_device_s_paper_is_free_while_a_batch_waits_on_its_channel() {
    let scanner = scanner();
    scanner.load(sheets(2));
    let mut device = open(&scanner);
    // A rendezvous channel: each `send` waits until the test takes its event.
    let (events, received) = mpsc::sync_channel(0);
    let batch = thread::spawn(move || {
        device.scan(
            &settings(Source::Feeder, 50, Format::Bw1),
            1,
            &events,
            &Tracker::new(),
        )
    });
    assert!(matches!(received.recv(), Ok(Event::Applied(_))));
    assert_eq!(received.recv(), Ok(Event::Sheet(1)));
    // Sheet 1's page waits to be taken, or is being drawn.
    assert_eq!(within_10_s(&scanner, FakeDevice::fed), 1);
    assert_eq!(within_10_s(&scanner, FakeDevice::in_feeder), 1);
    within_10_s(&scanner, |scanner| scanner.load(sheets(1)));
    let rest: Vec<Event> = received.iter().collect();
    assert_eq!(batch.join().unwrap(), Ok(Done::FeederEmpty));
    // The sheet loaded during the batch scanned too.
    assert_eq!(
        outline(&rest),
        [
            "page 1 front",
            "sheet 2",
            "page 2 front",
            "sheet 3",
            "page 3 front"
        ]
    );
    assert_eq!(scanner.fed(), 3);
}
