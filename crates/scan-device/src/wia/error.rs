//! Mapping WIA 2.0 and COM `HRESULT` codes (`i32`) to [`ScanError`].
//!
//! Error code tables follow the Windows SDK (`WiaDef.h`, `WinError.h`) and also accept Microsoft
//! Learn's mis-converted hex literals (`0x8021_0016..=0x8021_0021` for `WiaDef.h` decimal offsets
//! `16..=21`, as documented in permissive prior art: `printscan-windows`, MIT OR Apache-2.0, and
//! `NAPS2.Wia`, MIT). All `Driver` messages are formatted from the numeric `HRESULT` and symbolic
//! constant rather than localized OS `FormatMessageW` strings (`E_FAIL` text varies by locale).

#![allow(missing_docs)]

use crate::ScanError;

/// What isn't installed when the WIA 2.0 COM service (`CLSID_WiaDevMgr2` / `stisvc`) is missing
/// or disabled, or off Windows.
pub(super) const WIA: &str = "WIA";

// Standard COM / Win32 HRESULTs (`u32::cast_signed()`).
pub const S_OK_HR: i32 = 0;
pub const S_FALSE_HR: i32 = 1;
pub const E_NOTIMPL_HR: i32 = 0x8000_4001u32.cast_signed();
pub const E_NOINTERFACE_HR: i32 = 0x8000_4002u32.cast_signed();
pub const E_POINTER_HR: i32 = 0x8000_4003u32.cast_signed();
pub const E_ABORT_HR: i32 = 0x8000_4004u32.cast_signed();
pub const E_FAIL_HR: i32 = 0x8000_4005u32.cast_signed();
pub const REGDB_E_CLASSNOTREG_HR: i32 = 0x8004_0154u32.cast_signed();
pub const CO_E_OBJNOTCONNECTED_HR: i32 = 0x8004_01FDu32.cast_signed();
pub const STG_E_MEDIUMFULL_HR: i32 = 0x8003_0070u32.cast_signed();
pub const E_ACCESSDENIED_HR: i32 = 0x8007_0005u32.cast_signed();
pub const E_OUTOFMEMORY_HR: i32 = 0x8007_000Eu32.cast_signed();
pub const E_INVALIDARG_HR: i32 = 0x8007_0057u32.cast_signed();
pub const ERROR_FILE_NOT_FOUND_HR: i32 = 0x8007_0002u32.cast_signed();
pub const ERROR_BUSY_HR: i32 = 0x8007_00AAu32.cast_signed();
pub const ERROR_SERVICE_DISABLED_HR: i32 = 0x8007_0422u32.cast_signed();
pub const ERROR_SERVICE_DOES_NOT_EXIST_HR: i32 = 0x8007_0424u32.cast_signed();
pub const ERROR_SERVICE_NOT_ACTIVE_HR: i32 = 0x8007_0426u32.cast_signed();
pub const ERROR_DEVICE_NOT_CONNECTED_HR: i32 = 0x8007_048Fu32.cast_signed();
pub const ERROR_NOT_FOUND_HR: i32 = 0x8007_0490u32.cast_signed();
pub const ERROR_CANCELLED_HR: i32 = 0x8007_04C7u32.cast_signed();
pub const RPC_S_SERVER_UNAVAILABLE_HR: i32 = 0x8007_06BAu32.cast_signed();
pub const RPC_S_CALL_FAILED_HR: i32 = 0x8007_06BEu32.cast_signed();

// WIA 2.0 non-negative status HRESULTs (`FACILITY_WIA = 33 = 0x21`, `SEVERITY_SUCCESS = 0`).
pub const WIA_STATUS_END_OF_MEDIA_HR: i32 = 0x0021_0001;
pub const WIA_STATUS_WARMING_UP_HR: i32 = 0x0021_0002;
pub const WIA_STATUS_CALIBRATING_HR: i32 = 0x0021_0003;
pub const WIA_STATUS_RESERVING_NETWORK_DEVICE_HR: i32 = 0x0021_0006;
pub const WIA_STATUS_NETWORK_DEVICE_RESERVED_HR: i32 = 0x0021_0007;
pub const WIA_STATUS_CLEAR_HR: i32 = 0x0021_0008;
pub const WIA_STATUS_SKIP_ITEM_HR: i32 = 0x0021_0009;
pub const WIA_STATUS_NOT_HANDLED_HR: i32 = 0x0021_000A;

