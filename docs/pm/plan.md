# pdf-scan: the plan

As of 2026-11-08. Building: S0 slice 1 has built the workspace and `sane-dl`, with `fake-libsane` for its
tests (`slices/s0-1-sane-dl.md`); the rest is planned.

pdf-scan is a workspace of crates for getting paper into PDF, which sheaf will depend on: scanners driven
through each system's own driver API, in a worker process; page images cleaned up, judged blank or not, and
read for patch codes; and scanned pages written to PDF with a searchable text layer, and read back out of
scanned PDFs without rendering them. The device and pixel crates know nothing of PDF. The PDF crates are
built on the family in pdf-lint's workspace (pdf-objects, pdf-structure, pdf-filters, pdf-content-streams,
pdf-semantics, pdf-color), which keeps pixels out of itself.

## Why now, and why as crates

- **M5 is deferred, but its plan is done.** sheaf's M5 left scanner intake and separator sheets (slices 4 to
  10) deferred "until there are scanners to test against" (decided 2026-10-23). The plan for them, proposals
  1 to 22 and asks 1 to 14, is in sheaf's `plan/m05-scan/`, taken as recommended at kickoff. It's the
  spec for the device crates here.
- **They're library candidates already.** sheaf's `plan/libraries.md`: "scanner access (M5's
  `sheaf-scanner`) and Kodak patch codes … are candidates again when M5 resumes": no maintained Rust crate
  does SANE or TWAIN (entries 5 to 8), and none reads patch codes (entry 12).
- **Library work may go first.** ROADMAP's ordering principle (decided 2026-10-23): between milestones, work
  "that can be published as a library of its own" can go first.
- **Half of it exists.** sheaf-image's cleanup, blank pages, patch codes and image files in and out are
  built, tested and in use (about 4,400 lines with their tests, `sheaf/from-sheaf.md`). Cutting them out needs no
  scanner and no PDF. The device side is planned to the proposal level and unbuilt. The PDF side is small
  in sheaf (images to pages, the glyphless text layer) and becomes possible without lopdf now that the
  family can write.

## The family

Eleven crates in three layers, nine published and two for tests (details and boundaries: `crates/`).

| Crate | Layer | What | Starts from |
|---|---|---|---|
| `scan-device` | devices | The device model (devices, what each can do, settings, pages, errors) and backends behind features: `fake`, `sane`, `twain`, `wia` | M5 proposals 5, 7 to 10, 22; unbuilt |
| `sane-dl` | devices | SANE loaded at run time: its own bindings from `sane.h`, a safe handle, cancel from another thread, frames assembled | M5 proposal 6; built in S0 slice 1 (0.1.0-alpha.1, unpublished) |
| `twain-dsm` | devices | TWAIN through the Data Source Manager loaded at run time: bindings, the state machine, a hidden window and its messages, 32- and 64-bit | M5 proposal 7; unbuilt |
| `scan-worker` | devices | Drivers out of process: the protocol, the worker's loop over any backend, the client with its watchdog | M5 proposal 4; unbuilt |
| `scan-image` | pages | A scanned page's image held as compactly as it allows (a JPEG as its bytes, black and white as CCITT G4), read from and written to image files | `sheaf_image::{io, encode}` |
| `scan-cleanup` | pages | Scans and photos of paper made clean pages: page finding, lighting, deskew, upright, Sauvola, despeckle, the Photo and Scanner profiles; and blank pages | `sheaf_image::{clean, blank}` |
| `patch-codes` | pages | Kodak patch codes read along any edge of a page image, and drawn in Kodak's layout | `sheaf_image::codes::patch` |
| `pdf-images` | PDF | An image XObject's or inline image's samples, decoded in its own color space, depth and `/Decode` | entry 33 (unbuilt) |
| `pdf-scan` | PDF | Scanned pages written to PDF (images as pages, an invisible text layer from words with boxes), and a scanned PDF's page images found and read back | `sheaf_doc::images`, the glyphless text layer |
| `scan-fixtures` | tests | Seeded synthetic pages, the M5 bench's degradations, codes drawn, device scripts. Never published | `sheaf_image::codes::fixture` |
| `fake-libsane` | tests | A stand-in for libsane, a `cdylib` that tests load by its path: scripted devices and faults, hostile values, SANE's calling rules checked. Never published, and no crate depends on it | Built in S0 slice 1 |

