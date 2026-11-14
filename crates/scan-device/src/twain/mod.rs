//! The TWAIN backend: scanners reached through Windows' Data Source Manager (`TWAINDSM.dll`).
//!
//! [`TwainBackend`] reaches TWAIN through `twain-dsm`, which loads `TWAINDSM.dll` at run time
//! after checking its PE machine, Authenticode signature, and signer (`TWAIN Working Group`) and
//! never links it at build time. A program built with this backend runs on machines without the
//! Data Source Manager installed (and builds on non-Windows platforms too), reporting
//! [`ScanError::NotInstalled`] when asked to list or open devices.
//!
//! **Run it in a worker process—one 64-bit and one 32-bit.** TWAIN data sources (`.ds` files) are
//! in-process native DLLs: 32-bit drivers load only in a 32-bit process, 64-bit drivers only in a
//! 64-bit process, and vendor drivers can crash, hang, corrupt the heap, or write diagnostics to
//! `stdout`. `scan-worker` runs each bitness in an isolated worker process inside a Windows Job
//! Object and merges their listings.
//!
//! # Loading the Data Source Manager
//!
//! `TWAINDSM.dll` is loaded lazily on the first [`list`](crate::Backend::list) or
//! [`open`](crate::Backend::open), never when [`TwainBackend::new`] is called. By default it loads
//! from `%SystemRoot%\System32\TWAINDSM.dll` (which Windows' WoW64 redirects to `SysWOW64` inside a
//! 32-bit worker); [`TwainBackend::with_dsm_path`] loads an explicit path (such as a bundled
//! `TWAINDSM.dll` shipped beside the executable), still checking its PE machine header,
//! Authenticode signature, and `TWAIN Working Group` signer before running its code.
//!
//! Every `TwainBackend` and open device in a process shares one lazy single-threaded apartment
//! (STA) thread and message-only window (`twain_dsm::run`). When the last backend and device in
//! the process drop, the STA thread exits cleanly unless a driver call hung past its deadline.
//!
//! # Devices and their ids
//!
//! A device's [`DeviceId`](crate::DeviceId) is `twain64:<ProductName>` in a 64-bit process and
//! `twain32:<ProductName>` in a 32-bit process, with [`Driver::Twain`](crate::Driver::Twain)
//! carrying `bits: 64` or `bits: 32`. Its [`DeviceInfo`] maps TWAIN's
//! `TW_IDENTITY` strings as:
//! - `name`: `ProductName`
//! - `maker`: `Manufacturer`
//! - `model`: `ProductFamily`
//!
//! [`open`](crate::Backend::open) validates `stored` before loading `TWAINDSM.dll`, refusing as
//! [`ScanError::BadId`] any other [`Driver`](crate::Driver), a mismatched bitness or prefix, an
//! empty `ProductName`, one containing `NUL`, or one longer than 32 bytes (`TW_STR32`). It then
//! lists the DSM's sources and requires an exact `(ProductName, Manufacturer, ProductFamily)`
//! match (`Opened::renamed_from` is always `None`; an unlisted or differently-attributed source is
//! [`ScanError::Gone`]). Because TWAIN allows one active session cleanly per STA pump, opening a
//! second device in the same process while one is already open returns [`ScanError::Busy`]
//! immediately.
//!
//! # Capabilities, settings, and batch scans
//!
//! [`Device::caps`](crate::Device::caps) negotiates `ICAP_UNITS` to inches when settable (and
//! converts lengths and resolutions from whatever unit remains active when read-only), inspects
//! `CAP_FEEDERENABLED` across both TWAIN 2 enumerations and TWAIN 1.x `OneValue` booleans, checks
//! `CAP_DUPLEX` and `CAP_DUPLEXENABLED`, reads `ICAP_PIXELTYPE` and `ICAP_BITDEPTH` (omitting
//! 16-bit formats when native DIB transfer is used), reads `ICAP_XRESOLUTION`, reads physical bounds
//! (`ICAP_PHYSICALWIDTH` / `ICAP_PHYSICALHEIGHT` or `DAT_IMAGELAYOUT`), checks automatic paper
//! detection (`ICAP_AUTOMATICBORDERDETECTION` / `ICAP_AUTOSIZE`), and reports
//! `CAP_ENABLEDSUIONLY` as [`Caps::settings_window`](crate::Caps::settings_window).
//!
//! [`Device::scan`](crate::Device::scan) resets sticky driver image-processing capabilities
//! (`ICAP_AUTODISCARDBLANKPAGES = TWBP_DISABLE`, `ICAP_AUTOMATICROTATE = false`,
//! `CAP_INDICATORS = false`, `CAP_XFERCOUNT = -1`, `ICAP_COMPRESSION = TWCP_NONE`,
//! `ICAP_PLANARCHUNKY = TWPC_CHUNKY`), negotiates settings in order, enforces mutual exclusion
//! between `Paper::Auto` border detection and `Paper::Size` `DAT_IMAGELAYOUT` frames, reads back
//! the resulting state for [`Event::Applied`](crate::Event::Applied) (with a `0.005` mm layout
//! rounding tolerance), and transfers pages via `TWSX_MEMORY` (falling back to `TWSX_NATIVE`, or
//! using `TWSX_NATIVE` when [`TwainBackend::with_native_transfer`] is set). Sheet cancellation is
//! checked between sheets so a duplex sheet in flight finishes both sides cleanly.

use std::path::PathBuf;
use std::time::Duration;

use crate::{Backend, DeviceInfo, Opened, ScanError};

