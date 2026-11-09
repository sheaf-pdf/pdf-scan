//! The bindings checked against the installed `sane.h`.
//!
//! The idea is sanetui's header-contract test (Apache-2.0,
//! <https://github.com/nathawatc/sanetui>); this code is our own. Two passes,
//! where the header and a C compiler are installed:
//!
//! - **The compiler's:** a C program, written here, includes the header and
//!   prints every constant's value, every type's size, alignment and
//!   signedness, every field's offset and size, and the function-like
//!   macros' values for a few arguments, and asserts at compile time that
//!   each function has the type spelled in [`FUNCTIONS`] (which references no
//!   symbol of libsane, so nothing links it). Each is compared with the
//!   bindings.
//! - **The text's:** every numeric `#define`, every enumerator, every
//!   typedef, every function and every function-like macro the header's text
//!   declares must be in the tables here, and nothing else: the values kept
//!   under `#if 0` (reserved for a later SANE) among them, with their values.
//!
//! The C spellings use the header's own typedef names, since C takes an enum
//! as compatible with `unsigned int` rather than `int`. Each table row ties
//! the binding to the row's Rust type at compile time, so the two can't
//! drift. The bindings are the crate's own source, `src/ffi.rs`, taken here
//! by its path, so the function types the crate keeps inside it are checked
//! too, in the very file the crate compiles. The header is
//! `/usr/include/sane/sane.h`, or the path in `SANE_DL_SANE_H`; the compiler
//! `$CC`, or `cc`. Where either is missing the test says so on standard
//! error and passes, or fails with `SANE_DL_REQUIRE_HEADER=1`.

mod common;

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::mem::{align_of, offset_of, size_of};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::ptr;

// The bindings' own source, whose function types are inside the crate; not
// every declaration is used here.
#[allow(dead_code)]
#[path = "../src/ffi.rs"]
mod ffi;

/// The identity, for tying a binding's type to a row's at compile time.
fn same<T>(value: T) -> T {
    value
}

/// A field's size, from a function that borrows it.
fn field_size<S, F>(_: fn(&S) -> &F) -> usize {
    size_of::<F>()
}

/// A constant: its name in C and Rust, the binding's value, and whether the
/// header keeps it under `#if 0`.
struct Constant {
    name: &'static str,
    value: i64,
    reserved: bool,
}

/// Rows of constants, each binding checked to have the type given.
macro_rules! constants {
    ($($name:ident: $type:ident $(, $reserved:ident)?;)*) => {
        &[$({
            const _: ffi::$type = ffi::$name;
            Constant { name: stringify!($name), value: ffi::$name as i64, reserved: constants!(@reserved $($reserved)?) }
        }),*]
    };
    (@reserved reserved) => { true };
    (@reserved) => { false };
}

