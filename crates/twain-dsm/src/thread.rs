//! The TWAIN thread: the caller's, for the length of [`run`].

use core::cell::{Cell, RefCell};
use core::fmt;
use std::collections::VecDeque;
use std::panic::{self, AssertUnwindSafe};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

use crate::codes::ReturnCode;
use crate::dsm::Dsm;
use crate::error::{Error, Gone, RunError};
use crate::ffi::{self, TW_IDENTITY, TW_USERINTERFACE};
use crate::states::Delivery;
use crate::window::{self, Apartment, Message, RunLock, Window};

/// Makes the calling thread the TWAIN thread while `f` runs, and hands `f`
/// the [`Twain`] that every TWAIN state is made from.
///
/// The thread enters a single-threaded apartment and gets a hidden top-level
/// window, the parent the DSM and drivers are given; both are undone after.
/// The states `f` makes borrow the `Twain`, so none outlives the call, and
/// none can be sent to another thread: there, a [`Remote`] posts requests of
/// the caller's type `T`. When `f` returns (or panics), whatever it left open
/// is closed: the `Twain` keeps a record of what is, apart from the states,
/// so a state forgotten rather than dropped still is.
///
/// One `run` at a time per process, whatever copies of this crate the process
/// holds: the DSM has no locks, and two threads using it would race. The
/// lock is a semaphore named for the process, so every copy takes the same.
/// A second `run` is [`RunError::AlreadyRunning`], as is every `run` after
/// one whose teardown couldn't close the DSM's connection
/// ([`RunReport::stranded`]).
///
/// A state can't leave the closure, kept outside it:
///
/// ```compile_fail,E0521
/// let mut kept = None;
/// twain_dsm::run::<(), _>(|twain| kept = twain.load(twain_dsm::Load::System).ok());
/// ```
///
/// or returned from it (rustc's error for this, "lifetime may not live long
/// enough", has no code):
///
/// ```compile_fail
/// let loaded = twain_dsm::run::<(), _>(|twain| twain.load(twain_dsm::Load::System).ok());
/// ```
///
/// while what it found can, either way:
///
/// ```no_run
/// let mut kept = None;
/// twain_dsm::run::<(), _>(|twain| kept = twain.load(twain_dsm::Load::System).ok().map(|l| l.path().to_owned()));
/// let path = twain_dsm::run::<(), _>(|twain| twain.load(twain_dsm::Load::System).ok().map(|l| l.path().to_owned()));
/// ```
///
/// Nor can a state be sent to another thread:
///
/// ```compile_fail,E0277
/// twain_dsm::run::<(), _>(|twain| {
///     let loaded = twain.load(twain_dsm::Load::System).unwrap();
///     std::thread::scope(|s| {
///         s.spawn(move || drop(loaded));
///     });
/// });
/// ```
///
/// while a [`Remote`] can:
///
/// ```no_run
/// twain_dsm::run::<u32, _>(|twain| {
///     let remote = twain.remote();
///     std::thread::scope(|s| {
///         s.spawn(move || remote.post(7));
///     });
/// });
/// ```
///
/// (Stable rustdoc checks that a `compile_fail` example fails, not why;
/// `just twain-dsm-doc-codes`, on nightly, checks the error codes too.)
pub fn run<T, R>(f: impl for<'t> FnOnce(&'t Twain<T>) -> R) -> Result<RunReport<R>, RunError> {
    let lock = RunLock::take()?;
    let apartment = Apartment::enter()?;
    let window = match Window::create() {
        Ok(window) => window,
        Err(e) => {
            apartment.leave();
            return Err(e);
        }
    };
    window::arm_callback(window.handle());
    let (sender, requests) = mpsc::channel();
    let twain = Twain {
        shared: Arc::new(Shared {
            window: Mutex::new(Some(window.handle())),
            sender,
        }),
        window,
        requests,
        app: Box::new(Cell::new(TW_IDENTITY::default())),
        source: Box::new(Cell::new(TW_IDENTITY::default())),
        ui: Box::new(Cell::new(TW_USERINTERFACE::default())),
        dsm: Cell::new(None),
        loaded: Cell::new(false),
        connected: Cell::new(false),
        source_open: Cell::new(None),
        level: Cell::new(Level::Open),
        forced_ui: Cell::new(false),
        events: RefCell::new(VecDeque::new()),
        held: RefCell::new(VecDeque::new()),
        drop_errors: RefCell::new(Vec::new()),
    };

    let outcome = panic::catch_unwind(AssertUnwindSafe(|| f(&twain)));

    // What the closure left open, closed: the source first (the DSM doesn't
    // close every source left open itself), then the connection.
    let closed_source = twain.source_open.get().is_some();
    let source_left = twain.close_source().err();
    let closed_connection = twain.connected.get();
    let stranded = twain.close_connection().err();
    let Twain {
        window,
        shared,
        app,
        source,
        ui,
        ..
    } = twain;
    // No post reaches the window after this, nor a window that reuses its
    // handle.
    *shared.window.lock().unwrap_or_else(|e| e.into_inner()) = None;
    if stranded.is_some() {
        // The DSM still has this application: its identity, the window it was
        // given and the apartment stay (the window isn't destroyed, the
        // apartment isn't left), and so does the flag.
        Box::leak(app);
        Box::leak(source);
        Box::leak(ui);
        lock.keep();
    } else {
        window::disarm_callback();
        window.destroy();
        apartment.leave();
        drop(lock);
    }
    match outcome {
        Ok(value) => Ok(RunReport {
            value,
            closed_source,
            closed_connection,
            source_left,
            stranded,
        }),
        Err(payload) => panic::resume_unwind(payload),
    }
}

