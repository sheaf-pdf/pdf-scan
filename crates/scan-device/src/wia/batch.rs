//! Running a WIA 2.0 batch scan: negotiating settings (`Event::Applied`), streaming pages through
//! `IWiaTransfer::Download`, reporting `Status::WarmingUp` and monotonic byte progress, pairing
//! duplex sheets (`FRONT_FIRST` and `BACK_FIRST`) across one or many `Download` calls, deferring
//! `Event::Sheet` until a sheet's first page finishes decoding, falling back from `TYMED_CALLBACK`
//! to `TYMED_FILE` on 0-page medium refusal, and stopping cleanly on sheet boundaries via
//! `S_FALSE` + `WIA_CMD_STOP_FEEDER`.

use std::sync::mpsc::SyncSender;

use in_progress::{Progress, Unit};

use super::driver::{
    ItemId, TransferAction, TransferSink, WIA_CMD_STOP_FEEDER_GUID, WiaDeviceTree,
};
use super::error::{
    self, E_ABORT_HR, E_FAIL_HR, ERROR_CANCELLED_HR, S_FALSE_HR, S_OK_HR, STG_E_MEDIUMFULL_HR,
    WIA_ERROR_GENERAL_ERROR_HR, WIA_ERROR_WARMING_UP_HR, WIA_STATUS_END_OF_MEDIA_HR, WiaOp,
};
use super::pixels;
use super::settings::{self, Taken};
use crate::{Done, Event, RawPage, ScanError, Settings, Side, SideKnown, Source, Status};

/// Maximum consecutive 0-page `WarmingUp` / `Calibrating` `Download` retries before failing.
const MAX_WARMUP_RETRIES: u32 = 20;

/// Pure state machine tracking sheet numbers and side pairing across one or more `Download` calls.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct SheetTracker {
    first_sheet: u32,
    duplex: bool,
    back_first: bool,
    completed_sheets: u64,
    in_flight_first_done: bool,
}

impl SheetTracker {
    pub(super) fn new(first_sheet: u32, duplex: bool, back_first: bool) -> Self {
        SheetTracker {
            first_sheet,
            duplex,
            back_first,
            completed_sheets: 0,
            in_flight_first_done: false,
        }
    }

    /// True when the first side of a duplex sheet has been delivered and its second side is still
    /// expected — cancellation must wait until this becomes `false`.
    pub(super) fn mid_sheet(&self) -> bool {
        self.in_flight_first_done
    }

    /// Validates before starting a new sheet (`!mid_sheet()`) that its sheet number fits in `u32`,
    /// so an overflowing sheet stays in the feeder untransferred.
    pub(super) fn ensure_can_start_page(&self) -> Result<(), ScanError> {
        if !self.in_flight_first_done {
            let _ = self.sheet_at(self.completed_sheets)?;
        }
        Ok(())
    }

