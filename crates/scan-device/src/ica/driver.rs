//! ImageCaptureCore's numbers the `ica` backend reads, in constants of its own; the plain data
//! the Objective-C glue hands the rest of the backend, and the events its callbacks send; the
//! internal trait the session, caps, settings and batch logic call (`IcaSystem`); and, behind the
//! `fake` feature, a scripted ImageCaptureCore (`fake_ica`) that answers it as the framework's
//! documentation reads.
//!
//! Pure safe Rust, compiled on every system: nothing here names an objc2 type, so the logic over
//! it runs and is tested where ImageCaptureCore isn't. The numbers stay integers as the framework
//! gives them (`NSUInteger` and `NSInteger`, 64 bits on every Mac this builds for): none becomes a
//! Rust enum without a check, and a value the backend doesn't know is said, never assumed. On
//! macOS a unit test holds each constant against the bindings' own.

use std::time::Duration;

use crate::ScanError;

// The framework's functional unit types (`ICScannerFunctionalUnitType`).

/// The flatbed.
pub const UNIT_FLATBED: u64 = 0;
/// The transparency unit for positive film.
pub const UNIT_POSITIVE_TRANSPARENCY: u64 = 1;
/// The transparency unit for negative film.
pub const UNIT_NEGATIVE_TRANSPARENCY: u64 = 2;
/// The document feeder.
pub const UNIT_DOCUMENT_FEEDER: u64 = 3;

// Pixel data types (`ICScannerPixelDataType`), a unit's and a band's.

/// Black and white, a bit a pixel.
pub const PIXEL_BW: u64 = 0;
/// Gray.
pub const PIXEL_GRAY: u64 = 1;
/// Red, green and blue.
pub const PIXEL_RGB: u64 = 2;
/// Indexed through a palette.
pub const PIXEL_PALETTE: u64 = 3;
/// Cyan, magenta and yellow.
pub const PIXEL_CMY: u64 = 4;
/// Cyan, magenta, yellow and black.
pub const PIXEL_CMYK: u64 = 5;
/// YUV.
pub const PIXEL_YUV: u64 = 6;
/// YUV and black.
pub const PIXEL_YUVK: u64 = 7;
/// CIE XYZ.
pub const PIXEL_CIEXYZ: u64 = 8;

// Bit depths (`ICScannerBitDepth`): a sample's bits.

/// A bit a sample.
pub const DEPTH_1: u64 = 1;
/// A byte a sample.
pub const DEPTH_8: u64 = 8;
/// Two bytes a sample.
pub const DEPTH_16: u64 = 16;

// Measurement units (`ICScannerMeasurementUnit`), in which a unit gives its lengths.

/// Inches.
pub const MEASURE_INCHES: u64 = 0;
/// Centimetres.
pub const MEASURE_CENTIMETERS: u64 = 1;
/// Picas, six to the inch.
pub const MEASURE_PICAS: u64 = 2;
/// Points, 72 to the inch.
pub const MEASURE_POINTS: u64 = 3;
/// Twips, 1,440 to the inch.
pub const MEASURE_TWIPS: u64 = 4;
/// Pixels, at the unit's resolution.
pub const MEASURE_PIXELS: u64 = 5;

// Document types (`ICScannerDocumentType`) a feeder is set to: those the backend states a size
// for (`caps::DOCUMENT_TYPES`). The B sizes and the business card are left out: which B the
// framework's types mean isn't shown by its documentation, and no standard fixes a business
// card's size.

/// The module's default.
pub const DOCUMENT_DEFAULT: u64 = 0;
/// A4.
pub const DOCUMENT_A4: u64 = 1;
/// US Letter.
pub const DOCUMENT_US_LETTER: u64 = 3;
/// US Legal.
pub const DOCUMENT_US_LEGAL: u64 = 4;
/// A5.
pub const DOCUMENT_A5: u64 = 5;
/// US Ledger.
pub const DOCUMENT_US_LEDGER: u64 = 9;
/// US Executive.
pub const DOCUMENT_US_EXECUTIVE: u64 = 10;
/// A3.
pub const DOCUMENT_A3: u64 = 11;
/// A6.
pub const DOCUMENT_A6: u64 = 13;
/// US Statement.
pub const DOCUMENT_US_STATEMENT: u64 = 52;

