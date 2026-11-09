//! What can go wrong, loading SANE and calling it.

use std::path::PathBuf;

use crate::{Parameters, Status, ValueType, Version};

/// A source error, kept without naming the type of the crate it came from.
type Source = Box<dyn std::error::Error + Send + Sync + 'static>;

/// Why SANE couldn't be loaded.
///
/// A load that fails leaves nothing behind: the process's one slot is free
/// again, and SANE, if it was started, has been ended.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum LoadError {
    /// No library could be opened by the names tried: missing, or not one
    /// this process can load (`source` says which, for the last one).
    #[error("couldn't open a SANE library (tried {})", list_paths(tried))]
    NotFound {
        /// The names or paths tried, in order.
        tried: Vec<PathBuf>,
        /// Why the last one couldn't be opened.
        #[source]
        source: Source,
    },
    /// This system has no default SANE library (Windows): SANE isn't its
    /// scanner API. [`Sane::load_from`](crate::Sane::load_from) still loads
    /// one by its path.
    #[error("there is no default SANE library on this system")]
    NoDefault,
    /// The library lacks one of SANE's functions, so it isn't SANE's.
    #[error("the library has no `{name}`, so it isn't SANE")]
    MissingSymbol {
        /// The function missing.
        name: &'static str,
        /// What the system's loader said.
        #[source]
        source: Source,
    },
    /// `sane_init` returned a status other than `GOOD`.
    #[error("sane_init returned {0}")]
    Init(Status),
    /// The library follows a SANE other than version 1. It was ended
    /// (`sane_exit`) before this was returned.
    #[error("the library follows SANE {0}, and only SANE 1 is supported")]
    UnsupportedVersion(Version),
    /// SANE is already loaded in this process: one [`Sane`](crate::Sane) at
    /// a time, since SANE is started once per process.
    #[error("SANE is already loaded in this process")]
    AlreadyLoaded,
}

fn list_paths(paths: &[PathBuf]) -> String {
    let names: Vec<String> = paths
        .iter()
        .map(|path| path.display().to_string())
        .collect();
    names.join(", ")
}

/// The SANE functions that return a status, to say which one answered.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Call {
    /// `sane_get_devices`.
    GetDevices,
    /// `sane_open`.
    Open,
    /// `sane_control_option`.
    ControlOption,
    /// `sane_get_parameters`.
    GetParameters,
    /// `sane_start`.
    Start,
    /// `sane_read`.
    Read,
    /// `sane_set_io_mode`.
    SetIoMode,
    /// `sane_get_select_fd`.
    GetSelectFd,
}

impl Call {
    /// The C function's name, such as `sane_open`.
    pub fn name(self) -> &'static str {
        match self {
            Call::GetDevices => "sane_get_devices",
            Call::Open => "sane_open",
            Call::ControlOption => "sane_control_option",
            Call::GetParameters => "sane_get_parameters",
            Call::Start => "sane_start",
            Call::Read => "sane_read",
            Call::SetIoMode => "sane_set_io_mode",
            Call::GetSelectFd => "sane_get_select_fd",
        }
    }
}

impl std::fmt::Display for Call {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}

