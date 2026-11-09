//! Scanning against the fake libsane: what SANE's own backends can't do (a
//! feeder that ends where a test says, duplex pages, a jam or an open cover
//! at a page, Brother's `2500x0`, hostile frames, statuses and lengths, an
//! endless stream, `INVAL` from `sane_start` with options changed behind
//! it), and the fake's log checked for SANE's rules each time, `sane_cancel`
//! after every error and at every batch's end among them. Each test runs in
//! a child process of its own.

#[macro_use]
mod common;

use common::Fake;
use sane_dl::{
    Call, Error, FrameFormat, Image, LimitKind, Limits, Page, Parameters, Progress, ReadOutcome,
    Status,
};

/// The device every script lists.
const DEVICE: &str = "device fake:0 Fake Scanner flatbed\n";

fn script(lines: &str) -> String {
    format!("{DEVICE}{lines}")
}

fn image(page: Result<Page, Error>) -> Image {
    match page {
        Ok(Page::Image(image)) => image,
        other => panic!("not a page: {other:?}"),
    }
}

fn gray(bpl: i32, ppl: i32, lines: i32) -> Parameters {
    Parameters {
        format: FrameFormat::GRAY,
        last_frame: 1,
        bytes_per_line: bpl,
        pixels_per_line: ppl,
        lines,
        depth: 8,
    }
}

/// The calls on the handle once its options are read, but the reads (which
/// come as the fake's chunks fall).
fn scanning_calls(log: &common::FakeLog) -> Vec<&str> {
    log.calls_but_options()
        .into_iter()
        .filter(|call| !matches!(*call, "sane_init" | "sane_open" | "sane_exit"))
        .fold(Vec::new(), |mut calls, call| {
            if !(call == "sane_read" && calls.last() == Some(&"sane_read")) {
                calls.push(call);
            }
            calls
        })
}

#[test]
#[cfg_attr(not(unix), ignore = "loads the fake by SANE's name: none on Windows")]
fn a_feeder_gives_its_sheets_then_no_docs() {
    let fake = Fake::new(
        "a_feeder_gives_its_sheets_then_no_docs",
        &script("sheets 3\nframe GRAY 1 5 3 2 8\nread-max 4\n"),
    );
    in_child!(a_feeder_gives_its_sheets_then_no_docs, fake.child(), || {
        let sane = common::load_fake();
        let mut handle = sane.open("fake:0").unwrap();
        let mut batch = handle.batch();
        for page in 1..=3 {
            let mut heard = Vec::new();
            let image = image(batch.next_page(|progress| heard.push(progress)));
            assert_eq!(
                image,
                Image {
                    format: FrameFormat::GRAY,
                    depth: 8,
                    width: 3,
                    height: 2,
                    stride: 3,
                    // Each row's two bytes of padding dropped.
                    data: common::fake_pixels(page, false, 0, 3, 2),
                    frames: vec![gray(5, 3, 2)],
                }
            );
            let expected = Some(10);
            assert_eq!(
                heard,
                [
                    Progress {
                        bytes_read: 4,
                        bytes_expected: expected
                    },
                    Progress {
                        bytes_read: 8,
                        bytes_expected: expected
                    },
                    Progress {
                        bytes_read: 10,
                        bytes_expected: expected
                    },
                ]
            );
            assert_eq!(batch.pages(), page);
        }
        assert_eq!(
            batch.next_page(|_| {}).unwrap(),
            Page::NoDocs {
                from: Call::Start,
                pages: 3
            }
        );
    });
    let log = fake.log();
    log.assert_no_violations();
    let page = ["sane_start", "sane_get_parameters", "sane_read"];
    let mut expected: Vec<&str> = page.repeat(3);
    expected.extend(["sane_start", "sane_cancel", "sane_close"]);
    assert_eq!(scanning_calls(&log), expected);
    // Read blocking, as the device opened, without asking; the length asked
    // for the read chunk.
    assert_eq!(
        log.nth("call", "sane_read", 0).1.rest,
        "handle=0x5a00 max_length=262144"
    );
    assert_eq!(log.nth("return", "sane_start", 3).1.rest, "status=NO_DOCS");
}

