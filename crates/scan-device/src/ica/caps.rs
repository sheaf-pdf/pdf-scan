//! What a functional unit can do, from what the framework says of it: its resolutions from an
//! index set, its formats from its bit depths, its lengths in millimetres from its measurement
//! unit, and a feeder's document types with the sizes the backend states for them; and a
//! device's caps, each of its units selected in turn and read.
//!
//! A select's answer is matched by the unit type asked for, as scanline's is: the framework
//! selects a unit unasked as the device is instantiated, so an answer of another type (that one,
//! or a late answer of a select given up on) is never read as the answer. An answer with an error
//! is the answer, whatever unit it carries. One with no unit and no error (which the framework's
//! header says never comes, and scanline saw) can't be matched by its type, so the selected unit
//! is read then, and taken where it's of the type asked; otherwise the wait goes on.

use std::num::NonZeroU32;
use std::time::Instant;

use super::driver::{
    DEPTH_1, DEPTH_8, DEPTH_16, DOCUMENT_A3, DOCUMENT_A4, DOCUMENT_A5, DOCUMENT_A6,
    DOCUMENT_US_EXECUTIVE, DOCUMENT_US_LEDGER, DOCUMENT_US_LEGAL, DOCUMENT_US_LETTER,
    DOCUMENT_US_STATEMENT, IcaEvent, IndexSet, MEASURE_CENTIMETERS, MEASURE_INCHES, MEASURE_PICAS,
    MEASURE_PIXELS, MEASURE_POINTS, MEASURE_TWIPS, UNIT_DOCUMENT_FEEDER, UNIT_FLATBED, UnitProps,
    UnitSize,
};
use super::error::{driver_msg, scan_error};
use super::session::IcaDevice;
use crate::{Caps, Format, Resolutions, ScanError, SizeMm, Source, SourceCaps};

/// Millimetres an inch.
const MM_PER_INCH: f64 = 25.4;

/// How far, in millimetres, a document type may fall short of a size asked and still be taken
/// as holding it: a size asked in whole millimetres (216 by 279 for Letter's 215.9 by 279.4)
/// stays Letter.
const DOCUMENT_FIT_MM: f32 = 0.5;

/// The resolutions an index set lists (`supportedResolutions`), or `current` (the unit's
/// `resolution`) alone where it lists none: 0 and any past `u32` dropped; one contiguous run of
/// two or more a [`Resolutions::Range`] stepping by 1, however long; otherwise a
/// [`Resolutions::List`] of the indexes read. `None` where none is left, current included: the
/// unit is then left out of the device's sources.
pub(super) fn resolutions(set: &IndexSet, current: u64) -> Option<Resolutions> {
    let dpi = |value: u64| u32::try_from(value).ok().filter(|&dpi| dpi > 0);
    let contiguous =
        set.count >= 2 && set.last >= set.first && set.last - set.first == set.count - 1;
    if contiguous {
        let min = set.first.max(1);
        let max = set.last.min(u64::from(u32::MAX));
        if min < max {
            return Some(Resolutions::Range {
                // Both within 1..=u32::MAX: clamped to it above.
                min: dpi(min)?,
                max: dpi(max)?,
                step: NonZeroU32::MIN,
            });
        }
        // A run of 0 and 1 alone, or one past u32 but its first: what's left is one value, or
        // none, as the list below finds.
    }
    let mut list: Vec<u32> = set.indexes.iter().copied().filter_map(dpi).collect();
    list.dedup();
    if list.is_empty() {
        list.extend(dpi(current));
    }
    (!list.is_empty()).then_some(Resolutions::List(list))
}

/// The formats a unit's bit depths give (`supportedBitDepths`), or its `current` depth
/// (`bitDepth`) alone where it lists none: 1 bit black and white; 8 bits gray and color; 16 bits
/// gray and color, each a byte more a sample. The framework lists no pixel types beside its
/// depths, so these are what the depths allow, not what the unit says it gives: a batch reads
/// back what was set, and its pages come as their bands say. Empty where no depth is one of
/// these.
pub(super) fn formats(depths: &IndexSet, current: u64) -> Vec<Format> {
    let listed = |depth: u64| {
        if depths.count == 0 {
            depth == current
        } else {
            depths.indexes.contains(&depth)
        }
    };
    let mut formats = Vec::new();
    if listed(DEPTH_1) {
        formats.push(Format::Bw1);
    }
    if listed(DEPTH_8) {
        formats.push(Format::Gray8);
    }
    if listed(DEPTH_16) {
        formats.push(Format::Gray16);
    }
    if listed(DEPTH_8) {
        formats.push(Format::Rgb8);
    }
    if listed(DEPTH_16) {
        formats.push(Format::Rgb16);
    }
    formats
}

