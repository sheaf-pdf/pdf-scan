//! Mapping TWAIN return codes, condition codes, and `twain-dsm` errors to [`ScanError`].

use twain_dsm::{ConditionCode, ReturnCode, ffi};

use crate::{Done, ScanError, Source};

/// What isn't installed when `TWAINDSM.dll` isn't found, or off Windows.
pub(super) const TWAIN: &str = "TWAIN";

/// Maps a failed TWAIN triplet's [`ReturnCode`], optional [`ConditionCode`] (`by_source` preferred
/// over `by_dsm`), and formatted error message to [`ScanError`].
pub(super) fn triplet_failed(
    rc: ReturnCode,
    condition: Option<ConditionCode>,
    message: String,
) -> ScanError {
    if matches!(rc, ReturnCode::BUSY | ReturnCode::SCANNERLOCKED) {
        return ScanError::Busy;
    }
    match condition {
        Some(code) => condition_to_error(code, message),
        None => ScanError::Driver {
            message,
            shown: false,
        },
    }
}

/// Maps a TWAIN [`ConditionCode`] and formatted message to [`ScanError`].
///
/// `TWCC_OPERATIONERROR` means the source has already displayed its own error dialog to the user,
/// so [`ScanError::Driver::shown`] is `true`; all other unmapped codes set `shown: false`.
pub(super) fn condition_to_error(code: ConditionCode, message: String) -> ScanError {
    match code {
        ConditionCode::PAPERJAM => ScanError::Jammed,
        ConditionCode::PAPERDOUBLEFEED => ScanError::DoubleFeed,
        ConditionCode::INTERLOCK => ScanError::CoverOpen,
        ConditionCode::MAXCONNECTIONS => ScanError::Busy,
        ConditionCode::CHECKDEVICEONLINE | ConditionCode::NODS | ConditionCode::BADDEST => {
            ScanError::Gone
        }
        ConditionCode::DENIED => ScanError::AccessDenied,
        ConditionCode::OPERATIONERROR => ScanError::Driver {
            message,
            shown: true,
        },
        _ => ScanError::Driver {
            message,
            shown: false,
        },
    }
}

/// Maps a failure of `MSG_ENABLEDS` at the start of a batch:
/// - `TWCC_NOMEDIA` on `Source::Feeder` (before any page) is [`ScanError::FeederEmpty`];
/// - `TWCC_BADVALUE` is [`ScanError::SettingsRefused`];
/// - everything else follows [`triplet_failed`].
pub(super) fn enable_failed(
    source: Source,
    rc: ReturnCode,
    condition: Option<ConditionCode>,
    message: String,
) -> ScanError {
    if source == Source::Feeder && condition == Some(ConditionCode::NOMEDIA) {
        return ScanError::FeederEmpty;
    }
    if condition == Some(ConditionCode::BADVALUE) {
        return ScanError::SettingsRefused;
    }
    triplet_failed(rc, condition, message)
}

/// Maps a failure of `MSG_ENABLEDSUIONLY` (`Device::settings_window`):
/// capability/operation refusals mean the source has no standalone settings window
/// ([`ScanError::Unsupported`]).
pub(super) fn settings_ui_failed(
    rc: ReturnCode,
    condition: Option<ConditionCode>,
    message: String,
) -> ScanError {
    if matches!(
        condition,
        Some(
            ConditionCode::CAPUNSUPPORTED
                | ConditionCode::BADCAP
                | ConditionCode::CAPBADOPERATION
                | ConditionCode::BADPROTOCOL
        )
    ) {
        return ScanError::Unsupported;
    }
    triplet_failed(rc, condition, message)
}

/// Resolves an unexpected `Event::CloseRequest` (`MSG_CLOSEDSREQ`) in state 5 during a headless
/// scan (`!forced_ui`), given `DAT_STATUS` (`status`), `source`, `pages` already delivered in the
/// batch, and whether `CAP_FEEDERLOADED` reported `Some(false)` once back in state 4.
pub(super) fn unexpected_close_request(
    source: Source,
    pages: u32,
    status: Option<ConditionCode>,
    feeder_loaded: Option<bool>,
) -> Result<Done, ScanError> {
    if let Some(code) = status
        && code != ConditionCode::SUCCESS
    {
        if code == ConditionCode::NOMEDIA && source == Source::Feeder {
            return if pages == 0 {
                Err(ScanError::FeederEmpty)
            } else {
                Ok(Done::FeederEmpty)
            };
        }
        let label = code.to_string();
        return Err(condition_to_error(
            code,
            format!("the source closed the scan ({label})"),
        ));
    }
    if source == Source::Feeder && feeder_loaded == Some(false) {
        return if pages == 0 {
            Err(ScanError::FeederEmpty)
        } else {
            Ok(Done::FeederEmpty)
        };
    }
    Ok(Done::Cancelled)
}

