//! Decoding native-transfer DIBs and assembling memory-transfer strips or tiles (including
//! undefined-size transfers where state 6 reports `-1` width or length) into [`RawImage`].
//!
//! Pure safe Rust: every header field, palette index, row stride, tile offset, and dynamic growth
//! step is checked in `u64` / `usize` arithmetic against [`MAX_PIXELS`] and
//! [`RawImage::data_len`] before allocating or indexing.

use twain_dsm::ffi;

use super::error::driver_msg;
use crate::{Format, LayoutError, RawImage, ScanError};

/// Maximum pixels per image (`16,384 × 16,384 = 268,435,456`). `RawImage::data_len` additionally
/// caps byte length at `isize::MAX` (`2 GiB - 1` in a 32-bit worker).
pub(super) const MAX_PIXELS: u64 = 1 << 28;

/// Maximum native-transfer DIB block accepted: up to 4 bytes per pixel at `MAX_PIXELS` (a 1-pixel
/// wide 24-bit row padded to 4 bytes) plus 64 KiB for header and palette.
pub(super) const MAX_DIB_BYTES: u64 = 4 * MAX_PIXELS + (1 << 16);

/// Which sample value represents black in `Bw1`, `Gray8`, and `Gray16` memory transfers
/// (`ICAP_PIXELFLAVOR`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Polarity {
    /// `TWPF_CHOCOLATE`: `0` is black / minimum intensity (matches `RawImage`).
    ZeroBlack,
    /// `TWPF_VANILLA`: `0` is white / maximum intensity (inverted for `Bw1`, `Gray8`, `Gray16`;
    /// never applied to `Rgb8` or `Rgb16`).
    ZeroWhite,
}

impl Polarity {
    /// Maps a `TWPF_*` constant to [`Polarity`], defaulting unknown values to `ZeroBlack`.
    pub(super) fn from_twpf(raw: u16) -> Polarity {
        if raw == ffi::TWPF_VANILLA {
            Polarity::ZeroWhite
        } else {
            Polarity::ZeroBlack
        }
    }
}

/// Platform-independent view of `DAT_IMAGEINFO` fields needed for pixel assembly.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct ImageDesc {
    pub width: i32,
    pub length: i32,
    pub samples_per_pixel: i16,
    pub bits_per_sample: i16,
    pub bits_per_pixel: i16,
    pub planar: bool,
    pub pixel_type: i16,
    pub compression: u16,
}

#[cfg(windows)]
impl From<twain_dsm::ImageInfo> for ImageDesc {
    fn from(info: twain_dsm::ImageInfo) -> Self {
        ImageDesc {
            width: info.width,
            length: info.length,
            samples_per_pixel: info.samples_per_pixel,
            bits_per_sample: info.bits_per_sample[0],
            bits_per_pixel: info.bits_per_pixel,
            planar: info.planar,
            pixel_type: info.pixel_type,
            compression: info.compression,
        }
    }
}

/// Platform-independent view of a `DAT_IMAGEMEMXFER` strip or tile.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct StripDesc<'a> {
    pub compression: u16,
    pub bytes_per_row: u32,
    pub columns: u32,
    pub rows: u32,
    pub x_offset: u32,
    pub y_offset: u32,
    pub bytes: &'a [u8],
}

#[cfg(windows)]
impl<'a> From<twain_dsm::Strip<'a>> for StripDesc<'a> {
    fn from(strip: twain_dsm::Strip<'a>) -> Self {
        StripDesc {
            compression: strip.compression,
            bytes_per_row: strip.bytes_per_row,
            columns: strip.columns,
            rows: strip.rows,
            x_offset: strip.x_offset,
            y_offset: strip.y_offset,
            bytes: strip.bytes,
        }
    }
}

/// Packed unpadded byte length of one row of `width` pixels in `format`.
fn row_bytes(format: Format, width: u32) -> u64 {
    let bits: u64 = match format {
        Format::Bw1 => 1,
        Format::Gray8 => 8,
        Format::Gray16 => 16,
        Format::Rgb8 => 24,
        Format::Rgb16 => 48,
    };
    (u64::from(width) * bits).div_ceil(8)
}

/// Validates `(format, width, height)` against [`MAX_PIXELS`] and [`RawImage::data_len`], returning
/// the exact packed buffer byte length.
fn checked_data_len(format: Format, width: u32, height: u32) -> Result<usize, ScanError> {
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
    RawImage::data_len(format, width, height).map_err(|err| match err {
        LayoutError::TooLarge { width, height } => driver_msg(format!(
            "the scanner gave a {width} by {height} image, more than this process can hold; \
             a lower resolution asks for less"
        )),
        other => driver_msg(format!("invalid image layout: {other}")),
    })
}