// WIA 2.0 negative error HRESULTs (`WiaDef.h`, `FACILITY_WIA = 33 = 0x21`, `SEVERITY_ERROR = 1`).
pub const WIA_ERROR_GENERAL_ERROR_HR: i32 = 0x8021_0001u32.cast_signed();
pub const WIA_ERROR_PAPER_JAM_HR: i32 = 0x8021_0002u32.cast_signed();
pub const WIA_ERROR_PAPER_EMPTY_HR: i32 = 0x8021_0003u32.cast_signed();
pub const WIA_ERROR_PAPER_PROBLEM_HR: i32 = 0x8021_0004u32.cast_signed();
pub const WIA_ERROR_OFFLINE_HR: i32 = 0x8021_0005u32.cast_signed();
pub const WIA_ERROR_BUSY_HR: i32 = 0x8021_0006u32.cast_signed();
pub const WIA_ERROR_WARMING_UP_HR: i32 = 0x8021_0007u32.cast_signed();
pub const WIA_ERROR_USER_INTERVENTION_HR: i32 = 0x8021_0008u32.cast_signed();
pub const WIA_ERROR_ITEM_DELETED_HR: i32 = 0x8021_0009u32.cast_signed();
pub const WIA_ERROR_DEVICE_COMMUNICATION_HR: i32 = 0x8021_000Au32.cast_signed();
pub const WIA_ERROR_INVALID_COMMAND_HR: i32 = 0x8021_000Bu32.cast_signed();
pub const WIA_ERROR_INCORRECT_HARDWARE_SETTING_HR: i32 = 0x8021_000Cu32.cast_signed();
pub const WIA_ERROR_DEVICE_LOCKED_HR: i32 = 0x8021_000Du32.cast_signed();
pub const WIA_ERROR_EXCEPTION_IN_DRIVER_HR: i32 = 0x8021_000Eu32.cast_signed();
pub const WIA_ERROR_INVALID_DRIVER_RESPONSE_HR: i32 = 0x8021_000Fu32.cast_signed();
pub const WIA_ERROR_COVER_OPEN_HR: i32 = 0x8021_0010u32.cast_signed();
pub const WIA_ERROR_LAMP_OFF_HR: i32 = 0x8021_0011u32.cast_signed();
pub const WIA_ERROR_DESTINATION_HR: i32 = 0x8021_0012u32.cast_signed();
pub const WIA_ERROR_NETWORK_RESERVATION_FAILED_HR: i32 = 0x8021_0013u32.cast_signed();
pub const WIA_ERROR_MULTI_FEED_HR: i32 = 0x8021_0014u32.cast_signed();
/// In `WiaDef.h`, `MAKE_HRESULT(SEVERITY_ERROR, FACILITY_WIA, 21)` (`0x8021_0015`) is defined
/// twice: as `WIA_S_NO_DEVICE_AVAILABLE` (returned by `stisvc` `CreateDevice` for missing device
/// IDs) and as `WIA_ERROR_MAXIMUM_PRINTER_ENDORSER_COUNTER` (returned during transfer).
pub const WIA_NO_DEVICE_OR_ENDORSER_HR: i32 = 0x8021_0015u32.cast_signed();

// Microsoft Learn documentation hex misprints (decimal 16..=21 written as 0x16..=0x21).
pub const LEARN_WIA_ERROR_COVER_OPEN_HR: i32 = 0x8021_0016u32.cast_signed();
pub const LEARN_WIA_ERROR_LAMP_OFF_HR: i32 = 0x8021_0017u32.cast_signed();
pub const LEARN_WIA_ERROR_DESTINATION_HR: i32 = 0x8021_0018u32.cast_signed();
pub const LEARN_WIA_ERROR_NETWORK_RESERVATION_FAILED_HR: i32 = 0x8021_0019u32.cast_signed();
pub const LEARN_WIA_ERROR_MULTI_FEED_HR: i32 = 0x8021_0020u32.cast_signed();
pub const LEARN_WIA_ERROR_MAX_ENDORSER_HR: i32 = 0x8021_0021u32.cast_signed();

