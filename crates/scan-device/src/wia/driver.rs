//! Platform-independent WIA 2.0 constants, property types, transfer sink, internal driver traits
//! (`WiaSystem`, `WiaDeviceTree`), and scripted fake WIA implementation for cross-platform tests.
//!
//! Pure safe Rust: compiled on Linux, macOS, and Windows with zero `windows` crate types.

#![allow(missing_docs)]

#[cfg(any(test, feature = "fake"))]
use std::collections::{BTreeMap, VecDeque};
#[cfg(any(test, feature = "fake"))]
use std::sync::atomic::{AtomicUsize, Ordering};
#[cfg(any(test, feature = "fake"))]
use std::sync::{Arc, Mutex, PoisonError};
#[cfg(any(test, feature = "fake"))]
use std::time::Duration;

use super::id::RawDeviceProps;
use crate::ScanError;

/// WIA property identifier (`PROPID`).
pub type PropId = u32;

// Device information properties (`WIA_DIP_*` and `WIA_WSD_*`).
pub const WIA_DIP_DEV_ID: PropId = 2;
pub const WIA_DIP_VEND_DESC: PropId = 3;
pub const WIA_DIP_DEV_DESC: PropId = 4;
pub const WIA_DIP_DEV_TYPE: PropId = 5;
pub const WIA_DIP_DEV_NAME: PropId = 7;
pub const WIA_WSD_MANUFACTURER: PropId = 38914;
pub const WIA_WSD_MODEL_NAME: PropId = 38916;
pub const WIA_WSD_FRIENDLY_NAME: PropId = 38920;

// Scanner root & item properties (`WIA_DPS_*`, `WIA_IPA_*`, `WIA_IPS_*`).
pub const WIA_DPS_HORIZONTAL_BED_SIZE: PropId = 3074;
pub const WIA_DPS_VERTICAL_BED_SIZE: PropId = 3075;
pub const WIA_DPS_HORIZONTAL_SHEET_FEED_SIZE: PropId = 3076;
pub const WIA_DPS_VERTICAL_SHEET_FEED_SIZE: PropId = 3077;
pub const WIA_DPS_DOCUMENT_HANDLING_CAPABILITIES: PropId = 3086;
pub const WIA_DPS_DOCUMENT_HANDLING_STATUS: PropId = 3087;
pub const WIA_IPS_DOCUMENT_HANDLING_SELECT: PropId = 3088;
pub const WIA_IPS_OPTICAL_XRES: PropId = 3090;
pub const WIA_IPS_OPTICAL_YRES: PropId = 3091;
pub const WIA_IPS_PAGES: PropId = 3096;
pub const WIA_IPS_PAGE_SIZE: PropId = 3097;
pub const WIA_IPS_PAGE_WIDTH: PropId = 3098;
pub const WIA_IPS_PAGE_HEIGHT: PropId = 3099;
pub const WIA_IPS_PREVIEW: PropId = 3100;
pub const WIA_IPS_AUTO_DESKEW: PropId = 3107;

pub const WIA_IPA_ITEM_NAME: PropId = 4098;
pub const WIA_IPA_FULL_ITEM_NAME: PropId = 4099;
pub const WIA_IPA_ITEM_FLAGS: PropId = 4101;
pub const WIA_IPA_DATATYPE: PropId = 4103;
pub const WIA_IPA_DEPTH: PropId = 4104;
pub const WIA_IPA_FORMAT: PropId = 4106;
pub const WIA_IPA_COMPRESSION: PropId = 4107;
pub const WIA_IPA_TYMED: PropId = 4108;
pub const WIA_IPA_CHANNELS_PER_PIXEL: PropId = 4109;
pub const WIA_IPA_BITS_PER_CHANNEL: PropId = 4110;
pub const WIA_IPA_PLANAR: PropId = 4111;
pub const WIA_IPA_PIXELS_PER_LINE: PropId = 4112;
pub const WIA_IPA_BYTES_PER_LINE: PropId = 4113;
pub const WIA_IPA_NUMBER_OF_LINES: PropId = 4114;
pub const WIA_IPA_ITEM_SIZE: PropId = 4116;
pub const WIA_IPA_ITEM_CATEGORY: PropId = 4125;

pub const WIA_IPS_BLANK_PAGES: PropId = 4167;
pub const WIA_IPS_MULTI_FEED: PropId = 4168;
pub const WIA_IPS_AUTO_CROP: PropId = 4170;
pub const WIA_IPS_CUR_INTENT: PropId = 6146;
pub const WIA_IPS_XRES: PropId = 6147;
pub const WIA_IPS_YRES: PropId = 6148;
pub const WIA_IPS_XPOS: PropId = 6149;
pub const WIA_IPS_YPOS: PropId = 6150;
pub const WIA_IPS_XEXTENT: PropId = 6151;
pub const WIA_IPS_YEXTENT: PropId = 6152;
pub const WIA_IPS_PHOTOMETRIC_INTERP: PropId = 6153;
pub const WIA_IPS_ORIENTATION: PropId = 6156;
pub const WIA_IPS_ROTATION: PropId = 6157;
pub const WIA_IPS_MAX_HORIZONTAL_SIZE: PropId = 6165;
pub const WIA_IPS_MAX_VERTICAL_SIZE: PropId = 6166;
pub const WIA_IPS_MIN_HORIZONTAL_SIZE: PropId = 6167;
pub const WIA_IPS_MIN_VERTICAL_SIZE: PropId = 6168;
pub const WIA_IPS_TRANSFER_CAPABILITIES: PropId = 6169;

