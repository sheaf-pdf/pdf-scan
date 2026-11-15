//! The WIA 2.0 backend: scanners reached through Windows Image Acquisition (`IWiaDevMgr2`).
//!
//! [`WiaBackend`] reaches WIA 2.0 over COM (`CLSID_WiaDevMgr2` in the out-of-process `stisvc`
//! Windows Image Acquisition service), covering modern USB, network WSD (`WSDScan.sys`), and
//! Mopria/IPP (`MSIPP` / `EsclScan`) scanners on Windows without requiring a vendor TWAIN `.ds`
//! driver. Off Windows (or when the WIA service is disabled or absent), listing and opening report
//! [`ScanError::NotInstalled`].
//!
//! **Run it in a worker process.** Even though `stisvc` hosts WIA minidrivers out of process,
//! `ole32` / `sti.dll` RPC stubs run in the caller's process, vendor shell/COM extensions can hang
//! synchronous COM calls past caller deadlines or write diagnostics to `stdout`, and WSD network
//! reservations bind per client process. `scan-worker` isolates `WiaBackend` inside a dedicated
//! worker process in a Windows Job Object alongside TWAIN workers.
//!
//! # Connecting to WIA 2.0
//!
//! COM (`COINIT_APARTMENTTHREADED`) and `IWiaDevMgr2` are initialized lazily on a dedicated
//! single-threaded apartment (STA) thread on the first [`list`](crate::Backend::list) or
//! [`open`](crate::Backend::open) call, never when [`WiaBackend::new`] is called. Every
//! `WiaBackend` and open device in a process shares that STA session so apartment-affined COM
//! proxies stay on one thread and WSD (`WSDScan.sys`) scanners—which reject concurrent
//! `CreateDevice` handles in the same process—are guarded by a process-wide single-open-device
//! gate (opening a second device while one is already open returns [`ScanError::Busy`]
//! immediately).
//!
//! # Devices and their ids
//!
//! A device's [`DeviceId`](crate::DeviceId) is `wia:<WIA_DIP_DEV_ID>`, with
//! [`Driver::Wia`](crate::Driver::Wia). Only scanner nodes (`StiDeviceTypeScanner == 1` in
//! `WIA_DIP_DEV_TYPE`) are listed; cameras and video devices are skipped. [`DeviceInfo`] maps
//! root device properties as:
//! - `name`: `WIA_DIP_DEV_NAME` (falling back to `WIA_WSD_FRIENDLY_NAME`, `model`, or the raw ID)
//! - `maker`: `WIA_DIP_VEND_DESC` (falling back to `WIA_WSD_MANUFACTURER`)
//! - `model`: `WIA_DIP_DEV_DESC` (falling back to `WIA_WSD_MODEL_NAME`)
//!
//! [`open`](crate::Backend::open) validates `stored` before touching COM (including off Windows
//! before [`ScanError::NotInstalled`]), refusing as [`ScanError::BadId`] any other
//! [`Driver`](crate::Driver), a missing `wia:` prefix, an empty `WIA_DIP_DEV_ID`, a `NUL` byte, or
//! an ID past 4,096 bytes. After opening the root `IWiaItem2` via `IWiaDevMgr2::CreateDevice`, it
//! verifies that `(id, maker, model)` still match `stored` so a reassigned STI instance slot
//! (`\NNNN` now pointing to another model) fails cleanly with [`ScanError::Gone`]
//! (`Opened::renamed_from` is always `None`).
//!
//! # Capabilities, settings, and batch scans
//!
//! [`Device::caps`](crate::Device::caps) walks the root `IWiaItem2` child tree (`Flatbed` and
//! `Feeder` items via `WIA_IPA_ITEM_CATEGORY` and item-name fallbacks), reading duplex support,
//! discrete or ranged `WIA_IPS_XRES` resolutions, `WIA_IPA_DATATYPE` / `WIA_IPA_DEPTH` pixel
//! formats (gating 16-bit formats on `WiaImgFmt_RAW` support), `TYMED_FILE` encoded transfer
//! formats, physical bed/feeder dimensions in thousandths of an inch, and `WIA_IPS_AUTO_CROP` /
//! `WIA_IPS_PAGE_SIZE` auto-paper support. WIA's modal `DeviceDlg` is never used as a standalone
//! settings window (`Caps::settings_window` is `false`, and
//! [`Device::settings_window`](crate::Device::settings_window) returns
//! [`ScanError::Unsupported`]).
//!
//! [`Device::scan`](crate::Device::scan) resets sticky image-processing properties
//! (`WIA_IPS_BLANK_PAGES`, `WIA_IPS_AUTO_DESKEW`, `WIA_IPS_ROTATION`, `WIA_IPS_PREVIEW`), writes
//! properties in Windows Driver Kit dependency order (`DATATYPE` before `DEPTH`, `TYMED` before
//! `FORMAT`, `XRES` before `YRES`, `PAGE_SIZE = WIA_PAGE_CUSTOM` and `XPOS`/`YPOS` before stepped
//! `XEXTENT`/`YEXTENT`), reads back [`Event::Applied`](crate::Event::Applied), and streams pages
//! through `IWiaTransfer::Download` with a size-capped in-memory `IStream` and sheet-granular
//! cancellation (`S_FALSE` from `GetNextStream` plus `WIA_CMD_STOP_FEEDER`). Quirk handling for
//! WSD page counts, dual SDK/Microsoft Learn `HRESULT` values, and `TYMED_CALLBACK` -> `TYMED_FILE`
//! fallback follows the Windows Driver Kit specification and permissive prior art
//! (`printscan-windows`, MIT OR Apache-2.0; `NAPS2.Wia`, MIT).

