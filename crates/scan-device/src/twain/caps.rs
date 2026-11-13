//! Reading a TWAIN source's capabilities ([`Caps`] / [`SourceCaps`]), unit conversions across all
//! `ICAP_UNITS` values, and shared capability helpers for batch settings negotiation.

use std::num::NonZeroU32;

use twain_dsm::{Container, Fix32, Frame, Item, ItemType, ffi};

use super::error::driver_msg;
use crate::{Caps, Format, Resolutions, ScanError, SizeMm, Source, SourceCaps};

/// Fallback resolution when a source exposes no `ICAP_XRESOLUTION` (or reports unitless `1.0` in
/// `TWUN_PIXELS`), matching `SaneBackend`.
pub(super) const NO_RESOLUTION_DPI: u32 = 72;

/// Outcome of a single TWAIN capability or layout call.
#[derive(Debug)]
pub(super) enum CapError {
    /// Capability unsupported, read-only, or value refused by the source.
    Refused,
    /// Device-level failure (offline, busy, jammed, cover open, access denied, or a driver error
    /// dialog already shown) that must abort immediately.
    Fatal(ScanError),
}

/// Converts optional-capability probes: `Refused` becomes `Ok(None)`, `Fatal` aborts with `Err`.
pub(super) fn opt<T>(res: Result<T, CapError>) -> Result<Option<T>, ScanError> {
    match res {
        Ok(val) => Ok(Some(val)),
        Err(CapError::Refused) => Ok(None),
        Err(CapError::Fatal(err)) => Err(err),
    }
}

/// Abstract state-4 TWAIN capability and layout access so `caps` and `settings` are unit-tested
/// across 32-bit and 64-bit targets without a live `.ds` binary.
pub(super) trait CapSource {
    fn get(&mut self, cap: u16) -> Result<Container, CapError>;
    fn get_current(&mut self, cap: u16) -> Result<Container, CapError>;
    fn query_support(&mut self, cap: u16) -> Result<u32, CapError>;
    fn set(&mut self, cap: u16, value: Container) -> Result<(), CapError>;
    fn image_layout(&mut self) -> Result<Frame, CapError>;
    fn default_image_layout(&mut self) -> Result<Frame, CapError>;
    fn reset_image_layout(&mut self) -> Result<Frame, CapError>;
    fn set_image_layout(&mut self, frame: Frame) -> Result<(), CapError>;
}

/// Adapter implementing [`CapSource`] over a live `twain_dsm::Source` on Windows.
#[cfg(windows)]
pub(super) struct LiveSource<'a, 't, T>(pub &'a twain_dsm::Source<'t, T>);

#[cfg(windows)]
impl<T> LiveSource<'_, '_, T> {
    fn classify(err: twain_dsm::Error) -> CapError {
        let mapped = super::error::map(&err);
        if super::error::is_fatal_scan_error(&mapped) {
            CapError::Fatal(mapped)
        } else {
            CapError::Refused
        }
    }
}

#[cfg(windows)]
impl<T> CapSource for LiveSource<'_, '_, T> {
    fn get(&mut self, cap: u16) -> Result<Container, CapError> {
        self.0.get(cap).map_err(Self::classify)
    }

    fn get_current(&mut self, cap: u16) -> Result<Container, CapError> {
        self.0.get_current(cap).map_err(Self::classify)
    }

    fn query_support(&mut self, cap: u16) -> Result<u32, CapError> {
        self.0.query_support(cap).map_err(Self::classify)
    }

    fn set(&mut self, cap: u16, value: Container) -> Result<(), CapError> {
        self.0.set(cap, value).map(|_| ()).map_err(Self::classify)
    }

    fn image_layout(&mut self) -> Result<Frame, CapError> {
        self.0
            .image_layout()
            .map(|l| l.frame)
            .map_err(Self::classify)
    }

    fn default_image_layout(&mut self) -> Result<Frame, CapError> {
        self.0
            .default_image_layout()
            .map(|l| l.frame)
            .map_err(Self::classify)
    }

    fn reset_image_layout(&mut self) -> Result<Frame, CapError> {
        self.0
            .reset_image_layout()
            .map(|l| l.frame)
            .map_err(Self::classify)
    }

    fn set_image_layout(&mut self, frame: Frame) -> Result<(), CapError> {
        self.0
            .set_image_layout(frame)
            .map(|_| ())
            .map_err(Self::classify)
    }
}

/// Active `ICAP_UNITS` of a TWAIN source.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Units {
    Inches,
    Centimeters,
    Picas,
    Points,
    Twips,
    Pixels,
    Millimeters,
}

impl Units {
    /// Parses a `TWUN_*` value.
    pub(super) fn from_twun(raw: u16) -> Option<Units> {
        Some(match raw {
            ffi::TWUN_INCHES => Units::Inches,
            ffi::TWUN_CENTIMETERS => Units::Centimeters,
            ffi::TWUN_PICAS => Units::Picas,
            ffi::TWUN_POINTS => Units::Points,
            ffi::TWUN_TWIPS => Units::Twips,
            ffi::TWUN_PIXELS => Units::Pixels,
            ffi::TWUN_MILLIMETERS => Units::Millimeters,
            _ => return None,
        })
    }

    /// Physical units per inch (`None` for dimensionless `TWUN_PIXELS`).
    pub(super) fn per_inch(self) -> Option<f64> {
        Some(match self {
            Units::Inches => 1.0,
            Units::Centimeters => 2.54,
            Units::Picas => 6.0,
            Units::Points => 72.0,
            Units::Twips => 1440.0,
            Units::Millimeters => 25.4,
            Units::Pixels => return None,
        })
    }

    /// Converts a resolution reported in `self` (pixels per unit) into dots per inch.
    ///
    /// Under `TWUN_PIXELS`, TWAIN defines `ICAP_XRESOLUTION` as dimensionless `1.0`; values `<= 1.0`
    /// map to [`NO_RESOLUTION_DPI`] (`72.0`), while drivers that still report DPI there (`> 1.0`)
    /// keep their value.
    pub(super) fn res_to_dpi(self, res: f64) -> f64 {
        match self.per_inch() {
            Some(scale) => res * scale,
            None => {
                if res > 1.0 {
                    res
                } else {
                    f64::from(NO_RESOLUTION_DPI)
                }
            }
        }
    }

    /// Converts dots per inch into a resolution in `self` (pixels per unit).
    pub(super) fn dpi_to_res(self, dpi: f64) -> f64 {
        match self.per_inch() {
            Some(scale) => dpi / scale,
            None => dpi,
        }
    }

    /// Converts a length in `self` to millimetres, using `dpi` when `self` is `Units::Pixels`.
    pub(super) fn length_to_mm(self, length: f64, dpi: f64) -> Option<f64> {
        let mm = match self.per_inch() {
            Some(scale) => length / scale * 25.4,
            None => {
                if !dpi.is_finite() || dpi <= 0.0 {
                    return None;
                }
                length / dpi * 25.4
            }
        };
        mm.is_finite().then_some(mm)
    }

    /// Converts millimetres into a length in `self`, using `dpi` when `self` is `Units::Pixels`.
    pub(super) fn mm_to_length(self, mm: f64, dpi: f64) -> Option<f64> {
        let len = match self.per_inch() {
            Some(scale) => mm / 25.4 * scale,
            None => {
                if !dpi.is_finite() || dpi <= 0.0 {
                    return None;
                }
                mm / 25.4 * dpi
            }
        };
        len.is_finite().then_some(len)
    }
}

