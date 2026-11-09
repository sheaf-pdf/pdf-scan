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
//!
//! **Cancelling.** `sane_cancel` is the one call that may overlap others, so
//! it never takes the call token: a [`CancelToken`] calls it from any thread
//! while the handle's own thread is blocked in a read. It holds the handle's
//! own lock while it does, and calls only while the handle's alive flag
//! says the handle is open; a handle clears the flag under that lock before
//! its `sane_close`, so a cancel that comes late does nothing, and one in
//! flight finishes before the close begins. (The pattern of airbridge's
//! SANE backend, github.com/tobhoster/airbridge, MIT OR Apache-2.0, written
//! here in this crate's own code.) The token also sets a flag the handle's
//! scanning calls check, so a backend that ignores `sane_cancel` still
//! stops being read.

#![allow(unsafe_code)]

use std::ffi::{CString, c_char};
use std::fmt;
use std::mem;
use std::path::PathBuf;
use std::ptr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use crate::options::{
    self, Buffer, Member, RawConstraint, RawDescriptor, Request, check_count_option,
    constraint_member, option_count, prepare, read_value, still_current, word_list_length,
};
use crate::{
    Call, Device, DeviceName, Error, LimitKind, Limits, LoadError, OptionDescriptor, Parameters,
    ReadOutcome, SetInfo, Status, Value, Version, ffi,
};

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
    get_option_descriptor: ffi::SaneGetOptionDescriptorFn,
    control_option: ffi::SaneControlOptionFn,
    get_parameters: ffi::SaneGetParametersFn,
    start: ffi::SaneStartFn,
    read: ffi::SaneReadFn,
    cancel: ffi::SaneCancelFn,
    set_io_mode: ffi::SaneSetIoModeFn,
    // Looked up everywhere, so a library without it is refused when it's
    // loaded; called only where a select fd means something (Unix).
    #[cfg_attr(not(unix), allow(dead_code))]
    get_select_fd: ffi::SaneGetSelectFdFn,
}

