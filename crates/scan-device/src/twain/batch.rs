//! Running a TWAIN batch scan: negotiating settings, pre-checking feeder state, pumping state 5
//! with 100 ms cancellation checks and concurrent `Remote` request servicing, transferring pages
//! via `TWSX_MEMORY` or `TWSX_NATIVE`, reading `TWEI_PAGESIDE`, and stepping cleanly back to
//! state 4 on completion, cancellation, or error.

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
#[cfg(windows)]
use super::error;
#[cfg(windows)]
use super::pixels::{self, ImageDesc};
#[cfg(windows)]
use super::settings::{self, Taken, TransferMech};
#[cfg(windows)]
use crate::{Done, Event, Pixels, RawImage, RawPage, ScanError, Settings, Source};
use crate::{Side, SideKnown};

/// Polling interval while waiting in state 5 (`MSG_ENABLEDS` -> `MSG_XFERREADY`) so sheet
/// cancellation is noticed promptly even if a source stalls before producing its first page.
#[cfg(windows)]
const STATE5_POLL: Duration = Duration::from_millis(100);

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
            && let Some(raw) = first.as_i64()
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
    if events.send(Event::Applied(taken.applied.clone())).is_err() {
        return (Some(source), Ok(Done::Cancelled));
    }

    // Checked after `Event::Applied` and before touching `CAP_FEEDERLOADED` or `MSG_ENABLEDS` so a
    // pre-cancelled tracker always ends `Done::Cancelled` (even on an empty feeder).
    if progress.check().is_err() {
        return (Some(source), Ok(Done::Cancelled));
    }

    if applied_settings.source == Source::Feeder {
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
            return (
                Some(source),
                Err(error::map_enable(applied_settings.source, &err)),
            );
        }
    };

    let mut ready = loop {
        match enabled.next_event(STATE5_POLL) {
            Ok(twain_dsm::Next::Ready(ready)) => break ready,
            Ok(twain_dsm::Next::Request(back, req)) => {
                on_request(back.opened(), req);
                enabled = back;
            }
            Ok(twain_dsm::Next::Timeout(back)) => {
                if progress.check().is_err() {
                    return finish_with_enabled(back, Ok(Done::Cancelled));
                }
                enabled = back;
            }
            Ok(twain_dsm::Next::Quit(back, _)) => {
                return finish_with_enabled(back, Ok(Done::Cancelled));
            }
            Ok(twain_dsm::Next::Event(back, twain_dsm::Event::CloseRequest)) => {
                return resolve_close_request(back, applied_settings.source, 0, 0, progress);
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

    let sides: &[Side] = if applied_settings.duplex {
        &[Side::Front, Side::Back]
    } else {
        &[Side::Front]
    };

    let mut sheets: u64 = 0;
    let mut pages_sent: u32 = 0;
    let mut last_bytes: u64 = 0;

    loop {
        // Sheet-granular cancellation: sheet 0 was checked before `enable` and while waiting for
        // `MSG_XFERREADY`, so once sheet 0 enters `Ready` it finishes in flight; subsequent sheets
        // check `progress` before their front side is transferred.
        if sheets > 0 && progress.check().is_err() {
            let _ = ready.stop_feeder();
            return finish_with_ready(ready, Ok(Done::Cancelled));
        }

        let Ok(sheet) = u32::try_from(u64::from(first_sheet) + sheets) else {
            let _ = ready.stop_feeder();
            return finish_with_ready(
                ready,
                Err(error::driver_msg(format!(
                    "no sheet is numbered past {}: the sheet after it stays in the feeder",
                    u32::MAX
                ))),
            );
        };

        for (side_idx, &inferred_side) in sides.iter().enumerate() {
            match transfer_one_page(ready, &taken, inferred_side, progress) {
                TransferOutcome::Page {
                    raw,
                    dpi,
                    side,
                    side_known,
                    bytes_read,
                    pending,
                } => {
                    last_bytes = bytes_read;
                    pages_sent = pages_sent.saturating_add(1);
                    let page = RawPage::new(Pixels::Raw(raw), dpi, sheet, side, side_known);
                    if side_idx == 0 && events.send(Event::Sheet(sheet)).is_err() {
                        return finish_with_pending(pending, Ok(Done::Cancelled));
                    }
                    if events.send(Event::Page(page)).is_err() {
                        return finish_with_pending(pending, Ok(Done::Cancelled));
                    }
                    if applied_settings.source == Source::Flatbed {
                        return finish_with_pending(pending, Ok(Done::OnePage));
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
                TransferOutcome::Cancelled(src) => {
                    return (src, Ok(Done::Cancelled));
                }
                TransferOutcome::CloseRequested(Some(back)) => {
                    return resolve_close_request(
                        back,
                        applied_settings.source,
                        pages_sent,
                        last_bytes,
                        progress,
                    );
                }
                TransferOutcome::CloseRequested(None) => {
                    let outcome = error::unexpected_close_request(
                        applied_settings.source,
                        pages_sent,
                        None,
                        None,
                    );
                    if outcome == Ok(Done::FeederEmpty) && pages_sent > 0 {
                        progress.set_total(Some(last_bytes), Unit::Bytes);
                        progress.set(last_bytes);
                    }
                    return (None, outcome);
                }
                TransferOutcome::Failed(src, err) => {
                    return (src, Err(err));
                }
            }
        }

        sheets += 1;
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
    Cancelled(Option<twain_dsm::Source<'t, T>>),
    CloseRequested(Option<twain_dsm::Enabled<'t, T>>),
    Failed(Option<twain_dsm::Source<'t, T>>, ScanError),
}

/// Transfers one page in state 6 via `taken.transfer` (`Memory` or `Native`), reporting byte
/// progress on `progress` and ending the transfer (`MSG_ENDXFER`).
#[cfg(windows)]
fn transfer_one_page<'t, T>(
    ready: twain_dsm::Ready<'t, T>,
    taken: &Taken,
    inferred_side: Side,
    progress: &dyn Progress,
) -> TransferOutcome<'t, T> {
    progress.set_total(None, Unit::Bytes);
    progress.set(0);

    let info = match ready.image_info() {
        Ok(info) => info,
        Err(twain_dsm::Error::CloseRequested) => {
            return TransferOutcome::CloseRequested(ready.reset().ok());
        }
        Err(err) => {
            let mapped = error::map(&err);
            return TransferOutcome::Failed(step_down_ready(ready).ok(), mapped);
        }
    };
    let desc = ImageDesc::from(info);
    let page_dpi = dpi_from_image_res(info.x_resolution, info.y_resolution, taken.units, taken.dpi);

    match taken.transfer {
        TransferMech::Memory => {
            transfer_memory(ready, &desc, page_dpi, taken, inferred_side, progress)
        }
        TransferMech::Native => transfer_native(ready, page_dpi, inferred_side, progress),
    }
}

#[cfg(windows)]
fn transfer_memory<'t, T>(
    ready: twain_dsm::Ready<'t, T>,
    desc: &ImageDesc,
    page_dpi: (f32, f32),
    taken: &Taken,
    inferred_side: Side,
    progress: &dyn Progress,
) -> TransferOutcome<'t, T> {
    let mut assembler = match pixels::Assembler::new(desc) {
        Ok(asm) => asm,
        Err(err) => return TransferOutcome::Failed(step_down_ready(ready).ok(), err),
    };

    let mut transfer = match ready.memory() {
        Ok(t) => t,
        Err(refused) => {
            let (ready, err) = refused.into_parts();
            if matches!(err, twain_dsm::Error::CloseRequested) {
                return TransferOutcome::CloseRequested(ready.reset().ok());
            }
            let mapped = error::map(&err);
            return TransferOutcome::Failed(step_down_ready(ready).ok(), mapped);
        }
    };

    let preferred = transfer.preferred_size();
    let mut storage = vec![0u8; preferred + 4];
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
                    let src = abort_memory_transfer(transfer);
                    return TransferOutcome::Failed(src, err);
                }
            }
            Ok(None) => break,
            Err(twain_dsm::Error::CloseRequested) => {
                return TransferOutcome::CloseRequested(memory_to_enabled(transfer));
            }
            Err(err) => {
                let mapped = error::map(&err);
                let src = abort_memory_transfer(transfer);
                return TransferOutcome::Failed(src, mapped);
            }
        }
    }

    if transfer.cancelled() {
        let src = abort_memory_transfer(transfer);
        return TransferOutcome::Cancelled(src);
    }

    let state7_dims = transfer.image_info().ok().map(|i| (i.width, i.length));
    let ext = transfer.ext_image_info(&[ffi::TWEI_PAGESIDE]).ok();
    let (side, side_known) = page_side_from_ext(ext.as_deref(), inferred_side);

    let raw = match assembler.finish(taken.polarity, state7_dims) {
        Ok(raw) => raw,
        Err(err) => {
            let src = abort_memory_transfer(transfer);
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
            drop(transfer);
            if matches!(err, twain_dsm::Error::CloseRequested) {
                TransferOutcome::CloseRequested(None)
            } else {
                TransferOutcome::Failed(None, error::map(&err))
            }
        }
    }
}