/// Negotiates `ICAP_UNITS` to `TWUN_INCHES` when supported, returning whatever unit is active
/// afterward (so read-only non-inch sources still convert accurately).
pub(super) fn negotiate_units(source: &mut dyn CapSource) -> Result<Units, ScanError> {
    let Some(container) = opt(source.get(ffi::ICAP_UNITS))? else {
        return Ok(Units::Inches);
    };
    let item_type = container_item_type(&container);
    let current = current_int(&container).and_then(|v| u16::try_from(v).ok());
    if current == Some(ffi::TWUN_INCHES) {
        return Ok(Units::Inches);
    }
    if container_allows_int(&container, i64::from(ffi::TWUN_INCHES))
        && let Some(inches_item) = make_int_item(item_type, i64::from(ffi::TWUN_INCHES))
    {
        let _ = opt(source.set(ffi::ICAP_UNITS, Container::One(inches_item)))?;
    }
    let active_raw = match opt(source.get_current(ffi::ICAP_UNITS))? {
        Some(c) => current_int(&c).and_then(|v| u16::try_from(v).ok()),
        None => current,
    };
    match active_raw {
        Some(raw) => Units::from_twun(raw)
            .ok_or_else(|| driver_msg(format!("the scanner uses unknown ICAP_UNITS {raw:#06x}"))),
        None => Ok(Units::Inches),
    }
}

/// Reads full [`Caps`] from an open state-4 source.
pub(super) fn read(source: &mut dyn CapSource, native_transfer: bool) -> Result<Caps, ScanError> {
    let units = negotiate_units(source)?;
    let allow_16bit = !native_transfer && supports_memory_xfer(source)?;
    let settings_window = opt(source.get_current(ffi::CAP_ENABLEDSUIONLY))?
        .or(opt(source.get(ffi::CAP_ENABLEDSUIONLY))?)
        .and_then(|c| current_int(&c))
        .is_some_and(|v| v != 0);

    let feeder_info = probe_feeder(source)?;
    let mut sources = Vec::new();

    match feeder_info {
        FeederCap::Absent => {
            if let Some(sc) = read_one_source(source, Source::Flatbed, units, allow_16bit)? {
                sources.push(sc);
            }
        }
        FeederCap::Fixed { feeder } => {
            let which = if feeder {
                Source::Feeder
            } else {
                Source::Flatbed
            };
            if let Some(sc) = read_one_source(source, which, units, allow_16bit)? {
                sources.push(sc);
            }
        }
        FeederCap::Selectable {
            initial_feeder,
            item_type,
        } => {
            // Read the already-selected source first to minimize capability churn, then toggle and
            // always restore `initial_feeder`.
            let order = if initial_feeder {
                [(Source::Feeder, true), (Source::Flatbed, false)]
            } else {
                [(Source::Flatbed, false), (Source::Feeder, true)]
            };
            let mut active = initial_feeder;
            let mut outcome: Result<(), ScanError> = Ok(());
            for (kind, want_feeder) in order {
                if want_feeder != active
                    && let Some(item) = make_int_item(item_type, i64::from(want_feeder))
                {
                    if let Err(err) = opt(source.set(ffi::CAP_FEEDERENABLED, item.into())) {
                        outcome = Err(err);
                        break;
                    }
                    active = want_feeder;
                }
                match read_one_source(source, kind, units, allow_16bit) {
                    Ok(Some(sc)) => sources.push(sc),
                    Ok(None) => {}
                    Err(err) => {
                        outcome = Err(err);
                        break;
                    }
                }
            }
            if active != initial_feeder
                && let Some(item) = make_int_item(item_type, i64::from(initial_feeder))
            {
                let restore = opt(source.set(ffi::CAP_FEEDERENABLED, item.into())).map(drop);
                outcome = outcome.and(restore);
            }
            outcome?;
            sources.sort_by_key(|sc| sc.source != Source::Flatbed);
        }
    }

    if sources.is_empty() {
        return Err(ScanError::Unsupported);
    }
    let mut caps = Caps::new(sources);
    caps.settings_window = settings_window;
    Ok(caps)
}

/// How `CAP_FEEDERENABLED` behaves on a source.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum FeederCap {
    /// No `CAP_FEEDERENABLED`: flatbed only.
    Absent,
    /// Single fixed source (`false` = flatbed only, `true` = feeder only).
    Fixed { feeder: bool },
    /// Both flatbed (`false`) and feeder (`true`) selectable.
    Selectable {
        initial_feeder: bool,
        item_type: ItemType,
    },
}

/// Probes `CAP_FEEDERENABLED` across both TWAIN 2 (`Enumeration`) and TWAIN 1.x (`One`) sources.
pub(super) fn probe_feeder(source: &mut dyn CapSource) -> Result<FeederCap, ScanError> {
    let Some(container) = opt(source.get(ffi::CAP_FEEDERENABLED))? else {
        return Ok(FeederCap::Absent);
    };
    let item_type = container_item_type(&container);
    let initial = current_int(&container).is_some_and(|v| v != 0);
    match &container {
        Container::Enumeration(_) | Container::Array(_) | Container::Range(_) => {
            let has_false = container_allows_int(&container, 0);
            let has_true = container_allows_int(&container, 1);
            match (has_false, has_true) {
                (true, true) => Ok(FeederCap::Selectable {
                    initial_feeder: initial,
                    item_type,
                }),
                (false, true) => Ok(FeederCap::Fixed { feeder: true }),
                (true, false) | (false, false) => Ok(FeederCap::Fixed { feeder: false }),
            }
        }
        Container::One(_) => {
            // TWAIN 1.x returns `TWON_ONEVALUE` from `MSG_GET` on `BOOL` capabilities. Check
            // `MSG_QUERYSUPPORT` first; if `TWQC_SET` isn't excluded, probe toggling and restore.
            if let Some(qc) = opt(source.query_support(ffi::CAP_FEEDERENABLED))?
                && qc & u32::from(ffi::TWQC_SET) == 0
            {
                return Ok(FeederCap::Fixed { feeder: initial });
            }
            let other = !initial;
            let Some(toggle_item) = make_int_item(item_type, i64::from(other)) else {
                return Ok(FeederCap::Fixed { feeder: initial });
            };
            if opt(source.set(ffi::CAP_FEEDERENABLED, toggle_item.into()))?.is_none() {
                return Ok(FeederCap::Fixed { feeder: initial });
            }
            let toggled = opt(source.get_current(ffi::CAP_FEEDERENABLED))?
                .and_then(|c| current_int(&c))
                .is_some_and(|v| v != 0);
            if let Some(back_item) = make_int_item(item_type, i64::from(initial)) {
                let _ = opt(source.set(ffi::CAP_FEEDERENABLED, back_item.into()))?;
            }
            if toggled == other {
                Ok(FeederCap::Selectable {
                    initial_feeder: initial,
                    item_type,
                })
            } else {
                Ok(FeederCap::Fixed { feeder: initial })
            }
        }
    }
}

/// Reads capabilities for the currently active `source_kind`. Returns `Ok(None)` if the source has
/// no usable format or resolution.
fn read_one_source(
    source: &mut dyn CapSource,
    source_kind: Source,
    units: Units,
    allow_16bit: bool,
) -> Result<Option<SourceCaps>, ScanError> {
    let formats = read_formats(source, allow_16bit)?;
    if formats.is_empty() {
        return Ok(None);
    }
    let Some(resolutions) = read_resolutions(source, units)? else {
        return Ok(None);
    };
    let (across_dpi, down_dpi) = read_current_dpi(source, units)?;
    let max_area = read_max_area(source, units, across_dpi, down_dpi)?;
    let paper_auto = auto_paper_caps(source)?.supported();
    let duplex = if source_kind == Source::Feeder {
        duplex_supported(source)?
    } else {
        false
    };

    let mut sc = SourceCaps::new(source_kind, resolutions, formats);
    sc.duplex = duplex;
    sc.max_area = max_area;
    sc.paper_auto = paper_auto;
    Ok(Some(sc))
}

