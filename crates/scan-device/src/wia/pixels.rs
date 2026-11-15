//! Decoding WIA 2.0 transferred streams (`WiaImgFmt_BMP`, `WiaImgFmt_MEMORYBMP`, `WiaImgFmt_RAW`,
//! and magic-checked `Pixels::Encoded` pass-through) into [`Pixels`] and `(across, down)` DPI.
//!
//! Pure safe Rust: every untrusted stream header field (`BITMAPFILEHEADER`, `BITMAPINFOHEADER`,
//! `WIA_RAW_HEADER`), offset (`HeaderSize + RawDataOffset`), row stride, bit depth, and buffer
//! length is validated in checked `u64` / `usize` arithmetic against `MAX_DIB_BYTES`,
//! `MAX_PIXELS`, and `MAX_IMAGE_BYTES` (`512 MiB`) before allocating or indexing.

use super::driver::{
    WIA_COMPRESSION_NONE, WIA_DATA_COLOR, WIA_DATA_DITHER, WIA_DATA_GRAYSCALE, WIA_DATA_RAW_BGR,
    WIA_DATA_RAW_RGB, WIA_DATA_THRESHOLD, WIA_LINE_ORDER_BOTTOM_TO_TOP,
    WIA_LINE_ORDER_TOP_TO_BOTTOM, WIA_PHOTO_WHITE_0, WIA_PHOTO_WHITE_1,
};
use super::error::driver_msg;
use super::settings::TransferMode;
use crate::dib::{
    DibMode, MAX_DIB_BYTES, checked_data_len, clear_bw_padding, decode as decode_dib, row_bytes,
};
use crate::{Encoded, EncodedKind, Encoding, Format, Pixels, RawImage, ScanError};

/// Minimum valid `WIA_RAW_HEADER::HeaderSize` (`sizeof(WIA_RAW_HEADER) == 80` bytes in `WiaDef.h`).
const WIA_RAW_HEADER_MIN_SIZE: u32 = 80;

/// Four-byte signature `"WRAW"` (`0x5741_5257` in little-endian) at byte `0` of `WIA_RAW_HEADER`.
const WIA_RAW_TAG: &[u8; 4] = b"WRAW";

const BMP_FILE_ONLY: DibMode = DibMode {
    allow_file_header: true,
    allow_bare_dib: false,
};

const MEMORY_BMP_AUTO: DibMode = DibMode {
    allow_file_header: true,
    allow_bare_dib: true,
};

/// Decodes or validates a completed WIA page stream according to `mode`, returning `(Pixels, dpi)`
/// where `dpi` is guaranteed finite and `> 0.0` on both axes (falling back to `fallback_dpi` per
/// axis whenever a stream header omits DPI or reports `0`).
pub(super) fn decode_stream(
    bytes: Vec<u8>,
    mode: TransferMode,
    fallback_dpi: (f32, f32),
) -> Result<(Pixels, (f32, f32)), ScanError> {
    match mode {
        TransferMode::RawBmp => {
            let raw = decode_dib(&bytes, BMP_FILE_ONLY)?;
            Ok((Pixels::Raw(raw), sanitize_fallback_dpi(fallback_dpi)))
        }
        TransferMode::RawMemoryBmp => {
            let raw = decode_dib(&bytes, MEMORY_BMP_AUTO)?;
            Ok((Pixels::Raw(raw), sanitize_fallback_dpi(fallback_dpi)))
        }
        TransferMode::RawWia => {
            let (raw, dpi) = decode_wia_raw(&bytes, sanitize_fallback_dpi(fallback_dpi))?;
            Ok((Pixels::Raw(raw), dpi))
        }
        TransferMode::Encoded(encoding) => {
            let encoded = validate_encoded(bytes, encoding)?;
            Ok((
                Pixels::Encoded(encoded),
                sanitize_fallback_dpi(fallback_dpi),
            ))
        }
    }
}

fn sanitize_fallback_dpi(dpi: (f32, f32)) -> (f32, f32) {
    let x = if dpi.0.is_finite() && dpi.0 > 0.0 {
        dpi.0
    } else {
        300.0
    };
    let y = if dpi.1.is_finite() && dpi.1 > 0.0 {
        dpi.1
    } else {
        x
    };
    (x, y)
}

#[derive(Clone, Copy, Debug)]
enum RawSampleLayout {
    Bw1,
    Gray8,
    Gray16,
    Rgb24 { bgr: bool },
    Rgbx32 { bgr: bool },
    Rgb48 { bgr: bool },
    Rgbx64 { bgr: bool },
}

