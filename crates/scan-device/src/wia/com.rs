//! Thin Windows COM adapter (`IWiaDevMgr2`, `IWiaItem2`, `IWiaPropertyStorage`, `IWiaTransfer`,
//! `BoundedStream`, and `TransferCallback`) implementing [`super::driver::WiaSystem`] and
//! [`super::driver::WiaDeviceTree`].
//!
//! This is the only module in `scan-device` that permits `unsafe` code (`#![allow(unsafe_code)]`
//! under crate-level `#![deny(unsafe_code)]`), with `unsafe_op_in_unsafe_fn = "deny"` and
//! `clippy::undocumented_unsafe_blocks = "deny"` active on every block.

#![allow(unsafe_code)]

use std::any::Any;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::ptr::NonNull;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc::SyncSender;
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::ThreadId;

use windows::Win32::Devices::ImageAcquisition::{
    IEnumWIA_DEV_INFO, IEnumWIA_FORMAT_INFO, IEnumWiaItem2, IWiaDevMgr2, IWiaItem2,
    IWiaPropertyStorage, IWiaTransfer, IWiaTransferCallback, IWiaTransferCallback_Vtbl,
    WIA_CATEGORY_AUTO, WIA_CATEGORY_FEEDER, WIA_CATEGORY_FEEDER_BACK, WIA_CATEGORY_FEEDER_FRONT,
    WIA_CATEGORY_FILM, WIA_CATEGORY_FINISHED_FILE, WIA_CATEGORY_FLATBED, WIA_CATEGORY_FOLDER,
    WIA_CATEGORY_ROOT, WIA_CMD_STOP_FEEDER, WIA_FORMAT_INFO, WiaDevMgr2, WiaImgFmt_BMP,
    WiaImgFmt_JPEG, WiaImgFmt_MEMORYBMP, WiaImgFmt_PNG, WiaImgFmt_RAW, WiaImgFmt_TIFF,
    WiaTransferParams,
};
use windows::Win32::Foundation::{
    E_ABORT, E_FAIL, E_INVALIDARG, E_NOINTERFACE, E_NOTIMPL, E_POINTER, RPC_E_WRONG_THREAD,
    S_FALSE, S_OK, STG_E_INVALIDFUNCTION, STG_E_INVALIDPOINTER, STG_E_MEDIUMFULL,
};
use windows::Win32::System::Com::StructuredStorage::{
    InitPropVariantFromCLSID, PROPSPEC, PROPSPEC_0, PROPVARIANT, PRSPEC_PROPID,
};
use windows::Win32::System::Com::{
    CLSCTX_LOCAL_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
    CoUninitialize, ISequentialStream_Impl, IStream, IStream_Impl, LOCKTYPE, STATFLAG, STATSTG,
    STGC, STGTY_STREAM, STREAM_SEEK, STREAM_SEEK_CUR, STREAM_SEEK_END, STREAM_SEEK_SET,
};
use windows::Win32::System::Variant::{
    VARENUM, VT_BOOL, VT_BSTR, VT_CLSID, VT_EMPTY, VT_I2, VT_I4, VT_INT, VT_LPWSTR, VT_UI1, VT_UI2,
    VT_UI4, VT_UINT, VT_VECTOR,
};
use windows_core::{BSTR, GUID, HRESULT, IUnknown, IUnknown_Vtbl, Interface, Ref, implement};

use super::driver::{
    DownloadOutcome, FormatPair, GuidBytes, ItemId, PropAttr, PropConstraint, PropId, PropValue,
    TransferAction, TransferSink, WIA_CATEGORY_AUTO_GUID, WIA_CATEGORY_FEEDER_BACK_GUID,
    WIA_CATEGORY_FEEDER_FRONT_GUID, WIA_CATEGORY_FEEDER_GUID, WIA_CATEGORY_FILM_GUID,
    WIA_CATEGORY_FINISHED_FILE_GUID, WIA_CATEGORY_FLATBED_GUID, WIA_CATEGORY_FOLDER_GUID,
    WIA_CATEGORY_ROOT_GUID, WIA_CMD_STOP_FEEDER_GUID, WIA_DEVINFO_ENUM_LOCAL, WIA_DIP_DEV_DESC,
    WIA_DIP_DEV_ID, WIA_DIP_DEV_NAME, WIA_DIP_DEV_TYPE, WIA_DIP_VEND_DESC, WIA_IMG_FMT_BMP_GUID,
    WIA_IMG_FMT_JPEG_GUID, WIA_IMG_FMT_MEMORYBMP_GUID, WIA_IMG_FMT_PNG_GUID, WIA_IMG_FMT_RAW_GUID,
    WIA_IMG_FMT_TIFF_GUID, WIA_PROP_FLAG, WIA_PROP_LIST, WIA_PROP_RANGE,
    WIA_TRANSFER_MSG_DEVICE_STATUS, WIA_TRANSFER_MSG_END_OF_STREAM,
    WIA_TRANSFER_MSG_END_OF_TRANSFER, WIA_TRANSFER_MSG_NEW_PAGE, WIA_TRANSFER_MSG_STATUS,
    WIA_WSD_FRIENDLY_NAME, WIA_WSD_MANUFACTURER, WIA_WSD_MODEL_NAME, WiaDeviceTree, WiaSystem,
};
use super::error::{self, WiaOp};
use super::id::RawDeviceProps;
use crate::ScanError;
use crate::dib::MAX_DIB_BYTES;

// C1 compile-time guard: `PROPVARIANT` in `windows 0.62` MUST run `PropVariantClear` on `Drop` so
// `BSTR`, `VT_CLSID`, and `CA*` attribute vectors never leak COM heap allocations.
const _: () = assert!(core::mem::needs_drop::<PROPVARIANT>());

const fn guid_to_bytes(g: GUID) -> GuidBytes {
    GuidBytes {
        d1: g.data1,
        d2: g.data2,
        d3: g.data3,
        d4: g.data4,
    }
}

const fn bytes_to_guid(b: GuidBytes) -> GUID {
    GUID {
        data1: b.d1,
        data2: b.d2,
        data3: b.d3,
        data4: b.d4,
    }
}

const fn guid_eq(a: GuidBytes, b: GUID) -> bool {
    a.d1 == b.data1
        && a.d2 == b.data2
        && a.d3 == b.data3
        && a.d4[0] == b.data4[0]
        && a.d4[1] == b.data4[1]
        && a.d4[2] == b.data4[2]
        && a.d4[3] == b.data4[3]
        && a.d4[4] == b.data4[4]
        && a.d4[5] == b.data4[5]
        && a.d4[6] == b.data4[6]
        && a.d4[7] == b.data4[7]
}

// Compile-time cross-check of every pure `GuidBytes` constant against `windows 0.62`'s SDK GUIDs.
const _: () = {
    assert!(guid_eq(
        WIA_CATEGORY_FINISHED_FILE_GUID,
        WIA_CATEGORY_FINISHED_FILE
    ));
    assert!(guid_eq(WIA_CATEGORY_FLATBED_GUID, WIA_CATEGORY_FLATBED));
    assert!(guid_eq(WIA_CATEGORY_FEEDER_GUID, WIA_CATEGORY_FEEDER));
    assert!(guid_eq(WIA_CATEGORY_FILM_GUID, WIA_CATEGORY_FILM));
    assert!(guid_eq(WIA_CATEGORY_ROOT_GUID, WIA_CATEGORY_ROOT));
    assert!(guid_eq(WIA_CATEGORY_FOLDER_GUID, WIA_CATEGORY_FOLDER));
    assert!(guid_eq(
        WIA_CATEGORY_FEEDER_FRONT_GUID,
        WIA_CATEGORY_FEEDER_FRONT
    ));
    assert!(guid_eq(
        WIA_CATEGORY_FEEDER_BACK_GUID,
        WIA_CATEGORY_FEEDER_BACK
    ));
    assert!(guid_eq(WIA_CATEGORY_AUTO_GUID, WIA_CATEGORY_AUTO));
    assert!(guid_eq(WIA_IMG_FMT_MEMORYBMP_GUID, WiaImgFmt_MEMORYBMP));
    assert!(guid_eq(WIA_IMG_FMT_BMP_GUID, WiaImgFmt_BMP));
    assert!(guid_eq(WIA_IMG_FMT_JPEG_GUID, WiaImgFmt_JPEG));
    assert!(guid_eq(WIA_IMG_FMT_PNG_GUID, WiaImgFmt_PNG));
    assert!(guid_eq(WIA_IMG_FMT_TIFF_GUID, WiaImgFmt_TIFF));
    assert!(guid_eq(WIA_IMG_FMT_RAW_GUID, WiaImgFmt_RAW));
    assert!(guid_eq(WIA_CMD_STOP_FEEDER_GUID, WIA_CMD_STOP_FEEDER));
};

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

fn panic_payload_string(payload: Box<dyn Any + Send>) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_owned()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "panic inside WIA COM callback".to_owned()
    }
}

/// RAII guard for single-threaded apartment (`COINIT_APARTMENTTHREADED`) initialization.
///
/// Must be declared *before* any COM interface local on the STA thread so every out-of-process
/// proxy drops (`IUnknown::Release`) before `CoUninitialize()` unloads `sti.dll` / RPC stubs.
struct ComGuard(());

impl ComGuard {
    fn init_sta() -> Result<Self, ScanError> {
        // SAFETY: `CoInitializeEx` with `None` reserved pointer and `COINIT_APARTMENTTHREADED`
        // initializes COM on the current thread; balanced by `CoUninitialize` in `Drop` iff
        // `hr.is_ok()` (`S_OK` or `S_FALSE`).
        let hr = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
        if hr.is_ok() {
            Ok(ComGuard(()))
        } else {
            Err(error::hresult(WiaOp::CreateManager, hr.0))
        }
    }
}

impl Drop for ComGuard {
    fn drop(&mut self) {
        // SAFETY: `ComGuard` is constructed only when `CoInitializeEx` succeeded on this STA
        // thread, and drops after all COM interfaces on the thread have been released.
        unsafe { CoUninitialize() };
    }
}

