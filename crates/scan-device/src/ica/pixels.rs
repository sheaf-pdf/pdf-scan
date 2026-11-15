//! The bands of a memory-based transfer assembled into a page in the raw layout.
//!
//! The framework sends a page in bands (`ICScannerBandData`), each with the page's size and
//! format and its own rows. Every number in a band's header is untrusted and checked in `u64`
//! before a byte is read or a page allocated: the page's size against the shared page caps once,
//! at its first band, which allocates it once; each band's rows, in order and within the page,
//! against its row length and its buffer. Each row's packed bytes are copied, its padding dropped,
//! 16-bit samples put little-endian and black and white rows' bits past the last pixel cleared.
//!
//! Five layouts are taken, each as its header's counts say: black and white at a bit a pixel,
//! gray at 8 or 16 bits, and red, green and blue interleaved at 8 or 16 bits a sample. Anything
//! else (a palette, CMY, CMYK, YUV, a fourth component, counts that disagree) is refused with the
//! header's numbers, so a Mac's first report of one says what came. Unverified on a device: gray's
//! 0 is taken as the least intensity, as Core Graphics' gray is, and black and white's 0 as black
//! ([`BW_ZERO_IS_BLACK`]); a band's colour is taken as interleaved, since its header has no plane
//! count; and a page's bands are taken to come in order, top down.

use super::driver::{BandHeader, MAX_BAND_BYTES, PIXEL_BW, PIXEL_GRAY, PIXEL_RGB};
use super::error::driver_msg;
use crate::page_caps::checked_data_len;
use crate::{Format, RawImage, ScanError};

/// Whether a black and white band's 0 is black, as the raw layout's is, rather than white.
///
/// Taken as so, since Core Graphics' gray has 0 black and the framework's documentation says
/// nothing of black and white bands; unverified on a device, and a Mac's first black and white
/// scan settles it.
pub(super) const BW_ZERO_IS_BLACK: bool = true;

/// The raw format a band's header describes: [`ScanError::Driver`] naming the header's numbers
/// for any other.
pub(super) fn format_of(header: &BandHeader) -> Result<Format, ScanError> {
    let counts = (
        header.pixel_type,
        header.bits_per_component,
        header.components,
        header.bits_per_pixel,
    );
    Ok(match counts {
        (PIXEL_BW, 1, 1, 1) => Format::Bw1,
        (PIXEL_GRAY, 8, 1, 8) => Format::Gray8,
        (PIXEL_GRAY, 16, 1, 16) => Format::Gray16,
        (PIXEL_RGB, 8, 3, 24) => Format::Rgb8,
        (PIXEL_RGB, 16, 3, 48) => Format::Rgb16,
        (pixel_type, bits_per_component, components, bits_per_pixel) => {
            return Err(driver_msg(format!(
                "the scanner sent a band of pixel type {pixel_type} at {bits_per_component} \
                 bits a component, {components} components and {bits_per_pixel} bits a pixel, \
                 a layout this backend doesn't take"
            )));
        }
    })
}

/// Refuses a band's buffer past [`MAX_BAND_BYTES`]: checked by the glue on the buffer's length
/// before it's copied, and again as a band is placed.
pub(super) fn check_band_len(len: u64) -> Result<(), ScanError> {
    if len > MAX_BAND_BYTES {
        return Err(driver_msg(format!(
            "the scanner sent a band of {len} bytes, past the {MAX_BAND_BYTES}-byte cap"
        )));
    }
    Ok(())
}

/// A packed row's bytes: `width` pixels in `format`, rounded up to whole bytes.
fn packed_row(format: Format, width: u32) -> u64 {
    let bits: u64 = match format {
        Format::Bw1 => 1,
        Format::Gray8 => 8,
        Format::Gray16 => 16,
        Format::Rgb8 => 24,
        Format::Rgb16 => 48,
    };
    (u64::from(width) * bits).div_ceil(8)
}

/// A page's width or height from a band's header: at least 1 and within `u32`.
fn dimension(value: u64, what: &str, header: &BandHeader) -> Result<u32, ScanError> {
    u32::try_from(value)
        .ok()
        .filter(|&value| value > 0)
        .ok_or_else(|| {
            driver_msg(format!(
                "the scanner sent a page {} by {} pixels, its {what} not one this backend takes",
                header.full_width, header.full_height
            ))
        })
}