const CONSTANTS: &[Constant] = constants! {
    SANE_CURRENT_MAJOR: SANE_Word;
    SANE_CURRENT_MINOR: SANE_Word;
    SANE_FALSE: SANE_Bool;
    SANE_TRUE: SANE_Bool;
    SANE_FIXED_SCALE_SHIFT: SANE_Word;
    SANE_STATUS_GOOD: SANE_Status;
    SANE_STATUS_UNSUPPORTED: SANE_Status;
    SANE_STATUS_CANCELLED: SANE_Status;
    SANE_STATUS_DEVICE_BUSY: SANE_Status;
    SANE_STATUS_INVAL: SANE_Status;
    SANE_STATUS_EOF: SANE_Status;
    SANE_STATUS_JAMMED: SANE_Status;
    SANE_STATUS_NO_DOCS: SANE_Status;
    SANE_STATUS_COVER_OPEN: SANE_Status;
    SANE_STATUS_IO_ERROR: SANE_Status;
    SANE_STATUS_NO_MEM: SANE_Status;
    SANE_STATUS_ACCESS_DENIED: SANE_Status;
    SANE_STATUS_WARMING_UP: SANE_Status, reserved;
    SANE_STATUS_HW_LOCKED: SANE_Status, reserved;
    SANE_TYPE_BOOL: SANE_Value_Type;
    SANE_TYPE_INT: SANE_Value_Type;
    SANE_TYPE_FIXED: SANE_Value_Type;
    SANE_TYPE_STRING: SANE_Value_Type;
    SANE_TYPE_BUTTON: SANE_Value_Type;
    SANE_TYPE_GROUP: SANE_Value_Type;
    SANE_UNIT_NONE: SANE_Unit;
    SANE_UNIT_PIXEL: SANE_Unit;
    SANE_UNIT_BIT: SANE_Unit;
    SANE_UNIT_MM: SANE_Unit;
    SANE_UNIT_DPI: SANE_Unit;
    SANE_UNIT_PERCENT: SANE_Unit;
    SANE_UNIT_MICROSECOND: SANE_Unit;
    SANE_CAP_SOFT_SELECT: SANE_Int;
    SANE_CAP_HARD_SELECT: SANE_Int;
    SANE_CAP_SOFT_DETECT: SANE_Int;
    SANE_CAP_EMULATED: SANE_Int;
    SANE_CAP_AUTOMATIC: SANE_Int;
    SANE_CAP_INACTIVE: SANE_Int;
    SANE_CAP_ADVANCED: SANE_Int;
    SANE_INFO_INEXACT: SANE_Int;
    SANE_INFO_RELOAD_OPTIONS: SANE_Int;
    SANE_INFO_RELOAD_PARAMS: SANE_Int;
    SANE_CONSTRAINT_NONE: SANE_Constraint_Type;
    SANE_CONSTRAINT_RANGE: SANE_Constraint_Type;
    SANE_CONSTRAINT_WORD_LIST: SANE_Constraint_Type;
    SANE_CONSTRAINT_STRING_LIST: SANE_Constraint_Type;
    SANE_ACTION_GET_VALUE: SANE_Action;
    SANE_ACTION_SET_VALUE: SANE_Action;
    SANE_ACTION_SET_AUTO: SANE_Action;
    SANE_FRAME_GRAY: SANE_Frame;
    SANE_FRAME_RGB: SANE_Frame;
    SANE_FRAME_RED: SANE_Frame;
    SANE_FRAME_GREEN: SANE_Frame;
    SANE_FRAME_BLUE: SANE_Frame;
    SANE_FRAME_TEXT: SANE_Frame, reserved;
    SANE_FRAME_JPEG: SANE_Frame, reserved;
    SANE_FRAME_G31D: SANE_Frame, reserved;
    SANE_FRAME_G32D: SANE_Frame, reserved;
    SANE_FRAME_G42D: SANE_Frame, reserved;
    SANE_FRAME_IR: SANE_Frame, reserved;
    SANE_FRAME_RGBI: SANE_Frame, reserved;
    SANE_FRAME_GRAYI: SANE_Frame, reserved;
    SANE_FRAME_XML: SANE_Frame, reserved;
    SANE_MAX_USERNAME_LEN: SANE_Int;
    SANE_MAX_PASSWORD_LEN: SANE_Int;
};

/// A type: its name in C and Rust, and the binding's size, alignment and,
/// for an integer, whether it's signed. The header's enums are integers
/// here, checked by size and values only: C chooses their signedness.
struct Type {
    name: &'static str,
    size: usize,
    align: usize,
    signed: Option<bool>,
}

macro_rules! types {
    ($($name:ident $(: $integer:ident)?;)*) => {
        &[$(Type {
            name: stringify!($name),
            size: size_of::<ffi::$name>(),
            align: align_of::<ffi::$name>(),
            signed: types!(@signed $name $($integer)?),
        }),*]
    };
    (@signed $name:ident integer) => { Some(<ffi::$name>::MIN != 0) };
    (@signed $name:ident) => { None };
}

const TYPES: &[Type] = types! {
    SANE_Byte: integer;
    SANE_Word: integer;
    SANE_Bool: integer;
    SANE_Int: integer;
    SANE_Char: integer;
    SANE_String;
    SANE_String_Const;
    SANE_Handle;
    SANE_Fixed: integer;
    SANE_Status;
    SANE_Value_Type;
    SANE_Unit;
    SANE_Device;
    SANE_Constraint_Type;
    SANE_Range;
    SANE_Option_Descriptor;
    SANE_Action;
    SANE_Frame;
    SANE_Parameters;
    SANE_Auth_Callback;
};

/// A field of a struct, or a member of the descriptor's constraint union
/// (`constraint.range`): its offset and size.
struct Field {
    owner: &'static str,
    field: &'static str,
    offset: usize,
    size: usize,
}

