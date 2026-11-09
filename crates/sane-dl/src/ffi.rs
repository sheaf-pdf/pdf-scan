// sane - Scanner Access Now Easy.
// Copyright (C) 1997-1999 David Mosberger-Tang and Andreas Beck
// This file is part of the SANE package.
//
// This file is in the public domain.  You may use and modify it as
// you see fit, as long as this copyright message is included and
// that there is an indication as to what modifications have been
// made (if any).
//
// SANE is distributed in the hope that it will be useful, but WITHOUT
// ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or
// FITNESS FOR A PARTICULAR PURPOSE.
//
// This file declares SANE application interface.  See the SANE
// standard for a detailed explanation of the interface.
//
// Modifications: this is a translation of SANE 1.4.0's `sane.h` into Rust
// declarations, for sane-dl, which loads libsane at run time and so links
// none of these functions:
// - the functions are declared as function-pointer types (`SaneInitFn` and
//   the rest), each with the C function's parameters and return type, and
//   kept inside the crate (`pub(crate)`), as the authorization callback's
//   type is;
// - every C enum (`SANE_Status`, `SANE_Value_Type`, `SANE_Unit`,
//   `SANE_Constraint_Type`, `SANE_Action`, `SANE_Frame`) is an integer type
//   with a constant per enumerator, never a Rust enum, since a library may
//   return a value its enum doesn't list;
// - the values the header keeps under `#if 0` for a later SANE (statuses 12
//   and 13, frame types 0x0A to 0x12) are given as constants, marked
//   reserved;
// - the function-like macros (`SANE_VERSION_CODE`, `SANE_FIX` and the rest)
//   are `const fn`s;
// - the option descriptor's anonymous constraint union is named
//   `SANE_Constraint`;
// - the comments are reworded, and the structs' field `type` is spelled
//   `r#type`.

//! SANE's C API as `sane.h` declares it: its types and constants.
//!
//! The raw names, for anyone who wants them, and what the rest of this crate
//! is built on. Nothing here calls anything, and nothing here can be called:
//! the functions' types, as pointers, and the authorization callback's are
//! the crate's own, inside it, since a pointer of one would be `unsafe` to
//! call; the crate looks the functions up in the library it loads. Every C
//! enum is an integer type with constants, because a backend may return a
//! value the enum doesn't list, and a Rust enum holding one would be
//! undefined behaviour. The structs are plain data, as SANE gives them:
//! reading through one of their pointers is the reader's own `unsafe`.
//!
//! Translated from SANE 1.4.0's public-domain `sane.h`, whose notice heads
//! this module's source with what was changed. A test checks these
//! declarations against the installed header: every constant's value, every
//! type's size and alignment, every field's offset, and every function's
//! type.

#![allow(non_camel_case_types, non_snake_case, non_upper_case_globals)]

use core::ffi::{c_char, c_int, c_uchar, c_void};

/// The major version of the SANE standard these declarations follow.
pub const SANE_CURRENT_MAJOR: SANE_Word = 1;
/// The minor version of the SANE standard these declarations follow.
pub const SANE_CURRENT_MINOR: SANE_Word = 0;

/// A version code from its major, minor and build numbers: major in the top
/// byte, minor in the next, build in the low 16 bits.
pub const fn SANE_VERSION_CODE(major: SANE_Word, minor: SANE_Word, build: SANE_Word) -> SANE_Word {
    ((major & 0xff) << 24) | ((minor & 0xff) << 16) | (build & 0xffff)
}

/// A version code's major number.
pub const fn SANE_VERSION_MAJOR(code: SANE_Word) -> SANE_Word {
    (code >> 24) & 0xff
}

/// A version code's minor number.
pub const fn SANE_VERSION_MINOR(code: SANE_Word) -> SANE_Word {
    (code >> 16) & 0xff
}

