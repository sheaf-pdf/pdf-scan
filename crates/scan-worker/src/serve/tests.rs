//! The worker's core driven over pipes, as a client would: requests written, its lines and a
//! page's bytes read back.

use std::{
    io::{BufRead, PipeReader, PipeWriter},
    num::NonZeroU32,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
        mpsc::SyncSender,
    },
    thread::JoinHandle,
};

use scan_device::fake::{FakeBackend, FakeDevice, Fault, Sheet, When, pattern};
use scan_device::in_progress::{Level, Progress};
use scan_device::{
    Caps, DeviceId, Done, Driver, Encoding, Format, Opened, Paper, Resolutions, Side, SideKnown,
    SizeMm, Source, SourceCaps, Status, WindowClosed,
};

use super::*;
use crate::wire::MessageLevel;

/// The longest a test waits for a line it expects: far past any it should take.
const AT_MOST: Duration = Duration::from_secs(10);

/// A line the worker wrote, as read, and a page's bytes after it.
#[derive(Debug)]
struct Got {
    line: String,
    reply: Reply,
    bytes: Vec<u8>,
}

/// A worker's core on a thread of its own, its input and output pipes the test's.
struct Piped {
    requests: Option<PipeWriter>,
    got: Option<Receiver<Got>>,
    core: Option<JoinHandle<Exit>>,
}

impl Piped {
    fn start(backends: Vec<Box<dyn Backend>>) -> Piped {
        let (input, requests) = io::pipe().expect("a pipe for the requests");
        let (replies, output) = io::pipe().expect("a pipe for the replies");
        let core = thread::spawn(move || run(backends, input, output, None));
        let (sent, got) = mpsc::channel();
        thread::spawn(move || read_replies(replies, &sent));
        Piped {
            requests: Some(requests),
            got: Some(got),
            core: Some(core),
        }
    }

    /// A worker of one fake backend with `devices`.
    fn fake(devices: impl IntoIterator<Item = FakeDevice>) -> Piped {
        let backend = devices
            .into_iter()
            .fold(FakeBackend::new(), FakeBackend::with);
        let worker = Piped::start(vec![Box::new(backend)]);
        worker.hello();
        worker
    }

    /// `bytes` written to the worker's input as they are.
    fn write(&mut self, bytes: &[u8]) {
        let requests = self.requests.as_mut().expect("the input open");
        requests.write_all(bytes).expect("the worker's input");
    }

    /// `line` and its `\n`.
    fn send(&mut self, line: &str) {
        self.write(format!("{line}\n").as_bytes());
    }

    fn request(&mut self, request: &Request) {
        self.write(&wire::line(request).unwrap());
    }

    /// The worker's next line, and a page's bytes after it.
    fn next(&self) -> Got {
        let got = self.got.as_ref().expect("the output read");
        match got.recv_timeout(AT_MOST) {
            Ok(got) => got,
            Err(RecvTimeoutError::Timeout) => panic!("no line from the worker in {AT_MOST:?}"),
            Err(RecvTimeoutError::Disconnected) => panic!("the worker's output ended"),
        }
    }

    fn reply(&self) -> Reply {
        self.next().reply
    }

    fn hello(&self) {
        let got = self.next();
        assert_eq!(
            got.line,
            concat!(
                r#"{"hello":{"protocol":1,"version":""#,
                env!("CARGO_PKG_VERSION"),
                r#""}}"#
            )
        );
    }

    /// The worker's lines up to and with the batch's end, `done` or `error`.
    fn batch(&self) -> Vec<Got> {
        let mut lines = Vec::new();
        loop {
            let got = self.next();
            let end = matches!(got.reply, Reply::Done(_) | Reply::Error(_));
            lines.push(got);
            if end {
                return lines;
            }
        }
    }

    /// The worker's lines up to and with the first `until` holds of.
    fn until(&self, until: impl Fn(&Reply) -> bool) -> Vec<Got> {
        let mut lines = Vec::new();
        loop {
            let got = self.next();
            let found = until(&got.reply);
            lines.push(got);
            if found {
                return lines;
            }
        }
    }

    /// The device `stored` opened, and the answer.
    fn open(&mut self, stored: &DeviceInfo) -> Reply {
        self.request(&Request::Open {
            device: stored.clone(),
            deadline: AT_MOST,
        });
        self.reply()
    }

    /// `scan` with `settings` from sheet 1.
    fn scan(&mut self, settings: &Settings) {
        self.request(&Request::Scan {
            settings: settings.clone(),
            first_sheet: 1,
        });
    }

    /// The input closed: the client gone.
    fn end_input(&mut self) {
        self.requests = None;
    }

    /// The worker's output ends: nothing more written.
    fn output_ends(&mut self) {
        let got = self.got.take().expect("the output read");
        match got.recv_timeout(AT_MOST) {
            Err(RecvTimeoutError::Disconnected) => {}
            Ok(got) => panic!(
                "the worker wrote {:?} where its output should end",
                got.line
            ),
            Err(RecvTimeoutError::Timeout) => panic!("the worker's output didn't end"),
        }
    }

    /// How the core ended.
    fn join(mut self) -> Exit {
        self.core
            .take()
            .expect("the core")
            .join()
            .expect("the core didn't panic")
    }

