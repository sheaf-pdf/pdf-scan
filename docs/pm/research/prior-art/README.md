# Scanning crates prior art: build every crate, borrow its parts

Nothing published, in Rust or any other language, can stand in for any of the ten planned crates, so each one is a build. Almost every dependency beneath them, though, is already good enough to use directly, and the plan should lean on those rather than rebuild them. Each whole-crate candidate fails on a specific fact. The one SANE binding, `sane-scan` 0.1.2, links libsane at build time and marshals `SANE_Bool` unsoundly. The one TWAIN crate is a 2021 prototype with undefined behaviour. No open patch-code reader exists under any license. And no Rust library returns a PDF image's samples in its own color space and depth, or writes a searchable scanned PDF a page at a time. Lower down the picture flips: `libloading`, `windows` 0.62, `objc2-image-capture-core`, `tiff`, `png`, `zune-jpeg`, `jpeg-encoder`, the three hayro codecs, `image`, `imageproc` and `rxing` are each maintained, permissive and adequate. The survey also forces four changes the plan doesn't yet carry. TWAIN definitions should come from the Working Group's MIT-licensed `TWAIN.cs`, which closes crates.md's open `twain.h` license question. crates.md's WIA duplex value and error codes are wrong. `fax` 0.3.0 still panics on G4 rows wider than 32,767 pixels, so BUG-1's fix needs an upstream fix that hasn't been released. And every CCITT strip should decode through `hayro-ccitt` 0.5.0, whose API broke in that release and whose MSRV of 1.92 sits above the family's 1.89. The survey day also brought `printscan`, a permissive Rust scanning family. It is one day old, carries encoded pages rather than raw ones, and pulls image codecs under the device layer, so its knowledge is worth porting but the crate isn't worth depending on. What remains unverified is mostly CI spikes and measurements, not open questions of fact.

## Every planned crate fills a hole, and almost every dependency beneath it is already built

The six research notes checked each need against Rust crates, code in other languages and the 2026-10-23 audit. They applied the plan's rules: every crate MIT OR Apache-2.0; copyleft code studied but never copied; an unmodified copyleft library loaded at run time allowed (D17). The result has the same shape in every layer. **Each crate is a build because nothing adequate exists at its level, and each sits on dependencies that are use-directly.** The table compresses that, and the sections after it give a verdict table per need.

