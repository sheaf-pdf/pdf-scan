//! Running a TWAIN batch scan: negotiating settings, pre-checking feeder state, pumping state 5
//! with 100 ms cancellation checks and concurrent `Remote` request servicing, transferring pages
//! via `TWSX_MEMORY` or `TWSX_NATIVE`, tracking duplex sheets via [`SheetTracker`], and stepping
//! cleanly back to state 4 on completion, cancellation, or error.

#[cfg(windows)]
use std::sync::mpsc::SyncSender;
#[cfg(windows)]
use std::time::Duration;

#[cfg(windows)]
use in_progress::{Progress, Unit};
use twain_dsm::{ExtInfo, ExtValue, Fix32, ffi};

#[cfg(windows)]
use super::caps::LiveSource;
use super::caps::Units;
use super::error;
#[cfg(windows)]
use super::pixels::{self, ImageDesc};
#[cfg(windows)]
use super::settings::{self, Taken, TransferMech};
#[cfg(windows)]
use crate::{Done, Event, Format, Paper, Pixels, RawImage, RawPage, Settings, Source};
use crate::{ScanError, Side, SideKnown};

/// Polling interval while waiting in state 5 (`MSG_ENABLEDS` -> `MSG_XFERREADY`) so sheet
/// cancellation is noticed promptly even if a source stalls before producing its first page.
#[cfg(windows)]
const STATE5_POLL: Duration = Duration::from_millis(100);

/// Pure state machine assigning sheet numbers and enforcing protocol ordering (`Event::Sheet`
/// before its `Event::Page`s, and never two identical `Side`s on the same `sheet`) across simplex
/// and duplex batches.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct SheetTracker {
    first_sheet: u32,
    duplex: bool,
    completed_sheets: u64,
    open_sheet: Option<(u32, Side)>,
}

impl SheetTracker {
    /// Creates a tracker starting at `first_sheet`.
    pub(super) fn new(first_sheet: u32, duplex: bool) -> Self {
        SheetTracker {
            first_sheet,
            duplex,
            completed_sheets: 0,
            open_sheet: None,
        }
    }

    /// True when at least one sheet has finished and no duplex sheet is half-open — the exact
    /// boundary where sheet-granular cancellation may stop a feeder batch.
    pub(super) fn is_between_sheets(&self) -> bool {
        self.open_sheet.is_none() && self.completed_sheets > 0
    }

    /// Default `Side` when `DAT_EXTIMAGEINFO` (`TWEI_PAGESIDE`) is absent or inconclusive:
    /// `Side::Front` when opening a sheet, and the opposite of the first side when closing an open
    /// duplex sheet (so back-first duplex scanners still pair cleanly).
    pub(super) fn inferred_side(&self) -> Side {
        match self.open_sheet {
            Some((_, Side::Front)) => Side::Back,
            Some((_, Side::Back)) => Side::Front,
            None => Side::Front,
        }
    }

    /// Validates before starting a new sheet (`open_sheet.is_none()`) that its sheet number fits in
    /// `u32`, so an overflowing sheet stays in the feeder untransferred.
    pub(super) fn ensure_can_start_page(&self) -> Result<(), ScanError> {
        if self.open_sheet.is_none() {
            let _ = self.sheet_at(self.completed_sheets)?;
        }
        Ok(())
    }

    /// Records a transferred page of `side`, returning `(emit_sheet, page_sheet)` where
    /// `emit_sheet` is `Some(s)` iff `Event::Sheet(s)` must be emitted before `Event::Page`.
    pub(super) fn accept_page(&mut self, side: Side) -> Result<(Option<u32>, u32), ScanError> {
        if !self.duplex {
            let sheet = self.sheet_at(self.completed_sheets)?;
            self.completed_sheets += 1;
            return Ok((Some(sheet), sheet));
        }
        match self.open_sheet {
            None => {
                let sheet = self.sheet_at(self.completed_sheets)?;
                self.open_sheet = Some((sheet, side));
                Ok((Some(sheet), sheet))
            }
            Some((sheet, first_side)) if side != first_side => {
                self.open_sheet = None;
                self.completed_sheets += 1;
                Ok((None, sheet))
            }
            Some((_, _)) => {
                // Hardware blank-page skip dropped the previous sheet's second side (or a driver
                // reported the same `TWEI_PAGESIDE` twice in a row): close the previous sheet as
                // single-sided and open the next sheet for this page so `Order::take` never sees a
                // duplicate `(sheet, side)` and subsequent duplex pairs stay aligned.
                self.completed_sheets += 1;
                let next_sheet = self.sheet_at(self.completed_sheets)?;
                self.open_sheet = Some((next_sheet, side));
                Ok((Some(next_sheet), next_sheet))
            }
        }
    }