macro_rules! fields {
    ($($owner:ident . $field:ident;)*) => {
        [$(Field {
            owner: stringify!($owner),
            field: stringify!($field),
            offset: offset_of!(ffi::$owner, $field),
            size: field_size(|value: &ffi::$owner| &value.$field),
        }),*]
    };
}

/// The constraint union's members, each checked to have the type given by
/// building the union from a null pointer of that type.
macro_rules! constraint_members {
    ($($member:ident: $type:ty;)*) => {
        [$({
            let _ = ffi::SANE_Constraint { $member: ptr::null::<$type>() };
            Field {
                owner: "SANE_Option_Descriptor",
                field: concat!("constraint.", stringify!($member)),
                offset: offset_of!(ffi::SANE_Option_Descriptor, constraint) + offset_of!(ffi::SANE_Constraint, $member),
                size: size_of::<*const $type>(),
            }
        }),*]
    };
}

fn fields() -> Vec<Field> {
    let mut fields = Vec::from(fields! {
        SANE_Device.name;
        SANE_Device.vendor;
        SANE_Device.model;
        SANE_Device.r#type;
        SANE_Range.min;
        SANE_Range.max;
        SANE_Range.quant;
        SANE_Option_Descriptor.name;
        SANE_Option_Descriptor.title;
        SANE_Option_Descriptor.desc;
        SANE_Option_Descriptor.r#type;
        SANE_Option_Descriptor.unit;
        SANE_Option_Descriptor.size;
        SANE_Option_Descriptor.cap;
        SANE_Option_Descriptor.constraint_type;
        SANE_Option_Descriptor.constraint;
        SANE_Parameters.format;
        SANE_Parameters.last_frame;
        SANE_Parameters.bytes_per_line;
        SANE_Parameters.pixels_per_line;
        SANE_Parameters.lines;
        SANE_Parameters.depth;
    });
    fields.extend(constraint_members! {
        string_list: ffi::SANE_String_Const;
        word_list: ffi::SANE_Word;
        range: ffi::SANE_Range;
    });
    for field in &mut fields {
        field.field = field.field.trim_start_matches("r#");
    }
    fields
}

/// A function (or the callback's typedef): its name, and its type spelled in
/// C with the header's typedef names, beside the Rust type its binding is
/// checked to have.
struct Function {
    name: &'static str,
    c_type: &'static str,
}

impl Function {
    /// Whether `name` is a typedef of a function pointer, not a function.
    fn is_typedef(&self) -> bool {
        self.name.starts_with("SANE_")
    }
}

macro_rules! functions {
    ($($name:ident: $binding:ident = $rust:ty, $c:literal;)*) => {
        &[$({
            const _: fn(ffi::$binding) -> $rust = same::<ffi::$binding>;
            Function { name: stringify!($name), c_type: $c }
        }),*]
    };
}

