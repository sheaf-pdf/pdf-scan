//! Process-wide lazy single-threaded apartment (STA) session backing
//! [`WiaBackend`](super::WiaBackend) and its open [`Device`].
//!
//! WIA 2.0 COM proxies (`IWiaDevMgr2`, `IWiaItem2`, `IWiaPropertyStorage`, `IWiaTransfer`) have
//! single-threaded apartment affinity, and WSD (`WSDScan.sys`) scanners frequently reject a second
//! concurrent `CreateDevice` handle in the same process (`WIA_ERROR_BUSY` /
//! `WIA_ERROR_NETWORK_RESERVATION_FAILED`). Sharing one lazy STA session across all `WiaBackend`
//! instances in a process enforces single-open-device exclusivity, serializes STA COM calls, tracks
//! live in-flight RPC activity (`StaActivity`), and ensures any `CreateDevice` call whose caller
//! timed out (`Gate::AbandonedOpen`) immediately releases its `IWiaItem2` tree when the RPC
//! returns.

#[cfg(windows)]
use std::sync::Weak;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, SyncSender};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use in_progress::{Level, NoProgress, Progress, Unit};

use super::driver::{WiaDeviceTree, WiaSystem};
use super::{error, id};
use crate::{
    Caps, Device, DeviceInfo, Done, Event, Opened, ScanError, Settings, WindowClosed, WindowOwner,
};

/// Request lifecycle state shared between the caller thread and the STA thread:
/// - `QUEUED` -> `ABANDONED` if the caller's `deadline` expires before the STA thread dequeues it;
/// - `QUEUED` -> `RUNNING` when the STA thread begins servicing it;
/// - `RUNNING` -> `ABANDONED` if `open` times out while the STA thread is inside `handle_open`
///   (telling the STA thread to immediately release any `WiaDeviceTree` it just opened);
/// - `RUNNING` -> `COMPLETED` when `handle_open` finishes before timeout and commits to sending on
///   `reply`.
const QUEUED: u8 = 0;
const RUNNING: u8 = 1;
const COMPLETED: u8 = 2;
const ABANDONED: u8 = 3;

/// Maximum time `WiaDevice::drop` waits for `Request::CloseDevice` before returning so a hung
/// out-of-process RPC stub cannot deadlock `Drop`.
const CLOSE_TIMEOUT: Duration = Duration::from_secs(5);

#[cfg(windows)]
/// Process-wide active WIA STA session, shared by all live `WiaBackend`s and any open `WiaDevice`.
static LOADED: Mutex<Weak<Session>> = Mutex::new(Weak::new());

#[cfg(windows)]
/// Holds STA threads of dropped process-wide `Session`s that were still busy (`is_busy()`) at drop
/// time. While any detached thread (or dead `LOADED` session) has not yet exited, `shared()`
/// returns `ScanError::TimedOut` rather than piling up concurrent hung RPC threads against
/// `stisvc`; once finished, `shared()` joins them and spawns fresh.
static DETACHED: Mutex<Vec<(Arc<StaActivity>, JoinHandle<()>)>> = Mutex::new(Vec::new());

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Live activity counters for a WIA STA thread.
struct StaActivity {
    /// Number of `Request`s currently executing on the STA thread (`0` while parked in `rx.recv`).
    in_flight: AtomicUsize,
    /// Number of non-`Shutdown` `Request`s queued in `tx` or currently being handled. Incremented
    /// before `tx.send(...)` so `Session::drop` never observes `is_busy() == false` while a
    /// timed-out request is still sitting in the channel about to start an RPC.
    pending_ops: AtomicUsize,
    /// `true` while `sta_loop` holds a live `Box<dyn WiaDeviceTree>` whose `Drop` must release COM
    /// interfaces on the STA thread.
    has_held_tree: AtomicBool,
    /// Set to `true` only after `sta_loop`, any held `WiaDeviceTree`, `WiaSystem`, and
    /// `CoUninitialize` have all finished on the STA thread.
    exited: AtomicBool,
    exit_lock: Mutex<()>,
    exit_cv: Condvar,
}

