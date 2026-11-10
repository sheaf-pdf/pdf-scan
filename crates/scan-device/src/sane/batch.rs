//! A batch: the settings set and said, then sheet after sheet, each page started, read whole,
//! put in the raw layout and sent, until the feeder is empty, a flatbed's one page is in, the
//! batch is cancelled, or an error ends it.
//!
//! SANE says a sheet is there only by giving its page: a start answers `NO_DOCS` once the feeder
//! is empty, not before. So a sheet's [`Event::Sheet`] comes once its front is read, with that
//! page, and the back's page once it's read; and every `send` is made between pages, never from
//! within a read, so a backend held by a full channel holds there, with no sheet started, and a
//! scanner is never held mid-sheet.

use std::sync::mpsc::SyncSender;

use in_progress::{Progress, Unit};
use sane_dl::{Error, Handle, Image, Page};

use super::{error, pixels, settings};
use crate::{Done, Event, RawPage, ScanError, Settings, Side, SideKnown, Source};

/// What a batch needs of its device.
pub(super) struct Device<'a> {
    /// Its handle.
    pub handle: &'a mut Handle,
    /// The options set by name, after the settings, at each batch.
    pub by_name: &'a [(String, String)],
    /// The most bytes a frame may have, which a 1-bit color page's expansion is capped at too.
    pub frame_cap: usize,
}

/// A batch of `asked`, its sheets numbered on from `first_sheet`, as the module's docs and the
/// model's "A batch" say.
///
/// - The settings set and read back, and said in [`Event::Applied`] before any sheet.
/// - Before each sheet's front, never within a sheet, `progress` checked: once it fails, the
///   batch is dropped (`sane_cancel`) and ends [`Done::Cancelled`], the sheet in flight finished,
///   front and back. Then the sheet numbered: past `u32::MAX`, [`ScanError::Driver`] before it
///   starts.
/// - A flatbed gives one page, then the batch is dropped: [`Done::OnePage`] (some flatbeds give
///   the same page for ever).
/// - `NO_DOCS` before a page: from a feeder with no page sent yet in this batch,
///   [`ScanError::FeederEmpty`]; after pages, or at a sheet's back, [`Done::FeederEmpty`], the
///   progress holding the last page's bytes again; from a flatbed, [`ScanError::Driver`], since
///   the model's empty feeder is a feeder's.
/// - A start the device refuses with the settings (`INVAL`) at a sheet's front: the batch
///   dropped, the settings set again as `Applied` gave them, and the page started once more in a
///   new batch, its sheet's number the same; refused again, or a setting that won't go back, is
///   [`ScanError::SettingsRefused`]. At a sheet's back, never tried again, since dropping the
///   batch mid-sheet may drop the back: [`ScanError::Driver`], the front kept.
/// - SANE's own `CANCELLED` (the scanner's button) ends it [`Done::Cancelled`]; every other error
///   ends it as `error::batch_end` maps it, the pages sent before kept and the page being read
///   dropped.
/// - A `send` that fails, the receiver gone, drops the batch: [`Done::Cancelled`].
pub(super) fn scan(
    device: Device<'_>,
    asked: &Settings,
    first_sheet: u32,
    events: &SyncSender<Event>,
    progress: &dyn Progress,
) -> Result<Done, ScanError> {
    let Device {
        handle,
        by_name,
        frame_cap,
    } = device;
    // The model's cancel is the progress's, checked between sheets; a cancel the token kept from
    // before stops nothing here.
    handle.clear_cancel();
    let taken = settings::apply(handle, asked, by_name)?;
    let settings = taken.applied.settings.clone();
    if events.send(Event::Applied(taken.applied.clone())).is_err() {
        return Ok(Done::Cancelled);
    }
    let sides: &[Side] = if settings.duplex {
        &[Side::Front, Side::Back]
    } else {
        &[Side::Front]
    };
    // The pages sent in this batch, the model's: a start met by `INVAL` goes on in a new
    // `sane-dl` batch, whose count starts again.
    let mut sent: u64 = 0;
    // The last page's bytes, which the progress holds again once the feeder is found empty.
    let mut last_bytes = 0;
    let mut sheets: u64 = 0;
    let mut batch = handle.batch();
    loop {
        // Between sheets, never within one: dropping the batch cancels SANE's. Before the sheet
        // is numbered, so a cancel is a cancel whatever the number.
        if progress.check().is_err() {
            return Ok(Done::Cancelled);
        }
        let Ok(sheet) = u32::try_from(u64::from(first_sheet) + sheets) else {
            return Err(ScanError::Driver {
                message: format!(
                    "no sheet is numbered past {}: the sheet after it stays in the feeder",
                    u32::MAX
                ),
                shown: false,
            });
        };
        for &side in sides {
            // A start refused with the settings is met once, at a sheet's front: the settings
            // set again, in a batch of their own, and the page started once more. Never at a
            // back, since a new batch cancels SANE's mid-sheet, which a scanner reading both
            // sides at once ends by dropping the back: the next page would be the next sheet's
            // front.
            let mut retried = side == Side::Back;
            let page = loop {
                let (page, bytes) = read_page(&mut batch, progress);
                if matches!(page, Ok(Page::Image(_))) {
                    last_bytes = bytes;
                }
                match page {
                    Err(Error::StartInvalid { .. }) if !retried => {
                        retried = true;
                        drop(batch);
                        settings::restore(handle, &taken, by_name)?;
                        batch = handle.batch();
                    }
                    other => break other,
                }
            };
            let image = match page {
                Ok(Page::Image(image)) => image,
                Ok(Page::NoDocs { .. }) => {
                    if sent > 0 {
                        progress.set_total(Some(last_bytes), Unit::Bytes);
                        progress.set(last_bytes);
                    }
                    return match (settings.source, side) {
                        (Source::Flatbed, _) => Err(ScanError::Driver {
                            message: "the scanner says there's no document".to_owned(),
                            shown: false,
                        }),
                        (Source::Feeder, Side::Front) if sent == 0 => Err(ScanError::FeederEmpty),
                        (Source::Feeder, _) => Ok(Done::FeederEmpty),
                    };
                }
                // The front kept: the back refused with the settings the front took.
                Err(Error::StartInvalid { .. }) if side == Side::Back => {
                    return Err(ScanError::Driver {
                        message: format!(
                            "the scanner refused to start sheet {sheet}'s back with the settings \
                             its front was scanned with"
                        ),
                        shown: false,
                    });
                }
                Err(error) => return error::batch_end(&error),
            };
            let page = raw_page(image, taken.dpi, sheet, side, frame_cap)?;
            sent += 1;
            if side == Side::Front && events.send(Event::Sheet(sheet)).is_err() {
                return Ok(Done::Cancelled);
            }
            if events.send(Event::Page(page)).is_err() {
                return Ok(Done::Cancelled);
            }
        }
        if settings.source == Source::Flatbed {
            return Ok(Done::OnePage);
        }
        sheets += 1;
    }
}

