//! SANE scanner access for Rust, with libsane loaded at run time.
//!
//! [SANE](http://www.sane-project.org/) is the scanner API of Linux and the
//! other Unix-like systems. This crate opens `libsane` only when asked to,
//! never at build time or start-up, so a program that takes it builds and
//! runs where SANE isn't installed, and hears so as a typed error when it
//! tries to scan. It keeps its own bindings, written from SANE's
//! public-domain `sane.h`, and checks every integer, pointer and length that
//! crosses from C before it becomes a Rust value.
//!
//! Planned: the library loaded once per process; devices listed under a
//! deadline; a handle with typed options; frames assembled into images; a
//! cancel token any thread can use; and a configuration directory that opens
//! a known device without waiting on every backend's probe.
//!
//! This version is the crate's first, empty cut: it has no API yet.

// Only the module that loads and calls the library may hold unsafe code.
#![deny(unsafe_code)]
