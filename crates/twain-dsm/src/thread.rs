//! The TWAIN thread: the caller's, for the length of [`run`].

use core::cell::Cell;
use core::fmt;
use std::panic::{self, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

use crate::codes::ReturnCode;
use crate::dsm::Dsm;
use crate::error::{Error, Gone, RunError};
use crate::ffi::TW_IDENTITY;
use crate::window::{self, Apartment, WM_REQUEST, Window};

/// Whether a `run` is going on in this process.
static RUNNING: AtomicBool = AtomicBool::new(false);

/// Releases [`RUNNING`] when dropped, panics included; a teardown that had to
/// leave the DSM's connection open forgets it, and the flag stays taken.
struct Running;

impl Drop for Running {
    fn drop(&mut self) {
        RUNNING.store(false, Ordering::Release);
    }
}

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
/// One `run` at a time per process: the DSM has no locks, and two threads
/// using it would race. A second is [`RunError::AlreadyRunning`], as is every
/// `run` after one whose teardown couldn't close the DSM's connection
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
    if RUNNING
        .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
        .is_err()
    {
        return Err(RunError::AlreadyRunning);
    }
    let running = Running;
    let apartment = Apartment::enter()?;
    let window = match Window::create() {
        Ok(window) => window,
        Err(e) => {
            apartment.leave();
            return Err(e);
        }
    };
    let (sender, requests) = mpsc::channel();
    let twain = Twain {
        shared: Arc::new(Shared {
            window: Mutex::new(Some(window.handle())),
            sender,
        }),
        window,
        requests,
        app: Box::new(Cell::new(TW_IDENTITY::default())),
        dsm: Cell::new(None),
        loaded: Cell::new(false),
        connected: Cell::new(false),
    };

    let outcome = panic::catch_unwind(AssertUnwindSafe(|| f(&twain)));

    // What the closure left open, closed. The DSM closes a source still open
    // itself when its application closes the connection.
    let stranded = twain.close_connection().err();
    let Twain {
        window,
        shared,
        app,
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
        core::mem::forget(running);
    } else {
        window.destroy();
        apartment.leave();
        drop(running);
    }
    match outcome {
        Ok(value) => Ok(RunReport { value, stranded }),
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
    /// The application's identity, at an address that doesn't move while
    /// the DSM has it.
    pub(crate) app: Box<Cell<TW_IDENTITY>>,
    /// The record, kept apart from the states: what's open, whatever became
    /// of the state that opened it.
    pub(crate) dsm: Cell<Option<&'static Dsm>>,
    pub(crate) loaded: Cell<bool>,
    pub(crate) connected: Cell<bool>,
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

    /// The next request posted, if one is waiting.
    pub fn take_request(&self) -> Option<T> {
        self.requests.try_recv().ok()
    }

    /// Pumps the thread's messages until a request is posted, the thread is
    /// asked to quit, or `timeout` passes. Other windows' messages on this
    /// thread are dispatched as a message loop would.
    pub fn wait(&self, timeout: Duration) -> Wake<T> {
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(request) = self.take_request() {
                return Wake::Request(request);
            }
            while let Some(msg) = window::take_message() {
                if msg.message() == windows_sys::Win32::UI::WindowsAndMessaging::WM_QUIT {
                    return Wake::Quit(msg.wparam() as i32);
                }
                if msg.hwnd() == self.window.handle() && msg.message() == WM_REQUEST {
                    // The request itself is in the channel.
                    continue;
                }
                msg.dispatch();
            }
            if let Some(request) = self.take_request() {
                return Wake::Request(request);
            }
            let now = Instant::now();
            if now >= deadline {
                return Wake::Timeout;
            }
            window::wait_for_message(deadline - now);
        }
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
        let (triplet, rc) = dsm.parent(&self.app, false, self.window.hwnd());
        if rc == ReturnCode::SUCCESS {
            self.connected.set(false);
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
        window::post(handle, WM_REQUEST);
        Ok(())
    }
}
