# What's settled

Rulings this plan builds on, with where each was made: sheaf's and pdf-lint's first, then those made here.

## From sheaf's plan

- **M5's scanner and separator work is deferred** (Greg, 2026-10-23): "without physical scanners to test
  against it's not a valuable use of time or tokens right now". Slices 4 to 10 wait for scanners. This plan
  doesn't overturn that for sheaf; ask 4 asks how much of the crates' device work it holds back.
- **Library work may go first** between milestones (Greg, 2026-10-23, ROADMAP's ordering principle):
  "capability or speed that much of sheaf uses, or work that can be published as a library of its own".
- **Scanner access and Kodak patch codes are library candidates** when M5 resumes; the OCR engine trait
  isn't one ("thin over the engines' own crates") (`plan/libraries.md`, 2026-10-23).
- **D17, copyleft loaded at run time:** allowed (Greg, 2026-10-23): "The linking rule for dynamic libs is
  fine." Loading an unmodified, separately shipped library (`TWAINDSM.dll`, LGPL-2.1; libsane, whose
  backends are GPL-2.0+ with the SANE exception) isn't copying, translating or linking. Ship its license
  text and a source offer.
- **M5's kickoff answers** (2026-10-23), taken as recommended under Greg's standing instruction, open to his
  review until the slice that depends on each starts:
  - ask 3: TWAIN and WIA on Windows, TWAIN first, with a 32-bit worker for 32-bit TWAIN drivers;
  - ask 4: macOS isn't in M5 (a Pending entry for ImageCaptureCore);
  - ask 5: drivers always in a worker process;
  - ask 6: libsane loaded at run time, with sheaf's own bindings;
  - ask 7: rxing behind a small trait, zxing-cpp second;
  - ask 13: Linux CI installs libsane1 and runs SANE's test backend and a fake libsane; TWAIN starts with a
    spike on the Windows runner; WIA by hand.
- **Ask 12, answered** (Greg, 2026-10-23): no scanners at home; at work, several Fujitsu fi-6130 duplex
  color scanners, "old, but very good". Old drivers are a main case, not a fallback. The fi-6130's Windows
  driver is FUJITSU TWAIN32, 32-bit; SANE's fujitsu backend rates it complete. Open: whether a development
  build may run on a work PC.
- **Build or adopt** (audit 2026-10-23): SANE, TWAIN, patch codes and cleanup built in-house (entries 5, 6,
  12, 14); WIA through `windows` (7); ImageCaptureCore through objc2 when scheduled (8); eSCL not planned
  (9); rxing for barcodes, its encoder for QR (10, 11); zune-jpeg, hayro-ccitt, hayro-jbig2 and
  hayro-jpeg2000 to decode, `fax` to encode G4, JBIG2 never encoded, jpeg-encoder to re-encode (33 to 38).
