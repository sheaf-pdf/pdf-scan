//! SANE loaded, called, and what it returns copied into owned values.
//!
//! The one module that holds unsafe code. Every call into the library goes
//! through it, and so do the rules that make those calls sound: one live
//! [`Sane`] per process, so `sane_init` and `sane_exit` pair up; one call at
//! a time ("the call token", below), so no two SANE calls overlap, as the
//! standard requires of every function but `sane_cancel`; a handle closed
//! once and never used after; `sane_exit` only once every handle is closed;
//! and whatever SANE returns copied before anything else is called, since
//! SANE keeps it valid only until then.
//!
//! **The call token.** `State::busy` says who may call SANE: nobody
//! (`Idle`), one caller (`Call`), or a device listing (`Listing`), which runs
//! on a thread of its own so its caller can stop waiting at a deadline. A
//! caller takes the token under the state's mutex, calls SANE with the mutex
//! released, and gives the token back. Past a listing's deadline the listing
//! is marked late: callers then get [`Error::ListingPending`] at once rather
//! than wait, and a handle dropped meanwhile leaves its `sane_close` to the
//! listing's thread, which runs it when the listing returns, before giving
//! the token back.

#![allow(unsafe_code)]

use std::ffi::{CString, c_char};
use std::fmt;
use std::mem;
use std::path::{Path, PathBuf};
use std::ptr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use crate::{Call, Device, DeviceName, Error, Limits, LoadError, Status, Version, ffi};

/// SANE's 14 functions, looked up in a library that's never unloaded, so
/// each pointer stays valid for the rest of the process.
#[derive(Clone, Copy)]
struct Api {
    init: ffi::SaneInitFn,
    exit: ffi::SaneExitFn,
    get_devices: ffi::SaneGetDevicesFn,
    open: ffi::SaneOpenFn,
    close: ffi::SaneCloseFn,
    strstatus: ffi::SaneStrstatusFn,
    // Looked up with the rest, so a library without them is refused when
    // it's loaded; called by the option and scanning calls.
    #[allow(dead_code)]
    get_option_descriptor: ffi::SaneGetOptionDescriptorFn,
    #[allow(dead_code)]
    control_option: ffi::SaneControlOptionFn,
    #[allow(dead_code)]
    get_parameters: ffi::SaneGetParametersFn,
    #[allow(dead_code)]
    start: ffi::SaneStartFn,
    #[allow(dead_code)]
    read: ffi::SaneReadFn,
    #[allow(dead_code)]
    cancel: ffi::SaneCancelFn,
    #[allow(dead_code)]
    set_io_mode: ffi::SaneSetIoModeFn,
    #[allow(dead_code)]
    get_select_fd: ffi::SaneGetSelectFdFn,
}

impl Api {
    /// Opens the first of `names` that loads, never to unload it, and looks
    /// up SANE's functions in it.
    fn load(names: &[PathBuf]) -> Result<Api, LoadError> {
        let mut last_error = None;
        for name in names {
            // SAFETY: opening a library runs its initialisers in this
            // process. The caller chose the library as SANE's (`load_from`'s
            // contract, or the system's own by its soname), and trusting it
            // is the premise of loading it at all. The library is never
            // unloaded, so no finaliser of it runs while anything it set up
            // may still be in use.
            match unsafe { libloading::Library::new(name) } {
                Ok(library) => return Api::look_up(Box::leak(Box::new(library))),
                Err(error) => last_error = Some(error),
            }
        }
        let source = match last_error {
            Some(error) => Box::new(error) as Box<dyn std::error::Error + Send + Sync>,
            None => "no library name to try".into(),
        };
        Err(LoadError::NotFound {
            tried: names.to_vec(),
            source,
        })
    }

