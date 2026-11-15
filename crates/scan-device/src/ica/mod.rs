//! The ImageCaptureCore backend: scanners on macOS, through Apple's ImageCaptureCore framework,
//! which is part of the system.
//!
//! **Not yet wired to the framework.** In this version the backend's platform-independent parts
//! are built and tested on every system (its ids, what it reads of a scanner's functional units,
//! a page assembled from a memory-based transfer's bands, the framework's errors mapped), but
//! [`IcaBackend`] doesn't call ImageCaptureCore yet: listing and opening report
//! [`ScanError::NotInstalled`] on every system, macOS included, once an id it doesn't form has
//! been refused as [`ScanError::BadId`]. Off macOS, where there's no ImageCaptureCore, that stays
//! its answer.
//!
//! # Devices and their ids
//!
//! A device's [`DeviceId`](crate::DeviceId) is `ica:` and the framework's persistent id for it
//! (`ICDevice`'s `persistentIDString`), or its `UUIDString` where it gives none; a device with
//! neither isn't listed. Only scanners are listed: devices whose type says so, and which are
//! `ICScannerDevice`s. A [`DeviceInfo`]'s name is the framework's (`name`, capped at 1 KiB), or
//! the native id where it gives none; its maker and model are empty, since the framework gives
//! neither (its `productKind` is a device's class, such as "Scanner", and a network scanner has no
//! USB vendor or product id). Which of the two ids survives a replugging, a restart or a network
//! scanner's new address is unverified on a device.
//!
//! [`open`](crate::Backend::open) refuses as [`ScanError::BadId`], before the framework is
//! reached (and, where it isn't there, before [`ScanError::NotInstalled`]): another
//! [`Driver`](crate::Driver)'s device, an id without `ica:`, and a native id that's empty, holds a
//! `NUL` or is past 4,096 bytes.
//!
//! # What a source can do
//!
//! A scanner's functional units (`ICScannerFunctionalUnit`) are its sources: its flatbed and its
//! document feeder, the transparency units having no source in the model. The framework lists a
//! unit's bit depths but no pixel types, so its formats are those its depths allow: at 1 bit black
//! and white, at 8 and 16 bits gray and color. Its resolutions are its `supportedResolutions`, one
//! contiguous run a range stepping by 1 and anything else a list, or its current resolution where
//! it lists none; its largest area is its `physicalSize`, converted to millimetres from the unit's
//! own measurement unit (inches, centimetres, picas, points, twips, or pixels at its resolution).
//! A feeder's paper is set by document type rather than by size: the backend states sizes for A3,
//! A4, A5, A6, US Letter, Legal, Ledger, Executive and Statement, and leaves out the B sizes (the
//! framework's documentation doesn't show which B its types mean) and the business card.
//!
//! # Pages
//!
//! A page comes by memory-based transfer, in bands (`ICScannerBandData`), assembled into the raw
//! layout: every number in a band's header is checked before a byte is read; the page's size
//! against the caps every backend that assembles pages holds (2^28 pixels, 512 MiB) before its
//! buffer is allocated, once; its bands in order, top down, each within the page; a band's buffer
//! past 64 MiB refused. Each row's padding is dropped and 16-bit samples put little-endian. Black
//! and white, and gray and interleaved red, green and blue at 8 or 16 bits a sample, are taken;
//! anything else is refused with the header's numbers. Unverified on a device: black and white's
//! 0 is taken as black, and gray's as the least intensity, as Core Graphics' gray has it.
//!
//! # Errors
//!
//! An `NSError` in ImageCaptureCore's domain is mapped by its code: a scanner in use, a session
//! open already or a device still being enumerated is [`ScanError::Busy`]; a device this process
//! may not open, or one asking for credentials (none are given), [`ScanError::AccessDenied`]; the
//! device's module exiting, its session closed suddenly or the device ejected suddenly,
//! [`ScanError::Gone`]. Every other code, and any other domain, is [`ScanError::Driver`] in the
//! framework's words, with the code. The framework has no code for a jam, a double feed or an open
//! cover, so those come as [`ScanError::Driver`] too.
//!
//! The framework's facts here are from Apple's documentation
//! (<https://developer.apple.com/documentation/imagecapturecore>) and the SDK's headers as
//! `objc2-image-capture-core` gives them (<https://docs.rs/objc2-image-capture-core/0.3.2/>).

use std::time::Duration;

use crate::{Backend, DeviceInfo, Opened, ScanError};

#[allow(dead_code)]
mod caps;
#[allow(dead_code)]
mod driver;
#[allow(dead_code)]
mod error;
#[allow(dead_code)]
mod id;
#[allow(dead_code)]
mod pixels;

/// The ImageCaptureCore scanner backend (`ica:` ids).
///
/// Not yet wired to the framework: listing and opening report [`ScanError::NotInstalled`] on
/// every system for now, after refusing an id it doesn't form; see [the module
/// documentation](self).
#[derive(Debug, Default)]
pub struct IcaBackend {
    _private: (),
}

impl IcaBackend {
    /// An `IcaBackend`. It reaches nothing as it's made.
    pub fn new() -> IcaBackend {
        IcaBackend::default()
    }
}

fn not_installed() -> ScanError {
    ScanError::NotInstalled {
        what: error::ICA.to_owned(),
    }
}

impl Backend for IcaBackend {
    fn list(&mut self, _deadline: Duration) -> Result<Vec<DeviceInfo>, ScanError> {
        Err(not_installed())
    }

    fn open(&mut self, stored: &DeviceInfo, _deadline: Duration) -> Result<Opened, ScanError> {
        let _ = id::native_id(stored)?;
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
