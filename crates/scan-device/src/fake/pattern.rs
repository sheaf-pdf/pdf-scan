//! The pages the fake draws.

use crate::{Format, LayoutError, RawImage, Side};

/// The page the fake draws for `side` of sheet `sheet`, `width` by `height` pixels in `format`:
/// the same bytes on every run and every system, different for each sheet and side, in the raw
/// layout (in [`Format::Bw1`], each row's bits past its last pixel 0).
///
/// A row's worth of bytes is made from the sheet's number and the side, and each row is those
/// bytes turned left by as many pixels as the row's number, counted from 0 at the top (bytes, in
/// `Bw1`), and then, in `Bw1`, its bits past the last pixel cleared; so drawing a page costs little
/// more than copying its bytes. Where a `Bw1` row has such bits, a row isn't the one before it
/// turned: the bits cleared in one come back in the next. It's public so that a page carried
/// elsewhere can be checked byte for byte against the one drawn here. The page's bytes are
/// allocated at once: refused as [`RawImage::data_len`] refuses its size, it's otherwise drawn
/// whatever that size.
///
/// ```
/// use scan_device::fake::pattern;
/// use scan_device::{Format, Side};
///
/// let front = pattern(Format::Rgb8, 64, 48, 1, Side::Front)?;
/// assert_eq!((front.width(), front.height(), front.stride()), (64, 48, 192));
/// assert_eq!(front, pattern(Format::Rgb8, 64, 48, 1, Side::Front)?);
/// assert_ne!(front, pattern(Format::Rgb8, 64, 48, 1, Side::Back)?);
/// assert_ne!(front, pattern(Format::Rgb8, 64, 48, 2, Side::Front)?);
/// # Ok::<(), scan_device::LayoutError>(())
/// ```
pub fn pattern(
    format: Format,
    width: u32,
    height: u32,
    sheet: u32,
    side: Side,
) -> Result<RawImage, LayoutError> {
    let bytes = RawImage::data_len(format, width, height)?;
    // Every row is a byte at least, so `height` fits a `usize` as the whole does.
    let stride = bytes / height as usize;
    let pixel = RawImage::data_len(format, 1, 1)? as u64;
    let first = first_row(stride, sheet, side);
    // The low bits of a `Bw1` row's last byte, past its last pixel.
    let pixels_in_last_byte = width % 8;
    let padding = match format {
        Format::Bw1 if pixels_in_last_byte != 0 => 0xff_u8 >> pixels_in_last_byte,
        _ => 0,
    };
    let mut data = Vec::with_capacity(bytes);
    for row in 0..height {
        // Below `stride`, a `usize`.
        let turn = (u64::from(row) * pixel % stride as u64) as usize;
        data.extend_from_slice(&first[turn..]);
        data.extend_from_slice(&first[..turn]);
        if let Some(last) = data.last_mut() {
            *last &= !padding;
        }
    }
    RawImage::new(format, width, height, data)
}

