//! A batch's settings set on its source's functional unit, and read back for [`Applied`].
//!
//! On the source's unit, selected first where it isn't, in order: a feeder's duplex, the pixel
//! type then the bit depth, the resolution, the paper, a feeder's odd and even pages' orientation
//! (as read, so a back comes as the scanner reads it). Each value set is one the unit lists, or
//! one cut to what it says it holds: the format asked, else its sibling at the other depth, else
//! the first the unit's depths allow; the nearest resolution listed, the higher of two as near; a
//! flatbed's scan area from its top left corner, cut to its bed, in its own measurement unit; a
//! feeder's document type, the smallest it supports holding the size asked, else its largest
//! (never a custom size, which a feeder's module may not take). Neither unit finds the paper's
//! size itself, so [`Paper::Auto`] is the largest: the flatbed's bed, the feeder's largest type.
//!
//! Then the unit is read back whole, and [`Applied`] says what it holds, each difference from what
//! was asked a [`Clamped`]. Unverified on a device: whether a module applies a set before its
//! getter answers (the framework's objects may be local proxies); the read-back is what the
//! backend can say, and a page's bands say what came.

use std::cmp::Reverse;

use super::caps::{
    self, document_for, document_size, formats, largest_document, length_mm, max_area, mm_length,
    resolutions,
};
use super::driver::{
    DEPTH_1, DEPTH_8, DEPTH_16, ORIENTATION_AS_READ, PIXEL_BW, PIXEL_GRAY, PIXEL_RGB,
    UNIT_DOCUMENT_FEEDER, UNIT_FLATBED, UnitProps, UnitRect, UnitWrite,
};
use super::error::driver_msg;
use super::session::IcaDevice;
use crate::{Applied, Clamped, Format, Paper, Resolutions, ScanError, Settings, SizeMm, Source};

/// How far apart, in millimetres, a side read back may be from the side asked and still be the
/// size asked: a length converted to the unit's measurement unit and back.
const PAPER_TOLERANCE_MM: f32 = 0.05;

/// What a batch took: what it says in [`Applied`], and its pages' resolution.
pub(super) struct Taken {
    /// The settings taken, and what was changed.
    pub(super) applied: Applied,
    /// Each page's resolution, across and down.
    pub(super) dpi: f32,
}

/// `asked` set on `device`'s unit for its source, as the module's docs say, and read back.
pub(super) fn apply(device: &mut IcaDevice, asked: &Settings) -> Result<Taken, ScanError> {
    let types = device.unit_types()?;
    let wanted = caps::unit_type_of(asked.source);
    let unit_type = if types.contains(&wanted) {
        wanted
    } else {
        [UNIT_FLATBED, UNIT_DOCUMENT_FEEDER]
            .into_iter()
            .find(|unit_type| types.contains(unit_type))
            .ok_or(ScanError::Unsupported)?
    };
    let unit = caps::unit(device, unit_type)?;
    let write = to_write(&unit, asked)?;
    device.write_unit(&write)?;
    let read = device
        .selected_unit()?
        .filter(|read| read.unit_type == unit_type)
        .ok_or_else(|| {
            driver_msg(format!(
                "the scanner's {} wasn't selected as its settings were read back",
                caps::unit_name(unit_type)
            ))
        })?;
    let (applied, dpi) = applied_from(asked, &read)?;
    Ok(Taken { applied, dpi })
}

/// The pixel type and bit depth of a format.
fn pixel_of(format: Format) -> (u64, u64) {
    match format {
        Format::Bw1 => (PIXEL_BW, DEPTH_1),
        Format::Gray8 => (PIXEL_GRAY, DEPTH_8),
        Format::Gray16 => (PIXEL_GRAY, DEPTH_16),
        Format::Rgb8 => (PIXEL_RGB, DEPTH_8),
        Format::Rgb16 => (PIXEL_RGB, DEPTH_16),
    }
}

/// The format a unit's pixel type and bit depth make: `None` for one the model doesn't have.
fn format_of(pixel_type: u64, bit_depth: u64) -> Option<Format> {
    [
        Format::Bw1,
        Format::Gray8,
        Format::Gray16,
        Format::Rgb8,
        Format::Rgb16,
    ]
    .into_iter()
    .find(|&format| pixel_of(format) == (pixel_type, bit_depth))
}

