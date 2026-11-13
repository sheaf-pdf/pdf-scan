# S0 slice 9: scan-device's twain backend, 32-bit worker, and Windows hardening

Status: active, started 2026-11-11. Milestone: `milestones/s0-devices.md`, slice 9. Run on the Windows machine,
the same way as slices 7 and 8 (`archive/slices/s0-8-twain-dsm.md`): this plan, an adversarial review (below),
the steps (each committed and pushed on `s0-9-twain-backend`), parallel code reviews, a fix stage.

Paths: `docs/pm/` files from `docs/pm/`; the code's from the repository's root with a leading `/`; `scratch/`
from the root, local and never tracked.

## What it builds

1. **`twain-dsm` (`0.1.0-alpha.1`) finished for its first user and ready to package:**
   - Parent-window ownership for a driver's window (`Ui::ShownOwned(u64)` and `Source::settings_ui(self,
     owner: Option<u64>)`), validated inside `window.rs` against canonical 32-bit Win32 handle range,
     `IsWindow(hwnd) != 0`, and `GetWindowThreadProcessId(hwnd, null) != caller_tid` (preventing same-thread
     modal deadlocks), plus an `EnableWindow` guard on step-down/drop if a modal driver leaves `hParent`
     disabled, with no raw pointer in the public API.
   - `DG_IMAGE / DAT_IMAGELAYOUT` (`TW_IMAGELAYOUT` in `ffi.rs` + `header_contract.rs`, `ImageLayout`,
     `Source::image_layout`, `default_image_layout`, `set_image_layout`, `reset_image_layout`, and
     `Ready::image_layout`), plus `Source::status` and `Enabled::status` (`DG_CONTROL / DAT_STATUS / MSG_GET`)
     so unexpected `Event::CloseRequest` in state 5 can read the source's condition code.
   - Release-readiness polish from `review-2026-11-11.md`: `#![warn(missing_docs)]` (with `#[allow(missing_docs)]`
     on `pub mod ffi`), `#![cfg_attr(docsrs, feature(doc_cfg))]`, `[package.metadata.docs.rs] default-target =
     "x86_64-pc-windows-msvc"`, `include = ["/src", "/README.md", "/CHANGELOG.md"]`,
     `/crates/twain-dsm/README.md` and `/crates/twain-dsm/CHANGELOG.md`, version `0.1.0-alpha.1` (`publish`
     enabled, dry-run recipe `just twain-dsm-package` and CI job), and `twain-dsm` checked by `public-api` on
     Windows (`TEST-14`).
2. **`scan-device`'s `twain` backend (`scan_device::twain::TwainBackend` behind `feature = "twain"`):**
   - Pure safe Rust (`#![forbid(unsafe_code)]` kept on `scan-device`), building on every OS: on Windows it
     drives `twain-dsm` over a process-wide lazy STA `Session`; off Windows `list` and `open` return
     `ScanError::NotInstalled { what: "TWAIN".into() }`, just as `SaneBackend` does off Unix.
   - Ids `twain32:<ProductName>` and `twain64:<ProductName>` according to `target_pointer_width`, checked before
     loading the DSM (`BadId` on wrong prefix, mismatched `Driver::Twain { bits }`, empty name, NUL, or >32
     bytes); `open` lists the DSM first and matches `(ProductName, Manufacturer, ProductFamily)`.
   - Auto-selected delivery (`Delivery::Callback` for TWAIN 2 sources with `DF_DS2`, `Delivery::Events` for
     TWAIN 1.x sources, overridable via `TwainBackend::with_delivery`), capabilities (`Caps` / `SourceCaps`),
     settings clamping & readback (`Applied` / `Clamped` with `0.005` mm paper tolerance), explicit per-batch
     reset of sticky driver processing (`ICAP_AUTODISCARDBLANKPAGES = TWBP_DISABLE`, `ICAP_AUTOMATICROTATE =
     false`, `CAP_INDICATORS = false`, and mutual exclusion of `ICAP_AUTOMATICBORDERDETECTION` /
     `ICAP_UNDEFINEDIMAGESIZE` vs `DAT_IMAGELAYOUT`), both transfer mechanisms (`TWSX_MEMORY` default with
     `TWSX_NATIVE` selectable/fallback), strip/tile + undefined-size (`width == -1` / `length == -1`) + DIB
     pixel assembly into `RawImage` (`Bw1`, `Gray8`, `Gray16`, `Rgb8`, `Rgb16`), `TWEI_PAGESIDE`
     (`SideKnown::Reported` vs `Inferred`), sheet-granular cancel, condition-code mapping to `ScanError`, and
     `Device::settings_window`.
   - `TEST-12`: `scan-device --lib` unit-tested on `i686-pc-windows-msvc` as well as `x86_64-pc-windows-msvc`,
     plus integration tests scanning 10 images through the TWAIN sample source in both bitnesses.
