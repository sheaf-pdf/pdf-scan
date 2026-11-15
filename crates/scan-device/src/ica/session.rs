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
//! have shown it and [`ScanError::TimedOut`] where its deadline comes before the snapshot would
//! be taken, then opens the session and waits for the device to be ready, all within its
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
    Caps, Device, DeviceId, DeviceInfo, Done, Event, Opened, ScanError, Settings, WindowClosed,
    WindowOwner,
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

/// What a callback that came off the main thread is said as: the glue read nothing of it.
fn off_main(whose: &str) -> ScanError {
    driver_msg(format!(
        "ImageCaptureCore called the {whose}'s delegate off the main thread, where the backend \
         reads nothing it's handed"
    ))
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
            IcaEvent::OffMainThread { .. } => Err(off_main("device browser")),
            _ => Ok(None),
        }
    }

    /// When the snapshot is taken: the settle after the local enumeration.
    fn settled(&self, bounds: &Bounds) -> Option<Instant> {
        self.enumerated.and_then(|at| end_of(at, bounds.settle))
    }

    /// The device found at `index`, where it's a scanner of `id`, with its info.
    fn matching(&self, index: usize, id: &DeviceId) -> Option<(DeviceRef, DeviceInfo)> {
        let (device, props) = self.found.get(index)?;
        id::device_info(props)
            .filter(|info| info.id == *id)
            .map(|info| (*device, info))
    }

    /// The events already queued, at most [`MAX_DRAIN`], taken as a snapshot is: so a callback
    /// the framework had queued by then counts however slow the backend was to take it, and a
    /// flood of them can't hold it. The first device of `wanted`'s id they add, if any, ends it.
    fn drain(
        &mut self,
        system: &mut dyn IcaSystem,
        wanted: Option<&DeviceId>,
    ) -> Result<Option<(DeviceRef, DeviceInfo)>, ScanError> {
        for _ in 0..MAX_DRAIN {
            let Some(event) = system.next_event(Duration::ZERO)? else {
                break;
            };
            if let (Some(index), Some(id)) = (self.take(event)?, wanted)
                && let Some(found) = self.matching(index, id)
            {
                return Ok(Some(found));
            }
        }
        Ok(None)
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
    browsing.drain(system, None)?;
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
        // An open given up on waits for its session's close no longer than its deadline.
        close_by: end,
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
    // Open: a close waits its whole bound from here.
    opened.close_by = None;
    Ok(Opened {
        device: Box::new(opened),
        info,
        renamed_from: None,
    })
}

