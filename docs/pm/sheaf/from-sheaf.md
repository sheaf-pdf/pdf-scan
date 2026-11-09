# What sheaf has for the crates

The code each crate starts from and the plan files that are its spec, read at sheaf `3232a35`
(2026-10-28). Paths are from sheaf's root. sheaf's code is MIT OR Apache-2.0: taken as it is, and changed
here. Line counts include doc comments.

## Code

| Crate | sheaf file | Lines | What it is | What changes in the cut |
|---|---|---|---|---|
| `scan-image` | `crates/sheaf-image/src/io.rs` | 960 | Image files in: decoded, upright, with their resolution; a JPEG kept as its bytes (SOF and EXIF read), black and white held as G4, a TIFF's pages one at a time | anyhow to thiserror; B25 (G3 and Modified Huffman through hayro-ccitt), B15 (TIFF kinds refused), B14 (Orientation in `tiff_pages`) |
| | `crates/sheaf-image/src/encode.rs` | 220 | PNG, JPEG and multi-page TIFF out with their resolution; CCITT G4 through `fax` 0.2 | B24 (widths past 65,535): `fax` 0.3 or a refusal |
| `scan-cleanup` | `crates/sheaf-image/src/clean.rs`, `clean/tests.rs` | 835 + 234 | The cleanup pipeline and its steps, the profiles; sheaf's own deskew, Sauvola and despeckle over imageproc | A trait for the caller's models; anyhow to thiserror; B39 (Otsu in `u64`); nothing else (byte for byte) |
| | `crates/sheaf-image/src/blank.rs` | 275 | Blank by pixels | As is |
| `patch-codes` | `crates/sheaf-image/src/codes/patch.rs`, `patch/tests.rs` | 362 + 344 | Kodak patch codes found along each edge, on `image` alone | The drawing of Kodak's layout joins it, from `codes::fixture` |
| `scan-fixtures` | `crates/sheaf-image/src/codes/fixture.rs` | 840 | Seeded pages, `Degradation`, codes drawn through rxing, patch sheets drawn | Its own crate, never published |
| `pdf-scan` | `crates/sheaf-doc/src/images.rs` | 252 | Images as PDF pages through sheaf's serializer (lopdf): DCT as is with `/Rotate`, G4, Flate; `Fit` | Written through pdf-structure and pdf-content-streams; a page at a time |
| | `crates/sheaf-doc/src/content/font/glyphless.rs` | 126 | The glyphless Type0 font, Tesseract's approach | As is |
| | `crates/sheaf-doc/src/text_layer.rs`, `text_layer/fixture.rs` | 489 + 164 | Words into lines in PageSpace, and the invisible layer drawn through sheaf's `ContentWriter` | The writing only, without sheaf's model (PageSpace, blocks, `Private`): words in, operators out |

Stays in sheaf (named here so nobody cuts it by mistake):