impl RawSampleLayout {
    fn target_format(self) -> Format {
        match self {
            RawSampleLayout::Bw1 => Format::Bw1,
            RawSampleLayout::Gray8 => Format::Gray8,
            RawSampleLayout::Gray16 => Format::Gray16,
            RawSampleLayout::Rgb24 { .. } | RawSampleLayout::Rgbx32 { .. } => Format::Rgb8,
            RawSampleLayout::Rgb48 { .. } | RawSampleLayout::Rgbx64 { .. } => Format::Rgb16,
        }
    }

    fn source_row_bytes(self, width: u32) -> u64 {
        let w = u64::from(width);
        match self {
            RawSampleLayout::Bw1 => w.div_ceil(8),
            RawSampleLayout::Gray8 => w,
            RawSampleLayout::Gray16 => w * 2,
            RawSampleLayout::Rgb24 { .. } => w * 3,
            RawSampleLayout::Rgbx32 { .. } => w * 4,
            RawSampleLayout::Rgb48 { .. } => w * 6,
            RawSampleLayout::Rgbx64 { .. } => w * 8,
        }
    }
}

/// Decodes a `WiaImgFmt_RAW` stream (`WIA_RAW_HEADER` + uncompressed pixel rows) into a checked
/// [`RawImage`] and `(across, down)` DPI.
pub(super) fn decode_wia_raw(
    bytes: &[u8],
    fallback_dpi: (f32, f32),
) -> Result<(RawImage, (f32, f32)), ScanError> {
    let fallback_dpi = sanitize_fallback_dpi(fallback_dpi);
    if bytes.is_empty() {
        return Err(driver_msg("the scanner returned an empty WIA raw stream"));
    }
    if (bytes.len() as u64) > MAX_DIB_BYTES {
        return Err(driver_msg(format!(
            "the scanner's WIA raw stream is {} bytes, past the {MAX_DIB_BYTES}-byte cap; \
             a lower resolution asks for less",
            bytes.len()
        )));
    }
    if bytes.len() < WIA_RAW_HEADER_MIN_SIZE as usize {
        return Err(driver_msg(format!(
            "the scanner's WIA raw stream is {} bytes, shorter than an 80-byte WIA_RAW_HEADER",
            bytes.len()
        )));
    }
    if &bytes[0..4] != WIA_RAW_TAG {
        return Err(driver_msg(
            "the scanner's WIA raw stream lacks the 'WRAW' signature",
        ));
    }

    let version = u32_at(bytes, 4);
    if version != 0x0001_0000 && version != 0 {
        return Err(driver_msg(format!(
            "the scanner's WIA_RAW_HEADER Version (0x{version:08x}) is unsupported"
        )));
    }

    let header_size = u32_at(bytes, 8);
    if header_size < WIA_RAW_HEADER_MIN_SIZE || u64::from(header_size) > bytes.len() as u64 {
        return Err(driver_msg(format!(
            "the scanner's WIA_RAW_HEADER HeaderSize ({header_size}) is invalid"
        )));
    }

    let x_res = u32_at(bytes, 12);
    let y_res = u32_at(bytes, 16);
    let width = u32_at(bytes, 20);
    let height = u32_at(bytes, 24);
    let bytes_per_line = u32_at(bytes, 28);
    let bits_per_pixel = u32_at(bytes, 32);
    let channels_per_pixel = u32_at(bytes, 36);
    let data_type = u32_at(bytes, 40);
    let bits_per_channel: [u8; 8] = [
        bytes[44], bytes[45], bytes[46], bytes[47], bytes[48], bytes[49], bytes[50], bytes[51],
    ];
    let compression = u32_at(bytes, 52);
    let photometric = u32_at(bytes, 56);
    let line_order = u32_at(bytes, 60);
    let raw_data_offset = u32_at(bytes, 64);
    let raw_data_size = u32_at(bytes, 68);
    let palette_size = u32_at(bytes, 76);

    if compression != WIA_COMPRESSION_NONE as u32 {
        return Err(driver_msg(format!(
            "the scanner's WIA raw stream uses unsupported compression ({compression})"
        )));
    }
    if palette_size != 0 {
        return Err(driver_msg(format!(
            "the scanner's WIA raw stream uses an unsupported {palette_size}-byte color palette"
        )));
    }

    let invert_mono = match photometric {
        x if x == WIA_PHOTO_WHITE_1 as u32 => false,
        x if x == WIA_PHOTO_WHITE_0 as u32 => true,
        other => {
            return Err(driver_msg(format!(
                "the scanner's WIA raw stream uses unknown PhotometricInterp ({other})"
            )));
        }
    };

    let bottom_up = match line_order {
        WIA_LINE_ORDER_TOP_TO_BOTTOM => false,
        WIA_LINE_ORDER_BOTTOM_TO_TOP => true,
        other => {
            return Err(driver_msg(format!(
                "the scanner's WIA raw stream uses unknown LineOrder ({other})"
            )));
        }
    };

    let layout = classify_raw_layout(
        bits_per_pixel,
        channels_per_pixel,
        data_type,
        &bits_per_channel,
    )?;
    let format = layout.target_format();
    let dst_len = checked_data_len(format, width, height)?;
    let dst_row = row_bytes(format, width) as usize;
    let src_row_u64 = layout.source_row_bytes(width);
    let src_row = usize::try_from(src_row_u64)
        .map_err(|_| driver_msg("the scanner's WIA raw row length overflows usize"))?;

    let stride_u64 = if bytes_per_line == 0 {
        src_row_u64
    } else {
        let bpl = u64::from(bytes_per_line);
        if bpl < src_row_u64 {
            return Err(driver_msg(format!(
                "the scanner's WIA raw BytesPerLine ({bytes_per_line}) is smaller than a \
                 {src_row_u64}-byte row"
            )));
        }
        bpl
    };
    let stride = usize::try_from(stride_u64)
        .map_err(|_| driver_msg("the scanner's WIA raw stride overflows usize"))?;

    let data_start_u64 = u64::from(header_size)
        .checked_add(u64::from(raw_data_offset))
        .ok_or_else(|| driver_msg("the scanner's WIA raw data offset overflows"))?;
    if data_start_u64 > bytes.len() as u64 {
        return Err(driver_msg(format!(
            "the scanner's WIA raw data offset ({data_start_u64}) is past the {}-byte stream",
            bytes.len()
        )));
    }
    let data_start = data_start_u64 as usize;
    let pixel_slice = &bytes[data_start..];

    let needed_u64 = (u64::from(height) - 1)
        .checked_mul(stride_u64)
        .and_then(|v| v.checked_add(src_row_u64))
        .ok_or_else(|| driver_msg("the scanner's WIA raw payload size overflows"))?;
    if (pixel_slice.len() as u64) < needed_u64 {
        return Err(driver_msg(format!(
            "the scanner's WIA raw pixel payload is {} bytes, shorter than the required {needed_u64} bytes",
            pixel_slice.len()
        )));
    }
    if raw_data_size != 0 {
        let declared = u64::from(raw_data_size);
        if declared < needed_u64 || declared > pixel_slice.len() as u64 {
            return Err(driver_msg(format!(
                "the scanner's WIA raw RawDataSize ({raw_data_size}) disagrees with the \
                 {needed_u64}-byte image and {}-byte payload",
                pixel_slice.len()
            )));
        }
    }

    let mut out = Vec::new();
    out.try_reserve_exact(dst_len).map_err(|_| {
        driver_msg(format!(
            "couldn't allocate {dst_len} bytes for decoded WIA raw image"
        ))
    })?;
    for y in 0..height as usize {
        let src_y = if bottom_up {
            (height as usize) - 1 - y
        } else {
            y
        };
        let row_start = src_y * stride;
        let src = &pixel_slice[row_start..row_start + src_row];
        match layout {
            RawSampleLayout::Bw1 | RawSampleLayout::Gray8 | RawSampleLayout::Gray16 => {
                debug_assert_eq!(src.len(), dst_row);
                if invert_mono {
                    // For `Bw1`, `Gray8`, and little-endian `Gray16` (`0xffff - u16 == !u16`),
                    // bitwise NOT of each byte inverts intensity; `Bw1` padding bits are masked
                    // below via `clear_bw_padding`.
                    out.extend(src.iter().map(|&b| !b));
                } else {
                    out.extend_from_slice(src);
                }
            }
            RawSampleLayout::Rgb24 { bgr: false } => {
                debug_assert_eq!(src.len(), dst_row);
                out.extend_from_slice(src);
            }
            RawSampleLayout::Rgb24 { bgr: true } => {
                for &[b, g, r] in src.as_chunks::<3>().0 {
                    out.extend_from_slice(&[r, g, b]);
                }
            }
            RawSampleLayout::Rgbx32 { bgr: false } => {
                for &[r, g, b, _] in src.as_chunks::<4>().0 {
                    out.extend_from_slice(&[r, g, b]);
                }
            }
            RawSampleLayout::Rgbx32 { bgr: true } => {
                for &[b, g, r, _] in src.as_chunks::<4>().0 {
                    out.extend_from_slice(&[r, g, b]);
                }
            }
            RawSampleLayout::Rgb48 { bgr: false } => {
                debug_assert_eq!(src.len(), dst_row);
                out.extend_from_slice(src);
            }
            RawSampleLayout::Rgb48 { bgr: true } => {
                for &[b0, b1, g0, g1, r0, r1] in src.as_chunks::<6>().0 {
                    out.extend_from_slice(&[r0, r1, g0, g1, b0, b1]);
                }
            }
            RawSampleLayout::Rgbx64 { bgr: false } => {
                for &[r0, r1, g0, g1, b0, b1, _, _] in src.as_chunks::<8>().0 {
                    out.extend_from_slice(&[r0, r1, g0, g1, b0, b1]);
                }
            }
            RawSampleLayout::Rgbx64 { bgr: true } => {
                for &[b0, b1, g0, g1, r0, r1, _, _] in src.as_chunks::<8>().0 {
                    out.extend_from_slice(&[r0, r1, g0, g1, b0, b1]);
                }
            }
        }
    }

    clear_bw_padding(format, width, &mut out);
    let raw = RawImage::new(format, width, height, out)
        .map_err(|err| driver_msg(format!("decoded WIA raw stream failed layout check: {err}")))?;

    let dpi_x = if x_res > 0 {
        x_res as f32
    } else {
        fallback_dpi.0
    };
    let dpi_y = if y_res > 0 {
        y_res as f32
    } else if x_res > 0 && fallback_dpi.1 <= 0.0 {
        dpi_x
    } else {
        fallback_dpi.1
    };

    Ok((raw, (dpi_x, dpi_y)))
}