/// Whether `ICAP_XFERMECH` offers `TWSX_MEMORY` (or is absent, where only `TWSX_NATIVE` is safe to
/// assume -> `false`).
pub(super) fn supports_memory_xfer(source: &mut dyn CapSource) -> Result<bool, ScanError> {
    let Some(container) = opt(source.get(ffi::ICAP_XFERMECH))? else {
        return Ok(false);
    };
    Ok(container_allows_int(
        &container,
        i64::from(ffi::TWSX_MEMORY),
    ))
}

/// Whether the active feeder source supports duplex scanning (`CAP_DUPLEX` + `CAP_DUPLEXENABLED`).
pub(super) fn duplex_supported(source: &mut dyn CapSource) -> Result<bool, ScanError> {
    let hw_duplex = opt(source.get_current(ffi::CAP_DUPLEX))?
        .or(opt(source.get(ffi::CAP_DUPLEX))?)
        .and_then(|c| current_int(&c));
    if let Some(dx) = hw_duplex {
        let has_hw =
            dx == i64::from(ffi::TWDX_1PASSDUPLEX) || dx == i64::from(ffi::TWDX_2PASSDUPLEX);
        if !has_hw {
            return Ok(false);
        }
        // Confirm `CAP_DUPLEXENABLED` doesn't restrict the feeder to `[false]` only.
        if let Some(en) = opt(source.get(ffi::CAP_DUPLEXENABLED))? {
            return match &en {
                Container::One(item) => {
                    if item_as_int(item) == Some(1) {
                        Ok(true)
                    } else if let Some(qc) = opt(source.query_support(ffi::CAP_DUPLEXENABLED))? {
                        Ok(qc & u32::from(ffi::TWQC_SET) != 0)
                    } else {
                        Ok(true)
                    }
                }
                _ => Ok(container_allows_int(&en, 1)),
            };
        }
        return Ok(true);
    }
    // Fallback for drivers exposing `CAP_DUPLEXENABLED` without `CAP_DUPLEX`.
    let Some(en) = opt(source.get(ffi::CAP_DUPLEXENABLED))? else {
        return Ok(false);
    };
    Ok(container_allows_int(&en, 1))
}

/// Reads supported [`Format`]s in canonical model order, inspecting `ICAP_BITDEPTH` per pixel type
/// and restoring `ICAP_PIXELTYPE` if changed.
pub(super) fn read_formats(
    source: &mut dyn CapSource,
    allow_16bit: bool,
) -> Result<Vec<Format>, ScanError> {
    let Some(pt_container) = opt(source.get(ffi::ICAP_PIXELTYPE))? else {
        return Ok(Vec::new());
    };
    let pt_type = container_item_type(&pt_container);
    let initial_pt = current_int(&pt_container);

    let mut candidate_pts = Vec::new();
    for pt in [ffi::TWPT_BW, ffi::TWPT_GRAY, ffi::TWPT_RGB] {
        if container_allows_int(&pt_container, i64::from(pt)) {
            candidate_pts.push(pt);
        }
    }
    if candidate_pts.is_empty() {
        return Ok(Vec::new());
    }
    // Probe the currently active pixel type first so single-type or already-matched caps avoid sets.
    candidate_pts.sort_by_key(|&pt| initial_pt != Some(i64::from(pt)));

    let can_switch = candidate_pts.len() > 1;
    let mut active_pt = initial_pt;
    let mut found = [false; 5];
    let mut outcome: Result<(), ScanError> = Ok(());

    for pt in candidate_pts {
        if can_switch
            && active_pt != Some(i64::from(pt))
            && let Some(item) = make_int_item(pt_type, i64::from(pt))
        {
            match opt(source.set(ffi::ICAP_PIXELTYPE, item.into())) {
                Ok(Some(())) => active_pt = Some(i64::from(pt)),
                Ok(None) => continue,
                Err(err) => {
                    outcome = Err(err);
                    break;
                }
            }
        }
        let depths = match opt(source.get(ffi::ICAP_BITDEPTH)) {
            Ok(d) => d,
            Err(err) => {
                outcome = Err(err);
                break;
            }
        };
        match pt {
            ffi::TWPT_BW => {
                let ok = depths.as_ref().is_none_or(|c| container_allows_int(c, 1));
                if ok {
                    found[0] = true;
                }
            }
            ffi::TWPT_GRAY => match &depths {
                None => found[1] = true,
                Some(c) => {
                    if container_allows_int(c, 8) {
                        found[1] = true;
                    }
                    if allow_16bit && container_allows_int(c, 16) {
                        found[2] = true;
                    }
                    if !found[0] && container_allows_int(c, 1) {
                        found[0] = true;
                    }
                }
            },
            ffi::TWPT_RGB => match &depths {
                None => found[3] = true,
                Some(c) => {
                    // Drivers report either bits-per-channel (`8`, `16`) or total bits-per-pixel
                    // (`24`, `48`, as `Twain_DS_sample01` does per TWAIN 2.5).
                    if container_allows_int(c, 8) || container_allows_int(c, 24) {
                        found[3] = true;
                    }
                    if allow_16bit && (container_allows_int(c, 16) || container_allows_int(c, 48)) {
                        found[4] = true;
                    }
                }
            },
            _ => {}
        }
    }

    if can_switch
        && let Some(orig) = initial_pt
        && active_pt != Some(orig)
        && let Some(item) = make_int_item(pt_type, orig)
    {
        let restore = opt(source.set(ffi::ICAP_PIXELTYPE, item.into())).map(drop);
        outcome = outcome.and(restore);
    }
    outcome?;

    const ORDER: [Format; 5] = [
        Format::Bw1,
        Format::Gray8,
        Format::Gray16,
        Format::Rgb8,
        Format::Rgb16,
    ];
    Ok(ORDER
        .into_iter()
        .zip(found)
        .filter_map(|(fmt, yes)| yes.then_some(fmt))
        .collect())
}

/// Reads supported [`Resolutions`] from `ICAP_XRESOLUTION`, converted through `units`.
///
/// If `ICAP_XRESOLUTION` is unsupported altogether, falls back to `72` dpi; if `ICAP_XRESOLUTION`
/// is present but offers no positive finite whole-dpi value, returns `Ok(None)`.
pub(super) fn read_resolutions(
    source: &mut dyn CapSource,
    units: Units,
) -> Result<Option<Resolutions>, ScanError> {
    let Some(container) = opt(source.get(ffi::ICAP_XRESOLUTION))? else {
        return Ok(Some(Resolutions::List(vec![NO_RESOLUTION_DPI])));
    };
    Ok(resolutions_from_container(&container, units))
}

