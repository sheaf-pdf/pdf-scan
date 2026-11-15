//! The caps every backend that assembles a page from a driver's bytes holds it to (`twain`, `wia`
//! and `ica`), and the check each makes before it allocates one.
//!
//! A driver's sizes are untrusted: a page's pixels and bytes are checked against these caps, in
//! `u64` and against what one buffer can hold here ([`RawImage::data_len`]), before its buffer is
//! allocated, so a lying driver can't make a backend ask for more memory than a page needs.

use crate::{Format, LayoutError, RawImage, ScanError};

/// The most pixels a page may have: `16,384 × 16,384 = 268,435,456`.
pub(crate) const MAX_PIXELS: u64 = 1 << 28;

/// The most bytes a page's packed buffer may have (`512 MiB`): the fake backend's page ceiling,
/// within `scan-worker`'s default 1 GiB page on the wire, and leaving a 32-bit (`i686`) worker
/// address space to work in.
pub(crate) const MAX_IMAGE_BYTES: usize = 512 << 20;

fn driver_msg(msg: impl Into<String>) -> ScanError {
    ScanError::Driver {
        message: msg.into(),
        shown: false,
    }
}

/// The packed buffer's length for a `width` by `height` page in `format`, once its size is
/// checked: above 0 both ways, its pixels within [`MAX_PIXELS`], and its bytes, as
/// [`RawImage::data_len`] counts them, within [`MAX_IMAGE_BYTES`]. Each refusal is
/// [`ScanError::Driver`], saying what the scanner gave.
pub(crate) fn checked_data_len(
    format: Format,
    width: u32,
    height: u32,
) -> Result<usize, ScanError> {
    if width == 0 || height == 0 {
        return Err(driver_msg(format!(
            "the scanner reported an empty {width} by {height} image"
        )));
    }
    let pixels = u64::from(width) * u64::from(height);
    if pixels > MAX_PIXELS {
        return Err(driver_msg(format!(
            "the scanner gave a {width} by {height} image, past the {MAX_PIXELS}-pixel cap; \
             a lower resolution asks for less"
        )));
    }
    let len = RawImage::data_len(format, width, height).map_err(|err| match err {
        LayoutError::TooLarge { width, height } => driver_msg(format!(
            "the scanner gave a {width} by {height} image, more than this process can hold; \
             a lower resolution asks for less"
        )),
        other => driver_msg(format!("invalid image layout: {other}")),
    })?;
    if len > MAX_IMAGE_BYTES {
        return Err(driver_msg(format!(
            "the scanner gave a {width} by {height} image ({len} bytes), past the \
             {MAX_IMAGE_BYTES}-byte cap; a lower resolution asks for less"
        )));
    }
    Ok(len)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn is_driver(result: Result<usize, ScanError>) -> bool {
        matches!(result, Err(ScanError::Driver { shown: false, .. }))
    }

    #[test]
    fn a_page_within_the_caps_has_its_packed_length() {
        assert_eq!(checked_data_len(Format::Rgb8, 2550, 3300), Ok(25_245_000));
        assert_eq!(checked_data_len(Format::Bw1, 13, 2), Ok(4));
        // 16,384 squared is the pixel cap itself, and in black and white 32 MiB.
        assert_eq!(
            checked_data_len(Format::Bw1, 16_384, 16_384),
            Ok(16_384 / 8 * 16_384)
        );
    }

    #[test]
    fn an_empty_page_or_one_past_a_cap_is_refused() {
        assert!(is_driver(checked_data_len(Format::Gray8, 0, 10)));
        assert!(is_driver(checked_data_len(Format::Gray8, 10, 0)));
        // One row past the pixel cap.
        assert!(is_driver(checked_data_len(Format::Bw1, 16_384, 16_385)));
        // 120 million pixels, within the pixel cap, but 720 MB of Rgb16, past the byte cap.
        assert!(is_driver(checked_data_len(Format::Rgb16, 12_000, 10_000)));
        // Both caps exactly: 8,192 by 32,768 is 2^28 pixels, and in Gray16 512 MiB; a row more
        // passes both.
        assert_eq!(
            checked_data_len(Format::Gray16, 8_192, 32_768),
            Ok(MAX_IMAGE_BYTES)
        );
        assert!(is_driver(checked_data_len(Format::Gray16, 8_192, 32_769)));
    }
}