#[test]
#[cfg_attr(not(unix), ignore = "loads the fake by SANE's name: none on Windows")]
fn a_backend_refusing_either_io_mode_scans_its_pages() {
    // fujitsu's profile answers UNSUPPORTED to either mode, as SANE 1.4.0's
    // fujitsu backend does.
    let fake = Fake::new(
        "a_backend_refusing_either_io_mode_scans_its_pages",
        &script("profile fujitsu\nsheets 3\n"),
    );
    in_child!(
        a_backend_refusing_either_io_mode_scans_its_pages,
        fake.child(),
        || {
            let sane = common::load_fake();
            let mut handle = sane.open("fake:0").unwrap();
            handle.start().unwrap();
            for non_blocking in [false, true] {
                match handle.set_io_mode(non_blocking) {
                    Err(Error::Status {
                        status: Status::UNSUPPORTED,
                        call: Call::SetIoMode,
                    }) => {}
                    other => panic!("{other:?}"),
                }
            }
            // That start took the first sheet; a batch takes the rest.
            handle.cancel();
            let mut batch = handle.batch();
            for page in 2..=3 {
                let image = image(batch.next_page(|_| {}));
                assert_eq!(image.data, common::fake_pixels(page, false, 0, 16, 4));
            }
            assert_eq!(
                batch.next_page(|_| {}).unwrap(),
                Page::NoDocs {
                    from: Call::Start,
                    pages: 2
                }
            );
        }
    );
    let log = fake.log();
    log.assert_no_violations();
    let modes = log
        .lines
        .iter()
        .filter(|line| line.kind == "call" && line.function == "sane_set_io_mode")
        .count();
    assert_eq!(modes, 2, "only the step-by-step calls set a mode");
}

#[test]
#[cfg_attr(not(unix), ignore = "loads the fake by SANE's name: none on Windows")]
fn a_page_sets_blocking_back_after_the_step_by_step_calls() {
    let fake = Fake::new(
        "a_page_sets_blocking_back_after_the_step_by_step_calls",
        &script("non-blocking\nsheets 3\n"),
    );
    in_child!(
        a_page_sets_blocking_back_after_the_step_by_step_calls,
        fake.child(),
        || {
            let sane = common::load_fake();
            let mut handle = sane.open("fake:0").unwrap();
            handle.start().unwrap();
            handle.set_io_mode(true).unwrap();
            handle.cancel();
            let mut batch = handle.batch();
            image(batch.next_page(|_| {}));
            image(batch.next_page(|_| {}));
        }
    );
    let log = fake.log();
    log.assert_no_violations();
    let modes: Vec<&str> = log
        .lines
        .iter()
        .filter(|line| line.kind == "call" && line.function == "sane_set_io_mode")
        .map(|line| line.rest.as_str())
        .collect();
    // Non-blocking by the step-by-step call, blocking again for the first
    // page, and the second page's start left as it is.
    assert_eq!(
        modes,
        [
            "handle=0x5a00 non_blocking=1",
            "handle=0x5a00 non_blocking=0"
        ]
    );
    let page = [
        "sane_start",
        "sane_set_io_mode",
        "sane_get_parameters",
        "sane_read",
    ];
    let calls = scanning_calls(&log);
    assert_eq!(&calls[3..7], page, "{calls:?}");
}

