# S0 slice 7: the TWAIN spike, end to end

Status: running, on branch `s0-7-twain-spike` (a draft pull request), from 2026-11-08; steps 1 and 2 done. Milestone:
`milestones/s0-devices.md`, slice 7. Run on the Windows machine, while slice 1 runs on Linux.

Paths: `docs/pm/` files from `docs/pm/`, as everywhere here; the code's from the repository's root, written
with a leading `/` (`/crates/twain-spike/`, `/justfile`); `scratch/` from the root, local and never tracked.

## What it builds

TWAIN driven end to end through the TWAIN Working Group's sample data source, in both bitnesses, with a
hidden window: the DSM loaded and walked from state 1 to 7 and back, native and memory transfers, each
transfer's image info, the pending-transfer count, whatever extended image info the source offers, and a
cancel posted from another thread. A throwaway program, `twain-spike`, does it and checks what it sees; a CI
job on `windows-latest` installs the sample source and runs it, so CI scans through TWAIN from then on. What
it finds answers the seam note's TWAIN unknowns (`crates/scan-device-seam.md`), revised before slice 8 fixes
`twain-dsm`'s API. It closes TEST-3.

Not built here: `twain-dsm` (slice 8: the full bindings, the typestate, the Authenticode check), the `twain`
backend and the 32-bit worker (slice 9). The spike's bindings are a subset, and slice 8 takes from them only
what it re-checks.

## Taken as it starts

