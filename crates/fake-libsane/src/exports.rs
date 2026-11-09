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

use std::collections::BTreeMap;

use crate::fake::{Fake, Given, Log, Wait, show};
use crate::feed::ReadAnswer;
use crate::ffi;
use crate::options::{ConstraintSpec, Descriptor, RawValue};
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

/// A leaked string, `NUL`-terminated, or null.
fn leak_text(text: &Text) -> *const c_char {
    match text {
        Text::Null => ptr::null(),
        Text::Bytes(bytes) => {
            let mut owned = bytes.clone();
            owned.push(0);
            Box::leak(owned.into_boxed_slice()).as_ptr().cast()
        }
    }
}

/// A descriptor as C has it, its strings, range and lists in leaked memory
/// that's never freed.
fn build_descriptor(descriptor: &Descriptor) -> ffi::SANE_Option_Descriptor {
    let constraint = match &descriptor.constraint {
        ConstraintSpec::None => ffi::SANE_Constraint { range: ptr::null() },
        ConstraintSpec::Range(min, max, quant) => ffi::SANE_Constraint {
            range: Box::leak(Box::new(ffi::SANE_Range {
                min: *min,
                max: *max,
                quant: *quant,
            })),
        },
        ConstraintSpec::Words(words, claimed) => {
            let length = claimed.unwrap_or_else(|| i32::try_from(words.len()).unwrap_or(i32::MAX));
            let list: Vec<ffi::SANE_Word> = std::iter::once(length)
                .chain(words.iter().copied())
                .collect();
            ffi::SANE_Constraint {
                word_list: Box::leak(list.into_boxed_slice()).as_ptr(),
            }
        }
        ConstraintSpec::Strings(strings) => {
            let list: Vec<ffi::SANE_String_Const> = strings
                .iter()
                .map(leak_text)
                .chain(std::iter::once(ptr::null()))
                .collect();
            ffi::SANE_Constraint {
                string_list: Box::leak(list.into_boxed_slice()).as_ptr(),
            }
        }
    };
    ffi::SANE_Option_Descriptor {
        name: leak_text(&descriptor.name),
        title: leak_text(&descriptor.title),
        desc: leak_text(&descriptor.desc),
        r#type: descriptor.value_type,
        unit: descriptor.unit,
        size: descriptor.size,
        cap: descriptor.cap,
        constraint_type: descriptor.constraint_type,
        constraint,
    }
}

/// A descriptor given out, leaked and never freed, so it stays valid at its
/// address for good, as SANE requires until the device is closed.
struct Built(*mut ffi::SANE_Option_Descriptor);

// SAFETY: the pointer is to a leaked allocation this module made and writes
// only while holding `DESCRIPTORS`; nothing about it is tied to a thread.
unsafe impl Send for Built {}

/// The descriptors given out, by handle and option, with the version of the
/// option's descriptor each was built from.
static DESCRIPTORS: Mutex<BTreeMap<(usize, i32), (u64, Built)>> = Mutex::new(BTreeMap::new());

/// The descriptor of a handle's option: the one given before if it hasn't
/// changed, rewritten in place at the same address if it has, or a new one.
fn descriptor_pointer(
    id: usize,
    option: i32,
    version: u64,
    descriptor: &Descriptor,
) -> *const ffi::SANE_Option_Descriptor {
    let mut built = DESCRIPTORS.lock().unwrap_or_else(PoisonError::into_inner);
    match built.get_mut(&(id, option)) {
        Some((given, pointer)) => {
            if *given != version {
                // SAFETY: `pointer` is a leaked descriptor this module made,
                // never freed, written only here and only while holding
                // `DESCRIPTORS`; its old strings and lists stay leaked, so a
                // copy of the old descriptor still points to valid memory.
                unsafe { pointer.0.write(build_descriptor(descriptor)) };
                *given = version;
            }
            pointer.0
        }
        None => {
            let pointer = Box::leak(Box::new(build_descriptor(descriptor)));
            built.insert((id, option), (version, Built(pointer)));
            pointer
        }
    }
}

