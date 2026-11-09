# S0 slice 1: sane-dl, and the workspace it starts

Status: running, on branch `s0-1-sane-dl`, from 2026-11-08. Milestone: `milestones/s0-devices.md`, slice 1.

Paths: `docs/pm/` files from `docs/pm/`, as everywhere here; the code's from the repository's root, written
with a leading `/` (`/crates/sane-dl/`, `/justfile`); `scratch/` from the root, local and never tracked.

## What it builds

The Cargo workspace, laid from S1 slice 1's stashed work (`scratch/stash-s1-1/files/`) and reshaped so
`sane-dl` is its first member, with its licenses, README, justfile, CI on Ubuntu, Windows and macOS, and the
gates for what exists. Then `sane-dl` (`crates/sane-dl.md`): SANE loaded at run time, its own bindings from
`sane.h`, a safe library and handle, a device list under a deadline, typed options, frames assembled into
images, a cancel token any thread can use and that never reaches a closed handle, and the one-backend
configuration that opens a known device fast. Beside it, the fake libsane, a never-published `cdylib` that
exports SANE's API, narrowed to what SANE's own `test` and `pnm` backends can't do, and checking that
`sane-dl` keeps SANE's rules. `sane-dl` is made ready to publish, and not published.

Nothing is cut from sheaf: `sane-dl` is unbuilt there (`sheaf/from-sheaf.md` lists only plan material for it:
M5 proposal 6, `research/scanners.md`, entry 5), so there is no harness comparison. The plan files it cites
were checked for drift: sheaf is still at `3232a35`.

**Later slices build on it** (`milestones/s0-devices.md`): slice 2 revises the seam note from what this slice
learns of SANE (its device names, whether a batch can stop after the page in flight); slice 3's model fixes
the raw layout's polarity and byte order, which `sane-dl` hands over as SANE gives them; slice 4's watchdog
needs the fake's crash and hang; slice 5's `sane` backend drives `sane-dl`'s batches, options and
configuration, maps its statuses (`crates/scan-device-seam.md`, section 6), and reads the fake's feeder and
duplex layouts; slice 6 runs it on an fi-6130. The design below exposes what each needs.

## Taken as it starts

Under the asks' standing rule (`asks.md`), starting this slice takes the answers it depends on as
recommended, each open to Greg's review:

- **ask 2:** the cut, the ten crates; this slice depends on its device half, `sane-dl` a crate of its own
  beside `scan-device`. Recorded in `decisions.md` with step 1, and ask 2 leaves `asks.md`.
- **asks 6 and 7** (the names; the standing rules) were taken as this slice started (`decisions.md`, "Made
  here"); asks 3 and 4 Greg answered by putting S0 first.

## The design

### The workspace

From `scratch/stash-s1-1/files/` (its README says what each file is), with S1 slice 1's decisions 3 to 9
reused, reshaped:

- **`/Cargo.toml`:** resolver 3, members `crates/sane-dl` and `crates/fake-libsane`. The four S1 crates are
  dropped (S1 slice 1 adds them back). No `[workspace.package]`: each crate states its edition (2024), license
  and `rust-version`. `[workspace.dependencies]` only for what more than one crate takes: nothing yet (the
  fake takes no dependency), so the table is absent and its comment says when it returns. `[workspace.lints]`
  holds the unsafe rules both crates need: `unsafe_op_in_unsafe_fn` and `clippy::undocumented_unsafe_blocks`
  denied; each crate takes `lints.workspace = true`.
- **`/Cargo.lock`**, tracked: made with step 1 (once its dependencies are named), changed only by a step that
  adds one, since every recipe runs `--locked`.
- **`/LICENSE-MIT`, `/LICENSE-APACHE`, `/.gitattributes`** as stashed; `.gitattributes` adds the PNM types
  (`*.pbm`, `*.pgm`, `*.ppm`, `*.pnm` binary).
- **`/README.md`:** scanner access first (`sane-dl` built; `scan-device`, `scan-worker`, `twain-dsm` next),
  then the pages and PDF crates as planned. Never naming the downstream app. It drops "every crate is safe
  Rust": unsafe code sits only where a driver API is called, in as few modules as it can.
- **`/justfile`:** `ci`, `fmt`, `clippy`, `test`, `doc`, `gates`, `msrv` and `sane-dl-msrv` (on the version
  its `Cargo.toml` states, read as the stash reads it), and with step 5 `sane-dl-package`.
- **The gates** (`just gates`, run on Ubuntu, over `cargo tree --target all -e normal,build`):
  - the devices layer (`sane-dl` now; `scan-device`, `scan-worker`, `twain-dsm` join it) takes no image codec
    (`image`, `tiff`, `png`, `zune-*`, `jpeg-*`, `fax`, `hayro-*`), no imageproc, no PDF crate, no OCR or
    model runtime, no GUI toolkit (the stash's patterns, the codec one added);
  - nothing links libsane: no package in `cargo metadata` declares `links = "sane"`, and no binary built has
    libsane among the `NEEDED` entries of its dynamic section (`readelf -d`): the test binaries, listed by
    `cargo test --no-run --message-format=json`, and the fake's `cdylib`, listed by `cargo build --workspace
    --message-format=json` (`cargo test --no-run` builds only the fake's unit-test harness, not the shared
    object: seen in a probe, `scratch/work/s0-1/review-probe/cdy/`). The gate says how many files it read, and
    fails, saying why, if `readelf` is missing;
  - `fake-libsane` is never published (`publish = false`) and no crate takes it as a dependency of any kind
    (tests build it themselves, below);
  - the name gate as the stash has it: no file git tracks or would track (untracked but not ignored) outside
    `docs/pm/` names the downstream app, but as the org in a URL, its comment and message worded so it can't
    match itself.
- **CI** (`/.github/workflows/ci.yml`), on pushes to `master` and on pull requests, read-only permissions,
  credentials not persisted, each step a recipe:
  - **check**, on `ubuntu-latest`, `windows-latest` and `macos-latest`: `just fmt clippy test doc`. On Ubuntu
    first `apt-get install libsane1 libsane-dev` (the `test` and `pnm` backends are in `libsane1`), the
    versions installed printed and the two backends listed, and the tests run with `SANE_DL_REQUIRE_LIBSANE=1`
    and `SANE_DL_REQUIRE_HEADER=1`, so a test that would skip there fails;
  - **gates**, on Ubuntu, with `libsane1` and `libsane-dev` installed too, so a stray link would build and the
    gate, not the linker, says what's wrong;
  - **sane-dl-msrv**, on Ubuntu: `dtolnay/rust-toolchain@<sane-dl's MSRV>`, with a note to keep it in step
    with `rust-version` in `/crates/sane-dl/Cargo.toml`. No libsane there: its tests that need it say they
    skipped;
  - **sane-dl-package** (step 5), on Ubuntu.
- **The MSRV,** measured as S1 slice 1's decision 2 did: a probe under `scratch/work/s0-1/msrv-probe/` takes
  `sane-dl`'s dependencies (libloading 0.9, thiserror 2) and its dev-dependencies (tempfile 3, rusty-fork 0.3
  with `timeout`), since `just sane-dl-msrv` runs the tests; each package's `rust-version` read from `cargo
  metadata` over `cargo tree -p` on every target; the highest, never below 1.89, built and tested on that
  toolchain. Expected: 1.89 (libloading 0.9.0 declares 1.88). The fake takes the same floor.

### sane-dl's API

SANE alone: no device model, no policy. It says what SANE says and checks everything that crosses from C.
The names below are the design's; an implementation that keeps the design and renames says so in "So far".

- **The bindings** (`sane_dl::ffi`, a public module: types and constants only, no unsafe code of its own, so
  the header contract, an integration test, can reach them, and so can anyone who wants SANE's raw names):
  `sane.h`'s 14 functions as `unsafe extern "C"` function-pointer types, its structs `#[repr(C)]`
  (`SANE_Device`, `SANE_Option_Descriptor` with its constraint as a union of three pointers, `SANE_Range`,
  `SANE_Parameters`), its types (`SANE_Word` and the rest as `c_int` and friends) and constants. Every C enum
  is an integer type with constants, never a Rust enum: statuses 0 to 11, and the reserved
  `SANE_STATUS_WARMING_UP` (12) and `SANE_STATUS_HW_LOCKED` (13) from its `#if 0`; frame types 0 to 4 and the
  reserved 0x0A to 0x12; value types, units, constraint types, actions, the `SANE_CAP_*` and `SANE_INFO_*`
  bits. The file keeps the header's notice word for word and marks what changed (translated to Rust
  declarations; enums as integers with constants; the `#if 0` values given as constants marked reserved;
  comments reworded). Written from `sane.h` 1.4.0 as `libsane-dev` installs it here
  (`/usr/include/sane/sane.h`, byte for byte the header at sane-backends' tag `1.4.0`, also fetched to
  `scratch/work/s0-1/sane-h/`: the same SHA-256). Nothing from `saneopts.h`: unlike `sane.h` it is GPL-2.0+
  with SANE's exception, so it's study only.