fn bpc_channels_match(bits_per_channel: &[u8; 8], channels: usize, expected: u8) -> bool {
    let active = &bits_per_channel[..channels];
    active.iter().all(|&b| b == 0) || active.iter().all(|&b| b == expected)
}

fn classify_raw_layout(
    bits_per_pixel: u32,
    channels_per_pixel: u32,
    data_type: u32,
    bits_per_channel: &[u8; 8],
) -> Result<RawSampleLayout, ScanError> {
    let dt = i32::try_from(data_type).ok();
    let bgr = dt == Some(WIA_DATA_RAW_BGR);
    let is_color_dt = matches!(
        dt,
        Some(WIA_DATA_COLOR | WIA_DATA_RAW_RGB | WIA_DATA_RAW_BGR)
    );
    let is_bw_dt = matches!(
        dt,
        Some(WIA_DATA_THRESHOLD | WIA_DATA_DITHER | WIA_DATA_GRAYSCALE)
    );
    let is_gray_dt = matches!(dt, Some(WIA_DATA_GRAYSCALE));

    match (bits_per_pixel, channels_per_pixel) {
        (1, 0 | 1) if is_bw_dt && bpc_channels_match(bits_per_channel, 1, 1) => {
            Ok(RawSampleLayout::Bw1)
        }
        (8, 0 | 1) if is_gray_dt && bpc_channels_match(bits_per_channel, 1, 8) => {
            Ok(RawSampleLayout::Gray8)
        }
        (16, 0 | 1) if is_gray_dt && bpc_channels_match(bits_per_channel, 1, 16) => {
            Ok(RawSampleLayout::Gray16)
        }
        (24, 0 | 3) if is_color_dt && bpc_channels_match(bits_per_channel, 3, 8) => {
            Ok(RawSampleLayout::Rgb24 { bgr })
        }
        (32, 3 | 4)
            if is_color_dt
                && bpc_channels_match(bits_per_channel, 3, 8)
                && matches!(bits_per_channel[3], 0 | 8) =>
        {
            Ok(RawSampleLayout::Rgbx32 { bgr })
        }
        (48, 0 | 3) if is_color_dt && bpc_channels_match(bits_per_channel, 3, 16) => {
            Ok(RawSampleLayout::Rgb48 { bgr })
        }
        (64, 3 | 4)
            if is_color_dt
                && bpc_channels_match(bits_per_channel, 3, 16)
                && matches!(bits_per_channel[3], 0 | 16) =>
        {
            Ok(RawSampleLayout::Rgbx64 { bgr })
        }
        _ => Err(driver_msg(format!(
            "unsupported WIA raw pixel layout (BitsPerPixel={bits_per_pixel}, \
             ChannelsPerPixel={channels_per_pixel}, DataType={data_type})"
        ))),
    }
}

