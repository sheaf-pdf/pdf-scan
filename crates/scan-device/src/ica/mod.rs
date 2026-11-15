//! The ImageCaptureCore backend: scanners on macOS, through Apple's ImageCaptureCore framework,
//! which is part of the system.
//!
//! **Not yet wired to the framework.** In this version [`IcaBackend::new`]'s listing and opening
//! report [`ScanError::NotInstalled`] on every system, macOS included, once an id it doesn't form
//! has been refused as [`ScanError::BadId`]; off macOS, where there's no ImageCaptureCore, that
//! stays its answer. Everything below the Objective-C side is built, and tested on every system
//! over a scripted ImageCaptureCore (behind the `fake` feature, hidden): the listing and the open,
//! what a source can do, the settings, the batch and its pages, the errors.
//!
//! # Devices and their ids
//!
//! A listing browses for scanners attached to the Mac, found on the network by Bonjour, or
//! shared. ImageCaptureCore's listing is a stream with no end, so the backend takes a snapshot: 2
//! s after the browse says it has found every device attached to the Mac, so network scanners
//! found by then are in it, or at the deadline where that comes first, with what's been found. A
//! deadline before the attached devices are all found is [`ScanError::TimedOut`]: the listing
//! isn't done.
//!
//! A device's [`DeviceId`](crate::DeviceId) is `ica:` and the framework's persistent id for it
//! (`ICDevice`'s `persistentIDString`), or its `UUIDString` where it gives none; a device with
//! neither isn't listed, and of two with one id the first is, but one attached to the Mac before
//! one on the network. Only scanners are listed: devices whose type says so, and which are
//! `ICScannerDevice`s. A [`DeviceInfo`]'s name is the framework's (`name`, capped at 1 KiB), or
//! the native id where it gives none; its maker and model are empty, since the framework gives
//! neither (its `productKind` is a device's class, such as "Scanner", and a network scanner has no
//! USB vendor or product id). Which of the two ids survives a replugging, a restart or a network
//! scanner's new address is unverified on a device.
//!
//! [`open`](crate::Backend::open) refuses as [`ScanError::BadId`], before the framework is
//! reached (and, where it isn't there, before [`ScanError::NotInstalled`]): another
//! [`Driver`](crate::Driver)'s device, an id without `ica:`, and a native id that's empty, holds a
//! `NUL` or is past 4,096 bytes. Then it browses until it finds the stored id, by the listing's
//! rule: where the listing's snapshot would be taken without it, it's [`ScanError::Gone`], in
//! seconds rather than at the deadline; where the deadline comes first, [`ScanError::TimedOut`]
//! if the attached devices weren't all found by then, [`ScanError::Gone`] if they were. The id
//! alone is matched, there being no maker or model to check, and
//! [`Opened::renamed_from`](crate::Opened::renamed_from) is never said: the persistent id is the
//! device's identity. Then its session is opened and the device waited for until it's ready,
//! within the same deadline; past it, [`ScanError::TimedOut`], the session closed. The browse is
//! kept while the device is open, since the browser keeps its devices alive, and stopped as the
//! device is dropped, after its session is closed.
//!
//! # What a source can do
//!
//! A scanner's functional units (`ICScannerFunctionalUnit`) are its sources: its flatbed and its
//! document feeder, the transparency units having no source in the model. Each is read by
//! selecting it in turn (the one selected already read as it is), and the one first selected is
//! selected again after. A select's answer is matched by the unit type asked for, as scanline's
//! is: the framework selects a unit unasked as the device is instantiated, so an answer of
//! another type (that one, or a late answer of a select given up on) is never read as the answer.
//! An answer with no unit and no error, which the framework's header says never comes but
//! scanline saw, is resolved by reading the selected unit, taken where it's of the type asked. A
//! select unanswered for 30 s is [`ScanError::Driver`].
//!
//! The framework lists a unit's bit depths but no pixel types, so its formats are those its
//! depths allow: at 1 bit black and white, at 8 and 16 bits gray and color. Its resolutions are
//! its `supportedResolutions`, one contiguous run a range stepping by 1 and anything else a list,
//! or its current resolution where it lists none. A flatbed's largest area is its `physicalSize`,
//! converted to millimetres from the unit's own measurement unit (inches, centimetres, picas,
//! points, twips, or pixels at its resolution). A feeder's paper is set by document type rather
//! than by size, so its largest area is its largest type's: the backend states sizes for A3, A4,
//! A5, A6, US Letter, Legal, Ledger, Executive and Statement, and leaves out the B sizes (the
//! framework's documentation doesn't show which B its types mean) and the business card. A
//! feeder's duplex is its `supportsDuplexScanning`. Nothing in the framework's API says a unit
//! finds the paper's size itself, so no source does ([`Paper::Auto`](crate::Paper::Auto) is the
//! largest area); no source gives encoded pages (memory-based transfers alone); and there's no
//! settings window ([`Device::settings_window`](crate::Device::settings_window) answers
//! [`ScanError::Unsupported`]).
//!
//! # Settings
//!
//! A batch sets, on its source's unit (selected first where it isn't), in order: a feeder's
//! duplex; the pixel type and the bit depth of the format asked, else of its sibling at the other
//! depth, else of the first the unit's depths allow; the resolution listed nearest the one asked,
//! the higher of two as near; the paper (a flatbed's scan area from its top left corner, cut to
//! its bed, in its own measurement unit; a feeder's document type, the smallest it supports
//! holding the size asked, else its largest, never a custom size); and a feeder's odd and even
//! pages' orientation as read, so a back comes as the scanner reads it, for the app to turn. The
//! unit is then read back whole, and [`Event::Applied`](crate::Event::Applied) says what it holds,
//! each difference from what was asked a [`Clamped`](crate::Clamped). Unverified on a device:
//! whether a module applies a setting before it's read back.
//!
//! # A batch
//!
//! A batch scans by memory-based transfer, in bands of at most 1 MiB. ImageCaptureCore says a
//! sheet is there only by sending its page, and nothing names a side, so a sheet's
//! [`Event::Sheet`](crate::Event::Sheet) comes once its front is whole, with that page, and in
//! duplex the next page is its back, each side inferred from the order. A feeder whose module
//! reads its stack last sheet first (`reverseFeederPageOrder`) sends its sheets in that order, and
//! the backend numbers them as they come: the model has nowhere to say it. A scanner warming up
//! is said as [`Status::WarmingUp`](crate::Status::WarmingUp), between pages.
//!
//! **Cancel:** the progress is checked after each sheet is whole, never within one; once it
//! fails, the scan is cancelled, the pages completing after it dropped, and the batch ends
//! [`Done::Cancelled`](crate::Done::Cancelled). ImageCaptureCore's cancel stops the whole scan,
//! with no "feed no more", and a feeder may have begun the next sheet by then: what becomes of it
//! (left in the feeder, ejected unscanned, jammed) is unverified on a device. So the model's "no
//! other sheet is fed" is this backend's aim, not its promise.
//!
//! **A full channel** holds the backend between pages, as with every backend; but
//! ImageCaptureCore sends its bands without waiting for the backend to take them, so while the
//! backend is held, as far as the framework's documentation says, the scanner isn't: the bands it
//! sends meanwhile wait to be taken. Whether a module stops feeding then, or a stack's pages wait
//! in the process's memory, each within the page caps, is unverified on a device.
//!
//! A batch ends, with no error, from a feeder after pages as
//! [`Done::FeederEmpty`](crate::Done::FeederEmpty) (in duplex after a front too, the front kept)
//! and before any as [`ScanError::FeederEmpty`]; from a flatbed as
//! [`Done::OnePage`](crate::Done::OnePage). A page begun and not whole as the scan ends is
//! [`ScanError::Driver`]. The framework's cancel, whether the backend's or the scanner's own
//! button, ends it [`Done::Cancelled`](crate::Done::Cancelled). The pages sent before an error
//! are kept, the page in flight dropped.
//!
//! # Pages
//!
//! A page comes in bands (`ICScannerBandData`), assembled into the raw layout: every number in a
//! band's header is checked before a byte is read; the page's size against the caps every backend
//! that assembles pages holds (2^28 pixels, 512 MiB) before its buffer is allocated, once; its
//! bands in order, top down, each within the page; a band's buffer past 64 MiB refused. Each
//! row's padding is dropped and 16-bit samples put little-endian. Black and white, and gray and
//! interleaved red, green and blue at 8 or 16 bits a sample, are taken; anything else is refused
//! with the header's numbers. A page's resolution is the one read back. Unverified on a device:
//! black and white's 0 is taken as black, and gray's as the least intensity, as Core Graphics'
//! gray has it; the color profile a band names (`colorSyncProfilePath`) isn't carried.
//!
//! # Errors
//!
//! An `NSError` in ImageCaptureCore's domain is mapped by its code: a scanner in use, a session
//! open already or a device still being enumerated is [`ScanError::Busy`]; a device this process
//! may not open, or one asking for credentials (none are given), [`ScanError::AccessDenied`]; the
//! device's module exiting, its session closed suddenly or the device ejected suddenly,
//! [`ScanError::Gone`], as is the device removed or its session closed unasked. A feeder's scan
//! failing before any page while its module finds no paper in it is [`ScanError::FeederEmpty`].
//! Every other code, and any other domain, is [`ScanError::Driver`] in the framework's words, with
//! the code. The framework has no code for a jam, a double feed or an open cover, so those come as
//! [`ScanError::Driver`] too.
//!
//! # Sources
//!
//! The framework's facts here are from Apple's documentation
//! (<https://developer.apple.com/documentation/imagecapturecore>) and the SDK's headers as
//! `objc2-image-capture-core` gives them (<https://docs.rs/objc2-image-capture-core/0.3.2/>).
//! A session's order, and a select's answer matched by its unit's type, follow scanline's (MIT,
//! Copyright (c) 2025 by Scott J. Kleper, <https://github.com/klep/scanline>), in our own words.