- **`Status` and `FrameFormat`:** `pub struct Status(pub i32)` with a constant per status (the reserved two
  marked), `name()` (`None` for a code SANE doesn't define) and `Display`; `FrameFormat` the same. `Version {
  major, minor, build }` from `sane_init`'s code; anything but major 1 is refused.
- **Loading: `Sane`.** `Sane::load()` opens the system's library by its versioned name (`libsane.so.1` on
  Linux and the other Unixes, `libsane.1.dylib` on macOS, none on Windows, where SANE isn't the system's API);
  `Sane::load_from(path)` opens the one the caller names (a test's fake; an app's override, whose environment
  variable is the app's). Loading happens only when asked; then `sane_init` with no authorization callback.
  `LoadError` (thiserror): `NotFound { tried, source }` naming the files tried, `NoDefault` (Windows),
  `MissingSymbol { name }`, `Init(Status)`, `UnsupportedVersion(Version)`, `AlreadyLoaded`. One live `Sane`
  per process (a second load while one lives is `AlreadyLoaded`); a later one after the first's `sane_exit` is
  allowed. A load that fails leaves nothing behind: a version refused after a good `sane_init` still gets its
  `sane_exit`, and every failure frees the process's one slot, so a later load can succeed. The library is
  never unloaded (no `dlclose`): backends may leave threads, exit handlers or thread-local destructors in it.
  Every call but `sane_cancel` holds the instance's lock, so no two SANE calls ever overlap, as the standard
  requires ("the SANE API does not require any other operations to be re-entrant"); `Sane` is `Send + Sync`,
  handles keep it alive, and `sane_exit` runs when the last of them goes.
- **Devices under a deadline:** `devices(local_only, deadline) -> Result<Vec<Device>, Error>`.
  `sane_get_devices` runs on a thread of its own, its list copied there before anything else is called (SANE
  keeps it valid only until the next listing or `sane_exit`); past the deadline the call returns
  `DeadlineExceeded` and the listing keeps the library until it returns: calls meanwhile return
  `ListingPending` rather than block (named apart from SANE's `DEVICE_BUSY`, which the seam's section 6 maps
  to the model's `Busy`); a handle dropped meanwhile has its alive flag cleared at once and its `sane_close`
  left to the listing's thread, which runs it when the listing returns, so a drop never blocks on a hung
  backend; and if the `Sane` is dropped first, the listing's thread calls `sane_exit` after those closes. The
  docs say that a listing and a scan in one process wait on each other (one lock), so a listing belongs in a
  process of its own, as the seam's section 1 has it. `Device { name: DeviceName, vendor, model, kind }`: the
  name kept as SANE's bytes (it goes back to `sane_open` exactly), the rest as text (lossy UTF-8). A null list
  or a null name is `BadDeviceList`; a null vendor, model or kind is empty; each string read up to the
  descriptor-string cap.
- **Opening a known device fast: `ConfigDir`.** Writes a `dll.conf` naming the given backends and an empty
  `dll.d/` into a directory the caller chooses (a temporary one, 0700), and gives the value for
  `SANE_CONFIG_DIR`: alone (`Isolated`: only this directory, as tests use it), or with a trailing colon
  (`BeforeSystem`: then the system's directories, so the backend still finds its own `<backend>.conf`; the
  empty `dll.d/` keeps the system's `dll.d` snippets out, measured below). `BeforeSystem` also searches the
  process's current directory before `/etc/sane.d` (SANE's default list is `.` then `/etc/sane.d`,
  `sane-dll(5)`), so its docs say to run the process that loads SANE from a directory it trusts. Backend names
  are checked (ASCII lower-case letters, digits, `_`) before they're written. `backend_of(device_name)` gives
  the text before the first `:`. `sane-dl` never sets an environment variable (it isn't thread-safe, and SANE
  reads it once at `sane_init`): the caller applies the value to the process that loads SANE, through
  `Command::env` when it spawns a worker. `ConfigError`: `BadBackendName`, `Io`.
- **`Handle`** (`open(name)`; `Send`, not `Sync`; `sane_close` on drop or `close()`):
  - **options:** descriptors read and copied on open into owned `OptionDescriptor`s (index, name, title,
    description, `ValueType`, `Unit`, size, capabilities, `Constraint`: `Range { min, max, quant }`,
    `WordList`, `StringList`), the count from option 0's value. An integer from C becomes one of these enums
    only through a check: an unknown value type is kept as `ValueType::Unknown(i32)` and its option can't be
    got or set; an unknown unit is `Unit::Other(i32)`. The constraint's union is read only as the member its
    type names, and only where the standard allows that constraint (ranges and word lists on `INT` and `FIXED`
    options, string lists on `STRING` ones); any other pairing makes the option unusable, said, and its
    pointer is never read. Sizes are checked as the standard has them (a positive multiple of a word for `INT`
    and `FIXED`, exactly a word for `BOOL`). `get` returns a `Value` (`Bool`, `Int(Vec<i32>)`,
    `Fixed(Vec<Fixed>)`, `String(SaneString)`); `set` takes one (`Button` for a button, passed as a word);
    `set_auto(option)` takes none (SANE ignores the value) and is refused for an option without
    `SANE_CAP_AUTOMATIC`. Each refuses before calling the backend an option that's inactive, not
    soft-selectable (for a set), of another type, of another length, or a string longer than the option's
    size; strings go in a buffer of the option's full size, as the backend may write back, and are read back
    only up to a `NUL` inside it. A `BOOL` travels as a word, never a Rust `bool`'s byte; a word read back
    that is neither 0 nor 1 is `BadValue`, never a bool by cast. They return `SetInfo { inexact,
    reload_options, reload_params }`; `reload_options` re-reads every descriptor before returning, and so does
    `sane_start` answering `INVAL`, each reload said in what the call returns. `Fixed` is SANE's 16.16 word,
    with `to_f64` and a checked `from_f64`. Strings that go back to C (string values, string-list entries,
    device names) stay bytes; text for people is lossy UTF-8.
  - **scanning, step by step** (what the seam needs as SANE gives it): `start()`, `parameters()` (the six
    fields as SANE gives them, and checked accessors), `read(buf)` (each read's status; the length asked for
    capped at the read chunk and `i32::MAX`; a length returned below 0 or above it is `BadReadLength`, never
    trusted), `set_io_mode(non_blocking)`, `select_fd()` and `cancel()`. `select_fd()` is Unix only
    (`cfg(unix)`) and gives a `RawFd`: the descriptor is the backend's, valid only until the next
    `sane_start`, `sane_cancel` or end of frame (the standard), so `sane-dl` never closes it and never wraps
    it in `OwnedFd` or `BorrowedFd`; a negative one is an error. `sane_cancel` follows every error status from
    `sane_start`, `sane_get_parameters` and `sane_read`, and every batch's end, as SANE's frontends do (an
    error from `sane_control_option` needs none; the fake's check holds the library to exactly this).
  - **a batch:** `batch()` gives pages until `NO_DOCS`, from `sane_start` or from the first read of a page
    (each said: `Page::NoDocs { from }`), counting the pages delivered; it calls `sane_cancel` however it
    ends. Whether an empty feeder at the first page is an error is the backend's call (the seam, section 5),
    so the batch only reports.
  - **a page assembled** (`next_page(on_progress)` returns an `Image`): every frame read to `EOF`, `lines =
    -1` included; rows top down, padding dropped (each row exactly the pixels' bytes); three-pass red, green
    and blue frames in any order interleaved into one RGB image (sample by sample at 8 and 16 bits; byte by
    byte at 1 bit, as SANE's own 1-bit RGB frames are laid out); the samples as SANE encodes them, said in the
    type: 1-bit gray with 1 black and the leftmost pixel in the high bit, 1-bit RGB with 0 the least
    intensity, 16-bit in this machine's byte order. `Image { format, depth, width, height, stride, data,
    frames }`, `frames` the parameters of each frame as SANE gave them. `on_progress` hears the bytes read and
    the bytes expected where `lines` is known, for the seam's progress events; it's called between reads with
    no lock held, so it may use the cancel token. Page assembly reads blocking: it sets blocking mode after
    each `sane_start` (which every backend must support), and non-blocking reads with the select fd are the
    step-by-step API's. A status mid-page (`JAMMED`, `COVER_OPEN`, `IO_ERROR`, `NO_DOCS` after the first byte,
    any other) is an error, and the page in flight is dropped.
  - **caps on hostile input** (`Limits`, defaults said in the docs, each checked before the work it caps):
    options per device (4,096), bytes in a descriptor's string (64 KiB), entries in a word or string list
    (65,536), an option's size (1 MiB), devices in a list (1,024), bytes in a frame (1 GiB; a 1200 dpi 16-bit
    color Letter page is about 0.75 GiB), bytes per line (16 MiB), the read chunk (256 KiB). Parameters that
    make no sense are `BadParameters { parameters, reason }`, carrying what SANE said: a frame type other than
    0 to 4 (the reserved JPEG and CCITT ones included), a `last_frame` other than 0 or 1, a depth other than
    1, 8 or 16, pixels per line below 1, fewer bytes per line than the pixels need (the standard's formula),
    `lines` below -1 or 0 (Brother's `2500x0`), a gray or RGB frame that isn't the last or that follows a
    single-channel one, a three-pass image whose frames differ in size or depth, repeat a channel, or end
    (`last_frame`) before all three channels came. A frame that ends before `lines` rows is `ShortFrame`, one
    that runs past them `LongFrame`, a final partial row under `lines = -1` `PartialRow`, each with the
    counts; a stream past the frame cap `FrameTooLarge`, its reading stopped.
- **`CancelToken`** (`Clone + Send + Sync`, from `cancel_token()`): `cancel()` calls `sane_cancel` from any
  thread, under the handle's own lock with an alive flag that `close` clears before `sane_close`, so a late
  cancel is a no-op and never reaches a closed handle (airbridge's pattern, credited where it sits, in our own
  code: below, "Dependencies, vetted"). It also sets a flag the read loop checks, so a page stops even if a
  backend ignores `sane_cancel` and keeps answering `GOOD` with no bytes. The flag stays set until a call
  returns `Cancelled` for it (a read, a `start`, a batch's next page), so a cancel between pages isn't lost.
  Stopping after the page in flight is the caller's (it starts no next page and ends the batch).
- **`Error`** for the handle and the library (thiserror): `Status { status, call }`, `call` saying which
  function answered (so `INVAL` from `sane_open` and `IO_ERROR` from a read, the seam's section 6, tell
  apart); `ListingPending`; `DeadlineExceeded`; `BadDeviceList`, `BadHandle` (a `GOOD` from `sane_open` with a
  null handle), `BadDescriptor { option, reason }`, `BadValue { option, word }`; the option refusals
  (`NoSuchOption`, `Inactive`, `NotSettable`, `NotAutomatic`, `WrongType`, `WrongLength`, `StringTooLong`,
  `Unusable`); `BadReadLength`, `BadParameters`, `ShortFrame`, `LongFrame`, `PartialRow`, `FrameTooLarge`;
  `Cancelled`; `NulInName`. `status_message(Status)` gives `sane_strstatus`'s text, bounded, `None` for a
  null.
- **The API's shape for its callers:** the errors (`LoadError`, `Error`, `ConfigError`) and the status-like
  enums that may grow are `#[non_exhaustive]`; the plain data (`Device`, `Parameters`, `OptionDescriptor`,
  `Constraint`, `Value`, `SetInfo`, `Image`, `Version`) has public fields and isn't, so slice 5's `sane`
  backend can build them in its own tests without a library. Tests in `tests/`, outside the crate, match the
  errors with a wildcard arm.
