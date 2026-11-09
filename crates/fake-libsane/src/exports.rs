//! SANE's 14 functions, exported.
//!
//! The one module that holds unsafe code: each export reads its arguments
//! from C, asks the [`Fake`](crate::fake::Fake) what to do, and writes the
//! answer back. Each is asserted at compile time to have its binding's type.

#![allow(unsafe_code)]

use std::ffi::{c_char, c_void};
use std::path::PathBuf;
use std::ptr;
use std::sync::{Mutex, MutexGuard, PoisonError};

use crate::fake::{Fake, Log};
use crate::ffi;
use crate::script::{Script, Text};

/// The fake, made at the first call from the script and log the
/// environment names.
static FAKE: Mutex<Option<Fake>> = Mutex::new(None);

fn fake() -> MutexGuard<'static, Option<Fake>> {
    FAKE.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Runs `f` on the fake, making it first if this is the first call.
fn with_fake<T>(f: impl FnOnce(&mut Fake) -> T) -> T {
    let mut guard = fake();
    let fake = guard.get_or_insert_with(|| {
        let log = Log::open(
            std::env::var_os("FAKE_LIBSANE_LOG")
                .map(PathBuf::from)
                .as_deref(),
        );
        let (script, problem) = match std::env::var_os("FAKE_LIBSANE_SCRIPT") {
            None => (Script::default(), None),
            Some(path) => match std::fs::read_to_string(&path) {
                Err(error) => (
                    Script::default(),
                    Some(format!("couldn't read the script: {error}")),
                ),
                Ok(text) => match Script::parse(&text) {
                    Ok(script) => (script, None),
                    Err(error) => (
                        Script::default(),
                        Some(format!("the script doesn't parse: {error}")),
                    ),
                },
            },
        };
        let mut fake = Fake::new(script, log);
        if let Some(problem) = problem {
            fake.note(&thread(), &format!("script-error {problem}"));
            fake.script_failed = true;
        }
        fake
    });
    f(fake)
}

/// The calling thread, as the log names it: `t` and the number Rust gives
/// it.
fn thread() -> String {
    let id = format!("{:?}", std::thread::current().id());
    let digits: String = id.chars().filter(char::is_ascii_digit).collect();
    format!("t{digits}")
}

/// The fake's name for a handle: its address.
fn handle_id(handle: ffi::SANE_Handle) -> usize {
    handle.addr()
}

/// The device lists given out: the current one, and those gone stale, kept
/// (never freed) with their strings overwritten, so a stale read shows as
/// wrong text rather than as freed memory.
struct Lists {
    /// The current list's strings: where each starts and its length, `NUL`
    /// included.
    current: Vec<(*mut u8, usize)>,
}

// SAFETY: the pointers are to leaked allocations this module made and
// writes only while holding `LISTS`; nothing about them is tied to a thread.
unsafe impl Send for Lists {}

static LISTS: Mutex<Lists> = Mutex::new(Lists {
    current: Vec::new(),
});

/// Overwrites the current list's strings, which SANE says are valid only
/// until the next listing or `sane_exit`, and forgets them.
fn stale_list() {
    let mut lists = LISTS.lock().unwrap_or_else(PoisonError::into_inner);
    for (start, length) in lists.current.drain(..) {
        // SAFETY: `start` is a leaked allocation of `length` bytes this
        // module made (in `build_list`), never freed, written only here and
        // only while holding `LISTS`; all but its last byte (the `NUL`) are
        // overwritten.
        unsafe { ptr::write_bytes(start, b'#', length - 1) };
    }
}