/// Entry point run on the dedicated `"scan-device-wia"` STA thread.
pub(super) fn run_sta(
    startup_tx: SyncSender<Result<(), ScanError>>,
    body: impl FnOnce(&mut dyn WiaSystem),
) {
    let _com = match ComGuard::init_sta() {
        Ok(guard) => guard,
        Err(err) => {
            let _ = startup_tx.send(Err(err));
            return;
        }
    };
    let mut sys = ComWiaSystem;
    if startup_tx.send(Ok(())).is_err() {
        return;
    }
    body(&mut sys);
}

struct ComWiaSystem;

impl ComWiaSystem {
    fn create_manager(op: WiaOp) -> Result<IWiaDevMgr2, ScanError> {
        // SAFETY: `CoCreateInstance` is called on an initialized STA thread (`ComGuard`), passing
        // `WiaDevMgr2` CLSID and `CLSCTX_LOCAL_SERVER` to connect to the WIA `stisvc` service.
        unsafe { CoCreateInstance(&WiaDevMgr2, None, CLSCTX_LOCAL_SERVER) }
            .map_err(|err| error::hresult(op, err.code().0))
    }
}

const DEV_INFO_PROPS: [PropId; 8] = [
    WIA_DIP_DEV_ID,
    WIA_DIP_DEV_TYPE,
    WIA_DIP_DEV_NAME,
    WIA_DIP_VEND_DESC,
    WIA_DIP_DEV_DESC,
    WIA_WSD_MANUFACTURER,
    WIA_WSD_MODEL_NAME,
    WIA_WSD_FRIENDLY_NAME,
];

fn read_device_props(
    storage: &IWiaPropertyStorage,
    op: WiaOp,
) -> Result<RawDeviceProps, ScanError> {
    let vals = read_multiple(storage, &DEV_INFO_PROPS, op)?;
    let str_at = |idx: usize| vals.get(idx)?.as_ref()?.as_str().map(str::to_owned);
    let i4_at = |idx: usize| vals.get(idx)?.as_ref()?.as_i4();
    Ok(RawDeviceProps {
        dev_id: str_at(0).unwrap_or_default(),
        dev_type: i4_at(1),
        dev_name: str_at(2),
        vend_desc: str_at(3),
        dev_desc: str_at(4),
        wsd_manufacturer: str_at(5),
        wsd_model_name: str_at(6),
        wsd_friendly_name: str_at(7),
    })
}

impl WiaSystem for ComWiaSystem {
    fn list_devices(&mut self) -> Result<Vec<RawDeviceProps>, ScanError> {
        let mgr = Self::create_manager(WiaOp::CreateManager)?;
        // SAFETY: `mgr` is a valid STA `IWiaDevMgr2` proxy; `WIA_DEVINFO_ENUM_LOCAL` enumerates
        // connected local/WSD devices.
        let enumerator: IEnumWIA_DEV_INFO =
            match unsafe { mgr.EnumDeviceInfo(WIA_DEVINFO_ENUM_LOCAL) } {
                Ok(e) => e,
                Err(err) if err.code().0 == error::WIA_NO_DEVICE_OR_ENDORSER_HR => {
                    return Ok(Vec::new());
                }
                Err(err) => return Err(error::hresult(WiaOp::EnumDevices, err.code().0)),
            };

        let mut out = Vec::new();
        loop {
            let mut slot: Option<IWiaPropertyStorage> = None;
            let mut fetched = 0u32;
            // SAFETY: `slot` is a single-element output buffer and `fetched` is initialized to `0`
            // so `S_FALSE` (mapped to `Ok(())` by `windows 0.62`) leaves `fetched == 0` or
            // `slot == None`.
            unsafe { enumerator.Next(1, &mut slot, &mut fetched) }
                .map_err(|err| error::hresult(WiaOp::EnumDevices, err.code().0))?;
            if fetched == 0 {
                break;
            }
            let Some(storage) = slot else {
                break;
            };
            if let Ok(props) = read_device_props(&storage, WiaOp::EnumDevices) {
                out.push(props);
            }
        }
        Ok(out)
    }

    fn open_device(
        &mut self,
        native_id: &str,
    ) -> Result<(RawDeviceProps, Box<dyn WiaDeviceTree>), ScanError> {
        let mgr = Self::create_manager(WiaOp::CreateManager)?;
        let id_bstr = BSTR::from(native_id);
        // SAFETY: `mgr` is a valid STA `IWiaDevMgr2` proxy and `id_bstr` is a valid `BSTR`.
        let root: IWiaItem2 = unsafe { mgr.CreateDevice(0, &id_bstr) }
            .map_err(|err| error::hresult(WiaOp::OpenDevice, err.code().0))?;
        let storage: IWiaPropertyStorage = root
            .cast()
            .map_err(|err| error::hresult(WiaOp::OpenDevice, err.code().0))?;
        let props = read_device_props(&storage, WiaOp::OpenDevice)?;
        Ok((props, Box::new(ComDeviceTree { items: vec![root] })))
    }
}

struct ComDeviceTree {
    items: Vec<IWiaItem2>,
}

impl ComDeviceTree {
    fn item(&self, id: ItemId) -> Result<&IWiaItem2, ScanError> {
        self.items
            .get(id.0)
            .ok_or_else(|| error::driver_msg(format!("invalid WIA item index {}", id.0)))
    }
}

impl WiaDeviceTree for ComDeviceTree {
    fn reset_children(&mut self) {
        self.items.truncate(1);
    }

    fn children_of(&mut self, item: ItemId) -> Result<Vec<ItemId>, ScanError> {
        let parent = self.item(item)?.clone();
        // SAFETY: `parent` is a valid STA `IWiaItem2`; passing `None` enumerates all direct child
        // items regardless of category.
        let enumerator: IEnumWiaItem2 = match unsafe { parent.EnumChildItems(None) } {
            Ok(e) => e,
            Err(err) if err.code() == E_INVALIDARG || err.code() == E_NOTIMPL => {
                return Ok(Vec::new());
            }
            Err(err) => return Err(error::hresult(WiaOp::Caps, err.code().0)),
        };

        let mut ids = Vec::new();
        loop {
            let mut slot: Option<IWiaItem2> = None;
            let mut fetched = 0u32;
            // SAFETY: `slot` is a 1-element buffer and `fetched` is initialized to `0`.
            unsafe { enumerator.Next(1, &mut slot, &mut fetched) }
                .map_err(|err| error::hresult(WiaOp::Caps, err.code().0))?;
            if fetched == 0 {
                break;
            }
            let Some(child) = slot else {
                break;
            };
            let idx = self.items.len();
            self.items.push(child);
            ids.push(ItemId(idx));
        }
        Ok(ids)
    }

    fn item_type(&mut self, item: ItemId) -> Result<i32, ScanError> {
        let node = self.item(item)?;
        // SAFETY: `node` is a valid STA `IWiaItem2`.
        unsafe { node.GetItemType() }.map_err(|err| error::hresult(WiaOp::Caps, err.code().0))
    }

    fn read_props(
        &mut self,
        item: ItemId,
        ids: &[PropId],
    ) -> Result<Vec<Option<PropValue>>, ScanError> {
        let storage: IWiaPropertyStorage = self
            .item(item)?
            .cast()
            .map_err(|err| error::hresult(WiaOp::Caps, err.code().0))?;
        read_multiple(&storage, ids, WiaOp::Caps)
    }

    fn prop_attrs(
        &mut self,
        item: ItemId,
        ids: &[PropId],
    ) -> Result<Vec<Option<PropAttr>>, ScanError> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let storage: IWiaPropertyStorage = self
            .item(item)?
            .cast()
            .map_err(|err| error::hresult(WiaOp::Caps, err.code().0))?;
        let specs: Vec<PROPSPEC> = ids.iter().copied().map(make_propspec).collect();
        let mut flags = vec![0u32; specs.len()];
        let mut vars = vec![PROPVARIANT::default(); specs.len()];

        // Try batch `GetPropertyAttributes` first; if the driver rejects the batch because one of
        // the queried optional properties does not exist on this item, fall back to per-property
        // queries so supported properties are still collected.
        // SAFETY: `specs`, `flags`, and `vars` all have length `specs.len()`, and `vars` is
        // freshly initialized to `VT_EMPTY` (`PROPVARIANT::default()`).
        let batch_res = unsafe {
            storage.GetPropertyAttributes(
                specs.len() as u32,
                specs.as_ptr(),
                flags.as_mut_ptr(),
                vars.as_mut_ptr(),
            )
        };
        if batch_res.is_ok() {
            return Ok(flags
                .into_iter()
                .zip(vars.iter())
                .map(|(f, pv)| {
                    if f == 0 && pv.vt() == VT_EMPTY {
                        None
                    } else {
                        Some(decode_prop_attr(f, pv))
                    }
                })
                .collect());
        }
        if let Err(err) = batch_res {
            let mapped = error::hresult(WiaOp::Caps, err.code().0);
            if error::is_fatal_scan_error(&mapped) {
                return Err(mapped);
            }
        }

