# scan-device

Layer: Devices (`crates/README.md`). Paths are from `docs/pm/`.

- **What:** the device model of M5 proposal 9, as `crates/scan-device-seam.md`'s "The model, for slice 3"
  writes its types and traits down (S0 slice 2, for review), and the backends behind it.
  - `DeviceInfo` (an id such as `sane:fujitsu:fi-7160:12345`, `twain64:PaperStream IP fi-7160`, `wia:{…}`,
    opaque and compared whole, a SANE name's bytes read as Latin-1; name, maker, model, driver), `Caps` (a
    list of sources, flatbed or feeder, each with its duplex, its resolutions as a list or a range, its
    formats, its largest area in millimetres and whether the driver finds the paper's size; and whether the
    driver has a settings window of its own), `Settings` (source, duplex, dpi, format, paper: the driver's own
    size detection or a size), each clamped to what the device can do, and what was clamped said (`Applied`).
  - `RawPage` (the image as the driver gave it: a `RawImage` in the note's raw layout, which its checked
    constructor holds, or encoded bytes with their kind; its resolution, sheet and side, the side reported or
    inferred), `ScanError` (feeder empty, jammed, double feed, cover open, busy, gone, stopped by the
    watchdog, the driver's API not installed, an id the driver doesn't form, the driver's own words and
    whether it showed them), `Done`
    (feeder empty, one flatbed page, cancelled), `Event` (the settings applied, a sheet begun, a page, a
    status such as warming up).
  - Two traits: `Backend` (list under a deadline; open by id, saying when it opened a renamed device in its
    place) and `Device` (caps, scan a batch, the settings window), both `Send`. Synchronous, a batch on a
    thread of its own until it ends, events on a channel (proposal 9, as sheaf's OCR job's are), cancel and
    progress through `in-progress`'s `Progress`, checked between sheets, so a cancel finishes the sheet in
    flight.