/// Builds a device list in leaked memory, and returns its array.
fn build_list(devices: &[[Text; 4]]) -> *const *const ffi::SANE_Device {
    let mut lists = LISTS.lock().unwrap_or_else(PoisonError::into_inner);
    let mut string = |text: &Text| -> *const c_char {
        match text {
            Text::Null => ptr::null(),
            Text::Bytes(bytes) => {
                let mut owned = bytes.clone();
                owned.push(0);
                let length = owned.len();
                let start = Box::leak(owned.into_boxed_slice()).as_mut_ptr();
                lists.current.push((start, length));
                start.cast_const().cast()
            }
        }
    };
    let entries: Vec<ffi::SANE_Device> = devices
        .iter()
        .map(|[name, vendor, model, kind]| ffi::SANE_Device {
            name: string(name),
            vendor: string(vendor),
            model: string(model),
            r#type: string(kind),
        })
        .collect();
    let entries: &'static [ffi::SANE_Device] = Box::leak(entries.into_boxed_slice());
    let mut array: Vec<*const ffi::SANE_Device> = entries.iter().map(ptr::from_ref).collect();
    array.push(ptr::null());
    Box::leak(array.into_boxed_slice()).as_ptr()
}

/// Starts SANE: reads the script (at the first call), and answers with its
/// status and version code (none for a null `version_code`, as the standard
/// allows).
///
/// # Safety
///
/// `version_code` is null or valid for a write.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sane_init(
    version_code: *mut ffi::SANE_Int,
    authorize: ffi::SANE_Auth_Callback,
) -> ffi::SANE_Status {
    let thread = thread();
    let args = format!(
        "authorize={}",
        if authorize.is_some() { "given" } else { "null" }
    );
    let init = with_fake(|fake| {
        fake.enter(&thread, "sane_init", None, &args);
        if fake.script_failed {
            return None;
        }
        Some(fake.init())
    });
    let Some(init) = init else {
        with_fake(|fake| {
            fake.leave(
                &thread,
                "sane_init",
                None,
                Some(ffi::SANE_STATUS_INVAL),
                "script=bad",
            )
        });
        return ffi::SANE_STATUS_INVAL;
    };
    // The standard lets the caller pass a null `version_code`, and then
    // gives no version: no rule is broken.
    let more = if version_code.is_null() {
        "version_code=null".to_owned()
    } else {
        // SAFETY: not null, and valid for a write (the caller's contract).
        unsafe { version_code.write(init.version_code) };
        format!("version={:#010x}", init.version_code)
    };
    with_fake(|fake| fake.leave(&thread, "sane_init", None, Some(init.status), &more));
    init.status
}
const _: ffi::SaneInitFn = sane_init;

/// Ends SANE; the last device list goes stale.
///
/// # Safety
///
/// None beyond SANE's own rules, which the fake checks.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sane_exit() {
    let thread = thread();
    with_fake(|fake| {
        fake.enter(&thread, "sane_exit", None, "");
        fake.exit();
    });
    stale_list();
    with_fake(|fake| fake.leave(&thread, "sane_exit", None, None, ""));
}
const _: ffi::SaneExitFn = sane_exit;

/// Lists the script's devices, after its delay; the last list goes stale
/// first. A null `device_list` is a violation, answered `INVAL`.
///
/// # Safety
///
/// `device_list` is null or valid for a write.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sane_get_devices(
    device_list: *mut *mut *const ffi::SANE_Device,
    local_only: ffi::SANE_Bool,
) -> ffi::SANE_Status {
    let thread = thread();
    let listing = with_fake(|fake| {
        fake.enter(
            &thread,
            "sane_get_devices",
            None,
            &format!("local_only={local_only}"),
        );
        if device_list.is_null() {
            fake.null_argument(&thread, "sane_get_devices", "device_list");
            fake.leave(
                &thread,
                "sane_get_devices",
                None,
                Some(ffi::SANE_STATUS_INVAL),
                "",
            );
            return None;
        }
        Some(fake.listing())
    });
    let Some(listing) = listing else {
        return ffi::SANE_STATUS_INVAL;
    };
    stale_list();
    std::thread::sleep(listing.delay);
    if listing.status != ffi::SANE_STATUS_GOOD {
        with_fake(|fake| fake.leave(&thread, "sane_get_devices", None, Some(listing.status), ""));
        return listing.status;
    }
    let (list, more) = if listing.null {
        (ptr::null(), "list=null".to_owned())
    } else {
        (
            build_list(&listing.devices),
            format!("devices={}", listing.devices.len()),
        )
    };
    // SAFETY: not null (checked above), and valid for a write (the caller's
    // contract).
    unsafe { device_list.write(list.cast_mut()) };
    with_fake(|fake| {
        fake.leave(
            &thread,
            "sane_get_devices",
            None,
            Some(ffi::SANE_STATUS_GOOD),
            &more,
        )
    });
    ffi::SANE_STATUS_GOOD
}
const _: ffi::SaneGetDevicesFn = sane_get_devices;