#[cfg(any(windows, test, feature = "fake"))]
use std::sync::Arc;
use std::time::Duration;

use crate::{Backend, DeviceInfo, Opened, ScanError};

#[cfg_attr(not(any(windows, test, feature = "fake")), allow(dead_code))]
mod batch;
#[cfg_attr(not(any(windows, test, feature = "fake")), allow(dead_code))]
mod caps;
#[cfg(windows)]
mod com;
#[allow(dead_code)]
mod driver;
#[allow(dead_code)]
mod error;
#[cfg_attr(not(any(windows, test, feature = "fake")), allow(dead_code))]
mod id;
#[cfg_attr(not(any(windows, test, feature = "fake")), allow(dead_code))]
mod pixels;
#[cfg(any(windows, test, feature = "fake"))]
mod session;
#[cfg_attr(not(any(windows, test, feature = "fake")), allow(dead_code))]
mod settings;

/// Internal scripted WIA 2.0 test harness exposed solely for crate integration tests and
/// `scan-worker-fake` when `feature = "fake"` is enabled.
#[cfg(feature = "fake")]
#[doc(hidden)]
#[allow(missing_docs)]
pub mod fake_wia {
    pub use super::driver::fake_wia::{
        FakeDeviceSpec, FakeDownloadCall, FakeItem, FakeTransferEvent, FakeWiaHandle, FakeWiaState,
        WriteHook,
    };
    pub use super::driver::{
        ALL_PAGES, BACK_FIRST, DUP, DUPLEX, FEED, FEEDER, FLAT, FLATBED, FRONT_FIRST, FRONT_ONLY,
        FormatPair, GuidBytes, ItemId, PORTRAIT, PropAttr, PropConstraint, PropId, PropValue,
        TYMED_CALLBACK, TYMED_FILE, WIA_AUTO_CROP_DISABLED, WIA_AUTO_CROP_SINGLE,
        WIA_AUTO_DESKEW_OFF, WIA_AUTO_DESKEW_ON, WIA_BLANK_PAGE_DETECTION_DISABLED,
        WIA_CATEGORY_FEEDER_BACK_GUID, WIA_CATEGORY_FEEDER_FRONT_GUID, WIA_CATEGORY_FEEDER_GUID,
        WIA_CATEGORY_FILM_GUID, WIA_CATEGORY_FINISHED_FILE_GUID, WIA_CATEGORY_FLATBED_GUID,
        WIA_CATEGORY_FOLDER_GUID, WIA_CMD_STOP_FEEDER_GUID, WIA_COMPRESSION_NONE, WIA_DATA_COLOR,
        WIA_DATA_DITHER, WIA_DATA_GRAYSCALE, WIA_DATA_RAW_BGR, WIA_DATA_RAW_RGB,
        WIA_DATA_THRESHOLD, WIA_DPS_DOCUMENT_HANDLING_CAPABILITIES, WIA_DPS_HORIZONTAL_BED_SIZE,
        WIA_DPS_HORIZONTAL_SHEET_FEED_SIZE, WIA_DPS_VERTICAL_BED_SIZE,
        WIA_DPS_VERTICAL_SHEET_FEED_SIZE, WIA_FINAL_SCAN, WIA_IMG_FMT_BMP_GUID,
        WIA_IMG_FMT_JPEG_GUID, WIA_IMG_FMT_MEMORYBMP_GUID, WIA_IMG_FMT_PNG_GUID,
        WIA_IMG_FMT_RAW_GUID, WIA_IMG_FMT_TIFF_GUID, WIA_IPA_BITS_PER_CHANNEL, WIA_IPA_COMPRESSION,
        WIA_IPA_DATATYPE, WIA_IPA_DEPTH, WIA_IPA_FORMAT, WIA_IPA_ITEM_CATEGORY, WIA_IPA_ITEM_NAME,
        WIA_IPA_PLANAR, WIA_IPA_TYMED, WIA_IPS_AUTO_CROP, WIA_IPS_AUTO_DESKEW, WIA_IPS_BLANK_PAGES,
        WIA_IPS_DOCUMENT_HANDLING_SELECT, WIA_IPS_MAX_HORIZONTAL_SIZE, WIA_IPS_MAX_VERTICAL_SIZE,
        WIA_IPS_PAGE_SIZE, WIA_IPS_PAGES, WIA_IPS_PHOTOMETRIC_INTERP, WIA_IPS_PREVIEW,
        WIA_IPS_ROTATION, WIA_IPS_XEXTENT, WIA_IPS_XPOS, WIA_IPS_XRES, WIA_IPS_YEXTENT,
        WIA_IPS_YPOS, WIA_IPS_YRES, WIA_ITEM_TYPE_FILE, WIA_ITEM_TYPE_FOLDER, WIA_ITEM_TYPE_IMAGE,
        WIA_ITEM_TYPE_ROOT, WIA_ITEM_TYPE_TRANSFER, WIA_LINE_ORDER_BOTTOM_TO_TOP,
        WIA_LINE_ORDER_TOP_TO_BOTTOM, WIA_PACKED_PIXEL, WIA_PAGE_AUTO, WIA_PAGE_CUSTOM,
        WIA_PAGE_LETTER, WIA_PHOTO_WHITE_0, WIA_PHOTO_WHITE_1, WIA_PROP_FLAG, WIA_PROP_LIST,
        WIA_PROP_NONE, WIA_PROP_RANGE, WIA_PROP_READ, WIA_PROP_RW, WIA_PROP_WRITE,
        WIA_TRANSFER_MSG_DEVICE_STATUS, WIA_TRANSFER_MSG_STATUS,
    };
    pub use super::error::{
        CO_E_OBJNOTCONNECTED_HR, E_ABORT_HR, E_ACCESSDENIED_HR, E_FAIL_HR, E_INVALIDARG_HR,
        E_NOTIMPL_HR, ERROR_BUSY_HR, ERROR_CANCELLED_HR, ERROR_DEVICE_NOT_CONNECTED_HR,
        ERROR_SERVICE_DISABLED_HR, LEARN_WIA_ERROR_COVER_OPEN_HR, LEARN_WIA_ERROR_MULTI_FEED_HR,
        REGDB_E_CLASSNOTREG_HR, S_FALSE_HR, S_OK_HR, STG_E_MEDIUMFULL_HR, WIA_ERROR_BUSY_HR,
        WIA_ERROR_COVER_OPEN_HR, WIA_ERROR_DEVICE_COMMUNICATION_HR, WIA_ERROR_DEVICE_LOCKED_HR,
        WIA_ERROR_GENERAL_ERROR_HR, WIA_ERROR_INCORRECT_HARDWARE_SETTING_HR,
        WIA_ERROR_MULTI_FEED_HR, WIA_ERROR_OFFLINE_HR, WIA_ERROR_PAPER_EMPTY_HR,
        WIA_ERROR_PAPER_JAM_HR, WIA_ERROR_WARMING_UP_HR, WIA_NO_DEVICE_OR_ENDORSER_HR,
        WIA_STATUS_CALIBRATING_HR, WIA_STATUS_END_OF_MEDIA_HR, WIA_STATUS_WARMING_UP_HR,
    };
    pub use super::id::RawDeviceProps;
}