fn validate_encoded(bytes: Vec<u8>, encoding: Encoding) -> Result<Encoded, ScanError> {
    if bytes.is_empty() {
        return Err(driver_msg(format!(
            "the scanner returned an empty {encoding:?} stream"
        )));
    }
    if (bytes.len() as u64) > MAX_DIB_BYTES {
        return Err(driver_msg(format!(
            "the scanner's {encoding:?} stream is {} bytes, past the {MAX_DIB_BYTES}-byte cap; \
             a lower resolution asks for less",
            bytes.len()
        )));
    }

    let kind = match encoding {
        Encoding::Png => {
            if !bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
                return Err(driver_msg(
                    "the scanner's PNG stream lacks a valid 8-byte PNG signature",
                ));
            }
            EncodedKind::Png
        }
        Encoding::Jpeg => {
            if !bytes.starts_with(&[0xff, 0xd8, 0xff]) {
                return Err(driver_msg(
                    "the scanner's JPEG stream lacks a valid SOI marker (FF D8 FF)",
                ));
            }
            EncodedKind::Jpeg
        }
        Encoding::Tiff => {
            if !bytes.starts_with(b"II*\0") && !bytes.starts_with(b"MM\0*") {
                return Err(driver_msg(
                    "the scanner's TIFF stream lacks a valid II*\\0 or MM\\0* header",
                ));
            }
            EncodedKind::Tiff
        }
        Encoding::Bmp => {
            if bytes.len() < 26 || !bytes.starts_with(b"BM") {
                return Err(driver_msg(
                    "the scanner's BMP stream lacks a valid 'BM' header",
                ));
            }
            EncodedKind::Bmp
        }
        Encoding::CcittG4 => return Err(ScanError::Unsupported),
    };

    Ok(Encoded { kind, bytes })
}