/// Whether a [`ScanError`] represents a device-level failure (hardware fault, offline, busy,
/// access denied, or a dialog already shown by the driver) that must abort capability negotiation
/// immediately rather than being treated as an unsupported optional capability.
pub(super) fn is_fatal_scan_error(error: &ScanError) -> bool {
    matches!(
        error,
        ScanError::Jammed
            | ScanError::DoubleFeed
            | ScanError::CoverOpen
            | ScanError::Busy
            | ScanError::Gone
            | ScanError::AccessDenied
            | ScanError::TimedOut
            | ScanError::NotInstalled { .. }
            | ScanError::Driver { shown: true, .. }
    )
}

/// Formats an error and its `source()` chain on a single line.
pub(super) fn chain(error: &dyn std::error::Error) -> String {
    let mut text = error.to_string();
    let mut current = error.source();
    while let Some(next) = current {
        text.push_str(": ");
        text.push_str(&next.to_string());
        current = next.source();
    }
    text
}

/// Wraps `message` as an unshown [`ScanError::Driver`].
pub(super) fn driver_msg(message: impl Into<String>) -> ScanError {
    ScanError::Driver {
        message: message.into(),
        shown: false,
    }
}

/// Maps `twain_dsm::LoadError` to [`ScanError`].
#[cfg(windows)]
pub(super) fn load(error: &twain_dsm::LoadError) -> ScanError {
    match error {
        twain_dsm::LoadError::NotFound { .. } => ScanError::NotInstalled {
            what: TWAIN.to_owned(),
        },
        other => driver_msg(chain(other)),
    }
}

/// Maps `twain_dsm::RunError` to [`ScanError`].
#[cfg(windows)]
pub(super) fn run(error: &twain_dsm::RunError) -> ScanError {
    driver_msg(chain(error))
}

/// Maps `twain_dsm::Error` to [`ScanError`].
#[cfg(windows)]
pub(super) fn map(error: &twain_dsm::Error) -> ScanError {
    match error {
        twain_dsm::Error::Load(err) => load(err),
        twain_dsm::Error::NoSuchSource => ScanError::Gone,
        twain_dsm::Error::Failed {
            triplet,
            rc,
            by_source,
            by_dsm,
        } => {
            // `MSG_OPENDS` failing with `NODS`/`BADDEST`/`MAXCONNECTIONS` or `DG_CONTROL` codes.
            let condition = by_source.or(*by_dsm);
            if triplet.dg == ffi::DG_CONTROL
                && triplet.dat == ffi::DAT_IDENTITY
                && triplet.msg == ffi::MSG_OPENDS
                && condition == Some(ConditionCode::BUMMER)
            {
                // Keep driver message when `MSG_OPENDS` fails generically (`TWCC_BUMMER`).
                return driver_msg(chain(error));
            }
            triplet_failed(*rc, condition, chain(error))
        }
        other => driver_msg(chain(other)),
    }
}

/// Maps a `twain_dsm::Error` from `Source::enable` at batch start.
#[cfg(windows)]
pub(super) fn map_enable(source: Source, error: &twain_dsm::Error) -> ScanError {
    match error {
        twain_dsm::Error::Failed {
            rc,
            by_source,
            by_dsm,
            ..
        } => enable_failed(source, *rc, by_source.or(*by_dsm), chain(error)),
        other => map(other),
    }
}

/// Maps a `twain_dsm::Error` from `Source::settings_ui`.
#[cfg(windows)]
pub(super) fn map_settings_ui(error: &twain_dsm::Error) -> ScanError {
    match error {
        twain_dsm::Error::Failed {
            rc,
            by_source,
            by_dsm,
            ..
        } => settings_ui_failed(*rc, by_source.or(*by_dsm), chain(error)),
        other => map(other),
    }
}

