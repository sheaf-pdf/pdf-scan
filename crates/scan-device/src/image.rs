//! The raw layout: a page's pixels as [`RawImage`] holds them, checked as it's made.

use std::fmt;

/// A raw format: how a [`RawImage`] holds its pixels, and what a scan asks for.
///
/// Each is laid out as the crate's docs say ("The raw layout"): rows top down and packed, 0 the
/// least intensity, 16-bit samples little-endian.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Format {
    /// Black and white, a bit a pixel, eight pixels a byte with the leftmost in its most
    /// significant bit; 0 is black. Each row is rounded up to whole bytes, its bits past the last
    /// pixel 0.
    Bw1,
    /// Gray, a byte a pixel; 0 is black.
    Gray8,
    /// Gray, two bytes a pixel, little-endian; 0 is black.
    Gray16,
    /// Color, three bytes a pixel: red, green and blue.
    Rgb8,
    /// Color, six bytes a pixel: red, green and blue, each two bytes, little-endian.
    Rgb16,
}

impl Format {
    /// Bits a pixel.
    const fn bits(self) -> u64 {
        match self {
            Format::Bw1 => 1,
            Format::Gray8 => 8,
            Format::Gray16 => 16,
            Format::Rgb8 => 24,
            Format::Rgb16 => 48,
        }
    }
}

/// A page's pixels in the raw layout, checked as it's made.
///
/// Private fields: `RawImage::new` checks them. `Debug` by hand: the format, the size, the length.
#[derive(Clone, PartialEq, Eq)]
pub struct RawImage {
    format: Format,
    width: u32,
    height: u32,
    data: Vec<u8>,
}

/// Why [`RawImage::new`] or [`RawImage::data_len`] refused an image.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum LayoutError {
    /// A width or a height of 0.
    #[error("an image has at least one row and one column")]
    Empty,
    /// More bytes than `u64` counts, or than this platform's `usize` holds.
    #[error("{width} by {height} is more than this system can hold")]
    TooLarge {
        /// The width asked for, in pixels.
        width: u32,
        /// The height asked for, in pixels.
        height: u32,
    },
    /// Data of another length than the format and the size make.
    #[error("{expected} bytes expected, {got} given")]
    Length {
        /// The bytes the format and the size make.
        expected: u64,
        /// The bytes given.
        got: u64,
    },
    /// A [`Format::Bw1`] row with a bit set past its last pixel: the first such row.
    #[error("row {row}'s bits past its last pixel aren't 0")]
    Padding {
        /// The row, counted from 0 at the top.
        row: u32,
    },
}

/// What this platform's `usize` holds, the most bytes an image's buffer can have here.
const PLATFORM_LIMIT: u64 = usize::MAX as u64;

/// A row's bytes in `format` at `width`: bits a pixel, rounded up to whole bytes.
fn row_bytes(format: Format, width: u32) -> Option<u64> {
    format
        .bits()
        .checked_mul(u64::from(width))
        .map(|bits| bits.div_ceil(8))
}

/// A row's bytes and the image's, in `format` at `width` by `height`: [`LayoutError::Empty`]
/// where either is 0, [`LayoutError::TooLarge`] where the image's bytes pass `u64` or `limit`.
///
/// `limit` is the platform's ([`PLATFORM_LIMIT`]) but in tests, which pass a 32-bit `usize`'s on
/// any host.
fn layout(format: Format, width: u32, height: u32, limit: u64) -> Result<(u64, u64), LayoutError> {
    if width == 0 || height == 0 {
        return Err(LayoutError::Empty);
    }
    let too_large = || LayoutError::TooLarge { width, height };
    let row = row_bytes(format, width).ok_or_else(too_large)?;
    let total = row
        .checked_mul(u64::from(height))
        .filter(|&total| total <= limit)
        .ok_or_else(too_large)?;
    Ok((row, total))
}