- **Unsafe** sits in one module that loads the library, calls it and copies what it returns into owned values;
  the crate root is `#![deny(unsafe_code)]` and only that module allows it. Every block has a `// SAFETY:`
  comment saying why it holds.

### For the seam

`crates/sane-dl.md`'s "For the seam" and `crates/scan-device-seam.md`, as `sane-dl` exposes them:

- **1, devices:** `Device`'s four fields as SANE gives them; `local_only`; the deadline; `open` by name
  without a list; `ConfigDir` and `backend_of` for opening the last device fast.
- **3, a page:** each frame's parameters as SANE gave them (in `Image::frames` and from `parameters()`), each
  read's status, blocking or not with the select fd; progress through `on_progress`; the samples' polarity
  and byte order said, not changed: the model's raw layout fixes them in slice 3.
- **5, a batch's end:** `NO_DOCS` and which call returned it, with the pages delivered so far.
- **6, errors:** every status with the call that answered it; statuses SANE doesn't define kept as integers.
- **7, cancel:** the token, synchronized with close; "feed no more" left to the caller, who starts no
  next page.
- **8, threads:** a handle on one thread at a time, calls never overlapping, the token from any thread.

### The fake libsane

`/crates/fake-libsane/`, package `fake-libsane`, `crate-type = ["cdylib"]`, `publish = false`, MIT OR
Apache-2.0, no dependencies. It stays its own crate after S1 (`scan-fixtures` will be a library of pages
that crates take as a dev-dependency; the fake is a shared object loaded by path, and must not carry image
codecs into it): `crates/scan-fixtures.md` and `plan.md` change to say so (step 5).

- **Its declarations are the bindings':** it takes `sane-dl`'s `ffi.rs` itself (`#[path]`, not a dependency),
  never a second copy, so what it exports is declared with the very types the header contract checks; and each
  export is asserted at compile time to have its binding's function type. Its exports are `unsafe extern "C"`
  functions (clippy refuses a safe `pub` function that dereferences a raw pointer) with `# Safety` sections
  (`clippy::missing_safety_doc`, seen in a probe), in one module; the rest of the crate denies unsafe code.
- **What it does:** exports SANE's 14 functions; `sane_init` reads a script from `FAKE_LIBSANE_SCRIPT` (set by
  the test when it spawns the child) and logs every call, with its thread, to `FAKE_LIBSANE_LOG`, which the
  parent test reads after the child exits. The script's format and the log's are documented in its crate docs,
  so slice 5's `sane` backend tests can drive it as `sane-dl`'s do (copying the small helper that builds and
  finds it, or sharing one then, slice 5's call: no crate may depend on the fake).
- **SANE's rules, checked:** it records a violation when two calls other than `sane_cancel` overlap, when
  anything reaches a closed handle (`sane_cancel` included), when `sane_exit` comes with handles open, when
  the call after an error status from `sane_start`, `sane_get_parameters` or `sane_read`, or after a batch's
  `NO_DOCS`, isn't `sane_cancel` or `sane_close`, and it overwrites a device list once it's stale, so a stale
  read shows. Tests assert the log holds no violation, and its own unit tests show each check catching one.
- **Narrowed to what the `test` and `pnm` backends can't do** (`crates/sane-dl.md`, "Tested by", as the survey
  corrected it):
  - backends' option layouts for the feeder and duplex: fujitsu's `source` (`ADF Front`, `ADF Back`, `ADF
    Duplex`), airscan's (`ADF`, `ADF Duplex`), Brother's long strings (`Automatic Document Feeder(left
    aligned,Duplex)`), Canon DR's `ScanMode`, Lexmark's `scan-both-sides`, Epson's `adf-mode`, Kodak's string
    `duplex` (`front`, `both`), each a profile with its `page-height` and the rest; duplex pages marked front
    or back in their pixels, so slice 5 can check order;
  - a jam at sheet N (from `sane_read` mid-page, or from `sane_start`), `COVER_OPEN` at sheet N;
  - Brother's `2500x0`;
  - descriptors that change after a set (`page-height` active only for the feeder; a constraint that changes),
    and `INVAL` from `sane_start` with descriptors changed behind it;
  - a read that hangs until `sane_cancel`, one that hangs regardless, and an abort at sheet N, for this
    slice's cancel tests and slice 4's watchdog;
  - a slow `sane_get_devices`, for the deadline;
  - hostile values, which no real backend gives on purpose: a version of 2, `sane_init` failing, unknown
    statuses (12, 13, 42, -1) from `sane_start` and `sane_read`, a read length past the buffer or below 0,
    unknown and reserved frame types, a `last_frame` of 2, depths of 0, 3 and 32, bytes per line too few or
    negative, `lines` of -2 and past the cap, a gray frame that isn't last, a three-pass image missing a
    channel, an endless `lines = -1` stream, descriptors with null or overlong strings, unknown types, units
    and constraints, a constraint on a type it doesn't fit (a string list on an `INT`), sizes of 0, negative,
    not a word's multiple or past the cap, a `BOOL` word of 2, word lists claiming 2³¹ entries, string lists
    past the cap, an option count of 10⁹, a null descriptor below the count, device lists that are null, have
    a null name or run past the cap, a `GOOD` `sane_open` with a null handle, a null `sane_strstatus`.
  - Not in it: `lines = -1` from a feeder, which the `test` backend gives (hand-scanner with the feeder, seen
    here on 1.4.0), and everything else the survey moved to the `test` backend.

### How the tests run

