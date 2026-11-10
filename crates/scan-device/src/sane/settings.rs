//! A batch's settings: each clamped to what the device's options allow as it's set, set in
//! order, then read back for [`Event::Applied`](crate::Event::Applied); and set again after a
//! start the device refused with them.
//!
//! The order: the source; duplex, where an option apart from the source gives it; the mode, then
//! the depth; the resolution; a feeder's page size; the scan area's top left at its least, and
//! its bottom right from there; then the options set by name. Each is clamped against the
//! descriptors as they are when it's set, since a set can change the others (the resolutions a
//! duplex source allows, say). Then every option the model sets is read back, and what the
//! device holds is what `Applied` says, so a value the device moved (rounded, or changed by a
//! reload or an option set by name) is said, whatever moved it.

use std::cmp::Reverse;

use sane_dl::{Call, Error, Handle, OptionDescriptor, SaneString, Status, Value, ValueType};

use super::caps::{self, Area, Dpi, NO_RESOLUTION_OPTION};
use super::error;
use super::options::{self, DuplexSwitch, ModeKind, ModeValues, SourceKind, SourceValues};
use crate::{Applied, Clamped, Format, Paper, Resolutions, ScanError, Settings, SizeMm, Source};

/// How near a size read back is to the one asked for it to be the one asked, in millimetres:
/// fixed point's rounding isn't a change.
const SAME_SIZE_MM: f64 = 0.005;

/// What a batch scans with, once its settings are set.
#[derive(Debug)]
pub(super) struct Taken {
    /// What the device took, and what it changed of what was asked.
    pub applied: Applied,
    /// A page's resolution across and down, as SANE gave it, its fraction kept.
    pub dpi: (f32, f32),
    /// Each option of the model's the device has, with its value read back, in the order they're
    /// set: what a start the device refused is checked against.
    restore: Vec<(String, Value)>,
    /// The options of those the settings set: what a start the device refused sets again.
    set: Vec<String>,
    /// The source option, where it's never set (it has one value, or software can't set it):
    /// left as it is after a start the device refused, too.
    fixed_source: Option<String>,
}

/// Sets `asked` on the device, in order, each clamped as it's set, then the options by name;
/// reads back what it took. A device with no source or no format the model can name is
/// [`ScanError::Unsupported`]; an area read back with no width or height, a resolution read back
/// at or below 0 or past `u32` once rounded, and a format read back that the model can't name,
/// [`ScanError::Driver`], since no page could carry them.
pub(super) fn apply(
    handle: &mut Handle,
    asked: &Settings,
    by_name: &[(String, String)],
) -> Result<Taken, ScanError> {
    let mut set = Vec::new();
    let source = set_source(handle, asked, &mut set)?;
    let switch = set_duplex(handle, asked, &source, &mut set)?;
    set_format(handle, asked.format, &mut set)?;
    let dpi_set = set_resolution(handle, asked.dpi, &mut set)?;
    set_area(handle, asked.paper, source.source, &mut set)?;
    options::set_by_name(handle, by_name)?;
    let restore = read_options(handle, &model_options(handle, &source, switch.as_ref()))?;
    let read = read_back(handle, &restore, &source, switch.as_ref(), dpi_set)?;
    let (applied, dpi) = applied_of(asked, &read)?;
    let fixed_source = source.option.clone().filter(|_| source.fixed);
    Ok(Taken {
        applied,
        dpi,
        restore,
        set,
        fixed_source,
    })
}

/// Sets again, in order, each of the model's options to the value `taken` read back, where it's
/// there and can be set now, after a start the device refused (`INVAL`, after which SANE may have
/// changed its options): those the settings set, and each other that holds another value now; a
/// source option that's never set is left as it is. Then the options by name, and every option
/// `taken` read is read back. A value the device refuses, or one that reads back otherwise than
/// `Applied` said, is [`ScanError::SettingsRefused`]. An option gone inactive is neither set nor
/// read.
pub(super) fn restore(
    handle: &mut Handle,
    taken: &Taken,
    by_name: &[(String, String)],
) -> Result<(), ScanError> {
    // A start's `INVAL` reads the descriptors again; where that failed, none are left to set.
    if handle.options().is_empty() {
        handle
            .reload_options()
            .map_err(|error| error::map(&error))?;
    }
    for (name, value) in &taken.restore {
        if taken.fixed_source.as_ref() == Some(name) {
            continue;
        }
        let Some(option) = handle
            .find_option(name)
            .filter(|option| options::settable(option))
            .cloned()
        else {
            continue;
        };
        let index = option.index;
        // One the settings didn't set (a value found as asked, say) is set back only where the
        // start moved it.
        if !taken.set.contains(name) && (!readable(&option) || get(handle, index)? == *value) {
            continue;
        }
        handle.set(index, value).map_err(|error| match error {
            Error::Status {
                status: Status::INVAL,
                call: Call::ControlOption,
            } => ScanError::SettingsRefused,
            other => refused(name, &other),
        })?;
    }
    options::set_by_name(handle, by_name)?;
    for (name, value) in &taken.restore {
        let Some(option) = handle.find_option(name).filter(|option| readable(option)) else {
            continue;
        };
        let index = option.index;
        if get(handle, index)? != *value {
            return Err(ScanError::SettingsRefused);
        }
    }
    Ok(())
}

