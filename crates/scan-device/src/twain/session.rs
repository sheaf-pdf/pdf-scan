//! Process-wide lazy single-threaded apartment (STA) session over `twain_dsm::run`, backing
//! [`TwainBackend`](super::TwainBackend) and its open [`Device`] on Windows.
//!
//! Because `twain-dsm`'s `RunLock` and `TWAINDSM.dll` handle are process-wide singletons, every
//! `TwainBackend` and open device in a process shares one lazy STA session, torn down and joined
//! when the last `TwainBackend` and open device drop. Live in-flight tracking (`StaActivity`)
//! distinguishes a briefly slow call that timed out on the caller side from a genuinely hung STA
//! thread, so a tight `deadline` on `list` or `open` never permanently bricks an active device or
//! leaks an open TWAIN source.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicUsize, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError, Sender, SyncSender};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError, Weak};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use in_progress::{Level, NoProgress, Progress, Unit};
use twain_dsm::ffi;

use super::caps::{self, LiveSource};
use super::{Delivery, batch, error, id, settings};
use crate::{
    Caps, Device, DeviceInfo, Done, Event, Opened, ScanError, Settings, WindowClosed, WindowOwner,
};

/// Request lifecycle state shared between the caller thread and the STA thread:
/// - `QUEUED` -> `ABANDONED` if the caller's `deadline` expires before the STA thread dequeues it;
/// - `QUEUED` -> `RUNNING` when the STA thread begins servicing it;
/// - `RUNNING` -> `ABANDONED` if `open` times out while the STA thread is inside `handle_open`
///   (telling the STA thread to immediately close any `Held::Source` it just opened);
/// - `RUNNING` -> `COMPLETED` when `handle_open` finishes before timeout and commits to sending on
///   `reply`.
const QUEUED: u8 = 0;
const RUNNING: u8 = 1;
const COMPLETED: u8 = 2;
const ABANDONED: u8 = 3;

/// Polling interval while pumping a standalone driver settings window (`MSG_ENABLEDSUIONLY`).
const UI_POLL: Duration = Duration::from_millis(200);

/// Maximum time `TwainDevice::drop` waits for `Request::CloseDevice` before returning so a driver
/// hung inside a DSM call cannot deadlock `Drop`.
const CLOSE_TIMEOUT: Duration = Duration::from_secs(5);

/// Process-wide active session, shared by all live `TwainBackend`s and any open `TwainDevice`.
static LOADED: Mutex<Weak<Session>> = Mutex::new(Weak::new());

/// Holds the STA thread of a dropped `Session` that was still mid-DSM-call (`in_flight > 0`) at
/// drop time. While `exited` is `false`, `twain_dsm`'s singleton `RunLock` is still held by that
/// stuck call, so `shared()` returns `ScanError::TimedOut`; if the slow driver call eventually
/// returns and exits `twain_dsm::run`, `shared()` joins the finished handle and spawns fresh.
static DETACHED: Mutex<Option<(Arc<StaActivity>, JoinHandle<()>)>> = Mutex::new(None);

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Live activity counters for an STA thread running `twain_dsm::run`.
struct StaActivity {
    /// Number of `Request`s currently executing inside a DSM/driver call on the STA thread (`0`
    /// while parked in `twain.wait`).
    in_flight: AtomicUsize,
    /// Set to `true` once `twain_dsm::run` returns on the STA thread (releasing `RunLock`).
    exited: AtomicBool,
}

impl StaActivity {
    fn new() -> Self {
        StaActivity {
            in_flight: AtomicUsize::new(0),
            exited: AtomicBool::new(false),
        }
    }

    fn enter(&self) -> InFlightGuard<'_> {
        self.in_flight.fetch_add(1, Ordering::AcqRel);
        InFlightGuard(self)
    }

    fn is_busy(&self) -> bool {
        self.in_flight.load(Ordering::Acquire) > 0 && !self.exited.load(Ordering::Acquire)
    }
}

struct InFlightGuard<'a>(&'a StaActivity);

impl Drop for InFlightGuard<'_> {
    fn drop(&mut self) {
        self.0.in_flight.fetch_sub(1, Ordering::Release);
    }
}

/// High-level state of the process-wide TWAIN session, coordinated between caller threads and the
/// STA thread.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Gate {
    Idle,
    /// A caller is actively waiting in `Session::open`.
    Opening,
    /// A `Session::open` caller timed out while `handle_open` was `RUNNING` on the STA thread; the
    /// STA thread will close any opened source and reset back to `Idle` once `handle_open` returns.
    AbandonedOpen,
    /// A `TwainDevice` is currently open.
    DeviceOpen,
    /// `TwainDevice::drop` posted `Request::CloseDevice`; the STA thread will reset back to `Idle`
    /// once `handle_close_device` finishes.
    Closing,
    /// The STA thread exited unexpectedly.
    Dead,
}

