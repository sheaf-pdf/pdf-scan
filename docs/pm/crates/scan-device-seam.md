# The seam: how each driver API meets `scan-device`'s model

`crates/scan-device.md`'s design note. For each concept of the device model: how SANE, TWAIN, WIA 2.0 and
ImageCaptureCore (ICA) deliver it, what the model makes of it, and what's still to settle. Paths are from
`docs/pm/`.

**Status: first version, 2026-11-08, written before any driver layer exists** (asked for ahead of the
device slices). It's drawn from M5 proposals 4 to 10 (`scratch/reference/sheaf/m05-scan/proposals/`), sheaf's
`research/scanners.md`, the survey's notes (`research/prior-art/notes/sane-and-network-scanning.md`,
`twain-wia-ica.md`), `sane.h` (sane-backends' master) and `twain.h` 2.5 (twain-dsm), the last two read for the
constants named here. ICA's names are Apple's, unchecked here: each is marked "to confirm". It's revised in S0
slice 2 (after `sane-dl` is written), after the TWAIN spike (S0 slice 7), and whenever a backend finds the
model lacking, each change said. The model it describes is provisional until S0 slice 11 freezes it
(`milestones/s0-devices.md`).

## The rules it keeps

- **The bindings say what their API says; the backends decide what it means.** `sane-dl` and `twain-dsm` expose
  SANE's and TWAIN's facts unchanged (frame parameters, image info, statuses, condition codes); WIA and ICA
  come through the `windows` and objc2 crates as they are. Each `scan-device` backend maps them onto the model.
  Nothing in the model is shaped to suit one API's quirks.
- **The model holds what the app needs, not what every API can say.** Where an API can't give something (a
  page's side, a feeder's state), the backend infers it and says that it did, or the model has it as unknown.
- **One shape for every backend:** synchronous, on a thread of its own, events on a channel (M5 proposal 9).
  Pull APIs (SANE) and push APIs (TWAIN, WIA, ICA) are both turned into that by their backend.

## 1. Finding devices, and their ids

| API | How |
|---|---|
| SANE | `sane_get_devices` (optionally local only) gives each device's `name`, `vendor`, `model`, `type`. Slow (10.7 s here with nothing attached). No attach or detach events. The name is the backend's: fujitsu's carries the model and a serial (`fujitsu:fi-6130dj:…`), others a USB bus address that changes on replugging (to confirm per backend in S0 slice 1). |
| TWAIN | The DSM lists sources (`DAT_IDENTITY`, `MSG_GETFIRST`/`MSG_GETNEXT`) as `TW_IDENTITY`s: product name, manufacturer, product family, version. Fast and local, but per bitness: a 64-bit DSM lists 64-bit sources only. A source is opened by its identity. No serial. `MSG_DEVICEEVENT` reports events of an open source, not attach and detach. |
| WIA | `IWiaDevMgr2::EnumDeviceInfo`: a device id string, name, manufacturer. `RegisterEventCallbackInterface` can report connect and disconnect. |
| ICA | `ICDeviceBrowser` and its delegate: devices added and removed as they come (`didAddDevice`, `didRemoveDevice`), asynchronously; each with a persistent id (to confirm). |

**The model:** `DeviceInfo { id, name, maker, model, driver }` (proposal 9). The id is opaque, namespaced by
backend and, for TWAIN, bitness: `sane:<name>`, `twain32:<product name>`, `twain64:<product name>`,
`wia:<device id>`, `ica:<persistent id>`. Listing is a snapshot under a deadline, in a short-lived worker of
its own; no hot-plug events in the model, since SANE and TWAIN have none (proposal 10). Opening the last
device goes straight to it by id, without a list: SANE's `sane_open(name)`, TWAIN's identity found by a quick
DSM walk, WIA's `CreateDevice(id)`; ICA has to browse for it, under the same deadline.

**To settle:** when a SANE name has changed (replugged), whether to fall back to a device of the same vendor and
model; how TWAIN's two lists merge (`scan-worker`'s client, leaning); ICA's persistent id.

## 2. What a device can do, and settings