/// The source as set.
#[derive(Debug)]
struct SourceSet {
    /// The source the model names it.
    source: Source,
    /// Whether its value is a feeder's both sides.
    both_sides: bool,
    /// The source option's name, where there is one.
    option: Option<String>,
    /// Whether the option is never set: it has one value, or software can't set it.
    fixed: bool,
}

/// Sets the source asked for, or the first the device lists where it lacks it (a flatbed, then a
/// feeder): its value as [`source_value`] chooses it. Not set where it already holds that value,
/// nor where the option has one value or software can't set it (some backends refuse to be set to
/// the one value they offer), so the device's own is taken. A device with no source option is one
/// flatbed.
fn set_source(
    handle: &mut Handle,
    asked: &Settings,
    touched: &mut Vec<String>,
) -> Result<SourceSet, ScanError> {
    let Some(option) = caps::source_option(handle) else {
        return Ok(SourceSet {
            source: Source::Flatbed,
            both_sides: false,
            option: None,
            fixed: false,
        });
    };
    let found = caps::get_string(handle, &option.name)?;
    let values = if option.values.is_empty() {
        vec![found.clone()]
    } else {
        option.values.clone()
    };
    let never_set = values.len() == 1 || !option.settable;
    let wanted = source_value(asked, &options::source_values(&values));
    let now = match wanted {
        Some(value) if value != found && !never_set => {
            set(handle, &option.name, &Value::String(value.clone()), touched)?;
            value
        }
        Some(_) => found,
        None => return Err(ScanError::Unsupported),
    };
    let (source, both_sides) = source_of(&now, asked.source);
    Ok(SourceSet {
        source,
        both_sides,
        option: Some(option.name),
        fixed: never_set,
    })
}

/// The source value for `asked`: a flatbed's for a flatbed; for a feeder, its both sides' where
/// duplex is asked and there is one, else its one side's, else its both sides'; and where the
/// device lacks the source asked, the first it lists of a flatbed and a feeder.
fn source_value(asked: &Settings, values: &SourceValues) -> Option<SaneString> {
    let SourceValues {
        flatbed,
        feeder,
        duplex,
    } = values.clone();
    let feeder = if asked.duplex {
        duplex.or(feeder)
    } else {
        feeder.or(duplex)
    };
    match asked.source {
        Source::Flatbed => flatbed.or(feeder),
        Source::Feeder => feeder.or(flatbed),
    }
}

/// The source a value stands for, and whether it's a feeder's both sides; a value no row names
/// (moved there by an option set by name, say) is taken as `set`, the source the batch set, since
/// the model names no other.
fn source_of(value: &SaneString, set: Source) -> (Source, bool) {
    match options::source_kind(value.as_bytes()) {
        Some(SourceKind::Flatbed) => (Source::Flatbed, false),
        Some(SourceKind::Feeder) => (Source::Feeder, false),
        Some(SourceKind::FeederDuplex) => (Source::Feeder, true),
        None => (set, false),
    }
}

/// Sets duplex through an option apart from the source, where the source value doesn't give it
/// and the device has such an option active now: on for a feeder where duplex is asked, off
/// otherwise. The option's name and how it reads, where there is one.
fn set_duplex(
    handle: &mut Handle,
    asked: &Settings,
    source: &SourceSet,
    touched: &mut Vec<String>,
) -> Result<Option<(String, DuplexSwitch)>, ScanError> {
    if source.both_sides {
        return Ok(None);
    }
    let Some((option, switch)) = options::duplex_switch(handle.options()) else {
        return Ok(None);
    };
    let name = option.name.clone();
    let on = asked.duplex && source.source == Source::Feeder;
    let value = match switch {
        DuplexSwitch::Bool => Value::Bool(on),
        DuplexSwitch::Values { simplex, duplex } => {
            Value::String(SaneString::from(if on { duplex } else { simplex }))
        }
    };
    set(handle, &name, &value, touched)?;
    Ok(Some((name, switch)))
}

/// Sets the mode, then the depth, for `asked`, or for the format the source gives in its place
/// ([`format_for`]). A device whose mode can't be set, or that has none, gives what it gives.
fn set_format(
    handle: &mut Handle,
    asked: Format,
    touched: &mut Vec<String>,
) -> Result<(), ScanError> {
    let offered = caps::formats(handle)?;
    if offered.is_empty() {
        return Err(ScanError::Unsupported);
    }
    let format = format_for(asked, &offered);
    let Some(mode) = handle
        .find_option(options::MODE)
        .filter(|mode| mode.value_type == ValueType::String && options::settable(mode))
    else {
        return Ok(());
    };
    let (value, bits) = mode_for(format, &options::mode_values(options::strings(mode)));
    if let Some(value) = value {
        set(handle, options::MODE, &Value::String(value), touched)?;
    }
    let depth = bits.and_then(|bits| {
        let depth = handle.find_option(options::DEPTH)?;
        if !options::settable(depth) || !options::allows(depth, f64::from(bits)) {
            return None;
        }
        options::number_value(depth, f64::from(bits))
    });
    if let Some(depth) = depth {
        set(handle, options::DEPTH, &depth, touched)?;
    }
    Ok(())
}

