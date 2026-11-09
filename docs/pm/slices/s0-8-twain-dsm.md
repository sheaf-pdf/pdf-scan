# S0 slice 8: twain-dsm

Status: planned, on branch `s0-8-twain-dsm`, from 2026-11-08. Milestone: `milestones/s0-devices.md`, slice 8.
Run on the Windows machine, the same way as slice 7 (`archive/slices/s0-7-twain-spike.md`): this plan, an
adversarial review, the steps (each committed and pushed), parallel code reviews, a fix stage.

Paths: `docs/pm/` files from `docs/pm/`; the code's from the repository's root with a leading `/`; `scratch/`
from the root, local and never tracked.

## What it builds

`twain-dsm` (`crates/twain-dsm.md`): TWAIN through the Data Source Manager, as a library. Its bindings ported
from `TWAINH.cs` and checked against `twain.h` 2.5, by a script and by a header contract compiled with MSVC
in both bitnesses; the DSM loaded from the system folder, its signer checked by name, never unloaded; a
TWAIN thread (an apartment, a hidden top-level window, a message loop); TWAIN's states as owned types, each
stepping down on `Drop` and rolling back when the source is further along; capabilities in all four
containers; native and memory transfers; extended image info; the callback; condition codes from both
parties; a handle other threads use to post requests. **Safe by construction** (`decisions.md`, 2026-11-08):
no `unsafe fn` in its public API. It says what TWAIN says (the seam's rule): it hands over a DIB's block and
each memory buffer as TWAIN gives them; decoding and assembling them are the `twain` backend's (slice 9).

Its tests take over the spike's: the sample source driven through the crate's API in both bitnesses, on this
machine and in CI. `twain-spike` is deleted; its `image` module moves into the tests' support, for the
native-against-memory comparison, until slice 9 takes it into the backend.

Not built here: the `twain` backend and the 32-bit worker (slice 9); file transfers, `ShowUI` with the
application's own window as owner, and several sources open at once (later, if a slice needs them).

## Taken as it starts

- **Safe APIs** (`decisions.md`, 2026-11-08): the API explored below before one is fixed; the review asked to
  attack the choice, not only the code.
- **The same process as slice 7** (2026-11-08: "lets run the same workflow that we just did for slice 8").
- **Slice 7's close-out** (the spike's plan archived, the milestone and status updated) is this branch's first
  commit, pushed with step 1.

## The API, explored

Six questions, each with the shapes considered and the one taken. The names are the design's; an
implementation that keeps the design and renames says so in "So far".

**1. Who owns the TWAIN thread.** Every DSM call must come from one thread, which pumps its window's messages.
- *(A) The crate's thread, behind a `Send` handle:* the user calls `twain.open_source(..)` from anywhere and the
  call is sent to the thread and answered. Simple to call, but every TWAIN state lives hidden behind the
  handle, checked at run time; and the crate must define a request for everything TWAIN can do.
- *(B) The caller's thread, scoped:* `twain_dsm::run(|twain| { .. })` makes the calling thread the TWAIN
  thread for the closure (the apartment entered, the window made, both undone after) and hands it a `Twain<'t>`.
  Every state is a value on that thread, checked by the compiler; the caller (slice 9's backend) chooses the
  thread and owns the loop.
- *(C) Both,* (A) built on (B).
- **Taken: (B).** It keeps `twain-dsm` to TWAIN's own shape (the seam's rule) and lets the types carry the
  states; (A) is the backend's to build in slice 9 if it wants it, from (B).

**2. How states are held.**
- *(i) An owned chain:* each state owns the one beneath it; a step up consumes `self`; a step that fails hands
  `self` back with the error (`Refused<S> { state, error }`), so nothing is lost; `Drop` steps down.
