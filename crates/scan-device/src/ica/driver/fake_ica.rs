//! A scripted ImageCaptureCore: devices found by a browse, their sessions, their functional
//! units and the selects of them, and a feeder's stack of sheets scanned into bands, each call
//! answered with the events the framework would send, in order, as its documentation reads.
//!
//! Our model of the framework, not evidence of it: it's built from the same reading of Apple's
//! documentation and the SDK's headers that the backend is, so it proves the backend's logic over
//! that reading, and a misreading would be in both. It records the calls the backend makes, and
//! each page a scanner begins to feed, so a test reads what the backend asked and when.
//!
//! Each [`FakeIcaSystem`] has a queue of its own, as each real system has its callbacks' channel:
//! an event is queued with the time it's due, and handed over by `next_event` once it is. A scan
//! queues a page's bands only once the page before has been taken, as a scanner reads while it's
//! read from; a browse stopped keeps what it had queued, as a main queue would.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use super::{
    BandHeader, Bounds, BrowseRef, DEPTH_1, DEPTH_8, DEPTH_16, DEVICE_SCANNER, DeviceProps,
    DeviceRef, DeviceStatus, IcaError, IcaEvent, IcaSystem, IndexSet, LOCATION_BONJOUR,
    LOCATION_LOCAL, MAX_UNITS, MEASURE_INCHES, Origin, PIXEL_BW, PIXEL_GRAY, PIXEL_RGB,
    RETURN_SCAN_OPERATION_CANCELED, RETURN_SCANNER_FAILED_TO_COMPLETE_SCAN,
    RETURN_SCANNER_FAILED_TO_SELECT_FUNCTIONAL_UNIT, UNIT_DOCUMENT_FEEDER, UNIT_FLATBED, UnitProps,
    UnitRect, UnitSize, UnitWrite,
};
use crate::fake::pattern;
use crate::ica::caps::{document_size, length_mm};
use crate::ica::pixels::BW_ZERO_IS_BLACK;
use crate::{Format, ScanError, Side};

impl Bounds {
    /// The scripted framework's: a few milliseconds each, so no test waits out a real bound.
    pub const FAKE: Bounds = Bounds {
        settle: Duration::from_millis(20),
        answer: Duration::from_millis(200),
        close: Duration::from_millis(20),
        post: Duration::from_millis(20),
    };
}

/// The byte a band's row padding is filled with, so a test sees it dropped.
const PADDING_BYTE: u8 = 0xa5;

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// An `NSError` in ImageCaptureCore's domain with `code`.
pub fn ica_error(code: i64) -> IcaError {
    IcaError {
        ica_domain: true,
        domain: "com.apple.ImageCaptureCore".to_owned(),
        code,
        description: format!("The scripted framework's error {code}."),
    }
}

/// A local scanner's props: its persistent id, its name, attached to this Mac.
pub fn scanner_props(persistent_id: &str, name: &str) -> DeviceProps {
    DeviceProps {
        persistent_id: Some(persistent_id.to_owned()),
        uuid: Some(format!("UUID-{persistent_id}")),
        name: Some(name.to_owned()),
        type_bits: DEVICE_SCANNER | LOCATION_LOCAL,
        scanner_class: true,
    }
}

/// A scanner found on the network by Bonjour.
pub fn network_props(persistent_id: &str, name: &str) -> DeviceProps {
    DeviceProps {
        type_bits: DEVICE_SCANNER | LOCATION_BONJOUR,
        ..scanner_props(persistent_id, name)
    }
}

/// A flatbed of `width` by `height` inches, at `resolutions` (the first selected), in `depths`
/// (8 bits gray selected), its whole bed its scan area.
pub fn flatbed(resolutions: &[u64], depths: &[u64], width: f64, height: f64) -> UnitProps {
    let size = UnitSize { width, height };
    UnitProps {
        unit_type: UNIT_FLATBED,
        pixel_type: PIXEL_GRAY,
        bit_depth: DEPTH_8,
        bit_depths: IndexSet::of(depths),
        measurement_unit: MEASURE_INCHES,
        resolution: resolutions.first().copied().unwrap_or(0),
        resolutions: IndexSet::of(resolutions),
        physical_size: size,
        scan_area: UnitRect {
            x: 0.0,
            y: 0.0,
            width,
            height,
        },
        ..UnitProps::default()
    }
}

/// A document feeder taking `document_types` (the first selected), at `resolutions`, in
/// `depths`, scanning both sides where `duplex`; its largest sheet `width` by `height` inches.
pub fn feeder(
    resolutions: &[u64],
    depths: &[u64],
    document_types: &[u64],
    duplex: bool,
    width: f64,
    height: f64,
) -> UnitProps {
    UnitProps {
        unit_type: UNIT_DOCUMENT_FEEDER,
        document_types: IndexSet::of(document_types),
        document_type: document_types.first().copied().unwrap_or(0),
        duplex_supported: duplex,
        odd_orientation: 1,
        even_orientation: 1,
        ..flatbed(resolutions, depths, width, height)
    }
}