struct GateSlot {
    state: Mutex<Gate>,
    changed: Condvar,
}

impl GateSlot {
    fn new() -> Self {
        GateSlot {
            state: Mutex::new(Gate::Idle),
            changed: Condvar::new(),
        }
    }

    fn set(&self, next: Gate) {
        *lock(&self.state) = next;
        self.changed.notify_all();
    }

    fn finish_cleanup(&self, expected: Gate) {
        let mut guard = lock(&self.state);
        if *guard == expected {
            *guard = Gate::Idle;
            self.changed.notify_all();
        }
    }
}

/// Process-wide STA session shared via `Arc<Session>`.
pub(super) struct Session {
    remote: twain_dsm::Remote<Request>,
    gate: Arc<GateSlot>,
    activity: Arc<StaActivity>,
    thread: Mutex<Option<JoinHandle<()>>>,
}

impl core::fmt::Debug for Session {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let gate = *lock(&self.gate.state);
        f.debug_struct("Session")
            .field("gate", &gate)
            .field(
                "in_flight",
                &self.activity.in_flight.load(Ordering::Acquire),
            )
            .finish()
    }
}

/// Returns the process-wide `Session`, spawning its STA thread lazily if none is running.
pub(super) fn shared() -> Result<Arc<Session>, ScanError> {
    let mut slot = lock(&LOADED);
    if let Some(existing) = slot.upgrade()
        && *lock(&existing.gate.state) != Gate::Dead
    {
        return Ok(existing);
    }

    {
        let mut detached = lock(&DETACHED);
        if let Some((act, handle)) = detached.take() {
            if act.exited.load(Ordering::Acquire) {
                let _ = handle.join();
            } else {
                *detached = Some((act, handle));
                return Err(ScanError::TimedOut);
            }
        }
    }

    let session = Session::spawn()?;
    *slot = Arc::downgrade(&session);
    Ok(session)
}

impl Session {
    fn spawn() -> Result<Arc<Session>, ScanError> {
        let (startup_tx, startup_rx) =
            mpsc::sync_channel::<Result<twain_dsm::Remote<Request>, ScanError>>(1);
        let activity = Arc::new(StaActivity::new());
        let thread_activity = Arc::clone(&activity);

        let handle = thread::Builder::new()
            .name("scan-device-twain".to_owned())
            .spawn(move || {
                let run_res = twain_dsm::run::<Request, _>(|twain| {
                    if startup_tx.send(Ok(twain.remote())).is_err() {
                        return;
                    }
                    sta_loop(twain, &thread_activity);
                });
                thread_activity.exited.store(true, Ordering::Release);
                if let Err(run_err) = run_res {
                    let _ = startup_tx.send(Err(error::run(&run_err)));
                }
            })
            .map_err(|err| error::driver_msg(format!("couldn't spawn TWAIN STA thread: {err}")))?;

        match startup_rx.recv() {
            Ok(Ok(remote)) => Ok(Arc::new(Session {
                remote,
                gate: Arc::new(GateSlot::new()),
                activity,
                thread: Mutex::new(Some(handle)),
            })),
            Ok(Err(err)) => {
                let _ = handle.join();
                Err(err)
            }
            Err(_) => {
                let _ = handle.join();
                Err(error::driver_msg(
                    "the TWAIN STA thread exited before starting",
                ))
            }
        }
    }

    fn disconnected_error(&self) -> ScanError {
        self.gate.set(Gate::Dead);
        error::driver_msg("the TWAIN session thread ended unexpectedly")
    }

    /// Lists sources under `deadline`. Safe to call concurrently with an open device (including
    /// during `scan` or `settings_window`): if `deadline` elapses before or during enumeration,
    /// `list` returns `ScanError::TimedOut` without poisoning the session or any open device.
    pub(super) fn list(
        &self,
        dsm_path: Option<PathBuf>,
        deadline: Duration,
    ) -> Result<Vec<DeviceInfo>, ScanError> {
        if *lock(&self.gate.state) == Gate::Dead {
            return Err(self.disconnected_error());
        }
        let state = Arc::new(AtomicU8::new(QUEUED));
        let (reply_tx, reply_rx) = mpsc::sync_channel(1);
        let req = Request::List {
            load: make_load(dsm_path),
            state: Arc::clone(&state),
            reply: reply_tx,
        };
        if self.remote.post(req).is_err() {
            return Err(self.disconnected_error());
        }

        match reply_rx.recv_timeout(deadline) {
            Ok(res) => res,
            Err(RecvTimeoutError::Disconnected) => Err(self.disconnected_error()),
            Err(RecvTimeoutError::Timeout) => {
                let _ =
                    state.compare_exchange(QUEUED, ABANDONED, Ordering::AcqRel, Ordering::Acquire);
                Err(ScanError::TimedOut)
            }
        }
    }

