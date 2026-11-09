//! The fake backend's faults and controls: a driver's errors scripted on a sheet's side (a jam then
//! Continue, a double feed, an open cover, the device gone, an error to show), each fired once and
//! the sheet kept; a hang, a crash and an exit; a cancel between sheets, and a side that waits for
//! it; a batch held by a full channel, and one whose receiver is gone.

use std::{
    io::{self, Read, Seek, SeekFrom, Write},
    slice,
    sync::{
        atomic::{AtomicU32, Ordering},
        mpsc::{self, RecvTimeoutError},
    },
    thread,
    time::{Duration, Instant},
};

use scan_device::{
    Backend, Device, Done, Encoded, EncodedKind, Event, Format, Pixels, RawImage, RawPage,
    ScanError, Settings, Side, Source, Status,
    fake::{FakeBackend, FakeDevice, Fault, Sheet, When, pattern},
    in_progress::{Level, Progress, Tracker, Unit},
};

/// A deadline no listing here comes near.
const SOON: Duration = Duration::from_secs(5);

/// How long a test waits for what a batch on another thread is to reach, however slow the runner.
const AT_MOST: Duration = Duration::from_secs(10);

/// How long a test watches for what a batch on another thread is never to do.
const A_WHILE: Duration = Duration::from_millis(300);

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

/// Letter in gray at 50 dpi: 425 by 550 pixels, a byte each, so a page is quick to draw.
fn gray(source: Source, duplex: bool) -> Settings {
    let mut settings = Settings::new(source, 50, Format::Gray8);
    settings.duplex = duplex;
    settings
}

/// A Letter page's rows at 50 dpi, and its bytes in a row in gray.
const ROWS: u64 = 550;
const ROW_BYTES: u64 = 425;

/// A sheet whose `side` faults `when` as `fault`.
fn faulting(side: Side, when: When, fault: Fault) -> Sheet {
    Sheet::new().fault(side, when, fault)
}

/// Waits up to [`AT_MOST`] for `reached`, polling: a batch on another thread gets there however
/// slow the runner.
fn wait_for(what: &str, reached: impl Fn() -> bool) {
    let start = Instant::now();
    while !reached() {
        assert!(start.elapsed() < AT_MOST, "{what}: not within {AT_MOST:?}");
        thread::sleep(Duration::from_millis(5));
    }
}

/// `read(device)` on a thread of its own, within [`AT_MOST`]: the device's paper isn't locked.
fn within_10_s<T: Send + 'static>(device: &FakeDevice, read: fn(&FakeDevice) -> T) -> T {
    let (sent, got) = mpsc::channel();
    let device = device.clone();
    thread::spawn(move || sent.send(read(&device)));
    got.recv_timeout(AT_MOST)
        .expect("the device's paper is free while a batch waits")
}

#[test]
fn a_jam_at_sheet_4_then_continue_scans_it_and_the_rest() {
    let scanner = scanner();
    let mut loaded = sheets(6);
    loaded[3] = faulting(Side::Front, When::Start, Fault::Error(ScanError::Jammed));
    scanner.load(loaded);
    let mut device = open(&scanner);
    let asked = gray(Source::Feeder, false);
    let (done, events) = run(&mut device, &asked, 1);
    assert_eq!(done, Err(ScanError::Jammed));
    // Sheet 4 said before its jam, so an app can say where it jammed.
    assert_eq!(
        outline(&events),
        [
            "applied",
            "sheet 1",
            "page 1 front",
            "sheet 2",
            "page 2 front",
            "sheet 3",
            "page 3 front",
            "sheet 4"
        ]
    );
    // Sheet 4 put back at the head of the feeder, before 5 and 6.
    assert_eq!((scanner.fed(), scanner.in_feeder()), (4, 3));

    // Continue: the feeder run again from sheet 4, which scans now, to the feeder's end.
    let (done, events) = run(&mut device, &asked, 4);
    assert_eq!(done, Ok(Done::FeederEmpty));
    assert_eq!(
        outline(&events)[1..],
        [
            "sheet 4",
            "page 4 front",
            "sheet 5",
            "page 5 front",
            "sheet 6",
            "page 6 front"
        ]
    );
    assert_eq!(
        *raw(pages(&events)[0]),
        pattern(Format::Gray8, 425, 550, 4, Side::Front).unwrap()
    );
    // Sheet 4 counted at each taking.
    assert_eq!((scanner.fed(), scanner.in_feeder()), (7, 0));
}

