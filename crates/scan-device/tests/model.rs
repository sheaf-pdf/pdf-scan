//! The model as code outside the crate meets it: each `#[non_exhaustive]` struct made by its
//! constructor and its fields set, the traits' objects and the events sent between threads, a
//! `Tracker` through the re-export, and a device whose driver delivers on a thread of its own.

use std::{
    num::NonZeroU32,
    sync::mpsc::{self, SyncSender},
    thread,
    time::Duration,
};

use scan_device::{
    Applied, Backend, Caps, Clamped, Device, DeviceId, DeviceInfo, Done, Driver, Encoded,
    EncodedKind, Encoding, Event, Format, Opened, Paper, Pixels, RawImage, RawPage, Resolutions,
    ScanError, Settings, Side, SideKnown, SizeMm, Source, SourceCaps, Status, WindowClosed,
    WindowOwner,
    in_progress::{Progress, Tracker, Unit},
};

fn assert_send<T: Send>() {}

#[test]
fn the_traits_objects_the_events_and_their_sender_are_send() {
    assert_send::<Box<dyn Backend>>();
    assert_send::<Box<dyn Device>>();
    assert_send::<Opened>();
    assert_send::<Event>();
    assert_send::<SyncSender<Event>>();
    assert_send::<ScanError>();
}

#[test]
fn device_info_is_made_by_its_constructor_and_its_fields_set() {
    let mut info = DeviceInfo::new(
        DeviceId("fake:a".to_owned()),
        "Maker Model".to_owned(),
        "Maker".to_owned(),
        "Model".to_owned(),
        Driver::Fake,
    );
    assert_eq!(info.id, DeviceId("fake:a".to_owned()));
    assert_eq!(
        (info.name.as_str(), info.maker.as_str(), info.model.as_str()),
        ("Maker Model", "Maker", "Model")
    );
    assert_eq!(info.driver, Driver::Fake);

    info.id = DeviceId("twain32:Maker Model".to_owned());
    info.name = "Front desk".to_owned();
    info.maker = "Another".to_owned();
    info.model = "M2".to_owned();
    info.driver = Driver::Twain { bits: 32 };
    let copy = info.clone();
    assert_eq!(copy, info);
    assert_eq!(copy.id.0, "twain32:Maker Model");
    assert_eq!(copy.driver, Driver::Twain { bits: 32 });
    assert_ne!(copy.driver, Driver::Twain { bits: 64 });
}

#[test]
fn settings_are_one_side_auto_and_raw_until_set() {
    let mut settings = Settings::new(Source::Feeder, 300, Format::Rgb8);
    assert_eq!(settings.source, Source::Feeder);
    assert!(!settings.duplex);
    assert_eq!(settings.dpi, 300);
    assert_eq!(settings.format, Format::Rgb8);
    assert_eq!(settings.paper, Paper::Auto);
    assert!(settings.encodings.is_empty());

    settings.source = Source::Flatbed;
    settings.duplex = true;
    settings.dpi = 600;
    settings.format = Format::Bw1;
    settings.paper = Paper::Size(SizeMm::LETTER);
    settings.encodings = vec![Encoding::CcittG4, Encoding::Png];
    assert_eq!(settings.source, Source::Flatbed);
    assert!(settings.duplex);
    assert_eq!((settings.dpi, settings.format), (600, Format::Bw1));
    assert_eq!(settings.paper, Paper::Size(SizeMm::LETTER));
    assert_eq!(settings.encodings, [Encoding::CcittG4, Encoding::Png]);
    assert_ne!(settings, Settings::new(Source::Flatbed, 600, Format::Bw1));
}

