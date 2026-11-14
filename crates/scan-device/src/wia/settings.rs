//! WDK dependency-ordered WIA 2.0 property negotiation, clamping, and readback into [`Applied`].
//!
//! Property write order follows the Windows Driver Kit minidriver validation contract (`DATATYPE`
//! before `DEPTH`, `TYMED` before `FORMAT`, `XRES` before `YRES`, `AUTO_CROP = 0` and
//! `PAGE_SIZE = WIA_PAGE_CUSTOM` and `XPOS`/`YPOS` before stepped `XEXTENT`/`YEXTENT`). Quirk
//! handling for WSD `WIA_IPS_PAGES` non-zero side counts and 0-page `TYMED_CALLBACK` ->
//! `TYMED_FILE` fallback is informed by the WDK documentation and permissive prior art
//! (`printscan-windows`, MIT OR Apache-2.0; `NAPS2.Wia`, MIT).

use std::cmp::Reverse;

use super::caps::{self, DiscoveredSource};
use super::driver::{
    ALL_PAGES, BACK_FIRST, BACK_ONLY, DUPLEX, FEEDER, FLATBED, FRONT_ONLY, FormatPair, GuidBytes,
    ItemId, PORTRAIT, PropAttr, PropConstraint, PropId, PropValue, TYMED_CALLBACK, TYMED_FILE,
    WIA_AUTO_CROP_DISABLED, WIA_AUTO_CROP_SINGLE, WIA_AUTO_DESKEW_OFF,
    WIA_BLANK_PAGE_DETECTION_DISABLED, WIA_COMPRESSION_NONE, WIA_DATA_COLOR, WIA_DATA_GRAYSCALE,
    WIA_DATA_RAW_BGR, WIA_DATA_RAW_RGB, WIA_DATA_THRESHOLD, WIA_FINAL_SCAN, WIA_IMG_FMT_BMP_GUID,
    WIA_IMG_FMT_MEMORYBMP_GUID, WIA_IMG_FMT_RAW_GUID, WIA_IPA_BITS_PER_CHANNEL,
    WIA_IPA_COMPRESSION, WIA_IPA_DATATYPE, WIA_IPA_DEPTH, WIA_IPA_FORMAT, WIA_IPA_PLANAR,
    WIA_IPA_TYMED, WIA_IPS_AUTO_CROP, WIA_IPS_AUTO_DESKEW, WIA_IPS_BLANK_PAGES,
    WIA_IPS_DOCUMENT_HANDLING_SELECT, WIA_IPS_OPTICAL_XRES, WIA_IPS_OPTICAL_YRES,
    WIA_IPS_PAGE_SIZE, WIA_IPS_PAGES, WIA_IPS_PHOTOMETRIC_INTERP, WIA_IPS_PREVIEW,
    WIA_IPS_ROTATION, WIA_IPS_XEXTENT, WIA_IPS_XPOS, WIA_IPS_XRES, WIA_IPS_YEXTENT, WIA_IPS_YPOS,
    WIA_IPS_YRES, WIA_PACKED_PIXEL, WIA_PAGE_AUTO, WIA_PAGE_CUSTOM, WIA_PHOTO_WHITE_1,
    WiaDeviceTree,
};
use super::error::{self, WiaOp};
use crate::{
    Applied, Clamped, Encoding, Format, Paper, Resolutions, ScanError, Settings, SizeMm, Source,
};

/// Maximum difference in millimetres between a requested [`SizeMm`] and the readback scan area
/// before reporting [`Clamped::Paper`], matching `SaneBackend` and `TwainBackend`.
pub(super) const SAME_SIZE_MM: f64 = 0.005;

/// How transferred stream bytes from `IWiaTransfer::Download` must be interpreted for each page.
///
/// Disambiguates `WiaImgFmt_BMP` used as an internal raw transfer container (`RawBmp` ->
/// `Pixels::Raw`) from `WiaImgFmt_BMP` explicitly requested via `Settings::encodings`
/// (`Encoded(Encoding::Bmp)` -> `Pixels::Encoded`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum TransferMode {
    /// `WiaImgFmt_BMP` (`14`-byte `BITMAPFILEHEADER` + DIB) decoded into `Pixels::Raw`.
    RawBmp,
    /// `WiaImgFmt_MEMORYBMP` (packed DIB, or `"BM"`-prefixed `.bmp`) decoded into `Pixels::Raw`.
    RawMemoryBmp,
    /// `WiaImgFmt_RAW` (`WIA_RAW_HEADER`) decoded into `Pixels::Raw`.
    RawWia,
    /// `TYMED_FILE` stream validated by magic header and delivered as `Pixels::Encoded`.
    Encoded(Encoding),
}

/// Negotiated WIA batch configuration returned by [`apply`].
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Taken {
    /// Settings taken by the scanner and which fields were clamped from what was asked.
    pub applied: Applied,
    /// Selected WIA child item on which `IWiaTransfer::Download` runs.
    pub item: ItemId,
    /// Item holding `WIA_IPS_DOCUMENT_HANDLING_SELECT` / feeder commands (`item` or `ROOT`).
    pub handling_item: ItemId,
    /// Stream decoding / pass-through mode for this batch.
    pub transfer_mode: TransferMode,
    /// Active `WIA_IPA_TYMED` (`TYMED_FILE` or `TYMED_CALLBACK`).
    pub tymed: i32,
    /// Whether duplex sheets arrive back-side first (`BACK_FIRST` set on handling select).
    pub back_first: bool,
    /// Non-zero `WIA_IPS_PAGES` side cap per `Download` call when the feeder rejected `0`.
    pub pages_per_download: Option<u32>,
    /// Readback `(across, down)` resolution in dots per inch (`> 0.0`, finite).
    pub dpi: (f32, f32),
}

/// Negotiates `asked` on `tree` in strict WDK dependency order, reads back the active properties,
/// and constructs [`Taken`].
pub(super) fn apply(tree: &mut dyn WiaDeviceTree, asked: &Settings) -> Result<Taken, ScanError> {
    let discovered = caps::discover_usable_sources(tree)?;
    if discovered.is_empty() {
        return Err(ScanError::Unsupported);
    }

    let disc = discovered
        .iter()
        .find(|d| d.caps.source == asked.source)
        .unwrap_or(&discovered[0])
        .clone();
    let source_now = disc.caps.source;
    let item = disc.item;
    let handling_item = disc.handling_item;

    // 1. Sticky image-processing resets + source/duplex/pages handling.
    reset_sticky_properties(tree, item)?;
    let (duplex_now, back_first) = negotiate_handling_select(tree, &disc, asked.duplex)?;
    let pages_per_download = negotiate_pages(tree, item, handling_item, source_now, duplex_now)?;

    // 2. Pixel datatype -> re-query depth constraint -> write depth + monochrome photometric.
    let allow_16bit = caps::supports_raw_transfer(&disc.transfer_pairs);
    let format_now = negotiate_format(tree, item, &disc.caps.formats, asked.format, allow_16bit)?;

    // 3. Transfer medium (`WIA_IPA_TYMED`) -> transfer format (`WIA_IPA_FORMAT`).
    let (transfer_mode, tymed, encodings_now) =
        negotiate_transfer_format(tree, item, format_now, &asked.encodings)?;

    // 4. Horizontal resolution (`WIA_IPS_XRES`) -> re-query vertical (`WIA_IPS_YRES`).
    let (across_dpi, down_dpi, whole_across_dpi) = negotiate_resolution(tree, item, asked.dpi)?;

    // 5. `Paper::Auto` vs `Paper::Size` (`AUTO_CROP`, `PAGE_SIZE`, `XPOS`, `YPOS`, `XEXTENT`,
    //    `YEXTENT`) after resolution is finalized so pixel extents match the active DPI.
    let paper_now = negotiate_paper(tree, item, source_now, asked.paper, across_dpi, down_dpi)?;

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
    if encodings_now != asked.encodings {
        taken.encodings = encodings_now;
        clamped.push(Clamped::Encodings(asked.encodings.clone()));
    }

    Ok(Taken {
        applied: Applied::new(taken, clamped),
        item,
        handling_item,
        transfer_mode,
        tymed,
        back_first,
        pages_per_download,
        dpi: (across_dpi as f32, down_dpi as f32),
    })
}