use std::time::Duration;

use crate::{Backend, DeviceInfo, Opened, ScanError};

#[cfg_attr(not(any(test, feature = "fake")), allow(dead_code))]
mod batch;
#[cfg_attr(not(any(test, feature = "fake")), allow(dead_code))]
mod caps;
#[allow(dead_code)]
mod driver;
#[cfg_attr(not(any(test, feature = "fake")), allow(dead_code))]
mod error;
#[cfg_attr(not(any(test, feature = "fake")), allow(dead_code))]
mod id;
#[cfg_attr(not(any(test, feature = "fake")), allow(dead_code))]
mod pixels;
#[cfg_attr(not(any(test, feature = "fake")), allow(dead_code))]
mod session;
#[cfg_attr(not(any(test, feature = "fake")), allow(dead_code))]
mod settings;

/// The scripted ImageCaptureCore, for this crate's integration tests and the test worker:
/// behind `fake`, never for a shipped build.
#[cfg(feature = "fake")]
#[doc(hidden)]
#[allow(missing_docs)]
pub mod fake_ica {
    pub use super::driver::fake_ica::{
        FakeBands, FakeCall, FakeFault, FakeIcaDevice, FakeIcaHandle, FakeSelect, FakeSession,
        FakeSheet, Unasked, WriteRule, feeder, flatbed, ica_error, network_props, scanner_props,
    };
    pub use super::driver::{
        BAND_BYTES, BandHeader, Bounds, BrowseRef, DEPTH_1, DEPTH_8, DEPTH_16, DEVICE_CAMERA,
        DEVICE_SCANNER, DOCUMENT_A3, DOCUMENT_A4, DOCUMENT_A5, DOCUMENT_A6, DOCUMENT_DEFAULT,
        DOCUMENT_US_EXECUTIVE, DOCUMENT_US_LEDGER, DOCUMENT_US_LEGAL, DOCUMENT_US_LETTER,
        DOCUMENT_US_STATEMENT, DeviceProps, DeviceRef, IcaError, IndexSet, LOCATION_BONJOUR,
        LOCATION_LOCAL, LOCATION_SHARED, MAX_INDEXES, MEASURE_CENTIMETERS, MEASURE_INCHES,
        MEASURE_PIXELS, ORIENTATION_AS_READ, PIXEL_BW, PIXEL_GRAY, PIXEL_RGB,
        RETURN_CONNECTION_DRIVER_EXITED, RETURN_CONNECTION_NOT_AUTHORIZED_TO_OPEN_DEVICE,
        RETURN_SCAN_OPERATION_CANCELED, RETURN_SCANNER_FAILED_TO_COMPLETE_SCAN,
        RETURN_SCANNER_IN_USE_BY_LOCAL_USER, UNIT_DOCUMENT_FEEDER, UNIT_FLATBED,
        UNIT_POSITIVE_TRANSPARENCY, UnitProps, UnitRect, UnitSize, UnitWrite,
    };
}