    fn sheet_at(&self, offset: u64) -> Result<u32, ScanError> {
        u32::try_from(u64::from(self.first_sheet).saturating_add(offset)).map_err(|_| {
            error::driver_msg(format!(
                "no sheet is numbered past {}: the sheet after it stays in the feeder",
                u32::MAX
            ))
        })
    }
}

/// Returns a `len`-byte mutable subslice of `storage` whose start address is 4-byte aligned, as
/// `DAT_IMAGEMEMXFER` (`MemoryTransfer::next_buffer`) requires.
///
/// `storage` must hold at least `len + 3` bytes.
pub(super) fn aligned_slice(storage: &mut [u8], len: usize) -> &mut [u8] {
    let misalign = (storage.as_ptr() as usize) % 4;
    let offset = (4 - misalign) % 4;
    &mut storage[offset..offset + len]
}

/// Converts state-6 `DAT_IMAGEINFO` `(x_resolution, y_resolution)` in `units` into `(across, down)`
/// dots per inch, falling back to `fallback` per axis if a driver reports `<= 0` or non-finite.
pub(super) fn dpi_from_image_res(
    x_res: Fix32,
    y_res: Fix32,
    units: Units,
    fallback: (f32, f32),
) -> (f32, f32) {
    (
        axis_dpi(x_res, units, fallback.0),
        axis_dpi(y_res, units, fallback.1),
    )
}

fn axis_dpi(raw: Fix32, units: Units, fallback: f32) -> f32 {
    let val = raw.to_f64();
    if !val.is_finite() || val <= 0.0 {
        return fallback;
    }
    let dpi = units.res_to_dpi(val) as f32;
    if dpi.is_finite() && dpi > 0.0 {
        dpi
    } else {
        fallback
    }
}

/// Maps `DAT_EXTIMAGEINFO`'s `TWEI_PAGESIDE` answer to `(Side, SideKnown)`.
///
/// - `TWCS_TOP` -> `(Side::Front, SideKnown::Reported)`
/// - `TWCS_BOTTOM` -> `(Side::Back, SideKnown::Reported)`
/// - `TWCS_BOTH`, unsupported, unavailable, malformed, or unknown -> `(inferred, SideKnown::Inferred)`
pub(super) fn page_side_from_ext(ext: Option<&[ExtInfo]>, inferred: Side) -> (Side, SideKnown) {
    let Some(items) = ext else {
        return (inferred, SideKnown::Inferred);
    };
    for info in items {
        if info.id != ffi::TWEI_PAGESIDE {
            continue;
        }
        if let ExtValue::Items(ref values) = info.value
            && let Some(first) = values.as_slice().first()
            && let Some(raw) = super::caps::item_as_int(first)
        {
            match u16::try_from(raw) {
                Ok(ffi::TWCS_TOP) => return (Side::Front, SideKnown::Reported),
                Ok(ffi::TWCS_BOTTOM) => return (Side::Back, SideKnown::Reported),
                _ => return (inferred, SideKnown::Inferred),
            }
        }
    }
    (inferred, SideKnown::Inferred)
}