/// How many of a measurement unit make an inch; `None` for pixels, whose length is the
/// resolution's, and for a unit the backend doesn't know.
fn per_inch(unit: u64) -> Option<f64> {
    Some(match unit {
        MEASURE_INCHES => 1.0,
        MEASURE_CENTIMETERS => 2.54,
        MEASURE_PICAS => 6.0,
        MEASURE_POINTS => 72.0,
        MEASURE_TWIPS => 1440.0,
        _ => return None,
    })
}

/// How many of `unit` make an inch, with pixels at `dpi`; `None` for a unit the backend doesn't
/// know, or pixels at a resolution of 0.
fn units_per_inch(unit: u64, dpi: u64) -> Option<f64> {
    match unit {
        MEASURE_PIXELS => (dpi > 0).then_some(dpi as f64),
        _ => per_inch(unit),
    }
}

/// `length` in the unit's measurement unit (pixels at `dpi`) in millimetres: `None` where the
/// unit isn't known, or the length isn't finite or above 0, before or after.
pub(super) fn length_mm(length: f64, unit: u64, dpi: u64) -> Option<f64> {
    if !length.is_finite() || length <= 0.0 {
        return None;
    }
    let mm = length / units_per_inch(unit, dpi)? * MM_PER_INCH;
    (mm.is_finite() && mm > 0.0).then_some(mm)
}

/// `mm` in the unit's measurement unit (pixels at `dpi`), for an area set in it: `None` where the
/// unit isn't known, or the length isn't finite or above 0, before or after.
pub(super) fn mm_length(mm: f64, unit: u64, dpi: u64) -> Option<f64> {
    if !mm.is_finite() || mm <= 0.0 {
        return None;
    }
    let length = mm / MM_PER_INCH * units_per_inch(unit, dpi)?;
    (length.is_finite() && length > 0.0).then_some(length)
}

/// A unit's largest area (`physicalSize`) in millimetres: `None` where either length isn't
/// finite and above 0, in millimetres and as an `f32`, or the unit isn't known.
pub(super) fn max_area(size: UnitSize, unit: u64, dpi: u64) -> Option<SizeMm> {
    let side = |length: f64| {
        let mm = length_mm(length, unit, dpi)? as f32;
        (mm.is_finite() && mm > 0.0).then_some(mm)
    };
    Some(SizeMm {
        width: side(size.width)?,
        height: side(size.height)?,
    })
}

/// The document types whose sizes the backend states, portrait, width by height in millimetres:
/// a feeder is set to one of these. The B sizes are left out until a Mac shows which B the
/// framework's types mean, and the business card, whose size no standard fixes. Unverified on a
/// device: each is the paper's usual size, not one the framework's documentation gives.
pub(super) const DOCUMENT_TYPES: [(u64, SizeMm); 9] = [
    (DOCUMENT_A3, mm(297.0, 420.0)),
    (DOCUMENT_A4, SizeMm::A4),
    (DOCUMENT_A5, mm(148.0, 210.0)),
    (DOCUMENT_A6, mm(105.0, 148.0)),
    (DOCUMENT_US_LETTER, SizeMm::LETTER),
    (DOCUMENT_US_LEGAL, SizeMm::LEGAL),
    // 11 by 17 inches.
    (DOCUMENT_US_LEDGER, mm(279.4, 431.8)),
    // 7.25 by 10.5 inches.
    (DOCUMENT_US_EXECUTIVE, mm(184.15, 266.7)),
    // 5.5 by 8.5 inches.
    (DOCUMENT_US_STATEMENT, mm(139.7, 215.9)),
];

const fn mm(width: f32, height: f32) -> SizeMm {
    SizeMm { width, height }
}

