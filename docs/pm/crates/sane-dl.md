# sane-dl

Layer: Devices (`crates/README.md`). Paths are from `docs/pm/`.

**As built** in S0 slice 1 (`slices/s0-1-sane-dl.md`, which has the design, its decisions, what each step
checked and the code review's fixes): `/crates/sane-dl/`, 0.1.0-alpha.1, ready to publish and not published;
MSRV 1.89 (`decisions.md`). Its tests drive SANE's own `test` and `pnm` backends and `fake-libsane`
(`crates/fake-libsane.md`).

- **What:** SANE for Rust, loaded at run time (M5 proposal 6, M5 ask 6, entry 5): `libsane.so.1` opened by
  `libloading` (0.9, ISC) through the system loader's search (on macOS `libsane.1.dylib` at Homebrew's and
  MacPorts' absolute paths, never a bare name, which dyld would also seek in the working directory), or a
  library the caller names (`Sane::load_from`, an `unsafe fn`, since it trusts the library's code and its
  symbols' types; sheaf's tests would name theirs by `SHEAF_LIBSANE`, an environment variable that's the
  app's). Nothing fails until SANE is asked for, and then it says so (`LoadError`: `NotFound` with the names
  tried, `NoDefault` on Windows, `MissingSymbol`, `Init`, `UnsupportedVersion`, `AlreadyLoaded`). Never
  unloaded; one `Sane` per process, another after it ends; one copy of the crate per program (its manifest's
  `links = "sane-dl"`, with a build script that builds nothing).
  - Bindings of its own (`sane_dl::ffi`, public, types and constants only), from SANE 1.4.0's public-domain
    `sane.h`: its 14 functions as function-pointer types, its structs, types and constants. The grant asks
    that the notice be kept and changes marked: both done in the file's header. No bindgen, no build-time
    link; nothing from `saneopts.h`, which is GPL with SANE's exception.
  - Statuses and frame types as integers with named constants, never Rust enums (`Status`, `FrameFormat`):
    statuses 12 and 13 and frame types 0x0A to 0x12 are reserved under `#if 0`, and an enum read from FFI is
    undefined behaviour the day a backend returns one. Every status is said with the function that answered
    it (`Error::Status { status, call }`).
  - The library and its calls: every call but `sane_cancel` under one lock, so no two overlap; `Sane` is
    `Send + Sync`, a `Handle` `Send` and used from one thread at a time.
  - A device list under a deadline (about 9 s here with nothing attached through the system's
    configuration, 0.001 s through a one-backend one): the listing runs on a thread of its own, and past the
    deadline calls say `ListingPending` rather than block until it returns.
  - A cancel token (`CancelToken`, `Clone + Send + Sync`) that calls `sane_cancel` from any thread (the
    standard allows it asynchronously), synchronized with close: the standard forbids every other call,
    `sane_close` included, until the cancelled one returns, so a late cancel never reaches a closed handle
    (airbridge's alive flag, MIT OR Apache-2.0, followed in our own code and credited). It also flags the
    cancel for the handle's next start or read, so a backend that ignores `sane_cancel` stops being read.
    `sane_cancel` after every batch and every error status from a start, the parameters or a read.
  - Opening a known device fast: `open` by name needs no listing (SANE's `dll` loads the backend a name
    starts with), and `ConfigDir` writes a private `SANE_CONFIG_DIR` whose `dll.conf` names only the given
    backends, with an empty `dll.d/` (NAPS2's practice), for listings that don't wait on every backend's
    probe. It never sets the variable: the caller gives it to the process it starts.
  - Options typed: descriptors (`OptionDescriptor`), constraints (ranges, word lists, string lists), units,
    capabilities, values (`Value`, `Fixed`); refusals before the backend sees a call SANE's rules don't
    allow; descriptors read again whenever a set says so (`SANE_INFO_RELOAD_OPTIONS`), when `sane_start`
    returns `SANE_STATUS_INVAL`, and before any call on an option SANE now describes differently, each
    re-read counted (`Handle::options_generation`); a set made whose re-read fails is `ReloadAfterSet`. Of a
    group's descriptor only its title and type are read, as the standard defines no more.
  - Frames assembled (`Batch::next_page`, `Image`): `lines = -1` read to EOF; padded rows; 1-bit rows with 1
    black and the first pixel in the high bit; 16-bit samples in native order; three-pass frames into one
    image; `NO_DOCS` from `sane_read` as well as `sane_start` (`Page::NoDocs`, with the pages delivered). A
    page reads blocking as a device opens, setting the mode only where the step-by-step calls changed it,
    since several backends answer `UNSUPPORTED` to either mode; an empty read is made again after a pause of
    up to 10 ms. NAPS2 handles 1- and 8-bit transfers only, so 16-bit, padded and unknown-length frames go
    beyond every frontend, and are proven against the test backend and beside `scanimage`'s pages.
  - Caps on hostile input (`Limits`): devices, strings, options, list entries, option sizes, frames, lines,
    the read chunk, each checked before the work it caps; a device list, an option count, a frame or a line
    past its cap is `LimitExceeded`, naming the cap (`LimitKind`).
- **For the seam** (`crates/scan-device-seam.md`): each frame's parameters as SANE gives them (format, last
  frame, bytes and pixels per line, lines, depth), in `Image::frames` and from `parameters()`; each read's
  status; each option reload, SANE's word in what the call returns (`SetInfo`, `StartInvalid`) and every
  re-read, whatever its cause, counted by `Handle::options_generation`; `NO_DOCS` and which call returned it,
  with the pages so far; every status with its call, unknown ones kept as integers; the token, and "feed no
  more" left to the caller, who starts no next page. All exposed, so the `sane` backend decides what they mean
  and `sane-dl` stays SANE alone. What the seam takes of it, as S0 slice 2 revised the note (its SANE rows,
  and the table of what the `sane` backend maps in "The model, for slice 3"): `DeviceName`'s bytes for the id,
  read as Latin-1 and given back byte for byte; `Image` converted by the `sane` backend into the raw layout
  (1-bit gray inverted, 1-bit RGB expanded, 16-bit made little-endian), `sane-dl` passing SANE's samples on
  unchanged; "feed no more" as the backend starting no next page and dropping the batch, the token kept for a
  stop at once; `backend_of` and `ConfigDir` (`Search::BeforeSystem`) for the worker that opens a stored id,
  which lists its backend before it opens; `DeadlineExceeded` and `ListingPending` as the model's `TimedOut`,
  `INVAL` from a start twice as `SettingsRefused`, `ACCESS_DENIED` as `AccessDenied`, and every other error as
  the model's `Driver` (S0 slice 2's code review).
- **Prior art:** `sane-scan` 0.1.2 (2023), the one published binding, links at build time (bindgen, libclang,
  `-lsane`), passes a one-byte `bool` where backends read a four-byte word, sizes `lines = -1` buffers wrong,
  turns a mid-page `JAMMED` into a truncated `Ok`, and hasn't answered a PR since 2023. Three permissive
  apps load libsane as planned (airbridge, janus-scan, ruscan), each weeks old and none a library; airbridge's
  SANE backend was read in full before its pattern was followed. printscan-sane (an open PR) has
  `protocol.rs` and `scan/frame.rs` to cross-check against. sanetui's header-contract test (Apache-2.0) is
  the idea behind the bindings' check, written in our own code.
- **Boundary:** SANE alone; no device model (that's `scan-device`'s `sane` backend), so it's useful to anyone
  who wants SANE. Dependencies: `libloading` and `thiserror`; dev: `rusty-fork` (a child process per test
  that loads a library) and `tempfile`. The devices layer's gate holds its graph; nothing links libsane
  (`just gates`: no `links = "sane"`, and no binary built with libsane among its `NEEDED` entries).
- **Tested by** (221 tests on Linux as built; the plan's "So far" and "Code review" count them):
  - SANE's own `test` backend (`libsane1` installed, the child's `SANE_CONFIG_DIR` an isolated `ConfigDir`
    naming `test`): a 10-sheet feeder then `NO_DOCS`; gray and color at 1, 8 and 16 bits; unknown height, alone
    and from the feeder (on 1.4.0 the hand scanner gives `lines = -1` from the feeder too); short reads;
    three-pass frames in all six orders; padded rows (`ppl-loss`); 16-bit byte swapping; every status
    `read-return-value` forces; `SANE_INFO_INEXACT` and every type, constraint and unit with
    `enable-test-options`; non-blocking reads with a select fd; fuzzy parameters. It has no duplex. Its reads
    are paced in the tests, against a deadlock of its own (UP-8).
  - SANE's `pnm` backend: PBM, PGM and PPM images the tests write, scanned byte for byte, three-pass and as a
    hand scanner (it reads 8-bit images only, so no 16-bit there); its `filename` and automatic `resolution`.
  - `fake-libsane` (`crates/fake-libsane.md`), a never-published `cdylib` exporting SANE's API, narrowed to
    what those can't do: seven backends' feeder and duplex layouts, duplex pages marked in their pixels, a jam
    or an open cover at sheet N, `2500x0`, descriptors that change after a set, hangs and an abort, a slow
    listing, and hostile values of every kind; and it checks SANE's calling rules, so every test holds
    `sane-dl` to them.
  - The bindings checked against the installed `sane.h` (sanetui's idea), statuses included: printscan's own
    fake gave `NO_DOCS` the code 6, which is `JAMMED`. Here against 1.4.0's header, in CI against Ubuntu's
    (TEST-2).
  - Cancel: a cancel racing close over 10,000 rounds, never reaching a closed handle, and the cancel tests
    once under ThreadSanitizer.
  - Here and in CI on Ubuntu with `SANE_DL_REQUIRE_LIBSANE=1` and `SANE_DL_REQUIRE_HEADER=1`, so nothing
    skips; on Windows and macOS the tests that need libsane or the header say they skipped.
- **Open:**
  - `Image` (and `Page`, which holds one) derives `Debug` over its bytes, so `{:?}` of a page prints every
    byte (BUG-7, from S0 slice 2's code review): written by hand for the next alpha, with a changelog entry.
  - macOS against Homebrew's sane-backends (TEST-7): whether `load()`'s absolute paths find it.
  - No authorization callback (FEAT-11), until a networked backend needs one.
  - Defects found in SANE 1.4.0's `test` backend, `scanimage` and several backends' `sane_set_io_mode`,
    each an upstream report waiting on review (UP-7 to UP-11).
