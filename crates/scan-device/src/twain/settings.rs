//! Ordered TWAIN capability negotiation, clamping, and readback for [`Event::Applied`](crate::Event::Applied).
//!
//! Each batch resets sticky driver processing flags, negotiates `ICAP_UNITS` and `ICAP_XFERMECH`,
//! then sets source, duplex, pixel format + bit depth + polarity, resolution, and scan area /
//! automatic border detection in dependency order before reading back what the source actually
//! holds.

use std::cmp::Reverse;

use twain_dsm::{Container, Fix32, Frame, Item, ItemType, ffi};

use super::caps::{
    self, CapSource, FeederCap, NO_RESOLUTION_DPI, PhysicalBounds, Units, container_allows_int,
    container_item_type, current_f64, current_int, item_as_f64, make_int_item, make_num_item, opt,
};
use super::error::driver_msg;
use super::pixels::Polarity;
use crate::{Applied, Clamped, Format, Paper, Resolutions, ScanError, Settings, SizeMm, Source};

/// Maximum difference in millimetres between a requested [`SizeMm`] and a readback `DAT_IMAGELAYOUT`
/// frame before reporting [`Clamped::Paper`], matching `SaneBackend` so `TW_FIX32` fixed-point
/// rounding (`1/65536` unit) isn't reported as clamping.
pub(super) const SAME_SIZE_MM: f64 = 0.005;

/// Transfer mechanism negotiated for a batch (`ICAP_XFERMECH`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum TransferMech {
    /// `TWSX_MEMORY` (`DAT_IMAGEMEMXFER` strips or tiles).
    Memory,
    /// `TWSX_NATIVE` (`DAT_IMAGENATIVEXFER` packed Windows DIB).
    Native,
}

/// Everything a batch needs once [`apply`] has negotiated and read back settings on a state-4
/// TWAIN source.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Taken {
    /// Settings taken by the source and which requested fields were clamped.
    pub applied: Applied,
    /// Transfer mechanism active for the batch.
    pub transfer: TransferMech,
    /// Active `ICAP_UNITS` after negotiation (used to convert `DAT_IMAGEINFO` resolutions too).
    pub units: Units,
    /// Active `ICAP_PIXELFLAVOR` polarity for 1/8/16-bit monochrome memory transfers.
    pub polarity: Polarity,
    /// Readback `(across, down)` resolution in dots per inch, preserving fractional DPI.
    pub dpi: (f32, f32),
}

/// Negotiates `asked` on `source` in dependency order, clamps unsupported values to what the
/// source offers, reads back the resulting state, and builds [`Taken`].
pub(super) fn apply(
    source: &mut dyn CapSource,
    asked: &Settings,
    force_native: bool,
) -> Result<Taken, ScanError> {
    reset_sticky_processing(source)?;
    let units = caps::negotiate_units(source)?;
    let transfer = negotiate_transfer(source, force_native)?;
    let allow_16bit = transfer == TransferMech::Memory;

    let source_now = negotiate_source(source, asked.source, allow_16bit, units)?;
    let duplex_now = negotiate_duplex(source, source_now, asked.duplex)?;
    let (format_now, polarity) = negotiate_format(source, asked.format, allow_16bit)?;
    let (across_dpi, down_dpi, whole_across_dpi) = negotiate_resolution(source, asked.dpi, units)?;
    let paper_now = negotiate_paper(source, asked.paper, units, across_dpi, down_dpi)?;

    let mut taken = asked.clone();
    let mut clamped = Vec::new();
    if source_now != asked.source {
        taken.source = source_now;
        clamped.push(Clamped::Source(asked.source));
    }
    if duplex_now != asked.duplex {
        taken.duplex = duplex_now;
        clamped.push(Clamped::Duplex(asked.duplex));
    }
    if whole_across_dpi != asked.dpi {
        taken.dpi = whole_across_dpi;
        clamped.push(Clamped::Dpi(asked.dpi));
    }
    if format_now != asked.format {
        taken.format = format_now;
        clamped.push(Clamped::Format(asked.format));
    }
    if paper_now != asked.paper {
        taken.paper = paper_now;
        clamped.push(Clamped::Paper(asked.paper));
    }
    if !asked.encodings.is_empty() {
        taken.encodings = Vec::new();
        clamped.push(Clamped::Encodings(asked.encodings.clone()));
    }

    Ok(Taken {
        applied: Applied::new(taken, clamped),
        transfer,
        units,
        polarity,
        dpi: (across_dpi as f32, down_dpi as f32),
    })
}

/// Resets sticky driver image-processing and transfer flags before negotiating batch settings.
///
/// Never sets `ICAP_BITORDER` (`Twain_DS_sample01` swaps RGB channel order in opposite directions
/// between native and memory transfers when `ICAP_BITORDER` is touched).
fn reset_sticky_processing(source: &mut dyn CapSource) -> Result<(), ScanError> {
    let _ = set_optional_int(
        source,
        ffi::ICAP_AUTODISCARDBLANKPAGES,
        i64::from(ffi::TWBP_DISABLE),
        ItemType::Int32,
    )?;
    let _ = set_optional_int(source, ffi::ICAP_AUTOMATICROTATE, 0, ItemType::Bool)?;
    let _ = set_optional_int(source, ffi::CAP_INDICATORS, 0, ItemType::Bool)?;
    let _ = set_optional_int(source, ffi::CAP_XFERCOUNT, -1, ItemType::Int16)?;
    let _ = set_optional_int(
        source,
        ffi::ICAP_COMPRESSION,
        i64::from(ffi::TWCP_NONE),
        ItemType::UInt16,
    )?;
    let _ = set_optional_int(
        source,
        ffi::ICAP_PLANARCHUNKY,
        i64::from(ffi::TWPC_CHUNKY),
        ItemType::UInt16,
    )?;
    Ok(())
}