impl StaActivity {
    fn new() -> Self {
        StaActivity {
            in_flight: AtomicUsize::new(0),
            pending_ops: AtomicUsize::new(0),
            has_held_tree: AtomicBool::new(false),
            exited: AtomicBool::new(false),
            exit_lock: Mutex::new(()),
            exit_cv: Condvar::new(),
        }
    }

    fn enter(&self) -> InFlightGuard<'_> {
        self.in_flight.fetch_add(1, Ordering::AcqRel);
        InFlightGuard(self)
    }

    fn queue_op(self: &Arc<Self>) -> OpToken {
        self.pending_ops.fetch_add(1, Ordering::AcqRel);
        OpToken(Arc::clone(self))
    }

    fn is_busy(&self) -> bool {
        if self.exited.load(Ordering::Acquire) {
            return false;
        }
        self.in_flight.load(Ordering::Acquire) > 0
            || self.pending_ops.load(Ordering::Acquire) > 0
            || self.has_held_tree.load(Ordering::Acquire)
    }

    fn mark_exited(&self) {
        let _guard = lock(&self.exit_lock);
        self.exited.store(true, Ordering::Release);
        self.exit_cv.notify_all();
    }

    #[cfg_attr(not(windows), allow(dead_code))]
    fn wait_exited(&self, timeout: Duration) -> bool {
        if self.exited.load(Ordering::Acquire) {
            return true;
        }
        let start = Instant::now();
        let mut guard = lock(&self.exit_lock);
        while !self.exited.load(Ordering::Acquire) {
            let rem = timeout.saturating_sub(start.elapsed());
            if rem.is_zero() {
                return false;
            }
            let (next, res) = self
                .exit_cv
                .wait_timeout(guard, rem)
                .unwrap_or_else(PoisonError::into_inner);
            guard = next;
            if res.timed_out() {
                return self.exited.load(Ordering::Acquire);
            }
        }
        true
    }
}

struct InFlightGuard<'a>(&'a StaActivity);

impl Drop for InFlightGuard<'_> {
    fn drop(&mut self) {
        self.0.in_flight.fetch_sub(1, Ordering::Release);
    }
}

struct OpToken(Arc<StaActivity>);

impl Drop for OpToken {
    fn drop(&mut self) {
        self.0.pending_ops.fetch_sub(1, Ordering::AcqRel);
    }
}

struct StaExitGuard {
    gate: Arc<GateSlot>,
    activity: Arc<StaActivity>,
}

impl Drop for StaExitGuard {
    fn drop(&mut self) {
        self.gate.set(Gate::Dead);
        self.activity.mark_exited();
    }
}

/// High-level open-device gate coordinated between caller threads and the STA thread.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Gate {
    Idle,
    /// A caller is actively waiting in `Session::open`.
    Opening,
    /// A `Session::open` caller timed out while `handle_open` was `RUNNING` on the STA thread; the
    /// STA thread will drop any opened `WiaDeviceTree` and reset back to `Idle` once `handle_open`
    /// returns.
    AbandonedOpen,
    /// A `WiaDevice` is currently open.
    DeviceOpen,
    /// `WiaDevice::drop` posted `Request::CloseDevice`; the STA thread will drop the open
    /// `WiaDeviceTree` and reset back to `Idle` once finished.
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

