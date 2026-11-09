# S0 slice 8: twain-dsm

Status: running, on branch `s0-8-twain-dsm`, from 2026-11-08: planned, reviewed, its six steps built and
pushed, code-reviewed and fixed; CI on a pull request and approval close it. Milestone:
`milestones/s0-devices.md`, slice 8. Run on the Windows machine, the same way as slice 7
(`archive/slices/s0-7-twain-spike.md`): this plan, an adversarial review (below), the steps (each committed and
pushed), parallel code reviews, a fix stage.

Paths: `docs/pm/` files from `docs/pm/`; the code's from the repository's root with a leading `/`; `scratch/`
from the root, local and never tracked.

## What it builds

`twain-dsm` (`crates/twain-dsm.md`): TWAIN through the Data Source Manager, as a library. Its bindings
generated from `TWAINH.cs` and checked against `twain.h` 2.5 by a committed script and by a header contract
compiled with MSVC in both bitnesses; the DSM loaded by its full path from the system folder after its signer
is checked, never unloaded; a TWAIN thread scoped by the caller (an apartment, a hidden top-level window, a
message loop that works in every state); TWAIN's states as owned types; the application's real record of what
is open kept apart from them, so that teardown is sound whatever a caller forgets; capabilities in all four
containers; native and memory transfers; extended image info; the driver's own window; a request channel
other threads post to. **Safe by construction** (`decisions.md`, 2026-11-08): no `unsafe fn` in its public API.
It says what TWAIN says (the seam's rule): it hands over a DIB's block and each memory buffer as TWAIN gives
them; decoding and assembling them are the `twain` backend's (slice 9).

Its tests take over the spike's: the sample source driven through the crate's API in both bitnesses, here and
in CI. `twain-spike` is deleted; its `image` module moves into the tests' support, for the native-against-memory
comparison, until slice 9 takes it into the backend.

Not built here: the `twain` backend and the 32-bit worker (slice 9); file transfers; the driver's window owned
by the application's own window (slice 9 settles the owner, seam section 9); several sources open at once.

## Taken as it starts

- **Safe APIs** (`decisions.md`, 2026-11-08): the API explored below before one is fixed; the review attacked
  the choice as well as the code.
- **The same process as slice 7** (2026-11-08: "lets run the same workflow that we just did for slice 8").
- **Slice 7's close-out** (its plan archived, the milestone and status updated) is this branch's first commit,
  pushed with step 1.

## The API, explored

Six questions, each with the shapes considered and the one taken. The names are the design's; an
implementation that keeps the design and renames says so in "So far".

**1. Who owns the TWAIN thread.** Every DSM call must come from one thread, which pumps its window's messages.
- *(A) The crate's thread, behind a `Send` handle:* simple to call from anywhere, but every TWAIN state hidden
  and checked at run time, and the crate defining a request for everything TWAIN can do.
- *(B) The caller's thread, scoped:* `twain_dsm::run(|twain| { .. })` makes the calling thread the TWAIN thread
  for the closure (the apartment entered, the window made, both undone after) and hands it a `&Twain<T>`.
  Every state is a value on that thread, checked by the compiler.
- **Taken: (B)**, with what (A) needs to be built on it in slice 9: `Twain::wait(timeout) -> Wake<T>`, which
  pumps messages in every state (`Wake::Request(T)`, `Wake::Timeout`), and a non-blocking
  `Twain::take_request()`. `T` is the caller's own request type: `run::<T, R>` and `Remote<T>` carry it, so
  the backend's commands (list, settings, scan, cancel) travel as they are.

**2. How states are held.**
- *(i) An owned chain:* each state owns the one beneath it; a step up consumes `self`; a step that fails hands
  `self` back (`Refused<S> { state, error }`, `#[must_use]`, with `into_state()` and `into_error()`, the
  latter dropping the state, which steps down; no blanket `From`, so `?` never walks a chain down unseen).
- *(ii) Borrows* (`Source<'d>` of `&'d mut Opened`), for several sources at once. *(iii) One struct, its
  state checked at run time*, as the spike's `Session` was.
- **Taken: (i)**, one source at a time, as the worker needs. A state's `Drop` steps down and rolls back as the
  spike learned (a step refused with `TWCC_SEQERROR` restarts from state 7's first step). **Soundness doesn't
  rest on `Drop`:** `mem::forget` and `Rc` cycles are safe, so a state can vanish without stepping down.
  `Twain` keeps the real record (connection open, source open or enabled), updated at each triplet that
  succeeds, and `run`'s end acts on it: if the connection is open it sends `MSG_CLOSEDSM`, which the DSM
  answers by closing every source still open itself (`MSG_ENDXFER`, `MSG_RESET`, `MSG_DISABLEDS`,
  `MSG_CLOSEDS`; `apps.cpp` 497-519). If even that fails, `run` keeps the window and the apartment (leaks
  them) and says so (`RunReport::stranded`). States that need an event (`Ready`, 6) are only made from one:
  `Enabled::next_event(self) -> Result<Next>`, `Next::Ready(Ready) | Next::Event(Enabled, Event) |
  Next::Request(Enabled, T) | Next::Timeout(Enabled)`; nothing else makes a `Ready`.

**3. Foreign memory, without contracts.**
- *The DSM is never unloaded* (as `sane-dl` never unloads libsane): loaded once per process, its entry point
  and memory functions are valid for the process.
- *Containers decoded into owned values at once:* `get` returns a `CapValue`; `set` takes a `Value` (one, an
  enumeration or an array) and answers `SetOutcome::Exact` or `Adjusted` (`TWRC_CHECKSTATUS`, seam section 2).
- *A native transfer's DIB as an owner:* `Dib`, owning the global block, locked once for its whole life (so its
  slice is valid on any thread, the block fixed or moveable), unlocked and freed on `Drop`; `Send`.
  `Ready::native() -> Native::Done(Dib, Transferred) | Native::Cancelled(Transferred)`.
- *Memory buffers borrowed from the caller* for one call: `MemoryTransfer::preferred_size()` resolves
  `TWON_DONTCARE32` and caps it; `next_buffer(&mut self, &mut [u8])` refuses a slice under `MinBufSize`.
- *Extended image info* decoded into owned values; the handles the source allocated (an item too big for
  `TW_UINTPTR`, or several) freed inside with the DSM's `MemFree`.

**4. One thread, by borrow.** Every state holds `&'t Twain<T>`, and `Twain` is `!Send` and `!Sync`; `run`'s
closure is higher-ranked over `'t`, so no state can be returned from it, stored in a thread-local or a
static, or sent to another thread (the review's probe: E0521, "lifetime may not live long enough"). Other
threads hold a `Remote<T>` (`Send + Sync`, `Clone`): `post(T)` sends on the `Twain`'s channel and wakes the
loop with a posted message, under a lock that `run`'s teardown takes to clear the window's handle, so a post
never reaches a reused handle; after the `run` ends it's `Err(Gone(T))`, the value handed back.

**5. Loading and trust.** `twain.load(Load::System)` loads `<system folder>\TWAINDSM.dll` by its full path
(`GetSystemDirectoryW`, SysWOW64 in a 32-bit process); `Load::Path(path)` the application's own copy. Either
way: the file opened denying writes and deletes; its Authenticode signature checked through that handle
(`WinVerifyTrust`, offline) and its signer's name, `O=TWAIN Working Group` (by name, not certificate: 2.5.0
and 2.5.1 have different certificates and one signer), **before** it's loaded; loaded while the handle is
held; its loaded path compared with the path checked; and refused if a `TWAINDSM.dll` from elsewhere is in the
process already (`GetModuleHandleW`). No unchecked load: nothing in this slice needs one, and calling an
unverified DLL through `DSMENTRYPROC`'s ABI would be a contract only a caller could keep. One `Loaded` at a
time per `Twain`, and **one `Twain` per process**: the DSM has no locks (none in `dsm.cpp`, `apps.cpp`,
`hook.cpp`), so two TWAIN threads would race on its globals; the flag is an atomic taken by `compare_exchange`
and released by a guard, so it clears on a panic too, and stays taken if a teardown had to leak.

**6. The callback.** Registered with `DAT_CALLBACK2` only (its `RefCon` is `TW_UINTPTR`; `DAT_CALLBACK`'s is
32 bits, `dsm.cpp` 1006), its procedure our trampoline, its context a `static` (the window's handle, atomic,
and a generation number), so it can never dangle; it posts to the window and returns `TWRC_SUCCESS`, touching
nothing else and unable to panic. **Delivery is fixed per open source** (`open_source(id, Delivery)`): TWAIN
has no unregister, and a registration survives `MSG_DISABLEDS` and `MSG_CLOSEDS` (the review's probe: a later
event-mode enable hung). `Delivery::Events` registers a null procedure, which clears a stale one, and offers
every message to `MSG_PROCESSEVENT` while enabled; `Delivery::Callback` is refused for a source without
`DF_DS2`, which signals only through `MSG_PROCESSEVENT`.

**What stays `unsafe`, inside:** the DSM and Win32 calls (one module each), the trampoline, `GetProcAddress`'s
cast, the reads of a driver's blocks (each length checked first). `ffi`'s function-pointer types and the
structs holding them (`TW_ENTRYPOINT`, `TW_CALLBACK2`) are `pub(crate)`. Safe means safe given a DSM and
drivers that keep TWAIN's calling rules, as any FFI does; their data stays untrusted.

**A scan, as the API reads** (memory transfer, no UI; `Cmd` is the caller's request type):

```rust
let report = twain_dsm::run::<Cmd, _>(|twain| -> Result<(), Error> {
    let opened = twain.load(Load::System)?.open(App::new("pdf-scan", "worker", "worker"))
        .map_err(Refused::into_error)?;
    let id = opened.sources()?.into_iter().find(|s| s.product_name() == "TWAIN2 Software Scanner")
        .ok_or(Error::NoSuchSource)?;
    let mut source = opened.open_source(&id, Delivery::Callback).map_err(Refused::into_error)?;
    source.set(cap::ICAP_XFERMECH, Value::U16(ffi::TWSX_MEMORY))?;
    let remote = twain.remote(); // to another thread: remote.post(Cmd::Cancel)
    let mut enabled = source.enable(Ui::Hidden).map_err(Refused::into_error)?;
    let mut ready = loop {
        match enabled.next_event(Duration::from_secs(20))? {
            Next::Ready(ready) => break ready,
            Next::Request(back, Cmd::Cancel) => { back.disable().map_err(Refused::into_error)?; return Ok(()) }
            Next::Event(back, _) | Next::Timeout(back) => enabled = back,
        }
    };
    loop {
        let info = ready.image_info()?;
        let mut transfer = ready.memory().map_err(Refused::into_error)?;
        let mut buf = vec![0; transfer.preferred_size()];
        while let Some(b) = transfer.next_buffer(&mut buf)? { /* b.shape(), b.bytes() */ }
        match transfer.end().map_err(Refused::into_error)? {
            Pending::More(next) => ready = next,
            Pending::Done(back) => { back.disable().map_err(Refused::into_error)?; break }
        }
    }
    Ok(())
})?; // RunError: AlreadyRunning, Apartment; report.value is the closure's result
```

## The design

- **The crate:** `/crates/twain-dsm/`, a library, edition 2024, `rust-version` measured as slice 1 measures
  it (the floor 1.89), MIT OR Apache-2.0, `publish = false` at 0.0.0 (decision 7), `lints.workspace = true`.
  Windows only in what it does: every module but `ffi`, the codes and the pure decoders is `#[cfg(windows)]`.
  Dependencies: `windows-sys` 0.61 (Windows only; the spike's features, plus `Win32_Security_WinTrust`,
  `Win32_Security_Cryptography` and `Win32_System_SystemInformation`), `thiserror` 2; dev: `rusty-fork` 0.3
  (`timeout`) and `find-msvc-tools` (MSVC for the header contract; smaller than `cc`, which wraps it), vetted
  at the step that adds them. `thiserror` and `rusty-fork` move to `[workspace.dependencies]`, as the root
  `Cargo.toml`'s comment asks once two members take one, `sane-dl`'s two lines with them.
- **`ffi`** (public, types and constants only, no unsafe): every numeric `#define` of `twain.h` that
  `TWAINH.cs` defines too (about 1150 of 1205), generated from `TWAINH.cs` and checked against `twain.h` by
  `/scripts/twain-bindings.py` (committed; it fetches both files at their pins into a temporary folder, writes
  `ffi`'s constants and a `CONSTANTS: &[(&str, u64)]` table, and reports the names in one file and not the
  other, with its rename map: `TWBT.X3OF9` for `TWBT_3OF9` and the like); the rest (`TWPT_BGR`, `TWSX_FILE2`,
  `TWAF_`, others) written by hand from `twain.h`, each said. The structs the crate uses and the containers'
  (`TW_RANGE`, `TW_ARRAY`, `TW_FRAME`, the `TW_STR*` and `TW_UNI512`), `#[repr(C, packed(2))]`, layouts
  asserted at compile time for both bitnesses. The notice from `TWAINH.cs` and the TWAIN Toolkit's disclaimer
  in the file and in the crate's rustdoc.
- **The header contract** (an integration test, Windows): with `twain.h` at `TWAIN_DSM_TWAIN_H` (here,
  `scratch/reference/twain/twain-dsm/twain.h`; in CI, fetched at twain-dsm `a50f7326a191` into the runner's
  temp, its SHA-256 pinned, never committed: the header grants no licence), it writes a C program (`<windows.h>`
  first, which `twain.h` needs) printing every struct's size and field offsets, compiles it with the MSVC that
  `find-msvc-tools` finds for the test's own target, runs it, and compares; and it reads every numeric
  `#define` and checks each has an entry of the same value in `CONSTANTS`, the hand-written exceptions listed.
  Without the header it says so on stderr; `TWAIN_DSM_REQUIRE_HEADER=1` (CI, and every check here) fails.
- **Codes:** `ReturnCode(pub u16)` and `ConditionCode(pub u16)` with a constant per code, `name()` and
  `Display`; `Triplet { dg, dat, msg }` named ("DG_CONTROL/DAT_PARENT/MSG_OPENDSM").
- **Errors** (thiserror, `#[non_exhaustive]`; plain data isn't): `RunError` (`AlreadyRunning`, `Apartment`,
  `Window`); `LoadError` (`NotFound`, `Elsewhere { loaded }`, `Unsigned`, `WrongSigner { subject }`,
  `MissingEntry`, `Machine`, `AlreadyLoaded`); `Error` with `Failed { triplet, rc, by_source:
  Option<ConditionCode>, by_dsm: Option<ConditionCode> }`. Each party asked once, the DSM first; the DSM keeps
  some refusals' codes globally, and asking with the application's identity reads and clears only its own, so
  a per-application `TWCC_SUCCESS` after a DSM failure is asked again with an identity whose id is out of range,
  which reads the global one (`apps.cpp` 619-637), and is `None` if that's success too. Also `Refused` (a
  container or block that doesn't hold up), `Timeout`, `NoSuchSource`, `CallbackUnsupported`.
- **The thread** (`run`): takes the process's flag; enters a single-threaded apartment (a thread already in a
  multithreaded one is `RunError::Apartment`); registers and creates the hidden top-level window (its procedure
  `DefWindowProcW`); runs the closure; then the teardown above (`MSG_CLOSEDSM` if the record says open), clears
  the `Remote`s' handle, destroys the window, leaves the apartment, releases the flag. The loop (`wait`,
  `next_event`): only the window's own messages are the crate's (matched by window, not number); requests are
  kept; in `Delivery::Events` every other message is offered to `MSG_PROCESSEVENT` while a source is enabled,
  and a carrier of a source message isn't dispatched; a callback's post in `Events` mode, or a `DSEVENT` in
  `Callback` mode, is a failure.
