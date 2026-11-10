//! The tables: which of a device's options, and which of their values, stand for what the model
//! names; and the options an app sets by name.
//!
//! SANE's standard names few options and fewer values, so backends name a feeder, duplex and a
//! scan mode each their own way. Each row below says where it's seen: a backend's man page, or a
//! frontend that reads it (each studied, none of its code taken):
//!
//! - NAPS2's `SaneOptionMatchers.cs` and `SaneScanDriver.cs`,
//!   <https://github.com/cyanfish/naps2/tree/master/NAPS2.Sdk/Scan/Internal/Sane>;
//! - simple-scan's `scanner.vala`, <https://github.com/GNOME/simple-scan/blob/master/src/scanner.vala>;
//! - libinsane's `workarounds.h` and `normalizers.h`,
//!   <https://gitlab.gnome.org/World/OpenPaperwork/libinsane/-/tree/master/subprojects/libinsane/include/libinsane>;
//! - the backends' man pages, <https://gitlab.com/sane-project/backends/-/tree/master/doc>
//!   (sane-test(5), sane-pnm(5), sane-fujitsu(5)), and SANE's standard, whose well-known options
//!   are `source`, `mode`, `depth`, `resolution` and the scan area's `tl-x`, `tl-y`, `br-x` and
//!   `br-y`, <https://gitlab.com/sane-project/standard>.
//!
//! Values are matched whole and exactly, never by a part of them, so `ADF Back` is never taken
//! for a feeder. A value no row names (one translated into the user's language among them) is
//! left out: a source or a mode the model doesn't offer.

use sane_dl::{
    Constraint, Fixed, FrameFormat, Handle, OptionDescriptor, Parameters, SaneString, Unit, Value,
    ValueType,
};

use super::error;
use crate::{Format, Resolutions, ScanError, SizeMm};

/// The option that chooses the source: the standard's `source`, else `doc-source`, Samsung's
/// (libinsane's `workarounds.h`; simple-scan).
pub(super) const SOURCE: [&str; 2] = ["source", "doc-source"];

/// The option that chooses the scan mode (the standard's).
pub(super) const MODE: &str = "mode";

/// The option that chooses the bits a sample (the standard's).
pub(super) const DEPTH: &str = "depth";

/// The option that chooses the resolution: the standard's `resolution`, else `scan-resolution`,
/// Lexmark's (libinsane's `workarounds.h`; simple-scan), else `x-resolution`, with
/// `y-resolution` set alike (NAPS2's `SaneScanDriver.cs`).
pub(super) const RESOLUTION: [&str; 3] = ["resolution", "scan-resolution", X_RESOLUTION];

/// The resolution across the page, of a device with none for both ways.
pub(super) const X_RESOLUTION: &str = "x-resolution";

/// The resolution down the page, set alike where the resolution option is `x-resolution`, and
/// what lengths down the page in pixels are converted at.
pub(super) const Y_RESOLUTION: &str = "y-resolution";

/// The scan area's corners (the standard's).
pub(super) const TL_X: &str = "tl-x";
/// The scan area's top.
pub(super) const TL_Y: &str = "tl-y";
/// The scan area's right edge.
pub(super) const BR_X: &str = "br-x";
/// The scan area's bottom.
pub(super) const BR_Y: &str = "br-y";
/// The paper's width in a feeder, which some backends centre the area on: fujitsu's
/// (sane-fujitsu(5); libinsane's `normalizers.h`; simple-scan sets it).
pub(super) const PAGE_WIDTH: &str = "page-width";
/// The paper's length in a feeder.
pub(super) const PAGE_HEIGHT: &str = "page-height";

/// A flatbed: `Flatbed` (the `test` backend, sane-test(5); fujitsu; sane-airscan), `FlatBed`
/// (Brother), `Normal`, and Epson's `Document Table` (simple-scan; libinsane's `normalizers.h`).
/// Left out: `Auto`, which libinsane's `workarounds.h` hides, a flatbed that never stops.
const FLATBED: [&[u8]; 4] = [b"Flatbed", b"FlatBed", b"Normal", b"Document Table"];

/// A feeder, one side: `ADF` (sane-airscan; simple-scan), fujitsu's `ADF Front` (sane-fujitsu(5);
/// NAPS2; simple-scan), Samsung's `ADF Simplex`, `Automatic Document Feeder` (the `test`
/// backend, sane-test(5); simple-scan; NAPS2), and Brother's aligned feeders (simple-scan).
/// Left out: fujitsu's `ADF Back`, which scans the backs alone.
const FEEDER: [&[u8]; 7] = [
    b"ADF",
    b"ADF Front",
    b"ADF Simplex",
    b"Automatic Document Feeder",
    b"Automatic Document Feeder(left aligned)",
    b"Automatic Document Feeder(centrally aligned)",
    b"Automatic Document Feeder(center aligned)",
];

/// A feeder, both sides: `ADF Duplex` (fujitsu; sane-airscan; simple-scan; NAPS2), Brother's
/// `Duplex ADF`, HP's `Duplex` and Samsung's long-edge binding (simple-scan); and Brother's
/// aligned feeders' (below).
const FEEDER_DUPLEX: [&[u8]; 4] = [
    b"ADF Duplex",
    b"Duplex ADF",
    b"Duplex",
    b"ADF Duplex - Long-Edge Binding",
];

/// Brother's aligned feeders, one side, end `)`; both sides, `,Duplex)` (simple-scan).
const BROTHER_FEEDER: &[u8] = b"Automatic Document Feeder(";
/// How Brother's both-sides feeders end.
const BROTHER_DUPLEX_END: &[u8] = b",Duplex)";