#[test]
#[cfg_attr(not(unix), ignore = "loads the fake by SANE's name: none on Windows")]
fn a_batch_dropped_after_a_page_ends_with_a_cancel() {
    let fake = Fake::new("a_batch_dropped_after_a_page_ends_with_a_cancel", DEVICE);
    in_child!(
        a_batch_dropped_after_a_page_ends_with_a_cancel,
        fake.child(),
        || {
            let sane = common::load_fake();
            let mut handle = sane.open("fake:0").unwrap();
            let mut batch = handle.batch();
            let first = image(batch.next_page(|_| {}));
            assert_eq!(first.data, common::fake_pixels(1, false, 0, 16, 4));
            drop(batch);
            // A new batch takes the feeder's next page.
            let second = image(handle.batch().next_page(|_| {}));
            assert_eq!(second.data, common::fake_pixels(2, false, 0, 16, 4));
            handle.close();
            // A handle closed with a frame started and no batch ended ends
            // it.
            let mut handle = sane.open("fake:0").unwrap();
            handle.start().unwrap();
            handle.close();
        }
    );
    let log = fake.log();
    log.assert_no_violations();
    let page = ["sane_start", "sane_get_parameters", "sane_read"];
    let mut expected: Vec<&str> = Vec::new();
    for _ in 0..2 {
        expected.extend(page);
        expected.push("sane_cancel");
    }
    expected.extend(["sane_close", "sane_start", "sane_cancel", "sane_close"]);
    assert_eq!(scanning_calls(&log), expected);
}

#[test]
#[cfg_attr(not(unix), ignore = "loads the fake by SANE's name: none on Windows")]
fn duplex_pages_are_marked_front_then_back() {
    let fake = Fake::new(
        "duplex_pages_are_marked_front_then_back",
        &script("sheets 2\nduplex\nframe GRAY 1 4 4 3 8\n"),
    );
    in_child!(
        duplex_pages_are_marked_front_then_back,
        fake.child(),
        || {
            let sane = common::load_fake();
            let mut handle = sane.open("fake:0").unwrap();
            let mut batch = handle.batch();
            let mut marks = Vec::new();
            for page in 1..=4 {
                let image = image(batch.next_page(|_| {}));
                let back = page % 2 == 0;
                assert_eq!(image.data, common::fake_pixels(page, back, 0, 4, 3));
                marks.push(image.data[0]);
            }
            assert_eq!(marks, b"FBFB");
            assert_eq!(
                batch.next_page(|_| {}).unwrap(),
                Page::NoDocs {
                    from: Call::Start,
                    pages: 4
                }
            );
        }
    );
    fake.log().assert_no_violations();
}

#[test]
#[cfg_attr(not(unix), ignore = "loads the fake by SANE's name: none on Windows")]
fn a_jam_or_an_open_cover_is_an_error_saying_its_call() {
    let fake = Fake::new(
        "a_jam_or_an_open_cover_is_an_error_saying_its_call",
        &script("at 2 start JAMMED\nat 3 read COVER_OPEN 10\nsheets 4\n"),
    );
    in_child!(
        a_jam_or_an_open_cover_is_an_error_saying_its_call,
        fake.child(),
        || {
            let sane = common::load_fake();
            let mut handle = sane.open("fake:0").unwrap();
            let mut batch = handle.batch();
            image(batch.next_page(|_| {}));
            match batch.next_page(|_| {}) {
                Err(Error::Status {
                    status: Status::JAMMED,
                    call: Call::Start,
                }) => {}
                other => panic!("{other:?}"),
            }
            // Mid-page, the page in flight is dropped.
            let mut heard = 0;
            match batch.next_page(|_| heard += 1) {
                Err(Error::Status {
                    status: Status::COVER_OPEN,
                    call: Call::Read,
                }) => {}
                other => panic!("{other:?}"),
            }
            assert_eq!(heard, 1);
            // The batch may go on, once the caller says: the next sheet.
            let last = image(batch.next_page(|_| {}));
            assert_eq!(last.data, common::fake_pixels(4, false, 0, 16, 4));
            assert_eq!(batch.pages(), 2);
        }
    );
    let log = fake.log();
    log.assert_no_violations();
    let calls = scanning_calls(&log);
    let page = ["sane_start", "sane_get_parameters", "sane_read"];
    let mut expected: Vec<&str> = page.to_vec();
    expected.extend(["sane_start", "sane_cancel"]);
    expected.extend(page);
    expected.push("sane_cancel");
    expected.extend(page);
    expected.extend(["sane_cancel", "sane_close"]);
    assert_eq!(calls, expected);
}