| sheaf file | Lines | Why it stays |
|---|---|---|
| `crates/sheaf-image/src/codes.rs`, `codes/tests.rs` | 237 + 420 | The barcode reader's policy is separator finding's (ask 2, as taken: `decisions.md`) |
| `crates/sheaf-image/src/raster.rs` | 162 | Rendered pages as pixels: sheaf-render's |
| `crates/sheaf-ops/src/intake.rs` | 429 | The pipeline, its batches and landings into sheaf's documents |
| `crates/sheaf-ops/src/scan.rs`, `blank.rs` | 348, 279 | `sheaf scan`, and blank pages by a page's content and pixels together |
| `crates/sheaf-ocr/src/*` | 2,037 | OCR (libraries.md: the engine trait isn't a candidate); `orient.rs` and `rectify.rs` are the models `scan-cleanup`'s trait takes |

## Plan files that are the spec

Each is copied as at `3232a35` in `scratch/reference/sheaf/`, under its path below `plan/`
(`scratch/reference/README.md`).

| Crate | sheaf's plan |
|---|---|
| `scan-device` | M5 proposals 5 (which API where), 7 (TWAIN's settings and errors), 8 (WIA), 9 (the model), 10 (controls and errors), 22 (testing without a scanner); `research/scanners.md`; build-or-adopt 7, 8 |
| `sane-dl` | M5 proposal 6; `research/scanners.md`, "SANE" and "SANE's test backend, here"; build-or-adopt 5 |
| `twain-dsm` | M5 proposal 7, asks 3 and 12; `research/scanners.md`, "TWAIN"; build-or-adopt 6; D17 |
| `scan-worker` | M5 proposal 4, M5 ask 5; M5 proposal 21 (the gates, packaging) |
| `scan-image` | build-or-adopt 15, 16, 38; B14, B15, B17, B24, B25; `complete/m05-scan/s01-intake-pipeline.md` (G4 held, the memory gate) |
| `scan-cleanup` | M5 proposal 3; build-or-adopt 14, 64; B39; `complete/cleanup-cli.md`; slice 6's speed gate |
| `patch-codes` | M5 proposal 14; `research/separators.md`, `research/reading-writing-codes.md`; build-or-adopt 12; `complete/m05-scan/s03-reading-codes.md` |
| `pdf-images` | build-or-adopt 33 to 38; M11 slice 6 |
| `pdf-scan` | `complete/m05-ocr.md` (the layer's design); B18, B30, F10; M10.5 §3.4 and §11.4, and its slice 18 (OCR as content); M5 `in-place.md` (memory sources); `risks.md` (decoding a scan's image instead of drawing it) |

## Prior art

Read for what it learned, never copied where it's copyleft (plan.md, "Standing rules"). The whole survey,
need by need, is `research/prior-art/README.md`; these are the sources a cut or a port
takes from most.

| What | License | For |
|---|---|---|
| NAPS2 | GPL-2.0+, its SDK LGPL-2.1+ | Study only: every driver API in workers, its STA thread and window for TWAIN, cancel by state, tiles, a one-backend `dll.conf` |
| NAPS2.Wia | MIT | WIA's calls (its error table repeats Learn's misprints) |
| printscan-windows | MIT OR Apache-2.0 | WIA rules measured on Windows' WSD driver, ported with credit |
| `TWAIN.cs` (twain-cs) | MIT | TWAIN's definitions, ported for `twain-dsm` |
| RSWebTWAIN | MIT OR Apache-2.0 | The design of its typestate machine and 32-bit JSON-lines sidecar, not its code |
| NTwain | MIT | Capability container encoding |
| libinsane | LGPL | Study only: SANE in its own process because some backends corrupt memory |
| simple-scan | GPL | Study only: SANE's feeder and duplex option names across backends |
| sane-airscan | GPL (with SANE's exception) | Study only: eSCL quirks, if eSCL is ever wanted |
| pytwain, Saraff.Twain | GPL-2.0, LGPL-3.0+ | Study only |
| Leptonica (C, 1.87.0) | BSD-2 | Skew with a confidence, background normalization, up/down, page foreground, for `scan-cleanup` |
| Doxa | CC0 | Binarization methods and BinBench |
| Augraphy | MIT | Degradation effects for `scan-fixtures` |
| hayro-syntax `filter/{dct,ccitt,jbig2,jpx}.rs`, hayro-interpret | MIT OR Apache-2.0 | PDF's image-filter glue and sample unpacking, ported for `pdf-images` |
| pdf.js | Apache-2.0 | `/Matte`, soft-mask fallbacks, `SMaskInData` |
| PDFBox | Apache-2.0 | The shape of a native-sample API (`getRawRaster`); an optional oracle run as a process |
| pdf_oxide | MIT OR Apache-2.0 | Thresholds for a scanned page and an existing OCR layer |
| Tesseract `pdfrenderer.cpp`, `pdf.ttf` | Apache-2.0 | The glyphless text layer and its page-at-a-time layout |
| OCRmyPDF | MPL-2.0 | Study only: its 17.x release notes on what breaks in invisible layers; facts about other tools' layers |
| krilla | MIT OR Apache-2.0 | `/ID` from a content hash; CMYK and YCCK `/Decode`; PDF/A clause tables later |
