# Page cleanup, blank pages and synthetic degraded pages: prior art for `scan-cleanup` and `scan-fixtures`

As of 2026-11-08. Versions and dates come from the crates.io API, PyPI JSON or GitHub release pages fetched that day unless a finding says otherwise. "Local" sources are files read on this machine: crate sources in cargo's local registry cache, crates downloaded (not built) to a temporary directory, and sheaf's code (read only). The GitHub REST API was rate-limited (shared IP), so GitHub facts come from repository web pages and raw files instead. Stars and commit counts are as the page showed them on the day.

What the crates need (from `docs/pm/crates.md`): `scan-cleanup` covers page corners and unwarping, edge clearing, light flattening, quarter turns (word shape, plus a model the caller passes in), projection-profile deskew, Sauvola on integral images, despeckle, and blank pages. It has two profiles (Photo and Scanner), a trait for models so the crate itself runs no ONNX, and the B39 Otsu fix. Its open question is speed. `scan-fixtures` holds seeded synthetic pages and the `Degradation` list. The existing code is `clean.rs` (835 lines) plus `blank.rs` (275), with `codes/fixture.rs` (840) for fixtures ([local: sheaf-image/src](https://github.com/sheaf-pdf/sheaf/blob/3232a35/crates/sheaf-image/src/clean.rs)).

## Q1. Rust crates for each cleanup step; is imageproc still the base; faster alternatives

### Takeaway
No Rust crate does document cleanup better than sheaf's own code. Two crates the 2026-10-23 audit missed deserve attention:
- **`leptonica` 0.5.0**, a pure-Rust port of Leptonica written by AI agents, BSD-2, with no `unsafe`. It is too young and too thinly verified to depend on, but its source is a permissive cross-check for porting.
- **`seiza-imgproc`**, Apache-2.0, with an f64 Otsu that is bit-exact with OpenCV.

`imageproc` 0.27 is still the right base: maintained, MIT, about 410 reverse dependencies. It has known numeric weak spots (an Otsu sum in u32, u32 integral images inside `adaptive_threshold`, a canny hysteresis bug that is open but unreachable from sheaf's call) and rayon only in some operations. Faster drop-ins exist for resampling (`fast_image_resize`) and morphology (`fast_morphology`), not for the document-specific steps.

### Cited Findings

**imageproc, the base**
- `imageproc` **0.27.0, released 2026-06-02**. MIT, MSRV 1.87, edition 2024. 14.96 M downloads (3.07 M recent), 410 reverse dependencies, 36 versions since 2016 — [crates.io](https://crates.io/crates/imageproc). The two releases before it were 0.26.0 (2026-01-01) and 0.26.1 (2026-02-28) — [CHANGELOG](https://github.com/image-rs/imageproc/blob/master/CHANGELOG.md).
- Maintenance: 13 open PRs, the newest opened 2026-10-26, and reviewers still ask for tests (cospectrum on #746, 2025-12-20) — [PR list](https://github.com/image-rs/imageproc/pulls), [PR #746](https://github.com/image-rs/imageproc/pull/746). An open PR (#734, 2025-10-31) adds a Gaussian type to `adaptive_threshold`. Nothing in the repository adds Sauvola or deskew — [PR list](https://github.com/image-rs/imageproc/pulls); [CHANGELOG](https://github.com/image-rs/imageproc/blob/master/CHANGELOG.md).
- What imageproc 0.27 has for thresholds:
  - `adaptive_threshold(image, block_radius, delta)` (a mean minus delta, Bradley-like);
  - `otsu_level`;
  - `kapur_level` (added in 0.26);
  - `threshold` with a `ThresholdType`.

  It has no Sauvola, Niblack, Wolf or deskew. Its Hough is `hough::detect_lines` — [local: imageproc-0.27.0/src/contrast.rs, hough.rs](https://docs.rs/crate/imageproc/0.27.0/source/src/contrast.rs).
- **The Otsu overflow (B39) is confirmed in the 0.27.0 source.** `total_pixel_sum` folds `(t as u32 * h) as f64`, and `background_pixel_sum += (threshold as u32 * hist_count) as f64`. Both products are formed in u32 before the cast, so 255 × count overflows once one gray level has more than 16,843,009 pixels. `total_weight = width * height` is u32 as well — [local: contrast.rs:52-104](https://docs.rs/crate/imageproc/0.27.0/source/src/contrast.rs).
  - A search of the repository's issues for "otsu" finds no report of this overflow.
  - PR #740, "Better Otsu's method" (2025-11-19), concerned float precision. It was closed without merging on 2025-12-30 — [issue search](https://github.com/image-rs/imageproc/issues?q=otsu), [PR #740](https://github.com/image-rs/imageproc/pull/740).
- **The same overflow class sits in `adaptive_threshold`.** It builds `integral_image::<_, u32>`, whose sums use plain `+=`, so a page whose total brightness passes 2³² (about 16.8 M white pixels, for instance a 600 dpi Letter page) overflows. In debug builds that panics. In release it wraps, and window differences stay correct modulo 2³². `integral_squared_image::<_, u32>` overflows after about 66 k white pixels if a caller picks u32 — [local: contrast.rs:19-47, integral_image.rs:98-131](https://docs.rs/crate/imageproc/0.27.0/source/src/integral_image.rs).
- **Canny hysteresis bug.** Open PR #746 says `hysteresis` "can underflow … or go out of bounds in `get_pixel`" when an edge reaches the image border — [PR #746](https://github.com/image-rs/imageproc/pull/746). In 0.27.0, non-maximum suppression writes 0 on the 1-pixel border, so `canny()` reaches the bug only when `low_threshold <= 0` — [local: edges.rs:30-150](https://docs.rs/crate/imageproc/0.27.0/source/src/edges.rs). sheaf calls `canny(&blurred, 20.0, 60.0)` — [local: clean.rs:285](https://github.com/sheaf-pdf/sheaf/blob/3232a35/crates/sheaf-image/src/clean.rs).
- **What the `rayon` feature covers.** It parallelizes:
  - `map_pixels*` and `map_subpixels*`;
  - the filter family (`filter_map_parallel` and friends);
  - `warp_into` (`par_chunks_mut` over rows, so `rotate_about_center` and projective warps too);
  - `equalize_histogram`.

  It does not parallelize `median_filter`, morphology, `connected_components`, integral images or `canny` — [local: grep of imageproc-0.27.0/src](https://docs.rs/crate/imageproc/0.27.0/source/src/geometric_transformations.rs). The 0.26.0 changelog lists "Parallel Pixel Maps" and `filter_map_parallel` — [CHANGELOG](https://github.com/image-rs/imageproc/blob/master/CHANGELOG.md).
- `unsafe` in imageproc 0.27.0: 199 occurrences. The most are in corners.rs (34), filter/mod.rs (22), median.rs (21), distance_transform.rs (18) and region_labelling.rs (17). Most carry a "JUSTIFICATION" comment for unchecked pixel access — [local](https://docs.rs/crate/imageproc/0.27.0/source/src/integral_image.rs).
- `imageproc::noise` seeds `rand::rngs::StdRng` (rand 0.10) — [local: noise.rs](https://docs.rs/crate/imageproc/0.27.0/source/src/noise.rs). rand's documentation says StdRng is "Non-portable: any future library version may replace the algorithm … even with a fixed seed, output is not portable" — [docs.rs rand 0.10.3](https://docs.rs/rand/latest/rand/rngs/struct.StdRng.html).

**Pure-Rust Leptonica port, missed by the audit**
- **`leptonica` 0.5.0, released 2026-07-16**: "Rust port of Leptonica". BSD-2-Clause, MSRV 1.88, first published 2026-02-23. 877 downloads, 1 reverse dependency — [crates.io](https://crates.io/crates/leptonica).
  - Size: about 154 k lines of Rust in 187 files, with 0 `unsafe` and about 4,500 `#[test]`s.
  - No rayon.
  - Dependencies are pure-Rust codecs (png, tiff, jpeg-decoder, jpeg-encoder, image-webp, gif, hayro-jpeg2000, pdf-writer, miniz_oxide) — [local: leptonica-0.5.0](https://docs.rs/crate/leptonica/0.5.0/source/README.md).
- What it has (local source):
  - `recog::skew::find_skew`, `find_skew_sweep_and_search_score_pivot` and `deskew` (Postl differential square sums by shear; defaults ±7°, a 1° sweep, a 0.01° binary search, reductions 4 and 2);
  - `filter::adaptmap::background_norm`, `background_norm_morph`, `background_norm_flex` and `clean_background_to_white`;
  - `color::threshold::sauvola_threshold` (u64 integral images, f64 variance), `sauvola_binarize_tiled`, `otsu_adaptive_threshold` and `sauvola_on_contrast_norm`;
  - `recog::flipdetect::orient_detect`, `up_down_detect` and `mirror_detect` (hit-miss on ascenders and descenders);
  - `recog::dewarp` (`dewarp_single_page_*`);
  - `recog::pageseg::find_page_foreground`, `segment_regions` and `estimate_background`;
  - `region::conncomp`.

  [local: src/recog/skew.rs, src/filter/adaptmap.rs, src/color/threshold.rs, src/recog/flipdetect.rs](https://docs.rs/crate/leptonica/0.5.0/source/src/recog/skew.rs)
- Quality evidence, against it:
  - The GitHub repository shows 1 star, 0 forks, 2,290 commits and no GitHub releases.
  - Its README says "The porting work is carried out primarily by AI coding agents" (Claude Code) — [GitHub](https://github.com/tagawa0525/leptonica-rs).
  - Pixel-level C compatibility, as of 2026-08-24: Ok 517, Mismatch 29, Unmapped 361, Excluded 119. At the 0.5.0 snapshot it was Ok 44, Mismatch 29, Unmapped 500. The covered areas are mostly morphology, scaling, dither and distance transform. **Skew, adaptmap, binarize, dewarp and flipdetect are not in the compatibility report** — [c-compat-status](https://github.com/tagawa0525/leptonica-rs/blob/main/docs/porting/c-compat-status.md).

**Other new or missed Rust crates**
- **`aktujo-scan` 0.4.0, released 2026-11-07** (first published 2026-11-06). MIT, MSRV 1.99, 100 downloads — [crates.io](https://crates.io/crates/aktujo-scan). It is about 1,440 lines with 0 `unsafe` and 13 tests.
  - Steps: corner finding (Otsu mask, plus edges "closed in" from the border, each side fitted to the outline), unwarping, quarter turns, crop, projection-profile deskew (±10° in 0.1° steps, on a copy 1,000 px wide), gray, and black and white.
  - Its black and white is a mean-of-window threshold at 0.88 (Bradley's t = 12%), not Sauvola. No rayon. It renders PDFs through hayro — [local: aktujo-scan-0.4.0/src](https://docs.rs/crate/aktujo-scan/0.4.0/source/src/lib.rs).
- **`seiza-imgproc` 0.3.2, released 2026-08-14.** Apache-2.0, MSRV 1.89, 8,684 downloads, 3 reverse dependencies. It is built for astrophotography — [crates.io](https://crates.io/crates/seiza-imgproc).
  - Its README says the Gaussian, Canny, Otsu, morphology, contour and domain-transform paths match OpenCV 4.13 bit for bit, locked by golden tests.
  - Its Otsu ports OpenCV's `getThreshVal_Otsu_8u` in f64, so it cannot overflow.
  - It is about 2,950 lines, with 6 `unsafe` and 50 tests, and depends on rayon. The API works on slices — [local: seiza-imgproc-0.3.2](https://docs.rs/crate/seiza-imgproc/0.3.2/source/src/threshold.rs).
- **`kornia-imgproc` 0.2.0, released 2026-09-26.** Apache-2.0, MSRV 1.89, 50.7 k downloads, 5 reverse dependencies — [crates.io](https://crates.io/crates/kornia-imgproc).
  - About 75 k lines with 370 `unsafe`. It has CLAHE, canny, contours, connected components, distance transform, perspective and affine warps, and SIMD resize.
  - Its thresholds are fixed-value only: no Otsu, adaptive or Sauvola.
  - Optional `opencv` and `cudarc` dependencies. It uses its own image types, not `image` buffers — [local](https://docs.rs/crate/kornia-imgproc/0.2.0/source/src/threshold.rs).
  - The kornia-rs repository has 751 stars. Optional native features: v4l (clang), turbojpeg (nasm), gstreamer and CUDA — [GitHub](https://github.com/kornia/kornia-rs).
- `zune-imageprocs` 0.5.1 (2026-01-24; MIT OR Apache-2.0 OR Zlib; 21 `unsafe`): blur, median, fixed threshold, rotate, resize, sobel and contrast stretch. No morphology or labelling — [crates.io](https://crates.io/crates/zune-imageprocs); [local](https://docs.rs/crate/zune-imageprocs/0.5.1/source/src/threshold.rs).
- `yscv-imgproc` 0.2.0 (2026-10-01; MIT OR Apache-2.0; MSRV 1.94; 480 downloads; 817 `unsafe`): Otsu and mean or Gaussian adaptive thresholds — [crates.io](https://crates.io/crates/yscv-imgproc); [local](https://docs.rs/crate/yscv-imgproc/0.2.0/source/src/ops/threshold.rs).
- `fast_morphology` 0.3.2 (2025-10-20; BSD-3-Clause OR Apache-2.0; awxkee; 43 k downloads; 0 reverse dependencies; 68 `unsafe`; rayon). Erosion, dilation, opening, closing, top-hat, black-hat and gradient with arbitrary structuring elements. Its README claims it is "In most cases … faster than OpenCV" — [crates.io](https://crates.io/crates/fast_morphology); [local README](https://docs.rs/crate/fast_morphology/0.3.2/source/README.md).
- `fast_image_resize` 6.1.0 (2026-07-21; MIT OR Apache-2.0; MSRV 1.87; 24.0 M downloads; 149 reverse dependencies) — [crates.io](https://crates.io/crates/fast_image_resize).
  - README benchmark, single thread, 4928×3279 → 852×567. L8 bilinear: `image` 57.97 ms, fir Rust 5.48 ms, SSE4.1 2.25 ms, AVX2 1.81 ms. RGB8 Lanczos3: 173.98 ms against 13.26 ms with AVX2.
  - rayon is optional. Pixel types U8 to U16x4 and F32 — [GitHub](https://github.com/Cykooz/fast_image_resize).
- `pic-scale` 0.7.12 (2026-09-10; BSD-3-Clause OR Apache-2.0; MSRV 1.89; 3.48 M downloads) — [crates.io](https://crates.io/crates/pic-scale).
- `photon-rs` 0.3.3 (2025-05-10; Apache-2.0): only its metadata was checked. Searches for binarization, Sauvola and deskew did not surface it — [crates.io](https://crates.io/crates/photon-rs).
- Crates with unsuitable licenses or shape:
  - `superbook-pdf` 0.1.0 is AGPL-3.0 — [crates.io](https://crates.io/crates/superbook-pdf).
  - `photocraft` 0.1.2 is GPL-3.0-or-later — [crates.io](https://crates.io/crates/photocraft).
  - `bgustreadimg` 0.3.0 has a "non-standard" license and depends on ort, napi and pyo3 — [crates.io](https://crates.io/crates/bgustreadimg).
  - `ocr` 0.1.3 (Apache-2.0) is a 32 k-line application with Sauvola, Otsu, a dewarp module, and cudarc, ocl, axum and tokio dependencies — [crates.io](https://crates.io/crates/ocr).
  - `justpdf-special` 0.1.2 (MIT OR Apache-2.0; 81 downloads) has a small projection-profile deskew with a confidence value — [crates.io](https://crates.io/crates/justpdf-special).
  - `deepocr-core` 0.1.0 (MIT OR Apache-2.0) has a deskew and a turn-upright step for ocrs — [crates.io](https://crates.io/crates/deepocr-core).
- Exact-name lookups on crates.io returned 404 (no crate) for `binarize`, `sauvola`, `deskew`, `otsu`, `unpaper`, `dewarp`, `docscan`, `scan-cleanup` and `scan-fixtures`. `doxa` exists but is an unrelated OpenAPI crate — [crates.io/crates/doxa](https://crates.io/crates/doxa).

**Relevant facts about the existing code (local)**
- `max_filter` reads every element with `get_pixel`, O(r) per pixel and per pass. `background` runs `median_filter` with radius 2r on the roughly 1,000 px copy. `flatten_light` resizes three channel backgrounds back to full size with `image::imageops::resize` (Triangle) — [local: clean.rs:548-618](https://github.com/sheaf-pdf/sheaf/blob/3232a35/crates/sheaf-image/src/clean.rs).
- `sauvola` uses u64 integral images, one thread, and marks a pixel as ink when it is at or below the threshold (`<=`) — [local: clean.rs:675-713](https://github.com/sheaf-pdf/sheaf/blob/3232a35/crates/sheaf-image/src/clean.rs).
- `skew_angle` scores squared row bins over sampled ink points (at most about 60 k): ±15° in 1° steps, then ±1° in 0.1° steps — [local: clean.rs:490-531](https://github.com/sheaf-pdf/sheaf/blob/3232a35/crates/sheaf-image/src/clean.rs).
- `find_page` and `ink_mask` call imageproc's `otsu_level`, which is the B39 path — [local: clean.rs:289, 466](https://github.com/sheaf-pdf/sheaf/blob/3232a35/crates/sheaf-image/src/clean.rs).

**Checking the 2026-10-23 audit**
- Entry 14: "imageproc 0.27.0 (2026-06-02; MIT) … no Sauvola and no deskew" is still true — [local source](https://docs.rs/crate/imageproc/0.27.0/source/src/contrast.rs). "Searches … found only small apps and one-off crates" is **stale or incomplete**:
  - `leptonica` 0.5.0 (a pure-Rust port, out since 2026-02, with 0.5.0 on 2026-07-16) was missed — [crates.io](https://crates.io/crates/leptonica);
  - `seiza-imgproc` (from 2026-07-22) too — [crates.io](https://crates.io/crates/seiza-imgproc);
  - `aktujo-scan` came out after the audit — [crates.io](https://crates.io/crates/aktujo-scan).
  - "leptonica-sys 0.4.9 (2024)" is right: 2024-11-26, MIT — [crates.io](https://crates.io/crates/leptonica-sys).
- Entry 64: fast_image_resize 6.1.0 (2026-07-21) and pic-scale 0.7.12 (2026-09-10) are right. Downloads have since grown to 24.0 M — [crates.io](https://crates.io/crates/fast_image_resize).

### Inferences
- **Speed:** the Scanner target is reachable by page parallelism alone. 2.3 pages a second on 4 cores allows about 1.74 core-seconds per page. Scanner's measured 0.8 s on one thread fits with room to spare, if four pages' buffers fit in memory. Photo's 3.2 s does not (about 1.25 pages a second on 4 cores), so Photo needs roughly a 1.4–2× faster page.
  - Likely hot spots, judging from the code rather than a profile: the naive `max_filter` and `median_filter` in `flatten_light` and `clear_edges`; three full-size Triangle upsamples of the background; one-thread Sauvola and `connected_components` at full size; full-size bilinear rotation of RGB.
  - Known remedies, all permissive:
    - van Herk/Gil-Werman max and min filters, O(1) per pixel (cited in Doxa's README, Q3);
    - Leptonica's tile-based background map instead of a median;
    - skew measured on a 4× rank-reduced binary image (Leptonica, Q2);
    - rayon over rows for Sauvola;
    - `fast_image_resize` for the background upsample.
  - The byte-identical rule in crates.md means each change is a decision about output.
- imageproc stays the base: no alternative has its breadth (contours, polygon approximation, projective warps, labelling) with comparable adoption and a permissive license. kornia-imgproc has no thresholds worth having and its own image types; seiza is narrow and has one author.
- leptonica-rs is best used as a reading aid next to C Leptonica, which is BSD-2 and so may be ported either way. Depending on it would bring 154 k AI-written lines, single-maintainer risk, no parallelism, and no C-compatibility evidence for exactly the functions that matter here.

#### Verdicts by need (Rust side; ML and fixtures are in Q5 and Q7)
| Need | Verdict | Reason |
|---|---|---|
| Base primitives (blur, Canny, contours, warps, labelling, morphology) | **Use directly: imageproc 0.27** (and **contribute upstream**: Otsu in u64 or f64, `adaptive_threshold` integral in u64) | Maintained, MIT, 410 dependents. No issue has been filed for the overflow, so a fix is likely welcome. Keep the crate's own u64 Otsu (B39) meanwhile. |
| Otsu | **Build our own** (a u64 or f64 histogram Otsu, about 30 lines), and contribute it upstream | seiza's f64 port of OpenCV (Apache-2.0) is a reference for exact OpenCV tie-breaking if wanted. |
| Deskew | **Build our own** (keep), **port** Leptonica's ideas | No crate is better. Port Postl's differential sums on a reduced binary image, a confidence score, and shear rotation for small angles (Q2). |
| Sauvola and other binarization | **Build our own** (keep). **Port** Wolf, NICK or Bradley from Doxa (CC0) only if measurement asks for them | No Rust crate has a Sauvola better than sheaf's. Doxa is the reference set (Q3). |
| Despeckle and morphology | **Use directly** imageproc (`connected_components`, `dilate`); consider **fast_morphology** only after a measurement | fast_morphology has 0 dependents and 68 `unsafe`, but SIMD and rayon. |
| Max and min filters (background) | **Build our own** (van Herk/Gil-Werman) | O(1) per pixel. Not in imageproc. |
| Border and black-edge removal, auto-crop | **Build our own** (keep `clear_edges`), **port** Leptonica's `pixFindPageForeground` and border-connected-component removal as candidates | unpaper and ScanTailor are study only (Q4). |
| Punch-hole removal | **Build our own** | No permissive or Rust implementation exists. Commercial SDKs and patents describe it (Q4 and Holes). |
| Page corners and perspective | **Build our own** (keep). Study aktujo-scan's two-mask approach (MIT, may be ported) | aktujo-scan is two days old with 13 tests. It does what sheaf already does, in a different way. |
| Illumination flattening | **Build our own** (keep), **port** Leptonica `pixBackgroundNorm` and `pixCleanBackgroundToWhite` if they measure faster | Q2. |
| Blank pages | **Build our own** (keep) | Better than the open-source prior art found (Q4). |
| Color dropout | **Build our own** | No open-source implementation found (Holes). |
| Resampling | **Use directly: fast_image_resize** for new paths, measured | As entry 64 recommends. Keep the current calls under the byte-identical rule. |

### Gaps
- There is no profile of where the 0.8 s and 3.2 s go. The hot spots above are read from the code, not measured (building and running were out of scope).
- Benchmarks of imageproc's `median_filter`, `connected_components` or `rotate_about_center` against kornia, OpenCV or Leptonica were not found.
- `photon-rs`'s operations were not inspected. GitHub issue-response times for imageproc could not be measured (API rate limit).

## Q2. Leptonica: which algorithms to port, Rust ports and bindings

### Takeaway
Leptonica (BSD-2, C) is the best permissive source to port from. The candidates, in order of value:
1. Its skew finder (Postl's differential line sums on reduced binary images, with a confidence) and shear rotation, for speed and confidence.
2. Tile-based background normalization and clean-to-white, a faster lighting step.
3. Ascender and descender orientation detection, a pure-Rust up/down signal.
4. Page-foreground finding, for auto-crop.
5. The textline dewarp, a classic alternative to UVDoc.

A pure-Rust port (`leptonica` 0.5.0) now exists as a cross-check. The bindings (`leptonica-sys`, `leptess`) are C links, stale, and useful only as reference.

### Cited Findings
- Leptonica C **1.87.0, 2025-12-24**, marked "Latest". Before it: 1.86.0 (2025-09-18) and 1.85.0 (2024-10-16) — [GitHub releases](https://github.com/DanBloomberg/leptonica/releases). The release notes say nothing about skew, dewarp or binarization changes — [GitHub releases](https://github.com/DanBloomberg/leptonica/releases).
- **Skew.** Leptonica's documentation (updated 2022-08-23) recommends one method: "the variance of differential line sums", from W. Postl's 1988 patent US 4,723,297. On "a 1 GHz P3 with an 8 Mpixel image … at 4x reduction, which will give you an accuracy of about 1/20 of a degree, it takes about 0.10 seconds to find the skew angle and about 0.12 seconds to rotate the image." Those are old hardware figures — [leptonica.org skew](http://www.leptonica.org/skew-measurement.html).
  - The Rust port keeps C's defaults: sweep ±7°, 1° sweep step, 0.01° minimum binary-search step, sweep reduction 4, search reduction 2. Each angle is scored by vertical shear plus a differential square sum — [local: leptonica-0.5.0/src/recog/skew.rs](https://docs.rs/crate/leptonica/0.5.0/source/src/recog/skew.rs).
- **Background normalization** (`adaptmap.c`):
  - Tile the image and ignore "pixels that are almost certainly not background". Average the rest per tile, requiring "about 1/3 of the pixels in the tile". Fill tiles with too few from their neighbours, smooth by block convolution, and map so the background becomes a constant such as 150 or 200.
  - `pixBackgroundNormFlex` handles fast-changing backgrounds; `pixContrastNorm` applies a local TRC for low contrast. Leptonica warns that pictures on the page are degraded unless text and images are segmented first — [leptonica.org binarization](http://www.leptonica.org/binarization.html).
- **Sauvola** in Leptonica uses integral images and a tiled version "with identical results" to bound memory, because the mean-squared accumulator "requires 64 bit floating point arrays" — [leptonica.org binarization](http://www.leptonica.org/binarization.html).
- **Dewarp** (`dewarp.c`):
  1. Binarize, then find the vertical centers of textlines.
  2. Fit least-squares quadratics to the longest lines to build a vertical disparity array.
  3. Optionally add a horizontal disparity, from the difference between top and bottom.
  4. Apply it with `dewarpApplyDisparity`.

  There is also a ruled-line model for calibration pages — [Leptonica 1.68 dewarp.c docs](https://tpgit.github.io/Leptonica/dewarp_8c.html).
- **Orientation** (`flipdetect.c`): hit-miss transforms with 6×5 structuring elements for ascenders (b, d, h, k, l) and descenders (g, p, q). They give `up_down_detect`, `orient_detect` (0/90/180/270), `make_orient_decision` and `mirror_detect` — [local: leptonica-0.5.0/src/recog/flipdetect.rs](https://docs.rs/crate/leptonica/0.5.0/source/src/recog/flipdetect.rs).
- **Page segmentation and foreground** (`pageseg.c`, ported): `find_page_foreground`, `estimate_background`, `segment_regions`, textline and textblock masks, `decide_if_table` and `auto_photoinvert` — [local: src/recog/pageseg.rs](https://docs.rs/crate/leptonica/0.5.0/source/src/recog/pageseg.rs).
- **Rust port:** `leptonica` 0.5.0, BSD-2, pure Rust, 0 `unsafe`, written by AI agents, 1 star, skew, binarize and adaptmap not checked against C. Details in Q1 — [crates.io](https://crates.io/crates/leptonica); [GitHub](https://github.com/tagawa0525/leptonica-rs); [c-compat-status](https://github.com/tagawa0525/leptonica-rs/blob/main/docs/porting/c-compat-status.md).
  - Its Sauvola is the same formula as sheaf's: u64 integral images, f64 variance, one thread, per-pixel `get_pixel_unchecked` on packed `Pix` words. It marks ink with a strict `<` where sheaf uses `<=` — [local: src/color/threshold.rs:246-284](https://docs.rs/crate/leptonica/0.5.0/source/src/color/threshold.rs).
- **Bindings:**
  - `leptonica-sys` 0.4.9 (2024-11-26; MIT; 589 k downloads) is FFI that needs a C Leptonica — [crates.io](https://crates.io/crates/leptonica-sys).
  - `leptonica-plumbing` 1.4.0 (2024-04-12; MIT) is a safe wrapper over it — [crates.io](https://crates.io/crates/leptonica-plumbing).
  - `leptess` 0.14.0 (2023-02-21; MIT) binds Tesseract and Leptonica — [crates.io](https://crates.io/crates/leptess).
  - All three are C links, and none has been released since 2024 — [crates.io](https://crates.io/crates/leptess).

### Inferences
- Porting Leptonica's skew finder (from C, cross-checked against the Rust port) would bring two things sheaf lacks:
  - a **confidence** (Leptonica returns a score ratio), so a nearly blank or picture page can say "no skew" instead of guessing;
  - a cheaper pipeline: 4× rank-reduced binary for the sweep, 2× for the search, and shear rotation for small angles instead of full-resolution bilinear rotation.
- sheaf's ±15° range is wider than Leptonica's default ±7°, and the porting must keep that.
- `up_down_detect` fits sheaf's orientation voting as a cheap pure-Rust signal for Latin text. It covers the 180° case that word shape cannot decide. It is weak on drawings and non-Latin scripts, where the caller's model stays the decider.
- Leptonica's dewarp needs many long textlines. It would serve book and letter pages in the Scanner profile without a model, but not drawings or forms.

#### Verdicts
| Leptonica algorithm | Verdict | Reason |
|---|---|---|
| Postl skew sweep and search, reductions, confidence, shear rotation | **Port** (BSD-2) | Speed and confidence, with decades of use (OCRmyPDF used it, Q3). |
| `pixBackgroundNorm`, `pixCleanBackgroundToWhite`, `pixContrastNorm` | **Port**, measured against today's flatten | Tile-based and cheaper than a median filter. |
| `pixSauvolaBinarizeTiled` | **Study** (sheaf's equals it apart from tiling) | Only memory changes. |
| `pixOrientDetect` and `pixUpDownDetect` | **Port** | A pure-Rust up/down signal for the vote. |
| `pixFindPageForeground`, border connected components | **Port** as candidates for Scanner-profile auto-crop | |
| Dewarp (textline model) | **Study, port later** if Scanner-profile book pages need it | UVDoc already covers Photo through the model trait. |
| `leptonica` crate 0.5.0 | **Study only** (reference while porting), not a dependency | Q1 evidence. |
| `leptonica-sys`, `leptess` | **Study only** | C links, against the pure-Rust goal. |

### Gaps
- There is no current published speed for Leptonica's skew or background normalization on modern CPUs; the 0.10 s and 0.12 s figures are on a 1 GHz P3.
- Leptonica's own published skew accuracy on a test set ("about 1000 pages", per its docs) was not retrieved.

## Q3. Permissive non-Rust references: OpenCV, Doxa, noteshrink, page-dewarp, OCRmyPDF, Tesseract

### Takeaway
- **Doxa (CC0)** is the most useful permissive reference found: 18 binarization algorithms, the DIBCO metrics bit-exact, and the BinBench tool.
- OpenCV's recipes cover most of the same ground, but its Rust bindings need a native OpenCV and libclang.
- noteshrink and page-dewarp are MIT and may be ported.
- OCRmyPDF no longer has its own cleaning: it hands off to unpaper (GPL, as an external program), Tesseract (deskew, OSD), and a background removal that is now disabled.

### Cited Findings
- **Doxa.** CC0-1.0, "CC0 - Brandon M. Petty, 2026". Header-only C++ with Python (nanobind), JS/WASM and MATLAB bindings; no Rust — [GitHub](https://github.com/brandonmpetty/Doxa).
  - 18 algorithms: Otsu, Bernsen, Niblack, Wellner, Sauvola, Wolf, Feng, Gatos (partial), Bradley, NICK, AdOtsu, Su, T.R. Singh, Bataineh, Phansalkar, XDoG, ISauvola, WAN.
  - Metrics: accuracy, F-measure, pseudo-F, PSNR, NRM, MCC, DRDM.
  - SIMD: SSE2, NEON and WASM SIMD128. It cites Shafait (2008) on integral images and van Herk and Gil-Werman for fast min and max filters — [GitHub](https://github.com/brandonmpetty/Doxa).
  - BinBench (v0.9.8, announced 2026-07-05) is said to be bit-exact with `DIBCO_metrics.exe` and `BinEvalWeights.exe`, except "slight floating-point drift in DRDM". It found "a slight floating-point rounding issue in DIBCO_metrics' DRDM" and "a label mismatch in Table III of the DIBCO 2019 paper" — [Discussion #58](https://github.com/brandonmpetty/Doxa/discussions/58). That is one author's claim, not independently checked.
  - `doxapy` 0.9.9 was released 2026-07-17 under CC0-1.0 — [PyPI](https://pypi.org/project/doxapy/).
- **OpenCV.**
  - opencv_contrib's `ximgproc::niBlackThreshold` offers `BINARIZATION_NIBLACK`, `_SAUVOLA`, `_WOLF` and `_NICK`, under Apache-2.0 — [ximgproc.hpp](https://github.com/opencv/opencv_contrib/blob/4.x/modules/ximgproc/include/opencv2/ximgproc.hpp).
  - `opencv-python` 5.0.0.93 was released 2026-07-02 (Apache 2.0) — [PyPI](https://pypi.org/project/opencv-python/).
  - The Rust `opencv` crate is at 0.102.0 (2026-11-08; MIT; 5.19 M downloads). It supports OpenCV 3.4 (deprecated), 4.x and 5.x and needs Clang "for automatic binding generation", plus a native OpenCV found through pkg-config, CMake or vcpkg — [crates.io](https://crates.io/crates/opencv); [GitHub](https://github.com/twistedfall/opencv-rust).
- **noteshrink** (Python, 2016): MIT, © Matt Zucker — [LICENSE](https://github.com/mzucker/noteshrink/blob/master/LICENSE.txt). The Rust `noteshrink-rs` 0.4.2 (2026-09-28; MIT; 101 downloads; 3 tests) puts clap, indicatif and pdf-writer in its library dependencies without feature gates — [crates.io](https://crates.io/crates/noteshrink-rs); [local Cargo.toml](https://docs.rs/crate/noteshrink-rs/0.4.2/source/Cargo.toml).
- **page-dewarp**: MIT, © 2016 Matt Zucker. The maintained `page-dewarp` 0.3.4 (lmmx) was released 2026-05-12 — [LICENSE](https://github.com/lmmx/page-dewarp/blob/master/LICENSE); [PyPI](https://pypi.org/project/page-dewarp/).
- **OCRmyPDF** (17.14.0 on PyPI):
  - It uses unpaper for `--clean` and `--clean-final`, and warns that some unpaper features move text — [docs](https://ocrmypdf.readthedocs.io/en/latest/advanced.html).
  - Its default unpaper arguments are `--layout none --mask-scan-size 100 --no-border-align --no-mask-center --no-grayfilter --no-blackfilter --no-deskew` — [unpaper.py](https://github.com/ocrmypdf/OCRmyPDF/blob/main/src/ocrmypdf/_exec/unpaper.py).
  - `--remove-background` raises "temporarily not implemented" — [_pipeline.py](https://github.com/ocrmypdf/OCRmyPDF/blob/main/src/ocrmypdf/_pipeline.py).
  - `--deskew` now asks the OCR engine (`tesseract.get_deskew`) — [tesseract_ocr.py](https://github.com/ocrmypdf/OCRmyPDF/blob/main/src/ocrmypdf/builtin_plugins/tesseract_ocr.py). Older docs said "Postl's variance of line sums algorithm as implemented in Leptonica" — [v9 cookbook](https://ocrmypdf.readthedocs.io/en/v9.0.5/cookbook.html).
  - The pipeline order is rotate, remove background, deskew, clean — [v9 cookbook](https://ocrmypdf.readthedocs.io/en/v9.0.5/cookbook.html).
  - The default `--rotate-pages-threshold` is 14.0 — [_defaults.py](https://github.com/ocrmypdf/OCRmyPDF/blob/main/src/ocrmypdf/_defaults.py).
- **Tesseract.** It binarizes with Otsu internally, which "can be suboptimal, particularly if the page background is of uneven darkness". 5.0.0 added Adaptive Otsu and Sauvola (`thresholding_*` parameters). It "works best on images which have a DPI of at least 300 dpi". Scanned dark borders can be "erroneously picked up as extra characters". OSD is page segmentation modes 0 and 1 — [tessdoc ImproveQuality](https://tesseract-ocr.github.io/tessdoc/ImproveQuality.html).
- Other permissive deskew tools:
  - `jdeskew` 0.4.2 (2026-10-05; MIT): "Document Image Skew Estimation using Adaptive Radial Projection" — [PyPI](https://pypi.org/project/jdeskew/).
  - `deskew` 1.6.1 (2026-06-03; MIT; sbrunner): returns −45° to 45° by default — [GitHub](https://github.com/sbrunner/deskew); [PyPI](https://pypi.org/project/deskew/).

### Inferences
- sheaf's `flatten_light` (a max filter, then a median, then division) is the widely shared OpenCV shadow-removal recipe done per channel. Nothing permissive improves on its idea, only on its speed.
- Doxa is CC0, so its C++ can be translated freely. It is the cheapest way to get Wolf, NICK, ISauvola or Bradley, and to check sheaf's Sauvola against a reference implementation.
- page-dewarp's cubic-sheet fit (MIT) is a model-free curl correction that could be ported. It needs textlines, like Leptonica's.

#### Verdicts
| Project | Verdict | Reason |
|---|---|---|
| Doxa | **Port** (CC0) the algorithms needed; **use its BinBench outside the build** as the measurement oracle | Permissive, complete, bit-exact metrics. |
| OpenCV (Apache-2.0) and the `opencv` crate | **Study only** for recipes; never link | A native C++ OpenCV and libclang break the pure-Rust goal. |
| noteshrink | **Port** (MIT) for a later "notes" mode | `noteshrink-rs` pulls clap and indicatif into its library. |
| page-dewarp | **Study, port later** (MIT) | A model-free curl correction for text pages. |
| OCRmyPDF | **Study only** for its defaults (its own license was not verified this session; see Gaps) | Its cleaning is unpaper (GPL) and Tesseract. |
| Tesseract preprocessing and OSD | **Study only** inside scan-cleanup. OSD may be a caller-supplied model behind the trait (it is C++) | Pure-Rust goal. |
| jdeskew | **Study, port** if Leptonica's method misses (MIT) | A different method with claims on DISEC'13 (not retrieved). |

### Gaps
- jdeskew's published DISEC'13 figures were not retrieved (its PyPI page did not render).
- OCRmyPDF's license was not re-verified this session.
- No published comparison of Doxa's SIMD Sauvola speed was found ("Speed numbers: none").

## Q4. Copyleft, study only: unpaper, ScanTailor (Advanced), gscan2pdf, NAPS2

### Takeaway
These set the bar for the features expected, not for algorithmic depth:
- **unpaper:** noise, blur, gray and black filters, masks, deskew from page edges, borders.
- **ScanTailor Advanced:** content and page detection, Sauvola and Wolf, despeckle levels, auto, marginal and manual dewarp, picture zones, multithreading.
- **NAPS2:** a very simple blank detector.

sheaf's blank-page check is already more careful than NAPS2's. All four are GPL, so they may be studied and cited only.

### Cited Findings
- **unpaper.** GPL-2 overall, with some files MIT or Apache-2.0. 1.2 k stars, 416 commits, FFmpeg as its only hard dependency, Meson build — [GitHub](https://github.com/unpaper/unpaper). Its last release, 7.0.0, came out 2022-05-10 — [release post](https://flameeyes.blog/2022/05/10/unpaper-7-0-0-release/). Distributions still ship 7.0.0 (Alpine 3.23: 7.0.0-r3) — [Alpine](https://pkgs.alpinelinux.org/package/v3.23/community/armhf/unpaper).
  - Order of steps: pre-rotate, then noisefilter and blurfilter (cluster removal), then blackfilter and grayfilter (wipes), then mask detection, deskew per mask, mask re-detection, mask centering, border detection and alignment.
  - Deskew sweeps a "virtual line" across each mask edge at a range of angles and takes the largest jump in dark pixels, averaging over edges. It gives up when their deviation passes `--deskew-scan-deviation`. Defaults: scan size 1500, depth 0.66, range ±5.0°.
  - Every step can be switched off per sheet. The documentation does not describe blank-page detection — [doc/image-processing.md](https://github.com/unpaper/unpaper/blob/main/doc/image-processing.md).
- **ScanTailor Advanced.** GPL-3.0 — [GitHub](https://github.com/4lex4/scantailor-advanced).
  - Features: Sauvola and Wolf binarization for when "normalizing illumination doesn't help"; color segmentation and posterization; free-shape and rectangular picture zones; dewarping in marginal (automatic, for curved top and bottom borders), auto and manual modes; a despeckle slider (1.0 cautious, 2.0 normal, 3.0 aggressive); auto margins; page detection inside black margins; filling outside areas with the estimated background or white; splitting into foreground and background layers; multithreading — [GitHub](https://github.com/4lex4/scantailor-advanced).
  - The 4lex4 upstream was last pushed 2023-09-13 — [ecosyste.ms](https://awesome.ecosyste.ms/projects/github.com%2F4lex4%2Fscantailor-advanced). Version 1.0.19 is packaged by ALT Linux, with a 2025 build — [ALT Linux](https://packages.altlinux.org/en/sisyphus/srpms/scantailor/) (a secondary source).
- **gscan2pdf.** GPL-3 (© 2006–2024). Its tools are optional unpaper, threshold, unsharp mask, crop, OCR (gocr, tesseract, cuneiform) and user-defined commands, on Perl, GTK3, ImageMagick and SANE. The page shows version 2.13.3 — [SourceForge](https://gscan2pdf.sourceforge.net/).
- **NAPS2.** GPL-2.0 or later — [README](https://github.com/cyanfish/naps2). Its blank detection, `BlankDetectionImageOp`:
  - ignores 1% of each edge (`IGNORE_EDGE_FRACTION = 0.01`);
  - counts a pixel as non-white when its luma is below a white threshold mapped from 0–100 onto 1–255;
  - calls a page non-blank when the non-white fraction passes a coverage threshold mapped from 0–100 onto 0–1%.

  [BlankDetectionImageOp.cs](https://github.com/cyanfish/naps2/blob/master/NAPS2.Images/Bitwise/BlankDetectionImageOp.cs)
  - The defaults are `BlankPageWhiteThreshold = 70` and `BlankPageCoverageThreshold = 15` — [ScanOptions.cs](https://github.com/cyanfish/naps2/blob/master/NAPS2.Sdk/Scan/ScanOptions.cs). Mapped, that is gray 179 and 0.15% coverage (arithmetic from the constants above).
- sheaf's blank check, for comparison:
  - a 4% margin;
  - the paper level taken from a high percentile;
  - ink only well below paper;
  - a 32×32 grid of cells, counting ink only in cells above a 1% floor;
  - a "nearly blank" verdict for a single short line;
  - Strict, Normal and Lenient sensitivity.

  [local: blank.rs](https://github.com/sheaf-pdf/sheaf/blob/3232a35/crates/sheaf-image/src/blank.rs)
- Commercial and patent prior art for blank pages:
  - Tungsten PSIcapture has a tolerance level that allows one line of text such as "This page intentionally left blank" — [Tungsten docs](https://docshield.tungstenautomation.com/PSIcapture/en_US/7.10.0-7xpzjipxyy/help/admin/PSIcapture_adminhelp/c_recognition_blank_pages_cp.html).
  - Patent US8724182 compares a sheet's two duplex sides to decide whether one is blank — [Google Patents](https://patents.google.com/patent/US8724182).
  - pdftl uses an ink-coverage default of 0.005 plus a check on pixel standard deviation — [pdftl](https://pdftl.readthedocs.io/en/stable/operations/delete_blank.html).

### Inferences
- The quality bar these tools set for `scan-cleanup`:
  - each step can be switched off per page, as unpaper and ScanTailor allow;
  - deskew can refuse when its evidence disagrees, which unpaper does with its deviation check; sheaf has no such refusal today;
  - despeckle has a strength setting;
  - margins can be filled with the page's own background, which sheaf already does in `clear_edges`;
  - book-page dewarping, which ScanTailor's marginal dewarp does from page borders without textlines.
- NAPS2's fixed white threshold would call gray or recycled paper non-blank and miss faint bleed-through handling. sheaf's paper-relative level and speck cells are stronger, so nothing should be taken from NAPS2 beyond the idea of exposing two sliders.
- Duplex pairing (judging a back side against its front) is a possible later refinement. It is patented (US8724182), and whether that patent is still in force was not checked.

#### Verdicts
| Project | Verdict | Reason |
|---|---|---|
| unpaper | **Study only** (GPL-2) | Its filter set and deskew-refusal idea can be cited; no code. |
| ScanTailor / ScanTailor Advanced | **Study only** (GPL-3) | Feature bar: picture zones, marginal dewarp, despeckle levels. |
| gscan2pdf | **Study only** (GPL-3) | It wraps unpaper and ImageMagick and has little to learn from. |
| NAPS2 | **Study only** (GPL-2+) | Its blank defaults are a point of comparison. |

### Gaps
- ScanTailor Advanced's current upstream (the ScanTailor-Advanced organization fork) and its latest release date were not confirmed from a primary source.
- The status of the duplex blank-page patent was not checked.

## Q5. ML models for orientation, dewarping and layout; pure-Rust runtimes against ort

### Takeaway
The permissive weights:
- UVDoc: MIT weights in its repository, and Apache-2.0 for the Paddle port;
- PaddleOCR's `PP-LCNet_x1_0_doc_ori`: 7 MB, Apache-2.0 repository;
- DocRes: MIT code, weights unverified.

Excluded:
- DocTr (non-commercial);
- DocLayout-YOLO (AGPL);
- ocrs's models, which are CC BY-SA 4.0 through HierText, so share-alike.

rten 0.28 and tract-onnx 0.23.10 both implement `GridSample`, which the full-resolution UVDoc export needs; candle-onnx 0.11.1 does not. Keeping models outside `scan-cleanup` behind a trait remains right.

### Cited Findings
- **UVDoc.** The repository is MIT (© 2023 Tanguy Magne) and its trained model is `model/best_model.pkl` in that same repository — [LICENSE](https://github.com/tanguymagne/UVDoc/blob/main/LICENSE); [README](https://github.com/tanguymagne/UVDoc).
  - Corrected UVDoc-benchmark results (MS-SSIM / AD): UVDoc 0.785 / 0.119, DocGeoNet 0.706 / 0.168, DocTr 0.697 / 0.160, DewarpNet 0.589 / 0.193. The README notes the paper's figures used "only half of the UVDoc benchmark" — [README](https://github.com/tanguymagne/UVDoc).
  - The paper is arXiv 2302.02887 (SIGGRAPH Asia 2023) — [arXiv](https://arxiv.org/abs/2302.02887).
- **PaddleX UVDoc:** CER 0.179 on the DocUNet benchmark, 869.82 ms CPU (high-performance mode), 19.05 ms GPU, 30.3 MB — [PaddleX docs](https://paddlepaddle.github.io/PaddleX/latest/en/module_usage/tutorials/ocr_modules/text_image_unwarping.html). The PaddleOCR and PaddleX repositories are Apache-2.0 — [PaddleOCR LICENSE](https://github.com/PaddlePaddle/PaddleOCR/blob/main/LICENSE).
- **How oar-ocr runs UVDoc.** In oar-ocr-core 0.10.1, `DocumentRectificationPredictor` builds the model with `rec_image_shape: [3, 0, 0]`, so the full-resolution page goes into the ONNX graph unresized. The lower-level `UVDoc` default is `[3, 512, 512]`, which would resize to 512 and back — [local: oar-ocr-core-0.10.1/src/predictors/document_rectification.rs:55, models/rectification/uvdoc.rs:24](https://docs.rs/crate/oar-ocr-core/0.10.1/source/src/predictors/document_rectification.rs). sheaf uses the predictor's builder — [local: sheaf-ocr/src/rectify.rs:72](https://github.com/sheaf-pdf/sheaf/blob/3232a35/crates/sheaf-ocr/src/rectify.rs).
- **`PP-LCNet_x1_0_doc_ori`:** four classes (0°, 90°, 180°, 270°), Top-1 99.06%, 3.24 / 1.19 ms on CPU, 7 MB. The accuracy dataset is not named — [PaddleX docs](https://paddlepaddle.github.io/PaddleX/latest/en/module_usage/tutorials/ocr_modules/doc_img_orientation_classification.html).
- **DocTr:** "Copyright © Hao Feng 2024. All Rights Reserved". Non-commercial use only; "Any Commercial Use … is strictly prohibited without explicit prior written permission" — [LICENSE.md](https://github.com/fh2019ustc/DocTr/blob/main/LICENSE.md).
- **DewarpNet** code is MIT — [LICENSE](https://github.com/cvlab-stonybrook/DewarpNet/blob/master/LICENSE). **DocRes** code is MIT, © 2024 Jiaxin Zhang — [LICENSE](https://github.com/ZZZHANG-jx/DocRes/blob/master/LICENSE). **DocLayout-YOLO** is AGPL-3.0 — [LICENSE](https://github.com/opendatalab/DocLayout-YOLO/blob/main/LICENSE).
- **ocrs models** "are trained on the HierText dataset … licensed under CC BY-SA 4.0 … and inherit its license"; the code is MIT or Apache-2.0 — [ocrs-models LICENSE.md](https://github.com/robertknight/ocrs-models/blob/main/LICENSE.md). `ocrs` 0.14.0 was released 2026-11-08 (MIT OR Apache-2.0) — [crates.io](https://crates.io/crates/ocrs). Its README says it "recognizes the Latin alphabet only" and names no orientation model — [GitHub](https://github.com/robertknight/ocrs).
- **Corner-detection models:** DocAligner (DocsaidLab) uses heatmap regression, is exported to ONNX, and is listed as Apache-2.0 by a third-party index, not by its own license file — [ecosyste.ms](https://awesome.ecosyste.ms/projects/github.com%2Fdocsaidlab%2Fdocaligner); [PyPI](https://pypi.org/project/docaligner-docsaid/).
- **Classic corner-detection bar:** Dropbox's scanner (2016) chose "a custom computer vision pipeline built from well-studied components" over a neural network, targeting 100 ms per frame. Its detections were "60% less likely to be manually corrected" than Apple's API, per a secondary summary — [Dropbox tech blog](https://dropbox.tech/machine-learning/fast-and-accurate-document-detection-for-scanning).
- **Runtimes** (crates.io, 2026-11-08):
  - `rten` 0.28.0 (2026-11-08; MSRV 1.94; 2.23 M downloads) — [crates.io](https://crates.io/crates/rten);
  - `tract-onnx` 0.23.10 (2026-11-05; MSRV 1.91; 3.97 M) — [crates.io](https://crates.io/crates/tract-onnx);
  - `candle-onnx` 0.11.1 (2026-10-31) — [crates.io](https://crates.io/crates/candle-onnx);
  - `burn` and `burn-onnx` 0.23.0-pre.1 (2026-10-29; burn MSRV 1.95) — [crates.io](https://crates.io/crates/burn), [crates.io](https://crates.io/crates/burn-onnx);
  - `ort` 2.0.0-rc.15 (2026-10-19; MSRV 1.92; "ONNX Runtime 1.31") — [crates.io](https://crates.io/crates/ort);
  - `oar-ocr` 0.10.2 (2026-10-26; Apache-2.0; MSRV 1.95) — [crates.io](https://crates.io/crates/oar-ocr).
- **GridSample support**, from the crate sources: rten 0.28.0 registers `GridSample`, and its changelog accepts the opset-20 "linear" spelling. tract-onnx 0.23.10 has `ops/grid_sample.rs`, as does tract-onnx-opl. candle-onnx 0.11.1 has no `GridSample` — [local: src-ml/x](https://docs.rs/crate/rten/0.28.0/source/CHANGELOG.md).
- **Checking the audit:**
  - Entry 90 is stale on versions: rten 0.27.0 is now 0.28.0, tract-onnx 0.23.9 is now 0.23.10, candle-onnx 0.11.0 is now 0.11.1, and burn 0.23.0-pre.1 exists. ort rc.15 is unchanged — [crates.io](https://crates.io/crates/rten).
  - Entry 13: oar-ocr 0.10.1 is now 0.10.2, ocrs 0.13.1 is now 0.14.0 — [crates.io](https://crates.io/crates/oar-ocr).
  - The cleanup-cli plan's "weights are MIT (original repo)" and "DocTr … non-commercial" are confirmed — [UVDoc LICENSE](https://github.com/tanguymagne/UVDoc/blob/main/LICENSE); [DocTr LICENSE.md](https://github.com/fh2019ustc/DocTr/blob/main/LICENSE.md).

### Inferences
- Since the predictor feeds the full-resolution image, the UVDoc ONNX graph must contain the resize, the grid upsample and `GridSample`. That would explain the cost of about 0.9 s per page on CPU. This is inferred from the code path; the graph itself was not inspected.
- A caller in the sheaf or pdf-scan application could run UVDoc and doc_ori on rten or tract and drop ONNX Runtime, but only after the spike entry 90 already asks for (operator coverage, output equality, speed).
- For `scan-cleanup` itself, the model trait stays the right boundary. No model belongs inside the crate.
- If a permissive corner-detection model is wanted for busy backgrounds (an open problem in the tuning notes), DocAligner is the lead to check. Its license and the provenance of its weights must be confirmed from the repository first.

#### Verdicts
| Model or runtime | Verdict | Reason |
|---|---|---|
| UVDoc (MIT weights) or the PaddleX UVDoc (Apache-2.0) | **Use directly**, through the caller's model trait | Permissive and best in its class. |
| PP-LCNet_x1_0_doc_ori (Apache-2.0) | **Use directly**, through the trait | 7 MB, 99.06% (vendor figure). |
| Tesseract OSD | **Use directly**, through the trait in builds that have Tesseract | C++; not in scan-cleanup. |
| DocTr, DocTr++ | **Excluded** (non-commercial) | |
| DocLayout-YOLO | **Excluded** (AGPL) | |
| ocrs models | **Study only** for scan-cleanup (CC BY-SA weights; ocrs has no orientation model) | |
| DocRes | **Study**; check its weights' license before any use | Code MIT, weights unverified. |
| rten / tract | **Use directly** in a caller after a spike; not a scan-cleanup dependency | Pure Rust, with GridSample. |
| candle-onnx | **Excluded** for UVDoc | No GridSample. |
| ort | Keep where oar-ocr pins it (entry 90) | Native ONNX Runtime. |

### Gaps
- Whether the PaddleX UVDoc was retrained, or converted from the MIT weights, is unverified.
- The license of the doc_ori weights is assumed from the Apache-2.0 repository; no separate model card was found.
- No speed or operator-coverage runs were done for UVDoc on rten or tract.
- DocAligner's license file and the provenance of its weights were not opened.

## Q6. Evaluation: datasets and metrics, so quality is measured

### Takeaway
There are standard benchmarks for three of the steps:
- **binarization:** DIBCO and H-DIBCO 2009–2019, scored by FM, pseudo-FM, PSNR and DRD, with Doxa's BinBench as an open, bit-exact scorer;
- **skew:** DISEC'13 (AED, TOP80, CE within 0.1°);
- **dewarping:** the DocUNet benchmark (MS-SSIM, LD) and the UVDoc benchmark (MS-SSIM, AD).

None exists for blank pages, phone-photo page finding or 0/90/180/270 orientation, so those need an in-house labelled corpus.

### Cited Findings
- **DIBCO.** Editions: DIBCO 2009, H-DIBCO 2010, DIBCO 2011, H-DIBCO 2012, DIBCO 2013, H-DIBCO 2014, H-DIBCO 2016, DIBCO 2017, H-DIBCO 2018, DIBCO 2019. DIBCO 2019 added machine-printed and handwritten pages and released its test data and evaluation program afterwards "to the research community" — [DIBCO 2019](https://vc.ee.duth.gr/dibco2019). Its report covers 24 submitted methods — [AMiner](https://www.aminer.cn/pub/5e3be4983a55ac2a0b37ea1d/icdar-competition-on-document-image-binarization-dibco).
  - The standard metrics are FM, pseudo-FM (recall on the skeletonized ground truth), PSNR and DRD — [arXiv 2401.11831](https://arxiv.org/pdf/2401.11831).
  - One 2026 preprint reports Otsu on DIBCO 2019 at FM 63.87, Fps 60.24, PSNR 12.67, DRD 16.87, a single, unchecked source — [arXiv 2606.08781](https://arxiv.org/pdf/2606.08781).
- **DISEC'13** (ICDAR 2013): 1,550 images, made from 155 documents × 10 rotations within about ±15°.
  - Metrics: AED (mean error), TOP80 (the mean over the best 80%) and CE (the share with error under 0.1°).
  - The winning LRDE-EPITA method reports AED 0.072°, TOP80 0.046°, CE 77.48%, figures self-reported in the authors' paper — [LRDE ICIP'14](https://www.lrde.epita.fr/wiki/Publications/fabrizio.14.icip). A Fast-Hough method reports AED 0.086, TOP80 0.056, CE 68.80 — [arXiv 1912.02504](https://arxiv.org/pdf/1912.02504).
- **DocUNet benchmark:** original photos, document-centered crops (used in the paper) and flatbed scans. Metrics MS-SSIM and Local Distortion (SIFT flow). No license is stated beyond a request to cite — [DocUNet](https://www3.cs.stonybrook.edu/~cvl/docunet.html).
- **UVDoc benchmark:** MS-SSIM and AD (the SIFT-flow distortion with the best affine transform factored out), plus OCR CER and ED — [UVDoc README](https://github.com/tanguymagne/UVDoc); [arXiv](https://arxiv.org/abs/2302.02887).
- **BinBench:** open-source and cross-platform, generating "all DIBCO metrics and bit-exact weight files" — [Discussion #58](https://github.com/brandonmpetty/Doxa/discussions/58).

### Inferences
- DIBCO is mostly historical and handwritten material. It tests Sauvola's limits but not office scans or phone photos. A small in-house set (300 dpi feeder pages and phone photos with hand-made ground truth) should complement it, scored with BinBench's FM, pseudo-FM and DRD, outside the build (it is C++).
- Deskew can be measured with DISEC'13's AED, TOP80 and CE on DISEC images. Synthetic `Degradation::Skew` pages already give exact ground truth, and the same three metrics apply to them.
- Orientation and blank pages: define accuracy and false-blank rate, and treat a false "blank" as the costly error, as `blank.rs` already does, on a labelled corpus that includes nearly blank pages (signatures, "intentionally left blank", bleed-through).

### Gaps
- The terms of use for the DIBCO, DISEC'13 and DocUNet data were not found; redistributing them as test fixtures in a repository is unverified.
- No public benchmark was found for blank-page detection, phone-photo page finding, or document orientation classification (PaddleX does not name its accuracy dataset).

## Q7. Synthetic document degradation for test fixtures

### Takeaway
There is no Rust document-degradation crate. Augraphy (MIT) is the richest permissive catalogue of effects to port. ocrodeg has no license file, which makes it study only in practice. DocCreator is LGPL-3 and so study only. `scan-fixtures` should keep its own seeded generators and add effects that exercise the Photo profile and blank pages:
- uneven light and shadow, color cast and colored paper;
- dark feeder borders, punch holes and staples;
- bleed-through and perspective;
- curl and folds.

### Cited Findings
- **Augraphy:** MIT, © 2023 Sparkfish LLC — [LICENSE](https://github.com/sparkfish/augraphy/blob/master/LICENSE). Latest PyPI release 8.2.6 on 2023-12-31 — [PyPI](https://pypi.org/project/augraphy/). 586 stars, 18 open issues, 4 open PRs — [GitHub](https://github.com/sparkfish/augraphy).
  - 35 pixel-level effects, including BadPhotoCopy, BleedThrough, ColorPaper, DirtyDrum, DirtyRollers, Dithering, Faxify, InkBleed, Jpeg, LightingGradient, LowInkPeriodicLines, NoiseTexturize, ShadowCast and SubtleNoise.
  - 8 spatial effects: BookBinding, Folding, Geometric, GlitchEffect, InkShifter, PageBorder, SectionShift, Squish.
  - The pipeline is ink layer, then paper layer, then merged effects. Speeds range from Geometric at about 136 images a second to BadPhotoCopy at 0.17. No seed or deterministic mode is documented — [GitHub](https://github.com/sparkfish/augraphy).
- **ocrodeg:** functions `random_transform` and `transform_image`, `bounded_gaussian_noise` and `distort_with_noise` (warping), `noise_distort1d` (curl), `binary_blur`, `random_blotches`, `make_fibrous_image`, `printlike_multiscale` and `printlike_fibrous`. 166 stars, 14 commits — [GitHub](https://github.com/NVlabs/ocrodeg). It has no LICENSE file (the `LICENSE` URL returns 404); `setup.py` says only "See the LICENSE file for licensing terms (BSD-style)" — [setup.py](https://github.com/NVlabs/ocrodeg/blob/master/setup.py).
- **DocCreator:** LGPL-3.0 — [LICENSE](https://github.com/DocCreator/DocCreator/blob/master/LICENSE).
- **Rust:** crates.io searches for augmentation, degradation, imgaug and synthetic-document terms found only general machine-learning augmentation (`albumrust` 0.2.0, 80 downloads, "inspired by Albumentations") — [crates.io](https://crates.io/crates/albumrust). imageproc 0.27 has `gaussian_noise` and `salt_and_pepper_noise`, both seeded through `StdRng` — [local: noise.rs](https://docs.rs/crate/imageproc/0.27.0/source/src/noise.rs) — which rand says is "not portable" across versions — [docs.rs](https://docs.rs/rand/latest/rand/rngs/struct.StdRng.html).
- **Today's fixtures** use their own xorshift `Rng` for noise and salt-and-pepper. Rotation, blur and resampling come from imageproc and `image`, and the JPEG round trip from `image`'s codec. All degradations work on `GrayImage` only — [local: codes/fixture.rs:385-470, 585-597](https://github.com/sheaf-pdf/sheaf/blob/3232a35/crates/sheaf-image/src/codes/fixture.rs).

### Inferences
- The fixtures' own RNG is already the right choice: imageproc's noise could change silently with a rand upgrade. The JPEG, blur and resample steps still depend on `image` and imageproc versions, so their bytes are not stable across upgrades. Tests should assert on cleanup outcomes (angle, verdict, ink share), not on the degraded bytes, or pin those dependencies.
- What matters most, judged against the steps and the known failures in the cleanup-cli notes (an inference, not a published ranking):
  1. **Photo profile:** a lighting gradient and cast shadow (augraphy LightingGradient and ShadowCast); a color cast and colored paper (ColorPaper); perspective and dark or busy backgrounds; curl and folds (BookBinding, Folding; ocrodeg-style `noise_distort1d`).
  2. **Scanner profile and blank pages:** a dark feeder edge or skewed-feed shadow (PageBorder); punch holes and staples in the margin; bleed-through from the back (BleedThrough), which is the key false "not blank" case; specks and dust (DirtyDrum, DirtyRollers); a near-blank page with one signature.
  3. **Binarization:** low ink and faded print (LowInkPeriodicLines, InkBleed); fax and dither patterns (Faxify, Dithering); 1-bit inputs.
  4. Keep: skew, turn, noise, salt and pepper, blur, low contrast, JPEG, threshold, resolution.
  5. Add RGB degradations, because the Photo profile's color steps cannot be tested on gray pages.

#### Verdicts
| Source | Verdict | Reason |
|---|---|---|
| Augraphy (MIT) | **Port** selected effects into `scan-fixtures`, with the fixture RNG | Permissive and the richest catalogue. It is Python, slow, and has no documented seeding. |
| ocrodeg | **Study only** | No license file; the "BSD-style" claim points to a missing file. |
| DocCreator (LGPL-3) | **Study only** | Copyleft. |
| imageproc `noise` | **Do not use** for fixtures | StdRng is not portable across versions. |
| Overall degradation engine | **Build our own** (extend `Degradation`) | Nothing in Rust. |

### Gaps
- No published study ranking which synthetic degradations best predict real scan-cleanup failures was found.
- Augraphy's GitHub commit activity after 8.2.6 could not be dated (API rate limit).

## Holes in the ecosystem

### Takeaway
Nothing permissive and pure-Rust meets these needs adequately today:
- document binarization beyond Otsu and mean-adaptive, with Sauvola, Wolf and NICK in a maintained crate;
- a deskew with a confidence;
- a fast tile-based background normalization;
- punch-hole removal;
- color dropout;
- blank-page detection;
- auto-crop and border removal for scans;
- up/down orientation without a model;
- model-free dewarping;
- a seeded document-degradation library;
- a pure-Rust DIBCO-style scorer.

`leptonica` 0.5.0 nominally covers several of these, but has not shown the maturity to depend on.

### Cited Findings
- Exact-name crates `binarize`, `sauvola`, `deskew`, `otsu`, `unpaper`, `dewarp`, `scan-cleanup` and `scan-fixtures` do not exist (404) — [crates.io API](https://crates.io/crates/deskew).
- imageproc has no Sauvola, Niblack or Wolf, and no deskew — [local: contrast.rs](https://docs.rs/crate/imageproc/0.27.0/source/src/contrast.rs).
- The `leptonica` port covers skew, adaptmap, Sauvola, orientation, dewarp and page foreground. It has 1 star, is AI-ported, and these functions are not in its C pixel-compatibility report — [GitHub](https://github.com/tagawa0525/leptonica-rs); [c-compat-status](https://github.com/tagawa0525/leptonica-rs/blob/main/docs/porting/c-compat-status.md).
- Punch-hole removal appears only in commercial SDKs (Black Ice, LEADTOOLS' `HolePunchRemove`, which needs 1-bit images) and patents. US 8,494,304 describes a morphological open, then size and roundness checks for objects near the page border — [Black Ice](https://blackice.com/docimagingPUNCHHOLE.htm); [LEADTOOLS](https://www.leadtools.com/help/sdk/main/clib/lbitmap-holepunchremove.html); [US8494304](https://image-ppubs.uspto.gov/dirsearch-public/print/downloadPdf/8494304).
- Color dropout appears only in commercial SDKs (Accusoft ScanFix's HSV dropout, and emulating a dropout-bulb scanner by keeping one channel) and patents (US7853074: find the dominant background and form colors on a subsampled image, then drop them in HSV tile by tile) — [Accusoft](https://help.accusoft.com/ScanFixXpress/dotnet/topic51.html); [US7853074](https://patents.google.com/patent/US7853074).
- No Rust document-degradation crate exists — [crates.io](https://crates.io/crates/albumrust).

### Inferences
- These holes are why `scan-cleanup` is worth publishing: it would be the first maintained, permissive, pure-Rust crate with Sauvola, deskew with a confidence, background flattening, blank detection and page finding behind one API.
- Punch holes and color dropout are small to build: a margin-restricted open followed by connected components with a roundness test; a per-pixel hue and saturation distance, or keeping one channel. Patents exist, so the approach should be one of the textbook building blocks, and the patents' status checked before shipping.

### Gaps
- Patent status (expiry and claims) for punch-hole removal (US8494304) and color dropout (US7853074, US7961941) was not checked.
- No permissive model-free curl correction exists in Rust. page-dewarp (MIT, Python) and Leptonica's dewarp (BSD-2, C) are the port sources.