| Crate | Closest prior art, and why it falls short | Verdict for the crate | Used directly beneath it |
|---|---|---|---|
| `sane-dl` | `sane-scan` 0.1.2: links at build time, marshals values unsoundly, unmaintained. airbridge, janus-scan, ruscan: private app code, weeks old | Build our own | `libloading` 0.9 |
| `scan-device` | `printscan` 0.1.0: one day old, encoded pages, codecs under the device layer | Build our own; port printscan-windows' WIA rules | `windows` 0.62.2, `objc2-image-capture-core` 0.3.2 |
| `twain-dsm` | `twain` 0.1.0: undefined behaviour, bundled DLLs. RSWebTWAIN: an alpha app, not a crate | Build our own; port definitions from MIT `TWAIN.cs` | the system's `TWAINDSM.dll`, at run time |
| `scan-worker` | NAPS2's workers (GPL); RSWebTWAIN's sidecar (inside an app) | Build our own | none |
| `scan-image` | None needed: a thin layer over codecs | Build our own, thin | `tiff` 0.11.3, `hayro-ccitt` 0.5.0, `fax` (after its #31 fix), `png` 0.18.1, `zune-jpeg` 0.5.15, `jpeg-encoder` 0.7.1, `image` 0.25.10 |
| `scan-cleanup` | `leptonica` 0.5.0: ported by AI agents, key functions unchecked against C. `aktujo-scan`: two days old | Build our own (keep sheaf's); port from C Leptonica and Doxa | `imageproc` 0.27, `fast_image_resize` 6.1 |
| `patch-codes` | Nothing open in any language; about eight closed SDKs | Build our own (keep sheaf's) | `image` |
| `pdf-images` | PDFBox's `getRawRaster` (Java); hayro-syntax (its own parser, widens to 8 bits) | Build our own; port hayro's filter glue | pdf-filters, pdf-color; `zune-jpeg`, `hayro-ccitt`, `hayro-jbig2` 0.4.0, `hayro-jpeg2000` 0.5.0 |
| `pdf-scan` | Tesseract's `pdfrenderer.cpp` (C++); pdf-writer and krilla (wrong shape) | Build our own; port Tesseract's font design | pdf-structure, pdf-content-streams; PDFium, qpdf and pdf-extract as test oracles |
| `scan-fixtures` | SANE's `test` and `pnm` backends, TWAIN's sample source, Augraphy (Python) | Build our own; port Augraphy's effects | SANE's `test`/`pnm`, TWAIN's sample source, rxing's encoders |

"Good enough" turned out to be a sharp line in 2026. The dependencies that pass it share long histories and many dependents. Most of what fails it is under a year old, has a single author, and is often written by AI agents, while being close enough in shape to tempt. `leptonica` 0.5.0's README says its porting "is carried out primarily by AI coding agents" ([leptonica-rs](https://github.com/tagawa0525/leptonica-rs)). libjpeg-turbo-rs "makes **no memory-safety guarantee**" ([README](https://docs.rs/crate/libjpeg-turbo-rs/0.9.0/source/README.md)). openjpeg2-pure-rs calls itself an "LLM-mediated translation" ([README](https://static.crates.io/readmes/openjpeg2-pure-rs/openjpeg2-pure-rs-0.1.1.html)). jbig2enc-rust's own NOTICE leaves its provenance unresolved ([NOTICE](https://docs.rs/crate/jbig2enc-rust/0.6.0/source/NOTICE)). And printscan's PR bodies end "Generated with Claude Code" ([printscan](https://github.com/b1tninja/printscan)). The notes treat the whole wave the same way: as a cross-check, or a port source with attribution, re-checked at each slice and never taken as a dependency.

The plan's main exposure is concentration rather than absence. One maintainer, Laurenz Stampfl, writes the CCITT, JBIG2 and JPEG 2000 decoders that both `scan-image` and `pdf-images` sit on. Those crates now declare an **MSRV of 1.92** ([hayro-ccitt](https://crates.io/crates/hayro-ccitt)), above the 1.89 decisions.md gives for the family. pdf-content-streams already pins `hayro-ccitt = "0.3"`, two breaking releases behind ([Cargo.toml](~/repos/sheaf-arlington/crates/pdf-content-streams/Cargo.toml)). Otherwise the pure-Rust goal holds: **no route needs C at build time**, provided the optional C features stay off (`tiff`'s `zstd`, `image`'s `avif-native`). Native code appears only at run time (libsane, `TWAINDSM.dll`, the drivers themselves) and in test oracles (PDFium, qpdf, and libtiff run as a separate process).

## Devices: four builds, each over a solid system binding

### sane-dl: the one published binding links at build time and is unsound

`sane-scan` 0.1.2 (March 2023) is still the only published SANE binding, and it fails the plan on every point that matters. It links at build time: its `build.rs` runs bindgen against a hard-coded `/usr/include/sane/sane.h` and emits `rustc-link-lib=sane`, so building it needs libclang and libsane's headers ([build.rs](https://docs.rs/crate/sane-scan/0.1.2/source/build.rs)). It is unsound. Its `lib.rs` passes a pointer to a one-byte Rust `bool` where the backend reads a four-byte `SANE_Word`, reserves `bytes_per_line × lines` even when `lines` is −1, turns a `JAMMED` in mid-page into a truncated `Ok`, and makes SANE's inclusive ranges exclusive ([lib.rs](https://docs.rs/crate/sane-scan/0.1.2/source/src/lib.rs)). And it is unmaintained: five PRs have gone unanswered since 2023. The author of the newest, a rewrite for batch scanning, reports that "compiled with -O3 this library just don't work" ([PR #8](https://github.com/aQaTL/sane-scan/pull/8)), which fits the bool bug.

Since September 2026 about 20 Rust repositories have grown private SANE FFI ([code search](https://github.com/search?q=sane_control_option+language%3Arust&type=code)). Three of them are permissive and load libsane with libloading, exactly as planned. **airbridge** (MIT OR Apache-2.0) has a cancel handle that clears an `alive` flag before close, so a late cancel never touches a closed handle ([sane.rs](https://github.com/tobhoster/airbridge/blob/HEAD/src/backend/sane.rs)). **janus-scan** (MIT) tests against SANE's `test:0` under a private `SANE_CONFIG_DIR` ([tests](https://github.com/januschung/janus-scan/blob/HEAD/crates/backend-sane/tests/sane_test_backend.rs)). **ruscan** (MIT) is a GTK app. Each is days or weeks old with 0 or 1 stars, and none is a library, so they serve as cross-checks rather than dependencies.

Three details from the header and the standard change the bindings. **`sane.h` declares 14 functions, not the 15** that crates.md and proposal 6 state, and its public-domain grant carries a condition: keep the notice and mark any modifications ([sane.h](https://gitlab.com/sane-project/backends/-/blob/master/include/sane/sane.h)). Statuses 12 and 13 sit under `#if 0`, reserved for a later version. A Rust enum built from FFI returns would be undefined behaviour the day a backend returns one, so statuses and frame types should be integers with named constants. And the standard makes `sane_cancel` safe to call asynchronously while forbidding any other call until the cancelled operation returns ([api.rst](https://gitlab.com/sane-project/standard/-/blob/master/api.rst)). The cancel token must therefore be **synchronized with close**, as airbridge's flag is, not merely `Send`.

The batch loop's quirks are all known, and every list of them is copyleft. libinsane states the reason for a worker plainly: some backends "corrupt memory, some cause crashes" ([workarounds.h](https://gitlab.gnome.org/World/OpenPaperwork/libinsane/-/blob/master/subprojects/libinsane/include/libinsane/workarounds.h)). Beyond crates.md's list, the frontends handle Canon DR-C240's separate `ScanMode` option and Lexmark's `scan-both-sides`, Kodak `kds_s2000`'s string-valued `duplex` = `front`/`both`, and Brother's source strings shaped like `Automatic Document Feeder(centrally aligned,Duplex)` ([scanner.vala](https://github.com/GNOME/simple-scan/blob/master/src/scanner.vala)). They also handle option values translated into the user's language, and an ambiguous `INVAL` from `sane_start`, which NAPS2 maps to "offline" where proposal 6 re-reads the descriptors ([SaneScanDriver.cs](https://github.com/cyanfish/naps2/blob/master/NAPS2.Sdk/Scan/Internal/Sane/SaneScanDriver.cs)). NAPS2 also speeds up opening a device with a `dll.conf` that names a single backend, a direct answer to the 10.7-second device listing crates.md wants under a deadline. Two cautions: Brother's `2500x0` is the only quirk with a single source (libinsane, which names two models), and NAPS2's SANE path handles only 1- and 8-bit transfers. `sane-dl`'s 16-bit, padded and unknown-length frames go further than any frontend, so they must be proven against SANE's test backend.

A pure-Rust client for SANE's network protocol (saned) exists, unpublished: printscan-sane, PR #20 in b1tninja/printscan. It is MIT OR Apache-2.0 with no `unsafe`, about 2,300 lines plus tests against a fake saned, and "not tried against real hardware yet" ([PR #20](https://github.com/b1tninja/printscan/pull/20)). The protocol's structures sit in the public-domain `sanei_net.h`, though the standard's text carries no license at all. One trap isn't in the standard's prose: a status byte follows the stream's `0xffffffff` terminator ([net.c](https://gitlab.com/sane-project/backends/-/blob/master/backend/net.c)). Such a client would keep `unsafe` and libsane out of every process sheaf ships. USB scanners would still run sane-backends' C drivers, though, inside saned.

| Need | Verdict | Reason |
|---|---|---|
| Loading libsane at run time | **Use directly**: `libloading` 0.9.0 | ISC, 624M downloads, 1,646 dependents ([crates.io](https://crates.io/crates/libloading)); what every permissive Rust SANE app did |
| Bindings from `sane.h` | **Build our own** | `sane-scan` links at build time and is unsound; nothing else is published. Keep the header's notice and mark changes; 14 functions; statuses as integer constants |
| Checking the bindings against the header | **Port** sanetui's header-contract test idea (Apache-2.0) | Fits "no bindgen"; checks hand-written bindings against the installed `sane.h` in CI ([sanetui](https://github.com/nathawatc/sanetui/tree/HEAD/crates/sane-sys)) |
| Typed options (constraints, units, reload) | **Build our own**; cross-check printscan-sane's `protocol.rs` | No permissive published model; libinsane and NAPS2 are copyleft |
| Frame assembly | **Build our own**; cross-check printscan-sane's `scan/frame.rs` | NAPS2 handles 1- and 8-bit only; prove 16-bit, padding and `lines = -1` against the test backend |
| `Send` cancel token | **Build our own**, after airbridge's alive-flag pattern (permissive) | The standard forbids other calls, close included, until the cancelled call returns |
| Batch-loop and option-name quirks | **Study only** (libinsane, NAPS2, simple-scan) | All copyleft; record the facts in our own table, with a citation per row |
| saned network client | **Study only now**; use printscan-sane and contribute upstream if remote saned is ever wanted | Pure Rust and permissive, but an unpublished PR never run against a real saned |

### scan-device: printscan arrived on the day of the survey and still doesn't fit

The device model has a new near neighbour. **printscan 0.1.0 was published on 2026-11-08**, together with `printscan-windows` and `printscan-wsd`. It is a permissive (MIT OR Apache-2.0) Rust contract close in spirit to proposal 9: ids namespaced as `"<backend>:<native id>"`, an empty feeder reported as `FeederEmpty`, `ScanError::Incomplete` keeping partial pages, and advice on retrying ([docs/scan.md](https://docs.rs/crate/printscan/0.1.0/source/docs/scan.md)). It fails as a dependency for three reasons. Its "pages are encoded images", not raw pixels with format and resolution. Its `scan` feature pulls `image`, `png` and `tiff` under the device layer, which the plan's gate forbids. And it is one day old, from a single account ([crates.io](https://crates.io/crates/printscan)). Its SANE-net, eSCL and ImageCaptureCore backends are still open PRs.

WIA is where printscan earns its keep as a port source. `windows` 0.62.2 is still the latest release and covers the whole WIA 2.0 surface, including transfer callbacks a Rust type can implement, with no C build ([crates.io](https://crates.io/crates/windows)). printscan-windows builds on it with `unsafe_code = "deny"`, 34 documented `SAFETY` sites, and 72 tests over rule tables that need no COM. It records quirks measured on Windows' built-in WSD driver ([crate source](https://static.crates.io/crates/printscan-windows/printscan-windows-0.1.0.crate)). Duplex lives on the Feeder item, which refuses `FRONT_ONLY` and `DUPLEX` together but takes `DUPLEX` alone. `FEED_READY` is a cached reading that can be wrong either way. The page count can read back while the feeder keeps running, so the transfer is cancelled and `WIA_CMD_STOP_FEEDER` sent. Progress should come from bytes, because the percentage peaks near 40 and then jumps to 100. And some WSD drivers fault if two devices are held open.

Those rules also expose three errors in crates.md's `wia` line. First, **on WIA 2.0, `FEEDER` in `WIA_IPS_DOCUMENT_HANDLING_SELECT` has been obsolete since Vista**: the value is `DUPLEX` or `FRONT_ONLY`, written to the root's Feeder child ([Learn](https://learn.microsoft.com/en-us/windows-hardware/drivers/image/wia-ips-document-handling-select)). That is what NAPS2 does, keeping the combined value for WIA 1.0's root only ([WiaScanDriver.cs](https://github.com/cyanfish/naps2/blob/master/NAPS2.Sdk/Scan/Internal/Wia/WiaScanDriver.cs)). Second, **the "undocumented" `0x210001` is `WIA_STATUS_END_OF_MEDIA`**, named in the SDK metadata though absent from Learn. Third, **Learn's archived error table prints the wrong hex for four codes**: cover open, lamp off, multi-feed and the endorser counter appear as 0x80210016, 17, 20 and 21, where the SDK has 0x80210010, 11, 14 and 15 ([Learn](https://learn.microsoft.com/en-us/windows/win32/wia/-wia-error-codes)). printscan-windows measured the SDK values on a driver and maps both spellings, while NAPS2.Wia (MIT) repeats Learn's.

macOS is a use-directly. `objc2-image-capture-core` 0.3.2 (Zlib OR Apache-2.0 OR MIT) binds `ICScannerDevice`, the functional units and band data ([crates.io](https://crates.io/crates/objc2-image-capture-core)), and `scanline` (MIT) shows a current, permissive session flow ([scanline](https://github.com/klep/scanline/blob/master/README.md)). What's missing is a safe scanning layer over those raw bindings, which is the hole the `ica` backend would fill.

Network protocols stay out, and the office makes the reason concrete. The fi-6130, fi-7160 and fi-8170 are USB-only for scanning as far as any source shows ([PFU](https://www.pfu.ricoh.com/fi/software/multios-driver/)). Of 205 devices in sane-airscan's tested list, only one desktop document scanner, the Brother ADS-4300N, speaks eSCL ([sane-airscan](https://github.com/alexpevzner/sane-airscan#compatibility)). If eSCL is ever wanted, three facts matter. `escl` 0.3.0 is too thin: it has no 503 retry and no job DELETE, and its discovery returns only the first responder ([lib.rs](https://docs.rs/crate/escl/0.3.0/source/src/lib.rs)). Mopria's specification may be used internally but never redistributed, and it grants no patent rights ([Mopria](https://mopria.org/spec-download)). And OpenPrinting's go-mfp (BSD-2) is the permissive model to port ([go-mfp](https://github.com/OpenPrinting/go-mfp)).

No pure-Rust driver exists for any document scanner. The nearest precedents are nkscan, a Nikon film-scanner driver on `nusb`, and scan2drive's fss500, a Go driver for the ScanSnap iX500 written from USB captures ([fss500](https://github.com/stapelberg/scan2drive/tree/HEAD/internal/fss500)).

Driver-reported patch codes are a small optional addition. TWAIN 2.5 defines `ICAP_PATCHCODEDETECTIONENABLED` and a per-image `TWEI_PATCHCODE` ([twain.h](https://github.com/twain/twain-dsm/blob/master/TWAIN_DSM/src/twain.h)), and WIA 2.0 has a Patch Code Reader item ([Learn](https://learn.microsoft.com/en-us/windows-hardware/drivers/image/wia-ips-patch-code-reader)). SANE exposes nothing, and neither TWAIN nor WIA says which edge a code lies along, so these can only be hints beside pixel reading.

| Need | Verdict | Reason |
|---|---|---|
| Device model, ids, errors | **Build our own**; study printscan's contract | Close in spirit, but encoded pages, `image`/`png`/`tiff` under devices, one day old, MSRV 1.95 |
| `fake` backend | **Build our own** | Nothing scripted at this level exists; printscan's fakes sit behind its own trait |
| WIA 2.0 COM surface | **Use directly**: `windows` 0.62.2 | Complete, maintained by Microsoft, no C build |
| WIA quirks and error mapping | **Port** from printscan-windows (MIT OR Apache-2.0) and NAPS2.Wia (MIT), checking values against `windows` | WSD-measured rules; Learn's table is wrong for four codes |
| printscan or printscan-windows as dependencies | **Study only for now**; contribute upstream later if the contract settles | In-process, its own contract, codecs under devices; re-check before S3 and S4 |
| macOS (`ica`) | **Use directly** `objc2-image-capture-core` 0.3.2, contributing missing bindings upstream; port the session flow from scanline (MIT) | Current, permissive bindings to a system framework; NAPS2's Apple driver is study only |
| Driver-reported patch codes | **Build our own**, optional | TWAIN and WIA define them; SANE has nothing; neither reports the edge |
| eSCL client | **Not needed**; if ever, build our own, porting go-mfp (BSD-2) | The office's scanners are USB-only; `escl` 0.3.0 is too thin; sane-airscan's quirks are GPL facts |
| WSD-Scan client | **Study only**; printscan-wsd if ever needed | Already reached through WIA and sane-airscan |
| mDNS | **Use directly**: `mdns-sd` 0.21.7, if a network backend is built | Pure Rust, `forbid(unsafe_code)`, 7.4M downloads ([crates.io](https://crates.io/crates/mdns-sd)) |
| Pure-Rust USB driver for fi-series scanners | **Don't build**; if ever, port fss500 (Apache-2.0) onto `nusb` | SANE's fujitsu backend and PFU's own driver already reach these scanners through libsane |
| TWAIN Direct | **Study only** | No commits since 2022; no shipping devices found |

### twain-dsm: definitions from MIT TWAIN.cs, the state machine's shape from RSWebTWAIN

**Correction** (S0 slice 7, 2026-11-08): the definitions are in `TWAINH.cs`, twain-cs's translation of
`twain.h`, under the same MIT notice; `TWAIN.cs` is the wrapper over them. Read "`TWAINH.cs`" for "`TWAIN.cs`"
where this survey means the definitions (`crates/twain-dsm.md`).

Nothing in Rust is usable. `twain` 0.1.0 (2021) loads `./lib/windows/TWAINDSM.dll` relative to the working directory. It passes an immutable local as the out-pointer the data source writes through, and never calls `GlobalLock`. It also bundles the DSM and Qt's `qwindows.dll` inside an MIT crate, without their LGPL text ([crate source](https://static.crates.io/crates/twain/twain-0.1.0.crate)).

RSWebTWAIN, an alpha Tauri tray app with no stars, is the only Rust code with the planned shape: a 64-bit app plus a 32-bit sidecar speaking JSON lines. Its seven state structs, which consume `self` and step down on `Drop`, are worth porting as a design ([RSWebTWAIN](https://github.com/seancampbell3161/RSWebTWAIN)). Its code is not. It keeps two hand-copied bindings files from `twain.h` 2.3, which its own test says "have silently diverged before". It passes a Rust `Box` pointer as `hContainer`, where TWAIN expects a handle from `GlobalAlloc`. Its memory transfer is uncompressed only and ignores tile offsets. And it needed a fix to six wrong constants as recently as September 2026.

crates.md's open license question now has an answer. **`twain.h` grants no license in any copy checked.** It carries copyright notices and "All rights reserved", and it isn't covered by the DSM's LGPL ([twain.h](https://github.com/twain/twain-dsm/blob/HEAD/TWAIN_DSM/src/twain.h)). The only grant is the TWAIN License on twain.org. It allows royalty-free reproduction and distribution of "the TWAIN toolkit", on condition that software based on it carry a fixed warranty disclaimer in its header and documentation ([Wayback 2024](https://web.archive.org/web/20240214221503/https://twain.org/twain-license/)). Whether a Rust transcription counts as such software is a question for counsel.

The clean route sidesteps it: **the Working Group's own C# translation, `TWAIN.cs`, is plain MIT** (Kodak Alaris), current to 2.5, with layouts for Windows, Linux and macOS ([TWAIN.cs](https://github.com/twain/twain-cs/blob/HEAD/twaincs/source/TWAIN.cs)). Port the definitions from it, keep its notice, check the values against `twain.h` 2.5, and add the TWAIN disclaimer anyway: a warranty disclaimer doesn't conflict with MIT or Apache-2.0.

How to drive the DSM comes from NAPS2 (study only) and from the specification. NAPS2 defaults to memory transfer, using native transfer only for Kyocera, and passes `modal = false` when enabling the source. It cancels by state: `ForceStepDown` in states 6 and 7, but a natural cancel in state 5, where forcing it "seems to produce an error message from the scanner". It copies tiles at their `XOffset` and `YOffset`, and trusts only the first `BitsPerSample` entry, because "some scanners only set the first value" ([TwainScanRunner.cs](https://github.com/cyanfish/naps2/blob/master/NAPS2.Sdk/Scan/Internal/Twain/TwainScanRunner.cs), [TwainMemoryBufferReader.cs](https://github.com/cyanfish/naps2/blob/master/NAPS2.Sdk/Scan/Internal/Twain/TwainMemoryBufferReader.cs)). Its 8.6.0 release fixed TWAIN hangs by running the worker on an STA thread ([CHANGELOG](https://github.com/cyanfish/naps2/blob/master/CHANGELOG.md)), and it parents sources to a real hidden top-level window, not the message-only (`HWND_MESSAGE`) window RSWebTWAIN uses ([Win32MessagePump.cs](https://github.com/cyanfish/naps2/blob/master/NAPS2.Sdk/Platform/Windows/Win32MessagePump.cs)).

The specification says the official DSM is signed and can be verified, that applications must install it themselves and link to it at run time, and that all calls should come from one thread ([TWAIN 2.4](https://github.com/twain/twain-specification/blob/HEAD/versions/2.4/TWAIN-2.4-Specification.pdf)). So load it with a system-folder search and an Authenticode check, rather than by bare name, which searches the application's folder first.

| Need | Verdict | Reason |
|---|---|---|
| Constants, structures, `DSM_Entry` | **Port** from `TWAIN.cs` (MIT), checked against `twain.h` 2.5; add the TWAIN Toolkit disclaimer | `twain.h` grants nothing; `TWAIN.cs` is the Working Group's own MIT translation |
| Loading the DSM | **Use directly** the unmodified `TWAINDSM.dll`, through our own `LoadLibraryExW` with a system-folder search, plus an Authenticode check | The spec says link at run time, and the official DLL is signed; DSM 2.5.1 (2023) is the latest |
| The seven states | **Build our own**, porting RSWebTWAIN's typestate shape (MIT OR Apache-2.0) | The only design in Rust; the idea is sound, the code alpha |
| Window and message loop | **Build our own** on `windows`: an STA thread, a real hidden top-level window, a child or owner window for driver dialogs | NAPS2's STA fix and window design (study only) |
| Capability containers | **Port** the encoding from `TWAIN.cs` or NTwain (MIT); allocate through `DSM_MemAllocate`, else `GlobalAlloc` | RSWebTWAIN passes a `Box`. NTwain fixed a string-size bug in May 2026 ([NTwain](https://github.com/soukoku/ntwain)), so the bug is an easy one to make |
| Memory and native transfer | **Build our own**: memory by default, native per driver; tiles at their offsets; accept a single `BitsPerSample` entry | No permissive code handles tiles; NAPS2's rules are study only |
| One bindings crate for both bitnesses | **Build our own** | RSWebTWAIN's two hand-kept copies diverged |
| `twain` 0.1.0 | **Study only** (don't use) | Undefined behaviour, a relative DLL path, bundled LGPL DLLs without their license text |
| NAPS2, NAPS2.Sdk, pytwain, Saraff.Twain | **Study only** | GPL-2.0+, LGPL-2.1+, GPL-2.0 ([pytwain](https://github.com/denisenkom/pytwain/blob/master/COPYING.txt)) and LGPL-3.0+ |

### scan-worker: everyone isolates drivers in a process, and nobody publishes the broker

Every tool that drives 32-bit TWAIN from a 64-bit process uses a separate process of the driver's bitness, and none does it in-process. **NAPS2** uses gRPC over named pipes, a Windows Job object so its workers die with the parent, and spare workers started ahead in both bitnesses ([WorkerFactory.cs](https://github.com/cyanfish/naps2/blob/master/NAPS2.Sdk/Remoting/Worker/WorkerFactory.cs)). **RSWebTWAIN** uses JSON lines over stdin and stdout, **VintaSoft** a 32-bit service executable ([VintaSoft](https://www.vintasoft.com/docs/vstwain-dotnet/Programming-Twain-How_To_Use_32bit_TWAIN_driver_in_64bit_Application.html)), and **LEADTOOLS** a COM+ thunk server ([LEADTOOLS](https://www.leadtools.com/help/sdk/dh/to/using-the-leadtools-thunk-utility-with-twain.html)).

The plan's JSON-lines worker with a watchdog matches the field and is simpler than NAPS2's. Three rules are worth adding. A Job object with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` means workers never outlive the app. The worker's stdout and stderr should be drained: NAPS2 carries a TODO that it doesn't, and undrained output can stall a worker. And an optional spare worker can be kept warm. No Rust broker exists as a library, so this crate fills a hole.

| Need | Verdict | Reason |
|---|---|---|
| JSON-lines protocol, the worker's loop, the client | **Build our own** | No Rust broker library exists; NAPS2's (gRPC, GPL) and libinsane's (fork, LGPL) are study only |
| A 32-bit worker for 32-bit TWAIN | **Build our own**, as planned | NAPS2 and every commercial vendor use a separate process |
| Lifetime and hygiene | **Build our own**: a Job object that kills on close, drained pipes, an optional warm spare | NAPS2's practice (study only) |
| saned spawned as the worker | **Study only** (an untested idea) | It would replace in-process libsane with an unmodified GPL program speaking a public-domain protocol. It needs `sane-utils`, and whether an unprivileged `saned -l -o` works is unverified ([saned.man](https://gitlab.com/sane-project/backends/-/blob/master/doc/saned.man)) |

## Pages: borrow the codecs, keep sheaf's cleanup and patch reader

### scan-image: route every CCITT strip through hayro-ccitt

`scan-image` is a thin crate over borrowed codecs, and the survey's main change is which codec decodes fax images. `tiff` 0.11.3 (February 2026, image-rs) is still the latest release. It reads the basics well, but it refuses compressions 2 and 3, and its G4 reader converts both dimensions with `u16::try_from` ([image.rs](https://docs.rs/crate/tiff/0.11.3/source/src/decoder/image.rs), [stream.rs](https://docs.rs/crate/tiff/0.11.3/source/src/decoder/stream.rs)). The `main` branch has merged G3 1D decoding with FillOrder ([PR #363](https://github.com/image-rs/image-tiff/pull/363)). But it never reads `T4Options`, so 2D-coded G3 isn't handled, and nothing has been released for nine months. A Modified Huffman PR has sat unreviewed since September ([PR #414](https://github.com/image-rs/image-tiff/pull/414)).

`hayro-ccitt` 0.5.0 (2026-11-02) covers all of it. It decodes G4, G3 1D and G3 2D; detects EOLs whatever the flag says; handles byte-aligned rows (and so Modified Huffman) and end of block; recovers from damaged lines; and has a strict mode. It is `no_std`, `forbid(unsafe_code)` and has no dependencies ([group3.rs](https://docs.rs/crate/hayro-ccitt/0.5.0/source/src/group3.rs)).

So `scan-image` should take the raw strip bytes from tiff's IFD and send **every CCITT strip, compressions 2, 3 and 4**, through one hayro-ccitt adapter, the same adapter `pdf-images` needs. That fixes BUG-2 for every value of K. It also lets a G4 TIFF 70,000 pixels wide, which BUG-1's fix lets the crate write, be read back. The cost is that 0.5.0 broke the API: rows arrive as colour-change positions, `invert_black` is gone, and `Group3_2D` lost its `k` field ([CHANGELOG](https://docs.rs/crate/hayro-ccitt/0.5.0/source/CHANGELOG.md)).

G4 encoding stays with `fax`, but BUG-1's planned fix isn't enough. `fax` 0.3.0 moved widths to `u32`, yet its G4 encoder computes `a1 as i16 - b1 as i16`, which panics under overflow checks whenever the two positions straddle 32,767 ([issue #30](https://github.com/pdf-rs/fax/issues/30)). The fix merged on 2026-11-05 and hasn't been released. **BUG-1's own test, a 70,000 × 100 page run with overflow checks on, would hit it.** Pin the merge commit, wait for 0.3.1, or refuse widths over 32,767 until then.

`tiff` writes nothing 1-bit or CCITT, so sheaf's G4 TIFF directory writer stays, while LZW and Deflate multi-page output can use `tiff` directly. BUG-3's two-sample gray needs nothing upstream: `tiff` decodes it as `Multiband { num_samples: 2 }` "for compatibility", and it's the callers that refuse it. BUG-4 is confirmed: `tiff` exposes the Orientation tag and doesn't apply it, while `image`'s TIFF decoder maps it ([image tiff.rs](https://docs.rs/crate/image/0.25.10/source/src/codecs/tiff.rs)).

JPEG divides cleanly. **zune-jpeg 0.5.15 decodes** baseline, extended and progressive 8-bit Huffman JPEGs, with Adobe APP14. It refuses 12-bit, lossless and arithmetic-coded files. Release candidate 0.5.16-rc3 adds arithmetic decoding, but it still produces only 8-bit pixels ([rc3 headers.rs](https://docs.rs/crate/zune-jpeg/0.5.16-rc3/source/src/headers.rs)). Those files become typed refusals until the corpus shows how common they are.

**jpeg-encoder 0.7.1 encodes**: CMYK, custom tables and progressive output, with no `unsafe` unless its `simd` feature is on. image's `main` branch is moving to it, and its "(MIT OR Apache-2.0) AND IJG" license asks for a notice in the documentation ([README](https://static.crates.io/readmes/jpeg-encoder/jpeg-encoder-0.7.1.html)).

Lossless transforms aren't needed, because the EXIF turn becomes `/Rotate`. The only pure-Rust candidate for them, libjpeg-turbo-rs, carries about 326 `unsafe` sites across 72,000 lines. EXIF orientation is a few dozen bytes of IFD0, so sheaf's reader stays, with kamadak-exif 0.6.2 as fallback; that release just fixed an O(n³) allocation ([NEWS](https://docs.rs/crate/kamadak-exif/0.6.2/source/NEWS)).

PNG goes to `png` 0.18.1, which forbids `unsafe` and runs on OSS-Fuzz ([lib.rs](https://docs.rs/crate/png/0.18.1/source/src/lib.rs)). zune-png expands images below 8 bits to 8, which is wrong for 1-bit scans. `image` 0.25.10 stays for BMP, GIF and WebP and as the pixel-buffer type, with default features off. Its own changelog warns that "Many decoders will panic on malicious input" ([CHANGES.md](https://github.com/image-rs/image/blob/main/CHANGES.md)).

| Need | Verdict | Reason |
|---|---|---|
| TIFF IFDs, multi-page, LZW/Deflate/PackBits/JPEG-7, planar, BigTIFF | **Use directly**: `tiff` 0.11.3 | Mature, fuzzed, image-rs |
| TIFF CCITT strips (compressions 2, 3, 4; FillOrder; T4Options) | **Use directly** `hayro-ccitt` 0.5.0 through our own adapter; contribute T4Options and `u32` G4 to `tiff` when cheap | The release refuses 2 and 3 and caps G4 at 65,535 px; `main`'s G3 is 1D only and unreleased |
| CCITT decode | **Use directly**: `hayro-ccitt` 0.5.0 | Every mode, recovery, strict mode, no `unsafe`; one maintainer, MSRV 1.92 |
| G4 encode | **Use and contribute upstream**: `fax` with the #31 fix (0.3.1 or a pinned revision) | 0.3.0 has `u32` widths but panics past 32,767 px under overflow checks |
| Writing G4 TIFFs | **Build our own** directory writer (keep sheaf's) | `tiff` writes no 1-bit or CCITT ([compression/mod.rs](https://docs.rs/crate/tiff/0.11.3/source/src/encoder/compression/mod.rs)) |
| Writing LZW multi-page TIFFs with resolution | **Use directly**: `tiff` | Supported |
| Two-sample gray, CMYK16, float (BUG-3) | **Build our own** mapping | `tiff` decodes them; the callers refuse |
| TIFF Orientation (BUG-4) | **Build our own**: one orientation step on every path | `tiff` exposes the tag and doesn't apply it |
| JPEG decode | **Use directly**: `zune-jpeg` 0.5.15; typed refusals for 12-bit and arithmetic coding | 8-bit only; `arith` only in a release candidate |
| JPEG encode | **Use directly**: `jpeg-encoder` 0.7.1, with the IJG notice in the docs | CMYK and custom tables; image's `main` adopts it |
| Lossless JPEG rotation | **Study only** (libjpeg-turbo-rs; libjpeg-turbo's `transupp.c`) | Not needed while `/Rotate` carries the turn |
| EXIF orientation | **Build our own** (keep sheaf's reader); `kamadak-exif` 0.6.2 as fallback | A few dozen bytes |
| PNG in and out | **Use directly**: `png` 0.18.1 | `forbid(unsafe_code)`, OSS-Fuzz, keeps depths below 8 bits |
| BMP, GIF, WebP in; pixel buffers | **Use directly**: `image` 0.25.10, default features off | Avoids rayon, ravif and the exr dependency tree |
| Page count from headers (FEAT-1) | **Build our own** (keep `count_frames`) | A walk over the headers |

### scan-cleanup: imageproc stays the base and Leptonica becomes the port source

No Rust crate cleans documents better than sheaf's own code. The audit's "only small apps and one-off crates" missed three newcomers. **`leptonica` 0.5.0** (BSD-2, pure Rust, no `unsafe`, 154,000 lines) ports skew, background normalization, Sauvola, orientation, dewarping and page-foreground finding. But it has one star, and its own C-compatibility report leaves out exactly the skew, background-map, binarization, dewarp and orientation code ([c-compat-status](https://github.com/tagawa0525/leptonica-rs/blob/main/docs/porting/c-compat-status.md)). **seiza-imgproc** (Apache-2.0) has an f64 Otsu bit-exact with OpenCV, but it is built for astrophotography and has one author ([crates.io](https://crates.io/crates/seiza-imgproc)). **aktujo-scan** (MIT, two days old, 13 tests) does corners, unwarping and projection deskew, and thresholds with a mean rather than Sauvola ([crates.io](https://crates.io/crates/aktujo-scan)).

imageproc 0.27.0 remains the base. It is MIT, has 410 dependents, and offers contours, projective warps and labelling that no alternative matches ([crates.io](https://crates.io/crates/imageproc)). Its weak spots are now pinned down in its source. **BUG-5 is confirmed**: `otsu_level` forms `threshold as u32 * hist_count` in u32, which overflows once one gray level has more than 16,843,009 pixels, and no issue has been filed ([contrast.rs](https://docs.rs/crate/imageproc/0.27.0/source/src/contrast.rs), [issue search](https://github.com/image-rs/imageproc/issues?q=otsu)). The same class of overflow sits in `adaptive_threshold`, whose u32 integral image overflows once a page's total brightness passes 2³². Canny's open hysteresis bug can't be reached with sheaf's thresholds ([PR #746](https://github.com/image-rs/imageproc/pull/746)). And rayon covers maps, filters and warps, but not median, morphology, labelling or integral images.

Speed is less dire than crates.md's open item suggests. Slice 6's gate is 2.3 pages a second on four cores, about 1.74 core-seconds a page. The Scanner profile's measured 0.8 s on one thread fits by page parallelism alone. The Photo profile's 3.2 s gives about 1.25 pages a second, so it needs a page 1.4 to 2 times faster.

Read from the code rather than a profile, the likely hot spots are the naive O(r) `max_filter` and `median_filter`, three upsamples of the background to full size, single-threaded Sauvola and labelling at full size, and full-resolution bilinear rotation of RGB pages. Permissive remedies exist for each: van Herk/Gil-Werman min and max filters, Leptonica's tile-based background map, skew measured on a binary image reduced four times, and rayon over Sauvola's rows. fast_image_resize also helps; its README measures L8 bilinear at 5.48 ms against `image`'s 57.97 ms on one thread ([fast_image_resize](https://github.com/Cykooz/fast_image_resize)). Under the byte-identical rule, each of these is an output change that needs its own decision.

C Leptonica (BSD-2) is the port source. Its skew finder uses Postl's differential line sums, which come with a **confidence**, so a page of pictures can say "no skew" instead of guessing ([leptonica.org](http://www.leptonica.org/skew-measurement.html)). It also offers tile-based background normalization, hit-miss up/down detection (a model-free signal for the 180° case that word shape can't settle), and page-foreground finding for auto-crop. Doxa (CC0) adds 18 binarization algorithms and BinBench, a DIBCO scorer its author claims is bit-exact ([Doxa](https://github.com/brandonmpetty/Doxa)).

The tools with richer features (unpaper, ScanTailor Advanced, gscan2pdf) are GPL and only set the bar: per-step switches, a deskew that refuses when its evidence disagrees, and a despeckle strength ([unpaper](https://github.com/unpaper/unpaper/blob/main/doc/image-processing.md)). sheaf's blank check, with its paper-relative level, its cells that ignore specks and its "nearly blank" verdict, is already stronger than NAPS2's fixed white threshold ([BlankDetectionImageOp.cs](https://github.com/cyanfish/naps2/blob/master/NAPS2.Images/Bitwise/BlankDetectionImageOp.cs)).

Models stay behind the trait. UVDoc's weights are MIT and lead their class on the corrected benchmark ([UVDoc](https://github.com/tanguymagne/UVDoc)), and PP-LCNet's orientation model is 7 MB under Apache-2.0. DocTr is excluded as non-commercial ([LICENSE](https://github.com/fh2019ustc/DocTr/blob/main/LICENSE.md)), and ocrs's models as CC BY-SA. Among runtimes, rten 0.28 and tract-onnx 0.23.10 implement the `GridSample` operator UVDoc needs; candle-onnx 0.11.1 doesn't.

| Need | Verdict | Reason |
|---|---|---|
| Primitives (blur, Canny, contours, warps, labelling, morphology) | **Use and contribute upstream**: `imageproc` 0.27 | Maintained, MIT, 410 dependents; the Otsu and `adaptive_threshold` overflows are unreported |
| Otsu (BUG-5) | **Build our own**, u64 or f64 (as `patch-codes` has), and offer it upstream | Confirmed overflow; seiza's f64 port of OpenCV shows OpenCV's tie-breaking |
| Deskew | **Build our own** (keep sheaf's); **port** Leptonica's Postl sums, image reductions, confidence and shear rotation | No crate is better; Leptonica adds a confidence and a cheaper pipeline |
| Sauvola and other binarization | **Build our own** (keep sheaf's); **port** Wolf, NICK or Bradley from Doxa (CC0) if a measurement asks for them | sheaf's Sauvola equals Leptonica's apart from tiling |
| Light flattening | **Build our own** (keep sheaf's); **port** Leptonica's `pixBackgroundNorm` and clean-to-white if they measure faster | Tile averages cost less than a median filter |
| Min and max filters | **Build our own** (van Herk/Gil-Werman) | O(1) per pixel; not in imageproc |
| Despeckle and morphology | **Use directly** `imageproc`; try `fast_morphology` only after a measurement | fast_morphology has 0 dependents and 68 `unsafe` |
| Resampling | **Use directly**: `fast_image_resize` on new code paths | About 10× faster on one thread |
| Page corners and perspective | **Build our own** (keep sheaf's); study aktujo-scan (MIT) | aktujo-scan is two days old |
| Up/down without a model | **Port** Leptonica's `pixUpDownDetect` and `pixOrientDetect` | Settles 180°; weak beyond Latin text |
| Auto-crop, border removal | **Build our own** (keep `clear_edges`); **port** `pixFindPageForeground` as a candidate | unpaper and ScanTailor are GPL |
| Dewarping without a model | **Study, port later** (Leptonica's textline model; page-dewarp, MIT) | UVDoc covers Photo through the trait |
| Blank pages | **Build our own** (keep sheaf's) | Stronger than any open prior art found |
| Punch holes, colour dropout | **Build our own** if wanted, after checking the patents | Only commercial SDKs and patents exist |
| Models (UVDoc, orientation) | **Use directly**, through the caller's trait, on rten or tract after a spike | Permissive weights; candle-onnx lacks `GridSample` |
| `leptonica` 0.5.0 as a crate | **Study only**, a reading aid beside C Leptonica | AI-written, one star, its key functions unchecked against C |
| unpaper, ScanTailor Advanced, gscan2pdf, NAPS2 | **Study only** | GPL |

### patch-codes: no open reader exists in any language or under any license

`patch-codes` is the clearest hole in the stack: **no open-source patch-code reader or writer exists, under any license, in any language.** crates.io, npm and NuGet have none, and neither do ZXing's descendants. NAPS2 and Paperless-ngx "support" patch T by reading the Code 39 text printed on the sheet, not the bars ([BarcodeDetector.cs](https://github.com/cyanfish/naps2/blob/master/NAPS2.Sdk/Scan/BarcodeDetector.cs)). About eight closed SDKs read the bars, among them Accusoft, Dynamsoft (since 2019), LEADTOOLS, Aspose and VintaSoft, and none publishes its accuracy or its method ([NuGet](https://azuresearch-usnc.nuget.org/query?q=patchcode), [Dynamsoft](https://www.dynamsoft.com/Barcode-Types/Patch-Code.aspx)).

Kodak's A-61599 (2006) is public with only a copyright notice, and its zone tables match sheaf's bar table exactly ([A-61599](https://archive.org/download/manualsonline-id-e24f73ef-f1cf-4f31-bc70-8aae8572f136/e24f73ef-f1cf-4f31-bc70-8aae8572f136.pdf)). It also corrects the plan. **crates.md's "at least 0.75 in long" belongs to Kodak's perpendicular-bar layout** (i280, 3590C and i600, toggle patch only). The parallel layout the reader handles needs bars at least 2 in long, with 2.5 in recommended, placed within 3.75 in of the lead edge.

Kodak's own software reader searches only the top 4 in after rotation, with an optional retry at 180° ([Capture Pro guide](https://www.kodakalaris.com/sites/default/files/2024-11/a61957_capture_pro_admin_guide_en.pdf)). A four-edge reader that reports the edge is therefore a strict superset. Capture suites disagree on whether the separator sheet is kept; Tungsten TotalAgility keeps it by default ([TotalAgility](https://docshield.tungstenautomation.com/KTA/en_US/8.0.0-ivy45l9g96/help/Designer/All_Shared/Capture/t_profilesepforadocument.html)). That supports proposal 14's per-rule choice to drop it.

Barcodes stay in sheaf, and rxing stays the reader. Version 0.9.4 shipped on the survey day, outside contributors fix bugs weekly, and v0.10 is due in December to lift the exact `image = "=0.25.8"` pin ([issue #87](https://github.com/rxing-core/rxing/issues/87)). Its weaknesses are documented. It has no ported negative tests, no fuzzing, and CI that doesn't test feature combinations ([README](https://github.com/rxing-core/rxing/blob/main/README.md)). In a benchmark by zxing-cpp's author, which he calls "personal (and biased)", it made 10 false reads on 25 code-free images with every format enabled, and ran about 6.7× slower than zxing-cpp on BoofCV's hard QR set ([zxing-bench](https://github.com/axxel/zxing-bench)). sheaf's format limits held false reads to 0 of 150 real scans, so the right response is upstream work: negative tests, fuzz targets and a CI matrix.

zxing-cpp needs C++20 and CMake, and its wrapper has 48 `unsafe` blocks, which makes it fit only as a test oracle. `anyd` (MIT, written from scratch, `forbid(unsafe_code)`, accepting a 1D read only when scan lines agree) deserves an hour in the bench ([anyd](https://crates.io/crates/anyd)).

For writing, rxing's encoders suffice, with two cautions. Every Rust Code 39 encoder draws wide bars at twice the narrow width, so a Code 39 printed at another ratio has to be drawn from the pattern ([code_39_writer.rs](https://github.com/rxing-core/rxing/blob/main/src/oned/code_39_writer.rs)). And rxing's Data Matrix writer still panics past 144×144, so the input needs capping ([#106](https://github.com/rxing-core/rxing/issues/106)).

| Need | Verdict | Reason |
|---|---|---|
| Reading patch codes 1, 2, 3, 4, 6 and T on any edge, with the edge | **Build our own** (keep sheaf's reader) | Nothing open exists; A-61599 is public; the reader passed 204/204 synthetic pages and 60/60 renders |
| Drawing Kodak's layout | **Build our own** | Four rectangles from the zone tables; only commercial writers exist |
| The perpendicular-bar layout | **Build our own, or document it as a non-goal** | LEADTOOLS offers a vertical search; Kodak's toggle-only layout |
| Otsu inside the reader | **Build our own** (keep its u64 version) | Avoids BUG-5's overflow |
| Barcode reading (sheaf's) | **Use and contribute upstream**: rxing 0.9.4 | The only pure-Rust, permissive multi-format reader with users; its gaps are fixable upstream |
| A second engine | zxing-cpp **as a test oracle only**; `anyd` **evaluated in the bench**; zedbar, zbar-rust, masuri **study only** (LGPL) | zxing-cpp needs C++20 and CMake; anyd is four months old |
| Barcode writing for sheaf's separator sheet | **Use directly**: rxing's encoders; `qrcode` and `datamatrix` as fallbacks | Mature and permissive; draw Code 39 from its pattern; cap Data Matrix input |

## PDF: port hayro's glue and Tesseract's font onto the family

### pdf-images: only Java's PDFBox exposes native samples

**No library in any language returns an image's samples in its own color space, depth and `/Decode` for objects the caller parsed.** PDFBox's `getRawRaster()` is the API model, but it is Java ([PDImage.java](https://github.com/apache/pdfbox/blob/trunk/pdfbox/src/main/java/org/apache/pdfbox/pdmodel/graphics/image/PDImage.java)). The Rust candidates all fall short. **hayro-syntax 0.9.0** has a public `Stream::decoded_image` that runs every filter and codec, but only on a `Stream` hayro's own parser produced (`Stream::new` is crate-private). It also widens CCITT, JBIG2 and JPX to 8 bits unless the image is Indexed, "so that it can be easier converted into an RGBA8 image" ([stream.rs](https://docs.rs/crate/hayro-syntax/0.9.0/source/src/object/stream.rs), [ccitt.rs](https://docs.rs/crate/hayro-syntax/0.9.0/source/src/filter/ccitt.rs)). Adopting it would mean a second parser beside pdf-structure, against the family's design of a single object layer. **hayro-interpret, PDFium and pdf.js** hand out 8-bit RGB, gray or RGBA. **pdf-rs** panics on CCITT with any `K >= 0` and narrows widths to `u16` ([enc.rs](https://docs.rs/crate/pdf/0.10.0/source/src/enc.rs)). **lopdf** returns the encoded bytes. And **pdf_oxide** counts every DeviceN space as four components, whatever N is ([images.rs](https://docs.rs/crate/pdf_oxide/0.3.78/source/src/extractors/images.rs)).

What's worth taking is hayro's PDF-level glue, which is small and permissive:

| File | Lines | What it handles |
|---|---|---|
| `dct.rs` | 181 | The 65,535 guard, SOF dimension patching, `/ColorTransform`, APP14 overrides, YCCK |
| `ccitt.rs` | 256 | Parameter mapping, recovery of partial rows |
| `jbig2.rs` | 112 | Globals, then `Page::new_embedded`, then the black=1 inversion |
| `jpx.rs` | 130 | The alpha split for SMaskInData |

To those, add hayro-interpret's `unpack_samples` and `fix_image_length`, and pdf.js's Matte un-premultiply and fallback dimensions for corrupt soft masks ([dct.rs](https://docs.rs/crate/hayro-syntax/0.9.0/source/src/filter/dct.rs), [image.js](https://github.com/mozilla/pdf.js/blob/master/src/core/image.js)). Port them with 1-bit output kept packed, and with JPX read through hayro-jpeg2000's `components()`. That gives f32 samples at native bit depth, which corrects the audit's claim that 16-bit JPX drops to 8 bits ([lib.rs](https://docs.rs/crate/hayro-jpeg2000/0.5.0/source/src/lib.rs)). A comment in `dct.rs` calls its marker scanner "AI-generated", so it needs tests of its own. No candidate caps `Width × Height × components × bpc` before decoding beyond hayro's DCT guard, so the plan's cap has no prior art to lean on.

The codecs themselves are use-directly. hayro-jbig2 0.4.0 still calls itself "under development", but it now reads multi-page files, shares `Globals` between pages, has a strict mode and is fuzzed ([CHANGELOG](https://docs.rs/crate/hayro-jbig2/0.4.0/source/CHANGELOG.md)). Every alternative fails on provenance or age. justbig2 says it is "Ported from jbig2dec", which is AGPL ([arith_int.rs](https://docs.rs/crate/justbig2/0.2.0/source/src/arith_int.rs)). jbig2enc-rust's provenance is unresolved by its own NOTICE. And Microsoft's `jbig2-decoder`, first released on 2026-11-02, says it "is not yet suitable for untrusted production documents" ([README](https://static.crates.io/readmes/jbig2-decoder/jbig2-decoder-0.1.2.html)).

| Need | Verdict | Reason |
|---|---|---|
| Image dictionary, read and capped before decoding | **Build our own** on pdf-objects and pdf-color | No candidate caps the area; port hayro's DCT guard, short-data padding and inline space names as ideas |
| Stream filters | **Use directly**: pdf-filters | The family's |
| DCT at the PDF level | **Port** hayro-syntax's `filter/dct.rs` onto zune-jpeg; test its marker scanner | 181 permissive lines; the scanner is marked AI-generated |
| CCITT parameters | **Port** `filter/ccitt.rs`'s mapping and partial-row recovery, keeping 1-bit output packed | hayro widens to 8 bits |
| JBIG2 with `/JBIG2Globals` | **Port** `filter/jbig2.rs`'s glue onto hayro-jbig2 0.4.0's `File`, `Page` and `Globals` | 112 lines; the API changed in 0.4.0 |
| JBIG2 decode | **Use and contribute upstream**: hayro-jbig2 0.4.0 | The only clean, active decoder |
| JPX, SMaskInData, 16-bit | **Port** `filter/jpx.rs`'s alpha split; read `components()` at native depth; take SMaskInData's semantics from pdf.js (Apache-2.0) | hayro-jpeg2000 exposes f32 samples with their bit depth |
| JPX decode | **Use directly**: hayro-jpeg2000 0.5.0 | Tested on 20,000+ images from PDFs; OpenJPEG (BSD-2, C) as an oracle only |
| Samples in the image's own space | **Build our own**, porting hayro-interpret's `unpack_samples` and `fix_image_length`; model the API on PDFBox's `getRawRaster` | Nothing in Rust exposes them |
| `/ImageMask`, `/SMask`, `/Mask`, `/Matte` | **Build our own**, porting pdf.js's Matte un-premultiply and soft-mask fallbacks | pdf.js is Apache-2.0 |
| The whole job, by hayro-syntax or hayro-interpret | **Study only**; offer upstream a public `Filter::apply` or a 1-bit option | A second parser beside pdf-structure; 8-bit output |
| pdf-rs, lopdf, pdf_oxide, pdf-extract for images | **Study only** | G3 panics; raw bytes only; the DeviceN bug and C dependencies; no image support |
| Oracle | **Use directly** PDFium through pdfium-render 0.9.5, as a dev-dependency; PDFBox's raw rasters as an optional second oracle, never linked | PDFium's bitmaps are 8-bit, so compare after its conversion |

### pdf-scan: no Rust writer streams a searchable scan deterministically

No Rust crate writes a searchable scanned PDF the way the plan needs. **pdf-writer 0.15.0** could emit every object needed (CCITT `DecodeParms`, a `CIDToGIDMap` stream, invisible text), but it is a second serializer, finishes into one `Vec<u8>`, and writes f32 numbers ([lib.rs](https://docs.rs/crate/pdf-writer/0.15.0/source/src/lib.rs)). **krilla 0.8.2** has no render mode 3, no 1-bit or CCITT images, and one CID per glyph, which defeats the glyphless trick ([cid.rs](https://docs.rs/crate/krilla/0.8.2/source/src/text/cid.rs)). **printpdf** names resources from a process-wide counter, so its output depends on what else the process made first ([utils.rs](https://docs.rs/crate/printpdf/0.12.8/source/src/utils.rs)). Of the 2026 searchable-PDF crates, **deepocr-core** maps every CID to `.notdef` ([font.rs](https://docs.rs/crate/deepocr-core/0.1.0/source/src/searchable/font.rs)) and **pdf-ocr** is AGPL. The writer is therefore a build on pdf-structure's `write::Parts`, following Tesseract's layout: pages streamed out as they're made, `/Pages` written last ([pdfrenderer.cpp](https://github.com/tesseract-ocr/tesseract/blob/main/src/api/pdfrenderer.cpp)).

Tesseract's glyphless font (Apache-2.0) remains the design to port: a Type0 font over Identity-H, `/DW 500`, every CID sent to glyph 1 because Acrobat "gets upset" at glyph 0. OCRmyPDF is MPL-2.0, so study only, but its 17.x release notes are the best catalogue of what breaks, and four entries apply directly to sheaf's `Glyphless` ([release notes](https://github.com/ocrmypdf/OCRmyPDF/blob/main/docs/releasenotes/version17.md)). Ghostscript 9.56 to 10.04 discards a whole ToUnicode map when any block has more than 100 entries. Poppler doesn't carry `Tz` across `BT`/`ET`, and sheaf writes one `BT` for the whole layer. Right-to-left words need visual order, because extractors run the bidi algorithm on glyph positions, where Tesseract mirrors the text matrix instead. And glyphless text "lost characters" when copied in Chrome and other PDFium-based viewers, for a cause the notes don't describe.

Three spec-level rules bear on the design too. **sheaf's single `bfrange <0000> <FFFF> <0000>` crosses 255 high-byte boundaries**, which Chromium's PDF writer forbids ([Chromium review](https://codereview.chromium.org/23519006/diff/32001/src/pdf/SkPDFFont.cpp)); 256 per-high-byte ranges in blocks of at most 100 satisfy both rules for about 7 KB more before compression. **Text outside the Basic Multilingual Plane needs a decision**, because each such character becomes two Identity-H CIDs mapped to lone surrogates, and no source documents how viewers treat them. And **any reference to `.notdef` fails veraPDF's PDF/A-2 check 6.2.11.8-1**, whatever the render mode ([veraPDF](https://lists.verapdf.org/hyperkitty/list/users@lists.openpreservation.org/thread/3MMZ3M4BR2SMSIY4WW24SOUTH2IKBUWD/)). For determinism, krilla is a permissive model: its file ID is a stable hash of the bytes written ([chunk_container.rs](https://docs.rs/crate/krilla/0.8.2/source/src/chunk_container.rs)).

Reading back can be exact where other tools are heuristic, because the target shape is narrow. pdf_oxide's permissive classifier supplies usable thresholds: a page is a scan when its image area, summed over every draw, covers at least 0.80 of the page, and an invisible-glyph share of 0.50 or more over a scan means an OCR layer exists ([auto.rs](https://docs.rs/crate/pdf_oxide/0.3.78/source/src/extractors/auto.rs)). OCRmyPDF supplies facts about other tools' output rather than code ([_contentstream.py](https://github.com/ocrmypdf/OCRmyPDF/blob/main/src/ocrmypdf/pdfinfo/_contentstream.py)). Text is an OCR layer when drawn in render mode 3 or 7, or in a font whose `/BaseFont`, with the subset prefix stripped, is `GlyphLessFont` or `Occulta`; images drawn into a 1×1 unit square are skipped when computing DPI. That also answers most of FEAT-4.

Mixed raster content (MRC) stays out of scope. archive-pdf-tools is AGPL, didjvu is GPL, and nothing permissive exists ([archive-pdf-tools](https://github.com/internetarchive/archive-pdf-tools/blob/master/README.rst)).

| Need | Verdict | Reason |
|---|---|---|
| JPEG passthrough with `/Rotate` from EXIF | **Build our own** (move sheaf's code). Treat Orientation 0 as "none" (img2pdf, study only); write `/Decode` inversion for Adobe CMYK and YCCK (krilla, permissive) | |
| CCITT G4 for black and white; Flate for gray and RGB | **Build our own** through pdf-structure and pdf-filters; pick the Flate backend by measurement | miniz_oxide forbids `unsafe`; zlib-rs is faster, with about 456 `unsafe`; D4 already sets zlib-rs for sheaf |
| Glyphless text layer | **Port** Tesseract's design and `pdf.ttf` (Apache-2.0). Move sheaf's `Glyphless` with fixes: per-high-byte bfranges in blocks of 100, a rule for text beyond the BMP, a right-to-left rule, `Tz` after every `BT`, never glyph 0 | Each fix maps to a documented regression or a spec rule |
| Layer written into a pdf-content-streams writer | **Build our own** | The family's writer |
| A page at a time, memory flat | **Build our own** on `write::Parts`, in Tesseract's layout | pdf-writer, krilla, lopdf and printpdf all buffer the whole document |
| The same inputs make the same bytes | **Build our own**: no clocks or random ids; `/ID` from a content hash (krilla's approach); `/Producer` and `/CreationDate` from the caller (FEAT-3) | printpdf's global counter is the pattern to avoid |
| An existing Rust writer end to end | **Study only** (krilla, pdf-writer, deepocr-core, printpdf, genpdf; pdf-ocr is AGPL) | Each misses a requirement |
| Is the page a scan; its image, placement and resolution | **Build our own** on pdf-content-streams; borrow pdf_oxide's signals and thresholds (permissive) and OCRmyPDF's ideas (study only) | The narrow target shape allows exact answers |
| An existing text layer (and FEAT-4) | **Build our own**: render mode 3 or 7, the font names `GlyphLessFont` and `Occulta`, an invisible-glyph share | Facts about other tools' files, not their code |
| MRC | **Study only now** | Only AGPL and GPL implementations exist |
| PDF/A | **Study only for now**; krilla's clause tables when it starts; veraPDF as an external check | OCRmyPDF's route goes through Ghostscript or pikepdf (copyleft) |
| Structure and text checks | **Use directly** qpdf `--check`, pdf-lint, PDFium's text extraction, and pdf-extract (MIT) as a second extractor blind to render modes ([lib.rs](https://docs.rs/crate/pdf-extract/0.12.1/source/src/lib.rs)) | |

## scan-fixtures: SANE's own test backend does more than the plan credits

SANE's `test` backend covers more than proposal 22 gives it credit for: three-pass frames in any channel order, padded rows (`ppl-loss`), 16-bit byte swapping, any status (`NO_DOCS` included) returned from every `sane_read`, `SANE_INFO_INEXACT` and every constraint type, and non-blocking reads with a select fd ([test.c](https://gitlab.com/sane-project/backends/-/blob/master/backend/test.c)). Its feeder yields 10 sheets and then `NO_DOCS`, because `sane_start` refuses every 11th scan, which matches crates.md. It has no duplex. Debian also builds the `pnm` backend ([rules](https://sources.debian.org/src/sane-backends/1.4.0-1/debian/rules/)), which scans a given PNM file and can inject statuses. CI could push `scan-fixtures` pages through real libsane that way and check frame assembly byte for byte.

The fake libsane is still needed, but for less: each backend's feeder and duplex names, a jam at a chosen sheet, Brother's `2500x0`, `lines = -1` from a feeder, duplex sides, descriptors that change after a set, and a backend that crashes or hangs. No fake libsane exists in any language. printscan's SANE-shaped fake gave `NO_DOCS` the code 6, which is `JAMMED` ([printscan #13](https://github.com/b1tninja/printscan/pull/13)), a reason to check hand-written status tables against the header.

TWAIN has a BSD-3 virtual data source that reads its images from disk, with its page count set by a `CAP_XFERCOUNT` environment variable ([README](https://github.com/twain/twain-samples/blob/HEAD/TWAIN-Samples/Twain_DS_sample01/README.txt)). Whether it installs and scans headless on `windows-latest` is S4's spike. Copying the DSM and the source's files into `twain_32` and `twain_64` may avoid the installer. yushulx/virtual-scanner repackages the same source as a portable zip, keeping the BSD notice ([virtual-scanner](https://github.com/yushulx/virtual-scanner/blob/main/README.md)). WIA has no virtual device that installs unattended, so its rule tables get unit tests, as printscan-windows' do.

No Rust library degrades document images. Augraphy (MIT) is the richest catalogue to port, though it is Python, slow and documents no seeding ([Augraphy](https://github.com/sparkfish/augraphy)). Its effects include LightingGradient, ShadowCast, ColorPaper, PageBorder, BleedThrough, DirtyRollers, BookBinding and Folding. ocrodeg has no license file, and DocCreator is LGPL.

The fixtures' own xorshift RNG is the right choice. imageproc's noise seeds `StdRng`, whose output rand says may change across versions "even with a fixed seed" ([rand](https://docs.rs/rand/latest/rand/rngs/struct.StdRng.html)). Today's degradations are gray only, so the Photo profile's colour steps can't yet be tested on them.

G3 and Modified Huffman fixtures for BUG-2 come most cheaply from libtiff run as a separate process, the way image-tiff made its own ([PR #369](https://github.com/image-rs/image-tiff/pull/369)), plus libtiff's sample images `fax2d.tif` and `g3test.tif`. For patch codes, the printable sources are A-61599's camera-ready pages, Kodak's 2012 "Patches for Printing" ([PDF](https://help.ascendsoftware.com/hc/article_attachments/360100776993)) and Alliance Group's sheets. No public set of scanned patch-code pages exists.

| Need | Verdict | Reason |
|---|---|---|
| SANE tests without hardware | **Use directly** SANE's `test` and `pnm` backends from the runner's libsane1, under a private `SANE_CONFIG_DIR` | They cover three-pass, padding, 16-bit order, statuses from `sane_read` and constraints |
| Fake libsane (`cdylib`) | **Build our own**, narrowed to what `test` and `pnm` can't do | None exists anywhere |
| TWAIN in CI | **Use directly** the Working Group's sample source (BSD-3), or yushulx's portable build, if S4's spike works | Headless install is unverified |
| WIA in CI | **Build our own** rule-table tests; scan by hand | No virtual WIA device installs unattended |
| eSCL or WSD mock servers | **Use directly** fakescan (Apache-2.0) or go-mfp (BSD-2), only if a network backend is built | AirSane over `test` is GPL |
| Seeded pages and degradations | **Build our own** (extend `Degradation`) with the fixtures' own RNG; add RGB and Photo-profile effects | No Rust library; imageproc's noise isn't seed-stable |
| Effects catalogue | **Port** selected Augraphy effects (MIT) | The richest permissive catalogue |
| G3 and MH fixtures | **Use directly** libtiff as a separate process, plus its sample images | No mature Rust G3/MH encoder |
| Codes drawn | **Use directly** rxing's encoders; patch sheets from A-61599's geometry and Alliance's layout | |
| A gate on barcode false reads | **Use directly** zxing-cpp's `falsepositives` sets, once the images' licenses are checked | The cheapest regression gate |
| Measuring cleanup | **Use directly** Doxa's BinBench outside the build; DISEC'13's metrics on synthetic skew | The datasets' terms for vendoring are unverified |

## Corrections the notes make to the audit and the plan

### To the 2026-10-23 audit

The audit's direction survives almost intact; what changes is versions, sources and details, several of them load-bearing. "Audit" here means what the notes checked: the build-or-adopt entries, M5's research files and its proposals. Entry 65's "default to zlib-rs" isn't contradicted, but the codec note asks that safety count in that measurement alongside speed.

| Where | The audit said | What's true now | Source |
|---|---|---|---|
| Entry 5, proposal 6 | `sane.h` has 15 functions | 14; its public-domain grant asks that the notice be kept and changes marked | [sane.h](https://gitlab.com/sane-project/backends/-/blob/master/include/sane/sane.h) |
| Entry 5 | About a dozen Rust apps with private SANE FFI | About 20 repositories (27 files) since September 2026; three are permissive and load libsane at run time | [code search](https://github.com/search?q=sane_control_option+language%3Arust&type=code) |
| Entry 5 | libloading, 590M downloads | 0.9.0, 624M | [crates.io](https://crates.io/crates/libloading) |
| Proposal 22 | What the test backend covers | Also three-pass in any order, padded rows, 16-bit swapping, any status from every `sane_read`, `INFO_INEXACT`, non-blocking reads | [test.c](https://gitlab.com/sane-project/backends/-/blob/master/backend/test.c) |
| Entry 9 | mdns-sd 0.21.6, 6.6M downloads | 0.21.7, 7.44M | [crates.io](https://crates.io/crates/mdns-sd) |
| Proposal 7 | RSWebTWAIN is Apache-2.0, pushed 2026-10-16 | MIT OR Apache-2.0; the default branch's last commit was 2026-09-11; v0.1.0 was released 2026-04-28 | [RSWebTWAIN](https://github.com/seancampbell3161/RSWebTWAIN) |
| Proposal 8 | `0x210001` is undocumented | It's `WIA_STATUS_END_OF_MEDIA` in the SDK metadata; only Learn leaves it out | [windows crate](https://static.crates.io/crates/windows/windows-0.62.2.crate) |
| Proposal 8 | `FEEDER` and `DUPLEX` together in `WIA_IPS_DOCUMENT_HANDLING_SELECT` | That's WIA 1.0's root value; on WIA 2.0, write `DUPLEX` or `FRONT_ONLY` to the Feeder item | [Learn](https://learn.microsoft.com/en-us/windows-hardware/drivers/image/wia-ips-document-handling-select) |
| Proposal 8 | WIA error codes as Learn lists them | Learn is wrong for cover open, lamp off, multi-feed and the endorser counter; match the SDK's 0x80210010, 11, 14 and 15 as well | [Learn](https://learn.microsoft.com/en-us/windows/win32/wia/-wia-error-codes) |
| Proposal 8, `scanners.md` | WIA 1.0 scans only to files | `idtGetBandedData` transfers to memory; files-only is true of WinRT | [Learn](https://learn.microsoft.com/en-us/windows/win32/api/wia_xp/nf-wia_xp-iwiadatatransfer-idtgetbandeddata) |
| `scanners.md` | Kodak defaulting to duplex is a driver quirk | It's SANE's `kds_s2000` backend on Linux, fixed in NAPS2 8.6.0 | [CHANGELOG](https://github.com/cyanfish/naps2/blob/master/CHANGELOG.md) |
| The TWAIN note's list (the survey brief assumed it) | pytwain is permissive | It's GPL-2.0, so study only | [COPYING](https://github.com/denisenkom/pytwain/blob/master/COPYING.txt) |
| Entry 6 | `twain.h`'s license is an open question | The header grants nothing; the TWAIN License's disclaimer condition applies; `TWAIN.cs` is an MIT alternative | [TWAIN.cs](https://github.com/twain/twain-cs/blob/HEAD/twaincs/source/TWAIN.cs) |
| Entry 7 | No Rust WIA backend exists | printscan-windows 0.1.0, published 2026-11-08 | [crates.io](https://crates.io/crates/printscan-windows) |
| Entry 16 | G3 and MH are refused | Also: `main` reads only G3 1D, unreleased, and never reads T4Options; the release and `main` both refuse G4 wider than 65,535 px | [stream.rs](https://docs.rs/crate/tiff/0.11.3/source/src/decoder/stream.rs) |
| Entry 38 | hayro-ccitt 0.4.0 is the latest | 0.5.0 (2026-11-02), with a breaking API | [CHANGELOG](https://docs.rs/crate/hayro-ccitt/0.5.0/source/CHANGELOG.md) |
| Entry 38 | fax has 69M downloads; 0.3 fixes wide images | 79.1M; 0.3.0 panics past 32,767 px; the fix is unreleased | [issue #30](https://github.com/pdf-rs/fax/issues/30) |
| Entry 34 | libjpeg-turbo-rs, at an earlier version | 0.9.0 does lossless transforms, and disclaims memory safety | [README](https://docs.rs/crate/libjpeg-turbo-rs/0.9.0/source/README.md) |
| Entry 37 | hayro-jbig2 0.3.1, `Image::new_embedded` | 0.4.0: `File`, `Page` and `Globals`, multi-page files, a strict mode | [CHANGELOG](https://docs.rs/crate/hayro-jbig2/0.4.0/source/CHANGELOG.md) |
| Entry 36 | 16-bit JPX drops to 8 bits | `components()` gives f32 samples at native depth | [lib.rs](https://docs.rs/crate/hayro-jpeg2000/0.5.0/source/src/lib.rs) |
| Entry 14 | Searches found only small apps and one-off crates | `leptonica` 0.5.0 and seiza-imgproc were missed; aktujo-scan has appeared since | [crates.io](https://crates.io/crates/leptonica) |
| Entry 90 | rten 0.27.0, tract-onnx 0.23.9, candle-onnx 0.11.0 | 0.28.0, 0.23.10 and 0.11.1 (which lacks `GridSample`); burn 0.23.0-pre.1 exists | [crates.io](https://crates.io/crates/rten) |
| Entry 13 | oar-ocr 0.10.1, ocrs 0.13.1 | 0.10.2 and 0.14.0 | [crates.io](https://crates.io/crates/oar-ocr) |
| Entry 10 | rxing 0.9.3, maintained by hschimke alone | 0.9.4 (2026-11-08); the `=0.25.8` pin lasts until v0.10, due in December; outside contributors' fixes are merged | [issue #87](https://github.com/rxing-core/rxing/issues/87) |
| Entry 10 and `reading-writing-codes.md` | Two `image` versions that seemed to conflict | They are image 0.25.10 and imageproc 0.27, and both are right | sheaf's `Cargo.lock` |
| Entry 12, `patch.rs` | "At least 0.75 in long", beside the 3.75 in band | 0.75 in belongs to the perpendicular layout; the parallel layout needs 2 in (2.5 in recommended) | [A-61599](https://archive.org/download/manualsonline-id-e24f73ef-f1cf-4f31-bc70-8aae8572f136/e24f73ef-f1cf-4f31-bc70-8aae8572f136.pdf) |
| `separators.md` | TWAIN's patch capability is a `CAP_`; WIA isn't mentioned | It's `ICAP_PATCHCODEDETECTIONENABLED`; WIA 2.0 has a Patch Code Reader item with metadata | [twain.h](https://github.com/twain/twain-dsm/blob/master/TWAIN_DSM/src/twain.h) |
| Entry 10 | No newer pure-Rust readers | anyd, tapirscan and wxing, all from 2026 | [crates.io](https://crates.io/crates/anyd) |
| Entries 33 to 37, 91 | hayro 0.8.0, hayro-jpeg2000 0.4.1, pdfium-render 0.9.4 | 0.9.0, 0.5.0 and 0.9.5; lopdf is at 0.47.0 | [crates.io](https://crates.io/crates/hayro-syntax) |
| Entry 33 | `Filter::apply` is crate-private, so sheaf can't call it | Still true, but `Stream::decoded_image` is public, for hayro's own `Stream` only | [stream.rs](https://docs.rs/crate/hayro-syntax/0.9.0/source/src/object/stream.rs) |
| Entries 33, 91 | Every crate is pure Rust and safe at the crate level; the codecs' `simd` is on by default | hayro-syntax has no `forbid(unsafe_code)` and has a `transmute`; its default build is SIMD-free, behind a new `unsafe` feature | [Cargo.toml](https://docs.rs/crate/hayro-syntax/0.9.0/source/Cargo.toml.orig) |
| Entry 91 | hayro-interpret depends on skrifa | Only through hayro-font 0.5.0 | [Cargo.toml](https://docs.rs/crate/hayro-interpret/0.9.0/source/Cargo.toml.orig) |
| Entry 91 | The README says "1,400+ PDFs" and "experimental" | "1,600+"; it now lists robustness (timeouts, OOM), performance and API surface as open | [README](https://github.com/LaurenzV/hayro/blob/main/README.md) |
| Entry 33 | pdf_oxide has 1.35M downloads | 1.53M; it counts DeviceN as 4 components whatever N is | [images.rs](https://docs.rs/crate/pdf_oxide/0.3.78/source/src/extractors/images.rs) |

### To crates.md and bugs.md

| Where | The plan says | Change | Source |
|---|---|---|---|
| crates.md, `sane-dl` | "the 15 functions" | 14; keep `sane.h`'s notice and mark changes | [sane.h](https://gitlab.com/sane-project/backends/-/blob/master/include/sane/sane.h) |
| crates.md, `sane-dl` | Safe types | Statuses and frame types as integer constants, not Rust enums (12 and 13 are reserved) | same |
| crates.md, `sane-dl` | "a `Send` cancel token" | Synchronized with close, since no other call may run until the cancelled one returns | [api.rst](https://gitlab.com/sane-project/standard/-/blob/master/api.rst) |
| crates.md, `sane-dl` | A device list under a deadline (10.7 s) | Add NAPS2's one-backend `dll.conf`, to open a known device fast | [SaneScanDriver.cs](https://github.com/cyanfish/naps2/blob/master/NAPS2.Sdk/Scan/Internal/Sane/SaneScanDriver.cs) |
| crates.md, `scan-device`'s `sane` backend | The quirk list | Add `ScanMode`, `scan-both-sides`, `duplex` = `front`/`both`, Brother's long source strings, translated values, the ambiguous `INVAL` from `sane_start` | [scanner.vala](https://github.com/GNOME/simple-scan/blob/master/src/scanner.vala) |
| crates.md, `sane-dl` tested by | The fake libsane covers `NO_DOCS` from `sane_read`, 1-bit packing, padded rows and three passes | Those move to the test backend. The fake keeps backend names, a jam at sheet N, `2500x0`, `lines = -1` from a feeder, duplex sides, `RELOAD_OPTIONS`, crash and hang. Add `pnm` | [test.c](https://gitlab.com/sane-project/backends/-/blob/master/backend/test.c) |
| crates.md, `scan-device`'s `wia` backend | `WIA_IPS_DOCUMENT_HANDLING_SELECT` = `FEEDER` and `DUPLEX` | `DUPLEX` or `FRONT_ONLY` on the Feeder item. Add `WIA_CMD_STOP_FEEDER`, progress by bytes, and one device open at a time | [Learn](https://learn.microsoft.com/en-us/windows-hardware/drivers/image/wia-ips-document-handling-select) |
| crates.md, `scan-device`'s `wia` backend | "the undocumented `0x210001`" | Match it by name, `WIA_STATUS_END_OF_MEDIA`; accept both spellings of codes 16 to 21 | [Learn](https://learn.microsoft.com/en-us/windows/win32/wia/-wia-error-codes) |
| crates.md, `twain-dsm` open | `twain.h`'s license, before the bindings are written | Closed: port from `TWAIN.cs` (MIT), checked against `twain.h` 2.5, with the TWAIN Toolkit disclaimer added | [TWAIN.cs](https://github.com/twain/twain-cs/blob/HEAD/twaincs/source/TWAIN.cs) |
| crates.md, `twain-dsm` | "RSWebTWAIN's typestate machine (MIT and Apache-2.0 files)" | The repository is dual MIT OR Apache-2.0; take its design, not its code | [RSWebTWAIN](https://github.com/seancampbell3161/RSWebTWAIN) |
| crates.md, `twain-dsm` | "A hidden window it owns and a message loop" | An STA thread and a real hidden top-level window (not `HWND_MESSAGE`), with a child or owner window for driver dialogs | [Win32MessagePump.cs](https://github.com/cyanfish/naps2/blob/master/NAPS2.Sdk/Platform/Windows/Win32MessagePump.cs) |
| crates.md, `twain-dsm` | "native and memory transfer" | Memory by default, native per driver; tiles at their offsets; containers allocated through `DSM_MemAllocate` | [TwainMemoryBufferReader.cs](https://github.com/cyanfish/naps2/blob/master/NAPS2.Sdk/Scan/Internal/Twain/TwainMemoryBufferReader.cs) |
| crates.md, `twain-dsm` | "the system's `TWAINDSM.dll`" | Loaded through a system-folder search, with an Authenticode check | [TWAIN 2.4](https://github.com/twain/twain-specification/blob/HEAD/versions/2.4/TWAIN-2.4-Specification.pdf) |
| crates.md, `twain-dsm` tested by | The sample source's MSI installers | The spike should also try yushulx's portable zip, and copying the files into place | [virtual-scanner](https://github.com/yushulx/virtual-scanner/blob/main/README.md) |
| crates.md, `scan-worker` | The watchdog | Add a Job object that kills on close, drained pipes and an optional spare worker | [WorkerFactory.cs](https://github.com/cyanfish/naps2/blob/master/NAPS2.Sdk/Remoting/Worker/WorkerFactory.cs) |
| crates.md, `scan-image` B24 | "Move to `fax` 0.3 (`u32` widths)" | 0.3.0 panics past 32,767 px: take the #31 fix or refuse such widths, and read the result back through hayro-ccitt | [issue #30](https://github.com/pdf-rs/fax/issues/30) |
| crates.md, `scan-image` B25 | "Decode them through hayro-ccitt" | Every CCITT compression, 4 included, on 0.5.0's new API (rows of change positions; `BlackIs1` inverted by the caller) | [lib.rs](https://docs.rs/crate/hayro-ccitt/0.5.0/source/src/lib.rs) |
| crates.md, `scan-image` boundary | "codecs only (image, tiff, …)" | `image` with default features off; hayro's MSRV of 1.92 sets the floor | [image Cargo.toml](https://docs.rs/crate/image/0.25.10/source/Cargo.toml) |
| crates.md, `scan-cleanup` open | Speed | The Scanner profile meets slice 6's gate by page parallelism alone; Photo needs a page 1.4 to 2 times faster; profile before optimizing | arithmetic on crates.md's figures |
| crates.md, `patch-codes` | "at least 0.75 in long" | For the parallel layout, 2 in minimum with 2.5 in recommended; 0.75 in is the perpendicular layout's figure | [A-61599](https://archive.org/download/manualsonline-id-e24f73ef-f1cf-4f31-bc70-8aae8572f136/e24f73ef-f1cf-4f31-bc70-8aae8572f136.pdf) |
| crates.md, `patch-codes` boundary | "No barcode library reads patch codes" | Broader still: nothing open reads them, in any language | [NuGet](https://azuresearch-usnc.nuget.org/query?q=patchcode) |
| crates.md, `pdf-images` | `BlackIs1`; JBIG2 with `/JBIG2Globals`; JPX | `BlackIs1` is now the caller's inversion; JBIG2 goes through `File`, `Page` and `Globals`; JPX through `components()` | [hayro-jbig2 CHANGELOG](https://docs.rs/crate/hayro-jbig2/0.4.0/source/CHANGELOG.md) |
| crates.md, `pdf-images` open | hayro-jbig2 is "under development" (medium confidence) | 0.4.0 still says so, but it reads multi-page files, shares globals, has a strict mode and is fuzzed, so the label is mostly cosmetic | same |
| crates.md, `pdf-images` open | "keep one version of each across the two workspaces" | pdf-content-streams pins hayro-ccitt 0.3, two breaking releases behind; hayro's MSRV of 1.92 is above the family's 1.89 | [Cargo.toml](~/repos/sheaf-arlington/crates/pdf-content-streams/Cargo.toml) |
| crates.md, `pdf-scan` | The text layer, Tesseract's approach | Add five fixes: per-high-byte bfranges in blocks of 100, `Tz` after every `BT`, a right-to-left rule, a rule for text beyond the BMP, never glyph 0 | [OCRmyPDF v17](https://github.com/ocrmypdf/OCRmyPDF/blob/main/docs/releasenotes/version17.md) |
| crates.md, `pdf-scan` tested by | PDFium's text extraction | Add right-to-left, CJK, combining marks, text beyond the BMP, vertical lines, and a word crossing a `BT`. Run pdf.js and `pdftotext` as external oracles and veraPDF for `.notdef`; compare bytes across processes | same |
| crates.md, `scan-fixtures` | The bench's degradations | Never use imageproc's noise; add RGB degradations and Photo-profile effects | [rand](https://docs.rs/rand/latest/rand/rngs/struct.StdRng.html) |
| bugs.md, BUG-1 | Fix with `fax` 0.3.0 or a refusal; checked by a 70,000 × 100 round trip | That check panics on 0.3.0 under overflow checks. Take the #31 fix or refuse widths over 32,767, and read back through hayro-ccitt, since tiff's G4 reader refuses widths over 65,535 | [issue #30](https://github.com/pdf-rs/fax/issues/30), [stream.rs](https://docs.rs/crate/tiff/0.11.3/source/src/decoder/stream.rs) |
| bugs.md, BUG-2 | Fixtures from outside (a fax service's files, or a tool run as a process) | libtiff as a separate process, as image-tiff did, plus libtiff's `fax2d.tif` and `g3test.tif`. Don't wait for `tiff`: its G3 1D is unreleased and its MH PR unreviewed | [PR #369](https://github.com/image-rs/image-tiff/pull/369), [PR #414](https://github.com/image-rs/image-tiff/pull/414) |
| bugs.md, BUG-3 | Two-sample gray is refused by both paths | `tiff` decodes it as `Multiband`, and `scan-image` can map it; `image` 0.25.9 added initial 16-bit CMYK TIFF support | [image.rs](https://docs.rs/crate/tiff/0.11.3/source/src/decoder/image.rs), [CHANGES.md](https://github.com/image-rs/image/blob/main/CHANGES.md) |
| bugs.md, BUG-4 | Orientation ignored by `tiff_pages` | Confirmed: `tiff` exposes the tag and never applies it | [image tiff.rs](https://docs.rs/crate/image/0.25.10/source/src/codecs/tiff.rs) |
| bugs.md, BUG-5 | "Upstream's bug too" | Confirmed in 0.27.0's source; no issue has been filed, and PR #740 was closed unmerged. `adaptive_threshold`'s u32 integral image has the same kind of overflow and belongs in any report (with Greg's OK) | [contrast.rs](https://docs.rs/crate/imageproc/0.27.0/source/src/contrast.rs), [PR #740](https://github.com/image-rs/imageproc/pull/740) |

## Thirty-two holes in the ecosystem, of which the planned crates fill eleven

Across the stack, 32 needs have no adequate permissive, pure-Rust and mature implementation. Eleven are exactly what the published crates would supply, one of them (the ImageCaptureCore layer) only once macOS is scheduled. Two more are filled only inside the unpublished `scan-fixtures`, so for everyone else they stay holes. The rest are upstream's to close, or nobody's, and the plan needs none of them closed to proceed.

| Layer | Hole | Who fills it |
|---|---|---|
| Devices | A sound, maintained, published libsane binding that loads at run time | **`sane-dl`** |
| Devices | A safe, maintained TWAIN binding | **`twain-dsm`** |
| Devices | TWAIN memory transfer with tiles and odd `BitsPerSample`, in permissive code | **`twain-dsm`** |
| Devices | An out-of-process scan broker as a library (32-bit TWAIN from 64-bit) | **`scan-worker`** |
| Devices | A safe ImageCaptureCore scanning layer over objc2's raw bindings | **`scan-device`'s `ica` backend**, later |
| Devices | A permissive, machine-readable list of SANE and TWAIN quirks | Partly: our own cited fact tables (every existing list lives in copyleft frontends) |
| Devices | A fake libsane, in any language | `scan-fixtures` (unpublished) |
| Devices | A published pure-Rust saned client | Nobody yet (printscan-sane is an open PR) |
| Devices | A mature Rust eSCL client (whose spec can't be redistributed) | Nobody; not planned |
| Devices | A pure-Rust document-scanner USB driver; a permissive fi-series driver anywhere; a public fi-series SCSI reference | Nobody; not planned |
| Devices | A virtual WIA device that installs unattended in CI | Nobody |
| Devices | A SANE channel for patch-code or barcode results found by the scanner | Nobody (SANE backends define the fields and expose none) |
| Pages | A patch-code reader or writer in any language, under any open license | **`patch-codes`** |
| Pages | A public set of scanned patch-code pages | `patch-codes`' print-and-scan fixtures, if published |
| Pages | A Rust Code 39 encoder with a settable wide-to-narrow ratio | sheaf's own drawing |
| Pages | A pure-Rust barcode reader with negative tests and fuzzing, at zxing-cpp's read rate | rxing, upstream |
| Pages | Maintained, permissive, pure-Rust document cleanup (Sauvola, deskew with a confidence, light flattening, blank detection, page finding) | **`scan-cleanup`** |
| Pages | Up/down orientation and dewarping without a model, in Rust | `scan-cleanup`, by porting Leptonica; later |
| Pages | Punch-hole removal and colour dropout outside commercial SDKs and patents | `scan-cleanup`, if wanted |
| Pages | A pure-Rust DIBCO scorer; public benchmarks for blank pages, page finding in photos, and orientation | Nobody |
| Pages | A seeded document-degradation library in Rust | `scan-fixtures` (unpublished) |
| Pages | G3 and MH encoding in Rust | Nobody, and not needed (fixtures come from libtiff) |
| Pages | A released `tiff` that reads G3 2D, MH and G4 wider than 65,535 px; a released `fax` without the 32,767 panic | Upstream; `scan-image` works around both |
| Pages | Lossless JPEG transforms in safe Rust; 12-bit and arithmetic-coded JPEG in a safe, stable crate | Nobody; later milestones |
| Pages | A lossless JBIG2 encoder with clean provenance; a mature JPX encoder | Nobody; JBIG2 encoding deferred until measured |
| Pages | Independent conformance results or OSS-Fuzz coverage for hayro-ccitt, hayro-jbig2 and hayro-jpeg2000 | Partly: our fuzzing through our adapters, and corpus runs |
| PDF | A PDF image decoder giving native samples over the caller's object model, capped before decoding | **`pdf-images`** |
| PDF | A streaming, deterministic writer of searchable scanned PDFs (CCITT, glyphless layer) | **`pdf-scan`** |
| PDF | A permissive classifier of scans and OCR layers, outside a kitchen-sink crate | **`pdf-scan`** |
| PDF | A tested answer for text beyond the BMP and right-to-left text in glyphless layers | **`pdf-scan`'s** test matrix |
| PDF | A permissive MRC implementation | Nobody |
| PDF | PDF/A output for scans without copyleft tools | Nobody yet |

## What is still unverified

Almost nothing here blocks S1. The items fall into three groups: CI spikes for S3 and S4; measurements (cleanup's speed, JBIG2 against G4, the corpus's rare JPEGs); and viewer behaviour around the text layer, which only experiments settle.

| Crate | Unverified | What settles it |
|---|---|---|
| `sane-dl` | The soundness of airbridge and janus-scan: only their headers, cancel paths and tests were read | Read them in full before borrowing a pattern |
| `sane-dl` | Whether the crate builds and loads against Homebrew's sane-backends on macOS | A CI try (already open in crates.md) |
| `sane-dl` | Whether the test backend's hand-scanner mode combines with the ADF to give `lines = -1` from a feeder | Run it; otherwise the fake covers it |
| `sane-dl` | Quirks beyond the frontends' (the sane-devel archives); Rust SANE FFI hosted on Codeberg or GitLab | A later survey |
| `scan-device` | printscan's quality on real devices, and its author's plans (one account, AI-assisted PRs, any TWAIN or ICA backends) | Re-check before S3 and S4 |
| `scan-device` | printscan-windows' WSD measurements, made by its author on unnamed devices | By hand, on the office's network MFPs |
| `scan-device` | Whether every eSCL copier appears under WIA on Windows 11 | By hand |
| `scan-device` | Whether network-capable fi models (fi-8040, fi-7300NX) or the ScanSnap iX2500 speak eSCL or WSD; the license of PFU's `pfufs` driver | Vendor documentation, only if network scanning is wanted |
| `scan-device` | Whether ImageCaptureCore's delegates need the main run loop in a worker; how scanline is implemented | The `ica` slice |
| `scan-device`'s `wia` | Whether `stisvc` runs on Windows Server runners; whether a WDK WIA sample installs unattended | A spike, only if WIA in CI is ever wanted |
| `scan-worker` | Whether an unprivileged `saned -l -o` works per scan on Debian and Ubuntu, and how a client learns the port `-p 0` picked | A spike, only if the saned-as-worker idea is pursued |
| `twain-dsm` | Whether the TWAIN License covers a Rust transcription of `twain.h`; the TWAIN 2.5 specification's notices | Moot via `TWAIN.cs`; counsel if `twain.h` is ever used directly |
| `twain-dsm` | Whether the sample source installs silently, whether hosted runners allow writing to `C:\Windows\twain_*`, and whether it scans with a hidden window | S4's spike |
| `twain-dsm` | Whether the fi-6130's FUJITSU TWAIN32 driver needs a visible parent window or a modal enable; which drivers fail with a message-only parent | The hardware |
| `twain-dsm` | NAPS2's "a jam that persists after clearing", which couldn't be found in its tracker | Keep it off the quirk list until it's seen |
| `scan-image` | Whether `fax` 0.3.1 (with #31) is coming; when `tiff` 0.12 ships and what it holds; whether `main`'s G3 path silently mis-decodes 2D data | Watch upstream; pin a revision meanwhile |
| `scan-image` | How many 12-bit or arithmetic-coded JPEGs the corpus holds | Count them in the 464 DCT scans |
| `scan-image` | The speed of an all-safe build, with SIMD features off | Measure on 300 dpi Letter pages |
| `scan-image` | zune-jpeg's fuzz coverage, and whether it inverts Adobe APP14 CMYK; `png`'s PngSuite results | Read the repos; test against PDFium; run PngSuite |
| `scan-cleanup` | Where Scanner's 0.8 s and Photo's 3.2 s actually go | A profile, before any speed slice |
| `scan-cleanup` | Leptonica's skew speed on modern CPUs (its published figures are from a 1 GHz P3) | Measure the port |
| `scan-cleanup` | Whether PaddleX's UVDoc was retrained or converted; the license of the orientation weights; DocAligner's license; DocRes's weights | Read the model cards and license files |
| `scan-cleanup` | UVDoc on rten or tract: operator coverage, output equality, speed | Entry 90's spike |
| `scan-cleanup` | Terms for vendoring DIBCO, DISEC'13 and DocUNet data | Ask, or don't vendor |
| `scan-cleanup` | Patent status for punch-hole removal (US8494304), colour dropout (US7853074, US7961941) and judging blanks across duplex sides (US8724182) | Check before building any of them |
| `patch-codes` | Whether the fi-6130, fi-7160 and fi-8170 report `TWEI_PATCHCODE` through PaperStream IP; which Windows version added WIA's Patch Code Reader | Hardware and documentation |
| `patch-codes` | Kodak Alaris's A-61801 (2013), and whether anything supersedes A-61599 (Kodak's server refused the fetches) | Another mirror |
| `patch-codes` | `anyd` in the bench; a rerun with rxing 0.9.4 (the bench's figures are 0.9.3's) | The bench |
| `patch-codes` | The licenses of the images in zxing-cpp's samples and in BarBeR | Check before vendoring |
| `pdf-images` | PDFium's current 8-bit conversion (`fpdf_editimg.cpp` wasn't re-read) | Read it when the oracle is written |
| `pdf-images` | Whether hayro-jpeg2000's `components()` come before or after its colour conversion and upsampling | Read or test |
| `pdf-images` | The hayro codecs' pass rates on conformance sets (T.88 Annex K, openjpeg-data) | Run them |
| `pdf-images` | Lossless generic JBIG2's size against G4 on scans; JBIG2Decode's standing under PDF/A (from memory) | Measure on the 150 CCITT and 127 JBIG2 scans; check the standard |
| `pdf-scan` | The cause of OCRmyPDF 17.13's PDFium regression, and whether it affects fonts that map to glyph 1 | The text-layer test matrix in PDFium, with CJK and combining marks |
| `pdf-scan` | How PDFium, pdf.js and Poppler join text beyond the BMP written as two Identity-H CIDs | The matrix |
| `pdf-scan` | Whether sheaf's separately scaled space avoids OCRmyPDF's 17.4 text-selection bug | Test in PDFium, pdf.js and Preview |
| `pdf-scan` | How pdf.js treats render mode 3 in text extraction; veraPDF's license | Read the source and the license |
| `pdf-scan` | The accuracy of either classifier (pdf_oxide's, OCRmyPDF's) | Run the reader over the 709 scans, against PDFium |
| `pdf-scan` | Leptonica's PDF writer (`l_generateCIDataForPdf`, BSD-2) as a permissive reference; oxidize-pdf's PDF/A claims | A later look |

## Conclusion

The audit asked whether to build or adopt, and answered "build" for most of the stack. The survey sharpens that into: build the crates, adopt what sits beneath them, and commit to each dependency deliberately. In practice that means pinning the hayro codecs behind one in-house CCITT adapter shared by `scan-image` and `pdf-images`. It means fuzzing them through that adapter, since none of them is on OSS-Fuzz. And it means moving pdf-content-streams off hayro-ccitt 0.3 before the two workspaces drift further apart. The 2026 wave of young, often AI-written crates calls for the same discipline. printscan, `leptonica`, anyd and the rest are worth reading and porting from with attribution, but each needs a fresh check at the slice that would use it, never a dependency entry. Upstream contributions are worth offering in parallel, with Greg's OK: imageproc's Otsu, `tiff`'s fax gaps, rxing's negative tests, a public filter API in hayro-syntax. None of them should block a slice.

The case for building these as libraries first is stronger than when the audit was written. `patch-codes` would be the first open patch-code reader anywhere. `sane-dl`, `twain-dsm` and `pdf-images` would each be the first sound crate of their kind in Rust. And `scan-cleanup` would be the first maintained, permissive, pure-Rust document-cleanup crate. The remaining risk sits less in missing code than in viewer behaviour: how PDFium, pdf.js and Poppler handle the glyphless layer's right-to-left text, text beyond the BMP, and CJK. A test matrix built early in S2 will settle that; more reading won't.
