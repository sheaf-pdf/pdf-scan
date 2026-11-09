//! What can go wrong, loading SANE and calling it.

use std::path::PathBuf;

use crate::{Status, Version};

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
}