#[test]
fn caps_and_a_source_s_caps_are_their_plainest_until_set() {
    let range = Resolutions::Range {
        min: 50,
        max: 600,
        step: NonZeroU32::new(1).unwrap(),
    };
    let mut feeder = SourceCaps::new(Source::Feeder, range.clone(), vec![Format::Bw1]);
    assert_eq!(feeder.source, Source::Feeder);
    assert!(!feeder.duplex);
    assert_eq!(feeder.resolutions, range);
    assert_eq!(feeder.formats, [Format::Bw1]);
    assert!(feeder.encodings.is_empty());
    assert_eq!(feeder.max_area, None);
    assert!(!feeder.paper_auto);

    feeder.duplex = true;
    feeder.formats.push(Format::Gray16);
    feeder.encodings = vec![Encoding::Jpeg];
    feeder.max_area = Some(SizeMm::LEGAL);
    feeder.paper_auto = true;
    feeder.resolutions = Resolutions::List(vec![150, 300]);
    assert!(feeder.duplex && feeder.paper_auto);
    assert_eq!(feeder.formats, [Format::Bw1, Format::Gray16]);
    assert_eq!(feeder.encodings, [Encoding::Jpeg]);
    assert_eq!(feeder.max_area, Some(SizeMm::LEGAL));
    assert_eq!(feeder.resolutions, Resolutions::List(vec![150, 300]));

    let flatbed = SourceCaps::new(
        Source::Flatbed,
        Resolutions::List(vec![75, 150, 300, 600]),
        vec![Format::Gray8, Format::Rgb8],
    );
    let mut caps = Caps::new(vec![feeder.clone(), flatbed.clone()]);
    assert_eq!(caps.sources, [feeder.clone(), flatbed]);
    assert!(!caps.settings_window);
    caps.settings_window = true;
    caps.sources.truncate(1);
    assert!(caps.settings_window);
    assert_eq!(caps.sources, [feeder]);
}

#[test]
fn applied_and_a_page_are_made_and_their_fields_set() {
    let asked = Settings::new(Source::Feeder, 1200, Format::Gray16);
    let mut taken = asked.clone();
    taken.dpi = 600;
    let mut applied = Applied::new(taken.clone(), vec![Clamped::Dpi(1200)]);
    assert_eq!(applied.settings, taken);
    assert_eq!(applied.clamped, [Clamped::Dpi(1200)]);
    applied
        .clamped
        .push(Clamped::Paper(Paper::Size(SizeMm::A4)));
    applied
        .clamped
        .push(Clamped::Encodings(vec![Encoding::Bmp]));
    applied.settings = asked.clone();
    assert_eq!(applied.settings, asked);
    assert_eq!(applied.clamped.len(), 3);

    let image = RawImage::new(Format::Gray8, 2, 2, vec![0, 64, 128, 255]).unwrap();
    let mut page = RawPage::new(
        Pixels::Raw(image.clone()),
        (300.0, 300.0),
        1,
        Side::Front,
        SideKnown::Inferred,
    );
    assert_eq!(page.pixels, Pixels::Raw(image));
    assert_eq!(page.dpi, (300.0, 300.0));
    assert_eq!(
        (page.sheet, page.side, page.side_known),
        (1, Side::Front, SideKnown::Inferred)
    );

    let encoded = Encoded {
        kind: EncodedKind::CcittG4 {
            width: 2,
            height: 2,
            black_is_1: true,
        },
        bytes: vec![0x26, 0xa0],
    };
    page.pixels = Pixels::Encoded(encoded.clone());
    page.dpi = (199.5, 200.0);
    page.sheet = 14;
    page.side = Side::Back;
    page.side_known = SideKnown::Reported;
    assert_eq!(page.pixels, Pixels::Encoded(encoded));
    assert_eq!(page.dpi, (199.5, 200.0));
    assert_eq!(
        (page.sheet, page.side, page.side_known),
        (14, Side::Back, SideKnown::Reported)
    );
}

#[test]
fn each_scan_error_says_what_went_wrong() {
    let cases = [
        (ScanError::FeederEmpty, "the feeder is empty"),
        (ScanError::Jammed, "the paper jammed"),
        (ScanError::DoubleFeed, "two sheets fed at once"),
        (ScanError::CoverOpen, "the cover is open"),
        (ScanError::Busy, "the scanner is busy"),
        (ScanError::Gone, "the scanner isn't there"),
        (
            ScanError::BadId,
            "the device id isn't one this driver gives",
        ),
        (
            ScanError::SettingsRefused,
            "the scanner wouldn't start with these settings",
        ),
        (ScanError::AccessDenied, "the scanner refused access"),
        (ScanError::TimedOut, "the driver didn't answer in time"),
        (ScanError::Unsupported, "the scanner can't do that"),
        (
            ScanError::NotInstalled {
                what: "SANE".to_owned(),
            },
            "SANE isn't installed",
        ),
        (
            ScanError::Driver {
                message: "Lamp failure".to_owned(),
                shown: true,
            },
            "Lamp failure",
        ),
    ];
    for (error, message) in cases {
        assert_eq!(error.to_string(), message, "{error:?}");
        let source: &dyn std::error::Error = &error;
        assert!(source.source().is_none());
    }
}