- **Slice 7 before slices 2 to 6** (Greg, 2026-11-08, on the Windows machine: "we can write a slice plan
  here"). Slice 7 depends on nothing they build: it is a throwaway scan whose findings go into the seam note.
  So the seam note's TWAIN sections are revised here, before slice 2 revises its SANE sections; each says
  which it touched. Slice 8 still waits on this slice's revision; slice 9 still waits on slices 3 and 4.
- **The process** (Greg, the same day: "it's a different machine, different paths, different build tools";
  not the `scan-slice` workflow): this plan, an adversarial review of it (below), the steps (each built, then
  checked), parallel code reviews, and a fix stage. A commit per step on the branch; nothing pushed without
  Greg's word.
- **The install** is `scripts/twain-sample.ps1` (written before the slice, 2026-11-08, and run here: the DSM
  2.5.0 and the sample source installed in both bitnesses through the official MSI, `msiexec /qn` elevated).
  Step 1 commits it.

## What it answers

| Unknown (seam note, TEST-3) | How the spike answers it | Step |
|---|---|---|
| Whether the sample source installs silently on `windows-latest` and scans with a hidden window, both bitnesses | The CI job: the script's `Install`, then the spike. CI runs only on `master` and pull requests (`ci.yml`), so it's seen on a draft pull request after step 1, on Greg's word; if the MSI fails there, the fallbacks under Risk 1 | 1 |
| `CAP_XFERCOUNT` from the environment sets the page count | Runs with `CAP_XFERCOUNT=3`, and unset (2); the spike counts the transfers | 2 |
| Tiles or assembled images from `twain-dsm`; what a DIB's palette becomes | The tiles' shapes recorded; tiles assembled and compared pixel for pixel with the native DIB of the same page, for RGB, gray and black and white; the DIB palettes, `ICAP_PIXELFLAVOR` and `ICAP_BITORDER` recorded | 2 |
| Whether `DAT_CALLBACK2` is enough on Windows without the loop | Two delivery modes, each run in full: every message forwarded through `MSG_PROCESSEVENT`, and a registered callback with messages dispatched but not forwarded. **Settled only for a source that signals synchronously**, as the sample does (inside `MSG_ENABLEDS`); sources that signal later, from their own windows or threads, are slice 12's, and the window and loop stay | 2 |
| What the pending-transfer count says while scanning with `CAP_XFERCOUNT = -1` | `MSG_ENDXFER`'s count recorded after each transfer (the sample's code: 0xFFFF, "unknown", until the last page, then 0) | 2 |
| What extended image info the sample offers | `ICAP_SUPPORTEDEXTIMAGEINFO` read; `DAT_EXTIMAGEINFO` tried after each transfer, its return and condition code recorded | 2 |
| How events cross from the TWAIN thread; posting cancels to its loop | Events sent on a channel to the main thread; a cancel posted from the main thread to the TWAIN thread's window, at a fixed point (the handshake in `cancel`) | 3 |
| Cancel as "finish the page in flight, feed no more" | The cancel during the first of three transfers: the transfer finishes, `MSG_ENDXFER`, then `MSG_RESET`; `MSG_STOPFEEDER` tried and its answer recorded. On the sample this shows **no further transfer, not no further feed**: it feeds the next sheet inside `MSG_ENDXFER` (`CAP_AUTOFEED`); feeding is slice 12's | 3 |
| "Continue" after a jam | Not answerable with the sample source (it can't jam); stays with slice 12, said | 4 |

## The design

### The sample source, as its code has it

Read in `twain/twain-samples` at `18352fe4ac6c` (2021-11-09, the 2.5.0 release; BSD-3; local copy in
`scratch/reference/twain/twain-samples/`), so the spike expects what the code does. The installed `.ds`
files carry the same identity strings.

- **Its identity:** Manufacturer `TWAIN Working Group`, ProductFamily `Software Scan`, ProductName `TWAIN2
  Software Scanner` (`CTWAINDS_FreeImage.cpp`). The spike opens the one source matching Manufacturer and
  ProductName, and fails unless exactly one matches (the 32-bit list also holds Windows' `wiatwain.ds`).
- **Its image** is `TWAIN_logo.png` beside the `.ds` file (850 by 1100, about 100 dpi), loaded fresh for every
  page and scaled into the frame (1700 by 2200 at the default 200 dpi): every page is the same image, so a page
  through native and through memory must match. Both paths copy from one strip buffer; the DIB stays BGR,
  memory RGB is swapped to RGB, and memory rows are padded to four bytes too.
- **Page count:** `CAP_XFERCOUNT` from the process's environment, read when the source is constructed and
  again at `MSG_OPENDS`; unset, **2** pages (`getDocumentCount`; the README says 1); 0 or less, a random count
  from 0 to 14. An empty feeder reloads itself (`isFeederLoaded`), so each open starts full. One page is
  acquired at `MSG_ENABLEDS` whatever the count. The spike never sets the variable (`std::env::set_var` is
  unsafe in edition 2024, and the recipe owns the value); it reads it to know what to expect, and refuses to
  judge counts if it's set but not a positive integer.
- **One `MSG_XFERREADY` per enable.** Without its UI (`ShowUI = FALSE`), `MSG_ENABLEDS` loads the image and
  signals `MSG_XFERREADY` before it returns (`enableDS` calls `DoXferReadyEvent`, which calls the DSM's
  `DAT_NULL`/`MSG_XFERREADY`). After `MSG_ENDXFER` with pages pending it moves straight to state 6 and signals
  nothing more: the app transfers again while the count isn't 0. With the capability `CAP_XFERCOUNT = -1` the
  count stays 0xFFFF (`TWON_DONTCARE16`) until the feeder is empty, then 0.
- **Signalling, through the DSM:** with a callback registered, the DSM calls it at once, on the thread that
  called `MSG_ENABLEDS`, inside that call, and hands its return to the source: anything but `TWRC_SUCCESS`
  fails the enable (`TWCC_SEQERROR`). Without one, the DSM queues the message and posts `WM_NULL` to the
  window given at `MSG_OPENDSM`; the next `MSG_PROCESSEVENT` returns it as `TWRC_DSEVENT`, carried by whatever
  message was forwarded.
- **Indicators:** with `ShowUI = FALSE` and `CAP_INDICATORS` true, its Qt UI creates a `QApplication` on the
  calling thread and parents a widget under the app's window (`QT_UI.cpp`), though it shows nothing. The spike
  sets `CAP_INDICATORS` false, reads it back, and fails unless it is, so Qt stays out of the process.
- **Feeding:** `CAP_AUTOFEED` is true by default, so `MSG_ENDXFER` with pages pending acquires the next one
  before returning. `DAT_PENDINGXFERS` handles `MSG_ENDXFER`, `MSG_GET` and `MSG_RESET`; anything else (so
  `MSG_STOPFEEDER`) fails with `TWCC_BADPROTOCOL`. It accepts `MSG_RESET` in any state but loaded.
- **Transfers:** native (the default), memory and file; compression none only; pixel types black and white
  (1-bit), gray (8) and RGB (24, the default); 200 dpi. Memory: preferred buffer 64 KiB, so a 1700 by 2200
  RGB page comes in about 184 strips of 12 rows.
- **Its bit-order quirk:** it reads `ICAP_BITORDER` as a color-order switch, with opposite logic in the two
  paths (`CTWAINDS_Base.cpp`, `getDIBImage` and the memory transfer), so setting `TWBO_MSBFIRST` would make
  the DIB RGB and memory BGR. The spike never sets it, and records it and `ICAP_PIXELFLAVOR` (which offers
  `TWPF_CHOCOLATE`, 0 black, only). Its DIB palettes are fixed: black then white for 1-bit, an identity ramp for
  gray.
- **Capabilities:** `MSG_GET` on a boolean returns a `TW_ENUMERATION` to a `DF_APP2` app, not a
  `TW_ONEVALUE`; the spike reads current values with `MSG_GETCURRENT`, and decodes every container by its
  `ConType`. `CAP_SUPPORTEDDATS` lists `DAT_EXTIMAGEINFO` (not implemented) and `DAT_CALLBACK` but not
  `DAT_CALLBACK2`; the spike never decides anything from it.
- **No extended image info:** neither `ICAP_SUPPORTEDEXTIMAGEINFO` nor `ICAP_EXTIMAGEINFO` is supported, and
  `DAT_EXTIMAGEINFO` is the base class's, which fails with `TWCC_CAPUNSUPPORTED`; `TWEI_PAGESIDE` and its kind
  wait for real drivers (slice 12).
- **The current directory:** the DSM changes the process's current directory to each source's folder while it
  loads it, and the sample's UI object changes it again. The spike writes only to stdout; slice 9's worker
  needs to know (step 4 records it).

### Expected answers

Per the sample's code; any other answer is a failure, printed with its condition code. `DAT_STATUS` is asked
of the source for source triplets and of the DSM (`pDest` null) for DSM triplets. An implementation that sees
a different answer and finds the code agrees says so in "So far", rather than changing the table quietly.

| Triplet | State | Expected |
|---|---|---|
| `DAT_PARENT`/`MSG_OPENDSM`, then `DAT_ENTRYPOINT`/`MSG_GET` | 2, 3 | success, success (entry points all non-null) |
| `DAT_IDENTITY`/`MSG_GETFIRST`, `MSG_GETNEXT` | 3 | success, then `TWRC_ENDOFLIST` |
| `DAT_IDENTITY`/`MSG_OPENDS` | 3 to 4 | success |
| `DAT_CALLBACK2`/`MSG_REGISTER_CALLBACK` (callback mode, right after opening) | 4 | success |
| `DAT_CAPABILITY`/`MSG_SET` of `ICAP_XFERMECH`, `CAP_XFERCOUNT` (-1, or 1), `CAP_INDICATORS` (false), `ICAP_PIXELTYPE` (`compare`) | 4 | success |
| `MSG_GETCURRENT` of `CAP_INDICATORS`, `CAP_FEEDERLOADED`, `ICAP_PIXELTYPE`, `ICAP_XRESOLUTION`, `ICAP_BITORDER`, `ICAP_PIXELFLAVOR` | 4 | success (false; true; as set; 200; `TWBO_LSBFIRST`; `TWPF_CHOCOLATE`) |
| `MSG_GETCURRENT` of `ICAP_SUPPORTEDEXTIMAGEINFO`; `MSG_SET` of `ICAP_EXTIMAGEINFO` true | 4 | failure, `TWCC_CAPUNSUPPORTED` |
| `DAT_USERINTERFACE`/`MSG_ENABLEDS`, `ShowUI = FALSE`, `ModalUI = FALSE`, `hParent` the window | 4 to 5, then 6 | success; `MSG_XFERREADY` inside (callback) or queued (event) |
| `DAT_IMAGEINFO`/`MSG_GET` | 6 | success |
| `DAT_IMAGENATIVEXFER`/`MSG_GET` | 6 to 7 | `TWRC_XFERDONE`, a handle |
| `DAT_SETUPMEMXFER`/`MSG_GET`, then `DAT_IMAGEMEMXFER`/`MSG_GET` repeated | 6, 6 to 7 | success; success per strip, `TWRC_XFERDONE` on the last |
| `DAT_EXTIMAGEINFO`/`MSG_GET` | 7 | failure, `TWCC_CAPUNSUPPORTED` |
| `DAT_PENDINGXFERS`/`MSG_ENDXFER` | 7 to 6 or 5 | success; 0xFFFF then 0 (`CAP_XFERCOUNT = -1`), 0 (= 1) |
| `DAT_PENDINGXFERS`/`MSG_STOPFEEDER` (`cancel`) | 6 | failure, `TWCC_BADPROTOCOL` |
| `DAT_PENDINGXFERS`/`MSG_RESET` (`cancel`) | 6 to 5 | success, count 0 |
| `DAT_USERINTERFACE`/`MSG_DISABLEDS`; `DAT_IDENTITY`/`MSG_CLOSEDS`; `DAT_PARENT`/`MSG_CLOSEDSM` | 5 to 4, 4 to 3, 3 to 2 | success |

### twain-spike

`/crates/twain-spike/`: a binary, `publish = false`, MIT OR Apache-2.0, edition 2024, `rust-version = "1.89"`
(the floor; no MSRV job: it's never a dependency), `lints.workspace = true`, its one dependency `windows-sys`
0.61 under `[target.'cfg(windows)'.dependencies]` with the features it calls (`Win32_Foundation`,
`Win32_System_LibraryLoader`, `Win32_System_Memory`, `Win32_System_Com`, `Win32_System_Threading`,
`Win32_UI_WindowsAndMessaging`, `Win32_Graphics_Gdi`, `Win32_Storage_FileSystem`). Not named by the
devices-layer gate (it's not in the cut); no other crate depends on it. Its crate doc says it's a throwaway:
slice 8's own tests replace it, and it's deleted then.

- **On other systems:** everything but `image` is `#[cfg(windows)]`; `image` compiles everywhere, under
  `cfg_attr(not(windows), allow(dead_code))`, so its unit tests run on every runner; `main` says it's Windows
  only and exits 2; no doc link points into a Windows-only module. Checked here by a check build for
  `x86_64-unknown-linux-gnu` (below), since no Linux or macOS runner is at hand.
- **`ffi`: the bindings subset**, ported from `TWAINH.cs` (twain-cs at `74fe02ed78a6`, MIT, Kodak Alaris:
  the definitions are in `TWAINH.cs`, not `TWAIN.cs`, which wraps them; `crates/twain-dsm.md` and the
  milestone say `TWAIN.cs` and are corrected in step 4), each value checked against `twain.h` 2.5 (twain-dsm
  at `a50f7326a191`). The file keeps Kodak Alaris's copyright and permission notice word for word and the
  TWAIN Toolkit's disclaimer, as `crates/twain-dsm.md` has it. Only what the spike calls: the `DG`, `DAT`,
  `MSG`, `TWRC`, `TWCC`, `TWON`, `TWTY`, `TWSX`, `TWPT`, `TWPF`, `TWBO`, `TWMF` and `TWEI` constants it uses,
  the capability ids it touches, and `TW_STR32`, `TW_VERSION`, `TW_IDENTITY`, `TW_FIX32`, `TW_CAPABILITY`,
  `TW_ONEVALUE`, `TW_ENUMERATION`, `TW_USERINTERFACE`, `TW_EVENT`, `TW_PENDINGXFERS`, `TW_IMAGEINFO`,
  `TW_SETUPMEMXFER`, `TW_MEMORY`, `TW_IMAGEMEMXFER`, `TW_STATUS`, `TW_ENTRYPOINT`, `TW_CALLBACK2`,
  `TW_EXTIMAGEINFO` and `TW_INFO`. `#[repr(C, packed(2))]`, as `twain.h`'s `#pragma pack (2)` on Windows;
  fields read by copy, never by reference; container items past a struct's end (`TW_ENUMERATION`'s list, at
  offset 14) read with `read_unaligned`, unit-tested on a buffer offset by 2. Layouts follow `twain.h` where
  `TWAINH.cs` differs, said in the file: `TWAINH.cs`'s Windows `TW_IDENTITY_LEGACY` ends in 8 bytes of padding
  ("accounts for Id and SupportedGroups") that `twain.h` hasn't, so `TW_IDENTITY` is 156 bytes. Function
  pointers are `unsafe extern "system"` (`TW_CALLINGSTYLE` is `PASCAL`: `__stdcall` on x86): `DSM_Entry`, the
  callback, and `TW_ENTRYPOINT`'s five fields, typed `Option<unsafe extern "system" fn(..)>` from `twain.h`'s
  `DSMENTRYPROC`, `DSM_MEMALLOCATE`, `DSM_MEMFREE`, `DSM_MEMLOCK` and `DSM_MEMUNLOCK` (`TWAINH.cs` types them
  `IntPtr`, so they're written from the header). Each struct's size, and the offsets that differ by bitness,
  asserted at compile time for both targets, with the numbers the review's probe measured from MSVC compiling
  `twain.h` (x64 / x86): `TW_IDENTITY` 156/156, `TW_CAPABILITY` 12/8, `TW_MEMORY` 16/12, `TW_IMAGEMEMXFER`
  42/38 (`Memory` at 26), `TW_ENTRYPOINT` 44/24, `TW_EVENT` 10/6, `TW_CALLBACK2` 18/10, `TW_INFO` 16/12,
  `TW_EXTIMAGEINFO` 20/16, `TW_USERINTERFACE` 12/8, `TW_ENUMERATION` 16, `TW_IMAGEINFO` 42, `TW_ONEVALUE` 6
  (`Item` at 2).
- **`dsm`:** the DSM loaded with `LoadLibraryExW("TWAINDSM.dll", LOAD_LIBRARY_SEARCH_SYSTEM32)` (System32 in a
  64-bit process, SysWOW64 in a 32-bit one); `DSM_Entry` taken by `GetProcAddress`. Reported: the module's
  path (a 32-bit process reports the System32 alias for the SysWOW64 copy), its file version, and the `Machine`
  field of its PE header, read from the loaded image, so the bitness is shown, not assumed. `DAT_ENTRYPOINT`
  read after `MSG_OPENDSM` (the app's identity says `DF_APP2`; `Size` set exactly), and every container
  allocated, locked, unlocked and freed through the DSM's memory functions (in both bitnesses), with
  `GlobalAlloc` and friends if the DSM gives none, said. No signature check (slice 8's).
- **`twain_thread`:** a thread of its own, made an STA (`CoInitializeEx`, apartment-threaded), with a real
  top-level window, never shown (a registered class, no `WS_VISIBLE`, not `HWND_MESSAGE`), passed to
  `MSG_OPENDSM` and as `hParent`. Every DSM call is made on it, **in the loop body, never in the window
  procedure**, which only defers to `DefWindowProcW`: a panic in an `extern "system"` frame aborts the process.
  The loop is `GetMessageW`. Requests from the main thread are posted messages (`WM_APP + n`) to the window,
  handled in the loop body and never forwarded to the DSM; while a source is enabled, every other message is
  forwarded through `DAT_EVENT`/`MSG_PROCESSEVENT` in event mode (`TWRC_DSEVENT`: act on its `TWMessage`, don't
  dispatch; `TWRC_NOTDSEVENT`: dispatch), and only dispatched in callback mode. Events go out on a
  `std::sync::mpsc` channel. The callback, registered with `DAT_CALLBACK2`/`MSG_REGISTER_CALLBACK` in state 4
  right after `MSG_OPENDS` (as `TWAIN.cs` does), never calls the DSM: it records the message, its thread and
  whether it ran inside `MSG_ENABLEDS`, posts to the window, and **always returns `TWRC_SUCCESS`**. The
  transfer loop is the one the sample needs: on the single `MSG_XFERREADY`, repeat {`DAT_IMAGEINFO`, the
  transfer, `DAT_EXTIMAGEINFO`, `MSG_ENDXFER`} while the count isn't 0, the count read as `u16` and 0xFFFF
  printed as "unknown (-1)"; then disable.
- **`image`: decoding, without the DSM.** A DIB (header, palette, bottom-up rows padded to four bytes, BGR)
  into rows top down; memory strips or tiles into rows by `XOffset`, `YOffset`, `Columns`, `Rows` and
  `BytesPerRow`, with only the first `BitsPerSample` entry trusted; both into one comparable form (rows top
  down, RGB, gray or 1-bit, its polarity taken from `ICAP_PIXELFLAVOR` and checked against the DIB's palette).
  A driver's bytes are untrusted (`plan.md`): the DIB's header, palette (`biClrUsed`) and rows checked against
  the handle's size (`GlobalSize`) before they're read; each tile against `TW_IMAGEINFO`'s bounds and
  `BytesWritten` against its buffer; the image capped (1 GiB, as `sane-dl`'s frames). Pure functions,
  unit-tested on made-up data: a padded DIB, a palette, 1-bit rows of odd width, tiles out of order, and each
  refusal (a short handle, `biClrUsed` too large, a tile past the image, `BytesWritten` over the buffer).
- **The run** (`twain-spike all`, or one scenario by name): the main thread starts the TWAIN thread, sends it
  the scenario, prints each event as a line (`state 3 -> 4`, `xferready via callback, inside ENABLEDS, thread
  …`, `imageinfo 1: 1700x2200 bps=[8,8,8] 200 ppi rgb`, `memory 1: 184 strips of 12 rows, bpr 5100`, `pending
  unknown (-1)`, `extimageinfo: failure, TWCC_CAPUNSUPPORTED`), and checks what it expects. If no event comes
  for 30 seconds, it prints why, flushes, and ends the process with `TerminateProcess(GetCurrentProcess(), 1)`
  (not `std::process::exit`: `ExitProcess` runs every DLL's detach code, which can deadlock on a lock the hung
  thread holds; the sample's `DllMain` unloads the DSM on detach). The scenarios, each expecting a page count:
  - `list`: states 1 to 3 and back; every source's identity. No pages.
  - `native-event`, `memory-event`, `native-callback`, `memory-callback`: open the sample source (state 4),
    register the callback (callback modes), set `ICAP_XFERMECH`, `CAP_XFERCOUNT = -1` and `CAP_INDICATORS =
    FALSE`, read the capabilities in the table, try `ICAP_EXTIMAGEINFO`; enable (5, then 6); the transfer loop;
    disable (4), close (3), close the DSM (2), unload (1). Pages: `CAP_XFERCOUNT` from the environment, else 2.
  - `compare`: for RGB, gray and black and white, one page native and one through memory (`CAP_XFERCOUNT = 1`
    each), event mode, assembled and compared pixel for pixel; each DIB's palette recorded. Six pages.
  - `cancel`: memory transfer, event mode, three pages expected (the recipe's `CAP_XFERCOUNT=3`). After the
    first strip of page 1 the TWAIN thread sends `transfer started` and pumps only its `WM_APP` range
    (`PeekMessageW`, `PM_REMOVE`) for up to 10 seconds; the main thread answers with a posted cancel. The
    TWAIN thread finishes the page, then `MSG_ENDXFER` (7 to 6), `MSG_STOPFEEDER` (its answer recorded),
    `MSG_RESET` (6 to 5, count 0), disables, and walks down. Checked: page 1 complete, no second transfer, the
    states in that order, down to 1. One page.
- **What it checks itself, exiting 1 on any failure:** the sample source found, exactly once; every answer as
  the table has it; the states walked in order and back to 1; each scenario's page count; each image's size
  from `DAT_IMAGEINFO` equal to the decoded DIB's and to the assembled tiles'; native and memory equal pixel for
  pixel per pixel type; `CAP_INDICATORS` false after setting it; the cancel's single page. What it observes
  but doesn't judge (callback thread and timing, strip shapes, pending counts, palettes, the module's path and
  `Machine`) it prints, for step 4.

### CI and the justfile

- **Here, `just` and `cargo` run from Git Bash.** From PowerShell `just` finds no `sh` (only `Git\cmd` is on
  its `PATH`, and `bash` there is WSL's launcher). The justfile doesn't set `windows-shell`, which would break
  its inline `VAR=value` assignments.
- **`just twain-spike target`**: `CAP_XFERCOUNT=3 cargo run --locked -p twain-spike --target <target> --
  all`, the target required (`x86_64-pc-windows-msvc` or `i686-pc-windows-msvc`). Not in `ci`, which must pass
  on Linux; its comment says CI's Windows TWAIN job runs it.
- **The `twain-spike` job** in `/.github/workflows/ci.yml`, on `windows-latest`, read-only and credentials not
  persisted as the others; it introduces `timeout-minutes: 15` (no other job has one yet). Steps:
  `dtolnay/rust-toolchain@stable` with `targets: i686-pc-windows-msvc`; `setup-just`; `powershell -NoProfile
  -ExecutionPolicy Bypass -File scripts\twain-sample.ps1 Install`, then `Status`, with `shell: powershell`
  (Windows PowerShell 5.1, where it was tested; the runner is elevated, so there's no UAC prompt); then `just
  twain-spike x86_64-pc-windows-msvc` and `just twain-spike i686-pc-windows-msvc` (just's recipes run under
  the runner's Git Bash, as the check job's already do). Step 1 runs `list` only; step 3 switches it to `all`.
  The check job's Windows leg needs nothing new: the spike's unit tests use no DSM.

## Steps

**Every build here** runs from Git Bash with `CARGO_HOME` at `scratch/cargo-home` (`plan.md`'s "no writes to
`~/.cargo`"; rustup's toolchains and targets stay in `~/.rustup`); `plan.md`'s memory scope is systemd's, with
no equivalent on Windows, and these builds are small.

**At every step**, the check runs once, on this machine: `just fmt clippy test doc` (what CI's check job
runs; `gates` and `msrv` can't run here: the gates read ELF files and need `jq`, and no 1.89 toolchain is
installed); for the other targets, `cargo clippy --locked -p twain-spike --all-targets -- -D warnings` with
`--target i686-pc-windows-msvc` and with `--target x86_64-unknown-linux-gnu` (a check build, no linker), and
`RUSTDOCFLAGS="-D warnings" cargo doc --locked -p twain-spike --no-deps --target x86_64-unknown-linux-gnu`;
from the gates, the parts that can run here by hand: the name gate's `git grep` and `cargo tree -p
twain-spike --target all -e normal,build` read against the devices layer's patterns; `cargo test --locked
--workspace -- --list` recorded under `scratch/work/s0-7/` and compared with the step before's (none lost);
`~/.cargo` unchanged by the step's builds (its listing, with sizes and times, before and after); every `//
SAFETY:` comment the step adds read against what the call needs; and the step's diff and message scanned for
local paths, the username, and the downstream app's name outside `docs/pm/`. Each "Done when" says only what
the step adds.

1. **The sample source on CI; the DSM loaded; sources listed.** The script committed, its help corrected
   (the default page count is 2; 0 or less is random from 0 to 14). `/crates/twain-spike/` with its manifest,
   crate doc, the stub `main` for other systems, `ffi` (the whole subset above, with its notice, disclaimer
   and layout assertions), `dsm`, `twain_thread` (window, STA, loop, requests and events, the 30-second
   watchdog) and the `list` scenario. `Cargo.lock` updated for `windows-sys` (once without `--locked`, then
   with). `just twain-spike` and the CI job (`list` only). `rustup target add x86_64-unknown-linux-gnu` here
   (its standard library, for the check builds).
   **Done when:** the core; `twain-spike list` in both bitnesses finds the sample source exactly once (the
   32-bit run lists Windows' `wiatwain.ds` too), walks 1 to 3 and back, and reports the DSM's path, version,
   `Machine` (0x8664, 0x14C) and memory functions; the layout assertions compile on both targets, and one is
   seen to fail when a field is changed (a scratch edit, reverted); the report saved to
   `scratch/work/s0-7/step-1/`. **Then, on Greg's word:** the branch pushed and a draft pull request opened,
   so CI's `twain-spike` job reports before step 2; its result recorded in "So far". If the MSI fails there,
   the fallbacks under Risk 1, in order, before step 2.
2. **States 4 to 7: transfers, image info, the two delivery modes, the comparison.** `image` and its unit
   tests; the four transfer scenarios and `compare`; the expected-answers table enforced.
   **Done when:** the core; each scenario passes in both bitnesses with `CAP_XFERCOUNT=3` (three pages each)
   and unset (two); `compare` equal for RGB, gray and black and white in both; the unit tests cover the cases
   and refusals above; a callback made to return failure (a scratch edit, reverted) is seen to fail the enable
   with `TWCC_SEQERROR`, so the return matters as the code says; the reports saved to
   `scratch/work/s0-7/step-2/`, with what each observation says.
3. **Cancel from another thread.** The `cancel` scenario; `all` runs every scenario in order; the CI job runs
   `all`.
   **Done when:** the core; `cancel` passes in both bitnesses, one page and the walk down as checked;
   `all` passes in both; a hang made inside a DSM call (a scratch edit: the callback sleeps 60 seconds while
   `MSG_ENABLEDS` is on the stack, reverted) ends the process at 30 seconds with exit code 1 rather than
   hanging; reports in `scratch/work/s0-7/step-3/`.
4. **What it found, written down.** The seam note revised from steps 1 to 3: its TWAIN rows in sections 3 to
   8 (section 4: the sample offers no extended image info; section 6: "Continue" after a jam isn't answerable
   on the sample, still slice 12's), "Known unknowns" (what's settled, by what, and "on the sample source
   only"; the callback answer scoped to synchronous signalling; cancel as "no further transfer", feeding
   slice 12's), and a line in its status saying slice 7 revised it before slice 2. `crates/twain-dsm.md` and
   the milestone's slices 7 and 8 corrected: the definitions from `TWAINH.cs`; `TW_IDENTITY`'s layout from
   `twain.h`; the memory functions `stdcall`; the DSM is the app's to install (the specification) as well as
   drivers'; the signature check by signer name, not certificate (2.5.0 and 2.5.1 have different certificates
   and the same signer); the sample's quirks (bit order, one `MSG_XFERREADY`, the 0xFFFF count, autofeed at
   `MSG_ENDXFER`); the DSM's current-directory changes, for slice 9's worker. TEST-3 closed into
   `archive/backlog-done.md` if CI's job has passed by then, else updated with what this machine showed.
   `milestones/s0-devices.md` slice 7 said done here (CI's result, or pending); `status.md`; `decisions.md`
   for "Taken as it starts". This plan's "So far".
   **Done when:** the core; every claim in the revised sections traced to a report under `scratch/work/s0-7/`
   or a line of the sources; the docs read for anything private (the repository is public).

**Closing:** the code review (parallel reviewers), the fixes, and then, on Greg's word, the pull request
marked ready; CI's check job on three systems and the `twain-spike` job green closes the slice (and TEST-3, if
step 4 couldn't).

## Baselines

This machine, 2026-11-08: Windows 11 Pro 10.0.26300; Visual Studio 2022 Community, MSVC 14.37.32822 (x64 and
x86 libraries); Windows SDK 10.0.22000.0 (x64 and x86); rustc 1.99.0 stable, `x86_64-pc-windows-msvc` host,
with `i686-pc-windows-msvc` installed (added 17:47 that day), `x86_64-pc-windows-gnu` and `thumbv6m-none-eabi`;
`just` 1.50.0; Git for Windows 2.42.0 (`core.autocrlf=true`, system-wide). TWAIN: `TWAINDSM.dll` 2.5.0.0 in
System32 and SysWOW64, byte-identical to twain-dsm's 2.5.0 release, signed `O=TWAIN Working Group`, exporting
an undecorated `DSM_Entry` in both, importing only KERNEL32, USER32, GDI32 and ADVAPI32;
`TWAINDS_Sample64.ds` and `TWAINDS_Sample32.ds` 2.5.0.0 in `twain_64\sample2` and `twain_32\sample2`; Windows'
own `twain_32\wiatwain.ds` (10.0.26100.8875) and `twain_32.dll` (1.7.1.3). Sources read: `TWAINH.cs` and
`TWAIN.cs` at twain-cs `74fe02ed78a6`, `twain.h` at twain-dsm `a50f7326a191`, the sample at twain-samples
`18352fe4ac6c` (local copies in `scratch/reference/twain/`, each with a README of its pin); the DSM's own
sources (LGPL) read by the review, never copied.

## Decisions taken alone, for Greg to review

1. **The spike is a crate, `twain-spike`, never published**, a binary in the workspace so CI can run it;
   deleted when slice 8's own tests replace it.
2. **Its bindings are ported from `TWAINH.cs`**, the file in twain-cs that holds the definitions, checked
   against `twain.h` 2.5; where they differ, `twain.h`'s layout wins, said in the file; function-pointer
   types come from the header, since `TWAINH.cs` has only `IntPtr`s.
3. **`windows-sys` 0.61, not `windows`:** no COM interfaces are needed; `windows` stays the WIA backend's.
4. **No signature check in the spike**; the script's `Status` reports the DSMs' signers, and slice 8 builds
   the check.
5. **The spike checks itself** and prints a readable report; no JSON, no serde.
6. **`CAP_XFERCOUNT` is the recipe's to set**, never the spike's.
7. **The CI job runs the script in Windows PowerShell 5.1**, where it was tested; pwsh 7's COM late binding
   wasn't tried.
8. **The seam note's TWAIN sections are revised before slice 2's SANE revision.**
9. **A draft pull request after step 1**, on Greg's word, so the runner's answer comes before the work that
   rests on it; rather than `workflow_dispatch`, which runs only once the workflow is on `master`.
10. **The watchdog ends the process with `TerminateProcess`**, so a hang can't turn into a deadlock at exit.

## For Greg (security)

1. **The install runs an unsigned MSI as administrator**, vouched for only by two SHA-256 pins (the release
   zip and the MSI in it), over TLS from GitHub's release download. On CI the runner is thrown away after;
   here, `Uninstall` removes it (and the DSM, unless another product shares its component).
2. **Loading the DSM runs every installed source's code in the process:** the DSM loads each `.ds` under
   `twain_32` or `twain_64` to list them, changing the process's current directory as it does. That's TWAIN's
   model, and why the app runs drivers in a worker (slice 9); the spike is a separate process, run by hand or
   by CI.
3. **The DSM is taken from the system folder only** (`LOAD_LIBRARY_SEARCH_SYSTEM32`), never the working
   directory or the app's folder.
4. **The draft pull request is public** (the repository is): it holds only the slice's code and `docs/pm/`
   files, read for anything private at each step.

## Risks

1. **`windows-latest` may not run it:** the MSI's silent install, a hidden window in the runner's session, or
   the sample's DLLs loading there. Seen at the draft pull request after step 1. If the MSI fails, in order:
   `msiexec /a` (an administrative extract, which installs nothing) and the files copied into System32,
   SysWOW64, `twain_32\sample2` and `twain_64\sample2`; else yushulx/virtual-scanner's portable zip (64-bit
   only, the 32-bit run then by hand here); else the milestone's desk check, the seam note revised from this
   machine's runs, said. If the install works and the scan doesn't, the report says where it stopped, and
   step 4 says so.
2. **Layout or calling-convention mistakes crash rather than fail,** most likely on i686 (`stdcall`, packing,
   pointer width, the memory functions): the compile-time assertions, the memory functions driven in the
   32-bit run, and the 32-bit run itself are the guard.
3. **Re-entrancy:** the sample signals `MSG_XFERREADY` inside `MSG_ENABLEDS`, so a callback runs inside a DSM
   call; the callback only records, posts and returns success.
4. **The sample is not a scanner:** one image, no jams, no extended info, no duplex, synchronous signalling
   only. What it settles is said as "on the sample source"; the hand checks (slice 12) settle the rest.
5. **Two slices on one tree:** slice 1 lands on `master` from Linux while this runs; this branch is rebased
   before each push, and `status.md`, the CI file and the justfile are the likely conflicts.
6. **The cfg split shows only on CI's Linux and macOS legs:** the Linux check build here covers clippy and
   docs, not macOS, and not running the tests there.

## Plan review

2026-11-08, before step 1, by one adversarial reviewer (a fresh agent given the plan, the repository and the
primary sources, not the author's reasoning), with probes in `scratch/work/s0-7/review-probe/` (layouts from
MSVC compiling `twain.h` and from Rust in both bitnesses; the DSM loaded in a 32-bit process). Each finding was
checked against the sources before it was acted on (the load-bearing ones re-read here: the one
`MSG_XFERREADY`, the 0xFFFF count, autofeed at `MSG_ENDXFER`, the bit-order switch, the identity strings,
`CAP_XFERCOUNT`'s validation, CI's triggers, `just` from PowerShell). The plan as drawn is kept at
`scratch/work/s0-7/plan.before-review.md`. All 24 accepted; none rejected.

**High:**

1. CI doesn't run on a pushed branch, only on `master` and pull requests, so the runner's answer would have
   come after step 4; and no fallback was planned. Now a draft pull request after step 1, on Greg's word, and
   the fallbacks in Risk 1.
2. The sample signals `MSG_XFERREADY` once per enable, and with `CAP_XFERCOUNT = -1` its count stays 0xFFFF:
   one transfer per signal would have hung after page 1. Now the loop while the count isn't 0, read as `u16`.
3. The cancel could flake or never be read (transfers run synchronously on the TWAIN thread), skipped
   `MSG_ENDXFER`, and named no mode. Now a handshake after page 1's first strip, `MSG_ENDXFER` then
   `MSG_RESET`, event mode.

**Medium:**

4. The callback's return goes to the source, which fails the enable on anything but success: it returns
   `TWRC_SUCCESS`, registered right after `MSG_OPENDS`; step 2 sees a failing return fail the enable.
5. `std::process::exit` can deadlock in DLL detach code: the watchdog uses `TerminateProcess`, and step 3's
   hang is made inside a DSM call.
6. Autofeed feeds the next sheet inside `MSG_ENDXFER`: the cancel shows "no further transfer", not "no further
   feed"; `MSG_STOPFEEDER` tried and recorded.
7. The callback answer is scoped to synchronous signalling; the window and loop stay.
8. The cfg split for other systems is planned, and checked here by Linux check builds.
9. `just` runs from Git Bash here.
10. The bit-order quirk: `ICAP_BITORDER` never set; it and `ICAP_PIXELFLAVOR` recorded; polarity from the
    flavor, checked against the palette.
11. `TW_ENTRYPOINT`'s memory functions are `stdcall` too, typed from `twain.h`.
12. `compare` stops at one page per run (`CAP_XFERCOUNT = 1`); each scenario's expected count is given; counts
    aren't judged when the variable isn't a positive integer.
13. The expected answers are a table; capabilities read with `MSG_GETCURRENT` and decoded by `ConType`;
    `CAP_SUPPORTEDDATS` never trusted.

**Low:**

14. Indicators would bring Qt in (a `QApplication` on the TWAIN thread): `CAP_INDICATORS` read back false;
    `QT_UI.cpp`, `Common.h` and `DSMInterface.cpp` added to the local sources.
15. The 32-bit run reports the System32 alias: the PE `Machine` reported too; `Win32_Storage_FileSystem` added.
16. No job had `timeout-minutes`; the script is run with `-ExecutionPolicy Bypass -File`.
17. Sections 4 and 6 of the seam note, and the milestone's `TWAIN.cs` wording, are in step 4.
18. Items past a container's end are read with `read_unaligned`.
19. The decoder checks a driver's bytes before reading them, with a test per refusal.
20. No DSM call in the window procedure (a panic there aborts).
21. The spike's own `WM_APP` messages are never forwarded to `MSG_PROCESSEVENT`.
22. The DSM's current-directory changes: stdout only; recorded for slice 9.
23. The core gains slice 1's three checks (cargo home, the test list, `SAFETY` comments); Baselines lists the
    targets installed.
24. The sample is matched by Manufacturer and ProductName, exactly once; `DAT_STATUS` asked of the right
    party.

**Checked and sound** (the reviewer's list, kept): the layouts above, measured; `TWAINH.cs`'s padding is
there; `DAT_CALLBACK2` is right for this DSM (it stores either registration in one `TW_CALLBACK2`); event mode
won't stall (the DSM posts `WM_NULL`); `DAT_ENTRYPOINT` needs `DF_APP2` and an exact `Size`, and
`DSM_MemAllocate` is `GlobalAlloc(GPTR)`; the 32-bit DSM exports an undecorated `DSM_Entry`, and
`LoadLibraryExW` with `LOAD_LIBRARY_SEARCH_SYSTEM32` loads the SysWOW64 copy in a 32-bit process; the sample's
facts as corrected above; native and memory pages match at defaults; windows-sys 0.61.2 has each function in
the feature named; the script is ASCII with LF endings and passes the name gate.

## So far

### Step 1: the sample source on CI; the DSM loaded; sources listed (2026-11-08)

- **What it does:** `scripts/twain-sample.ps1` committed, its help corrected (2 pages by default; 0 or less
  random from 0 to 14). `/crates/twain-spike/`: its manifest (`windows-sys` 0.61 for Windows only, the eight
  features; `Cargo.lock` gained only the crate's own entry, `windows-sys` 0.61.2 being locked already); `ffi`
  (the whole subset, with Kodak Alaris's notice and the TWAIN Toolkit's disclaimer; 136 constants, each read
  from `TWAINH.cs` and checked against `twain.h` by a script, `scratch/work/s0-7/step-1/consts.py`, all
  agreeing; the layouts asserted for both bitnesses); `dsm` (loading from System32, `DSM_Entry`, the module's
  path, version and PE `Machine`, the memory functions with the `GlobalAlloc` fallback); `window` (the
  apartment and the hidden top-level window, `DefWindowProcW` its window procedure); `session` (the state, the
  identities, typed triplets with their answers checked, the walk down on drop); `twain_thread` (the thread,
  its events); `run` (the scenarios chosen, the 30-second watchdog ending the process with
  `TerminateProcess`); the `list` scenario. `just twain-spike target [scenario]`; the CI job.
- **Checked here:** the core (`just fmt clippy test doc`; clippy for `i686-pc-windows-msvc` and
  `x86_64-unknown-linux-gnu`, and the Linux docs, all without a warning); `twain-spike list` passing in both
  bitnesses (`scratch/work/s0-7/step-1/list-64.txt`, `list-32.txt`): the DSM 2.5.0.0, machine 0x8664 and
  0x14C (the 32-bit one under the System32 name), the DSM's own memory functions driven, the sample listed once,
  states 1 to 3 and back; a layout assertion seen to fail on both targets when `ProtocolMajor` was widened to
  `u32` (`make-fail-*.log`), the file restored byte for byte; the twain-spike graph (`twain-spike`,
  `windows-sys`, `windows-link`) clear of the devices layer's patterns; the name gate's `git grep` finding
  nothing; the test list empty, as the workspace has no tests yet (`test-list.txt`, step 2's baseline);
  `~/.cargo` unchanged at depth 3 (a full walk is too slow here); every `SAFETY` comment read; the diff
  scanned.
- **Found:**
  - **The DSM skips Windows' `wiatwain.ds` on purpose**: its log says "We're deliberately skipping this file:
    C:\WINDOWS\twain_32\wiatwain.ds" (`apps.cpp` line 1591; `scratch/work/s0-7/step-1/dsm32.log`), so the
    32-bit list holds the sample alone, as the 64-bit one does. The plan expected `wiatwain.ds` listed; it
    isn't, and WIA devices aren't reached through TWAIN this way: the `wia` backend stays needed.
  - **The DSM loads every source at `MSG_OPENDSM`**, not when they're listed (its log), so a source's code
    runs as soon as the DSM is opened.
  - **The DSM logs on request:** `TWAINDSM_LOG` naming a file and `TWAINDSM_MODE=w`; a tool for slices 8, 9
    and 12.
- **Departures from the plan, and why:**
  - *The message loop and posted requests wait for step 2*, where the first scenario needs them; this step has
    the thread, apartment, window, events and watchdog.
  - *`dsm_triplet` is an `unsafe fn`*, found in the `SAFETY` reading: as a safe function taking a raw data
    pointer, a wrong pointer anywhere in the session would have been undefined behaviour with no `unsafe` at
    the call. Each call now says why its pointer is right.
  - *`just twain-spike` takes the scenario as an optional second argument*, `all` by default, so the CI job
    runs `all` from now on (just `list` so far) and doesn't change in step 3.
  - *`TWLG_USA`*, not `TWLG_ENGLISH_USA`, which both sources define as an alias of it.
- **CI:** pushed, and the draft pull request opened (Greg, 2026-11-08); every job passed (Greg: "ci passed
  on GH"): the check job on Ubuntu, Windows and macOS, so the spike's split for other systems holds; and the
  `twain-spike` job, so **the sample source installs silently on `windows-latest`** (`msiexec /qn`, the runner
  elevated) and the DSM loads, opens with the hidden window and lists it in both bitnesses there. TEST-3's
  install question is answered; its scanning one waits on step 2's push.

### Step 2: states 4 to 7, transfers, the two delivery modes, the comparison (2026-11-08)

- **What it does:** `container` (capability containers as bytes: a `TW_ONEVALUE` built, a one-value or an
  enumeration decoded, every length checked) and `image` (a DIB decoded, memory tiles assembled, both into one
  comparable form; the checks against a block's size, the image's bounds, the buffer, overlaps and gaps; the
  1 GiB and 2^28-pixel caps), both compiled and tested on every system, as `ffi` now is; `MSG_NULL` and
  `TWCP_NONE` added to the checked constants (138, `scratch/work/s0-7/step-2/consts-check.txt`). `session`:
  the source opened and closed, the callback (atomics only, never the DSM, `TWRC_SUCCESS` always),
  capabilities set and read through containers from the DSM's allocator, enable and disable, the wait for the
  source's message (callback, or `MSG_PROCESSEVENT`, a 20-second timer under the watchdog), image info,
  native and memory transfers, extended image info, `MSG_ENDXFER`, `MSG_RESET`, and `walk_down`, a step at a
  time from any state. The scenarios `native-event`, `memory-event`, `native-callback`, `memory-callback` and
  `compare`; the table's answers enforced.
- **Checked here:** the core on all three targets without a warning; 17 unit tests (`container` 5, `image`
  12: a padded and a top-down DIB, both 1-bit palettes, the gray ramp and an inverted one, odd widths, strips
  and two-dimensional tiles out of order, polarity, and each refusal the plan names); `all` passing in both
  bitnesses with `CAP_XFERCOUNT=3` (three pages a batch) and unset (two), and `CAP_XFERCOUNT=0` giving a
  random count (six, here), not judged (`scratch/work/s0-7/step-2/all-*.txt`); `compare` equal pixel for pixel
  for RGB, gray and black and white in both; a callback made to return `TWRC_FAILURE` failing the enable with
  `TWCC_SEQERROR` and the session walking down cleanly (`make-fail-callback.txt`), the file restored; the
  test list from 0 to 17, none lost; `~/.cargo` unchanged; the graph and the name gate as before.
- **Found** (on the sample source, both bitnesses alike):
  - **Every answer is as the table has it.** The pending count is 0xFFFF, "unknown", after every page but the
    last, then 0, with `CAP_XFERCOUNT = -1`.
  - **Memory transfers come as strips of full rows, in order, rows padded to four bytes** as a DIB's are:
    RGB in 184 strips of 12 rows (5100 bytes a row), gray in 58 of 38 (1700), black and white in 8 of 303
    (216 bytes for 1700 bits); a strip is as many rows as fit the preferred 64 KiB.
  - **Native and memory are equal pixel for pixel** in all three types: the DIBs bottom up, RGB with no
    palette, gray with an identity ramp, 1-bit black then white; memory's 0 is black, as `TWPF_CHOCOLATE`
    says.
  - **Callback mode:** `MSG_XFERREADY` arrives in the callback inside `MSG_ENABLEDS`, on the TWAIN thread;
    a failing return fails the enable. Synchronous signalling only, as the plan scopes it.
  - **Event mode:** `MSG_XFERREADY` comes back from the first message forwarded through `MSG_PROCESSEVENT`,
    whatever it is: `0x031F` (`WM_DWMNCRENDERINGCHANGED`, the desktop window manager's, to a new window) on a
    window's first batch, the DSM's own `WM_NULL` on later ones.
  - **`MSG_GETCURRENT` gives one-values**, booleans included; the enumeration the review found in the code is
    `MSG_GET`'s.
- **Departures from the plan, and why:**
  - *Containers are parsed from byte slices with `from_le_bytes`*, not read with `read_unaligned`: nothing
    misaligned is ever dereferenced, and the offset-by-2 test is a slice two bytes in.
  - *`walk_down`*, found by the callback make-fail: the scenarios' own ending unloaded the DSM from state 4;
    every scenario now ends with the same walk the session's drop does.
  - *`as_chunks::<3>()`*, which clippy 1.99 asks for over `chunks_exact(3)` (stable since 1.88).
  - *The posted-request reader waits for step 3*, its first user.