/// Opens a device by name, as the script says. A null `devicename` or
/// `handle` is a violation, answered `INVAL` with nothing opened.
///
/// # Safety
///
/// `devicename` is null or a `NUL`-terminated string; `handle` is null or
/// valid for a write.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sane_open(
    devicename: ffi::SANE_String_Const,
    handle: *mut ffi::SANE_Handle,
) -> ffi::SANE_Status {
    let thread = thread();
    let name = if devicename.is_null() {
        None
    } else {
        // SAFETY: not null, so a `NUL`-terminated string (the caller's
        // contract).
        Some(
            unsafe { std::ffi::CStr::from_ptr(devicename) }
                .to_bytes()
                .to_vec(),
        )
    };
    let shown = name.as_deref().map_or_else(
        || "null".to_owned(),
        |name| format!("{:?}", String::from_utf8_lossy(name)),
    );
    let (status, id) = with_fake(|fake| {
        fake.enter(&thread, "sane_open", None, &format!("name={shown}"));
        // Refused before anything is opened, so no handle is left open that
        // the caller was never given.
        if handle.is_null() {
            fake.null_argument(&thread, "sane_open", "handle");
        }
        match &name {
            None => {
                fake.null_argument(&thread, "sane_open", "devicename");
                (ffi::SANE_STATUS_INVAL, None)
            }
            Some(_) if handle.is_null() => (ffi::SANE_STATUS_INVAL, None),
            Some(name) => fake.open(name),
        }
    });
    if !handle.is_null() {
        let given = id.map_or(ptr::null_mut(), ptr::without_provenance_mut::<c_void>);
        // SAFETY: not null, and valid for a write (the caller's contract).
        unsafe { handle.write(given) };
    }
    let more = id.map_or_else(|| "handle=null".to_owned(), |id| format!("handle={id:#x}"));
    with_fake(|fake| fake.leave(&thread, "sane_open", None, Some(status), &more));
    status
}
const _: ffi::SaneOpenFn = sane_open;

/// Closes a device.
///
/// # Safety
///
/// None beyond SANE's own rules, which the fake checks: the handle is only
/// compared, never followed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sane_close(handle: ffi::SANE_Handle) {
    let thread = thread();
    let id = handle_id(handle);
    with_fake(|fake| {
        fake.enter(&thread, "sane_close", Some(id), &format!("handle={id:#x}"));
        fake.close(id);
        fake.leave(&thread, "sane_close", Some(id), None, "");
    });
}
const _: ffi::SaneCloseFn = sane_close;

/// No options yet: always null.
///
/// # Safety
///
/// None beyond SANE's own rules, which the fake checks.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sane_get_option_descriptor(
    handle: ffi::SANE_Handle,
    option: ffi::SANE_Int,
) -> *const ffi::SANE_Option_Descriptor {
    let thread = thread();
    let id = handle_id(handle);
    with_fake(|fake| {
        fake.enter(
            &thread,
            "sane_get_option_descriptor",
            Some(id),
            &format!("handle={id:#x} option={option}"),
        );
        fake.leave(
            &thread,
            "sane_get_option_descriptor",
            Some(id),
            None,
            "descriptor=null",
        );
    });
    ptr::null()
}
const _: ffi::SaneGetOptionDescriptorFn = sane_get_option_descriptor;