    /// Opens `stored` (`expected_name` already validated against `BadId`) under `deadline`.
    pub(super) fn open(
        self: &Arc<Self>,
        dsm_path: Option<PathBuf>,
        delivery: Option<Delivery>,
        native_transfer: bool,
        stored: &DeviceInfo,
        expected_name: String,
        deadline: Duration,
    ) -> Result<Opened, ScanError> {
        let start = Instant::now();
        {
            let mut gate = lock(&self.gate.state);
            loop {
                match *gate {
                    Gate::Dead => return Err(self.disconnected_error()),
                    Gate::Opening | Gate::DeviceOpen => return Err(ScanError::Busy),
                    Gate::AbandonedOpen | Gate::Closing => {
                        let remaining = deadline.saturating_sub(start.elapsed());
                        if remaining.is_zero() {
                            return Err(ScanError::TimedOut);
                        }
                        let (next_guard, wait_res) = self
                            .gate
                            .changed
                            .wait_timeout(gate, remaining)
                            .unwrap_or_else(PoisonError::into_inner);
                        gate = next_guard;
                        if wait_res.timed_out()
                            && matches!(*gate, Gate::AbandonedOpen | Gate::Closing)
                        {
                            return Err(ScanError::TimedOut);
                        }
                    }
                    Gate::Idle => {
                        *gate = Gate::Opening;
                        break;
                    }
                }
            }
        }

        let state = Arc::new(AtomicU8::new(QUEUED));
        let (reply_tx, reply_rx) = mpsc::sync_channel(1);
        let req = Request::Open {
            load: make_load(dsm_path),
            stored: stored.clone(),
            expected_name,
            delivery,
            state: Arc::clone(&state),
            gate: Arc::clone(&self.gate),
            reply: reply_tx,
        };
        if self.remote.post(req).is_err() {
            return Err(self.disconnected_error());
        }

        let remaining = deadline.saturating_sub(start.elapsed());
        match reply_rx.recv_timeout(remaining) {
            Ok(res) => self.finish_open_reply(res, native_transfer),
            Err(RecvTimeoutError::Disconnected) => Err(self.disconnected_error()),
            Err(RecvTimeoutError::Timeout) => {
                if state
                    .compare_exchange(QUEUED, ABANDONED, Ordering::AcqRel, Ordering::Acquire)
                    .is_ok()
                {
                    self.gate.finish_cleanup(Gate::Opening);
                    return Err(ScanError::TimedOut);
                }
                if state
                    .compare_exchange(RUNNING, ABANDONED, Ordering::AcqRel, Ordering::Acquire)
                    .is_ok()
                {
                    // Tell subsequent callers that the previous `open` timed out in flight; when
                    // `handle_open` returns on the STA thread, it will close any `Held::Source`
                    // and reset `Gate::AbandonedOpen -> Gate::Idle`.
                    let mut gate = lock(&self.gate.state);
                    if *gate == Gate::Opening {
                        *gate = Gate::AbandonedOpen;
                        self.gate.changed.notify_all();
                    }
                    return Err(ScanError::TimedOut);
                }
                // `state` is `COMPLETED`: the STA thread already finished `handle_open` and is
                // sending on the buffered `sync_channel(1)` right now.
                match reply_rx.recv() {
                    Ok(res) => self.finish_open_reply(res, native_transfer),
                    Err(_) => Err(self.disconnected_error()),
                }
            }
        }
    }

    fn finish_open_reply(
        self: &Arc<Self>,
        res: Result<DeviceInfo, ScanError>,
        native_transfer: bool,
    ) -> Result<Opened, ScanError> {
        match res {
            Ok(info) => {
                self.gate.set(Gate::DeviceOpen);
                Ok(Opened {
                    device: Box::new(TwainDevice {
                        session: Arc::clone(self),
                        native_transfer,
                    }),
                    info,
                    renamed_from: None,
                })
            }
            Err(err) => {
                self.gate.finish_cleanup(Gate::Opening);
                Err(err)
            }
        }
    }

    fn ensure_healthy(&self) -> Result<(), ScanError> {
        if *lock(&self.gate.state) == Gate::Dead {
            Err(self.disconnected_error())
        } else {
            Ok(())
        }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.remote.post(Request::Shutdown);
        if let Some(handle) = lock(&self.thread).take() {
            if self.activity.is_busy() {
                *lock(&DETACHED) = Some((Arc::clone(&self.activity), handle));
            } else {
                let _ = handle.join();
            }
        }
    }
}