3. **`scan-worker` Windows hardening, dual-bitness TWAIN list merge, and carried S0 items:**
   - **No unconditional `sane-dl` runtime dependency** (`review-2026-11-11.md`): inline the ~40-line
     `SANE_CONFIG_DIR` file layout in `/crates/scan-worker/src/client/spawn.rs` (setting `LC_ALL=C` on SANE
     workers unless `WorkerCommand::env` sets it), keep `sane-dl` only in `[target.'cfg(unix)'.dev-dependencies]`
     for `tests/sane_worker.rs`, and add optional `twain = ["scan-device/twain"]`.
   - **Protocol output isolated from driver `stdout` writes** (`milestones/s0-devices.md`, PaperStream IP notes):
     `scan_worker::serve` duplicates the protocol output stream to a private, non-inheritable descriptor/handle
     at startup (before any driver DLL is loaded) and redirects standard output (`STD_OUTPUT_HANDLE` + UCRT fd 1
     on Windows; fd 1 on Unix) to `NUL` / `/dev/null`.
   - **Windows Job Object (`FEAT-14`):** each spawned worker on Windows is placed in a Job Object with
     `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` (tolerating fast-exit `ERROR_ACCESS_DENIED` when the child has already
     exited), terminated via `TerminateJobObject` before reaping on kill/timeout so driver helper processes die
     immediately with the worker and never outlive the client.
   - **`StopHandle` (`FEAT-17`) and `WindowOwner` protection:** `RemoteDevice::stop_handle(&self) -> StopHandle`
     (`Send + Sync + Clone`) shares `Arc<Mutex<Option<LiveProcess>>>` + `Arc<AtomicBool>` with `Worker` (zero
     Unix PID-reuse race, unblocking full `SyncSender<Event>` sends too), returning `Error::Aborted` on the
     aborted call and `Error::Ended` thereafter; on Windows, `WindowGuard` rejects same-thread `HWND` self-calls
     before blocking and re-enables `HWND` via `EnableWindow(hwnd, 1)` if a `settings_window(Some(owner))` call
     ends in `Err(_)`.
   - **32-bit + 64-bit TWAIN list merge and 32-bit worker in CI:** `Client::list_twain(deadline) ->
     Result<TwainList, Error>` queries registered TWAIN bitnesses concurrently via `std::thread::scope` (64-bit
     ordered before 32-bit, deduplicating dual-installed sources in favour of 64-bit, keeping 32-bit-only
     drivers, ignoring single-side `NotInstalled`, and surfacing a single-side crash/timeout in
     `TwainList::other_failed` while still returning the working bitness's scanners), tested end-to-end with
     both `x86_64` and `i686` workers in CI.

Not built here: `scan-device`'s `wia` backend (slice 10); encoded TWAIN transfers (`encodings` stays empty in
`SourceCaps` and clamped when asked, just as in `SaneBackend`); freezing `0.1.0` on `scan-device` and
`scan-worker` (slice 11).

## Taken as it starts

- **PaperStream IP fi-6130 findings folded in** (`stash@{0}` applied to `milestones/s0-devices.md`): driver
  `stdout` chatter, sticky non-default settings at open (`CAP_DUPLEXENABLED`, `ICAP_AUTODISCARDBLANKPAGES`,
  `ICAP_AUTOMATICBORDERDETECTION`, `ICAP_AUTOMATICROTATE`), 32-bit-only installation under `twain_32`, and
  `TWEI_PAGESIDE` reporting.
- **No breaking change to `scan-device`'s model:** every TWAIN capability, paper mode, page side, error, and
  window owner already fits `scan-device`'s existing types; `ICAP_AUTOMATICROTATE` is turned off (`false`) in
  the backend so page orientation belongs to `scan-cleanup`.

## The API, explored

**1. Parent window handle in `twain-dsm` (no raw pointers, no UI-thread self-deadlock, no stranded disabled window).**
- `Ui::ShownOwned(u64)` and `Source::settings_ui(self, owner: Option<u64>)`, with `Error::InvalidWindow { hwnd:
  u64 }`.
- Canonical Win32 handle check: accept `1..=u32::MAX as u64` and sign-extended `0xFFFF_FFFF_8000_0000..=u64::MAX`,
  cast `hwnd as u32 as usize as HWND`, check `IsWindow(hwnd) != 0`, and check `GetWindowThreadProcessId(hwnd,
  null) != caller_tid` (where `caller_tid` is the blocking caller thread: the TWAIN thread in direct
  `twain-dsm`, or the caller thread of `TwainDevice::settings_window` / `RemoteDevice::settings_window`),
  failing fast with `Error::InvalidWindow` / `ScanError::Driver` rather than deadlocking on synchronous Win32
  cross-thread messages (`SetParent` / `EnableWindow`).
- Both `twain-dsm` (when disabling/dropping an `Enabled` started with an external owner) and `scan-worker`
  (`WindowGuard` when `settings_window` returns `Err(_)`) record `IsWindowEnabled(hwnd)` beforehand and restore
  `EnableWindow(hwnd, 1)` if a modal driver disabled the window and failed to re-enable it.