/// One packed row put into the raw layout in place: 16-bit samples little-endian where they
/// came big-endian; black and white inverted where its 0 is white, and its bits past the last
/// pixel cleared.
fn fix_row(format: Format, width: u32, big_endian: bool, zero_is_black: bool, row: &mut [u8]) {
    match format {
        Format::Gray16 | Format::Rgb16 if big_endian => {
            let (samples, _) = row.as_chunks_mut::<2>();
            for sample in samples {
                sample.swap(0, 1);
            }
        }
        Format::Bw1 => {
            if !zero_is_black {
                for byte in row.iter_mut() {
                    *byte = !*byte;
                }
            }
            let used = width % 8;
            if used != 0
                && let Some(last) = row.last_mut()
            {
                *last &= 0xff_u8 << (8 - used);
            }
        }
        _ => {}
    }
}

/// A page being assembled from its bands: allocated once, at its first band, then filled top down
/// as each band is placed.
pub(super) struct PageAssembler {
    format: Format,
    width: u32,
    height: u32,
    /// A packed row's bytes, the page's row length.
    row_bytes: usize,
    /// The rows placed so far, from the top: the next band's start row.
    rows_placed: u32,
    data: Vec<u8>,
    zero_is_black: bool,
}

impl PageAssembler {
    /// The page the band `first` begins, its size and format checked against the shared page caps
    /// and its buffer allocated, before any band is placed.
    pub(super) fn new(first: &BandHeader) -> Result<PageAssembler, ScanError> {
        PageAssembler::with_polarity(first, BW_ZERO_IS_BLACK)
    }

    /// As [`PageAssembler::new`], black and white's 0 black where `zero_is_black`.
    fn with_polarity(first: &BandHeader, zero_is_black: bool) -> Result<PageAssembler, ScanError> {
        let format = format_of(first)?;
        let width = dimension(first.full_width, "width", first)?;
        let height = dimension(first.full_height, "height", first)?;
        let len = checked_data_len(format, width, height)?;
        let mut data = Vec::new();
        data.try_reserve_exact(len).map_err(|_| {
            driver_msg(format!(
                "couldn't allocate {len} bytes for a {width} by {height} page; a lower \
                 resolution asks for less"
            ))
        })?;
        data.resize(len, 0);
        // A row's bytes are at most the page's, which fit a `usize`.
        let row_bytes = packed_row(format, width) as usize;
        Ok(PageAssembler {
            format,
            width,
            height,
            row_bytes,
            rows_placed: 0,
            data,
            zero_is_black,
        })
    }

    /// The page's format.
    pub(super) fn format(&self) -> Format {
        self.format
    }

    /// The page's bytes once whole, for progress.
    pub(super) fn len(&self) -> usize {
        self.data.len()
    }

    /// The page's bytes placed so far, for progress.
    pub(super) fn placed_bytes(&self) -> usize {
        self.rows_placed as usize * self.row_bytes
    }

    /// Whether every row of the page is placed.
    pub(super) fn is_complete(&self) -> bool {
        self.rows_placed == self.height
    }

