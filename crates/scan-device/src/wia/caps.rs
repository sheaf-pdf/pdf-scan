//! WIA 2.0 item-tree source discovery and [`Caps`] / [`SourceCaps`] construction.
//!
//! Pure safe Rust over [`super::driver::WiaDeviceTree`], tested across 64-bit, 32-bit, and
//! non-Windows hosts via [`super::driver::fake_wia`].

use std::num::NonZeroU32;

use super::driver::{
    DUP, DUPLEX, FormatPair, GuidBytes, ItemId, PropAttr, PropConstraint, PropId, PropValue,
    TYMED_CALLBACK, TYMED_FILE, WIA_AUTO_CROP_SINGLE, WIA_CATEGORY_FEEDER_BACK_GUID,
    WIA_CATEGORY_FEEDER_FRONT_GUID, WIA_CATEGORY_FEEDER_GUID, WIA_CATEGORY_FILM_GUID,
    WIA_CATEGORY_FINISHED_FILE_GUID, WIA_CATEGORY_FLATBED_GUID, WIA_CATEGORY_FOLDER_GUID,
    WIA_CATEGORY_ROOT_GUID, WIA_DATA_COLOR, WIA_DATA_DITHER, WIA_DATA_GRAYSCALE, WIA_DATA_RAW_BGR,
    WIA_DATA_RAW_RGB, WIA_DATA_THRESHOLD, WIA_DPS_DOCUMENT_HANDLING_CAPABILITIES,
    WIA_DPS_HORIZONTAL_BED_SIZE, WIA_DPS_HORIZONTAL_SHEET_FEED_SIZE, WIA_DPS_VERTICAL_BED_SIZE,
    WIA_DPS_VERTICAL_SHEET_FEED_SIZE, WIA_IMG_FMT_BMP_GUID, WIA_IMG_FMT_JPEG_GUID,
    WIA_IMG_FMT_MEMORYBMP_GUID, WIA_IMG_FMT_PNG_GUID, WIA_IMG_FMT_RAW_GUID, WIA_IMG_FMT_TIFF_GUID,
    WIA_IPA_BITS_PER_CHANNEL, WIA_IPA_DATATYPE, WIA_IPA_DEPTH, WIA_IPA_FORMAT,
    WIA_IPA_FULL_ITEM_NAME, WIA_IPA_ITEM_CATEGORY, WIA_IPA_ITEM_NAME, WIA_IPA_TYMED,
    WIA_IPS_AUTO_CROP, WIA_IPS_DOCUMENT_HANDLING_SELECT, WIA_IPS_MAX_HORIZONTAL_SIZE,
    WIA_IPS_MAX_VERTICAL_SIZE, WIA_IPS_OPTICAL_XRES, WIA_IPS_OPTICAL_YRES, WIA_IPS_PAGE_SIZE,
    WIA_IPS_XEXTENT, WIA_IPS_XRES, WIA_IPS_YEXTENT, WIA_IPS_YRES, WIA_ITEM_TYPE_FILE,
    WIA_ITEM_TYPE_FOLDER, WIA_ITEM_TYPE_IMAGE, WIA_ITEM_TYPE_TRANSFER, WIA_PAGE_AUTO,
    WiaDeviceTree,
};
use crate::{Caps, Encoding, Format, Resolutions, ScanError, SizeMm, Source, SourceCaps};

/// Maximum recursion depth when walking `IWiaItem2::EnumChildItems` folders.
const MAX_TREE_DEPTH: usize = 8;
/// Maximum total child items inspected in one device tree so a malformed driver cannot loop.
const MAX_TREE_ITEMS: usize = 64;

/// A usable WIA source item discovered in the `IWiaItem2` tree alongside its [`SourceCaps`].
#[derive(Clone, Debug)]
pub(super) struct DiscoveredSource {
    pub item: ItemId,
    pub handling_item: ItemId,
    pub caps: SourceCaps,
    pub transfer_pairs: Vec<FormatPair>,
}

/// Reads full [`Caps`] from an open [`WiaDeviceTree`].
///
/// Returns [`ScanError::Unsupported`] when the tree exposes zero usable scanner sources (`Flatbed`
/// or `Feeder` with at least one raw format and at least one positive resolution).
pub(super) fn read(tree: &mut dyn WiaDeviceTree) -> Result<Caps, ScanError> {
    let discovered = discover_usable_sources(tree)?;
    if discovered.is_empty() {
        return Err(ScanError::Unsupported);
    }
    let sources = discovered.into_iter().map(|d| d.caps).collect();
    Ok(Caps::new(sources))
}

/// Walks the `IWiaItem2` child tree, deduplicating `Flatbed` and `Feeder` candidates (preferring
/// `WIA_CATEGORY_FEEDER` over `WIA_CATEGORY_FEEDER_FRONT` over item-name fallbacks), and builds
/// [`DiscoveredSource`] entries in canonical `[Flatbed, Feeder]` order.
pub(super) fn discover_usable_sources(
    tree: &mut dyn WiaDeviceTree,
) -> Result<Vec<DiscoveredSource>, ScanError> {
    tree.set_op(super::error::WiaOp::Caps);
    tree.reset_children();

    let mut candidates: Vec<(ItemId, Source, u8, bool)> = Vec::new();
    let mut visited = 0usize;
    collect_candidates(tree, ItemId::ROOT, 0, &mut visited, &mut candidates)?;

    // Pick highest-priority candidate per `Source` (preserving first-seen order on ties), but if
    // the highest-priority item turns out unusable (no formats or resolutions), allow falling back
    // to a lower-priority candidate of the same `Source`.
    let mut out = Vec::new();
    for want in [Source::Flatbed, Source::Feeder] {
        let mut matching: Vec<(usize, ItemId, u8, bool)> = candidates
            .iter()
            .enumerate()
            .filter(|(_, (_, src, _, _))| *src == want)
            .map(|(ord, &(item, _, prio, has_back_child))| (ord, item, prio, has_back_child))
            .collect();
        matching.sort_by(|a, b| b.2.cmp(&a.2).then_with(|| a.0.cmp(&b.0)));

        for (_, item, _, has_back_child) in matching {
            if let Some(disc) = build_discovered_source(tree, item, want, has_back_child)? {
                out.push(disc);
                break;
            }
        }
    }

    Ok(out)
}