/// The format a source offering `offered` (never empty) gives for `asked`: `asked` where it's
/// offered, else its sibling (gray at 16 bits and at 8, color at 16 and at 8, black and white and
/// gray at 8), else the first offered.
fn format_for(asked: Format, offered: &[Format]) -> Format {
    if offered.contains(&asked) {
        return asked;
    }
    let sibling = match asked {
        Format::Bw1 | Format::Gray16 => Format::Gray8,
        Format::Gray8 => Format::Gray16,
        Format::Rgb8 => Format::Rgb16,
        Format::Rgb16 => Format::Rgb8,
    };
    if offered.contains(&sibling) {
        sibling
    } else {
        offered[0]
    }
}

/// The mode value and the depth that give `format`: black and white from a black-and-white mode,
/// else gray at 1 bit; gray and color at 8 and 16 bits. `None` for what there's no value for.
fn mode_for(format: Format, modes: &ModeValues) -> (Option<SaneString>, Option<u32>) {
    match format {
        Format::Bw1 => match &modes.lineart {
            Some(lineart) => (Some(lineart.clone()), None),
            None => (modes.gray.clone(), Some(1)),
        },
        Format::Gray8 => (modes.gray.clone(), Some(8)),
        Format::Gray16 => (modes.gray.clone(), Some(16)),
        Format::Rgb8 => (modes.color.clone(), Some(8)),
        Format::Rgb16 => (modes.color.clone(), Some(16)),
    }
}

/// Sets the resolution nearest `asked` that the option allows now ([`nearest`]), and
/// `y-resolution` alike where the option is `x-resolution`; the dpi set, where one was. A device
/// whose resolution can't be set, or allows none the model can name, gives what it gives, and
/// the read-back judges it.
fn set_resolution(
    handle: &mut Handle,
    asked: u32,
    touched: &mut Vec<String>,
) -> Result<Option<u32>, ScanError> {
    let Some(option) = caps::resolution_option(handle).cloned() else {
        return Ok(None);
    };
    if !options::settable(&option) {
        return Ok(None);
    }
    let current = if options::readable(&option) {
        caps::current_number(handle, &option)?
    } else {
        None
    };
    let Some(resolutions) = options::resolutions(&option, current) else {
        return Ok(None);
    };
    let dpi = nearest(&resolutions, asked);
    set_number(handle, &option.name, f64::from(dpi), touched)?;
    if option.name == options::X_RESOLUTION
        && handle
            .find_option(options::Y_RESOLUTION)
            .is_some_and(options::settable)
    {
        set_number(handle, options::Y_RESOLUTION, f64::from(dpi), touched)?;
    }
    Ok(Some(dpi))
}

/// The resolution nearest `asked` among `resolutions`, the higher of two as near.
fn nearest(resolutions: &Resolutions, asked: u32) -> u32 {
    match resolutions {
        Resolutions::List(values) => values
            .iter()
            .copied()
            .min_by_key(|&value| (value.abs_diff(asked), Reverse(value)))
            .unwrap_or(asked),
        Resolutions::Range { min, max, step } => {
            // In `u64`, so no step overflows; never past `max`, itself a `u32`.
            let [min, max, step, asked] = [*min, *max, step.get(), asked].map(u64::from);
            let last = min + max.saturating_sub(min) / step * step;
            let nearest = if asked <= min {
                min
            } else if asked >= last {
                last
            } else {
                let below = min + (asked - min) / step * step;
                let above = below + step;
                if asked - below < above - asked {
                    below
                } else {
                    above
                }
            };
            u32::try_from(nearest).unwrap_or(u32::MAX)
        }
    }
}

/// Sets the scan area for `paper` ([`paper_for`]), where the device has area options: a feeder's
/// page size, where it's active, to the paper's; the top left at its least; the bottom right at
/// that plus the paper's size, top-left aligned. In each option's unit, pixels at the resolution
/// set, lengths across at the resolution across and down at the resolution down
/// ([`caps::dpi_now`]).
fn set_area(
    handle: &mut Handle,
    paper: Paper,
    source: Source,
    touched: &mut Vec<String>,
) -> Result<(), ScanError> {
    let dpi = caps::dpi_now(handle)?;
    let Some(largest) = Area::of(handle, source).largest(dpi) else {
        return Ok(());
    };
    let size = paper_for(paper, largest);
    let (width, height) = (f64::from(size.width), f64::from(size.height));
    let Dpi { across, down } = dpi;
    set_length(handle, options::PAGE_WIDTH, width, across, touched)?;
    set_length(handle, options::PAGE_HEIGHT, height, down, touched)?;
    let left = set_least(handle, options::TL_X, across, touched)?;
    let top = set_least(handle, options::TL_Y, down, touched)?;
    set_length(handle, options::BR_X, left + width, across, touched)?;
    set_length(handle, options::BR_Y, top + height, down, touched)?;
    Ok(())
}

/// The paper set for `asked` on a source whose largest area is `largest`: a size cut to it, and
/// `Paper::Auto` (no SANE source finds the paper's size itself here) or a size that isn't one
/// (not finite and above 0) the largest.
fn paper_for(asked: Paper, largest: SizeMm) -> SizeMm {
    match asked {
        Paper::Size(size) if is_paper(size) => SizeMm {
            width: size.width.min(largest.width),
            height: size.height.min(largest.height),
        },
        _ => largest,
    }
}