/// Runs one batch scan on `source` (state 4), returning `Some(source)` stepped back to state 4
/// whenever the driver steps down cleanly (or `None` if the driver refused step-down and was closed
/// by drop), together with the batch outcome.
#[cfg(windows)]
pub(super) fn scan<'t, T>(
    source: twain_dsm::Source<'t, T>,
    native_transfer: bool,
    asked: &Settings,
    first_sheet: u32,
    events: &SyncSender<Event>,
    progress: &dyn Progress,
    mut on_request: impl FnMut(&twain_dsm::Opened<'t, T>, T),
) -> (Option<twain_dsm::Source<'t, T>>, Result<Done, ScanError>) {
    let taken = match settings::apply(&mut LiveSource(&source), asked, native_transfer) {
        Ok(taken) => taken,
        Err(err) => return (Some(source), Err(err)),
    };
    let applied_settings = taken.applied.settings.clone();
    let source_kind = applied_settings.source;
    if events.send(Event::Applied(taken.applied.clone())).is_err() {
        return (Some(source), Ok(Done::Cancelled));
    }

    // Checked after `Event::Applied` and before touching `CAP_FEEDERLOADED` or `MSG_ENABLEDS` so a
    // pre-cancelled tracker always ends `Done::Cancelled` (even on an empty feeder).
    if progress.check().is_err() {
        return (Some(source), Ok(Done::Cancelled));
    }

    if source_kind == Source::Feeder {
        let detectable =
            match settings::read_bool_cap(&mut LiveSource(&source), ffi::CAP_PAPERDETECTABLE) {
                Ok(v) => v,
                Err(err) => return (Some(source), Err(err)),
            };
        if detectable == Some(true) {
            let loaded =
                match settings::read_bool_cap(&mut LiveSource(&source), ffi::CAP_FEEDERLOADED) {
                    Ok(v) => v,
                    Err(err) => return (Some(source), Err(err)),
                };
            if loaded == Some(false) {
                return (Some(source), Err(ScanError::FeederEmpty));
            }
        }
    }

    let mut enabled = match source.enable(twain_dsm::Ui::Hidden) {
        Ok(enabled) => enabled,
        Err(refused) => {
            let (source, err) = refused.into_parts();
            return (Some(source), Err(error::map_enable(source_kind, &err)));
        }
    };

    let mut ready = loop {
        if progress.check().is_err() {
            return finish_with_enabled(enabled, Ok(Done::Cancelled));
        }
        match enabled.next_event(STATE5_POLL) {
            Ok(twain_dsm::Next::Ready(ready)) => break ready,
            Ok(twain_dsm::Next::Request(back, req)) => {
                on_request(back.opened(), req);
                enabled = back;
            }
            Ok(twain_dsm::Next::Timeout(back)) => {
                enabled = back;
            }
            Ok(twain_dsm::Next::Quit(back, _)) => {
                return finish_with_enabled(back, Ok(Done::Cancelled));
            }
            Ok(twain_dsm::Next::Event(back, twain_dsm::Event::CloseRequest)) => {
                return resolve_close_request(Some(back), None, source_kind, 0, 0, progress);
            }
            Ok(twain_dsm::Next::Event(back, twain_dsm::Event::CloseOk)) => {
                return finish_with_enabled(back, Ok(Done::Cancelled));
            }
            Ok(twain_dsm::Next::Event(back, _)) => {
                enabled = back;
            }
            Err(refused) => {
                let (back, err) = refused.into_parts();
                return finish_with_enabled(back, Err(error::map(&err)));
            }
        }
    };

    let mut tracker = SheetTracker::new(first_sheet, applied_settings.duplex);
    let mut pages_sent: u32 = 0;
    let mut last_bytes: u64 = 0;

    loop {
        while let Some(req) = ready.take_request() {
            on_request(ready.opened(), req);
        }

        // Sheet-granular cancellation: sheet 0 was checked before `enable` and while waiting in
        // state 5, so once sheet 0 enters `Ready` it finishes in flight (including its back side
        // on duplex); subsequent sheets check `progress` between sheets.
        if tracker.is_between_sheets() && progress.check().is_err() {
            if source_kind == Source::Feeder {
                let _ = ready.stop_feeder();
            }
            return finish_with_ready(ready, Ok(Done::Cancelled));
        }

        if let Err(err) = tracker.ensure_can_start_page() {
            if source_kind == Source::Feeder {
                let _ = ready.stop_feeder();
            }
            return finish_with_ready(ready, Err(err));
        }

        let inferred_side = tracker.inferred_side();
        match transfer_one_page(ready, &taken, inferred_side, pages_sent, progress) {
            TransferOutcome::Page {
                raw,
                dpi,
                side,
                side_known,
                bytes_read,
                pending,
            } => {
                let (emit_sheet, page_sheet) = match tracker.accept_page(side) {
                    Ok(pair) => pair,
                    Err(err) => return finish_with_pending(pending, source_kind, Err(err)),
                };
                last_bytes = bytes_read;
                pages_sent = pages_sent.saturating_add(1);
                let page = RawPage::new(Pixels::Raw(raw), dpi, page_sheet, side, side_known);
                if let Some(sheet) = emit_sheet
                    && events.send(Event::Sheet(sheet)).is_err()
                {
                    return finish_with_pending(pending, source_kind, Ok(Done::Cancelled));
                }
                if events.send(Event::Page(page)).is_err() {
                    return finish_with_pending(pending, source_kind, Ok(Done::Cancelled));
                }
                if source_kind == Source::Flatbed {
                    return finish_with_pending(pending, source_kind, Ok(Done::OnePage));
                }
                match pending {
                    twain_dsm::Pending::More(next_ready, _) => {
                        ready = next_ready;
                    }
                    twain_dsm::Pending::Done(back) => {
                        return finish_with_enabled(back, Ok(Done::FeederEmpty));
                    }
                }
            }
            TransferOutcome::FeederEmpty(src) => {
                if pages_sent > 0 {
                    progress.set_total(Some(last_bytes), Unit::Bytes);
                    progress.set(last_bytes);
                    return (src, Ok(Done::FeederEmpty));
                }
                return (src, Err(ScanError::FeederEmpty));
            }
            TransferOutcome::Cancelled(src) => {
                return (src, Ok(Done::Cancelled));
            }
            TransferOutcome::CloseRequested(back, pre_status) => {
                return resolve_close_request(
                    back,
                    pre_status,
                    source_kind,
                    pages_sent,
                    last_bytes,
                    progress,
                );
            }
            TransferOutcome::Failed(src, err) => {
                return (src, Err(err));
            }
        }
    }
}