/// Which WIA operation produced an `HRESULT`, disambiguating overloaded codes such as
/// `0x8021_0015` (`WIA_S_NO_DEVICE_AVAILABLE` vs `WIA_ERROR_MAXIMUM_PRINTER_ENDORSER_COUNTER`),
/// `E_INVALIDARG`, and `WIA_ERROR_PAPER_EMPTY`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum WiaOp {
    CreateManager,
    EnumDevices,
    OpenDevice,
    Caps,
    Settings,
    Download,
    StopFeeder,
}

impl WiaOp {
    fn label(self) -> &'static str {
        match self {
            WiaOp::CreateManager => "starting WIA device manager",
            WiaOp::EnumDevices => "listing WIA devices",
            WiaOp::OpenDevice => "opening WIA device",
            WiaOp::Caps => "reading WIA capabilities",
            WiaOp::Settings => "applying WIA settings",
            WiaOp::Download => "transferring WIA page",
            WiaOp::StopFeeder => "stopping WIA feeder",
        }
    }
}

/// Wraps `message` as an unshown [`ScanError::Driver`].
pub(super) fn driver_msg(message: impl Into<String>) -> ScanError {
    ScanError::Driver {
        message: message.into(),
        shown: false,
    }
}

/// Symbolic name for a known WIA or COM `HRESULT`.
pub(super) fn symbol(op: WiaOp, hr: i32) -> Option<&'static str> {
    match hr {
        S_OK_HR => Some("S_OK"),
        S_FALSE_HR => Some("S_FALSE"),
        WIA_STATUS_END_OF_MEDIA_HR => Some("WIA_STATUS_END_OF_MEDIA"),
        WIA_STATUS_WARMING_UP_HR => Some("WIA_STATUS_WARMING_UP"),
        WIA_STATUS_CALIBRATING_HR => Some("WIA_STATUS_CALIBRATING"),
        WIA_STATUS_RESERVING_NETWORK_DEVICE_HR => Some("WIA_STATUS_RESERVING_NETWORK_DEVICE"),
        WIA_STATUS_NETWORK_DEVICE_RESERVED_HR => Some("WIA_STATUS_NETWORK_DEVICE_RESERVED"),
        WIA_STATUS_CLEAR_HR => Some("WIA_STATUS_CLEAR"),
        WIA_STATUS_SKIP_ITEM_HR => Some("WIA_STATUS_SKIP_ITEM"),
        WIA_STATUS_NOT_HANDLED_HR => Some("WIA_STATUS_NOT_HANDLED"),
        WIA_ERROR_GENERAL_ERROR_HR => Some("WIA_ERROR_GENERAL_ERROR"),
        WIA_ERROR_PAPER_JAM_HR => Some("WIA_ERROR_PAPER_JAM"),
        WIA_ERROR_PAPER_EMPTY_HR => Some("WIA_ERROR_PAPER_EMPTY"),
        WIA_ERROR_PAPER_PROBLEM_HR => Some("WIA_ERROR_PAPER_PROBLEM"),
        WIA_ERROR_OFFLINE_HR => Some("WIA_ERROR_OFFLINE"),
        WIA_ERROR_BUSY_HR => Some("WIA_ERROR_BUSY"),
        WIA_ERROR_WARMING_UP_HR => Some("WIA_ERROR_WARMING_UP"),
        WIA_ERROR_USER_INTERVENTION_HR => Some("WIA_ERROR_USER_INTERVENTION"),
        WIA_ERROR_ITEM_DELETED_HR => Some("WIA_ERROR_ITEM_DELETED"),
        WIA_ERROR_DEVICE_COMMUNICATION_HR => Some("WIA_ERROR_DEVICE_COMMUNICATION"),
        WIA_ERROR_INVALID_COMMAND_HR => Some("WIA_ERROR_INVALID_COMMAND"),
        WIA_ERROR_INCORRECT_HARDWARE_SETTING_HR => Some("WIA_ERROR_INCORRECT_HARDWARE_SETTING"),
        WIA_ERROR_DEVICE_LOCKED_HR => Some("WIA_ERROR_DEVICE_LOCKED"),
        WIA_ERROR_EXCEPTION_IN_DRIVER_HR => Some("WIA_ERROR_EXCEPTION_IN_DRIVER"),
        WIA_ERROR_INVALID_DRIVER_RESPONSE_HR => Some("WIA_ERROR_INVALID_DRIVER_RESPONSE"),
        WIA_ERROR_COVER_OPEN_HR | LEARN_WIA_ERROR_COVER_OPEN_HR => Some("WIA_ERROR_COVER_OPEN"),
        WIA_ERROR_LAMP_OFF_HR | LEARN_WIA_ERROR_LAMP_OFF_HR => Some("WIA_ERROR_LAMP_OFF"),
        WIA_ERROR_DESTINATION_HR | LEARN_WIA_ERROR_DESTINATION_HR => Some("WIA_ERROR_DESTINATION"),
        WIA_ERROR_NETWORK_RESERVATION_FAILED_HR | LEARN_WIA_ERROR_NETWORK_RESERVATION_FAILED_HR => {
            Some("WIA_ERROR_NETWORK_RESERVATION_FAILED")
        }
        WIA_ERROR_MULTI_FEED_HR | LEARN_WIA_ERROR_MULTI_FEED_HR => Some("WIA_ERROR_MULTI_FEED"),
        WIA_NO_DEVICE_OR_ENDORSER_HR => match op {
            WiaOp::CreateManager | WiaOp::EnumDevices | WiaOp::OpenDevice => {
                Some("WIA_S_NO_DEVICE_AVAILABLE")
            }
            _ => Some("WIA_ERROR_MAXIMUM_PRINTER_ENDORSER_COUNTER"),
        },
        LEARN_WIA_ERROR_MAX_ENDORSER_HR => Some("WIA_ERROR_MAXIMUM_PRINTER_ENDORSER_COUNTER"),
        E_NOTIMPL_HR => Some("E_NOTIMPL"),
        E_NOINTERFACE_HR => Some("E_NOINTERFACE"),
        E_POINTER_HR => Some("E_POINTER"),
        E_ABORT_HR => Some("E_ABORT"),
        E_FAIL_HR => Some("E_FAIL"),
        E_ACCESSDENIED_HR => Some("E_ACCESSDENIED"),
        E_OUTOFMEMORY_HR => Some("E_OUTOFMEMORY"),
        E_INVALIDARG_HR => Some("E_INVALIDARG"),
        STG_E_MEDIUMFULL_HR => Some("STG_E_MEDIUMFULL"),
        REGDB_E_CLASSNOTREG_HR => Some("REGDB_E_CLASSNOTREG"),
        CO_E_OBJNOTCONNECTED_HR => Some("CO_E_OBJNOTCONNECTED"),
        ERROR_FILE_NOT_FOUND_HR => Some("ERROR_FILE_NOT_FOUND"),
        ERROR_BUSY_HR => Some("ERROR_BUSY"),
        ERROR_SERVICE_DISABLED_HR => Some("ERROR_SERVICE_DISABLED"),
        ERROR_SERVICE_DOES_NOT_EXIST_HR => Some("ERROR_SERVICE_DOES_NOT_EXIST"),
        ERROR_SERVICE_NOT_ACTIVE_HR => Some("ERROR_SERVICE_NOT_ACTIVE"),
        ERROR_DEVICE_NOT_CONNECTED_HR => Some("ERROR_DEVICE_NOT_CONNECTED"),
        ERROR_NOT_FOUND_HR => Some("ERROR_NOT_FOUND"),
        ERROR_CANCELLED_HR => Some("ERROR_CANCELLED"),
        RPC_S_SERVER_UNAVAILABLE_HR => Some("RPC_S_SERVER_UNAVAILABLE"),
        RPC_S_CALL_FAILED_HR => Some("RPC_S_CALL_FAILED"),
        _ => None,
    }
}