fn collect_candidates(
    tree: &mut dyn WiaDeviceTree,
    parent: ItemId,
    depth: usize,
    visited: &mut usize,
    out: &mut Vec<(ItemId, Source, u8, bool)>,
) -> Result<bool, ScanError> {
    if depth >= MAX_TREE_DEPTH || *visited >= MAX_TREE_ITEMS {
        return Ok(false);
    }
    let children = tree.children_of(parent)?;
    let mut saw_feeder_back = false;
    let level_start = out.len();

    for child in children {
        if *visited >= MAX_TREE_ITEMS {
            break;
        }
        *visited += 1;

        let flags = tree.item_type(child)?;
        let props = tree.read_props(
            child,
            &[
                WIA_IPA_ITEM_CATEGORY,
                WIA_IPA_ITEM_NAME,
                WIA_IPA_FULL_ITEM_NAME,
            ],
        )?;
        let category = props
            .first()
            .and_then(|v| v.as_ref())
            .and_then(PropValue::as_guid);
        let name = props
            .get(1)
            .and_then(|v| v.as_ref())
            .and_then(PropValue::as_str);
        let full_name = props
            .get(2)
            .and_then(|v| v.as_ref())
            .and_then(PropValue::as_str);

        if category == Some(WIA_CATEGORY_FEEDER_BACK_GUID) {
            saw_feeder_back = true;
        }

        let mut child_has_back = false;
        if (flags & WIA_ITEM_TYPE_FOLDER) != 0 {
            child_has_back = collect_candidates(tree, child, depth + 1, visited, out)?;
            if child_has_back {
                saw_feeder_back = true;
            }
        }

        if let Some((source, prio)) = classify_item(flags, category, name, full_name) {
            out.push((child, source, prio, child_has_back));
        }
    }

    if saw_feeder_back {
        for entry in &mut out[level_start..] {
            if entry.1 == Source::Feeder {
                entry.3 = true;
            }
        }
    }

    Ok(saw_feeder_back)
}

fn classify_item(
    item_flags: i32,
    category: Option<GuidBytes>,
    name: Option<&str>,
    full_name: Option<&str>,
) -> Option<(Source, u8)> {
    let transferable =
        (item_flags & (WIA_ITEM_TYPE_IMAGE | WIA_ITEM_TYPE_TRANSFER | WIA_ITEM_TYPE_FILE)) != 0;

    if let Some(cat) = category {
        if cat == WIA_CATEGORY_FLATBED_GUID {
            return transferable.then_some((Source::Flatbed, 3));
        }
        if cat == WIA_CATEGORY_FEEDER_GUID {
            return transferable.then_some((Source::Feeder, 3));
        }
        if cat == WIA_CATEGORY_FEEDER_FRONT_GUID {
            return transferable.then_some((Source::Feeder, 2));
        }
        if cat == WIA_CATEGORY_FEEDER_BACK_GUID
            || cat == WIA_CATEGORY_FILM_GUID
            || cat == WIA_CATEGORY_FINISHED_FILE_GUID
            || cat == WIA_CATEGORY_ROOT_GUID
        {
            return None;
        }
        if cat != WIA_CATEGORY_FOLDER_GUID {
            // Unknown vendor category GUID: fall through to item-name heuristic below if
            // transferable.
        }
    }

    if !transferable {
        return None;
    }

    let label = name.or_else(|| full_name.and_then(|s| s.rsplit(['\\', '/']).next()))?;
    let lower = label.to_ascii_lowercase();
    if lower.contains("flatbed") || lower.contains("platen") || lower.contains("glass") {
        Some((Source::Flatbed, 1))
    } else if (lower.contains("feeder") || lower.contains("adf")) && !lower.contains("back") {
        Some((Source::Feeder, 1))
    } else {
        None
    }
}

fn build_discovered_source(
    tree: &mut dyn WiaDeviceTree,
    item: ItemId,
    source: Source,
    has_back_child: bool,
) -> Result<Option<DiscoveredSource>, ScanError> {
    let transfer_pairs = collect_transfer_pairs(tree, item)?;
    if !has_any_raw_transfer(&transfer_pairs) {
        return Ok(None);
    }
    let allow_16bit = supports_raw_transfer(&transfer_pairs);

    let Some(resolutions) = read_resolutions_for_item(tree, item)? else {
        return Ok(None);
    };

    let formats = read_formats_for_item(tree, item, allow_16bit)?;
    if formats.is_empty() {
        return Ok(None);
    }

    let encodings = read_encodings(&transfer_pairs, &formats);
    let (duplex, handling_item) = if source == Source::Feeder {
        read_feeder_duplex(tree, item, has_back_child)?
    } else {
        let h_item = find_handling_item(tree, item)?;
        (false, h_item)
    };
    let max_area = read_max_area_mm(tree, item, source)?;
    let paper_auto = read_paper_auto(tree, item)?;

    let mut caps = SourceCaps::new(source, resolutions, formats);
    caps.duplex = duplex;
    caps.encodings = encodings;
    caps.max_area = max_area;
    caps.paper_auto = paper_auto;

    Ok(Some(DiscoveredSource {
        item,
        handling_item,
        caps,
        transfer_pairs,
    }))
}

/// Collects `(guidFormatID, lTymed)` pairs from `IWiaTransfer::EnumWIA_FORMAT_INFO`, falling back
/// to synthesizing pairs from `WIA_IPA_FORMAT` and `WIA_IPA_TYMED` properties/constraints when
/// `EnumWIA_FORMAT_INFO` returns an empty list or non-fatal refusal.
pub(super) fn collect_transfer_pairs(
    tree: &mut dyn WiaDeviceTree,
    item: ItemId,
) -> Result<Vec<FormatPair>, ScanError> {
    let mut pairs = match tree.transfer_formats(item) {
        Ok(list) => list,
        Err(err) if super::error::is_fatal_scan_error(&err) => return Err(err),
        Err(_) => Vec::new(),
    };
    if !pairs.is_empty() {
        return Ok(pairs);
    }

    let vals = tree.read_props(item, &[WIA_IPA_FORMAT, WIA_IPA_TYMED])?;
    let attrs = tree.prop_attrs(item, &[WIA_IPA_FORMAT, WIA_IPA_TYMED])?;

    let mut tymeds = Vec::new();
    if let Some(Some(attr)) = attrs.get(1) {
        match &attr.constraint {
            PropConstraint::ListI4 { values, .. } => tymeds.extend(values.iter().copied()),
            PropConstraint::Flags { valid_bits, .. } => {
                for cand in [TYMED_FILE, TYMED_CALLBACK] {
                    if (*valid_bits & cand) != 0 {
                        tymeds.push(cand);
                    }
                }
            }
            _ => {}
        }
    }
    if let Some(cur_t) = vals
        .get(1)
        .and_then(|v| v.as_ref())
        .and_then(PropValue::as_i4)
        && !tymeds.contains(&cur_t)
    {
        tymeds.push(cur_t);
    }
    if tymeds.is_empty() {
        tymeds.push(TYMED_FILE);
    }

    let mut guids = Vec::new();
    if let Some(Some(attr)) = attrs.first()
        && let PropConstraint::ListGuid { values, .. } = &attr.constraint
    {
        for &g in values {
            if !guids.contains(&g) {
                guids.push(g);
            }
        }
    }
    if let Some(cur_g) = vals
        .first()
        .and_then(|v| v.as_ref())
        .and_then(PropValue::as_guid)
        && !guids.contains(&cur_g)
    {
        guids.push(cur_g);
    }

    for &t in &tymeds {
        for &g in &guids {
            let pair = FormatPair {
                format: g,
                tymed: t,
            };
            if !pairs.contains(&pair) {
                pairs.push(pair);
            }
        }
    }

    Ok(pairs)
}