/// The batch's next page, its bytes counted on `progress`: its total unknown and its count 0 as
/// it's asked for, then SANE's bytes, padding included, as they come, and its total once known;
/// a page whose height was never known is given its total once it ends. With the bytes read.
fn read_page(
    batch: &mut sane_dl::Batch<'_>,
    progress: &dyn Progress,
) -> (Result<Page, Error>, u64) {
    progress.set_total(None, Unit::Bytes);
    progress.set(0);
    let mut total = None;
    let mut read = 0;
    let page = batch.next_page(|now| {
        if now.bytes_expected != total {
            total = now.bytes_expected;
            progress.set_total(total, Unit::Bytes);
        }
        read = now.bytes_read;
        progress.set(read);
    });
    if matches!(page, Ok(Page::Image(_))) && total != Some(read) {
        progress.set_total(Some(read), Unit::Bytes);
    }
    (page, read)
}

/// `image` as the model's page of `side` of `sheet`, at `dpi` (as SANE gave the resolution), its
/// side worked out from the order, since SANE says nothing of it.
fn raw_page(
    image: Image,
    dpi: (f32, f32),
    sheet: u32,
    side: Side,
    frame_cap: usize,
) -> Result<RawPage, ScanError> {
    // `sane-dl` gives 16-bit samples in the host's order.
    let raw = pixels::raw(image, cfg!(target_endian = "big"), frame_cap)?;
    Ok(RawPage::new(
        crate::Pixels::Raw(raw),
        dpi,
        sheet,
        side,
        SideKnown::Inferred,
    ))
}
