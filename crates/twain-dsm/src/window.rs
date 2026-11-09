//! The TWAIN thread's apartment, its hidden window, its message queue, the
//! callback's ring and the process's run lock.
//!
//! The window is a real top-level window, never shown, not a message-only
//! one: drivers parent their dialogs to it, and the DSM is given it. Its
//! window procedure is the crate's own and does almost nothing: it ignores
//! `WM_CLOSE`, so only the crate destroys the window, and the crate's wake-up
//! message, which carries nothing; the rest goes to `DefWindowProcW`. What
//! the crate does with a message happens in the loop that took it.
//!
//! What the source says through the callback is kept in process memory (a
//! ring of atomics), not in the message that wakes the loop, so another
//! message loop on the thread (a driver's dialog, a message box) that takes
//! the wake-up loses nothing.

use core::ffi::c_void;
use core::marker::PhantomData;
use core::ptr::{null, null_mut};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::time::Duration;

use windows_sys::Win32::Foundation::{
    CloseHandle, ERROR_CLASS_ALREADY_EXISTS, GetLastError, HANDLE, HINSTANCE, HWND, LPARAM,
    LRESULT, POINT, WAIT_OBJECT_0, WPARAM,
};
use windows_sys::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx, CoUninitialize};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Threading::{
    CreateSemaphoreW, GetCurrentProcessId, INFINITE, ReleaseSemaphore, WaitForSingleObject,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CW_USEDEFAULT, CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, MSG,
    MWMO_INPUTAVAILABLE, MsgWaitForMultipleObjectsEx, PM_REMOVE, PeekMessageW, PostMessageW,
    QS_ALLINPUT, RegisterClassExW, RegisterWindowMessageW, TranslateMessage, WM_APP, WM_CLOSE,
    WM_NULL, WNDCLASSEXW, WS_OVERLAPPEDWINDOW,
};

use crate::error::RunError;
use crate::ffi::{TW_IDENTITY, TWRC_SUCCESS};

/// NUL-terminated UTF-16.
pub(crate) fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(Some(0)).collect()
}

/// The crate's one message: a wake-up, posted to the window by the callback
/// and by a [`Remote`](crate::Remote), carrying nothing (what it woke for is
/// in the ring or the channel). Registered, so it's no other window's
/// number; `WM_APP + 2` if registering fails.
static WAKE: AtomicU32 = AtomicU32::new(WM_APP + 2);

pub(crate) fn wake_message() -> u32 {
    WAKE.load(Ordering::Relaxed)
}

/// The run lock: one `run` at a time in the process, whatever copies of the
/// crate it holds (two versions in one build, a DLL and its host), since the
/// DSM has no locks. A named semaphore of one count, named for the process:
/// a semaphore, not a mutex, since a mutex would let the thread that holds it
/// take it again.
pub(crate) struct RunLock(HANDLE);

impl RunLock {
    /// The lock, or `AlreadyRunning` if a `run` has it (or a stranded one kept
    /// it).
    pub(crate) fn take() -> Result<RunLock, RunError> {
        // SAFETY: no arguments; always succeeds.
        let pid = unsafe { GetCurrentProcessId() };
        let name = wide(&format!("Local\\twain-dsm run {pid}"));
        // SAFETY: no security attributes; an initial and largest count of one;
        // `name` is NUL-terminated UTF-16. An existing semaphore of the name is
        // opened instead.
        let handle = unsafe { CreateSemaphoreW(null(), 1, 1, name.as_ptr()) };
        if handle.is_null() {
            // SAFETY: read straight after the failed call.
            let code = unsafe { GetLastError() };
            return Err(RunError::Lock { code });
        }
        // SAFETY: `handle` is the semaphore just opened; a zero timeout takes
        // it only if it's free.
        if unsafe { WaitForSingleObject(handle, 0) } == WAIT_OBJECT_0 {
            Ok(RunLock(handle))
        } else {
            // SAFETY: opened above, closed once.
            unsafe { CloseHandle(handle) };
            Err(RunError::AlreadyRunning)
        }
    }