- **Each test that loads a library runs in a child process of its own** (rusty-fork 0.3): SANE is one
  `sane_init` per process and reads `SANE_CONFIG_DIR` once, so each scenario gets its own process, its
  configuration directory and script given through the child's environment at spawn (no `set_var`, so no
  unsafe in `sane-dl`'s tests), and its timeout (rusty-fork's `timeout`) so a hang fails the test rather than
  the run. They run on all three systems where what they load exists. rusty-fork sends the child's stdout and
  stderr to a file it relays through `print!` (captured, so shown only on a failure), so whatever must be seen
  on a pass is said by the parent: the child exits with a code of its own for "skipped" (77) after printing
  why, and the parent writes the reason straight to its own stderr (not captured by the test harness), or
  fails under `SANE_DL_REQUIRE_LIBSANE=1`. Every file a test writes (configuration directories, scripts, logs,
  PNM images, the header program) goes under `CARGO_TARGET_TMPDIR`; rusty-fork's small capture file is the one
  thing in the system's temp directory.
- **Tests find the fake by building it:** a helper in the parent half of each test, once per test binary
  (before any child is spawned; the path then goes to the child in its environment), runs `cargo build
  --locked -p fake-libsane` (the cargo that built the test, `env!("CARGO")`, so `cargo +1.89 test` builds it
  on 1.89 too) into a target directory of its own under the test's `CARGO_TARGET_TMPDIR`, and loads
  `<DLL_PREFIX>fake_libsane<DLL_SUFFIX>` from there. The nested build clears `RUSTFLAGS`,
  `CARGO_ENCODED_RUSTFLAGS` and `CARGO_BUILD_TARGET`, so a sanitizer or cross run of the tests doesn't reach
  it (under `-Zsanitizer=thread` a plain build of the fake fails, "mixing `-Zsanitizer` will cause an ABI
  mismatch": seen in a probe). The same locally and in CI; no recipe needed, no stale copy.
- **Real libsane** (Linux): the child's `SANE_CONFIG_DIR` is a `ConfigDir`, `Isolated`, in a temporary
  directory under `CARGO_TARGET_TMPDIR`, naming `test` or `pnm`: never `/etc/sane.d`. The library is the
  system's, or `SANE_DL_LIBSANE`'s path. Where it isn't there (Windows, macOS, a Linux without it, known by
  `LoadError::NotFound` or `NoDefault`; any other load error fails) the test skips as above, saying why, and
  passes; with `SANE_DL_REQUIRE_LIBSANE=1` (CI's Ubuntu job, and the checking agents here) it fails instead.
- **The header contract (TEST-2):** an integration test (it reaches the bindings through `sane_dl::ffi`).
  Where `sane.h` is installed (`/usr/include/sane/sane.h`, here and on CI's Ubuntu, or the path in
  `SANE_DL_SANE_H`) and a C compiler is (`$CC` or `cc`), it writes a C file that prints every constant's value
  and every struct's size and field offsets, and asserts each function's type at compile time
  (`_Static_assert` over `__builtin_types_compatible_p(__typeof__(&sane_open), …)`, which references no symbol
  of libsane, so nothing links it: seen in a probe); compiles and runs it; and compares with the bindings. The
  C spellings are the header's own typedef names (GCC and Clang take `SANE_Status` as compatible with
  `unsigned int`, not `int`: seen in a probe, so an enum can't be spelled as an integer there; the enums'
  sizes and values are checked instead), kept in one table beside the binding each checks, which asserts at
  compile time that the binding has the table's Rust type, so the two can't drift. A pass over the header's
  text checks that every numeric `#define` and enumerator, the `SANE_STATUS_`, `SANE_FRAME_`, `SANE_TYPE_`,
  `SANE_UNIT_`, `SANE_CONSTRAINT_`, `SANE_ACTION_`, `SANE_CAP_` and `SANE_INFO_` families among them, has a
  constant, the `#if 0` ones included with their values. sanetui's header-contract test (Apache-2.0) is the
  idea, credited in the test; the code is our own. Where the header isn't installed, the test says so,
  straight to stderr; `SANE_DL_REQUIRE_HEADER=1` makes that a failure.
- **Loading itself:** a missing library is `NotFound` with what was tried, on every system; a library without
  SANE's symbols (the system's C library, by its name on each system) is `MissingSymbol`; a second load while
  one lives is `AlreadyLoaded`, and a load after the first's exit succeeds; a refused version is exited (the
  fake's log shows `sane_exit`), and the next load in the same child, its script giving version 1 then,
  succeeds.
- **Unit tests** need no library: assembly from chunks and parameters, descriptor decoding from owned copies,
  statuses, `Fixed`, `ConfigDir`, the limits. They run everywhere.

### Dependencies, vetted

| Crate | For | Vetted |
|---|---|---|
| `libloading` 0.9 | Loading the library | ISC; 0.9.0, 2025-11-05; 625M downloads, about 1,650 dependents; what every permissive Rust SANE frontend uses (the survey); `rust-version` 1.88; pulls `cfg-if` (Unix) and `windows-link` (Windows) only; its unsafe is the platform loader's, and `Library::new` and `get` are unsafe to call, which our one module does |
| `thiserror` 2 | Typed errors | MIT OR Apache-2.0; 2.0.21; 1.8G downloads; `rust-version` 1.77 at 2.0.21; a derive, no unsafe in what it generates |
| `tempfile` 3 (dev) | Tests' temporary directories | MIT OR Apache-2.0; 3.27.0, 2026-03; 970M downloads; `rust-version` 1.63 |
| `rusty-fork` 0.3 (dev, `timeout`) | A child process per test, its environment set at spawn, a timeout | MIT OR Apache-2.0; 0.3.1, 2025-10; 196M downloads (proptest's); `#![deny(unsafe_code)]`; pulls `fnv`, `quick-error`, `tempfile`, `wait-timeout` |

Each is checked again at the step that adds it (its version, what `cargo tree` shows it pulls); a
mature crate that does part of the work better is taken over our own. Studied, never copied: libinsane,
NAPS2, simple-scan and sane-backends' code (GPL or LGPL); `saneopts.h` (GPL with SANE's exception).
Cross-checked, permissive: printscan-sane's `protocol.rs` and `scan/frame.rs`, janus-scan's tests against
`test:0`. The two patterns taken (airbridge's alive flag, sanetui's header contract) are each a few lines of
idea, so both are written in our own code and credited where they sit: sanetui is Apache-2.0 alone, and
airbridge says MIT OR Apache-2.0 in its manifest but Apache-2.0 on GitHub (the survey), so code copied from
either would carve a file out of the crates' MIT OR Apache-2.0. airbridge is still read in full before its
pattern is followed (the survey read only its cancel path).

## Steps

**At every step**, the checking agent runs the core, once: `just ci` (fmt, clippy `-D warnings`, test, doc
`-D warnings`, gates, `sane-dl-msrv` on its installed toolchain), from step 2 with
`SANE_DL_REQUIRE_LIBSANE=1` and `SANE_DL_REQUIRE_HEADER=1` (the header is installed here, so the contract
runs here and is recorded as run); `cargo test --locked --workspace -- --list` recorded under
`scratch/work/s0-1/` and compared with the step before's (none lost); the commit's diff and message scanned
for local paths, the username, the downstream app's name outside `docs/pm/`, and pdf-lint's unpublished
plan. Below, each "Done when" says only what the step adds; a check whose subject a step doesn't touch is
left out, with why.

1. **The workspace, and sane-dl's empty crate.** Everything in "The workspace" above: `/Cargo.toml`,
   `/Cargo.lock`, licenses, `.gitattributes`, `/README.md`, `/justfile` with its gates, CI; `sane-dl` with
   its manifest (its dependencies named, `rust-version` from the probe, `publish = false` until step 5) and
   a crate doc; `fake-libsane` with its manifest and a crate doc. The MSRV probe. Ask 2 recorded in
   `decisions.md`.
   **Done when:** the core; the probe recorded in `scratch/work/s0-1/msrv-probe/` (each package's
   `rust-version`, normal and dev graphs on every target) and `just sane-dl-msrv` passing on that toolchain;
   `cargo tree -p sane-dl --target all -e normal,build` read against the devices layer's rule; the user's
   own cargo home unchanged by every build (`CARGO_HOME` at `scratch/cargo-home`); the gates passing with an
   untracked file present, and the `readelf` gate seen to read the test binaries and the fake's `cdylib`
   (the count it prints); each gate failing on a violation made for the check on a worktree under
   `scratch/work/s0-1/` (an image codec, a PDF crate, a GUI toolkit and an OCR runtime under `sane-dl`; a
   build script in `sane-dl` linking libsane (`cargo:rustc-link-lib=sane`, which links here now that
   `libsane-dev` is installed) and one in the fake; a manifest declaring `links = "sane"`; `fake-libsane`
   publishable, and taken as a dev-dependency; a tracked file and an untracked one naming the downstream
   app, beside a URL with the org's name that passes), the worktree removed and the branch checked
   unchanged: once, here, where the gates are new, the crates small, and before step 2's FFI code needs
   them; the CI file read against S1 slice 1's decisions 5 and 6 (the MSRV pin and its note, permissions,
   the apt step).