#[test]
fn a_jam_midway_through_a_back_keeps_the_front_and_drops_the_back() {
    let scanner = scanner();
    scanner.load([
        faulting(Side::Back, When::Midway, Fault::Error(ScanError::Jammed)),
        Sheet::new(),
    ]);
    let mut device = open(&scanner);
    let asked = gray(Source::Feeder, true);
    let tracker = Tracker::new();
    let (done, events) = run_with(&mut device, &asked, 1, &tracker);
    assert_eq!(done, Err(ScanError::Jammed));
    assert_eq!(outline(&events), ["applied", "sheet 1", "page 1 front"]);
    // The back's first half counted, then the jam.
    let snapshot = tracker.snapshot();
    assert_eq!(snapshot.total, Some(ROWS * ROW_BYTES));
    assert_eq!(snapshot.done, ROWS / 2 * ROW_BYTES);
    assert_eq!((scanner.fed(), scanner.in_feeder()), (1, 2));

    // Continue from sheet 1: its front again, then its back.
    let (done, events) = run(&mut device, &asked, 1);
    assert_eq!(done, Ok(Done::FeederEmpty));
    assert_eq!(
        outline(&events)[1..],
        [
            "sheet 1",
            "page 1 front",
            "page 1 back",
            "sheet 2",
            "page 2 front",
            "page 2 back"
        ]
    );
    assert_eq!((scanner.fed(), scanner.in_feeder()), (3, 0));
}

#[test]
fn a_double_feed_ends_the_batch_at_its_sheet() {
    let scanner = scanner();
    scanner.load([
        Sheet::new(),
        faulting(
            Side::Front,
            When::Midway,
            Fault::Error(ScanError::DoubleFeed),
        ),
        Sheet::new(),
    ]);
    let mut device = open(&scanner);
    let asked = gray(Source::Feeder, false);
    let (done, events) = run(&mut device, &asked, 1);
    assert_eq!(done, Err(ScanError::DoubleFeed));
    assert_eq!(
        outline(&events),
        ["applied", "sheet 1", "page 1 front", "sheet 2"]
    );
    assert_eq!((scanner.fed(), scanner.in_feeder()), (2, 2));
    let (done, events) = run(&mut device, &asked, 2);
    assert_eq!(done, Ok(Done::FeederEmpty));
    assert_eq!(
        outline(&events)[1..],
        ["sheet 2", "page 2 front", "sheet 3", "page 3 front"]
    );
}

#[test]
fn an_open_cover_before_the_first_page_comes_after_the_warm_up() {
    let scanner = scanner().warms_up();
    scanner.load([
        faulting(Side::Front, When::Start, Fault::Error(ScanError::CoverOpen)),
        Sheet::new(),
    ]);
    let mut device = open(&scanner);
    let asked = gray(Source::Feeder, false);
    let (done, events) = run(&mut device, &asked, 1);
    assert_eq!(done, Err(ScanError::CoverOpen));
    assert_eq!(outline(&events), ["applied", "warming up", "sheet 1"]);
    assert_eq!((scanner.fed(), scanner.in_feeder()), (1, 2));
    // The cover closed: the same batch again.
    let (done, events) = run(&mut device, &asked, 1);
    assert_eq!(done, Ok(Done::FeederEmpty));
    assert_eq!(
        outline(&events),
        [
            "applied",
            "warming up",
            "sheet 1",
            "page 1 front",
            "sheet 2",
            "page 2 front"
        ]
    );
}

#[test]
fn a_driver_s_error_ends_the_batch_as_scripted_to_show_or_not() {
    let lamp = ScanError::Driver {
        message: "Lamp failure: call service".to_owned(),
        shown: true,
    };
    let io = ScanError::Driver {
        message: "I/O error".to_owned(),
        shown: false,
    };
    let scanner = scanner();
    scanner.load([
        faulting(Side::Front, When::Start, Fault::Error(lamp.clone())),
        faulting(Side::Front, When::Midway, Fault::Error(io.clone())),
    ]);
    let mut device = open(&scanner);
    let asked = gray(Source::Feeder, false);
    let (done, events) = run(&mut device, &asked, 1);
    assert_eq!(done, Err(lamp));
    assert_eq!(outline(&events), ["applied", "sheet 1"]);
    let (done, events) = run(&mut device, &asked, 1);
    assert_eq!(done, Err(io));
    assert_eq!(
        outline(&events),
        ["applied", "sheet 1", "page 1 front", "sheet 2"]
    );
    let (done, events) = run(&mut device, &asked, 2);
    assert_eq!(done, Ok(Done::FeederEmpty));
    assert_eq!(outline(&events), ["applied", "sheet 2", "page 2 front"]);
}

