# S0 slice 1: sane-dl, and the workspace it starts

Status: done on branch `s0-1-sane-dl` (2026-11-08): its five steps built and checked, its code review's
findings fixed ("Code review", below). Waiting on review, then the push, the pull request and CI's run on
Ubuntu, Windows and macOS. Milestone: `milestones/s0-devices.md`, slice 1.

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
recommended, each open to review:

- **ask 2:** the cut, the ten crates; this slice depends on its device half, `sane-dl` a crate of its own
  beside `scan-device`. Recorded in `decisions.md` with step 1, and ask 2 leaves `asks.md`.
- **asks 6 and 7** (the names; the standing rules) were taken as this slice started (`decisions.md`, "Made
  here"); asks 3 and 4 were answered by putting S0 first.

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
  Linux and the other Unixes, `libsane.1.dylib` on macOS, none on Windows, where SANE isn't the system's API;
  on macOS by Homebrew's and MacPorts' absolute paths since the code review, decision 63);
  `Sane::load_from(path)` opens the one the caller names (a test's fake; an app's override, whose environment
  variable is the app's), an `unsafe fn` since the code review (decision 62). Loading happens only when asked;
  then `sane_init` with no authorization callback. `LoadError` (thiserror): `NotFound { tried, source }`
  naming the files tried, `NoDefault` (Windows), `MissingSymbol { name }`, `Init(Status)`,
  `UnsupportedVersion(Version)`, `AlreadyLoaded`. One live `Sane` per process (a second load while one lives
  is `AlreadyLoaded`); a later one after the first's `sane_exit` is allowed. A load that fails leaves nothing
  behind: a version refused after a good `sane_init` still gets its `sane_exit`, and every failure frees the
  process's one slot, so a later load can succeed. The library is never unloaded (no `dlclose`): backends may
  leave threads, exit handlers or thread-local destructors in it. Every call but `sane_cancel` holds the
  instance's lock, so no two SANE calls ever overlap, as the standard requires ("the SANE API does not require
  any other operations to be re-entrant"); `Sane` is `Send + Sync`, handles keep it alive, and `sane_exit`
  runs when the last of them goes.
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
    `sane_start` answering `INVAL`, each reload said in what the call returns (as built, after the code
    review: SANE's word in `SetInfo`, a start's in `StartInvalid`, and every re-read, whatever its cause,
    counted by `Handle::options_generation`, decision 65). `Fixed` is SANE's 16.16 word, with `to_f64` and a
    checked `from_f64`. Strings that go back to C (string values, string-list entries, device names) stay
    bytes; text for people is lossy UTF-8.
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
    step-by-step API's. (Corrected in the code review, decision 57: several of SANE 1.4.0's backends answer
    `UNSUPPORTED` to either mode, against the standard, so a page sets blocking mode only on a handle the
    step-by-step calls made non-blocking, a device opening blocking, and goes on whatever that answers.) A
    status mid-page (`JAMMED`, `COVER_OPEN`, `IO_ERROR`, `NO_DOCS` after the first byte,
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
    counts; a stream past the frame cap `FrameTooLarge`, its reading stopped (as built, after the code review:
    `LimitExceeded` for every cap a value passes, decision 67).
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
  `Unusable`); `BadReadLength`, `BadParameters`, `ShortFrame`, `LongFrame`, `PartialRow`, `FrameTooLarge`
  (`LimitExceeded` since the code review, and `ReloadAfterSet` beside it); `Cancelled`; `NulInName`.
  `status_message(Status)` gives `sane_strstatus`'s text, bounded, `None` for a
  null.
- **The API's shape for its callers:** the errors (`LoadError`, `Error`, `ConfigError`) and the status-like
  enums that may grow are `#[non_exhaustive]`; the plain data (`Device`, `Parameters`, `OptionDescriptor`,
  `Constraint`, `Value`, `SetInfo`, `Image`, `Version`) has public fields and isn't, so slice 5's `sane`
  backend can build them in its own tests without a library (`SetInfo` made `#[non_exhaustive]` in the code
  review, built from `Default`, decision 65). Tests in `tests/`, outside the crate, match the
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
  (before any child is spawned, as the child's preparation, which rusty-fork runs in the parent alone,
  since its child runs the test function from the top until the fork; the path then goes to the child in
  its environment), runs `cargo build
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
one; the `test` backend's resolution default (below), for the maintainer's OK before any report upstream; and
whatever else the steps find.

## Baselines

- **The MSRV probe** (`scratch/work/s0-1/msrv-probe/`): each package's `rust-version` over `sane-dl`'s own
  normal and dev graphs, on every target; built and tested on the result.
- **SANE as installed here** (2026-11-08): `libsane1`, `libsane-common`, `sane-utils` and, since the plan was
  drawn, `libsane-dev` (the maintainer, the control file's note), all 1.4.0-1ubuntu1: `libsane.so.1` with the
  `test` and `pnm` backends, and `/usr/include/sane/sane.h`. Observed with `scanimage` through a private
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
  `libsane-dev`, whatever version `ubuntu-latest` has: 1.4.0-1ubuntu1, as here, at CI's first run (Risk 1).
- **`scanimage`** (sane-utils 1.4.0, run as a separate process): the reference frontend for step 4's
  byte-for-byte check and step 3's descriptor listing.
- **The fake's logs:** each scenario's expected call sequence, asserted by the tests.
- **sheaf:** no comparison; nothing is cut from it.

## Decisions taken alone, for review

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
    checked in `just ci` and CI; publishing is the maintainer's word.
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
    blocking; `set_auto` takes no value and needs `SANE_CAP_AUTOMATIC`. (Page assembly sets blocking mode
    only where it was changed, decision 57; a stale flag can be taken back, decision 69.)
25. **The name gate ignores case** (step 1): the stash's pattern as it was, with `grep -i`, so a sentence
    that starts with the downstream app's name, capitalized, is caught too; the org's name in a URL still
    passes.
26. **Device strings past the cap are refused, not cut** (step 2): a list holding a name, vendor, model or
    type longer than `Limits::string_bytes` is `BadDeviceList` whole, since a cut name wouldn't reopen; a
    status's words, text for people only, are cut at the cap.
27. **A listing's deadline counts the wait for the lock** (step 2): one that can't start in time returns
    `DeadlineExceeded` and leaves nothing pending.
28. **`ConfigDir` wants an absolute path** (step 2): SANE resolves a relative one against the directory of
    the process that loads it, which needn't be the caller's. It refuses `:` in the path on Unix (SANE's
    separator) and a `dll.d` already there that isn't empty, before writing anything.
29. **Limits are the instance's** (step 2), set with `Sane::set_limits` for the calls that start after;
    `Limits` is `#[non_exhaustive]`, so steps 3 and 4 add their caps without a break.