/// Whether `size` is a paper's: finite and above 0 both ways.
fn is_paper(size: SizeMm) -> bool {
    let fits = |side: f32| side.is_finite() && side > 0.0;
    fits(size.width) && fits(size.height)
}

/// Sets the area option `name` to `mm` millimetres, where it's active and can be set.
fn set_length(
    handle: &mut Handle,
    name: &str,
    mm: f64,
    dpi: Option<f64>,
    touched: &mut Vec<String>,
) -> Result<(), ScanError> {
    let Some(option) = handle
        .find_option(name)
        .filter(|option| options::settable(option))
    else {
        return Ok(());
    };
    let Some(value) = options::mm_value(option, mm, dpi) else {
        return Err(driver(format!(
            "the scanner's option {name:?} can't take {mm} mm"
        )));
    };
    set(handle, name, &value, touched)
}

/// Sets the area option `name` to the least its constraint allows, where it's active and can be
/// set; its least in millimetres, where it's active, else 0.
fn set_least(
    handle: &mut Handle,
    name: &str,
    dpi: Option<f64>,
    touched: &mut Vec<String>,
) -> Result<f64, ScanError> {
    let Some(option) = handle
        .find_option(name)
        .filter(|option| option.capabilities.is_active())
        .cloned()
    else {
        return Ok(0.0);
    };
    let least = options::length_mm(&option, options::End::Min, dpi).unwrap_or(0.0);
    if options::settable(&option)
        && let Some(value) = options::least_value(&option)
    {
        set(handle, name, &value, touched)?;
    }
    Ok(least)
}

/// The names of the options the model sets that the device has, in the order they're set.
fn model_options(
    handle: &Handle,
    source: &SourceSet,
    switch: Option<&(String, DuplexSwitch)>,
) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    names.extend(source.option.clone());
    names.extend(switch.map(|(name, _)| name.clone()));
    names.extend([options::MODE, options::DEPTH].map(str::to_owned));
    if let Some(resolution) = caps::resolution_option(handle) {
        names.push(resolution.name.clone());
        if resolution.name == options::X_RESOLUTION {
            names.push(options::Y_RESOLUTION.to_owned());
        }
    }
    names.extend(
        [
            options::PAGE_WIDTH,
            options::PAGE_HEIGHT,
            options::TL_X,
            options::TL_Y,
            options::BR_X,
            options::BR_Y,
        ]
        .map(str::to_owned),
    );
    names
}

/// Whether an option has a value software can read now.
fn readable(option: &OptionDescriptor) -> bool {
    options::readable(option) && !matches!(option.value_type, ValueType::Button | ValueType::Group)
}

/// The value of each option named that the device has and can read now, in order.
fn read_options(handle: &mut Handle, names: &[String]) -> Result<Vec<(String, Value)>, ScanError> {
    let mut values = Vec::new();
    for name in names {
        let Some(option) = handle.find_option(name).filter(|option| readable(option)) else {
            continue;
        };
        let index = option.index;
        values.push((name.clone(), get(handle, index)?));
    }
    Ok(values)
}

/// What the device holds once the settings are set, as read back.
#[derive(Clone, Debug, PartialEq)]
struct ReadBack {
    /// The source.
    source: Source,
    /// Whether it scans both sides.
    duplex: bool,
    /// The format.
    format: Format,
    /// The resolution across and down, in dpi.
    dpi: (f64, f64),
    /// The scan area's width and height in millimetres; `None` where it has no area options.
    area: Option<(f64, f64)>,
}