/// The ImageCaptureCore scanner backend (`ica:` ids).
///
/// Not yet wired to the framework: [`IcaBackend::new`]'s listing and opening report
/// [`ScanError::NotInstalled`] on every system for now, after refusing an id it doesn't form; see
/// [the module documentation](self).
#[derive(Default)]
pub struct IcaBackend {
    /// The scripted framework this backend runs over, where it was made from one.
    #[cfg(feature = "fake")]
    fake: Option<driver::fake_ica::FakeIcaHandle>,
    /// The system its listings browse on, made at the first and kept, so a late event of one
    /// listing meets the next and is dropped there.
    #[cfg(feature = "fake")]
    listing: Option<Box<dyn driver::IcaSystem>>,
}

impl std::fmt::Debug for IcaBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IcaBackend").finish_non_exhaustive()
    }
}

impl IcaBackend {
    /// An `IcaBackend`. It reaches nothing as it's made.
    pub fn new() -> IcaBackend {
        IcaBackend::default()
    }

    /// A backend over the scripted framework `fake`, for tests.
    #[cfg(feature = "fake")]
    #[doc(hidden)]
    pub fn from_fake(fake: &fake_ica::FakeIcaHandle) -> IcaBackend {
        IcaBackend {
            fake: Some(fake.clone()),
            listing: None,
        }
    }
}