/// The same format at the other depth, where it has one.
fn sibling(format: Format) -> Option<Format> {
    match format {
        Format::Gray8 => Some(Format::Gray16),
        Format::Gray16 => Some(Format::Gray8),
        Format::Rgb8 => Some(Format::Rgb16),
        Format::Rgb16 => Some(Format::Rgb8),
        Format::Bw1 => None,
    }
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
            // Never past `max`, itself a `u32`.
            u32::try_from(nearest).unwrap_or(u32::MAX)
        }
    }
}

/// What to set on `unit` for `asked`, from what it lists.
fn to_write(unit: &UnitProps, asked: &Settings) -> Result<UnitWrite, ScanError> {
    let feeder = unit.unit_type == UNIT_DOCUMENT_FEEDER;
    let listed = formats(&unit.bit_depths, unit.bit_depth);
    let format = [Some(asked.format), sibling(asked.format)]
        .into_iter()
        .flatten()
        .find(|format| listed.contains(format))
        .or_else(|| listed.first().copied())
        .ok_or(ScanError::Unsupported)?;
    let (pixel_type, bit_depth) = pixel_of(format);
    let dpi = resolutions(&unit.resolutions, unit.resolution)
        .map(|listed| nearest(&listed, asked.dpi))
        .ok_or(ScanError::Unsupported)?;
    let resolution = u64::from(dpi);
    let mut write = UnitWrite {
        duplex: feeder.then_some(asked.duplex && unit.duplex_supported),
        pixel_type,
        bit_depth,
        resolution,
        scan_area: None,
        document_type: None,
        orientation: feeder.then_some(ORIENTATION_AS_READ),
    };
    if feeder {
        write.document_type = match asked.paper {
            Paper::Size(size) => document_for(&unit.document_types, size),
            Paper::Auto => largest_document(&unit.document_types),
        }
        .map(|(document_type, _)| document_type);
    } else {
        write.scan_area = flatbed_area(unit, asked.paper, resolution);
    }
    Ok(write)
}

/// A flatbed's scan area for `paper`, from its top left corner, in its measurement unit at the
/// resolution set: the whole bed for [`Paper::Auto`], a size cut to the bed. `None` where its
/// lengths can't be converted (a measurement unit the backend doesn't know), and then it's left
/// as it is.
fn flatbed_area(unit: &UnitProps, paper: Paper, resolution: u64) -> Option<UnitRect> {
    let measure = unit.measurement_unit;
    let bed = max_area(unit.physical_size, measure, unit.resolution)?;
    let size = match paper {
        Paper::Size(size) => SizeMm {
            width: size.width.min(bed.width),
            height: size.height.min(bed.height),
        },
        _ => bed,
    };
    Some(UnitRect {
        x: 0.0,
        y: 0.0,
        width: mm_length(f64::from(size.width), measure, resolution)?,
        height: mm_length(f64::from(size.height), measure, resolution)?,
    })
}

/// The paper `read` holds: a flatbed's scan area, a feeder's document type's size (its largest
/// area where it's set to a type the backend states no size for); `None` where it can't be told.
fn paper_read(read: &UnitProps) -> Option<SizeMm> {
    let measure = read.measurement_unit;
    if read.unit_type == UNIT_DOCUMENT_FEEDER {
        return document_size(read.document_type)
            .or_else(|| max_area(read.physical_size, measure, read.resolution));
    }
    let side = |length: f64| {
        let mm = length_mm(length, measure, read.resolution)? as f32;
        (mm.is_finite() && mm > 0.0).then_some(mm)
    };
    Some(SizeMm {
        width: side(read.scan_area.width)?,
        height: side(read.scan_area.height)?,
    })
}