/// What the device holds, from `values` (the model's options read back), and from the parameters
/// where its mode names no format.
fn read_back(
    handle: &mut Handle,
    values: &[(String, Value)],
    source: &SourceSet,
    switch: Option<&(String, DuplexSwitch)>,
    dpi_set: Option<u32>,
) -> Result<ReadBack, ScanError> {
    let value = |name: &str| {
        values
            .iter()
            .find(|(given, _)| given == name)
            .map(|(_, value)| value)
    };
    let number = |name: &str| value(name).and_then(options::first_number);
    let (source_now, both_sides) = match source.option.as_deref().and_then(value) {
        Some(Value::String(value)) => source_of(value, source.source),
        _ => (source.source, source.both_sides),
    };
    let switched_on = switch.is_some_and(|(name, switch)| match (switch, value(name)) {
        (DuplexSwitch::Bool, Some(Value::Bool(on))) => *on,
        (DuplexSwitch::Values { duplex, .. }, Some(Value::String(value))) => {
            value.as_bytes() == *duplex
        }
        _ => false,
    });
    let duplex = source_now == Source::Feeder && (both_sides || switched_on);
    let mode = match value(options::MODE) {
        Some(Value::String(mode)) => options::mode_kind(mode.as_bytes()),
        _ => None,
    };
    // A depth that isn't a whole number of bits names no format.
    let depth = number(options::DEPTH).map(|bits| {
        if bits.fract() == 0.0 && (0.0..=64.0).contains(&bits) {
            bits as u32
        } else {
            u32::MAX
        }
    });
    let format = match format_of(mode, depth) {
        Some(format) => format,
        None => {
            let parameters = handle.parameters().map_err(|error| error::map(&error))?;
            options::format_of(&parameters).ok_or_else(|| {
                driver(format!(
                    "the scanner gives {} at {} bits, which isn't a format this backend names",
                    parameters.format, parameters.depth
                ))
            })?
        }
    };
    let resolution = caps::resolution_option(handle).map(|option| option.name.clone());
    let across = match resolution.as_deref().and_then(number) {
        Some(dpi) => dpi,
        None => f64::from(dpi_set.unwrap_or(NO_RESOLUTION_OPTION)),
    };
    let down = number(options::Y_RESOLUTION)
        .filter(|_| resolution.as_deref() == Some(options::X_RESOLUTION))
        .unwrap_or(across);
    let length = |name: &str, dpi: f64| {
        let option = handle.find_option(name)?;
        options::value_mm(option, number(name)?, Some(dpi))
    };
    let corners = (length(options::BR_X, across), length(options::BR_Y, down));
    let area = match corners {
        (Some(right), Some(bottom)) => {
            let left = length(options::TL_X, across).unwrap_or(0.0);
            let top = length(options::TL_Y, down).unwrap_or(0.0);
            Some((right - left, bottom - top))
        }
        _ => match (
            length(options::PAGE_WIDTH, across),
            length(options::PAGE_HEIGHT, down),
        ) {
            (Some(width), Some(height)) => Some((width, height)),
            _ => None,
        },
    };
    Ok(ReadBack {
        source: source_now,
        duplex,
        format,
        dpi: (across, down),
        area,
    })
}

/// The format a mode and a depth read back name: black and white from a black-and-white mode or
/// gray at 1 bit; gray and color at 8 bits (or no depth) and 16. `None` for what the model
/// doesn't name (color at 1 bit, say), read from the parameters instead.
fn format_of(mode: Option<ModeKind>, depth: Option<u32>) -> Option<Format> {
    match (mode?, depth) {
        (ModeKind::Lineart, _) => Some(Format::Bw1),
        (ModeKind::Gray, Some(1)) => Some(Format::Bw1),
        (ModeKind::Gray, None | Some(8)) => Some(Format::Gray8),
        (ModeKind::Gray, Some(16)) => Some(Format::Gray16),
        (ModeKind::Color, None | Some(8)) => Some(Format::Rgb8),
        (ModeKind::Color, Some(16)) => Some(Format::Rgb16),
        _ => None,
    }
}

/// `Applied` from what was read back, against what was asked: each setting the device holds
/// otherwise in [`Clamped`], with what was asked. The dpi rounded to whole dpi; the paper the
/// area read (one within 0.005 mm of the size asked is the size asked), or `Paper::Auto` where
/// there are no area options; the encodings asked dropped, since SANE gives none. With a page's
/// resolution, as SANE gave it.
///
/// A resolution at or below 0, or past `u32` once rounded, and an area with no width or height,
/// are [`ScanError::Driver`]: the model's dpi and paper are finite and above 0.
fn applied_of(asked: &Settings, read: &ReadBack) -> Result<(Applied, (f32, f32)), ScanError> {
    let (across, down) = read.dpi;
    let dpi = whole_dpi(across)?;
    whole_dpi(down)?;
    let paper = match read.area {
        None => Paper::Auto,
        Some((width, height)) => {
            let fits = |side: f64| side.is_finite() && side > 0.0 && side <= f64::from(f32::MAX);
            if !fits(width) || !fits(height) {
                return Err(driver(format!(
                    "the scanner's scan area reads back as {width} by {height} mm, which no page \
                     can be"
                )));
            }
            match asked.paper {
                Paper::Size(size)
                    if (width - f64::from(size.width)).abs() <= SAME_SIZE_MM
                        && (height - f64::from(size.height)).abs() <= SAME_SIZE_MM =>
                {
                    asked.paper
                }
                _ => Paper::Size(SizeMm {
                    width: width as f32,
                    height: height as f32,
                }),
            }
        }
    };
    let mut taken = asked.clone();
    let mut clamped = Vec::new();
    if read.source != asked.source {
        taken.source = read.source;
        clamped.push(Clamped::Source(asked.source));
    }
    if read.duplex != asked.duplex {
        taken.duplex = read.duplex;
        clamped.push(Clamped::Duplex(asked.duplex));
    }
    if dpi != asked.dpi {
        taken.dpi = dpi;
        clamped.push(Clamped::Dpi(asked.dpi));
    }
    if read.format != asked.format {
        taken.format = read.format;
        clamped.push(Clamped::Format(asked.format));
    }
    if paper != asked.paper {
        taken.paper = paper;
        clamped.push(Clamped::Paper(asked.paper));
    }
    if !asked.encodings.is_empty() {
        taken.encodings = Vec::new();
        clamped.push(Clamped::Encodings(asked.encodings.clone()));
    }
    Ok((Applied::new(taken, clamped), (across as f32, down as f32)))
}

