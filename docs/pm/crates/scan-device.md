# scan-device

Layer: Devices (`crates/README.md`). Paths are from `docs/pm/`.

- **What:** the device model of M5 proposal 9 and the backends behind it.
  - `DeviceInfo` (an id such as `sane:fujitsu:fi-7160:12345`, `twain64:PaperStream IP fi-7160`, `wia:{…}`; name,
    maker, model, driver), `Caps` (sources, flatbed or feeder with or without duplex; resolutions; modes;
    largest area), `Settings` (source, dpi, mode, paper: the driver's own size detection or a size), each
    clamped to what the device can do, and what was clamped said.
  - `RawPage` (the image as the driver gave it, raw pixels with width, height, format and resolution, or
    encoded; its sheet and side), `ScanError` (feeder empty, jammed, double feed, cover open, busy, gone,
    stopped by the watchdog, the driver's own words), `Done` (feeder empty, one flatbed page, cancelled).
  - A `Backend` trait: list, caps, scan a batch, cancel. Synchronous, run on a thread of its own, events on
    a channel (proposal 9, as sheaf's OCR job's are).
- **Backends, each a feature:**
  - `fake`: scripted (proposal 22): pages and sides, a jam at sheet 4 then Continue, a double feed, an
    open cover, an empty feeder at the start, a hang, a crash, a cancel mid-page. On every system, in CI.
    Nothing scripted at this level exists elsewhere.
  - `sane` (Linux), on `sane-dl`: the batch loop and the backend quirks that are about scanning rather
    than the API (proposal 6): `sane_start` per page until `NO_DOCS`, which on the first page means the
    feeder is empty; one flatbed page per press; `page-height` set after `source`; parameters that make no
    sense (Brother's `2500x0`, from one source only: libinsane) end the batch.
    - The feeder and duplex found by what backends call them: `ADF Front`, `ADF Duplex`, `Automatic Document
      Feeder`, a separate `adf-mode` or `duplex`; Canon DR-C240's `ScanMode`; Lexmark's `scan-both-sides`;
      Kodak `kds_s2000`'s string `duplex` = `front` or `both`; Brother's sources shaped like `Automatic
      Document Feeder(centrally aligned,Duplex)`; option values translated into the user's language.
    - An `INVAL` from `sane_start` is ambiguous (NAPS2 reads it as offline; proposal 6 reads the
      descriptors again): read them again, and say "offline" if it repeats.
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
- **Stability:** provisional (0.0.x, unpublished) through S3, with one real backend behind it; frozen at
  0.1.0 in S4, once the `twain` backend has been through it (`decisions.md`, "Made here"). sheaf takes it no
  sooner (`sheaf/swaps.md`).
- **The seam:** `crates/scan-device-seam.md`: for each concept of the model, how SANE, TWAIN, WIA and ICA
  deliver it, what the model makes of it, and what's still to settle, with the known unknowns. First version
  2026-11-08, before any driver layer; revised in S3 step 2, after S4's spike, and whenever a backend finds
  the model lacking. It changes proposal 9's model in seven places (its "Changes from proposal 9's model").
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
    its contract; check it again before S3 and S4.
