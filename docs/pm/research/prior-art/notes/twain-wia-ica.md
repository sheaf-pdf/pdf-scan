# Prior art for scanner access on Windows and macOS: TWAIN (with a 32-bit worker), WIA 2.0, ImageCaptureCore, cross-platform SDKs

As of 2026-11-08. This checks the 2026-10-23 audit (build-or-adopt entries 06, 07 and 08, M5 research `scanners.md`, proposals p04, p07, p08 and p22) and goes further. Each correction to that audit is marked **Correction**. Crate and repository facts come from the crates.io API, NuGet, PyPI and GitHub's Atom feeds, all fetched on 2026-11-08. Code facts come from reading the downloaded sources, none of which were built or run. The GitHub REST API hit its unauthenticated rate limit partway through, so after that, star and issue counts come only from pages fetched earlier.

## Q1. Rust TWAIN crates: what exists, and how good is it?

### Takeaway
No Rust TWAIN crate is usable. The crate named `twain` is a 2021 prototype with undefined behaviour. It hard-codes a relative DLL path, and it bundles `TWAINDSM.dll` and Qt's `qwindows.dll` inside an MIT crate. No `twain-sys`, `twain-rs` crate or other TWAIN binding exists on crates.io. RSWebTWAIN (MIT OR Apache-2.0, alpha, a Tauri app, not a crate) is the only working Rust code with the 64-bit app plus 32-bit sidecar shape. Its typestate idea is worth porting; its bindings and capability code aren't.