// A device's `type`: its device type (`ICDeviceType`) OR'd with its location's
// (`ICDeviceLocationType`).

/// A camera.
pub const DEVICE_CAMERA: u64 = 0x0000_0001;
/// A scanner.
pub const DEVICE_SCANNER: u64 = 0x0000_0002;
/// Attached to this Mac.
pub const LOCATION_LOCAL: u64 = 0x0000_0100;
/// Shared by another Mac.
pub const LOCATION_SHARED: u64 = 0x0000_0200;
/// Found on the network by Bonjour.
pub const LOCATION_BONJOUR: u64 = 0x0000_0400;
/// Reached over Bluetooth.
pub const LOCATION_BLUETOOTH: u64 = 0x0000_0800;
/// Every location that isn't this Mac (`ICDeviceLocationTypeMaskRemote`).
pub const LOCATION_REMOTE: u64 = 0x0000_FE00;

/// What a listing browses for: scanners, attached, found by Bonjour, or shared.
pub const BROWSE_MASK: u64 = DEVICE_SCANNER | LOCATION_LOCAL | LOCATION_BONJOUR | LOCATION_SHARED;

/// A page's orientation as the scanner reads it (`ICEXIFOrientationType`'s first): set on a
/// feeder's odd and even pages, so a back comes as read, for the app to turn.
pub const ORIENTATION_AS_READ: u64 = 1;

// Transfer modes (`ICScannerTransferMode`).

/// Pages written to files in a directory.
pub const TRANSFER_FILE_BASED: u64 = 0;
/// Pages sent in bands of memory: the backend's.
pub const TRANSFER_MEMORY_BASED: u64 = 1;

// The framework's return codes (`ICReturnCode`, `ICReturnConnectionErrorCode`), an `NSError`'s
// code in ImageCaptureCore's domain: those the backend maps (`error::scan_error`).

/// The device didn't answer in time.
pub const RETURN_COMMUNICATION_TIMED_OUT: i64 = -9923;
/// The scan was cancelled, by the backend or at the scanner.
pub const RETURN_SCAN_OPERATION_CANCELED: i64 = -9924;
/// Another program on this Mac is using the scanner.
pub const RETURN_SCANNER_IN_USE_BY_LOCAL_USER: i64 = -9925;
/// Someone on another computer is using the scanner.
pub const RETURN_SCANNER_IN_USE_BY_REMOTE_USER: i64 = -9926;
/// The session didn't open.
pub const RETURN_DEVICE_FAILED_TO_OPEN_SESSION: i64 = -9927;
/// The session didn't close.
pub const RETURN_DEVICE_FAILED_TO_CLOSE_SESSION: i64 = -9928;
/// The functional unit asked for wasn't selected.
pub const RETURN_SCANNER_FAILED_TO_SELECT_FUNCTIONAL_UNIT: i64 = -9929;
/// The scan didn't complete.
pub const RETURN_SCANNER_FAILED_TO_COMPLETE_SCAN: i64 = -9931;
/// The device asks for credentials.
pub const RETURN_DEVICE_NEEDS_CREDENTIALS: i64 = -9953;
/// The device is still being enumerated.
pub const RETURN_DEVICE_IS_BUSY_ENUMERATING: i64 = -9954;
/// The device's session isn't open.
pub const RETURN_SESSION_NOT_OPENED: i64 = -9958;
/// The device's module exited.
pub const RETURN_CONNECTION_DRIVER_EXITED: i64 = -21350;
/// The session closed suddenly.
pub const RETURN_CONNECTION_CLOSED_SESSION_SUDDENLY: i64 = -21349;
/// The device was ejected suddenly.
pub const RETURN_CONNECTION_EJECTED_SUDDENLY: i64 = -21348;
/// A session with the device is open already.
pub const RETURN_CONNECTION_SESSION_ALREADY_OPEN: i64 = -21347;
/// This process isn't allowed to open the device.
pub const RETURN_CONNECTION_NOT_AUTHORIZED_TO_OPEN_DEVICE: i64 = -21343;