    /// How the core ended, once its output has.
    fn ended(mut self) -> Exit {
        self.output_ends();
        self.join()
    }
}

/// The worker's output read as a client would: each line, and a page's bytes after it.
fn read_replies(replies: PipeReader, sent: &Sender<Got>) {
    let mut replies = io::BufReader::new(replies);
    loop {
        let mut line = String::new();
        match replies.read_line(&mut line) {
            Ok(0) | Err(_) => return,
            Ok(_) => {}
        }
        assert_eq!(line.pop(), Some('\n'), "a line cut short: {line:?}");
        let reply: Reply = serde_json::from_str(&line)
            .unwrap_or_else(|error| panic!("an unreadable line {line:?}: {error}"));
        let bytes = match &reply {
            Reply::Page(page) => {
                let mut bytes = vec![0; usize::try_from(page.bytes).unwrap()];
                replies.read_exact(&mut bytes).expect("a page's bytes");
                bytes
            }
            _ => Vec::new(),
        };
        if sent.send(Got { line, reply, bytes }).is_err() {
            return;
        }
    }
}

fn settings(source: Source, dpi: u32, format: Format, duplex: bool) -> Settings {
    let mut settings = Settings::new(source, dpi, format);
    settings.duplex = duplex;
    settings
}

/// `n` plain sheets.
fn sheets(n: usize) -> Vec<Sheet> {
    vec![Sheet::new(); n]
}

/// The scanner most tests open: three sheets in its feeder.
fn scanner() -> FakeDevice {
    let scanner = FakeDevice::new("desk", "Maker", "Model");
    scanner.load(sheets(3));
    scanner
}

/// What a page's line and bytes say: its sheet and side, its format and size.
fn page_of(got: &Got) -> Option<(u32, Side, &PageHeader)> {
    match &got.reply {
        Reply::Page(page) => Some((page.sheet, page.side, page)),
        _ => None,
    }
}

/// The batch's lines but its progress and messages, in order, each by its kind: the events.
fn events(lines: &[Got]) -> Vec<String> {
    lines
        .iter()
        .filter_map(|got| match &got.reply {
            Reply::Applied(_) => Some("applied".to_owned()),
            Reply::Status(status) => Some(format!("status {status:?}")),
            Reply::Sheet(sheet) => Some(format!("sheet {sheet}")),
            Reply::Page(page) => Some(format!("page {} {:?}", page.sheet, page.side)),
            Reply::Done(done) => Some(format!("done {done:?}")),
            Reply::Error(error) => Some(format!("error {error:?}")),
            Reply::Progress { .. } | Reply::Message { .. } => None,
            other => Some(format!("{other:?}")),
        })
        .collect()
}

/// Each page of `lines` drawn by the fake's `pattern`, byte for byte, at its line's format and
/// size.
fn pages_are_pattern_s(lines: &[Got]) {
    for got in lines {
        let Some((sheet, side, page)) = page_of(got) else {
            continue;
        };
        let PixelsHeader::Raw {
            format,
            width,
            height,
        } = page.pixels
        else {
            panic!("a raw page: {}", got.line);
        };
        let drawn = pattern(format, width, height, sheet, side).unwrap();
        assert_eq!(page.bytes, drawn.data().len() as u64, "{}", got.line);
        assert!(got.bytes == drawn.data(), "sheet {sheet} {side:?}'s bytes");
    }
}

/// The batch's progress: each line within a page's size, `size`, and the last the whole of it.
fn progress_is_in_pages_of(lines: &[Got], size: u64) {
    let progress: Vec<(u64, Option<u64>)> = lines
        .iter()
        .filter_map(|got| match got.reply {
            Reply::Progress { done, total } => Some((done, total)),
            _ => None,
        })
        .collect();
    assert!(!progress.is_empty(), "no progress line");
    for &(done, total) in &progress {
        assert_eq!(total, Some(size));
        assert!(done <= size, "{done} of {size}");
    }
    assert_eq!(progress.last(), Some(&(size, Some(size))));
}

#[test]
fn hello_is_the_first_line() {
    let worker = Piped::start(Vec::new());
    worker.hello();
}

