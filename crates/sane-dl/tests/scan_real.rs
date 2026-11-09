//! Scanning through SANE's own `test` backend: its feeder's ten sheets and
//! then `NO_DOCS`; gray and color at 1, 8 and 16 bits; a hand scanner's
//! unknown height, alone and from the feeder; short reads; three-pass color
//! in every order; padded rows (`ppl-loss`); swapped 16-bit samples
//! (`invert-endianess`); every status forced from `sane_read`, read step by
//! step;
//! non-blocking reads with a select fd; and estimated parameters before the
//! start. Each test runs in a child process whose configuration is a
//! private, isolated directory, never the system's; where there's no
//! libsane each says it skipped, and with `SANE_DL_REQUIRE_LIBSANE=1` fails
//! instead.

#[macro_use]
mod common;

use sane_dl::{
    Call, Error, Fixed, FrameFormat, Handle, Image, Page, Parameters, Progress, ReadOutcome, Sane,
    Status, Value,
};

/// An option's number, by its name.
fn index(handle: &Handle, name: &str) -> usize {
    handle
        .find_option(name)
        .unwrap_or_else(|| panic!("no option {name}"))
        .index
}

fn set(handle: &mut Handle, name: &str, value: Value) {
    let option = index(handle, name);
    handle
        .set(option, &value)
        .unwrap_or_else(|error| panic!("{name} = {value:?}: {}", common::chain(&error)));
}

fn text(handle: &mut Handle, name: &str, value: &str) {
    set(handle, name, Value::String(value.into()));
}

fn on(handle: &mut Handle, name: &str) {
    set(handle, name, Value::Bool(true));
}

/// `test:0` at 10 dpi: 31 pixels by 39 lines for its default 80 by 100 mm,
/// and 43 by 66 as a hand scanner (110 by 170 mm); its reads limited to 64
/// bytes.
///
/// The limit, and [`paced`], keep clear of a race in the backend (1.4.0,
/// built with threads): each frame's picture is written into a pipe by a
/// thread of its own, which the read that reaches the frame's end cancels,
/// asynchronously. If that read comes as the thread is freeing its buffer or
/// exiting, the cancel can leave the allocator's lock held, and the next
/// frame's thread blocks on it for good (seen 4 times in 72,000 pages read
/// whole, from 24 processes at once). The pictures here fit the pipe, so the
/// thread writes once and exits; read 64 bytes at a time, with a pause after
/// each frame's first read, the frame's end comes long after (none in
/// 144,000 pages).
fn open(sane: &Sane) -> Handle {
    let mut handle = sane.open("test:0").unwrap();
    // The backend opens with 50/65536 dpi (a bug of 1.4.0), so it's set.
    set(
        &mut handle,
        "resolution",
        Value::Fixed(vec![Fixed::from_f64(10.0).unwrap()]),
    );
    on(&mut handle, "read-limit");
    set(&mut handle, "read-limit-size", Value::Int(vec![64]));
    handle
}