/// No options yet: always `UNSUPPORTED`.
///
/// # Safety
///
/// None beyond SANE's own rules, which the fake checks: no pointer is
/// followed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sane_control_option(
    handle: ffi::SANE_Handle,
    option: ffi::SANE_Int,
    action: ffi::SANE_Action,
    _value: *mut c_void,
    _info: *mut ffi::SANE_Int,
) -> ffi::SANE_Status {
    unsupported(
        handle,
        "sane_control_option",
        &format!("option={option} action={action}"),
    )
}
const _: ffi::SaneControlOptionFn = sane_control_option;

/// No scanning yet: always `UNSUPPORTED`.
///
/// # Safety
///
/// None beyond SANE's own rules, which the fake checks: no pointer is
/// followed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sane_get_parameters(
    handle: ffi::SANE_Handle,
    _params: *mut ffi::SANE_Parameters,
) -> ffi::SANE_Status {
    unsupported(handle, "sane_get_parameters", "")
}
const _: ffi::SaneGetParametersFn = sane_get_parameters;

/// No scanning yet: always `UNSUPPORTED`.
///
/// # Safety
///
/// None beyond SANE's own rules, which the fake checks.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sane_start(handle: ffi::SANE_Handle) -> ffi::SANE_Status {
    unsupported(handle, "sane_start", "")
}
const _: ffi::SaneStartFn = sane_start;

/// No scanning yet: always `UNSUPPORTED`, nothing read.
///
/// # Safety
///
/// `length` is null or valid for a write.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sane_read(
    handle: ffi::SANE_Handle,
    _data: *mut ffi::SANE_Byte,
    max_length: ffi::SANE_Int,
    length: *mut ffi::SANE_Int,
) -> ffi::SANE_Status {
    if !length.is_null() {
        // SAFETY: not null, and valid for a write (the caller's contract).
        unsafe { length.write(0) };
    }
    unsupported(handle, "sane_read", &format!("max_length={max_length}"))
}
const _: ffi::SaneReadFn = sane_read;

/// Cancels; logged and checked, as the one call that may overlap others.
///
/// # Safety
///
/// None beyond SANE's own rules, which the fake checks.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sane_cancel(handle: ffi::SANE_Handle) {
    let thread = thread();
    let id = handle_id(handle);
    with_fake(|fake| {
        fake.enter(&thread, "sane_cancel", Some(id), &format!("handle={id:#x}"));
        fake.leave(&thread, "sane_cancel", Some(id), None, "");
    });
}
const _: ffi::SaneCancelFn = sane_cancel;

/// No scanning yet: always `UNSUPPORTED`.
///
/// # Safety
///
/// None beyond SANE's own rules, which the fake checks.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sane_set_io_mode(
    handle: ffi::SANE_Handle,
    non_blocking: ffi::SANE_Bool,
) -> ffi::SANE_Status {
    unsupported(
        handle,
        "sane_set_io_mode",
        &format!("non_blocking={non_blocking}"),
    )
}
const _: ffi::SaneSetIoModeFn = sane_set_io_mode;

/// No scanning yet: always `UNSUPPORTED`.
///
/// # Safety
///
/// None beyond SANE's own rules, which the fake checks: no pointer is
/// followed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sane_get_select_fd(
    handle: ffi::SANE_Handle,
    _fd: *mut ffi::SANE_Int,
) -> ffi::SANE_Status {
    unsupported(handle, "sane_get_select_fd", "")
}
const _: ffi::SaneGetSelectFdFn = sane_get_select_fd;

/// A status in words, or null if the script says so.
///
/// # Safety
///
/// None beyond SANE's own rules, which the fake checks.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sane_strstatus(status: ffi::SANE_Status) -> ffi::SANE_String_Const {
    let thread = thread();
    with_fake(|fake| {
        fake.enter(&thread, "sane_strstatus", None, &format!("status={status}"));
        let text = fake.strstatus(status);
        fake.leave(
            &thread,
            "sane_strstatus",
            None,
            None,
            if text.is_some() { "text" } else { "text=null" },
        );
        text.map_or(ptr::null(), |text| text.as_ptr())
    })
}
const _: ffi::SaneStrstatusFn = sane_strstatus;