        let mut out = Vec::with_capacity(ids.len());
        for &id in ids {
            let spec = make_propspec(id);
            let mut flag = 0u32;
            let mut pv = PROPVARIANT::default();
            // SAFETY: single-element `spec`, `flag`, and fresh `VT_EMPTY` `pv`.
            let single = unsafe { storage.GetPropertyAttributes(1, &spec, &mut flag, &mut pv) };
            match single {
                Ok(()) => out.push(Some(decode_prop_attr(flag, &pv))),
                Err(err) => {
                    let mapped = error::hresult(WiaOp::Caps, err.code().0);
                    if error::is_fatal_scan_error(&mapped) {
                        return Err(mapped);
                    }
                    out.push(None);
                }
            }
        }
        Ok(out)
    }

    fn write_prop(&mut self, item: ItemId, id: PropId, val: &PropValue) -> Result<(), i32> {
        let storage: IWiaPropertyStorage = self
            .item(item)
            .map_err(|_| E_FAIL.0)?
            .cast()
            .map_err(|e| e.code().0)?;
        let spec = make_propspec(id);
        let pv = encode_prop_value(val)?;
        // SAFETY: `spec` and `pv` are valid single-element `[in]` arguments, and `WIA_DIP_FIRST`
        // (`2`) is the standard minimum property name allocation ID required by
        // `IWiaPropertyStorage::WriteMultiple`.
        unsafe { storage.WriteMultiple(1, &spec, &pv, WIA_DIP_DEV_ID) }.map_err(|err| err.code().0)
    }

    fn transfer_formats(&mut self, item: ItemId) -> Result<Vec<FormatPair>, ScanError> {
        let transfer: IWiaTransfer = self
            .item(item)?
            .cast()
            .map_err(|err| error::hresult(WiaOp::Caps, err.code().0))?;
        // SAFETY: `transfer` is a valid STA `IWiaTransfer`.
        let enumerator: IEnumWIA_FORMAT_INFO = unsafe { transfer.EnumWIA_FORMAT_INFO() }
            .map_err(|err| error::hresult(WiaOp::Caps, err.code().0))?;
        let mut pairs = Vec::new();
        loop {
            let mut info = WIA_FORMAT_INFO::default();
            let mut fetched = 0u32;
            // SAFETY: `info` is a 1-element output buffer and `fetched` is initialized to `0`.
            unsafe { enumerator.Next(1, &mut info, &mut fetched) }
                .map_err(|err| error::hresult(WiaOp::Caps, err.code().0))?;
            if fetched == 0 {
                break;
            }
            pairs.push(FormatPair {
                format: guid_to_bytes(info.guidFormatID),
                tymed: info.lTymed,
            });
        }
        Ok(pairs)
    }

    fn download(
        &mut self,
        item: ItemId,
        sink: &mut dyn TransferSink,
    ) -> Result<DownloadOutcome, ScanError> {
        let transfer: IWiaTransfer = self
            .item(item)?
            .cast()
            .map_err(|err| error::hresult(WiaOp::Download, err.code().0))?;
        Ok(run_download_with_callback(&transfer, MAX_DIB_BYTES, sink))
    }

    fn device_command(&mut self, item: ItemId, command: GuidBytes) -> Result<(), i32> {
        let node = self.item(item).map_err(|_| E_FAIL.0)?;
        let guid = bytes_to_guid(command);
        let mut out_item: Option<IWiaItem2> = None;
        // SAFETY: `node` is a valid STA `IWiaItem2`, `guid` is a valid command GUID, and
        // `out_item` receives any optional created item (dropped immediately if returned).
        unsafe { node.DeviceCommand(0, &guid, &mut out_item) }.map_err(|err| err.code().0)
    }
}

fn make_propspec(id: PropId) -> PROPSPEC {
    PROPSPEC {
        ulKind: PRSPEC_PROPID,
        Anonymous: PROPSPEC_0 { propid: id },
    }
}

fn read_multiple(
    storage: &IWiaPropertyStorage,
    ids: &[PropId],
    op: WiaOp,
) -> Result<Vec<Option<PropValue>>, ScanError> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let specs: Vec<PROPSPEC> = ids.iter().copied().map(make_propspec).collect();
    let mut vars = vec![PROPVARIANT::default(); specs.len()];
    // SAFETY: `specs` and `vars` have identical length `specs.len()`, and `vars` is freshly
    // initialized to `VT_EMPTY` (`PROPVARIANT::default()`). `S_FALSE` (when some properties do not
    // exist on this item) is mapped to `Ok(())` by `windows 0.62`, leaving missing entries as
    // `VT_EMPTY`.
    unsafe { storage.ReadMultiple(specs.len() as u32, specs.as_ptr(), vars.as_mut_ptr()) }
        .map_err(|err| error::hresult(op, err.code().0))?;
    Ok(vars.iter().map(decode_prop_value).collect())
}

fn encode_prop_value(val: &PropValue) -> Result<PROPVARIANT, i32> {
    match val {
        PropValue::I4(v) => Ok(PROPVARIANT::from(*v)),
        PropValue::Str(s) => Ok(PROPVARIANT::from(s.as_str())),
        PropValue::Guid(g) => {
            let guid = bytes_to_guid(*g);
            // SAFETY: `InitPropVariantFromCLSID` allocates the `GUID` buffer on the COM task
            // allocator (`CoTaskMemAlloc`) and tags the `PROPVARIANT` as `VT_CLSID` so
            // `PropVariantClear` on `Drop` frees it cleanly.
            unsafe { InitPropVariantFromCLSID(&guid) }.map_err(|err| err.code().0)
        }
    }
}

fn decode_prop_value(pv: &PROPVARIANT) -> Option<PropValue> {
    let vt = pv.vt();
    // SAFETY: Accessing the `Anonymous.Anonymous` union field matching `pv.vt()` as tagged by COM
    // / `PropVariantInit`.
    let inner = unsafe { &pv.Anonymous.Anonymous };
    match vt {
        VT_EMPTY => None,
        VT_I4 => {
            // SAFETY: `vt == VT_I4` selects `lVal`.
            Some(PropValue::I4(unsafe { inner.Anonymous.lVal }))
        }
        VT_UI4 => {
            // SAFETY: `vt == VT_UI4` selects `ulVal`.
            Some(PropValue::I4(
                unsafe { inner.Anonymous.ulVal }.cast_signed(),
            ))
        }
        VT_I2 => {
            // SAFETY: `vt == VT_I2` selects `iVal`.
            Some(PropValue::I4(i32::from(unsafe { inner.Anonymous.iVal })))
        }
        VT_UI2 => {
            // SAFETY: `vt == VT_UI2` selects `uiVal`.
            Some(PropValue::I4(i32::from(unsafe { inner.Anonymous.uiVal })))
        }
        VT_UI1 => {
            // SAFETY: `vt == VT_UI1` selects `bVal`.
            Some(PropValue::I4(i32::from(unsafe { inner.Anonymous.bVal })))
        }
        VT_INT => {
            // SAFETY: `vt == VT_INT` selects `intVal`.
            Some(PropValue::I4(unsafe { inner.Anonymous.intVal }))
        }
        VT_UINT => {
            // SAFETY: `vt == VT_UINT` selects `uintVal`.
            Some(PropValue::I4(
                unsafe { inner.Anonymous.uintVal }.cast_signed(),
            ))
        }
        VT_BOOL => {
            // SAFETY: `vt == VT_BOOL` selects `boolVal`.
            let b = unsafe { inner.Anonymous.boolVal };
            Some(PropValue::I4(i32::from(b.as_bool())))
        }
        VT_BSTR => {
            // SAFETY: `vt == VT_BSTR` selects `bstrVal` (`ManuallyDrop<BSTR>`). `Deref` to `[u16]`
            // safely handles null/empty `BSTR`s, and `from_utf16_lossy` never panics on unpaired
            // UTF-16 surrogates.
            let bstr = unsafe { &*inner.Anonymous.bstrVal };
            Some(PropValue::Str(String::from_utf16_lossy(bstr)))
        }
        VT_LPWSTR => {
            // SAFETY: `vt == VT_LPWSTR` selects `pwszVal`. Walk up to 65,536 `u16` code units for
            // the terminating NUL when non-null.
            let pwsz = unsafe { inner.Anonymous.pwszVal };
            if pwsz.is_null() {
                return Some(PropValue::Str(String::new()));
            }
            let ptr = pwsz.0;
            let mut len = 0usize;
            while len < 65_536 {
                // SAFETY: `ptr` points to a NUL-terminated wide string in the `PROPVARIANT`.
                if unsafe { *ptr.add(len) } == 0 {
                    break;
                }
                len += 1;
            }
            // SAFETY: `ptr[..len]` was scanned up to the NUL terminator.
            let slice = unsafe { core::slice::from_raw_parts(ptr, len) };
            Some(PropValue::Str(String::from_utf16_lossy(slice)))
        }
        VT_CLSID => {
            // SAFETY: `vt == VT_CLSID` selects `puuid`.
            let ptr = unsafe { inner.Anonymous.puuid };
            if ptr.is_null() {
                None
            } else {
                // SAFETY: `ptr` is non-null and points to a `GUID` inside the `PROPVARIANT`.
                Some(PropValue::Guid(guid_to_bytes(unsafe { *ptr })))
            }
        }
        _ => None,
    }
}

/// Maximum number of vector elements accepted from an untrusted `GetPropertyAttributes` `CA*`
/// array so a buggy driver cannot trigger an unbounded copy.
const MAX_ATTR_VECTOR_ELEMS: usize = 65_536;

fn read_vector_i4(pv: &PROPVARIANT) -> Option<Vec<i32>> {
    let vt = pv.vt();
    if (vt.0 & VT_VECTOR.0) == 0 {
        return None;
    }
    let elem_vt = VARENUM(vt.0 & !VT_VECTOR.0);
    // SAFETY: Accessing the `Anonymous.Anonymous` union matching `vt`.
    let inner = unsafe { &pv.Anonymous.Anonymous };
    match elem_vt {
        VT_I4 | VT_INT => {
            // SAFETY: `VT_VECTOR | VT_I4` uses `cal`.
            let ca = unsafe { &inner.Anonymous.cal };
            let n = (ca.cElems as usize).min(MAX_ATTR_VECTOR_ELEMS);
            if n == 0 || ca.pElems.is_null() {
                return Some(Vec::new());
            }
            // SAFETY: `ca.pElems` is non-null with at least `n` `i32` elements.
            Some(unsafe { core::slice::from_raw_parts(ca.pElems, n) }.to_vec())
        }
        VT_UI4 | VT_UINT => {
            // SAFETY: `VT_VECTOR | VT_UI4` uses `caul`.
            let ca = unsafe { &inner.Anonymous.caul };
            let n = (ca.cElems as usize).min(MAX_ATTR_VECTOR_ELEMS);
            if n == 0 || ca.pElems.is_null() {
                return Some(Vec::new());
            }
            // SAFETY: `ca.pElems` is non-null with at least `n` `u32` elements.
            Some(
                unsafe { core::slice::from_raw_parts(ca.pElems, n) }
                    .iter()
                    .map(|&v| v.cast_signed())
                    .collect(),
            )
        }
        VT_I2 => {
            // SAFETY: `VT_VECTOR | VT_I2` uses `cai`.
            let ca = unsafe { &inner.Anonymous.cai };
            let n = (ca.cElems as usize).min(MAX_ATTR_VECTOR_ELEMS);
            if n == 0 || ca.pElems.is_null() {
                return Some(Vec::new());
            }
            // SAFETY: `ca.pElems` is non-null with at least `n` `i16` elements.
            Some(
                unsafe { core::slice::from_raw_parts(ca.pElems, n) }
                    .iter()
                    .map(|&v| i32::from(v))
                    .collect(),
            )
        }
        VT_UI2 => {
            // SAFETY: `VT_VECTOR | VT_UI2` uses `caui`.
            let ca = unsafe { &inner.Anonymous.caui };
            let n = (ca.cElems as usize).min(MAX_ATTR_VECTOR_ELEMS);
            if n == 0 || ca.pElems.is_null() {
                return Some(Vec::new());
            }
            // SAFETY: `ca.pElems` is non-null with at least `n` `u16` elements.
            Some(
                unsafe { core::slice::from_raw_parts(ca.pElems, n) }
                    .iter()
                    .map(|&v| i32::from(v))
                    .collect(),
            )
        }
        VT_UI1 => {
            // SAFETY: `VT_VECTOR | VT_UI1` uses `caub`.
            let ca = unsafe { &inner.Anonymous.caub };
            let n = (ca.cElems as usize).min(MAX_ATTR_VECTOR_ELEMS);
            if n == 0 || ca.pElems.is_null() {
                return Some(Vec::new());
            }
            // SAFETY: `ca.pElems` is non-null with at least `n` `u8` elements.
            Some(
                unsafe { core::slice::from_raw_parts(ca.pElems, n) }
                    .iter()
                    .map(|&v| i32::from(v))
                    .collect(),
            )
        }
        _ => None,
    }
}