/// An option's descriptor, as the script has it now, or null for an option
/// that doesn't exist or that the script makes null.
///
/// # Safety
///
/// None beyond SANE's own rules, which the fake checks: the handle is only
/// compared, never followed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sane_get_option_descriptor(
    handle: ffi::SANE_Handle,
    option: ffi::SANE_Int,
) -> *const ffi::SANE_Option_Descriptor {
    let thread = thread();
    let id = handle_id(handle);
    let found = with_fake(|fake| {
        fake.enter(
            &thread,
            "sane_get_option_descriptor",
            Some(id),
            &format!("handle={id:#x} option={option}"),
        );
        let found = fake.descriptor(id, option);
        fake.leave(
            &thread,
            "sane_get_option_descriptor",
            Some(id),
            None,
            if found.is_some() {
                "descriptor"
            } else {
                "descriptor=null"
            },
        );
        found
    });
    match found {
        None => ptr::null(),
        Some((descriptor, version)) => descriptor_pointer(id, option, version, &descriptor),
    }
}
const _: ffi::SaneGetOptionDescriptorFn = sane_get_option_descriptor;

/// Reads what a set gives: a string up to its `NUL` within `size` bytes,
/// or `size / 4` words (one for a button).
///
/// # Safety
///
/// `value` is valid for reads of `size` bytes (one word for a button), and
/// `size` is positive for anything but a button.
unsafe fn read_given(value: *const c_void, value_type: i32, size: i32) -> Given {
    let size = usize::try_from(size).unwrap_or(0);
    if value_type == ffi::SANE_TYPE_STRING {
        // SAFETY: valid for `size` bytes (the caller's contract).
        let bytes = unsafe { std::slice::from_raw_parts(value.cast::<u8>(), size) };
        return match bytes.iter().position(|&byte| byte == 0) {
            Some(end) => Given::Value(RawValue::Bytes(bytes[..end].to_vec())),
            None => Given::Unterminated(size),
        };
    }
    let count = if value_type == ffi::SANE_TYPE_BUTTON {
        1
    } else {
        size / 4
    };
    // SAFETY: valid for `count` words (the caller's contract), and a
    // caller's word buffer is aligned for words.
    let words = unsafe { std::slice::from_raw_parts(value.cast::<ffi::SANE_Word>(), count) };
    Given::Value(RawValue::Words(words.to_vec()))
}