#[test]
fn faults_on_one_side_fire_once_each_in_the_order_scripted() {
    let scanner = scanner();
    scanner.load([Sheet::new()
        .fault(Side::Front, When::Start, Fault::Error(ScanError::Jammed))
        .fault(
            Side::Front,
            When::Midway,
            Fault::Error(ScanError::DoubleFeed),
        )
        .fault(Side::Front, When::Start, Fault::Error(ScanError::CoverOpen))]);
    let mut device = open(&scanner);
    let asked = gray(Source::Feeder, false);
    // A start's faults before a midway's: the second start's comes once the first's has fired.
    for (expected, outlined) in [
        (Err(ScanError::Jammed), &["applied", "sheet 1"][..]),
        (Err(ScanError::CoverOpen), &["applied", "sheet 1"][..]),
        (Err(ScanError::DoubleFeed), &["applied", "sheet 1"][..]),
        (
            Ok(Done::FeederEmpty),
            &["applied", "sheet 1", "page 1 front"][..],
        ),
    ] {
        let (done, events) = run(&mut device, &asked, 1);
        assert_eq!(done, expected);
        assert_eq!(outline(&events), outlined);
    }
    assert_eq!((scanner.fed(), scanner.in_feeder()), (4, 0));
}

#[test]
fn midway_through_an_encoded_page_is_before_its_bytes_are_counted() {
    let encoded = Pixels::Encoded(Encoded {
        kind: EncodedKind::Png,
        bytes: vec![7; 10],
    });
    // A row high: midway is before its one row.
    let one_row = Pixels::Raw(RawImage::new(Format::Gray8, 3, 1, vec![1, 2, 3]).unwrap());
    for (given, total) in [(encoded, 10), (one_row, 3)] {
        let scanner = scanner();
        scanner.load([Sheet::new().side(Side::Front, given.clone()).fault(
            Side::Front,
            When::Midway,
            Fault::Error(ScanError::Jammed),
        )]);
        let mut device = open(&scanner);
        let asked = gray(Source::Feeder, false);
        let tracker = Tracker::new();
        let (done, _) = run_with(&mut device, &asked, 1, &tracker);
        assert_eq!(done, Err(ScanError::Jammed));
        let snapshot = tracker.snapshot();
        assert_eq!((snapshot.total, snapshot.done), (Some(total), 0));
        // Run again: delivered as given, its bytes all counted.
        let tracker = Tracker::new();
        let (done, events) = run_with(&mut device, &asked, 1, &tracker);
        assert_eq!(done, Ok(Done::FeederEmpty));
        assert_eq!(pages(&events)[0].pixels, given);
        assert_eq!(tracker.snapshot().done, total);
    }
}

#[test]
fn a_fault_on_a_side_the_batch_doesn_t_read_doesn_t_fire() {
    let scanner = scanner();
    scanner.load([faulting(
        Side::Back,
        When::Start,
        Fault::Error(ScanError::Jammed),
    )]);
    let mut device = open(&scanner);
    let (done, events) = run(&mut device, &gray(Source::Feeder, false), 1);
    assert_eq!(done, Ok(Done::FeederEmpty));
    assert_eq!(outline(&events), ["applied", "sheet 1", "page 1 front"]);
}

#[test]
fn a_fault_on_the_glass_fires_once_and_its_sheet_stays_there() {
    let scanner = scanner();
    scanner.glass(faulting(
        Side::Front,
        When::Midway,
        Fault::Error(ScanError::CoverOpen),
    ));
    let mut device = open(&scanner);
    let asked = gray(Source::Flatbed, false);
    let (done, events) = run(&mut device, &asked, 1);
    assert_eq!(done, Err(ScanError::CoverOpen));
    assert_eq!(outline(&events), ["applied", "sheet 1"]);
    let (done, events) = run(&mut device, &asked, 1);
    assert_eq!(done, Ok(Done::OnePage));
    assert_eq!(outline(&events), ["applied", "sheet 1", "page 1 front"]);
    assert_eq!(scanner.fed(), 0);
}