/// Windows Image Acquisition (WIA 2.0) scanner backend (`wia:<WIA_DIP_DEV_ID>`).
///
/// Connects to `IWiaDevMgr2` lazily on the first [`list`](Backend::list) or
/// [`open`](Backend::open) call; see [the module documentation](self) for details.
#[derive(Debug, Default)]
pub struct WiaBackend {
    #[cfg(any(windows, test, feature = "fake"))]
    session: Option<Arc<session::Session>>,
}

impl WiaBackend {
    /// Creates a `WiaBackend` that connects lazily to the system WIA 2.0 service (`IWiaDevMgr2`).
    pub fn new() -> WiaBackend {
        WiaBackend::default()
    }

    #[cfg(feature = "fake")]
    #[doc(hidden)]
    pub fn from_fake(handle: &fake_wia::FakeWiaHandle) -> Result<WiaBackend, ScanError> {
        let session = handle.shared_session()?;
        Ok(WiaBackend {
            session: Some(session),
        })
    }

    #[cfg(all(test, not(feature = "fake")))]
    pub(crate) fn from_fake(
        handle: &driver::fake_wia::FakeWiaHandle,
    ) -> Result<WiaBackend, ScanError> {
        let session = handle.shared_session()?;
        Ok(WiaBackend {
            session: Some(session),
        })
    }