#[cfg(windows)]
enum TransferOutcome<'t, T> {
    Page {
        raw: RawImage,
        dpi: (f32, f32),
        side: Side,
        side_known: SideKnown,
        bytes_read: u64,
        pending: twain_dsm::Pending<'t, T>,
    },
    FeederEmpty(Option<twain_dsm::Source<'t, T>>),
    Cancelled(Option<twain_dsm::Source<'t, T>>),
    CloseRequested(
        Option<twain_dsm::Enabled<'t, T>>,
        Option<twain_dsm::ConditionCode>,
    ),
    Failed(Option<twain_dsm::Source<'t, T>>, ScanError),
}

/// Synthesizes an undefined-dimensions (`-1 × -1`) [`ImageDesc`] from `format` for drivers under
/// `TWSX_MEMORY` + `ICAP_UNDEFINEDIMAGESIZE` that defer `DAT_IMAGEINFO` until state 7.
#[cfg(windows)]
fn undefined_desc_for_format(format: Format) -> ImageDesc {
    let (pixel_type, samples_per_pixel, bits_per_sample, bits_per_pixel) = match format {
        Format::Bw1 => (ffi::TWPT_BW as i16, 1, 1, 1),
        Format::Gray8 => (ffi::TWPT_GRAY as i16, 1, 8, 8),
        Format::Gray16 => (ffi::TWPT_GRAY as i16, 1, 16, 16),
        Format::Rgb8 => (ffi::TWPT_RGB as i16, 3, 8, 24),
        Format::Rgb16 => (ffi::TWPT_RGB as i16, 3, 16, 48),
    };
    ImageDesc {
        width: -1,
        length: -1,
        samples_per_pixel,
        bits_per_sample,
        bits_per_pixel,
        planar: false,
        pixel_type,
        compression: ffi::TWCP_NONE,
    }
}

/// Checks whether `err` failed specifically with `TWCC_NOMEDIA` on `Source::Feeder`.
#[cfg(windows)]
fn is_feeder_nomedia(source_kind: Source, err: &twain_dsm::Error) -> bool {
    source_kind == Source::Feeder
        && matches!(
            err,
            twain_dsm::Error::Failed {
                by_source: Some(twain_dsm::ConditionCode::NOMEDIA),
                ..
            } | twain_dsm::Error::Failed {
                by_source: None,
                by_dsm: Some(twain_dsm::ConditionCode::NOMEDIA),
                ..
            }
        )
}

/// Transfers one page in state 6 via `taken.transfer` (`Memory` or `Native`), reporting byte
/// progress on `progress` and ending the transfer (`MSG_ENDXFER`).
#[cfg(windows)]
fn transfer_one_page<'t, T>(
    ready: twain_dsm::Ready<'t, T>,
    taken: &Taken,
    inferred_side: Side,
    pages_sent: u32,
    progress: &dyn Progress,
) -> TransferOutcome<'t, T> {
    let source_kind = taken.applied.settings.source;
    progress.set_total(None, Unit::Bytes);
    progress.set(0);

    let (desc, page_dpi) = match ready.image_info() {
        Ok(info) => (
            ImageDesc::from(info),
            dpi_from_image_res(info.x_resolution, info.y_resolution, taken.units, taken.dpi),
        ),
        Err(twain_dsm::Error::CloseRequested) => {
            let status = ready.status();
            return TransferOutcome::CloseRequested(ready.reset().ok(), status);
        }
        Err(err) => {
            if is_feeder_nomedia(source_kind, &err) {
                return TransferOutcome::FeederEmpty(step_down_ready(ready).ok());
            }
            let mapped = error::map(&err);
            if taken.transfer == TransferMech::Memory
                && taken.applied.settings.paper == Paper::Auto
                && !error::is_fatal_scan_error(&mapped)
            {
                (
                    undefined_desc_for_format(taken.applied.settings.format),
                    taken.dpi,
                )
            } else {
                return TransferOutcome::Failed(step_down_ready(ready).ok(), mapped);
            }
        }
    };

    match taken.transfer {
        TransferMech::Memory => transfer_memory(
            ready,
            &desc,
            page_dpi,
            taken,
            inferred_side,
            pages_sent,
            progress,
        ),
        TransferMech::Native => {
            transfer_native(ready, source_kind, page_dpi, inferred_side, progress)
        }
    }
}