/// Whether `pairs` includes `WiaImgFmt_RAW` over `TYMED_FILE` or `TYMED_CALLBACK`.
pub(super) fn supports_raw_transfer(pairs: &[FormatPair]) -> bool {
    pairs.iter().any(|p| {
        p.format == WIA_IMG_FMT_RAW_GUID && (p.tymed == TYMED_FILE || p.tymed == TYMED_CALLBACK)
    })
}

fn has_any_raw_transfer(pairs: &[FormatPair]) -> bool {
    pairs.iter().any(|p| {
        (p.tymed == TYMED_FILE
            && (p.format == WIA_IMG_FMT_BMP_GUID
                || p.format == WIA_IMG_FMT_RAW_GUID
                || p.format == WIA_IMG_FMT_MEMORYBMP_GUID))
            || (p.tymed == TYMED_CALLBACK
                && (p.format == WIA_IMG_FMT_MEMORYBMP_GUID || p.format == WIA_IMG_FMT_RAW_GUID))
    })
}

/// Whether `encoding` can represent `format` without loss of color model / bit-depth invalidity.
pub(super) fn encoding_supports_format(encoding: Encoding, format: Format) -> bool {
    match encoding {
        Encoding::Png | Encoding::Tiff => true,
        Encoding::Bmp => matches!(format, Format::Bw1 | Format::Gray8 | Format::Rgb8),
        Encoding::Jpeg => matches!(format, Format::Gray8 | Format::Rgb8),
        Encoding::CcittG4 => false,
    }
}

/// Maps `Encoding` to its WIA `WiaImgFmt_*` GUID (`TYMED_FILE`).
pub(super) fn encoding_to_guid(encoding: Encoding) -> Option<GuidBytes> {
    match encoding {
        Encoding::Png => Some(WIA_IMG_FMT_PNG_GUID),
        Encoding::Tiff => Some(WIA_IMG_FMT_TIFF_GUID),
        Encoding::Bmp => Some(WIA_IMG_FMT_BMP_GUID),
        Encoding::Jpeg => Some(WIA_IMG_FMT_JPEG_GUID),
        Encoding::CcittG4 => None,
    }
}

fn read_encodings(pairs: &[FormatPair], formats: &[Format]) -> Vec<Encoding> {
    let mut out = Vec::new();
    for enc in [Encoding::Png, Encoding::Tiff, Encoding::Bmp, Encoding::Jpeg] {
        let Some(guid) = encoding_to_guid(enc) else {
            continue;
        };
        let offered_as_file = pairs
            .iter()
            .any(|p| p.tymed == TYMED_FILE && p.format == guid);
        let compatible = formats
            .iter()
            .copied()
            .any(|f| encoding_supports_format(enc, f));
        if offered_as_file && compatible {
            out.push(enc);
        }
    }
    out
}

/// Reads supported [`Format`]s in canonical model order `[Bw1, Gray8, Gray16, Rgb8, Rgb16]`.
///
/// When `WIA_IPA_DATATYPE` is writable, probes each supported datatype so drivers whose
/// `WIA_IPA_DEPTH` constraint updates dynamically with `WIA_IPA_DATATYPE` reveal their 16-bit /
/// 48-bit depths, then restores the initial `(DATATYPE, DEPTH)` values.
pub(super) fn read_formats_for_item(
    tree: &mut dyn WiaDeviceTree,
    item: ItemId,
    allow_16bit: bool,
) -> Result<Vec<Format>, ScanError> {
    let init_vals = tree.read_props(
        item,
        &[WIA_IPA_DATATYPE, WIA_IPA_DEPTH, WIA_IPA_BITS_PER_CHANNEL],
    )?;
    let init_attrs = tree.prop_attrs(
        item,
        &[WIA_IPA_DATATYPE, WIA_IPA_DEPTH, WIA_IPA_BITS_PER_CHANNEL],
    )?;

    let init_dt = init_vals
        .first()
        .and_then(|v| v.as_ref())
        .and_then(PropValue::as_i4);
    let init_depth = init_vals
        .get(1)
        .and_then(|v| v.as_ref())
        .and_then(PropValue::as_i4);
    let dt_attr = init_attrs.first().and_then(|a| a.as_ref());
    let dt_writable = dt_attr.is_some_and(PropAttr::is_writable);

    let mut datatypes = Vec::new();
    if dt_writable && let Some(attr) = dt_attr {
        collect_constraint_i4(&attr.constraint, &mut datatypes);
    }
    if let Some(cur) = init_dt
        && !datatypes.contains(&cur)
    {
        datatypes.push(cur);
    }

    let mut has_bw1 = false;
    let mut has_gray8 = false;
    let mut has_gray16 = false;
    let mut has_rgb8 = false;
    let mut has_rgb16 = false;

    let mut changed_dt = false;
    let probe_res: Result<(), ScanError> = (|| {
        for &dt in &datatypes {
            let active_dt = if Some(dt) == init_dt && !changed_dt {
                true
            } else if dt_writable {
                match tree.write_prop(item, WIA_IPA_DATATYPE, &PropValue::I4(dt)) {
                    Ok(()) => {
                        changed_dt = true;
                        true
                    }
                    Err(hr) => {
                        let mapped = super::error::hresult(super::error::WiaOp::Caps, hr);
                        if super::error::is_fatal_scan_error(&mapped) {
                            return Err(mapped);
                        }
                        false
                    }
                }
            } else {
                false
            };
            if !active_dt {
                continue;
            }

            let (depths, bpcs) = read_depth_candidates(tree, item)?;
            let no_depth_info = depths.is_empty() && bpcs.is_empty();
            match dt {
                WIA_DATA_THRESHOLD | WIA_DATA_DITHER => {
                    if no_depth_info || depths.contains(&1) || bpcs.contains(&1) {
                        has_bw1 = true;
                    }
                }
                WIA_DATA_GRAYSCALE => {
                    if no_depth_info || depths.contains(&8) || bpcs.contains(&8) {
                        has_gray8 = true;
                    }
                    if allow_16bit && (depths.contains(&16) || bpcs.contains(&16)) {
                        has_gray16 = true;
                    }
                    if (depths.contains(&1) && !depths.contains(&8))
                        || (depths.is_empty() && bpcs.contains(&1) && !bpcs.contains(&8))
                    {
                        has_bw1 = true;
                    }
                }
                WIA_DATA_COLOR => {
                    if no_depth_info
                        || depths.contains(&24)
                        || depths.contains(&32)
                        || depths.contains(&8)
                        || bpcs.contains(&8)
                    {
                        has_rgb8 = true;
                    }
                    if allow_16bit
                        && (depths.contains(&48) || depths.contains(&16) || bpcs.contains(&16))
                    {
                        has_rgb16 = true;
                    }
                }
                WIA_DATA_RAW_RGB | WIA_DATA_RAW_BGR if allow_16bit => {
                    if no_depth_info
                        || depths.contains(&24)
                        || depths.contains(&8)
                        || bpcs.contains(&8)
                    {
                        has_rgb8 = true;
                    }
                    if depths.contains(&48) || depths.contains(&16) || bpcs.contains(&16) {
                        has_rgb16 = true;
                    }
                }
                _ => {}
            }
        }
        Ok(())
    })();

    if changed_dt {
        if let Some(dt) = init_dt {
            let _ = tree.write_prop(item, WIA_IPA_DATATYPE, &PropValue::I4(dt));
        }
        if let Some(depth) = init_depth {
            let _ = tree.write_prop(item, WIA_IPA_DEPTH, &PropValue::I4(depth));
        }
    }
    probe_res?;

    let mut out = Vec::new();
    if has_bw1 {
        out.push(Format::Bw1);
    }
    if has_gray8 {
        out.push(Format::Gray8);
    }
    if has_gray16 {
        out.push(Format::Gray16);
    }
    if has_rgb8 {
        out.push(Format::Rgb8);
    }
    if has_rgb16 {
        out.push(Format::Rgb16);
    }
    Ok(out)
}

