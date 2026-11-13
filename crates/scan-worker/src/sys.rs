//! Operating-system boundaries for [`serve`](crate::serve) and [`Client`](crate::Client):
//! isolating the worker's protocol output from driver `stdout` writes, containing Windows workers
//! and any helpers they spawn in a Job Object, and guarding caller-supplied Win32 owner windows.

use std::fs::File;
use std::io;

#[cfg(windows)]
pub(crate) use self::windows::{Job, WindowGuard};

/// Duplicates the process's initial standard output to a private, non-inheritable file handle or
/// descriptor for [`serve`](crate::serve), and redirects standard output (`fd 1`, plus
/// `STD_OUTPUT_HANDLE` on Windows) to `/dev/null` or `NUL`.
///
/// Called at the start of [`serve`](crate::serve), before any scanner backend loads a driver
/// library: C/C++ scanner drivers that chatter on `stdout` (`printf`, `puts`, `WriteFile` to
/// `GetStdHandle(STD_OUTPUT_HANDLE)`, or child helpers inheriting standard output) write harmlessly
/// to `/dev/null` / `NUL` instead of corrupting the JSON-lines protocol stream.
#[cfg(unix)]
pub(crate) fn isolate_stdout() -> io::Result<File> {
    use std::fs::OpenOptions;
    use std::os::fd::{AsRawFd, BorrowedFd};

    // SAFETY: file descriptor 1 is standard output for the duration of this borrow at the start of
    // `serve`, before worker threads or driver libraries run. `try_clone_to_owned` duplicates it
    // with `F_DUPFD_CLOEXEC`, so child processes started by drivers do not inherit the pipe.
    let protocol_out = unsafe { BorrowedFd::borrow_raw(1) }.try_clone_to_owned()?;
    let devnull = OpenOptions::new().write(true).open("/dev/null")?;
    if devnull.as_raw_fd() != 1 {
        // SAFETY: `devnull` is a live open descriptor and 1 is standard output's slot; `dup2`
        // atomically replaces fd 1 with `/dev/null` without `FD_CLOEXEC`, and `devnull` closes its
        // temporary descriptor on drop.
        if unsafe { libc::dup2(devnull.as_raw_fd(), 1) } < 0 {
            return Err(io::Error::last_os_error());
        }
    } else {
        // SAFETY: `devnull` landed directly on fd 1; clear `FD_CLOEXEC` and forget `devnull` so fd
        // 1 remains open on `/dev/null`.
        unsafe {
            let flags = libc::fcntl(1, libc::F_GETFD);
            if flags >= 0 {
                let _ = libc::fcntl(1, libc::F_SETFD, flags & !libc::FD_CLOEXEC);
            }
        }
        core::mem::forget(devnull);
    }
    Ok(File::from(protocol_out))
}

/// Windows implementation of `isolate_stdout`, `Job`, and `WindowGuard`.
#[cfg(windows)]
pub(crate) fn isolate_stdout() -> io::Result<File> {
    windows::isolate_stdout()
}

#[cfg(windows)]
mod windows {
    use core::ffi::c_int;
    use core::ptr::{null, null_mut};
    use std::fs::File;
    use std::io;
    use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
    use std::process::Child;

