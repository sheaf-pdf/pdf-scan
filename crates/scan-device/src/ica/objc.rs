//! The Objective-C glue: ImageCaptureCore through objc2, every call and callback on the main
//! thread, implementing [`IcaSystem`]. macOS only, and the one module of the backend allowed
//! `unsafe` (`#![allow(unsafe_code)]` under the crate's `#![deny(unsafe_code)]`), with
//! `unsafe_op_in_unsafe_fn` and `clippy::undocumented_unsafe_blocks` denied on every block.
//!
//! - **The delegate class** (`Delegate`): a subclass of `NSObject` usable on the main thread
//!   alone, implementing the device browser's, the device's and the scanner's delegate methods
//!   the backend uses. Every object it's handed is taken as an `Option`, the same encoding as a
//!   reference, so a nil where a header promises an object is `None`, never a reference made from
//!   null. Every body runs under `catch_unwind`, a panic sent as an event, never unwound into the
//!   framework. A body only reads what it's handed into an [`IcaEvent`] and sends it: it requests,
//!   sets and selects nothing, touches no registry and never blocks. A browse's delegate keeps
//!   the devices its browse found, beside the numbers the backend knows them by.
//! - **The registry:** every browser and scanner device the backend uses, each beside the
//!   delegate set on it (the framework doesn't retain its delegate), in a thread-local on the main
//!   thread, reached only through a [`MainThreadMarker`] and keyed by the numbers the rest of the
//!   backend holds. Each access is a short borrow, never held across a run of the run loop or a
//!   call into the framework; a delegate is unset before it or its object is dropped, and the
//!   registry itself is never dropped, so the process's end lets go of nothing still set.
//! - **The main thread's marker** is taken by every function here that calls the framework, so
//!   each `unsafe` call's "on the main thread" is the marker's proof, not a claim: a callback has
//!   it from its delegate's class, usable on the main thread alone.
//! - **The executor:** on the main thread a call runs at once, and a wait for a callback runs the
//!   main run loop's default mode in slices, reading each slice's answer (the rules are
//!   `main_thread`'s); elsewhere a call is posted to the main queue and its answer awaited, the
//!   handshake letting the caller give up only on a call not yet begun, and its events come on
//!   the callbacks' channel.
//! - **The real system** (`MacSystem`): one per listing backend and per open device, with the
//!   channel its delegates send to.

#![allow(unsafe_code)]

use std::cell::RefCell;
use std::collections::HashMap;
use std::mem::ManuallyDrop;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::ptr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};

use dispatch2::{DispatchQueue, DispatchTime};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, Message, define_class, msg_send};
use objc2_core_foundation::{CFRunLoop, kCFRunLoopDefaultMode};
use objc2_foundation::{
    NSArray, NSData, NSDictionary, NSError, NSIndexSet, NSNumber, NSObject, NSObjectProtocol,
    NSPoint, NSRect, NSSize, NSString,
};
use objc2_image_capture_core::{
    ICDevice, ICDeviceBrowser, ICDeviceBrowserDelegate, ICDeviceDelegate, ICDeviceTypeMask,
    ICEXIFOrientationType, ICErrorDomain, ICScannerBandData, ICScannerBitDepth, ICScannerDevice,
    ICScannerDeviceDelegate, ICScannerDocumentType, ICScannerFunctionalUnit,
    ICScannerFunctionalUnitDocumentFeeder, ICScannerFunctionalUnitType, ICScannerPixelDataType,
    ICScannerStatusWarmUpDone, ICScannerStatusWarmingUp, ICScannerTransferMode,
    ICStatusNotificationKey,
};

use super::driver::{
    BROWSE_MASK, BandHeader, Bounds, BrowseRef, DeviceProps, DeviceRef, DeviceStatus, IcaError,
    IcaEvent, IcaSystem, IndexSet, MAX_BAND_BYTES, MAX_INDEXES, MAX_UNITS, Origin,
    TRANSFER_MEMORY_BASED, UnitProps, UnitRect, UnitSize, UnitWrite,
};
use super::error::driver_msg;
use super::id::{MAX_ID_BYTES, MAX_TEXT_BYTES, capped};
use super::main_thread::{
    Handshake, MainWait, RUN_FINISHED, SLICE, Verdict, await_posted, contained, panic_message,
    run_posted, unserved_in_block,
};
use crate::ScanError;

/// The most UTF-16 code units of one string the glue copies: a longer one is taken as absent.
const MAX_STRING_UNITS: usize = 64 * 1024;

/// The most devices one browse keeps: one found past it isn't kept, nor said.
const MAX_FOUND: usize = 1024;

/// What an index set answers for no index: Foundation's `NSNotFound`, `NSIntegerMax`.
const NOT_FOUND: usize = isize::MAX.unsigned_abs();

/// The numbers browses and devices are known by, unique in the process.
static NEXT_REF: AtomicU64 = AtomicU64::new(1);

fn next_ref() -> u64 {
    NEXT_REF.fetch_add(1, Ordering::Relaxed)
}

/// An `NSUInteger` as the backend holds it.
fn n64(value: usize) -> u64 {
    u64::try_from(value).unwrap_or(u64::MAX)
}

/// An `NSInteger` as the backend holds it.
fn i64_of(value: isize) -> i64 {
    i64::try_from(value).unwrap_or(i64::MIN)
}