/// STA session shared via `Arc<Session>`.
pub(crate) struct Session {
    tx: Sender<Request>,
    gate: Arc<GateSlot>,
    activity: Arc<StaActivity>,
    thread: Mutex<Option<JoinHandle<()>>>,
    is_global: bool,
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

/// Returns the process-wide Windows WIA `Session`, spawning its STA thread lazily if none is
/// running (and waiting up to `deadline` if a detached STA thread from a prior timed-out call is
/// still winding down).
#[cfg(windows)]
pub(super) fn shared(deadline: Duration) -> Result<Arc<Session>, ScanError> {
    let start = Instant::now();
    loop {
        let mut wait_for = Vec::new();
        {
            let mut slot = lock(&LOADED);
            if let Some(existing) = slot.upgrade() {
                if !existing.is_dead() {
                    return Ok(existing);
                }
                if !existing.activity.exited.load(Ordering::Acquire) {
                    wait_for.push(Arc::clone(&existing.activity));
                }
            }

            let mut detached = lock(&DETACHED);
            let mut still_running = Vec::new();
            for (act, handle) in detached.drain(..) {
                if act.exited.load(Ordering::Acquire) {
                    let _ = handle.join();
                } else {
                    wait_for.push(Arc::clone(&act));
                    still_running.push((act, handle));
                }
            }
            *detached = still_running;

            if wait_for.is_empty() {
                let session = Session::spawn()?;
                *slot = Arc::downgrade(&session);
                return Ok(session);
            }
        }

        for act in wait_for {
            let rem = deadline.saturating_sub(start.elapsed());
            if !act.wait_exited(rem) {
                return Err(ScanError::TimedOut);
            }
        }
    }
}

impl Session {
    #[cfg(windows)]
    fn spawn() -> Result<Arc<Session>, ScanError> {
        let (tx, rx) = mpsc::channel::<Request>();
        let (startup_tx, startup_rx) = mpsc::sync_channel::<Result<(), ScanError>>(1);
        let gate = Arc::new(GateSlot::new());
        let activity = Arc::new(StaActivity::new());
        let thread_gate = Arc::clone(&gate);
        let thread_activity = Arc::clone(&activity);

        let handle = thread::Builder::new()
            .name("scan-device-wia".to_owned())
            .spawn(move || {
                let exit = StaExitGuard {
                    gate: thread_gate,
                    activity: thread_activity,
                };
                super::com::run_sta(startup_tx, |sys| {
                    sta_loop(sys, &rx, &exit.activity);
                });
            })
            .map_err(|err| error::driver_msg(format!("couldn't spawn WIA STA thread: {err}")))?;

        match startup_rx.recv() {
            Ok(Ok(())) => Ok(Arc::new(Session {
                tx,
                gate,
                activity,
                thread: Mutex::new(Some(handle)),
                is_global: true,
            })),
            Ok(Err(err)) => {
                let _ = handle.join();
                Err(err)
            }
            Err(_) => {
                let _ = handle.join();
                Err(error::driver_msg(
                    "the WIA STA thread exited before starting",
                ))
            }
        }
    }

    /// Spawns an isolated STA-mode `Session` backed by `sys` (for cross-platform scripted tests).
    #[cfg(any(test, feature = "fake"))]
    #[cfg_attr(not(test), allow(dead_code))]
    pub(super) fn spawn_with(
        mut sys: Box<dyn WiaSystem + Send>,
    ) -> Result<Arc<Session>, ScanError> {
        let (tx, rx) = mpsc::channel::<Request>();
        let gate = Arc::new(GateSlot::new());
        let activity = Arc::new(StaActivity::new());
        let thread_gate = Arc::clone(&gate);
        let thread_activity = Arc::clone(&activity);

        let handle = thread::Builder::new()
            .name("scan-device-wia-fake".to_owned())
            .spawn(move || {
                let exit = StaExitGuard {
                    gate: thread_gate,
                    activity: thread_activity,
                };
                sta_loop(&mut *sys, &rx, &exit.activity);
                drop(sys);
            })
            .map_err(|err| {
                error::driver_msg(format!("couldn't spawn fake WIA STA thread: {err}"))
            })?;

        Ok(Arc::new(Session {
            tx,
            gate,
            activity,
            thread: Mutex::new(Some(handle)),
            is_global: false,
        }))
    }

