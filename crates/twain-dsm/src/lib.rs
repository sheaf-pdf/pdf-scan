//! TWAIN for Rust, through the system's Data Source Manager on Windows.
//!
//! [TWAIN](https://twain.org/) is the scanner API of Windows (and of macOS,
//! which this crate doesn't cover): an application talks to the Data Source
//! Manager (`TWAINDSM.dll`), which loads each scanner's driver, a data source,
//! and passes the application's calls to it. This crate's API is safe Rust:
//! TWAIN's states are types the compiler checks, its containers and blocks
//! are decoded into owned values with every length checked first, and the
//! `unsafe` code that calls the DSM stays inside.
//!
//! Planned: the DSM loaded from the system folder after its signature is
//! checked, and never unloaded; a TWAIN thread the caller scopes, with a
//! hidden window and a message loop; sources listed and opened, their
//! capabilities read and set; native and memory transfers, with the extended
//! image info a source offers; a handle other threads post requests to.
//!
//! This version is the crate's first cut: TWAIN's definitions ([`ffi`]),
//! checked against `twain.h`; its return and condition codes, named; and its
//! containers, decoded and encoded ([`Container`]).
//!
//! Its TWAIN definitions are generated from `TWAINH.cs` in the TWAIN Working
//! Group's twain-cs, under Kodak Alaris's MIT notice, which the generated file
//! keeps; and, as the TWAIN License asks of software based on the TWAIN
//! Toolkit:
//!
//! > The TWAIN Toolkit is distributed as is. The developer and distributors of
//! > the TWAIN Toolkit expressly disclaim all implied, express or statutory
//! > warranties including, without limitation, the implied warranties of
//! > merchantability, noninfringement of third party rights and fitness for a
//! > particular purpose. Neither the developers nor the distributors will be
//! > liable for damages, whether direct, indirect, special, incidental, or
//! > consequential, as a result of the reproduction, modification,
//! > distribution or other use of the TWAIN Toolkit.

// Only the modules that call Windows or the DSM may hold unsafe code.
#![deny(unsafe_code)]

mod codes;
// Pure, so tested everywhere; off Windows, nothing calls them but the tests.
#[cfg_attr(not(windows), allow(dead_code))]
mod container;
#[cfg_attr(not(windows), allow(dead_code))]
mod ext_info;
pub mod ffi;
#[cfg_attr(not(windows), allow(dead_code))]
mod pe;

#[cfg(windows)]
#[allow(unsafe_code)]
mod dsm;
#[cfg(windows)]
mod enabled;
#[cfg(windows)]
mod error;
#[cfg(windows)]
mod states;
#[cfg(windows)]
mod thread;
#[cfg(windows)]
#[allow(unsafe_code)]
mod window;

#[cfg(all(test, windows))]
mod header_contract;
#[cfg(all(test, windows))]
mod test_support;

pub use codes::{ConditionCode, ReturnCode, Triplet};
pub use container::{
    Container, ContainerError, Enumeration, Fix32, Frame, Item, ItemType, Items, Range,
};
#[cfg(windows)]
pub use dsm::Dib;
#[cfg(windows)]
pub use enabled::{
    Enabled, Event, MemoryTransfer, Native, Next, Pending, Ready, Strip, Transferred, Ui,
};
#[cfg(windows)]
pub use error::{Error, Gone, LoadError, Refused, RunError};
pub use ext_info::{ExtInfo, ExtValue};
#[cfg(windows)]
pub use states::{App, Delivery, Load, Loaded, Opened, SetOutcome, Source, SourceId};
#[cfg(windows)]
pub use thread::{Remote, RunReport, Twain, Wake, run};