/// A version code's build number.
pub const fn SANE_VERSION_BUILD(code: SANE_Word) -> SANE_Word {
    code & 0xffff
}

/// False, as a `SANE_Bool`.
pub const SANE_FALSE: SANE_Bool = 0;
/// True, as a `SANE_Bool`.
pub const SANE_TRUE: SANE_Bool = 1;

/// A byte of image data.
pub type SANE_Byte = c_uchar;
/// SANE's word: the type of integers, booleans and fixed-point values.
pub type SANE_Word = c_int;
/// A boolean, which travels as a whole word: [`SANE_FALSE`] or [`SANE_TRUE`].
pub type SANE_Bool = SANE_Word;
/// An integer.
pub type SANE_Int = SANE_Word;
/// A character of a string.
pub type SANE_Char = c_char;
/// A string the callee may write to.
pub type SANE_String = *mut SANE_Char;
/// A string the callee only reads.
pub type SANE_String_Const = *const SANE_Char;
/// An open device, opaque.
pub type SANE_Handle = *mut c_void;
/// A fixed-point number: a word whose low 16 bits are the fraction.
pub type SANE_Fixed = SANE_Word;

/// How far a fixed-point value's integer part is shifted.
pub const SANE_FIXED_SCALE_SHIFT: SANE_Word = 16;

/// A number as a fixed-point word, truncated toward zero.
pub const fn SANE_FIX(v: f64) -> SANE_Word {
    (v * (1 << SANE_FIXED_SCALE_SHIFT) as f64) as SANE_Word
}

/// A fixed-point word as a number.
pub const fn SANE_UNFIX(v: SANE_Word) -> f64 {
    v as f64 / (1 << SANE_FIXED_SCALE_SHIFT) as f64
}

/// A status: what a call says of how it went.
pub type SANE_Status = c_int;
/// Everything went well.
pub const SANE_STATUS_GOOD: SANE_Status = 0;
/// The operation isn't supported.
pub const SANE_STATUS_UNSUPPORTED: SANE_Status = 1;
/// The operation was cancelled.
pub const SANE_STATUS_CANCELLED: SANE_Status = 2;
/// The device is busy; try again later.
pub const SANE_STATUS_DEVICE_BUSY: SANE_Status = 3;
/// The data is invalid, which includes no such device when opening one.
pub const SANE_STATUS_INVAL: SANE_Status = 4;
/// No more data: the end of a frame.
pub const SANE_STATUS_EOF: SANE_Status = 5;
/// The document feeder jammed.
pub const SANE_STATUS_JAMMED: SANE_Status = 6;
/// The document feeder is out of documents.
pub const SANE_STATUS_NO_DOCS: SANE_Status = 7;
/// The scanner's cover is open.
pub const SANE_STATUS_COVER_OPEN: SANE_Status = 8;
/// An error in the device's input or output.
pub const SANE_STATUS_IO_ERROR: SANE_Status = 9;
/// Out of memory.
pub const SANE_STATUS_NO_MEM: SANE_Status = 10;
/// Access to a resource was denied.
pub const SANE_STATUS_ACCESS_DENIED: SANE_Status = 11;
/// Reserved for a later SANE: the lamp isn't ready, so try again.
pub const SANE_STATUS_WARMING_UP: SANE_Status = 12;
/// Reserved for a later SANE: the scanner's mechanism is locked for
/// transport.
pub const SANE_STATUS_HW_LOCKED: SANE_Status = 13;

/// How an option's value is read.
pub type SANE_Value_Type = c_int;
/// A boolean.
pub const SANE_TYPE_BOOL: SANE_Value_Type = 0;
/// One or more integers.
pub const SANE_TYPE_INT: SANE_Value_Type = 1;
/// One or more fixed-point numbers.
pub const SANE_TYPE_FIXED: SANE_Value_Type = 2;
/// A string.
pub const SANE_TYPE_STRING: SANE_Value_Type = 3;
/// A button, which has no value.
pub const SANE_TYPE_BUTTON: SANE_Value_Type = 4;
/// The heading of a group of options, which has no value.
pub const SANE_TYPE_GROUP: SANE_Value_Type = 5;