/// When the select the framework makes as a device is instantiated is answered to the device's
/// delegate, once its session is asked for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unasked {
    /// Before `deviceDidBecomeReady:`.
    BeforeReady,
    /// After it.
    AfterReady,
    /// Never seen by the delegate (it came before the delegate was set).
    Never,
}

/// How a session asked for is answered.
#[derive(Clone, Debug, PartialEq)]
pub enum FakeSession {
    /// Opened, then ready.
    Opens,
    /// Refused with this error.
    Refused(IcaError),
    /// Opened, and never ready.
    NeverReady,
    /// Never answered.
    NeverAnswered,
}

/// How a select of a unit is answered.
#[derive(Clone, Debug, PartialEq)]
pub enum FakeSelect {
    /// With the unit.
    Answers,
    /// With no unit and no error (the unit selected all the same).
    Nil,
    /// With this error, nothing selected.
    Refused(IcaError),
    /// Never.
    NeverAnswered,
}

/// What goes wrong at one side of a sheet.
#[derive(Clone, Debug, PartialEq)]
pub enum FakeFault {
    /// The scan ends with this error instead of the page; at a front, the sheet stays in the
    /// feeder, its fault cleared.
    Error(IcaError),
    /// The scanner's own cancel (its button): the scan ends cancelled instead of the page.
    ScannerCancel,
    /// The device is removed after this many of the page's bands.
    Removed {
        /// The bands sent first.
        after_bands: u32,
    },
    /// This band of the page (from 0) says it starts a row past where it does.
    LyingBand {
        /// The band.
        band: u32,
    },
    /// A callback panics after this many of the page's bands, that band lost.
    Panic {
        /// The bands sent first.
        after_bands: u32,
    },
    /// The scanner says it's warming up after the page's first band, and warmed up after its
    /// last.
    WarmingUp,
    /// The scan ends, with no error, after this page.
    EndAfter,
}

/// A sheet in a feeder: the number its pages are drawn as, and a fault at one of its sides.
#[derive(Clone, Debug, PartialEq)]
pub struct FakeSheet {
    /// The sheet `fake::pattern` draws its pages as.
    pub number: u32,
    /// What goes wrong, and at which side.
    pub fault: Option<(Side, FakeFault)>,
}

impl FakeSheet {
    /// Sheet `number`, with no fault.
    pub fn new(number: u32) -> FakeSheet {
        FakeSheet {
            number,
            fault: None,
        }
    }

    /// Sheets `numbers`, with no fault.
    pub fn stack(numbers: impl IntoIterator<Item = u32>) -> Vec<FakeSheet> {
        numbers.into_iter().map(FakeSheet::new).collect()
    }

    /// The sheet with `fault` at `side`.
    pub fn with_fault(mut self, side: Side, fault: FakeFault) -> FakeSheet {
        self.fault = Some((side, fault));
        self
    }
}

/// How a page is cut into bands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FakeBands {
    /// The rows a band, the last band the rows left.
    pub rows: u32,
    /// Bytes of padding after each row's packed bytes.
    pub padding: u64,
    /// 16-bit samples high byte first.
    pub big_endian: bool,
}

impl Default for FakeBands {
    fn default() -> FakeBands {
        FakeBands {
            rows: 16,
            padding: 0,
            big_endian: false,
        }
    }
}

/// A rule a unit's module applies after a write, so what's read back differs from what was set.
pub type WriteRule = Arc<dyn Fn(&mut UnitProps) + Send + Sync>;

/// A scripted scanner.
#[derive(Clone)]
pub struct FakeIcaDevice {
    /// Its props, as a browse finds it.
    pub props: DeviceProps,
    /// How long after a browse starts it's found.
    pub found_after: Duration,
    /// Found only as a browse stops: a callback the framework queued before the stop, delivered
    /// after it.
    pub late: bool,
    /// Its functional units, in `availableFunctionalUnitTypes`' order, as each reads.
    pub units: Vec<UnitProps>,
    /// The unit selected as the device is instantiated, the unasked select's.
    pub selected: Option<u64>,
    /// When the unasked select's answer comes.
    pub unasked: Unasked,
    /// Whether the unasked select's answer comes with no unit and no error (the nil the header
    /// says never comes, and scanline saw), the unit selected all the same.
    pub unasked_nil: bool,
    /// How its session is answered.
    pub session: FakeSession,
    /// How a select of each unit type is answered, where not [`FakeSelect::Answers`].
    pub selects: Vec<(u64, FakeSelect)>,
    /// The sheets in its feeder, the first fed first.
    pub sheets: VecDeque<FakeSheet>,
    /// The sheet its flatbed's next page is drawn as.
    pub flatbed_sheet: u32,
    /// How its pages are cut into bands.
    pub bands: FakeBands,
    /// What its module does to a unit after a write.
    pub write_rule: Option<WriteRule>,
    /// A cancel finds the next sheet already feeding: its page still comes, before the scan's
    /// end, and the sheet is gone from the feeder.
    pub feeds_ahead: bool,
    /// How a feeder's scan ends where it finds no sheet: with no error, or this one.
    pub empty_answer: Option<IcaError>,
}