**2. `DAT_IMAGELAYOUT` and `DAT_STATUS` on `twain-dsm`.**
- Per TWAIN 2.5 (`DG_IMAGE / DAT_IMAGELAYOUT`), only `MSG_GET`, `MSG_GETDEFAULT`, `MSG_RESET`, and `MSG_SET` are
  valid (`MSG_GETCURRENT` is *not* a `DAT_IMAGELAYOUT` message and triggers `TWCC_BADPROTOCOL` on strict
  drivers!).
- Methods on `Source`: `image_layout` (`MSG_GET`), `default_image_layout` (`MSG_GETDEFAULT`),
  `reset_image_layout` (`MSG_RESET`), `set_image_layout(frame: Frame)` (`MSG_SET` with `TWON_DONTCARE32` for
  `DocumentNumber`, `PageNumber`, `FrameNumber`; rejects non-finite or `left >= right || top >= bottom` frames
  up front before calling the driver), and `status(&self) -> Option<ConditionCode>`.
- Methods on `Enabled`: `status(&self) -> Option<ConditionCode>`.
- Methods on `Ready`: `image_layout(&self) -> Result<ImageLayout, Error>` (`MSG_GET` in state 6).
- `header_contract.rs` adds `impl FieldKind for ffi::TW_FRAME` and verifies `TW_IMAGELAYOUT` against `twain.h`
  in both 64-bit and 32-bit.

**3. `TwainBackend` & `TwainDevice` over one process-wide lazy STA `Session`.**
- Because `twain_dsm::run` (`RunLock`) and `Dsm` (`LOADED`) are process-wide singletons, `session.rs` maintains
  **one process-wide `Mutex<Weak<Session>>`**, shut down and joined when the last `TwainBackend` and
  `TwainDevice` drop (unless poisoned by a hung call past its deadline).
- On `list`/`open`, `Session` validates the backend's requested `Load` (`System` vs `Path`) against the loaded
  DSM path (calling `twain.load(which)` when unloaded so `LoadError::Elsewhere` / `NotFound` maps cleanly).
- `Session` tracks `Mutex<SessionState>` (`Idle | DeviceOpen { busy: bool } | Poisoned`) *before* sending on the
  STA command channel:
  - If a `TwainDevice` is already open, `backend.open()` returns `Err(ScanError::Busy)` immediately without
    blocking or poisoning the STA thread.
  - If `DeviceOpen { busy: true }` (e.g. a long `scan` or `settings_window` is running on the STA thread),
    `backend.list()` posts a listing request via `Remote` so `Enabled::next_event`'s `Next::Request` answers
    `source.opened().sources()` on the STA thread; if `deadline` elapses while waiting during `busy: true`,
    `list` returns `ScanError::TimedOut` *without* poisoning the active device! `Session` is poisoned only when
    the STA thread itself hangs inside an uncompleted `MSG_OPENDSM` / `MSG_OPENDS` / `sources()` call past its
    `deadline`.

**4. Isolating `scan-worker`'s protocol output from driver `stdout` writes.**
- Inside `scan_worker::serve()` at process startup, before any backend loads a driver:
  - **Windows:** verify `orig = GetStdHandle(STD_OUTPUT_HANDLE)` is neither null nor `INVALID_HANDLE_VALUE`;
    `DuplicateHandle(..., orig, ..., bInheritHandle = FALSE, DUPLICATE_SAME_ACCESS)` -> `OwnedHandle` -> `File`
    for `serve::run`; open `NUL` (`CreateFileW(L"NUL", GENERIC_WRITE, FILE_SHARE_READ | FILE_SHARE_WRITE, null,
    OPEN_EXISTING, 0, null)`), hand ownership to UCRT via `nul_fd = _open_osfhandle(nul, 0)`, `_dup2(nul_fd, 1)`,
    `_close(nul_fd)`, and then call `SetStdHandle(STD_OUTPUT_HANDLE, _get_osfhandle(1) as HANDLE)` so fd 1 owns
    the live handle and `STD_OUTPUT_HANDLE` points to that exact live handle (never a closed/recycled one!).
  - **Unix:** duplicate fd 1 with `FD_CLOEXEC` (`OwnedFd::try_clone`) into `File` for `serve::run`; open
    `/dev/null` (`O_WRONLY | O_CLOEXEC`) and `libc::dup2(devnull, 1)`.

**5. Windows Job Object (`FEAT-14`) and `StopHandle` (`FEAT-17`).**
- **Job Object:** `CreateJobObjectW` + `SetInformationJobObject(..., JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE)` +
  `AssignProcessToJobObject`. If `AssignProcessToJobObject` fails with `ERROR_ACCESS_DENIED (5)` and
  `child.try_wait()` shows the child already exited (`STATUS_PROCESS_IS_TERMINATING`), `spawn` still succeeds so
  `Worker` reads stdout/stderr and reports `Error::Crashed` / `Error::Protocol` accurately. On `Ending::Kill` or
  grace timeout, `job.terminate()` runs *before* `reap()` and pipe `DRAIN`.
