//! The pages the fake draws.

use crate::{Format, LayoutError, RawImage, Side};

/// The page the fake draws for `side` of sheet `sheet`, `width` by `height` pixels in `format`:
/// the same bytes on every run and every system, different for each sheet and side, in the raw
/// layout (in [`Format::Bw1`], each row's bits past its last pixel 0).
///
/// Its first row is bytes made from the sheet's number and the side, and each row after it is the
/// row before turned left by one pixel (a byte, in `Bw1`), so drawing a page costs little more
/// than copying its bytes. It's public so that a page carried elsewhere can be checked byte for
/// byte against the one drawn here. The page's bytes are allocated at once: refused as
/// [`RawImage::data_len`] refuses its size, it's otherwise drawn whatever that size.
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
/// 17), each step's state giving eight bytes.
fn first_row(stride: usize, sheet: u32, side: Side) -> Vec<u8> {
    let side = match side {
        Side::Front => 0,
        Side::Back => 1,
    };
    // Never 0, which the steps would keep: the constant's high bits are set.
    let mut state = ((u64::from(sheet) << 1) | side) ^ 0x9e37_79b9_7f4a_7c15;
    let mut row = Vec::with_capacity(stride + 8);
    while row.len() < stride {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        row.extend_from_slice(&state.to_le_bytes());
    }
    row.truncate(stride);
    row
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_row_is_the_one_before_turned_left_a_pixel() {
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