/// Maps a failure `HRESULT` from `op` to [`ScanError`].
pub(super) fn hresult(op: WiaOp, hr: i32) -> ScanError {
    match hr {
        REGDB_E_CLASSNOTREG_HR
        | ERROR_SERVICE_DISABLED_HR
        | ERROR_SERVICE_DOES_NOT_EXIST_HR
        | RPC_S_SERVER_UNAVAILABLE_HR
        | ERROR_SERVICE_NOT_ACTIVE_HR
            if matches!(
                op,
                WiaOp::CreateManager | WiaOp::EnumDevices | WiaOp::OpenDevice
            ) =>
        {
            ScanError::NotInstalled {
                what: WIA.to_owned(),
            }
        }
        RPC_S_SERVER_UNAVAILABLE_HR | RPC_S_CALL_FAILED_HR
            if matches!(
                op,
                WiaOp::Caps | WiaOp::Settings | WiaOp::Download | WiaOp::StopFeeder
            ) =>
        {
            ScanError::Gone
        }
        E_ACCESSDENIED_HR => ScanError::AccessDenied,
        WIA_ERROR_PAPER_JAM_HR => ScanError::Jammed,
        WIA_ERROR_MULTI_FEED_HR | LEARN_WIA_ERROR_MULTI_FEED_HR => ScanError::DoubleFeed,
        WIA_ERROR_COVER_OPEN_HR | LEARN_WIA_ERROR_COVER_OPEN_HR => ScanError::CoverOpen,
        WIA_ERROR_BUSY_HR
        | WIA_ERROR_DEVICE_LOCKED_HR
        | ERROR_BUSY_HR
        | WIA_ERROR_NETWORK_RESERVATION_FAILED_HR
        | LEARN_WIA_ERROR_NETWORK_RESERVATION_FAILED_HR => ScanError::Busy,
        WIA_ERROR_OFFLINE_HR
        | WIA_ERROR_ITEM_DELETED_HR
        | WIA_ERROR_DEVICE_COMMUNICATION_HR
        | ERROR_DEVICE_NOT_CONNECTED_HR
        | CO_E_OBJNOTCONNECTED_HR => ScanError::Gone,
        ERROR_FILE_NOT_FOUND_HR | ERROR_NOT_FOUND_HR | WIA_NO_DEVICE_OR_ENDORSER_HR
            if matches!(op, WiaOp::OpenDevice) =>
        {
            ScanError::Gone
        }
        WIA_ERROR_INCORRECT_HARDWARE_SETTING_HR
            if matches!(op, WiaOp::Settings | WiaOp::Download) =>
        {
            ScanError::SettingsRefused
        }
        E_INVALIDARG_HR if matches!(op, WiaOp::Download) => ScanError::SettingsRefused,
        STG_E_MEDIUMFULL_HR => driver_msg(format!(
            "the scanner's stream exceeded the maximum page buffer size (STG_E_MEDIUMFULL, {:#010x}); \
             a lower resolution asks for less",
            hr.cast_unsigned()
        )),
        _ => {
            let what = op.label();
            match symbol(op, hr) {
                Some(name) => driver_msg(format!(
                    "{what} failed: {name} ({:#010x})",
                    hr.cast_unsigned()
                )),
                None => driver_msg(format!("{what} failed ({:#010x})", hr.cast_unsigned())),
            }
        }
    }
}