    /// Places the band `band`, whose bytes are `bytes`, after the rows placed so far.
    ///
    /// Refused, as [`ScanError::Driver`] naming the header's numbers, with nothing placed: a band
    /// of another format, width or height than the page's first; one not starting where the rows
    /// placed end (out of order, a gap, an overlap); no rows, or rows past the page's last; a row
    /// shorter than the page's; a band whose rows need more bytes than its data size, or a data
    /// size past its buffer; a buffer past [`MAX_BAND_BYTES`].
    pub(super) fn place(&mut self, band: &BandHeader, bytes: &[u8]) -> Result<(), ScanError> {
        check_band_len(bytes.len() as u64)?;
        let format = format_of(band)?;
        if format != self.format
            || band.full_width != u64::from(self.width)
            || band.full_height != u64::from(self.height)
        {
            return Err(driver_msg(format!(
                "the scanner sent a band of a {} by {} {format:?} page within a {} by {} {:?} \
                 one",
                band.full_width, band.full_height, self.width, self.height, self.format
            )));
        }
        let placed = u64::from(self.rows_placed);
        let height = u64::from(self.height);
        if band.start_row != placed {
            return Err(driver_msg(format!(
                "the scanner sent a band starting at row {} of a {height}-row page after {placed} \
                 rows: out of order, a gap or an overlap",
                band.start_row
            )));
        }
        let end = placed.checked_add(band.rows).filter(|&end| end <= height);
        let end = match end {
            Some(end) if band.rows > 0 => end,
            _ => {
                return Err(driver_msg(format!(
                    "the scanner sent a band of {} rows from row {placed} of a {height}-row page",
                    band.rows
                )));
            }
        };
        let row = self.row_bytes as u64;
        if band.bytes_per_row < row {
            return Err(driver_msg(format!(
                "the scanner sent a band of {} bytes a row, short of the {row} a row of {} \
                 {:?} pixels needs",
                band.bytes_per_row, self.width, self.format
            )));
        }
        let needed = (band.rows - 1)
            .checked_mul(band.bytes_per_row)
            .and_then(|before_last| before_last.checked_add(row));
        let len = bytes.len() as u64;
        match needed {
            Some(needed) if needed <= band.data_size && band.data_size <= len => {}
            _ => {
                return Err(driver_msg(format!(
                    "the scanner sent a band of {} rows of {} bytes in a data size of {} and a \
                     buffer of {len} bytes",
                    band.rows, band.bytes_per_row, band.data_size
                )));
            }
        }
        // Each offset below is within the page, or within `needed`, at most the buffer's length,
        // so the casts and the slices hold: a band of two rows or more has its row length within
        // `needed`, and a band of one reads its first row alone, at offset 0, whatever its row
        // length.
        let stride = band.bytes_per_row as usize;
        let first = placed as usize * self.row_bytes;
        let rows = band.rows as usize;
        let destination = &mut self.data[first..first + rows * self.row_bytes];
        for (r, out) in destination.chunks_exact_mut(self.row_bytes).enumerate() {
            let start = r * stride;
            out.copy_from_slice(&bytes[start..start + self.row_bytes]);
            fix_row(
                self.format,
                self.width,
                band.big_endian,
                self.zero_is_black,
                out,
            );
        }
        // Within the page's height, a `u32`.
        self.rows_placed = end as u32;
        Ok(())
    }

    /// The page, once every row is placed: [`ScanError::Driver`] where the scanner ended it early.
    pub(super) fn finish(self) -> Result<RawImage, ScanError> {
        if !self.is_complete() {
            return Err(driver_msg(format!(
                "the scanner ended a page early: {} of its {} rows",
                self.rows_placed, self.height
            )));
        }
        RawImage::new(self.format, self.width, self.height, self.data)
            .map_err(|err| driver_msg(format!("the scanner's page isn't a raw image: {err}")))
    }
}

#[cfg(test)]
mod tests {
    use super::super::driver::{PIXEL_CMYK, PIXEL_PALETTE, PIXEL_YUV};
    use super::*;
    use crate::page_caps::MAX_PIXELS;

    /// A band of a `width` by `height` page in `format`: `rows` rows from `start`, each
    /// `bytes_per_row` long, its data size exactly what those rows take.
    fn band(format: Format, width: u64, height: u64, start: u64, rows: u64) -> BandHeader {
        let (pixel_type, bits_per_component, components) = match format {
            Format::Bw1 => (PIXEL_BW, 1, 1),
            Format::Gray8 => (PIXEL_GRAY, 8, 1),
            Format::Gray16 => (PIXEL_GRAY, 16, 1),
            Format::Rgb8 => (PIXEL_RGB, 8, 3),
            Format::Rgb16 => (PIXEL_RGB, 16, 3),
        };
        let bits_per_pixel = bits_per_component * components;
        // Saturating, so a test can ask for sizes past any cap.
        let bytes_per_row = width.saturating_mul(bits_per_pixel).div_ceil(8);
        BandHeader {
            full_width: width,
            full_height: height,
            bits_per_pixel,
            bits_per_component,
            components,
            big_endian: false,
            pixel_type,
            bytes_per_row,
            start_row: start,
            rows,
            data_size: rows.saturating_mul(bytes_per_row),
        }
    }