/// Reads, sets or lets the backend choose an option's value, as the script
/// has it, checking first that the call is one a caller should make.
///
/// # Safety
///
/// `value` is null or valid for the option's whole value, as its
/// descriptor sizes it (one word for a button); `info` is null or valid for
/// a write.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sane_control_option(
    handle: ffi::SANE_Handle,
    option: ffi::SANE_Int,
    action: ffi::SANE_Action,
    value: *mut c_void,
    info: *mut ffi::SANE_Int,
) -> ffi::SANE_Status {
    let thread = thread();
    let id = handle_id(handle);
    let action_name = match action {
        ffi::SANE_ACTION_GET_VALUE => "GET".to_owned(),
        ffi::SANE_ACTION_SET_VALUE => "SET".to_owned(),
        ffi::SANE_ACTION_SET_AUTO => "AUTO".to_owned(),
        other => other.to_string(),
    };
    let shape = with_fake(|fake| fake.shape(id, option));
    let given = match shape {
        Some((value_type, size)) if action == ffi::SANE_ACTION_SET_VALUE && !value.is_null() => {
            // SAFETY: not null, and valid for the option's whole value (the
            // caller's contract); `shape` gives only options of a size the
            // standard allows for their type.
            unsafe { read_given(value, value_type, size) }
        }
        _ => Given::Nothing,
    };
    let shown = match &given {
        Given::Value(given) => format!(" value={}", show(given)),
        Given::Unterminated(size) => format!(" value=unterminated:{size}"),
        Given::Nothing => String::new(),
    };
    let answer = with_fake(|fake| {
        fake.enter(
            &thread,
            "sane_control_option",
            Some(id),
            &format!("handle={id:#x} option={option} action={action_name}{shown}"),
        );
        let answer = fake.control(&thread, id, option, action, value.is_null(), given);
        let written = answer
            .write
            .as_ref()
            .map(|written| format!(" value={}", show(written)))
            .unwrap_or_default();
        fake.leave(
            &thread,
            "sane_control_option",
            Some(id),
            Some(answer.status),
            &format!("info={:#x}{written}", answer.info),
        );
        answer
    });
    if let Some(written) = &answer.write
        && !value.is_null()
    {
        match written {
            RawValue::Words(words) => {
                // SAFETY: not null, and valid for the option's whole value,
                // which `words` is (the fake sizes it from the descriptor);
                // a caller's word buffer is aligned for words.
                unsafe { ptr::copy_nonoverlapping(words.as_ptr(), value.cast(), words.len()) };
            }
            RawValue::Bytes(bytes) => {
                // SAFETY: not null, and valid for the option's whole value,
                // which `bytes` doesn't pass (the fake sizes it from the
                // descriptor).
                unsafe { ptr::copy_nonoverlapping(bytes.as_ptr(), value.cast(), bytes.len()) };
            }
        }
    }
    if !info.is_null() {
        // SAFETY: not null, and valid for a write (the caller's contract).
        unsafe { info.write(answer.info) };
    }
    answer.status
}
const _: ffi::SaneControlOptionFn = sane_control_option;

/// The parameters of the frame in progress, or of the next, as the script
/// has them. A null `params` is a violation, answered `INVAL`.
///
/// # Safety
///
/// `params` is null or valid for a write.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sane_get_parameters(
    handle: ffi::SANE_Handle,
    params: *mut ffi::SANE_Parameters,
) -> ffi::SANE_Status {
    let thread = thread();
    let id = handle_id(handle);
    with_fake(|fake| {
        fake.enter(
            &thread,
            "sane_get_parameters",
            Some(id),
            &format!("handle={id:#x}"),
        );
        if params.is_null() {
            fake.null_argument(&thread, "sane_get_parameters", "params");
            fake.leave(
                &thread,
                "sane_get_parameters",
                Some(id),
                Some(ffi::SANE_STATUS_INVAL),
                "",
            );
            return ffi::SANE_STATUS_INVAL;
        }
        let (status, given) = fake.parameters(id);
        // SAFETY: not null, and valid for a write (the caller's contract).
        unsafe { params.write(given) };
        fake.leave(
            &thread,
            "sane_get_parameters",
            Some(id),
            Some(status),
            &format!(
                "format={} last_frame={} bytes_per_line={} pixels_per_line={} lines={} depth={}",
                given.format,
                given.last_frame,
                given.bytes_per_line,
                given.pixels_per_line,
                given.lines,
                given.depth
            ),
        );
        status
    })
}
const _: ffi::SaneGetParametersFn = sane_get_parameters;

/// Starts the next frame of the page, or the next page, as the script has
/// them; aborts the process where the script says.
///
/// # Safety
///
/// None beyond SANE's own rules, which the fake checks.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sane_start(handle: ffi::SANE_Handle) -> ffi::SANE_Status {
    let thread = thread();
    let id = handle_id(handle);
    let status = with_fake(|fake| {
        fake.enter(&thread, "sane_start", Some(id), &format!("handle={id:#x}"));
        let status = fake.start(&thread, id);
        match status {
            Some(status) => {
                let position = fake.position(id);
                fake.leave(&thread, "sane_start", Some(id), Some(status), &position);
            }
            None => fake.note(
                &thread,
                "abort sane_start aborts the process, as the script says",
            ),
        }
        status
    });
    status.unwrap_or_else(|| std::process::abort())
}
const _: ffi::SaneStartFn = sane_start;