const FUNCTIONS: &[Function] = functions! {
    sane_init: SaneInitFn
        = unsafe extern "C" fn(*mut ffi::SANE_Int, ffi::SANE_Auth_Callback) -> ffi::SANE_Status,
        "SANE_Status (*)(SANE_Int *, SANE_Auth_Callback)";
    sane_exit: SaneExitFn = unsafe extern "C" fn(), "void (*)(void)";
    sane_get_devices: SaneGetDevicesFn
        = unsafe extern "C" fn(*mut *mut *const ffi::SANE_Device, ffi::SANE_Bool) -> ffi::SANE_Status,
        "SANE_Status (*)(const SANE_Device ***, SANE_Bool)";
    sane_open: SaneOpenFn
        = unsafe extern "C" fn(ffi::SANE_String_Const, *mut ffi::SANE_Handle) -> ffi::SANE_Status,
        "SANE_Status (*)(SANE_String_Const, SANE_Handle *)";
    sane_close: SaneCloseFn = unsafe extern "C" fn(ffi::SANE_Handle), "void (*)(SANE_Handle)";
    sane_get_option_descriptor: SaneGetOptionDescriptorFn
        = unsafe extern "C" fn(ffi::SANE_Handle, ffi::SANE_Int) -> *const ffi::SANE_Option_Descriptor,
        "const SANE_Option_Descriptor *(*)(SANE_Handle, SANE_Int)";
    sane_control_option: SaneControlOptionFn
        = unsafe extern "C" fn(
            ffi::SANE_Handle, ffi::SANE_Int, ffi::SANE_Action, *mut std::ffi::c_void, *mut ffi::SANE_Int,
        ) -> ffi::SANE_Status,
        "SANE_Status (*)(SANE_Handle, SANE_Int, SANE_Action, void *, SANE_Int *)";
    sane_get_parameters: SaneGetParametersFn
        = unsafe extern "C" fn(ffi::SANE_Handle, *mut ffi::SANE_Parameters) -> ffi::SANE_Status,
        "SANE_Status (*)(SANE_Handle, SANE_Parameters *)";
    sane_start: SaneStartFn = unsafe extern "C" fn(ffi::SANE_Handle) -> ffi::SANE_Status,
        "SANE_Status (*)(SANE_Handle)";
    sane_read: SaneReadFn
        = unsafe extern "C" fn(ffi::SANE_Handle, *mut ffi::SANE_Byte, ffi::SANE_Int, *mut ffi::SANE_Int) -> ffi::SANE_Status,
        "SANE_Status (*)(SANE_Handle, SANE_Byte *, SANE_Int, SANE_Int *)";
    sane_cancel: SaneCancelFn = unsafe extern "C" fn(ffi::SANE_Handle), "void (*)(SANE_Handle)";
    sane_set_io_mode: SaneSetIoModeFn
        = unsafe extern "C" fn(ffi::SANE_Handle, ffi::SANE_Bool) -> ffi::SANE_Status,
        "SANE_Status (*)(SANE_Handle, SANE_Bool)";
    sane_get_select_fd: SaneGetSelectFdFn
        = unsafe extern "C" fn(ffi::SANE_Handle, *mut ffi::SANE_Int) -> ffi::SANE_Status,
        "SANE_Status (*)(SANE_Handle, SANE_Int *)";
    sane_strstatus: SaneStrstatusFn = unsafe extern "C" fn(ffi::SANE_Status) -> ffi::SANE_String_Const,
        "SANE_String_Const (*)(SANE_Status)";
    SANE_Auth_Callback: SANE_Auth_Callback
        = Option<unsafe extern "C" fn(ffi::SANE_String_Const, *mut ffi::SANE_Char, *mut ffi::SANE_Char)>,
        "void (*)(SANE_String_Const, SANE_Char *, SANE_Char *)";
};

/// What a function-like macro gives for some arguments, as the bindings'
/// `const fn` of the same name computes it.
enum Value {
    Int(i64),
    Float(f64),
}