/// Switches a raw `TYMED_CALLBACK` batch over to `TYMED_FILE` (`WiaImgFmt_BMP` or `WiaImgFmt_RAW`)
/// when the first `Download` call refused `TYMED_CALLBACK` before delivering any page.
///
/// Returns `Ok(true)` if the switch succeeded and `taken` was updated in place, `Ok(false)` if no
/// `TYMED_FILE` raw fallback is available on this item.
#[cfg_attr(not(test), allow(dead_code))]
pub(super) fn fallback_callback_to_file(
    tree: &mut dyn WiaDeviceTree,
    taken: &mut Taken,
) -> Result<bool, ScanError> {
    if taken.tymed != TYMED_CALLBACK || matches!(taken.transfer_mode, TransferMode::Encoded(_)) {
        return Ok(false);
    }
    let pairs = caps::collect_transfer_pairs(tree, taken.item)?;
    let is_16bit = matches!(
        taken.applied.settings.format,
        Format::Gray16 | Format::Rgb16
    );
    let candidates: &[(GuidBytes, TransferMode)] = if is_16bit {
        &[(WIA_IMG_FMT_RAW_GUID, TransferMode::RawWia)]
    } else {
        &[
            (WIA_IMG_FMT_BMP_GUID, TransferMode::RawBmp),
            (WIA_IMG_FMT_RAW_GUID, TransferMode::RawWia),
            (WIA_IMG_FMT_MEMORYBMP_GUID, TransferMode::RawMemoryBmp),
        ]
    };

    for &(guid, mode) in candidates {
        let supported = pairs
            .iter()
            .any(|p| p.tymed == TYMED_FILE && p.format == guid);
        if !supported {
            continue;
        }
        if try_write_prop(tree, taken.item, WIA_IPA_TYMED, &PropValue::I4(TYMED_FILE))?
            && try_write_prop(tree, taken.item, WIA_IPA_FORMAT, &PropValue::Guid(guid))?
        {
            taken.tymed = TYMED_FILE;
            taken.transfer_mode = mode;
            return Ok(true);
        }
    }
    Ok(false)
}

fn try_write_prop(
    tree: &mut dyn WiaDeviceTree,
    item: ItemId,
    id: PropId,
    val: &PropValue,
) -> Result<bool, ScanError> {
    match tree.write_prop(item, id, val) {
        Ok(()) => Ok(true),
        Err(hr) => {
            let mapped = error::hresult(WiaOp::Settings, hr);
            if error::is_fatal_scan_error(&mapped) {
                Err(mapped)
            } else {
                Ok(false)
            }
        }
    }
}

fn write_optional_i4(
    tree: &mut dyn WiaDeviceTree,
    item: ItemId,
    id: PropId,
    target: i32,
) -> Result<bool, ScanError> {
    let attrs = tree.prop_attrs(item, &[id])?;
    let Some(Some(attr)) = attrs.first() else {
        return Ok(false);
    };
    if !attr.is_writable() || !caps::constraint_allows_i4(&attr.constraint, target, true) {
        return Ok(false);
    }
    try_write_prop(tree, item, id, &PropValue::I4(target))
}

fn read_i4(
    tree: &mut dyn WiaDeviceTree,
    item: ItemId,
    id: PropId,
) -> Result<Option<i32>, ScanError> {
    let vals = tree.read_props(item, &[id])?;
    Ok(vals
        .first()
        .and_then(|v| v.as_ref())
        .and_then(PropValue::as_i4))
}

fn reset_sticky_properties(tree: &mut dyn WiaDeviceTree, item: ItemId) -> Result<(), ScanError> {
    for (prop, val) in [
        (WIA_IPS_BLANK_PAGES, WIA_BLANK_PAGE_DETECTION_DISABLED),
        (WIA_IPS_AUTO_DESKEW, WIA_AUTO_DESKEW_OFF),
        (WIA_IPS_ROTATION, PORTRAIT),
        (WIA_IPS_PREVIEW, WIA_FINAL_SCAN),
        (WIA_IPA_COMPRESSION, WIA_COMPRESSION_NONE),
        (WIA_IPA_PLANAR, WIA_PACKED_PIXEL),
    ] {
        let _ = write_optional_i4(tree, item, prop, val)?;
    }
    // Some legacy WIA minidrivers keep `WIA_IPS_PREVIEW` on the root item.
    let _ = write_optional_i4(tree, ItemId::ROOT, WIA_IPS_PREVIEW, WIA_FINAL_SCAN)?;
    Ok(())
}

fn negotiate_handling_select(
    tree: &mut dyn WiaDeviceTree,
    disc: &DiscoveredSource,
    asked_duplex: bool,
) -> Result<(bool, bool), ScanError> {
    let target_item = disc.handling_item;
    let vals = tree.read_props(target_item, &[WIA_IPS_DOCUMENT_HANDLING_SELECT])?;
    let attrs = tree.prop_attrs(target_item, &[WIA_IPS_DOCUMENT_HANDLING_SELECT])?;
    let cur = vals
        .first()
        .and_then(|v| v.as_ref())
        .and_then(PropValue::as_i4);
    let attr = attrs.first().and_then(|a| a.as_ref());

    if disc.caps.source == Source::Flatbed {
        if let Some(attr) = attr
            && attr.is_writable()
        {
            for cand in [FLATBED, 0] {
                if caps::constraint_allows_i4(&attr.constraint, cand, cand == FLATBED)
                    && try_write_prop(
                        tree,
                        target_item,
                        WIA_IPS_DOCUMENT_HANDLING_SELECT,
                        &PropValue::I4(cand),
                    )?
                {
                    break;
                }
            }
        }
        return Ok((false, false));
    }

    let want_duplex = asked_duplex && disc.caps.duplex;
    if let Some(attr) = attr
        && attr.is_writable()
    {
        let candidates = build_feeder_select_candidates(&attr.constraint, cur, want_duplex);
        for cand in candidates {
            if try_write_prop(
                tree,
                target_item,
                WIA_IPS_DOCUMENT_HANDLING_SELECT,
                &PropValue::I4(cand),
            )? {
                break;
            }
        }
    }

    let after = read_i4(tree, target_item, WIA_IPS_DOCUMENT_HANDLING_SELECT)?.or(cur);
    let duplex_now = disc.caps.duplex && after.is_some_and(|v| (v & DUPLEX) != 0);
    let back_first = duplex_now && after.is_some_and(|v| (v & BACK_FIRST) != 0);
    Ok((duplex_now, back_first))
}

fn build_feeder_select_candidates(
    constraint: &PropConstraint,
    cur: Option<i32>,
    want_duplex: bool,
) -> Vec<i32> {
    let mut out = Vec::new();
    let mut push_if_ok = |v: i32| {
        if caps::constraint_allows_i4(constraint, v, true) && !out.contains(&v) {
            out.push(v);
        }
    };

    if want_duplex {
        if let Some(c) = cur {
            let preserved = (c & !(FRONT_ONLY | BACK_ONLY)) | DUPLEX;
            push_if_ok(preserved);
        }
        push_if_ok(FEEDER | DUPLEX);
        push_if_ok(DUPLEX);
        if let PropConstraint::ListI4 { values, .. } = constraint {
            for &v in values {
                if (v & DUPLEX) != 0 && (v & BACK_ONLY) == 0 {
                    push_if_ok(v);
                }
            }
            for &v in values {
                if (v & DUPLEX) != 0 {
                    push_if_ok(v);
                }
            }
        }
    } else {
        if let Some(c) = cur {
            let simplex = (c & !(DUPLEX | BACK_ONLY)) | FRONT_ONLY;
            push_if_ok(simplex);
            let cleared = c & !(DUPLEX | BACK_ONLY);
            if cleared != 0 {
                push_if_ok(cleared);
            }
        }
        push_if_ok(FEEDER | FRONT_ONLY);
        push_if_ok(FRONT_ONLY);
        push_if_ok(FEEDER);
        if let PropConstraint::ListI4 { values, .. } = constraint {
            for &v in values {
                if (v & DUPLEX) == 0 && (v & BACK_ONLY) == 0 {
                    push_if_ok(v);
                }
            }
        }
    }
    out
}