/// What [`run`] returns: the closure's value, and whether its teardown was
/// clean.
#[derive(Debug)]
#[non_exhaustive]
pub struct RunReport<R> {
    /// The closure's value.
    pub value: R,
    /// Whether the closure left a source open, which the end of the run
    /// stepped down and closed.
    pub closed_source: bool,
    /// Whether it left the connection open, which the end of the run closed.
    pub closed_connection: bool,
    /// `Some` if a source left open by the closure wouldn't close at the end:
    /// the connection's close was still tried, but the driver may still
    /// count the connection (a later `MSG_OPENDS` of it can fail
    /// `TWCC_MAXCONNECTIONS` until the process ends).
    pub source_left: Option<Error>,
    /// `Some` if the DSM refused to close the connection at the end: then the
    /// application's identity, the window and the apartment are left as they
    /// were (the DSM may still use them), and no `run` can follow in this
    /// process.
    pub stranded: Option<Error>,
}

/// The TWAIN thread, while [`run`] lasts: the states are made from it, and
/// it pumps the thread's messages. `T` is the type of the requests other
/// threads post through a [`Remote`].
///
/// Neither `Send` nor `Sync`: it belongs to its thread, and so does every
/// state that borrows it.
pub struct Twain<T> {
    pub(crate) window: Window,
    requests: mpsc::Receiver<T>,
    shared: Arc<Shared<T>>,
    /// The application's identity, the open source's, and the
    /// `TW_USERINTERFACE` of its enable (`MSG_DISABLEDS` is given the same), at
    /// addresses that don't move while the DSM and the source have them.
    pub(crate) app: Box<Cell<TW_IDENTITY>>,
    pub(crate) source: Box<Cell<TW_IDENTITY>>,
    pub(crate) ui: Box<Cell<TW_USERINTERFACE>>,
    /// The record, kept apart from the states: what's open, whatever became
    /// of the state that opened it.
    pub(crate) dsm: Cell<Option<&'static Dsm>>,
    pub(crate) loaded: Cell<bool>,
    pub(crate) connected: Cell<bool>,
    /// The open source's delivery, if one is open, and its state.
    pub(crate) source_open: Cell<Option<Delivery>>,
    pub(crate) level: Cell<Level>,
    /// Whether `MSG_ENABLEDS` answered `TWRC_CHECKSTATUS`: the source showed
    /// its window though asked not to.
    pub(crate) forced_ui: Cell<bool>,
    /// Messages from the source, as they came, not yet handed over.
    pub(crate) events: RefCell<VecDeque<u16>>,
    /// Requests taken from the channel and put back, ahead of it.
    pub(crate) held: RefCell<VecDeque<T>>,
    /// What states' `Drop`s couldn't do, for `drop_errors`.
    drop_errors: RefCell<Vec<Error>>,
}

