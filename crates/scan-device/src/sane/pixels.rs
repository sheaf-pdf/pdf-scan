//! Pages into the raw layout: SANE's images, as `sane-dl` assembles them, in the layout the
//! crate's docs give ("The raw layout").
//!
//! `sane-dl` gives a page's rows top down, their padding dropped and a three-pass page's frames
//! interleaved, the samples as SANE encodes them: at 1 bit, eight pixels a byte with 1 black in
//! gray, and in color a byte of eight pixels of one channel, the bytes red, green, blue, 1 the
//! most intensity; at 16 bits, two bytes a sample in the host's order. Each becomes the model's
//! layout here, in place where it can: gray at 1 bit inverted and each row's bits past its last
//! pixel cleared; 16-bit samples made little-endian; 1-bit color expanded to 8 bits a sample.

use sane_dl::{FrameFormat, Image};

use crate::{Format, RawImage, ScanError};

/// `image` in the raw layout: its 16-bit samples read in `big_endian` order (the host's, as
/// `sane-dl` gives them, passed in so either order is tested on any host), a 1-bit color page's
/// expansion refused past `frame_cap` bytes before anything is allocated.
///
/// A page is the format its frames say, whatever the settings asked: gray at 1 bit is
/// [`Format::Bw1`], at 8 and 16 [`Format::Gray8`] and [`Format::Gray16`]; color at 8 and 16
/// [`Format::Rgb8`] and [`Format::Rgb16`], and at 1 bit [`Format::Rgb8`], each sample 0 or 255.
/// Anything else, a width or height past `u32`, and a buffer [`RawImage::new`] refuses, are
/// [`ScanError::Driver`].
pub(super) fn raw(image: Image, big_endian: bool, frame_cap: usize) -> Result<RawImage, ScanError> {
    let Image {
        format,
        depth,
        width,
        height,
        stride,
        mut data,
        ..
    } = image;
    let (Ok(width), Ok(height)) = (u32::try_from(width), u32::try_from(height)) else {
        return Err(driver(format!(
            "the scanner gave a page {width} by {height} pixels, more than a page can be"
        )));
    };
    let format = match (format, depth) {
        (FrameFormat::GRAY, 1) => {
            invert(&mut data);
            clear_padding(&mut data, width, stride);
            Format::Bw1
        }
        (FrameFormat::GRAY, 8) => Format::Gray8,
        (FrameFormat::RGB, 8) => Format::Rgb8,
        (FrameFormat::GRAY, 16) => {
            to_little_endian(&mut data, big_endian);
            Format::Gray16
        }
        (FrameFormat::RGB, 16) => {
            to_little_endian(&mut data, big_endian);
            Format::Rgb16
        }
        (FrameFormat::RGB, 1) => {
            data = expand(&data, width, height, stride, frame_cap)?;
            Format::Rgb8
        }
        (format, depth) => {
            return Err(driver(format!(
                "the scanner gave a page of {format} at {depth} bits, which isn't a raw format"
            )));
        }
    };
    RawImage::new(format, width, height, data).map_err(|error| {
        driver(format!(
            "the scanner's page doesn't make a raw image: {error}"
        ))
    })
}

/// Each byte inverted: SANE's 1-bit gray has 1 black, the model's 0.
fn invert(data: &mut [u8]) {
    for byte in data {
        *byte = !*byte;
    }
}

/// Each row's bits past its last pixel cleared, in rows of `stride` bytes: the model's padding is
/// 0, whatever the scanner left there (and the inversion made of it).
fn clear_padding(data: &mut [u8], width: u32, stride: usize) {
    let pixels_in_last_byte = width % 8;
    if pixels_in_last_byte == 0 || stride == 0 {
        return;
    }
    // The high bits of a row's last byte, its pixels'.
    let kept = !(0xff_u8 >> pixels_in_last_byte);
    for row in data.chunks_exact_mut(stride) {
        if let Some(last) = row.last_mut() {
            *last &= kept;
        }
    }
}

/// Each two bytes made little-endian from `big_endian` order: swapped where they're big-endian,
/// left alone where they're little-endian already.
fn to_little_endian(data: &mut [u8], big_endian: bool) {
    if big_endian {
        let (samples, _) = data.as_chunks_mut::<2>();
        for sample in samples {
            sample.swap(0, 1);
        }
    }
}