#[cfg(windows)]
fn transfer_memory<'t, T>(
    ready: twain_dsm::Ready<'t, T>,
    desc: &ImageDesc,
    page_dpi: (f32, f32),
    taken: &Taken,
    inferred_side: Side,
    pages_sent: u32,
    progress: &dyn Progress,
) -> TransferOutcome<'t, T> {
    let source_kind = taken.applied.settings.source;
    let mut assembler = match pixels::Assembler::new(desc) {
        Ok(asm) => asm,
        Err(err) => return TransferOutcome::Failed(step_down_ready(ready).ok(), err),
    };

    let mut transfer = match ready.memory() {
        Ok(t) => t,
        Err(refused) => {
            let (ready, err) = refused.into_parts();
            if matches!(err, twain_dsm::Error::CloseRequested) {
                let status = ready.status();
                return TransferOutcome::CloseRequested(ready.reset().ok(), status);
            }
            if is_feeder_nomedia(source_kind, &err) {
                return TransferOutcome::FeederEmpty(step_down_ready(ready).ok());
            }
            let mapped = error::map(&err);
            return TransferOutcome::Failed(step_down_ready(ready).ok(), mapped);
        }
    };

    let preferred = transfer.preferred_size();
    let alloc_len = preferred.saturating_add(4);
    let mut storage = Vec::new();
    if storage.try_reserve_exact(alloc_len).is_err() {
        let src = abort_memory_transfer(transfer, source_kind);
        return TransferOutcome::Failed(
            src,
            error::driver_msg(format!(
                "couldn't allocate a {alloc_len}-byte memory transfer buffer"
            )),
        );
    }
    storage.resize(alloc_len, 0);
    let buf = aligned_slice(&mut storage, preferred);

    let mut total: Option<u64> = None;
    let mut bytes_read: u64 = 0;

    loop {
        match transfer.next_buffer(buf) {
            Ok(Some(strip)) => {
                if total.is_none()
                    && desc.width > 0
                    && desc.length > 0
                    && strip.x_offset == 0
                    && u64::from(strip.columns) == (desc.width as u64)
                {
                    let expected =
                        u64::from(strip.bytes_per_row).saturating_mul(desc.length as u64);
                    total = Some(expected);
                    progress.set_total(total, Unit::Bytes);
                }
                bytes_read = bytes_read.saturating_add(strip.bytes.len() as u64);
                if total.is_some_and(|t| bytes_read > t) {
                    total = Some(bytes_read);
                    progress.set_total(total, Unit::Bytes);
                }
                progress.set(bytes_read);

                if let Err(err) = assembler.place(strip.into()) {
                    let src = abort_memory_transfer(transfer, source_kind);
                    return TransferOutcome::Failed(src, err);
                }
            }
            Ok(None) => break,
            Err(twain_dsm::Error::CloseRequested) => {
                let status = transfer.status();
                return TransferOutcome::CloseRequested(transfer.reset().ok(), status);
            }
            Err(err) => {
                if bytes_read == 0 && is_feeder_nomedia(source_kind, &err) {
                    let src = abort_memory_transfer(transfer, source_kind);
                    return TransferOutcome::FeederEmpty(src);
                }
                if pages_sent == 0 && is_feeder_nomedia(source_kind, &err) {
                    let src = abort_memory_transfer(transfer, source_kind);
                    return TransferOutcome::Failed(src, ScanError::FeederEmpty);
                }
                let mapped = error::map(&err);
                let src = abort_memory_transfer(transfer, source_kind);
                return TransferOutcome::Failed(src, mapped);
            }
        }
    }

    if transfer.cancelled() {
        let src = abort_memory_transfer(transfer, source_kind);
        return TransferOutcome::Cancelled(src);
    }

    let state7_dims = match transfer.image_info() {
        Ok(i) => Some((i.width, i.length)),
        Err(twain_dsm::Error::CloseRequested) => {
            let status = transfer.status();
            return TransferOutcome::CloseRequested(transfer.reset().ok(), status);
        }
        Err(_) => None,
    };
    let ext = match transfer.ext_image_info(&[ffi::TWEI_PAGESIDE]) {
        Ok(items) => Some(items),
        Err(twain_dsm::Error::CloseRequested) => {
            let status = transfer.status();
            return TransferOutcome::CloseRequested(transfer.reset().ok(), status);
        }
        Err(_) => None,
    };
    let (side, side_known) = page_side_from_ext(ext.as_deref(), inferred_side);

    let raw = match assembler.finish(taken.polarity, state7_dims) {
        Ok(raw) => raw,
        Err(err) => {
            let src = abort_memory_transfer(transfer, source_kind);
            return TransferOutcome::Failed(src, err);
        }
    };

    if total != Some(bytes_read) {
        progress.set_total(Some(bytes_read), Unit::Bytes);
        progress.set(bytes_read);
    }

    match transfer.end() {
        Ok(pending) => TransferOutcome::Page {
            raw,
            dpi: page_dpi,
            side,
            side_known,
            bytes_read,
            pending,
        },
        Err(refused) => {
            let (transfer, err) = refused.into_parts();
            if matches!(err, twain_dsm::Error::CloseRequested) {
                let status = transfer.status();
                TransferOutcome::CloseRequested(transfer.reset().ok(), status)
            } else {
                let src = abort_memory_transfer(transfer, source_kind);
                TransferOutcome::Failed(src, error::map(&err))
            }
        }
    }
}