### Cited Findings
- **`twain` 0.1.0** is the only release (2021-07-24): MIT, 2,001 downloads in total, 15 recent, 0 reverse dependencies, a 791,310-byte crate, repository `oussama/twain` with three commits, all on 2021-07-24 ("init", "add description", "open for cancelled") — [crates.io](https://crates.io/crates/twain), [commit feed](https://github.com/oussama/twain/commits.atom)
- The crate ships `lib/windows/TWAINDSM.dll` (202,648 bytes) and `lib/windows/qwindows.dll` (1,357,432 bytes, "Qt windows plugin"). It `include_bytes!`s `qwindows.dll` into every binary, and it ships no LGPL text for either DLL — [crate source](https://static.crates.io/crates/twain/twain-0.1.0.crate) (`README.md`, `src/lib.rs` lines 888-901)
- In `twain` 0.1.0, `Client::new()` loads `"./lib/windows/TWAINDSM.dll"` (relative to the working directory). `image_native_transfer` passes `&handle`, where `let handle = 0` is immutable, as the out-pointer the source writes through (undefined behaviour). It never calls `GlobalLock`, `panic!`s on any bit depth but 1, 8 or 24, writes `./img1.bmp` to the working directory, and is full of debug `println!`s. `OPENDSM` gets a null parent window. There's no memory transfer and no message loop. `src/lib.rs` has 18 `unsafe` uses — [crate source](https://static.crates.io/crates/twain/twain-0.1.0.crate) (`src/lib.rs` lines 888-1160)
- The `twain` crate ships `src/twain.h` verbatim (TWAIN 2.4b, March 2017, `TWON_PROTOCOLMINOR 4`) with no license file covering it — [crate source](https://static.crates.io/crates/twain/twain-0.1.0.crate)
- crates.io searches for "twain", "twain scanner", "wia", "image acquisition", "document scanner" and "tauri scanner" find no other TWAIN, WIA or ICA binding (2026-11-08). The prior audit's "no `twain-sys` exists" still holds — [crates.io search API](https://crates.io/api/v1/crates?q=twain)
- **`jtubbenhauer/twain-rs`**: all its commits are from 2024-10-10 ("dsm_entry boilerplate", "add license"), and it has no releases. It's abandoned, as the prior audit said — [commit feed](https://github.com/jtubbenhauer/twain-rs/commits.atom)
- **RSWebTWAIN** (`seancampbell3161/RSWebTWAIN`): a headless Windows tray app bridging browsers to TWAIN, "alpha". Its own README says "Core scanning, WebSocket protocol, and 32-bit driver sidecar work end-to-end on Windows." The 64-bit app talks to `TWAINDSM.dll` directly; for 32-bit-only drivers it starts a 32-bit `scanner-sidecar` and speaks JSON lines over stdin and stdout. It has 38 commits, 0 stars and 0 forks — [GitHub](https://github.com/seancampbell3161/RSWebTWAIN)
- RSWebTWAIN's license is `MIT OR Apache-2.0` (workspace `Cargo.toml`, with `LICENSE-MIT` and `LICENSE-APACHE`). It has one GitHub release, v0.1.0 (2026-04-28); the default branch's last commit was 2026-09-11 ("fix: correct six TWAIN constants and negotiate decode assumptions"). It's not on crates.io — [releases feed](https://github.com/seancampbell3161/RSWebTWAIN/releases.atom), [commit feed](https://github.com/seancampbell3161/RSWebTWAIN/commits.atom), [source tarball](https://codeload.github.com/seancampbell3161/RSWebTWAIN/tar.gz/refs/heads/main)
  - **Correction:** p07 calls it "Apache-2.0"; it's dual MIT OR Apache-2.0. The audit's "pushed 2026-10-16" isn't the default branch, whose last commit is 2026-09-11. And it does have a GitHub release (v0.1.0).
- RSWebTWAIN's quality, from reading its code:
  - **Bindings.** Its FFI says it's a "`#[repr(C)]` translation of the structures defined in twain.h (v2.3)", and it's duplicated in two files (`src-tauri/src/scanner/twain_ffi.rs`, 710 lines, and `scanner-sidecar/src/twain_ffi.rs`, 491 lines). Its own test `capability_parity.rs` says the two "hand-maintained copies of the same TWAIN setup … have silently diverged before" (`ICAP_COMPRESSION`, `ICAP_PIXELFLAVOR`).
  - **Capability containers.** `set_capability_*` passes `Box::into_raw(Box::new(one_value))` as `hContainer`, under the comment "In a real implementation, this would be a GlobalAlloc'd handle", where TWAIN expects a DSM- or `GlobalAlloc`-allocated handle.
  - **Memory transfer.** Uncompressed only (`UnsupportedCompression` error). It uses `TWMF_APPOWNS | TWMF_POINTER`, and nothing reads tile `XOffset` or `YOffset`.
  - **Window.** The window it uses is `HWND_MESSAGE` (message-only), pumped with `PeekMessageW` and `MSG_PROCESSEVENT`.
  - **Unsafe.** `unsafe` appears 36 times in `twain.rs` and 39 in the sidecar's `main.rs`.
  - **Typestate.** Seven state structs (`PreSession`, `DsmLoaded`, `DsmOpened`, `SourceOpened`, `SourceEnabled`, `TransferReady`, `Transferring`) consume `self`, and their `Drop` impls warn and step down.
  - **Tests.** Integration tests run a fake sidecar binary (`tests/bin/fake_sidecar.rs`), not a real DSM.
  - Source: [source tarball](https://codeload.github.com/seancampbell3161/RSWebTWAIN/tar.gz/refs/heads/main) (files named above)
- **node-twain** (npm, a Node addon over the TWAIN DSM, MIT, "under develop"): its README example only does file transfer (`TWSX_FILE`) — [README via jsDelivr](https://cdn.jsdelivr.net/npm/node-twain@0.0.16/README.MD)

### Inferences
- Nothing can be adopted. The planned `twain-dsm` is a build, as the audit said, but its sources should change (see Q2 and Q4): definitions from the TWAIN Working Group's MIT-licensed `TWAIN.cs`, cross-checked against `twain.h` 2.5, rather than RSWebTWAIN's hand transcription of 2.3, which needed a six-constant fix in September 2026.
- RSWebTWAIN's typestate shape is permissive and worth porting as a design (state structs that consume `self` and step down on `Drop`). Its capability-container handling and its two diverging copies of the bindings are things to avoid: one bindings crate used by both the 64-bit and 32-bit workers removes that class of bug.
- Two choices from RSWebTWAIN to reconsider:
  - It parents the source to a message-only window, where NAPS2 uses a real hidden top-level window (see Q5).
  - It loads `TWAINDSM.dll` by bare name. The TWAIN 2.4 specification says the official `TWAINDSM.dll` is signed and that the signature can be verified (Q5).

### Gaps
- I didn't search GitHub's code search (it needs authentication) for app-internal Rust TWAIN code beyond these, so smaller private-feeling repos may exist.
- RSWebTWAIN's issue response can't be judged: it shows no issues, PRs or contributors.

## Q2. The license of `twain.h` and of the TWAIN specification

### Takeaway
`twain.h` itself carries copyright notices and "All rights reserved" and **no license grant**, in every copy checked: the twain-dsm repository (2.5, January 2021), twain-samples (identical) and the `twain` crate (2.4b). The only grant is the **TWAIN License** on twain.org. It covers "the software and documentation of the TWAIN toolkit": royalty-free, worldwide, non-exclusive, "reproduce and distribute". Its condition is that software "based in whole or in part on the TWAIN Toolkit" carry a fixed disclaimer in its header file and prominently in its documentation, and that sublicensees be made to do the same. The DSM's sources are LGPL-2.1-or-later; `twain.h` is not under the LGPL. The specification PDF (2.4) has no copyright or license notice at all. The TWAIN Working Group's own C# translation of `twain.h`, `TWAIN.cs`, is plain MIT (Kodak Alaris). That's the cleanest permissive source for Rust bindings.

### Cited Findings
- `twain-dsm/TWAIN_DSM/src/twain.h` (82,452 bytes, "version 2.5 Jan 2021", `TWON_PROTOCOLMINOR 5`) opens with five copyright blocks for the TWAIN Working Group's members (1991, 1992, 1997, 1998, 2000, 2007), each ending "All rights reserved." A grep for "licen", "permission", "redistrib", "warrant", "LGPL" or "GNU" finds no license text, only compiler macros — [twain.h in twain-dsm](https://github.com/twain/twain-dsm/blob/HEAD/TWAIN_DSM/src/twain.h)
- The copy in `twain-samples/TWAIN-Samples/pub/external/include/twain.h` is byte-identical to twain-dsm's — [twain.h in twain-samples](https://github.com/twain/twain-samples/blob/HEAD/TWAIN-Samples/pub/external/include/twain.h)
- The DSM's own sources are LGPL: `dsm.cpp` says "This library is free software; you can redistribute it and/or modify it under the terms of the GNU Lesser General Public License … version 2.1 of the License, or (at your option) any later version." `TWAIN_DSM/license.txt` is the LGPL-2.1 text. GitHub reports no repo-level license (`license: None`) for twain-dsm, twain-specification, twain-samples or twain-cs — [dsm.cpp](https://github.com/twain/twain-dsm/blob/HEAD/TWAIN_DSM/src/dsm.cpp), [license.txt](https://github.com/twain/twain-dsm/blob/HEAD/TWAIN_DSM/license.txt), [GitHub API for twain-dsm](https://api.github.com/repos/twain/twain-dsm)
- The twain-dsm README: "This DSM is open source software and uses LGPL licenses." — [README](https://github.com/twain/twain-dsm/blob/HEAD/README.md)
- **The TWAIN License**, quoted exactly from twain.org (Wayback snapshot of 2024-02-14; the same text was at `twain.org/license.shtm` in a 2008 snapshot):

  > "The TWAIN Working Group grants customer ("Customer") the worldwide, royalty-free, non-exclusive license to reproduce and distribute the software and documentation of the TWAIN toolkit ("TWAIN Toolkit"). The TWAIN Toolkit was designed to be used by third parties to assist them in becoming compliant with the TWAIN standard, but it has not been developed to the standards of a commercial product. Consequently, the TWAIN toolkit is provided AS IS without any warranty. […] As a condition of this license, Customer agrees to include in software programs based in whole or in part on the TWAIN Toolkit the following providions in (i) the header or similar file in such software and (ii) prominently in its documentation and to require its sublicensees to include these provisions in similar locations: The TWAIN Toolkit is distributed as is. The developer and distributors of the TWAIN Toolkit expressly disclaim all implied, express or statutory warranties including, without limitation, the implied warranties of merchantability, noninfringement of third party rights and fitness for a particular purpose. Neither the developers nor the distributors will be liable for damages, whether direct, indirect, special, incidental, or consequential, as a result of the reproduction, modification, distribution or other use of the TWAIN Toolkit."

  Sources: [twain.org/twain-license (live page renders by script)](https://twain.org/twain-license/), [Wayback 2024-02-14](https://web.archive.org/web/20240214221503/https://twain.org/twain-license/), [Wayback 2008-12-20, license.shtm](https://web.archive.org/web/20081220185118/http://www.twain.org/license.shtm)
- Debian's copyright file for KDE's libksane lists `ksanetwain/twain.h` with "License: TWAIN", whose first line reads "The file twain.h may be copied under the TWAIN License as specified at http://www.twain.org/license.shtm", followed by the text above. That first line is KDE's statement (its `COPYING.twain`), not a TWAIN Working Group notice inside the header — [Debian copyright](https://metadata.ftp-master.debian.org/changelogs/main/libk/libkf5sane/oldoldstable_copyright), [KDE COPYING.twain](https://lxr.kde.org/source/graphics/libksane/COPYING.twain)
- The TWAIN 2.4 specification PDF (ratified 2015-12-11) contains no "copyright", "license", "royalty" or "all rights reserved" text (`pdftotext` and a grep of the whole document). The twain-specification repository's README still names 2.4 as current (its last push was 2020-01-10), though `twain.h` 2.5 exists in twain-dsm — [TWAIN-2.4-Specification.pdf](https://github.com/twain/twain-specification/blob/HEAD/versions/2.4/TWAIN-2.4-Specification.pdf), [README](https://github.com/twain/twain-specification/blob/HEAD/README.md)
- **`TWAIN.cs`** (in the TWAIN Working Group's `twain-cs` repository, 15,564 lines, "2.5.0.0 Updated to latest spec", 17-May-2021) carries "Copyright (C) 2013-2021 Kodak Alaris Inc." and the full MIT permission notice. It defines TWAIN's constants, enums and `Pack = 2` structures for Windows, Linux and macOS layouts — [TWAIN.cs](https://github.com/twain/twain-cs/blob/HEAD/twaincs/source/TWAIN.cs)
- How other permissive bindings treat it:
  - NTwain is MIT (Copyright 2012 Yin-Chun Wang) — [NTwain LICENSE](https://github.com/soukoku/ntwain/blob/v3/LICENSE.txt)
  - TwainDotNet is MIT (Copyright 2009 Luke Quinane) — [TwainDotNet License.txt](https://github.com/tmyroadctfig/twaindotnet/blob/master/License.txt)
  - RSWebTWAIN's Rust FFI is MIT OR Apache-2.0, "translations of the structures defined in twain.h (v2.3)", with no TWAIN disclaimer in it — [RSWebTWAIN twain_ffi.rs](https://github.com/seancampbell3161/RSWebTWAIN/blob/main/src-tauri/src/scanner/twain_ffi.rs)
- The Debian copyright file for libksane shows KDE shipping `twain.h` under the TWAIN License inside an LGPL project — [Debian copyright](https://metadata.ftp-master.debian.org/changelogs/main/libk/libkf5sane/oldoldstable_copyright)

### Inferences
- The License's verb list is "reproduce and distribute" (it says "modification" only in the disclaimer), and it never names `twain.h` itself. So whether a Rust transcription is "based in part on the TWAIN Toolkit", and whether the License lets it be relicensed, is unclear; that's for counsel, not for a guess.
- Two routes keep `twain-dsm` publishable as MIT OR Apache-2.0:
  1. **Port the definitions from `TWAIN.cs`** (MIT; keep Kodak Alaris's copyright and permission notice in the bindings file and in `NOTICE`), cross-checking values against `twain.h` 2.5 and the specification.
  2. **Write them from `twain.h`, adding the TWAIN Toolkit disclaimer** to the bindings file's header and to the crate's documentation, as the License requires.

  Either way, adding the disclaimer costs nothing and covers both readings. It's a warranty disclaimer, not a restriction, so it doesn't conflict with MIT or Apache-2.0.
- Many jurisdictions treat bare interface constants and structure layouts needed for interoperability as unprotectable, but that's a legal question to confirm, not a basis to rely on.
- Shipping `TWAINDSM.dll` unmodified and loading it at run time (the plan) is unaffected: the DSM is LGPL-2.1-or-later, separately shipped. `twain.h` is the only open item, and either route above closes it.

### Gaps
- The live twain.org pages render by JavaScript and couldn't be read, so the License text above is from the 2024-02-14 snapshot. I found no newer statement that it changed, or that it covers `twain.h` by name.
- I found no TWAIN Working Group statement on licensing translations of `twain.h` (other than its own MIT `TWAIN.cs`).
- The TWAIN 2.5 specification PDF wasn't fetched; only 2.4 was checked for notices.

## Q3. TWAIN Direct: status in 2026, license, shipping devices, usable route?

### Takeaway
TWAIN Direct looks dormant. Its main repository's last commit was 2022-11-09, with no releases. Its iOS and Android repositories stopped in 2023 and 2019. Its site `twaindirect.org` now redirects to twain.org. I found no 2025-2026 evidence of scanners shipping it natively. The code is MIT, but it isn't a route for the office's fi-6130 (USB, a 32-bit TWAIN driver), and it isn't worth building for now.

### Cited Findings
- `twain/twain-direct` is MIT (per the GitHub API), with 32 stars, no releases, and a last commit on 2022-11-09 ("fixed missing quotes on profile property"). The one before that was 2020-05-01 ("bumped to 1.24; synced to TWAIN CS 2.4.9") — [GitHub API](https://api.github.com/repos/twain/twain-direct), [commit feed](https://github.com/twain/twain-direct/commits.atom)
- The other TWAIN Direct repositories in the twain organization: twain-direct-ios (MIT, last push 2023-06-18), twain-direct-android (MIT, 2019-05-06), twain-cloud (MIT, 2018-07-25), twain-cloud-express (MIT, 2019-05-09) — [GitHub API, twain org repos](https://api.github.com/orgs/twain/repos?per_page=100&sort=pushed)
- `https://www.twaindirect.org/faq/` returns a 302 to `https://twain.org/` (2026-11-08) — fetched directly
- twain.org's TWAIN Direct page (Wayback, 2021-04-17) describes it as driverless network scanning with a JSON command language, and says that "TWAIN's initial cloud initiative is based on the collaborative efforts of the TWAIN Working Group with the Google Cloud Print team" — [Wayback 2021-04-17](https://web.archive.org/web/20210417140252/https://twain.org/twain-direct/)
- A TWAIN forum thread (undated in the search snippet) said there were no native TWAIN Direct scanners yet; Visioneer had support through the TWAIN Bridge, and a later reply named the Xerox D70n — [twain.org forum, via search](https://twain.org/forums/topic/which-scanners-support-twain-direct/)

### Inferences
- TWAIN Direct gives the office nothing: their scanner is USB with a 32-bit TWAIN driver. Network copiers are better reached through WIA's WSD and eSCL class drivers (Q6), or eSCL directly (another researcher's area).
- **Verdict: study only** (or ignore). Don't build a TWAIN Direct backend.

### Gaps
- The forum thread itself didn't render (script-loaded), so its dates and posters are unverified.
- I found no current list of certified TWAIN Direct devices.

## Q4. Permissive non-Rust TWAIN code to port, copyleft code to study, and the quirk lists

### Takeaway
The best permissive sources:

| Source | License | Latest | Use it for |
| --- | --- | --- | --- |
| The TWAIN Working Group's `TWAIN.cs` | MIT, Kodak Alaris | 2.5, 2021; repo last commit 2023-05-18 | Definitions, and DSM memory functions with a `GlobalLock` fallback |
| NTwain | MIT | 3.7.5, 2024-07-12; 4.0.0-beta.4, 2026-02-05 | Capability containers, internal message loop |
| NAPS2's own MIT forks: `NAPS2.NTwain` | MIT | 1.0.1, 2025-04-16 | The TWAIN layer NAPS2 actually ships |
| `NAPS2.Wia` | MIT | 2.0.3, 2023-08-30 | The WIA layer (Q6) |
| The TWAIN sample data source and application | BSD-3-Clause | 2021-11-09 | Test fixture (Q9) |
| TwainDotNet | MIT | Dormant since 2020 | Little |

Copyleft, study only:
- NAPS2 (GPL-2.0-or-later) and NAPS2.Sdk (LGPL-2.1-or-later), whose TWAIN runner, worker and quirk logic live there
- pytwain: **GPL-2.0, not permissive** (**correction** to the brief's assumption)
- Saraff.Twain.NET (LGPL-3.0-or-later)

No published quirk list exists. NAPS2's changelog and source are the nearest thing, and they're copyleft: cite what they learned, never copy.

### Cited Findings
- **TWAIN.cs**'s stated goals: "Run thread safe. Force as many TWAIN calls into a single thread as possible. The main exceptions are MSG_PROCESSEVENT and any of the calls that unwind the state machine (ex: MSG_DISABLEDS)." It supports both TWAIN_32.DLL and TWAINDSM.DLL, and 32- and 64-bit Windows, Linux and macOS. It locks memory through `DSM_MemLock` when the DSM provides it, else `GlobalLock` (lines 605-613). Its many "some drivers will do that to you" comments are about first-chance exceptions thrown inside drivers — [TWAIN.cs](https://github.com/twain/twain-cs/blob/HEAD/twaincs/source/TWAIN.cs)
- twain-cs releases: "TWAIN CS v2.5" (2021-11-19); last commit 2023-05-18. The README: "Runs on Windows (.NET or Mono)… 32bit and 64bit support… Follows TWAIN 'best practices'" — [releases feed](https://github.com/twain/twain-cs/releases.atom), [commit feed](https://github.com/twain/twain-cs/commits.atom), [README](https://github.com/twain/twain-cs/blob/HEAD/README.md)
- **NTwain**: MIT; NuGet 3.7.5 (2024-07-12) and 4.0.0-beta.4 (2026-02-05). The default branch v3's last commit was 2026-05-08 ("Fix #82 wrong cap size when string is used"). 167 stars, 43 open issues, 2 open PRs.
  - Its README: "Can host an internal message loop so there's no need to hook into application UI thread"; "you will always need to have the twaindsm installed" in 64-bit; 32-bit drivers need an x86 build; "Some scanner drivers use modal dialogs that may hang the app."
  - Sources: [NuGet registration](https://api.nuget.org/v3/registration5-gz-semver2/ntwain/index.json), [commit feed](https://github.com/soukoku/ntwain/commits.atom), [GitHub](https://github.com/soukoku/ntwain), [README](https://github.com/soukoku/ntwain/blob/v3/README.md)
- **NAPS2's TWAIN and WIA layers are MIT forks or libraries**: `NAPS2.NTwain` 1.0.1 (2025-04-16, MIT, "Fork of NTwain for NAPS2.Sdk"; fork repo last commit 2024-03-10) and `NAPS2.Wia` 2.0.3 (2023-08-30, MIT). `NAPS2.Sdk.csproj` references both — [NuGet NAPS2.NTwain](https://api.nuget.org/v3/registration5-gz-semver2/naps2.ntwain/index.json), [NuGet NAPS2.Wia](https://api.nuget.org/v3/registration5-gz-semver2/naps2.wia/index.json), [NAPS2.Sdk.csproj](https://github.com/cyanfish/naps2/blob/master/NAPS2.Sdk/NAPS2.Sdk.csproj), [cyanfish/ntwain commits](https://github.com/cyanfish/ntwain/commits.atom)
- **NAPS2** itself is GPL-2.0-or-later ("Copyright 2009-2026 NAPS2 Contributors"), and `NAPS2.Sdk/LICENSE` is LGPL-2.1-or-later. NAPS2 8.6.0 (2026-10-25) and SDK 1.6.0 (2026-10-25) are current, with commits on 2026-10-26 — [LICENSE](https://github.com/cyanfish/naps2/blob/master/LICENSE), [NAPS2.Sdk/LICENSE](https://github.com/cyanfish/naps2/blob/master/NAPS2.Sdk/LICENSE), [releases feed](https://github.com/cyanfish/naps2/releases.atom), [NuGet NAPS2.Sdk](https://api.nuget.org/v3/registration5-gz-semver2/naps2.sdk/index.json)
- **pytwain** is GPL-2: its `COPYING.txt` reads "This software is distributed under the GNU General Public Licence, Version 2". PyPI 2.3.0 (2024-03-02); last commit 2024-03-09; ctypes-based. **Study only** — [COPYING.txt](https://github.com/denisenkom/pytwain/blob/master/COPYING.txt), [PyPI JSON](https://pypi.org/pypi/pytwain/json), [commit feed](https://github.com/denisenkom/pytwain/commits.atom)
- **TwainDotNet**: MIT (Luke Quinane, 2009). Its last commit was 2020-11-16, and it's "based off the code from Thomas Scheidegger's … article on The Code Project" — [README](https://github.com/tmyroadctfig/twaindotnet/blob/master/README.md), [License.txt](https://github.com/tmyroadctfig/twaindotnet/blob/master/License.txt), [commit feed](https://github.com/tmyroadctfig/twaindotnet/commits.atom)
- **Saraff.Twain** (NuGet) is LGPL-3.0-or-later, 1.0.33.756 (2024-12-09) — [NuGet](https://api.nuget.org/v3/registration5-gz-semver2/saraff.twain/index.json)
- **Dynamic Web TWAIN**'s GitHub repository is an npm wrapper ("Dynamic Web TWAIN SDK v19.5.0 is built in this package") that needs a license key, with no LICENSE file. I found no open parts — [README](https://github.com/Dynamsoft/Dynamic-Web-TWAIN/blob/master/README.md)
- **The TWAIN Working Group's samples**: `Twain_DS_sample01/license.txt` and `Twain_App_sample01/license.txt` are BSD-3-Clause ("Redistribution and use in source and binary forms … Neither the name of the TWAIN Working Group …"). The README says "released as open source software and uses a Modified BSD licenses", last built with Qt 5.9.9. Its one release is dated 2021-11-09 — [license.txt](https://github.com/twain/twain-samples/blob/HEAD/TWAIN-Samples/Twain_DS_sample01/license.txt), [README](https://github.com/twain/twain-samples/blob/HEAD/README.md), [releases feed](https://github.com/twain/twain-samples/releases.atom)
- **Quirks NAPS2 learned**, cited from its source (study only):
  - **Transfer mode.** It defaults to **memory transfer**, but uses native transfer for Kyocera Ecosys sources (detected by manufacturer and product family) — [TwainScanRunner.cs](https://github.com/cyanfish/naps2/blob/master/NAPS2.Sdk/Scan/Internal/Twain/TwainScanRunner.cs)
  - **Enable.** It passes `modal = false` to `Enable`, because "according to the twain spec, on Windows it is recommended to set the modal parameter to false" — same file
  - **Cancel, by state.** In states 6-7 it uses `ForceStepDown`. In state 5 it waits for `TransferReady` and cancels "naturally", because "calling ForceStepDown from state 5 seems to produce an error message from the scanner" — same file
  - **Errors.** `TWCC_OPERATIONERROR` means "the scanner has already shown the user an error message". `TWCC_CHECKDEVICEONLINE` means offline at state ≤ 3 and a communication error at state ≥ 4 — same file
  - **Settings.** Paper source maps to `CAP_FEEDERENABLED` and `CAP_DUPLEXENABLED` (both false for flatbed). "Auto" uses `CAP_AUTOMATICSENSEMEDIUM`, else `CAP_FEEDERLOADED`. `CAP_INDICATORS` is turned off to hide the driver's progress window — same file
  - **Memory buffers.** Tiles are copied at `XOffset` and `YOffset`, and "Technically for RGB we should check for [8, 8, 8, ...] but some scanners only set the first value" of `BitsPerSample` — [TwainMemoryBufferReader.cs](https://github.com/cyanfish/naps2/blob/master/NAPS2.Sdk/Scan/Internal/Twain/TwainMemoryBufferReader.cs)
- **NAPS2's changelog, by version**:
  - 8.6.0: "Sdk: Fixed TWAIN hangs with NAPS2.Sdk.Worker.Win32 by running the worker on an STA thread"; "Linux: Fixed 'Feeder' scanning both sides with some scanners (e.g. Kodak kds_s2000 driver)"; "Wia: Fixed an error with devices that don't report their scan area size"
  - 8.1.3: "Twain: Fixed issues with Kyocera Ecosys scanners"
  - 8.1.0: "Fixed an issue with shared TWAIN scanners"
  - 7.0b4: "Twain: Changed default transfer mode"
  - 7.0b1: "TWAIN should also now support scanning larger images (e.g. 1200dpi) without out-of-memory issues"
  - 5.5.0: "Added 'Alternative Transfer' TWAIN compatibility option"
  - Source: [CHANGELOG.md](https://github.com/cyanfish/naps2/blob/master/CHANGELOG.md)
  - **Correction:** the audit's "Kodak's backend defaulting to duplex" is a SANE backend on Linux (`kds_s2000`, fixed in NAPS2 8.6.0), not a TWAIN quirk.

### Inferences
- **Port (permissive):**
  - From `TWAIN.cs`: the definitions, the single-thread rule, and `DSM_MemAllocate`/`DSM_MemLock` with the `GlobalAlloc`/`GlobalLock` fallback.
  - From NTwain or NAPS2.NTwain: container encoding (`TW_ONEVALUE`, `TW_ENUMERATION`, `TW_RANGE`, `TW_ARRAY`) and string-capability sizing (NTwain fixed a wrong cap size for strings in May 2026, which shows it's an easy bug).
- **Study only:** NAPS2 and NAPS2.Sdk's `TwainScanRunner` (transfer-mode defaults, cancel by state, condition-code mapping, the STA rule), pytwain and Saraff.
- Prefer memory transfer by default (as NAPS2 does), with tile offsets and per-sample bit depths read defensively, and native as a per-driver fallback.

### Gaps
- I couldn't verify the audit's "a jam that persists after clearing" in NAPS2's tracker. GitHub issue search needs authentication, and web search found no such ticket.
- I found no published list of drivers that need a visible parent window, or of `CAP_DUPLEXENABLED` oddities. The nearest evidence is in Q5.

## Q5. Driving 32-bit TWAIN drivers from a 64-bit process, and isolating driver crashes

### Takeaway
Everyone does this the same way: a separate process of the driver's bitness, and nobody thunks in-process.

| Tool | How | Detail |
| --- | --- | --- |
| NAPS2 | x86 worker | gRPC over named pipes, a Windows Job object, spare pre-started workers, an STA thread, a real hidden top-level window |
| RSWebTWAIN | 32-bit sidecar | JSON lines over stdin and stdout, a message-only window |
| VintaSoft | 32-bit service exe | `Use32BitDevices()` |
| LEADTOOLS | Thunk server | Registered COM+ server (`LtThunkServer.exe`) |
| Dynamsoft | 32- and 64-bit service builds | Since 16.1 (2020) lists both drivers at once |

The specification says the official `TWAINDSM.dll` is signed and can be verified, that applications must install the DSM themselves, and that all TWAIN calls should be made from a single thread.

### Cited Findings
- NAPS2's TWAIN driver "Delegates to RemoteTwainController in most cases which runs Twain in a 32-bit worker process as Twain drivers are generally 32-bit only". It refuses 64-bit TWAIN from a 32-bit process, and "In general we always prefer to run TWAIN in a worker" — [TwainScanDriver.cs](https://github.com/cyanfish/naps2/blob/master/NAPS2.Sdk/Scan/Internal/Twain/TwainScanDriver.cs)
- NAPS2's worker transport and lifetime:
  - `GrpcDotNetNamedPipes` (pipe name from the process ID), `NAPS2.Worker.exe` built for `win-x86`.
  - A Windows `Job` object so workers die with the parent.
  - "We start a 'spare' worker so that when we need one, it's immediately ready". On Windows that means two spares, 32- and 64-bit.
  - A TODO that stdout isn't drained and could stall the worker.
  - Source: [WorkerFactory.cs](https://github.com/cyanfish/naps2/blob/master/NAPS2.Sdk/Remoting/Worker/WorkerFactory.cs)
- NAPS2's remote controller waits for the worker to close "so we guarantee the parent process doesn't die before the TWAIN process has a chance to clean up". After a remote scan the parent re-enables its own window, in case the worker died while that window was disabled as a modal parent — [RemoteTwainController.cs](https://github.com/cyanfish/naps2/blob/master/NAPS2.Sdk/Scan/Internal/Twain/RemoteTwainController.cs), [TwainScanDriver.cs](https://github.com/cyanfish/naps2/blob/master/NAPS2.Sdk/Scan/Internal/Twain/TwainScanDriver.cs)
- NAPS2's window, on its own STA message thread:
  - `Win32MessagePump` creates a real window (`CreateWindowEx(0, class, "", 0, 0, 0, 0, 0, IntPtr.Zero, …)`, so not `HWND_MESSAGE`), never shown, and ignores other processes' attempts to close it.
  - It makes a separate child window as the dialog parent: returning the caller's own window "can become non-interactable (e.g. unable to cancel a native UI scan). The cause might be related to the window being in another process."
  - Sources: [DefaultTwainHandleManager.cs](https://github.com/cyanfish/naps2/blob/master/NAPS2.Sdk/Scan/Internal/Twain/DefaultTwainHandleManager.cs), [Win32MessagePump.cs](https://github.com/cyanfish/naps2/blob/master/NAPS2.Sdk/Platform/Windows/Win32MessagePump.cs), [Win32TwainHandleManager.cs](https://github.com/cyanfish/naps2/blob/master/NAPS2.Sdk/Platform/Windows/Win32TwainHandleManager.cs)
- NAPS2 8.6.0: "Fixed TWAIN hangs with NAPS2.Sdk.Worker.Win32 by running the worker on an STA thread" — [CHANGELOG.md](https://github.com/cyanfish/naps2/blob/master/CHANGELOG.md)
- RSWebTWAIN's 64-bit app starts a 32-bit `scanner-sidecar` and speaks JSON lines over stdin and stdout. Its parent window is `CreateWindowExW(…, Some(HWND_MESSAGE), …)` — [README](https://github.com/seancampbell3161/RSWebTWAIN), [scanner-sidecar/src/main.rs](https://github.com/seancampbell3161/RSWebTWAIN/blob/main/scanner-sidecar/src/main.rs)
- VintaSoft TWAIN .NET SDK: a 32-bit driver can be used only from a 32-bit application, so the SDK runs a separate 32-bit service executable as an intermediary, chosen with `Use32BitDevices()` — [VintaSoft docs](https://www.vintasoft.com/docs/vstwain-dotnet/Programming-Twain-How_To_Use_32bit_TWAIN_driver_in_64bit_Application.html)
- LEADTOOLS uses a COM+ "Thunk Server" (`LtThunkServer.exe`), registered at setup and enabled with `UseThunkServer` in `TwainSession.Startup`, and only for devices without a 64-bit driver — [LEADTOOLS help](https://www.leadtools.com/help/sdk/dh/to/using-the-leadtools-thunk-utility-with-twain.html), [LEADTOOLS v20 help](https://leadtools.com/help/sdk/v20/twain/api/using-the-leadtools-thunk-utility-with-twain.html)
- Dynamsoft's history:
  - 2014: added 32-bit TWAIN scanner support to 64-bit apps in its .NET TWAIN SDK because "most scanners only provide 32-bit TWAIN drivers" — [Dynamsoft news](https://www.dynamsoft.com/company-news/Dynamsoft-.NET-TWAIN-Software-Dev-Kit-Adds-32-Bit-Scanner-Support-in-64-Bit-Applications/)
  - DWT 16.1 (2020): "Support for showing both 64-bit TWAIN drivers and 32-bit TWAIN drivers at the same time" — [ComponentSource 16.1](https://www.componentsource.com/product/dynamic-web-twain/releases/2589366)
  - DWT 19.0 (April 2025): renamed the Dynamsoft Service to the Dynamic Web TWAIN Service, and moved it out of System32 and SysWOW64 into Program Files — [ComponentSource 19.0](https://www.componentsource.com/product/dynamic-web-twain/releases/2892141)
- The TWAIN 2.4 specification, chapter 12:
  - "All TWAIN 2.x Applications must use TWAINDSM.DLL."
  - "Applications that wish to use access the Data Source Manager, must install it themselves."
  - The 32-bit DSM belongs in `SysWOW64`.
  - "Applications must perform a dynamic run-time link to DSM_Entry( )."
  - "On Windows the official TWAINDSM.dll is signed. This digital signature can be verified to check if an official version is being used."
  - "It is strongly recommended that all TWAIN calls be made from a single thread."
  - Sources are found in `twain_32` and `twain_64`.
  - Source: [TWAIN-2.4-Specification.pdf](https://github.com/twain/twain-specification/blob/HEAD/versions/2.4/TWAIN-2.4-Specification.pdf)
- TWAIN DSM releases: 2.5.1 (2023-02-10, "this is just a code signing for SHA") and 2.5 (2021-11-04). The README says the 64-bit merge module `TWAINDSM64.msm` contains both bitnesses, and that "To prevent hook code from causing an exception on some systems with DEP turned on, link the 32bit version with the /NXCOMPAT:NO flag." `TWAINDSM_LOG` names a log file — [releases feed](https://github.com/twain/twain-dsm/releases.atom), [TWAIN_DSM/README.txt](https://github.com/twain/twain-dsm/blob/HEAD/TWAIN_DSM/README.txt)

### Inferences
- The plan's shape (a 32-bit `scan-worker`, JSON lines, a watchdog, one driver per worker) matches what everyone does. It's simpler than NAPS2's gRPC, and as isolating.
- Worth adopting as rules, learned from NAPS2 but implemented independently:
  - run TWAIN on an STA thread;
  - parent the source to a real hidden top-level window, not `HWND_MESSAGE`, plus a child or owner window for driver dialogs;
  - kill workers through a Job object (`JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`) so they never outlive the app;
  - drain the worker's stderr and stdout;
  - optionally keep a spare worker warm.
- Load the DSM with `LoadLibraryExW(…, LOAD_LIBRARY_SEARCH_SYSTEM32)` (or from the app's own folder for the bundled copy), and check its Authenticode signature as the specification suggests. A bare `LoadLibrary("TWAINDSM.dll")` searches the application directory first.
- The fi-6130's FUJITSU TWAIN32 driver must load in the 32-bit worker. Whether that particular driver needs a visible parent or a modal enable can only be learned on the hardware.

### Gaps
- I found no source saying which drivers fail with a message-only parent. The NAPS2 versus RSWebTWAIN difference is design evidence, not a measured quirk.
- I found no documentation of Dynamsoft's crash isolation, or of how VintaSoft's service is protected.

## Q6. WIA 2.0 from Rust: coverage, wrappers, quirks, and whether Microsoft maintains WIA

### Takeaway
- **Coverage.** `windows` 0.62.2 (2025-10-06, still the latest on 2026-11-08) has the whole WIA 2.0 and 1.0 surface the plan needs, including `implement`able callbacks. There's no C build.
- **A new Rust wrapper.** **`printscan-windows` 0.1.0, published today (2026-11-08, MIT OR Apache-2.0)**, is a WIA 2.0 backend on `windows` 0.62, with confined and documented `unsafe` and quirk rules measured on Windows' inbox WSD driver. The prior audit missed it, as it didn't exist yet. It's in-process, sits under its own `printscan` contract, and pulls in `image`, `png` and `tiff` through `printscan`.
- **Corrections to the plan:**
  - On WIA 2.0, `FEEDER | DUPLEX` in `WIA_IPS_DOCUMENT_HANDLING_SELECT` is wrong (FEEDER is obsolete there since Vista; select the Feeder item and set `DUPLEX`).
  - `0x210001` is `WIA_STATUS_END_OF_MEDIA` in the SDK metadata.
  - Microsoft Learn's error table prints wrong hex for codes 16-21.
  - WIA 1.0 can transfer to memory (`idtGetBandedData`).
  - Microsoft's WIA application documentation is now archived ("previous-versions"), while the WIA driver documentation and samples remain.

### Cited Findings
- `windows` 0.62.2 (2025-10-06; MIT OR Apache-2.0; 398.6M downloads, 107.9M recent) is crates.io's `max_version` on 2026-11-08 — [crates.io](https://crates.io/crates/windows)
- Its `Win32/Devices/ImageAcquisition/mod.rs` is 5,427 lines with 178 `pub unsafe fn` methods, and is gated by the `Win32_Devices_ImageAcquisition` feature. It contains:
  - **Interfaces:** `IWiaDevMgr2` (`EnumDeviceInfo`, `CreateDevice`, `SelectDeviceDlg`, `RegisterEventCallbackInterface`, …), `IWiaItem2`, `IWiaTransfer` (`Download`, `Upload`, `Cancel`, `EnumWIA_FORMAT_INFO`), `IWiaTransferCallback` with an `IWiaTransferCallback_Impl` trait (`TransferCallback`, `GetNextStream`), `IWiaPropertyStorage`, and WIA 1.0's `IWiaDevMgr`, `IWiaItem`, `IWiaDataTransfer` and `IWiaDataCallback`.
  - **Constants:** `WiaDevMgr2` (CLSID), `WIA_IPS_DOCUMENT_HANDLING_SELECT = 3088`, `WIA_IPS_PAGES = 3096`, `WIA_DPS_DOCUMENT_HANDLING_STATUS = 3087`, `FEEDER = 1`, `DUPLEX = 4`, `FRONT_FIRST = 8`, `WIA_CATEGORY_FEEDER`, and `WIA_TRANSFER_ACQUIRE_CHILDREN`.
  - Source: [windows 0.62.2 crate source](https://static.crates.io/crates/windows/windows-0.62.2.crate) (`src/Windows/Win32/Devices/ImageAcquisition/mod.rs`), [docs](https://microsoft.github.io/windows-docs-rs/doc/windows/Win32/Devices/ImageAcquisition/index.html)
- **`0x210001` is named.** The same file has `WIA_STATUS_END_OF_MEDIA: HRESULT = HRESULT(0x210001)`, beside `WIA_STATUS_WARMING_UP` (0x210002), `WIA_STATUS_CALIBRATING` (0x210003) and `WIA_STATUS_SKIP_ITEM` (0x210009). NAPS2 catches `0x210001` after `Download()` with the comment "This error code is undocumented but seems to mean 'no more pages' which can be ignored". A web search for `WIA_STATUS_END_OF_MEDIA` found no Microsoft Learn page — [windows crate source](https://static.crates.io/crates/windows/windows-0.62.2.crate) (mod.rs line 5311), [NAPS2 WiaScanDriver.cs](https://github.com/cyanfish/naps2/blob/master/NAPS2.Sdk/Scan/Internal/Wia/WiaScanDriver.cs)
  - **Correction:** p08 and crates.md call `0x210001` "undocumented". It's a named SDK constant that Microsoft Learn doesn't document. Match it by name.
- **Error-code hex conflict.** The `windows` crate (from the SDK metadata) has these values, while Microsoft Learn's archived "Error Codes (WIA)" table (ms.date 2018-05-31) prints others:

  | Code | `windows` crate (SDK) | Microsoft Learn |
  | --- | --- | --- |
  | `WIA_ERROR_COVER_OPEN` | 0x80210010 | 0x80210016 |
  | `WIA_ERROR_LAMP_OFF` | 0x80210011 | 0x80210017 |
  | `WIA_ERROR_MULTI_FEED` | 0x80210014 | 0x80210020 |
  | `WIA_ERROR_MAXIMUM_PRINTER_ENDORSER_COUNTER` | 0x80210015 | 0x80210021 |

  - `printscan-windows` says "the Windows WSD driver was measured returning 0x80210010 for a cover and 0x80210011 for the lamp", and maps both spellings.
  - `NAPS2.Wia`'s `WiaErrorCodes.cs` uses the Learn values (`COVER_OPEN = 0x80210016`, `LAMP_OFF = 0x80210017`).
  - Sources: [Learn error codes (archived)](https://learn.microsoft.com/en-us/windows/win32/wia/-wia-error-codes), [windows crate source](https://static.crates.io/crates/windows/windows-0.62.2.crate), [printscan-windows crate source](https://static.crates.io/crates/printscan-windows/printscan-windows-0.1.0.crate) (`src/wia/rules.rs`), [NAPS2.Wia WiaErrorCodes.cs](https://github.com/cyanfish/naps2-wia/blob/master/NAPS2.Wia/WiaErrorCodes.cs)
- **`WIA_IPS_DOCUMENT_HANDLING_SELECT`** (WDK docs, ms.date 2023-05-05):
  - Valid since Vista: `ADVANCED_DUPLEX`, `BACK_FIRST`, `BACK_ONLY`, `DUPLEX`, `FRONT_FIRST` and `FRONT_ONLY`.
  - "The values DUPLEX and FRONT_ONLY are mutually exclusive."
  - "A WIA 2.0 minidriver must set the initial value of this property to its default value, FRONT_ONLY."
  - `FEEDER`, `FLATBED`, `AUTO_ADVANCE`, `NEXT_PAGE` and `PREFEED` "are valid with Windows XP but are obsolete with Windows Vista and later".
  - Source: [Learn: WIA_IPS_DOCUMENT_HANDLING_SELECT](https://learn.microsoft.com/en-us/windows-hardware/drivers/image/wia-ips-document-handling-select)
- NAPS2 on WIA 2.0 does exactly that:
  - It picks the root's child named "Feeder" (or "Flatbed").
  - It sets `IPS_PAGES = 0` and `IPS_DOCUMENT_HANDLING_SELECT = FRONT_ONLY` (simplex) or `DUPLEX` (duplex) on that item.
  - It uses `DPS_DOCUMENT_HANDLING_SELECT = FEEDER | DUPLEX` on the device only for WIA 1.0, where it also sets `DPS_PAGES = 1` and repeats `Download()` until `WIA_ERROR_PAPER_EMPTY`.
  - A feeder scan with no image and no cancel becomes "feeder empty".
  - Sources: [WiaScanDriver.cs](https://github.com/cyanfish/naps2/blob/master/NAPS2.Sdk/Scan/Internal/Wia/WiaScanDriver.cs), [NAPS2.Wia README](https://github.com/cyanfish/naps2-wia/blob/master/README.md)
  - **Correction:** crates.md and p08's "`WIA_IPS_DOCUMENT_HANDLING_SELECT = FEEDER | DUPLEX`" is the WIA 1.0 root property's value. On WIA 2.0, write `DUPLEX` (or `FRONT_ONLY`) to the Feeder item.
- **`printscan-windows` 0.1.0**:
  - **Release.** Published 2026-11-08 with its siblings `printscan` and `printscan-wsd` (9 downloads so far, 0 reverse dependencies). MIT OR Apache-2.0, Copyright 2026 Justin Capella, repository `b1tninja/printscan`; it "Requires Rust 1.95".
  - **Scope.** "WIA 2.0 scanners as a `printscan::scan` backend… Listing opens no device; reading details opens it; only `acquire` feeds paper."
  - **Unsafe.** `unsafe_code = "deny"` and `clippy::undocumented_unsafe_blocks = "deny"`. Its "34 sites", each with a `// SAFETY:` comment, sit in `src/wia/ffi.rs` and `src/wia/sinks.rs` (`docs/wia-unsafe-audit.md`), and the spooler's `src/spooler/draw.rs` also allows `unsafe`. The backend moved from `storytold/pdfcraft#1724`.
  - **Dependencies.** `windows` 0.62 with the `Win32_Devices_ImageAcquisition`, `Win32_System_Com`, `Win32_System_Com_StructuredStorage` and `Win32_System_Variant` features.
  - **Tests.** 72 test functions in `tests/wia_rules.rs` and `tests/wia_rule_tables.rs`, on COM-free rule tables that build on every platform.
  - Sources: [crates.io printscan-windows](https://crates.io/crates/printscan-windows), [crate source](https://static.crates.io/crates/printscan-windows/printscan-windows-0.1.0.crate), [releases feed](https://github.com/b1tninja/printscan/releases.atom)
- `printscan-windows`' measured WSD-driver quirks, from `src/wia/rules.rs`, `sinks.rs` and `backend.rs` ([crate source](https://static.crates.io/crates/printscan-windows/printscan-windows-0.1.0.crate)):
  - "The inbox WSD driver keeps duplex here, on the feeder item, not on the device root (where the property does not exist)."
  - The WSD feeder item "publishes the flag bits `FRONT_ONLY | DUPLEX` as allowed, refuses the value `FRONT_ONLY | DUPLEX` with `E_INVALIDARG` and takes `DUPLEX`".
  - The WSD driver's `FEED_READY` bit "is a cached reading that can stay set with an empty tray (or clear with paper)".
  - "On this driver the Pages property can read back as that count while the feeder still runs the stack, so the transfer is cancelled and the feeder is told to stop" (`WIA_CMD_STOP_FEEDER`).
  - The whole feeder (`0`) is replaced with a maximum page count for WSD.
  - Progress: "trust `ulTransferredBytes`, not `lPercentComplete` (peaks ~40 then snaps to 100)".
  - "Some WSD drivers fault if two are held", so open one device at a time.
  - `S_FALSE` from the callback is WIA's cancel.
  - Banded transfer falls back to file transfer when a driver rejects it "without naming a device fault".
  - The text of `E_FAIL` is localized, so decide by the code.
- **`printscan`** (the core, 0.1.0, 2026-11-08, 28,405 lines of Rust) has backends for WSD (WS-Scan), IPP and Windows (spooler, WIA). It has no TWAIN, SANE or ICA. Its `scan` feature pulls `thiserror`, `image`, `png` and `tiff` — [printscan crate source](https://static.crates.io/crates/printscan/printscan-0.1.0.crate) (`README.md`)
- **`NAPS2.Wia`** (`cyanfish/naps2-wia`): MIT (Copyright 2020 Ben Olden-Cooligan), about 3,000 lines of C#, NuGet 2.0.3 (2023-08-30), last commit 2023-08-30. Its README:
  - "Unlike most of NAPS2.Sdk which is licensed under the LGPL, NAPS2.Wia uses the more permissive MIT license."
  - "Native acquisition with WIA 1.0 only works in 32-bit processes."
  - Its example sets `IPS_PAGES` to all pages and `IPS_DOCUMENT_HANDLING_SELECT = DUPLEX` on the "Feeder" sub-item.
  - Sources: [README](https://github.com/cyanfish/naps2-wia/blob/master/README.md), [LICENSE](https://github.com/cyanfish/naps2-wia/blob/master/LICENSE), [commit feed](https://github.com/cyanfish/naps2-wia/commits.atom)
- **WIA 1.0 can transfer to memory.** `IWiaDataTransfer::idtGetBandedData` "transfers a band of data from a hardware device to an application", through a shared memory section and the `BandedDataCallback`. The page is archived (`is_archived: true`, canonical under `previous-versions`) — [Learn idtGetBandedData](https://learn.microsoft.com/en-us/windows/win32/api/wia_xp/nf-wia_xp-iwiadatatransfer-idtgetbandeddata)
  - **Correction:** p08 and `scanners.md` say "WIA 1.0 … scan[s] only to files". That holds for WinRT, not for WIA 1.0's `IWiaDataTransfer`. WIA 1.0 is still rightly skipped, for the 32-bit-only native-acquisition caveat and because WIA 2.0 covers every device that matters.
- **WinRT's `Windows.Devices.Scanners.ImageScanner`** has `ScanFilesToFolderAsync` (files only) and `ScanPreviewToStreamAsync`, which applies the lowest resolution — [Learn ImageScanner](https://learn.microsoft.com/en-us/uwp/api/windows.devices.scanners.imagescanner)
- **Is WIA maintained?**
  - The WIA application reference pages fetched (error codes, `idtGetBandedData`) are Learn "previous-versions" archives (`is_archived: true`, updated 2025-02). The WDK's WIA driver pages are live but say "Some information contained in this article applies to obsolete Windows operating systems" (ms.date 2023-05-05).
  - `microsoft/Windows-driver-samples` still has `wia/` with `ProdScan` (a WIA 2.0 minidriver with production-scanning features), `wiadriverex` (a WIA 2.0 stream-based minidriver with segmentation, image-processing and error-handling extensions), `microdrv` and `inc`, with no deprecation note.
  - Sources: [Learn error codes](https://learn.microsoft.com/en-us/windows/win32/wia/-wia-error-codes), [Learn WIA_IPS_DOCUMENT_HANDLING_SELECT](https://learn.microsoft.com/en-us/windows-hardware/drivers/image/wia-ips-document-handling-select), [Windows-driver-samples/wia](https://github.com/microsoft/Windows-driver-samples/tree/main/wia)
- I found no Microsoft announcement deprecating WIA or third-party scan drivers. The print-side change (Windows Protected Print Mode, and an end of third-party printer drivers through Windows Update reported for 2027) is about printers, and is secondhand — [Wikipedia: Mopria Alliance](https://en.wikipedia.org/wiki/Mopria_Alliance), [search summary of blogs, low authority](https://www.elevenforum.com/t/will-legacy-scanner-driver-still-work-in-26h2.49975/)

### Inferences
- **Use `windows` directly** for the COM surface (as the audit said). **Port** (MIT OR Apache-2.0, with attribution) the parts of `printscan-windows` that are pure knowledge:
  - the HRESULT tables, both spellings of codes 16-21;
  - the `item_handling_select` rule;
  - halting the feeder with `WIA_CMD_STOP_FEEDER`;
  - the WSD `FEED_READY` distrust;
  - one device open at a time;
  - progress from bytes, not percent;
  - the banded-to-file fallback.
- Don't depend on `printscan-windows` as a crate, for three reasons:
  - It's one day old (0.1.0, 9 downloads).
  - It drives scans in-process behind its own contract, not `scan-device`'s.
  - Its `printscan` core brings `image`, `png` and `tiff` under the device layer, which breaks the plan's "no image codecs under `scan-device`" gate.
- Contributing upstream is reasonable later, if its `printscan::scan` contract stabilizes. Watch it.
- `NAPS2.Wia` (MIT) is a second permissive reference for the transfer callback and `IStream` wrapper. Its error table repeats the Learn mistake, so check values against the `windows` crate.
- Revise p08: on WIA 2.0, open the root's Feeder child, set `WIA_IPS_PAGES = 0`, and set `WIA_IPS_DOCUMENT_HANDLING_SELECT` to `DUPLEX` or `FRONT_ONLY`. Treat `WIA_STATUS_END_OF_MEDIA` (0x210001) as "done", and both spellings of cover open and double feed as those errors.

### Gaps
- I couldn't fetch the WIA 2.0 application reference pages (`IWiaTransfer` and `IWiaDevMgr2` URLs returned 404), so whether they're also archived is unverified.
- `printscan-windows` has no history of users or issues yet. Its WSD measurements are its author's, on unnamed devices.
- Whether every eSCL copier appears under WIA on Windows 11 is still unverified (as in the audit).

## Q7. macOS ImageCaptureCore from Rust

### Takeaway
`objc2-image-capture-core` 0.3.2 (2025-10-04, Zlib OR Apache-2.0 OR MIT) is still current, and covers `ICScannerDevice`, the functional units including the document feeder, and band data. The objc2 repository is active (Xcode 27.1 update on 2026-10-16), but no newer framework crates have been released. The older `image-capture-core` 0.1.5 (2019, on the deprecated `objc` and `cocoa` crates) is superseded. A permissive, current macOS command-line scanner to learn the ImageCaptureCore flow from exists: `scanline` (MIT, 2025). NAPS2 uses ImageCaptureCore in a worker (study only).

### Cited Findings
- `objc2-image-capture-core` 0.3.2 (2025-10-04): Zlib OR Apache-2.0 OR MIT, 75,011 downloads (72,797 recent), 1 reverse dependency (`objc2-quartz`).
  - 18 features, 17 on by default, including `ICDeviceBrowser`, `ICScannerDevice`, `ICScannerFunctionalUnits` and `ICScannerBandData`.
  - Targets: aarch64 and x86_64 Darwin, plus iOS, Mac Catalyst and visionOS.
  - Sources: [crates.io](https://crates.io/crates/objc2-image-capture-core), [docs.rs features](https://docs.rs/crate/objc2-image-capture-core/latest/features)
- objc2 releases: `objc2-0.6.5` (2026-10-05), with commits through 2026-10-17 ("Update to Xcode 27.1", 2026-10-16). The framework crates are still at 0.3.2 on crates.io — [releases feed](https://github.com/madsmtm/objc2/releases.atom), [commit feed](https://github.com/madsmtm/objc2/commits.atom)
- `image-capture-core` 0.1.5 (2019-09-02, MIT, 7,713 downloads; last commit 2021-08-22) depends on `objc` 0.2, `cocoa` 0.19 and `core-foundation` 0.6, and has scanner-device and functional-unit modules — [crates.io](https://crates.io/crates/image-capture-core), [crate source](https://static.crates.io/crates/image-capture-core/image-capture-core-0.1.5.crate), [commit feed](https://github.com/brandonhamilton/image-capture-core-rs/commits.atom)
- `scanline` (`klep/scanline`) is MIT (Copyright 2025 Scott J. Kleper), last commit 2025-11-30. Its README: "Scan from the document feeder or flatbed", batches, formats and sizes, a config file — [LICENSE](https://github.com/klep/scanline/blob/master/LICENSE), [README](https://github.com/klep/scanline/blob/master/README.md), [commit feed](https://github.com/klep/scanline/commits.atom)
- NAPS2 on macOS uses `ImageCaptureCore` (`AppleScanDriver.cs`: "we don't want to dispose the device, as the ICDeviceBrowser manages its lifetime"). Its changelog:
  - 8.6.0: added "Apple Driver Implementation … (File or Memory transfer)"
  - 8.4.1: "Fixed a scanning issue with Samsung scanners and Apple Driver"
  - 8.4.0: "Custom page sizes are no longer shown for feeders with the Apple driver (as they don't work)"; "Fixed flatbed/feeder detection with the Apple driver"
  - 7.5.2: "Fixed an issue with Apple Driver and out-of-order pages"
  - Sources: [AppleScanDriver.cs](https://github.com/cyanfish/naps2/blob/master/NAPS2.Sdk/Scan/Internal/Apple/AppleScanDriver.cs), [CHANGELOG.md](https://github.com/cyanfish/naps2/blob/master/CHANGELOG.md)
- Apple's `VirtualScanner` sample (an Image Capture scanner *device module*, version 1.2, 2012-06-12) is archived. A Dynamsoft write-up says it needs a `require_action` macro fix to build on macOS Sequoia, and that it terminates after one scan (`_ICD_ScannerCloseDevice`) — [Apple archive](https://developer.apple.com/library/archive/samplecode/VirtualScanner/Introduction/Intro.html), [Dynamsoft article](https://dynamsoft.com/codepool/macos-virtual-scanner-custom-images.html)

### Inferences
- **Use directly**; contribute upstream to objc2 if a binding is missing. It's FFI to a system framework, needs no C compiler, and is permissive.
- `scanline` (MIT) is a legitimate port source for the session flow. NAPS2's Apple driver is study only.
- NAPS2's macOS quirks (no custom sizes on feeders, flatbed and feeder detection, out-of-order pages, file versus memory transfer) are the checklist to verify by hand.

### Gaps
- I didn't confirm `scanline`'s implementation language or that it uses `ICScannerDevice` (a README read only).
- Whether ImageCaptureCore's delegates need the main thread's run loop in a worker process is unverified. The worker will need a run loop either way.
- No Mac scanner is available to test with (as in the audit).

## Q8. Commercial SDKs (context only): what they offer that open source lacks

### Takeaway
Dynamsoft (Dynamic Web TWAIN 19.x), LEADTOOLS, VintaSoft and Asprise sell what open source lacks:
- a supported, installed 32-and-64-bit broker (a service or COM+ thunk);
- browser bridges;
- vendor support with driver-compatibility testing;
- multi-protocol coverage (TWAIN, WIA, ICA, SANE) behind one API.

None of it can be reused; it's context for what "done" looks like.

### Cited Findings
- Dynamsoft:
  - DWT 16.1 (2020) shows 32- and 64-bit TWAIN drivers at once — [ComponentSource 16.1](https://www.componentsource.com/product/dynamic-web-twain/releases/2589366)
  - DWT 19.0 (April 2025) renamed and relocated its local service — [ComponentSource 19.0](https://www.componentsource.com/product/dynamic-web-twain/releases/2892141)
  - The npm package builds in 19.5.0 and needs a license key — [Dynamic-Web-TWAIN README](https://github.com/Dynamsoft/Dynamic-Web-TWAIN/blob/master/README.md)
- LEADTOOLS: a COM+ Thunk Server for 32-bit drivers from 64-bit processes — [LEADTOOLS help](https://www.leadtools.com/help/sdk/dh/to/using-the-leadtools-thunk-utility-with-twain.html)
- VintaSoft: a 32-bit service executable and `Use32BitDevices()` — [VintaSoft docs](https://www.vintasoft.com/docs/vstwain-dotnet/Programming-Twain-How_To_Use_32bit_TWAIN_driver_in_64bit_Application.html)
- Asprise documents the hidden-dialog problem (driver dialogs opening behind the app) as a support topic — [Asprise doc](https://cdn.asprise.com/c%23-vb.net-scanner-api/scan-dialogues-behind-not-shown-hidden.html)
- Dynamsoft and Atalasoft people are among the TWAIN 2.4 specification's acknowledged contributors — [TWAIN-2.4-Specification.pdf](https://github.com/twain/twain-specification/blob/HEAD/versions/2.4/TWAIN-2.4-Specification.pdf)

### Inferences
- The one thing they have that the planned stack doesn't is breadth of tested drivers. The plan answers that with a per-scanner checklist by hand (p22).

### Gaps
- I didn't check Atalasoft DotTwain's current 32-bit bridging under that name. VintaSoft's documentation was the closest found.
- No pricing or license terms were gathered (not needed).

## Q9. Testing without hardware

### Takeaway
- **TWAIN.** The TWAIN Working Group's sample data source (BSD-3-Clause, a virtual scanner reading images from disk, 32- and 64-bit MSIs in the repository) remains the best CI candidate. Whether it installs silently and scans on `windows-latest` is still unverified.
- **A newer virtual scanner set.** `yushulx/virtual-scanner` (active 2026-10-15) packages a fork of that sample as an installer or portable zip, plus a SANE backend and a macOS ICA module, built by GitHub Actions. Its Windows part keeps the BSD license; there's no repository-level license.
- **WIA.** I found no virtual device that installs unattended. The WDK samples exist, but need driver installation.
- **macOS.** Apple's VirtualScanner is a 2012 device-module sample that's hard to build and terminates after one scan.

### Cited Findings
- The sample data source:
  - "Instead of interfacing with a real scanner, it talks to a virtual scanner. The virtual scanner acquires its image from a file on disk."
  - It uses FreeImage.
  - "The DS reads an environment variable, CAP_XFERCOUNT, to get the number of times to send an image. By default only 1 is sent. Setting this to -1 will make the source choose a random number."
  - The README says the MSIs were "fixed so a development system is not needed".
  - Sources: [Twain_DS_sample01 README.txt](https://github.com/twain/twain-samples/blob/HEAD/TWAIN-Samples/Twain_DS_sample01/README.txt), [twain-samples README](https://github.com/twain/twain-samples/blob/HEAD/README.md)
- The repository tree has `releases/Twain_Ds_sample01_02040100/twainds.win32.installer.msi` (8.7 MB) and `twainds.win64.installer.msi` (19.0 MB), plus application MSIs for 2.4.0.0 and 2.4.1.0. The GitHub release is dated 2021-11-09 — [twain-samples tree](https://github.com/twain/twain-samples/tree/HEAD/releases), [releases feed](https://github.com/twain/twain-samples/releases.atom)
- `yushulx/virtual-scanner`:
  - "Virtual (software only) scanners for Windows, Linux and macOS… handy for testing scanning workflows, demos and CI without hardware."
  - Windows: a TWAIN data source (C++, Qt 5, FreeImage) as an Inno Setup installer, or `VirtualScanner-1.0.0-win64-portable.zip` "unpack[ed] into `C:\Windows\twain_64\sample2`". It needs a DSM present.
  - Linux: a SANE backend `.deb`.
  - macOS: an ICA device module `.pkg`.
  - Installers are built by a GitHub Actions workflow; last commits 2026-10-15.
  - `windows/TWAIN-Samples/Twain_DS_sample01/license.txt` keeps the TWAIN Working Group's BSD-3-Clause text; `LICENSE` at the root returns 404.
  - Sources: [README](https://github.com/yushulx/virtual-scanner/blob/main/README.md), [sample license](https://github.com/yushulx/virtual-scanner/blob/main/windows/TWAIN-Samples/Twain_DS_sample01/license.txt), [commit feed](https://github.com/yushulx/virtual-scanner/commits.atom)
- Dynamsoft's write-up (updated 2026-10-15) calls it "ideal for automated CI/CD testing" but documents no headless setup, and its installer asks for administrator rights — [Dynamsoft article](https://www.dynamsoft.com/codepool/windows-virtual-scanner-custom-images.html)
- The WIA samples in `microsoft/Windows-driver-samples/wia` (`wiadriverex`, `ProdScan`, `microdrv`) are minidrivers to build with the WDK and install — [Windows-driver-samples/wia](https://github.com/microsoft/Windows-driver-samples/tree/main/wia)
- `printscan-windows` tests its WIA rule tables off-device (72 test functions over COM-free code), and RSWebTWAIN tests its protocol with a fake sidecar binary. Neither drives a real DSM or WIA in CI — [printscan-windows crate source](https://static.crates.io/crates/printscan-windows/printscan-windows-0.1.0.crate), [RSWebTWAIN tests](https://github.com/seancampbell3161/RSWebTWAIN/tree/main/src-tauri/tests)
- NAPS2's CI runs no scanner; its scanner tests are a manual project (audit finding, not re-checked).

### Inferences
- A CI path for TWAIN that may avoid the MSI:
  1. Copy the DSM (both bitnesses, from twain-dsm's release or merge module) into `System32` and `SysWOW64`.
  2. Unpack the sample data source's files (from the BSD sample's MSI or `yushulx`'s portable zip) into `C:\Windows\twain_64\sample2` and `twain_32\sample2`.
  3. Set `CAP_XFERCOUNT` for page counts.

  This is unverified, and S4's spike should try it.
- Use the official BSD sample's binaries, or `yushulx`'s Windows zip, whose BSD notice is retained. Don't vendor `yushulx`'s macOS or Linux parts without a license.
- For WIA, keep the plan: a trait over the WIA calls, with the rule tables (ported from `printscan-windows`) unit-tested on every platform, and scanning by hand.

### Gaps
- Unverified:
  - whether GitHub's hosted Windows runners allow writing to `C:\Windows\twain_*`;
  - whether the sample data source works there with a hidden window;
  - whether `msiexec /qn` succeeds.
- Whether the WIA service (`stisvc`) runs on Windows Server-based runners, and whether a WDK WIA sample can be root-enumerated and installed unattended (driver signing), is unverified. I found no source.

## Verdicts per need (`twain-dsm`, `scan-device`'s `twain`, `wia` and `ica` backends, `scan-worker`)

### Takeaway
Build the TWAIN stack. Take its definitions from the MIT `TWAIN.cs` (or from `twain.h` with the TWAIN Toolkit disclaimer). Use `windows` directly for WIA, and port `printscan-windows`' and `NAPS2.Wia`'s permissive quirk knowledge. Use `objc2-image-capture-core` directly for macOS. NAPS2, NAPS2.Sdk, pytwain and Saraff are study only.

### Cited Findings
Each row's evidence is in the Q section named.

| Need | Verdict | Why |
| --- | --- | --- |
| TWAIN bindings (constants, structures, `DSM_Entry`) | **Port (permissive)** from `TWAIN.cs`, cross-checked against `twain.h` 2.5. Add the TWAIN Toolkit disclaimer to the file header and docs | No usable crate (Q1). `twain.h` has no grant of its own, and the TWAIN License requires that disclaimer (Q2). `TWAIN.cs` is MIT and current to 2.5. RSWebTWAIN's hand transcription from 2.3 needed a six-constant fix |
| DSM loading (`TWAINDSM.dll` at run time) | **Use directly** (the unmodified LGPL-2.1-or-later DLL). Our own loader with `LoadLibraryExW` and system-folder search, plus an Authenticode check | The specification says to link at run time and that the official DLL is signed (Q5). DSM 2.5.1 (2023-02-10) is the latest. Zero C build |
| `twain` crate 0.1.0 | **Study only** (don't use) | 2021 prototype with UB, a hard-coded DLL path, bundled LGPL DLLs and no memory transfer (Q1) |
| Typestate wrapper over the 7 states | **Build our own**, porting the shape from RSWebTWAIN (MIT OR Apache-2.0) | Only RSWebTWAIN has one. Its design is sound; its code is alpha (Q1) |
| Hidden window and message loop | **Build our own** on `windows` (or `windows-sys`): an STA thread, a real hidden top-level window (not `HWND_MESSAGE`), a child or owner for driver dialogs, every message through `MSG_PROCESSEVENT` | NAPS2's STA fix and window design (study only) and the specification's single-thread rule (Q5) |
| Capability negotiation | **Build our own**, porting container encoding from `TWAIN.cs` or NTwain (MIT). Allocate with `DSM_MemAllocate`, else `GlobalAlloc`; lock with `DSM_MemLock`, else `GlobalLock` | RSWebTWAIN passes a `Box` pointer as `hContainer` (Q1). `TWAIN.cs` shows the fallback (Q4) |
| Native and memory transfer | **Build our own**: memory by default, native as a per-driver fallback; tiles at `XOffset`/`YOffset`; tolerate `BitsPerSample` with only the first entry set | NAPS2's defaults and quirks (study only, Q4). No permissive implementation handles tiles |
| 32-bit worker and crash isolation (`scan-worker`) | **Build our own**, as planned (JSON lines). Add a Job object, drained pipes and an optional spare worker | NAPS2 (GPL, study only), RSWebTWAIN (permissive, simple sidecar), VintaSoft, LEADTOOLS and Dynamsoft all use a separate process (Q5) |
| WIA 2.0 COM surface | **Use directly**: `windows` 0.62 (`Win32_Devices_ImageAcquisition` plus the COM, StructuredStorage and Variant features) | Complete, Microsoft-maintained, MIT OR Apache-2.0, no C build (Q6) |
| WIA quirks and error mapping | **Port (permissive)** from `printscan-windows` (MIT OR Apache-2.0) and `NAPS2.Wia` (MIT), checking values against the `windows` crate | WSD-measured rules; Learn's table is wrong for codes 16-21; 0x210001 is `WIA_STATUS_END_OF_MEDIA` (Q6) |
| `printscan` and `printscan-windows` as dependencies | **Study only for now** (watch; contribute upstream later if its contract fits) | One day old, in-process, its own contract, and `image`, `png` and `tiff` under the device layer (Q6) |
| macOS ImageCaptureCore | **Use directly** `objc2-image-capture-core` 0.3.2 (contribute upstream if a binding is missing). `scanline` (MIT) as a port reference; NAPS2's Apple driver study only | Current, permissive, system FFI (Q7) |
| TWAIN Direct | **Study only** (don't build) | Dormant since 2022, no shipping devices found (Q3) |
| TWAIN test fixture | **Use directly** the TWAIN Working Group's sample data source (BSD-3-Clause) if the CI spike works; `yushulx`'s portable Windows build (BSD notice kept) as an alternative | Q9 |
| NTwain, NAPS2.NTwain, TwainDotNet | **Port (permissive)** selectively (NTwain and NAPS2.NTwain); TwainDotNet study only (dormant since 2020, origin in a CodeProject article) | Q4 |
| NAPS2, NAPS2.Sdk, pytwain, Saraff.Twain | **Study only** (GPL-2.0+, LGPL-2.1+, GPL-2.0, LGPL-3.0+) | License rule (Q4) |

- FFI, `unsafe` and native build requirements:
  - **FFI that can't be avoided:** `DSM_Entry` and the DSM memory callbacks (TWAIN), WIA COM (`windows` crate calls are `unsafe fn`), and ImageCaptureCore (objc2).
  - **`unsafe` as others measured it:**
    - `printscan-windows` keeps 34 WIA sites in two modules, with `unsafe_code = "deny"` and documented blocks.
    - RSWebTWAIN uses 36 plus 39 `unsafe` occurrences across its TWAIN wrapper and sidecar.
    - `twain` 0.1.0 has 18, including UB.
  - **C at build time:** none on any of the three routes.
  - **At run time:** `TWAINDSM.dll` (C++, LGPL) and the drivers themselves.
  - **Test fixtures:** C++ with Qt 5 and FreeImage, prebuilt.
  - Sources: [printscan-windows crate source](https://static.crates.io/crates/printscan-windows/printscan-windows-0.1.0.crate) (`docs/wia-unsafe-audit.md`), [RSWebTWAIN](https://github.com/seancampbell3161/RSWebTWAIN), [twain 0.1.0](https://static.crates.io/crates/twain/twain-0.1.0.crate)
- **Corrections to the 2026-10-23 audit, in one place:**
  1. RSWebTWAIN is MIT OR Apache-2.0, not Apache-2.0 alone. It has a v0.1.0 release (2026-04-28), and its default branch's last commit is 2026-09-11.
  2. `0x210001` is `WIA_STATUS_END_OF_MEDIA`, named in the SDK metadata.
  3. The WIA 2.0 duplex setting is `DUPLEX` on the Feeder item, not `FEEDER | DUPLEX`.
  4. Learn's WIA error hex for cover open, lamp off, multi-feed and the endorser counter is wrong. Match the SDK values (0x80210010, 0x80210011, 0x80210014, 0x80210015) as well.
  5. WIA 1.0 can transfer to memory (`idtGetBandedData`).
  6. "Kodak defaulting to duplex" is a SANE `kds_s2000` issue on Linux.
  7. pytwain is GPL-2.0.
  8. `twain.h` has no license of its own. The TWAIN License's disclaimer condition applies, and `TWAIN.cs` is an MIT alternative.
  9. A Rust WIA 2.0 backend now exists (`printscan-windows`, 2026-11-08).

### Inferences
- The verdict for `twain-dsm` stays "build". The change is where the definitions come from (MIT `TWAIN.cs`), plus the disclaimer. That closes crates.md's open question, "`twain.h`'s license, before the bindings are written".
- The `wia` backend's plan needs the three WIA corrections above before slice 8.

### Gaps
- None beyond those listed in each question.

## Holes in the ecosystem

### Takeaway
Six needs have no adequate permissive or pure-Rust implementation.

### Cited Findings
- **A safe, maintained Rust TWAIN binding.** Nothing on crates.io, beyond the 2021 prototype — [crates.io](https://crates.io/crates/twain)
- **A Rust 32-bit TWAIN broker as a library.** Only RSWebTWAIN's app-internal sidecar exists — [RSWebTWAIN](https://github.com/seancampbell3161/RSWebTWAIN)
- **A permissive, published TWAIN quirk list.** The knowledge sits in copyleft NAPS2 and NAPS2.Sdk code and changelogs — [NAPS2 CHANGELOG](https://github.com/cyanfish/naps2/blob/master/CHANGELOG.md)
- **TWAIN memory transfer with tiles and odd `BitsPerSample`.** Handled in NAPS2.Sdk (LGPL) and not by RSWebTWAIN — [TwainMemoryBufferReader.cs](https://github.com/cyanfish/naps2/blob/master/NAPS2.Sdk/Scan/Internal/Twain/TwainMemoryBufferReader.cs)
- **An unattended virtual WIA device for CI.** None found — [Windows-driver-samples/wia](https://github.com/microsoft/Windows-driver-samples/tree/main/wia)
- **A safe Rust ImageCaptureCore scanning layer** (session, feeder, duplex and band assembly over objc2). Only raw bindings exist — [objc2-image-capture-core](https://crates.io/crates/objc2-image-capture-core)

### Inferences
- `twain-dsm`, the `twain` and `ica` backends, and `scan-worker` would each fill a real hole.
- The `wia` backend overlaps a day-old permissive crate whose knowledge can be ported.

### Gaps
- Whether `printscan`'s author plans TWAIN or ICA backends is unknown: its README lists only WSD, IPP and Windows.