#[test]
#[cfg_attr(not(unix), ignore = "loads the fake by SANE's name: none on Windows")]
fn no_docs_from_a_pages_first_read_ends_the_batch_and_later_is_an_error() {
    let fake = Fake::new(
        "no_docs_from_a_pages_first_read_ends_the_batch_and_later_is_an_error",
        &script("at 2 read NO_DOCS 0\nat 3 read NO_DOCS 5\n"),
    );
    in_child!(
        no_docs_from_a_pages_first_read_ends_the_batch_and_later_is_an_error,
        fake.child(),
        || {
            let sane = common::load_fake();
            let mut handle = sane.open("fake:0").unwrap();
            let mut batch = handle.batch();
            image(batch.next_page(|_| {}));
            assert_eq!(
                batch.next_page(|_| {}).unwrap(),
                Page::NoDocs {
                    from: Call::Read,
                    pages: 1
                }
            );
            match batch.next_page(|_| {}) {
                Err(Error::Status {
                    status: Status::NO_DOCS,
                    call: Call::Read,
                }) => {}
                other => panic!("{other:?}"),
            }
        }
    );
    fake.log().assert_no_violations();
}

#[test]
#[cfg_attr(not(unix), ignore = "loads the fake by SANE's name: none on Windows")]
fn inval_from_start_reads_the_descriptors_again() {
    let script = script(
        "option page-height FIXED 4 soft-select+soft-detect\nrange 0 300 0\noption mode STRING 8 soft-select+soft-detect\nvalue Gray\nat 1 start INVAL\nstart-inval-deactivates page-height\n",
    );
    let fake = Fake::new("inval_from_start_reads_the_descriptors_again", &script);
    in_child!(
        inval_from_start_reads_the_descriptors_again,
        fake.child(),
        || {
            let sane = common::load_fake();
            let mut handle = sane.open("fake:0").unwrap();
            assert!(
                handle
                    .find_option("page-height")
                    .unwrap()
                    .capabilities
                    .is_active()
            );
            match handle.batch().next_page(|_| {}) {
                Err(Error::StartInvalid { reload: None }) => {}
                other => panic!("{other:?}"),
            }
            assert!(
                !handle
                    .find_option("page-height")
                    .unwrap()
                    .capabilities
                    .is_active()
            );
            let mode = handle.find_option("mode").unwrap().index;
            assert_eq!(
                handle.get(mode).unwrap(),
                sane_dl::Value::String("Gray".into())
            );
            // The next start is the next page.
            image(handle.batch().next_page(|_| {}));
        }
    );
    let log = fake.log();
    log.assert_no_violations();
    let calls = log.calls();
    let inval = log.nth("return", "sane_start", 0);
    assert_eq!(inval.1.rest, "status=INVAL");
    // A cancel, then every descriptor read again, the count among them.
    let after: Vec<&str> = calls
        .iter()
        .copied()
        .skip_while(|call| *call != "sane_start")
        .skip(1)
        .take(5)
        .collect();
    assert_eq!(
        after,
        [
            "sane_cancel",
            "sane_get_option_descriptor",
            "sane_control_option",
            "sane_get_option_descriptor",
            "sane_get_option_descriptor"
        ]
    );
}