#[test]
fn a_listing_gives_the_devices_and_its_deadline_s_error() {
    let mut worker = Piped::fake([
        FakeDevice::new("desk", "Maker", "Model"),
        FakeDevice::new("libusb:001:004", "Other", "Flatbed"),
    ]);
    worker.send(r#"{"list":{"deadline":{"secs":5,"nanos":0}}}"#);
    let got = worker.next();
    assert_eq!(
        got.line,
        concat!(
            r#"{"devices":["#,
            r#"{"id":"fake:desk","name":"Maker Model","maker":"Maker","model":"Model","driver":"Fake"},"#,
            r#"{"id":"fake:libusb:001:004","name":"Other Flatbed","maker":"Other","model":"Flatbed","driver":"Fake"}"#,
            r#"]}"#
        )
    );

    let slow = FakeBackend::new().listing_takes(AT_MOST);
    let mut worker = Piped::start(vec![Box::new(slow)]);
    worker.hello();
    worker.send(r#"{"list":{"deadline":{"secs":0,"nanos":100000000}}}"#);
    let got = worker.next();
    assert_eq!(got.line, r#"{"error":"TimedOut"}"#);
    // And the worker goes on.
    worker.request(&Request::List {
        deadline: Duration::ZERO,
    });
    assert_eq!(worker.reply(), Reply::Error(ScanError::TimedOut));
}

#[test]
fn a_stored_device_opens_as_stored_or_renamed() {
    let desk = FakeDevice::new("desk", "Maker", "Model");
    let mut worker = Piped::fake([desk.clone()]);
    let stored = desk.info();
    assert_eq!(
        worker.open(&stored),
        Reply::Opened {
            info: stored,
            renamed_from: None,
        }
    );

    // Replugged: the stored USB name is another's now, the same scanner found under a new one.
    let replugged = FakeDevice::new("libusb:001:005", "Maker", "Model");
    let mut worker = Piped::fake([replugged.clone()]);
    let stored = FakeDevice::new("libusb:001:004", "Maker", "Model").info();
    assert_eq!(
        worker.open(&stored),
        Reply::Opened {
            info: replugged.info(),
            renamed_from: Some(stored.id),
        }
    );

    // Gone, refused and not this driver's: errors, and no device open after them.
    let mut worker =
        Piped::fake([FakeDevice::new("busy", "Maker", "Model").refuses_open(ScanError::Busy)]);
    let busy = FakeDevice::new("busy", "Maker", "Model").info();
    assert_eq!(worker.open(&busy), Reply::Error(ScanError::Busy));
    let mut gone = busy.clone();
    gone.id = DeviceId("fake:elsewhere".to_owned());
    assert_eq!(worker.open(&gone), Reply::Error(ScanError::Gone));
    let mut other = busy;
    other.id = DeviceId("sane:test:0".to_owned());
    assert_eq!(worker.open(&other), Reply::Error(ScanError::BadId));
    worker.request(&Request::Caps);
    assert!(matches!(worker.reply(), Reply::Refused { .. }));
}

#[test]
fn caps_and_the_settings_window_are_the_device_s() {
    let mut flatbed = SourceCaps::new(
        Source::Flatbed,
        Resolutions::Range {
            min: 75,
            max: 1200,
            step: NonZeroU32::new(75).unwrap(),
        },
        vec![Format::Gray8, Format::Rgb16],
    );
    flatbed.encodings = vec![Encoding::Jpeg];
    flatbed.max_area = Some(SizeMm::A4);
    let desk = FakeDevice::new("desk", "Maker", "Model")
        .caps(Caps::new(vec![flatbed.clone()]))
        .settings_window(WindowClosed::Dismissed);
    let mut worker = Piped::fake([desk.clone()]);
    worker.open(&desk.info());
    worker.send(r#""caps""#);
    let mut caps = Caps::new(vec![flatbed]);
    caps.settings_window = true;
    assert_eq!(worker.reply(), Reply::Caps(caps));
    worker.send(r#"{"settings_window":{"owner":4096}}"#);
    let got = worker.next();
    assert_eq!(got.line, r#"{"window":"Dismissed"}"#);

    // A device without one.
    let plain = FakeDevice::new("plain", "Maker", "Model");
    let mut worker = Piped::fake([plain.clone()]);
    worker.open(&plain.info());
    worker.request(&Request::SettingsWindow { owner: None });
    assert_eq!(worker.reply(), Reply::Error(ScanError::Unsupported));
}

#[test]
fn a_simplex_batch_s_events_come_in_order_its_pages_whole_and_drawn() {
    let desk = scanner();
    let mut worker = Piped::fake([desk.clone()]);
    worker.open(&desk.info());
    // Black and white Letter at 75 dpi: 637 pixels a row (215.9 mm is 637.4999 as an `f32`), its
    // last byte padded.
    worker.scan(&settings(Source::Feeder, 75, Format::Bw1, false));
    let lines = worker.batch();
    assert_eq!(
        events(&lines),
        [
            "applied",
            "sheet 1",
            "page 1 Front",
            "sheet 2",
            "page 2 Front",
            "sheet 3",
            "page 3 Front",
            "done FeederEmpty",
        ]
    );
    pages_are_pattern_s(&lines);
    let first = lines.iter().find_map(page_of).unwrap().2;
    assert_eq!(
        first.pixels,
        PixelsHeader::Raw {
            format: Format::Bw1,
            width: 637,
            height: 825,
        }
    );
    assert_eq!(first.dpi, (75.0, 75.0));
    assert_eq!(first.side_known, SideKnown::Inferred);
    progress_is_in_pages_of(&lines, 80 * 825);
    assert_eq!(desk.fed(), 3);
}

#[test]
fn a_duplex_batch_gives_each_sheet_s_front_then_back() {
    let desk = FakeDevice::new("desk", "Maker", "Model").reports_sides();
    desk.load(sheets(2));
    let mut worker = Piped::fake([desk.clone()]);
    worker.open(&desk.info());
    worker.scan(&settings(Source::Feeder, 100, Format::Rgb8, true));
    let lines = worker.batch();
    assert_eq!(
        events(&lines),
        [
            "applied",
            "sheet 1",
            "page 1 Front",
            "page 1 Back",
            "sheet 2",
            "page 2 Front",
            "page 2 Back",
            "done FeederEmpty",
        ]
    );
    pages_are_pattern_s(&lines);
    // Letter at 100 dpi: 850 by 1100, three bytes a pixel.
    progress_is_in_pages_of(&lines, 850 * 1100 * 3);
    for got in &lines {
        if let Some((_, _, page)) = page_of(got) {
            assert_eq!(page.side_known, SideKnown::Reported);
        }
    }
    // Nothing but the batch's lines: the next is the next call's answer.
    worker.request(&Request::Caps);
    assert!(matches!(worker.reply(), Reply::Caps(_)));
}

#[test]
fn applied_and_status_come_before_the_first_sheet_and_clamped_is_said() {
    let desk = FakeDevice::new("desk", "Maker", "Model").warms_up();
    desk.glass(Sheet::new());
    let mut worker = Piped::fake([desk.clone()]);
    worker.open(&desk.info());
    // The flatbed lists 75, 150, 300 and 600 dpi: 1000 becomes 600. An inch square keeps the
    // page small.
    let mut asked = settings(Source::Flatbed, 1000, Format::Gray8, false);
    asked.paper = Paper::Size(SizeMm {
        width: 25.4,
        height: 25.4,
    });
    worker.scan(&asked);
    let lines = worker.batch();
    assert_eq!(
        events(&lines),
        [
            "applied",
            "status WarmingUp",
            "sheet 1",
            "page 1 Front",
            "done OnePage",
        ]
    );
    let Reply::Applied(applied) = &lines[0].reply else {
        panic!("{}", lines[0].line);
    };
    assert_eq!(applied.settings.dpi, 600);
    assert_eq!(applied.clamped, [scan_device::Clamped::Dpi(1000)]);
    pages_are_pattern_s(&lines);
}

#[test]
fn an_encoded_page_s_bytes_follow_its_line_as_the_driver_gave_them() {
    let mut feeder = SourceCaps::new(
        Source::Feeder,
        Resolutions::List(vec![200]),
        vec![Format::Bw1, Format::Gray8],
    );
    feeder.encodings = vec![Encoding::CcittG4, Encoding::Png];
    let desk = FakeDevice::new("desk", "Maker", "Model").caps(Caps::new(vec![feeder]));
    desk.load(sheets(1));
    let mut worker = Piped::fake([desk.clone()]);
    worker.open(&desk.info());
    let mut asked = settings(Source::Feeder, 200, Format::Bw1, false);
    asked.encodings = vec![Encoding::CcittG4];
    worker.scan(&asked);
    let lines = worker.batch();
    let page = lines.iter().find(|got| page_of(got).is_some()).unwrap();
    assert_eq!(
        page.line,
        concat!(
            r#"{"page":{"pixels":{"encoded":{"kind":{"CcittG4":{"width":1700,"height":2200,"black_is_1":true}}}},"#,
            r#""dpi":[200.0,200.0],"sheet":1,"side":"Front","side_known":"Inferred","bytes":34}}"#
        )
    );
    assert_eq!(page.bytes, b"fake CCITT G4 page: sheet 1, front");
    assert_eq!(events(&lines).last().unwrap(), "done FeederEmpty");
}

#[test]
fn a_jam_s_error_comes_after_the_pages_before_it_and_the_worker_goes_on() {
    let desk = FakeDevice::new("desk", "Maker", "Model");
    desk.load([
        Sheet::new(),
        Sheet::new().fault(Side::Back, When::Midway, Fault::Error(ScanError::Jammed)),
        Sheet::new(),
    ]);
    let mut worker = Piped::fake([desk.clone()]);
    worker.open(&desk.info());
    let duplex = settings(Source::Feeder, 50, Format::Gray8, true);
    worker.scan(&duplex);
    let lines = worker.batch();
    assert_eq!(
        events(&lines),
        [
            "applied",
            "sheet 1",
            "page 1 Front",
            "page 1 Back",
            "sheet 2",
            "page 2 Front",
            "error Jammed",
        ]
    );
    assert_eq!(lines.last().unwrap().line, r#"{"error":"Jammed"}"#);
    pages_are_pattern_s(&lines);
    // Continue: the jammed sheet scanned again, and the rest.
    worker.request(&Request::Scan {
        settings: duplex,
        first_sheet: 2,
    });
    let lines = worker.batch();
    assert_eq!(
        events(&lines),
        [
            "applied",
            "sheet 2",
            "page 2 Front",
            "page 2 Back",
            "sheet 3",
            "page 3 Front",
            "page 3 Back",
            "done FeederEmpty",
        ]
    );
    pages_are_pattern_s(&lines);
}

/// A scanner whose sheet 2's front waits midway for the batch's cancel, and the worker that
/// opened it, its batch started and read up to that wait: the progress at half the page.
fn held_at_sheet_2(duplex: bool) -> (FakeDevice, Piped, Vec<Got>) {
    let desk = FakeDevice::new("desk", "Maker", "Model");
    desk.load([
        Sheet::new(),
        Sheet::new().fault(Side::Front, When::Midway, Fault::AwaitCancel),
        Sheet::new(),
    ]);
    let mut worker = Piped::fake([desk.clone()]);
    worker.open(&desk.info());
    worker.scan(&settings(Source::Feeder, 50, Format::Gray8, duplex));
    // Letter at 50 dpi: 425 by 550 in gray, its first 275 rows midway.
    let half = 425 * 275;
    let mut lines = worker.until(|reply| matches!(reply, Reply::Sheet(2)));
    lines.extend(
        worker.until(
            |reply| matches!(reply, Reply::Progress { done, total: Some(_) } if *done == half),
        ),
    );
    (desk, worker, lines)
}

#[test]
fn a_cancel_mid_sheet_finishes_it_and_feeds_no_more() {
    let (desk, mut worker, mut lines) = held_at_sheet_2(true);
    // Nothing comes while the side waits.
    let got = worker.got.as_ref().unwrap();
    assert!(matches!(
        got.recv_timeout(Duration::from_millis(300)),
        Err(RecvTimeoutError::Timeout)
    ));
    assert_eq!(desk.fed(), 2);
    worker.send(r#""cancel""#);
    lines.extend(worker.batch());
    assert_eq!(
        events(&lines),
        [
            "applied",
            "sheet 1",
            "page 1 Front",
            "page 1 Back",
            "sheet 2",
            "page 2 Front",
            "page 2 Back",
            "done Cancelled",
        ]
    );
    assert_eq!(lines.last().unwrap().line, r#"{"done":"Cancelled"}"#);
    pages_are_pattern_s(&lines);
    assert_eq!(desk.fed(), 2);
    assert_eq!(desk.in_feeder(), 1);
    // A cancel with no batch does nothing, and the next batch takes the sheet left.
    worker.send(r#""cancel""#);
    worker.scan(&settings(Source::Feeder, 50, Format::Gray8, false));
    let lines = worker.batch();
    assert_eq!(
        events(&lines),
        ["applied", "sheet 1", "page 1 Front", "done FeederEmpty"]
    );
}

#[test]
fn a_cancel_written_with_its_scan_is_never_missed() {
    // Each sheet's front waits for the cancel as it starts, so a cancel the worker missed holds
    // the batch there for good.
    let desk = FakeDevice::new("desk", "Maker", "Model");
    let waits = Sheet::new().fault(Side::Front, When::Start, Fault::AwaitCancel);
    desk.load(vec![waits; 20]);
    let mut worker = Piped::fake([desk.clone()]);
    worker.open(&desk.info());
    let scan = wire::line(&Request::Scan {
        settings: settings(Source::Feeder, 50, Format::Gray8, false),
        first_sheet: 1,
    })
    .unwrap();
    // Again and again, as each try is a race the worker must never lose.
    for _ in 0..20 {
        let fed = desk.fed();
        // One write: the worker reads the cancel as the batch starts.
        worker.write(&[scan.clone(), b"\"cancel\"\n".to_vec()].concat());
        let lines = worker.batch();
        let events = events(&lines);
        // Nearly always taken before the batch's first check, so no sheet is fed; where the
        // batch reaches that check first, the first sheet is, and finished.
        assert!(
            events == ["applied", "done Cancelled"]
                || events == ["applied", "sheet 1", "page 1 Front", "done Cancelled"],
            "{events:?}"
        );
        assert!(desk.fed() - fed <= 1, "{} fed", desk.fed() - fed);
    }
}

#[test]
fn a_request_out_of_its_state_is_refused_and_the_worker_goes_on() {
    let desk = scanner();
    let mut worker = Piped::fake([desk.clone()]);
    let refused = |reply: Reply, why: &str| match reply {
        Reply::Refused { reason } => assert!(reason.contains(why), "{reason}"),
        other => panic!("{other:?}"),
    };
    // No device open.
    worker.send(r#""caps""#);
    refused(worker.reply(), "no device is open");
    worker.scan(&settings(Source::Feeder, 50, Format::Gray8, false));
    refused(worker.reply(), "no device is open");
    worker.request(&Request::SettingsWindow { owner: None });
    refused(worker.reply(), "no device is open");
    assert_eq!(desk.fed(), 0);
    // One open: no listing, and no other.
    assert!(matches!(worker.open(&desk.info()), Reply::Opened { .. }));
    worker.request(&Request::List { deadline: AT_MOST });
    refused(worker.reply(), "a device is open");
    refused(worker.open(&desk.info()), "a device is open");
    worker.request(&Request::Caps);
    assert!(matches!(worker.reply(), Reply::Caps(_)));
}

#[test]
fn a_request_while_a_call_runs_is_refused_at_once() {
    let (desk, mut worker, _) = held_at_sheet_2(false);
    for request in [
        Request::Caps,
        Request::List { deadline: AT_MOST },
        Request::SettingsWindow { owner: None },
        Request::Scan {
            settings: settings(Source::Feeder, 50, Format::Gray8, false),
            first_sheet: 4,
        },
    ] {
        worker.request(&request);
        // Answered while the batch waits, the progress lines aside.
        let got = worker.until(|reply| !matches!(reply, Reply::Progress { .. }));
        match &got.last().unwrap().reply {
            Reply::Refused { reason } => assert!(reason.contains("a call is running"), "{reason}"),
            other => panic!("{request:?}: {other:?}"),
        }
    }
    worker.request(&Request::Cancel);
    let lines = worker.batch();
    assert_eq!(
        events(&lines),
        ["page 2 Front", "done Cancelled"],
        "the batch goes on"
    );
    assert_eq!(desk.fed(), 2);
}

#[test]
fn an_unreadable_line_is_refused_and_the_worker_goes_on() {
    let mut worker = Piped::fake([FakeDevice::new("desk", "Maker", "Model")]);
    for line in [
        "",
        "not json",
        r#""lis""#,
        r#"{"list":{}}"#,
        r#"{"list":{"deadline":{"secs":5,"nanos":0}}} {}"#,
    ] {
        worker.send(line);
        match worker.reply() {
            Reply::Refused { reason } => {
                assert!(
                    reason.starts_with("an unreadable request"),
                    "{line:?}: {reason}"
                );
            }
            other => panic!("{line:?}: {other:?}"),
        }
    }
    // Not UTF-8.
    worker.write(b"\"\xff\"\n");
    assert!(matches!(worker.reply(), Reply::Refused { .. }));
    worker.request(&Request::List { deadline: AT_MOST });
    assert!(matches!(worker.reply(), Reply::Devices(devices) if devices.len() == 1));
}

#[test]
fn a_line_past_the_cap_is_refused_unread_and_one_at_it_is_taken() {
    let mut worker = Piped::fake([FakeDevice::new("desk", "Maker", "Model")]);
    let list = r#"{"list":{"deadline":{"secs":5,"nanos":0}}}"#;
    // Spaces after the request, which JSON allows, to the cap and one past it.
    let at_cap = list.to_owned() + &" ".repeat(REQUEST_LIMIT - list.len());
    let past_cap = at_cap.clone() + " ";
    assert_eq!(at_cap.len(), REQUEST_LIMIT);
    worker.send(&past_cap);
    match worker.reply() {
        Reply::Refused { reason } => {
            assert_eq!(reason, "a line past the 1048576-byte cap");
        }
        other => panic!("{other:?}"),
    }
    worker.send(&at_cap);
    assert!(matches!(worker.reply(), Reply::Devices(devices) if devices.len() == 1));
    // A line far past the cap, refused once.
    worker.write(&vec![b'['; 4 * REQUEST_LIMIT]);
    worker.send("");
    assert!(matches!(worker.reply(), Reply::Refused { .. }));
    worker.request(&Request::List { deadline: AT_MOST });
    assert!(matches!(worker.reply(), Reply::Devices(_)));
}

#[test]
fn quit_ends_the_worker_once_its_driver_is_closed() {
    let closed = Arc::new(AtomicUsize::new(0));
    let mut worker = Piped::start(vec![Box::new(Talking {
        closed: Arc::clone(&closed),
    })]);
    worker.hello();
    worker.open(&Talking::info());
    worker.send(r#""quit""#);
    worker.output_ends();
    // The device and the backend dropped before the output ended.
    assert_eq!(closed.load(Ordering::SeqCst), 2);
    assert_eq!(worker.join(), Exit::Quit);

    // With nothing open.
    let mut worker = Piped::start(Vec::new());
    worker.hello();
    worker.request(&Request::Quit);
    assert_eq!(worker.ended(), Exit::Quit);
}

#[test]
fn quit_cancels_a_running_batch_first() {
    let (desk, mut worker, _) = held_at_sheet_2(true);
    worker.request(&Request::Quit);
    let lines = worker.batch();
    assert_eq!(
        events(&lines),
        ["page 2 Front", "page 2 Back", "done Cancelled"]
    );
    assert_eq!(worker.ended(), Exit::Quit);
    assert_eq!(desk.fed(), 2);
}

#[test]
fn the_input_s_end_cancels_a_running_batch_and_ends_the_worker() {
    let (desk, mut worker, _) = held_at_sheet_2(true);
    worker.end_input();
    let lines = worker.batch();
    assert_eq!(
        events(&lines),
        ["page 2 Front", "page 2 Back", "done Cancelled"]
    );
    assert_eq!(worker.ended(), Exit::InputEnded);
    assert_eq!(desk.fed(), 2);

    // Idle.
    let mut worker = Piped::fake([FakeDevice::new("desk", "Maker", "Model")]);
    worker.end_input();
    assert_eq!(worker.ended(), Exit::InputEnded);
}

#[test]
fn output_that_fails_ends_the_batch_at_its_next_event_and_the_worker_with_it() {
    let desk = FakeDevice::new("desk", "Maker", "Model");
    desk.load(sheets(50));
    let mut worker = Piped::fake([desk.clone()]);
    worker.open(&desk.info());
    worker.scan(&settings(Source::Feeder, 150, Format::Gray8, false));
    worker.until(|reply| matches!(reply, Reply::Sheet(2)));
    // The client gone: its reading ends, and the worker's next write fails.
    drop(worker.got.take());
    let core = worker.core.take().unwrap();
    let ended = core.join().expect("the core didn't panic");
    assert_eq!(ended, Exit::OutputFailed);
    assert!(desk.fed() < 50, "{} sheets fed", desk.fed());

    // Before its hello.
    let (input, _requests) = io::pipe().unwrap();
    let (replies, output) = io::pipe().unwrap();
    drop(replies);
    assert_eq!(run(Vec::new(), input, output, None), Exit::OutputFailed);
}

#[test]
fn a_driver_s_messages_and_its_progress_in_bytes_are_carried() {
    let mut worker = Piped::start(vec![Box::new(Talking {
        closed: Arc::default(),
    })]);
    worker.hello();
    worker.open(&Talking::info());
    worker.scan(&settings(Source::Flatbed, 300, Format::Gray8, false));
    let lines = worker.batch();
    let carried: Vec<&Reply> = lines.iter().map(|got| &got.reply).collect();
    assert!(
        carried.contains(&&Reply::Message {
            level: MessageLevel::Warn,
            text: "the lamp is warming".to_owned(),
        }),
        "{carried:?}"
    );
    assert!(carried.contains(&&Reply::Sheet(1)), "{carried:?}");
    let progress: Vec<_> = carried
        .iter()
        .filter(|reply| matches!(reply, Reply::Progress { .. }))
        .collect();
    assert_eq!(
        progress,
        [
            &&Reply::Progress {
                done: 0,
                total: Some(8),
            },
            &&Reply::Progress {
                done: 8,
                total: Some(8),
            },
        ]
    );
    // The last message, reported as the batch ends, read before its `done`.
    assert_eq!(
        lines[lines.len() - 2..]
            .iter()
            .map(|got| got.line.as_str())
            .collect::<Vec<_>>(),
        [
            r#"{"message":{"level":"error","text":"done, with a fault"}}"#,
            r#"{"done":"OnePage"}"#,
        ]
    );
}

/// An output's bytes, kept to read back.
#[derive(Clone, Default)]
struct Buffer(Arc<Mutex<Vec<u8>>>);

impl Write for Buffer {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        lock(&self.0).extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Buffer {
    /// The lines written since the last call.
    fn lines(&self) -> Vec<String> {
        let written = std::mem::take(&mut *lock(&self.0));
        String::from_utf8(written)
            .unwrap()
            .lines()
            .map(str::to_owned)
            .collect()
    }
}

#[test]
fn progress_is_carried_in_bytes_as_it_moves_and_the_latest_message_as_one_comes() {
    let buffer = Buffer::default();
    let out = Output::new(buffer.clone());
    let tracker = Tracker::new();
    let mut seen = Seen::default();
    let mut poll = || {
        seen.poll(&out, &tracker).unwrap();
        buffer.lines()
    };
    assert_eq!(poll(), [""; 0]);
    // A count in another unit than bytes isn't carried.
    tracker.set_total(Some(3), Unit::Count("pages"));
    tracker.set(1);
    assert_eq!(poll(), [""; 0]);
    // In bytes, it is, the count kept.
    tracker.set_total(Some(100), Unit::Bytes);
    assert_eq!(poll(), [r#"{"progress":{"done":1,"total":100}}"#]);
    assert_eq!(poll(), [""; 0]);
    tracker.set(100);
    assert_eq!(poll(), [r#"{"progress":{"done":100,"total":100}}"#]);
    tracker.set_total(None, Unit::Bytes);
    assert_eq!(poll(), [r#"{"progress":{"done":100,"total":null}}"#]);
    tracker.set(0);
    tracker.set_total(Some(20), Unit::Bytes);
    assert_eq!(poll(), [r#"{"progress":{"done":0,"total":20}}"#]);
    // The latest message alone, those before it since the last poll lost.
    tracker.message(Level::Info, "first");
    tracker.message(Level::Warn, "second");
    assert_eq!(poll(), [r#"{"message":{"level":"warn","text":"second"}}"#]);
    assert_eq!(poll(), [""; 0]);
    tracker.message(Level::Error, "third");
    tracker.inc(5);
    assert_eq!(
        poll(),
        [
            r#"{"progress":{"done":5,"total":20}}"#,
            r#"{"message":{"level":"error","text":"third"}}"#,
        ]
    );
    // And the same message again is a message.
    tracker.message(Level::Info, "third");
    assert_eq!(poll(), [r#"{"message":{"level":"info","text":"third"}}"#]);
}

#[test]
fn several_backends_list_in_turn_and_open_through_the_first_that_takes_the_id() {
    let desk = FakeDevice::new("desk", "Maker", "Model");
    let mut worker = Piped::start(vec![
        Box::new(Talking {
            closed: Arc::default(),
        }),
        Box::new(FakeBackend::new().with(desk.clone())),
    ]);
    worker.hello();
    worker.request(&Request::List { deadline: AT_MOST });
    assert_eq!(
        worker.reply(),
        Reply::Devices(vec![Talking::info(), desk.info()])
    );
    // The first takes only its own ids, answering `BadId` for the fake's.
    assert_eq!(
        worker.open(&desk.info()),
        Reply::Opened {
            info: desk.info(),
            renamed_from: None,
        }
    );
    let mut worker = Piped::start(vec![
        Box::new(Talking {
            closed: Arc::default(),
        }),
        Box::new(FakeBackend::new().with(desk.clone())),
    ]);
    worker.hello();
    assert!(matches!(
        worker.open(&Talking::info()),
        Reply::Opened { .. }
    ));
    worker.request(&Request::Caps);
    assert_eq!(worker.reply(), Reply::Caps(Talking::caps()));
    // An id neither forms.
    let mut worker = Piped::start(vec![
        Box::new(Talking {
            closed: Arc::default(),
        }),
        Box::new(FakeBackend::new().with(desk.clone())),
    ]);
    worker.hello();
    let mut stranger = desk.info();
    stranger.id = DeviceId("sane:test:0".to_owned());
    assert_eq!(worker.open(&stranger), Reply::Error(ScanError::BadId));

    // The first error ends a listing.
    let mut worker = Piped::start(vec![
        Box::new(FakeBackend::new().with(desk.clone())),
        Box::new(FakeBackend::new().not_installed()),
        Box::new(Talking {
            closed: Arc::default(),
        }),
    ]);
    worker.hello();
    worker.request(&Request::List { deadline: AT_MOST });
    assert!(matches!(
        worker.reply(),
        Reply::Error(ScanError::NotInstalled { .. })
    ));
}

#[test]
fn the_backends_share_the_deadline() {
    // Each takes 400 ms to list, under one deadline of 600: the second has 200 left.
    let listing = Duration::from_millis(400);
    let mut worker = Piped::start(vec![
        Box::new(FakeBackend::new().listing_takes(listing)),
        Box::new(FakeBackend::new().listing_takes(listing)),
    ]);
    worker.hello();
    let start = Instant::now();
    worker.request(&Request::List {
        deadline: Duration::from_millis(600),
    });
    assert_eq!(worker.reply(), Reply::Error(ScanError::TimedOut));
    assert!(start.elapsed() < Duration::from_millis(800) + AT_MOST / 10);
    // One alone has the time.
    let mut worker = Piped::start(vec![Box::new(FakeBackend::new().listing_takes(listing))]);
    worker.hello();
    worker.request(&Request::List {
        deadline: Duration::from_millis(600),
    });
    assert_eq!(worker.reply(), Reply::Devices(Vec::new()));
}

/// A backend of the tests' own, `talking:`: one device, whose batch reports messages and its
/// bytes, and which counts its drops and its device's; it answers `BadId` for any other id.
struct Talking {
    /// Its device's drop and its own, counted.
    closed: Arc<AtomicUsize>,
}

impl Talking {
    fn info() -> DeviceInfo {
        DeviceInfo::new(
            DeviceId("talking:one".to_owned()),
            "Talking".to_owned(),
            "Maker".to_owned(),
            "Talking".to_owned(),
            Driver::Fake,
        )
    }

    fn caps() -> Caps {
        Caps::new(vec![SourceCaps::new(
            Source::Flatbed,
            Resolutions::List(vec![300]),
            vec![Format::Gray8],
        )])
    }
}

impl Drop for Talking {
    fn drop(&mut self) {
        self.closed.fetch_add(1, Ordering::SeqCst);
    }
}

impl Backend for Talking {
    fn list(&mut self, _deadline: Duration) -> Result<Vec<DeviceInfo>, ScanError> {
        Ok(vec![Talking::info()])
    }

    fn open(&mut self, device: &DeviceInfo, _deadline: Duration) -> Result<Opened, ScanError> {
        if device.id != Talking::info().id {
            return Err(ScanError::BadId);
        }
        Ok(Opened {
            device: Box::new(TalkingDevice {
                closed: Arc::clone(&self.closed),
            }),
            info: Talking::info(),
            renamed_from: None,
        })
    }
}

/// [`Talking`]'s device.
struct TalkingDevice {
    closed: Arc<AtomicUsize>,
}

impl Drop for TalkingDevice {
    fn drop(&mut self) {
        self.closed.fetch_add(1, Ordering::SeqCst);
    }
}

impl Device for TalkingDevice {
    fn caps(&mut self) -> Result<Caps, ScanError> {
        Ok(Talking::caps())
    }

    /// Its total set before the first event, which the drain can't read before it's sent; its
    /// count set only once the second event is taken, so the drain has read the progress after
    /// the first.
    fn scan(
        &mut self,
        _settings: &Settings,
        first_sheet: u32,
        events: &SyncSender<Event>,
        progress: &dyn Progress,
    ) -> Result<Done, ScanError> {
        progress.set_total(Some(8), Unit::Bytes);
        progress.message(Level::Warn, "the lamp is warming");
        if events.send(Event::Sheet(first_sheet)).is_err() {
            return Ok(Done::Cancelled);
        }
        if events.send(Event::Status(Status::WarmingUp)).is_err() {
            return Ok(Done::Cancelled);
        }
        progress.set(8);
        progress.message(Level::Error, "done, with a fault");
        Ok(Done::OnePage)
    }

    fn settings_window(&mut self, _owner: Option<WindowOwner>) -> Result<WindowClosed, ScanError> {
        Err(ScanError::Unsupported)
    }
}