/// A resolution read back, in whole dpi: above 0 and within `u32` once rounded.
fn whole_dpi(dpi: f64) -> Result<u32, ScanError> {
    let rounded = dpi.round();
    // NaN fails both comparisons.
    if dpi > 0.0 && rounded >= 1.0 && rounded <= f64::from(u32::MAX) {
        Ok(rounded as u32)
    } else {
        Err(driver(format!(
            "the scanner's resolution reads back as {dpi} dpi, which no page can have"
        )))
    }
}

/// Sets the option `name` to `value`, by its number now, and counts it among those `touched`: a
/// refusal says which option.
fn set(
    handle: &mut Handle,
    name: &str,
    value: &Value,
    touched: &mut Vec<String>,
) -> Result<(), ScanError> {
    let index = caps::index_of(handle, name)?;
    handle
        .set(index, value)
        .map_err(|error| refused(name, &error))?;
    if !touched.iter().any(|set| set == name) {
        touched.push(name.to_owned());
    }
    Ok(())
}

/// Sets the numeric option `name` to `value`, in its type.
fn set_number(
    handle: &mut Handle,
    name: &str,
    value: f64,
    touched: &mut Vec<String>,
) -> Result<(), ScanError> {
    let option = handle
        .find_option(name)
        .ok_or_else(|| driver(format!("the scanner's option {name:?} is gone")))?;
    let value = options::number_value(option, value)
        .ok_or_else(|| driver(format!("the scanner's option {name:?} can't take {value}")))?;
    set(handle, name, &value, touched)
}

/// Option `index`'s value.
fn get(handle: &mut Handle, index: usize) -> Result<Value, ScanError> {
    handle.get(index).map_err(|error| error::map(&error))
}

/// A set of `name` the device refused, as the model has it: the driver's words say which option.
fn refused(name: &str, error: &Error) -> ScanError {
    match error::map(error) {
        ScanError::Driver { message, shown } => ScanError::Driver {
            message: format!("couldn't set the scanner's option {name:?}: {message}"),
            shown,
        },
        other => other,
    }
}