    #[cfg_attr(not(windows), allow(dead_code))]
    pub(super) fn is_global(&self) -> bool {
        self.is_global
    }

    pub(super) fn is_dead(&self) -> bool {
        *lock(&self.gate.state) == Gate::Dead
    }

    fn disconnected_error(&self) -> ScanError {
        self.gate.set(Gate::Dead);
        error::driver_msg("the WIA session thread ended unexpectedly")
    }

    /// Lists WIA scanners under `deadline`. Runs normally while an open device is idle; if an STA
    /// call is already in flight and does not finish within `deadline`, returns
    /// `ScanError::TimedOut` without poisoning the session or any open device.
    pub(super) fn list(&self, deadline: Duration) -> Result<Vec<DeviceInfo>, ScanError> {
        if *lock(&self.gate.state) == Gate::Dead {
            return Err(self.disconnected_error());
        }
        let state = Arc::new(AtomicU8::new(QUEUED));
        let (reply_tx, reply_rx) = mpsc::sync_channel(1);
        let req = Request::List {
            _op: self.activity.queue_op(),
            state: Arc::clone(&state),
            reply: reply_tx,
        };
        if self.tx.send(req).is_err() {
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

    /// Opens `stored` (`native_id` already validated against `BadId`) under `deadline`.
    pub(super) fn open(
        self: &Arc<Self>,
        stored: &DeviceInfo,
        native_id: String,
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
            _op: self.activity.queue_op(),
            stored: stored.clone(),
            native_id,
            state: Arc::clone(&state),
            gate: Arc::clone(&self.gate),
            reply: reply_tx,
        };
        if self.tx.send(req).is_err() {
            return Err(self.disconnected_error());
        }

        let remaining = deadline.saturating_sub(start.elapsed());
        match reply_rx.recv_timeout(remaining) {
            Ok(res) => self.finish_open_reply(res),
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
                    let mut gate = lock(&self.gate.state);
                    if *gate == Gate::Opening {
                        *gate = Gate::AbandonedOpen;
                        self.gate.changed.notify_all();
                    }
                    return Err(ScanError::TimedOut);
                }
                match reply_rx.recv() {
                    Ok(res) => self.finish_open_reply(res),
                    Err(_) => Err(self.disconnected_error()),
                }
            }
        }
    }

