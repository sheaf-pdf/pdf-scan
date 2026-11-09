# S0 slice 3: scan-device, provisional

Status: **done on branch `s0-3-scan-device`** (2026-11-09), waiting on review, the push and its pull request
against `master`: its four steps, each checked, then its code review's 18 findings, all fixed ("Code review").
The branch was made by the plan review from `6f01d7f` (slice 2's tip, as the run set it), then moved by the
main session onto `master` at `3f33be5` before step 1, the base the steps are written for ("Taken as it
starts", decision 1). Milestone: `milestones/s0-devices.md`, slice 3.

Paths: `docs/pm/` files from `docs/pm/`, as everywhere here; the code's from the repository's root, written
with a leading `/` (`/crates/scan-device/`, `/justfile`); `scratch/` from the root, local and never tracked.

## What it builds

`scan-device` 0.0.0, unpublished (`publish = false`): the device model exactly as the seam note's "The model,
for slice 3" writes it (`crates/scan-device-seam.md`; S0 slice 2, revised by its code review), and its first
backend, `fake`, scripted, which needs no driver API and runs on every runner. As the milestone's slice 3 has
it:

- **The model:** the types and the two traits of the note's block (`DeviceId`, `DeviceInfo`, `Driver`, `Caps`,
  `SourceCaps`, `Source`, `Resolutions`, `SizeMm`, `Settings`, `Paper`, `Applied`, `Clamped`, `Format`,
  `RawImage`, `LayoutError`, `RawPage`, `Pixels`, `Encoded`, `EncodedKind`, `Encoding`, `Side`, `SideKnown`,
  `Event`, `Status`, `Done`, `ScanError`, `WindowClosed`, `WindowOwner`; `Backend`, `Opened`, `Device`), a
  constructor for each struct marked `#[non_exhaustive]`, `RawImage`'s checked constructor and its methods,
  `Debug` by hand for `RawImage` and `Encoded`, cancel and progress through `in-progress`, taken here.
- **The `fake` backend, behind a feature:** devices listed under a deadline and opened as the `sane` backend
  will open them (a renamed device found by its maker and model, a name now another model's taken as gone),
  batches of sheets and sides with their settings clamped and said, and [M5 proposal 22](https://github.com/sheaf-pdf/sheaf/blob/3232a35/plan/m05-scan/proposals/p22-testing-without-scanner.md)'s
  scripts: a jam at sheet
  4 then Continue, a double feed, an open cover, an empty feeder at the start, a hang, a crash, a cancel
  mid-page (the sheet in flight finished, none fed after), a cancel after a duplex front (its back still
  delivered), a batch held by a full channel.
- **Around it:** `scan-device` in the gates' devices-layer loop, its graph read with `in-progress` in it and
  the gate's output naming it; its MSRV measured, a recipe and a CI job for it; `plan.md`'s graph with
  `in-progress` under it.

No `sane` backend (slice 5) and no worker (slice 4): `scan-device` takes no dependency on `sane-dl` here,
nor on anything but `in-progress` and `thiserror`. Nothing is cut from sheaf (it has no device model built:
`sheaf/from-sheaf.md` lists plan material only), so there is no harness comparison; its cited plan files
were checked for drift as the slice starts. Nothing here touches the PDF family.

**Later slices build on it** (`milestones/s0-devices.md`; the note's "What slice 4 needs of it"): slice 4
carries the model's types over the worker's pipe (a `serde` feature or wire types, its choice), decodes a
page through `RawImage::new` after capping its size, and drives the `fake` backend through a real worker
process (a hang, a crash, a cancel mid-page, pages of known bytes, the throughput gate's pages); slice 5 adds
the `sane` feature and backend behind the same traits; slice 9 the `twain` backend, a push backend whose
thread takes a child progress and a clone of the `SyncSender`; S1's `scan-fixtures` holds device scripts for
the fake (`crates/scan-fixtures.md`), pages it draws given as a sheet's sides; slice 11 freezes the model.
The design below builds nothing those must undo, and exposes what each takes.

## Taken as it starts

- **No ask bears on it:** `asks.md` holds ask 5 alone (where `pdf-images` lives), which is S2's.
- **`in-progress`, taken as S0 slice 2's decision 10 has it** (open to review), its vetting re-read on
  2026-11-09 from the registry's copy (`scratch/cargo-home`) and crates.io's index: 0.1.0 is still the only
  release (published 2026-10-17, not yanked), MIT OR Apache-2.0, edition 2024, `rust-version` 1.85, no
  dependencies (dev: `rayon`, and `loom` under `cfg(loom)`), one source file with no `unsafe`, packaged from
  commit `3949d79` of github.com/sheaf-pdf/in-progress. Its `Progress` (`check`, `set_total`, `set`, `inc`,
  `message`, `is_cancelled`, `child`) and `Tracker` are what `Device::scan` takes; how it's pinned is decision
  4. `plan.md`'s graph gains it (step 4).
- **Where it starts: slices 1, 2 and 8 are merged, and the steps start from `master`.** Since this slice was set
  up (to start from slice 2's tip, `6f01d7f`, its pull request stacked on slice 2's), pull requests #2 (slice 1)
  and #3 (slice 2) were squash-merged into `master` (`3ebea15`, `70d7b20`; `70d7b20`'s tree is `6f01d7f`'s
  exactly), and then, as this plan was drafted, #4 (S0 slice 8, `twain-dsm`, from the Windows machine;
  `3f33be5`, its branch pruned). The plan review made the branch from `6f01d7f`, as its job gave it, and
  committed the plan there; the main session then moves it onto `3f33be5` before step 1 (the run's control
  file, 2026-11-09), so step 1 starts from `master`'s workspace (decision 1). Slice 8 leaves the note's model
  block as it is (its edits are the note's head paragraph, sections 3 and 8 and the unknowns' TWAIN rows), so
  nothing this plan reads of the model differs there.
- **What `master` holds that this slice builds on** (`git diff 6f01d7f 3f33be5`): `crates/twain-spike` gone,
  `crates/twain-dsm` a member; `[workspace.dependencies]` with `rusty-fork` and `thiserror` (its comment: "A
  dependency joins once a second member takes it"), which `sane-dl` and `twain-dsm` take with `workspace =
  true`; the justfile's gates loop `for crate in sane-dl twain-dsm`, `twain_dsm_msrv` after `sane_dl_msrv`,
  `msrv: sane-dl-msrv twain-dsm-msrv`, and `twain-dsm`'s own recipes outside `ci`; CI's `twain-dsm`,
  `twain-dsm-doc-codes` and `twain-dsm-msrv` jobs; the root README's `twain-dsm` row and a "Next" line of
  `scan-device` and `scan-worker`; `decisions.md`'s ruling that public APIs are safe Rust (`scan-device` needs no
  `unsafe` and forbids it); slice 7's plan in `archive/slices/`; and the backlog's **TEST-11, taken**
  (`twain-dsm`'s decoders fuzzed), so the next TEST is TEST-12. `crates/sane-dl/` and `crates/sane-dl.md` are
  left alone: another slice changes `sane-dl` beside this one (the control file), and this one needs nothing of
  them changed (`thiserror` and `rusty-fork` are in the block already).
- **Should the branch stay on `6f01d7f`** (the move not made), the plan review's trial holds for it: the
  workspace's block and `sane-dl`'s two lines written as `master` writes them, line for line, and each step's
  check running the rebase's trial (`git merge-tree --write-tree --name-only --merge-base 6f01d7f HEAD
  origin/master`, this slice's commits applied to `master`) and recording what it conflicts in. A step 1
  simulated so on a worktree by the plan review (the manifests, the lock with `in-progress`, `scan-device` and its
  self dev-dependency, CI's job at the file's end, the justfile's loop and MSRV lines) conflicted in the justfile
  alone.
- **sheaf, checked for drift:** still at `3232a35` (2026-10-28); `git diff --stat 3232a35 HEAD` over the
  cited plan files is empty (`scratch/reference/README.md`'s command, 2026-11-09).
- **printscan, checked again** (the spec asks it before slices 3 and 9): `printscan`, `printscan-windows` and
  `printscan-wsd` are still at 0.1.0 alone (crates.io's index, 2026-11-09), MSRV 1.95; `printscan-sane` is
  unpublished. Its test doubles (`tests/common/`: a WIA-, a SANE- and an eSCL-like one, 737 lines, read in
  0.1.0's crate under `scratch/work/s0-3/plan-probe/`) are per-call stand-ins for its own `Backend` trait, not
  scripted devices. The verdict holds: study only, nothing taken; the spec's "nothing scripted at this level
  exists elsewhere" stands.

## The design

**The note is the model.** Where slice 2's plan and the note disagree, the note wins: the plan's own design
section ("The model, as slice 3 builds it") predates its code review, and gives `open(&DeviceId)`, a
`Sender<Event>`, `ScanError::Stopped` and `max_area: SizeMm`, which the note's block replaced with
`open(&DeviceInfo, Duration)`, a `SyncSender<Event>`, the client's own error and `Option<SizeMm>`. Every name
is the note's; nothing is renamed. What the note and the plan both leave open is decided below and listed
under "Decisions taken alone, for review".

### The crate

- **`/crates/scan-device/Cargo.toml`:** version `0.0.0`, edition 2024, `rust-version` from the MSRV probe
  (expected 1.89, the family's floor: `in-progress` declares 1.85, `thiserror` 2.0.21 1.77), `license = "MIT
  OR Apache-2.0"`, a one-line `description`, `repository`, `publish = false` (provisional until slice 11
  freezes the model; no `keywords`, `categories`, `include` or changelog until then). Dependencies:
  `in-progress = "0.1.0"`, `thiserror.workspace = true`; dev: from step 2, the crate itself with `fake` (below),
  and from step 3, `rusty-fork.workspace = true` (its child processes; taken where it's first used, which also
  keeps step 1's unit tests building for `wasm32-wasip1`, where `rusty-fork`'s `wait-timeout` doesn't compile:
  the plan review's trial). Features: `fake = []`, from step 2, nothing by default. `[lints] workspace = true`.
- **`/crates/scan-device/src/`:** `#![forbid(unsafe_code)]` (nothing here needs it) and `#![warn(missing_docs)]`
  (a published model's every item documented; `-D warnings` makes it an error). Modules by concept, the
  implementation's to cut (the note's sections suggest: devices and the traits; capabilities and settings;
  images and the raw layout; pages, events and outcomes; errors; `fake`), everything public re-exported at the
  root as the note's block has it, and `pub use in_progress;` (decision 4).
- **The crate docs** say, in our own words: what the model is (one shape for every driver API: a backend
  lists and opens, a device gives its caps and scans; synchronous, a batch on a thread of its own until it
  ends); a batch's events and their order, the bounded channel and what a full or a closed one does; cancel
  (the caller's `Tracker`, checked between sheets, so the sheet in flight is finished, front and back) and
  progress (each page's bytes); the raw layout, stated once (below); ids compared whole and read by nobody;
  the features (`fake`); and that the model is provisional (0.0.x) until it's frozen. An example through the
  fake (a doctest: list, open, caps, scan with a `Tracker` and `sync_channel`, the events read), the one place
  the crate docs name the fake's items in code, never by an intra-doc link, so `cargo doc` without the feature
  stays clean. They never name the downstream app, its milestones or `scratch/`, and say what the code does,
  not where it came from (the gates check the name; the step's check reads the rest).
- **`/crates/scan-device/README.md`:** a short one, as `sane-dl`'s is shaped: what the crate is, that it's
  provisional and unpublished, its features, and the licences at the root (links to the root's
  `LICENSE-MIT` and `LICENSE-APACHE`). No licence files in the crate.

### The model, as built

The note's block, type for type, field for field, variant for variant and derive for derive (its first
paragraph: plain data, every field public but `RawImage`'s, a growing enum or struct `#[non_exhaustive]`,
sizes and counts `u32` but a page's `dpi`, paper in `f32` millimetres), the doc comments the block carries
kept as each item's doc. What the block leaves out, the crate adds (decision 3):

- **A constructor for each struct marked `#[non_exhaustive]`,** its required fields as arguments and the rest
  at their plainest, for code outside the crate (an app's tests, the fake's scripts, slice 4's client if it
  writes wire types), which can't write a literal:
  - `DeviceInfo::new(id, name, maker, model, driver)` and `Settings::new(source, dpi, format)` (one side,
    `Paper::Auto`, raw only), as the note names them;
  - `Caps::new(sources)` (no settings window); `SourceCaps::new(source, resolutions, formats)` (no duplex, no
    encodings, `max_area` `None`, `paper_auto` false); `Applied::new(settings, clamped)`; `RawPage::new(pixels,
    dpi, sheet, side, side_known)`.

  Each infallible, as the fields are public: the invariants the note puts on the decoding side (a `dpi`
  finite and above 0, a `Resolutions::List` never empty, a `Range`'s `min` never past its `max`) stay slice
  4's client's to check, and the fake's to keep in what it makes.
- **`RawImage`'s methods:** `new`, `format`, `width`, `height`, `stride`, `data`, `into_data`, as the note's
  bullet has them, and `RawImage::data_len(format, width, height) -> Result<usize, LayoutError>`, the bytes a
  `RawImage` of that format and size holds, refused as `new` refuses it (`Empty`, `TooLarge`), so a reader
  caps a page before it reads it (slice 4's "a page's format, width and height before its bytes") and
  allocates once; `new` uses it.
- **`Debug` by hand** for `RawImage` and `Encoded` (the format or kind, the size, the byte count; never the
  bytes: decision 39 of slice 2), and for `Opened`, which holds a `Box<dyn Device>` and derives nothing (its
  `info` and `renamed_from`, the device elided), so a test can `expect` an `open`.
- **The traits** as the block has them, `Backend: Send` and `Device: Send`, `Box<dyn Device>` in `Opened`.
  Each method's doc says what the note says of it: `list` a snapshot under its deadline; `open` refusing an id
  its driver doesn't form (`BadId`), `TimedOut` past its deadline, `renamed_from` where it knowingly opens
  another device; `scan` blocking until the batch ends, its events and their order, `check()` between sheets
  and never within one, a failed `send` ending the batch as `Done::Cancelled`, `FeederEmpty` only before any
  page, the pages sent before an error kept; `settings_window` and `Unsupported`.
- **Errors:** `ScanError` and `LayoutError` as the block has them (`thiserror`, `Clone`, `Eq`), their messages
  the block's. No `From<LayoutError> for ScanError`: a backend that fails to make a page says why in a
  `Driver` error of its own words.

### `RawImage` and its checks

`RawImage::new(format, width, height, data)` refuses, in order: a width or height of 0 (`Empty`); a row's
bytes (`Bw1` a bit a pixel rounded up to whole bytes, `Gray8` one byte, `Gray16` two, `Rgb8` three, `Rgb16`
six) or the image's (`height` rows) past `u64`, worked out with checked arithmetic, or past what the
platform's `usize` holds (`TooLarge`); a `data` of any other length (`Length`, both counts as `u64`); and a
`Bw1` row whose bits past its last pixel aren't 0 (`Padding`, the first such row named). The platform's limit
is a parameter of the private function that works the length out (`usize::MAX` as `u64` in `new` and
`data_len`), so its unit tests take the 32-bit case on any host by passing `u32::MAX`: 65,536 by 65,536 in
`Rgb16` is 6 × 2^32 bytes, refused there, where a wrapping 32-bit `usize` would make it 0 and an empty buffer
would match. On a 64-bit host the same image passes the size check and its empty `data` is a `Length`. At
`u32::MAX` by `u32::MAX`, `Gray16`, `Rgb8` and `Rgb16` overflow `u64` and are `TooLarge`; `Bw1` and `Gray8` fit
`u64` (at most 2^64 − 2^33 + 1 bytes) and are refused by the platform's limit or the length (slice 2's step 2,
said in the tests). Probe M1 tested the same; these are the crate's own, written fresh.

**The raw layout** the docs state (the note's section 3, its decisions 2 to 6): rows top down and packed, no
stride stored; 0 the least intensity at every format, so in `Bw1` 0 is black; in `Bw1` the leftmost pixel in a
byte's most significant bit and the bits past a row's last pixel 0; `Rgb8` and `Rgb16` R, G, B a pixel; 16-bit
samples little-endian whatever the host. `new` checks the length and the padding bits, the parts a buffer can
get wrong; the rest is how a backend fills it.

### The `fake` backend

A feature, `fake`, as the spec has every backend (decision 6): `scan_device::fake`, compiled only when asked
for, so an app's shipped build has no fake in it; the crate's own tests turn it on through a dev-dependency on
the crate itself (`scan-device = { path = ".", features = ["fake"] }`), which a probe showed reaching unit
tests, integration tests and doctests on stable and on 1.89, while `cargo build` leaves it off
(`scratch/work/s0-3/plan-probe/selfdev/`). `cargo doc` doesn't build dev-dependencies, so it documents the fake
only with `--all-features` (the same probe): the `doc` and `clippy` recipes take `--all-features` (below). It
takes no dependency. Nothing in it calls an operating system's driver API, so it runs on every runner.

**Its scripts are Rust values** (decision 8), built by the test that uses it; no text format. The shape below
is the design's; the step may rename and says so.

```rust
// `scan_device::fake`, behind the `fake` feature.
pub struct FakeBackend { .. }                    // implements `Backend`
impl FakeBackend {
    pub fn new() -> FakeBackend;                 // lists nothing until given devices
    pub fn with(self, device: FakeDevice) -> FakeBackend;
    pub fn listing_takes(self, delay: Duration) -> FakeBackend;
    pub fn not_installed(self) -> FakeBackend;   // `list` and `open` answer `NotInstalled`
}
#[derive(Clone)]
pub struct FakeDevice { .. }                     // a device's script, and a handle on its paper
impl FakeDevice {
    pub fn new(name: &str, maker: &str, model: &str) -> FakeDevice; // id `fake:<name>`
    pub fn info(&self) -> DeviceInfo;
    pub fn caps(self, caps: Caps) -> FakeDevice; // panics on caps the model forbids
    pub fn unlisted(self) -> FakeDevice;         // opens by its name, isn't listed
    pub fn refuses_open(self, error: ScanError) -> FakeDevice;
    pub fn reports_sides(self) -> FakeDevice;    // `SideKnown::Reported`, else `Inferred`
    pub fn warms_up(self) -> FakeDevice;         // `Event::Status(Status::WarmingUp)` each batch
    pub fn settings_window(self, closes: WindowClosed) -> FakeDevice;
    pub fn load(&self, sheets: impl IntoIterator<Item = Sheet>); // the feeder, at any time
    pub fn glass(&self, sheet: Sheet);           // what the flatbed scans (a plain sheet by default)
    pub fn fed(&self) -> u32;                    // sheets taken from the feeder so far
    pub fn in_feeder(&self) -> usize;
}
pub struct Sheet { .. }
impl Sheet {
    pub fn new() -> Sheet;                       // Letter; both sides drawn by `pattern`
    pub fn size(self, size: SizeMm) -> Sheet;    // its own size, which `Paper::Auto` scans
    pub fn side(self, side: Side, pixels: Pixels) -> Sheet; // a page given, delivered as given
    pub fn fault(self, side: Side, when: When, fault: Fault) -> Sheet;
}
pub enum When { Start, Midway }                  // before a side's first byte, or after half its rows
#[non_exhaustive]
pub enum Fault { Error(ScanError), Hang, Crash, Exit(i32) }
pub fn pattern(format: Format, width: u32, height: u32, sheet: u32, side: Side)
    -> Result<RawImage, LayoutError>;
```

`FakeBackend` and `Sheet` also implement `Default`, as their `new()` (clippy's `new_without_default` asks it,
and `-D warnings` makes it an error). The device a `FakeBackend` opens is a private type of its own, never
`FakeDevice`, so `FakeDevice::caps(self, Caps)` and `Device::caps(&mut self)` never meet in one type. `Fault`,
`When` and `Sheet::fault` come in step 3 with their behaviour, so no step has a script that does nothing.

- **State shared with the test:** a `FakeDevice` is a script and a handle at once (its paper, its count of
  sheets fed, whether it's gone, behind one `Arc<Mutex<…>>`), so a test that hands its clone to a
  `FakeBackend` loads paper between batches ("Scan More"), and reads `fed()` while a batch is blocked on
  another thread. The lock is never held across a `send`, a `Progress` call or a hang, so `fed()` and
  `in_feeder()` answer while a batch waits on a full channel or hangs. `fed()` counts each sheet taken; a
  faulted sheet put back and taken again is counted again. `FakeBackend` and the device it opens are `Send`.
- **Default caps** (`FakeDevice::new`): a feeder with duplex (`Resolutions::Range { min: 50, max: 600, step:
  1 }`, the five formats, no encodings, `max_area` Legal, `paper_auto` true) and a flatbed (`List([75, 150,
  300, 600])`, the five formats, `max_area` 215.9 by 297 mm, `paper_auto` false), so both kinds of
  `Resolutions` and both sources are exercised; no settings window. `caps(…)` replaces them, and panics on what
  the model forbids (no source, a source with no format, an empty `List`, a `Range` whose `min` is past its
  `max`, an area not finite and above 0): a script's mistake said as it's written.

#### Listing and opening

As the note's section 1 and its mapping table have the `sane` backend do it (decision 9), so the app and
slice 4's worker meet the same behaviour before slice 5 exists:

- **`list(deadline)`** gives the listed devices' `DeviceInfo`s (`id` `fake:<name>`, `name` the maker and model,
  `maker`, `model`, `driver` `Fake`), after the backend's scripted delay; a delay not under the deadline waits
  out the deadline and answers `TimedOut` (so a zero deadline is `TimedOut`, as probe M1 had it).
- **`open(stored, deadline)`**, in order: `NotInstalled` where the backend is scripted so; `BadId` for an id
  that isn't `fake:` and a name of at least one character (the stored id is never echoed); then the listing,
  under the deadline (`TimedOut`); the stored id listed with the stored maker and model opens that device,
  `Opened::info` the listing's; listed with another maker or model, it's taken as gone (another device's name
  now); not listed but scripted `unlisted`, it opens by name, `info` the stored one, unchecked (as an alias
  does through SANE); not there at all, it's gone. Taken as gone, it **falls back only from a USB connection's
  name** (`fake:libusb:<bus>:<device>`): to the one listed device under a name of that form with the stored
  maker and model, if exactly one is, said in `renamed_from`; else `Gone`. A scripted `refuses_open(error)`
  answers that error once the device is found (`Busy`, `AccessDenied`, `Gone`).
- **`caps()`** gives the scripted caps, or `Gone` once the device is; **`settings_window(owner)`** answers the
  scripted `WindowClosed`, with `Caps::settings_window` true, or `Unsupported` where none is scripted.

#### A batch

`scan(settings, first_sheet, events, progress)`:

1. **The settings clamped** to the chosen source's caps, and `Event::Applied` sent with what changed
   (`Clamped`, holding what was asked), as the fake's rules (decision 10): a source the caps lack becomes the
   first they list; duplex on a source without it, off; a dpi the nearest the source allows, a tie to the
   higher (a list's nearest value; a range's nearest of `min`, `min + step`, … that isn't past `max`, worked
   out in `u64`, so a `max` off the steps is never exceeded); a format the source lacks, the first it lists; a
   paper size beyond the largest area, cut to it in each dimension, and one not finite and above 0, the largest
   area; a size where `max_area` is `None`, `Auto`; `Auto` on a source without `paper_auto`, its largest area
   where it has one, and kept `Auto` where it has none (the page then drawn at the sheet's own size, as SANE's
   `pnm` scans what it holds); encodings the source doesn't offer dropped, the rest in the app's order. Any
   settings value, however wild (`dpi` 0 or `u32::MAX`, `NaN`, infinite or negative paper), is clamped without a
   panic or an overflow. The clamping is the fake's, not a public function of the model: real backends learn
   what their driver took by reading it back (the note's section 2).
2. **`Event::Status(Status::WarmingUp)`** where the device is scripted to warm up.
3. **Sheet by sheet** (a feeder) or the glass's one page (a flatbed): `progress.check()` first, and a failed check
   ends the batch as `Done::Cancelled` before the next sheet is taken (even with the feeder empty); then its
   number, counted on from `first_sheet` with checked arithmetic (past `u32::MAX`, a `Driver` error, the sheet left
   in the feeder); then the sheet taken from the feeder (`fed` counts it), `Event::Sheet(n)` sent, and its sides
   read: the front, and the back where duplex is on. An empty feeder before the first sheet is
   `ScanError::FeederEmpty` (after `Applied`, as SANE's `NO_DOCS` comes after the settings are set); after pages
   it's `Done::FeederEmpty`; a flatbed's page is `Done::OnePage`.
4. **A side's page:** a page given on the sheet is delivered as given; else `pattern` draws one at the
   settings as applied: the paper's size (or the sheet's own under `Auto`, cut to the largest area) at the dpi,
   `round(mm / 25.4 × dpi)` pixels a dimension and at least 1 (Letter at 300 dpi is 2550 by 3300), its
   length through `RawImage::data_len`, a page past 512 MiB refused as a `Driver` error before anything is
   allocated (decision 11). Where `Settings::encodings`, as applied, holds a form that fits the page's format
   (`CcittG4` black and white only, `Jpeg` 8-bit gray or color only, `Png`, `Tiff` and `Bmp` any), the page is
   `Pixels::Encoded` in the first that fits, its bytes a short stand-in naming the kind, the sheet and the side
   (the fake encodes nothing: the devices layer takes no codec; decision 12); else raw. Progress for each:
   `set_total(Some(bytes), Unit::Bytes)`, `set(0)`, then `inc` a row at a time (an encoded page's in one), so
   a cancel can land midway through a page. Then `Event::Page`, with `dpi` the applied dpi on both axes, the
   sheet, the side and `side_known`.
5. **The channel:** every `send` waits while the channel is full, so a batch whose events nobody takes holds
   at its next send, no further sheet taken; a `send` that fails (the receiver gone) ends the batch at once as
   `Done::Cancelled`, no sheet taken after it.

**`pattern(format, width, height, sheet, side)`** draws a page that differs by sheet and side, the same on
every run and every system, its `Bw1` padding bits 0, made a row at a time and repeated with a shift, so it's
cheap beside the pipe slice 4 measures (step 2 measures its rate). It's public so slice 4 checks the raw
layout across its pipe byte for byte against the same function.

#### Faults

`Sheet::fault(side, when, fault)` scripts what happens as that side starts (`Start`: after the sheet's
`Event::Sheet`, before the side's first byte) or midway (`Midway`: after half its rows, their progress
reported). Each fires once, so a batch run again meets the sheet as it is now (decision 13):

- **`Fault::Error(e)`** ends the batch with `e`, the pages before it kept and the side being read dropped:
  `Jammed`, `DoubleFeed`, `CoverOpen`, `Busy`, `Driver { message, shown }`, and the rest of the model's
  errors. The sheet stays at the feeder's head (the user clears the jam and puts it back), so a batch run again
  from that sheet ("Continue": `first_sheet` the jammed sheet's number) scans it and the rest; after a fault
  on a back, its front again, said in the docs. `Gone` also leaves the device gone: unlisted from then on, and
  every later call answers `Gone`. `FeederEmpty` isn't scriptable (the feeder's own emptiness says it): `fault`
  panics on it.
- **`Fault::Hang`** blocks the batch's thread for good, sending nothing and checking nothing, as a driver's
  call that never returns (the watchdog's to end, slice 4).
- **`Fault::Crash`** ends the process at once (`std::process::abort`), as a driver's crash would;
  **`Fault::Exit(code)`** exits with `code`. Both are for a worker process (slice 4's "a crash with the pages
  kept and the exit said"), never an app's own: the docs say so.

### The workspace around it

On `master` at `3f33be5` (decision 1):

- **`/Cargo.toml`:** `crates/scan-device` a member, its line right after `crates/sane-dl`; nothing else.
  `[workspace.dependencies]` holds `thiserror` and `rusty-fork` already, which `scan-device` takes with
  `workspace = true` (`thiserror` from step 1, `rusty-fork` from step 3: decision 5); `in-progress` has one taker
  and stays in `scan-device`'s manifest. `/Cargo.lock`, under `--locked`, changes at three steps and in nothing
  else: step 1 adds `in-progress` and `scan-device`; step 2's dev-dependency on the crate itself adds no package
  but puts `scan-device` in its own entry's dependencies (the plan review's trial); step 3 puts `rusty-fork` there.
- **`/justfile`:**
  - the gates' devices-layer loop `for crate in sane-dl twain-dsm scan-device`, so the gate's output names it
    ("gates: scan-device's graph read for the devices layer (N packages)");
  - `deps()` reads `cargo tree … --all-features`, so a backend's optional dependency behind a feature is read
    too (slice 5's `sane-dl`, slice 9's `twain-dsm`, slice 10's `windows`), not only the default graph (decision
    7); for `sane-dl`, `twain-dsm` and `fake-libsane`, which have no features, it reads what it read;
  - `scan_device_msrv` read from the manifest after `twain_dsm_msrv`, as the others are, `scan-device-msrv`
    (`cargo +{{ scan_device_msrv }} test --locked -p scan-device`) beside the other two, and `msrv: sane-dl-msrv
    twain-dsm-msrv scan-device-msrv`;
  - from step 2, `clippy` and `doc` with `--all-features` (the fake linted and documented as a user with it
    sees it; `test` needs nothing, the dev-dependency turns it on).
- **CI** (`/.github/workflows/ci.yml`, read as `master` has it before it's added to): a `scan-device-msrv` job on
  `ubuntu-latest`, as `sane-dl-msrv` is (`dtolnay/rust-toolchain@<MSRV>`, a note to keep it in step with the
  manifest, `just scan-device-msrv`), after `twain-dsm-msrv` with the other MSRV jobs (decision 21); no SANE
  needed. The `check` job's `just test` runs the fake's tests on Ubuntu, Windows and macOS as it is.
- **`/README.md`:** a `scan-device` row in "Scanner access", after `twain-dsm`'s (the device model, provisional
  and unpublished; from step 2, its `fake` backend), and its "Next" line `scan-worker` alone.

### For later slices

- **Slice 4** (the note's "What slice 4 needs of it"): plain data throughout, so a `serde` feature added later
  derives on every type and decodes `RawImage` through `RawImage::new` (`try_from`); or its wire types use the
  constructors. `RawImage::data_len` caps a page before its bytes are read. The fake through a worker: `Hang`,
  `Crash` and `Exit`; a cancel mid-page; `pattern` for the pipe's byte-for-byte check and the throughput gate's
  300 dpi Letter pages in `Rgb8` and `Bw1`. How a worker gets a script is slice 4's (a scenario its own test
  worker builds by name, or a script it decodes once it has `serde`); a text format isn't built here.
- **Slice 5:** a `sane` feature beside `fake`, its backend behind the same traits; the fake's opening rules are
  the note's for SANE, so slice 5's tests can be read against the fake's.
- **Slice 9:** the push shape (a child progress and a `SyncSender` clone moved into a thread of the backend's)
  shown in `tests/` by a test device of that shape, as probe M1 showed it, so the traits keep allowing it.
- **S1's `scan-fixtures`:** a sheet's sides given as `Pixels` (its drawn pages), delivered as given.

### Caps on hostile input

- **A page's buffer** (`RawImage::new`, `RawImage::data_len`): sizes in checked `u64`, against the platform's
  `usize`, the length exact, `Bw1` padding 0 (above): the check slice 4's client decodes every page through.
- **A stored id** (`Backend::open`, from the app's storage): the fake refuses one not of its form as `BadId`,
  which echoes nothing.
- **Settings** (the app's, through the worker later): clamped whatever their values, no panic, no overflow;
  pages sized in `f64`, clamped to at least 1 pixel and the caps' largest area and resolution, their bytes
  through `data_len`, and none past 512 MiB allocated.
- **Counts:** a sheet number past `u32::MAX` ends the batch with an error, never wraps or panics.
- **The events:** a bounded channel whose bound the caller sets, so pages never pile up past it.
- **The fake's scripts** are a test's, trusted: a malformed one panics as it's written, never in a batch.

### Dependencies, vetted

None new to the workspace but `in-progress` ("Taken as it starts"). `thiserror` 2 (2.0.21 in the lock) and
`rusty-fork` 0.3 (0.3.1, default features off, `timeout`: pulls `fnv`, `quick-error`, `tempfile` and
`wait-timeout`) are `sane-dl`'s, vetted in S0 slice 1's plan ("Dependencies, vetted"), and taken here from
`[workspace.dependencies]`; step 1 reads `cargo tree -p scan-device` for what `thiserror` pulls, step 3 for what
`rusty-fork` does (as a dev-dependency, `sane-dl`'s graph of it). No `serde` (slice 4), no `sane-dl` (slice 5).

### Where it differs from probe M1

Probe M1 (`scratch/work/s0-2/probes/model-sketch/src/lib.rs`, as its code review left it: the note's block,
`RawImage`'s methods, `Settings::new`, `DeviceInfo::new`, `Debug` by hand, 23 tests) is read, not copied: the crate
is written fresh to the note. It differs in: the four other constructors; `RawImage::data_len` in place of M1's
public `Format::row_bytes` (a row's length stays private); the platform's limit a parameter, so the 32-bit case
runs on every host; `pub use in_progress`; M1's in-test `Pull`, `PushSpawned` and `PushResident` devices become the
fake (a pull backend) and one push-shaped test device; M1's `Fake` took an unlisted name as gone, where the fake
opens a device scripted `unlisted` with the stored info, as the note's SANE row has it; and the tests run from
outside the crate, through its public API, so each `#[non_exhaustive]` struct is shown buildable there.

## Steps

**At every step**, the checking agent runs the core, once: `just ci` with `SANE_DL_REQUIRE_LIBSANE=1` and
`SANE_DL_REQUIRE_HEADER=1` (fmt, clippy `-D warnings`, test, doc `-D warnings`, the gates, `sane-dl-package`, which
asks the registry and is recorded as not run, never as passed, if the network is down, and the MSRV recipes on
1.89, installed here); `cargo test --locked --workspace -- --list` recorded under `scratch/work/s0-3/` and compared
with the step before's (none lost; the base's recorded by step 1's check on a worktree at `3f33be5`, since slice
2's 245, `scratch/work/s0-2/tests-fix.txt`, were `6f01d7f`'s, before `twain-dsm`); the commit's message and diff
scanned for local paths, the username, the maintainer's name, the downstream app's name and `scratch/` outside
`docs/pm/`, and pdf-lint's unpublished plan; and that the branch holds `3f33be5` and, after it, this slice's
commits alone (`git log --oneline 3f33be5..HEAD`), and `crates/sane-dl/` and `docs/pm/crates/sane-dl.md` untouched
by them. Should the branch still be on `6f01d7f`, the rebase's trial too ("Taken as it starts"). The control file
is read before each long run. Below, each "Done when" says only what the step adds; a check whose subject the step
doesn't touch is left out.

1. **The crate and its model; the workspace around it.** `/crates/scan-device/` (manifest, crate docs without the
   fake's example yet, README), the model as "The model, as built" has it, `RawImage` and its checks, and their
   tests: units for `RawImage::new` and `data_len` (each format's whole layout taken; `Empty`; `Length` short and
   long; a set padding bit, its row named, at a width not a multiple of 8; `TooLarge` past `u64`, and past a 32-bit
   `usize` through the parameter; `data_len` agreeing with `new`; `stride`, the accessors and `into_data`; `Debug`
   giving the length and not a byte; Letter and Legal not whole millimetres); and `tests/model.rs`, from outside
   the crate: every `#[non_exhaustive]` struct made by its constructor and its fields set, `Box<dyn Backend>`,
   `Box<dyn Device>`, `Opened`, `Event` and `SyncSender<Event>` `Send`, a `Tracker` passed as `&dyn Progress`
   through the re-export, a push-shaped test device moving a child progress and a `SyncSender` clone into a thread
   it spawns and seeing a cancel from a third thread at its next check, each `ScanError`'s message. The workspace,
   on `master`'s: `/Cargo.toml`'s member line, `/Cargo.lock`, the justfile (the loop, `deps()`, the MSRV variable,
   recipe and `msrv`), CI's job, the root README's row and "Next". The MSRV probe. No `rusty-fork` in `scan-device`
   yet (step 3), and nothing in `crates/sane-dl/`.
   **Done when:** the core; the probe recorded under `scratch/work/s0-3/msrv-probe/` as S1 slice 1's decision 2
   measured (each package's `rust-version` from `cargo metadata` over `cargo tree -p scan-device -e
   normal,build,dev --target all`, the highest, never below 1.89) and `just scan-device-msrv` passing on that
   toolchain (in the core's `just ci`); `cargo tree -p scan-device -e normal,build --target all --all-features`
   read: `in-progress`, `thiserror` and its macro's crates, no `sane-dl`, nothing else, and the gate's output
   naming `scan-device` with that count, beside `sane-dl`'s and `twain-dsm`'s; the layout tests built for
   `wasm32-wasip1` (`cargo test --lib --no-run`, a target directory under `scratch/work/s0-3/`) and run once
   through Node's WASI, as M1's were (`scratch/work/s0-2/step-2/wasi/run-wasi.mjs`), so the 32-bit branch runs on a
   32-bit `usize` for real; three mutations of `RawImage::new` on a worktree under `scratch/work/s0-3/` (no padding
   check; the length in wrapping `usize` arithmetic; no platform limit), each failing a test; the gates'
   make-fails, once, here where the gate changes, on a worktree under `scratch/work/s0-3/`, its lock made again on
   the copy first (`cargo update --offline -w`; `png` 0.18.1 is in the cargo home) so the gate fails on the
   dependency it names and not on a stale lock: `png` as a normal dependency of `scan-device`, and `png` as an
   optional one behind a feature of its own (which only `deps()`'s `--all-features` reads), each failing with
   `scan-device` and `png` named; each worktree removed after and the branch checked unchanged; the model read
   against the note's block, a row per type in the check's record (each name, field, variant, derive, and each
   trait method's signature), every departure one this plan names. Left out: other targets (no `cfg` code);
   `sane-dl-package`'s manifest read beyond the core's run (`sane-dl`'s manifest unchanged).
2. **The fake: devices, listing and opening, and batches that go right.** The `fake` feature and the dev-dependency
   on the crate itself, and the lock's change for it; the justfile's `clippy` and `doc` with `--all-features`;
   `FakeBackend`, `FakeDevice`, `Sheet` and `pattern`, as "The `fake` backend" has them, with "Listing and opening"
   and "A batch" whole (no faults: step 3); the crate docs' example; the README's clause. Tests (`tests/`, the
   public API): listing and its deadline; opening as stored (`info` the listing's), renamed from a USB connection's
   name by maker and model (`renamed_from`), a stored name now listed as another model taken as gone, the same with
   that model listed under another USB name found there, no fallback from a serial name, nor to one of two, an
   unlisted device opened with the stored info, a name not there `Gone`, `BadId` (no `fake:`, an empty name),
   `TimedOut` (a zero deadline, and a listing slower than the deadline), `NotInstalled`, a scripted `Busy` and
   `AccessDenied`; caps and the settings window (`Unsupported`, `Accepted`, `Dismissed`); batches: simplex and
   duplex pages and their sides, `side_known` inferred and reported, `first_sheet` carried through "Scan More"
   (paper loaded between batches), `Done::FeederEmpty`, `ScanError::FeederEmpty` after `Applied`, a flatbed's
   `Done::OnePage`, every clamp rule's `Clamped` (and none when nothing changed; a range whose `max` is off its
   steps never exceeded), hostile settings clamped without a panic (a `NaN` asked for matched with `is_nan`, never
   `==`), page sizes (Letter at 300 dpi 2550 by 3300; `Auto` the sheet's own size), `pattern` the same on two runs
   and different by sheet and side, its `Bw1` padding 0, a page given on a sheet delivered as given, encoded pages
   as asked and raw where none fits, the 512 MiB cap, progress (each page's `set_total` its bytes, and the tracker
   finished at the batch's end).
   **Done when:** the core (its clippy and doc now with the feature); `cargo doc -p scan-device --no-deps` without
   the feature, `RUSTDOCFLAGS="-D warnings"`, clean, so a user without it meets no broken link; the fake's opening
   rules read against the note's section 1 and its mapping table, a row each in the check's record; two mutations
   of the fallback on a worktree (the first match taken where several are; the maker and model not compared), each
   failing a test; the fake's page rate measured once in a release build (a scratch program under
   `scratch/work/s0-3/`, never committed: 300 dpi Letter `Rgb8` and `Bw1` pages a second from `pattern` and from a
   whole batch), recorded for slice 4's throughput gate (2.3 pages a second, about 58 MB/s). Left out: the MSRV
   probe (no package added: the dev-dependency on itself changes only the lock's own entry), other targets (no
   `cfg` code), the gates' make-fails (no gate changed).
3. **The fake's faults and controls.** `rusty-fork` as a dev-dependency (`workspace = true`) and the lock's change
   for it; `Fault`, `When` and `Sheet::fault`, with their behaviour (`Error`, `Hang`, `Crash`, `Exit`), the device
   gone after `Gone`, a fault firing once, the sheet kept at the feeder's head; warming up; cancel between sheets;
   the channel's hold and a failed `send`; the count past `u32::MAX`. Tests, every script of proposal 22 on every
   runner: a jam at sheet 4 (its front's start) then Continue from sheet 4 to the feeder's end; a jam midway
   through a back (the front kept, the back dropped); a double feed; an open cover before the first page; `Gone`
   mid-batch then every call `Gone`; a `Driver` error with `shown`; `WarmingUp` before the first sheet; a cancel
   mid-page (a test's own `Progress` cancelling its tracker from another thread at a page's middle row: the sheet
   finished, front and back in duplex, `Done::Cancelled`, `fed()` counting no sheet after); a cancel after a duplex
   front (taken on a rendezvous channel: its back still delivered, no sheet after); a tracker cancelled before the
   batch (`Applied`, then `Done::Cancelled`, nothing fed); a batch held by a full channel (bound 1, nobody reading:
   `fed()` waited on until it's 1, for up to 10 s, then still 1 after 300 ms more, then every page once read, as
   M1's test waited, so a slow runner can't fail it); a receiver gone after the first page (`Done::Cancelled`, no
   sheet taken after the failed `send`); a hang (on a thread of its own, the events before it taken on a rendezvous
   channel, then nothing, the batch unfinished 300 ms after a cancel, `fed()` still answering; the thread left
   blocked); a crash and an exit (each in a child process through `rusty_fork::fork`, whose supervisor gets the
   child's status and output as `sane-dl`'s `Child` does, its events printed and flushed as each is taken on a
   rendezvous channel, so every page before the fault's sheet is printed before that sheet's `Sheet` event is
   taken: the parent reads the pages printed and the child's exit, by signal on Unix and by its code on Windows for
   the abort, the code itself for `Exit`, as `sane-dl`'s abort test reads its child's).
   **Done when:** the core; `cargo clippy --locked -p scan-device --all-targets --all-features --target
   x86_64-pc-windows-gnu -- -D warnings`, the same for `x86_64-apple-darwin` (the crash tests' expected exits sit
   behind `cfg`), and their docs left out (no `cfg` in them); the timing tests (the held channel, the hang, the
   deadlines) run 50 times in a loop once, under the memory scope, 8 test binaries at a time, none failing; four
   mutations of the batch on a worktree (the check made within a sheet, between front and back; the next sheet
   taken before the last `send` returns; a failed `send` ignored; a jammed sheet dropped from the feeder), each
   failing a test; the MSRV probe again, over the graph `rusty-fork` joins (expected unchanged: it's `sane-dl`'s,
   whose highest is `libloading`'s, not in this graph), and `cargo tree -p scan-device -e normal,build --target all
   --all-features` unchanged (a dev-dependency). Left out: the fallback (unchanged), the gates' make-fails (no gate
   changed).
4. **The plan's files; the slice's end.** `crates/scan-device.md` as built (the model's additions, the fake's
   feature, scripts and rules, the dependencies, the MSRV, "Stability" unchanged); `crates/README.md`'s line;
   `plan.md`'s graph (`in-progress` under `scan-device`) and its family table's row; one sentence at the head of
   the note's "The model, for slice 3" saying slice 3 built it as written and where its additions are said
   (`crates/scan-device.md`), off every TWAIN line; the milestone's slice 3 entry (done on the branch, waiting on
   review, the counts of this plan's decisions and security items); `decisions.md`, one bullet: scan-device's MSRV,
   as `sane-dl`'s is recorded, placed right after `sane-dl`'s MSRV bullet, beside it, not after slice 8's ruling at
   the section's end (decision 22); `status.md` and `slices/README.md`, a line each (slice 8's entries there are
   its machine's, left as they are); the backlog: whatever the steps found, one line each, with the next free IDs
   as `master` has them (TEST-12, since TEST-11 is slice 8's; FEAT-13, BUG-8, DOC-2, Q-5, UP-14; read again from
   `origin/master` as the step starts, in case it has moved), the README's table counted again with `rg` or its
   counts left out.
   **Done when:** the core, but `just ci` only if the step's diff reaches outside `docs/pm/`: no build, test or
   gate reads `docs/pm/` (the name gate excludes it), so step 3's run on the same code stands; no backlog ID the
   step adds found on `origin/master` as it then is (`git grep` there); `scan-device`'s publish dry run not run
   (`publish = false` until slice 11), said, while `sane-dl`'s runs in the core; every commit message of the branch
   read for the maintainer's name, local paths, `scratch/`, the downstream app and pdf-lint's plan; `git grep -i`
   for the maintainer's name finding only the root `LICENSE-MIT`; every backlog line in the README's format; every
   path the edited files cite found. **What only CI shows,** said: the fake's tests on Windows and macOS (the child
   processes' exits there, the timing tests on slower runners), `scan-device-msrv` as CI's job, and the gates on
   `ubuntu-latest`'s SANE.

## Baselines

- **The base, `master` at `3f33be5`** (slice 8's merge, on slices 1 and 2's, whose tree is `6f01d7f`'s): CI's run
  on pull request #4 for its code; here, step 1's check records its test list (and its `just ci`, if it runs it,
  on the worktree) before comparing. `6f01d7f`'s were green locally (slice 2's fix stage on `a174dde`: fmt,
  clippy, test, doc, the gates with 15 binaries read, `sane-dl-package` 16 files, `sane-dl-msrv` on 1.89;
  `scratch/work/s0-2/fix/just-ci.log`; 245 tests, `scratch/work/s0-2/tests-fix.txt`).
- **The note's block** ("The model, for slice 3", the same at `6f01d7f` and `3f33be5`): what the crate is held
  to, type by type.
- **Probe M1** (`scratch/work/s0-2/probes/model-sketch/src/lib.rs`, described in `archive/slices/s0-2-seam-note.md`'s
  "So far", step 2, and its "Code review"; it has no README): read for the shapes it proved, its departures said
  above; never copied.
- **The MSRV probe** (step 1, `scratch/work/s0-3/msrv-probe/`): `scan-device`'s own graph, normal, build and
  dev, on every target.
- **The plan's own probe** (`scratch/work/s0-3/plan-probe/selfdev/`): a feature turned on for a crate's tests by
  a dev-dependency on itself reaches unit tests, integration tests and doctests, on stable and 1.89; `cargo
  build` and plain `cargo doc` leave it off.
- **No third party:** nothing here has one (the fake is the model's own behaviour); the `sane` backend's
  comparisons with SANE's backends and `scanimage` are slice 5's.
- **sheaf:** no comparison; its plan files checked for drift ("Taken as it starts").

## Decisions taken alone, for review

1. **The branch made from `6f01d7f`, as the plan review's job gave it, and moved onto `master` at `3f33be5` by
   the main session before step 1** (the control file, 2026-11-09; revised by the plan review: the draft had it
   from `origin/master` at `70d7b20`). Slices 1 and 2 were squash-merged (#2, #3; `70d7b20`'s tree is
   `6f01d7f`'s), and then slice 8 (#4, `3f33be5`) as the plan was drafted; so the steps are written for
   `master`'s workspace (its `[workspace.dependencies]`, `twain-dsm` in the gates' loop and the MSRV recipes), and
   the pull request goes against `master` with this slice's commits alone. Should the branch stay on `6f01d7f`,
   the plan review's trial says what its rebase would meet ("Taken as it starts"): the justfile's lines alone, the
   rest placed to merge.
2. **Four steps:** the model with the workspace around it, and the gates' make-fails there, where the gate
   changes (moved by the plan review from step 4: a check where it can catch something, once); the fake's
   devices and batches that go right; its faults and controls; the plan's files and the slice's end, with what
   only CI shows there, as the run asks.
3. **The model as the note's block, nothing renamed, and these added:** a constructor for each of the six
   `#[non_exhaustive]` structs (the milestone's "the constructors of the structs marked `#[non_exhaustive]`"),
   the four the note doesn't name with their required fields as arguments and the rest at their plainest, all
   infallible; `RawImage::data_len`, for a reader to cap a page before its bytes; `Debug` by hand for `Opened`.
   The alternative, the note's two constructors alone, leaves the fake's scripts and an app's tests unable to
   make `Caps`, `SourceCaps` or `RawPage` outside the crate.
4. **`in-progress = "0.1.0"`, a caret requirement, and `pub use in_progress`:** its trait is in the public API,
   so an exact pin (`=0.1.0`, as probe M1 had it) would make any app holding a later 0.1.x fail to resolve,
   while the lock pins this workspace's builds; the re-export hands an app the very version the trait names.
   (The exact pins of the family's alphas are about pre-releases, whose caret takes any later alpha.)
5. **`thiserror` and `rusty-fork` from `[workspace.dependencies]`, `rusty-fork` from step 3:** `master`'s block
   holds both (its rule: a dependency joins once a second member takes it), so `scan-device` takes each with
   `workspace = true` and changes neither the block nor `sane-dl`'s manifest. It takes `rusty-fork` in step 3,
   where it's first used (revised by the plan review: the draft named it in step 1), which keeps step 1's unit
   tests building for `wasm32-wasip1` (`rusty-fork`'s `wait-timeout` doesn't compile there; the plan review's
   trial) and adds no dependency before it's used.
6. **The fake is a feature, `fake`, turned on for the crate's tests by a dev-dependency on the crate itself,**
   with `clippy` and `doc` given `--all-features`: the spec has each backend a feature, an app's shipped build
   then holds no fake, and plain `cargo test` works with nothing to remember (the plan's probe). The
   alternatives: a module always compiled (no feature to forget, but a fake in every build); `required-features`
   on the tests (`cargo test` then skips them silently); `--all-features` on `test` alone (the MSRV recipe and a
   plain `cargo test` would not build the fake's tests).
7. **The gates read each devices crate's graph with `--all-features`,** so a dependency a feature holds is read
   as a default one is; and the make-fail shows it with a codec behind a feature (step 1). `master`'s
   `twain-dsm` has no features, so its reading doesn't change.
8. **The fake's scripts are Rust values, with no text format:** the tests that use it are Rust, slice 4 can give
   its own test worker a scenario by name or decode a script once it has `serde`, and S1's `scan-fixtures`
   builds sheets from its pages. A text format (as `fake-libsane`'s) would need a parser and a format for pages
   with no user yet; it can be added without undoing anything.
9. **The fake opens as the `sane` backend will** (the note's section 1 and mapping table, for review there as
   S0 slice 2's decisions 27 to 29): it lists first; the stored id listed with the stored maker and model opens,
   with the listing's info; listed as another model, it's gone; unlisted but scripted so, it opens with the
   stored info; and it falls back by maker and model only from a `fake:libusb:<bus>:<device>` name, to exactly
   one of that form. So the app and the worker meet the model's renaming before slice 5, and slice 5's tests
   read against the fake's.
10. **The fake's clamp rules** (the nearest dpi, a tie to the higher; the first source or format listed for one
    the caps lack; duplex off where there's none; paper cut to the largest area, a size with no area `Auto`,
    `Auto` without size detection the largest area; encodings not offered dropped) live in the fake, not in a
    public `Caps::clamp`: real backends find what was clamped by reading their driver back, and a public
    function would fix one backend's policy into the model. Slice 5 may lift a helper (a range's nearest step)
    into the model if it wants one, which undoes nothing.
11. **The fake's pages:** sized from the applied paper (or the sheet's own under `Auto`) and dpi; drawn by a
    public `pattern`, the same everywhere and different by sheet and side, for slice 4's byte-for-byte check; a
    page given on a sheet delivered as given; none past 512 MiB (a 600 dpi Legal page in `Rgb16` is 257 MB), a
    `Driver` error before any allocation, so a test's mistake can't take a runner's memory.
12. **Encoded pages are stand-in bytes,** labelled with their kind and naming the sheet and side, in the first
    encoding asked that fits the page's format (G4 black and white, JPEG 8-bit): the devices layer takes no
    codec, and slice 4 carries bytes, not images.
13. **Faults:** the model's errors at a side's start or midway, each firing once; the faulted sheet left at the
    feeder's head, so Continue is a batch run again from its number, and after a fault on a back its front comes
    again (the app's to drop or keep; said in the docs); `Gone` leaving the device gone; `FeederEmpty` not
    scriptable; `Event::Sheet(n)` sent before a fault at sheet `n`'s start, so the app can say which sheet
    jammed ([M5 proposal 10](https://github.com/sheaf-pdf/sheaf/blob/3232a35/plan/m05-scan/proposals/p10-scanning-controls-errors-scanners-that-come.md)'s "Paper jam at
    sheet 14").
14. **The order of a batch's events:** `Applied` first, before `ScanError::FeederEmpty` too (SANE answers
    `NO_DOCS` after the settings are set); `Status(WarmingUp)` after it; then each sheet's `Sheet` and pages.
    The cancel check comes before each sheet is taken, so a cancel with the feeder already empty is
    `Done::Cancelled`.
15. **Hang, crash and exit tests:** the hang in-process, on a thread left blocked when the test ends; the crash
    and the exit in child processes through `rusty-fork` (vetted, in the workspace), the events printed as each
    is taken on a rendezvous channel, so the pages before the fault are printed whatever the timing. The
    alternative, the test binary spawning itself with `std::process::Command`, writes what `rusty-fork` does.
16. **The 32-bit case on every host:** the private length check takes the platform's limit, so its tests pass
    `u32::MAX` on 64-bit hosts; once, step 1's check runs them on `wasm32-wasip1` as M1's ran (no 32-bit target
    for the host is installed here, and CI has none).
17. **Version 0.0.0, `publish = false`, no changelog, docs.rs metadata or package recipe** until slice 11 makes
    it ready (as `twain-dsm`, slice 8's, waits for slice 9); a short README now, the licences at the root.
    0.0.0 is the 0.0.x the milestone asks, as the workspace's other unpublished crates are numbered.
18. **`#![warn(missing_docs)]`** beside `#![forbid(unsafe_code)]`: a model meant to be published, every item
    said; `sane-dl` doesn't have it, and nothing obliges this crate to match.
19. **Clippy for other targets only in step 3,** where the crash tests' expected exits sit behind `cfg`; the
    model has no platform code.
20. **The note gets one sentence** at the head of its model section saying slice 3 built it and where the
    additions are said (step 4), and nothing else: the additions live in `crates/scan-device.md`, and the note's
    other lines are where `master`'s slice 8 edits are.
21. **CI's `scan-device-msrv` job after `twain-dsm-msrv`, with the other MSRV jobs,** on `ubuntu-latest` as
    `sane-dl-msrv` is (the model has no platform code, and the `check` job runs its tests on all three systems).
    On `6f01d7f`, it would go at the workflow's end instead, where the rebase merges it (the plan review's trial).
    Since step 3 the crate's sources still have none, but its crash test has a `cfg(windows)` branch (a
    constant, an exit code read), which this job leaves uncompiled on 1.89, and 1.89 has no Windows target
    here either: accepted, and said in the job's comment (the code review).
22. **The bookkeeping beside its kin, off slice 8's lines** (the plan review): `decisions.md`'s MSRV bullet after
    `sane-dl`'s, not after slice 8's ruling at the end of "Made here"; backlog IDs as `master` numbers them
    (TEST-12 next, TEST-11 being slice 8's), read again from `origin/master` as step 4 starts, so no item is
    numbered twice once the Windows machine's next slice merges; slice 8's own entries in `status.md` and
    `slices/README.md` left as its machine wrote them.

Taken in the steps:

23. **The constructors take owned values** (`String`, `Vec`), as the fields hold them, with no `impl
    Into<String>` (step 1): plain, the same shape as the fields an app then sets. The alternative,
    `DeviceInfo::new` taking `impl Into<String>`, reads better in tests; widening to it later breaks only a
    caller that names the function as a pointer.
24. **Modules by concept, private, everything re-exported at the root** as the block has it (step 1): `device`
    (ids, `DeviceInfo`, `Driver`, the two traits, `Opened`, `WindowClosed`, `WindowOwner`), `caps` (capabilities
    and settings), `image` (`Format`, `RawImage`, `LayoutError`), `page` (pages, the encodings, events, `Done`),
    `error` (`ScanError`). `Encoding` sits beside `EncodedKind`, as the block orders it.
25. **`Debug` by hand names the byte count `len`** (`RawImage { format, width, height, len }`, `Encoded { kind,
    len }`), and `Opened`'s ends in `..` for the device it leaves out (step 1).
26. **How `RawImage` works its length out** (step 1): a private function gives a row's bytes and the image's in
    `u64`, refusing `Empty` and then `TooLarge` (past `u64`, or past the limit it's given); `data_len` gives it
    the platform's limit and turns the total into a `usize` with `try_from` (a second guard, never reached while
    the limit is `usize::MAX`; `isize::MAX` since the code review, which a 32-bit `usize` holds too); `new` calls
    `data_len`, then checks the length and the padding; `stride()` is worked out again from the format and the
    width, never stored.

27. **More of a script's mistakes panic as they're written** (step 2), beside the caps the plan names:
    `FakeDevice::new` with an empty name (no stored id could open it), `FakeBackend::with` given a second device
    of the same id (the opening rules would pick one silently), `Sheet::size` not finite and above 0, and caps
    holding a resolution of 0 (a list holding it, a range from it), since every page the fake makes has a `dpi`
    above 0, the model's rule the fake keeps in what it makes.
28. **The caps' `settings_window` is the device's own** (step 2): `Device::caps` says true where
    `FakeDevice::settings_window` scripts a window, whatever the `Caps` given say, so the caps and
    `settings_window()` never disagree, whichever order a script is written in.
29. **The feeder's emptiness is read before the sheet's number** (step 2; the plan had the number first): both
    under one lock, the number after the emptiness, so a batch whose last sheet was numbered `u32::MAX` and whose
    feeder is then empty ends as `Done::FeederEmpty`, not a `Driver` error; past `u32::MAX` with paper left, the
    `Driver` error, the sheet left in the feeder, as the plan has it.
30. **A sheet whose page fails goes back at the feeder's head** (step 2): in this step only the 512 MiB cap (and a
    size `RawImage::data_len` refuses) ends a batch at a side; the sheet wasn't scanned, so it's put back, as step
    3's faults will put theirs; `fed()` still counts its taking. The cap applies to every page drawn, an
    encoded stand-in included (the size the page would be), never to a page given on a sheet.
31. **A flatbed's batch sends `Event::Sheet(first_sheet)`** before its page, as a feeder's sheet does, and its
    sheet stays on the glass (`fed()` unchanged); duplex, where a test's caps give a flatbed one, reads both
    sides.
32. **`pattern`'s bytes** (step 2): a first row from the sheet and side by Marsaglia's xorshift (shifts 13, 7 and
    17, eight bytes a step), each row its bytes turned left by the row's number in pixels (bytes in `Bw1`), then
    its padding bits cleared (so worded by the code review: at a `Bw1` width not a multiple of 8, a row isn't
    the one before it turned, the bits cleared in one coming back in the next); pinned in
    a unit test by values a separate drawing (`scratch/work/s0-3/step-2/pattern-golden.py`, never committed)
    gives, so a change slice 4's byte-for-byte check would meet is seen here first.
33. **Small names and forms** (step 2): `NotInstalled`'s `what` "the fake driver"; a USB name
    `fake:libusb:<bus>:<device>`, each one or more ASCII digits; `DeviceInfo::name` the maker and model joined by a
    space, an empty one left out; a stand-in's bytes `fake <form> page: sheet <n>, <side>`, a G4 one's kind
    `black_is_1: true`, the fax convention; `FakeBackend`, `FakeDevice` and `Sheet` derive `Debug` (and `Sheet`
    `Clone` and `PartialEq`), for a test's messages.
34. **Step 2's tests take some of what step 3 tests at length** (cancelled before the batch, a receiver gone before
    it, the number past `u32::MAX`, warming up, the paper read while a batch waits on a rendezvous channel),
    since step 2's code does them and each needed a make-fail; step 3's scripts of proposal 22 come on top.
35. **A sheet holds a list of faults, each taken from it as it fires** (step 3): `Sheet::fault` may be called
    more than once, on any side and point; a side's faults at one point fire in the order scripted, one a batch,
    so a sheet that jams twice can be scripted; a fault on a side a batch doesn't read (a back in simplex) doesn't
    fire. The alternative, one fault a side, the last given replacing the one before, couldn't script a jam that
    comes back.
36. **Midway is after the first half of a raw page's rows, rounded down,** counted on the progress, and for an
    encoded page, whose bytes are counted at once, after its total is set and before its bytes (step 3). The page
    is drawn whole before its rows are counted, so a midway fault comes after the drawing; a page one row high
    faults before its row.
37. **A hang parks the batch's thread for good, its sheet left taken** (step 3), not put back: a hung driver's
    call holds the paper in the scanner, and what ends it (slice 4's watchdog) ends the process and its state.
    Parked again if woken all the same; the paper's lock isn't held while it waits.
38. **The glass keeps its sheet without the faults that fired** (step 3): a flatbed's batch writes the sheet it
    read back to the glass, unless another was put there meanwhile (a count of sheets put on the glass tells
    it), so a fault on the glass fires once, as one in the feeder does.
39. **A device gone is gone to every caller** (step 3): its state is shared by its clones, so every backend
    holding it lists it no more and finds it by its id no more, while the fallback may still open another device
    of its maker and model under another USB name, as a replugged scanner would be found; `caps`, `scan` (before
    `Applied`, nothing sent) and `settings_window` on a device already open answer `Gone`. A device scripted to
    `refuses_open(Gone)` stays a refusal at open (step 2), not a device gone.
40. **Two tests made deterministic** (step 3): the cancel after a duplex front also cancels once the front is read
    whole (its rows all counted on the progress) but before it's taken, which a check between front and back
    always meets, where a cancel after the front is taken races that check and was seen to miss it; and the crash
    and exit tests' children write each event's line by one `write_all`, the parent counting whole lines alone,
    since `process::exit` on the batch's thread flushes the standard output and makes it unbuffered, so a line
    written in pieces by the reading thread could end part-written (seen once in 16 runs).
41. **Small forms in the fault tests** (step 3): the exit's code 42, neither a pass, a test's panic (101) nor
    `rusty-fork`'s (70); the abort read as `sane-dl`'s abort test reads it, `SIGABRT` (6) on Unix and the code
    `0xC0000409` on Windows, `cfg` for each, any other system failing the test; the child's events marked
    `fake-event: ` among the test harness's own lines; waits bounded at 10 s and polled every 5 ms, a state
    watched for 300 ms.

Taken in the code review's fixes:

42. **`Paper` and the fake's `When` `#[non_exhaustive]`,** a departure from the note's block, written into the
    note with a sentence at its head: paper may yet grow (an area with an offset, SANE's `tl-x` and `tl-y` or
    TWAIN's `ICAP_FRAMES`; a detection bounded by a size), section 2's "To settle" says so, and a variant added
    after slice 11's freeze would break every exhaustive match, slice 4's conversions among them; `When` beside
    a `Fault` that already was. Nothing outside the crate matched either. The rest (the exhaustive `Resolutions`,
    `Side` and `SideKnown`; `Copy` on growing enums; the plain structs) is slice 11's to weigh: Q-6.
43. **`Backend::open`'s order is the fake's: `NotInstalled` may come before `BadId`,** which comes before the
    backend lists or opens anything; the trait's docs say so, and the fake and its test are unchanged. A driver
    API not installed is what an app can tell its user to fix, while an id no driver forms is the app's own
    storage gone wrong; and a backend not installed has nothing to read an id against. Slices 5, 9 and 10 keep
    the same order. The alternative, the id checked first in every backend, changes a tested behaviour for no
    caller.
44. **`pattern`'s docs follow its bytes,** not the bytes the docs: each row the first row's bytes turned by its
    number, then its padding cleared. The pinned bytes stay, and slice 4's byte-for-byte check compares with
    `pattern` itself; a code change would have moved every `Bw1` page for a sentence.
45. **The build an app ships checked in `just clippy` and `just doc` for every member,** `cargo clippy
    --workspace --lib` with default features and a default-feature `cargo doc` before the every-feature one,
    rather than `cargo hack --each-feature` (a tool CI doesn't have, and each feature alone matters once slice
    5 brings a second). Both run on CI's three systems through the `check` job.
46. **The libsane gate builds every binary with every feature on,** as `deps()` reads the graphs (decision 7),
    rather than recording that only default builds are read: a backend's feature (slice 5's `sane`) is exactly
    where a link would come from.
47. **The cancel's and the receiver's rules said, the behaviour kept:** a cancel doesn't wake a `send` held at a
    full channel, so the caller keeps taking events or drops the receiver (the docs say so); and a sheet taken
    before a `send` fails for a receiver gone isn't put back, since nobody takes pages any more and a driver's
    start has fed it (`Done::Cancelled`'s docs say so). Making the fake wake a held `send` on a cancel would
    promise what no real backend's blocking driver call can keep.
48. **Backlog IDs with slice 1a beside it:** this slice's TEST-12 is kept although slice 1a's branch (at
    `1f6d3f6`) took TEST-12 too: whichever merges second renumbers (the control file). The new items take IDs
    free on both branches and on `origin/master`: TEST-13 and Q-6.
49. **Slices 1 and 2's plans moved to `archive/slices/`** (the run's control file: both merged, as #2 and #3),
    every reference to them updated but `crates/sane-dl.md`'s one, which this slice leaves alone while slice 1a
    changes that file (the control file): its merge updates it.
50. **This plan's decisions for review aren't filed one by one in the backlog:** `status.md`'s "Waiting on
    review" names them by plan, with their counts, as slices 1, 2 and 8 have.

## For review (security)

1. **`RawImage::new` is where a page from a worker becomes a page** (slice 4 decodes through it): its sizes are
   worked out in checked `u64` against what one buffer can hold on the platform (`isize::MAX` bytes since the
   code review: item 6), so a 32-bit worker's or client's arithmetic can't wrap to a small buffer that a short
   `data` would match; its length is exact; `data_len` gives a reader the same answer before it reads, so it
   allocates once and never on a header's word alone.
2. **The fake's `Crash` and `Exit` end the process they run in,** and `Hang` holds a thread for good. They exist
   only behind the `fake` feature and fire only where a script asks; scripts are Rust values a test builds, not
   input from a pipe or a file, so no worker built with the fake can be made to crash by what it's sent. The docs
   say the feature is for tests and never for a shipped build; slice 4 keeps the fake to its test worker.
3. **A stored id is untrusted** (the app's storage): the fake refuses one not of its form before it lists or
   opens anything (a backend not installed says so first: the code review's decision 43), as `BadId`, which
   carries nothing of the id, as the note's SANE backend will.
4. **The renamed-device fallback opens a device the user didn't name:** in the fake, as the note has SANE's,
   only from a USB connection's name, to exactly one listed device of that form with the stored maker and model,
   and said (`renamed_from`); a name now another model's is never opened as the stored device.
5. **No page is printed:** `Debug` for `RawImage` and `Encoded` gives sizes, never bytes, so a log line or a
   failed assertion can't write a page (about 100 MB at 600 dpi color) or its content.
6. **A page's size from a hostile header can't panic the reader that sizes its buffer by `data_len`** (the code
   review's major finding, fixed in `eac0b5b`). The limit had been `usize::MAX`, but no allocation passes
   `isize::MAX`: on a 32-bit system (slice 9's worker) `data_len` answered `Ok` for 2^31 to 2^32 − 1 bytes, and
   a reader that allocated that length, as the docs tell it to, panicked with a capacity overflow, a crash a
   driver's or a pipe's header could cause, where `TooLarge` was promised. Now `isize::MAX`, held by unit
   tests on any host through the private limit and run on a 32-bit `usize` under WASI; whether the memory is
   there stays the reader's cap to say (`plan.md`'s hostile-input rule: slice 4's client caps a page before it
   allocates).

## Risks

1. **The note's types meet the crate's compiler** with docs, modules and tests around them: probe M1 compiled
   the block as written, so the risk is small; a departure is said in "So far" and in `crates/scan-device.md`.
2. **The dev-dependency on the crate itself:** cargo's handling of it (features unified for tests, not for
   `build` or `doc`) is shown by the plan's probe on stable and 1.89, not documented as a promise; if a cargo
   changes it, the fallback is `--all-features` in `test` and the MSRV recipe (decision 6's alternative).
3. **Timing in tests** (a held channel, a hang, deadlines): each waits, with a bound of seconds, for the state
   it then asserts hasn't moved (the held channel waits for `fed()` to reach 1 before its 300 ms), which a slow
   runner can't break, or takes a deadline with a wide margin (a listing ten times slower than its deadline);
   step 3 runs them 50 times under load once; Windows and macOS runners are CI's to show.
4. **The child processes on Windows and macOS** (`rusty-fork`, an abort's exit): `sane-dl`'s abort test runs the
   same way on CI's three systems already; the expected exit sits behind `cfg`, checked here with clippy for
   both targets; only CI runs them.
5. **`master` moves again during the slice:** the Windows machine pushes to `master` through pull requests, and
   another slice changes `sane-dl` beside this one. This slice keeps off `crates/sane-dl/` and
   `crates/sane-dl.md`, edits `status.md`, `decisions.md` and the backlog only where it must, and step 4 reads
   `master`'s backlog IDs again as it starts; a merge after the run is the main session's.
6. **`in-progress` in the public API** (S0 slice 2's Risk 6): young, the org's, 0.x; the re-export keeps an
   app on its version, and a 0.2 is a change here, said.
7. **The fake outgrows its use:** it does what the spec, proposal 22 and slices 4 and S1 need, and no more (no
   text format, no one-open-at-a-time, no option model); a want beyond that becomes a backlog item for the slice
   that wants it.
8. **The base moved under the run** (decision 1): `master` took slices 1, 2 and then 8 while this plan was
   drafted, and the branch moves onto it before step 1. Where this plan still reads as `6f01d7f`'s, `master`
   holds (the control file's note to the implementation agents), each departure said in "So far".

## Plan review

2026-11-09, adversarial, through three lenses (fidelity to the milestone, the spec, the note and the rules;
mechanics, each step green on its own on three systems; behaviour and proof). Each finding was checked against
the sources or a run before it was acted on; the trials are local (`scratch/work/s0-3/review/`): the plan's
self dev-dependency probe run again on stable and 1.89, a crate with and without `rusty-fork` built for
`wasm32-wasip1`, and step 1's manifests, lock, CI job and justfile lines simulated on a worktree of `6f01d7f`
and merged onto `master` as the rebase would (the worktree removed after). No FFI in this slice:
`#![forbid(unsafe_code)]`, so the FFI questions of the third lens don't arise.

**Accepted:**

1. *Fidelity, major: the base and the trial merge were stale.* The draft made the branch from `origin/master`
   (`70d7b20`), where the run named `6f01d7f`, and its per-step trial merged with `origin/s0-8-twain-dsm`, which
   no longer exists: slice 8 was squash-merged as `3f33be5` (#4) just after the draft, its branch pruned
   (`git reflog refs/remotes/origin/master`), and a plain merge of `6f01d7f` with the new `master` (their merge
   base `d6a434e`) conflicts in eight files. Meanwhile the control file (2026-11-09) had the main session move
   the branch onto `3f33be5` before step 1. **Changed:** the branch made from `6f01d7f` as the job gave it, the
   plan committed there, and the steps written for `master`'s workspace, which they start from (decision 1):
   `thiserror` and `rusty-fork` from its block, `sane-dl`'s manifest untouched, the gates' loop `sane-dl twain-dsm
   scan-device`, the MSRV lines beside `twain-dsm`'s, the base's test list recorded at `3f33be5`; the rebase's
   trial (`--merge-base 6f01d7f HEAD origin/master`) kept only should the branch stay on `6f01d7f`; "Taken as it
   starts", "The workspace around it", the steps' core, the baselines and Risks 5 and 8 revised to it.
2. *Mechanics, major: `rusty-fork` in step 1 breaks step 1's 32-bit run.* `wait-timeout` 0.2.1 doesn't compile
   for `wasm32-wasip1` (`E0433`, its `imp` module), so with `rusty-fork` a dev-dependency the unit-test binary
   can't be built there; without it, a crate on `in-progress` and `thiserror` built and ran its test under
   Node's WASI with a 32-bit `usize`. **Changed:** `scan-device` takes `rusty-fork` in step 3, where it's first
   used, and step 3 runs the MSRV probe again (decision 5; "The crate").
3. *Fidelity, major: a backlog ID taken twice.* Step 4 would number a new test item TEST-11, which `master`'s
   slice 8 holds (`twain-dsm`'s decoders fuzzed; `backlog/tests.md` there: "the next new one is TEST-12").
   **Changed:** IDs as `master` numbers them, read again as step 4 starts and checked there with `git grep`
   (step 4, decision 22).
4. *Mechanics, minor: the lock changes at steps 2 and 3,* not only at step 1 (the draft: "nothing else"): the
   dev-dependency on the crate itself puts `scan-device` in its own entry's dependencies (the trial's lock), and
   `rusty-fork` joins it in step 3; `--locked` fails until each is committed. **Changed:** the workspace's
   bullet and steps 2 and 3.
5. *Mechanics, minor: clippy's `new_without_default`* fires on `FakeBackend::new()` and `Sheet::new()`, an error
   under `-D warnings`. **Changed:** both implement `Default`.
6. *Behaviour, minor: the held-channel test could fail on a slow runner:* `fed()` asserted 1 after a fixed 300
   ms, before which the batch's thread may not have reached sheet 1; probe M1 waited first. **Changed:** it
   waits up to 10 s for `fed()` to reach 1, then 300 ms more (step 3, Risk 3).
7. *Behaviour, minor: the shared state's lock across a wait.* Held across a `send` or a hang, it would deadlock
   the tests that read `fed()` while a batch is held or hung. **Changed:** never held across a `send`, a
   `Progress` call or a hang; the hang test reads `fed()` too; `fed()` counts each taking, a faulted sheet
   counted again when taken again.
8. *Behaviour, minor: a range's clamp could pass its `max`.* "Clamped to its ends and to the nearest step from
   `min`" takes 100 to 110 where `min` is 50, `max` 100 and `step` 30. **Changed:** the nearest step not past
   `max`, in `u64`, with a test of a `max` off its steps.
9. *Behaviour, minor: three rules unsaid:* `Auto` on a source with neither size detection nor a largest area
   (kept `Auto`, the sheet's own size); a sheet number past `u32::MAX` (worked out before the sheet is taken,
   which stays in the feeder); a device gone (unlisted from then on). **Changed:** each said where its rule is.
10. *Fidelity, minor: `Fault` and `When` in step 2 without behaviour,* a script that did nothing for a step.
    **Changed:** `Fault`, `When` and `Sheet::fault` all come in step 3.
11. *Proof, minor: the gates' make-fails three steps after the gate changes.* The loop and `deps()` change in
    step 1, nothing later touches them. **Changed:** the make-fails in step 1's check; steps 2 to 4 leave them out,
    said (decision 2).
12. *Mechanics, minor: two edits on slice 8's new lines.* CI's job beside `sane-dl-msrv` would land where
    `master` inserted slice 8's two jobs, and a `decisions.md` bullet at the end of "Made here" after slice 8's
    ruling. **Changed:** the job after `twain-dsm-msrv`, with the MSRV jobs (at the workflow's end, where the
    trial merged it cleanly, should the branch stay on `6f01d7f`), the bullet after `sane-dl`'s MSRV bullet
    (decisions 21 and 22).
13. *Proof, minor: three tests' wording.* A `NaN` asked for compared with `==` fails against itself (`is_nan`
    instead); "no sheet taken after" a receiver gone means after the failed `send` (in simplex the next sheet
    is taken before its `Sheet` event fails, as a SANE start feeds it); the crash and exit tests read the
    child's status and output through `rusty_fork::fork`'s supervisor, as `sane-dl`'s `Child` does, the events
    flushed as printed. **Changed:** step 2's and step 3's tests say so.
14. *Hygiene, minor: probe M1's "README.md" doesn't exist.* **Changed:** its `src/lib.rs`, and slice 2's plan
    where it's described.

**Rejected, as checked:**

1. *The dev-dependency on the crate itself breaks a recipe.* The probe, run again: `cargo test` (unit,
   integration and doctests), `clippy -D warnings`, `doc -D warnings` and `cargo tree -e normal,build` clean on
   stable, `cargo +1.89 test` too; plain `cargo doc` leaves the fake out, `--all-features` documents it.
2. *`in-progress` should be pinned exactly.* Slice 2's plan prescribes no pin; the exact pins are the PDF
   family's alphas' rule, about pre-releases; a caret on 0.1.0 takes 0.1.x alone, and the lock pins this
   workspace's builds (decision 4 stands).
3. *`--all-features` on the workspace's `clippy` and `doc` breaks on `master`'s `twain-dsm`.* Its manifest at
   `3f33be5` has no `[features]`; `sane-dl` and `fake-libsane` have none either.
4. *`Fault::Exit` is beyond the milestone.* Slice 4's client says how a worker exited, a signal for the abort on
   Unix and a code for an exit; one variant gives it the second shape.
5. *The layout figures.* In `Gray8`, (2^32 − 1)^2 = 2^64 − 2^33 + 1 fits `u64`; two bytes a pixel or more at
   `u32::MAX` square don't; 65,536 square in `Rgb16` is 6 × 2^32, 0 modulo 2^32; `thiserror` 2.0.21 declares
   1.77 and `in-progress` 0.1.0 1.85 (their manifests in the cargo home).
6. *Letter at 300 dpi isn't 2550 by 3300 in `f32`.* 215.9 as `f32` gives 2549.9999…, rounded to 2550, and pages
   are sized in `f64` anyway.
7. *The cancel after a duplex front races on a rendezvous channel.* The back's `send` can't return before the
   test takes it, and the next `check()` comes after it, so a cancel made once the front is taken always lands
   between that sheet and the next.
8. *The manifests and the lock conflict at a rebase from `6f01d7f`.* The trial merged `/Cargo.toml`, `sane-dl`'s
   manifest and `/Cargo.lock` cleanly (moot once the branch starts from `master`).
9. *The 512 MiB cap can't be reached.* With the default caps the largest page is 245 MiB (600 dpi Legal in
   `Rgb16`); step 2's test reaches it through caps it gives (a resolution or an area past the defaults), as
   intended.
10. *Other targets' clippy needs the network.* `windows-sys`, `rustix` and the rest `rusty-fork` pulls there are
    in the cargo home, and both targets' standard libraries are installed.

## So far

**Step 1, the crate and its model; the workspace around it** (2026-11-09; one commit, "Add scan-device: the
device model, provisional", on `2a6cc3c`, the branch `3f33be5` and the plan's commit). The checks before it:
none (the plan review's are in "Plan review"; no step had been checked).

- **The crate:** `/crates/scan-device/` 0.0.0, edition 2024, `rust-version` 1.89, `publish = false`, `in-progress
  = "0.1.0"` and `thiserror.workspace = true`, `[lints] workspace = true`; `#![forbid(unsafe_code)]` and
  `#![warn(missing_docs)]`; crate docs (the model, devices and their ids, a batch and its events' order, the
  channel, cancel and progress, the raw layout, stability; no example until the fake's, step 2) and a short
  README (the licences at the root, linked). Modules `device`, `caps`, `image`, `page` and `error`, everything
  re-exported at the root, and `pub use in_progress` (decision 24).
- **The model:** the note's block, its 31 items, each struct's and enum's body and each trait method's
  signature as the block writes them, derives and `#[non_exhaustive]` the same, `SizeMm`'s three constants;
  read by a script against the block (`scratch/work/s0-3/step-1/model-vs-note.py`, "31 items ... 0 differ"),
  the block's doc comments kept on their items. Added as "The model, as built" has it: the six constructors
  (decision 23), `RawImage::new` with its four checks in order, `data_len`, the accessors and `into_data`
  (decision 26), and `Debug` by hand for `RawImage`, `Encoded` and `Opened` (decision 25).
- **Tests:** 11 units (`image`: each format's layout taken and the length refused short and long; a row's bits
  rounded up; `Empty`; a set padding bit at 13 pixels a row, its row named, the first such row; `TooLarge` past
  `u64` at `u32::MAX` square for `Gray16`, `Rgb8` and `Rgb16`, `Bw1` and `Gray8` fitting `u64` and refused by
  the length here, by the limit on 32 bits; 65,536 square in `Rgb16` past a 32-bit `usize` through the
  parameter, a `Length` here, `TooLarge` on 32 bits, the limit itself held; `data_len` agreeing with `new`;
  `Debug` without a byte; the layout errors' messages; `caps`: Letter and Legal whole inches, not whole
  millimetres; `page`: `Encoded`'s `Debug`), two doctests (`RawImage::new`, `data_len`), and `tests/model.rs`, 9
  tests from outside the crate (each `#[non_exhaustive]` struct by its constructor, its defaults, its fields
  set; the trait objects, `Opened`, `Event`, `SyncSender<Event>` and `ScanError` `Send`; `Opened` through a
  boxed test backend, its `Debug`; each `ScanError`'s message; a push-shaped test device moving a child
  progress and a `SyncSender` clone into a thread it spawns: a batch to the end, the caller's `Tracker` at 1,
  and a cancel from a third thread while sheet 1's page waits on a rendezvous channel, the page delivered and
  nothing after, `Done::Cancelled`).
- **The workspace:** `/Cargo.toml`'s member line after `sane-dl`'s; `/Cargo.lock` gains `in-progress` 0.1.0 and
  `scan-device`, nothing else ("Locking 1 package to highest Rust 1.89 compatible version", offline); the
  justfile's `scan_device_msrv`, `scan-device-msrv`, `msrv` naming all three, the gates' loop `sane-dl twain-dsm
  scan-device`, `deps()` with `--all-features`, and the gates' heading comment saying so; CI's `scan-device-msrv`
  job after `twain-dsm-msrv`, on `ubuntu-latest` and 1.89; the root README's row after `twain-dsm`'s and its
  "Next" line `scan-worker` alone. Nothing in `crates/sane-dl/` or `crates/sane-dl.md`.
- **The MSRV probe** (`scratch/work/s0-3/msrv-probe/`, its README): over `cargo tree -e normal,build,dev
  --target all`, the probe's (newest versions, resolved offline from the cargo home's index) and the
  workspace's own graph alike, the highest `rust-version` is `in-progress` 0.1.0's 1.85, then `thiserror`'s
  1.77: so the family's floor, 1.89. `cargo tree -p scan-device -e normal,build --target all --all-features`:
  8 packages (`scan-device`, `in-progress`, `thiserror`, `thiserror-impl`, `proc-macro2`, `quote`, `syn`,
  `unicode-ident`).
- **Quick checks, the implementation's:** `cargo fmt --all -- --check`, `cargo clippy --locked --workspace
  --all-targets -- -D warnings` and `cargo test --locked -p scan-device` green; `cargo doc -p scan-device
  --no-deps` with `-D warnings` clean; `cargo +1.89 check -p scan-device --all-targets`; the unit tests built
  for `wasm32-wasip1` and run once under Node's WASI (11 passed, the 32-bit branches taken;
  `scratch/work/s0-3/step-1/wasi/`), ahead of the check's own run, since a wrong 32-bit expectation would
  otherwise cost a repair. Make-fails on a worktree of its own (`scratch/work/s0-3/step-1/make-fail.md`): 17
  mutations (the padding check gone, its mask turned, the row misnamed; the length in wrapping arithmetic; no
  platform limit; no `Empty`; `Bw1` rows rounded down; `Debug` with the bytes, for `RawImage` and `Encoded`;
  Letter whole millimetres; a constructor's default changed, three ways; a `ScanError`'s message; `Opened`'s
  `Debug`; the push device's check between sheets gone, and its child progress replaced), each failing a test;
  the worktree removed.
- **Departures:** none from the model or the steps. The gates' heading comment, which the plan didn't name,
  says the graph is read with every feature on. `master`'s slice 8 changes the control file lists were already
  the plan's (written for `3f33be5`), so nothing there departs.
- **Decisions:** 23 to 26 ("Taken in the steps").

**Step 2, the fake: devices, listing and opening, and batches that go right** (2026-11-09; one commit, "Add
scan-device's fake backend: devices and batches", on `566b0ae`). The checks before it: step 1's check passed on
`566b0ae` (`scratch/work/s0-3/flow/checks.md`): `just ci` green with SANE required, its MSRV recipes on 1.89,
`scan-device-msrv` among them; 268 tests, the base's 246 at `3f33be5` and scan-device's 22, none lost; the gates
naming `scan-device` with 8 packages; its MSRV 1.89, `in-progress`'s 1.85 the highest in its graph; the
`wasm32-wasip1` run, 11 passed; four mutations of `RawImage::new`, each failing a test; both gate make-fails
(`png` as a normal and as an optional dependency) failing with `scan-device` and `png` named; the model the same
as the note's block on all 32 items; the push device's tests run 300 times, none failing.

- **The feature and the manifests:** `fake = []` in `/crates/scan-device/Cargo.toml` and the dev-dependency on the
  crate itself with it; `/Cargo.lock` puts `scan-device` in its own entry's dependencies, nothing else ("Locking
  0 packages", offline). The justfile's `clippy` and `doc` take `--all-features`, their comments saying why;
  `test` and `scan-device-msrv` need nothing (the dev-dependency).
- **`scan_device::fake`** (`src/fake/`: `mod.rs` the backend, its devices, sheets and opening; `clamp.rs`;
  `batch.rs`; `pattern.rs`), as "The `fake` backend" has it, without faults: `FakeBackend` (`new`, `Default`,
  `with`, `listing_takes`, `not_installed`); `FakeDevice` (`new`, `info`, `caps`, `unlisted`, `refuses_open`,
  `reports_sides`, `warms_up`, `settings_window`, `load`, `glass`, `fed`, `in_feeder`), its paper behind one
  `Arc<Mutex<…>>` that clones share, locked only to take, put back or read a sheet; the device a backend opens a
  private type of its own; `Sheet` (`new`, `Default`, `size`, `side`); `pattern`. Listing and opening as "Listing
  and opening" says, read against the note's section 1 and its mapping table a row each
  (`scratch/work/s0-3/step-2/opening-vs-note.md`); the clamp rules, the batch's order, page sizes, encoded
  stand-ins, the 512 MiB cap and progress as "A batch" says. Module docs with the rules and an example; `Fault`,
  `When` and `Sheet::fault` wait for step 3, as planned.
- **The crate docs** gain "Features" and "Example" (listed, opened, caps read, a duplex batch on its own thread
  with a `Tracker` and `sync_channel(2)`, the events read), naming the fake's items in code only, so `cargo doc
  -p scan-device --no-deps` without the feature stays clean under `-D warnings` (run); the README a "Features"
  section and its clause; the root README's row the fake.
- **Tests:** 11 units (`fake`: ids of the fake's form, USB names, the name shown; `clamp`: a range's nearest step
  never past its `max`, at the top of `u32` too, and a list's nearest, ties to the higher; `batch`: a dimension's
  pixels, a page of exactly 512 MiB drawn and one byte more refused, each encoding's formats, a stand-in's bytes;
  `pattern`: each row the one before turned a pixel, its bytes pinned), 3 doctests (the crate's example, the
  module's, `pattern`'s), `tests/fake_devices.rs` (22: listing and its deadline, slower, as long as it, zero;
  `NotInstalled` first; opened as stored with the listing's info; the milestone's renamed device found by maker
  and model; a stored name now another model's taken as gone, and found under another USB name where that model
  is; the fallback needing both maker and model, never to one of two, only from and to a USB name, never to an
  unlisted device; an unlisted device with the stored info; a name not there; `BadId` before the listing; `Busy`,
  `AccessDenied`, `Gone` and a `Driver` refusal once found; default and scripted caps; the settings window
  `Unsupported`, `Accepted`, `Dismissed`; the script's mistakes panicking) and `tests/fake_batches.rs` (35:
  simplex and duplex, sides inferred and reported, "Scan More" numbered on, `ScanError::FeederEmpty` after
  `Applied` (and after `WarmingUp`), `Done::FeederEmpty`, the flatbed's `Done::OnePage` from its glass, every
  clamp rule's `Clamped` and none where nothing changed, all of them at once in the settings' order, a range's
  `max` off its steps never passed, wild settings on both sources without a panic (`NaN` matched with
  `is_nan`), page sizes (Letter at 300 dpi 2550 by 3300, A4, Legal, at least a pixel) and `Auto` the sheet's own
  cut to the largest, drawn pages `pattern`'s, `pattern` the same each run and different by sheet and side, its
  `Bw1` padding 0 at every width from 1 to 17, a page given delivered as given, encoded pages in the first form
  that fits and raw where none does, the 512 MiB cap with the pages before kept and the sheet put back, each
  page's progress (a recording `Progress`) and the tracker finished at the end, a tracker cancelled before the
  batch, a receiver gone, the number past `u32::MAX`, and the paper read while a batch waits on a rendezvous
  channel). scan-device's tests: 22 before, 93 now (22 units, 9 + 22 + 35 integration, 5 doctests): 339 in the
  workspace expected, none lost.
- **Quick checks, the implementation's:** `cargo fmt --all -- --check`, `cargo clippy --locked --workspace
  --all-targets --all-features -- -D warnings` and `cargo test --locked -p scan-device` green; `cargo doc -p
  scan-device --no-deps` with and without `--all-features`, `-D warnings`, clean; `cargo +1.89 check --locked -p
  scan-device --all-targets`. Make-fails on a worktree of its own (`scratch/work/s0-3/step-2/make-fail.md`, the
  script `mutate.py` beside it): 49 mutations, each failing a test, among them the plan's two of the fallback
  (the first match taken among several: `the_fallback_is_never_to_one_of_two`; maker and model not compared:
  five tests), the maker alone and the model alone compared, the fallback from or to a name of any form, a range
  clamped past its `max`, either tie to the lower, no check before a sheet, the number wrapping, the sheet not put
  back, the 512 MiB cap at `>=` and gone, progress a page at once, the paper locked across the sends (two tests
  failing at a 10 s bound, not hanging), and the pattern's padding, sides and turns; one test sharpened after its
  mutation was caught only elsewhere; the worktree removed.
- **The page rate,** for slice 4's throughput gate (2.3 pages a second, 58 MB/s), measured once in a release
  build from a scratch program (`scratch/work/s0-3/step-2/rate/`, never committed; `rate.txt`): 300 dpi Letter
  from `pattern`, about 560 pages a second in `Rgb8` (14 GB/s) and 8,000 to 18,000 in `Bw1`; from a whole batch
  through `sync_channel(1)` with a `Tracker`, about 500 a second in `Rgb8` simplex and 6,000 to 9,800 in `Bw1`,
  and 49 a second in `Rgb8` duplex, where each page came in memory fresh from the system (1.23 million minor
  faults for 100 pages against 12,825 simplex, `/usr/bin/time`): the allocator's cost, not the drawing's. The
  fake is never what limits the gate.
- **Departures:** the forbidden caps include a resolution of 0, and other script mistakes panic (decision 27); the
  feeder's emptiness read before the number (decision 29); a sheet whose page fails put back (decision 30); a
  module example beside the crate's. No other: names as the plan's design has them, nothing renamed.
- **Decisions:** 27 to 34 ("Taken in the steps").

**Step 3, the fake's faults and controls** (2026-11-09; one commit, "Add scan-device's fake faults: errors, hangs,
crashes and exits", on `65c42c1`). The checks before it: step 2's check passed on `65c42c1`
(`scratch/work/s0-3/flow/checks.md`): `just ci` green with SANE required, its clippy and doc with `--all-features`,
`scan-device-msrv` on 1.89 running the fake's tests; 339 tests, 71 added, all scan-device's, none lost; `cargo doc
-p scan-device --no-deps` without the feature clean under `-D warnings`; the fake's opening rules held against the
note's section 1 and its mapping table, 14 rows; both fallback mutations failing tests (one and five); the page
rate measured again, about 490 a second in `Rgb8` simplex and 49 in duplex, twenty times slice 4's gate at its
slowest; the gates' graph of `scan-device` still 8 packages.

- **The manifest and the lock:** `rusty-fork.workspace = true` among the dev-dependencies, with a comment; the lock
  puts `rusty-fork` in `scan-device`'s own entry, nothing else. The MSRV probe run again over the graph it joins
  (`scratch/work/s0-3/step-3/msrv-probe/`): 25 packages, the highest `rust-version` 1.85 (`getrandom` 0.4.3,
  through `tempfile`, and `in-progress`), so `scan-device` stays at 1.89; the probe's graph and the workspace's are
  the same packages and versions. `cargo tree -p scan-device -e normal,build --target all --all-features`: the
  same 8 packages as step 1's.
- **`Fault`, `When` and `Sheet::fault`** (`src/fake/mod.rs`), as the design has them: `When { Start, Midway }`,
  `Fault { Error(ScanError), Hang, Crash, Exit(i32) }` `#[non_exhaustive]`, both `Clone`, `Debug`, `PartialEq` and
  `Eq` (`When` `Copy`), and `Sheet::fault(side, when, fault)`, which panics on `Fault::Error(ScanError::FeederEmpty)`.
  A sheet holds its faults not yet fired, in order (decision 35). The batch (`src/fake/batch.rs`) fires a side's
  `Start` fault after the sheet's `Event::Sheet` (and a back's after its front's page), and its `Midway` fault in
  the progress loop, after the first half of its rows (decision 36); each fault is taken from the sheet as it
  fires. `Error` ends the batch with its error, the pages before it kept, the side dropped, the sheet put back at
  the feeder's head without the fault (the step 2 path, decision 30), or left on the glass (decision 38); `Gone`
  also marks the device gone in the state its clones share, so it's listed no more, `open` doesn't find it by its
  id from any backend (the fallback may still find another device in its place), and `caps`, `scan` and
  `settings_window` on a device already open answer `Gone` (decision 39). `Hang` parks the batch's thread for good,
  its sheet taken (decision 37); `Crash` is `std::process::abort`, `Exit(code)` `std::process::exit(code)`. Cancel
  between sheets, the channel's hold, a failed `send` and the count past `u32::MAX` were step 2's code; this step
  tests them at length.
- **The docs:** the fake module's "Faults" section (each fault, firing once, Continue, a device gone, and how a
  test of a crash or an exit takes its events), each new item's doc (`Fault::Crash` and `Fault::Exit` for a test's
  or a worker's process, never an app's), the crate docs' and the README's `fake` clause, each saying the faults
  can block a thread for good and end the process.
- **Tests:** `tests/fake_faults.rs`, 19, every script of proposal 22 that step 2 hadn't: a jam at sheet 4's start
  then Continue from sheet 4 to the feeder's end (sheet 4 said before its jam, put back before 5 and 6, scanned as
  `pattern` draws it, counted at each taking); a jam midway through a back (the front kept, the back's first 275
  rows counted, then Continue giving the front again); a double feed midway; an open cover before the first page,
  after the warm-up, then the same batch whole; a `Driver` error shown and one not, at a start and midway; faults on
  one side fired once each in the order scripted, a start's before a midway's; a fault on a back in simplex not
  firing; midway through an encoded page before its bytes, and through a page one row high; a fault on the glass
  firing once, and a sheet put on the glass during a batch kept there; `FeederEmpty` not scriptable; `Gone` midway
  through sheet 2, then `caps`, `settings_window` and a batch `Gone` (nothing sent, nothing taken), the device
  unlisted and not opened from either backend holding it, and a twin under another USB name found in its place; a
  cancel mid-page from another thread through a test's own `Progress` at a page's middle row (sheet 2's page in
  simplex, its front's and its back's in duplex: the sheet finished whole, `Done::Cancelled`, `fed()` 2); a cancel
  after a duplex front on a rendezvous channel, once the front is taken and once it's read whole but waiting to be
  taken (its back delivered, nothing after; decision 40); a batch held by a full channel (bound 1, nobody reading:
  `fed()` waited on until 1 for up to 10 s, still 1 and the batch unfinished 300 ms later, then every page once);
  a receiver gone after sheet 1's front (simplex: sheet 2 taken, its `Sheet` failing, its page never drawn, nothing
  after; duplex: the back drawn, its `send` failing, nothing after); a hang midway through sheet 2's front on a
  thread of its own (the events before it taken on a rendezvous channel, half the page's rows counted, nothing more
  300 ms after a cancel, the thread unfinished, `fed()` and `in_feeder()` answering; the thread left parked); a crash
  and an exit, each in a child process through `rusty_fork::fork`, a duplex batch whose sheet 2 faults at its
  front's start, its events written whole and flushed as each is taken on a rendezvous channel: the parent reads
  sheet 1's events and perhaps sheet 2's `Sheet`, nothing else, and the child's end (`SIGABRT` on Unix, the code
  `0xC0000409` on Windows, behind `cfg`; the code 42 for `Exit`), as `sane-dl`'s abort test reads its child's
  (decision 41). Proposal 22's warm-up and a tracker cancelled before the batch are step 2's tests (decision 34),
  the warm-up met here too. scan-device's tests: 93 before, 112 now; 358 in the workspace expected, none lost.
- **Quick checks, the implementation's:** `cargo fmt --all -- --check`, `cargo clippy --locked --workspace
  --all-targets -D warnings` with and without `--all-features`, and `cargo test --locked -p scan-device` green;
  `cargo doc -p scan-device --no-deps` with and without `--all-features`, `-D warnings`, clean; `cargo clippy
  --locked -p scan-device --all-targets --all-features -- -D warnings` for `x86_64-pc-windows-gnu` and
  `x86_64-apple-darwin`, clean (ahead of the check's own run, since the `cfg` is new); `cargo +1.89 check --locked
  -p scan-device --all-targets`. The fault tests run in a short loop under the memory scope, 8 at a time: 16 runs
  found the exit test's flake (decision 40), then 48 runs of the two child tests and 40 of the whole binary
  passed; the 50-run loop of the timing tests is the check's. Make-fails on a worktree of its own
  (`scratch/work/s0-3/step-3/make-fail.md`, the script `mutate.py` beside it): 27 mutations, each failing a test:
  the plan's four (the check between front and back: both cancel tests, after the rendezvous test's second case;
  the next sheet taken before the last send returns, as a read-ahead: 14 tests, the held channel among them; a
  failed `send` ignored, a page's and a sheet's: the receiver-gone test; a jammed sheet dropped: 9 tests), and a
  fault left on its sheet, faults fired last first or on either side, midway at the first row or after an encoded
  page's bytes, a start's fault fired midway, `FeederEmpty` scriptable, the glass's sheet not written back or
  written over one put there meanwhile, its loads not counted, `Gone` not marking the device, a gone device listed,
  opened, or answering `caps`, `scan` or `settings_window`, a hang that returns or holds the paper, a crash that
  returns or exits with a code, an exit with another code or by an abort; the worktree removed.
- **Departures:** the rendezvous cancel test's second case (decision 40) and its whole-line output, which the plan
  didn't name; a device gone answering `Gone` from every backend that holds it, where the plan said every later
  call; 27 make-fails where the plan named four. No other: names as the design has them.
- **Decisions:** 35 to 41 ("Taken in the steps").

**Step 4, the plan's files; the slice's end** (2026-11-09; one commit, "Record S0 slice 3 in the plan's files", on
`b798a34`; docs only, `docs/pm/` alone). The checks before it: step 3's check passed on `b798a34`
(`scratch/work/s0-3/flow/checks.md`): `just ci` green with SANE required, run after `cargo clean -p scan-device` so
clippy ran fresh, `scan-device-msrv` on 1.89 with `rusty-fork` built; 358 tests, 19 added, all in `fake_faults`,
none lost; clippy with `--all-targets --all-features -D warnings` clean for `x86_64-pc-windows-gnu` and
`x86_64-apple-darwin`; `cargo doc` without the feature clean; the three binaries with timing tests run 50 times
each, 8 at a time, in the memory scope, 150 runs, none failing (the crash test takes about 1.1 s here, up to 7.45 s
with 8 at once, since each abort goes to the system's crash handler one at a time: this machine's, far under the
test's 60 s); the plan's four batch mutations each failing tests (2, 14, 1 and 1, and 9), and one of the check's
own (a jammed sheet put back at the feeder's tail: one test, the only one that tells head from tail); the MSRV
over the graph `rusty-fork` joins, 25 packages, highest 1.85, so 1.89 stands, and `cargo tree -e normal,build
--all-features` the same 8 packages. It found the unit tests no longer building for `wasm32-wasip1` since
`rusty-fork` joined (its `wait-timeout`, `E0433`), as decision 5 foresaw: filed below.

- **`crates/scan-device.md` as built:** an "As built" head (0.0.0, `publish = false`, the model and `fake`, MSRV
  1.89, `#![forbid(unsafe_code)]` and `#![warn(missing_docs)]`); what the crate adds to the note's block (the six
  constructors and their defaults, `RawImage::new`'s refusals in order and `data_len`, the platform's limit a
  parameter, `Debug` by hand for `RawImage`, `Encoded` and `Opened`, `pub use in_progress`, the modules); the
  `fake` backend as built (the feature and the self dev-dependency, the scripts' items, the default caps and the
  mistakes that panic, listing and opening, a batch and its clamp rules, the faults); the boundary as the gates
  now read it (every feature on; 8 packages); the dependencies (`in-progress`'s caret and re-export, `thiserror`,
  each backend's binding by slice, the tests' two); the MSRV and how it was measured; its 112 tests; "Open" with
  printscan's check for this slice and Q-5. "Stability" unchanged.
- **The rest:** `crates/README.md`'s `scan-device` line (built, 0.0.0, not published); `plan.md`'s family row and
  its graph (`in-progress` under `scan-device`, the arrow to it as `pdf-scan`'s to what it takes; the fixtures'
  lines a line lower for it); the note's "The model, for slice 3" headed by one sentence (built as written, the
  additions in `crates/scan-device.md`), off every TWAIN line; the milestone's slice 3 entry (done on the branch,
  waiting on review, 41 decisions and 5 security items, the code review next); `decisions.md`'s MSRV bullet right
  after `sane-dl`'s; `status.md`'s line for the slice, after slice 2's, and its "Waiting on review" (slice 3's
  counts) and "Next" (slice 3's code review) lines kept true; `slices/README.md`'s line, after slice 2's.
- **The backlog,** read again from `origin/master` as the step started (it had moved to `8276ad1`, pull request
  #5, closing slice 8, which touched no backlog file: the next free IDs still TEST-12, FEAT-13, BUG-8, DOC-2, Q-5
  and UP-14; slice 1a's branch takes none so far): **TEST-12** (`RawImage`'s layout tests on a 32-bit `usize`
  again, in CI: the `twain-dsm` job's `i686-pc-windows-msvc`, where `rusty-fork` builds, or slice 9's 32-bit
  worker job) and **Q-5** (the packaged crate's tests without the fake: packaging drops the path-only
  dev-dependency on itself, so from a `cargo package` of the crate the three fake test files don't build and the
  crate docs' example fails as a doctest, while the layout's tests pass; a probe of `cargo package --no-verify` and
  of the unpacked crate's tests, in a target directory under `scratch/work/s0-3/step-4/package-probe/`, removed
  after; for slice 11). Each in the README's format, counted into its table again with `rg` (S0 17, all 47).
- **Checks, the implementation's:** the diff is `docs/pm/` alone, so `just ci` isn't run (no build, test or gate
  reads `docs/pm/`; step 3's run on the same code stands); `cargo fmt --all -- --check` and `cargo clippy --locked
  --workspace --all-targets --all-features -- -D warnings` clean all the same. Neither new ID is on `origin/master`
  (`git grep`, at `8276ad1`, finds each only as the next free one). `scan-device`'s publish dry run not run:
  `publish = false` until slice 11 (the probe above packages without publishing or verifying). The branch's five
  commit messages read for the maintainer's name, local paths, `scratch/`, the downstream app and pdf-lint's plan:
  none. `git grep -i` for the maintainer's name: the root `LICENSE-MIT` alone. Every path the edited lines cite
  found. A trial merge of the step's tree with `origin/master` (`git merge-tree`) conflicts in the root
  `README.md` alone, step 1's `scan-device` row beside the `twain-dsm` row #5 rewrote; every `docs/pm/` file merges.
- **What only CI shows:** the fake's tests on Windows and macOS (the crash child's exit there, `0xC0000409` and
  `SIGABRT`; the exit's code 42; the 10 s and 300 ms timing tests and the crate docs' example on slower runners),
  clippy on `x86_64-pc-windows-msvc` and `aarch64-apple-darwin` (checked here as `windows-gnu` and
  `x86_64-apple-darwin`), `scan-device-msrv` as CI's job on 1.89, and the gates on `ubuntu-latest`'s SANE.
- **Departures:** `status.md`'s "Waiting on review" and "Next" lines changed beside its new line, so neither says
  what's no longer so; its head paragraph, slice 8's entries and the merged state of slices 1 and 2 left as they
  are (`master`'s lines; the fix stage's bookkeeping). The 32-bit run's end filed as TEST-12 and said in
  `crates/scan-device.md` both, where the step's check offered either.
- **Decisions:** none new for review; the counts stay 41 and 5.

## Code review

2026-11-09, on `3f33be5..97fa72c`, by three reviewers in parallel through three lenses (behaviour, adversarial and
correctness; the crate as a published library; the repository's hygiene), their probes under
`scratch/work/s0-3/review-behaviour/`, `review-api/` and `review-hygiene/`. The checks before it: step 4's check
passed on `97fa72c` (`scratch/work/s0-3/flow/checks.md`): docs only, so `just ci` left out with step 3's run
standing; 358 tests, none lost or added; `just gates` green (`scan-device` 8 packages); TEST-12 and Q-5 free on
`origin/master` (`8276ad1`); the backlog in its format and its table exact; `crates/scan-device.md` as built
matching the code, and Q-5 reproduced; the commit messages and `git grep` clean of the maintainer's name and
private paths; a trial merge with `origin/master` conflicting in the root README alone.

18 findings, 1 major and 17 minor, three of them found by two lenses: 15 distinct, each verified against the code
first (the behavioural ones reproduced: below), **all fixed**, none refuted, none deferred; one part of one,
slice 11's freeze, filed as Q-6. Fixed in `eac0b5b` (`RawImage`'s limit and `pattern`), `5674194` (the traits'
and the model's docs, what may grow, the fake's builders), `5a5c367` (the recipes and the gates), and the last
commit (this bookkeeping). Decisions 42 to 50 are the fixes' (above); "For review (security)" gains item 6.

**Behaviour:**

1. *Major: `RawImage`'s platform limit was `usize::MAX`, but no buffer passes `isize::MAX`.* Holds: under
   Node's WASI (the reviewer's `datalen32-wasi.txt`) `data_len(Gray8, 65535, 65537)` answered `Ok(4294967295)`
   and allocating it panicked ("capacity overflow"), `pattern` the same; on 64 bits, `Gray8` at `u32::MAX`
   square (about 1.8e19 bytes) passed past 2^63 − 1. **Fixed** (`eac0b5b`): the limit `isize::MAX`; the unit
   tests' 32-bit limit `i32::MAX` (2^31 − 1 bytes held, 2^31, 2^32 − 1 and 2.4e9 refused there), a 64-bit one
   beside it (`Gray8` at `u32::MAX` square `TooLarge` everywhere, `Bw1`'s 2.3e18 bytes still a `Length` on 64
   bits), and one pinning the platform's; `pattern` refusing what `data_len` refuses (a unit test, its 32-bit
   sizes run on 32 bits only) and its first row made at its stride, where it had asked for 8 bytes more (which
   only a row within 8 bytes of a 32-bit `isize::MAX` would meet: not tested, a 2 GiB row); `TooLarge`'s,
   `new`'s and `data_len`'s docs, the crate docs, the README, the seam note's section 3 and
   `crates/scan-device.md`. Security item 6.
2. *Minor (twice, with the API lens's 9): `Backend::open`'s docs put `BadId` "before anything else", and the
   fake answers `NotInstalled` first; `list`'s read as waiting the deadline out.* Holds: `FakeBackend::new()
   .not_installed().open(<a sane: id>)` gives `NotInstalled`, and `a_backend_not_installed_says_so_before_anything_else`
   pins it. **Fixed** (`5674194`) in the trait's docs, the fake as it is (decision 43): `BadId` before the
   backend lists or opens anything, `NotInstalled` possibly first; `list` the devices found within the deadline,
   or `TimedOut` where the listing isn't done by then.
3. *Minor: `Device::scan`'s "no next sheet starts before the pages are taken".* Holds: a `send` on
   `sync_channel(n)` returns once the event is buffered (the reviewer's three sheets through a bound of 16,
   nobody reading, ending `FeederEmpty` with 7 events untaken; the fake module's own doctest does it through 8).
   **Fixed** (`5674194`): the bound as it works (at most `n` events untaken, the backend then held at its next
   `send`; each event waited on only through `sync_channel(0)`), in `Device::scan` and the crate docs, and the
   seam note's two sentences (section 3, and "The model, for slice 3") the same.
4. *Minor (twice, with the API lens's 11): `pattern`'s docs said each row is the one before turned, false in
   `Bw1` at a width not a multiple of 8.* Holds: at 13 pixels its pinned rows are `a9 c8`, then `cc a8`, where
   the first turned is `c8 a9`. **Fixed** (`eac0b5b`) in the docs, the bytes unchanged (decision 44): each row
   the first row's bytes turned by its number in pixels (bytes in `Bw1`), then its padding cleared; a unit test
   holds every row to that at 13, 9 and 40 pixels and in two other formats, and shows a 13-pixel row isn't the
   one before turned. Decision 32 reads so now.

**API:**

5. *Minor: invariants stated and enforced nowhere* (a source's formats "never empty", `Caps::sources` too, read
   as `[0]` by the fake only because its scripts are checked; `RawPage`'s `dpi` left to "the worker's client").
   Holds: `SourceCaps::new` takes an empty `Vec`. **Fixed** (`5674194`): `Caps`' docs give the rules a backend
   keeps, which the constructors don't check and code decoding caps from outside the process checks;
   `Caps::sources` "never empty"; `RawPage`'s `dpi` said the same way, naming no party; the seam note's block
   (`RawPage`'s doc) and its "What slice 4 needs of it" (an empty `Caps::sources` and a `SourceCaps` with no
   format among what the client refuses), and `crates/scan-device.md`.
6. *Minor: a cancel, then a wait for the batch with the receiver kept and not read, never returns.* Holds: the
   check comes before a sheet is taken, and a `send` on a full channel waits (the reviewer's bound of 1 still
   running 3 s after the cancel, then `Ok(Cancelled)` once the receiver was dropped). **Fixed** (`5674194`): the
   crate docs' "Cancel and progress" and `Device::scan` say to keep taking events until `scan` returns, or drop
   the receiver; the behaviour kept (decision 47).
7. *Minor (twice, with the hygiene lens's 13): the build an app ships, the fake off, was never linted or
   documented under `-D warnings`; and the self dev-dependency hides a member's missing `fake` under `cargo
   test --workspace`.* Holds: the reviewer's link to `fake` passed `just doc` and failed a plain `cargo doc`;
   their two-crate workspace, run again here, passes `cargo test --workspace` and fails `cargo test -p b`
   (`E0433`). **Fixed** (`5a5c367`, decision 45): `just clippy` also lints each library with its default
   features, and `just doc` documents with them before every feature, on CI's three systems; a link to `fake`
   now fails `just doc`, and a helper only the fake uses fails `just clippy`'s new line while its first passes
   (make-fails, below). Slice 4 is told in the seam note's "What slice 4 needs of it": `scan-device` with
   `features = ["fake"]` in its own dev-dependencies, and its tests run with `-p` too.
8. *Minor: `Done::Cancelled` promised the sheet in flight finished, which a receiver gone doesn't.* Holds:
   `a_receiver_gone_mid_batch_ends_it_and_no_sheet_is_taken_after` has sheet 2 taken in simplex and never
   scanned. **Fixed** (`5674194`): its two causes said apart; the behaviour kept (decision 47).
9. *Minor: `Paper` exhaustive while paper may grow, and the fake's `When` beside a non-exhaustive `Fault`.*
   Holds. **Fixed** (`5674194`, decision 42): both `#[non_exhaustive]`, the note's block written so; the rest
   for slice 11, Q-6.
10. *Minor: the fake's twelve builder methods weren't `#[must_use]`,* so `scanner.clone().refuses_open(..);`
    scripted nothing without a word. Holds (clippy's `return_self_not_must_use` named each). **Fixed**
    (`5674194`): each marked, with a message; that lint passes on the crate now.

**Hygiene:**

11. *Minor: the libsane gate built its binaries with default features alone.* Holds: the reviewer's feature
    `sane` whose build script links libsane passed `just gates`. **Fixed** (`5a5c367`, decision 46): both builds
    take `--all-features`, the comments say so, and the same make-fail now fails the gate, `scan-device`'s five
    binaries named.
12. *Minor: CI's `scan-device-msrv` comment, "no platform code", stale since step 3's crash test.* Holds.
    **Fixed** (`5a5c367`): the comment, and decision 21, say its Windows branch is left uncompiled on 1.89,
    accepted (no 1.89 Windows target here either).
13. *Minor: the comment over `clippy` gave `doc`'s reason too.* Holds. **Fixed** (`5a5c367`): each recipe its own.
14. *Minor: "its name its maker and model" in the fake's listing docs.* Holds. **Fixed** (`5674194`): "its name,
    its maker and model joined by a space; its maker; its model", which says what the name is.
15. *Minor: M5's proposals 9, 10 and 22 named in public files with no link.* Holds. **Fixed** (the bookkeeping):
    the first mention of each in this plan and in `crates/scan-device.md` linked at sheaf's `3232a35` (each file
    found there); the bare references elsewhere follow `README.md`'s "Citations", left as they are.

**Checks after the fixes,** on `5a5c367` (`scratch/work/s0-3/fix/`): `just ci` green with SANE required (fmt;
clippy every feature and each library alone; test; doc with default features and with every one; the gates,
`scan-device` 8 packages and 25 binaries read for libsane, built with every feature; `sane-dl-package`, 16 files;
`sane-dl-msrv`, `twain-dsm-msrv` and `scan-device-msrv` on 1.89). 361 tests, 3 added (two of `pattern`'s, one of
the limit), none lost (`tests-fix.txt` against step 4's 358; four doctests' line numbers moved). The layout's and
the fake's unit tests built for `wasm32-wasip1` from a copy of the crate without `rusty-fork` (whose
`wait-timeout` doesn't build there) and run under Node's WASI: 25 passed, the 32-bit branches taken. Make-fails on
a worktree (removed after; `make-fail.md`): the limit back to `usize::MAX` fails 3 tests here and aborts the WASI
run (capacity overflow); a link to `fake` fails `just doc` at its default-feature line; a helper only the fake
uses fails `just clippy`'s new line, the first passing; a feature whose build script links libsane fails `just
gates`. `cargo clippy -p scan-device` for `x86_64-pc-windows-gnu` and `x86_64-apple-darwin`, the library alone and
every target with every feature, clean. Left out, the fixes not touching their subject: the MSRV probe and `cargo
tree` (no manifest or lock changed; `just ci`'s MSRV recipes ran), the timing loops and the batch's mutations
(the batch's code unchanged), the fallback's mutations (opening unchanged), the publish dry run (`publish =
false`).

**What only CI shows** (TEST-13): the fake's tests on Windows and macOS (the crash child's exit, `0xC0000409` and
`SIGABRT`; the exit's 42; the timing tests on slower runners); `just clippy`'s and `just doc`'s new lines on
Windows and macOS (checked here for both targets by clippy, not by rustdoc); `scan-device-msrv` as CI's job; the
gates on `ubuntu-latest`'s SANE with every feature built; and the layout tests on a 32-bit `usize` in CI (TEST-12).

**The bookkeeping** (the last commit): the milestone's slice 3 done on the branch, and slices 1 and 2 merged;
`status.md`; `slices/README.md`; slices 1 and 2's plans moved to `archive/slices/` and the references to them
updated but `crates/sane-dl.md`'s (decision 49); `plan.md`'s head and its MSRV rule; the backlog's TEST-13 and Q-6,
TEST-12 brought up to date, the README's table counted again; and the privacy scan of the branch's files and
commit messages.