/// `message`, the driver's words, unshown.
fn driver(message: String) -> ScanError {
    ScanError::Driver {
        message,
        shown: false,
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU32;

    use super::*;
    use crate::Encoding;

    fn values(flatbed: Option<&str>, feeder: Option<&str>, duplex: Option<&str>) -> SourceValues {
        SourceValues {
            flatbed: flatbed.map(SaneString::from),
            feeder: feeder.map(SaneString::from),
            duplex: duplex.map(SaneString::from),
        }
    }

    fn settings(source: Source, duplex: bool) -> Settings {
        let mut settings = Settings::new(source, 300, Format::Gray8);
        settings.duplex = duplex;
        settings
    }

    #[test]
    fn the_source_asked_or_the_first_listed_where_it_lacks_it() {
        let all = values(Some("Flatbed"), Some("ADF Front"), Some("ADF Duplex"));
        let pick = |source, duplex, values: &SourceValues| {
            source_value(&settings(source, duplex), values).map(|value| value.to_string())
        };
        assert_eq!(
            pick(Source::Flatbed, false, &all).as_deref(),
            Some("Flatbed")
        );
        assert_eq!(
            pick(Source::Feeder, false, &all).as_deref(),
            Some("ADF Front")
        );
        assert_eq!(
            pick(Source::Feeder, true, &all).as_deref(),
            Some("ADF Duplex")
        );
        // Duplex on a flatbed is the flatbed.
        assert_eq!(
            pick(Source::Flatbed, true, &all).as_deref(),
            Some("Flatbed")
        );
        // A feeder of one side alone: its one side, duplex asked or not (an option may switch it).
        let one_side = values(None, Some("ADF"), None);
        assert_eq!(
            pick(Source::Feeder, true, &one_side).as_deref(),
            Some("ADF")
        );
        // A feeder of both sides alone: its both sides, duplex asked or not.
        let both = values(None, None, Some("Duplex ADF"));
        assert_eq!(
            pick(Source::Feeder, false, &both).as_deref(),
            Some("Duplex ADF")
        );
        // The first listed of a flatbed and a feeder, where the one asked isn't there.
        assert_eq!(
            pick(Source::Flatbed, false, &both).as_deref(),
            Some("Duplex ADF")
        );
        let glass = values(Some("FlatBed"), None, None);
        assert_eq!(
            pick(Source::Feeder, true, &glass).as_deref(),
            Some("FlatBed")
        );
        assert_eq!(pick(Source::Feeder, false, &values(None, None, None)), None);
    }

    #[test]
    fn a_source_value_read_back_is_the_source_it_names_or_the_one_set() {
        let read = |value: &str, set| source_of(&SaneString::from(value), set);
        assert_eq!(read("Flatbed", Source::Feeder), (Source::Flatbed, false));
        assert_eq!(read("ADF Front", Source::Flatbed), (Source::Feeder, false));
        assert_eq!(read("ADF Duplex", Source::Flatbed), (Source::Feeder, true));
        assert_eq!(read("ADF Back", Source::Feeder), (Source::Feeder, false));
        assert_eq!(read("ADF Back", Source::Flatbed), (Source::Flatbed, false));
    }

    #[test]
    fn a_format_lacking_becomes_its_sibling_else_the_first_offered() {
        use Format::*;
        assert_eq!(format_for(Rgb16, &[Bw1, Gray8, Rgb8]), Rgb8);
        assert_eq!(format_for(Rgb8, &[Bw1, Rgb16]), Rgb16);
        assert_eq!(format_for(Gray16, &[Bw1, Gray8]), Gray8);
        assert_eq!(format_for(Gray8, &[Bw1, Gray16]), Gray16);
        assert_eq!(format_for(Bw1, &[Gray8, Rgb8]), Gray8);
        // No sibling: the first offered.
        assert_eq!(format_for(Rgb16, &[Bw1, Gray8]), Bw1);
        assert_eq!(format_for(Bw1, &[Rgb8]), Rgb8);
        // Offered: as asked.
        assert_eq!(format_for(Gray16, &[Bw1, Gray8, Gray16]), Gray16);
    }

    #[test]
    fn each_format_s_mode_and_depth() {
        let test = ModeValues {
            lineart: None,
            gray: Some("Gray".into()),
            color: Some("Color".into()),
        };
        let pick = |format, modes: &ModeValues| {
            let (value, bits) = mode_for(format, modes);
            (value.map(|value| value.to_string()), bits)
        };
        assert_eq!(pick(Format::Bw1, &test), (Some("Gray".to_owned()), Some(1)));
        assert_eq!(
            pick(Format::Gray8, &test),
            (Some("Gray".to_owned()), Some(8))
        );
        assert_eq!(
            pick(Format::Gray16, &test),
            (Some("Gray".to_owned()), Some(16))
        );
        assert_eq!(
            pick(Format::Rgb8, &test),
            (Some("Color".to_owned()), Some(8))
        );
        assert_eq!(
            pick(Format::Rgb16, &test),
            (Some("Color".to_owned()), Some(16))
        );
        let lineart = ModeValues {
            lineart: Some("Lineart".into()),
            ..test
        };
        assert_eq!(
            pick(Format::Bw1, &lineart),
            (Some("Lineart".to_owned()), None)
        );
        // Read back, each names its format; color at 1 bit names none.
        assert_eq!(
            format_of(Some(ModeKind::Lineart), Some(8)),
            Some(Format::Bw1)
        );
        assert_eq!(format_of(Some(ModeKind::Gray), Some(1)), Some(Format::Bw1));
        assert_eq!(format_of(Some(ModeKind::Gray), None), Some(Format::Gray8));
        assert_eq!(
            format_of(Some(ModeKind::Gray), Some(16)),
            Some(Format::Gray16)
        );
        assert_eq!(format_of(Some(ModeKind::Color), None), Some(Format::Rgb8));
        assert_eq!(
            format_of(Some(ModeKind::Color), Some(16)),
            Some(Format::Rgb16)
        );
        assert_eq!(format_of(Some(ModeKind::Color), Some(1)), None);
        assert_eq!(format_of(Some(ModeKind::Gray), Some(12)), None);
        assert_eq!(format_of(None, Some(8)), None);
    }

    #[test]
    fn the_nearest_resolution_the_higher_of_two_as_near() {
        let list = Resolutions::List(vec![75, 150, 300, 600]);
        assert_eq!(nearest(&list, 225), 300);
        assert_eq!(nearest(&list, 224), 150);
        assert_eq!(nearest(&list, 0), 75);
        assert_eq!(nearest(&list, u32::MAX), 600);
        assert_eq!(nearest(&list, 600), 600);
        let step = |step| NonZeroU32::new(step).unwrap();
        // 50, 80, 110 to 100: 100 off the steps.
        let range = Resolutions::Range {
            min: 50,
            max: 100,
            step: step(30),
        };
        assert_eq!(nearest(&range, 65), 80);
        assert_eq!(nearest(&range, 64), 50);
        assert_eq!(nearest(&range, 100), 80);
        assert_eq!(nearest(&range, u32::MAX), 80);
        assert_eq!(nearest(&range, 0), 50);
        let every = Resolutions::Range {
            min: 1,
            max: u32::MAX,
            step: step(1),
        };
        assert_eq!(nearest(&every, u32::MAX), u32::MAX);
        assert_eq!(nearest(&every, 10), 10);
    }

    #[test]
    fn the_paper_cut_to_the_largest_area_and_auto_the_largest() {
        let largest = SizeMm {
            width: 200.0,
            height: 200.0,
        };
        let cut = paper_for(Paper::Size(SizeMm::LETTER), largest);
        assert_eq!((cut.width, cut.height), (200.0, 200.0));
        let small = SizeMm {
            width: 100.0,
            height: 250.0,
        };
        let one_way = paper_for(Paper::Size(small), largest);
        assert_eq!((one_way.width, one_way.height), (100.0, 200.0));
        assert_eq!(paper_for(Paper::Auto, largest), largest);
        for odd in [f32::NAN, f32::INFINITY, 0.0, -1.0] {
            let size = SizeMm {
                width: odd,
                height: 100.0,
            };
            assert_eq!(paper_for(Paper::Size(size), largest), largest, "{odd}");
        }
    }

    /// What a device that took everything asked would read back.
    fn as_asked(asked: &Settings) -> ReadBack {
        let area = match asked.paper {
            Paper::Size(size) => Some((f64::from(size.width), f64::from(size.height))),
            Paper::Auto => None,
        };
        ReadBack {
            source: asked.source,
            duplex: asked.duplex,
            format: asked.format,
            dpi: (f64::from(asked.dpi), f64::from(asked.dpi)),
            area,
        }
    }

    fn letter_feeder() -> Settings {
        let mut asked = settings(Source::Feeder, true);
        asked.paper = Paper::Size(SizeMm::LETTER);
        asked
    }

    #[test]
    fn applied_as_asked_says_nothing_changed_fixed_point_s_rounding_aside() {
        let asked = letter_feeder();
        let (applied, dpi) = applied_of(&asked, &as_asked(&asked)).unwrap();
        assert_eq!(applied, Applied::new(asked.clone(), Vec::new()));
        assert_eq!(dpi, (300.0, 300.0));
        // Letter as fixed point holds it, within 0.005 mm: the size asked.
        let mut read = as_asked(&asked);
        read.area = Some((sane_dl::Fixed::from_f64(215.9).unwrap().to_f64(), 279.404));
        let (applied, _) = applied_of(&asked, &read).unwrap();
        assert_eq!(applied.settings.paper, Paper::Size(SizeMm::LETTER));
        assert!(applied.clamped.is_empty());
        // Past 0.005 mm, a change, said.
        read.area = Some((215.9, 279.41));
        let (applied, _) = applied_of(&asked, &read).unwrap();
        assert_eq!(applied.clamped, [Clamped::Paper(asked.paper)]);
    }

    #[test]
    fn applied_says_each_change_with_what_was_asked() {
        let mut asked = letter_feeder();
        asked.encodings = vec![Encoding::Jpeg];
        let read = ReadBack {
            source: Source::Flatbed,
            duplex: false,
            format: Format::Rgb8,
            // A FIXED resolution's fraction, kept for the page and rounded for `Applied`.
            dpi: (149.6, 150.4),
            area: Some((200.0, 200.0)),
        };
        let (applied, dpi) = applied_of(&asked, &read).unwrap();
        let mut taken = asked.clone();
        taken.source = Source::Flatbed;
        taken.duplex = false;
        taken.dpi = 150;
        taken.format = Format::Rgb8;
        taken.paper = Paper::Size(SizeMm {
            width: 200.0,
            height: 200.0,
        });
        taken.encodings = Vec::new();
        assert_eq!(
            applied,
            Applied::new(
                taken,
                vec![
                    Clamped::Source(Source::Feeder),
                    Clamped::Duplex(true),
                    Clamped::Dpi(300),
                    Clamped::Format(Format::Gray8),
                    Clamped::Paper(Paper::Size(SizeMm::LETTER)),
                    Clamped::Encodings(vec![Encoding::Jpeg]),
                ]
            )
        );
        assert_eq!(dpi, (149.6, 150.4));
    }

    #[test]
    fn the_paper_asked_auto_is_the_area_read_and_a_size_with_no_area_options_auto() {
        let auto = settings(Source::Flatbed, false);
        let mut read = as_asked(&auto);
        read.area = Some((200.0, 200.0));
        let (applied, _) = applied_of(&auto, &read).unwrap();
        let area = Paper::Size(SizeMm {
            width: 200.0,
            height: 200.0,
        });
        assert_eq!(applied.settings.paper, area);
        assert_eq!(applied.clamped, [Clamped::Paper(Paper::Auto)]);
        // No area options: `Auto`, whatever was asked.
        let letter = letter_feeder();
        let mut read = as_asked(&letter);
        read.area = None;
        let (applied, _) = applied_of(&letter, &read).unwrap();
        assert_eq!(applied.settings.paper, Paper::Auto);
        assert_eq!(applied.clamped, [Clamped::Paper(letter.paper)]);
        let (applied, _) = applied_of(&auto, &as_asked(&auto)).unwrap();
        assert!(applied.clamped.is_empty());
    }

    #[test]
    fn a_resolution_or_an_area_no_page_can_have_is_the_driver_s_words() {
        let asked = letter_feeder();
        let refused = |read: ReadBack| match applied_of(&asked, &read) {
            Err(ScanError::Driver { message, shown }) => {
                assert!(!shown);
                message
            }
            other => panic!("{read:?} gave {other:?}"),
        };
        for dpi in [0.0, -300.0, 0.4, f64::NAN, f64::INFINITY, 4_294_967_296.0] {
            let mut read = as_asked(&asked);
            read.dpi = (dpi, 300.0);
            assert!(refused(read.clone()).contains("resolution"), "{dpi}");
            read.dpi = (300.0, dpi);
            assert!(refused(read).contains("resolution"), "{dpi} down");
        }
        // 0.5 rounds to 1 dpi; u32's greatest is whole.
        for dpi in [0.5, 4_294_967_295.0] {
            let mut read = as_asked(&asked);
            read.dpi = (dpi, dpi);
            assert!(applied_of(&asked, &read).is_ok(), "{dpi}");
        }
        for area in [(0.0, 279.4), (215.9, -1.0), (f64::NAN, 1.0), (1.0, 1e40)] {
            let mut read = as_asked(&asked);
            read.area = Some(area);
            assert!(refused(read).contains("scan area"), "{area:?}");
        }
    }
}
