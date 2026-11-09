//! A stand-in for libsane, for tests.
//!
//! A shared library that exports SANE's C API, loaded by its path in place of
//! the system's `libsane`. It plays what SANE's own `test` and `pnm` backends
//! can't: values no well-behaved backend returns, listings that take their
//! time, the feeder and duplex options of particular backends and
//! descriptors that change after a set, a feeder that ends where a test
//! says, duplex pages marked front or back, a jam or an open cover at a
//! given page, frames no real backend sends, reads that hang, and an abort.
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
//! wrong names rather than freed memory.
//!
//! ## Options
//!
//! Every handle has option 0, the count, then the script's options in
//! order, each with its own value. An `option` line starts one; the lines
//! after it, until the next, describe it. Any integer may be given where a
//! name is taken, so a descriptor can hold what SANE doesn't allow.
//!
//! | Line | What it does |
//! |---|---|
//! | `option NAME TYPE SIZE CAPS` | An option: its name (`@null` allowed); its type (`BOOL`, `INT`, `FIXED`, `STRING`, `BUTTON`, `GROUP`); its size in bytes; its capabilities, `-` for none or names joined by `+` (`soft-select`, `hard-select`, `soft-detect`, `emulated`, `automatic`, `inactive`, `advanced`). Its title is its name, its description empty, its unit `NONE`, unconstrained, its value 0 or empty |
//! | `title TEXT`, `desc TEXT` | Its title, its description |
//! | `unit UNIT` | Its unit: `NONE`, `PIXEL`, `BIT`, `MM`, `DPI`, `PERCENT`, `MICROSECOND` |
//! | `range MIN MAX QUANT` | A range constraint |
//! | `words W...` | A word-list constraint |
//! | `strings S...` | A string-list constraint |
//! | `constraint TYPE` | Its constraint type alone (`NONE`, `RANGE`, `WORD_LIST`, `STRING_LIST`), whatever the constraint's pointer holds: without a `range`, `words` or `strings` line, the pointer is null |
//! | `word-list-length N` | What the word list's first word claims, whatever it holds |
//! | `strings-repeat N` | `N` more string-list entries, `s<index>` |
//! | `value V...` | Its value: a string, or as many numbers as given, the last repeated to its size |
//! | `auto V...` | The value `SET_AUTO` chooses (without one it keeps the value) |
//! | `reload-params` | A set says `RELOAD_PARAMS` |
//! | `when V CHANGE NAME [ARGS]` | Once the option is set to `V`, the named option changes: `activate`, `deactivate` (any number of names), `range NAME MIN MAX QUANT`, `words NAME W...`, `size NAME BYTES`, `null NAME` (its descriptor becomes null); and the set says `RELOAD_OPTIONS`, unless a `when V quiet` line says nothing is to be said |
//! | `start-inactive NAME...` | Named options start inactive |
//! | `option-null` | An option whose descriptor is null, though it's counted |
//! | `option-count N` | Option 0's value, in place of the real count |
//! | `control-status N STATUS` | `sane_control_option` on option `N` answers `STATUS`, doing nothing |
//! | `profile NAME` | A backend's layout, its lines as above: `fujitsu` (`source` from `Flatbed`, `ADF Front`, `ADF Back`, `ADF Duplex`), `airscan` (`Flatbed`, `ADF`, `ADF Duplex`, fewer resolutions in duplex), `brother` (long `Automatic Document Feeder(left aligned,Duplex)` sources), `canon-dr` (a `ScanMode` of `Simplex` or `Duplex`), `lexmark` (a `BOOL` `scan-both-sides`), `epson` (an `adf-mode` of `Simplex` or `Duplex`), `kodak` (a `duplex` of `front`, `back` or `both`); each with `mode`, `resolution`, the scan area and a `page-width` and `page-height`, active only while a feeder is the source |
//!
//! A number for a `FIXED` option is decimal (`215.9`), rounded to the
//! nearest 1/65536; for any other, an integer. A descriptor is given at the
//! same address for as long as the process lives, rewritten in place when
//! a rule changes it, and its strings and lists are never freed.
//!
//! `sane_control_option` reads a set's value as the option's descriptor
//! sizes it (a string up to its `NUL`), writes a get's the same way (a
//! string's `NUL` only if it fits, so a value as long as the size has
//! none), and plays a backend's part: a string not in its list is `INVAL`,
//! a number out of its range or word list is moved to the nearest it allows
//! and said `INEXACT`, and the rules fire.
//!
//! ## Scanning
//!
//! Every page is the same frames, scanned from a feeder. `sane_start` takes
//! the next frame of the page in progress, or the next page; a page is
//! taken from the feeder by the start that begins it, whatever that start
//! answers (but `NO_DOCS` for a feeder that's empty). `sane_cancel` drops
//! the page in flight, and reads then answer `CANCELLED` until the next
//! start. `sane_get_parameters` gives the frame in progress, or else the
//! next one.
//!
//! | Line | What it does |
//! |---|---|
//! | `frame FORMAT LAST BPL PPL LINES DEPTH [BYTES]` | A frame of every page, in order: its parameters as `sane_get_parameters` gives them (the format by name, `GRAY`, `RGB`, `RED`, `GREEN`, `BLUE`, or any number; the rest any integers), then the bytes it sends before `EOF` (by default `BPL` × `LINES`, or 4 lines for `LINES` -1), or `endless` for none. Without any, every page is one frame `GRAY 1 16 16 4 8` |
//! | `sheets N` | The feeder holds `N` sheets (without, it never ends): the start after the last page answers `NO_DOCS` |
//! | `duplex` | Each sheet is two pages, front then back |
//! | `at PAGE start STATUS` | That page's start answers `STATUS` (pages count from 1) |
//! | `at PAGE read STATUS [BYTES]` | Once `BYTES` of that page's bytes (all its frames', default 0) are sent, every read answers `STATUS` |
//! | `at PAGE hang [BYTES]` | Once `BYTES` are sent, a read blocks until `sane_cancel`, then answers `CANCELLED` |
//! | `at PAGE hang-forever [BYTES]` | Once `BYTES` are sent, a read blocks for good, cancelled or not |
//! | `at PAGE ignore-cancel [BYTES]` | Once `BYTES` are sent, every read answers `GOOD` with no bytes, and `sane_cancel` changes nothing |
//! | `at PAGE abort` | That page's start aborts the process, its log's last line `note … abort` |
//! | `read-max N` | A read gives at most `N` bytes |
//! | `read-length N` | Every `GOOD` read claims `N` bytes, whatever it gave; `read-length past`, one more than it was asked for |
//! | `non-blocking` | `sane_set_io_mode` takes non-blocking (without, `UNSUPPORTED`) |
//! | `select-fd N` | `sane_get_select_fd` answers `GOOD` with `N` (without, `UNSUPPORTED`) |
//! | `parameters-status STATUS` | `sane_get_parameters` answers `STATUS` |
//! | `start-inval-deactivates NAME...` | A start answering `INVAL` makes these options inactive, saying nothing |
//!
//! `sane_set_io_mode` and `sane_get_select_fd` answer `INVAL` with no frame
//! in progress, as the standard says. A start that answers `INVAL` makes
//! every descriptor stale, as a set that says `RELOAD_OPTIONS` does.
//!
//! **The bytes:** a frame's bytes are its lines, each `BPL` bytes, the
//! bytes past what its pixels need (the standard's formula) padding,
//! `0xEE`. Of the rest, each frame's first byte is `F` on a front and `B`
//! on a back (every page without `duplex` is a front), and every other is
//! `(page × 31 + frame × 7 + line × 3 + column) mod 251`, the page counted
//! from 1, the frame, line and column (in bytes) from 0.
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
//! call t1 sane_get_option_descriptor handle=0x5a00 option=0
//! return t1 sane_get_option_descriptor descriptor
//! call t1 sane_control_option handle=0x5a00 option=2 action=SET value="ADF"
//! return t1 sane_control_option status=GOOD info=0x2
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
//! - `sane_close` comes with a scan pending (a `sane_start` answered `GOOD`
//!   and no `sane_cancel` came since, and no error status is waiting for
//!   one): a batch ends with `sane_cancel`, even after its last `EOF`;
//! - `sane_start` comes before the frame in progress has reached `EOF`, with
//!   no `sane_cancel` between (answered `INVAL`), or `sane_read` with no
//!   frame started (answered `INVAL`);
//! - a call comes before `sane_init` or after `sane_exit`, or `sane_init`
//!   twice without `sane_exit` between;
//! - a pointer SANE must write through is null (`sane_get_devices`'s list,
//!   `sane_open`'s handle, `sane_control_option`'s value but for
//!   `SET_AUTO`, `sane_get_parameters`'s parameters, `sane_read`'s data and
//!   length, `sane_get_select_fd`'s descriptor), or `sane_open`'s device
//!   name is: the call is answered `INVAL`, and nothing is listed or
//!   opened. (`sane_init`'s
//!   `version_code` may be null, as the standard allows: no version is
//!   given, and the log's `return` line says `version_code=null`.)
//!
//! And, for options, what a careful caller refuses before the backend sees
//! it (`sane-dl` promises these), each answered `INVAL` with nothing done:
//! `sane_control_option` on an option that doesn't exist, whose descriptor
//! is null or holds what SANE doesn't allow (an unknown type, a size that
//! doesn't fit its type, a constraint that doesn't fit it), or that is
//! inactive; a get of a group or a button, or of an option software can't
//! read; a set or `SET_AUTO` of one software can't set; `SET_AUTO` of one
//! that can't choose; a `BOOL` set as anything but a word of 0 or 1; a
//! string with no `NUL` within the option's size; an action SANE doesn't
//! define; and, after a set that said `RELOAD_OPTIONS` or a start that
//! answered `INVAL`, a call on any option but 0 before every descriptor has
//! been read again.
//!
//! A `note` line says what the script made happen that no call returns:
//! `note … hang sane_read waits for sane_cancel` (or `… waits for good`)
//! as a read blocks, `note … abort …` as the process aborts. A scanning
//! call's `return` line says more: `sane_start`'s the page and frame it
//! began (`page=2 frame=0`), `sane_get_parameters`' the six fields,
//! `sane_read`'s the length it claimed and the bytes it gave
//! (`length=64 given=64`), `sane_get_select_fd`'s the descriptor.
//!
//! A test asserts its log has no `violation` line.

// Only the module that exports SANE's functions may hold unsafe code.
#![deny(unsafe_code)]

mod exports;
mod fake;
mod feed;
// The bindings' own source, so the exports are declared with the types the
// bindings' header contract checks; not every declaration is used here.
#[allow(dead_code)]
#[path = "../../sane-dl/src/ffi.rs"]
mod ffi;
mod options;
mod script;