/// The function-like macros' rows: the C expression, and the binding's
/// value for it.
fn macros() -> Vec<(&'static str, &'static str, Value)> {
    use Value::{Float, Int};
    let negative = 0xFF02_FFFF_u32 as i32;
    vec![
        (
            "SANE_VERSION_CODE",
            "SANE_VERSION_CODE(1, 2, 3)",
            Int(ffi::SANE_VERSION_CODE(1, 2, 3).into()),
        ),
        (
            "SANE_VERSION_CODE",
            "SANE_VERSION_CODE(255, 255, 65535)",
            Int(ffi::SANE_VERSION_CODE(255, 255, 65535).into()),
        ),
        (
            "SANE_VERSION_CODE",
            "SANE_VERSION_CODE(1, 300, 70000)",
            Int(ffi::SANE_VERSION_CODE(1, 300, 70000).into()),
        ),
        (
            "SANE_VERSION_MAJOR",
            "SANE_VERSION_MAJOR(0x01040007)",
            Int(ffi::SANE_VERSION_MAJOR(0x0104_0007).into()),
        ),
        (
            "SANE_VERSION_MINOR",
            "SANE_VERSION_MINOR(0x01040007)",
            Int(ffi::SANE_VERSION_MINOR(0x0104_0007).into()),
        ),
        (
            "SANE_VERSION_BUILD",
            "SANE_VERSION_BUILD(0x01040007)",
            Int(ffi::SANE_VERSION_BUILD(0x0104_0007).into()),
        ),
        (
            "SANE_VERSION_MAJOR",
            "SANE_VERSION_MAJOR((SANE_Word)0xFF02FFFF)",
            Int(ffi::SANE_VERSION_MAJOR(negative).into()),
        ),
        (
            "SANE_VERSION_MINOR",
            "SANE_VERSION_MINOR((SANE_Word)0xFF02FFFF)",
            Int(ffi::SANE_VERSION_MINOR(negative).into()),
        ),
        (
            "SANE_VERSION_BUILD",
            "SANE_VERSION_BUILD((SANE_Word)0xFF02FFFF)",
            Int(ffi::SANE_VERSION_BUILD(negative).into()),
        ),
        ("SANE_FIX", "SANE_FIX(1.5)", Int(ffi::SANE_FIX(1.5).into())),
        (
            "SANE_FIX",
            "SANE_FIX(-2.25)",
            Int(ffi::SANE_FIX(-2.25).into()),
        ),
        ("SANE_FIX", "SANE_FIX(0.3)", Int(ffi::SANE_FIX(0.3).into())),
        (
            "SANE_FIX",
            "SANE_FIX(-0.3)",
            Int(ffi::SANE_FIX(-0.3).into()),
        ),
        (
            "SANE_UNFIX",
            "SANE_UNFIX(98304)",
            Float(ffi::SANE_UNFIX(98304)),
        ),
        ("SANE_UNFIX", "SANE_UNFIX(-1)", Float(ffi::SANE_UNFIX(-1))),
        ("SANE_UNFIX", "SANE_UNFIX(50)", Float(ffi::SANE_UNFIX(50))),
        (
            "SANE_OPTION_IS_ACTIVE",
            "SANE_OPTION_IS_ACTIVE(32)",
            Int(ffi::SANE_OPTION_IS_ACTIVE(32).into()),
        ),
        (
            "SANE_OPTION_IS_ACTIVE",
            "SANE_OPTION_IS_ACTIVE(95)",
            Int(ffi::SANE_OPTION_IS_ACTIVE(95).into()),
        ),
        (
            "SANE_OPTION_IS_SETTABLE",
            "SANE_OPTION_IS_SETTABLE(1)",
            Int(ffi::SANE_OPTION_IS_SETTABLE(1).into()),
        ),
        (
            "SANE_OPTION_IS_SETTABLE",
            "SANE_OPTION_IS_SETTABLE(4)",
            Int(ffi::SANE_OPTION_IS_SETTABLE(4).into()),
        ),
    ]
}

/// The header, or why the test can't run here.
fn header() -> Result<PathBuf, String> {
    let path = std::env::var_os("SANE_DL_SANE_H")
        .map_or_else(|| PathBuf::from("/usr/include/sane/sane.h"), PathBuf::from);
    if path.is_file() {
        Ok(path)
    } else {
        Err(format!("there's no sane.h at {}", path.display()))
    }
}

/// The C program that prints what the compiler makes of the header.
fn c_program(header: &Path, fields: &[Field]) -> String {
    let mut c = String::from("#include <stddef.h>\n#include <stdio.h>\n");
    c.push_str(&format!("#include \"{}\"\n\n", header.display()));
    for function in FUNCTIONS {
        let subject = if function.is_typedef() {
            function.name.to_owned()
        } else {
            format!("__typeof__(&{})", function.name)
        };
        c.push_str(&format!(
            "_Static_assert(__builtin_types_compatible_p({subject}, {}), \"{} has the binding's type\");\n",
            function.c_type, function.name
        ));
    }
    c.push_str("\nint main(void) {\n");
    for constant in CONSTANTS.iter().filter(|constant| !constant.reserved) {
        let name = constant.name;
        c.push_str(&format!(
            "  printf(\"const {name} %lld\\n\", (long long)({name}));\n"
        ));
    }
    for ty in TYPES {
        let name = ty.name;
        c.push_str(&format!(
            "  printf(\"size {name} %zu\\n\", sizeof({name}));\n"
        ));
        c.push_str(&format!(
            "  printf(\"align {name} %zu\\n\", (size_t)_Alignof({name}));\n"
        ));
        if ty.signed.is_some() {
            c.push_str(&format!(
                "  printf(\"signed {name} %d\\n\", (int)(({name})-1 < 0));\n"
            ));
        }
    }
    for field in fields {
        let (owner, name) = (field.owner, field.field);
        c.push_str(&format!(
            "  printf(\"offset {owner}.{name} %zu\\n\", offsetof({owner}, {name}));\n"
        ));
        c.push_str(&format!(
            "  printf(\"fieldsize {owner}.{name} %zu\\n\", sizeof((({owner} *)0)->{name}));\n"
        ));
    }
    for (_, expression, value) in macros() {
        match value {
            Value::Int(_) => c.push_str(&format!(
                "  printf(\"macro {expression}=%lld\\n\", (long long)({expression}));\n"
            )),
            Value::Float(_) => c.push_str(&format!(
                "  printf(\"macro {expression}=%.17g\\n\", (double)({expression}));\n"
            )),
        }
    }
    c.push_str("  return 0;\n}\n");
    c
}