fn read_vector_guid(pv: &PROPVARIANT) -> Option<Vec<GuidBytes>> {
    if pv.vt() != VARENUM(VT_VECTOR.0 | VT_CLSID.0) {
        return None;
    }
    // SAFETY: `vt == VT_VECTOR | VT_CLSID` selects `cauuid` (`CACLSID`).
    let ca = unsafe { &pv.Anonymous.Anonymous.Anonymous.cauuid };
    let n = (ca.cElems as usize).min(MAX_ATTR_VECTOR_ELEMS);
    if n == 0 || ca.pElems.is_null() {
        return Some(Vec::new());
    }
    // SAFETY: `ca.pElems` is non-null with at least `n` `GUID` elements.
    Some(
        unsafe { core::slice::from_raw_parts(ca.pElems, n) }
            .iter()
            .copied()
            .map(guid_to_bytes)
            .collect(),
    )
}

fn decode_prop_attr(access_flags: u32, pv: &PROPVARIANT) -> PropAttr {
    let constraint = if (access_flags & WIA_PROP_RANGE) != 0 {
        match read_vector_i4(pv).as_deref() {
            Some(&[min, nom, max, step, ..]) => PropConstraint::Range {
                min,
                nom,
                max,
                step,
            },
            _ => PropConstraint::None,
        }
    } else if (access_flags & WIA_PROP_LIST) != 0 {
        if let Some(elems) = read_vector_i4(pv) {
            // `WiaDef.h`: `WIA_LIST_COUNT = 0`, `WIA_LIST_NOM = 1`, `WIA_LIST_VALUES = 2`.
            if elems.len() >= 2 {
                let declared_count = elems[0].max(0) as usize;
                let nom = elems[1];
                let avail = &elems[2..];
                let take = avail.len().min(declared_count);
                PropConstraint::ListI4 {
                    nom,
                    values: avail[..take].to_vec(),
                }
            } else {
                PropConstraint::None
            }
        } else if let Some(guids) = read_vector_guid(pv) {
            match guids.as_slice() {
                [nom, rest @ ..] if !rest.is_empty() => PropConstraint::ListGuid {
                    nom: *nom,
                    values: rest.to_vec(),
                },
                [only] => PropConstraint::ListGuid {
                    nom: *only,
                    values: vec![*only],
                },
                _ => PropConstraint::None,
            }
        } else {
            PropConstraint::None
        }
    } else if (access_flags & WIA_PROP_FLAG) != 0 {
        match read_vector_i4(pv).as_deref() {
            // `WiaDef.h`: `WIA_FLAG_NOM = 0`, `WIA_FLAG_VALUES = 1`.
            Some(&[nom, valid_bits, ..]) => PropConstraint::Flags { nom, valid_bits },
            _ => PropConstraint::None,
        }
    } else {
        PropConstraint::None
    };

    PropAttr {
        access_flags,
        constraint,
    }
}

// ============================================================================
// Size-capped in-memory `IStream` (`BoundedStream`) with `catch_unwind`
// ============================================================================

struct StreamInner {
    buf: Vec<u8>,
    pos: u64,
    cap: u64,
    overflowed: bool,
    finalized: bool,
    panic_msg: Option<String>,
}

#[implement(IStream)]
struct BoundedStream {
    inner: Arc<Mutex<StreamInner>>,
}

impl BoundedStream {
    fn create(cap: u64) -> (Arc<Mutex<StreamInner>>, IStream) {
        let inner = Arc::new(Mutex::new(StreamInner {
            buf: Vec::new(),
            pos: 0,
            cap,
            overflowed: false,
            finalized: false,
            panic_msg: None,
        }));
        let stream: IStream = BoundedStream {
            inner: Arc::clone(&inner),
        }
        .into();
        (inner, stream)
    }

    fn run_guarded<T>(
        &self,
        f: impl FnOnce(&mut StreamInner) -> windows_core::Result<T>,
    ) -> windows_core::Result<T> {
        match catch_unwind(AssertUnwindSafe(|| {
            let mut guard = lock(&self.inner);
            f(&mut guard)
        })) {
            Ok(res) => res,
            Err(payload) => {
                let mut guard = lock(&self.inner);
                if guard.panic_msg.is_none() {
                    guard.panic_msg = Some(panic_payload_string(payload));
                }
                Err(windows_core::Error::from_hresult(E_FAIL))
            }
        }
    }
}

impl ISequentialStream_Impl for BoundedStream_Impl {
    fn Read(&self, pv: *mut core::ffi::c_void, cb: u32, pcbread: *mut u32) -> HRESULT {
        match self.run_guarded(|inner| {
            if cb == 0 {
                if !pcbread.is_null() {
                    // SAFETY: `pcbread` was checked non-null.
                    unsafe { *pcbread = 0 };
                }
                return Ok(());
            }
            if pv.is_null() {
                return Err(windows_core::Error::from_hresult(STG_E_INVALIDPOINTER));
            }
            let avail = usize::try_from(inner.pos)
                .ok()
                .and_then(|pos| inner.buf.get(pos..))
                .unwrap_or(&[]);
            let to_copy = avail.len().min(cb as usize);
            if to_copy > 0 {
                // SAFETY: `pv` is non-null with capacity `cb >= to_copy`, and `avail[..to_copy]`
                // is inside `inner.buf` (disjoint from caller's `pv`).
                unsafe {
                    core::ptr::copy_nonoverlapping(avail.as_ptr(), pv.cast::<u8>(), to_copy);
                }
                inner.pos += to_copy as u64;
            }
            if !pcbread.is_null() {
                // SAFETY: `pcbread` was checked non-null.
                unsafe { *pcbread = to_copy as u32 };
            }
            Ok(())
        }) {
            Ok(()) => S_OK,
            Err(err) => err.into(),
        }
    }

    fn Write(&self, pv: *const core::ffi::c_void, cb: u32, pcbwritten: *mut u32) -> HRESULT {
        match self.run_guarded(|inner| {
            if cb == 0 {
                if !pcbwritten.is_null() {
                    // SAFETY: `pcbwritten` was checked non-null.
                    unsafe { *pcbwritten = 0 };
                }
                return Ok(());
            }
            if pv.is_null() {
                return Err(windows_core::Error::from_hresult(STG_E_INVALIDPOINTER));
            }
            let fail_full = |inner: &mut StreamInner| {
                inner.overflowed = true;
                if !pcbwritten.is_null() {
                    // SAFETY: `pcbwritten` was checked non-null.
                    unsafe { *pcbwritten = 0 };
                }
                windows_core::Error::from_hresult(STG_E_MEDIUMFULL)
            };

            let Some(end) = inner.pos.checked_add(u64::from(cb)) else {
                return Err(fail_full(inner));
            };
            if end > inner.cap {
                return Err(fail_full(inner));
            }
            let (Ok(pos_usize), Ok(end_usize)) = (usize::try_from(inner.pos), usize::try_from(end))
            else {
                return Err(fail_full(inner));
            };
            if end_usize > inner.buf.len() {
                let needed = end_usize - inner.buf.len();
                if inner.buf.try_reserve(needed).is_err() {
                    return Err(fail_full(inner));
                }
                inner.buf.resize(end_usize, 0);
            }
            // SAFETY: `pv` is non-null and `cb > 0` was checked above, so the caller supplies
            // `cb` readable bytes.
            let src = unsafe { core::slice::from_raw_parts(pv.cast::<u8>(), cb as usize) };
            inner.buf[pos_usize..end_usize].copy_from_slice(src);
            inner.pos = end;
            if !pcbwritten.is_null() {
                // SAFETY: `pcbwritten` was checked non-null.
                unsafe { *pcbwritten = cb };
            }
            Ok(())
        }) {
            Ok(()) => S_OK,
            Err(err) => err.into(),
        }
    }
}

impl IStream_Impl for BoundedStream_Impl {
    fn Seek(
        &self,
        dlibmove: i64,
        dworigin: STREAM_SEEK,
        plibnewposition: *mut u64,
    ) -> windows_core::Result<()> {
        self.run_guarded(|inner| {
            let base: i128 = match dworigin {
                STREAM_SEEK_SET => 0,
                STREAM_SEEK_CUR => i128::from(inner.pos),
                STREAM_SEEK_END => i128::from(inner.buf.len() as u64),
                _ => return Err(windows_core::Error::from_hresult(STG_E_INVALIDFUNCTION)),
            };
            let target = base + i128::from(dlibmove);
            if target < 0 {
                return Err(windows_core::Error::from_hresult(STG_E_INVALIDFUNCTION));
            }
            if target > i128::from(inner.cap) {
                inner.overflowed = true;
                return Err(windows_core::Error::from_hresult(STG_E_MEDIUMFULL));
            }
            inner.pos = target as u64;
            if !plibnewposition.is_null() {
                // SAFETY: `plibnewposition` was checked non-null.
                unsafe { *plibnewposition = inner.pos };
            }
            Ok(())
        })
    }