fn u32_at(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[allow(clippy::too_many_arguments)]
    fn build_wraw(
        header_size: u32,
        x_res: u32,
        y_res: u32,
        width: u32,
        height: u32,
        bytes_per_line: u32,
        bpp: u32,
        channels: u32,
        data_type: u32,
        bpc: [u8; 8],
        compression: u32,
        photometric: u32,
        line_order: u32,
        raw_data_offset: u32,
        payload: &[u8],
    ) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(b"WRAW");
        out.extend_from_slice(&0x0001_0000u32.to_le_bytes());
        out.extend_from_slice(&header_size.to_le_bytes());
        out.extend_from_slice(&x_res.to_le_bytes());
        out.extend_from_slice(&y_res.to_le_bytes());
        out.extend_from_slice(&width.to_le_bytes());
        out.extend_from_slice(&height.to_le_bytes());
        out.extend_from_slice(&bytes_per_line.to_le_bytes());
        out.extend_from_slice(&bpp.to_le_bytes());
        out.extend_from_slice(&channels.to_le_bytes());
        out.extend_from_slice(&data_type.to_le_bytes());
        out.extend_from_slice(&bpc);
        out.extend_from_slice(&compression.to_le_bytes());
        out.extend_from_slice(&photometric.to_le_bytes());
        out.extend_from_slice(&line_order.to_le_bytes());
        out.extend_from_slice(&raw_data_offset.to_le_bytes());
        out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        if header_size > 80 {
            out.resize(header_size as usize, 0);
        }
        if raw_data_offset > 0 {
            out.resize((header_size + raw_data_offset) as usize, 0xAA);
        }
        out.extend_from_slice(payload);
        out
    }

    #[cfg(windows)]
    #[test]
    fn windows_wia_raw_header_size_matches_80_bytes() {
        assert_eq!(
            core::mem::size_of::<windows::Win32::Devices::ImageAcquisition::WIA_RAW_HEADER>(),
            80
        );
    }

    #[test]
    fn wia_raw_decodes_all_five_formats_polarity_bgr_bottom_up_stride_and_zero_dpi_fallback() {
        // 1. `Bw1` with `WIA_PHOTO_WHITE_0` (`1`), width = 10 (6 padding bits!), `BytesPerLine = 4`,
        // bottom-up (`LineOrder = 2`), `XRes = 0` -> fallback DPI `(300.0, 600.0)`.
        // Row 0 in bottom-up stream is bottom row (y = 1); Row 1 is top row (y = 0).
        // Under `WHITE_0`, `0` = white (`1` in our layout), `1` = black (`0` in our layout).
        let bw_payload = [
            0b1111_0000,
            0b00_111111,
            0xFF,
            0xFF, // bottom row (y=1): inverted -> 0b0000_1111, 0b11_000000 (padding masked to 0!)
            0b0101_0101,
            0b10_000000,
            0x00,
            0x00, // top row (y=0): inverted -> 0b1010_1010, 0b01_000000 (padding masked to 0!)
        ];
        let bw_stream = build_wraw(
            84,
            0,
            600,
            10,
            2,
            4,
            1,
            1,
            WIA_DATA_THRESHOLD as u32,
            [1, 0, 0, 0, 0, 0, 0, 0],
            0,
            WIA_PHOTO_WHITE_0 as u32,
            WIA_LINE_ORDER_BOTTOM_TO_TOP,
            12,
            &bw_payload,
        );
        let (Pixels::Raw(bw_img), bw_dpi) =
            decode_stream(bw_stream, TransferMode::RawWia, (300.0, 300.0)).unwrap()
        else {
            panic!("expected Raw");
        };
        assert_eq!(bw_dpi, (300.0, 600.0));
        assert_eq!(bw_img.format(), Format::Bw1);
        assert_eq!(bw_img.width(), 10);
        assert_eq!(bw_img.height(), 2);
        assert_eq!(
            bw_img.data(),
            &[0b1010_1010, 0b0100_0000, 0b0000_1111, 0b1100_0000]
        );

        // 2. `Gray8` with `BytesPerLine = 0` (packed) and `WIA_PHOTO_WHITE_0` (`1`).
        let g8_stream = build_wraw(
            80,
            200,
            200,
            3,
            1,
            0,
            8,
            1,
            WIA_DATA_GRAYSCALE as u32,
            [8, 0, 0, 0, 0, 0, 0, 0],
            0,
            WIA_PHOTO_WHITE_0 as u32,
            WIA_LINE_ORDER_TOP_TO_BOTTOM,
            0,
            &[0x00, 0x40, 0xFF],
        );
        let (Pixels::Raw(g8_img), g8_dpi) =
            decode_stream(g8_stream, TransferMode::RawWia, (300.0, 300.0)).unwrap()
        else {
            panic!("expected Raw");
        };
        assert_eq!(g8_dpi, (200.0, 200.0));
        assert_eq!(g8_img.format(), Format::Gray8);
        assert_eq!(g8_img.data(), &[0xFF, 0xBF, 0x00]);

        // 3. `Gray16` with `WIA_PHOTO_WHITE_0` (`1`) — inverts 16-bit LE samples (`0x1234` -> `0xEDCB`).
        let g16_stream = build_wraw(
            80,
            400,
            400,
            2,
            1,
            0,
            16,
            1,
            WIA_DATA_GRAYSCALE as u32,
            [16, 0, 0, 0, 0, 0, 0, 0],
            0,
            WIA_PHOTO_WHITE_0 as u32,
            WIA_LINE_ORDER_TOP_TO_BOTTOM,
            0,
            &[0x34, 0x12, 0x00, 0x00],
        );
        let (Pixels::Raw(g16_img), _) =
            decode_stream(g16_stream, TransferMode::RawWia, (300.0, 300.0)).unwrap()
        else {
            panic!("expected Raw");
        };
        assert_eq!(g16_img.format(), Format::Gray16);
        assert_eq!(g16_img.data(), &[0xCB, 0xED, 0xFF, 0xFF]);

        // 4. `Rgb8` (`WIA_DATA_RAW_BGR`) with `WIA_PHOTO_WHITE_0` (`1`): swaps B/R and does NOT invert color!
        let rgb8_stream = build_wraw(
            80,
            300,
            300,
            1,
            1,
            0,
            24,
            3,
            WIA_DATA_RAW_BGR as u32,
            [8, 8, 8, 0, 0, 0, 0, 0],
            0,
            WIA_PHOTO_WHITE_0 as u32,
            WIA_LINE_ORDER_TOP_TO_BOTTOM,
            0,
            &[0x33, 0x22, 0x11], // B=0x33, G=0x22, R=0x11 -> R=0x11, G=0x22, B=0x33
        );
        let (Pixels::Raw(rgb8_img), _) =
            decode_stream(rgb8_stream, TransferMode::RawWia, (300.0, 300.0)).unwrap()
        else {
            panic!("expected Raw");
        };
        assert_eq!(rgb8_img.format(), Format::Rgb8);
        assert_eq!(rgb8_img.data(), &[0x11, 0x22, 0x33]);

        // 5. `Rgb16` (`WIA_DATA_RAW_BGR`, 48-bit): swaps 16-bit LE B and R channels cleanly.
        let rgb16_stream = build_wraw(
            80,
            600,
            600,
            1,
            1,
            0,
            48,
            3,
            WIA_DATA_RAW_BGR as u32,
            [16, 16, 16, 0, 0, 0, 0, 0],
            0,
            WIA_PHOTO_WHITE_1 as u32,
            WIA_LINE_ORDER_TOP_TO_BOTTOM,
            0,
            &[0x05, 0x06, 0x03, 0x04, 0x01, 0x02],
        );
        let (Pixels::Raw(rgb16_img), _) =
            decode_stream(rgb16_stream, TransferMode::RawWia, (300.0, 300.0)).unwrap()
        else {
            panic!("expected Raw");
        };
        assert_eq!(rgb16_img.format(), Format::Rgb16);
        assert_eq!(rgb16_img.data(), &[0x01, 0x02, 0x03, 0x04, 0x05, 0x06]);
    }

    #[test]
    fn rejects_malformed_wia_raw_headers_and_validates_bmp_memorybmp_and_encoded_magic() {
        // HeaderSize < 80 rejected.
        let mut bad_hdr = build_wraw(
            80,
            300,
            300,
            1,
            1,
            1,
            8,
            1,
            WIA_DATA_GRAYSCALE as u32,
            [8; 8],
            0,
            0,
            1,
            0,
            &[0x42],
        );
        bad_hdr[8..12].copy_from_slice(&76u32.to_le_bytes());
        assert!(decode_stream(bad_hdr, TransferMode::RawWia, (300.0, 300.0)).is_err());

        // Non-zero compression rejected.
        let compressed = build_wraw(
            80,
            300,
            300,
            1,
            1,
            1,
            8,
            1,
            WIA_DATA_GRAYSCALE as u32,
            [8; 8],
            5,
            0,
            1,
            0,
            &[0x42],
        );
        assert!(decode_stream(compressed, TransferMode::RawWia, (300.0, 300.0)).is_err());

        // `RawDataOffset = u32::MAX` checked overflow rejected without panicking.
        let mut overflow_off = build_wraw(
            80,
            300,
            300,
            1,
            1,
            1,
            8,
            1,
            WIA_DATA_GRAYSCALE as u32,
            [8; 8],
            0,
            0,
            1,
            0,
            &[0x42],
        );
        overflow_off[64..68].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(decode_stream(overflow_off, TransferMode::RawWia, (300.0, 300.0)).is_err());

        // `RawBmp` requires `"BM"` header, whereas `RawMemoryBmp` accepts both bare DIB and `"BM"`.
        let mut bare_dib = Vec::new();
        bare_dib.extend_from_slice(&40u32.to_le_bytes());
        bare_dib.extend_from_slice(&1i32.to_le_bytes());
        bare_dib.extend_from_slice(&1i32.to_le_bytes());
        bare_dib.extend_from_slice(&1u16.to_le_bytes());
        bare_dib.extend_from_slice(&24u16.to_le_bytes());
        bare_dib.extend_from_slice(&0u32.to_le_bytes());
        bare_dib.extend_from_slice(&4u32.to_le_bytes());
        bare_dib.extend_from_slice(&[0u8; 16]);
        bare_dib.extend_from_slice(&[0x30, 0x20, 0x10, 0x00]);

        assert!(decode_stream(bare_dib.clone(), TransferMode::RawBmp, (300.0, 300.0)).is_err());
        let (Pixels::Raw(mem_dib), _) =
            decode_stream(bare_dib.clone(), TransferMode::RawMemoryBmp, (300.0, 300.0)).unwrap()
        else {
            panic!("expected Raw");
        };
        assert_eq!(mem_dib.data(), &[0x10, 0x20, 0x30]);

        let mut bmp_file = Vec::new();
        bmp_file.extend_from_slice(b"BM");
        bmp_file.extend_from_slice(&((14 + bare_dib.len()) as u32).to_le_bytes());
        bmp_file.extend_from_slice(&0u32.to_le_bytes());
        bmp_file.extend_from_slice(&54u32.to_le_bytes());
        bmp_file.extend_from_slice(&bare_dib);

        assert!(decode_stream(bmp_file.clone(), TransferMode::RawBmp, (300.0, 300.0)).is_ok());
        assert!(
            decode_stream(bmp_file.clone(), TransferMode::RawMemoryBmp, (300.0, 300.0)).is_ok()
        );
        let (Pixels::Encoded(enc_bmp), _) = decode_stream(
            bmp_file,
            TransferMode::Encoded(Encoding::Bmp),
            (300.0, 300.0),
        )
        .unwrap() else {
            panic!("expected Encoded");
        };
        assert_eq!(enc_bmp.kind, EncodedKind::Bmp);

        // Encoded magic validation accepts valid PNG/JPEG/TIFF and rejects empty or mismatched headers.
        assert!(
            decode_stream(
                b"\x89PNG\r\n\x1a\nIHDR".to_vec(),
                TransferMode::Encoded(Encoding::Png),
                (300.0, 300.0)
            )
            .is_ok()
        );
        assert!(
            decode_stream(
                vec![0xff, 0xd8, 0xff, 0xe0, 0x00],
                TransferMode::Encoded(Encoding::Jpeg),
                (300.0, 300.0)
            )
            .is_ok()
        );
        assert!(
            decode_stream(
                b"II*\0\x08\0\0\0".to_vec(),
                TransferMode::Encoded(Encoding::Tiff),
                (300.0, 300.0)
            )
            .is_ok()
        );
        assert!(
            decode_stream(
                b"MM\0*\0\0\0\x08".to_vec(),
                TransferMode::Encoded(Encoding::Tiff),
                (300.0, 300.0)
            )
            .is_ok()
        );
        assert!(
            decode_stream(
                Vec::new(),
                TransferMode::Encoded(Encoding::Png),
                (300.0, 300.0)
            )
            .is_err()
        );
        assert!(
            decode_stream(
                vec![0xff, 0xd8, 0xff],
                TransferMode::Encoded(Encoding::Png),
                (300.0, 300.0)
            )
            .is_err()
        );

        // `WIA_DATA_DITHER` (`1`) decodes cleanly as `Bw1`, whereas unknown `DataType` (e.g. CMYK `9`),
        // non-zero `PaletteSize`, `LineOrder == 0`, unsupported `Version`, asymmetric `BitsPerChannel`
        // (`[10, 10, 10, 2, ..]`), or `RawDataSize` past payload are all rejected.
        let dither = build_wraw(
            80,
            300,
            300,
            8,
            1,
            1,
            1,
            1,
            WIA_DATA_DITHER as u32,
            [1, 0, 0, 0, 0, 0, 0, 0],
            0,
            0,
            WIA_LINE_ORDER_TOP_TO_BOTTOM,
            0,
            &[0b1010_0101],
        );
        let (Pixels::Raw(dither_img), _) =
            decode_stream(dither, TransferMode::RawWia, (0.0, f32::NAN)).unwrap()
        else {
            panic!("expected Raw");
        };
        assert_eq!(dither_img.format(), Format::Bw1);
        assert_eq!(dither_img.data(), &[0b1010_0101]);

        // CMYK (`DataType = 9`, 32 bpp, 4 channels) rejected instead of dropping K channel.
        let cmyk = build_wraw(
            80,
            300,
            300,
            1,
            1,
            4,
            32,
            4,
            9,
            [8, 8, 8, 8, 0, 0, 0, 0],
            0,
            0,
            WIA_LINE_ORDER_TOP_TO_BOTTOM,
            0,
            &[1, 2, 3, 4],
        );
        assert!(decode_stream(cmyk, TransferMode::RawWia, (300.0, 300.0)).is_err());

        // Asymmetric `BitsPerChannel` (`10-10-10-2` packed in 32 bpp) rejected.
        let rgb10 = build_wraw(
            80,
            300,
            300,
            1,
            1,
            4,
            32,
            4,
            WIA_DATA_COLOR as u32,
            [10, 10, 10, 2, 0, 0, 0, 0],
            0,
            0,
            WIA_LINE_ORDER_TOP_TO_BOTTOM,
            0,
            &[1, 2, 3, 4],
        );
        assert!(decode_stream(rgb10, TransferMode::RawWia, (300.0, 300.0)).is_err());

        // `LineOrder == 0`, non-zero `PaletteSize`, and `RawDataSize > payload.len()` rejected.
        let line_order_zero = build_wraw(
            80,
            300,
            300,
            1,
            1,
            1,
            8,
            1,
            WIA_DATA_GRAYSCALE as u32,
            [8, 0, 0, 0, 0, 0, 0, 0],
            0,
            0,
            0,
            0,
            &[0x42],
        );
        assert!(decode_stream(line_order_zero, TransferMode::RawWia, (300.0, 300.0)).is_err());

        let mut paletted = build_wraw(
            80,
            300,
            300,
            1,
            1,
            1,
            8,
            1,
            WIA_DATA_GRAYSCALE as u32,
            [8, 0, 0, 0, 0, 0, 0, 0],
            0,
            0,
            WIA_LINE_ORDER_TOP_TO_BOTTOM,
            0,
            &[0x42],
        );
        paletted[76..80].copy_from_slice(&1024u32.to_le_bytes());
        assert!(decode_stream(paletted, TransferMode::RawWia, (300.0, 300.0)).is_err());

        let mut bad_raw_size = build_wraw(
            80,
            300,
            300,
            1,
            1,
            1,
            8,
            1,
            WIA_DATA_GRAYSCALE as u32,
            [8, 0, 0, 0, 0, 0, 0, 0],
            0,
            0,
            WIA_LINE_ORDER_TOP_TO_BOTTOM,
            0,
            &[0x42],
        );
        bad_raw_size[68..72].copy_from_slice(&99u32.to_le_bytes());
        assert!(decode_stream(bad_raw_size, TransferMode::RawWia, (300.0, 300.0)).is_err());
    }
}