/// The device the test backend lists.
fn push_info() -> DeviceInfo {
    DeviceInfo::new(
        DeviceId("fake:push".to_owned()),
        "Push Scanner".to_owned(),
        "Push".to_owned(),
        "Scanner".to_owned(),
        Driver::Fake,
    )
}

/// A backend with one device, `Push`.
struct PushBackend;

impl Backend for PushBackend {
    fn list(&mut self, _deadline: Duration) -> Result<Vec<DeviceInfo>, ScanError> {
        Ok(vec![push_info()])
    }

    fn open(&mut self, device: &DeviceInfo, _deadline: Duration) -> Result<Opened, ScanError> {
        if !device.id.0.starts_with("fake:") {
            return Err(ScanError::BadId);
        }
        if device.id != push_info().id {
            return Err(ScanError::Gone);
        }
        Ok(Opened {
            device: Box::new(Push { sheets: 5 }),
            info: push_info(),
            renamed_from: None,
        })
    }
}

/// A device whose driver delivers its pages on a thread of its own: `scan` moves a child of its
/// progress and a clone of the channel into a thread it spawns, and waits for it.
struct Push {
    sheets: u32,
}

impl Device for Push {
    fn caps(&mut self) -> Result<Caps, ScanError> {
        let feeder = SourceCaps::new(
            Source::Feeder,
            Resolutions::List(vec![300]),
            vec![Format::Gray8],
        );
        Ok(Caps::new(vec![feeder]))
    }

    fn scan(
        &mut self,
        settings: &Settings,
        first_sheet: u32,
        events: &SyncSender<Event>,
        progress: &dyn Progress,
    ) -> Result<Done, ScanError> {
        let mut taken = settings.clone();
        let mut clamped = Vec::new();
        if (taken.source, taken.dpi, taken.format) != (Source::Feeder, 300, Format::Gray8) {
            clamped.push(Clamped::Dpi(taken.dpi));
            (taken.source, taken.dpi, taken.format) = (Source::Feeder, 300, Format::Gray8);
        }
        if events
            .send(Event::Applied(Applied::new(taken, clamped)))
            .is_err()
        {
            return Ok(Done::Cancelled);
        }
        if events.send(Event::Status(Status::WarmingUp)).is_err() {
            return Ok(Done::Cancelled);
        }
        let child = progress.child("pages", 1.0);
        let events = events.clone();
        let sheets = self.sheets;
        let driver = thread::spawn(move || -> Result<Done, ScanError> {
            for n in 0..sheets {
                // Between sheets, never within one.
                if child.check().is_err() {
                    return Ok(Done::Cancelled);
                }
                let sheet = first_sheet.checked_add(n).ok_or(ScanError::Driver {
                    message: "too many sheets".to_owned(),
                    shown: false,
                })?;
                if events.send(Event::Sheet(sheet)).is_err() {
                    return Ok(Done::Cancelled);
                }
                child.set_total(Some(4), Unit::Bytes);
                child.set(0);
                let image = RawImage::new(Format::Gray8, 2, 2, vec![sheet as u8; 4])
                    .expect("a 2 by 2 gray page");
                child.inc(4);
                let page = RawPage::new(
                    Pixels::Raw(image),
                    (300.0, 300.0),
                    sheet,
                    Side::Front,
                    SideKnown::Inferred,
                );
                if events.send(Event::Page(page)).is_err() {
                    return Ok(Done::Cancelled);
                }
            }
            Ok(Done::FeederEmpty)
        });
        driver.join().unwrap_or_else(|_| {
            Err(ScanError::Driver {
                message: "the driver's thread panicked".to_owned(),
                shown: false,
            })
        })
    }

    fn settings_window(&mut self, _owner: Option<WindowOwner>) -> Result<WindowClosed, ScanError> {
        Err(ScanError::Unsupported)
    }
}

/// Opens `Push` through a boxed backend, as an app would.
fn open_push() -> Opened {
    let mut backend: Box<dyn Backend> = Box::new(PushBackend);
    let listed = backend.list(Duration::from_secs(1)).unwrap();
    assert_eq!(listed, [push_info()]);
    backend
        .open(&listed[0], Duration::from_secs(1))
        .expect("the listed device opens")
}