/// What a read does once the fake has decided.
enum Reading {
    Data(Vec<u8>, ffi::SANE_Int),
    Done(ffi::SANE_Status),
    Wait(Wait),
    Forever,
}

/// Reads the frame's next bytes, as the script has them; or blocks until
/// `sane_cancel`, or for good, where the script says, outside the fake's
/// lock, so a cancel from another thread gets in. A null `data` or `length`
/// is a violation, answered `INVAL`.
///
/// # Safety
///
/// `data` is null or valid for writes of `max_length` bytes; `length` is
/// null or valid for a write.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sane_read(
    handle: ffi::SANE_Handle,
    data: *mut ffi::SANE_Byte,
    max_length: ffi::SANE_Int,
    length: *mut ffi::SANE_Int,
) -> ffi::SANE_Status {
    let thread = thread();
    let id = handle_id(handle);
    let reading = with_fake(|fake| {
        fake.enter(
            &thread,
            "sane_read",
            Some(id),
            &format!("handle={id:#x} max_length={max_length}"),
        );
        let leave = |fake: &mut Fake, status: i32, more: &str| {
            fake.leave(&thread, "sane_read", Some(id), Some(status), more);
        };
        if data.is_null() || length.is_null() {
            let what = if data.is_null() { "data" } else { "length" };
            fake.null_argument(&thread, "sane_read", what);
            leave(fake, ffi::SANE_STATUS_INVAL, "");
            return Reading::Done(ffi::SANE_STATUS_INVAL);
        }
        match fake.read(&thread, id, max_length) {
            (ReadAnswer::Data { bytes, claim }, _) => {
                leave(
                    fake,
                    ffi::SANE_STATUS_GOOD,
                    &format!("length={claim} given={}", bytes.len()),
                );
                Reading::Data(bytes, claim)
            }
            (ReadAnswer::Status(status), _) => {
                leave(fake, status, "");
                Reading::Done(status)
            }
            (ReadAnswer::Hang, Some(wait)) => {
                fake.note(&thread, "hang sane_read waits for sane_cancel");
                Reading::Wait(wait)
            }
            (ReadAnswer::HangForever, _) => {
                fake.note(&thread, "hang sane_read waits for good");
                Reading::Forever
            }
            (ReadAnswer::Hang | ReadAnswer::NotStarted, _) => {
                leave(fake, ffi::SANE_STATUS_INVAL, "");
                Reading::Done(ffi::SANE_STATUS_INVAL)
            }
        }
    });
    let status = match reading {
        Reading::Data(bytes, claim) => {
            // SAFETY: not null (checked above), and valid for `max_length`
            // bytes (the caller's contract), which `bytes` doesn't pass: the
            // fake gives no more than it's asked for.
            unsafe { ptr::copy_nonoverlapping(bytes.as_ptr(), data, bytes.len()) };
            // SAFETY: not null (checked above), and valid for a write.
            unsafe { length.write(claim) };
            return ffi::SANE_STATUS_GOOD;
        }
        Reading::Done(status) => status,
        Reading::Wait((signal, seen)) => {
            signal.wait_past(seen);
            with_fake(|fake| {
                fake.leave(
                    &thread,
                    "sane_read",
                    Some(id),
                    Some(ffi::SANE_STATUS_CANCELLED),
                    "",
                )
            });
            ffi::SANE_STATUS_CANCELLED
        }
        Reading::Forever => loop {
            std::thread::park();
        },
    };
    if !length.is_null() {
        // SAFETY: not null, and valid for a write (the caller's contract).
        unsafe { length.write(0) };
    }
    status
}
const _: ffi::SaneReadFn = sane_read;

/// Cancels: the page in flight is dropped, and a read waiting for a cancel
/// returns. Logged and checked, as the one call that may overlap others.
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
        fake.cancel(id);
        fake.leave(&thread, "sane_cancel", Some(id), None, "");
    });
}
const _: ffi::SaneCancelFn = sane_cancel;