/// Whether `container` under `TWUN_PIXELS` holds only TWAIN's dimensionless `<= 1.0` sentinel
/// (as opposed to a source that reports real DPI values under `TWUN_PIXELS`).
pub(super) fn container_is_dimensionless_pixels(container: &Container, units: Units) -> bool {
    if units != Units::Pixels {
        return false;
    }
    let positive_finite = |item: &Item| item_as_f64(item).filter(|v| v.is_finite() && *v > 0.0);
    match container {
        Container::One(item) => positive_finite(item).is_some_and(|v| v <= 1.0),
        Container::Enumeration(e) => {
            let mut any = false;
            for v in e.items.as_slice().iter().filter_map(positive_finite) {
                any = true;
                if v > 1.0 {
                    return false;
                }
            }
            any
        }
        Container::Array(items) => {
            let mut any = false;
            for v in items.as_slice().iter().filter_map(positive_finite) {
                any = true;
                if v > 1.0 {
                    return false;
                }
            }
            any
        }
        Container::Range(range) => {
            let (Some(min_raw), Some(max_raw)) =
                (item_as_f64(range.min()), item_as_f64(range.max()))
            else {
                return false;
            };
            min_raw.is_finite()
                && max_raw.is_finite()
                && max_raw > 0.0
                && min_raw <= max_raw
                && max_raw <= 1.0
        }
    }
}

/// Converts a resolution [`Container`] into [`Resolutions`] in whole dots per inch.
pub(super) fn resolutions_from_container(
    container: &Container,
    units: Units,
) -> Option<Resolutions> {
    if container_is_dimensionless_pixels(container, units) {
        return Some(Resolutions::List(vec![NO_RESOLUTION_DPI]));
    }
    let scale = units.per_inch().unwrap_or(1.0);
    let to_whole_dpi = |item: &Item| -> Option<u32> {
        let raw = item_as_f64(item)?;
        if !raw.is_finite() || raw <= 0.0 {
            return None;
        }
        let dpi = (raw * scale).round();
        if dpi.is_finite() && (1.0..=f64::from(u32::MAX)).contains(&dpi) {
            Some(dpi as u32)
        } else {
            None
        }
    };

    match container {
        Container::One(item) => Some(Resolutions::List(vec![to_whole_dpi(item)?])),
        Container::Enumeration(e) => {
            let mut list: Vec<u32> = e.items.as_slice().iter().filter_map(to_whole_dpi).collect();
            list.sort_unstable();
            list.dedup();
            (!list.is_empty()).then_some(Resolutions::List(list))
        }
        Container::Array(items) => {
            let mut list: Vec<u32> = items.as_slice().iter().filter_map(to_whole_dpi).collect();
            list.sort_unstable();
            list.dedup();
            (!list.is_empty()).then_some(Resolutions::List(list))
        }
        Container::Range(range) => {
            let min_raw = item_as_f64(range.min())?;
            let max_raw = item_as_f64(range.max())?;
            let step_raw = item_as_f64(range.step()).unwrap_or(1.0);
            if !min_raw.is_finite() || !max_raw.is_finite() || max_raw <= 0.0 || min_raw > max_raw {
                return None;
            }
            let min_dpi = (min_raw.max(0.0) * scale).ceil().max(1.0);
            let max_dpi = (max_raw * scale).floor();
            if !min_dpi.is_finite()
                || !max_dpi.is_finite()
                || min_dpi > max_dpi
                || min_dpi > f64::from(u32::MAX)
            {
                return None;
            }
            let min = min_dpi as u32;
            let max = max_dpi.min(f64::from(u32::MAX)) as u32;
            let step_dpi = if step_raw.is_finite() && step_raw > 0.0 {
                (step_raw * scale).ceil()
            } else {
                1.0
            };
            let step_u32 = if step_dpi.is_finite() && step_dpi >= 1.0 {
                step_dpi.min(f64::from(u32::MAX)) as u32
            } else {
                1
            };
            let step = NonZeroU32::new(step_u32).unwrap_or(NonZeroU32::MIN);
            Some(Resolutions::Range { min, max, step })
        }
    }
}

/// Reads the current `(across, down)` DPI, falling back to `NO_RESOLUTION_DPI` when absent or
/// unusable.
pub(super) fn read_current_dpi(
    source: &mut dyn CapSource,
    units: Units,
) -> Result<(f64, f64), ScanError> {
    let across = opt(source.get_current(ffi::ICAP_XRESOLUTION))?
        .and_then(|c| current_f64(&c))
        .filter(|&v| v.is_finite() && v > 0.0)
        .map(|v| units.res_to_dpi(v))
        .unwrap_or(f64::from(NO_RESOLUTION_DPI));
    let down = opt(source.get_current(ffi::ICAP_YRESOLUTION))?
        .and_then(|c| current_f64(&c))
        .filter(|&v| v.is_finite() && v > 0.0)
        .map(|v| units.res_to_dpi(v))
        .unwrap_or(across);
    Ok((across, down))
}

/// Physical area bounds in the source's active `ICAP_UNITS`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct PhysicalBounds {
    /// Origin left coordinate (`0` when `ICAP_PHYSICAL*` is used; `frame.left` when derived from
    /// `DAT_IMAGELAYOUT`).
    pub left: Fix32,
    /// Origin top coordinate (`0` when `ICAP_PHYSICAL*` is used; `frame.top` when derived from
    /// `DAT_IMAGELAYOUT`).
    pub top: Fix32,
    pub min_width: f64,
    pub min_height: f64,
    pub max_width: f64,
    pub max_height: f64,
    /// Exact `Fix32` right/bottom ceilings when `ICAP_PHYSICAL*` or `DAT_IMAGELAYOUT` reported
    /// `Fix32`, so clamping never overshoots by `1/65536` unit.
    pub max_right_fix: Option<Fix32>,
    pub max_bottom_fix: Option<Fix32>,
}

/// Reads the source's physical bounds in active `ICAP_UNITS` from `ICAP_PHYSICALWIDTH` /
/// `ICAP_PHYSICALHEIGHT` (falling back to `default_image_layout` / `image_layout` when absent or
/// non-positive) and optional `ICAP_MINIMUMWIDTH` / `ICAP_MINIMUMHEIGHT`.
pub(super) fn read_physical_bounds(
    source: &mut dyn CapSource,
) -> Result<Option<PhysicalBounds>, ScanError> {
    let phys_w = opt(source.get_current(ffi::ICAP_PHYSICALWIDTH))?
        .or(opt(source.get(ffi::ICAP_PHYSICALWIDTH))?);
    let phys_h = opt(source.get_current(ffi::ICAP_PHYSICALHEIGHT))?
        .or(opt(source.get(ffi::ICAP_PHYSICALHEIGHT))?);

    let valid_phys = phys_w.zip(phys_h).and_then(|(cw, ch)| {
        let w = current_f64(&cw)?;
        let h = current_f64(&ch)?;
        (w.is_finite() && w > 0.0 && h.is_finite() && h > 0.0).then_some((
            Fix32::from_int(0),
            Fix32::from_int(0),
            w,
            h,
            current_fix32(&cw),
            current_fix32(&ch),
        ))
    });

    let (left, top, max_w, max_h, max_r_fix, max_b_fix) = match valid_phys {
        Some(found) => found,
        None => {
            let positive_frame = |f: Frame| {
                let w = f.right.to_f64() - f.left.to_f64();
                let h = f.bottom.to_f64() - f.top.to_f64();
                (w.is_finite() && w > 0.0 && h.is_finite() && h > 0.0).then_some((f, w, h))
            };
            let chosen = match opt(source.default_image_layout())?.and_then(positive_frame) {
                Some(found) => Some(found),
                None => opt(source.image_layout())?.and_then(positive_frame),
            };
            let Some((frame, w, h)) = chosen else {
                return Ok(None);
            };
            (
                frame.left,
                frame.top,
                w,
                h,
                Some(frame.right),
                Some(frame.bottom),
            )
        }
    };

    let min_w = opt(source.get_current(ffi::ICAP_MINIMUMWIDTH))?
        .and_then(|c| current_f64(&c))
        .filter(|&v| v.is_finite() && v > 0.0 && v <= max_w)
        .unwrap_or(0.0);
    let min_h = opt(source.get_current(ffi::ICAP_MINIMUMHEIGHT))?
        .and_then(|c| current_f64(&c))
        .filter(|&v| v.is_finite() && v > 0.0 && v <= max_h)
        .unwrap_or(0.0);

    Ok(Some(PhysicalBounds {
        left,
        top,
        min_width: min_w,
        min_height: min_h,
        max_width: max_w,
        max_height: max_h,
        max_right_fix: max_r_fix,
        max_bottom_fix: max_b_fix,
    }))
}