- **Capabilities:** `get`, `get_current`, `get_default`, `set`, `reset`, `query_support`; `CapValue` for all four
  containers and every item type `twain.h` defines (integers, `TW_BOOL`, `TW_FIX32`, `TW_FRAME`, the strings,
  `TWTY_STR1024` and `TWTY_UNI512` among them), every length checked against `GlobalSize` before it's read.
- **Enabling and the driver's window:** `Source::enable(Ui::Hidden | Ui::Shown)` (`Shown` parented to the hidden
  window: the application's own as owner is slice 9's); `Source::settings_ui()` (`MSG_ENABLEDSUIONLY`, where
  `CAP_ENABLEDSUIONLY` says the source has it); `Event::CloseRequest` (`MSG_CLOSEDSREQ`), `Event::CloseOk`
  (`MSG_CLOSEDSOK`), `Event::Device` (`MSG_DEVICEEVENT`, then `DAT_DEVICEEVENT`).
- **Transfers and a batch's end:** `Ready::image_info`, `native`, `memory`; `ext_image_info(ids)` in state 7;
  `Transferred::end() -> Pending` (`More(Ready)` or `Done(Enabled)`, the count kept, 0xFFFF "unknown");
  `Ready::reset() -> Enabled`; `Ready::stop_feeder()`.