/// Blocking always; non-blocking where the script says, else
/// `UNSUPPORTED`; `INVAL` with no frame in progress. Or `UNSUPPORTED`
/// whatever is asked, where the script says.
///
/// # Safety
///
/// None beyond SANE's own rules, which the fake checks.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sane_set_io_mode(
    handle: ffi::SANE_Handle,
    non_blocking: ffi::SANE_Bool,
) -> ffi::SANE_Status {
    let thread = thread();
    let id = handle_id(handle);
    with_fake(|fake| {
        fake.enter(
            &thread,
            "sane_set_io_mode",
            Some(id),
            &format!("handle={id:#x} non_blocking={non_blocking}"),
        );
        let status = fake.io_mode(id, non_blocking);
        fake.leave(&thread, "sane_set_io_mode", Some(id), Some(status), "");
        status
    })
}
const _: ffi::SaneSetIoModeFn = sane_set_io_mode;

/// The script's descriptor, else `UNSUPPORTED`; `INVAL` with no frame in
/// progress. A null `fd` is a violation, answered `INVAL`.
///
/// # Safety
///
/// `fd` is null or valid for a write.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sane_get_select_fd(
    handle: ffi::SANE_Handle,
    fd: *mut ffi::SANE_Int,
) -> ffi::SANE_Status {
    let thread = thread();
    let id = handle_id(handle);
    with_fake(|fake| {
        fake.enter(
            &thread,
            "sane_get_select_fd",
            Some(id),
            &format!("handle={id:#x}"),
        );
        if fd.is_null() {
            fake.null_argument(&thread, "sane_get_select_fd", "fd");
            fake.leave(
                &thread,
                "sane_get_select_fd",
                Some(id),
                Some(ffi::SANE_STATUS_INVAL),
                "",
            );
            return ffi::SANE_STATUS_INVAL;
        }
        let (status, given) = fake.select_fd(id);
        if let Some(given) = given {
            // SAFETY: not null, and valid for a write (the caller's
            // contract).
            unsafe { fd.write(given) };
        }
        let more = given.map(|given| format!("fd={given}")).unwrap_or_default();
        fake.leave(&thread, "sane_get_select_fd", Some(id), Some(status), &more);
        status
    })
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

    #[test]
    fn a_descriptor_that_changes_stays_at_its_address() {
        let _serial = serial();
        let script = Script::parse(
            "device fake:0 a b c\noption s STRING 8 soft-select+soft-detect\nwhen x size t 8\noption t INT 4 soft-select+soft-detect\n",
        )
        .unwrap();
        *fake() = Some(Fake::new(script, Log::memory()));
        let mut handle: ffi::SANE_Handle = ptr::null_mut();
        let mut info: ffi::SANE_Int = 0;
        let mut value = *b"x\0\0\0\0\0\0\0";
        // SAFETY: every pointer is valid for what the call does with it: the
        // handle and info for a write, the name a `NUL`-terminated string,
        // the value the option's whole 8 bytes.
        let (first, second, size) = unsafe {
            sane_init(ptr::null_mut(), None);
            sane_open(c"fake:0".as_ptr(), &mut handle);
            let first = sane_get_option_descriptor(handle, 2);
            let size = (*first).size;
            sane_control_option(
                handle,
                1,
                ffi::SANE_ACTION_SET_VALUE,
                value.as_mut_ptr().cast(),
                &mut info,
            );
            for option in 0..3 {
                sane_get_option_descriptor(handle, option);
            }
            let second = sane_get_option_descriptor(handle, 2);
            (first, second, size)
        };
        assert_eq!(first, second);
        assert_eq!(size, 4);
        // SAFETY: the descriptor is leaked and never freed.
        assert_eq!(unsafe { (*second).size }, 8);
        assert_eq!(info, ffi::SANE_INFO_RELOAD_OPTIONS);
        // SAFETY: SANE was started above; the handle is open, and closed once.
        unsafe {
            sane_close(handle);
            sane_exit();
        }
        assert_eq!(fake().take().unwrap().violations, Vec::<String>::new());
    }
}