/// Negotiates `ICAP_XFERMECH` (`TWSX_MEMORY` with fallback to `TWSX_NATIVE`, or `TWSX_NATIVE` when
/// `force_native` is `true`) and reads back the active mechanism.
fn negotiate_transfer(
    source: &mut dyn CapSource,
    force_native: bool,
) -> Result<TransferMech, ScanError> {
    let set_memory = !force_native
        && set_optional_int(
            source,
            ffi::ICAP_XFERMECH,
            i64::from(ffi::TWSX_MEMORY),
            ItemType::UInt16,
        )?;
    if !set_memory {
        let _ = set_optional_int(
            source,
            ffi::ICAP_XFERMECH,
            i64::from(ffi::TWSX_NATIVE),
            ItemType::UInt16,
        )?;
    }

    let raw = opt(source.get_current(ffi::ICAP_XFERMECH))?
        .or(opt(source.get(ffi::ICAP_XFERMECH))?)
        .and_then(|c| current_int(&c))
        .and_then(|v| u16::try_from(v).ok());

    match raw {
        None | Some(ffi::TWSX_NATIVE) => Ok(TransferMech::Native),
        Some(ffi::TWSX_MEMORY) if !force_native => Ok(TransferMech::Memory),
        Some(ffi::TWSX_MEMORY) => Err(ScanError::Unsupported),
        Some(other) => Err(driver_msg(format!(
            "the scanner held unsupported ICAP_XFERMECH {other:#06x}"
        ))),
    }
}

/// Negotiates `CAP_FEEDERENABLED` for `asked`, falling back to the other source if a selectable
/// device offers no usable formats/resolutions on the requested one.
fn negotiate_source(
    source: &mut dyn CapSource,
    asked: Source,
    allow_16bit: bool,
    units: Units,
) -> Result<Source, ScanError> {
    match caps::probe_feeder(source)? {
        FeederCap::Absent => Ok(Source::Flatbed),
        FeederCap::Fixed { feeder } => Ok(if feeder {
            Source::Feeder
        } else {
            Source::Flatbed
        }),
        FeederCap::Selectable {
            initial_feeder,
            item_type,
        } => {
            let want_feeder = asked == Source::Feeder;
            let mut active = initial_feeder;
            if want_feeder != active
                && let Some(item) = make_int_item(item_type, i64::from(want_feeder))
                && opt(source.set(ffi::CAP_FEEDERENABLED, item.into()))?.is_some()
            {
                active = read_bool_cap(source, ffi::CAP_FEEDERENABLED)?.unwrap_or(want_feeder);
            }
            if !source_is_usable(source, allow_16bit, units)? {
                let alt = !active;
                if let Some(item) = make_int_item(item_type, i64::from(alt))
                    && opt(source.set(ffi::CAP_FEEDERENABLED, item.into()))?.is_some()
                {
                    active = read_bool_cap(source, ffi::CAP_FEEDERENABLED)?.unwrap_or(alt);
                }
            }
            Ok(if active {
                Source::Feeder
            } else {
                Source::Flatbed
            })
        }
    }
}

/// Whether the currently selected source offers at least one format and one resolution.
fn source_is_usable(
    source: &mut dyn CapSource,
    allow_16bit: bool,
    units: Units,
) -> Result<bool, ScanError> {
    if caps::read_formats(source, allow_16bit)?.is_empty() {
        return Ok(false);
    }
    Ok(caps::read_resolutions(source, units)?.is_some())
}

/// Negotiates `CAP_DUPLEXENABLED`: enabled only when `source_now == Source::Feeder`, `asked` is
/// `true`, and `caps::duplex_supported` holds; explicitly set to `false` otherwise so sticky duplex
/// from an earlier scan is cleared.
fn negotiate_duplex(
    source: &mut dyn CapSource,
    source_now: Source,
    asked: bool,
) -> Result<bool, ScanError> {
    let want = if source_now == Source::Feeder && asked {
        caps::duplex_supported(source)?
    } else {
        false
    };
    let _ = set_optional_int(
        source,
        ffi::CAP_DUPLEXENABLED,
        i64::from(want),
        ItemType::Bool,
    )?;

    if source_now != Source::Feeder {
        return Ok(false);
    }
    let hw_duplex = opt(source.get_current(ffi::CAP_DUPLEX))?
        .and_then(|c| current_int(&c))
        .map(|dx| dx == i64::from(ffi::TWDX_1PASSDUPLEX) || dx == i64::from(ffi::TWDX_2PASSDUPLEX));
    if hw_duplex == Some(false) {
        return Ok(false);
    }
    Ok(read_bool_cap(source, ffi::CAP_DUPLEXENABLED)?.unwrap_or(false))
}