- **For the seam** (`crates/twain-dsm.md`): per transfer, `DAT_IMAGEINFO` as TWAIN gives it; the pending
  count; extended image info as offered; each memory buffer's shape and bytes, and a native transfer's DIB
  block, not assembled; events with how they came; condition codes from both parties; requests served in every
  state.
- **Hostile input:** the decoders (containers, extended image info, strings) take random input in a unit test
  (a fixed-seed generator, many thousand cases, every result `Ok` or `Err`, never a panic, on every runner,
  i686 included); a `cargo-fuzz` target for them is TEST-11, for a slice with nightly at hand.

### How the tests run

- **Every test that calls `run` runs in a child process of its own** (rusty-fork, `timeout`), as `sane-dl`'s
  do: one `Twain` per process, the DSM never unloaded; `CAP_XFERCOUNT` given in the child's environment at
  spawn. A test that needs the sample source and doesn't find it exits 77 after saying why, and the parent says
  so on its own stderr and passes; `TWAIN_DSM_REQUIRE_SAMPLE=1` (CI's job, and every check here) fails instead.
- **The spike's scenarios as tests:** listing; native and memory transfers in both deliveries, page and
  pending counts; `compare` (the spike's `image` module in `tests/support/`); a cancel mid-page and one before
  the enable; a state dropped right after the enable rolling back; **an `Enabled` forgotten (`mem::forget`),
  `run` closing the connection, and a second `run` in the same process working**; a delivery switched on a
  reopened source not hanging (the null procedure); the signature cases (the installed DSM passes; Windows'
  `twain_32.dll`, catalog-signed, is `Unsigned`; a DLL with an embedded signature not the TWAIN Working
  Group's, chosen by probe in step 2, is `WrongSigner`, refused before it's loaded); `AlreadyRunning`;
  `AlreadyLoaded`; `Gone`.
- **Inside the crate** (unit tests with access to its private calls, in child processes): a source triplet to a
  source never opened, which the DSM refuses itself (`TWCC_SEQERROR`, `dsm.cpp` 599), reported with both
  parties' codes.
- **Compile-time checks:** `compile_fail,E0521` (a state returned from `run`'s closure) and `compile_fail,E0277`
  (a state sent to a thread) doctests, each beside a passing twin that differs only by the violation, all
  Windows only.
- **Unit tests** need no DSM and run everywhere: containers both ways, extended image info items, codes,
  strings, `FIX32`, the triplets' names, the random-input test.

## Steps

**Every build here** runs from Git Bash with `CARGO_HOME` at `scratch/cargo-home`. **At every step** the check
runs once: `just fmt clippy test doc`; clippy for `i686-pc-windows-msvc` and `x86_64-unknown-linux-gnu`, docs
for the latter; the crate's tests in both bitnesses with `TWAIN_DSM_REQUIRE_SAMPLE=1` and
`TWAIN_DSM_REQUIRE_HEADER=1`; `cargo +1.89 check` of the crate; `cargo tree -p twain-dsm --target all -e
normal,build` against the devices layer's patterns; the name gate's `git grep`; no `pub unsafe fn` and no
`unsafe` in a public signature (`rg` over the crate, and the rustdoc of its public items read); the test list
compared with the step before's; `~/.cargo` unchanged; every `SAFETY` comment the step adds read against its
call; the diff and message scanned for local paths, the username, the downstream app's name outside
`docs/pm/`, and the maintainer's name anywhere.

1. **The crate and its bindings.** The manifest and `[workspace.dependencies]`, the crate doc (the disclaimer),
   `/scripts/twain-bindings.py` and `ffi` from it (its report saved in `scratch/work/s0-8/`), the hand-written
   exceptions, `CONSTANTS`, the codes, the header contract, the pure decoders with their unit and random-input
   tests; the gates' devices-layer list gains `twain-dsm`; CI's TWAIN job fetches `twain.h` and runs the
   contract in both bitnesses; a `twain-dsm-msrv` job (Windows, the measured toolchain). Slice 7's close-out
   pushed with it.
   **Done when:** the core; the script's report saved; the contract passing in both bitnesses and seen to fail
   on a change the compile-time assertions don't cover (a constant's value; an unasserted offset), reverted;
   the MSRV measured.
2. **The thread, loading, states 1 to 3.** `run`, `RunError`, the flag and its guard, `Twain`, `wait`,
   `take_request`, `Remote<T>`, `load` with the signature and its order, `Loaded`, `Opened`, `App`, `sources`,
   the record, the teardown, `RunReport`.
   **Done when:** the core; listing in both bitnesses; the signature cases; `AlreadyRunning`, `AlreadyLoaded`,
   `Gone`; requests through `wait` with no source open; the compile-time checks, each twin passing.
3. **Sources and capabilities.** `Source`, `open_source` with `Delivery`, the null procedure, `get` and the
   rest, `SetOutcome`, both parties' codes.
   **Done when:** the core; the spike's capability answers through the API in both bitnesses; the internal
   refusal test; `CallbackUnsupported` for a source without `DF_DS2` (a unit test of the check, the sample
   having it).
4. **Enabled, transfers and events.** `Enabled`, `Next`, `Ready`, `Native`, `Dib`, `MemoryTransfer`,
   `Transferred`, `Pending`, `ext_image_info`, `stop_feeder`, `reset`, `Ui::Shown`, `settings_ui`, the events.
   **Done when:** the core; the transfer tests (both mechanisms, both deliveries, page and pending counts,
   `compare`) in both bitnesses; `Ui::Shown` and `settings_ui` run by hand here (the sample's Qt window, closed
   by hand), not in CI.
5. **Drops, rollback, cancels, teardown.** Every state's `Drop` and the rollback; requests in `next_event`; the
   cancels; the forgotten state and the second `run`; the delivery switch.
   **Done when:** the core; those tests in both bitnesses, here and in CI; a make-fail where `MSG_CLOSEDSM` is
   taken as refused: `run` keeps its window and apartment, `RunReport::stranded` says so, a second `run` is
   `AlreadyRunning`, no crash.
6. **The spike retired; what it found, written down.** `crates/twain-spike` deleted (its `image` module in
   `tests/support/`); CI's job renamed `twain-dsm` and its recipe `twain-dsm-sample`; README rows; the code and
   CI free of the spike, the docs citing it as history; `crates/twain-dsm.md` (as built, "For the seam"), the
   seam note where it changed, the milestone, `status.md`, `decisions.md`, TEST-11, this plan's "So far".
   **Done when:** the core; every claim in the docs traced to a report or a line.

**Closing:** the code review (parallel reviewers), the fixes, CI green, approval.

## Decisions taken alone, for review

1. **The thread is the caller's, scoped by `run`**, with `wait`, `take_request` and `Remote<T>` for slice 9.
2. **An owned chain of states, one source at a time;** no blanket `From<Refused<S>>`.
3. **Teardown from `Twain`'s record, not from `Drop`:** `MSG_CLOSEDSM` at `run`'s end; a leak of the window and
   apartment, said, if even that fails.
4. **The DSM never unloaded; containers decoded at once; the DIB an owner locked for life.**
5. **Loading by full path, verified before the load through a held handle; no unchecked load.**
6. **One `Twain` per process, for the DSM's lack of locks.**
7. **`publish = false` at 0.0.0** until slice 9's backend has used the API; then made ready to publish.
8. **The bindings: every constant of `twain.h`, generated where `TWAINH.cs` has it, by hand where not;** the
   structs the crate and the containers use, the rest when a slice needs them.
9. **The header contract compiles `twain.h` fetched at test time,** never committed.
10. **The spike's `image` module lives in the tests until slice 9.**
11. **The callback's context a `static`; delivery fixed per open source.**

## Security, for review

1. **Loading the DSM runs every installed source's code** in the process (the DSM loads each at
   `MSG_OPENDSM`); the backend runs it in a worker (slice 9). The DSM is loaded by its full path, its file held
   against change, its signer checked before its code runs.
2. **The header and `TWAINH.cs` are fetched** by the binding script and by CI from the TWAIN Working Group's
   repositories at pinned commits, over TLS, their hashes pinned.
3. **A driver's bytes are untrusted:** containers, extended image info, the DIB block and memory buffers are
   checked against their measured lengths and the caps before they're read.

## Risks

1. **The API meets its first real user in slice 9:** kept at 0.0.0 meanwhile.
2. **`find-msvc-tools` at test time** on `windows-latest` (the review saw it work here in both bitnesses).
3. **Child processes per test, in both bitnesses,** lengthen CI; the job's timeout grows if it must.
4. **Slice 1 lands on `master` meanwhile:** this branch rebases before each push; `sane-dl`'s manifest (the
   workspace dependencies), the justfile, CI and `status.md` are the likely conflicts.