/// [`Applied`] from the unit `read` back, against what was `asked`, and the pages' resolution.
fn applied_from(asked: &Settings, read: &UnitProps) -> Result<(Applied, f32), ScanError> {
    let source = caps::source_of(read.unit_type).ok_or_else(|| {
        driver_msg(format!(
            "the scanner's selected unit reads as a {}",
            caps::unit_name(read.unit_type)
        ))
    })?;
    let dpi = u32::try_from(read.resolution)
        .ok()
        .filter(|&dpi| dpi > 0)
        .ok_or_else(|| {
            driver_msg(format!(
                "the scanner's {} reads back a resolution of {} dpi",
                caps::unit_name(read.unit_type),
                read.resolution
            ))
        })?;
    let format = format_of(read.pixel_type, read.bit_depth).ok_or_else(|| {
        driver_msg(format!(
            "the scanner's {} reads back pixel type {} at {} bits, a format the model doesn't have",
            caps::unit_name(read.unit_type),
            read.pixel_type,
            read.bit_depth
        ))
    })?;
    let duplex = source == Source::Feeder && read.duplex_enabled;

    let mut taken = asked.clone();
    let mut clamped = Vec::new();
    if source != asked.source {
        taken.source = source;
        clamped.push(Clamped::Source(asked.source));
    }
    if duplex != asked.duplex {
        taken.duplex = duplex;
        clamped.push(Clamped::Duplex(asked.duplex));
    }
    if dpi != asked.dpi {
        taken.dpi = dpi;
        clamped.push(Clamped::Dpi(asked.dpi));
    }
    if format != asked.format {
        taken.format = format;
        clamped.push(Clamped::Format(asked.format));
    }
    // Where the paper read can't be told, what was asked is said, unchanged.
    if let Some(size) = paper_read(read) {
        let same = match asked.paper {
            Paper::Size(asked) => {
                (size.width - asked.width).abs() <= PAPER_TOLERANCE_MM
                    && (size.height - asked.height).abs() <= PAPER_TOLERANCE_MM
            }
            _ => false,
        };
        if !same {
            taken.paper = Paper::Size(size);
            clamped.push(Clamped::Paper(asked.paper));
        }
    }
    if !asked.encodings.is_empty() {
        taken.encodings = Vec::new();
        clamped.push(Clamped::Encodings(asked.encodings.clone()));
    }
    Ok((Applied::new(taken, clamped), dpi as f32))
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU32;

    use super::super::driver::{
        DOCUMENT_A4, DOCUMENT_A5, DOCUMENT_US_LEGAL, DOCUMENT_US_LETTER, IndexSet,
        MEASURE_CENTIMETERS, MEASURE_INCHES, UnitSize,
    };
    use super::*;

    fn flatbed() -> UnitProps {
        UnitProps {
            unit_type: UNIT_FLATBED,
            pixel_type: PIXEL_GRAY,
            bit_depth: DEPTH_8,
            bit_depths: IndexSet::of(&[1, 8, 16]),
            measurement_unit: MEASURE_INCHES,
            resolution: 300,
            resolutions: IndexSet::of(&[75, 150, 300, 600]),
            physical_size: UnitSize {
                width: 8.5,
                height: 11.7,
            },
            scan_area: UnitRect {
                x: 0.0,
                y: 0.0,
                width: 8.5,
                height: 11.7,
            },
            ..UnitProps::default()
        }
    }

    fn feeder() -> UnitProps {
        UnitProps {
            unit_type: UNIT_DOCUMENT_FEEDER,
            bit_depths: IndexSet::of(&[8]),
            document_types: IndexSet::of(&[DOCUMENT_A4, DOCUMENT_US_LETTER, DOCUMENT_US_LEGAL]),
            document_type: DOCUMENT_A4,
            duplex_supported: true,
            ..flatbed()
        }
    }

    /// `unit` as a module that takes `write` as given would read it back.
    fn written(mut unit: UnitProps, write: &UnitWrite) -> UnitProps {
        if let Some(duplex) = write.duplex {
            unit.duplex_enabled = duplex;
        }
        unit.pixel_type = write.pixel_type;
        unit.bit_depth = write.bit_depth;
        unit.resolution = write.resolution;
        if let Some(area) = write.scan_area {
            unit.scan_area = area;
        }
        if let Some(document_type) = write.document_type {
            unit.document_type = document_type;
        }
        unit
    }

    #[test]
    fn a_range_s_and_a_list_s_nearest_is_the_higher_of_two_as_near() {
        let list = Resolutions::List(vec![300, 75, 600, 150]);
        assert_eq!(nearest(&list, 225), 300);
        assert_eq!(nearest(&list, 224), 150);
        assert_eq!(nearest(&list, 0), 75);
        assert_eq!(nearest(&list, u32::MAX), 600);
        let range = Resolutions::Range {
            min: 50,
            max: 1200,
            step: NonZeroU32::MIN,
        };
        assert_eq!(nearest(&range, 333), 333);
        assert_eq!(nearest(&range, 10), 50);
        assert_eq!(nearest(&range, 4800), 1200);
        let stepped = Resolutions::Range {
            min: 50,
            max: 100,
            step: NonZeroU32::new(30).expect("30"),
        };
        assert_eq!(nearest(&stepped, 65), 80);
        assert_eq!(nearest(&stepped, 100), 80);
    }

    #[test]
    fn a_flatbed_is_set_from_what_it_lists_and_said_as_read_back() {
        let unit = flatbed();
        let mut asked = Settings::new(Source::Flatbed, 200, Format::Rgb16);
        asked.paper = Paper::Size(SizeMm::LETTER);
        let write = to_write(&unit, &asked).expect("a write");
        assert_eq!(write.duplex, None);
        assert_eq!(write.orientation, None);
        assert_eq!(write.document_type, None);
        assert_eq!((write.pixel_type, write.bit_depth), (PIXEL_RGB, DEPTH_16));
        // 200 is 50 from 150 and 100 from 300.
        assert_eq!(write.resolution, 150);
        let area = write.scan_area.expect("an area");
        assert!((area.width - 8.5).abs() < 1e-5 && (area.height - 11.0).abs() < 1e-5);
        assert_eq!((area.x, area.y), (0.0, 0.0));

        let (applied, dpi) = applied_from(&asked, &written(unit.clone(), &write)).expect("applied");
        assert_eq!(applied.clamped, [Clamped::Dpi(200)]);
        assert_eq!(applied.settings.dpi, 150);
        assert_eq!(applied.settings.paper, Paper::Size(SizeMm::LETTER));
        assert_eq!(dpi, 150.0);

        // Larger than the bed: cut to it, and said.
        asked.paper = Paper::Size(SizeMm::LEGAL);
        let write = to_write(&unit, &asked).expect("a write");
        let area = write.scan_area.expect("an area");
        assert!((area.height - 11.7).abs() < 1e-5);
        let (applied, _) = applied_from(&asked, &written(unit.clone(), &write)).expect("applied");
        assert!(
            applied
                .clamped
                .contains(&Clamped::Paper(Paper::Size(SizeMm::LEGAL)))
        );

        // Auto: the whole bed, said as changed, since the unit doesn't find the paper.
        asked.paper = Paper::Auto;
        let write = to_write(&unit, &asked).expect("a write");
        let (applied, _) = applied_from(&asked, &written(unit, &write)).expect("applied");
        assert!(applied.clamped.contains(&Clamped::Paper(Paper::Auto)));
        let Paper::Size(bed) = applied.settings.paper else {
            panic!("a size");
        };
        assert!((bed.width - 215.9).abs() < 1e-3 && (bed.height - 297.18).abs() < 1e-3);
    }

    #[test]
    fn a_flatbed_s_area_is_in_its_own_measurement_unit() {
        let unit = UnitProps {
            measurement_unit: MEASURE_CENTIMETERS,
            physical_size: UnitSize {
                width: 21.59,
                height: 29.7,
            },
            ..flatbed()
        };
        let mut asked = Settings::new(Source::Flatbed, 300, Format::Gray8);
        asked.paper = Paper::Size(SizeMm::A4);
        let write = to_write(&unit, &asked).expect("a write");
        let area = write.scan_area.expect("an area");
        assert!((area.width - 21.0).abs() < 1e-6 && (area.height - 29.7).abs() < 1e-6);
        let (applied, _) = applied_from(&asked, &written(unit, &write)).expect("applied");
        assert!(applied.clamped.is_empty(), "{:?}", applied.clamped);
    }

    #[test]
    fn a_feeder_is_set_by_document_type_with_its_duplex_and_orientation() {
        let unit = feeder();
        let mut asked = Settings::new(Source::Feeder, 300, Format::Rgb8);
        asked.duplex = true;
        asked.paper = Paper::Size(SizeMm::LETTER);
        let write = to_write(&unit, &asked).expect("a write");
        assert_eq!(write.duplex, Some(true));
        assert_eq!(write.orientation, Some(ORIENTATION_AS_READ));
        assert_eq!(write.document_type, Some(DOCUMENT_US_LETTER));
        assert_eq!(write.scan_area, None);
        let (applied, _) = applied_from(&asked, &written(unit.clone(), &write)).expect("applied");
        assert!(applied.clamped.is_empty(), "{:?}", applied.clamped);
        assert!(applied.settings.duplex);

        // A size it has no type for: the smallest by area holding it (Letter, a little smaller
        // than A4), said as the type's size.
        asked.paper = Paper::Size(SizeMm {
            width: 148.0,
            height: 210.0,
        });
        let write = to_write(&unit, &asked).expect("a write");
        assert_eq!(write.document_type, Some(DOCUMENT_US_LETTER));
        let (applied, _) = applied_from(&asked, &written(unit.clone(), &write)).expect("applied");
        assert_eq!(applied.settings.paper, Paper::Size(SizeMm::LETTER));
        assert!(applied.clamped.contains(&Clamped::Paper(asked.paper)));

        // Auto: its largest type.
        asked.paper = Paper::Auto;
        let write = to_write(&unit, &asked).expect("a write");
        assert_eq!(write.document_type, Some(DOCUMENT_US_LEGAL));

        // Duplex where it has none: off, and said.
        let simplex = UnitProps {
            duplex_supported: false,
            document_types: IndexSet::of(&[DOCUMENT_A5]),
            ..feeder()
        };
        asked.paper = Paper::Size(SizeMm::A4);
        let write = to_write(&simplex, &asked).expect("a write");
        assert_eq!(write.duplex, Some(false));
        let (applied, _) = applied_from(&asked, &written(simplex, &write)).expect("applied");
        assert!(applied.clamped.contains(&Clamped::Duplex(true)));
        assert!(!applied.settings.duplex);
    }

    #[test]
    fn a_format_falls_back_to_its_sibling_then_the_first_listed() {
        let gray16_only = UnitProps {
            bit_depths: IndexSet::of(&[16]),
            ..flatbed()
        };
        let asked = Settings::new(Source::Flatbed, 300, Format::Gray8);
        let write = to_write(&gray16_only, &asked).expect("a write");
        assert_eq!((write.pixel_type, write.bit_depth), (PIXEL_GRAY, DEPTH_16));
        let bw_only = UnitProps {
            bit_depths: IndexSet::of(&[1]),
            ..flatbed()
        };
        let write = to_write(&bw_only, &asked).expect("a write");
        assert_eq!((write.pixel_type, write.bit_depth), (PIXEL_BW, DEPTH_1));
        let none = UnitProps {
            bit_depth: 4,
            bit_depths: IndexSet::of(&[4]),
            ..flatbed()
        };
        assert_eq!(to_write(&none, &asked), Err(ScanError::Unsupported));
    }

    #[test]
    fn applied_says_what_the_unit_holds_not_what_was_set() {
        let unit = flatbed();
        let asked = Settings::new(Source::Flatbed, 600, Format::Gray8);
        let write = to_write(&unit, &asked).expect("a write");
        assert_eq!(write.resolution, 600);
        // The module kept 300.
        let mut read = written(unit, &write);
        read.resolution = 300;
        let (applied, dpi) = applied_from(&asked, &read).expect("applied");
        assert_eq!(applied.settings.dpi, 300);
        assert_eq!(dpi, 300.0);
        assert!(applied.clamped.contains(&Clamped::Dpi(600)));

        // A read-back the model can't say is the driver's error.
        for bad in [
            UnitProps {
                resolution: 0,
                ..read.clone()
            },
            UnitProps {
                pixel_type: 5,
                ..read.clone()
            },
            UnitProps {
                unit_type: 1,
                ..read.clone()
            },
        ] {
            assert!(matches!(
                applied_err(&asked, &bad),
                Some(ScanError::Driver { shown: false, .. })
            ));
        }
    }

    fn applied_err(asked: &Settings, read: &UnitProps) -> Option<ScanError> {
        applied_from(asked, read).err()
    }

    #[test]
    fn encodings_asked_are_said_dropped() {
        let unit = flatbed();
        let mut asked = Settings::new(Source::Flatbed, 300, Format::Gray8);
        asked.paper = Paper::Size(SizeMm::LETTER);
        asked.encodings = vec![crate::Encoding::Jpeg];
        let write = to_write(&unit, &asked).expect("a write");
        let (applied, _) = applied_from(&asked, &written(unit, &write)).expect("applied");
        assert_eq!(
            applied.clamped,
            [Clamped::Encodings(vec![crate::Encoding::Jpeg])]
        );
        assert!(applied.settings.encodings.is_empty());
    }
}
