//! Builds nothing, and links nothing: libsane is loaded at run time.
//!
//! The manifest's `links` key, which cargo takes only from a package with a
//! build script, keeps a build to one version of this crate. SANE is started
//! once per process, and two copies would each count their own; copies built
//! apart (each shared library built with the crate holds one) are the
//! process slot's to refuse on Linux, and elsewhere the program's rule.

fn main() {
    println!("cargo::rerun-if-changed=build.rs");
}