    #[cfg(windows)]
    fn active_session(&mut self) -> Result<Arc<session::Session>, ScanError> {
        if let Some(session) = &self.session {
            if !session.is_global() || !session.is_dead() {
                return Ok(Arc::clone(session));
            }
            self.session = None;
        }
        let session = session::shared()?;
        self.session = Some(Arc::clone(&session));
        Ok(session)
    }
}

#[cfg(windows)]
impl Backend for WiaBackend {
    fn list(&mut self, deadline: Duration) -> Result<Vec<DeviceInfo>, ScanError> {
        let session = self.active_session()?;
        session.list(deadline)
    }

    fn open(&mut self, stored: &DeviceInfo, deadline: Duration) -> Result<Opened, ScanError> {
        let native_id = id::native_id(stored)?.to_owned();
        let session = self.active_session()?;
        session.open(stored, native_id, deadline)
    }
}

#[cfg(not(windows))]
impl Backend for WiaBackend {
    fn list(&mut self, deadline: Duration) -> Result<Vec<DeviceInfo>, ScanError> {
        #[cfg(any(test, feature = "fake"))]
        if let Some(session) = &self.session {
            return session.list(deadline);
        }
        let _ = deadline;
        Err(ScanError::NotInstalled {
            what: error::WIA.to_owned(),
        })
    }

