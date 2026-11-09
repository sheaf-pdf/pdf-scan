# twain-dsm

Layer: Devices (`crates/README.md`). Paths are from `docs/pm/`.

- **What:** TWAIN through the Data Source Manager, loaded at run time (M5 proposal 7, M5 ask 3, entry 6, D17):
  the system's `TWAINDSM.dll` (installed by drivers), else one the caller names (the app's bundled copy).
  - **Loading:** through `LoadLibraryExW` with a system-folder search, never by bare name (that searches
    the application's folder first), and the official DSM's Authenticode signature checked: the
    specification says it's signed and that applications link to it at run time. DSM 2.5.1 (2023) is the
    latest.
  - **Bindings** ported from the TWAIN Working Group's own C# translation, `TWAIN.cs` (MIT, Kodak Alaris;
    current to 2.5, with layouts for Windows, Linux and macOS), keeping its notice, every value checked
    against `twain.h` 2.5. The TWAIN Toolkit's warranty disclaimer added in the source and the docs too (the
    TWAIN License's condition; it sits beside MIT and Apache-2.0 without conflict). One bindings module for
    both bitnesses: RSWebTWAIN's two hand-kept copies diverged, and needed six constants corrected.
  - **TWAIN's seven states as types** (from 1, before the DSM is loaded, to 7, transferring): each state a
    struct that consumes `self` to step and steps down on `Drop`, RSWebTWAIN's design (MIT OR Apache-2.0),
    taken as a design, not its code.
  - **The window:** an STA thread of its own, every DSM call on it (the specification asks one thread); a
    real hidden top-level window as the parent, not a message-only one (`HWND_MESSAGE`), and a child or
    owner window for the driver's dialogs; each message through `MSG_PROCESSEVENT`; the source enabled with
    `modal = false`. NAPS2's 8.6.0 cured its TWAIN hangs with the STA thread (study only).
  - **Capabilities:** `ICAP_XRESOLUTION`, `ICAP_PIXELTYPE`, `CAP_FEEDERENABLED`, `CAP_DUPLEXENABLED`,
    `ICAP_SUPPORTEDSIZES` or a frame, `CAP_XFERCOUNT = -1`, `CAP_FEEDERLOADED`; `ShowUI`, and
    `CAP_ENABLEDSUIONLY` for the driver's own settings window. Containers allocated through
    `DSM_MemAllocate` (else `GlobalAlloc`), never a Rust allocation (RSWebTWAIN passes a `Box`); their
    encoding ported from `TWAIN.cs` or NTwain (MIT).
  - **Transfers:** memory by default, native per driver (NAPS2: Kyocera Ecosys); memory-transfer tiles
    copied at their `XOffset` and `YOffset`; only the first `BitsPerSample` entry trusted ("some scanners
    only set the first value"). No permissive code handles tiles.
  - **Cancel by state:** a forced step down in states 6 and 7; in state 5 a natural cancel, since forcing it
    there draws an error from some scanners.
  - Condition codes: `TWCC_PAPERJAM`, `TWCC_PAPERDOUBLEFEED`, `TWCC_NOMEDIA`, cover open.
  - Both bitnesses: built for `x86_64-pc-windows-msvc` and `i686-pc-windows-msvc`. A source loads only into
    a process of its own bitness, and the office's fi-6130s have a 32-bit driver (M5 ask 12).
- **For the seam** (`crates/scan-device-seam.md`): per transfer, its image info (`DAT_IMAGEINFO`:
  size, resolution, pixel type, bits per pixel); the pending-transfer count; the extended image info the
  source offers (a page's side, patch codes for FEAT-5), all as TWAIN gives them. Whether a memory transfer
  comes back assembled or as tiles is the seam note's to settle, after S4's spike.
- **Prior art:** `twain` 0.1.0 (2021, the only TWAIN crate) loads a DLL by a relative path, passes an
  immutable local as an out-pointer, never calls `GlobalLock`, and bundles the DSM and Qt's `qwindows.dll`
  without their LGPL text: study only. TWAIN Direct has had no commit since 2022: study only. pytwain is
  GPL-2.0 and Saraff.Twain LGPL: study only.
- **Boundary:** TWAIN alone, Windows alone (TWAIN on macOS is gone, M5 research).
- **Tested by:** the mapping of states and codes in unit tests; the TWAIN Working Group's sample data source
  (`Twain_DS_sample01`, BSD-3; it reads its images from disk, its page count from a `CAP_XFERCOUNT`
  environment variable) on `windows-latest`, through its MSI installers, yushulx/virtual-scanner's portable
  zip (which keeps the BSD notice), or the DSM and the source copied into `twain_32` and `twain_64`, if any
  installs silently and scans with a hidden window (S4's spike: unverified); by hand on a real driver.
- **Open:**
  - `twain.h`'s license, settled 2026-11-08: no copy grants a license (copyright notices, "All rights
    reserved", outside the DSM's LGPL); the only grant is twain.org's TWAIN License, whose reach to a Rust
    transcription is for counsel. `TWAIN.cs` makes it moot.
  - Whether the fi-6130's FUJITSU TWAIN32 needs a visible parent window or a modal enable: the hardware.
  - Whether the crate offers the 64-bit app's view of 32-bit sources (it can't load them), or leaves the
    merge to `scan-worker` (leaning: the worker, proposal 4).
