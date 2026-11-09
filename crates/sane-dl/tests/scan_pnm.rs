//! Scanning through SANE's `pnm` backend, which scans an image file: PBM,
//! PGM and PPM images the test writes, scanned and compared byte for byte
//! with what they hold, in one pass and three (green, blue, red), and as a
//! hand scanner of unknown height. `pnm` reads only 8-bit and 1-bit files
//! (it takes any PGM or PPM as 8-bit, whatever its maximum), so no 16-bit
//! image goes through it. Each test runs in a child process whose
//! configuration is a private, isolated directory naming `pnm` alone; where
//! there's no libsane each says it skipped, and with
//! `SANE_DL_REQUIRE_LIBSANE=1` fails instead.

#[macro_use]
mod common;

use std::path::{Path, PathBuf};

use sane_dl::{FrameFormat, Handle, Image, Page, Value};

/// The images the test writes: a 13 by 4 bitmap (two bytes a row, three
/// bits of the second unused), a 16 by 9 gray image and a 5 by 3 color one.
fn images() -> [(&'static str, Vec<u8>, Vec<u8>); 3] {
    let bitmap: Vec<u8> = (0..4u8)
        .flat_map(|row| [0b1010_0101 ^ row, 0b1100_1000 | (row << 3)])
        .collect();
    let gray: Vec<u8> = (0..16 * 9).map(|at| (at * 7 % 256) as u8).collect();
    let color: Vec<u8> = (0..5 * 3 * 3).map(|at| (255 - at * 5) as u8).collect();
    [
        ("bitmap.pbm", b"P4\n13 4\n".to_vec(), bitmap),
        ("gray.pgm", b"P5\n16 9\n255\n".to_vec(), gray),
        ("color.ppm", b"P6\n5 3\n255\n".to_vec(), color),
    ]
}

fn image_path(test: &str, name: &str) -> PathBuf {
    common::test_dir(test).join(name)
}

/// A child for `pnm`, its images written by the parent as it's spawned.
fn child(test: &'static str) -> common::Child {
    common::real_child(test, &["pnm"]).prepare(move |_| {
        for (name, header, raster) in images() {
            let mut file = header;
            file.extend_from_slice(&raster);
            std::fs::write(image_path(test, name), file).expect("couldn't write an image");
        }
    })
}

fn set(handle: &mut Handle, name: &str, value: Value) {
    let option = handle
        .find_option(name)
        .unwrap_or_else(|| panic!("no option {name}"))
        .index;
    handle.set(option, &value).unwrap();
}

fn scan(handle: &mut Handle, path: &Path) -> Image {
    let path = path.to_str().expect("the test's path is UTF-8");
    set(handle, "filename", Value::String(path.into()));
    match handle.batch().next_page(|_| {}) {
        Ok(Page::Image(image)) => image,
        other => panic!("not a page: {other:?}"),
    }
}

#[test]
fn images_scan_as_they_are_written() {
    const TEST: &str = "images_scan_as_they_are_written";
    in_child!(images_scan_as_they_are_written, child(TEST), || {
        let sane = common::load_real();
        let mut handle = sane.open("pnm:0").unwrap();
        let shapes = [
            (FrameFormat::GRAY, 1, 13, 4, 2),
            (FrameFormat::GRAY, 8, 16, 9, 16),
            (FrameFormat::RGB, 8, 5, 3, 15),
        ];
        for ((name, _, raster), (format, depth, width, height, stride)) in
            images().into_iter().zip(shapes)
        {
            let image = scan(&mut handle, &image_path(TEST, name));
            assert_eq!(
                (
                    image.format,
                    image.depth,
                    image.width,
                    image.height,
                    image.stride
                ),
                (format, depth, width, height, stride),
                "{name}"
            );
            // A bitmap's 1 is black in PBM and in SANE alike.
            assert_eq!(image.data, raster, "{name}");
            assert_eq!(image.frames.len(), 1);
        }
    });
}

#[test]
fn three_passes_come_green_blue_red_and_interleave_to_the_image() {
    const TEST: &str = "three_passes_come_green_blue_red_and_interleave_to_the_image";
    in_child!(
        three_passes_come_green_blue_red_and_interleave_to_the_image,
        child(TEST),
        || {
            let sane = common::load_real();
            let mut handle = sane.open("pnm:0").unwrap();
            set(&mut handle, "three-pass", Value::Bool(true));
            let [_, _, (name, _, raster)] = images();
            let image = scan(&mut handle, &image_path(TEST, name));
            let formats: Vec<FrameFormat> = image.frames.iter().map(|frame| frame.format).collect();
            assert_eq!(
                formats,
                [FrameFormat::GREEN, FrameFormat::BLUE, FrameFormat::RED]
            );
            assert_eq!((image.format, image.stride), (FrameFormat::RGB, 15));
            assert_eq!(image.data, raster);
        }
    );
}

#[test]
fn a_hand_scanner_reads_each_image_to_its_end() {
    const TEST: &str = "a_hand_scanner_reads_each_image_to_its_end";
    in_child!(
        a_hand_scanner_reads_each_image_to_its_end,
        child(TEST),
        || {
            let sane = common::load_real();
            let mut handle = sane.open("pnm:0").unwrap();
            set(&mut handle, "hand-scanner", Value::Bool(true));
            for (name, _, raster) in images() {
                let image = scan(&mut handle, &image_path(TEST, name));
                assert_eq!(image.frames[0].lines, -1, "{name}");
                assert_eq!(image.data, raster, "{name}");
            }
            set(&mut handle, "three-pass", Value::Bool(true));
            let [_, _, (name, _, raster)] = images();
            let image = scan(&mut handle, &image_path(TEST, name));
            assert_eq!((image.frames.len(), image.height), (3, 3));
            assert_eq!(image.data, raster);
        }
    );
}