/// The size the backend states for a document type, or `None` for one it doesn't.
pub(super) fn document_size(document_type: u64) -> Option<SizeMm> {
    DOCUMENT_TYPES
        .iter()
        .find(|&&(known, _)| known == document_type)
        .map(|&(_, size)| size)
}

/// A size's area, for ordering document types.
fn area(size: SizeMm) -> f32 {
    size.width * size.height
}

/// The document types a feeder supports (`supportedDocumentTypes`) whose sizes the backend
/// states, each with its size, in the table's order.
fn supported(types: &IndexSet) -> impl Iterator<Item = (u64, SizeMm)> + '_ {
    DOCUMENT_TYPES
        .iter()
        .copied()
        .filter(|(known, _)| types.indexes.contains(known))
}

/// A feeder's largest document type, by area, among those it supports whose sizes the backend
/// states: its largest area, and what [`Paper::Auto`](crate::Paper::Auto) sets. `None` where it
/// supports none of them.
pub(super) fn largest_document(types: &IndexSet) -> Option<(u64, SizeMm)> {
    supported(types).reduce(|best, candidate| {
        if area(candidate.1) > area(best.1) {
            candidate
        } else {
            best
        }
    })
}

/// The document type a feeder is set to for paper of `asked`: the smallest, by area, it supports
/// that holds the size (each side at least as long, within half a millimetre), else its largest.
/// `None` where it supports none whose size the backend states. A custom size is never set on a
/// feeder.
pub(super) fn document_for(types: &IndexSet, asked: SizeMm) -> Option<(u64, SizeMm)> {
    let holds = |size: SizeMm| {
        size.width + DOCUMENT_FIT_MM >= asked.width && size.height + DOCUMENT_FIT_MM >= asked.height
    };
    supported(types)
        .filter(|&(_, size)| holds(size))
        .reduce(|best, candidate| {
            if area(candidate.1) < area(best.1) {
                candidate
            } else {
                best
            }
        })
        .or_else(|| largest_document(types))
}

/// The model's source for a unit type: the flatbed and the document feeder; `None` for the
/// transparency units, which the model has no source for, and any type the backend doesn't know.
pub(super) fn source_of(unit_type: u64) -> Option<Source> {
    match unit_type {
        UNIT_FLATBED => Some(Source::Flatbed),
        UNIT_DOCUMENT_FEEDER => Some(Source::Feeder),
        _ => None,
    }
}

/// The unit type of a source.
pub(super) fn unit_type_of(source: Source) -> u64 {
    match source {
        Source::Flatbed => UNIT_FLATBED,
        Source::Feeder => UNIT_DOCUMENT_FEEDER,
    }
}

/// A unit's name, for a message.
pub(super) fn unit_name(unit_type: u64) -> String {
    match unit_type {
        UNIT_FLATBED => "flatbed".to_owned(),
        UNIT_DOCUMENT_FEEDER => "document feeder".to_owned(),
        other => format!("functional unit of type {other}"),
    }
}

/// What the unit `unit` read can do, as a source: `None` where the model has no source for it,
/// or it lists no resolution or no format the model has.
///
/// A feeder's duplex is its `supportsDuplexScanning`, and its largest area its largest document
/// type's size of those the backend states, since its paper is set by type (its `physicalSize`
/// where it supports none of them); a flatbed's largest area is its `physicalSize`. Neither finds
/// the paper's size itself, as far as the framework's API says, and both give raw pages alone.
pub(super) fn source_caps(unit: &UnitProps) -> Option<SourceCaps> {
    let source = source_of(unit.unit_type)?;
    let resolutions = resolutions(&unit.resolutions, unit.resolution)?;
    let formats = formats(&unit.bit_depths, unit.bit_depth);
    if formats.is_empty() {
        return None;
    }
    let physical = || max_area(unit.physical_size, unit.measurement_unit, unit.resolution);
    let mut caps = SourceCaps::new(source, resolutions, formats);
    caps.max_area = match source {
        Source::Feeder => largest_document(&unit.document_types)
            .map(|(_, size)| size)
            .or_else(physical),
        Source::Flatbed => physical(),
    };
    caps.duplex = source == Source::Feeder && unit.duplex_supported;
    Some(caps)
}