5. **The sample signals synchronously and is TWAIN 2:** a 1.x driver (the fi-6130's may be) is seen only by
   hand, in slice 12.

## Plan review

2026-11-08, before step 1, by one adversarial reviewer (a fresh agent given the plan, the repository, the
spike's code in git and the primary sources), with probes in `scratch/work/s0-8/review-probe/` (the spike
rebuilt and run against the sample; a branded-lifetime crate; MSVC found from a test; the DSM's load order;
`WinVerifyTrust`; the constants counted). The load-bearing findings were re-read in the DSM's source here: a
null callback procedure accepted (`dsm.cpp` 1069-1087), refusals' codes kept globally (`apps.cpp` 619-637), no
locks anywhere. The plan as drawn is at `scratch/work/s0-8/plan.before-review.md`. All 24 accepted; none
rejected.

**High:** 1. *Soundness rested on `Drop`*, which `mem::forget` skips, leaving a registered callback's
context freed under the DSM: now the record in `Twain`, `MSG_CLOSEDSM` at `run`'s end, the callback's context
a `static`, and a test that forgets a state. 2. *The DSM accepts a null callback procedure*, so step 3's
refusal couldn't happen: now a triplet to a source never opened, from inside the crate. 3. *Requests couldn't
be served outside `Enabled`*, and a crate-defined request type couldn't carry the backend's: now `wait` in
every state, `take_request`, `Remote<T>`.

**Medium:** 4. A callback registration survives close and reopen, and 1.x sources never call it: delivery fixed
per open source, a null procedure for events, `Callback` refused without `DF_DS2`. 5. The DSM keeps some codes
globally: asked again with an out-of-range id, else `None`. 6. `twain_32.dll` is catalog-signed, so
`Unsigned`, not `WrongSigner`: an embedded-signed DLL found by probe for that. 7. A bare-name load returns any
`TWAINDSM.dll` already loaded, and a check after loading is too late: full path, a held handle, check first,
compare after. 8. One `Twain` per process for the DSM's lack of locks, not its connections; the flag released
by a guard. 9. Every test calling `run` forked; exit 77 for a skip. 10. The header contract: `<windows.h>`
first; `CONSTANTS`; a deliberate failure the compile-time asserts don't catch first. 11. thiserror reads a
field named `source` as the error's source: `by_source`, `by_dsm`. 12. The driver's window, its close events
and `TWRC_CHECKSTATUS` were missing: `Ui::Shown`, `settings_ui`, the events, `SetOutcome`. 13. A bare
`compile_fail` passes on any error and on Linux: error codes pinned, passing twins, Windows only. 14.
`ready()` could forge state 6: only `next_event` makes a `Ready`. 15. `?` through a blanket `From` walked whole
chains down unseen: `into_error` and `into_state`, `#[must_use]`. 16. The make-fail asserted what can't fail
(the DSM is never unloaded; the DSM copies identities): it asserts the window and apartment kept and a second
`run` refused. 17. `Trust::Unchecked` was a caller's memory contract under a safe name: left out.

**Low:** 18. Two `Loaded` from one `Twain`: `AlreadyLoaded`. 19. `DAT_CALLBACK2` only (its `RefCon` is full
width; `DAT_CALLBACK`'s 32 bits). 20. The DIB locked for life, `Native::Cancelled`, `preferred_size` resolving
`TWON_DONTCARE32`, `MinBufSize` checked. 21. 1205 numeric defines, 58 without a `TWAINH.cs` name: the
generator committed with its rename map, the rest by hand, `TWTY_STR1024` and `TWTY_UNI512` decoded. 22.
Function-pointer types `pub(crate)`. 23. `rust-version`, `[workspace.dependencies]`, an MSRV job, the decoders'
random-input test and TEST-11, `App` given to `open`, `RunError`, `find-msvc-tools` direct. 24. Six steps, not
five; step 6's "no trace" limited to code and CI; question 4 a borrow, not a brand.

**Checked and sound** (the reviewer's list, kept): a state can't be returned from `run`'s closure or stored in a
thread-local (E0521), and `!Send` carries through `&'t Twain`; `DAT_CALLBACK2`'s `RefCon` comes back full width
in both bitnesses; a null procedure clears a stale callback; `find-msvc-tools` (through `cc`) finds `cl.exe` for
both targets from a test with no developer prompt; `twain.h` builds with MSVC after `<windows.h>`, `TW_IDENTITY`
156 bytes in both; both installed DSMs pass `WinVerifyTrust` offline; never unloading is enough for the
function pointers, and the DSM frees its state at the last `MSG_CLOSEDSM`, so a later `run` starts clean; the
DSM refuses a duplicate `MSG_OPENDSM`; `MSG_CLOSEDSM` force-closes open sources; `TWRC_CANCEL` leaves the source
in state 7; a `Dib` is `Send`. (Step 3 found the force-close skips the last source the DSM lists.)

## Code review

2026-11-09, after step 6, by four reviewers in parallel, each a fresh agent given the code and the primary
sources, not the author's conclusions, each with probes of its own (`scratch/work/s0-8/review-code/`: one
report each, and `fixes.md`, every finding's disposition): soundness (`unsafe`, FFI, threads); TWAIN's
protocol, against the DSM's source, the sample's and the specification; hostile input and the bindings; the
tests, CI, the docs and the API for slice 9. About seventy findings, a few found by two; all taken but four,
said in `fixes.md` with why. The high ones:
- **Two copies of the crate in one process** (two versions, a DLL and its host) each had their own run flag and
  took the DSM the other loaded: two threads in the lockless DSM, which crashed in two of three probe runs.
- **Another message loop on the TWAIN thread** (a driver's dialog, a message box) took the callback's post,
  which carried the source's message, or the DSM's wake-up: `MSG_XFERREADY` lost, the source stuck in 6.
- **Extended image info on 64-bit** read two `TW_UINT32` (a handle the source sent) as inline data, by
  `TW_EXTIMAGEINFO`'s rule; `TW_INFO`'s, the specification's example and the sample application say four bytes.
- **A driver's large container** made the decoder panic on 32-bit (capacity overflow).
- **Tests that passed with the thing broken:** a request that never woke the loop (no wait was timed);
  `reset` and `disable` that sent nothing (the next step down made up for them); a child that ran no test.
- Medium, among others: a stale `MSG_XFERREADY` made a `Ready` in state 5; a refused step moved the record;
  `MSG_CLOSEDSREQ` in 6 or 7 went unrecorded; messages weren't offered to `MSG_PROCESSEVENT` with a callback;
  the buffer rules (largest, even, aligned, one size) unchecked; image info not offered in state 7; the hidden
  window closable from outside; a handle named twice freed twice; `preferred_size` past its 64 MiB; defines the
  generator and the contract couldn't read passing unseen; the contract blind to fields' signedness; two NUL
  bytes making a test file binary to git, and so to the name gate.

## So far

**Step 1, the crate and its bindings** (2026-11-08; reports in `scratch/work/s0-8/step-1/`):
- **The bindings:** `/scripts/twain-bindings.py` generates `ffi/consts.rs` from `TWAINH.cs`, both files fetched at
  their pinned commits and their SHA-256 checked (or given by path): 1192 constants, each checked against
  `twain.h`, none differing. Its renames recover 27 of the 58 names the review counted (`X3OF9` as
  `TWBT_3OF9` and the other leading digits, `TWPFS` as `TWPF_`, `DG`'s flags as `DF_`, `TWON_PROTOCOL` as
  `TWON_PROTOCOLMAJOR`/`MINOR`); `TWMF_` is typed `u32` (`TW_MEMORY`'s `Flags`) and the protocol `u16`, where
  `TWAINH.cs` types them otherwise; `TWLG_` stays `i16`, as `TWLG_USERLOCALE` is -1. The other 31, and 5
  aliases in `twain.h`'s deprecated section (`TWSS_A4LETTER` and four more), are in `ffi/by_hand.rs`, which the
  script checks too: 1228 in all, `twain.h`'s 1205 numeric defines and 23 aliases. Only `DAT_CUSTOM` is in
  `TWAINH.cs` and not `twain.h`. `--check` compares the file with what it would write; the module is
  `#[rustfmt::skip]`, so the text stays the script's. The report: `bindings-report.txt`.
- **The header contract** is a unit test, not an integration test, so that it reads the structs kept inside the
  crate (`TW_ENTRYPOINT`, `TW_CALLBACK2`): it compiles a C program (`<windows.h>` first) with the `cl.exe`
  `find-msvc-tools` finds for the test's target, and compares 28 structs, 98 offsets and the 1228 constants.
  It found the 5 aliases the script hadn't looked for (the script now does). Passing in both bitnesses; failing
  as it should (`make-fail.txt`) on `ICAP_XFERMECH` changed and on `TW_DEVICEEVENT`'s `FlashUsed2` and
  `AutomaticCapture` swapped, which keeps every size the compile-time assertions check: 3 differences,
  reverted, the script's `--check` passing after.
- **Renamed from the plan:** the constants' table is `ffi::constants()`, crate-private, its values `i64` (for
  the negative ones); the containers are `Container` (`One`, `Enumeration`, `Array`, `Range`) of `Item`s, not
  `CapValue` and `Value`; a list's one item type is checked when it's made (`Items::new`, `Range::new`).
  `TWTY_HANDLE` items decode to their value and are refused when sent: only a source gives a handle.
- **Decoders:** every length checked before it's read, in `u64`; random input (60,000 blocks, 20,000 answers, a
  fixed seed) decodes or is refused, never a panic, and 20,000 random containers encode and decode to
  themselves. Extended image info: the request written and the answer's entries read; reading an entry's
  data, inline or behind a handle, is step 4's.
- **Manifests and CI:** `[workspace.dependencies]` with `thiserror` (`sane-dl` takes it from there);
  `rusty-fork` joins with step 2, the first to use it. CI's TWAIN job fetches `twain.h` (its hash checked),
  runs the script's `--check`, and the crate's tests in both bitnesses with the header required; a
  `twain-dsm-msrv` job on Windows; the gates read `twain-dsm`'s graph (`thiserror` and its macro crates).
- **MSRV:** the code needs 1.88 (`slice::as_chunks`: 1.87 fails with E0658, `msrv-1.87.0.log`); the floor stays
  1.89.
- **The check:** `just fmt clippy test doc`; clippy for i686 and Linux, docs for Linux; 20 tests in each
  bitness with the header; `cargo +1.89 test`; no `unsafe` in a public signature (the five function-pointer
  types are `pub(crate)`); the name gate; `~/.cargo` unchanged.

**Step 2, the thread, loading, states 1 to 3** (2026-11-08; reports in `scratch/work/s0-8/step-2/`):
- **The thread:** `run` takes the process's flag (`compare_exchange`, released by a guard), enters the
  apartment, makes the window (its class registered once per process, never unregistered), runs the closure
  under `catch_unwind`, then closes the connection if the record says it's open; a panic is resumed after.
  The record is `Twain`'s own (`dsm`, `loaded`, `connected`, the application's identity in a
  `Box<Cell<TW_IDENTITY>>`, so the DSM can write its `Id` through a pointer that doesn't move). If the DSM
  refuses `MSG_CLOSEDSM`, the identity is leaked and the window, the apartment and the flag stay:
  `RunReport { value, stranded: Option<Error> }`. `Wake` is `Request`, `Timeout` or `Quit` (a `WM_QUIT`
  taken from the queue is handed to the caller rather than lost).
- **Loading** as planned, two things learned. A 32-bit process reaches SysWOW64 by the System32 path and may
  be told either name, so files are compared by identity (volume and file index), never by path: the module
  of the same name already loaded, the module after loading, a later `load`. And `WinVerifyTrust` verified
  `kernel32.dll`, catalog-signed, as Microsoft's (`WrongSigner`), not `Unsigned` as the review had it (the fix
  stage found `cmd.exe`, catalog-signed too, `Unsigned`: what a check of the file finds varies, and either is
  refused). So the test makes its own cases from the installed DSM: a copy with its certificate table
  emptied is `Unsigned`, a copy with one byte changed is `BadSignature` (`TRUST_E_BAD_DIGEST`); with
  `concrt140.dll` (signed in the file, by Microsoft) each is refused before it's loaded, the test checking
  that no module of its name is. `LoadError` gained `Open`, `BadSignature`, `Loader` and `NotTheFileChecked`;
  `AlreadyLoaded` holds while a connection is open as well as while a `Loaded` lives.
- **Condition codes, corrected:** the DSM clears its code for no application whenever an application reads
  its own (`apps.cpp`, `AppGetConditionCode`), so the plan's order (the application's, then the global one)
  would always read success second. The DSM's code is asked for no application first (an identity whose `Id`
  is out of range, which takes the read path that checks it), then the application's, which wins if it has
  one.
