//! Pages, a batch's events, and how a batch ends.

use std::fmt;

use crate::{Applied, RawImage};

/// A scanned page: its pixels, its resolution, and the sheet and side it's from.
///
/// `dpi` finite and above 0: a backend keeps it so, and code that decodes a page from outside the
/// process (from a pipe, say) refuses others.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct RawPage {
    /// Its pixels, raw or encoded.
    pub pixels: Pixels,
    /// Its resolution across and down, in dots per inch: finite and above 0. A fraction where the
    /// driver gives one.
    pub dpi: (f32, f32),
    /// The sheet it's from, as [`Event::Sheet`] counted it.
    pub sheet: u32,
    /// The side of the sheet it's from.
    pub side: Side,
    /// Whether the driver said the side, or the backend worked it out from the order.
    pub side_known: SideKnown,
}

impl RawPage {
    /// The page of `pixels` at `dpi`, from `side` of `sheet`.
    pub fn new(
        pixels: Pixels,
        dpi: (f32, f32),
        sheet: u32,
        side: Side,
        side_known: SideKnown,
    ) -> RawPage {
        RawPage {
            pixels,
            dpi,
            sheet,
            side,
            side_known,
        }
    }
}

/// A page's pixels: raw in the crate's layout, or encoded as the app asked.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Pixels {
    /// In the raw layout, checked.
    Raw(RawImage),
    /// Encoded, in a form [`Settings::encodings`] asked for.
    ///
    /// [`Settings::encodings`]: crate::Settings::encodings
    Encoded(Encoded),
}

/// A page's bytes in an encoded form, as the driver gave them.
///
/// `Debug` by hand, as `RawImage`'s: the kind and the length.
#[derive(Clone, PartialEq, Eq)]
pub struct Encoded {
    /// The form they're in.
    pub kind: EncodedKind,
    /// The bytes.
    pub bytes: Vec<u8>,
}

impl fmt::Debug for Encoded {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Never the bytes: a page can be tens of megabytes.
        f.debug_struct("Encoded")
            .field("kind", &self.kind)
            .field("len", &self.bytes.len())
            .finish()
    }
}

/// The form an [`Encoded`] page's bytes are in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum EncodedKind {
    /// A JPEG file.
    Jpeg,
    /// A PNG file.
    Png,
    /// A TIFF file.
    Tiff,
    /// A BMP file.
    Bmp,
    /// CCITT Group 4 data, with nothing around it to say its size or polarity, so they're said
    /// here.
    CcittG4 {
        /// Its width, in pixels.
        width: u32,
        /// Its height, in pixels.
        height: u32,
        /// Whether a 1 bit is black.
        black_is_1: bool,
    },
}

/// An encoded form, as the app asks for it and a source offers it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum Encoding {
    /// JPEG, for 8-bit gray or color.
    Jpeg,
    /// PNG.
    Png,
    /// TIFF.
    Tiff,
    /// BMP.
    Bmp,
    /// CCITT Group 4, for black and white.
    CcittG4,
}

/// A side of a sheet.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Side {
    /// The front, the side scanned first.
    Front,
    /// The back, scanned in duplex.
    Back,
}

/// How a page's side is known.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum SideKnown {
    /// The driver said which side the page is.
    Reported,
    /// The backend worked it out from the pages' order.
    Inferred,
}

/// What a batch sends as it goes, in the order the crate's docs give ("A batch").
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum Event {
    /// The settings set, before the first sheet: what the device took, and what it changed.
    Applied(Applied),
    /// Sheet `n`, counted on from the batch's `first_sheet`, before its pages: as it begins, or,
    /// where the driver says a sheet is there only by scanning it, with its front once that's
    /// read.
    Sheet(u32),
    /// A page, whole, once it's complete.
    Page(RawPage),
    /// Something the user should know meanwhile.
    Status(Status),
}

/// What the user should know while a batch goes on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum Status {
    /// The scanner is warming up: the batch waits for it.
    WarmingUp,
}

/// How a batch ended, when it ended well.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum Done {
    /// The feeder emptied, after at least one page.
    FeederEmpty,
    /// A flatbed's page was scanned: a flatbed gives one a batch.
    OnePage,
    /// Cancelled. Through the batch's progress: the sheet in flight finished, front and back, and
    /// no sheet fed after it. With the receiver gone: the batch ended at the `send` that failed,
    /// so the sheet in flight may have been fed and not delivered, and no sheet was fed after it.
    Cancelled,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_of_encoded_gives_the_kind_and_length_never_a_byte() {
        let encoded = Encoded {
            kind: EncodedKind::Jpeg,
            bytes: vec![123; 4],
        };
        let debug = format!("{encoded:?}");
        assert_eq!(debug, "Encoded { kind: Jpeg, len: 4 }");
        assert!(!debug.contains("123"), "{debug}");
    }
}