    /// Kept for the process's life: a stranded run's, so no run follows.
    pub(crate) fn keep(self) {
        core::mem::forget(self);
    }
}

impl Drop for RunLock {
    fn drop(&mut self) {
        // SAFETY: the semaphore this lock took, given back once and its handle
        // closed (the semaphore goes with its last handle).
        unsafe {
            ReleaseSemaphore(self.0, 1, null_mut());
            CloseHandle(self.0);
        }
    }
}

/// The callback's generation: what each message it keeps is tagged with, and
/// what the loop takes. Changed at every enable and every step down to state
/// 4, so a message from an earlier enable (or from before this `run`) is
/// never taken for one of this.
static GENERATION: AtomicU32 = AtomicU32::new(0);

/// A new generation, now the current one.
pub(crate) fn new_generation() -> u32 {
    GENERATION.fetch_add(1, Ordering::AcqRel).wrapping_add(1)
}

pub(crate) fn generation() -> u32 {
    GENERATION.load(Ordering::Acquire)
}

/// The ring the callback writes: the source's messages, each tagged with the
/// generation, a slot each, taken by the TWAIN thread's loop. 64 unread
/// messages would wrap it; a source sends a handful an enable.
const RING: usize = 64;
static RING_SLOTS: [AtomicU64; RING] = [const { AtomicU64::new(0) }; RING];
static RING_WRITTEN: AtomicUsize = AtomicUsize::new(0);
static RING_READ: AtomicUsize = AtomicUsize::new(0);
/// A slot's mark that it holds a message.
const FULL: u64 = 1 << 63;

/// The window the callback wakes (0: none).
static CALLBACK_WINDOW: AtomicUsize = AtomicUsize::new(0);

/// Points the callback's wake-up at `window`.
pub(crate) fn arm_callback(window: usize) {
    CALLBACK_WINDOW.store(window, Ordering::Release);
}

/// The callback wakes nothing.
pub(crate) fn disarm_callback() {
    CALLBACK_WINDOW.store(0, Ordering::Release);
}

/// The procedure registered with `DAT_CALLBACK2`: the DSM calls it when a
/// source has a message for the application, on whatever thread the source
/// called the DSM from. It keeps the message in the ring, wakes the window
/// and returns, touching nothing else: atomics only, no lock, no allocation,
/// nothing that can panic. Its signature is `TWAINCALLBACKPROC`'s.
pub(crate) extern "system" fn source_callback(
    _origin: *mut TW_IDENTITY,
    _dest: *mut TW_IDENTITY,
    _dg: u32,
    _dat: u16,
    msg: u16,
    _data: *mut c_void,
) -> u16 {
    let slot = RING_WRITTEN.fetch_add(1, Ordering::AcqRel) % RING;
    let entry = FULL | (u64::from(generation()) << 16) | u64::from(msg);
    RING_SLOTS[slot].store(entry, Ordering::Release);
    let window = CALLBACK_WINDOW.load(Ordering::Acquire);
    if window != 0 {
        post(window, wake_message());
    }
    TWRC_SUCCESS
}

/// The messages the callback kept since the last call, in order, with their
/// generations. On the TWAIN thread only (the one reader). A slot claimed but
/// not yet written is left for the next call.
pub(crate) fn take_source_messages(mut each: impl FnMut(u32, u16)) {
    let written = RING_WRITTEN.load(Ordering::Acquire);
    let mut read = RING_READ.load(Ordering::Relaxed);
    if written.wrapping_sub(read) > RING {
        // Wrapped: the oldest are gone.
        read = written - RING;
    }
    while read != written {
        let entry = RING_SLOTS[read % RING].swap(0, Ordering::AcqRel);
        if entry & FULL == 0 {
            break;
        }
        read = read.wrapping_add(1);
        each((entry >> 16) as u32, entry as u16);
    }
    RING_READ.store(read, Ordering::Relaxed);
}