/// Negotiates `WIA_IPS_PAGES` (`3096`).
///
/// On `Feeder`, tries `ALL_PAGES` (`0`) first. If the driver refuses `0` (as Microsoft's inbox WSD
/// driver often does), selects the largest valid side count `min + k * step <= max`, preferring an
/// even count `>= 2` when `duplex_now` is `true` and `max >= 2` so a single `Download` call never
/// splits a duplex sheet in half unless `max == 1`.
fn negotiate_pages(
    tree: &mut dyn WiaDeviceTree,
    item: ItemId,
    handling_item: ItemId,
    source: Source,
    duplex_now: bool,
) -> Result<Option<u32>, ScanError> {
    let pages_item = if read_i4(tree, item, WIA_IPS_PAGES)?.is_some()
        || tree
            .prop_attrs(item, &[WIA_IPS_PAGES])?
            .first()
            .is_some_and(Option::is_some)
    {
        item
    } else if handling_item != item
        && (read_i4(tree, handling_item, WIA_IPS_PAGES)?.is_some()
            || tree
                .prop_attrs(handling_item, &[WIA_IPS_PAGES])?
                .first()
                .is_some_and(Option::is_some))
    {
        handling_item
    } else {
        return Ok(None);
    };

    let attrs = tree.prop_attrs(pages_item, &[WIA_IPS_PAGES])?;
    let attr = attrs.first().and_then(|a| a.as_ref());

    if source == Source::Flatbed {
        if attr.is_some_and(PropAttr::is_writable)
            && !try_write_prop(tree, pages_item, WIA_IPS_PAGES, &PropValue::I4(1))?
        {
            let _ = try_write_prop(tree, pages_item, WIA_IPS_PAGES, &PropValue::I4(ALL_PAGES))?;
        }
        return Ok(None);
    }

    // `Source::Feeder`: prefer `ALL_PAGES` (`0`) for continuous scanning.
    let writable = attr.is_some_and(PropAttr::is_writable);
    if writable && try_write_prop(tree, pages_item, WIA_IPS_PAGES, &PropValue::I4(ALL_PAGES))? {
        let rb = read_i4(tree, pages_item, WIA_IPS_PAGES)?;
        if rb.unwrap_or(0) <= 0 {
            return Ok(None);
        }
    }

    if writable
        && let Some(attr) = attr
        && let Some(fallback) = choose_nonzero_pages(&attr.constraint, duplex_now)
    {
        let _ = try_write_prop(tree, pages_item, WIA_IPS_PAGES, &PropValue::I4(fallback))?;
    }

    match read_i4(tree, pages_item, WIA_IPS_PAGES)? {
        Some(n) if n > 0 => Ok(Some(n as u32)),
        _ => Ok(None),
    }
}

pub(super) fn choose_nonzero_pages(constraint: &PropConstraint, duplex: bool) -> Option<i32> {
    match constraint {
        PropConstraint::ListI4 { values, .. } => {
            if duplex
                && let Some(even) = values
                    .iter()
                    .copied()
                    .filter(|&v| v >= 2 && v % 2 == 0)
                    .max()
            {
                return Some(even);
            }
            values.iter().copied().filter(|&v| v >= 1).max()
        }
        PropConstraint::Range { min, max, step, .. } => {
            if *max < 1 || *min > *max {
                return None;
            }
            if *step <= 0 {
                return (*min >= 1).then_some(*min);
            }
            let step64 = i64::from(*step);
            let start = if *min >= 1 {
                i64::from(*min)
            } else {
                let gap = (1_i64 - i64::from(*min)) as u64;
                let k = gap.div_ceil(*step as u64) as i64;
                i64::from(*min) + k * step64
            };
            if start > i64::from(*max) {
                return None;
            }
            let total_steps = (i64::from(*max) - start) / step64;
            let top = start + total_steps * step64;
            if duplex && *max >= 2 {
                if top >= 2 && top % 2 == 0 {
                    return i32::try_from(top).ok();
                }
                if total_steps >= 1 {
                    let prev = top - step64;
                    if prev >= 2 && prev % 2 == 0 {
                        return i32::try_from(prev).ok();
                    }
                }
            }
            i32::try_from(top).ok().filter(|&v| v >= 1)
        }
        PropConstraint::None | PropConstraint::Flags { .. } | PropConstraint::ListGuid { .. } => {
            Some(if duplex { 2 } else { 1 })
        }
    }
}

/// Chooses the format from `offered` (non-empty) for `asked`: `asked` if offered, else its
/// bit-depth / monochrome sibling, else `offered[0]` — identical to `SaneBackend` and
/// `TwainBackend`.
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

fn negotiate_format(
    tree: &mut dyn WiaDeviceTree,
    item: ItemId,
    offered: &[Format],
    asked: Format,
    allow_16bit: bool,
) -> Result<Format, ScanError> {
    if offered.is_empty() {
        return Err(ScanError::Unsupported);
    }
    let target = format_for(asked, offered);

    let dt_candidates: &[i32] = match target {
        Format::Bw1 => &[WIA_DATA_THRESHOLD, WIA_DATA_GRAYSCALE],
        Format::Gray8 | Format::Gray16 => &[WIA_DATA_GRAYSCALE],
        Format::Rgb8 => &[WIA_DATA_COLOR, WIA_DATA_RAW_RGB, WIA_DATA_RAW_BGR],
        Format::Rgb16 => &[WIA_DATA_COLOR, WIA_DATA_RAW_RGB, WIA_DATA_RAW_BGR],
    };
    let depth_candidates: &[i32] = match target {
        Format::Bw1 => &[1],
        Format::Gray8 => &[8],
        Format::Gray16 => &[16],
        Format::Rgb8 => &[24, 8, 32],
        Format::Rgb16 => &[48, 16],
    };

    let dt_attrs = tree.prop_attrs(item, &[WIA_IPA_DATATYPE])?;
    let dt_writable = dt_attrs
        .first()
        .and_then(|a| a.as_ref())
        .is_some_and(PropAttr::is_writable);

    for &dt in dt_candidates {
        if dt_writable {
            let allowed = dt_attrs
                .first()
                .and_then(|a| a.as_ref())
                .is_none_or(|a| caps::constraint_allows_i4(&a.constraint, dt, true));
            if !allowed || !try_write_prop(tree, item, WIA_IPA_DATATYPE, &PropValue::I4(dt))? {
                continue;
            }
        }
        // Re-query `WIA_IPA_DEPTH` constraint now that `WIA_IPA_DATATYPE` is updated.
        let depth_attrs = tree.prop_attrs(item, &[WIA_IPA_DEPTH])?;
        let depth_attr = depth_attrs.first().and_then(|a| a.as_ref());
        let mut depth_ok = false;
        if let Some(attr) = depth_attr
            && attr.is_writable()
        {
            for &d in depth_candidates {
                if caps::constraint_allows_i4(&attr.constraint, d, true)
                    && try_write_prop(tree, item, WIA_IPA_DEPTH, &PropValue::I4(d))?
                {
                    depth_ok = true;
                    break;
                }
            }
        } else {
            let cur_d = read_i4(tree, item, WIA_IPA_DEPTH)?;
            depth_ok = cur_d.is_none_or(|d| depth_candidates.contains(&d));
        }
        if depth_ok || !dt_writable {
            break;
        }
    }

    if matches!(target, Format::Bw1 | Format::Gray8 | Format::Gray16) {
        let _ = write_optional_i4(tree, item, WIA_IPS_PHOTOMETRIC_INTERP, WIA_PHOTO_WHITE_1)?;
    }

    readback_format(tree, item, allow_16bit)
}

fn readback_format(
    tree: &mut dyn WiaDeviceTree,
    item: ItemId,
    allow_16bit: bool,
) -> Result<Format, ScanError> {
    let vals = tree.read_props(
        item,
        &[WIA_IPA_DATATYPE, WIA_IPA_DEPTH, WIA_IPA_BITS_PER_CHANNEL],
    )?;
    let dt = vals
        .first()
        .and_then(|v| v.as_ref())
        .and_then(PropValue::as_i4)
        .ok_or(ScanError::Unsupported)?;
    let depth = vals
        .get(1)
        .and_then(|v| v.as_ref())
        .and_then(PropValue::as_i4);
    let bpc = vals
        .get(2)
        .and_then(|v| v.as_ref())
        .and_then(PropValue::as_i4);

    match dt {
        WIA_DATA_THRESHOLD => Ok(Format::Bw1),
        WIA_DATA_GRAYSCALE => match (depth, bpc) {
            (Some(1), _) | (_, Some(1)) => Ok(Format::Bw1),
            (Some(16), _) | (_, Some(16)) if allow_16bit => Ok(Format::Gray16),
            (None | Some(8), _) | (_, Some(8)) => Ok(Format::Gray8),
            _ => Err(error::driver_msg(format!(
                "unsupported WIA grayscale depth {depth:?} (bpc {bpc:?})"
            ))),
        },
        WIA_DATA_COLOR | WIA_DATA_RAW_RGB | WIA_DATA_RAW_BGR => match (depth, bpc) {
            (Some(48 | 16), _) | (_, Some(16)) if allow_16bit => Ok(Format::Rgb16),
            (None | Some(24 | 32 | 8), _) | (_, Some(8)) => Ok(Format::Rgb8),
            _ => Err(error::driver_msg(format!(
                "unsupported WIA color depth {depth:?} (bpc {bpc:?})"
            ))),
        },
        other => Err(error::driver_msg(format!(
            "unsupported WIA_IPA_DATATYPE {other}"
        ))),
    }
}