/// The unit of `unit_type` selected and read whole: its answer waited for at most
/// [`Bounds::answer`](super::driver::Bounds::answer) and matched by the type asked, as the
/// module's docs say. A select answered with an error is that error's [`ScanError`]; one never
/// answered, [`ScanError::Driver`].
pub(super) fn select(device: &mut IcaDevice, unit_type: u64) -> Result<UnitProps, ScanError> {
    device.request_select(unit_type)?;
    let bound = device.bounds.answer;
    let end = Instant::now().checked_add(bound);
    loop {
        let left = end.map_or(bound, |end| end.saturating_duration_since(Instant::now()));
        if left.is_zero() {
            return Err(driver_msg(format!(
                "the scanner didn't answer the select of its {} within {} s",
                unit_name(unit_type),
                bound.as_secs_f32()
            )));
        }
        match device.next(left)? {
            Some(IcaEvent::UnitSelected { unit, error, .. }) => {
                if let Some(error) = error {
                    return Err(scan_error(&error));
                }
                match unit {
                    Some(unit) if unit.unit_type == unit_type => return Ok(*unit),
                    // Another unit's: the one the framework selected unasked, or a late answer.
                    Some(_) => {}
                    None => {
                        if let Some(unit) = device.selected_unit()?
                            && unit.unit_type == unit_type
                        {
                            return Ok(unit);
                        }
                    }
                }
            }
            Some(IcaEvent::DeviceError { error, .. }) => return Err(scan_error(&error)),
            _ => {}
        }
    }
}

/// The unit of `unit_type` read whole: the selected one where it's of that type, else selected.
pub(super) fn unit(device: &mut IcaDevice, unit_type: u64) -> Result<UnitProps, ScanError> {
    match device.selected_unit()? {
        Some(unit) if unit.unit_type == unit_type => Ok(unit),
        _ => select(device, unit_type),
    }
}

/// What `device` can do: its flatbed and its document feeder, each selected in turn (the one
/// selected already read as it is) and read, then the one first selected selected again.
/// [`ScanError::Unsupported`] where it has neither, or neither lists what the model needs.
pub(super) fn read(device: &mut IcaDevice) -> Result<Caps, ScanError> {
    let first = device.selected_unit()?;
    let mut wanted = Vec::new();
    for unit_type in device.unit_types()? {
        if source_of(unit_type).is_some() && !wanted.contains(&unit_type) {
            wanted.push(unit_type);
        }
    }
    let first_type = first.as_ref().map(|unit| unit.unit_type);
    let mut selected = first_type;
    let mut sources = Vec::new();
    for unit_type in wanted {
        let unit = match &first {
            Some(first) if first.unit_type == unit_type => first.clone(),
            _ => {
                let unit = select(device, unit_type)?;
                selected = Some(unit_type);
                unit
            }
        };
        sources.extend(source_caps(&unit));
    }
    if let Some(first_type) = first_type
        && selected != Some(first_type)
    {
        select(device, first_type)?;
    }
    if sources.is_empty() {
        return Err(ScanError::Unsupported);
    }
    Ok(Caps::new(sources))
}

#[cfg(test)]
mod tests {
    use super::super::driver::DOCUMENT_DEFAULT;
    use super::*;

    fn set_of(indexes: &[u64]) -> IndexSet {
        IndexSet::of(indexes)
    }

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn a_contiguous_run_is_a_range_and_anything_else_a_list() {
        let run: Vec<u64> = (50..=1200).collect();
        assert_eq!(
            resolutions(&set_of(&run), 300),
            Some(Resolutions::Range {
                min: 50,
                max: 1200,
                step: NonZeroU32::MIN
            })
        );
        // A run longer than the indexes read is still one range, by its count, first and last.
        let long = IndexSet {
            count: 19_151,
            first: 50,
            last: 19_200,
            indexes: (50..50 + 4096).collect(),
        };
        assert_eq!(
            resolutions(&long, 300),
            Some(Resolutions::Range {
                min: 50,
                max: 19_200,
                step: NonZeroU32::MIN
            })
        );
        assert_eq!(
            resolutions(&set_of(&[75, 150, 300, 600, 1200]), 300),
            Some(Resolutions::List(vec![75, 150, 300, 600, 1200]))
        );
        // One resolution is a list of one.
        assert_eq!(
            resolutions(&set_of(&[300]), 150),
            Some(Resolutions::List(vec![300]))
        );
    }