/// The calling thread in a single-threaded apartment, until it's left. Not
/// `Send`: it belongs to the thread that entered it.
pub(crate) struct Apartment(PhantomData<*const ()>);

impl Apartment {
    pub(crate) fn enter() -> Result<Apartment, RunError> {
        // SAFETY: the reserved argument is null, as it must be; the flag asks
        // for a single-threaded apartment. S_FALSE (already one) succeeds and
        // is balanced by `leave` like S_OK.
        let hr = unsafe { CoInitializeEx(null(), COINIT_APARTMENTTHREADED as u32) };
        if hr < 0 {
            return Err(RunError::Apartment { code: hr });
        }
        Ok(Apartment(PhantomData))
    }

    pub(crate) fn leave(self) {
        // SAFETY: balances the CoInitializeEx that made `self`, on its thread
        // (`self` isn't `Send`).
        unsafe { CoUninitialize() };
    }
}

/// The hidden window's procedure: `WM_CLOSE` ignored (another process can
/// post one, and the crate alone destroys the window, at the end of `run`);
/// the crate's wake-up, dispatched by another loop on the thread, ignored
/// (what it woke for waits in the ring or the channel); the rest
/// `DefWindowProcW`'s. Nothing here can panic.
extern "system" fn window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if message == WM_CLOSE || message == wake_message() {
        return 0;
    }
    // SAFETY: the arguments are those this window procedure was called with.
    unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
}

/// The hidden top-level window. Not `Send`: a window belongs to the thread
/// that made it.
pub(crate) struct Window {
    hwnd: HWND,
}

const CLASS: &str = "twain-dsm hidden window";

/// The class, registered once per process and never unregistered: a window
/// left behind by a teardown that couldn't close the DSM's connection still
/// has it. The wake-up message is registered with it.
fn class() -> Result<&'static [u16], RunError> {
    static CLASS_NAME: OnceLock<Result<Vec<u16>, u32>> = OnceLock::new();
    let registered = CLASS_NAME.get_or_init(|| {
        let wake = wide("twain-dsm wake");
        // SAFETY: `wake` is NUL-terminated UTF-16; 0 means it couldn't be
        // registered.
        let number = unsafe { RegisterWindowMessageW(wake.as_ptr()) };
        if number != 0 {
            WAKE.store(number, Ordering::Relaxed);
        }
        let name = wide(CLASS);
        let wc = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            style: 0,
            lpfnWndProc: Some(window_proc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: module(),
            hIcon: null_mut(),
            hCursor: null_mut(),
            hbrBackground: null_mut(),
            lpszMenuName: null(),
            lpszClassName: name.as_ptr(),
            hIconSm: null_mut(),
        };
        // SAFETY: `wc` is initialized whole; the class name is NUL-terminated
        // and copied by the call; the procedure lives as long as the program.
        if unsafe { RegisterClassExW(&wc) } == 0 {
            // SAFETY: read straight after the failed call.
            let error = unsafe { GetLastError() };
            if error != ERROR_CLASS_ALREADY_EXISTS {
                return Err(error);
            }
        }
        Ok(name)
    });
    match registered {
        Ok(name) => Ok(name),
        Err(code) => Err(RunError::Window { code: *code }),
    }
}

fn module() -> HINSTANCE {
    // SAFETY: a null name asks for the executable's own module handle.
    unsafe { GetModuleHandleW(null()) }
}

impl Window {
    pub(crate) fn create() -> Result<Window, RunError> {
        let class = class()?;
        let title = wide("twain-dsm");
        // SAFETY: the class is registered with this module; both strings are
        // NUL-terminated and outlive the call; no parent, menu or creation
        // data. The style lacks WS_VISIBLE and the window is never shown.
        let hwnd = unsafe {
            CreateWindowExW(
                0,
                class.as_ptr(),
                title.as_ptr(),
                WS_OVERLAPPEDWINDOW,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                null_mut(),
                null_mut(),
                module(),
                null(),
            )
        };
        if hwnd.is_null() {
            // SAFETY: read straight after the failed call.
            let code = unsafe { GetLastError() };
            return Err(RunError::Window { code });
        }
        Ok(Window { hwnd })
    }

