//! The TWAIN thread's apartment and its hidden window.
//!
//! The window is a real top-level window, never shown, not a message-only
//! one: drivers parent their dialogs to it, and the DSM posts to it. Its
//! window procedure is `DefWindowProcW` itself, so no Rust code runs inside
//! one; everything the spike does with a message happens in the loop that
//! retrieved it.

use core::marker::PhantomData;
use core::ptr::{null, null_mut};

use windows_sys::Win32::Foundation::{ERROR_CLASS_ALREADY_EXISTS, GetLastError, HINSTANCE, HWND};
use windows_sys::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx, CoUninitialize};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Threading::GetCurrentThreadId;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CW_USEDEFAULT, CreateWindowExW, DefWindowProcW, DestroyWindow, RegisterClassExW,
    UnregisterClassW, WNDCLASSEXW, WS_OVERLAPPEDWINDOW,
};

use crate::dsm::wide;

/// The calling thread made a single-threaded apartment, until dropped. Not
/// `Send`: it belongs to the thread that entered it.
pub struct Apartment(PhantomData<*const ()>);

impl Apartment {
    pub fn enter() -> Result<Apartment, String> {
        // SAFETY: the reserved argument is null, as it must be; the flag asks
        // for a single-threaded apartment.
        let hr = unsafe { CoInitializeEx(null(), COINIT_APARTMENTTHREADED as u32) };
        if hr < 0 {
            return Err(format!("CoInitializeEx (apartment-threaded): {hr:#010x}"));
        }
        Ok(Apartment(PhantomData))
    }
}

impl Drop for Apartment {
    fn drop(&mut self) {
        // SAFETY: balances the successful CoInitializeEx on this thread.
        unsafe { CoUninitialize() };
    }
}

/// The hidden top-level window. Not `Send`: a window belongs to the thread
/// that created it.
pub struct Window {
    hwnd: HWND,
    hinstance: HINSTANCE,
    class: Vec<u16>,
    _thread: PhantomData<*const ()>,
}

const CLASS: &str = "twain-spike";

impl Window {
    pub fn create() -> Result<Window, String> {
        // SAFETY: a null name asks for the executable's own module handle.
        let hinstance = unsafe { GetModuleHandleW(null()) };
        let class = wide(CLASS);
        let wc = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            style: 0,
            lpfnWndProc: Some(DefWindowProcW),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: hinstance,
            hIcon: null_mut(),
            hCursor: null_mut(),
            hbrBackground: null_mut(),
            lpszMenuName: null(),
            lpszClassName: class.as_ptr(),
            hIconSm: null_mut(),
        };
        // SAFETY: `wc` is fully initialized and its class name lives in `class`,
        // which the window keeps until the class is unregistered.
        if unsafe { RegisterClassExW(&wc) } == 0 {
            // SAFETY: read straight after the failed call.
            let error = unsafe { GetLastError() };
            if error != ERROR_CLASS_ALREADY_EXISTS {
                return Err(format!("RegisterClassExW: error {error}"));
            }
        }
        let title = wide("twain-spike");
        // SAFETY: the class is registered with this module; both strings are
        // NUL-terminated and outlive the call; no parent, menu or creation data.
        // The style has no WS_VISIBLE and the window is never shown.
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
                hinstance,
                null(),
            )
        };
        if hwnd.is_null() {
            // SAFETY: read straight after the failed call.
            let error = unsafe { GetLastError() };
            return Err(format!("CreateWindowExW: error {error}"));
        }
        Ok(Window {
            hwnd,
            hinstance,
            class,
            _thread: PhantomData,
        })
    }

    pub fn hwnd(&self) -> HWND {
        self.hwnd
    }
}

impl Drop for Window {
    fn drop(&mut self) {
        // SAFETY: the window was created on this thread and is destroyed once;
        // the class is unregistered after (it fails harmlessly if another
        // window of the class still exists).
        unsafe {
            DestroyWindow(self.hwnd);
            UnregisterClassW(self.class.as_ptr(), self.hinstance);
        }
    }
}

/// The calling thread's id.
pub fn thread_id() -> u32 {
    // SAFETY: no arguments; always succeeds.
    unsafe { GetCurrentThreadId() }
}
