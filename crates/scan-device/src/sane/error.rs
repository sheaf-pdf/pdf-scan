//! What `sane-dl` says went wrong, as the model's errors.

use sane_dl::{Call, Error, LoadError, Status};

use crate::ScanError;

/// SANE, as a user knows it, where it isn't installed.
const SANE: &str = "SANE";

/// Why SANE couldn't be loaded: not installed where there's no library to load (`NotFound`) or
/// the system has none (`NoDefault`, as on Windows), so the user hears SANE isn't there; any
/// other failure in the driver's words.
pub(super) fn load(error: &LoadError) -> ScanError {
    match error {
        LoadError::NotFound { .. } | LoadError::NoDefault => ScanError::NotInstalled {
            what: SANE.to_owned(),
        },
        other => driver(other),
    }
}

/// A call's error, as the model has it.
///
/// A status by what it says of the scanner (a jam, an open cover, busy, access refused, gone);
/// `INVAL` from `sane_open`, which comes for a device gone and a backend not installed alike, as
/// gone. A listing past its deadline, and every call while it holds SANE after, as timed out.
/// Everything else in the driver's words: what `sane-dl` said and why, on one line, and for a cap
/// passed, that a lower resolution asks for less.
pub(super) fn map(error: &Error) -> ScanError {
    match error {
        Error::Status { status, call } => match *status {
            Status::JAMMED => ScanError::Jammed,
            Status::COVER_OPEN => ScanError::CoverOpen,
            Status::DEVICE_BUSY => ScanError::Busy,
            Status::ACCESS_DENIED => ScanError::AccessDenied,
            Status::IO_ERROR => ScanError::Gone,
            Status::INVAL if *call == Call::Open => ScanError::Gone,
            _ => driver(error),
        },
        Error::DeadlineExceeded | Error::ListingPending => ScanError::TimedOut,
        Error::LimitExceeded { .. } => ScanError::Driver {
            message: format!("{}; a lower resolution asks for less", chain(error)),
            shown: false,
        },
        other => driver(other),
    }
}

/// `error` in the driver's words, which it hasn't shown.
pub(super) fn driver(error: &dyn std::error::Error) -> ScanError {
    ScanError::Driver {
        message: chain(error),
        shown: false,
    }
}

/// An error and its sources, on one line.
pub(super) fn chain(error: &dyn std::error::Error) -> String {
    let mut text = error.to_string();
    let mut source = error.source();
    while let Some(error) = source {
        text.push_str(": ");
        text.push_str(&error.to_string());
        source = error.source();
    }
    text
}

#[cfg(test)]
mod tests {
    use std::{io, path::PathBuf};

    use sane_dl::{FrameFormat, LimitKind, Parameters, SetInfo, ValueType, Version};

    use super::*;

    fn status(status: Status, call: Call) -> ScanError {
        map(&Error::Status { status, call })
    }

    fn driver_text(error: ScanError) -> String {
        match error {
            ScanError::Driver { message, shown } => {
                assert!(!shown, "{message}");
                message
            }
            other => panic!("not the driver's words: {other:?}"),
        }
    }

    #[test]
    fn no_library_and_no_default_are_sane_not_installed() {
        let not_installed = ScanError::NotInstalled {
            what: "SANE".to_owned(),
        };
        let not_found = LoadError::NotFound {
            tried: vec![PathBuf::from("libsane.so.1")],
            source: Box::new(io::Error::other("no such file")),
        };
        assert_eq!(load(&not_found), not_installed);
        assert_eq!(load(&LoadError::NoDefault), not_installed);
    }

    #[test]
    fn every_other_load_error_is_the_driver_s_words() {
        let others = [
            (
                LoadError::MissingSymbol {
                    name: "sane_open",
                    source: Box::new(io::Error::other("undefined symbol")),
                },
                "the library has no `sane_open`, so it isn't SANE: undefined symbol",
            ),
            (
                LoadError::Init(Status::IO_ERROR),
                "sane_init returned IO_ERROR (9)",
            ),
            (
                LoadError::UnsupportedVersion(Version::from_code(0x0200_0000)),
                "the library follows SANE 2.0.0, and only SANE 1 is supported",
            ),
            (
                LoadError::AlreadyLoaded,
                "SANE is already loaded in this process",
            ),
            (
                LoadError::SlotRefused(io::Error::other("refused")),
                "couldn't take this process's slot for SANE, so nothing was loaded: refused",
            ),
        ];
        for (error, words) in others {
            assert_eq!(driver_text(load(&error)), words);
        }
        assert!(driver_text(load(&LoadError::SlotTaken)).starts_with("SANE's slot"));
    }