/// An open TWAIN source driven over the shared STA [`Session`].
struct TwainDevice {
    session: Arc<Session>,
    native_transfer: bool,
}

impl Device for TwainDevice {
    fn caps(&mut self) -> Result<Caps, ScanError> {
        self.session.ensure_healthy()?;
        let (reply_tx, reply_rx) = mpsc::sync_channel(1);
        let req = Request::Caps {
            native_transfer: self.native_transfer,
            reply: reply_tx,
        };
        if self.session.remote.post(req).is_err() {
            return Err(self.session.disconnected_error());
        }
        reply_rx
            .recv()
            .unwrap_or_else(|_| Err(self.session.disconnected_error()))
    }

    fn scan(
        &mut self,
        settings: &Settings,
        first_sheet: u32,
        events: &SyncSender<Event>,
        progress: &dyn Progress,
    ) -> Result<Done, ScanError> {
        self.session.ensure_healthy()?;
        let (step_tx, step_rx) = mpsc::channel();
        let req = Request::Scan {
            native_transfer: self.native_transfer,
            settings: settings.clone(),
            first_sheet,
            events: events.clone(),
            steps: step_tx,
        };
        if self.session.remote.post(req).is_err() {
            return Err(self.session.disconnected_error());
        }

        loop {
            match step_rx.recv() {
                Ok(ScanStep::SetTotal(total, unit)) => progress.set_total(total, unit),
                Ok(ScanStep::Set(done)) => progress.set(done),
                Ok(ScanStep::Inc(delta)) => progress.inc(delta),
                Ok(ScanStep::Message(level, text)) => progress.message(level, &text),
                Ok(ScanStep::CheckCancel(reply)) => {
                    let _ = reply.send(progress.is_cancelled());
                }
                Ok(ScanStep::Finished(result)) => return result,
                Err(_) => return Err(self.session.disconnected_error()),
            }
        }
    }

    fn settings_window(&mut self, owner: Option<WindowOwner>) -> Result<WindowClosed, ScanError> {
        // Reject a window owned by the calling thread *before* blocking on the STA channel:
        // synchronous cross-thread Win32 parenting/modal messages (`WM_ENABLE`, `SetParent`) would
        // deadlock if the owning UI thread stopped pumping its message queue here.
        if let Some(WindowOwner(hwnd)) = owner
            && twain_dsm::window_belongs_to_current_thread(hwnd)
        {
            return Err(error::driver_msg(format!(
                "window handle {hwnd:#x} belongs to the calling thread, which would deadlock while waiting for the scanner's settings window"
            )));
        }

        self.session.ensure_healthy()?;
        let (reply_tx, reply_rx) = mpsc::sync_channel(1);
        let req = Request::SettingsWindow {
            owner,
            reply: reply_tx,
        };
        if self.session.remote.post(req).is_err() {
            return Err(self.session.disconnected_error());
        }
        reply_rx
            .recv()
            .unwrap_or_else(|_| Err(self.session.disconnected_error()))
    }
}

impl Drop for TwainDevice {
    fn drop(&mut self) {
        {
            let mut gate = lock(&self.session.gate.state);
            if *gate == Gate::Dead {
                return;
            }
            *gate = Gate::Closing;
            self.session.gate.changed.notify_all();
        }
        let (done_tx, done_rx) = mpsc::sync_channel(1);
        if self
            .session
            .remote
            .post(Request::CloseDevice {
                gate: Arc::clone(&self.session.gate),
                done: done_tx,
            })
            .is_err()
        {
            self.session.gate.set(Gate::Dead);
            return;
        }
        if let Err(RecvTimeoutError::Disconnected) = done_rx.recv_timeout(CLOSE_TIMEOUT) {
            self.session.gate.set(Gate::Dead);
        }
    }
}

/// Messages sent from the STA thread back to `TwainDevice::scan`'s caller thread so all
/// [`Progress`] updates and cancellation checks execute directly against `&dyn Progress` without
/// lifetime erasure or child-weight side effects.
enum ScanStep {
    SetTotal(Option<u64>, Unit),
    Set(u64),
    Inc(u64),
    Message(Level, String),
    CheckCancel(SyncSender<bool>),
    Finished(Result<Done, ScanError>),
}

/// [`Progress`] forwarder used on the STA thread during `batch::scan`.
struct RelayProgress {
    steps: Sender<ScanStep>,
}

impl Progress for RelayProgress {
    fn set_total(&self, total: Option<u64>, unit: Unit) {
        let _ = self.steps.send(ScanStep::SetTotal(total, unit));
    }

    fn set(&self, current: u64) {
        let _ = self.steps.send(ScanStep::Set(current));
    }

    fn inc(&self, delta: u64) {
        let _ = self.steps.send(ScanStep::Inc(delta));
    }