- *(ii) Borrows:* `Source<'d>` borrowing `&'d mut Opened`, for several sources at once.
- *(iii) One struct, its state checked at run time,* as the spike's `Session` was.
- **Taken: (i)**, one source at a time, as the worker needs; (ii) if a later slice needs two. `From<Refused<S>>
  for Error`, so `?` works and drops the state, which steps down. Each `Drop` rolls back as the spike learned:
  a step the source refuses with `TWCC_SEQERROR` restarts from state 7's first step (`MSG_ENDXFER`,
  `MSG_RESET`, then `MSG_DISABLEDS`); a state that can't step down keeps what's beneath it alive (the DSM
  stays loaded, the identities kept) and says so through `Twain::stranded()`.

**3. Foreign memory, without contracts.**
- *The DSM is never unloaded* (as `sane-dl` never unloads libsane): loaded once per process, its entry point
  and memory functions are then valid for the process, so nothing holding them can outlive them.
- *Containers decoded into owned values at once:* `get` returns a `CapValue` (the container's kind, item type,
  current and default values, the list or range); `set` takes a `Value`. The handles are allocated, locked,
  read and freed inside the call; no container type reaches the user.
- *A native transfer's DIB as an owner:* `Dib`, owning the `GlobalAlloc` block, its bytes as a slice of the
  measured length while it lives, freed on `Drop`; `Send`, since a fixed global block is valid on any thread.
- *Memory buffers borrowed from the caller* for one call: `next_buffer(&mut self, &mut [u8]) -> Option<Buffer>`,
  the `Buffer` borrowing the slice it was written into.
- *Extended image info* decoded into owned values; the source's handles (an item too big for `TW_UINTPTR`, or
  several) freed inside.

**4. One thread, by type.** Every state, and `Twain<'t>` itself, is `!Send`. Each is also branded with `'t`,
the lifetime `run` gives its closure (higher-ranked, as `std::thread::scope` does with its `'scope`), so none
can escape the closure, into a thread-local or a static, and outlive the window and apartment. Other threads
hold a `Remote` (`Send + Sync`, `Clone`) to post requests (`Remote::post(Request)`): it holds the window's handle
behind a lock that the window's teardown clears, so a post never reaches a reused handle; a post after the
`run` ends says `Gone`.

**5. Loading and trust.** `twain.load(Load::System)` loads `TWAINDSM.dll` from the system folder only
(`LOAD_LIBRARY_SEARCH_SYSTEM32`); `Load::Path(path)` loads the application's own copy. Both check the
Authenticode signature (`WinVerifyTrust`, offline: no revocation fetch) and the signer's name, `O=TWAIN
Working Group`, by name and not by certificate (2.5.0 and 2.5.1 have different certificates and the same
signer), unless `Trust::Unchecked` is asked for. Safe, as `sane-dl`'s `load_from` is, with its trust said in
its docs: a DLL's code runs when it loads, and in Rust's terms that's the platform's contract, not a memory
contract the caller could keep. One `Twain` per process at a time (a second `run` while one lives is
`AlreadyRunning`), as `sane-dl` has one live `Sane`: the DSM keeps one application connection per identity and
the sample keys its state by it.

**6. The callback's context.** The DSM hands the callback's `RefCon` back as its data pointer (`dsm.cpp`
2486-2497, read): `RefCon` points to the window's slot, boxed in the `Twain`, which lives as long as any source
could call. The callback posts to the window and returns `TWRC_SUCCESS`; it touches no other state and can't
panic.

**What stays `unsafe`, inside:** the DSM and Win32 calls (one module each), the callback trampoline,
`GetProcAddress`'s cast, the reads of a driver's blocks (each length checked first). Safe means safe given a
DSM and drivers that keep TWAIN's calling rules, as any FFI does; their data stays untrusted (caps on every
length, as `plan.md`'s rule has it).

**A scan, as the API reads** (memory transfer, no UI):

```rust
twain_dsm::run(|twain| -> Result<(), Error> {
    let opened = twain.load(Load::System)?.open()?;
    let id = opened.sources()?.into_iter().find(|s| s.product_name() == "TWAIN2 Software Scanner")
        .ok_or(Error::NoSuchSource)?;
    let mut source = opened.open_source(&id)?;
    source.set(cap::ICAP_XFERMECH, Value::U16(ffi::TWSX_MEMORY))?;
    let remote = twain.remote(); // to another thread, for a cancel
    let mut enabled = source.enable(Ui::Hidden, Delivery::Callback)?;
    let mut ready = match enabled.next_event(Duration::from_secs(20))? {
        Event::XferReady => enabled.ready(),
        other => return Err(Error::Unexpected(other)),
    };
    loop {
        let info = ready.image_info()?;
        let mut transfer = ready.memory()?;
        let mut buf = vec![0; transfer.preferred_size()];
        while let Some(b) = transfer.next_buffer(&mut buf)? { /* b.shape(), b.bytes() */ }
        match transfer.end()? {
            Pending::More(next) => ready = next,
            Pending::Done(back) => { back.disable()?; break; }
        }
    }
    Ok(())
})?;
```

## The design

- **The crate:** `/crates/twain-dsm/`, a library, edition 2024, MIT OR Apache-2.0, `publish = false` at
  0.0.0 (decision 7), `lints.workspace = true`. Windows only in what it does: every module but `ffi`, the codes
  and the pure decoders is `#[cfg(windows)]`, so the crate builds, lints and tests (its pure parts) on every
  runner. Dependencies: `windows-sys` 0.61 (Windows only; the spike's features, plus `Win32_Security_WinTrust`
  and `Win32_Security_Cryptography` for the signature), `thiserror` 2; dev: `rusty-fork` 0.3 (`timeout`) and
  `cc` 1 (to find MSVC for the header contract), vetted at the step that adds them as slice 1 vets its own.