fn read_depth_candidates(
    tree: &mut dyn WiaDeviceTree,
    item: ItemId,
) -> Result<(Vec<i32>, Vec<i32>), ScanError> {
    let vals = tree.read_props(item, &[WIA_IPA_DEPTH, WIA_IPA_BITS_PER_CHANNEL])?;
    let attrs = tree.prop_attrs(item, &[WIA_IPA_DEPTH, WIA_IPA_BITS_PER_CHANNEL])?;

    let mut depths = Vec::new();
    let depth_attr = attrs.first().and_then(|a| a.as_ref());
    if depth_attr.is_some_and(PropAttr::is_writable)
        && let Some(attr) = depth_attr
    {
        collect_depth_constraint(&attr.constraint, &mut depths);
    }
    if let Some(cur_d) = vals
        .first()
        .and_then(|v| v.as_ref())
        .and_then(PropValue::as_i4)
        && !depths.contains(&cur_d)
    {
        depths.push(cur_d);
    }

    // `WIA_IPA_DEPTH` is the authoritative negotiable depth property in WIA 2.0; consult
    // `WIA_IPA_BITS_PER_CHANNEL` only when `WIA_IPA_DEPTH` is absent so a stale read-only channel
    // depth never invents an unsupported bit depth alongside an explicit `WIA_IPA_DEPTH` list.
    let mut bpcs = Vec::new();
    if depths.is_empty() {
        let bpc_attr = attrs.get(1).and_then(|a| a.as_ref());
        if bpc_attr.is_some_and(PropAttr::is_writable)
            && let Some(attr) = bpc_attr
        {
            collect_depth_constraint(&attr.constraint, &mut bpcs);
        }
        if let Some(cur_b) = vals
            .get(1)
            .and_then(|v| v.as_ref())
            .and_then(PropValue::as_i4)
            && !bpcs.contains(&cur_b)
        {
            bpcs.push(cur_b);
        }
    }

    Ok((depths, bpcs))
}

fn collect_constraint_i4(constraint: &PropConstraint, out: &mut Vec<i32>) {
    match constraint {
        PropConstraint::ListI4 { values, .. } => {
            for &v in values {
                if !out.contains(&v) {
                    out.push(v);
                }
            }
        }
        PropConstraint::Range { min, max, step, .. }
            if *min <= *max && (i64::from(*max) - i64::from(*min)) <= 64 =>
        {
            let s = (*step).max(1);
            let mut v = *min;
            while v <= *max {
                if !out.contains(&v) {
                    out.push(v);
                }
                match v.checked_add(s) {
                    Some(next) => v = next,
                    None => break,
                }
            }
        }
        _ => {}
    }
}

fn collect_depth_constraint(constraint: &PropConstraint, out: &mut Vec<i32>) {
    match constraint {
        PropConstraint::ListI4 { values, .. } => {
            for &v in values {
                if !out.contains(&v) {
                    out.push(v);
                }
            }
        }
        PropConstraint::Range { min, max, step, .. } if *min <= *max => {
            for cand in [1, 8, 16, 24, 32, 48] {
                if cand >= *min && cand <= *max {
                    let ok = if *step <= 0 {
                        cand == *min
                    } else {
                        (i64::from(cand) - i64::from(*min)) % i64::from(*step) == 0
                    };
                    if ok && !out.contains(&cand) {
                        out.push(cand);
                    }
                }
            }
        }
        _ => {}
    }
}

/// Reads [`Resolutions`] from `WIA_IPS_XRES` (falling back to `WIA_IPS_OPTICAL_XRES`), upholding
/// every model and `scan-worker` invariant (`> 0`, `min <= max`, `step > 0`).
pub(super) fn read_resolutions_for_item(
    tree: &mut dyn WiaDeviceTree,
    item: ItemId,
) -> Result<Option<Resolutions>, ScanError> {
    read_resolution_prop(tree, item, WIA_IPS_XRES, WIA_IPS_OPTICAL_XRES)
}

/// Reads [`Resolutions`] from `WIA_IPS_YRES` (falling back to `WIA_IPS_OPTICAL_YRES`).
pub(super) fn read_y_resolutions_for_item(
    tree: &mut dyn WiaDeviceTree,
    item: ItemId,
) -> Result<Option<Resolutions>, ScanError> {
    read_resolution_prop(tree, item, WIA_IPS_YRES, WIA_IPS_OPTICAL_YRES)
}

fn read_resolution_prop(
    tree: &mut dyn WiaDeviceTree,
    item: ItemId,
    primary: PropId,
    optical_fallback: PropId,
) -> Result<Option<Resolutions>, ScanError> {
    let vals = tree.read_props(item, &[primary, optical_fallback])?;
    let attrs = tree.prop_attrs(item, &[primary])?;

    let cur = vals
        .first()
        .and_then(|v| v.as_ref())
        .and_then(PropValue::as_i4)
        .filter(|&v| v > 0);
    let optical = vals
        .get(1)
        .and_then(|v| v.as_ref())
        .and_then(PropValue::as_i4)
        .filter(|&v| v > 0);
    let attr = attrs.first().and_then(|a| a.as_ref());

    if let Some(attr) = attr
        && attr.is_writable()
    {
        match &attr.constraint {
            PropConstraint::ListI4 { values, .. } => {
                let mut list: Vec<u32> = values
                    .iter()
                    .copied()
                    .filter(|&v| v > 0)
                    .map(|v| v as u32)
                    .collect();
                list.sort_unstable();
                list.dedup();
                if !list.is_empty() {
                    return Ok(Some(Resolutions::List(list)));
                }
            }
            PropConstraint::Range {
                min,
                nom,
                max,
                step,
            } => {
                if let Some(res) = sanitize_dpi_range(*min, *nom, *max, *step, cur) {
                    return Ok(Some(res));
                }
            }
            PropConstraint::None
            | PropConstraint::ListGuid { .. }
            | PropConstraint::Flags { .. } => {}
        }
    }

    if let Some(dpi) = cur.or(optical) {
        return Ok(Some(Resolutions::List(vec![dpi as u32])));
    }

    Ok(None)
}