```
 devices: no pixel work, no PDF        pages: pixels only                PDF: on pdf-lint's family
 ──────────────────────────────        ──────────────────                ─────────────────────────
 sane-dl ───┐                          scan-image ◄─────────────────────  pdf-scan ──► pdf-structure, pdf-objects,
 twain-dsm ─┼─► scan-device ◄── scan-  scan-cleanup  (image, imageproc,      │         pdf-content-streams,
 windows ───┘   (model, backends,      worker         in-progress)           │         pdf-semantics, pdf-filters
                fake)                  patch-codes   (image)                 ▼
                                                                         pdf-images ──► pdf-objects, pdf-filters, pdf-color;
                                                                                        zune-jpeg, hayro-ccitt,
 scan-fixtures (dev-dependency of any)                                                  hayro-jbig2, hayro-jpeg2000
 fake-libsane (built and loaded by path by sane-dl's tests; nothing depends on it)
```

- **The layers don't reach into each other.** No device crate depends on a pixel or PDF crate; no pixel
  crate on a device or PDF crate; `pdf-scan` takes `scan-image`'s page type and nothing of the devices. A
  gate over `cargo tree` keeps each so, the shape of sheaf's `sheaf-image` gate (M5 proposal 21).
- **sheaf joins them.** Its intake pipeline (`sheaf_ops::intake`, staying in sheaf) takes a `scan-worker`
  client's pages, cleans them with `scan-cleanup`, holds them through `scan-image`, finds separators with
  `patch-codes` and its own barcode reader, recognizes them with `sheaf-ocr`, and lands them in its
  documents.

### Not in the family

- **OCR engines and the OCR job.** sheaf's libraries.md rules the engine trait out ("thin over the engines'
  own crates"), and the job renders through sheaf's `PageSource`. `pdf-scan` takes words as plain data
  (text, a box, a confidence), from any engine. The orientation and UVDoc models (`sheaf_ocr::{orient,
  rectify}`, on ort) stay with OCR; `scan-cleanup` takes a model through a trait, as `clean` takes one
  today.
- **Barcode reading** (`sheaf_image::codes`, 237 lines over rxing): thin, and its policy (formats limited,
  QR first, going on past a code the caller turns down) is separator finding's. Stays with separation in
  sheaf, as ask 2 was taken (`decisions.md`).
- **Separation:** finders and rules, sheaf's own `SHEAFSEP1` sheet, the review, bookmarks and splits. Core
  to sheaf (M5 ask 11, proposal 20), in sheaf's own format and ops.
- **The intake pipeline and its landings:** batches, undo groups, documents with no file yet.
- **eSCL:** not planned (entry 9); each system reaches network scanners already (M5 proposal 5).
- **macOS:** later, an `ica` backend of `scan-device` on `objc2-image-capture-core` (entry 8), when sheaf
  schedules it (M5 ask 4: a Pending entry).

## The milestones

Details in `milestones/`. Devices first, by the maintainer's order (2026-11-08), on fakes and SANE's own test backend
where no scanner is at hand; then the pixel crates cut from sheaf; then the PDF crates on the family.