/// Latches a negative non-warmup `WiaTransferParams::hrErrorStatus` into `slot`, allowing a
/// specific WIA/COM error code to upgrade an earlier generic `E_FAIL` or
/// `WIA_ERROR_GENERAL_ERROR`.
pub(super) fn latch_callback_error(slot: &mut Option<i32>, hr_status: i32) {
    if hr_status >= 0 || is_warming_up(hr_status) {
        return;
    }
    match *slot {
        None => *slot = Some(hr_status),
        Some(prev)
            if matches!(prev, E_FAIL_HR | WIA_ERROR_GENERAL_ERROR_HR)
                && !matches!(hr_status, E_FAIL_HR | WIA_ERROR_GENERAL_ERROR_HR) =>
        {
            *slot = Some(hr_status);
        }
        Some(_) => {}
    }
}

/// Whether `hr` indicates an empty feeder (`WIA_ERROR_PAPER_EMPTY`).
pub(super) fn is_paper_empty(hr: i32) -> bool {
    hr == WIA_ERROR_PAPER_EMPTY_HR
}

/// Whether `hr` indicates the scanner lamp/mechanism is warming up (`WIA_STATUS_WARMING_UP`,
/// `WIA_STATUS_CALIBRATING`, or `WIA_ERROR_WARMING_UP`).
pub(super) fn is_warming_up(hr: i32) -> bool {
    matches!(
        hr,
        WIA_STATUS_WARMING_UP_HR | WIA_STATUS_CALIBRATING_HR | WIA_ERROR_WARMING_UP_HR
    )
}