    fn open(&mut self, stored: &DeviceInfo, deadline: Duration) -> Result<Opened, ScanError> {
        let native_id = id::native_id(stored)?.to_owned();
        #[cfg(any(test, feature = "fake"))]
        if let Some(session) = &self.session {
            return session.open(stored, native_id, deadline);
        }
        let _ = (native_id, deadline);
        Err(ScanError::NotInstalled {
            what: error::WIA.to_owned(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wia::driver::fake_wia::{FakeDeviceSpec, FakeWiaHandle};
    use crate::wia::id::RawDeviceProps;
    use crate::{DeviceId, Driver};

    fn assert_send<T: Send>() {}

    #[test]
    fn bad_id_is_checked_before_not_installed_or_com() {
        assert_send::<WiaBackend>();
        assert_send::<Opened>();

        let mut backend = WiaBackend::new();
        for bad in [
            DeviceInfo::new(
                DeviceId("sane:test:0".to_owned()),
                "Test".to_owned(),
                "Maker".to_owned(),
                "Model".to_owned(),
                Driver::Sane,
            ),
            DeviceInfo::new(
                DeviceId("sane:test:0".to_owned()),
                "Test".to_owned(),
                "Maker".to_owned(),
                "Model".to_owned(),
                Driver::Wia,
            ),
            DeviceInfo::new(
                DeviceId("wia:".to_owned()),
                "Test".to_owned(),
                "Maker".to_owned(),
                "Model".to_owned(),
                Driver::Wia,
            ),
            DeviceInfo::new(
                DeviceId("wia:bad\0id".to_owned()),
                "Test".to_owned(),
                "Maker".to_owned(),
                "Model".to_owned(),
                Driver::Wia,
            ),
        ] {
            assert_eq!(
                backend.open(&bad, Duration::from_secs(1)).err(),
                Some(ScanError::BadId)
            );
        }
    }

    #[test]
    fn fake_backed_wia_backend_lists_opens_and_shares_single_device_guard() {
        use crate::wia::driver::{
            FormatPair, ItemId, PropValue, TYMED_FILE, WIA_CATEGORY_FLATBED_GUID, WIA_DATA_COLOR,
            WIA_IMG_FMT_BMP_GUID, WIA_IPA_DATATYPE, WIA_IPA_DEPTH, WIA_IPA_ITEM_CATEGORY,
            WIA_IPS_XRES, WIA_ITEM_TYPE_IMAGE, WIA_ITEM_TYPE_TRANSFER, fake_wia::FakeItem,
        };
        use crate::{Format, Resolutions, Source};

        let fake = FakeWiaHandle::new();
        let mut spec = FakeDeviceSpec::new(RawDeviceProps {
            dev_id: r"{6BDD1FC6}\0000".to_owned(),
            dev_type: Some(0x0001_0001),
            dev_name: Some("Desk Scanner".to_owned()),
            vend_desc: Some("Acme".to_owned()),
            dev_desc: Some("DS-500".to_owned()),
            ..RawDeviceProps::default()
        });
        spec.add_child(
            ItemId::ROOT,
            FakeItem::new(WIA_ITEM_TYPE_IMAGE | WIA_ITEM_TYPE_TRANSFER)
                .with_prop(
                    WIA_IPA_ITEM_CATEGORY,
                    PropValue::Guid(WIA_CATEGORY_FLATBED_GUID),
                    None,
                )
                .with_prop(WIA_IPS_XRES, PropValue::I4(300), None)
                .with_prop(WIA_IPA_DATATYPE, PropValue::I4(WIA_DATA_COLOR), None)
                .with_prop(WIA_IPA_DEPTH, PropValue::I4(24), None)
                .with_formats(vec![FormatPair {
                    format: WIA_IMG_FMT_BMP_GUID,
                    tymed: TYMED_FILE,
                }]),
        );
        fake.add_device(spec);

        let mut backend1 = WiaBackend::from_fake(&fake).expect("fake backend");
        let mut backend2 = WiaBackend::from_fake(&fake).expect("second fake backend");

        let listed = backend1.list(Duration::from_secs(2)).expect("list");
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id.0, r"wia:{6BDD1FC6}\0000");

        let mut opened = backend1
            .open(&listed[0], Duration::from_secs(2))
            .expect("open on backend1");
        let caps = opened.device.caps().expect("caps through session");
        assert!(!caps.settings_window);
        assert_eq!(caps.sources.len(), 1);
        assert_eq!(caps.sources[0].source, Source::Flatbed);
        assert_eq!(caps.sources[0].resolutions, Resolutions::List(vec![300]));
        assert_eq!(caps.sources[0].formats, vec![Format::Rgb8]);
        assert_eq!(
            opened.device.settings_window(None),
            Err(ScanError::Unsupported)
        );
        assert_eq!(
            backend2.open(&listed[0], Duration::from_secs(2)).err(),
            Some(ScanError::Busy)
        );
        drop(opened);
        let _reopened = backend2
            .open(&listed[0], Duration::from_secs(2))
            .expect("open on backend2 after close");
    }
}
