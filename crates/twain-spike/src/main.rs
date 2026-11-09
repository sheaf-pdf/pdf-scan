//! A throwaway program that drives TWAIN end to end through the TWAIN Working
//! Group's sample data source, and checks what it sees.
//!
//! It loads the system's Data Source Manager (`TWAINDSM.dll`, from the system
//! folder only) and walks TWAIN's states on a thread of its own: a
//! single-threaded apartment with a hidden top-level window, every DSM call
//! made there, what happens sent to the main thread on a channel. Each
//! scenario checks every answer it gets against what the sample source's code
//! says it should be, and the run exits 1 on any failure, or if a scenario goes
//! 30 seconds without an event. On other systems it says it's Windows only and
//! exits 2.
//!
//! Install the sample source and the DSM first (`scripts/twain-sample.ps1
//! Install`), then run `twain-spike all`, or `twain-spike <scenario>`; the
//! bitness of the build decides which DSM and which sources it sees.
//!
//! It is a spike: what it finds goes into twain-dsm's design, and it is
//! deleted once twain-dsm's own tests replace it.

// Only the modules that call Windows or the DSM may hold unsafe code.
#![deny(unsafe_code)]

// Pure: compiled and tested everywhere.
#[cfg_attr(not(windows), allow(dead_code))]
mod container;
// Its own `allow(dead_code)`: a bindings module declares more than it uses.
mod ffi;
#[cfg_attr(not(windows), allow(dead_code))]
mod image;

#[cfg(windows)]
#[allow(unsafe_code)]
mod dsm;
#[cfg(windows)]
#[allow(unsafe_code)]
mod run;
#[cfg(windows)]
mod scenarios;
#[cfg(windows)]
#[allow(unsafe_code)]
mod session;
#[cfg(windows)]
mod twain_thread;
#[cfg(windows)]
#[allow(unsafe_code)]
mod window;

#[cfg(windows)]
fn main() {
    std::process::exit(run::main());
}

#[cfg(not(windows))]
fn main() {
    eprintln!("twain-spike drives TWAIN's Windows Data Source Manager: it runs on Windows only");
    std::process::exit(2);
}