impl std::fmt::Debug for FakeIcaDevice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FakeIcaDevice")
            .field("props", &self.props)
            .field("units", &self.units.len())
            .field("sheets", &self.sheets.len())
            .finish_non_exhaustive()
    }
}

impl FakeIcaDevice {
    /// A scanner of `props` with no unit, found at once, its session opening, its unasked select
    /// before it's ready.
    pub fn new(props: DeviceProps) -> FakeIcaDevice {
        FakeIcaDevice {
            props,
            found_after: Duration::ZERO,
            late: false,
            units: Vec::new(),
            selected: None,
            unasked: Unasked::BeforeReady,
            unasked_nil: false,
            session: FakeSession::Opens,
            selects: Vec::new(),
            sheets: VecDeque::new(),
            flatbed_sheet: 1,
            bands: FakeBands::default(),
            write_rule: None,
            feeds_ahead: false,
            empty_answer: None,
        }
    }

    /// The scanner with `unit`; the first unit given is the one selected as it's instantiated.
    pub fn with_unit(mut self, unit: UnitProps) -> FakeIcaDevice {
        self.selected.get_or_insert(unit.unit_type);
        self.units.push(unit);
        self
    }

    /// The scanner with `sheets` in its feeder, after those there.
    pub fn load(mut self, sheets: impl IntoIterator<Item = FakeSheet>) -> FakeIcaDevice {
        self.sheets.extend(sheets);
        self
    }

    /// The scanner whose select of `unit_type` is answered as `select` says.
    pub fn with_select(mut self, unit_type: u64, select: FakeSelect) -> FakeIcaDevice {
        self.selects.push((unit_type, select));
        self
    }

    /// The scanner whose module applies `rule` to a unit after each write.
    pub fn with_write_rule(
        mut self,
        rule: impl Fn(&mut UnitProps) + Send + Sync + 'static,
    ) -> FakeIcaDevice {
        self.write_rule = Some(Arc::new(rule));
        self
    }

    fn select_script(&self, unit_type: u64) -> FakeSelect {
        self.selects
            .iter()
            .find(|(known, _)| *known == unit_type)
            .map_or(FakeSelect::Answers, |(_, select)| select.clone())
    }
}

/// A call the backend made, or a page a scanner began to feed.
#[derive(Clone, Debug, PartialEq)]
pub enum FakeCall {
    /// A browse started.
    Browse(BrowseRef),
    /// A browse stopped.
    StopBrowse(BrowseRef),
    /// A session asked for.
    OpenSession(DeviceRef),
    /// A session closed.
    CloseSession(DeviceRef),
    /// A unit of this type asked for.
    SelectUnit(u64),
    /// Settings written on the selected unit.
    WriteUnit(UnitWrite),
    /// A memory-based scan asked for, with its band size.
    RequestScan {
        /// `maxMemoryBandSize`.
        band_bytes: u32,
    },
    /// The scan cancelled.
    CancelScan,
    /// The scanner began to feed this side of this sheet (the fake's record, not a call).
    Fed {
        /// The sheet, as drawn.
        sheet: u32,
        /// The side.
        side: Side,
    },
}

/// A scripted device and what the framework holds of it now.
struct Scripted {
    script: FakeIcaDevice,
    /// Whether a browse finds it (not unplugged).
    present: bool,
    /// Its units as they read now, written to by a session.
    units: Vec<UnitProps>,
    /// The unit selected now.
    selected: Option<u64>,
}

impl Scripted {
    fn unit(&self, unit_type: u64) -> Option<UnitProps> {
        let mut unit = self
            .units
            .iter()
            .find(|unit| unit.unit_type == unit_type)?
            .clone();
        if unit.unit_type == UNIT_DOCUMENT_FEEDER {
            unit.document_loaded = !self.script.sheets.is_empty();
        }
        Some(unit)
    }

    fn selected_unit(&self) -> Option<UnitProps> {
        self.unit(self.selected?)
    }

    fn selected_mut(&mut self) -> Option<&mut UnitProps> {
        let selected = self.selected?;
        self.units
            .iter_mut()
            .find(|unit| unit.unit_type == selected)
    }

    fn reset(&mut self) {
        self.units = self.script.units.clone();
        self.selected = self.script.selected;
    }
}

/// What every system of one handle shares: the scripts, the record, the numbers given out.
struct State {
    bounds: Bounds,
    /// When a browse's local enumeration ends, after it starts; `None` for never.
    enumeration: Option<Duration>,
    devices: Vec<Scripted>,
    next_ref: u64,
    calls: Vec<FakeCall>,
}

impl State {
    fn next_ref(&mut self) -> u64 {
        self.next_ref += 1;
        self.next_ref
    }

    fn by_id(&mut self, persistent_id: &str) -> Option<&mut Scripted> {
        self.devices
            .iter_mut()
            .find(|device| device.script.props.persistent_id.as_deref() == Some(persistent_id))
    }
}