| # | Milestone | Crates | Needs | State |
|---|---|---|---|---|
| S0 | **Scan devices:** SANE, the device model (provisional) and the worker on Linux; then TWAIN and WIA on Windows; the model frozen | `sane-dl`, `scan-device` (`fake`, `sane`, `twain`, `wia`), `scan-worker`, `twain-dsm`; `fake-libsane` for tests | Nothing unbuilt; by hand, scanners; for Windows, a Windows machine | Slice 1 running |
| S1 | **Pages:** the pixel crates cut from sheaf, sheaf's output held byte for byte | `scan-fixtures`, `scan-image`, `scan-cleanup`, `patch-codes` | Nothing unbuilt | Planned |
| S2 | **PDF:** scanned pages to PDF and back, on the family | `pdf-images`, `pdf-scan` | S1's `scan-image`; the family (alphas; its writers built, unpublished) | Planned |
| — | Later: macOS (`ica`), a second barcode engine, a faster cleanup if S0 slice 4's throughput asks for it | | | — |

**sheaf's side** is sheaf's own track, as pdf-lint's family has "sheaf's migration": sheaf swaps its copy
for each crate in a PR of its own, when the maintainer says, and its copy goes, as it went for pdf-lint's checker.
It's done in sheaf, never from here, and sheaf's plan doesn't point here (decisions.md, "Made here"):
`sheaf/from-sheaf.md` lists what each crate takes from sheaf, and `sheaf/swaps.md` what each swap replaces and
which of sheaf's entries it closes.

## Standing rules

pdf-lint's, which the maintainer set for that workspace, and sheaf's on licenses, taken as S0 slice 1 starts (ask 7,
as recommended; decisions.md, "Made here").

- **Code, crate docs, crate READMEs and commit messages never mention** sheaf, its milestones, the corpus
  or anything counted on it, or `scratch/`; the org's name in a URL (`github.com/sheaf-pdf/…`) aside. They
  say what the code does, not where it came from. `docs/pm/` may name them, keeping nothing private
  (decisions.md). `just gates` checks every file outside `docs/pm/`.
- **Nothing is published** (crates.io, tags, releases), pushed or merged without the maintainer's word. The PDF
  crates can't publish before the family crates they take are on crates.io (no path dependencies there):
  `pdf-images`' are, as alphas; `pdf-scan`'s pdf-structure and pdf-content-streams aren't yet
  (`pdf-lint/README.md`).
- **Nothing is reported upstream without the maintainer's OK** (SANE backends, the TWAIN DSM, rxing, hayro, image-rs).
- **Licenses:** every crate MIT OR Apache-2.0. GPL, LGPL and AGPL code (NAPS2, libinsane, simple-scan,
  sane-backends, jbig2dec, zbar) is studied and cited, never copied, translated or linked. An unmodified
  copyleft library loaded at run time is allowed (D17): libsane, `TWAINDSM.dll`. What ships beside an app
  (the DSM, its license text, a source offer) is the app's packaging, not a crate's.
- **The ISO standards** (and Mopria's eSCL specification) are never quoted or published.
- **sheaf isn't changed from here; anything in it may be copied out** (decided 2026-11-08). Its code (MIT OR
  Apache-2.0) is taken into the crates; its plan files are copied into `scratch/reference/sheaf/` and its open
  entries into `backlog/`, and they stay open in sheaf.
- **A crate cut from sheaf gives sheaf's output byte for byte** at the commit it was cut from (the
  harness), where sheaf's output was right; where it wasn't (`backlog/bugs.md`), the fix is the slice's, said.
- **Libraries:** typed errors with thiserror (sheaf's code uses anyhow); MSRV the family's, 1.89, unless a
  dependency needs more, measured per crate over its own graph (`sane-dl`'s in S0 slice 1, 1.89; the pixel
  crates' in S1 slice 1).
- **Hostile input:** caps with sane defaults, read before the work they cap: an image's dimensions before
  it's decoded, a worker's message sizes before they're read (a driver's bytes are untrusted, and so is a
  crashed worker's half message). Decoders are fuzzed.
- **Builds** as pdf-lint's: in its memory scope, no writes to cargo's home directory, `rg` not grep, commits with the
  `Co-Authored-By` trailer.