/// `value` as an `NSUInteger`, for a set.
fn ns_uint(value: u64, what: &str) -> Result<usize, ScanError> {
    usize::try_from(value).map_err(|_| {
        driver_msg(format!(
            "{what} {value} doesn't fit ImageCaptureCore's integers"
        ))
    })
}

/// `text` copied, capped at `max` bytes on a character boundary; `None` where it's past
/// [`MAX_STRING_UNITS`], so a hostile string is never copied whole.
fn text_of(text: &NSString, max: usize) -> Option<String> {
    (text.length() <= MAX_STRING_UNITS).then(|| capped(&text.to_string(), max))
}

/// An id copied whole, never cut (a cut one would name another device); `None` where it's past
/// [`MAX_ID_BYTES`] already in UTF-16 code units, each at least a byte in UTF-8, so the backend
/// couldn't form an id of it anyway.
fn id_text(text: &NSString) -> Option<String> {
    (text.length() <= MAX_ID_BYTES).then(|| text.to_string())
}

/// An `NSError` as [`IcaError`]: whether its domain is ImageCaptureCore's, its domain and its
/// localized description capped, its code.
fn ica_error(error: &NSError) -> IcaError {
    let domain = error.domain();
    // SAFETY: `ICErrorDomain` is an extern static the framework defines for the life of the
    // process, read once; the binding types it as an optional string, so a nil is `None`.
    let ica = unsafe { ICErrorDomain };
    IcaError {
        ica_domain: ica.is_some_and(|ica| domain.isEqualToString(ica)),
        domain: text_of(&domain, MAX_TEXT_BYTES).unwrap_or_default(),
        code: i64_of(error.code()),
        description: text_of(&error.localizedDescription(), MAX_TEXT_BYTES).unwrap_or_default(),
    }
}

/// What a device's error callback says where it hands no `NSError`.
fn no_error() -> IcaError {
    IcaError {
        description: "the scanner reported an error and gave none".to_owned(),
        ..IcaError::default()
    }
}

/// A device as [`DeviceProps`], each string capped as it's read.
fn device_props(_mtm: MainThreadMarker, device: &ICDevice) -> DeviceProps {
    // SAFETY: property reads (`persistentIDString`, `UUIDString`, `name`, `type`) of a device the
    // framework handed a callback, on the main thread (the marker), where its callbacks come and
    // its objects are used; each takes no argument, and each object it returns is typed
    // optional, so a nil is `None`.
    let (persistent, uuid, name, kind) = unsafe {
        (
            device.persistentIDString(),
            device.UUIDString(),
            device.name(),
            device.r#type(),
        )
    };
    DeviceProps {
        persistent_id: persistent.as_deref().and_then(id_text),
        uuid: uuid.as_deref().and_then(id_text),
        name: name
            .as_deref()
            .and_then(|name| text_of(name, MAX_TEXT_BYTES)),
        type_bits: n64(kind.0),
        scanner_class: device.downcast_ref::<ICScannerDevice>().is_some(),
    }
}

/// An `NSIndexSet` as [`IndexSet`]: its count, first and last read first, then at most
/// [`MAX_INDEXES`] of its indexes in order.
fn index_set(set: Option<&NSIndexSet>) -> IndexSet {
    let Some(set) = set else {
        return IndexSet::default();
    };
    let count = set.count();
    if count == 0 {
        return IndexSet::default();
    }
    let (first, last) = (set.firstIndex(), set.lastIndex());
    let mut indexes = Vec::with_capacity(count.min(MAX_INDEXES));
    let mut index = first;
    while indexes.len() < MAX_INDEXES && index != NOT_FOUND {
        indexes.push(n64(index));
        let next = set.indexGreaterThanIndex(index);
        // A set whose next index doesn't rise is read no further.
        if next <= index {
            break;
        }
        index = next;
    }
    IndexSet {
        count: n64(count),
        first: n64(first),
        last: n64(last),
        indexes,
    }
}

