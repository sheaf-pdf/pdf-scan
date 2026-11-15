//! The session over [`IcaSystem`]: a listing, an open, the open device's events, and its close.
//!
//! A listing is a browse of scanners attached, found by Bonjour or shared, as scanline's is
//! (MIT, Copyright (c) 2025 by Scott J. Kleper, <https://github.com/klep/scanline>), whose
//! order a session follows: the device's delegate set, its session asked for, the device ready,
//! then its functional unit selected and set, and the scan asked for. In our own words, and
//! nothing of its Swift.
//!
//! ImageCaptureCore's listing is a stream with no end, so a listing takes a snapshot: the
//! [`Bounds::settle`] after the browse's local enumeration, so network scanners found by then are
//! in it, or at the deadline where that comes first. A deadline before the local enumeration ends
//! is [`ScanError::TimedOut`]: the listing isn't done by the model's own measure. An open browses
//! by the same rule until it finds the stored id, [`ScanError::Gone`] where a listing wouldn't
//! have shown it, then opens the session and waits for the device to be ready, all within its
//! deadline.

use std::sync::mpsc::SyncSender;
use std::time::{Duration, Instant};

use in_progress::Progress;

use super::driver::{
    BAND_BYTES, Bounds, BrowseRef, DeviceProps, DeviceRef, IcaEvent, IcaSystem, Origin, UnitProps,
    UnitWrite,
};
use super::error::{driver_msg, scan_error};
use super::{batch, caps, id};
use crate::{
    Caps, Device, DeviceInfo, Done, Event, Opened, ScanError, Settings, WindowClosed, WindowOwner,
};

/// The most events a snapshot takes at its end of those already queued, so a flood of them can't
/// hold a listing past its time.
const MAX_DRAIN: usize = 256;

/// `start` plus `wait`, or `None` where that's past what an `Instant` holds: never.
fn end_of(start: Instant, wait: Duration) -> Option<Instant> {
    start.checked_add(wait)
}

/// The time from `now` to the earlier of `ends`, those there.
fn until(now: Instant, ends: &[Option<Instant>]) -> Duration {
    ends.iter()
        .flatten()
        .min()
        .map_or(Duration::MAX, |end| end.saturating_duration_since(now))
}

/// A browse's devices so far, and when its local enumeration ended.
struct Browsing {
    browse: BrowseRef,
    enumerated: Option<Instant>,
    found: Vec<(DeviceRef, DeviceProps)>,
}

impl Browsing {
    fn new(browse: BrowseRef) -> Browsing {
        Browsing {
            browse,
            enumerated: None,
            found: Vec::new(),
        }
    }

    /// `event` taken where it's this browse's, and dropped where it isn't (another browse's, a
    /// late one of an earlier request, or a device's); the device it added, if it added one.
    fn take(&mut self, event: IcaEvent) -> Result<Option<usize>, ScanError> {
        if event.origin() != Origin::Browse(self.browse) {
            return Ok(None);
        }
        match event {
            IcaEvent::DeviceAdded { device, props, .. } => {
                self.found.push((device, props));
                Ok(Some(self.found.len() - 1))
            }
            IcaEvent::DeviceRemoved { device, .. } => {
                self.found.retain(|(found, _)| *found != device);
                Ok(None)
            }
            IcaEvent::LocalEnumerated { .. } => {
                self.enumerated.get_or_insert_with(Instant::now);
                Ok(None)
            }
            IcaEvent::Panicked { message, .. } => Err(driver_msg(format!(
                "a callback of ImageCaptureCore's device browser panicked: {message}"
            ))),
            _ => Ok(None),
        }
    }

    /// When the snapshot is taken: the settle after the local enumeration.
    fn settled(&self, bounds: &Bounds) -> Option<Instant> {
        self.enumerated.and_then(|at| end_of(at, bounds.settle))
    }
}

/// The scanners found within `deadline`, as the module's docs say: the snapshot at the settle
/// after the local enumeration, or at the deadline where that comes first; [`ScanError::TimedOut`]
/// where the local enumeration hadn't ended by the deadline. Two devices of one id: the first
/// kept, but a local one before a network one.
pub(super) fn list(
    system: &mut dyn IcaSystem,
    deadline: Duration,
) -> Result<Vec<DeviceInfo>, ScanError> {
    let bounds = system.bounds();
    let end = end_of(Instant::now(), deadline);
    let browse = system.browse()?;
    let mut browsing = Browsing::new(browse);
    let result = snapshot(system, &mut browsing, &bounds, end);
    system.stop_browse(browse);
    result?;
    let mut listed: Vec<(DeviceInfo, bool)> = Vec::new();
    for (_, props) in &browsing.found {
        let Some(info) = id::device_info(props) else {
            continue;
        };
        let local = id::is_local(props);
        match listed.iter_mut().find(|(known, _)| known.id == info.id) {
            Some(slot) => {
                if local && !slot.1 {
                    *slot = (info, local);
                }
            }
            None => listed.push((info, local)),
        }
    }
    Ok(listed.into_iter().map(|(info, _)| info).collect())
}