/// What a source value is, as the model reads it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum SourceKind {
    /// The glass.
    Flatbed,
    /// The feeder, one side.
    Feeder,
    /// The feeder, both sides.
    FeederDuplex,
}

/// What a source value stands for, or `None` where no row names it.
pub(super) fn source_kind(value: &[u8]) -> Option<SourceKind> {
    if FLATBED.contains(&value) {
        Some(SourceKind::Flatbed)
    } else if FEEDER.contains(&value) {
        Some(SourceKind::Feeder)
    } else if FEEDER_DUPLEX.contains(&value)
        || (value.starts_with(BROTHER_FEEDER) && value.ends_with(BROTHER_DUPLEX_END))
    {
        Some(SourceKind::FeederDuplex)
    } else {
        None
    }
}

/// The values a source option offers for what the model names.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct SourceValues {
    /// The flatbed's: the first listed.
    pub flatbed: Option<SaneString>,
    /// The feeder's, one side: the first listed.
    pub feeder: Option<SaneString>,
    /// The feeder's, both sides: Brother's that matches the feeder's one side where there is one,
    /// else the first listed.
    pub duplex: Option<SaneString>,
}

/// The values of `values`, as [`SourceValues`] picks them.
pub(super) fn source_values(values: &[SaneString]) -> SourceValues {
    let first = |kind: SourceKind| {
        values
            .iter()
            .find(|value| source_kind(value.as_bytes()) == Some(kind))
            .cloned()
    };
    let feeder = first(SourceKind::Feeder);
    // Brother's `...(left aligned)` and `...(left aligned,Duplex)`.
    let matching = feeder.as_ref().and_then(|one_side| {
        let stem = one_side.as_bytes().strip_suffix(b")")?;
        let both: Vec<u8> = [stem, BROTHER_DUPLEX_END].concat();
        values
            .iter()
            .find(|value| value.as_bytes() == both)
            .cloned()
    });
    SourceValues {
        flatbed: first(SourceKind::Flatbed),
        duplex: matching.or_else(|| first(SourceKind::FeederDuplex)),
        feeder,
    }
}

/// How an option apart from `source` turns a feeder's duplex on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum DuplexSwitch {
    /// A string option, one value for one side and another for both.
    Values {
        /// One side.
        simplex: &'static [u8],
        /// Both sides.
        duplex: &'static [u8],
    },
    /// A boolean option, true for both sides.
    Bool,
}

/// The options that turn a feeder's duplex on apart from `source`, in the order they're looked
/// for: Canon DR-C240's `ScanMode` (simple-scan), Epson's `adf-mode` (simple-scan; NAPS2) and
/// `adf_mode` (NAPS2), Lexmark's `scan-both-sides` and a `duplex`, as booleans (simple-scan;
/// NAPS2), and Kodak's `kds_s2000`'s `duplex` of `front` or `both` (NAPS2's
/// `SaneScanDriver.cs`).
const DUPLEX_SWITCHES: [(&str, ValueType, DuplexSwitch); 6] = {
    const SIMPLEX_DUPLEX: DuplexSwitch = DuplexSwitch::Values {
        simplex: b"Simplex",
        duplex: b"Duplex",
    };
    [
        ("ScanMode", ValueType::String, SIMPLEX_DUPLEX),
        ("adf-mode", ValueType::String, SIMPLEX_DUPLEX),
        ("adf_mode", ValueType::String, SIMPLEX_DUPLEX),
        ("scan-both-sides", ValueType::Bool, DuplexSwitch::Bool),
        ("duplex", ValueType::Bool, DuplexSwitch::Bool),
        (
            "duplex",
            ValueType::String,
            DuplexSwitch::Values {
                simplex: b"front",
                duplex: b"both",
            },
        ),
    ]
};

/// The first option of `options` that turns a feeder's duplex on apart from `source`, and how:
/// one of those above, of its type, active and settable, a string one offering both its values.
pub(super) fn duplex_switch(
    options: &[OptionDescriptor],
) -> Option<(&OptionDescriptor, DuplexSwitch)> {
    DUPLEX_SWITCHES
        .iter()
        .find_map(|&(name, value_type, switch)| {
            let option = options.iter().find(|option| {
                option.name == name && option.value_type == value_type && settable(option)
            })?;
            let offered = match switch {
                DuplexSwitch::Bool => true,
                DuplexSwitch::Values { simplex, duplex } => {
                    let values = strings(option);
                    [simplex, duplex]
                        .iter()
                        .all(|wanted| values.iter().any(|value| value.as_bytes() == *wanted))
                }
            };
            offered.then_some((option, switch))
        })
}

/// What a scan mode gives.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ModeKind {
    /// Black and white, a bit a pixel.
    Lineart,
    /// Gray.
    Gray,
    /// Color.
    Color,
}

/// Black and white: `Lineart` (the standard's value; fujitsu, sane-fujitsu(5)), Epson's `Binary`,
/// Brother's `Black & White` and Samsung's `Black and White - Line Art` (simple-scan;
/// libinsane's `workarounds.h`). Left out: `Halftone` and Brother's `Gray[Error Diffusion]`,
/// dithered, which aren't line art.
const LINEART: [&[u8]; 4] = [
    b"Lineart",
    b"Binary",
    b"Black & White",
    b"Black and White - Line Art",
];

/// Gray: `Gray` (the standard's; the `test` backend), `Grayscale`, and Brother's `True Gray`
/// (simple-scan; libinsane's `workarounds.h`).
const GRAY: [&[u8]; 3] = [b"Gray", b"Grayscale", b"True Gray"];

/// Color: `Color` (the standard's; the `test` backend), and Brother's `24bit Color`
/// (simple-scan; libinsane's `workarounds.h`).
const COLOR: [&[u8]; 2] = [b"Color", b"24bit Color"];