/// A functional unit read whole, a feeder's own properties where it's a feeder.
fn unit_props(_mtm: MainThreadMarker, unit: &ICScannerFunctionalUnit) -> UnitProps {
    // SAFETY: property reads of a functional unit the framework gave (`type`, `pixelDataType`,
    // `bitDepth`, `measurementUnit`, `resolution`, `physicalSize`, `scanArea`), on the main
    // thread (the marker), where its objects are used; each takes no argument and returns a
    // plain value.
    let (unit_type, pixel_type, bit_depth, measurement_unit, resolution, physical, area) = unsafe {
        (
            unit.r#type(),
            unit.pixelDataType(),
            unit.bitDepth(),
            unit.measurementUnit(),
            unit.resolution(),
            unit.physicalSize(),
            unit.scanArea(),
        )
    };
    // SAFETY: `supportedBitDepths` and `supportedResolutions` take no argument and return an
    // `NSIndexSet` by the header, read here as an optional so a nil is `None` rather than a panic
    // in the binding's conversion; objc2 checks the encoding in debug builds. On the main thread
    // (the marker).
    let (depths, resolutions): (Option<Retained<NSIndexSet>>, Option<Retained<NSIndexSet>>) = unsafe {
        (
            msg_send![unit, supportedBitDepths],
            msg_send![unit, supportedResolutions],
        )
    };
    let mut props = UnitProps {
        unit_type: n64(unit_type.0),
        pixel_type: n64(pixel_type.0),
        bit_depth: n64(bit_depth.0),
        bit_depths: index_set(depths.as_deref()),
        measurement_unit: n64(measurement_unit.0),
        resolution: n64(resolution),
        resolutions: index_set(resolutions.as_deref()),
        physical_size: UnitSize {
            width: physical.width,
            height: physical.height,
        },
        scan_area: UnitRect {
            x: area.origin.x,
            y: area.origin.y,
            width: area.size.width,
            height: area.size.height,
        },
        ..UnitProps::default()
    };
    if let Some(feeder) = unit.downcast_ref::<ICScannerFunctionalUnitDocumentFeeder>() {
        // SAFETY: `supportedDocumentTypes` takes no argument and returns an `NSIndexSet` by the
        // header, read as an optional as above. On the main thread (the marker).
        let types: Option<Retained<NSIndexSet>> =
            unsafe { msg_send![feeder, supportedDocumentTypes] };
        // SAFETY: property reads of a feeder (the downcast checked its class), each taking no
        // argument and returning a plain value. On the main thread (the marker).
        let (document_type, duplex_supported, duplex_enabled, loaded, reverse, odd, even) = unsafe {
            (
                feeder.documentType(),
                feeder.supportsDuplexScanning(),
                feeder.duplexScanningEnabled(),
                feeder.documentLoaded(),
                feeder.reverseFeederPageOrder(),
                feeder.oddPageOrientation(),
                feeder.evenPageOrientation(),
            )
        };
        props.document_types = index_set(types.as_deref());
        props.document_type = n64(document_type.0);
        props.duplex_supported = duplex_supported;
        props.duplex_enabled = duplex_enabled;
        props.document_loaded = loaded;
        props.reverse_feeder_order = reverse;
        props.odd_orientation = n64(odd.0);
        props.even_orientation = n64(even.0);
    }
    props
}

/// `write` set on `unit`, in [`UnitWrite`]'s order: every value one the unit listed (the settings
/// chose it so), converted before the first set.
fn write_props(
    _mtm: MainThreadMarker,
    unit: &ICScannerFunctionalUnit,
    write: &UnitWrite,
) -> Result<(), ScanError> {
    let pixel_type = ns_uint(write.pixel_type, "the pixel type")?;
    let bit_depth = ns_uint(write.bit_depth, "the bit depth")?;
    let resolution = ns_uint(write.resolution, "the resolution")?;
    let document_type = write
        .document_type
        .map(|value| ns_uint(value, "the document type"))
        .transpose()?;
    let orientation = write
        .orientation
        .map(|value| ns_uint(value, "the orientation"))
        .transpose()?;
    let feeder = unit.downcast_ref::<ICScannerFunctionalUnitDocumentFeeder>();
    if let (Some(duplex), Some(feeder)) = (write.duplex, feeder) {
        // SAFETY: a property set on a feeder (the downcast checked its class), with a plain value,
        // on the main thread (the marker).
        unsafe { feeder.setDuplexScanningEnabled(duplex) };
    }
    // SAFETY: property sets of the selected unit with values of the types the binding takes, on
    // the main thread (the marker); the values are those the unit listed, so none is refused by
    // an exception.
    unsafe {
        unit.setPixelDataType(ICScannerPixelDataType(pixel_type));
        unit.setBitDepth(ICScannerBitDepth(bit_depth));
        unit.setResolution(resolution);
    }
    if let Some(area) = write.scan_area {
        let rect = NSRect::new(
            NSPoint::new(area.x, area.y),
            NSSize::new(area.width, area.height),
        );
        // SAFETY: a property set of the selected unit, an `NSRect` by value, on the main thread
        // (the marker).
        unsafe { unit.setScanArea(rect) };
    }
    if let (Some(document_type), Some(feeder)) = (document_type, feeder) {
        // SAFETY: a property set on a feeder, a type it listed, on the main thread (the marker).
        unsafe { feeder.setDocumentType(ICScannerDocumentType(document_type)) };
    }
    if let (Some(orientation), Some(feeder)) = (orientation, feeder) {
        // SAFETY: property sets on a feeder with plain values, on the main thread (the marker).
        unsafe {
            feeder.setOddPageOrientation(ICEXIFOrientationType(orientation));
            feeder.setEvenPageOrientation(ICEXIFOrientationType(orientation));
        }
    }
    Ok(())
}

/// The selected unit of `scanner`, read as an optional (the header says it's never nil, but a
/// nil was seen at a select's answer).
fn selected(
    _mtm: MainThreadMarker,
    scanner: &ICScannerDevice,
) -> Option<Retained<ICScannerFunctionalUnit>> {
    // SAFETY: `selectedFunctionalUnit` takes no argument and returns an
    // `ICScannerFunctionalUnit` by the header, read as an optional so a nil is `None` rather than
    // a panic in the binding's conversion; objc2 checks the encoding in debug builds. On the main
    // thread (the marker).
    unsafe { msg_send![scanner, selectedFunctionalUnit] }
}