/// The scripted framework's handle: its devices, its waits and its record, shared by every system
/// it makes.
#[derive(Clone)]
pub struct FakeIcaHandle {
    state: Arc<Mutex<State>>,
}

impl std::fmt::Debug for FakeIcaHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let state = lock(&self.state);
        f.debug_struct("FakeIcaHandle")
            .field("bounds", &state.bounds)
            .field("devices", &state.devices.len())
            .finish_non_exhaustive()
    }
}

impl Default for FakeIcaHandle {
    fn default() -> FakeIcaHandle {
        FakeIcaHandle::new()
    }
}

impl FakeIcaHandle {
    /// A framework with no device, its waits [`Bounds::FAKE`], each browse's local enumeration
    /// ending at once.
    pub fn new() -> FakeIcaHandle {
        FakeIcaHandle {
            state: Arc::new(Mutex::new(State {
                bounds: Bounds::FAKE,
                enumeration: Some(Duration::ZERO),
                devices: Vec::new(),
                next_ref: 0,
                calls: Vec::new(),
            })),
        }
    }

    /// `device` added, found by the browses that start after.
    pub fn add_device(&self, device: FakeIcaDevice) {
        let mut scripted = Scripted {
            units: Vec::new(),
            selected: None,
            present: true,
            script: device,
        };
        scripted.reset();
        lock(&self.state).devices.push(scripted);
    }

    /// The device of `persistent_id` unplugged: no browse finds it after.
    pub fn remove_device(&self, persistent_id: &str) {
        if let Some(device) = lock(&self.state).by_id(persistent_id) {
            device.present = false;
        }
    }

    /// The sheets left in the feeder of the device of `persistent_id`.
    pub fn sheets_left(&self, persistent_id: &str) -> usize {
        lock(&self.state)
            .by_id(persistent_id)
            .map_or(0, |device| device.script.sheets.len())
    }

    /// The systems' waits, for the systems made after.
    pub fn set_bounds(&self, bounds: Bounds) {
        lock(&self.state).bounds = bounds;
    }

    /// When a browse's local enumeration ends after it starts: `None` for never.
    pub fn set_enumeration(&self, after: Option<Duration>) {
        lock(&self.state).enumeration = after;
    }

    /// The calls made, and the pages fed, so far, in order.
    pub fn calls(&self) -> Vec<FakeCall> {
        lock(&self.state).calls.clone()
    }

    /// A system over this framework, with a queue of its own.
    pub(crate) fn system(&self) -> FakeIcaSystem {
        let bounds = lock(&self.state).bounds;
        FakeIcaSystem {
            state: Arc::clone(&self.state),
            bounds,
            queue: Vec::new(),
            browses: HashSet::new(),
            late: HashMap::new(),
            devices: HashMap::new(),
            sessions: HashSet::new(),
            scan: None,
        }
    }
}

/// An event waiting in a system's queue, made whole as it's handed over.
enum Pending {
    Event(IcaEvent),
    /// A select's answer: the unit selected as it's handed over, then read.
    Selected {
        device: DeviceRef,
        index: usize,
        unit_type: u64,
        nil: bool,
        error: Option<IcaError>,
    },
}

struct Queued {
    due: Instant,
    pending: Pending,
}

/// A scan going on.
struct Scan {
    device: DeviceRef,
    index: usize,
    flatbed: bool,
    duplex: bool,
    /// The side the next page is.
    next: Side,
    /// The sheet whose front was fed, its back next in duplex.
    sheet: Option<FakeSheet>,
    /// Pages fed in this scan.
    pages: u32,
}

/// One system over a [`FakeIcaHandle`]'s framework.
pub(crate) struct FakeIcaSystem {
    state: Arc<Mutex<State>>,
    bounds: Bounds,
    /// Sorted by when each is due, those due together in the order they were queued.
    queue: Vec<Queued>,
    browses: HashSet<BrowseRef>,
    /// The late devices of each browse, found as it stops.
    late: HashMap<BrowseRef, Vec<usize>>,
    /// Each device found, its browse and its script.
    devices: HashMap<DeviceRef, (BrowseRef, usize)>,
    /// The devices whose session is open, or asked for.
    sessions: HashSet<DeviceRef>,
    scan: Option<Scan>,
}

impl FakeIcaSystem {
    fn push(&mut self, due: Instant, pending: Pending) {
        let at = self.queue.partition_point(|queued| queued.due <= due);
        self.queue.insert(at, Queued { due, pending });
    }

    fn now(&mut self, event: IcaEvent) {
        self.push(Instant::now(), Pending::Event(event));
    }

    fn record(&self, call: FakeCall) {
        lock(&self.state).calls.push(call);
    }

    /// The device's script, where it's one this system found and is still plugged in.
    fn index(&self, device: DeviceRef) -> Result<usize, ScanError> {
        let &(_, index) = self.devices.get(&device).ok_or(ScanError::Gone)?;
        if lock(&self.state).devices[index].present {
            Ok(index)
        } else {
            Err(ScanError::Gone)
        }
    }