/// Maps `DAT_IMAGEINFO` to [`Format`], refusing compressed, planar multi-channel, or unrecognized
/// layouts.
pub(super) fn format_from_info(info: &ImageDesc) -> Result<Format, ScanError> {
    if info.compression != ffi::TWCP_NONE {
        return Err(driver_msg(format!(
            "the scanner sent compressed image data (TWCP {:#06x}) when uncompressed was set",
            info.compression
        )));
    }
    if info.samples_per_pixel > 1 && info.planar {
        return Err(driver_msg(
            "the scanner sent planar color channels when chunky pixels were set",
        ));
    }
    match (
        u16::try_from(info.pixel_type),
        info.samples_per_pixel,
        info.bits_per_sample,
        info.bits_per_pixel,
    ) {
        (Ok(ffi::TWPT_BW), 1, 1, 1) => Ok(Format::Bw1),
        (Ok(ffi::TWPT_GRAY), 1, 8, 8) => Ok(Format::Gray8),
        (Ok(ffi::TWPT_GRAY), 1, 16, 16) => Ok(Format::Gray16),
        (Ok(ffi::TWPT_RGB), 3, 8, 24) => Ok(Format::Rgb8),
        (Ok(ffi::TWPT_RGB), 3, 16, 48) => Ok(Format::Rgb16),
        _ => Err(driver_msg(format!(
            "unsupported TWAIN pixel layout (pixel type {}, {} samples, {} bits/sample, {} bits/pixel)",
            info.pixel_type, info.samples_per_pixel, info.bits_per_sample, info.bits_per_pixel
        ))),
    }
}

/// Decodes a Windows packed DIB (`BITMAPINFOHEADER` or larger header, color table, 4-byte padded
/// rows) from a native transfer (`DAT_IMAGENATIVEXFER`) into a checked [`RawImage`].
pub(super) fn decode_dib(bytes: &[u8]) -> Result<RawImage, ScanError> {
    if bytes.is_empty() {
        return Err(driver_msg("the scanner returned an empty DIB handle"));
    }
    if (bytes.len() as u64) > MAX_DIB_BYTES {
        return Err(driver_msg(format!(
            "the scanner's DIB is {} bytes, past the {MAX_DIB_BYTES}-byte cap; \
             a lower resolution asks for less",
            bytes.len()
        )));
    }
    if bytes.len() < 40 {
        return Err(driver_msg(format!(
            "the scanner's DIB is {} bytes, shorter than a 40-byte BITMAPINFOHEADER",
            bytes.len()
        )));
    }
    let header_size = u32_at(bytes, 0);
    let width = i32_at(bytes, 4);
    let height = i32_at(bytes, 8);
    let planes = u16_at(bytes, 12);
    let bits = u16_at(bytes, 14);
    let compression = u32_at(bytes, 16);
    let colors_used = u32_at(bytes, 32);

    if header_size < 40 || u64::from(header_size) > bytes.len() as u64 {
        return Err(driver_msg(format!(
            "the scanner's DIB header size ({header_size}) is invalid"
        )));
    }
    if planes != 1 {
        return Err(driver_msg(format!(
            "the scanner's DIB has {planes} planes, expected 1"
        )));
    }
    if compression != 0 {
        return Err(driver_msg(format!(
            "the scanner's DIB uses compression {compression}, expected BI_RGB (0)"
        )));
    }
    let format = match bits {
        1 => Format::Bw1,
        8 => Format::Gray8,
        24 => Format::Rgb8,
        other => {
            return Err(driver_msg(format!(
                "the scanner's DIB uses {other} bits per pixel, expected 1, 8, or 24"
            )));
        }
    };
    if width <= 0 || height == 0 || height == i32::MIN {
        return Err(driver_msg(format!(
            "the scanner's DIB has invalid dimensions ({width} by {height})"
        )));
    }
    let (w, h) = (width as u32, height.unsigned_abs());
    let bottom_up = height > 0;
    let total = checked_data_len(format, w, h)?;

    let max_entries = if bits <= 8 { 1u32 << bits } else { 256 };
    let entries = match colors_used {
        0 if bits <= 8 => max_entries,
        0 => 0,
        n => n,
    };
    if entries > max_entries {
        return Err(driver_msg(format!(
            "the scanner's DIB palette has {entries} entries, past {max_entries} for {bits}-bit data"
        )));
    }

    let palette_at = u64::from(header_size);
    let rows_at = palette_at + u64::from(entries) * 4;
    let dib_stride = (u64::from(w) * u64::from(bits)).div_ceil(32) * 4;
    let need = rows_at + dib_stride * u64::from(h);
    if (bytes.len() as u64) < need {
        return Err(driver_msg(format!(
            "the scanner's DIB holds {} bytes, and its rows need {need}",
            bytes.len()
        )));
    }

    let palette: Vec<[u8; 3]> = (0..entries as usize)
        .map(|i| {
            let at = palette_at as usize + i * 4;
            [bytes[at + 2], bytes[at + 1], bytes[at]]
        })
        .collect();

    let invert_bw = match format {
        Format::Bw1 => match palette.as_slice() {
            [[0, 0, 0], [255, 255, 255]] => false,
            [[255, 255, 255], [0, 0, 0]] => true,
            _ => {
                return Err(driver_msg(
                    "the scanner's 1-bit DIB palette isn't black and white",
                ));
            }
        },
        Format::Gray8 if palette.iter().any(|&[r, g, b]| r != g || g != b) => {
            return Err(driver_msg(
                "the scanner's 8-bit DIB uses a color palette rather than grayscale",
            ));
        }
        _ => false,
    };

    let (rows_at, dib_stride) = (rows_at as usize, dib_stride as usize);
    let out_row = row_bytes(format, w) as usize;
    let mut data = Vec::new();
    data.try_reserve_exact(total).map_err(|_| {
        driver_msg(format!(
            "couldn't allocate {total} bytes for a {w} by {h} DIB; a lower resolution asks for less"
        ))
    })?;

    for y in 0..h as usize {
        let src_row = if bottom_up { h as usize - 1 - y } else { y };
        let src = &bytes[rows_at + src_row * dib_stride..][..out_row];
        match format {
            Format::Rgb8 => {
                for &[b, g, r] in src.as_chunks::<3>().0 {
                    data.extend_from_slice(&[r, g, b]);
                }
            }
            Format::Gray8 => {
                for &idx in src {
                    let Some(&[gray, _, _]) = palette.get(usize::from(idx)) else {
                        return Err(driver_msg(format!(
                            "the scanner's 8-bit DIB references palette index {idx} past {entries} entries"
                        )));
                    };
                    data.push(gray);
                }
            }
            Format::Bw1 => {
                data.extend(src.iter().map(|&b| if invert_bw { !b } else { b }));
            }
            Format::Gray16 | Format::Rgb16 => unreachable!("BI_RGB DIBs are 1, 8, or 24 bits"),
        }
    }

    clear_bw_padding(format, w, &mut data);
    RawImage::new(format, w, h, data)
        .map_err(|err| driver_msg(format!("decoded DIB failed layout check: {err}")))
}