    fn SetSize(&self, libnewsize: u64) -> windows_core::Result<()> {
        self.run_guarded(|inner| {
            if libnewsize > inner.cap {
                inner.overflowed = true;
                return Err(windows_core::Error::from_hresult(STG_E_MEDIUMFULL));
            }
            let Ok(new_len) = usize::try_from(libnewsize) else {
                inner.overflowed = true;
                return Err(windows_core::Error::from_hresult(STG_E_MEDIUMFULL));
            };
            if new_len > inner.buf.len()
                && inner.buf.try_reserve(new_len - inner.buf.len()).is_err()
            {
                inner.overflowed = true;
                return Err(windows_core::Error::from_hresult(STG_E_MEDIUMFULL));
            }
            inner.buf.resize(new_len, 0);
            Ok(())
        })
    }

    fn CopyTo(
        &self,
        _pstm: Ref<'_, IStream>,
        _cb: u64,
        _pcbread: *mut u64,
        _pcbwritten: *mut u64,
    ) -> windows_core::Result<()> {
        Err(windows_core::Error::from_hresult(E_NOTIMPL))
    }

    fn Commit(&self, _grfcommitflags: &STGC) -> windows_core::Result<()> {
        Ok(())
    }

    fn Revert(&self) -> windows_core::Result<()> {
        Ok(())
    }

    fn LockRegion(
        &self,
        _liboffset: u64,
        _cb: u64,
        _dwlocktype: &LOCKTYPE,
    ) -> windows_core::Result<()> {
        Err(windows_core::Error::from_hresult(E_NOTIMPL))
    }

    fn UnlockRegion(
        &self,
        _liboffset: u64,
        _cb: u64,
        _dwlocktype: u32,
    ) -> windows_core::Result<()> {
        Err(windows_core::Error::from_hresult(E_NOTIMPL))
    }

    fn Stat(&self, pstatstg: *mut STATSTG, _grfstatflag: &STATFLAG) -> windows_core::Result<()> {
        self.run_guarded(|inner| {
            if pstatstg.is_null() {
                return Err(windows_core::Error::from_hresult(STG_E_INVALIDPOINTER));
            }
            // Zero-initialize all fields (`pwcsName` null) so COM RPC marshaling never frees an
            // uninitialized pointer regardless of `STATFLAG_NONAME`.
            let stat = STATSTG {
                r#type: STGTY_STREAM.0.cast_unsigned(),
                cbSize: inner.buf.len() as u64,
                ..Default::default()
            };
            // SAFETY: `pstatstg` was checked non-null.
            unsafe { *pstatstg = stat };
            Ok(())
        })
    }

    fn Clone(&self) -> windows_core::Result<IStream> {
        Err(windows_core::Error::from_hresult(E_NOTIMPL))
    }
}

// ============================================================================
// STA-affine `IWiaTransferCallback` COM object with `SinkLease` & `catch_unwind`
// ============================================================================

struct CallbackShared {
    owner_thread: ThreadId,
    lease_active: bool,
    /// Active stack borrow of `&mut dyn TransferSink` during `run_download_with_callback`, taken
    /// out while invoking a sink method (preventing re-entrant `&mut` aliasing) and cleared
    /// unconditionally by `SinkLease::drop` before `run_download_with_callback` returns.
    sink: Option<NonNull<dyn TransferSink>>,
    stream_cap: u64,
    current_stream: Option<Arc<Mutex<StreamInner>>>,
    callback_error_hr: Option<i32>,
    stream_overflowed: bool,
    callback_panic: Option<String>,
}

// SAFETY: `CallbackShared` may be dropped on another thread if an out-of-process COM stub releases
// `TransferCallbackCom` asynchronously after `Download` returns, by which time `SinkLease::drop`
// has set `lease_active = false` and `sink = None`. While `sink` is `Some`, it is dereferenced
// ONLY after verifying `std::thread::current().id() == owner_thread`.
unsafe impl Send for CallbackShared {}

struct SinkLease {
    shared: Arc<Mutex<CallbackShared>>,
}

impl Drop for SinkLease {
    fn drop(&mut self) {
        let mut guard = lock(&self.shared);
        guard.lease_active = false;
        guard.sink = None;
        if let Some(stream) = guard.current_stream.take() {
            let s = lock(&stream);
            if s.overflowed {
                guard.stream_overflowed = true;
            }
            if s.panic_msg.is_some() && guard.callback_panic.is_none() {
                guard.callback_panic = s.panic_msg.clone();
            }
        }
    }
}

struct RestoreSink<'a> {
    shared: &'a Mutex<CallbackShared>,
    sink: NonNull<dyn TransferSink>,
}

impl Drop for RestoreSink<'_> {
    fn drop(&mut self) {
        let mut guard = lock(self.shared);
        if guard.lease_active && guard.sink.is_none() {
            guard.sink = Some(self.sink);
        }
    }
}

/// Custom `#[repr(C)]` COM implementation of `IWiaTransferCallback`.
///
/// Intentionally implemented manually rather than via `#[implement(IWiaTransferCallback)]` for two
/// soundness reasons:
/// 1. `windows-core`'s `#[implement]` automatically answers `S_OK` to `IAgileObject`, whereas our
///    callback borrows `&mut dyn TransferSink` on the STA thread and must stay STA-affine so
///    `ole32` dispatches callbacks onto the STA modal loop rather than an RPC worker thread.
/// 2. `windows 0.62`'s generated `IWiaTransferCallback_Vtbl::GetNextStream` does not zero
///    `*ppDestination` when returning `Err(Error::from_hresult(S_FALSE))`, even though `S_FALSE`
///    (`1`) is a `SUCCEEDED` `HRESULT` whose `[out]` pointer COM stubs inspect.
#[repr(C)]
struct TransferCallbackCom {
    vtable: *const IWiaTransferCallback_Vtbl,
    ref_count: AtomicU32,
    shared: Arc<Mutex<CallbackShared>>,
}

impl TransferCallbackCom {
    const VTBL: IWiaTransferCallback_Vtbl = IWiaTransferCallback_Vtbl {
        base__: IUnknown_Vtbl {
            QueryInterface: Self::query_interface,
            AddRef: Self::add_ref,
            Release: Self::release,
        },
        TransferCallback: Self::transfer_callback,
        GetNextStream: Self::get_next_stream,
    };

    fn into_interface(shared: Arc<Mutex<CallbackShared>>) -> IWiaTransferCallback {
        let raw = Box::into_raw(Box::new(Self {
            vtable: &Self::VTBL,
            ref_count: AtomicU32::new(1),
            shared,
        }));
        // SAFETY: `TransferCallbackCom` is `#[repr(C)]` with `vtable: *const IWiaTransferCallback_Vtbl`
        // at offset 0 and initial COM reference count `1` owned by the returned `IWiaTransferCallback`.
        unsafe { IWiaTransferCallback::from_raw(raw.cast()) }
    }

    unsafe extern "system" fn query_interface(
        this: *mut core::ffi::c_void,
        riid: *const GUID,
        ppv: *mut *mut core::ffi::c_void,
    ) -> HRESULT {
        if ppv.is_null() {
            return E_POINTER;
        }
        // SAFETY: `ppv` was checked non-null; COM requires `*ppv = null` on failure too.
        unsafe { *ppv = core::ptr::null_mut() };
        if this.is_null() || riid.is_null() {
            return E_POINTER;
        }
        // SAFETY: `riid` was checked non-null.
        let iid = unsafe { *riid };
        if iid == IWiaTransferCallback::IID || iid == IUnknown::IID {
            // SAFETY: `this` is a live `TransferCallbackCom` pointer.
            let me = unsafe { &*this.cast::<Self>() };
            me.ref_count.fetch_add(1, Ordering::Relaxed);
            // SAFETY: `ppv` is non-null.
            unsafe { *ppv = this };
            S_OK
        } else {
            E_NOINTERFACE
        }
    }

    unsafe extern "system" fn add_ref(this: *mut core::ffi::c_void) -> u32 {
        // SAFETY: `this` is a live `TransferCallbackCom` pointer.
        let me = unsafe { &*this.cast::<Self>() };
        me.ref_count.fetch_add(1, Ordering::Relaxed) + 1
    }

    unsafe extern "system" fn release(this: *mut core::ffi::c_void) -> u32 {
        // SAFETY: `this` is a live `TransferCallbackCom` pointer with positive refcount.
        let prev = unsafe {
            (*this.cast::<Self>())
                .ref_count
                .fetch_sub(1, Ordering::Release)
        };
        if prev == 1 {
            core::sync::atomic::fence(Ordering::Acquire);
            // SAFETY: Last COM reference released; reclaim the heap allocation from `into_interface`.
            unsafe { drop(Box::from_raw(this.cast::<Self>())) };
            0
        } else {
            prev - 1
        }
    }

    fn run_guarded(&self, f: impl FnOnce(&Self) -> HRESULT) -> HRESULT {
        match catch_unwind(AssertUnwindSafe(|| f(self))) {
            Ok(hr) => hr,
            Err(payload) => {
                let mut guard = lock(&self.shared);
                if guard.callback_panic.is_none() {
                    guard.callback_panic = Some(panic_payload_string(payload));
                }
                E_FAIL
            }
        }
    }

    fn with_sink(
        &self,
        f: impl FnOnce(&mut dyn TransferSink) -> TransferAction,
    ) -> Result<TransferAction, HRESULT> {
        let mut sink_ptr = {
            let mut guard = lock(&self.shared);
            if std::thread::current().id() != guard.owner_thread {
                return Err(RPC_E_WRONG_THREAD);
            }
            let Some(ptr) = guard.sink.take() else {
                return Err(E_ABORT);
            };
            ptr
        };
        let _restore = RestoreSink {
            shared: &self.shared,
            sink: sink_ptr,
        };
        // SAFETY: Verified `current().id() == owner_thread`, `SinkLease` is active on this thread's
        // stack (`guard.sink` was `Some`), and `guard.sink.take()` guarantees exclusive access even
        // if `f` triggers re-entrant COM work.
        Ok(f(unsafe { sink_ptr.as_mut() }))
    }

    fn harvest_stream_flags(state: &mut CallbackShared, stream: &Arc<Mutex<StreamInner>>) {
        let s = lock(stream);
        if s.overflowed {
            state.stream_overflowed = true;
        }
        if s.panic_msg.is_some() && state.callback_panic.is_none() {
            state.callback_panic = s.panic_msg.clone();
        }
    }