/// The device of `stored`'s id, as a browse finds it, by the listing's snapshot rule: the events
/// already queued at the settle after the local enumeration taken too, as a snapshot takes them,
/// and [`ScanError::Gone`] once they're taken without it; [`ScanError::TimedOut`] where the
/// deadline comes before the settle, since until then a scanner on the network may yet be found
/// (and [`Backend::open`](crate::Backend::open) answers so past its deadline).
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
            return browsing
                .drain(system, Some(&stored.id))?
                .ok_or(ScanError::Gone);
        }
        if end.is_some_and(|end| now >= end) {
            return Err(ScanError::TimedOut);
        }
        let Some(event) = system.next_event(until(now, &[settled, end]))? else {
            continue;
        };
        if let Some(index) = browsing.take(event)?
            && let Some(found) = browsing.matching(index, &stored.id)
        {
            return Ok(found);
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
    /// While it's being opened, the open's deadline, past which its close isn't waited for: an
    /// open given up on answers by its deadline, not a close's bound after it.
    close_by: Option<Instant>,
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
    /// where one of its callbacks, or its browse's, panicked or came off the main thread, or where
    /// the wait can't be served (on the main thread, from inside a block of the main queue).
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
                    IcaEvent::OffMainThread { .. } => return Err(off_main("device browser")),
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
                IcaEvent::OffMainThread { .. } => Err(off_main("scanner device")),
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
    /// blocks), and no later than an open's deadline while it's being opened, and the browse
    /// stopped, which lets the device go.
    fn close(&mut self) {
        if self.session && !self.gone {
            self.session = false;
            if self.system.close_session(self.device) {
                self.closed();
            }
        }
        self.system.stop_browse(self.browse);
    }

    /// The session's close waited for, at most [`Bounds::close`], and while the device is being
    /// opened no later than the open's deadline.
    fn closed(&mut self) {
        let end = end_of(Instant::now(), self.bounds.close);
        loop {
            let left = until(Instant::now(), &[end, self.close_by]);
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
    use std::collections::VecDeque;
    use std::thread;

    use super::super::driver::{DEVICE_SCANNER, LOCATION_BONJOUR};
    use super::*;
    use crate::Driver;

    const ID: &str = "net-1";

    fn props() -> DeviceProps {
        DeviceProps {
            persistent_id: Some(ID.to_owned()),
            uuid: None,
            name: Some("Network scanner".to_owned()),
            type_bits: DEVICE_SCANNER | LOCATION_BONJOUR,
            scanner_class: true,
        }
    }

    fn stored() -> DeviceInfo {
        DeviceInfo::new(
            id::id_of(ID),
            "Network scanner".to_owned(),
            String::new(),
            String::new(),
            Driver::Ica,
        )
    }

    fn added() -> IcaEvent {
        IcaEvent::DeviceAdded {
            browse: BrowseRef(1),
            device: DeviceRef(7),
            props: props(),
        }
    }

    /// A system for the cases the scripted framework doesn't script: its browse's local
    /// enumeration ends at once; `queue` is what any wait takes first, `held` what only a wait of
    /// no time takes (a callback queued as the settle comes, which a snapshot's drain takes), and
    /// `later` what comes after the given wait (a network scanner found during the settle).
    struct Scripted {
        bounds: Bounds,
        queue: VecDeque<IcaEvent>,
        held: Option<IcaEvent>,
        later: Option<(Instant, IcaEvent)>,
        /// Whether the session opens, and whether the device is then ready.
        opens: bool,
        ready: bool,
        /// Whether a close is answered.
        answers_close: bool,
    }

    impl Scripted {
        fn new(bounds: Bounds) -> Scripted {
            Scripted {
                bounds,
                queue: VecDeque::new(),
                held: None,
                later: None,
                opens: true,
                ready: true,
                answers_close: true,
            }
        }
    }

    impl IcaSystem for Scripted {
        fn bounds(&self) -> Bounds {
            self.bounds
        }
        fn browse(&mut self) -> Result<BrowseRef, ScanError> {
            self.queue.push_back(IcaEvent::LocalEnumerated {
                browse: BrowseRef(1),
            });
            Ok(BrowseRef(1))
        }
        fn stop_browse(&mut self, _browse: BrowseRef) {}
        fn next_event(&mut self, wait: Duration) -> Result<Option<IcaEvent>, ScanError> {
            if let Some(event) = self.queue.pop_front() {
                return Ok(Some(event));
            }
            if wait.is_zero() {
                return Ok(self.held.take());
            }
            let now = Instant::now();
            if let Some((at, _)) = &self.later
                && now + wait >= *at
            {
                thread::sleep(at.saturating_duration_since(now));
                return Ok(self.later.take().map(|(_, event)| event));
            }
            thread::sleep(wait.min(Duration::from_secs(60)));
            Ok(None)
        }
        fn open_session(&mut self, device: DeviceRef) -> Result<(), ScanError> {
            if self.opens {
                self.queue.push_back(IcaEvent::SessionOpened {
                    device,
                    error: None,
                });
                if self.ready {
                    self.queue.push_back(IcaEvent::Ready { device });
                }
            }
            Ok(())
        }
        fn close_session(&mut self, device: DeviceRef) -> bool {
            if self.answers_close {
                self.queue.push_back(IcaEvent::SessionClosed {
                    device,
                    error: None,
                });
            }
            // As the real system on the main thread: the close was asked, so it's waited for.
            true
        }
        fn unit_types(&mut self, _: DeviceRef) -> Result<Vec<u64>, ScanError> {
            Ok(Vec::new())
        }
        fn selected_unit(&mut self, _: DeviceRef) -> Result<Option<UnitProps>, ScanError> {
            Ok(None)
        }
        fn select_unit(&mut self, _: DeviceRef, _: u64) -> Result<(), ScanError> {
            Ok(())
        }
        fn write_unit(&mut self, _: DeviceRef, _: &UnitWrite) -> Result<(), ScanError> {
            Ok(())
        }
        fn request_scan(&mut self, _: DeviceRef, _: u32) -> Result<(), ScanError> {
            Ok(())
        }
        fn cancel_scan(&mut self, _: DeviceRef) {}
    }

    const SHORT: Bounds = Bounds {
        settle: Duration::from_millis(50),
        answer: Duration::from_millis(200),
        close: Duration::from_millis(20),
        post: Duration::from_millis(20),
    };

    #[test]
    fn an_open_given_up_on_answers_by_its_deadline_not_a_close_bound_after() {
        // Opened, never ready, and its close never answered, with the real close bound (5 s): the
        // open answers `TimedOut` at its deadline, not 5 s later, so a worker's client hears it
        // before its own limit.
        let mut system = Scripted::new(Bounds::REAL);
        system.queue.push_back(added());
        system.ready = false;
        system.answers_close = false;
        let deadline = Duration::from_millis(300);
        let start = Instant::now();
        let opened = open(Box::new(system), &stored(), deadline);
        let took = start.elapsed();
        assert_eq!(opened.err(), Some(ScanError::TimedOut));
        assert!(took >= deadline, "{took:?}");
        assert!(took < deadline + Duration::from_secs(1), "{took:?}");
    }

    #[test]
    fn an_open_device_s_close_waits_its_whole_bound() {
        // Once open, a close the framework never answers is waited for its bound.
        let mut system = Scripted::new(Bounds {
            close: Duration::from_millis(300),
            ..SHORT
        });
        system.queue.push_back(added());
        system.answers_close = false;
        let opened = open(Box::new(system), &stored(), Duration::from_millis(100))
            .expect("the device opened");
        let start = Instant::now();
        drop(opened);
        assert!(start.elapsed() >= Duration::from_millis(300));
    }

    #[test]
    fn an_open_finds_a_device_queued_at_the_settle_as_a_listing_does() {
        // The device's callback comes as the settle does, taken by a wait of no time alone: a
        // listing has it, and so an open of its id finds it rather than saying it's gone.
        let mut listing = Scripted::new(SHORT);
        listing.held = Some(added());
        let listed = list(&mut listing, Duration::from_secs(10)).expect("a listing");
        let ids: Vec<&str> = listed.iter().map(|info| info.id.0.as_str()).collect();
        assert_eq!(ids, [format!("ica:{ID}").as_str()]);

        let mut opening = Scripted::new(SHORT);
        opening.held = Some(added());
        let opened = open(Box::new(opening), &stored(), Duration::from_secs(10))
            .expect("the device the listing has");
        assert_eq!(opened.info.id, stored().id);

        // And one that isn't queued even then is gone, at the settle.
        let start = Instant::now();
        let gone = open(
            Box::new(Scripted::new(SHORT)),
            &stored(),
            Duration::from_secs(10),
        );
        assert_eq!(gone.err(), Some(ScanError::Gone));
        assert!(start.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn an_open_whose_deadline_comes_before_the_settle_times_out() {
        // The local enumeration done, a network scanner found 400 ms into a 2 s settle: a deadline
        // before it is `TimedOut`, since the device may yet be found, not `Gone`; one after
        // finds it.
        let bounds = Bounds {
            settle: Duration::from_secs(2),
            ..SHORT
        };
        let mut early = Scripted::new(bounds);
        early.later = Some((Instant::now() + Duration::from_millis(400), added()));
        let start = Instant::now();
        let opened = open(Box::new(early), &stored(), Duration::from_millis(150));
        assert_eq!(opened.err(), Some(ScanError::TimedOut));
        assert!(start.elapsed() < Duration::from_millis(400));

        let mut later = Scripted::new(bounds);
        later.later = Some((Instant::now() + Duration::from_millis(400), added()));
        assert!(open(Box::new(later), &stored(), Duration::from_secs(5)).is_ok());
    }

    #[test]
    fn a_callback_off_the_main_thread_is_said_never_read() {
        let off = |origin| IcaEvent::OffMainThread { origin };
        // A listing's.
        let mut listing = Scripted::new(SHORT);
        listing.queue.push_back(off(Origin::Browse(BrowseRef(1))));
        let Err(ScanError::Driver { message, .. }) = list(&mut listing, Duration::from_secs(1))
        else {
            panic!("a listing that met one is refused");
        };
        assert!(message.contains("off the main thread"), "{message}");

        // The open device's, or its browse's.
        for origin in [Origin::Device(DeviceRef(7)), Origin::Browse(BrowseRef(1))] {
            let mut system = Scripted::new(SHORT);
            system.queue.push_back(off(origin));
            let mut device = IcaDevice {
                system: Box::new(system),
                browse: BrowseRef(1),
                device: DeviceRef(7),
                bounds: SHORT,
                session: false,
                gone: false,
                close_by: None,
            };
            let Err(ScanError::Driver { message, .. }) = device.next(Duration::from_secs(1)) else {
                panic!("a device's wait that met one is refused");
            };
            assert!(message.contains("off the main thread"), "{message}");
        }
    }

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