    fn look_up(library: &'static libloading::Library) -> Result<Api, LoadError> {
        macro_rules! symbol {
            ($name:literal, $type:ty) => {{
                let symbol = concat!($name, "\0").as_bytes();
                // SAFETY: a library that exports SANE's names is taken to be
                // SANE's (the premise of loading it), so the type is the
                // binding's for this function as `sane.h` declares it (the
                // header contract checks every binding against the header).
                // It's read as an `Option`, whose null niche makes a null
                // address `None` rather than a null function pointer. The
                // pointer is copied out of the `Symbol`, past its borrow of
                // the library, which is sound because the library is leaked
                // and so never unloaded.
                let found = unsafe { library.get::<Option<$type>>(symbol) };
                match found.map(|symbol| *symbol) {
                    Ok(Some(function)) => function,
                    Ok(None) => {
                        return Err(LoadError::MissingSymbol {
                            name: $name,
                            source: "its address is null".into(),
                        });
                    }
                    Err(error) => {
                        return Err(LoadError::MissingSymbol {
                            name: $name,
                            source: Box::new(error),
                        });
                    }
                }
            }};
        }
        Ok(Api {
            init: symbol!("sane_init", ffi::SaneInitFn),
            exit: symbol!("sane_exit", ffi::SaneExitFn),
            get_devices: symbol!("sane_get_devices", ffi::SaneGetDevicesFn),
            open: symbol!("sane_open", ffi::SaneOpenFn),
            close: symbol!("sane_close", ffi::SaneCloseFn),
            get_option_descriptor: symbol!(
                "sane_get_option_descriptor",
                ffi::SaneGetOptionDescriptorFn
            ),
            control_option: symbol!("sane_control_option", ffi::SaneControlOptionFn),
            get_parameters: symbol!("sane_get_parameters", ffi::SaneGetParametersFn),
            start: symbol!("sane_start", ffi::SaneStartFn),
            read: symbol!("sane_read", ffi::SaneReadFn),
            cancel: symbol!("sane_cancel", ffi::SaneCancelFn),
            set_io_mode: symbol!("sane_set_io_mode", ffi::SaneSetIoModeFn),
            get_select_fd: symbol!("sane_get_select_fd", ffi::SaneGetSelectFdFn),
            strstatus: symbol!("sane_strstatus", ffi::SaneStrstatusFn),
        })
    }
}

/// Whether this process has a live [`Sane`]: from before its `sane_init`
/// until after its `sane_exit` has returned.
static LOADED: AtomicBool = AtomicBool::new(false);

/// The process's one slot, held from before `sane_init` until after
/// `sane_exit`, and freed when dropped.
struct Slot(());

impl Slot {
    fn take() -> Result<Slot, LoadError> {
        LOADED
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .map(|_| Slot(()))
            .map_err(|_| LoadError::AlreadyLoaded)
    }
}

impl Drop for Slot {
    fn drop(&mut self) {
        LOADED.store(false, Ordering::Release);
    }
}

/// A handle `sane_open` gave, which only this module's code turns into
/// calls.
#[derive(Clone, Copy)]
struct RawHandle(ffi::SANE_Handle);

// SAFETY: a SANE handle is an opaque token, never dereferenced here. The
// standard says nothing of threads: it forbids only calls that overlap, and
// every call made with a handle holds the call token, whichever thread makes
// it.
unsafe impl Send for RawHandle {}

/// Who may call SANE.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Busy {
    /// Nobody: the token is free.
    Idle,
    /// A caller, until its call returns.
    Call,
    /// A device listing, on its own thread, until it returns and has closed
    /// the handles left to it; `late` once its caller's deadline has passed.
    Listing { id: u64, late: bool },
}

struct State {
    busy: Busy,
    /// Handles dropped while a late listing held the token, for the
    /// listing's thread to close.
    pending_closes: Vec<RawHandle>,
    next_listing: u64,
    limits: Limits,
}

