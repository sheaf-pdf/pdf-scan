//! A batch: the settings set and said, a memory-based scan asked for, its bands assembled into
//! pages and each page sent whole, until the scan ends, the batch is cancelled, or an error ends
//! it.
//!
//! ImageCaptureCore says a sheet is there only by sending its page, and nothing names a side, so
//! a sheet's [`Event::Sheet`] comes once its front is whole, with that page, then in duplex the
//! next page as its back, each side inferred from the order. Every `send` is made between the
//! framework's callbacks, in the batch's own loop, so a full channel holds the backend there.
//!
//! The progress is checked after each sheet is whole (its front in simplex, its back in duplex),
//! never within one: once it fails the scan is cancelled (`cancelScan`), the events taken until
//! its end, any page completing after the cancel dropped, and the batch ends
//! [`Done::Cancelled`]. ImageCaptureCore has no "feed no more": its cancel stops the whole scan,
//! and a feeder may have begun the next sheet by then, which is left as its module leaves it (in
//! the feeder, ejected unscanned, or jammed: unverified on a device). So the model's "no other
//! sheet is fed" is this backend's aim, not its promise.

use std::sync::mpsc::SyncSender;
use std::time::Instant;

use in_progress::{Progress, Unit};

use super::driver::{BandHeader, DeviceStatus, IcaError, IcaEvent, UNIT_DOCUMENT_FEEDER};
use super::error::{driver_msg, is_cancel, scan_error};
use super::pixels::{PageAssembler, check_band_len};
use super::session::IcaDevice;
use super::settings;
use crate::{Done, Event, Pixels, RawPage, ScanError, Settings, Side, SideKnown, Source, Status};

/// A batch of `asked`, its sheets numbered on from `first_sheet`, as the module's docs and the
/// model's "A batch" say.
///
/// - The settings set and read back, and said in [`Event::Applied`]; then `progress` checked, a
///   cancel then [`Done::Cancelled`] with nothing asked of the scanner; then the scan asked for.
/// - Each band checked and placed; a page whole once its rows reach its height, then sent: in a
///   feeder's duplex batch, pages alternate front and back. The progress counts each page's
///   bytes: its total and 0 at its first band, its bytes placed at each band.
/// - A warming-up status said as [`Status::WarmingUp`], between pages only.
/// - The end: no error, from a feeder with pages [`Done::FeederEmpty`] (in duplex after a front,
///   too, the front kept), with none [`ScanError::FeederEmpty`]; from a flatbed with its page
///   [`Done::OnePage`], with none [`ScanError::Driver`]; a page begun and not whole,
///   [`ScanError::Driver`], the pages before kept. The framework's cancel (the backend's or the
///   scanner's own button) [`Done::Cancelled`], the pages before kept. Another error as the
///   module's docs map it, but before any page from a feeder its module finds no paper in,
///   [`ScanError::FeederEmpty`].
/// - The device removed, [`ScanError::Gone`]; a band refused, a callback's panic or the device's
///   error, the scan cancelled and the error said; the pages before kept in each.
/// - A `send` that fails, the receiver gone, cancels the scan: [`Done::Cancelled`].
pub(super) fn scan(
    device: &mut IcaDevice,
    asked: &Settings,
    first_sheet: u32,
    events: &SyncSender<Event>,
    progress: &dyn Progress,
) -> Result<Done, ScanError> {
    let taken = settings::apply(device, asked)?;
    let settings = &taken.applied.settings;
    let mut batch = Batch {
        source: settings.source,
        duplex: settings.duplex,
        dpi: taken.dpi,
        first_sheet,
        events,
        progress,
        page: None,
        pages: 0,
        sheets: 0,
        sheet: first_sheet,
        warming_up: false,
    };
    if events.send(Event::Applied(taken.applied.clone())).is_err() {
        return Ok(Done::Cancelled);
    }
    if progress.check().is_err() {
        return Ok(Done::Cancelled);
    }
    device.request_scan()?;
    batch.run(device)
}

/// What a page completing leads to.
enum Next {
    /// The scan goes on.
    On,
    /// The batch ends so.
    End(Result<Done, ScanError>),
}

/// A batch under way.
struct Batch<'a> {
    source: Source,
    duplex: bool,
    dpi: f32,
    first_sheet: u32,
    events: &'a SyncSender<Event>,
    progress: &'a dyn Progress,
    /// The page whose bands are coming.
    page: Option<PageAssembler>,
    /// The pages sent.
    pages: u64,
    /// The sheets whose front has been sent.
    sheets: u64,
    /// The sheet the last front sent is of.
    sheet: u32,
    /// A warming-up status waiting for the page in flight to be sent.
    warming_up: bool,
}