    fn finalize_current_stream(&self, require_non_empty: bool) -> HRESULT {
        let bytes = {
            let mut state = lock(&self.shared);
            if std::thread::current().id() != state.owner_thread {
                return RPC_E_WRONG_THREAD;
            }
            let Some(stream_arc) = state.current_stream.take() else {
                return S_OK;
            };
            let (bytes, overflowed, already_finalized, stream_panic) = {
                let mut s = lock(&stream_arc);
                let already = s.finalized;
                s.finalized = true;
                let taken = if !already && !s.overflowed && s.panic_msg.is_none() {
                    Some(core::mem::take(&mut s.buf))
                } else {
                    None
                };
                (taken, s.overflowed, already, s.panic_msg.clone())
            };
            if overflowed {
                state.stream_overflowed = true;
            }
            if let Some(msg) = stream_panic {
                if state.callback_panic.is_none() {
                    state.callback_panic = Some(msg);
                }
                return E_FAIL;
            }
            if already_finalized || overflowed || state.callback_error_hr.is_some() {
                return S_OK;
            }
            let Some(bytes) = bytes else {
                return S_OK;
            };
            if require_non_empty && bytes.is_empty() {
                return S_OK;
            }
            bytes
        };

        match self.with_sink(|sink| sink.on_stream_complete(bytes)) {
            Ok(TransferAction::Continue) => S_OK,
            Ok(TransferAction::Cancel) => S_FALSE,
            Err(hr) => hr,
        }
    }

    unsafe extern "system" fn transfer_callback(
        this: *mut core::ffi::c_void,
        _lflags: i32,
        pwiatransferparams: *const WiaTransferParams,
    ) -> HRESULT {
        if this.is_null() || pwiatransferparams.is_null() {
            return E_POINTER;
        }
        // SAFETY: `this` is a live `TransferCallbackCom` pointer.
        let me = unsafe { &*this.cast::<Self>() };
        me.run_guarded(|me| {
            // SAFETY: `pwiatransferparams` was checked non-null above.
            let params = unsafe { &*pwiatransferparams };
            let msg = params.lMessage;
            let hr_status = params.hrErrorStatus.0;
            let transferred = params.ulTransferredBytes;

            {
                let mut state = lock(&me.shared);
                if std::thread::current().id() != state.owner_thread {
                    return RPC_E_WRONG_THREAD;
                }
                if let Some(stream) = state.current_stream.clone() {
                    Self::harvest_stream_flags(&mut state, &stream);
                }
                if hr_status < 0 && state.callback_error_hr.is_none() {
                    state.callback_error_hr = Some(hr_status);
                }
            }

            match msg {
                WIA_TRANSFER_MSG_STATUS
                | WIA_TRANSFER_MSG_DEVICE_STATUS
                | WIA_TRANSFER_MSG_NEW_PAGE => {
                    match me.with_sink(|sink| sink.on_status(msg, transferred, hr_status)) {
                        Ok(TransferAction::Continue) => S_OK,
                        Ok(TransferAction::Cancel) => S_FALSE,
                        Err(hr) => hr,
                    }
                }
                WIA_TRANSFER_MSG_END_OF_STREAM => me.finalize_current_stream(false),
                WIA_TRANSFER_MSG_END_OF_TRANSFER => {
                    // Finalize any non-empty error-free stream if a driver skipped END_OF_STREAM.
                    me.finalize_current_stream(true)
                }
                _ => S_OK,
            }
        })
    }

    unsafe extern "system" fn get_next_stream(
        this: *mut core::ffi::c_void,
        _lflags: i32,
        _bstritemname: *mut core::ffi::c_void,
        _bstrfullitemname: *mut core::ffi::c_void,
        ppdestination: *mut *mut core::ffi::c_void,
    ) -> HRESULT {
        if ppdestination.is_null() {
            return E_POINTER;
        }
        // SAFETY: `ppdestination` was checked non-null; unconditionally zero `*ppdestination`
        // upfront so `S_FALSE` (`SUCCEEDED`) and error paths never leave garbage in the `[out]`
        // slot.
        unsafe { *ppdestination = core::ptr::null_mut() };
        if this.is_null() {
            return E_POINTER;
        }
        // SAFETY: `this` is a live `TransferCallbackCom` pointer.
        let me = unsafe { &*this.cast::<Self>() };
        me.run_guarded(|me| {
            let cap = {
                let mut state = lock(&me.shared);
                if std::thread::current().id() != state.owner_thread {
                    return RPC_E_WRONG_THREAD;
                }
                if let Some(prev) = state.current_stream.take() {
                    Self::harvest_stream_flags(&mut state, &prev);
                }
                state.stream_cap
            };

            match me.with_sink(|sink| sink.on_next_stream()) {
                Ok(TransferAction::Cancel) => S_FALSE,
                Err(hr) => hr,
                Ok(TransferAction::Continue) => {
                    let (stream_inner, istream) = BoundedStream::create(cap);
                    lock(&me.shared).current_stream = Some(stream_inner);
                    // SAFETY: `ppdestination` is non-null and transfers ownership of `istream`'s
                    // initial COM reference count to the caller.
                    unsafe { *ppdestination = istream.into_raw() };
                    S_OK
                }
            }
        })
    }
}

