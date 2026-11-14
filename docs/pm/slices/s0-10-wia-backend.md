# S0 slice 10: scan-device's wia backend and scan-worker's wia feature

Status: active, started 2026-11-14. Milestone: `milestones/s0-devices.md`, slice 10. Run on the Windows machine,
the same way as slices 7, 8 and 9 (`decisions.md`): empirical COM probes, this plan, an adversarial review
(folded in below), four implementation steps (each committed and pushed on `s0-10-wia-backend`), parallel code
reviews, and a fix stage.

Paths: `docs/pm/` files from `docs/pm/`; the code's from the repository's root with a leading `/`.

## What it builds

1. **`scan-device`'s `wia` backend (`scan_device::wia::WiaBackend` behind `feature = "wia"`):**
   - WIA 2.0 (`IWiaDevMgr2`, `IWiaItem2`, `IWiaPropertyStorage`, `IWiaTransfer`, `IWiaTransferCallback`,
     `IStream`) through `windows = "0.62"` and `windows-core = "0.62"` (`0.62.2`, matching GPUI's minor line and
     compiling cleanly on MSRV 1.89; `windows 0.100` needs Rust 1.95). Zero `windows` types in `scan-device`'s
     public API.
   - Builds on every OS: on Windows it runs WIA 2.0 COM calls on a process-wide lazy STA `Session`
     (`COINIT_APARTMENTTHREADED`), keeping at most one device open at a time across the process (WSD drivers
     fault with two); off Windows `list` returns `Err(ScanError::NotInstalled { what: "WIA".into() })`, and
     `open` validates `BadId` first (`id::device_id`) before returning `NotInstalled`, matching `SaneBackend`
     and `TwainBackend` and preserving `scan_worker::serve` multi-backend dispatch.
   - `#![deny(unsafe_code)]` at `/crates/scan-device/src/lib.rs`, with `#[allow(unsafe_code)]` confined strictly
     to `/crates/scan-device/src/wia/com.rs` (`#[cfg(windows)]`), every `unsafe` block carrying a `// SAFETY:`
     invariant (`unsafe_op_in_unsafe_fn = "deny"` and `clippy::undocumented_unsafe_blocks = "deny"`).
   - Every pure layer (`id.rs`, `error.rs`, `driver.rs`, `caps.rs`, `settings.rs`, `pixels.rs`, `batch.rs`,
     `session.rs`) is 100% safe Rust and compiled on Linux, macOS, and Windows behind an internal `Driver` trait
     (`milestones/s0-devices.md`: *"Checked against a fake of the WIA calls behind a trait (no virtual WIA device
     installs without a person)"*), so CI exercises the entire WIA state machine, property constraints, clamping,
     BMP/DIB/`WIA_RAW_HEADER` decoding, encoded pass-through, `WarmingUp`, `WIA_STATUS_END_OF_MEDIA`, error
     tables, and sheet-granular cancellation on every runner, plus live Windows COM vtable and `stisvc` tests on
     `windows-latest` in both 64-bit and 32-bit (`i686-pc-windows-msvc`).
2. **Shared safe DIB / `.bmp` decoder (`/crates/scan-device/src/dib.rs`):**
   - Factor `/crates/scan-device/src/twain/pixels.rs`'s `BITMAPINFOHEADER` DIB validation and decoding into a
     single unified entry surface in `/crates/scan-device/src/dib.rs` (`#[cfg(any(feature = "twain", feature =
     "wia"))]`, zero single-feature `dead_code`), supporting both packed header-only DIBs (`DAT_IMAGENATIVEXFER`
     and `WiaImgFmt_MEMORYBMP`) and complete `.bmp` streams with a 14-byte `BITMAPFILEHEADER` (`WiaImgFmt_BMP`
     and `"BM"`-prefixed `WiaImgFmt_MEMORYBMP` streams), honoring `bfOffBits` from byte `0` (with `bfOffBits ==
     0` fallback), `BI_RGB`, and `16`/`32`-bit `BI_BITFIELDS` channel masks (accounting for `biSize == 40`
     trailing 12-byte masks vs `biSize >= 52` embedded `BITMAPV4`/`V5` masks).
3. **`scan-worker`'s optional `wia = ["scan-device/wia"]` feature and worker tests:**
   - Optional `wia = ["scan-device/wia"]` feature on `scan-worker` (and included in `fake` for `scan-worker-fake`),
     `wia` scenarios in `/crates/scan-worker/src/bin/scan-worker-fake/`, and end-to-end process-isolated tests in
     `/crates/scan-worker/tests/wia_worker.rs` covering `Driver::Wia` listing, opening, caps, raw and encoded
     batches, mid-sheet cancel, driver stdout chatter isolation, and `NotInstalled` / error propagation in both
     64-bit and 32-bit (`i686`).