#[test]
#[cfg_attr(not(unix), ignore = "loads the fake by SANE's name: none on Windows")]
fn the_step_by_step_calls_say_what_sane_says() {
    let fake = Fake::new(
        "the_step_by_step_calls_say_what_sane_says",
        &script("frame GRAY 1 3 3 2 8\nread-max 4\n"),
    );
    in_child!(
        the_step_by_step_calls_say_what_sane_says,
        fake.child(),
        || {
            let sane = common::load_fake();
            let mut handle = sane.open("fake:0").unwrap();
            // An estimate before the start, the frame's own after.
            assert_eq!(handle.parameters().unwrap(), gray(3, 3, 2));
            // A frame must have started.
            match handle.set_io_mode(false) {
                Err(Error::Status {
                    status: Status::INVAL,
                    call: Call::SetIoMode,
                }) => {}
                other => panic!("{other:?}"),
            }
            handle.start().unwrap();
            assert_eq!(handle.parameters().unwrap(), gray(3, 3, 2));
            match handle.set_io_mode(true) {
                Err(Error::Status {
                    status: Status::UNSUPPORTED,
                    call: Call::SetIoMode,
                }) => {}
                other => panic!("{other:?}"),
            }
            handle.set_io_mode(false).unwrap();
            #[cfg(unix)]
            match handle.select_fd() {
                Err(Error::Status {
                    status: Status::UNSUPPORTED,
                    call: Call::GetSelectFd,
                }) => {}
                other => panic!("{other:?}"),
            }
            let mut buffer = [0; 100];
            let mut bytes = Vec::new();
            // An empty buffer reads nothing and calls nothing.
            assert_eq!(handle.read(&mut []).unwrap(), ReadOutcome::Bytes(0));
            // A buffer past the read chunk is asked to the chunk.
            let mut large = vec![0; 1 << 20];
            let ReadOutcome::Bytes(first) = handle.read(&mut large).unwrap() else {
                panic!()
            };
            bytes.extend_from_slice(&large[..first]);
            while let ReadOutcome::Bytes(length) = handle.read(&mut buffer).unwrap() {
                bytes.extend_from_slice(&buffer[..length]);
            }
            // The step-by-step reads keep the padding: they're SANE's bytes.
            assert_eq!(bytes, common::fake_pixels(1, false, 0, 3, 2));
            assert_eq!(handle.read(&mut buffer).unwrap(), ReadOutcome::Eof);
            handle.cancel();
        }
    );
    let log = fake.log();
    log.assert_no_violations();
    assert_eq!(
        scanning_calls(&log),
        [
            "sane_get_parameters",
            "sane_set_io_mode",
            "sane_start",
            "sane_get_parameters",
            "sane_set_io_mode",
            "sane_set_io_mode",
            "sane_get_select_fd",
            "sane_read",
            "sane_cancel",
            "sane_close"
        ]
        .into_iter()
        .filter(|call| cfg!(unix) || *call != "sane_get_select_fd")
        .collect::<Vec<_>>()
    );
    assert_eq!(
        log.nth("call", "sane_read", 0).1.rest,
        "handle=0x5a00 max_length=262144"
    );
    assert_eq!(
        log.nth("call", "sane_read", 1).1.rest,
        "handle=0x5a00 max_length=100"
    );
}

#[cfg(unix)]
#[test]
fn a_select_fd_is_given() {
    let fake = Fake::new(
        "a_select_fd_is_given",
        &script("select-fd 5\nnon-blocking\n"),
    );
    in_child!(a_select_fd_is_given, fake.child(), || {
        let sane = common::load_fake();
        let mut handle = sane.open("fake:0").unwrap();
        handle.start().unwrap();
        handle.set_io_mode(true).unwrap();
        assert_eq!(handle.select_fd().unwrap(), 5);
        handle.cancel();
    });
    fake.log().assert_no_violations();
}

#[cfg(unix)]
#[test]
fn a_negative_select_fd_is_refused() {
    let fake = Fake::new("a_negative_select_fd_is_refused", &script("select-fd -1\n"));
    in_child!(a_negative_select_fd_is_refused, fake.child(), || {
        let sane = common::load_fake();
        let mut handle = sane.open("fake:0").unwrap();
        handle.start().unwrap();
        match handle.select_fd() {
            Err(Error::BadSelectFd { fd: -1 }) => {}
            other => panic!("{other:?}"),
        }
        handle.cancel();
    });
    fake.log().assert_no_violations();
}