/// The most errors `drop_errors` keeps between calls.
const DROP_ERRORS_KEPT: usize = 16;

/// Where the open source stands, as the record has it: TWAIN's states 4 to 7.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Level {
    Open = 4,
    Enabled = 5,
    Ready = 6,
    Transferring = 7,
}

/// Why a pump stopped.
pub(crate) enum Pumped {
    /// What it waited for came.
    Stop,
    Timeout,
    Quit(i32),
}

/// When a pump stops waiting: `now + timeout`, or never (`None`), for a
/// timeout too long to add.
pub(crate) fn deadline(timeout: Duration) -> Option<Instant> {
    Instant::now().checked_add(timeout)
}

struct Shared<T> {
    /// The window to post to, while it lives.
    window: Mutex<Option<usize>>,
    sender: mpsc::Sender<T>,
}

/// What [`Twain::wait`] woke for.
#[derive(Debug)]
#[non_exhaustive]
pub enum Wake<T> {
    /// A request a [`Remote`] posted.
    Request(T),
    /// The time ran out.
    Timeout,
    /// The thread was asked to quit (`WM_QUIT`), with the code given: taken
    /// from the queue, so the caller decides.
    Quit(i32),
}

impl<T> Twain<T> {
    /// A handle another thread posts requests with.
    pub fn remote(&self) -> Remote<T> {
        Remote {
            shared: Arc::clone(&self.shared),
        }
    }

    /// What the states dropped since the last call couldn't do: each `Drop`
    /// steps down, and has nowhere to put a refusal but here (the first 16).
    pub fn drop_errors(&self) -> Vec<Error> {
        core::mem::take(&mut *self.drop_errors.borrow_mut())
    }

    /// A state's `Drop`'s result, kept if it's an error.
    pub(crate) fn dropped(&self, result: Result<(), Error>) {
        if let Err(error) = result {
            let mut errors = self.drop_errors.borrow_mut();
            if errors.len() < DROP_ERRORS_KEPT {
                errors.push(error);
            }
        }
    }

    /// Puts `request` back, ahead of every request waiting: the next
    /// [`take_request`](Twain::take_request), [`wait`](Twain::wait) or
    /// `next_event` hands it over first.
    pub fn requeue(&self, request: T) {
        self.held.borrow_mut().push_front(request);
    }

    /// The next request posted, if one is waiting.
    pub fn take_request(&self) -> Option<T> {
        if let Some(request) = self.held.borrow_mut().pop_front() {
            return Some(request);
        }
        self.requests.try_recv().ok()
    }

    /// Pumps the thread's messages until a request is posted, the thread is
    /// asked to quit, or `timeout` passes. Other windows' messages on this
    /// thread are dispatched as a message loop would.
    ///
    /// While a source is enabled, its messages are kept for
    /// [`Enabled::next_event`](crate::Enabled::next_event): each of the
    /// thread's messages but the crate's own is offered to it first
    /// (`MSG_PROCESSEVENT`). A timeout too long to add to now waits without
    /// one.
    pub fn wait(&self, timeout: Duration) -> Wake<T> {
        let mut request = None;
        let pumped = self.pump(deadline(timeout), |twain| {
            if request.is_none() {
                request = twain.take_request();
            }
            request.is_some()
        });
        match (pumped, request) {
            (_, Some(request)) => Wake::Request(request),
            (Pumped::Quit(code), None) => Wake::Quit(code),
            _ => Wake::Timeout,
        }
    }