4. **CI, `justfile`, and `docs/pm/` sync:**
   - Wire `wia` into `/justfile` and `/.github/workflows/ci.yml` (64-bit and `i686-pc-windows-msvc` tests and
     clippy, MSRV 1.89 on Ubuntu and Windows, and `just gates` across all features), and update `/README.md`,
     `crates/scan-device-seam.md`, `crates/scan-device.md`, `crates/scan-worker.md`, `milestones/s0-devices.md`,
     `status.md`, `plan.md`, and `slices/README.md`.

Not built here: WIA 1.0 (`IWiaDevMgr`, obsolete since Vista and 32-bit-only for native acquisition); `ica` on
macOS; freezing `scan-device` and `scan-worker` at `0.1.0` (slice 11); physical scanner hand-checks (slice 12).

## Empirical findings before planning (2026-11-14)

Probed in an isolated throwaway binary outside the repository on this Windows 10/11 host against `windows`
`0.62.2` and `windows-core` `0.62.2` (`cargo +1.89 run` and `--target i686-pc-windows-msvc`):

1. **MSRV and dependency gate:** `windows 0.62.2` + `windows-core 0.62.2` with features
   `["Win32_Devices_ImageAcquisition", "Win32_Foundation", "Win32_System_Com",
   "Win32_System_Com_StructuredStorage", "Win32_System_Variant"]` compile cleanly on Rust `1.89` and add only
   `windows*` and proc-macro crates (zero image codecs, PDF, OCR, or GUI crates under `just gates`). Because
   `scan-device` is the only workspace member using `windows` / `windows-core`, `/Cargo.toml`'s rule (*"A
   dependency goes in `[workspace.dependencies]` once a second member uses it"*) keeps them in
   `/crates/scan-device/Cargo.toml` under `[target.'cfg(windows)'.dependencies]`.
2. **COM teardown ordering (`0xC0000005` trap):** `IWiaDevMgr2` (`CLSCTX_LOCAL_SERVER`) loads out-of-process RPC
   proxy DLLs (`sti.dll` / `wiarpc`) into the STA apartment. Calling `CoUninitialize()` before dropping local COM
   pointers unloads the proxy DLL first, causing `Drop` (`IUnknown::Release`) to jump into unmapped vtable
   memory (`STATUS_ACCESS_VIOLATION`). Declaring an RAII `ComGuard` *first* on the STA thread (calling
   `CoUninitialize()` only when `CoInitializeEx` returned `S_OK` or `S_FALSE`, after every COM interface,
   enumerator, `PROPVARIANT`, and `BSTR` on the thread has dropped) eliminates the fault in both 64-bit and
   32-bit.
3. **`windows 0.62.2` high-level `HRESULT` wrapper trap:** The generated `IWiaTransfer::Download` wrapper calls
   `HRESULT::ok()`, mapping **all** non-negative `HRESULT`s (`S_OK = 0`, `S_FALSE = 1`,
   `WIA_STATUS_END_OF_MEDIA = 0x00210001`, `WIA_STATUS_WARMING_UP = 0x00210002`,
   `WIA_STATUS_CALIBRATING = 0x00210003`) to `Ok(())` and discarding the status code. Calling the raw vtable
   slot `(Interface::vtable(&transfer).Download)(Interface::as_raw(&transfer), 0, Interface::as_raw(&cb))`
   preserves the exact `HRESULT`. Likewise, `IWiaPropertyStorage::ReadMultiple` returns `S_FALSE` (`Ok(())`) when
   some queried properties don't exist on a device or item (leaving missing entries cleanly as `VT_EMPTY` /
   `VARENUM(0)`), and `IEnumWIA_DEV_INFO::Next` / `IEnumWiaItem2::Next` / `IEnumWIA_FORMAT_INFO::Next` map
   `S_FALSE` (end of enumeration) to `Ok(())`, requiring `let mut fetched = 0u32;` before every `Next(1, ...,
   &mut fetched)` call and breaking when `fetched == 0` or the output interface is `None`.
4. **Returning `S_FALSE` from `#[implement(IWiaTransferCallback)]`:** `windows-result 0.4.1` stores non-zero
   `HRESULT`s as `NonZeroI32`, so returning `Err(windows_core::Error::from_hresult(S_FALSE))` from
   `IWiaTransferCallback_Impl::{TransferCallback, GetNextStream}` returns `HRESULT(1) == S_FALSE` across the COM
   vtable ABI, verified by direct vtable invocation in both 64-bit and `i686`. Both `#[implement(IStream)]` and
   `#[implement(IWiaTransferCallback)]` compile with zero warnings under `unsafe_op_in_unsafe_fn = "deny"`,
   `missing_docs = "warn"`, and `clippy::undocumented_unsafe_blocks = "deny"`.
