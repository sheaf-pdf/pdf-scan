//! Safe Windows DIB (`BITMAPINFOHEADER` / `BITMAPV4HEADER` / `BITMAPV5HEADER`) and `.bmp`
//! (`BITMAPFILEHEADER`) decoding and packed-layout helpers shared by the `twain` and `wia`
//! backends.
//!
//! Pure safe Rust: every header field, bitfield mask, palette index, `bfOffBits` offset, and row
//! stride is validated in `u64` / `usize` arithmetic against the shared page caps
//! ([`MAX_PIXELS`](crate::page_caps::MAX_PIXELS), [`MAX_IMAGE_BYTES`], checked by
//! [`checked_data_len`]), [`MAX_DIB_BYTES`], and [`RawImage::data_len`] before allocating or
//! indexing.

use crate::page_caps::{MAX_IMAGE_BYTES, checked_data_len};
use crate::{Format, RawImage, ScanError};

/// Maximum raw/DIB/BMP block accepted (`512 MiB + 64 KiB` for headers, masks, and palette; a
/// 32-bit `BGRX` DIB up to `512 MiB` decodes into `384 MiB` of `Rgb8`).
pub(crate) const MAX_DIB_BYTES: u64 = (MAX_IMAGE_BYTES as u64) + (1 << 16);

const BI_RGB: u32 = 0;
const BI_BITFIELDS: u32 = 3;

/// Which DIB container framing [`decode`] accepts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct DibMode {
    /// Accept a 14-byte `BITMAPFILEHEADER` (`"BM"`, using `bfOffBits` from byte `0` with a
    /// `bfOffBits == 0` fallback).
    pub allow_file_header: bool,
    /// Accept a bare packed DIB starting directly with `BITMAPINFOHEADER` (or a larger header).
    pub allow_bare_dib: bool,
}

fn driver_msg(msg: impl Into<String>) -> ScanError {
    ScanError::Driver {
        message: msg.into(),
        shown: false,
    }
}

/// Packed unpadded byte length of one row of `width` pixels in `format`.
pub(crate) fn row_bytes(format: Format, width: u32) -> u64 {
    let bits: u64 = match format {
        Format::Bw1 => 1,
        Format::Gray8 => 8,
        Format::Gray16 => 16,
        Format::Rgb8 => 24,
        Format::Rgb16 => 48,
    };
    (u64::from(width) * bits).div_ceil(8)
}