- **`ffi`** (public, types and constants only, no unsafe): every constant `TWAINH.cs`'s enums define that
  `twain.h` defines too, generated by a script from `TWAINH.cs` and checked against `twain.h` (the spike's
  `consts.py`, grown to every enum; kept in `scratch/`, its output committed; its report of names in one file
  and not the other saved); the structs the crate uses and the containers' (`TW_RANGE`, `TW_ARRAY`,
  `TW_FRAME`, the `TW_STR*`), `#[repr(C, packed(2))]`, layouts asserted at compile time for both bitnesses;
  function-pointer types from `twain.h`. The notice from `TWAINH.cs` and the TWAIN Toolkit's disclaimer in the
  file and in the crate's docs (rustdoc), as the TWAIN License asks.
- **The header contract** (an integration test, Windows): with `twain.h` at `TWAIN_DSM_TWAIN_H` (here,
  `scratch/reference/twain/twain-dsm/twain.h`; in CI, fetched at twain-dsm `a50f7326a191` into the runner's
  temp, never committed: the header grants no license), it writes a C program printing every struct's size and
  field offsets, compiles it with MSVC found by `cc`'s `windows_registry::find_tool` for the test's own target,
  runs it, and compares with the bindings; and it reads every numeric `#define` in the header and checks each
  has a constant of the same value, the exceptions listed with why. Without the header it says so on stderr;
  `TWAIN_DSM_REQUIRE_HEADER=1` (CI) makes that a failure.
- **Codes:** `ReturnCode(pub u16)` and `ConditionCode(pub u16)` with a constant per code, `name()` and
  `Display`; `Triplet { dg, dat, msg }` named for messages ("DG_CONTROL/DAT_PARENT/MSG_OPENDSM").
- **Errors** (thiserror): `LoadError` (`NotFound`, `MissingEntry`, `Unsigned`, `WrongSigner { subject }`,
  `Machine`, `AlreadyRunning`); `Error` with `Failed { triplet, rc, source: Option<ConditionCode>, dsm:
  Option<ConditionCode> }` (each party asked once, the DSM first: asking clears a code), `Refused` for a
  container or block that doesn't hold up, `Timeout`, `Gone`, `Unexpected(Event)`; `Refused<S>`. Errors are
  `#[non_exhaustive]`; plain data isn't.