fn sanitize_dpi_range(
    raw_min: i32,
    raw_nom: i32,
    raw_max: i32,
    raw_step: i32,
    current: Option<i32>,
) -> Option<Resolutions> {
    if raw_max <= 0 || raw_min > raw_max {
        return None;
    }
    if raw_step <= 0 || raw_min == raw_max {
        let single = current
            .filter(|&c| c > 0 && c >= raw_min && c <= raw_max)
            .or_else(|| {
                (raw_nom > 0 && raw_nom >= raw_min && raw_nom <= raw_max).then_some(raw_nom)
            })
            .or_else(|| (raw_min > 0).then_some(raw_min))
            .unwrap_or(raw_max);
        return (single > 0).then(|| Resolutions::List(vec![single as u32]));
    }

    let step_u64 = raw_step as u64;
    let max_u64 = raw_max as u64;
    let min_u64 = if raw_min > 0 {
        raw_min as u64
    } else {
        // Advance from `raw_min <= 0` by multiples of `raw_step` to the first positive value `>= 1`.
        let neg_dist = (1i64 - i64::from(raw_min)) as u64;
        let k = neg_dist.div_ceil(step_u64);
        let first_pos = i64::from(raw_min) + (k as i64) * i64::from(raw_step);
        if first_pos <= 0 || (first_pos as u64) > max_u64 {
            return None;
        }
        first_pos as u64
    };

    if min_u64 > max_u64 {
        return None;
    }
    let snapped_max = min_u64 + (max_u64 - min_u64) / step_u64 * step_u64;
    let min_u32 = u32::try_from(min_u64).ok()?;
    let max_u32 = u32::try_from(snapped_max).ok()?;
    if min_u32 == max_u32 {
        return Some(Resolutions::List(vec![min_u32]));
    }
    let step_nz = NonZeroU32::new(u32::try_from(step_u64).ok()?)?;
    Some(Resolutions::Range {
        min: min_u32,
        max: max_u32,
        step: step_nz,
    })
}

fn find_handling_item(tree: &mut dyn WiaDeviceTree, item: ItemId) -> Result<ItemId, ScanError> {
    let item_val = tree.read_props(item, &[WIA_IPS_DOCUMENT_HANDLING_SELECT])?;
    if item_val.first().is_some_and(Option::is_some) {
        return Ok(item);
    }
    let item_attr = tree.prop_attrs(item, &[WIA_IPS_DOCUMENT_HANDLING_SELECT])?;
    if item_attr.first().is_some_and(Option::is_some) {
        return Ok(item);
    }
    Ok(ItemId::ROOT)
}

fn read_feeder_duplex(
    tree: &mut dyn WiaDeviceTree,
    item: ItemId,
    has_back_child: bool,
) -> Result<(bool, ItemId), ScanError> {
    let item_props = tree.read_props(
        item,
        &[
            WIA_DPS_DOCUMENT_HANDLING_CAPABILITIES,
            WIA_IPS_DOCUMENT_HANDLING_SELECT,
        ],
    )?;
    let root_props = tree.read_props(
        ItemId::ROOT,
        &[
            WIA_DPS_DOCUMENT_HANDLING_CAPABILITIES,
            WIA_IPS_DOCUMENT_HANDLING_SELECT,
        ],
    )?;
    let item_attrs = tree.prop_attrs(item, &[WIA_IPS_DOCUMENT_HANDLING_SELECT])?;
    let root_attrs = tree.prop_attrs(ItemId::ROOT, &[WIA_IPS_DOCUMENT_HANDLING_SELECT])?;

    let item_sel_val = item_props
        .get(1)
        .and_then(|v| v.as_ref())
        .and_then(PropValue::as_i4);
    let item_sel_attr = item_attrs.first().and_then(|a| a.as_ref());
    let root_sel_val = root_props
        .get(1)
        .and_then(|v| v.as_ref())
        .and_then(PropValue::as_i4);
    let root_sel_attr = root_attrs.first().and_then(|a| a.as_ref());

    let (handling_item, sel_val, sel_attr) = if item_sel_val.is_some() || item_sel_attr.is_some() {
        (item, item_sel_val, item_sel_attr)
    } else {
        (ItemId::ROOT, root_sel_val, root_sel_attr)
    };

    let hw_caps = item_props
        .first()
        .and_then(|v| v.as_ref())
        .and_then(PropValue::as_i4)
        .or_else(|| {
            root_props
                .first()
                .and_then(|v| v.as_ref())
                .and_then(PropValue::as_i4)
        });

    if let Some(caps_bits) = hw_caps
        && (caps_bits & DUP) == 0
    {
        return Ok((false, handling_item));
    }

    let select_allows_duplex = match (sel_val, sel_attr) {
        (cur, Some(attr)) => {
            if attr.is_writable() {
                match &attr.constraint {
                    PropConstraint::Flags { valid_bits, .. } => (*valid_bits & DUPLEX) != 0,
                    PropConstraint::ListI4 { values, .. } => {
                        values.iter().any(|&v| (v & DUPLEX) != 0)
                    }
                    PropConstraint::None => {
                        hw_caps.is_some_and(|c| (c & DUP) != 0) || has_back_child
                    }
                    PropConstraint::Range { .. } | PropConstraint::ListGuid { .. } => false,
                }
            } else {
                cur.is_some_and(|c| (c & DUPLEX) != 0)
            }
        }
        (Some(cur), None) => (cur & DUPLEX) != 0,
        (None, None) => false,
    };

    let hw_indicates_duplex =
        hw_caps.is_some_and(|c| (c & DUP) != 0) || has_back_child || select_allows_duplex;
    Ok((hw_indicates_duplex && select_allows_duplex, handling_item))
}

fn thousandths_inch_to_mm(v: i32) -> Option<f32> {
    if v <= 0 {
        return None;
    }
    let mm = (f64::from(v) * 25.4 / 1000.0) as f32;
    (mm.is_finite() && mm > 0.0).then_some(mm)
}