    fn finish_open_reply(
        self: &Arc<Self>,
        res: Result<DeviceInfo, ScanError>,
    ) -> Result<Opened, ScanError> {
        match res {
            Ok(info) => {
                self.gate.set(Gate::DeviceOpen);
                Ok(Opened {
                    device: Box::new(WiaDevice {
                        session: Arc::clone(self),
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
        // Check `is_busy()` *before* sending `Request::Shutdown` so post-loop cleanup on the STA
        // thread is never mistaken for a hung pre-shutdown request!
        let busy = self.activity.is_busy();
        let _ = self.tx.send(Request::Shutdown);
        if let Some(handle) = lock(&self.thread).take() {
            if busy {
                let _ = self.is_global;
                #[cfg(windows)]
                if self.is_global {
                    let mut detached = lock(&DETACHED);
                    let mut still_running = Vec::with_capacity(detached.len() + 1);
                    for (act, old_handle) in detached.drain(..) {
                        if act.exited.load(Ordering::Acquire) {
                            let _ = old_handle.join();
                        } else {
                            still_running.push((act, old_handle));
                        }
                    }
                    still_running.push((Arc::clone(&self.activity), handle));
                    *detached = still_running;
                }
            } else {
                let _ = handle.join();
            }
        }
    }
}

/// An open WIA 2.0 device driven over the shared STA [`Session`].
struct WiaDevice {
    session: Arc<Session>,
}

impl Device for WiaDevice {
    fn caps(&mut self) -> Result<Caps, ScanError> {
        self.session.ensure_healthy()?;
        let (reply_tx, reply_rx) = mpsc::sync_channel(1);
        if self
            .session
            .tx
            .send(Request::Caps {
                _op: self.session.activity.queue_op(),
                reply: reply_tx,
            })
            .is_err()
        {
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
            _op: self.session.activity.queue_op(),
            settings: settings.clone(),
            first_sheet,
            events: events.clone(),
            steps: step_tx,
        };
        if self.session.tx.send(req).is_err() {
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

    fn settings_window(&mut self, _owner: Option<WindowOwner>) -> Result<WindowClosed, ScanError> {
        Err(ScanError::Unsupported)
    }
}

impl Drop for WiaDevice {
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
            .tx
            .send(Request::CloseDevice {
                _op: self.session.activity.queue_op(),
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

/// Messages sent from the STA thread back to `WiaDevice::scan`'s caller thread so all
/// [`Progress`] updates and cancellation checks execute directly against `&dyn Progress`.
#[cfg_attr(not(windows), allow(dead_code))]
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

/// Requests processed by the WIA STA thread.
enum Request {
    List {
        _op: OpToken,
        state: Arc<AtomicU8>,
        reply: SyncSender<Result<Vec<DeviceInfo>, ScanError>>,
    },
    Open {
        _op: OpToken,
        stored: DeviceInfo,
        native_id: String,
        state: Arc<AtomicU8>,
        gate: Arc<GateSlot>,
        reply: SyncSender<Result<DeviceInfo, ScanError>>,
    },
    Caps {
        _op: OpToken,
        reply: SyncSender<Result<Caps, ScanError>>,
    },
    Scan {
        _op: OpToken,
        settings: Settings,
        first_sheet: u32,
        events: SyncSender<Event>,
        steps: Sender<ScanStep>,
    },
    CloseDevice {
        _op: OpToken,
        gate: Arc<GateSlot>,
        done: SyncSender<()>,
    },
    Shutdown,
}

struct HeldTreeSlot<'a> {
    tree: Option<Box<dyn WiaDeviceTree>>,
    activity: &'a StaActivity,
}

impl<'a> HeldTreeSlot<'a> {
    fn new(activity: &'a StaActivity) -> Self {
        HeldTreeSlot {
            tree: None,
            activity,
        }
    }

    fn set(&mut self, tree: Box<dyn WiaDeviceTree>) {
        self.tree = Some(tree);
        self.activity.has_held_tree.store(true, Ordering::Release);
    }

    fn clear(&mut self) {
        let prev = self.tree.take();
        drop(prev);
        self.activity.has_held_tree.store(false, Ordering::Release);
    }

    fn as_deref_mut(&mut self) -> Option<&mut (dyn WiaDeviceTree + 'static)> {
        self.tree.as_deref_mut()
    }
}

impl Drop for HeldTreeSlot<'_> {
    fn drop(&mut self) {
        let prev = self.tree.take();
        drop(prev);
        self.activity.has_held_tree.store(false, Ordering::Release);
    }
}

fn sta_loop(sys: &mut dyn WiaSystem, rx: &Receiver<Request>, activity: &StaActivity) {
    let mut held = HeldTreeSlot::new(activity);
    while let Ok(req) = rx.recv() {
        if handle_request(sys, &mut held, req, activity) {
            break;
        }
    }
    let _in_flight = activity.enter();
    held.clear();
}

/// Processes one `Request` on the STA thread; returns `true` on `Request::Shutdown`.
///
/// Both `_op` (`pending_ops`) and `_in_flight` (`in_flight`) are dropped strictly *before* sending
/// each request's completion reply (`reply`, `steps`, or `done`) so a caller that immediately
/// drops `Session` upon receiving its reply never observes a stale `is_busy() == true` from the
/// request that just finished.
fn handle_request(
    sys: &mut dyn WiaSystem,
    held: &mut HeldTreeSlot<'_>,
    req: Request,
    activity: &StaActivity,
) -> bool {
    match req {
        Request::List { _op, state, reply } => {
            let to_send = {
                let _op = _op;
                let _in_flight = activity.enter();
                state
                    .compare_exchange(QUEUED, RUNNING, Ordering::AcqRel, Ordering::Acquire)
                    .ok()
                    .map(|_| handle_list(sys))
            };
            if let Some(res) = to_send {
                let _ = reply.send(res);
            }
            false
        }
        Request::Open {
            _op,
            stored,
            native_id,
            state,
            gate,
            reply,
        } => {
            let to_send = {
                let _op = _op;
                let _in_flight = activity.enter();
                if state
                    .compare_exchange(QUEUED, RUNNING, Ordering::AcqRel, Ordering::Acquire)
                    .is_ok()
                {
                    let res = handle_open(sys, held, &stored, &native_id);
                    commit_sta_open(held, res, &state, &gate)
                } else {
                    None
                }
            };
            if let Some(res) = to_send {
                let _ = reply.send(res);
            }
            false
        }
        Request::Caps { _op, reply } => {
            let res = {
                let _op = _op;
                let _in_flight = activity.enter();
                match held.as_deref_mut() {
                    Some(tree) => handle_caps(tree),
                    None => Err(error::driver_msg("WIA device is not open")),
                }
            };
            let _ = reply.send(res);
            false
        }
        Request::Scan {
            _op,
            settings,
            first_sheet,
            events,
            steps,
        } => {
            let res = {
                let _op = _op;
                let _in_flight = activity.enter();
                let relay = RelayProgress {
                    steps: steps.clone(),
                };
                match held.as_deref_mut() {
                    Some(tree) => handle_scan(tree, &settings, first_sheet, &events, &relay),
                    None => Err(error::driver_msg("WIA device is not open")),
                }
            };
            let _ = steps.send(ScanStep::Finished(res));
            false
        }
        Request::CloseDevice { _op, gate, done } => {
            {
                let _op = _op;
                let _in_flight = activity.enter();
                held.clear();
                gate.finish_cleanup(Gate::Closing);
            }
            let _ = done.send(());
            false
        }
        Request::Shutdown => true,
    }
}

fn handle_list(sys: &mut dyn WiaSystem) -> Result<Vec<DeviceInfo>, ScanError> {
    let props = sys.list_devices()?;
    Ok(props
        .iter()
        .filter_map(id::device_info_from_props)
        .collect())
}

fn handle_open(
    sys: &mut dyn WiaSystem,
    held: &mut HeldTreeSlot<'_>,
    stored: &DeviceInfo,
    native_id: &str,
) -> Result<DeviceInfo, ScanError> {
    let (mut root_props, tree) = sys.open_device(native_id)?;
    if root_props.dev_id.is_empty() {
        root_props.dev_id = native_id.to_owned();
    }
    let Some(opened_info) = id::device_info_from_props(&root_props) else {
        drop(tree);
        return Err(ScanError::Gone);
    };
    if !id::matches_opened(stored, &opened_info) {
        drop(tree);
        return Err(ScanError::Gone);
    }
    held.set(tree);
    Ok(opened_info)
}

fn commit_sta_open(
    held: &mut HeldTreeSlot<'_>,
    res: Result<DeviceInfo, ScanError>,
    state: &AtomicU8,
    gate: &GateSlot,
) -> Option<Result<DeviceInfo, ScanError>> {
    if state
        .compare_exchange(RUNNING, COMPLETED, Ordering::AcqRel, Ordering::Acquire)
        .is_ok()
    {
        return Some(res);
    }
    if res.is_ok() {
        held.clear();
    }
    let mut guard = lock(&gate.state);
    if matches!(*guard, Gate::Opening | Gate::AbandonedOpen) {
        *guard = Gate::Idle;
        gate.changed.notify_all();
    }
    None
}

fn handle_caps(tree: &mut dyn WiaDeviceTree) -> Result<Caps, ScanError> {
    super::caps::read(tree)
}

fn handle_scan(
    tree: &mut dyn WiaDeviceTree,
    settings: &Settings,
    first_sheet: u32,
    events: &SyncSender<Event>,
    progress: &dyn Progress,
) -> Result<Done, ScanError> {
    super::batch::run(tree, settings, first_sheet, events, progress)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Driver;
    use crate::wia::driver::fake_wia::{FakeDeviceSpec, FakeWiaHandle};
    use crate::wia::id::RawDeviceProps;

    fn scanner_props(dev_id: &str, maker: &str, model: &str, name: &str) -> RawDeviceProps {
        RawDeviceProps {
            dev_id: dev_id.to_owned(),
            dev_type: Some(0x0001_0001),
            dev_name: Some(name.to_owned()),
            vend_desc: Some(maker.to_owned()),
            dev_desc: Some(model.to_owned()),
            ..RawDeviceProps::default()
        }
    }

    #[test]
    fn single_open_device_guard_returns_busy_and_allows_idle_list() {
        let fake = FakeWiaHandle::new();
        let props0 = scanner_props(r"{6BDD1FC6}\0000", "Acme", "Scan 100", "Scanner Zero");
        let props1 = scanner_props(r"{6BDD1FC6}\0001", "Acme", "Scan 200", "Scanner One");
        let spec0 = FakeDeviceSpec::new(props0);
        let closed0 = Arc::clone(&spec0.closed_count);
        fake.add_device(spec0);
        fake.add_device(FakeDeviceSpec::new(props1));

        let session = Session::spawn_with(Box::new(fake.make_system())).expect("spawn session");
        let listed = session.list(Duration::from_secs(2)).expect("list");
        assert_eq!(listed.len(), 2);

        let opened0 = session
            .open(
                &listed[0],
                r"{6BDD1FC6}\0000".to_owned(),
                Duration::from_secs(2),
            )
            .expect("open dev0");
        assert_eq!(lock(&fake.state).active_opens.load(Ordering::Acquire), 1);

        // Opening a second device (or the same device again) while one is open returns Busy.
        assert_eq!(
            session
                .open(
                    &listed[1],
                    r"{6BDD1FC6}\0001".to_owned(),
                    Duration::from_secs(2),
                )
                .err(),
            Some(ScanError::Busy)
        );

        // Listing while the open device is idle still works.
        let while_open = session
            .list(Duration::from_secs(2))
            .expect("list while open");
        assert_eq!(while_open.len(), 2);

        // Dropping the open device releases the tree and unblocks opening the next scanner.
        drop(opened0);
        assert_eq!(closed0.load(Ordering::Acquire), 1);
        assert_eq!(lock(&fake.state).active_opens.load(Ordering::Acquire), 0);

        let opened1 = session
            .open(
                &listed[1],
                r"{6BDD1FC6}\0001".to_owned(),
                Duration::from_secs(2),
            )
            .expect("open dev1 after closing dev0");
        assert_eq!(opened1.info.driver, Driver::Wia);
    }

    #[test]
    fn timed_out_open_closes_late_tree_and_next_open_recovers() {
        let fake = FakeWiaHandle::new();
        let props = scanner_props(r"{6BDD1FC6}\0000", "Acme", "NetScan", "Network Scanner");
        let mut spec = FakeDeviceSpec::new(props);
        spec.open_delay = Duration::from_millis(80);
        let closed = Arc::clone(&spec.closed_count);
        fake.add_device(spec);

        let session = Session::spawn_with(Box::new(fake.make_system())).expect("spawn session");
        let stored = session
            .list(Duration::from_secs(2))
            .expect("list")
            .remove(0);

        assert_eq!(
            session
                .open(
                    &stored,
                    r"{6BDD1FC6}\0000".to_owned(),
                    Duration::from_millis(10),
                )
                .err(),
            Some(ScanError::TimedOut)
        );

        // Speed up subsequent opens once the in-flight one finishes; the next `open` waits for
        // `Gate::AbandonedOpen` cleanup and then opens cleanly without leaking the first tree.
        lock(&fake.state).devices[0].open_delay = Duration::ZERO;
        let recovered = session
            .open(
                &stored,
                r"{6BDD1FC6}\0000".to_owned(),
                Duration::from_secs(2),
            )
            .expect("next open recovers after abandoned open closes");
        assert_eq!(closed.load(Ordering::Acquire), 1);
        assert_eq!(lock(&fake.state).active_opens.load(Ordering::Acquire), 1);
        drop(recovered);
        assert_eq!(closed.load(Ordering::Acquire), 2);
        assert_eq!(lock(&fake.state).active_opens.load(Ordering::Acquire), 0);
    }

    #[test]
    fn reassigned_instance_slot_returns_gone_and_drops_tree() {
        let fake = FakeWiaHandle::new();
        let enum_props = scanner_props(r"{6BDD1FC6}\0000", "Acme", "OldModel", "Old");
        let root_props = scanner_props(r"{6BDD1FC6}\0000", "OtherVendor", "NewModel", "New");
        let mut spec = FakeDeviceSpec::new(enum_props.clone());
        spec.root_props = root_props;
        let closed = Arc::clone(&spec.closed_count);
        fake.add_device(spec);

        let session = Session::spawn_with(Box::new(fake.make_system())).expect("spawn session");
        let stored = id::device_info_from_props(&enum_props).expect("stored");
        assert_eq!(
            session
                .open(
                    &stored,
                    r"{6BDD1FC6}\0000".to_owned(),
                    Duration::from_secs(2),
                )
                .err(),
            Some(ScanError::Gone)
        );
        assert_eq!(closed.load(Ordering::Acquire), 1);
        assert_eq!(lock(&fake.state).active_opens.load(Ordering::Acquire), 0);
    }

    #[test]
    fn list_timeout_does_not_poison_session() {
        let fake = FakeWiaHandle::new();
        fake.add_device(FakeDeviceSpec::new(scanner_props(
            r"{6BDD1FC6}\0000",
            "Acme",
            "Model",
            "Scanner",
        )));
        lock(&fake.state).list_delay = Duration::from_millis(80);

        let session = Session::spawn_with(Box::new(fake.make_system())).expect("spawn session");
        assert_eq!(
            session.list(Duration::from_millis(10)),
            Err(ScanError::TimedOut)
        );

        // Wait for the slow list to complete, clear delay, and verify the session still works.
        thread::sleep(Duration::from_millis(100));
        lock(&fake.state).list_delay = Duration::ZERO;
        let listed = session
            .list(Duration::from_secs(2))
            .expect("list after timeout");
        assert_eq!(listed.len(), 1);
    }

    #[test]
    fn rapid_session_drop_and_reopen_cycles_never_leave_busy_or_unexited_sta() {
        let fake = FakeWiaHandle::new();
        fake.add_device(FakeDeviceSpec::new(scanner_props(
            r"{6BDD1FC6}\0000",
            "Acme",
            "Model",
            "Scanner",
        )));

        for _ in 0..32 {
            let session = Session::spawn_with(Box::new(fake.make_system())).expect("spawn");
            let act = Arc::clone(&session.activity);
            let listed = session.list(Duration::from_secs(2)).expect("list");
            let opened = session
                .open(
                    &listed[0],
                    r"{6BDD1FC6}\0000".to_owned(),
                    Duration::from_secs(2),
                )
                .expect("open");
            drop(opened);
            // Immediately after `WiaDevice` drops, `activity.is_busy()` must already be `false` so
            // `Session::drop` joins synchronously rather than detaching.
            assert!(!act.is_busy());
            drop(session);
            assert!(act.exited.load(Ordering::Acquire));
        }
    }
}
