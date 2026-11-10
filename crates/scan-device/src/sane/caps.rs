//! What a device can do, per source, read from its options' descriptors.

use sane_dl::{Handle, OptionDescriptor, SaneString, Value};

use super::error;
use super::options::{self, End, SourceValues};
use crate::{Caps, Format, Resolutions, ScanError, Source, SourceCaps};

/// The resolution taken where a device has no resolution option: 72 dpi, what image software
/// takes where a file gives none.
const NO_RESOLUTION_OPTION: u32 = 72;

/// What the device behind `handle` can do, read afresh from its descriptors.
///
/// A device with no source option (or one that can't be read now) is one source, a flatbed.
/// Otherwise each source the model names that the option offers is read by setting the option to
/// its value, since SANE's options change with the source, and the option is set back to the
/// value it was found at once they're read. A value already set isn't set again, and an option
/// with one value, or one software can't set, is never set (some backends refuse to be set to
/// the one value they offer), so only the source it's at is read. A
/// feeder is read through its one-sided value where it has one, its duplex said where it has a
/// value or an option for it. A source that offers no format or no resolution the model can name
/// is left out, and a device left with none is [`ScanError::Unsupported`].
pub(super) fn read(handle: &mut Handle) -> Result<Caps, ScanError> {
    let sources = match source_option(handle) {
        None => read_source(handle, Source::Flatbed)?.into_iter().collect(),
        Some(option) => read_sources(handle, &option)?,
    };
    if sources.is_empty() {
        return Err(ScanError::Unsupported);
    }
    Ok(Caps::new(sources))
}

/// The option that chooses the source, as far as caps need it.
struct SourceOption {
    /// Its name.
    name: String,
    /// The values it lists; none where it has no list.
    values: Vec<SaneString>,
    /// Whether software can set it now; where it can't, it's read as it is, as one with one
    /// value.
    settable: bool,
}

/// The device's source option, where it has one it can read now: a string option.
fn source_option(handle: &Handle) -> Option<SourceOption> {
    let option = options::SOURCE
        .iter()
        .find_map(|name| handle.find_option(name))?;
    let usable = option.value_type == sane_dl::ValueType::String && options::readable(option);
    usable.then(|| SourceOption {
        name: option.name.clone(),
        values: options::strings(option).to_vec(),
        settable: options::settable(option),
    })
}

/// Each source the source option offers, read as [`read`] says; the option set back after,
/// however the reading went.
fn read_sources(handle: &mut Handle, option: &SourceOption) -> Result<Vec<SourceCaps>, ScanError> {
    let found = get_string(handle, &option.name)?;
    let values = if option.values.is_empty() {
        vec![found.clone()]
    } else {
        option.values.clone()
    };
    let never_set = values.len() == 1 || !option.settable;
    let SourceValues {
        flatbed,
        feeder,
        duplex,
    } = options::source_values(&values);
    let has_duplex_value = duplex.is_some();
    let mut wanted: Vec<(Source, SaneString)> = Vec::new();
    if let Some(flatbed) = flatbed {
        wanted.push((Source::Flatbed, flatbed));
    }
    if let Some(feeder) = feeder.or(duplex) {
        wanted.push((Source::Feeder, feeder));
    }
    // The value found first, so it's read without a set.
    wanted.sort_by_key(|(_, value)| *value != found);
    let mut current = found.clone();
    let mut sources = Vec::new();
    let mut outcome = Ok(());
    for (source, value) in wanted {
        if value != current {
            if never_set {
                continue;
            }
            if let Err(error) = set_string(handle, &option.name, &value) {
                outcome = Err(error);
                break;
            }
            current = value;
        }
        match read_source(handle, source) {
            Ok(Some(mut caps)) => {
                if source == Source::Feeder {
                    caps.duplex =
                        has_duplex_value || options::duplex_switch(handle.options()).is_some();
                }
                sources.push(caps);
            }
            Ok(None) => {}
            Err(error) => {
                outcome = Err(error);
                break;
            }
        }
    }
    if current != found {
        let back = set_string(handle, &option.name, &found);
        outcome = outcome.and(back);
    }
    outcome?;
    sources.sort_by_key(|caps| caps.source != Source::Flatbed);
    Ok(sources)
}

/// What the source set now can do, read from the descriptors as they are; `None` where it offers
/// no format or no resolution the model can name.
fn read_source(handle: &mut Handle, source: Source) -> Result<Option<SourceCaps>, ScanError> {
    let formats = formats(handle)?;
    let resolution = options::RESOLUTION
        .iter()
        .find_map(|name| handle.find_option(name))
        .cloned();
    let area = Area::of(handle, source);
    // The resolution now, where something is read through it: an unconstrained resolution, or an
    // area in pixels.
    let needs_dpi = resolution
        .as_ref()
        .is_some_and(|option| option.constraint == sane_dl::Constraint::None)
        || area.in_pixels();
    let dpi = match &resolution {
        None => Some(f64::from(NO_RESOLUTION_OPTION)),
        Some(option) if needs_dpi && options::readable(option) => current_number(handle, option)?,
        Some(_) => None,
    };
    let resolutions = match &resolution {
        None => Some(Resolutions::List(vec![NO_RESOLUTION_OPTION])),
        Some(option) => options::resolutions(option, dpi),
    };
    let Some(resolutions) = resolutions else {
        return Ok(None);
    };
    if formats.is_empty() {
        return Ok(None);
    }
    let mut caps = SourceCaps::new(source, resolutions, formats);
    caps.max_area = area.largest(dpi);
    Ok(Some(caps))
}