/// A device the browser found, as the glue reads it, each string capped as it's read.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DeviceProps {
    /// `persistentIDString`, where the device gives one.
    pub persistent_id: Option<String>,
    /// `UUIDString`, where the device gives one.
    pub uuid: Option<String>,
    /// `name`, where the device gives one.
    pub name: Option<String>,
    /// `type`: the device's type bits ([`DEVICE_SCANNER`]) OR'd with its location's
    /// ([`LOCATION_LOCAL`] and the rest).
    pub type_bits: u64,
    /// Whether the object is a scanner device (`ICScannerDevice`), as the glue's check of its
    /// class finds.
    pub scanner_class: bool,
}

/// An `NSIndexSet` as the glue reads it: its count, its first and last indexes, and its indexes
/// in order, at most [`MAX_INDEXES`] of them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct IndexSet {
    /// How many indexes the set holds.
    pub count: u64,
    /// Its lowest index; meaningless where `count` is 0.
    pub first: u64,
    /// Its highest index; meaningless where `count` is 0.
    pub last: u64,
    /// Its indexes from the lowest, at most [`MAX_INDEXES`].
    pub indexes: Vec<u64>,
}

impl IndexSet {
    /// The set of `indexes`, sorted and without duplicates, as the framework's are: for tests and
    /// the scripted framework.
    pub fn of(indexes: &[u64]) -> IndexSet {
        let mut sorted = indexes.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        IndexSet {
            count: sorted.len() as u64,
            first: sorted.first().copied().unwrap_or(0),
            last: sorted.last().copied().unwrap_or(0),
            indexes: sorted.into_iter().take(MAX_INDEXES).collect(),
        }
    }
}

/// The most indexes of one index set the glue reads.
pub const MAX_INDEXES: usize = 4096;

/// The most functional unit types of one device the glue reads.
pub const MAX_UNITS: usize = 16;

/// A width and a height, in a unit's measurement unit (`NSSize`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct UnitSize {
    /// Across.
    pub width: f64,
    /// Down.
    pub height: f64,
}

/// An area from its top left corner, in a unit's measurement unit (`NSRect`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct UnitRect {
    /// From the left edge.
    pub x: f64,
    /// From the top edge.
    pub y: f64,
    /// Across.
    pub width: f64,
    /// Down.
    pub height: f64,
}

/// A functional unit as the glue reads it whole: the selected one's properties, the feeder's
/// where it is one.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct UnitProps {
    /// `type`: [`UNIT_FLATBED`], [`UNIT_DOCUMENT_FEEDER`] or another.
    pub unit_type: u64,
    /// `pixelDataType`, as set.
    pub pixel_type: u64,
    /// `bitDepth`, as set.
    pub bit_depth: u64,
    /// `supportedBitDepths`.
    pub bit_depths: IndexSet,
    /// `measurementUnit`, in which `physical_size` and `scan_area` are given.
    pub measurement_unit: u64,
    /// `resolution`, as set, in dots per inch.
    pub resolution: u64,
    /// `supportedResolutions`.
    pub resolutions: IndexSet,
    /// `physicalSize`: the largest area the unit scans.
    pub physical_size: UnitSize,
    /// `scanArea`, as set.
    pub scan_area: UnitRect,
    /// A feeder's `supportedDocumentTypes`; empty for a flatbed.
    pub document_types: IndexSet,
    /// A feeder's `documentType`, as set.
    pub document_type: u64,
    /// A feeder's `supportsDuplexScanning`.
    pub duplex_supported: bool,
    /// A feeder's `duplexScanningEnabled`, as set.
    pub duplex_enabled: bool,
    /// A feeder's `documentLoaded`: true where its module finds paper in it, and false where
    /// it finds none or can't tell.
    pub document_loaded: bool,
    /// A feeder's `reverseFeederPageOrder`: true where it reads its stack last sheet first.
    pub reverse_feeder_order: bool,
    /// A feeder's `oddPageOrientation`, as set.
    pub odd_orientation: u64,
    /// A feeder's `evenPageOrientation`, as set.
    pub even_orientation: u64,
}

/// A band of a memory-based transfer (`ICScannerBandData`): its numbers as the framework gives
/// them, every one checked before the band's bytes are read (`pixels::PageAssembler`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BandHeader {
    /// `fullImageWidth`: the page's width, in pixels.
    pub full_width: u64,
    /// `fullImageHeight`: the page's height, in rows.
    pub full_height: u64,
    /// `bitsPerPixel`.
    pub bits_per_pixel: u64,
    /// `bitsPerComponent`.
    pub bits_per_component: u64,
    /// `numComponents`.
    pub components: u64,
    /// `isBigEndian`: whether a 16-bit sample's high byte comes first.
    pub big_endian: bool,
    /// `pixelDataType`.
    pub pixel_type: u64,
    /// `bytesPerRow`: a row's bytes in the band, its padding included.
    pub bytes_per_row: u64,
    /// `dataStartRow`: the page's row the band's first is.
    pub start_row: u64,
    /// `dataNumRows`: the band's rows.
    pub rows: u64,
    /// `dataSize`: the band's bytes of image data.
    pub data_size: u64,
}