/// Compiles and runs the program, giving what it printed, keyed by all but
/// its last word; `Err` if no compiler is here.
fn run_c(program: &str) -> Result<BTreeMap<String, String>, String> {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("header-contract");
    fs::create_dir_all(&dir).unwrap();
    let source = dir.join("contract.c");
    let binary = dir.join(format!("contract{}", std::env::consts::EXE_SUFFIX));
    fs::write(&source, program).unwrap();
    let compiler = std::env::var_os("CC").unwrap_or_else(|| "cc".into());
    let compiled = match Command::new(&compiler)
        .arg("-o")
        .arg(&binary)
        .arg(&source)
        .output()
    {
        Ok(output) => output,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(format!(
                "there's no C compiler here ({} isn't found)",
                compiler.to_string_lossy()
            ));
        }
        Err(error) => panic!("couldn't run {}: {error}", compiler.to_string_lossy()),
    };
    assert!(
        compiled.status.success(),
        "the header's program didn't compile ({}):\n{}",
        source.display(),
        String::from_utf8_lossy(&compiled.stderr)
    );
    let ran = Command::new(&binary)
        .output()
        .expect("couldn't run the header's program");
    assert!(
        ran.status.success(),
        "the header's program failed: {}",
        ran.status
    );
    let mut printed = BTreeMap::new();
    for line in String::from_utf8(ran.stdout).unwrap().lines() {
        let (key, value) = if let Some(rest) = line.strip_prefix("macro ") {
            let (expression, value) = rest.rsplit_once('=').unwrap();
            (format!("macro {expression}"), value)
        } else {
            let (key, value) = line.rsplit_once(' ').unwrap();
            (key.to_owned(), value)
        };
        printed.insert(key, value.to_owned());
    }
    Ok(printed)
}

/// What the header's text declares.
#[derive(Default)]
struct Declared {
    /// Numeric `#define`s and enumerators: their values, and whether under
    /// `#if 0`.
    constants: BTreeMap<String, (i64, bool)>,
    typedefs: BTreeSet<String>,
    functions: BTreeSet<String>,
    macros: BTreeSet<String>,
}

/// A number as the header writes one: decimal, hexadecimal, or `(A << B)`.
fn number(text: &str) -> Option<i64> {
    let text = text.trim();
    if let Some(inner) = text
        .strip_prefix('(')
        .and_then(|text| text.strip_suffix(')'))
    {
        let (left, right) = inner.split_once("<<")?;
        return Some(number(left)? << number(right)?);
    }
    match text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")) {
        Some(hex) => i64::from_str_radix(hex, 16).ok(),
        None => text.parse().ok(),
    }
}

fn strip_comments(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(start) = rest.find("/*") {
        out.push_str(&rest[..start]);
        rest = rest[start..]
            .find("*/")
            .map_or("", |end| &rest[start + end + 2..]);
        out.push(' ');
    }
    out.push_str(rest);
    out
}

/// The header's C code (not its preprocessor lines) as tokens:
/// identifiers and numbers whole, string literals whole, any other
/// character alone.
fn tokens(code: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut chars = code.chars().peekable();
    while let Some(c) = chars.next() {
        if c.is_whitespace() {
            continue;
        }
        let mut token = String::from(c);
        if c.is_ascii_alphanumeric() || c == '_' {
            while let Some(&next) = chars
                .peek()
                .filter(|next| next.is_ascii_alphanumeric() || **next == '_')
            {
                token.push(next);
                chars.next();
            }
        } else if c == '"' {
            for next in chars.by_ref() {
                token.push(next);
                if next == '"' {
                    break;
                }
            }
        }
        tokens.push(token);
    }
    tokens
}