#[test]
#[cfg_attr(not(unix), ignore = "loads the fake by SANE's name: none on Windows")]
fn an_endless_stream_stops_at_a_small_cap() {
    let fake = Fake::new(
        "an_endless_stream_stops_at_a_small_cap",
        &script("frame GRAY 1 4096 4096 -1 8 endless\n"),
    );
    in_child!(an_endless_stream_stops_at_a_small_cap, fake.child(), || {
        let sane = common::load_fake();
        let limit = 8 << 20;
        let mut limits = Limits::default();
        limits.frame_bytes = limit;
        sane.set_limits(limits);
        let mut handle = sane.open("fake:0").unwrap();
        match handle.batch().next_page(|_| {}) {
            Err(Error::LimitExceeded {
                limit: LimitKind::FrameBytes,
                value,
                cap,
            }) if cap == limit && value > limit as u64 => {}
            other => panic!("{other:?}"),
        }
        // The frame's rows, and no more, were held.
        if let Some(peak) = common::peak_memory() {
            assert!(peak < 64 << 20, "the process held {peak} bytes");
        }
    });
    let log = fake.log();
    log.assert_no_violations();
    let calls = scanning_calls(&log);
    assert_eq!(&calls[calls.len() - 2..], ["sane_cancel", "sane_close"]);
    // 32 reads of the 256 KiB chunk fill the cap; the next is refused.
    let reads = log
        .lines
        .iter()
        .filter(|line| line.kind == "call" && line.function == "sane_read")
        .count();
    assert_eq!(reads, 33);
}

#[test]
#[cfg_attr(not(unix), ignore = "loads the fake by SANE's name: none on Windows")]
fn three_pass_frames_from_the_fake_are_interleaved() {
    let fake = Fake::new(
        "three_pass_frames_from_the_fake_are_interleaved",
        &script("frame GREEN 0 3 2 2 8\nframe BLUE 0 3 2 2 8\nframe RED 1 3 2 2 8\n"),
    );
    in_child!(
        three_pass_frames_from_the_fake_are_interleaved,
        fake.child(),
        || {
            let sane = common::load_fake();
            let mut handle = sane.open("fake:0").unwrap();
            let mut heard = Vec::new();
            let image = image(handle.batch().next_page(|progress| heard.push(progress)));
            let plane = |frame| common::fake_pixels(1, false, frame, 2, 2);
            let (green, blue, red) = (plane(0), plane(1), plane(2));
            let mut expected = Vec::new();
            for at in 0..4 {
                expected.extend([red[at], green[at], blue[at]]);
            }
            assert_eq!(image.data, expected);
            assert_eq!(
                (image.format, image.stride, image.height),
                (FrameFormat::RGB, 6, 2)
            );
            let formats: Vec<FrameFormat> = image.frames.iter().map(|frame| frame.format).collect();
            assert_eq!(
                formats,
                [FrameFormat::GREEN, FrameFormat::BLUE, FrameFormat::RED]
            );
            assert_eq!(
                heard.last(),
                Some(&Progress {
                    bytes_read: 18,
                    bytes_expected: Some(18)
                })
            );
        }
    );
    fake.log().assert_no_violations();
}

/// A scan whose page fails, its error checked by `$check`; the fake's log
/// holding no violation, and a `sane_cancel` after the failure.
macro_rules! failing_page {
    ($name:ident, $lines:expr, $check:expr) => {
        #[test]
        #[cfg_attr(not(unix), ignore = "loads the fake by SANE's name: none on Windows")]
        fn $name() {
            let fake = Fake::new(stringify!($name), &script($lines));
            in_child!($name, fake.child(), || {
                let sane = common::load_fake();
                let mut handle = sane.open("fake:0").unwrap();
                let failed = handle.batch().next_page(|_| {});
                let check: fn(&Error) -> bool = $check;
                match failed {
                    Err(error) if check(&error) => {}
                    Err(error) => panic!("not the error expected: {error:?}"),
                    Ok(page) => panic!("not an error: {page:?}"),
                }
            });
            let log = fake.log();
            log.assert_no_violations();
            let calls = scanning_calls(&log);
            assert_eq!(
                &calls[calls.len() - 2..],
                ["sane_cancel", "sane_close"],
                "{calls:?}"
            );
        }
    };
}

