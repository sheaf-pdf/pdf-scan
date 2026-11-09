# S0 slice 2: the seam note revised

Status: running, on branch `s0-2-seam-note`, from 2026-11-09; its three steps done, its code review next.
Milestone: `milestones/s0-devices.md`, slice 2.

Paths: `docs/pm/` files from `docs/pm/`, as everywhere here; the code's from the repository's root, written
with a leading `/` (`/crates/sane-dl/`, `/justfile`); `scratch/` from the root, local and never tracked.

## What it builds

A documentation slice. `crates/scan-device-seam.md` revised as the milestone's slice 2 has it: what slice 1
showed of SANE folded into the SANE rows, the seam's two SANE unknowns answered (names that change on
replugging; stopping after the page in flight), the raw layout fixed as one stated layout (its polarity, its
bit order within a byte, its 16-bit byte order, its row padding, its channel order), and the unknowns only
TWAIN can settle kept as slice 7 left them. Then what follows from it: the model's types written down for
slice 3 to build (the milestone: "reviewed before slice 3 starts, since it fixes slice 3's API"),
`crates/scan-device.md`, `crates/sane-dl.md`'s "For the seam", the milestone's slices 3 to 5 where their API or
their checks change, and the backlog.

No crate's code changes, unless a probe shows a defect in `sane-dl` the seam can't do without; then the fix is
small, tested and said here, and anything bigger is a backlog item for slice 5. The probes (a scratch crate
against `sane-dl`, SANE's `test` and `pnm` backends, `fake-libsane`, `scanimage`) live in
`scratch/work/s0-2/probes/`, are never committed, and are summarized in "So far".

Nothing is cut from sheaf, so there is no harness comparison; the sheaf plan files the note cites were checked
for drift as the slice starts (sheaf still at `3232a35`, 2026-11-09). Nothing here touches the PDF family.

**Later slices build on it** (`milestones/s0-devices.md`): slice 3 builds the model the note now fixes (its
types, the `Backend` and `Device` traits, the raw layout's constructor, cancel and progress, the `fake`
backend's scripts); slice 4 carries those types over the worker's pipe (plain data, a page's bytes after its
header, sizes checked before they're read); slice 5's `sane` backend makes the conversions, names, fallback,
cancel and status mapping the SANE rows say; slice 6's hand check answers what only an fi-6130 can (TEST-9,
below); slice 9's `twain` backend meets the same model, and makes the changes it asks for; slice 11 freezes it.

## Taken as it starts

- **No ask bears on it:** `asks.md` holds ask 5 alone (where `pdf-images` lives), which is S2's.
- **The `in-progress` proposal** (the seam note's section 7, "for review; asked 2026-11-08 whether
  `in-progress` fits") is taken as the note proposed it, since slice 3's API needs an answer, and stays open to
  review (decision 10).
- **Where it starts:** S0 slice 1 is done on `s0-1-sane-dl` (pull request #2), whose tip `4624760` merges
  `master` with slice 7. This branch is made from that tip, and its pull request is stacked on slice 1's; the
  main session pushes and opens it. If CI's Windows reruns on pull request #2 show a fault in slice 1, the main
  session fixes it on `s0-1-sane-dl` and merges it in here; the step after that merge runs the code checks again
  (Steps).
- **Slice 8 runs meanwhile on the Windows machine** (`origin/s0-8-twain-dsm`, at `0f08bb0` when this plan was
  reviewed) and changes the same note: a status paragraph after slice 7's, the path in slice 7's own paragraph
  (its plan moved to `archive/slices/`), section 3's "Decided by the TWAIN spike" paragraph, section 8's TWAIN
  lines, and the unknowns table's events row; also `status.md`, `decisions.md`, `backlog/tests.md`,
  `slices/README.md` and the milestone's slices 7 and 8. This slice keeps off those lines (below, "Where the
  note changes"), and a trial merge at each step shows it (Steps).

## The design

### What the SANE rows say, section by section

Each fact is cited where the note gives it: slice 1's plan (`slices/s0-1-sane-dl.md`, its "So far" steps 2 and
4, "Code review", decisions), `sane-dl`'s code and README, `sane.h` 1.4.0, the standard
(sane-project.gitlab.io/standard: the API, "Image Data Format", "Code Flow"), the man pages (sane-dll(5),
sane-test(5), sane-pnm(5), sane-usb(5), and the backends' own), or this slice's probes. sane-backends' code
(GPL) is studied and cited by its repository path at the commit read (master `a113b257`, the copies slice 1's
code review read), never quoted at length or copied; libinsane, simple-scan and NAPS2 the same.

- **1, finding devices:** the listing as `sane-dl` gives it (each `Device`'s name as SANE's bytes; vendor,
  model and kind as text), about 9 s here through the system's configuration with nothing attached, 0.001 s
  through a one-backend `ConfigDir` (slice 1, step 2); a listing and a scan in one process wait on each other,
  so the listing has a worker of its own; opening by name needs no listing, and SANE's `dll` loads the backend a
  name starts with whatever `dll.conf` says (slice 1's security item 7, which the fallback below keeps). The
  names, and what replugging does to them, are the next section's.
- **2, settings** (slice 1's step 3, "What it learned of SANE", for slice 2 and slice 5): an option's type read
  from its descriptor, never assumed (the `test` backend's `resolution` is `FIXED`, `pnm`'s an `INT` word list
  that can be automatic); the `test` backend's `source` is `Flatbed` or `Automatic Document Feeder`, and `mode`
  set to `Color` says `RELOAD_OPTIONS`; `int-inexact` moves the value it's given and says `INEXACT`; every
  re-read of the descriptors, whatever its cause (a set's `RELOAD_OPTIONS`, `INVAL` from a start), counted by
  `Handle::options_generation`, a set whose re-read fails `ReloadAfterSet`.
- **3, a page:** what `sane-dl` hands over: `Batch::next_page` gives a page whole (`Image`), its frames read to
  `EOF` (`lines = -1` included), padding dropped, three-pass frames interleaved into RGB, each frame's
  parameters kept; the samples as SANE encodes them (below, "The raw layout"); progress by bytes
  (`Progress { bytes_read, bytes_expected }`). Reads are blocking: several of SANE 1.4.0's backends answer
  `UNSUPPORTED` to `sane_set_io_mode` either way (UP-11; slice 1's decision 57), so the select fd is the
  step-by-step API's, and no backend of the model relies on it. The row's "Blocking, or non-blocking with a
  select fd" goes.
- **5, a batch's end:** `NO_DOCS` from `sane_start` or from a page's first read, said by `Page::NoDocs { from,
  pages }`; `NO_DOCS` after a page's first byte is an error, the page dropped; the `test` backend's feeder counts
  every start since the device opened, a flatbed's too (slice 1, step 4).
- **6, errors:** every status with the call that answered it (`Error::Status { status, call }`); `INVAL` from
  `sane_open` ("the device name is not valid", the standard) apart from `INVAL` from `sane_start`
  (`StartInvalid`, descriptors read again); `DEVICE_BUSY` at open (`pnm` opens one device at a time); the
  reserved statuses 12 and 13 kept as integers, never returned by 1.4.0; `ACCESS_DENIED` (no authorization
  callback, FEAT-11) and `NO_MEM` among the `Driver` ones; `LoadError::NotFound` (no libsane) its own case
  (below, `ScanError::NotInstalled`).
- **7, cancel:** the token (`CancelToken`) calls `sane_cancel` at once from any thread, synchronized with close;
  a cancel wins over what SANE answers, so the page in flight is dropped (slice 1's decision 41). That isn't
  the model's cancel (the next section).
- **8, threads:** one call at a time under `sane-dl`'s lock, a handle on one thread, the token from any; an
  empty read paced, without a cap, so a lamp warming up is waited on (slice 1's decision 58) and a read that
  never returns is the watchdog's.

### The SANE unknowns, answered

**Names that change on replugging.** Nothing can be replugged here (no scanner, no USB), so the answer is drawn
from the standard, the man pages, the backends' code studied, and the names the `test` backend and the fake
give; what only the fi-6130 can show is said, and is TEST-9's.

- *The standard:* a name is the backend's to form, unique among its devices, a "legal SANE text string"
  (Latin-1); `sane_open` takes it as listed. Through `dll` it is `<backend>:<the backend's name>`.
- *Seen here:* `test:0`, `test:1`, `pnm:0`, `pnm:1`: the backend, then an index it gives in order, which says
  nothing of which device it is (slice 1, step 2). The fake's names are whatever its script says, and `sane-dl`
  hands them over byte for byte (probe N1). **A name no longer listed answers `INVAL` from `sane_open`,** from the
  installed backends themselves with nothing attached, not from the fake (whose `INVAL` for an unlisted name is
  its own rule, so no evidence): while reviewing the plan, `scanimage -d` with an empty private configuration
  opened `fujitsu:fi-6130dj:12345`, `libusb:001:004` under `fujitsu`, `canon_dr`, `epson2`, `avision`, `kodak`
  and `xerox_mfp`, `pixma:04A91234_ABCDE`, `test:9`, `pnm:2` and `nosuch:0`, each answering "Invalid
  argument" in 0.02 to 0.55 s (`scratch/work/s0-2/review/stale-open.txt`; `dll`'s log shows `fujitsu` and `epson2`
  loaded by their prefix, `stale-open-dll.txt`). Probe N1 opens the same names through `sane-dl`, which reports
  `Status { INVAL, Open }`. So at open `INVAL` covers a device gone and a backend not installed (`nosuch:0`)
  alike, and the fallback below answers both.
- *Studied (expected, to be checked line by line in step 1, a row per backend in the note):* `fujitsu` and
  `canon_dr` name a scanner `<model>:<serial>` when it answers a serial query, else by its connection
  (`libusb:<bus>:<device>`), and open by either (`fujitsu.c`'s `init_serial` and `sane_open`); most others
  (`kodak`, `epson2`, `epsonds`, `avision`, `plustek`, `xerox_mfp`, `hp5590`, `kodakaio`) by the connection alone,
  a USB bus and device number (sane-usb(5) names the device files so), the device number the system's, given again
  each time the scanner is plugged in; `pixma` by vendor and product ids and serial (sane-pixma(5)); `net` by host
  and the remote name (sane-net(5)); `airscan` and `escl` by network identities. The step words only what it
  finds.
- *So the model:* a SANE id is `sane:` and the name's bytes read as Latin-1, every byte one character, so it goes
  back to `sane_open` exactly (decision 11). Opening an id that answers `INVAL` lists that id's backend alone,
  under the deadline, and opens the one device of the same backend, vendor and model, if exactly one is listed,
  saying so (`Opened::renamed_from`); none or several is `Gone`, and the app shows the list (decision 12; security
  item 1). **Where that listing runs** (decision 18): SANE starts once a process and reads its configuration as it
  starts (`/crates/sane-dl/README.md`), and `ConfigDir` never sets `SANE_CONFIG_DIR` (`crates/sane-dl.md`: the
  caller gives it to the process it starts), so the fallback can't switch configuration in the worker that tried
  the open. The worker that opens a stored SANE id is started with a one-backend `ConfigDir` for the id's backend
  (`sane_dl::backend_of`): its open goes straight to the backend (`dll` loads by prefix), and its fallback listing
  is that backend's alone, fast (0.001 s through a one-backend directory, slice 1's step 2) and unable to cross
  backends. Who writes the directory and passes it at the spawn is slices 4's and 5's (the milestone's entries say
  so). The fi-6130 answers the rest by hand (slice 6, TEST-9): its name through `fujitsu` (serial or connection),
  what replugging does to it, and whether the fallback finds it.

**Stopping after the page in flight.** Settled on the `test` backend and the fake, through `sane-dl`, with the
probe's output in "So far" (probe S1); what real feeders do is slice 6's.

- *What the probe shows (expected from slice 1's step 4; the step records what it sees):* the backend reads the
  page to its `EOF`, starts no next page, and ends the batch (`sane_cancel`, at the batch's drop). The `test`
  backend takes a sheet only at a start, so the sheets after stay in its feeder: a batch ended after 3 pages
  leaves 7, which the next batch gives before `NO_DOCS`, after which it fills again. Step by step, `sane_cancel`
  after a page's `EOF` and then `sane_start` start the next sheet. The token, cancelling midway through the second
  page, drops that page and its sheet with it (the next batch gives 8). A token cancel that lands between pages
  stops the next start before SANE is called, so no sheet goes. The fake shows the same calls in its log, and its
  duplex script, after a batch ended on a front, starts the next batch with that sheet's back: the fake is our
  model of a duplex backend, not evidence of one, so this shows only that `sane-dl` lets a backend read the back
  before it ends (one more `next_page`).
- *What only hardware shows:* whether a feeder has pulled the next sheet in before the page in flight ends, and
  what a backend does at `sane_cancel` while its batch is started (`fujitsu.c`'s `check_for_cancel` sends the
  scanner a cancel or halt command then, studied): slice 6.
- *So the model:* `cancel` means feed no more and finish **the sheet** in flight: in duplex, a front already
  delivered is followed by its back (decision 8). The `sane` backend does it by checking between sheets and
  dropping the batch; `sane-dl`'s token isn't the model's cancel, since it loses the sheet in flight (decision
  9). A read that never returns is the watchdog's.

### The raw layout

One layout for every backend, which `RawImage` carries (decisions 2 to 6):

| | The model's `RawImage` | SANE gives (and `sane-dl` passes on unchanged) | TWAIN gives (slice 7's rows, as they are) | Which converts |
|---|---|---|---|---|
| Formats | `Bw1`, `Gray8`, `Gray16`, `Rgb8`, `Rgb16` | `GRAY` or `RGB` (or three single-channel frames, interleaved by `sane-dl`) at depth 1, 8 or 16 | Black and white, gray, RGB; 16 bits not seen (the sample offers 1, 8 and 24) | `sane`: RGB at depth 1 (byte-interleaved, a byte of eight pixels of one channel, the standard) expanded to `Rgb8`, each bit 0 or 255 |
| Polarity | 0 the least intensity at every format: in `Bw1`, 0 black and 1 white | `GRAY` at depth 1: 1 black, 0 white; every other: 0 the least intensity (the standard) | DIB: through its palette (the sample's 1-bit black then white, its gray an identity ramp); memory: `ICAP_PIXELFLAVOR` (`TWPF_CHOCOLATE`, 0 black, all the sample offers) | `sane`: 1-bit gray inverted; `twain`: the palette and flavor applied (`TWPF_VANILLA`, or a white-then-black palette, inverted) |
| Bits in a byte | The leftmost pixel in the most significant bit; bits past a row's last pixel 0 | The leftmost pixel in the most significant bit (the standard) | DIB: the leftmost pixel in the high bit (a DIB's rule); memory: equal to the DIB pixel for pixel on the sample, so the same, whatever `ICAP_BITORDER` reports (`TWBO_LSBFIRST`, read and never set: the sample's quirk reads it as a color order) | `sane`: padding bits cleared again after the inversion |
| 16-bit | Little-endian, whatever the host | The host's byte order (the standard) | Not seen: slice 8 reads what TWAIN says, slice 12 sees a driver's | `sane`: swapped on a big-endian host, nothing on CI's three systems |
| Rows | Top down, packed: each row exactly its pixels' bytes; no stride stored, `stride()` worked out from format and width | Top down; `bytes_per_line` may pad, `sane-dl` drops it | DIB bottom up, rows padded to four bytes; memory strips top down (the sample pads to four bytes), tiles by offset | `twain`: rows flipped (DIB), padding dropped, tiles placed |
| Channels | R, G, B per pixel, interleaved | R, G, B (one frame, or three interleaved by `sane-dl`) | DIB B, G, R; memory R, G, B on the sample | `twain`: the DIB's swapped |

WIA's and ICA's columns stay "to confirm" (slices 10 and later). The layout is slice 7's comparable form
(`twain-spike`'s `image` module, its module doc: rows top down, packed, RGB, gray and 1-bit with 0 black, the
first pixel in the high bit, the padding bits zero), with 16-bit little-endian added: so the TWAIN side's
conversion already exists, and SANE's 1-bit gray is the one format a backend inverts. The note names the
module by crate and module, not by path: slice 8's branch moves it into `twain-dsm`'s tests' support and moves
slice 7's plan into `archive/slices/`, and a path would break at that merge. A probe (L2) checks the claim
beside a third party: the model's conversion of `sane-dl`'s pages equals the raster of `scanimage
--format=png` for the same scan, pixel for pixel (PNG's 1-bit gray is 0 black, the high bit first; its 16-bit
big-endian, compared after a swap). Pixels, not bytes: `scanimage` inverts a 1-bit row's whole bytes for PNG,
padding included (`frontend/scanimage.c`, its PNG write, studied), so a 157-pixel `Solid white` row ends `0xFF`
in its PNG and `0x00` in its PBM (seen while reviewing the plan, `scratch/work/s0-2/review/`). Its 16-bit PNG
is big-endian as PNG requires: a gray-16 `Color pattern` page's PNG and PGM were byte-equal, values such as
`0x0001` and `0x0100` both present (the same review probe). Both conventions show
in one tool: a 1-bit `Solid black` page from `test:0`, scanned here while planning
(`scratch/work/s0-2/plan-probe/`), is all `0xFF` in `scanimage`'s PBM and in its TIFF (big-endian, photometric
0, white is zero: SANE's bits kept) and all `0x00` in its PNG (gray, depth 1: inverted).

`RawImage`'s fields are private: `RawImage::new(format, width, height, data)` checks that `width` and `height`
are at least 1, that `data` is `height` × the row's bytes exactly, and that a `Bw1` row's padding bits are 0
(decision 6), so every `RawImage` holds the layout wherever it's made, the worker's client included. The
length is worked out in `u64` with checked arithmetic, a size past what the platform can hold refused, never
wrapped: in a 32-bit worker `u32` × `u32` × 6 overflows `usize`, and at `u32::MAX` by `u32::MAX` it overflows
`u64` too, at two bytes a pixel or more (step 2: at `Bw1` and `Gray8` it fits, and the length refuses it)
(slice 7's `image` module works its sizes out in `u64` for the same reason).

### The model, as slice 3 builds it

Written into the note as a new section ("The model, for slice 3"), from proposal 9 with the note's seven changes
and this slice's. Plain data, public fields but `RawImage`'s; enums that may grow `#[non_exhaustive]`; pixel
sizes, resolutions and counts `u32`, the same in a 32-bit worker as in a 64-bit app (slice 9); paper sizes `f32`
millimetres (decision 17). Names are the design's; slice 3 may rename and says so.

- **Devices:** `DeviceId` (a string, `<driver>:<native id>`: `sane:<name as Latin-1>`, `twain32:<product name>`,
  `twain64:<product name>`, `wia:<device id>`, `ica:<persistent id>`, `fake:<name>`; opaque, compared whole);
  `DeviceInfo { id, name, maker, model, driver }`; `Driver { Sane, Twain { bits }, Wia, Ica, Fake }`.
- **Capabilities, per source** (the note's change 1): `Caps { sources: Vec<SourceCaps>, settings_window: bool }`;
  `SourceCaps { source: Source, duplex: bool, resolutions: Resolutions, formats: Vec<Format>, max_area: SizeMm,
  paper_auto: bool }`; `Source { Flatbed, Feeder }`; `Resolutions { List(Vec<u32>), Range { min, max, step } }`
  (SANE and TWAIN give either: the note's "To settle" answered by keeping both); `SizeMm { width: f32, height:
  f32 }`, millimetres, as SANE gives its area (Letter's 215.9 isn't a whole millimetre).
- **Settings:** `Settings { source, duplex, dpi, format: Format, paper: Paper }`; `Paper { Auto, Size(SizeMm) }`
  (Letter, Legal and A4 as `SizeMm` constants). The raw layout's `Format` is also what's asked for, in place of
  proposal 9's `Mode`, so 16-bit can be asked for and clamped like the rest (decision 7). Clamping is said by
  `Applied { settings, clamped: Vec<Clamped> }`, the settings as the device took them and each change.
- **Pages:** `RawPage { pixels: Pixels, dpi: (f32, f32), sheet: u32, side: Side, side_known: SideKnown }`;
  `Pixels { Raw(RawImage), Encoded(Encoded) }`; `Encoded { kind: EncodedKind, bytes }`, `EncodedKind { Jpeg,
  Png, Tiff, Bmp, CcittG4 { width, height, black_is_1 } }` (SANE gives none: its JPEG and CCITT frames are
  reserved); `Side { Front, Back }`, `SideKnown { Reported, Inferred }`.
- **Events and outcomes:** `Event { Applied(Applied), Sheet(u32), Page(RawPage), Status(Status) }` on a
  `std::sync::mpsc::Sender<Event>`, which a push backend's own thread can hold a clone of; `Status` a typed note
  for the user (`WarmingUp` first); `Done { FeederEmpty, OnePage, Cancelled }`;
  `ScanError { FeederEmpty, Jammed, DoubleFeed, CoverOpen, Busy, Gone, BadId, Stopped, NotInstalled { what },
  Driver { message, shown } }`: `FeederEmpty` only before any page, `Stopped` the watchdog's alone,
  `NotInstalled` when the system's driver API isn't there (no libsane, no `TWAINDSM.dll`: proposal 6's
  "Scanning on Linux needs SANE"), added here, and `BadId` for a stored id the driver doesn't form (decision
  25, added by step 2's repair).
- **Traits** (decision 13): `Backend: Send` with `list(&mut self, deadline) -> Result<Vec<DeviceInfo>,
  ScanError>` and `open(&mut self, &DeviceId) -> Result<Opened, ScanError>`; `Opened { device: Box<dyn Device>,
  info, renamed_from: Option<DeviceId> }`; `Device: Send` with `caps(&mut self)`, `scan(&mut self, &Settings,
  first_sheet: u32, &Sender<Event>, &dyn in_progress::Progress) -> Result<Done, ScanError>` (blocking its own
  thread until the batch ends; `first_sheet` carries the count on through "Scan More"), and
  `settings_window(&mut self)` where `Caps::settings_window` says there is one. Two levels, as SANE's library
  and handle are and as a worker keeps one device open while a dialog is.
- **Cancel and progress** (decision 10): through `in-progress`'s `Progress`, which `scan` takes. A backend calls
  `check()` between sheets, never within one: that is "finish the sheet in flight"; it reports each page's
  bytes (`set_total(expected, Unit::Bytes)`, `set(0)` as the page begins, `inc` or `set` as it's read), which
  replaces the note's separate progress event. The caller cancels through its `Tracker` (a cancelled tracker
  stays cancelled, so each batch has its own); `scan-worker` polls the worker's tracker or implements `Progress`
  itself to push lines (slice 4's choice). Slice 7's sentence stays: a flag can't wake a thread waiting in
  `GetMessageW`, a posted message can. Since `in-progress` calls nothing when cancelled, the `twain` backend
  either looks at the flag wherever its loop wakes (a source's message, a request, a timer) or slice 9 adds a
  wake-up to the model, as the model's changes are slice 9's to make: said in the note as a change to the
  paragraph both share. A push backend's own thread reports through `Progress::child`, which returns an owned
  `Box<dyn Progress>` (`Send`, `'static`, cancelled with its parent: `in-progress`'s docs), moved into that
  thread for the batch with a clone of the `Sender` (a review sketch built and ran that shape,
  `scratch/work/s0-2/review/sketch/`).
- **What slice 4 needs of it, said in the note:** every type plain data the worker can encode (a `serde`
  feature here, or wire types of the worker's own: slice 4's choice); a page's format, width and height read
  before its bytes, and `RawImage::new` refusing a length that doesn't match, so a crashed worker's half page is
  never a page; a page delivered whole, once complete, never cut short. And the protocol's consequences, which
  `crates/scan-worker.md`'s list (`list`, `caps`, `scan`, `cancel`, `quit`; `device`, `caps`, `sheet`, `page`,
  `status`, `error`, `done`) predates: an `open` request, its answer carrying `renamed_from`, before `caps` and
  `scan` (the two traits); an `applied` event (`Event::Applied`); progress lines from the worker's `Progress`;
  `cancel` cancelling the batch's tracker; and a SANE worker started with the stored id's one-backend
  `ConfigDir` (decision 18).

### What follows elsewhere

- **`crates/scan-device.md`:** `RawPage` as the raw layout has it; `Caps` per source; `Settings` with `Format`;
  the two traits; `in-progress` among its dependencies (taken by slice 3); the `fake` backend's scripts as
  slice 3's entry below has them ("a cancel mid-page" now the sheet finished); the `sane` backend's bullet
  gaining what the note gives it (ids from Latin-1, the fallback in a one-backend worker, the conversions of
  the raw layout); "The seam" saying slice 2 revised the note.
- **`crates/sane-dl.md`, "For the seam":** what the seam now takes from it, as built: `DeviceName`'s bytes for
  the id; `Image` converted by the `sane` backend (1-bit gray inverted, 1-bit RGB expanded, 16-bit to
  little-endian); "feed no more" by starting no next page and dropping the batch, the token kept for a stop at
  once; its sentence pointing slice 2 at slice 1's plan replaced by one pointing at the note.
- **The milestone's slices 3 to 5** (`milestones/s0-devices.md`): slice 3, the model as the note's new section
  has it, `in-progress` taken (vetted below; the devices layer's gate holding, and `plan.md`'s graph updated
  then), `RawImage::new`'s checks unit-tested, and the `fake` backend's scripts gaining a cancel after a duplex
  front (its back still delivered) and a renamed device; slice 4, the protocol as the note's "What slice 4
  needs" has it (`open` and `renamed_from`, `applied`, progress lines, a SANE worker spawned with its id's
  one-backend `ConfigDir`), "a cancel mid-page" read as the sheet finished and none fed after, and the raw
  layout across the pipe byte for byte; slice 5, `pnm`'s PBM compared after the inversion (PBM is 1 black, the
  model 0), PGM and PPM byte for byte, 16-bit and 1-bit RGB through the `test` backend in the model's layout
  (the 16-bit conversion a function of the source's byte order, so its swap is unit-tested on CI's
  little-endian hosts), ids from Latin-1, the fallback on `INVAL` at open in a worker given the id's
  one-backend `ConfigDir` (`BeforeSystem`, naming none for an id with no backend: decisions 18 and 19; the fake
  listing a device under a new name), decision 20's refusals before `sane_open` (`BadId`), `INVAL` from a start
  retried once and then `Gone` (both added by step 2's repair), and the feeder keeping its sheets after a
  cancel. Slice 2's own entry says it's done on its branch (step 3).
- **The backlog** (step 3): TEST-9 (slice 6's SANE questions: the fi-6130's name and what replugging does; a
  sheet pulled in before the page in flight ends; the back after a cancel after a front; its vendor and model
  strings); Q-4 (SANE's strings are Latin-1 by the standard: whether `sane-dl` gives vendor, model and kind as
  bytes too, where it now gives lossy UTF-8, settled by slice 5; written only as the study finds it, with what
  backends are seen to send); decision 20's refusals (`BadId`, decision 25), slice 5's; and whatever the probes
  find. The README's counts recounted with `rg`, or left out.
- **Not changed:** `decisions.md` (the model's facts live in the note, this plan's decisions are for review),
  `crates/twain-dsm.md`, `plan.md` (slice 3 adds `in-progress` to its graph), `crates/scan-worker.md` (slice 4
  writes its protocol from the note's "What slice 4 needs", and the milestone's slice 4 points there), the PDF
  family's files.

### Where the note changes, and what it leaves

- **The TWAIN rows, slice 7's TWAIN paragraphs and the TWAIN unknowns stay byte for byte**, but where a SANE
  fact changes what both share: the model's cancel (section 7's model paragraph: "the sheet in flight", which
  slice 9's backend also meets) and its progress (section 3's and 7's: through `in-progress`, section 7's
  proposal paragraph made the model's, its TWAIN facts kept and the wake-up left to slice 9), each change said
  in the note's revision paragraph. A new unknowns row for the duplex back after a cancel names both slice 6
  (SANE) and slice 12 (TWAIN).
- **Off slice 8's lines:** the revision paragraph goes after the first version's status paragraph and before
  slice 7's, saying it came after slice 7's (below it), so slice 8's paragraph after slice 7's meets nothing;
  slice 7's sentence "SANE's rows are as they were, for slice 2" stays as history; the unknowns' SANE rows are
  rewritten in place, the raw layout's marked settled, and new rows go at the table's end, away from the events
  row slice 8 changes; section 3's "Decided by the TWAIN spike" paragraph and section 8's TWAIN paragraph are
  untouched (decision 15). **Section 8's model paragraph holds both:** its first sentences are the model's
  (`Backend::scan(settings, events)`, `progress` events, "`cancel` is a separate handle", the backends'
  threads, "`scan-worker` sees one shape for all of them"), and from "**TWAIN, by the spike:**" on, on the same
  lines, it is slice 7's and slice 8's (slice 8's branch rewrites its end). Step 2 rewrites only the model's
  sentences, leaving every line from "**TWAIN, by the spike:**" on byte for byte and the paragraph unreflowed.
- **Every model paragraph the new model contradicts is revised, not only added to** (step 2): section 1's (the
  id as Latin-1, the fallback, `renamed_from`; its "To settle" answered), section 2's (`Format` for `mode`,
  `Resolutions` as both, `duplex` in `Settings`; its "To settle"'s resolutions answered), section 3's (the raw
  layout; progress through `in-progress`), section 4's (`SideKnown`), section 5's, section 6's (`NotInstalled`
  in its table and model; warming up waited on inside the backend and said as a `Status` event, never an error,
  where the paragraph now says "not surfaced"), section 7's (the sheet in flight; the proposal made the
  model's), section 8's (above); the unknowns' `in-progress` row (taken by slice 2, for review; the TWAIN
  thread's wake-up slice 9's).
- **"Changes from proposal 9's model"**: its items 2 ("progress a separate event") and 6 ("finish the page in
  flight") revised, and the slice's added: `Format` for `Mode`; the sheet in flight; cancel and progress
  through `in-progress`; `NotInstalled`; the two traits.

### The probes

Under `scratch/work/s0-2/probes/` (a `README.md` saying how each runs and what it showed): a scratch crate,
`seam-probe`, taking `sane-dl` by path, with an empty `[workspace]` table and a lock of its own (it sits under
the repository's root, whose workspace would otherwise claim it: "current package believes it's in a
workspace", seen while reviewing; slice 1's probes did the same), built with `CARGO_HOME` at
`scratch/cargo-home` into a target directory under `scratch/work/s0-2/`, `--offline` where the registry's copy
has what it takes, every binary under the memory scope; one scenario a process (SANE is one
`sane_init` a process and reads `SANE_CONFIG_DIR` once), each under a timeout, through an isolated `ConfigDir`
naming `test` or `pnm` (never `/etc/sane.d`), or the fake built from `/crates/fake-libsane/` and loaded with
`Sane::load_from`, its script and log under the probe's directory. Reads are paced as `sane-dl`'s tests pace the
`test` backend (64-byte reads, a pause after each frame's first read), against UP-8's hang; a hang is the
timeout's, never a finding.

- **N1, names:** the isolated listing of `test` and `pnm` beside `scanimage -L`; the fake listing
  `fujitsu:fi-6130dj:12345`, `canon_dr:libusb:001:004` and a name and vendor holding a Latin-1 byte, what
  `sane-dl` gives for each. Then `open` of names no longer listed, through the real backends with nothing
  attached (an isolated `ConfigDir` naming none, since `dll` loads by prefix): `fujitsu:fi-6130dj:12345`,
  `fujitsu:libusb:001:004`, `canon_dr:`, `epson2:`, `avision:`, `kodak:`, `xerox_mfp:libusb:001:004`,
  `pixma:04A91234_ABCDE`, `test:9`, `pnm:2`, `nosuch:0`, each said as `sane-dl` reports it, beside the review's
  `scanimage` run (`scratch/work/s0-2/review/stale-open.txt`).
- **S1, stopping:** the `test` backend's feeder (`Automatic Document Feeder`): a batch of 3, dropped, then the
  next batch counted to `NO_DOCS`, then the one after; step by step, `sane_cancel` after a page's `EOF` and the
  next `sane_start`; the token midway through a batch's second page, then the next batch counted; the token
  between pages (after `next_page` returns, on the same thread, so it lands there for certain), then the next
  batch counted. The fake, `sheets 10`: the same, with its log; and `duplex`, a batch ended on a front, then the
  next batch's first page's mark.
- **L1, SANE's samples** (step 1): through `sane-dl` on the `test` backend, `Solid black` and `Solid white` at
  gray 1 and 8, a 16-bit gray page beside `scanimage`'s PGM (big-endian by PNM's rule), RGB at depth 1 (its bytes'
  channel order), three-pass at 1 and 16; through `pnm`, a PBM with a known bit pattern, its raster given back.
- **L2, the model's layout** (step 2): a conversion of `sane-dl`'s `Image` into the raw layout, written in the
  probe, beside `scanimage --format=png` (and `--format=tiff`'s photometric tag) for gray 1, gray 8, gray 16,
  color 8 and color 16, so each format SANE gives in one pass is held to a third party; compared pixel for
  pixel, at a width not a multiple of 8, a 1-bit row's padding bits masked on `scanimage`'s side (it inverts
  them) and checked 0 on the model's. 1-bit RGB stays L1's alone: `scanimage` refuses it, its assertion that
  an RGB frame is 8 or 16 bits deep stopping it before it writes a byte, in every format (corrected by step 2's
  repair: this line said it wrote an 8-bit PNM header, which its PNM writer would, but the assertion comes
  first), so it is no third party there. The PNG decoded with the `png` crate in the
  probe alone (image-rs's, MIT OR Apache-2.0, the decoder the pages crates will take): never a dependency of a
  crate here.
- **M1, the model sketched** (step 2): a scratch crate, `model-sketch` (an empty `[workspace]`, `in-progress`
  0.1.0 from the registry's copy, `--offline`), declaring the note's new section's types and traits as written,
  bodies stubbed, and compiling: `Box<dyn Device>` (the trait dyn-compatible), `Backend: Send` and `Device: Send`,
  a push backend's `scan` moving a `Sender<Event>` clone and `progress.child(..)` into a spawned thread, a
  `Tracker` cancelled from another thread and seen at the next `check()`, and `RawImage::new`'s checks (length,
  padding bits, `u32::MAX` by `u32::MAX` refused rather than wrapped) as unit tests. It shows the note buildable
  as written (Risk 2); slice 3 writes its own crate, never this.

### Dependencies, vetted

This slice adds none to any crate. For slice 3, which takes it, `in-progress` 0.1.0 read from the registry's copy
(`scratch/cargo-home`): MIT OR Apache-2.0; edition 2024, `rust-version` 1.85 (below the family's 1.89, so it
doesn't raise `scan-device`'s floor); no dependencies (dev: `rayon`, and `loom` under `cfg(loom)` for its own
tests); no `unsafe` in its one source file; `Tracker` is `Clone` and `Send + Sync`, `Progress` a `Send + Sync`
trait with `check()`, `set_total`, `set`, `inc` and `child`, its cancel cooperative ("nothing is interrupted"),
which is the model's "finish the sheet in flight"; published from github.com/sheaf-pdf/in-progress, the
org's own, taken by every sheaf crate that reports progress, and planned for `scan-cleanup` here
(`crates/scan-cleanup.md`). Young (0.1.0) and the
org's: its trait in `scan-device`'s public API ties a breaking change there to one here (Risk 6). The probes take
`sane-dl` by path and `png` (above), in `scratch/` only.

## Steps

**At every step**, the checking agent runs the core, once: `cargo test --locked --workspace -- --list` recorded
under `scratch/work/s0-2/` and compared with the step before's (none lost; with a warm target directory it builds
nothing); `git diff --stat` from the step's base outside `docs/pm/` (empty for a docs-only step); the commit's
message and diff scanned for local paths, the username, the maintainer's name, the downstream app's name outside
`docs/pm/`, and pdf-lint's unpublished plan; every path and section the changed lines cite found to exist; every
fact the changed lines state traced to its evidence (a probe's output line, a line of the standard, a man page,
`sane.h`, a line of `sane-dl`'s code, a studied source line at its commit, slice 1's or slice 7's plan), and any
claim without one narrowed or marked. **`just ci`** (with `SANE_DL_REQUIRE_LIBSANE=1` and
`SANE_DL_REQUIRE_HEADER=1`) runs only at a step whose diff reaches outside `docs/pm/` (a merge of slice 1's fixes,
or a `sane-dl` fix). Its baseline is the run on `4624760`'s own tree as the merge was committed (Baselines), not
run again at step 1: no build, test or gate reads `docs/pm/` (the gates' name check excludes it: `git grep … --
':!docs/pm'`), so a docs-only step can't change what it shows; `sane-dl-package` asks the registry, so if the
network is down it is recorded as not run, never as passed. **The trial merge** with slice 8's branch, part of the
core (every step edits the note or a file that branch edits, and it builds nothing): `git merge-tree --write-tree
--name-only HEAD origin/s0-8-twain-dsm`, the branch's commit named, listing no conflicted file beyond the six that
already conflict at `4624760` (`.github/workflows/ci.yml`, `README.md`, `docs/pm/backlog/tests.md`,
`docs/pm/slices/README.md`, `docs/pm/status.md`, `justfile`: slice 1's, seen while reviewing the plan, against
`0f08bb0`); the seam note, the milestone and `decisions.md` merging cleanly then, they must still. Left out
throughout, and why: the gates' make-fails (no gate changes), `cargo publish --dry-run` (no crate made ready;
`just ci`'s package recipe runs with it where it runs), the MSRV probe and `cargo tree` (no dependency changes),
`cargo check` for other targets (no platform code), the harness (nothing cut from sheaf). If a `sane-dl` fix
lands, its step adds: the fix's test, the fix undone on a worktree under `scratch/work/s0-2/` and its test seen to
fail, `sane-dl`'s changelog entry (unreleased), clippy for `x86_64-pc-windows-gnu` and `x86_64-apple-darwin` if it
touches code behind `cfg`, and `just ci` (its package and MSRV recipes among it).

1. **SANE as slice 1 and the probes found it: the SANE rows.** Probes N1, S1 and L1 (above), run and summarized.
   The note: the revision paragraph (placed as "Where the note changes" says); the SANE rows of sections 1, 2, 3,
   5, 6, 7 and 8 and their SANE "To settle" (section 2's from slice 1's step 3, as "What the SANE rows say" has
   it); a table of the backends' name forms, a row per backend with its source; the two SANE unknowns answered as
   "The SANE unknowns, answered" has them, with what only slice 6 settles; the unknowns table's SANE rows. Nothing
   of the model yet, but where a SANE row must name it.
   **Done when:** the core (its test list the slice's base); the probes run again by the checking agent from their
   README, each output the step quotes reproduced (the page counts of S1, the bytes of L1, N1's names and each
   stale name's `INVAL` from the real backend that answered it), the fake's logs read for the calls the note
   states; each backend's row in the names table checked against its studied source line or man page; the TWAIN
   rows, slice 7's TWAIN paragraphs and the TWAIN unknowns byte for byte as at `4624760` (a diff of those lines).
2. **The model fixed for slice 3, and what follows.** Probes L2 and M1. The note: the raw layout's table and
   `RawImage`'s checks; the new section "The model, for slice 3"; every section's model paragraph the new section
   contradicts, 1 to 8, as "Where the note changes" lists them (section 8's model sentences only, its TWAIN lines
   untouched and unreflowed); "Changes from proposal 9's model" (items 2 and 6 revised, the slice's added); the
   unknowns table (the raw layout settled; the `in-progress` row taken, for review; the duplex back after a
   cancel, slices 6 and 12; the rest as they were). Then `crates/scan-device.md`, `crates/sane-dl.md`'s "For the
   seam", and the milestone's slices 3 to 5, as "What follows elsewhere" has them.
   **Done when:** the core; L2 run again, each of the five formats equal to `scanimage`'s PNG raster pixel for
   pixel (16-bit after the swap, 1-bit padding masked on its side and 0 on the model's); M1 built and its tests
   run by the checking agent; `rg` over the note for the old model's words (`Backend::scan`, `mode`, "progress
   event", "separate handle", "page in flight", "not surfaced", "proposed") finding each either revised or a TWAIN
   or history line kept on purpose, said in the check's record; the model's types read against what each later
   slice needs, a row each in the check's record: slice 3 (every type and trait the milestone's slice 3 names,
   buildable as written, as M1 shows: `Device: Send`, `Box<dyn Device>`, `&dyn Progress`, a `Sender<Event>` and a
   child progress moved to another thread), slice 4 (plain data, sizes before bytes, `Stopped`, whole pages, the
   same sizes in a 32-bit worker, the protocol's consequences: `open`, `applied`, progress lines, the one-backend
   `ConfigDir` at the spawn), slice 5 (every `sane-dl` fact the SANE rows name mapped to a type: `Page::NoDocs`,
   `StartInvalid`, `Status { INVAL, Open }`, `LoadError`, `Image`'s formats, `Progress`), slice 9 (slice 7's TWAIN
   rows: tiles and the DIB, palettes and flavor, `TWEI_PAGESIDE` as `Reported`, `TWCC_OPERATIONERROR` as `shown`,
   sizes per source as `CAP_FEEDERENABLED` changes them, the settings window, the pending count 0xFFFF as an
   unknown length, a push backend's thread holding a child progress); `in-progress` 0.1.0's vetting re-read from
   the registry's copy; the TWAIN rows again byte for byte (section 8's from "**TWAIN, by the spike:**" on
   included), and each shared row's change said in the note.
3. **The backlog and the plan's files; the slice's end.** The backlog items ("What follows elsewhere"), each one
   line, the next free IDs on this branch, the README's table recounted or its counts left out; the milestone's
   slice 2 entry (done on the branch, waiting on review); `status.md` (slice 2 done on its branch, slice 3 next)
   and `slices/README.md`, a line each, kept short since slice 8's branch changes both.
   **Done when:** the core (its trial merge on the final tree); every commit message of the branch
   (`4624760..HEAD`) read for the names, the paths and `scratch/`; `git grep -i` for the maintainer's name outside
   the root `LICENSE-MIT` finding nothing new; `git diff --stat 4624760..HEAD` holding `docs/pm/` alone (else
   `just ci` here, on the final tree); every backlog line in the README's format; every file the note cites at the
   paths it gives. **What only CI shows:** nothing of this slice's (docs only); CI's Windows legs on pull request
   #2 are slice 1's (TEST-8), and its Ubuntu run of the tests is TEST-2's.

## Baselines

- **The tree at `4624760`** (slice 1's tip, `master` merged in with slice 7): `just ci` green on it, run in full
  after the merge was committed (`scratch/work/s0-1/merge/just-ci-2.log`, 2026-11-09 00:00:34, the commit at
  00:00:01, its tree clean since `sane-dl-package` refuses a dirty one): fmt, clippy, test, doc, the gates (15
  binaries read for libsane, all hold), `sane-dl-package` (16 files) and `sane-dl-msrv` on 1.89, no test saying
  it skipped, so the require variables would have changed nothing here. Step 1 records its test list, the base
  for each step's comparison.
- **SANE as installed here:** `libsane1`, `libsane-common`, `libsane-dev` and `sane-utils` 1.4.0-1ubuntu1, with
  the `test` and `pnm` backends; `/usr/include/sane/sane.h` 1.4.0; the man pages named above.
- **The standard:** its API and environment chapters, as slice 1 read them (sane-project.gitlab.io/standard;
  the source copies in `scratch/work/s0-1/sane-standard/`): names (Latin-1, the backend's form), "Image Data
  Format" (the polarity, the bit order, native 16-bit, byte-interleaved 1-bit RGB), "Code Flow" (`sane_cancel`
  after the frames wanted, even after `EOF`), `sane_set_io_mode` and `sane_get_select_fd`.
- **sane-backends' code, studied:** master `a113b257`, the copies slice 1's code review and step 5 read
  (`scratch/work/s0-1/review-api/upstream/`, `scratch/work/s0-1/step-5/upstream/`): `fujitsu.c`, `canon_dr.c`,
  `kodak.c`, `epson2.c`, `epsonds.c`, `avision.c`, `plustek.c`, `xerox_mfp.c`, `hp5590.c`, `kodakaio.c`,
  `pixma/pixma.c`, `escl/escl.c`, `net.c`, `dll.c`, `test.c`, under `backend/`, and `frontend/scanimage.c`.
  GPL: read, cited, never copied. `airscan` isn't sane-backends': sane-airscan is a project of its own (GPL
  too), cited by its `sane-airscan(5)`, installed here; slice 1's copy of its `airscan.c` is cited only with
  the commit it was read at, if step 1 can name it.
- **Slice 1's records:** its plan's "So far" (steps 2 and 4) and "Code review", its probes
  (`scratch/work/s0-1/step-2/`, `step-4/`), and `sane-dl` as built.
- **Slice 7's:** the note's TWAIN rows at `4624760`, `slices/s0-7-twain-spike.md`, and `twain-spike`'s `image`
  module (the comparable form).
- **`scanimage`** (sane-utils 1.4.0, a separate process): names (`-L`), and pages in PNM, PNG and TIFF for L1 and
  L2; the planning probe's three formats of one 1-bit page (`scratch/work/s0-2/plan-probe/`); the plan review's
  probes (`scratch/work/s0-2/review/`): eleven stale names opened (nine installed backends and one that isn't), a
  1-bit page's PNG padding, a 16-bit page's PNG beside its PGM, and the push backend's sketch against
  `in-progress`.
- **sheaf:** no comparison. Its cited plan files checked for drift as the slice starts (2026-11-09: still at
  `3232a35`, `git diff --stat` empty).

## Decisions taken alone, for review

1. **Three steps, docs only:** SANE's facts, then the model, then the bookkeeping; no crate's code unless a probe
   finds a `sane-dl` defect the seam can't do without; `just ci` wherever a step reaches outside `docs/pm/`, not
   at docs-only steps, its baseline the merge's own run on `4624760`; the test list at every step.
2. **The raw layout's polarity: 0 the least intensity at every format, `Bw1` included (0 black).** It reverses the
   first version's proposal (1 black, as SANE has it): one rule for every format, so a depth change never
   inverts; it is slice 7's comparable form, TWAIN's chocolate flavor and the sample's DIB palette, PNG's and
   PDF's `DeviceGray`; SANE's 1-bit gray is the one format inverted, by the `sane` backend, about 1 MB a 300 dpi
   Letter page beside the 25 MB of its color page.
3. **Bits:** the leftmost pixel in the most significant bit, the bits past a row's last pixel 0.
4. **16-bit little-endian, whatever the host:** SANE's native order on all three CI systems, so nothing to do
   there; the `sane` backend swaps on a big-endian host. A layout stated once, not "the host's", since a page
   crosses a process and may be stored.
5. **Rows top down and packed, channels R, G, B:** no padding, no stored stride (`stride()` from format and
   width); every backend drops its API's padding.
6. **Five formats, no 1-bit color, and a checked constructor:** SANE's 1-bit RGB expanded to `Rgb8` by the
   `sane` backend; `RawImage`'s fields private, `RawImage::new` checking the length and a `Bw1` row's padding bits.
7. **`Format` is also the setting,** in place of proposal 9's `Mode`, so a source's caps list the formats it
   gives and 16-bit is asked for and clamped like any setting.
8. **Cancel finishes the sheet in flight:** in duplex, a delivered front is followed by its back before the batch
   ends, so "Scan More" never starts on a back. Proposal 10's "page in flight" read as the sheet; a change to
   the model both backends meet, said in the note.
9. **`sane-dl`'s token isn't the model's cancel:** it stops at once and loses the sheet in flight (probe S1); the
   `sane` backend checks between sheets and drops the batch; stopping at once stays the watchdog's.
10. **Cancel and progress through `in-progress`'s `Progress`,** as the note proposed, open to review: checked
    between sheets, each page's bytes reported, replacing the note's progress event; the TWAIN thread's wake-up
    left to slice 9. If review declines, the model takes a cancel handle of its own (`Clone + Send + Sync`, a flag)
    and the progress event back, and nothing else changes.
11. **A SANE id is `sane:` and the name's bytes as Latin-1** (the standard's encoding): reversible byte for byte
    whatever the backend sent, UTF-8 included; the name shown to people is the bytes as UTF-8 where they are,
    else as Latin-1.
12. **A renamed device is found by backend, vendor and model, only when exactly one matches,** after `INVAL` from
    `sane_open`, and said (`renamed_from`); otherwise `Gone`.
13. **Two traits, `Backend` and `Device`,** as SANE's library and handle are, and as a worker keeps one device
    open for a dialog; `scan` blocks its thread and takes the sheet to count from.
14. **`ScanError::NotInstalled`** for a system without its driver API (no libsane, no DSM), apart from `Gone`.
15. **The note's edits keep off slice 8's lines:** its revision paragraph above slice 7's, the SANE unknowns
    rewritten in place and new rows at the table's end, slice 7's paragraphs untouched, section 8's model
    sentences rewritten without touching or reflowing its TWAIN lines; shown at each step by a trial merge with
    slice 8's branch (`git merge-tree`), not only by reading.
16. **The probes are summarized, never committed,** and the `png` crate decodes `scanimage`'s pages in the probe
    alone; `in-progress` is built only in M1's sketch, which slice 3 doesn't reuse.
17. **Paper sizes in `f32` millimetres** (`SizeMm { width: f32, height: f32 }`): SANE gives its area in
    millimetres (fixed-point), and Letter (215.9) and Legal (355.6) aren't whole millimetres, so a `u32` of
    millimetres would make them inexact; `f32` as proposal 9's `dpi` already is. `Settings` then derives
    `PartialEq`, not `Eq`. The alternative, `u32` micrometres (exact for the three constants and for WIA's
    thousandths of an inch), needs a unit the name doesn't say; slice 3 may take it and say so.
18. **The worker that opens a stored SANE id is started with a one-backend `ConfigDir` for that id's backend**
    (none where `backend_of` gives none: decision 19), written `Search::BeforeSystem` (step 1's repair), so the
    fallback's listing runs in it, fast and confined to that backend, while the backend still reads its own
    `<backend>.conf` and `dll` the system's `dll.aliases` (decision 19): SANE reads its configuration once, as it
    starts, so a worker can't narrow its own listing afterwards, and `ConfigDir` leaves the variable to the caller.
    The client (slice 4) passes it at the spawn; the `sane` backend (slice 5) says what it needs. The alternative,
    listing through the system's configuration in that worker, waits on every backend's probe (about 9 s here) and
    lets a listing cross backends, which security item 1 forbids; an `Isolated` directory leaves a backend on its
    built-in defaults and resolves no alias.
19. **An alias needs nothing of its own: its worker's directory is decision 18's, naming the backend `backend_of`
    gives, or none where it gives none, and the alias resolves there** (step 1, revised by its repair). An alias in
    the system's `dll.aliases` is listed in place of the device's name and may be named anything: `Desk scanner`
    has no `:` (probe N1, `backend_of` none), and `office:scan`'s `office` is no backend (the check's probe,
    `backend_of` `Some("office")`), so nothing in a listing tells an alias from a backend's name. None is needed:
    SANE takes each file from the first of its directories that holds one, so a `BeforeSystem` directory holding
    `dll.conf` and an empty `dll.d` but no `dll.aliases` lists its own backends and resolves the system's aliases
    at open (the repair's probe, a directory standing for the system's after it: `office:scan` opened `test`'s
    device 0 through a directory naming `office`, `Desk scanner` `test`'s device 1 through one naming none, each
    listing 0 devices). An alias whose name names no backend has no fallback, by construction: its listing finds
    nothing, and `INVAL` is `Gone`; one named after the backend it aliases falls back within that backend, as any
    id. No worker opening a stored id runs with the system's `dll.conf`. The first version of this decision gave an
    id with no backend before a `:` the system's configuration and left the directory's search unsaid: it missed
    the alias with a colon (an `Isolated` directory naming `office` answers `INVAL`: `Gone` though there), and sent
    `:0` to the system's last-listed backend (decision 20). The alternative, reading `dll.aliases` ourselves to
    find the alias's backend, reads SANE's configuration files, which `sane-dl` leaves to SANE.
20. **The `sane` backend refuses, before `sane_open`, an id whose name starts with one of `dll`'s testing
    prefixes** (`fakeusb:`, `fakeusbdev:`, `fakeusbout:`) **or with `:`, or is empty** (step 1; the last two
    added by its repair): no listing gives the first, and through them a name makes SANE replay USB traffic from
    a file the name gives, or record into one (`backend/dll.c`; probe N1). A name whose backend part is empty
    goes to `dll`'s first backend, the last its `dll.conf` lists (`sane_open`; `add_backend` puts each before the
    others): `:0` opened `pnm:0` through a configuration naming `test` then `pnm` (the check's probe), the
    device of sane-dll(5)'s "default backend", which the id never named. In decision 19's directory it reaches
    that directory's backend or answers `INVAL` (the repair's probe: `:0` and the empty name through a directory
    naming none), so the refusal gives a malformed id its own error (`ScanError::BadId`, decision 25), not
    `Gone`, and doesn't rest on which directory a worker was given. No backend forms such a name through
    `dll`; an alias named with a leading `:` is refused too, a limit. Slice 5's, its backlog item written in
    step 3. An empty name after the backend (`fujitsu:`), or a backend's name alone (`fujitsu`), which opens
    the backend's first device, isn't refused: it stays within the backend the id names, as the fallback does.
21. **The model is written in the note as a Rust block** ("The model, for slice 3"), declarations only, which
    probe M1 compiles and a check holds it to (the block, formatted, equals the sketch's), so slice 3 starts
    from types that build (step 2). What the design left open, taken there: `Clamped` holds what was asked of
    each setting the device changed, what it took being in `Applied::settings`; `Event::Sheet(n)` as sheet
    `n` begins; `Backend::list` takes a `Duration`, as `sane-dl`'s `devices` does; `Driver::Ica` and
    `fake:<name>` ids; `LayoutError` (`Empty`, `TooLarge`, `Length`, `Padding`) and `ScanError` derive
    `thiserror::Error`, `Clone` and `Eq`, as plain data; `Opened` derives nothing, since it holds a `Box<dyn
    Device>`. Slice 3 may rename and says so.
22. **SANE's resolutions become whole dpi, and its automatic value isn't the app's** (step 2): a `FIXED` or
    `INT` range or word list becomes `Resolutions` in whole dpi, and the app asks for a dpi, never the
    automatic value; the model's `dpi: u32` is proposal 9's.
23. **`LoadError::NotFound` is `NotInstalled`; the other `LoadError`s are `Driver`** (step 2): a library found
    but unusable (a symbol missing, `sane_init` failing, a major version not 1, a second load) is the system's
    SANE broken, not missing, and the message says which.
24. **The note's model section says what slices 4, 5 and 9 meet** (step 2): beside "What slice 4 needs", a
    table of what the `sane` backend maps from `sane-dl` and a paragraph of what the `twain` backend meets
    from slice 7's rows, so the review can read each later slice against the types; the `StartInvalid` row
    (set again, start once more, then `Gone`) is `crates/scan-device.md`'s `sane` bullet as it was, and since
    step 2's repair section 6's table says the same (step 1 had put `INVAL` from `sane_start` under `Driver`,
    so slice 5 had two answers). A re-read that failed (`reload`) is mapped as its own error is. The
    alternative is `Driver` with `sane-dl`'s message, but a second `INVAL`, the settings set again from the
    descriptors as read again, leaves the app nothing to change, and NAPS2 reads the status as offline (the
    spec's `sane` bullet), so `Gone`, the spec's answer, for review.
25. **`ScanError::BadId` for a stored id the backend refuses before its driver sees it** (step 2's repair):
    decision 20's names, an id that isn't `sane:` and Latin-1 (a character past U+00FF), and a `NUL`
    (`sane-dl`'s `NulInName`, where one reaches it) for SANE; for TWAIN, WIA and ICA what their backends
    refuse, theirs to say (slices 9 and 10). Decision 20 gave such an id "its own error, not `Gone`" with no
    variant to carry it, and the model had no row for it. Apart from `Gone` because they differ for the app: a
    device gone may come back and its id is kept, while such an id never opens, wherever its worker runs, so
    the app can drop it and offer the list. A unit variant: the caller holds the id it passed, and an untrusted
    id isn't echoed into a message. The alternatives: `Gone` (the app keeps retrying an id that can't open) or
    `Driver` (a message for the user about a stored setting the driver never saw). Probe M1 holds it (its
    fourteenth test).
26. **The check's points for slice 4 go into the note's "What slice 4 needs"** (step 3; step 2's check, its
    minor points): a `RawImage` decoded on the client is made by `RawImage::new`, never a derive that fills its
    private fields; the kill after a cancel allows for the sheet in flight, since `crates/scan-worker.md`'s 10 s
    (proposal 4) predates a cancel that finishes a sheet's two sides, and slice 4 sets it anew (counted from the
    worker's last line, as the watchdog's 60 s is, the note's suggestion); a `settings_window` request, which
    the protocol's list lacked; and the stored-id worker started in a directory it trusts (security item 6).
    The milestone's slice 4 says each in a clause. The alternative, a backlog item for slice 4, would leave
    the note that slice 4 writes its protocol from short of them.

## For review (security)

1. **The fallback to a renamed device opens a device the user didn't name:** only one of the same backend, vendor
   and model, and only when it's the only one listed, never across backends (so a stored `sane:fujitsu:…` id can
   never become `pnm:0`, which reads any file its option names), and the app is told. Slice 1's item 7 holds:
   the names a worker opens come from a listing or the user's stored id. Decision 18 makes "never across
   backends" hold by construction: the worker's SANE lists only the stored id's backend.
2. **A stored id is untrusted input:** a character past U+00FF can't be Latin-1 and is refused, never cut; a `NUL`
   is refused before SANE sees it (`sane-dl`'s `NulInName`); each is `ScanError::BadId`, which doesn't echo
   the id (decision 25).
3. **A page from a worker is untrusted:** its format and size read and capped before its bytes, `RawImage::new`
   refusing a length that doesn't match (slice 4's caps; `plan.md`'s hostile-input rule).
4. **The probes load SANE only through private configuration directories,** and `pnm` reads only the files the
   probe wrote. N1's stale names load installed backends by prefix (`fujitsu`, `canon_dr`, `epson2`, `avision`,
   `kodak`, `xerox_mfp`, `pixma`), each of which looks for its scanners on USB and finds none: reads only, with
   no configuration file of theirs in the private directory. *Corrected in step 1:* `pixma`'s open also
   broadcasts its network discovery (BJNP) on each IPv4 and IPv6 interface, loopback skipped (seen with
   `SANE_DEBUG_BJNP`), as any open of a `pixma` name no longer there does, so N1 sent discovery packets on the
   local network, and found no scanner. `kodak` looks on SCSI, not USB.
5. **A stored SANE id reaches `dll`'s own name rules** (step 1, probe N1; its repair): `fakeusb:<file>` makes
   SANE read and parse an XML file the id names (a missing one answered `ACCESS_DENIED` after libxml2 tried to
   load it), `fakeusbout:<file>:<name>` makes it record a USB backend's traffic into one, a name whose backend
   part is empty (`:<device>`, or the empty name) opens a device of `dll`'s first backend, the last its
   `dll.conf` lists, which the id never named, and an empty name after the backend opens that backend's first
   device. No listing gives a testing prefix and no backend an empty backend part, so the `sane` backend refuses
   both before `sane_open` (decision 20), beside item 2's checks; the empty name after a backend stays within
   it. An alias resolves through the system's `dll.aliases` in the worker (decision 19) and opens the device the
   system's listing showed under it, on whichever backend the alias names: the system's configuration, which the
   listing trusts already, not a fallback, which stays within the directory's backend (item 1).
6. **The stored-id worker's current directory** (step 3, from step 2's check): `Search::BeforeSystem` has SANE
   read the process's current directory after the `ConfigDir` and before `/etc/sane.d` (`sane-dl`'s `Search`;
   sane-dll(5)), and decision 19 leaves `dll.aliases` and the backend's own `.conf` to the first directory that
   holds one, so either file planted in the worker's current directory would be read: an alias there could
   turn a stored id into a device of another backend, which `dll` loads by its prefix (slice 1's security item
   7), past item 1's "never across backends" (from `dll.c`'s order, not tried). The worker is started in a
   directory it trusts, as slice 1's item 4 says of any process loading SANE so: the note's "What slice 4
   needs" and the milestone's slice 4 say it, slice 4 chooses the directory.

## Risks

1. **A docs slice's claims outrun its evidence:** names on replugging can't be tried here. Each claim is traced
   at its step's check; what only the fi-6130 shows is said as slice 6's (TEST-9), and studied facts are marked
   as studied.
2. **The model's types meet slice 3's compiler:** a bound or a lifetime that doesn't build as written. Slice 3
   departs and says so; the model stays provisional (0.0.x) until slice 11.
3. **The merge with slice 8's branch:** the note's head, sections 3 and 8, the events row; `status.md`,
   `decisions.md`, `slices/README.md`, `backlog/tests.md`. Edits here keep off those lines, and the core's trial
   merge shows it at each step. Slice 8's branch adds its own TEST-7 and names TEST-8 next, both of which slice
   1's branch already holds, so the IDs are renumbered at whichever merge lands second, this slice's TEST-9
   included, said at the merge. That branch also deletes `twain-spike` (its `image` module moved into
   `twain-dsm`'s tests' support) and moves slice 7's plan into `archive/slices/`: the note's new text names the
   module without a path, and this plan's own citations of those paths are a record as of `4624760`.
4. **A fix to slice 1 merged in mid-slice** (pull request #2's Windows reruns): the step after it runs `just ci`
   and the test list, and the account says so; `master` merged in with slice 8 would drop `twain-spike`'s tests
   from the list, expected and said.
5. **The `test` backend's hang (UP-8) in the probes:** reads paced, each scenario under a timeout; a hang is said
   as one, never read as a finding, and the run made again. `scanimage` meets it too: while planning, 2 of 21
   one-page scans of `test:0` here hung after writing their whole file, ended by `timeout`
   (`scratch/work/s0-2/plan-probe/out/runs.txt`).
6. **`in-progress` in `scan-device`'s public API:** a breaking change there is one here; both are 0.x and the
   org's, and decision 10 says what the model does if review declines it.
7. **The polarity reverses the first version's proposal:** if review rules for 1 black, the inversion moves from
   the `sane` backend to the `twain` backend (chocolate and black-then-white inverted) and `scan-image`'s side,
   one line each; the note gives the alternative.

## Plan review

2026-11-09, before step 1, through three lenses: fidelity (slice 2 exactly, the specs, the seam, the later
slices' path), mechanics (each step green on its own; the probes' builds; the merge with slice 8's branch),
behaviour and proof (what each check can show, and where its evidence comes from). Each finding was checked
against the sources, the code or a probe (`scratch/work/s0-2/review/`, its `README.md` saying how each ran)
before it was acted on; the plan as drawn is kept at `scratch/work/s0-2/plan.before-review.md`.

**Accepted:**

1. *Fidelity, major.* Step 2 revised the model paragraphs of sections 3, 4, 5 and 7 only, and the note would
   have contradicted its new section elsewhere: section 8's model sentences still say `Backend::scan(settings,
   events)`, `progress` events and "`cancel` is a separate handle" (and share their lines with slice 7's and
   slice 8's TWAIN text, from "**TWAIN, by the spike:**" on); section 1's id is `sane:<name>`; section 2's
   `Settings` has `mode`; section 6 says warming up is "not surfaced" beside a `Status::WarmingUp` event and has
   no `NotInstalled`; "Changes from proposal 9's model" keeps "progress a separate event" and "the page in
   flight"; the unknowns' `in-progress` row is "proposed, slice 3". Step 2 now revises every model paragraph
   the new section contradicts, sections 1 to 8, section 8's model sentences only and its TWAIN lines
   unreflowed, with an `rg` of the old model's words in its check ("Where the note changes").
2. *Fidelity, medium.* The fallback listed "through a one-backend `ConfigDir`" in the worker that tried the
   open, which can't be: SANE reads its configuration once, as it starts (`/crates/sane-dl/README.md`), and
   `ConfigDir` leaves `SANE_CONFIG_DIR` to the process's starter. Decision 18: the worker that opens a stored
   SANE id is started with its backend's one-backend `ConfigDir`, so the open and its fallback listing run
   there, fast and unable to cross backends (security item 1 by construction); slices 4 and 5 carry it.
3. *Behaviour, medium.* "A name no longer listed answers `INVAL`" rested on the fake, whose `INVAL` for an
   unlisted name is its own rule (`fake-libsane`'s crate docs: "any other name returns `INVAL`"), so no
   evidence. The installed backends answer it themselves with nothing attached: eleven stale names through
   `scanimage` with an empty private configuration, each "Invalid argument", `dll`'s log showing `fujitsu` and
   `epson2` loaded by prefix (`stale-open.txt`, `stale-open-dll.txt`). N1 opens them through `sane-dl`;
   security item 4 says what loading those backends does.
4. *Behaviour, medium.* "Keeps off slice 8's lines" was checked by reading alone. A trial merge shows it:
   `git merge-tree --write-tree --name-only 4624760 origin/s0-8-twain-dsm` (at `0f08bb0`) conflicts in six
   files already, all slice 1's, and merges the seam note, the milestone and `decisions.md` cleanly; the core
   runs it at each step, and no new file may conflict.
5. *Behaviour, medium.* The model's "buildable as written" was a reading, though Risk 2 names that risk. Probe
   M1 compiles the note's types and traits against `in-progress` 0.1.0, offline. A review sketch built and ran
   the shape a push backend needs (`Box<dyn Device>`, a `Sender` clone and `progress.child(..)` moved into a
   spawned thread: `sketch/`, "events 1").
6. *Fidelity, medium.* Section 2's SANE row was left out "unless a probe adds to it", though slice 1's step 3
   recorded SANE facts "for slice 2" (`resolution` `FIXED` on `test`, an `INT` word list that can be automatic
   on `pnm`; `source` values; `mode` `Color` saying `RELOAD_OPTIONS`; `INEXACT`) and the milestone asks for
   slice 1's findings folded in. Step 1 folds them in ("What the SANE rows say", 2).
7. *Behaviour, minor.* L2's comparison with `scanimage`'s PNG would fail falsely at 1 bit: `scanimage` inverts
   a row's whole bytes for PNG, padding included (`frontend/scanimage.c`, studied), and a 157-pixel `Solid
   white` row ends `0xFF` in its PNG, `0x00` in its PBM (`png-checks.txt`). L2 compares pixels, the padding
   masked on its side and checked 0 on the model's, at a width not a multiple of 8, and adds color 16, which
   `scanimage`'s PNG gives big-endian (a gray-16 page's PNG and PGM byte-equal, `0x0001` and `0x0100` both in
   it); 1-bit RGB stays L1's.
8. *Fidelity, minor.* "What follows elsewhere" left out `crates/scan-device.md`'s `sane` and `fake` bullets and
   what the two traits do to `scan-worker`'s protocol (an `open` request with `renamed_from`, an `applied`
   event, progress lines). Both added; `crates/scan-worker.md` is left to slice 4, said under "Not changed".
9. *Fidelity, minor (API).* `SizeMm`'s type was unstated while "sizes `u32`" read across the model, and
   Letter's 215.9 mm isn't whole. Decision 17: `f32` millimetres, the alternative said.
10. *Fidelity, minor (API).* How a push backend's thread reports progress was unsaid: through
    `Progress::child`, an owned `Box<dyn Progress>`, `Send` and cancelled with its parent (finding 5's sketch).
11. *Behaviour, minor.* `RawImage::new`'s length could wrap: `u32` × `u32` × 6 overflows a 32-bit `usize`, and
    at `u32::MAX` by `u32::MAX` a `u64` too. Worked out in `u64`, checked, as slice 7's `image` module works
    its sizes; M1 tests it. Slice 5's 16-bit conversion takes the source's byte order, so its swap is tested
    on CI's little-endian hosts.
12. *Behaviour, minor.* The TWAIN column's bit order said "the same in both transfers" without saying what, and
    the sample reports `ICAP_BITORDER` as `TWBO_LSBFIRST` (slice 7's capability table). The cell now reads
    slice 7's record: the high bit first, a DIB's rule, memory equal to it pixel for pixel, whatever that
    capability reports. No TWAIN fact re-decided.
13. *Mechanics, minor.* The probe crates sit under the repository's root, and cargo refuses them ("current
    package believes it's in a workspace when it's not", seen with the sketch): each takes an empty
    `[workspace]` and its own lock, as slice 1's probes did.
14. *Mechanics, minor.* Slice 8's branch deletes `twain-spike` (its `image` module moved into `twain-dsm`'s
    tests' support) and moves slice 7's plan into `archive/slices/`, so a path to either in the note breaks at
    that merge: the note names the module by crate and module. Risk 3 also says slice 8's branch names TEST-8
    next, which slice 1's branch holds too.
15. *Fidelity, minor.* Baselines put `airscan.c` among sane-backends' files at `a113b257`; sane-airscan is a
    project of its own (its header names its own author and licence file). Cited by `sane-airscan(5)` instead,
    or with the commit its copy was read at.
16. *Behaviour, minor (proportion).* Step 1 ran `just ci` as "the slice's baseline", but that baseline exists:
    the run on `4624760`'s clean tree right after the merge was committed, every recipe green, `sane-dl-msrv`
    on 1.89 included, no test skipped (`scratch/work/s0-1/merge/just-ci-2.log`). Nothing the slice changes is
    read by a build, test or gate, so step 1 cites it rather than running it again (the 2026-10-29 ruling);
    `just ci` stays for any step whose diff leaves `docs/pm/`, and `sane-dl-package` is "not run", never
    passed, if the network is down.
17. *Behaviour, minor.* S1's token cancels were placed loosely ("mid-page"; "between pages") while its
    expected counts presume a place (the next batch giving 8 means the second page): the scenario names the
    second page, and the cancel between pages is made on the same thread after `next_page` returns, so it
    lands there for certain.

**Rejected:**

1. *The model's section reaches past slice 2,* whose entry names the SANE rows, the unknowns and the raw
   layout: the milestone checks it as "reviewed before slice 3 starts, since it fixes slice 3's API", and this
   run's review is to check the model's types against slices 3, 4, 5 and 9, which needs them written.
2. *`scanimage`'s 16-bit PNG may be in the host's order, so L2's "after the swap" would be wrong:* it swaps for
   PNG as for PNM on a little-endian host (its PNG write's comment and swap), and the review probe found the PNG
   and PGM rasters byte-equal with asymmetric values.
3. *A vanished device may answer `IO_ERROR` or `ACCESS_DENIED` at open, not `INVAL`, so the fallback's trigger
   is too narrow:* the seven USB backends tried all answer `INVAL` with nothing attached; what a replugged
   fi-6130 answers stays TEST-9's.
4. *The revision paragraph above slice 7's breaks the note's order:* the order keeps the merge clean (slice
   8's paragraph goes after slice 7's), and the paragraph says it came after; decision 15, for review.
5. *Drop the test list at docs-only steps:* the run's rules ask for it at each step, and with a warm target
   directory it builds nothing.
6. *Give vendor, model and kind as bytes from `sane-dl` now, for the fallback's match:* lossy UTF-8 is the same
   on both sides of a match within one worker, and enough for display; Q-4 asks slice 5, and a docs slice
   changes no crate for a want that isn't a defect.
7. *`cargo check --target x86_64-pc-windows-gnu`:* no code changes; the plan's "Left out" already says so.

## So far

### Step 1: SANE as slice 1 and the probes found it: the SANE rows (2026-11-09)

One commit, the one that carries this account. Docs only: `crates/scan-device-seam.md` and this plan.

- **What it does:** the note's revision paragraph, above slice 7's and saying it came after; the SANE rows of
  sections 1, 2, 3, 5, 6 (its three SANE cells, and a paragraph on `sane-dl`'s statuses and
  `LoadError::NotFound`), 7 and 8, as `sane-dl` gives SANE and as the probes found it; SANE sentences added to
  the "To settle" of sections 1, 2 and 6; a table of SANE's names, a row per backend with its source (section
  1); the two SANE unknowns answered, names that change on replugging (section 1, with the fallback's listing in
  a worker started with the id's one-backend `ConfigDir`) and stopping after the page in flight (section 7),
  each with what only slice 6 settles; the unknowns table's two SANE rows rewritten in place. Nothing of the
  model's paragraphs yet (step 2), but where a SANE answer names it (the Latin-1 id, the fallback, decisions 18
  to 20). Slice 7's TWAIN rows, its TWAIN paragraphs and the TWAIN unknowns are byte for byte as at `4624760`:
  every line of that note missing from this one is a SANE row, a SANE cell's row of section 6 (its TWAIN, WIA and
  ICA cells equal), or a first-version "To settle" line, checked by a script over both files. Section 7's "To
  settle" is left as it was, since slice 7's TWAIN text shares its lines; the answer is a paragraph after it.
- **The probes** (`scratch/work/s0-2/probes/`, its `README.md` holding how to run them and each output quoted;
  `run.sh`; a crate `seam-probe` taking `sane-dl` by path, an empty `[workspace]` and its own lock, built
  `--offline`; the fake built from `/crates/fake-libsane/` into a target directory there): one scenario a
  process, in the memory scope, under `timeout` (no try needed again), through an isolated `ConfigDir` written
  by `sane-dl` or through the fake.
  - *N1, names:* `test` and `pnm` listed in 0.001 s (`pnm:0`, `pnm:1`, `test:0`, `test:1`), as `scanimage -L`
    lists them. Through the fake, `fujitsu:fi-6130dj:12345`, `canon_dr:libusb:001:004`, a name with a Latin-1
    byte (`epson2:net:caf\xe9.local`) and one with UTF-8 bytes given byte for byte, each made a Latin-1 id and
    back to equal bytes, and opened by them (the fake opens only a listed name); the vendor's Latin-1 byte lost
    to lossy UTF-8 (`"Caf\u{fffd} Scan"`: Q-4's case). The eleven stale names, through a configuration naming no
    backend: each `Status { status: Status::INVAL, call: Open }`, the backend's own log showing the open
    reaching it (`fujitsu`, `canon_dr` and `kodak` ending `no device found`, `epson2` `open failed: Invalid
    argument`, `test` `device '9' not found`), `nosuch:0` `INVAL` from `dll` (`couldn't find backend`); 0.000 to
    0.020 s, `pixma` 0.534 s (the review's `scanimage` run: the same eleven, "Invalid argument").
  - *S1, stopping* (`test:0` at 10 dpi from its feeder; the fake with `sheets 10`; the same counts on both
    unless said): a batch dropped after 3 pages, then `(7, Start)`, then `(10, Start)` on `test` and `(0,
    Start)` on the fake, which doesn't fill again; `sane_cancel` after a page's `EOF`, then `start 2: GOOD`, then
    a batch `(8, Start)`; the token midway through page 2 (`at 640 of Some(1209) bytes`, the fake's `32 of
    Some(64)`), `page 2: Cancelled`, then `(8, Start)`; the token after page 1 returned, `page 2: Cancelled`,
    then `(9, Start)`, the fake's log holding no `sane_start` between. The fake's duplex: a batch ended after
    page 1 (`F`), the next batch's first page `B`, page 2. No `violation` in any of the fake's logs.
  - *L1, SANE's samples* (`test:0` at 50 dpi, 157 by 196): gray 1 `Solid black` every byte `0xff`, padding bits
    included, `Solid white` `0x00`; gray 8 and RGB 1 and 8, black `0x00` and white `0xff`; RGB 1's stride 60
    (3 × 20), its `Color pattern` row 0 eight bars of `000` to `111` as R G B bits; a three-pass page with its
    frames in the order blue, green, red equal byte for byte to the one-pass page, at 1 and at 16 bits; a 16-bit
    gray page, each sample swapped, equal to `scanimage`'s PGM (`P5 157 196 65535`; 29,223 of 30,772 samples
    with two different bytes); a 13 by 3 PBM through `pnm` given back byte for byte (`b3 af 00 07 ff f8`), its
    set padding bits included.
- **Departures from the plan, and why:**
  - *`canon_dr` names by its connection alone*, not by model and serial as the plan expected: the lines that
    would use `init_serial` are commented out at `a113b257`. *`kodak` is SCSI*, its names device paths (its
    models speak SCSI and FireWire; sane-kodak(5)), not a USB connection; its stale `libusb` name still answered
    `INVAL` from `kodak`. *`plustek` names by its configuration's `device` line* (`auto` by default). The
    note's table says what was found.
  - *N1 gained two scenarios:* an alias in `dll.aliases`, and `dll`'s USB testing prefixes (`fakeusbout:`,
    `fakeusb:`), both name rules of `dll` that a stored id meets: decisions 19 and 20, security item 5. Found on
    the way: a name with no `:` is read by `dll` as a backend's name (`backend/dll.c`, and the alias probe), not
    as the default backend's device, as sane-dll(5) says.
  - *`pixma`'s open broadcasts its network discovery:* security item 4 corrected.
  - *1-bit RGB's channel order shown through three-pass*, the frames labelled by SANE, not beside the 8-bit
    page: the `test` backend draws its 1-bit `Color pattern` apart from its 8-bit one (a comparison at a
    threshold of 128 differed in about half the samples), so there's no third party for 1-bit RGB here, as the
    plan said of `scanimage`.
  - *The fake's names include a UTF-8 one,* to show the Latin-1 id goes back byte for byte whatever the bytes.
  - *The `airscan` row cites sane-airscan(5) alone:* slice 1's copy of its `airscan.c` has no commit recorded.
  - *SANE as installed says `1.2.1-debian`* (`scanimage --version`, and `dll`'s and `pnm`'s debug lines),
    though the packages are 1.4.0-1ubuntu1: the build's version string; the note keeps "1.4.0", the packages'.
- **Decisions taken alone:** 19 and 20 above; the names table placed in section 1 after its API table, and the
  answers as paragraphs of their own in sections 1 and 7, so no TWAIN line moves.
- **Quick checks here:** `git diff --stat` touching `docs/pm/` alone; the trial merge (`git merge-tree
  --write-tree --name-only` of a commit of this tree and `origin/s0-8-twain-dsm` at `0f08bb0`) conflicting in
  the six files of `4624760` alone, the seam note merging cleanly, slice 8's paragraph landing after slice 7's;
  the TWAIN lines as above; the diff and this message scanned for local paths, the maintainer's name and
  pdf-lint's plan. The test list as the slice's base, the probes run again and the other checks of the core are
  the checking agent's; `just ci` isn't run (no file outside `docs/pm/` changed; Baselines).
- **Its check** (at `f0fc619`; record in `scratch/work/s0-2/flow/checks.md`, "Step 1"): the test list at
  `4624760` and at `f0fc619` 245 tests, the same names (the slice's base); nothing outside `docs/pm/` changed;
  the message and added lines free of local paths, the maintainer's name and pdf-lint's plan; the trial merge
  against `0f08bb0` and against slice 8's current tip `6ca9a2d` conflicting in the six files of `4624760` alone;
  `just gates` holding; every cited path, section, man page and studied line found; N1, S1 and L1 run again from
  the README, each output the step quotes reproduced; each names-table row held against its source; the TWAIN
  lines byte for byte as at `4624760`. Two failures, both of `dll`'s name rules, and six minor wordings.
- **Repaired after its check** (amending this step's commit, docs only):
  - *`dll`'s fourth name rule was missed.* The note said three rules and that a name without a `:` isn't
    sane-dll(5)'s default backend's, but `sane_open` has a fourth: a name whose backend part is empty
    (`:<device>`, and the empty name) opens on `dll`'s first backend, the last its `dll.conf` lists
    (`backend/dll.c` at `a113b257`, `sane_open`'s `if (!be_name[0]) be = first_backend;`; `add_backend` puts each
    before the others). The check's probe: `:0` through a configuration naming `test` opened `test`'s device 0,
    through one naming `test` then `pnm` `pnm:0`. With `backend_of(":0")` `None`, the first decision 19 sent such
    an id to the system's configuration, where it opens a device of a backend the id never named. *Cause:* step
    1 read `sane_open`'s branch for a name without a `:` and not the one after it. *Fixed:* the note's paragraph
    gives the rule and says the default backend lives on there; decision 20 refuses an id whose name starts with
    `:` or is empty, beside the testing prefixes; security item 5 lists it.
  - *Decision 19 took `backend_of`'s `None` for an alias,* but an alias may hold a colon: `alias office:scan
    test:0` lists `office:scan`, `backend_of` `Some("office")` (the check's probe), so decision 18's worker got a
    directory naming `office`, no backend, and, `Isolated`, no aliases: `INVAL` (the check's probe through a
    directory naming `test`, the repair's through one naming `office`), and `Gone` though the device is there.
    *Cause:* the alias probe tried one alias, without a colon. *Fixed:* decisions 18 and 19 revised, for review:
    the worker's directory is written `Search::BeforeSystem` and names `backend_of`'s backend or none; SANE takes
    each file from the first of its directories that holds one, so that directory's `dll.conf` and empty `dll.d`
    decide what lists while the system's `dll.aliases` still resolves an alias at open, whatever its name; no
    worker opening a stored id runs with the system's `dll.conf`, and an alias whose name names no backend has no
    fallback by construction. The note's section 1 model paragraph says so; security item 5 says an alias opens on
    the backend the system's alias names, which the listing trusts already.
  - *The repair's probe* (`scratch/work/s0-2/repair-1/`, `run.sh` and `out/`; `seam-probe` as built for N1, each
    case a process in the memory scope under `timeout`; a one-backend directory written by `sane-dl` placed
    before a private one standing for the system's, `SANE_CONFIG_DIR="<one>:<system>"`, as `BeforeSystem` places
    it before `/etc/sane.d`, which isn't read; the stand-in naming `test` and `pnm` and holding `alias
    office:scan test:0` and `alias "Desk scanner" test:1`): through a directory naming `office`, `office:scan`
    opened `test`'s device 0 (`dll`'s log adding backend `test`, `test`'s `sane_open: devicename = "0"`, GOOD)
    and the listing found 0 devices (`dll` finding no backend `office`), while through that directory alone
    (`Isolated`) the open answered `INVAL`; through a directory naming none, `Desk scanner` opened `test`'s device
    1 and the listing found 0; through one naming `test`, the listing gave `office:scan` and `Desk scanner`, the
    system's aliases applied, and `test:9` answered `INVAL` from `test`; through the stand-in alone, `:0` opened
    `pnm`'s device 0 and the empty name and `:` `pnm`'s first (`devicename = ""`); `:0` through a directory naming
    `test` opened `test`'s device 0, and `:0` and the empty name through one naming none answered `INVAL`, `dll`
    finding no backend `''`. rc 0 each, no timeout.
  - *The minor wordings, while there:* `avision`'s 97 USB lookups are of 94 ids (some twice; N1's log);
    `fakeusb:<file>` and `fakeusbdev:<file>` take the rest of the name as the file, `fakeusbout:<file>:<name>` a
    name after it (`dll.c`); the `plustek` row says what of its naming isn't read (`plustek-usb.c`); `pixma`'s
    broadcast goes out on each IPv4 and IPv6 interface, loopback skipped (step-1's BJNP log), in the note and
    security item 4; section 1's lock is every call's but `sane_cancel`'s, as section 8 and `sane-dl`'s README say;
    the design's list of the review's stale names gives each `libusb:001:004` as the review's run opened it.
  - *Checks run again,* on the amended commit: the repair's probe above, and the check's `:0` results reproduced
    by it; every changed line traced to `dll.c`, a probe's output or the README; the trial merge against `0f08bb0`
    and `6ca9a2d`; the TWAIN lines against `4624760`; the diff and message scanned. Left out: `just ci` and the
    test list (nothing outside `docs/pm/` changed since `4624760`; the check's list stands), the other probes
    (no claim of theirs changed).

### Step 2: the model fixed for slice 3, and what follows (2026-11-09)

One commit, the one that carries this account. Docs only: `crates/scan-device-seam.md`,
`crates/scan-device.md`, `crates/sane-dl.md`, `milestones/s0-devices.md` and this plan.

- **What it does:** in the note, the raw layout's table (section 3; its TWAIN cells read from slice 7's
  record, the comparable form named as `twain-spike`'s `image` module, without a path) with probe L2's evidence
  and `RawImage::new`'s checks; the new section "The model, for slice 3": the types and traits as a Rust
  block (decision 21), what each claims (ids and opening, `RawImage`, a batch, cancel and progress, the
  settings window), a table of what the `sane` backend maps from `sane-dl` (slice 5), what slice 4 needs of it
  (plain data, sizes before bytes, whole pages, `u32` sizes, `Stopped` the client's, and the protocol's
  consequences: `open` with `renamed_from`, `applied`, progress lines, `cancel` on the batch's tracker, the
  one-backend `ConfigDir`, `Search::BeforeSystem`, at the spawn), and what slice 9's `twain` backend meets
  (decision 24); every model paragraph the new section contradicted, sections 1 to 8 (section 1's id and
  `renamed_from`; section 2's `Settings` with `duplex` and `format`, `SourceCaps`, `Resolutions`, `SizeMm`, its
  "To settle"'s resolutions answered; section 3's `RawImage` and progress; section 4's `side_known` and
  `first_sheet`; section 5's `Done::Cancelled` after the sheet and SANE's `NoDocs`; section 6's
  `NotInstalled`, a table row and the model, and warming up as `Event::Status`; section 7's model, its "To
  settle"'s question made the sheet's, and its `in-progress` paragraph made the model's; section 8's model
  sentences); "Changes from proposal 9's model" (items 2 and 6 revised, 8 to 11 added); the unknowns (the raw
  layout settled, the `in-progress` row taken for review, a row for the back after a cancel, slices 6 and
  12, at the table's end); the revision paragraph, saying each change to what both backends share. Then
  `crates/scan-device.md` (types, the two traits, `in-progress` and `thiserror` as dependencies, the `fake`
  scripts, the `sane` bullet's ids and fallback, conversions and cancel, "The seam"'s eleven changes),
  `crates/sane-dl.md`'s "For the seam" (what the seam takes of it, as built), and the milestone's slices 3 to
  5 (slice 3's model, `in-progress` and tests; slice 4's protocol and the one-backend `ConfigDir` at the spawn;
  slice 5's checks).
- **Probe L2** (`scratch/work/s0-2/probes/`, `./run.sh L2`, its `README.md`): `seam-probe` gained a `layout`
  scenario and the `png` crate (0.18.1, MIT OR Apache-2.0, the registry's copy, `--offline`; the probe's
  alone). `test:0` at 50 dpi, 157 by 196, each format a process of its own beside `scanimage --format=png` of
  the same settings: gray 1 (`Color pattern` and `Grid`), gray 8 and 16 and color 8 and 16 each `pixel for
  pixel: 0 samples or pixels differ`, the model's data `height` × the row's bytes exactly; 16-bit compared after
  the PNG's big-endian swap, the bytes unequal unswapped (29,223 of 30,772 gray samples with two different
  bytes); at 1 bit the PNG's padding bits masked (set in 96 rows of 196 on `Color pattern`, 166 to 186 on
  `Grid` across runs, so not steady) and the model's 0 in every row, SANE's bits uninverted differing in all
  30,772 pixels (the control), and `scanimage`'s TIFF of the same page photometric 0. Every run rc 0, first
  try.
- **Probe M1** (`scratch/work/s0-2/probes/model-sketch/`, its `README.md` section): the note's block as
  `rustfmt` formats it, `RawImage`'s methods, and 13 tests, built `--offline` against `in-progress` 0.1.0 and
  `thiserror` 2: the traits `Send` and dyn-compatible, a boxed device moved to the thread that scans,
  `renamed_from`, a pull backend cancelled from another thread as sheet 2's front begins delivering its back
  and no sheet 3, a push backend's spawned thread with a `Sender` clone and `progress.child(..)` seeing a
  cancel from a third thread at its next `check()`, a resident push thread taking each batch's clone and
  child over a channel, and `RawImage::new`'s checks (an empty image, a wrong length, a set padding bit,
  `u32::MAX` by `u32::MAX` at two bytes a pixel or more `TooLarge`, 65,536 by 65,536 `Rgb16` with no data,
  which a wrapping 32-bit `usize` would take). All pass; clippy `-D warnings` clean. The layout tests also ran
  on `wasm32-wasip1` (a 32-bit `usize`, through Node's WASI), taking their 32-bit branches: 7 passed. Every
  test made to fail on a broken copy (`scratch/work/s0-2/step-2/make-fail.md`: eight cases), and
  `check-block.sh` (the note's block, formatted, against the sketch's: equal) made to fail on a changed copy of
  the note.
- **`in-progress` 0.1.0, re-read from the registry's copy:** as "Dependencies, vetted" has it (MIT OR
  Apache-2.0, edition 2024, `rust-version` 1.85, no dependencies, dev `rayon` and `loom` under `cfg(loom)`,
  one source file, no `unsafe` in it or its examples, `Tracker` `Clone` and `Send + Sync`, a cooperative
  cancel: "Nothing is interrupted"), its trait also holding `message` and `is_cancelled`; packaged from
  commit `3949d79`.
- **Departures from the plan, and why:**
  - *The model is a Rust block in the note,* not prose alone: M1 compiles it and `check-block.sh` holds the
    two together (decision 21).
  - *The model section also maps slices 5 and 9,* beside slice 4's needs, so the review reads each later slice
    against the types (decision 24).
  - *The `u64` overflow is narrowed:* at `u32::MAX` by `u32::MAX`, `Bw1` and `Gray8` fit `u64` (2^64 − 2^33 + 1
    bytes at most), and the length refuses them; the design's sentence says so now.
  - *L2 adds a second 1-bit picture* (`Grid`, both bit values in quantity) and the uninverted control; *M1
    adds a resident push thread* (a TWAIN thread per open source) and a 32-bit run, neither planned.
  - *Section 6's table gains a `NotInstalled` row,* its TWAIN cell the DSM's file as `twain-dsm` loads it
    (slice 8), WIA's and ICA's to confirm: a new row, no slice 7 row changed.
  - *Section 7's "To settle"* (the first version's line) asks of the sheet now; slice 7's TWAIN text after it
    is untouched.
- **Decisions taken alone:** 21 to 24 above.
- **`rg` over the note for the old model's words,** each hit revised or kept on purpose: `Backend::scan`,
  "progress event", "separate handle", "not surfaced" and "(proposed" are gone; `mode` is SANE's option
  (section 2's SANE row and the model's mapping) or `sane_set_io_mode`'s (section 3's SANE row); "page in
  flight" is history (the revision paragraph's unknown, proposal 10's words as quoted, "Changes" item 6),
  SANE's facts at the page (section 7's SANE row, the answer's heading and body, the unknowns' SANE row),
  slice 7's TWAIN lines (section 7's "To settle", the unknowns' TWAIN row), or an error's page (section 6);
  "proposed" is the `in-progress` paragraph's history.
- **The TWAIN rows:** every line of the note at `4624760` that slice 7 wrote (`git blame`) is in the note
  verbatim but the planned shared ones, section 7's `in-progress` paragraph and the unknowns' `in-progress`
  row, both said in the revision paragraph; section 8's lines from "**TWAIN, by the spike:**" on are as they
  were, the line that holds it untouched, the model's sentences before it rewritten in their own lines.
- **Quick checks here:** `cargo fmt --all -- --check` and `cargo clippy --locked --workspace --all-targets --
  -D warnings` green (no crate touched); `git diff --stat` touching `docs/pm/` alone; the trial merge of a
  commit of this tree with `origin/s0-8-twain-dsm` at `0f08bb0` and with `6ca9a2d`; every line of the edited
  files at most 115 characters outside tables; the diff and this message scanned for local paths, the
  username, the maintainer's name, sheaf, `scratch/` and pdf-lint's plan outside `docs/pm/`. The test list,
  L2 and M1 run again and the other checks of the core are the checking agent's; `just ci` isn't run (no
  file outside `docs/pm/` changed; Baselines).
- **Its check** (at `0175f81`; record in `scratch/work/s0-2/flow/checks.md`, "Step 2 at 0175f81"): the test
  list 245 tests, the same names as step 1's; nothing outside `docs/pm/` changed; `just ci` green once (34 s,
  `SANE_DL_REQUIRE_LIBSANE=1` and `SANE_DL_REQUIRE_HEADER=1`, no test skipped); the trial merge against
  `0f08bb0` and `6ca9a2d` conflicting in the six known files alone; the message and added lines clean; every
  cited path found and the facts traced; L2 run again, its six cases equal pixel for pixel; M1 built fresh, its
  tests passing here, on `wasm32-wasip1` and on 1.89, `check-block.sh` equal and failing on a changed copy;
  `in-progress` as vetted; slice 7's lines verbatim but the ten planned shared ones; slices 3, 4 and 9 read
  against the types and holding. Two failures, docs only, and seven minor points for step 3 and the code
  review.
- **Repaired after its check** (amending this step's commit, docs only):
  - *`StartInvalid` was mapped two ways.* Section 6's table put `INVAL` from `sane_start` under `Driver` (the
    row's SANE cell, step 1's), while the model section's mapping (step 2) sets the settings again, starts once
    more and says `Gone` if it repeats, as `crates/scan-device.md`'s `sane` bullet has it, so slice 5 had two
    answers. *Cause:* step 2 wrote the mapping from the spec and left step 1's cell as it was. *Fixed:* one
    answer, the spec's: the `Gone` row's SANE cell gains `INVAL` from `sane_start` twice running and the
    `Driver` row's loses it; the mapping row says a re-read that failed (`reload`) is mapped as its own error
    is; `crates/scan-device.md`'s bullet says set again, start once more, `Gone`; the milestone's slice 5
    checks it through the fake's `at PAGE start INVAL`; decision 24 says so, with the alternative, for review.
  - *A refused id had no error to carry it.* Decision 20 gave a malformed id "its own error, not `Gone`", but
    `ScanError` had no variant for it, and no row covered it, `NulInName` or a character past U+00FF.
    *Fixed:* `ScanError::BadId`, a unit variant (decision 25, for review; security item 2), in the note's
    block, section 1's paragraph, section 6's table (a new row at its end, its TWAIN cell slice 9's to say, WIA's
    and ICA's to confirm: no slice 7 row changed) and its model paragraph, the ids bullet, the mapping table and
    "Changes" item 10 (still eleven); in `crates/scan-device.md`'s `ScanError` and its `sane` bullet; in the
    milestone's slice 5; and in this plan's design. Probe M1's sketch gained the variant and a fourteenth test, a
    backend refusing an id it doesn't form with `BadId`, apart from `Gone`.
  - *Section 3 gave a wrong reason for 1-bit RGB's having no third party:* that `scanimage` writes it under an
    8-bit PNM header. The installed `scanimage` aborts on it in PNG, TIFF and PNM alike (`scan_it: Assertion
    '(parm.depth == 8) || (parm.depth == 16)' failed`, rc 134, no byte written: the check's run, and the
    repair's, `scratch/work/s0-2/repair-2/rgb1/`), and master `a113b257`'s `frontend/scanimage.c` asserts it in
    `scan_it`'s RGB case, before its PNM writer, whose P6 header with a maxval of 255 the claim was read from, is
    reached. *Cause:* the plan read the header's writer, not the assertion before it. *Fixed:* the note's
    parenthesis, and the plan's L2 design text, say `scanimage` refuses it; the conclusion stands.
  - *Checks run again,* on the amended commit: `rg` for `StartInvalid`, `INVAL` from a start and `BadId`
    across `docs/pm/`, each saying one thing; the 1-bit RGB `scanimage` runs, reproduced; M1 with the new
    variant, its 14 tests passing, clippy `-D warnings` clean, `check-block.sh` equal, and made to fail twice
    (the new test on a copy whose backend answers `Gone`; the block check against `0175f81`'s note, which
    lacks the variant); section 6's TWAIN, WIA and ICA cells equal to `0175f81`'s, and the rows touched
    blamed to the first version and step 1, none to slice 7; the trial merge against `0f08bb0` and `6ca9a2d`;
    lines at most 115 characters outside tables; the diff and the message scanned. Left out: `just ci` and the
    test list (nothing outside `docs/pm/` changed since the check's green run), L2 and the other probes (no
    claim of theirs changed), M1's 32-bit and 1.89 runs (no layout test or dependency changed).
  - *Left for step 3 and the code review,* as the check put them: the `NotInstalled` TWAIN cell and the app's
    own DSM, the stored-id worker's current directory under `BeforeSystem`, the 10 s kill after a cancel against
    the sheet in flight, a `serde` `RawImage` going through `new`, the settings window's protocol request, a
    clause on the `test` backend's varying padding bits, and the `image` module's name after slice 8's merge.

### Step 3: the backlog and the plan's files; the slice's end (2026-11-09)

One commit, the one that carries this account. Docs only: the backlog (`README.md`, `tests.md`,
`questions.md`, `features.md`, `upstream.md`), `crates/scan-device-seam.md`, `milestones/s0-devices.md`,
`status.md`, `slices/README.md` and this plan.

- **What it does:** the backlog's items, one line each in the README's format, the next free IDs on this
  branch: TEST-9 (slice 6: the fi-6130's name through `fujitsu` and what replugging, another port and a reboot
  do to it, what its open answers once gone and once back, whether the fallback finds it; a sheet pulled in
  before the page in flight ends, and `sane_cancel` on a started batch; the back after a cancel after a front;
  its vendor, model and kind byte for byte); TEST-10 (slice 5: the stored-id worker's `BeforeSystem`
  configuration tested through a stand-in for the system's directory, never `/etc/sane.d`, its aliases with and
  without a colon); Q-4 (slice 5: a device's vendor, model and kind as bytes or lossy UTF-8, N1's lost `\xe9`);
  FEAT-12 (slice 5: decision 20's refusals as `BadId`, decision 25); UP-12 (`scanimage` aborting on one-pass
  1-bit color, the assertion step 2's check found, beside UP-10's) and UP-13 (sane-dll(5)'s name without a `:`,
  which `dll.c` reads as a backend's, step 1's finding). The README's table counted again with `rg` (45 items:
  15 for S0, 15 for S1, 7 for S2, 8 later; 12 waiting on review) and its "Where to start" given slice 2's. The
  milestone's slice 2 entry: done on the branch, waiting on review, its decisions and security items counted.
  `status.md` and `slices/README.md` a line each (`status.md` also its date, its next slice, and UP-7 to UP-13).
- **Step 2's check's minor points, taken here** (`scratch/work/s0-2/flow/checks.md`, "Step 2 at 0175f81",
  "Minor"), six of the seven, prose only (the model's block untouched): in the note's "What slice 4 needs", a
  `RawImage` decoded on the client made by `RawImage::new`, the kill after a cancel allowing for the sheet in
  flight, a `settings_window` request, and the stored-id worker in a directory it trusts (decision 26, security
  item 6), each also a clause in the milestone's slice 4; section 6's `NotInstalled` TWAIN cell, the DSM neither
  the system's nor the app's own copy (`crates/twain-dsm.md`'s loading); section 3's SANE row, the `test`
  backend's `Grid` padding bits not the same from run to run (`scanimage`'s PBMs of it at 01:55 and 02:25, their
  pixels equal, differing in 191 rows' padding of 196, three at 01:55 equal among themselves and two at 02:25:
  `scratch/work/s0-2/step-3/grid-padding.txt`). The seventh, the `image` module's name beside slice 8's after
  its merge, is the merge's to align: the note names it as it is at `4624760`, a record.
- **Departures from the plan, and why:** TEST-10, UP-12 and UP-13 beyond the plan's list, as "whatever the
  probes find" (step 1's repair probe, step 2's check of `scanimage`, step 1's reading of `dll.c` beside the man
  page); the `test` backend's varying padding bits got no upstream item, since the standard says nothing of
  the bits past a row's last pixel (its "padded" formats are whole bytes past `bytes_per_line`'s minimum). The
  note and the milestone's slice 4 edited for the check's points, which the plan's step 3 didn't list; the
  check's notes left them to this step or the code review, and the code review then reads them in place.
- **Decisions taken alone:** 26 above; security item 6.
- **Found, for the code review** (not changed here, since it changes the model's block and M1's sketch):
  `Backend::open` takes the stored id alone (`&DeviceId`), but SANE's fallback opens "the one device of the same
  backend, vendor and model", and a SANE id holds the backend and the name, neither vendor nor model
  (`sane:fujitsu:libusb:001:004`), so the `sane` backend can't make the match as the trait stands. M1 didn't
  show it, its fake renaming by id alone. Either `open` takes what the app stored of the device (its
  `DeviceInfo`, or the id with its maker and model), and slice 4's `open` request carries the same, or the
  fallback goes. For the api lens.
- **Checks before it:** step 2's check and its repair are in step 2's account above; no check ran on the
  repair's commit before this step, so nothing more to carry. CI on pull request #2 (`4624760`'s code tree, this
  branch's too) is green on every job (run 42582929367, the Windows jobs on a rerun after GitHub's download
  timeouts; its seven jobs' results read with `gh`, `scratch/work/s0-2/step-3/ci-run-42582929367.json`), which
  closes TEST-2 and TEST-8; moving them to `archive/backlog-done.md` is the fix stage's.
- **Quick checks here:** `git diff --stat 4624760` touching `docs/pm/` alone; the trial merge of a commit of
  this tree with `origin/s0-8-twain-dsm` at `0f08bb0` and at the remote's tip `6ca9a2d` (unchanged since step 2)
  conflicting in the six files of `4624760` alone, the seam note and the milestone merging cleanly, each edit
  above in the merged note; every commit message of `4624760..HEAD` read for names, local paths, `scratch/`,
  sheaf and pdf-lint (none); `git grep -i` for the maintainer's name finding only the root `LICENSE-MIT`; every
  backlog line matched against the README's format, the new IDs used nowhere else; the paths the note cites
  found (sane-backends' by their path at `a113b257`); lines at most 115 characters outside tables and backlog
  lines; the diff and this message scanned. The test list, the core's tracing of each fact and the rest are the
  checking agent's; `just ci` isn't run (no file outside `docs/pm/` changed; Baselines). **What only CI shows:**
  nothing of this slice's (docs only).
