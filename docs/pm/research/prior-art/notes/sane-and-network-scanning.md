# Prior art: SANE, the SANE network protocol, eSCL/AirScan and WSD scanning (Rust and other languages)

As of 2026-11-08. Scope: what exists for the planned `sane-dl`, `scan-device` and `scan-worker` crates
(`docs/pm/crates.md`, Devices section), verified against the 2026-10-23 audit
(`scratch/reference/sheaf/research/build-or-adopt/05-sane-linux.md`,
`https://github.com/sheaf-pdf/sheaf/blob/3232a35/plan/research/build-or-adopt/09-escl.md`,
`scratch/reference/sheaf/m05-scan/research/scanners.md`, proposals p06 and p22).
Crate sources were downloaded to a temporary directory and read, never built or run. Crate numbers come
from the crates.io API on 2026-11-08; repository numbers from the GitHub API the same day. Where a fact
comes from a search-engine summary that I did not open myself, it says so.

License rule used throughout: GPL, LGPL, AGPL and MPL code is "study only" (facts may be learned and cited,
code never copied, translated or linked). Permissive (MIT, Apache-2.0, BSD, ISC, Zlib, CC0, public domain)
may be ported. Loading the system's unmodified libsane at run time is allowed.

## 1. Which Rust crates bind SANE, and how good are they?

### Takeaway
There is still exactly one published SANE binding crate, `sane-scan` 0.1.2 (2023). It links libsane at
build time, runs bindgen against `/usr/include/sane/sane.h`, has no cancel from another thread, and has
several soundness bugs. Its own users report it fails under optimization, and its maintainer has left five PRs
unanswered since 2023. Since September 2026 there has been a wave of new Rust apps with their own private
SANE FFI, about 20 repositories against the audit's "about a dozen". A few are permissively licensed and
`dlopen` libsane with `libloading`: airbridge (MIT OR Apache-2.0), janus-scan (MIT) and ruscan (MIT). All
of them are days or weeks old, with 0 or 1 stars, and none is a published library. Verdict: **build our own
`sane-dl`**, as planned.