    use scan_device::{ScanError, WindowOwner};
    use windows_sys::Win32::Foundation::{
        CloseHandle, DUPLICATE_SAME_ACCESS, DuplicateHandle, ERROR_ACCESS_DENIED, GENERIC_WRITE,
        HANDLE, HWND, INVALID_HANDLE_VALUE,
    };
    use windows_sys::Win32::Storage::FileSystem::{
        CreateFileW, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
    };
    use windows_sys::Win32::System::Console::{GetStdHandle, STD_OUTPUT_HANDLE, SetStdHandle};
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
        SetInformationJobObject, TerminateJobObject,
    };
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, GetCurrentThreadId};
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{EnableWindow, IsWindowEnabled};
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetWindowThreadProcessId, IsWindow};

    unsafe extern "C" {
        fn _open_osfhandle(osfhandle: isize, flags: c_int) -> c_int;
        fn _get_osfhandle(fd: c_int) -> isize;
        fn _dup2(fd1: c_int, fd2: c_int) -> c_int;
        fn _close(fd: c_int) -> c_int;
    }

    pub(super) fn isolate_stdout() -> io::Result<File> {
        // SAFETY: querying the process's current `STD_OUTPUT_HANDLE` takes no pointers.
        let orig = unsafe { GetStdHandle(STD_OUTPUT_HANDLE) };
        if orig == INVALID_HANDLE_VALUE {
            return Err(io::Error::last_os_error());
        }
        if orig.is_null() {
            return Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "the worker has no standard output handle",
            ));
        }

        let mut dup: HANDLE = null_mut();
        // SAFETY: `GetCurrentProcess` returns the pseudo-handle for this process; `orig` was
        // checked above; `bInheritHandle` is 0 (`FALSE`) so child processes spawned by drivers do
        // not inherit the protocol pipe.
        let duplicated = unsafe {
            let me = GetCurrentProcess();
            DuplicateHandle(me, orig, me, &mut dup, 0, 0, DUPLICATE_SAME_ACCESS)
        };
        if duplicated == 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: `dup` is a freshly duplicated handle owned solely by `protocol_out`, which closes
        // it on drop if any step below fails.
        let protocol_out = unsafe { OwnedHandle::from_raw_handle(dup.cast()) };

        let nul_name: [u16; 4] = [u16::from(b'N'), u16::from(b'U'), u16::from(b'L'), 0];
        // SAFETY: `nul_name` is NUL-terminated UTF-16 naming the Win32 `NUL` device; null security
        // attributes and template handle are valid.
        let nul = unsafe {
            CreateFileW(
                nul_name.as_ptr(),
                GENERIC_WRITE,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                null(),
                OPEN_EXISTING,
                0,
                null_mut(),
            )
        };
        if nul == INVALID_HANDLE_VALUE || nul.is_null() {
            return Err(io::Error::last_os_error());
        }

        // SAFETY: `nul` is a freshly opened Win32 handle; on success `_open_osfhandle` transfers
        // ownership of `nul` to the C runtime descriptor `nul_fd`.
        let nul_fd = unsafe { _open_osfhandle(nul as isize, 0) };
        if nul_fd < 0 {
            let err = io::Error::last_os_error();
            // SAFETY: `_open_osfhandle` failed, so `nul` remains ours to close once.
            unsafe {
                CloseHandle(nul);
            }
            return Err(err);
        }

        if nul_fd != 1 {
            // SAFETY: `nul_fd` is an open C runtime descriptor and 1 is standard output's slot;
            // `_dup2` closes any descriptor previously on 1 (including the inherited pipe handle)
            // and duplicates `nul_fd`'s handle into slot 1.
            let rc = unsafe { _dup2(nul_fd, 1) };
            let err = (rc < 0).then(io::Error::last_os_error);
            // SAFETY: `nul_fd` is still open and closing it releases its original handle while
            // leaving slot 1's duplicated handle live.
            unsafe {
                _close(nul_fd);
            }
            if let Some(err) = err {
                return Err(err);
            }
        }

        // SAFETY: C runtime slot 1 is guaranteed open here (either `nul_fd == 1` or `_dup2` into 1
        // succeeded above). Reading `_get_osfhandle(1)` after `_close(nul_fd)` yields the live
        // handle owned by slot 1, never the closed `nul` handle.
        let fd1_handle = unsafe { _get_osfhandle(1) };
        if fd1_handle == -1 || fd1_handle == 0 {
            return Err(io::Error::other("CRT fd 1 has no OS handle after _dup2"));
        }
        // SAFETY: `fd1_handle` is the live `NUL` handle owned by CRT fd 1.
        if unsafe { SetStdHandle(STD_OUTPUT_HANDLE, fd1_handle as HANDLE) } == 0 {
            return Err(io::Error::last_os_error());
        }

        Ok(File::from(protocol_out))
    }

    /// A Windows Job Object configured with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` so a worker and
    /// any helper processes its driver spawns end when terminated or when the client drops the
    /// last handle.
    pub(crate) struct Job(OwnedHandle);

    impl Job {
        /// Creates an unnamed job object that kills all processes in it when closed.
        pub(crate) fn new() -> io::Result<Job> {
            // SAFETY: null security attributes and null name create a new unnamed job object.
            let raw = unsafe { CreateJobObjectW(null(), null()) };
            if raw.is_null() {
                return Err(io::Error::last_os_error());
            }
            // SAFETY: `raw` is the newly created job handle; `OwnedHandle` closes it on drop.
            let handle = unsafe { OwnedHandle::from_raw_handle(raw.cast()) };

            // SAFETY: `JOBOBJECT_EXTENDED_LIMIT_INFORMATION` is a plain C struct for which all-zero
            // bytes are valid.
            let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { core::mem::zeroed() };
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            // SAFETY: `handle` is our job object and `info` matches
            // `JobObjectExtendedLimitInformation`.
            let ok = unsafe {
                SetInformationJobObject(
                    handle.as_raw_handle() as HANDLE,
                    JobObjectExtendedLimitInformation,
                    (&raw const info).cast(),
                    core::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                )
            };
            if ok == 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(Job(handle))
        }

        /// Places `child` into this job, tolerating the Windows race where `child` has already
        /// exited (`ERROR_ACCESS_DENIED` with `try_wait() == Ok(Some(_))`).
        pub(crate) fn assign(&self, child: &mut Child) -> io::Result<()> {
            let job = self.0.as_raw_handle() as HANDLE;
            let proc_handle = child.as_raw_handle() as HANDLE;
            // SAFETY: `job` is our job object and `proc_handle` is `child`'s live process handle.
            if unsafe { AssignProcessToJobObject(job, proc_handle) } != 0 {
                return Ok(());
            }
            let err = io::Error::last_os_error();
            if err.raw_os_error() == Some(ERROR_ACCESS_DENIED as i32)
                && matches!(child.try_wait(), Ok(Some(_)))
            {
                return Ok(());
            }
            Err(err)
        }

        /// Terminates every process remaining in the job at once.
        pub(crate) fn terminate(&self) {
            let job = self.0.as_raw_handle() as HANDLE;
            // SAFETY: `job` is our job object; exit status 1 terminates any processes still inside.
            unsafe {
                let _ = TerminateJobObject(job, 1);
            }
        }
    }

    /// Client-side guard around a `WindowOwner` passed to `RemoteDevice::settings_window`:
    /// refuses a live `HWND` owned by the calling thread before blocking on the worker pipe, and
    /// restores `EnableWindow(hwnd, 1)` on drop if the window started enabled and was left
    /// disabled when the call ended (for instance because the worker crashed or was stopped).
    pub(crate) struct WindowGuard {
        target: Option<(usize, bool)>,
    }

    impl WindowGuard {
        /// Validates `owner` on the calling client thread.
        ///
        /// Non-window integers (such as test stand-ins on `FakeBackend` or stale handles) pass
        /// through with `target: None` so the worker's backend decides how to treat them.
        pub(crate) fn check(owner: Option<WindowOwner>) -> Result<WindowGuard, ScanError> {
            let Some(WindowOwner(raw)) = owner else {
                return Ok(WindowGuard { target: None });
            };
            let Some(hwnd) = resolve_hwnd(raw) else {
                return Ok(WindowGuard { target: None });
            };
            // SAFETY: `hwnd` was verified with `IsWindow`; a null process-id pointer queries only
            // the owning thread id, and `GetCurrentThreadId` has no preconditions.
            let same_thread =
                unsafe { GetWindowThreadProcessId(hwnd, null_mut()) == GetCurrentThreadId() };
            if same_thread {
                return Err(ScanError::Driver {
                    message: format!(
                        "parent window {raw:#x} belongs to the calling thread and would deadlock \
                         while waiting on the worker"
                    ),
                    shown: false,
                });
            }
            // SAFETY: `hwnd` was verified with `IsWindow`; `IsWindowEnabled` touches no memory.
            let was_enabled = unsafe { IsWindowEnabled(hwnd) } != 0;
            Ok(WindowGuard {
                target: Some((hwnd as usize, was_enabled)),
            })
        }
    }

    impl Drop for WindowGuard {
        fn drop(&mut self) {
            let Some((raw_hwnd, true)) = self.target.take() else {
                return;
            };
            let hwnd = raw_hwnd as HWND;
            // SAFETY: `IsWindow`, `IsWindowEnabled`, and `EnableWindow` look up `hwnd` in User32's
            // handle table without dereferencing it; if the window was destroyed meanwhile,
            // `IsWindow` returns 0 and nothing is sent.
            unsafe {
                if IsWindow(hwnd) != 0 && IsWindowEnabled(hwnd) == 0 {
                    EnableWindow(hwnd, 1);
                }
            }
        }
    }

    /// Resolves `raw` as a canonical 32-bit Win32 `HWND` recognized by `IsWindow`.
    fn resolve_hwnd(raw: u64) -> Option<HWND> {
        let canonical = (1..u64::from(u32::MAX)).contains(&raw)
            || (0xFFFF_FFFF_8000_0000..u64::MAX).contains(&raw);
        if !canonical {
            return None;
        }
        let zero_ext = (raw as u32 as usize) as HWND;
        let sign_ext = (raw as u32 as i32 as isize) as HWND;
        // SAFETY: `IsWindow` checks any integer in User32's handle table and touches no memory.
        if unsafe { IsWindow(zero_ext) } != 0 {
            return Some(zero_ext);
        }
        // SAFETY: Same User32 handle-table lookup for the `LongToHandle` sign-extended form.
        if unsafe { IsWindow(sign_ext) } != 0 {
            Some(sign_ext)
        } else {
            None
        }
    }

    #[cfg(test)]
    mod tests {
        use std::sync::mpsc;
        use std::thread;

        use windows_sys::Win32::UI::WindowsAndMessaging::{
            CW_USEDEFAULT, CreateWindowExW, DestroyWindow, MSG, PM_REMOVE, PeekMessageW,
            PostThreadMessageW, WM_QUIT, WS_OVERLAPPEDWINDOW,
        };

        use super::*;

        fn static_class() -> Vec<u16> {
            "STATIC\0".encode_utf16().collect()
        }

        #[test]
        fn window_guard_passes_non_windows_and_refuses_same_thread_hwnd() {
            for bogus in [0, 0xFFFF, u64::from(u32::MAX), u64::MAX, 1u64 << 40] {
                assert!(WindowGuard::check(Some(WindowOwner(bogus))).is_ok());
            }

            let class = static_class();
            // SAFETY: `"STATIC"` is a system-registered class; creates a hidden top-level window on
            // this thread.
            let hwnd = unsafe {
                CreateWindowExW(
                    0,
                    class.as_ptr(),
                    class.as_ptr(),
                    WS_OVERLAPPEDWINDOW,
                    CW_USEDEFAULT,
                    CW_USEDEFAULT,
                    100,
                    100,
                    null_mut(),
                    null_mut(),
                    null_mut(),
                    null(),
                )
            };
            assert!(!hwnd.is_null());
            let raw = hwnd as usize as u64;
            match WindowGuard::check(Some(WindowOwner(raw))) {
                Err(ScanError::Driver { shown: false, .. }) => {}
                Ok(_) => panic!("same-thread HWND {raw:#x} should be refused"),
                Err(other) => panic!("unexpected {other:?}"),
            }
            // SAFETY: `hwnd` was created on this thread and is destroyed once here.
            unsafe {
                DestroyWindow(hwnd);
            }
        }

        #[test]
        fn window_guard_restores_only_cross_thread_windows_that_started_enabled() {
            let (ready_tx, ready_rx) = mpsc::channel();
            let ui = thread::spawn(move || {
                // SAFETY: `GetCurrentThreadId` has no preconditions.
                let tid = unsafe { GetCurrentThreadId() };
                let class = static_class();
                // SAFETY: `"STATIC"` is a system-registered class; creates a hidden top-level
                // window on this UI thread, which pumps messages until `WM_QUIT`.
                let hwnd = unsafe {
                    CreateWindowExW(
                        0,
                        class.as_ptr(),
                        class.as_ptr(),
                        WS_OVERLAPPEDWINDOW,
                        CW_USEDEFAULT,
                        CW_USEDEFAULT,
                        100,
                        100,
                        null_mut(),
                        null_mut(),
                        null_mut(),
                        null(),
                    )
                };
                assert!(!hwnd.is_null());
                ready_tx.send((tid, hwnd as usize)).unwrap();
                // SAFETY: `msg` is zeroed and pumped on the owning thread until `WM_QUIT`.
                unsafe {
                    let mut msg: MSG = core::mem::zeroed();
                    loop {
                        while PeekMessageW(&mut msg, null_mut(), 0, 0, PM_REMOVE) != 0 {
                            if msg.message == WM_QUIT {
                                DestroyWindow(hwnd);
                                return;
                            }
                        }
                        thread::sleep(std::time::Duration::from_millis(5));
                    }
                }
            });

            let (ui_tid, raw_usize) = ready_rx.recv().unwrap();
            let hwnd = raw_usize as HWND;
            let owner = Some(WindowOwner(raw_usize as u64));

            // Started enabled -> disabled mid-call -> restored on guard drop.
            {
                let _guard = WindowGuard::check(owner).expect("cross-thread window accepted");
                // SAFETY: `hwnd` is live on `ui` which is pumping messages.
                unsafe {
                    EnableWindow(hwnd, 0);
                    assert_eq!(IsWindowEnabled(hwnd), 0);
                }
            }
            // SAFETY: `hwnd` is live on `ui`.
            assert_ne!(unsafe { IsWindowEnabled(hwnd) }, 0);

            // Started disabled -> stays disabled on guard drop.
            // SAFETY: `hwnd` is live on `ui`.
            unsafe {
                EnableWindow(hwnd, 0);
            }
            {
                let _guard =
                    WindowGuard::check(owner).expect("cross-thread disabled window accepted");
            }
            // SAFETY: `hwnd` is live on `ui`.
            assert_eq!(unsafe { IsWindowEnabled(hwnd) }, 0);

            // SAFETY: posts `WM_QUIT` to `ui_tid` so the UI thread destroys its window and exits.
            unsafe {
                PostThreadMessageW(ui_tid, WM_QUIT, 0, 0);
            }
            ui.join().unwrap();
        }
    }
}
