//! A scripted device's batch: the settings clamped, sheets taken and their pages sent, and the
//! faults a sheet's script holds fired.

use std::{process, sync::mpsc::SyncSender, thread, time::Duration};

use in_progress::{Progress, Unit};

use super::{FakeDevice, Fault, Sheet, When, clamp, pattern};
use crate::{
    Done, Encoded, EncodedKind, Encoding, Event, Format, Paper, Pixels, RawImage, RawPage,
    ScanError, Settings, Side, SizeMm, Source, Status,
};

/// The most bytes the fake draws a raw page in: past it, a page is a mistake in a test's
/// settings or caps, refused before anything is allocated.
const MOST_PAGE_BYTES: usize = 512 * 1024 * 1024;

/// How often a side waiting on [`Fault::AwaitCancel`] checks the progress.
const AWAIT_CANCEL_POLL: Duration = Duration::from_millis(1);

/// `device`'s batch, as the module's docs say ("A batch").
pub(super) fn scan(
    device: &FakeDevice,
    settings: &Settings,
    first_sheet: u32,
    events: &SyncSender<Event>,
    progress: &dyn Progress,
) -> Result<Done, ScanError> {
    let (applied, source) = clamp::clamp(&device.caps, settings);
    let batch = Batch {
        device,
        settings: applied.settings.clone(),
        largest: source.max_area,
        events,
        progress,
    };
    if events.send(Event::Applied(applied)).is_err() {
        return Ok(Done::Cancelled);
    }
    if device.warms_up && events.send(Event::Status(Status::WarmingUp)).is_err() {
        return Ok(Done::Cancelled);
    }
    match batch.settings.source {
        Source::Flatbed => batch.flatbed(first_sheet),
        Source::Feeder => batch.feeder(first_sheet),
    }
}

/// A batch under way, its settings as applied.
struct Batch<'a> {
    device: &'a FakeDevice,
    settings: Settings,
    /// The source's largest paper, which a sheet's own size is cut to under `Paper::Auto`.
    largest: Option<SizeMm>,
    events: &'a SyncSender<Event>,
    progress: &'a dyn Progress,
}

/// Whether a sheet's pages were all sent, or the receiver had gone.
enum Sent {
    All,
    ReceiverGone,
}