/// Negotiates `ICAP_PIXELTYPE`, `ICAP_BITDEPTH`, and `ICAP_PIXELFLAVOR`, returning the readback
/// [`Format`] and [`Polarity`].
fn negotiate_format(
    source: &mut dyn CapSource,
    asked: Format,
    allow_16bit: bool,
) -> Result<(Format, Polarity), ScanError> {
    let offered = caps::read_formats(source, allow_16bit)?;
    if offered.is_empty() {
        return Err(ScanError::Unsupported);
    }
    let target = format_for(asked, &offered);

    if let Some(pt_cont) = opt(source.get(ffi::ICAP_PIXELTYPE))? {
        let pt_type = container_item_type(&pt_cont);
        let want_pt = match target {
            Format::Bw1 => {
                if container_allows_int(&pt_cont, i64::from(ffi::TWPT_BW)) {
                    ffi::TWPT_BW
                } else {
                    ffi::TWPT_GRAY
                }
            }
            Format::Gray8 | Format::Gray16 => ffi::TWPT_GRAY,
            Format::Rgb8 | Format::Rgb16 => ffi::TWPT_RGB,
        };
        if current_int(&pt_cont) != Some(i64::from(want_pt))
            && let Some(item) = make_int_item(pt_type, i64::from(want_pt))
        {
            let _ = opt(source.set(ffi::ICAP_PIXELTYPE, item.into()))?;
        }
    }

    if let Some(bd_cont) = opt(source.get(ffi::ICAP_BITDEPTH))? {
        let bd_type = container_item_type(&bd_cont);
        let candidates: &[i64] = match target {
            Format::Bw1 => &[1],
            Format::Gray8 => &[8],
            Format::Gray16 => &[16],
            Format::Rgb8 => &[24, 8],
            Format::Rgb16 => &[48, 16],
        };
        let cur_bd = current_int(&bd_cont);
        if cur_bd.is_none_or(|cur| !candidates.contains(&cur)) {
            let chosen = match &bd_cont {
                Container::One(_) => Some(candidates[0]),
                _ => candidates
                    .iter()
                    .copied()
                    .find(|&c| container_allows_int(&bd_cont, c)),
            };
            if let Some(bits) = chosen
                && let Some(item) = make_int_item(bd_type, bits)
            {
                let _ = opt(source.set(ffi::ICAP_BITDEPTH, item.into()))?;
            }
        }
    }

    let format_now = read_back_format(source, allow_16bit)?;

    let polarity = if matches!(format_now, Format::Bw1 | Format::Gray8 | Format::Gray16) {
        let _ = set_optional_int(
            source,
            ffi::ICAP_PIXELFLAVOR,
            i64::from(ffi::TWPF_CHOCOLATE),
            ItemType::UInt16,
        )?;
        opt(source.get_current(ffi::ICAP_PIXELFLAVOR))?
            .or(opt(source.get(ffi::ICAP_PIXELFLAVOR))?)
            .and_then(|c| current_int(&c))
            .and_then(|v| u16::try_from(v).ok())
            .map_or(Polarity::ZeroBlack, Polarity::from_twpf)
    } else {
        Polarity::ZeroBlack
    };

    Ok((format_now, polarity))
}

/// Reads back `ICAP_PIXELTYPE` and `ICAP_BITDEPTH` and maps them to a model [`Format`].
fn read_back_format(source: &mut dyn CapSource, allow_16bit: bool) -> Result<Format, ScanError> {
    let pt = opt(source.get_current(ffi::ICAP_PIXELTYPE))?
        .or(opt(source.get(ffi::ICAP_PIXELTYPE))?)
        .and_then(|c| current_int(&c))
        .and_then(|v| u16::try_from(v).ok())
        .ok_or(ScanError::Unsupported)?;
    let depth = opt(source.get_current(ffi::ICAP_BITDEPTH))?
        .or(opt(source.get(ffi::ICAP_BITDEPTH))?)
        .and_then(|c| current_int(&c));

    match (pt, depth) {
        (ffi::TWPT_BW, None | Some(1)) => Ok(Format::Bw1),
        (ffi::TWPT_GRAY, Some(1)) => Ok(Format::Bw1),
        (ffi::TWPT_GRAY, None | Some(8)) => Ok(Format::Gray8),
        (ffi::TWPT_GRAY, Some(16)) if allow_16bit => Ok(Format::Gray16),
        (ffi::TWPT_RGB, None | Some(8 | 24)) => Ok(Format::Rgb8),
        (ffi::TWPT_RGB, Some(16 | 48)) if allow_16bit => Ok(Format::Rgb16),
        _ => Err(driver_msg(format!(
            "the scanner's pixel type ({pt:#06x}) and bit depth ({depth:?}) aren't a format this backend supports"
        ))),
    }
}

/// Chooses the format from `offered` (non-empty) for `asked`: `asked` if offered, else its
/// bit-depth / monochrome sibling, else `offered[0]` — identical to `SaneBackend`.
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