#[test]
fn a_sheet_put_on_the_glass_during_a_batch_stays_there() {
    let scanner = scanner();
    scanner.glass(faulting(
        Side::Front,
        When::Midway,
        Fault::Error(ScanError::CoverOpen),
    ));
    let mut device = open(&scanner);
    let (events, received) = mpsc::sync_channel(0);
    let batch = thread::spawn(move || {
        device.scan(&gray(Source::Flatbed, false), 1, &events, &Tracker::new())
    });
    assert!(matches!(received.recv(), Ok(Event::Applied(_))));
    assert_eq!(received.recv(), Ok(Event::Sheet(1)));
    // Another sheet put down while the first is read, with a fault of its own.
    scanner.glass(faulting(
        Side::Front,
        When::Start,
        Fault::Error(ScanError::Jammed),
    ));
    assert!(received.recv().is_err());
    assert_eq!(batch.join().unwrap(), Err(ScanError::CoverOpen));
    // The batch didn't put the sheet it read back over the new one.
    let mut device = open(&scanner);
    let (done, _) = run(&mut device, &gray(Source::Flatbed, false), 1);
    assert_eq!(done, Err(ScanError::Jammed));
}

#[test]
#[should_panic(expected = "an empty feeder isn't a fault a sheet can script")]
fn an_empty_feeder_isn_t_a_fault_a_sheet_can_script() {
    let _ = Sheet::new().fault(
        Side::Front,
        When::Start,
        Fault::Error(ScanError::FeederEmpty),
    );
}

#[test]
fn gone_mid_batch_leaves_the_device_gone_to_every_call() {
    let scanner = scanner();
    let other = FakeDevice::new("libusb:001:005", "Maker", "Other model");
    scanner.load([
        Sheet::new(),
        faulting(Side::Front, When::Midway, Fault::Error(ScanError::Gone)),
        Sheet::new(),
    ]);
    let mut backend = FakeBackend::new().with(scanner.clone()).with(other.clone());
    let stored = backend.list(SOON).unwrap().remove(0);
    assert_eq!(stored, scanner.info());
    let mut device = backend.open(&stored, SOON).unwrap().device;
    let asked = gray(Source::Feeder, false);
    let (done, events) = run(&mut device, &asked, 1);
    assert_eq!(done, Err(ScanError::Gone));
    assert_eq!(
        outline(&events),
        ["applied", "sheet 1", "page 1 front", "sheet 2"]
    );
    assert_eq!((scanner.fed(), scanner.in_feeder()), (2, 2));

    // Every call on the device open answers `Gone`; a batch sends nothing and takes nothing.
    assert_eq!(device.caps(), Err(ScanError::Gone));
    assert_eq!(device.settings_window(None), Err(ScanError::Gone));
    let (done, events) = run(&mut device, &asked, 2);
    assert_eq!(done, Err(ScanError::Gone));
    assert!(events.is_empty(), "{events:?}");
    assert_eq!((scanner.fed(), scanner.in_feeder()), (2, 2));

    // Listed no more, and opened no more, whichever backend holds it.
    assert_eq!(backend.list(SOON), Ok(vec![other.info()]));
    assert_eq!(
        backend.open(&stored, SOON).map(|opened| opened.info),
        Err(ScanError::Gone)
    );
    let mut again = FakeBackend::new().with(scanner.clone());
    assert_eq!(again.list(SOON), Ok(vec![]));
    assert_eq!(
        again.open(&stored, SOON).map(|opened| opened.info),
        Err(ScanError::Gone)
    );

    // Where the same maker and model is listed under another USB name, that one is found in its
    // place, never the device gone.
    let twin = FakeDevice::new("libusb:002:001", "Maker", "Model");
    let mut beside = FakeBackend::new().with(scanner.clone()).with(twin.clone());
    let opened = beside.open(&stored, SOON).unwrap();
    assert_eq!(opened.info, twin.info());
    assert_eq!(opened.renamed_from, Some(stored.id.clone()));
}

/// A progress that passes each call to a tracker and counts the pages begun, and cancels the
/// tracker from another thread at the middle row of the page it's given, counted from 1.
struct Watch {
    tracker: Tracker,
    cancel_at_page: Option<u32>,
    pages: AtomicU32,
    rows: AtomicU32,
}