// Keep `ffi` referenced off Windows so imports stay clean everywhere.
#[cfg(not(windows))]
const _: u32 = ffi::DG_CONTROL;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn condition_codes_map_to_model_errors() {
        let cases = [
            (ConditionCode::PAPERJAM, ScanError::Jammed),
            (ConditionCode::PAPERDOUBLEFEED, ScanError::DoubleFeed),
            (ConditionCode::INTERLOCK, ScanError::CoverOpen),
            (ConditionCode::MAXCONNECTIONS, ScanError::Busy),
            (ConditionCode::CHECKDEVICEONLINE, ScanError::Gone),
            (ConditionCode::NODS, ScanError::Gone),
            (ConditionCode::BADDEST, ScanError::Gone),
            (ConditionCode::DENIED, ScanError::AccessDenied),
        ];
        for (code, expected) in cases {
            assert_eq!(
                triplet_failed(ReturnCode::FAILURE, Some(code), "failed".to_owned()),
                expected,
                "{code}"
            );
        }
    }

    #[test]
    fn return_codes_busy_and_scanner_locked_map_to_busy() {
        for rc in [ReturnCode::BUSY, ReturnCode::SCANNERLOCKED] {
            assert_eq!(
                triplet_failed(rc, Some(ConditionCode::BUMMER), "locked".to_owned()),
                ScanError::Busy
            );
            assert_eq!(
                triplet_failed(rc, None, "locked".to_owned()),
                ScanError::Busy
            );
        }
    }

    #[test]
    fn operation_error_marks_driver_shown_true_and_other_codes_false() {
        assert_eq!(
            triplet_failed(
                ReturnCode::FAILURE,
                Some(ConditionCode::OPERATIONERROR),
                "dialog shown".to_owned()
            ),
            ScanError::Driver {
                message: "dialog shown".to_owned(),
                shown: true,
            }
        );
        for code in [
            ConditionCode::BUMMER,
            ConditionCode::LOWMEMORY,
            ConditionCode::SEQERROR,
            ConditionCode::BADCAP,
            ConditionCode::CAPUNSUPPORTED,
            ConditionCode(0x9001),
        ] {
            assert_eq!(
                triplet_failed(ReturnCode::FAILURE, Some(code), "msg".to_owned()),
                ScanError::Driver {
                    message: "msg".to_owned(),
                    shown: false,
                },
                "{code}"
            );
        }
    }

    #[test]
    fn enable_maps_feeder_nomedia_and_badvalue() {
        assert_eq!(
            enable_failed(
                Source::Feeder,
                ReturnCode::FAILURE,
                Some(ConditionCode::NOMEDIA),
                "empty".to_owned()
            ),
            ScanError::FeederEmpty
        );
        assert!(matches!(
            enable_failed(
                Source::Flatbed,
                ReturnCode::FAILURE,
                Some(ConditionCode::NOMEDIA),
                "flatbed".to_owned()
            ),
            ScanError::Driver { shown: false, .. }
        ));
        assert_eq!(
            enable_failed(
                Source::Feeder,
                ReturnCode::FAILURE,
                Some(ConditionCode::BADVALUE),
                "bad".to_owned()
            ),
            ScanError::SettingsRefused
        );
    }

    #[test]
    fn settings_ui_refusals_map_to_unsupported() {
        for code in [
            ConditionCode::CAPUNSUPPORTED,
            ConditionCode::BADCAP,
            ConditionCode::CAPBADOPERATION,
            ConditionCode::BADPROTOCOL,
        ] {
            assert_eq!(
                settings_ui_failed(ReturnCode::FAILURE, Some(code), "no ui".to_owned()),
                ScanError::Unsupported
            );
        }
        assert_eq!(
            settings_ui_failed(
                ReturnCode::FAILURE,
                Some(ConditionCode::MAXCONNECTIONS),
                "busy".to_owned()
            ),
            ScanError::Busy
        );
    }

    #[test]
    fn unexpected_close_request_distinguishes_empty_feeder_faults_and_cancel() {
        assert_eq!(
            unexpected_close_request(Source::Feeder, 0, Some(ConditionCode::NOMEDIA), None),
            Err(ScanError::FeederEmpty)
        );
        assert_eq!(
            unexpected_close_request(Source::Feeder, 2, Some(ConditionCode::NOMEDIA), None),
            Ok(Done::FeederEmpty)
        );
        assert_eq!(
            unexpected_close_request(Source::Feeder, 0, None, Some(false)),
            Err(ScanError::FeederEmpty)
        );
        assert_eq!(
            unexpected_close_request(Source::Feeder, 3, None, Some(false)),
            Ok(Done::FeederEmpty)
        );
        assert_eq!(
            unexpected_close_request(
                Source::Feeder,
                1,
                Some(ConditionCode::PAPERJAM),
                Some(false)
            ),
            Err(ScanError::Jammed)
        );
        assert_eq!(
            unexpected_close_request(
                Source::Feeder,
                1,
                Some(ConditionCode::INTERLOCK),
                Some(true)
            ),
            Err(ScanError::CoverOpen)
        );
        assert_eq!(
            unexpected_close_request(Source::Flatbed, 0, None, Some(false)),
            Ok(Done::Cancelled)
        );
        assert_eq!(
            unexpected_close_request(Source::Feeder, 0, None, Some(true)),
            Ok(Done::Cancelled)
        );
    }

    #[cfg(windows)]
    #[test]
    fn windows_load_and_source_errors_map_cleanly() {
        let missing = twain_dsm::LoadError::NotFound {
            path: std::path::PathBuf::from(r"C:\Windows\System32\TWAINDSM.dll"),
        };
        assert_eq!(
            load(&missing),
            ScanError::NotInstalled {
                what: "TWAIN".to_owned()
            }
        );
        assert!(matches!(
            load(&twain_dsm::LoadError::Unsigned),
            ScanError::Driver { shown: false, .. }
        ));
        assert_eq!(map(&twain_dsm::Error::NoSuchSource), ScanError::Gone);
        assert!(matches!(
            map(&twain_dsm::Error::InvalidWindow { hwnd: 0xDEAD }),
            ScanError::Driver { shown: false, .. }
        ));
    }
}