/// 1-bit color expanded to 8 bits a sample, in rows of `stride` bytes: each group of eight pixels
/// a byte of red, one of green and one of blue, the leftmost pixel in each byte's high bit; a bit
/// set is 255, clear 0. Its size is worked out first and refused past `frame_cap`, since it's
/// eight times the frame `sane-dl` capped.
fn expand(
    data: &[u8],
    width: u32,
    height: u32,
    stride: usize,
    frame_cap: usize,
) -> Result<Vec<u8>, ScanError> {
    let length = RawImage::data_len(Format::Rgb8, width, height)
        .map_err(|error| driver(format!("the scanner's page can't be held here: {error}")))?;
    if length > frame_cap {
        return Err(driver(format!(
            "the scanner's 1-bit color page is {length} bytes at 8 bits a sample, past the cap of \
             {frame_cap}; a lower resolution asks for less"
        )));
    }
    // A width that fits `u32` fits `usize` wherever a page of it fits memory, as `length` did.
    let pixels = width as usize;
    let needed = pixels.div_ceil(8) * 3;
    let rows = stride.checked_mul(height as usize);
    if stride < needed || rows != Some(data.len()) {
        return Err(driver(format!(
            "the scanner's 1-bit color page has {} bytes in rows of {stride}, not {height} rows \
             of {needed}",
            data.len()
        )));
    }
    let mut expanded = Vec::with_capacity(length);
    for row in data.chunks_exact(stride) {
        for x in 0..pixels {
            let group = x / 8 * 3;
            let bit = 7 - x % 8;
            for channel in &row[group..group + 3] {
                expanded.push(if channel >> bit & 1 == 1 { 0xff } else { 0 });
            }
        }
    }
    Ok(expanded)
}

