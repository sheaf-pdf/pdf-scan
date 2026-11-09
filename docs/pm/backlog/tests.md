# Tests: checks still to build, spikes and measurements

IDs TEST-1 to TEST-6; the next new one is **TEST-7**. Format and rules: `README.md`.

From the prior-art survey's "What is still unverified" (`research/prior-art/README.md`,
2026-11-08): what settles each is an experiment, not more reading.

## For S1 (pages)

- **TEST-4** · scan-image, pdf-images · S1 · open — **The hayro codecs fuzzed through our adapters, and run on their conformance sets.** hayro-ccitt, hayro-jbig2 and hayro-jpeg2000 come from one maintainer, and none is on OSS-Fuzz or has independent conformance results. Fuzz each through the adapter `scan-image` and `pdf-images` call it by; run T.88 Annex K's JBIG2 cases and openjpeg-data; count the corpus's 12-bit and arithmetic-coded JPEGs among the 464 DCT scans (zune-jpeg refuses them); measure an all-safe build's speed with SIMD off on 300 dpi Letter pages. Findings go upstream as UP-5. *(survey, image codecs; pdf images and writing)*
- **TEST-6** · scan-fixtures · S1 · open — **Outside images' licenses, before any is vendored.** zxing-cpp's sample and `falsepositives` sets (the cheapest barcode false-read gate), BarBeR's images, and libtiff's sample images (`fax2d.tif`, `g3test.tif`) for BUG-2. Read each license; vendor only what allows it, else fetch at test time or don't use. *(survey, patch codes and barcodes; image codecs)*

## For S2 (PDF)

- **TEST-1** · pdf-scan · S2 · open — **The text layer's viewer matrix.** Early in S2, before the layer is called done: the layer in PDFium, pdf.js and Poppler (`pdftotext` run as a process), and Preview by hand, for right-to-left words, CJK, combining marks, text beyond the BMP, vertical lines, and a word across a `BT`; selection boxes and copy and paste as well as extraction. It settles Q-2 and Q-3 and checks BUG-6. Also: the cause of OCRmyPDF 17.13's "lost characters" in PDFium-based viewers with glyphless fonts (undescribed in its notes), and whether it touches a font that maps every CID to glyph 1; whether sheaf's separately scaled space avoids OCRmyPDF 17.4's selection running past the words; how pdf.js treats render mode 3 in extraction. veraPDF for `.notdef` (its license unread). *(survey, pdf images and writing; OCRmyPDF 17.x release notes)*

## For S0 (scan devices)

- **TEST-2** · sane-dl · S0 · open — **SANE in CI: the `test` and `pnm` backends, and the bindings against the header.** Check that Ubuntu runners' `libsane1` includes `pnm` (Debian's build enables it); whether the `test` backend's hand-scanner mode combines with its feeder to give `lines = -1` from a feeder (else the fake does it); a test that the hand-written bindings and status table match the installed `sane.h` (sanetui's idea, Apache-2.0). *(survey, SANE and network scanning)*

- **TEST-3** · twain-dsm · S0 · open — **TWAIN's sample source on a hosted runner.** Whether `Twain_DS_sample01` (BSD-3) installs silently on `windows-latest` (`msiexec /qn`), or runs from yushulx/virtual-scanner's portable zip, or works with the DSM and the source copied into `twain_32` and `twain_64` (if the runner lets us write `C:\Windows\twain_*`); whether it scans with a hidden window in both bitnesses; `CAP_XFERCOUNT` from its environment variable sets the page count. *(survey, TWAIN, WIA and ICA)*

## Later

- **TEST-5** · scan-cleanup · later · open — **Cleanup measured before it's sped up or changed.** A profile of where Scanner's 0.8 s and Photo's 3.2 s a page go, before any speed slice (`crates/scan-cleanup.md`'s reading of the code is a guess); Leptonica's skew speed on today's CPUs if FEAT-7 ports it (its published figures are from a 1 GHz P3); binarization scored by Doxa's BinBench (DIBCO's FM, pseudo-FM, PSNR, DRD) and skew by DISEC'13's metrics, run outside the build; the terms for vendoring DIBCO, DISEC'13 and DocUNet data (else fetched, not vendored). *(survey, page cleanup)*