// `EnumDeviceInfo` flags.
pub const WIA_DEVINFO_ENUM_LOCAL: i32 = 0x10;

// `IWiaItem2::GetItemType` bitflags (`WiaDef.h`).
pub const WIA_ITEM_TYPE_IMAGE: i32 = 0x0000_0001;
pub const WIA_ITEM_TYPE_FILE: i32 = 0x0000_0002;
pub const WIA_ITEM_TYPE_FOLDER: i32 = 0x0000_0004;
pub const WIA_ITEM_TYPE_ROOT: i32 = 0x0000_0008;
pub const WIA_ITEM_TYPE_TRANSFER: i32 = 0x0000_2000;
pub const WIA_ITEM_TYPE_DOCUMENT: i32 = 0x0004_0000;
pub const WIA_ITEM_TYPE_PROGRAMMABLE: i32 = 0x0008_0000;

// `GetPropertyAttributes` access & constraint flags (`WiaDef.h`).
pub const WIA_PROP_READ: u32 = 0x01;
pub const WIA_PROP_WRITE: u32 = 0x02;
pub const WIA_PROP_RW: u32 = WIA_PROP_READ | WIA_PROP_WRITE;
pub const WIA_PROP_NONE: u32 = 0x08;
pub const WIA_PROP_RANGE: u32 = 0x10;
pub const WIA_PROP_LIST: u32 = 0x20;
pub const WIA_PROP_FLAG: u32 = 0x40;

// `WIA_IPA_TYMED` media constants (`objidl.h` + `WiaDef.h`).
pub const TYMED_FILE: i32 = 2;
pub const TYMED_CALLBACK: i32 = 128;
pub const TYMED_MULTIPAGE_FILE: i32 = 256;
pub const TYMED_MULTIPAGE_CALLBACK: i32 = 512;

// `WIA_IPA_DATATYPE` constants (`WiaDef.h`).
pub const WIA_DATA_THRESHOLD: i32 = 0;
pub const WIA_DATA_DITHER: i32 = 1;
pub const WIA_DATA_GRAYSCALE: i32 = 2;
pub const WIA_DATA_COLOR: i32 = 3;
pub const WIA_DATA_RAW_RGB: i32 = 6;
pub const WIA_DATA_RAW_BGR: i32 = 7;

// `WIA_DPS_DOCUMENT_HANDLING_CAPABILITIES` & `WIA_IPS_DOCUMENT_HANDLING_SELECT` flags.
pub const FEED: i32 = 0x001;
pub const FLAT: i32 = 0x002;
pub const DUP: i32 = 0x004;

pub const FEEDER: i32 = 0x001;
pub const FLATBED: i32 = 0x002;
pub const DUPLEX: i32 = 0x004;
pub const FRONT_FIRST: i32 = 0x008;
pub const BACK_FIRST: i32 = 0x010;
pub const FRONT_ONLY: i32 = 0x020;
pub const BACK_ONLY: i32 = 0x040;
pub const ADVANCED_DUPLEX: i32 = 0x400;

// Sticky / page / photometric constants (`WiaDef.h`).
pub const ALL_PAGES: i32 = 0;
pub const PORTRAIT: i32 = 0;
pub const WIA_FINAL_SCAN: i32 = 0;
pub const WIA_AUTO_DESKEW_ON: i32 = 0;
pub const WIA_AUTO_DESKEW_OFF: i32 = 1;
pub const WIA_BLANK_PAGE_DETECTION_DISABLED: i32 = 0;
pub const WIA_AUTO_CROP_DISABLED: i32 = 0;
pub const WIA_AUTO_CROP_SINGLE: i32 = 1;
pub const WIA_PAGE_A4: i32 = 0;
pub const WIA_PAGE_LETTER: i32 = 1;
pub const WIA_PAGE_CUSTOM: i32 = 2;
pub const WIA_PAGE_AUTO: i32 = 100;
pub const WIA_PHOTO_WHITE_1: i32 = 0;
pub const WIA_PHOTO_WHITE_0: i32 = 1;
pub const WIA_COMPRESSION_NONE: i32 = 0;
pub const WIA_PACKED_PIXEL: i32 = 0;
pub const WIA_LINE_ORDER_TOP_TO_BOTTOM: u32 = 1;
pub const WIA_LINE_ORDER_BOTTOM_TO_TOP: u32 = 2;

// `IWiaTransferCallback` message types (`WiaDef.h`).
pub const WIA_TRANSFER_MSG_STATUS: i32 = 1;
pub const WIA_TRANSFER_MSG_END_OF_STREAM: i32 = 2;
pub const WIA_TRANSFER_MSG_END_OF_TRANSFER: i32 = 3;
pub const WIA_TRANSFER_MSG_DEVICE_STATUS: i32 = 5;
pub const WIA_TRANSFER_MSG_NEW_PAGE: i32 = 6;

/// Platform-independent COM `GUID` value (`data1`, `data2`, `data3`, `data4`).
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct GuidBytes {
    pub d1: u32,
    pub d2: u16,
    pub d3: u16,
    pub d4: [u8; 8],
}

impl GuidBytes {
    pub const fn new(d1: u32, d2: u16, d3: u16, d4: [u8; 8]) -> Self {
        GuidBytes { d1, d2, d3, d4 }
    }
}

