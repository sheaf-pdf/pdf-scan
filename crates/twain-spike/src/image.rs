//! Page images as TWAIN hands them over, decoded into one form so that a page
//! through native transfer and one through memory transfer can be compared:
//! rows top down, packed with no padding, RGB in that order, 8-bit gray with 0
//! black, 1-bit with the first pixel in the high bit and 0 black, and the bits
//! past each row's last pixel zero.
//!
//! A driver's bytes are untrusted. A DIB's header, palette and rows are checked
//! against the size of the block that holds them, and each memory tile against
//! the image's bounds, its own buffer and the tiles before it, before anything
//! is read.

use std::fmt;

use crate::ffi::{TWPT_BW, TWPT_GRAY, TWPT_RGB};

/// The largest image, in bytes, the spike assembles.
pub const MAX_BYTES: u64 = 1 << 30;
/// The most pixels an image may have (16384 by 16384).
pub const MAX_PIXELS: u64 = 1 << 28;

/// The pixel formats TWAIN's sample source delivers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Bw,
    Gray,
    Rgb,
}

impl Format {
    pub fn bits(self) -> usize {
        match self {
            Format::Bw => 1,
            Format::Gray => 8,
            Format::Rgb => 24,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Format::Bw => "black and white",
            Format::Gray => "gray",
            Format::Rgb => "RGB",
        }
    }

    /// The format `DAT_IMAGEINFO` describes. Only the first `BitsPerSample`
    /// entry is trusted: some drivers set no other.
    pub fn from_image_info(
        pixel_type: i16,
        samples: i16,
        first_bits: i16,
    ) -> Result<Format, Refusal> {
        match (u16::try_from(pixel_type), samples, first_bits) {
            (Ok(TWPT_BW), 1, 1) => Ok(Format::Bw),
            (Ok(TWPT_GRAY), 1, 8) => Ok(Format::Gray),
            (Ok(TWPT_RGB), 3, 8) => Ok(Format::Rgb),
            _ => Err(Refusal::Format(format!(
                "pixel type {pixel_type}, {samples} samples of {first_bits} bits"
            ))),
        }
    }
}

/// Which value is black in a memory transfer's gray or 1-bit data:
/// `ICAP_PIXELFLAVOR`'s `TWPF_CHOCOLATE` (0 black) or `TWPF_VANILLA` (0 white).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Polarity {
    ZeroBlack,
    ZeroWhite,
}

/// An image in the comparable form.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub format: Format,
    pub data: Vec<u8>,
}

/// The bytes of one row of `width` pixels, unpadded.
pub fn row_bytes(width: u32, format: Format) -> usize {
    (width as usize * format.bits()).div_ceil(8)
}

/// Why an image was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    Size { width: i64, height: i64 },
    TooLarge { width: u32, height: u32 },
    Format(String),
    Short { need: u64, have: usize },
    Header(String),
    Palette(String),
    Tile { index: usize, why: String },
    Overlap { index: usize },
    Uncovered { pixels: u64 },
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Refusal::Size { width, height } => write!(f, "an image of {width} by {height}"),
            Refusal::TooLarge { width, height } => {
                write!(f, "{width} by {height} is past the caps")
            }
            Refusal::Format(what) => write!(f, "an unsupported format: {what}"),
            Refusal::Short { need, have } => {
                write!(f, "the block is {have} bytes, and needs {need}")
            }
            Refusal::Header(what) => write!(f, "the DIB's header: {what}"),
            Refusal::Palette(what) => write!(f, "the DIB's palette: {what}"),
            Refusal::Tile { index, why } => write!(f, "tile {index}: {why}"),
            Refusal::Overlap { index } => write!(f, "tile {index} overlaps an earlier one"),
            Refusal::Uncovered { pixels } => write!(f, "{pixels} pixels no tile covered"),
        }
    }
}

/// Bytes the image needs, after its size is checked against the caps.
fn checked_size(width: u32, height: u32, format: Format) -> Result<usize, Refusal> {
    if width == 0 || height == 0 {
        return Err(Refusal::Size {
            width: i64::from(width),
            height: i64::from(height),
        });
    }
    let pixels = u64::from(width) * u64::from(height);
    let bytes = row_bytes(width, format) as u64 * u64::from(height);
    if pixels > MAX_PIXELS || bytes > MAX_BYTES {
        return Err(Refusal::TooLarge { width, height });
    }
    Ok(bytes as usize)
}