    /// `band` with each row padded to `bytes_per_row`.
    fn padded(mut band: BandHeader, bytes_per_row: u64) -> BandHeader {
        band.bytes_per_row = bytes_per_row;
        band.data_size = band.rows * bytes_per_row;
        band
    }

    fn is_driver<T>(result: Result<T, ScanError>) -> bool {
        matches!(result, Err(ScanError::Driver { shown: false, .. }))
    }

    /// The page of `bands`, each with its bytes.
    fn page(bands: &[(BandHeader, Vec<u8>)]) -> Result<RawImage, ScanError> {
        let mut page = PageAssembler::new(&bands[0].0)?;
        for (header, bytes) in bands {
            page.place(header, bytes)?;
        }
        page.finish()
    }

    #[test]
    fn each_layout_is_its_format() {
        for format in [
            Format::Bw1,
            Format::Gray8,
            Format::Gray16,
            Format::Rgb8,
            Format::Rgb16,
        ] {
            assert_eq!(format_of(&band(format, 8, 1, 0, 1)), Ok(format));
        }
    }

    #[test]
    fn any_other_layout_is_refused_with_its_numbers() {
        let gray = band(Format::Gray8, 8, 1, 0, 1);
        for header in [
            BandHeader {
                pixel_type: PIXEL_PALETTE,
                ..gray
            },
            BandHeader {
                pixel_type: PIXEL_CMYK,
                bits_per_component: 8,
                components: 4,
                bits_per_pixel: 32,
                ..gray
            },
            BandHeader {
                pixel_type: PIXEL_YUV,
                ..gray
            },
            // RGB with a fourth byte a pixel, padding or alpha.
            BandHeader {
                bits_per_pixel: 32,
                ..band(Format::Rgb8, 8, 1, 0, 1)
            },
            // Counts that disagree.
            BandHeader {
                bits_per_pixel: 16,
                ..gray
            },
            BandHeader {
                components: 3,
                ..gray
            },
            BandHeader {
                bits_per_component: 8,
                bits_per_pixel: 8,
                ..band(Format::Bw1, 8, 1, 0, 1)
            },
            BandHeader {
                bits_per_component: 12,
                bits_per_pixel: 12,
                ..gray
            },
        ] {
            let err = format_of(&header).unwrap_err();
            let ScanError::Driver { message, .. } = &err else {
                panic!("{err:?}");
            };
            assert!(
                message.contains(&format!("pixel type {}", header.pixel_type)),
                "{message}"
            );
            assert!(is_driver(PageAssembler::new(&header)));
        }
    }

    #[test]
    fn a_page_comes_whole_from_its_bands_in_each_format() {
        // Gray8, 3 by 4, in bands of 1, 2 and 1 rows.
        let gray = page(&[
            (band(Format::Gray8, 3, 4, 0, 1), vec![0, 1, 2]),
            (band(Format::Gray8, 3, 4, 1, 2), vec![3, 4, 5, 6, 7, 8]),
            (band(Format::Gray8, 3, 4, 3, 1), vec![9, 10, 11]),
        ])
        .expect("a page");
        assert_eq!(
            (gray.format(), gray.width(), gray.height()),
            (Format::Gray8, 3, 4)
        );
        assert_eq!(gray.data(), (0..12).collect::<Vec<u8>>());

        // Rgb8, 2 by 1, one band: red, green and blue as given.
        let rgb =
            page(&[(band(Format::Rgb8, 2, 1, 0, 1), vec![1, 2, 3, 4, 5, 6])]).expect("a page");
        assert_eq!(rgb.data(), [1, 2, 3, 4, 5, 6]);

        // Rgb16, little-endian already: as given.
        let rgb16 = page(&[(
            band(Format::Rgb16, 1, 1, 0, 1),
            vec![0x01, 0x02, 0x03, 0x04, 0x05, 0x06],
        )])
        .expect("a page");
        assert_eq!(rgb16.data(), [0x01, 0x02, 0x03, 0x04, 0x05, 0x06]);
    }

