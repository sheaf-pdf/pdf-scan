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
names per backend (section 1); the two SANE unknowns answered, names that change on replugging (section 1)
and stopping after the page in flight (section 7), each with what only the fi-6130 settles (S0 slice 6); the
raw layout fixed (section 3) and held beside `scanimage`'s pages; and the model written down for slice 3 to
build ("The model, for slice 3"), the model's paragraphs of sections 1 to 8 revised to it, "Changes from
proposal 9's model" with them, and the unknowns table's rows. Slice 7's TWAIN rows, its TWAIN paragraphs and
the TWAIN unknowns are as it left them, but where the model both backends meet changed: a cancel finishes the
sheet in flight, not the page (section 7's model paragraph and its question; in duplex, a delivered front's
back too); section 7's `in-progress` proposal is the model's, its TWAIN facts kept and the TWAIN thread's
wake-up left to slice 9 (that paragraph, and the unknowns' `in-progress` row); and section 8's model sentences,
before "TWAIN, by the spike", say the two traits and `in-progress`. A new unknown, the back after a cancel,
names slices 6 and 12. Its code review then changed the model before slice 3 (the slice's plan, "Code review"):
`Backend::open` takes the stored `DeviceInfo` and a deadline, the SANE fallback lists before it opens and only
from a USB connection's name, events go on a bounded channel, the growing structs are `#[non_exhaustive]`,
`Encoded` is asked for, the settings window takes an owner, `Stopped` moves to `scan-worker`'s client, and
`ScanError` gains `SettingsRefused`, `AccessDenied`, `TimedOut` and `Unsupported`.

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

`libusb:<bus>:<device>` is SANE's name for a USB device, the bus's and the device's numbers as the system's device
files under `/dev/bus/usb/` are named (sane-usb(5); sane-avision(5) gives `libusb:002:003`); the device's number is
the one the system gives it as it's plugged in (on Linux the next on its bus at each enumeration: the kernel's,
outside SANE, and not tried here), so a replugging or a reboot can change it. `dll` keeps four rules of its own
(`backend/dll.c`, `sane_open`; probe N1, and step 1's repair): an alias in `dll.aliases` is listed in place of a
name and hides it (sane-dll(5)), is resolved before anything else at open, and may be named anything, so neither
the listing nor `backend_of` tells it from a backend's name (`alias "Desk scanner" test:0` listed `Desk scanner`,
`backend_of` none; `alias office:scan test:0` listed `office:scan`, `backend_of` `office`, no backend); a name with
no `:` is read as a backend's, and an empty name after the backend's opens its first device (`Desk scanner` opened
through a configuration without the alias answered `INVAL`, `dll` having looked for a backend of that name); a name
whose backend part is empty (`:<device>`, and the empty name) goes to `dll`'s first backend, the one its `dll.conf`
lists last (`add_backend` puts each before the others): `:0` opened `test`'s device 0 through a configuration
naming `test`, `pnm`'s through one naming `test` then `pnm`, and answered `INVAL` through one naming none, `dll`
finding no backend `''`. That is where sane-dll(5)'s "default backend", the last listed, lives on, not in a name
without a `:` as the man page has it. And `fakeusb:<file>` and `fakeusbdev:<file>` (the rest of the name the file)
and `fakeusbout:<file>:<name>` turn on SANE's replay of USB traffic from a file, or its record into one (`fakeusb:`
naming a missing file answered `ACCESS_DENIED` once SANE had tried to read it; `fakeusbout:<file>:test:0` opened
`test:0` and wrote nothing, `test` being no USB backend).

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
- *So the model* (S0 slice 2's decisions 11, 12, 18 to 20, 28 and 29, for review): a SANE id is `sane:` and the
  name's bytes read as Latin-1, so any name goes back to `sane_open` byte for byte (probe N1, through the fake: a
  Latin-1 byte and UTF-8 bytes each made an id and back, equal). The worker that opens a stored SANE id is
  started with a `ConfigDir` naming the id's backend (`sane_dl::backend_of`), or none where that gives none,
  written `Search::BeforeSystem`, since SANE reads its configuration once, as it starts, and `ConfigDir` leaves
  `SANE_CONFIG_DIR` to the process's starter. SANE takes each of its files from the first of its directories
  that holds one, so the directory's `dll.conf` and empty `dll.d` decide what lists, while the backend's own
  `<backend>.conf` and `dll.aliases` are the system's, and an alias opens as it was listed, whatever its name
  holds (step 1's repair, a directory naming the id's backend ahead of one standing for the system's, which held
  `alias office:scan test:0` and `alias "Desk scanner" test:1`: `office:scan` opened `test`'s device 0 through a
  directory naming `office`, `Desk scanner` `test`'s device 1 through one naming none; an `Isolated` directory
  has no aliases, and `office:scan` answered `INVAL` there, `dll` finding no backend `office`). There the `sane`
  backend lists the id's backend **before it opens anything**, under the open's deadline: fast (0.001 s through a
  one-backend directory) and that backend's alone, which a listing after an open needn't be, since `dll` adds a
  backend as it opens a name, an alias's target's too, and lists every backend it has added (the code review's
  probe: through a directory naming none, a listing found 0 devices, and after `Desk scanner` answered `INVAL`
  the same process listed `test:0`, of the vendor and model the alias was listed with; so too after
  `office:scan` through one naming `office`). The stored name listed with the stored vendor and model is opened,
  `Opened::info` the listing's; listed with another, it's taken as gone, the name another device's now (an index
  or a connection's number given again); not listed, it's opened by name all the same (an alias, or a name the
  backend opens but lists another way), its info the stored one, unchecked, since `sane_open` gives a handle and
  no vendor or model (`sane.h`). A name gone, or answering `INVAL`, **falls back only from a USB connection's
  name** (`libusb:<bus>:<device>` after the backend, the one form replugging renumbers): over that listing, to
  the one device listed under a name of that form whose `backend_of` is the id's and whose vendor and model are
  the stored ones, if exactly one is, said in `renamed_from`. Anything else is `Gone`, and the app shows the
  list: none or several; an index (`test:9`), which says nothing of the device; a serial, which survives
  replugging; a network name (`net`'s, `escl`'s, `airscan`'s, a backend's `net:` names), since one of the same
  model on another host is another machine's scanner; an alias (`Desk scanner`'s `backend_of` none, `office`
  no backend). An id whose name starts with one of `dll`'s testing prefixes or with `:`, or is empty, none of
  which a backend forms, is refused before `sane_open`, as is one that isn't `sane:` and Latin-1 (a character
  past U+00FF) or holds a `NUL`: each is `ScanError::BadId`, not `Gone`, since such an id never opens,
  whichever directory its worker was given. Who writes the directory and passes it at the spawn is slices 4's
  and 5's.
- *What only the fi-6130 settles* (S0 slice 6): its name through `fujitsu` (by serial, or by connection), what
  replugging does to it, what its open answers once it's gone or plugged in again (`INVAL` here, with nothing
  attached), and whether the fallback finds it.