fn not_installed() -> ScanError {
    ScanError::NotInstalled {
        what: error::ICA.to_owned(),
    }
}

impl Backend for IcaBackend {
    fn list(&mut self, deadline: Duration) -> Result<Vec<DeviceInfo>, ScanError> {
        #[cfg(feature = "fake")]
        if let Some(fake) = &self.fake {
            let system = self.listing.get_or_insert_with(|| Box::new(fake.system()));
            return session::list(system.as_mut(), deadline);
        }
        let _ = deadline;
        Err(not_installed())
    }

    fn open(&mut self, stored: &DeviceInfo, deadline: Duration) -> Result<Opened, ScanError> {
        let _ = id::native_id(stored)?;
        #[cfg(feature = "fake")]
        if let Some(fake) = &self.fake {
            // A system of the device's own, which it keeps.
            return session::open(Box::new(fake.system()), stored, deadline);
        }
        let _ = deadline;
        Err(not_installed())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DeviceId, Driver};

    fn assert_send<T: Send>() {}

    fn device(id: &str, driver: Driver) -> DeviceInfo {
        DeviceInfo::new(
            DeviceId(id.to_owned()),
            "Desk scanner".to_owned(),
            String::new(),
            String::new(),
            driver,
        )
    }

    #[test]
    fn a_bad_id_is_refused_before_not_installed() {
        assert_send::<IcaBackend>();
        let mut backend = IcaBackend::new();
        let one_past = format!("ica:{}", "a".repeat(4097));
        for bad in [
            device("ica:0x1234", Driver::Sane),
            device("sane:test:0", Driver::Ica),
            device("ica:", Driver::Ica),
            device("ica:0x12\u{0}34", Driver::Ica),
            device(&one_past, Driver::Ica),
        ] {
            assert_eq!(
                backend.open(&bad, Duration::from_secs(1)).err(),
                Some(ScanError::BadId),
                "{:?}",
                bad.id
            );
        }
    }

    #[test]
    fn listing_and_opening_say_imagecapturecore_is_not_installed() {
        let mut backend = IcaBackend::default();
        let not_installed = ScanError::NotInstalled {
            what: "ImageCaptureCore".to_owned(),
        };
        assert_eq!(
            backend.list(Duration::from_secs(1)).err(),
            Some(not_installed.clone())
        );
        assert_eq!(
            backend
                .open(&device("ica:0x1234", Driver::Ica), Duration::from_secs(1))
                .err(),
            Some(not_installed)
        );
        assert!(format!("{backend:?}").starts_with("IcaBackend"));
    }
}