    /// Pumps the thread's messages until `done` says so, `deadline` passes,
    /// or the thread is asked to quit. Each turn takes every message queued
    /// (or as many as come before the deadline), then what the source has
    /// said ([`poll_source`](Twain::poll_source)), then asks `done`: so a
    /// source's message queued already is seen before a request is handed
    /// over.
    pub(crate) fn pump(
        &self,
        deadline: Option<Instant>,
        mut done: impl FnMut(&Self) -> bool,
    ) -> Pumped {
        let past = |deadline: Option<Instant>| deadline.is_some_and(|d| Instant::now() >= d);
        loop {
            while let Some(mut msg) = window::take_message() {
                if msg.message() == windows_sys::Win32::UI::WindowsAndMessaging::WM_QUIT {
                    return Pumped::Quit(msg.wparam() as i32);
                }
                // The crate's own wake-up (matched by window and number) is
                // never offered to the source: what it woke for is in the
                // ring or the channel.
                let ours =
                    msg.hwnd() == self.window.handle() && msg.message() == window::wake_message();
                if !ours && !self.offered_to_source(&mut msg) {
                    msg.dispatch();
                }
                if past(deadline) {
                    break;
                }
            }
            self.poll_source();
            if done(self) {
                return Pumped::Stop;
            }
            let wait = match deadline {
                Some(d) => match d.checked_duration_since(Instant::now()) {
                    Some(left) if !left.is_zero() => Some(left),
                    _ => return Pumped::Timeout,
                },
                None => None,
            };
            window::wait_for_message(wait);
        }
    }

    /// What the source has said that no message carried: the callback's ring
    /// read; and with delivery by events, a made-up `WM_NULL` offered to
    /// `MSG_PROCESSEVENT`, which the DSM answers with a message it holds for
    /// the application (`dsm.cpp`, the start of `DSM_Entry`), in case another
    /// message loop on this thread took the message that would have carried
    /// it.
    pub(crate) fn poll_source(&self) {
        let current = window::generation();
        let delivery = self.source_open.get();
        window::take_source_messages(|generation, message| {
            if generation == current && delivery == Some(Delivery::Callback) {
                self.source_message(message);
            }
        });
        if delivery == Some(Delivery::Events) && self.level.get() >= Level::Enabled {
            let mut null = Message::null(self.window.hwnd());
            self.offered_to_source(&mut null);
        }
    }

    /// With a source enabled, `msg` offered to it (`MSG_PROCESSEVENT`, in
    /// either delivery, as TWAIN asks: the source's own windows need it):
    /// true if it was the source's, and isn't to be dispatched. In delivery by
    /// events the DSM answers `TWRC_DSEVENT` for whatever message comes first
    /// after the source has one for the application, and hands that over in
    /// it: for a TWAIN 2 source, whose messages go through the DSM so, that
    /// carrier was someone else's message, and is dispatched still.
    fn offered_to_source(&self, msg: &mut Message) -> bool {
        let (Some(dsm), Some(delivery)) = (self.dsm.get(), self.source_open.get()) else {
            return false;
        };
        if self.level.get() < Level::Enabled {
            return false;
        }
        let (rc, message) = dsm.process_event(&self.app, &self.source, msg);
        if rc != ReturnCode::DSEVENT {
            return false;
        }
        if message == ffi::MSG_NULL {
            return true;
        }
        self.source_message(message);
        let carrier = delivery == Delivery::Events && self.source_is_twain2();
        !carrier
    }

    fn source_is_twain2(&self) -> bool {
        self.source.get().SupportedGroups & ffi::DF_DS2 != 0
    }