/// What went wrong calling SANE, or what SANE gave that couldn't be
/// trusted.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// A SANE function returned a status other than `GOOD`, kept as it came
    /// with the function that returned it: `INVAL` from `sane_open` means
    /// there's no such device, from elsewhere something else.
    #[error("{call} returned {status}")]
    Status {
        /// The status.
        status: Status,
        /// The function that returned it.
        call: Call,
    },
    /// A device listing that ran past its deadline still holds the library,
    /// and every call but a handle's close waits on it. Calls say so rather
    /// than block until it returns, which may be never.
    #[error("a device listing that ran past its deadline still holds SANE")]
    ListingPending,
    /// The deadline passed before the call could finish (a listing still
    /// running holds the library: see [`ListingPending`](Self::ListingPending))
    /// or start.
    #[error("the deadline passed")]
    DeadlineExceeded,
    /// The device list SANE gave can't be read: null, a device with a null
    /// name, more devices than the limit, or a string longer than its limit.
    #[error("the device list can't be read: {reason}")]
    BadDeviceList {
        /// What's wrong with it.
        reason: String,
    },
    /// `sane_open` returned `GOOD` with a null handle.
    #[error("sane_open returned GOOD but no handle")]
    BadHandle,
    /// A device name holds a NUL byte, so it can't go to C.
    #[error("the device name holds a NUL byte")]
    NulInName,
    /// The thread a listing runs on couldn't be started.
    #[error("couldn't start the listing's thread")]
    Thread(#[source] std::io::Error),
    /// An option's descriptor can't stand for an option at all, so the
    /// device's options can't be read: option 0 isn't the count, the count
    /// is below 1 or past [`Limits::options`](crate::Limits::options), a
    /// descriptor below the count is null, or a name is longer than
    /// [`Limits::string_bytes`](crate::Limits::string_bytes). A descriptor
    /// that only holds what SANE doesn't allow makes its option unusable
    /// instead ([`OptionDescriptor::unusable`](crate::OptionDescriptor::unusable)).
    #[error("option {option}'s descriptor can't be read: {reason}")]
    BadDescriptor {
        /// The option.
        option: usize,
        /// What's wrong with it.
        reason: String,
    },
    /// The value a backend gave can't be trusted: a `BOOL` word that's
    /// neither 0 nor 1, or a string with no `NUL` within the option's size.
    #[error("option {option}'s value can't be read: {reason}")]
    BadValue {
        /// The option.
        option: usize,
        /// What's wrong with it.
        reason: String,
    },
    /// No option has this number.
    #[error("there is no option {option}")]
    NoSuchOption {
        /// The number asked for.
        option: usize,
    },
    /// The option is a group's heading, or a button (which can be set, not
    /// read): it has no value.
    #[error("option {option} is a {value_type}, which has no value")]
    NoValue {
        /// The option.
        option: usize,
        /// Its type.
        value_type: ValueType,
    },
    /// The option is inactive: another option's value makes it mean
    /// nothing for now.
    #[error("option {option} is inactive")]
    Inactive {
        /// The option.
        option: usize,
    },
    /// Software can't read the option's value (it lacks `SANE_CAP_SOFT_DETECT`).
    #[error("option {option} can't be read by software")]
    NotReadable {
        /// The option.
        option: usize,
    },
    /// Software can't set the option (it lacks `SANE_CAP_SOFT_SELECT`): it
    /// may be set on the device itself, or only read.
    #[error("option {option} can't be set by software")]
    NotSettable {
        /// The option.
        option: usize,
    },
    /// The backend can't choose the option's value itself (it lacks
    /// `SANE_CAP_AUTOMATIC`).
    #[error("option {option} can't be set automatically")]
    NotAutomatic {
        /// The option.
        option: usize,
    },
    /// The value given is of another type than the option's.
    #[error("option {option} takes {value_type} values, not {given}")]
    WrongType {
        /// The option.
        option: usize,
        /// The option's type.
        value_type: ValueType,
        /// The value's.
        given: ValueType,
    },
    /// The value given holds another number of words than the option's
    /// size.
    #[error("option {option} takes {expected} values, not {given}")]
    WrongLength {
        /// The option.
        option: usize,
        /// How many the option takes.
        expected: usize,
        /// How many were given.
        given: usize,
    },
    /// The string given, with its `NUL`, doesn't fit the option's size.
    #[error("option {option} takes strings shorter than {size} bytes, not {given}")]
    StringTooLong {
        /// The option.
        option: usize,
        /// The option's size, its `NUL` included.
        size: usize,
        /// The string's length.
        given: usize,
    },
    /// The string given holds a `NUL` byte, which would end it early.
    #[error("the string for option {option} holds a NUL byte")]
    NulInString {
        /// The option.
        option: usize,
    },
    /// The option's descriptor holds what SANE doesn't allow, so it can't be
    /// read or set ([`OptionDescriptor::unusable`](crate::OptionDescriptor::unusable)
    /// says why).
    #[error("option {option} can't be used: {reason}")]
    Unusable {
        /// The option.
        option: usize,
        /// Why.
        reason: String,
    },
    /// `sane_start` answered `INVAL`: the scan can't start with the options
    /// as they're set. The standard says the device's options may have
    /// changed, so every descriptor was read again before this was
    /// returned ([`Handle::options`](crate::Handle::options)); `reload` is
    /// why that failed, if it did, leaving no descriptors.
    #[error("sane_start returned INVAL: the scan can't start with the options as set{}", match reload {
        None => "; the descriptors were read again",
        Some(_) => ", and the descriptors couldn't be read again",
    })]
    StartInvalid {
        /// Why the descriptors couldn't be read again, if they couldn't.
        #[source]
        reload: Option<Box<Error>>,
    },
    /// `sane_read` said it gave fewer than 0 bytes, or more than it was
    /// asked for: not trusted, and the scan was cancelled.
    #[error("sane_read said it gave {length} bytes, asked for at most {asked}")]
    BadReadLength {
        /// The length SANE gave.
        length: i32,
        /// The length asked for.
        asked: i32,
    },
    /// A frame's parameters can't stand for a frame, alone or among the
    /// page's frames ([`Parameters::check`](crate::Parameters::check)): the
    /// page was dropped and the scan cancelled.
    #[error("the frame's parameters make no sense: {reason} ({parameters})")]
    BadParameters {
        /// The parameters, as SANE gave them.
        parameters: Parameters,
        /// What's wrong with them.
        reason: String,
    },
    /// The frame ended (`EOF`) before the bytes its parameters promised.
    #[error("the frame ended after {read} of its {expected} bytes")]
    ShortFrame {
        /// The bytes its parameters promised, padding included.
        expected: u64,
        /// The bytes read.
        read: u64,
    },
    /// The frame went on past the bytes its parameters promised; reading
    /// stopped there.
    #[error("the frame ran past its {expected} bytes")]
    LongFrame {
        /// The bytes its parameters promised, padding included.
        expected: u64,
    },
    /// A frame of unknown height ended partway into a row.
    #[error(
        "the frame ended {extra} bytes into a row, after {rows} rows of {bytes_per_line} bytes"
    )]
    PartialRow {
        /// The whole rows read.
        rows: u64,
        /// The bytes of the row it ended in.
        extra: usize,
        /// The bytes in a row, padding included.
        bytes_per_line: usize,
    },
    /// A frame of unknown height ran past
    /// [`Limits::frame_bytes`](crate::Limits::frame_bytes); reading stopped
    /// there.
    #[error("the frame ran past {limit} bytes, the limit")]
    FrameTooLarge {
        /// The limit.
        limit: usize,
    },
    /// The scan was cancelled through a [`CancelToken`](crate::CancelToken).
    #[error("the scan was cancelled")]
    Cancelled,
    /// `sane_get_select_fd` answered `GOOD` with a negative descriptor.
    #[error("sane_get_select_fd gave {fd}, which isn't a file descriptor")]
    BadSelectFd {
        /// What it gave.
        fd: i32,
    },
}