impl Watch {
    fn new(tracker: &Tracker, cancel_at_page: Option<u32>) -> Watch {
        Watch {
            tracker: tracker.clone(),
            cancel_at_page,
            pages: AtomicU32::new(0),
            rows: AtomicU32::new(0),
        }
    }

    /// The pages begun so far.
    fn pages(&self) -> u32 {
        self.pages.load(Ordering::SeqCst)
    }
}

impl Progress for Watch {
    fn set_total(&self, total: Option<u64>, unit: Unit) {
        self.pages.fetch_add(1, Ordering::SeqCst);
        self.rows.store(0, Ordering::SeqCst);
        self.tracker.set_total(total, unit);
    }
    fn set(&self, done: u64) {
        self.tracker.set(done);
    }
    fn inc(&self, n: u64) {
        self.tracker.inc(n);
        let rows = self.rows.fetch_add(1, Ordering::SeqCst) + 1;
        if Some(self.pages()) == self.cancel_at_page && u64::from(rows) == ROWS / 2 {
            // As an app's cancel comes: from a thread of its own.
            let tracker = self.tracker.clone();
            thread::spawn(move || tracker.cancel()).join().unwrap();
        }
    }
    fn message(&self, level: Level, msg: &str) {
        self.tracker.message(level, msg);
    }
    fn is_cancelled(&self) -> bool {
        self.tracker.is_cancelled()
    }
    fn child(&self, name: &str, weight: f64) -> Box<dyn Progress> {
        self.tracker.child(name, weight)
    }
}

#[test]
fn a_cancel_mid_page_finishes_the_sheet_front_and_back_and_feeds_no_more() {
    // Cancelled at sheet 2's page in simplex, and at its front's and its back's in duplex.
    for (duplex, at_page) in [(false, 2), (true, 3), (true, 4)] {
        let scanner = scanner();
        scanner.load(sheets(3));
        let mut device = open(&scanner);
        let tracker = Tracker::new();
        let watch = Watch::new(&tracker, Some(at_page));
        let (done, events) = run_with(&mut device, &gray(Source::Feeder, duplex), 1, &watch);
        assert_eq!(done, Ok(Done::Cancelled), "{duplex} {at_page}");
        let mut expected = vec!["applied", "sheet 1", "page 1 front"];
        if duplex {
            expected.push("page 1 back");
        }
        expected.extend(["sheet 2", "page 2 front"]);
        if duplex {
            expected.push("page 2 back");
        }
        assert_eq!(outline(&events), expected, "{duplex} {at_page}");
        // Sheet 2 finished whole; sheet 3 never taken.
        assert!(pages(&events).iter().all(|page| raw(page).height() == 550));
        assert_eq!(
            (scanner.fed(), scanner.in_feeder()),
            (2, 1),
            "{duplex} {at_page}"
        );
        assert!(tracker.snapshot().cancelled);
    }
}

#[test]
fn a_cancel_after_a_duplex_front_still_delivers_its_back() {
    // Cancelled once the front is taken, and once it's read whole but waits to be taken: either
    // way the back is still read and sent, and no sheet comes after.
    for taken in [true, false] {
        let scanner = scanner();
        scanner.load(sheets(3));
        let mut device = open(&scanner);
        let tracker = Tracker::new();
        // A rendezvous channel: each `send` returns once the test has taken its event.
        let (events, received) = mpsc::sync_channel(0);
        let batch = {
            let tracker = tracker.clone();
            thread::spawn(move || device.scan(&gray(Source::Feeder, true), 1, &events, &tracker))
        };
        assert!(matches!(received.recv(), Ok(Event::Applied(_))));
        assert_eq!(received.recv(), Ok(Event::Sheet(1)));
        if !taken {
            // Its rows all counted, so it's sent or about to be, and the back not begun.
            wait_for("sheet 1's front read whole", || {
                let snapshot = tracker.snapshot();
                snapshot.total == Some(ROWS * ROW_BYTES) && snapshot.done == ROWS * ROW_BYTES
            });
            tracker.cancel();
        }
        let Ok(Event::Page(front)) = received.recv() else {
            panic!("sheet 1's front");
        };
        assert_eq!((front.sheet, front.side), (1, Side::Front));
        if taken {
            // From this thread, the batch's on its way to the back.
            tracker.cancel();
        }
        let rest: Vec<Event> = received.iter().collect();
        assert_eq!(batch.join().unwrap(), Ok(Done::Cancelled), "{taken}");
        assert_eq!(outline(&rest), ["page 1 back"], "{taken}");
        assert_eq!((scanner.fed(), scanner.in_feeder()), (1, 2), "{taken}");
    }
}