- **`StopHandle`:** shares `Arc<Mutex<Option<LiveProcess>>>` (`Child` + `#[cfg(windows)] Job`) and
  `aborted: Arc<AtomicBool>` with `Worker`. `stop.stop()` sets `aborted = true` and under the mutex kills `Child`
  (and terminates `Job` on Windows) only if not yet reaped—zero Unix `unsafe` kill FFI and zero PID-reuse hazard.
  `Worker::scan` sends events with a `try_send` loop (sleeping 10 ms on `Full` while checking `is_aborted()`) so
  `stop.stop()` never deadlocks on a full caller channel. An aborted active call (or the first call after an idle
  `stop.stop()`) returns `Error::Aborted`; subsequent calls return `Error::Ended`, and `Drop` / `close()` on an
  idle-aborted `RemoteDevice` reaps immediately via `Ending::Kill` without sending `"quit"`.

**6. Dual-bitness TWAIN list merge (`Client::list_twain`).**
- `Client::list_twain(&self, deadline: Duration) -> Result<TwainList, Error>` runs registered `Driver::Twain {
  bits: 64 }` and `Driver::Twain { bits: 32 }` workers **concurrently** inside `std::thread::scope`, each given
  the full `deadline`.
- Merges 64-bit devices before 32-bit, deduplicating dual-installed sources by `(maker, model, name)` in favour
  of 64-bit while preserving 32-bit-only drivers.
- Single-side `ScanError::NotInstalled` is ignored cleanly; a single-side crash/timeout when the other bitness
  succeeded is surfaced in `TwainList::other_failed: Option<(u8, Error)>` while keeping the working bitness's
  devices!

## The design

### Module layout in `/crates/scan-device/src/twain/`

```
crates/scan-device/src/twain/
  mod.rs        TwainBackend (new, with_dsm_path, with_delivery, with_native_transfer), TwainDevice, non-Windows stub
  session.rs    (cfg(windows)) Process-wide lazy STA thread over twain_dsm::run, SessionState, Remote wakeup
  id.rs         twain32:/twain64: prefix, BadId validation (<=32 bytes, no NUL, matching Driver::Twain { bits })
  error.rs      twain_dsm::{Error, LoadError, RunError, ConditionCode} -> ScanError
  caps.rs       (cfg(windows)) Units conversion, Flatbed/Feeder, CAP_DUPLEX, ICAP_XRESOLUTION, ICAP_PIXELTYPE +
                ICAP_BITDEPTH (8|24 and 16|48, 16-bit omitted on TWSX_NATIVE), ICAP_PHYSICAL*, paper_auto
  settings.rs   (cfg(windows)) Ordered negotiation + clamping + readback -> Applied (0.005 mm tolerance),
                mutual exclusion of border-detect/undefined-size vs DAT_IMAGELAYOUT, sticky processing reset
  batch.rs      (cfg(windows)) Pre-enable CAP_PAPERDETECTABLE+CAP_FEEDERLOADED, 100ms pump + sheet-granular
                cancel, memory & native transfers, CloseRequest DAT_STATUS check, Flatbed reset, TWEI_PAGESIDE
  pixels.rs     Pure DIB decoder + strip/tile + undefined-size (width/length == -1) Assembler -> RawImage
```

### Key edge cases handled in `scan-device::twain`

1. **Units conversion (`ICAP_UNITS`):** sets `TWUN_INCHES` first; if read-only/refused, converts lengths and
   `DAT_IMAGEINFO` resolutions through active `ICAP_UNITS` (`INCHES`, `CENTIMETERS`, `PICAS`, `POINTS`, `TWIPS`,
   `PIXELS`) so `SizeMm` is always millimetres and `RawPage::dpi` is always dots per inch.
2. **16-bit vs `TWSX_NATIVE`:** Windows `BI_RGB` DIBs have no 16-bit gray or 48-bit RGB, so when `TWSX_NATIVE`
   is forced (or `TWSX_MEMORY` is unsupported by the source), `caps()` omits `Gray16`/`Rgb16` and `settings`
   clamps them to `Gray8`/`Rgb8`.
3. **`Paper::Size` vs `Paper::Auto`:**
   - `Paper::Size(size)`: clamps non-positive/non-finite `SizeMm` to `max_area` and valid `size` to
     `[min_area, max_area]`; explicitly disables `ICAP_AUTOMATICBORDERDETECTION = false`, `ICAP_AUTOSIZE =
     TWAS_NONE`, and `ICAP_UNDEFINEDIMAGESIZE = false`; sets `Frame { 0, 0, width, height }` via
     `set_image_layout`; compares readback `image_layout()` within `SAME_SIZE_MM = 0.005` mm.
   - `Paper::Auto`: if supported, resets `DAT_IMAGELAYOUT` (`reset_image_layout`, or full physical frame) first
     so no leftover crop clips border detection, then enables `ICAP_AUTOMATICBORDERDETECTION = true` and
     `ICAP_UNDEFINEDIMAGESIZE = true` (or `ICAP_AUTOSIZE = TWAS_AUTO`); if unsupported, clamps to
     `Paper::Size(max_area)` (`Clamped::Paper(Paper::Auto)`).
