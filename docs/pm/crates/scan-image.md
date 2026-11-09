# scan-image

Layer: Pages (`crates/README.md`). Paths are from `docs/pm/`.

- **What:** a scanned page's image, held as compactly as it allows, and image files in and out
  (`sheaf_image::{io, encode}`).
  - The page (`io::PageImage`): a JPEG kept as its bytes, its SOF read for size and components and its EXIF
    turn for which way is up (sheaf's own few-dozen-byte IFD0 reader kept, `kamadak-exif` 0.6.2 as the
    fallback); black and white held as CCITT G4 from the moment it's made (`Pixels::G4`; M5 slice 1's gate
    holds 500 pages under 400 MB); gray and RGB as pixels; each with its resolution.
  - Files in: JPEG, PNG, TIFF (a multi-page TIFF's pages one at a time, `io::frames`; reduced-resolution
    images aren't pages), BMP, GIF, WebP. Out: PNG, JPEG, TIFF (multi-page, LZW or G4) with their resolution,
    and G4.
  - **CCITT:** every CCITT strip in a TIFF, compressions 2, 3 and 4 alike, taken as raw strip bytes from
    `tiff`'s IFD (`FillOrder` and `T4Options` read here) and decoded through one hayro-ccitt adapter, the
    same one `pdf-images` uses. hayro-ccitt 0.5.0 (2026-11-02) decodes G4, G3 1D and 2D, EOLs whatever the
    flag says, byte-aligned rows (Modified Huffman), end of block and damaged lines, with no `unsafe`; its
    API broke in 0.5.0 (rows as change positions, `BlackIs1` the caller's inversion, `Group3_2D` without
    `k`). This fixes BUG-2 and reads back G4 TIFFs wider than 65,535 px, which `tiff` refuses.
  - **TIFF writing:** LZW and Deflate multi-page through `tiff`; G4 TIFFs through sheaf's own directory
    writer, kept, since `tiff` writes nothing 1-bit or CCITT.
  - **JPEG:** decoded by zune-jpeg 0.5.15 (baseline, extended, progressive 8-bit Huffman, Adobe APP14);
    12-bit, lossless and arithmetic-coded files are typed refusals until the corpus shows they matter.
    Encoded by `jpeg-encoder` 0.7.1 (CMYK, custom tables; "(MIT OR Apache-2.0) AND IJG", so the IJG notice
    goes in the docs). No lossless transforms: a JPEG's turn is `/Rotate` (the only pure-Rust candidate,
    libjpeg-turbo-rs, disclaims memory safety).
- **Fixed in the cut** (said in the slice; sheaf's output changes only where it was wrong; `backlog/bugs.md`):
  - **B24** (BUG-1): G4 encoding truncates widths over 65,535 pixels (`fax` 0.2's `u16`). `fax` 0.3.0 takes
    `u32` widths but panics past 32,767 px with overflow checks on (issue #30); its fix (#31, merged
    2026-11-05) is unreleased. Take 0.3.1 or a pinned revision, or refuse widths past 32,767 until then.
  - **B25** (BUG-2): G3 and Modified Huffman TIFFs are refused (`tiff` 0.11.3). Fixed by the CCITT route
    above; `tiff`'s `main` reads G3 1D only, unreleased, and its Modified Huffman PR is unreviewed.
  - **B15** (BUG-3): CMYK16, float RGB and RGBA, planar and two-sample gray TIFFs refused after a file's
    first page. `tiff` decodes them (two-sample gray as `Multiband`): the mapping is this crate's.
  - **B14** (BUG-4): a TIFF's Orientation tag ignored by `tiff_pages`, where every other path honors it.
    `tiff` exposes the tag and doesn't apply it: one orientation step on every path.
- **And offered** (`backlog/features.md`): a file's page count from its headers (FEAT-1, for sheaf's B17).
- **Boundary:** no `imageproc`; codecs only: `tiff` 0.11.3, `hayro-ccitt` 0.5.0, `fax`, `png` 0.18.1
  (`forbid(unsafe_code)`, on OSS-Fuzz; zune-png widens depths below 8 bits, wrong for 1-bit scans),
  `zune-jpeg`, `jpeg-encoder`, and `image` 0.25.10 with default features off (BMP, GIF, WebP, pixel
  buffers). No C at build time: `tiff`'s `zstd` and `image`'s `avif-native` stay off.
- **Tested by:** sheaf's tests for `io` and `encode`, moved; the harness on the corpus's image files and
  sheaf's fixtures, byte for byte; fuzzing of every decoder over hostile headers, dimensions capped first
  (`image`'s own changelog: "Many decoders will panic on malicious input"); the hayro codecs fuzzed through
  our adapter, since none is on OSS-Fuzz (TEST-4).
- **Open:**
  - MSRV: the hayro crates need Rust 1.92, above the family's 1.89 (Q-1).
  - How many 12-bit or arithmetic-coded JPEGs the corpus holds; the speed of an all-safe build, SIMD off.
  - One maintainer writes hayro's CCITT, JBIG2 and JPEG 2000 decoders, which both this crate and
    `pdf-images` sit on: pinned behind our adapters, fuzzed there.