/// What a scan mode gives, or `None` where no row names it.
pub(super) fn mode_kind(value: &[u8]) -> Option<ModeKind> {
    if LINEART.contains(&value) {
        Some(ModeKind::Lineart)
    } else if GRAY.contains(&value) {
        Some(ModeKind::Gray)
    } else if COLOR.contains(&value) {
        Some(ModeKind::Color)
    } else {
        None
    }
}

/// The values a mode option offers for each kind: the first listed of each.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct ModeValues {
    /// Black and white's.
    pub lineart: Option<SaneString>,
    /// Gray's.
    pub gray: Option<SaneString>,
    /// Color's.
    pub color: Option<SaneString>,
}

/// The values of `values`, as [`ModeValues`] picks them.
pub(super) fn mode_values(values: &[SaneString]) -> ModeValues {
    let first = |kind: ModeKind| {
        values
            .iter()
            .find(|value| mode_kind(value.as_bytes()) == Some(kind))
            .cloned()
    };
    ModeValues {
        lineart: first(ModeKind::Lineart),
        gray: first(ModeKind::Gray),
        color: first(ModeKind::Color),
    }
}

/// The depths of 1, 8 and 16 bits a depth option's constraint allows.
pub(super) fn depths(depth: &OptionDescriptor) -> Vec<u32> {
    [1_u32, 8, 16]
        .into_iter()
        .filter(|&bits| allows(depth, f64::from(bits)))
        .collect()
}

/// Whether an option's constraint allows `value`: within a range, on its step from its least, or
/// in a word list. An option with no such constraint allows nothing here.
pub(super) fn allows(option: &OptionDescriptor, value: f64) -> bool {
    match &option.constraint {
        Constraint::Range { min, max, quant } => {
            let (Some(min), Some(max), Some(quant)) = (
                number(option, *min),
                number(option, *max),
                number(option, *quant),
            ) else {
                return false;
            };
            min <= value && value <= max && (quant <= 0.0 || ((value - min) % quant) == 0.0)
        }
        Constraint::WordList(words) => words
            .iter()
            .any(|&word| number(option, word) == Some(value)),
        _ => false,
    }
}

/// The raw formats a device's modes and depths give, in the model's order: black and white
/// from a black-and-white mode, or from gray at a depth of 1 where there's none; gray and color
/// at 8 bits and 16 where the depth option allows them, at 8 where there's no depth option.
pub(super) fn formats(modes: &ModeValues, depths: Option<&[u32]>) -> Vec<Format> {
    let at = |bits: u32| depths.map_or(bits == 8, |depths| depths.contains(&bits));
    let gray = modes.gray.is_some();
    let color = modes.color.is_some();
    let mut formats = Vec::new();
    if modes.lineart.is_some() || (gray && depths.is_some_and(|depths| depths.contains(&1))) {
        formats.push(Format::Bw1);
    }
    if gray && at(8) {
        formats.push(Format::Gray8);
    }
    if gray && at(16) {
        formats.push(Format::Gray16);
    }
    if color && at(8) {
        formats.push(Format::Rgb8);
    }
    if color && at(16) {
        formats.push(Format::Rgb16);
    }
    formats
}

/// The format a device with no mode option gives, from its parameters before a start (the
/// backend's estimate of its next frame): gray at 1, 8 or 16 bits, color (or one color's frame,
/// a three-pass scan's) at 8 or 16, and color at 1 bit as 8, as its pages come.
pub(super) fn format_of(parameters: &Parameters) -> Option<Format> {
    let color = [
        FrameFormat::RGB,
        FrameFormat::RED,
        FrameFormat::GREEN,
        FrameFormat::BLUE,
    ];
    match parameters.depth {
        1 if parameters.format == FrameFormat::GRAY => Some(Format::Bw1),
        8 if parameters.format == FrameFormat::GRAY => Some(Format::Gray8),
        16 if parameters.format == FrameFormat::GRAY => Some(Format::Gray16),
        1 | 8 if color.contains(&parameters.format) => Some(Format::Rgb8),
        16 if color.contains(&parameters.format) => Some(Format::Rgb16),
        _ => None,
    }
}

/// The resolutions a resolution option allows, in whole dpi above 0, or `None` where none is:
///
/// - a range, `INT` or `FIXED`: its ends rounded inward to whole dpi, its step whole dpi (a step
///   of 0, which is none, or one under 1 is 1, and a larger fractional one rounded up), its
///   least raised to the first of its steps at 1 or above; none where its least is then past its
///   greatest;
/// - a word list: each value to the nearest whole dpi, those below 1 and the duplicates dropped,
///   in order from the least;
/// - no constraint: `current`, its value now, as a list of one.
///
/// Every number is converted in checked arithmetic, so a value past what `u32` holds is dropped.
pub(super) fn resolutions(option: &OptionDescriptor, current: Option<f64>) -> Option<Resolutions> {
    match &option.constraint {
        Constraint::Range { min, max, quant } => {
            let min = number(option, *min)?.ceil();
            let max = number(option, *max)?.floor();
            let quant = number(option, *quant)?;
            let step = if quant < 1.0 { 1.0 } else { quant.ceil() };
            let min = if min < 1.0 {
                min + ((1.0 - min) / step).ceil() * step
            } else {
                min
            };
            if min > max {
                return None;
            }
            let step = std::num::NonZeroU32::new(whole_dpi(step)?)?;
            Some(Resolutions::Range {
                min: whole_dpi(min)?,
                max: whole_dpi(max)?,
                step,
            })
        }
        Constraint::WordList(words) => {
            let mut list: Vec<u32> = words
                .iter()
                .filter_map(|&word| whole_dpi(number(option, word)?.round()))
                .collect();
            list.sort_unstable();
            list.dedup();
            (!list.is_empty()).then_some(Resolutions::List(list))
        }
        Constraint::None => Some(Resolutions::List(vec![whole_dpi(current?.round())?])),
        Constraint::StringList(_) => None,
    }
}

