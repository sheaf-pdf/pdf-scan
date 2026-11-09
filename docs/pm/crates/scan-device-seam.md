# The seam: how each driver API meets `scan-device`'s model

`crates/scan-device.md`'s design note. For each concept of the device model: how SANE, TWAIN, WIA 2.0 and
ImageCaptureCore (ICA) deliver it, what the model makes of it, and what's still to settle. Paths are from
`docs/pm/`.

**Status: first version, 2026-11-08, written before any driver layer exists** (Greg asked for it ahead of the
device slices). It's drawn from M5 proposals 4 to 10 (`scratch/reference/sheaf/m05-scan/proposals/`), sheaf's
`research/scanners.md`, the survey's notes (`research/prior-art/notes/sane-and-network-scanning.md`,
`twain-wia-ica.md`), `sane.h` (sane-backends' master) and `twain.h` 2.5 (twain-dsm), the last two read for the
constants named here. ICA's names are Apple's, unchecked here: each is marked "to confirm". It's revised in S0
slice 2 (after `sane-dl` is written), after the TWAIN spike (S0 slice 7), and whenever a backend finds the
model lacking, each change said. The model it describes is provisional until S0 slice 11 freezes it
(`milestones/s0-devices.md`).

**Revised 2026-11-08 by S0 slice 7, the TWAIN spike, before slice 2** (slice 7 ran first, on the Windows
machine): its TWAIN rows in sections 1 and 3 to 8 and its TWAIN unknowns, from `twain-spike`'s runs against
the TWAIN Working Group's sample data source in both bitnesses, here and on `windows-latest`
(`slices/s0-7-twain-spike.md`, "So far"). What it settled is settled **on the sample source**, which signals
synchronously, has one image, and can't jam, feed two sides or report extended image info; the hand checks
(S0 slice 12) settle the rest. SANE's rows are as they were, for slice 2.

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
| TWAIN | The DSM lists sources (`DAT_IDENTITY`, `MSG_GETFIRST`/`MSG_GETNEXT`) as `TW_IDENTITY`s: product name, manufacturer, product family, version. Local, but per bitness: a 64-bit DSM lists 64-bit sources only. The DSM loads every source's library at `MSG_OPENDSM`, before anything is listed, so opening the DSM runs each installed driver's code (its log says so), and its cost grows with the drivers installed. DSM 2.5 deliberately skips Windows' own `wiatwain.ds` (its log: "We're deliberately skipping this file"), so WIA devices aren't reached through TWAIN. A source is opened by its identity. No serial. `MSG_DEVICEEVENT` reports events of an open source, not attach and detach. |
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
| TWAIN | Per image, pushed then pulled: the source posts `MSG_XFERREADY` (state 6), once per enable: after `MSG_ENDXFER` with pages pending the application transfers again, with no second signal. `DAT_IMAGEINFO` gives width, height, resolution, pixel type, bits per pixel and compression; then a transfer (state 7): **native** (on Windows a `GlobalAlloc` handle to a packed DIB, the application's to free: a header, a palette, rows bottom up, each padded to four bytes, BGR), **memory** (buffers of rows or tiles with `XOffset`, `YOffset` and bytes per row, compressed if `ICAP_COMPRESSION` asked for G4 or JPEG; the sample sends strips of full rows, top to bottom, rows padded to four bytes, as many rows as fit the preferred buffer), **file** or **memory file** (the driver writes a file format). After it, `DAT_EXTIMAGEINFO` (section 4), then `DAT_PENDINGXFERS`/`MSG_ENDXFER`, which says how many more are pending. |
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

**Settled by the TWAIN spike:** `twain-dsm` hands over **tiles as TWAIN gives them** (each buffer's offsets,
columns, rows, bytes per row and bytes written) and **the DIB's bytes as a block**; the `twain` backend
assembles and decodes, checking each against its bounds before reading (`twain-spike`'s `image` module is the
shape: tiles in any order, overlaps and gaps refused, 1-bit tiles that start or end within a byte refused).
**A DIB's palette is the backend's to apply:** a 1-bit palette black then white (the sample's) or white then
black, read into the raw layout's polarity; an 8-bit gray palette read through (the sample's is an identity
ramp); a color palette on 8 bits refused until a driver sends one. A memory transfer's 1-bit and gray polarity
is `ICAP_PIXELFLAVOR`'s (`TWPF_CHOCOLATE`, 0 black, is all the sample offers). The sample's native and memory
pages matched pixel for pixel in RGB, gray and black and white once decoded so. Its `ICAP_BITORDER` swaps
color order the opposite way in the two paths, so the backend never sets it.

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
| TWAIN | `CAP_FEEDERLOADED` false before enabling: empty. `TWCC_NOMEDIA`. With `CAP_XFERCOUNT = -1` the source scans until empty; `MSG_ENDXFER`'s pending count reaching 0 is done (state 5). Until then the count is 0xFFFF, "don't know" (the sample's, after every page but the last), so a TWAIN batch's length is unknown until it ends. |
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
TWAIN: whether the source stays enabled or needs `MSG_RESET` and enabling again, the hand check, since the
sample source can't jam and the spike couldn't settle it; WIA: a new `Download`); whether `Busy` and `Gone` need
telling apart for the user beyond their words.

## 7. Cancel

| API | How |
|---|---|
| SANE | `sane_cancel` from any thread; the read in progress returns `CANCELLED`; no other call on the handle, `sane_close` included, until it has. |
| TWAIN | By state: in 5, wait for `MSG_XFERREADY` and end it there (forcing a step down in 5 draws errors from some scanners); in 6 and 7, finish the transfer in flight, `MSG_ENDXFER`, then `MSG_RESET` (state 5, nothing pending). `MSG_STOPFEEDER` exists for "feed no more", but the sample refuses it (`TWCC_BADPROTOCOL`). On the TWAIN thread, so a cancel is posted to its window and taken by its loop. |
| WIA | `S_FALSE` from `TransferCallback`, on the STA thread, or `IWiaTransfer::Cancel`. |
| ICA | Cancel the scan on the device, on its run loop's thread (to confirm). |

**The model:** `cancel` means **feed no more and stop after the page in flight** (proposal 10: "stops after
the page in flight; pages so far stay"), through a handle any thread can call (the worker's protocol thread
calls it). Each backend does it its way: SANE by not calling `sane_start` again (and `sane_cancel` if a read is
stuck), TWAIN by `MSG_RESET` after the current transfer ends, WIA by refusing the next stream, ICA to confirm.
Stopping mid-page is the watchdog's: it kills the worker (proposal 4).

**To settle:** whether each API can finish the page in flight and feed no more, or only stop at once (S0's SANE
slices for SANE; WIA and ICA by hand). **TWAIN, by the spike:** the page in flight finishes and nothing more is
transferred (`MSG_ENDXFER`, then `MSG_RESET`); whether the feeder stops is the driver's: the sample fed the next
sheet inside `MSG_ENDXFER`, which the application can't see, so the hand checks (slice 12) say what real
feeders do, and whether `MSG_STOPFEEDER` helps where a driver has it.

**Proposed, for slice 3 (for review; asked 2026-11-08 whether `in-progress` fits):** the cancel handle
and the progress events through `in-progress`, his crate, which `scan-cleanup` already takes. A `Tracker` is the
`Send + Sync` handle any thread can cancel; each backend calls `check()` between pages, never within one, which
is "finish the page in flight"; progress is `set_total(Some(page bytes), Unit::Bytes)` and `inc` per buffer;
and the intake pipeline could give scanning and cleanup weighted children of one tracker. Two seams: its
progress is polled, not pushed, so `scan-worker` polls the worker's tracker and forwards snapshots over its
pipe; and a flag can't wake the TWAIN thread blocked in `GetMessageW`, so the TWAIN backend posts a message to
its window as well.

## 8. Threads and events

| API | How |
|---|---|
| SANE | Pull, on the caller's thread: one thread per handle, calls never overlapping; `sane_cancel` the one call allowed from elsewhere. |
| TWAIN | Push, through a window's message loop on one STA thread that makes every DSM call; `MSG_XFERREADY`, `MSG_CLOSEDSREQ` (the user closed the driver's window) and `MSG_DEVICEEVENT` arrive there. Without a callback, every window message is forwarded through `MSG_PROCESSEVENT` while the source is enabled, and a waiting source message comes back with whichever is forwarded first (on a new window, the desktop window manager's `WM_DWMNCRENDERINGCHANGED`; later, the DSM's own `WM_NULL`), the carrier not to be dispatched. With TWAIN 2's `DAT_CALLBACK2`, registered on the source in state 4, the DSM calls the callback on the thread that called into it, inside that call (for the sample, inside `MSG_ENABLEDS`), and hands its return to the source, which fails the enable on anything but `TWRC_SUCCESS`: the callback only records and posts. Both worked with the sample, which signals synchronously; whether a callback is enough without the loop for sources that signal later, from their own windows or threads, is slice 12's, and the window and loop stay either way. The DSM changes the process's current directory to each source's folder while loading it, so the worker uses no relative paths. |
| WIA | Push, on an STA thread: `Download` is one blocking call that calls back on the same thread. |
| ICA | Push, through delegates on a run loop: the main thread's by default; in a worker, the worker's main thread runs it (whether that's enough is open, the survey). |

**The model:** `Backend::scan(settings, events)` blocks its own thread until the batch is done, sending
`sheet`, `page`, `progress` and `status` events on the channel; `cancel` is a separate handle, `Send + Sync`.
The SANE backend runs on that thread directly; the TWAIN, WIA and ICA backends start the thread their API
needs (an STA thread with a message loop or a run loop), forward its events to the channel, and post
cancels and requests to it. `scan-worker` sees one shape for all of them. **TWAIN, by the spike:** events go out
on a channel (`std::sync::mpsc`); requests come in as the application's own messages (`WM_APP` and up) posted to
the TWAIN thread's window, taken in the loop and never forwarded to the DSM; the window procedure is
`DefWindowProcW` itself, so no backend code runs in a callback frame (a panic there would abort the process),
and the callback holds only atomics. A cancel posted from another thread was taken 5 ms later.

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
| Whether `twain-dsm` hands over tiles or assembled images; what a DIB's palette becomes | **Settled, S0 slice 7:** tiles and the DIB's block; the backend assembles, decodes and applies the palette (section 3) |
| How TWAIN's 32- and 64-bit lists merge (leaning: `scan-worker`'s client) | S0 slice 9 |
| How events cross from the TWAIN thread to the backend's; posting cancels to its loop | **Settled, S0 slice 7:** a channel out; the application's own messages posted to the window, never forwarded (section 8) |
| Whether TWAIN can finish the page in flight and feed no more | **Settled for transfers, S0 slice 7** (`MSG_ENDXFER`, then `MSG_RESET`); whether the feeder stops, and `MSG_STOPFEEDER`, S0 slice 12 |
| Whether `DAT_CALLBACK2` is enough without the loop for sources that signal later | S0 slice 12; the loop stays meanwhile |
| The cancel handle and progress through `in-progress` (proposed) | S0 slice 3, for review |
| Which drivers fill `TWEI_PAGESIDE`; whether WIA or ICA say a page's side | S0 slice 12, the hand checks; the `ica` slice |
| "Continue" after a jam in TWAIN | S0 slice 12 (the sample source can't jam) |
| Compressed transfers (G4, JPEG) for the pipe's throughput, or raw only at first | S0 slice 4's throughput gate; slice 9 |
| The raw layout's 1-bit polarity and 16-bit byte order | S0 slice 3 |
| The driver window's owner across processes | S0 slice 9 |
| Every ICA name marked "to confirm"; its run loop in a worker | The `ica` slice (later) |
| Where driver-reported patch codes go (FEAT-5) | Later |