/// Whether an error is `BadParameters` for these parameters, its reason
/// starting `reason`.
fn bad(error: &Error, parameters: (i32, i32, i32, i32, i32, i32), reason: &str) -> bool {
    let (format, last, bpl, ppl, lines, depth) = parameters;
    match error {
        Error::BadParameters {
            parameters: given,
            reason: said,
        } => {
            *given
                == Parameters {
                    format: FrameFormat(format),
                    last_frame: last,
                    bytes_per_line: bpl,
                    pixels_per_line: ppl,
                    lines,
                    depth,
                }
                && said.starts_with(reason)
        }
        _ => false,
    }
}

failing_page!(
    brothers_2500x0_is_bad_parameters,
    "frame GRAY 1 2500 2500 0 8\n",
    |error| bad(error, (0, 1, 2500, 2500, 0, 8), "it has 0 lines")
);
failing_page!(
    an_unknown_frame_format_is_bad_parameters,
    "frame 7 1 16 16 4 8\n",
    |error| bad(error, (7, 1, 16, 16, 4, 8), "its format, 7")
);
failing_page!(
    a_reserved_frame_format_is_bad_parameters,
    "frame 11 1 16 16 4 8\n",
    |error| bad(
        error,
        (11, 1, 16, 16, 4, 8),
        "its format, JPEG (11, reserved), is reserved"
    )
);
failing_page!(
    a_last_frame_of_2_is_bad_parameters,
    "frame GRAY 2 16 16 4 8\n",
    |error| bad(error, (0, 2, 16, 16, 4, 8), "its last_frame, 2")
);
failing_page!(
    a_depth_of_0_is_bad_parameters,
    "frame GRAY 1 16 16 4 0\n",
    |error| bad(error, (0, 1, 16, 16, 4, 0), "its depth, 0")
);
failing_page!(
    a_depth_of_3_is_bad_parameters,
    "frame GRAY 1 16 16 4 3\n",
    |error| bad(error, (0, 1, 16, 16, 4, 3), "its depth, 3")
);
failing_page!(
    a_depth_of_32_is_bad_parameters,
    "frame GRAY 1 64 16 4 32\n",
    |error| bad(error, (0, 1, 64, 16, 4, 32), "its depth, 32")
);
failing_page!(
    too_few_bytes_a_line_is_bad_parameters,
    "frame RGB 1 47 16 4 8\n",
    |error| bad(error, (1, 1, 47, 16, 4, 8), "it has 47 bytes a line, fewer")
);
failing_page!(
    negative_bytes_a_line_is_bad_parameters,
    "frame GRAY 1 -16 16 4 8\n",
    |error| bad(error, (0, 1, -16, 16, 4, 8), "it has -16 bytes a line")
);
failing_page!(
    lines_of_minus_2_is_bad_parameters,
    "frame GRAY 1 16 16 -2 8\n",
    |error| bad(error, (0, 1, 16, 16, -2, 8), "it has -2 lines")
);
failing_page!(
    lines_past_the_cap_are_a_cap_exceeded,
    "frame GRAY 1 16 16 70000000 8\n",
    |error| matches!(
        error,
        Error::LimitExceeded {
            limit: LimitKind::FrameBytes,
            value: 1_120_000_000,
            cap: 1_073_741_824
        }
    )
);
failing_page!(
    a_gray_frame_that_isnt_last_is_bad_parameters,
    "frame GRAY 0 16 16 4 8\n",
    |error| bad(
        error,
        (0, 0, 16, 16, 4, 8),
        "a GRAY (0) frame is a whole image"
    )
);
failing_page!(
    a_three_pass_image_missing_a_channel_is_bad_parameters,
    "frame RED 0 16 16 4 8\nframe GREEN 1 16 16 4 8\n",
    |error| bad(
        error,
        (3, 1, 16, 16, 4, 8),
        "the image ends with 2 of its three channels"
    )
);
failing_page!(
    a_three_pass_channel_twice_is_bad_parameters,
    "frame RED 0 16 16 4 8\nframe RED 0 16 16 4 8\nframe BLUE 1 16 16 4 8\n",
    |error| bad(
        error,
        (2, 0, 16, 16, 4, 8),
        "its channel, RED (2), came already"
    )
);
failing_page!(
    three_pass_frames_of_other_sizes_are_bad_parameters,
    "frame RED 0 16 16 4 8\nframe GREEN 0 16 16 5 8\nframe BLUE 1 16 16 4 8\n",
    |error| bad(error, (3, 0, 16, 16, 5, 8), "its size or depth differs")
);
failing_page!(
    a_gray_frame_after_a_single_channel_one_is_bad_parameters,
    "frame RED 0 16 16 4 8\nframe GRAY 1 16 16 4 8\n",
    |error| bad(
        error,
        (0, 1, 16, 16, 4, 8),
        "a GRAY (0) frame follows a single-channel one"
    )
);
failing_page!(
    a_short_frame_is_refused_with_its_counts,
    "frame GRAY 1 16 16 4 8 60\n",
    |error| matches!(
        error,
        Error::ShortFrame {
            expected: 64,
            read: 60
        }
    )
);
failing_page!(
    a_long_frame_is_refused_where_it_runs_past,
    "frame GRAY 1 16 16 4 8 65\n",
    |error| matches!(error, Error::LongFrame { expected: 64 })
);
failing_page!(
    a_partial_last_row_of_unknown_height_is_refused,
    "frame GRAY 1 16 16 -1 8 70\n",
    |error| matches!(
        error,
        Error::PartialRow {
            rows: 4,
            extra: 6,
            bytes_per_line: 16
        }
    )
);
failing_page!(
    an_unknown_height_frame_with_no_rows_is_short,
    "frame GRAY 1 16 16 -1 8 0\n",
    |error| matches!(
        error,
        Error::ShortFrame {
            expected: 16,
            read: 0
        }
    )
);
failing_page!(
    three_passes_of_unknown_height_with_no_rows_are_short,
    "frame RED 0 4 4 -1 8 0\nframe GREEN 0 4 4 -1 8 0\nframe BLUE 1 4 4 -1 8 0\n",
    |error| matches!(
        error,
        Error::ShortFrame {
            expected: 4,
            read: 0
        }
    )
);
failing_page!(
    a_read_length_past_what_was_asked_is_refused,
    "read-length past\n",
    |error| matches!(
        error,
        Error::BadReadLength {
            length: 262_145,
            asked: 262_144
        }
    )
);
failing_page!(
    a_read_length_below_zero_is_refused,
    "read-length -1\n",
    |error| matches!(error, Error::BadReadLength { length: -1, .. })
);
failing_page!(
    an_unknown_status_from_start_is_kept,
    "at 1 start 42\n",
    |error| matches!(
        error,
        Error::Status {
            status: Status(42),
            call: Call::Start
        }
    )
);
failing_page!(
    a_negative_status_from_start_is_kept,
    "at 1 start -1\n",
    |error| matches!(
        error,
        Error::Status {
            status: Status(-1),
            call: Call::Start
        }
    )
);
failing_page!(
    a_reserved_status_from_start_is_kept,
    "at 1 start WARMING_UP\n",
    |error| matches!(
        error,
        Error::Status {
            status: Status::WARMING_UP,
            call: Call::Start
        }
    )
);
failing_page!(
    an_unknown_status_from_read_is_kept,
    "at 1 read 42 10\n",
    |error| matches!(
        error,
        Error::Status {
            status: Status(42),
            call: Call::Read
        }
    )
);
failing_page!(
    a_reserved_status_from_read_is_kept,
    "at 1 read HW_LOCKED 0\n",
    |error| matches!(
        error,
        Error::Status {
            status: Status::HW_LOCKED,
            call: Call::Read
        }
    )
);
failing_page!(
    a_negative_status_from_read_is_kept,
    "at 1 read -1 0\n",
    |error| matches!(
        error,
        Error::Status {
            status: Status(-1),
            call: Call::Read
        }
    )
);
failing_page!(
    a_status_from_get_parameters_is_said,
    "parameters-status IO_ERROR\n",
    |error| matches!(
        error,
        Error::Status {
            status: Status::IO_ERROR,
            call: Call::GetParameters
        }
    )
);