#[cfg(windows)]
fn transfer_native<'t, T>(
    ready: twain_dsm::Ready<'t, T>,
    page_dpi: (f32, f32),
    inferred_side: Side,
    progress: &dyn Progress,
) -> TransferOutcome<'t, T> {
    let (dib, transferred) = match ready.native() {
        Ok(twain_dsm::Native::Done(dib, transferred)) => (dib, transferred),
        Ok(twain_dsm::Native::Cancelled(transferred)) => {
            let src = transferred
                .end()
                .ok()
                .and_then(|p| step_down_pending(p).ok());
            return TransferOutcome::Cancelled(src);
        }
        Err(refused) => {
            let (ready, err) = refused.into_parts();
            if matches!(err, twain_dsm::Error::CloseRequested) {
                return TransferOutcome::CloseRequested(ready.reset().ok());
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
            let src = transferred
                .end()
                .ok()
                .and_then(|p| step_down_pending(p).ok());
            return TransferOutcome::Failed(src, err);
        }
    };

    let ext = transferred.ext_image_info(&[ffi::TWEI_PAGESIDE]).ok();
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
            drop(transferred);
            if matches!(err, twain_dsm::Error::CloseRequested) {
                TransferOutcome::CloseRequested(None)
            } else {
                TransferOutcome::Failed(None, error::map(&err))
            }
        }
    }
}