5. **Live `stisvc` enumeration, WSD properties, and `CreateDevice` `HRESULT`s:**
   - `EnumDeviceInfo(WIA_DEVINFO_ENUM_LOCAL)` (`0x10`) returns only active/connected scanners (`0` when offline),
     whereas `WIA_DEVINFO_ENUM_ALL` (`0x0F`) includes offline historical device nodes. `list` therefore uses
     `WIA_DEVINFO_ENUM_LOCAL`.
   - Installed STI scanner nodes carry IDs shaped `{6BDD1FC6-810F-11D0-BEC7-08002BE2092F}\NNNN`, `WIA_DIP_DEV_TYPE =
     0x0001_0001` (`HIWORD == 1 == StiDeviceTypeScanner`), `WIA_DIP_DEV_NAME`, `WIA_DIP_VEND_DESC`, and
     `WIA_DIP_DEV_DESC`. On Microsoft's inbox WSD scan driver, `WIA_DIP_VEND_DESC` is `"Microsoft"` and
     `WIA_DIP_DEV_DESC` is `"WSD Scan Device"` for every WSD copier while `WIA_DIP_DEV_NAME` holds the friendly
     name, proving WIA cannot safely fall back across device IDs by `(maker, model)` (`Opened::renamed_from` is
     always `None`, matching `crates/scan-device-seam.md` §1).
   - Calling `IWiaDevMgr2::CreateDevice(0, &id)` on an offline installed scanner returns `0x80210005`
     (`WIA_ERROR_OFFLINE`), whereas calling `CreateDevice` on a nonexistent/uninstalled ID returns **`0x80210015`
     (`WIA_S_NO_DEVICE_AVAILABLE`)** — the same numeric value `WiaDef.h` reuses for
     `WIA_ERROR_MAXIMUM_PRINTER_ENDORSER_COUNTER` during transfers! Thus `error.rs` distinguishes device-open /
     enumeration context (`0x80210015` -> `ScanError::Gone`) from scan/transfer context (`0x80210015` and Learn's
     `0x80210021` -> `ScanError::Driver`).
6. **`WIA_IPA_TYMED` and `WIA_RAW_HEADER` ground truths (`WiaDef.h` + WDK docs):**
   - In WIA 2.0 stream-based transfers (`IWiaTransfer::Download`), single-page stream formats use
     **`TYMED_FILE` (`2`)** (mandatory for all WIA 2.0 minidrivers with `WiaImgFmt_BMP`), legacy banded memory
     bitmaps use `TYMED_CALLBACK` (`128`) with `WiaImgFmt_MEMORYBMP`, and `TYMED_MULTIPAGE_FILE` (`256`) packs
     all pages into one multi-page stream (never selected here, since `scan-device` delivers one `RawPage` per
     stream). Valid `WIA_IPA_FORMAT` GUIDs depend on `WIA_IPA_DATATYPE`, `WIA_IPA_DEPTH`, and `WIA_IPA_TYMED`.
   - `WIA_PROP_LIST` attribute vectors (`IWiaPropertyStorage::GetPropertyAttributes`) begin with two header
     slots (`WIA_LIST_COUNT = 0`, `WIA_LIST_NOM = 1`); actual list elements start at `WIA_LIST_VALUES = 2`
     (`WiaDef.h:1741-1775`).
   - `WIA_IPS_AUTO_DESKEW` uses `WIA_AUTO_DESKEW_ON = 0` and **`WIA_AUTO_DESKEW_OFF = 1`** (`WiaDef.h:851-852`);
     `WIA_IPS_BLANK_PAGES` uses `WIA_BLANK_PAGE_DETECTION_DISABLED = 0` (`WiaDef.h:1054`); `WIA_IPS_PAGE_SIZE`
     uses `WIA_PAGE_CUSTOM = 2` (`WiaDef.h:2053`) and `WIA_PAGE_AUTO = 100` (`WiaDef.h:2143`).
   - `WIA_IPS_PHOTOMETRIC_INTERP` uses `WIA_PHOTO_WHITE_1 = 0` (`0` = black, `1` = white, matching `RawImage`)
     and `WIA_PHOTO_WHITE_0 = 1` (`0` = white, `1` = black, requiring inversion on `Bw1`/`Gray8`/`Gray16`;
     `WiaDef.h:2203-2204`).
   - In `WIA_RAW_HEADER` (`80` bytes, tag `b"WRAW"`), `HeaderSize >= 80`, and `RawDataOffset` / `PaletteOffset`
     are byte offsets starting at `0` where the header ends (`HeaderSize + RawDataOffset`).

## Adversarial plan review (2026-11-14)

Reviewed before Step 1 (`ec9bf8be`), folding in all 14 findings (5 Critical, 6 Major, 3 Minor):

- **C1 (`PROPVARIANT` `Drop` + `[out]` overwrite & `VT_CLSID` safety):** `src/wia/com.rs` includes a
  compile-time `const _: () = assert!(core::mem::needs_drop::<PROPVARIANT>());` check, passes a freshly
  defaulted `vec![PROPVARIANT::default(); n]` (`VT_EMPTY`, never `MaybeUninit` or reused buffers) to every
  `ReadMultiple` and `GetPropertyAttributes` call, and constructs `VT_CLSID` values exclusively via
  `InitPropVariantFromCLSID` (`CoTaskMemAlloc`).