impl core::fmt::Debug for GuidBytes {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "{{{:08x}-{:04x}-{:04x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}}}",
            self.d1,
            self.d2,
            self.d3,
            self.d4[0],
            self.d4[1],
            self.d4[2],
            self.d4[3],
            self.d4[4],
            self.d4[5],
            self.d4[6],
            self.d4[7]
        )
    }
}

// `WIA_IPA_ITEM_CATEGORY` GUIDs (`WiaDef.h`).
pub const WIA_CATEGORY_FINISHED_FILE_GUID: GuidBytes = GuidBytes::new(
    0xff2b_77ca,
    0xcf84,
    0x432b,
    [0xa7, 0x35, 0x3a, 0x13, 0x0d, 0xde, 0x2a, 0x88],
);
pub const WIA_CATEGORY_FLATBED_GUID: GuidBytes = GuidBytes::new(
    0xfb60_7b1f,
    0x43f3,
    0x488b,
    [0x85, 0x5b, 0xfb, 0x70, 0x3e, 0xc3, 0x42, 0xa6],
);
pub const WIA_CATEGORY_FEEDER_GUID: GuidBytes = GuidBytes::new(
    0xfe13_1934,
    0xf84c,
    0x42ad,
    [0x8d, 0xa4, 0x61, 0x29, 0xcd, 0xdd, 0x72, 0x88],
);
pub const WIA_CATEGORY_FILM_GUID: GuidBytes = GuidBytes::new(
    0xfcf6_5be7,
    0x3ce3,
    0x4473,
    [0xaf, 0x85, 0xf5, 0xd3, 0x7d, 0x21, 0xb6, 0x8a],
);
pub const WIA_CATEGORY_ROOT_GUID: GuidBytes = GuidBytes::new(
    0xf193_526f,
    0x59b8,
    0x4a26,
    [0x98, 0x88, 0xe1, 0x6e, 0x4f, 0x97, 0xce, 0x10],
);
pub const WIA_CATEGORY_FOLDER_GUID: GuidBytes = GuidBytes::new(
    0xc692_a446,
    0x6f5a,
    0x481d,
    [0x85, 0xbb, 0x92, 0xe2, 0xe8, 0x6f, 0xd3, 0x0a],
);
pub const WIA_CATEGORY_FEEDER_FRONT_GUID: GuidBytes = GuidBytes::new(
    0x4823_175c,
    0x3b28,
    0x487b,
    [0xa7, 0xe6, 0xee, 0xbc, 0x17, 0x61, 0x4f, 0xd1],
);
pub const WIA_CATEGORY_FEEDER_BACK_GUID: GuidBytes = GuidBytes::new(
    0x61ca_74d4,
    0x39db,
    0x42aa,
    [0x89, 0xb1, 0x8c, 0x19, 0xc9, 0xcd, 0x4c, 0x23],
);
pub const WIA_CATEGORY_AUTO_GUID: GuidBytes = GuidBytes::new(
    0xdefe_5fd8,
    0x6c97,
    0x4dde,
    [0xb1, 0x1e, 0xcb, 0x50, 0x9b, 0x27, 0x0e, 0x11],
);

// `WiaImgFmt_*` GUIDs (`WiaDef.h`).
pub const WIA_IMG_FMT_MEMORYBMP_GUID: GuidBytes = GuidBytes::new(
    0xb96b_3caa,
    0x0728,
    0x11d3,
    [0x9d, 0x7b, 0x00, 0x00, 0xf8, 0x1e, 0xf3, 0x2e],
);
pub const WIA_IMG_FMT_BMP_GUID: GuidBytes = GuidBytes::new(
    0xb96b_3cab,
    0x0728,
    0x11d3,
    [0x9d, 0x7b, 0x00, 0x00, 0xf8, 0x1e, 0xf3, 0x2e],
);
pub const WIA_IMG_FMT_JPEG_GUID: GuidBytes = GuidBytes::new(
    0xb96b_3cae,
    0x0728,
    0x11d3,
    [0x9d, 0x7b, 0x00, 0x00, 0xf8, 0x1e, 0xf3, 0x2e],
);
pub const WIA_IMG_FMT_PNG_GUID: GuidBytes = GuidBytes::new(
    0xb96b_3caf,
    0x0728,
    0x11d3,
    [0x9d, 0x7b, 0x00, 0x00, 0xf8, 0x1e, 0xf3, 0x2e],
);
pub const WIA_IMG_FMT_TIFF_GUID: GuidBytes = GuidBytes::new(
    0xb96b_3cb1,
    0x0728,
    0x11d3,
    [0x9d, 0x7b, 0x00, 0x00, 0xf8, 0x1e, 0xf3, 0x2e],
);
pub const WIA_IMG_FMT_RAW_GUID: GuidBytes = GuidBytes::new(
    0x6f12_0719,
    0xf1a8,
    0x4e07,
    [0x9a, 0xde, 0x9b, 0x64, 0xc6, 0x3a, 0x3d, 0xcc],
);

// `WIA_CMD_*` GUIDs (`WiaDef.h`).
pub const WIA_CMD_STOP_FEEDER_GUID: GuidBytes = GuidBytes::new(
    0xd847_b06d,
    0x3905,
    0x459c,
    [0x95, 0x09, 0x9b, 0x29, 0xcd, 0xb6, 0x91, 0xe7],
);

/// Index of an item inside an open `WiaDeviceTree`, where `ItemId::ROOT` (`0`) is always the
/// root `IWiaItem2`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ItemId(pub usize);