    /// A message from the source, kept for `next_event`, the record moved as
    /// the source has: `MSG_XFERREADY` is taken only in state 5 (TWAIN allows
    /// it nowhere else, and one from an earlier enable, or said twice, would
    /// make a `Ready` of a source that isn't), and moves the record to 6 as
    /// it comes, the source being there already (the sample, by callback,
    /// before `MSG_ENABLEDS` returns), so a step down starts with
    /// `MSG_RESET`; `MSG_CLOSEDSREQ` in 6 or 7 moves it to 5, where the source
    /// goes on saying it.
    fn source_message(&self, message: u16) {
        match message {
            ffi::MSG_NULL => return,
            ffi::MSG_XFERREADY => {
                if self.level.get() != Level::Enabled {
                    return;
                }
                self.level.set(Level::Ready);
            }
            ffi::MSG_CLOSEDSREQ if self.level.get() > Level::Enabled => {
                self.level.set(Level::Enabled)
            }
            _ => {}
        }
        self.events.borrow_mut().push_back(message);
    }

    /// The connection closed if the record says it's open: `MSG_CLOSEDSM`,
    /// which closes any source still open too.
    pub(crate) fn close_connection(&self) -> Result<(), Error> {
        let Some(dsm) = self.dsm.get() else {
            return Ok(());
        };
        if !self.connected.get() {
            return Ok(());
        }
        #[cfg(test)]
        if tests::REFUSE_CLOSEDSM.load(std::sync::atomic::Ordering::Relaxed) {
            return Err(Error::Failed {
                triplet: crate::Triplet::new(ffi::DG_CONTROL, ffi::DAT_PARENT, ffi::MSG_CLOSEDSM),
                rc: ReturnCode::FAILURE,
                by_source: None,
                by_dsm: None,
            });
        }
        let (triplet, rc) = dsm.parent(&self.app, false, self.window.hwnd());
        if rc == ReturnCode::SUCCESS {
            self.connected.set(false);
            self.source_open.set(None);
            return Ok(());
        }
        Err(Error::Failed {
            triplet,
            rc,
            by_source: None,
            by_dsm: dsm.dsm_condition(&self.app),
        })
    }
}

impl<T> fmt::Debug for Twain<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Twain")
            .field("loaded", &self.loaded.get())
            .field("connected", &self.connected.get())
            .finish_non_exhaustive()
    }
}

/// Posts requests to a TWAIN thread from any thread. `Send` and `Sync` when
/// `T` is `Send`; cloned freely.
pub struct Remote<T> {
    shared: Arc<Shared<T>>,
}

impl<T> Clone for Remote<T> {
    fn clone(&self) -> Self {
        Remote {
            shared: Arc::clone(&self.shared),
        }
    }
}

impl<T> fmt::Debug for Remote<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Remote(..)")
    }
}

impl<T> Remote<T> {
    /// Queues `request` and wakes the TWAIN thread's loop. After its `run`
    /// ends, the request is handed back.
    pub fn post(&self, request: T) -> Result<(), Gone<T>> {
        let window = self.shared.window.lock().unwrap_or_else(|e| e.into_inner());
        let Some(handle) = *window else {
            return Err(Gone(request));
        };
        self.shared.sender.send(request).map_err(|e| Gone(e.0))?;
        // Under the lock, so the window is alive. If the post fails (a full
        // queue), the request waits in the channel for the loop's next turn.
        window::post(handle, window::wake_message());
        Ok(())
    }
}

#[cfg(test)]
#[allow(unsafe_code)]
pub(crate) mod tests {
    use std::sync::atomic::{AtomicBool, Ordering};