- **C2 (No unwinds across `extern "system"` COM vtables + pointer/overflow hygiene):** `windows-implement`
  vtable shims have no `catch_unwind`; every `IStream_Impl` and `IWiaTransferCallback_Impl` method wraps its body
  in `std::panic::catch_unwind` (latching any panic payload onto callback state and returning `E_FAIL`).
  `BoundedStream` guards `cb == 0` before `core::slice::from_raw_parts`, handles nullable
  `pcbwritten`/`pcbread`/`plibnewposition`, zero-initializes `STATSTG` (`pwcsName` null so COM RPC marshaling
  never frees garbage), checks `Seek`/`SetSize`/`Write` in `u64` against `MAX_STREAM_BYTES` before casting to
  `usize` on `i686`, and `TransferCallback`/`GetNextStream` check null pointers and write `*ppdestination = None`
  on `S_FALSE`.
- **C3 (`WiaImgFmt_BMP` dual-role split via `TransferMode` + `Clamped::Encodings` parity):** `settings.rs`
  returns an explicit `TransferMode { RawBmp, RawMemoryBmp, RawWia, Encoded(Encoding) }` alongside `Applied` so
  `pixels.rs` never guesses between raw `WiaImgFmt_BMP` and `Pixels::Encoded(EncodedKind::Bmp)`.
  `applied.settings.encodings` contains the subset of `asked.encodings` offered by `SourceCaps::encodings`,
  compatible with the applied `Format` (`Jpeg`: `Gray8 | Rgb8`; `Bmp`: `Bw1 | Gray8 | Rgb8`; `Png`/`Tiff`: all
  five formats), and accepted by the driver; `Clamped::Encodings(asked.encodings.clone())` is emitted iff
  `applied.settings.encodings != asked.encodings`, satisfying `scan-worker`'s `check_applied` and `Order::take`.
- **C4 (WIA 2.0 property write dependency order, stepped `XPOS`/`XEXTENT`, and `Paper::Auto` readback):**
  `settings.rs` writes properties in strict WDK dependency order:
  1. Sticky resets (`BLANK_PAGES = 0`, `AUTO_DESKEW = 1`, `ROTATION = 0`, `PREVIEW = 0`) + source handling
     (`PAGES`, `DOCUMENT_HANDLING_SELECT`).
  2. `WIA_IPA_DATATYPE`, then re-read `WIA_IPA_DEPTH` constraint and write `WIA_IPA_DEPTH`.
  3. `WIA_IPA_TYMED`, then write `WIA_IPA_FORMAT` (falling back to raw if an encoded GUID is rejected).
  4. `WIA_IPS_XRES`, then re-read `WIA_IPS_YRES` constraint and write `WIA_IPS_YRES`.
  5. For `Paper::Size`: disable `WIA_IPS_AUTO_CROP` (`0`) and set `WIA_IPS_PAGE_SIZE = WIA_PAGE_CUSTOM` (`2`)
     first, then query post-DPI `XPOS`, `YPOS`, `XEXTENT`, `YEXTENT` constraints, write `XPOS = xpos.min` and
     `YPOS = ypos.min`, re-query `XEXTENT`/`YEXTENT` and snap to `min + k * step` within `[min, max]`, and read
     back `Applied` (`0.005` mm tolerance). For `Paper::Auto` when supported: enable `WIA_PAGE_AUTO` (`100`) or
     `WIA_AUTO_CROP_SINGLE` (`1`, resetting extents to full bed `max` first so no prior crop clips auto-crop)
     and keep `Applied.settings.paper = Paper::Auto` (never reading `0` extents under `WIA_PAGE_AUTO` into
     `Paper::Size`); when `Paper::Auto` is unsupported (or a caller passes non-positive/non-finite `SizeMm`),
     clamp to `max_area` (`Clamped::Paper(asked.paper)`).
- **C5 (Duplex `WIA_IPS_PAGES` side count, cross-`Download` `SheetTracker`, and deferred `Event::Sheet`):**
  In WIA 2.0, `WIA_IPS_PAGES` counts *sides (images)* (`1` duplex sheet = `2` pages, often `step = 2`). When `0`
  is rejected on `Feeder`, `settings.rs` snaps to `min + k * step <= max`, preferring an even count `>= 2` in
  duplex whenever `max >= 2`. `SheetTracker` persists across the outer `Download` loop (continuing only when
  `hr == S_OK && streams_in_call == capped_n`), `GetNextStream` checks `progress.check()` **only** when starting
  a new sheet (`!tracker.mid_sheet()`), and `Event::Sheet(n)` is emitted **only** when sheet `n`'s first page has
  finished transferring and decoding (so 0-byte `GetNextStream` calls before `WIA_ERROR_PAPER_EMPTY` never emit
  an orphan `Event::Sheet`).
