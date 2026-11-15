//! ImageCaptureCore's errors as [`ScanError`]s: an `NSError` in the framework's own domain by its
//! code, anything else as the framework's own words.
//!
//! The framework's codes say nothing of a jam, a double feed or an open cover, so those come as
//! [`ScanError::Driver`], in its words; a scan cancelled (by the backend, or at the scanner) is a
//! batch's end rather than an error, which the batch tells by [`is_cancel`].

use super::driver::{
    IcaError, RETURN_COMMUNICATION_TIMED_OUT, RETURN_CONNECTION_CLOSED_SESSION_SUDDENLY,
    RETURN_CONNECTION_DRIVER_EXITED, RETURN_CONNECTION_EJECTED_SUDDENLY,
    RETURN_CONNECTION_NOT_AUTHORIZED_TO_OPEN_DEVICE, RETURN_CONNECTION_SESSION_ALREADY_OPEN,
    RETURN_DEVICE_FAILED_TO_CLOSE_SESSION, RETURN_DEVICE_FAILED_TO_OPEN_SESSION,
    RETURN_DEVICE_IS_BUSY_ENUMERATING, RETURN_DEVICE_NEEDS_CREDENTIALS,
    RETURN_SCAN_OPERATION_CANCELED, RETURN_SCANNER_FAILED_TO_COMPLETE_SCAN,
    RETURN_SCANNER_FAILED_TO_SELECT_FUNCTIONAL_UNIT, RETURN_SCANNER_IN_USE_BY_LOCAL_USER,
    RETURN_SCANNER_IN_USE_BY_REMOTE_USER, RETURN_SESSION_NOT_OPENED,
};
use super::id::{MAX_TEXT_BYTES, capped};
use crate::ScanError;

/// What isn't installed, off macOS, as a user would know it.
pub(super) const ICA: &str = "ImageCaptureCore";

/// [`ScanError::Driver`] with `message`, not shown to the user.
pub(super) fn driver_msg(message: impl Into<String>) -> ScanError {
    ScanError::Driver {
        message: message.into(),
        shown: false,
    }
}

/// The framework's name for a code the backend knows, for a message.
fn code_name(code: i64) -> Option<&'static str> {
    Some(match code {
        RETURN_COMMUNICATION_TIMED_OUT => "ICReturnCommunicationTimedOut",
        RETURN_SCAN_OPERATION_CANCELED => "ICReturnScanOperationCanceled",
        RETURN_SCANNER_IN_USE_BY_LOCAL_USER => "ICReturnScannerInUseByLocalUser",
        RETURN_SCANNER_IN_USE_BY_REMOTE_USER => "ICReturnScannerInUseByRemoteUser",
        RETURN_DEVICE_FAILED_TO_OPEN_SESSION => "ICReturnDeviceFailedToOpenSession",
        RETURN_DEVICE_FAILED_TO_CLOSE_SESSION => "ICReturnDeviceFailedToCloseSession",
        RETURN_SCANNER_FAILED_TO_SELECT_FUNCTIONAL_UNIT => {
            "ICReturnScannerFailedToSelectFunctionalUnit"
        }
        RETURN_SCANNER_FAILED_TO_COMPLETE_SCAN => "ICReturnScannerFailedToCompleteScan",
        RETURN_DEVICE_NEEDS_CREDENTIALS => "ICReturnDeviceNeedsCredentials",
        RETURN_DEVICE_IS_BUSY_ENUMERATING => "ICReturnDeviceIsBusyEnumerating",
        RETURN_SESSION_NOT_OPENED => "ICReturnSessionNotOpened",
        RETURN_CONNECTION_DRIVER_EXITED => "ICReturnConnectionDriverExited",
        RETURN_CONNECTION_CLOSED_SESSION_SUDDENLY => "ICReturnConnectionClosedSessionSuddenly",
        RETURN_CONNECTION_EJECTED_SUDDENLY => "ICReturnConnectionEjectedSuddenly",
        RETURN_CONNECTION_SESSION_ALREADY_OPEN => "ICReturnConnectionSessionAlreadyOpen",
        RETURN_CONNECTION_NOT_AUTHORIZED_TO_OPEN_DEVICE => {
            "ICReturnConnectionNotAuthorizedToOpenDevice"
        }
        _ => return None,
    })
}

/// Whether `err` is the framework's word that a scan was cancelled, whether the backend cancelled
/// it or the scanner did (its own button): a batch then ends as cancelled, its pages kept.
pub(super) fn is_cancel(err: &IcaError) -> bool {
    err.ica_domain && err.code == RETURN_SCAN_OPERATION_CANCELED
}

/// `err` as a [`ScanError`].
///
/// In the framework's domain: a scanner in use, a session open already or a device still being
/// enumerated [`ScanError::Busy`]; a device this process may not open, or one asking for
/// credentials (none are given), [`ScanError::AccessDenied`]; the device's module exited, or its
/// session closed or the device ejected suddenly, [`ScanError::Gone`]. Every other code, and any
/// other domain, is [`ScanError::Driver`], the framework's localized description (capped) with the
/// code, never shown.
pub(super) fn scan_error(err: &IcaError) -> ScanError {
    if err.ica_domain {
        match err.code {
            RETURN_SCANNER_IN_USE_BY_LOCAL_USER
            | RETURN_SCANNER_IN_USE_BY_REMOTE_USER
            | RETURN_CONNECTION_SESSION_ALREADY_OPEN
            | RETURN_DEVICE_IS_BUSY_ENUMERATING => return ScanError::Busy,
            RETURN_CONNECTION_NOT_AUTHORIZED_TO_OPEN_DEVICE | RETURN_DEVICE_NEEDS_CREDENTIALS => {
                return ScanError::AccessDenied;
            }
            RETURN_CONNECTION_DRIVER_EXITED
            | RETURN_CONNECTION_CLOSED_SESSION_SUDDENLY
            | RETURN_CONNECTION_EJECTED_SUDDENLY => return ScanError::Gone,
            _ => {}
        }
    }
    driver_msg(message(err))
}