/// A band as [`IcaEvent::Band`], its buffer's length read before it's copied, and one past
/// [`MAX_BAND_BYTES`] said as [`IcaEvent::OversizedBand`] uncopied.
fn band_event(_mtm: MainThreadMarker, device: DeviceRef, data: &ICScannerBandData) -> IcaEvent {
    // SAFETY: property reads of a band the framework handed the callback, on the main thread (the
    // marker); each takes no argument and returns a plain value, every one checked before a byte
    // is read (`pixels::PageAssembler`).
    let header = unsafe {
        BandHeader {
            full_width: n64(data.fullImageWidth()),
            full_height: n64(data.fullImageHeight()),
            bits_per_pixel: n64(data.bitsPerPixel()),
            bits_per_component: n64(data.bitsPerComponent()),
            components: n64(data.numComponents()),
            big_endian: data.isBigEndian(),
            pixel_type: n64(data.pixelDataType().0),
            bytes_per_row: n64(data.bytesPerRow()),
            start_row: n64(data.dataStartRow()),
            rows: n64(data.dataNumRows()),
            data_size: n64(data.dataSize()),
        }
    };
    // SAFETY: a property read of the band, taking no argument, typed optional so a nil is `None`;
    // on the main thread (the marker).
    let buffer: Option<Retained<NSData>> = unsafe { data.dataBuffer() };
    let bytes = match buffer {
        None => Vec::new(),
        Some(buffer) => {
            let len = n64(buffer.length());
            if len > MAX_BAND_BYTES {
                return IcaEvent::OversizedBand { device, len };
            }
            buffer.to_vec()
        }
    };
    IcaEvent::Band {
        device,
        header,
        bytes,
    }
}

/// A device's status as the backend names it: the value of the status's
/// `ICStatusNotificationKey`, compared with the two the backend reads.
fn status_of(status: &NSDictionary<NSString, AnyObject>) -> DeviceStatus {
    // SAFETY: extern statics the framework defines for the life of the process, each a string
    // the binding types as present, read once.
    let (key, warming, warmed) = unsafe {
        (
            ICStatusNotificationKey,
            ICScannerStatusWarmingUp,
            ICScannerStatusWarmUpDone,
        )
    };
    let Some(value) = status.objectForKey(key) else {
        return DeviceStatus::Other;
    };
    match value.downcast::<NSString>() {
        Ok(value) if value.isEqualToString(warming) => DeviceStatus::WarmingUp,
        Ok(value) if value.isEqualToString(warmed) => DeviceStatus::WarmedUp,
        _ => DeviceStatus::Other,
    }
}

/// Whose delegate one is.
#[derive(Clone, Copy, Debug)]
enum Role {
    Browse(BrowseRef),
    Device(DeviceRef),
}

impl Role {
    fn origin(self) -> Origin {
        match self {
            Role::Browse(browse) => Origin::Browse(browse),
            Role::Device(device) => Origin::Device(device),
        }
    }
}

/// A delegate's state: whose it is, where its events go, and a browse's devices found.
struct Ivars {
    role: Role,
    events: Sender<IcaEvent>,
    /// A browse's devices, each kept while its browse is, beside the number the backend knows it
    /// by; a device's delegate keeps none.
    found: RefCell<Vec<(DeviceRef, Retained<ICDevice>)>>,
}