- **M1–M6 & m1–m3:**
  - `dib.rs`: `BI_BITFIELDS` (`3`) applies to `16`- and `32`-bit DIBs (never `24`-bit); trailing 12-byte masks
    follow `biSize` only when `biSize == 40` (embedded inside `biSize` when `biSize >= 52`); `bfOffBits` is
    measured from byte `0` of `.bmp`; unified entry API avoids single-feature `dead_code` and auto-detects `"BM"`
    headers on `WiaImgFmt_MEMORYBMP`.
  - `wia/pixels.rs`: requires `HeaderSize >= 80` and `Compression == 0`; treats `BytesPerLine == 0` as packed
    row bytes; inverts only `Bw1 | Gray8 | Gray16` on `WIA_PHOTO_WHITE_0` (masking `Bw1` padding bits after
    inversion); swaps B and R on `WIA_DATA_RAW_BGR` for both 24- and 48-bit; and falls back to applied
    `(xres, yres)` whenever stream header DPI is `0` or non-positive so `RawPage.dpi` is always finite and `> 0`.
  - `wia/com.rs` & `wia/batch.rs`: latches the first negative `WiaTransferParams::hrErrorStatus` so generic
    `E_FAIL` from `Download` doesn't hide `Jammed`/`CoverOpen`/`FeederEmpty`; marks `BoundedStream` failed on
    `STG_E_MEDIUMFULL` so truncated streams are never decoded; finalizes unclosed non-empty error-free streams on
    `END_OF_TRANSFER`; handles `SyncSender` receiver drop (`S_FALSE` + `WIA_CMD_STOP_FEEDER` ->
    `Ok(Done::Cancelled)`); and restricts `TYMED_CALLBACK` -> `TYMED_FILE` fallback strictly to 0 pages delivered
    and parameter/medium refusal `HRESULT`s (`E_INVALIDARG`, `WIA_ERROR_INVALID_PARAMETER`, `E_NOTIMPL`,
    `WIA_ERROR_INCORRECT_HARDWARE_SETTING`).
  - `wia/id.rs` & `wia/session.rs`: `BadId` is checked before `NotInstalled` on every OS; `Session` is a
    process-wide lazy singleton (`Mutex<Weak<Session>>`) with `StaActivity` + `OpenGate` tracking (like
    `twain/session.rs`) so multiple `WiaBackend` instances share the single-open-device WSD guard and timed-out
    WSD calls never spawn concurrent `CreateDevice` RPCs (`list` runs normally while an open device is idle and
    returns `Busy` / `TimedOut` while an STA call is in flight).
  - `wia/caps.rs`: upholds every `scan-worker` `check_caps` invariant (deduplicates `Flatbed`/`Feeder`, checks
    `WiaItemTypeFolder` before recursing into `EnumChildItems`, maps `<= 0` physical sizes to `max_area: None`,
    filters `0` DPI, guards `step == 0`, skips sources with empty `formats` or `resolutions`, and returns
    `Err(ScanError::Unsupported)` when zero usable sources exist).
  - Docs & CI: plain code spans `` `scan_device::wia::WiaBackend` `` in `lib.rs` so `just doc` default-features
    builds pass with `-D warnings`; permissive attribution comments (`printscan-windows`, `NAPS2.Wia`); 64- and
    32-bit WIA recipes run in CI's Windows job (where `i686-pc-windows-msvc` is installed).

## Design & Architecture

### Module layout under `/crates/scan-device/src/`

- `dib.rs` (`#[cfg(any(feature = "twain", feature = "wia"))]`): shared safe DIB / `.bmp` decoder used by both
  `twain::pixels` and `wia::pixels`.
- `wia/mod.rs`: public `WiaBackend` (`new()`, `Default`) and opaque `WiaDevice` implementing `Backend` and
  `Device` (`Send`, no `windows` types exposed).
- `wia/id.rs`: `wia:<device_id>` formatting and pre-COM validation (`Driver::Wia`, `"wia:"` prefix, non-empty
  device ID, no `\0` -> `ScanError::BadId`), `StiDeviceTypeScanner` (`1`) filter on `WIA_DIP_DEV_TYPE`
  (`HIWORD` when non-zero, else `LOWORD`, accepting missing `WIA_DIP_DEV_TYPE`), WSD vendor/model fallback
  (`WIA_WSD_MANUFACTURER`, `WIA_WSD_MODEL_NAME` when `WIA_DIP_VEND_DESC` / `WIA_DIP_DEV_DESC` are empty), and
  `(maker, model)` verification on `open` (`ScanError::Gone` if an instance index `\NNNN` was reassigned to
  another model).
- `wia/error.rs`: `WiaOp` (`CreateManager`, `EnumDevices`, `OpenDevice`, `Caps`, `Settings`, `Download`,
  `StopFeeder`) + `HRESULT` (`i32`) mapping to `ScanError`, with permissive attribution to `printscan-windows`
  (MIT OR Apache-2.0) and `NAPS2.Wia` (MIT).
- `wia/driver.rs`: safe, platform-independent internal traits (`WiaSystem`, `WiaDeviceTree`, `WiaSourceItem`,
  `TransferSink`) + pure types (`PropId`, `PropValue`, `PropConstraint`, `FormatPair`, `GuidBytes`) + scripted
  fake WIA driver (`FakeWiaSystem` for cross-platform unit, integration, and `scan-worker-fake` tests).