4. **Pixel assembly (`pixels.rs`):**
   - Supports `width == -1` and/or `length == -1` in state 6 `DAT_IMAGEINFO` for `TWSX_MEMORY` (accumulating
     contiguous strips dynamically under `RawImage::data_len` / `MAX_PIXELS` and cross-checking state 7
     `image_info()`).
   - Uses $O(1)$ `next_row` tracking for normal top-down strips, lazily allocating a word-masked `Vec<u64>`
     bitmap only if out-of-order or sub-row tiles arrive.
   - Applies `Polarity::ZeroWhite` (`TWPF_VANILLA`) inversion *only* to `Bw1 | Gray8 | Gray16` (never `Rgb8 |
     Rgb16`), clearing `Bw1` padding bits *after* inversion.
5. **Unexpected `Event::CloseRequest` & `Flatbed` pending transfers (`batch.rs`):**
   - On `Event::CloseRequest` when `!enabled.forced_ui()`, checks `enabled.status()` and (on `Feeder`)
     `CAP_FEEDERLOADED` after stepping down so `TWCC_NOMEDIA` / empty feeder / `TWCC_PAPERJAM` /
     `TWCC_INTERLOCK` report the real `ScanError` (or `Done::FeederEmpty` after pages, restoring `last_bytes` on
     `progress`) instead of false `Done::Cancelled`.
   - On `Source::Flatbed`, after 1 page, if `end()` returns `Pending::More(ready, _)`, calls
     `ready.reset()?.disable()?` and returns `Ok(Done::OnePage)`.
   - Checks `u32::MAX` sheet-number overflow before starting a sheet (`ScanError::Driver`), matching `sane/batch.rs`.

## Steps

Each step leaves every workspace crate building, clippy-clean, formatted, gate-clean (`just gates`), and tested,
and is committed and pushed on `s0-9-twain-backend`.

1. **Step 1 — `twain-dsm` `0.1.0-alpha.1`: owner window, `DAT_IMAGELAYOUT`, `status()`, docs & packaging.**
   - Add `TW_IMAGELAYOUT` (`ffi.rs` + `header_contract.rs` with `FieldKind for TW_FRAME`), `ImageLayout` +
     `Source` / `Ready` layout methods (`MSG_GET` / `MSG_GETDEFAULT` / `MSG_RESET` / `MSG_SET`), `Source::status`
     & `Enabled::status`, `Ui::ShownOwned(u64)`, `Source::settings_ui(self, owner: Option<u64>)`,
     `Error::InvalidWindow { hwnd: u64 }`, same-thread deadlock guard, and `EnableWindow` restore guard.
   - Enable `#![warn(missing_docs)]` (`#[allow(missing_docs)]` on `pub mod ffi`), document every public item,
     add `#![cfg_attr(docsrs, feature(doc_cfg))]`, `[package.metadata.docs.rs] default-target =
     "x86_64-pc-windows-msvc"`, and `include = ["/src", "/README.md", "/CHANGELOG.md"]`, write
     `/crates/twain-dsm/README.md` and `/crates/twain-dsm/CHANGELOG.md`, bump version to `0.1.0-alpha.1`
     (`publish` enabled), add `just twain-dsm-package`, and add `just public-api-windows` (`TEST-14`).
2. **Step 2 — `scan-device::twain` pure layers: ids, errors, DIB + strip/tile/undefined-size pixels, caps & settings.**
   - Wire optional `twain = ["dep:twain-dsm"]` in `/crates/scan-device/Cargo.toml` (`#![forbid(unsafe_code)]`
     kept), non-Windows stub, `id.rs`, `error.rs`, `pixels.rs`, `caps.rs`, and `settings.rs`, with thorough unit
     tests running in both 64-bit and 32-bit (`TEST-12`).
3. **Step 3 — `scan-device::twain` STA session, batch transfers, cancel, settings window & dual-bitness sample tests.**
   - Build `session.rs`, `batch.rs`, and `TwainBackend` / `TwainDevice`, plus integration tests in
     `/crates/scan-device/tests/twain.rs` exercising 10-page batches, memory vs native pixel parity, custom
     `Paper::Size` via `DAT_IMAGELAYOUT`, clamping, cancel, same-thread owner refusal, and error mapping in both
     `x86_64-pc-windows-msvc` and `i686-pc-windows-msvc`.