/// The formats the source set now gives: from its modes and depths, or from its parameters
/// before a start where it has no mode option.
fn formats(handle: &mut Handle) -> Result<Vec<Format>, ScanError> {
    let Some(mode) = handle.find_option(options::MODE) else {
        let parameters = handle.parameters().map_err(|error| error::map(&error))?;
        return Ok(options::format_of(&parameters).into_iter().collect());
    };
    let modes = options::mode_values(options::strings(mode));
    let depths = handle.find_option(options::DEPTH).map(options::depths);
    Ok(options::formats(&modes, depths.as_deref()))
}

/// The options the largest area is read from.
struct Area {
    /// A feeder's page size, where both are active: its width and length.
    page: Option<(OptionDescriptor, OptionDescriptor)>,
    /// The scan area's corners, where the bottom right ones are active: top left (where there are
    /// such) and bottom right.
    corners: Option<(
        Option<OptionDescriptor>,
        Option<OptionDescriptor>,
        OptionDescriptor,
        OptionDescriptor,
    )>,
}

impl Area {
    /// The area options of the source set now.
    fn of(handle: &Handle, source: Source) -> Area {
        let active = |name: &str| {
            handle
                .find_option(name)
                .filter(|option| option.capabilities.is_active())
                .cloned()
        };
        let page = match (
            source,
            active(options::PAGE_WIDTH),
            active(options::PAGE_HEIGHT),
        ) {
            (Source::Feeder, Some(width), Some(height)) => Some((width, height)),
            _ => None,
        };
        let corners = match (active(options::BR_X), active(options::BR_Y)) {
            (Some(right), Some(bottom)) => {
                Some((active(options::TL_X), active(options::TL_Y), right, bottom))
            }
            _ => None,
        };
        Area { page, corners }
    }

    /// Whether a length is read in pixels, so through the resolution.
    fn in_pixels(&self) -> bool {
        let pixels = |option: &OptionDescriptor| option.unit == sane_dl::Unit::Pixel;
        match (&self.page, &self.corners) {
            (Some((width, height)), _) => pixels(width) || pixels(height),
            (None, Some((left, top, right, bottom))) => {
                pixels(right)
                    || pixels(bottom)
                    || left.as_ref().is_some_and(pixels)
                    || top.as_ref().is_some_and(pixels)
            }
            (None, None) => false,
        }
    }

    /// The largest area: a feeder's page size's greatest, else the bottom right corner's greatest
    /// less the top left's least (0 where there's no top left), lengths in pixels converted at
    /// `dpi`. `None` where there are no area options, or the area isn't finite and above 0.
    fn largest(&self, dpi: Option<f64>) -> Option<crate::SizeMm> {
        if let Some((width, height)) = &self.page {
            return options::area(
                options::length_mm(width, End::Max, dpi)?,
                options::length_mm(height, End::Max, dpi)?,
            );
        }
        let (left, top, right, bottom) = self.corners.as_ref()?;
        let least = |option: &Option<OptionDescriptor>| match option {
            Some(option) => options::length_mm(option, End::Min, dpi),
            None => Some(0.0),
        };
        options::area(
            options::length_mm(right, End::Max, dpi)? - least(left)?,
            options::length_mm(bottom, End::Max, dpi)? - least(top)?,
        )
    }
}

/// A numeric option's value now, its first word, as a number.
fn current_number(
    handle: &mut Handle,
    option: &OptionDescriptor,
) -> Result<Option<f64>, ScanError> {
    let value = handle
        .get(option.index)
        .map_err(|error| error::map(&error))?;
    Ok(match value {
        Value::Int(words) => words.first().map(|&word| f64::from(word)),
        Value::Fixed(words) => words.first().map(|word| word.to_f64()),
        _ => None,
    })
}

/// A string option's value now, found by its name.
fn get_string(handle: &mut Handle, name: &str) -> Result<SaneString, ScanError> {
    let index = index_of(handle, name)?;
    match handle.get(index).map_err(|error| error::map(&error))? {
        Value::String(value) => Ok(value),
        other => Err(ScanError::Driver {
            message: format!("the scanner's option {name:?} gave {other:?}, not a string"),
            shown: false,
        }),
    }
}

/// Sets a string option, found by its name, to `value`.
fn set_string(handle: &mut Handle, name: &str, value: &SaneString) -> Result<(), ScanError> {
    let index = index_of(handle, name)?;
    handle
        .set(index, &Value::String(value.clone()))
        .map(|_| ())
        .map_err(|error| error::map(&error))
}

/// The number of the option named `name`, which the device described a moment ago.
fn index_of(handle: &Handle, name: &str) -> Result<usize, ScanError> {
    handle
        .find_option(name)
        .map(|option| option.index)
        .ok_or_else(|| ScanError::Driver {
            message: format!("the scanner's option {name:?} is gone"),
            shown: false,
        })
}