fn is_identifier(token: &str) -> bool {
    token.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_')
}

fn declared(header_text: &str) -> Declared {
    let text = strip_comments(header_text);
    let mut declared = Declared::default();
    // The preprocessor's lines, continuations joined; the rest is code.
    let mut code = String::new();
    // Whether each open condition is `#if 0`.
    let mut conditions: Vec<bool> = Vec::new();
    let mut lines = text.lines();
    while let Some(line) = lines.next() {
        let Some(directive) = line.trim().strip_prefix('#') else {
            code.push_str(line);
            code.push('\n');
            continue;
        };
        let mut directive = directive.trim().to_owned();
        while directive.ends_with('\\') {
            directive.pop();
            directive.push_str(lines.next().unwrap_or(""));
        }
        let reserved = conditions.iter().any(|zero| *zero);
        if directive.starts_with("if") {
            conditions.push(directive.split_whitespace().collect::<Vec<_>>() == ["if", "0"]);
        } else if directive.starts_with("endif") {
            conditions.pop();
        } else if let Some(define) = directive.strip_prefix("define") {
            let define = define.trim_start();
            let name: String = define
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                .collect();
            let after = &define[name.len()..];
            if after.starts_with('(') {
                declared.macros.insert(name);
            } else if let Some(value) = number(after) {
                declared.constants.insert(name, (value, reserved));
            }
        }
    }
    let tokens = tokens(&code);
    let mut index = 0;
    while index < tokens.len() {
        match tokens[index].as_str() {
            "typedef" => {
                // To the `;` that ends it, skipping any braced body, whose
                // enumerators are constants if it's an enum's.
                let is_enum = tokens.get(index + 1).is_some_and(|token| token == "enum");
                let mut depth = 0;
                let mut body: Vec<&str> = Vec::new();
                let mut end = index + 1;
                while end < tokens.len() && !(depth == 0 && tokens[end] == ";") {
                    match tokens[end].as_str() {
                        "{" => depth += 1,
                        "}" => depth -= 1,
                        token if depth > 0 => body.push(token),
                        _ => {}
                    }
                    end += 1;
                }
                let statement = &tokens[index..end];
                // A function pointer's name follows `(*`; any other's is
                // the last identifier.
                let pointer = statement
                    .windows(3)
                    .find(|w| w[0] == "(" && w[1] == "*")
                    .map(|w| w[2].clone());
                let name =
                    pointer.or_else(|| statement.iter().rev().find(|t| is_identifier(t)).cloned());
                declared.typedefs.insert(name.expect("a typedef's name"));
                if is_enum {
                    let mut next = 0;
                    for enumerator in body.split(|token| *token == ",").filter(|e| !e.is_empty()) {
                        let value = match enumerator.iter().position(|token| *token == "=") {
                            Some(at) => number(&enumerator[at + 1..].concat())
                                .expect("an enumerator's value"),
                            None => next,
                        };
                        declared
                            .constants
                            .insert(enumerator[0].to_owned(), (value, false));
                        next = value + 1;
                    }
                }
                index = end;
            }
            "extern" => {
                // A declaration is a function's if a `(` comes before any
                // `;`, `{` or string (`extern "C" {`): its name is just
                // before the `(`.
                let mut end = index + 1;
                while end < tokens.len()
                    && !matches!(tokens[end].as_str(), "(" | ";" | "{")
                    && !tokens[end].starts_with('"')
                {
                    end += 1;
                }
                if tokens.get(end).is_some_and(|token| token == "(") {
                    declared.functions.insert(tokens[end - 1].clone());
                }
                index = end;
            }
            _ => index += 1,
        }
    }
    declared
}

/// A set as a map, for [`assert_same`].
fn as_map(set: &BTreeSet<String>) -> BTreeMap<String, ()> {
    set.iter().map(|name| (name.clone(), ())).collect()
}