/// One loaded SANE: its functions, its version, the call token, and the
/// process's slot. When the last reference goes (the `Sane`, its handles and
/// any listing still running all hold one), `sane_exit` runs.
struct Shared {
    api: Api,
    version: Version,
    state: Mutex<State>,
    /// Signalled whenever the call token is given back, or a listing turns
    /// late.
    wake: Condvar,
    /// Dropped after `Drop::drop` below has run `sane_exit`.
    _slot: Slot,
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, State> {
        // Nothing panics while holding the mutex but a failed allocation,
        // and the state is consistent between statements, so a poisoned
        // mutex is used as it is.
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Takes the call token, waiting for it until `until` if given:
    /// `ListingPending` at once if a late listing holds it,
    /// `DeadlineExceeded` if `until` passes first.
    fn acquire(&self, until: Option<Instant>) -> Result<CallGuard<'_>, Error> {
        let mut state = self.lock();
        loop {
            match state.busy {
                Busy::Idle => {
                    state.busy = Busy::Call;
                    let limits = state.limits;
                    return Ok(CallGuard {
                        shared: self,
                        limits,
                    });
                }
                Busy::Listing { late: true, .. } => return Err(Error::ListingPending),
                Busy::Call | Busy::Listing { late: false, .. } => {
                    state = match until {
                        None => self
                            .wake
                            .wait(state)
                            .unwrap_or_else(PoisonError::into_inner),
                        Some(until) => {
                            let now = Instant::now();
                            if now >= until {
                                return Err(Error::DeadlineExceeded);
                            }
                            self.wake
                                .wait_timeout(state, until - now)
                                .unwrap_or_else(PoisonError::into_inner)
                                .0
                        }
                    };
                }
            }
        }
    }

    /// Closes a handle: now, holding the token, or, if a late listing holds
    /// it, by leaving the handle to the listing's thread, so this never
    /// waits on a listing that may not return.
    fn close(&self, raw: RawHandle) {
        let mut state = self.lock();
        loop {
            match state.busy {
                Busy::Idle => break,
                Busy::Listing { late: true, .. } => {
                    state.pending_closes.push(raw);
                    return;
                }
                Busy::Call | Busy::Listing { late: false, .. } => {
                    state = self
                        .wake
                        .wait(state)
                        .unwrap_or_else(PoisonError::into_inner);
                }
            }
        }
        state.busy = Busy::Call;
        let limits = state.limits;
        drop(state);
        let guard = CallGuard {
            shared: self,
            limits,
        };
        // SAFETY: this caller holds the call token, so no other SANE call
        // runs. `raw` came from `sane_open` and its `Handle` is being
        // dropped, which happens once, so it's closed once and never used
        // after. SANE is initialised: the handle's reference keeps `Shared`,
        // and so SANE, alive.
        unsafe { (self.api.close)(raw.0) };
        drop(guard);
    }
}

impl Drop for Shared {
    fn drop(&mut self) {
        let state = self.state.get_mut().unwrap_or_else(PoisonError::into_inner);
        // A listing's thread closes what's left to it before it gives the
        // token back and drops its reference, so this finds none; closed
        // here all the same, before `sane_exit`, if it ever did.
        for raw in mem::take(&mut state.pending_closes) {
            // SAFETY: the last reference is going, so nothing else can call
            // SANE; `raw` came from `sane_open`, was never closed, and isn't
            // used after.
            unsafe { (self.api.close)(raw.0) };
        }
        // SAFETY: the last reference is going: every handle (each holds one)
        // has been closed, and no call or listing (each holds one too) is
        // running, so `sane_exit` overlaps nothing and comes once, after the
        // one `sane_init` this instance made.
        unsafe { (self.api.exit)() };
    }
}

/// The call token, given back when dropped.
struct CallGuard<'a> {
    shared: &'a Shared,
    limits: Limits,
}

impl Drop for CallGuard<'_> {
    fn drop(&mut self) {
        let mut state = self.shared.lock();
        state.busy = Busy::Idle;
        self.shared.wake.notify_all();
    }
}

/// The call token, held by a device listing's thread: when dropped it
/// closes the handles left to it, then gives the token back.
struct ListingGuard {
    shared: Arc<Shared>,
    limits: Limits,
}