/// Converts [`read_physical_bounds`] into [`SourceCaps::max_area`] in millimetres.
pub(super) fn read_max_area(
    source: &mut dyn CapSource,
    units: Units,
    across_dpi: f64,
    down_dpi: f64,
) -> Result<Option<SizeMm>, ScanError> {
    let Some(bounds) = read_physical_bounds(source)? else {
        return Ok(None);
    };
    Ok(bounds_to_size_mm(&bounds, units, across_dpi, down_dpi))
}

/// Converts [`PhysicalBounds`]'s maximum dimensions into a finite positive [`SizeMm`].
pub(super) fn bounds_to_size_mm(
    bounds: &PhysicalBounds,
    units: Units,
    across_dpi: f64,
    down_dpi: f64,
) -> Option<SizeMm> {
    let w_mm = units.length_to_mm(bounds.max_width, across_dpi)?;
    let h_mm = units.length_to_mm(bounds.max_height, down_dpi)?;
    let (w, h) = (w_mm as f32, h_mm as f32);
    (w.is_finite() && w > 0.0 && h.is_finite() && h > 0.0).then_some(SizeMm {
        width: w,
        height: h,
    })
}

/// Which automatic paper-size capabilities a source supports.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct AutoPaperCaps {
    pub border_detection: bool,
    pub autosize: bool,
    pub undefined_size: bool,
}

impl AutoPaperCaps {
    pub(super) fn supported(self) -> bool {
        self.border_detection || self.autosize
    }
}

/// Probes `ICAP_AUTOMATICBORDERDETECTION`, `ICAP_AUTOSIZE`, and `ICAP_UNDEFINEDIMAGESIZE`.
pub(super) fn auto_paper_caps(source: &mut dyn CapSource) -> Result<AutoPaperCaps, ScanError> {
    let border_detection = bool_cap_allows_true(source, ffi::ICAP_AUTOMATICBORDERDETECTION)?;
    let autosize = opt(source.get(ffi::ICAP_AUTOSIZE))?
        .is_some_and(|c| container_allows_int(&c, i64::from(ffi::TWAS_AUTO)));
    let undefined_size = bool_cap_allows_true(source, ffi::ICAP_UNDEFINEDIMAGESIZE)?;
    Ok(AutoPaperCaps {
        border_detection,
        autosize,
        undefined_size,
    })
}

/// Whether a boolean capability can take `true` (`1`), handling both `Enumeration` (TWAIN 2) and
/// settable `One` (TWAIN 1.x).
fn bool_cap_allows_true(source: &mut dyn CapSource, cap: u16) -> Result<bool, ScanError> {
    let Some(container) = opt(source.get(cap))? else {
        return Ok(false);
    };
    match &container {
        Container::One(item) => {
            if item_as_int(item) == Some(1) {
                return Ok(true);
            }
            if let Some(qc) = opt(source.query_support(cap))? {
                return Ok(qc & u32::from(ffi::TWQC_SET) != 0);
            }
            Ok(false)
        }
        _ => Ok(container_allows_int(&container, 1)),
    }
}

/// Returns the [`ItemType`] of any [`Container`].
pub(super) fn container_item_type(container: &Container) -> ItemType {
    match container {
        Container::One(item) => item.item_type(),
        Container::Enumeration(e) => e.items.item_type(),
        Container::Array(items) => items.item_type(),
        Container::Range(r) => r.item_type(),
    }
}

/// Extracts the active [`Item`] from `container`.
pub(super) fn current_item(container: &Container) -> Option<&Item> {
    match container {
        Container::One(item) => Some(item),
        Container::Enumeration(e) => e.current_item().or_else(|| e.items.as_slice().first()),
        Container::Array(items) => items.as_slice().first(),
        Container::Range(r) => Some(r.current()),
    }
}

/// Extracts an exact integer/boolean value from `item`, including integral `Fix32` values
/// (`frac == 0`) on drivers that store integer capabilities in `TWTY_FIX32`.
pub(super) fn item_as_int(item: &Item) -> Option<i64> {
    match item {
        Item::Fix32(f) if f.frac == 0 => Some(i64::from(f.whole)),
        other => other.as_i64(),
    }
}

/// Extracts the active integer/boolean value from `container`.
pub(super) fn current_int(container: &Container) -> Option<i64> {
    current_item(container).and_then(item_as_int)
}

/// Extracts the active numeric value as `f64` from `container`.
pub(super) fn current_f64(container: &Container) -> Option<f64> {
    current_item(container).and_then(item_as_f64)
}

/// Extracts the active `Fix32` when `container` stores `Fix32`.
fn current_fix32(container: &Container) -> Option<Fix32> {
    match current_item(container)? {
        Item::Fix32(f) => Some(*f),
        _ => None,
    }
}

/// Converts any numeric/boolean [`Item`] to `f64`.
pub(super) fn item_as_f64(item: &Item) -> Option<f64> {
    match item {
        Item::Fix32(f) => Some(f.to_f64()),
        other => other.as_i64().map(|v| v as f64),
    }
}

/// Checks in $O(1)$ space and time for `Range` (and linear scan for lists) whether `container`
/// allows the integer `target`.
pub(super) fn container_allows_int(container: &Container, target: i64) -> bool {
    match container {
        Container::One(item) => item_as_int(item) == Some(target),
        Container::Enumeration(e) => e
            .items
            .as_slice()
            .iter()
            .any(|i| item_as_int(i) == Some(target)),
        Container::Array(items) => items
            .as_slice()
            .iter()
            .any(|i| item_as_int(i) == Some(target)),
        Container::Range(r) => {
            let (Some(min), Some(max)) = (item_as_int(r.min()), item_as_int(r.max())) else {
                return false;
            };
            if target < min || target > max {
                return false;
            }
            match item_as_int(r.step()) {
                Some(step) if step > 1 => (target - min) % step == 0,
                _ => true,
            }
        }
    }
}

/// Encodes `value` into an [`Item`] of `item_type`.
pub(super) fn make_int_item(item_type: ItemType, value: i64) -> Option<Item> {
    Some(match item_type {
        ItemType::Bool => Item::Bool(value != 0),
        ItemType::Int8 => Item::Int8(i8::try_from(value).ok()?),
        ItemType::Int16 => Item::Int16(i16::try_from(value).ok()?),
        ItemType::Int32 => Item::Int32(i32::try_from(value).ok()?),
        ItemType::UInt8 => Item::UInt8(u8::try_from(value).ok()?),
        ItemType::UInt16 => Item::UInt16(u16::try_from(value).ok()?),
        ItemType::UInt32 => Item::UInt32(u32::try_from(value).ok()?),
        ItemType::Fix32 => Item::Fix32(Fix32::from_f64(value as f64)?),
        _ => return None,
    })
}