/// A whole number of dpi, at least 1 and within `u32`.
fn whole_dpi(value: f64) -> Option<u32> {
    (value >= 1.0 && value <= f64::from(u32::MAX) && value.fract() == 0.0).then_some(value as u32)
}

/// Which end of an area option's constraint.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum End {
    /// Its least.
    Min,
    /// Its greatest.
    Max,
}

/// The length at one end of an area option's constraint, in millimetres: a range's end, or a word
/// list's least or greatest; in millimetres as it is, in pixels converted at `dpi`, the resolution
/// now. `None` for another unit, no such constraint, or pixels with no resolution.
pub(super) fn length_mm(option: &OptionDescriptor, end: End, dpi: Option<f64>) -> Option<f64> {
    let value = match &option.constraint {
        Constraint::Range { min, max, .. } => number(
            option,
            match end {
                End::Min => *min,
                End::Max => *max,
            },
        )?,
        Constraint::WordList(words) => {
            let numbers = words.iter().filter_map(|&word| number(option, word));
            match end {
                End::Min => numbers.reduce(f64::min)?,
                End::Max => numbers.reduce(f64::max)?,
            }
        }
        _ => return None,
    };
    match option.unit {
        Unit::Mm => Some(value),
        Unit::Pixel => dpi.filter(|&dpi| dpi > 0.0).map(|dpi| value / dpi * 25.4),
        _ => None,
    }
}

/// A length an area option holds, `value` in its unit, in millimetres: as it is in millimetres,
/// converted at `dpi` in pixels; `None` for another unit, or pixels with no resolution above 0.
pub(super) fn value_mm(option: &OptionDescriptor, value: f64, dpi: Option<f64>) -> Option<f64> {
    match option.unit {
        Unit::Mm => Some(value),
        Unit::Pixel => dpi.filter(|&dpi| dpi > 0.0).map(|dpi| value / dpi * 25.4),
        _ => None,
    }
}

/// `mm` millimetres as a value of an area option, in its unit (pixels converted at `dpi`) and of
/// its type, in each of its words: an `INT`'s rounded. `None` where the unit isn't one of those,
/// or the number doesn't fit the type.
pub(super) fn mm_value(option: &OptionDescriptor, mm: f64, dpi: Option<f64>) -> Option<Value> {
    let length = match option.unit {
        Unit::Mm => mm,
        Unit::Pixel => mm / 25.4 * dpi.filter(|&dpi| dpi > 0.0)?,
        _ => return None,
    };
    number_value(option, length)
}

/// `value` as a value of a numeric option, in each of its words: an `INT`'s rounded to the
/// nearest, a `FIXED`'s to the nearest step; `None` where it doesn't fit the type, or the option
/// isn't a number.
pub(super) fn number_value(option: &OptionDescriptor, value: f64) -> Option<Value> {
    let words = option.word_count().max(1);
    match option.value_type {
        ValueType::Int => {
            let rounded = value.round();
            // NaN fails both comparisons.
            (rounded >= f64::from(i32::MIN) && rounded <= f64::from(i32::MAX))
                .then(|| Value::Int(vec![rounded as i32; words]))
        }
        ValueType::Fixed => Fixed::from_f64(value).map(|fixed| Value::Fixed(vec![fixed; words])),
        _ => None,
    }
}

/// The least value a numeric option's constraint allows, as a value of it, in each of its words:
/// a range's least, a word list's; 0 where it has no such constraint.
pub(super) fn least_value(option: &OptionDescriptor) -> Option<Value> {
    let words = option.word_count().max(1);
    let least = match &option.constraint {
        Constraint::Range { min, .. } => *min,
        // A fixed-point word orders as its number does.
        Constraint::WordList(list) => list.iter().copied().min().unwrap_or(0),
        _ => 0,
    };
    match option.value_type {
        ValueType::Int => Some(Value::Int(vec![least; words])),
        ValueType::Fixed => Some(Value::Fixed(vec![Fixed(least); words])),
        _ => None,
    }
}

/// The first word of a numeric value, as a number; `None` for another value.
pub(super) fn first_number(value: &Value) -> Option<f64> {
    match value {
        Value::Int(words) => words.first().map(|&word| f64::from(word)),
        Value::Fixed(words) => words.first().map(|word| word.to_f64()),
        _ => None,
    }
}

/// An area `width` by `height` millimetres, where both are finite and above 0, as `f32`s.
pub(super) fn area(width: f64, height: f64) -> Option<SizeMm> {
    let size = SizeMm {
        width: width as f32,
        height: height as f32,
    };
    let fits = |side: f32| side.is_finite() && side > 0.0;
    (fits(size.width) && fits(size.height)).then_some(size)
}

/// A word of an option's value or constraint as a number: an `INT`'s as it is, a `FIXED`'s as
/// its fixed-point number; `None` for an option of another type.
pub(super) fn number(option: &OptionDescriptor, word: i32) -> Option<f64> {
    match option.value_type {
        ValueType::Int => Some(f64::from(word)),
        ValueType::Fixed => Some(Fixed(word).to_f64()),
        _ => None,
    }
}

/// The strings a string option's constraint lists; none where it has no list.
pub(super) fn strings(option: &OptionDescriptor) -> &[SaneString] {
    match &option.constraint {
        Constraint::StringList(values) => values,
        _ => &[],
    }
}