- **The compile-time checks:** stable rustdoc checks that a `compile_fail` example fails, not why (one named
  E0599 instead of E0521 passed); nightly's checks the code. So a `twain-dsm-doc-codes` job runs the doctests
  on nightly. A state kept outside the closure is E0521, sent to a thread E0277; returned from the closure,
  rustc says "lifetime may not live long enough", which has no code, so that example is a bare
  `compile_fail` beside its twin. Each has a twin that compiles.
- **Renamed from the plan:** `Error::Container` for a container or block that doesn't hold up (`Refused` is
  the state handed back); `Timeout` and `CallbackUnsupported` come with the steps that need them.
  `rusty-fork` joined `[workspace.dependencies]`, `sane-dl`'s line with it.
- **Tests:** 7 in child processes (the sample listed, the signature cases, one `run` at a time on this thread
  and another, one `Loaded` at a time, requests in states 1 and 3, `Gone`, a connection forgotten with
  `mem::forget` closed at the end and a second `run` after it, twice); 5 doctests; 2 unit tests of the PE
  reader, random input among them.
- **The check:** as step 1's; 34 tests in each bitness with the sample and the header required; clippy for
  i686 and Linux; `cargo +1.89 test`; nightly's doctests; the graph adds `windows-sys` and `windows-link`.