/// What a DIB said of itself, for the report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DibFacts {
    pub bits: u16,
    pub bottom_up: bool,
    /// Its palette, as RGB.
    pub palette: Vec<[u8; 3]>,
}

/// A packed DIB (a `BITMAPINFOHEADER`, its palette, then its rows, each padded
/// to four bytes), as a native transfer's handle holds it on Windows.
pub fn decode_dib(bytes: &[u8]) -> Result<(Image, DibFacts), Refusal> {
    if bytes.len() < 40 {
        return Err(Refusal::Short {
            need: 40,
            have: bytes.len(),
        });
    }
    let header_size = u32_at(bytes, 0);
    let width = i32_at(bytes, 4);
    let height = i32_at(bytes, 8);
    let planes = u16_at(bytes, 12);
    let bits = u16_at(bytes, 14);
    let compression = u32_at(bytes, 16);
    let colors_used = u32_at(bytes, 32);
    if header_size < 40 || header_size as usize > bytes.len() {
        return Err(Refusal::Header(format!("its size is {header_size}")));
    }
    if planes != 1 {
        return Err(Refusal::Header(format!("{planes} planes")));
    }
    if compression != 0 {
        return Err(Refusal::Header(format!(
            "compression {compression}, not BI_RGB"
        )));
    }
    let format = match bits {
        1 => Format::Bw,
        8 => Format::Gray,
        24 => Format::Rgb,
        other => return Err(Refusal::Header(format!("{other} bits a pixel"))),
    };
    if width <= 0 || height == 0 || height == i32::MIN {
        return Err(Refusal::Size {
            width: i64::from(width),
            height: i64::from(height),
        });
    }
    let (w, h) = (width as u32, height.unsigned_abs());
    let bottom_up = height > 0;
    let total = checked_size(w, h, format)?;

    let most = if bits <= 8 { 1u32 << bits } else { 256 };
    let entries = match colors_used {
        0 if bits <= 8 => most,
        0 => 0,
        n => n,
    };
    if entries > most {
        return Err(Refusal::Palette(format!(
            "{entries} entries, more than {most} for {bits} bits"
        )));
    }
    let palette_at = header_size as u64;
    let rows_at = palette_at + u64::from(entries) * 4;
    let stride = (u64::from(w) * u64::from(bits)).div_ceil(32) * 4;
    let need = rows_at + stride * u64::from(h);
    if (bytes.len() as u64) < need {
        return Err(Refusal::Short {
            need,
            have: bytes.len(),
        });
    }
    let palette: Vec<[u8; 3]> = (0..entries as usize)
        .map(|i| {
            let at = palette_at as usize + i * 4;
            [bytes[at + 2], bytes[at + 1], bytes[at]]
        })
        .collect();

    let (rows_at, stride) = (rows_at as usize, stride as usize);
    let out_row = row_bytes(w, format);
    let mut data = Vec::with_capacity(total);
    let invert_bw = match format {
        Format::Bw => match palette.as_slice() {
            [[0, 0, 0], [255, 255, 255]] => false,
            [[255, 255, 255], [0, 0, 0]] => true,
            _ => {
                return Err(Refusal::Palette(
                    "a 1-bit palette that isn't black and white".to_owned(),
                ));
            }
        },
        Format::Gray if palette.iter().any(|&[r, g, b]| r != g || g != b) => {
            return Err(Refusal::Palette(
                "an 8-bit DIB with a color palette".to_owned(),
            ));
        }
        _ => false,
    };
    for y in 0..h as usize {
        let file_row = if bottom_up { h as usize - 1 - y } else { y };
        let src = &bytes[rows_at + file_row * stride..][..stride];
        match format {
            Format::Rgb => {
                for &[b, g, r] in src[..out_row].as_chunks::<3>().0 {
                    data.extend_from_slice(&[r, g, b]);
                }
            }
            Format::Gray => {
                for &index in &src[..out_row] {
                    let Some(&[v, _, _]) = palette.get(usize::from(index)) else {
                        return Err(Refusal::Palette(format!(
                            "pixel index {index} past its {entries} entries"
                        )));
                    };
                    data.push(v);
                }
            }
            Format::Bw => data.extend(
                src[..out_row]
                    .iter()
                    .map(|&b| if invert_bw { !b } else { b }),
            ),
        }
    }
    let mut image = Image {
        width: w,
        height: h,
        format,
        data,
    };
    clear_padding_bits(&mut image);
    Ok((
        image,
        DibFacts {
            bits,
            bottom_up,
            palette,
        },
    ))
}