/// Negotiates `ICAP_XRESOLUTION` and `ICAP_YRESOLUTION`, returning `(across_dpi, down_dpi,
/// whole_across_dpi)` after readback.
fn negotiate_resolution(
    source: &mut dyn CapSource,
    asked_dpi: u32,
    units: Units,
) -> Result<(f64, f64, u32), ScanError> {
    if let Some(x_cont) = opt(source.get(ffi::ICAP_XRESOLUTION))? {
        let resolutions =
            caps::resolutions_from_container(&x_cont, units).ok_or(ScanError::Unsupported)?;
        let target_dpi = nearest(&resolutions, asked_dpi);
        set_resolution_cap(source, ffi::ICAP_XRESOLUTION, &x_cont, target_dpi, units)?;
        if let Some(y_cont) = opt(source.get(ffi::ICAP_YRESOLUTION))? {
            let y_target = caps::resolutions_from_container(&y_cont, units)
                .map_or(target_dpi, |r| nearest(&r, target_dpi));
            set_resolution_cap(source, ffi::ICAP_YRESOLUTION, &y_cont, y_target, units)?;
        }
    }

    let across_dpi = match opt(source.get_current(ffi::ICAP_XRESOLUTION))? {
        Some(c) => {
            let raw = current_f64(&c).ok_or_else(|| {
                driver_msg("the scanner's ICAP_XRESOLUTION held a non-numeric item")
            })?;
            if !raw.is_finite() || raw <= 0.0 {
                return Err(driver_msg(format!(
                    "the scanner's ICAP_XRESOLUTION read back as {raw}"
                )));
            }
            units.res_to_dpi(raw)
        }
        None => f64::from(NO_RESOLUTION_DPI),
    };
    let down_dpi = match opt(source.get_current(ffi::ICAP_YRESOLUTION))? {
        Some(c) => {
            let raw = current_f64(&c).ok_or_else(|| {
                driver_msg("the scanner's ICAP_YRESOLUTION held a non-numeric item")
            })?;
            if !raw.is_finite() || raw <= 0.0 {
                return Err(driver_msg(format!(
                    "the scanner's ICAP_YRESOLUTION read back as {raw}"
                )));
            }
            units.res_to_dpi(raw)
        }
        None => across_dpi,
    };

    let whole_across = whole_dpi(across_dpi)?;
    let _ = whole_dpi(down_dpi)?;
    Ok((across_dpi, down_dpi, whole_across))
}

/// Sets a single resolution capability (`ICAP_XRESOLUTION` or `ICAP_YRESOLUTION`).
///
/// For `Enumeration` / `Array`, picks the exact [`Item`] from the source's list whose converted
/// whole DPI is nearest `target_dpi` (breaking ties toward higher), avoiding `Fix32` ULP mismatches
/// on non-inch units. For `Range` / `One`, converts `target_dpi` via `units.dpi_to_res`.
fn set_resolution_cap(
    source: &mut dyn CapSource,
    cap: u16,
    container: &Container,
    target_dpi: u32,
    units: Units,
) -> Result<(), ScanError> {
    let chosen: Option<Item> = match container {
        Container::Enumeration(e) => pick_enum_resolution(e.items.as_slice(), target_dpi, units),
        Container::Array(items) => pick_enum_resolution(items.as_slice(), target_dpi, units),
        Container::Range(r) => {
            let mut want = units.dpi_to_res(f64::from(target_dpi));
            if let (Some(min), Some(max)) = (item_as_f64(r.min()), item_as_f64(r.max()))
                && min.is_finite()
                && max.is_finite()
                && min <= max
            {
                want = want.clamp(min, max);
            }
            make_num_item(r.item_type(), want)
        }
        Container::One(cur) => {
            make_num_item(cur.item_type(), units.dpi_to_res(f64::from(target_dpi)))
        }
    };

    if let Some(item) = chosen
        && caps::current_item(container) != Some(&item)
    {
        let _ = opt(source.set(cap, item.into()))?;
    }
    Ok(())
}

/// Picks the item in `items` whose rounded DPI in `units` is nearest `target_dpi` (higher on tie).
fn pick_enum_resolution(items: &[Item], target_dpi: u32, units: Units) -> Option<Item> {
    items
        .iter()
        .filter_map(|item| {
            let raw = item_as_f64(item)?;
            if !raw.is_finite() || raw <= 0.0 {
                return None;
            }
            let dpi = units.res_to_dpi(raw).round();
            if !dpi.is_finite() || !(1.0..=f64::from(u32::MAX)).contains(&dpi) {
                return None;
            }
            let whole = dpi as u32;
            Some(((whole.abs_diff(target_dpi), Reverse(whole)), item.clone()))
        })
        .min_by_key(|(score, _)| *score)
        .map(|(_, item)| item)
}

/// Nearest whole DPI in `resolutions` to `asked`, preferring the higher value on a tie.
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
            u32::try_from(nearest).unwrap_or(u32::MAX)
        }
    }
}

/// Validates and rounds a readback DPI to a positive `u32`.
fn whole_dpi(dpi: f64) -> Result<u32, ScanError> {
    let rounded = dpi.round();
    if dpi.is_finite() && dpi > 0.0 && (1.0..=f64::from(u32::MAX)).contains(&rounded) {
        Ok(rounded as u32)
    } else {
        Err(driver_msg(format!(
            "the scanner's resolution reads back as {dpi} dpi, which no page can have"
        )))
    }
}