impl ItemId {
    pub const ROOT: ItemId = ItemId(0);
}

/// Decoded value of a WIA property (`PROPVARIANT`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PropValue {
    /// 32-bit integer (`VT_I4`, `VT_UI4`, `VT_I2`, `VT_UI2`, `VT_UI1`, `VT_INT`, `VT_UINT`, `VT_BOOL`).
    I4(i32),
    /// String (`VT_BSTR` / `VT_LPWSTR`).
    Str(String),
    /// GUID (`VT_CLSID`).
    Guid(GuidBytes),
}

impl PropValue {
    pub fn as_i4(&self) -> Option<i32> {
        match self {
            PropValue::I4(v) => Some(*v),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            PropValue::Str(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_guid(&self) -> Option<GuidBytes> {
        match self {
            PropValue::Guid(g) => Some(*g),
            _ => None,
        }
    }
}

/// Valid-value constraint decoded from `IWiaPropertyStorage::GetPropertyAttributes`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PropConstraint {
    /// Unconstrained (`WIA_PROP_NONE`) or unrecognized attribute variant.
    None,
    /// `WIA_PROP_RANGE`: `[min, nom, max, step]`.
    Range {
        min: i32,
        nom: i32,
        max: i32,
        step: i32,
    },
    /// `WIA_PROP_LIST` of integers (`WIA_LIST_COUNT` and `WIA_LIST_NOM` header already stripped;
    /// `values` contains the `count` elements starting at `WIA_LIST_VALUES = 2`).
    ListI4 { nom: i32, values: Vec<i32> },
    /// `WIA_PROP_LIST` of GUIDs (`VT_VECTOR | VT_CLSID`).
    ListGuid {
        nom: GuidBytes,
        values: Vec<GuidBytes>,
    },
    /// `WIA_PROP_FLAG`: `nom` (`WIA_FLAG_NOM`) and `valid_bits` (`WIA_FLAG_VALUES`).
    Flags { nom: i32, valid_bits: i32 },
}

/// Property access flags + valid-value constraint from `GetPropertyAttributes`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PropAttr {
    pub access_flags: u32,
    pub constraint: PropConstraint,
}

impl PropAttr {
    pub fn is_writable(&self) -> bool {
        (self.access_flags & WIA_PROP_WRITE) != 0
    }
}

/// One `(guidFormatId, lTymed)` entry from `IWiaTransfer::EnumWIA_FORMAT_INFO`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FormatPair {
    pub format: GuidBytes,
    pub tymed: i32,
}

/// Control decision returned by [`TransferSink`] callbacks across COM (`S_OK` vs `S_FALSE`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum TransferAction {
    /// Continue transferring (`S_OK`).
    Continue,
    /// Request clean transfer stop (`S_FALSE`).
    Cancel,
}

/// Result of a single `IWiaTransfer::Download` call.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct DownloadOutcome {
    /// Exact `HRESULT` returned by `IWiaTransfer::Download` (`S_OK`, `S_FALSE`,
    /// `WIA_STATUS_END_OF_MEDIA`, `WIA_STATUS_WARMING_UP`, or a negative failure `HRESULT`).
    pub hr: i32,
    /// First negative `WiaTransferParams::hrErrorStatus` reported to `TransferCallback`, if any.
    pub callback_error_hr: Option<i32>,
    /// Whether any page stream hit [`crate::dib::MAX_DIB_BYTES`] (`STG_E_MEDIUMFULL`).
    pub stream_overflowed: bool,
    /// Panic message caught at a COM vtable boundary (`IStream` or `IWiaTransferCallback`), if any.
    pub callback_panic: Option<String>,
}

/// Safe callback sink driven by `IWiaTransferCallback` during [`WiaDeviceTree::download`].
pub(super) trait TransferSink {
    /// Called inside `IWiaTransferCallback::GetNextStream` before allocating a stream for the next
    /// page. Returning [`TransferAction::Cancel`] sets `*ppDestination = None` and returns
    /// `S_FALSE` to the WIA driver.
    fn on_next_stream(&mut self) -> TransferAction;

    /// Called on `WIA_TRANSFER_MSG_STATUS`, `WIA_TRANSFER_MSG_DEVICE_STATUS`, and
    /// `WIA_TRANSFER_MSG_NEW_PAGE` with `lMessage`, `ulTransferredBytes`, and `hrErrorStatus`.
    fn on_status(&mut self, msg: i32, transferred_bytes: u64, hr_status: i32) -> TransferAction;

    /// Called when a non-overflowed, error-free page stream finishes (`WIA_TRANSFER_MSG_END_OF_STREAM`
    /// or an unclosed non-empty stream at `WIA_TRANSFER_MSG_END_OF_TRANSFER`).
    fn on_stream_complete(&mut self, bytes: Vec<u8>) -> TransferAction;
}

/// Safe interface to an open WIA 2.0 device tree (`IWiaItem2` root + child items) on the STA
/// thread.
pub(super) trait WiaDeviceTree {
    /// Drops any previously enumerated child items so subsequent [`Self::children_of`] calls start
    /// fresh from [`ItemId::ROOT`].
    fn reset_children(&mut self);

    /// Enumerates direct child items (`EnumChildItems`) of `item`.
    fn children_of(&mut self, item: ItemId) -> Result<Vec<ItemId>, ScanError>;