    use windows_sys::Win32::Foundation::{CloseHandle, WAIT_OBJECT_0};
    use windows_sys::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx};
    use windows_sys::Win32::System::Threading::{
        CreateSemaphoreW, GetCurrentProcessId, ReleaseSemaphore, WaitForSingleObject,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{IsWindow, PostMessageW, WM_CLOSE};

    use super::*;
    use crate::states::{App, Load};
    use crate::test_support::{self, Outcome};

    /// A make-fail: `MSG_CLOSEDSM` taken as refused, without being sent.
    pub(crate) static REFUSE_CLOSEDSM: AtomicBool = AtomicBool::new(false);

    /// The DSM refusing to close the connection at the end of a run: the run
    /// reports it, leaves the window and the apartment as they are, and keeps
    /// the flag, so no run follows; nothing crashes.
    #[test]
    fn a_connection_the_dsm_wont_close_strands_the_run() {
        test_support::fork(
            rusty_fork::rusty_fork_test_name!(a_connection_the_dsm_wont_close_strands_the_run),
            rusty_fork::rusty_fork_id!(),
            || {
                REFUSE_CLOSEDSM.store(true, Ordering::Relaxed);
                let report = run::<(), _>(|twain| {
                    let loaded = match twain.load(Load::System) {
                        Ok(loaded) => loaded,
                        Err(crate::LoadError::NotFound { .. }) => {
                            return Err(Outcome::Skip("no DSM installed".into()));
                        }
                        Err(e) => panic!("the system's DSM: {e}"),
                    };
                    let app = App::new("pdf-scan", "twain-dsm tests", "unit").expect("fits");
                    let opened = loaded
                        .open(app)
                        .map_err(crate::Refused::into_error)
                        .expect("opens");
                    drop(opened);
                    let errors = twain.drop_errors();
                    assert_eq!(errors.len(), 1, "the Opened's drop was refused: {errors:?}");
                    Ok(twain.window.handle())
                })
                .expect("TWAIN runs");
                let window = match report.value {
                    Ok(window) => window,
                    Err(outcome) => return outcome,
                };
                assert!(
                    matches!(
                        report.stranded,
                        Some(Error::Failed {
                            rc: ReturnCode::FAILURE,
                            ..
                        })
                    ),
                    "stranded: {:?}",
                    report.stranded
                );
                // SAFETY: IsWindow takes any handle and says whether it's a
                // window's.
                assert_ne!(unsafe { IsWindow(window as _) }, 0, "the window is left");
                // SAFETY: the reserved argument is null. S_FALSE (1): this
                // thread is a single-threaded apartment still. The child
                // process ends without balancing it.
                let hr =
                    unsafe { CoInitializeEx(core::ptr::null(), COINIT_APARTMENTTHREADED as u32) };
                assert_eq!(hr, 1, "the apartment is left");
                assert!(matches!(
                    run::<(), _>(|_| ()),
                    Err(RunError::AlreadyRunning)
                ));
                Outcome::Done
            },
        );
    }

    /// A `WM_CLOSE` posted to the hidden window, as anyone may, is ignored:
    /// the window lives, and a request still wakes the loop.
    #[test]
    fn the_hidden_window_outlives_a_wm_close() {
        test_support::fork(
            rusty_fork::rusty_fork_test_name!(the_hidden_window_outlives_a_wm_close),
            rusty_fork::rusty_fork_id!(),
            || {
                run::<u32, _>(|twain| {
                    let hwnd = twain.window.hwnd();
                    // SAFETY: posting copies its arguments; the window is this
                    // run's.
                    assert_ne!(unsafe { PostMessageW(hwnd, WM_CLOSE, 0, 0) }, 0);
                    assert!(matches!(
                        twain.wait(Duration::from_millis(100)),
                        Wake::Timeout
                    ));
                    // SAFETY: IsWindow takes any handle.
                    let alive = unsafe { IsWindow(hwnd) };
                    assert_ne!(alive, 0, "the window was closed from outside");
                    let remote = twain.remote();
                    std::thread::spawn(move || remote.post(5).expect("posted"));
                    let started = Instant::now();
                    match twain.wait(Duration::from_secs(30)) {
                        Wake::Request(5) => {}
                        other => panic!("{other:?}"),
                    }
                    assert!(
                        started.elapsed() < Duration::from_secs(2),
                        "the post woke the loop"
                    );
                })
                .expect("TWAIN runs");
                Outcome::Done
            },
        );
    }

    /// The run lock is the process's, not this copy of the crate's: with
    /// another copy holding it (the named semaphore taken here), a run is
    /// refused; once it's given back, one runs.
    #[test]
    fn the_run_lock_is_the_processs_whatever_copy_holds_it() {
        test_support::fork(
            rusty_fork::rusty_fork_test_name!(the_run_lock_is_the_processs_whatever_copy_holds_it),
            rusty_fork::rusty_fork_id!(),
            || {
                // SAFETY: no arguments.
                let pid = unsafe { GetCurrentProcessId() };
                let name = window::wide(&format!("Local\\twain-dsm run {pid}"));
                // SAFETY: as the lock makes it; `name` is NUL-terminated.
                let other = unsafe { CreateSemaphoreW(core::ptr::null(), 1, 1, name.as_ptr()) };
                assert!(!other.is_null());
                // SAFETY: the semaphore just made; taken at once.
                assert_eq!(unsafe { WaitForSingleObject(other, 0) }, WAIT_OBJECT_0);
                assert!(matches!(
                    run::<(), _>(|_| ()),
                    Err(RunError::AlreadyRunning)
                ));
                // SAFETY: given back once, its handle closed.
                unsafe {
                    ReleaseSemaphore(other, 1, core::ptr::null_mut());
                    CloseHandle(other);
                }
                run::<(), _>(|_| ()).expect("runs once the other copy is done");
                Outcome::Done
            },
        );
    }

    /// `drop_errors` keeps the first 16, and empties.
    #[test]
    fn drop_errors_keeps_sixteen() {
        test_support::fork(
            rusty_fork::rusty_fork_test_name!(drop_errors_keeps_sixteen),
            rusty_fork::rusty_fork_id!(),
            || {
                run::<(), _>(|twain| {
                    for _ in 0..20 {
                        twain.dropped(Err(Error::StillOpen));
                    }
                    assert_eq!(twain.drop_errors().len(), 16);
                    assert!(twain.drop_errors().is_empty());
                })
                .expect("TWAIN runs");
                Outcome::Done
            },
        );
    }

    /// The end of the run says what it closed, and sends `MSG_CLOSEDSM` for a
    /// connection forgotten, and nothing for one closed.
    #[test]
    fn the_report_says_what_the_teardown_closed() {
        test_support::fork(
            rusty_fork::rusty_fork_test_name!(the_report_says_what_the_teardown_closed),
            rusty_fork::rusty_fork_id!(),
            || {
                let closedsm =
                    crate::Triplet::new(ffi::DG_CONTROL, ffi::DAT_PARENT, ffi::MSG_CLOSEDSM);
                for forget in [true, false] {
                    let report = run::<(), _>(|twain| {
                        let loaded = match twain.load(Load::System) {
                            Ok(loaded) => loaded,
                            Err(crate::LoadError::NotFound { .. }) => {
                                return Err(Outcome::Skip("no DSM installed".into()));
                            }
                            Err(e) => panic!("the system's DSM: {e}"),
                        };
                        let app = App::new("pdf-scan", "twain-dsm tests", "unit").expect("fits");
                        let opened = loaded
                            .open(app)
                            .map_err(crate::Refused::into_error)
                            .expect("opens");
                        if forget {
                            core::mem::forget(opened);
                        } else {
                            drop(opened);
                        }
                        crate::dsm::sent_log::take();
                        Ok(())
                    })
                    .expect("TWAIN runs");
                    if let Err(outcome) = report.value {
                        return outcome;
                    }
                    let sent = crate::dsm::sent_log::take();
                    let closes = sent.iter().filter(|(t, _)| *t == closedsm).count();
                    assert_eq!(report.closed_connection, forget);
                    assert!(!report.closed_source);
                    assert_eq!(closes, usize::from(forget), "forget {forget}: {sent:?}");
                }
                Outcome::Done
            },
        );
    }
}