/// One buffer of a memory transfer: `rows` rows of `columns` pixels at (`x`,
/// `y`), `bytes_per_row` apart.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tile {
    pub x: u32,
    pub y: u32,
    pub columns: u32,
    pub rows: u32,
    pub bytes_per_row: u32,
    pub data: Vec<u8>,
}

impl Tile {
    /// A tile copied out of the transfer's buffer, `written` (the source's
    /// `BytesWritten`) checked against the buffer first.
    pub fn from_buffer(
        (x, y, columns, rows, bytes_per_row): (u32, u32, u32, u32, u32),
        written: u32,
        buffer: &[u8],
    ) -> Result<Tile, String> {
        if written as usize > buffer.len() {
            return Err(format!(
                "{written} bytes written into a {}-byte buffer",
                buffer.len()
            ));
        }
        Ok(Tile {
            x,
            y,
            columns,
            rows,
            bytes_per_row,
            data: buffer[..written as usize].to_vec(),
        })
    }
}

/// The tiles of a memory transfer put together: each inside the image, none
/// overlapping, every pixel covered.
pub fn assemble(
    width: u32,
    height: u32,
    format: Format,
    polarity: Polarity,
    tiles: &[Tile],
) -> Result<Image, Refusal> {
    let total = checked_size(width, height, format)?;
    let stride = row_bytes(width, format);
    let mut data = vec![0u8; total];
    let mut covered = Coverage::new(width, height);
    for (index, t) in tiles.iter().enumerate() {
        let bad = |why: String| Refusal::Tile { index, why };
        if t.columns == 0 || t.rows == 0 {
            return Err(bad("it's empty".to_owned()));
        }
        if u64::from(t.x) + u64::from(t.columns) > u64::from(width)
            || u64::from(t.y) + u64::from(t.rows) > u64::from(height)
        {
            return Err(bad(format!(
                "{} by {} at ({}, {}) runs past the {width} by {height} image",
                t.columns, t.rows, t.x, t.y
            )));
        }
        let tile_row = row_bytes(t.columns, format);
        if (t.bytes_per_row as usize) < tile_row {
            return Err(bad(format!(
                "{} bytes a row, under the {tile_row} its columns need",
                t.bytes_per_row
            )));
        }
        let need = (t.rows as usize - 1) * t.bytes_per_row as usize + tile_row;
        if t.data.len() < need {
            return Err(bad(format!("{} bytes, and needs {need}", t.data.len())));
        }
        if format == Format::Bw
            && (t.x % 8 != 0 || (t.columns % 8 != 0 && t.x + t.columns != width))
        {
            return Err(bad(
                "it starts or ends within a byte of 1-bit rows".to_owned()
            ));
        }
        if !covered.mark(t.x, t.y, t.columns, t.rows) {
            return Err(Refusal::Overlap { index });
        }
        let x_byte = t.x as usize * format.bits() / 8;
        for r in 0..t.rows as usize {
            let dst = (t.y as usize + r) * stride + x_byte;
            let src = r * t.bytes_per_row as usize;
            data[dst..dst + tile_row].copy_from_slice(&t.data[src..src + tile_row]);
        }
    }
    let missing = u64::from(width) * u64::from(height) - covered.count;
    if missing > 0 {
        return Err(Refusal::Uncovered { pixels: missing });
    }
    if polarity == Polarity::ZeroWhite && format != Format::Rgb {
        data.iter_mut().for_each(|b| *b = !*b);
    }
    let mut image = Image {
        width,
        height,
        format,
        data,
    };
    clear_padding_bits(&mut image);
    Ok(image)
}

