//! Builds nothing, and links nothing: libsane is loaded at run time.
//!
//! The manifest's `links` key, which cargo takes only from a package with a
//! build script, keeps a program to one copy of this crate. SANE is started
//! once per process, and two copies would each count their own.

fn main() {
    println!("cargo::rerun-if-changed=build.rs");
}