2. **Bindings, loading, devices, handles; the fake's first cut; the header contract.** `ffi` with the
   header's notice; `Status`, `FrameFormat`, `Version`; `Sane` (loading, the one-per-process rule, the
   lock, never unloaded), devices under a deadline, `open` and close, `ConfigDir` and `backend_of`; the
   fake's exports (its declarations from `ffi.rs`) with the script, the log and the rule checks, and its
   hostile `sane_init`, `sane_open`, `sane_strstatus` and device lists, and a slow listing; the tests'
   child-process (skip code, timeout) and fake-building helpers; tests: units, the fake (deadline,
   `ListingPending`, a handle dropped and the `Sane` dropped during a late listing, every hostile list,
   versions, each rule check), real libsane (`test:0`, `test:1`, `pnm:0`, `pnm:1` listed in an isolated
   directory, opened and closed, a name that isn't there answered by `sane_open` with its status, `INVAL`
   expected), loading itself ("How the tests run"), and the header contract.
   **Done when:** the core, the header contract run here against the installed 1.4.0 header and passing,
   and failing once on a constant changed on a worktree copy (`git worktree add` under `scratch/work/s0-1/`,
   removed after); a skipped test seen to say so on a pass (the real-libsane tests with `SANE_DL_LIBSANE`
   at a path that isn't there, and without the require variable); the fake's rule checks each shown
   catching its violation; `cargo clippy --target <t> -p sane-dl -p fake-libsane --all-targets -- -D
   warnings` and `cargo doc` with `-D warnings` for `x86_64-pc-windows-gnu` and `x86_64-apple-darwin` (the
   default names and the fake's paths differ by system); listing through `sane-dl` timed by a probe under
   `scratch/work/s0-1/` (not a test, since it reads the system's directories), isolated (`test` and `pnm`)
   and with a one-backend `BeforeSystem` directory, beside the system's 9.5 s (Baselines); every `// SAFETY:`
   comment read for truth, not only present.
3. **Options, typed.** Descriptors, constraints, units, `Value`, `get`, `set`, `set_auto`, the refusals,
   `SetInfo` and the re-read on `RELOAD_OPTIONS`, `Fixed`, the caps; the fake's profiles, its descriptors
   that change after a set, and its hostile descriptors. Tests: the `test` backend with
   `enable-test-options` (every type and constraint, `int-inexact` answering `INEXACT`, the hard-select and
   soft-detect options, the button, the groups, the gamma tables' arrays), `pnm`'s `filename` set and read
   back and its `resolution` word list set to `auto`, and `set_auto` refused for an option without
   `SANE_CAP_AUTOMATIC`; every profile read as typed; each refusal shown in the fake's log as no call
   reaching the backend; each hostile descriptor a `BadDescriptor`, a `BadValue` or an unusable option,
   never a panic, a read of a constraint's pointer as the wrong member, or an allocation past its cap.
   **Done when:** the core, and the descriptors as `sane-dl` reads them for `test:0` (with
   `enable-test-options`) compared once with `scanimage -A`'s listing of the same (`sane-utils` is installed
   here). The other targets' clippy and docs are left out unless the step adds code behind `cfg`.
4. **Scanning: frames, batches, cancel.** `start`, `parameters`, `read`, `set_io_mode`, `select_fd`,
   `cancel`; the batch and `NO_DOCS` from either call; page assembly with `on_progress`, its checks and
   caps; `INVAL` from `sane_start` re-reading descriptors; the cancel token and its flag (airbridge read in
   full first). The fake's feeds (sheets, duplex marks, a jam or open cover at sheet N, `2500x0`, the
   hangs, the abort) and its hostile frames and statuses; its checks of `sane_cancel` after errors and
   batches, and of cancels after close. Tests, through the `test` backend: the feeder's 10 sheets then
   `NO_DOCS`, again after reopening; gray and color at 1, 8 and 16 bits; `hand-scanner`, alone and from the
   feeder (`lines = -1`); `read-limit` (short reads); `three-pass` in all six orders; `ppl-loss`;
   `invert-endianess`; `read-return-value` at each status; `non-blocking` with `select-fd`;
   `fuzzy-parameters` (estimates before `sane_start`, exact after). Through `pnm`: PBM, PGM and PPM images the
   test writes (16-bit too, if `pnm` reads them), scanned and compared byte for byte, with `three-pass`
   (green, blue, red) and `hand-scanner`. Through the fake: a cancel during a blocked read returns
   `Cancelled`; a cancel between pages stops the next one; a cancel racing close never reaches a closed
   handle, over at least 10,000 rounds; a read that ignores the cancel stops on the token's flag; the abort
   ends the child (by signal on Unix, by its exit code on Windows), and the test says so; every hostile
   frame an error carrying what SANE said.
   **Done when:** the core; `sane-dl`'s assembled bytes equal `scanimage`'s for the same settings (gray 8,
   1-bit gray, color 16 three-pass, `ppl-loss`, `hand-scanner` from the feeder), once, through a check kept
   under `scratch/work/s0-1/` and not committed (16-bit compared after `scanimage`'s byte order; for
   `ppl-loss`, after dropping the padding `scanimage` keeps: it writes each padded row under a header giving
   the unpadded width, Baselines); the cancel tests once under ThreadSanitizer on nightly (`rust-src` is
   installed), the fake built plain by the helper; an endless stream stopped at a small cap with the memory
   scope untouched; clippy and docs for the other two targets (`select_fd` is Unix only, and no doc link
   may reach it from docs built on Windows); what this step learned of the seam's SANE unknowns written
   into "So far" for slice 2: the names seen and what in them is a backend's or an index, and what ending a
   batch after a page does to the `test` backend's feeder.
5. **sane-dl ready to publish; the plan's files; the slice's end.** `sane-dl` at `0.1.0-alpha.1`, publish
   allowed: its README (what it is, loading and its errors, the threading and cancel rules, the caps, why
   SANE belongs in a process of its own, the licenses at the root), a `CHANGELOG.md`, `include` (sources,
   README, changelog), `documentation`, `keywords`, `categories`. A `sane-dl-package` recipe after
   pdf-lint's `_package` (a fresh target directory, `target/sane-dl-package` unless `CARGO_TARGET_DIR`
   names another, `cargo publish --dry-run --locked`, which asks the registry and so needs the network, the
   package's file list held to sources, README, changelog, manifest and lock), in `ci` and as a CI job. The
   plan's files: `status.md`; `decisions.md` (the fake's home; `sane-dl`'s MSRV); `crates/sane-dl.md` as
   built; `crates/scan-fixtures.md`, `crates/README.md`, `plan.md` and `milestones/s0-devices.md`'s slice 1
   (the fake its own crate); `slices/README.md`; the backlog: TEST-2 with what's left (CI's run) and the
   new items (below), the counts redone or left out.
   **Done when:** the core; `just sane-dl-package` with `CARGO_TARGET_DIR` at a fresh
   `scratch/work/s0-1/package/`, a check, never a publish, and the package's file list read; the gates'
   make-fails not repeated (step 1's), unless this step changes a gate; every commit message of the branch
   read for the name and private paths. What only CI shows, said: Windows and macOS building and testing
   (the fake as a DLL and a dylib, rusty-fork's children there, `just` under Git's `sh` on Windows),
   Ubuntu's `libsane1` and `libsane-dev` versions with the `test` and `pnm` backends in them and the header
   contract against that header (here it ran against 1.4.0's), the MSRV job, and the package job.

**Backlog items the slice adds** (step 5, with the next IDs): trying Homebrew's `sane-backends` on the macOS
runner (the open point in `crates/sane-dl.md`; `Sane::load()`'s bare `libsane.1.dylib` goes through dyld's
search, which on Apple silicon doesn't include Homebrew's `/opt/homebrew/lib`, so the item says whether
`load()` needs a second name or the app's path); an authorization callback, if a networked backend ever needs
one; the `test` backend's resolution default (below), for Greg's OK before any report upstream; and whatever
else the steps find.

## Baselines

- **The MSRV probe** (`scratch/work/s0-1/msrv-probe/`): each package's `rust-version` over `sane-dl`'s own
  normal and dev graphs, on every target; built and tested on the result.
- **SANE as installed here** (2026-11-08): `libsane1`, `libsane-common`, `sane-utils` and, since the plan was
  drawn, `libsane-dev` (Greg, the control file's note), all 1.4.0-1ubuntu1: `libsane.so.1` with the `test` and
  `pnm` backends, and `/usr/include/sane/sane.h`. Observed with `scanimage` through a private
  `SANE_CONFIG_DIR` (`scratch/work/s0-1/probe-sane/`):
  - listing with the system's configuration: 9.54 s, nothing attached; an isolated directory naming `test` and
    `pnm`: 0.17 s; `BeforeSystem` with an empty `dll.d/`: 0.01 s, and libsane's debug log says it read that
    `dll.d/` and no other; the same without `dll.d/`: 1.07 s (the system's `dll.d` adds `airscan` and
    `hpaio`);
  - the `test` backend's `hand-scanner` with its feeder: "variable height" (`lines = -1`) on every sheet, so
    TEST-2's question is answered for 1.4.0; its feeder: 10 sheets, then `NO_DOCS` from `sane_start`;
  - `pnm`'s `filename` option (default `/tmp/input.ppm`, so tests always set it), its `three-pass` (green,
    blue, red) and `hand-scanner`, and statuses forced after `sane_read`;
  - `test:0` opens with its fixed-point `resolution` reading 50/65536 dpi (`scanimage` shows `[0.000762939]`
    and scans 1×1 pixels until it's set): the integer 50 in a fixed-point option, a backend bug on 1.4.0.
    Tests set the resolution; `sane-dl` reports the value as the descriptor types it;
  - `ppl-loss`: the `test` backend keeps `bytes_per_line` and lowers `pixels_per_line` (3 lost: 16 pixels in
    19 bytes), and `scanimage` writes the padded rows as they come under a header giving the unpadded width
    (`16 9`, then 171 bytes of samples), so its output isn't a well-formed PGM there
    (`scratch/work/s0-1/review-probe/scan/`).
- **`sane.h` 1.4.0:** installed here by `libsane-dev` (`/usr/include/sane/sane.h`), byte for byte the header
  at sane-backends' tag 1.4.0 (commit `c7e4b5e3`, also fetched 2026-11-08 to `scratch/work/s0-1/sane-h/`; both
  SHA-256 `28833486…`): the bindings' source and the header contract's local run. CI's run uses Ubuntu's
  `libsane-dev`, whatever version `ubuntu-latest` has.
- **`scanimage`** (sane-utils 1.4.0, run as a separate process): the reference frontend for step 4's
  byte-for-byte check and step 3's descriptor listing.
- **The fake's logs:** each scenario's expected call sequence, asserted by the tests.
- **sheaf:** no comparison; nothing is cut from it.

## Decisions taken alone, for Greg to review

1. **The fake libsane is `fake-libsane`, a crate of its own for good** (`/crates/fake-libsane/`, never
   published), not moved into `scan-fixtures` in S1: a shared object loaded by path, kept free of the image
   codecs `scan-fixtures` will take. The plan's files say so with step 5.
2. **Tests build the fake themselves** (nested `cargo build` into the test's own target directory, with the
   toolchain that built the test), so `cargo test` alone works, locally and in CI, and never loads a stale
   copy.
3. **A child process per test that loads a library** (rusty-fork, vetted above), its environment set at
   spawn and a timeout on each, since SANE is one `sane_init` per process and reads its configuration once.
4. **One live `Sane` per process; every call but `sane_cancel` under one lock; the library never unloaded.**
   SANE's rule that no call overlaps another then holds whatever a backend's own thread safety, at the cost
   of two handles' calls waiting on each other, which a worker with one scanner never sees.
5. **A listing past its deadline keeps the library**: later calls say `ListingPending` instead of
   blocking, a handle dropped meanwhile is closed by the listing's thread when it returns, and that thread
   exits SANE if the `Sane` is gone. A hang that never returns is the worker's watchdog's.
6. **`sane-dl` never sets an environment variable**: `ConfigDir` gives the value; the caller applies it to
   the process it spawns. `BeforeSystem` keeps the backend's own configuration file reachable; the empty
   `dll.d/` keeps the system's snippets out.
7. **Page assembly is strict:** a short or long frame, a partial last row, `lines = 0`, or a gray frame that
   isn't last is an error carrying SANE's parameters, never a page. Brother's `2500x0` is so a
   `BadParameters`; the backend ends the batch on it. To revisit if a real backend sends partial rows.
8. **Samples stay as SANE encodes them** (1-bit gray 1 black; 1-bit RGB 0 the least; 16-bit in the host's
   order), padding dropped and frames interleaved; the model fixes its own layout in slice 3.
9. **Option access is checked before the backend sees it** (inactive, not settable, wrong type, length or
   size), as libinsane learned that touching inactive options crashes some backends.
10. **No authorization callback** in this slice: a backend asking for a password gets none.
11. **Default library names by system,** versioned only (`libsane.so.1`, `libsane.1.dylib`), no fallback to
    an unversioned `libsane.so`, none on Windows.
12. **Skipping is said, and CI can't skip:** tests that need libsane or the header write their reason
    straight to stderr and pass where those are absent (a forked test's child exits 77 and its parent says
    why, since rusty-fork captures the child's output); `SANE_DL_REQUIRE_*` in CI's Ubuntu job, and in the
    checking agents' runs here, turn a skip into a failure.
13. **The header contract compiles a C program** that prints the header's values, sizes and offsets and
    asserts the functions' types, spelled with the header's typedef names from a table tied to the
    bindings' Rust types, with a pass over its text for completeness, rather than parsing C alone.
14. **No `saneopts.h`:** it's GPL with SANE's exception, not public domain. The standard option names wait
    for slice 5's `sane` backend, in our own words from the standard.
15. **`sane-dl` 0.1.0-alpha.1, ready to publish, not published**, after pdf-lint's alphas, with its package
    checked in `just ci` and CI; publishing is Greg's word.
16. **The MSRV job runs without libsane;** its libsane tests say they skipped. The check job's Ubuntu leg
    runs them on stable with the require variables.
17. **No `[workspace.dependencies]` yet** (nothing shared); `[workspace.lints]` for the unsafe rules both
    crates take.
18. **macOS with Homebrew's sane-backends isn't tried in this slice:** a backlog item, so an unknown on a
    runner can't hold the slice.
19. **`ubuntu-latest`, its libsane version printed in the log;** the tests rely on nothing 1.4.0 alone does.
20. **The bindings are public** (`sane_dl::ffi`, types and constants only), so the header contract runs as
    an integration test and SANE's raw names are there for anyone; the fake takes the same file by `#[path]`
    rather than a copy of its own.
21. **Errors `#[non_exhaustive]`, plain data not**, so slice 5's `sane` backend can build descriptors,
    parameters and images in its tests without a library.
22. **The two permissive patterns are written in our own code** (airbridge's alive flag, sanetui's header
    contract), credited, so no file of the crates is Apache-2.0 alone.
23. **The gates' make-fails run once, in step 1,** where the gates are new, rather than at the slice's end,
    so the `readelf` gate is proven before step 2's FFI code leans on it.
24. **The cancel token's flag is sticky** until a call returns `Cancelled` for it; page assembly reads
    blocking; `set_auto` takes no value and needs `SANE_CAP_AUTOMATIC`.
25. **The name gate ignores case** (step 1): the stash's pattern as it was, with `grep -i`, so a sentence
    that starts with the downstream app's name, capitalized, is caught too; the org's name in a URL still
    passes.

## For Greg (security)

1. **Loading a library runs its code** in the process: `load_from` trusts the path it's given, and the app
   chooses it (an override variable is the app's to guard). The default goes by soname through the system
   loader's own search; there is none on Windows, so no DLL search order is involved.
2. **Backends are C code running in-process.** `sane-dl` checks and caps every integer, pointer and length
   that crosses, but can't contain a backend that corrupts memory or crashes ("some corrupt memory, some
   cause crashes", libinsane): its README says to run SANE in a process of its own, as `scan-worker` will.
3. **No credentials pass through `sane-dl`** (no authorization callback); the `net` backend's passwords go
   in plain text or MD5 (the survey).
4. **The configuration directory chooses which backends load:** `ConfigDir` writes only names it has
   checked, into a directory the caller makes private (tests use 0700 temporary directories); whoever sets
   `SANE_CONFIG_DIR` for a process has the same power as whoever sets its library path. `BeforeSystem` (and
   SANE without `SANE_CONFIG_DIR` at all) also reads backends' configuration from the process's current
   directory before `/etc/sane.d` (`sane-dll(5)`): the worker should start in a directory it trusts, said in
   `ConfigDir`'s docs; `Isolated` reads only its own directory.
5. **The `pnm` backend reads any file its option names;** only the tests' isolated directories enable it,
   and `sane-dl` never does (upstream disables it by default for that reason).
6. **The fake reads a script and writes a log from environment variables** the tests set; it is never
   published, and a gate keeps every crate from depending on it.

## Risks

1. **CI's libsane isn't this machine's:** `ubuntu-latest` may ship an older sane-backends than 1.4.0, whose
   `test` or `pnm` backend behaves differently (hand-scanner with the feeder, the resolution bug, `pnm` built
   at all). The require variables make it a failure, not a skip; the tests then follow the older behaviour,
   or the fake covers it, said.
2. **The nested build of the fake** may be slow the first time, contend for cargo's package cache lock, or
   pick the wrong toolchain under `cargo +1.89`; `env!("CARGO")` and a target directory of its own are the
   answers, the first CI run the proof.
3. **`just` on Windows** runs recipes, and the justfile's backtick variables, under Git's `sh`; S1 slice 1's
   CI never ran, and pdf-lint's CI has no Windows runner to learn from (it lints for Windows from Ubuntu).
   If it fails, the Windows job runs the recipes' cargo lines directly.
4. **rusty-fork may not fit** (its child's test name, its environment hook, output relayed); then a small
   re-execution helper of our own, said in "So far".
5. **The cancel token's synchronization** is concurrency code over C: the race test, the fake's checks and
   one ThreadSanitizer run are the evidence; none proves it for every backend.
6. **Exports from a Rust `cdylib`** on Windows and macOS, and loading it by path there, are seen only in
   CI.
7. **Memory:** frames up to the 1 GiB cap; tests of endless streams use small caps, under the memory scope.
8. **Backends that never return from `sane_exit` or `sane_close`** would hang a child; the timeouts bound it
   in tests, the worker's watchdog in use.

## Plan review

2026-11-08, before step 1, through three lenses: fidelity (slice 1 exactly, the spec, the seam, the rules),
mechanics (each step green on three systems), behaviour and proof (what each check can show, the FFI's
contracts). Each finding was checked against the sources or a probe (`scratch/work/s0-1/review-probe/`)
before it was acted on; the plan as drawn is kept at `scratch/work/s0-1/plan.before-review.md`.

**Accepted:**

1. *Behaviour, medium.* `libsane-dev` 1.4.0 is now installed here (the control file's note), and the plan
   assumed it absent: the core pointed `SANE_DL_SANE_H` at the fetched header, Baselines said "not
   installed", a make-fail relied on there being no `libsane.so`. Now: the bindings are written from
   `/usr/include/sane/sane.h` (the same SHA-256 as the tag's copy, checked), the contract runs here and is
   recorded as run, and the make-fail links `-lsane`.
2. *Mechanics, medium.* The `readelf` gate would never have read the fake: `cargo test --no-run` builds a
   `cdylib`'s unit-test harness, not the shared object (a probe's JSON lists no `.so`). The gate takes the
   `cdylib` from `cargo build --workspace`, prints how many files it read, and fails if `readelf` is
   missing.
3. *Mechanics, medium.* A skip said in a forked child is never seen on a pass: rusty-fork sends the child's
   stdout and stderr to a file it relays with `print!`, which the harness captures (`fork.rs`, read in
   `scratch/work/s0-1/vet/`). The child exits 77 and the parent says why on its own stderr, or fails under
   the require variable; step 2 checks a skip is seen.
4. *Mechanics, medium.* The header contract couldn't reach private bindings from `tests/`, and
   `CARGO_TARGET_TMPDIR` exists only for integration tests: `ffi` is public (types and constants only).
5. *Mechanics, medium.* No `#[non_exhaustive]` policy: errors are, plain data isn't, so slice 5's backend
   can build descriptors and images in its own tests, and `tests/` matches errors with a wildcard.
6. *Behaviour, medium.* The fake, with no dependencies, would have declared SANE's types in a second copy the
   header contract never checks: it takes `ffi.rs` by `#[path]` and asserts each export's type.
7. *Behaviour, medium.* The gates' make-fails sat in step 5, after steps 2 to 4 had leaned on the gates (the
   `readelf` gate matters most when step 2 writes the FFI): moved to step 1, once, where the crates are
   small; step 5 repeats them only for a gate it changes.
8. *Behaviour, medium.* Step 4's byte-for-byte check against `scanimage` can't pass for `ppl-loss` as
   written: `scanimage` writes the padded rows under a header giving the unpadded width (seen: `16 9`, 171
   bytes). The check drops that padding first; the fact is in Baselines.
9. *Mechanics, minor.* GCC and Clang take `SANE_Status` as compatible with `unsigned int`, not `int` (a
   probe), so the contract spells types by the header's typedef names, from a table tied at compile time
   to the bindings' Rust types; `__typeof__(&sane_open)` references no symbol (`nm`), so nothing links.
10. *Mechanics, minor.* The fake's nested build inherits `RUSTFLAGS`, and under step 4's ThreadSanitizer run
    a plain build fails ("mixing `-Zsanitizer` will cause an ABI mismatch", a probe): the helper clears the
    flags and the build target for it.
11. *Mechanics, minor.* clippy refuses the fake's exports in either plain form (`not_unsafe_ptr_arg_deref`
    for a safe one, `missing_safety_doc` for an undocumented unsafe one, a probe): `unsafe extern "C"` with
    `# Safety` sections, in one module.
12. *Mechanics, minor.* A doc link to `select_fd` (`cfg(unix)`) breaks `doc -D warnings` on Windows: steps 2
    and 4 build the docs for the other two targets as well as linting them.
13. *Mechanics, minor.* `/Cargo.lock` wasn't named, though every recipe is `--locked`: step 1 makes and
    tracks it.
14. *Mechanics, minor.* The package recipe's directory couldn't be put under `scratch/work/s0-1/package/`
    for the check, and its dry run needs the network (pdf-lint's `_package` says so): `CARGO_TARGET_DIR`
    overrides its default.
15. *Fidelity, minor.* `set_auto` took a `Value` and wasn't limited to options that can pick their own
    value: the standard ignores the value for `SET_AUTO` and allows it only with `SANE_CAP_AUTOMATIC`
    (`pnm`'s `resolution auto` is the case to test).
16. *Fidelity, minor.* `Error::Busy` would read as SANE's `DEVICE_BUSY`, which the seam's section 6 maps to
    the model's `Busy`: renamed `ListingPending`, and the docs say a listing and a scan in one process wait
    on each other.
17. *Fidelity, minor.* Slice 5 is to drive the fake from `scan-device`'s tests, but its script format was
    unwritten and the gate forbids depending on it: the formats are documented in its crate docs, and the
    helper is slice 5's to copy or share.
18. *Fidelity, minor.* Porting sanetui's Apache-2.0-only code (or airbridge's, whose licence reads two ways)
    would carve a file out of MIT OR Apache-2.0: both patterns are written in our own code and credited.
19. *Fidelity, minor.* Risk 3 fell back on "pdf-lint's Windows job", which doesn't exist (every job of its
    CI runs on `ubuntu-latest`): the fallback is the recipes' cargo lines.
20. *Behaviour, minor.* `BOOL` words and `last_frame` are integers from C that would have become Rust bools
    unchecked: anything but 0 or 1 is `BadValue` or `BadParameters`.
21. *Behaviour, minor.* The constraint union could be read as a member its value type can't take (the
    standard allows ranges and word lists on `INT` and `FIXED`, string lists on `STRING`): any other
    pairing makes the option unusable, its pointer unread.
22. *Behaviour, minor.* Contracts left unnamed: a `GOOD` `sane_open` with a null handle (`BadHandle`), a
    null device list (`BadDeviceList`), a null `sane_strstatus` (`None`), and the select fd, the backend's
    and valid only until the next start, cancel or end of frame (a `RawFd`, never closed or wrapped).
23. *Behaviour, minor.* Failed loads weren't said to clean up, and `MissingSymbol`, `AlreadyLoaded` and a
    load after exit were untested: a refused version is exited, the slot freed, each tested.
24. *Behaviour, minor.* A handle dropped during a late listing would have blocked, though calls "never
    block" then: its close goes to the listing's thread.
25. *Behaviour, minor.* The frame checks missed a three-pass image that ends without a channel and a gray
    frame after a single-channel one; and the cancel flag's lifetime, page assembly in non-blocking mode
    (a spin) and `on_progress` under the lock were unsaid. Each said now.
26. *Behaviour, minor.* "`sane_cancel` after every error status" read two ways (after a failed
    `sane_control_option` too?), and the fake's check would have held the library to one of them: both say
    the scan calls.
27. *Behaviour, minor (security).* `BeforeSystem` also reads configuration from the current directory
    before `/etc/sane.d` (`sane-dll(5)`): in `ConfigDir`'s docs and "For Greg (security)" 4.
28. *Behaviour, minor.* A 1200 dpi 16-bit Letter page is 0.75 GiB (807,840,000 bytes), not 0.8.

**Rejected:**

1. *The MSRV probe is circular under resolver 3*, which picks versions fitting the `rust-version` declared:
   the plan measures as S1 slice 1 did, and that probe (`scratch/probe/`) declares none, so it resolves the
   newest versions; the real lock is then resolved with 1.89 in place. Nothing to change.
2. *`BeforeSystem`'s trailing `:` is wrong on Windows:* `sane-dll(5)` gives `:` on Unix-like systems and `;`
   only on OS/2, and `sane-dl` has no default library on Windows.
3. *The nested `cargo build` deadlocks on cargo's locks:* its own target directory keeps it off the build
   directory's lock, as trybuild's tests run cargo; the package cache is Risk 2's, the first CI run its proof.
4. *Install libsane in the MSRV job too:* the check job runs the same tests with libsane on stable; what
   1.89 can break is compiling, which the MSRV job shows without it.
5. *Making the fake its own crate for good reaches past slice 1:* the slice's notes leave its name and place
   to the plan, the plan's files must say where it lives, and it's decision 1, for Greg's review.
6. *A gate that the fake takes no dependency:* it has none, and S1 may want `scan-fixtures`' pages behind
   it; a gate would bind a later slice for little.

## So far

### Step 1: the workspace, and sane-dl's empty crate (2026-11-08)

Commits `d0941e1` (the workspace) and the one that carries this account (ask 2 recorded, this section,
and the repair after the check).

- **What it does:** `/Cargo.toml` (resolver 3; members `crates/sane-dl` and `crates/fake-libsane`; no
  `[workspace.dependencies]`, its comment saying when it returns; `[workspace.lints]` denying
  `unsafe_op_in_unsafe_fn` and `clippy::undocumented_unsafe_blocks`, taken by both crates); `/Cargo.lock`
  (resolved with 1.89 declared: the same versions as the probe's newest, libloading 0.9.0, thiserror
  2.0.21, rusty-fork 0.3.1, tempfile 3.27.0); `LICENSE-MIT` and `LICENSE-APACHE` as stashed;
  `.gitattributes` with the PNM types; the root README, scanner access first; the justfile; CI.
  `sane-dl` 0.0.0, `publish = false`, `rust-version = "1.89"`, its four dependencies named, a crate doc
  and `#![deny(unsafe_code)]` (the design's root rule, ahead of step 2's one module that allows it);
  `fake-libsane` 0.0.0, `cdylib`, `publish = false`, no dependencies, a crate doc, the same root rule.
- **The MSRV probe** (`scratch/work/s0-1/msrv-probe/`, its README): over `sane-dl`'s own graph, every
  target, normal and build: libloading 0.9.0's 1.88.0 is the highest; with dev too, still 1.88.0
  (getrandom 0.4.3 1.85 next; rusty-fork, fnv, quick-error and wait-timeout declare none). So 1.89, the
  floor; the probe built and tested on 1.89 at the newest versions. The fake has no dependencies: 1.89.
- **The gates** (`just gates`, needing `jq` and `readelf`, and failing at once, saying so, without
  either): the devices layer's patterns over `cargo tree -p sane-dl --target all -e normal,build`;
  `links = "sane"` over `cargo metadata`'s whole graph; `readelf -d` over the test harnesses (`cargo test
  --no-run`, `profile.test` artifacts) and the workspace's `cdylib`s (`cargo build`, by target kind),
  printing the count and failing if it's 0 (here 3: both crates' unit-test harnesses and
  `libfake_libsane.so`); `fake-libsane`'s `publish` and every member's dependencies of any kind from
  `cargo metadata --no-deps`; the name gate. They passed here with the new files still untracked.
- **CI:** the check job on the three systems, its Ubuntu leg installing `libsane1` and `libsane-dev`
  (`--no-install-recommends`), printing their versions, failing if `libsane1` lacks the `test` or `pnm`
  backend, and setting `SANE_DL_REQUIRE_LIBSANE=1` and `SANE_DL_REQUIRE_HEADER=1` for the steps after;
  the gates job with both installed; `sane-dl-msrv` on `dtolnay/rust-toolchain@1.89`, its note to keep
  it in step with `rust-version`. Read-only permissions, credentials not persisted, as S1 slice 1's
  decisions 5 and 6.
- **Ask 2** recorded in `decisions.md` (folded into "Asks 2, 6 and 7") and gone from `asks.md`; the four
  places that pointed at it as open (`plan.md`, `sheaf/from-sheaf.md`, `milestones/later.md`, `status.md`)
  now point at the decision, and `status.md`'s first line says the slice is building.
- **Departures from the plan, and why:**
  - *A make-fail as worded can't fail:* a build script's `cargo:rustc-link-lib=sane` alone links
    nothing, since rustc passes `-Wl,--as-needed` first and nothing references libsane, so no binary
    needs it and the gate rightly passes (a probe, `scratch/work/s0-1/step-1/make-fail.md`; mold, the
    linker here, also folds a second `-lsane` into the first). The make-fail that does link prints
    `cargo:rustc-link-arg=-Wl,--no-as-needed,-lsane`. The gate reads what binaries need, so a reference
    to libsane in code no test reaches would slip past it (the linker drops dead code); the `links`
    check, and `sane-dl` taking SANE's functions only as pointers from the loader, never as symbols,
    cover that.
  - *The name gate ignores case* (decision 25).
  - *The require variables* are set by the step that installs SANE (`$GITHUB_ENV`), so they hold exactly
    where SANE is installed, rather than on the test step by matrix.
  - *`fake-libsane`'s checks read `cargo metadata`* (its `publish` and every member's declared
    dependencies, dev and build included) rather than grepping the manifest and `cargo tree -i`, which
    sees only resolved normal and build edges.
  - *Crate READMEs and changelogs* wait for step 5, as the plan has them.
- **Checked here:** `cargo fmt --check`, `clippy -D warnings`, `cargo test` and `cargo doc -D warnings`
  (the justfile's recipes), `just gates`; the user's cargo home unchanged (names and times to depth 3,
  before and after). Left for the checking agent: `just ci` with `sane-dl-msrv` on 1.89, the test list,
  the gates' make-fails, `cargo tree`, the CI file's reading.
- **Checked** before the repair (`scratch/work/s0-1/flow/checks.md`, "Step 1"): `just ci` with
  `sane-dl-msrv` on 1.89; the test list (0 tests in 3 harnesses; no workspace before, so none lost);
  `cargo tree -p sane-dl` against the devices layer's rule; the MSRV probe redone from the real lock's
  metadata (libloading's 1.88.0 the highest, so 1.89) and the fake checked on 1.89; `cargo check` for
  `x86_64-pc-windows-gnu` and `x86_64-apple-darwin`; the user's cargo home unchanged; the gates with an
  untracked file (3 binaries read), and every make-fail on a worktree, each violation named in its own
  message; the CI file against S1 slice 1's decisions 5 and 6; the commits' messages and diff. All passed
  but two, repaired in this commit:
  1. *The devices layer's gate held having read nothing.* Cause: the loop read `deps` through a process
     substitution (`done < <(deps "$crate")`, the stash's construct), whose exit status neither `set -e`
     nor `pipefail` sees: with the loop naming `sane-dll`, cargo said no package matched and the gate said
     "all hold". Fix: the list is taken first (`if ! list=$(deps "$crate")`), so a failing `cargo tree`
     fails the gate; the crate must be in its own list, or the gate fails; the loop reads the list, and
     the gate prints how many packages it read (10 for `sane-dl`), as the `readelf` gate prints its count.
  2. *CI's backend check passed with either backend.* Cause: one `grep -E '(test|pnm)'` over `libsane1`'s
     files, which matches if either is there, so the step failed only if both were missing. Fix: the file
     list taken once into a variable and each backend looked for by name, the step printing the paths it
     finds and failing with the name of the one that's missing; not `dpkg | grep -q`, which can fail on
     SIGPIPE under `pipefail`.
  - *With them, the name gate's swallowed errors* (the check's note, not counted as a failure): `git
    ls-files | xargs grep … || true` held when git or grep couldn't read. Now `git grep -IilP
    --untracked` over the same files (tracked, and untracked but not ignored, outside `docs/pm/`), with
    only its exit 1, no match, allowed; a file deleted from the checkout but still tracked is skipped,
    where `grep` failed on it. One narrowing: `git grep -I` also skips what `.gitattributes` marks binary,
    so an ASCII PNM isn't read, where `grep -I` read one with no NUL bytes in it.
  - *Checked again*, on a worktree under `scratch/work/s0-1/repair-1/` (removed after; the branch, status,
    stash and worktree list unchanged): clean, with an untracked file holding the org's URL and an ignored
    one naming the app, the gates pass (10 packages, 3 binaries); the loop naming `sane-dll` fails ("cargo
    tree couldn't list"); the list's names swapped for versions (`awk`'s second field) fails ("isn't in its
    own dependency list"); `GIT_DIR` pointing nowhere fails (git grep's exit 128); `png` taken by
    `sane-dl`, the name in tracked `lib.rs` and in an untracked file, and `.gitattributes` deleted, fails
    on `png` and those two files alone. CI's step, taken from the file and run under `bash -e` and under
    `-eo pipefail` with stand-ins for `apt-get` and `dpkg`: passes on this machine's `libsane1` list (both
    backends printed, both variables written), fails naming `pnm` on the list without it and `test` on
    the list without it, and fails when `dpkg` does; the step as it was passed on the list without `pnm`.
    Then `just ci` on the committed tree.
