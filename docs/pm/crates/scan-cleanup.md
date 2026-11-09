# scan-cleanup

Layer: Pages (`crates/README.md`). Paths are from `docs/pm/`.

- **What:** scans and photos of paper made into clean pages (`sheaf_image::clean`), and blank pages
  (`sheaf_image::blank`).
  - The steps, each a function of its own for a preview: find the page's corners, unwarp, clear the edges,
    flatten the light (shadows, color casts, white paper), quarter turns (word shape, and a model if the
    caller passes one), deskew (projection profile), Sauvola on integral images, despeckle. Analysis on a
    copy about 1,000 pixels across; pixel work at full resolution.
  - The profiles (M5 proposal 3): Photo (page finding and lighting on) and Scanner (both off).
  - Blank pages by pixels: margins ignored, paper level by a high percentile, ink only well below paper and
    in cells with more than a speck, "nearly blank" for one short line. Stronger than any open prior art
    found (NAPS2's: 1% of each edge ignored, blank under 0.15% non-white pixels).
  - A trait for the models the caller brings (orientation, UVDoc), so nothing here runs ONNX. Usable
    weights: UVDoc's (MIT), PaddleX's PP-LCNet document orientation (7 MB, in an Apache-2.0 repository; the
    weights' own license unchecked); not DocTr
    (non-commercial) or ocrs's (CC BY-SA). UVDoc needs `GridSample`, which rten 0.28 and tract-onnx
    0.23.10 have and candle-onnx 0.11.1 lacks: the caller's choice.
- **Fixed in the cut:** B39 (BUG-5), imageproc's `u32` Otsu overflowing past about 16.8 million pixels of one
  gray; a `u64` one of its own, as `patch-codes` has. imageproc's `adaptive_threshold` has the same kind of
  overflow (a `u32` integral image) and isn't called; it stays uncalled on full pages.
- **Port sources** (permissive; each port an output change, so a decision, said; FEAT-7): C Leptonica 1.87.0
  (BSD-2) for Postl's skew measure with a confidence (so a page of pictures says "no skew"), on a binary
  image reduced four times, with shear rotation; tile-based background normalization and clean-to-white;
  up/down detection for the 180° case word shape can't settle; page-foreground finding for auto-crop.
  Doxa (CC0) for Wolf, NICK or Bradley binarization if a measurement asks. The pure-Rust `leptonica` 0.5.0
  (BSD-2, written mainly by AI agents, its skew, background, binarization, dewarp and orientation code
  outside its own comparison with C) is a reading aid beside C Leptonica, not a dependency. unpaper,
  ScanTailor Advanced, gscan2pdf and NAPS2 are GPL: they set the bar (per-step switches, a deskew that
  refuses when its evidence disagrees), nothing more. Punch-hole removal and color dropout, if ever wanted,
  are FEAT-9 (patents checked first).
- **Boundary:** image and imageproc (0.27, rayon); `in-progress` for progress and cancellation (the maintainer's, on
  crates.io); `fast_image_resize` on new resampling paths, after a measurement. imageproc stays the base
  (MIT, 410 dependents, maintained); its `rayon` feature covers maps, filters and warps, not median,
  morphology, labelling or integral images.
- **Tested by:** sheaf's cleanup and blank tests, moved; the harness, byte for byte on sheaf's fixtures and
  a sample of the corpus's scans (entry 64's byte-identical rule for cleanup). Measured, outside the build:
  Doxa's BinBench (DIBCO's FM, pseudo-FM, PSNR and DRD) for binarization, DISEC'13's metrics on synthetic
  skew (TEST-5). Blank pages, page finding in photos and orientation have no public benchmark: our own
  labelled pages.
- **Open:** speed. M5 slice 6's gate needs 2.3 pages a second on 4 cores at 300 dpi, about 1.74 core-seconds
  a page; slice 1 measured about 0.8 s a Letter page on one thread under Scanner, which meets it by pages in
  parallel alone, and 3.2 s under Photo, which needs a page 1.4 to 2 times faster. Pages cleaned in
  parallel is the intake's; a faster step is this crate's. Profile first (TEST-5); the likely hot spots, read
  from the code: O(r) max and median filters (van Herk/Gil-Werman is O(1) a pixel), three upsamples of the
  background, single-threaded Sauvola and labelling, full-resolution bilinear rotation of RGB pages. Never
  at the cost of the output (correctness first; a change of output is a decision, said).