/// The browse's events taken until its snapshot is due, then those already queued.
fn snapshot(
    system: &mut dyn IcaSystem,
    browsing: &mut Browsing,
    bounds: &Bounds,
    end: Option<Instant>,
) -> Result<(), ScanError> {
    loop {
        let now = Instant::now();
        let settled = browsing.settled(bounds);
        if settled.is_some_and(|settled| now >= settled) {
            break;
        }
        if end.is_some_and(|end| now >= end) {
            if browsing.enumerated.is_none() {
                return Err(ScanError::TimedOut);
            }
            break;
        }
        if let Some(event) = system.next_event(until(now, &[settled, end]))? {
            browsing.take(event)?;
        }
    }
    for _ in 0..MAX_DRAIN {
        let Some(event) = system.next_event(Duration::ZERO)? else {
            break;
        };
        browsing.take(event)?;
    }
    Ok(())
}

/// The device of `stored`'s id opened, as the module's docs say: found by a browse, its session
/// opened and the device ready, all within `deadline`. The browse is kept while the device is
/// open, since the browser keeps its devices alive.
pub(super) fn open(
    mut system: Box<dyn IcaSystem>,
    stored: &DeviceInfo,
    deadline: Duration,
) -> Result<Opened, ScanError> {
    let bounds = system.bounds();
    let end = end_of(Instant::now(), deadline);
    let browse = system.browse()?;
    let found = find(system.as_mut(), browse, stored, &bounds, end);
    let (device, info) = match found {
        Ok(found) => found,
        Err(err) => {
            system.stop_browse(browse);
            return Err(err);
        }
    };
    let mut opened = IcaDevice {
        system,
        browse,
        device,
        bounds,
        session: false,
        gone: false,
    };
    // From here the device's drop closes what was opened, and stops the browse.
    opened.system.open_session(device)?;
    opened.session = true;
    loop {
        let now = Instant::now();
        if end.is_some_and(|end| now >= end) {
            return Err(ScanError::TimedOut);
        }
        match opened.next(until(now, &[end]))? {
            Some(IcaEvent::SessionOpened {
                error: Some(error), ..
            }) => {
                opened.session = false;
                return Err(scan_error(&error));
            }
            Some(IcaEvent::Ready { .. }) => break,
            Some(IcaEvent::DeviceError { error, .. }) => return Err(scan_error(&error)),
            _ => {}
        }
    }
    Ok(Opened {
        device: Box::new(opened),
        info,
        renamed_from: None,
    })
}

/// The device of `stored`'s id, as a browse finds it: [`ScanError::Gone`] once the local
/// enumeration has ended and the settle passed without it, or the deadline passed after the
/// enumeration; [`ScanError::TimedOut`] where the deadline came first.
fn find(
    system: &mut dyn IcaSystem,
    browse: BrowseRef,
    stored: &DeviceInfo,
    bounds: &Bounds,
    end: Option<Instant>,
) -> Result<(DeviceRef, DeviceInfo), ScanError> {
    let mut browsing = Browsing::new(browse);
    loop {
        let now = Instant::now();
        let settled = browsing.settled(bounds);
        if settled.is_some_and(|settled| now >= settled) {
            return Err(ScanError::Gone);
        }
        if end.is_some_and(|end| now >= end) {
            return Err(if browsing.enumerated.is_some() {
                ScanError::Gone
            } else {
                ScanError::TimedOut
            });
        }
        let Some(event) = system.next_event(until(now, &[settled, end]))? else {
            continue;
        };
        if let Some(index) = browsing.take(event)? {
            let (device, props) = &browsing.found[index];
            if let Some(info) = id::device_info(props).filter(|info| info.id == stored.id) {
                return Ok((*device, info));
            }
        }
    }
}

/// An open device: its system, the browse that found it, and the device as the system holds it.
pub(super) struct IcaDevice {
    pub(super) system: Box<dyn IcaSystem>,
    pub(super) browse: BrowseRef,
    pub(super) device: DeviceRef,
    pub(super) bounds: Bounds,
    /// Whether its session was asked for and not refused, so its close closes it.
    session: bool,
    /// Whether it's been removed: every call after is [`ScanError::Gone`].
    gone: bool,
}

impl IcaDevice {
    /// [`ScanError::Gone`] where the device has been removed.
    pub(super) fn live(&self) -> Result<(), ScanError> {
        if self.gone {
            Err(ScanError::Gone)
        } else {
            Ok(())
        }
    }