    /// The first event due by `now`, made whole.
    fn take_due(&mut self, now: Instant) -> Option<IcaEvent> {
        if self.queue.first()?.due > now {
            return None;
        }
        Some(match self.queue.remove(0).pending {
            Pending::Event(event) => event,
            Pending::Selected {
                device,
                index,
                unit_type,
                nil,
                error,
            } => {
                let mut state = lock(&self.state);
                let scripted = &mut state.devices[index];
                if error.is_none() {
                    scripted.selected = Some(unit_type);
                }
                let unit = if nil {
                    None
                } else {
                    scripted.selected_unit().map(Box::new)
                };
                IcaEvent::UnitSelected {
                    device,
                    unit,
                    error,
                }
            }
        })
    }

    /// A select's answer for `unit_type` queued, as `select` says.
    fn answer_select(
        &mut self,
        device: DeviceRef,
        index: usize,
        unit_type: u64,
        select: FakeSelect,
    ) {
        let (nil, error) = match select {
            FakeSelect::Answers => (false, None),
            FakeSelect::Nil => (true, None),
            FakeSelect::Refused(error) => (false, Some(error)),
            FakeSelect::NeverAnswered => return,
        };
        self.push(
            Instant::now(),
            Pending::Selected {
                device,
                index,
                unit_type,
                nil,
                error,
            },
        );
    }

    /// The scan's next step queued, where one goes on: a page's bands, or its end. False where no
    /// scan goes on.
    fn feed(&mut self) -> bool {
        let Some(mut scan) = self.scan.take() else {
            return false;
        };
        let device = scan.device;
        let side = scan.next;
        let sheet = if scan.flatbed {
            if scan.pages > 0 {
                self.now(IcaEvent::ScanCompleted {
                    device,
                    error: None,
                });
                return true;
            }
            let mut state = lock(&self.state);
            let scripted = &mut state.devices[scan.index];
            let number = scripted.script.flatbed_sheet;
            scripted.script.flatbed_sheet = number.saturating_add(1);
            FakeSheet::new(number)
        } else if side == Side::Front {
            let next = lock(&self.state).devices[scan.index]
                .script
                .sheets
                .pop_front();
            match next {
                Some(sheet) => sheet,
                None => {
                    let error = lock(&self.state).devices[scan.index]
                        .script
                        .empty_answer
                        .clone();
                    self.now(IcaEvent::ScanCompleted { device, error });
                    return true;
                }
            }
        } else {
            match scan.sheet.take() {
                Some(sheet) => sheet,
                None => {
                    self.now(IcaEvent::ScanCompleted {
                        device,
                        error: None,
                    });
                    return true;
                }
            }
        };
        let fault = sheet
            .fault
            .clone()
            .filter(|(at, _)| *at == side)
            .map(|(_, fault)| fault);
        let ends = self.page(&scan, &sheet, side, fault);
        scan.pages += 1;
        if !ends {
            if scan.flatbed {
                self.now(IcaEvent::ScanCompleted {
                    device,
                    error: None,
                });
            } else if scan.duplex && side == Side::Front {
                scan.next = Side::Back;
                scan.sheet = Some(sheet);
                self.scan = Some(scan);
            } else {
                scan.next = Side::Front;
                self.scan = Some(scan);
            }
        }
        true
    }

    /// `side` of `sheet` scanned into bands and queued, with `fault`; true where the scan ends
    /// with it.
    fn page(
        &mut self,
        scan: &Scan,
        sheet: &FakeSheet,
        side: Side,
        fault: Option<FakeFault>,
    ) -> bool {
        let device = scan.device;
        let cancelled = || {
            Some(IcaError {
                description: "The scan was cancelled.".to_owned(),
                ..ica_error(RETURN_SCAN_OPERATION_CANCELED)
            })
        };
        match &fault {
            Some(FakeFault::Error(error)) => {
                // A sheet that fails at its front stays in the feeder, as a jam leaves it, its
                // fault cleared.
                if side == Side::Front && !scan.flatbed {
                    lock(&self.state).devices[scan.index]
                        .script
                        .sheets
                        .push_front(FakeSheet::new(sheet.number));
                }
                self.now(IcaEvent::ScanCompleted {
                    device,
                    error: Some(error.clone()),
                });
                return true;
            }
            Some(FakeFault::ScannerCancel) => {
                self.now(IcaEvent::ScanCompleted {
                    device,
                    error: cancelled(),
                });
                return true;
            }
            _ => {}
        }
        self.record(FakeCall::Fed {
            sheet: sheet.number,
            side,
        });
        let (unit, bands) = {
            let state = lock(&self.state);
            let scripted = &state.devices[scan.index];
            (scripted.selected_unit(), scripted.script.bands)
        };
        let Some(cut) = unit.and_then(|unit| cut(&unit, sheet.number, side, bands)) else {
            self.now(IcaEvent::ScanCompleted {
                device,
                error: Some(ica_error(RETURN_SCANNER_FAILED_TO_COMPLETE_SCAN)),
            });
            return true;
        };
        let last = cut.len().saturating_sub(1);
        for (n, (mut header, bytes)) in cut.into_iter().enumerate() {
            let n32 = u32::try_from(n).unwrap_or(u32::MAX);
            match &fault {
                Some(FakeFault::Removed { after_bands }) if n32 == *after_bands => {
                    lock(&self.state).devices[scan.index].present = false;
                    let browse = self.devices.get(&device).map(|&(browse, _)| browse);
                    self.now(IcaEvent::Removed { device });
                    if let Some(browse) = browse {
                        self.now(IcaEvent::DeviceRemoved { browse, device });
                    }
                    return true;
                }
                Some(FakeFault::Panic { after_bands }) if n32 == *after_bands => {
                    self.now(IcaEvent::Panicked {
                        origin: Origin::Device(device),
                        message: "the scripted band callback panicked".to_owned(),
                    });
                    continue;
                }
                Some(FakeFault::LyingBand { band }) if n32 == *band => header.start_row += 1,
                _ => {}
            }
            self.now(IcaEvent::Band {
                device,
                header,
                bytes,
            });
            if fault == Some(FakeFault::WarmingUp) && n == 0 {
                self.now(IcaEvent::Status {
                    device,
                    status: DeviceStatus::WarmingUp,
                });
            }
            if fault == Some(FakeFault::WarmingUp) && n == last {
                self.now(IcaEvent::Status {
                    device,
                    status: DeviceStatus::WarmedUp,
                });
            }
        }
        if fault == Some(FakeFault::EndAfter) {
            self.now(IcaEvent::ScanCompleted {
                device,
                error: None,
            });
            return true;
        }
        false
    }
}