impl ListingGuard {
    /// Lists the devices and copies the list.
    fn list(&self, local_only: bool) -> Result<Vec<Device>, Error> {
        let mut list: *mut *const ffi::SANE_Device = ptr::null_mut();
        // SAFETY: this thread holds the call token, so no other SANE call
        // runs; SANE is initialised (this guard keeps `Shared` alive); `list`
        // is valid for the write SANE makes.
        let status = Status(unsafe {
            (self.shared.api.get_devices)(&mut list, ffi::SANE_Bool::from(local_only))
        });
        if !status.is_good() {
            return Err(Error::Status {
                status,
                call: Call::GetDevices,
            });
        }
        // SAFETY: `sane_get_devices` returned GOOD, so `list` is SANE's
        // device list, valid until the next listing or `sane_exit`, neither
        // of which can run while this thread holds the token.
        unsafe { copy_device_list(list, self.limits) }
    }
}

impl Drop for ListingGuard {
    fn drop(&mut self) {
        let shared = &*self.shared;
        let mut state = shared.lock();
        loop {
            if state.pending_closes.is_empty() {
                state.busy = Busy::Idle;
                shared.wake.notify_all();
                return;
            }
            let closes = mem::take(&mut state.pending_closes);
            drop(state);
            for raw in closes {
                // SAFETY: this thread still holds the call token (`busy` is
                // this listing's, which nothing else changes but to mark it
                // late), so no other SANE call runs. Each handle came from
                // `sane_open`, and its `Handle` was dropped without closing
                // it, leaving it here once: it's closed once and never used
                // after. SANE is initialised: this guard keeps `Shared` alive.
                unsafe { (shared.api.close)(raw.0) };
            }
            state = shared.lock();
        }
    }
}

/// A C string SANE gave, read no further than its `NUL` or a cap.
enum CText {
    Null,
    Text(Vec<u8>),
    /// The first `cap` bytes, with no `NUL` among them.
    TooLong(Vec<u8>),
}

/// Copies a C string, reading no further than its `NUL` and no more than
/// `cap` bytes before it.
///
/// # Safety
///
/// `text` is null, or points to a `NUL`-terminated string that stays valid
/// and unchanged for the call.
unsafe fn copy_c_string(text: *const c_char, cap: usize) -> CText {
    if text.is_null() {
        return CText::Null;
    }
    let mut bytes = Vec::new();
    loop {
        // SAFETY: the string is `NUL`-terminated (the caller's contract), and
        // every byte before the first `NUL` is checked not to be one, so this
        // offset is at or before the `NUL`: inside the string.
        let byte = unsafe { *text.add(bytes.len()) } as u8;
        if byte == 0 {
            return CText::Text(bytes);
        }
        if bytes.len() == cap {
            return CText::TooLong(bytes);
        }
        bytes.push(byte);
    }
}