4. **Step 4 — `scan-worker`: drop runtime `sane-dl`, isolate `stdout`, Windows Job Object (`FEAT-14`), `StopHandle` (`FEAT-17`), `WindowGuard`, & 32/64-bit `list_twain`.**
   - Inline SANE `ConfigDir` + `LC_ALL=C` in `client/spawn.rs`, move `sane-dl` to
     `[target.'cfg(unix)'.dev-dependencies]`, inline `"SANE_CONFIG_DIR"` in `scan-worker-fake`, and add
     `twain = ["scan-device/twain"]`.
   - Switch `/crates/scan-worker/src/lib.rs` to `#![deny(unsafe_code)]` with `#[cfg(any(unix, windows))]
     #[allow(unsafe_code)] mod sys;` (`windows-sys = "0.61"` on Windows, `libc = "0.2"` on Unix): `Job`,
     `isolate_stdout`, `WindowGuard`, `StopHandle` + `Error::Aborted`, `Client::list_twain` + `TwainList`, TWAIN
     mode in `scan-worker-fake` (or `scan-worker-twain`), and end-to-end tests covering 32-bit + 64-bit workers
     from a 64-bit client, `TWAIN_SHOWCONSOLEMESSAGES=1` + noisy `stdout`, grandchild termination, `StopHandle`
     (including full `SyncSender` channel), and disabled-owner recovery.
5. **Step 5 — CI workflow, `justfile`, and PM / crate documentation sync.**
   - Update `/.github/workflows/ci.yml` and `/justfile` (`twain-dsm-package`, Windows `public-api` `TEST-14`,
     `TEST-12` 32-bit `scan-device` unit tests, 64-bit + 32-bit `scan-device --features twain` and `scan-worker`
     dual-bitness TWAIN jobs).
   - Sync `docs/pm/{status,plan}.md`, `docs/pm/milestones/s0-devices.md`, `docs/pm/slices/README.md` (archiving
     `s0-4-scan-worker.md`), `docs/pm/crates/{scan-device-seam,scan-device,scan-worker,twain-dsm}.md`,
     `docs/pm/backlog/{features,tests}.md` (`archive/backlog-done.md`), and root `/README.md`.

## Plan review

Reviewed 2026-11-11 by a fresh adversarial subagent against the codebase, `twain.h` 2.5, `Twain_DS_sample01`,
and empirical Win32/UCRT/rustdoc probes (`scratch/work/s0-9/plan.before-review.md` holds the draft before these
changes):

- **Accepted, HIGH (H1 — UCRT `_open_osfhandle` handle ownership in `isolate_stdout`):** `_open_osfhandle`
  transfers `HANDLE` ownership to the CRT fd and `_close(nul_fd)` closes the original handle (`_dup2` duplicates
  a separate handle into fd 1). Fixed to call `SetStdHandle(STD_OUTPUT_HANDLE, _get_osfhandle(1) as HANDLE)`
  *after* `_close(nul_fd)`, and check `GetStdHandle(STD_OUTPUT_HANDLE)` for null/`INVALID_HANDLE_VALUE` first.
- **Accepted, HIGH (H2 — Windows Job Object fast-exit `ERROR_ACCESS_DENIED` race & `terminate()` order):**
  empirically confirmed that `AssignProcessToJobObject` returns `ERROR_ACCESS_DENIED` if the worker already
  exited (`STATUS_PROCESS_IS_TERMINATING`). Fixed to treat `ERROR_ACCESS_DENIED` with `child.try_wait() ==
  Ok(Some(_))` as success so crash/protocol reporting works, and to call `job.terminate()` *before* `reap()` on
  kill/timeout.
- **Accepted, HIGH (H3 — `StopHandle` Unix PID-reuse hazard, full-channel deadlock, and post-abort state):**
  fixed by sharing `Arc<Mutex<Option<LiveProcess>>>` + `Arc<AtomicBool>` (using `Child::kill` under the mutex
  with zero PID-reuse race), replacing blocking `events.send` in `Worker::scan` with a `try_send` poll checking
  `is_aborted()`, returning `Error::Aborted` on the aborted call then `Error::Ended` thereafter, and reaping
  idle-aborted workers immediately via `Ending::Kill` on `close`/`Drop`.
- **Accepted, HIGH (H4 — `scan-worker` `sane-dl` dev-dependency & Unix `sys` cfg):** kept `sane-dl` under
  `[target.'cfg(unix)'.dev-dependencies]` for `tests/sane_worker.rs`, inlined `"SANE_CONFIG_DIR"` in
  `scan-worker-fake`, and enabled `mod sys` on `any(unix, windows)` with `libc = "0.2"` on Unix.
- **Accepted, HIGH (H5 — `DAT_IMAGELAYOUT` `MSG_GET` vs invalid `MSG_GETCURRENT`, plus `FieldKind` for
  `TW_FRAME`):** dropped `MSG_GETCURRENT` on `DAT_IMAGELAYOUT` (using `MSG_GET` in both state 4 and state 6 per
  TWAIN 2.5), added `FieldKind` for `TW_FRAME` in `header_contract.rs`, and rejected empty/inverted frames up
  front in `set_image_layout`.