define_class!(
    // SAFETY: `NSObject` has no subclassing requirements, and `Delegate` implements no `Drop`
    // (its ivars are dropped by objc2's `dealloc`). It's usable on the main thread alone, where
    // the framework calls it.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[ivars = Ivars]
    struct Delegate;

    // SAFETY: `NSObjectProtocol` asks nothing of a subclass of `NSObject`.
    unsafe impl NSObjectProtocol for Delegate {}

    // SAFETY: each method has its selector's signature as the header gives it, every object
    // parameter taken as an `Option` (the same encoding as a reference) and `BOOL` as `bool`
    // (objc2 converts it); each body catches its own panic and returns nothing.
    unsafe impl ICDeviceBrowserDelegate for Delegate {
        #[unsafe(method(deviceBrowser:didAddDevice:moreComing:))]
        fn browser_added(
            &self,
            _browser: Option<&ICDeviceBrowser>,
            device: Option<&ICDevice>,
            _more_coming: bool,
        ) {
            self.guarded(|mtm, ivars| {
                let Role::Browse(browse) = ivars.role else {
                    return None;
                };
                let device = device?;
                // Read before the borrow, so no framework call is made inside it.
                let props = device_props(mtm, device);
                let mut found = ivars.found.borrow_mut();
                if found.len() >= MAX_FOUND {
                    return None;
                }
                let reference = DeviceRef(next_ref());
                found.push((reference, device.retain()));
                Some(IcaEvent::DeviceAdded {
                    browse,
                    device: reference,
                    props,
                })
            });
        }

        #[unsafe(method(deviceBrowser:didRemoveDevice:moreGoing:))]
        fn browser_removed(
            &self,
            _browser: Option<&ICDeviceBrowser>,
            device: Option<&ICDevice>,
            _more_going: bool,
        ) {
            self.guarded(|_, ivars| {
                let Role::Browse(browse) = ivars.role else {
                    return None;
                };
                let device = device?;
                let removed = {
                    let mut found = ivars.found.borrow_mut();
                    let at = found
                        .iter()
                        .position(|(_, known)| ptr::eq(&**known, device))?;
                    found.remove(at)
                };
                // Let go of outside the borrow.
                let (reference, _object) = removed;
                Some(IcaEvent::DeviceRemoved {
                    browse,
                    device: reference,
                })
            });
        }

        #[unsafe(method(deviceBrowserDidEnumerateLocalDevices:))]
        fn browser_enumerated(&self, _browser: Option<&ICDeviceBrowser>) {
            self.guarded(|_, ivars| match ivars.role {
                Role::Browse(browse) => Some(IcaEvent::LocalEnumerated { browse }),
                Role::Device(_) => None,
            });
        }
    }

    // SAFETY: as for the browser's delegate methods above.
    unsafe impl ICDeviceDelegate for Delegate {
        #[unsafe(method(device:didCloseSessionWithError:))]
        fn device_closed(&self, _device: Option<&ICDevice>, error: Option<&NSError>) {
            self.on_device(|_, device| IcaEvent::SessionClosed {
                device,
                error: error.map(ica_error),
            });
        }

        #[unsafe(method(didRemoveDevice:))]
        fn device_removed(&self, _device: Option<&ICDevice>) {
            self.on_device(|_, device| IcaEvent::Removed { device });
        }

        #[unsafe(method(device:didOpenSessionWithError:))]
        fn device_opened(&self, _device: Option<&ICDevice>, error: Option<&NSError>) {
            self.on_device(|_, device| IcaEvent::SessionOpened {
                device,
                error: error.map(ica_error),
            });
        }

        #[unsafe(method(deviceDidBecomeReady:))]
        fn device_ready(&self, _device: Option<&ICDevice>) {
            self.on_device(|_, device| IcaEvent::Ready { device });
        }

        #[unsafe(method(device:didReceiveStatusInformation:))]
        fn device_status(
            &self,
            _device: Option<&ICDevice>,
            status: Option<&NSDictionary<NSString, AnyObject>>,
        ) {
            self.on_device(|_, device| IcaEvent::Status {
                device,
                status: status.map_or(DeviceStatus::Other, status_of),
            });
        }

        #[unsafe(method(device:didEncounterError:))]
        fn device_error(&self, _device: Option<&ICDevice>, error: Option<&NSError>) {
            self.on_device(|_, device| IcaEvent::DeviceError {
                device,
                error: error.map_or_else(no_error, ica_error),
            });
        }
    }

    // SAFETY: as for the browser's delegate methods above.
    unsafe impl ICScannerDeviceDelegate for Delegate {
        #[unsafe(method(scannerDevice:didSelectFunctionalUnit:error:))]
        fn scanner_selected(
            &self,
            _scanner: Option<&ICScannerDevice>,
            unit: Option<&ICScannerFunctionalUnit>,
            error: Option<&NSError>,
        ) {
            self.on_device(|mtm, device| IcaEvent::UnitSelected {
                device,
                unit: unit.map(|unit| Box::new(unit_props(mtm, unit))),
                error: error.map(ica_error),
            });
        }

        #[unsafe(method(scannerDevice:didScanToBandData:))]
        fn scanner_band(
            &self,
            _scanner: Option<&ICScannerDevice>,
            data: Option<&ICScannerBandData>,
        ) {
            // A band callback handing no band says nothing to take.
            if let Some(data) = data {
                self.on_device(|mtm, device| band_event(mtm, device, data));
            }
        }

        #[unsafe(method(scannerDevice:didCompleteScanWithError:))]
        fn scanner_completed(&self, _scanner: Option<&ICScannerDevice>, error: Option<&NSError>) {
            self.on_device(|_, device| IcaEvent::ScanCompleted {
                device,
                error: error.map(ica_error),
            });
        }
    }
);

impl Delegate {
    fn new(mtm: MainThreadMarker, role: Role, events: Sender<IcaEvent>) -> Retained<Delegate> {
        let this = Delegate::alloc(mtm).set_ivars(Ivars {
            role,
            events,
            found: RefCell::new(Vec::new()),
        });
        // SAFETY: `NSObject`'s designated initializer, sent once to a freshly allocated object.
        unsafe { msg_send![super(this), init] }
    }

    /// A callback's body: run under `catch_unwind`, its event (or its panic, as
    /// [`IcaEvent::Panicked`]) sent; nothing unwinds into the framework, a payload whose drop
    /// panics included.
    fn guarded(&self, body: impl FnOnce(MainThreadMarker, &Ivars) -> Option<IcaEvent>) {
        contained(|| {
            let ivars = self.ivars();
            let event = match catch_unwind(AssertUnwindSafe(|| body(self.mtm(), ivars))) {
                Ok(event) => event,
                Err(payload) => Some(IcaEvent::Panicked {
                    origin: ivars.role.origin(),
                    message: panic_message(payload),
                }),
            };
            if let Some(event) = event {
                let _ = ivars.events.send(event);
            }
        });
    }

    /// A device's callback: its event made where this is a device's delegate.
    fn on_device(&self, event: impl FnOnce(MainThreadMarker, DeviceRef) -> IcaEvent) {
        self.guarded(|mtm, ivars| match ivars.role {
            Role::Device(device) => Some(event(mtm, device)),
            Role::Browse(_) => None,
        });
    }
}

/// A browse: the browser, and the delegate set on it, kept while it is.
struct BrowseEntry {
    browser: Retained<ICDeviceBrowser>,
    delegate: Retained<Delegate>,
}

/// A device whose session the backend asked for: the device, the browse that found it, and the
/// delegate set on it, kept while it is.
struct DeviceEntry {
    browse: BrowseRef,
    device: Retained<ICScannerDevice>,
    #[expect(
        dead_code,
        reason = "held, never read: the device doesn't retain the delegate set on it"
    )]
    delegate: Retained<Delegate>,
}

#[derive(Default)]
struct Registry {
    browses: HashMap<BrowseRef, BrowseEntry>,
    devices: HashMap<DeviceRef, DeviceEntry>,
}