/// Negotiates `Paper::Auto` vs `Paper::Size` with strict mutual exclusion between automatic border
/// detection / auto-size / undefined-image-size and explicit `DAT_IMAGELAYOUT` frames.
fn negotiate_paper(
    source: &mut dyn CapSource,
    asked: Paper,
    units: Units,
    across_dpi: f64,
    down_dpi: f64,
) -> Result<Paper, ScanError> {
    let auto_caps = caps::auto_paper_caps(source)?;
    let bounds = caps::read_physical_bounds(source)?;

    if asked == Paper::Auto && auto_caps.supported() {
        // Reset any leftover crop from a previous `Paper::Size` batch before enabling border
        // detection so the source scans its full window.
        if opt(source.reset_image_layout())?.is_none()
            && let Some(ref b) = bounds
            && let Some(frame) = frame_from_lengths(b.max_width, b.max_height, b)
        {
            let _ = opt(source.set_image_layout(frame))?;
        }
        if auto_caps.border_detection {
            let _ = set_optional_int(
                source,
                ffi::ICAP_AUTOMATICBORDERDETECTION,
                1,
                ItemType::Bool,
            )?;
        }
        if auto_caps.autosize {
            let _ = set_optional_int(
                source,
                ffi::ICAP_AUTOSIZE,
                i64::from(ffi::TWAS_AUTO),
                ItemType::UInt16,
            )?;
        }
        if auto_caps.undefined_size {
            let _ = set_optional_int(source, ffi::ICAP_UNDEFINEDIMAGESIZE, 1, ItemType::Bool)?;
        }

        let border_on = read_bool_cap(source, ffi::ICAP_AUTOMATICBORDERDETECTION)?.unwrap_or(false);
        let autosize_on = opt(source.get_current(ffi::ICAP_AUTOSIZE))?
            .and_then(|c| current_int(&c))
            .is_some_and(|v| v == i64::from(ffi::TWAS_AUTO));
        if border_on || autosize_on {
            return Ok(Paper::Auto);
        }
    }

    // Explicit `Paper::Size` (or `Paper::Auto` when unsupported / refused): turn off all sticky
    // automatic border/size/undefined-size capabilities first.
    let _ = set_optional_int(
        source,
        ffi::ICAP_AUTOMATICBORDERDETECTION,
        0,
        ItemType::Bool,
    )?;
    let _ = set_optional_int(
        source,
        ffi::ICAP_AUTOSIZE,
        i64::from(ffi::TWAS_NONE),
        ItemType::UInt16,
    )?;
    let _ = set_optional_int(source, ffi::ICAP_UNDEFINEDIMAGESIZE, 0, ItemType::Bool)?;

    if let Some(ref b) = bounds {
        let (want_w, want_h) = match asked {
            Paper::Size(sz) if is_valid_size(sz) => {
                let w_u = units
                    .mm_to_length(f64::from(sz.width), across_dpi)
                    .unwrap_or(b.max_width)
                    .clamp(b.min_width, b.max_width);
                let h_u = units
                    .mm_to_length(f64::from(sz.height), down_dpi)
                    .unwrap_or(b.max_height)
                    .clamp(b.min_height, b.max_height);
                (w_u, h_u)
            }
            _ => (b.max_width, b.max_height),
        };
        if let Some(frame) = frame_from_lengths(want_w, want_h, b) {
            let _ = opt(source.set_image_layout(frame))?;
        }
    }

    if let Some(frame) = opt(source.image_layout())? {
        let w_units = frame.right.to_f64() - frame.left.to_f64();
        let h_units = frame.bottom.to_f64() - frame.top.to_f64();
        let w_mm = units
            .length_to_mm(w_units, across_dpi)
            .ok_or_else(|| driver_msg("the scanner's DAT_IMAGELAYOUT width isn't finite"))?;
        let h_mm = units
            .length_to_mm(h_units, down_dpi)
            .ok_or_else(|| driver_msg("the scanner's DAT_IMAGELAYOUT height isn't finite"))?;
        return paper_from_read_mm(asked, w_mm, h_mm);
    }

    if let Some(ref b) = bounds
        && let (Some(w_mm), Some(h_mm)) = (
            units.length_to_mm(b.max_width, across_dpi),
            units.length_to_mm(b.max_height, down_dpi),
        )
    {
        return paper_from_read_mm(asked, w_mm, h_mm);
    }

    Ok(Paper::Auto)
}

/// Builds a top-left `(0, 0)` [`Frame`] from `(width, height)` in active `ICAP_UNITS`, clamping
/// `Fix32` coordinates directly against `bounds.max_width_fix` / `bounds.max_height_fix` so float
/// round-trips never overshoot `ICAP_PHYSICALWIDTH` / `ICAP_PHYSICALHEIGHT` by `1/65536` unit.
fn frame_from_lengths(width: f64, height: f64, bounds: &PhysicalBounds) -> Option<Frame> {
    let mut right = length_to_positive_fix32(width, bounds.max_width_fix)?;
    let mut bottom = length_to_positive_fix32(height, bounds.max_height_fix)?;
    if let Some(max_w) = bounds.max_width_fix
        && fix32_gt(right, max_w)
    {
        right = max_w;
    }
    if let Some(max_h) = bounds.max_height_fix
        && fix32_gt(bottom, max_h)
    {
        bottom = max_h;
    }
    if !fix32_gt(right, Fix32::from_int(0)) || !fix32_gt(bottom, Fix32::from_int(0)) {
        return None;
    }
    Some(Frame {
        left: Fix32::from_int(0),
        top: Fix32::from_int(0),
        right,
        bottom,
    })
}

