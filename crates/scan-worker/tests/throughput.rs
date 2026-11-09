//! The pipe's throughput: 300 dpi Letter pages through a worker, in color and in black and
//! white, measured on every run and held at 2.3 pages a second. A test binary of its own, with
//! one test, so nothing runs beside it.

mod support;

use std::{
    io::{self, Write},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

use scan_worker::RemoteDevice;
use scan_worker::scan_device::in_progress::Tracker;
use scan_worker::scan_device::{Done, Event, Format, Pixels, Source};

use support::{client_of, open, settings};

/// The gate, in pages a second.
const GATE: f64 = 2.3;

/// The sheets in the feeder for each batch, both sides read.
const SHEETS: usize = 24;

/// The pages the rate leaves out: the batch's start, and the first sheets'.
const LEFT_OUT: usize = 8;

/// A batch's rate, over the pages after the first [`LEFT_OUT`].
struct Rate {
    pages: usize,
    elapsed: Duration,
    bytes: u64,
}

impl Rate {
    fn pages_a_second(&self) -> f64 {
        self.pages as f64 / self.elapsed.as_secs_f64()
    }

    fn megabytes_a_second(&self) -> f64 {
        self.bytes as f64 / 1e6 / self.elapsed.as_secs_f64()
    }
}

#[test]
fn letter_pages_at_300_dpi_cross_at_2_3_a_second_at_least() {
    // Every batch finds its sheets in the feeder: one worker for both formats.
    let mut device = open(&client_of("scan-more", SHEETS));
    let build = if cfg!(debug_assertions) {
        "debug"
    } else {
        "release"
    };
    for format in [Format::Rgb8, Format::Bw1] {
        let rate = batch_rate(&mut device, format);
        // Past the harness's capture, so every run shows it.
        let _ = writeln!(
            io::stderr(),
            "throughput: {format:?}, 300 dpi Letter, duplex, {build} build: {:.1} pages a second, \
             {:.1} MB a second, over pages {} to {} ({:.2} s)",
            rate.pages_a_second(),
            rate.megabytes_a_second(),
            LEFT_OUT + 1,
            LEFT_OUT + rate.pages,
            rate.elapsed.as_secs_f64(),
        );
        assert!(
            rate.pages_a_second() >= GATE,
            "{format:?}: {:.2} pages a second, under the gate's {GATE}",
            rate.pages_a_second()
        );
    }
    device.close().expect("closed");
}

/// A batch of [`SHEETS`] duplex sheets in `format` through `device`, each page timed as the
/// caller takes it: the rate over the pages after the first [`LEFT_OUT`].
fn batch_rate(device: &mut RemoteDevice, format: Format) -> Rate {
    let (events, received) = mpsc::sync_channel(2);
    // The caller's side: each page's time and length, the page dropped.
    let taken = thread::spawn(move || {
        received
            .iter()
            .filter_map(|event| match event {
                Event::Page(page) => match &page.pixels {
                    Pixels::Raw(image) => Some((Instant::now(), image.data().len() as u64)),
                    _ => panic!("a raw page"),
                },
                _ => None,
            })
            .collect::<Vec<_>>()
    });
    let done = device.scan(
        &settings(Source::Feeder, 300, format, true),
        1,
        &events,
        &Tracker::new(),
    );
    drop(events);
    let taken = taken.join().expect("the caller's thread");
    assert_eq!(done.expect("a batch"), Done::FeederEmpty);
    assert_eq!(taken.len(), 2 * SHEETS);
    // Letter at 300 dpi is 2550 by 3300 pixels.
    let page = match format {
        Format::Rgb8 => 2550 * 3300 * 3,
        Format::Bw1 => 2550_u64.div_ceil(8) * 3300,
        _ => unreachable!("Rgb8 and Bw1 alone"),
    };
    assert!(taken.iter().all(|(_, bytes)| *bytes == page));
    let counted = &taken[LEFT_OUT..];
    let (start, _) = taken[LEFT_OUT - 1];
    let (end, _) = taken[taken.len() - 1];
    Rate {
        pages: counted.len(),
        elapsed: end - start,
        bytes: counted.iter().map(|(_, bytes)| bytes).sum(),
    }
}