    /// The device's next event, waiting at most `wait`, the others' dropped: `None` once that
    /// has passed with none. [`ScanError::Gone`] where the device was removed (by its own
    /// delegate's word or its browse's) or its session closed unasked; [`ScanError::Driver`]
    /// where one of its callbacks, or its browse's, panicked, or where the wait can't be served
    /// (on the main thread, from inside a block of the main queue).
    pub(super) fn next(&mut self, wait: Duration) -> Result<Option<IcaEvent>, ScanError> {
        self.live()?;
        let end = end_of(Instant::now(), wait);
        loop {
            let left = until(Instant::now(), &[end]);
            let Some(event) = self.system.next_event(left)? else {
                return Ok(None);
            };
            let origin = event.origin();
            if origin == Origin::Browse(self.browse) {
                match event {
                    IcaEvent::DeviceRemoved { device, .. } if device == self.device => {
                        self.gone = true;
                        return Err(ScanError::Gone);
                    }
                    IcaEvent::Panicked { message, .. } => {
                        return Err(driver_msg(format!(
                            "a callback of ImageCaptureCore's device browser panicked: {message}"
                        )));
                    }
                    _ => continue,
                }
            }
            if origin != Origin::Device(self.device) {
                continue;
            }
            return match event {
                IcaEvent::Removed { .. } => {
                    self.gone = true;
                    Err(ScanError::Gone)
                }
                IcaEvent::SessionClosed { .. } => {
                    self.session = false;
                    self.gone = true;
                    Err(ScanError::Gone)
                }
                IcaEvent::Panicked { message, .. } => Err(driver_msg(format!(
                    "a callback of ImageCaptureCore's scanner device panicked: {message}"
                ))),
                event => Ok(Some(event)),
            };
        }
    }

    /// The device's unit types, as the system gives them.
    pub(super) fn unit_types(&mut self) -> Result<Vec<u64>, ScanError> {
        self.live()?;
        self.system.unit_types(self.device)
    }

    /// The selected unit, read whole.
    pub(super) fn selected_unit(&mut self) -> Result<Option<UnitProps>, ScanError> {
        self.live()?;
        self.system.selected_unit(self.device)
    }

    /// The unit of `unit_type` asked for; its answer is the caller's to wait for.
    pub(super) fn request_select(&mut self, unit_type: u64) -> Result<(), ScanError> {
        self.live()?;
        self.system.select_unit(self.device, unit_type)
    }

    /// `write` set on the selected unit.
    pub(super) fn write_unit(&mut self, write: &UnitWrite) -> Result<(), ScanError> {
        self.live()?;
        self.system.write_unit(self.device, write)
    }

    /// A memory-based scan asked for, its bands at most [`BAND_BYTES`].
    pub(super) fn request_scan(&mut self) -> Result<(), ScanError> {
        self.live()?;
        self.system.request_scan(self.device, BAND_BYTES)
    }

    /// The scan cancelled, where the device is still there.
    pub(super) fn cancel_scan(&mut self) {
        if !self.gone {
            self.system.cancel_scan(self.device);
        }
    }

    /// The session closed, waiting at most [`Bounds::close`] for the framework's word where the
    /// system says it comes (not where the close was posted off the main thread, so a drop never
    /// blocks), and the browse stopped, which lets the device go.
    fn close(&mut self) {
        if self.session && !self.gone {
            self.session = false;
            if self.system.close_session(self.device) {
                self.closed();
            }
        }
        self.system.stop_browse(self.browse);
    }

    /// The session's close waited for, at most [`Bounds::close`].
    fn closed(&mut self) {
        let end = end_of(Instant::now(), self.bounds.close);
        loop {
            let left = until(Instant::now(), &[end]);
            if left.is_zero() {
                return;
            }
            match self.system.next_event(left) {
                Ok(Some(IcaEvent::SessionClosed { device, .. } | IcaEvent::Removed { device }))
                    if device == self.device =>
                {
                    return;
                }
                Ok(Some(_)) => {}
                Ok(None) | Err(_) => return,
            }
        }
    }
}

impl Drop for IcaDevice {
    fn drop(&mut self) {
        self.close();
    }
}

impl Device for IcaDevice {
    fn caps(&mut self) -> Result<Caps, ScanError> {
        caps::read(self)
    }

    fn scan(
        &mut self,
        settings: &Settings,
        first_sheet: u32,
        events: &SyncSender<Event>,
        progress: &dyn Progress,
    ) -> Result<Done, ScanError> {
        batch::scan(self, settings, first_sheet, events, progress)
    }

    fn settings_window(&mut self, _owner: Option<WindowOwner>) -> Result<WindowClosed, ScanError> {
        Err(ScanError::Unsupported)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_real_bounds_are_the_backends_own() {
        assert_eq!(Bounds::REAL.settle, Duration::from_secs(2));
        assert_eq!(Bounds::REAL.answer, Duration::from_secs(30));
        assert_eq!(Bounds::REAL.close, Duration::from_secs(5));
        assert_eq!(Bounds::REAL.post, Duration::from_secs(5));
    }

    #[test]
    fn a_wait_ends_at_the_earliest_end_given() {
        let now = Instant::now();
        let soon = now + Duration::from_millis(10);
        let later = now + Duration::from_secs(10);
        assert_eq!(
            until(now, &[Some(later), Some(soon)]),
            Duration::from_millis(10)
        );
        assert_eq!(until(now, &[None, Some(later)]), Duration::from_secs(10));
        assert_eq!(until(now, &[None]), Duration::MAX);
        // Past: no time left.
        assert_eq!(until(later, &[Some(soon)]), Duration::ZERO);
        // A deadline past what an `Instant` holds never ends.
        assert_eq!(end_of(now, Duration::MAX), None);
    }
}