/// Fails, naming each difference, unless the bindings' entries are the
/// header's.
fn assert_same<V: PartialEq + std::fmt::Debug>(
    what: &str,
    bound: &BTreeMap<String, V>,
    declared: &BTreeMap<String, V>,
) {
    let mut differences = Vec::new();
    for (name, value) in bound {
        match declared.get(name) {
            None => differences.push(format!(
                "{name}: bound as {value:?}, and the header declares none"
            )),
            Some(declared) if declared != value => differences.push(format!(
                "{name}: bound as {value:?}, and the header declares {declared:?}"
            )),
            Some(_) => {}
        }
    }
    for (name, value) in declared {
        if !bound.contains_key(name) {
            differences.push(format!(
                "{name}: the header declares {value:?}, and nothing binds it"
            ));
        }
    }
    assert!(
        differences.is_empty(),
        "the {what} differ from the header's text:\n{}",
        differences.join("\n")
    );
}

#[test]
fn the_bindings_match_the_installed_header() {
    let test = "the_bindings_match_the_installed_header";
    let header = match header() {
        Ok(header) => header,
        Err(reason) => return common::report_skip(test, &reason, Some(common::REQUIRE_HEADER)),
    };
    let fields = fields();

    // The text: everything it declares is bound, and nothing else is.
    let declared = declared(&fs::read_to_string(&header).unwrap());
    let bound: BTreeMap<String, (i64, bool)> = CONSTANTS
        .iter()
        .map(|constant| {
            (
                constant.name.to_owned(),
                (constant.value, constant.reserved),
            )
        })
        .collect();
    assert_same(
        "constants (value, and whether under #if 0)",
        &bound,
        &declared.constants,
    );
    let typedefs: BTreeSet<String> = TYPES.iter().map(|ty| ty.name.to_owned()).collect();
    assert_same("typedefs", &as_map(&typedefs), &as_map(&declared.typedefs));
    let functions: BTreeSet<String> = FUNCTIONS
        .iter()
        .filter(|function| !function.is_typedef())
        .map(|function| function.name.to_owned())
        .collect();
    assert_eq!(functions.len(), 14);
    assert_same(
        "functions",
        &as_map(&functions),
        &as_map(&declared.functions),
    );
    let macro_names: BTreeSet<String> = macros()
        .iter()
        .map(|(name, _, _)| (*name).to_owned())
        .collect();
    assert_same(
        "function-like macros",
        &as_map(&macro_names),
        &as_map(&declared.macros),
    );

    // The compiler: every value, size, offset and macro as the bindings have
    // them, and every function's type (asserted as it compiled).
    let printed = match run_c(&c_program(&header, &fields)) {
        Ok(printed) => printed,
        Err(reason) => return common::report_skip(test, &reason, Some(common::REQUIRE_HEADER)),
    };
    let mut expected = BTreeMap::new();
    for constant in CONSTANTS.iter().filter(|constant| !constant.reserved) {
        expected.insert(
            format!("const {}", constant.name),
            constant.value.to_string(),
        );
    }
    for ty in TYPES {
        expected.insert(format!("size {}", ty.name), ty.size.to_string());
        expected.insert(format!("align {}", ty.name), ty.align.to_string());
        if let Some(signed) = ty.signed {
            expected.insert(format!("signed {}", ty.name), u8::from(signed).to_string());
        }
    }
    for field in &fields {
        expected.insert(
            format!("offset {}.{}", field.owner, field.field),
            field.offset.to_string(),
        );
        expected.insert(
            format!("fieldsize {}.{}", field.owner, field.field),
            field.size.to_string(),
        );
    }
    let mut floats = Vec::new();
    for (_, expression, value) in macros() {
        match value {
            Value::Int(value) => {
                expected.insert(format!("macro {expression}"), value.to_string());
            }
            Value::Float(value) => floats.push((format!("macro {expression}"), value)),
        }
    }
    for (key, value) in floats {
        let printed_value: f64 = printed[&key].parse().unwrap();
        assert_eq!(
            printed_value.to_bits(),
            value.to_bits(),
            "{key}: C gives {printed_value}, the binding {value}"
        );
        expected.insert(key.clone(), printed[&key].clone());
    }
    let differences: Vec<String> = expected
        .iter()
        .filter(|(key, value)| printed.get(*key) != Some(value))
        .map(|(key, value)| {
            format!(
                "{key}: the bindings say {value}, the header {:?}",
                printed.get(key)
            )
        })
        .collect();
    assert!(
        differences.is_empty(),
        "the bindings differ from {}:\n{}",
        header.display(),
        differences.join("\n")
    );
    assert_eq!(
        printed.len(),
        expected.len(),
        "the program printed what was asked, no more"
    );
}