/// A batch of three duplex sheets in gray, sheet 2 scripted with `AwaitCancel` on `side`
/// `when`, on a thread of its own with a rendezvous channel: the events up to the wait taken, then
/// nothing sent while the side waits, then the cancel, and the rest. Gives the events taken
/// before the cancel and after it.
fn await_cancel(side: Side, when: When) -> (Vec<Event>, Vec<Event>) {
    let scanner = scanner();
    scanner.load([
        Sheet::new(),
        faulting(side, when, Fault::AwaitCancel),
        Sheet::new(),
    ]);
    let mut device = open(&scanner);
    let tracker = Tracker::new();
    // A rendezvous channel: each `send` returns once the test has taken its event.
    let (events, received) = mpsc::sync_channel(0);
    let batch = {
        let tracker = tracker.clone();
        thread::spawn(move || device.scan(&gray(Source::Feeder, true), 1, &events, &tracker))
    };
    // Up to the side that waits: sheet 2's `Sheet`, and its front's page where the back waits.
    let before = match side {
        Side::Front => 5,
        Side::Back => 6,
    };
    let mut taken = Vec::new();
    for _ in 0..before {
        taken.push(
            received
                .recv_timeout(AT_MOST)
                .expect("an event before the wait"),
        );
    }
    // Where the side waits: half its rows counted midway; at a back's start, its front's whole
    // and none of the back's yet.
    let page = ROWS * ROW_BYTES;
    let waits_at = match when {
        When::Midway => ROWS / 2 * ROW_BYTES,
        _ => page,
    };
    wait_for("the side waiting", || {
        let snapshot = tracker.snapshot();
        snapshot.total == Some(page) && snapshot.done == waits_at
    });
    // It waits, sending nothing and taking no sheet, until the cancel.
    assert_eq!(
        received.recv_timeout(A_WHILE),
        Err(RecvTimeoutError::Timeout)
    );
    assert!(!batch.is_finished());
    assert_eq!(tracker.snapshot().done, waits_at);
    assert_eq!(within_10_s(&scanner, FakeDevice::fed), 2);
    tracker.cancel();
    let after: Vec<Event> = received.iter().collect();
    assert_eq!(batch.join().unwrap(), Ok(Done::Cancelled));
    // Sheet 3 never taken.
    assert_eq!((scanner.fed(), scanner.in_feeder()), (2, 1));
    (taken, after)
}

#[test]
fn awaiting_the_cancel_midway_through_a_front_finishes_the_sheet_and_feeds_no_more() {
    let (before, after) = await_cancel(Side::Front, When::Midway);
    assert_eq!(
        outline(&before),
        [
            "applied",
            "sheet 1",
            "page 1 front",
            "page 1 back",
            "sheet 2"
        ]
    );
    // Both of sheet 2's sides, whole.
    assert_eq!(outline(&after), ["page 2 front", "page 2 back"]);
    for page in pages(&after) {
        assert_eq!(
            raw(page),
            &pattern(Format::Gray8, 425, 550, 2, page.side).unwrap()
        );
    }
}

#[test]
fn awaiting_the_cancel_at_a_back_s_start_delivers_the_front_first() {
    let (before, after) = await_cancel(Side::Back, When::Start);
    assert_eq!(
        outline(&before),
        [
            "applied",
            "sheet 1",
            "page 1 front",
            "page 1 back",
            "sheet 2",
            "page 2 front"
        ]
    );
    assert_eq!(outline(&after), ["page 2 back"]);
    assert_eq!(
        raw(pages(&after)[0]),
        &pattern(Format::Gray8, 425, 550, 2, Side::Back).unwrap()
    );
}