/// The physical unit of an option's value.
pub type SANE_Unit = c_int;
/// No unit, such as a count.
pub const SANE_UNIT_NONE: SANE_Unit = 0;
/// Pixels.
pub const SANE_UNIT_PIXEL: SANE_Unit = 1;
/// Bits.
pub const SANE_UNIT_BIT: SANE_Unit = 2;
/// Millimetres.
pub const SANE_UNIT_MM: SANE_Unit = 3;
/// Resolution, in dots per inch.
pub const SANE_UNIT_DPI: SANE_Unit = 4;
/// A percentage.
pub const SANE_UNIT_PERCENT: SANE_Unit = 5;
/// Microseconds.
pub const SANE_UNIT_MICROSECOND: SANE_Unit = 6;

/// A device, as listed by `sane_get_devices`.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct SANE_Device {
    /// Its name, unique among the devices listed.
    pub name: SANE_String_Const,
    /// Its vendor.
    pub vendor: SANE_String_Const,
    /// Its model.
    pub model: SANE_String_Const,
    /// What kind of device it is, such as "flatbed scanner".
    pub r#type: SANE_String_Const,
}

/// The option's value can be set by software.
pub const SANE_CAP_SOFT_SELECT: SANE_Int = 1 << 0;
/// The option's value is set by the user on the device itself.
pub const SANE_CAP_HARD_SELECT: SANE_Int = 1 << 1;
/// The option's value can be read by software.
pub const SANE_CAP_SOFT_DETECT: SANE_Int = 1 << 2;
/// The option is emulated by the backend, not done by the device.
pub const SANE_CAP_EMULATED: SANE_Int = 1 << 3;
/// The backend can choose the option's value itself.
pub const SANE_CAP_AUTOMATIC: SANE_Int = 1 << 4;
/// The option is inactive for now.
pub const SANE_CAP_INACTIVE: SANE_Int = 1 << 5;
/// The option is for advanced users.
pub const SANE_CAP_ADVANCED: SANE_Int = 1 << 6;

/// Whether capabilities `cap` make an option active.
pub const fn SANE_OPTION_IS_ACTIVE(cap: SANE_Int) -> bool {
    (cap & SANE_CAP_INACTIVE) == 0
}

/// Whether capabilities `cap` let software set an option.
pub const fn SANE_OPTION_IS_SETTABLE(cap: SANE_Int) -> bool {
    (cap & SANE_CAP_SOFT_SELECT) != 0
}

/// The value set wasn't exactly the one given.
pub const SANE_INFO_INEXACT: SANE_Int = 1 << 0;
/// Setting the option changed others' descriptors: read them again.
pub const SANE_INFO_RELOAD_OPTIONS: SANE_Int = 1 << 1;
/// Setting the option changed the scan's parameters.
pub const SANE_INFO_RELOAD_PARAMS: SANE_Int = 1 << 2;

/// How an option's values are constrained.
pub type SANE_Constraint_Type = c_int;
/// Not constrained.
pub const SANE_CONSTRAINT_NONE: SANE_Constraint_Type = 0;
/// Constrained to a range.
pub const SANE_CONSTRAINT_RANGE: SANE_Constraint_Type = 1;
/// Constrained to a list of words.
pub const SANE_CONSTRAINT_WORD_LIST: SANE_Constraint_Type = 2;
/// Constrained to a list of strings.
pub const SANE_CONSTRAINT_STRING_LIST: SANE_Constraint_Type = 3;

/// A range of values: from `min` to `max`, both included, in steps of
/// `quant` (0 for any step).
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SANE_Range {
    /// The least value.
    pub min: SANE_Word,
    /// The greatest value.
    pub max: SANE_Word,
    /// The step between values, or 0 for none.
    pub quant: SANE_Word,
}

