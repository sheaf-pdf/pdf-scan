//! A stand-in for libsane, for tests.
//!
//! A shared library that exports SANE's C API, loaded by its path in place of
//! the system's `libsane`. It plays what SANE's own `test` and `pnm` backends
//! can't: values no well-behaved backend returns, listings that take their
//! time, and, as it grows, the feeder and duplex options of particular
//! backends, a jam or an open cover at a given sheet, and reads that hang.
//! Each is scripted by the test that loads it. It logs every call, and
//! records a violation whenever its caller breaks SANE's calling rules.
//!
//! Never published, and no crate depends on it: tests build it and load the
//! shared object by its path. Its exports are declared with the very types
//! of `sane-dl`'s bindings (the same source file), and each is checked at
//! compile time to have its binding's type.
//!
//! # Driving it
//!
//! A test spawns the process that loads the fake with two environment
//! variables, read at its first call:
//!
//! - `FAKE_LIBSANE_SCRIPT`: the path of a script, below. Without one, the
//!   fake starts SANE 1.0.0 with no devices. A script that can't be read or
//!   parsed makes `sane_init` fail with `INVAL`, and the log says why.
//! - `FAKE_LIBSANE_LOG`: the path of the log, appended to as calls come and
//!   go, so it's complete even if the process is killed. Without one,
//!   nothing is logged.
//!
//! # The script
//!
//! A line per instruction: a keyword, then its values. A value is a bare
//! word, or quoted with `"` (with `\"`, `\\` and `\xHH` escapes inside, so
//! any bytes can be given); `@null` is a null pointer, and `@long:N` is `N`
//! bytes of `x`. `#` starts a comment.
//!
//! | Line | What it does |
//! |---|---|
//! | `init STATUS [M.m.b]` | One `sane_init`'s answer: its status and version (default `1.0.0`). One line per call, in order; the last repeats. Without any: `init GOOD 1.0.0` |
//! | `device NAME VENDOR MODEL TYPE` | A device `sane_get_devices` lists, in order; any of the four may be `@null` |
//! | `device-repeat N` | `N` more devices, named `fake:<index>` |
//! | `devices STATUS` | What `sane_get_devices` returns (default `GOOD`); other than `GOOD`, no list |
//! | `devices-null` | `sane_get_devices` returns `GOOD` with a null list |
//! | `listing-delay MS...` | How long each listing takes, in milliseconds, in order; the last repeats |
//! | `open NAME STATUS` | What `sane_open` returns for `NAME`; `GOOD` gives a handle |
//! | `open NAME null-handle` | `sane_open` returns `GOOD` for `NAME`, with a null handle |
//! | `strstatus-null` | `sane_strstatus` returns null |
//!
//! A status is SANE's name without `SANE_STATUS_` (`GOOD`, `INVAL`,
//! `NO_DOCS`, the reserved `WARMING_UP` and `HW_LOCKED`), or any integer.
//! Without an `open` line for it, a listed device's name opens with `GOOD`
//! and any other name returns `INVAL`. A device list is overwritten with
//! `#`s when it goes stale (at the next listing or `sane_exit`, as SANE
//! has it), and kept, never freed, so a caller that reads it late reads the
//! wrong names rather than freed memory. Options and scanning aren't played
//! yet: those calls return `UNSUPPORTED` (`sane_get_option_descriptor`
//! null).
//!
//! # The log
//!
//! A line per event, its words separated by spaces: what happened, the
//! thread (`t` and a number, the same for every call from one thread), the
//! function, then details as `key=value`.
//!
//! ```text
//! call t1 sane_init authorize=null
//! return t1 sane_init status=GOOD version=0x01000000
//! call t2 sane_get_devices local_only=0
//! return t2 sane_get_devices status=GOOD devices=1
//! call t1 sane_open name="fake:0"
//! return t1 sane_open status=GOOD handle=0x5a00
//! violation t3 sane_exit came with handles open: 0x5a00
//! note t1 script-error the script doesn't parse: line 2: "frob" isn't a keyword
//! ```
//!
//! `call` is logged as a function starts and `return` as it returns, so a
//! call that never returns shows. `violation` says a rule was broken:
//!
//! - two calls other than `sane_cancel` overlap;
//! - a call reaches a handle that's closed, or that `sane_open` never gave
//!   (`sane_cancel` included);
//! - `sane_exit` comes with handles open;
//! - after `sane_start`, `sane_get_parameters` or `sane_read` returns a
//!   status other than `GOOD` and `EOF` (an error, or a batch's `NO_DOCS`),
//!   the next call on that handle is neither `sane_cancel` nor `sane_close`;
//! - a call comes before `sane_init` or after `sane_exit`, or `sane_init`
//!   twice without `sane_exit` between;
//! - a pointer SANE must write through is null (`sane_get_devices`'s list,
//!   `sane_open`'s handle), or `sane_open`'s device name is: the call is
//!   answered `INVAL`, and nothing is listed or opened. (`sane_init`'s
//!   `version_code` may be null, as the standard allows: no version is
//!   given, and the log's `return` line says `version_code=null`.)
//!
//! A test asserts its log has no `violation` line.

// Only the module that exports SANE's functions may hold unsafe code.
#![deny(unsafe_code)]

mod exports;
mod fake;
// The bindings' own source, so the exports are declared with the types the
// bindings' header contract checks; not every declaration is used here.
#[allow(dead_code)]
#[path = "../../sane-dl/src/ffi.rs"]
mod ffi;
mod script;