#[cfg(windows)]
fn transfer_native<'t, T>(
    ready: twain_dsm::Ready<'t, T>,
    source_kind: Source,
    page_dpi: (f32, f32),
    inferred_side: Side,
    progress: &dyn Progress,
) -> TransferOutcome<'t, T> {
    let (dib, transferred) = match ready.native() {
        Ok(twain_dsm::Native::Done(dib, transferred)) => (dib, transferred),
        Ok(twain_dsm::Native::Cancelled(transferred)) => {
            let src = abort_native_transfer(transferred, source_kind);
            return TransferOutcome::Cancelled(src);
        }
        Err(refused) => {
            let (ready, err) = refused.into_parts();
            if matches!(err, twain_dsm::Error::CloseRequested) {
                let status = ready.status();
                return TransferOutcome::CloseRequested(ready.reset().ok(), status);
            }
            if is_feeder_nomedia(source_kind, &err) {
                return TransferOutcome::FeederEmpty(step_down_ready(ready).ok());
            }
            let mapped = error::map(&err);
            return TransferOutcome::Failed(step_down_ready(ready).ok(), mapped);
        }
    };

    let bytes_read = dib.bytes().len() as u64;
    progress.set_total(Some(bytes_read), Unit::Bytes);
    progress.set(bytes_read);

    let decoded = pixels::decode_dib(dib.bytes());
    drop(dib);

    let raw = match decoded {
        Ok(raw) => raw,
        Err(err) => {
            let src = abort_native_transfer(transferred, source_kind);
            return TransferOutcome::Failed(src, err);
        }
    };

    let ext = match transferred.ext_image_info(&[ffi::TWEI_PAGESIDE]) {
        Ok(items) => Some(items),
        Err(twain_dsm::Error::CloseRequested) => {
            let status = transferred.status();
            return TransferOutcome::CloseRequested(transferred.reset().ok(), status);
        }
        Err(_) => None,
    };
    let (side, side_known) = page_side_from_ext(ext.as_deref(), inferred_side);

    match transferred.end() {
        Ok(pending) => TransferOutcome::Page {
            raw,
            dpi: page_dpi,
            side,
            side_known,
            bytes_read,
            pending,
        },
        Err(refused) => {
            let (transferred, err) = refused.into_parts();
            if matches!(err, twain_dsm::Error::CloseRequested) {
                let status = transferred.status();
                TransferOutcome::CloseRequested(transferred.reset().ok(), status)
            } else {
                let src = abort_native_transfer(transferred, source_kind);
                TransferOutcome::Failed(src, error::map(&err))
            }
        }
    }
}

#[cfg(windows)]
fn resolve_close_request<'t, T>(
    enabled: Option<twain_dsm::Enabled<'t, T>>,
    pre_status: Option<twain_dsm::ConditionCode>,
    source_kind: Source,
    pages_sent: u32,
    last_bytes: u64,
    progress: &dyn Progress,
) -> (Option<twain_dsm::Source<'t, T>>, Result<Done, ScanError>) {
    let Some(enabled) = enabled else {
        let outcome = error::unexpected_close_request(source_kind, pages_sent, pre_status, None);
        if outcome == Ok(Done::FeederEmpty) && pages_sent > 0 {
            progress.set_total(Some(last_bytes), Unit::Bytes);
            progress.set(last_bytes);
        }
        return (None, outcome);
    };

    if enabled.forced_ui() {
        return finish_with_enabled(enabled, Ok(Done::Cancelled));
    }
    let status = pre_status.or_else(|| enabled.status());
    let src = match enabled.disable() {
        Ok(src) => src,
        Err(refused) => {
            if let Err(specific) =
                error::unexpected_close_request(source_kind, pages_sent, status, None)
                && !matches!(specific, ScanError::Driver { shown: false, .. })
            {
                return (None, Err(specific));
            }
            let err = refused.into_error();
            return (None, Err(error::map(&err)));
        }
    };
    let feeder_loaded = if source_kind == Source::Feeder {
        settings::read_bool_cap(&mut LiveSource(&src), ffi::CAP_FEEDERLOADED)
            .ok()
            .flatten()
    } else {
        None
    };
    let outcome = error::unexpected_close_request(source_kind, pages_sent, status, feeder_loaded);
    if outcome == Ok(Done::FeederEmpty) && pages_sent > 0 {
        progress.set_total(Some(last_bytes), Unit::Bytes);
        progress.set(last_bytes);
    }
    (Some(src), outcome)
}

#[cfg(windows)]
fn finish_with_enabled<'t, T>(
    enabled: twain_dsm::Enabled<'t, T>,
    outcome: Result<Done, ScanError>,
) -> (Option<twain_dsm::Source<'t, T>>, Result<Done, ScanError>) {
    match enabled.disable() {
        Ok(src) => (Some(src), outcome),
        // Preserve `outcome` (all delivered pages remain valid; `None` marks the device for
        // close + reopen before its next operation).
        Err(_) => (None, outcome),
    }
}