/// The raw format of a unit's pixel type at its bit depth, as the fake draws it.
fn drawn_format(unit: &UnitProps) -> Option<Format> {
    Some(match (unit.pixel_type, unit.bit_depth) {
        (PIXEL_BW, DEPTH_1) => Format::Bw1,
        (PIXEL_GRAY, DEPTH_8) => Format::Gray8,
        (PIXEL_GRAY, DEPTH_16) => Format::Gray16,
        (PIXEL_RGB, DEPTH_8) => Format::Rgb8,
        (PIXEL_RGB, DEPTH_16) => Format::Rgb16,
        _ => return None,
    })
}

/// A length in millimetres in pixels at `dpi`, rounded, at least 1.
fn pixels(mm: f64, dpi: u64) -> Option<u32> {
    let px = (mm / 25.4 * dpi as f64).round().max(1.0);
    (px.is_finite() && px <= f64::from(u32::MAX)).then_some(px as u32)
}

/// The page a unit scans now: a flatbed its scan area, a feeder its document type's size (its
/// largest sheet where the type has none the backend states), at its resolution.
fn page_size(unit: &UnitProps) -> Option<(u32, u32)> {
    let dpi = unit.resolution;
    let (width, height) = if unit.unit_type == UNIT_DOCUMENT_FEEDER {
        match document_size(unit.document_type) {
            Some(size) => (f64::from(size.width), f64::from(size.height)),
            None => (
                length_mm(unit.physical_size.width, unit.measurement_unit, dpi)?,
                length_mm(unit.physical_size.height, unit.measurement_unit, dpi)?,
            ),
        }
    } else {
        (
            length_mm(unit.scan_area.width, unit.measurement_unit, dpi)?,
            length_mm(unit.scan_area.height, unit.measurement_unit, dpi)?,
        )
    };
    Some((pixels(width, dpi)?, pixels(height, dpi)?))
}

/// The bands, in ImageCaptureCore's form, of the page `fake::pattern` draws for `side` of sheet
/// `sheet` at what `unit` is set to: padded rows, big-endian 16-bit samples and black and white's
/// bits past the last pixel set, where `bands` says.
fn cut(
    unit: &UnitProps,
    sheet: u32,
    side: Side,
    bands: FakeBands,
) -> Option<Vec<(BandHeader, Vec<u8>)>> {
    let format = drawn_format(unit)?;
    let (width, height) = page_size(unit)?;
    let image = pattern(format, width, height, sheet, side).ok()?;
    let stride = image.stride();
    let (pixel_type, bits_per_component, components) = match format {
        Format::Bw1 => (PIXEL_BW, 1, 1),
        Format::Gray8 => (PIXEL_GRAY, 8, 1),
        Format::Gray16 => (PIXEL_GRAY, 16, 1),
        Format::Rgb8 => (PIXEL_RGB, 8, 3),
        Format::Rgb16 => (PIXEL_RGB, 16, 3),
    };
    let bytes_per_row = stride as u64 + bands.padding;
    let band_rows = bands.rows.max(1);
    let mut out = Vec::new();
    let mut start = 0;
    while start < height {
        let rows = band_rows.min(height - start);
        let mut bytes = Vec::with_capacity(rows as usize * bytes_per_row as usize);
        for row in start..start + rows {
            let at = row as usize * stride;
            let mut packed = image.data()[at..at + stride].to_vec();
            match format {
                Format::Gray16 | Format::Rgb16 if bands.big_endian => {
                    for sample in packed.as_chunks_mut::<2>().0 {
                        sample.swap(0, 1);
                    }
                }
                Format::Bw1 => {
                    if !BW_ZERO_IS_BLACK {
                        for byte in &mut packed {
                            *byte = !*byte;
                        }
                    }
                    let used = width % 8;
                    if used != 0
                        && let Some(last) = packed.last_mut()
                    {
                        *last |= 0xff_u8 >> used;
                    }
                }
                _ => {}
            }
            bytes.extend_from_slice(&packed);
            bytes.resize(bytes.len() + bands.padding as usize, PADDING_BYTE);
        }
        let header = BandHeader {
            full_width: u64::from(width),
            full_height: u64::from(height),
            bits_per_pixel: bits_per_component * components,
            bits_per_component,
            components,
            big_endian: bands.big_endian,
            pixel_type,
            bytes_per_row,
            start_row: u64::from(start),
            rows: u64::from(rows),
            data_size: bytes.len() as u64,
        };
        out.push((header, bytes));
        start += rows;
    }
    Some(out)
}