#[cfg_attr(not(windows), allow(dead_code))]
mod batch;
#[cfg_attr(not(windows), allow(dead_code))]
mod caps;
#[cfg_attr(not(windows), allow(dead_code))]
mod error;
#[cfg_attr(not(windows), allow(dead_code))]
mod id;
#[cfg_attr(not(windows), allow(dead_code))]
mod pixels;
#[cfg(windows)]
mod session;
#[cfg_attr(not(windows), allow(dead_code))]
mod settings;

/// How TWAIN state-5 notifications (`MSG_XFERREADY`, `MSG_CLOSEDSREQ`, `MSG_CLOSEDSOK`,
/// `MSG_DEVICEEVENT`) are delivered from the open data source.
///
/// By default ([`TwainBackend::new`]), TWAIN 2 sources (`DF_DS2`) use [`Delivery::Callback`] and
/// TWAIN 1.x sources use [`Delivery::Events`]; [`TwainBackend::with_delivery`] overrides that
/// choice for every source the backend opens.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Delivery {
    /// Win32 message queue pumping via `DG_CONTROL / DAT_EVENT / MSG_PROCESSEVENT`.
    Events,
    /// Registered callback via `DG_CONTROL / DAT_CALLBACK` (`DAT_CALLBACK2`).
    Callback,
}

/// TWAIN scanner backend for the running process's bitness (`twain64:` in a 64-bit process,
/// `twain32:` in a 32-bit process).
///
/// Loads `TWAINDSM.dll` lazily on the first [`list`](Backend::list) or [`open`](Backend::open)
/// call; see [the module documentation](self) for details.
#[derive(Debug, Default)]
pub struct TwainBackend {
    dsm_path: Option<PathBuf>,
    delivery: Option<Delivery>,
    native_transfer: bool,
    #[cfg(windows)]
    session: Option<std::sync::Arc<session::Session>>,
}

impl TwainBackend {
    /// Creates a `TwainBackend` that loads the system `TWAINDSM.dll`, selects [`Delivery::Callback`]
    /// for TWAIN 2 sources and [`Delivery::Events`] for TWAIN 1.x sources, and prefers
    /// `TWSX_MEMORY` transfers (falling back to `TWSX_NATIVE` when a source lacks memory transfer).
    pub fn new() -> TwainBackend {
        TwainBackend::default()
    }

    /// Loads `TWAINDSM.dll` from `path` (checking its PE machine header, Authenticode signature,
    /// and `TWAIN Working Group` signer) instead of `%SystemRoot%\System32\TWAINDSM.dll`.
    #[must_use]
    pub fn with_dsm_path(mut self, path: impl Into<PathBuf>) -> TwainBackend {
        self.dsm_path = Some(path.into());
        self
    }

    /// Forces `delivery` (`Events` or `Callback`) for every source this backend opens instead of
    /// choosing automatically from `DF_DS2`.
    #[must_use]
    pub fn with_delivery(mut self, delivery: Delivery) -> TwainBackend {
        self.delivery = Some(delivery);
        self
    }

    /// Uses `TWSX_NATIVE` (packed Windows DIB handles) instead of `TWSX_MEMORY` for capability
    /// reporting and batch scans.
    #[must_use]
    pub fn with_native_transfer(mut self) -> TwainBackend {
        self.native_transfer = true;
        self
    }

    #[cfg(windows)]
    fn session(&mut self) -> Result<std::sync::Arc<session::Session>, ScanError> {
        if let Some(session) = &self.session {
            return Ok(std::sync::Arc::clone(session));
        }
        let session = session::shared()?;
        self.session = Some(std::sync::Arc::clone(&session));
        Ok(session)
    }
}

#[cfg(windows)]
impl Backend for TwainBackend {
    fn list(&mut self, deadline: Duration) -> Result<Vec<DeviceInfo>, ScanError> {
        let session = self.session()?;
        session.list(self.dsm_path.clone(), deadline)
    }

    fn open(&mut self, stored: &DeviceInfo, deadline: Duration) -> Result<Opened, ScanError> {
        let expected_name = id::product_name(stored)?.to_owned();
        let session = self.session()?;
        session.open(
            self.dsm_path.clone(),
            self.delivery,
            self.native_transfer,
            stored,
            expected_name,
            deadline,
        )
    }
}

#[cfg(not(windows))]
impl Backend for TwainBackend {
    fn list(&mut self, _deadline: Duration) -> Result<Vec<DeviceInfo>, ScanError> {
        let _ = (&self.dsm_path, &self.delivery, self.native_transfer);
        Err(ScanError::NotInstalled {
            what: error::TWAIN.to_owned(),
        })
    }

    fn open(&mut self, stored: &DeviceInfo, _deadline: Duration) -> Result<Opened, ScanError> {
        let _ = id::product_name(stored)?;
        Err(ScanError::NotInstalled {
            what: error::TWAIN.to_owned(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DeviceId, Driver};

    #[test]
    fn builder_methods_configure_backend_and_bad_id_is_checked_first() {
        let mut backend = TwainBackend::new()
            .with_dsm_path(r"C:\Tools\TWAINDSM.dll")
            .with_delivery(Delivery::Events)
            .with_native_transfer();
        assert_eq!(backend.delivery, Some(Delivery::Events));
        assert!(backend.native_transfer);

        let wrong_driver = DeviceInfo::new(
            DeviceId("sane:test:0".to_owned()),
            "Test".to_owned(),
            "Maker".to_owned(),
            "Model".to_owned(),
            Driver::Sane,
        );
        assert_eq!(
            backend
                .open(&wrong_driver, Duration::from_secs(1))
                .unwrap_err(),
            ScanError::BadId
        );
    }
}