/// The most bytes of one band's buffer the glue copies: one past it is refused uncopied.
pub const MAX_BAND_BYTES: u64 = 64 << 20;

/// An `NSError` the framework gave, as the glue reads it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct IcaError {
    /// Whether its domain is ImageCaptureCore's (`ICErrorDomain`), so its code is one of the
    /// framework's.
    pub ica_domain: bool,
    /// Its domain, capped, for the message where it's another's.
    pub domain: String,
    /// Its code.
    pub code: i64,
    /// Its localized description, capped.
    pub description: String,
}

/// A browse the system started (an `ICDeviceBrowser` and its delegate), as the backend holds it:
/// a number, never the object, so what holds it can be sent to any thread.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct BrowseRef(pub u64);

/// A device a browse found (an `ICScannerDevice`), as the backend holds it: a number, never the
/// object. A device found again by another browse has another number.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DeviceRef(pub u64);

/// Whose delegate a callback came to: a browse's or a device's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Origin {
    /// A browse's (`ICDeviceBrowserDelegate`).
    Browse(BrowseRef),
    /// A device's (`ICDeviceDelegate` and `ICScannerDeviceDelegate`).
    Device(DeviceRef),
}

/// A device's status, as `device:didReceiveStatusInformation:` names it
/// (`ICStatusNotificationKey`'s value): the two the backend reads, and anything else.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeviceStatus {
    /// `ICScannerStatusWarmingUp`: the scanner is warming up.
    WarmingUp,
    /// `ICScannerStatusWarmUpDone`: it has warmed up.
    WarmedUp,
    /// Any other, unread.
    Other,
}

/// What one of the framework's callbacks said, as owned plain data, tagged with the browse or
/// device it came for, so a late event of an earlier request is told apart and dropped.
///
/// The glue only reads what a callback is handed into one of these; the numbers stay as the
/// framework gave them, every one checked by whoever reads it.
#[derive(Clone, Debug, PartialEq)]
pub enum IcaEvent {
    /// `deviceBrowser:didAddDevice:moreComing:`: a browse found a device.
    DeviceAdded {
        /// The browse.
        browse: BrowseRef,
        /// The device, as the backend holds it from now.
        device: DeviceRef,
        /// The device as the glue read it.
        props: DeviceProps,
    },
    /// `deviceBrowser:didRemoveDevice:moreGoing:`: a browse lost a device.
    DeviceRemoved {
        /// The browse.
        browse: BrowseRef,
        /// The device.
        device: DeviceRef,
    },
    /// `deviceBrowserDidEnumerateLocalDevices:`: a browse has found every device attached to
    /// this Mac; network scanners may still come.
    LocalEnumerated {
        /// The browse.
        browse: BrowseRef,
    },
    /// `device:didOpenSessionWithError:`.
    SessionOpened {
        /// The device.
        device: DeviceRef,
        /// The error, where the session didn't open.
        error: Option<IcaError>,
    },
    /// `device:didCloseSessionWithError:`, asked for or not.
    SessionClosed {
        /// The device.
        device: DeviceRef,
        /// The error, where there was one.
        error: Option<IcaError>,
    },
    /// `deviceDidBecomeReady:`: the device's session is open and it takes requests.
    Ready {
        /// The device.
        device: DeviceRef,
    },
    /// `didRemoveDevice:`, to the device's own delegate: the device is gone.
    Removed {
        /// The device.
        device: DeviceRef,
    },
    /// `scannerDevice:didSelectFunctionalUnit:error:`: a unit was selected, asked for or not (the
    /// framework selects one as the device is instantiated), or a select failed.
    UnitSelected {
        /// The device.
        device: DeviceRef,
        /// The unit selected, read whole as the callback came (boxed, a unit being the largest
        /// thing an event carries but a band's bytes); `None` where the framework handed none,
        /// though its header says it never does.
        unit: Option<Box<UnitProps>>,
        /// The error, where the select failed.
        error: Option<IcaError>,
    },
    /// `scannerDevice:didScanToBandData:`: a band of a memory-based transfer.
    Band {
        /// The device.
        device: DeviceRef,
        /// The band's numbers, as given.
        header: BandHeader,
        /// The band's buffer, copied once its length was found within [`MAX_BAND_BYTES`].
        bytes: Vec<u8>,
    },
    /// A band whose buffer was past [`MAX_BAND_BYTES`], refused uncopied.
    OversizedBand {
        /// The device.
        device: DeviceRef,
        /// The buffer's length.
        len: u64,
    },
    /// `scannerDevice:didCompleteScanWithError:`: the scan ended.
    ScanCompleted {
        /// The device.
        device: DeviceRef,
        /// The error, where it ended in one (a cancel among them).
        error: Option<IcaError>,
    },
    /// `device:didEncounterError:`.
    DeviceError {
        /// The device.
        device: DeviceRef,
        /// The error.
        error: IcaError,
    },
    /// `device:didReceiveStatusInformation:`.
    Status {
        /// The device.
        device: DeviceRef,
        /// The status.
        status: DeviceStatus,
    },
    /// A callback panicked: caught at the boundary, never unwound into the framework.
    Panicked {
        /// The delegate it came to.
        origin: Origin,
        /// The panic's message.
        message: String,
    },
}