/// The constraint of an option descriptor: which member holds is said by
/// its `constraint_type`.
#[repr(C)]
#[derive(Clone, Copy)]
pub union SANE_Constraint {
    /// A list of strings, ended by a null pointer.
    pub string_list: *const SANE_String_Const,
    /// A list of words, its length in its first word.
    pub word_list: *const SANE_Word,
    /// A range.
    pub range: *const SANE_Range,
}

/// What an option is: its names, how its value is read, and its
/// constraint.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct SANE_Option_Descriptor {
    /// Its name, as a command line spells it.
    pub name: SANE_String_Const,
    /// Its title, one line.
    pub title: SANE_String_Const,
    /// Its description, any number of lines.
    pub desc: SANE_String_Const,
    /// How its value is read.
    pub r#type: SANE_Value_Type,
    /// The value's physical unit.
    pub unit: SANE_Unit,
    /// The value's size in bytes.
    pub size: SANE_Int,
    /// Its capabilities, the `SANE_CAP_*` bits.
    pub cap: SANE_Int,
    /// Which member of `constraint` holds, if any.
    pub constraint_type: SANE_Constraint_Type,
    /// The constraint.
    pub constraint: SANE_Constraint,
}

/// What `sane_control_option` is asked to do.
pub type SANE_Action = c_int;
/// Read the option's value.
pub const SANE_ACTION_GET_VALUE: SANE_Action = 0;
/// Set the option's value.
pub const SANE_ACTION_SET_VALUE: SANE_Action = 1;
/// Let the backend choose the option's value.
pub const SANE_ACTION_SET_AUTO: SANE_Action = 2;

/// What a frame holds.
pub type SANE_Frame = c_int;
/// Gray: one band covering what the eye sees.
pub const SANE_FRAME_GRAY: SANE_Frame = 0;
/// Red, green and blue, interleaved pixel by pixel.
pub const SANE_FRAME_RGB: SANE_Frame = 1;
/// The red band alone.
pub const SANE_FRAME_RED: SANE_Frame = 2;
/// The green band alone.
pub const SANE_FRAME_GREEN: SANE_Frame = 3;
/// The blue band alone.
pub const SANE_FRAME_BLUE: SANE_Frame = 4;
/// Reserved for a later SANE: text, specific to the backend.
pub const SANE_FRAME_TEXT: SANE_Frame = 0x0A;
/// Reserved for a later SANE: a complete baseline JPEG file.
pub const SANE_FRAME_JPEG: SANE_Frame = 0x0B;
/// Reserved for a later SANE: CCITT Group 3, one-dimensional.
pub const SANE_FRAME_G31D: SANE_Frame = 0x0C;
/// Reserved for a later SANE: CCITT Group 3, two-dimensional.
pub const SANE_FRAME_G32D: SANE_Frame = 0x0D;
/// Reserved for a later SANE: CCITT Group 4.
pub const SANE_FRAME_G42D: SANE_Frame = 0x0E;
/// Reserved for a later SANE: an infrared band alone.
pub const SANE_FRAME_IR: SANE_Frame = 0x0F;
/// Reserved for a later SANE: red, green, blue and infrared.
pub const SANE_FRAME_RGBI: SANE_Frame = 0x10;
/// Reserved for a later SANE: gray and infrared.
pub const SANE_FRAME_GRAYI: SANE_Frame = 0x11;
/// Reserved for a later SANE: XML of no set schema.
pub const SANE_FRAME_XML: SANE_Frame = 0x12;

/// A frame's parameters: what it holds and its size.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SANE_Parameters {
    /// What the frame holds.
    pub format: SANE_Frame,
    /// Whether this frame is the image's last.
    pub last_frame: SANE_Bool,
    /// Bytes in each line, padding included.
    pub bytes_per_line: SANE_Int,
    /// Pixels in each line.
    pub pixels_per_line: SANE_Int,
    /// Lines in the frame, or -1 if not known in advance.
    pub lines: SANE_Int,
    /// Bits per sample.
    pub depth: SANE_Int,
}

