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

**Revised 2026-11-09 by S0 slice 2, after slice 7's revision below** (slice 7 ran first): its SANE rows in
sections 1 to 3 and 5 to 8, from `sane-dl` as S0 slice 1 built it (`slices/s0-1-sane-dl.md`, "So far" and
"Code review") and the slice's probes through it, against SANE's `test` and `pnm` backends, the installed
backends with nothing attached, and `fake-libsane` (`slices/s0-2-seam-note.md`, "So far"); a table of SANE's
names per backend (section 1); and the two SANE unknowns answered, names that change on replugging (section 1)
and stopping after the page in flight (section 7), each with what only the fi-6130 settles (S0 slice 6).
Slice 7's TWAIN rows, its TWAIN paragraphs and the TWAIN unknowns are as it left them.

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
| SANE | `sane_get_devices` (optionally local only) gives each device's `name`, `vendor`, `model`, `type`; `sane-dl` gives the name as SANE's bytes (`DeviceName`) and the other three as text (bytes that aren't UTF-8 replaced), from a listing on a thread of its own, under a deadline. Slow through the system's configuration: about 9 s here with nothing attached, all of it `sane_get_devices` (S0 slice 1, step 2: 8.88 to 8.90 s; `scanimage -L` 9.54 s); 0.001 s through a one-backend `ConfigDir`. Every call but `sane_cancel` takes one lock, the listing's too, so a listing and a scan in one process wait on each other. No attach or detach events. Opening by name needs no listing: `dll` loads the backend a name starts with, whatever `dll.conf` lists (S0 slice 1's security item 7). The names: below the table. |
| TWAIN | The DSM lists sources (`DAT_IDENTITY`, `MSG_GETFIRST`/`MSG_GETNEXT`) as `TW_IDENTITY`s: product name, manufacturer, product family, version. Local, but per bitness: a 64-bit DSM lists 64-bit sources only. The DSM loads every source's library at `MSG_OPENDSM`, before anything is listed, so opening the DSM runs each installed driver's code (its log says so), and its cost grows with the drivers installed. DSM 2.5 deliberately skips Windows' own `wiatwain.ds` (its log: "We're deliberately skipping this file"), so WIA devices aren't reached through `TWAINDSM.dll`. A source is opened by its identity. No serial. `MSG_DEVICEEVENT` reports events of an open source, not attach and detach. |
| WIA | `IWiaDevMgr2::EnumDeviceInfo`: a device id string, name, manufacturer. `RegisterEventCallbackInterface` can report connect and disconnect. |
| ICA | `ICDeviceBrowser` and its delegate: devices added and removed as they come (`didAddDevice`, `didRemoveDevice`), asynchronously; each with a persistent id (to confirm). |