    fn message(&self, level: Level, msg: &str) {
        let _ = self.steps.send(ScanStep::Message(level, msg.to_owned()));
    }

    fn is_cancelled(&self) -> bool {
        let (tx, rx) = mpsc::sync_channel(1);
        if self.steps.send(ScanStep::CheckCancel(tx)).is_err() {
            return true;
        }
        rx.recv().unwrap_or(true)
    }

    fn child(&self, _name: &str, _weight: f64) -> Box<dyn Progress> {
        Box::new(NoProgress)
    }
}

/// Requests processed by the STA thread inside `twain_dsm::run`.
enum Request {
    List {
        load: twain_dsm::Load,
        state: Arc<AtomicU8>,
        reply: SyncSender<Result<Vec<DeviceInfo>, ScanError>>,
    },
    Open {
        load: twain_dsm::Load,
        stored: DeviceInfo,
        expected_name: String,
        delivery: Option<Delivery>,
        state: Arc<AtomicU8>,
        gate: Arc<GateSlot>,
        reply: SyncSender<Result<DeviceInfo, ScanError>>,
    },
    Caps {
        native_transfer: bool,
        reply: SyncSender<Result<Caps, ScanError>>,
    },
    Scan {
        native_transfer: bool,
        settings: Settings,
        first_sheet: u32,
        events: SyncSender<Event>,
        steps: Sender<ScanStep>,
    },
    SettingsWindow {
        owner: Option<WindowOwner>,
        reply: SyncSender<Result<WindowClosed, ScanError>>,
    },
    CloseDevice {
        gate: Arc<GateSlot>,
        done: SyncSender<()>,
    },
    Shutdown,
}

/// State owned on the STA thread across requests.
enum Held<'t> {
    Disconnected,
    Connected(twain_dsm::Opened<'t, Request>),
    Source {
        source: twain_dsm::Source<'t, Request>,
        load: twain_dsm::Load,
        source_id: twain_dsm::SourceId,
        delivery: twain_dsm::Delivery,
    },
    BrokenDevice {
        opened: Option<twain_dsm::Opened<'t, Request>>,
        load: twain_dsm::Load,
        source_id: twain_dsm::SourceId,
        delivery: twain_dsm::Delivery,
    },
}

fn make_load(dsm_path: Option<PathBuf>) -> twain_dsm::Load {
    match dsm_path {
        Some(path) => twain_dsm::Load::Path(path),
        None => twain_dsm::Load::System,
    }
}

fn app_identity() -> Result<twain_dsm::App, ScanError> {
    twain_dsm::App::new("pdf-scan", "scan-device", "scan-device").map_err(|e| error::map(&e))
}

fn connect_dsm<'t>(
    twain: &'t twain_dsm::Twain<Request>,
    load: twain_dsm::Load,
) -> Result<twain_dsm::Opened<'t, Request>, ScanError> {
    let loaded = twain.load(load).map_err(|e| error::load(&e))?;
    let app = app_identity()?;
    loaded.open(app).map_err(|r| error::map(&r.into_error()))
}

fn list_from_opened(opened: &twain_dsm::Opened<'_, Request>) -> Result<Vec<DeviceInfo>, ScanError> {
    let sources = opened.sources().map_err(|e| error::map(&e))?;
    Ok(sources.iter().filter_map(id::device_info_of).collect())
}

fn sta_loop(twain: &twain_dsm::Twain<Request>, activity: &StaActivity) {
    let mut held = Held::Disconnected;
    loop {
        match twain.wait(Duration::MAX) {
            twain_dsm::Wake::Request(req) => {
                if handle_request(twain, &mut held, req, activity) {
                    break;
                }
            }
            twain_dsm::Wake::Quit(_) => break,
            twain_dsm::Wake::Timeout | _ => {}
        }
    }
    let _in_flight = activity.enter();
    drop(held);
}