| API | How |
|---|---|
| SANE | Options with descriptors and constraints (ranges, word lists, string lists), by standard names: `source`, `mode`, `resolution`, `tl-x`…`br-y`, and backends' `page-width`/`page-height`. Values vary by backend (`Color`, `Gray`, `Lineart`, `Halftone`…; feeder and duplex by many names). Setting one can change others: `SANE_INFO_RELOAD_OPTIONS`; a value adjusted to fit says `SANE_INFO_INEXACT`. |
| TWAIN | Capabilities, each with a current value, a default and what's allowed (one value, an enumeration, a range): `ICAP_XRESOLUTION`, `ICAP_PIXELTYPE`, `ICAP_BITDEPTH`, `CAP_FEEDERENABLED`, `CAP_DUPLEXENABLED`, `ICAP_SUPPORTEDSIZES`, `ICAP_FRAMES`. A set the source adjusted answers `TWRC_CHECKSTATUS`; read it back. |
| WIA | Properties on items, with their valid values (a range, a list, flags): the root's children are the sources (`Flatbed`, `Feeder`); resolution, data type, depth and page size on each; duplex on the Feeder item (`DUPLEX` or `FRONT_ONLY`). |
| ICA | Functional units (flatbed, document feeder), each with its supported resolutions, bit depths, pixel data types and scan area; the feeder says whether it supports duplex (to confirm). |

**The model:** `Settings { source, dpi, mode, paper }` (proposal 9), clamped to what the device can do, and
what was clamped said. **One change from proposal 9: capabilities per source.** In all four APIs what's
allowed depends on the source chosen (SANE's options change with `source`; TWAIN's sizes with
`CAP_FEEDERENABLED`; WIA's and ICA's live on the source's own item or unit), so `Caps` becomes a list of
sources, each with its own resolutions, modes and largest area, and whether a feeder has duplex. Clamping is
found the same way everywhere: set, then read back (SANE's `INEXACT`, TWAIN's `CHECKSTATUS`, WIA's and ICA's
values read again). Modes are mapped by each backend's table (`Color`, `Gray`, `Bw` from whatever the
backend calls them), as the feeder's names are.