impl Batch<'_> {
    /// The glass's one sheet, scanned where it lies.
    fn flatbed(&self, sheet: u32) -> Result<Done, ScanError> {
        if self.progress.check().is_err() {
            return Ok(Done::Cancelled);
        }
        let (mut glass, loaded) = {
            let paper = self.device.paper();
            (paper.glass.clone(), paper.glass_loads)
        };
        if self.events.send(Event::Sheet(sheet)).is_err() {
            return Ok(Done::Cancelled);
        }
        let sent = self.sides(&mut glass, sheet);
        // The sheet left on the glass as it is now, without the faults that fired, unless
        // another was put there meanwhile.
        {
            let mut paper = self.device.paper();
            if paper.glass_loads == loaded {
                paper.glass = glass;
            }
        }
        match sent? {
            Sent::All => Ok(Done::OnePage),
            Sent::ReceiverGone => Ok(Done::Cancelled),
        }
    }

    /// Sheet after sheet from the feeder, until it's empty.
    fn feeder(&self, first_sheet: u32) -> Result<Done, ScanError> {
        let mut taken: u64 = 0;
        loop {
            // Between sheets, never within one.
            if self.progress.check().is_err() {
                return Ok(Done::Cancelled);
            }
            let (number, mut sheet) = {
                let mut paper = self.device.paper();
                if paper.feeder.is_empty() {
                    return if taken == 0 {
                        Err(ScanError::FeederEmpty)
                    } else {
                        Ok(Done::FeederEmpty)
                    };
                }
                // Counted before the sheet is taken, so one past the last number stays in the
                // feeder.
                let Ok(number) = u32::try_from(u64::from(first_sheet) + taken) else {
                    return Err(driver(format!(
                        "no sheet is numbered past {}: the sheet after it stays in the feeder",
                        u32::MAX
                    )));
                };
                let Some(sheet) = paper.feeder.pop_front() else {
                    return Ok(Done::FeederEmpty);
                };
                paper.fed = paper.fed.saturating_add(1);
                (number, sheet)
            };
            taken += 1;
            if self.events.send(Event::Sheet(number)).is_err() {
                return Ok(Done::Cancelled);
            }
            match self.sides(&mut sheet, number) {
                Ok(Sent::All) => {}
                Ok(Sent::ReceiverGone) => return Ok(Done::Cancelled),
                Err(error) => {
                    // Not scanned: the sheet goes back at the head of the feeder, without the
                    // fault that fired, if one did.
                    self.device.paper().feeder.push_front(sheet);
                    return Err(error);
                }
            }
        }
    }

    /// `sheet`'s pages, its front's and, in duplex, its back's, each sent once it's made; a
    /// fault scripted on a side read fired as it comes, and taken from the sheet.
    fn sides(&self, sheet: &mut Sheet, number: u32) -> Result<Sent, ScanError> {
        let sides: &[Side] = if self.settings.duplex {
            &[Side::Front, Side::Back]
        } else {
            &[Side::Front]
        };
        for &side in sides {
            if let Some(fault) = sheet.take_fault(side, When::Start) {
                self.fire(fault)?;
            }
            let page = self.page(sheet, number, side)?;
            if self.events.send(Event::Page(page)).is_err() {
                return Ok(Sent::ReceiverGone);
            }
        }
        Ok(Sent::All)
    }

    /// `side`'s page of `sheet`: the one given, or one drawn; its bytes counted on the progress,
    /// and a fault scripted midway through it fired there.
    fn page(&self, sheet: &mut Sheet, number: u32, side: Side) -> Result<RawPage, ScanError> {
        let pixels = match sheet.given(side) {
            Some(given) => given.clone(),
            None => self.draw(sheet, number, side)?,
        };
        let midway = sheet.take_fault(side, When::Midway);
        self.report(&pixels, midway)?;
        // At least 1 dpi: the caps hold no resolution of 0.
        let dpi = self.settings.dpi as f32;
        Ok(RawPage::new(
            pixels,
            (dpi, dpi),
            number,
            side,
            self.device.side_known,
        ))
    }

    /// A page drawn at the settings: encoded, in the first form asked that fits its format, or
    /// raw, by `pattern`.
    fn draw(&self, sheet: &Sheet, number: u32, side: Side) -> Result<Pixels, ScanError> {
        let format = self.settings.format;
        let (width, height) = self.size(sheet);
        page_bytes(format, width, height)?;
        let encoding = self
            .settings
            .encodings
            .iter()
            .copied()
            .find(|&encoding| fits(encoding, format));
        if let Some(encoding) = encoding {
            return Ok(Pixels::Encoded(stand_in(
                encoding, width, height, number, side,
            )));
        }
        pattern(format, width, height, number, side)
            .map(Pixels::Raw)
            .map_err(|error| driver(format!("the fake can't draw its page: {error}")))
    }

    /// A page's bytes on the progress: its total, then a row at a time (an encoded page's at
    /// once), with `midway` fired after the first half of its rows (an encoded page's before its
    /// bytes).
    fn report(&self, pixels: &Pixels, mut midway: Option<Fault>) -> Result<(), ScanError> {
        let progress = self.progress;
        match pixels {
            Pixels::Raw(image) => {
                progress.set_total(Some(image.data().len() as u64), Unit::Bytes);
                progress.set(0);
                let row = image.stride() as u64;
                let half = image.height() / 2;
                for at in 0..image.height() {
                    if at == half
                        && let Some(fault) = midway.take()
                    {
                        self.fire(fault)?;
                    }
                    progress.inc(row);
                }
            }
            Pixels::Encoded(encoded) => {
                let bytes = encoded.bytes.len() as u64;
                progress.set_total(Some(bytes), Unit::Bytes);
                progress.set(0);
                if let Some(fault) = midway {
                    self.fire(fault)?;
                }
                progress.inc(bytes);
            }
        }
        Ok(())
    }

    /// A scripted fault, fired: an error to end the batch with, the thread or the process ended,
    /// or a wait for the batch's cancel.
    fn fire(&self, fault: Fault) -> Result<(), ScanError> {
        match fault {
            Fault::Error(error) => {
                if error == ScanError::Gone {
                    self.device.paper().gone = true;
                }
                Err(error)
            }
            Fault::Hang => loop {
                // Never unparked; parked again if woken all the same.
                thread::park();
            },
            Fault::Crash => process::abort(),
            Fault::Exit(code) => process::exit(code),
            Fault::AwaitCancel => {
                // Nothing locked here: the sheet is the batch's own, out of the device's paper.
                while self.progress.check().is_ok() {
                    thread::sleep(AWAIT_CANCEL_POLL);
                }
                Ok(())
            }
        }
    }

    /// A drawn page's width and height in pixels: the paper's size, or under `Paper::Auto` the
    /// sheet's own, cut to the source's largest, at the resolution.
    fn size(&self, sheet: &Sheet) -> (u32, u32) {
        let size = match self.settings.paper {
            Paper::Size(size) => size,
            Paper::Auto => match self.largest {
                Some(largest) => SizeMm {
                    width: sheet.size.width.min(largest.width),
                    height: sheet.size.height.min(largest.height),
                },
                None => sheet.size,
            },
        };
        let dpi = self.settings.dpi;
        (pixels(size.width, dpi), pixels(size.height, dpi))
    }
}

/// The bytes of a raw page `width` by `height` in `format`, which the fake draws only up to
/// [`MOST_PAGE_BYTES`].
fn page_bytes(format: Format, width: u32, height: u32) -> Result<usize, ScanError> {
    let bytes = RawImage::data_len(format, width, height)
        .map_err(|error| driver(format!("the fake can't draw its page: {error}")))?;
    if bytes > MOST_PAGE_BYTES {
        return Err(driver(format!(
            "the fake draws no page past 512 MiB: {width} by {height} in {format:?} is {bytes} \
             bytes"
        )));
    }
    Ok(bytes)
}