    /// Returns `IWiaItem2::GetItemType` bitflags for `item`.
    fn item_type(&mut self, item: ItemId) -> Result<i32, ScanError>;

    /// Reads `ids` from `item`'s `IWiaPropertyStorage::ReadMultiple`, returning `None` in slots
    /// whose `PROPVARIANT` remained `VT_EMPTY` or had an unrecognized type.
    fn read_props(
        &mut self,
        item: ItemId,
        ids: &[PropId],
    ) -> Result<Vec<Option<PropValue>>, ScanError>;

    /// Reads `ids` from `item`'s `IWiaPropertyStorage::GetPropertyAttributes`, returning `None`
    /// when the item does not support a property.
    fn prop_attrs(
        &mut self,
        item: ItemId,
        ids: &[PropId],
    ) -> Result<Vec<Option<PropAttr>>, ScanError>;

    /// Writes a single `(id, val)` via `IWiaPropertyStorage::WriteMultiple`, returning `Err(hr)`
    /// with the raw negative `HRESULT` if the driver rejects it.
    fn write_prop(&mut self, item: ItemId, id: PropId, val: &PropValue) -> Result<(), i32>;

    /// Enumerates `(format, tymed)` pairs supported by `item`'s `IWiaTransfer`.
    fn transfer_formats(&mut self, item: ItemId) -> Result<Vec<FormatPair>, ScanError>;

    /// Runs `IWiaTransfer::Download(0, callback)` on `item`.
    fn download(
        &mut self,
        item: ItemId,
        sink: &mut dyn TransferSink,
    ) -> Result<DownloadOutcome, ScanError>;

    /// Issues `IWiaItem2::DeviceCommand(0, &command)` on `item` (e.g. `WIA_CMD_STOP_FEEDER`).
    fn device_command(&mut self, item: ItemId, command: GuidBytes) -> Result<(), i32>;
}

/// Safe interface to WIA 2.0 device enumeration and opening on the STA thread.
pub(super) trait WiaSystem {
    /// Enumerates local/connected WIA device info records (`WIA_DEVINFO_ENUM_LOCAL`).
    fn list_devices(&mut self) -> Result<Vec<RawDeviceProps>, ScanError>;

    /// Opens `native_id` via `CreateDevice(0, native_id)` and reads its root properties.
    fn open_device(
        &mut self,
        native_id: &str,
    ) -> Result<(RawDeviceProps, Box<dyn WiaDeviceTree>), ScanError>;
}

// ============================================================================
// Scripted fake WIA 2.0 system for cross-platform unit, integration, and worker tests
// ============================================================================