/// Assembles `TWSX_MEMORY` strips or tiles into a [`RawImage`].
///
/// Handles both fixed-dimension transfers (`width > 0 && length > 0` in state 6) and
/// undefined-size transfers (`width == -1` and/or `length == -1` in state 6).
pub(super) struct Assembler {
    format: Format,
    placed: usize,
    mode: AssemblyMode,
}

enum AssemblyMode {
    Fixed {
        width: u32,
        height: u32,
        stride: u64,
        data: Vec<u8>,
        coverage: FixedCoverage,
    },
    Growing {
        width: Option<u32>,
        next_row: u32,
        data: Vec<u8>,
    },
}

/// Pixel coverage tracker for fixed-size images: $O(1)$ `next_row` for normal top-down full-row
/// strips, promoting to a word-masked `Vec<u64>` bitmap only if out-of-order or sub-row tiles
/// arrive.
enum FixedCoverage {
    TopDown {
        next_row: u32,
    },
    Bitmap {
        width: u32,
        words: Vec<u64>,
        covered: u64,
    },
}

impl FixedCoverage {
    fn new() -> Self {
        FixedCoverage::TopDown { next_row: 0 }
    }

    fn mark(&mut self, img_w: u32, img_h: u32, x: u32, y: u32, cols: u32, rows: u32) -> bool {
        if let FixedCoverage::TopDown { next_row } = self {
            if x == 0 && cols == img_w && y == *next_row {
                *next_row += rows;
                return true;
            }
            let already_rows = *next_row;
            let total_pixels = u64::from(img_w) * u64::from(img_h);
            let word_len = total_pixels.div_ceil(64) as usize;
            let mut words = vec![0u64; word_len];
            let pre_covered = u64::from(already_rows) * u64::from(img_w);
            fill_prefix_bits(&mut words, pre_covered);
            *self = FixedCoverage::Bitmap {
                width: img_w,
                words,
                covered: pre_covered,
            };
        }

        let FixedCoverage::Bitmap {
            width,
            words,
            covered,
            ..
        } = self
        else {
            unreachable!();
        };
        for r in 0..rows {
            let start = (u64::from(y) + u64::from(r)) * u64::from(*width) + u64::from(x);
            if !mark_bit_range(words, start, u64::from(cols)) {
                return false;
            }
            *covered += u64::from(cols);
        }
        true
    }

    fn uncovered_pixels(&self, img_w: u32, img_h: u32) -> u64 {
        let total = u64::from(img_w) * u64::from(img_h);
        match self {
            FixedCoverage::TopDown { next_row } => {
                total.saturating_sub(u64::from(*next_row) * u64::from(img_w))
            }
            FixedCoverage::Bitmap { covered, .. } => total.saturating_sub(*covered),
        }
    }
}

/// Sets the first `bit_count` bits (`0..bit_count`) in `words` to `1`.
fn fill_prefix_bits(words: &mut [u64], bit_count: u64) {
    let full_words = (bit_count / 64) as usize;
    let rem_bits = (bit_count % 64) as u32;
    words[..full_words].fill(u64::MAX);
    if rem_bits > 0 {
        words[full_words] = (1u64 << rem_bits) - 1;
    }
}