/// Clears trailing bits past `width` on every row of a [`Format::Bw1`] buffer.
pub(crate) fn clear_bw_padding(format: Format, width: u32, data: &mut [u8]) {
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

#[derive(Clone, Copy, Debug)]
struct ChannelMask {
    mask: u32,
    shift: u32,
    width: u32,
}

impl ChannelMask {
    fn parse(mask: u32, label: &str) -> Result<Self, ScanError> {
        if mask == 0 {
            return Err(driver_msg(format!(
                "the scanner's DIB has a zero {label} channel mask"
            )));
        }
        let shift = mask.trailing_zeros();
        let shifted = mask >> shift;
        let width = shifted.trailing_ones();
        let contiguous = width == 32 || shifted == (1u32 << width) - 1;
        if !contiguous {
            return Err(driver_msg(format!(
                "the scanner's DIB has a non-contiguous {label} channel mask ({mask:#010x})"
            )));
        }
        Ok(ChannelMask { mask, shift, width })
    }

    fn extract(self, pixel: u32) -> u8 {
        let raw = (pixel & self.mask) >> self.shift;
        if self.width == 8 {
            raw as u8
        } else {
            let max = (1u64 << self.width) - 1;
            ((u64::from(raw) * 255 + max / 2) / max) as u8
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct Bitfields {
    r: ChannelMask,
    g: ChannelMask,
    b: ChannelMask,
}

impl Bitfields {
    fn from_masks(bits: u16, r_mask: u32, g_mask: u32, b_mask: u32) -> Result<Self, ScanError> {
        if bits == 16 && ((r_mask | g_mask | b_mask) & !0xFFFF) != 0 {
            return Err(driver_msg(format!(
                "the scanner's 16-bit DIB has channel masks past 16 bits \
                 ({r_mask:#010x}, {g_mask:#010x}, {b_mask:#010x})"
            )));
        }
        if (r_mask & g_mask) != 0 || (r_mask & b_mask) != 0 || (g_mask & b_mask) != 0 {
            return Err(driver_msg(format!(
                "the scanner's DIB has overlapping channel masks \
                 ({r_mask:#010x}, {g_mask:#010x}, {b_mask:#010x})"
            )));
        }
        Ok(Bitfields {
            r: ChannelMask::parse(r_mask, "red")?,
            g: ChannelMask::parse(g_mask, "green")?,
            b: ChannelMask::parse(b_mask, "blue")?,
        })
    }

    fn decode_pixel(self, pixel: u32) -> [u8; 3] {
        [
            self.r.extract(pixel),
            self.g.extract(pixel),
            self.b.extract(pixel),
        ]
    }
}

/// Decodes a Windows packed DIB or `.bmp` stream (according to `mode`) into a checked [`RawImage`].
pub(crate) fn decode(bytes: &[u8], mode: DibMode) -> Result<RawImage, ScanError> {
    if bytes.is_empty() {
        return Err(driver_msg("the scanner returned an empty DIB stream"));
    }
    if (bytes.len() as u64) > MAX_DIB_BYTES {
        return Err(driver_msg(format!(
            "the scanner's DIB is {} bytes, past the {MAX_DIB_BYTES}-byte cap; \
             a lower resolution asks for less",
            bytes.len()
        )));
    }

    let has_bm = bytes.starts_with(b"BM");
    let (dib_offset, bf_off_bits) = match (has_bm, mode.allow_file_header, mode.allow_bare_dib) {
        (true, true, _) => {
            if bytes.len() < 14 {
                return Err(driver_msg(format!(
                    "the scanner's BMP stream is {} bytes, shorter than a 14-byte BITMAPFILEHEADER",
                    bytes.len()
                )));
            }
            (14usize, Some(u32_at(bytes, 10)))
        }
        (true, false, true) | (false, _, true) => (0usize, None),
        (false, true, false) => {
            return Err(driver_msg(
                "the scanner's BMP stream lacks a 14-byte BITMAPFILEHEADER ('BM')",
            ));
        }
        (_, false, false) => return Err(driver_msg("no DIB container mode enabled")),
    };

    let dib = &bytes[dib_offset..];
    if dib.len() < 40 {
        return Err(driver_msg(format!(
            "the scanner's DIB is {} bytes, shorter than a 40-byte BITMAPINFOHEADER",
            dib.len()
        )));
    }

    let header_size = u32_at(dib, 0);
    let width = i32_at(dib, 4);
    let height = i32_at(dib, 8);
    let planes = u16_at(dib, 12);
    let bits = u16_at(dib, 14);
    let compression = u32_at(dib, 16);
    let colors_used = u32_at(dib, 32);

    if header_size < 40 || u64::from(header_size) > dib.len() as u64 {
        return Err(driver_msg(format!(
            "the scanner's DIB header size ({header_size}) is invalid"
        )));
    }
    if planes != 1 {
        return Err(driver_msg(format!(
            "the scanner's DIB has {planes} planes, expected 1"
        )));
    }

    let (initial_format, bitfields, extra_mask_bytes) = match (compression, bits) {
        (BI_RGB, 1) => (Format::Bw1, None, 0u64),
        (BI_RGB, 8) => (Format::Gray8, None, 0u64),
        (BI_RGB, 16) => (
            Format::Rgb8,
            Some(Bitfields::from_masks(16, 0x7C00, 0x03E0, 0x001F)?),
            0u64,
        ),
        (BI_RGB, 24 | 32) => (Format::Rgb8, None, 0u64),
        (BI_BITFIELDS, 16 | 32) => {
            let extra = if header_size == 40 {
                if dib.len() < 52 {
                    return Err(driver_msg(
                        "the scanner's BI_BITFIELDS DIB is truncated before its 12-byte channel masks",
                    ));
                }
                12u64
            } else if header_size >= 52 {
                0u64
            } else {
                return Err(driver_msg(format!(
                    "the scanner's BI_BITFIELDS DIB has invalid header size ({header_size})"
                )));
            };
            let bf =
                Bitfields::from_masks(bits, u32_at(dib, 40), u32_at(dib, 44), u32_at(dib, 48))?;
            (Format::Rgb8, Some(bf), extra)
        }
        (BI_RGB, other) => {
            return Err(driver_msg(format!(
                "the scanner's DIB uses {other} bits per pixel, expected 1, 8, 16, 24, or 32"
            )));
        }
        (BI_BITFIELDS, other) => {
            return Err(driver_msg(format!(
                "the scanner's DIB uses BI_BITFIELDS (3) with {other} bits per pixel, expected 16 or 32"
            )));
        }
        (other, _) => {
            return Err(driver_msg(format!(
                "the scanner's DIB uses compression {other}, expected BI_RGB (0) or BI_BITFIELDS (3)"
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
    let _ = checked_data_len(initial_format, w, h)?;

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

    let palette_at = (dib_offset as u64) + u64::from(header_size) + extra_mask_bytes;
    let default_rows_at = palette_at + u64::from(entries) * 4;
    let rows_at = match bf_off_bits {
        Some(off) if off != 0 => {
            let off = u64::from(off);
            if off < default_rows_at {
                return Err(driver_msg(format!(
                    "the scanner's BMP bfOffBits ({off}) points inside the {default_rows_at}-byte header/palette"
                )));
            }
            off
        }
        _ => default_rows_at,
    };

    let dib_stride = (u64::from(w) * u64::from(bits)).div_ceil(32) * 4;
    let need = rows_at
        .checked_add(dib_stride * u64::from(h))
        .ok_or_else(|| driver_msg("the scanner's DIB row byte offset overflowed"))?;
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

    let (rows_at, dib_stride) = (rows_at as usize, dib_stride as usize);

    let (format, invert_bw) = match bits {
        1 => {
            if palette.len() < 2 {
                return Err(driver_msg(
                    "the scanner's 1-bit DIB palette has fewer than 2 entries",
                ));
            }
            let lum =
                |c: [u8; 3]| 77 * u32::from(c[0]) + 150 * u32::from(c[1]) + 29 * u32::from(c[2]);
            let (l0, l1) = (lum(palette[0]), lum(palette[1]));
            if l0 == l1 {
                return Err(driver_msg(
                    "the scanner's 1-bit DIB palette has two identical-luminance entries",
                ));
            }
            (Format::Bw1, l0 > l1)
        }
        8 => {
            let mut has_color = false;
            for y in 0..h as usize {
                let src = &bytes[rows_at + y * dib_stride..][..w as usize];
                for &idx in src {
                    let Some(&[r, g, b]) = palette.get(usize::from(idx)) else {
                        return Err(driver_msg(format!(
                            "the scanner's 8-bit DIB references palette index {idx} past {entries} entries"
                        )));
                    };
                    if r != g || g != b {
                        has_color = true;
                    }
                }
            }
            let fmt = if has_color {
                Format::Rgb8
            } else {
                Format::Gray8
            };
            (fmt, false)
        }
        16 | 24 | 32 => (Format::Rgb8, false),
        _ => unreachable!(),
    };

    let total = checked_data_len(format, w, h)?;
    let src_row_bytes = ((u64::from(w) * u64::from(bits)).div_ceil(8)) as usize;
    let mut data = Vec::new();
    data.try_reserve_exact(total).map_err(|_| {
        driver_msg(format!(
            "couldn't allocate {total} bytes for a {w} by {h} DIB; a lower resolution asks for less"
        ))
    })?;

    for y in 0..h as usize {
        let src_row = if bottom_up { h as usize - 1 - y } else { y };
        let src = &bytes[rows_at + src_row * dib_stride..][..src_row_bytes];
        match (bits, format, bitfields) {
            (24, Format::Rgb8, None) => {
                for &[b, g, r] in src.as_chunks::<3>().0 {
                    data.extend_from_slice(&[r, g, b]);
                }
            }
            (32, Format::Rgb8, None) => {
                for &[b, g, r, _] in src.as_chunks::<4>().0 {
                    data.extend_from_slice(&[r, g, b]);
                }
            }
            (32, Format::Rgb8, Some(bf)) => {
                for &chunk in src.as_chunks::<4>().0 {
                    data.extend_from_slice(&bf.decode_pixel(u32::from_le_bytes(chunk)));
                }
            }
            (16, Format::Rgb8, Some(bf)) => {
                for &chunk in src.as_chunks::<2>().0 {
                    data.extend_from_slice(&bf.decode_pixel(u32::from(u16::from_le_bytes(chunk))));
                }
            }
            (8, Format::Gray8, None) => {
                for &idx in src {
                    data.push(palette[usize::from(idx)][0]);
                }
            }
            (8, Format::Rgb8, None) => {
                for &idx in src {
                    data.extend_from_slice(&palette[usize::from(idx)]);
                }
            }
            (1, Format::Bw1, None) => {
                data.extend(src.iter().map(|&b| if invert_bw { !b } else { b }));
            }
            _ => unreachable!(),
        }
    }

    clear_bw_padding(format, w, &mut data);
    RawImage::new(format, w, h, data)
        .map_err(|err| driver_msg(format!("decoded DIB failed layout check: {err}")))
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

    const BARE_ONLY: DibMode = DibMode {
        allow_file_header: false,
        allow_bare_dib: true,
    };
    const BMP_ONLY: DibMode = DibMode {
        allow_file_header: true,
        allow_bare_dib: false,
    };
    const AUTO_MODE: DibMode = DibMode {
        allow_file_header: true,
        allow_bare_dib: true,
    };

    fn make_dib(width: i32, height: i32, bits: u16, palette: &[[u8; 4]], rows: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&40u32.to_le_bytes());
        out.extend_from_slice(&width.to_le_bytes());
        out.extend_from_slice(&height.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&bits.to_le_bytes());
        out.extend_from_slice(&BI_RGB.to_le_bytes());
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

    fn wrap_bmp(
        dib: &[u8],
        bf_size: u32,
        bf_off_bits: u32,
        gap_before_rows: usize,
        rows_len: usize,
    ) -> Vec<u8> {
        let header_and_pal = &dib[..dib.len() - rows_len];
        let rows = &dib[dib.len() - rows_len..];
        let mut out = Vec::new();
        out.extend_from_slice(b"BM");
        out.extend_from_slice(&bf_size.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&bf_off_bits.to_le_bytes());
        out.extend_from_slice(header_and_pal);
        out.extend(std::iter::repeat_n(0xCCu8, gap_before_rows));
        out.extend_from_slice(rows);
        out
    }

    #[test]
    fn bmp_file_header_honors_bf_off_bits_zero_fallback_and_container_modes() {
        let rows = [30, 20, 10, 0, 3, 2, 1, 0]; // 1x2 24-bit bottom-up
        let dib = make_dib(1, 2, 24, &[], &rows);

        // Bare DIB rejected when BMP file header is required, accepted in AUTO_MODE and BARE_ONLY.
        assert!(decode(&dib, BMP_ONLY).is_err());
        assert_eq!(
            decode(&dib, BARE_ONLY).unwrap().data(),
            &[1, 2, 3, 10, 20, 30]
        );
        assert_eq!(
            decode(&dib, AUTO_MODE).unwrap().data(),
            &[1, 2, 3, 10, 20, 30]
        );

        // Standard .bmp with exact bfOffBits (14 + 40 = 54) and unpatched bfSize = 0.
        let bmp = wrap_bmp(&dib, 0, 54, 0, rows.len());
        assert!(decode(&bmp, BARE_ONLY).is_err());
        let decoded = decode(&bmp, BMP_ONLY).expect("valid .bmp");
        assert_eq!(decoded.data(), &[1, 2, 3, 10, 20, 30]);
        assert_eq!(
            decode(&bmp, AUTO_MODE).unwrap().data(),
            &[1, 2, 3, 10, 20, 30]
        );

        // bfOffBits == 0 falls back to default header + palette offset (54).
        let zero_off = wrap_bmp(&dib, 0, 0, 0, rows.len());
        assert_eq!(
            decode(&zero_off, BMP_ONLY).unwrap().data(),
            &[1, 2, 3, 10, 20, 30]
        );

        // bfOffBits > 54 skips an embedded profile/alignment gap measured from byte 0 of .bmp.
        let padded = wrap_bmp(&dib, 0, 54 + 16, 16, rows.len());
        assert_eq!(
            decode(&padded, BMP_ONLY).unwrap().data(),
            &[1, 2, 3, 10, 20, 30]
        );

        // bfOffBits != 0 pointing inside the header/palette is refused.
        let bad_off = wrap_bmp(&dib, 0, 40, 0, rows.len());
        assert!(decode(&bad_off, BMP_ONLY).is_err());
        assert!(decode(b"BMshort", BMP_ONLY).is_err());
    }

    #[test]
    fn bi_bitfields_supports_16_and_32_bit_with_v1_trailing_and_v4_embedded_masks() {
        // 32-bit BI_BITFIELDS with biSize == 40 (12-byte masks trailing header at dib[40..52]):
        // Masks put R in byte 0, G in byte 1, B in byte 2 (RGBA little-endian: R=0xFF, G=0xFF00, B=0xFF_0000).
        let mut dib_v1 = Vec::new();
        dib_v1.extend_from_slice(&40u32.to_le_bytes());
        dib_v1.extend_from_slice(&2i32.to_le_bytes());
        dib_v1.extend_from_slice(&(-1i32).to_le_bytes()); // top-down 2x1
        dib_v1.extend_from_slice(&1u16.to_le_bytes());
        dib_v1.extend_from_slice(&32u16.to_le_bytes());
        dib_v1.extend_from_slice(&BI_BITFIELDS.to_le_bytes());
        dib_v1.extend_from_slice(&8u32.to_le_bytes());
        dib_v1.extend_from_slice(&[0u8; 16]);
        dib_v1.extend_from_slice(&0x0000_00FFu32.to_le_bytes()); // R
        dib_v1.extend_from_slice(&0x0000_FF00u32.to_le_bytes()); // G
        dib_v1.extend_from_slice(&0x00FF_0000u32.to_le_bytes()); // B
        dib_v1.extend_from_slice(&[11, 22, 33, 0xFF, 44, 55, 66, 0x00]);
        let img_v1 = decode(&dib_v1, BARE_ONLY).expect("32-bit BI_BITFIELDS v1");
        assert_eq!(img_v1.data(), &[11, 22, 33, 44, 55, 66]);

        // 32-bit BI_BITFIELDS with BITMAPV4HEADER (biSize == 108): masks are embedded at dib[40..52],
        // NOT followed by an extra 12 bytes after byte 108!
        let mut dib_v4 = vec![0u8; 108];
        dib_v4[0..4].copy_from_slice(&108u32.to_le_bytes());
        dib_v4[4..8].copy_from_slice(&1i32.to_le_bytes());
        dib_v4[8..12].copy_from_slice(&(-1i32).to_le_bytes());
        dib_v4[12..14].copy_from_slice(&1u16.to_le_bytes());
        dib_v4[14..16].copy_from_slice(&32u16.to_le_bytes());
        dib_v4[16..20].copy_from_slice(&BI_BITFIELDS.to_le_bytes());
        dib_v4[40..44].copy_from_slice(&0xFF00_0000u32.to_le_bytes()); // R in MSB
        dib_v4[44..48].copy_from_slice(&0x00FF_0000u32.to_le_bytes()); // G
        dib_v4[48..52].copy_from_slice(&0x0000_FF00u32.to_le_bytes()); // B
        dib_v4.extend_from_slice(&0x7788_9900u32.to_le_bytes());
        let img_v4 = decode(&dib_v4, BARE_ONLY).expect("32-bit BI_BITFIELDS v4");
        assert_eq!(img_v4.data(), &[0x77, 0x88, 0x99]);

        // 16-bit RGB565 BI_BITFIELDS (0xF800, 0x07E0, 0x001F): pure white 0xFFFF -> [255, 255, 255],
        // pure red 0xF800 -> [255, 0, 0] (2 pixels = 4 bytes, 4-byte aligned).
        let mut dib_565 = Vec::new();
        dib_565.extend_from_slice(&40u32.to_le_bytes());
        dib_565.extend_from_slice(&2i32.to_le_bytes());
        dib_565.extend_from_slice(&(-1i32).to_le_bytes());
        dib_565.extend_from_slice(&1u16.to_le_bytes());
        dib_565.extend_from_slice(&16u16.to_le_bytes());
        dib_565.extend_from_slice(&BI_BITFIELDS.to_le_bytes());
        dib_565.extend_from_slice(&[0u8; 20]);
        dib_565.extend_from_slice(&0xF800u32.to_le_bytes());
        dib_565.extend_from_slice(&0x07E0u32.to_le_bytes());
        dib_565.extend_from_slice(&0x001Fu32.to_le_bytes());
        dib_565.extend_from_slice(&0xFFFFu16.to_le_bytes());
        dib_565.extend_from_slice(&0xF800u16.to_le_bytes());
        let img_565 = decode(&dib_565, BARE_ONLY).expect("16-bit RGB565 BI_BITFIELDS");
        assert_eq!(img_565.data(), &[255, 255, 255, 255, 0, 0]);

        // 16-bit BI_RGB defaults to RGB555 (0x7C00, 0x03E0, 0x001F).
        let mut rows_555 = Vec::new();
        rows_555.extend_from_slice(&0x03E0u16.to_le_bytes()); // pure green
        rows_555.extend_from_slice(&0x001Fu16.to_le_bytes()); // pure blue
        let img_555 =
            decode(&make_dib(2, -1, 16, &[], &rows_555), BARE_ONLY).expect("16-bit BI_RGB");
        assert_eq!(img_555.data(), &[0, 255, 0, 0, 0, 255]);

        // 32-bit 10-bit-per-channel BI_BITFIELDS (R=0x3FF0_0000, G=0x000F_FC00, B=0x0000_03FF):
        // verifies rounded scaling for >8-bit channels (1023 -> 255, 512 -> 128, 511 -> 127).
        let mut dib_10 = Vec::new();
        dib_10.extend_from_slice(&40u32.to_le_bytes());
        dib_10.extend_from_slice(&1i32.to_le_bytes());
        dib_10.extend_from_slice(&(-1i32).to_le_bytes());
        dib_10.extend_from_slice(&1u16.to_le_bytes());
        dib_10.extend_from_slice(&32u16.to_le_bytes());
        dib_10.extend_from_slice(&BI_BITFIELDS.to_le_bytes());
        dib_10.extend_from_slice(&[0u8; 20]);
        dib_10.extend_from_slice(&0x3FF0_0000u32.to_le_bytes());
        dib_10.extend_from_slice(&0x000F_FC00u32.to_le_bytes());
        dib_10.extend_from_slice(&0x0000_03FFu32.to_le_bytes());
        let px_10: u32 = (1023 << 20) | (512 << 10) | 511;
        dib_10.extend_from_slice(&px_10.to_le_bytes());
        let img_10 = decode(&dib_10, BARE_ONLY).expect("32-bit 10bpc BI_BITFIELDS");
        assert_eq!(img_10.data(), &[255, 128, 127]);
    }

    #[test]
    fn bi_bitfields_rejects_24bit_overlapping_noncontiguous_zero_or_out_of_width_masks() {
        // 24-bit BI_BITFIELDS is invalid per Windows DIB spec.
        let mut bad_24 = make_dib(1, 1, 24, &[], &[0; 16]);
        bad_24[16..20].copy_from_slice(&BI_BITFIELDS.to_le_bytes());
        assert!(decode(&bad_24, BARE_ONLY).is_err());

        let make_bf32 = |r: u32, g: u32, b: u32, bits: u16| {
            let mut v = Vec::new();
            v.extend_from_slice(&40u32.to_le_bytes());
            v.extend_from_slice(&1i32.to_le_bytes());
            v.extend_from_slice(&(-1i32).to_le_bytes());
            v.extend_from_slice(&1u16.to_le_bytes());
            v.extend_from_slice(&bits.to_le_bytes());
            v.extend_from_slice(&BI_BITFIELDS.to_le_bytes());
            v.extend_from_slice(&[0u8; 20]);
            v.extend_from_slice(&r.to_le_bytes());
            v.extend_from_slice(&g.to_le_bytes());
            v.extend_from_slice(&b.to_le_bytes());
            v.extend_from_slice(&[0u8; 4]);
            v
        };
        // Zero mask, non-contiguous mask, overlapping masks, and 16-bit mask > 0xFFFF.
        assert!(decode(&make_bf32(0, 0xFF00, 0xFF, 32), BARE_ONLY).is_err());
        assert!(decode(&make_bf32(0x0101_0000, 0xFF00, 0xFF, 32), BARE_ONLY).is_err());
        assert!(decode(&make_bf32(0xFFFF_0000, 0x00FF_0000, 0xFF, 32), BARE_ONLY).is_err());
        assert!(decode(&make_bf32(0x1_0000, 0x07E0, 0x001F, 16), BARE_ONLY).is_err());
    }
}