/// Copies SANE's device list, refusing a malformed one.
///
/// # Safety
///
/// `list` is what a `sane_get_devices` that returned `GOOD` gave: null, or
/// an array of pointers to devices ended by a null one, each device's
/// strings null or `NUL`-terminated, all valid and unchanged for the call.
unsafe fn copy_device_list(
    list: *const *const ffi::SANE_Device,
    limits: Limits,
) -> Result<Vec<Device>, Error> {
    let bad = |reason: String| Error::BadDeviceList { reason };
    if list.is_null() {
        return Err(bad(
            "sane_get_devices returned GOOD and a null list".to_owned()
        ));
    }
    let mut devices = Vec::new();
    loop {
        let index = devices.len();
        // SAFETY: the array ends with a null pointer, and the entries before
        // `index` were all found not null, so the array has at least
        // `index + 1` entries: this read is inside it.
        let entry = unsafe { *list.add(index) };
        if entry.is_null() {
            return Ok(devices);
        }
        if index == limits.devices {
            return Err(bad(format!(
                "it lists more than {} devices",
                limits.devices
            )));
        }
        // SAFETY: a non-null entry points to a valid `SANE_Device` (the
        // caller's contract), copied here as plain pointers.
        let device = unsafe { *entry };
        let string = |text, what: &str, required: bool| -> Result<Vec<u8>, Error> {
            // SAFETY: each of a device's strings is null or `NUL`-terminated
            // and valid for the call (the caller's contract).
            match unsafe { copy_c_string(text, limits.string_bytes) } {
                CText::Text(bytes) => Ok(bytes),
                CText::Null if !required => Ok(Vec::new()),
                CText::Null => Err(bad(format!("device {index}'s {what} is null"))),
                CText::TooLong(_) => Err(bad(format!(
                    "device {index}'s {what} is longer than {} bytes",
                    limits.string_bytes
                ))),
            }
        };
        let lossy = |bytes: Vec<u8>| String::from_utf8_lossy(&bytes).into_owned();
        devices.push(Device {
            name: DeviceName::new(string(device.name, "name", true)?),
            vendor: lossy(string(device.vendor, "vendor", false)?),
            model: lossy(string(device.model, "model", false)?),
            kind: lossy(string(device.r#type, "type", false)?),
        });
    }
}

/// SANE, loaded and started: the way to list devices and open them.
///
/// Loading opens the library and runs `sane_init`; SANE is ended
/// (`sane_exit`) when this and every [`Handle`] opened from it are gone, by
/// whichever drop is the last: at once, on its thread, unless a device
/// listing past its deadline still holds the library, whose thread then
/// ends SANE when the listing returns ([`devices`](Self::devices)). The
/// library itself is never unloaded: backends may leave threads, exit
/// handlers or thread-local destructors in it.
///
/// **One per process.** SANE is started once per process, so a second load
/// while one `Sane` lives is [`LoadError::AlreadyLoaded`]; once the first has
/// ended, another may load. (Two copies of this crate in one program would
/// each keep their own count: don't.) `Sane` is `Send` and `Sync`, and clones
/// share the one instance.
///
/// **One call at a time.** SANE lets no call overlap another but
/// `sane_cancel`, so every call through this instance takes its one lock,
/// whatever thread it comes from and whichever handle it's for: calls from
/// several threads wait on each other. A device listing waits on a scan and
/// a scan on a listing, so list devices in a process of their own, as a
/// short-lived worker, rather than beside a scan.
#[derive(Clone)]
pub struct Sane {
    shared: Arc<Shared>,
}

impl Sane {
    /// The name [`load`](Self::load) opens: SANE's library by its versioned
    /// name, found by the system loader's own search, or `None` where SANE
    /// isn't the system's scanner API (Windows).
    pub const DEFAULT_LIBRARY: Option<&'static str> = if cfg!(target_os = "macos") {
        Some("libsane.1.dylib")
    } else if cfg!(unix) {
        Some("libsane.so.1")
    } else {
        None
    };

    /// Loads the system's SANE ([`DEFAULT_LIBRARY`](Self::DEFAULT_LIBRARY))
    /// and starts it.
    ///
    /// Where it isn't installed this is [`LoadError::NotFound`], and on
    /// Windows [`LoadError::NoDefault`]. `sane_init` is called with no
    /// authorization callback, so a backend that asks for a password gets
    /// none.
    pub fn load() -> Result<Sane, LoadError> {
        let Some(name) = Self::DEFAULT_LIBRARY else {
            return Err(LoadError::NoDefault);
        };
        Sane::start(&[PathBuf::from(name)])
    }

    /// Loads the library at `path` as SANE, and starts it.
    ///
    /// Loading a library runs its code in this process, so `path` must name
    /// a library trusted as much as the program itself: one the program
    /// ships or the user chose, never one an untrusted party can write.
    pub fn load_from(path: impl AsRef<Path>) -> Result<Sane, LoadError> {
        Sane::start(&[path.as_ref().to_path_buf()])
    }

    fn start(names: &[PathBuf]) -> Result<Sane, LoadError> {
        let slot = Slot::take()?;
        let api = Api::load(names)?;
        let mut code: ffi::SANE_Int = 0;
        // SAFETY: this caller holds the process's slot, so no other `Sane`
        // lives and the last one's `sane_exit` has returned: nothing else
        // calls SANE, and this `sane_init` follows no other unended one.
        // `code` is valid for SANE's write; no authorization callback is
        // given.
        let status = Status(unsafe { (api.init)(&mut code, None) });
        if !status.is_good() {
            return Err(LoadError::Init(status));
        }
        let version = Version::from_code(code);
        let state = State {
            busy: Busy::Idle,
            pending_closes: Vec::new(),
            next_listing: 0,
            limits: Limits::default(),
        };
        let shared = Shared {
            api,
            version,
            state: Mutex::new(state),
            wake: Condvar::new(),
            _slot: slot,
        };
        if version.major != 1 {
            // Ends SANE, then frees the slot.
            drop(shared);
            return Err(LoadError::UnsupportedVersion(version));
        }
        Ok(Sane {
            shared: Arc::new(shared),
        })
    }

    /// The version `sane_init` gave.
    pub fn version(&self) -> Version {
        self.shared.version
    }

    /// The caps on what this instance accepts from SANE.
    pub fn limits(&self) -> Limits {
        self.shared.lock().limits
    }

    /// Sets the caps on what this instance accepts from SANE, for the calls
    /// that start after this.
    pub fn set_limits(&self, limits: Limits) {
        self.shared.lock().limits = limits;
    }

    /// Lists the devices SANE finds, or only those attached to this machine
    /// (`local_only`), waiting no longer than `deadline`.
    ///
    /// Listing can take seconds, as every backend loaded looks for its
    /// devices, and some backends hang. `sane_get_devices` runs on a thread
    /// of its own, which copies the list there before anything else is
    /// called; this waits for it until the deadline (waiting for another
    /// call to finish first counts against it), then returns
    /// [`Error::DeadlineExceeded`]. A listing past its deadline keeps the
    /// library until it returns: meanwhile every call but a handle's close
    /// returns [`Error::ListingPending`] rather than block, a handle dropped
    /// is closed by the listing's thread once the listing returns, and if
    /// this `Sane` is dropped too, that thread ends SANE after those closes.
    ///
    /// A malformed list ([`Error::BadDeviceList`]) is refused whole: null, a
    /// device with a null name, more devices than [`Limits::devices`], or a
    /// string longer than [`Limits::string_bytes`]. A null vendor, model or
    /// type is empty.
    pub fn devices(&self, local_only: bool, deadline: Duration) -> Result<Vec<Device>, Error> {
        let until = Instant::now().checked_add(deadline);
        let guard = self.shared.acquire(until)?;
        let limits = guard.limits;
        // The token passes from this call to the listing, without being free
        // in between.
        let id = {
            let mut state = self.shared.lock();
            let id = state.next_listing;
            state.next_listing += 1;
            state.busy = Busy::Listing { id, late: false };
            id
        };
        mem::forget(guard);
        let listing = ListingGuard {
            shared: Arc::clone(&self.shared),
            limits,
        };
        let (sender, receiver) = mpsc::channel();
        // If the thread can't start, the closure, and the guard with it, is
        // dropped, which gives the token back.
        std::thread::Builder::new()
            .name("sane-dl listing".to_owned())
            .spawn(move || {
                let result = listing.list(local_only);
                // The guard goes before the result is sent: it closes what
                // was left to it, gives the token back and drops this
                // thread's reference to the library. So a caller that gets
                // its list in time holds every reference left, and the last
                // of them dropped ends SANE at once, on the caller's thread.
                // A caller that stopped waiting gets nothing: the list is
                // dropped, and if this thread held the last reference, it has
                // ended SANE already.
                drop(listing);
                let _ = sender.send(result);
            })
            .map_err(Error::Thread)?;
        let received = match until {
            Some(until) => receiver.recv_timeout(until.saturating_duration_since(Instant::now())),
            None => receiver.recv().map_err(|_| RecvTimeoutError::Disconnected),
        };
        match received {
            Ok(result) => result,
            Err(RecvTimeoutError::Timeout) => {
                let mut state = self.shared.lock();
                // The listing may have returned as the deadline passed.
                if let Ok(result) = receiver.try_recv() {
                    return result;
                }
                if let Busy::Listing { id: current, late } = &mut state.busy
                    && *current == id
                {
                    *late = true;
                    self.shared.wake.notify_all();
                    return Err(Error::DeadlineExceeded);
                }
                drop(state);
                // The listing has given the token back and dropped its
                // reference; its thread sends the result next, without
                // waiting on anything.
                receiver
                    .recv()
                    .unwrap_or_else(|_| panic!("sane-dl: the device listing's thread panicked"))
            }
            Err(RecvTimeoutError::Disconnected) => {
                panic!("sane-dl: the device listing's thread panicked")
            }
        }
    }

    /// Opens the device named, as [`Device::name`] gives it, or as kept from
    /// an earlier listing: SANE opens a device by its name without a list.
    ///
    /// A name SANE doesn't know is `sane_open`'s [`Status::INVAL`], in
    /// [`Error::Status`]; a name holding a `NUL` byte is
    /// [`Error::NulInName`], refused before SANE sees it.
    pub fn open(&self, name: impl AsRef<[u8]>) -> Result<Handle, Error> {
        let name = name.as_ref();
        let c_name = CString::new(name).map_err(|_| Error::NulInName)?;
        let guard = self.shared.acquire(None)?;
        let mut raw: ffi::SANE_Handle = ptr::null_mut();
        // SAFETY: this caller holds the call token, so no other SANE call
        // runs; SANE is initialised (`self` keeps `Shared` alive); `c_name`
        // is a `NUL`-terminated string that outlives the call, and `raw` is
        // valid for SANE's write.
        let status = Status(unsafe { (self.shared.api.open)(c_name.as_ptr(), &mut raw) });
        drop(guard);
        if !status.is_good() {
            return Err(Error::Status {
                status,
                call: Call::Open,
            });
        }
        if raw.is_null() {
            return Err(Error::BadHandle);
        }
        Ok(Handle {
            shared: Arc::clone(&self.shared),
            raw: RawHandle(raw),
            name: DeviceName::from(name),
        })
    }

    /// A status in words, as `sane_strstatus` gives them (no longer than
    /// [`Limits::string_bytes`]), or `None` if it gives none.
    pub fn status_message(&self, status: Status) -> Result<Option<String>, Error> {
        let guard = self.shared.acquire(None)?;
        // SAFETY: this caller holds the call token, so no other SANE call
        // runs; SANE is initialised (`self` keeps `Shared` alive);
        // `sane_strstatus` takes any status.
        let text = unsafe { (self.shared.api.strstatus)(status.0) };
        // SAFETY: SANE gives a null pointer or a `NUL`-terminated string,
        // read here before any other call can change it (the token is still
        // held).
        let copied = unsafe { copy_c_string(text, guard.limits.string_bytes) };
        drop(guard);
        Ok(match copied {
            CText::Null => None,
            CText::Text(bytes) | CText::TooLong(bytes) => {
                Some(String::from_utf8_lossy(&bytes).into_owned())
            }
        })
    }
}

impl fmt::Debug for Sane {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Sane")
            .field("version", &self.shared.version)
            .finish_non_exhaustive()
    }
}