#[test]
fn opened_says_what_was_found_and_leaves_the_device_out_of_its_debug() {
    let opened = open_push();
    assert_eq!(opened.info, push_info());
    assert_eq!(opened.renamed_from, None);
    let debug = format!("{opened:?}");
    assert!(debug.starts_with("Opened { info: DeviceInfo {"), "{debug}");
    assert!(debug.ends_with("renamed_from: None, .. }"), "{debug}");

    let mut backend = PushBackend;
    let mut gone = push_info();
    gone.id = DeviceId("fake:another".to_owned());
    let refused = backend.open(&gone, Duration::ZERO).unwrap_err();
    assert_eq!(refused, ScanError::Gone);
    gone.id = DeviceId("sane:test:0".to_owned());
    assert_eq!(
        backend.open(&gone, Duration::ZERO).unwrap_err(),
        ScanError::BadId
    );

    let mut device = opened.device;
    assert_eq!(device.caps().unwrap().sources[0].source, Source::Feeder);
    assert_eq!(
        device.settings_window(Some(WindowOwner(0x1234))),
        Err(ScanError::Unsupported)
    );
}

#[test]
fn a_push_device_delivers_from_its_own_thread_to_the_callers_tracker() {
    let mut device = open_push().device;
    let tracker = Tracker::new();
    let (events, received) = mpsc::sync_channel(2);
    let batch = {
        let tracker = tracker.clone();
        let mut settings = Settings::new(Source::Flatbed, 600, Format::Gray8);
        settings.paper = Paper::Size(SizeMm::A4);
        thread::spawn(move || device.scan(&settings, 7, &events, &tracker))
    };
    let got: Vec<Event> = received.iter().collect();
    assert_eq!(batch.join().unwrap(), Ok(Done::FeederEmpty));

    let Event::Applied(applied) = &got[0] else {
        panic!("{:?}", got[0]);
    };
    assert_eq!(applied.settings.source, Source::Feeder);
    assert_eq!(applied.clamped, [Clamped::Dpi(600)]);
    assert_eq!(got[1], Event::Status(Status::WarmingUp));
    let sheets: Vec<u32> = got[2..]
        .chunks(2)
        .map(|pair| match pair {
            [Event::Sheet(n), Event::Page(page)] if page.sheet == *n => *n,
            other => panic!("{other:?}"),
        })
        .collect();
    assert_eq!(sheets, [7, 8, 9, 10, 11]);
    // The child's share, finished as its thread ended, is the whole job.
    assert_eq!(tracker.snapshot().fraction, Some(1.0));
    assert!(!tracker.is_cancelled());
}

#[test]
fn a_push_device_sees_a_cancel_from_a_third_thread_at_its_next_check() {
    let mut device = open_push().device;
    let tracker = Tracker::new();
    // A rendezvous channel: each `send` returns once the test has taken its event.
    let (events, received) = mpsc::sync_channel(0);
    let batch = {
        let tracker = tracker.clone();
        let settings = Settings::new(Source::Feeder, 300, Format::Gray8);
        thread::spawn(move || device.scan(&settings, 1, &events, &tracker as &dyn Progress))
    };
    let Event::Applied(applied) = received.recv().unwrap() else {
        panic!("the settings first");
    };
    assert!(applied.clamped.is_empty());
    assert_eq!(received.recv().unwrap(), Event::Status(Status::WarmingUp));
    assert_eq!(received.recv().unwrap(), Event::Sheet(1));

    // Sheet 1's page waits to be taken while a third thread cancels the batch.
    let canceller = tracker.clone();
    thread::spawn(move || canceller.cancel()).join().unwrap();

    // The sheet in flight is finished...
    let Event::Page(page) = received.recv().unwrap() else {
        panic!("sheet 1's page");
    };
    assert_eq!((page.sheet, page.side), (1, Side::Front));
    // ...and the driver's thread stops at its next check: no other sheet, and the channel's
    // senders all gone once the batch has ended.
    assert!(received.recv().is_err());
    assert_eq!(batch.join().unwrap(), Ok(Done::Cancelled));
    assert!(tracker.is_cancelled());
    assert!(tracker.snapshot().cancelled);
}