/// `stride` bytes made from `sheet` and `side`, by Marsaglia's xorshift (its shifts 13, 7 and
/// 17), each step's state giving eight bytes, the last step's as many as are left.
fn first_row(stride: usize, sheet: u32, side: Side) -> Vec<u8> {
    let side = match side {
        Side::Front => 0,
        Side::Back => 1,
    };
    // Never 0, which the steps would keep: the constant's high bits are set.
    let mut state = ((u64::from(sheet) << 1) | side) ^ 0x9e37_79b9_7f4a_7c15;
    // No more than `stride`, which `RawImage::data_len` found a buffer can hold.
    let mut row = Vec::with_capacity(stride);
    while row.len() < stride {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        let bytes = state.to_le_bytes();
        let left = bytes.len().min(stride - row.len());
        row.extend_from_slice(&bytes[..left]);
    }
    row
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_row_is_the_one_before_turned_left_a_pixel() {
        // So at a width with no padding bits.
        for (format, pixel) in [
            (Format::Bw1, 1),
            (Format::Gray8, 1),
            (Format::Gray16, 2),
            (Format::Rgb8, 3),
            (Format::Rgb16, 6),
        ] {
            let page = pattern(format, 40, 9, 7, Side::Back).unwrap();
            let stride = page.stride();
            let rows: Vec<&[u8]> = page.data().chunks_exact(stride).collect();
            for pair in rows.windows(2) {
                let mut turned = pair[0].to_vec();
                turned.rotate_left(pixel);
                assert_eq!(pair[1], turned, "{format:?}");
            }
        }
    }

    #[test]
    fn each_row_is_the_first_s_bytes_turned_by_its_number_then_its_padding_cleared() {
        // As the docs say, at widths with padding bits in `Bw1` too, where a row isn't the one
        // before it turned.
        for (format, width, pixel, padding) in [
            (Format::Bw1, 13, 1, 0b0000_0111),
            (Format::Bw1, 9, 1, 0b0111_1111),
            (Format::Bw1, 40, 1, 0),
            (Format::Gray8, 13, 1, 0),
            (Format::Rgb8, 5, 3, 0),
        ] {
            let page = pattern(format, width, 7, 2, Side::Front).unwrap();
            let stride = page.stride();
            let bytes = first_row(stride, 2, Side::Front);
            for (number, row) in page.data().chunks_exact(stride).enumerate() {
                let mut expected = bytes.clone();
                expected.rotate_left(number * pixel % stride);
                if let Some(last) = expected.last_mut() {
                    *last &= !padding;
                }
                assert_eq!(row, expected, "{format:?} at {width}, row {number}");
            }
        }
        // At 13 pixels, the bits cleared in the first row come back in the next.
        let page = pattern(Format::Bw1, 13, 2, 2, Side::Front).unwrap();
        let mut turned = page.data()[..2].to_vec();
        turned.rotate_left(1);
        assert_ne!(page.data()[2..], turned[..]);
    }

    #[test]
    fn a_page_is_refused_as_its_size_is() {
        // Past what a buffer can hold on any system: about 1.8e19 bytes.
        let too_large = LayoutError::TooLarge {
            width: u32::MAX,
            height: u32::MAX,
        };
        assert_eq!(
            pattern(Format::Gray8, u32::MAX, u32::MAX, 1, Side::Front),
            Err(too_large)
        );
        assert_eq!(
            pattern(Format::Rgb8, 0, 5, 1, Side::Front),
            Err(LayoutError::Empty)
        );
        // Past a 32-bit system's `isize::MAX` and within its `usize`: refused there, where
        // allocating it would panic. Here, drawn, so not tried on a 64-bit host.
        if usize::BITS == 32 {
            for (format, width, height) in [
                (Format::Gray8, 65_535, 65_537),
                (Format::Gray8, 65_536, 32_768),
                (Format::Rgb8, 40_000, 20_000),
            ] {
                assert_eq!(
                    pattern(format, width, height, 1, Side::Front),
                    Err(LayoutError::TooLarge { width, height }),
                    "{format:?} {width} by {height}"
                );
            }
        }
    }

    /// FNV-1a, 64-bit: a page's bytes in a word.
    fn fnv1a(data: &[u8]) -> u64 {
        data.iter().fold(0xcbf2_9ce4_8422_2325, |hash, &byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
        })
    }

    #[test]
    fn its_bytes_are_pinned() {
        // As a drawing written apart from this one gives them, so a change to the pattern, which
        // pages carried elsewhere are checked against, is seen.
        let bw = pattern(Format::Bw1, 13, 4, 2, Side::Front).unwrap();
        assert_eq!(bw.data(), [0xa9, 0xc8, 0xcc, 0xa8, 0xa9, 0xc8, 0xcc, 0xa8]);
        for (format, width, height, sheet, side, bytes, hash) in [
            (
                Format::Rgb16,
                37,
                5,
                3,
                Side::Back,
                1110,
                0x9f7a_edca_164d_fe2b,
            ),
            (
                Format::Gray8,
                2550,
                30,
                1,
                Side::Front,
                76_500,
                0xe066_896b_8af4_571f,
            ),
        ] {
            let page = pattern(format, width, height, sheet, side).unwrap();
            assert_eq!(page.data().len(), bytes, "{format:?}");
            assert_eq!(fnv1a(page.data()), hash, "{format:?}");
        }
    }
}