    #[test]
    fn zero_and_past_u32_are_dropped_and_none_falls_back_to_the_current() {
        assert_eq!(
            resolutions(&set_of(&[0, 1, 2, 3]), 300),
            Some(Resolutions::Range {
                min: 1,
                max: 3,
                step: NonZeroU32::MIN
            })
        );
        assert_eq!(
            resolutions(&set_of(&[0, 1]), 300),
            Some(Resolutions::List(vec![1]))
        );
        let past = u64::from(u32::MAX) + 1;
        assert_eq!(
            resolutions(&set_of(&[0, 300, past]), 150),
            Some(Resolutions::List(vec![300]))
        );
        assert_eq!(
            resolutions(&set_of(&[u64::from(u32::MAX), past]), 150),
            Some(Resolutions::List(vec![u32::MAX]))
        );
        // A run reaching past u32 is cut at u32::MAX.
        let top = IndexSet {
            count: 3,
            first: u64::from(u32::MAX) - 1,
            last: past,
            indexes: vec![u64::from(u32::MAX) - 1, u64::from(u32::MAX), past],
        };
        assert_eq!(
            resolutions(&top, 150),
            Some(Resolutions::Range {
                min: u32::MAX - 1,
                max: u32::MAX,
                step: NonZeroU32::MIN
            })
        );
        // Nothing listed: the current resolution alone; that 0 too, nothing.
        assert_eq!(
            resolutions(&set_of(&[]), 300),
            Some(Resolutions::List(vec![300]))
        );
        assert_eq!(
            resolutions(&set_of(&[0]), 200),
            Some(Resolutions::List(vec![200]))
        );
        assert_eq!(resolutions(&set_of(&[]), 0), None);
        assert_eq!(resolutions(&set_of(&[0, past]), 0), None);
        // A count that lies about a run isn't taken as one.
        let lying = IndexSet {
            count: 3,
            first: 100,
            last: 50,
            indexes: vec![100],
        };
        assert_eq!(resolutions(&lying, 0), Some(Resolutions::List(vec![100])));
    }

    #[test]
    fn the_depths_give_the_formats() {
        assert_eq!(
            formats(&set_of(&[1, 8, 16]), 8),
            [
                Format::Bw1,
                Format::Gray8,
                Format::Gray16,
                Format::Rgb8,
                Format::Rgb16
            ]
        );
        assert_eq!(formats(&set_of(&[8]), 8), [Format::Gray8, Format::Rgb8]);
        assert_eq!(formats(&set_of(&[1]), 8), [Format::Bw1]);
        assert_eq!(formats(&set_of(&[16]), 8), [Format::Gray16, Format::Rgb16]);
        // Depths the model has no format for give none.
        assert_eq!(formats(&set_of(&[4, 12, 32]), 8), []);
        // None listed: the current depth's.
        assert_eq!(
            formats(&set_of(&[]), DEPTH_8),
            [Format::Gray8, Format::Rgb8]
        );
        assert_eq!(formats(&set_of(&[]), 0), []);
    }

    #[test]
    fn each_measurement_unit_converts_to_millimetres_and_back() {
        // 8.5 inches in each unit, the pixels at 300 dpi.
        for (unit, length) in [
            (MEASURE_INCHES, 8.5),
            (MEASURE_CENTIMETERS, 21.59),
            (MEASURE_PICAS, 51.0),
            (MEASURE_POINTS, 612.0),
            (MEASURE_TWIPS, 12_240.0),
            (MEASURE_PIXELS, 2550.0),
        ] {
            let mm = length_mm(length, unit, 300).expect("a length");
            assert!(close(mm, 215.9), "{unit}: {mm}");
            let back = mm_length(mm, unit, 300).expect("a length");
            assert!(close(back, length), "{unit}: {back}");
        }
    }

