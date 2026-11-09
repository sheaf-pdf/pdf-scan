# scan-device

Layer: Devices (`crates/README.md`). Paths are from `docs/pm/`.

**As built** in S0 slice 3 (`archive/slices/s0-3-scan-device.md`, which has the design, its decisions and what each step
checked): `/crates/scan-device/`, 0.0.0, provisional and unpublished (`publish = false`): the model, and the
`fake` backend; no backend for a driver API yet (`sane` is slice 5's). MSRV 1.89 (`decisions.md`). No `unsafe`
(`#![forbid(unsafe_code)]`), and every public item documented (`#![warn(missing_docs)]`). Changed by S0 slice 4
for `scan-worker` (`slices/s0-4-scan-worker.md`): a `serde` feature, and the fake's `Fault::AwaitCancel`
("Slice 4's changes", below).

- **What:** the device model of [M5 proposal 9](https://github.com/sheaf-pdf/sheaf/blob/3232a35/plan/m05-scan/proposals/p09-device-model.md), as `crates/scan-device-seam.md`'s "The model, for slice 3"
  writes its types and traits down (S0 slice 2, for review), built so in S0 slice 3, every name the note's; and
  the backends behind it.
  - `DeviceInfo` (an id such as `sane:fujitsu:fi-7160:12345`, `twain64:PaperStream IP fi-7160`, `wia:{…}`,
    compared whole and read by nobody, a SANE name's bytes read as Latin-1; name, maker, model, driver: what the
    app stores and gives back to open), `Caps` (a list of sources, flatbed or feeder, each with its duplex, its
    resolutions as a list or a range, its formats, its largest area in millimetres and whether the driver finds
    the paper's size; and whether the driver has a settings window of its own), `Settings` (source, duplex, dpi,
    format, paper: the driver's own size detection or a size; and the encoded forms the app takes, none by
    default), each clamped to what the device can do, and what was clamped said (`Applied`). The structs that
    may grow are `#[non_exhaustive]`, made outside the crate by a constructor or by decoding.
  - `RawPage` (the image as the driver gave it: a `RawImage` in the note's raw layout, which its checked
    constructor holds, or encoded bytes with their kind; its resolution, sheet and side, the side reported or
    inferred), `ScanError` (feeder empty, jammed, double feed, cover open, busy, gone, an id the driver doesn't
    form, the settings refused, access refused, a deadline passed, something the device can't do, the driver's
    API not installed, the driver's own words and whether it showed them; the watchdog's stop is
    `scan-worker`'s), `Done` (feeder empty, one flatbed page, cancelled), `Event` (the settings applied, a sheet
    begun, a page, a status such as warming up).
  - Two traits: `Backend` (list under a deadline; open the device the app stored, under a deadline, saying when
    it opened a renamed device in its place) and `Device` (caps, scan a batch, the settings window, owned by the
    app's), both `Send`. Synchronous, a batch on a thread of its own until it ends, events on a bounded channel,
    which holds a backend back while it's full (proposal 9's channel, as sheaf's OCR job's is), cancel and
    progress through `in-progress`'s `Progress`, checked between sheets, so a cancel finishes the sheet in
    flight.
  - **What the crate adds to the note's block** (S0 slice 3, its decisions 3 and 23 to 26), in modules by
    concept (`device`, `caps`, `image`, `page`, `error`), private, everything re-exported at the root:
    - A constructor for each `#[non_exhaustive]` struct, infallible, taking owned values:
      `DeviceInfo::new(id, name, maker, model, driver)` and `Settings::new(source, dpi, format)` (one side,
      `Paper::Auto`, raw only) as the note names them; `Caps::new(sources)` (no settings window),
      `SourceCaps::new(source, resolutions, formats)` (no duplex, no encodings, `max_area` `None`, `paper_auto`
      false), `Applied::new(settings, clamped)` and `RawPage::new(pixels, dpi, sheet, side, side_known)`. The
      rules the docs put on these values (a `dpi` finite and above 0; at least one source, each with at least
      one format; resolutions above 0, a `Resolutions::List` never empty, a `Range`'s `min` not past its
      `max`; a `max_area` finite and above 0, the last two said in the docs by S0 slice 4's code review, as
      the fake already checked them) are each a backend's to keep, the fake's among them, and a decoder's to
      check (slice 4's client); the constructors don't check them (the code review's wording, `Caps`' and
      `RawPage`'s docs).
    - `RawImage::new(format, width, height, data)` refuses, in order: a width or height of 0 (`Empty`); a row's
      bytes or the image's past `u64`, in checked arithmetic, or past what one buffer can hold on the platform,
      `isize::MAX`, the most any allocation can be (`TooLarge`; `usize::MAX` until the code review); a
      `data` of any other length (`Length`); a `Bw1` row whose bits past its last pixel aren't 0 (`Padding`,
      the first such row named). `RawImage::data_len(format, width, height)` gives the bytes such an image
      holds, refused as `new` refuses it, so a reader caps a page before its bytes and allocates once. The
      platform's limit is a parameter of the private function both use, so the 32-bit case is unit-tested on
      any host. The raw layout is stated once, in the crate docs.
    - `Debug` by hand for `RawImage` and `Encoded` (the format or kind, the size, `len`; never a byte), and for
      `Opened` (its `info` and `renamed_from`, the device left out as `..`).
    - `pub use in_progress`, so an app names the very version `Device::scan`'s `Progress` is.
    - From the code review: `Paper` `#[non_exhaustive]`, a departure from the note's block written into it
      (paper may grow: section 2's "To settle"), and the fake's `When` too; the fake's twelve builder methods
      `#[must_use]`, so a script made on a clone and dropped is warned of.
  - **Slice 4's changes** (S0 slice 4, its decisions 2, 3 and 30; the seam note's "What slice 4 needs of it"):
    - **A `serde` feature,** off by default: `Serialize` and `Deserialize` derived on the model's plain data
      (`DeviceId`, `DeviceInfo`, `Driver`, `Caps`, `SourceCaps`, `Source`, `Resolutions`, `SizeMm`,
      `Settings`, `Paper`, `Applied`, `Clamped`, `Format`, `Encoding`, `EncodedKind`, `Side`, `SideKnown`,
      `Status`, `Done`, `ScanError`, `WindowClosed`, `WindowOwner`), in serde's default form (fields and
      variants by their Rust names, an enum externally tagged), with no attribute; not on `RawImage`,
      `Encoded`, `Pixels`, `RawPage` or `Event`, whose bytes don't belong in a text format, so a transport
      carries them beside it, as `scan-worker`'s protocol does. A `NonZeroU32` step of 0 is refused as it's
      read, and JSON writes a size that isn't finite as `null`, which reading refuses; but `serde_json` reads
      a number past an `f32`'s range as an infinity (`1e39`), so a reader of sizes another program wrote
      checks them for being finite itself, as slice 4's client does (its code review). It's how
      `scan-worker`'s protocol carries the model, and how an app stores a `DeviceInfo`; provisional as the
      model is, its names and the fields added after data is stored settled before the freeze (Q-8). Its unit
      tests (`/crates/scan-device/src/round_trip.rs`) take a value of every variant of every enum above,
      each numbered by an exhaustive `match`, so a variant added fails the build until it's listed, and every
      struct with each field set, through JSON and back, equal; the `f32` edges bit for bit; the form pinned
      on literal JSON.
    - **`Fault::AwaitCancel`** in the fake: at a side's start or midway, as any fault, the side waits, sending
      nothing and holding no lock, until the batch is cancelled (checked each millisecond), then goes on, so
      the sheet in flight finishes and a batch from the feeder ends `Done::Cancelled` before the next. A
      cancel lands mid-page by the script rather than by timing, in a test's process or a worker's. Tested at
      a duplex front's midway and at a back's start (`fake_faults.rs`).
- **Backends, each a feature:**
  - `fake`, **built in S0 slice 3**: scripted ([proposal 22](https://github.com/sheaf-pdf/sheaf/blob/3232a35/plan/m05-scan/proposals/p22-testing-without-scanner.md)): pages and sides, a jam at sheet 4 then Continue, a
    double feed, an open cover, an empty feeder at the start, a hang, a crash, a cancel mid-page (the sheet in
    flight finished, none fed after), a cancel after a duplex front (its back still delivered), a stored device
    opened as a renamed one found by its maker and model. On every system, in CI. Nothing scripted at this
    level exists elsewhere.
    - **The feature:** `scan_device::fake`, off by default, for tests and never for a shipped build (its faults
      can block a thread for good and end the process). The crate's own tests, doctests included, turn it on
      through a dev-dependency on the crate itself, so a plain `cargo test` runs them and a build without the
      feature has no fake in it; `cargo doc` and `clippy` take `--all-features` (the justfile), and the crate
      docs name the fake's items in code only, so `cargo doc` without the feature stays clean: `just doc` and
      `just clippy` check that too, each library documented and linted with its default features alone as well
      (the code review). A crate that tests through the fake asks for the feature in its own dev-dependencies:
      `cargo test --workspace` unifies it from this crate's tests, hiding one that forgot (the seam note's "What
      slice 4 needs of it").
    - **Scripts are Rust values,** no text format: `FakeBackend` (`new`, `with(FakeDevice)`,
      `listing_takes(delay)`, `not_installed()`) holds `FakeDevice`s (`new(name, maker, model)`, its id
      `fake:<name>`; `caps`, `unlisted`, `refuses_open`, `reports_sides`, `warms_up`, `settings_window`), each
      also a handle on its paper shared by its clones (`load`, `glass`, `fed`, `in_feeder`), so a test loads
      paper between batches and reads the sheets fed while a batch waits on another thread; the paper is never
      locked across a `send`, a `Progress` call or a hang. A feeder and a glass hold `Sheet`s (`new`, Letter;
      `size`; `side(side, pixels)`, a page given; `fault(side, when, fault)`). `pattern(format, width, height,
      sheet, side)` draws the pages, the same bytes on every system, different by sheet and side, public so
      slice 4 checks its pipe byte for byte against it. By default a device has a feeder with duplex (50 to 600
      dpi in steps of 1, the five formats, Legal, size detection) and a flatbed (75, 150, 300 and 600 dpi,
      the five formats, 215.9 by 297 mm). A script's mistakes panic as they're written (caps the model forbids
      or with a resolution of 0, an empty name, two devices of one id, a sheet's size not finite and above 0,
      `FeederEmpty` scripted as a fault).
    - **Listing and opening** as the note's section 1 and its mapping table have the `sane` backend do it:
      `list` after its scripted delay, `TimedOut` where that isn't under the deadline; `open` answering
      `NotInstalled` where scripted, then `BadId` (not `fake:` and a name), then listing under the deadline; the
      stored id listed with the stored maker and model opened, `info` the listing's; listed as another model,
      taken as gone; unlisted but scripted so, opened with the stored info; taken as gone, a fallback only from
      a USB connection's name (`fake:libusb:<bus>:<device>`) to exactly one listed device of that form with the
      stored maker and model, said in `renamed_from`, else `Gone`; a scripted refusal answered once the device
      is found.
    - **A batch:** the settings clamped to the chosen source's caps, and `Applied` sent with each change and
      what was asked (a source the caps lack, the first listed; duplex off where there's none; the nearest dpi,
      the higher of two as near, never past a range's `max`; a format the source lacks, the first listed;
      paper cut to the largest area, a size with no area `Auto`, `Auto` without size detection the largest
      area; encodings not offered dropped), without a panic or an overflow whatever the values. The clamping is
      the fake's, not a public function of the model: a real backend reads back what its driver took. Then
      `WarmingUp` where scripted, and sheet by sheet: `check()` before each, its number counted on from
      `first_sheet` in checked arithmetic, `Sheet(n)`, the front's page and in duplex the back's;
      `ScanError::FeederEmpty` before any page (after `Applied`), `Done::FeederEmpty` after, a flatbed's
      `Done::OnePage`. A page is the one given on its sheet, delivered as given, or drawn at the applied paper
      and dpi, or a short stand-in in the first encoding asked that fits its format (the fake encodes nothing);
      none past 512 MiB, refused before anything is allocated; its progress in bytes, a row at a time. Every
      `send` waits while the channel is full; one that fails ends the batch as `Done::Cancelled`.
    - **Faults:** at a side's start (after its sheet's `Sheet` event) or midway (half its rows counted), each
      firing once. `Error` ends the batch with the pages before it kept and the sheet put back at the feeder's
      head, so Continue is a batch run again from its number (its front again after a fault on its back);
      `Gone` leaves the device gone to every holder of it; `Hang` parks the batch's thread for good; `Crash`
      aborts the process and `Exit(code)` exits it, for a test's or a worker's process, never an app's; and
      since S0 slice 4, `AwaitCancel` holds the side until the batch is cancelled (above).
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
      say the settings were refused (`SettingsRefused`) if it repeats: the backends studied answer it for an
      area or a size they can't scan (`escl`, `lexmark`, `pixma`, `hp5590`), not for a device gone (S0 slice
      2's code review, which revised the first answer, `Gone`).
    - **Ids and the fallback** (the seam note's section 1): an id is `sane:` and the name's bytes as Latin-1,
      given back to `sane_open` byte for byte. The worker that opens a stored id is started with a `ConfigDir`
      naming that id's backend, or none, written `Search::BeforeSystem`, and lists that backend before it
      opens anything: the stored name listed with the stored vendor and model opens, listed with another is
      taken as gone, not listed is opened by name all the same. One gone, or answering `INVAL` at open, falls
      back only from a USB connection's name (`libusb:`), to the one listed device under such a name whose
      backend is the id's, with the same vendor and model, if exactly one is, and says so (`renamed_from`),
      else `Gone`; never across backends nor to a network scanner. An id with one of `dll`'s testing prefixes,
      a leading `:`, or no name at all, or one not Latin-1 or holding a `NUL`, is refused before `sane_open`
      (`BadId`).
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
- **Boundary:** no image codecs, imageproc, PDF crate or GUI toolkit under it. A gate keeps it so: the gates'
  devices-layer loop reads its graph with every feature on, so a dependency a backend's feature brings is read
  as a default one is, and the libsane gate builds every binary with every feature on (S0 slice 3, the latter
  from its code review; 8 packages as built: itself, `in-progress`, `thiserror` and its macro's
  `thiserror-impl`, `proc-macro2`, `quote`, `syn` and `unicode-ident`; 11 since S0 slice 4's `serde` feature,
  with `serde`, `serde_core` and `serde_derive`).
  Dependencies: `in-progress = "0.1.0"` (cancel and progress, in its public API and re-exported; taken by S0
  slice 3, vetted in S0 slice 2's plan and read again in slice 3's; a caret requirement, since an exact pin
  would keep an app on a later 0.1.x from resolving) and `thiserror` (the workspace's); each backend its
  binding, behind its feature (`sane-dl` in slice 5, `twain-dsm` in slice 9, `windows` in slice 10); `serde`
  (the workspace's, optional: the `serde` feature, S0 slice 4). For its tests: the crate itself with `fake` and
  `serde`, `rusty-fork` (the workspace's), whose child processes take the crash and the exit, and `serde_json`
  (the workspace's).
- **MSRV:** 1.89, the family's floor (`decisions.md`): the highest `rust-version` in its own graph (`cargo tree
  -p scan-device`, normal, build and dev, on every target; 25 packages) is 1.85, `in-progress` 0.1.0's and
  `getrandom` 0.4.3's (through `rusty-fork`'s `tempfile`). Measured again by S0 slice 4 with `serde` and
  `serde_json` in it: 32 packages, still 1.85 (theirs 1.71 at most). CI tests it on 1.89 (`scan-device-msrv`).
- **Tests** (S0 slice 3; 115, and S0 slice 4's 8: the `serde` feature's 6 units and `AwaitCancel`'s 2): units
  for the layout (each format's, `Empty`, `Length` short and long, the padding bits, `TooLarge` past `u64`,
  past a 64-bit `isize` and past a 32-bit one through the private limit, `data_len` agreeing with `new`,
  `Debug` without a byte), the fake's clamping, page sizes and `pattern`'s bytes pinned, its rows as its docs
  say and its sizes refused as `data_len` refuses them; in
  `/crates/scan-device/tests/`, from outside the crate, `model.rs` (each constructor, what's `Send`, `Opened`,
  each error's message, a push-shaped device that moves a child progress and a `SyncSender` clone into a thread
  of its own and sees a cancel from a third) and `fake_devices.rs`, `fake_batches.rs` and `fake_faults.rs`
  (every rule above and every script of proposal 22, the crash and the exit in child processes through
  `rusty-fork`); doctests, among them the crate's example through the fake. The layout's units ran on a real
  32-bit `usize`, under WASI: in step 1, and again in the code review's fixes on a copy of the crate without
  `rusty-fork`, whose `wait-timeout` doesn't build there; running them there in CI is TEST-12.
- **Stability:** provisional (0.0.x, unpublished) through S0's Linux slices, with one real backend behind it;
  frozen at 0.1.0 in S0 slice 11, once the `twain` backend has been through it (`decisions.md`, "Made here").
  sheaf takes it no sooner (`sheaf/swaps.md`).
- **The seam:** `crates/scan-device-seam.md`: for each concept of the model, how SANE, TWAIN, WIA and ICA
  deliver it, what the model makes of it, and what's still to settle, with the known unknowns. First version
  2026-11-08, before any driver layer; revised by the TWAIN spike (S0 slice 7) and by S0 slice 2 (its SANE
  rows from `sane-dl`, the raw layout, and the model written down for slice 3), and whenever a backend finds
  the model lacking. It changes proposal 9's model in sixteen places (its "Changes from proposal 9's model"),
the last five from S0 slice 2's code review.
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
    its contract; check it again before S0 slice 9. Checked again for slice 3 (2026-11-09): still 0.1.0
    alone, `printscan-sane` unpublished; its test doubles are per-call stand-ins for its own trait, not
    scripted devices. Study only, nothing taken.
  - How the packaged crate's tests get the fake, once slice 11 makes it ready to publish: packaging drops the
    dev-dependency on the crate itself, a path alone, so its fake's tests don't build from the package (Q-5).
  - What else of the model may grow before slice 11 freezes it: the enums still exhaustive (`Resolutions`,
    `Side`, `SideKnown`), the plain structs a caller writes as literals, and `Copy` on the growing enums (Q-6).
