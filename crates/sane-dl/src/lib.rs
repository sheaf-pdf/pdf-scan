//! SANE scanner access for Rust, with libsane loaded at run time.
//!
//! [SANE](http://www.sane-project.org/) is the scanner API of Linux and the
//! other Unix-like systems. This crate opens `libsane` only when asked to,
//! never at build time or start-up, so a program that takes it builds and
//! runs where SANE isn't installed, and hears so as a typed error when it
//! tries to scan. It keeps its own bindings ([`ffi`]), written from SANE's
//! public-domain `sane.h`, and checks every integer, pointer and length that
//! crosses from C before it becomes a Rust value.
//!
//! ```no_run
//! use std::time::Duration;
//!
//! let sane = sane_dl::Sane::load()?;
//! for device in sane.devices(false, Duration::from_secs(30))? {
//!     println!("{} ({} {})", device.name, device.vendor, device.model);
//! }
//! let mut handle = sane.open("test:0")?;
//! if let Some(resolution) = handle.find_option("resolution") {
//!     let resolution = resolution.index;
//!     println!("resolution: {:?}", handle.get(resolution)?);
//! }
//! handle.close();
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! - **Loading:** [`Sane::load`] opens the system's library by its
//!   versioned name (`libsane.so.1`, or `libsane.1.dylib` on macOS; there's
//!   none on Windows), and [`Sane::load_from`] one the caller names. Then
//!   `sane_init`, and a version other than SANE 1 is refused. One [`Sane`]
//!   per process, since SANE is started once per process; the library is
//!   never unloaded.
//! - **Calls never overlap:** every call through a [`Sane`] or its handles
//!   takes one lock, as SANE requires of every function but `sane_cancel`.
//! - **Devices under a deadline:** [`Sane::devices`] lists them on a thread
//!   of its own and stops waiting at the deadline. A listing belongs in a
//!   process of its own, since it and a scan wait on each other.
//! - **A known device, fast:** [`ConfigDir`] writes a configuration that
//!   loads only that device's backend ([`backend_of`]), for the process
//!   that loads SANE to be given as `SANE_CONFIG_DIR`, and [`Sane::open`]
//!   opens a device by its name without a list.
//! - **Options, typed:** a device's options are read as it opens
//!   ([`Handle::options`]): each descriptor's types, unit, size,
//!   capabilities and constraint ([`OptionDescriptor`]), and read again
//!   whenever SANE says they changed. [`Handle::get`], [`Handle::set`] and
//!   [`Handle::set_auto`] take and give a [`Value`], and refuse a call
//!   SANE's rules don't allow before the backend sees it.
//! - **What crosses from C is checked:** statuses and frame formats are
//!   integers ([`Status`], [`FrameFormat`]) that keep codes SANE doesn't
//!   define; null pointers, lists and strings are refused or capped
//!   ([`Limits`]) before they become values; a descriptor holding what SANE
//!   doesn't allow makes its option unusable, and a constraint is read only
//!   as the member its type allows.
//!
//! **Run SANE in a process of its own.** Backends are C code running in the
//! process that loads them; some corrupt memory and some crash, and nothing
//! this crate checks can stop that.
//!
//! Planned: frames assembled into images; a cancel token any thread can
//! use.

// Only the module that loads and calls the library may hold unsafe code.
#![deny(unsafe_code)]

mod config;
mod device;
mod error;
pub mod ffi;
mod library;
mod options;
mod status;

pub use config::{CONFIG_DIR_VARIABLE, ConfigDir, ConfigError, Search, backend_of};
pub use device::{Device, DeviceName};
pub use error::{Call, Error, LoadError};
pub use library::{Handle, Sane};
pub use options::{
    Capabilities, Constraint, Fixed, OptionDescriptor, SaneString, SetInfo, Unit, Value, ValueType,
};
pub use status::{FrameFormat, Status, Version};

/// Caps on what this crate accepts from SANE, each checked before the work
/// it caps.
///
/// A backend is C code that may return anything: these keep a malformed or
/// hostile answer from becoming an unbounded allocation or loop. The
/// defaults are far above what any real backend gives. Set on a [`Sane`]
/// with [`Sane::set_limits`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Limits {
    /// Devices in a list (default 1,024).
    pub devices: usize,
    /// Bytes in a string SANE gives, before its `NUL`: a device's name,
    /// vendor, model and type, a status's words, an option's name, title and
    /// description, an entry of a string list (default 64 KiB).
    pub string_bytes: usize,
    /// Options a device has, option 0 included (default 4,096).
    pub options: usize,
    /// Entries in an option's word list or string list (default 65,536).
    pub list_entries: usize,
    /// Bytes in an option's value, as its size gives them (default 1 MiB).
    pub option_bytes: usize,
}

impl Default for Limits {
    fn default() -> Limits {
        Limits {
            devices: 1024,
            string_bytes: 64 * 1024,
            options: 4096,
            list_entries: 65_536,
            option_bytes: 1024 * 1024,
        }
    }
}
