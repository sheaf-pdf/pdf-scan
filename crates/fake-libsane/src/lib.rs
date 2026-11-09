//! A stand-in for libsane, for tests.
//!
//! A shared library that exports SANE's C API, loaded by its path in place of
//! the system's `libsane`. It plays what SANE's own `test` and `pnm` backends
//! can't: the feeder and duplex options of particular backends, a jam or an
//! open cover at a given sheet, reads that hang, and values no well-behaved
//! backend returns, each scripted by the test that loads it. It logs every
//! call, and records a violation whenever its caller breaks SANE's calling
//! rules.
//!
//! Never published, and no crate depends on it: tests build it and load the
//! shared object by its path. This version exports nothing yet.

// Only the module that exports SANE's functions may hold unsafe code.
#![deny(unsafe_code)]
