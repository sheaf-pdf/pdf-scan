//! The TWAIN thread's apartment, its hidden window and its message queue.
//!
//! The window is a real top-level window, never shown, not a message-only
//! one: drivers parent their dialogs to it, and the DSM is given it. Its
//! window procedure is `DefWindowProcW` itself, so no Rust code runs inside
//! one; what the crate does with a message happens in the loop that took it.

use core::ffi::c_void;
use core::marker::PhantomData;
use core::ptr::{null, null_mut};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use windows_sys::Win32::Foundation::{
    ERROR_CLASS_ALREADY_EXISTS, GetLastError, HINSTANCE, HWND, POINT,
};
use windows_sys::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx, CoUninitialize};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CW_USEDEFAULT, CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, MSG,
    MWMO_INPUTAVAILABLE, MsgWaitForMultipleObjectsEx, PM_REMOVE, PeekMessageW, PostMessageW,
    QS_ALLINPUT, RegisterClassExW, TranslateMessage, WM_APP, WNDCLASSEXW, WS_OVERLAPPEDWINDOW,
};

use crate::error::RunError;
use crate::ffi::{TW_IDENTITY, TWRC_SUCCESS};

/// Posted by the callback: a message from the source in `wParam`, the
/// generation it was armed with in `lParam`.
pub(crate) const WM_SOURCE: u32 = WM_APP + 1;
/// Posted by another thread's [`Remote`](crate::Remote): a request is in the
/// channel.
pub(crate) const WM_REQUEST: u32 = WM_APP + 2;

/// The window the callback posts to (0: none), and the generation it posts
/// with: process-wide, as the callback has no context of its own that could
/// dangle (one `run` at a time, one source open in it).
static CALLBACK_WINDOW: AtomicUsize = AtomicUsize::new(0);
static CALLBACK_GENERATION: AtomicUsize = AtomicUsize::new(0);

/// Points the callback at `window`, its posts marked with `generation`.
pub(crate) fn arm_callback(window: usize, generation: usize) {
    CALLBACK_GENERATION.store(generation, Ordering::Release);
    CALLBACK_WINDOW.store(window, Ordering::Release);
}

/// The callback posts nowhere.
pub(crate) fn disarm_callback() {
    CALLBACK_WINDOW.store(0, Ordering::Release);
}

/// The procedure registered with `DAT_CALLBACK2`: the DSM calls it, on the
/// TWAIN thread, when a source has a message for the application. It posts
/// the message to the window and returns, touching nothing else: no lock, no
/// allocation, nothing that can panic. The loop that takes the post does the
/// rest. Its signature is `TWAINCALLBACKPROC`'s.
pub(crate) extern "system" fn source_callback(
    _origin: *mut TW_IDENTITY,
    _dest: *mut TW_IDENTITY,
    _dg: u32,
    _dat: u16,
    msg: u16,
    _data: *mut c_void,
) -> u16 {
    let window = CALLBACK_WINDOW.load(Ordering::Acquire);
    if window != 0 {
        let generation = CALLBACK_GENERATION.load(Ordering::Acquire);
        post(window, WM_SOURCE, usize::from(msg), generation as isize);
    }
    TWRC_SUCCESS
}

/// NUL-terminated UTF-16.
pub(crate) fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(Some(0)).collect()
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

/// The hidden top-level window. Not `Send`: a window belongs to the thread
/// that made it.
pub(crate) struct Window {
    hwnd: HWND,
}

const CLASS: &str = "twain-dsm hidden window";

/// The class, registered once per process and never unregistered: a window
/// left behind by a teardown that couldn't close the DSM's connection still
/// has it.
fn class() -> Result<&'static [u16], RunError> {
    static CLASS_NAME: OnceLock<Result<Vec<u16>, u32>> = OnceLock::new();
    let registered = CLASS_NAME.get_or_init(|| {
        let name = wide(CLASS);
        let wc = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            style: 0,
            lpfnWndProc: Some(DefWindowProcW),
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
        // and copied by the call.
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
        // is destroyed once, here.
        unsafe { DestroyWindow(self.hwnd) };
    }
}

/// A message taken from the calling thread's queue.
pub(crate) struct Message(MSG);

impl Message {
    pub(crate) fn hwnd(&self) -> usize {
        self.0.hwnd as usize
    }

    pub(crate) fn message(&self) -> u32 {
        self.0.message
    }

    pub(crate) fn wparam(&self) -> usize {
        self.0.wParam
    }

    pub(crate) fn lparam(&self) -> isize {
        self.0.lParam
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

/// Waits until a message is in the calling thread's queue, or `timeout`.
pub(crate) fn wait_for_message(timeout: Duration) {
    let ms = u32::try_from(timeout.as_millis()).unwrap_or(u32::MAX - 1);
    // SAFETY: no handles; the flags wait for any input, including what's
    // queued already but not yet seen.
    unsafe { MsgWaitForMultipleObjectsEx(0, null(), ms, QS_ALLINPUT, MWMO_INPUTAVAILABLE) };
}

/// Posts `message` to the window `handle`, from any thread. False if it
/// couldn't be posted.
pub(crate) fn post(handle: usize, message: u32, wparam: usize, lparam: isize) -> bool {
    // SAFETY: posting copies its arguments into the queue of the window's
    // thread and touches no memory of ours. A window that's gone makes it
    // fail; one whose handle was reused would get it, so callers post only
    // while the window lives (`Remote` holds the handle under a lock that the
    // teardown takes to clear it before the window goes).
    unsafe { PostMessageW(handle as HWND, message, wparam, lparam) != 0 }
}