/// Whether an option can be set now: usable, active and settable by software.
pub(super) fn settable(option: &OptionDescriptor) -> bool {
    option.unusable.is_none()
        && option.capabilities.is_active()
        && option.capabilities.soft_select()
}

/// Whether an option's value can be read now: usable, active and readable by software.
pub(super) fn readable(option: &OptionDescriptor) -> bool {
    option.unusable.is_none()
        && option.capabilities.is_active()
        && option.capabilities.soft_detect()
}

/// The value an option set by name takes from `text`, read by the option's type: a string's
/// bytes; a boolean from `true`, `false`, `yes`, `no`, `1` or `0`; an integer or a fixed-point
/// number from one decimal number, put in each of its words; a button pressed, whatever `text`
/// says. Why it can't be read, where it can't.
pub(super) fn value_by_name(option: &OptionDescriptor, text: &str) -> Result<Value, String> {
    let words = option.word_count().max(1);
    match option.value_type {
        ValueType::String => Ok(Value::String(SaneString::from(text))),
        ValueType::Bool => match text {
            "true" | "yes" | "1" => Ok(Value::Bool(true)),
            "false" | "no" | "0" => Ok(Value::Bool(false)),
            _ => Err("it takes true, false, yes, no, 1 or 0".to_owned()),
        },
        ValueType::Int => text
            .parse::<i32>()
            .map(|number| Value::Int(vec![number; words]))
            .map_err(|_| "it takes a whole number".to_owned()),
        ValueType::Fixed => text
            .parse::<f64>()
            .ok()
            .and_then(Fixed::from_f64)
            .map(|number| Value::Fixed(vec![number; words]))
            .ok_or_else(|| "it takes a number from -32768 to 32767".to_owned()),
        ValueType::Button => Ok(Value::Button),
        other => Err(format!("it's a {other}, which has no value to set")),
    }
}