/// Encodes `value` into a floating/numeric [`Item`] of `item_type`.
pub(super) fn make_num_item(item_type: ItemType, value: f64) -> Option<Item> {
    if !value.is_finite() {
        return None;
    }
    match item_type {
        ItemType::Fix32 => Fix32::from_f64(value).map(Item::Fix32),
        other => {
            let rounded = value.round();
            if !(i64::MIN as f64..=i64::MAX as f64).contains(&rounded) {
                return None;
            }
            make_int_item(other, rounded as i64)
        }
    }
}

#[cfg(test)]
pub(super) mod test_mock {
    use std::collections::HashMap;

    use twain_dsm::{Enumeration, Items, Range};

    use super::*;

    /// In-memory fake TWAIN source for unit-testing `caps` and `settings` in both 32-bit and
    /// 64-bit builds.
    pub(in crate::twain) struct MockSource {
        pub caps: HashMap<u16, MockCap>,
        pub bit_depths_by_pt: HashMap<u16, Vec<u16>>,
        pub layout: Option<Frame>,
        pub default_layout: Option<Frame>,
        pub fatal_on: Option<(u16, ScanError)>,
        /// Simulates `TWRC_CHECKSTATUS` (`Ok(())` from `LiveSource::set`) leaving the capability's
        /// value untouched when setting `(cap, item)`.
        pub ignore_set_on: Option<(u16, Item)>,
        pub set_log: Vec<(u16, Item)>,
    }

    #[derive(Clone, Debug)]
    pub(in crate::twain) struct MockCap {
        pub container: Container,
        pub read_only: bool,
        pub qc: u32,
    }

    impl MockCap {
        pub fn one(item: Item) -> Self {
            MockCap {
                container: Container::One(item),
                read_only: false,
                qc: u32::from(ffi::TWQC_GET | ffi::TWQC_SET | ffi::TWQC_GETCURRENT),
            }
        }

        pub fn one_ro(item: Item) -> Self {
            MockCap {
                container: Container::One(item),
                read_only: true,
                qc: u32::from(ffi::TWQC_GET | ffi::TWQC_GETCURRENT),
            }
        }

        pub fn enumeration(item_type: ItemType, items: Vec<Item>, current: u32) -> Self {
            MockCap {
                container: Container::Enumeration(Enumeration {
                    items: Items::new(item_type, items).unwrap(),
                    current,
                    default: current,
                }),
                read_only: false,
                qc: u32::from(ffi::TWQC_GET | ffi::TWQC_SET | ffi::TWQC_GETCURRENT),
            }
        }

        pub fn range(min: Item, max: Item, step: Item, current: Item) -> Self {
            MockCap {
                container: Container::Range(
                    Range::new(min, max, step, current.clone(), current).unwrap(),
                ),
                read_only: false,
                qc: u32::from(ffi::TWQC_GET | ffi::TWQC_SET | ffi::TWQC_GETCURRENT),
            }
        }
    }

    impl MockSource {
        /// A realistic baseline TWAIN 2 sample scanner (Flatbed + Feeder, 1/8/24-bit, 50..=600 dpi,
        /// Legal physical area, Letter default layout, UI-only supported).
        pub fn sample() -> Self {
            let mut caps = HashMap::new();
            caps.insert(
                ffi::ICAP_UNITS,
                MockCap::enumeration(
                    ItemType::UInt16,
                    vec![
                        Item::UInt16(ffi::TWUN_INCHES),
                        Item::UInt16(ffi::TWUN_CENTIMETERS),
                        Item::UInt16(ffi::TWUN_PIXELS),
                    ],
                    0,
                ),
            );
            caps.insert(
                ffi::ICAP_XFERMECH,
                MockCap::enumeration(
                    ItemType::UInt16,
                    vec![
                        Item::UInt16(ffi::TWSX_NATIVE),
                        Item::UInt16(ffi::TWSX_MEMORY),
                    ],
                    0,
                ),
            );
            caps.insert(
                ffi::CAP_FEEDERENABLED,
                MockCap::enumeration(ItemType::Bool, vec![Item::Bool(false), Item::Bool(true)], 1),
            );
            caps.insert(
                ffi::CAP_DUPLEX,
                MockCap::one_ro(Item::UInt16(ffi::TWDX_NONE)),
            );
            caps.insert(
                ffi::CAP_DUPLEXENABLED,
                MockCap::enumeration(ItemType::Bool, vec![Item::Bool(false)], 0),
            );
            caps.insert(ffi::CAP_ENABLEDSUIONLY, MockCap::one_ro(Item::Bool(true)));
            caps.insert(
                ffi::ICAP_PIXELTYPE,
                MockCap::enumeration(
                    ItemType::UInt16,
                    vec![
                        Item::UInt16(ffi::TWPT_BW),
                        Item::UInt16(ffi::TWPT_GRAY),
                        Item::UInt16(ffi::TWPT_RGB),
                    ],
                    2,
                ),
            );
            let mut bit_depths_by_pt = HashMap::new();
            bit_depths_by_pt.insert(ffi::TWPT_BW, vec![1]);
            bit_depths_by_pt.insert(ffi::TWPT_GRAY, vec![8, 16]);
            bit_depths_by_pt.insert(ffi::TWPT_RGB, vec![24, 48]);
            caps.insert(
                ffi::ICAP_BITDEPTH,
                MockCap::enumeration(
                    ItemType::UInt16,
                    vec![Item::UInt16(24), Item::UInt16(48)],
                    0,
                ),
            );
            let dpis: Vec<Item> = [50, 100, 150, 200, 300, 600]
                .into_iter()
                .map(|d| Item::Fix32(Fix32::from_int(d)))
                .collect();
            caps.insert(
                ffi::ICAP_XRESOLUTION,
                MockCap::enumeration(ItemType::Fix32, dpis.clone(), 3),
            );
            caps.insert(
                ffi::ICAP_YRESOLUTION,
                MockCap::enumeration(ItemType::Fix32, dpis, 3),
            );
            caps.insert(
                ffi::ICAP_PHYSICALWIDTH,
                MockCap::one_ro(Item::Fix32(Fix32::from_f64(8.5).unwrap())),
            );
            caps.insert(
                ffi::ICAP_PHYSICALHEIGHT,
                MockCap::one_ro(Item::Fix32(Fix32::from_f64(14.0).unwrap())),
            );
            let default_frame = Frame {
                left: Fix32::from_int(0),
                top: Fix32::from_int(0),
                right: Fix32::from_f64(8.5).unwrap(),
                bottom: Fix32::from_f64(11.0).unwrap(),
            };
            MockSource {
                caps,
                bit_depths_by_pt,
                layout: Some(default_frame),
                default_layout: Some(default_frame),
                fatal_on: None,
                ignore_set_on: None,
                set_log: Vec::new(),
            }
        }

        fn sync_bit_depth(&mut self, pt: u16) {
            if let Some(depths) = self.bit_depths_by_pt.get(&pt).cloned() {
                let items = depths.into_iter().map(Item::UInt16).collect();
                self.caps.insert(
                    ffi::ICAP_BITDEPTH,
                    MockCap::enumeration(ItemType::UInt16, items, 0),
                );
            }
        }
    }

    impl CapSource for MockSource {
        fn get(&mut self, cap: u16) -> Result<Container, CapError> {
            if let Some((bad_cap, ref err)) = self.fatal_on
                && bad_cap == cap
            {
                return Err(CapError::Fatal(err.clone()));
            }
            self.caps
                .get(&cap)
                .map(|c| c.container.clone())
                .ok_or(CapError::Refused)
        }