fn run_download_with_callback(
    transfer: &IWiaTransfer,
    stream_cap: u64,
    sink: &mut dyn TransferSink,
) -> DownloadOutcome {
    // Erase lifetime of `sink` strictly for the duration of `SinkLease` in this stack frame.
    // SAFETY: `SinkLease` clears `shared.sink = None` and `lease_active = false` in `Drop` before
    // this function returns (even on unwind), and `with_sink` checks `owner_thread`, so the erased
    // pointer never outlives `sink` or crosses threads.
    let raw_sink: NonNull<dyn TransferSink> = unsafe {
        core::mem::transmute::<NonNull<dyn TransferSink + '_>, NonNull<dyn TransferSink + 'static>>(
            NonNull::from(&mut *sink),
        )
    };
    let shared = Arc::new(Mutex::new(CallbackShared {
        owner_thread: std::thread::current().id(),
        lease_active: true,
        sink: Some(raw_sink),
        stream_cap,
        current_stream: None,
        callback_error_hr: None,
        stream_overflowed: false,
        callback_panic: None,
    }));
    let lease = SinkLease {
        shared: Arc::clone(&shared),
    };
    let cb = TransferCallbackCom::into_interface(Arc::clone(&shared));

    // Call the raw COM vtable slot directly: `windows 0.62`'s high-level `transfer.Download(...)`
    // calls `HRESULT::ok()`, collapsing `S_OK` (`0`), `S_FALSE` (`1`), `WIA_STATUS_END_OF_MEDIA`
    // (`0x00210001`), and `WIA_STATUS_WARMING_UP` (`0x00210002`) into `Ok(())`.
    // SAFETY: `transfer` and `cb` are live COM interfaces on the current STA thread.
    let hr: HRESULT = unsafe {
        (Interface::vtable(transfer).Download)(
            Interface::as_raw(transfer),
            0,
            Interface::as_raw(&cb),
        )
    };

    drop(cb);
    drop(lease);

    let guard = lock(&shared);
    DownloadOutcome {
        hr: hr.0,
        callback_error_hr: guard.callback_error_hr,
        stream_overflowed: guard.stream_overflowed,
        callback_panic: guard.callback_panic.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::System::Com::STATFLAG_NONAME;

    #[test]
    fn propvariant_round_trips_i4_bstr_clsid_and_attributes() {
        let i4_pv = encode_prop_value(&PropValue::I4(-42)).unwrap();
        assert_eq!(decode_prop_value(&i4_pv), Some(PropValue::I4(-42)));

        let str_pv = encode_prop_value(&PropValue::Str("Desk Scanner".to_owned())).unwrap();
        assert_eq!(
            decode_prop_value(&str_pv),
            Some(PropValue::Str("Desk Scanner".to_owned()))
        );

        let guid_pv = encode_prop_value(&PropValue::Guid(WIA_CATEGORY_FEEDER_GUID)).unwrap();
        assert_eq!(
            decode_prop_value(&guid_pv),
            Some(PropValue::Guid(WIA_CATEGORY_FEEDER_GUID))
        );

        let empty = PROPVARIANT::default();
        assert_eq!(decode_prop_value(&empty), None);

        // Verify `WIA_PROP_LIST` skips `[WIA_LIST_COUNT = 0, WIA_LIST_NOM = 1]` header.
        let list_elems: [i32; 5] = [3, 300, 150, 300, 600];
        let mut raw_pv = PROPVARIANT::default();
        {
            // SAFETY: Constructing a temporary `VT_VECTOR | VT_I4` view over stack `list_elems`
            // and clearing `vt = VT_EMPTY` before `raw_pv` drops so `PropVariantClear` never frees
            // stack memory.
            let inner = unsafe { &mut raw_pv.Anonymous.Anonymous };
            inner.vt = VARENUM(VT_VECTOR.0 | VT_I4.0);
            inner.Anonymous.cal.cElems = list_elems.len() as u32;
            inner.Anonymous.cal.pElems = list_elems.as_ptr().cast_mut();
        }
        let attr = decode_prop_attr(super::super::driver::WIA_PROP_RW | WIA_PROP_LIST, &raw_pv);
        {
            // SAFETY: Reset `vt` back to `VT_EMPTY` before `raw_pv` drops.
            unsafe { (*raw_pv.Anonymous.Anonymous).vt = VT_EMPTY };
        }
        assert!(attr.is_writable());
        assert_eq!(
            attr.constraint,
            PropConstraint::ListI4 {
                nom: 300,
                values: vec![150, 300, 600],
            }
        );
    }

    #[test]
    fn bounded_stream_supports_backpatching_null_ptrs_stat_and_enforces_cap() {
        let (inner, stream) = BoundedStream::create(16);

        // Zero-length Read/Write with null buffer and null byte-count pointers succeed cleanly.
        // SAFETY: Testing COM `IStream` vtable contract with zero-length / null optional pointers.
        unsafe {
            assert_eq!(
                (Interface::vtable(&stream).base__.Write)(
                    Interface::as_raw(&stream),
                    core::ptr::null(),
                    0,
                    core::ptr::null_mut(),
                ),
                S_OK
            );
            assert_eq!(
                (Interface::vtable(&stream).base__.Read)(
                    Interface::as_raw(&stream),
                    core::ptr::null_mut(),
                    0,
                    core::ptr::null_mut(),
                ),
                S_OK
            );
            // Non-zero Write with null buffer returns STG_E_INVALIDPOINTER.
            assert_eq!(
                (Interface::vtable(&stream).base__.Write)(
                    Interface::as_raw(&stream),
                    core::ptr::null(),
                    4,
                    core::ptr::null_mut(),
                ),
                STG_E_INVALIDPOINTER
            );
        }

        // Write 10 bytes, seek back to byte 2, back-patch 4 bytes (like a BMP header update),
        // verify `Stat` reports 10 bytes and `pwcsName` is null.
        let initial = *b"BM0000data";
        let mut written = 0u32;
        // SAFETY: Valid buffer of 10 bytes.
        unsafe {
            assert_eq!(
                (Interface::vtable(&stream).base__.Write)(
                    Interface::as_raw(&stream),
                    initial.as_ptr().cast(),
                    initial.len() as u32,
                    &mut written,
                ),
                S_OK
            );
            assert_eq!(written, 10);

            let mut new_pos = 99u64;
            assert_eq!(
                (Interface::vtable(&stream).Seek)(
                    Interface::as_raw(&stream),
                    2,
                    STREAM_SEEK_SET,
                    &mut new_pos,
                ),
                S_OK
            );
            assert_eq!(new_pos, 2);

            let patch = 10u32.to_le_bytes();
            assert_eq!(
                (Interface::vtable(&stream).base__.Write)(
                    Interface::as_raw(&stream),
                    patch.as_ptr().cast(),
                    4,
                    core::ptr::null_mut(),
                ),
                S_OK
            );

            let mut stat = core::mem::MaybeUninit::<STATSTG>::new(core::mem::zeroed());
            // Fill with non-zero bytes first to prove `Stat` overwrites `pwcsName` with null!
            core::ptr::write_bytes(stat.as_mut_ptr(), 0xAA, 1);
            assert_eq!(
                (Interface::vtable(&stream).Stat)(
                    Interface::as_raw(&stream),
                    stat.as_mut_ptr(),
                    STATFLAG_NONAME.0.cast_unsigned(),
                ),
                S_OK
            );
            let stat = stat.assume_init();
            assert_eq!(stat.cbSize, 10);
            assert!(stat.pwcsName.is_null());

            // Negative seek before 0 fails with STG_E_INVALIDFUNCTION.
            assert_eq!(
                (Interface::vtable(&stream).Seek)(
                    Interface::as_raw(&stream),
                    -20,
                    STREAM_SEEK_CUR,
                    core::ptr::null_mut(),
                ),
                STG_E_INVALIDFUNCTION
            );
        }
        assert_eq!(&lock(&inner).buf, b"BM\x0a\x00\x00\x00data");
        assert!(!lock(&inner).overflowed);

        // Seeking to end and writing 7 more bytes (10 + 7 = 17 > 16 cap) returns STG_E_MEDIUMFULL
        // and latches `overflowed = true`.
        let extra = [1u8; 7];
        // SAFETY: Valid 7-byte slice and seek to end.
        unsafe {
            assert_eq!(
                (Interface::vtable(&stream).Seek)(
                    Interface::as_raw(&stream),
                    0,
                    STREAM_SEEK_END,
                    core::ptr::null_mut(),
                ),
                S_OK
            );
            assert_eq!(
                (Interface::vtable(&stream).base__.Write)(
                    Interface::as_raw(&stream),
                    extra.as_ptr().cast(),
                    extra.len() as u32,
                    core::ptr::null_mut(),
                ),
                STG_E_MEDIUMFULL
            );
        }
        assert!(lock(&inner).overflowed);
    }

    struct RecordingSink {
        cancel_on_next: bool,
        panic_on_status: bool,
        pages: Vec<Vec<u8>>,
        statuses: Vec<(i32, u64, i32)>,
    }

    impl TransferSink for RecordingSink {
        fn on_next_stream(&mut self) -> TransferAction {
            if self.cancel_on_next {
                TransferAction::Cancel
            } else {
                TransferAction::Continue
            }
        }

        fn on_status(
            &mut self,
            msg: i32,
            transferred_bytes: u64,
            hr_status: i32,
        ) -> TransferAction {
            if self.panic_on_status {
                panic!("boom inside sink");
            }
            self.statuses.push((msg, transferred_bytes, hr_status));
            TransferAction::Continue
        }

        fn on_stream_complete(&mut self, bytes: Vec<u8>) -> TransferAction {
            self.pages.push(bytes);
            TransferAction::Continue
        }
    }

    #[implement(IWiaTransfer)]
    struct MockComTransfer {
        mode: u8,
    }

    impl windows::Win32::Devices::ImageAcquisition::IWiaTransfer_Impl for MockComTransfer_Impl {
        fn Download(
            &self,
            _lflags: i32,
            pwiatransfercallback: Ref<'_, IWiaTransferCallback>,
        ) -> windows_core::Result<()> {
            let cb = pwiatransfercallback.ok()?;
            match self.mode {
                // Mode 0: normal 1-page transfer + END_OF_MEDIA status return, plus STA-affinity
                // (`E_NOINTERFACE` for `IAgileObject`, `RPC_E_WRONG_THREAD` off-owner-thread).
                0 => {
                    let agile_iid = GUID::from_u128(0x94ea2b94_e9cc_49e0_c0ff_ee64ca8f5b90);
                    let mut agile_out: *mut core::ffi::c_void = core::ptr::dangling_mut();
                    // SAFETY: Testing raw `QueryInterface` rejects `IAgileObject` and zeroes `ppv`.
                    unsafe {
                        let qi_hr = (Interface::vtable(cb).base__.QueryInterface)(
                            Interface::as_raw(cb),
                            &agile_iid,
                            &mut agile_out,
                        );
                        assert_eq!(qi_hr, E_NOINTERFACE);
                        assert!(agile_out.is_null());
                    }

                    // Cross-thread call must return `RPC_E_WRONG_THREAD` without touching `sink`.
                    let raw_cb_addr = Interface::as_raw(cb) as usize;
                    let off_thread_hr = std::thread::spawn(move || {
                        let raw_cb = raw_cb_addr as *mut core::ffi::c_void;
                        let params = WiaTransferParams {
                            lMessage: WIA_TRANSFER_MSG_STATUS,
                            lPercentComplete: 1,
                            ulTransferredBytes: 1,
                            hrErrorStatus: S_OK,
                        };
                        // SAFETY: `cb` is kept alive by the parent thread waiting on `.join()`.
                        unsafe {
                            let vtbl = *(raw_cb as *const *const IWiaTransferCallback_Vtbl);
                            ((*vtbl).TransferCallback)(raw_cb, 0, &params)
                        }
                    })
                    .join()
                    .unwrap();
                    assert_eq!(off_thread_hr, RPC_E_WRONG_THREAD);

                    let name = BSTR::from("Flatbed");
                    let full = BSTR::from("0000\\Root\\Flatbed");
                    let mut raw_stream: *mut core::ffi::c_void = core::ptr::null_mut();
                    // SAFETY: Invoking `GetNextStream` and `TransferCallback` through raw COM
                    // vtable slots just as `stisvc` / WIA minidrivers do.
                    unsafe {
                        let hr = (Interface::vtable(cb).GetNextStream)(
                            Interface::as_raw(cb),
                            0,
                            core::mem::transmute_copy(&name),
                            core::mem::transmute_copy(&full),
                            &mut raw_stream,
                        );
                        assert_eq!(hr, S_OK);
                        assert!(!raw_stream.is_null());
                        let st = IStream::from_raw(raw_stream);
                        let payload = b"hello-wia";
                        assert_eq!(
                            (Interface::vtable(&st).base__.Write)(
                                Interface::as_raw(&st),
                                payload.as_ptr().cast(),
                                payload.len() as u32,
                                core::ptr::null_mut(),
                            ),
                            S_OK
                        );
                        let status = WiaTransferParams {
                            lMessage: WIA_TRANSFER_MSG_STATUS,
                            lPercentComplete: 50,
                            ulTransferredBytes: payload.len() as u64,
                            hrErrorStatus: S_OK,
                        };
                        assert_eq!(
                            (Interface::vtable(cb).TransferCallback)(
                                Interface::as_raw(cb),
                                0,
                                &status,
                            ),
                            S_OK
                        );
                        // Deliberately skip END_OF_STREAM and send END_OF_TRANSFER directly to
                        // prove unclosed non-empty error-free streams are finalized cleanly!
                        let eot = WiaTransferParams {
                            lMessage: WIA_TRANSFER_MSG_END_OF_TRANSFER,
                            lPercentComplete: 100,
                            ulTransferredBytes: payload.len() as u64,
                            hrErrorStatus: S_OK,
                        };
                        assert_eq!(
                            (Interface::vtable(cb).TransferCallback)(
                                Interface::as_raw(cb),
                                0,
                                &eot,
                            ),
                            S_OK
                        );
                    }
                    Err(windows_core::Error::from_hresult(HRESULT(
                        error::WIA_STATUS_END_OF_MEDIA_HR,
                    )))
                }
                // Mode 1: verify null `ppDestination` -> `E_POINTER`, `GetNextStream` cancel
                // returns `S_FALSE` (`HRESULT(1)`) AND zeroes a pre-filled non-null
                // `*ppDestination`, and panic containment returns `E_FAIL`.
                1 => {
                    let name = BSTR::from("Feeder");
                    let full = BSTR::from("0000\\Root\\Feeder");
                    let mut raw_stream: *mut core::ffi::c_void = core::ptr::dangling_mut();
                    // SAFETY: Testing raw COM vtable null guard, `S_FALSE` zeroing, and `catch_unwind`.
                    unsafe {
                        let null_hr = (Interface::vtable(cb).GetNextStream)(
                            Interface::as_raw(cb),
                            0,
                            core::mem::transmute_copy(&name),
                            core::mem::transmute_copy(&full),
                            core::ptr::null_mut(),
                        );
                        assert_eq!(null_hr, E_POINTER);

                        let hr = (Interface::vtable(cb).GetNextStream)(
                            Interface::as_raw(cb),
                            0,
                            core::mem::transmute_copy(&name),
                            core::mem::transmute_copy(&full),
                            &mut raw_stream,
                        );
                        assert_eq!(hr, S_FALSE);
                        assert!(raw_stream.is_null());

                        let jam_status = WiaTransferParams {
                            lMessage: WIA_TRANSFER_MSG_STATUS,
                            lPercentComplete: 0,
                            ulTransferredBytes: 0,
                            hrErrorStatus: HRESULT(error::WIA_ERROR_PAPER_JAM_HR),
                        };
                        let panic_hr = (Interface::vtable(cb).TransferCallback)(
                            Interface::as_raw(cb),
                            0,
                            &jam_status,
                        );
                        assert_eq!(panic_hr, E_FAIL);
                    }
                    Err(windows_core::Error::from_hresult(E_FAIL))
                }
                // Mode 2: Stream 1 writes placeholder BMP header, streams pixel row, seeks back
                // to byte 2 and byte 10 to back-patch `bfSize` and `bfOffBits`, then sends
                // `END_OF_STREAM`; Stream 2 writes past `stream_cap`, gets `STG_E_MEDIUMFULL`,
                // and returns generic `E_FAIL`.
                _ => {
                    let name = BSTR::from("Feeder");
                    let full = BSTR::from("0000\\Root\\Feeder");
                    let mut raw_s1: *mut core::ffi::c_void = core::ptr::null_mut();
                    // SAFETY: Exercising `GetNextStream`, `IStream::Write`/`Seek`, and `END_OF_STREAM`.
                    unsafe {
                        assert_eq!(
                            (Interface::vtable(cb).GetNextStream)(
                                Interface::as_raw(cb),
                                0,
                                core::mem::transmute_copy(&name),
                                core::mem::transmute_copy(&full),
                                &mut raw_s1,
                            ),
                            S_OK
                        );
                        let st1 = IStream::from_raw(raw_s1);
                        // Write 14-byte placeholder BITMAPFILEHEADER (`bfSize = 0`, `bfOffBits = 0`).
                        let mut bmp = Vec::with_capacity(58);
                        bmp.extend_from_slice(b"BM");
                        bmp.extend_from_slice(&[0u8; 12]);
                        bmp.extend_from_slice(&40u32.to_le_bytes());
                        bmp.extend_from_slice(&1i32.to_le_bytes());
                        bmp.extend_from_slice(&1i32.to_le_bytes());
                        bmp.extend_from_slice(&1u16.to_le_bytes());
                        bmp.extend_from_slice(&24u16.to_le_bytes());
                        bmp.extend_from_slice(&0u32.to_le_bytes());
                        bmp.extend_from_slice(&4u32.to_le_bytes());
                        bmp.extend_from_slice(&[0u8; 16]);
                        bmp.extend_from_slice(&[0xCC, 0xBB, 0xAA, 0x00]); // BGR -> RGB (0xAA, 0xBB, 0xCC)

                        assert_eq!(
                            (Interface::vtable(&st1).base__.Write)(
                                Interface::as_raw(&st1),
                                bmp.as_ptr().cast(),
                                bmp.len() as u32,
                                core::ptr::null_mut(),
                            ),
                            S_OK
                        );
                        // Seek back to byte 2 to patch `bfSize = 58`, and byte 10 to patch `bfOffBits = 54`.
                        assert_eq!(
                            (Interface::vtable(&st1).Seek)(
                                Interface::as_raw(&st1),
                                2,
                                STREAM_SEEK_SET,
                                core::ptr::null_mut(),
                            ),
                            S_OK
                        );
                        let bf_size = 58u32.to_le_bytes();
                        assert_eq!(
                            (Interface::vtable(&st1).base__.Write)(
                                Interface::as_raw(&st1),
                                bf_size.as_ptr().cast(),
                                4,
                                core::ptr::null_mut(),
                            ),
                            S_OK
                        );
                        assert_eq!(
                            (Interface::vtable(&st1).Seek)(
                                Interface::as_raw(&st1),
                                10,
                                STREAM_SEEK_SET,
                                core::ptr::null_mut(),
                            ),
                            S_OK
                        );
                        let bf_off = 54u32.to_le_bytes();
                        assert_eq!(
                            (Interface::vtable(&st1).base__.Write)(
                                Interface::as_raw(&st1),
                                bf_off.as_ptr().cast(),
                                4,
                                core::ptr::null_mut(),
                            ),
                            S_OK
                        );
                        let eos = WiaTransferParams {
                            lMessage: WIA_TRANSFER_MSG_END_OF_STREAM,
                            lPercentComplete: 100,
                            ulTransferredBytes: 58,
                            hrErrorStatus: S_OK,
                        };
                        assert_eq!(
                            (Interface::vtable(cb).TransferCallback)(
                                Interface::as_raw(cb),
                                0,
                                &eos,
                            ),
                            S_OK
                        );

                        // Stream 2: exceed 64-byte cap -> STG_E_MEDIUMFULL!
                        let mut raw_s2: *mut core::ffi::c_void = core::ptr::null_mut();
                        assert_eq!(
                            (Interface::vtable(cb).GetNextStream)(
                                Interface::as_raw(cb),
                                0,
                                core::mem::transmute_copy(&name),
                                core::mem::transmute_copy(&full),
                                &mut raw_s2,
                            ),
                            S_OK
                        );
                        let st2 = IStream::from_raw(raw_s2);
                        let oversized = [0x55u8; 80];
                        assert_eq!(
                            (Interface::vtable(&st2).base__.Write)(
                                Interface::as_raw(&st2),
                                oversized.as_ptr().cast(),
                                oversized.len() as u32,
                                core::ptr::null_mut(),
                            ),
                            STG_E_MEDIUMFULL
                        );
                    }
                    Err(windows_core::Error::from_hresult(E_FAIL))
                }
            }
        }

        fn Upload(
            &self,
            _lflags: i32,
            _pstream: Ref<'_, IStream>,
            _pwiatransfercallback: Ref<'_, IWiaTransferCallback>,
        ) -> windows_core::Result<()> {
            Err(windows_core::Error::from_hresult(E_NOTIMPL))
        }

        fn Cancel(&self) -> windows_core::Result<()> {
            Err(windows_core::Error::from_hresult(E_NOTIMPL))
        }

        fn EnumWIA_FORMAT_INFO(&self) -> windows_core::Result<IEnumWIA_FORMAT_INFO> {
            Err(windows_core::Error::from_hresult(E_NOTIMPL))
        }
    }

    #[test]
    fn transfer_callback_preserves_end_of_media_s_false_cancel_error_latch_and_catch_unwind() {
        let transfer_ok: IWiaTransfer = MockComTransfer { mode: 0 }.into();
        let mut sink = RecordingSink {
            cancel_on_next: false,
            panic_on_status: false,
            pages: Vec::new(),
            statuses: Vec::new(),
        };
        let out = run_download_with_callback(&transfer_ok, 1024, &mut sink);
        assert_eq!(out.hr, error::WIA_STATUS_END_OF_MEDIA_HR);
        assert_eq!(out.callback_error_hr, None);
        assert!(!out.stream_overflowed);
        assert_eq!(out.callback_panic, None);
        assert_eq!(sink.pages, vec![b"hello-wia".to_vec()]);
        assert_eq!(sink.statuses, vec![(WIA_TRANSFER_MSG_STATUS, 9, 0)]);

        let transfer_cancel_panic: IWiaTransfer = MockComTransfer { mode: 1 }.into();
        let mut sink2 = RecordingSink {
            cancel_on_next: true,
            panic_on_status: true,
            pages: Vec::new(),
            statuses: Vec::new(),
        };
        let out2 = run_download_with_callback(&transfer_cancel_panic, 1024, &mut sink2);
        assert_eq!(out2.hr, E_FAIL.0);
        // Negative `hrErrorStatus` (`WIA_ERROR_PAPER_JAM`) was latched before the callback panic!
        assert_eq!(out2.callback_error_hr, Some(error::WIA_ERROR_PAPER_JAM_HR));
        assert_eq!(out2.callback_panic.as_deref(), Some("boom inside sink"));
        assert!(sink2.pages.is_empty());

        // Mode 2: back-patched `.bmp` stream decodes cleanly via `pixels::decode_stream`, while
        // the second stream exceeding the 64-byte cap sets `stream_overflowed = true` and is never
        // delivered to `on_stream_complete`.
        let transfer_bmp_overflow: IWiaTransfer = MockComTransfer { mode: 2 }.into();
        let mut sink3 = RecordingSink {
            cancel_on_next: false,
            panic_on_status: false,
            pages: Vec::new(),
            statuses: Vec::new(),
        };
        let out3 = run_download_with_callback(&transfer_bmp_overflow, 64, &mut sink3);
        assert_eq!(out3.hr, E_FAIL.0);
        assert!(out3.stream_overflowed);
        assert_eq!(sink3.pages.len(), 1);
        let (decoded, dpi) = super::super::pixels::decode_stream(
            sink3.pages.remove(0),
            super::super::settings::TransferMode::RawBmp,
            (300.0, 300.0),
        )
        .unwrap();
        assert_eq!(dpi, (300.0, 300.0));
        let crate::Pixels::Raw(img) = decoded else {
            panic!("expected RawImage");
        };
        assert_eq!(img.data(), &[0xAA, 0xBB, 0xCC]);
    }

    #[test]
    fn live_sta_com_guard_and_wia_dev_mgr_tear_down_cleanly() {
        // Spawn a fresh STA thread via `run_sta` and exercise live `ComWiaSystem::list_devices`
        // and `open_device` on a nonexistent ID (`0x80210015` -> `ScanError::Gone`, or
        // `NotInstalled` if `stisvc` is disabled on a stripped container).
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        let handle = std::thread::spawn(move || {
            run_sta(tx, |sys| {
                let list_res = sys.list_devices();
                match &list_res {
                    Ok(_) | Err(ScanError::NotInstalled { .. }) => {}
                    Err(other) => panic!("unexpected live WIA list error: {other:?}"),
                }
                if list_res.is_ok() {
                    let missing = sys.open_device(r"{6BDD1FC6-810F-11D0-BEC7-08002BE2092F}\9999");
                    assert_eq!(missing.err(), Some(ScanError::Gone));
                }
            });
        });
        assert_eq!(rx.recv().unwrap(), Ok(()));
        handle
            .join()
            .expect("STA thread joined cleanly without 0xC0000005");
    }
}
