# twain-dsm

Layer: Devices (`crates/README.md`). Paths are from `docs/pm/`.

- **What:** TWAIN through the Data Source Manager, loaded at run time (M5 proposal 7, M5 ask 3, entry 6, D17):
  the system's `TWAINDSM.dll`, else one the caller names (the app's bundled copy). The specification puts
  installing it on the application ("Applications that wish to use access the Data Source Manager, must install
  it themselves"); drivers often install it too, so a machine may have one or not, and an old driver's may be
  old.
  - **Loading:** through `LoadLibraryExW` with a system-folder search, never by bare name (that searches
    the application's folder first), and the official DSM's Authenticode signature checked: the
    specification says it's signed and that applications link to it at run time. Checked by its signer's
    name (`O=TWAIN Working Group`), not its certificate: 2.5.0 and 2.5.1 carry different certificates from
    different DigiCert issuers, both expired and both timestamped, so still valid. DSM 2.5.1 (2023) is the
    latest, 2.5.0 re-signed with no code change. Loading it from System32 in a 32-bit process gets the
    SysWOW64 copy (the spike read its PE header), under the System32 name. `TWAINDSM_LOG` naming a file, with
    `TWAINDSM_MODE=w`, makes the DSM log every triplet: the tool when a driver misbehaves.
  - **What the DSM does that the crate must know** (the spike, S0 slice 7): it loads every source's library
    at `MSG_OPENDSM`, so opening it runs each installed driver's code; it skips Windows' own `wiatwain.ds` on
    purpose; and it changes the process's current directory to each source's folder while loading it.
  - **Bindings** ported from the TWAIN Working Group's own C# translation of `twain.h`, `TWAINH.cs` (MIT,
    Kodak Alaris; current to 2.5, with layouts for Windows, Linux and macOS; `TWAIN.cs` beside it is the
    wrapper over it, not the definitions), keeping its notice, every value checked against `twain.h` 2.5.
    Where they differ, `twain.h`'s layout wins: `TWAINH.cs`'s Windows identity ends in 8 bytes of padding that
    `twain.h` hasn't (`TW_IDENTITY` is 156 bytes). `TWAINH.cs` types every function pointer as an `IntPtr`,
    so their signatures come from `twain.h`: `DSM_Entry`, the callback and `TW_ENTRYPOINT`'s four memory
    functions are all `TW_CALLINGSTYLE`, `__stdcall` on x86. `twain-spike`'s `ffi` is a start: 138 constants
    read from `TWAINH.cs` and checked against `twain.h` by a script, and the layouts asserted for both
    bitnesses. The TWAIN Toolkit's warranty disclaimer added in the source and the docs too (the
    TWAIN License's condition; it sits beside MIT and Apache-2.0 without conflict). One bindings module for
    both bitnesses: RSWebTWAIN's two hand-kept copies diverged, and needed six constants corrected.
  - **TWAIN's seven states as types** (from 1, before the DSM is loaded, to 7, transferring): each state a
    struct that consumes `self` to step and steps down on `Drop`, RSWebTWAIN's design (MIT OR Apache-2.0),
    taken as a design, not its code.
  - **The window:** an STA thread of its own, every DSM call on it (the specification asks one thread); a
    real hidden top-level window as the parent, not a message-only one (`HWND_MESSAGE`), and a child or
    owner window for the driver's dialogs; each message through `MSG_PROCESSEVENT`, or a `DAT_CALLBACK2`
    callback, registered on the source in state 4, that only records, posts to the window and returns
    `TWRC_SUCCESS` (the DSM hands its return to the source); the application's own messages never forwarded;
    the source enabled with `modal = false`. NAPS2's 8.6.0 cured its TWAIN hangs with the STA thread (study
    only). The spike's window procedure is `DefWindowProcW` itself, so no Rust code runs in a callback
    frame.
  - **Capabilities:** `ICAP_XRESOLUTION`, `ICAP_PIXELTYPE`, `CAP_FEEDERENABLED`, `CAP_DUPLEXENABLED`,
    `ICAP_SUPPORTEDSIZES` or a frame, `CAP_XFERCOUNT = -1`, `CAP_FEEDERLOADED`; `ShowUI`, and
    `CAP_ENABLEDSUIONLY` for the driver's own settings window. Containers allocated through
    `DSM_MemAllocate` (else `GlobalAlloc`), never a Rust allocation (RSWebTWAIN passes a `Box`); their
    encoding ported from `TWAINH.cs` or NTwain (MIT). Current values read with `MSG_GETCURRENT` and decoded by
    `ConType`, from the block's bytes as `GlobalSize` measures it, never dereferenced unaligned (an
    enumeration's items start at offset 14).
  - **Transfers:** memory by default, native per driver (NAPS2: Kyocera Ecosys); memory-transfer tiles
    copied at their `XOffset` and `YOffset`; only the first `BitsPerSample` entry trusted ("some scanners
    only set the first value"). One `MSG_XFERREADY` per enable: after `MSG_ENDXFER` with pages pending, the
    application transfers again; the pending count may be 0xFFFF, "don't know", until the last page. The crate
    hands over each buffer's tile and a native transfer's DIB as TWAIN gives them; the `twain` backend
    assembles and decodes them (`twain-spike`'s `image` module, with its checks, is the start; the seam note,
    section 3).
  - **Cancel by state:** in states 6 and 7, the transfer in flight finished, then `MSG_ENDXFER` and
    `MSG_RESET`; `MSG_STOPFEEDER` where a driver has it (the sample hasn't); in state 5 a natural cancel, since
    forcing it there draws an error from some scanners. A cancel from another thread is a message posted to
    the TWAIN thread's window, a flag alone being unable to wake its `GetMessageW`; the loop keeps such
    requests while it waits for the source, and takes only its own window's.
  - **Walking down:** the application's record of the state can lag the source's (the sample is in state 6
    inside `MSG_ENABLEDS`), so a step refused with `TWCC_SEQERROR` restarts the walk from state 7's first step,
    as `TWAIN.cs`'s rollback does; and the DSM is never unloaded while a source is open, since the source
    keeps the DSM's entry points and calls through them. In the typestate, a state that can't step down stays
    alive, not dropped. Seen with DSM 2.5.0: `MSG_CLOSEDS` with an identity it never opened answers success.
  - **Condition codes:** the DSM refuses some source triplets itself (a callback it won't register, a source
    not open) and keeps its own code; a failure asks the DSM first, then the source through it, each once,
    since asking clears it.
  - **Containers' memory functions** are only callable while the DSM that gave them is loaded: tie them to
    the DSM's lifetime (the spike's `Allocator` makes every call `unsafe` instead).
  - Condition codes: `TWCC_PAPERJAM`, `TWCC_PAPERDOUBLEFEED`, `TWCC_NOMEDIA`, cover open.
  - Both bitnesses: built for `x86_64-pc-windows-msvc` and `i686-pc-windows-msvc`. A source loads only into
    a process of its own bitness, and the office's fi-6130s have a 32-bit driver (M5 ask 12).
- **For the seam** (`crates/scan-device-seam.md`): per transfer, its image info (`DAT_IMAGEINFO`:
  size, resolution, pixel type, bits per pixel); the pending-transfer count; the extended image info the
  source offers (a page's side, patch codes for FEAT-5), all as TWAIN gives them. Whether a memory transfer
  comes back assembled or as tiles was the seam note's to settle: settled by the TWAIN spike (S0 slice 7) as
  tiles, and a native transfer's DIB as its block.
- **Prior art:** `twain` 0.1.0 (2021, the only TWAIN crate) loads a DLL by a relative path, passes an
  immutable local as an out-pointer, never calls `GlobalLock`, and bundles the DSM and Qt's `qwindows.dll`
  without their LGPL text: study only. TWAIN Direct has had no commit since 2022: study only. pytwain is
  GPL-2.0 and Saraff.Twain LGPL: study only.
- **Boundary:** TWAIN alone, Windows alone (TWAIN on macOS is gone, M5 research).
- **Tested by:** the mapping of states and codes in unit tests; the TWAIN Working Group's sample data source
  (`Twain_DS_sample01`, BSD-3; it reads its images from disk, its page count from a `CAP_XFERCOUNT`
  environment variable) on `windows-latest`, through its MSI installers, yushulx/virtual-scanner's portable
  zip (which keeps the BSD notice), or the DSM and the source copied into `twain_32` and `twain_64`, if any
  installs silently and scans with a hidden window; by hand on a real driver. **The spike (S0 slice 7)
  settled the route:** `scripts/twain-sample.ps1` installs the sample's MSI silently (`msiexec /qn`, the DSM
  2.5.0 with it, both bitnesses), on `windows-latest` too; and `twain-spike` lists, transfers, compares and
  cancels through it with a hidden window in both bitnesses here. CI's `twain-spike` job runs the same on
  `windows-latest`, green on every push of the slice (`list` at step 1, six scenarios at step 2, all at step 3
  on). `twain-dsm`'s own
  tests replace the spike's, and the spike goes.
- **Open:**
  - `twain.h`'s license, settled 2026-11-08: no copy grants a license (copyright notices, "All rights
    reserved", outside the DSM's LGPL); the only grant is twain.org's TWAIN License, whose reach to a Rust
    transcription is for counsel. `TWAINH.cs` makes it moot.
  - Whether the fi-6130's FUJITSU TWAIN32 needs a visible parent window or a modal enable: the hardware.
  - Whether the crate offers the 64-bit app's view of 32-bit sources (it can't load them), or leaves the
    merge to `scan-worker` (leaning: the worker, proposal 4).