fn negotiate_transfer_format(
    tree: &mut dyn WiaDeviceTree,
    item: ItemId,
    format_now: Format,
    asked_encodings: &[Encoding],
) -> Result<(TransferMode, i32, Vec<Encoding>), ScanError> {
    let pairs = caps::collect_transfer_pairs(tree, item)?;

    if !asked_encodings.is_empty() {
        let mut rejected: Vec<Encoding> = Vec::new();
        for &enc in asked_encodings {
            if rejected.contains(&enc) || !caps::encoding_supports_format(enc, format_now) {
                continue;
            }
            let Some(guid) = caps::encoding_to_guid(enc) else {
                continue;
            };
            let offered_file = pairs
                .iter()
                .any(|p| p.tymed == TYMED_FILE && p.format == guid);
            if !offered_file {
                continue;
            }
            if select_tymed_and_format(tree, item, TYMED_FILE, guid)? {
                let taken_encodings: Vec<Encoding> = asked_encodings
                    .iter()
                    .copied()
                    .filter(|&e| {
                        !rejected.contains(&e)
                            && caps::encoding_supports_format(e, format_now)
                            && caps::encoding_to_guid(e).is_some_and(|g| {
                                pairs.iter().any(|p| p.tymed == TYMED_FILE && p.format == g)
                            })
                    })
                    .collect();
                return Ok((TransferMode::Encoded(enc), TYMED_FILE, taken_encodings));
            }
            rejected.push(enc);
        }
    }

    // Raw transfer mode selection (`TYMED` written before `FORMAT`).
    let raw_candidates: &[(i32, GuidBytes, TransferMode)] = match format_now {
        Format::Gray16 | Format::Rgb16 => &[
            (TYMED_FILE, WIA_IMG_FMT_RAW_GUID, TransferMode::RawWia),
            (TYMED_CALLBACK, WIA_IMG_FMT_RAW_GUID, TransferMode::RawWia),
        ],
        Format::Bw1 | Format::Gray8 | Format::Rgb8 => &[
            (TYMED_FILE, WIA_IMG_FMT_BMP_GUID, TransferMode::RawBmp),
            (TYMED_FILE, WIA_IMG_FMT_RAW_GUID, TransferMode::RawWia),
            (
                TYMED_CALLBACK,
                WIA_IMG_FMT_MEMORYBMP_GUID,
                TransferMode::RawMemoryBmp,
            ),
            (
                TYMED_FILE,
                WIA_IMG_FMT_MEMORYBMP_GUID,
                TransferMode::RawMemoryBmp,
            ),
            (TYMED_CALLBACK, WIA_IMG_FMT_RAW_GUID, TransferMode::RawWia),
        ],
    };

    for &(tymed, guid, mode) in raw_candidates {
        let supported = pairs
            .iter()
            .any(|pair: &FormatPair| pair.tymed == tymed && pair.format == guid);
        if !supported {
            continue;
        }
        if select_tymed_and_format(tree, item, tymed, guid)? {
            return Ok((mode, tymed, Vec::new()));
        }
    }

    Err(ScanError::Unsupported)
}

fn select_tymed_and_format(
    tree: &mut dyn WiaDeviceTree,
    item: ItemId,
    tymed: i32,
    guid: GuidBytes,
) -> Result<bool, ScanError> {
    let attrs = tree.prop_attrs(item, &[WIA_IPA_TYMED, WIA_IPA_FORMAT])?;
    let tymed_writable = attrs
        .first()
        .and_then(|a| a.as_ref())
        .is_some_and(PropAttr::is_writable);
    if tymed_writable {
        if !try_write_prop(tree, item, WIA_IPA_TYMED, &PropValue::I4(tymed))? {
            return Ok(false);
        }
    } else if let Some(cur_t) = read_i4(tree, item, WIA_IPA_TYMED)?
        && cur_t != tymed
    {
        return Ok(false);
    }

    // Re-read `WIA_IPA_FORMAT` attribute after updating `WIA_IPA_TYMED`.
    let fmt_attrs = tree.prop_attrs(item, &[WIA_IPA_FORMAT])?;
    let fmt_attr = fmt_attrs.first().and_then(|a| a.as_ref());
    if let Some(attr) = fmt_attr
        && attr.is_writable()
    {
        if let PropConstraint::ListGuid { values, .. } = &attr.constraint
            && !values.contains(&guid)
        {
            return Ok(false);
        }
        if !try_write_prop(tree, item, WIA_IPA_FORMAT, &PropValue::Guid(guid))? {
            return Ok(false);
        }
    }

    let vals = tree.read_props(item, &[WIA_IPA_FORMAT])?;
    if let Some(actual_guid) = vals
        .first()
        .and_then(|v| v.as_ref())
        .and_then(PropValue::as_guid)
        && actual_guid != guid
    {
        return Ok(false);
    }
    Ok(true)
}

fn nearest_dpi(resolutions: &Resolutions, asked: u32) -> u32 {
    match resolutions {
        Resolutions::List(values) => values
            .iter()
            .copied()
            .min_by_key(|&v| (v.abs_diff(asked), Reverse(v)))
            .unwrap_or(asked),
        Resolutions::Range { min, max, step } => nearest_step_u32(*min, *max, step.get(), asked),
    }
}

