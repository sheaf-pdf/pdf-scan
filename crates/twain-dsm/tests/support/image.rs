//! Page images as TWAIN hands them over, decoded into one form so that a page
//! through native transfer and one through memory transfer can be compared:
//! rows top down, packed with no padding, RGB in that order, 8-bit gray with 0
//! black, 1-bit with the first pixel in the high bit and 0 black, and the bits
//! past each row's last pixel zero.
//!
//! A driver's bytes are untrusted. A DIB's header, palette and rows are checked
//! against the size of the block that holds them, and each memory buffer
//! against the image's bounds, what the source wrote and the buffers before
//! it, before anything is read. Sizes are worked out in `u64`, since on a
//! 32-bit target a driver's widths and row lengths overflow `usize`. An
//! [`Assembler`] places each buffer as it comes and keeps only the image, so a
//! memory transfer costs no more than the image cap.

use std::fmt;

use twain_dsm::ffi::{TWPT_BW, TWPT_GRAY, TWPT_RGB};

/// The most pixels an image may have (16384 by 16384). It's the cap that
/// binds: at it, an RGB image is 768 MiB.
pub const MAX_PIXELS: u64 = 1 << 28;
/// The largest native transfer block taken: a DIB's rows hold at most 4 bytes
/// a pixel (24 bits a pixel padded to a 4-byte row, at width 1), and a header
/// and palette fit in the rest.
pub const MAX_DIB_BYTES: u64 = 4 * MAX_PIXELS + (1 << 16);

/// The pixel formats TWAIN's sample source delivers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Bw,
    Gray,
    Rgb,
}

impl Format {
    pub fn bits(self) -> u64 {
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
pub fn row_bytes(width: u32, format: Format) -> u64 {
    (u64::from(width) * format.bits()).div_ceil(8)
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
            Refusal::Tile { index, why } => write!(f, "buffer {index}: {why}"),
            Refusal::Overlap { index } => write!(f, "buffer {index} overlaps an earlier one"),
            Refusal::Uncovered { pixels } => write!(f, "{pixels} pixels no buffer covered"),
        }
    }
}

/// Bytes the image needs, the pixel cap checked first, so nothing after can
/// overflow.
fn checked_size(width: u32, height: u32, format: Format) -> Result<usize, Refusal> {
    if width == 0 || height == 0 {
        return Err(Refusal::Size {
            width: i64::from(width),
            height: i64::from(height),
        });
    }
    let pixels = u64::from(width) * u64::from(height);
    if pixels > MAX_PIXELS {
        return Err(Refusal::TooLarge { width, height });
    }
    // At most 3 bytes a pixel and a byte a row: under 2^30.
    let bytes = row_bytes(width, format) * u64::from(height);
    usize::try_from(bytes).map_err(|_| Refusal::TooLarge { width, height })
}

/// What a DIB said of itself, for the report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DibFacts {
    pub bits: u16,
    pub bottom_up: bool,
    /// Its palette, as RGB.
    pub palette: Vec<[u8; 3]>,
}

/// A packed DIB (a `BITMAPINFOHEADER` or a larger header, its palette, then its
/// rows, each padded to four bytes), as a native transfer's handle holds it on
/// Windows.
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
    if header_size < 40 || u64::from(header_size) > bytes.len() as u64 {
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
    let palette_at = u64::from(header_size);
    let rows_at = palette_at + u64::from(entries) * 4;
    let stride = (u64::from(w) * u64::from(bits)).div_ceil(32) * 4;
    let need = rows_at + stride * u64::from(h);
    if (bytes.len() as u64) < need {
        return Err(Refusal::Short {
            need,
            have: bytes.len(),
        });
    }
    // Every offset below is under `need`, so under the block's length: they
    // fit `usize`.
    let palette: Vec<[u8; 3]> = (0..entries as usize)
        .map(|i| {
            let at = palette_at as usize + i * 4;
            [bytes[at + 2], bytes[at + 1], bytes[at]]
        })
        .collect();
    let (rows_at, stride) = (rows_at as usize, stride as usize);
    let out_row = row_bytes(w, format) as usize;
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

/// Where a memory transfer's buffer goes: `rows` rows of `columns` pixels at
/// (`x`, `y`), `bytes_per_row` apart, as `TW_IMAGEMEMXFER` says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Shape {
    pub x: u32,
    pub y: u32,
    pub columns: u32,
    pub rows: u32,
    pub bytes_per_row: u32,
}

