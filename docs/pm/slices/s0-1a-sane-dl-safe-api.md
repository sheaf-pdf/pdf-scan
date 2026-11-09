# S0 slice 1a: sane-dl's public API made safe

Status: running, on branch `s0-1a-sane-dl-safe-api`, from `master` at `3f33be5`, since 2026-11-09. Milestone:
`milestones/s0-devices.md`, slice 1a (added there by step 1, after slice 1).

Paths: `docs/pm/` files from `docs/pm/`, as everywhere here; the code's from the repository's root, written
with a leading `/` (`/crates/sane-dl/`, `/justfile`); `scratch/` from the root, local and never tracked.

## What it builds

A follow-up to slice 1 (`slices/s0-1-sane-dl.md`), asked for on 2026-11-09: "i think that we should try to
define safe apis where possible". That confirms `decisions.md`'s ruling of 2026-11-08 ("Public APIs are safe
Rust; `unsafe` contracts stay inside a crate"), which slice 8 recorded after slice 1's code review had made
`Sane::load_from` a `pub unsafe fn` (slice 1's decision 62, its security items 1 and 9). `sane-dl` is
0.1.0-alpha.1, ready to publish and not published, so its API can still change freely.

Every public item of `sane-dl` whose contract a caller keeps is given a safe shape, or named as an exception
with why:

- **Loading:** the library is chosen by the system's loader alone, never by a path from code. `Sane::load_from`
  goes; `Sane::load` stays, its safety argument written down whole; another SANE is loaded by the loader's own
  search, set for a process as it starts. The API space is weighed below ("Loading: the shapes weighed").
- **The tests** load `fake-libsane` through `Sane::load` itself, the loader pointed at it by the child process's
  environment, with no route a published API exposes; what can only be tested over an arbitrary path (a library
  missing SANE's functions, a path where nothing is) is tested inside the crate.
- **`ffi`'s function-pointer types** (unsafe to call, though nothing public hands one out) move inside the crate,
  as `twain-dsm`'s did.
- **One SANE per process, whatever copies of the crate it holds:** cargo's `links` refuses two copies in one
  build; on Linux a process-wide slot refuses a second copy built apart (two shared libraries each holding one).
- **BUG-7:** `Image`'s `Debug` by hand, since this change to the API takes it.
- **A gate** over every public item, read from rustdoc's JSON on a pinned nightly: no `unsafe` function, method
  or trait, no raw pointer or unsafe function pointer in a public signature (the crate's own type aliases
  followed), kept in `just ci` and CI.

**Later slices build on it** (`milestones/s0-devices.md`): slice 5's `sane` backend loads SANE with `Sane::load`
and drives `fake-libsane` in its tests through the same route as `sane-dl`'s (copying the small helper, as slice
1 left it to do for the fake's build); slice 4's worker loads the system's SANE, and a client that wants another
gives the worker's process the loader's search path at spawn, beside `SANE_CONFIG_DIR`; `sane-dl`'s first
publish waits on review either way. Slice 3 (`scan-device`, running beside this slice in the main checkout)
doesn't touch `sane-dl`.

Nothing is cut from sheaf, so there's no harness comparison; the sheaf plan files cited (M5 proposals 6 and 22,
entry 5, `libraries.md`) are unchanged since `3232a35` (checked 2026-11-09). No PDF crate is touched.

## Taken as it starts

No ask in `asks.md` bears on it (ask 5 is `pdf-images`'). It takes the ruling above as its brief: no `pub unsafe
fn`, unsafe trait or unsafe method in what a user of a published crate calls; a contract a caller would keep
bound instead by types, lifetimes, ownership, a checked constructor or a library kept loaded; an exception named
only where no safe shape exists.

## The design

### What a caller keeps today

Every public item of `sane-dl` at `3f33be5`, read from rustdoc's JSON (Baselines: 243 functions, one of them
unsafe, 15 type aliases of unsafe function pointers, no raw pointer in a public function's signature) and from
its docs, for a rule its caller must keep:

| Item | The rule | Bound by, after this slice |
|---|---|---|
| `Sane::load_from(path)` | the library is one the process may run, its SANE functions of `sane.h`'s types (`unsafe fn`) | Removed: the loader chooses (below) |
| `Sane::load()` | that a library found under SANE's name is SANE | Said, as a build-time link's argument (below); no caller's rule |
| `Sane::load()` | one copy of the crate in the process | `links` within a build; on Linux a process-wide slot (step 3); on macOS and the BSDs the program's rule still, named (decision 9, BUG-8) |
| `Sane`, `Handle`, `CancelToken` | calls never overlap; a handle on one thread at a time; no cancel on a closed handle | Types and locks, as built: `Handle` is `Send` and not `Sync`, the call token, the alive flag |
| `Handle::select_fd()` | the descriptor is the backend's, valid until the next start, cancel or frame's end; never closed | Plain data: a `RawFd`, any use of which is the caller's own `unsafe`. A `BorrowedFd<'_>` tied to the handle would be unsound: a token's cancel on another thread may end the descriptor while it's borrowed (decision 8) |
| `ConfigDir`, `CONFIG_DIR_VARIABLE` | give the value to the process spawned; run it from a trusted directory | No soundness at stake: in-process `set_var` is std's own `unsafe fn`; the directory is advice (security) |
| `ffi`'s 14 function types, `SANE_Auth_Callback` | unsafe to call | `pub(crate)` (step 2): nothing public hands one out, and none stays in the public API |
| `ffi`'s structs | raw pointers in their fields | Plain data, as `twain-dsm`'s: nothing public takes or returns one, and reading through a field is the reader's own `unsafe` |
| `Image` (and `Page`) | (`Debug` prints every byte: BUG-7) | `Debug` by hand (step 4) |

### Loading: the shapes weighed

What `load()` already assumes, said whole: **the library is the one the system's loader finds under SANE's
name, in the environment the process started with, and a library under SANE's name is SANE.** On Linux and the
other Unix-likes that's the soname `libsane.so.1`, searched as a library linked at build time is (glibc's
dlopen(3): the calling object's `DT_RPATH` where it has no `DT_RUNPATH`, `LD_LIBRARY_PATH` as it was at start,
the calling object's `DT_RUNPATH`, the loader's cache, the default directories); the soname is the name of SANE
1's ABI, which the bindings are checked against (the header contract). On macOS it's the package managers'
absolute paths, which dyld searches after `DYLD_LIBRARY_PATH` (as it was at launch) under the same leaf name
(Apple's dlopen(3)). The program's code chooses nothing. Whoever starts the process chooses through those
variables every library it loads by name, as they do with `LD_PRELOAD`: outside what any Rust crate can promise,
as for every library linked at build time, which safe Rust uses through every `-sys` crate; and the loader
ignores them in a set-id process (glibc's secure-execution mode) and in a macOS process dyld restricts (set-id,
or the hardened runtime every notarized app has, unless an entitlement allows the variables). libloading's
`Library::new` is unsafe because opening runs a library's initialisers and closing its finalisers: the first is
the build-time link's too, and `sane-dl` never closes the library. (ash and openxr make
even their soname loaders `unsafe`, `Entry::load`, partly because their `Entry` unloads the library on drop; their
build-time-linked `Entry::linked` is safe, though the Vulkan loader it links picks drivers by environment
variables. `sane-dl`'s `load` keeps its slice 1 shape: decision 2, for review.)

Each shape below is weighed against that assumption, against slice 5's backend and slice 4's worker (which load
the system's SANE, and may give a worker another), against the tests' need to load the fake from every runner,
and against whether its safety argument is honest or a promise renamed.

1. **The loader chooses; no path from code** (taken). `Sane::load()` alone. Another SANE is loaded as any
   library is: the loader's search path set for the process as it starts (`LD_LIBRARY_PATH` on Linux and the
   BSDs, `DYLD_LIBRARY_PATH` on macOS), or a run path built into the program (`$ORIGIN/…`, a builder's choice, as
   a bundled library's always is). The argument is `load()`'s, unchanged. It serves the system's SANE (every
   production use planned), a Linux app that bundles SANE (AppImage and snap wrappers set the variable; Flatpak's
   runtime puts it on the loader's path), slice 4's worker (its client sets the variable on the worker's
   `Command`, as it sets `SANE_CONFIG_DIR`), and the tests on Linux and macOS (the variable set on each child at
   spawn, before its loader starts). It costs two things, both decisions for review: on Windows, where `load()` is
   `NoDefault`, the fake can't be loaded at all, so the fake's tests are ignored there, saying why (decision 3;
   no Windows user can reach that code either); and a notarized macOS app (the hardened runtime) can't point dyld
   at a SANE it bundles (FEAT-13, later).
2. **A caller's path, checked as far as it can be:** absolute, canonical, a regular file, owned by root or the
   user, not writable by others, its directories likewise, all 14 symbols present (read from the file before it's
   opened, with `object`). Rejected. The checks say who could have written the file, not that its code is SANE's,
   keeps SANE's contract, or has harmless initialisers. Allowing the user's own files (which the tests need: the
   fake is built by the user) leaves safe code free to load any library the user can write, which is the unsound
   case slice 1's code review found; allowing root's alone doesn't serve the tests and adds nothing to (1) but an
   `/opt` prefix. A safe function over it would carry the same promise as `load_from`'s, renamed. `twain-dsm`'s
   `Load::Path` is safe for a reason that doesn't carry over (`crates/twain-dsm.md`, "Safe by construction"):
   the DSM is signed, and its Authenticode signer (the TWAIN Working Group) is checked in the file before any of
   its code runs, which says whose code it is; a libsane file carries no signature to check (a distribution
   signs its packages, not the shared object in them).
3. **A type that carries the trust, made only by the crate** (`SaneLibrary::system()`, then `Sane::start(lib)`).
   Rejected: made from the crate's fixed list it is (1) in a type; made from a caller's path it needs an `unsafe`
   constructor, which is `load_from` moved. It would add a type and no safety.
4. **An environment variable of the crate's own** (`SANE_DL_LIBRARY`, read by `load()`). Rejected: it's read when
   `load()` runs, not as the process starts, so code in the process can set it first (`set_var` is `unsafe` only
   from edition 2024, and its contract is about threads, not values), and it isn't ignored in a set-id process as
   the loader's own variables are, unless the crate re-implements that check (more `unsafe`, per system). Weaker
   than (1), for nothing (1) lacks.
5. **A `ConfigDir`-like route through SANE's own `dll`:** the system's libsane loading the fake as a backend
   (`libsane-<name>.so.1`, its functions prefixed `sane_<name>_`). Rejected for the fake: `dll` would sit between
   `sane-dl` and the fake, which would then see `dll`'s calls rather than `sane-dl`'s (its rule checks and
   hostile values lose their point: `dll` rewrites device lists and prefixes names), and it needs the system's
   libsane, which macOS's runner and the MSRV job lack. It's how the real `test` and `pnm` backends are reached,
   and stays so.
6. **A `#[cfg(test)]` route.** Rejected: integration tests link the crate built without `cfg(test)`, and slice
   5's tests are another crate's.
7. **A cargo feature, or a `--cfg` the builder sets, that turns a test loader on.** Rejected: a feature is
   additive, so any crate in a graph can turn it on, and a safe loader behind it is unsound for the whole graph
   (an `unsafe` one is a `pub unsafe fn` again); a `--cfg` stays the final builder's, but every build in the
   workspace would need it (clippy, docs, the MSRV recipe, a sanitizer run's `RUSTFLAGS`, slice 5's crate), for a
   promise moved to a flag.
8. **`load_from` kept as a `pub unsafe fn`, named an exception.** Rejected: the ruling allows one only where no
   safe shape exists, and (1) serves every need found.

Within (1), Windows keeps no default library: SANE isn't its scanner API, and a default would bring its DLL search
order (the application's directory, the current one after the system's, `PATH`) into play, where today none is.

### sane-dl's API, as this slice leaves it

- **`Sane::load()`**, unchanged in what it does. Its docs say the argument above in a user's words, and how
  another SANE is loaded (the loader's variable at the process's start, naming a directory that holds SANE's
  own libraries alone, since the variable reaches every library the process loads by name; or a run path built
  in; on macOS not for a process dyld restricts). `Sane::DEFAULT_LIBRARIES` stays, data. `LoadError::NoDefault`
  no longer points at `load_from`; `NotFound` names what was tried, as now.
- **Inside the crate,** the loader that takes names (`Sane::start`, and `Api::load` beneath it) becomes a private
  `unsafe fn` with a `# Safety` of its own (each name the system's SANE by the loader's search, or a library
  already in the process with none of SANE's names, or a path where nothing is), so the promise `load_from`'s
  callers made is the crate's own, kept at its two call sites: `load()` and the unit tests. (In step 1, while
  `load_from` still calls it, the `# Safety` names `load_from`'s promise as a third case; step 2 takes it out.)
  Every `SAFETY:` comment that leans on `load_from`'s caller (`Api::load`, `look_up`) is rewritten.
- **`ffi`** stays public, types and constants only (decision 6): `sane.h`'s structs, typedefs and constants, its
  function-like macros as `const fn`s. Its 14 function-pointer types and `SANE_Auth_Callback` become `pub(crate)`,
  as `twain-dsm`'s five did; its module doc and its header's list of changes say so. `fake-libsane` takes the file
  by `#[path]` already, so it sees them; the header contract, which asserts each function's type against the
  header, takes the file by `#[path]` too (it can't reach them through `sane_dl::ffi` any more), which makes it
  check the very file the crate compiles, as the fake's compile-time assertions do.
- **The process's slot** (step 3). Today a per-copy atomic (`LOADED`), taken before `sane_init` and freed after
  `sane_exit` or a failed load. On Linux, beside it, a process-wide one that every copy of the crate finds: a Unix
  datagram socket bound to an abstract name formed from the process's pid namespace (the inode of
  `/proc/self/ns/pid`, or none where it can't be read) and its pid, through std alone
  (`std::os::linux::net::SocketAddrExt::from_abstract_name`, `UnixDatagram::bind_addr`, both stable since 1.70,
  so within 1.89). It's bound before the library opens and held until after `sane_exit` (dropped with the
  instance's last reference, as the atomic is), and the kernel frees it when the process ends, however it ends.
  std makes the socket close-on-exec, so a spawned program never holds it; a child forked without an exec holds
  the parent's name until it exits or execs, and the parent's next load meanwhile is `AlreadyLoaded`, a refusal
  (a probe here, `scratch/work/s0-1a/plan-review/abstract.rs`, built on 1.89: a second bind `AddrInUse`, a bind
  after the first's drop free). The name is fixed for good, a comment says so, so every version of the crate
  finds every other. Bound already: `LoadError::AlreadyLoaded`, its docs saying "by this or another copy of this
  crate". The system refusing the
  socket for another reason: a new `LoadError` variant carrying the `io::Error` (named by the step; `LoadError` is
  `#[non_exhaustive]`), refused rather than loaded unguarded. No `unsafe`; a module of its own. Nothing to do on
  Windows (`load()` is `NoDefault` before any slot), and on macOS and the BSDs the per-copy atomic and `links`
  stay, the case of two copies built apart named (decision 9).
- **`Image`'s `Debug`** (step 4) by hand: its format, depth, width, height, stride, frames, and its data's length
  (`data: <n> bytes`), never the bytes; a `#[derive(Debug)]` already gives `Image` an `impl Debug`, so the
  public API's list doesn't change; `Page` derives `Debug` still, and so prints that. Its public fields stay
  (slice 1's decision 21). `Value`'s word arrays (a gamma table) are left as they are: an option's value is
  capped at 1 MiB and isn't a page.
- **Nothing else changes** in what the API does: no new dependency, no cap changed, the errors as they are but
  the one new variant. The seam's mapping (`crates/scan-device-seam.md`, "What the `sane` backend maps") holds as
  written: `NotFound` is `NotInstalled`, every other `LoadError` `Driver`.
- **The version** stays 0.1.0-alpha.1 (decision 11): nothing was published. Its changelog entry,
  `0.1.0-alpha.1 (unreleased)`, is rewritten to the crate as it now is, with one line saying what changed before
  the release (`load_from` gone, `ffi`'s function types inside the crate, `Image`'s `Debug`), for anyone who
  built from the repository.

### The tests: how they load what they load

- **The fake, through `Sane::load()`.** The parent builds the fake as now (`fake_library()`, a nested `cargo
  build` into the test's own target directory), then copies it under SANE's leaf name, the file name of
  `Sane::DEFAULT_LIBRARIES[0]` (`libsane.so.1`; on macOS `libsane.1.dylib`), into a directory of its own per test
  binary (`CARGO_TARGET_TMPDIR/fake-libsane/as-sane/<binary>/`), written under a temporary name holding the
  process's id and renamed into place, so no process ever maps a half-written copy, and a copy a running child
  still maps is replaced, never written over. Each child that loads the fake is spawned with the
  loader's variable (`LD_LIBRARY_PATH`; `DYLD_LIBRARY_PATH` on macOS) naming that directory, ahead of any value
  the parent had. The child calls `Sane::load()`, and `load_fake()` then checks the fake's log exists (its
  `sane_init` writes it), so a fake test that loaded the system's libsane by mistake fails rather than passing on
  it. The probe (Baselines) showed the loader taking such a copy ahead of the installed libsane.
- **Windows:** no default library, so no route. Known when the tests are built, so each test that loads the
  fake (directly or through a helper) is marked at compile time, `#[cfg_attr(not(unix), ignore = "…")]`, its
  reason saying the fake is loaded through the system loader's search for SANE, which Windows has none of;
  libtest prints the reason with each, and nothing of the test runs, its parent's checks after the child
  included. A skip at spawn, as the real-libsane tests skip, won't do: the parent half of a fake test reads the
  fake's log after its child (`fake.log()`, 59 places), and on an empty log its assertions would fail. The step
  may take another mechanism only if it has the same effect (a test gated `#[cfg(unix)]`, with one test per
  binary saying why). No require variable makes it a failure there, since none could pass.
- **Real libsane:** `Sane::load()` only. `SANE_DL_LIBSANE` is retired (decision 5): to run the real tests on
  another libsane, set the loader's variable for the run, which every child inherits. The skips are as they were:
  `NotFound` or `NoDefault` skip, a failure under `SANE_DL_REQUIRE_LIBSANE=1`.
- **What needs a path, inside the crate:** `a_missing_library_is_not_found_with_what_was_tried` and
  `a_library_without_sanes_functions_is_refused` move from `/crates/sane-dl/tests/loading.rs` to the unit tests in
  `library.rs` (renamed as the step chooses; the test list maps each old name to its new one). They call the
  private loader in the test process itself, under one lock so they never race for the slot: a path where nothing
  is (an absolute path in a fresh `tempfile` directory, so it holds nothing and no search is made: cargo sets
  `CARGO_TARGET_TMPDIR`, which the integration test used, only for integration tests and benchmarks, not unit
  tests; `NotFound`, the path in `tried`, and a second try `NotFound` again, so the slot was freed), and the
  system's C library, already loaded into every process (`libc.so.6`; `/usr/lib/libSystem.B.dylib` on macOS;
  `kernel32.dll` on Windows, in place of the integration test's `msvcrt.dll`, which a process needn't have
  loaded), whose initialisers ran as the process started and of which nothing is called
  (`MissingSymbol` for `sane_init`, then again, the slot freed). Their `SAFETY:` comments say so.
- **The slot across copies** (step 3, Linux): integration tests in children: the slot's name bound by the test
  first (the test forms it as the crate does: the name is fixed for good) makes `load()` `AlreadyLoaded`, the
  fake never loaded (no log); bound while a `Sane` lives, the test's bind fails, and after the `Sane` and its
  handles are gone it succeeds; a refused version frees it too.
- **The header contract** takes `/crates/sane-dl/src/ffi.rs` by `#[path]` (above). The fake is unchanged but for
  its docs, which say how tests load it.

### For the seam and later slices

`crates/sane-dl.md`'s "For the seam" is unchanged: nothing the seam takes is touched. What changes for slices 4
and 5 is how a SANE other than the system's is loaded (the loader's variable on the worker's or test child's
`Command`), and how their tests reach the fake (the helper above, copied: no crate may depend on the fake, and
slice 5's backend is Linux only, where the route works). `crates/sane-dl.md` says both with step 4, and that
M5 proposal 6's `SHEAF_LIBSANE` becomes, for sheaf's tests, the loader's variable on the test's child.

### The public API's gate

`just public-api`, a recipe of its own in `ci` (before `msrv`), and a CI job of its own on Ubuntu, both on one
pinned nightly:

- **The toolchain is pinned** (decision 10): a justfile variable, `public_api_nightly := "nightly-2026-10-16"`,
  which the recipe runs as `cargo +{{ public_api_nightly }}`, and CI's job installs the same
  (`dtolnay/rust-toolchain@master` with `toolchain: nightly-2026-10-16`, a comment keeping it in step with the
  variable, as the MSRV jobs' pins are kept). That toolchain is installed here by that name, and is the compiler
  rustup's `nightly` is here (rustc 1.101.0-nightly `4ebd25825`, 2026-10-15). So rustdoc's JSON changes only
  when someone moves the pin, never under an unrelated pull request.
- For each crate in its list (`sane-dl`; a crate joins when it's made ready to publish), the pinned nightly's
  `cargo rustdoc --locked -p <crate> --lib -- -Z unstable-options --output-format json
  --document-hidden-items`, into a directory of its own under the target directory (`public-api/`, under
  `CARGO_TARGET_DIR` where that's set), so nightly's artifacts never churn stable's. Hidden items are included,
  so `#[doc(hidden)]` hides nothing from it.
- Read with `jq` (which the gates already need), over the crate's own items (`crate_id` 0; std's blanket
  `clone_to_uninit`, an unsafe method of another crate, is left out so). It fails, naming each, on: a function or
  method whose header is `unsafe`; a trait declared `unsafe` (which catches an `unsafe impl` of one too); an item
  holding an `unsafe` function pointer anywhere in it (a type alias, a field, a signature); a public function or
  method with a raw pointer in its signature; and a public re-export of another crate's item (a `use` whose
  target isn't the crate's own), which the gate can't read. **The crate's own type aliases are followed:**
  rustdoc's JSON keeps an alias as a path (`SANE_Device`'s `name` reads `resolved_path` `SANE_String_Const` in
  the baseline's JSON), and `ffi` has public aliases of raw pointers (`SANE_Handle`, `SANE_String`,
  `SANE_String_Const`), so a signature naming an alias counts what the alias holds, transitively; otherwise a
  public `fn` returning `ffi::SANE_Handle` would pass unseen. Raw pointers in public struct fields are allowed
  (`ffi`'s plain data, as `twain-dsm`'s are). **An `unsafe impl`** (`Send`, `Sync`, or another crate's unsafe
  trait) is listed, not failed: it's the crate's own promise, never one its caller keeps, and `twain-dsm`'s
  public `Dib` has two (`unsafe impl Send` and `Sync`), which TEST-12 would otherwise fail on.
- **It checks the JSON's shape first:** its `format_version` is the one the recipe was written against (61), every
  function item holds a boolean `header.is_unsafe`, and the crate's own items number more than a floor; else it
  fails saying rustdoc's JSON changed shape, so a moved pin, or a field renamed, fails the gate rather than
  passing it unread, until the `jq` is read again against the new format.
- It writes the list of every public item it read, by path and kind, methods under their types, the `unsafe impl`s
  marked, to `public-api/<crate>.txt`, and says how many it read, how many are functions, and that none is
  unsafe. The list isn't committed as a snapshot to diff: it's the host's view only, it would churn with every
  addition to the alpha's API, and the gate holds the rule the slice asks of it, a list kept as a gate. Each
  checking agent copies it to `scratch/work/s0-1a/step-<n>/` to compare with the step before's.
- On the host's target only: the pinned nightly here has only `x86_64-unknown-linux-gnu`, and so does CI's job.
  Of `sane-dl`'s items only constants differ by system (`DEFAULT_LIBRARIES`), and `select_fd` is Unix's.
  `twain-dsm` isn't in its list: its items are Windows's, which a Linux run can't see, and slice 8 read them by
  hand; its run on Windows is TEST-12 (new, below).
- Not `cargo public-api`: a maintained tool over the same JSON, but one more thing to install on every CI run,
  for facts `jq` reads directly (decision 10).
- CI's job goes after `sane-dl-package`, the workflow's last job today, away from where slice 3 adds its
  `scan-device-msrv` (after `twain-dsm-msrv`), so the two merges don't meet.

### Dependencies, vetted

None added; `cargo tree -p sane-dl` is unchanged by every step. The slot uses std alone; the gate, the pinned
nightly's rustdoc and `jq`. The MSRV stays 1.89 (`decisions.md`): the std APIs added are stable since 1.70, and
`just sane-dl-msrv` builds and tests the crate on 1.89 at every step.

## Steps

**At every step**, the checking agent runs the core, once: `just ci` with `SANE_DL_REQUIRE_LIBSANE=1` and
`SANE_DL_REQUIRE_HEADER=1` (fmt, clippy `-D warnings`, test, doc `-D warnings`, gates, from step 2 `public-api`,
`sane-dl-package`, and the MSRV recipes, each on its installed toolchain); `cargo test --locked --workspace --
--list` recorded under `scratch/work/s0-1a/` and compared with the step before's, step 1's with the base's
(`3f33be5`, recorded by step 1 before it changes anything), every test kept or its move mapped; the commit's diff
and message scanned for local paths, the username, the maintainer's name, the downstream app's name outside
`docs/pm/`, and pdf-lint's unpublished plan; every `SAFETY:` comment the step adds or changes read against its
call. Below, each "Done when" says only what the step adds; a check whose subject a step doesn't touch is left
out, with why. The control file's `skip` is read before each long check.

1. **The milestone's slice 1a; the tests load the fake through `Sane::load()`.** Slice 1a added to
   `milestones/s0-devices.md` after slice 1, in its style: what it builds (the list in "What it builds" above),
   what it's checked by (`just ci`, the soundness review, the gate over the public API, the publish dry run), and
   the plan's path. The tests as "The tests" above has them: the fake copied under SANE's leaf name per test
   binary and the loader's variable on each child, `load_fake()` through `Sane::load()` with its log checked,
   every fake test ignored off Unix at compile time with its reason, `load_real()` through `Sane::load()`,
   `SANE_DL_LIBSANE` and `common::load_from` gone (and `tests/real.rs`'s module doc, which names the variable),
   the two path tests moved into `library.rs`'s unit tests over the private loader (made an `unsafe fn` with its
   `# Safety`, `load_from`'s promise its third case until step 2). `fake-libsane`'s crate docs and manifest
   comment, and the gate's comment in `/justfile`, say tests load it as SANE through the loader's search path.
   `load_from` itself stays until step 2, unused by any test.
   **Done when:** the core; the test list with the two moved tests mapped old name to new and none lost; `rg
   'load_from|SANE_DL_LIBSANE' /crates` finding only `Sane::load_from` and its docs in `/crates/sane-dl/src/`; the
   fake's guard made to fail once, on a worktree under `scratch/work/s0-1a/step-1/` (the loader's variable left
   off a fake child: the test fails at `load_fake`'s check, never passing against the system's libsane); the
   Windows marking made to show it misses no fake test, on the same worktree (the attribute's condition flipped to
   `unix` and `fake_library()` made to panic: the test run has no failure, so no test that loads the fake was
   left unmarked, and its ignored count is recorded); the worktree removed and the branch checked
   unchanged; `cargo clippy --locked -p sane-dl --all-targets --target <t> -- -D warnings` for
   `x86_64-pc-windows-gnu` and `x86_64-apple-darwin` (the tests' Windows marking and macOS variable are behind
   `cfg`); after a run, the per-binary directories under `CARGO_TARGET_TMPDIR/fake-libsane/as-sane/` listed, each
   holding SANE's leaf name alone, no temporary file left (that the variable is what loads the fake, the guard's
   make-fail shows; no running child is watched). The header contract, the docs' other targets and the MSRV
   probe are left out: nothing they read changes (the MSRV build and tests run in the core).
2. **`load_from` gone, `ffi`'s function types inside the crate, the public API's gate.** `Sane::load_from`
   removed; the loader's `SAFETY:` comments rewritten (no caller's promise left in them); the crate docs, README
   ("Loading, and what can fail"), `LoadError`'s docs and the changelog say how another SANE is loaded (the
   loader's variable at the process's start, naming a directory of SANE's own libraries alone, or a run path built
   in) and why `load` is safe. `ffi`'s 14 function types and `SANE_Auth_Callback` `pub(crate)`; its notice's list
   of changes and module doc updated; the header contract on `ffi.rs` by `#[path]` (with `#[allow(dead_code)]`, as
   the fake takes it). `just public-api` (above), in `ci`, its pinned nightly in a justfile variable; its CI job
   after `sane-dl-package` (`dtolnay/rust-toolchain@master` with the pinned `toolchain`, read-only, credentials
   not persisted, as the others); and the justfile's header saying the recipe needs that nightly and `jq`.
   **Done when:** the core (its `public-api` the gate's first green run: no unsafe item); the gate's list read
   whole by the checker, every item, and the docs of each public item read for a rule asked of the caller, each
   either bound by a type or advice whose breach can't be undefined behaviour, and what `mem::forget` of each
   owning type (`Sane`, `Handle`, `Batch`, `CancelToken`) leaves behind (a leak at worst), said in "So far" (the
   table in "What a caller keeps today", checked against the code as it now is); the gate's make-fails on a
   worktree under `scratch/work/s0-1a/step-2/` (where the gate is new, once, as slice 1's decision 23 placed its
   gates'), planted together and run once, since the gate names each: `load_from` put back; a `#[doc(hidden)] pub
   unsafe fn`; a `pub unsafe trait` with an `unsafe impl` of it for a public type; a `pub unsafe fn` method on a
   public type; a public type alias of an `unsafe extern "C" fn`; a public function taking `*const u8`; a public
   function returning `ffi::SANE_Handle` (a raw pointer behind an alias); a public re-export of another crate's
   item; and beside them an `unsafe impl Send` for a public type, which the output lists and doesn't name as a
   failure; then, alone, the recipe's expected `format_version` changed, so the shape check fails; then the
   worktree removed and the branch checked unchanged; the header contract run here against the installed 1.4.0
   header and passing, and failing once on a function type changed in `ffi.rs` on a worktree copy (it still
   checks the function types it now takes by `#[path]`); `cargo clippy --all-targets` and `cargo doc` with `-D
   warnings` for `x86_64-pc-windows-gnu` and `x86_64-apple-darwin` (the docs mention `cfg`'d items);
   `fake-libsane` building against the changed `ffi.rs` (in `just ci`'s nested builds); the README's example as a
   doctest (in the core). The cancel tests under ThreadSanitizer are left out: no synchronization changes.
3. **One SANE per process, whatever copies of the crate it holds (Linux).** The process-wide slot as "The
   process's slot" above, a module of its own with no `unsafe`, taken before the library opens and freed after
   `sane_exit` and after every failed load; `AlreadyLoaded`'s docs; the new `LoadError` variant; the README's and
   crate docs' "one copy" paragraph (cargo within a build, the slot on Linux across builds, the program's rule on
   macOS and the BSDs); the changelog. Its integration tests (Linux, above).
   **Done when:** the core; a probe under `scratch/work/s0-1a/step-3/two-copies/`, not committed: two `cdylib`s,
   each taking `sane-dl` by path and exporting a function that loads SANE and holds it, and a host that opens both
   and calls each, the fake found by the loader's variable: at the step's parent commit both load and the fake's
   log shows its second `sane_init` (a violation of its own), after the step the second is `AlreadyLoaded` and the
   log has none; the slot's module undone on a worktree under `scratch/work/s0-1a/step-3/` (the per-copy atomic
   alone), its tests failing, then removed; `cargo clippy --all-targets` and `cargo doc` for
   `x86_64-pc-windows-gnu` and `x86_64-apple-darwin` (the slot is `cfg(target_os = "linux")`); `git diff` of the
   step showing no `unsafe` added; the gate's list compared with step 2's: the new variant (with its field) the
   only new items.
4. **BUG-7; the plan's files; the slice's end.** `Image`'s `Debug` by hand, with unit tests (an image of 1 MiB,
   whose derived form would be megabytes, its `Debug` and `{:#?}` under 1 KiB, holding its length and none of its
   bytes; a `Page::Image`'s the same; each assertion's message giving the text's length and its first 200
   characters, never the whole text, so the make-fail below doesn't print megabytes); the changelog's line. The
   plan's files: `crates/sane-dl.md` as built (loading and the loader's variable, the slot, `ffi`, the tests'
   route, M5 proposal 6's variable as the loader's, BUG-7 gone from "Open"); `crates/fake-libsane.md`,
   `crates/README.md`'s line and `crates/scan-fixtures.md`'s sentence on the fake (loaded as SANE through the
   loader's search path; ignored on Windows); `plan.md` (the fake's row and its line under the graph);
   `milestones/s0-devices.md`'s slice 1a (done on the branch, what waits on review), and in slice 1's entry one
   clause after "`Sane::load_from` an `unsafe fn`" saying slice 1a removed it; `decisions.md` (the ruling's line
   gains the 2026-11-09 words and that `sane-dl` follows it: the loader chooses, the slot on Linux);
   `status.md`; `slices/README.md`; the backlog: BUG-7 to `archive/backlog-done.md`, the new items (below), TEST-7
   revised (macOS's fake tests through `DYLD_LIBRARY_PATH` beside Homebrew's SANE), the counts redone or left
   out. Only what this slice needs, in the files slice 3 also touches. The new items take the next IDs as
   `origin/master` has them when the step starts (a `git fetch`, which moves no local branch and pushes
   nothing): slice 3, beside this one from the same base, plans to take the same next IDs (BUG-8, FEAT-13,
   TEST-12), so whichever slice merges second renumbers its own, as slice 8's TEST-7 became TEST-11; crate code
   and crate docs cite no backlog ID, so a renumbering touches `docs/pm/` alone.
   **Done when:** the core; `just sane-dl-package` with `CARGO_TARGET_DIR` at a fresh `scratch/work/s0-1a/
   package/`, a check, never a publish, its file list read (17 files: slice 1's 16 and step 3's slot module); the
   derived `Debug` put back on a worktree, the new tests failing, then removed; the gate's list unchanged from
   step 3's (the derived impl and the written one are the same item); every commit message of the branch
   (`3f33be5..HEAD`) read for the names and paths the rules forbid, and `git grep -i` for the maintainer's name
   outside the root `LICENSE-MIT` finding no new line; the backlog's IDs and counts checked with `rg`, and none
   of the new IDs found on `origin/master` as fetched (`git grep`). The gates' and the gate's make-fails aren't
   repeated (no gate changes after step 2). What only CI shows, said (it waits on the push): macOS's fake tests
   through `DYLD_LIBRARY_PATH` (Risk 1); Windows's fake tests ignored with their reason, none failing; the
   `public-api` job on CI's pinned nightly; the slot's tests on CI's Ubuntu; the MSRV job's real-libsane tests
   skipping; the package job.

**After step 4,** the workflow's code review is the milestone's "soundness review": among its lenses one for
soundness, reading every `unsafe` block and `SAFETY:` comment of the crate, the private loader's `# Safety` and
its two call sites, the gate's list of public items with each one's docs, and the slot's freeing on every path.

**Backlog items the slice adds** (step 4, with the next IDs as `origin/master` then has them; the IDs below are
as `master` has them at `3f33be5`, and slice 3 may take them first; each read against the code first):

- **BUG-8** · sane-dl · later: on macOS and the BSDs, two copies of the crate built apart and loaded into one
  process (two shared libraries, each holding one) each keep their own slot, and so could start SANE twice; Linux
  refuses it (step 3). No kernel-freed, per-process name std offers there: a lock file can be removed under its
  holder (a temp cleaner), a named semaphore outlives a crash, and `dlopen(RTLD_NOLOAD)` races two loads at once.
  The rule is said in the docs meanwhile (decision 9).
- **FEAT-13** · sane-dl · later: a SANE an app ships beside it, on macOS, for a process dyld restricts (the
  hardened runtime every notarized app has, where dyld ignores `DYLD_LIBRARY_PATH`): a path fixed relative to
  the executable, chosen by the crate (`@executable_path/../Frameworks/libsane.1.dylib`), trusted as the program
  is. Only if such an app comes.
- **TEST-12** · twain-dsm · S0: `just public-api` over `twain-dsm` on Windows, on the gate's pinned nightly, in a
  CI job there, where its items compile (its public `Dib`'s `unsafe impl Send` and `Sync` listed, not failed);
  for the slice that next touches `twain-dsm`'s CI.

## Baselines

- **The public API at `3f33be5`** (`scratch/work/s0-1a/probe-public-api/`, the planner's probe): rustdoc's JSON on
  nightly 1.101.0 (`4ebd25825`, 2026-10-15; the toolchain `nightly-2026-10-16`, installed here by that name too),
  `format_version` 61. 1,523 items of the crate's own; 243 functions, one unsafe (`Sane::load_from`); 15 type
  aliases holding an unsafe function pointer (the 14 `Sane*Fn` types and `SANE_Auth_Callback`, all in `ffi`); no
  unsafe trait, no written unsafe impl among the crate's items; no raw pointer written in a public function's
  signature (aliases unfollowed; the plan review read the same JSON: `ffi`'s aliases of raw pointers appear as
  paths, and no public function names one); `select_fd` returning `RawFd`. The one other `is_unsafe: true` in
  the file is core's `clone_to_uninit`, another crate's. Step 2's gate is held to this: those 16 gone, nothing
  new.
- **The loader picking the fake** (`scratch/work/s0-1a/probe-loader-path/`, the planner's probe): the fake, built
  here, copied as `libsane.so.1` into a directory of its own (no `SONAME`, no run path in it, `readelf -d`); a
  process opening `libsane.so.1` maps the system's libsane without `LD_LIBRARY_PATH`, and the copy with it.
- **dyld's search** (Apple's dlopen(3), read 2026-11-09): for a path with a slash, `DYLD_LIBRARY_PATH` (as set at
  launch) under the path's leaf name first, then the path; every variable ignored in a process dyld restricts
  (set-id, or the hardened runtime without the entitlement that allows them). Untried here (no Mac): CI's macOS
  job is the proof (Risk 1).
- **The slot's socket on 1.89** (`scratch/work/s0-1a/plan-review/abstract.rs`, the plan review's probe):
  `SocketAddr::from_abstract_name` and `UnixDatagram::bind_addr` build on 1.89; the name formed from
  `/proc/self/ns/pid`'s inode and the pid; a second bind `AddrInUse`, a bind after the first is dropped free.
- **libloading 0.9's contract** (docs.rs): `Library::new` is unsafe because a library's initialisers run as it
  opens and its finalisers as it closes; `Library::get` because the caller names the symbol's type.
- **The test list at `3f33be5`**, recorded by step 1 before its change (`scratch/work/s0-1a/`); slice 1's 221
  `sane-dl` and `fake-libsane` tests among them, beside `twain-dsm`'s on Linux.
- **The two-copies probe at the step's parent** (step 3): the fake's log showing a second `sane_init`.
- **SANE as installed here:** `libsane1`, `libsane-dev` and `sane-utils` 1.4.0-1ubuntu1, the `test` and `pnm`
  backends, `/usr/include/sane/sane.h`, as slice 1 had them.
- **sheaf:** no comparison; nothing is cut from it. Its cited plan files unchanged since `3232a35`.

## Decisions taken alone, for review

1. **The library is the loader's choice alone; `Sane::load_from` is removed** (shape 1 of "Loading: the shapes
   weighed"), rather than kept `unsafe` as an exception, made safe over a checked path, carried by a trust type,
   chosen by a variable of the crate's own, or turned on by a feature or `--cfg`. Another SANE is loaded by the
   loader's own search path, set as the process starts, or a run path built in.
2. **`Sane::load()` stays safe,** on a build-time link's argument: the loader's search, by SANE's soname (on macOS
   by the package managers' paths), in the environment the process started with, the library never unloaded.
   ash's and openxr's soname loaders are `unsafe`, partly for the unload they do on drop.
3. **The fake's tests are ignored on Windows,** saying why: no default library, so no route, and no Windows user
   can reach that code. They ran there by path until now (TEST-8's run). Marked at compile time
   (`#[cfg_attr(not(unix), ignore = "…")]`), since a skip at spawn would leave each test's checks after its child
   to fail on an empty log (the plan review).
4. **What needs a path is tested inside the crate:** `NotFound` and `MissingSymbol` as unit tests over the private
   loader, in the test process, with a path where nothing is and the system's C library, already loaded.
5. **`SANE_DL_LIBSANE` is retired;** the real tests take another libsane through the loader's variable.
6. **`ffi` stays public, types and constants only; its function types and `SANE_Auth_Callback` are
   `pub(crate)`,** as `twain-dsm`'s are, and the header contract takes `ffi.rs` by `#[path]`. Making `ffi` private
   was weighed (less API to keep stable): kept public for SANE's raw names (slice 1's decision 20) and the family's
   one practice.
7. **The fake is found as SANE by a copy under SANE's leaf name,** per test binary, written then renamed; its log
   checked after every load of it, so a fake test can't pass on the system's libsane.
8. **`select_fd` keeps returning `RawFd`:** a `BorrowedFd` tied to the handle would be unsound, since a token's
   cancel on another thread may end the descriptor while it's borrowed; a `RawFd`'s every use is the caller's own
   `unsafe`.
9. **The slot across copies on Linux only:** an abstract socket named for the process (pid namespace and pid),
   std alone, freed by the kernel. On macOS and the BSDs the program's rule stays, said in the docs (BUG-8),
   since no exact shape was found there in std; on Linux it stays only across what the slot can't see, a change
   of the process's network namespace or of its view of `/proc` between two copies' loads (security item 7),
   said in `Sane`'s docs, the README and `sane_init`'s `SAFETY:` comment. Those two are the exceptions the slice
   leaves (revised in step 4, from step 3's check).
10. **The gate:** rustdoc's JSON on a pinned nightly, `nightly-2026-10-16` (revised by the plan review from
    unpinned: a format change would otherwise turn an unrelated pull request red; `twain-dsm-doc-codes` stays on
    unpinned nightly, whose error codes rarely move), one justfile variable and CI's pin kept in step; its
    `format_version` and shape checked, so a moved pin or a renamed field fails it until the `jq` is read again;
    hidden items included; `jq`, not `cargo public-api`; a recipe and CI job of their own rather than in `gates`
    (a second toolchain in the gates job would change its default); `sane-dl` alone in its list (TEST-12 for
    `twain-dsm`). Raw pointers allowed in public fields, not in public signatures, the crate's own aliases
    followed.
11. **The version stays 0.1.0-alpha.1,** its unreleased changelog entry rewritten, with one line on what changed
    before the release.
12. **BUG-7 by `Image`'s `Debug` alone** (`Page` derives, and prints it); `Value`'s arrays left as they are.
13. **No public helper for the loader's path** (a `ConfigDir` for the library): the tests' helper does it, slice 5
    copies it; an app sets one variable.
14. **The slot's name is fixed for good,** across versions, and the tests form it as the crate does.
15. **The slot refuses rather than loads unguarded** when the system won't give the socket (a new `LoadError`
    variant): a sandbox that forbids Unix sockets gets no SANE, said.
16. **The gate's make-fails run once, in step 2,** where it's new; the existing gates' aren't repeated (unchanged).
    Planted together and run once, since the gate names each; the shape check's alone (the plan review).
17. **An `unsafe impl` is listed by the gate, not failed** (the plan review): it's the crate's promise, never its
    caller's (`unsafe impl Send` for a type whose raw parts the crate guards), and `twain-dsm`'s public `Dib` has
    two; an unsafe trait the crate declares fails, which covers an `unsafe impl` of one. A public re-export of
    another crate's item fails, since the gate can't read it.
18. **Backlog IDs as `origin/master` has them when step 4 starts** (the plan review): slice 3, beside, plans to
    take the same next ones; the second to merge renumbers its own. Crate code and docs cite none.
19. **A fake child's `SANE_CONFIG_DIR` is its test's own directory** (step 1), which holds no `dll.conf`: a child
    that reached the system's libsane by mistake, as the guard's make-fail makes one, reads none of the system's
    configuration before `load_fake` fails it, as the real tests never read `/etc/sane.d`.
20. **`Api::look_up` is a private `unsafe fn` too** (step 1), its contract that the library's functions by SANE's
    names have `sane.h`'s types, as `Api::load`'s contract makes them; its `SAFETY:` comment had leaned on
    `load_from`'s caller.
21. **The `MissingSymbol` unit test exists only where the system library it opens is known** (step 1): Linux with
    glibc (`libc.so.6`; musl's isn't that), macOS and Windows, by `cfg` rather than a skip at run time.
22. **The gate's `jq` is a file of its own, `/scripts/public-api.jq`** (step 2), which the recipe runs with `-f`,
    rather than inline in `/justfile`: about 280 lines, its comment saying what it reads and why, kept clear of
    just's `{{ }}` and of bash's quoting.
23. **An `unsafe impl` is known by its trait** (step 2): rustdoc's JSON on the pinned nightly gives `is_unsafe:
    false` for every impl, written `unsafe impl`s included (a probe, `scratch/work/s0-1a/step-2/probe-json/`), so
    the gate takes an impl as unsafe when its trait is the crate's own declared `unsafe` or one of std's stable
    unsafe traits by path (`Send`, `Sync`, `GlobalAlloc`), and lists an impl of any other crate's trait as "its
    safety unread". The flag is read as well, should a later pin set it. It bears on the list only: no impl fails.
24. **The gate's list** (step 2): every public item by the path a user writes (re-exports followed, globs too),
    then under each type its fields, variants, inherent items and trait impls; the five stable auto traits on
    one line (`auto Send, !Sync, …`), the compiler's others (`Freeze`, `UnsafeUnpin`) and blanket impls left out
    (counted); `unsafe` marked on a function, method or trait. Its header names the format, the toolchain and the
    target.
25. **A fake child's loader variable holds no empty entry** (step 2, from step 1's check): `split_paths` of an
    unset variable gives one empty entry, which the loader reads as the working directory, so it's left out; the
    child's search is then the fake's directory and what the parent was given, as security item 1 advises.
26. **The slot's name holds the pid namespace's device and inode** (step 3), which together identify a
    namespace (namespaces(7)); the plan had the inode alone. The name, fixed for good:
    `sane-dl/slot/pidns/<device>/<inode>/pid/<pid>`, or `sane-dl/slot/pidns/none/pid/<pid>` where
    `/proc/self/ns/pid` can't be read (as planned: two processes with one pid sharing a network namespace then
    collide, a refusal). The nsfs device is 5 here, so the device isn't a constant to leave out.
27. **The new variant is `LoadError::SlotRefused(io::Error)`** (step 3), the system's error its source. It exists
    on every system, so the enum is the same everywhere, and is returned on Linux alone. A name
    `from_abstract_name` refused would be the same variant; none can be (the longest is 75 bytes of the 107
    allowed, a unit test).
28. **Two guards in one `Slot`** (step 3): this copy's atomic (`CopySlot`) taken first, then the process's
    socket, freed in the reverse order by the fields' drop order. The atomic stays: two threads of one copy
    never race for the socket, and on macOS and the BSDs it's the only guard.
29. **The system's refusal tested by exhausting a child's descriptors** (step 3), a fourth integration test
    beyond the plan's three, since no test can make a sandbox: the child lowers its own `RLIMIT_NOFILE` to 256
    with util-linux's `prlimit --pid` (std has no `setrlimit`, and the tests take no `unsafe` or dependency for
    it), opens `/dev/null` until `EMFILE`, and loads: `SlotRefused` carrying `EMFILE`, nothing opened, then the
    fake loads once descriptors are free. Skipped, saying why, where `prlimit` isn't there or the limit
    doesn't take.
30. **The docs no longer say `links` keeps a program to one copy** (step 3): it refuses two versions of the crate
    in one build, but each shared library built with the crate holds a copy of its own, even two from one build
    (the probe built its two apart). The crate docs, the README, the changelog and the manifest's comment say
    so, then the slot on Linux, and the program's rule elsewhere and across what the slot can't see (security
    item 7).
31. **`Image`'s `Debug` through `debug_struct`** (step 4), the data given by a private helper that writes
    `<n> bytes`, so `{:#?}` keeps the struct's layout and the field's name stays `data`; `frames` given whole,
    since an image from a batch holds at most three (the assembly's rule). Its doc names the form; the
    changelog gives an example (`data: 1048576 bytes`).
32. **The prlimit test's skips skip** (step 4, from step 3's check, as step 2 took step 1's): `Child::run`
    returns whether the child ran its body, false where it skipped, so the parent of
    `a_socket_the_system_refuses_refuses_the_load` leaves out its checks of the fake's log then; and "the limit
    doesn't take" is a skip, not an `assert!`, as decision 29 had it. Every other caller ignores the value: no
    other test has checks of its own after a child that can skip.

## For review (security)

1. **Which library loads** (supersedes slice 1's items 1 and 9): only what the loader finds under SANE's name (on
   macOS at the package managers' paths), in the environment the process started with; no code chooses it.
   Whoever sets that environment (`LD_LIBRARY_PATH`, `DYLD_LIBRARY_PATH`) chooses every library the process
   loads by name, as with `LD_PRELOAD`; the loader ignores those variables in a set-id process and in a macOS
   process dyld restricts (the hardened runtime). Run paths are the builder's. The docs advise a variable naming
   a directory of SANE's own libraries alone, since it reaches every library the process loads by name, as the
   tests' per-binary directories hold SANE's leaf name alone.
2. **A library under SANE's name is taken as SANE** (the soname names SANE 1's ABI): its functions called with
   `sane.h`'s types, as a build-time link would. The fake is SANE in that sense: its exports are asserted at
   compile time to have the bindings' types, which the header contract checks against `sane.h`.
3. **Two copies of the crate in one process** (supersedes slice 1's item 10): cargo refuses them within a build
   (`links`); on Linux the process-wide slot refuses a second copy built apart; on macOS and the BSDs that's the
   program's rule (BUG-8).
4. **The slot's name can be taken first** by any local user in the same network namespace, who can work out a
   process's pid and pid namespace: the process then gets `AlreadyLoaded`, a denial of SANE, never two copies
   in. Two processes with one pid in pid namespaces sharing a network namespace would collide only if the
   namespace's inode can't be read, the same refusal. A child forked without an exec holds its parent's name
   until it exits or execs (the socket is close-on-exec), so the parent's next load meanwhile is refused, the
   same refusal.
5. **The unit tests open the system's C library in their own process:** already loaded there, so no initialiser
   runs, and nothing of it is called.
6. **The gate reads the host's view only:** items behind another system's `cfg` aren't in its JSON (in `sane-dl`,
   constants); `twain-dsm`'s Windows items wait on TEST-12.
7. **What the slot can't see** (step 3): the name lives in the process's network namespace and is formed from
   what `/proc` shows it, so a process that moves itself to another network namespace (`unshare`, `setns`)
   between two copies' loads, or loses `/proc` (a `chroot`, a mount namespace) so its second copy forms the
   `none` name, would let SANE start twice. Each takes privilege, or a user namespace, and a deliberate step
   between the loads, and std's `chroot` is safe: so the program's rule stays for that case, said in
   `Sane`'s docs and the README. Refusing a load where `/proc` can't be read would close the second half and
   deny SANE to a process without `/proc`; the first half has no answer here.
8. **The slot's name is visible within the network namespace** (step 3): any process there can list abstract
   names (`ss -xl`, `/proc/net/unix`), and so see which pids have SANE loaded; the name says nothing more.
   Nothing is read from the socket: datagrams sent to it queue unread within the kernel's per-socket bound
   until it closes.
9. **A sandbox that mediates socket names** (step 3): AppArmor can deny binding an abstract name, and a strictly
   confined snap's profile may deny this one. The load is then `SlotRefused` (the error isn't `AddrInUse`),
   never SANE unguarded, until the profile allows `@sane-dl/slot/**`. Untried here.

## Risks

1. **macOS:** the fake's tests there now rest on dyld searching `DYLD_LIBRARY_PATH` under the leaf of an absolute
   path, as dlopen(3) says, in a test binary rusty-fork spawns directly (ad-hoc signed, no entitlements, so the
   variable is kept). Seen only on CI's runner, after the push. If dyld ignores it, those tests fail there; the
   answer then is the Windows one, ignored saying why, with TEST-7 carrying the route.
2. **Coverage on Windows drops** to building, the unit tests and `NoDefault`: by design (decision 3), but a
   change in what CI shows.
3. **The pinned nightly ages:** moving it is a deliberate change (the variable, CI's pin, the `format_version`
   and the `jq` read again), never a surprise. A machine without `nightly-2026-10-16` can't run `just ci` whole,
   as one without an MSRV toolchain can't.
4. **Shared files with slice 3,** running beside: `/justfile` (a new recipe and variable, the `ci` line, the
   header's needs, a comment in `gates`), `/.github/workflows/ci.yml` (a new job, at the end, away from slice
   3's), `status.md`, `decisions.md`, `backlog/*` (the same next IDs: decision 18), `plan.md`, `crates/README.md`,
   `slices/README.md`, the milestone. Edits kept to what this slice needs, in blocks of their own, so a merge
   meets few conflicts, each small.
5. **The per-binary copies of the fake** add a copy of a debug shared object per test binary under
   `CARGO_TARGET_TMPDIR`; each written under a name with its process's id and renamed into place, so no process
   (test binaries run at once, nextest's one process a test) maps half a file or has its mapped copy written
   over.
6. **The abstract socket** may be refused in a sandbox that forbids `socket(AF_UNIX)`: SANE then doesn't load
   there (decision 15), said in the docs.
7. **The two-copies probe** builds two `cdylib`s against the workspace's `sane-dl` by path, outside the workspace
   (its own target directory under `scratch/work/s0-1a/step-3/`): cargo's `links` check doesn't span separate
   builds, which is the case it shows.

## Plan review

2026-11-09, adversarial, through three lenses (fidelity to slice 1a and the specs; mechanics, each step green
alone on three systems; behaviour and proof, each "Done when" checkable here), each finding checked against the
code, the sources or a probe before it was acted on. The plan as the planner left it is kept locally,
`scratch/work/s0-1a/plan.before-review.md`; the review's probe and notes are in `scratch/work/s0-1a/plan-review/`.

**Accepted** (lens, severity: what changed):

1. Mechanics, high: **the Windows skip at spawn would fail the fake tests.** The parent half of a fake test reads
   the fake's log after its child (`fake.log()` in 59 places, `/crates/sane-dl/tests/fake.rs` among them), so a
   test skipped before its child would assert on an empty log. Now each fake test is ignored off Unix at compile
   time, with its reason, and step 1 shows on a worktree that the marking misses none ("The tests", decision 3,
   step 1).
2. Fidelity, medium: **backlog IDs taken twice.** Slice 3, beside this one from the same base, plans to take
   TEST-12, FEAT-13 and BUG-8 too (its plan, `slices/s0-3-scan-device.md` on its branch: step 4, decision 22).
   Step 4 takes the next IDs from `origin/master` as it starts, the second slice to merge renumbers its own, and
   crate docs cite none (step 4, decision 18).
3. Behaviour and proof, medium: **raw pointers behind aliases passed the gate.** rustdoc's JSON keeps an alias as a
   path (`SANE_Device`'s `name` is `resolved_path` `SANE_String_Const` in the baseline's JSON), and `ffi`'s
   `SANE_Handle`, `SANE_String` and `SANE_String_Const` are public aliases of raw pointers, so a public `fn`
   returning `ffi::SANE_Handle` would have passed. The gate follows the crate's own aliases; a make-fail plants
   that case (the gate, step 2). No public function names one today (checked on the same JSON).
4. Fidelity, medium: **failing every `unsafe impl` would fail `twain-dsm`.** Its public `Dib` has `unsafe impl
   Send` and `Sync` (`/crates/twain-dsm/src/dsm.rs`), the crate's own promise and never a caller's, so TEST-12
   couldn't pass. `unsafe impl`s are listed, not failed; a declared unsafe trait fails, which covers an `unsafe
   impl` of one (the gate, step 2's make-fails, decision 17, TEST-12).
5. Mechanics, medium: **unpinned nightly.** A change to rustdoc's JSON would turn an unrelated pull request red.
   `nightly-2026-10-16` is installed here by that name, the same compiler as `nightly` (`4ebd25825`); the recipe
   and CI's job pin it, and the gate checks `format_version` 61 (the gate, step 2, decision 10, Risk 3).
6. Mechanics, medium: **`CARGO_TARGET_TMPDIR` isn't set for unit tests** (cargo sets it for integration tests and
   benchmarks only), so the `NotFound` test couldn't move as written: its path is now in a fresh `tempfile`
   directory, absolute, absent by construction ("The tests").
7. Mechanics, medium: **the package holds 17 files after step 3, not 16:** `src/` holds 9 sources today, and
   the slot is a module of its own (step 4).
8. Behaviour and proof, low: **"a fake test's child seen with the loader's variable"** needs a running process
   watched, which the run's rules forbid; the guard's make-fail already shows the variable is what loads the
   fake. Step 1 lists the per-binary directories after a run instead.
9. Behaviour and proof, low: **BUG-7 adds no item to the public list:** `Image` derives `Debug` today, so its
   `impl Debug` is listed already; step 4 expects the list unchanged.
10. Behaviour and proof, low: **step 3's new variant brings its field** as a second item in the list; said.
11. Behaviour and proof, low: **BUG-7's test at 64 MiB** would print hundreds of MB in its assertion message under
    step 4's make-fail: 1 MiB is enough (its derived form is megabytes), the messages giving a length and a
    prefix.
12. Mechanics, low: **the gate's make-fails batched,** planted together and run once, since the gate names each;
    the shape check's alone (step 2, decision 16).
13. Fidelity, low: **shape 2 against `twain-dsm`'s `Load::Path`,** which is safe because the DSM's Authenticode
    signer is checked in the file first; a libsane file has no signature to check. Said in shape 2.
14. Fidelity, low: **the milestone's "soundness review"** wasn't placed: it's the workflow's code review after
    step 4, its soundness lens, and what it reads is said (Steps).
15. Fidelity, low: **two files said "loaded by its path"** and weren't in step 4: `crates/scan-fixtures.md`, and
    the milestone's slice 1 entry, which lists `load_from` as an `unsafe fn` for review (step 4).
16. Fidelity, low: **the loaders' search, said exactly:** on Linux only `DT_RPATH` comes before
    `LD_LIBRARY_PATH`, `DT_RUNPATH` after it (glibc's dlopen(3)); on macOS dyld drops the variables in a process
    it restricts (set-id, or the hardened runtime), not in any process "signed with entitlements" (the shapes,
    the API, Baselines, security item 1, FEAT-13).
17. Behaviour and proof, low: **the slot and `fork`:** a child forked without an exec holds its parent's name, so
    the parent's next load is refused meanwhile; said. The socket itself probed on 1.89 (the API, Baselines,
    security item 4).
18. Mechanics, low: **the per-binary copy's temporary name** holds the process's id, and the rename is there so a
    copy a running child maps is replaced, never written over (each binary has its own directory already)
    ("The tests", Risk 5).
19. Mechanics, low: **step 1's private loader** is still called by `load_from` until step 2, so its `# Safety`
    names that promise until then (the API, step 1).
20. Behaviour and proof, low: **`mem::forget`** of each owning type read in step 2's pass over the public items.
21. Behaviour and proof, low: **the loader's variable reaches every library loaded by name,** so the docs advise
    a directory of SANE's libraries alone, as the tests' directories are (the API, step 2, security item 1).
22. Fidelity, low: **`tests/real.rs`'s module doc names `SANE_DL_LIBSANE`;** step 1's `rg` covers the variable.
23. Mechanics, low: **CI's new job after `sane-dl-package`,** away from where slice 3 adds its job (the gate,
    step 2, Risk 4).
24. Behaviour and proof, low: **a re-export of another crate's item** is a public item the gate can't read, so it
    fails (the gate, decision 17).
25. Fidelity, low: **"the list kept as a test or a gate":** why the list isn't committed as a snapshot, said (the
    gate).

**Rejected:**

1. Fidelity: *step 3 is more than slice 1a.* The one-copy rule is `Sane::load`'s caller's contract (its docs: "A
   program holds one copy of this crate"), which safe code breaks with two shared libraries each holding a copy;
   1a takes every contract a caller keeps, and slice 8 bound the same rule for `twain-dsm` with a semaphore named
   for the process.
2. Mechanics: *keep the fake's tests on Windows through a hidden or feature-gated loader.* Shapes 6 and 7: a route
   a published build carries is a `pub unsafe fn` again, or unsound.
3. Behaviour and proof: *set `DYLD_FALLBACK_LIBRARY_PATH` too, against Risk 1.* dyld drops both together where it
   drops either, and searches `DYLD_LIBRARY_PATH` first; it adds nothing.
4. Behaviour and proof: *drop the two-copies probe as heavy.* It's the one direct proof of step 3's purpose; the
   step's tests bind the name by hand, which shows the slot, not two copies.
5. Fidelity: *make `ffi` private.* Weighed in decision 6; the raw names stay public (slice 1's decision 20).
6. Mechanics: *`cargo public-api` rather than `jq`.* Decision 10: the facts are read directly, with nothing more
   to install on every run.
7. Behaviour and proof: *the slot's name taken first by a local user is a defect.* A refusal, never two copies;
   said in security item 4.
8. Behaviour and proof: *step 4's package check repeats the core's.* The run's rule asks for the dry run in a fresh
   target directory under the slice's work directory once the crate is ready; the core's runs under `target/`.

## So far

### Step 1: the milestone's slice 1a; the tests load the fake through `Sane::load()` (2026-11-09)

One commit, this one, which holds this record.

- **The milestone:** slice 1a in `milestones/s0-devices.md`, after slice 1: what it builds, what it's checked by,
  the plan's path. A paragraph of its own (`**1a. …**`), since Markdown has no `1a.` list marker: slice 1's list
  ends there and slice 2's starts again at 2.
- **The private loader:** `Sane::start` and `Api::load` are private `unsafe fn`s, each with its `# Safety`: every
  name the system's SANE found by the loader's search under SANE's name, a library already in the process with
  none of SANE's names or a path where nothing is, or (until step 2) a library `load_from`'s caller promised.
  `Api::look_up` too (decision 20). `load()`'s call and `load_from`'s each carry a `SAFETY:` naming their case;
  the `SAFETY:` comments of `Api::load` and `look_up` no longer lean on `load_from`'s caller directly, but on the
  contracts.
- **The tests' route:** `fake_library()` builds the fake as before, then copies it as SANE's leaf name (the file
  name of `Sane::DEFAULT_LIBRARIES[0]`, `libsane.so.1` here) into `CARGO_TARGET_TMPDIR/fake-libsane/as-sane/
  <binary>/`, written as `.libsane.so.1.<pid>.tmp` and renamed, and returns that directory. `Fake::child` puts it
  first on `LD_LIBRARY_PATH` (`DYLD_LIBRARY_PATH` on macOS, by `cfg`), ahead of the parent's value, and sets the
  child's `SANE_CONFIG_DIR` (decision 19). `load_fake()` and a new `try_load_fake()` (for the tests that expect a
  load of the fake to fail: `AlreadyLoaded`, `UnsupportedVersion`, `Init`; it was `load_from(fake_path())`) call
  `Sane::load()`, then fail unless the fake's log is there, naming what was loaded. `load_real()` is
  `Sane::load()` alone. `SANE_DL_LIBSANE`, `common::load_from`, `fake_path` and `SANE_DL_TEST_FAKE` are gone, and
  `tests/real.rs`'s module doc says the loader's variable instead.
- **Windows:** the 98 tests that load the fake (59 `#[test]` attributes, three of them in the test-making macros
  of `options_fake.rs` and `scan_fake.rs`) carry `#[cfg_attr(not(unix), ignore = "loads the fake by SANE's name:
  none on Windows")]`, a reason short enough that rustfmt keeps each attribute on one line.
- **The two path tests** moved into `library.rs`'s unit tests, their names kept:
  `library::tests::a_missing_library_is_not_found_with_what_was_tried` (an absolute path in a fresh `tempfile`
  directory: `NotFound` with the path in `tried`, then again) and
  `library::tests::a_library_without_sanes_functions_is_refused` (`libc.so.6`, `/usr/lib/libSystem.B.dylib`,
  `kernel32.dll`: `MissingSymbol` for `sane_init`, then again), each under one lock, calling the private loader
  in the unit tests' own process (decision 21).
- **Docs:** `fake-libsane`'s crate docs and manifest comment, and the gate's comment in `/justfile`, say tests
  load the fake as SANE through the loader's search path. `sane-dl`'s README, crate docs and changelog are
  unchanged: its public API is, until step 2 rewrites them.

**Departed from the plan:** the "Done when"'s `rg 'load_from|SANE_DL_LIBSANE' /crates` finds, besides
`Sane::load_from` and its docs in `/crates/sane-dl/src/`, `load_from`'s lines in `/crates/sane-dl/README.md` and
`CHANGELOG.md`: they describe the public API as it stands until step 2 removes the function and rewrites both.
No test or helper names either.

**Checked here** (records in `scratch/work/s0-1a/step-1/`): `cargo fmt --check`; `cargo clippy --workspace
--all-targets -D warnings`; `cargo clippy -p sane-dl --all-targets -D warnings` for `x86_64-pc-windows-gnu` and
`x86_64-apple-darwin`; `cargo doc -D warnings` of both crates touched; `sane-dl`'s and `fake-libsane`'s tests with
`SANE_DL_REQUIRE_LIBSANE=1` and `SANE_DL_REQUIRE_HEADER=1`, 221 passed, as at the base. The test list, recorded at
the base (`b47f6e3`, no code changed since `3f33be5`) and after: 246 on Linux each, the two path tests moved as
above, none lost. After a run each `as-sane/<binary>/` (`cancel`, `fake`, `loading`, `options_fake`,
`scan_fake`) holds `libsane.so.1` alone. Make-fails, on a worktree removed after (the branch unchanged): the
loader's variable left off a fake child, and every one of the 98 fake tests fails at `try_load_fake`'s check,
having loaded the system's libsane (its version 1.2.1, not the fake's 1.0.0); the marking's condition flipped to
`unix` with `fake_library()` made to panic, and the run has no failure (80 passed, 98 ignored), while one test's
marking taken off fails it. Left to the step's checking agent: `just ci` whole (gates, MSRV, the package), the
`SAFETY:` comments read against their calls.

**Step 1's check** (2026-11-09, at `142420e`, by the checking agent; its record is local, `scratch/work/s0-1a/flow/
checks.md`): passed. `just ci` with both require variables green (246 tests, `sane-dl`'s 178 on 1.89 too, the
gates, the package's 16 files); the test list 246 to 246, the two path tests moved and none lost; both make-fails
redone on a worktree of its own (98 fake tests failing at the guard; the marking flipped, 80 passed, 98 ignored);
`clippy` for `x86_64-pc-windows-gnu` and `x86_64-apple-darwin` green; each `as-sane/<binary>/` holding SANE's leaf
name alone; the `SAFETY:` comments read against their calls. One low finding, taken by step 2 (decision 25): the
fake child's loader variable got an empty entry, the working directory, where the parent had none. One wording
nit for step 2: "a path where nothing is" means an absolute one, and dyld looks for its file name too.

### Step 2: `load_from` gone, `ffi`'s function types inside the crate, the public API's gate (2026-11-09)

One commit, this one, which holds this record.

- **`Sane::load_from` removed.** The private loader's contract (`Api::load`, `Sane::start`) loses its case:
  each name is SANE's own by system, so what opens is what the loader finds under it, in the environment the
  process started with; or a library already in the process exporting none of SANE's names; or an absolute path
  where nothing is, its file name one no library has (dyld looks for it under that name in its variables'
  directories too). Every `SAFETY:` comment that named `load_from`'s caller (`Api::load`'s two,
  `load()`'s, the `NotFound` unit test's) is rewritten; `look_up`'s contract and comment named none.
- **The docs:** `Sane::load` gains "Which library, and why this is safe": no code chooses the library; a library
  under SANE's name is taken as SANE, as one linked at build time is; never closed; another SANE through
  `LD_LIBRARY_PATH` or `DYLD_LIBRARY_PATH` at the process's start, naming a directory of SANE's own libraries
  alone, or a run path; too late from inside the process; ignored in a set-id process and one dyld restricts.
  The crate docs' "Loading" points there; the README's "What it does" and "Loading, and what can fail" say the
  same, and that a worker's client sets the variable on the worker's command beside `SANE_CONFIG_DIR`.
  `LoadError::NoDefault` no longer points at `load_from`; `NotFound` and `MissingSymbol` say "where the system's
  loader looks" and "found under SANE's name". The changelog's entry is rewritten so, with a paragraph saying
  what changed before the release (`load_from`, `ffi`'s function types; step 4 adds `Image`'s `Debug`).
- **`ffi`:** its 14 function-pointer types and `SANE_Auth_Callback` are `pub(crate)`; the notice's list of
  changes says so, and the module doc says types and constants, nothing callable, the structs plain data.
  The header contract takes `src/ffi.rs` by `#[path]` with `#[allow(dead_code)]`, as the fake does, its module
  doc saying so.
- **`just public-api`** (decisions 10, 22 to 24): `public_api_nightly := "nightly-2026-10-16"`; the recipe
  checks `jq` and that toolchain are there, removes the last JSON, runs `cargo +nightly-2026-10-16 rustdoc
  --locked -p sane-dl --lib -- -Z unstable-options --output-format json --document-hidden-items` with
  `CARGO_TARGET_DIR` at `public-api/` under the target directory, and reads it with `/scripts/public-api.jq`
  (`--argjson format 61`, the floor 1,000 of the crate's own items): the shape first (`format_version`; every
  function's, trait's and function pointer's `is_unsafe` a boolean; every impl's `is_synthetic` and
  `blanket_impl`; every `use`'s `id`, `is_glob` and `name`; the root the crate's module; more own items than
  the floor), then the failures, each named with its source line, then the `unsafe impl`s, listed, then the
  list to `public-api/sane-dl.txt` and a line of counts. In `ci` before `msrv`; the justfile's header says it
  needs `jq` and the pinned nightly. CI's `public-api` job after `sane-dl-package`: `ubuntu-latest`,
  `dtolnay/rust-toolchain@master` with `toolchain: nightly-2026-10-16` and a comment keeping it in step with
  the variable, `persist-credentials: false` under the workflow's read-only permissions.
- **`loader_path`** (the tests' helper) leaves empty entries out (decision 25).

**The gate's first green run, and the API read against the table** ("What a caller keeps today"): 1,651 items
of the crate's own read, 129 public by path, 266 functions (hidden items included), none unsafe; no failure, no
`unsafe impl` (the crate's two, on `RawHandle` and `CancelHandle`, are on private types). The list (758 lines,
kept locally as `scratch/work/s0-1a/step-2/public-api-sane-dl.txt`) was read whole, and the docs of every public
item searched and read for a rule asked of the caller:

| Item | After step 2 |
|---|---|
| `Sane::load_from` | Gone: not in the list |
| `Sane::load()`, which library | Said in its docs; no rule of the caller's |
| `Sane::load()`, one copy | `links` within a build; across builds still the program's rule, in `Sane`'s docs, until step 3 (Linux) |
| `Sane`, `Handle`, `CancelToken` | Bound by types: the list's auto line gives `Sane` and `CancelToken` `Send, Sync`, `Handle` `Send, !Sync`, `Batch` `Send, !Sync` |
| `Handle::select_fd` | A `RawFd` ("never close it, read it, or keep it past then"): advice, since any use of it is the caller's own `unsafe` |
| `ConfigDir::write` | "must exist and be an absolute path": absoluteness checked, `ConfigError::BadPath`, and a missing directory `ConfigError::Io` (corrected after step 2's check); "a directory private to the process": advice for security, no soundness at stake |
| `ffi`'s function types, `SANE_Auth_Callback` | `pub(crate)`: not in the list, and no unsafe function pointer anywhere in it |
| `ffi`'s structs and union | Raw pointers in public fields, plain data (`!Send, !Sync` in the list); a union's field is read only in `unsafe` |
| Advice elsewhere | List devices in a process of their own (calls wait, never UB); whether a batch goes on after an error, and stopping after the page in flight, are the caller's (`Batch` and `Handle` end a pending scan with `sane_cancel` when dropped) |
| `Image` (BUG-7) | Step 4 |

`mem::forget` of each owning type, read in the code: `Sane` (an `Arc` of the instance) leaks the instance, so
`sane_exit` never runs and the process's slot stays taken (a later load is `AlreadyLoaded`); `Handle` leaks its
reference to the instance and its device, never closed, so the same, and a `CancelToken` cloned from it may
still cancel, which stays sound, since the handle is still open and SANE still started; `Batch` (it borrows the
handle mutably) leaves the handle's pending scan to its next call or its drop, which cancels it; `CancelToken`
leaks a small allocation and nothing of SANE's. A leak at worst, each: no soundness rests on a drop running,
since a leak only withholds `sane_close` and `sane_exit`, and what they'd have ended stays valid.

**Departed from the plan:** the gate's `jq` sits in `/scripts/public-api.jq` (decision 22); an `unsafe impl` is
known by its trait, since rustdoc's flag for impls is always false here (decision 23); the step takes step 1's
check's finding on `loader_path` (decision 25), which the plan didn't hold. The recipe also checks the pinned
toolchain is installed first, failing with the command that installs it.

**Checked here** (records in `scratch/work/s0-1a/step-2/`): `cargo fmt --check`; `cargo clippy --workspace
--all-targets -D warnings`; `cargo doc -D warnings` of `sane-dl` and `fake-libsane`; their tests with both require
variables, 221 passed (the header contract against the installed 1.4.0 `sane.h` among them, and the fake built
against the changed `ffi.rs`); `just public-api` green. The test list: 246, the same names as step 1's, the
README's doctest now at line 184 of `lib.rs` (183 before). Make-fails, on a worktree removed after (the branch
unchanged): the nine cases planted together and run once, the eight named (`load_from`, the hidden `unsafe fn`,
the unsafe method, the unsafe trait, the alias of an `unsafe extern "C" fn`, the `*const u8` parameter,
`ffi::SANE_Handle` returned, `libloading::Library` re-exported) and the two `unsafe impl`s (of the planted unsafe
trait, and `Send` for a public type) listed, not failed; then alone the expected format changed to 62, failing
the shape check, and (more than asked) the floor raised and `is_unsafe` renamed in a copy of the JSON, each
failing it too; `SaneStrstatusFn`'s return changed to `SANE_String`, which the library still compiles with, and
the header contract failing to compile at its row for it. Left to the step's checking agent: `just ci` whole,
`clippy --all-targets` and `doc` for `x86_64-pc-windows-gnu` and `x86_64-apple-darwin`, the `SAFETY:` comments
read against their calls.

**Step 2's check** (2026-11-09, at `53eb916`, by the checking agent; its record is local, `scratch/work/s0-1a/flow/
checks.md`): passed. `just ci` with both require variables green (246 tests; the gate's first run in `ci`: 1,651
items read, 129 public by path, 266 functions, none unsafe; `sane-dl` and `twain-dsm` on 1.89; the package's 16
files); the test list as step 1's but the README doctest's line; the gate's list read whole, and every public
item's docs for a rule asked of the caller, none whose breach is undefined behaviour; `mem::forget` of each
owning type a leak at worst; the gate's make-fails redone apart (twelve plants: ten failures named, the two
`unsafe impl`s listed, and a `pub static mut`, which passed: below), the shape check failing alone; the header
contract failing on `SaneExitFn`'s return changed; `clippy` and `doc` for `x86_64-pc-windows-gnu` and
`x86_64-apple-darwin` green; the package and the gate under another `CARGO_TARGET_DIR` green. Low findings, left
for the code review or a fix: the gate doesn't fail a `pub static mut` or an extern static, every use of which is
the caller's `unsafe` (`sane-dl` has no public static); the list doesn't mark the compiler's `unsafe impl
TrivialClone` from `derive(Clone)` on a `Copy` type (no impl fails, so no verdict changes); a failed run leaves
the last list in place. Nits: a 102-column doc line in `/crates/sane-dl/tests/header_contract.rs`; the table above
had `ConfigDir::write`'s "must exist" checked as `BadPath` (corrected there).

### Step 3: one SANE per process, whatever copies of the crate it holds (Linux) (2026-11-09)

One commit, this one, which holds this record.

- **The process's slot:** `/crates/sane-dl/src/process_slot.rs`, `#![forbid(unsafe_code)]`, declared in `lib.rs`
  under `cfg(target_os = "linux")`. `ProcessSlot::take` binds a Unix datagram socket (`UnixDatagram::bind_addr`)
  to an abstract name (`SocketAddr::from_abstract_name`) formed from `/proc/self/ns/pid`'s device and inode and
  the pid (decision 26), the rule's comment saying it's fixed for good. `AddrInUse` is `AlreadyLoaded`; any other
  refusal is the new `LoadError::SlotRefused` (decision 27).
- **In `library.rs`:** the per-copy atomic's guard is `CopySlot`; `Slot` holds the process's slot (Linux) and
  this copy's, takes this copy's first, and frees them in reverse (decision 28). `Sane::start` takes it before
  `Api::load` opens anything; it's a local of `start` until `Shared` takes it, and `Shared` drops it after
  `sane_exit`, as before, so the socket closes after `sane_exit` and on every failed load. `start`'s `sane_init`
  `SAFETY:` comment says the slot covers every copy on Linux and one copy elsewhere, by the docs' rule. The
  step's diff adds no `unsafe`.
- **Docs:** `AlreadyLoaded` (by this or another copy; on Linux also a forked child's hold, or the name bound
  first by another process) and `SlotRefused`; `Sane`'s "One per process" (`links` within a build, but each
  shared library built with the crate a copy of its own; the slot on Linux, and what it can't see; the
  program's rule on macOS and the BSDs: decision 30, security item 7); the crate docs' loading line; the
  README's error list and its paragraph on copies; the changelog's loading entry and its "Changed before the
  release" paragraph; the manifest's comment on `links`.
- **Tests:** unit tests in `process_slot`: the name's rule for given inputs, pinned; the longest name a valid
  abstract address; this process's pid namespace readable. Integration tests in
  `/crates/sane-dl/tests/loading.rs`, Linux only, each in a child, binding the name by the same rule
  (`bind_slot`): `a_slot_bound_first_refuses_the_load_before_the_library_opens` (`AlreadyLoaded`, nothing by
  SANE's name in `/proc/self/maps`, no fake log; once the test's socket is dropped the fake loads, and its log
  holds one `sane_init` and one `sane_exit`); `the_slot_is_held_until_sane_and_its_handles_are_gone` (the
  test's bind `AddrInUse` while SANE is loaded and while a handle outlives the `Sane`, free once it's gone,
  and then, held by the test, the next load `AlreadyLoaded`); `a_refused_load_frees_the_slot` (after an `Init`
  failure and after a refused version); `a_socket_the_system_refuses_refuses_the_load` (decision 29).

**Departed from the plan:** the name holds the namespace's device beside its inode (decision 26); a fourth
integration test, for the new variant (decision 29); the docs say `links`'s reach more exactly than the plan's
"cargo within a build" (decision 30), and say what the slot can't see (security item 7).

**Checked here** (records in `scratch/work/s0-1a/step-3/`): `cargo fmt --check`; `cargo clippy --workspace
--all-targets -D warnings`; `cargo clippy -p sane-dl --all-targets -D warnings` and `cargo doc -D warnings` for
`x86_64-pc-windows-gnu` and `x86_64-apple-darwin`, and `cargo doc -D warnings` on the host, private items too;
`sane-dl`'s and `fake-libsane`'s tests with both require variables, 228 passed. The test list: 253, step 2's 246
and the seven new tests, none lost, the README's doctest now at line 188 of `lib.rs`. `just public-api`: 1,653
items of the crate's own, 129 public paths, none unsafe; the list differs from step 2's by `variant SlotRefused`
and its `field 0` alone. The two-copies probe (`scratch/work/s0-1a/step-3/two-copies/`, not committed: two
`cdylib`s, each its own workspace and target, each taking `sane-dl` by path, and a host that opens both and
calls each): at `53eb916` both copies loaded, and the fake's log showed a second `sane_init` with its violation,
"called again before sane_exit"; with the step, the second copy's load is `AlreadyLoaded`, and the log has one
`sane_init` and no violation. Make-fails, on a worktree removed after (the branch unchanged), each failing at
least one of the step's tests for the reason planted: the slot's module undone, the per-copy atomic alone (three
of the four integration tests fail; the fourth checks freeing, which needs a slot to free); the socket leaked,
never freed; this copy's atomic kept when the process's slot is refused; `AddrInUse` taken as `SlotRefused`;
another refusal taken as `AlreadyLoaded`; the name's rule changed. Left to the step's checking agent: `just ci`
whole (the gates, the MSRV build and tests on 1.89, the package), the `SAFETY:` comment changed, read against its
call.

**Step 3's check** (2026-11-09, at `8568577`, by the checking agent; its record is local, `scratch/work/s0-1a/flow/
checks.md`): passed. `just ci` with both require variables green (253 tests, no skip, `prlimit` there; the
package's 17 files, `src/process_slot.rs` the new one; the gate: 1,653 items, none unsafe; `sane-dl` and
`twain-dsm` on 1.89); the test list 246 to 253, seven new, none lost; the gate's list as step 2's but `variant
SlotRefused` and its `field 0`; the two-copies probe redone apart, with a drop between (before the step both
copies loaded and the fake logged `sane_init` twice; after, the second refused until the first was dropped, and
no violation); make-fails: the slot's module undone (three of the four integration tests fail) and, its own,
the process's slot taken after the library opens (two fail); cross `clippy` and `doc` green; no `unsafe` added;
a fork and exec probe bearing out the docs (the socket close-on-exec; an exec'd child holds nothing, a forked one
holds the slot until it ends). Three findings, taken by step 4 (decisions 9 and 32): the prlimit test failed
rather than skipped where `prlimit` was missing (the parent checked the fake's empty log) and asserted where the
limit didn't take; the `sane_init` `SAFETY:` comment left out what the slot can't see; decision 9 named only
macOS and the BSDs as exceptions.

### Step 4: BUG-7; the plan's files; the slice's end (2026-11-09)

One commit, this one, which holds this record.

- **BUG-7:** `Image`'s `Debug` written by hand (decision 31): `Image { format: FrameFormat::GRAY, depth: 8,
  width: 1024, height: 1024, stride: 1024, frames: [Parameters { .. }], data: 1048576 bytes }`, never the
  bytes; `Image` alone drops `derive(Debug)`, `Page` derives it still and prints that; the struct's doc says
  why. Unit tests in `scan.rs`, on a 1 MiB gray image whose every byte is 171, a number nothing else in its
  `Debug` holds: `an_images_debug_gives_its_datas_length_not_its_bytes` (its `{:?}` the text above exactly; it
  and `{:#?}` under 1 KiB, `1048576 bytes` in each, no `171`, every other field named) and
  `a_pages_debug_gives_its_images_length_not_its_bytes` (a `Page::Image` of it, the same for both forms). Each
  assertion's message gives the text's length and its first 200 characters (`told`), never the text. The
  changelog: "Changed before the release" names it, and the scanning entry says what `Debug` gives.
- **Step 3's check's findings** (decisions 9 and 32): `Child::run` returns whether the child ran its body, and
  `a_socket_the_system_refuses_refuses_the_load` leaves out its checks of the fake's log when its child
  skipped; "the limit doesn't take" is a skip; the test's doc says when it skips. `sane_init`'s `SAFETY:`
  comment names what the slot can't see (a change of network namespace or of `/proc`'s view between two
  copies' loads) and that beyond it a program keeps to one copy, as `Sane`'s docs say. Decision 9 names both
  exceptions.
- **The plan's files:** `crates/sane-dl.md` as built (slice 1a beside slice 1; which library and why that's
  safe; another SANE through the loader's variable, and M5 proposal 6's `SHEAF_LIBSANE` as that variable on
  sheaf's test's child; the slot, its name's rule, `AlreadyLoaded`, `SlotRefused` and what it can't see;
  `ffi`'s function types inside; `Image`'s `Debug`; the gate in "Boundary"; the tests' route, the path tests
  inside the crate, the slot's tests and the probe; 230 `sane-dl` and `fake-libsane` tests; BUG-7 gone from
  "Open", BUG-8 and FEAT-13 in it, TEST-7 revised); `crates/fake-libsane.md` ("How tests load it", and loaded
  as SANE wherever it said by path); `crates/README.md`'s line and `crates/scan-fixtures.md`'s sentence (as
  SANE through the loader's search path, ignored on Windows); `plan.md`'s row and its line under the graph;
  the milestone (in slice 1's entry, `load_from` "which slice 1a removed"; slice 1a's "Built on the branch",
  and what it waits on); `decisions.md` (the ruling confirmed 2026-11-09, in its words, and how `sane-dl`
  follows it); `status.md` (a bullet of its own, after slice 2's); `slices/README.md`. The backlog: BUG-7 to
  `archive/backlog-done.md` under "Bugs closed by S0 slice 1a", its "Done" said; BUG-8 (bugs, later), FEAT-13
  (features, later), TEST-12 (tests, S0), each read against the code first; TEST-7 revised (the fake's tests on
  macOS through `DYLD_LIBRARY_PATH`, beside Homebrew's SANE); "Where to start"; the counts redone (47 open:
  BUG 7, FEAT 13, TEST 9; S0 15, later 10). The IDs are the next ones `origin/master` names, fetched as the
  step started (`8276ad1`, which closed slice 8 and changed no backlog file): none of the three is an item
  there, only each file's "the next new one". Slice 3 plans the same three (decision 18).

**Departed from the plan:** step 3's check's three findings are taken here (decisions 9 and 32), as step 2 took
step 1's. `decisions.md`'s "Made here" entry on the fake also said tests "load it by its path", a place the plan
didn't list: now "load as SANE". `bugs.md` had no "Later" heading, so BUG-8 adds one, and its S0 heading stays,
saying none is open. `origin/master` has moved to `8276ad1` since the slice began (slice 8 closed: `status.md`,
the milestone's slice 8 entry, `slices/README.md`, `archive/`); this branch doesn't take it, and its edits to
those files keep to lines of their own, so the merge meets as few as it can.

**Checked here** (records in `scratch/work/s0-1a/step-4/`): `cargo fmt --check`; `cargo clippy --workspace
--all-targets -D warnings`; `cargo clippy -p sane-dl --all-targets -D warnings` for `x86_64-pc-windows-gnu` and
`x86_64-apple-darwin` (the tests' helper changed); `cargo doc -D warnings` of `sane-dl`; its tests with both
require variables, 187 passed (the fake's 43 untouched). The test list: 255, step 3's 253 and the two `Debug`
tests, none lost, the README's doctest still at line 188. Make-fails, on a worktree removed after (the branch
unchanged; `step-4/make-fail.md`): the derived `Debug` put back, and both new tests fail on texts of over 5 MB
each, the log 3.5 KB; `prlimit` renamed, and the test skips and passes (at `8568577` it failed so); the same with
the parent's guard removed, and it fails on the empty log; the limit made not to take, and it skips and passes.
Left to the step's checking agent: `just ci` whole (the gates, the gate's list unchanged from step 3's, the MSRV
build and tests, the package), `just sane-dl-package` in a fresh `scratch/work/s0-1a/package/` with its 17
files read, the branch's commit messages and the maintainer's name, the backlog's IDs against `origin/master`,
the changed `SAFETY:` comment read against its call. What only CI shows, after the push: macOS's fake tests
through `DYLD_LIBRARY_PATH` (Risk 1, TEST-7); Windows's fake tests ignored with their reason, none failing; the
`public-api` job on CI's pinned nightly; the slot's tests on CI's Ubuntu; the MSRV job's real-libsane tests
skipping; the package job.