- `wia/com.rs` (`#[cfg(windows)] #[allow(unsafe_code)]`): thin COM adapter implementing `driver.rs` over
  `IWiaDevMgr2`, `IWiaItem2`, `IWiaPropertyStorage`, `IWiaTransfer`, `BoundedStream` (`#[implement(IStream)]`),
  and `TransferCallback` (`#[implement(IWiaTransferCallback)]`), with `ComGuard`, `needs_drop::<PROPVARIANT>()`,
  and `catch_unwind` at every COM callback boundary.
- `wia/session.rs`: process-wide lazy STA `Session` (`Mutex<Weak<Session>>`) enforcing single-open-device WSD
  exclusivity, live `StaActivity` tracking across timed-out calls, and `OpenGate` cleanup of late-completed
  abandoned opens.
- `wia/caps.rs`: item-tree source discovery and `Caps` / `SourceCaps` construction satisfying all model and
  `scan-worker` invariants.
- `wia/settings.rs`: strict WDK dependency-ordered property negotiation, WSD `WIA_IPS_PAGES` side-count fallback,
  stepped `XPOS`/`YPOS`/`XEXTENT`/`YEXTENT` snapping, `TransferMode`, and readback into `Applied`.
- `wia/pixels.rs`: `WiaImgFmt_BMP`, `WiaImgFmt_MEMORYBMP`, `WiaImgFmt_RAW` (`WIA_RAW_HEADER`), and magic-checked
  `Pixels::Encoded` handling.
- `wia/batch.rs`: `Download` loop, `WarmingUp` status + bounded retry, monotonic byte progress, `SheetTracker`
  (`FRONT_FIRST` / `BACK_FIRST` and cross-`Download` duplex continuity), deferred `Event::Sheet`, `END_OF_MEDIA`
  / `PAPER_EMPTY`, `TYMED_CALLBACK` -> `TYMED_FILE` fallback, and sheet-granular cancel (`S_FALSE` +
  `WIA_CMD_STOP_FEEDER`).

## Steps

### Step 1: `scan-device` WIA foundation, `dib` extraction, IDs, error table, `Driver` trait, COM adapter, and STA session

1. Extract `/crates/scan-device/src/dib.rs` (`#[cfg(any(feature = "twain", feature = "wia"))]`) from
   `twain/pixels.rs`, adding 14-byte `BITMAPFILEHEADER` (`WiaImgFmt_BMP`, `bfOffBits`) and `16`/`32`-bit
   `BI_BITFIELDS` support while keeping all existing `twain` DIB unit tests green and single-feature builds free
   of `dead_code`.
2. Add optional `[target.'cfg(windows)'.dependencies]` `windows = "0.62"` and `windows-core = "0.62"` and
   feature `wia = ["dep:windows", "dep:windows-core"]` in `/crates/scan-device/Cargo.toml` (plus `"wia"` on the
   self dev-dependency so `cargo test -p scan-device` runs WIA tests everywhere). Switch
   `/crates/scan-device/src/lib.rs` to `#![deny(unsafe_code)]`, keeping `#[allow(unsafe_code)]` solely on
   `src/wia/com.rs`.
3. Implement `wia/{mod.rs, id.rs, error.rs, driver.rs, com.rs, session.rs}` with comprehensive unit tests (IDs,
   off-OS `BadId`-before-`NotInstalled`, `StiDeviceTypeScanner` bitfields, WSD metadata fallback, dual SDK/Learn
   `HRESULT` tables, `0x80210015` open-vs-download disambiguation, process-wide single-device `Busy` guard,
   `TimedOut` `OpenGate` / `StaActivity` hygiene) and Windows COM unit tests (`BoundedStream` cap/seek/stat/null
   pointers, `TransferCallback` `S_FALSE` and `catch_unwind`, `ComGuard` drop order, live `WiaDevMgr2` smoke
   test).
4. Gate: `fmt`, `clippy` (64-bit, default-only, `--features twain` alone, `--features wia` alone, and
   `i686-pc-windows-msvc`), `test` (64-bit and `i686`), `doc`, `scan-device-msrv` (`+1.89`), naming check;
   commit and push.

### Step 2: `wia/caps.rs` and `wia/settings.rs` — item tree, capabilities, sticky resets, clamping, and readback

1. Implement `wia/caps.rs`: `Flatbed`/`Feeder` child item matching (`WIA_IPA_ITEM_CATEGORY` + name fallback,
   `WiaItemTypeFolder` recursion, deduplicating `Flatbed`/`Feeder` with `WIA_CATEGORY_FEEDER` preferred over
   `WIA_CATEGORY_FEEDER_FRONT`), `SourceCaps` (`duplex`, `Resolutions::List` skipping `[count, nom]` header and
   `Resolutions::Range` with zero-DPI/zero-step guards, `formats` with 16-bit transfer-format gating, `encodings`
   from `TYMED_FILE` `WIA_FORMAT_INFO`, positive `max_area` in mm from `1/1000` inch item/root properties,
   `paper_auto` from `WIA_IPS_AUTO_CROP` / `WIA_IPS_PAGE_SIZE`), `Err(ScanError::Unsupported)` when zero usable
   sources exist, and `Caps::settings_window = false` (`Device::settings_window` ->
   `Err(ScanError::Unsupported)`).