impl RawImage {
    /// The image of `format`, `width` by `height` pixels, holding `data`, laid out as the crate's
    /// docs say ("The raw layout").
    ///
    /// Refused, in this order: a width or a height of 0 ([`LayoutError::Empty`]); more bytes than
    /// `u64` counts or than this platform's `usize` holds ([`LayoutError::TooLarge`]); `data` of
    /// any other length than the format and the size make ([`LayoutError::Length`]); and in
    /// [`Format::Bw1`], a row with a bit set past its last pixel ([`LayoutError::Padding`]).
    ///
    /// ```
    /// use scan_device::{Format, LayoutError, RawImage};
    ///
    /// // 13 pixels a row in black and white: two bytes, the last three bits of each row 0.
    /// let image = RawImage::new(Format::Bw1, 13, 2, vec![0xff, 0xf8, 0x00, 0x00])?;
    /// assert_eq!(image.stride(), 2);
    /// assert_eq!(
    ///     RawImage::new(Format::Bw1, 13, 2, vec![0xff, 0xf8, 0x00, 0x01]),
    ///     Err(LayoutError::Padding { row: 1 }),
    /// );
    /// assert_eq!(
    ///     RawImage::new(Format::Rgb8, 2, 2, vec![0; 11]),
    ///     Err(LayoutError::Length { expected: 12, got: 11 }),
    /// );
    /// # Ok::<(), LayoutError>(())
    /// ```
    pub fn new(
        format: Format,
        width: u32,
        height: u32,
        data: Vec<u8>,
    ) -> Result<RawImage, LayoutError> {
        let expected = RawImage::data_len(format, width, height)?;
        if data.len() != expected {
            return Err(LayoutError::Length {
                expected: expected as u64,
                got: data.len() as u64,
            });
        }
        let image = RawImage {
            format,
            width,
            height,
            data,
        };
        let pixels_in_last_byte = width % 8;
        if format == Format::Bw1 && pixels_in_last_byte != 0 {
            // The low bits of each row's last byte, past its last pixel.
            let padding = 0xff_u8 >> pixels_in_last_byte;
            let rows = image.data.chunks_exact(image.stride());
            for (row, bytes) in (0..height).zip(rows) {
                if bytes.last().is_some_and(|last| last & padding != 0) {
                    return Err(LayoutError::Padding { row });
                }
            }
        }
        Ok(image)
    }

    /// The bytes a `RawImage` of `format`, `width` by `height` pixels holds, refused as
    /// [`RawImage::new`] refuses the size ([`LayoutError::Empty`], [`LayoutError::TooLarge`]):
    /// for a reader to cap a page's size before it reads its bytes, and allocate once.
    ///
    /// ```
    /// use scan_device::{Format, RawImage};
    ///
    /// // Letter at 300 dpi in color.
    /// assert_eq!(RawImage::data_len(Format::Rgb8, 2550, 3300)?, 25_245_000);
    /// // A row of 13 black and white pixels is two bytes.
    /// assert_eq!(RawImage::data_len(Format::Bw1, 13, 2)?, 4);
    /// # Ok::<(), scan_device::LayoutError>(())
    /// ```
    pub fn data_len(format: Format, width: u32, height: u32) -> Result<usize, LayoutError> {
        let (_, total) = layout(format, width, height, PLATFORM_LIMIT)?;
        usize::try_from(total).map_err(|_| LayoutError::TooLarge { width, height })
    }

    /// Its format.
    pub fn format(&self) -> Format {
        self.format
    }

    /// Its width, in pixels.
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Its height, in pixels: its rows.
    pub fn height(&self) -> u32 {
        self.height
    }

    /// A row's bytes, from the format and the width; the rows follow each other with nothing
    /// between them.
    pub fn stride(&self) -> usize {
        // A row's bytes are at most the image's, which `new` found to fit a `usize`.
        row_bytes(self.format, self.width).map_or(0, |row| row as usize)
    }

    /// Its pixels, rows top down.
    pub fn data(&self) -> &[u8] {
        &self.data
    }

    /// Its pixels, rows top down, taken out.
    pub fn into_data(self) -> Vec<u8> {
        self.data
    }
}