30. **The fake checks a little more than the design's rules** (step 2): a call outside `sane_init` and
    `sane_exit`, a second `sane_init`, and a null pointer SANE must write through or read
    (`sane_get_devices`' list, `sane_open`'s handle and name; answered `INVAL`, nothing opened), beside
    the four the design names. A null `version_code` to `sane_init` isn't one: the standard allows it
    (step 2's repair).
31. **`open` takes any bytes** (step 2): `impl AsRef<[u8]>`, so a `&str`, a `DeviceName` or stored bytes;
    `DeviceName` itself is unchecked, and a `NUL` inside is `NulInName` at `open`, before SANE sees it.
32. **An option's descriptor is read again before every call on it** (step 3): its type, size and
    capabilities compared with the copy, and every descriptor read again if any differ, then the request
    checked again; so a buffer is always sized from the descriptor as SANE gives it now, even from a backend
    that changes one without saying `RELOAD_OPTIONS`. One more `sane_get_option_descriptor` per call.
33. **A get needs `SANE_CAP_SOFT_DETECT`** (step 3; `NotReadable`, beyond the design's refusals): the
    standard says such a value can't be read by software. The `test` backend's `bool-hard-select` answers
    all the same (`scanimage -A` shows its `yes`); `sane-dl` doesn't ask.
34. **`set_auto` needs `SANE_CAP_SOFT_SELECT` as well as `SANE_CAP_AUTOMATIC`, and passes a buffer** (step
    3): zeroed, of the option's full size, not null, in case a backend writes the value it chose there. A
    button is pressed with the word 1 (`SANE_TRUE`).
35. **What makes a device's options unreadable, and what makes one option unusable** (step 3).
    `BadDescriptor`, failing `open` or the re-read: what can't stand for an option or for the set (option 0
    not an `INT` of one word, a count below 1 or past `Limits::options`, a null descriptor below the count,
    a name past `Limits::string_bytes`). Unusable, the option kept and never called (`unusable` says why,
    `Unusable` refuses): an unknown type, a size that doesn't fit the type or is past `Limits::option_bytes`,
    an unknown constraint type, a constraint the type can't have, a null constraint pointer, a list past
    `Limits::list_entries` or holding an entry past the string cap. A title or description past the cap is
    cut there (text for people, as status words are); a name is refused, as device names are (26).
36. **A group's name, unit, size, capabilities and constraint aren't read** (step 3): the standard says only
    its title, description and type mean anything, so its pointers may be anything. (Corrected in the code
    review, decision 61: the published standard defines only a group's title and type, so its description
    isn't read either.)
37. **A failed re-read leaves no descriptors** (step 3): every option call is then `NoSuchOption` until
    `reload_options` succeeds, never sized from descriptors that may be stale.
38. **`Fixed::from_f64` rounds to the nearest word** (step 3; `SANE_FIX` truncates), `None` for NaN, the
    infinities, and anything outside a word's range.
39. **Values aren't checked against their constraints, nor names against the standard's spelling** (step
    3): whether a value fits is the backend's to answer (`INVAL`, or rounded with `INEXACT`); Canon DR's
    `ScanMode` has capitals the standard doesn't allow, and is read as it is.
40. **The fake checks the refusals `sane-dl` promises** (step 3): a `sane_control_option` that a careful
    caller refuses (an inactive option, one software can't read or set, a group, an unusable descriptor, a
    `BOOL` word other than 0 or 1, a string with no `NUL`, a call before every descriptor is read again
    after `RELOAD_OPTIONS`) is a violation in its log, beyond SANE's own rules, so every test that uses
    options holds `sane-dl` to them.
41. **A cancel wins over what SANE answers** (step 4): once a token has cancelled, the next start or read
    returns `Cancelled` without calling SANE, and a read in flight returns `Cancelled` whatever it answers,
    a page's last `EOF` and a jam included; the page is dropped.
42. **A batch may go on after an error or `NO_DOCS`** (step 4): `next_page` then starts again (SANE's way to
    carry on after a jam is `sane_cancel`, then `sane_start`); the batch reports, and the caller decides.
43. **`Handle::cancel` leaves a token's flag alone, and the flag outlives a batch** (step 4): a cancel that
    comes after a batch has ended stops the next batch's first page (decision 24, taken as written).
44. **`sane_cancel` before `sane_close` while a scan is pending** (step 4): a handle closed mid-batch ends
    the batch first, as SANE asks of every batch; none when nothing is pending.
45. **`INVAL` from a start is `StartInvalid`**, carrying the re-read's failure, not `Status { INVAL, Start
    }` (step 4), so the call says the descriptors were read again.
46. **Three-pass frames may differ in padding** (step 4), not in pixels, lines or depth; frames of unknown
    height must end at the same row.
47. **An empty buffer reads nothing** (step 4): `read` returns `Bytes(0)` and calls nothing, since the
    `test` backend takes a zero length as the frame's end.
48. **The tests pace the `test` backend** (step 4): 64-byte reads and a pause after each frame's first read,
    against its deadlock; a cancel at once after a start is the fake's to test.
49. **The fake takes a sheet at the start that begins its page, whatever that start answers, and counts
    `at` by pages** (step 4: two a sheet in duplex); its checks go beyond the standard's words (a close with a
    scan pending, a start before the frame's `EOF`, a read before any start), as decisions 30 and 40 did.
50. **The package recipe removes only its own directory** (step 5): `sane-dl-package` under `target/`, or
    under `CARGO_TARGET_DIR` where one is set, rather than `CARGO_TARGET_DIR` itself, so a target directory
    shared by every build (a common setting) survives `just ci`. The check's `CARGO_TARGET_DIR` at a fresh
    `scratch/work/s0-1/package/` puts it at `package/sane-dl-package/`.
51. **The package's list is held both ways** (step 5): nothing but sources, README, changelog, manifest and
    lock (pdf-lint's check), and `src/lib.rs`, the README, the changelog, the manifest and the lock each
    there, which nothing else would see missing. cargo adds a README it finds whatever `include` says, so
    only a README gone altogether trips that one (make-fails M7 and M7b).
52. **The changelog's entry is `0.1.0-alpha.1 (unreleased)`** (step 5), dated when the version is
    published, which waits on review; a unit test holds that the changelog has an entry for the manifest's
    version, so a version bump can't go unsaid.
53. **The README's licence links are the repository's URLs on `master`**, held by a unit test, as pdf-lint's
    are on its branch (the package holds no licence file, and a relative link would climb out of it); the
    README's example is compiled as a doctest (`no_run`, since it needs libsane), written as a whole `main`
    so crates.io, which shows a doctest's hidden lines, shows none (step 5).
54. **Keywords and categories** (step 5): `sane`, `scanner`, `scanning`, `libsane`, `ffi`; `api-bindings`,
    `external-ffi-bindings`, `hardware-support`, `os::unix-apis`. `documentation` is docs.rs.
55. **`fake-libsane` has a spec file of its own** (`crates/fake-libsane.md`, step 5), as every crate does,
    and a row in `plan.md`'s table: the family is eleven crates, nine published (decision 1).
56. **The new backlog items** (step 5) are TEST-7 (Homebrew's sane-backends on macOS), FEAT-11 (an
    authorization callback) and UP-7 to UP-10 (the `test` backend's resolution default, its hang, its -1
    length, and `scanimage`'s 16-bit three-pass abort), each read against sane-backends' master first; the
    upstream ones "waiting on review", as the control file's note words what waits.
57. **A page sets blocking mode only where it was changed, and goes on whatever that answers** (code
    review): a device opens blocking (the standard), so a page calls `sane_set_io_mode(FALSE)` only on a
    handle whose `set_io_mode(true)` answered `GOOD`, and ignores its status. SANE 1.4.0's `fujitsu`,
    `canon_dr`, `kodak`, `epson2`, `epsonds`, `escl`, `hp5590`, `kodakaio` and others, and HPLIP's `hpaio`,
    answer `UNSUPPORTED` to either mode, against the standard's promise; `scanimage` never calls it. A
    backend left non-blocking is read as an empty read is (58). Decision 24 is revised by it.
58. **An empty read is made again after a pause, with no cap** (code review): from 100 µs, doubling to
    10 ms, reset by a read that gives bytes, so a backend answering `GOOD` with no bytes costs no processor
    (34 reads in 300 ms, where reading at once made 17,219). No cap fails the page: a backend that answers
    so while its lamp warms up would fail under one, where `scanimage`, which reads again at once, scans; a
    wait that never ends is a hang, the token's to cancel or the watchdog's, as with a blocking read that
    never returns. The README no longer says the caps bound loops.
59. **A frame of unknown height that ends before its first row is `ShortFrame`**, a row's bytes expected
    (code review): its parameters were fine, so not `BadParameters`; a page always has a row, and
    `Assembly::started` is true after any frame that ends well, so a later `NO_DOCS` is never a feeder's end.
60. **Progress expects what's known** (code review): the frames that ended as they were read, and the
    frame in flight for itself and for each frame to come; never below what's read, exact once the last
    frame begins.
61. **Of a group's descriptor, only the title and type are read** (code review; decision 36 revised). The
    standard as published (sane-project.gitlab.io/standard) defines only those two; the source of its
    current edition counts the description too. The copy reads only what every edition defines, so a
    group's description is empty; `scanimage` doesn't read it either.
62. **`Sane::load_from` is an `unsafe fn`; `Sane::load` stays safe** (code review, before the first
    publish): `load_from` dlopens any path and calls its symbols with `sane.h`'s types, which a safe function
    can't promise (libloading's `Library::new` is unsafe for this; ash and openxr made their loaders unsafe
    for it). Its `# Safety`: a library this process may run, whose SANE functions have `sane.h`'s types.
    `load` opens only the system's own SANE, trusted as a library linked at build time would be, which safe
    Rust does through every `-sys` crate. The tests load through one helper whose SAFETY comment covers the
    libraries they load.
63. **On macOS, `load` tries absolute paths and no bare name** (code review; security item 8):
    `/opt/homebrew/lib/libsane.1.dylib`, `/usr/local/lib/libsane.1.dylib`, `/opt/local/lib/libsane.1.dylib`,
    in that order, since dyld looks for a bare name in the working directory of an unrestricted process.
    `Sane::DEFAULT_LIBRARIES: &[&str]` replaces `DEFAULT_LIBRARY: Option<&str>` (an API change before the
    first publish); empty on Windows, so `NoDefault`. Untried on a Mac (TEST-7).
64. **`links = "sane-dl"`, with a build script that builds nothing** (code review; security item 10): cargo
    then refuses two copies of the crate in one build (shown by a probe depending on 0.1.0-alpha.1 and a
    copy at 0.2.0: "only one package in the dependency graph may specify the same links value"), where each
    copy would keep its own one-per-process slot and start SANE twice. The gate's `links = "sane"` check
    still holds; the package holds `build.rs`, and its recipe expects it; the README no longer says "no
    build script".
65. **Every re-read of the descriptors is counted, and `SetInfo` is `#[non_exhaustive]`** (code review):
    `Handle::options_generation()` counts each re-read, whatever its cause (a set's `RELOAD_OPTIONS`, a
    start's `INVAL`, a changed descriptor before an option call, `reload_options`), so a caller compares it
    around a call. `get` keeps returning a `Value`, and `SetInfo` stays SANE's own word, now made from
    `Default` outside the crate so more can be added. The spec's "said in what the call returns" reads
    accordingly. Decision 21 holds for the rest of the plain data.
66. **A set made before its re-read failed is `Error::ReloadAfterSet { info, reload }`** (code review), as
    `StartInvalid` is for a start: the caller knows the value took effect, and what SANE said of it.
67. **A cap passed is `Error::LimitExceeded { limit: LimitKind, value, cap }`** (code review): a known frame
    or a line past its cap (was `BadParameters`), a frame of unknown height (was `FrameTooLarge`, removed), a
    device list (was `BadDeviceList`), an option count (was `BadDescriptor`). What no frame can have is
    checked first. What makes one option unusable (its size or a list past its cap) stays its `unusable`
    reason: the option is kept and the device opens, and no real backend nears those caps.
68. **A `dll.d` already there that isn't an empty directory is `ConfigError::BadPath`** for the directory
    (code review), not a made-up `Io` error; `Io` is what the system refused.
69. **`Handle::clear_cancel()` takes back a waiting cancel** (code review; decision 43 kept): on the handle,
    whose thread knows when a new batch begins, not on the token, where a clear from another thread would
    race the cancel it's meant for. It returns whether one was waiting; `batch()`'s docs say when to call
    it.
70. **The fake plays both** (code review): `io-mode-unsupported` (`sane_set_io_mode` answers `UNSUPPORTED`
    to either mode, frame or none), which the `fujitsu`, `canon-dr`, `kodak` and `epson` profiles imply as
    those backends answer; and `@dangling`, a pointer to no memory, for a field the standard leaves
    undefined.
71. **The gates read fake-libsane's graph, and each binary whole** (code review): the fake may take no
    dependency; `readelf -d` is read into a variable first, and a binary it can't read fails the gate.
72. **No one is named in a tracked file but the root `LICENSE-MIT`, and no local path is given** (the
    control file's note, the fix's privacy commit): rulings by their date, "waiting on review" for what
    waits, "the maintainer" where who matters; this plan's headings renamed; checkouts by their public
    repositories, files in them by their repository paths.

## For review (security)

1. **Loading a library runs its code** in the process: `load_from` trusts the path it's given, and the app
   chooses it (an override variable is the app's to guard); since the code review it's an `unsafe fn`, so the
   trust is its caller's promise in Rust's terms, not a doc comment (item 9). The default goes by soname
   through the system loader's own search (on macOS by absolute paths, item 8); there is none on Windows, so
   no DLL search order is involved.
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
7. **SANE's `dll` opens any installed backend by name, whatever `dll.conf` says** (step 2's probe, 1.4.0:
   `test:0` opened with a `dll.conf` naming only `pnm`, and with the system's, where `test` is commented
   out). A `ConfigDir` decides what a listing loads, not what `sane_open` can reach, so item 5 holds for
   listings only: `pnm`, which reads any file its option names, can be opened by name in any process
   where `libsane1` is installed. The name a worker opens must come from a listing or the user's own
   stored choice, never from an untrusted party: slice 5's `sane` backend, said there.
8. **On macOS `load()` opened a bare name, which dyld also looks for in the working directory** (code
   review, from Apple's dlopen(3); not run here, no Mac): a `libsane.1.dylib` planted in the directory an
   app starts from would have been loaded. `load()` now tries only absolute paths there (decision 63); an
   app with SANE elsewhere gives its path to `load_from`.
9. **`load_from` was safe, its trust in the library resting on doc text** (code review): any path from safe
   code could run a library's initialisers and have arbitrary symbols called as SANE's. It's an `unsafe fn`
   now (decision 62), so the promise is its caller's, in Rust's terms.
10. **Two copies of the crate in one program would start SANE twice** (code review): each with its own
    one-per-process slot, so overlapping calls and one copy's `sane_exit` freeing what the other's handles
    use, from safe code. Cargo refuses two copies now (`links`, decision 64).

## Risks

1. **CI's libsane isn't this machine's:** `ubuntu-latest` may ship an older sane-backends than 1.4.0, whose
   `test` or `pnm` backend behaves differently (hand-scanner with the feeder, the resolution bug, `pnm` built
   at all). The require variables make it a failure, not a skip; the tests then follow the older behaviour,
   or the fake covers it, said. *CI's first run (step 1's push, step 5's account) found the same SANE as
   here: `ubuntu-latest` is `ubuntu-26.04`, with `libsane1` and `libsane-dev` 1.4.0-1ubuntu1 and both
   backends. The risk returns only if the image moves to another sane-backends; its printed versions would
   show it.*
2. **The nested build of the fake** may be slow the first time, contend for cargo's package cache lock, or
   pick the wrong toolchain under `cargo +1.89`; `env!("CARGO")` and a target directory of its own are the
   answers, the first CI run the proof.
3. **`just` on Windows** runs recipes, and the justfile's backtick variables, under Git's `sh`; S1 slice 1's
   CI never ran, and pdf-lint's CI has no Windows runner to learn from (it lints for Windows from Ubuntu).
   If it fails, the Windows job runs the recipes' cargo lines directly. *Answered by CI's first run (step
   1's push, step 5's account): on `windows-latest`, `just fmt`, `clippy`, `test` and `doc` ran under Git's
   `sh`, the `doc` recipe's `RUSTDOCFLAGS` prefix and the justfile's backtick variable (`just` evaluates it
   even for recipes that don't use it) included. The recipes added since (`sane-dl-package`) run on Ubuntu
   only.*
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
    before `/etc/sane.d` (`sane-dll(5)`): in `ConfigDir`'s docs and "For review (security)" 4.
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
   to the plan, the plan's files must say where it lives, and it's decision 1, for review.
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

### Step 2: bindings, loading, devices, handles; the fake's first cut; the header contract (2026-11-08)

Commit: the one that carries this account.

- **What it does:**
  - `sane_dl::ffi`, public, no unsafe: `sane.h` 1.4.0 as Rust declarations, from the installed header,
    its notice kept word for word with what changed (the functions as `unsafe extern "C"` pointer types,
    `SaneInitFn` and the rest; the enums as integers with constants; the `#if 0` values as constants
    marked reserved; the function-like macros as `const fn`s; the anonymous union named
    `SANE_Constraint`; comments reworded; `type` spelled `r#type`).
  - `Status` and `FrameFormat` (integers with SANE's constants, the reserved ones marked; `name()`,
    `is_reserved()`, `Display` as `JAMMED (6)`), `Version` (`from_code`, `code`).
  - `Sane`: `load()` (`libsane.so.1`, `libsane.1.dylib` on macOS, `NoDefault` on Windows;
    `Sane::DEFAULT_LIBRARY`) and `load_from(path)`; `sane_init` without a callback; a version other than
    1 refused after `sane_exit`. One per process: a static slot, taken before `sane_init` and freed
    after `sane_exit` or by any failed load. The library leaked, never unloaded. `Clone`, `Send + Sync`;
    `sane_exit` when the last of it, its handles and any late listing are gone. Every call takes one
    call token (a flag under a mutex, with a condition variable), so a late listing can be said rather
    than waited on. `devices(local_only, deadline)`: a thread per listing, the list copied there;
    `DeadlineExceeded` past the deadline (the wait for the token counts); calls meanwhile
    `ListingPending`; a handle dropped meanwhile left on a pending list the listing's thread closes when
    it returns, which then ends SANE if the `Sane` is gone too. `open(name)`, `Handle` (`Send`, not
    `Sync`; `name()`, `close()`, closed on drop), `status_message`, `Limits` (devices 1,024; strings
    64 KiB) with `limits()` and `set_limits()`.
  - `ConfigDir::write(dir, backends, Search::Isolated | Search::BeforeSystem)`, `value()`,
    `CONFIG_DIR_VARIABLE`, `backend_of`; `Device`, `DeviceName`; `LoadError`, `Error` (`Status { status,
    call }` with `Call` naming the function, `ListingPending`, `DeadlineExceeded`, `BadDeviceList`,
    `BadHandle`, `NulInName`, `Thread`), `ConfigError`. Unsafe in `library.rs` alone.
  - `fake-libsane`: the 14 exports in `exports.rs`, its one unsafe module, each asserted at compile time
    to have its binding's type, declared with `sane-dl`'s `ffi.rs` by `#[path]`; the script (an
    `sane_init` sequence of statuses and versions, devices, `@null` and `@long:N` strings, a repeated
    device for the cap, a listing's status or null list, listing delays in turn, `sane_open`'s answers,
    a null `sane_strstatus`) and the log (`call`, `return`, `violation`, `note`, with the thread) in its
    crate docs; the rule checks (calls overlapping, a closed or unknown handle reached, `sane_cancel`
    included, `sane_exit` with handles open, the call after an error status from `sane_start`,
    `sane_get_parameters` or `sane_read` that isn't `sane_cancel` or `sane_close`, plus decision 30's);
    a stale list overwritten with `#` and kept. Options and scanning answer `UNSUPPORTED` until steps 3
    and 4.
  - Tests, 71 here (68 as first committed, three added by the repair below): `sane-dl` 10 unit,
    `tests/config_dir.rs` 9, `tests/fake.rs` 20, `tests/loading.rs` 6 (7 on Windows, with `NoDefault`),
    `tests/real.rs` 4, `tests/header_contract.rs` 1, a doctest; `fake-libsane` 20 unit. `tests/common/`
    holds the child process (rusty-fork's `fork`, the timeout, exit 77 and the parent's own stderr for a
    skip, the require variables), the fake built once per
    test binary by `env!("CARGO")` into `CARGO_TARGET_TMPDIR/fake-libsane/` with `RUSTFLAGS`,
    `CARGO_ENCODED_RUSTFLAGS`, `CARGO_BUILD_RUSTFLAGS` and `CARGO_BUILD_TARGET` cleared, and the log
    read back. Every test that loads a library runs in a child, and every fake test asserts its log has
    no violation and the calls expected.
- **Checked here:** `cargo fmt --check`; `cargo clippy --locked --workspace --all-targets -D warnings`;
  the same for `x86_64-pc-windows-gnu` and `x86_64-apple-darwin` (`-p sane-dl -p fake-libsane`), and
  `cargo doc -D warnings` for those two and Linux; `cargo test --locked --workspace` with
  `SANE_DL_REQUIRE_LIBSANE=1` and `SANE_DL_REQUIRE_HEADER=1`, all passing, the header contract among
  them against the installed 1.4.0 header; `cargo +1.89 check --workspace --all-targets`.
  - **Make-fails** (`scratch/work/s0-1/step-2/make-fail.md`, on a worktree under `step-2/`, removed
    after): 25 breaks, each failing its test. The header contract: a constant changed (`NO_DOCS` to 6,
    named exactly by the text pass), a field's type, a binding's function type (a compile error at the
    table's tie), a C spelling (the compiler's static assertion), a reserved constant missing from the
    table, one not marked reserved. Each of the fake's six checks turned off (overlap, closed handle,
    exit with handles open, cancel after an error, outside `sane_init` and `sane_exit`, the stale
    overwrite): its unit test fails. The core: a null name taken, the device and string caps off by
    one, no deadline, calls waiting on a late listing, a drop waiting on one, a refused version not
    exited, no one-per-process slot, a null handle taken, the call token not taken (the fake logs
    overlaps), a dropped handle never closed (the fake logs `sane_exit` with handles open), a late
    listing leaving its closes undone, `sane_open`'s status named for another call (the real-libsane
    test).
  - **Skips said on a pass:** the real-libsane tests with `SANE_DL_LIBSANE` at a missing path and no
    require variable each print `<test>: skipped: there's no libsane to load here (…)` on stderr and
    pass; with `SANE_DL_REQUIRE_LIBSANE=1` all four fail. The header contract likewise with
    `SANE_DL_SANE_H` at a missing path, and with `CC` at a missing compiler.
  - **The `// SAFETY:` comments, read for truth:** each rests on one of the module's rules (the call
    token held, the slot, the last reference going, a handle closed once, SANE's own contracts for the
    list and strings, the library leaked). Two needed fixing: a symbol was copied straight into a
    function pointer, and a null address would have made that undefined behaviour, which the comment
    didn't answer (libloading can return one where `dlerror` is thread-safe), so it's read as
    `Option<fn>` and a null address is `MissingSymbol`; and the handle's `Send` said the standard ties a
    handle to no thread, where it says nothing of threads at all.
- **Listing, timed** (`scratch/work/s0-1/step-2/listing-probe/`, not a test since it reads the system's
  directories: a release build, each run a child of its own, three rounds, in the memory scope):
  isolated `test` and `pnm`, 0.001 s to list 4 devices; `BeforeSystem` naming `test` alone, 0.001 s
  (2); the system's configuration, 8.88 to 8.90 s (none), beside `scanimage`'s 9.54 s (Baselines).
  `sane_init` itself takes 0.001 to 0.006 s with the system's configuration: the wait is all
  `sane_get_devices`.
- **What it learned of SANE, for slice 2** (the seam's unknowns):
  - **Names:** `test:0`, `test:1`, `pnm:0`, `pnm:1`: the backend's name, then an index the backend gives
    in order (`test` from its `number_of_devices`, `pnm` two, fixed). An index says nothing of which
    device it is, so a backend that numbers its devices can rename one when another comes before it;
    what real backends put after the `:` (fujitsu's model and serial, USB addresses) needs their
    hardware, so replugging stays the hand check's.
  - **Opening by name needs no listing, nor a `ConfigDir`:** SANE's `dll` loads the backend a name
    starts with whether or not `dll.conf` names it (`test:0` opened with a `dll.conf` naming only `pnm`,
    and with the system's, where `test` is commented out). So the fast path for a known device is
    `open` alone; a one-backend `ConfigDir` is for fast listings, and keeps other backends out of them.
    For review (security) 7 says what that means for the names a worker opens.
  - **Concurrent opens:** `pnm` opens one device at a time (a second is `DEVICE_BUSY`); `test` opens
    both.
  - Stopping after the page in flight is step 4's.
- **Departures from the plan, and why:**
  - *`ConfigDir`'s writing tests are integration tests* (`tests/config_dir.rs`), not unit tests: every
    file a test writes goes under `CARGO_TARGET_TMPDIR`, which only integration tests are given.
  - *Added beyond the design's lists:* `Error::Thread` (a listing's thread that can't start);
    `ConfigError::BadPath` (decision 28); `Call` (the `call` field's type); `Search` and
    `CONFIG_DIR_VARIABLE` (the design's `Isolated` and `BeforeSystem`, and SANE's variable, named);
    `Sane::DEFAULT_LIBRARY`, `version()`, `limits()` and `set_limits()`, `Handle::name()`.
    `LoadError::NotFound` and `MissingSymbol` carry their source boxed, keeping libloading's type out of
    the API.
  - *`Limits` holds the two caps this step uses* (decision 29); steps 3 and 4 add theirs.
  - *No alive flag yet:* it's the cancel token's, step 4. A handle dropped during a late listing goes on
    the pending list, which step 4's flag joins.
  - *The real test that opens devices at once* opens the `test` backend's two together, and `pnm`'s one
    at a time (above).
  - *Crate READMEs and changelogs* wait for step 5, as in step 1.
- **Decisions taken alone:** 26 to 31 above; For review (security) 7.
- **Checked** (`scratch/work/s0-1/flow/checks.md`, "Step 2"): `just ci` with both require variables (68
  tests in 8 harnesses, none skipped; the gates read 8 binaries, as there are 5 integration harnesses;
  `sane-dl-msrv` on 1.89, the fake built by 1.89's cargo); the test list (68; step 1 had none); the header
  contract against the installed 1.4.0 header, and failing on `NO_DOCS` changed to 6, naming it; the
  fake's rule checks turned off one at a time on a worktree; the skips said, and failures with the require
  variables; clippy and docs with `-D warnings` for `x86_64-pc-windows-gnu` and `x86_64-apple-darwin`; the
  listing probe run again (the same times); every `// SAFETY:` comment read for truth; the commit's
  message and diff; the user's cargo home unchanged. All passed but four, repaired in this commit:
  1. *After a listing that returned in time, `sane_exit` was left to the listing's thread* (medium). Cause:
     the listing's thread sent its result, then dropped its guard, which holds a reference to the
     library, so `devices()` could return while that thread still held it; a `Sane` dropped then wasn't
     the last reference, and `sane_exit` and the slot came later on the listing's thread, or never if
     the process ended first. Seen by the check: one fake test alone failing 1 run in 100, two failing
     under `strace -f` every time, 4 of 10,000 loads right after a drop refused `AlreadyLoaded`, 2 of
     2,000 processes ending without `sane_exit`. Fix: the guard goes first (it closes what's left to it,
     gives the token back and drops the thread's reference), then the result is sent, so a caller that
     has its list holds every reference left and its last drop ends SANE at once, on its own thread; the
     timeout path's reasoning and `Sane`'s doc say so. Tests: a new one, 500 rounds in a child of a
     listing, a drop and a load at once that must not be `AlreadyLoaded`, every `sane_exit` on the
     test's thread; three in-time tests now also assert `sane_exit` came from the test's thread.
  2. *The fake's null-pointer check was shown catching nothing* (minor). Cause: no test called it. Fix: a
     unit test calls the exports with each null (serialized with the other test that uses the fake's
     shared state). Writing it showed two faults in the fake, fixed with it: a null `version_code` to
     `sane_init` was recorded as a violation, though the standard allows it (no version is then given:
     now no violation, and the log says `version_code=null`); and `sane_open` with a null handle opened
     the device anyway, leaving a handle open that no caller had, which `sane_exit` would have reported
     as a second violation. Now a null list, handle or name is answered `INVAL` before anything is
     listed or opened; the fake's crate docs say so, and decision 30.
  3. *`ConfigDir::write` replaced `dll.conf` before refusing a `dll.d` that isn't empty* (minor),
     against decision 28's "before writing anything". Fix: `dll.d` is looked at first (its metadata,
     then its listing, so a file in its place is refused alike on every system), `dll.conf` is written
     after, and a missing `dll.d` is made last (and checked again if someone made it meanwhile); the doc
     says every check comes before any write. Tests: the refusal leaves no `dll.conf`, and one already
     there as it was; a new test refuses a file named `dll.d` the same way.
  4. *The tests' parent-only work ran in every child too* (minor). Cause: rusty-fork's child runs the
     test function from its top until it reaches the fork, so `Fake::new` (writing the script),
     `real_child` (writing the configuration) and `fake.child()` (a nested `cargo build`, a no-op but
     a process and the target directory's lock) ran again in each child. Fix: `Fake::new` and
     `real_child` only compute paths; a `Child` carries the parent's work as preparations, which
     rusty-fork's process modifier runs in the parent alone, just before the spawn; every child gets
     `SANE_DL_TEST_CHILD=1`, and the helpers that write or build (`fresh_dir`, `fake_library`) fail the
     test if they run in a child.
  - *Checked again* (`scratch/work/s0-1/step-2/repair/`): on a worktree of the repaired tree (removed
    after; branch, HEAD, stash and worktree list unchanged), each fault put back and its test seen to
    fail, the file restored by SHA-256: the old order (send, then the guard) failed the new test in 12 of
    20 plain runs, and under `strace` and with a 200 ms sleep between send and drop (four tests then);
    the new order with a 200 ms sleep between drop and send passed the harness (the 500-round test with
    20 ms, since 500 rounds of 200 ms pass its 60 s timeout); the null check off (the check's F7), and a
    null `version_code` made a violation again, each failed the new unit test; `dll.conf` written before
    the check failed both `dll.d` tests; `fake_library()` called before `in_child!` failed its test, the
    child saying it's the parent's work. On the repaired tree: the fake harness under `strace -f` 5 times
    and whole 20 times, and the in-time test alone 200 times, all passing (at 4af5ff1 it failed under
    `strace` 3 runs of 3); the check's probe, 0 of 10,000 loads refused and 0 of 2,000 processes without
    `sane_exit`; an `execve` trace of one fake test, `cargo` run once, by the parent, where the check saw
    it in the child too; clippy for all targets, and clippy and docs for the other two systems. Then
    `just ci` with both require variables on the committed tree, and the test list (71, the 68 kept).
    One slip, in the repair's own runs: the harness binaries run straight (under `strace`, and in the
    loops) ran the tests' nested `cargo build` of the fake without the run's own cargo home, so cargo
    updated its last-use cache in the user's (`.global-cache`; nothing downloaded or added there). A
    test binary run outside `cargo test` needs `CARGO_HOME` set too, since the tests run cargo.

### Step 3: options, typed (2026-11-08)

Commit: the one that carries this account.

- **What it does:**
  - `sane-dl`'s options module (safe code): `OptionDescriptor` (index, name, title, description,
    `ValueType`, `Unit`, size, `Capabilities`, `Constraint`, and `unusable`, the reason it can't be used),
    `ValueType` and `Unit` (`Unknown(i32)` and `Other(i32)` for codes SANE doesn't define;
    `#[non_exhaustive]`), `Capabilities` (the bits as they came, with a method per bit), `Constraint`
    (`None`, `Range { min, max, quant }`, `WordList`, `StringList`; words as they came, read as `Fixed`
    for a `FIXED` option), `SaneString` (bytes, as `DeviceName`), `Fixed` (`to_f64`, a checked
    `from_f64`), `Value` (`Bool`, `Int(Vec<i32>)`, `Fixed(Vec<Fixed>)`, `String(SaneString)`, `Button`),
    `SetInfo`. What a copied descriptor means (`decode`), which constraint member its types allow, the
    refusals, the buffer a call passes and the value read back are all decided here, from owned copies,
    and unit-tested without a library.
  - In the one unsafe module: each descriptor copied (its constraint read only as the member its types
    allow; a word list's length checked against the cap before any entry is read; a string list walked no
    further than the cap; a group's name and constraint never read); `open` reading every descriptor, the
    count from option 0's value, and closing the device again if they can't be read; `Handle::options`,
    `find_option`, `get`, `set`, `set_auto`, `reload_options`, each under the call token. A call reads
    its option's descriptor again first (decision 32); a set or `set_auto` answering `RELOAD_OPTIONS`
    reads every descriptor again before it returns. `Limits` gains `options` (4,096), `list_entries`
    (65,536) and `option_bytes` (1 MiB). `Error` gains `BadDescriptor`, `BadValue`, `NoSuchOption`,
    `NoValue`, `Inactive`, `NotReadable`, `NotSettable`, `NotAutomatic`, `WrongType`, `WrongLength`,
    `StringTooLong`, `NulInString`, `Unusable`.
  - The fake: every handle has option 0 and the script's options, each with its value; descriptors given
    from leaked memory at one address for good, rewritten in place when a rule changes them. Script lines
    (in its crate docs): `option`, `title`, `desc`, `unit`, `range`, `words`, `strings`, `constraint`,
    `word-list-length`, `strings-repeat`, `value`, `auto`, `reload-params`, `when V activate|deactivate|
    range|words|size|null NAME`, `when V quiet`, `start-inactive`, `option-null`, `option-count`,
    `control-status`, `profile`. The profiles, written as script lines: `fujitsu`, `airscan`, `brother`,
    `canon-dr`, `lexmark`, `epson`, `kodak`, each with `source`, its duplex as that backend names it,
    `mode`, `resolution`, the scan area and a `page-width` and `page-height` active only for the feeder;
    `airscan`'s resolutions change in duplex. `sane_control_option` plays a backend's part (a string not in
    its list `INVAL`, a number moved into its range or onto its word list and said `INEXACT`, the rules
    fired) and checks the refusals (decision 40). Duplex pages marked in their pixels wait for step 4.
  - Tests, 128 here (71 before, all kept): `sane-dl` 27 unit (17 new), `tests/options_real.rs` 6 (the
    `test` backend with `enable-test-options`, through a `test.conf` in the isolated directory: every value
    type, constraint and unit, the groups in order, the gamma tables as 256-word arrays and a table set and
    read back, arrays, `int-inexact` answering `INEXACT` and its value changed, the hard-select,
    soft-detect, emulated and automatic options, the button, `mode` set to `Color` making `three-pass`
    active through `RELOAD_OPTIONS`, the type, length and size refusals; `pnm`'s `filename` set and read
    back, its `resolution` word list set to auto, `set_auto` refused for `brightness`, its read-only
    option), `tests/options_fake.rs` 23 (what `open` calls; each profile read as typed with its feeder and
    duplex set and its page size active only for the feeder; a constraint that changes; 20 refusals with
    no call on their options in the log and the values that fit seen as the backend gets them, a `BOOL`
    as the word 1; 22 hostile descriptors, each unusable or a `BadValue` as expected; 6 that fail `open`
    with `BadDescriptor`, the device closed; each cap at its limit and one past; a descriptor changed
    without `RELOAD_OPTIONS`, read before the call that sizes from it; a re-read that fails; a count the
    backend won't give; a value the backend refuses; option calls during a late listing); `fake-libsane`
    31 unit (11 new). The step 2 tests that compare call sequences now leave the option calls `open`
    makes out (`FakeLog::calls_but_options`).
- **Checked here:** `cargo fmt --check`; `cargo clippy --locked --workspace --all-targets -D warnings`;
  `cargo test --locked --workspace` with `SANE_DL_REQUIRE_LIBSANE=1` and `SANE_DL_REQUIRE_HEADER=1`, all
  128 passing, none skipped; `cargo doc -D warnings` (the recipe's); `cargo check --all-targets` for
  `x86_64-pc-windows-gnu` and `x86_64-apple-darwin` (no code behind `cfg` was added, so their clippy and
  docs are left out, as the step says).
  - **Make-fails** (`scratch/work/s0-1/step-3/make-fail.md`, on a worktree under `step-3/`, the step's
    files copied in; removed after, branch and stash unchanged): 33 breaks, each failing its test. The
    reading: a string list read on an `INT`; a word list's length uncapped (the child dies by `SIGSEGV`
    reading 2³¹ − 1 words); a string list uncapped; the count uncapped; the size uncapped, not a word's
    multiple, a `BOOL`'s unchecked; a `BOOL` word read by cast; a string with no `NUL` taken whole; a null
    descriptor below the count taken as the end; a name past the cap cut; the option and list caps off
    by one. The refusals: inactive, `SOFT_SELECT`, `SOFT_DETECT`, `AUTOMATIC`, another length, a string
    filling the size, a `NUL` inside, a `BOOL` set as -1 (the fake catches each reaching it). The
    re-reads: none on `RELOAD_OPTIONS` (against the fake and the `test` backend), none before a call,
    stale descriptors kept after a failed re-read, a device left open after bad descriptors. `Fixed`
    unchecked; `INEXACT` from the wrong bit. And the fake's own checks off one at a time (inactive, the
    re-read, the `BOOL` word, a malformed descriptor, a descriptor rewritten at its address).
  - **The descriptors beside `scanimage -A`** (`scratch/work/s0-1/step-3/listing/compare.md`; a probe
    crate there printing `sane-dl`'s view in `scanimage`'s layout, through the same private configuration
    with the test options on): for `test:0`, 57 options in 8 groups, every title, name, constraint (with
    units, steps and fixed-point bounds), array, value and inactive flag agrees, but `bool-hard-select`,
    which `scanimage` reads (`[yes]`) and `sane-dl` refuses as not readable (decision 33), and the
    descriptions of `-x` and `-y`, which `scanimage` replaces with its own. For `pnm:0` everything agrees
    but a separator `scanimage` prints after `auto|`.
- **What it learned of SANE** (for slice 2, and slice 5's `sane` backend): nothing new on the seam's
  two unknowns (names, and stopping after the page in flight). For slice 5: the `test` backend's
  `resolution` is `FIXED` and `pnm`'s an `INT` word list that can be automatic, so a backend reads the
  type from the descriptor; the `test` backend's `source` is `Flatbed` or `Automatic Document Feeder`,
  and `mode` set to `Color` says `RELOAD_OPTIONS`; `int-inexact` changes the value it's given and says
  `INEXACT`.
- **Departures from the plan, and why:**
  - *`BadValue` carries a reason, not a word:* a string with no `NUL` within its size has no word to
    carry.
  - *Added beyond the design's lists:* `NoValue` (a get of a button or group), `NotReadable` (decision
    33), `NulInString` (a `NUL` would end the string early); `Capabilities`, `Value::value_type`,
    `OptionDescriptor::word_count` and `is_group`, `Handle::find_option` and `reload_options`.
  - *The option calls take `&mut self`:* any of them may read the descriptors again.
  - *Each call reads its descriptor again first* (decision 32), which the design didn't ask for.
  - *The fake's extra lines* (`control-status`, `option-null`, `option-count`, `when … size`, `null`,
    `quiet`, `start-inactive`) are for the hostile cases; its option checks (decision 40) are beyond the
    design's rules, as decision 30's were.
  - *Crate READMEs and changelogs* wait for step 5, as in steps 1 and 2.
- **Decisions taken alone:** 32 to 40 above.
- **Checked** (`scratch/work/s0-1/flow/checks.md`, "Step 3"), at `2cde32f`, all passed: `just ci` with both
  require variables (128 tests in 10 harnesses, none skipped; the gates read 10 binaries; `sane-dl-msrv`, 97
  tests on 1.89); the test list (128; the 71 of step 2 kept, 57 new); the `scanimage -A` comparison reproduced,
  and an oracle reading libsane straight through Python's ctypes agreeing with every field of all 57 of
  `test:0`'s descriptors and 27 of `pnm:0`'s; the 33 make-fails run again on the committed tree, each failing
  its test; 1.89 building every target; the new harnesses passing 60 repeated runs; upstream `test.c` and
  `pnm.c` laying out their options the same at 1.2.1 (Ubuntu 24.04's) as at 1.4.0 (Risk 1). Noted for the
  code review, not a fault: no cap on a descriptor's total (one option's string list could copy
  65,536 × 64 KiB from a backend pointing every entry at one string), within every cap the design names.

### Step 4: scanning: frames, batches, cancel (2026-11-08)

Commit: the one that carries this account.

- **What it does:**
  - `sane-dl`'s scan module (safe code): `Parameters` (the six fields as SANE gives them; `is_last_frame`,
    `height` (`Height::Known` or `Unknown`), `channels`, `pixel_bytes`, and `check`, the design's every
    refusal, `BadParameters` carrying the parameters and the reason); `Image` (`format`, `depth`, `width`,
    `height`, `stride`, `data`, `frames`); `Progress` (the page's bytes read, and expected where known);
    `Page` (`Image`, or `NoDocs { from, pages }`); `ReadOutcome` (`Bytes(n)` or `Eof`); the assembly: rows'
    padding dropped as they come, a frame's end checked (`ShortFrame`, `LongFrame` where it runs past,
    `PartialRow`, `FrameTooLarge` past the cap as it reads), a page's frames checked against each other (a
    gray or RGB frame after a single-channel one, a channel twice, another size or depth, an image that ends
    short of three channels or goes on past them, a three-pass page of unknown height whose frames end at
    different rows), three-pass frames put in red, green, blue order and interleaved a sample at a time (two
    bytes at 16 bits, a byte of eight pixels at 1); `Batch` (`Handle::batch`, `next_page(on_progress)`,
    `pages`), reading blocking after each start, `NO_DOCS` from a start or a page's first read a
    `Page::NoDocs`, `sane_cancel` on every error and when the batch goes. Unit-tested from chunks.
  - In the one unsafe module: `Handle::start` (`INVAL` re-reads every descriptor, after the `sane_cancel`,
    and is `Error::StartInvalid`, carrying the re-read's failure if it failed), `parameters`, `read` (the
    length asked capped at the buffer, `Limits::read_chunk` and `i32::MAX`; one below 0 or past it
    `BadReadLength`; an empty buffer calls nothing), `set_io_mode`, `select_fd` (`cfg(unix)`, a `RawFd`,
    negative `BadSelectFd`), `cancel`, `cancel_token`; `sane_cancel` after every error status from a start,
    the parameters or a read, and before `sane_close` while a scan is pending. `CancelToken` (`Clone`,
    `Send + Sync`; `cancel`, `is_requested`): `sane_cancel` from any thread with no call token, under the
    handle's own lock, only while its alive flag says the handle is open (cleared under that lock before
    `sane_close`), and a flag that `start` and `read` check before they call SANE and after it returns, taken
    by the call that returns `Cancelled`. `Limits` gains `frame_bytes` (1 GiB), `line_bytes` (16 MiB) and
    `read_chunk` (256 KiB); `Error` gains `StartInvalid`, `BadReadLength`, `BadParameters`, `ShortFrame`,
    `LongFrame`, `PartialRow`, `FrameTooLarge`, `Cancelled`, `BadSelectFd`.
  - The fake: scanning played (a `feed` module, the script lines in its crate docs): `frame` (any
    parameters, and the bytes sent, or `endless`), `sheets`, `duplex` (pages marked `F` and `B` in their
    first byte, every byte else a formula of page, frame, line and column, padding `0xEE`), `at PAGE start
    STATUS`, `at PAGE read STATUS [BYTES]`, `at PAGE hang`, `hang-forever` and `ignore-cancel`, `at PAGE
    abort`, `read-max`, `read-length N|past`, `non-blocking`, `select-fd`, `parameters-status`,
    `start-inval-deactivates`. A read that hangs waits for a cancel outside the fake's lock. New checks: a
    close with a scan pending (a batch not ended by `sane_cancel`), a start before the frame's `EOF`, a read
    with no frame started, the null pointers of the scanning calls; and after a start that answers `INVAL`,
    a call on an option before every descriptor is read again.
  - Tests, 212 here (128 before, all kept; 84 new): `sane-dl` 40 unit (13 new), `tests/scan_fake.rs` 39 (a
    feeder's sheets then `NO_DOCS` with the calls and progress, a batch dropped after a page, duplex marks,
    a jam at a start and an open cover mid-page with the batch going on after, `NO_DOCS` from a first read
    and later, `INVAL` from a start re-reading the descriptors, the step-by-step calls, select fds, an
    endless stream at an 8 MiB cap holding under 64 MiB, three-pass from the fake, and 28 hostile frames,
    statuses and lengths, each an error carrying what SANE said and followed by `sane_cancel`),
    `tests/cancel.rs` 6, `tests/scan_real.rs` 11 and `tests/scan_pnm.rs` 3 (below); `fake-libsane` 43 unit
    (12 new). `tests/common/` gains `Child::aborts` (the child must end by `SIGABRT` on Unix, by exit code
    `0xC0000409` on Windows, and the parent says how it ended), `fake_pixels` and `peak_memory`.
  - **Through the `test` backend:** the feeder's 10 sheets then `NO_DOCS`, again after reopening; gray and
    color at 1, 8 and 16 bits, solid black and white, every sample checked; the hand scanner alone and from
    the feeder (`lines = -1`, 10 pages); short reads (7 bytes) giving the image 64-byte reads give, a progress
    call each; three-pass in all six orders at 8 bits, and at 16 and 1, equal to one pass; `ppl-loss` (the
    bytes a line kept, three pixels fewer, each row the first 28 of the plain scan's); `invert-endianess`
    (every 16-bit sample's bytes swapped); every status `read-return-value` forces (`EOF` an immediate end);
    non-blocking reads with the select fd (the backend's pipe, `/proc/self/fd`), the same bytes as blocking;
    `fuzzy-parameters` (the estimate kept as it came, the frame's own after the start). **Through `pnm`:** a
    PBM, a PGM and a PPM the test writes, scanned byte for byte, the PPM in three passes (green, blue, red),
    each as a hand scanner. **Through the fake** (`tests/cancel.rs`): a cancel from another thread during a
    blocked read returns `Cancelled`, the read answering `CANCELLED` and the reading thread then ending the
    scan; a cancel between pages stops the next with no `sane_start` after it, and the page after comes; a
    cancel after the close reaches nothing; 10,000 rounds of cancels racing a close, none reaching a closed
    handle (140,000 to 170,000 a run reaching open ones); a backend that ignores the cancel stopped by the flag, at most
    one read after it; a read that hangs for good still blocked after a cancel (the watchdog's, slice 4); the
    abort at page 2 ending the child by `SIGABRT`, said by the test.
- **Checked here:** `cargo fmt --check`; `cargo clippy --locked --workspace --all-targets -D warnings`, and
  `-p sane-dl -p fake-libsane --all-targets` for `x86_64-pc-windows-gnu` and `x86_64-apple-darwin`; `cargo doc
  -D warnings` for those two and Linux (no doc link reaches `select_fd`); `cargo test --locked --workspace`
  with both require variables, 212 passing, three times in a row.
  - **Make-fails** (`scratch/work/s0-1/step-4/make-fail.md`, on a worktree under `step-4/`, the step's files
    copied in; removed after, branch and stash unchanged): 41 breaks, each failing its test. Assembly:
    padding kept (two tests), three-pass frames left in their order (the `test` backend's and `pnm`'s),
    16-bit samples interleaved a byte at a time, short and long frames and partial rows taken, the frame cap
    doubled, 1-bit rows' bytes rounded down, progress not heard or expected for one frame of three, no
    blocking mode after a start, `NO_DOCS` from a first read taken as an error. Parameters: a depth of 3,
    `2500x0`, a `last_frame` of 2, a reserved format, too few bytes a line, a frame past its cap, a gray frame
    that isn't last, a missing channel, a channel twice, frames of other sizes, gray after a single channel.
    The handle: a read length past what was asked, no `sane_cancel` after a read's error, a batch or a
    handle dropped without ending its scan, no re-read on `INVAL`, the read chunk uncapped. The token: a
    cancel past the alive flag, the flag cleared after the close (the race test catches both), the cancel's
    flag not set, not checked before a start, not checked by reads (the ignoring backend then spins until
    the child's timeout). The fake's new checks off one at a time, and its abort not made.
  - **Pages beside `scanimage`'s** (`scratch/work/s0-1/step-4/compare/compare.md`; a probe crate scanning
    `test:0` at 50 dpi, "Color pattern", through the same private configuration): gray 8, 1-bit gray,
    `ppl-loss` (scanimage's padded rows cut to sane-dl's width) and the hand scanner from the feeder (three
    pages) byte for byte the same. scanimage 1.4.0 can't scan color 16 in three passes (it asserts three-pass
    frames are 8-bit, and aborts), so sane-dl's 16-bit three-pass page was set beside scanimage's one-pass
    16-bit page of the same picture, after its byte order: the same; and 8-bit three-pass beside scanimage's
    own three-pass page: the same.
  - **ThreadSanitizer** (`scratch/work/s0-1/step-4/tsan/`): `tests/cancel.rs` on nightly (1.101.0-nightly,
    2026-10-15) with `-Zsanitizer=thread -Zbuild-std`, the fake built plain by the helper (no TSan symbol in
    it, 49 in the test binary): all 6 pass, no report (TSan's exit code would fail the child).
  - **The memory cap:** the endless stream stops at an 8 MiB cap after 33 reads, the child's peak under 64
    MiB (asserted from `VmHWM`), the memory scope untouched.
- **What it learned of SANE, for slice 2** (the seam's unknowns):
  - **Names:** nothing beyond step 2's (`test:0`, `test:1`, `pnm:0`, `pnm:1`: the backend's name, then an
    index it gives in order). New: the `test` backend keeps its options' values from one `sane_open` to the
    next in a process (a `ppl-loss` set on one handle was there on the next), and its feeder counts every
    start since the device opened, a flatbed's too (a page scanned before choosing the feeder leaves nine).
  - **Stopping after the page in flight:** SANE lets a frontend do it: read the page to its `EOF`, start no
    next page, end the batch with `sane_cancel`. The `test` backend takes a sheet only at a start, so a batch
    ended after its third page leaves the fourth in the feeder: the next batch gives 7 pages, then `NO_DOCS`;
    after `NO_DOCS` it fills again, 10 more. Whether a real feeder has already pulled the next sheet in by
    then is its backend's and its hardware's: slice 6's fi-6130.
- **Found in SANE's own code** (not reported: that waits on review; backlog items with step 5;
  `scratch/work/s0-1/step-4/README.md`):
  - *The `test` backend 1.4.0 can deadlock* (built with threads, as Ubuntu builds it): it ends each frame's
    reader thread with an asynchronous cancel, and one that lands while the thread is in the allocator leaves
    the allocator's lock held, so the next frame's thread blocks for good (gdb: in `malloc`). Seen at a
    cancel at once after a start (1 in about 2,000), and at the reads that end frames (1 to 4 processes in
    12 or 24 reading 3,000 pages each in parallel). The tests pace the backend (reads of 64 bytes, a 1 ms
    pause after each frame's first read: none in 144,000 pages), and leave a cancel at once after a start to
    the fake. A worker meets the same backend only in tests; for any backend that hangs, slice 4's watchdog.
  - *The `test` backend reads an empty pipe as the frame's end in non-blocking mode*, answering `GOOD` with
    a length of -1: `BadReadLength`, as it should be.
  - *`scanimage` 1.4.0 aborts on 16-bit three-pass color* (above).
- **Departures from the plan, and why:**
  - *Renamed and added:* `read`'s answer is `ReadOutcome`; `next_page` is `Batch`'s, from `Handle::batch`;
    `Page::NoDocs` carries the pages delivered; `StartInvalid` says `INVAL` from a start and its re-read (the
    design's "said in what the call returns"); `BadSelectFd`; `Height`, `Progress`, `CancelToken::is_requested`.
  - *`read-return-value` read step by step:* each forced status is read after a start and a pause, so the
    backend's reader isn't cancelled mid-allocation (its deadlock, above); a batch's `NoDocs` from a read and
    `ShortFrame` at an immediate `EOF` are the fake's tests. *`fuzzy-parameters`:* the estimate is checked
    for its format and depth only, since it may be anything (here a line count of 0).
  - *No 16-bit image through `pnm`:* `pnm` 1.4.0 reads only P4, P5 and P6, and takes any P5 or P6 as 8-bit
    whatever its maximum.
  - *16-bit three-pass beside `scanimage`'s one-pass page*, as scanimage can't do it.
  - *airbridge, read in full where it touches SANE* (its SANE backend, its backend trait, its jobs' and
    pages' cancel paths, at `530074da`), not its eSCL server, mDNS or HTTP. What it does: an `alive` flag
    under a mutex, held across `sane_cancel` and cleared before `sane_close` (the pattern followed, in our
    own code, credited in `library.rs`); a cancel before every close. What it doesn't, which `sane-dl` does:
    `sane_cancel` after an error, a check of the length a read gives, caps on word lists.
  - *Crate READMEs and changelogs* wait for step 5, as in steps 1 to 3.
- **Decisions taken alone:** 41 to 49 below.
- **Checked** (`scratch/work/s0-1/flow/checks.md`, "Step 4"), at `f8370fa`: `just ci` with both require
  variables (212 tests in 14 harnesses, none skipped; the gates read 14 binaries; `sane-dl-msrv`, 169 tests
  on 1.89); the test list (212; the 128 of step 3 kept, 84 new); clippy and docs with `-D warnings` for
  `x86_64-pc-windows-gnu` and `x86_64-apple-darwin`, and 1.89 checking every target; the make-fails, the
  ThreadSanitizer run and the `scanimage` comparison run again on the commit, since `library.rs` changed
  after them (the 10 `library.rs` make-fails each failing its test; of 14 more the check made, 7 failing
  theirs and 7 unseen, test gaps for the code review; the 6 cancel tests clean under ThreadSanitizer, the
  race 10,000 rounds; every page the same as `scanimage`'s); the endless stream stopping at its 8 MiB cap;
  the scan harnesses passing 60 runs in a row and 80 more pinned to 1 and to 4 CPUs; the new code read
  where the job named it, no fault found (`CancelHandle`'s `Sync` comment quoting the standard truly); the
  commit's message and diff; the user's cargo home unchanged. All passed but one, repaired in this commit:
  1. *The maintainer named in two lines this commit wrote* (minor): step 3's "Checked" and step 4's "Found
     in SANE's own code", written after the control file's note against anything new that names the
     maintainer. Fix: both reworded as the note says, their meaning kept ("noted for the code review, not a
     fault"; "not reported: that waits on review"). The mentions from before this step stay for the fix
     to clear, as the note has it.
  - *Checked again:* on the committed tree, `git grep -i` for the maintainer's name outside the root
    `LICENSE-MIT` finds the 79 lines it found at `2cde32f`, and the diff from `2cde32f` adds none. The tree
    differs from `f8370fa` in this file alone, which no build, test or gate reads (the gates leave out
    `docs/pm`), so `just ci` and the step's other checks weren't run again.

### Step 5: sane-dl ready to publish; the plan's files; the slice's end (2026-11-08)

Commits `5255df9` (the crate made ready to publish, its package checked) and the one that carries this account
(the plan's files).

- **What it does:**
  - `sane-dl` 0.1.0-alpha.1, publish allowed: `documentation` (docs.rs), `keywords`, `categories` (decision
    54) and `include = ["/src", "/README.md", "/CHANGELOG.md"]`; the lock changes only in its own version.
    Its README: what it is (SANE alone, loaded at run time, its own bindings, everything from C checked), an
    example, what it does, loading and each `LoadError`, the rules for threads and cancelling (one call at a
    time, a handle on one thread, a late listing, the token and its flag, stopping after the page in
    flight), the caps with their defaults, why SANE belongs in a process of its own (and that SANE opens any
    installed backend by name), the platforms, and the licences at the root by the repository's URLs, with
    `sane.h`'s public-domain notice kept in the bindings. Its `CHANGELOG.md`: the first alpha, what it holds,
    and how to depend on an alpha (`=0.1.0-alpha.1`). Two unit tests (the licence links reach the files; the
    changelog has an entry for the manifest's version) and the README's example as a doctest.
  - `just sane-dl-package`, after pdf-lint's `_package`: a fresh directory of its own (decision 50),
    `cargo publish --dry-run --locked -p sane-dl`, `cargo package --list` into `files.txt`, held to
    sources, README, changelog, manifest and lock, and `src/lib.rs`, the README, the changelog, the manifest
    and the lock each there (decision 51). In `just ci` (before `msrv`), and CI's `sane-dl-package` job on
    Ubuntu, read-only and without credentials as the others. The root README says the crate is built and
    ready to publish.
  - The plan's files: `status.md`; `decisions.md` (the fake's home, `sane-dl`'s MSRV); `crates/sane-dl.md`
    as built; `crates/fake-libsane.md`, new; `crates/scan-fixtures.md`, `crates/README.md`, `plan.md` (eleven
    crates, the fake's row and place in the graph, the MSRV measured per crate) and
    `milestones/s0-devices.md`'s slice 1 (the fake its own crate; slice 1 built); `slices/README.md`; the
    backlog: TEST-2 with what's left (CI's run of the tests, and what it's to show), TEST-7, FEAT-11, UP-7
    to UP-10, the table counted again with `rg` (38 open: 8 for S0, 15 for S1, 7 for S2, 8 later; 9 waiting).
  - Tests, 215 here (212 before, all kept; 3 new: `sane-dl` unit 42, the README's doctest).
- **Checked here:** `cargo fmt --check`; `cargo clippy --locked --workspace --all-targets -D warnings`;
  `cargo doc -D warnings` (the recipe's); `cargo test --locked -p sane-dl` with both require variables, 172
  passing (the fake's 43 untouched); the test list (215, the 212 of step 4 kept); `just gates` (14 binaries,
  the new files read for the name); `cargo +1.89 check -p sane-dl --all-targets`. `just sane-dl-package` with
  `CARGO_TARGET_DIR` at a fresh `scratch/work/s0-1/package/`, on `5255df9` (its `.cargo_vcs_info.json`
  names it): the dry run passes, and the list read: 15 files, `.cargo_vcs_info.json`, `CHANGELOG.md`,
  `Cargo.lock`, `Cargo.toml`, `Cargo.toml.orig`, `README.md` and the nine sources; the packaged manifest
  inlines the workspace's lints and drops the test targets (a warning per test, expected). A check: nothing
  was published. Every commit message of the branch read (`137be1a..HEAD`) for the downstream app's name,
  the maintainer's, local paths and `scratch/`: none. `git grep -i` for the maintainer's name outside the
  root `LICENSE-MIT` finds 78 lines, one fewer than step 4 left (a line of `crates/sane-dl.md` rewritten as
  built), none added.
  - **Make-fails** (`scratch/work/s0-1/step-5/make-fail.md`, on a detached worktree under `step-5/`, the
    step's files committed there, since the dry run refuses uncommitted changes; removed after, the branch,
    stash and worktree list unchanged): a licence link on `blob/main`, a relative link climbing out, the
    changelog a version behind, the README's example broken, each failing its test or doctest; `/tests`
    included, the changelog left out, the README deleted, `publish = false`, a stray `src/notes.txt`, each
    failing the recipe, saying why. One couldn't fail: `include` without the README, as cargo adds the
    README it finds (decision 51). The gates are unchanged, so step 1's make-fails stand.
- **What CI has shown already:** its first run, on step 1's push (`db62336`, run 42550930498, 2026-11-08,
  all five jobs green; its logs read at this step's check), before any test existed. `ubuntu-latest` is the
  `ubuntu-26.04` image: it installs `libsane1` and `libsane-dev` 1.4.0-1ubuntu1, the version here (CI's
  in the `amd64v3` build), and `libsane1` holds the `test` and `pnm` backends (Risk 1). `windows-latest`
  (`windows-2025-vs2026`) ran `just fmt`, `clippy`, `test` and `doc` under Git's `sh`, the `RUSTDOCFLAGS`
  prefix and the justfile's backtick variable included (Risk 3). `macos-latest` is `macos-26-arm64`. The
  MSRV job installs 1.89.0.
- **What only CI shows** (it waits on the push): the tests and the header contract against Ubuntu's
  1.4.0-1ubuntu1 (they assert 1.4.0's behaviour, and pass here on the same version); the gates' count of
  binaries (3 at that run, 14 here); the MSRV job with these tests (172 here on 1.89; there without libsane,
  so its libsane tests say they skipped, decision 16); the package job, which asks crates.io; and Windows and
  macOS building and testing this code (the fake as a DLL and a dylib, its nested build, rusty-fork's
  children there, the abort's exit code `0xC0000409` on Windows; Risks 2 and 6).
- **Found, correcting step 4's account:** step 4 gave the `test` backend's hang as "an asynchronous cancel"
  landing in the allocator, but sane-backends' `sanei_thread` cancels its threads in deferred mode, glibc's
  default, by its own comment (master at `a113b257`, read for UP-8), and the gdb backtraces step 4's notes
  name weren't kept. The hang and its rates stand (its runs' logs are kept), and so does the tests' pacing;
  its cause is unconfirmed, and UP-8 says so.
- **Departures from the plan, and why:**
  - *The package's directory under `CARGO_TARGET_DIR`*, not `CARGO_TARGET_DIR` itself (decision 50).
  - *Added beyond the step's list:* `crates/fake-libsane.md` (decision 55); the root README's line on the
    crate, and `README.md`'s row for `slices/`; the two unit tests and the README's doctest; the list's check
    that those five files are there.
  - *No README or changelog for `fake-libsane`*, which is never published: its crate docs say what it is
    and how to drive it, and `crates/fake-libsane.md` points at them.
- **Decisions taken alone:** 50 to 56 above.
- **Checked** (`scratch/work/s0-1/flow/checks.md`, "Step 5", at `0767885`): `just ci` with both require
  variables (215 tests in 14 harnesses, none skipped; the gates read 14 binaries; `sane-dl-package` 15 files;
  `sane-dl-msrv`, 172 tests on 1.89); the test list (215; the 212 of step 4 kept, 3 new);
  `just sane-dl-package` in a fresh directory, its file list, packaged manifest and lock read, nothing
  published; the recipe and its CI job read, the make-fails' logs spot-read; `cargo tree` and the lock
  unchanged but the version; the README and changelog against the code; UP-7 to UP-10 and the correction
  against sane-backends' master at `a113b257`, fetched again; the backlog's counts; every commit message of
  the branch; the user's cargo home unchanged. All passed but one, repaired in this commit:
  1. *TEST-2 and this step's account of what only CI shows were out of date* (minor, docs only). CI had run
     on step 1's push (`db62336`, run 42550930498, all five jobs green) four hours before this step, and its
     logs answer what TEST-2, the account and Risks 1 and 3 still gave as waiting: the versions
     `ubuntu-latest` installs (1.4.0-1ubuntu1, as here; TEST-2 still cited 24.04's 1.2.1), both backends in
     its `libsane1`, and `just` under Git's `sh` on Windows. Cause: the step wrote CI's part from the plan
     without looking at the run already there. Fix: TEST-2 says what that run showed and leaves the tests and
     the header contract against Ubuntu's 1.4.0-1ubuntu1; the account above is split into what CI has shown
     and what only it shows (those tests, the gates' count, the MSRV job with these tests, the package job,
     Windows and macOS on this code); Risks 1 and 3 and the `sane.h` baseline say what the run found; and
     the lines that called the run to come "CI's first run" (`crates/sane-dl.md`, `crates/fake-libsane.md`,
     `backlog/README.md`, the milestone's slice 1) now call it CI's run of this code.
  - *Checked again:* each fact written read again in the run's logs, saved at the check
    (`scratch/work/s0-1/step-5/check/ci-db62336-*.log`), and the run's head, jobs and conclusion through
    `gh run view`. A probe (`scratch/work/s0-1/step-5/repair/just-probe/`) showed `just` 1.59, the version
    CI installs, evaluating a backtick variable when the recipe run doesn't use it, so the Windows job ran
    the justfile's under Git's `sh`. The tree differs from `0767885` in `docs/pm` alone, which no build, test
    or gate reads (the gates leave out `docs/pm`), so `just ci` and the step's other checks weren't run
    again. `git grep -i` for the maintainer's name outside the root `LICENSE-MIT` finds the 78 lines it
    found, none added.

## Code review

Four reviewers read the branch at `db9b695` (2026-11-08), each through one lens: behaviour, the crates as
published libraries, the repository, and FFI soundness. Their findings and evidence are local
(`scratch/work/s0-1/flow/review-*.json`, and `scratch/work/s0-1/review-*/`): 23 findings, the set I/O mode
one found twice, so 22 distinct; 3 blockers, 5 major, 15 minor. Each was checked against the code first, the
behavioural ones reproduced (the fake's scripts, the reviewers' shim over the real `libsane`, the published
standard read again). All held, and all are fixed; none refuted, none deferred. Commits from `942553a` to
`9b8bd5a`, then this one.

**Fixed:**

1. *`Batch::next_page` failed every page on backends that answer `UNSUPPORTED` to `sane_set_io_mode`
   whatever the mode* (blocker; found by two lenses: SANE 1.4.0's `fujitsu`, `canon_dr`, `kodak`, `epson2`,
   `epsonds`, `escl`, `hp5590`, `kodakaio` and more, and HPLIP's `hpaio`, so most Epson, HP, network and
   Fujitsu scanners, slice 6's fi-6130 among them; each retry fed a sheet away). `942553a`, decision 57;
   the fake's `io-mode-unsupported` and four profiles (decision 70), and tests: a batch through the
   `fujitsu` profile, a page setting blocking back after the step-by-step calls. The reviewers' shim over
   the real `libsane`, which answers `UNSUPPORTED` as those backends do, now scans `test:0`.
2. *A frame of unknown height that ended before its first row was a page of height 0* (major).
   `942553a`, decision 59; one frame and three passes in the fake, and a unit test.
3. *A read answering `GOOD` with no bytes was retried at once, without end* (minor; the README promised no
   unbounded loop). `942553a`, decision 58: pauses growing to 10 ms, no cap, the README's claim corrected;
   the cancel test counts the reads.
4. *Progress on a three-pass page whose frames differ in padding passed what it expected* (minor).
   `942553a`, decision 60; a unit test with the reviewer's frames.
5. *A group's description was followed, though the published standard defines only a group's title and
   type: `Sane::open` crashed on a stand-in library that `scanimage` reads* (blocker). `2b424f4`, decision
   61; the fake's `@dangling`, and a test opening a device whose group's name and description point
   nowhere.
6. *`Sane::load_from` was safe, its SAFETY comments resting on a contract a safe function can't carry*
   (major). `3b6abdb`, decision 62; security item 9.
7. *On macOS `load()` opened a bare name, which dyld also seeks in the working directory* (major,
   security). `3b6abdb`, decision 63; security item 8; TEST-7 updated.
8. *Two copies of the crate in one program would each keep a slot of their own* (minor). `3b6abdb`,
   decision 64; security item 10.
9. *`RawHandle`'s `Send` comment said every call holds the call token; `sane_cancel` doesn't* (minor).
   `3b6abdb`.
10. *Descriptor re-reads made unasked weren't said, against the spec, and `SetInfo` couldn't grow*
    (major). `43e40c0`, decision 65; `crates/sane-dl.md`'s "For the seam" says it as built.
11. *A set the backend made, whose re-read failed, returned the same error as a refusal* (minor).
    `43e40c0`, decision 66.
12. *Caps a real scan can reach were reported in reason strings* (minor; A4 at 2400 dpi, 16-bit color, is
    3,340,837,224 bytes). `e689a42`, decision 67.
13. *A token's cancel landing after a batch had ended couldn't be cleared* (minor). `e689a42`, decision 69.
14. *`ConfigDir` refused a used `dll.d` as a made-up I/O error* (minor). `e689a42`, decision 68.
15. *The `readelf` gate counted binaries it couldn't read, and passed* (minor). `493b0c7`, decision 71.
16. *fake-libsane's "no dependencies" had no gate* (minor). `493b0c7`, decision 71.
17. *`just ci` didn't say that CI sets the require variables* (minor). `493b0c7`: the justfile's header and
    CI's comment say it.
18. *The justfile's doc lines: `gates`' a fragment in `just --list`, "those four" naming five, no line for
    `sane-dl-msrv`* (minor). `493b0c7`, and "each there" in `3b6abdb` (the package now holds six).
19. *`.gitattributes`' `*.sh` rule for scripts the repository doesn't have* (minor). `493b0c7`: the
    comment says what it's for.
20. *Local home paths in public docs, from before the branch* (major; 16 in 8 files, and six dead
    `file://` links). `9b8bd5a`, with the maintainer's name (the control file's note): 78 lines reworded,
    `git grep -i` for it finds only `LICENSE-MIT`; the checkouts by their public repositories, files in them
    by their repository paths, and the surveys' temporary download and cargo-registry paths reworded.
21. *`crates/sane-dl.md` gave the plan's decisions as 49* (minor). This commit: no count given.
22. *`status.md` said step 5's check was next* (minor). This commit: `status.md` rewritten.

**Checked, on the committed tree** (at `9b8bd5a`; this commit changes `docs/pm` alone, which no build,
test or gate reads):

- `just ci` with `SANE_DL_REQUIRE_LIBSANE=1` and `SANE_DL_REQUIRE_HEADER=1`, green: fmt, clippy, 221 tests
  in 14 harnesses with none skipped, doc, the gates (sane-dl's graph 10 packages, fake-libsane's none, 14
  binaries read), `sane-dl-package` (16 files), and `sane-dl-msrv`, 178 tests on 1.89 (the fake built on
  1.89 too, by the nested build: `ptr::dangling` needs 1.84 and the build script's `cargo::` 1.77).
- The test list: 221, step 5's 215 all kept (three renamed: the README's doctest a line down, the group's
  unit test, the cap's fake test), 6 new.
- `cargo clippy --all-targets -D warnings` for `x86_64-pc-windows-gnu` and `x86_64-apple-darwin`.
- `just sane-dl-package` in a fresh `CARGO_TARGET_DIR` under `scratch/work/s0-1/fix/package/`: the dry
  run passes, 16 files (`build.rs` new), nothing published.
- Make-fails, on a detached worktree under `scratch/work/s0-1/fix/`, removed after: the `readelf` gate
  given a non-ELF file and a missing path, and fake-libsane given a dependency, each failing the gates
  saying why; and each behavioural fix undone in turn, each failing its tests (the old `set_io_mode` call:
  `Status { UNSUPPORTED, SetIoMode }`; reading again at once: 17,219 empty reads in 300 ms; a group's
  description read: the child killed by `SIGSEGV`; zero-row frames: the two fake tests and the unit test;
  the old progress: `(16, Some(12))`).
- The `links` probe (`scratch/work/s0-1/links-probe/`): cargo refuses sane-dl 0.1.0-alpha.1 beside a copy
  at 0.2.0. The reviewers' I/O mode shim over the system's `libsane.so.1`, scanning `test:0` through an
  isolated configuration: a page, as from the plain library.
- Not run again, and why: ThreadSanitizer (the cancel's synchronization, the alive flag and the call token,
  is untouched; `clear_cancel` is the flag's swap and the read loop's pause is on one thread); the
  reviewers' AddressSanitizer run (the unsafe code changed by one pointer read fewer and the library names
  tried); the header contract (`ffi.rs` unchanged; it ran in `just ci` all the same); `scanimage`'s pages
  beside ours (no assembled byte changed, and the tests against the `test` and `pnm` backends ran); the MSRV
  probe (no dependency added).
- The privacy scan of every file and commit message the slice committed runs last, after this commit; its
  result is in the run's local record (`scratch/work/s0-1/flow/notes.md`), and anything it finds is fixed
  in a commit of its own.

**What only CI shows** (it waits on the push), beyond step 5's list: the build script and `links` on
Windows and macOS, and on macOS `load()` refused with the three paths named (TEST-7 tries them with
Homebrew's SANE). Windows and macOS on this code are TEST-8.

**Backlog:** UP-11 (the backends that answer `UNSUPPORTED` to a blocking `sane_set_io_mode`, against the
standard), TEST-8 (this slice's code on CI's Windows and macOS), TEST-7 revised (absolute paths on macOS);
TEST-2 still closes with CI's run of the tests.

**For slice 2, of the seam's SANE unknowns:** a backend's blocking promise isn't to be trusted: many answer
`UNSUPPORTED` to `sane_set_io_mode` either way and read blocking, so the seam's "blocking or not with the
select fd" (section 3) is the backend's to offer, and `sane-dl` reads a page blocking without asking. Nothing
new on names that change on replugging, or on stopping after the page in flight.