- **The thread** (`run`): enters a single-threaded apartment, registers and creates the hidden top-level
  window (its procedure `DefWindowProcW`), runs the closure, then destroys the window and leaves the
  apartment; refuses a thread already in a multithreaded apartment. The loop is the states' `next_event`:
  only the window's own messages are the crate's (matched by window, not by number); requests are kept while
  it waits; in `Delivery::Events` every other message is offered to `MSG_PROCESSEVENT` while a source is
  enabled, and in `Delivery::Callback` none is; a carrier of a source message isn't dispatched.
- **Capabilities:** `get`, `get_current`, `get_default`, `set`, `reset`, `query_support`, with `CapValue`
  for `TW_ONEVALUE`, `TW_ENUMERATION`, `TW_RANGE` and `TW_ARRAY` of the item types TWAIN defines (integers,
  `TW_BOOL`, `TW_FIX32`, `TW_FRAME`, the strings), every length checked against `GlobalSize` before it's read.
  `set` takes one value, an enumeration or an array.
- **Transfers and the end of a batch:** `Ready::image_info`; `Ready::native() -> (Dib, Transferred)`;
  `Ready::memory() -> MemoryTransfer` (`setup`, `preferred_size`, `next_buffer`, `end`); `ext_image_info(ids)`
  in state 7; `Transferred::end() -> Pending` (`More(Ready)` or `Done(Enabled)`, the count kept, 0xFFFF as
  "unknown"); `Ready::reset() -> Enabled`; `stop_feeder`; `TWRC_CANCEL` from a transfer keeps the state at 7.
- **For the seam** (`crates/twain-dsm.md`): per transfer, `DAT_IMAGEINFO` as TWAIN gives it; the pending count;
  extended image info as offered; each memory buffer's shape and bytes, and a native transfer's DIB block,
  not assembled; events with how they came (callback or `MSG_PROCESSEVENT`); condition codes from both
  parties.

### How the tests run