    pub(crate) fn hwnd(&self) -> HWND {
        self.hwnd
    }

    /// The handle as a number, for another thread to post to.
    pub(crate) fn handle(&self) -> usize {
        self.hwnd as usize
    }

    pub(crate) fn destroy(self) {
        // SAFETY: the window was made on this thread (`self` isn't `Send`) and
        // nothing else destroys it (its procedure ignores WM_CLOSE, and only
        // its own thread could call DestroyWindow); destroyed once, here.
        unsafe { DestroyWindow(self.hwnd) };
    }
}

/// A message taken from the calling thread's queue, or made up.
pub(crate) struct Message(MSG);

impl Message {
    /// A `WM_NULL` to `hwnd`, made up to offer to `MSG_PROCESSEVENT`: the DSM
    /// answers any message with one it holds for the application.
    pub(crate) fn null(hwnd: HWND) -> Message {
        Message(MSG {
            hwnd,
            message: WM_NULL,
            wParam: 0,
            lParam: 0,
            time: 0,
            pt: POINT { x: 0, y: 0 },
        })
    }

    pub(crate) fn hwnd(&self) -> usize {
        self.0.hwnd as usize
    }

    pub(crate) fn message(&self) -> u32 {
        self.0.message
    }

    pub(crate) fn wparam(&self) -> usize {
        self.0.wParam
    }

    /// The `MSG`, for `MSG_PROCESSEVENT`'s `TW_EVENT`.
    pub(crate) fn as_mut_ptr(&mut self) -> *mut c_void {
        (&raw mut self.0).cast()
    }

    /// Translated and sent to its window's procedure.
    pub(crate) fn dispatch(&self) {
        // SAFETY: the message came from PeekMessageW on this thread (`Message`
        // isn't `Send`: MSG holds a window handle).
        unsafe {
            TranslateMessage(&self.0);
            DispatchMessageW(&self.0);
        }
    }
}

/// The calling thread's next message, any window's or the thread's own,
/// without waiting.
pub(crate) fn take_message() -> Option<Message> {
    let mut msg = MSG {
        hwnd: null_mut(),
        message: 0,
        wParam: 0,
        lParam: 0,
        time: 0,
        pt: POINT { x: 0, y: 0 },
    };
    // SAFETY: `msg` is writable; a null window takes any of this thread's
    // messages; no filter.
    let got = unsafe { PeekMessageW(&mut msg, null_mut(), 0, 0, PM_REMOVE) };
    (got != 0).then_some(Message(msg))
}

/// Waits until a message is in the calling thread's queue, or `timeout`
/// (`None`: no limit).
pub(crate) fn wait_for_message(timeout: Option<Duration>) {
    let ms = timeout.map_or(INFINITE, |t| {
        u32::try_from(t.as_millis()).unwrap_or(INFINITE - 1)
    });
    // SAFETY: no handles; the flags wait for any input, including what's
    // queued already but not yet seen.
    unsafe { MsgWaitForMultipleObjectsEx(0, null(), ms, QS_ALLINPUT, MWMO_INPUTAVAILABLE) };
}

/// Posts `message` to the window `handle`, from any thread. False if it
/// couldn't be posted.
pub(crate) fn post(handle: usize, message: u32) -> bool {
    // SAFETY: posting copies its arguments into the queue of the window's
    // thread and touches no memory of ours. The handles posted to are the
    // hidden window's, which lives until the end of `run` (nothing else can
    // destroy it), and are cleared before it's destroyed: `Remote` holds its
    // handle under a lock the teardown takes; the callback's is disarmed. A
    // stranded run's window is never destroyed.
    unsafe { PostMessageW(handle as HWND, message, 0, 0) != 0 }
}