/// Converts a positive `f64` length to [`Fix32`], falling back to `ceiling` (or maximum positive
/// `Fix32`) when `len` exceeds `32767.99998` (e.g. twips on a tall Legal page).
fn length_to_positive_fix32(len: f64, ceiling: Option<Fix32>) -> Option<Fix32> {
    if !len.is_finite() || len <= 0.0 {
        return None;
    }
    if let Some(fix) = Fix32::from_f64(len)
        && fix32_gt(fix, Fix32::from_int(0))
    {
        return Some(fix);
    }
    if len >= f64::from(i16::MAX) {
        return Some(ceiling.unwrap_or(Fix32 {
            whole: i16::MAX,
            frac: u16::MAX,
        }));
    }
    // Positive sub-1/65536 length: clamp up to 1/65536 so `Frame` stays strictly non-empty.
    Some(Fix32 { whole: 0, frac: 1 })
}

/// Exact signed fixed-point comparison `a > b`.
fn fix32_gt(a: Fix32, b: Fix32) -> bool {
    (a.whole, a.frac) > (b.whole, b.frac)
}

/// Validates readback dimensions `(w_mm, h_mm)` and compares them to `asked` within
/// [`SAME_SIZE_MM`].
fn paper_from_read_mm(asked: Paper, w_mm: f64, h_mm: f64) -> Result<Paper, ScanError> {
    let fits = |side: f64| side.is_finite() && side > 0.0 && side <= f64::from(f32::MAX);
    if !fits(w_mm) || !fits(h_mm) {
        return Err(driver_msg(format!(
            "the scanner's scan area reads back as {w_mm} by {h_mm} mm, which no page can be"
        )));
    }
    if let Paper::Size(sz) = asked
        && is_valid_size(sz)
        && (w_mm - f64::from(sz.width)).abs() <= SAME_SIZE_MM
        && (h_mm - f64::from(sz.height)).abs() <= SAME_SIZE_MM
    {
        return Ok(asked);
    }
    Ok(Paper::Size(SizeMm {
        width: w_mm as f32,
        height: h_mm as f32,
    }))
}

/// Whether `size` has finite positive width and height.
fn is_valid_size(size: SizeMm) -> bool {
    size.width.is_finite() && size.width > 0.0 && size.height.is_finite() && size.height > 0.0
}

/// Sets an optional integer/boolean capability to `value` when supported and allowed, leaving
/// already-matching capabilities untouched and propagating fatal hardware errors.
///
/// Returns `Ok(true)` if the capability already held `value` or accepted `MSG_SET`.
fn set_optional_int(
    source: &mut dyn CapSource,
    cap: u16,
    value: i64,
    default_type: ItemType,
) -> Result<bool, ScanError> {
    let Some(container) = opt(source.get(cap))? else {
        return Ok(false);
    };
    if current_int(&container) == Some(value) {
        return Ok(true);
    }
    if !matches!(container, Container::One(_)) && !container_allows_int(&container, value) {
        return Ok(false);
    }
    let item_type = container_item_type(&container);
    let item = make_int_item(item_type, value).or_else(|| make_int_item(default_type, value));
    let Some(item) = item else {
        return Ok(false);
    };
    Ok(opt(source.set(cap, item.into()))?.is_some())
}

/// Reads the current boolean state of `cap` (`Some(true)` for non-zero, `Some(false)` for `0`,
/// `None` if unsupported).
fn read_bool_cap(source: &mut dyn CapSource, cap: u16) -> Result<Option<bool>, ScanError> {
    Ok(opt(source.get_current(cap))?
        .or(opt(source.get(cap))?)
        .and_then(|c| current_int(&c))
        .map(|v| v != 0))
}

#[cfg(test)]
mod tests {
    use super::super::caps::test_mock::{MockCap, MockSource};
    use super::*;
    use crate::Encoding;

    #[test]
    fn resets_sticky_driver_processing_without_touching_bitorder() {
        let mut src = MockSource::sample();
        src.caps.insert(
            ffi::ICAP_AUTODISCARDBLANKPAGES,
            MockCap::one(Item::Int32(-1)),
        );
        src.caps
            .insert(ffi::ICAP_AUTOMATICROTATE, MockCap::one(Item::Bool(true)));
        src.caps
            .insert(ffi::CAP_INDICATORS, MockCap::one(Item::Bool(true)));
        src.caps
            .insert(ffi::CAP_XFERCOUNT, MockCap::one(Item::Int16(1)));
        src.caps.insert(
            ffi::ICAP_COMPRESSION,
            MockCap::enumeration(
                ItemType::UInt16,
                vec![Item::UInt16(ffi::TWCP_NONE), Item::UInt16(ffi::TWCP_JPEG)],
                1,
            ),
        );
        src.caps.insert(
            ffi::ICAP_PLANARCHUNKY,
            MockCap::enumeration(
                ItemType::UInt16,
                vec![
                    Item::UInt16(ffi::TWPC_CHUNKY),
                    Item::UInt16(ffi::TWPC_PLANAR),
                ],
                1,
            ),
        );
        src.caps
            .insert(ffi::ICAP_BITORDER, MockCap::one(Item::UInt16(0)));

        let asked = Settings::new(Source::Feeder, 300, Format::Rgb8);
        let taken = apply(&mut src, &asked, false).expect("apply succeeds");
        assert_eq!(taken.transfer, TransferMech::Memory);
        assert_eq!(
            current_int(&src.get_current(ffi::ICAP_AUTODISCARDBLANKPAGES).unwrap()),
            Some(i64::from(ffi::TWBP_DISABLE))
        );
        assert_eq!(
            current_int(&src.get_current(ffi::ICAP_AUTOMATICROTATE).unwrap()),
            Some(0)
        );
        assert_eq!(
            current_int(&src.get_current(ffi::CAP_INDICATORS).unwrap()),
            Some(0)
        );
        assert_eq!(
            current_int(&src.get_current(ffi::CAP_XFERCOUNT).unwrap()),
            Some(-1)
        );
        assert_eq!(
            current_int(&src.get_current(ffi::ICAP_COMPRESSION).unwrap()),
            Some(i64::from(ffi::TWCP_NONE))
        );
        assert_eq!(
            current_int(&src.get_current(ffi::ICAP_PLANARCHUNKY).unwrap()),
            Some(i64::from(ffi::TWPC_CHUNKY))
        );
        assert!(
            src.set_log
                .iter()
                .all(|(cap, _)| *cap != ffi::ICAP_BITORDER),
            "ICAP_BITORDER must never be modified"
        );
    }