**SANE's names, per backend** (S0 slice 2; sane-backends' code read at master `a113b257`, GPL, cited and never
copied; the man pages as installed with 1.4.0). A name is the backend's to form, unique among its devices, and a
SANE text string, which the standard fixes as Latin-1; it goes back to `sane_open` as it was listed (the
standard's API chapter, "Device Descriptor Type" and "Character Type"). Through `dll` it is `<backend>:` and the
backend's name for the device (sane-dll(5)), which is:

| Backend | Its name for a device | Source |
|---|---|---|
| `test` | `0`, `1`, …: an index given in order, two of them without a `test.conf` | probe N1; S0 slice 1, step 2 |
| `pnm` | `0` and `1`, fixed | sane-pnm(5); probe N1 |
| `fujitsu` | `<model>:<serial>` (`fi-6130dj:12345`) when the scanner answers its serial query, else its connection's name; it opens by either | `backend/fujitsu.c`: `attach_one`, `init_serial`, `sane_open` |
| `canon_dr` | its connection's name: the lines that would name it by serial are commented out | `backend/canon_dr.c`: `attach_one`, `sane_open` |
| `kodak` | a SCSI device's path (its models speak SCSI and FireWire) | `backend/kodak.c`: `attach_one`; sane-kodak(5) |
| `avision` | `libusb:<bus>:<device>`, or a SCSI device's path | `backend/avision.c`: `attach`; sane-avision(5) |
| `epson2` | `libusb:<bus>:<device>`, `net:<address>`, or a SCSI or parallel device | `backend/epson2.c`: `device_detect`, `attach_one_net` |
| `epsonds` | `libusb:<bus>:<device>`, or `net:<address>` | `backend/epsonds.c`: `device_detect`, `attach_one_net` |
| `kodakaio` | `libusb:<bus>:<device>`, or `net:<address>`, with `?model=0x…` after it where the model is known | `backend/kodakaio.c`: `k_dev_init`, `attach_one_net` |
| `hp5590` | `libusb:<bus>:<device>` | `backend/hp5590.c`: `attach_usb_device` |
| `xerox_mfp` | `libusb:<bus>:<device>`, or a `tcp <host> [port]` line of its configuration | `backend/xerox_mfp.c`: `list_one_device`; sane-xerox_mfp(5) |
| `plustek` | its configuration's `device` line: `auto` by default, the next matching device; `attach` names the device by the line (whether `plustek-usb.c`'s `usbDev_open`, which finds an `auto` device, renames it isn't read here) | `backend/plustek.c`: `decodeDevName`, `attach`; sane-plustek(5) |
| `pixma` | over USB `<vendor id><product id>_<serial>` (`04A91709_123456`); over the network `<model>_<host or address>` | sane-pixma(5) |
| `net` | `<host>`, `:<port>` if not the default, then `:` and the remote device's own name (`<backend>:<name>` within); an IPv6 address in brackets | `backend/net.c`: `sane_get_devices`; sane-net(5) |
| `escl` | the scanner's URL, `http://` or `https://` `<address>:<port>`, after `unix:<socket>:` where a socket is configured | `backend/escl/escl.c`: `convertFromESCLDev` |
| `airscan` (sane-airscan, a project of its own, 0.99.36 here) | a listed name from its discovery (DNS-SD or WS-Discovery), its form not read here; `escl:<name>:<URL>` or `wsd:<name>:<URL>` opens a device by its URL without one | sane-airscan(5) |

`libusb:<bus>:<device>` is SANE's name for a USB device, the bus's and the device's numbers as the system's
device files under `/dev/bus/usb/` are named (sane-usb(5); sane-avision(5) gives `libusb:002:003`); the
device's number is the one the system gives it as it's plugged in (on Linux the next on its bus at each
enumeration: the kernel's, outside SANE, and not tried here), so a replugging or a reboot can change it.
`dll` keeps four rules
of its own (`backend/dll.c`, `sane_open`; probe N1, and step 1's repair): an alias in `dll.aliases` is listed in
place of a name and hides it (sane-dll(5)), is resolved before anything else at open, and may be named anything,
so neither the listing nor `backend_of` tells it from a backend's name (`alias "Desk scanner" test:0` listed
`Desk scanner`, `backend_of` none; `alias office:scan test:0` listed `office:scan`, `backend_of` `office`, no
backend); a name with no `:` is read as a backend's, and an empty name after the backend's opens its first
device (`Desk scanner` opened through a configuration without the alias answered `INVAL`, `dll` having looked
for a backend of that name); a name whose backend part is empty (`:<device>`, and the empty name) goes to
`dll`'s first backend, the one its `dll.conf` lists last (`add_backend` puts each before the others): `:0` opened
`test`'s device 0 through a configuration naming `test`, `pnm`'s through one naming `test` then `pnm`, and
answered `INVAL` through one naming none, `dll` finding no backend `''`. That is where sane-dll(5)'s "default
backend", the last listed, lives on, not in a name without a `:` as the man page has it. And `fakeusb:<file>`
and `fakeusbdev:<file>` (the rest of the name the file) and `fakeusbout:<file>:<name>` turn on SANE's replay of
USB traffic from a file, or its record into one (`fakeusb:` naming a missing file answered `ACCESS_DENIED` once
SANE had tried to read it; `fakeusbout:<file>:test:0` opened `test:0` and wrote nothing, `test` being no USB
backend).

**Names that change on replugging (SANE): answered by S0 slice 2.** Nothing could be replugged here (no
scanner, no USB), so the answer is drawn from the sources above, the names the `test` backend and the fake give,
and names no longer there opened through the installed backends with nothing attached (probe N1):

- *A name no longer listed answers `INVAL` from `sane_open`, from the backend itself.* Through a configuration
  naming no backend, so that `dll` loaded each by its prefix, `sane-dl` reported `Status { INVAL, Open }` for
  `fujitsu:fi-6130dj:12345`, for `libusb:001:004` under `fujitsu`, `canon_dr`, `epson2`, `avision`, `kodak`
  and `xerox_mfp`, for `pixma:04A91234_ABCDE`, `test:9` and `pnm:2`, each from the backend `dll` handed it to
  (the logs show the open reaching it, `fujitsu`'s, `canon_dr`'s and `kodak`'s ending `no device found`),
  each in at most 0.02 s but `pixma`'s 0.53 s; and for `nosuch:0`, whose backend isn't installed, from `dll`
  itself. So at open `INVAL` covers a device gone and a backend missing alike. The logs
  show what such an open costs: `fujitsu`, `canon_dr` and `kodak` list their devices again inside `sane_open`,
  `avision` makes 97 USB lookups as it starts (94 ids, some twice), and `pixma` broadcasts its network
  discovery (BJNP) on each IPv4 and IPv6 interface, loopback skipped.
- *What a name holds decides what replugging does to it* (the table): an index (`test`, `pnm`) says nothing of
  which device it is, and a backend that numbers its devices can give one's number to another; a connection's
  name (`libusb:<bus>:<device>`, a SCSI path) changes when the device comes back on another number or port; a
  network address (`net`, `escl`, `pixma`'s, `epson2`'s, `epsonds`'s and `kodakaio`'s `net:`, `xerox_mfp`'s
  `tcp`) changes with the address; a serial (`fujitsu`'s, `pixma`'s USB names) survives replugging, as far as
  the code and the man page show.
- *So the model* (S0 slice 2's decisions 11, 12, 18 to 20, for review): a SANE id is `sane:` and the name's bytes
  read as Latin-1, so any name goes back to `sane_open` byte for byte (probe N1, through the fake: a Latin-1 byte
  and UTF-8 bytes each made an id and back, equal). A stored id that answers `INVAL` at open falls back to the
  one device of the same backend, vendor and model, if exactly one is listed, and says so; none or several is
  `Gone`. The worker that opens a stored SANE id is started with a `ConfigDir` naming the id's backend
  (`sane_dl::backend_of`), or none where that gives none, written `Search::BeforeSystem`, since SANE reads its
  configuration once, as it starts, and `ConfigDir` leaves `SANE_CONFIG_DIR` to the process's starter. SANE takes
  each of its files from the first of its directories that holds one, so the directory's `dll.conf` and empty
  `dll.d` decide what lists, while the backend's own `<backend>.conf` and `dll.aliases` are the system's, and an
  alias opens as it was listed, whatever its name holds (step 1's repair, a directory naming the id's backend
  ahead of one standing for the system's, which held `alias office:scan test:0` and `alias "Desk scanner"
  test:1`: `office:scan` opened `test`'s device 0 through a directory naming `office`, `Desk scanner` `test`'s
  device 1 through one naming none; an `Isolated` directory has no aliases, and `office:scan` answered `INVAL`
  there, `dll` finding no backend `office`). There the open goes straight to the backend, and the fallback's
  listing is the directory's backend alone, fast (0.001 s through a one-backend directory) and unable to reach
  another backend. An alias's fallback lists the backend its name happens to start with, if any: none for `Desk
  scanner`, and no backend `office` (0 devices listed each), so such an alias that answers `INVAL` is `Gone`; one
  named after the backend it aliases falls back within that backend, as any id does. An id whose name starts with
  one of `dll`'s testing prefixes or with `:`, or is empty, none of which a backend forms, is refused before
  `sane_open`. Who writes the directory and passes it at the spawn is slices 4's and 5's.
- *What only the fi-6130 settles* (S0 slice 6): its name through `fujitsu` (by serial, or by connection), what
  replugging does to it, what its open answers once it's gone or plugged in again (`INVAL` here, with nothing
  attached), and whether the fallback finds it.

**The model:** `DeviceInfo { id, name, maker, model, driver }` (proposal 9). The id is opaque, namespaced by
backend and, for TWAIN, bitness: `sane:<name>`, `twain32:<product name>`, `twain64:<product name>`,
`wia:<device id>`, `ica:<persistent id>`. Listing is a snapshot under a deadline, in a short-lived worker of
its own; no hot-plug events in the model, since SANE and TWAIN have none (proposal 10). Opening the last
device goes straight to it by id, without a list: SANE's `sane_open(name)`, TWAIN's identity found by a quick
DSM walk, WIA's `CreateDevice(id)`; ICA has to browse for it, under the same deadline.

**To settle:** how TWAIN's two lists merge (`scan-worker`'s client, leaning); ICA's persistent id. A SANE name
that has changed (replugged), and a fallback to the same vendor and model: answered above, the fi-6130's by S0
slice 6.

## 2. What a device can do, and settings

| API | How |
|---|---|
| SANE | Options with descriptors and constraints (ranges, word lists, string lists), by standard names: `source`, `mode`, `resolution`, `tl-x`…`br-y`, and backends' `page-width`/`page-height`. Values vary by backend (`Color`, `Gray`, `Lineart`, `Halftone`…; feeder and duplex by many names). An option's type is its descriptor's, never assumed: the `test` backend's `resolution` is a `FIXED` range (1 to 1200 dpi), `pnm`'s an `INT` word list (75 to 300) that can be automatic; the `test` backend's `source` is `Flatbed` or `Automatic Document Feeder` (S0 slice 1, step 3). Setting one can change others: `SANE_INFO_RELOAD_OPTIONS` (the `test` backend's `mode` set to `Color` says it); a value adjusted to fit says `SANE_INFO_INEXACT` (its `int-inexact` moves the value given). `sane-dl` reads every descriptor again whenever a set says `RELOAD_OPTIONS` or a start answers `INVAL` (`StartInvalid`), counts each re-read, whatever its cause (`Handle::options_generation`), and says a set made whose re-read failed (`ReloadAfterSet`). |
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
sizes against a feeder's own size detection (`Auto`, proposal 9) in each API. SANE's half, from S0 slice 1's
step 3: a resolution comes as a range or a word list, `FIXED` or `INT`, and may offer an automatic value, so
the model's answer takes all of them.

## 3. How a page arrives

| API | How |
|---|---|
| SANE | Per page, pulled: `sane_start`, then `sane_get_parameters` (format `GRAY`, `RGB`, or one of `RED`/`GREEN`/`BLUE`; `last_frame`; bytes and pixels per line; lines, or -1 for unknown; depth 1, 8 or 16), then `sane_read` in chunks to `EOF`. If not `last_frame`, `sane_start` again for the next frame (three-pass color). SANE 1 has only these five frame types: JPEG, CCITT and infrared frames are reserved for a later SANE under `#if 0`. `sane-dl`'s `Batch::next_page` gives a page whole (`Image`): every frame read to `EOF` (`lines = -1` included), the bytes past a row's pixels dropped, three-pass frames interleaved into RGB by their labels, each frame's parameters kept (`Image::frames`), progress by bytes (`Progress { bytes_read, bytes_expected }`). The samples as SANE encodes them, unchanged (the standard's "Image Data Format"; probe L1): rows top down; at depth 1 eight pixels a byte, the leftmost in the high bit, a gray image's 1 black and 0 white (`Solid black` all `0xFF`), the bits past a row's last pixel as the backend sent them (the `test` backend's black sets them, `pnm` passes a PBM's); at depth 1 in RGB each byte eight pixels of one channel, the bytes R, G, B, 0 the least intensity (`Solid black` all `0x00`); at 8 and 16 bits 0 the least intensity; 16-bit in the host's byte order (a `test` page here, each sample swapped, equals `scanimage`'s big-endian PGM). Reads are blocking: several backends answer `UNSUPPORTED` to `sane_set_io_mode` whichever mode is asked for (UP-11; S0 slice 1's decision 57), so the select fd is `sane-dl`'s step-by-step API's alone, and no backend of the model relies on it. |
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

**Decided by the TWAIN spike:** `twain-dsm` hands over **tiles as TWAIN gives them** (each buffer's offsets,
columns, rows, bytes per row and bytes written) and **the DIB's bytes as a block**; the `twain` backend
assembles and decodes, checking each against its bounds before reading. The sample only ever sent full-row
strips, in order; tiles in two dimensions, out of order, are handled (`twain-spike`'s `image` module, with an
`Assembler` that places each buffer as it comes and holds nothing but the image) but shown only by unit tests
on made-up data, and which drivers send them is slice 12's. The refusals (overlaps, gaps, 1-bit tiles that
start or end within a byte, an 8-bit color palette) are the backend's policy, not the sample's behaviour.
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
| SANE | `NO_DOCS` from `sane_start`: on the first page the feeder is empty, later it's done. Some backends return it from `sane_read`. `sane-dl` says either as `Page::NoDocs { from, pages }` (the call, and the pages the batch gave) when it comes before a page's first byte; after one it's an error, and the page is dropped. A flatbed: one page per press (some give the same page forever). The `test` backend's feeder counts every start since the device opened, a flatbed's too, and fills again after `NO_DOCS` (S0 slice 1, step 4; probe S1: 10 more); the fake's stays empty. |
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
| `Busy` | `DEVICE_BUSY`, at open too (`pnm` opens one device at a time) | `TWCC_MAXCONNECTIONS` | `WIA_ERROR_BUSY` | to confirm |
| `Gone` (unplugged, offline) | `IO_ERROR` after opening; `INVAL` from `sane_open`, for a device gone and a backend not installed alike (section 1) | `TWCC_CHECKDEVICEONLINE` (offline at state 3 or less, lost at 4 or more) | `WIA_ERROR_OFFLINE` | the device removed |
| `Driver { message, shown }` | other statuses: `INVAL` from `sane_start` (`StartInvalid`, the descriptors read again), `ACCESS_DENIED` (no authorization callback, FEAT-11), `NO_MEM`, `UNSUPPORTED`; 12 and 13, which `sane.h` 1.4.0 reserves for a later SANE (`#if 0`), kept as integers | `TWCC_OPERATIONERROR` (the driver has already shown the user its message: `shown`), others | others, decided by code (`E_FAIL`'s text is localized) | others |

**SANE, as `sane-dl` gives it** (S0 slice 1): every status with the function that answered it
(`Error::Status { status, call }`), statuses SANE doesn't define kept as integers. No libsane at all is no
status but `LoadError::NotFound`, with the names tried, which the model's errors must say apart from `Gone`.

**The model:** proposal 9's `ScanError`, with two changes: `Driver` carries whether the driver has already
shown its own message, so the app doesn't say it twice; and warming up is handled inside the backends (wait
and retry: WIA's `WIA_ERROR_WARMING_UP`; SANE's warming-up status is reserved and never returned), not
surfaced. `Stopped` stays the watchdog's (proposal 4). Pages before an error are kept; the page in flight is
not.

**To settle:** what "Continue" after a jam is in each API (SANE: `sane_cancel`, then `sane_start` again,
which `sane-dl`'s batch allows, calling `sane_cancel` after every error and starting again if asked, S0 slice
1's decision 42; whether a real feeder goes on after a cleared jam, S0 slice 6;
TWAIN: whether the source stays enabled or needs `MSG_RESET` and enabling again, the hand check, since the
sample source can't jam and the spike couldn't settle it; WIA: a new `Download`); whether `Busy` and `Gone` need
telling apart for the user beyond their words.

## 7. Cancel

| API | How |
|---|---|
| SANE | `sane_cancel` from any thread; the read in progress returns `CANCELLED`; no other call on the handle, `sane_close` included, until it has. A frontend ends every batch with it, even after the last page's `EOF` (the standard's "Code Flow"). `sane-dl`'s token (`CancelToken`) calls it at once from any thread, synchronized with close, and flags the cancel: the next start or read returns `Cancelled` without calling SANE, and a read in flight returns it whatever SANE answers, so the page in flight is dropped (S0 slice 1's decision 41). Stopping after the page in flight is the caller's: it starts no next page and drops the batch (below). |
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

**Stopping after the page in flight (SANE): answered by S0 slice 2**, on the `test` backend's feeder and the
fake's (`sheets 10`), through `sane-dl` (probe S1; `slices/s0-2-seam-note.md`, "So far"). SANE lets a frontend
finish the page in flight and feed no more: read the page to its `EOF`, start no next page, and end the batch
with `sane_cancel`, which `sane-dl`'s `Batch` calls as it's dropped. A batch dropped after its third page left
the rest in the feeder, on both: the next batch gave 7 pages, then `NO_DOCS`. Step by step, `sane_cancel` after
a page's `EOF` and then `sane_start` started the next sheet (after two pages so, a batch gave the other 8). The
token stops at once instead: cancelled midway through the second page (at 640 of its 1,209 bytes on `test`, 32
of 64 on the fake), it dropped that page and its sheet (the next batch gave 8); cancelled between pages, after
`next_page` returned, it stopped the next start before SANE was called (the fake's log: `sane_cancel`, and no
`sane_start` until the next batch), so no sheet went (the next batch gave 9). The fake's duplex, a batch ended
after a front, gave that sheet's back first in the next batch (marked `B`, its second page): the fake is our
model of a duplex backend, not evidence of one, so this shows only that `sane-dl` lets a backend deliver the
back before a batch ends (one more `next_page`). **What only hardware shows** (S0 slice 6): whether a feeder
has pulled the next sheet in before the page in flight ends; what a backend does at `sane_cancel` while its
batch is started (`fujitsu.c`'s `sane_cancel`, through `check_for_cancel`, sends the scanner a halt or a cancel
command then, studied); and what a duplex scanner gives after a batch ended on a front.

**Proposed, for slice 3 (for review; asked 2026-11-08 whether `in-progress` fits):** the cancel handle
and the progress events through the `in-progress` crate (github.com/sheaf-pdf/in-progress), which
`scan-cleanup` already takes. A `Tracker` is the
`Send + Sync` handle any thread can cancel; each backend calls `check()` between pages, never within one, which
is "finish the page in flight"; progress is `set_total(Some(page bytes), Unit::Bytes)` and `inc` per buffer;
and the intake pipeline could give scanning and cleanup weighted children of one tracker. Two seams: its
progress is polled, not pushed, so `scan-worker` polls the worker's tracker and forwards snapshots over its
pipe; and a flag can't wake the TWAIN thread blocked in `GetMessageW`, so the TWAIN backend posts a message to
its window as well.

## 8. Threads and events

| API | How |
|---|---|
| SANE | Pull, on the caller's thread: one thread per handle, calls never overlapping; `sane_cancel` the one call allowed from elsewhere. `sane-dl` takes one lock for every call but `sane_cancel`, whatever the thread or device (`Sane` is `Send + Sync`; a `Handle` is `Send` and used from one thread at a time), and its token cancels from any thread. A read that gives no bytes is made again after a pause growing to 10 ms, with no cap, so a backend answering so while its lamp warms up is waited on (S0 slice 1's decision 58); a read that never returns is the watchdog's. The `test` backend 1.4.0 can deadlock as a frame ends (UP-8), so tests and probes pace its reads. |
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
and the callback holds only atomics. Only messages to the spike's own window are the spike's (matched by
window, not by number: a source's own windows may use the same numbers), and a request that comes while the
loop waits for the source is kept, not dropped. **Cancel, as shown:** a cancel queued before the enable was
kept by the loop that waited for `MSG_XFERREADY` and ended the batch before any transfer (`MSG_RESET` from
state 6, the source then accepting `MSG_DISABLEDS`); one posted during a memory transfer, at a pause the TWAIN
thread made after the page's first buffer, ended the batch after that page. Nothing is taken during a native
transfer or while the thread is inside a DSM call; how the backend serves requests while a driver is busy is
slices 8 and 9's. **The state the application records isn't the source's:** the sample is in state 6 as
soon as it signals inside `MSG_ENABLEDS`, before the application has heard, so a walk down after a failure
there must allow for it; `twain-spike` rolls back as `TWAIN.cs` does (on `TWCC_SEQERROR`, from state 7's first
step), and never unloads the DSM while a source is open, since the source holds the DSM's entry points.

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
| SANE names that change on replugging, per backend; a fallback by vendor and model | **Answered, S0 slice 2** (section 1): each backend's names from its source or man page; `INVAL` at open for a name no longer there; the fallback by backend, vendor and model, in a worker started with the id's one-backend `ConfigDir`. The fi-6130's name, and what replugging does to it: S0 slice 6 |
| Whether SANE can stop after the page in flight | **Settled on the `test` backend and the fake, S0 slice 2** (section 7): the page read to `EOF`, no next start, the batch ended, the sheets after left in the feeder. Whether a real feeder has pulled the next sheet in: S0 slice 6 |
| Whether `twain-dsm` hands over tiles or assembled images; what a DIB's palette becomes | **Settled, S0 slice 7:** tiles and the DIB's block; the backend assembles, decodes and applies the palette (section 3) |
| How TWAIN's 32- and 64-bit lists merge (leaning: `scan-worker`'s client) | S0 slice 9 |
| How events cross from the TWAIN thread to the backend's; posting cancels to its loop | **Settled, S0 slice 7:** a channel out; requests posted to the window, matched by window, kept while the loop waits, never forwarded; a cancel queued before the enable, and one during a memory transfer, shown (section 8). Serving requests while a driver is busy, slices 8 and 9 |
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