        fn get_current(&mut self, cap: u16) -> Result<Container, CapError> {
            let full = self.get(cap)?;
            let item = current_item(&full).cloned().ok_or(CapError::Refused)?;
            Ok(Container::One(item))
        }

        fn query_support(&mut self, cap: u16) -> Result<u32, CapError> {
            self.caps.get(&cap).map(|c| c.qc).ok_or(CapError::Refused)
        }

        fn set(&mut self, cap: u16, value: Container) -> Result<(), CapError> {
            if let Some((bad_cap, ref err)) = self.fatal_on
                && bad_cap == cap
            {
                return Err(CapError::Fatal(err.clone()));
            }
            let item = current_item(&value).cloned().ok_or(CapError::Refused)?;
            if let Some((ignore_cap, ref ignore_item)) = self.ignore_set_on
                && ignore_cap == cap
                && ignore_item == &item
            {
                self.set_log.push((cap, item));
                return Ok(());
            }
            let entry = self.caps.get_mut(&cap).ok_or(CapError::Refused)?;
            if entry.read_only {
                return Err(CapError::Refused);
            }
            match &mut entry.container {
                Container::One(cur) => *cur = item.clone(),
                Container::Enumeration(e) => {
                    let pos = e
                        .items
                        .as_slice()
                        .iter()
                        .position(|candidate| candidate == &item)
                        .ok_or(CapError::Refused)?;
                    e.current = pos as u32;
                }
                Container::Range(r) => {
                    let v = item_as_f64(&item).ok_or(CapError::Refused)?;
                    let min = item_as_f64(r.min()).unwrap();
                    let max = item_as_f64(r.max()).unwrap();
                    let clamped = v.clamp(min, max);
                    let new_cur = make_num_item(r.item_type(), clamped).unwrap();
                    entry.container = Container::Range(
                        Range::new(
                            r.min().clone(),
                            r.max().clone(),
                            r.step().clone(),
                            r.default().clone(),
                            new_cur,
                        )
                        .unwrap(),
                    );
                }
                Container::Array(_) => {
                    return Err(CapError::Refused);
                }
            }
            self.set_log.push((cap, item.clone()));
            if cap == ffi::ICAP_PIXELTYPE
                && let Some(pt) = item_as_int(&item).and_then(|v| u16::try_from(v).ok())
            {
                self.sync_bit_depth(pt);
            }
            Ok(())
        }

        fn image_layout(&mut self) -> Result<Frame, CapError> {
            self.layout.ok_or(CapError::Refused)
        }

        fn default_image_layout(&mut self) -> Result<Frame, CapError> {
            self.default_layout.ok_or(CapError::Refused)
        }

        fn reset_image_layout(&mut self) -> Result<Frame, CapError> {
            let def = self.default_layout.ok_or(CapError::Refused)?;
            self.layout = Some(def);
            Ok(def)
        }