    #[test]
    fn clamps_every_field_and_records_original_values() {
        let mut src = MockSource::sample();
        // Flatbed-only scanner, Gray8-only, 100/200/300 dpi, Letter max area, no auto paper.
        src.caps.remove(&ffi::CAP_FEEDERENABLED);
        src.caps.insert(
            ffi::ICAP_PIXELTYPE,
            MockCap::one_ro(Item::UInt16(ffi::TWPT_GRAY)),
        );
        src.caps
            .insert(ffi::ICAP_BITDEPTH, MockCap::one_ro(Item::UInt16(8)));
        src.caps.insert(
            ffi::ICAP_PHYSICALWIDTH,
            MockCap::one_ro(Item::Fix32(Fix32::from_f64(8.5).unwrap())),
        );
        src.caps.insert(
            ffi::ICAP_PHYSICALHEIGHT,
            MockCap::one_ro(Item::Fix32(Fix32::from_f64(11.0).unwrap())),
        );

        let mut asked = Settings::new(Source::Feeder, 250, Format::Rgb8);
        asked.duplex = true;
        asked.paper = Paper::Auto;
        asked.encodings = vec![Encoding::Jpeg];

        let taken = apply(&mut src, &asked, false).expect("clamped apply");
        assert_eq!(taken.applied.settings.source, Source::Flatbed);
        assert!(!taken.applied.settings.duplex);
        // 250 is equidistant between 200 and 300 -> prefers higher (300)!
        assert_eq!(taken.applied.settings.dpi, 300);
        assert_eq!(taken.applied.settings.format, Format::Gray8);
        assert_eq!(taken.applied.settings.paper, Paper::Size(SizeMm::LETTER));
        assert!(taken.applied.settings.encodings.is_empty());
        assert_eq!(
            taken.applied.clamped,
            vec![
                Clamped::Source(Source::Feeder),
                Clamped::Duplex(true),
                Clamped::Dpi(250),
                Clamped::Format(Format::Rgb8),
                Clamped::Paper(Paper::Auto),
                Clamped::Encodings(asked.encodings),
            ]
        );
    }

    #[test]
    fn native_transfer_clamps_16bit_formats_to_8bit_siblings_and_tracks_vanilla_polarity() {
        let mut src = MockSource::sample();
        src.caps.insert(
            ffi::ICAP_PIXELFLAVOR,
            MockCap::one_ro(Item::UInt16(ffi::TWPF_VANILLA)),
        );

        // Forced native transfer: Gray16 -> Gray8, Rgb16 -> Rgb8.
        let mut g16 = Settings::new(Source::Feeder, 200, Format::Gray16);
        g16.paper = Paper::Size(SizeMm::LETTER);
        let taken_g = apply(&mut src, &g16, true).expect("gray16 clamped");
        assert_eq!(taken_g.transfer, TransferMech::Native);
        assert_eq!(taken_g.applied.settings.format, Format::Gray8);
        assert_eq!(
            taken_g.applied.clamped,
            vec![Clamped::Format(Format::Gray16)]
        );
        assert_eq!(taken_g.polarity, Polarity::ZeroWhite);

        let mut rgb16 = Settings::new(Source::Feeder, 200, Format::Rgb16);
        rgb16.paper = Paper::Size(SizeMm::LETTER);
        let taken_rgb = apply(&mut src, &rgb16, true).expect("rgb16 clamped");
        assert_eq!(taken_rgb.applied.settings.format, Format::Rgb8);
        assert_eq!(
            taken_rgb.applied.clamped,
            vec![Clamped::Format(Format::Rgb16)]
        );
        // Polarity is never ZeroWhite on RGB!
        assert_eq!(taken_rgb.polarity, Polarity::ZeroBlack);
    }