**Step 3, sources and capabilities** (2026-11-08; reports in `scratch/work/s0-8/step-3/`):
- **The DSM doesn't close every source left open.** A `Source` forgotten, the connection closed cleanly at
  the end, the next `run` couldn't open the sample: `TWCC_MAXCONNECTIONS` (`forgotten-source-before-fix.txt`).
  `RemoveApp` (`apps.cpp`) closes sources left open in a loop from 1 to `NumFiles - 1`, while sources take ids
  1 to `NumFiles` as they're listed, so the last listed is never closed, and a source installed alone never
  is. The plan's teardown leaned on that loop, and the review's "checked and sound" list had it. Now the end
  of `run` closes the source itself from the record first (`MSG_CLOSEDS`; the states above it with steps 4
  and 5), then the connection; a source that won't close is `RunReport::source_left`. The same test passes
  now, twice in one process.
- **A triplet to a source not open** is the DSM's `TWCC_OPERATIONERROR` for the application ("Unable to find
  driver", `dsm.cpp`), not `TWCC_SEQERROR` as the plan had it (that one is for an application whose
  connection isn't open); the in-crate test asserts it, the source not asked.
- **The callback:** `window::source_callback`, a safe `extern "system" fn` with `TWAINCALLBACKPROC`'s
  signature, posts `WM_SOURCE` (the message in `wParam`, a generation in `lParam`) to the window in a
  process-wide atomic, and returns `TWRC_SUCCESS`; the loop keeps a post of the current generation while the
  source open delivers by callback. `RefCon` is 0. The window is cleared at the end of `run` unless it's left.