impl IcaEvent {
    /// The browse or device the event came for.
    pub fn origin(&self) -> Origin {
        match self {
            IcaEvent::DeviceAdded { browse, .. }
            | IcaEvent::DeviceRemoved { browse, .. }
            | IcaEvent::LocalEnumerated { browse } => Origin::Browse(*browse),
            IcaEvent::SessionOpened { device, .. }
            | IcaEvent::SessionClosed { device, .. }
            | IcaEvent::Ready { device }
            | IcaEvent::Removed { device }
            | IcaEvent::UnitSelected { device, .. }
            | IcaEvent::Band { device, .. }
            | IcaEvent::OversizedBand { device, .. }
            | IcaEvent::ScanCompleted { device, .. }
            | IcaEvent::DeviceError { device, .. }
            | IcaEvent::Status { device, .. } => Origin::Device(*device),
            IcaEvent::Panicked { origin, .. } => *origin,
        }
    }
}

/// What a batch sets on the selected unit, in the order the glue sets it: a feeder's duplex, the
/// pixel type then the bit depth, the resolution, the paper (a flatbed's area or a feeder's
/// document type), a feeder's odd and even pages' orientation.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct UnitWrite {
    /// A feeder's `duplexScanningEnabled`; `None` for a flatbed.
    pub duplex: Option<bool>,
    /// `pixelDataType`.
    pub pixel_type: u64,
    /// `bitDepth`.
    pub bit_depth: u64,
    /// `resolution`, in dots per inch.
    pub resolution: u64,
    /// A flatbed's `scanArea`, in its measurement unit; `None` for a feeder.
    pub scan_area: Option<UnitRect>,
    /// A feeder's `documentType`; `None` for a flatbed, and for a feeder supporting no type whose
    /// size the backend states.
    pub document_type: Option<u64>,
    /// A feeder's `oddPageOrientation` and `evenPageOrientation`; `None` for a flatbed.
    pub orientation: Option<u64>,
}

/// The most bytes a band may have, as the backend asks it of the device (`maxMemoryBandSize`).
pub const BAND_BYTES: u32 = 1 << 20;

/// How long the session waits for the framework, outside a caller's deadline.
///
/// Data, so the scripted framework's are milliseconds and its tests never wait out a real
/// bound.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Bounds {
    /// How long a listing, and an open looking for its device, goes on after the browse's local
    /// enumeration, for network scanners.
    pub settle: Duration,
    /// How long a request outside `open` waits for its callback (a select's answer), and a
    /// cancelled scan for its end.
    pub answer: Duration,
    /// How long a device's close waits for its session's close.
    pub close: Duration,
    /// How long a call posted to the main thread waits for the main thread to run it.
    pub post: Duration,
}

