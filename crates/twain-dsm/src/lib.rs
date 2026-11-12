//! TWAIN for Rust, through the system's Data Source Manager on Windows.
//!
//! [TWAIN](https://twain.org/) is the scanner API of Windows (and of macOS,
//! which this crate doesn't cover): an application talks to the Data Source
//! Manager (`TWAINDSM.dll`), which loads each scanner's driver, a data source,
//! and passes the application's calls to it. This crate's API is safe Rust:
//! TWAIN's states are types the compiler checks, its containers and blocks
//! are decoded into owned values with every length checked first, and the
//! `unsafe` code that calls the DSM stays inside.

#![cfg_attr(
    windows,
    doc = r#"
On Windows, [`run`] makes the calling thread the TWAIN thread for a
closure: a single-threaded apartment, a hidden window the DSM and drivers
are given as their parent, and a message loop. Inside it the DSM is loaded
([`Twain::load`]) from the system folder, or a path, only after its file's
machine, signature and signer (the TWAIN Working Group) are checked, and
is never unloaded; then each of TWAIN's states is a value that owns the
one beneath it: [`Loaded`], [`Opened`], [`Source`], [`Enabled`], [`Ready`],
[`Transferred`] and [`MemoryTransfer`]. A step up consumes the state, and
hands it back in a [`Refused`] when TWAIN refuses; dropping one steps the
source down from wherever it really is. None can leave the closure or the
thread; another thread posts requests through a [`Remote`]. What the
closure leaves open, forgotten states included, is closed when it returns.

Capabilities are read and set as a [`Container`] of [`Item`]s; a native
transfer's image comes as a [`Dib`], a memory transfer's as [`Strip`]s of
the caller's buffers, each as TWAIN gives it: decoding and assembling them
is the caller's.

```no_run
use std::time::Duration;
use twain_dsm::{App, Delivery, Item, Load, Next, Pending, Ui, ffi};

twain_dsm::run::<(), _>(|twain| -> Result<usize, twain_dsm::Error> {
    let opened = twain.load(Load::System)?.open(App::new("Example", "Scanning", "Example")?)
        .map_err(|r| r.into_error())?;
    let id = opened.sources()?.into_iter().next().ok_or(twain_dsm::Error::NoSuchSource)?;
    let source = opened.open_source(&id, Delivery::Callback).map_err(|r| r.into_error())?;
    source.set(ffi::ICAP_XFERMECH, Item::UInt16(ffi::TWSX_MEMORY))?;
    let mut enabled = source.enable(Ui::Hidden).map_err(|r| r.into_error())?;
    let mut ready = loop {
        match enabled.next_event(Duration::from_secs(30)).map_err(|r| r.into_error())? {
            Next::Ready(ready) => break ready,
            Next::Event(back, _) | Next::Request(back, _) | Next::Timeout(back) | Next::Quit(back, _) => {
                enabled = back
            }
        }
    };
    let mut pages = 0;
    loop {
        let mut transfer = ready.memory().map_err(|r| r.into_error())?;
        let mut buffer = vec![0; transfer.preferred_size()];
        while let Some(strip) = transfer.next_buffer(&mut buffer)? {
            let _ = (strip.x_offset, strip.y_offset, strip.bytes);
        }
        pages += 1;
        match transfer.end().map_err(|r| r.into_error())? {
            Pending::More(next, _) => ready = next,
            Pending::Done(_) => return Ok(pages),
        }
    }
})
.expect("TWAIN runs");
```
"#
)]
#![cfg_attr(
    not(windows),
    doc = "Off Windows only its pure parts build: [`ffi`], the codes and the containers."
)]
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

#![cfg_attr(docsrs, feature(doc_cfg))]
// Only the modules that call Windows or the DSM may hold unsafe code.
#![deny(unsafe_code)]
#![warn(missing_docs)]

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
#[cfg_attr(docsrs, doc(cfg(windows)))]
pub use dsm::Dib;
#[cfg(windows)]
#[cfg_attr(docsrs, doc(cfg(windows)))]
pub use enabled::{
    Enabled, Event, ImageInfo, MemoryTransfer, Native, Next, Pending, Ready, Strip, Transferred, Ui,
};
#[cfg(windows)]
#[cfg_attr(docsrs, doc(cfg(windows)))]
pub use error::{Error, Gone, LoadError, Refused, RunError};
pub use ext_info::{ExtInfo, ExtValue};
#[cfg(windows)]
#[cfg_attr(docsrs, doc(cfg(windows)))]
pub use states::{App, Delivery, ImageLayout, Load, Loaded, Opened, SetOutcome, Source, SourceId};
#[cfg(windows)]
#[cfg_attr(docsrs, doc(cfg(windows)))]
pub use thread::{Remote, RunReport, Twain, Wake, run};

/// The README's example, compiled as a doctest on Windows.
#[cfg(all(doctest, windows))]
#[doc = include_str!("../README.md")]
pub struct ReadmeDoctests;

#[cfg(test)]
mod tests {
    /// The README's licence links are the repository's URLs, which reach
    /// the files from crates.io and docs.rs: the package holds no licence
    /// file, and a relative link would climb out of it.
    #[test]
    fn the_readmes_licence_links_reach_the_files() {
        let readme = include_str!("../README.md");
        let repository = env!("CARGO_PKG_REPOSITORY");
        for file in ["LICENSE-APACHE", "LICENSE-MIT"] {
            let link = format!("[{file}]({repository}/blob/master/{file})");
            assert!(
                readme.contains(&link),
                "{file}'s link isn't the repository's URL"
            );
        }
        assert!(!readme.contains("](../"), "a link climbs out of the crate");
    }

    /// The changelog has an entry for the version the manifest gives, so a
    /// version is never packaged unsaid.
    #[test]
    fn the_changelog_has_an_entry_for_this_version() {
        let changelog = include_str!("../CHANGELOG.md");
        let heading = format!("## {} (", env!("CARGO_PKG_VERSION"));
        assert!(
            changelog.lines().any(|line| line.starts_with(&heading)),
            "the changelog has no entry headed {heading}…)"
        );
    }
}