/// The first difference between two images, if any.
pub fn compare(a: &Image, b: &Image) -> Result<(), String> {
    if (a.width, a.height, a.format) != (b.width, b.height, b.format) {
        return Err(format!(
            "{} by {} {} against {} by {} {}",
            a.width,
            a.height,
            a.format.name(),
            b.width,
            b.height,
            b.format.name()
        ));
    }
    let Some(at) = a.data.iter().zip(&b.data).position(|(x, y)| x != y) else {
        return Ok(());
    };
    let differing = a.data.iter().zip(&b.data).filter(|(x, y)| x != y).count();
    let stride = row_bytes(a.width, a.format);
    let (row, byte) = (at / stride, at % stride);
    let column = match a.format {
        Format::Rgb => byte / 3,
        Format::Gray => byte,
        Format::Bw => byte * 8 + (a.data[at] ^ b.data[at]).leading_zeros() as usize,
    };
    Err(format!(
        "{differing} bytes differ, the first at pixel ({column}, {row})"
    ))
}

/// A palette in a few words, for the report.
pub fn describe_palette(palette: &[[u8; 3]]) -> String {
    match palette {
        [] => "none".to_owned(),
        [[0, 0, 0], [255, 255, 255]] => "2 entries, black then white".to_owned(),
        [[255, 255, 255], [0, 0, 0]] => "2 entries, white then black".to_owned(),
        p if p.len() == 256 && p.iter().enumerate().all(|(i, &c)| c == [i as u8; 3]) => {
            "256 entries, an identity gray ramp".to_owned()
        }
        p => format!("{} entries", p.len()),
    }
}

/// 1-bit rows' bits past the last pixel, zeroed, so padding never compares.
fn clear_padding_bits(image: &mut Image) {
    let used = image.width % 8;
    if image.format != Format::Bw || used == 0 {
        return;
    }
    let keep = 0xffu8 << (8 - used);
    let stride = row_bytes(image.width, image.format);
    for row in image.data.chunks_exact_mut(stride) {
        if let Some(last) = row.last_mut() {
            *last &= keep;
        }
    }
}

/// Which pixels the tiles so far have covered.
struct Coverage {
    width: u32,
    bits: Vec<u64>,
    count: u64,
}

impl Coverage {
    fn new(width: u32, height: u32) -> Coverage {
        let pixels = u64::from(width) * u64::from(height);
        Coverage {
            width,
            bits: vec![0; pixels.div_ceil(64) as usize],
            count: 0,
        }
    }

    /// Marks a rectangle; false if any of it was marked already.
    fn mark(&mut self, x: u32, y: u32, columns: u32, rows: u32) -> bool {
        for r in 0..u64::from(rows) {
            let start = (u64::from(y) + r) * u64::from(self.width) + u64::from(x);
            for p in start..start + u64::from(columns) {
                let (word, bit) = ((p / 64) as usize, p % 64);
                if self.bits[word] & (1 << bit) != 0 {
                    return false;
                }
                self.bits[word] |= 1 << bit;
            }
        }
        self.count += u64::from(columns) * u64::from(rows);
        true
    }
}

fn u16_at(b: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([b[at], b[at + 1]])
}

fn u32_at(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}