    #[test]
    fn sixteen_bit_samples_come_little_endian_from_either_order() {
        let little = band(Format::Gray16, 2, 1, 0, 1);
        let big = BandHeader {
            big_endian: true,
            ..little
        };
        // 0x1234 and 0xabcd, each in the band's order.
        let from_little = page(&[(little, vec![0x34, 0x12, 0xcd, 0xab])]).expect("a page");
        let from_big = page(&[(big, vec![0x12, 0x34, 0xab, 0xcd])]).expect("a page");
        assert_eq!(from_little.data(), [0x34, 0x12, 0xcd, 0xab]);
        assert_eq!(from_big.data(), from_little.data());

        let rgb_big = BandHeader {
            big_endian: true,
            ..band(Format::Rgb16, 1, 1, 0, 1)
        };
        let rgb = page(&[(rgb_big, vec![0x00, 0x01, 0x00, 0x02, 0x00, 0x03])]).expect("a page");
        assert_eq!(rgb.data(), [0x01, 0x00, 0x02, 0x00, 0x03, 0x00]);

        // Eight-bit samples are never swapped, whatever the header says.
        let gray_big = BandHeader {
            big_endian: true,
            ..band(Format::Gray8, 2, 1, 0, 1)
        };
        assert_eq!(
            page(&[(gray_big, vec![1, 2])]).expect("a page").data(),
            [1, 2]
        );
    }

    #[test]
    fn a_rows_padding_is_dropped() {
        // Gray8, 3 wide, each row padded to 8 bytes; the last row's padding past the data size.
        let header = padded(band(Format::Gray8, 3, 2, 0, 2), 8);
        let mut bytes = vec![0xee; 16];
        bytes[..3].copy_from_slice(&[1, 2, 3]);
        bytes[8..11].copy_from_slice(&[4, 5, 6]);
        let image = page(&[(header, bytes)]).expect("a page");
        assert_eq!(image.data(), [1, 2, 3, 4, 5, 6]);

        // The last row needs only its packed bytes: a data size short of its padding is taken.
        let short = BandHeader {
            data_size: 8 + 3,
            ..header
        };
        let image = page(&[(short, vec![1, 2, 3, 0, 0, 0, 0, 0, 4, 5, 6])]).expect("a page");
        assert_eq!(image.data(), [1, 2, 3, 4, 5, 6]);
    }

    #[test]
    fn black_and_white_rows_have_their_last_bits_cleared_at_every_width() {
        for width in 1..=17_u32 {
            let row = (width as usize).div_ceil(8);
            // Every bit set, those past the last pixel too.
            let header = band(Format::Bw1, u64::from(width), 2, 0, 2);
            let image = page(&[(header, vec![0xff; 2 * row])]).expect("a page");
            let used = width % 8;
            let last = if used == 0 {
                0xff
            } else {
                0xff_u8 << (8 - used)
            };
            for bytes in image.data().chunks_exact(row) {
                assert!(bytes[..row - 1].iter().all(|&b| b == 0xff), "{width}");
                assert_eq!(bytes[row - 1], last, "{width}");
            }
        }
    }

    #[test]
    fn black_and_white_is_inverted_only_where_its_zero_is_white() {
        let header = band(Format::Bw1, 12, 1, 0, 1);
        let bytes = [0b1010_0000, 0b1100_1111];
        let mut as_is = PageAssembler::with_polarity(&header, true).expect("a page");
        as_is.place(&header, &bytes).expect("placed");
        assert_eq!(
            as_is.finish().expect("a page").data(),
            [0b1010_0000, 0b1100_0000]
        );
        let mut inverted = PageAssembler::with_polarity(&header, false).expect("a page");
        inverted.place(&header, &bytes).expect("placed");
        assert_eq!(
            inverted.finish().expect("a page").data(),
            [0b0101_1111, 0b0011_0000]
        );
    }

    #[test]
    fn a_band_out_of_order_a_gap_or_an_overlap_is_refused() {
        let first = band(Format::Gray8, 2, 4, 0, 2);
        for start in [0, 1, 3, 4] {
            let mut page = PageAssembler::new(&first).expect("a page");
            page.place(&first, &[0; 4]).expect("the first band");
            let next = band(Format::Gray8, 2, 4, start, 1);
            assert!(is_driver(page.place(&next, &[0; 2])), "start {start}");
            // Nothing placed by the refused band.
            assert_eq!(page.placed_bytes(), 4);
        }
        // A first band that doesn't start at the top.
        let mut page = PageAssembler::new(&first).expect("a page");
        assert!(is_driver(
            page.place(&band(Format::Gray8, 2, 4, 1, 1), &[0; 2])
        ));
        assert_eq!(page.placed_bytes(), 0);
    }