/// Whether `hr` is a parameter/medium refusal eligible for a 0-page `TYMED_CALLBACK` ->
/// `TYMED_FILE` fallback.
pub(super) fn is_medium_refusal(hr: i32) -> bool {
    matches!(
        hr,
        E_INVALIDARG_HR | E_NOTIMPL_HR | WIA_ERROR_INCORRECT_HARDWARE_SETTING_HR
    )
}

/// Whether a [`ScanError`] represents a device-level failure that must abort capability reading or
/// optional property writes rather than being ignored as an unsupported optional property.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sdk_and_learn_wia_hresults_map_to_model_errors() {
        let cases = [
            (WiaOp::Download, WIA_ERROR_PAPER_JAM_HR, ScanError::Jammed),
            (
                WiaOp::Download,
                WIA_ERROR_MULTI_FEED_HR,
                ScanError::DoubleFeed,
            ),
            (
                WiaOp::Download,
                LEARN_WIA_ERROR_MULTI_FEED_HR,
                ScanError::DoubleFeed,
            ),
            (
                WiaOp::Download,
                WIA_ERROR_COVER_OPEN_HR,
                ScanError::CoverOpen,
            ),
            (
                WiaOp::Download,
                LEARN_WIA_ERROR_COVER_OPEN_HR,
                ScanError::CoverOpen,
            ),
            (WiaOp::OpenDevice, WIA_ERROR_BUSY_HR, ScanError::Busy),
            (
                WiaOp::OpenDevice,
                WIA_ERROR_DEVICE_LOCKED_HR,
                ScanError::Busy,
            ),
            (
                WiaOp::OpenDevice,
                WIA_ERROR_NETWORK_RESERVATION_FAILED_HR,
                ScanError::Busy,
            ),
            (
                WiaOp::OpenDevice,
                LEARN_WIA_ERROR_NETWORK_RESERVATION_FAILED_HR,
                ScanError::Busy,
            ),
            (WiaOp::OpenDevice, WIA_ERROR_OFFLINE_HR, ScanError::Gone),
            (
                WiaOp::Caps,
                WIA_ERROR_DEVICE_COMMUNICATION_HR,
                ScanError::Gone,
            ),
            (WiaOp::Caps, WIA_ERROR_ITEM_DELETED_HR, ScanError::Gone),
            (
                WiaOp::OpenDevice,
                E_ACCESSDENIED_HR,
                ScanError::AccessDenied,
            ),
            (
                WiaOp::Download,
                WIA_ERROR_INCORRECT_HARDWARE_SETTING_HR,
                ScanError::SettingsRefused,
            ),
            (WiaOp::Download, E_INVALIDARG_HR, ScanError::SettingsRefused),
        ];
        for (op, hr, expected) in cases {
            assert_eq!(
                hresult(op, hr),
                expected,
                "{op:?} {:#010x}",
                hr.cast_unsigned()
            );
        }
    }

    #[test]
    fn overloaded_0x80210015_is_gone_on_open_and_driver_endorser_on_download() {
        assert_eq!(
            hresult(WiaOp::OpenDevice, WIA_NO_DEVICE_OR_ENDORSER_HR),
            ScanError::Gone
        );
        assert!(matches!(
            hresult(WiaOp::EnumDevices, WIA_NO_DEVICE_OR_ENDORSER_HR),
            ScanError::Driver { shown: false, .. }
        ));
        match hresult(WiaOp::Download, WIA_NO_DEVICE_OR_ENDORSER_HR) {
            ScanError::Driver { message, shown } => {
                assert!(!shown);
                assert!(
                    message.contains("WIA_ERROR_MAXIMUM_PRINTER_ENDORSER_COUNTER"),
                    "{message}"
                );
            }
            other => panic!("expected Driver for 0x80210015 on Download, got {other:?}"),
        }
        match hresult(WiaOp::Download, LEARN_WIA_ERROR_MAX_ENDORSER_HR) {
            ScanError::Driver { message, shown } => {
                assert!(!shown);
                assert!(
                    message.contains("WIA_ERROR_MAXIMUM_PRINTER_ENDORSER_COUNTER"),
                    "{message}"
                );
            }
            other => panic!("expected Driver for 0x80210021 on Download, got {other:?}"),
        }
    }

    #[test]
    fn service_missing_or_disabled_maps_to_not_installed_and_dead_rpc_on_open_item_maps_to_gone() {
        for op in [WiaOp::CreateManager, WiaOp::EnumDevices, WiaOp::OpenDevice] {
            for hr in [
                REGDB_E_CLASSNOTREG_HR,
                ERROR_SERVICE_DISABLED_HR,
                ERROR_SERVICE_DOES_NOT_EXIST_HR,
                RPC_S_SERVER_UNAVAILABLE_HR,
                ERROR_SERVICE_NOT_ACTIVE_HR,
            ] {
                assert_eq!(
                    hresult(op, hr),
                    ScanError::NotInstalled {
                        what: "WIA".to_owned()
                    },
                    "{op:?} {:#010x}",
                    hr.cast_unsigned()
                );
            }
        }
        for op in [
            WiaOp::Caps,
            WiaOp::Settings,
            WiaOp::Download,
            WiaOp::StopFeeder,
        ] {
            for hr in [RPC_S_SERVER_UNAVAILABLE_HR, RPC_S_CALL_FAILED_HR] {
                assert_eq!(
                    hresult(op, hr),
                    ScanError::Gone,
                    "{op:?} {:#010x}",
                    hr.cast_unsigned()
                );
            }
        }
    }

    #[test]
    fn helpers_classify_paper_empty_warming_up_medium_refusal_and_driver_codes() {
        assert!(is_paper_empty(WIA_ERROR_PAPER_EMPTY_HR));
        assert!(!is_paper_empty(WIA_ERROR_PAPER_JAM_HR));
        assert!(is_warming_up(WIA_STATUS_WARMING_UP_HR));
        assert!(is_warming_up(WIA_STATUS_CALIBRATING_HR));
        assert!(is_warming_up(WIA_ERROR_WARMING_UP_HR));
        assert!(!is_warming_up(S_OK_HR));
        assert!(is_medium_refusal(E_INVALIDARG_HR));
        assert!(is_medium_refusal(E_NOTIMPL_HR));
        assert!(is_medium_refusal(WIA_ERROR_INCORRECT_HARDWARE_SETTING_HR));
        assert!(!is_medium_refusal(WIA_ERROR_PAPER_JAM_HR));
        assert!(is_fatal_scan_error(&ScanError::Gone));
        assert!(!is_fatal_scan_error(&ScanError::SettingsRefused));

        for hr in [
            WIA_ERROR_LAMP_OFF_HR,
            LEARN_WIA_ERROR_LAMP_OFF_HR,
            WIA_ERROR_DESTINATION_HR,
            LEARN_WIA_ERROR_DESTINATION_HR,
            WIA_ERROR_PAPER_PROBLEM_HR,
            E_FAIL_HR,
            STG_E_MEDIUMFULL_HR,
            0x8021_0999u32.cast_signed(),
        ] {
            assert!(
                matches!(
                    hresult(WiaOp::Download, hr),
                    ScanError::Driver { shown: false, .. }
                ),
                "{:#010x}",
                hr.cast_unsigned()
            );
        }
    }
}