impl fmt::Debug for RawImage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Never the bytes: a page can be a hundred megabytes.
        f.debug_struct("RawImage")
            .field("format", &self.format)
            .field("width", &self.width)
            .field("height", &self.height)
            .field("len", &self.data.len())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FORMATS: [Format; 5] = [
        Format::Bw1,
        Format::Gray8,
        Format::Gray16,
        Format::Rgb8,
        Format::Rgb16,
    ];

    /// A 32-bit `usize`'s limit, passed to `layout` on any host.
    const LIMIT_32: u64 = u32::MAX as u64;

    #[test]
    fn each_format_takes_its_whole_layout_and_no_other_length() {
        // 3 by 2 pixels: a row of 1, 3, 6, 9 and 18 bytes.
        for (format, stride) in FORMATS.into_iter().zip([1, 3, 6, 9, 18]) {
            let len = stride * 2;
            assert_eq!(RawImage::data_len(format, 3, 2), Ok(len), "{format:?}");
            let data: Vec<u8> = match format {
                // The bits past the third pixel 0.
                Format::Bw1 => vec![0b1010_0000, 0b0110_0000],
                _ => (0..len).map(|i| i as u8).collect(),
            };
            let image = RawImage::new(format, 3, 2, data.clone()).expect("the whole layout");
            assert_eq!(image.format(), format);
            assert_eq!(image.width(), 3);
            assert_eq!(image.height(), 2);
            assert_eq!(image.stride(), stride, "{format:?}");
            assert_eq!(image.data(), &data[..]);
            assert_eq!(image.clone().into_data(), data);

            let short = RawImage::new(format, 3, 2, vec![0; len - 1]);
            let long = RawImage::new(format, 3, 2, vec![0; len + 1]);
            let length = |got: usize| LayoutError::Length {
                expected: len as u64,
                got: got as u64,
            };
            assert_eq!(short, Err(length(len - 1)), "{format:?}");
            assert_eq!(long, Err(length(len + 1)), "{format:?}");
        }
    }

    #[test]
    fn a_row_is_its_bits_rounded_up_to_whole_bytes() {
        for (width, stride) in [(1, 1), (7, 1), (8, 1), (9, 2), (13, 2), (16, 2), (17, 3)] {
            assert_eq!(
                RawImage::data_len(Format::Bw1, width, 3),
                Ok(stride * 3),
                "{width}"
            );
            let image = RawImage::new(Format::Bw1, width, 3, vec![0; stride * 3]).unwrap();
            assert_eq!(image.stride(), stride, "{width}");
        }
        // Two bytes a pixel and more.
        assert_eq!(RawImage::data_len(Format::Gray16, 5, 1), Ok(10));
        assert_eq!(RawImage::data_len(Format::Rgb16, 5, 1), Ok(30));
    }

    #[test]
    fn no_row_or_no_column_is_empty() {
        for format in FORMATS {
            for (width, height) in [(0, 1), (1, 0), (0, 0), (0, u32::MAX), (u32::MAX, 0)] {
                assert_eq!(
                    RawImage::new(format, width, height, Vec::new()),
                    Err(LayoutError::Empty)
                );
                assert_eq!(
                    RawImage::data_len(format, width, height),
                    Err(LayoutError::Empty)
                );
            }
        }
    }

    #[test]
    fn a_set_padding_bit_is_refused_and_its_row_named() {
        // 13 pixels: the second byte holds 5, its 3 low bits padding.
        let mut data = vec![0xff, 0xf8, 0x00, 0x00, 0xaa, 0xa8];
        assert!(RawImage::new(Format::Bw1, 13, 3, data.clone()).is_ok());
        for bit in 0..3 {
            data[3] = 1 << bit;
            assert_eq!(
                RawImage::new(Format::Bw1, 13, 3, data.clone()),
                Err(LayoutError::Padding { row: 1 }),
                "bit {bit}"
            );
        }
        // The first such row.
        data[5] |= 1;
        assert_eq!(
            RawImage::new(Format::Bw1, 13, 3, data.clone()),
            Err(LayoutError::Padding { row: 1 })
        );
        data[3] = 0;
        assert_eq!(
            RawImage::new(Format::Bw1, 13, 3, data),
            Err(LayoutError::Padding { row: 2 })
        );
        // The last pixel's bit isn't padding.
        assert!(RawImage::new(Format::Bw1, 13, 1, vec![0x00, 0x08]).is_ok());
        // A width of whole bytes has none, and other formats no padding.
        assert!(RawImage::new(Format::Bw1, 16, 1, vec![0xff, 0xff]).is_ok());
        assert!(RawImage::new(Format::Gray8, 13, 1, vec![0xff; 13]).is_ok());
    }

    #[test]
    fn a_size_past_u64_is_too_large() {
        // At u32::MAX by u32::MAX, two bytes a pixel or more pass u64.
        for format in [Format::Gray16, Format::Rgb8, Format::Rgb16] {
            let too_large = LayoutError::TooLarge {
                width: u32::MAX,
                height: u32::MAX,
            };
            assert_eq!(
                layout(format, u32::MAX, u32::MAX, u64::MAX),
                Err(too_large.clone()),
                "{format:?}"
            );
            assert_eq!(
                RawImage::data_len(format, u32::MAX, u32::MAX),
                Err(too_large.clone())
            );
            assert_eq!(
                RawImage::new(format, u32::MAX, u32::MAX, Vec::new()),
                Err(too_large)
            );
        }
        // One byte a pixel or less fits u64 (at most 2^64 - 2^33 + 1 bytes), so the platform's
        // limit or the length refuses it.
        let side = u64::from(u32::MAX);
        assert_eq!(
            layout(Format::Gray8, u32::MAX, u32::MAX, u64::MAX),
            Ok((side, side * side))
        );
        assert_eq!(
            layout(Format::Bw1, u32::MAX, u32::MAX, u64::MAX),
            Ok((1 << 29, (1 << 29) * side))
        );
        for format in [Format::Bw1, Format::Gray8] {
            let refused = RawImage::new(format, u32::MAX, u32::MAX, Vec::new());
            if usize::BITS == 64 {
                assert!(
                    matches!(refused, Err(LayoutError::Length { got: 0, .. })),
                    "{refused:?}"
                );
            } else {
                assert_eq!(
                    refused,
                    Err(LayoutError::TooLarge {
                        width: u32::MAX,
                        height: u32::MAX
                    })
                );
            }
        }
    }

    #[test]
    fn a_size_past_a_32_bit_usize_is_too_large_there() {
        // 65,536 by 65,536 in Rgb16 is 6 * 2^32 bytes: 0 in wrapping 32-bit arithmetic, which an
        // empty buffer would match.
        let too_large = LayoutError::TooLarge {
            width: 65_536,
            height: 65_536,
        };
        assert_eq!(
            layout(Format::Rgb16, 65_536, 65_536, LIMIT_32),
            Err(too_large.clone())
        );
        assert_eq!(
            layout(Format::Rgb16, 65_536, 65_536, u64::MAX),
            Ok((6 * 65_536, 6 << 32))
        );
        // Through `new`, the platform's own limit.
        let refused = RawImage::new(Format::Rgb16, 65_536, 65_536, Vec::new());
        if usize::BITS == 32 {
            assert_eq!(refused, Err(too_large));
        } else {
            assert_eq!(
                refused,
                Err(LayoutError::Length {
                    expected: 6 << 32,
                    got: 0
                })
            );
        }
        // The limit itself is held, and a byte past it refused.
        assert_eq!(
            layout(Format::Gray8, 65_535, 65_537, LIMIT_32),
            Ok((65_535, LIMIT_32))
        );
        assert_eq!(
            layout(Format::Gray8, 65_536, 65_536, LIMIT_32),
            Err(LayoutError::TooLarge {
                width: 65_536,
                height: 65_536
            })
        );
        // And at the largest sizes, every format past u64 or past the 32-bit limit.
        for format in FORMATS {
            assert_eq!(
                layout(format, u32::MAX, u32::MAX, LIMIT_32),
                Err(LayoutError::TooLarge {
                    width: u32::MAX,
                    height: u32::MAX
                }),
                "{format:?}"
            );
        }
    }

    #[test]
    fn data_len_agrees_with_new() {
        for format in FORMATS {
            for (width, height) in [(1, 1), (3, 2), (13, 5), (2550, 1), (8, 300)] {
                let len = RawImage::data_len(format, width, height).unwrap();
                let image = RawImage::new(format, width, height, vec![0; len]).unwrap();
                assert_eq!(image.data().len(), len);
                assert_eq!(image.stride() * height as usize, len);
            }
        }
        assert_eq!(
            RawImage::data_len(Format::Rgb8, 2550, 3300),
            Ok(2550 * 3 * 3300)
        );
        assert_eq!(
            RawImage::data_len(Format::Bw1, 2550, 3300),
            Ok(2550usize.div_ceil(8) * 3300)
        );
    }

    #[test]
    fn debug_gives_the_size_and_length_never_a_byte() {
        let image = RawImage::new(Format::Gray8, 3, 2, vec![123; 6]).unwrap();
        let debug = format!("{image:?}");
        assert_eq!(
            debug,
            "RawImage { format: Gray8, width: 3, height: 2, len: 6 }"
        );
        assert!(!debug.contains("123"), "{debug}");
    }

    #[test]
    fn layout_errors_say_what_was_refused() {
        assert_eq!(
            LayoutError::Empty.to_string(),
            "an image has at least one row and one column"
        );
        assert_eq!(
            LayoutError::TooLarge {
                width: 7,
                height: 9
            }
            .to_string(),
            "7 by 9 is more than this system can hold"
        );
        assert_eq!(
            LayoutError::Length {
                expected: 12,
                got: 11
            }
            .to_string(),
            "12 bytes expected, 11 given"
        );
        assert_eq!(
            LayoutError::Padding { row: 4 }.to_string(),
            "row 4's bits past its last pixel aren't 0"
        );
    }
}