    #[test]
    fn statuses_say_what_they_say_of_the_scanner() {
        for call in [Call::Open, Call::Start, Call::Read, Call::ControlOption] {
            assert_eq!(status(Status::JAMMED, call), ScanError::Jammed);
            assert_eq!(status(Status::COVER_OPEN, call), ScanError::CoverOpen);
            assert_eq!(status(Status::DEVICE_BUSY, call), ScanError::Busy);
            assert_eq!(status(Status::ACCESS_DENIED, call), ScanError::AccessDenied);
            assert_eq!(status(Status::IO_ERROR, call), ScanError::Gone);
        }
        // INVAL at open: a device gone, or a backend not installed.
        assert_eq!(status(Status::INVAL, Call::Open), ScanError::Gone);
        // Elsewhere, and every other status, the driver's words.
        assert_eq!(
            driver_text(status(Status::INVAL, Call::ControlOption)),
            "sane_control_option returned INVAL (4)"
        );
        assert_eq!(
            driver_text(status(Status::NO_MEM, Call::Open)),
            "sane_open returned NO_MEM (10)"
        );
        for other in [
            Status::UNSUPPORTED,
            Status::NO_DOCS,
            Status::CANCELLED,
            Status::EOF,
            Status::WARMING_UP,
            Status::HW_LOCKED,
            Status(99),
        ] {
            assert!(matches!(
                status(other, Call::Read),
                ScanError::Driver { shown: false, .. }
            ));
        }
    }

    #[test]
    fn a_listing_past_its_deadline_is_timed_out() {
        assert_eq!(map(&Error::DeadlineExceeded), ScanError::TimedOut);
        assert_eq!(map(&Error::ListingPending), ScanError::TimedOut);
    }

    #[test]
    fn a_cap_passed_says_a_lower_resolution_asks_for_less() {
        let error = Error::LimitExceeded {
            limit: LimitKind::FrameBytes,
            value: 2_000_000_000,
            cap: 1 << 30,
        };
        assert_eq!(
            driver_text(map(&error)),
            "SANE gave 2000000000 bytes in a frame, past the cap of 1073741824; \
             a lower resolution asks for less"
        );
    }

    #[test]
    fn every_other_error_is_the_driver_s_words_with_its_sources() {
        let parameters = Parameters {
            format: FrameFormat::GRAY,
            last_frame: 1,
            bytes_per_line: 2500,
            pixels_per_line: 2500,
            lines: 0,
            depth: 8,
        };
        let others = [
            Error::BadDeviceList {
                reason: "null".to_owned(),
            },
            Error::BadHandle,
            Error::NulInName,
            Error::Thread(io::Error::other("no threads")),
            Error::BadDescriptor {
                option: 3,
                reason: "null".to_owned(),
            },
            Error::BadValue {
                option: 3,
                reason: "a BOOL of 2".to_owned(),
            },
            Error::NoSuchOption { option: 9 },
            Error::NoValue {
                option: 1,
                value_type: ValueType::Group,
            },
            Error::Inactive { option: 2 },
            Error::NotReadable { option: 2 },
            Error::NotSettable { option: 2 },
            Error::NotAutomatic { option: 2 },
            Error::WrongType {
                option: 2,
                value_type: ValueType::Int,
                given: ValueType::String,
            },
            Error::WrongLength {
                option: 2,
                expected: 1,
                given: 2,
            },
            Error::StringTooLong {
                option: 2,
                size: 8,
                given: 9,
            },
            Error::NulInString { option: 2 },
            Error::Unusable {
                option: 2,
                reason: "an unknown type".to_owned(),
            },
            Error::StartInvalid { reload: None },
            Error::ReloadAfterSet {
                info: SetInfo::default(),
                reload: Box::new(Error::BadDescriptor {
                    option: 4,
                    reason: "null".to_owned(),
                }),
            },
            Error::BadReadLength {
                length: -1,
                asked: 64,
            },
            Error::BadParameters {
                parameters,
                reason: "lines is 0".to_owned(),
            },
            Error::ShortFrame {
                expected: 16,
                read: 8,
            },
            Error::LongFrame { expected: 16 },
            Error::PartialRow {
                rows: 2,
                extra: 3,
                bytes_per_line: 8,
            },
            Error::Cancelled,
            Error::BadSelectFd { fd: -1 },
        ];
        for error in &others {
            assert_eq!(driver_text(map(error)), chain(error), "{error:?}");
        }
        // The sources follow on the same line.
        let reloaded = Error::ReloadAfterSet {
            info: SetInfo::default(),
            reload: Box::new(Error::NoSuchOption { option: 4 }),
        };
        assert_eq!(
            driver_text(map(&reloaded)),
            "the set was made, but the descriptors couldn't be read again: there is no option 4"
        );
        assert_eq!(
            driver_text(map(&Error::Thread(io::Error::other("no threads")))),
            "couldn't start the listing's thread: no threads"
        );
    }
}