#[cfg(windows)]
fn resolve_close_request<'t, T>(
    enabled: twain_dsm::Enabled<'t, T>,
    source_kind: Source,
    pages_sent: u32,
    last_bytes: u64,
    progress: &dyn Progress,
) -> (Option<twain_dsm::Source<'t, T>>, Result<Done, ScanError>) {
    if enabled.forced_ui() {
        return finish_with_enabled(enabled, Ok(Done::Cancelled));
    }
    let status = enabled.status();
    let src = match enabled.disable() {
        Ok(src) => src,
        Err(refused) => {
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
        Err(refused) => {
            let err = refused.into_error();
            match outcome {
                Ok(_) => (None, Err(error::map(&err))),
                Err(primary) => (None, Err(primary)),
            }
        }
    }
}

#[cfg(windows)]
fn finish_with_ready<'t, T>(
    ready: twain_dsm::Ready<'t, T>,
    outcome: Result<Done, ScanError>,
) -> (Option<twain_dsm::Source<'t, T>>, Result<Done, ScanError>) {
    match step_down_ready(ready) {
        Ok(src) => (Some(src), outcome),
        Err(step_err) => match outcome {
            Ok(_) => (None, Err(step_err)),
            Err(primary) => (None, Err(primary)),
        },
    }
}

#[cfg(windows)]
fn finish_with_pending<'t, T>(
    pending: twain_dsm::Pending<'t, T>,
    outcome: Result<Done, ScanError>,
) -> (Option<twain_dsm::Source<'t, T>>, Result<Done, ScanError>) {
    match step_down_pending(pending) {
        Ok(src) => (Some(src), outcome),
        Err(step_err) => match outcome {
            Ok(_) => (None, Err(step_err)),
            Err(primary) => (None, Err(primary)),
        },
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
) -> Result<twain_dsm::Source<'t, T>, ScanError> {
    match pending {
        twain_dsm::Pending::More(mut ready, _) => {
            let _ = ready.stop_feeder();
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
) -> Option<twain_dsm::Source<'t, T>> {
    transfer.end().ok().and_then(|p| step_down_pending(p).ok())
}

#[cfg(windows)]
fn memory_to_enabled<'t, T>(
    transfer: twain_dsm::MemoryTransfer<'t, T>,
) -> Option<twain_dsm::Enabled<'t, T>> {
    match transfer.end().ok()? {
        twain_dsm::Pending::Done(enabled) => Some(enabled),
        twain_dsm::Pending::More(ready, _) => ready.reset().ok(),
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
}