impl IcaSystem for FakeIcaSystem {
    fn bounds(&self) -> Bounds {
        self.bounds
    }

    fn browse(&mut self) -> Result<BrowseRef, ScanError> {
        let start = Instant::now();
        let (browse, found, late, enumeration) = {
            let mut state = lock(&self.state);
            let browse = BrowseRef(state.next_ref());
            state.calls.push(FakeCall::Browse(browse));
            let mut found = Vec::new();
            let mut late = Vec::new();
            for index in 0..state.devices.len() {
                let scripted = &state.devices[index];
                if !scripted.present {
                    continue;
                }
                if scripted.script.late {
                    late.push(index);
                    continue;
                }
                let after = scripted.script.found_after;
                let props = scripted.script.props.clone();
                let device = DeviceRef(state.next_ref());
                found.push((device, index, after, props));
            }
            (browse, found, late, state.enumeration)
        };
        self.browses.insert(browse);
        self.late.insert(browse, late);
        for (device, index, after, props) in found {
            self.devices.insert(device, (browse, index));
            self.push(
                start + after,
                Pending::Event(IcaEvent::DeviceAdded {
                    browse,
                    device,
                    props,
                }),
            );
        }
        if let Some(after) = enumeration {
            self.push(
                start + after,
                Pending::Event(IcaEvent::LocalEnumerated { browse }),
            );
        }
        Ok(browse)
    }

    fn stop_browse(&mut self, browse: BrowseRef) {
        self.record(FakeCall::StopBrowse(browse));
        if !self.browses.remove(&browse) {
            return;
        }
        // What wasn't due yet the stopped browser never sends; what was is in the queue still.
        let now = Instant::now();
        self.queue.retain(|queued| {
            queued.due <= now
                || !matches!(&queued.pending, Pending::Event(event)
                    if event.origin() == Origin::Browse(browse))
        });
        // Its late devices' callbacks, queued before the stop, come after it.
        for index in self.late.remove(&browse).unwrap_or_default() {
            let (device, props) = {
                let mut state = lock(&self.state);
                let props = state.devices[index].script.props.clone();
                (DeviceRef(state.next_ref()), props)
            };
            self.now(IcaEvent::DeviceAdded {
                browse,
                device,
                props,
            });
        }
        let sessions = &self.sessions;
        self.devices
            .retain(|device, (found_by, _)| *found_by != browse || sessions.contains(device));
    }

    fn next_event(&mut self, wait: Duration) -> Result<Option<IcaEvent>, ScanError> {
        let start = Instant::now();
        let end = start.checked_add(wait);
        loop {
            let now = Instant::now();
            if let Some(event) = self.take_due(now) {
                return Ok(Some(event));
            }
            if self.feed() {
                continue;
            }
            if end.is_some_and(|end| now >= end) {
                return Ok(None);
            }
            let next_due = self.queue.first().map(|queued| queued.due);
            let until = match (next_due, end) {
                (Some(due), Some(end)) => due.min(end),
                (Some(due), None) => due,
                (None, Some(end)) => end,
                (None, None) => now + Duration::from_millis(5),
            };
            thread::sleep(until.saturating_duration_since(now));
        }
    }