#[cfg(windows)]
fn finish_with_ready<'t, T>(
    ready: twain_dsm::Ready<'t, T>,
    outcome: Result<Done, ScanError>,
) -> (Option<twain_dsm::Source<'t, T>>, Result<Done, ScanError>) {
    match step_down_ready(ready) {
        Ok(src) => (Some(src), outcome),
        Err(_) => (None, outcome),
    }
}

#[cfg(windows)]
fn finish_with_pending<'t, T>(
    pending: twain_dsm::Pending<'t, T>,
    source_kind: Source,
    outcome: Result<Done, ScanError>,
) -> (Option<twain_dsm::Source<'t, T>>, Result<Done, ScanError>) {
    match step_down_pending(pending, source_kind) {
        Ok(src) => (Some(src), outcome),
        Err(_) => (None, outcome),
    }
}

#[cfg(windows)]
fn step_down_ready<'t, T>(
    ready: twain_dsm::Ready<'t, T>,
) -> Result<twain_dsm::Source<'t, T>, ScanError> {
    let enabled = ready.reset().map_err(|r| error::map(&r.into_error()))?;
    enabled.disable().map_err(|r| error::map(&r.into_error()))
}

#[cfg(windows)]
fn step_down_pending<'t, T>(
    pending: twain_dsm::Pending<'t, T>,
    source_kind: Source,
) -> Result<twain_dsm::Source<'t, T>, ScanError> {
    match pending {
        twain_dsm::Pending::More(mut ready, _) => {
            if source_kind == Source::Feeder {
                let _ = ready.stop_feeder();
            }
            step_down_ready(ready)
        }
        twain_dsm::Pending::Done(enabled) => {
            enabled.disable().map_err(|r| error::map(&r.into_error()))
        }
    }
}

#[cfg(windows)]
fn abort_memory_transfer<'t, T>(
    transfer: twain_dsm::MemoryTransfer<'t, T>,
    source_kind: Source,
) -> Option<twain_dsm::Source<'t, T>> {
    match transfer.end() {
        Ok(p) => step_down_pending(p, source_kind).ok(),
        Err(refused) => refused
            .into_parts()
            .0
            .reset()
            .ok()
            .and_then(|e| e.disable().ok()),
    }
}

#[cfg(windows)]
fn abort_native_transfer<'t, T>(
    transferred: twain_dsm::Transferred<'t, T>,
    source_kind: Source,
) -> Option<twain_dsm::Source<'t, T>> {
    match transferred.end() {
        Ok(p) => step_down_pending(p, source_kind).ok(),
        Err(refused) => refused
            .into_parts()
            .0
            .reset()
            .ok()
            .and_then(|e| e.disable().ok()),
    }
}

#[cfg(test)]
mod tests {
    use twain_dsm::{Item, ItemType, Items, ReturnCode};

    use super::*;

    #[test]
    fn aligned_slice_is_always_four_byte_aligned_with_requested_len() {
        let mut backing = [0u8; 32];
        for start in 0..4 {
            let slice = aligned_slice(&mut backing[start..], 16);
            assert_eq!(slice.len(), 16);
            assert_eq!((slice.as_ptr() as usize) % 4, 0, "start offset {start}");
        }
    }

    #[test]
    fn page_side_maps_top_bottom_and_treats_both_or_absent_as_inferred() {
        let make = |val: u16| {
            vec![ExtInfo {
                id: ffi::TWEI_PAGESIDE,
                value: ExtValue::Items(
                    Items::new(ItemType::UInt16, vec![Item::UInt16(val)]).unwrap(),
                ),
            }]
        };

        assert_eq!(
            page_side_from_ext(Some(&make(ffi::TWCS_TOP)), Side::Back),
            (Side::Front, SideKnown::Reported)
        );
        assert_eq!(
            page_side_from_ext(Some(&make(ffi::TWCS_BOTTOM)), Side::Front),
            (Side::Back, SideKnown::Reported)
        );
        // `TWCS_BOTH` (0) doesn't disambiguate a single page's side, so keep inferred order.
        assert_eq!(
            page_side_from_ext(Some(&make(ffi::TWCS_BOTH)), Side::Front),
            (Side::Front, SideKnown::Inferred)
        );
        assert_eq!(
            page_side_from_ext(Some(&make(ffi::TWCS_BOTH)), Side::Back),
            (Side::Back, SideKnown::Inferred)
        );

        let unsupported = vec![ExtInfo {
            id: ffi::TWEI_PAGESIDE,
            value: ExtValue::Code(ReturnCode::INFONOTSUPPORTED),
        }];
        assert_eq!(
            page_side_from_ext(Some(&unsupported), Side::Back),
            (Side::Back, SideKnown::Inferred)
        );
        assert_eq!(
            page_side_from_ext(None, Side::Front),
            (Side::Front, SideKnown::Inferred)
        );
    }