impl Api {
    /// Opens the first of `names` that loads, never to unload it, and looks
    /// up SANE's functions in it.
    ///
    /// # Safety
    ///
    /// Opening a library runs its initialisers in this process, and the
    /// functions it exports by SANE's names are then called with the types
    /// `sane.h` gives them. Each of `names` must be one of these:
    ///
    /// - SANE's own name, by system ([`Sane::DEFAULT_LIBRARIES`]), so that
    ///   what opens is what the system's loader finds under it, in the
    ///   environment the process started with: the system's SANE, trusted
    ///   as a library linked at build time is, since a library under SANE's
    ///   name is SANE;
    /// - a library already in the process that exports none of SANE's
    ///   names, so opening it runs no initialiser and nothing of it is
    ///   called;
    /// - an absolute path where nothing is, its file name one no library
    ///   has (dyld looks for that name in its variables' directories too),
    ///   so nothing opens.
    unsafe fn load(names: &[PathBuf]) -> Result<Api, LoadError> {
        let mut last_error = None;
        for name in names {
            // SAFETY: opening a library runs its initialisers in this
            // process. `name` is one of the three this function's contract
            // allows: SANE's own name, so the system's SANE, whose
            // initialisers run here as a library linked at build time
            // would have them run; a library already in the process, whose
            // initialisers have run; or nothing at all. The library is
            // never unloaded, so no finaliser of it runs while anything it
            // set up may still be in use.
            match unsafe { libloading::Library::new(name) } {
                // SAFETY: `look_up`'s contract. The library was opened from
                // `name`, which this function's contract makes the system's
                // SANE, exporting SANE's functions with `sane.h`'s types, or
                // a library that exports none of SANE's names, of which
                // there's nothing to ask.
                Ok(library) => return unsafe { Api::look_up(Box::leak(Box::new(library))) },
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

    /// SANE's 14 functions, looked up in `library`; none is called.
    ///
    /// # Safety
    ///
    /// Each function `library` exports by one of SANE's names has the type
    /// `sane.h` gives it, as [`Api::load`]'s contract makes it.
    unsafe fn look_up(library: &'static libloading::Library) -> Result<Api, LoadError> {
        macro_rules! symbol {
            ($name:literal, $type:ty) => {{
                let symbol = concat!($name, "\0").as_bytes();
                // SAFETY: a function the library exports by one of SANE's
                // names has the type `sane.h` gives it (this function's
                // contract), and the binding is that type (the header
                // contract checks every binding against the header).
                // Nothing is called until all 14 are found.
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

/// Whether this copy of the crate has a live [`Sane`]: from before its
/// `sane_init` until after its `sane_exit` has returned.
static LOADED: AtomicBool = AtomicBool::new(false);

/// This copy's hold on [`LOADED`], freed when dropped.
struct CopySlot(());

impl CopySlot {
    fn take() -> Result<CopySlot, LoadError> {
        LOADED
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .map(|_| CopySlot(()))
            .map_err(|_| LoadError::AlreadyLoaded)
    }
}

impl Drop for CopySlot {
    fn drop(&mut self) {
        LOADED.store(false, Ordering::Release);
    }
}

/// The process's one slot, held from before the library opens until after
/// `sane_exit`, and freed when dropped: this copy's, and on Linux the
/// process's across every copy of the crate it holds (`ProcessSlot`).
/// Elsewhere a copy sees only its own.
struct Slot {
    // Fields drop in the order written: the process's slot is freed before
    // this copy's, the reverse of how they're taken.
    #[cfg(target_os = "linux")]
    _process: crate::process_slot::ProcessSlot,
    _copy: CopySlot,
}

impl Slot {
    /// Takes this copy's slot, then the process's; if the second is
    /// refused, the first is freed again as `copy` drops.
    fn take() -> Result<Slot, LoadError> {
        let copy = CopySlot::take()?;
        Ok(Slot {
            #[cfg(target_os = "linux")]
            _process: crate::process_slot::ProcessSlot::take()?,
            _copy: copy,
        })
    }
}

/// A handle `sane_open` gave, which only this module's code turns into
/// calls.
#[derive(Clone, Copy)]
struct RawHandle(ffi::SANE_Handle);

// SAFETY: a SANE handle is an opaque token, never dereferenced here. The
// standard says nothing of threads: it forbids only calls that overlap.
// Every call made with a handle but `sane_cancel` holds the call token,
// whichever thread makes it; `sane_cancel`, which the standard lets overlap
// any call, is made only while the handle's alive flag is set
// (`CancelState::call`), so never on a closed handle.
unsafe impl Send for RawHandle {}

/// What a [`CancelToken`] shares with its handle: the handle and
/// `sane_cancel`, the alive flag under the handle's own lock, and the flag
/// that a cancel was asked for.
struct CancelState {
    cancel: ffi::SaneCancelFn,
    raw: CancelHandle,
    /// Whether the handle is open: cleared, under this lock, before its
    /// `sane_close`.
    alive: Mutex<bool>,
    /// Set by a token's cancel, taken by the call that returns
    /// [`Error::Cancelled`] for it.
    requested: AtomicBool,
}

/// The handle a cancel is for.
struct CancelHandle(RawHandle);

// SAFETY: the handle is an opaque token, never dereferenced here, and the
// only call made through a shared `CancelHandle` is `sane_cancel`, which the
// standard allows from any thread at any time, overlapping any other call
// ("safe to call this function asynchronously"); `CancelState::call` makes
// it only while the handle is open.
unsafe impl Sync for CancelHandle {}

impl CancelState {
    fn new(api: &Api, raw: RawHandle) -> CancelState {
        CancelState {
            cancel: api.cancel,
            raw: CancelHandle(raw),
            alive: Mutex::new(true),
            requested: AtomicBool::new(false),
        }
    }

    /// `sane_cancel`, if the handle is still open.
    fn call(&self) {
        // Nothing panics while holding it, but a poisoned flag is still
        // the truth.
        let alive = self.alive.lock().unwrap_or_else(PoisonError::into_inner);
        if *alive {
            // SAFETY: the handle is open: `alive` is cleared, under this
            // lock, before the handle's `sane_close`, so it can't be closed
            // while this call runs. SANE is initialised, since an open
            // handle keeps its `Shared`, and so SANE, alive. `sane_cancel`
            // may overlap any other call and come from any thread (the
            // standard), so it takes no call token; the library is never
            // unloaded, so the function stays valid.
            unsafe { (self.cancel)(self.raw.0.0) };
        }
    }

    /// The handle is closing: no cancel reaches it from here on, and one in
    /// flight has returned when this does.
    fn close(&self) {
        *self.alive.lock().unwrap_or_else(PoisonError::into_inner) = false;
    }

    /// Takes the cancel asked for, if there is one.
    fn take_request(&self) -> bool {
        self.requested.swap(false, Ordering::SeqCst)
    }
}

/// Cancels a handle's scan from any thread ([`Handle::cancel_token`]).
///
/// [`cancel`](Self::cancel) calls `sane_cancel` at once, without waiting for
/// the handle's thread, which may be blocked in a read: SANE allows that
/// one call to overlap the others. A token outlives its handle harmlessly:
/// once the handle is closed, a cancel does nothing, and a cancel in flight
/// finishes before `sane_close` begins.
///
/// It also sets a flag the handle's scanning calls check, so a scan stops
/// even if the backend ignores `sane_cancel` and goes on answering: the
/// next [`Handle::start`] or [`Handle::read`] (and so the next read of a
/// [`Batch`](crate::Batch)'s page) returns [`Error::Cancelled`], and so does
/// a read in flight when it returns. The flag stays set until a call
/// returns `Cancelled` for it, so a cancel between pages stops the next
/// one, and a cancel after a batch has ended stops the next batch's first
/// page, unless [`Handle::clear_cancel`] takes it back first. Stopping
/// after the page in flight is the caller's: it starts no next page.
#[derive(Clone)]
pub struct CancelToken {
    state: Arc<CancelState>,
}

impl CancelToken {
    /// Cancels the handle's scan, now (`sane_cancel`), and marks it
    /// cancelled for the handle's next scanning call. Does nothing to a
    /// closed handle.
    pub fn cancel(&self) {
        // Set first, so a call that returns because of this cancel finds it.
        self.state.requested.store(true, Ordering::SeqCst);
        self.state.call();
    }

    /// Whether a cancel is waiting for a call to return
    /// [`Error::Cancelled`] for it.
    pub fn is_requested(&self) -> bool {
        self.state.requested.load(Ordering::SeqCst)
    }
}

impl fmt::Debug for CancelToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CancelToken")
            .field("requested", &self.is_requested())
            .finish_non_exhaustive()
    }
}

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
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum CText {
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
            return Err(Error::LimitExceeded {
                limit: LimitKind::Devices,
                value: index as u64 + 1,
                cap: limits.devices,
            });
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

/// Copies an option's descriptor: its integers as they are, its strings no
/// further than the cap, and its constraint read only as the member its
/// types allow ([`constraint_member`]), each list no further than its cap.
/// Of a group's, only the title and type are read: the standard defines no
/// other field of a group's descriptor (one edition of it counts the
/// description too, another doesn't), so its other pointers may be
/// anything.
///
/// # Safety
///
/// `descriptor` points to a valid `SANE_Option_Descriptor`, unchanged for
/// the call, as `sane_get_option_descriptor` gives one: its title null or
/// `NUL`-terminated, and unless it's a group, its name and description
/// too, and its constraint pointer, read as the member `constraint_type`
/// names, null or pointing to what that member says (a range; a word list
/// whose first word is how many follow; strings ended by a null pointer),
/// all valid for the call.
unsafe fn copy_descriptor(
    descriptor: *const ffi::SANE_Option_Descriptor,
    limits: Limits,
) -> RawDescriptor {
    // SAFETY: valid for the call (the caller's contract), and copied as
    // plain integers, pointers and a union of pointers.
    let copied = unsafe { *descriptor };
    let string = |text: *const c_char| {
        // SAFETY: each string read here is null or `NUL`-terminated and valid
        // for the call (the caller's contract): of a group's, only its title
        // is read.
        unsafe { copy_c_string(text, limits.string_bytes) }
    };
    let group = copied.r#type == ffi::SANE_TYPE_GROUP;
    let constraint = if group {
        RawConstraint::None
    } else {
        match constraint_member(copied.r#type, copied.constraint_type) {
            Err(reason) => RawConstraint::Unread(reason),
            Ok(Member::None) => RawConstraint::None,
            Ok(Member::Range) => {
                // SAFETY: `constraint_type` names the range member, which is
                // the one read.
                let range = unsafe { copied.constraint.range };
                if range.is_null() {
                    RawConstraint::Unread("its range is null".to_owned())
                } else {
                    // SAFETY: not null, so a valid range (the caller's
                    // contract).
                    RawConstraint::Range(unsafe { *range })
                }
            }
            Ok(Member::WordList) => {
                // SAFETY: `constraint_type` names the word list member, which
                // is the one read; the list is null or as the caller's
                // contract says.
                unsafe { copy_word_list(copied.constraint.word_list, limits) }
            }
            Ok(Member::StringList) => {
                // SAFETY: `constraint_type` names the string list member,
                // which is the one read; the list is null or as the caller's
                // contract says.
                unsafe { copy_string_list(copied.constraint.string_list, limits) }
            }
        }
    };
    let unless_group = |text| if group { CText::Null } else { string(text) };
    RawDescriptor {
        name: unless_group(copied.name),
        title: string(copied.title),
        desc: unless_group(copied.desc),
        value_type: copied.r#type,
        unit: copied.unit,
        size: copied.size,
        cap: copied.cap,
        constraint,
    }
}

/// Copies a word list, its length checked against the cap before any entry
/// is read.
///
/// # Safety
///
/// `list` is null, or points to a word giving how many words follow it, and
/// those words, all valid for the call.
unsafe fn copy_word_list(list: *const ffi::SANE_Word, limits: Limits) -> RawConstraint {
    if list.is_null() {
        return RawConstraint::Unread("its word list is null".to_owned());
    }
    // SAFETY: not null, so its first word is valid (the caller's contract).
    let claimed = unsafe { *list };
    let length = match word_list_length(claimed, limits) {
        Ok(length) => length,
        Err(reason) => return RawConstraint::Unread(reason),
    };
    // SAFETY: the list holds `claimed` words after the first (the caller's
    // contract), and `length` is that count, checked not negative and within
    // the cap; `list.add(1)` is aligned and not null, as `list` is.
    let words = unsafe { std::slice::from_raw_parts(list.add(1), length) };
    RawConstraint::WordList(words.to_vec())
}

/// Copies a string list, reading no more entries than the cap and no entry
/// further than the string cap.
///
/// # Safety
///
/// `list` is null, or points to an array of `NUL`-terminated strings ended
/// by a null pointer, all valid for the call.
unsafe fn copy_string_list(list: *const ffi::SANE_String_Const, limits: Limits) -> RawConstraint {
    if list.is_null() {
        return RawConstraint::Unread("its string list is null".to_owned());
    }
    let mut strings = Vec::new();
    loop {
        let index = strings.len();
        // SAFETY: the array ends with a null pointer, and the entries before
        // `index` were all found not null, so the array has at least
        // `index + 1` entries: this read is inside it.
        let entry = unsafe { *list.add(index) };
        if entry.is_null() {
            return RawConstraint::StringList(strings);
        }
        if index == limits.list_entries {
            return RawConstraint::Unread(format!(
                "its string list has more than {} entries",
                limits.list_entries
            ));
        }
        // SAFETY: a non-null entry is a `NUL`-terminated string valid for the
        // call (the caller's contract).
        match unsafe { copy_c_string(entry, limits.string_bytes) } {
            CText::Text(bytes) => strings.push(bytes),
            CText::Null | CText::TooLong(_) => {
                return RawConstraint::Unread(format!(
                    "entry {index} of its string list is longer than {} bytes",
                    limits.string_bytes
                ));
            }
        }
    }
}

/// Option `index`'s descriptor, copied and decoded; a null one is
/// [`Error::BadDescriptor`], since every option below the count has one.
///
/// # Safety
///
/// The caller holds the call token, SANE is initialised, and `raw` is a
/// handle `sane_open` gave and nothing has closed.
unsafe fn read_descriptor(
    api: &Api,
    raw: RawHandle,
    index: usize,
    limits: Limits,
) -> Result<OptionDescriptor, Error> {
    let number = option_number(index);
    // SAFETY: the caller's contract: no other SANE call runs, SANE is
    // initialised, and the handle is open.
    let descriptor = unsafe { (api.get_option_descriptor)(raw.0, number) };
    if descriptor.is_null() {
        return Err(Error::BadDescriptor {
            option: index,
            reason: if index == 0 {
                "sane_get_option_descriptor gave none for option 0, which every device has"
                    .to_owned()
            } else {
                "sane_get_option_descriptor gave none, though option 0 counts it".to_owned()
            },
        });
    }
    // SAFETY: not null, so a descriptor SANE keeps valid, at this address,
    // until the device is closed; nothing changes it while this caller holds
    // the call token.
    let copied = unsafe { copy_descriptor(descriptor, limits) };
    options::decode(index, copied, limits)
}

/// An option's number as SANE takes it. Every option looked up is below the
/// count, which came from a word, so it fits one.
fn option_number(index: usize) -> ffi::SANE_Int {
    ffi::SANE_Int::try_from(index).unwrap_or(ffi::SANE_Int::MAX)
}

/// Reads every option's descriptor, the count from option 0's value, each
/// checked before the work it sizes.
///
/// # Safety
///
/// As for [`read_descriptor`].
unsafe fn read_descriptors(
    api: &Api,
    raw: RawHandle,
    limits: Limits,
) -> Result<Vec<OptionDescriptor>, Error> {
    // SAFETY: the caller's contract.
    let zero = unsafe { read_descriptor(api, raw, 0, limits) }?;
    check_count_option(&zero)?;
    let mut count: ffi::SANE_Word = 0;
    let mut info: ffi::SANE_Int = 0;
    // SAFETY: the caller's contract, and option 0 was just checked to be an
    // INT of one word, so `count`, one word, is the whole of its value;
    // `info` is valid for SANE's write.
    let status = Status(unsafe {
        (api.control_option)(
            raw.0,
            0,
            ffi::SANE_ACTION_GET_VALUE,
            ptr::from_mut(&mut count).cast(),
            &mut info,
        )
    });
    if !status.is_good() {
        return Err(Error::Status {
            status,
            call: Call::ControlOption,
        });
    }
    let count = option_count(count, limits)?;
    let mut descriptors = Vec::with_capacity(count);
    descriptors.push(zero);
    for index in 1..count {
        // SAFETY: the caller's contract.
        descriptors.push(unsafe { read_descriptor(api, raw, index, limits) }?);
    }
    Ok(descriptors)
}

/// Reads every descriptor into `options`, or empties it if they can't be
/// read, so no call is sized from descriptors that may be stale; either
/// way `generation` counts it.
///
/// # Safety
///
/// As for [`read_descriptor`].
unsafe fn reload(
    api: &Api,
    raw: RawHandle,
    limits: Limits,
    options: &mut Vec<OptionDescriptor>,
    generation: &mut u64,
) -> Result<(), Error> {
    *generation += 1;
    // SAFETY: the caller's contract.
    match unsafe { read_descriptors(api, raw, limits) } {
        Ok(read) => {
            *options = read;
            Ok(())
        }
        Err(error) => {
            options.clear();
            Err(error)
        }
    }
}

/// The system's SANE, by system ([`Sane::DEFAULT_LIBRARIES`]).
#[cfg(target_os = "macos")]
const DEFAULT_LIBRARIES: &[&str] = &[
    "/opt/homebrew/lib/libsane.1.dylib",
    "/usr/local/lib/libsane.1.dylib",
    "/opt/local/lib/libsane.1.dylib",
];
#[cfg(all(unix, not(target_os = "macos")))]
const DEFAULT_LIBRARIES: &[&str] = &["libsane.so.1"];
#[cfg(not(unix))]
const DEFAULT_LIBRARIES: &[&str] = &[];

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
/// ended, another may load. `Sane` is `Send` and `Sync`, and clones share
/// the one instance.
///
/// That holds across copies of this crate as far as each system lets it be
/// seen. Cargo refuses two versions of the crate in one build (its
/// manifest's `links` key), but every shared library built with it holds a
/// copy of its own, so a process that loads two such libraries holds two
/// copies. On Linux every copy takes one slot for the whole process before
/// the library opens: a Unix socket bound to a name formed from the
/// process's id and pid namespace, in Linux's abstract socket namespace,
/// which the system frees when SANE has ended or the process ends. So a
/// second copy's load is `AlreadyLoaded` too, as is a load while a child
/// this process forked still holds the slot (until the child runs a program
/// or ends). Where the system refuses the socket (a sandbox that forbids
/// Unix sockets, or binding that name), the load is
/// [`LoadError::SlotRefused`], never SANE unguarded. The slot can't see
/// across a change, between two copies' loads, of the process's network
/// namespace (where the name lives) or of its view of `/proc` (which the
/// name is formed from), so a program that makes one keeps to one copy. On
/// macOS and the BSDs a copy sees only its own loads, so a program there
/// keeps to one copy of this crate in a process.
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
    /// What [`load`](Self::load) tries, in order: the system's SANE.
    ///
    /// On Linux and the other Unix-like systems, its versioned soname,
    /// `libsane.so.1`, found by the system loader's own search, as a library
    /// linked at build time would be. On macOS, where SANE isn't the
    /// system's and a bare name would be looked for in the working
    /// directory too, the paths its package managers install it at:
    /// Homebrew's on Apple silicon and on Intel, then MacPorts'. None on
    /// Windows, where SANE isn't the scanner API.
    pub const DEFAULT_LIBRARIES: &'static [&'static str] = DEFAULT_LIBRARIES;

    /// Loads the system's SANE ([`DEFAULT_LIBRARIES`](Self::DEFAULT_LIBRARIES),
    /// the first that opens) and starts it.
    ///
    /// Where it isn't installed this is [`LoadError::NotFound`], naming what
    /// was tried, and on Windows [`LoadError::NoDefault`]. A library found
    /// under SANE's name that lacks one of SANE's 14 functions is
    /// [`LoadError::MissingSymbol`], before any of them is called.
    /// `sane_init` is called with no authorization callback, so a backend
    /// that asks for a password gets none.
    ///
    /// # Which library, and why this is safe
    ///
    /// No code chooses the library, this crate's or its caller's: it's the
    /// one the system's loader finds under SANE's name, in the environment
    /// the process started with. Opening it runs its code in this process,
    /// and its functions are called as `sane.h` declares them, so a library
    /// under SANE's name is taken as SANE, as one linked at build time
    /// would be. It's never closed, so none of its code runs at an unload.
    ///
    /// To load another SANE, one a program ships or a build of one's own,
    /// give the process that loads it the loader's search path as it
    /// starts: `LD_LIBRARY_PATH` on Linux and the other Unix-like systems,
    /// `DYLD_LIBRARY_PATH` on macOS (where dyld looks for the file name of
    /// each of the installed paths there), naming a directory that holds
    /// SANE's own libraries alone, since the variable reaches every library
    /// the process loads by name. Or build a run path into the program. Set
    /// from inside the running process, the variable comes too late: the
    /// loader reads it as the process starts. Whoever starts a process
    /// chooses its libraries this way, as with any library linked at build
    /// time; the loaders ignore these variables in a set-id process, and
    /// dyld in a process it restricts, such as one under the hardened
    /// runtime.
    pub fn load() -> Result<Sane, LoadError> {
        if Self::DEFAULT_LIBRARIES.is_empty() {
            return Err(LoadError::NoDefault);
        }
        let names: Vec<PathBuf> = Self::DEFAULT_LIBRARIES.iter().map(PathBuf::from).collect();
        // SAFETY: the names are SANE's own, by system: its soname, found by
        // the system loader's search, or the paths its package managers
        // install it at, under whose file name dyld searches its own
        // variable's directories first. What the loader finds under them,
        // in the environment the process started with, is the system's
        // SANE, the first case `start`'s contract allows.
        unsafe { Sane::start(&names) }
    }

    /// Takes the process's slot, loads the first of `names` that opens
    /// ([`Api::load`]) and starts it; the slot is freed again if any of it
    /// fails.
    ///
    /// # Safety
    ///
    /// As [`Api::load`]'s: each of `names` is SANE's own name, so what opens
    /// is the system's SANE, found under it by the system loader's search; a
    /// library already in the process that exports none of SANE's names; or
    /// an absolute path where nothing is, under a file name no library has.
    unsafe fn start(names: &[PathBuf]) -> Result<Sane, LoadError> {
        let slot = Slot::take()?;
        // SAFETY: `names` are as `Api::load`'s contract asks (this
        // function's own).
        let api = unsafe { Api::load(names) }?;
        let mut code: ffi::SANE_Int = 0;
        // SAFETY: this caller holds the process's slot, so no other `Sane`
        // lives and the last one's `sane_exit` has returned: nothing else
        // calls SANE, and this `sane_init` follows no other unended one.
        // That holds across every copy of this crate in the process on
        // Linux, where the slot is the process's; elsewhere within this
        // copy, and a program holds one copy there (`Sane`'s docs).
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
    /// device with a null name, or a string longer than
    /// [`Limits::string_bytes`]; and so is one of more devices than
    /// [`Limits::devices`] ([`Error::LimitExceeded`]). A null vendor, model
    /// or type is empty.
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
    /// Its options' descriptors are read before this returns
    /// ([`Handle::options`]).
    ///
    /// A name SANE doesn't know is `sane_open`'s [`Status::INVAL`], in
    /// [`Error::Status`]; a name holding a `NUL` byte is
    /// [`Error::NulInName`], refused before SANE sees it. Descriptors that
    /// can't be read ([`Error::BadDescriptor`], or a status from reading
    /// option 0's count) close the device again and are returned.
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
        if !status.is_good() {
            return Err(Error::Status {
                status,
                call: Call::Open,
            });
        }
        if raw.is_null() {
            return Err(Error::BadHandle);
        }
        let raw = RawHandle(raw);
        // SAFETY: this caller still holds the call token; SANE is
        // initialised; `raw` was just opened, and nothing else has it.
        match unsafe { read_descriptors(&self.shared.api, raw, guard.limits) } {
            Ok(options) => Ok(Handle {
                token: Arc::new(CancelState::new(&self.shared.api, raw)),
                shared: Arc::clone(&self.shared),
                raw,
                name: DeviceName::from(name),
                options,
                options_generation: 0,
                scanning: false,
                non_blocking: false,
            }),
            Err(error) => {
                // SAFETY: this caller still holds the call token; `raw` came
                // from `sane_open` just now and no `Handle` was made of it,
                // so this is its one close, and it isn't used after.
                unsafe { (self.shared.api.close)(raw.0) };
                Err(error)
            }
        }
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
///
/// **Options.** A device's settings are its options, each described by a
/// descriptor ([`OptionDescriptor`]): its name, how its value is read, its
/// unit, size and capabilities, and the values it may take. They're read
/// when the device opens and kept ([`options`](Self::options)), and read
/// again whenever SANE says they changed: a set answering
/// `RELOAD_OPTIONS` ([`SetInfo::reload_options`]), and before any call on an
/// option whose type, size or capabilities SANE now gives differently than
/// the copy, so a buffer is always sized from the descriptor as it stands.
/// [`get`](Self::get), [`set`](Self::set) and [`set_auto`](Self::set_auto)
/// refuse, before SANE sees it, a call SANE's rules don't allow or that some
/// backends crash on: an option that doesn't exist, is unusable or inactive,
/// has no value, can't be read or set by software, or can't choose its own
/// value, and a value of another type or length, or a string too long for
/// the option's size. Whether a value fits the option's constraint is the
/// backend's to say.
///
/// **Scanning.** A page at a time, assembled, through a
/// [`batch`](Self::batch); or step by step, as SANE gives it:
/// [`start`](Self::start), [`parameters`](Self::parameters),
/// [`read`](Self::read) until `EOF`, [`start`](Self::start) again for a
/// page's next frame or the next page, and [`cancel`](Self::cancel) once
/// the batch is done, which SANE asks of every batch. After an error status
/// from a start, the parameters or a read, `sane_cancel` has been called
/// already. A [`CancelToken`] cancels from another thread.
pub struct Handle {
    shared: Arc<Shared>,
    raw: RawHandle,
    name: DeviceName,
    /// The descriptors as last read; empty if a re-read failed.
    options: Vec<OptionDescriptor>,
    /// How many times the descriptors have been read again since the
    /// device opened.
    options_generation: u64,
    /// What the cancel tokens share.
    token: Arc<CancelState>,
    /// Whether a `sane_start` has answered `GOOD` and no `sane_cancel` has
    /// followed it yet: a scan is pending, which SANE ends with
    /// `sane_cancel`.
    scanning: bool,
    /// Whether `set_io_mode(true)` has answered `GOOD` since the handle
    /// opened, or since `set_io_mode(false)` last did: a page then sets
    /// blocking mode back before it reads.
    non_blocking: bool,
}

impl Handle {
    /// The name the device was opened by.
    pub fn name(&self) -> &DeviceName {
        &self.name
    }

    /// The options' descriptors, indexed by option number (option 0 is the
    /// count of options), as last read. Empty if they couldn't be read
    /// again after SANE said they changed: then every option call answers
    /// [`Error::NoSuchOption`] until [`reload_options`](Self::reload_options)
    /// succeeds.
    pub fn options(&self) -> &[OptionDescriptor] {
        &self.options
    }

    /// How many times the descriptors have been read again since the
    /// device opened (0 until the first time), whatever the cause.
    ///
    /// Besides [`reload_options`](Self::reload_options), a call reads them
    /// again when SANE says they changed (a set answering `RELOAD_OPTIONS`,
    /// [`SetInfo::reload_options`]; a start answering `INVAL`,
    /// [`Error::StartInvalid`]), and before any option call on an option
    /// whose type, size or capabilities SANE now gives differently than the
    /// copy, which SANE doesn't say: a [`get`](Self::get) may read them all
    /// again too. A caller holding a view of [`options`](Self::options)
    /// compares this before and after a call to know whether it changed.
    pub fn options_generation(&self) -> u64 {
        self.options_generation
    }

    /// The option with this name, if any (groups' headings, which have no
    /// name, aren't looked at).
    pub fn find_option(&self, name: &str) -> Option<&OptionDescriptor> {
        self.options
            .iter()
            .find(|descriptor| !descriptor.is_group() && descriptor.name == name)
    }

    /// Reads every descriptor again.
    ///
    /// For when something the device did may have changed them; a set that
    /// changes them reads them again itself.
    pub fn reload_options(&mut self) -> Result<(), Error> {
        let guard = self.shared.acquire(None)?;
        // SAFETY: this caller holds the call token; SANE is initialised
        // (this handle keeps `Shared` alive); `raw` is open, since a handle
        // is closed only when it goes.
        unsafe {
            reload(
                &self.shared.api,
                self.raw,
                guard.limits,
                &mut self.options,
                &mut self.options_generation,
            )
        }
    }

    /// Reads an option's value.
    ///
    /// Refused before SANE sees it, unless the option exists, is usable and
    /// active, has a value, and can be read by software; a `BOOL` word that
    /// isn't 0 or 1, or a string with no `NUL` in the option's size, is
    /// [`Error::BadValue`]. Values are as SANE gives them, unchecked against
    /// the option's constraint. If SANE now describes the option otherwise
    /// than the copy, every descriptor is read again first, and the request
    /// checked again ([`options_generation`](Self::options_generation)
    /// counts it).
    pub fn get(&mut self, option: usize) -> Result<Value, Error> {
        let (buffer, _) = self.control(option, Request::Get)?;
        read_value(&self.options[option], buffer)
    }

    /// Sets an option's value: a button is pressed with [`Value::Button`].
    ///
    /// Refused before SANE sees it, unless the option exists, is usable and
    /// active, and can be set by software, and the value is of its type and
    /// length (a string with its `NUL` within its size, and no `NUL`
    /// inside). The backend may answer `INVAL` for a value outside the
    /// constraint, or choose a value near it ([`SetInfo::inexact`]); if
    /// other options changed, every descriptor is read again before this
    /// returns ([`SetInfo::reload_options`]), and if that fails, the set
    /// has still been made: [`Error::ReloadAfterSet`]. As for
    /// [`get`](Self::get), the descriptors may be read again before the
    /// call too ([`options_generation`](Self::options_generation)).
    pub fn set(&mut self, option: usize, value: &Value) -> Result<SetInfo, Error> {
        self.control(option, Request::Set(value))
            .map(|(_, info)| info)
    }

    /// Lets the backend choose an option's value itself, as it does until the
    /// option is set again.
    ///
    /// As [`set`](Self::set), and refused too unless the option can choose
    /// (`SANE_CAP_AUTOMATIC`).
    pub fn set_auto(&mut self, option: usize) -> Result<SetInfo, Error> {
        self.control(option, Request::Auto).map(|(_, info)| info)
    }

    /// One `sane_control_option`, under the call token: refused if the
    /// copy's descriptor refuses it; the descriptor read again first, and
    /// every descriptor if it changed, and the request checked again; the
    /// descriptors read again after a set that says they changed.
    fn control(&mut self, option: usize, request: Request<'_>) -> Result<(Buffer, SetInfo), Error> {
        let shared = &*self.shared;
        let guard = shared.acquire(None)?;
        let limits = guard.limits;
        let mut buffer = prepare(&self.options, option, request)?;
        // SAFETY: this caller holds the call token; SANE is initialised
        // (this handle keeps `Shared` alive); `raw` is open, since a handle
        // is closed only when it goes.
        let now = unsafe { descriptor_shape(&shared.api, self.raw, option) };
        let current = now.is_some_and(|(value_type, size, cap)| {
            still_current(&self.options[option], value_type, size, cap)
        });
        if !current {
            // SAFETY: as above.
            unsafe {
                reload(
                    &shared.api,
                    self.raw,
                    limits,
                    &mut self.options,
                    &mut self.options_generation,
                )
            }?;
            buffer = prepare(&self.options, option, request)?;
        }
        let mut info: ffi::SANE_Int = 0;
        // SAFETY: as above; `buffer` is the option's value in full, sized
        // from its descriptor as SANE gives it now, and stays put for the
        // call; `info` is valid for SANE's write.
        let status = Status(unsafe {
            (shared.api.control_option)(
                self.raw.0,
                option_number(option),
                request.action(),
                buffer.as_mut_ptr(),
                &mut info,
            )
        });
        if !status.is_good() {
            return Err(Error::Status {
                status,
                call: Call::ControlOption,
            });
        }
        if matches!(request, Request::Get) {
            return Ok((buffer, SetInfo::default()));
        }
        let info = SetInfo::from_word(info);
        if info.reload_options {
            // SAFETY: as above.
            let reloaded = unsafe {
                reload(
                    &shared.api,
                    self.raw,
                    limits,
                    &mut self.options,
                    &mut self.options_generation,
                )
            };
            // The backend applied the set: say so, with what it said.
            reloaded.map_err(|reload| Error::ReloadAfterSet {
                info,
                reload: Box::new(reload),
            })?;
        }
        drop(guard);
        Ok((buffer, info))
    }

    /// Closes the device, as dropping the handle does.
    pub fn close(self) {}

    /// The caps this handle's calls check, as its [`Sane`] has them now.
    pub(crate) fn limits(&self) -> Limits {
        self.shared.lock().limits
    }

    /// A token that cancels this handle's scan from any thread
    /// ([`CancelToken`]).
    pub fn cancel_token(&self) -> CancelToken {
        CancelToken {
            state: Arc::clone(&self.token),
        }
    }

    /// Ends what's in progress (`sane_cancel`), on this thread: a frame
    /// being read, or a batch whose last page has been read, which SANE
    /// asks a frontend to end this way even after `EOF`. A
    /// [`Batch`](crate::Batch) calls it however it ends, and the scanning
    /// calls after any error status; a scan still pending when the handle
    /// closes is ended first. It doesn't touch a [`CancelToken`]'s flag.
    pub fn cancel(&mut self) {
        self.token.call();
        self.scanning = false;
    }

    /// Ends a pending scan, if there is one.
    pub(crate) fn end_scan(&mut self) {
        if self.scanning {
            self.cancel();
        }
    }

    /// Takes back a token's cancel that no call has returned
    /// [`Error::Cancelled`] for yet, and says whether there was one.
    ///
    /// A token's cancel is kept until a call returns `Cancelled` for it, so
    /// one that comes as a batch ends (after its last page, or as the
    /// feeder empties) stops the next batch's first page. Call this before
    /// a new batch whose start a cancel then shouldn't stop; a cancel that
    /// comes after this returns is kept. `sane_cancel` was called when the
    /// token cancelled, and isn't again.
    pub fn clear_cancel(&mut self) -> bool {
        self.token.take_request()
    }

    /// Takes a token's cancel, if one is waiting, ending what's in
    /// progress: then the call returns [`Error::Cancelled`].
    fn cancelled(&mut self) -> bool {
        if self.token.take_request() {
            self.cancel();
            true
        } else {
            false
        }
    }

    /// Starts a frame (`sane_start`): the next page's first, or the next of
    /// a page with several (three-pass color).
    ///
    /// [`Error::Cancelled`] if a [`CancelToken`] has cancelled the scan, at
    /// once, without calling SANE, or once `sane_start` returns. Any status
    /// but `GOOD` is [`Error::Status`] (`NO_DOCS` for an empty feeder), and
    /// `INVAL`, which says the scan can't start with the options as they're
    /// set, [`Error::StartInvalid`], once every descriptor has been read
    /// again, as the standard asks. `sane_cancel` follows every error.
    pub fn start(&mut self) -> Result<(), Error> {
        if self.cancelled() {
            return Err(Error::Cancelled);
        }
        let shared = Arc::clone(&self.shared);
        let guard = shared.acquire(None)?;
        // SAFETY: this caller holds the call token; SANE is initialised
        // (this handle keeps `Shared` alive); `raw` is open, since a handle
        // is closed only when it goes.
        let status = Status(unsafe { (shared.api.start)(self.raw.0) });
        if status.is_good() {
            drop(guard);
            self.scanning = true;
            if self.cancelled() {
                return Err(Error::Cancelled);
            }
            return Ok(());
        }
        self.cancel();
        let reload = (status == Status::INVAL).then(|| {
            // SAFETY: as above; `sane_cancel` has answered the error status,
            // so a descriptor may be read.
            unsafe {
                reload(
                    &shared.api,
                    self.raw,
                    guard.limits,
                    &mut self.options,
                    &mut self.options_generation,
                )
            }
        });
        drop(guard);
        if self.token.take_request() {
            return Err(Error::Cancelled);
        }
        Err(match reload {
            Some(reloaded) => Error::StartInvalid {
                reload: reloaded.err().map(Box::new),
            },
            None => Error::Status {
                status,
                call: Call::Start,
            },
        })
    }

    /// The parameters of the frame being read, or before [`start`](Self::start)
    /// the backend's estimate of the next one (`sane_get_parameters`), as
    /// SANE gives them: [`Parameters::check`] says whether they can stand
    /// for a frame. Any status but `GOOD` is [`Error::Status`], after
    /// `sane_cancel`.
    pub fn parameters(&mut self) -> Result<Parameters, Error> {
        let shared = Arc::clone(&self.shared);
        let guard = shared.acquire(None)?;
        let mut raw = ffi::SANE_Parameters {
            format: ffi::SANE_FRAME_GRAY,
            last_frame: ffi::SANE_FALSE,
            bytes_per_line: 0,
            pixels_per_line: 0,
            lines: 0,
            depth: 0,
        };
        // SAFETY: as for `start`; `raw` is valid for SANE's write.
        let status = Status(unsafe { (shared.api.get_parameters)(self.raw.0, &mut raw) });
        drop(guard);
        if !status.is_good() {
            self.cancel();
            return Err(Error::Status {
                status,
                call: Call::GetParameters,
            });
        }
        Ok(Parameters::from_raw(raw))
    }

    /// Reads the frame's next bytes into `buffer` (`sane_read`): how many
    /// SANE gave, or `EOF` at the frame's end.
    ///
    /// SANE is asked for no more than `buffer` holds, nor
    /// [`Limits::read_chunk`] or `i32::MAX`; an empty `buffer` reads
    /// nothing and calls nothing. A length SANE gives below 0 or above what
    /// was asked for is [`Error::BadReadLength`], never trusted. In
    /// blocking mode (the default) a read waits for at least a byte; in
    /// non-blocking mode ([`set_io_mode`](Self::set_io_mode)) it may give
    /// none. Any other status is [`Error::Status`]; and once a
    /// [`CancelToken`] has cancelled the scan, the next read returns
    /// [`Error::Cancelled`] without calling SANE, as does a read in flight,
    /// whatever SANE answers. `sane_cancel` follows every error.
    pub fn read(&mut self, buffer: &mut [u8]) -> Result<ReadOutcome, Error> {
        if buffer.is_empty() {
            return Ok(ReadOutcome::Bytes(0));
        }
        if self.cancelled() {
            return Err(Error::Cancelled);
        }
        let shared = Arc::clone(&self.shared);
        let guard = shared.acquire(None)?;
        let asked = buffer
            .len()
            .min(guard.limits.read_chunk.max(1))
            .min(i32::MAX as usize);
        // At most `i32::MAX`, just above.
        let asked = ffi::SANE_Int::try_from(asked).unwrap_or(ffi::SANE_Int::MAX);
        let mut length: ffi::SANE_Int = 0;
        // SAFETY: as for `start`; `buffer` holds at least `asked` bytes and
        // stays put for the call; `length` is valid for SANE's write.
        let status = Status(unsafe {
            (shared.api.read)(self.raw.0, buffer.as_mut_ptr(), asked, &mut length)
        });
        drop(guard);
        if status.is_good() {
            let Some(given) = usize::try_from(length).ok().filter(|_| length <= asked) else {
                self.cancel();
                return Err(Error::BadReadLength { length, asked });
            };
            if self.cancelled() {
                return Err(Error::Cancelled);
            }
            return Ok(ReadOutcome::Bytes(given));
        }
        if status == Status::EOF {
            if self.cancelled() {
                return Err(Error::Cancelled);
            }
            return Ok(ReadOutcome::Eof);
        }
        // A backend need not set `length` with an error, and the `test`
        // backend doesn't: it isn't read.
        self.cancel();
        if self.token.take_request() {
            return Err(Error::Cancelled);
        }
        Err(Error::Status {
            status,
            call: Call::Read,
        })
    }

    /// Reads blocking or not (`sane_set_io_mode`), once a frame has
    /// started. A device opens reading blocking.
    ///
    /// A backend that can't read without blocking answers `UNSUPPORTED`, in
    /// [`Error::Status`]. Some answer `UNSUPPORTED` whichever mode is asked
    /// for, blocking too, though the standard says every backend supports
    /// blocking (SANE 1.4.0's `fujitsu`, `canon_dr`, `epson2` and `escl`
    /// among them): they read blocking all the same.
    pub fn set_io_mode(&mut self, non_blocking: bool) -> Result<(), Error> {
        let shared = Arc::clone(&self.shared);
        let guard = shared.acquire(None)?;
        // SAFETY: as for `start`.
        let status = Status(unsafe {
            (shared.api.set_io_mode)(self.raw.0, ffi::SANE_Bool::from(non_blocking))
        });
        drop(guard);
        if !status.is_good() {
            return Err(Error::Status {
                status,
                call: Call::SetIoMode,
            });
        }
        self.non_blocking = non_blocking;
        Ok(())
    }

    /// Whether [`set_io_mode`](Self::set_io_mode) has put the handle in
    /// non-blocking mode, and nothing has set it back.
    pub(crate) fn is_non_blocking(&self) -> bool {
        self.non_blocking
    }

    /// A file descriptor that's readable when a read would give bytes
    /// (`sane_get_select_fd`), once a frame has started: for `poll` or
    /// `select`, and nothing else.
    ///
    /// The descriptor is the backend's, valid only until the next
    /// [`start`](Self::start) or [`cancel`](Self::cancel), or the frame's
    /// `EOF`: never close it, read it, or keep it past then. A backend
    /// without one answers `UNSUPPORTED`, in [`Error::Status`]; a negative
    /// descriptor is [`Error::BadSelectFd`].
    #[cfg(unix)]
    pub fn select_fd(&mut self) -> Result<std::os::fd::RawFd, Error> {
        let shared = Arc::clone(&self.shared);
        let guard = shared.acquire(None)?;
        let mut fd: ffi::SANE_Int = -1;
        // SAFETY: as for `start`; `fd` is valid for SANE's write.
        let status = Status(unsafe { (shared.api.get_select_fd)(self.raw.0, &mut fd) });
        drop(guard);
        if !status.is_good() {
            return Err(Error::Status {
                status,
                call: Call::GetSelectFd,
            });
        }
        if fd < 0 {
            return Err(Error::BadSelectFd { fd });
        }
        Ok(fd)
    }
}

/// The type, size and capabilities option `index`'s descriptor has now, or
/// `None` if SANE gives none.
///
/// # Safety
///
/// As for [`read_descriptor`].
unsafe fn descriptor_shape(api: &Api, raw: RawHandle, index: usize) -> Option<(i32, i32, i32)> {
    // SAFETY: the caller's contract.
    let descriptor = unsafe { (api.get_option_descriptor)(raw.0, option_number(index)) };
    if descriptor.is_null() {
        return None;
    }
    // SAFETY: not null, so a descriptor SANE keeps valid until the device
    // is closed; only its integers are read.
    let copied = unsafe { *descriptor };
    Some((copied.r#type, copied.size, copied.cap))
}

impl Drop for Handle {
    fn drop(&mut self) {
        // A scan pending is ended first, as SANE asks of every batch; then
        // no cancel can reach the handle; then it's closed.
        self.end_scan();
        self.token.close();
        self.shared.close(self.raw);
    }
}

impl fmt::Debug for Handle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Handle")
            .field("name", &self.name)
            .field("options", &self.options.len())
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
        assert_send_sync::<CancelToken>();
    }

    #[test]
    fn default_names_are_versioned_and_by_system() {
        let expected: &[&str] = if cfg!(target_os = "macos") {
            &[
                "/opt/homebrew/lib/libsane.1.dylib",
                "/usr/local/lib/libsane.1.dylib",
                "/opt/local/lib/libsane.1.dylib",
            ]
        } else if cfg!(unix) {
            &["libsane.so.1"]
        } else {
            &[]
        };
        assert_eq!(Sane::DEFAULT_LIBRARIES, expected);
        // On macOS no bare name, which dyld would look for in the working
        // directory.
        if cfg!(target_os = "macos") {
            assert!(expected.iter().all(|name| name.starts_with('/')));
        }
    }

    /// Held by each test that loads a library in this process, so they
    /// never race for its slot.
    static LOADING: Mutex<()> = Mutex::new(());

    fn loading() -> MutexGuard<'static, ()> {
        LOADING.lock().unwrap_or_else(PoisonError::into_inner)
    }

    #[test]
    fn a_missing_library_is_not_found_with_what_was_tried() {
        let _loading = loading();
        // A fresh directory holds nothing, and an absolute path is opened
        // as it is, never searched for (but by dyld, under its file name,
        // which no library has).
        let dir = tempfile::tempdir().expect("couldn't make a temporary directory");
        let missing = dir.path().join("no-such-libsane");
        assert!(missing.is_absolute(), "{}", missing.display());
        let names = [missing];
        // SAFETY: `missing` is absolute, nothing is at it (a fresh
        // directory), and no library has its file name, a case `start`'s
        // contract allows: nothing opens, so nothing runs.
        match unsafe { Sane::start(&names) } {
            Err(LoadError::NotFound { tried, .. }) => assert_eq!(tried, names),
            other => panic!("expected NotFound, got {other:?}"),
        }
        // The slot (this copy's, and on Linux the process's) was freed: a
        // second try gets as far as the loader again, rather than
        // `AlreadyLoaded`.
        // SAFETY: as above.
        let again = unsafe { Sane::start(&names) };
        assert!(
            matches!(again, Err(LoadError::NotFound { .. })),
            "expected NotFound again, got {again:?}"
        );
    }

    /// A library of the system's that every process already has loaded, by
    /// its name on each system (the C library; on Windows, kernel32), and
    /// which exports none of SANE's functions.
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    const C_LIBRARY: &str = "libc.so.6";
    #[cfg(target_os = "macos")]
    const C_LIBRARY: &str = "/usr/lib/libSystem.B.dylib";
    #[cfg(windows)]
    const C_LIBRARY: &str = "kernel32.dll";

    #[cfg(any(
        all(target_os = "linux", target_env = "gnu"),
        target_os = "macos",
        windows
    ))]
    #[test]
    fn a_library_without_sanes_functions_is_refused() {
        let _loading = loading();
        let names = [PathBuf::from(C_LIBRARY)];
        // SAFETY: `C_LIBRARY` is already loaded into this process, as into
        // every one, so opening it again runs no initialiser; it exports
        // none of SANE's names, so the look-up fails at the first and
        // nothing of it is called: a case `start`'s contract allows.
        match unsafe { Sane::start(&names) } {
            Err(LoadError::MissingSymbol { name, .. }) => assert_eq!(name, "sane_init"),
            other => panic!("expected MissingSymbol, got {other:?}"),
        }
        // The slot (this copy's, and on Linux the process's) was freed: a
        // second try is refused for the library again, rather than
        // `AlreadyLoaded`.
        // SAFETY: as above.
        let again = unsafe { Sane::start(&names) };
        assert!(
            matches!(again, Err(LoadError::MissingSymbol { .. })),
            "expected MissingSymbol again, got {again:?}"
        );
    }
}