/// Reads positive finite `max_area` in millimetres from item `WIA_IPS_MAX_*_SIZE` (`1/1000` inch),
/// falling back to root `WIA_DPS_*_SIZE` (`1/1000` inch), and finally to `XEXTENT`/`YEXTENT` `max`
/// pixels at the item's current `(XRES, YRES)`.
pub(super) fn read_max_area_mm(
    tree: &mut dyn WiaDeviceTree,
    item: ItemId,
    source: Source,
) -> Result<Option<SizeMm>, ScanError> {
    let item_sizes = tree.read_props(
        item,
        &[WIA_IPS_MAX_HORIZONTAL_SIZE, WIA_IPS_MAX_VERTICAL_SIZE],
    )?;
    let item_w = item_sizes
        .first()
        .and_then(|v| v.as_ref())
        .and_then(PropValue::as_i4)
        .and_then(thousandths_inch_to_mm);
    let item_h = item_sizes
        .get(1)
        .and_then(|v| v.as_ref())
        .and_then(PropValue::as_i4)
        .and_then(thousandths_inch_to_mm);
    if let (Some(width), Some(height)) = (item_w, item_h) {
        return Ok(Some(SizeMm { width, height }));
    }

    let (root_h_prop, root_v_prop) = match source {
        Source::Flatbed => (WIA_DPS_HORIZONTAL_BED_SIZE, WIA_DPS_VERTICAL_BED_SIZE),
        Source::Feeder => (
            WIA_DPS_HORIZONTAL_SHEET_FEED_SIZE,
            WIA_DPS_VERTICAL_SHEET_FEED_SIZE,
        ),
    };
    let root_sizes = tree.read_props(ItemId::ROOT, &[root_h_prop, root_v_prop])?;
    let root_w = item_w.or_else(|| {
        root_sizes
            .first()
            .and_then(|v| v.as_ref())
            .and_then(PropValue::as_i4)
            .and_then(thousandths_inch_to_mm)
    });
    let root_h = item_h.or_else(|| {
        root_sizes
            .get(1)
            .and_then(|v| v.as_ref())
            .and_then(PropValue::as_i4)
            .and_then(thousandths_inch_to_mm)
    });
    if let (Some(width), Some(height)) = (root_w, root_h) {
        return Ok(Some(SizeMm { width, height }));
    }

    // Fallback: derive from `(XEXTENT.max, YEXTENT.max)` at current `(XRES, YRES)` (with optical
    // DPI fallback).
    let ext_vals = tree.read_props(
        item,
        &[
            WIA_IPS_XRES,
            WIA_IPS_YRES,
            WIA_IPS_OPTICAL_XRES,
            WIA_IPS_OPTICAL_YRES,
            WIA_IPS_XEXTENT,
            WIA_IPS_YEXTENT,
        ],
    )?;
    let ext_attrs = tree.prop_attrs(item, &[WIA_IPS_XEXTENT, WIA_IPS_YEXTENT])?;
    let xres = ext_vals
        .first()
        .and_then(|v| v.as_ref())
        .and_then(PropValue::as_i4)
        .filter(|&v| v > 0)
        .or_else(|| {
            ext_vals
                .get(2)
                .and_then(|v| v.as_ref())
                .and_then(PropValue::as_i4)
                .filter(|&v| v > 0)
        });
    let yres = ext_vals
        .get(1)
        .and_then(|v| v.as_ref())
        .and_then(PropValue::as_i4)
        .filter(|&v| v > 0)
        .or_else(|| {
            ext_vals
                .get(3)
                .and_then(|v| v.as_ref())
                .and_then(PropValue::as_i4)
                .filter(|&v| v > 0)
        })
        .or(xres);

    let max_px = |attr: Option<&PropAttr>, cur: Option<i32>| -> Option<i32> {
        match attr.map(|a| &a.constraint) {
            Some(PropConstraint::Range { max, .. }) if *max > 0 => Some(*max),
            Some(PropConstraint::ListI4 { values, .. }) => {
                values.iter().copied().filter(|&v| v > 0).max()
            }
            _ => cur.filter(|&v| v > 0),
        }
    };

    let x_px = max_px(
        ext_attrs.first().and_then(|a| a.as_ref()),
        ext_vals
            .get(4)
            .and_then(|v| v.as_ref())
            .and_then(PropValue::as_i4),
    );
    let y_px = max_px(
        ext_attrs.get(1).and_then(|a| a.as_ref()),
        ext_vals
            .get(5)
            .and_then(|v| v.as_ref())
            .and_then(PropValue::as_i4),
    );

    let px_to_mm = |px: Option<i32>, dpi: Option<i32>| -> Option<f32> {
        let (px, dpi) = (px?, dpi?);
        let mm = (f64::from(px) / f64::from(dpi) * 25.4) as f32;
        (mm.is_finite() && mm > 0.0).then_some(mm)
    };

    let width = root_w.or_else(|| px_to_mm(x_px, xres));
    let height = root_h.or_else(|| px_to_mm(y_px, yres));
    match (width, height) {
        (Some(width), Some(height)) => Ok(Some(SizeMm { width, height })),
        _ => Ok(None),
    }
}

/// Checks whether `item` supports automatic paper-size / border detection via `WIA_IPS_AUTO_CROP`
/// or `WIA_IPS_PAGE_SIZE == WIA_PAGE_AUTO`.
pub(super) fn read_paper_auto(
    tree: &mut dyn WiaDeviceTree,
    item: ItemId,
) -> Result<bool, ScanError> {
    let vals = tree.read_props(item, &[WIA_IPS_AUTO_CROP, WIA_IPS_PAGE_SIZE])?;
    let attrs = tree.prop_attrs(item, &[WIA_IPS_AUTO_CROP, WIA_IPS_PAGE_SIZE])?;

    let crop_val = vals
        .first()
        .and_then(|v| v.as_ref())
        .and_then(PropValue::as_i4);
    let crop_attr = attrs.first().and_then(|a| a.as_ref());
    let auto_crop_ok = match (crop_val, crop_attr) {
        (cur, Some(attr)) => {
            if attr.is_writable() {
                constraint_allows_i4(&attr.constraint, WIA_AUTO_CROP_SINGLE, true)
            } else {
                cur.is_some_and(|c| c != 0)
            }
        }
        (Some(cur), None) => cur != 0,
        (None, None) => false,
    };
    if auto_crop_ok {
        return Ok(true);
    }

    let page_val = vals
        .get(1)
        .and_then(|v| v.as_ref())
        .and_then(PropValue::as_i4);
    let page_attr = attrs.get(1).and_then(|a| a.as_ref());
    let page_auto_ok = match (page_val, page_attr) {
        (cur, Some(attr)) => {
            if attr.is_writable() {
                // Require explicit listing of `WIA_PAGE_AUTO` (`100`) rather than unconstrained `None`.
                constraint_allows_i4(&attr.constraint, WIA_PAGE_AUTO, false)
            } else {
                cur == Some(WIA_PAGE_AUTO)
            }
        }
        (Some(cur), None) => cur == WIA_PAGE_AUTO,
        (None, None) => false,
    };
    Ok(page_auto_ok)
}