- **Containers through the DSM:** `capability_read` and `capability_write` keep every handle inside `dsm.rs`:
  the source's block measured with `GlobalSize`, copied out, freed; ours allocated with the DSM's
  `DSM_MemAllocate`, freed after the call. `Source::set` takes `impl Into<Container>`, so an `Item` is a
  `TW_ONEVALUE`; `TWRC_CHECKSTATUS` is `SetOutcome::Adjusted`. `query_support` gives the `TWQC_` bits.
  `Error::Allocation` and `Error::CallbackUnsupported` joined.
- **Tests:** the sample through `Callback`, `Events`, then `Callback` again on the source reopened (the
  delivery switch, which step 5 was to add): the spike's capability answers, an enumeration (`MSG_GET` of a
  pixel type, and of a boolean), an array (`CAP_SUPPORTEDCAPS`), a range (`ICAP_THRESHOLD`, 0 to 255, set to
  100 and reset to 128), `MSG_QUERYSUPPORT`, the source's refusals with its `TWCC_CAPUNSUPPORTED` and no code
  of the DSM's, a string too long refused before it's sent; a source forgotten and closed at the end, twice;
  the delivery check for a TWAIN 1 source (a unit test, the sample being TWAIN 2).
- **The check:** as step 2's; 38 tests in each bitness.

**Step 4, enabled, transfers and events** (2026-11-08; reports in `scratch/work/s0-8/step-4/`):
- **One step down, from the record:** the record gained the source's state (4 to 7), set at each triplet that
  moves it, and `Twain::step_down_to(floor)` walks down from there (`MSG_ENDXFER`, `MSG_RESET`,
  `MSG_DISABLEDS`, `MSG_CLOSEDS`; a step refused is passed over to the next, but for `MSG_CLOSEDS`). Every
  state's `Drop` is one call of it (`Transferred` and `MemoryTransfer` to 6, `Ready` to 5, `Enabled` to 4,
  `Source` to 3), and so is the teardown. `MSG_XFERREADY` moves the record to 6 as it's received, not when
  `next_event` hands it over: the sample is in state 6 before `MSG_ENABLEDS` returns, so an `Enabled` dropped
  then starts with `MSG_RESET` (the spike's `TWCC_SEQERROR` lesson). Step 5 adds the rollback.
- **The loop:** `Twain::wait` and `Enabled::next_event` share one pump. With a source enabled that delivers by
  events, every message it takes is offered to `MSG_PROCESSEVENT` first, the crate's own posts too (a request
  the DSM eats is still in the channel); by callback, the posts of the current generation are kept. `Next`
  gained `Quit`; a request taken while looking for the source's message is put back ahead of the channel.
  `Event::Device` holds `DAT_DEVICEEVENT`'s answer, boxed (it's 294 bytes).
- **Transfers** as planned: `Native::Done(Dib, Transferred)` or `Cancelled`; `Dib` locked for its life,
  `Send` and `Sync`, freed with `GlobalFree`. `image_info` gives `TW_IMAGEINFO` as TWAIN does.
  `MemoryTransfer::preferred_size` takes the preferred size, else the largest, else 64 KiB, at least the
  smallest and at most 64 MiB; `next_buffer` refuses a buffer under `MinBufSize` (`Error::BufferTooSmall`)
  and checks `BytesWritten` against the buffer. `MSG_ENDXFER` from 6 or 7 gives `Pending`, the count kept
  (`Ready::pending`, `None` for 0xFFFF).