/// `mm` at `dpi`, in whole pixels, rounded, and at least 1.
fn pixels(mm: f32, dpi: u32) -> u32 {
    let pixels = (f64::from(mm) / 25.4 * f64::from(dpi)).round();
    if pixels.is_nan() {
        return 1;
    }
    // In range, so the conversion is exact.
    pixels.clamp(1.0, f64::from(u32::MAX)) as u32
}

/// Whether a page in `format` can be encoded as `encoding`: CCITT G4 black and white only, JPEG
/// 8-bit gray or color only, the rest any.
fn fits(encoding: Encoding, format: Format) -> bool {
    match encoding {
        Encoding::CcittG4 => format == Format::Bw1,
        Encoding::Jpeg => matches!(format, Format::Gray8 | Format::Rgb8),
        Encoding::Png | Encoding::Tiff | Encoding::Bmp => true,
    }
}

/// An encoded page's stand-in: a few bytes naming the form, the sheet and the side.
fn stand_in(encoding: Encoding, width: u32, height: u32, sheet: u32, side: Side) -> Encoded {
    let (kind, name) = match encoding {
        Encoding::Jpeg => (EncodedKind::Jpeg, "JPEG"),
        Encoding::Png => (EncodedKind::Png, "PNG"),
        Encoding::Tiff => (EncodedKind::Tiff, "TIFF"),
        Encoding::Bmp => (EncodedKind::Bmp, "BMP"),
        Encoding::CcittG4 => (
            EncodedKind::CcittG4 {
                width,
                height,
                black_is_1: true,
            },
            "CCITT G4",
        ),
    };
    let side = match side {
        Side::Front => "front",
        Side::Back => "back",
    };
    Encoded {
        kind,
        bytes: format!("fake {name} page: sheet {sheet}, {side}").into_bytes(),
    }
}

/// A `Driver` error in the fake's words, never shown.
fn driver(message: String) -> ScanError {
    ScanError::Driver {
        message,
        shown: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_dimension_is_its_millimetres_at_the_dpi_rounded_and_at_least_1() {
        assert_eq!(pixels(SizeMm::LETTER.width, 300), 2550);
        assert_eq!(pixels(SizeMm::LETTER.height, 300), 3300);
        assert_eq!(pixels(25.4, 1), 1);
        assert_eq!(pixels(0.01, 1), 1);
        // 1.496 and 1.504 pixels.
        assert_eq!(pixels(38.0, 1), 1);
        assert_eq!(pixels(38.2, 1), 2);
        assert_eq!(pixels(f32::MAX, u32::MAX), u32::MAX);
        assert_eq!(pixels(f32::NAN, 300), 1);
    }

    #[test]
    fn a_page_of_512_mib_is_drawn_and_one_byte_more_isn_t() {
        assert_eq!(
            page_bytes(Format::Gray8, 16_384, 32_768),
            Ok(MOST_PAGE_BYTES)
        );
        assert_eq!(
            page_bytes(Format::Bw1, 16_384 * 8, 32_768),
            Ok(MOST_PAGE_BYTES)
        );
        let past = |format, width, height| match page_bytes(format, width, height) {
            Err(ScanError::Driver { message, shown }) => {
                assert!(!shown);
                message
            }
            other => panic!("{other:?}"),
        };
        assert_eq!(
            past(Format::Gray8, 16_385, 32_768),
            "the fake draws no page past 512 MiB: 16385 by 32768 in Gray8 is 536903680 bytes"
        );
        assert_eq!(
            past(Format::Rgb16, u32::MAX, u32::MAX),
            "the fake can't draw its page: 4294967295 by 4294967295 is more than this system can \
             hold"
        );
        assert_eq!(
            past(Format::Gray8, 0, 1),
            "the fake can't draw its page: an image has at least one row and one column"
        );
    }

    #[test]
    fn each_encoding_fits_the_formats_it_takes() {
        let formats = [
            Format::Bw1,
            Format::Gray8,
            Format::Gray16,
            Format::Rgb8,
            Format::Rgb16,
        ];
        let fitting = |encoding| {
            formats
                .into_iter()
                .filter(|&format| fits(encoding, format))
                .collect::<Vec<_>>()
        };
        assert_eq!(fitting(Encoding::CcittG4), [Format::Bw1]);
        assert_eq!(fitting(Encoding::Jpeg), [Format::Gray8, Format::Rgb8]);
        for any in [Encoding::Png, Encoding::Tiff, Encoding::Bmp] {
            assert_eq!(fitting(any), formats);
        }
    }

    #[test]
    fn a_stand_in_names_its_form_sheet_and_side() {
        let g4 = stand_in(Encoding::CcittG4, 7, 9, 3, Side::Back);
        assert_eq!(
            g4.kind,
            EncodedKind::CcittG4 {
                width: 7,
                height: 9,
                black_is_1: true
            }
        );
        assert_eq!(g4.bytes, b"fake CCITT G4 page: sheet 3, back");
        let jpeg = stand_in(Encoding::Jpeg, 7, 9, 12, Side::Front);
        assert_eq!(jpeg.kind, EncodedKind::Jpeg);
        assert_eq!(jpeg.bytes, b"fake JPEG page: sheet 12, front");
    }
}