- **Accepted, HIGH (H6 — Auto-selected `Delivery` for TWAIN 1.x vs 2.x):** `TwainBackend::new()` now selects
  `Delivery::Callback` when `SourceId::is_twain2()` and `Delivery::Events` otherwise, unless overridden by
  `with_delivery`.
- **Accepted, HIGH (H7 — Process-wide `Session` singleton & no false poisoning during busy device calls):**
  matched `Session` to `twain-dsm`'s process-wide `RunLock` and `Loaded::path()`, returning `Busy` on concurrent
  `open` and routing concurrent `list` via `Remote` without poisoning an active device.
- **Accepted, HIGH (H8 — Same-thread `WindowOwner` self-deadlock & in-process `EnableWindow` guard):**
  empirically confirmed synchronous cross-thread Win32 messages deadlock if the caller blocks the thread owning
  `hwnd`; both `twain-dsm` and `scan-worker` now reject same-thread `hwnd` before blocking and restore
  `EnableWindow` on failure/drop.
- **Accepted, HIGH (H9 — `Paper::Size` vs `Paper::Auto` sticky capabilities & `0.005` mm tolerance):**
  explicitly disable border-detection/auto-size/undefined-size on `Paper::Size`, reset `DAT_IMAGELAYOUT` before
  enabling `Paper::Auto`, clamp invalid `SizeMm` to `max_area`, and apply `SAME_SIZE_MM = 0.005`.
- **Accepted, HIGH (H10 — Windows `public-api` target & `twain-dsm` `include` list):** added `include = ["/src",
  "/README.md", "/CHANGELOG.md"]` to `twain-dsm/Cargo.toml`, tailored `twain-dsm-package`, and added Windows CI
  execution of `public-api` on `twain-dsm` (`TEST-14`).
- **Accepted, MEDIUM & LOW (M1–M6, L1–L3):** `width == -1` as well as `length == -1` in `pixels.rs`; active-unit
  conversion for `DAT_IMAGEINFO` DPI; 16-bit omitted on `TWSX_NATIVE`, `TWPF_VANILLA` restricted to `Bw1 |
  Gray8 | Gray16`, and $O(1)$ `next_row` strip coverage; `DAT_STATUS` + `CAP_FEEDERLOADED` inspection on
  unexpected `Event::CloseRequest`; `ready.reset()?.disable()?` after 1 page on `Flatbed`; parallel
  `std::thread::scope` execution + `TwainList { devices, other_failed }` for `Client::list_twain`; edge tests for
  `0xFFFF` / `u64::MAX` `HWND`s, `ScanError::Unsupported` on empty formats/resolutions, and `TWEI_PAGESIDE`
  `TWCS_BOTH`.

## Decisions taken alone, for review

1. **`ICAP_AUTOMATICROTATE` turned off in `TwainBackend` (no new field on `Settings`).**
2. **Default transfer mechanism is `TWSX_MEMORY` (with `TwainBackend::with_native_transfer()` and automatic
   fallback to `TWSX_NATIVE` if a source refuses `TWSX_MEMORY`).**
3. **One process-wide lazy STA `Session` in `TwainBackend`.**
4. **`Error::Aborted` on `scan_worker::Error` for `StopHandle::stop()` (followed by `Error::Ended` on later
   calls).**
5. **`Client::list_twain(deadline) -> Result<TwainList, Error>` running 64-bit and 32-bit listings concurrently.**

## Security, for review

- **Untrusted driver inputs:** every DIB header, palette, row stride, memory strip offset/size, undefined-size
  growth, extended-info item, resolution (`NaN`/infinity/`<= 0`), physical dimension, and string from a `.ds` is
  validated in checked `u64`/`usize` arithmetic before allocation or slice indexing; `scan-device` retains
  `#![forbid(unsafe_code)]`.
- **Untrusted device ids:** `TwainBackend::open` validates `DeviceInfo` *before* loading `TWAINDSM.dll` and
  matches `(ProductName, Manufacturer, ProductFamily)` against `Opened::sources()`.
- **Win32 `HWND` validation:** `u64` owner handles are checked for canonical 32-bit Win32 representation,
  `IsWindow(hwnd) != 0`, and non-owning caller thread before passing to TWAIN or `EnableWindow`.
- **Handle inheritance & Job Object containment:** `scan_worker::serve`'s duplicated protocol output handle uses
  `bInheritHandle = FALSE` (`FD_CLOEXEC` on Unix), fd 1 / `STD_OUTPUT_HANDLE` points to `NUL` / `/dev/null`, and
  every Windows worker runs inside a `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` Job Object.

## Risks

- **`Twain_DS_sample01` Qt UI (`QT_UI.cpp:170`) with cross-process `hParent`:** Qt calls `SetWindowLong(...,
  WS_CHILD)` + `SetParent` on `hParent` when non-null. Our integration tests verify both `None` (hidden window)
  and `Some(owner)` (with a real Win32 top-level window pumped on its own thread) so cross-thread and
  cross-process parenting + `EnableWindow` recovery are proven empirically.