**The model** ("The model, for slice 3"): `DeviceInfo { id, name, maker, model, driver }` (proposal 9), what a
listing gives and what the app stores of a device, given back whole to `Backend::open` (S0 slice 2's decision
27). The id (`DeviceId`) is compared whole and namespaced by driver and, for TWAIN, bitness: `sane:` and the
SANE name's bytes as Latin-1 (above), `twain32:<product name>`, `twain64:<product name>`, `wia:<device id>`,
`ica:<persistent id>`, `fake:<name>`. The app reads nothing from it: `scan-worker`'s client picks the worker by
`driver` (TWAIN's `bits` choosing the 32- or 64-bit one), and a backend refuses an id that isn't of its form
(`BadId`), so an id and a driver that disagree never open. Listing is a snapshot under a deadline
(`Backend::list`), in a short-lived worker of its own; no hot-plug events in the model, since SANE and TWAIN
have none (proposal 10). Opening the last device goes to it by its stored `DeviceInfo`, under a deadline too,
without the system's listing (`Backend::open`): SANE's `sane_open(name)` after a listing of the id's backend
alone (above), TWAIN's identity found by a quick DSM walk, WIA's `CreateDevice(id)`; ICA has to browse for it.
A deadline passed is `ScanError::TimedOut`, and a backend that answered it is dropped with its worker, since a
SANE listing past its deadline still holds the library (`sane-dl`'s `ListingPending`); a call that never
returns is the watchdog's. A backend that knowingly opens another device in place of the stored one says so
(`Opened::renamed_from`), and the app shows it: SANE's fallback above is the only one so far. A name given
since to another device of the same vendor and model opens that device unremarked, which only a serial in the
name prevents (the fi-6130's, TEST-9).

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

**The model** ("The model, for slice 3"): `Settings { source, duplex, dpi, format, paper, encodings }`
(`Settings::new(source, dpi, format)`: one side, `Paper::Auto`, raw only), clamped to what the device can do, and
what was clamped said (`Event::Applied`: the settings as the device took them, and each it changed, with what was
asked). **Capabilities per source** (a change from proposal 9): in all four APIs what's allowed depends on the
source chosen (SANE's options change with `source`; TWAIN's sizes with `CAP_FEEDERENABLED`; WIA's and ICA's live on
the source's own item or unit), so `Caps` is a list of sources (`SourceCaps`), each with its own resolutions,
formats and largest area, whether it has duplex, and whether the driver finds the paper's size itself. Resolutions
are a list or a range (`Resolutions`), as SANE and TWAIN give either. Clamping is found the same way everywhere:
set, then read back (SANE's `INEXACT`, TWAIN's `CHECKSTATUS`, WIA's and ICA's values read again). **What's asked
for is a format** (`Format`, the raw layout's five, section 3), in place of proposal 9's `Mode` (`Color`, `Gray`,
`Bw`), so 16-bit is asked for and clamped like any setting: each backend maps its API's names to it by a table
(SANE's `mode` and `depth`, TWAIN's `ICAP_PIXELTYPE` and `ICAP_BITDEPTH`, WIA's data type and depth), as the
feeder's names are. Paper is `Auto` (the driver's own size detection) or a size (`SizeMm`, in `f32` millimetres;
Letter, Legal and A4 its constants, since Letter's 215.9 isn't a whole millimetre). SANE's scan area comes in
millimetres or in pixels, the backend's choice (the standard's "Scan Area Options"), pixels converted at the
resolution set. Where SANE's options lack one of these (S0 slice 2's decision 41; `pnm` has no `source`, `mode` or
area options, the code review's probe): a source without area options has no largest area (`SourceCaps::max_area`
`None`), and a size asked of it is clamped to `Auto`; one without a `mode` offers the one format
`sane_get_parameters` gives before the start (the backend's estimate, `sane-dl`'s `Handle::parameters`: `pnm` gave
a 13 by 3 PBM's 1-bit gray, as it then scanned it), any other clamped to it, and a page whose frames then say
another is delivered as they say; a device without a `source` option is one source, a flatbed. **Encoded forms are
asked for** (`Settings::encodings`, the app's order, among a source's `SourceCaps::encodings`); empty, the default,
is raw only, and whether the app asks is open (section 3).

**Asking drivers not to drop pages.** Some drivers can discard blank pages themselves (TWAIN's
`ICAP_AUTODISCARDBLANKPAGES`, PaperStream's own processing). That breaks every inference of a page's side
(section 4), and `scan-cleanup` finds blank pages anyway, so backends turn it off where the API lets them.

**To settle:** paper sizes against a feeder's own size detection (`Auto`, proposal 9) in each API. Whether
resolutions are listed or a range: both (`Resolutions`, S0 slice 2). SANE's, from S0 slice 1's step 3, come
as a range or a word list, `FIXED` or `INT`, and may offer an automatic value: the `sane` backend gives whole
dpi either way, and the app asks for a dpi, never the automatic value. A range's `step` is never 0
(`NonZeroU32`, decision 38): SANE's quant 0 ("none", `sane.h`) or one under 1 dpi is 1, a larger fractional one
is rounded up to whole dpi, and the device's `INEXACT` says what it took; a `FIXED` range's ends are rounded
inward and a word list's values to the nearest whole dpi, a range left with no whole dpi dropped. A list is
never empty and a range's `min` never past its `max`, which `scan-worker`'s client checks as it decodes.

## 3. How a page arrives

| API | How |
|---|---|
| SANE | Per page, pulled: `sane_start`, then `sane_get_parameters` (format `GRAY`, `RGB`, or one of `RED`/`GREEN`/`BLUE`; `last_frame`; bytes and pixels per line; lines, or -1 for unknown; depth 1, 8 or 16), then `sane_read` in chunks to `EOF`. If not `last_frame`, `sane_start` again for the next frame (three-pass color). SANE 1 has only these five frame types: JPEG, CCITT and infrared frames are reserved for a later SANE under `#if 0`. `sane-dl`'s `Batch::next_page` gives a page whole (`Image`): every frame read to `EOF` (`lines = -1` included), the bytes past a row's pixels dropped, three-pass frames interleaved into RGB by their labels, each frame's parameters kept (`Image::frames`), progress by bytes (`Progress { bytes_read, bytes_expected }`). The samples as SANE encodes them, unchanged (the standard's "Image Data Format"; probe L1): rows top down; at depth 1 eight pixels a byte, the leftmost in the high bit, a gray image's 1 black and 0 white (`Solid black` all `0xFF`), the bits past a row's last pixel as the backend sent them (the `test` backend's black sets them, its `Grid` not the same ones from one run to another: two of `scanimage`'s PBMs of it half an hour apart, their pixels equal, differ there in 191 rows of 196; `pnm` passes a PBM's); at depth 1 in RGB each byte eight pixels of one channel, the bytes R, G, B, 0 the least intensity (`Solid black` all `0x00`); at 8 and 16 bits 0 the least intensity; 16-bit in the host's byte order (a `test` page here, each sample swapped, equals `scanimage`'s big-endian PGM). Reads are blocking: several backends answer `UNSUPPORTED` to `sane_set_io_mode` whichever mode is asked for (UP-11; S0 slice 1's decision 57), so the select fd is `sane-dl`'s step-by-step API's alone, and no backend of the model relies on it. |
| TWAIN | Per image, pushed then pulled: the source posts `MSG_XFERREADY` (state 6), once per enable: after `MSG_ENDXFER` with pages pending the application transfers again, with no second signal. `DAT_IMAGEINFO` gives width, height, resolution, pixel type, bits per pixel and compression; then a transfer (state 7): **native** (on Windows a `GlobalAlloc` handle to a packed DIB, the application's to free: a header, a palette, rows bottom up, each padded to four bytes, BGR), **memory** (buffers of rows or tiles with `XOffset`, `YOffset` and bytes per row, compressed if `ICAP_COMPRESSION` asked for G4 or JPEG; the sample sends strips of full rows, top to bottom, rows padded to four bytes, as many rows as fit the preferred buffer), **file** or **memory file** (the driver writes a file format). After it, `DAT_EXTIMAGEINFO` (section 4), then `DAT_PENDINGXFERS`/`MSG_ENDXFER`, which says how many more are pending. |
| WIA | Pushed: `IWiaTransfer::Download` calls back `GetNextStream` for each page (we hand it a stream) and `TransferCallback` with progress (`ulTransferredBytes`; not the percentage, which peaks near 40 then jumps), end of stream and end of transfer. The page's format is the driver's choice among what it lists (`EnumWIA_FORMAT_INFO`): a bitmap, WIA's raw format, PNG, JPEG or TIFF. |
| ICA | Pushed, through delegates: memory-based transfers give bands (`ICScannerBandData`: the full image's size, bits per pixel and per component, rows from a start row, bytes per row, a buffer); file-based ones a file's URL (to confirm). |

**The model** ("The model, for slice 3"): `RawPage { pixels, dpi, sheet, side, side_known }` (proposal 9's,
with section 4's `side_known`), with `pixels` one of two:

- **raw** (`RawImage`): one layout for every backend, fixed by S0 slice 2 (below): a format (`Bw1`, `Gray8`,
  `Gray16`, `Rgb8`, `Rgb16`), a width and a height, rows top down and packed, with one polarity, bit order and
  byte order. Each backend assembles its API's pieces into it: SANE's three passes and padded rows, TWAIN's
  tiles and its DIB's bottom-up rows and palette, ICA's bands, WIA's raw header.
- **encoded** (`Encoded`): bytes as the driver gave them, with their kind (JPEG, PNG, TIFF, a bitmap, CCITT G4 with
  its width, height and polarity), when the settings ask for a form the source offers (`Settings::encodings`, among
  `SourceCaps::encodings`; `Encoding` names the forms), else raw. `scan-image` keeps a JPEG as its bytes, so
  passing one through avoids a decode and a re-encode. SANE 1 gives none: its JPEG and CCITT frames are reserved
  (above).

**A page is delivered whole**, once complete; a 600 dpi color Letter page is about 100 MB raw, and one at a time is
fine, which the bounded channel holds a backend to (section 8): its `send` waits while the channel is full, so a
pull backend starts no next sheet before its pages are taken. Progress goes through `in-progress` (section 7): each
page's bytes as it's read, against its size where known, as proposal 10's progress bar wants. A page cut short (a
jam mid-page) is never delivered.

**The raw layout** (S0 slice 2, its decisions 2 to 6, for review): one for every backend, which every
`RawImage` holds.

| | The model's `RawImage` | SANE gives (and `sane-dl` passes on unchanged) | TWAIN gives (slice 7's rows, as they are) | Which converts |
|---|---|---|---|---|
| Formats | `Bw1`, `Gray8`, `Gray16`, `Rgb8`, `Rgb16` | `GRAY` or `RGB` (or three single-channel frames, interleaved by `sane-dl`) at depth 1, 8 or 16 | Black and white, gray, RGB; 16 bits not seen (the sample offers 1, 8 and 24) | `sane`: RGB at depth 1 (byte-interleaved, a byte of eight pixels of one channel, the standard) expanded to `Rgb8`, each bit 0 or 255 |
| Polarity | 0 the least intensity at every format: in `Bw1`, 0 black and 1 white | `GRAY` at depth 1: 1 black, 0 white; every other: 0 the least intensity (the standard) | DIB: through its palette (the sample's 1-bit black then white, its gray an identity ramp); memory: `ICAP_PIXELFLAVOR` (`TWPF_CHOCOLATE`, 0 black, all the sample offers) | `sane`: 1-bit gray inverted; `twain`: the palette and flavor applied (`TWPF_VANILLA`, or a white-then-black palette, inverted) |
| Bits in a byte | The leftmost pixel in the most significant bit; bits past a row's last pixel 0 | The leftmost pixel in the most significant bit (the standard) | DIB: the leftmost pixel in the high bit (a DIB's rule); memory: equal to the DIB pixel for pixel on the sample, so the same, whatever `ICAP_BITORDER` reports (`TWBO_LSBFIRST`, read and never set: the sample's quirk reads it as a color order) | `sane`: padding bits cleared again after the inversion |
| 16-bit | Little-endian, whatever the host | The host's byte order (the standard) | Not seen: slice 8 reads what TWAIN says, slice 12 sees a driver's | `sane`: swapped on a big-endian host, nothing on CI's three systems |
| Rows | Top down, packed: each row exactly its pixels' bytes; no stride stored, `stride()` worked out from format and width | Top down; `bytes_per_line` may pad, `sane-dl` drops it | DIB bottom up, or top down where its height is negative, rows padded to four bytes; memory strips top down (the sample pads to four bytes), tiles by offset | `twain`: a DIB's rows flipped where it's bottom up (a positive height), padding dropped, tiles placed |
| Channels | R, G, B per pixel, interleaved | R, G, B (one frame, or three interleaved by `sane-dl`) | DIB B, G, R; memory R, G, B on the sample | `twain`: the DIB's swapped |

WIA's and ICA's columns are to confirm (slice 10, and the `ica` slice). The layout is slice 7's comparable form
(the spike's `image` module, its module doc: rows top down, packed, RGB, gray and 1-bit with 0 black, the first
pixel in the high bit, the bits past a row's last pixel zero), with 16-bit little-endian added, so the TWAIN side's
conversion exists already and SANE's 1-bit gray is the one format a backend inverts. **Held beside a third party**
(probe L2, `slices/s0-2-seam-note.md`, "So far"): `sane-dl`'s pages from the `test` backend at 157 pixels a row
(not a multiple of 8), converted as the table says, equal the raster of `scanimage --format=png` of the same scan
pixel for pixel at gray 1 (two pictures), 8 and 16 and color 8 and 16, PNG's big-endian 16-bit swapped first
(unswapped, the bytes differ); PNG's 1-bit gray is 0 black, the high bit first, and SANE's bits as they came differ
from it in every pixel. `scanimage` inverts a 1-bit row's whole bytes for PNG, padding bits included, so those are
masked on its side; the model's are 0 in every row. Its TIFF of the same 1-bit page keeps SANE's bits (photometric
0, white is zero). 1-bit RGB has no third party here (`scanimage` refuses it: its assertion that an RGB frame is 8
or 16 bits deep stops it before it writes a byte, in PNG, TIFF and PNM alike): its channel order is probe L1's (the
SANE row above).

**`RawImage::new(format, width, height, data)` is the one way to make one** (its fields are private): `width` and
`height` at least 1, `data` exactly `height` rows of the format's packed bytes, and a `Bw1` row's bits past its
last pixel 0, so every `RawImage` holds the layout wherever it's made, the worker's client included. The length is
worked out in `u64` with checked arithmetic, and a size past what `u64` or the platform's `usize` holds is refused,
never wrapped: in a 32-bit worker `u32` × `u32` × 6 overflows `usize` (65,536 by 65,536 in `Rgb16` is 6 × 2^32
bytes, 0 once wrapped, which an empty buffer would match), and at `u32::MAX` by `u32::MAX` a format of two bytes a
pixel or more overflows `u64` too (the spike's `image` module works its sizes out in `u64` for the same reason).
Probe M1 tests each, the 32-bit cases on a 32-bit target too.

**To settle:** whether the app asks TWAIN and WIA for compressed transfers (`Settings::encodings`: G4 for black and
white, JPEG for color) to cut the pipe's bytes against the 58 MB/s the throughput gate needs
(`crates/scan-worker.md`), or stays raw in the first version; whether `twain-dsm` hands over assembled images or
tiles (leaning: tiles, assembled by the backend, so `twain-dsm` stays TWAIN alone); what a native DIB's palette
becomes.

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

**The model** ("The model, for slice 3"): `RawPage::sheet` counted from 1 across a batch, and on through "Scan
More" (proposal 10), which starts the next batch at the sheet after (`Device::scan`'s `first_sheet`);
`Event::Sheet` as each sheet begins; `side` `Front` or `Back`, and `side_known` whether it was **reported** by
the driver (`SideKnown::Reported`) or **inferred** from the order (`Inferred`). Inference is right only while
no page is dropped, hence asking drivers not to drop blanks (section 2). The intake can pair and turn backs
(proposal 10's "Scan backs") knowing which it has.

**To settle:** which drivers fill `TWEI_PAGESIDE` (the fi-6130's FUJITSU TWAIN32 and PaperStream IP, by hand
in S0 slice 12); whether WIA or ICA says the side anywhere.

## 5. The end of a batch

| API | How |
|---|---|
| SANE | `NO_DOCS` from `sane_start`: on the first page the feeder is empty, later it's done. Some backends return it from `sane_read`. `sane-dl` says either as `Page::NoDocs { from, pages }` (the call, and the pages the batch gave) when it comes before a page's first byte; after one it's an error, and the page is dropped. A flatbed: one page per press (some give the same page forever). The `test` backend's feeder counts every start since the device opened, a flatbed's too, and fills again after `NO_DOCS` (S0 slice 1, step 4; probe S1: 10 more); the fake's stays empty. |
| TWAIN | `CAP_FEEDERLOADED` false before enabling: empty. `TWCC_NOMEDIA`. With `CAP_XFERCOUNT = -1` the source scans until empty; `MSG_ENDXFER`'s pending count reaching 0 is done (state 5). Until then the count is 0xFFFF, "don't know" (the sample's, after every page but the last), so a TWAIN batch's length is unknown until it ends. |
| WIA | `WIA_ERROR_PAPER_EMPTY` at the start: empty. `WIA_STATUS_END_OF_MEDIA` (0x210001), or the end of the transfer: done. `FEED_READY` is a hint, wrong either way at times. A page count reached while the feeder runs on: cancel and `WIA_CMD_STOP_FEEDER`. |
| ICA | The feeder unit's "document loaded" (to confirm); the scan's completion delegate without an error: done. |

**The model** ("The model, for slice 3"): an empty feeder before any page is the error
`ScanError::FeederEmpty` (nothing was scanned); an empty feeder after pages is `Done::FeederEmpty`; a
flatbed's page is `Done::OnePage`; a cancel is `Done::Cancelled`, once the sheet in flight is delivered
(section 7). Proposal 9 has `FeederEmpty` in both; this is the line between them. "Scan More" is a new batch
that carries the sheet count on (`first_sheet`). For SANE, `Page::NoDocs` with `pages` 0 from a feeder is
`ScanError::FeederEmpty`, and with pages before it `Done::FeederEmpty`.

## 6. Errors

| Model | SANE | TWAIN | WIA | ICA |
|---|---|---|---|---|
| `Jammed` | `JAMMED` | `TWCC_PAPERJAM` | `WIA_ERROR_PAPER_JAM` | an error from the delegate (to confirm) |
| `DoubleFeed` | none: backends say a jam or an I/O error | `TWCC_PAPERDOUBLEFEED` | `WIA_ERROR_MULTI_FEED` | to confirm |
| `CoverOpen` | `COVER_OPEN` | `TWCC_INTERLOCK` | `WIA_ERROR_COVER_OPEN` (SDK value and Learn's misprint both) | to confirm |
| `Busy` | `DEVICE_BUSY`, at open too (`pnm` opens one device at a time) | `TWCC_MAXCONNECTIONS` | `WIA_ERROR_BUSY` | to confirm |
| `Gone` (unplugged, offline) | `IO_ERROR` after opening; `INVAL` from `sane_open`, for a device gone and a backend not installed alike (section 1) | `TWCC_CHECKDEVICEONLINE` (offline at state 3 or less, lost at 4 or more) | `WIA_ERROR_OFFLINE` | the device removed |
| `Driver { message, shown }` | other statuses: `NO_MEM`, `UNSUPPORTED`; 12 and 13, which `sane.h` 1.4.0 reserves for a later SANE (`#if 0`), kept as integers; every other `sane-dl` error ("The model, for slice 3", its mapping) | `TWCC_OPERATIONERROR` (the driver has already shown the user its message: `shown`), others | others, decided by code (`E_FAIL`'s text is localized) | others |
| `NotInstalled { what }` (S0 slice 2) | no libsane: `LoadError::NotFound`, with the names tried | no `TWAINDSM.dll`, neither the system's nor the one the app names (its bundled copy: `crates/twain-dsm.md`, slice 8); which the app names, slice 9's | to confirm | to confirm |
| `BadId` (S0 slice 2) | a stored id refused before `sane_open` (section 1): not `sane:` and Latin-1, a `NUL` (`sane-dl`'s `NulInName`), `dll`'s testing prefixes, a leading `:`, the empty name | slice 9's to say | to confirm | to confirm |
| `SettingsRefused` (S0 slice 2's code review) | `INVAL` from `sane_start` again once the settings are set again (`StartInvalid` twice running): `escl`'s empty scan region, `lexmark`'s zero size, `pixma`'s and `hp5590`'s parameters it can't scan with (`backend/escl/escl.c`, `lexmark.c`, `pixma/pixma.c`, `hp5590.c`, `sane_start`, studied) | slice 9's to say | to confirm | to confirm |
| `AccessDenied` (S0 slice 2's code review) | `ACCESS_DENIED`: the device's permissions (`epson2`, `epsonds` and `kodakaio` log "check that you have permissions on the device" as `sanei_usb_open` answers it; `pixma` maps its `EACCES` to it: studied), or an authorization the app doesn't give (no callback, FEAT-11) | slice 9's to say | to confirm | to confirm |
| `TimedOut` (S0 slice 2's code review) | a listing past its deadline (`sane-dl`'s `DeadlineExceeded`, and `ListingPending` after it), in `list` or in `open`'s listing | the DSM walk past `open`'s deadline (slice 9) | to confirm | the browse past `open`'s deadline (to confirm) |
| `Unsupported` (S0 slice 2's code review) | a settings window asked of a SANE device, which has none (section 9) | slice 9's to say | to confirm | to confirm |

**SANE, as `sane-dl` gives it** (S0 slice 1): every status with the function that answered it
(`Error::Status { status, call }`), statuses SANE doesn't define kept as integers. No libsane at all is no
status but `LoadError::NotFound`, with the names tried: the model's `NotInstalled`, apart from `Gone`. The
other `LoadError`s (a symbol missing, `sane_init` failing, a version not 1, a second load) are `Driver`.

**The model** ("The model, for slice 3"): proposal 9's `ScanError`, with these changes: `Driver` carries
whether the driver has already shown its own message (`shown`), so the app doesn't say it twice; warming up is
waited on inside the backends (wait and retry: WIA's `WIA_ERROR_WARMING_UP`; SANE's warming-up status is
reserved and never returned, and an empty read is made again, section 8) and said to the user as
`Event::Status(Status::WarmingUp)` where the backend knows it, never an error; `NotInstalled { what }` says
the system's driver API isn't there (no libsane, no `TWAINDSM.dll`: proposal 6's "Scanning on Linux needs
SANE"), apart from `Gone`; and `BadId` says a stored id isn't one the driver forms, refused before the driver
sees it (SANE's refusals, section 1), apart from `Gone` too: a device gone may come back, such an id never
opens. From the slice's code review (decisions 31 to 34, for review): `SettingsRefused` says the scanner
wouldn't start with these settings, which the app can change, apart from `Gone`, since such a scanner is there
(SANE's `INVAL` from a start, the settings set again and refused again, which the backends studied answer for
an area or a size they can't scan, not for a device gone); `AccessDenied` says access was refused, which the
app can explain (on Linux the device's permissions, the commonest fault in setting SANE up); `TimedOut` says a
deadline of `list` or `open` passed; and `Unsupported` that the device can't do what was asked (a settings
window it hasn't). `Stopped`, the watchdog's (proposal 4), leaves the model for `scan-worker`'s client, whose
own error wraps `ScanError` beside its other failures (a crash, a protocol mismatch, a page cut short), so no
backend can return it. Pages before an error are kept; the page being read when it comes is not.

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

**The model** ("The model, for slice 3"; S0 slice 2's decision 8, for review): a cancel means **feed no more
and finish the sheet in flight** (proposal 10: "stops after the page in flight; pages so far stay", read as
the sheet): in duplex, a front already delivered is followed by its back, so "Scan More" never starts on a
back. The caller cancels through the batch's `Tracker`, from any thread (the worker's protocol thread does),
and a backend calls `check()` between sheets, never within one (below). Each backend does it its way: SANE by
starting no next sheet once the check fails and dropping the batch (`sane_cancel`), not by `sane-dl`'s token,
which stops at once and loses the sheet in flight (probe S1, below); TWAIN by `MSG_RESET` once the sheet's
transfers end; WIA by refusing the next sheet's stream; ICA to confirm. Stopping mid-page is the watchdog's: it
kills the worker (proposal 4).

**To settle:** whether each API can finish the sheet in flight and feed no more, or only stop at once (S0's SANE
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

**Cancel and progress through `in-progress`, the model's** (proposed for slice 3 on 2026-11-08, asked that day
whether `in-progress` fits; taken by S0 slice 2, its decision 10, for review, since slice 3's API needs an answer):
the `in-progress` crate (github.com/sheaf-pdf/in-progress), which `scan-cleanup` already takes. `Device::scan`
takes a `&dyn Progress`; the caller holds the `Tracker`, the `Send + Sync` handle any thread can cancel, and makes
a new one for each batch, since a cancelled tracker stays cancelled. A backend calls `check()` between sheets,
never within one, which is "finish the sheet in flight"; progress is each page's bytes, `set_total(Some(page
bytes), Unit::Bytes)` (`None` while a page's length isn't known) and `set(0)` as it begins, then `inc` per buffer
or `set`; and the intake pipeline could give scanning and cleanup weighted children of one tracker. A push
backend's own thread reports through `Progress::child`, an owned `Box<dyn Progress>` cancelled with its parent,
moved to that thread with a clone of the `SyncSender` for the batch. Two seams: its progress is polled, not pushed,
so `scan-worker` polls the worker's tracker and forwards snapshots over its pipe, or implements `Progress` itself
to push lines (slice 4's choice); and a flag can't wake the TWAIN thread blocked in `GetMessageW`, a posted message
can, while `in-progress` calls nothing when cancelled, so the `twain` backend looks at the flag wherever its loop
wakes (a source's message, a request, a timer) or slice 9 adds a wake-up to the model. The proposal had the TWAIN
backend post a message to its window as well, which nothing in `in-progress` would call it to do: the one change
here to what slice 9 meets of it.

## 8. Threads and events

| API | How |
|---|---|
| SANE | Pull, on the caller's thread: one thread per handle, calls never overlapping; `sane_cancel` the one call allowed from elsewhere. `sane-dl` takes one lock for every call but `sane_cancel`, whatever the thread or device (`Sane` is `Send + Sync`; a `Handle` is `Send` and used from one thread at a time), and its token cancels from any thread. A read that gives no bytes is made again after a pause growing to 10 ms, with no cap, so a backend answering so while its lamp warms up is waited on (S0 slice 1's decision 58); a read that never returns is the watchdog's. The `test` backend 1.4.0 can deadlock as a frame ends (UP-8), so tests and probes pace its reads. |
| TWAIN | Push, through a window's message loop on one STA thread that makes every DSM call; `MSG_XFERREADY`, `MSG_CLOSEDSREQ` (the user closed the driver's window) and `MSG_DEVICEEVENT` arrive there. Without a callback, every window message is forwarded through `MSG_PROCESSEVENT` while the source is enabled, and a waiting source message comes back with whichever is forwarded first (on a new window, the desktop window manager's `WM_DWMNCRENDERINGCHANGED`; later, the DSM's own `WM_NULL`), the carrier not to be dispatched. With TWAIN 2's `DAT_CALLBACK2`, registered on the source in state 4, the DSM calls the callback on the thread that called into it, inside that call (for the sample, inside `MSG_ENABLEDS`), and hands its return to the source, which fails the enable on anything but `TWRC_SUCCESS`: the callback only records and posts. Both worked with the sample, which signals synchronously; whether a callback is enough without the loop for sources that signal later, from their own windows or threads, is slice 12's, and the window and loop stay either way. The DSM changes the process's current directory to each source's folder while loading it, so the worker uses no relative paths. |
| WIA | Push, on an STA thread: `Download` is one blocking call that calls back on the same thread. |
| ICA | Push, through delegates on a run loop: the main thread's by default; in a worker, the worker's main thread runs it (whether that's enough is open, the survey). |

**The model** ("The model, for slice 3"): two traits, as SANE's library and handle are, and as a worker keeps
one device open while its dialog is (section 9): a `Backend` lists and opens, and the `Device` it opens gives
its caps and scans; both are `Send`, and a device is a `Box<dyn Device>`. `Device::scan(settings, first_sheet,
events, progress)` blocks its own thread until the batch is done, sending `Applied`, `Sheet`, `Page` and
`Status` events on a bounded channel (a `std::sync::mpsc::SyncSender<Event>`, its bound the caller's: a `send`
waits while the channel is full, and one that fails, nobody listening, ends the batch as `Done::Cancelled`, no
sheet started after it), with cancel and progress through `in-progress`'s `Progress` (section 7), not events
or a handle of the model's own. The SANE backend runs on that thread directly; the TWAIN, WIA and ICA backends
start the thread their API needs (an STA thread with a message loop or a run loop), hand it a clone of the
`SyncSender`, which holds it back while the channel is full, and a child progress for each batch, and pass
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
it is a backend call, and its settings then apply to scans until changed (proposal 7). Since S0 slice 2's code
review (decision 40, for review) the call takes the window to own it (`Device::settings_window(owner)`, a
`WindowOwner`: a native window's handle as an integer, an `HWND`'s value on Windows, which the worker passes to
the driver; `None` for none) and answers how the window closed (`WindowClosed::Accepted`, the settings kept, or
`Dismissed`); a device without one answers `ScanError::Unsupported`.

**To settle:** which window owns the dialog: the worker's hidden window owns it, so it can open behind the app;
passing the app's window handle to the worker for the driver to use as its owner is the likely answer (S0 slice 9).
The model takes the owner already (above), so slice 9 chooses only which window the app passes.

## 10. What drivers report beside a page

TWAIN's `TWEI_PATCHCODE` and barcode text through `DAT_EXTIMAGEINFO`, and WIA 2.0's patch code reader
(results as XML or raw); SANE has nothing; ICA, to confirm. **The model:** a list of what the driver reported
on the page, empty unless asked for; hints only, since `patch-codes` reads every edge and says which
(FEAT-5). Later: a field of `RawPage` and a setting of `Settings`, both `#[non_exhaustive]`, so adding them breaks
no caller (S0 slice 2's code review).

## The model, for slice 3

**Written by S0 slice 2, for review** (its decisions in `slices/s0-2-seam-note.md`): what `scan-device` 0.0.x
builds, proposal 9's model with this note's changes ("Changes from proposal 9's model"). Names are the
design's: slice 3 may rename and says so, and the model stays provisional until slice 11 freezes it. Plain
data, every field public but `RawImage`'s; an enum or a struct that may grow is `#[non_exhaustive]`; pixel
sizes, resolutions and counts `u32`, the same in a 32-bit worker as in the 64-bit app (slice 9), but a page's
`dpi`, `f32` as TWAIN's resolution is a fraction (`TW_FIX32`) and SANE's may be (`FIXED`); paper sizes `f32`
millimetres. The block leaves out bodies, constructors and `RawImage`'s methods; probe M1 (the slice's "So
far", revised with its code review's fixes) compiled it as it stands, formatted, with them added, against
`in-progress` 0.1.0, and tested the shapes the bullets after it claim.

```rust
use std::{num::NonZeroU32, sync::mpsc::SyncSender, time::Duration};

use in_progress::Progress;

// Devices (section 1): an id is `<driver>:<native id>`, compared whole. A `DeviceInfo` is what a listing
// gives and what the app stores, and gives back to `Backend::open`.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct DeviceId(pub String);
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct DeviceInfo {
    pub id: DeviceId, pub name: String, pub maker: String, pub model: String, pub driver: Driver,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Driver { Sane, Twain { bits: u8 }, Wia, Ica, Fake }

// What a device can do, per source, and what it's asked for (section 2). A struct that may grow is
// `#[non_exhaustive]`, made outside the crate by a constructor (`Settings::new`) or by decoding.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct Caps { pub sources: Vec<SourceCaps>, pub settings_window: bool }
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct SourceCaps {
    pub source: Source, pub duplex: bool, pub resolutions: Resolutions, pub formats: Vec<Format>,
    pub encodings: Vec<Encoding>, pub max_area: Option<SizeMm>, pub paper_auto: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Source { Flatbed, Feeder }
/// Whole dpi: a list never empty, a range's `min` never past its `max`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Resolutions { List(Vec<u32>), Range { min: u32, max: u32, step: NonZeroU32 } }
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SizeMm { pub width: f32, pub height: f32 }
impl SizeMm {
    pub const LETTER: SizeMm = SizeMm { width: 215.9, height: 279.4 };
    pub const LEGAL: SizeMm = SizeMm { width: 215.9, height: 355.6 };
    pub const A4: SizeMm = SizeMm { width: 210.0, height: 297.0 };
}
/// `encodings`: the encoded forms the app takes, in its order; empty (`Settings::new`'s) for raw only.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct Settings {
    pub source: Source, pub duplex: bool, pub dpi: u32, pub format: Format, pub paper: Paper,
    pub encodings: Vec<Encoding>,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Paper { Auto, Size(SizeMm) }
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct Applied { pub settings: Settings, pub clamped: Vec<Clamped> }
/// What was asked of a setting the device changed; what it took is in `Applied::settings`.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum Clamped {
    Source(Source), Duplex(bool), Dpi(u32), Format(Format), Paper(Paper), Encodings(Vec<Encoding>),
}

// The raw layout (section 3).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Format { Bw1, Gray8, Gray16, Rgb8, Rgb16 }
/// Private fields: `RawImage::new` checks them. `Debug` by hand: the format, the size, the length.
#[derive(Clone, PartialEq, Eq)]
pub struct RawImage { format: Format, width: u32, height: u32, data: Vec<u8> }
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum LayoutError {
    #[error("an image has at least one row and one column")]
    Empty,
    #[error("{width} by {height} is more than this system can hold")]
    TooLarge { width: u32, height: u32 },
    #[error("{expected} bytes expected, {got} given")]
    Length { expected: u64, got: u64 },
    #[error("row {row}'s bits past its last pixel aren't 0")]
    Padding { row: u32 },
}

// Pages (sections 3 and 4).
/// `dpi` finite and above 0: the worker's client refuses others as it decodes.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct RawPage {
    pub pixels: Pixels, pub dpi: (f32, f32), pub sheet: u32, pub side: Side, pub side_known: SideKnown,
}
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Pixels { Raw(RawImage), Encoded(Encoded) }
/// `Debug` by hand, as `RawImage`'s: the kind and the length.
#[derive(Clone, PartialEq, Eq)]
pub struct Encoded { pub kind: EncodedKind, pub bytes: Vec<u8> }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum EncodedKind { Jpeg, Png, Tiff, Bmp, CcittG4 { width: u32, height: u32, black_is_1: bool } }
/// An encoded form, as the app asks for it and a source offers it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Encoding { Jpeg, Png, Tiff, Bmp, CcittG4 }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side { Front, Back }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SideKnown { Reported, Inferred }

// Events and outcomes (sections 5 to 9).
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum Event { Applied(Applied), Sheet(u32), Page(RawPage), Status(Status) }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Status { WarmingUp }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Done { FeederEmpty, OnePage, Cancelled }
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum ScanError {
    #[error("the feeder is empty")]
    FeederEmpty,
    #[error("the paper jammed")]
    Jammed,
    #[error("two sheets fed at once")]
    DoubleFeed,
    #[error("the cover is open")]
    CoverOpen,
    #[error("the scanner is busy")]
    Busy,
    #[error("the scanner isn't there")]
    Gone,
    #[error("the device id isn't one this driver gives")]
    BadId,
    #[error("the scanner wouldn't start with these settings")]
    SettingsRefused,
    #[error("the scanner refused access")]
    AccessDenied,
    #[error("the driver didn't answer in time")]
    TimedOut,
    #[error("the scanner can't do that")]
    Unsupported,
    #[error("{what} isn't installed")]
    NotInstalled { what: String },
    #[error("{message}")]
    Driver { message: String, shown: bool },
}
/// How the driver's settings window closed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum WindowClosed { Accepted, Dismissed }
/// The app's window, to own the driver's: a native handle as an integer (an `HWND` on Windows).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WindowOwner(pub u64);

// The traits (section 8).
pub trait Backend: Send {
    fn list(&mut self, deadline: Duration) -> Result<Vec<DeviceInfo>, ScanError>;
    fn open(&mut self, device: &DeviceInfo, deadline: Duration) -> Result<Opened, ScanError>;
}
pub struct Opened { pub device: Box<dyn Device>, pub info: DeviceInfo, pub renamed_from: Option<DeviceId> }
pub trait Device: Send {
    fn caps(&mut self) -> Result<Caps, ScanError>;
    fn scan(
        &mut self, settings: &Settings, first_sheet: u32, events: &SyncSender<Event>, progress: &dyn Progress,
    ) -> Result<Done, ScanError>;
    fn settings_window(&mut self, owner: Option<WindowOwner>) -> Result<WindowClosed, ScanError>;
}
```

- **Ids and opening** (section 1): `Backend::list` is a snapshot under its deadline. `Backend::open` takes the
  `DeviceInfo` the app stored and a deadline; it refuses an id its driver doesn't form (`BadId`), answers
  `TimedOut` once the deadline passes (the backend then dropped with its worker), and where it knowingly opens
  another device in the stored one's place (SANE's fallback) says so in `Opened::renamed_from`; `Opened::info`
  is the device as the backend found it, or the stored one where it couldn't check (section 1). The deadline
  bounds what a backend can bound (SANE's listing, TWAIN's DSM walk, ICA's browse); a driver's call that never
  returns is the watchdog's. A worker keeps one `Box<dyn Device>` open while its dialog is, and lists in no
  worker that holds an open device (a listing and a scan in one process wait on each other, section 1).
- **What may grow** (decision 36): `DeviceInfo`, `Caps`, `SourceCaps`, `Settings`, `Applied` and `RawPage` are
  `#[non_exhaustive]`, as the enums that may grow are, so a field added later (section 10's) breaks no caller.
  Code outside the crate makes them by a constructor (`Settings::new(source, dpi, format)`,
  `DeviceInfo::new(id, name, maker, model, driver)`) or by decoding (slice 4, below), and sets their public
  fields.
- **`RawImage`** (section 3): `RawImage::new(format, width, height, data)` checks the layout, and
  `format()`, `width()`, `height()`, `stride()` (a row's bytes, from the format and the width), `data()` and
  `into_data()` read it. Its `Debug`, and `Encoded`'s, are written by hand, the format, the size and the length
  and never the bytes (decision 39): a page is up to about 100 MB, and `{:?}` of an event, a log line or an
  `assert_eq!` that fails would print it all.
- **A batch** (`Device::scan`, sections 4 to 8): events on a bounded channel (a `SyncSender<Event>`, the caller
  choosing the bound with `sync_channel(n)`; decision 30): a `send` waits while it's full, so a pull backend
  starts no next sheet before its pages are taken, and a push backend's thread waits the same, holding its
  driver back, where an unbounded `Sender` would grow by a page for each the consumer fell behind; a `send`
  that fails (the receiver gone) ends the batch, no sheet started after it, as `Done::Cancelled`.
  `Event::Applied` once the settings are set, before the first sheet; `Event::Sheet(n)` as sheet `n` begins,
  counted on from `first_sheet`; `Event::Page` for each page, whole, once complete, `Pixels::Encoded` only in a
  form `Settings::encodings` asks for; `Event::Status` for what the user should know meanwhile (`WarmingUp`).
  It returns a `Done` (section 5) or a `ScanError` (section 6): `FeederEmpty` only before any page, the pages
  sent before an error kept.
- **Cancel and progress** (section 7), through the `Progress` it's given: `check()` between sheets, never
  within one, and `Done::Cancelled` once it fails; each page's bytes reported. The caller cancels through the
  batch's `Tracker`, from any thread; a cancelled tracker stays cancelled, so each batch has its own. A push
  backend's thread gets `progress.child(..)` (an owned `Box<dyn Progress>`, `Send` and `'static`, cancelled
  whenever its parent is) and a clone of the `SyncSender`, moved into a thread spawned for the batch or sent to
  one that outlives it, as a TWAIN thread per open source would (M1 built and ran both).
- **The settings window** (section 9): `Caps::settings_window` says the driver has one;
  `Device::settings_window(owner)` opens it, owned by the app's window where `owner` gives one, and answers
  `WindowClosed::Accepted` (its settings kept, applying to scans until changed) or `Dismissed`; a device
  without one answers `ScanError::Unsupported`.

**What the `sane` backend maps** (S0 slice 5; the SANE rows of sections 1 to 8):

| `sane-dl` gives | The model |
|---|---|
| `Device { name, vendor, model, kind }` | `DeviceInfo`: `id` `sane:` and the name's bytes as Latin-1; `name` the bytes as UTF-8 where they are, else as Latin-1; `maker` and `model` the vendor and model; `driver` `Sane` |
| `Backend::open(device, deadline)` with a stored `sane:` id, in the worker started for it (section 1) | refused first where the next row says (`BadId`); then the id's backend listed under the deadline, before anything opens; the stored name listed with the stored vendor and model opened, `info` the listing's; listed with another, taken as gone; not listed, opened by name all the same, `info` the stored one, unchecked |
| `Error::Status { INVAL, Open }` from a stored id, or its name taken as gone | the fallback over that listing (section 1), only from a USB connection's name (`libusb:` after the backend) to the one listed name of that form whose `backend_of` is the id's, with the stored vendor and model, said in `Opened::renamed_from`; else `Gone` |
| A stored id refused before `sane_open` (section 1): not `sane:` and Latin-1 (a character past U+00FF), a `NUL` (`Error::NulInName`, where one reaches `sane-dl`), `dll`'s testing prefixes, a leading `:`, the empty name | `BadId` |
| `Error::DeadlineExceeded` from a listing, in `list` or `open`; `Error::ListingPending` after it | `TimedOut`, the backend then dropped with its worker |
| `LoadError::NotFound`; the other `LoadError`s | `NotInstalled { what }`; `Driver` |
| An option's descriptor and constraint; a set's `SetInfo` (`INEXACT`, `RELOAD_OPTIONS`) | `SourceCaps` (section 2); `Applied` and its `Clamped`; no area options, `max_area` `None`; no `mode`, the format `Handle::parameters` gives before the start; no `source`, one flatbed |
| `Error::StartInvalid` (the descriptors read again) | the settings set again and the start tried once more; `StartInvalid` again, `SettingsRefused` (section 6's table); a re-read that failed (`reload`), its error mapped as any other |
| `Page::Image(Image)`, `GRAY` or `RGB` at depth 1, 8 or 16 | `Event::Page`, `Pixels::Raw`, converted as section 3's table says |
| `Progress { bytes_read, bytes_expected }` | `set_total(bytes_expected, Unit::Bytes)` whenever it changes, and `set(bytes_read)` |
| `Page::NoDocs` from a feeder, with `pages` 0; with pages before it | `ScanError::FeederEmpty`; `Done::FeederEmpty` |
| `Error::Status { ACCESS_DENIED, .. }` | `AccessDenied` |
| `Error::Status { status, call }`, the other statuses | section 6's table |
| `Error::Cancelled` (the token's, which the model's cancel doesn't use) | `Done::Cancelled` |
| Every other `sane_dl::Error`: a page's `ShortFrame`, `LongFrame`, `PartialRow`, `BadReadLength`, `BadParameters` (`2500x0`), `LimitExceeded`; `BadDeviceList`, `BadHandle`, `Thread`; an option's refusals (`Inactive`, `NotSettable`, `WrongType` and the rest) and `ReloadAfterSet` | `Driver { message, shown: false }`, `sane-dl`'s message (for `LimitExceeded`, that a lower resolution asks for less), the page being read dropped and the pages before it kept (section 6) |

**What slice 4 needs of it** (`crates/scan-worker.md`'s protocol predates it; slice 4 writes its protocol from
this):

- every type plain data the worker can encode (decision 36): a `serde` feature of `scan-device` (leaning: the
  app stores `DeviceInfo` with it too, and the client then makes the `#[non_exhaustive]` structs by decoding,
  with no constructor of its own), or wire types of the worker's own with a test that round-trips every variant
  of every enum, since an enum's `#[non_exhaustive]` lets the client's conversions compile without a variant
  added here (slice 4's choice, said); either way a `RawImage` decoded on the client is made by
  `RawImage::new` (a `try_from`, never a derive that fills its private fields), so its checks hold there too,
  and the client refuses a `RawPage`'s `dpi` that isn't finite and above 0, an empty `Resolutions::List`, and a
  `Range` whose `min` is past its `max`, as it refuses a wrong length;
- a page's format, width and height before its bytes, so the client caps them before it reads them
  (`plan.md`'s hostile-input rule), and `RawImage::new` refusing a length that doesn't match: a crashed
  worker's half page is never a page;
- a page delivered whole, once complete, never cut short (section 3); every size `u32`, the same in the 32-bit
  worker; the worker's protocol thread draining the `SyncSender` as it writes its pipe, the bound the worker's
  to choose, so a slow pipe holds the backend back rather than filling the worker's memory;
- the client's own error type, wrapping `ScanError` beside the client's failures (decision 34): the watchdog's
  stop (proposal 9's `Stopped`, which isn't the model's), a crash with how the worker exited, a protocol
  mismatch (a version, a line it can't read), a page cut short; and the kill after a cancel allowing for the
  sheet in flight, which a cancel now finishes, front and back (section 7): `crates/scan-worker.md` kills a
  worker 10 s after a cancel (proposal 4), which a sheet's two sides may outlast on a slow scanner or at a high
  resolution, so slice 4 sets it anew, counted from the worker's last line, say, as the watchdog's 60 s is;
- the protocol's consequences, which `crates/scan-worker.md`'s list (`list`, `caps`, `scan`, `cancel`,
  `quit`; `device`, `caps`, `sheet`, `page`, `status`, `error`, `done`) predates: an `open` request carrying
  the stored `DeviceInfo` and the deadline, its answer carrying `renamed_from` and the `info` found, before
  `caps` and `scan` (the two traits); the worker chosen by the `DeviceInfo`'s `driver`; an `applied` event
  (`Event::Applied`); progress lines from the worker's `Progress`; `cancel` cancelling the batch's tracker; a
  `settings_window` request carrying the owner, answered with how the window closed (section 9); and a SANE
  worker that opens a stored id started with that id's one-backend `ConfigDir` (`Search::BeforeSystem`, naming
  the backend `backend_of` gives, or none where it gives none: section 1), passed at the spawn, in a directory
  it trusts: `BeforeSystem` has SANE read the process's current directory after the `ConfigDir` and before the
  system's, so a `dll.aliases` or a backend's `.conf` there would be read (`sane-dl`'s `Search`; S0 slice 1's
  security item 4).

**What slice 9's `twain` backend meets** (slice 7's rows, as they are): its tiles and DIB assembled into a
`RawImage` (section 3's table: a DIB's rows flipped where it's bottom up, padding dropped, tiles placed, the
DIB's B, G, R swapped); a DIB's palette and `ICAP_PIXELFLAVOR` read into the polarity; `TWEI_PAGESIDE`, where a
source fills it, as `SideKnown::Reported`, else `Inferred` (section 4); `TWCC_OPERATIONERROR` as `Driver {
shown: true }` (section 6); sizes per source, as `CAP_FEEDERENABLED` changes them (`SourceCaps`, section 2);
`ShowUI` and `CAP_ENABLEDSUIONLY` as `Caps::settings_window` and `Device::settings_window(owner)`, the source's
closing as `WindowClosed` (section 9); a pending count of 0xFFFF, a batch whose length isn't known, which the
model never asks for (it has no batch length); a fractional resolution as a page's `f32` `dpi`; and its STA
thread given each batch's child progress and `SyncSender` clone, on which it waits while the channel is full
(its driver's next transfer waiting too), the cancel's wake-up its own to settle (section 7).

## Changes from proposal 9's model

1. Capabilities per source, resolutions a list or a range (section 2).
2. `RawPage`'s pixels raw in one stated layout (`RawImage`, checked as it's made), or encoded with their kind;
   delivered whole (section 3). Revised by S0 slice 2: progress goes through `in-progress` (item 9), not an
   event of its own.
3. A page's side reported or inferred (`SideKnown`); drivers asked not to drop blank pages (sections 2 and 4).
4. `FeederEmpty` as an error before any page, a `Done` after (section 5).
5. `Driver` errors say whether the driver already showed its message; warming up waited on inside the
   backends, and said as a `Status` event, never an error (section 6).
6. Cancel is "feed no more, finish the sheet in flight", revised by S0 slice 2 from "the page in flight", so
   in duplex a delivered front's back comes too; stopping at once is the watchdog's (section 7).
7. The driver's own window as a capability (section 9); driver-reported metadata, later (section 10).

Added by S0 slice 2 ("The model, for slice 3"):

8. `Format`, the raw layout's five, is also what's asked for, in place of `Mode`, so 16-bit is asked for and
   clamped like any setting; paper `Auto` or a `SizeMm` in `f32` millimetres, Letter, Legal and A4 its
   constants (section 2).
9. Cancel and progress through `in-progress`'s `Progress`, which `scan` takes: the caller's `Tracker`,
   `check()` between sheets, each page's bytes (section 7).
10. `ScanError::NotInstalled` for a system without its driver API, and `ScanError::BadId` for a stored id the
    driver doesn't form (sections 1 and 6).
11. Two traits, `Backend` (list, open) and `Device` (caps, scan, the settings window), and a device opened in
    place of a stored id said (`Opened::renamed_from`; sections 1 and 8).

Added by S0 slice 2's code review (its decisions 27 to 41):

12. `Backend::open` takes the stored `DeviceInfo` and a deadline, and a deadline passed is
    `ScanError::TimedOut` (sections 1 and 8).
13. Events on a bounded channel (`SyncSender`), a `send` that fails ending the batch (section 8).
14. `Stopped` leaves `ScanError` for `scan-worker`'s client's own error; `SettingsRefused`, `AccessDenied` and
    `Unsupported` join it (section 6).
15. Encoded forms asked for (`Settings::encodings`, `SourceCaps::encodings`, `Encoding`); the structs that may
    grow `#[non_exhaustive]` (sections 2, 3 and 10).
16. The settings window owned by the app's window, and saying how it closed (section 9).

## Known unknowns

Each section's "To settle", gathered, with what settles it:

| Unknown | Settled by |
|---|---|
| SANE names that change on replugging, per backend; a fallback by vendor and model | **Answered, S0 slice 2** (section 1): each backend's names from its source or man page; `INVAL` at open for a name no longer there; the fallback by backend, vendor and model from a USB connection's name only, over a listing made before the open, in a worker started with the id's one-backend `ConfigDir` (revised by its code review). The fi-6130's name, and what replugging does to it: S0 slice 6 |
| Whether SANE can stop after the page in flight | **Settled on the `test` backend and the fake, S0 slice 2** (section 7): the page read to `EOF`, no next start, the batch ended, the sheets after left in the feeder. Whether a real feeder has pulled the next sheet in: S0 slice 6 |
| Whether `twain-dsm` hands over tiles or assembled images; what a DIB's palette becomes | **Settled, S0 slice 7:** tiles and the DIB's block; the backend assembles, decodes and applies the palette (section 3) |
| How TWAIN's 32- and 64-bit lists merge (leaning: `scan-worker`'s client) | S0 slice 9 |
| How events cross from the TWAIN thread to the backend's; posting cancels to its loop | **Settled, S0 slice 7:** a channel out; requests posted to the window, matched by window, kept while the loop waits, never forwarded; a cancel queued before the enable, and one during a memory transfer, shown (section 8). Serving requests while a driver is busy, slices 8 and 9 |
| Whether TWAIN can finish the page in flight and feed no more | **Settled for transfers, S0 slice 7** (`MSG_ENDXFER`, then `MSG_RESET`); whether the feeder stops, and `MSG_STOPFEEDER`, S0 slice 12 |
| Whether `DAT_CALLBACK2` is enough without the loop for sources that signal later | S0 slice 12; the loop stays meanwhile |
| The cancel handle and progress through `in-progress` | **Taken by S0 slice 2, for review** (section 7; "The model, for slice 3"); the TWAIN thread's wake-up on a cancel, S0 slice 9 |
| Which drivers fill `TWEI_PAGESIDE`; whether WIA or ICA say a page's side | S0 slice 12, the hand checks; the `ica` slice |
| "Continue" after a jam in TWAIN | S0 slice 12 (the sample source can't jam) |
| Compressed transfers (G4, JPEG) for the pipe's throughput, or raw only at first | S0 slice 4's throughput gate; slice 9 |
| The raw layout's 1-bit polarity and 16-bit byte order | **Settled, S0 slice 2, for review** (section 3): 0 the least intensity at every format, the leftmost pixel in the high bit, padding bits 0, 16-bit little-endian, rows top down and packed, R, G, B; held beside `scanimage`'s PNG |
| The driver window's owner across processes | **Taken by S0 slice 2's code review** (`WindowOwner`, section 9); which window the app passes, and the driver using it: S0 slice 9 |
| Every ICA name marked "to confirm"; its run loop in a worker | The `ica` slice (later) |
| Where driver-reported patch codes go (FEAT-5) | Later |
| What a duplex scanner gives after a cancel that lands after a front: its back, before the batch ends (the model's "sheet in flight") | S0 slice 6 (SANE, the fi-6130); S0 slice 12 (TWAIN) |