### Cited Findings
**`sane-scan` (the only published SANE binding)**
- Version 0.1.2, released 2023-03-10. Three releases in all: 0.1.0 and 0.1.1 on 2021-07-27, then 0.1.2. MIT on crates.io. 4,305 downloads in total, 24 in the last 90 days. No MSRV declared, no reverse dependencies — [crates.io/crates/sane-scan](https://crates.io/crates/sane-scan); [reverse deps API](https://crates.io/api/v1/crates/sane-scan/reverse_dependencies)
- The GitHub repository has no license file (the API reports `license: null`). It has 6 stars and 11 forks. The last commit on the default branch was on 2023-03-10 ("Bump version"), and the last push on 2024-05-11 — [github.com/aQaTL/sane-scan](https://github.com/aQaTL/sane-scan)
- `build.rs` runs `bindgen::builder().header("/usr/include/sane/sane.h")` with a hard-coded path, rustifies the `SANE_Status`, `SANE_Frame` and other enums, and emits `cargo:rustc-link-lib=sane`. So it needs libclang and libsane's development headers at build time, and links libsane — [sane-scan 0.1.2 build.rs](https://docs.rs/crate/sane-scan/0.1.2/source/build.rs)
- Coverage in `lib.rs` (445 lines): init and exit, the device list, open and close, option descriptors (range, word-list and string-list constraints), get, set and auto values, `sane_start`, `sane_get_parameters` and `sane_read`. It has no `sane_set_io_mode` or `sane_get_select_fd`, no multi-frame or three-pass assembly, and no tests. `sane_cancel` is called only from `Drop` and at EOF, and `DeviceHandle` holds a raw pointer, so it is `!Send` — [sane-scan 0.1.2 src/lib.rs](https://docs.rs/crate/sane-scan/0.1.2/source/src/lib.rs)
- Soundness and correctness problems seen in that `lib.rs`:
  - (a) `set_option` passes a pointer to a 1-byte Rust `bool` cast to `*mut SANE_Int`, so the backend reads 4 bytes from a 1-byte object. The standard says "`SANE_Bool` is simply an alias of `SANE_Word`" — [SANE standard api.rst](https://gitlab.com/sane-project/standard/-/blob/master/api.rst).
  - (b) `get_option` reads a `SANE_Word` as `*const bool`, which is invalid for any first byte other than 0 or 1.
  - (c) `read_to_vec` reserves `bytes_per_line as usize * lines as usize`. With `lines = -1` (unknown length, as hand scanners and feeders report it) this overflows into a huge capacity.
  - (d) `read_to_vec` calls `set_len` on uninitialized memory, then loops `while let Ok(Some(..))`, which silently swallows a `JAMMED` or `IO_ERROR` mid-page and returns a truncated image as `Ok`.
  - (e) A string value is set from a buffer sized to the string. The standard allows a shorter string for reading, but also says the value "may be modified by the backend if the value cannot be set exactly", which can write past that buffer — [SANE standard api.rst](https://gitlab.com/sane-project/standard/-/blob/master/api.rst).
  - (f) Range constraints become a Rust `min..max`, which is exclusive although SANE's max is inclusive.
  - (g) A null descriptor returns `Err(Error(Good))` — [sane-scan 0.1.2 src/lib.rs](https://docs.rs/crate/sane-scan/0.1.2/source/src/lib.rs)
- Issue #3: `get_options` segfaulted on an HP Laser MFP. It was fixed on main but not published until a user spent "> 3h of debugging", after which 0.1.2 was released — [sane-scan issue #3](https://github.com/aQaTL/sane-scan/issues/3)
- Five PRs have been open with no maintainer reply:
  - #4, `local_only` (2023-05)
  - #5, deps and CI (2024-05)
  - #6, header location (2025-02)
  - #7, bindgen 0.72 (2025-09)
  - #8, "Refactored the library with batch scanning in mind" (2025-12-22, +850/−444). Its author writes that batch scanning "is not possible in the current design, since `sane_cancle()` is called after the first page", and later adds that "compiled with -O3 this library just don't work. sane_start() returns E_INVAL" — [sane-scan PR #8](https://github.com/aQaTL/sane-scan/pull/8); [PR list](https://github.com/aQaTL/sane-scan/pulls)
- The fork behind PR #8 (jue89/sane-scan, 1 star, pushed 2025-12-22) has no license file and is not on crates.io — [github.com/jue89/sane-scan](https://github.com/jue89/sane-scan)

**Other crate names**
- `sane` 0.2.1 (2019) is a serialization format, not the scanner API. `libsane-sys` and `sane-sys` are not on crates.io; crates.io search for "sane", "libsane", "saned" and "sane scanner" returns only `sane-scan`, `ruscan` and unrelated crates — [crates.io search "sane"](https://crates.io/search?q=sane); [crates.io/crates/sane](https://crates.io/crates/sane)
- `ruscan` 0.1.0 (2026-02-11, MIT, 37 downloads) is a GTK4 app, not a library. It does `dlopen`: `Library::new("libsane.so.1").or_else(|_| Library::new("libsane.so"))` through `libloading`, with 36 `unsafe` sites in a 911-line `src/scanner/sane.rs`. Its repository has 1 star and was last pushed on 2026-02-15 — [crates.io/crates/ruscan](https://crates.io/crates/ruscan); [github.com/iruizr7/ruscan](https://github.com/iruizr7/ruscan)
- `libloading` 0.9.0 (2025-11-05, ISC, MSRV 1.88, 624M downloads, 1,646 reverse dependencies). The audit's "590M" is now stale — [crates.io/crates/libloading](https://crates.io/crates/libloading)

**Private SANE FFI in Rust apps** (GitHub code search for `sane_control_option language:rust`: 27 files in about 22 repositories) — [GitHub code search](https://github.com/search?q=sane_control_option+language%3Arust&type=code)
- tobhoster/airbridge: MIT OR Apache-2.0 in Cargo.toml, Apache-2.0 per GitHub. Created 2026-10-16, pushed 2026-11-07, 0 stars. An eSCL and AirPrint bridge daemon that loads libsane with `libloading` 0.9 and declares function-pointer types for the whole API. It has a `CancelHandle` that calls `sane_cancel` from any thread ("SAFETY: sane_cancel is allowed from any thread") and an `alive` flag "cleared before close so a late cancel never touches a closed handle". It uses `mdns-sd` 0.21.5 — [github.com/tobhoster/airbridge](https://github.com/tobhoster/airbridge); [src/backend/sane.rs](https://github.com/tobhoster/airbridge/blob/HEAD/src/backend/sane.rs)
- januschung/janus-scan: MIT, one commit on 2026-10-25, 0 stars. `crates/backend-sane/src/ffi.rs` says it loads libsane "at runtime with `libloading` so the crate builds and runs without libsane". There are integration tests against SANE's `test:0` with a private `SANE_CONFIG_DIR`, noting that the feeder "returns SANE_STATUS_NO_DOCS on every 11th `sane_start` after opening" — [github.com/januschung/janus-scan](https://github.com/januschung/janus-scan); [tests/sane_test_backend.rs](https://github.com/januschung/janus-scan/blob/HEAD/crates/backend-sane/tests/sane_test_backend.rs)
- nathawatc/sanetui: Apache-2.0, created and pushed on 2026-10-31, MSRV 1.95, 0 stars. Its `sane-sys` (with `publish = false`) has hand-written bindings and links at build time (`links = "sane"`, `cargo:rustc-link-lib=sane`). It also has a `tests/header_contract.rs` that "cross-checks the hand-written bindings against the installed `sane.h`", and an ABI test — [github.com/nathawatc/sanetui](https://github.com/nathawatc/sanetui); [crates/sane-sys](https://github.com/nathawatc/sanetui/tree/HEAD/crates/sane-sys)
- AfkaraLP/sane-scan-rs: no license, 1 star, 2026-05 to 2026-08. It binds `sane_get_select_fd` — [github.com/AfkaraLP/sane-scan-rs](https://github.com/AfkaraLP/sane-scan-rs)
- cschalle/negative-vibes: GPL-3.0, a GNOME negative-scanning app, 8 stars, pushed 2026-11-05. It has `nv-sane-sys` and `nv-sane`, plus an `nv-sane-portal` with a `sane_thread.rs`, i.e. SANE behind a separate service — [github.com/cschalle/negative-vibes](https://github.com/cschalle/negative-vibes)
- The rest have no license, are GPL, or are hours old:
  - ColinTimBarndt/libsane-rs: Apache-2.0, two days in 2024-06, unpublished. Confirms the audit — [github.com/ColinTimBarndt/libsane-rs](https://github.com/ColinTimBarndt/libsane-rs)
  - Masterchef365/libsane-rust: "VERY WIP", 2020, no license — [github.com/Masterchef365/libsane-rust](https://github.com/Masterchef365/libsane-rust)
  - NaniNoni/Powerscan: GPL-3.0 — [github.com/NaniNoni/Powerscan](https://github.com/NaniNoni/Powerscan)
  - vklachkov/mfp3000bot: Apache-2.0, a Telegram bot — [github.com/vklachkov/mfp3000bot](https://github.com/vklachkov/mfp3000bot)
  - florianbaer/scanner_app: no license, created 2026-11-06 — [github.com/florianbaer/scanner_app](https://github.com/florianbaer/scanner_app)
  - jasonulbright/Spectra-PDF: MIT, a Tauri PDF app — [github.com/jasonulbright/Spectra-PDF](https://github.com/jasonulbright/Spectra-PDF)

**`sane.h`**
- It declares 14 functions, not the "15" that crates.md and p06 state: `sane_init`, `sane_exit`, `sane_get_devices`, `sane_open`, `sane_close`, `sane_get_option_descriptor`, `sane_control_option`, `sane_get_parameters`, `sane_start`, `sane_read`, `sane_cancel`, `sane_set_io_mode`, `sane_get_select_fd`, `sane_strstatus` — [sane.h](https://gitlab.com/sane-project/backends/-/blob/master/include/sane/sane.h)
- Its notice: "This file is in the public domain. You may use and modify it as you see fit, as long as this copyright message is included and that there is an indication as to what modifications have been made (if any)." So the public-domain grant carries a notice-and-changes condition — [sane.h](https://gitlab.com/sane-project/backends/-/blob/master/include/sane/sane.h)
- `SANE_STATUS_WARMING_UP` (12) and `SANE_STATUS_HW_LOCKED` (13) sit under `#if 0` ("for later sane version, older frontends won't support"). They are not in the enum — [sane.h](https://gitlab.com/sane-project/backends/-/blob/master/include/sane/sane.h)
- The standard on `sane_cancel`: "It is safe to call this function asynchronously (e.g., from within a signal handler)… completion of this operation does *not* imply that the currently pending operation has been cancelled… a frontend must *not* call any other operation until the cancelled operation has returned." — [SANE standard api.rst](https://gitlab.com/sane-project/standard/-/blob/master/api.rst)

### Inferences
- `sane-scan` is not a base to build on. It links at build time, which the plan rules out; it carries undefined behaviour in its option-value marshalling; and it rustifies `SANE_Status` with bindgen. bindgen's own docs warn that a rustified enum is undefined behaviour when C passes a value outside it, and `sane.h` explicitly reserves 12 and 13 for later statuses. jue89's "-O3 … returns E_INVAL" fits the bool marshalling bug. Contributing upstream isn't realistic with five PRs unanswered over three years.
- Three permissive projects already show the planned shape: airbridge, janus-scan and ruscan, each `dlopen` plus a struct of function pointers. airbridge's cancel handle is the planned `Send` cancel token, and its `alive` flag addresses the standard's rule that nothing else may be called until the cancelled call returns, including `sane_close`. The token must be synchronized with close, not merely `Send`. These are small, permissive and readable, but each is days old with 0 stars and no outside users. Read them as cross-checks; don't depend on them.
- sanetui's header-contract test (Apache-2.0) is the one quality practice worth borrowing: it checks hand-written bindings against the installed `sane.h`. It fits the plan's "no bindgen", and could run in CI where `libsane-dev` is installed.
- Correct crates.md and p06: there are 14 functions, not 15, and the plan should keep `sane.h`'s notice and mark its changes, as the header's terms require.
- Bindings should treat `SANE_Status`, `SANE_Frame` and the like as plain integers with named constants, not as Rust enums built from FFI returns.
- **Verdict: SANE C API bindings and safe wrapper (`sane-dl`): build our own.** Nothing published is run-time loaded, sound and maintained. Optionally port sanetui's header-contract test idea (Apache-2.0, with attribution), and use airbridge and janus-scan (permissive) as cross-checks.

### Gaps
- I didn't read airbridge's or janus-scan's code in full, only their headers, cancel paths and tests, so their soundness is unjudged.
- GitHub code search is capped. Private SANE FFI may exist on Codeberg or GitLab; I searched GitHub only.

## 2. Is there a pure-Rust client for the SANE network protocol (saned, TCP 6566)? Spec license, difficulty, what it buys

### Takeaway
Nothing is published. There is a complete, unpublished pure-Rust implementation: `printscan-sane`, PR #20 in b1tninja/printscan, opened 2026-11-08, MIT OR Apache-2.0, no `unsafe`, about 2,300 lines plus about 850 lines of tests against a fake saned. It hasn't been tried on hardware. The SANE standard's text carries no license notice at all, but both protocol headers are public domain: `sane.h` and `sanei_net.h`, which defines the RPC structures. A client is a few thousand lines with a few traps the standard doesn't spell out. It buys an FFI-free path to scanners shared by a saned, and process isolation by construction. It doesn't remove sane-backends' C drivers for USB scanners: they still run inside saned on some Linux host. Verdict: **study only for now; use and contribute upstream if printscan-sane is published and the project wants remote saned**.

### Cited Findings
- The standard's chapter 5, "Network Protocol", covers data encoding plus the RPCs `SANE_NET_INIT`, `GET_DEVICES`, `OPEN`, `CLOSE`, `GET_OPTION_DESCRIPTORS`, `CONTROL_OPTION`, `GET_PARAMETERS`, `START`, `CANCEL`, `AUTHORIZE` and `EXIT`. The published version is 1.06 — [SANE standard](https://sane-project.gitlab.io/standard/)
- The standard's license: the site shows no copyright or license notice. The repository's `conf.py` sets `copyright = ''` and `html_show_copyright = False`. The repository has no LICENSE file (tree: `api.rst`, `net.rst`, `conf.py`, …). The GitLab project API reports no license, and the last activity was 2026-03-06 — [standard conf.py](https://gitlab.com/sane-project/standard/-/blob/master/conf.py); [standard repository](https://gitlab.com/sane-project/standard)
- `sanei_net.h`, which defines the RPC request and reply structures and `SANEI_NET_PROTOCOL_VERSION 3`, carries the same public-domain notice as `sane.h` — [sanei_net.h](https://gitlab.com/sane-project/backends/-/blob/master/include/sane/sanei_net.h)
- The wire format, per the standard:
  - big-endian words; the protocol version travels in the build-revision of `SANE_NET_INIT`'s version code
  - image data comes over a separate data port that the daemon names in the `START` reply, as records with a 4-byte big-endian length, where `0xffffffff` ends the stream
  - the `START` reply's `byte_order` word is 0x1234 (little-endian) or 0x4321 (big-endian), and "the client is responsible for adjusting the byte-order"
  - authorization sends plaintext passwords, or an MD5 digest when the server prefixes the resource with `$MD5$` — [SANE standard net.rst](https://gitlab.com/sane-project/standard/-/blob/master/net.rst)
- A detail the standard's text leaves out: after the `0xffffffff` length, libsane's own net backend "read[s] the status byte", one more byte carrying the `SANE_Status` that ended the frame — [backend/net.c](https://gitlab.com/sane-project/backends/-/blob/master/backend/net.c)
- **printscan-sane**:
  - PR #20 "sane: an experimental SANE network-protocol backend (not published)", opened 2026-11-08 on branch `sane-backend`: +4,552 lines over 26 files
  - Its README: "Pure Rust, no `unsafe`, no `libsane` at build or run time … scans from any machine that can reach a `saned`, the local one (`127.0.0.1`) included". Requires Rust 1.95
  - Layers: `wire` (bounded decoding with `Limits`: 64 KiB strings, 4,096-element arrays, 1 GiB images), `protocol` (803 lines), `client` (388), and `scan` (frames to PNG, three-pass interleaved, 1-bit inverted, 16-bit byte order from the start reply)
  - Its `DataStream` handles "the length `0xFFFFFFFF` and one status byte"
  - Missing: credentials, non-standard options, and live testing ("Not tried against real hardware yet"). There are 27 unit and 18 integration tests against "a fake `saned` written in the test (its own encoder, separate from the crate's)" — [printscan PR #20](https://github.com/b1tninja/printscan/pull/20); [crates/printscan-sane on the branch](https://github.com/b1tninja/printscan/tree/sane-backend/crates/printscan-sane)
- **saned deployment facts**:
  - It runs from inetd or systemd, or standalone with `-l`. `-p` sets the port ("A value of 0 tells saned to pick an unused port"), `-o` "exits after the first client disconnects", and `-n` is needed to let saned use network scanners ("By default this is forbidden")
  - The access list lives in `saned.conf`, and "Connections from localhost are always permitted"
  - `data_portrange` exists for firewalls
  - "If saned is exposed directly on the network, other users may be able to intercept scanned images, or learn passwords" — [saned.man](https://gitlab.com/sane-project/backends/-/blob/master/doc/saned.man)
- In Debian, `saned` and `scanimage` ship in `sane-utils`, separate from `libsane1` (sane-backends 1.4.0-1) — [Debian control](https://sources.debian.org/src/sane-backends/1.4.0-1/debian/control/); [sane-utils.install](https://sources.debian.org/src/sane-backends/1.4.0-1/debian/sane-utils.install/)
- The newest sane-backends release is 1.4.0 (2025-05-24); the previous ones were 1.3.1 (2024-05-23) and 1.2.1 (2023-02-06) — [sane-backends releases](https://gitlab.com/sane-project/backends/-/releases)
- Other-language saned clients, for scale:
  - node-sane-net (JavaScript, MIT, 2017, 1 star) — [github.com/moritzmhmk/node-sane-net](https://github.com/moritzmhmk/node-sane-net)
  - SANEWinDS (VB.NET, GPL-3.0, by NAPS2's author), a TWAIN data source that talks to saned — [github.com/cyanfish/SANEWinDS](https://github.com/cyanfish/SANEWinDS)
  - wiasane (C++, a WIA driver over saned, last push 2017) — [github.com/mback2k/wiasane](https://github.com/mback2k/wiasane)
  - Wireshark has a SANE dissector (`packet-sane.c`), found by code search
- GitHub code search finds no Rust code using `SANE_NET_OPEN` (0 results) — [GitHub code search](https://github.com/search?q=SANE_NET_OPEN+language%3Arust&type=code)

### Inferences
- **Difficulty.** printscan-sane suggests that a correct client with bounded decoding, statuses, options, frames and cancel is about 1,800 lines of protocol, wire and client code. That is smaller than the libsane FFI layer plus its safe types. The traps to know:
  - the trailing status byte, which isn't in the standard's prose
  - the data connection to a second port that the daemon names (printscan pins it to the control connection's IP)
  - the byte-order word
  - protocol version 3 in the build-revision
  - MD5 authorization
  - cancel by shutting the data socket
- **The spec's license.** The standard has no stated license, but the protocol's structure definitions are in two public-domain headers, so a clean implementation can cite `sanei_net.h` and the standard for facts. Reading `net.c` or `saned.c` (GPL with the SANE exception) is study only.
- **What it buys:**
  - (a) No `unsafe` and no libsane in any process sheaf ships.
  - (b) Scanners shared by a saned on another machine, reachable from Windows and macOS as well.
  - (c) Isolation by construction: backends run inside saned, so a corrupting backend can't touch the client.
  - (d) A possible alternative worker. sheaf could spawn the system's `saned -l -o -b 127.0.0.1 -p <port>`, an unmodified, separately shipped GPL program run as a process, and speak the protocol to it, instead of loading libsane into its own worker. This is untested.
- **What it doesn't buy:**
  - (a) Local USB scanners still need sane-backends' C drivers. They run inside saned instead of the worker, and saned must be installed (`sane-utils`, not `libsane1`) and either enabled as a service or spawned.
  - (b) `select`/non-blocking control over the backend: saned uses its own read loop.
  - (c) Confidentiality on a network: there is none.
  - (d) Anything the SANE protocol can't carry. For example, the fujitsu man page says "JPEG output is supported by the backend, but not by the SANE protocol" ([sane-fujitsu.man](https://gitlab.com/sane-project/backends/-/blob/master/doc/sane-fujitsu.man)), and the same holds for in-process libsane.
- **Verdict: SANE network protocol client: study only now. If printscan-sane is published and remote saned is wanted, use it and contribute upstream.** It is permissive, no `unsafe`, and close to the plan's needs. It fits a different device model (printscan's encoded pages), and it is one day old and has never touched a real saned. Building our own is the fallback, at the size estimated above.

### Gaps
- I didn't verify that an unprivileged `saned -l -o` spawned per scan works on stock Debian/Ubuntu (device permissions, `saned.conf` lookup through `SANE_CONFIG_DIR`, or how a client learns the port chosen by `-p 0`).
- I didn't confirm the standard's licensing intent with the SANE project (the read-only constraint), so "no license notice" is all that can be said.

## 3. Is there a pure-Rust USB scanner driver? Is a clean-room Fujitsu fi-series driver plausible?

### Takeaway
There is one serious pure-Rust scanner driver over USB: `nkscan`, for Nikon Coolscan film scanners, with no document scanner support. It is written against Nikon's documented protocol on `nusb` and has frequent releases. `pixma-rs` is a pure-Rust Canon PIXMA driver, but over the network (CHMP over HTTP), not USB. No Rust driver exists for any document scanner. The closest permissive document-scanner driver is in Go: scan2drive's `fss500` (Apache-2.0), a Fujitsu ScanSnap iX500 driver "implemented from scratch based on USB traffic captures". A clean-room fi-series driver is plausible for a few models: the protocol is SCSI-over-USB, older Fujitsu OEM manuals document the SCSI command set, and fss500 proves the approach. Covering the family as SANE's fujitsu backend does (10,318 lines, models back to the M309x) is a large, hardware-bound project. Verdict: **don't build now; if ever, port fss500 (Apache-2.0) and extend from our own USB captures; SANE's fujitsu backend is study only**.

### Cited Findings
- **nkscan**:
  - 0.19.0 (2026-11-08), 17 releases since 2026-08-19, MIT OR Apache-2.0, 285 downloads. Repository: 63 stars, 15 forks, 14 open issues, pushed 2026-11-08 — [crates.io/crates/nkscan](https://crates.io/crates/nkscan); [github.com/activexray/nkscan](https://github.com/activexray/nkscan)
  - Its transport is "SCSI-over-USB … implemented with nusb", following Nikon's LS5K handshake tables ("LS5K 1-1-2 phase handshake"). Other transports: Linux `sg_io` (2 `unsafe`), macOS IOKit SCSITask (31 `unsafe`, vtables "transcribed from `IOKit/scsi/SCSITaskLib.h`"), Windows SCSI pass-through (4 `unsafe`), and WebUSB on wasm32
  - Python bindings come through pyo3, and the toolchain is pinned to 1.99
  - Its README lists `sane-coolscan3` among "Related Projects and References", alongside a Coolscan reverse-engineering project — [nkscan README](https://github.com/activexray/nkscan#related-projects-and-references)
- **pixma-rs** (MIT OR Apache-2.0, 24 stars, pushed 2026-07-17): "First open-source implementation of Canon's CHMP scanning protocol, reverse-engineered from network captures". It is network-only (CHMP over HTTP port 80, BJNP), and its `pixma-bridge` re-exports the scanner as eSCL to macOS — [github.com/pdrgds/pixma-rs](https://github.com/pdrgds/pixma-rs)
- **USB crates**:
  - `nusb` 0.2.9 (2026-10-24): Apache-2.0 OR MIT, MSRV 1.85, 2.1M downloads, pure Rust — [crates.io/crates/nusb](https://crates.io/crates/nusb)
  - `rusb` 0.9.4 (2024-04-27): MIT, 10.1M downloads, wraps libusb (C) — [crates.io/crates/rusb](https://crates.io/crates/rusb)
- crates.io search for "scanner driver" finds only `nkscan`, plus fingerprint, barcode and RFID drivers (for example `futronic-fs80h-rs`, "Pure Rust userspace USB driver for Futronic FS80H … fingerprint scanners", published 2026-11-08) — [crates.io search](https://crates.io/search?q=scanner%20driver)
- GitHub code search for "ScanSnap" with rusb or nusb in Rust: 0 results. For Fujitsu's USB vendor id `0x04c5` in Rust: only unrelated hits — [GitHub code search](https://github.com/search?q=0x04c5+language%3Arust&type=code)
- **scan2drive `fss500`** (Go, Apache-2.0, 172 stars, pushed 2026-09-02): "Package fss500 is a driver for the Fujitsu ScanSnap iX500 document scanner, implemented from scratch based on USB traffic captures. Terminology has been chosen to be consistent with the SANE fujitsu driver's terminology where appropriate." It is about 26 KB of Go plus a roughly 5.7 KB Linux usbdevfs layer (`usb_linux.go`) — [scan2drive internal/fss500](https://github.com/stapelberg/scan2drive/tree/HEAD/internal/fss500); [github.com/stapelberg/scan2drive](https://github.com/stapelberg/scan2drive)
- **SANE's fujitsu backend**:
  - backend version 140, `fujitsu.c` at 10,318 lines, GPL-2.0+ with the SANE exception
  - "supports every known model which speaks the Fujitsu SCSI and SCSI-over-USB protocols", and "the best way to determine level of support is to test the scanner directly, or to collect a trace of the windows driver in action"
  - "Network interfaces are not supported on any scanner model"
  - "Hardware donated, software image enhancement and fi-6/7xxx support funded by: PFU America, Inc." — [sane-fujitsu.man](https://gitlab.com/sane-project/backends/-/blob/master/doc/sane-fujitsu.man); [backend/fujitsu.c](https://gitlab.com/sane-project/backends/-/blob/master/backend/fujitsu.c)
  - Its sources are `Flatbed`, `ADF Front`, `ADF Back`, `ADF Duplex`, `Card Front` and `Card Back`, and `page-height` is set inactive when the source is the flatbed — [backend/fujitsu.h](https://gitlab.com/sane-project/backends/-/blob/master/backend/fujitsu.h); [backend/fujitsu.c](https://gitlab.com/sane-project/backends/-/blob/master/backend/fujitsu.c)
- **Public Fujitsu documentation** (from search results, not opened by me): OEM manuals exist for older SCSI models, for example an M3097G OEM manual (spec 50FH5043E) and an M3099GX/GH manual with a "SCSI interface specifications" chapter. They list commands such as INQUIRY, SET WINDOW, READ, MODE SELECT, OBJECT POSITION and REQUEST SENSE. No fi-series interface specification turned up — [manualslib M3097G OEM manual](https://www.manualslib.com/manual/55057/Fujitsu-M3097g.html)
- **PFU ships its own SANE driver for Linux**, "Image Scanner Driver for Linux", a library with a "SANE API based application interface":
  - It supports fi-7700S, fi-7480, fi-7460, fi-8290, fi-8270, fi-8250, fi-8190, fi-8170, fi-8150, fi-8040, fi-800R and SP-1425, on Ubuntu 20.04, 22.04 and 24.04
  - It "support[s] USB only" for fi-8290, fi-8270, fi-8250, fi-8190, fi-8170 and fi-8150 — [PFU multi-OS driver page](https://www.pfu.ricoh.com/fi/software/multios-driver/)
  - The AUR packages it as `pfufs` (search result) — [aur.archlinux.org/packages/pfufs](https://aur.archlinux.org/packages/pfufs)
  - A search summary says PFU's fi-7160/fi-7260 page lists version 2.8.2, dated 2026-04-02; I didn't open that page — [PFU download page](https://www.pfu.ricoh.com/global/scanners/fi/dl/ubuntu-fi-7x60.html)

### Inferences
- **Precedents.** Pure-Rust scanner drivers from vendor documentation (nkscan) or from captures (pixma-rs, and fss500 in Go) are feasible. Both nkscan and fss500 are single-family drivers.
- **What a clean-room fi driver needs:**
  - the SCSI command set, partly documented by Fujitsu's older OEM manuals
  - per-model quirks, inquiry pages and vendor-specific data, which SANE's backend accumulated over two decades from donated hardware and Windows-driver traces
  - our own USB captures of PaperStream IP on each target model
- **Legal and practical position.** A Rust port of fss500 (Apache-2.0) is allowed. Its comment says it borrows SANE's terminology "where appropriate", which isn't a copy of code, but expect reviewers to ask. The office's scanners would still run better through SANE's fujitsu backend or PFU's own `pfufs` backend, both reachable through libsane at run time.
- **Verdict: pure-Rust USB driver for the fi-series: don't build now.** If ever wanted (for example, Windows without PaperStream, or macOS), port scan2drive's fss500 (Apache-2.0) to `nusb`, after nkscan's transport layering (MIT OR Apache-2.0, portable), and extend from our own captures. SANE's fujitsu backend is study only.

### Gaps
- Whether any fi-series interface specification (SCSI command reference) is publicly downloadable. Searches found only the M309x OEM manuals.
- Whether `pfufs` is free to redistribute, and its license terms: PFU's page states none.

## 4. eSCL (AirScan/Mopria) and WSD-Scan: Rust clients and servers, spec terms, mDNS crates, which office scanners speak eSCL

### Takeaway
Rust has no mature eSCL client:
- `escl` 0.3.0 (2024, MIT OR Apache-2.0) is a thin async wrapper with no retries, no job cancel, and a discovery bug.
- `escl-scan` (2019) is GPL or MPL and abandoned.
- `covet` is GPL-3.0 and HP-specific.
- The two newest efforts are both unpublished or brand new: printscan's eSCL backend (PR #19) and scankit.

`printscan-wsd` 0.1.0 (published 2026-11-08, MIT OR Apache-2.0) is the only published Rust WS-Scan client. The best permissive reference for both protocols isn't Rust: OpenPrinting's go-mfp (Go, BSD-2-Clause, by sane-airscan's author, actively developed) has eSCL and WS-Scan models, clients and a virtual MFP.

The Mopria eSCL specification is free but confidential: internal use only, no redistribution, no patent grant. For mDNS, `mdns-sd` 0.21.7 is the clear choice: pure Rust, `forbid(unsafe_code)`, 7.4M downloads, 226 reverse dependencies.

On eSCL support: desktop document scanners rarely speak it. Of 205 devices in sane-airscan's tested list, 138 work over eSCL, and only one is a desktop document scanner (Brother ADS-4300N). No source shows any Fujitsu/Ricoh fi model or ScanSnap speaking eSCL.

### Cited Findings
**Rust eSCL**
- `escl` 0.3.0 (2024-09-18): MIT OR Apache-2.0, 4 releases, 5,137 downloads, 19 recent, no reverse dependencies. Repository: 4 stars, 0 open issues, pushed 2024-10-19 — [crates.io/crates/escl](https://crates.io/crates/escl); [github.com/xJonathanLEI/escl-rs](https://github.com/xJonathanLEI/escl-rs)
- The source (1,083 lines across `lib.rs`, `capabilities.rs`, `settings.rs` and `status.rs`):
  - It depends on `reqwest` 0.11 (rustls), `serde-xml-rs` 0.6 and `mdns` 3.0.0
  - `scan()` POSTs `ScanJobs` and requires 201 plus `Location`; `next_document()` maps 404 to "no more" and anything other than 200 to an error
  - It has no 503/Retry-After handling, no `DELETE` of a job, and no use of `ScannerStatus` to tell an empty feeder from a jam
  - `discover()` reads only the first mDNS response (`mdns_stream.next().await`), so it reports only the first device to answer
  - It has one unit test — [escl 0.3.0 src/lib.rs](https://docs.rs/crate/escl/0.3.0/source/src/lib.rs)
- `mdns` 3.0.0 (2021-01-29) hasn't been released since — [crates.io/crates/mdns](https://crates.io/crates/mdns)
- `escl-scan` 0.1.1 (2019-12-29) is GPL-3.0 on crates.io, but the GitHub repository now shows MPL-2.0 (a conflict between the two). It has 4 stars and was last pushed 2024-04-05 — [crates.io/crates/escl-scan](https://crates.io/crates/escl-scan); [github.com/ElXreno/escl-scan](https://github.com/ElXreno/escl-scan)
- `covet` 2.0.2 (2025-10-03): GPL-3.0-or-later, MSRV 1.85, 16,388 downloads, 15 stars. "CLI and web frontend for scanning with HP Envy printer / scanners" — [crates.io/crates/covet](https://crates.io/crates/covet); [github.com/arminha/covet](https://github.com/arminha/covet)
- printscan PR #19, an eSCL backend (unpublished):
  - It reads ScannerCapabilities and ScannerStatus, POSTs ScanJobs, and fetches NextDocument "until 404 or the count"
  - It "DELETEs a job stopped early", explains failures "from a fresh ScannerStatus: feeder empty, jam, hatch open, double feed", and retries 503 "after Retry-After"
  - It takes local addresses only, with no mDNS. HTTPS uses rustls with ring behind a `tls` feature
  - It has 20 unit and 20 integration tests against a scripted server, and two known bugs found against fakescan (a multi-page PDF counted as one page; Down reported as Other) — [printscan PR #19](https://github.com/b1tninja/printscan/pull/19)
- P4suta/scankit (MIT OR Apache-2.0, unpublished, 0 stars, pushed 2026-10-30): "An early stage Rust workspace for eSCL and Web Services on Devices (WSD) scanner support". Its crates include `scankit-escl`, `scankit-wsd`, `scankit-discover`, `scankit-backend-virtual` and `scankit-testkit` — [github.com/P4suta/scankit](https://github.com/P4suta/scankit)
- Other Rust apps with eSCL inside (found by code search for `_uscan._tcp` and `ScannerCapabilities`):
  - airbridge (an eSCL server over SANE)
  - pixma-rs's bridge (an eSCL server over CHMP)
  - sebastianspicker/open-scanline (MIT; eSCL native, while SANE goes through `scanimage` and WIA through PowerShell)
  - iamngoni/papyr (no license)
  - cr0ma-gh/painter (Apache-2.0)
  - GoTrustbe/Euroos (EUPL-1.2)
  - sources: [GitHub code search](https://github.com/search?q=_uscan._tcp+language%3Arust&type=code); [github.com/sebastianspicker/open-scanline](https://github.com/sebastianspicker/open-scanline)
- `ippusb` 0.6.0 (2026-07-27, Google, BSD-3-Clause, MSRV 1.89): an HTTP proxy for IPP-over-USB, for "Mopria eSCL scanning" over USB. It depends on `rusb`, i.e. libusb in C — [crates.io/crates/ippusb](https://crates.io/crates/ippusb); [github.com/google/ippusb](https://github.com/google/ippusb)

**Rust WSD**
- `printscan-wsd` 0.1.0 (2026-11-08): MIT OR Apache-2.0, MSRV 1.95, 12 downloads. Per its README:
  - "No `unsafe`, no panics on hostile replies"
  - modules for SOAP 1.2 with WS-Addressing, bounded XML via `roxmltree` 0.21, MTOM/XOP, WS-Discovery, WS-Eventing and an `HttpTransport` on `ureq`
  - a WS-Scan backend: "ScanTicket, CreateScanJob with the Windows driver's format fallback, RetrieveImage over MTOM until the scanner has no more, CancelJob, faults as scan errors"
  - tests against fakescan run when an environment variable names it — [crates.io/crates/printscan-wsd](https://crates.io/crates/printscan-wsd); [printscan-wsd README](https://docs.rs/crate/printscan-wsd/0.1.0/source/README.md)

**Non-Rust permissive references**
- **OpenPrinting/go-mfp**:
  - BSD-2-Clause, Go, 25 stars, 7 open issues, created 2024-03-06, last commit 2026-11-08, no tags
  - "fairly complete eSCL and WSD WS-Scan implementations, DNS-SD and WS-Discovery publisher and querier … based on the similar work previously done for scanners in the sane-airscan project". It is "work in progress and under the intensive development"
  - Packages: `proto/escl` (each type with `_test.go` and a `realdata_test.go`), `proto/wsscan`, `proto/ippusbapi`, `usbip`; commands include `mfp-virtual` and `mfp-proxy`
  - Building it needs cgo dependencies (avahi, libjpeg, libpng, libusb, cpython) — [github.com/OpenPrinting/go-mfp](https://github.com/OpenPrinting/go-mfp)
- stapelberg/airscan: Go, Apache-2.0, a 2020 eSCL client library, last release v0.2 (2020-08-16) — [github.com/stapelberg/airscan](https://github.com/stapelberg/airscan)

**Specs**
- **Mopria eSCL.** It "is available for free download" and "is publicly available to enable companies to implement and support eSCL" — [mopria.org/specifications](https://mopria.org/specifications)
  - The license grants "the right to use and to make verbatim copies of the Specification, solely for internal use", lets you build "Implemented Products", and forbids distributing the specification "to any third party under any circumstances"
  - It treats the text as "confidential and proprietary information of Mopria and/or its Members", says "THIS LICENSE AGREEMENT DOES NOT GRANT ANY RIGHTS WITH RESPECT TO ANY PATENT CLAIMS", and states no version number — [mopria.org/spec-download](https://mopria.org/spec-download)
  - This confirms the audit's "internal implementation only, with no redistribution", and adds the confidentiality and no-patent-grant terms
- sane-airscan's README: "The eSCL protocol is not documented, but this is simple protocol, based on HTTP and XML, easy for reverse engineering". By contrast, AirSane says it "implements the Mopria eSCL protocol as published" — [sane-airscan README](https://github.com/alexpevzner/sane-airscan); [AirSane README](https://github.com/SimulPiscator/AirSane)
- **WS-Scan:**
  - Microsoft publishes the "Scan Service Definition V1.0 for Web Services on Devices" as a zip dated 2012-02-09, under a technical-documentation license that refers to the WDK. This is from a search summary of Microsoft's print-spec pages; I didn't read the license.
  - When Microsoft contributed WS-Scan to the PWG in 2008, press reports called the terms "RANDZ" (reasonable, non-discriminatory, zero cost) — [BetaNews 2008](https://betanews.com/2008/05/30/microsoft-gives-in-will-share-scanning-service-spec-with-working-group/); [Microsoft 2008 release](https://news.microsoft.com/2008/05/29/microsoft-contributes-web-services-protocol-to-broaden-interoperability/)
  - Microsoft Learn documents the WS-Scan schema elements, from a docs repository licensed CC-BY-4.0 (code samples MIT) — [WSD Scan Service Scanner Elements](https://learn.microsoft.com/en-us/windows-hardware/drivers/image/wsd-scan-service-scanner-elements); [MicrosoftDocs/windows-driver-docs](https://github.com/MicrosoftDocs/windows-driver-docs)

**mDNS crates**
- `mdns-sd` 0.21.7 (2026-11-02): Apache-2.0 OR MIT, MSRV 1.71, 86 versions, 7.44M downloads (3.42M in 90 days), 226 reverse dependencies, 228 stars, 23 open issues
  - `#![forbid(unsafe_code)]`; dependencies `socket2`, `mio`, `if-addrs`, `socket-pktinfo`, `flume` and `fastrand`, with no C library
  - The audit's 0.21.6 and "6.6M" are now stale — [crates.io/crates/mdns-sd](https://crates.io/crates/mdns-sd); [mdns-sd lib.rs](https://docs.rs/crate/mdns-sd/0.21.7/source/src/lib.rs); [github.com/keepsimple1/mdns-sd](https://github.com/keepsimple1/mdns-sd)
- Alternatives:
  - `zeroconf` 0.18.0 (2026-04-07), which wraps Bonjour or Avahi (C), with a "non-standard" license on crates.io — [crates.io/crates/zeroconf](https://crates.io/crates/zeroconf)
  - `simple-mdns` 0.7.0 (2026-05-15, MIT, 83k downloads) — [crates.io/crates/simple-mdns](https://crates.io/crates/simple-mdns)
  - `hick` 0.2.0 (2026-06-18, MIT OR Apache-2.0, MSRV 1.91, sans-I/O `mdns-proto` core, about 1k downloads) — [crates.io/crates/hick](https://crates.io/crates/hick)
  - `astro-dnssd` 0.3.8 (2026-11-03), which wraps native DNS-SD — [crates.io/crates/astro-dnssd](https://crates.io/crates/astro-dnssd)

**Which scanners speak eSCL**
- sane-airscan's tested-device table has 205 devices: 138 marked "Yes" for eSCL, 98 for WSD, and 106 eSCL-only. Its only desktop document scanners are Brother ADS-2700W (eSCL No, WSD Yes), Brother ADS-4300N (both Yes) and Epson WorkForce ES-580W (eSCL No, WSD Yes). There are no fi, ScanSnap, Canon DR or Kodak rows (counted from the README's table) — [sane-airscan README](https://github.com/alexpevzner/sane-airscan#compatibility)
- The README also says that "most (all?) of the eSCL devices will also work over USB, if IPP-over-USB daemon is installed", while "WSD-only devices cannot be used with the IPP-over-USB daemon" — [sane-airscan README](https://github.com/alexpevzner/sane-airscan)
- Searches found no source saying any Ricoh/Fujitsu fi model supports eSCL, AirScan or Mopria Scan. PFU's Linux page lists fi-8000 models as "USB only" for its SANE driver ([PFU multi-OS driver](https://www.pfu.ricoh.com/fi/software/multios-driver/)). A search summary of a Ricoh IA page says networked fi scanners go through "Network Setup Tool for fi Series" with PaperStream IP; I didn't open it — [Ricoh IA docs](https://docs.ria.ricoh.com/ricohia/Started/Web_Scanner.htm)
- Brother markets the ADS-4900W as able to "scan to mobile device with AirPrint" (search result). AirPrint scanning is eSCL, but the listing doesn't name eSCL — [Brother ADS-4900W](https://brother-sitecore-prd-cd.azurewebsites.net/scanners/ads4900w)

### Inferences
- **Fit to the office.** The fi-7160, fi-8170 and fi-6130 are USB-only for scanning as far as any source shows, so eSCL wouldn't reach them. eSCL and WSD reach network MFPs and copiers, which each OS already reaches: sane-airscan through libsane on Linux, WIA on Windows, ImageCaptureCore on macOS. The audit's "no network scan protocol of sheaf's own" still holds.
- **If eSCL is ever wanted:**
  - `escl` 0.3.0 isn't adequate. It lacks retries and cancel, its discovery finds only the first responder, it uses an unmaintained `mdns`, and contributing upstream to a 2024-dormant repository is uncertain.
  - The permissive sources worth porting are go-mfp's eSCL types (BSD-2, with real-device test data) and printscan's eSCL backend once published (MIT OR Apache-2.0).
  - sane-airscan's quirks (GPL) are facts to learn, not code to copy: 503 retries (30 for NextDocument, 1 s apart), Brother's false 404/AdfEmpty, Xerox B205/B215's 404/410, the WorkCentre 3345 jam that means empty, RICOH's status-before-NextDocument, and HP's `Host: localhost`.
- **WSD.** `printscan-wsd` is permissive and well scoped, but has existed one day. On Windows, WSD scanners already appear as WIA devices, and on Linux sane-airscan speaks WSD.
- **Verdicts:**
  - **eSCL client: build our own if ever needed, porting go-mfp's data model (BSD-2); study sane-airscan's quirks (GPL). Not needed for the planned crates.**
  - **WSD-Scan client: study only. If needed, use `printscan-wsd` and contribute upstream.**
  - **mDNS discovery: use `mdns-sd` directly.**
  - **IPP-over-USB: study only.** `ippusb` is BSD-3 but brings libusb; Linux's ipp-usb daemon already exposes such devices as network scanners.

### Gaps
- No count of eSCL-capable scanners in the market (from Apple's AirPrint list or Mopria's certified-product list) was found, and no count of Mopria Scan-certified document scanners.
- Whether newer network-capable fi models (fi-8040, fi-7300NX) or the ScanSnap iX2500 speak eSCL or WSD: nothing found either way.
- The eSCL spec's current version number. The Mopria page states none.

## 5. Non-Rust prior art: what libinsane, NAPS2, sane-airscan, simple-scan, python-sane, scanservjs, AirSane and scanimage learned; quirks lists

### Takeaway
The lessons are consistent and verified:
- Run SANE out of process. libinsane's reason is verbatim: "some corrupt memory, some cause crashes". NAPS2 runs SANE in a worker "for added stability".
- Normalize source names across backends.
- Never trust a flatbed to stop.
- Treat absurd parameters (Brother's `2500x0`) as end of feed.
- Restart on `NO_DOCS` in some flows.
- Speed up device opening by giving SANE a one-backend `dll.conf`.

Every quirk list worth having is copyleft (libinsane LGPL-3+, NAPS2 GPL-2+/LGPL-2.1+, simple-scan GPL-3, sane-airscan GPL-2+ with exception, AirSane GPL-3, scanservjs GPL-2). The facts (option names and values) can be learned; the code can't be copied. python-sane is permissive (HPND-style), but it's a C extension that links libsane, so it offers nothing to port. Verdict for all of them: **study only**.

### Cited Findings
**libinsane** (C, LGPL-3+): latest tag 1.0.10 (2023-03-01), project activity 2026-09-09, 4 stars on GNOME GitLab — [libinsane](https://gitlab.gnome.org/World/OpenPaperwork/libinsane); [tags](https://gitlab.gnome.org/World/OpenPaperwork/libinsane/-/tags). Its `workarounds.h`, verbatim where quoted — [workarounds.h](https://gitlab.gnome.org/World/OpenPaperwork/libinsane/-/blob/master/subprojects/libinsane/include/libinsane/workarounds.h):
- **dedicated process** ("Culprits: Brother drivers, old HP drivers"): "Sane backends run as part of the application that use them. However some of them are unstable: some corrupt memory, some cause crashes, etc. Some appear to work fine when running in simple applications … but do not anymore in more complex applications … the only reliable workaround is to run the code that use Sane and its backend in a dedicated process." On macOS a dedicated thread is used instead "because fork() is too limited"
- **thread safety**: "Most scanner APIs are not thread-safe"
- **one-page flatbed** (EPSON XP-425): drivers that "keep returning the same page/image again and again". The `auto` source is hidden for the same reason (HP Photosmart C410)
- **invalid page size** (Brother ADS-1200, DS-740D): "the Brother backend reports there is a next page. But when requesting the page size, we get something absurd like 2500x0", and "an height of -1 means there is still some paper to scan"
- **check capabilities**: don't touch inactive options ("Some may even crash"); with a single-valued `source=ADF` some drivers return `SANE_STATUS_INVAL` (Epson DS-310)
- **option names**: `scan-resolution` becomes `resolution` (Lexmark), `doc-source` becomes `source` (Samsung CLX-3300)
- **option values**: Brother `24bit Color`/`True Gray`/`Black & White`, Samsung `Black and White - Line Art`, translated values (OKI)
- **cache**: minimize `list_options`/`set`/`get` calls (HP plus the `net` backend)
- **lamp off**: Canon LiDE 30 on plustek

Its `normalizers.h` — [normalizers.h](https://gitlab.gnome.org/World/OpenPaperwork/libinsane/-/blob/master/subprojects/libinsane/include/libinsane/normalizers.h):
- source names: `ADF Duplex` becomes `feeder duplex`, `Document Table` becomes `flatbed` (Epson Perfection V19)
- resolution as range or list, with Fixed values allowed
- RAW1/RAW8 to RAW24
- Fujitsu `page-height`/`page-width`: "The default values are bad … set them to the max by default to disable automatic centering" (ScanSnap S1500, iX500)
- some scanners lack a `source` option at all

**NAPS2** (C#; GPL-2.0+ app; `NAPS2.Sdk` and `NAPS2.Escl` LGPL-2.1+): latest release v8.6.0 (2026-10-25), 4,758 stars — [github.com/cyanfish/naps2](https://github.com/cyanfish/naps2); [NAPS2.Escl LICENSE](https://github.com/cyanfish/naps2/blob/master/NAPS2.Escl/LICENSE)
- `ScanBridgeFactory`: "Worker processes generally aren't required, just preferred for stability", then "Run SANE in a worker process for added stability" (also ImageCaptureCore). The worker talks gRPC over named pipes (`GrpcDotNetNamedPipes`) — [ScanBridgeFactory.cs](https://github.com/cyanfish/naps2/blob/master/NAPS2.Sdk/Scan/Internal/ScanBridgeFactory.cs); [NAPS2.Sdk.csproj](https://github.com/cyanfish/naps2/blob/master/NAPS2.Sdk/NAPS2.Sdk.csproj)
- `SaneScanDriver.cs` (650 lines):
  - The cancel token is registered to call `device.Cancel` once
  - `NO_DOCS` with no image yet is "feeder empty"; `INVAL` is mapped to "device offline" (with a "TODO: Maybe not always correct")
  - Duplex is found by `source`, then `adf_mode`/`adf-mode`, then a bool `duplex`, then a string `duplex` = `both` (with front for simplex: "kds_s2000 has front/back/both")
  - Three-pass frames are assembled channel by channel
  - Its transfer formats are only 1-bit gray (inverted), 8-bit gray and 8-bit RGB
  - Speed-up: "By using a custom config dir with a dll.conf file that only has a single backend specified, we can force SANE to only check that backend", noting that "SANE caches the SANE_CONFIG_DIR environment variable process-wide" — [SaneScanDriver.cs](https://github.com/cyanfish/naps2/blob/master/NAPS2.Sdk/Scan/Internal/Sane/SaneScanDriver.cs)
- `SaneOptionMatchers.cs` matches sources by every translation of `Flatbed`, `FB`, `ADF`, `Automatic Document Feeder`, `ADF Front`, `Duplex` and `ADF Duplex`, plus substrings ("feeder", "adf", "duplex"). Option names: `resolution`, `x-resolution`, `y-resolution`, `mode`, `source`, `adf_mode`, `adf-mode`, `duplex` ("Not commonly used (usually adf_mode is used instead)"), `page-width`, `page-height`, `tl-x`…`br-y` — [SaneOptionMatchers.cs](https://github.com/cyanfish/naps2/blob/master/NAPS2.Sdk/Scan/Internal/Sane/SaneOptionMatchers.cs); [SaneOptionNames.cs](https://github.com/cyanfish/naps2/blob/master/NAPS2.Sdk/Scan/Internal/Sane/SaneOptionNames.cs)
- NAPS2 ships its own eSCL client, server and USB layers (`NAPS2.Escl`, `NAPS2.Escl.Server` with a `FakeEsclScanJob`, `NAPS2.Escl.Usb`) — [NAPS2 repository tree](https://github.com/cyanfish/naps2)

**simple-scan** (Vala, GPL-3.0; latest tag 50.0): its `scanner.vala` — [scanner.vala](https://github.com/GNOME/simple-scan/blob/master/src/scanner.vala); [tags](https://github.com/GNOME/simple-scan/tags)
- feeder strings: `Automatic Document Feeder`, `ADF`, `Automatic Document Feeder(centrally aligned)` / `(center aligned)` / `(left aligned)` (Brother brother3/brscan5), `ADF Simplex` (Samsung)
- front `ADF Front`, back `ADF Back`
- duplex strings: `ADF Duplex`, `Duplex ADF` (Brother DS-720), `ADF Duplex - Long-Edge Binding` / `- Short-Edge Binding` (Samsung), `Duplex` (HP), `Automatic Document Feeder(centrally aligned,Duplex)`
- flatbed strings: `Auto`, `Flatbed`, `FlatBed`, `Normal`, `Document Table` (Epson)
- separate options: `duplex` / `scan-both-sides` (Lexmark), `adf-mode` (Epson GT-S50), `ScanMode` = `Simplex`/`Duplex` (Canon DR-C240), `adf-auto-scan`, `AutoDocumentSize`
- it sets `page-width`/`page-height` when known
- with the test backend it turns off `hand-scanner` and `three-pass`
- a NO_DOCS restart in batch mode: "some scanners return NO_DOCS after the first page is scanned. Try cancelling and restarting"

**sane-airscan** (C, GPL-2.0+ with the SANE exception): tags up to 0.99.38 (tag commit 2026-07-30), last commit 2026-09-07, 374 stars, 31 open issues. Supports eSCL and WSD — [github.com/alexpevzner/sane-airscan](https://github.com/alexpevzner/sane-airscan); [LICENSE](https://github.com/alexpevzner/sane-airscan/blob/master/LICENSE)
- its `source` values are `Flatbed`, `ADF` and `ADF Duplex` — [airscan.h](https://github.com/alexpevzner/sane-airscan/blob/master/airscan.h)
- its eSCL quirks: `quirk_localhost`, `check_adf_state`, `status_before_load`, `port_in_host`, `next_load_delay`, `retry_on_404`, `retry_on_410`, `broken_ipv6_location`, `skip_cleanup`
- these are keyed by `MakeAndModel` (Canon iR2625/2630, HP LaserJet MFP M630, HP Color LaserJet FlowMFP M578, MF410 Series, RICOH, EPSON, Brother, Xerox B205/B215, WorkCentre 3345) or by the `Server: HP_Compact_Server` header
- 503 is retried 30 times for NextDocument and 10 for other requests, 1,000 ms apart — [airscan-escl.c](https://github.com/alexpevzner/sane-airscan/blob/master/airscan-escl.c)

**AirSane** (C++, GPL-3.0): an eSCL server publishing SANE scanners over mDNS. Latest release v0.4.16 (2026-11-02), 381 stars. Clients include Apple Image Capture, sane-airscan, Windows 10/11's eSCL client ("Windows does not allow more than 4 AirSane scanners in total") and the Mopria Scan app — [github.com/SimulPiscator/AirSane](https://github.com/SimulPiscator/AirSane)

**scanservjs** (JavaScript, GPL-2.0): "yet another scanimage-web-front-end", needing "sane-utils, ImageMagick, Tesseract and nodejs". Latest release v3.3.0 (2026-08-19), 1,193 stars — [github.com/sbs20/scanservjs](https://github.com/sbs20/scanservjs)

**python-sane** (python-pillow/Sane): a C extension (`_sane.c`) linking libsane. Latest release v2.9.2 (2025-07-21). Its COPYING is a permissive HPND-style notice: "Permission to use, copy, modify, and distribute this software … without fee is hereby granted, provided that the above copyright notice appear in all copies…" — [github.com/python-pillow/Sane](https://github.com/python-pillow/Sane); [COPYING](https://github.com/python-pillow/Sane/blob/main/COPYING)

**SANE `test` backend's source values**: `Flatbed` and `Automatic Document Feeder` — [backend/test.c](https://gitlab.com/sane-project/backends/-/blob/master/backend/test.c)

### Inferences
- **Verified against p06.** All of its quirks except Brother's `2500x0` are confirmed by more than one frontend. The `2500x0` comes from libinsane alone, with two Brother models named. p06's source list is confirmed for fujitsu (`ADF Front/Back/Duplex`), airscan (`ADF`/`ADF Duplex`) and test (`Automatic Document Feeder`).
- **Missing from crates.md's quirk list**, to consider:
  - (a) the separate `ScanMode` (Canon DR-C240) and `scan-both-sides` (Lexmark) options
  - (b) a string-valued `duplex` = `front`/`both` (Kodak `kds_s2000`)
  - (c) Brother's `(left aligned,Duplex)` style source strings
  - (d) translated option values, which NAPS2 matches through sane-backends' translations and libinsane reverses
  - (e) the one-backend `SANE_CONFIG_DIR` trick, which fits the "device list under a deadline" problem (10.7 s): open a known device with a `dll.conf` naming only its backend
  - (f) `INVAL` from `sane_start` being ambiguous (NAPS2 maps it to "offline"; p06 re-reads descriptors)
- **NAPS2's limits.** Its SANE path handles 1- and 8-bit only. The planned `sane-dl` (16-bit, padded rows, `lines = -1`) goes further than NAPS2, so these can't be ported (they are copyleft anyway) and must be tested against the test backend (§6).
- **Verdict: quirk knowledge for the SANE batch loop and option mapping: study only.** libinsane, NAPS2, simple-scan, sane-airscan and AirSane are all copyleft. Record the facts (option names, values, statuses) in our own words, in a table with a citation per row; don't copy code. python-sane is permissive, but there is nothing in it to port (a libsane-linking C extension).

### Gaps
- I didn't survey the sane-devel mailing-list archive for quirks beyond those encoded in these frontends.
- I didn't verify whether the NAPS2 Linux packages ship with the worker enabled by default (`WorkerFactory` non-null).

## 6. Testing without hardware: SANE's test and pnm backends, fake libsane, eSCL/WSD mock servers

### Takeaway
SANE's own `test` backend covers more than p22 credits it with:
- three-pass color, with a settable channel order
- padded rows (`ppl-loss`)
- 16-bit byte swapping
- fuzzy parameters
- non-blocking reads and a select fd
- any status, including `NO_DOCS`, returned from every `sane_read`

So the planned fake libsane is needed for less than crates.md lists. Debian also builds the `pnm` backend, which turns a PNM file into a scan, with three-pass, hand-scanner and injected statuses. No fake libsane exists in Rust or, as far as code search shows, any language. For eSCL and WSD, permissive virtual scanners exist: b1tninja/fakescan (Apache-2.0, Docker, WSD; eSCL on a branch) and go-mfp's `mfp-virtual` (BSD-2). sanei_usb has an XML record/replay mode, but it's a build option, not on by default. Verdicts:
- **Use SANE's test and pnm backends directly** (from the system's libsane1, at run time).
- **Build our own fake libsane**, narrowed.
- **Use fakescan or go-mfp in CI only if network backends are ever built.**

### Cited Findings
- **test backend options**, verified in `test.c` — [backend/test.c](https://gitlab.com/sane-project/backends/-/blob/master/backend/test.c):
  - `source`: `Flatbed` and `Automatic Document Feeder` (no duplex)
  - `sane_start` returns `SANE_STATUS_NO_DOCS` when the ADF is selected and `number_of_scans % 11 == 0`. The counter resets in `sane_open`, so a device yields 10 sheets, then NO_DOCS, then 10 more
  - geometry range 0–200 mm (default `br-x` is 80 mm); resolution 1–1200 dpi
  - `three-pass` with `three-pass-order` ∈ {RGB, RBG, GBR, GRB, BRG, BGR}
  - `hand-scanner` (height −1, fixed 11 cm width)
  - `ppl-loss` (unused pixels at the end of each line)
  - `invert-endianness` (16-bit)
  - `read-limit`/`read-limit-size`, `read-delay`
  - `read-return-value` ∈ {Default, UNSUPPORTED, CANCELLED, DEVICE_BUSY, INVAL, EOF, JAMMED, NO_DOCS, COVER_OPEN, IO_ERROR, NO_MEM, ACCESS_DENIED}, returned "by every `sane_read()` call"
  - `fuzzy-parameters`, `non-blocking`, `select-fd`
  - `enable-test-options`: bool, int, fixed, string and button options covering every capability and constraint type, including `int-inexact`, which "returns SANE_INFO_INEXACT"
  - `test.conf`'s `number_of_devices` — [sane-test.man](https://gitlab.com/sane-project/backends/-/blob/master/doc/sane-test.man)
- **pnm backend**: "primarily to aid in debugging of SANE frontends", disabled by default upstream for security reasons (remote users of saned can read PNM files). Its options are `grayify`, `three-pass`, `hand-scanner`, `status`, `status-eof`, `status-jammed`, `status-nodocs`, `status-coveropen`, `status-ioerror`, `status-nomem` and `status-accessdenied` — [backend/pnm.c](https://gitlab.com/sane-project/backends/-/blob/master/backend/pnm.c); [sane-pnm.man](https://gitlab.com/sane-project/backends/-/blob/master/doc/sane-pnm.man)
- Debian's `rules` builds sane-backends 1.4.0-1 with `--enable-pnm-backend` — [Debian rules](https://sources.debian.org/src/sane-backends/1.4.0-1/debian/rules/)
- **sanei_usb record/replay**: `sanei_usb_testing_mode_record` "records the communication" and `…_replay` "replays the communication with the scanner" from an XML file (libxml2). It is compiled only `#if WITH_USB_RECORD_REPLAY` and is enabled through `sanei_usb_testing_enable_replay(path, development_mode)`, not an environment variable that I could find — [sanei/sanei_usb.c](https://gitlab.com/sane-project/backends/-/blob/master/sanei/sanei_usb.c)
- **Fake libsane**: code searches for "fake libsane", "mock libsane" and Rust `cdylib` SANE exports found nothing usable (the hits were OpenWrt `.config` files and package lists) — [GitHub code search](https://github.com/search?q=%22fake+libsane%22&type=code)
- printscan's tests carry "a WIA-, a SANE- and an eSCL-shaped fake" behind its `Backend` trait. Its own issue #13 found that fake "gave `NO_DOCS` the code 6, which is `SANE_STATUS_JAMMED`. `SANE_STATUS_NO_DOCS` is 7" — [printscan docs/scan.md](https://docs.rs/crate/printscan/0.1.0/source/docs/scan.md); [printscan #13](https://github.com/b1tninja/printscan/pull/13)
- **b1tninja/fakescan** (Rust, Apache-2.0, 0 stars, created 2026-10-12, pushed 2026-11-02): "A fake scanner for testing acquire paths: scriptable glass/feeder, synthetic page patterns, image defects, and capability profiles that look like real WIA / WSD devices". It is Docker-first (`ghcr.io/b1tninja/fakescan`) and serves a control API on port 3920 for paper and fault plans, with profiles such as `brother-wsd` and `wia-lossless`. An eSCL device is on branch `escl-device` (per printscan PR #19) — [github.com/b1tninja/fakescan](https://github.com/b1tninja/fakescan); [printscan PR #19](https://github.com/b1tninja/printscan/pull/19)
- **go-mfp**: `cmd/mfp-virtual`, a virtual MFP simulator, and USB/IP device emulation for IPP-over-USB ("relies on the USB/IP kernel support (the `vhci_hcd` module)") — [github.com/OpenPrinting/go-mfp](https://github.com/OpenPrinting/go-mfp)
- NAPS2's own CI has no scanner, and its scanner tests are a manual project (audit, scanners.md). NAPS2.Escl.Server contains a `FakeEsclScanJob` — [NAPS2.Escl.Server](https://github.com/cyanfish/naps2/tree/master/NAPS2.Escl.Server)
- AirSane in front of SANE's test backend would be an eSCL server over a virtual device. AirSane's README describes publishing any SANE scanner over eSCL and mDNS, but doesn't mention the test backend specifically — [AirSane README](https://github.com/SimulPiscator/AirSane)

### Inferences
- **Correct p22 and crates.md.** These items are covered by SANE's test backend in CI and don't need the fake libsane:
  - three-pass frames (including channel order)
  - padded rows (`ppl-loss`)
  - 1-bit and 16-bit (with `invert-endianness` for byte order)
  - `NO_DOCS` from `sane_read` (`read-return-value = SANE_STATUS_NO_DOCS`)
  - `INFO_INEXACT` and constraint types (`enable-test-options`)
  - non-blocking reads and the select fd
- The fake libsane remains necessary for:
  - each backend's feeder and duplex names (`ADF Front`/`ADF Duplex`, Brother's long strings, `adf-mode`, `ScanMode`)
  - a jam at sheet N (the test backend injects a status on every read, not at a chosen sheet)
  - Brother's `2500x0`
  - `lines = -1` from a feeder (the test backend's hand-scanner mode is unknown-height, but with a fixed width; whether it combines with ADF is untested)
  - duplex sides
  - descriptors that change after a set (`RELOAD_OPTIONS`)
  - a backend that crashes or hangs, for the worker's watchdog
- **pnm** lets CI scan a known fixture image, such as the `scan-fixtures` pages, through real libsane. A byte-for-byte check of frame assembly then becomes possible. The test backend's patterns are synthetic.
- **Verdicts:**
  - **Hardware-free SANE testing: use SANE's `test` and `pnm` backends directly**, from the runner's `libsane1` with a private `SANE_CONFIG_DIR` (NAPS2 and janus-scan already do this).
  - **Fake libsane (`cdylib`): build our own**, scoped to what test and pnm can't do. Nothing exists to adopt.
  - **eSCL/WSD virtual scanners: use fakescan (Apache-2.0) or go-mfp (BSD-2) directly in CI if a network backend is ever built.** AirSane over the test backend is study only (GPL-3).
  - **USB record/replay: study only.** It sits in GPL sanei code behind a build option; useful only if sheaf ever debugs a specific backend.

### Gaps
- I didn't run anything (constraint), so "the test backend's three-pass works through libsane1 on ubuntu-latest" is from source, not observed.
- Whether Ubuntu's libsane1 package on GitHub runners includes `libsane-pnm.so` (Debian's rules enable it; Ubuntu usually inherits that, but I didn't verify), and whether pnm's statuses are returned from `sane_start` or `sane_read` (not checked in `pnm.c`).
- Whether fakescan's eSCL branch is merged or usable, and how much of go-mfp's `mfp-virtual` is finished ("work in progress").

## 7. Verdict per need, and holes in the ecosystem

### Takeaway
Most of the plan stands, and the audit's "build in-house" for SANE is confirmed with stronger evidence. Two things are new since the 2026-10-23 audit:
- a permissive pure-Rust family that overlaps `scan-device`, saned and WSD: printscan, published 2026-11-08, with SANE-net and eSCL in open PRs
- a permissive Go reference for eSCL and WSD (go-mfp)

All are too new to depend on. Corrections: `sane.h` has 14 functions, not 15; SANE's test backend covers three-pass, padding, 16-bit order and NO_DOCS from `sane_read`; Rust SANE FFI has spread to about 20 repositories, not about a dozen.

### Cited Findings
- **printscan** 0.1.0 (2026-11-08): MIT OR Apache-2.0, MSRV 1.95, 17 downloads; repository 0 stars, 10 open issues and PRs, created 2026-11-01. Its PR bodies end "Generated with Claude Code" — [crates.io/crates/printscan](https://crates.io/crates/printscan); [github.com/b1tninja/printscan](https://github.com/b1tninja/printscan)
- Its scan contract ("Extracted from PdfCraft's `crates/scan`"):
  - a `Backend` trait (`scanner_list`, `scanner_details`, `status`, `diagnostic_dump`, `acquire`) behind a process-wide registry
  - ids namespaced `"<backend>:<native id>"`, with partial-failure listings
  - "Pages are encoded images" (BMP from WIA; JPEG/PNG/TIFF/PDF from eSCL or SANE)
  - cancel and progress tokens in `ScanSettings`
  - `ScanError::Incomplete` keeps partial pages
  - an empty feeder is `InterruptKind::FeederEmpty` (SANE `NO_DOCS`, eSCL `AdfEmpty`)
  - `ScanJob` and `Interruption` give retry and "what to fix" advice
  - "no platform code, no unsafe, no network and no UI" — [printscan docs/scan.md](https://docs.rs/crate/printscan/0.1.0/source/docs/scan.md)
- **printscan-windows** 0.1.0 (WIA 2.0 scanning plus the print spooler, 55 `unsafe` occurrences in about 5.4k lines) is published, and the macOS ImageCaptureCore backend is an open PR (#21) — [crates.io/crates/printscan-windows](https://crates.io/crates/printscan-windows); [printscan PRs](https://github.com/b1tninja/printscan/pulls)
- The planned model wants raw pixels with format and resolution (`RawPage`), drivers in a JSON-lines worker, and events on a channel — [crates.md Devices](docs/pm/crates.md)

### Inferences
**Verdicts per need**

| Need | Verdict | Reason |
|---|---|---|
| libsane loaded at run time, own bindings from `sane.h` (`sane-dl`) | **Build our own** | `sane-scan` links at build time, is unsound, and is unmaintained. Others are private app code, days old. Port sanetui's header-contract test idea (Apache-2.0). Keep `sane.h`'s notice; 14 functions. |
| Typed option model (descriptors, constraints, units, reload) | **Build our own** | No permissive published one. printscan-sane's `protocol.rs` (MIT/Apache, unpublished) is a cross-check for constraint and value encoding. libinsane and NAPS2 are study only. |
| Frame assembly (three-pass, multi-frame, 1-bit, 16-bit, padded rows, `lines = -1`) | **Build our own** | NAPS2 handles only 1- and 8-bit (GPL, study). printscan-sane's `scan/frame.rs` (permissive, unpublished) is a cross-check. Test against SANE's test backend. |
| `Send` cancel token | **Build our own** | Small. airbridge's `alive`-flag pattern (permissive) shows the race to close; the standard forbids other calls until the cancelled call returns. |
| Batch loop and backend quirks | **Study only** (libinsane, NAPS2, simple-scan) | Copyleft. Record the facts in our own table. |
| Drivers in a worker process, JSON lines (`scan-worker`) | **Build our own** | Prior art is NAPS2 (gRPC named pipes, GPL), libinsane (fork, LGPL) and negative-vibes (GPL): study only. No Rust crate. |
| Device model shared with TWAIN/WIA (`scan-device`) | **Build our own; study printscan** | printscan is permissive and close in spirit (namespaced ids, FeederEmpty, Incomplete, retry advice), but pages are encoded images, not `RawPage`; it is one day old; MSRV 1.95. Could become "use and contribute upstream" if it matures. |
| SANE network protocol client | **Study only now; use and contribute upstream (printscan-sane) if remote saned is wanted** | Pure Rust, no `unsafe`, permissive, but unpublished and untested on hardware. |
| Pure-Rust USB driver (fi-series) | **Don't build now; if ever, port scan2drive's fss500 (Apache-2.0)** | nkscan (MIT/Apache) shows the transport pattern. SANE's fujitsu backend is study only. |
| eSCL client | **Build our own if ever needed, porting go-mfp's model (BSD-2)** | `escl` 0.3.0 is too thin; sane-airscan's quirks are study only. Not needed for USB fi-series scanners. |
| WSD-Scan client | **Study only; use and contribute upstream (`printscan-wsd`) if needed** | Reached already through WIA and sane-airscan. |
| mDNS | **Use `mdns-sd` directly** | Pure Rust, `forbid(unsafe_code)`, widely used, frequent releases. |
| IPP-over-USB | **Study only** | `ippusb` brings libusb (C); OS daemons already cover it. |
| Hardware-free SANE tests | **Use SANE's `test` and `pnm` backends directly; build our own fake libsane (narrowed)** | No fake libsane exists anywhere. |
| eSCL/WSD mock servers | **Use fakescan (Apache-2.0) or go-mfp (BSD-2) directly, if a network backend is built** | AirSane over the test backend is study only (GPL-3). |

**Holes in the ecosystem**
- No sound, maintained, published Rust binding to libsane, and none that loads at run time. Every run-time-loading one is private app code under a month old.
- No published pure-Rust saned client. The only one (printscan-sane) is an open PR, untested on real daemons.
- No mature Rust eSCL client: `escl` 0.3.0 is thin and dormant, and printscan's is unpublished. The spec itself is confidential and can't be quoted in an open repository.
- No fake libsane in any language, and no permissive, machine-readable quirk list for SANE backends. Every list is embedded in copyleft frontends.
- No pure-Rust document-scanner USB driver at all, and no permissive fi-series driver in any language. scan2drive's fss500 (Go) covers the ScanSnap iX500 only.
- No public fi-series SCSI interface document found. Only older M309x OEM manuals.

### Gaps
- printscan's actual quality on real devices, and its maintainers' intentions (one account, AI-assisted PRs, published the day of this survey), can't be judged yet. Re-check before any decision to adopt.
- The licensing of `twain.h` and PFU's `pfufs` driver is outside this note's scope, or was unanswered.