- **Cleanup's existing output doesn't change** without a reason said (entry 64's byte-identical rule).
- **Licenses** (sheaf's AGENTS.md): GPL, LGPL and AGPL code may be studied and cited, never copied,
  translated or linked; running such a tool as a separate process in tests is fine.

## From pdf-lint's plan and M18

- **Pixels stay outside the PDF family:** "Image codecs (DCT, JPX, CCITT, JBIG2) stay outside the family:
  entry 33 owns them" (sheaf's M18 `crates.md:85-87`, `scope.md:147-149`; the only reason given is that, and
  that PDFium decodes images for sheaf's pages). As pdf-lint practises it: the family writes no codec and
  keeps no samples; mature codec crates are allowed behind a feature, as
  `image-ends` does, which decodes DCT and CCITT inline data only to find where it
  ends.
- **The family publishes as alphas** once each crate's boundaries are reviewed and it's polished (Greg,
  2026-11-04, for the order). On 2026-11-08 crates.io has pdf-tokenizer 0.2.1, pdf-objects and pdf-filters
  0.1.0-alpha.2, pdf-crypt and pdf-color 0.1.0-alpha.1, and pdf-lint 0.1.0; pdf-structure,
  pdf-content-streams, pdf-fonts, pdf-semantics, pdf-linearize and pdf-arlington aren't published yet
  (`pdf-lint/README.md`).
- **The family's MSRV** is 1.89, every crate's `rust-version`; sheaf's is 1.98.
  The hayro codecs pdf-scan needs declare 1.92 (Q-1, PL-3).
- **D4:** zlib-rs in every sheaf build; pdf-filters supports both Flate backends. pdf-structure deflates on
  miniz_oxide unless told (`Deflate::DEFAULT`) and pdf-content-streams forces zlib-rs for inline images, so
  pdf-scan names the backend at every call, for byte-stable output.

## Made here

- **sheaf isn't changed from here; anything in it may be copied out** (Greg, 2026-11-08): "no changes in
  sheaf from here, but free to copy items out". So:
  - ask 8 is answered no: sheaf's plan doesn't point here (no line in `libraries.md` or M5's card). sheaf
    moves on without knowing this plan exists, so this plan checks sheaf for changes before each slice
    (`scratch/reference/README.md`);
  - sheaf's open entries a crate here fixes are copied into `backlog/` (BUG-1 to BUG-5, FEAT-1 to FEAT-4) and
    stay open in sheaf, for sheaf's swap to close;
  - the plan files this plan cites are copied as at `3232a35` into `scratch/reference/sheaf/`, never edited there;
  - sheaf's code is copied into each crate as planned, at the commit each slice pins;
  - sheaf's side of each milestone (`sheaf/swaps.md`) is sheaf's own work, done in sheaf, not from here.
- **pdf-scan is the PDF family's consumer; its shortcomings go back as proposals** (Greg, 2026-11-08):
  "pdf-lint will continue to move. it is currently in alpha releases for most of the family of libraries. As a
  consumer of these crates if you identify any major shortcomings we should create proposals that can be
  handed off to them for refactor". So nothing is changed in pdf-lint from here; a major shortcoming becomes a
  proposal (kept local, `scratch/pdf-lint/proposals/`), handed off with Greg's OK, while pdf-scan works around
  it; published alphas are pinned exactly (Greg, the same day: "pin alphas exactly is fine"), unpublished
  crates taken by path; the family is read again before each slice that touches it (`pdf-lint/README.md`).
- **The device layer is built bindings first, its model frozen late** (Greg, 2026-11-08, taking the
  recommendation: "make the edits"). `sane-dl` comes first (it follows SANE alone and needs no model); a
  seam note (`crates/scan-device-seam.md`) records how each API delivers each concept, before
  `scan-device`'s code; the model stays provisional (0.0.x) through the Linux slices and is frozen once the
  `twain` backend has been through it (`milestones/s0-devices.md`, slice 11).
- **sheaf adopts the device crates after S0 at the earliest, and maybe later** (Greg, the same day: "for
  swaps we may delay sheafs adoption even further"; `sheaf/swaps.md`).
- **Where the crates live: this workspace, its own repository** (ask 1, answered by Greg, 2026-11-08: "push
  to a new repo under sheaf-pdf/pdf-scan"): github.com/sheaf-pdf/pdf-scan, created private, its plan in
  `docs/pm/` and local-only material in `scratch/`. It went public the same day (below).
- **The repository is public; nothing private goes in it** (Greg, 2026-11-08: "it can be public as long as
  we don't have any private info in the docs"). So: no local paths, usernames or session paths (crate sources
  are cited by docs.rs, sheaf's by GitHub at its pinned commit); nothing from pdf-lint's unpublished plan (its
  backlog and audit IDs, its slices, its unpushed branches), whose assessment and the proposals to it stay
  local in `scratch/pdf-lint/`; facts about sheaf only as its own public plan has them.
- **S0, the scan devices, comes first** (Greg, 2026-11-08: "we want to add a s0 milestone for the scan
  devices. the first slice that i want to start a workflow for is the sane-dl"; taking the recommendations
  he was offered). So:
  - S0 takes all the device work, S3 and S4 as they were: Linux slices first (`sane-dl` first), then
    Windows; then S1 pages and S2 PDF. This answers ask 3 (the order: devices first, the option it didn't
    recommend) and ask 4 (device work starts before a scanner is at hand). Slices waiting on hardware don't
    hold S1 back (`milestones/s0-devices.md`).
  - The workspace (`Cargo.toml`, licenses, justfile, CI, gates, README), first built for S1 and stashed, is
    laid by S0 slice 1, reshaped around `sane-dl`; S1 slice 1 keeps the pixel crates, their MSRVs and the
    harness.
  - Slices run through a reusable workflow, `scan-slice` (`.claude/workflows/scan-slice.js`, local and
    untracked), modelled on pdf-lint's `family-slice`: plan, plan review, an implementing and a checking agent
    per step, code review, fixes; steered by a control file, restartable part-way.
  - The workflow's agents commit on the slice's branch as they go (the `Co-Authored-By` trailer, messages
    never naming sheaf); the branch isn't pushed and no pull request opened without Greg's word.
- **Asks 2, 6 and 7, taken as recommended as S0 slice 1 starts** (the asks' standing rule), each open to
  Greg's review:
  - ask 2, the cut: ten crates, `scan-device` (the model, and its backends behind features), `sane-dl`,
    `twain-dsm`, `scan-worker`, `scan-image`, `scan-cleanup`, `patch-codes`, `pdf-images`, `pdf-scan`, and
    `scan-fixtures` (never published). SANE's and TWAIN's bindings are useful alone and nobody has them;
    patch codes are the crate nobody else has; each layer's gate stays simple. Barcode reading stays with
    separation in sheaf. S0 slice 1 depends on its device half: `sane-dl` a crate of its own beside
    `scan-device`, so a SANE user takes no TWAIN types (recorded with the slice's step 1);
  - ask 6, the names: `scan-device`, `sane-dl`, `twain-dsm`, `scan-worker`, `scan-image`, `scan-cleanup`,
    `patch-codes`, `pdf-images`, `pdf-scan`, `scan-fixtures`;
  - ask 7, the standing rules: pdf-lint's, with the first narrowed: code, crate docs, crate READMEs and commit
    messages never name sheaf (the org's name in a URL aside); `docs/pm/` may, keeping nothing private.
    `just gates` checks the files.