    fn open_session(&mut self, device: DeviceRef) -> Result<(), ScanError> {
        self.record(FakeCall::OpenSession(device));
        let index = self.index(device)?;
        let (session, unasked, unasked_nil, selected) = {
            let mut state = lock(&self.state);
            let scripted = &mut state.devices[index];
            scripted.reset();
            (
                scripted.script.session.clone(),
                scripted.script.unasked,
                scripted.script.unasked_nil,
                scripted.selected,
            )
        };
        self.sessions.insert(device);
        let unasked_select = |system: &mut FakeIcaSystem| {
            if let Some(unit_type) = selected {
                let answer = if unasked_nil {
                    FakeSelect::Nil
                } else {
                    FakeSelect::Answers
                };
                system.answer_select(device, index, unit_type, answer);
            }
        };
        match session {
            FakeSession::Opens => {
                self.now(IcaEvent::SessionOpened {
                    device,
                    error: None,
                });
                if unasked == Unasked::BeforeReady {
                    unasked_select(self);
                }
                self.now(IcaEvent::Ready { device });
                if unasked == Unasked::AfterReady {
                    unasked_select(self);
                }
            }
            FakeSession::Refused(error) => {
                self.sessions.remove(&device);
                self.now(IcaEvent::SessionOpened {
                    device,
                    error: Some(error),
                });
            }
            FakeSession::NeverReady => self.now(IcaEvent::SessionOpened {
                device,
                error: None,
            }),
            FakeSession::NeverAnswered => {}
        }
        Ok(())
    }

    fn close_session(&mut self, device: DeviceRef) -> bool {
        self.record(FakeCall::CloseSession(device));
        if !self.sessions.remove(&device) {
            return false;
        }
        self.now(IcaEvent::SessionClosed {
            device,
            error: None,
        });
        true
    }

    fn unit_types(&mut self, device: DeviceRef) -> Result<Vec<u64>, ScanError> {
        let index = self.index(device)?;
        let state = lock(&self.state);
        Ok(state.devices[index]
            .units
            .iter()
            .map(|unit| unit.unit_type)
            .take(MAX_UNITS)
            .collect())
    }

    fn selected_unit(&mut self, device: DeviceRef) -> Result<Option<UnitProps>, ScanError> {
        let index = self.index(device)?;
        Ok(lock(&self.state).devices[index].selected_unit())
    }

    fn select_unit(&mut self, device: DeviceRef, unit_type: u64) -> Result<(), ScanError> {
        self.record(FakeCall::SelectUnit(unit_type));
        let index = self.index(device)?;
        let select = {
            let state = lock(&self.state);
            let scripted = &state.devices[index];
            if scripted.unit(unit_type).is_some() {
                scripted.script.select_script(unit_type)
            } else {
                FakeSelect::Refused(ica_error(RETURN_SCANNER_FAILED_TO_SELECT_FUNCTIONAL_UNIT))
            }
        };
        self.answer_select(device, index, unit_type, select);
        Ok(())
    }

    fn write_unit(&mut self, device: DeviceRef, write: &UnitWrite) -> Result<(), ScanError> {
        self.record(FakeCall::WriteUnit(write.clone()));
        let index = self.index(device)?;
        let mut state = lock(&self.state);
        let scripted = &mut state.devices[index];
        let rule = scripted.script.write_rule.clone();
        let Some(unit) = scripted.selected_mut() else {
            return Ok(());
        };
        if let Some(duplex) = write.duplex {
            unit.duplex_enabled = duplex && unit.duplex_supported;
        }
        unit.pixel_type = write.pixel_type;
        unit.bit_depth = write.bit_depth;
        unit.resolution = write.resolution;
        if let Some(area) = write.scan_area {
            unit.scan_area = area;
        }
        if let Some(document_type) = write.document_type {
            unit.document_type = document_type;
        }
        if let Some(orientation) = write.orientation {
            unit.odd_orientation = orientation;
            unit.even_orientation = orientation;
        }
        if let Some(rule) = rule {
            rule(unit);
        }
        Ok(())
    }

    fn request_scan(&mut self, device: DeviceRef, band_bytes: u32) -> Result<(), ScanError> {
        self.record(FakeCall::RequestScan { band_bytes });
        let index = self.index(device)?;
        if self.scan.is_some() {
            return Err(ScanError::Busy);
        }
        let unit = lock(&self.state).devices[index].selected_unit();
        let Some(unit) = unit else {
            self.now(IcaEvent::ScanCompleted {
                device,
                error: Some(ica_error(RETURN_SCANNER_FAILED_TO_COMPLETE_SCAN)),
            });
            return Ok(());
        };
        let flatbed = unit.unit_type == UNIT_FLATBED;
        self.scan = Some(Scan {
            device,
            index,
            flatbed,
            duplex: !flatbed && unit.duplex_enabled,
            next: Side::Front,
            sheet: None,
            pages: 0,
        });
        Ok(())
    }

    fn cancel_scan(&mut self, device: DeviceRef) {
        self.record(FakeCall::CancelScan);
        let Some(scan) = self.scan.take() else {
            return;
        };
        if scan.device != device {
            self.scan = Some(scan);
            return;
        }
        let feeds_ahead = lock(&self.state).devices[scan.index].script.feeds_ahead;
        if feeds_ahead && !scan.flatbed {
            // The next page was feeding as the cancel came: it comes all the same.
            self.scan = Some(scan);
            self.feed();
            self.scan = None;
        }
        self.now(IcaEvent::ScanCompleted {
            device,
            error: Some(IcaError {
                description: "The scan was cancelled.".to_owned(),
                ..ica_error(RETURN_SCAN_OPERATION_CANCELED)
            }),
        });
    }
}