## So far

- **Step 1 (`2eef030`, pushed):** Prepared `twain-dsm` `0.1.0-alpha.1` with `DG_IMAGE / DAT_IMAGELAYOUT`
  (`TW_IMAGELAYOUT` in `ffi.rs` and `header_contract.rs` verified in 64- and 32-bit, `ImageLayout`, `Source` and
  `Ready` layout methods), `DG_CONTROL / DAT_STATUS` (`Source::status`, `Enabled::status`), caller parent windows
  (`Ui::ShownOwned(u64)`, `Source::settings_ui(Option<u64>)`, `Error::InvalidWindow { hwnd }`,
  `window_belongs_to_current_thread`, and `EnableWindow` restore guard), `#![warn(missing_docs)]` (0 warnings),
  `docs.rs` `x86_64-pc-windows-msvc` target metadata, `README.md`, `CHANGELOG.md`, `just twain-dsm-package`, and
  Windows `twain-dsm:3000` in `just public-api`.
- **Step 2 (`65a878a`, pushed):** Built the pure cross-platform layers of `scan_device::twain` behind optional
  `twain = ["dep:twain-dsm"]` (`#![forbid(unsafe_code)]` preserved): `id.rs`, `error.rs`, `pixels.rs` (1/8/24-bit
  DIB decoder + 1/8/16/24/48-bit strip, sub-row tile, and undefined `width`/`length == -1` memory assembler with
  $O(1)$ top-down fast path and lazy `Vec<u64>` bitmap promotion), `caps.rs`, `settings.rs`, and non-Windows stub;
  all 103 `scan-device --lib` unit tests pass on both `x86_64-pc-windows-msvc` and `i686-pc-windows-msvc`
  (`TEST-12`).
- **Step 3 (`45348a8`, pushed):** Built `session.rs` (process-wide lazy STA `Session` matching `twain-dsm`'s
  `RunLock` and `Loaded::path()`, concurrent `list` via `Remote` without poisoning an active device), `batch.rs`,
  and `TwainBackend` / `TwainDevice`, plus 9 end-to-end sample-source integration tests in
  `/crates/scan-device/tests/twain.rs` (10-page batches, memory vs native pixel parity, custom `Paper::Size` via
  `DAT_IMAGELAYOUT`, clamping, cancel, same-thread owner refusal, and error mapping) passing in both
  `x86_64-pc-windows-msvc` and `i686-pc-windows-msvc`. Note: `Twain_DS_sample01`'s `ProductName` (`DeviceInfo.name`)
  is `"TWAIN2 Software Scanner"`, `Manufacturer` (`maker`) is `"TWAIN Working Group"`, and `ProductFamily` (`model`)
  is `"Software Scan"`.
- **Step 4 (`514006b`, pushed):** Hardened `scan-worker` and added 32/64-bit TWAIN support:
  - Dropped `sane-dl` and `libloading` completely from runtime `[dependencies]` (inlining `SANE_CONFIG_DIR` +
    default `LC_ALL=C` in `client/spawn.rs`; `sane-dl` stays in `[dev-dependencies]` so `tests/support/sane.rs`
    and `tests/sane_worker.rs` continue compiling cleanly on Windows too, skipping at runtime when SANE isn't
    installed).
  - Added `src/sys.rs` (`#![deny(unsafe_code)]` at crate root, `#[allow(unsafe_code)]` confined to `sys`): Unix
    and Windows `isolate_stdout()` (duplicating the protocol stream non-inheritably and redirecting fd 1 +
    `STD_OUTPUT_HANDLE` to `/dev/null` / `NUL` before any driver loads), Windows `Job`
    (`JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` + `TerminateJobObject`, `FEAT-14`), and Windows `WindowGuard`.
  - Added `RemoteDevice::stop_handle -> StopHandle` (`FEAT-17`) + `Error::Aborted` (unblocking full
    `SyncSender<Event>` channels too) and concurrent `Client::list_twain -> TwainList`, verified with 51 unit
    tests (64- and 32-bit), `tests/stop_handle.rs` (8 tests), and `tests/twain_worker.rs` (5 tests, including a
    64-bit client driving both 64-bit and `i686` sample workers with `TWAIN_SHOWCONSOLEMESSAGES=1` and
    `--noisy-stdout`).
- **Step 5:** Wired `scan-device-twain-test`, `scan-device-clippy`, `scan-worker-twain-test`, and
  `scan-worker-clippy` into `/justfile` and `/.github/workflows/ci.yml` (`twain-dsm` 64/32-bit job,
  `twain-dsm-package`, Windows `public-api` `TEST-14` reading 3,665 `twain-dsm` items with 0 unsafe failures, and
  Ubuntu+Windows MSRV 1.89 matrices for `scan-device` and `scan-worker`), archived `s0-4-scan-worker.md`, moved
  `FEAT-14`, `FEAT-17`, `TEST-12`, and `TEST-14` to `archive/backlog-done.md`, and synced crate and PM docs.
