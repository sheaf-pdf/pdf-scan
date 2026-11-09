//! What a device can do, per source, and what a scan asks of it.

use std::num::NonZeroU32;

use crate::{Encoding, Format};

/// What a device can do: each of its sources, and whether its driver has a settings window of
/// its own.
///
/// A backend keeps the rules its fields' docs give (at least one source, each with at least one
/// format, and its resolutions as [`Resolutions`] says). The constructors don't check them, the
/// fields being public, so code that decodes caps from outside the process (from a pipe, say)
/// checks them as it decodes and refuses caps that break them.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct Caps {
    /// The device's sources, each with what it can do: never empty.
    pub sources: Vec<SourceCaps>,
    /// Whether the driver has a settings window of its own ([`Device::settings_window`]).
    ///
    /// [`Device::settings_window`]: crate::Device::settings_window
    pub settings_window: bool,
}

impl Caps {
    /// Caps of these sources, with no settings window.
    pub fn new(sources: Vec<SourceCaps>) -> Caps {
        Caps {
            sources,
            settings_window: false,
        }
    }
}

/// What one source of a device can do.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct SourceCaps {
    /// The source.
    pub source: Source,
    /// Whether it scans both sides of a sheet.
    pub duplex: bool,
    /// The resolutions it scans at.
    pub resolutions: Resolutions,
    /// The raw formats it gives, never empty.
    pub formats: Vec<Format>,
    /// The encoded forms it can give a page in, beside the raw ones; empty for raw only.
    pub encodings: Vec<Encoding>,
    /// The largest paper it takes, where the driver says.
    pub max_area: Option<SizeMm>,
    /// Whether the driver can find the paper's size itself ([`Paper::Auto`]).
    pub paper_auto: bool,
}

impl SourceCaps {
    /// A source scanning at `resolutions` in `formats`: one side, raw only, no largest area said,
    /// and no size detection.
    pub fn new(source: Source, resolutions: Resolutions, formats: Vec<Format>) -> SourceCaps {
        SourceCaps {
            source,
            duplex: false,
            resolutions,
            formats,
            encodings: Vec::new(),
            max_area: None,
            paper_auto: false,
        }
    }
}

/// Where the paper is scanned from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Source {
    /// The glass: one page a batch.
    Flatbed,
    /// The document feeder: sheet after sheet until it's empty.
    Feeder,
}

/// Whole dpi: a list never empty, a range's `min` never past its `max`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Resolutions {
    /// These resolutions alone.
    List(Vec<u32>),
    /// Every resolution from `min` to `max`, in steps of `step` from `min`.
    Range {
        /// The lowest.
        min: u32,
        /// The highest.
        max: u32,
        /// The step between one and the next, from `min`.
        step: NonZeroU32,
    },
}

/// A paper size in millimetres, width by height.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SizeMm {
    /// Across, in millimetres.
    pub width: f32,
    /// Down, in millimetres.
    pub height: f32,
}

impl SizeMm {
    /// US Letter, 8.5 by 11 inches.
    pub const LETTER: SizeMm = SizeMm {
        width: 215.9,
        height: 279.4,
    };
    /// US Legal, 8.5 by 14 inches.
    pub const LEGAL: SizeMm = SizeMm {
        width: 215.9,
        height: 355.6,
    };
    /// A4, 210 by 297 millimetres.
    pub const A4: SizeMm = SizeMm {
        width: 210.0,
        height: 297.0,
    };
}

/// What a scan asks for. The device takes what it can and says what it changed
/// ([`Applied`]).
///
/// `encodings`: the encoded forms the app takes, in its order; empty (`Settings::new`'s) for raw
/// only.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct Settings {
    /// The source to scan from.
    pub source: Source,
    /// Both sides of each sheet, where the source can.
    pub duplex: bool,
    /// The resolution, in dots per inch, the same across and down.
    pub dpi: u32,
    /// The raw format of each page.
    pub format: Format,
    /// The paper's size, or the driver's own detection of it.
    pub paper: Paper,
    /// The encoded forms the app takes a page in, in its order of preference; empty for raw only.
    /// A page comes encoded only in one of these.
    pub encodings: Vec<Encoding>,
}

impl Settings {
    /// Settings scanning from `source` at `dpi` in `format`: one side, the paper's size found by
    /// the driver ([`Paper::Auto`]), raw only.
    pub fn new(source: Source, dpi: u32, format: Format) -> Settings {
        Settings {
            source,
            duplex: false,
            dpi,
            format,
            paper: Paper::Auto,
            encodings: Vec::new(),
        }
    }
}

/// The paper's size, as a scan asks for it.
#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
pub enum Paper {
    /// Found by the driver, where the source can ([`SourceCaps::paper_auto`]).
    Auto,
    /// This size.
    Size(SizeMm),
}

/// The settings a device took for a batch, and what it changed of those asked for.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct Applied {
    /// The settings the batch scans with.
    pub settings: Settings,
    /// Each setting changed from what was asked, with what was asked; empty where nothing was.
    pub clamped: Vec<Clamped>,
}

impl Applied {
    /// The settings taken, and what was changed.
    pub fn new(settings: Settings, clamped: Vec<Clamped>) -> Applied {
        Applied { settings, clamped }
    }
}

/// What was asked of a setting the device changed; what it took is in `Applied::settings`.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum Clamped {
    /// The source asked for.
    Source(Source),
    /// Whether duplex was asked for.
    Duplex(bool),
    /// The resolution asked for.
    Dpi(u32),
    /// The format asked for.
    Format(Format),
    /// The paper asked for.
    Paper(Paper),
    /// The encoded forms asked for.
    Encodings(Vec<Encoding>),
}

#[cfg(test)]
mod tests {
    use super::SizeMm;

    /// Pixels across `mm` at 300 dpi, rounded.
    fn at_300_dpi(mm: f32) -> f64 {
        (f64::from(mm) / 25.4 * 300.0).round()
    }

    #[test]
    fn letter_and_legal_are_whole_inches_not_whole_millimetres() {
        for size in [SizeMm::LETTER, SizeMm::LEGAL] {
            assert_ne!(size.width.fract(), 0.0);
            assert_ne!(size.height.fract(), 0.0);
        }
        // 8.5 by 11 and 8.5 by 14 inches.
        let letter = SizeMm::LETTER;
        let legal = SizeMm::LEGAL;
        assert_eq!(
            (at_300_dpi(letter.width), at_300_dpi(letter.height)),
            (2550.0, 3300.0)
        );
        assert_eq!(
            (at_300_dpi(legal.width), at_300_dpi(legal.height)),
            (2550.0, 4200.0)
        );
        // A4 is whole millimetres.
        assert_eq!((SizeMm::A4.width, SizeMm::A4.height), (210.0, 297.0));
        assert_eq!(
            (at_300_dpi(SizeMm::A4.width), at_300_dpi(SizeMm::A4.height)),
            (2480.0, 3508.0)
        );
    }
}