2. Implement `wia/settings.rs`: WDK dependency-ordered negotiation (`BLANK_PAGES`/`AUTO_DESKEW`/`ROTATION`/
   `PREVIEW` + `PAGES` + `DOCUMENT_HANDLING_SELECT`, then `DATATYPE` -> `DEPTH`, then `TYMED` -> `FORMAT`, then
   `XRES` -> `YRES`, then `Paper::Auto` vs `Paper::Size` `WIA_PAGE_CUSTOM` + stepped `XPOS`/`YPOS`/`XEXTENT`/
   `YEXTENT`), explicit `TransferMode`, and readback into `Applied` and `Clamped` (`0.005` mm tolerance).
3. Add exhaustive unit tests covering property constraints, WSD quirks, duplex even-page snapping, stepped
   extents, `Paper::Auto` readback, `TransferMode` + `Clamped::Encodings` parity, 16-bit gating, and readback
   drift.
4. Gate: `fmt`, `clippy` (64-bit and `i686`), `test` (64-bit and `i686`), `doc`, `scan-device-msrv`; commit and
   push.

### Step 3: `wia/pixels.rs`, `wia/batch.rs`, scripted fake WIA integration tests, and Windows COM transfer tests

1. Implement `wia/pixels.rs`: `WiaImgFmt_BMP` / `WiaImgFmt_MEMORYBMP` decoding via `crate::dib`,
   `WiaImgFmt_RAW` (`WIA_RAW_HEADER`) decoding across `Bw1`, `Gray8`, `Gray16`, `Rgb8`, `Rgb16` (`RAW_RGB` and
   `RAW_BGR`, `WIA_PHOTO_WHITE_1` and `WIA_PHOTO_WHITE_0`, top-down and bottom-up, `BytesPerLine` row stride,
   1-bit padding masking, applied-DPI fallback on zero header DPI, `512 MiB` ceiling), and `Pixels::Encoded`
   magic-header validation.
2. Implement `wia/batch.rs` and wire `WiaDevice::{caps, scan, settings_window}` end to end: `WarmingUp` status
   and retry, `ulTransferredBytes` monotonic byte progress, `WiaTransferParams::hrErrorStatus` latching,
   0-page `TYMED_CALLBACK` -> `TYMED_FILE` fallback on medium refusal, `SideKnown::Inferred` (`FRONT_FIRST` and
   `BACK_FIRST`), deferred `Event::Sheet`, `WIA_STATUS_END_OF_MEDIA` / `WIA_ERROR_PAPER_EMPTY`, WSD
   multi-`Download` continuation across persistent `SheetTracker`, `Flatbed` `Done::OnePage`, receiver-drop
   handling, and sheet-granular cancel (`S_FALSE` after completing both sides of an in-flight duplex sheet +
   `WIA_CMD_STOP_FEEDER`).
3. Add `/crates/scan-device/tests/wia_fake.rs` (cross-platform scripted WIA integration suite exercising every
   feature, error code, WSD quirk, duplex cancel, channel backpressure, and pixel format) and
   `/crates/scan-device/tests/wia_com.rs` (Windows COM vtable tests exercising real `IWiaTransferCallback` +
   `IStream` marshaling, back-patched BMP streams, `STG_E_MEDIUMFULL` overflow refusal, callback panic containment,
   `S_FALSE` cancel, and live `IWiaDevMgr2` listing/offline-open behavior in 64-bit and `i686`).
4. Gate: `fmt`, `clippy` (64-bit and `i686`), `test` (64-bit and `i686`), `doc`, `scan-device-msrv`; commit and
   push.

### Step 4: `scan-worker` `wia` feature, `scan-worker-fake` scenarios, CI / `justfile`, and `docs/pm/` sync

1. Add `wia = ["scan-device/wia"]` (and include `"wia"` in `fake`) in `/crates/scan-worker/Cargo.toml`, expose a
   `#[doc(hidden)]` test/fake constructor on `WiaBackend` (behind `feature = "fake"` + `feature = "wia"`) or
   scripted WIA scenarios in `/crates/scan-worker/src/bin/scan-worker-fake/`, and add
   `/crates/scan-worker/tests/wia_worker.rs` testing `Driver::Wia` over real child worker pipes (listing,
   opening, caps, raw and encoded scans, mid-sheet cancel, noisy driver stdout isolation, error mapping) in
   64-bit and `i686`.
2. Add `scan-device-wia-test` and `scan-worker-wia-test` in `/justfile` and wire them into
   `/.github/workflows/ci.yml`'s Windows 64/32-bit job (where `i686-pc-windows-msvc` is installed) alongside
   `just ci`.