/// The bytes the source wrote into `buffer`: `BytesWritten` checked against
/// the buffer first.
pub fn written(written: u32, buffer: &[u8]) -> Result<&[u8], String> {
    usize::try_from(written)
        .ok()
        .and_then(|n| buffer.get(..n))
        .ok_or_else(|| {
            format!(
                "{written} bytes written into a {}-byte buffer",
                buffer.len()
            )
        })
}

/// A memory transfer's image, put together as its buffers come: each placed
/// inside the image, none overlapping, every pixel covered by the end. It
/// holds the image and which pixels are covered, nothing of the buffers.
pub struct Assembler {
    width: u32,
    height: u32,
    format: Format,
    stride: u64,
    data: Vec<u8>,
    covered: Coverage,
    placed: usize,
}

impl Assembler {
    /// An empty image of the size and format `DAT_IMAGEINFO` gave, after the
    /// caps.
    pub fn new(width: u32, height: u32, format: Format) -> Result<Assembler, Refusal> {
        let total = checked_size(width, height, format)?;
        Ok(Assembler {
            width,
            height,
            format,
            stride: row_bytes(width, format),
            data: vec![0; total],
            covered: Coverage::new(width, height),
            placed: 0,
        })
    }

    /// One buffer placed, from what the source wrote into it.
    pub fn place(&mut self, shape: Shape, bytes: &[u8]) -> Result<(), Refusal> {
        let index = self.placed;
        let bad = |why: String| Refusal::Tile { index, why };
        let Shape {
            x,
            y,
            columns,
            rows,
            bytes_per_row,
        } = shape;
        if columns == 0 || rows == 0 {
            return Err(bad("it's empty".to_owned()));
        }
        if u64::from(x) + u64::from(columns) > u64::from(self.width)
            || u64::from(y) + u64::from(rows) > u64::from(self.height)
        {
            return Err(bad(format!(
                "{columns} by {rows} at ({x}, {y}) runs past the {} by {} image",
                self.width, self.height
            )));
        }
        // Inside the image, so `rows` is at most 2^28: nothing below overflows
        // `u64`, and what reaches `usize` is under the buffer's length.
        let tile_row = row_bytes(columns, self.format);
        if u64::from(bytes_per_row) < tile_row {
            return Err(bad(format!(
                "{bytes_per_row} bytes a row, under the {tile_row} its columns need"
            )));
        }
        let need = u64::from(rows - 1) * u64::from(bytes_per_row) + tile_row;
        if (bytes.len() as u64) < need {
            return Err(bad(format!("{} bytes, and needs {need}", bytes.len())));
        }
        if self.format == Format::Bw
            && (x % 8 != 0 || (columns % 8 != 0 && x + columns != self.width))
        {
            return Err(bad(
                "it starts or ends within a byte of 1-bit rows".to_owned()
            ));
        }
        if !self.covered.mark(x, y, columns, rows) {
            return Err(Refusal::Overlap { index });
        }
        let x_byte = u64::from(x) * self.format.bits() / 8;
        let (tile_row, bpr) = (tile_row as usize, bytes_per_row as usize);
        for r in 0..rows as usize {
            let dst = ((u64::from(y) + r as u64) * self.stride + x_byte) as usize;
            let src = r * bpr;
            self.data[dst..dst + tile_row].copy_from_slice(&bytes[src..src + tile_row]);
        }
        self.placed += 1;
        Ok(())
    }

    /// The image, once every pixel is covered, in the comparable form.
    pub fn finish(self, polarity: Polarity) -> Result<Image, Refusal> {
        let missing = u64::from(self.width) * u64::from(self.height) - self.covered.count;
        if missing > 0 {
            return Err(Refusal::Uncovered { pixels: missing });
        }
        let mut data = self.data;
        if polarity == Polarity::ZeroWhite && self.format != Format::Rgb {
            data.iter_mut().for_each(|b| *b = !*b);
        }
        let mut image = Image {
            width: self.width,
            height: self.height,
            format: self.format,
            data,
        };
        clear_padding_bits(&mut image);
        Ok(image)
    }
}