    #[test]
    fn dpi_from_image_res_converts_active_units_and_falls_back_on_non_positive() {
        let cm_300 = Fix32::from_f64(300.0 / 2.54).unwrap();
        let (x, y) = dpi_from_image_res(cm_300, cm_300, Units::Centimeters, (150.0, 150.0));
        assert!((x - 300.0).abs() < 0.05, "x={x}");
        assert!((y - 300.0).abs() < 0.05, "y={y}");

        assert_eq!(
            dpi_from_image_res(
                Fix32::from_int(0),
                Fix32::from_int(-1),
                Units::Inches,
                (200.0, 400.0)
            ),
            (200.0, 400.0)
        );
    }

    #[test]
    fn sheet_tracker_pairs_normal_duplex_and_checks_cancellation_only_between_sheets() {
        let mut t = SheetTracker::new(5, true);
        assert!(!t.is_between_sheets());
        assert_eq!(t.inferred_side(), Side::Front);
        assert_eq!(t.accept_page(Side::Front).unwrap(), (Some(5), 5));
        // Mid-sheet after side 0: must NOT allow sheet-granular cancellation!
        assert!(!t.is_between_sheets());
        assert_eq!(t.inferred_side(), Side::Back);

        assert_eq!(t.accept_page(Side::Back).unwrap(), (None, 5));
        // Now sheet 5 is complete -> between sheets!
        assert!(t.is_between_sheets());
        assert_eq!(t.inferred_side(), Side::Front);

        assert_eq!(t.accept_page(Side::Front).unwrap(), (Some(6), 6));
        assert!(!t.is_between_sheets());
        assert_eq!(t.accept_page(Side::Back).unwrap(), (None, 6));
        assert!(t.is_between_sheets());
    }

    #[test]
    fn sheet_tracker_handles_blank_back_skip_stuck_top_and_back_first_duplex() {
        // 1) Hardware blank-page skip drops sheet 0's back (`Front`, `Front`, `Back`):
        //    sheet 0 closes as single-sided and sheet 1 pairs `(Front, Back)` without duplicate
        //    `(sheet, side)` or half-sheet misalignment!
        let mut skip = SheetTracker::new(0, true);
        assert_eq!(skip.accept_page(Side::Front).unwrap(), (Some(0), 0));
        assert_eq!(skip.accept_page(Side::Front).unwrap(), (Some(1), 1));
        assert!(!skip.is_between_sheets());
        assert_eq!(skip.accept_page(Side::Back).unwrap(), (None, 1));
        assert!(skip.is_between_sheets());

        // 2) Driver stuck reporting `TWCS_TOP` (`Side::Front`) on every duplex transfer:
        //    every transfer opens its own sheet cleanly instead of emitting two `Front`s on one
        //    sheet.
        let mut stuck = SheetTracker::new(10, true);
        assert_eq!(stuck.accept_page(Side::Front).unwrap(), (Some(10), 10));
        assert_eq!(stuck.accept_page(Side::Front).unwrap(), (Some(11), 11));
        assert_eq!(stuck.accept_page(Side::Front).unwrap(), (Some(12), 12));

        // 3) Back-first duplex scanner reporting `TWCS_BOTTOM` on side 0 and omitting
        //    `TWEI_PAGESIDE` on side 1: `inferred_side()` flips to `Side::Front` so both sides
        //    land on sheet 0!
        let mut back_first = SheetTracker::new(0, true);
        let (s0, _) = page_side_from_ext(
            Some(&[ExtInfo {
                id: ffi::TWEI_PAGESIDE,
                value: ExtValue::Items(
                    Items::new(ItemType::UInt16, vec![Item::UInt16(ffi::TWCS_BOTTOM)]).unwrap(),
                ),
            }]),
            back_first.inferred_side(),
        );
        assert_eq!(s0, Side::Back);
        assert_eq!(back_first.accept_page(s0).unwrap(), (Some(0), 0));
        assert_eq!(back_first.inferred_side(), Side::Front);
        let (s1, k1) = page_side_from_ext(None, back_first.inferred_side());
        assert_eq!((s1, k1), (Side::Front, SideKnown::Inferred));
        assert_eq!(back_first.accept_page(s1).unwrap(), (None, 0));
        assert!(back_first.is_between_sheets());

        // 4) `u32::MAX` overflow is caught both before a sheet starts and when a duplicate side
        //    forces advancing past `u32::MAX`.
        let mut max_simplex = SheetTracker::new(u32::MAX, false);
        assert!(max_simplex.ensure_can_start_page().is_ok());
        assert_eq!(
            max_simplex.accept_page(Side::Front).unwrap(),
            (Some(u32::MAX), u32::MAX)
        );
        assert!(max_simplex.ensure_can_start_page().is_err());

        let mut max_dup = SheetTracker::new(u32::MAX, true);
        assert_eq!(
            max_dup.accept_page(Side::Front).unwrap(),
            (Some(u32::MAX), u32::MAX)
        );
        assert!(max_dup.accept_page(Side::Front).is_err());
    }
}