- **Each test that loads the DSM runs in a child process of its own** (rusty-fork, `timeout`), as `sane-dl`'s
  do: one `Twain` per process, and the DSM never unloaded; its `CAP_XFERCOUNT` given in the child's
  environment at spawn. Where the sample source isn't installed the test says so on stderr and passes;
  `TWAIN_DSM_REQUIRE_SAMPLE=1` (CI's job, and every check here) makes it a failure.
- **The spike's scenarios as tests:** listing; native and memory transfers in both delivery modes, page counts
  and pending counts; `compare` (the spike's `image` module in `tests/support/`); the cancel mid-page and before
  the enable; a state dropped right after the enable rolling back to state 1; the signature check (the installed
  DSM passes; Windows' `twain_32.dll`, signed by Microsoft, is `WrongSigner`; an unsigned DLL, `Unsigned`);
  `AlreadyRunning`; a `Remote` posting after its `run` ends (`Gone`).
- **Unit tests** need no DSM and run everywhere: containers both ways, extended image info items, codes,
  strings, `FIX32`, the triplets' names.

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

1. **The crate and its bindings.** The manifest, crate doc (the disclaimer), `ffi` generated and checked (the
   script and its report in `scratch/work/s0-8/`), the codes, the header contract, the pure decoders
   (containers, extended image info, strings) and their unit tests; the gates' devices-layer list gains
   `twain-dsm`; CI's TWAIN job fetches `twain.h` and runs the contract in both bitnesses. Slice 7's close-out
   pushed with it.
   **Done when:** the core; the script's check clean, its report saved; the contract passing in both
   bitnesses and seen to fail when a field's type is changed (a scratch edit, reverted); `cargo +1.89 check`
   clean, the MSRV measured as slice 1 measures it.
2. **The thread, loading, states 1 to 3.** `run`, `Twain`, `Remote`, `load` with the signature, `Loaded`,
   `Opened`, `sources`, the rollback scaffold and `stranded`; the DSM's memory functions inside.
   **Done when:** the core; listing in both bitnesses; the signature cases; `AlreadyRunning`; `Gone`; a
   compile-fail check that a state can't leave `run`'s closure (a `compile_fail` doctest) nor be sent to
   another thread.
3. **Sources and capabilities.** `Source`, `open_source`, `get` and the rest, the callback's registration,
   both parties' codes.
   **Done when:** the core; the spike's table of answers for the capabilities, through the API, in both
   bitnesses; a refusal reported with both codes (`DAT_CALLBACK2` given a null procedure, which the DSM
   refuses itself).
4. **Enabled, transfers, events, cancels.** `Enabled`, `Ready`, `Dib`, `MemoryTransfer`, `Transferred`,
   `Pending`, `next_event` in both deliveries, requests, `ext_image_info`, `stop_feeder`, `reset`, every
   `Drop` and the rollback; the spike's scenarios as tests.
   **Done when:** the core; every test above in both bitnesses, here and in CI; the rollback test from state
   6; a make-fail where a step down is refused, leaving the DSM loaded and `stranded` saying so.
5. **The spike retired; what it found, written down.** `crates/twain-spike` deleted (its `image` module in
   `tests/support/`); CI's job renamed `twain-dsm`, its recipe `twain-dsm-sample`; README rows;
   `crates/twain-dsm.md` (what was built, "For the seam" as built), the seam note where it changed, the
   milestone, `status.md`, `decisions.md`, this plan's "So far".
   **Done when:** the core; the workspace with no trace of the spike but its archived plan; every claim in the
   docs traced to a report or a line.

**Closing:** the code review (parallel reviewers), the fixes, CI green, approval.

## Decisions taken alone, for review

1. **The thread is the caller's, scoped by `run`** (question 1), not the crate's behind a handle.
2. **An owned chain of states, one source at a time** (question 2).
3. **The DSM never unloaded; containers decoded at once; the DIB an owner** (question 3).
4. **`Load::Path` is safe, its trust documented, and signatures are checked by default** (question 5);
   `Trust::Unchecked` exists for a DSM the TWAIN Working Group didn't sign.
5. **One `Twain` per process** (question 5).
6. **`RefCon` carries the callback's context** (question 6).
7. **`publish = false` at 0.0.0** until slice 9's backend has used the API; then made ready to publish, as
   `sane-dl` is in its slice.
8. **The bindings: every constant both sources define; the structs the crate and the containers use**, the
   rest when a slice needs them.
9. **The header contract compiles `twain.h` fetched at test time**, never committed.
10. **The spike's `image` module lives in the tests until slice 9.**

## Security, for review

1. **Loading the DSM runs every installed source's code** in the process (the DSM loads each at
   `MSG_OPENDSM`); the backend runs it in a worker (slice 9). The default load is the system folder's only;
   `Load::Path` trusts its path; the signer is checked unless the caller says not to.
2. **The header is fetched at test time** from the TWAIN Working Group's repository at a pinned commit, over
   TLS, by CI only; its hash pinned in the job.
3. **A driver's bytes are untrusted:** containers, extended image info, the DIB block and memory buffers are
   checked against their measured lengths and the caps before they're read.

## Risks

1. **The API meets its first real user in slice 9:** the backend may want shapes this plan didn't choose
   (question 1's (A), several sources). Kept at 0.0.0 meanwhile.
2. **The brand lifetime** may make the API awkward to hold in a struct across calls; the backend's loop is
   written inside `run`'s closure, which this plan assumes is enough.
3. **MSVC found by `cc` at test time**, on `windows-latest` and here, in both bitnesses: seen in step 1, or the
   contract moves to a `just` recipe run from a developer prompt.
4. **Child processes per test, in both bitnesses,** lengthen CI; the job's timeout grows if it must.
5. **Slice 1 lands on `master` meanwhile:** this branch rebases before each push; the justfile, CI and
   `status.md` are the likely conflicts.

## Plan review

Not yet.

## So far

Nothing yet.