impl Bounds {
    /// The real framework's: 2 s, 30 s, 5 s and 5 s.
    pub const REAL: Bounds = Bounds {
        settle: Duration::from_secs(2),
        answer: Duration::from_secs(30),
        close: Duration::from_secs(5),
        post: Duration::from_secs(5),
    };
}

/// One process's ImageCaptureCore, as the backend uses it: the browse, the session, the units,
/// the scan, and the events its callbacks send.
///
/// The real one makes every call on the main thread (inline there, posted there from elsewhere),
/// and its callbacks feed its events; each system's browses and devices send to it alone. A call
/// that fails answers a [`ScanError`]; a request's answer comes as an event.
pub(super) trait IcaSystem: Send {
    /// The system's waits.
    fn bounds(&self) -> Bounds;

    /// A browse for scanners ([`BROWSE_MASK`]), its delegate set, started.
    fn browse(&mut self) -> Result<BrowseRef, ScanError>;

    /// The browse stopped, its delegate unset, the devices it found and nobody opened let go.
    fn stop_browse(&mut self, browse: BrowseRef);

    /// The next callback's event, waiting at most `wait` (the main run loop run meanwhile, on the
    /// main thread); `None` once that has passed with none.
    fn next_event(&mut self, wait: Duration) -> Option<IcaEvent>;

    /// The device's delegate set and its session asked for (`requestOpenSession`), answered by
    /// [`IcaEvent::SessionOpened`] then [`IcaEvent::Ready`].
    fn open_session(&mut self, device: DeviceRef) -> Result<(), ScanError>;

    /// The device's session closed (`requestCloseSession`), answered by
    /// [`IcaEvent::SessionClosed`].
    fn close_session(&mut self, device: DeviceRef);

    /// The device's unit types (`availableFunctionalUnitTypes`), at most [`MAX_UNITS`].
    fn unit_types(&mut self, device: DeviceRef) -> Result<Vec<u64>, ScanError>;

    /// The selected unit (`selectedFunctionalUnit`) read whole, or `None` where there's none.
    fn selected_unit(&mut self, device: DeviceRef) -> Result<Option<UnitProps>, ScanError>;

    /// The unit of `unit_type` asked for (`requestSelectFunctionalUnit:`), answered by
    /// [`IcaEvent::UnitSelected`].
    fn select_unit(&mut self, device: DeviceRef, unit_type: u64) -> Result<(), ScanError>;

    /// `write` set on the selected unit, in [`UnitWrite`]'s order.
    fn write_unit(&mut self, device: DeviceRef, write: &UnitWrite) -> Result<(), ScanError>;

    /// A memory-based scan asked for (`transferMode`, `maxMemoryBandSize` of `band_bytes`, then
    /// `requestScan`), answered by [`IcaEvent::Band`]s and [`IcaEvent::ScanCompleted`].
    fn request_scan(&mut self, device: DeviceRef, band_bytes: u32) -> Result<(), ScanError>;

    /// The scan cancelled (`cancelScan`), answered by [`IcaEvent::ScanCompleted`].
    fn cancel_scan(&mut self, device: DeviceRef);
}

#[cfg(feature = "fake")]
pub(crate) mod fake_ica;

#[cfg(all(test, target_os = "macos"))]
mod against_the_bindings {
    use objc2_image_capture_core::{
        ICDeviceLocationType, ICDeviceLocationTypeMask, ICDeviceType, ICEXIFOrientationType,
        ICReturnCode, ICReturnConnectionErrorCode, ICScannerBitDepth, ICScannerDocumentType,
        ICScannerFunctionalUnitType, ICScannerMeasurementUnit, ICScannerPixelDataType,
        ICScannerTransferMode,
    };

    use super::*;

    fn n(value: usize) -> u64 {
        u64::try_from(value).expect("an NSUInteger fits a u64")
    }

    fn i(value: isize) -> i64 {
        i64::try_from(value).expect("an NSInteger fits an i64")
    }