/// The framework's words for `err` and its code: `<description> (<ImageCaptureCore's name for
/// the code>, <code>)` in the framework's domain, `<description> (<domain> error <code>)` in
/// another's; the description and the domain capped.
fn message(err: &IcaError) -> String {
    let description = capped(err.description.trim(), MAX_TEXT_BYTES);
    let code = err.code;
    let source = if err.ica_domain {
        match code_name(code) {
            Some(name) => format!("{ICA}'s {name}, {code}"),
            None => format!("{ICA} error {code}"),
        }
    } else {
        format!("{} error {code}", capped(err.domain.trim(), MAX_TEXT_BYTES))
    };
    if description.is_empty() {
        source
    } else {
        format!("{description} ({source})")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ica(code: i64) -> IcaError {
        IcaError {
            ica_domain: true,
            domain: "com.apple.ImageCaptureCore".to_owned(),
            code,
            description: "The operation couldn't be completed.".to_owned(),
        }
    }

    fn driver(err: &IcaError) -> String {
        match scan_error(err) {
            ScanError::Driver {
                message,
                shown: false,
            } => message,
            other => panic!("{other:?} for {err:?}"),
        }
    }

    #[test]
    fn each_mapped_code_is_its_variant() {
        for code in [
            RETURN_SCANNER_IN_USE_BY_LOCAL_USER,
            RETURN_SCANNER_IN_USE_BY_REMOTE_USER,
            RETURN_CONNECTION_SESSION_ALREADY_OPEN,
            RETURN_DEVICE_IS_BUSY_ENUMERATING,
        ] {
            assert_eq!(scan_error(&ica(code)), ScanError::Busy, "{code}");
        }
        for code in [
            RETURN_CONNECTION_NOT_AUTHORIZED_TO_OPEN_DEVICE,
            RETURN_DEVICE_NEEDS_CREDENTIALS,
        ] {
            assert_eq!(scan_error(&ica(code)), ScanError::AccessDenied, "{code}");
        }
        for code in [
            RETURN_CONNECTION_DRIVER_EXITED,
            RETURN_CONNECTION_CLOSED_SESSION_SUDDENLY,
            RETURN_CONNECTION_EJECTED_SUDDENLY,
        ] {
            assert_eq!(scan_error(&ica(code)), ScanError::Gone, "{code}");
        }
    }

    #[test]
    fn every_other_code_is_the_frameworks_words_with_the_code() {
        assert_eq!(
            driver(&ica(RETURN_SCANNER_FAILED_TO_COMPLETE_SCAN)),
            "The operation couldn't be completed. \
             (ImageCaptureCore's ICReturnScannerFailedToCompleteScan, -9931)"
        );
        for code in [
            RETURN_DEVICE_FAILED_TO_OPEN_SESSION,
            RETURN_COMMUNICATION_TIMED_OUT,
            RETURN_SCANNER_FAILED_TO_SELECT_FUNCTIONAL_UNIT,
            RETURN_SESSION_NOT_OPENED,
            RETURN_SCAN_OPERATION_CANCELED,
        ] {
            let message = driver(&ica(code));
            assert!(message.ends_with(&format!(", {code})")), "{message}");
        }
        // A code the backend doesn't know.
        assert_eq!(
            driver(&ica(-9999)),
            "The operation couldn't be completed. (ImageCaptureCore error -9999)"
        );
        // No description: the code alone.
        let bare = IcaError {
            description: "  ".to_owned(),
            ..ica(-9999)
        };
        assert_eq!(driver(&bare), "ImageCaptureCore error -9999");
    }

    #[test]
    fn another_domains_code_is_never_read_as_the_frameworks() {
        // -9925 in ImageCaptureCore's domain is a scanner in use; in another, nothing known.
        let other = IcaError {
            ica_domain: false,
            domain: "NSOSStatusErrorDomain".to_owned(),
            code: RETURN_SCANNER_IN_USE_BY_LOCAL_USER,
            description: "Something failed.".to_owned(),
        };
        assert_eq!(
            driver(&other),
            "Something failed. (NSOSStatusErrorDomain error -9925)"
        );
        assert!(!is_cancel(&IcaError {
            code: RETURN_SCAN_OPERATION_CANCELED,
            ..other
        }));
    }

    #[test]
    fn a_cancel_is_told_apart() {
        assert!(is_cancel(&ica(RETURN_SCAN_OPERATION_CANCELED)));
        assert!(!is_cancel(&ica(RETURN_SCANNER_FAILED_TO_COMPLETE_SCAN)));
    }

    #[test]
    fn a_long_description_or_domain_is_capped() {
        let long = IcaError {
            description: "x".repeat(10 * MAX_TEXT_BYTES),
            ..ica(-9999)
        };
        let message = driver(&long);
        assert!(message.len() < MAX_TEXT_BYTES + 64, "{}", message.len());
        assert!(message.ends_with("(ImageCaptureCore error -9999)"));

        let long_domain = IcaError {
            ica_domain: false,
            domain: "d".repeat(10 * MAX_TEXT_BYTES),
            ..ica(1)
        };
        assert!(driver(&long_domain).len() < 2 * MAX_TEXT_BYTES + 64);
    }
}