- **Extended image info:** the specification's rule (TWAIN 2.4, `TW_EXTIMAGEINFO`): an entry's `Item` is the
  handle of a block, which the application frees, when the item's size times `NumItems` is larger than a
  `TW_HANDLE`, else its data inline; a `TWTY_HANDLE` entry's `Item` is always a handle, its block the data's
  bytes (the specification's barcode text). An unknown item type's `Item` can't be told from data, so isn't
  freed. Unit tests; the sample refuses `DAT_EXTIMAGEINFO` (`TWCC_CAPUNSUPPORTED`) in state 7, and
  `MSG_STOPFEEDER` (`TWCC_BADPROTOCOL`) in 6, both checked.
- **The source's window** (`tests/ui.rs`, `#[ignore]`d): run here in both deliveries, `Ui::Shown` and
  `settings_ui` (`CAP_ENABLEDSUIONLY` true) each enabled, no event in 5 seconds with no one at the window, and
  closed by `MSG_DISABLEDS`. With someone there, `TWAIN_DSM_UI_SECONDS=60` gives time to press Scan, OK or
  Cancel.
- **Tests:** three pages through each mechanism with each delivery, the pending count after each (0xFFFF,
  0xFFFF, then none); one page native and one through memory in RGB, gray and 1-bit, equal pixel for pixel.
  The spike's `image` module is in `tests/support/` now (its 18 unit tests run in the transfers binary); the
  spike's copy goes with it in step 6.
- **The check:** as step 3's; 60 tests in each bitness, the 2 by hand besides.

**Step 5, drops, rollback, cancels, teardown** (2026-11-08; reports in `scratch/work/s0-8/step-5/`):
- **The rollback:** a step down refused with `TWCC_SEQERROR` means the source isn't where the record says
  (an `Enabled` dropped right after `MSG_ENABLEDS`, before the pump has taken the source's `MSG_XFERREADY`:
  the record says 5, the source is in 6). The walk then starts again once from state 7, as TWAIN.cs's
  rollback does, and that refusal is reported only if the floor still isn't reached. Seen to matter: with the
  restart switched off, that drop fails, `MSG_DISABLEDS` and then `MSG_CLOSEDS` refused with `TWCC_SEQERROR`
  (`make-fail-rollback.txt`); with it, both deliveries pass.
- **Drop errors:** a state's `Drop` has nowhere to put a refusal, so `Twain::drop_errors()` hands over what the
  states dropped since couldn't do (the first 16).
- **The pump, corrected:** `next_event` said the source's messages came first, but asked for a request before
  the pump had taken the source's message already queued: a cancel posted before the enable came back ahead
  of `MSG_XFERREADY`. The pump now takes every message queued before it decides; the early-cancel test caught
  it.
- **The make-fail** as planned, from inside the crate: a switch compiled only into its tests takes
  `MSG_CLOSEDSM` as refused. The run reports `stranded`, the window is still a window (`IsWindow`), the thread
  still a single-threaded apartment (`CoInitializeEx` answers `S_FALSE`), the next `run` is `AlreadyRunning`,
  nothing crashes; the dropped `Opened`'s refusal is in `drop_errors`.
- **Tests:** a state dropped at each of five points (right after the enable; a transfer ready; a native
  transfer done; a memory transfer begun; one after its first buffer), in each delivery, with no drop error,
  and the sample scanning three pages after each; the same five forgotten with `mem::forget`, each run's end
  stepping the source down (`source_left` and `stranded` none), and a second run in the process scanning three
  pages; a cancel posted from another thread during page 1 (that page ends, then `MSG_RESET`) and one before
  the enable (the transfer ready comes first, then the request, then `MSG_RESET`), in each delivery; a request
  through `next_event` with the feeder empty. The delivery switch was step 3's test, and step 4's transfers
  run `Events` after `Callback` on one connection.
- **CI:** the TWAIN job sets `TWAIN_DSM_REQUIRE_SAMPLE=1`, which the plan had and step 1 left out.
- **The check:** as step 4's; 65 tests in each bitness. The plan's "here and in CI" waits on the pull request:
  no CI run has had the branch yet.

**Step 6, the spike retired; what it found, written down** (2026-11-08):
- **The spike gone:** `crates/twain-spike` deleted (its `image` module lives on in `tests/support/`, as step 4
  put it there), with its workspace entry, its two recipes and its CI steps. CI's TWAIN job is `twain-dsm`
  now, its recipe `twain-dsm-test` (the plan's `twain-dsm-sample`: the one recipe runs every test of the
  crate, the sample's among them). The code, scripts and CI name the spike nowhere; the docs cite it as
  history. The README's row is gone, `twain-dsm`'s rewritten; the crate's own doc describes it as built, with
  an example, its Windows part included only on Windows so the docs and doctests build elsewhere.
- **The pump, aligned with the plan:** the crate's own messages (its requests' and callback's posts) are
  never offered to `MSG_PROCESSEVENT`, as the plan's design and the spike had it; step 4 offered them too.
  Every test passes either way, in both bitnesses.
- **Docs:** `crates/twain-dsm.md` rewritten as built, "For the seam" included; the seam note revised where the
  crate changed it (sections 3 and 8, the unknowns' table, a paragraph saying so); the milestone's slice 8
  entry and `status.md` say it's built; `slices/README.md` lists it running; TEST-11 (the decoders fuzzed with
  `cargo-fuzz`) in `backlog/tests.md`. `decisions.md` is unchanged: no ruling was made during the slice; its
  decisions taken alone, above, wait for review.
- **The check:** as step 5's; 66 tests in each bitness.

**Fix stage** (2026-11-09; `scratch/work/s0-8/review-code/fixes.md`, the scripts and make-fails in
`scratch/work/s0-8/fix/`):
- **The thread:** the run lock is a semaphore named for the process (`Local\twain-dsm run <pid>`), which every
  copy of the crate takes, a semaphore since a mutex would let its own thread take it twice; `dsm::load`
  refuses a DSM this copy didn't load (`LoadError::InUse`). The hidden window's procedure is the crate's,
  ignoring `WM_CLOSE`. The callback keeps the source's message in a process-wide ring of atomics, tagged with a
  generation changed at every enable and disable, its post only a wake-up (a registered message); each turn of
  the pump reads the ring and, for delivery by events, offers a made-up `WM_NULL` to `MSG_PROCESSEVENT`, which
  the DSM answers with a message it holds. Messages are offered to the source in either delivery; for a TWAIN 2
  source by events, the carrier of a held message is dispatched still. The pump checks its deadline as it
  drains, and takes a timeout too long to add as none.
- **The record:** `MSG_XFERREADY` is taken only in state 5; `MSG_CLOSEDSREQ` in 6 or 7 moves it to 5, and the
  transfer methods, which poll the source first, answer `Error::CloseRequested`. `disable` and `reset` walk
  strictly (the record moves only on a step taken; a restart's guess undone if it's refused); a `Drop`'s walk
  passes over; a walk that reaches its floor reports nothing; the restart tries `MSG_RESET` before
  `MSG_ENDXFER`. Each enable registers the null procedure again for events, empties the queue, and keeps its
  `TW_USERINTERFACE` at one address for the disable (`Enabled::modal`, `forced_ui`). `open` and `open_source`
  are refused while a connection or source the DSM wouldn't close is open (`Error::StillOpen`).
- **Transfers:** the buffer rules checked (`BufferTooLarge`, `BufferMisaligned`, `BufferSizeChanged`), sizes no
  buffer meets refused (`BufferSizes`), `preferred_size` within them and a multiple of 4; `image_info` returns
  `ImageInfo`, unpacked, and is offered in state 7 after `TWRC_XFERDONE`, as `ext_image_info` is only then
  (`Error::TransferNotDone`); `Pending::More` carries the count; `stop_feeder` keeps none.
- **Decoders:** extended image info's four-byte rule; a source's block at most 16 MiB, a list at most 65,536
  items, `try_reserve`; a handle named twice freed once; a null handle for items `Malformed`; the request
  pre-filled `TWRC_INFONOTSUPPORTED`; strings read to what their type holds; ranges of `BOOL`; a one value sent
  padded to the widest item.
- **Bindings:** the generator and the contract fail on a `#define` with a value they can't read (but the three
  calling-convention macros, and `CAP_FILESYSTEM`'s commented-out value); the contract compares 70 integer
  fields' signedness; the generator reads every `pub const` line of `by_hand.rs` and checks its type against
  the generated prefix's. Each seen to fail on its case (`make-fail-sign.txt`, `make-fail-unread.txt`, and the
  generator's on a cast and a changed type).
- **Found in the fixing:** with `InUse`, `kernel32.dll` (loaded in every process) is `InUse` before its
  signature is read, so the catalog case moved to `cmd.exe`, which came back `Unsigned` where `kernel32.dll`
  had verified as Microsoft's: the test takes either, and step 2's note says so.
- **Tests:** waits timed (a request that doesn't wake the loop fails); `disable` then a `set` taken in state 4;
  the triplets `reset` and `disable` send, from a triplet log compiled into the crate's tests; the pending
  counts with `CAP_XFERCOUNT` 3 (2, 1, done); another message loop for 300 ms after the enable (fails with
  polling switched off, `make-fail-foreign-loop.txt`); a stale transfer ready; the buffer rules; `InUse` and
  `Elsewhere` with a DSM the test loads itself; the other bitness's DSM refused for its machine; `WM_QUIT`;
  `requeue`; `get_default`; inside the crate, `WM_CLOSE` posted to the window, the run lock held by another
  copy, the 16-error cap, `RunReport::closed_*`. A child that's done exits 78, so one that ran no test fails;
  skips reach stderr past the capture; the NUL bytes are escapes. The random-input test of extended image info
  reaches the decoding now (it reached it 0 times in 67,456 entries).
- **API for slice 9:** `Twain::requeue`, `Source::opened`, `ImageInfo`, `From<TW_FIX32> for Fix32`,
  `RunReport::closed_source` and `closed_connection`.
- **CI and tools:** the TWAIN job runs the crate's clippy for i686 too; the justfile says what `just ci` covers.
- **The check:** as step 6's; 78 tests in each bitness and on 1.89, the 2 by hand besides.