**Asking drivers not to drop pages.** Some drivers can discard blank pages themselves (TWAIN's
`ICAP_AUTODISCARDBLANKPAGES`, PaperStream's own processing). That breaks every inference of a page's side
(section 4), and `scan-cleanup` finds blank pages anyway, so backends turn it off where the API lets them.

**To settle:** whether resolutions are listed or a range in the model (SANE and TWAIN give either); paper
sizes against a feeder's own size detection (`Auto`, proposal 9) in each API.

## 3. How a page arrives

| API | How |
|---|---|
| SANE | Per page, pulled: `sane_start`, then `sane_get_parameters` (format `GRAY`, `RGB`, or one of `RED`/`GREEN`/`BLUE`; `last_frame`; bytes and pixels per line; lines, or -1 for unknown; depth 1, 8 or 16), then `sane_read` in chunks to `EOF`. If not `last_frame`, `sane_start` again for the next frame (three-pass color). Raw rows, top down; 1-bit with 1 black and the first pixel in the high bit; 16-bit in the host's byte order. SANE 1 has only these five frame types: JPEG, CCITT and infrared frames are reserved for a later SANE under `#if 0`. Blocking, or non-blocking with a select fd. |
| TWAIN | Per image, pushed then pulled: the source posts `MSG_XFERREADY` (state 6); `DAT_IMAGEINFO` gives width, height, resolution, pixel type, bits per pixel and compression; then a transfer (state 7): **native** (on Windows a handle to a DIB: a header, a palette, rows bottom up, each padded to four bytes), **memory** (buffers of rows or tiles with `XOffset`, `YOffset` and bytes per row, compressed if `ICAP_COMPRESSION` asked for G4 or JPEG), **file** or **memory file** (the driver writes a file format). After it, `DAT_EXTIMAGEINFO` (section 4), then `DAT_PENDINGXFERS`/`MSG_ENDXFER`, which says how many more are pending. |
| WIA | Pushed: `IWiaTransfer::Download` calls back `GetNextStream` for each page (we hand it a stream) and `TransferCallback` with progress (`ulTransferredBytes`; not the percentage, which peaks near 40 then jumps), end of stream and end of transfer. The page's format is the driver's choice among what it lists (`EnumWIA_FORMAT_INFO`): a bitmap, WIA's raw format, PNG, JPEG or TIFF. |
| ICA | Pushed, through delegates: memory-based transfers give bands (`ICScannerBandData`: the full image's size, bits per pixel and per component, rows from a start row, bytes per row, a buffer); file-based ones a file's URL (to confirm). |

**The model:** `RawPage { pixels, dpi, sheet, side }` (proposal 9), with `pixels` one of two:

- **raw**: one layout for every backend: rows top down, a stated stride, a format (1-bit, 8-bit gray, 16-bit
  gray, 8-bit RGB, 16-bit RGB), 1-bit packed with the first pixel in the high bit and a stated polarity,
  16-bit in a stated byte order. Each backend assembles its API's pieces into it: SANE's three passes and
  padded rows, TWAIN's tiles and its DIB's bottom-up rows and palette, ICA's bands, WIA's raw header.
- **encoded**: bytes as the driver gave them, with their kind (JPEG, PNG, TIFF, a bitmap, CCITT G4 with its
  parameters), when the driver gives an encoded form and the settings ask for it. `scan-image` keeps a JPEG
  as its bytes, so passing one through avoids a decode and a re-encode.

**A page is delivered whole**, once complete; a 600 dpi color Letter page is about 100 MB raw, and one at a
time is fine. Progress is a separate event (bytes of the page in flight, against its size where known), as
proposal 10's progress bar wants. A page cut short (a jam mid-page) is never delivered.

**To settle:** the 1-bit polarity and 16-bit byte order the raw layout fixes (proposed: 1 black, as SANE has
it, and 16-bit little-endian, both said in the type); whether to ask TWAIN and WIA for compressed transfers
(G4 for black and white, JPEG for color) to cut the pipe's bytes against the 58 MB/s the throughput gate needs
(`crates/scan-worker.md`), or stay raw in the first version; whether `twain-dsm` hands over assembled images
or tiles (leaning: tiles, assembled by the backend, so `twain-dsm` stays TWAIN alone); what a native DIB's
palette becomes.

## 4. Sheets, sides and order

| API | How |
|---|---|
| SANE | Nothing per frame. A duplex feeder gives front, back, front, back; every frontend counts sheets by pairs. |
| TWAIN | `DAT_EXTIMAGEINFO` after each transfer can give `TWEI_PAGESIDE` (`TWCS_TOP` or `TWCS_BOTTOM`), `TWEI_DOCUMENTNUMBER` and `TWEI_PAGENUMBER`, where the source supports them (`ICAP_SUPPORTEDEXTIMAGEINFO` lists which); otherwise the order, front first. |
| WIA | Order: two pages a sheet in duplex, front first unless `BACK_FIRST` is set; nothing per page that names the side (to confirm). |
| ICA | The feeder unit's odd and even page orientation and a reverse page order setting (to confirm); nothing per band that names the side (to confirm). |

**The model:** `sheet` counted from 1 across a batch (and on through "Scan More", proposal 10); `side`
`Front` or `Back`, and whether it was **reported** by the driver or **inferred** from the order. Inference is
right only while no page is dropped, hence asking drivers not to drop blanks (section 2). The intake can pair
and turn backs (proposal 10's "Scan backs") knowing which it has.

**To settle:** which drivers fill `TWEI_PAGESIDE` (the fi-6130's FUJITSU TWAIN32 and PaperStream IP, by hand
in S0 slice 12); whether WIA or ICA says the side anywhere.

## 5. The end of a batch

| API | How |
|---|---|
| SANE | `NO_DOCS` from `sane_start`: on the first page the feeder is empty, later it's done. Some backends return it from `sane_read`. A flatbed: one page per press (some give the same page forever). |
| TWAIN | `CAP_FEEDERLOADED` false before enabling: empty. `TWCC_NOMEDIA`. With `CAP_XFERCOUNT = -1` the source scans until empty; `MSG_ENDXFER`'s pending count reaching 0 is done (state 5). |
| WIA | `WIA_ERROR_PAPER_EMPTY` at the start: empty. `WIA_STATUS_END_OF_MEDIA` (0x210001), or the end of the transfer: done. `FEED_READY` is a hint, wrong either way at times. A page count reached while the feeder runs on: cancel and `WIA_CMD_STOP_FEEDER`. |
| ICA | The feeder unit's "document loaded" (to confirm); the scan's completion delegate without an error: done. |

**The model:** an empty feeder before any page is the error `ScanError::FeederEmpty` (nothing was scanned);
an empty feeder after pages is `Done::FeederEmpty`; a flatbed's page is `Done::OnePage`; a cancel is
`Done::Cancelled`. Proposal 9 has `FeederEmpty` in both; this is the line between them. "Scan More" is a new
batch that carries the sheet count on.

## 6. Errors

| Model | SANE | TWAIN | WIA | ICA |
|---|---|---|---|---|
| `Jammed` | `JAMMED` | `TWCC_PAPERJAM` | `WIA_ERROR_PAPER_JAM` | an error from the delegate (to confirm) |
| `DoubleFeed` | none: backends say a jam or an I/O error | `TWCC_PAPERDOUBLEFEED` | `WIA_ERROR_MULTI_FEED` | to confirm |
| `CoverOpen` | `COVER_OPEN` | `TWCC_INTERLOCK` | `WIA_ERROR_COVER_OPEN` (SDK value and Learn's misprint both) | to confirm |
| `Busy` | `DEVICE_BUSY` | `TWCC_MAXCONNECTIONS` | `WIA_ERROR_BUSY` | to confirm |
| `Gone` (unplugged, offline) | `IO_ERROR` after opening; `INVAL` at opening | `TWCC_CHECKDEVICEONLINE` (offline at state 3 or less, lost at 4 or more) | `WIA_ERROR_OFFLINE` | the device removed |
| `Driver { message, shown }` | other statuses | `TWCC_OPERATIONERROR` (the driver has already shown the user its message: `shown`), others | others, decided by code (`E_FAIL`'s text is localized) | others |

**The model:** proposal 9's `ScanError`, with two changes: `Driver` carries whether the driver has already
shown its own message, so the app doesn't say it twice; and warming up is handled inside the backends (wait
and retry: WIA's `WIA_ERROR_WARMING_UP`; SANE's warming-up status is reserved and never returned), not
surfaced. `Stopped` stays the watchdog's (proposal 4). Pages before an error are kept; the page in flight is
not.

**To settle:** what "Continue" after a jam is in each API (SANE: `sane_cancel`, then `sane_start` again;
TWAIN: whether the source stays enabled or needs `MSG_RESET` and enabling again, S0's TWAIN spike and the hand
check; WIA: a new `Download`); whether `Busy` and `Gone` need telling apart for the user beyond their words.

## 7. Cancel

| API | How |
|---|---|
| SANE | `sane_cancel` from any thread; the read in progress returns `CANCELLED`; no other call on the handle, `sane_close` included, until it has. |
| TWAIN | By state: in 5, wait for `MSG_XFERREADY` and end it there (forcing a step down in 5 draws errors from some scanners); in 6 and 7, `MSG_ENDXFER` and `MSG_RESET`. On the TWAIN thread, so a cancel is posted to its message loop. |
| WIA | `S_FALSE` from `TransferCallback`, on the STA thread, or `IWiaTransfer::Cancel`. |
| ICA | Cancel the scan on the device, on its run loop's thread (to confirm). |

**The model:** `cancel` means **feed no more and stop after the page in flight** (proposal 10: "stops after
the page in flight; pages so far stay"), through a handle any thread can call (the worker's protocol thread
calls it). Each backend does it its way: SANE by not calling `sane_start` again (and `sane_cancel` if a read is
stuck), TWAIN by `MSG_RESET` after the current transfer ends, WIA by refusing the next stream, ICA to confirm.
Stopping mid-page is the watchdog's: it kills the worker (proposal 4).

**To settle:** whether each API can finish the page in flight and feed no more, or only stop at once (S0's SANE
slices for SANE; the TWAIN spike for TWAIN; WIA and ICA by hand).

## 8. Threads and events

| API | How |
|---|---|
| SANE | Pull, on the caller's thread: one thread per handle, calls never overlapping; `sane_cancel` the one call allowed from elsewhere. |
| TWAIN | Push, through a window's message loop on one STA thread that makes every DSM call; `MSG_XFERREADY`, `MSG_CLOSEDSREQ` (the user closed the driver's window) and `MSG_DEVICEEVENT` arrive there. TWAIN 2's `DAT_CALLBACK2` delivers the source's messages by callback; whether that's enough on Windows without the loop is to confirm, and the plan keeps the window and its loop either way. |
| WIA | Push, on an STA thread: `Download` is one blocking call that calls back on the same thread. |
| ICA | Push, through delegates on a run loop: the main thread's by default; in a worker, the worker's main thread runs it (whether that's enough is open, the survey). |

**The model:** `Backend::scan(settings, events)` blocks its own thread until the batch is done, sending
`sheet`, `page`, `progress` and `status` events on the channel; `cancel` is a separate handle, `Send + Sync`.
The SANE backend runs on that thread directly; the TWAIN, WIA and ICA backends start the thread their API
needs (an STA thread with a message loop or a run loop), forward its events to the channel, and post
cancels and requests to it. `scan-worker` sees one shape for all of them.

## 9. The driver's own window

| API | How |
|---|---|
| SANE | None: every setting is an option (advanced ones included). |
| TWAIN | `ShowUI` with the scan; `CAP_ENABLEDSUIONLY` for its settings alone, where the driver has it (PaperStream's own processing, dropout color, double-feed sensitivity: proposal 7). |
| WIA | Windows' common dialogs, not the driver's own; not used. |
| ICA | None. |

**The model:** whether the source has its own settings window is a capability (TWAIN only, today); opening
it is a backend call, and its settings then apply to scans until changed (proposal 7).

**To settle:** which window owns the dialog: the worker's hidden window owns it, so it can open behind the app;
passing the app's window handle to the worker for the driver to use as its owner is the likely answer (S0 slice 9).

## 10. What drivers report beside a page

TWAIN's `TWEI_PATCHCODE` and barcode text through `DAT_EXTIMAGEINFO`, and WIA 2.0's patch code reader
(results as XML or raw); SANE has nothing; ICA, to confirm. **The model:** a list of what the driver reported
on the page, empty unless asked for; hints only, since `patch-codes` reads every edge and says which
(FEAT-5). Later; it doesn't change the rest.

## Changes from proposal 9's model

1. Capabilities per source (section 2).
2. `RawPage`'s pixels raw in one stated layout, or encoded with their kind; delivered whole; progress a
   separate event (section 3).
3. A page's side reported or inferred; drivers asked not to drop blank pages (sections 2 and 4).
4. `FeederEmpty` as an error before any page, a `Done` after (section 5).
5. `Driver` errors say whether the driver already showed its message; warming up stays inside the backends
   (section 6).
6. Cancel is "feed no more, finish the page in flight"; stopping at once is the watchdog's (section 7).
7. The driver's own window as a capability (section 9); driver-reported metadata, later (section 10).

## Known unknowns

Each section's "To settle", gathered, with what settles it:

| Unknown | Settled by |
|---|---|
| SANE names that change on replugging, per backend; a fallback by vendor and model | S0 slice 1, SANE's backends' sources and the office's scanners |
| Whether SANE can stop after the page in flight | S0 slice 1, the `test` backend |
| Whether `twain-dsm` hands over tiles or assembled images; what a DIB's palette becomes | S0 slice 7, the spike |
| How TWAIN's 32- and 64-bit lists merge (leaning: `scan-worker`'s client) | S0 slice 9 |
| How events cross from the TWAIN thread to the backend's; posting cancels to its loop | S0 slice 7, the spike |
| Which drivers fill `TWEI_PAGESIDE`; whether WIA or ICA say a page's side | S0 slice 12, the hand checks; the `ica` slice |
| "Continue" after a jam in TWAIN | S0 slices 7 and 12 |
| Compressed transfers (G4, JPEG) for the pipe's throughput, or raw only at first | S0 slice 4's throughput gate; slice 9 |
| The raw layout's 1-bit polarity and 16-bit byte order | S0 slice 3 |
| The driver window's owner across processes | S0 slice 9 |
| Every ICA name marked "to confirm"; its run loop in a worker | The `ica` slice (later) |
| Where driver-reported patch codes go (FEAT-5) | Later |