impl Batch<'_> {
    fn run(&mut self, device: &mut IcaDevice) -> Result<Done, ScanError> {
        loop {
            // A batch's silence is for whoever runs the backend to end (a worker's watchdog);
            // the wait here only slices it.
            let event = match device.next(device.bounds.answer) {
                Ok(Some(event)) => event,
                Ok(None) => continue,
                Err(ScanError::Gone) => return Err(ScanError::Gone),
                Err(err) => return self.fail(device, err),
            };
            match event {
                IcaEvent::Band { header, bytes, .. } => {
                    if let Err(err) = self.band(&header, &bytes) {
                        return self.fail(device, err);
                    }
                    if self.page.as_ref().is_some_and(PageAssembler::is_complete)
                        && let Next::End(end) = self.page_done(device)
                    {
                        return end;
                    }
                }
                IcaEvent::OversizedBand { len, .. } => {
                    let err = check_band_len(len).err().unwrap_or_else(|| {
                        driver_msg("the scanner sent a band it called too large")
                    });
                    return self.fail(device, err);
                }
                IcaEvent::ScanCompleted { error, .. } => return self.end(device, error),
                IcaEvent::DeviceError { error, .. } => {
                    return self.fail(device, scan_error(&error));
                }
                IcaEvent::Status {
                    status: DeviceStatus::WarmingUp,
                    ..
                } => {
                    if self.page.is_some() {
                        self.warming_up = true;
                    } else if self.events.send(Event::Status(Status::WarmingUp)).is_err() {
                        return Ok(self.cancel(device));
                    }
                }
                _ => {}
            }
        }
    }

    /// `header`'s band placed in the page it's of, the page begun at its first band.
    fn band(&mut self, header: &BandHeader, bytes: &[u8]) -> Result<(), ScanError> {
        let page = match &mut self.page {
            Some(page) => page,
            None => {
                let page = PageAssembler::new(header)?;
                self.progress
                    .set_total(Some(page.len() as u64), Unit::Bytes);
                self.progress.set(0);
                self.page.insert(page)
            }
        };
        page.place(header, bytes)?;
        self.progress.set(page.placed_bytes() as u64);
        Ok(())
    }

    /// The whole page sent, with its sheet's event where it's a front; then, between sheets, the
    /// progress checked.
    fn page_done(&mut self, device: &mut IcaDevice) -> Next {
        let Some(page) = self.page.take() else {
            return Next::On;
        };
        let image = match page.finish() {
            Ok(image) => image,
            Err(err) => return Next::End(self.fail(device, err)),
        };
        let side = if self.duplex && self.pages % 2 == 1 {
            Side::Back
        } else {
            Side::Front
        };
        if side == Side::Front {
            let Ok(sheet) = u32::try_from(u64::from(self.first_sheet) + self.sheets) else {
                let err = driver_msg(format!(
                    "no sheet is numbered past {}: the scan is cancelled at the sheet after it",
                    u32::MAX
                ));
                return Next::End(self.fail(device, err));
            };
            self.sheet = sheet;
            self.sheets += 1;
            if self.events.send(Event::Sheet(sheet)).is_err() {
                return Next::End(Ok(self.cancel(device)));
            }
        }
        let page = RawPage::new(
            Pixels::Raw(image),
            (self.dpi, self.dpi),
            self.sheet,
            side,
            SideKnown::Inferred,
        );
        if self.events.send(Event::Page(page)).is_err() {
            return Next::End(Ok(self.cancel(device)));
        }
        self.pages += 1;
        if std::mem::take(&mut self.warming_up)
            && self.events.send(Event::Status(Status::WarmingUp)).is_err()
        {
            return Next::End(Ok(self.cancel(device)));
        }
        // Between sheets, never within one; a flatbed's one page ends its scan anyway.
        let sheet_whole = !self.duplex || side == Side::Back;
        if sheet_whole && self.source == Source::Feeder && self.progress.check().is_err() {
            return Next::End(Ok(self.cancel(device)));
        }
        Next::On
    }

    /// How the batch ends at the scan's end, `error` its error.
    fn end(&mut self, device: &mut IcaDevice, error: Option<IcaError>) -> Result<Done, ScanError> {
        let in_flight = self.page.take();
        match error {
            Some(error) if is_cancel(&error) => Ok(Done::Cancelled),
            Some(error) => {
                if self.pages == 0 && self.source == Source::Feeder && feeder_reads_empty(device) {
                    Err(ScanError::FeederEmpty)
                } else {
                    Err(scan_error(&error))
                }
            }
            None => {
                if let Some(page) = in_flight {
                    // Not whole, or it would have been sent: `finish` says how far it got.
                    page.finish()?;
                }
                match (self.source, self.pages) {
                    (Source::Feeder, 0) => Err(ScanError::FeederEmpty),
                    (Source::Feeder, _) => Ok(Done::FeederEmpty),
                    (Source::Flatbed, 0) => Err(driver_msg(
                        "the scanner ended the flatbed's scan without a page",
                    )),
                    (Source::Flatbed, _) => Ok(Done::OnePage),
                }
            }
        }
    }

    /// The scan cancelled, its events taken until its end: [`Done::Cancelled`].
    fn cancel(&mut self, device: &mut IcaDevice) -> Done {
        device.cancel_scan();
        drain(device);
        Done::Cancelled
    }

    /// The scan cancelled over `err`, its events taken until its end: `err`.
    fn fail(&mut self, device: &mut IcaDevice, err: ScanError) -> Result<Done, ScanError> {
        device.cancel_scan();
        drain(device);
        Err(err)
    }
}

/// The cancelled scan's events taken, and dropped, until its end, at most
/// [`Bounds::answer`](super::driver::Bounds::answer).
fn drain(device: &mut IcaDevice) {
    let bound = device.bounds.answer;
    let end = Instant::now().checked_add(bound);
    loop {
        let left = end.map_or(bound, |end| end.saturating_duration_since(Instant::now()));
        if left.is_zero() {
            return;
        }
        match device.next(left) {
            Ok(Some(IcaEvent::ScanCompleted { .. })) | Ok(None) | Err(_) => return,
            Ok(Some(_)) => {}
        }
    }
}

/// Whether the feeder's module finds no paper in it now (`documentLoaded` false), which may also
/// mean it can't tell: read only to name a scan failing before any page.
fn feeder_reads_empty(device: &mut IcaDevice) -> bool {
    matches!(
        device.selected_unit(),
        Ok(Some(unit)) if unit.unit_type == UNIT_DOCUMENT_FEEDER && !unit.document_loaded
    )
}