    #[test]
    fn each_constant_is_the_frameworks() {
        assert_eq!(UNIT_FLATBED, n(ICScannerFunctionalUnitType::Flatbed.0));
        assert_eq!(
            UNIT_POSITIVE_TRANSPARENCY,
            n(ICScannerFunctionalUnitType::PositiveTransparency.0)
        );
        assert_eq!(
            UNIT_NEGATIVE_TRANSPARENCY,
            n(ICScannerFunctionalUnitType::NegativeTransparency.0)
        );
        assert_eq!(
            UNIT_DOCUMENT_FEEDER,
            n(ICScannerFunctionalUnitType::DocumentFeeder.0)
        );

        assert_eq!(PIXEL_BW, n(ICScannerPixelDataType::BW.0));
        assert_eq!(PIXEL_GRAY, n(ICScannerPixelDataType::Gray.0));
        assert_eq!(PIXEL_RGB, n(ICScannerPixelDataType::RGB.0));
        assert_eq!(PIXEL_PALETTE, n(ICScannerPixelDataType::Palette.0));
        assert_eq!(PIXEL_CMY, n(ICScannerPixelDataType::CMY.0));
        assert_eq!(PIXEL_CMYK, n(ICScannerPixelDataType::CMYK.0));
        assert_eq!(PIXEL_YUV, n(ICScannerPixelDataType::YUV.0));
        assert_eq!(PIXEL_YUVK, n(ICScannerPixelDataType::YUVK.0));
        assert_eq!(PIXEL_CIEXYZ, n(ICScannerPixelDataType::CIEXYZ.0));

        assert_eq!(DEPTH_1, n(ICScannerBitDepth::Depth1Bit.0));
        assert_eq!(DEPTH_8, n(ICScannerBitDepth::Depth8Bits.0));
        assert_eq!(DEPTH_16, n(ICScannerBitDepth::Depth16Bits.0));

        assert_eq!(MEASURE_INCHES, n(ICScannerMeasurementUnit::Inches.0));
        assert_eq!(
            MEASURE_CENTIMETERS,
            n(ICScannerMeasurementUnit::Centimeters.0)
        );
        assert_eq!(MEASURE_PICAS, n(ICScannerMeasurementUnit::Picas.0));
        assert_eq!(MEASURE_POINTS, n(ICScannerMeasurementUnit::Points.0));
        assert_eq!(MEASURE_TWIPS, n(ICScannerMeasurementUnit::Twips.0));
        assert_eq!(MEASURE_PIXELS, n(ICScannerMeasurementUnit::Pixels.0));

        assert_eq!(DOCUMENT_DEFAULT, n(ICScannerDocumentType::TypeDefault.0));
        assert_eq!(DOCUMENT_A4, n(ICScannerDocumentType::TypeA4.0));
        assert_eq!(DOCUMENT_US_LETTER, n(ICScannerDocumentType::TypeUSLetter.0));
        assert_eq!(DOCUMENT_US_LEGAL, n(ICScannerDocumentType::TypeUSLegal.0));
        assert_eq!(DOCUMENT_A5, n(ICScannerDocumentType::TypeA5.0));
        assert_eq!(DOCUMENT_US_LEDGER, n(ICScannerDocumentType::TypeUSLedger.0));
        assert_eq!(
            DOCUMENT_US_EXECUTIVE,
            n(ICScannerDocumentType::TypeUSExecutive.0)
        );
        assert_eq!(DOCUMENT_A3, n(ICScannerDocumentType::TypeA3.0));
        assert_eq!(DOCUMENT_A6, n(ICScannerDocumentType::TypeA6.0));
        assert_eq!(
            DOCUMENT_US_STATEMENT,
            n(ICScannerDocumentType::TypeUSStatement.0)
        );

        assert_eq!(DEVICE_CAMERA, n(ICDeviceType::Camera.0));
        assert_eq!(DEVICE_SCANNER, n(ICDeviceType::Scanner.0));
        assert_eq!(LOCATION_LOCAL, n(ICDeviceLocationType::Local.0));
        assert_eq!(LOCATION_SHARED, n(ICDeviceLocationType::Shared.0));
        assert_eq!(LOCATION_BONJOUR, n(ICDeviceLocationType::Bonjour.0));
        assert_eq!(LOCATION_BLUETOOTH, n(ICDeviceLocationType::Bluetooth.0));
        assert_eq!(LOCATION_REMOTE, n(ICDeviceLocationTypeMask::Remote.0));

        assert_eq!(
            ORIENTATION_AS_READ,
            n(ICEXIFOrientationType::Orientation1.0)
        );
        assert_eq!(TRANSFER_FILE_BASED, n(ICScannerTransferMode::FileBased.0));
        assert_eq!(
            TRANSFER_MEMORY_BASED,
            n(ICScannerTransferMode::MemoryBased.0)
        );

        assert_eq!(
            RETURN_COMMUNICATION_TIMED_OUT,
            i(ICReturnCode::CommunicationTimedOut.0)
        );
        assert_eq!(
            RETURN_SCAN_OPERATION_CANCELED,
            i(ICReturnCode::ScanOperationCanceled.0)
        );
        assert_eq!(
            RETURN_SCANNER_IN_USE_BY_LOCAL_USER,
            i(ICReturnCode::ScannerInUseByLocalUser.0)
        );
        assert_eq!(
            RETURN_SCANNER_IN_USE_BY_REMOTE_USER,
            i(ICReturnCode::ScannerInUseByRemoteUser.0)
        );
        assert_eq!(
            RETURN_DEVICE_FAILED_TO_OPEN_SESSION,
            i(ICReturnCode::DeviceFailedToOpenSession.0)
        );
        assert_eq!(
            RETURN_DEVICE_FAILED_TO_CLOSE_SESSION,
            i(ICReturnCode::DeviceFailedToCloseSession.0)
        );
        assert_eq!(
            RETURN_SCANNER_FAILED_TO_SELECT_FUNCTIONAL_UNIT,
            i(ICReturnCode::ScannerFailedToSelectFunctionalUnit.0)
        );
        assert_eq!(
            RETURN_SCANNER_FAILED_TO_COMPLETE_SCAN,
            i(ICReturnCode::ScannerFailedToCompleteScan.0)
        );
        assert_eq!(
            RETURN_DEVICE_NEEDS_CREDENTIALS,
            i(ICReturnCode::DeviceNeedsCredentials.0)
        );
        assert_eq!(
            RETURN_DEVICE_IS_BUSY_ENUMERATING,
            i(ICReturnCode::DeviceIsBusyEnumerating.0)
        );
        assert_eq!(
            RETURN_SESSION_NOT_OPENED,
            i(ICReturnCode::SessionNotOpened.0)
        );
        assert_eq!(
            RETURN_CONNECTION_DRIVER_EXITED,
            i(ICReturnConnectionErrorCode::DriverExited.0)
        );
        assert_eq!(
            RETURN_CONNECTION_CLOSED_SESSION_SUDDENLY,
            i(ICReturnConnectionErrorCode::ClosedSessionSuddenly.0)
        );
        assert_eq!(
            RETURN_CONNECTION_EJECTED_SUDDENLY,
            i(ICReturnConnectionErrorCode::EjectedSuddenly.0)
        );
        assert_eq!(
            RETURN_CONNECTION_SESSION_ALREADY_OPEN,
            i(ICReturnConnectionErrorCode::SessionAlreadyOpen.0)
        );
        assert_eq!(
            RETURN_CONNECTION_NOT_AUTHORIZED_TO_OPEN_DEVICE,
            i(ICReturnConnectionErrorCode::NotAuthorizedToOpenDevice.0)
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_index_set_is_sorted_deduplicated_and_capped() {
        let set = IndexSet::of(&[300, 75, 150, 300]);
        assert_eq!(set.count, 3);
        assert_eq!((set.first, set.last), (75, 300));
        assert_eq!(set.indexes, [75, 150, 300]);

        let empty = IndexSet::of(&[]);
        assert_eq!(empty.count, 0);
        assert!(empty.indexes.is_empty());

        let many: Vec<u64> = (1..=5000).collect();
        let capped = IndexSet::of(&many);
        assert_eq!(capped.count, 5000);
        assert_eq!((capped.first, capped.last), (1, 5000));
        assert_eq!(capped.indexes.len(), MAX_INDEXES);
    }

    #[test]
    fn the_browse_asks_for_scanners_attached_on_the_network_or_shared() {
        assert_eq!(BROWSE_MASK, 0x0702);
        assert_eq!(BROWSE_MASK & DEVICE_CAMERA, 0);
        assert_eq!(BROWSE_MASK & LOCATION_BLUETOOTH, 0);
        // Every remote location is in the remote mask, and the local one isn't.
        for remote in [LOCATION_SHARED, LOCATION_BONJOUR, LOCATION_BLUETOOTH] {
            assert_eq!(remote & LOCATION_REMOTE, remote);
        }
        assert_eq!(LOCATION_LOCAL & LOCATION_REMOTE, 0);
    }
}