/// Sets each option given by name on `handle`, in order, where the device has it and it's
/// active; one the device lacks or that's inactive is skipped. One whose value doesn't read, or
/// that SANE refuses, is [`ScanError::Driver`], saying which.
pub(super) fn set_by_name(
    handle: &mut Handle,
    options: &[(String, String)],
) -> Result<(), ScanError> {
    for (name, text) in options {
        let Some(option) = handle.find_option(name) else {
            continue;
        };
        if !option.capabilities.is_active() {
            continue;
        }
        let index = option.index;
        let refused = |reason: String| ScanError::Driver {
            message: format!("couldn't set the scanner's option {name:?} to {text:?}: {reason}"),
            shown: false,
        };
        let value = value_by_name(option, text).map_err(refused)?;
        handle
            .set(index, &value)
            .map_err(|error| refused(error::chain(&error)))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use sane_dl::Capabilities;

    use super::*;

    /// A descriptor as a backend might give it: active, readable and settable, unconstrained.
    fn option(
        name: &str,
        value_type: ValueType,
        unit: Unit,
        constraint: Constraint,
    ) -> OptionDescriptor {
        OptionDescriptor {
            index: 1,
            name: name.to_owned(),
            title: name.to_owned(),
            description: String::new(),
            value_type,
            unit,
            size: 4,
            capabilities: Capabilities(Capabilities::SOFT_SELECT | Capabilities::SOFT_DETECT),
            constraint,
            unusable: None,
        }
    }

    fn list(values: &[&str]) -> Vec<SaneString> {
        values
            .iter()
            .map(|&value| SaneString::from(value))
            .collect()
    }

    fn fixed(value: f64) -> i32 {
        Fixed::from_f64(value).unwrap().0
    }

    fn range(min: u32, max: u32, step: u32) -> Option<Resolutions> {
        Some(Resolutions::Range {
            min,
            max,
            step: std::num::NonZeroU32::new(step).unwrap(),
        })
    }

    #[test]
    fn each_source_value_the_rows_name() {
        for value in ["Flatbed", "FlatBed", "Normal", "Document Table"] {
            assert_eq!(
                source_kind(value.as_bytes()),
                Some(SourceKind::Flatbed),
                "{value}"
            );
        }
        for value in [
            "ADF",
            "ADF Front",
            "ADF Simplex",
            "Automatic Document Feeder",
            "Automatic Document Feeder(left aligned)",
            "Automatic Document Feeder(centrally aligned)",
            "Automatic Document Feeder(center aligned)",
        ] {
            assert_eq!(
                source_kind(value.as_bytes()),
                Some(SourceKind::Feeder),
                "{value}"
            );
        }
        for value in [
            "ADF Duplex",
            "Duplex ADF",
            "Duplex",
            "ADF Duplex - Long-Edge Binding",
            "Automatic Document Feeder(left aligned,Duplex)",
            "Automatic Document Feeder(centrally aligned,Duplex)",
        ] {
            assert_eq!(
                source_kind(value.as_bytes()),
                Some(SourceKind::FeederDuplex),
                "{value}"
            );
        }
        // Left out: the backs alone, the flatbed that never stops, a part of a value, another
        // case, a translation.
        for value in [
            "ADF Back",
            "Auto",
            "ADF Duplex - Short-Edge Binding",
            "adf",
            "flatbed",
            "Flatbed ",
            "Chargeur automatique",
            "",
        ] {
            assert_eq!(source_kind(value.as_bytes()), None, "{value}");
        }
    }

    #[test]
    fn the_first_of_each_source_is_taken_and_brother_s_duplex_matches_its_one_side() {
        let fujitsu = source_values(&list(&["Flatbed", "ADF Front", "ADF Back", "ADF Duplex"]));
        assert_eq!(
            fujitsu,
            SourceValues {
                flatbed: Some("Flatbed".into()),
                feeder: Some("ADF Front".into()),
                duplex: Some("ADF Duplex".into()),
            }
        );
        let brother = source_values(&list(&[
            "FlatBed",
            "Automatic Document Feeder(centrally aligned,Duplex)",
            "Automatic Document Feeder(left aligned)",
            "Automatic Document Feeder(left aligned,Duplex)",
            "Automatic Document Feeder(centrally aligned)",
        ]));
        assert_eq!(
            brother.feeder,
            Some("Automatic Document Feeder(left aligned)".into())
        );
        assert_eq!(
            brother.duplex,
            Some("Automatic Document Feeder(left aligned,Duplex)".into())
        );
        // Without the matching one, the first listed.
        let unmatched = source_values(&list(&[
            "Automatic Document Feeder(center aligned)",
            "Automatic Document Feeder(centrally aligned,Duplex)",
        ]));
        assert_eq!(
            unmatched.duplex,
            Some("Automatic Document Feeder(centrally aligned,Duplex)".into())
        );
        let samsung = source_values(&list(&[
            "ADF Duplex - Short-Edge Binding",
            "ADF Simplex",
            "ADF Duplex - Long-Edge Binding",
        ]));
        assert_eq!(samsung.flatbed, None);
        assert_eq!(samsung.feeder, Some("ADF Simplex".into()));
        assert_eq!(
            samsung.duplex,
            Some("ADF Duplex - Long-Edge Binding".into())
        );
        assert_eq!(
            source_values(&list(&["Auto", "ADF Back"])),
            SourceValues::default()
        );
    }

    #[test]
    fn duplex_apart_from_source_is_found_by_its_name_type_and_values() {
        let string = |name: &str, values: &[&str]| {
            option(
                name,
                ValueType::String,
                Unit::None,
                Constraint::StringList(list(values)),
            )
        };
        let boolean = |name: &str| option(name, ValueType::Bool, Unit::None, Constraint::None);
        let simplex_duplex = DuplexSwitch::Values {
            simplex: b"Simplex",
            duplex: b"Duplex",
        };
        for name in ["ScanMode", "adf-mode", "adf_mode"] {
            let options = [string(name, &["Simplex", "Duplex"])];
            assert_eq!(
                duplex_switch(&options).map(|(option, switch)| (option.name.as_str(), switch)),
                Some((name, simplex_duplex)),
                "{name}"
            );
        }
        for name in ["scan-both-sides", "duplex"] {
            let options = [boolean(name)];
            assert_eq!(
                duplex_switch(&options).map(|(_, switch)| switch),
                Some(DuplexSwitch::Bool),
                "{name}"
            );
        }
        let kodak = [string("duplex", &["front", "back", "both"])];
        assert_eq!(
            duplex_switch(&kodak).map(|(_, switch)| switch),
            Some(DuplexSwitch::Values {
                simplex: b"front",
                duplex: b"both",
            })
        );
        // One that lacks a value, is inactive, can't be set, or is of another type: none.
        assert!(duplex_switch(&[string("adf-mode", &["Simplex"])]).is_none());
        assert!(duplex_switch(&[string("duplex", &["front", "back"])]).is_none());
        let mut inactive = boolean("scan-both-sides");
        inactive.capabilities = Capabilities(inactive.capabilities.0 | Capabilities::INACTIVE);
        assert!(duplex_switch(&[inactive]).is_none());
        let mut fixed_by_hand = boolean("duplex");
        fixed_by_hand.capabilities = Capabilities(Capabilities::SOFT_DETECT);
        assert!(duplex_switch(&[fixed_by_hand]).is_none());
        assert!(
            duplex_switch(&[option(
                "duplex",
                ValueType::Int,
                Unit::None,
                Constraint::None
            )])
            .is_none()
        );
        assert!(duplex_switch(&[string("ADF-mode", &["Simplex", "Duplex"])]).is_none());
    }

    #[test]
    fn each_mode_the_rows_name_and_the_first_of_each() {
        for value in [
            "Lineart",
            "Binary",
            "Black & White",
            "Black and White - Line Art",
        ] {
            assert_eq!(
                mode_kind(value.as_bytes()),
                Some(ModeKind::Lineart),
                "{value}"
            );
        }
        for value in ["Gray", "Grayscale", "True Gray"] {
            assert_eq!(mode_kind(value.as_bytes()), Some(ModeKind::Gray), "{value}");
        }
        for value in ["Color", "24bit Color"] {
            assert_eq!(
                mode_kind(value.as_bytes()),
                Some(ModeKind::Color),
                "{value}"
            );
        }
        for value in [
            "Halftone",
            "Gray[Error Diffusion]",
            "24bit Color[Fast]",
            "gray",
            "Couleur",
        ] {
            assert_eq!(mode_kind(value.as_bytes()), None, "{value}");
        }
        let brother = mode_values(&list(&[
            "Black & White",
            "Gray[Error Diffusion]",
            "True Gray",
            "24bit Color",
            "24bit Color[Fast]",
        ]));
        assert_eq!(
            brother,
            ModeValues {
                lineart: Some("Black & White".into()),
                gray: Some("True Gray".into()),
                color: Some("24bit Color".into()),
            }
        );
    }

    #[test]
    fn formats_from_modes_and_depths_in_the_model_s_order() {
        let modes = |values: &[&str]| mode_values(&list(values));
        use Format::*;
        // No depth option: 8 bits, and black and white from its mode alone.
        assert_eq!(
            formats(&modes(&["Color", "Gray", "Lineart"]), None),
            [Bw1, Gray8, Rgb8]
        );
        assert_eq!(formats(&modes(&["Gray", "Color"]), None), [Gray8, Rgb8]);
        // The test backend's: gray at 1 is black and white, color at 1 left out.
        assert_eq!(
            formats(&modes(&["Gray", "Color"]), Some(&[1, 8, 16])),
            [Bw1, Gray8, Gray16, Rgb8, Rgb16]
        );
        assert_eq!(formats(&modes(&["Color"]), Some(&[1, 8])), [Rgb8]);
        assert_eq!(
            formats(&modes(&["Lineart", "Gray"]), Some(&[16])),
            [Bw1, Gray16]
        );
        assert_eq!(formats(&modes(&["Halftone", "Couleur"]), Some(&[8])), []);
        assert_eq!(formats(&ModeValues::default(), None), []);
    }

    #[test]
    fn depths_from_a_word_list_or_a_range() {
        let depth = |constraint| option("depth", ValueType::Int, Unit::Bit, constraint);
        assert_eq!(
            depths(&depth(Constraint::WordList(vec![1, 8, 16]))),
            [1, 8, 16]
        );
        assert_eq!(depths(&depth(Constraint::WordList(vec![8, 12]))), [8]);
        assert_eq!(
            depths(&depth(Constraint::Range {
                min: 1,
                max: 16,
                quant: 0
            })),
            [1, 8, 16]
        );
        assert_eq!(
            depths(&depth(Constraint::Range {
                min: 0,
                max: 16,
                quant: 8
            })),
            [8, 16]
        );
        assert_eq!(depths(&depth(Constraint::None)), Vec::<u32>::new());
    }

    #[test]
    fn a_format_from_the_parameters_where_there_is_no_mode() {
        let parameters = |format, depth| Parameters {
            format,
            last_frame: 1,
            bytes_per_line: 2,
            pixels_per_line: 13,
            lines: 3,
            depth,
        };
        use Format::*;
        assert_eq!(format_of(&parameters(FrameFormat::GRAY, 1)), Some(Bw1));
        assert_eq!(format_of(&parameters(FrameFormat::GRAY, 8)), Some(Gray8));
        assert_eq!(format_of(&parameters(FrameFormat::GRAY, 16)), Some(Gray16));
        assert_eq!(format_of(&parameters(FrameFormat::RGB, 8)), Some(Rgb8));
        assert_eq!(format_of(&parameters(FrameFormat::RGB, 1)), Some(Rgb8));
        assert_eq!(format_of(&parameters(FrameFormat::GREEN, 16)), Some(Rgb16));
        assert_eq!(format_of(&parameters(FrameFormat::RED, 8)), Some(Rgb8));
        assert_eq!(format_of(&parameters(FrameFormat::GRAY, 12)), None);
        assert_eq!(format_of(&parameters(FrameFormat::JPEG, 8)), None);
        assert_eq!(format_of(&parameters(FrameFormat(99), 8)), None);
    }

    #[test]
    fn resolutions_at_their_edges() {
        let int = |constraint| option("resolution", ValueType::Int, Unit::Dpi, constraint);
        let fix = |constraint| option("resolution", ValueType::Fixed, Unit::Dpi, constraint);
        // The test backend's: a FIXED range of 1 to 1200, its step 1.
        let test = fix(Constraint::Range {
            min: fixed(1.0),
            max: fixed(1200.0),
            quant: fixed(1.0),
        });
        assert_eq!(resolutions(&test, None), range(1, 1200, 1));
        // A step of 0, or under 1, is 1; a larger fractional one rounded up.
        assert_eq!(
            resolutions(
                &int(Constraint::Range {
                    min: 50,
                    max: 600,
                    quant: 0
                }),
                None
            ),
            range(50, 600, 1)
        );
        let half = fix(Constraint::Range {
            min: fixed(50.0),
            max: fixed(600.0),
            quant: fixed(0.5),
        });
        assert_eq!(resolutions(&half, None), range(50, 600, 1));
        let odd = fix(Constraint::Range {
            min: fixed(50.0),
            max: fixed(600.0),
            quant: fixed(2.5),
        });
        assert_eq!(resolutions(&odd, None), range(50, 600, 3));
        // A FIXED range's ends rounded inward.
        let inward = fix(Constraint::Range {
            min: fixed(74.5),
            max: fixed(600.75),
            quant: 0,
        });
        assert_eq!(resolutions(&inward, None), range(75, 600, 1));
        // Nothing below 1: the least raised to its first step at 1 or above.
        assert_eq!(
            resolutions(
                &int(Constraint::Range {
                    min: 0,
                    max: 1200,
                    quant: 0
                }),
                None
            ),
            range(1, 1200, 1)
        );
        assert_eq!(
            resolutions(
                &int(Constraint::Range {
                    min: -100,
                    max: 1200,
                    quant: 50
                }),
                None
            ),
            range(50, 1200, 50)
        );
        // A range with no whole dpi, or none at 1 or above: dropped.
        let none = fix(Constraint::Range {
            min: fixed(0.25),
            max: fixed(0.75),
            quant: 0,
        });
        assert_eq!(resolutions(&none, None), None);
        assert_eq!(
            resolutions(
                &int(Constraint::Range {
                    min: -10,
                    max: 0,
                    quant: 1
                }),
                None
            ),
            None
        );
        assert_eq!(
            resolutions(
                &int(Constraint::Range {
                    min: 600,
                    max: 300,
                    quant: 1
                }),
                None
            ),
            None
        );
        // A list's values to the nearest whole dpi, the duplicates and those below 1 dropped.
        let words = int(Constraint::WordList(vec![300, 75, 0, -5, 150, 300]));
        assert_eq!(
            resolutions(&words, None),
            Some(Resolutions::List(vec![75, 150, 300]))
        );
        let fixed_words = fix(Constraint::WordList(vec![
            fixed(74.6),
            fixed(75.4),
            fixed(0.4),
            fixed(150.0),
        ]));
        assert_eq!(
            resolutions(&fixed_words, None),
            Some(Resolutions::List(vec![75, 150]))
        );
        assert_eq!(
            resolutions(&int(Constraint::WordList(vec![0, -1])), None),
            None
        );
        assert_eq!(
            resolutions(&int(Constraint::WordList(Vec::new())), None),
            None
        );
        // No constraint: the value now, alone.
        assert_eq!(
            resolutions(&int(Constraint::None), Some(299.6)),
            Some(Resolutions::List(vec![300]))
        );
        assert_eq!(resolutions(&int(Constraint::None), None), None);
        assert_eq!(resolutions(&int(Constraint::None), Some(0.2)), None);
        // An option of another type allows none.
        let string = option(
            "resolution",
            ValueType::String,
            Unit::Dpi,
            Constraint::StringList(list(&["300"])),
        );
        assert_eq!(resolutions(&string, None), None);
    }

    #[test]
    fn lengths_in_millimetres_and_in_pixels() {
        let mm = option(
            "br-x",
            ValueType::Fixed,
            Unit::Mm,
            Constraint::Range {
                min: fixed(0.0),
                max: fixed(215.9),
                quant: 0,
            },
        );
        assert!((length_mm(&mm, End::Max, None).unwrap() - 215.9).abs() < 1e-4);
        assert_eq!(length_mm(&mm, End::Min, None), Some(0.0));
        let int_mm = option(
            "br-y",
            ValueType::Int,
            Unit::Mm,
            Constraint::Range {
                min: 5,
                max: 297,
                quant: 1,
            },
        );
        assert_eq!(length_mm(&int_mm, End::Max, None), Some(297.0));
        assert_eq!(length_mm(&int_mm, End::Min, None), Some(5.0));
        // In pixels, at the resolution now.
        let pixels = option(
            "br-x",
            ValueType::Int,
            Unit::Pixel,
            Constraint::Range {
                min: 0,
                max: 2550,
                quant: 1,
            },
        );
        let width = length_mm(&pixels, End::Max, Some(300.0)).unwrap();
        assert!((width - 215.9).abs() < 1e-9, "{width}");
        assert_eq!(length_mm(&pixels, End::Max, None), None);
        assert_eq!(length_mm(&pixels, End::Max, Some(0.0)), None);
        // A word list's least and greatest.
        let words = option(
            "page-height",
            ValueType::Int,
            Unit::Mm,
            Constraint::WordList(vec![297, 210, 356]),
        );
        assert_eq!(length_mm(&words, End::Max, None), Some(356.0));
        assert_eq!(length_mm(&words, End::Min, None), Some(210.0));
        // Another unit, or no constraint: none.
        let dpi = option(
            "br-x",
            ValueType::Int,
            Unit::Dpi,
            Constraint::Range {
                min: 0,
                max: 10,
                quant: 1,
            },
        );
        assert_eq!(length_mm(&dpi, End::Max, None), None);
        let free = option("br-x", ValueType::Int, Unit::Mm, Constraint::None);
        assert_eq!(length_mm(&free, End::Max, None), None);
    }

    #[test]
    fn an_area_is_finite_and_above_0() {
        assert_eq!(
            area(200.0, 200.0),
            Some(SizeMm {
                width: 200.0,
                height: 200.0
            })
        );
        assert_eq!(area(0.0, 200.0), None);
        assert_eq!(area(200.0, -1.0), None);
        assert_eq!(area(f64::NAN, 1.0), None);
        assert_eq!(area(1.0, f64::INFINITY), None);
        assert_eq!(area(1e300, 1.0), None);
    }

    #[test]
    fn values_set_by_name_are_read_by_the_option_s_type() {
        let string = option("filename", ValueType::String, Unit::None, Constraint::None);
        assert_eq!(
            value_by_name(&string, "/data/page.pgm"),
            Ok(Value::String("/data/page.pgm".into()))
        );
        let boolean = option("preview", ValueType::Bool, Unit::None, Constraint::None);
        for (text, value) in [
            ("true", true),
            ("yes", true),
            ("1", true),
            ("false", false),
            ("no", false),
            ("0", false),
        ] {
            assert_eq!(
                value_by_name(&boolean, text),
                Ok(Value::Bool(value)),
                "{text}"
            );
        }
        assert!(value_by_name(&boolean, "on").is_err());
        assert!(value_by_name(&boolean, "TRUE").is_err());
        let int = option("speed", ValueType::Int, Unit::None, Constraint::None);
        assert_eq!(value_by_name(&int, "-3"), Ok(Value::Int(vec![-3])));
        assert!(value_by_name(&int, "1.5").is_err());
        assert!(value_by_name(&int, "fast").is_err());
        // Each word of a vector.
        let mut table = int.clone();
        table.size = 12;
        assert_eq!(value_by_name(&table, "7"), Ok(Value::Int(vec![7, 7, 7])));
        let fix = option("gamma", ValueType::Fixed, Unit::None, Constraint::None);
        assert_eq!(
            value_by_name(&fix, "1.5"),
            Ok(Value::Fixed(vec![Fixed::from_f64(1.5).unwrap()]))
        );
        assert!(value_by_name(&fix, "40000").is_err());
        assert!(value_by_name(&fix, "NaN").is_err());
        assert!(value_by_name(&fix, "inf").is_err());
        let button = option("calibrate", ValueType::Button, Unit::None, Constraint::None);
        assert_eq!(value_by_name(&button, ""), Ok(Value::Button));
        let group = option("", ValueType::Group, Unit::None, Constraint::None);
        assert!(value_by_name(&group, "x").is_err());
    }
}