#[test]
fn a_batch_held_by_a_full_channel_takes_no_further_sheet() {
    let scanner = scanner();
    scanner.load(sheets(3));
    let mut device = open(&scanner);
    // A bound of 1 and nobody reading: `Applied` fills the channel, and the batch holds at sheet
    // 1's `Sheet`.
    let (events, received) = mpsc::sync_channel(1);
    let batch = thread::spawn(move || {
        device.scan(&gray(Source::Feeder, false), 1, &events, &Tracker::new())
    });
    wait_for("sheet 1 taken", || scanner.fed() == 1);
    thread::sleep(A_WHILE);
    assert_eq!((scanner.fed(), scanner.in_feeder()), (1, 2));
    assert!(!batch.is_finished());
    // Read at last: every page once.
    let got: Vec<Event> = received.iter().collect();
    assert_eq!(batch.join().unwrap(), Ok(Done::FeederEmpty));
    assert_eq!(
        outline(&got),
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
    assert_eq!(scanner.fed(), 3);
}

#[test]
fn a_receiver_gone_mid_batch_ends_it_and_no_sheet_is_taken_after() {
    for duplex in [false, true] {
        let scanner = scanner();
        scanner.load(sheets(3));
        let mut device = open(&scanner);
        let watch = Watch::new(&Tracker::new(), None);
        let (events, received) = mpsc::sync_channel(0);
        let done = thread::scope(|scope| {
            let watch = &watch;
            let batch =
                scope.spawn(move || device.scan(&gray(Source::Feeder, duplex), 1, &events, watch));
            assert!(matches!(received.recv(), Ok(Event::Applied(_))));
            assert_eq!(received.recv(), Ok(Event::Sheet(1)));
            assert!(matches!(received.recv(), Ok(Event::Page(_))));
            // Gone once sheet 1's front is taken.
            drop(received);
            batch.join().unwrap()
        });
        assert_eq!(done, Ok(Done::Cancelled), "{duplex}");
        if duplex {
            // The back read, its send failing: nothing taken after it.
            assert_eq!(watch.pages(), 2);
            assert_eq!((scanner.fed(), scanner.in_feeder()), (1, 2));
        } else {
            // Sheet 2 taken, as a driver's start feeds it, and its `Sheet` failing: its page
            // never read, and nothing taken after it.
            assert_eq!(watch.pages(), 1);
            assert_eq!((scanner.fed(), scanner.in_feeder()), (2, 1));
        }
    }
}

#[test]
fn a_hang_blocks_its_thread_for_good_while_the_paper_still_answers() {
    let scanner = scanner();
    scanner.load([
        Sheet::new(),
        faulting(Side::Front, When::Midway, Fault::Hang),
        Sheet::new(),
    ]);
    let mut device = open(&scanner);
    let tracker = Tracker::new();
    let (events, received) = mpsc::sync_channel(0);
    // A thread of its own, left blocked when the test ends.
    let batch = {
        let tracker = tracker.clone();
        thread::spawn(move || device.scan(&gray(Source::Feeder, false), 1, &events, &tracker))
    };
    let mut taken = Vec::new();
    for _ in 0..4 {
        taken.push(
            received
                .recv_timeout(AT_MOST)
                .expect("an event before the hang"),
        );
    }
    assert_eq!(
        outline(&taken),
        ["applied", "sheet 1", "page 1 front", "sheet 2"]
    );
    // Sheet 2's front hangs midway: half its rows counted, then nothing.
    wait_for("half of sheet 2's rows counted", || {
        tracker.snapshot().done == ROWS / 2 * ROW_BYTES
    });
    tracker.cancel();
    assert_eq!(
        received.recv_timeout(A_WHILE),
        Err(RecvTimeoutError::Timeout)
    );
    assert!(!batch.is_finished());
    assert_eq!(tracker.snapshot().done, ROWS / 2 * ROW_BYTES);
    assert_eq!(within_10_s(&scanner, FakeDevice::fed), 2);
    assert_eq!(within_10_s(&scanner, FakeDevice::in_feeder), 1);
}

/// What a child's line for an event starts with, among the test harness's own.
const EVENT_MARK: &str = "fake-event: ";

/// The exit code a child's [`Fault::Exit`] ends it with: neither a pass (0), a test's panic
/// (101), nor rusty-fork's (70).
const EXIT_CODE: i32 = 42;

/// In the child: a duplex batch of three sheets whose second's front faults as it starts, its
/// events taken on a rendezvous channel, each written out and flushed before the next is taken.
/// Every event before sheet 2's `Sheet` is written before sheet 2's front starts.
fn a_batch_ended_by(fault: Fault) {
    let scanner = scanner();
    scanner.load([
        Sheet::new(),
        faulting(Side::Front, When::Start, fault),
        Sheet::new(),
    ]);
    let mut device = open(&scanner);
    let (events, received) = mpsc::sync_channel(0);
    let batch = thread::spawn(move || {
        device.scan(&gray(Source::Feeder, true), 1, &events, &Tracker::new())
    });
    // Each line written whole by one call, and the standard output locked only for it, never
    // while an event is waited for: an exit on the batch's thread may flush the standard output
    // and make it unbuffered as this thread writes, and a line written in pieces could then end
    // the output part written.
    let mut out = io::stdout();
    for event in received {
        for line in outline(slice::from_ref(&event)) {
            out.write_all(format!("{EVENT_MARK}{line}\n").as_bytes())
                .unwrap();
        }
        out.flush().unwrap();
    }
    panic!("the batch ended, the process with it: {:?}", batch.join());
}

/// In the parent: how the child ended, and the events it wrote, once it has ended.
fn ended(
    test: &str,
    child: &mut rusty_fork::ChildWrapper,
    output: &mut std::fs::File,
) -> (rusty_fork::ExitStatusWrapper, Vec<String>) {
    let status = child
        .wait_timeout(Duration::from_secs(60))
        .expect("the child could be waited for")
        .unwrap_or_else(|| panic!("{test}: the child ran past 60 s"));
    let mut text = String::new();
    output.seek(SeekFrom::Start(0)).unwrap();
    output.read_to_string(&mut text).unwrap();
    // Whole lines alone: one the process ended part way through wasn't written.
    let events = text
        .split_inclusive('\n')
        .filter_map(|line| line.strip_suffix('\n'))
        .filter_map(|line| line.trim_end_matches('\r').strip_prefix(EVENT_MARK))
        .map(str::to_owned)
        .collect();
    (status, events)
}

/// The events a child wrote before its fault at sheet 2's front: sheet 1's, then sheet 2's
/// `Sheet`, written or not as the fault came, since the child takes it as the batch goes on.
fn written_before_sheet_2(test: &str, events: &[String]) {
    let before = ["applied", "sheet 1", "page 1 front", "page 1 back"];
    let with_sheet_2 = [&before[..], &["sheet 2"]].concat();
    assert!(
        events == before || events == with_sheet_2,
        "{test}: the child wrote {events:?}"
    );
}

/// Signal 6, which an abort raises on Unix.
#[cfg(unix)]
const SIGABRT: i32 = 6;

/// The exit code a process that aborts ends with on Windows: an abort is a fast fail there,
/// `STATUS_STACK_BUFFER_OVERRUN`.
#[cfg(windows)]
const ABORTED: i32 = 0xC000_0409_u32 as i32;

/// Whether a child ended by aborting: by `SIGABRT` on Unix, by its exit code on Windows.
fn aborted(status: &rusty_fork::ExitStatusWrapper) -> bool {
    #[cfg(unix)]
    {
        status.unix_signal() == Some(SIGABRT)
    }
    #[cfg(windows)]
    {
        status.code() == Some(ABORTED)
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = status;
        false
    }
}

#[test]
fn a_crash_ends_the_process_after_the_pages_before_it() {
    let test =
        rusty_fork::rusty_fork_test_name!(a_crash_ends_the_process_after_the_pages_before_it);
    rusty_fork::fork(
        test,
        rusty_fork::rusty_fork_id!(),
        |_| {},
        |child, output| {
            let (status, events) = ended(test, child, output);
            assert!(
                aborted(&status),
                "{test}: the child ended {status}, not by an abort"
            );
            written_before_sheet_2(test, &events);
        },
        || a_batch_ended_by(Fault::Crash),
    )
    .expect("the child could be started");
}

#[test]
fn an_exit_ends_the_process_with_its_code_after_the_pages_before_it() {
    let test = rusty_fork::rusty_fork_test_name!(
        an_exit_ends_the_process_with_its_code_after_the_pages_before_it
    );
    rusty_fork::fork(
        test,
        rusty_fork::rusty_fork_id!(),
        |_| {},
        |child, output| {
            let (status, events) = ended(test, child, output);
            assert_eq!(
                status.code(),
                Some(EXIT_CODE),
                "{test}: the child ended {status}"
            );
            written_before_sheet_2(test, &events);
        },
        || a_batch_ended_by(Fault::Exit(EXIT_CODE)),
    )
    .expect("the child could be started");
}