fn i32_at(b: &[u8], at: usize) -> i32 {
    u32_at(b, at) as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A packed DIB from rows given top down, each row's pixel bytes as the DIB
    /// holds them (BGR, indices or bits), padded here.
    fn dib(
        width: i32,
        height: i32,
        bits: u16,
        colors_used: u32,
        palette: &[[u8; 3]],
        rows: &[Vec<u8>],
    ) -> Vec<u8> {
        let mut b = Vec::new();
        for v in [40u32, width as u32, height as u32] {
            b.extend_from_slice(&v.to_le_bytes());
        }
        b.extend_from_slice(&1u16.to_le_bytes());
        b.extend_from_slice(&bits.to_le_bytes());
        for v in [0u32, 0, 0, 0, colors_used, 0] {
            b.extend_from_slice(&v.to_le_bytes());
        }
        for &[r, g, bl] in palette {
            b.extend_from_slice(&[bl, g, r, 0]);
        }
        let stride = (width.unsigned_abs() as usize * bits as usize).div_ceil(32) * 4;
        let ordered: Vec<&Vec<u8>> = if height > 0 {
            rows.iter().rev().collect()
        } else {
            rows.iter().collect()
        };
        for row in ordered {
            let mut padded = row.clone();
            padded.resize(stride, 0xee);
            b.extend_from_slice(&padded);
        }
        b
    }

    fn gray_ramp() -> Vec<[u8; 3]> {
        (0..=255u8).map(|i| [i; 3]).collect()
    }

    const BW: [[u8; 3]; 2] = [[0, 0, 0], [255, 255, 255]];

    #[test]
    fn a_bottom_up_24_bit_dib_with_padding() {
        // 3 pixels a row: 9 bytes, padded to 12.
        let top = vec![1, 2, 3, 4, 5, 6, 7, 8, 9];
        let bottom = vec![10, 11, 12, 13, 14, 15, 16, 17, 18];
        let (img, facts) = decode_dib(&dib(3, 2, 24, 0, &[], &[top, bottom])).unwrap();
        assert_eq!((img.width, img.height, img.format), (3, 2, Format::Rgb));
        assert_eq!(
            img.data,
            [
                3, 2, 1, 6, 5, 4, 9, 8, 7, 12, 11, 10, 15, 14, 13, 18, 17, 16
            ]
        );
        assert!(facts.bottom_up && facts.palette.is_empty());
    }

    #[test]
    fn a_top_down_dib() {
        let (img, facts) =
            decode_dib(&dib(1, -2, 24, 0, &[], &[vec![1, 2, 3], vec![4, 5, 6]])).unwrap();
        assert_eq!(img.data, [3, 2, 1, 6, 5, 4]);
        assert!(!facts.bottom_up);
    }

    #[test]
    fn an_8_bit_dib_through_its_gray_palette() {
        let rows = vec![vec![0, 1, 2, 254, 255]];
        let (img, facts) = decode_dib(&dib(5, 1, 8, 0, &gray_ramp(), &rows)).unwrap();
        assert_eq!(img.data, [0, 1, 2, 254, 255]);
        assert_eq!(
            describe_palette(&facts.palette),
            "256 entries, an identity gray ramp"
        );
        let inverted: Vec<[u8; 3]> = (0..=255u8).map(|i| [255 - i; 3]).collect();
        let (img, _) = decode_dib(&dib(5, 1, 8, 0, &inverted, &rows)).unwrap();
        assert_eq!(img.data, [255, 254, 253, 1, 0]);
    }

    #[test]
    fn a_1_bit_dib_of_odd_width_and_either_palette() {
        // 10 pixels: 2 bytes, the last 6 bits of the second set as junk.
        let rows = vec![
            vec![0b1010_1010, 0b1111_1111],
            vec![0b0000_0001, 0b0100_0000],
        ];
        let (img, facts) = decode_dib(&dib(10, 2, 1, 2, &BW, &rows)).unwrap();
        assert_eq!(
            img.data,
            [0b1010_1010, 0b1100_0000, 0b0000_0001, 0b0100_0000]
        );
        assert_eq!(
            describe_palette(&facts.palette),
            "2 entries, black then white"
        );
        let (img, _) = decode_dib(&dib(10, 2, 1, 2, &[BW[1], BW[0]], &rows)).unwrap();
        assert_eq!(
            img.data,
            [0b0101_0101, 0b0000_0000, 0b1111_1110, 0b1000_0000]
        );
    }

    #[test]
    fn dibs_that_dont_hold_up_are_refused() {
        let ok = dib(5, 1, 8, 0, &gray_ramp(), &[vec![0; 5]]);
        assert!(matches!(
            decode_dib(&ok[..ok.len() - 1]),
            Err(Refusal::Short { .. })
        ));
        assert!(matches!(
            decode_dib(&ok[..39]),
            Err(Refusal::Short { need: 40, .. })
        ));
        let many = dib(5, 1, 8, 300, &gray_ramp(), &[vec![0; 5]]);
        assert!(matches!(decode_dib(&many), Err(Refusal::Palette(_))));
        let colored = dib(1, 1, 8, 2, &[[0, 0, 0], [255, 0, 0]], &[vec![1]]);
        assert!(matches!(decode_dib(&colored), Err(Refusal::Palette(_))));
        let past = dib(1, 1, 8, 2, &[[0, 0, 0], [9, 9, 9]], &[vec![7]]);
        assert!(matches!(decode_dib(&past), Err(Refusal::Palette(_))));
        let not_bw = dib(8, 1, 1, 2, &[[0, 0, 0], [9, 9, 9]], &[vec![0]]);
        assert!(matches!(decode_dib(&not_bw), Err(Refusal::Palette(_))));
        let mut b16 = ok.clone();
        b16[14] = 16;
        assert!(matches!(decode_dib(&b16), Err(Refusal::Header(_))));
        let mut compressed = ok.clone();
        compressed[16] = 1;
        assert!(matches!(decode_dib(&compressed), Err(Refusal::Header(_))));
        let mut small_header = ok.clone();
        small_header[0] = 12;
        assert!(matches!(decode_dib(&small_header), Err(Refusal::Header(_))));
        assert!(matches!(
            decode_dib(&dib(5, 0, 8, 0, &gray_ramp(), &[])),
            Err(Refusal::Size { .. })
        ));
        assert!(matches!(
            decode_dib(&dib(-5, 1, 8, 0, &gray_ramp(), &[])),
            Err(Refusal::Size { .. })
        ));
        let huge = dib(100_000, 100_000, 24, 0, &[], &[]);
        assert!(matches!(decode_dib(&huge), Err(Refusal::TooLarge { .. })));
    }

    fn strips(
        width: u32,
        height: u32,
        rows_each: u32,
        format: Format,
        image: &[u8],
        pad: u32,
    ) -> Vec<Tile> {
        let row = row_bytes(width, format);
        let bpr = row as u32 + pad;
        (0..height)
            .step_by(rows_each as usize)
            .map(|y| {
                let rows = rows_each.min(height - y);
                let mut data = Vec::new();
                for r in 0..rows as usize {
                    data.extend_from_slice(&image[(y as usize + r) * row..][..row]);
                    data.extend(std::iter::repeat_n(0xcc, pad as usize));
                }
                Tile {
                    x: 0,
                    y,
                    columns: width,
                    rows,
                    bytes_per_row: bpr,
                    data,
                }
            })
            .collect()
    }

    #[test]
    fn strips_out_of_order_assemble() {
        let image: Vec<u8> = (0..5 * 7 * 3).map(|i| i as u8).collect();
        let mut tiles = strips(5, 7, 3, Format::Rgb, &image, 1);
        tiles.reverse();
        let img = assemble(5, 7, Format::Rgb, Polarity::ZeroBlack, &tiles).unwrap();
        assert_eq!(img.data, image);
    }

    #[test]
    fn tiles_in_two_dimensions_out_of_order_assemble() {
        // A 4 by 4 gray image in four 2 by 2 tiles.
        let image: Vec<u8> = (0..16).collect();
        let tile = |x: u32, y: u32| {
            let mut data = Vec::new();
            for r in 0..2 {
                data.extend_from_slice(&image[((y + r) * 4 + x) as usize..][..2]);
                data.extend_from_slice(&[0, 0]);
            }
            Tile {
                x,
                y,
                columns: 2,
                rows: 2,
                bytes_per_row: 4,
                data,
            }
        };
        let tiles = [tile(2, 2), tile(0, 2), tile(2, 0), tile(0, 0)];
        assert_eq!(
            assemble(4, 4, Format::Gray, Polarity::ZeroBlack, &tiles)
                .unwrap()
                .data,
            image
        );
    }

    #[test]
    fn one_bit_strips_of_odd_width_assemble_with_padding_cleared() {
        let image = vec![0b1010_1010, 0b1111_1111, 0b0000_0001, 0b0111_1111];
        let tiles = strips(10, 2, 1, Format::Bw, &image, 2);
        let img = assemble(10, 2, Format::Bw, Polarity::ZeroBlack, &tiles).unwrap();
        assert_eq!(
            img.data,
            [0b1010_1010, 0b1100_0000, 0b0000_0001, 0b0100_0000]
        );
    }

    #[test]
    fn vanilla_polarity_inverts_gray_and_1_bit_not_rgb() {
        let gray = strips(2, 1, 1, Format::Gray, &[0, 200], 0);
        assert_eq!(
            assemble(2, 1, Format::Gray, Polarity::ZeroWhite, &gray)
                .unwrap()
                .data,
            [255, 55]
        );
        let bw = strips(8, 1, 1, Format::Bw, &[0b1111_0000], 0);
        assert_eq!(
            assemble(8, 1, Format::Bw, Polarity::ZeroWhite, &bw)
                .unwrap()
                .data,
            [0b0000_1111]
        );
        let rgb = strips(1, 1, 1, Format::Rgb, &[1, 2, 3], 0);
        assert_eq!(
            assemble(1, 1, Format::Rgb, Polarity::ZeroWhite, &rgb)
                .unwrap()
                .data,
            [1, 2, 3]
        );
    }

    #[test]
    fn tiles_that_dont_hold_up_are_refused() {
        let image = vec![0u8; 4 * 4];
        let ok = strips(4, 4, 2, Format::Gray, &image, 0);
        let mut past = ok.clone();
        past[1].y = 3;
        assert!(matches!(
            assemble(4, 4, Format::Gray, Polarity::ZeroBlack, &past),
            Err(Refusal::Tile { index: 1, .. })
        ));
        let overlapping = [ok[0].clone(), ok[0].clone()];
        assert!(matches!(
            assemble(4, 4, Format::Gray, Polarity::ZeroBlack, &overlapping),
            Err(Refusal::Overlap { index: 1 })
        ));
        assert_eq!(
            assemble(4, 4, Format::Gray, Polarity::ZeroBlack, &ok[..1]),
            Err(Refusal::Uncovered { pixels: 8 })
        );
        let mut narrow = ok.clone();
        narrow[0].bytes_per_row = 3;
        assert!(matches!(
            assemble(4, 4, Format::Gray, Polarity::ZeroBlack, &narrow),
            Err(Refusal::Tile { index: 0, .. })
        ));
        let mut short = ok.clone();
        short[0].data.pop();
        assert!(matches!(
            assemble(4, 4, Format::Gray, Polarity::ZeroBlack, &short),
            Err(Refusal::Tile { index: 0, .. })
        ));
        let mut empty = ok.clone();
        empty[0].rows = 0;
        assert!(matches!(
            assemble(4, 4, Format::Gray, Polarity::ZeroBlack, &empty),
            Err(Refusal::Tile { index: 0, .. })
        ));
        let mid_byte = [Tile {
            x: 4,
            y: 0,
            columns: 12,
            rows: 1,
            bytes_per_row: 2,
            data: vec![0, 0],
        }];
        assert!(matches!(
            assemble(16, 1, Format::Bw, Polarity::ZeroBlack, &mid_byte),
            Err(Refusal::Tile { index: 0, .. })
        ));
        assert!(Tile::from_buffer((0, 0, 4, 1, 4), 9, &[0; 8]).is_err());
        assert_eq!(
            Tile::from_buffer((0, 0, 4, 1, 4), 4, &[1, 2, 3, 4, 5])
                .unwrap()
                .data,
            [1, 2, 3, 4]
        );
    }

    #[test]
    fn compare_names_the_first_differing_pixel() {
        let a = Image {
            width: 3,
            height: 2,
            format: Format::Rgb,
            data: vec![0; 18],
        };
        let mut b = a.clone();
        assert_eq!(compare(&a, &b), Ok(()));
        b.data[13] = 1;
        assert_eq!(
            compare(&a, &b),
            Err("1 bytes differ, the first at pixel (1, 1)".to_owned())
        );
        let bw = Image {
            width: 16,
            height: 1,
            format: Format::Bw,
            data: vec![0, 0b0010_0000],
        };
        let other = Image {
            data: vec![0, 0],
            ..bw.clone()
        };
        assert_eq!(
            compare(&bw, &other),
            Err("1 bytes differ, the first at pixel (10, 0)".to_owned())
        );
    }

    #[test]
    fn formats_from_image_info() {
        assert_eq!(
            Format::from_image_info(TWPT_RGB as i16, 3, 8),
            Ok(Format::Rgb)
        );
        assert_eq!(
            Format::from_image_info(TWPT_GRAY as i16, 1, 8),
            Ok(Format::Gray)
        );
        assert_eq!(
            Format::from_image_info(TWPT_BW as i16, 1, 1),
            Ok(Format::Bw)
        );
        assert!(Format::from_image_info(TWPT_RGB as i16, 3, 16).is_err());
        assert!(Format::from_image_info(-1, 1, 8).is_err());
    }
}