    #[test]
    fn a_band_unlike_its_page_or_past_its_rows_is_refused() {
        let first = band(Format::Gray8, 4, 4, 0, 1);
        let refused = [
            // Another format, width or height.
            (band(Format::Rgb8, 4, 4, 1, 1), 12),
            (band(Format::Gray8, 5, 4, 1, 1), 5),
            (band(Format::Gray8, 4, 5, 1, 1), 4),
            // No rows, or past the last.
            (band(Format::Gray8, 4, 4, 1, 0), 0),
            (band(Format::Gray8, 4, 4, 1, 4), 16),
            (
                BandHeader {
                    rows: u64::MAX,
                    ..band(Format::Gray8, 4, 4, 1, 1)
                },
                4,
            ),
            // A row shorter than the page's.
            (padded(band(Format::Gray8, 4, 4, 1, 1), 3), 4),
            // More rows than the data size holds, or a data size past the buffer.
            (
                BandHeader {
                    data_size: 7,
                    ..band(Format::Gray8, 4, 4, 1, 2)
                },
                8,
            ),
            (band(Format::Gray8, 4, 4, 1, 2), 7),
            // A row length whose product overflows.
            (
                BandHeader {
                    bytes_per_row: u64::MAX / 2,
                    data_size: u64::MAX,
                    ..band(Format::Gray8, 4, 4, 1, 3)
                },
                12,
            ),
        ];
        for (header, len) in refused {
            let mut page = PageAssembler::new(&first).expect("a page");
            page.place(&first, &[0; 4]).expect("the first band");
            assert!(is_driver(page.place(&header, &vec![0; len])), "{header:?}");
            assert_eq!(page.placed_bytes(), 4, "{header:?}");
            assert!(!page.is_complete());
        }
    }

    #[test]
    fn a_page_ended_early_is_refused() {
        let first = band(Format::Gray8, 2, 3, 0, 2);
        let mut page = PageAssembler::new(&first).expect("a page");
        page.place(&first, &[0; 4]).expect("placed");
        assert!(!page.is_complete());
        assert_eq!((page.placed_bytes(), page.len()), (4, 6));
        assert_eq!(page.format(), Format::Gray8);
        let err = page.finish().unwrap_err();
        assert_eq!(
            err,
            driver_msg("the scanner ended a page early: 2 of its 3 rows")
        );
    }

    #[test]
    fn the_caps_are_checked_before_the_page_is_allocated() {
        for (width, height) in [
            (0, 10),
            (10, 0),
            (u64::from(u32::MAX) + 1, 1),
            (1, u64::from(u32::MAX) + 1),
            (u64::MAX, u64::MAX),
        ] {
            assert!(
                is_driver(PageAssembler::new(&band(
                    Format::Gray8,
                    width,
                    height,
                    0,
                    1
                ))),
                "{width} by {height}"
            );
        }
        // One row past the pixel cap.
        let side = 1_u64 << 14;
        assert_eq!(side * side, MAX_PIXELS);
        assert!(is_driver(PageAssembler::new(&band(
            Format::Bw1,
            side,
            side + 1,
            0,
            1
        ))));
        // Within the pixel cap, past the byte cap: 120 million Rgb16 pixels, 720 MB of the
        // 512 MiB `MAX_IMAGE_BYTES`.
        assert!(is_driver(PageAssembler::new(&band(
            Format::Rgb16,
            12_000,
            10_000,
            0,
            1
        ))));
    }

    #[test]
    fn a_band_past_its_cap_is_refused_before_it_is_read() {
        assert_eq!(check_band_len(MAX_BAND_BYTES), Ok(()));
        assert!(is_driver(check_band_len(MAX_BAND_BYTES + 1)));
        assert!(is_driver(check_band_len(u64::MAX)));
    }
}