- **Backends, each a feature:**
  - `fake`: scripted (proposal 22): pages and sides, a jam at sheet 4 then Continue, a double feed, an
    open cover, an empty feeder at the start, a hang, a crash, a cancel mid-page (the sheet in flight
    finished, none fed after), a cancel after a duplex front (its back still delivered), a stored id opened as
    a renamed device. On every system, in CI. Nothing scripted at this level exists elsewhere.
  - `sane` (Linux), on `sane-dl`: the batch loop and the backend quirks that are about scanning rather
    than the API (proposal 6): `sane_start` per page until `NO_DOCS`, which on the first page means the
    feeder is empty; one flatbed page per press; `page-height` set after `source`; parameters that make no
    sense (Brother's `2500x0`, from one source only: libinsane) end the batch.
    - The feeder and duplex found by what backends call them: `ADF Front`, `ADF Duplex`, `Automatic Document
      Feeder`, a separate `adf-mode` or `duplex`; Canon DR-C240's `ScanMode`; Lexmark's `scan-both-sides`;
      Kodak `kds_s2000`'s string `duplex` = `front` or `both`; Brother's sources shaped like `Automatic
      Document Feeder(centrally aligned,Duplex)`; option values translated into the user's language.
    - An `INVAL` from `sane_start` is ambiguous (NAPS2 reads it as offline; proposal 6 reads the
      descriptors again): read them again (`sane-dl` does), set the settings again and start once more, and
      say "offline" (`Gone`) if it repeats.
    - **Ids and the fallback** (the seam note's section 1): an id is `sane:` and the name's bytes as Latin-1,
      given back to `sane_open` byte for byte; one that answers `INVAL` at open falls back to the one device
      of the same backend, vendor and model, if exactly one is listed, and says so (`renamed_from`), else
      `Gone`. The worker that opens a stored id is started with a `ConfigDir` naming that id's backend, or
      none, written `Search::BeforeSystem`, so the listing stays within the backend; an id with one of `dll`'s
      testing prefixes, a leading `:`, or no name at all, or one not Latin-1 or holding a `NUL`, is refused
      before `sane_open` (`BadId`).
    - **Pages into the raw layout** (the note's section 3): 1-bit gray inverted (SANE's 1 is black, the
      model's 0) and its padding bits cleared, 1-bit RGB expanded to 8 bits, 16-bit made little-endian.
    - **A cancel** finishes the sheet in flight: once the progress's check fails, no next sheet is started and
      the batch is dropped (`sane_cancel`); `sane-dl`'s token, which stops at once and loses the sheet in
      flight, isn't the model's cancel.
    - Every list of these quirks is in copyleft frontends (libinsane, NAPS2, simple-scan), so the facts go in
      our own table, a citation per row; nothing of their code.
  - `twain` (Windows), on `twain-dsm`: sheaf's settings as capabilities, `ShowUI` off by default, the
    driver's own window on request ("Scanner settings…"), condition codes as `ScanError`s.
  - `wia` (Windows), on the `windows` crate's `Win32_Devices_ImageAcquisition` (0.62.2, entry 7): its own STA
    thread; the root's `Feeder` child; `WIA_IPS_PAGES = 0`.
    - **Duplex:** `WIA_IPS_DOCUMENT_HANDLING_SELECT` = `DUPLEX` or `FRONT_ONLY` (one, never both) written to
      the Feeder item. `FEEDER` in that property is WIA 1.0's root value, obsolete since Vista; NAPS2 keeps
      the combined value for WIA 1.0's root only.
    - **The batch:** `WIA_STATUS_END_OF_MEDIA` (`0x210001`, named in the SDK metadata though Learn leaves it
      out) ends a feeder batch. `FEED_READY` is a cached reading, wrong either way at times: a hint, not a
      test. A page count can read back while the feeder keeps running: cancel the transfer and send
      `WIA_CMD_STOP_FEEDER`. Progress from bytes, since the percentage peaks near 40 and jumps to 100. One
      device open at a time (some WSD drivers fault with two).
    - **Errors:** matched by the SDK's values and also by Learn's misprinted ones: cover open, lamp off,
      multi-feed and the endorser counter are 0x80210010, 11, 14 and 15 in the SDK, 0x80210016, 17, 20 and
      21 on Learn (Windows' WSD driver returns 0x80210010 for an open cover).
    - These rules are ported from printscan-windows (MIT OR Apache-2.0, measured on Windows' WSD driver)
      and NAPS2.Wia (MIT, which repeats Learn's codes), with credit, every value checked against `windows`.
  - `ica` (macOS), later (entry 8): `objc2-image-capture-core` 0.3.2 (Zlib OR Apache-2.0 OR MIT) used
    directly, missing bindings contributed upstream; the session flow after scanline (MIT). A safe scanning
    layer over ImageCaptureCore is a hole nobody fills.
  - Patch codes the driver reports, optional, as hints beside pixel reading (FEAT-5).
- **Boundary:** no image codecs, imageproc, PDF crate or GUI toolkit under it. A gate keeps it so.
  Dependencies: `in-progress` (cancel and progress, in its public API; taken by S0 slice 3, vetted in S0 slice
  2's plan) and `thiserror`; each backend its binding, behind its feature.
- **Stability:** provisional (0.0.x, unpublished) through S0's Linux slices, with one real backend behind it;
  frozen at 0.1.0 in S0 slice 11, once the `twain` backend has been through it (`decisions.md`, "Made here").
  sheaf takes it no sooner (`sheaf/swaps.md`).
- **The seam:** `crates/scan-device-seam.md`: for each concept of the model, how SANE, TWAIN, WIA and ICA
  deliver it, what the model makes of it, and what's still to settle, with the known unknowns. First version
  2026-11-08, before any driver layer; revised by the TWAIN spike (S0 slice 7) and by S0 slice 2 (its SANE
  rows from `sane-dl`, the raw layout, and the model written down for slice 3), and whenever a backend finds
  the model lacking. It changes proposal 9's model in eleven places (its "Changes from proposal 9's model").
- **Not here:** network scanning. The office's fi-6130, fi-7160 and fi-8170 scan over USB only, as far as
  any source shows, and of sane-airscan's 205 tested devices one desktop document scanner speaks eSCL.
  Network copiers reach sheaf through WIA and sane-airscan already. If it's ever wanted: FEAT-10.
- **Open:**
  - The `windows` crate's version: sheaf wants GPUI's (0.62, entry 7) so the 64-bit build adds nothing; a
    library pins one minor at a time. Follow sheaf's, bumped with it. 0.62.2 (2025-10-06) is the latest.
  - Whether `RawPage` becomes `scan-image`'s page in `scan-image` (a feature there, `From<RawPage>`) or at
    the caller. Leaning: the caller (sheaf's intake), so neither layer names the other.
  - printscan (b1tninja, MIT OR Apache-2.0, first published 2026-11-08) is a contract close in spirit to
    proposal 9 (namespaced ids, `FeederEmpty`, partial pages kept), with WIA and WSD backends and SANE-net
    and eSCL in open PRs. Not a dependency: its pages are encoded images, its `scan` feature puts `image`,
    `png` and `tiff` under the device layer, it needs Rust 1.95, and it's one account's, days old. Study
    its contract; check it again before S0 slices 3 and 9.
