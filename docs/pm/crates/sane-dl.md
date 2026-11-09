# sane-dl

Layer: Devices (`crates/README.md`). Paths are from `docs/pm/`.

**As built** in S0 slice 1 (`slices/s0-1-sane-dl.md`, which has the design, its decisions, what each step
checked and the code review's fixes), its public API made safe in S0 slice 1a
(`slices/s0-1a-sane-dl-safe-api.md`: the shapes of loading weighed, the process's slot, the gate over the
public API): `/crates/sane-dl/`, 0.1.0-alpha.1, ready to publish and not published; MSRV 1.89
(`decisions.md`). No public item is `unsafe` (`decisions.md`, "Public APIs are safe Rust"). Its tests drive
SANE's own `test` and `pnm` backends and `fake-libsane` (`crates/fake-libsane.md`).

- **What:** SANE for Rust, loaded at run time (M5 proposal 6, M5 ask 6, entry 5): `Sane::load()` opens
  `libsane.so.1` with `libloading` (0.9, ISC) through the system loader's search (on macOS `libsane.1.dylib` at
  Homebrew's and MacPorts' absolute paths, never a bare name, which dyld would also seek in the working
  directory; none on Windows or Apple's other systems, `NoDefault`). Nothing fails until SANE is asked for,
  and then it says so (`LoadError`: `NotFound` with the names tried, `NoDefault`, `MissingSymbol`, `Init`,
  `UnsupportedVersion`, `AlreadyLoaded`, `SlotTaken`, `SlotRefused`). Never unloaded; one `Sane` per process,
  another after it ends.
  - **Which library:** the loader's choice alone; no code, the crate's or its caller's, names a path (slice
    1a removed `Sane::load_from`, an `unsafe fn`). What loads is what the system's loader finds under SANE's
    name, in the environment the process started with, taken as SANE as a library linked at build time is
    (the soname names SANE 1's ABI, which the header contract checks the bindings against), and never closed.
    Another SANE (one an app ships, a build of one's own, the tests' fake) is loaded through the loader's own
    search path given to the process as it starts, `LD_LIBRARY_PATH` (`DYLD_LIBRARY_PATH` on macOS), naming a
    directory of SANE's libraries alone, or a run path built into the binary or shared library that holds the
    crate (glibc searches the calling object's). The loaders ignore those variables in a set-id process, and
    dyld in a process it restricts (the hardened runtime: FEAT-13). So M5 proposal 6's `SHEAF_LIBSANE`
    becomes, for sheaf's tests, the loader's variable on the test's child, and a worker's client that wants
    another SANE sets it on the worker's command beside `SANE_CONFIG_DIR`.
  - **What that rests on** (slice 1a's code review): the process's environment, as every library loaded by
    name does, kept to two rules `Sane::load`'s docs give its starter and the program. Every entry of the
    search path (and of a run path) absolute: glibc reads an empty entry as the working directory and looks a
    relative one up from it as the library loads, and `set_current_dir` is safe, so with one the program's own
    code would choose (std's name lookups are open to it alike). And `LD_LIBRARY_PATH` (or `SHLIB_PATH`,
    `LIBPATH`) never changed inside a process using SANE: the system's loader reads it as the process starts,
    but SANE's `dll` reads the first of the three that's set with `getenv` each time it loads a backend, at a
    listing or an open, before its own directory, and `set_var` is safe before edition 2024. Said rather than
    made `unsafe`: a change after `load` reaches `open` and `devices` too, and the Vulkan loader's variables,
    under ash's and wgpu's safe entries, are the same case (the slice's decision 33, for review). In a set-id
    process glibc clears `LD_LIBRARY_PATH` but not the other two, which `dll` then reads, so the docs tell a
    set-id program to run SANE in a process it starts (UP-14, for sane-backends' `dll`).
  - **One SANE per process, across copies of the crate:** cargo refuses two versions in one build (the
    manifest's `links = "sane-dl"`, with a build script that builds nothing), but each shared library built
    with the crate holds a copy of its own. On Linux every copy takes one slot for the whole process before
    the library opens and holds it until after `sane_exit` (`src/process_slot.rs`, std alone, no `unsafe`): a
    Unix datagram socket bound to an abstract name formed from the pid namespace (`/proc/self/ns/pid`'s device
    and inode) and the pid, `sane-dl/slot/pidns/<device>/<inode>/pid/<pid>` (`…/pidns/none/pid/<pid>` where
    `/proc` can't be read), fixed for good so every version finds every other; the kernel frees it however the
    process ends. A load that finds it bound tries again for 100 ms, since every child the process spawns
    holds it from its fork until its exec (slice 1a's code review: without the wait, a load beside a thread
    spawning programs was refused up to half the time); still held (another copy with SANE loaded, a child
    forked without an exec, a local user binding the name first) is `SlotTaken`, and this copy's own live
    `Sane` is `AlreadyLoaded`; any other refusal (a sandbox that forbids Unix sockets or that name: a profile
    allows `@sane-dl/slot/**`) is `SlotRefused`, never SANE unguarded. The name is in the public docs, a
    promise across versions. It can't see a change of the process's network namespace or of its view of
    `/proc` between two copies' loads, nor another user of libsane in the process; there, and on every system
    but Linux, where a copy sees only its own loads (BUG-8), the program keeps to one copy, as the docs
    say.
  - Bindings of its own (`sane_dl::ffi`, public, types and constants only), from SANE 1.4.0's public-domain
    `sane.h`: its structs, types and constants, its function-like macros as `const fn`s; the types of its 14
    functions and `SANE_Auth_Callback` are `pub(crate)` (slice 1a), since nothing public hands one out, so
    nothing callable is public. The grant asks that the notice be kept and changes marked: both done in the
    file's header. No bindgen, no build-time link; nothing from `saneopts.h`, which is GPL with SANE's
    exception.
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
    image; `NO_DOCS` from `sane_read` as well as `sane_start` (`Page::NoDocs`, with the pages delivered).
    `Image`'s `Debug` gives every field but the data, whose length alone it gives (`data: <n> bytes`), and
    `Page`'s, which derives, prints that (slice 1a, closing BUG-7). A
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
  (`just gates`: no `links = "sane"`, and no binary built with libsane among its `NEEDED` entries). Its
  public API is gated (`just public-api`, slice 1a): every public item, read from rustdoc's JSON on a pinned
  nightly, the crate's own type aliases followed, and none an `unsafe` function, method or trait, an unsafe
  function pointer, a raw pointer in a public signature, or another crate's item re-exported; an `unsafe
  impl` is listed, not failed.
- **Tested by** (221 tests on Linux as slice 1 built it, 230 after slice 1a; each plan's "So far" counts
  them):
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
    `sane-dl` to them. **Loaded as SANE, through `Sane::load()`** (slice 1a): the tests copy it under SANE's
    file name into a directory of its own per test binary, written then renamed, and spawn each child with the
    loader's variable naming that directory first; every load of it is checked by its log, so a test can't pass
    on the system's libsane. No route a published API exposes. On Windows, where `load()` has no library, the
    tests that load it are ignored, each saying why; slice 5's `sane` backend reaches it the same way, copying
    the small helper.
  - What needs a path, inside the crate: `NotFound` (an absolute path where nothing is) and `MissingSymbol`
    (the system's C library, already loaded in the process) as unit tests over the private loader.
  - The process's slot (Linux): its name bound first refuses a load before the library opens; held while a
    `Sane` or a handle lives, freed after; freed by a refused load; a socket the system refuses (descriptors
    exhausted under a lowered limit, through util-linux's `prlimit`, skipped where that's missing) is
    `SlotRefused`; bound by hand, `SlotTaken`; 200 loads beside a thread spawning programs, none refused.
    A probe outside the workspace (two `cdylib`s each holding a copy, built apart) showed two copies starting
    SANE twice before it and the second refused after.
  - `Image`'s and `Page`'s `Debug` of a 1 MiB image under 1 KiB, its length given and none of its bytes.
  - The bindings checked against the installed `sane.h` (sanetui's idea), statuses included: printscan's own
    fake gave `NO_DOCS` the code 6, which is `JAMMED`. Here against 1.4.0's header, in CI against Ubuntu's
    (TEST-2).
  - Cancel: a cancel racing close over 10,000 rounds, never reaching a closed handle, and the cancel tests
    once under ThreadSanitizer.
  - Here and in CI on Ubuntu with `SANE_DL_REQUIRE_LIBSANE=1` and `SANE_DL_REQUIRE_HEADER=1`, so nothing
    skips; on Windows and macOS the tests that need libsane or the header say they skipped.
- **Open:**
  - macOS against Homebrew's sane-backends (TEST-7): whether `load()`'s absolute paths find it, and the fake's
    tests through `DYLD_LIBRARY_PATH` beside it.
  - Two copies of the crate built apart in one process on every system but Linux (BUG-8): no kernel-freed
    name per process in std there, so the program's rule.
  - A SANE an app ships, on macOS under the hardened runtime, where dyld ignores its variables (FEAT-13): only
    if such an app comes.
  - No authorization callback (FEAT-11), until a networked backend needs one.
  - Defects found in SANE 1.4.0's `test` backend, `scanimage` and several backends' `sane_set_io_mode`,
    each an upstream report waiting on review (UP-7 to UP-11).