/// Checks whether `constraint` permits integer `target` (`allow_unconstrained` controls
/// `PropConstraint::None`).
pub(super) fn constraint_allows_i4(
    constraint: &PropConstraint,
    target: i32,
    allow_unconstrained: bool,
) -> bool {
    match constraint {
        PropConstraint::None => allow_unconstrained,
        PropConstraint::ListI4 { values, .. } => values.contains(&target),
        PropConstraint::Range { min, max, step, .. } => {
            if target < *min || target > *max {
                return false;
            }
            if *step <= 0 {
                target == *min
            } else {
                (i64::from(target) - i64::from(*min)) % i64::from(*step) == 0
            }
        }
        PropConstraint::Flags { valid_bits, .. } => (target & !*valid_bits) == 0,
        PropConstraint::ListGuid { .. } => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wia::driver::fake_wia::{FakeDeviceSpec, FakeItem, FakeWiaHandle};
    use crate::wia::driver::{
        FEED, FLAT, FRONT_ONLY, WIA_AUTO_CROP_DISABLED, WIA_ITEM_TYPE_ROOT, WIA_PAGE_CUSTOM,
        WIA_PROP_FLAG, WIA_PROP_LIST, WIA_PROP_RANGE, WIA_PROP_READ, WIA_PROP_RW, WiaSystem,
    };
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

    #[test]
    fn discovers_flatbed_and_feeder_deduplicates_prefers_feeder_guid_and_ignores_back_and_film() {
        let fake = FakeWiaHandle::new();
        let mut spec = FakeDeviceSpec::new(scanner_props());
        spec.items[0] = FakeItem::new(WIA_ITEM_TYPE_ROOT | WIA_ITEM_TYPE_FOLDER)
            .with_prop(
                WIA_DPS_DOCUMENT_HANDLING_CAPABILITIES,
                PropValue::I4(FLAT | FEED | DUP),
                None,
            )
            .with_prop(WIA_DPS_HORIZONTAL_BED_SIZE, PropValue::I4(8500), None)
            .with_prop(WIA_DPS_VERTICAL_BED_SIZE, PropValue::I4(11000), None);

        // Folder containing Feeder Front, Feeder Back, and primary Feeder; plus Film and Flatbed.
        let folder_idx = spec.add_child(
            ItemId::ROOT,
            FakeItem::new(WIA_ITEM_TYPE_FOLDER).with_prop(
                WIA_IPA_ITEM_CATEGORY,
                PropValue::Guid(WIA_CATEGORY_FOLDER_GUID),
                None,
            ),
        );

        let make_source_item = |cat: GuidBytes, dpi_list: Vec<i32>, raw_ok: bool| {
            let mut formats = vec![
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
            ];
            if raw_ok {
                formats.push(FormatPair {
                    format: WIA_IMG_FMT_RAW_GUID,
                    tymed: TYMED_FILE,
                });
            }
            FakeItem::new(WIA_ITEM_TYPE_IMAGE | WIA_ITEM_TYPE_TRANSFER)
                .with_prop(WIA_IPA_ITEM_CATEGORY, PropValue::Guid(cat), None)
                .with_prop(
                    WIA_IPS_XRES,
                    PropValue::I4(dpi_list[0]),
                    rw_list(dpi_list[0], dpi_list),
                )
                .with_prop(
                    WIA_IPA_DATATYPE,
                    PropValue::I4(WIA_DATA_COLOR),
                    rw_list(
                        WIA_DATA_COLOR,
                        vec![WIA_DATA_THRESHOLD, WIA_DATA_GRAYSCALE, WIA_DATA_COLOR],
                    ),
                )
                .with_prop(
                    WIA_IPA_DEPTH,
                    PropValue::I4(24),
                    rw_list(24, vec![1, 8, 16, 24, 48]),
                )
                .with_formats(formats)
        };

        // Feeder Front (lower priority: 200 dpi only, no RAW)
        spec.add_child(
            folder_idx,
            make_source_item(WIA_CATEGORY_FEEDER_FRONT_GUID, vec![200], false),
        );
        // Feeder Back (must NEVER be treated as a source!)
        spec.add_child(
            folder_idx,
            make_source_item(WIA_CATEGORY_FEEDER_BACK_GUID, vec![600], true).with_prop(
                WIA_IPA_ITEM_NAME,
                PropValue::Str("Feeder Back".to_owned()),
                None,
            ),
        );
        // Primary WIA_CATEGORY_FEEDER (higher priority: [0, 300, 150, 300] dpi, with RAW + duplex + auto_crop)
        let feeder_item = make_source_item(WIA_CATEGORY_FEEDER_GUID, vec![0, 300, 150, 300], true)
            .with_prop(
                WIA_IPS_DOCUMENT_HANDLING_SELECT,
                PropValue::I4(FRONT_ONLY),
                rw_flags(FRONT_ONLY, FRONT_ONLY | DUPLEX),
            )
            .with_prop(
                WIA_IPS_AUTO_CROP,
                PropValue::I4(WIA_AUTO_CROP_DISABLED),
                rw_list(
                    WIA_AUTO_CROP_DISABLED,
                    vec![WIA_AUTO_CROP_DISABLED, WIA_AUTO_CROP_SINGLE],
                ),
            )
            .with_prop(WIA_IPS_MAX_HORIZONTAL_SIZE, PropValue::I4(8500), None)
            .with_prop(WIA_IPS_MAX_VERTICAL_SIZE, PropValue::I4(14000), None);
        spec.add_child(folder_idx, feeder_item);

        // Film item (must be ignored)
        spec.add_child(
            ItemId::ROOT,
            make_source_item(WIA_CATEGORY_FILM_GUID, vec![1200], true),
        );
        // Flatbed item (BMP + PNG + JPEG only, NO WiaImgFmt_RAW -> 16-bit formats gated off!)
        spec.add_child(
            ItemId::ROOT,
            make_source_item(WIA_CATEGORY_FLATBED_GUID, vec![100, 300], false),
        );

        fake.add_device(spec);
        let mut sys = fake.make_system();
        let (_, mut tree) = sys.open_device(r"{6BDD1FC6}\0000").unwrap();
        let caps = read(&mut *tree).unwrap();

        assert!(!caps.settings_window);
        assert_eq!(caps.sources.len(), 2);

        let fb = &caps.sources[0];
        assert_eq!(fb.source, Source::Flatbed);
        assert!(!fb.duplex);
        assert!(!fb.paper_auto);
        assert_eq!(fb.resolutions, Resolutions::List(vec![100, 300]));
        assert_eq!(fb.formats, vec![Format::Bw1, Format::Gray8, Format::Rgb8]);
        assert_eq!(
            fb.encodings,
            vec![Encoding::Png, Encoding::Bmp, Encoding::Jpeg]
        );
        assert_eq!(fb.max_area, Some(SizeMm::LETTER));

        let adf = &caps.sources[1];
        assert_eq!(adf.source, Source::Feeder);
        assert!(adf.duplex);
        assert!(adf.paper_auto);
        // Zero DPI filtered out, sorted and deduplicated `[150, 300]`.
        assert_eq!(adf.resolutions, Resolutions::List(vec![150, 300]));
        // `WiaImgFmt_RAW` present -> 16-bit Gray16 and Rgb16 included in canonical order!
        assert_eq!(
            adf.formats,
            vec![
                Format::Bw1,
                Format::Gray8,
                Format::Gray16,
                Format::Rgb8,
                Format::Rgb16,
            ]
        );
        assert_eq!(adf.max_area, Some(SizeMm::LEGAL));
    }

    #[test]
    fn range_resolutions_guard_zero_step_zero_min_and_empty_sources_return_unsupported() {
        // Zero step collapses to positive nominal/min list instead of panicking on NonZeroU32.
        assert_eq!(
            sanitize_dpi_range(100, 300, 600, 0, Some(300)),
            Some(Resolutions::List(vec![300]))
        );
        // Zero min advances by step to first positive step.
        assert_eq!(
            sanitize_dpi_range(0, 300, 600, 75, Some(300)),
            Some(Resolutions::Range {
                min: 75,
                max: 600,
                step: NonZeroU32::new(75).unwrap(),
            })
        );
        // Invalid `min > max` or `max <= 0` returns None.
        assert_eq!(sanitize_dpi_range(600, 300, 100, 50, None), None);
        assert_eq!(sanitize_dpi_range(0, 0, 0, 0, None), None);

        // Device with zero usable child sources returns `ScanError::Unsupported`.
        let fake = FakeWiaHandle::new();
        let mut spec = FakeDeviceSpec::new(scanner_props());
        spec.add_child(
            ItemId::ROOT,
            FakeItem::new(WIA_ITEM_TYPE_IMAGE | WIA_ITEM_TYPE_TRANSFER).with_prop(
                WIA_IPA_ITEM_CATEGORY,
                PropValue::Guid(WIA_CATEGORY_FILM_GUID),
                None,
            ),
        );
        fake.add_device(spec);
        let mut sys = fake.make_system();
        let (_, mut tree) = sys.open_device(r"{6BDD1FC6}\0000").unwrap();
        assert_eq!(read(&mut *tree), Err(ScanError::Unsupported));

        // Read-only `WIA_IPA_DATATYPE` advertises only the active datatype, never non-writable ones,
        // and `<= 0` physical sizes map to `max_area: None` while name fallback recognizes `"ADF"`.
        let fake2 = FakeWiaHandle::new();
        let mut spec2 = FakeDeviceSpec::new(scanner_props());
        spec2.add_child(
            ItemId::ROOT,
            FakeItem::new(WIA_ITEM_TYPE_IMAGE | WIA_ITEM_TYPE_TRANSFER)
                .with_prop(WIA_IPA_ITEM_NAME, PropValue::Str("ADF".to_owned()), None)
                .with_prop(WIA_IPS_XRES, PropValue::I4(300), rw_range(75, 300, 610, 25))
                .with_prop(
                    WIA_IPA_DATATYPE,
                    PropValue::I4(WIA_DATA_GRAYSCALE),
                    Some(PropAttr {
                        access_flags: WIA_PROP_READ | WIA_PROP_LIST,
                        constraint: PropConstraint::ListI4 {
                            nom: WIA_DATA_GRAYSCALE,
                            values: vec![WIA_DATA_THRESHOLD, WIA_DATA_GRAYSCALE, WIA_DATA_COLOR],
                        },
                    }),
                )
                .with_prop(WIA_IPA_DEPTH, PropValue::I4(8), None)
                .with_prop(WIA_IPS_MAX_HORIZONTAL_SIZE, PropValue::I4(0), None)
                .with_prop(WIA_IPS_MAX_VERTICAL_SIZE, PropValue::I4(-1), None)
                .with_prop(
                    WIA_IPS_PAGE_SIZE,
                    PropValue::I4(WIA_PAGE_CUSTOM),
                    rw_list(WIA_PAGE_CUSTOM, vec![WIA_PAGE_CUSTOM, WIA_PAGE_AUTO]),
                )
                .with_formats(vec![FormatPair {
                    format: WIA_IMG_FMT_BMP_GUID,
                    tymed: TYMED_FILE,
                }]),
        );
        fake2.add_device(spec2);
        let mut sys2 = fake2.make_system();
        let (_, mut tree2) = sys2.open_device(r"{6BDD1FC6}\0000").unwrap();
        let caps2 = read(&mut *tree2).unwrap();
        assert_eq!(caps2.sources.len(), 1);
        assert_eq!(caps2.sources[0].source, Source::Feeder);
        assert_eq!(caps2.sources[0].formats, vec![Format::Gray8]);
        assert_eq!(caps2.sources[0].max_area, None);
        assert!(caps2.sources[0].paper_auto);
        assert_eq!(
            caps2.sources[0].resolutions,
            Resolutions::Range {
                min: 75,
                max: 600,
                step: NonZeroU32::new(25).unwrap(),
            }
        );
    }

    #[test]
    fn sibling_feeder_back_marks_duplex_and_depth_stays_authoritative_over_bpc() {
        // Extreme `i32` ranges in constraint helpers never overflow signed subtraction.
        let mut extreme_out = Vec::new();
        collect_constraint_i4(
            &PropConstraint::Range {
                min: i32::MIN,
                nom: 0,
                max: i32::MAX,
                step: 1,
            },
            &mut extreme_out,
        );
        assert!(extreme_out.is_empty());
        collect_depth_constraint(
            &PropConstraint::Range {
                min: i32::MIN,
                nom: 8,
                max: i32::MAX,
                step: 8,
            },
            &mut extreme_out,
        );
        assert_eq!(extreme_out, vec![8, 16, 24, 32, 48]);

        // Sibling `Feeder Front` + `Feeder Back` directly under ROOT (unconstrained handling
        // select, no `WIA_DPS_DOCUMENT_HANDLING_CAPABILITIES`) detects `duplex: true`, and an
        // item with `WIA_IPA_DEPTH = [24]` + stale read-only `WIA_IPA_BITS_PER_CHANNEL = 16`
        // advertises `[Rgb8]` only (not `Rgb16`).
        let fake = FakeWiaHandle::new();
        let mut spec = FakeDeviceSpec::new(scanner_props());
        spec.add_child(
            ItemId::ROOT,
            FakeItem::new(WIA_ITEM_TYPE_IMAGE | WIA_ITEM_TYPE_TRANSFER)
                .with_prop(
                    WIA_IPA_ITEM_CATEGORY,
                    PropValue::Guid(WIA_CATEGORY_FEEDER_FRONT_GUID),
                    None,
                )
                .with_prop(
                    WIA_IPS_DOCUMENT_HANDLING_SELECT,
                    PropValue::I4(FRONT_ONLY),
                    Some(PropAttr {
                        access_flags: WIA_PROP_RW,
                        constraint: PropConstraint::None,
                    }),
                )
                .with_prop(WIA_IPS_OPTICAL_XRES, PropValue::I4(300), None)
                .with_prop(WIA_IPS_XEXTENT, PropValue::I4(2550), None)
                .with_prop(WIA_IPS_YEXTENT, PropValue::I4(3300), None)
                .with_prop(WIA_IPA_DATATYPE, PropValue::I4(WIA_DATA_COLOR), None)
                .with_prop(WIA_IPA_DEPTH, PropValue::I4(24), rw_list(24, vec![24]))
                .with_prop(WIA_IPA_BITS_PER_CHANNEL, PropValue::I4(16), None)
                .with_formats(vec![FormatPair {
                    format: WIA_IMG_FMT_RAW_GUID,
                    tymed: TYMED_FILE,
                }]),
        );
        spec.add_child(
            ItemId::ROOT,
            FakeItem::new(WIA_ITEM_TYPE_IMAGE | WIA_ITEM_TYPE_TRANSFER).with_prop(
                WIA_IPA_ITEM_CATEGORY,
                PropValue::Guid(WIA_CATEGORY_FEEDER_BACK_GUID),
                None,
            ),
        );
        fake.add_device(spec);
        let mut sys = fake.make_system();
        let (_, mut tree) = sys.open_device(r"{6BDD1FC6}\0000").unwrap();
        let caps = read(&mut *tree).unwrap();
        assert_eq!(caps.sources.len(), 1);
        assert_eq!(caps.sources[0].source, Source::Feeder);
        assert!(caps.sources[0].duplex);
        assert_eq!(caps.sources[0].formats, vec![Format::Rgb8]);
        assert_eq!(caps.sources[0].max_area, Some(SizeMm::LETTER));
    }
}