/// An open device.
///
/// `Send` but not `Sync`: use it from one thread at a time. It keeps its
/// [`Sane`] alive, and closes the device (`sane_close`) when dropped or
/// [`close`](Self::close)d. If a device listing past its deadline holds the
/// library then, the close is left to the listing's thread, so dropping a
/// handle never blocks on a backend that hangs.
pub struct Handle {
    shared: Arc<Shared>,
    raw: RawHandle,
    name: DeviceName,
}

impl Handle {
    /// The name the device was opened by.
    pub fn name(&self) -> &DeviceName {
        &self.name
    }

    /// Closes the device, as dropping the handle does.
    pub fn close(self) {}
}

impl Drop for Handle {
    fn drop(&mut self) {
        self.shared.close(self.raw);
    }
}

impl fmt::Debug for Handle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Handle")
            .field("name", &self.name)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_send_sync<T: Send + Sync>() {}
    fn assert_send<T: Send>() {}

    #[test]
    fn the_library_and_handles_cross_threads() {
        assert_send_sync::<Sane>();
        assert_send::<Handle>();
    }

    #[test]
    fn default_names_are_versioned_and_by_system() {
        let expected = if cfg!(target_os = "macos") {
            Some("libsane.1.dylib")
        } else if cfg!(unix) {
            Some("libsane.so.1")
        } else {
            None
        };
        assert_eq!(Sane::DEFAULT_LIBRARY, expected);
    }
}