    #[test]
    fn mutual_exclusion_between_paper_auto_and_paper_size_and_tolerance_and_invalid_size() {
        let mut src = MockSource::sample();
        src.caps.insert(
            ffi::ICAP_AUTOMATICBORDERDETECTION,
            MockCap::enumeration(ItemType::Bool, vec![Item::Bool(false), Item::Bool(true)], 0),
        );
        src.caps.insert(
            ffi::ICAP_UNDEFINEDIMAGESIZE,
            MockCap::enumeration(ItemType::Bool, vec![Item::Bool(false), Item::Bool(true)], 0),
        );

        // 1) Set custom `Paper::Size` (5 x 7 in = 127.0 x 177.8 mm).
        let custom = SizeMm {
            width: 127.0,
            height: 177.8,
        };
        let mut ask_size = Settings::new(Source::Flatbed, 300, Format::Rgb8);
        ask_size.paper = Paper::Size(custom);
        let t1 = apply(&mut src, &ask_size, false).unwrap();
        assert_eq!(t1.applied.settings.paper, Paper::Size(custom));
        assert!(t1.applied.clamped.is_empty());
        assert_eq!(
            current_int(&src.get_current(ffi::ICAP_AUTOMATICBORDERDETECTION).unwrap()),
            Some(0)
        );
        assert_eq!(
            current_int(&src.get_current(ffi::ICAP_UNDEFINEDIMAGESIZE).unwrap()),
            Some(0)
        );

        // 2) Switch to `Paper::Auto`: resets `DAT_IMAGELAYOUT` back to default and enables border
        //    detection + undefined image size.
        let mut ask_auto = Settings::new(Source::Feeder, 300, Format::Rgb8);
        ask_auto.paper = Paper::Auto;
        let t2 = apply(&mut src, &ask_auto, false).unwrap();
        assert_eq!(t2.applied.settings.paper, Paper::Auto);
        assert!(t2.applied.clamped.is_empty());
        assert_eq!(src.layout, src.default_layout);
        assert_eq!(
            current_int(&src.get_current(ffi::ICAP_AUTOMATICBORDERDETECTION).unwrap()),
            Some(1)
        );
        assert_eq!(
            current_int(&src.get_current(ffi::ICAP_UNDEFINEDIMAGESIZE).unwrap()),
            Some(1)
        );

        // 3) Switch back to `Paper::Size(SizeMm::A4)`: turns off sticky border detection &
        //    undefined image size, and keeps `SizeMm::A4` within the 0.005 mm `Fix32` tolerance!
        let mut ask_a4 = Settings::new(Source::Feeder, 300, Format::Rgb8);
        ask_a4.paper = Paper::Size(SizeMm::A4);
        let t3 = apply(&mut src, &ask_a4, false).unwrap();
        assert_eq!(t3.applied.settings.paper, Paper::Size(SizeMm::A4));
        assert!(t3.applied.clamped.is_empty());
        assert_eq!(
            current_int(&src.get_current(ffi::ICAP_AUTOMATICBORDERDETECTION).unwrap()),
            Some(0)
        );
        assert_eq!(
            current_int(&src.get_current(ffi::ICAP_UNDEFINEDIMAGESIZE).unwrap()),
            Some(0)
        );

        // 4) Non-positive or NaN `SizeMm` clamps cleanly to `max_area` (`SizeMm::LEGAL`).
        for bad in [
            SizeMm {
                width: 0.0,
                height: 100.0,
            },
            SizeMm {
                width: -10.0,
                height: 200.0,
            },
            SizeMm {
                width: f32::NAN,
                height: 200.0,
            },
        ] {
            let mut ask_bad = Settings::new(Source::Feeder, 300, Format::Rgb8);
            ask_bad.paper = Paper::Size(bad);
            let tb = apply(&mut src, &ask_bad, false).unwrap();
            assert_eq!(tb.applied.settings.paper, Paper::Size(SizeMm::LEGAL));
            assert_eq!(tb.applied.clamped.len(), 1);
            assert!(matches!(tb.applied.clamped[0], Clamped::Paper(_)));
        }
    }

    #[test]
    fn fix32_ceiling_never_overshoots_physical_bounds_by_one_ulp() {
        let mut src = MockSource::sample();
        // `215.9 / 25.4` in `Fix32` is `8.5` (`whole = 8, frac = 32768`), while an odd `Fix32`
        // ceiling like `whole = 8, frac = 19660` (`8.2999878 in = 210.81969 mm`) rounds up in `f32`
        // `SizeMm` to `210.8197 mm`, whose naive `Fix32::from_f64` would be `frac = 19661` (1 ULP
        // above `ICAP_PHYSICALWIDTH`!). Verify `frame_from_lengths` caps at `frac = 19660`.
        let tight = Fix32 {
            whole: 8,
            frac: 19660,
        };
        src.caps
            .insert(ffi::ICAP_PHYSICALWIDTH, MockCap::one_ro(Item::Fix32(tight)));
        let caps = caps::read(&mut src, false).unwrap();
        let max_area = caps.sources[0].max_area.unwrap();

        let mut ask = Settings::new(Source::Flatbed, 300, Format::Rgb8);
        ask.paper = Paper::Size(max_area);
        let taken = apply(&mut src, &ask, false).unwrap();
        assert!(taken.applied.clamped.is_empty());
        assert_eq!(src.layout.unwrap().right, tight);
    }

    #[test]
    fn fatal_hardware_error_during_negotiation_aborts_immediately() {
        let mut src = MockSource::sample();
        src.fatal_on = Some((ffi::ICAP_AUTOMATICROTATE, ScanError::CoverOpen));
        let asked = Settings::new(Source::Feeder, 300, Format::Rgb8);
        assert_eq!(apply(&mut src, &asked, false), Err(ScanError::CoverOpen));
    }
}