thread_local! {
    /// The main thread's registry; another thread's is never reached, every access taking a
    /// [`MainThreadMarker`]. Never dropped: what's in it as the process ends is left to the
    /// system, so no delegate is let go there while it's still set on its object.
    static REGISTRY: ManuallyDrop<RefCell<Registry>> = ManuallyDrop::new(RefCell::new(Registry::default()));
}

/// `access` given the registry, on the main thread: a short borrow, which takes out or clones
/// what it needs and lets go, never calling the framework or running the run loop inside it.
fn with_registry<T>(_mtm: MainThreadMarker, access: impl FnOnce(&mut Registry) -> T) -> T {
    REGISTRY.with(|registry| access(&mut registry.borrow_mut()))
}

/// The scanner device whose session the backend asked for, as [`DeviceRef`] `device` names it.
fn opened(
    mtm: MainThreadMarker,
    device: DeviceRef,
) -> Result<Retained<ICScannerDevice>, ScanError> {
    with_registry(mtm, |registry| {
        registry
            .devices
            .get(&device)
            .map(|entry| entry.device.clone())
    })
    .ok_or_else(|| driver_msg("the backend has no open session with that device"))
}

/// The device a browse found as `device`, and that browse.
fn found(mtm: MainThreadMarker, device: DeviceRef) -> Option<(BrowseRef, Retained<ICDevice>)> {
    let delegates: Vec<(BrowseRef, Retained<Delegate>)> = with_registry(mtm, |registry| {
        registry
            .browses
            .iter()
            .map(|(browse, entry)| (*browse, entry.delegate.clone()))
            .collect()
    });
    delegates.into_iter().find_map(|(browse, delegate)| {
        let found = delegate.ivars().found.try_borrow().ok()?;
        found
            .iter()
            .find(|(known, _)| *known == device)
            .map(|(_, object)| (browse, object.clone()))
    })
}

/// A browse started on the main thread: its delegate made and kept beside it, set, then the
/// browser started.
fn start_browse(mtm: MainThreadMarker, events: Sender<IcaEvent>) -> Result<BrowseRef, ScanError> {
    let mask = ICDeviceTypeMask(ns_uint(BROWSE_MASK, "the browse's mask")?);
    let browse = BrowseRef(next_ref());
    let delegate = Delegate::new(mtm, Role::Browse(browse), events);
    // SAFETY: `ICDeviceBrowser`'s `new` (`alloc` then its designated initializer), taking no
    // argument, on the main thread (the marker).
    let browser = unsafe { ICDeviceBrowser::new() };
    with_registry(mtm, |registry| {
        registry.browses.insert(
            browse,
            BrowseEntry {
                browser: browser.clone(),
                delegate: delegate.clone(),
            },
        )
    });
    // SAFETY: the delegate is unretained by the browser, and is kept alive beside it in the
    // registry until it's unset (`end_browse`); the mask and the start take plain values. On the
    // main thread (the marker), where the browser's callbacks come.
    unsafe {
        browser.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
        browser.setBrowsedDeviceTypeMask(mask);
        browser.start();
    }
    Ok(browse)
}

/// A browse ended on the main thread: the delegates of the devices it found and the backend
/// opened unset, then its own, the browser stopped, and all of them let go.
fn end_browse(mtm: MainThreadMarker, browse: BrowseRef) {
    let (entry, devices) = with_registry(mtm, |registry| {
        let entry = registry.browses.remove(&browse);
        let keys: Vec<DeviceRef> = registry
            .devices
            .iter()
            .filter(|(_, device)| device.browse == browse)
            .map(|(key, _)| *key)
            .collect();
        let devices: Vec<DeviceEntry> = keys
            .iter()
            .filter_map(|key| registry.devices.remove(key))
            .collect();
        (entry, devices)
    });
    // Outside the borrow, from here: each delegate unset before it or its object is let go.
    for device in &devices {
        // SAFETY: the device's delegate unset, on the main thread (the marker), while both are
        // still kept (`devices` drops them after), so it's never let go while set.
        unsafe { device.device.setDelegate(None) };
    }
    if let Some(entry) = &entry {
        // SAFETY: the browser's delegate unset while both are kept (`entry` drops them after), so
        // it's never let go while set, then the browser stopped; on the main thread (the
        // marker).
        unsafe {
            entry.browser.setDelegate(None);
            entry.browser.stop();
        }
    }
    drop(devices);
    drop(entry);
}

/// `work` posted to the main queue, any panic in it caught there (the queue runs it inside a C
/// function, where an unwind would abort the process). Never run where the main thread never
/// serves its queue: it's leaked then, with what it holds.
fn post(work: impl FnOnce() + Send + 'static) {
    DispatchQueue::main().exec_async(move || contained(work));
}

/// `work` posted to the main queue to run once `delay` has passed.
fn post_after(delay: Duration, work: impl FnOnce() + Send + 'static) {
    let work = move || contained(work);
    match DispatchTime::try_from(delay) {
        Ok(when) => {
            let _ = DispatchQueue::main().after(when, work);
        }
        Err(()) => DispatchQueue::main().exec_async(work),
    }
}

/// `work` given the main thread's marker, where the main queue ran it on the main thread, as it
/// always does.
fn on_main<T>(work: impl FnOnce(MainThreadMarker) -> Result<T, ScanError>) -> Result<T, ScanError> {
    match MainThreadMarker::new() {
        Some(mtm) => work(mtm),
        None => Err(driver_msg(
            "the main queue ran the backend's call off the main thread",
        )),
    }
}