        fn set_image_layout(&mut self, frame: Frame) -> Result<(), CapError> {
            if self.layout.is_none() {
                return Err(CapError::Refused);
            }
            self.layout = Some(frame);
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::test_mock::{MockCap, MockSource};
    use super::*;

    #[test]
    fn units_convert_lengths_and_resolutions_across_all_twain_units() {
        // 8.5 inches = 21.59 cm = 215.9 mm = 51 picas = 612 points = 12,240 twips = 2,550 px @ 300 dpi.
        let cases = [
            (Units::Inches, 8.5, 300.0, 215.9, 300.0),
            (Units::Centimeters, 21.59, 300.0 / 2.54, 215.9, 300.0),
            (Units::Millimeters, 215.9, 300.0 / 25.4, 215.9, 300.0),
            (Units::Picas, 51.0, 50.0, 215.9, 300.0),
            (Units::Points, 612.0, 300.0 / 72.0, 215.9, 300.0),
            (Units::Twips, 12_240.0, 300.0 / 1440.0, 215.9, 300.0),
            (Units::Pixels, 2550.0, 1.0, 215.9, 72.0),
        ];
        for (unit, len_u, res_u, want_mm, want_dpi) in cases {
            let dpi_for_px = 300.0;
            let got_mm = unit.length_to_mm(len_u, dpi_for_px).unwrap();
            assert!(
                (got_mm - want_mm).abs() < 1e-4,
                "{unit:?}: {got_mm} vs {want_mm}"
            );
            let round_u = unit.mm_to_length(want_mm, dpi_for_px).unwrap();
            assert!(
                (round_u - len_u).abs() < 1e-4,
                "{unit:?}: {round_u} vs {len_u}"
            );
            let got_dpi = unit.res_to_dpi(res_u);
            assert!(
                (got_dpi - want_dpi).abs() < 1e-4,
                "{unit:?}: {got_dpi} vs {want_dpi}"
            );
        }
    }

    #[test]
    fn sample_caps_include_flatbed_and_feeder_16bit_on_memory_and_omit_16bit_on_native() {
        let mut src = MockSource::sample();
        let mem_caps = read(&mut src, false).expect("memory caps");
        assert!(mem_caps.settings_window);
        assert_eq!(mem_caps.sources.len(), 2);
        assert_eq!(mem_caps.sources[0].source, Source::Flatbed);
        assert!(!mem_caps.sources[0].duplex);
        assert_eq!(mem_caps.sources[1].source, Source::Feeder);
        assert!(!mem_caps.sources[1].duplex);
        assert_eq!(
            mem_caps.sources[1].formats,
            vec![
                Format::Bw1,
                Format::Gray8,
                Format::Gray16,
                Format::Rgb8,
                Format::Rgb16
            ]
        );
        assert_eq!(mem_caps.sources[1].max_area, Some(SizeMm::LEGAL));
        assert!(!mem_caps.sources[1].paper_auto);
        // Initial feeder state (`true`) and pixel type (`TWPT_RGB`) were restored!
        assert_eq!(
            current_int(&src.get_current(ffi::CAP_FEEDERENABLED).unwrap()),
            Some(1)
        );
        assert_eq!(
            current_int(&src.get_current(ffi::ICAP_PIXELTYPE).unwrap()),
            Some(i64::from(ffi::TWPT_RGB))
        );

        // When native transfer is forced, 16-bit formats are omitted.
        let nat_caps = read(&mut src, true).expect("native caps");
        assert_eq!(
            nat_caps.sources[0].formats,
            vec![Format::Bw1, Format::Gray8, Format::Rgb8]
        );
    }

    #[test]
    fn read_only_centimeters_and_duplex_and_auto_paper_convert_cleanly() {
        let mut src = MockSource::sample();
        // Force read-only centimeters: 21.0 x 29.7 cm (A4), 100..=300 px/cm (~254..=762 dpi).
        src.caps.insert(
            ffi::ICAP_UNITS,
            MockCap::one_ro(Item::UInt16(ffi::TWUN_CENTIMETERS)),
        );
        src.caps.insert(
            ffi::ICAP_PHYSICALWIDTH,
            MockCap::one_ro(Item::Fix32(Fix32::from_f64(21.0).unwrap())),
        );
        src.caps.insert(
            ffi::ICAP_PHYSICALHEIGHT,
            MockCap::one_ro(Item::Fix32(Fix32::from_f64(29.7).unwrap())),
        );
        src.caps.insert(
            ffi::ICAP_XRESOLUTION,
            MockCap::range(
                Item::Fix32(Fix32::from_f64(100.0).unwrap()),
                Item::Fix32(Fix32::from_f64(200.0).unwrap()),
                Item::Fix32(Fix32::from_f64(1.0).unwrap()),
                Item::Fix32(Fix32::from_f64(118.11).unwrap()),
            ),
        );
        src.caps.insert(
            ffi::CAP_DUPLEX,
            MockCap::one_ro(Item::UInt16(ffi::TWDX_1PASSDUPLEX)),
        );
        src.caps.insert(
            ffi::CAP_DUPLEXENABLED,
            MockCap::enumeration(ItemType::Bool, vec![Item::Bool(false), Item::Bool(true)], 0),
        );
        src.caps.insert(
            ffi::ICAP_AUTOMATICBORDERDETECTION,
            MockCap::enumeration(ItemType::Bool, vec![Item::Bool(false), Item::Bool(true)], 1),
        );

        let caps = read(&mut src, false).expect("cm caps");
        let feeder = caps
            .sources
            .iter()
            .find(|s| s.source == Source::Feeder)
            .unwrap();
        assert!(feeder.duplex);
        assert!(feeder.paper_auto);
        let area = feeder.max_area.unwrap();
        assert!((area.width - 210.0).abs() < 0.01);
        assert!((area.height - 297.0).abs() < 0.01);
        assert_eq!(
            feeder.resolutions,
            Resolutions::Range {
                min: 254,
                max: 508,
                step: NonZeroU32::new(3).unwrap(),
            }
        );
    }

    #[test]
    fn twain1_onevalue_feeder_and_empty_formats_or_resolutions_and_fatal_errors() {
        // TWAIN 1.x settable `One(Bool(false))` offers both Flatbed and Feeder.
        let mut twain1 = MockSource::sample();
        twain1
            .caps
            .insert(ffi::CAP_FEEDERENABLED, MockCap::one(Item::Bool(false)));
        let caps1 = read(&mut twain1, false).unwrap();
        assert_eq!(caps1.sources.len(), 2);

        // Read-only `One(Bool(true))` offers Feeder alone.
        twain1
            .caps
            .insert(ffi::CAP_FEEDERENABLED, MockCap::one_ro(Item::Bool(true)));
        let feeder_only = read(&mut twain1, false).unwrap();
        assert_eq!(feeder_only.sources.len(), 1);
        assert_eq!(feeder_only.sources[0].source, Source::Feeder);

        // Fatal device error (`ScanError::Gone`) is never swallowed as an unsupported capability!
        let mut unplugged = MockSource::sample();
        unplugged.fatal_on = Some((ffi::CAP_DUPLEX, ScanError::Gone));
        assert_eq!(read(&mut unplugged, false), Err(ScanError::Gone));

        // Empty formats -> `ScanError::Unsupported`.
        let mut no_fmt = MockSource::sample();
        no_fmt.caps.remove(&ffi::ICAP_PIXELTYPE);
        assert_eq!(read(&mut no_fmt, false), Err(ScanError::Unsupported));

        // Only invalid `<= 0` resolutions -> `ScanError::Unsupported`.
        let mut bad_res = MockSource::sample();
        bad_res.caps.insert(
            ffi::ICAP_XRESOLUTION,
            MockCap::one_ro(Item::Fix32(Fix32::from_int(0))),
        );
        assert_eq!(read(&mut bad_res, false), Err(ScanError::Unsupported));
    }

    #[test]
    fn pixel_units_distinguish_dimensionless_one_from_dpi_ranges_and_lists() {
        // Compliant `TWUN_PIXELS` dimensionless `One(1.0)` maps to `[72]`.
        let dimless = Container::One(Item::Fix32(Fix32::from_int(1)));
        assert!(container_is_dimensionless_pixels(&dimless, Units::Pixels));
        assert_eq!(
            resolutions_from_container(&dimless, Units::Pixels),
            Some(Resolutions::List(vec![NO_RESOLUTION_DPI]))
        );

        // Non-compliant `TWUN_PIXELS` `Range(50..=600, step=1)` keeps `step = 1` (never `72`!).
        let dpi_range = MockCap::range(
            Item::Fix32(Fix32::from_int(50)),
            Item::Fix32(Fix32::from_int(600)),
            Item::Fix32(Fix32::from_int(1)),
            Item::Fix32(Fix32::from_int(300)),
        )
        .container;
        assert!(!container_is_dimensionless_pixels(
            &dpi_range,
            Units::Pixels
        ));
        assert_eq!(
            resolutions_from_container(&dpi_range, Units::Pixels),
            Some(Resolutions::Range {
                min: 50,
                max: 600,
                step: NonZeroU32::MIN,
            })
        );

        // `Range(1.0..=4.0, step=1.0)` crossing `1.0` stays monotonic (`1..=4, step=1`) instead of
        // inverting `min = 72 > max = 4`!
        let low_range = MockCap::range(
            Item::Fix32(Fix32::from_int(1)),
            Item::Fix32(Fix32::from_int(4)),
            Item::Fix32(Fix32::from_int(1)),
            Item::Fix32(Fix32::from_int(2)),
        )
        .container;
        assert_eq!(
            resolutions_from_container(&low_range, Units::Pixels),
            Some(Resolutions::Range {
                min: 1,
                max: 4,
                step: NonZeroU32::MIN,
            })
        );
    }

    #[test]
    fn fix32_integer_caps_and_negative_physical_dims_fallback_to_nonzero_origin_layout() {
        let mut src = MockSource::sample();
        // Integer capabilities stored in `TWTY_FIX32` (`frac == 0`) are decoded cleanly.
        src.caps.insert(
            ffi::ICAP_PIXELTYPE,
            MockCap::enumeration(
                ItemType::Fix32,
                vec![
                    Item::Fix32(Fix32::from_int(i16::try_from(ffi::TWPT_GRAY).unwrap())),
                    Item::Fix32(Fix32::from_int(i16::try_from(ffi::TWPT_RGB).unwrap())),
                ],
                1,
            ),
        );
        // Non-positive `ICAP_PHYSICALWIDTH` (`-1`) falls back to `default_image_layout` with a
        // non-zero `(left, top)` origin!
        src.caps.insert(
            ffi::ICAP_PHYSICALWIDTH,
            MockCap::one_ro(Item::Fix32(Fix32::from_int(-1))),
        );
        let shifted = Frame {
            left: Fix32::from_f64(0.5).unwrap(),
            top: Fix32::from_f64(0.25).unwrap(),
            right: Fix32::from_f64(9.0).unwrap(),
            bottom: Fix32::from_f64(11.25).unwrap(),
        };
        src.default_layout = Some(shifted);
        src.layout = Some(shifted);

        let bounds = read_physical_bounds(&mut src).unwrap().unwrap();
        assert_eq!(bounds.left, shifted.left);
        assert_eq!(bounds.top, shifted.top);
        assert!((bounds.max_width - 8.5).abs() < 1e-6);
        assert!((bounds.max_height - 11.0).abs() < 1e-6);
        assert_eq!(bounds.max_right_fix, Some(shifted.right));
        assert_eq!(bounds.max_bottom_fix, Some(shifted.bottom));

        let caps = read(&mut src, false).expect("fix32 caps + fallback bounds");
        assert_eq!(caps.sources[0].max_area, Some(SizeMm::LETTER));
        assert_eq!(
            caps.sources[0].formats,
            vec![Format::Gray8, Format::Gray16, Format::Rgb8, Format::Rgb16]
        );
    }
}