/// Processes one `Request` on the STA thread; returns `true` on `Request::Shutdown`.
fn handle_request<'t>(
    twain: &'t twain_dsm::Twain<Request>,
    held: &mut Held<'t>,
    req: Request,
    activity: &StaActivity,
) -> bool {
    if matches!(req, Request::Shutdown) {
        return true;
    }
    let _in_flight = activity.enter();
    match req {
        Request::List { load, state, reply } => {
            if state
                .compare_exchange(QUEUED, RUNNING, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
            {
                let res = handle_list(twain, held, load);
                let _ = reply.send(res);
            }
            false
        }
        Request::Open {
            load,
            stored,
            expected_name,
            delivery,
            state,
            gate,
            reply,
        } => {
            if state
                .compare_exchange(QUEUED, RUNNING, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
            {
                let res = handle_open(twain, held, load, &stored, &expected_name, delivery);
                finish_sta_open(held, res, &state, &gate, &reply);
            }
            false
        }
        Request::Caps {
            native_transfer,
            reply,
        } => {
            let res = with_open_source(twain, held, |source| {
                let res = caps::read(&mut LiveSource(&source), native_transfer);
                (Some(source), res)
            });
            let _ = reply.send(res);
            false
        }
        Request::Scan {
            native_transfer,
            settings,
            first_sheet,
            events,
            steps,
        } => {
            let relay = RelayProgress {
                steps: steps.clone(),
            };
            let res = with_open_source(twain, held, |source| {
                batch::scan(
                    source,
                    native_transfer,
                    &settings,
                    first_sheet,
                    &events,
                    &relay,
                    handle_nested_request,
                )
            });
            let _ = steps.send(ScanStep::Finished(res));
            false
        }
        Request::SettingsWindow { owner, reply } => {
            let res = with_open_source(twain, held, |source| {
                run_settings_window(source, owner, handle_nested_request)
            });
            let _ = reply.send(res);
            false
        }
        Request::CloseDevice { gate, done } => {
            handle_close_device(held);
            gate.finish_cleanup(Gate::Closing);
            let _ = done.send(());
            false
        }
        Request::Shutdown => true,
    }
}

/// Commits `handle_open` on the STA thread: if the caller is still waiting (`RUNNING ->
/// COMPLETED`), delivers `res`; if the caller already timed out (`ABANDONED`), immediately closes
/// any opened source and resets `Gate::Opening | Gate::AbandonedOpen -> Gate::Idle`.
fn finish_sta_open(
    held: &mut Held<'_>,
    res: Result<DeviceInfo, ScanError>,
    state: &AtomicU8,
    gate: &GateSlot,
    reply: &SyncSender<Result<DeviceInfo, ScanError>>,
) {
    if state
        .compare_exchange(RUNNING, COMPLETED, Ordering::AcqRel, Ordering::Acquire)
        .is_ok()
    {
        let _ = reply.send(res);
        return;
    }
    if res.is_ok() {
        handle_close_device(held);
    }
    let mut guard = lock(&gate.state);
    if matches!(*guard, Gate::Opening | Gate::AbandonedOpen) {
        *guard = Gate::Idle;
        gate.changed.notify_all();
    }
}

/// Handles a request posted while a source is enabled in state 5/6 (`scan` or `settings_window`).
/// Only `Request::List` is valid concurrently with an open device.
fn handle_nested_request(opened: &twain_dsm::Opened<'_, Request>, req: Request) {
    match req {
        Request::List { load, state, reply } => {
            if state
                .compare_exchange(QUEUED, RUNNING, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
            {
                let res = opened
                    .check_load(load)
                    .map_err(|e| error::load(&e))
                    .and_then(|()| list_from_opened(opened));
                let _ = reply.send(res);
            }
        }
        Request::Open {
            state, gate, reply, ..
        } => {
            if state
                .compare_exchange(QUEUED, RUNNING, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
            {
                if state
                    .compare_exchange(RUNNING, COMPLETED, Ordering::AcqRel, Ordering::Acquire)
                    .is_ok()
                {
                    let _ = reply.send(Err(ScanError::Busy));
                } else {
                    let mut guard = lock(&gate.state);
                    if matches!(*guard, Gate::Opening | Gate::AbandonedOpen) {
                        *guard = Gate::Idle;
                        gate.changed.notify_all();
                    }
                }
            }
        }
        Request::Caps { reply, .. } => {
            let _ = reply.send(Err(ScanError::Busy));
        }
        Request::Scan { steps, .. } => {
            let _ = steps.send(ScanStep::Finished(Err(ScanError::Busy)));
        }
        Request::SettingsWindow { reply, .. } => {
            let _ = reply.send(Err(ScanError::Busy));
        }
        Request::CloseDevice { gate, done } => {
            gate.finish_cleanup(Gate::Closing);
            let _ = done.send(());
        }
        Request::Shutdown => {}
    }
}

fn handle_list<'t>(
    twain: &'t twain_dsm::Twain<Request>,
    held: &mut Held<'t>,
    load: twain_dsm::Load,
) -> Result<Vec<DeviceInfo>, ScanError> {
    match held {
        Held::Disconnected => {
            let opened = connect_dsm(twain, load)?;
            let res = list_from_opened(&opened);
            *held = Held::Connected(opened);
            res
        }
        Held::Connected(opened) => {
            opened.check_load(load).map_err(|e| error::load(&e))?;
            list_from_opened(opened)
        }
        Held::Source { source, .. } => {
            source
                .opened()
                .check_load(load)
                .map_err(|e| error::load(&e))?;
            list_from_opened(source.opened())
        }
        Held::BrokenDevice { opened, .. } => {
            if let Some(conn) = opened {
                conn.check_load(load).map_err(|e| error::load(&e))?;
                list_from_opened(conn)
            } else {
                let conn = connect_dsm(twain, load)?;
                let res = list_from_opened(&conn);
                *opened = Some(conn);
                res
            }
        }
    }
}

fn handle_open<'t>(
    twain: &'t twain_dsm::Twain<Request>,
    held: &mut Held<'t>,
    load: twain_dsm::Load,
    stored: &DeviceInfo,
    expected_name: &str,
    delivery_override: Option<Delivery>,
) -> Result<DeviceInfo, ScanError> {
    let opened = match core::mem::replace(held, Held::Disconnected) {
        Held::Disconnected => connect_dsm(twain, load.clone())?,
        Held::Connected(opened) => {
            if let Err(err) = opened.check_load(load.clone()) {
                *held = Held::Connected(opened);
                return Err(error::load(&err));
            }
            opened
        }
        other @ (Held::Source { .. } | Held::BrokenDevice { .. }) => {
            *held = other;
            return Err(ScanError::Busy);
        }
    };

    let sources = match opened.sources() {
        Ok(sources) => sources,
        Err(err) => {
            *held = Held::Connected(opened);
            return Err(error::map(&err));
        }
    };

    let Some((matched, info)) = id::find_matching(expected_name, stored, &sources) else {
        *held = Held::Connected(opened);
        return Err(ScanError::Gone);
    };
    let source_id = *matched;

    let delivery = match delivery_override {
        Some(Delivery::Events) => twain_dsm::Delivery::Events,
        Some(Delivery::Callback) => twain_dsm::Delivery::Callback,
        None if source_id.is_twain2() => twain_dsm::Delivery::Callback,
        None => twain_dsm::Delivery::Events,
    };

    match opened.open_source(&source_id, delivery) {
        Ok(source) => {
            *held = Held::Source {
                source,
                load,
                source_id,
                delivery,
            };
            Ok(info)
        }
        Err(refused) => {
            let (opened, err) = refused.into_parts();
            *held = Held::Connected(opened);
            Err(error::map(&err))
        }
    }
}

fn with_open_source<'t, R>(
    twain: &'t twain_dsm::Twain<Request>,
    held: &mut Held<'t>,
    op: impl FnOnce(
        twain_dsm::Source<'t, Request>,
    ) -> (Option<twain_dsm::Source<'t, Request>>, Result<R, ScanError>),
) -> Result<R, ScanError> {
    let (source, load, source_id, delivery) = match core::mem::replace(held, Held::Disconnected) {
        Held::Source {
            source,
            load,
            source_id,
            delivery,
        } => (source, load, source_id, delivery),
        Held::BrokenDevice {
            opened,
            load,
            source_id,
            delivery,
        } => {
            let conn = match opened {
                Some(conn) => conn,
                None => match connect_dsm(twain, load.clone()) {
                    Ok(conn) => conn,
                    Err(err) => {
                        *held = Held::BrokenDevice {
                            opened: None,
                            load,
                            source_id,
                            delivery,
                        };
                        return Err(err);
                    }
                },
            };
            match conn.open_source(&source_id, delivery) {
                Ok(source) => (source, load, source_id, delivery),
                Err(refused) => {
                    let (conn, err) = refused.into_parts();
                    *held = Held::BrokenDevice {
                        opened: Some(conn),
                        load,
                        source_id,
                        delivery,
                    };
                    return Err(error::map(&err));
                }
            }
        }
        other => {
            *held = other;
            return Err(error::driver_msg("no TWAIN source is open"));
        }
    };

    let (returned, res) = op(source);
    *held = match returned {
        Some(source) => Held::Source {
            source,
            load,
            source_id,
            delivery,
        },
        None => Held::BrokenDevice {
            opened: None,
            load,
            source_id,
            delivery,
        },
    };
    res
}

fn handle_close_device(held: &mut Held<'_>) {
    match core::mem::replace(held, Held::Disconnected) {
        Held::Source { source, .. } => {
            if let Ok(opened) = source.close() {
                *held = Held::Connected(opened);
            }
        }
        Held::BrokenDevice {
            opened: Some(opened),
            ..
        } => {
            *held = Held::Connected(opened);
        }
        Held::Connected(opened) => {
            *held = Held::Connected(opened);
        }
        Held::Disconnected | Held::BrokenDevice { opened: None, .. } => {}
    }
}

/// Opens `MSG_ENABLEDSUIONLY` on `source` and pumps events until the user saves (`CloseOk`) or
/// cancels (`CloseRequest`).
fn run_settings_window<'t>(
    source: twain_dsm::Source<'t, Request>,
    owner: Option<WindowOwner>,
    mut on_request: impl FnMut(&twain_dsm::Opened<'t, Request>, Request),
) -> (
    Option<twain_dsm::Source<'t, Request>>,
    Result<WindowClosed, ScanError>,
) {
    match settings::read_bool_cap(&mut LiveSource(&source), ffi::CAP_ENABLEDSUIONLY) {
        Ok(Some(false)) => return (Some(source), Err(ScanError::Unsupported)),
        Ok(_) => {}
        Err(err) => return (Some(source), Err(err)),
    }

    let mut enabled = match source.settings_ui(owner.map(|o| o.0)) {
        Ok(enabled) => enabled,
        Err(refused) => {
            let (source, err) = refused.into_parts();
            return (Some(source), Err(error::map_settings_ui(&err)));
        }
    };

    loop {
        match enabled.next_event(UI_POLL) {
            Ok(twain_dsm::Next::Event(back, twain_dsm::Event::CloseOk)) => {
                return disable_after_ui(back, Ok(WindowClosed::Accepted));
            }
            Ok(twain_dsm::Next::Event(back, twain_dsm::Event::CloseRequest))
            | Ok(twain_dsm::Next::Quit(back, _)) => {
                return disable_after_ui(back, Ok(WindowClosed::Dismissed));
            }
            Ok(twain_dsm::Next::Request(back, req)) => {
                on_request(back.opened(), req);
                enabled = back;
            }
            Ok(twain_dsm::Next::Timeout(back)) | Ok(twain_dsm::Next::Event(back, _)) => {
                enabled = back;
            }
            Ok(twain_dsm::Next::Ready(ready)) => {
                // Defensive: `MSG_ENABLEDSUIONLY` shouldn't transfer, so reset and close cleanly.
                return match ready.reset() {
                    Ok(back) => disable_after_ui(back, Ok(WindowClosed::Accepted)),
                    Err(refused) => (None, Err(error::map(&refused.into_error()))),
                };
            }
            Err(refused) => {
                let (back, err) = refused.into_parts();
                return disable_after_ui(back, Err(error::map(&err)));
            }
        }
    }
}

fn disable_after_ui<'t>(
    enabled: twain_dsm::Enabled<'t, Request>,
    outcome: Result<WindowClosed, ScanError>,
) -> (
    Option<twain_dsm::Source<'t, Request>>,
    Result<WindowClosed, ScanError>,
) {
    match enabled.disable() {
        Ok(src) => (Some(src), outcome),
        Err(refused) => {
            let err = refused.into_error();
            match outcome {
                Ok(_) => (None, Err(error::map(&err))),
                Err(primary) => (None, Err(primary)),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DeviceId, Driver};

    #[test]
    fn sta_activity_tracks_in_flight_and_clears_once_exited() {
        let act = StaActivity::new();
        assert!(!act.is_busy());
        {
            let _g = act.enter();
            assert!(act.is_busy());
        }
        assert!(!act.is_busy());

        let _g = act.enter();
        assert!(act.is_busy());
        act.exited.store(true, Ordering::Release);
        assert!(!act.is_busy());
    }

    #[test]
    fn finish_sta_open_resets_abandoned_gate_and_delivers_when_not_abandoned() {
        let dummy = DeviceInfo::new(
            DeviceId("twain64:Scanner".to_owned()),
            "Scanner".to_owned(),
            "Vendor".to_owned(),
            "Model".to_owned(),
            Driver::Twain { bits: 64 },
        );

        // 1) Normal `RUNNING -> COMPLETED`: sends `Ok(info)` on `reply`.
        let mut held = Held::Disconnected;
        let state = AtomicU8::new(RUNNING);
        let gate = GateSlot::new();
        gate.set(Gate::Opening);
        let (tx, rx) = mpsc::sync_channel(1);
        finish_sta_open(&mut held, Ok(dummy.clone()), &state, &gate, &tx);
        assert_eq!(state.load(Ordering::Acquire), COMPLETED);
        assert_eq!(rx.try_recv().unwrap(), Ok(dummy.clone()));

        // 2) Timed-out `ABANDONED`: resets `Gate::AbandonedOpen` -> `Gate::Idle` and drops reply.
        let mut held2 = Held::Disconnected;
        let state2 = AtomicU8::new(ABANDONED);
        let gate2 = GateSlot::new();
        gate2.set(Gate::AbandonedOpen);
        let (tx2, rx2) = mpsc::sync_channel(1);
        finish_sta_open(&mut held2, Ok(dummy), &state2, &gate2, &tx2);
        assert_eq!(*lock(&gate2.state), Gate::Idle);
        assert!(rx2.try_recv().is_err());
    }
}