/// One slice of the main run loop's default mode, at most `slice` long, returning after a source
/// is handled; its answer as Core Foundation numbers it.
fn pump(_mtm: MainThreadMarker, slice: Duration) -> i32 {
    // SAFETY: `kCFRunLoopDefaultMode` is an extern static Core Foundation defines for the life of
    // the process, read once; typed optional, so a nil is `None`.
    let Some(mode) = (unsafe { kCFRunLoopDefaultMode }) else {
        // No default mode to run: nothing can be served.
        return RUN_FINISHED;
    };
    CFRunLoop::run_in_mode(Some(mode), slice.as_secs_f64(), true).0
}

/// ImageCaptureCore as one listing backend's, or one open device's: the channel its delegates send
/// to, and the browses it started and hasn't stopped.
pub(super) struct MacSystem {
    events: Receiver<IcaEvent>,
    sender: Sender<IcaEvent>,
    bounds: Bounds,
    browses: Vec<BrowseRef>,
    /// Whether a session's close was posted off the main thread and not waited for, so its
    /// browse's end is posted to come after the close has had its time.
    closing: bool,
}

impl MacSystem {
    /// A system of its own, with the real bounds. It reaches nothing as it's made.
    pub(super) fn new() -> MacSystem {
        let (sender, events) = mpsc::channel();
        MacSystem {
            events,
            sender,
            bounds: Bounds::REAL,
            browses: Vec::new(),
            closing: false,
        }
    }

    /// `body` run on the main thread: at once there, else posted to the main queue and its answer
    /// awaited at most [`Bounds::post`], never run late where it's given up on.
    fn call<T: Send + 'static>(
        &self,
        body: impl FnOnce(MainThreadMarker) -> Result<T, ScanError> + Send + 'static,
    ) -> Result<T, ScanError> {
        if let Some(mtm) = MainThreadMarker::new() {
            return body(mtm);
        }
        let handshake = Arc::new(Handshake::new());
        let (answer, answered) = mpsc::sync_channel(1);
        let posted = Arc::clone(&handshake);
        post(move || run_posted(&posted, &answer, || on_main(body)));
        await_posted(&handshake, &answered, self.bounds.post)
    }

    /// The browse ended: at once on the main thread; posted and not waited for elsewhere, after
    /// the close's bound where a session's close was posted before it.
    fn end(&self, browse: BrowseRef) {
        if let Some(mtm) = MainThreadMarker::new() {
            end_browse(mtm, browse);
            return;
        }
        let work = move || {
            let _ = on_main(|mtm| {
                end_browse(mtm, browse);
                Ok(())
            });
        };
        if self.closing {
            post_after(self.bounds.close, work);
        } else {
            post(work);
        }
    }

    /// On the main thread: the next event, the main run loop run in slices meanwhile, at most
    /// `wait`; refused at once where the main queue isn't served there (`main_thread`'s rules).
    fn pumped(&self, mtm: MainThreadMarker, wait: Duration) -> Result<Option<IcaEvent>, ScanError> {
        if let Ok(event) = self.events.try_recv() {
            return Ok(Some(event));
        }
        let end = Instant::now().checked_add(wait);
        let probe = Arc::new(AtomicBool::new(false));
        {
            let probe = Arc::clone(&probe);
            post(move || probe.store(true, Ordering::Release));
        }
        let mut judge = MainWait::new();
        loop {
            let left = end.map_or(SLICE, |end| end.saturating_duration_since(Instant::now()));
            let answer = pump(mtm, left.min(SLICE));
            if let Ok(event) = self.events.try_recv() {
                return Ok(Some(event));
            }
            if judge.after_slice(answer, probe.load(Ordering::Acquire)) == Verdict::Refuse {
                return Err(unserved_in_block());
            }
            if end.is_some_and(|end| Instant::now() >= end) {
                return Ok(None);
            }
        }
    }
}

impl Drop for MacSystem {
    fn drop(&mut self) {
        for browse in std::mem::take(&mut self.browses) {
            self.end(browse);
        }
    }
}

impl IcaSystem for MacSystem {
    fn bounds(&self) -> Bounds {
        self.bounds
    }

    fn browse(&mut self) -> Result<BrowseRef, ScanError> {
        let events = self.sender.clone();
        let browse = self.call(move |mtm| start_browse(mtm, events))?;
        self.browses.push(browse);
        Ok(browse)
    }

    fn stop_browse(&mut self, browse: BrowseRef) {
        let before = self.browses.len();
        self.browses.retain(|known| *known != browse);
        if self.browses.len() != before {
            self.end(browse);
        }
    }

    fn next_event(&mut self, wait: Duration) -> Result<Option<IcaEvent>, ScanError> {
        match MainThreadMarker::new() {
            Some(mtm) => self.pumped(mtm, wait),
            None => Ok(self.events.recv_timeout(wait).ok()),
        }
    }

    fn open_session(&mut self, device: DeviceRef) -> Result<(), ScanError> {
        let events = self.sender.clone();
        self.call(move |mtm| {
            let (browse, object) = found(mtm, device).ok_or_else(|| {
                driver_msg("ImageCaptureCore no longer holds the device its browse found")
            })?;
            let scanner = object
                .downcast::<ICScannerDevice>()
                .map_err(|_| driver_msg("the device ImageCaptureCore found isn't a scanner"))?;
            let delegate = Delegate::new(mtm, Role::Device(device), events);
            let replaced = with_registry(mtm, |registry| {
                registry.devices.insert(
                    device,
                    DeviceEntry {
                        browse,
                        device: scanner.clone(),
                        delegate: delegate.clone(),
                    },
                )
            });
            // SAFETY: the delegate is unretained by the device, and is kept alive beside it in the
            // registry until it's unset (`end_browse`), and one it replaces is let go only after;
            // then the session asked for. On the main thread (the marker), where the device's
            // callbacks come.
            unsafe {
                scanner.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
                scanner.requestOpenSession();
            }
            // A delegate this one replaced on the device is let go only now it's unset.
            drop(replaced);
            Ok(())
        })
    }