    #[test]
    fn an_unknown_unit_a_bad_length_or_pixels_without_a_resolution_is_none() {
        assert_eq!(length_mm(8.5, 6, 300), None);
        assert_eq!(mm_length(215.9, 6, 300), None);
        assert_eq!(length_mm(2550.0, MEASURE_PIXELS, 0), None);
        assert_eq!(mm_length(215.9, MEASURE_PIXELS, 0), None);
        for bad in [0.0, -1.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert_eq!(length_mm(bad, MEASURE_INCHES, 300), None, "{bad}");
            assert_eq!(mm_length(bad, MEASURE_INCHES, 300), None, "{bad}");
        }
        // Finite, but past f64 once converted.
        assert_eq!(length_mm(f64::MAX, MEASURE_INCHES, 300), None);
        assert_eq!(mm_length(f64::MAX, MEASURE_TWIPS, 300), None);
    }

    #[test]
    fn the_largest_area_is_both_sides_in_millimetres() {
        let letter = max_area(
            UnitSize {
                width: 8.5,
                height: 11.0,
            },
            MEASURE_INCHES,
            300,
        )
        .expect("an area");
        assert!((letter.width - 215.9).abs() < 1e-3);
        assert!((letter.height - 279.4).abs() < 1e-3);
        for bad in [
            UnitSize {
                width: 0.0,
                height: 11.0,
            },
            UnitSize {
                width: 8.5,
                height: f64::NAN,
            },
            // Finite in f64, past f32.
            UnitSize {
                width: 8.5,
                height: 1e300,
            },
        ] {
            assert_eq!(max_area(bad, MEASURE_INCHES, 300), None, "{bad:?}");
        }
        assert_eq!(
            max_area(
                UnitSize {
                    width: 8.5,
                    height: 11.0
                },
                99,
                300
            ),
            None
        );
    }

    #[test]
    fn each_document_type_has_its_size() {
        assert_eq!(document_size(DOCUMENT_A4), Some(SizeMm::A4));
        assert_eq!(document_size(DOCUMENT_US_LETTER), Some(SizeMm::LETTER));
        assert_eq!(document_size(DOCUMENT_US_LEGAL), Some(SizeMm::LEGAL));
        assert_eq!(document_size(DOCUMENT_A3), Some(mm(297.0, 420.0)));
        assert_eq!(document_size(DOCUMENT_A5), Some(mm(148.0, 210.0)));
        assert_eq!(document_size(DOCUMENT_A6), Some(mm(105.0, 148.0)));
        // The inch sizes, at 300 dpi.
        let at_300 = |mm: f32| (f64::from(mm) / 25.4 * 300.0).round();
        for (document_type, inches) in [
            (DOCUMENT_US_LEDGER, (3300.0, 5100.0)),
            (DOCUMENT_US_EXECUTIVE, (2175.0, 3150.0)),
            (DOCUMENT_US_STATEMENT, (1650.0, 2550.0)),
        ] {
            let size = document_size(document_type).expect("a size");
            assert_eq!((at_300(size.width), at_300(size.height)), inches);
        }
        // The default, a B size and the business card aren't stated.
        for unstated in [DOCUMENT_DEFAULT, 2, 6, 53] {
            assert_eq!(document_size(unstated), None, "{unstated}");
        }
        // Every row portrait, and each type once.
        for (i, &(document_type, size)) in DOCUMENT_TYPES.iter().enumerate() {
            assert!(size.width < size.height, "{document_type}");
            assert!(
                DOCUMENT_TYPES[i + 1..]
                    .iter()
                    .all(|&(other, _)| other != document_type)
            );
        }
    }