fn nearest_step_u32(min: u32, max: u32, step: u32, asked: u32) -> u32 {
    let [min, max, step, asked] = [min, max, step, asked].map(u64::from);
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

fn negotiate_resolution(
    tree: &mut dyn WiaDeviceTree,
    item: ItemId,
    asked_dpi: u32,
) -> Result<(f64, f64, u32), ScanError> {
    let x_res = caps::read_resolutions_for_item(tree, item)?.ok_or(ScanError::Unsupported)?;
    let target_x = nearest_dpi(&x_res, asked_dpi);
    let _ = write_optional_i4(
        tree,
        item,
        WIA_IPS_XRES,
        i32::try_from(target_x).unwrap_or(i32::MAX),
    )?;

    // Re-read `WIA_IPS_YRES` after writing `WIA_IPS_XRES`.
    if let Some(y_res) = caps::read_y_resolutions_for_item(tree, item)? {
        let target_y = nearest_dpi(&y_res, target_x);
        let _ = write_optional_i4(
            tree,
            item,
            WIA_IPS_YRES,
            i32::try_from(target_y).unwrap_or(i32::MAX),
        )?;
    }

    let vals = tree.read_props(
        item,
        &[
            WIA_IPS_XRES,
            WIA_IPS_YRES,
            WIA_IPS_OPTICAL_XRES,
            WIA_IPS_OPTICAL_YRES,
        ],
    )?;
    let x_rb = vals
        .first()
        .and_then(|v| v.as_ref())
        .and_then(PropValue::as_i4)
        .or_else(|| {
            vals.get(2)
                .and_then(|v| v.as_ref())
                .and_then(PropValue::as_i4)
        });
    let y_rb = vals
        .get(1)
        .and_then(|v| v.as_ref())
        .and_then(PropValue::as_i4)
        .or_else(|| {
            vals.get(3)
                .and_then(|v| v.as_ref())
                .and_then(PropValue::as_i4)
        })
        .or(x_rb);

    let Some(x_i32) = x_rb.filter(|&v| v > 0) else {
        return Err(error::driver_msg(format!(
            "the scanner's WIA_IPS_XRES read back as {x_rb:?}, which no page can have"
        )));
    };
    let Some(y_i32) = y_rb.filter(|&v| v > 0) else {
        return Err(error::driver_msg(format!(
            "the scanner's WIA_IPS_YRES read back as {y_rb:?}, which no page can have"
        )));
    };

    Ok((f64::from(x_i32), f64::from(y_i32), x_i32 as u32))
}

fn is_valid_paper(size: SizeMm) -> bool {
    size.width.is_finite() && size.width > 0.0 && size.height.is_finite() && size.height > 0.0
}

fn negotiate_paper(
    tree: &mut dyn WiaDeviceTree,
    item: ItemId,
    source: Source,
    asked: Paper,
    across_dpi: f64,
    down_dpi: f64,
) -> Result<Paper, ScanError> {
    let paper_auto_supported = caps::read_paper_auto(tree, item)?;
    let max_area = caps::read_max_area_mm(tree, item, source)?;

    if asked == Paper::Auto && paper_auto_supported {
        // Reset extents to full bed `[min, max]` before turning on auto-crop so a prior crop does
        // not clip the automatic border window.
        let _ = write_optional_i4(tree, item, WIA_IPS_PAGE_SIZE, WIA_PAGE_CUSTOM)?;
        reset_extents_to_full_bed(tree, item)?;

        let page_attrs = tree.prop_attrs(item, &[WIA_IPS_PAGE_SIZE])?;
        let page_auto_allowed = page_attrs
            .first()
            .and_then(|a| a.as_ref())
            .is_some_and(|a| {
                a.is_writable() && caps::constraint_allows_i4(&a.constraint, WIA_PAGE_AUTO, false)
            });

        if page_auto_allowed {
            let _ = try_write_prop(tree, item, WIA_IPS_PAGE_SIZE, &PropValue::I4(WIA_PAGE_AUTO))?;
        }
        let _ = write_optional_i4(tree, item, WIA_IPS_AUTO_CROP, WIA_AUTO_CROP_SINGLE)?;

        let rb = tree.read_props(item, &[WIA_IPS_AUTO_CROP, WIA_IPS_PAGE_SIZE])?;
        let crop_on = rb
            .first()
            .and_then(|v| v.as_ref())
            .and_then(PropValue::as_i4)
            .is_some_and(|v| v != WIA_AUTO_CROP_DISABLED);
        let page_auto_on = rb
            .get(1)
            .and_then(|v| v.as_ref())
            .and_then(PropValue::as_i4)
            .is_some_and(|v| v == WIA_PAGE_AUTO);
        if crop_on || page_auto_on {
            return Ok(Paper::Auto);
        }
    }

    // Explicit `Paper::Size` (or `Paper::Auto` when unsupported/refused): turn off auto-crop and
    // unlock custom extents via `WIA_PAGE_CUSTOM` (`2`) first.
    let _ = write_optional_i4(tree, item, WIA_IPS_AUTO_CROP, WIA_AUTO_CROP_DISABLED)?;
    let _ = write_optional_i4(tree, item, WIA_IPS_PAGE_SIZE, WIA_PAGE_CUSTOM)?;

    set_origin_to_min(tree, item)?;

    let ext_vals = tree.read_props(item, &[WIA_IPS_XEXTENT, WIA_IPS_YEXTENT])?;
    let ext_attrs = tree.prop_attrs(item, &[WIA_IPS_XEXTENT, WIA_IPS_YEXTENT])?;
    let has_extents = ext_vals.iter().any(Option::is_some) || ext_attrs.iter().any(Option::is_some);

    if !has_extents && max_area.is_none() {
        return Ok(Paper::Auto);
    }

    let desired_mm = match (asked, max_area) {
        (Paper::Size(sz), Some(largest)) if is_valid_paper(sz) => Some(SizeMm {
            width: sz.width.min(largest.width),
            height: sz.height.min(largest.height),
        }),
        (Paper::Size(sz), None) if is_valid_paper(sz) => Some(sz),
        (_, Some(largest)) => Some(largest),
        (_, None) => None,
    };

    if has_extents {
        apply_pixel_extents(tree, item, desired_mm, across_dpi, down_dpi)?;
        let rb = tree.read_props(item, &[WIA_IPS_XEXTENT, WIA_IPS_YEXTENT])?;
        let x_px = rb
            .first()
            .and_then(|v| v.as_ref())
            .and_then(PropValue::as_i4);
        let y_px = rb
            .get(1)
            .and_then(|v| v.as_ref())
            .and_then(PropValue::as_i4);
        if let (Some(xp), Some(yp)) = (x_px, y_px) {
            if xp <= 0 || yp <= 0 {
                return Err(error::driver_msg(format!(
                    "the scanner's pixel extents read back as {xp} by {yp}"
                )));
            }
            let width_mm = f64::from(xp) / across_dpi * 25.4;
            let height_mm = f64::from(yp) / down_dpi * 25.4;
            let fits = |mm: f64| mm.is_finite() && mm > 0.0 && mm <= f64::from(f32::MAX);
            if !fits(width_mm) || !fits(height_mm) {
                return Err(error::driver_msg(format!(
                    "the scanner's scan area read back as {width_mm} by {height_mm} mm"
                )));
            }
            return Ok(match asked {
                Paper::Size(req)
                    if is_valid_paper(req)
                        && (width_mm - f64::from(req.width)).abs() <= SAME_SIZE_MM
                        && (height_mm - f64::from(req.height)).abs() <= SAME_SIZE_MM =>
                {
                    asked
                }
                _ => Paper::Size(SizeMm {
                    width: width_mm as f32,
                    height: height_mm as f32,
                }),
            });
        }
    }

    match desired_mm {
        Some(sz) => Ok(match asked {
            Paper::Size(req)
                if is_valid_paper(req)
                    && (f64::from(sz.width) - f64::from(req.width)).abs() <= SAME_SIZE_MM
                    && (f64::from(sz.height) - f64::from(req.height)).abs() <= SAME_SIZE_MM =>
            {
                asked
            }
            _ => Paper::Size(sz),
        }),
        None => Ok(Paper::Auto),
    }
}

fn set_origin_to_min(tree: &mut dyn WiaDeviceTree, item: ItemId) -> Result<(), ScanError> {
    let attrs = tree.prop_attrs(item, &[WIA_IPS_XPOS, WIA_IPS_YPOS])?;
    for (idx, prop) in [(0usize, WIA_IPS_XPOS), (1usize, WIA_IPS_YPOS)] {
        let Some(Some(attr)) = attrs.get(idx) else {
            continue;
        };
        if !attr.is_writable() {
            continue;
        }
        let min_origin = match &attr.constraint {
            PropConstraint::Range { min, .. } => *min,
            PropConstraint::ListI4 { values, .. } => values.iter().copied().min().unwrap_or(0),
            _ => 0,
        };
        let _ = try_write_prop(tree, item, prop, &PropValue::I4(min_origin))?;
    }
    Ok(())
}

fn reset_extents_to_full_bed(tree: &mut dyn WiaDeviceTree, item: ItemId) -> Result<(), ScanError> {
    set_origin_to_min(tree, item)?;
    let attrs = tree.prop_attrs(item, &[WIA_IPS_XEXTENT, WIA_IPS_YEXTENT])?;
    for (idx, prop) in [(0usize, WIA_IPS_XEXTENT), (1usize, WIA_IPS_YEXTENT)] {
        let Some(Some(attr)) = attrs.get(idx) else {
            continue;
        };
        if !attr.is_writable() {
            continue;
        }
        let max_ext = match &attr.constraint {
            PropConstraint::Range { min, max, step, .. } if *max >= 1 && *min <= *max => {
                Some(snap_extent_to_range(*min, *max, *step, *max))
            }
            PropConstraint::ListI4 { values, .. } => {
                values.iter().copied().filter(|&v| v >= 1).max()
            }
            _ => None,
        };
        if let Some(ext) = max_ext {
            let _ = try_write_prop(tree, item, prop, &PropValue::I4(ext))?;
        }
    }
    Ok(())
}

fn apply_pixel_extents(
    tree: &mut dyn WiaDeviceTree,
    item: ItemId,
    desired_mm: Option<SizeMm>,
    across_dpi: f64,
    down_dpi: f64,
) -> Result<(), ScanError> {
    let attrs = tree.prop_attrs(item, &[WIA_IPS_XEXTENT, WIA_IPS_YEXTENT])?;
    let targets = [
        (
            0usize,
            WIA_IPS_XEXTENT,
            desired_mm.map(|s| f64::from(s.width)),
            across_dpi,
        ),
        (
            1usize,
            WIA_IPS_YEXTENT,
            desired_mm.map(|s| f64::from(s.height)),
            down_dpi,
        ),
    ];

    for (idx, prop, mm_opt, dpi) in targets {
        let Some(Some(attr)) = attrs.get(idx) else {
            continue;
        };
        if !attr.is_writable() {
            continue;
        }
        let desired_px = mm_opt.map(|mm| {
            let raw = (mm / 25.4 * dpi).round();
            if !raw.is_finite() || raw < 1.0 {
                1
            } else if raw >= f64::from(i32::MAX) {
                i32::MAX
            } else {
                raw as i32
            }
        });
        let chosen = choose_extent_pixels(&attr.constraint, desired_px);
        if let Some(px) = chosen {
            let _ = try_write_prop(tree, item, prop, &PropValue::I4(px))?;
        }
    }
    Ok(())
}

pub(super) fn choose_extent_pixels(
    constraint: &PropConstraint,
    desired: Option<i32>,
) -> Option<i32> {
    match constraint {
        PropConstraint::Range { min, max, step, .. } => {
            if *max < 1 || *min > *max {
                return None;
            }
            let ask = desired.unwrap_or(*max);
            Some(snap_extent_to_range(*min, *max, *step, ask))
        }
        PropConstraint::ListI4 { values, .. } => {
            let pos: Vec<i32> = values.iter().copied().filter(|&v| v >= 1).collect();
            if pos.is_empty() {
                return None;
            }
            match desired {
                Some(ask) => pos
                    .into_iter()
                    .min_by_key(|&v| (i64::from(v).abs_diff(i64::from(ask)), Reverse(v))),
                None => pos.into_iter().max(),
            }
        }
        PropConstraint::None => desired.filter(|&v| v >= 1),
        PropConstraint::Flags { .. } | PropConstraint::ListGuid { .. } => None,
    }
}

pub(super) fn snap_extent_to_range(min: i32, max: i32, step: i32, asked: i32) -> i32 {
    if step <= 0 || min == max {
        return min.max(1).min(max);
    }
    let step64 = i64::from(step);
    let min64 = if min >= 1 {
        i64::from(min)
    } else {
        let gap = (1_i64 - i64::from(min)) as u64;
        let k = gap.div_ceil(step as u64) as i64;
        i64::from(min) + k * step64
    };
    let max64 = i64::from(max);
    if min64 >= max64 {
        return max.max(1);
    }
    let last = min64 + (max64 - min64) / step64 * step64;
    let ask64 = i64::from(asked);
    let snapped = if ask64 <= min64 {
        min64
    } else if ask64 >= last {
        last
    } else {
        let below = min64 + (ask64 - min64) / step64 * step64;
        let above = below + step64;
        if ask64 - below < above - ask64 {
            below
        } else {
            above
        }
    };
    i32::try_from(snapped).unwrap_or(max)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wia::driver::fake_wia::{FakeDeviceSpec, FakeItem, FakeWiaHandle};
    use crate::wia::driver::{
        DUP, FEED, FLAT, WIA_AUTO_DESKEW_ON, WIA_CATEGORY_FEEDER_GUID, WIA_CATEGORY_FLATBED_GUID,
        WIA_DPS_DOCUMENT_HANDLING_CAPABILITIES, WIA_DPS_HORIZONTAL_BED_SIZE,
        WIA_DPS_HORIZONTAL_SHEET_FEED_SIZE, WIA_DPS_VERTICAL_BED_SIZE,
        WIA_DPS_VERTICAL_SHEET_FEED_SIZE, WIA_IMG_FMT_JPEG_GUID, WIA_IMG_FMT_PNG_GUID,
        WIA_IPA_ITEM_CATEGORY, WIA_ITEM_TYPE_FOLDER, WIA_ITEM_TYPE_IMAGE, WIA_ITEM_TYPE_ROOT,
        WIA_ITEM_TYPE_TRANSFER, WIA_PAGE_LETTER, WIA_PHOTO_WHITE_0, WIA_PROP_FLAG, WIA_PROP_LIST,
        WIA_PROP_RANGE, WIA_PROP_RW, WiaSystem,
    };
    use crate::wia::error::E_INVALIDARG_HR;
    use crate::wia::id::RawDeviceProps;

    fn rw_list(nom: i32, values: Vec<i32>) -> Option<PropAttr> {
        Some(PropAttr {
            access_flags: WIA_PROP_RW | WIA_PROP_LIST,
            constraint: PropConstraint::ListI4 { nom, values },
        })
    }

    fn rw_range(min: i32, nom: i32, max: i32, step: i32) -> Option<PropAttr> {
        Some(PropAttr {
            access_flags: WIA_PROP_RW | WIA_PROP_RANGE,
            constraint: PropConstraint::Range {
                min,
                nom,
                max,
                step,
            },
        })
    }

    fn rw_flags(nom: i32, valid_bits: i32) -> Option<PropAttr> {
        Some(PropAttr {
            access_flags: WIA_PROP_RW | WIA_PROP_FLAG,
            constraint: PropConstraint::Flags { nom, valid_bits },
        })
    }

    fn rw_guids(nom: GuidBytes, values: Vec<GuidBytes>) -> Option<PropAttr> {
        Some(PropAttr {
            access_flags: WIA_PROP_RW | WIA_PROP_LIST,
            constraint: PropConstraint::ListGuid { nom, values },
        })
    }

    fn scanner_props() -> RawDeviceProps {
        RawDeviceProps {
            dev_id: r"{6BDD1FC6}\0000".to_owned(),
            dev_type: Some(0x0001_0001),
            dev_name: Some("Office Scanner".to_owned()),
            vend_desc: Some("Acme".to_owned()),
            dev_desc: Some("DS-900".to_owned()),
            ..RawDeviceProps::default()
        }
    }

    /// Builds a WDK-strict fake scanner whose `write_hook` enforces exact dependency order:
    /// - `DEPTH` constraint updates ONLY when `DATATYPE` is written first;
    /// - `XEXTENT`/`YEXTENT` reject writes unless `AUTO_CROP == 0`, `PAGE_SIZE == WIA_PAGE_CUSTOM`,
    ///   and `XPOS == 0` / `YPOS == 0`, and recompute their `[min, max, step = 8]` range whenever
    ///   `XRES` / `YRES` or `XPOS` / `YPOS` changes!
    fn build_wdk_strict_scanner(wsd_nonzero_pages: bool) -> FakeWiaHandle {
        let fake = FakeWiaHandle::new();
        let mut spec = FakeDeviceSpec::new(scanner_props());
        spec.items[0] = FakeItem::new(WIA_ITEM_TYPE_ROOT | WIA_ITEM_TYPE_FOLDER)
            .with_prop(
                WIA_DPS_DOCUMENT_HANDLING_CAPABILITIES,
                PropValue::I4(FLAT | FEED | DUP),
                None,
            )
            .with_prop(WIA_DPS_HORIZONTAL_BED_SIZE, PropValue::I4(8500), None)
            .with_prop(WIA_DPS_VERTICAL_BED_SIZE, PropValue::I4(11000), None)
            .with_prop(
                WIA_DPS_HORIZONTAL_SHEET_FEED_SIZE,
                PropValue::I4(8500),
                None,
            )
            .with_prop(WIA_DPS_VERTICAL_SHEET_FEED_SIZE, PropValue::I4(14000), None);

        let pages_attr = if wsd_nonzero_pages {
            // WSD feeder rejecting `0` (`ALL_PAGES`) with range `[1..=9]`, `step = 1` (so duplex
            // must pick even `8` instead of odd `9`!).
            rw_range(1, 1, 9, 1)
        } else {
            rw_range(0, 0, 100, 1)
        };

        let feeder = FakeItem::new(WIA_ITEM_TYPE_IMAGE | WIA_ITEM_TYPE_TRANSFER)
            .with_prop(
                WIA_IPA_ITEM_CATEGORY,
                PropValue::Guid(WIA_CATEGORY_FEEDER_GUID),
                None,
            )
            // Sticky properties initially dirty!
            .with_prop(
                WIA_IPS_BLANK_PAGES,
                PropValue::I4(1),
                rw_list(1, vec![0, 1]),
            )
            .with_prop(
                WIA_IPS_AUTO_DESKEW,
                PropValue::I4(WIA_AUTO_DESKEW_ON),
                rw_list(
                    WIA_AUTO_DESKEW_ON,
                    vec![WIA_AUTO_DESKEW_ON, WIA_AUTO_DESKEW_OFF],
                ),
            )
            .with_prop(
                WIA_IPS_ROTATION,
                PropValue::I4(2),
                rw_list(2, vec![0, 1, 2, 3]),
            )
            .with_prop(WIA_IPS_PREVIEW, PropValue::I4(1), rw_list(1, vec![0, 1]))
            .with_prop(
                WIA_IPS_PHOTOMETRIC_INTERP,
                PropValue::I4(WIA_PHOTO_WHITE_0),
                rw_list(
                    WIA_PHOTO_WHITE_0,
                    vec![WIA_PHOTO_WHITE_1, WIA_PHOTO_WHITE_0],
                ),
            )
            .with_prop(
                WIA_IPS_DOCUMENT_HANDLING_SELECT,
                PropValue::I4(FEEDER | FRONT_ONLY),
                rw_flags(
                    FEEDER | FRONT_ONLY,
                    FEEDER | FRONT_ONLY | DUPLEX | BACK_FIRST,
                ),
            )
            .with_prop(WIA_IPS_PAGES, PropValue::I4(1), pages_attr)
            .with_prop(
                WIA_IPA_DATATYPE,
                PropValue::I4(WIA_DATA_THRESHOLD),
                rw_list(
                    WIA_DATA_THRESHOLD,
                    vec![WIA_DATA_THRESHOLD, WIA_DATA_GRAYSCALE, WIA_DATA_COLOR],
                ),
            )
            // Initially `DEPTH` only allows `[1]` because `DATATYPE` is `THRESHOLD`!
            .with_prop(WIA_IPA_DEPTH, PropValue::I4(1), rw_list(1, vec![1]))
            .with_prop(
                WIA_IPA_TYMED,
                PropValue::I4(TYMED_FILE),
                rw_list(TYMED_FILE, vec![TYMED_FILE, TYMED_CALLBACK]),
            )
            .with_prop(
                WIA_IPA_FORMAT,
                PropValue::Guid(WIA_IMG_FMT_BMP_GUID),
                rw_guids(
                    WIA_IMG_FMT_BMP_GUID,
                    vec![
                        WIA_IMG_FMT_BMP_GUID,
                        WIA_IMG_FMT_PNG_GUID,
                        WIA_IMG_FMT_JPEG_GUID,
                        WIA_IMG_FMT_RAW_GUID,
                    ],
                ),
            )
            .with_prop(
                WIA_IPS_XRES,
                PropValue::I4(150),
                rw_list(150, vec![150, 300, 600]),
            )
            .with_prop(
                WIA_IPS_YRES,
                PropValue::I4(150),
                rw_list(150, vec![150, 300, 600]),
            )
            .with_prop(
                WIA_IPS_AUTO_CROP,
                PropValue::I4(WIA_AUTO_CROP_SINGLE),
                rw_list(
                    WIA_AUTO_CROP_SINGLE,
                    vec![WIA_AUTO_CROP_DISABLED, WIA_AUTO_CROP_SINGLE],
                ),
            )
            .with_prop(
                WIA_IPS_PAGE_SIZE,
                PropValue::I4(WIA_PAGE_LETTER),
                rw_list(
                    WIA_PAGE_LETTER,
                    vec![WIA_PAGE_LETTER, WIA_PAGE_CUSTOM, WIA_PAGE_AUTO],
                ),
            )
            // Prior leftover non-zero offset `(XPOS = 200, YPOS = 300)` shrinking `XEXTENT.max`!
            .with_prop(WIA_IPS_XPOS, PropValue::I4(200), rw_range(0, 0, 1000, 1))
            .with_prop(WIA_IPS_YPOS, PropValue::I4(300), rw_range(0, 0, 1000, 1))
            .with_prop(
                WIA_IPS_XEXTENT,
                PropValue::I4(1000),
                rw_range(8, 1000, 1072, 8),
            )
            .with_prop(
                WIA_IPS_YEXTENT,
                PropValue::I4(1500),
                rw_range(8, 1500, 1800, 8),
            )
            .with_formats(vec![
                FormatPair {
                    format: WIA_IMG_FMT_BMP_GUID,
                    tymed: TYMED_FILE,
                },
                FormatPair {
                    format: WIA_IMG_FMT_PNG_GUID,
                    tymed: TYMED_FILE,
                },
                FormatPair {
                    format: WIA_IMG_FMT_JPEG_GUID,
                    tymed: TYMED_FILE,
                },
                FormatPair {
                    format: WIA_IMG_FMT_RAW_GUID,
                    tymed: TYMED_FILE,
                },
                FormatPair {
                    format: WIA_IMG_FMT_MEMORYBMP_GUID,
                    tymed: TYMED_CALLBACK,
                },
            ])
            .with_write_hook(|node, id, val| {
                let recompute_extents =
                    |props: &mut std::collections::BTreeMap<PropId, PropValue>,
                     attrs: &mut std::collections::BTreeMap<PropId, PropAttr>| {
                        let xres = props
                            .get(&WIA_IPS_XRES)
                            .and_then(PropValue::as_i4)
                            .unwrap_or(150);
                        let yres = props
                            .get(&WIA_IPS_YRES)
                            .and_then(PropValue::as_i4)
                            .unwrap_or(150);
                        let xpos = props
                            .get(&WIA_IPS_XPOS)
                            .and_then(PropValue::as_i4)
                            .unwrap_or(0);
                        let ypos = props
                            .get(&WIA_IPS_YPOS)
                            .and_then(PropValue::as_i4)
                            .unwrap_or(0);
                        let max_x = ((8500 * xres / 1000) - xpos).max(8) / 8 * 8;
                        let max_y = ((14000 * yres / 1000) - ypos).max(8) / 8 * 8;
                        attrs.insert(
                            WIA_IPS_XEXTENT,
                            PropAttr {
                                access_flags: WIA_PROP_RW | WIA_PROP_RANGE,
                                constraint: PropConstraint::Range {
                                    min: 8,
                                    nom: max_x,
                                    max: max_x,
                                    step: 8,
                                },
                            },
                        );
                        attrs.insert(
                            WIA_IPS_YEXTENT,
                            PropAttr {
                                access_flags: WIA_PROP_RW | WIA_PROP_RANGE,
                                constraint: PropConstraint::Range {
                                    min: 8,
                                    nom: max_y,
                                    max: max_y,
                                    step: 8,
                                },
                            },
                        );
                    };

                match id {
                    WIA_IPA_DATATYPE => {
                        let dt = val.as_i4().ok_or(E_INVALIDARG_HR)?;
                        let (nom, list) = match dt {
                            WIA_DATA_THRESHOLD => (1, vec![1]),
                            WIA_DATA_GRAYSCALE => (8, vec![8, 16]),
                            WIA_DATA_COLOR => (24, vec![24, 48]),
                            _ => return Err(E_INVALIDARG_HR),
                        };
                        node.props.insert(WIA_IPA_DEPTH, PropValue::I4(nom));
                        node.attrs.insert(
                            WIA_IPA_DEPTH,
                            PropAttr {
                                access_flags: WIA_PROP_RW | WIA_PROP_LIST,
                                constraint: PropConstraint::ListI4 { nom, values: list },
                            },
                        );
                    }
                    WIA_IPS_XRES | WIA_IPS_YRES | WIA_IPS_XPOS | WIA_IPS_YPOS => {
                        node.props.insert(id, val.clone());
                        recompute_extents(&mut node.props, &mut node.attrs);
                    }
                    WIA_IPS_PAGE_SIZE => {
                        if val.as_i4() == Some(WIA_PAGE_AUTO) {
                            // Under `WIA_PAGE_AUTO`, real WIA drivers report `XEXTENT = 0`!
                            node.props.insert(WIA_IPS_XEXTENT, PropValue::I4(0));
                            node.props.insert(WIA_IPS_YEXTENT, PropValue::I4(0));
                        }
                    }
                    WIA_IPS_XEXTENT | WIA_IPS_YEXTENT => {
                        let crop = node
                            .props
                            .get(&WIA_IPS_AUTO_CROP)
                            .and_then(PropValue::as_i4)
                            .unwrap_or(0);
                        let psize = node
                            .props
                            .get(&WIA_IPS_PAGE_SIZE)
                            .and_then(PropValue::as_i4)
                            .unwrap_or(WIA_PAGE_CUSTOM);
                        if crop != WIA_AUTO_CROP_DISABLED || psize != WIA_PAGE_CUSTOM {
                            return Err(E_INVALIDARG_HR);
                        }
                    }
                    _ => {}
                }
                Ok(())
            });

        spec.add_child(ItemId::ROOT, feeder);
        fake.add_device(spec);
        fake
    }

    #[test]
    fn negotiates_wdk_dependency_order_resets_sticky_props_snaps_stepped_extents_and_even_wsd_pages()
     {
        let fake = build_wdk_strict_scanner(true);
        let mut sys = fake.make_system();
        let (_, mut tree) = sys.open_device(r"{6BDD1FC6}\0000").unwrap();

        let mut asked = Settings::new(Source::Flatbed, 250, Format::Rgb16);
        asked.duplex = true;
        asked.paper = Paper::Size(SizeMm::LETTER);
        // `Jpeg` and `Bmp` are incompatible with `Rgb16`, while `Png` IS compatible!
        asked.encodings = vec![Encoding::Jpeg, Encoding::Bmp, Encoding::Png];

        let taken = apply(&mut *tree, &asked).unwrap();
        // Clamped: Source(Flatbed -> Feeder), Dpi(250 -> 300), Paper(Letter 2550px -> 2544px step 8),
        // Encodings([Jpeg, Bmp, Png] -> [Png]). Duplex(true) and Format(Rgb16) were accepted!
        assert_eq!(taken.applied.settings.source, Source::Feeder);
        assert!(taken.applied.settings.duplex);
        assert_eq!(taken.applied.settings.dpi, 300);
        assert_eq!(taken.applied.settings.format, Format::Rgb16);
        assert_eq!(taken.applied.settings.encodings, vec![Encoding::Png]);
        assert_eq!(taken.transfer_mode, TransferMode::Encoded(Encoding::Png));
        assert_eq!(taken.tymed, TYMED_FILE);
        // WSD range `[1..=9]` in duplex picked even `8`!
        assert_eq!(taken.pages_per_download, Some(8));
        assert_eq!(taken.dpi, (300.0, 300.0));

        // Verify sticky resets & XPOS/YPOS reset actually landed on the item.
        let props = tree
            .read_props(
                taken.item,
                &[
                    WIA_IPS_BLANK_PAGES,
                    WIA_IPS_AUTO_DESKEW,
                    WIA_IPS_ROTATION,
                    WIA_IPS_PREVIEW,
                    WIA_IPS_XPOS,
                    WIA_IPS_YPOS,
                    WIA_IPS_XEXTENT,
                    WIA_IPS_YEXTENT,
                ],
            )
            .unwrap();
        assert_eq!(
            props[0],
            Some(PropValue::I4(WIA_BLANK_PAGE_DETECTION_DISABLED))
        );
        assert_eq!(props[1], Some(PropValue::I4(WIA_AUTO_DESKEW_OFF)));
        assert_eq!(props[2], Some(PropValue::I4(PORTRAIT)));
        assert_eq!(props[3], Some(PropValue::I4(WIA_FINAL_SCAN)));
        assert_eq!(props[4], Some(PropValue::I4(0)));
        assert_eq!(props[5], Some(PropValue::I4(0)));
        // 8.5 in * 300 = 2550 px -> nearest multiple of 8 not past max `2544` is `2544`!
        // 11.0 in * 300 = 3300 px -> nearest multiple of 8 (`3296` vs `3304`, tie goes higher `3304`)!
        assert_eq!(props[6], Some(PropValue::I4(2544)));
        assert_eq!(props[7], Some(PropValue::I4(3304)));

        assert!(
            taken
                .applied
                .clamped
                .contains(&Clamped::Source(Source::Flatbed))
        );
        assert!(taken.applied.clamped.contains(&Clamped::Dpi(250)));
        assert!(
            taken
                .applied
                .clamped
                .contains(&Clamped::Paper(Paper::Size(SizeMm::LETTER)))
        );
        assert!(
            taken
                .applied
                .clamped
                .contains(&Clamped::Encodings(asked.encodings))
        );
    }

    #[test]
    fn paper_auto_keeps_auto_even_when_driver_reports_zero_extents_and_raw_bmp_stays_raw() {
        let fake = build_wdk_strict_scanner(false);
        let mut sys = fake.make_system();
        let (_, mut tree) = sys.open_device(r"{6BDD1FC6}\0000").unwrap();

        // Ask for raw (`encodings` empty) `Gray8` with `Paper::Auto`.
        let asked = Settings::new(Source::Feeder, 300, Format::Gray8);
        let taken = apply(&mut *tree, &asked).unwrap();

        assert_eq!(taken.applied.clamped, Vec::<Clamped>::new());
        assert_eq!(taken.applied.settings.paper, Paper::Auto);
        // `WiaImgFmt_BMP` chosen as internal raw transfer -> `TransferMode::RawBmp`, NOT `Encoded(Bmp)`!
        assert_eq!(taken.transfer_mode, TransferMode::RawBmp);
        assert_eq!(taken.pages_per_download, None);
        // Monochrome `Gray8` reset `WIA_IPS_PHOTOMETRIC_INTERP` to `WIA_PHOTO_WHITE_1` (`0`).
        assert_eq!(
            tree.read_props(taken.item, &[WIA_IPS_PHOTOMETRIC_INTERP])
                .unwrap()[0],
            Some(PropValue::I4(WIA_PHOTO_WHITE_1))
        );

        // Explicitly asking for `Encoding::Bmp` selects `TransferMode::Encoded(Encoding::Bmp)`.
        let mut asked_bmp = Settings::new(Source::Feeder, 300, Format::Rgb8);
        asked_bmp.encodings = vec![Encoding::Bmp];
        let taken_bmp = apply(&mut *tree, &asked_bmp).unwrap();
        assert_eq!(
            taken_bmp.transfer_mode,
            TransferMode::Encoded(Encoding::Bmp)
        );
        assert_eq!(taken_bmp.applied.settings.encodings, vec![Encoding::Bmp]);
        assert!(taken_bmp.applied.clamped.is_empty());
    }

    #[test]
    fn unsupported_paper_auto_and_invalid_size_clamp_to_max_area_and_callback_falls_back_to_file() {
        let fake = FakeWiaHandle::new();
        let mut spec = FakeDeviceSpec::new(scanner_props());
        spec.items[0] = FakeItem::new(WIA_ITEM_TYPE_ROOT | WIA_ITEM_TYPE_FOLDER)
            .with_prop(WIA_DPS_HORIZONTAL_BED_SIZE, PropValue::I4(8500), None)
            .with_prop(WIA_DPS_VERTICAL_BED_SIZE, PropValue::I4(11000), None);
        spec.add_child(
            ItemId::ROOT,
            FakeItem::new(WIA_ITEM_TYPE_IMAGE | WIA_ITEM_TYPE_TRANSFER)
                .with_prop(
                    WIA_IPA_ITEM_CATEGORY,
                    PropValue::Guid(WIA_CATEGORY_FLATBED_GUID),
                    None,
                )
                .with_prop(WIA_IPS_XRES, PropValue::I4(300), rw_list(300, vec![300]))
                .with_prop(WIA_IPS_YRES, PropValue::I4(300), rw_list(300, vec![300]))
                .with_prop(
                    WIA_IPA_DATATYPE,
                    PropValue::I4(WIA_DATA_COLOR),
                    rw_list(WIA_DATA_COLOR, vec![WIA_DATA_COLOR]),
                )
                .with_prop(WIA_IPA_DEPTH, PropValue::I4(24), None)
                .with_prop(
                    WIA_IPA_TYMED,
                    PropValue::I4(TYMED_CALLBACK),
                    rw_list(TYMED_CALLBACK, vec![TYMED_CALLBACK, TYMED_FILE]),
                )
                .with_prop(
                    WIA_IPA_FORMAT,
                    PropValue::Guid(WIA_IMG_FMT_MEMORYBMP_GUID),
                    rw_guids(
                        WIA_IMG_FMT_MEMORYBMP_GUID,
                        vec![WIA_IMG_FMT_MEMORYBMP_GUID, WIA_IMG_FMT_BMP_GUID],
                    ),
                )
                .with_prop(
                    WIA_IPS_XEXTENT,
                    PropValue::I4(2550),
                    rw_range(1, 2550, 2550, 1),
                )
                .with_prop(
                    WIA_IPS_YEXTENT,
                    PropValue::I4(3300),
                    rw_range(1, 3300, 3300, 1),
                )
                .with_formats(vec![
                    FormatPair {
                        format: WIA_IMG_FMT_MEMORYBMP_GUID,
                        tymed: TYMED_CALLBACK,
                    },
                    FormatPair {
                        format: WIA_IMG_FMT_MEMORYBMP_GUID,
                        tymed: TYMED_FILE,
                    },
                ]),
        );
        fake.add_device(spec);
        let mut sys = fake.make_system();
        let (_, mut tree) = sys.open_device(r"{6BDD1FC6}\0000").unwrap();

        // `Paper::Auto` on a source without `paper_auto` clamps to `SizeMm::LETTER`!
        let asked_auto = Settings::new(Source::Flatbed, 300, Format::Rgb8);
        let mut taken = apply(&mut *tree, &asked_auto).unwrap();
        assert_eq!(taken.applied.settings.paper, Paper::Size(SizeMm::LETTER));
        assert_eq!(taken.applied.clamped, vec![Clamped::Paper(Paper::Auto)]);
        assert_eq!(taken.tymed, TYMED_CALLBACK);
        assert_eq!(taken.transfer_mode, TransferMode::RawMemoryBmp);

        // `TYMED_CALLBACK` -> `TYMED_FILE` fallback switches `tymed` in place!
        assert!(fallback_callback_to_file(&mut *tree, &mut taken).unwrap());
        assert_eq!(taken.tymed, TYMED_FILE);
        assert_eq!(taken.transfer_mode, TransferMode::RawMemoryBmp);

        // Negative / non-positive `SizeMm` also clamps to `SizeMm::LETTER`!
        let mut asked_bad_sz = Settings::new(Source::Flatbed, 300, Format::Rgb8);
        asked_bad_sz.paper = Paper::Size(SizeMm {
            width: -10.0,
            height: 0.0,
        });
        let taken_bad = apply(&mut *tree, &asked_bad_sz).unwrap();
        assert_eq!(
            taken_bad.applied.settings.paper,
            Paper::Size(SizeMm::LETTER)
        );
        assert_eq!(
            taken_bad.applied.clamped,
            vec![Clamped::Paper(asked_bad_sz.paper)]
        );
    }
}