    fn close_session(&mut self, device: DeviceRef) -> bool {
        let close = move |mtm| {
            let scanner = opened(mtm, device)?;
            // SAFETY: a request with no argument, on the main thread (the marker); its answer comes
            // to the device's delegate, still set.
            unsafe { scanner.requestCloseSession() };
            Ok(())
        };
        match MainThreadMarker::new() {
            Some(mtm) => close(mtm).is_ok(),
            None => {
                self.closing = true;
                post(move || {
                    let _ = on_main(close);
                });
                false
            }
        }
    }

    fn unit_types(&mut self, device: DeviceRef) -> Result<Vec<u64>, ScanError> {
        self.call(move |mtm| {
            let scanner = opened(mtm, device)?;
            // SAFETY: `availableFunctionalUnitTypes` takes no argument and returns an `NSArray`
            // (of `NSNumber`s, by the header), read as an optional array of any objects, so a nil
            // is `None` and each element is checked by its downcast; objc2 checks the encoding in
            // debug builds. On the main thread (the marker).
            let types: Option<Retained<NSArray>> =
                unsafe { msg_send![&*scanner, availableFunctionalUnitTypes] };
            let Some(types) = types else {
                return Ok(Vec::new());
            };
            let count = types.count().min(MAX_UNITS);
            Ok((0..count)
                .filter_map(|index| types.objectAtIndex(index).downcast::<NSNumber>().ok())
                .map(|number| n64(number.unsignedIntegerValue()))
                .collect())
        })
    }

    fn selected_unit(&mut self, device: DeviceRef) -> Result<Option<UnitProps>, ScanError> {
        self.call(move |mtm| {
            let scanner = opened(mtm, device)?;
            Ok(selected(mtm, &scanner).map(|unit| unit_props(mtm, &unit)))
        })
    }

    fn select_unit(&mut self, device: DeviceRef, unit_type: u64) -> Result<(), ScanError> {
        let unit_type = ns_uint(unit_type, "the functional unit type")?;
        self.call(move |mtm| {
            let scanner = opened(mtm, device)?;
            // SAFETY: a request taking a unit type by value, on the main thread (the marker); its
            // answer comes to the device's delegate, still set.
            unsafe { scanner.requestSelectFunctionalUnit(ICScannerFunctionalUnitType(unit_type)) };
            Ok(())
        })
    }

    fn write_unit(&mut self, device: DeviceRef, write: &UnitWrite) -> Result<(), ScanError> {
        let write = write.clone();
        self.call(move |mtm| {
            let scanner = opened(mtm, device)?;
            let unit = selected(mtm, &scanner)
                .ok_or_else(|| driver_msg("the scanner has no functional unit selected"))?;
            write_props(mtm, &unit, &write)
        })
    }

    fn request_scan(&mut self, device: DeviceRef, band_bytes: u32) -> Result<(), ScanError> {
        let mode = ICScannerTransferMode(ns_uint(TRANSFER_MEMORY_BASED, "the transfer mode")?);
        self.call(move |mtm| {
            let scanner = opened(mtm, device)?;
            // SAFETY: property sets with plain values, then a request with no argument, on the
            // main thread (the marker); the bands and the scan's end come to the device's
            // delegate, still set.
            unsafe {
                scanner.setTransferMode(mode);
                scanner.setMaxMemoryBandSize(band_bytes);
                scanner.requestScan();
            }
            Ok(())
        })
    }

    fn cancel_scan(&mut self, device: DeviceRef) {
        let _ = self.call(move |mtm| {
            let scanner = opened(mtm, device)?;
            // SAFETY: a request with no argument, on the main thread (the marker); its answer comes
            // to the device's delegate, still set.
            unsafe { scanner.cancelScan() };
            Ok(())
        });
    }
}

#[cfg(test)]
mod tests {
    use objc2_core_foundation::CFRunLoopRunResult;

    use super::super::main_thread::{RUN_HANDLED_SOURCE, RUN_STOPPED, RUN_TIMED_OUT};
    use super::*;

    #[test]
    fn the_run_loop_s_answers_are_core_foundation_s() {
        assert_eq!(RUN_FINISHED, CFRunLoopRunResult::Finished.0);
        assert_eq!(RUN_STOPPED, CFRunLoopRunResult::Stopped.0);
        assert_eq!(RUN_TIMED_OUT, CFRunLoopRunResult::TimedOut.0);
        assert_eq!(RUN_HANDLED_SOURCE, CFRunLoopRunResult::HandledSource.0);
    }

    #[test]
    fn numbers_are_unique_and_integers_fit() {
        let (a, b) = (next_ref(), next_ref());
        assert_ne!(a, b);
        assert_eq!(n64(usize::MAX), u64::MAX);
        assert_eq!(i64_of(-9924), -9924);
        assert_eq!(ns_uint(3, "a unit"), Ok(3));
    }
}