/// Sets `[start .. start + len)` in `words` to `1`, returning `false` on any overlap with an
/// already-set bit.
fn mark_bit_range(words: &mut [u64], start: u64, len: u64) -> bool {
    let mut pos = start;
    let end = start + len;
    while pos < end {
        let word_idx = (pos / 64) as usize;
        let bit_off = (pos % 64) as u32;
        let take = (end - pos).min(u64::from(64 - bit_off)) as u32;
        let mask = if take == 64 {
            u64::MAX
        } else {
            ((1u64 << take) - 1) << bit_off
        };
        if words[word_idx] & mask != 0 {
            return false;
        }
        words[word_idx] |= mask;
        pos += u64::from(take);
    }
    true
}

impl Assembler {
    /// Creates an [`Assembler`] from state 6's `DAT_IMAGEINFO`.
    pub(super) fn new(info: &ImageDesc) -> Result<Assembler, ScanError> {
        let format = format_from_info(info)?;
        let width = parse_dim(info.width, "width")?;
        let height = parse_dim(info.length, "length")?;
        let mode = match (width, height) {
            (Some(w), Some(h)) => {
                let total = checked_data_len(format, w, h)?;
                let mut data = Vec::new();
                data.try_reserve_exact(total).map_err(|_| {
                    driver_msg(format!(
                        "couldn't allocate {total} bytes for a {w} by {h} image; \
                         a lower resolution asks for less"
                    ))
                })?;
                data.resize(total, 0);
                AssemblyMode::Fixed {
                    width: w,
                    height: h,
                    stride: row_bytes(format, w),
                    data,
                    coverage: FixedCoverage::new(),
                }
            }
            (w_opt, None) => AssemblyMode::Growing {
                width: w_opt,
                next_row: 0,
                data: Vec::new(),
            },
            (None, Some(_)) => AssemblyMode::Growing {
                width: None,
                next_row: 0,
                data: Vec::new(),
            },
        };
        Ok(Assembler {
            format,
            placed: 0,
            mode,
        })
    }

    /// Places one `DAT_IMAGEMEMXFER` strip or tile into the image.
    pub(super) fn place(&mut self, strip: StripDesc<'_>) -> Result<(), ScanError> {
        let idx = self.placed;
        if strip.compression != ffi::TWCP_NONE {
            return Err(driver_msg(format!(
                "memory buffer {idx} arrived compressed (TWCP {:#06x})",
                strip.compression
            )));
        }
        if strip.columns == 0 || strip.rows == 0 {
            return Err(driver_msg(format!(
                "memory buffer {idx} has zero columns ({}) or rows ({})",
                strip.columns, strip.rows
            )));
        }
        let tile_row = row_bytes(self.format, strip.columns);
        if u64::from(strip.bytes_per_row) < tile_row {
            return Err(driver_msg(format!(
                "memory buffer {idx} reports {} bytes per row, fewer than the {tile_row} bytes its {} columns need",
                strip.bytes_per_row, strip.columns
            )));
        }
        let need = u64::from(strip.rows - 1)
            .checked_mul(u64::from(strip.bytes_per_row))
            .and_then(|prefix| prefix.checked_add(tile_row))
            .ok_or_else(|| driver_msg(format!("memory buffer {idx} row byte count overflowed")))?;
        if (strip.bytes.len() as u64) < need {
            return Err(driver_msg(format!(
                "memory buffer {idx} holds {} bytes, fewer than the {need} bytes its {} rows need",
                strip.bytes.len(),
                strip.rows
            )));
        }

        match &mut self.mode {
            AssemblyMode::Fixed {
                width,
                height,
                stride,
                data,
                coverage,
            } => {
                let (w, h) = (*width, *height);
                if u64::from(strip.x_offset) + u64::from(strip.columns) > u64::from(w)
                    || u64::from(strip.y_offset) + u64::from(strip.rows) > u64::from(h)
                {
                    return Err(driver_msg(format!(
                        "memory buffer {idx} ({} by {} at ({}, {})) runs past the {w} by {h} image",
                        strip.columns, strip.rows, strip.x_offset, strip.y_offset
                    )));
                }
                if self.format == Format::Bw1
                    && (!strip.x_offset.is_multiple_of(8)
                        || (!strip.columns.is_multiple_of(8)
                            && strip.x_offset + strip.columns != w))
                {
                    return Err(driver_msg(format!(
                        "memory buffer {idx} starts or ends mid-byte in a 1-bit image"
                    )));
                }
                if !coverage.mark(
                    w,
                    h,
                    strip.x_offset,
                    strip.y_offset,
                    strip.columns,
                    strip.rows,
                ) {
                    return Err(driver_msg(format!(
                        "memory buffer {idx} overlaps an earlier buffer"
                    )));
                }
                let x_byte = match self.format {
                    Format::Bw1 => u64::from(strip.x_offset) / 8,
                    Format::Gray8 => u64::from(strip.x_offset),
                    Format::Gray16 => u64::from(strip.x_offset) * 2,
                    Format::Rgb8 => u64::from(strip.x_offset) * 3,
                    Format::Rgb16 => u64::from(strip.x_offset) * 6,
                };
                let (tile_row, bpr) = (tile_row as usize, strip.bytes_per_row as usize);
                for r in 0..strip.rows as usize {
                    let dst = ((u64::from(strip.y_offset) + r as u64) * *stride + x_byte) as usize;
                    let src = r * bpr;
                    data[dst..dst + tile_row].copy_from_slice(&strip.bytes[src..src + tile_row]);
                }
            }
            AssemblyMode::Growing {
                width,
                next_row,
                data,
            } => {
                let w = match *width {
                    Some(w) => w,
                    None => {
                        *width = Some(strip.columns);
                        strip.columns
                    }
                };
                if strip.x_offset != 0 || strip.columns != w || strip.y_offset != *next_row {
                    return Err(driver_msg(format!(
                        "undefined-size memory buffer {idx} ({} by {} at ({}, {})) isn't contiguous with width {w} at row {}",
                        strip.columns, strip.rows, strip.x_offset, strip.y_offset, *next_row
                    )));
                }
                let new_height = next_row.checked_add(strip.rows).ok_or_else(|| {
                    driver_msg(format!(
                        "undefined-size memory buffer {idx} overflowed image row count"
                    ))
                })?;
                let new_total = checked_data_len(self.format, w, new_height)?;
                let add_bytes = new_total - data.len();
                data.try_reserve(add_bytes).map_err(|_| {
                    driver_msg(format!(
                        "couldn't allocate {new_total} bytes for a {w} by {new_height} image; \
                         a lower resolution asks for less"
                    ))
                })?;
                let (tile_row, bpr) = (tile_row as usize, strip.bytes_per_row as usize);
                for r in 0..strip.rows as usize {
                    let src = r * bpr;
                    data.extend_from_slice(&strip.bytes[src..src + tile_row]);
                }
                *next_row = new_height;
            }
        }
        self.placed += 1;
        Ok(())
    }