#[cfg(any(test, feature = "fake"))]
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) mod fake_wia {
    use super::*;
    use crate::wia::error::{
        E_INVALIDARG_HR, S_FALSE_HR, S_OK_HR, WIA_NO_DEVICE_OR_ENDORSER_HR, WiaOp, hresult,
    };

    fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
        m.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// One scripted callback event inside a [`FakeDownloadCall`].
    #[derive(Clone)]
    pub enum FakeTransferEvent {
        /// Emits `on_status(msg, transferred_bytes, hr_status)`.
        Status {
            msg: i32,
            transferred_bytes: u64,
            hr_status: i32,
        },
        /// Requests a stream via `on_next_stream`, optionally emits intermediate status updates,
        /// and delivers `bytes` to `on_stream_complete`.
        Page { chunks: Vec<u64>, bytes: Vec<u8> },
        /// Calls `on_next_stream` (allocating an empty stream) and then reports a negative
        /// `hrErrorStatus` without completing a page (simulating WSD / ADF empty-paper or jam
        /// discovered after `GetNextStream`).
        EmptyStreamError { hr_status: i32 },
        /// Simulates a stream exceeding `MAX_DIB_BYTES` (`STG_E_MEDIUMFULL`).
        StreamOverflow { return_hr: i32 },
        /// Runs a test synchronization callback on the STA thread mid-`Download`.
        Hook(Arc<dyn Fn() + Send + Sync>),
    }

    impl core::fmt::Debug for FakeTransferEvent {
        fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
            match self {
                FakeTransferEvent::Status {
                    msg,
                    transferred_bytes,
                    hr_status,
                } => f
                    .debug_struct("Status")
                    .field("msg", msg)
                    .field("transferred_bytes", transferred_bytes)
                    .field("hr_status", hr_status)
                    .finish(),
                FakeTransferEvent::Page { chunks, bytes } => f
                    .debug_struct("Page")
                    .field("chunks", chunks)
                    .field("bytes_len", &bytes.len())
                    .finish(),
                FakeTransferEvent::EmptyStreamError { hr_status } => f
                    .debug_struct("EmptyStreamError")
                    .field("hr_status", hr_status)
                    .finish(),
                FakeTransferEvent::StreamOverflow { return_hr } => f
                    .debug_struct("StreamOverflow")
                    .field("return_hr", return_hr)
                    .finish(),
                FakeTransferEvent::Hook(_) => f.write_str("Hook(..)"),
            }
        }
    }

    /// Scripted outcome of one `IWiaTransfer::Download` invocation.
    #[derive(Clone, Debug)]
    pub struct FakeDownloadCall {
        pub events: Vec<FakeTransferEvent>,
        pub return_hr: i32,
        /// Optional required `WIA_IPA_TYMED` value; if the item's `WIA_IPA_TYMED` differs,
        /// returns `refuse_hr` immediately with 0 streams (for testing `TYMED_CALLBACK` ->
        /// `TYMED_FILE` fallback).
        pub require_tymed: Option<(i32, i32)>,
        /// Optional stdout line printed by the simulated noisy driver during `Download`.
        pub stdout_chatter: Option<String>,
    }

    impl FakeDownloadCall {
        pub fn ok(events: Vec<FakeTransferEvent>) -> Self {
            FakeDownloadCall {
                events,
                return_hr: S_OK_HR,
                require_tymed: None,
                stdout_chatter: None,
            }
        }

        pub fn with_hr(events: Vec<FakeTransferEvent>, return_hr: i32) -> Self {
            FakeDownloadCall {
                events,
                return_hr,
                require_tymed: None,
                stdout_chatter: None,
            }
        }
    }

    /// Write-hook behavior when a property is written on a [`FakeItem`].
    pub type WriteHook =
        Arc<dyn Fn(&mut FakeItem, PropId, &PropValue) -> Result<(), i32> + Send + Sync>;

    /// A scripted `IWiaItem2` node inside a [`FakeDeviceSpec`].
    #[derive(Clone)]
    pub struct FakeItem {
        pub item_type: i32,
        pub children: Vec<usize>,
        pub props: BTreeMap<PropId, PropValue>,
        pub attrs: BTreeMap<PropId, PropAttr>,
        pub formats: Vec<FormatPair>,
        pub downloads: VecDeque<FakeDownloadCall>,
        pub write_hook: Option<WriteHook>,
        pub commands: Vec<GuidBytes>,
    }

    impl core::fmt::Debug for FakeItem {
        fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
            f.debug_struct("FakeItem")
                .field("item_type", &self.item_type)
                .field("children", &self.children)
                .field("props", &self.props)
                .finish()
        }
    }

    impl FakeItem {
        pub fn new(item_type: i32) -> Self {
            FakeItem {
                item_type,
                children: Vec::new(),
                props: BTreeMap::new(),
                attrs: BTreeMap::new(),
                formats: Vec::new(),
                downloads: VecDeque::new(),
                write_hook: None,
                commands: Vec::new(),
            }
        }

        pub fn with_prop(mut self, id: PropId, val: PropValue, attr: Option<PropAttr>) -> Self {
            self.props.insert(id, val);
            if let Some(a) = attr {
                self.attrs.insert(id, a);
            }
            self
        }

        pub fn with_formats(mut self, formats: Vec<FormatPair>) -> Self {
            self.formats = formats;
            self
        }

        pub fn with_downloads(mut self, calls: impl IntoIterator<Item = FakeDownloadCall>) -> Self {
            self.downloads.extend(calls);
            self
        }

        pub fn with_write_hook<F>(mut self, hook: F) -> Self
        where
            F: Fn(&mut FakeItem, PropId, &PropValue) -> Result<(), i32> + Send + Sync + 'static,
        {
            self.write_hook = Some(Arc::new(hook));
            self
        }
    }

    /// Scripted WIA device entry registered in [`FakeWiaState`].
    #[derive(Clone, Debug)]
    pub struct FakeDeviceSpec {
        pub enum_props: RawDeviceProps,
        pub root_props: RawDeviceProps,
        /// Whether `EnumDeviceInfo(WIA_DEVINFO_ENUM_LOCAL)` lists this device.
        pub listed: bool,
        /// Optional `HRESULT` returned by `CreateDevice` instead of opening.
        pub open_error_hr: Option<i32>,
        /// Optional delay inside `CreateDevice` (for `TimedOut` / `AbandonedOpen` tests).
        pub open_delay: Duration,
        /// Item tree (`items[0]` is root).
        pub items: Vec<FakeItem>,
        /// Shared counter incremented whenever an opened `FakeDeviceTree` for this device drops.
        pub closed_count: Arc<AtomicUsize>,
        /// Shared log of `DeviceCommand` GUIDs issued across the tree.
        pub command_log: Arc<Mutex<Vec<GuidBytes>>>,
    }

    impl FakeDeviceSpec {
        pub fn new(props: RawDeviceProps) -> Self {
            let root = FakeItem::new(WIA_ITEM_TYPE_ROOT | WIA_ITEM_TYPE_FOLDER);
            FakeDeviceSpec {
                root_props: props.clone(),
                enum_props: props,
                listed: true,
                open_error_hr: None,
                open_delay: Duration::ZERO,
                items: vec![root],
                closed_count: Arc::new(AtomicUsize::new(0)),
                command_log: Arc::new(Mutex::new(Vec::new())),
            }
        }

        pub fn add_child(&mut self, parent: ItemId, child: FakeItem) -> ItemId {
            let idx = self.items.len();
            self.items.push(child);
            self.items[parent.0].children.push(idx);
            ItemId(idx)
        }
    }

    /// Shared mutable state of a scripted `FakeWiaSystem`.
    #[derive(Debug, Default)]
    pub struct FakeWiaState {
        pub list_error_hr: Option<i32>,
        pub list_delay: Duration,
        pub list_stdout_chatter: Option<String>,
        pub devices: Vec<FakeDeviceSpec>,
        pub active_opens: AtomicUsize,
    }

    /// Handle to a scripted `WiaSystem` shared between tests and the STA session thread.
    #[derive(Clone, Debug, Default)]
    pub struct FakeWiaHandle {
        pub state: Arc<Mutex<FakeWiaState>>,
    }

    impl FakeWiaHandle {
        pub fn new() -> Self {
            Self::default()
        }

        pub fn add_device(&self, spec: FakeDeviceSpec) {
            lock(&self.state).devices.push(spec);
        }

        pub(crate) fn make_system(&self) -> FakeWiaSystem {
            FakeWiaSystem {
                state: Arc::clone(&self.state),
            }
        }
    }

    pub(crate) struct FakeWiaSystem {
        state: Arc<Mutex<FakeWiaState>>,
    }

    impl WiaSystem for FakeWiaSystem {
        fn list_devices(&mut self) -> Result<Vec<RawDeviceProps>, ScanError> {
            let (err_hr, delay, chatter, out) = {
                let guard = lock(&self.state);
                let listed = guard
                    .devices
                    .iter()
                    .filter(|d| d.listed)
                    .map(|d| d.enum_props.clone())
                    .collect::<Vec<_>>();
                (
                    guard.list_error_hr,
                    guard.list_delay,
                    guard.list_stdout_chatter.clone(),
                    listed,
                )
            };
            if let Some(line) = chatter {
                println!("{line}");
            }
            if !delay.is_zero() {
                std::thread::sleep(delay);
            }
            if let Some(hr) = err_hr {
                return Err(hresult(WiaOp::EnumDevices, hr));
            }
            Ok(out)
        }

        fn open_device(
            &mut self,
            native_id: &str,
        ) -> Result<(RawDeviceProps, Box<dyn WiaDeviceTree>), ScanError> {
            let found = {
                let guard = lock(&self.state);
                guard
                    .devices
                    .iter()
                    .find(|d| d.enum_props.dev_id == native_id)
                    .cloned()
            };
            let Some(spec) = found else {
                return Err(hresult(WiaOp::OpenDevice, WIA_NO_DEVICE_OR_ENDORSER_HR));
            };
            if !spec.open_delay.is_zero() {
                std::thread::sleep(spec.open_delay);
            }
            if let Some(hr) = spec.open_error_hr {
                return Err(hresult(WiaOp::OpenDevice, hr));
            }
            {
                let guard = lock(&self.state);
                guard.active_opens.fetch_add(1, Ordering::AcqRel);
            }
            let tree = FakeDeviceTree {
                state: Arc::clone(&self.state),
                items: spec.items,
                exposed_len: 1,
                closed_count: spec.closed_count,
                command_log: spec.command_log,
            };
            Ok((spec.root_props, Box::new(tree)))
        }
    }

    struct FakeDeviceTree {
        state: Arc<Mutex<FakeWiaState>>,
        items: Vec<FakeItem>,
        exposed_len: usize,
        closed_count: Arc<AtomicUsize>,
        command_log: Arc<Mutex<Vec<GuidBytes>>>,
    }

    impl Drop for FakeDeviceTree {
        fn drop(&mut self) {
            self.closed_count.fetch_add(1, Ordering::AcqRel);
            lock(&self.state)
                .active_opens
                .fetch_sub(1, Ordering::AcqRel);
        }
    }

    impl FakeDeviceTree {
        fn item(&self, id: ItemId) -> Result<&FakeItem, ScanError> {
            self.items.get(id.0).ok_or_else(|| {
                crate::wia::error::driver_msg(format!("invalid fake ItemId {}", id.0))
            })
        }

        fn item_mut(&mut self, id: ItemId) -> Result<&mut FakeItem, ScanError> {
            self.items.get_mut(id.0).ok_or_else(|| {
                crate::wia::error::driver_msg(format!("invalid fake ItemId {}", id.0))
            })
        }
    }

    impl WiaDeviceTree for FakeDeviceTree {
        fn reset_children(&mut self) {
            self.exposed_len = 1;
        }

        fn children_of(&mut self, item: ItemId) -> Result<Vec<ItemId>, ScanError> {
            let indices = self.item(item)?.children.clone();
            for &idx in &indices {
                if idx >= self.exposed_len {
                    self.exposed_len = idx + 1;
                }
            }
            Ok(indices.into_iter().map(ItemId).collect())
        }

        fn item_type(&mut self, item: ItemId) -> Result<i32, ScanError> {
            Ok(self.item(item)?.item_type)
        }

        fn read_props(
            &mut self,
            item: ItemId,
            ids: &[PropId],
        ) -> Result<Vec<Option<PropValue>>, ScanError> {
            let node = self.item(item)?;
            Ok(ids.iter().map(|id| node.props.get(id).cloned()).collect())
        }

        fn prop_attrs(
            &mut self,
            item: ItemId,
            ids: &[PropId],
        ) -> Result<Vec<Option<PropAttr>>, ScanError> {
            let node = self.item(item)?;
            Ok(ids.iter().map(|id| node.attrs.get(id).cloned()).collect())
        }

        fn write_prop(&mut self, item: ItemId, id: PropId, val: &PropValue) -> Result<(), i32> {
            let Some(node) = self.items.get_mut(item.0) else {
                return Err(E_INVALIDARG_HR);
            };
            if let Some(attr) = node.attrs.get(&id) {
                if !attr.is_writable() {
                    return Err(E_INVALIDARG_HR);
                }
                match (&attr.constraint, val) {
                    (PropConstraint::Range { min, max, step, .. }, PropValue::I4(v)) => {
                        if *v < *min || *v > *max {
                            return Err(E_INVALIDARG_HR);
                        }
                        if *step > 1 && (*v - *min) % *step != 0 {
                            return Err(E_INVALIDARG_HR);
                        }
                    }
                    (PropConstraint::ListI4 { values, .. }, PropValue::I4(v))
                        if !values.contains(v) =>
                    {
                        return Err(E_INVALIDARG_HR);
                    }
                    (PropConstraint::ListGuid { values, .. }, PropValue::Guid(g))
                        if !values.contains(g) =>
                    {
                        return Err(E_INVALIDARG_HR);
                    }
                    (PropConstraint::Flags { valid_bits, .. }, PropValue::I4(v))
                        if (*v & !*valid_bits) != 0 =>
                    {
                        return Err(E_INVALIDARG_HR);
                    }
                    _ => {}
                }
            } else if !node.props.contains_key(&id) {
                return Err(E_INVALIDARG_HR);
            }
            if let Some(hook) = node.write_hook.clone() {
                hook(node, id, val)?;
            }
            node.props.insert(id, val.clone());
            Ok(())
        }

        fn transfer_formats(&mut self, item: ItemId) -> Result<Vec<FormatPair>, ScanError> {
            Ok(self.item(item)?.formats.clone())
        }

        fn download(
            &mut self,
            item: ItemId,
            sink: &mut dyn TransferSink,
        ) -> Result<DownloadOutcome, ScanError> {
            let current_tymed = self
                .item(item)?
                .props
                .get(&WIA_IPA_TYMED)
                .and_then(PropValue::as_i4);
            let Some(call) = self.item_mut(item)?.downloads.pop_front() else {
                return Ok(DownloadOutcome {
                    hr: crate::wia::error::WIA_ERROR_PAPER_EMPTY_HR,
                    callback_error_hr: None,
                    stream_overflowed: false,
                    callback_panic: None,
                });
            };
            if let Some(line) = &call.stdout_chatter {
                println!("{line}");
            }
            if let Some((expected_tymed, refuse_hr)) = call.require_tymed
                && current_tymed != Some(expected_tymed)
            {
                // Push back any subsequent calls? No, the fallback call is queued right after.
                return Ok(DownloadOutcome {
                    hr: refuse_hr,
                    callback_error_hr: None,
                    stream_overflowed: false,
                    callback_panic: None,
                });
            }

            let mut callback_error_hr = None;
            for ev in call.events {
                match ev {
                    FakeTransferEvent::Status {
                        msg,
                        transferred_bytes,
                        hr_status,
                    } => {
                        if hr_status < 0 && callback_error_hr.is_none() {
                            callback_error_hr = Some(hr_status);
                        }
                        if sink.on_status(msg, transferred_bytes, hr_status)
                            == TransferAction::Cancel
                        {
                            return Ok(DownloadOutcome {
                                hr: S_FALSE_HR,
                                callback_error_hr,
                                stream_overflowed: false,
                                callback_panic: None,
                            });
                        }
                    }
                    FakeTransferEvent::Page { chunks, bytes } => {
                        if sink.on_next_stream() == TransferAction::Cancel {
                            return Ok(DownloadOutcome {
                                hr: S_FALSE_HR,
                                callback_error_hr,
                                stream_overflowed: false,
                                callback_panic: None,
                            });
                        }
                        for transferred in chunks {
                            if sink.on_status(WIA_TRANSFER_MSG_STATUS, transferred, S_OK_HR)
                                == TransferAction::Cancel
                            {
                                return Ok(DownloadOutcome {
                                    hr: S_FALSE_HR,
                                    callback_error_hr,
                                    stream_overflowed: false,
                                    callback_panic: None,
                                });
                            }
                        }
                        if sink.on_stream_complete(bytes) == TransferAction::Cancel {
                            return Ok(DownloadOutcome {
                                hr: S_FALSE_HR,
                                callback_error_hr,
                                stream_overflowed: false,
                                callback_panic: None,
                            });
                        }
                    }
                    FakeTransferEvent::EmptyStreamError { hr_status } => {
                        if sink.on_next_stream() == TransferAction::Cancel {
                            return Ok(DownloadOutcome {
                                hr: S_FALSE_HR,
                                callback_error_hr,
                                stream_overflowed: false,
                                callback_panic: None,
                            });
                        }
                        if hr_status < 0 && callback_error_hr.is_none() {
                            callback_error_hr = Some(hr_status);
                        }
                        let _ = sink.on_status(WIA_TRANSFER_MSG_STATUS, 0, hr_status);
                    }
                    FakeTransferEvent::StreamOverflow { return_hr } => {
                        if sink.on_next_stream() == TransferAction::Cancel {
                            return Ok(DownloadOutcome {
                                hr: S_FALSE_HR,
                                callback_error_hr,
                                stream_overflowed: false,
                                callback_panic: None,
                            });
                        }
                        return Ok(DownloadOutcome {
                            hr: return_hr,
                            callback_error_hr,
                            stream_overflowed: true,
                            callback_panic: None,
                        });
                    }
                    FakeTransferEvent::Hook(hook) => {
                        hook();
                    }
                }
            }

            Ok(DownloadOutcome {
                hr: call.return_hr,
                callback_error_hr,
                stream_overflowed: false,
                callback_panic: None,
            })
        }

        fn device_command(&mut self, item: ItemId, command: GuidBytes) -> Result<(), i32> {
            lock(&self.command_log).push(command);
            if let Some(node) = self.items.get_mut(item.0) {
                node.commands.push(command);
                Ok(())
            } else {
                Err(E_INVALIDARG_HR)
            }
        }
    }
}