/// The longest user name an authorization callback may write, in bytes.
pub const SANE_MAX_USERNAME_LEN: SANE_Int = 128;
/// The longest password an authorization callback may write, in bytes.
pub const SANE_MAX_PASSWORD_LEN: SANE_Int = 128;

/// A callback a backend calls to ask for a user name and password for
/// `resource`, written into the two buffers.
pub(crate) type SANE_Auth_Callback = Option<
    unsafe extern "C" fn(
        resource: SANE_String_Const,
        username: *mut SANE_Char,
        password: *mut SANE_Char,
    ),
>;

/// `sane_init`: starts SANE, giving its version code.
pub(crate) type SaneInitFn =
    unsafe extern "C" fn(version_code: *mut SANE_Int, authorize: SANE_Auth_Callback) -> SANE_Status;
/// `sane_exit`: ends SANE.
pub(crate) type SaneExitFn = unsafe extern "C" fn();
/// `sane_get_devices`: lists the devices.
pub(crate) type SaneGetDevicesFn = unsafe extern "C" fn(
    device_list: *mut *mut *const SANE_Device,
    local_only: SANE_Bool,
) -> SANE_Status;
/// `sane_open`: opens a device by its name.
pub(crate) type SaneOpenFn =
    unsafe extern "C" fn(devicename: SANE_String_Const, handle: *mut SANE_Handle) -> SANE_Status;
/// `sane_close`: closes a device.
pub(crate) type SaneCloseFn = unsafe extern "C" fn(handle: SANE_Handle);
/// `sane_get_option_descriptor`: an option's descriptor, or null.
pub(crate) type SaneGetOptionDescriptorFn =
    unsafe extern "C" fn(handle: SANE_Handle, option: SANE_Int) -> *const SANE_Option_Descriptor;
/// `sane_control_option`: reads or sets an option's value.
pub(crate) type SaneControlOptionFn = unsafe extern "C" fn(
    handle: SANE_Handle,
    option: SANE_Int,
    action: SANE_Action,
    value: *mut c_void,
    info: *mut SANE_Int,
) -> SANE_Status;
/// `sane_get_parameters`: the parameters of the frame to come, or of the one
/// being read.
pub(crate) type SaneGetParametersFn =
    unsafe extern "C" fn(handle: SANE_Handle, params: *mut SANE_Parameters) -> SANE_Status;
/// `sane_start`: starts a frame.
pub(crate) type SaneStartFn = unsafe extern "C" fn(handle: SANE_Handle) -> SANE_Status;
/// `sane_read`: reads a frame's data.
pub(crate) type SaneReadFn = unsafe extern "C" fn(
    handle: SANE_Handle,
    data: *mut SANE_Byte,
    max_length: SANE_Int,
    length: *mut SANE_Int,
) -> SANE_Status;
/// `sane_cancel`: cancels what's in progress, from any thread.
pub(crate) type SaneCancelFn = unsafe extern "C" fn(handle: SANE_Handle);
/// `sane_set_io_mode`: reads blocking or not.
pub(crate) type SaneSetIoModeFn =
    unsafe extern "C" fn(handle: SANE_Handle, non_blocking: SANE_Bool) -> SANE_Status;
/// `sane_get_select_fd`: a file descriptor that's readable when data is.
pub(crate) type SaneGetSelectFdFn =
    unsafe extern "C" fn(handle: SANE_Handle, fd: *mut SANE_Int) -> SANE_Status;
/// `sane_strstatus`: a status as text.
pub(crate) type SaneStrstatusFn = unsafe extern "C" fn(status: SANE_Status) -> SANE_String_Const;