    #[test]
    fn a_unit_is_a_source_where_the_model_has_one_and_it_lists_enough() {
        let flatbed = UnitProps {
            unit_type: UNIT_FLATBED,
            bit_depth: DEPTH_8,
            bit_depths: set_of(&[1, 8]),
            measurement_unit: MEASURE_INCHES,
            resolution: 300,
            resolutions: set_of(&[75, 150, 300, 600]),
            physical_size: UnitSize {
                width: 8.5,
                height: 11.7,
            },
            ..UnitProps::default()
        };
        let caps = source_caps(&flatbed).expect("a source");
        assert_eq!(caps.source, Source::Flatbed);
        assert!(!caps.duplex && !caps.paper_auto && caps.encodings.is_empty());
        assert_eq!(caps.resolutions, Resolutions::List(vec![75, 150, 300, 600]));
        assert_eq!(caps.formats, [Format::Bw1, Format::Gray8, Format::Rgb8]);
        let area = caps.max_area.expect("an area");
        assert!((area.width - 215.9).abs() < 1e-3 && (area.height - 297.18).abs() < 1e-3);

        // A feeder's largest area is its largest document type's, and its duplex its own.
        let feeder = UnitProps {
            unit_type: UNIT_DOCUMENT_FEEDER,
            document_types: set_of(&[DOCUMENT_A4, DOCUMENT_US_LETTER]),
            duplex_supported: true,
            ..flatbed.clone()
        };
        let caps = source_caps(&feeder).expect("a source");
        assert_eq!(caps.source, Source::Feeder);
        assert!(caps.duplex);
        assert_eq!(caps.max_area, Some(SizeMm::A4));
        // Supporting no type the backend states, its physical size.
        let untyped = UnitProps {
            document_types: set_of(&[]),
            ..feeder.clone()
        };
        assert_eq!(
            source_caps(&untyped).and_then(|caps| caps.max_area),
            source_caps(&flatbed).and_then(|caps| caps.max_area)
        );

        // A flatbed's duplex is never said, whatever it reads.
        let odd = UnitProps {
            duplex_supported: true,
            ..flatbed.clone()
        };
        assert!(!source_caps(&odd).expect("a source").duplex);

        // A transparency unit, no resolution, or no format the model has: no source.
        for unit in [
            UnitProps {
                unit_type: 1,
                ..flatbed.clone()
            },
            UnitProps {
                resolution: 0,
                resolutions: set_of(&[]),
                ..flatbed.clone()
            },
            UnitProps {
                bit_depth: 4,
                bit_depths: set_of(&[4]),
                ..flatbed.clone()
            },
        ] {
            assert_eq!(source_caps(&unit), None, "{unit:?}");
        }
    }

    #[test]
    fn each_source_is_its_unit_type_and_back() {
        for source in [Source::Flatbed, Source::Feeder] {
            assert_eq!(source_of(unit_type_of(source)), Some(source));
        }
        assert_eq!(source_of(1), None);
        assert_eq!(source_of(2), None);
        assert_eq!(source_of(99), None);
        assert_eq!(unit_name(99), "functional unit of type 99");
    }

    #[test]
    fn a_feeder_takes_the_smallest_type_holding_the_paper_else_its_largest() {
        let office = set_of(&[
            DOCUMENT_DEFAULT,
            DOCUMENT_A4,
            DOCUMENT_US_LETTER,
            DOCUMENT_US_LEGAL,
            DOCUMENT_A5,
        ]);
        assert_eq!(
            largest_document(&office),
            Some((DOCUMENT_US_LEGAL, SizeMm::LEGAL))
        );
        // Each size it supports is its own.
        assert_eq!(
            document_for(&office, SizeMm::A4),
            Some((DOCUMENT_A4, SizeMm::A4))
        );
        assert_eq!(
            document_for(&office, SizeMm::LETTER),
            Some((DOCUMENT_US_LETTER, SizeMm::LETTER))
        );
        // Letter asked in whole millimetres is still Letter.
        assert_eq!(
            document_for(&office, mm(216.0, 279.0)),
            Some((DOCUMENT_US_LETTER, SizeMm::LETTER))
        );
        // A size between two types takes the smaller that holds it.
        assert_eq!(
            document_for(&office, mm(212.0, 290.0)),
            Some((DOCUMENT_US_LEGAL, SizeMm::LEGAL))
        );
        assert_eq!(
            document_for(&office, mm(100.0, 100.0)),
            Some((DOCUMENT_A5, mm(148.0, 210.0)))
        );
        // Larger than any: the largest.
        assert_eq!(
            document_for(&office, mm(297.0, 420.0)),
            Some((DOCUMENT_US_LEGAL, SizeMm::LEGAL))
        );
        // A feeder whose types the backend states none of.
        let unstated = set_of(&[DOCUMENT_DEFAULT, 2, 53]);
        assert_eq!(largest_document(&unstated), None);
        assert_eq!(document_for(&unstated, SizeMm::A4), None);
    }
}
