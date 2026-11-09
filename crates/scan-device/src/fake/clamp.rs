//! A scripted device's settings, clamped to what its source can do.
//!
//! The fake's own rules (the module's docs, "A batch"), not the model's: a backend for a driver
//! API learns what its driver took by reading it back.

use std::cmp::Reverse;

use crate::{Applied, Caps, Clamped, Paper, Resolutions, Settings, SizeMm, SourceCaps};

use super::is_paper;

/// `asked` clamped to `caps`, and the source's caps it was clamped to. Any settings, however
/// wild, without a panic or an overflow.
///
/// `caps` hold a source, each with a format and a resolution, and a range's `min` is never past
/// its `max` (`FakeDevice::caps` checks them).
pub(super) fn clamp<'a>(caps: &'a Caps, asked: &Settings) -> (Applied, &'a SourceCaps) {
    let mut taken = asked.clone();
    let mut clamped = Vec::new();

    let source = match caps.sources.iter().find(|caps| caps.source == asked.source) {
        Some(source) => source,
        None => {
            let first = &caps.sources[0];
            taken.source = first.source;
            clamped.push(Clamped::Source(asked.source));
            first
        }
    };
    if asked.duplex && !source.duplex {
        taken.duplex = false;
        clamped.push(Clamped::Duplex(asked.duplex));
    }
    let dpi = nearest(&source.resolutions, asked.dpi);
    if dpi != asked.dpi {
        taken.dpi = dpi;
        clamped.push(Clamped::Dpi(asked.dpi));
    }
    if !source.formats.contains(&asked.format) {
        taken.format = source.formats[0];
        clamped.push(Clamped::Format(asked.format));
    }
    if let Some(paper) = paper(source, asked.paper) {
        taken.paper = paper;
        clamped.push(Clamped::Paper(asked.paper));
    }
    let offered: Vec<_> = asked
        .encodings
        .iter()
        .copied()
        .filter(|encoding| source.encodings.contains(encoding))
        .collect();
    if offered.len() != asked.encodings.len() {
        taken.encodings = offered;
        clamped.push(Clamped::Encodings(asked.encodings.clone()));
    }
    (Applied::new(taken, clamped), source)
}

/// The resolution nearest `asked` among `resolutions`, the higher of two as near.
fn nearest(resolutions: &Resolutions, asked: u32) -> u32 {
    match resolutions {
        Resolutions::List(values) => values
            .iter()
            .copied()
            .min_by_key(|&value| (value.abs_diff(asked), Reverse(value)))
            .unwrap_or(asked),
        Resolutions::Range { min, max, step } => nearest_step(*min, *max, step.get(), asked),
    }
}

/// The value nearest `asked` among `min`, `min + step`, ... not past `max`, the higher of two as
/// near; in `u64`, so no step overflows.
fn nearest_step(min: u32, max: u32, step: u32, asked: u32) -> u32 {
    let [min, max, step, asked] = [min, max, step, asked].map(u64::from);
    // The highest value on the steps that isn't past `max`.
    let last = min + max.saturating_sub(min) / step * step;
    let nearest = if asked <= min {
        min
    } else if asked >= last {
        last
    } else {
        // Both on the steps, and neither past `last`.
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

/// The paper `asked` becomes on `source`, where it changes.
fn paper(source: &SourceCaps, asked: Paper) -> Option<Paper> {
    match (asked, source.max_area) {
        (Paper::Size(_), None) => Some(Paper::Auto),
        (Paper::Size(size), Some(largest)) if !is_paper(size) => Some(Paper::Size(largest)),
        (Paper::Size(size), Some(largest)) => {
            let beyond = size.width > largest.width || size.height > largest.height;
            beyond.then_some(Paper::Size(SizeMm {
                width: size.width.min(largest.width),
                height: size.height.min(largest.height),
            }))
        }
        (Paper::Auto, Some(largest)) if !source.paper_auto => Some(Paper::Size(largest)),
        (Paper::Auto, _) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_range_s_nearest_step_is_never_past_its_max() {
        // 50, 80, 110: 100 is off the steps.
        assert_eq!(nearest_step(50, 100, 30, 100), 80);
        assert_eq!(nearest_step(50, 100, 30, 1000), 80);
        assert_eq!(nearest_step(50, 100, 30, u32::MAX), 80);
        assert_eq!(nearest_step(50, 100, 30, 64), 50);
        // 65 is as near 50 as 80: the higher.
        assert_eq!(nearest_step(50, 100, 30, 65), 80);
        assert_eq!(nearest_step(50, 100, 30, 66), 80);
        assert_eq!(nearest_step(50, 100, 30, 0), 50);
        assert_eq!(nearest_step(50, 50, 30, 75), 50);
        // Every value at the top of `u32` with no overflow.
        assert_eq!(nearest_step(1, u32::MAX, 1, u32::MAX), u32::MAX);
        assert_eq!(
            nearest_step(u32::MAX - 1, u32::MAX, u32::MAX, u32::MAX),
            u32::MAX - 1
        );
        assert_eq!(nearest_step(1, u32::MAX, u32::MAX, u32::MAX), 1);
        assert_eq!(nearest_step(1, u32::MAX, 2, u32::MAX), u32::MAX);
        assert_eq!(nearest_step(2, u32::MAX, 2, u32::MAX), u32::MAX - 1);
    }

    #[test]
    fn a_list_s_nearest_is_the_higher_of_two_as_near() {
        let list = Resolutions::List(vec![300, 75, 600, 150]);
        assert_eq!(nearest(&list, 225), 300);
        assert_eq!(nearest(&list, 224), 150);
        assert_eq!(nearest(&list, 0), 75);
        assert_eq!(nearest(&list, u32::MAX), 600);
        assert_eq!(nearest(&list, 150), 150);
    }
}