    /// Finalizes the assembled image, cross-checking state 7's `DAT_IMAGEINFO` `(width, length)`
    /// when provided, applying `polarity` inversion to `Bw1 | Gray8 | Gray16` only, and clearing
    /// `Bw1` padding bits.
    pub(super) fn finish(
        self,
        polarity: Polarity,
        state7_dims: Option<(i32, i32)>,
    ) -> Result<RawImage, ScanError> {
        let (width, height, mut data) = match self.mode {
            AssemblyMode::Fixed {
                width,
                height,
                data,
                coverage,
                ..
            } => {
                let missing = coverage.uncovered_pixels(width, height);
                if missing > 0 {
                    return Err(driver_msg(format!(
                        "memory transfer ended with {missing} uncovered pixels"
                    )));
                }
                (width, height, data)
            }
            AssemblyMode::Growing {
                width,
                next_row,
                data,
            } => {
                let Some(w) = width.filter(|_| next_row > 0) else {
                    return Err(driver_msg(
                        "undefined-size memory transfer ended with zero rows transferred",
                    ));
                };
                (w, next_row, data)
            }
        };

        if let Some((w7, h7)) = state7_dims {
            let w7 = parse_dim(w7, "post-transfer width")?;
            let h7 = parse_dim(h7, "post-transfer length")?;
            if let Some(w7) = w7
                && w7 != width
            {
                return Err(driver_msg(format!(
                    "post-transfer DAT_IMAGEINFO width ({w7}) disagrees with transferred width ({width})"
                )));
            }
            if let Some(h7) = h7
                && h7 != height
            {
                return Err(driver_msg(format!(
                    "post-transfer DAT_IMAGEINFO length ({h7}) disagrees with transferred rows ({height})"
                )));
            }
        }

        if polarity == Polarity::ZeroWhite
            && matches!(self.format, Format::Bw1 | Format::Gray8 | Format::Gray16)
        {
            for byte in &mut data {
                *byte = !*byte;
            }
        }
        clear_bw_padding(self.format, width, &mut data);
        RawImage::new(self.format, width, height, data)
            .map_err(|err| driver_msg(format!("assembled image failed layout check: {err}")))
    }
}

/// Parses a `DAT_IMAGEINFO` dimension (`> 0` known, `-1` undefined, anything else invalid).
fn parse_dim(raw: i32, label: &str) -> Result<Option<u32>, ScanError> {
    if raw > 0 {
        Ok(Some(raw as u32))
    } else if raw == -1 {
        Ok(None)
    } else {
        Err(driver_msg(format!(
            "the scanner reported an invalid {label} ({raw})"
        )))
    }
}