/// `message`, the driver's words, unshown.
fn driver(message: String) -> ScanError {
    ScanError::Driver {
        message,
        shown: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An image as `sane-dl` assembles one: rows of `stride` bytes, no frames said.
    fn image(format: FrameFormat, depth: u8, width: usize, stride: usize, data: Vec<u8>) -> Image {
        Image {
            format,
            depth,
            width,
            height: data.len().checked_div(stride).unwrap_or(0),
            stride,
            data,
            frames: Vec::new(),
        }
    }

    const CAP: usize = 1 << 30;

    fn message(error: ScanError) -> String {
        match error {
            ScanError::Driver { message, shown } => {
                assert!(!shown);
                message
            }
            other => panic!("not the driver's words: {other:?}"),
        }
    }

    #[test]
    fn gray_at_1_bit_is_inverted_and_its_padding_cleared_at_every_width() {
        for width in 1..=17_usize {
            let stride = width.div_ceil(8);
            // Two rows: SANE's all black (every bit 1, padding too), then all white with its
            // padding bits set, as a scanner might leave them.
            let mut data = vec![0xff; stride];
            let mut white = vec![0x00; stride];
            if width % 8 != 0 {
                *white.last_mut().unwrap() = 0xff >> (width % 8);
            }
            data.extend(&white);
            let raw = raw(image(FrameFormat::GRAY, 1, width, stride, data), false, CAP).unwrap();
            assert_eq!(raw.format(), Format::Bw1);
            assert_eq!((raw.width(), raw.height()), (width as u32, 2));
            // Black is 0; white is 1 but past the last pixel.
            let mut expected = vec![0x00; stride];
            let mut ones = vec![0xff; stride];
            if width % 8 != 0 {
                *ones.last_mut().unwrap() = !(0xff >> (width % 8));
            }
            expected.extend(&ones);
            assert_eq!(raw.data(), expected, "width {width}");
        }
    }

    #[test]
    fn gray_and_color_at_8_bits_are_as_they_come() {
        let gray = raw(
            image(FrameFormat::GRAY, 8, 3, 3, vec![0, 128, 255]),
            false,
            CAP,
        )
        .unwrap();
        assert_eq!(
            (gray.format(), gray.data()),
            (Format::Gray8, &[0, 128, 255][..])
        );
        let rgb = (0..12).collect::<Vec<u8>>();
        let color = raw(image(FrameFormat::RGB, 8, 2, 6, rgb.clone()), false, CAP).unwrap();
        assert_eq!((color.format(), color.data()), (Format::Rgb8, &rgb[..]));
        assert_eq!((color.width(), color.height()), (2, 2));
    }

    #[test]
    fn sixteen_bit_samples_are_little_endian_from_either_order() {
        // The samples 0x0102 and 0xa0b0, as each order holds them.
        let little = vec![0x02, 0x01, 0xb0, 0xa0];
        let big = vec![0x01, 0x02, 0xa0, 0xb0];
        for (data, big_endian) in [(little.clone(), false), (big, true)] {
            let gray = raw(image(FrameFormat::GRAY, 16, 2, 4, data), big_endian, CAP).unwrap();
            assert_eq!(gray.format(), Format::Gray16);
            assert_eq!(gray.data(), little, "big-endian source: {big_endian}");
        }
        let color_big: Vec<u8> = vec![0x00, 0x01, 0x00, 0x02, 0x00, 0x03];
        let color = raw(image(FrameFormat::RGB, 16, 1, 6, color_big), true, CAP).unwrap();
        assert_eq!(color.format(), Format::Rgb16);
        assert_eq!(color.data(), [0x01, 0x00, 0x02, 0x00, 0x03, 0x00]);
    }

    #[test]
    fn one_bit_color_is_expanded_to_8_bits_a_sample() {
        // Ten pixels: two groups, each a byte of red, of green and of blue. Pixel 0 red, pixel 1
        // green, pixel 2 blue, pixel 3 white, pixel 9 yellow; the rest black. The padding bits of
        // the second group are set, and dropped.
        let row = vec![0b1001_0000, 0b0101_0000, 0b0011_0000, 0x7f, 0x7f, 0x3f];
        let data = [row.clone(), row].concat();
        let raw = raw(image(FrameFormat::RGB, 1, 10, 6, data), false, CAP).unwrap();
        assert_eq!(raw.format(), Format::Rgb8);
        assert_eq!((raw.width(), raw.height()), (10, 2));
        let mut pixels = [[0_u8; 3]; 10];
        pixels[0] = [255, 0, 0];
        pixels[1] = [0, 255, 0];
        pixels[2] = [0, 0, 255];
        pixels[3] = [255, 255, 255];
        pixels[9] = [255, 255, 0];
        let row: Vec<u8> = pixels.concat();
        assert_eq!(raw.data(), [row.clone(), row].concat());
    }

    #[test]
    fn one_bit_color_past_the_frame_cap_is_refused_before_anything_is_allocated() {
        // 8 by 1 pixels: 3 bytes at 1 bit, 24 at 8.
        let small = image(FrameFormat::RGB, 1, 8, 3, vec![0xff; 3]);
        assert!(message(raw(small.clone(), false, 23).unwrap_err()).contains("past the cap of 23"));
        assert_eq!(raw(small, false, 24).unwrap().data(), [0xff; 24]);
        // A page whose expansion no memory holds says so from its size alone: its frame is three
        // bytes, its expansion 3 × 2^62.
        let mut huge = image(FrameFormat::RGB, 1, 8, 3, vec![0xff; 3]);
        huge.width = u32::MAX as usize;
        huge.height = u32::MAX as usize;
        let refused = message(raw(huge, false, CAP).unwrap_err());
        assert!(refused.contains("can't be held"), "{refused}");
        let mut tall = image(FrameFormat::RGB, 1, 8, 3, vec![0xff; 3]);
        tall.height = 1 << 26;
        let refused = message(raw(tall, false, CAP).unwrap_err());
        assert!(
            refused.contains("a lower resolution asks for less"),
            "{refused}"
        );
    }

    #[test]
    fn what_isn_t_a_raw_page_is_the_driver_s_words() {
        for (format, depth) in [
            (FrameFormat::GRAY, 12),
            (FrameFormat::RED, 8),
            (FrameFormat::JPEG, 8),
        ] {
            let refused = raw(image(format, depth, 1, 1, vec![0]), false, CAP).unwrap_err();
            assert!(message(refused).contains("isn't a raw format"));
        }
        // A width past `u32`.
        let mut wide = image(FrameFormat::GRAY, 8, 1, 1, vec![0]);
        wide.width = u32::MAX as usize + 1;
        assert!(message(raw(wide, false, CAP).unwrap_err()).contains("more than a page"));
        // A buffer of the wrong length.
        let mut short = image(FrameFormat::GRAY, 8, 4, 4, vec![0; 4]);
        short.height = 2;
        assert!(message(raw(short, false, CAP).unwrap_err()).contains("doesn't make a raw image"));
        let mut short_color = image(FrameFormat::RGB, 1, 8, 3, vec![0; 3]);
        short_color.height = 2;
        assert!(message(raw(short_color, false, CAP).unwrap_err()).contains("not 2 rows of 3"));
    }
}