3. Update crate docs and READMEs (`/README.md`, `/crates/scan-device/src/lib.rs`,
   `/crates/scan-device/README.md`, `/crates/scan-worker/src/lib.rs`, `/crates/scan-worker/README.md`) and sync
   `docs/pm/` (`crates/scan-device-seam.md`, `crates/scan-device.md`, `crates/scan-worker.md`,
   `milestones/s0-devices.md`, `status.md`, `plan.md`, `slices/README.md`).
4. Gate: full local suite (`fmt`, `clippy`, `test`, `doc`, `msrv`, 64-bit and `i686` Windows recipes,
   `twain-dsm-package`, `public-api`, naming check); commit and push.

## Decisions taken alone, for review

1. **`#![deny(unsafe_code)]` on `scan-device` with `#[allow(unsafe_code)]` confined to `src/wia/com.rs`:**
   unlike SANE (`sane-dl`) and TWAIN (`twain-dsm`), `decisions.md` ("Build or adopt", entry 7, and "Ask 2, the
   cut") puts WIA directly on `windows = "0.62"` inside `scan-device` rather than in a separate binding crate,
   mirroring `scan-worker`'s `#![deny(unsafe_code)]` + `src/sys.rs` confinement. Every other module in
   `scan-device` stays 100% safe Rust.
2. **`windows = "0.62"` and `windows-core = "0.62"` kept in `/crates/scan-device/Cargo.toml`:** per
   `/Cargo.toml`'s workspace-dependency rule, single-consumer dependencies stay in their crate's manifest until a
   second workspace member uses them.
3. **No fallback across WIA device IDs (`Opened::renamed_from` always `None`):** STI device IDs
   (`{6BDD1FC6-810F-11D0-BEC7-08002BE2092F}\NNNN`) look identical for USB scanners and network WSD copiers, and
   Microsoft's WSD scan driver reports `maker = "Microsoft"`, `model = "WSD Scan Device"` for every WSD device
   on the network. Falling back by `(maker, model)` on `WIA_ERROR_OFFLINE` could silently open a different
   network copier, while checking `(maker, model)` on `CreateDevice(id)` still catches a local `\NNNN` slot
   reassigned to another model (`ScanError::Gone`).
4. **`0x80210015` disambiguated by operation (`WIA_S_NO_DEVICE_AVAILABLE` vs
   `WIA_ERROR_MAXIMUM_PRINTER_ENDORSER_COUNTER`):** empirically verified against Windows `stisvc` that
   `IWiaDevMgr2::CreateDevice` returns `0x80210015` for missing/uninstalled device IDs (`ScanError::Gone`),
   whereas during `IWiaTransfer::Download` `0x80210015` (and Learn's `0x80210021`) is the printer/endorser
   counter error (`ScanError::Driver`).
5. **Encoded transfers offered for `TYMED_FILE` single-page formats (`Jpeg`, `Png`, `Tiff`, `Bmp`), raw default
   preferring `WiaImgFmt_BMP` for 1/8-bit and `WiaImgFmt_RAW` for 16-bit:** `TYMED_MULTIPAGE_FILE` is never
   selected so every stream corresponds to one `RawPage`. `TransferMode` disambiguates `WiaImgFmt_BMP` requested
   via `Encoding::Bmp` from raw `WiaImgFmt_BMP` decoding when `Settings::encodings` omits `Bmp`.

## For review (security)

1. **Custom size-capped `BoundedStream` (`#[implement(IStream)]`) with `catch_unwind` at every COM boundary:**
   avoids unbounded `CreateStreamOnHGlobal` allocations by enforcing `MAX_STREAM_BYTES` (`512 MiB + 64 KiB`) in
   checked `u64` arithmetic inside `Write`, `Seek`, and `SetSize` before growing `Vec<u8>` (`try_reserve`,
   returning `STG_E_MEDIUMFULL` on overflow or past cap, including on 32-bit `i686`), zero-initializing `STATSTG`
   (`pwcsName` null), guarding zero-length/null raw pointers, and catching panics before `extern "system"` ABI
   boundaries.
2. **Untrusted driver headers (`WIA_RAW_HEADER`, `BITMAPFILEHEADER`, `BITMAPINFOHEADER`, `PROPVARIANT` arrays):**
   every dimension, offset (`HeaderSize + RawDataOffset`, `bfOffBits`), `BI_BITFIELDS` mask, row stride, palette
   index, and `CA*` count (`WIA_LIST_VALUES = 2` header check) is validated in checked `u64`/`usize` arithmetic
   before slicing or allocating, and encoded streams verify non-empty magic bytes before delivery.
3. **COM pointer & `PROPVARIANT` lifetime hygiene:** `ComGuard` drops last on the STA thread so all out-of-proc
   proxy pointers release before `CoUninitialize()`; `needs_drop::<PROPVARIANT>()` is asserted at compile time,
   `ReadMultiple`/`GetPropertyAttributes` always use fresh `VT_EMPTY` vectors, and `VT_CLSID` values are built
   via `InitPropVariantFromCLSID` (`CoTaskMemAlloc`).