/// A call on a handle that the fake doesn't play yet: logged, checked, and
/// answered `UNSUPPORTED`.
fn unsupported(handle: ffi::SANE_Handle, function: &'static str, args: &str) -> ffi::SANE_Status {
    let thread = thread();
    let id = handle_id(handle);
    with_fake(|fake| {
        fake.enter(
            &thread,
            function,
            Some(id),
            format!("handle={id:#x} {args}").trim_end(),
        );
        fake.leave(
            &thread,
            function,
            Some(id),
            Some(ffi::SANE_STATUS_UNSUPPORTED),
            "",
        );
    });
    ffi::SANE_STATUS_UNSUPPORTED
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Held by each test that calls the exports, which share the fake and
    /// the device lists, so no two of them run at once.
    fn serial() -> MutexGuard<'static, ()> {
        static SERIAL: Mutex<()> = Mutex::new(());
        SERIAL.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// A device's name, read from the list as a caller would.
    fn first_name(list: *const *const ffi::SANE_Device) -> Vec<u8> {
        // SAFETY: `list` came from `build_list`, whose memory is leaked and
        // never freed, so its first entry and that entry's name stay valid;
        // the name is `NUL`-terminated, before and after it goes stale.
        unsafe { std::ffi::CStr::from_ptr((**list).name) }
            .to_bytes()
            .to_vec()
    }

    #[test]
    fn a_stale_list_is_overwritten_and_kept() {
        let _serial = serial();
        let bytes = |text: &str| Text::Bytes(text.as_bytes().to_vec());
        let list = build_list(&[[bytes("fake:0"), bytes("v"), Text::Null, bytes("t")]]);
        assert_eq!(first_name(list), b"fake:0");
        stale_list();
        assert_eq!(first_name(list), b"######");
        // The next list is new memory, with its own strings.
        let next = build_list(&[[bytes("fake:1"), bytes("v"), bytes("m"), bytes("t")]]);
        assert_eq!(first_name(next), b"fake:1");
        assert_eq!(first_name(list), b"######");
    }

    #[test]
    fn null_pointers_sane_must_write_through_are_caught() {
        let _serial = serial();
        let script = Script::parse("device fake:0 a b c\n").unwrap();
        *fake() = Some(Fake::new(script, Log::memory()));
        // SAFETY: a null `version_code` is allowed, and the fake writes
        // nothing through it.
        let status = unsafe { sane_init(ptr::null_mut(), None) };
        assert_eq!(status, ffi::SANE_STATUS_GOOD);
        // SAFETY: the null pointers below are what's tested: the fake checks
        // each before it would write through it or read it; every other
        // pointer is valid for its write, or a `NUL`-terminated string.
        let statuses = unsafe {
            let mut handle: ffi::SANE_Handle = ptr::null_mut();
            [
                sane_get_devices(ptr::null_mut(), 0),
                sane_open(ptr::null(), &mut handle),
                sane_open(c"fake:0".as_ptr(), ptr::null_mut()),
            ]
        };
        assert_eq!(statuses, [ffi::SANE_STATUS_INVAL; 3]);
        // SAFETY: SANE was started above, and no handle is open: a null
        // handle's open opened nothing.
        unsafe { sane_exit() };
        let fake = fake().take().unwrap();
        assert_eq!(
            fake.violations,
            [
                "sane_get_devices: was given a null device_list",
                "sane_open: was given a null devicename",
                "sane_open: was given a null handle",
            ]
        );
        assert!(
            fake.log
                .lines
                .iter()
                .any(|line| line.ends_with(" sane_init status=GOOD version_code=null")),
            "{:?}",
            fake.log.lines
        );
    }
}