    /// Records a decoded page, returning `(emit_sheet, sheet_number, side)`.
    ///
    /// `emit_sheet` is `Some(n)` strictly on the first page of sheet `n` (even when `back_first`
    /// delivers `Side::Back` before `Side::Front`), and `None` on the second page of a duplex
    /// sheet.
    pub(super) fn accept_page(&mut self) -> Result<(Option<u32>, u32, Side), ScanError> {
        let sheet = self.sheet_at(self.completed_sheets)?;
        if !self.duplex {
            self.completed_sheets += 1;
            return Ok((Some(sheet), sheet, Side::Front));
        }
        if !self.in_flight_first_done {
            self.in_flight_first_done = true;
            let first_side = if self.back_first {
                Side::Back
            } else {
                Side::Front
            };
            Ok((Some(sheet), sheet, first_side))
        } else {
            self.in_flight_first_done = false;
            self.completed_sheets += 1;
            let second_side = if self.back_first {
                Side::Front
            } else {
                Side::Back
            };
            Ok((None, sheet, second_side))
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

/// Negotiates `asked` on `tree` and streams all pages of the batch onto `events`.
pub(super) fn run(
    tree: &mut dyn WiaDeviceTree,
    asked: &Settings,
    first_sheet: u32,
    events: &SyncSender<Event>,
    progress: &dyn Progress,
) -> Result<Done, ScanError> {
    let mut taken = settings::apply(tree, asked)?;
    if events.send(Event::Applied(taken.applied.clone())).is_err() {
        return Ok(Done::Cancelled);
    }

    // Check cancellation before starting sheet 0, matching `sane` and `fake`.
    if progress.check().is_err() {
        return Ok(Done::Cancelled);
    }

    let source = taken.applied.settings.source;
    let is_feeder = source == Source::Feeder;
    let mut tracker =
        SheetTracker::new(first_sheet, taken.applied.settings.duplex, taken.back_first);
    let mut total_pages: u64 = 0;
    let mut emitted_warming_up = false;
    let mut warmup_retries: u32 = 0;
    let mut retried_file_fallback = false;
    let mut last_completed_page_bytes: Option<u64> = None;
    let mut stream_opened_since_last_page = false;

    loop {
        if !tracker.mid_sheet() && progress.check().is_err() {
            if is_feeder && total_pages > 0 {
                stop_feeder_best_effort(tree, taken.item, taken.handling_item);
            }
            return Ok(Done::Cancelled);
        }

        let mut sink = BatchSink {
            events,
            progress,
            tracker: &mut tracker,
            taken: &taken,
            total_pages: &mut total_pages,
            emitted_warming_up: &mut emitted_warming_up,
            last_completed_page_bytes: &mut last_completed_page_bytes,
            stream_opened_since_last_page: &mut stream_opened_since_last_page,
            streams_in_call: 0,
            current_page_bytes: 0,
            cancelled_by_progress: false,
            receiver_dropped: false,
            stopped_flatbed_extra: false,
            fatal_err: None,
        };

        let outcome = tree.download(taken.item, &mut sink)?;
        let streams_in_call = sink.streams_in_call;
        let cancelled_by_progress = sink.cancelled_by_progress;
        let receiver_dropped = sink.receiver_dropped;
        let stopped_flatbed_extra = sink.stopped_flatbed_extra;
        let fatal_err = sink.fatal_err;

        if let Some(panic_msg) = outcome.callback_panic {
            if is_feeder {
                stop_feeder_best_effort(tree, taken.item, taken.handling_item);
            }
            return Err(error::driver_msg(format!(
                "internal callback panic during WIA transfer: {panic_msg}"
            )));
        }

        if receiver_dropped {
            if is_feeder {
                stop_feeder_best_effort(tree, taken.item, taken.handling_item);
            }
            return Ok(Done::Cancelled);
        }

        if cancelled_by_progress {
            if is_feeder {
                stop_feeder_best_effort(tree, taken.item, taken.handling_item);
            }
            return Ok(Done::Cancelled);
        }

        if let Some(err) = fatal_err {
            if is_feeder {
                stop_feeder_best_effort(tree, taken.item, taken.handling_item);
            }
            return Err(err);
        }

        if outcome.stream_overflowed {
            if is_feeder {
                stop_feeder_best_effort(tree, taken.item, taken.handling_item);
            }
            return Err(error::hresult(WiaOp::Download, STG_E_MEDIUMFULL_HR));
        }

        let cb_err = outcome
            .callback_error_hr
            .filter(|&hr| !error::is_warming_up(hr));
        let effective_hr = resolve_effective_hr(outcome.hr, cb_err);

        // Lamp / calibration warm-up retry when 0 pages transferred in this call.
        if streams_in_call == 0
            && (error::is_warming_up(effective_hr) || error::is_warming_up(outcome.hr))
        {
            if !emitted_warming_up {
                if events.send(Event::Status(Status::WarmingUp)).is_err() {
                    if is_feeder && total_pages > 0 {
                        stop_feeder_best_effort(tree, taken.item, taken.handling_item);
                    }
                    return Ok(Done::Cancelled);
                }
                emitted_warming_up = true;
            }
            if warmup_retries < MAX_WARMUP_RETRIES {
                warmup_retries += 1;
                continue;
            }
            return Err(error::hresult(WiaOp::Download, WIA_ERROR_WARMING_UP_HR));
        }

        // 0-page `TYMED_CALLBACK` -> `TYMED_FILE` fallback on medium/parameter refusal.
        if total_pages == 0
            && streams_in_call == 0
            && !retried_file_fallback
            && taken.tymed == super::driver::TYMED_CALLBACK
            && error::is_medium_refusal(effective_hr)
        {
            retried_file_fallback = true;
            if settings::fallback_callback_to_file(tree, &mut taken)? {
                continue;
            }
        }

        // `Source::Flatbed` produces strictly one page per batch (`Done::OnePage`).
        if !is_feeder {
            if total_pages >= 1
                && (effective_hr >= 0 || (effective_hr == S_FALSE_HR && stopped_flatbed_extra))
            {
                return Ok(Done::OnePage);
            }
            if effective_hr < 0 {
                return Err(error::hresult(WiaOp::Download, effective_hr));
            }
            return Err(error::driver_msg(
                "the scanner completed WIA flatbed transfer without delivering a page",
            ));
        }

        // `Source::Feeder`: restore progress if an empty stream was opened right before end-of-media.
        if stream_opened_since_last_page {
            if let Some(prev) = last_completed_page_bytes {
                progress.set_total(Some(prev), Unit::Bytes);
                progress.set(prev);
            }
            stream_opened_since_last_page = false;
        }

        let is_feeder_done = effective_hr == WIA_STATUS_END_OF_MEDIA_HR
            || error::is_paper_empty(effective_hr)
            || (effective_hr == S_OK_HR && streams_in_call == 0);
        if is_feeder_done {
            if total_pages == 0 {
                return Err(ScanError::FeederEmpty);
            }
            if !tracker.mid_sheet() && progress.check().is_err() {
                stop_feeder_best_effort(tree, taken.item, taken.handling_item);
                return Ok(Done::Cancelled);
            }
            return Ok(Done::FeederEmpty);
        }

        if effective_hr < 0 {
            if matches!(effective_hr, ERROR_CANCELLED_HR | E_ABORT_HR)
                && !tracker.mid_sheet()
                && progress.check().is_err()
            {
                stop_feeder_best_effort(tree, taken.item, taken.handling_item);
                return Ok(Done::Cancelled);
            }
            return Err(error::hresult(WiaOp::Download, effective_hr));
        }

        // Non-negative completion (`S_OK` / `S_FALSE`) with at least one page in this call.
        warmup_retries = 0;

        let should_continue_wsd_loop = taken
            .pages_per_download
            .is_some_and(|capped_n| outcome.hr == S_OK_HR && streams_in_call == capped_n);
        if !should_continue_wsd_loop {
            if !tracker.mid_sheet() && progress.check().is_err() {
                stop_feeder_best_effort(tree, taken.item, taken.handling_item);
                return Ok(Done::Cancelled);
            }
            return Ok(Done::FeederEmpty);
        }
    }
}

/// Combines `Download`'s return `HRESULT` with any latched negative `WiaTransferParams::hrErrorStatus`
/// so a generic `E_FAIL` or `S_OK` from `Download` never hides a specific `Jammed`, `CoverOpen`,
/// `DoubleFeed`, or `WIA_ERROR_PAPER_EMPTY` reported to the callback.
fn resolve_effective_hr(download_hr: i32, callback_err: Option<i32>) -> i32 {
    let Some(cb_hr) = callback_err else {
        return download_hr;
    };
    let cb_is_generic = matches!(cb_hr, E_FAIL_HR | WIA_ERROR_GENERAL_ERROR_HR);
    if !cb_is_generic {
        return cb_hr;
    }
    if download_hr < 0 { download_hr } else { cb_hr }
}

fn stop_feeder_best_effort(tree: &mut dyn WiaDeviceTree, item: ItemId, handling_item: ItemId) {
    if tree.device_command(item, WIA_CMD_STOP_FEEDER_GUID).is_err() && handling_item != item {
        let _ = tree.device_command(handling_item, WIA_CMD_STOP_FEEDER_GUID);
    }
}

struct BatchSink<'a> {
    events: &'a SyncSender<Event>,
    progress: &'a dyn Progress,
    tracker: &'a mut SheetTracker,
    taken: &'a Taken,
    total_pages: &'a mut u64,
    emitted_warming_up: &'a mut bool,
    last_completed_page_bytes: &'a mut Option<u64>,
    stream_opened_since_last_page: &'a mut bool,
    streams_in_call: u32,
    current_page_bytes: u64,
    cancelled_by_progress: bool,
    receiver_dropped: bool,
    stopped_flatbed_extra: bool,
    fatal_err: Option<ScanError>,
}

impl TransferSink for BatchSink<'_> {
    fn on_next_stream(&mut self) -> TransferAction {
        if self.taken.applied.settings.source == Source::Flatbed && *self.total_pages >= 1 {
            self.stopped_flatbed_extra = true;
            return TransferAction::Cancel;
        }

        if !self.tracker.mid_sheet() {
            if self.progress.check().is_err() {
                self.cancelled_by_progress = true;
                return TransferAction::Cancel;
            }
            if let Err(err) = self.tracker.ensure_can_start_page() {
                self.fatal_err = Some(err);
                return TransferAction::Cancel;
            }
        }

        self.current_page_bytes = 0;
        *self.stream_opened_since_last_page = true;
        self.progress.set_total(None, Unit::Bytes);
        self.progress.set(0);
        TransferAction::Continue
    }

    fn on_status(&mut self, _msg: i32, transferred_bytes: u64, hr_status: i32) -> TransferAction {
        if error::is_warming_up(hr_status) && !*self.emitted_warming_up {
            if self.events.send(Event::Status(Status::WarmingUp)).is_err() {
                self.receiver_dropped = true;
                return TransferAction::Cancel;
            }
            *self.emitted_warming_up = true;
        }

        if *self.stream_opened_since_last_page && transferred_bytes >= self.current_page_bytes {
            self.current_page_bytes = transferred_bytes;
            self.progress.set(self.current_page_bytes);
        }
        TransferAction::Continue
    }

    fn on_stream_complete(&mut self, bytes: Vec<u8>) -> TransferAction {
        let final_len = (bytes.len() as u64).max(self.current_page_bytes);
        self.progress.set_total(Some(final_len), Unit::Bytes);
        self.progress.set(final_len);
        *self.last_completed_page_bytes = Some(final_len);
        *self.stream_opened_since_last_page = false;

        let (pixels, page_dpi) =
            match pixels::decode_stream(bytes, self.taken.transfer_mode, self.taken.dpi) {
                Ok(pair) => pair,
                Err(err) => {
                    self.fatal_err = Some(err);
                    return TransferAction::Cancel;
                }
            };

        let (emit_sheet, sheet_num, side) = match self.tracker.accept_page() {
            Ok(triple) => triple,
            Err(err) => {
                self.fatal_err = Some(err);
                return TransferAction::Cancel;
            }
        };

        if let Some(s) = emit_sheet
            && self.events.send(Event::Sheet(s)).is_err()
        {
            self.receiver_dropped = true;
            return TransferAction::Cancel;
        }

        let page = RawPage::new(pixels, page_dpi, sheet_num, side, SideKnown::Inferred);
        if self.events.send(Event::Page(page)).is_err() {
            self.receiver_dropped = true;
            return TransferAction::Cancel;
        }

        *self.total_pages += 1;
        self.streams_in_call = self.streams_in_call.saturating_add(1);
        TransferAction::Continue
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sheet_tracker_pairs_front_first_and_back_first_and_rejects_u32_overflow() {
        let mut simplex = SheetTracker::new(10, false, false);
        assert!(!simplex.mid_sheet());
        assert_eq!(simplex.accept_page().unwrap(), (Some(10), 10, Side::Front));
        assert_eq!(simplex.accept_page().unwrap(), (Some(11), 11, Side::Front));

        let mut duplex = SheetTracker::new(1, true, false);
        assert_eq!(duplex.accept_page().unwrap(), (Some(1), 1, Side::Front));
        assert!(duplex.mid_sheet());
        assert_eq!(duplex.accept_page().unwrap(), (None, 1, Side::Back));
        assert!(!duplex.mid_sheet());
        assert_eq!(duplex.accept_page().unwrap(), (Some(2), 2, Side::Front));

        let mut back_first = SheetTracker::new(5, true, true);
        assert_eq!(back_first.accept_page().unwrap(), (Some(5), 5, Side::Back));
        assert!(back_first.mid_sheet());
        assert_eq!(back_first.accept_page().unwrap(), (None, 5, Side::Front));
        assert!(!back_first.mid_sheet());

        let mut at_max = SheetTracker::new(u32::MAX, true, false);
        assert!(at_max.ensure_can_start_page().is_ok());
        assert_eq!(
            at_max.accept_page().unwrap(),
            (Some(u32::MAX), u32::MAX, Side::Front)
        );
        // Second side of `u32::MAX` still completes cleanly!
        assert_eq!(at_max.accept_page().unwrap(), (None, u32::MAX, Side::Back));
        // Starting sheet `u32::MAX + 1` is refused before feeding.
        assert!(at_max.ensure_can_start_page().is_err());
    }
}