/// A page's progress, pausing a moment after each frame's first read, so
/// the backend's reader thread has exited before the frame's end (see
/// [`open`]): `frame_bytes`, if given, says where each frame begins.
fn paced(frame_bytes: Option<u64>) -> impl FnMut(Progress) {
    let mut last = None;
    move |progress: Progress| {
        let frame = frame_bytes.map_or(0, |bytes| (progress.bytes_read - 1) / bytes);
        if last != Some(frame) {
            last = Some(frame);
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    }
}

fn image(page: Result<Page, Error>) -> Image {
    match page {
        Ok(Page::Image(image)) => image,
        other => panic!("not a page: {other:?}"),
    }
}

fn page(handle: &mut Handle) -> Image {
    image(handle.batch().next_page(paced(None)))
}

/// Pages until the feeder is empty, and what said so.
fn pages_until_empty(handle: &mut Handle) -> (usize, Call) {
    let mut batch = handle.batch();
    loop {
        match batch.next_page(paced(None)).unwrap() {
            Page::Image(_) => {}
            Page::NoDocs { from, pages } => {
                assert_eq!(pages, batch.pages());
                return (pages, from);
            }
        }
    }
}

/// A sample of an image: pixel `x` of row `row`, channel `channel`.
fn sample(image: &Image, row: usize, x: usize, channel: usize) -> u16 {
    let line = &image.data[row * image.stride..(row + 1) * image.stride];
    let channels = if image.format == FrameFormat::RGB {
        3
    } else {
        1
    };
    match image.depth {
        1 => {
            // A byte of eight pixels of one channel, the channels' bytes
            // interleaved.
            let byte = line[(x / 8) * channels + channel];
            u16::from(byte >> (7 - x % 8) & 1)
        }
        8 => u16::from(line[x * channels + channel]),
        _ => {
            let at = (x * channels + channel) * 2;
            u16::from_ne_bytes([line[at], line[at + 1]])
        }
    }
}

#[test]
fn the_feeder_gives_ten_sheets_then_no_docs_again_after_reopening() {
    let child = common::real_child(
        "the_feeder_gives_ten_sheets_then_no_docs_again_after_reopening",
        &["test"],
    );
    in_child!(
        the_feeder_gives_ten_sheets_then_no_docs_again_after_reopening,
        child,
        || {
            let sane = common::load_real();
            for _ in 0..2 {
                let mut handle = open(&sane);
                text(&mut handle, "source", "Automatic Document Feeder");
                assert_eq!(pages_until_empty(&mut handle), (10, Call::Start));
                handle.close();
            }
        }
    );
}

#[test]
fn ending_a_batch_after_a_page_leaves_the_feeder_where_it_was() {
    let child = common::real_child(
        "ending_a_batch_after_a_page_leaves_the_feeder_where_it_was",
        &["test"],
    );
    in_child!(
        ending_a_batch_after_a_page_leaves_the_feeder_where_it_was,
        child,
        || {
            let sane = common::load_real();
            let mut handle = open(&sane);
            text(&mut handle, "source", "Automatic Document Feeder");
            // Three pages, then the batch ended (`sane_cancel`) with no
            // fourth started: nothing was fed beyond them.
            let mut batch = handle.batch();
            for _ in 0..3 {
                image(batch.next_page(paced(None)));
            }
            drop(batch);
            // A new batch goes on from the fourth sheet; after `NO_DOCS` the
            // backend's feeder fills again, ten more.
            assert_eq!(pages_until_empty(&mut handle), (7, Call::Start));
            assert_eq!(pages_until_empty(&mut handle), (10, Call::Start));
        }
    );
}

#[test]
fn gray_and_color_at_1_8_and_16_bits() {
    let child = common::real_child("gray_and_color_at_1_8_and_16_bits", &["test"]);
    in_child!(gray_and_color_at_1_8_and_16_bits, child, || {
        let sane = common::load_real();
        let mut handle = open(&sane);
        for (mode, format, channels) in [
            ("Gray", FrameFormat::GRAY, 1),
            ("Color", FrameFormat::RGB, 3),
        ] {
            text(&mut handle, "mode", mode);
            for depth in [1, 8, 16] {
                set(&mut handle, "depth", Value::Int(vec![depth]));
                for picture in ["Solid black", "Solid white"] {
                    text(&mut handle, "test-picture", picture);
                    let image = page(&mut handle);
                    let what = format!("{mode} {depth} {picture}");
                    let frame = image.frames[0];
                    assert_eq!(image.frames.len(), 1, "{what}");
                    assert_eq!(
                        (frame.format, frame.last_frame, frame.depth),
                        (format, 1, depth),
                        "{what}"
                    );
                    assert_eq!(
                        (
                            image.format,
                            i32::from(image.depth),
                            image.width,
                            image.height
                        ),
                        (format, depth, 31, 39),
                        "{what}"
                    );
                    let row = if depth == 1 {
                        31usize.div_ceil(8) * channels
                    } else {
                        31 * channels * depth as usize / 8
                    };
                    assert_eq!(image.stride, row, "{what}");
                    assert_eq!(image.data.len(), 39 * row, "{what}");
                    let full = match depth {
                        1 => 1,
                        8 => 0xFF,
                        _ => 0xFFFF,
                    };
                    // At depth 1 a gray 1 is black; everywhere else 0 is.
                    let expected = match (picture, depth, format) {
                        ("Solid black", 1, FrameFormat::GRAY) => 1,
                        ("Solid black", ..) => 0,
                        (_, 1, FrameFormat::GRAY) => 0,
                        _ => full,
                    };
                    for row in 0..image.height {
                        for x in 0..image.width {
                            for channel in 0..channels {
                                assert_eq!(
                                    sample(&image, row, x, channel),
                                    expected,
                                    "{what}: row {row}, pixel {x}, channel {channel}"
                                );
                            }
                        }
                    }
                }
            }
        }
    });
}

#[test]
fn a_hand_scanner_has_an_unknown_height_alone_and_from_the_feeder() {
    let child = common::real_child(
        "a_hand_scanner_has_an_unknown_height_alone_and_from_the_feeder",
        &["test"],
    );
    in_child!(
        a_hand_scanner_has_an_unknown_height_alone_and_from_the_feeder,
        child,
        || {
            let sane = common::load_real();
            let mut handle = open(&sane);
            on(&mut handle, "hand-scanner");
            let alone = page(&mut handle);
            assert_eq!(alone.frames[0].lines, -1);
            assert_eq!((alone.width, alone.height), (43, 66));
            assert_eq!(alone.data.len(), 43 * 66);
            // The backend's feeder counts every scan since the device
            // opened, the flatbed's too: opened again, it holds ten.
            handle.close();
            let mut handle = open(&sane);
            on(&mut handle, "hand-scanner");
            text(&mut handle, "source", "Automatic Document Feeder");
            let mut batch = handle.batch();
            for _ in 0..10 {
                let image = image(batch.next_page(paced(None)));
                assert_eq!(image.frames[0].lines, -1);
                assert_eq!((image.width, image.height), (43, 66));
            }
            assert_eq!(
                batch.next_page(paced(None)).unwrap(),
                Page::NoDocs {
                    from: Call::Start,
                    pages: 10
                }
            );
        }
    );
}

#[test]
fn short_reads_assemble_the_same_image() {
    let child = common::real_child("short_reads_assemble_the_same_image", &["test"]);
    in_child!(short_reads_assemble_the_same_image, child, || {
        let sane = common::load_real();
        let mut handle = open(&sane);
        text(&mut handle, "mode", "Color");
        text(&mut handle, "test-picture", "Color pattern");
        let mut long = 0;
        let mut pace = paced(None);
        let whole = image(handle.batch().next_page(|progress| {
            pace(progress);
            long += 1;
        }));
        assert_eq!(long, (31 * 3 * 39usize).div_ceil(64));
        set(&mut handle, "read-limit-size", Value::Int(vec![7]));
        let mut reads = 0;
        let mut pace = paced(None);
        let limited = image(handle.batch().next_page(|progress| {
            pace(progress);
            reads += 1;
            assert_eq!(progress.bytes_expected, Some(31 * 3 * 39));
        }));
        assert_eq!(limited, whole);
        assert_eq!(reads, (31 * 3 * 39usize).div_ceil(7));
    });
}

#[test]
fn three_pass_in_all_six_orders_equals_one_pass() {
    let child = common::real_child("three_pass_in_all_six_orders_equals_one_pass", &["test"]);
    in_child!(three_pass_in_all_six_orders_equals_one_pass, child, || {
        let sane = common::load_real();
        let mut handle = open(&sane);
        text(&mut handle, "mode", "Color");
        text(&mut handle, "test-picture", "Color pattern");
        for depth in [8, 16, 1] {
            set(&mut handle, "depth", Value::Int(vec![depth]));
            set(&mut handle, "three-pass", Value::Bool(false));
            let one = page(&mut handle);
            on(&mut handle, "three-pass");
            let orders: &[&str] = if depth == 8 {
                &["RGB", "RBG", "GBR", "GRB", "BRG", "BGR"]
            } else {
                &["BGR"]
            };
            let frame_bytes = Some(one.data.len() as u64 / 3);
            for order in orders {
                text(&mut handle, "three-pass-order", order);
                let three = image(handle.batch().next_page(paced(frame_bytes)));
                let formats: Vec<FrameFormat> =
                    three.frames.iter().map(|frame| frame.format).collect();
                let expected: Vec<FrameFormat> = order
                    .chars()
                    .map(|channel| match channel {
                        'R' => FrameFormat::RED,
                        'G' => FrameFormat::GREEN,
                        _ => FrameFormat::BLUE,
                    })
                    .collect();
                assert_eq!(formats, expected, "{order} at {depth}");
                let lasts: Vec<i32> = three.frames.iter().map(|frame| frame.last_frame).collect();
                assert_eq!(lasts, [0, 0, 1]);
                assert_eq!(
                    (
                        three.format,
                        three.width,
                        three.height,
                        three.stride,
                        three.depth
                    ),
                    (one.format, one.width, one.height, one.stride, one.depth),
                    "{order} at {depth}"
                );
                assert!(
                    three.data == one.data,
                    "{order} at {depth}: the samples differ"
                );
            }
        }
    });
}

#[test]
fn padded_rows_lose_their_padding() {
    let child = common::real_child("padded_rows_lose_their_padding", &["test"]);
    in_child!(padded_rows_lose_their_padding, child, || {
        let sane = common::load_real();
        let mut handle = open(&sane);
        text(&mut handle, "test-picture", "Color pattern");
        for (mode, channels) in [("Gray", 1), ("Color", 3)] {
            text(&mut handle, "mode", mode);
            set(&mut handle, "ppl-loss", Value::Int(vec![0]));
            let whole = page(&mut handle);
            set(&mut handle, "ppl-loss", Value::Int(vec![3]));
            let padded = page(&mut handle);
            // The backend keeps the bytes a line and lowers the pixels: the
            // last three of each line are padding now.
            let frame = padded.frames[0];
            assert_eq!(frame.bytes_per_line, 31 * channels as i32);
            assert_eq!(frame.pixels_per_line, 28);
            assert_eq!((padded.width, padded.stride), (28, 28 * channels));
            for row in 0..whole.height {
                let line = &whole.data[row * whole.stride..][..padded.stride];
                assert_eq!(&padded.data[row * padded.stride..][..padded.stride], line);
            }
        }
    });
}

#[test]
fn sixteen_bit_samples_are_as_the_backend_orders_them() {
    let child = common::real_child(
        "sixteen_bit_samples_are_as_the_backend_orders_them",
        &["test"],
    );
    in_child!(
        sixteen_bit_samples_are_as_the_backend_orders_them,
        child,
        || {
            let sane = common::load_real();
            let mut handle = open(&sane);
            text(&mut handle, "test-picture", "Color pattern");
            set(&mut handle, "depth", Value::Int(vec![16]));
            let native = page(&mut handle);
            on(&mut handle, "invert-endianess");
            let swapped = page(&mut handle);
            assert_eq!(native.data.len(), swapped.data.len());
            assert!(
                native.data != swapped.data,
                "the picture has samples that differ when swapped"
            );
            for (one, other) in native.data.chunks(2).zip(swapped.data.chunks(2)) {
                assert_eq!([one[1], one[0]], other);
            }
        }
    );
}

/// Waits for the `test` backend's reader thread, which a start begins, to
/// have its picture: the backend's 1.4.0 cancels that thread asynchronously,
/// and a cancel while it's still allocating can leave the allocator locked
/// for the next scan's thread (seen about once in 1,500 cancels coming at
/// once after a start). A cancel at once after a start is the fake's to
/// test.
fn let_the_reader_start() {
    std::thread::sleep(std::time::Duration::from_millis(50));
}

#[test]
fn every_status_forced_from_a_read_is_said() {
    let child = common::real_child("every_status_forced_from_a_read_is_said", &["test"]);
    in_child!(every_status_forced_from_a_read_is_said, child, || {
        let sane = common::load_real();
        let mut handle = open(&sane);
        let forced = index(&handle, "read-return-value");
        let errors = [
            ("SANE_STATUS_UNSUPPORTED", Status::UNSUPPORTED),
            ("SANE_STATUS_CANCELLED", Status::CANCELLED),
            ("SANE_STATUS_DEVICE_BUSY", Status::DEVICE_BUSY),
            ("SANE_STATUS_INVAL", Status::INVAL),
            ("SANE_STATUS_JAMMED", Status::JAMMED),
            ("SANE_STATUS_NO_DOCS", Status::NO_DOCS),
            ("SANE_STATUS_COVER_OPEN", Status::COVER_OPEN),
            ("SANE_STATUS_IO_ERROR", Status::IO_ERROR),
            ("SANE_STATUS_NO_MEM", Status::NO_MEM),
            ("SANE_STATUS_ACCESS_DENIED", Status::ACCESS_DENIED),
        ];
        let mut buffer = [0; 64];
        for (name, expected) in errors {
            handle.set(forced, &Value::String(name.into())).unwrap();
            handle.start().unwrap();
            let_the_reader_start();
            // Each read's status, with the call that answered; the scan is
            // cancelled after it.
            match handle.read(&mut buffer) {
                Err(Error::Status {
                    status,
                    call: Call::Read,
                }) if status == expected => {}
                other => panic!("{name}: {other:?}"),
            }
        }
        handle
            .set(forced, &Value::String("SANE_STATUS_EOF".into()))
            .unwrap();
        handle.start().unwrap();
        let_the_reader_start();
        assert_eq!(handle.read(&mut buffer).unwrap(), ReadOutcome::Eof);
        handle.cancel();
        handle
            .set(forced, &Value::String("Default".into()))
            .unwrap();
        assert_eq!(page(&mut handle).data.len(), 31 * 39);
    });
}

#[cfg(unix)]
#[test]
fn non_blocking_reads_with_a_select_fd() {
    let child = common::real_child("non_blocking_reads_with_a_select_fd", &["test"]);
    in_child!(non_blocking_reads_with_a_select_fd, child, || {
        let sane = common::load_real();
        let mut handle = open(&sane);
        text(&mut handle, "test-picture", "Color pattern");
        /// The frame's bytes as SANE gives them, read step by step.
        fn read_all(handle: &mut Handle) -> (Vec<u8>, usize) {
            let mut bytes = Vec::new();
            let mut empty = 0;
            let mut buffer = [0; 100];
            loop {
                match handle.read(&mut buffer).unwrap() {
                    ReadOutcome::Bytes(0) => {
                        empty += 1;
                        std::thread::sleep(std::time::Duration::from_millis(1));
                    }
                    ReadOutcome::Bytes(length) => bytes.extend_from_slice(&buffer[..length]),
                    ReadOutcome::Eof => return (bytes, empty),
                }
            }
        }
        // Without the backend's options, neither is offered.
        handle.start().unwrap();
        match handle.set_io_mode(true) {
            Err(Error::Status {
                status: Status::UNSUPPORTED,
                call: Call::SetIoMode,
            }) => {}
            other => panic!("{other:?}"),
        }
        match handle.select_fd() {
            Err(Error::Status {
                status: Status::UNSUPPORTED,
                call: Call::GetSelectFd,
            }) => {}
            other => panic!("{other:?}"),
        }
        handle.set_io_mode(false).unwrap();
        let (blocking, empty) = read_all(&mut handle);
        assert_eq!((blocking.len(), empty), (31 * 39, 0));
        handle.cancel();
        on(&mut handle, "non-blocking");
        on(&mut handle, "select-fd");
        handle.start().unwrap();
        handle.set_io_mode(true).unwrap();
        let fd = handle.select_fd().unwrap();
        assert!(fd >= 0);
        // The backend's 1.4.0 reads an empty pipe as the frame's end, and
        // answers GOOD with a length of -1 (refused, `BadReadLength`): its
        // reader thread is let fill the pipe first, as a poll of the select
        // fd would wait for it.
        let_the_reader_start();
        // The backend's pipe, open in this process.
        #[cfg(target_os = "linux")]
        {
            let target = std::fs::read_link(format!("/proc/self/fd/{fd}")).unwrap();
            assert!(target.to_string_lossy().starts_with("pipe:"), "{target:?}");
        }
        let (non_blocking, _) = read_all(&mut handle);
        assert_eq!(non_blocking, blocking);
        handle.cancel();
    });
}

#[test]
fn parameters_are_estimates_before_the_start_and_exact_after() {
    let child = common::real_child(
        "parameters_are_estimates_before_the_start_and_exact_after",
        &["test"],
    );
    in_child!(
        parameters_are_estimates_before_the_start_and_exact_after,
        child,
        || {
            let sane = common::load_real();
            let mut handle = open(&sane);
            let exact = Parameters {
                format: FrameFormat::GRAY,
                last_frame: 1,
                bytes_per_line: 31,
                pixels_per_line: 31,
                lines: 39,
                depth: 8,
            };
            assert_eq!(handle.parameters().unwrap(), exact);
            on(&mut handle, "fuzzy-parameters");
            // Before the start, an estimate: what the backend says, which
            // may be anything, kept as it came.
            let estimate = handle.parameters().unwrap();
            assert_eq!(
                (estimate.format, estimate.depth),
                (exact.format, exact.depth)
            );
            // After it, the frame's own, which the page assembled keeps.
            let image = page(&mut handle);
            assert_eq!(image.frames, [exact]);
            assert_eq!((image.width, image.height), (31, 39));
        }
    );
}