/// The first difference between two images, if any. Images whose data isn't
/// the size their dimensions say are refused, not compared.
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
    let stride = row_bytes(a.width, a.format);
    let expected = stride * u64::from(a.height);
    if expected == 0 || a.data.len() as u64 != expected || b.data.len() as u64 != expected {
        return Err(format!(
            "{} and {} bytes, where {} by {} {} is {expected}",
            a.data.len(),
            b.data.len(),
            a.width,
            a.height,
            a.format.name()
        ));
    }
    let Some(at) = a.data.iter().zip(&b.data).position(|(x, y)| x != y) else {
        return Ok(());
    };
    let differing = a.data.iter().zip(&b.data).filter(|(x, y)| x != y).count();
    let stride = stride as usize;
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
    let stride = row_bytes(image.width, image.format) as usize;
    for row in image.data.chunks_exact_mut(stride) {
        if let Some(last) = row.last_mut() {
            *last &= keep;
        }
    }
}

/// Which pixels the buffers so far have covered.
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

    /// A buffer of a memory transfer, as the tests make them.
    #[derive(Debug, Clone)]
    struct Tile {
        x: u32,
        y: u32,
        columns: u32,
        rows: u32,
        bytes_per_row: u32,
        data: Vec<u8>,
    }

    /// Tiles placed in order through an [`Assembler`].
    fn assemble(
        width: u32,
        height: u32,
        format: Format,
        polarity: Polarity,
        tiles: &[Tile],
    ) -> Result<Image, Refusal> {
        let mut a = Assembler::new(width, height, format)?;
        for t in tiles {
            let shape = Shape {
                x: t.x,
                y: t.y,
                columns: t.columns,
                rows: t.rows,
                bytes_per_row: t.bytes_per_row,
            };
            a.place(shape, &t.data)?;
        }
        a.finish(polarity)
    }

    /// Tiles of `columns` by `rows` cut from an image held in the comparable
    /// form, each row padded by `pad` bytes, in the order `order` gives.
    fn grid(
        width: u32,
        height: u32,
        (columns, rows): (u32, u32),
        format: Format,
        image: &[u8],
        pad: u32,
    ) -> Vec<Tile> {
        let stride = row_bytes(width, format) as usize;
        let mut tiles = Vec::new();
        for y in (0..height).step_by(rows as usize) {
            for x in (0..width).step_by(columns as usize) {
                let (c, r) = (columns.min(width - x), rows.min(height - y));
                let tile_row = row_bytes(c, format) as usize;
                let x_byte = (u64::from(x) * format.bits() / 8) as usize;
                let mut data = Vec::new();
                for row in 0..r as usize {
                    let at = (y as usize + row) * stride + x_byte;
                    data.extend_from_slice(&image[at..at + tile_row]);
                    data.extend(std::iter::repeat_n(0xcc, pad as usize));
                }
                tiles.push(Tile {
                    x,
                    y,
                    columns: c,
                    rows: r,
                    bytes_per_row: tile_row as u32 + pad,
                    data,
                });
            }
        }
        tiles
    }

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
        let row = row_bytes(width, format) as usize;
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
        assert!(written(9, &[0; 8]).is_err());
        assert!(written(u32::MAX, &[0; 8]).is_err());
        assert_eq!(written(4, &[1, 2, 3, 4, 5]), Ok(&[1u8, 2, 3, 4][..]));
    }

    #[test]
    fn rgb_and_1_bit_tiles_off_the_left_edge_assemble_out_of_order() {
        // RGB, 5 by 5 in 2 by 2 tiles (the last column and row narrower).
        let rgb: Vec<u8> = (0..5 * 5 * 3).map(|i| (i * 7) as u8).collect();
        let mut tiles = grid(5, 5, (2, 2), Format::Rgb, &rgb, 3);
        tiles.reverse();
        assert_eq!(
            assemble(5, 5, Format::Rgb, Polarity::ZeroBlack, &tiles)
                .unwrap()
                .data,
            rgb
        );
        // 1-bit, 21 pixels wide (3 bytes a row, the last 5 bits used), in
        // tiles 8 pixels wide; padding bits zero in the source image.
        let mut bw: Vec<u8> = (0..3 * 4).map(|i| (i * 37 + 11) as u8).collect();
        for row in bw.as_chunks_mut::<3>().0 {
            row[2] &= 0b1111_1000;
        }
        let mut tiles = grid(21, 4, (8, 3), Format::Bw, &bw, 1);
        tiles.rotate_left(2);
        assert_eq!(
            assemble(21, 4, Format::Bw, Polarity::ZeroBlack, &tiles)
                .unwrap()
                .data,
            bw
        );
    }

    #[test]
    fn partial_overlaps_and_tiles_past_the_right_edge_are_refused() {
        let image = vec![0u8; 4 * 4];
        let ok = grid(4, 4, (2, 2), Format::Gray, &image, 0);
        let mut down_one = ok[0].clone();
        down_one.y = 1;
        assert!(matches!(
            assemble(
                4,
                4,
                Format::Gray,
                Polarity::ZeroBlack,
                &[ok[0].clone(), down_one]
            ),
            Err(Refusal::Overlap { index: 1 })
        ));
        let mut right_one = ok[0].clone();
        right_one.x = 1;
        assert!(matches!(
            assemble(
                4,
                4,
                Format::Gray,
                Polarity::ZeroBlack,
                &[ok[0].clone(), right_one]
            ),
            Err(Refusal::Overlap { index: 1 })
        ));
        let mut past = ok[1].clone();
        past.x = 3;
        assert!(matches!(
            assemble(4, 4, Format::Gray, Polarity::ZeroBlack, &[past]),
            Err(Refusal::Tile { index: 0, .. })
        ));
    }

    #[test]
    fn vanilla_1_bit_of_odd_width_clears_the_inverted_padding() {
        let bw = vec![0b1010_1010, 0b1100_0000];
        let tiles = grid(10, 1, (10, 1), Format::Bw, &bw, 0);
        let img = assemble(10, 1, Format::Bw, Polarity::ZeroWhite, &tiles).unwrap();
        assert_eq!(img.data, [0b0101_0101, 0b0000_0000]);
    }

    #[test]
    fn a_larger_header_and_a_24_bit_color_table_are_skipped() {
        // A BITMAPV5HEADER (124 bytes): the palette starts after it.
        let mut v5 = dib(2, 1, 8, 0, &gray_ramp(), &[vec![10, 20]]);
        v5[0] = 124;
        v5.splice(40..40, std::iter::repeat_n(0u8, 84));
        assert_eq!(decode_dib(&v5).unwrap().0.data, [10, 20]);
        // A 24-bit DIB with an optional 3-entry color table before its rows.
        let table = dib(1, 1, 24, 3, &[[9, 9, 9]; 3], &[vec![1, 2, 3]]);
        assert_eq!(decode_dib(&table).unwrap().0.data, [3, 2, 1]);
    }

    #[test]
    fn sizes_a_32_bit_target_would_overflow_are_worked_out_or_refused() {
        // RGB rows past 2^32 bits, under the pixel cap.
        assert_eq!(row_bytes(178_956_971, Format::Rgb), 536_870_913);
        assert_eq!(checked_size(178_956_971, 1, Format::Rgb), Ok(536_870_913));
        assert!(matches!(
            checked_size(u32::MAX, u32::MAX, Format::Rgb),
            Err(Refusal::TooLarge { .. })
        ));
        // A buffer whose rows the source spaced 4 GiB apart.
        let huge = Tile {
            x: 0,
            y: 0,
            columns: 4,
            rows: 2,
            bytes_per_row: u32::MAX,
            data: vec![0; 8],
        };
        assert!(matches!(
            assemble(4, 4, Format::Gray, Polarity::ZeroBlack, &[huge]),
            Err(Refusal::Tile { index: 0, .. })
        ));
        let spaced = Tile {
            x: 0,
            y: 0,
            columns: 4,
            rows: 3,
            bytes_per_row: 0x8000_0000,
            data: vec![0; 64],
        };
        assert!(matches!(
            assemble(4, 4, Format::Gray, Polarity::ZeroBlack, &[spaced]),
            Err(Refusal::Tile { index: 0, .. })
        ));
    }

    #[test]
    fn compare_refuses_data_its_dimensions_dont_account_for() {
        let a = Image {
            width: 4,
            height: 1,
            format: Format::Gray,
            data: vec![1, 2, 3, 4],
        };
        let short = Image {
            data: vec![1, 2],
            ..a.clone()
        };
        assert!(compare(&a, &short).is_err());
        assert!(compare(&short, &a).is_err());
        let empty = Image {
            width: 0,
            height: 1,
            format: Format::Gray,
            data: vec![1],
        };
        assert!(
            compare(
                &empty,
                &Image {
                    data: vec![2],
                    ..empty.clone()
                }
            )
            .is_err()
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