/// Clears trailing bits past `width` on every row of a [`Format::Bw1`] buffer.
fn clear_bw_padding(format: Format, width: u32, data: &mut [u8]) {
    let used = width % 8;
    if format != Format::Bw1 || used == 0 {
        return;
    }
    let keep = 0xffu8 << (8 - used);
    let stride = row_bytes(format, width) as usize;
    for row in data.chunks_exact_mut(stride) {
        if let Some(last) = row.last_mut() {
            *last &= keep;
        }
    }
}

fn u16_at(bytes: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([bytes[at], bytes[at + 1]])
}

fn u32_at(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
}

fn i32_at(bytes: &[u8], at: usize) -> i32 {
    i32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_dib(width: i32, height: i32, bits: u16, palette: &[[u8; 4]], rows: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&40u32.to_le_bytes());
        out.extend_from_slice(&width.to_le_bytes());
        out.extend_from_slice(&height.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&bits.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes()); // BI_RGB
        out.extend_from_slice(&(rows.len() as u32).to_le_bytes());
        out.extend_from_slice(&0i32.to_le_bytes());
        out.extend_from_slice(&0i32.to_le_bytes());
        out.extend_from_slice(&(palette.len() as u32).to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        for quad in palette {
            out.extend_from_slice(quad);
        }
        out.extend_from_slice(rows);
        out
    }

    fn info(format: Format, width: i32, length: i32) -> ImageDesc {
        let (pixel_type, samples_per_pixel, bits_per_sample, bits_per_pixel) = match format {
            Format::Bw1 => (ffi::TWPT_BW as i16, 1, 1, 1),
            Format::Gray8 => (ffi::TWPT_GRAY as i16, 1, 8, 8),
            Format::Gray16 => (ffi::TWPT_GRAY as i16, 1, 16, 16),
            Format::Rgb8 => (ffi::TWPT_RGB as i16, 3, 8, 24),
            Format::Rgb16 => (ffi::TWPT_RGB as i16, 3, 16, 48),
        };
        ImageDesc {
            width,
            length,
            samples_per_pixel,
            bits_per_sample,
            bits_per_pixel,
            planar: false,
            pixel_type,
            compression: ffi::TWCP_NONE,
        }
    }

    #[test]
    fn dib_24bit_flips_bottom_up_swaps_bgr_and_drops_row_padding() {
        // 2x2 bottom-up DIB: each row is 2*3 = 6 bytes padded to 8 bytes.
        // Bottom row (y = 1 in top-down): (R=10,G=20,B=30), (R=40,G=50,B=60)
        // Top row    (y = 0 in top-down): (R=1,G=2,B=3),    (R=4,G=5,B=6)
        let rows = [
            30, 20, 10, 60, 50, 40, 0xAA, 0xBB, // file row 0 -> bottom row
            3, 2, 1, 6, 5, 4, 0xCC, 0xDD, // file row 1 -> top row
        ];
        let img = decode_dib(&make_dib(2, 2, 24, &[], &rows)).expect("valid 24-bit DIB");
        assert_eq!(img.format(), Format::Rgb8);
        assert_eq!((img.width(), img.height()), (2, 2));
        assert_eq!(img.data(), &[1, 2, 3, 4, 5, 6, 10, 20, 30, 40, 50, 60]);

        // Negative height means top-down: file row 0 is already y = 0, still swapped BGR -> RGB.
        let top_down = decode_dib(&make_dib(2, -2, 24, &[], &rows)).expect("top-down DIB");
        assert_eq!(top_down.data(), &[10, 20, 30, 40, 50, 60, 1, 2, 3, 4, 5, 6]);
    }

    #[test]
    fn dib_8bit_applies_grayscale_palette_and_refuses_color_palette() {
        // Inverted grayscale ramp (index i -> 255 - i).
        let mut inv_palette = Vec::with_capacity(256);
        for i in 0..=255u8 {
            let g = 255 - i;
            inv_palette.push([g, g, g, 0]);
        }
        // 3x1 row padded to 4 bytes.
        let img = decode_dib(&make_dib(3, 1, 8, &inv_palette, &[0, 200, 255, 0xEE]))
            .expect("valid 8-bit gray DIB");
        assert_eq!(img.format(), Format::Gray8);
        assert_eq!(img.data(), &[255, 55, 0]);

        // Color entry in 8-bit palette is refused.
        inv_palette[5] = [10, 20, 30, 0];
        assert!(decode_dib(&make_dib(3, 1, 8, &inv_palette, &[0, 200, 255, 0])).is_err());
    }

    #[test]
    fn dib_1bit_handles_both_palettes_and_clears_padding_bits() {
        // 13x1 image: 2 packed bytes per row, 4 padded bytes in DIB; low 3 bits of byte 1 are padding.
        let bw = [[0, 0, 0, 0], [255, 255, 255, 0]];
        let wb = [[255, 255, 255, 0], [0, 0, 0, 0]];

        let normal = decode_dib(&make_dib(
            13,
            1,
            1,
            &bw,
            &[0b1010_0101, 0b1111_1111, 0xFF, 0xFF],
        ))
        .expect("normal 1-bit DIB");
        assert_eq!(normal.format(), Format::Bw1);
        assert_eq!(normal.data(), &[0b1010_0101, 0b1111_1000]);

        let inverted = decode_dib(&make_dib(
            13,
            1,
            1,
            &wb,
            &[0b1010_0101, 0b0000_0000, 0xFF, 0xFF],
        ))
        .expect("white-then-black 1-bit DIB");
        // Inverted `0b0000_0000` becomes `0b1111_1111`, then low 3 padding bits cleared -> `0b1111_1000`.
        assert_eq!(inverted.data(), &[0b0101_1010, 0b1111_1000]);
    }

    #[test]
    fn dib_rejects_malformed_headers_truncation_and_overflow_dimensions() {
        assert!(decode_dib(&[]).is_err());
        assert!(decode_dib(&[0; 39]).is_err());
        // Truncated pixel data.
        assert!(decode_dib(&make_dib(10, 10, 24, &[], &[0; 100])).is_err());
        // Invalid dimensions (0, negative width, i32::MIN height, > MAX_PIXELS).
        assert!(decode_dib(&make_dib(0, 10, 24, &[], &[])).is_err());
        assert!(decode_dib(&make_dib(-4, 10, 24, &[], &[])).is_err());
        assert!(decode_dib(&make_dib(4, i32::MIN, 24, &[], &[])).is_err());
        assert!(decode_dib(&make_dib(20_000, 20_000, 24, &[], &[])).is_err());
    }

    #[test]
    fn assembler_top_down_strips_stay_on_fast_path_for_all_five_formats() {
        // Test 16-bit gray & 16-bit RGB little-endian preservation alongside 1/8/24-bit.
        let desc = info(Format::Rgb16, 2, 2);
        let mut asm = Assembler::new(&desc).unwrap();
        assert!(matches!(
            asm.mode,
            AssemblyMode::Fixed {
                coverage: FixedCoverage::TopDown { next_row: 0 },
                ..
            }
        ));
        // Row 0: 2 pixels * 6 bytes = 12 bytes, padded to 16 bytes.
        let row0 = [1, 0, 2, 0, 3, 0, 4, 0, 5, 0, 6, 0, 0xFF, 0xFF, 0xFF, 0xFF];
        let row1 = [
            7, 0, 8, 0, 9, 0, 10, 0, 11, 0, 12, 0, 0xEE, 0xEE, 0xEE, 0xEE,
        ];
        asm.place(StripDesc {
            compression: ffi::TWCP_NONE,
            bytes_per_row: 16,
            columns: 2,
            rows: 1,
            x_offset: 0,
            y_offset: 0,
            bytes: &row0,
        })
        .unwrap();
        asm.place(StripDesc {
            compression: ffi::TWCP_NONE,
            bytes_per_row: 16,
            columns: 2,
            rows: 1,
            x_offset: 0,
            y_offset: 1,
            bytes: &row1,
        })
        .unwrap();
        // Still on O(1) TopDown coverage — no bitmap allocated!
        assert!(matches!(
            asm.mode,
            AssemblyMode::Fixed {
                coverage: FixedCoverage::TopDown { next_row: 2 },
                ..
            }
        ));
        // ZeroWhite must NOT invert Rgb16!
        let img = asm.finish(Polarity::ZeroWhite, Some((2, 2))).unwrap();
        assert_eq!(img.format(), Format::Rgb16);
        assert_eq!(img.data(), &[&row0[..12], &row1[..12]].concat());
    }

    #[test]
    fn assembler_promotes_to_bitmap_on_out_of_order_2d_tiles_and_catches_overlaps_and_gaps() {
        let desc = info(Format::Gray8, 4, 3);
        let mut asm = Assembler::new(&desc).unwrap();
        // Row 0 full strip (starts on TopDown fast path).
        asm.place(StripDesc {
            compression: ffi::TWCP_NONE,
            bytes_per_row: 4,
            columns: 4,
            rows: 1,
            x_offset: 0,
            y_offset: 0,
            bytes: &[1, 2, 3, 4],
        })
        .unwrap();
        // Out-of-order right-half tile for rows 1..3 -> promotes to Bitmap!
        asm.place(StripDesc {
            compression: ffi::TWCP_NONE,
            bytes_per_row: 4,
            columns: 2,
            rows: 2,
            x_offset: 2,
            y_offset: 1,
            bytes: &[7, 8, 0, 0, 11, 12, 0, 0],
        })
        .unwrap();
        assert!(matches!(
            asm.mode,
            AssemblyMode::Fixed {
                coverage: FixedCoverage::Bitmap { .. },
                ..
            }
        ));
        // Overlapping tile at (1, 1) of width 2 overlaps column 2 -> rejected!
        assert!(
            asm.place(StripDesc {
                compression: ffi::TWCP_NONE,
                bytes_per_row: 2,
                columns: 2,
                rows: 1,
                x_offset: 1,
                y_offset: 1,
                bytes: &[99, 99],
            })
            .is_err()
        );
        // Left-half tile for rows 1..3 completes coverage.
        asm.place(StripDesc {
            compression: ffi::TWCP_NONE,
            bytes_per_row: 2,
            columns: 2,
            rows: 2,
            x_offset: 0,
            y_offset: 1,
            bytes: &[5, 6, 9, 10],
        })
        .unwrap();
        let img = asm.finish(Polarity::ZeroBlack, Some((4, 3))).unwrap();
        assert_eq!(img.data(), &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12]);

        // Uncovered gap at finish is refused.
        let mut incomplete = Assembler::new(&desc).unwrap();
        incomplete
            .place(StripDesc {
                compression: ffi::TWCP_NONE,
                bytes_per_row: 4,
                columns: 4,
                rows: 2,
                x_offset: 0,
                y_offset: 0,
                bytes: &[0; 8],
            })
            .unwrap();
        assert!(incomplete.finish(Polarity::ZeroBlack, None).is_err());
    }

    #[test]
    fn assembler_supports_undefined_width_and_length_and_cross_checks_state7() {
        for (w6, l6) in [(3, -1), (-1, 2), (-1, -1)] {
            let desc = info(Format::Gray8, w6, l6);
            let mut asm = Assembler::new(&desc).unwrap();
            asm.place(StripDesc {
                compression: ffi::TWCP_NONE,
                bytes_per_row: 4,
                columns: 3,
                rows: 1,
                x_offset: 0,
                y_offset: 0,
                bytes: &[10, 20, 30, 0xFF],
            })
            .unwrap();
            asm.place(StripDesc {
                compression: ffi::TWCP_NONE,
                bytes_per_row: 4,
                columns: 3,
                rows: 1,
                x_offset: 0,
                y_offset: 1,
                bytes: &[40, 50, 60, 0xFF],
            })
            .unwrap();
            let img = asm.finish(Polarity::ZeroBlack, Some((3, 2))).unwrap();
            assert_eq!((img.width(), img.height()), (3, 2));
            assert_eq!(img.data(), &[10, 20, 30, 40, 50, 60]);
        }

        // Mismatched state 7 dimensions are refused.
        let mut mismatch = Assembler::new(&info(Format::Gray8, 3, -1)).unwrap();
        mismatch
            .place(StripDesc {
                compression: ffi::TWCP_NONE,
                bytes_per_row: 3,
                columns: 3,
                rows: 2,
                x_offset: 0,
                y_offset: 0,
                bytes: &[1, 2, 3, 4, 5, 6],
            })
            .unwrap();
        assert!(mismatch.finish(Polarity::ZeroBlack, Some((3, 5))).is_err());

        // Zero strips in undefined-size mode is refused.
        let empty = Assembler::new(&info(Format::Gray8, -1, -1)).unwrap();
        assert!(empty.finish(Polarity::ZeroBlack, None).is_err());
    }

    #[test]
    fn zero_white_polarity_inverts_bw1_gray8_and_gray16_only_and_clears_bw1_padding() {
        // Bw1 13x1 with ZeroWhite: `0x00, 0x00` -> inverted `0xFF, 0xFF` -> low 3 padding bits cleared `0xFF, 0xF8`.
        let mut bw = Assembler::new(&info(Format::Bw1, 13, 1)).unwrap();
        bw.place(StripDesc {
            compression: ffi::TWCP_NONE,
            bytes_per_row: 4,
            columns: 13,
            rows: 1,
            x_offset: 0,
            y_offset: 0,
            bytes: &[0x00, 0x00, 0xAA, 0xBB],
        })
        .unwrap();
        let bw_img = bw.finish(Polarity::ZeroWhite, None).unwrap();
        assert_eq!(bw_img.data(), &[0xFF, 0xF8]);

        // Gray16 2x1 with ZeroWhite: `0x0000, 0x1234` -> `0xFFFF, 0xEDCB` in LE.
        let mut g16 = Assembler::new(&info(Format::Gray16, 2, 1)).unwrap();
        g16.place(StripDesc {
            compression: ffi::TWCP_NONE,
            bytes_per_row: 4,
            columns: 2,
            rows: 1,
            x_offset: 0,
            y_offset: 0,
            bytes: &[0x00, 0x00, 0x34, 0x12],
        })
        .unwrap();
        let g16_img = g16.finish(Polarity::ZeroWhite, None).unwrap();
        assert_eq!(g16_img.data(), &[0xFF, 0xFF, 0xCB, 0xED]);

        // Rgb8 with ZeroWhite is NOT inverted.
        let mut rgb = Assembler::new(&info(Format::Rgb8, 1, 1)).unwrap();
        rgb.place(StripDesc {
            compression: ffi::TWCP_NONE,
            bytes_per_row: 4,
            columns: 1,
            rows: 1,
            x_offset: 0,
            y_offset: 0,
            bytes: &[10, 20, 30, 0],
        })
        .unwrap();
        let rgb_img = rgb.finish(Polarity::ZeroWhite, None).unwrap();
        assert_eq!(rgb_img.data(), &[10, 20, 30]);
    }
}
