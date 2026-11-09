# PDF images and writing scanned PDFs: prior art for `pdf-images` and `pdf-scan`

As of 2026-11-08. Crate versions, dates, downloads and licenses are from the crates.io API, read on 2026-11-08 with a generic User-Agent. "Read in source" means I read the published crate's source (downloaded to a temporary directory, or already in cargo's local registry cache), not built or run. Line numbers refer to that version. Codec internals (zune-jpeg, hayro-ccitt, hayro-jbig2, hayro-jpeg2000, fax, JBIG2 encoders) belong to another researcher, so they come up here only where the PDF-level glue depends on them.

License rule used in the verdicts: GPL, LGPL, AGPL and MPL-2.0 projects are **study only** (read and cite, never copy, port or link). Only MIT, Apache-2.0, BSD, ISC, Zlib, CC0 or public-domain code may be ported.

## Q1. Decoding PDF images to native samples: who exposes an image's own color space, depth and /Decode?

### Takeaway
No library in any language hands back an image's samples in its own color space, depth and /Decode over an object model of our choosing. Only two come close: PDFBox (Apache-2.0, Java), with `getRawRaster()`, and hayro-syntax 0.9.0 (MIT OR Apache-2.0). hayro-syntax's public `Stream::decoded_image` runs every filter and codec, but only on hayro's own parsed `Stream`, and it widens CCITT, JBIG2 and JPX to 8 bits unless the image is Indexed. hayro-interpret, PDFium and pdf.js give 8-bit RGB, gray or RGBA only. The PDF-level quirk code in hayro-syntax's `filter/*.rs` and hayro-interpret's `x_object/decode/` is permissive and small, so it can be ported. The decoder itself has to be built on pdf-objects and pdf-color.

### Cited Findings

**hayro-syntax 0.9.0** (2026-11-02; Apache-2.0 OR MIT; MSRV 1.92; 6.2M downloads; 17 reverse dependencies; repo 858 stars, 92 open issues, pushed 2026-11-08) — [crates.io](https://crates.io/crates/hayro-syntax), [repo](https://github.com/LaurenzV/hayro)
- `Filter::apply` is still `pub(crate)` ([filter/mod.rs:65](https://docs.rs/crate/hayro-syntax/0.9.0/source/src/filter/mod.rs)). `Stream::decoded_image(&ImageDecodeParams) -> FilterResult` is public and runs the whole chain, codecs included. It returns `data` plus an optional `ImageData { alpha, color_space: Gray|Rgb|Cmyk|Unknown(n), bits_per_component, width, height }` ([object/stream.rs:197-245, 295-326](https://docs.rs/crate/hayro-syntax/0.9.0/source/src/object/stream.rs)).
- `Stream::new` is `pub(crate)` ([stream.rs:58](https://docs.rs/crate/hayro-syntax/0.9.0/source/src/object/stream.rs)), so a caller can't wrap its own bytes and dictionary. `decoded_image` works only on objects hayro's own parser produced.
- `ImageDecodeParams` carries `is_indexed`, `bpc`, `num_components`, `target_dimension`, `width` and `height` from the image dictionary ([stream.rs:38-55](https://docs.rs/crate/hayro-syntax/0.9.0/source/src/object/stream.rs)).
- **DCT** ([filter/dct.rs](https://docs.rs/crate/hayro-syntax/0.9.0/source/src/filter/dct.rs)):
  - It refuses dictionary dimensions above 65,535 (`:16`).
  - It patches the SOF dimensions when the JPEG claims a larger area than the dictionary (`maybe_patch_jpeg_dimensions`). The marker scanner carries the comment "Not sure if 100% correct/robust, is AI-generated".
  - It honors `/ColorTransform`.
  - It overrides CMYK to RGB when an APP14 says CMYK but there are 3 components.
  - It converts YCCK to CMYK with pdf.js's formula.
  - Its output is always 8-bit.
- **CCITT** ([filter/ccitt.rs:11-60](https://docs.rs/crate/hayro-syntax/0.9.0/source/src/filter/ccitt.rs)):
  - It maps `K`, `Columns` (default 1728), `Rows` (the dictionary's `/Height` wins), `EndOfBlock`, `EndOfLine`, `EncodedByteAlign` and `BlackIs1`.
  - The comment at `:50` reads: "Whenever possible (if we don't have an indexed color space), we convert the data as 8-bit instead of 1-bit, so that it can be easier converted into an RGBA8 image".
  - On a decode error it keeps the rows already decoded and fails only if there are none.
- **JBIG2** ([filter/jbig2.rs](https://docs.rs/crate/hayro-syntax/0.9.0/source/src/filter/jbig2.rs)):
  - It reads `/JBIG2Globals` from `DecodeParms` and calls `hayro_jbig2::Page::new_embedded(data, globals)`.
  - It inverts JBIG2's black=1 to PDF's convention.
  - The same 8-bit widening applies unless the image is Indexed (`:30`).
- **JPX** ([filter/jpx.rs](https://docs.rs/crate/hayro-syntax/0.9.0/source/src/filter/jpx.rs)):
  - It splits the alpha channel out as `alpha` (the SMaskInData case).
  - The comment at `:76` reads "The decoded image is always 8-bit, so if necessary we have to rescale ourselves". It rescales to `/BitsPerComponent` only for Indexed images.
  - It refuses ICC-profiled JPX with other than 1, 3 or 4 channels.
- **Safety and features:**
  - Default features are `std` and `images`; the codecs come in with `default-features = false`.
  - A non-default `unsafe` feature switches Flate to flate2 with zlib-rs, turns on memchr and enables the codecs' SIMD ([Cargo.toml](https://docs.rs/crate/hayro-syntax/0.9.0/source/Cargo.toml.orig)). Without it, hayro uses its own Flate (`filter/lzw_flate.rs`).
  - `lib.rs` has no `forbid(unsafe_code)`; `page.rs:528` has a `core::mem::transmute` ([page.rs](https://docs.rs/crate/hayro-syntax/0.9.0/source/src/page.rs)).
  - The crate docs say "When the `unsafe` feature is disabled, there is only use of `unsafe` in `hayro-syntax`" ([lib.rs:50](https://docs.rs/crate/hayro-syntax/0.9.0/source/src/lib.rs)). The sentence is garbled as published.

**hayro-interpret 0.9.0** (2026-11-02; Apache-2.0 OR MIT; 6.0M downloads; 8 reverse dependencies; `#![forbid(unsafe_code)]`) — [crates.io](https://crates.io/crates/hayro-interpret)
- Images reach a `Device` as `Image::Raster(RasterImage)` or `Image::Stencil`. The only pixel accessor is `RasterImage::with_rgba(|ImageData, Option<LumaData>|, target_dimension)`, and `ImageData` is `Rgb(RgbData)` or `Luma(LumaData)`, both `Vec<u8>` ([types.rs:77-230](https://docs.rs/crate/hayro-interpret/0.9.0/source/src/types.rs)). Color spaces are converted. Indexed, Separation, DeviceN, ICC and 16-bit samples never reach the caller.
- `RasterImage::stream()` exposes the raw encoded stream ([types.rs:99-101](https://docs.rs/crate/hayro-interpret/0.9.0/source/src/types.rs)).
- `decode_context` reads `/BitsPerComponent` (or `/BPC`), `/Decode` (or `/D`, falling back to the space's default) and the image's color space. A wrong `Width`/`Height` is reported through `scale_factors` ([x_object/decode/mod.rs:28-112](https://docs.rs/crate/hayro-interpret/0.9.0/source/src/x_object/decode/mod.rs)).
- `fix_image_length` truncates excess data. When data is short, it reduces the height and pads the last row ([mod.rs:115-141](https://docs.rs/crate/hayro-interpret/0.9.0/source/src/x_object/decode/mod.rs)).
- Samples of 1 to 16 bits are unpacked to `u16` (`unpack_samples`, `:326-362`).
- Color-key `/Mask` arrays and `/SMaskInData == 1` are handled ([decode/image.rs:35, 201](https://docs.rs/crate/hayro-interpret/0.9.0/source/src/x_object/decode/image.rs)).
- Inline-image color space names are resolved through resources. A comment notes that some producers name spaces this way in XObjects too, "though the PDF spec forbids it" ([x_object/image.rs:89-100](https://docs.rs/crate/hayro-interpret/0.9.0/source/src/x_object/image.rs)).
- `ImageXObject::new` rejects a zero width or height. I found no upper cap on `Width × Height` before decoding ([x_object/image.rs:108-116](https://docs.rs/crate/hayro-interpret/0.9.0/source/src/x_object/image.rs)).
- Dependencies: hayro-syntax, hayro-font 0.5.0 (which uses skrifa), hayro-cmap, kurbo, moxcms, smallvec and others. There is no direct skrifa dependency ([Cargo.toml](https://docs.rs/crate/hayro-interpret/0.9.0/source/Cargo.toml.orig)).

**hayro (the project)** — [README](https://github.com/LaurenzV/hayro/blob/main/README.md)
- The README claims a "visual regression test suite consisting of 1,600+ PDFs, many of which are tricky PDFs that have been scraped from the test suite of other PDF engines, like pdf.js or PDFBox".
- It names remaining work: "Robustness: … there are still many ways to trigger timeouts and OOM errors", plus performance and "API surface: I am still in the process of figuring out the right API surfaces for all crates".
- `hayro-jbig2` 0.4.0's README still says "This crate is currently under development" ([README](https://docs.rs/crate/hayro-jbig2/0.4.0/source/README.md)).
- `hayro-jpeg2000` 0.5.0 claims it was "tested against 20.000+ images scraped from random PDFs" ([README](https://docs.rs/crate/hayro-jpeg2000/0.5.0/source/README.md)).

**hayro-jpeg2000 0.5.0** (2026-11-02): `DecodedImage::data_u8()` scales to 8-bit. `components()` returns `ComponentData` whose `samples()` are `&[f32]` with a `bit_depth()`, so a caller can recover 16-bit values ([lib.rs:355-395](https://docs.rs/crate/hayro-jpeg2000/0.5.0/source/src/lib.rs), [j2c/mod.rs:33-48](https://docs.rs/crate/hayro-jpeg2000/0.5.0/source/src/j2c/mod.rs)).

**pdf (pdf-rs) 0.10.0** (2026-03-02; MIT; 650k downloads; repo 1,719 stars, pushed 2026-11-07) — [crates.io](https://crates.io/crates/pdf), read in source [enc.rs](https://docs.rs/crate/pdf/0.10.0/source/src/enc.rs)
- DCT goes through `jpeg-decoder` and ignores the DCT parameters entirely (`dct_decode(data, _params)`, `:355-360`).
- CCITT:
  - Only `K < 0` (G4) decodes. `K >= 0` reaches `unimplemented!()` (`:413`), a panic.
  - Columns and rows are narrowed to `u16`.
  - The code uses `assert_eq!` on the decoded length (`:404-406`).
  - The output is 8-bit, 0 or 255.
- JPX and JBIG2 need process-global decoder hooks the caller sets (`set_jpx_decoder`, `set_jbig2_decoder`, `:445-475`). JBIG2 globals are concatenated in front of the data.
- `decode()` bails on any filter other than ASCIIHex, ASCII85, LZW, Flate, RunLength and DCT (`:479-488`).

**lopdf 0.47.0** (2026-11-04; MIT; MSRV 1.88; 29.0M downloads; repo 2,292 stars, 86 open issues) — [crates.io](https://crates.io/crates/lopdf)
- `Document::get_page_images(page)` returns, for each `/XObject` of subtype Image in the page's (inherited) resources, its id, width, height, the first name of its color space, its filters, its BPC, the still-encoded `content` and the dictionary ([document.rs:929-990](https://docs.rs/crate/lopdf/0.47.0/source/src/document.rs), [xobject.rs:14-25](https://docs.rs/crate/lopdf/0.47.0/source/src/xobject.rs)).
- It doesn't decode codecs, doesn't look at inline images or images inside forms, and doesn't check whether the image is drawn.

**pdf_oxide 0.3.78** (2026-09-08; MIT OR Apache-2.0; MSRV 1.88; 1.53M downloads; repo 1,179 stars, 221 open issues) — [crates.io](https://crates.io/crates/pdf_oxide)
- It is a kitchen-sink crate. Its dependency list includes aws-lc-rs, lcms2, ort, pdfium-render, pyo3, tract-onnx, jpeg-decoder, fax, hayro-jbig2 and hayro-jpeg2000, and there are 99 `unsafe` occurrences in `src/` ([Cargo.toml](https://docs.rs/crate/pdf_oxide/0.3.78/source/Cargo.toml.orig)).
- Its image extractor hard-codes `DeviceN => 4` components ([extractors/images.rs:670](https://docs.rs/crate/pdf_oxide/0.3.78/source/src/extractors/images.rs)), which is wrong for any DeviceN with N ≠ 4.
- It expands Indexed to palette RGB and collapses 16-bit to 8-bit (doc comments at `:61-71`).

**pdf-extract 0.12.1** (2026-09-16; MIT; 7.86M downloads) only extracts text. Its `Do` handler recurses into any XObject's contents without a `/Subtype` check in the lines shown, and it unwraps names ([lib.rs:1860-1868](https://docs.rs/crate/pdf-extract/0.12.1/source/src/lib.rs)). It ignores `Tr`, so it extracts invisible text. That makes it a possible text round-trip oracle, though not an image tool.

**PDFium / pdfium-render 0.9.5** (2026-11-01; MIT OR Apache-2.0; 3.78M downloads) — [crates.io](https://crates.io/crates/pdfium-render)
- `FPDFImageObj_GetBitmap` realizes images into 8bpp gray or BGR/BGRx/BGRA. `FPDFImageObj_GetImageDataDecoded` stops before image codecs, per [fpdf_editimg.cpp](https://pdfium.googlesource.com/pdfium/+/HEAD/fpdfsdk/fpdf_editimg.cpp) and [fpdf_parser_decode.cpp](https://pdfium.googlesource.com/pdfium/+/HEAD/core/fpdfapi/parser/fpdf_parser_decode.cpp) (the prior audit's reading; not re-read today).
- pdfium-render wraps `get_raw_image`, `get_processed_image` (FPDFImageObj_GetRenderedBitmap), `get_raw_image_data` (GetImageDataRaw), `horizontal_dpi`/`vertical_dpi`, `bits_per_pixel` and `color_space` (FPDFImageObj_GetImageMetadata), and the filter list. Text objects expose `render_mode()` ([0.9.4 source, image.rs:288-900](https://docs.rs/crate/pdfium-render/0.9.4/source/src/pdf/document/page/object/image.rs); [text.rs:241](https://docs.rs/crate/pdfium-render/0.9.4/source/src/pdf/document/page/object/text.rs)).
- It is C++ underneath, a binary loaded at run time.

**PDFBox 3.0.8** (Apache-2.0; Maven metadata last updated 2026-07-15) — [maven-metadata.xml](https://repo1.maven.org/maven2/org/apache/pdfbox/pdfbox/maven-metadata.xml)
- `PDImage.getRawRaster()`: "Return the image data as WritableRaster. You should consult the PDColorSpace … to know how to interpret the data … Use this if e.g. want access to the raw color information of a PDDeviceN image."
- `getRawImage()`: "Try to get the raw image as AWT buffered image with its original colorspace. No color conversion is performed" ([PDImage.java:48-77](https://github.com/apache/pdfbox/blob/trunk/pdfbox/src/main/java/org/apache/pdfbox/pdmodel/graphics/image/PDImage.java)).
- A known bug: `getImage()` can throw when the raster's band count doesn't match the color space's components ([PDFBOX-5036](https://issues.apache.org/jira/browse/PDFBOX-5036)). Another report shows raw data diverging from the color space metadata ([PDFBOX-5738](https://issues.apache.org/jira/browse/PDFBOX-5738)).

**pdf.js** (Apache-2.0; pdfjs-dist 6.5.438, 2026-11-01) — [npm](https://www.npmjs.com/package/pdfjs-dist), [image.js](https://github.com/mozilla/pdf.js/blob/master/src/core/image.js)
- `PDFImage` handles `/SMaskInData` (`:185, 264-301`) and `/Matte`. It un-premultiplies preblended colors (`:105-140, 242, 268-277`).
- It handles `/ImageMask` (and `/IM`) and color-key `/Mask`, and warns on "Ignoring /Mask in image without /ImageMask" (`:371-373`).
- It supplies fallback dimensions for corrupt SMasks (`:354-357`) and applies `/Decode` (`needsDecode`, `:330-338`).
- Its output goes to canvas as RGBA.

**MuPDF** (AGPL-3.0; `mupdf` crate 0.8.0, 2026-06-22; PyMuPDF 2.1, 2026-11-02, "GNU AFFERO GPL 3.0 or Artifex Commercial License") — [crates.io](https://crates.io/crates/mupdf), [PyPI](https://pypi.org/project/PyMuPDF/)
- PyMuPDF's `Document.extract_image(xref)` "avoids using pixmaps wherever possible to present the image in its original format (e.g. as JPEG)". It returns `ext`, `smask`, `width`, `height`, `colorspace` (n), `cs-name`, `xres`, `yres` and `image` bytes.
- `get_page_images(full=True)` gives `(xref, smask, width, height, bpc, colorspace, alt_colorspace, name, filter, referencer)` ([docs](https://pymupdf.readthedocs.io/en/latest/document.html)).
- Study only.

**Poppler pdfimages** (GPL; man page for poppler-utils 26.07.0) — [man page](https://manpages.debian.org/testing/poppler-utils/pdfimages.1.en.html)
- `-j`, `-jp2`, `-jbig2` (with `.jb2g` globals) and `-ccitt` (with a `.params` file for fax2tiff) write the stream data "identical" to the PDF's. Everything else becomes PNM/PNG/TIFF; CMYK becomes TIFF.
- `-list` prints page, num, type (image/mask/smask/stencil), width, height, color (gray/rgb/cmyk/lab/icc/index/sep/devn), comp, bpc, enc (image/jpeg/jp2/jbig2/ccitt), interp, object ID, x-ppi, y-ppi, size and ratio.
- Study only.

**qpdf 12.4.2** (Apache-2.0, C++; 2026-09-26) — [release notes](https://github.com/qpdf/qpdf/blob/main/manual/release-notes.rst)
- `--decode-level=all` decodes the generalized filters (LZW, Flate, ASCII85, ASCIIHex), then RunLength ("specialized"), and of the lossy filters "currently this is just /DCTDecode". It leaves CCITT, JBIG2 and JPX untouched ([cli.rst:894-945](https://github.com/qpdf/qpdf/blob/main/manual/cli.rst)).

**The prior audit's corpus counts** (entry 33): `scans/` has 709 files, 464 with DCT, 150 CCITT, 127 JBIG2 and 52 JPX ([entry 33](https://github.com/sheaf-pdf/sheaf/blob/3232a35/plan/research/build-or-adopt/33-pdf-image-samples-image-s-own.md)). 15 of the 127 JBIG2 scans have a `/JBIG2Globals` ([entry 37](https://github.com/sheaf-pdf/sheaf/blob/3232a35/plan/research/build-or-adopt/37-jbig2-decode.md)).

**The family today:**
- `pdf-color` already types Indexed (with a `Lookup`), Separation, DeviceN (with `Attributes`) and ICC spaces (pdf-lint's `crates/pdf-color/src/space.rs`, lines 136-323).
- `pdf-filters` has the predictors (pdf-lint's `crates/pdf-filters/src/predictor.rs`).
- `pdf-content-streams`' `image-ends` feature pins `hayro-ccitt = "0.3"` (pdf-lint's `crates/pdf-content-streams/Cargo.toml`, lines 35-48), while hayro-syntax 0.9.0 is on hayro-ccitt 0.5.0 ([Cargo.toml](https://docs.rs/crate/hayro-syntax/0.9.0/source/Cargo.toml.orig)).

### Inferences
- How the candidates cover the needs:
  - Indexed: PDFBox (raw) keeps the index samples. pdf.js, hayro-interpret and PDFium expand it to RGB. hayro-syntax keeps 1-bit only when told the image is Indexed.
  - Separation and DeviceN: native samples only in PDFBox's `getRawRaster` (and MuPDF, study only). Everyone else converts.
  - ICCBased: everyone converts except PDFBox raw.
  - 16-bit: hayro-interpret unpacks to `u16` internally but outputs 8-bit. hayro-jpeg2000 gives f32 samples at native depth. PDFBox raw keeps it.
  - /Decode: applied by hayro-interpret, pdf.js and PDFBox (on `getImage`).
  - JBIG2Globals: hayro-syntax, pdfimages, PDFium, pdf.js and MuPDF.
  - SMaskInData: hayro-syntax, hayro-interpret and pdf.js.
  - Damaged data: hayro (short-data padding, JPEG SOF patch, CCITT partial rows) and pdf.js (fallback SMask dimensions).
- hayro-syntax's design choice to widen CCITT and JBIG2 to 8 bits "so that it can be easier converted into an RGBA8 image" is a renderer's choice. For `pdf-images` the 1-bit samples must stay packed, so porting its glue (and changing that) beats calling it, even if the object-model mismatch were solved.
- Adopting hayro-syntax as the image path would mean parsing every file a second time with a second parser (hayro's), beside pdf-structure. That contradicts the family's one-object-layer design.
- No candidate caps `Width × Height × components × bpc` before decoding. hayro-syntax caps only DCT at 65,535 per side. pdf-images' "read and capped before decoding" rule has no prior art to lean on beyond those checks.

### Gaps
- I didn't fuzz or run any of these decoders. "Quality" here is from source reading, README claims and issue titles, not from measured crash or timeout rates.
- I didn't re-read PDFium's current `fpdf_editimg.cpp`. The 8-bit conversion claim is the 2026-10-23 audit's.
- I couldn't confirm whether zune-jpeg inverts Adobe-APP14 CMYK. hayro-syntax's DCT path doesn't apply an inversion itself (dct.rs), so it is left to `/Decode` or the interpreter. That is codec territory.
- MuPDF's `fz_load_image` colorspace behavior for Separation and DeviceN wasn't checked (study only anyway).

## Q2. Writing scanned PDFs in Rust: could pdf-writer, krilla, printpdf, lopdf or another crate write a searchable scanned PDF with a glyphless-font layer today?

### Takeaway
pdf-writer could write every object needed: CCITT `DecodeParms`, a CIDFontType2 with a `CIDToGIDMap` stream, render mode 3. But it is a second serializer that buffers the whole file in one `Vec<u8>` and writes f32 numbers. krilla can't: it has no render mode 3, no 1-bit or CCITT images, and one CID per glyph, which defeats the glyphless trick. printpdf mints resource names from a process-global counter, lopdf keeps the whole document in memory, and genpdf has had no release since 2021. Three 2026 crates write searchable PDFs, all tiny:
- deepocr-core is permissive but maps every CID to `.notdef`.
- pdf-ocr is AGPL and uses Helvetica.
- ocrs-cjk-cli is a CLI.

None replaces building the layer on the family's writers.

### Cited Findings

**pdf-writer 0.15.0** (2026-05-27; MIT OR Apache-2.0; 7.59M downloads; 84 reverse dependencies; repo 746 stars, 12 open issues, pushed 2026-10-27; `#![forbid(unsafe_code)]`) — [crates.io](https://crates.io/crates/pdf-writer)
- It has `TextRenderingMode::Invisible` ([content.rs:576](https://docs.rs/crate/pdf-writer/0.15.0/source/src/content.rs)), and `CidFont::default_width` and `cid_to_gid_map_stream` ([font.rs:348-372](https://docs.rs/crate/pdf-writer/0.15.0/source/src/font.rs)).
- CCITT `DecodeParms` writers cover `k`, `columns`, `rows`, `encoded_byte_align`, `end_of_block` and `black_is_1` ([object.rs:1263-1340](https://docs.rs/crate/pdf-writer/0.15.0/source/src/object.rs)).
- `Pdf::finish(self) -> Vec<u8>` ([lib.rs:307](https://docs.rs/crate/pdf-writer/0.15.0/source/src/lib.rs)). The caller sets the file ID with `set_file_id` (`:280`).
- Numbers are f32 (`push_float(f32)`, [buf.rs:51](https://docs.rs/crate/pdf-writer/0.15.0/source/src/buf.rs)).
- 28 `#[test]`s in `src/`.

**krilla 0.8.2** (2026-06-04; MIT OR Apache-2.0; MSRV 1.92; 3.66M downloads; 42 reverse dependencies; repo 477 stars, 31 open issues, pushed 2026-11-02; `#![forbid(unsafe_code)]`) — [crates.io](https://crates.io/crates/krilla)
- `BitsPerComponent` is only `Eight` or `Sixteen` ([graphics/image.rs:35-40](https://docs.rs/crate/krilla/0.8.2/source/src/graphics/image.rs)). It has no 1-bit images and no CCITT filter.
- `Image::from_jpeg` passes the JPEG through as DCTDecode and sets `invert_cmyk` for CMYK and YCCK, which writes `/Decode [1 0 …]` for each component ([image.rs:497-500, 620-647](https://docs.rs/crate/krilla/0.8.2/source/src/graphics/image.rs); [stream.rs:280](https://docs.rs/crate/krilla/0.8.2/source/src/stream.rs)).
- Text render modes in use are only `Fill`, `Stroke` and `FillStroke` ([content.rs:470-509, 828-836](https://docs.rs/crate/krilla/0.8.2/source/src/content.rs)).
- CID fonts assign one CID per glyph ID through a glyph remapper and keep one text string per CID ([text/cid.rs:150-180](https://docs.rs/crate/krilla/0.8.2/source/src/text/cid.rs)). Text that conflicts goes through `/ActualText` spans ([content.rs:772-805](https://docs.rs/crate/krilla/0.8.2/source/src/content.rs)).
- `Document::finish(self) -> KrillaResult<Vec<u8>>` ([document.rs:143](https://docs.rs/crate/krilla/0.8.2/source/src/document.rs)).
- The file ID and XMP instance ID are a stable hash of the bytes written (`stable_hash_base64(pdf.as_bytes())`, [chunk_container.rs:159-192](https://docs.rs/crate/krilla/0.8.2/source/src/chunk_container.rs)), so the output is deterministic.
- It has PDF/A-1 to 4 and PDF/UA-1 clause tables ([src/configure/PDF_A*.md](https://docs.rs/crate/krilla/0.8.2/source/src/configure/)). Encryption and signatures are out of scope (prior audit [entry 03](https://github.com/sheaf-pdf/sheaf/blob/3232a35/plan/research/build-or-adopt/03-writing-pdf-sheaf-s-serializer-against.md)).

**printpdf 0.12.8** (2026-09-05; MIT; MSRV 1.88; 3.95M downloads; repo 1,191 stars) — [crates.io](https://crates.io/crates/printpdf)
- Resource IDs come from `random_character_string_32()`, fed by a process-global `AtomicUsize` seeded at 2100 ([utils.rs:11-41](https://docs.rs/crate/printpdf/0.12.8/source/src/utils.rs); used at [lib.rs:230-285](https://docs.rs/crate/printpdf/0.12.8/source/src/lib.rs)). Names therefore depend on what else the process made first.
- It has `ImageCompression::Jpeg2000` marked "TODO: uses DCT filter" ([image_types.rs:79-97](https://docs.rs/crate/printpdf/0.12.8/source/src/image_types.rs)) and no CCITT.
- Dependencies include azul-layout, allsorts, lopdf, svg2pdf and rust-fontconfig.
- `save_writer` exists ([lib.rs:750-757](https://docs.rs/crate/printpdf/0.12.8/source/src/lib.rs)), but the document is built first.

**lopdf 0.47.0**:
- `xobject::image_from` (feature `embed_image`) passes a JPEG through as DCTDecode with DeviceGray or DeviceRGB chosen from the `image` crate's color type.
- It strips alpha, writes 16-bit big-endian, and has no CCITT and no EXIF rotation ([xobject.rs:55-140](https://docs.rs/crate/lopdf/0.47.0/source/src/xobject.rs)).
- The document is an in-memory object map.

**genpdf 0.2.0** (2021-06-17; Apache-2.0 OR MIT): no release since — [crates.io](https://crates.io/crates/genpdf).

**deepocr-core 0.1.0** (2026-07-26; MIT OR Apache-2.0; 163 downloads; one release; repo 0 stars) — [crates.io](https://crates.io/crates/deepocr-core)
- It is a "searchable-PDF builder", pure Rust on lopdf, ocrs, rten and hayro.
- It generates its own TrueType: "a few hundred bytes of TrueType tables built here, rather than a third-party binary blob" with `GLYPH_COUNT = 1` ("one empty `.notdef`"), and a `CIDToGIDMap` of all zeros ([searchable/font.rs:1-12, 38-39, 74](https://docs.rs/crate/deepocr-core/0.1.0/source/src/searchable/font.rs)).
- Its content is `BT 3 Tr` and, per word, `/F0 size Tf scale Tz 1 0 0 1 x y Tm <utf16> Tj` ([searchable/mod.rs:165-185](https://docs.rs/crate/deepocr-core/0.1.0/source/src/searchable/mod.rs)).
- Images are DCT, Flate or raw only.

**pdf-ocr 1.3.0** (2026-11-03; "AGPL-3.0-only OR LicenseRef-PDFluent-Commercial"; 391 downloads; leptess and ort) writes its layer in Helvetica with WinAnsi widths and `Tz` ([pipeline.rs:253-601](https://docs.rs/crate/pdf-ocr/1.3.0/source/src/pipeline.rs)). Study only.

**Others from crates.io search** (2026-11-08): ocrs-cjk-cli 0.1.0 ("searchable PDF", 21 downloads), and alvio-ocr and df-ocr-switcher (OCR to Markdown, not PDF). oxidize-pdf 5.5.7 (2026-11-06; MIT; 190k downloads) describes "PDF generation … and PDF/A validation". I didn't read its source — [crates.io search](https://crates.io/search?q=searchable%20pdf).

**Tesseract writes a page at a time** (see Q3): each page's objects are appended as they are made. Only an offsets vector and the page list stay in memory, and the `/Pages` object (number 2) is written at the end ([pdfrenderer.cpp:194-203, 827-870, 922-945](https://github.com/tesseract-ocr/tesseract/blob/main/src/api/pdfrenderer.cpp)).

**sheaf's current writer** ([images.rs](https://github.com/sheaf-pdf/sheaf/blob/3232a35/crates/sheaf-doc/src/images.rs)):
- It uses lopdf objects and `serialize::full::write_built`, with DCTDecode passthrough and `/Rotate` from EXIF quarter turns, G4 under `K -1`, and 8-bit gray or RGB under Flate.
- The page is the unrotated one, with `/Rotate` turning it upright (`:76-91`).

### Inferences
- krilla would need three upstream features to do this job: render mode 3, 1-bit or CCITT images, and a font mode that keeps distinct ToUnicode text per CID for a single glyph. The last runs against its glyph-remapper design. I judge such an upstream contribution unlikely to be a small one.
- pdf-writer is technically sufficient, but adopting it would break the family's one-serializer rule (entry 03). It can't append to an existing file through pdf-structure, and its f32 numbers can't round-trip copied objects. Its whole-file buffer also defeats the "page at a time, memory flat" goal.
- Tesseract's object layout (pages streamed, `/Pages` last with a forward reference) maps directly onto pdf-structure's `write::Parts`.

### Gaps
- I found no krilla issue requesting invisible text. A search returned nothing relevant, and the GitHub API was rate-limited, so I couldn't search its tracker directly.
- I didn't read oxidize-pdf's source, so its PDF/A and image claims are unverified.

## Q3. Non-Rust prior art for searchable PDFs, and the pitfalls it documents

### Takeaway
Tesseract's `pdfrenderer.cpp` (Apache-2.0) is still the reference design: a Type0 font over a two-glyph TrueType, every CID sent to GID 1, `/DW 500`, `Tz` per word, `3 Tr`. sheaf's `Glyphless` copies it faithfully. OCRmyPDF dropped its glyphless hOCR renderer in v17.0.0 (2026-01-30) for fpdf2 with real fonts (Noto by script) and a glyphless fallback, "Occulta". Its 17.x release notes are the best catalogue of viewer pitfalls:
- ToUnicode blocks over 100 entries.
- Poppler losing `Tz` across `BT`/`ET`.
- `Tz` stretched over gaps.
- Bidi ordering of RTL words.
- Glyphless text losing characters in Chrome and PDFium.
- Vertical CJK.

Two spec-level traps also bear on our design: a bfrange like `<0000> <FFFF> <0000>` changes the high byte, and any reference to GID 0 / `.notdef` fails PDF/A-2.

### Cited Findings

**Tesseract `pdfrenderer.cpp`** (Apache-2.0; main branch; repo 77,413 stars, pushed 2026-11-02) — [source](https://github.com/tesseract-ocr/tesseract/blob/main/src/api/pdfrenderer.cpp)
- **Design notes from Ken Sharp:**
  - Map all CIDs to one glyph through a 128 KB `CIDToGIDMap`.
  - "if I use GID 0 then Acrobat gets upset about it and complains it cannot extract the font. If I set the CIDToGIDMap so that all the entries are 1 instead, it's happy."
  - "tools like pdfwrite assume that they are going to be able to (or even have to) modify the font entries … the font Tesseract embeds isn't valid (for this purpose)" (`:44-178`).
- **Objects:**
  - Type0 `/GlyphLessFont` with `/Identity-H`.
  - CIDFontType2 with `/DW 500` and a Flate-compressed `CIDToGIDMap` of `0x0001` × 65,536.
  - FontDescriptor: `/Flags 5`, `/FontBBox [0 0 500 1000]`, `/Ascent 1000`, `/Descent -1` ("Spec says must be negative").
  - ToUnicode with `/CMapName /Adobe-Identify-UCS` (sic), codespace `<0000> <FFFF>` and one `bfrange <0000> <FFFF> <0000>` (`:520-640`).
- **The font program:**
  - `pdf.ttf` from tessdata, with a compiled-in copy as fallback (`:650-662`).
  - I parsed sheaf's vendored copy: 572 bytes, 2 glyphs, advances `[0, 1024]` at 2048 units per em, so glyph 1 advances exactly half an em ([assets/pdf.ttf](https://github.com/sheaf-pdf/sheaf/blob/3232a35/crates/sheaf-doc/assets/pdf.ttf), read with a 20-line table parser).
- **Text** (`:336-500`):
  - `BT 3 Tr` per text block.
  - Words are projected onto the line's straight baseline, because "Viewers like evince can get really confused during copy-paste when the baseline wanders around".
  - Small tilts are clipped to flat for macOS Preview's selection (`ClipBaseline`, rise < 2 pt and run > 2 pt).
  - RTL is handled by mirroring the text matrix (`a = -a; b = -b`) while keeping logical character order.
  - Vertical writing is a TODO ("Consider using the vertical PDF writing mode").
  - The font size is Tesseract's estimate, with a default of 8 because "in Arabic, Tesseract will happily return a fontsize of zero".
  - The inter-word space `0020` is appended inside the word's `TJ` (`:481`).
  - `Tz = kCharWidth × 100 × word_length / (fontsize × pdf_word_len)`, where `pdf_word_len` counts code points, not UTF-16 units (`:465-486`). Non-BMP characters are written as surrogate pairs (`:315-327`).
- **Images** go through Leptonica's `l_generateCIDataForPdf` (`:700-760`):
  - Flate, DCT, CCITT G4 (`/K -1`) or JPX.
  - Indexed `/DeviceRGB` palettes.
  - `/Decode [1 0]` for 1-bit PNG input.
  - "Maybe someday we will accept RGBA but today is not that day."
  - No `/Rotate` is written.

**OCRmyPDF 17.14.0** (2026-11-08; MPL-2.0, so study only; repo 35,269 stars) — [PyPI](https://pypi.org/project/ocrmypdf/), [v17 release notes](https://github.com/ocrmypdf/OCRmyPDF/blob/main/docs/releasenotes/version17.md) (also on [readthedocs](https://ocrmypdf.readthedocs.io/en/latest/releasenotes/version17.html))
- **v17.0.0** (2026-01-30):
  - "Replaced legacy hOCR text renderer with new fpdf2-based implementation."
  - "The new Occulta font provides better handling of zero-width markers and double-width CJK characters."
  - Language-aware font selection for Devanagari, CJK, Arabic and others.
  - Ghostscript became optional ("produces best-effort PDF/A output without requiring Ghostscript when the verapdf validator is available").
  - "Lossy JBIG2 removed … due to well-documented risks of character substitution errors."
  - fpdf2 is LGPL-3.0-only (2.9.0, 2026-10-29, [PyPI](https://pypi.org/project/fpdf2/)).
- **v17.2.0** (2026-02-11):
  - Word spacing broke in Poppler viewers because "Tz (horizontal scaling) is not carried across BT/ET boundaries."
  - OCR text was visible because of a wrong fpdf2 render-mode attribute.
  - The text layer was misaligned with non-zero MediaBox origins.
- **v17.4.0** (2026-03-21): "The horizontal scaling (Tz) was incorrectly stretched to fill inter-word gaps instead of relying on Td positioning, causing text selection to highlight far beyond the actual word boundaries."
- **v17.4.1:** "Arabic lam-alef ligatures and other multi-character CMap entries were garbled by the bidi algorithm during text extraction."
- **v17.12.0** (2026-09-16):
  - Ghostscript 9.56 to 10.04.x discard a whole ToUnicode map when a block exceeds 100 entries ("the CMap specification allows at most 100 entries per block").
  - OCRmyPDF "now splits every CMap block to 100 entries".
  - Ghostscript 10.05.0 to 10.06.x dropped multi-character entries, so ligature letters went missing.
- **v17.13.0** (2026-09-28):
  - "Text set in the glyphless fallback font, used when no installed font covers a script (e.g. CJK), lost characters when selected or copied in Chrome and other pdfium-based viewers."
  - "Copied text from vertical Japanese and other vertical or rotated OCR lines had spurious spaces and characters out of order."
  - `PageInfo.has_visible_text` was added.
  - Speculative PDF/A is now validated with pikepdf's PDF/A support instead of veraPDF.
- **The renderer** ([fpdf_renderer/renderer.py](https://github.com/ocrmypdf/OCRmyPDF/blob/main/src/ocrmypdf/fpdf_renderer/renderer.py)):
  - RTL words are reordered into visual order with fpdf2's `BidiParagraph`, because "Text extractors apply the Unicode bidirectional algorithm to recover the logical order from glyph positions, so glyphs must be laid out left to right in visual order".
  - Complex-script ranges are shaped.
  - Rotation is detected from steep baselines.
  - Invisible text is `TextMode.INVISIBLE`.
- **Font fallback order:** language-preferred font, fallbacks by coverage, any installed font, then "Occulta.ttf (glyphless fallback)" ([font/multi_font_manager.py](https://github.com/ocrmypdf/OCRmyPDF/blob/main/src/ocrmypdf/font/multi_font_manager.py)).
- **Rotation and /UserUnit:**
  - `PageInfo` multiplies the page size by `/UserUnit` and passes a `(userunit, 0, 0, userunit, 0, 0)` CTM into image DPI analysis.
  - It reads `/Rotate`, accepting only cardinal angles ([pdfinfo/info.py:160-275](https://github.com/ocrmypdf/OCRmyPDF/blob/main/src/ocrmypdf/pdfinfo/info.py)).
  - Grafting turns the text layer by the page's `/Rotate` plus any autorotation applied while rasterizing ([_graft.py:78-95](https://github.com/ocrmypdf/OCRmyPDF/blob/main/src/ocrmypdf/_graft.py)).

**hocr-tools `hocr-pdf`** (Apache-2.0 per the file header; PyPI 1.2.0; repo pushed 2024-08-10, 416 stars, GitHub license "NOASSERTION") — [source](https://github.com/ocropus/hocr-tools/blob/master/hocr-pdf)
- It draws the page image with reportlab.
- Per word: `setTextRenderMode(3)`, font `'invisible'` at 8 pt, origin on the hOCR baseline, `setHorizScale(100 × box_width / font_width)`, and `get_display()` (python-bidi visual order) for RTL.
- The font is a "Glyphless variation of vedaal's invisible font", described as "unrestricted freeware", embedded as base64.

**img2pdf 0.6.3** (2025-11-05; LGPL-3.0, so study only) — [README](https://gitlab.mister-muffin.de/josch/img2pdf/raw/branch/main/README.md), [PyPI](https://pypi.org/project/img2pdf/)
- JPEG, JPEG 2000, non-interlaced PNG without transparency, and CCITT G4 TIFF are embedded "direct". Other input goes to PNG Paeth plus Flate, 1-bit to CCITT G4, and CMYK to plain Flate.
- An invalid EXIF Orientation of 0 ("Android phones and Canon DSLR cameras produce JPEG images with the invalid value of zero") is an error unless `--rotation=ifvalid`.
- Pillow's pixel limit guards against decompression bombs.

**Acrobat's output modes**, from Adobe community posts, not official docs:
- "Searchable Image (Exact)" keeps the scan and puts hidden text behind it.
- "Searchable Image" may deskew and clean the image.
- "ClearScan" rebuilds the page as visible fonts and graphics. It can mis-encode ligatures, so "different" comes out as "di erent" ([Adobe community](https://community.adobe.com/t5/acrobat-discussions/recognize-text-output-description-of-options/td-p/12986537); [ClearScan ligatures thread](https://community.adobe.com/t5/acrobat-sdk-discussions/clearscan-not-encoding-ligatures-properly/m-p/8303162)).
- One user reports black boxes when invisible OCR text in a glyphless font is made visible ([Adobe community](https://community.adobe.com/t5/acrobat-discussions/black-box-problems-with-viewing-invisible-text-layer/m-p/14369217)).

**CMap rules as implementers report them:**
- Chromium's PDF writer notes that a bfrange must not change the high byte ([Chromium review](https://codereview.chromium.org/23519006/diff/32001/src/pdf/SkPDFFont.cpp)).
- Skia's ToUnicode writer caps each `bf*` block at 100 entries ([SkPDFMakeToUnicodeCmap.cpp](https://skia.googlesource.com/skia/+/chrome/m124/src/pdf/SkPDFMakeToUnicodeCmap.cpp)).

**PDF/A and .notdef:** veraPDF's check 6.2.11.8-1 fails any PDF/A-2 file whose text-showing operators reference the `.notdef` glyph, "regardless of text rendering mode" ([veraPDF users list](https://lists.verapdf.org/hyperkitty/list/users@lists.openpreservation.org/thread/3MMZ3M4BR2SMSIY4WW24SOUTH2IKBUWD/); [Ghostscript bug 708479](https://bugs.ghostscript.com/show_bug.cgi?id=708479)).

**sheaf's layer** ([glyphless.rs](https://github.com/sheaf-pdf/sheaf/blob/3232a35/crates/sheaf-doc/src/content/font/glyphless.rs); [m05-ocr.md:88-135](https://github.com/sheaf-pdf/sheaf/blob/3232a35/plan/complete/m05-ocr.md)):
- It reproduces Tesseract's objects, with every CID going to glyph 1 and the CMap name spelled `Adobe-Identity-UCS`.
- It computes `tz = 200·w/(fs·n)` with n in UTF-16 units.
- It writes inter-word gaps as a separately scaled `<0020> Tj`.
- It opens the layer with `Q` after a `q` stream wrapped around the original content ("OCRmyPDF's trick").

### Inferences
- **The bfrange.** sheaf's and Tesseract's single `bfrange <0000> <FFFF> <0000>` crosses 255 high-byte boundaries. Viewers tolerate it, but a strict reader or a Ghostscript-like rewriter could drop it. Writing 256 ranges `<hh00> <hhFF> <hh00>` in blocks of at most 100 (100, 100 and 56) keeps the identity mapping and meets both reported rules, for about 7 KB more before Flate.
- **Non-BMP text.** Identity-H with 2-byte codes writes a non-BMP character as two CIDs, which the identity CMap maps to two lone surrogates. Whether PDFium, pdf.js and Poppler rejoin them is untested; I found no source either way. A safer design gives each non-BMP character a private-use CID with a `bfchar` to its UTF-16 pair. sheaf's UTF-16-unit `Tz` count is already more accurate than Tesseract's per-code-point count for such text.
- **Inter-word gaps.** OCRmyPDF's v17.4.0 fix says stretching `Tz` over inter-word gaps makes selection highlight far past words. sheaf scales a separate space glyph over the gap, so the space's own box covers the gap. That probably avoids the bug, but it should be tested in PDFium, pdf.js and Preview.
- **Poppler's `Tz` reset.** The v17.2.0 Poppler bug argues for keeping each line's words inside one `BT … ET`, or setting `Tz` again after every `BT`. sheaf writes one `BT` for the whole layer.
- **RTL.** Tesseract mirrors the text matrix and keeps logical order; OCRmyPDF and hocr-pdf reorder to visual order. Since extractors run bidi on glyph positions, the visual-order approach is the one OCRmyPDF converged on after an issue (#1781). pdf-scan should choose one and test extraction in PDFium.
- **GID 0.** deepocr-core's all-zero `CIDToGIDMap` sends every character to `.notdef`. Ken Sharp says Acrobat refuses that, and veraPDF's 6.2.11.8-1 rejects it for PDF/A-2. Tesseract's GID-1 design avoids both.
- **The Chrome / PDFium regression.** OCRmyPDF's 17.13.0 note on glyphless text losing characters in PDFium-based viewers is the most relevant warning for a PDFium-tested design. The fix wasn't described, so pdf-scan's PDFium round-trip test (Q6) should cover CJK and combining marks, not only Latin.

### Gaps
- I couldn't read the OCRmyPDF commit behind the 17.13.0 Chrome/PDFium fix, or the Occulta font's design. MPL-2.0 restricts copying, not reading, but I didn't get to it, so I can't say what PDFium mishandles.
- I didn't check how PDFium handles lone-surrogate ToUnicode mappings.
- I didn't fetch Tesseract's latest release tag and date: the GitHub API was rate-limited. The code is from the main branch.
- The Acrobat "Searchable Image (Exact)" claim that text sits behind the image comes from community posts, not Adobe documentation.

## Q4. Mixed raster content (MRC) for scans: is it in scope?

### Takeaway
The only maintained MRC-to-PDF tool, Internet Archive's archive-pdf-tools, is AGPL-3.0. didjvu is GPL-2.0. Neither can be ported, and I found no permissive MRC implementation. In PDF terms MRC is small: a background image, a foreground image, and a 1-bit mask as JBIG2 or CCITT. The hard part is segmentation and the codecs. It should stay a later idea, built in-house if ever, after `pdf-scan` and `pdf-images` exist.

### Cited Findings
- **archive-pdf-tools 1.5.7** (2025-08-20; AGPL-3.0; repo 155 stars, 42 open issues, pushed 2026-10-30) — [PyPI](https://pypi.org/project/archive-pdf-tools/), [README](https://github.com/internetarchive/archive-pdf-tools/blob/master/README.rst)
  - It claims "anywhere from 3-15x compression ratios", and an example ratio of 249.9 with a JPEG 2000 background downsampled three times.
  - It describes MRC as "decompose the image into a background, foreground and mask … The mask is a 1-bit image … losslessly compressed (typically using either JBIG2 or CCITT). In a PDF, this usually means the background image is inserted into a page, followed by the foreground image, which uses the mask as its alpha layer."
  - It needs hOCR input ("It is not possible to recode/compress a PDF without hOCR files"), plus OpenJPEG or Grok and jbig2enc.
- **didjvu 0.11.0** (2025-12-07; GPL-2.0-only) — [PyPI](https://pypi.org/project/didjvu/).
- **djvu-rs 0.44.0** (2026-10-31; MIT; 26.9k downloads): a "Pure-Rust DjVu decoder/encoder". DjVu is MRC's sibling format, but this is not a PDF tool, and I didn't read its encoder — [crates.io](https://crates.io/crates/djvu-rs).
- **jbig2enc-rust 0.6.0** (2026-11-02; "MIT AND Apache-2.0"):
  - Its README describes a pure-Rust JBIG2 encoder and decoder with "PDF embedding: globals stream plus page streams" and "pixel-exact symbol coding".
  - Its 0.6.0 notes say "The DjVu-derived component code and tests are replaced".
  - Dependencies include pyo3, rayon and wide, and there are 4 `unsafe` occurrences in `src/` ([crates.io](https://crates.io/crates/jbig2enc-rust); README in the 0.6.0 crate).
  - This is codec territory.
- OCRmyPDF removed lossy JBIG2 entirely in v17.0.0 ([release notes](https://github.com/ocrmypdf/OCRmyPDF/blob/main/docs/releasenotes/version17.md)), and the prior audit already rules out JBIG2 encoding for symbol-substitution risk ([entry 37](https://github.com/sheaf-pdf/sheaf/blob/3232a35/plan/research/build-or-adopt/37-jbig2-decode.md)).

### Inferences
- An MRC page needs from `pdf-scan` only:
  - two image XObjects;
  - a stencil or soft mask (CCITT G4 through `fax` would do, keeping JBIG2 out);
  - content that draws them in order.
- The segmentation, background downsampling and lossy foreground coding are the bulk, and they are image-processing work for `scan-cleanup`, not PDF work. Keeping MRC out of the first versions costs nothing structural.

### Gaps
- I found no permissive MRC segmenter in any language. The search covered crates.io ("mixed raster") and the known Python tools only.

## Q5. Reading scanned PDFs back: how tools decide a page is a scan and detect an existing text layer

### Takeaway
OCRmyPDF and pdf_oxide show the practical rules:
- Interpret the content stream, with forms and inline images.
- Collect each image draw's CTM.
- Treat a page as a scan when its images cover most of the page.
- Treat text as an existing OCR layer when it is drawn in render mode 3 (or 7), or in a font named like a known glyphless font.

Resolution is the image's pixels over its placed size, `/UserUnit` included, with degenerate draws excluded. pdf_oxide's classifier is permissive, so its thresholds can be borrowed. OCRmyPDF's is study only.

### Cited Findings
- **OCRmyPDF's text classification:**
  - Text is invisible when drawn in render mode 3 or 7, "or with a glyphless font such as those used by OCR engines".
  - Glyphless fonts are recognized by `/BaseFont` after stripping a 6-letter subset prefix, against `{'glyphlessfont', 'occulta', 'occulta-regular'}` ([pdfinfo/_contentstream.py:76-83, 145-175](https://github.com/ocrmypdf/OCRmyPDF/blob/main/src/ocrmypdf/pdfinfo/_contentstream.py)).
  - `PageInfo.has_text` counts all text; `has_visible_text` excludes invisible text; `has_vector` flags vector drawing "sometimes used to draw fonts" ([pdfinfo/info.py:212-245](https://github.com/ocrmypdf/OCRmyPDF/blob/main/src/ocrmypdf/pdfinfo/info.py)).
- **OCRmyPDF's image analysis:**
  - Images drawn at graphics-stack depth 0 into a 1×1 unit square are skipped for DPI ("The equivalent DPI will be >100,000").
  - Forms nest to at most 64 levels with at most 100,000 form draws per page.
  - Inline images are found and analyzed too ([pdfinfo/_image.py:263-330](https://github.com/ocrmypdf/OCRmyPDF/blob/main/src/ocrmypdf/pdfinfo/_image.py)).
  - Image DPI comes from the drawing CTM and the pixel size (`_get_dpi(shorthand, (w, h))`). `renderable` excludes stencil masks and non-finite DPI (`:224-246`).
  - The page DPI is the maximum over renderable images ([info.py:197-201](https://github.com/ocrmypdf/OCRmyPDF/blob/main/src/ocrmypdf/pdfinfo/info.py)). v17.13.0 fixed pages of full-page images at different resolutions being rasterized "at a weighted average resolution rather than the highest" ([release notes](https://github.com/ocrmypdf/OCRmyPDF/blob/main/docs/releasenotes/version17.md)).
- **OCRmyPDF's modes:** `--mode` default errors on any text, `skip` skips pages with text, and `redo` strips the old layer (v17.0.0). Since v17.14.0, "Only the invisible text is now removed" when stripping ([release notes](https://github.com/ocrmypdf/OCRmyPDF/blob/main/docs/releasenotes/version17.md)).
- **pdf_oxide's `classify_page`** (MIT OR Apache-2.0) ([extractors/auto.rs:646-910](https://docs.rs/crate/pdf_oxide/0.3.78/source/src/extractors/auto.rs)):
  - Its signals:
    - `image_area_ratio` ("Σ CTM-transformed image-box area ∩ page ÷ page area … summed not max'd → multi-strip scans … are caught");
    - `invisible_text_ratio` ("Tr-mode-3 (invisible) glyphs ÷ all glyphs");
    - a garbled-text ratio, a fragmented-word ratio, path density and count, the dominant codec, and a producer prior.
  - Its cascade:
    - `scan_cover_min = 0.80`;
    - a usable text layer needs `min_glyphs = 24`, `garbled_ratio <= 0.20` and `fragmented_word_ratio <= 0.80`;
    - `invisible_text_ratio >= 0.50` over a scan means an OCR layer exists;
    - CCITT and JBIG2 raise the scan confidence to 0.95.
  - `needs_ocr` returns true for scanned and hybrid pages ([ocr/mod.rs:121-154](https://docs.rs/crate/pdf_oxide/0.3.78/source/src/ocr/mod.rs)).
- **Poppler's `pdfimages -list`** gives per image the x-ppi and y-ppi "when rendered on the pdf page", plus type, color, comp, bpc, enc and object ID ([man page](https://manpages.debian.org/testing/poppler-utils/pdfimages.1.en.html)).
- **PDFium:** `FPDFImageObj_GetImageMetadata` reports horizontal and vertical DPI, bits per pixel and color space per image object. pdfium-render exposes these, and text objects' `render_mode()` ([image.rs:760-830](https://docs.rs/crate/pdfium-render/0.9.4/source/src/pdf/document/page/object/image.rs); [text.rs:241](https://docs.rs/crate/pdfium-render/0.9.4/source/src/pdf/document/page/object/text.rs)).
- **The plan:** "its content one image over the page, maybe with a text layer, as copiers, ScanSnap, OCRmyPDF and Tesseract write them" ([crates/pdf-scan.md](../../../crates/pdf-scan.md)).

### Inferences
- `pdf-scan`'s reader can be exact where these tools are heuristic, because it targets a narrow shape: one image draw covering the MediaBox or CropBox (or several strips summing to it), an optional invisible text block, and nothing else visible. That narrowness is a feature. Pages that fail it should fall back to "not a scan" with a reason, the way pdf_oxide reports a `ReasonCode`.
- Detecting an existing layer by render mode 3 or 7 plus the font names `GlyphLessFont` and `Occulta` (after the subset prefix) are facts about the files these tools write, not copyrightable code. Using them is compatible with "study only".
- Resolution: x-ppi = `Width / (|placed width in points| × UserUnit / 72)`, taking the CTM's axes after `/Rotate`. That matches what pdfimages and OCRmyPDF report. OCRmyPDF's unit-square exclusion is a cheap guard worth copying as an idea.

### Gaps
- I didn't read pdfimages' x-ppi formula in source (GPL, study only), only its man page definition.
- I found no published accuracy measurement for either classifier.

## Q6. Test approaches

### Takeaway
The plan's tests match what the mature tools use:
- PDFium as a rendering oracle;
- text-extraction round trips;
- qpdf `--check` for structure;
- veraPDF, once PDF/A comes.

PDFium's per-image metadata (DPI, bpp, color space) and per-text-object render mode make it a direct oracle for the reader too. Two more oracles: PDFBox's `getRawRaster()` gives native samples to compare `pdf-images` against, and pdf-extract extracts invisible text with no render-mode filter.

### Cited Findings
- **qpdf `--check`** checks "the file's structure as well as encryption, linearization, and encoding of stream data", and "does not perform any validation of the actual PDF page content" ([cli.rst:3380-3392](https://github.com/qpdf/qpdf/blob/main/manual/cli.rst)). `--decode-level=all` decodes DCT but not CCITT, JBIG2 or JPX (Q1).
- **veraPDF** core 1.30.3 (Maven metadata last updated 2026-10-08) — [maven-metadata.xml](https://repo1.maven.org/maven2/org/verapdf/core/maven-metadata.xml). OCRmyPDF had to fix veraPDF detection for 1.30.0 and later, because "recent builds print JVM warnings before their version string" (v17.8.0). It replaced veraPDF with pikepdf's validator for its speculative PDF/A in v17.13.0 ([release notes](https://github.com/ocrmypdf/OCRmyPDF/blob/main/docs/releasenotes/version17.md)).
- **krilla's CI** checks against veraPDF and the Arlington model (prior audit [entry 03](https://github.com/sheaf-pdf/sheaf/blob/3232a35/plan/research/build-or-adopt/03-writing-pdf-sheaf-s-serializer-against.md)).
- **pdfium-render** offers raw image data, filter names, DPI, bpp, color space and text render mode (Q1, Q5).
- **pypdfium2 5.15.1** (2026-11-07) is OCRmyPDF's default rasterizer since v17.0.0 ("The default `--rasterizer auto` prefers pypdfium when available") — [PyPI](https://pypi.org/project/pypdfium2/), [release notes](https://github.com/ocrmypdf/OCRmyPDF/blob/main/docs/releasenotes/version17.md).
- **OCRmyPDF's output validation** checks JPEG 2000 and CCITT images too since v17.13.0 ("JPEG 2000 and CCITT images are now checked too") ([release notes](https://github.com/ocrmypdf/OCRmyPDF/blob/main/docs/releasenotes/version17.md)).
- **PDFBox `getRawRaster()`** returns native samples (Q1).
- **hayro** tests against 1,600+ PDFs scraped partly from pdf.js's and PDFBox's suites ([README](https://github.com/LaurenzV/hayro/blob/main/README.md)).
- **The family** already runs PDFium (`oracle/`) and qpdf (pdf-fixtures), and fuzzes every crate in CI (pdf-lint's `README.md`).

### Inferences
- **Text round-trip cases to add**, beyond the plan's PDFium extraction:
  - RTL (Hebrew with digits);
  - CJK;
  - combining marks;
  - non-BMP characters (emoji, CJK Extension B);
  - vertical lines;
  - a word spanning a `BT` boundary.

  Each one maps to a documented OCRmyPDF regression or a Tesseract comment (Q3). Running the same files through pdf.js and Poppler's `pdftotext` would catch Poppler's `Tz` reset issue. Those are external programs used only as test oracles; Poppler is GPL, so it is never linked.
- **A PDF/A check now:** a veraPDF run on a PDF/A-2b candidate would catch `.notdef` references (6.2.11.8-1) and width mismatches, even before PDF/A is in scope.
- **Determinism:** writing the same inputs twice, in one process and across processes, and comparing bytes would catch the class of bug printpdf's global counter has.

### Gaps
- I didn't confirm veraPDF's license text in this session.
- I didn't confirm pdf.js's text-extraction treatment of render mode 3 in source.

## Q7. What in the 2026-10-23 audit is wrong or stale?

### Takeaway
The audit's shape holds: build on the family, port hayro's quirks, keep PDFium as the oracle. Its version numbers and several factual details are now stale or imprecise:
- the hayro family moved to 0.9.0 on 2026-11-02;
- hayro-syntax's image decode is partly public;
- JPX's native depth is reachable;
- the default build has no SIMD;
- hayro-interpret depends on skrifa through hayro-font, not directly;
- hayro's README wording changed.

### Cited Findings
- **Versions:**
  - hayro, hayro-syntax, hayro-interpret and hayro-write are 0.9.0 (2026-11-02), not 0.8.0.
  - hayro-ccitt is 0.5.0 (audit: 0.4.0), hayro-jbig2 0.4.0 (audit: 0.3.1) and hayro-jpeg2000 0.5.0 (audit: 0.4.1).
  - pdfium-render is 0.9.5 (2026-11-01; audit: 0.9.4) and lopdf 0.47.0 (2026-11-04).
  - Source: [crates.io API](https://crates.io/crates/hayro-syntax), read 2026-11-08.
- **Entry 33, "its `filter::Filter::apply` is crate-private, so sheaf can't call it":** still true ([filter/mod.rs:65](https://docs.rs/crate/hayro-syntax/0.9.0/source/src/filter/mod.rs)). But `Stream::decoded_image(&ImageDecodeParams)` is public and runs the full chain, codecs included. It works only on hayro's own `Stream`, because `Stream::new` is `pub(crate)` ([stream.rs:58, 197](https://docs.rs/crate/hayro-syntax/0.9.0/source/src/object/stream.rs)). Same conclusion, more precise reason.
- **Entry 33 / 91, "Every crate is pure Rust, safe at the crate level":** hayro-syntax 0.9.0 has no `forbid(unsafe_code)`, has a `transmute` at page.rs:528, and depends on smallvec.
  - Its new non-default `unsafe` feature gates flate2/zlib-rs, memchr and the codecs' SIMD.
  - So the default hayro-syntax build is SIMD-free, contrary to entry 91's "the codecs' default `simd` feature uses `fearless_simd`". Used directly, the codec crates still default to `simd`.
  - Sources: [Cargo.toml](https://docs.rs/crate/hayro-syntax/0.9.0/source/Cargo.toml.orig), [lib.rs:50-58](https://docs.rs/crate/hayro-syntax/0.9.0/source/src/lib.rs), [hayro-jbig2 README](https://docs.rs/crate/hayro-jbig2/0.4.0/source/README.md).
- **Entry 36, "It yields 8-bit only, so 16-bit JPX drops to 8-bit":** true of `data_u8()` and of hayro-syntax's glue. hayro-jpeg2000 0.5.0's `components()` gives f32 samples with their `bit_depth`, so `pdf-images` can keep 16-bit ([lib.rs:355-395](https://docs.rs/crate/hayro-jpeg2000/0.5.0/source/src/lib.rs); [j2c/mod.rs:33-48](https://docs.rs/crate/hayro-jpeg2000/0.5.0/source/src/j2c/mod.rs)).
- **Entry 91, "hayro-interpret uses moxcms, skrifa (since 2026-04, dropping hayro-font)":** in 0.9.0 hayro-interpret depends on hayro-font 0.5.0, which depends on skrifa. hayro-interpret has no direct skrifa dependency ([hayro-interpret Cargo.toml](https://docs.rs/crate/hayro-interpret/0.9.0/source/Cargo.toml.orig); hayro-font 0.5.0's Cargo.toml lists skrifa).
- **Entry 91, the README quotes:** the README now claims "1,600+ PDFs" (audit: 1,400+) and no longer reads "experimental, work-in-progress". It now lists robustness ("many ways to trigger timeouts and OOM errors"), performance and API surface as open ([README](https://github.com/LaurenzV/hayro/blob/main/README.md)). The repo has 858 stars (audit: 814).
- **Entry 33, pdf_oxide's downloads:** now 1.53M (audit: 1.35M). Not noted before: its image extractor counts DeviceN as 4 components regardless of N ([images.rs:670](https://docs.rs/crate/pdf_oxide/0.3.78/source/src/extractors/images.rs)).
- **Entry 37's "under development" note** for hayro-jbig2 still holds in 0.4.0 ([README](https://docs.rs/crate/hayro-jbig2/0.4.0/source/README.md)).
- **Version split:** `pdf-content-streams` pins `hayro-ccitt = "0.3"` (pdf-lint's `crates/pdf-content-streams/Cargo.toml`, line 41), while the current release is 0.5.0. crates.md's "keep one version of each across the two workspaces" therefore already needs a bump (then crates.md; now [crates/pdf-images.md](../../../crates/pdf-images.md), as PL-3).
- **Entry 03's pdf-writer and krilla facts** still hold: pdf-writer 0.15.0 of 2026-05-27 and krilla 0.8.2 of 2026-06-04 are still the latest.

### Inferences
- None of these change the audit's recommendations. They change the porting plan:
  - port hayro-syntax's per-filter glue, not call it;
  - keep 1-bit samples packed;
  - use `components()` for JPX;
  - leave the codecs' `simd` feature as a deliberate choice.

### Gaps
- I didn't re-check every number in entries 34 to 38. Those codecs are the other researcher's topic.

## Q8. Verdicts: for each need, use directly, use and contribute upstream, port, build our own, or study only

### Takeaway
Build both crates on the family. Port small permissive pieces: hayro-syntax's filter glue, hayro-interpret's sample unpacking and length repair, pdf.js's Matte and SMaskInData handling, Tesseract's glyphless-font design, and pdf_oxide's scan signals. Use PDFium, qpdf and pdf-extract as test oracles. Everything copyleft is study only, and no public crate does this end to end well enough to replace the family's object layer.

### Cited Findings
The verdicts rest on the findings in Q1 to Q7. The decisive facts:

**`pdf-images`:**
1. **Dictionary read and capped before decoding** (`Width`, `Height`, `BitsPerComponent`, `ColorSpace`, `Decode`, `ImageMask`, `Mask`, `SMask`, `SMaskInData`, `Interpolate`, inline abbreviations). **Verdict: build our own** on pdf-objects and pdf-color. hayro has no area cap and pdf.js targets canvas (Q1). Port, as ideas: hayro's DCT dimension guard and patch, its short-data padding, and its name-resolved inline color spaces ([hayro-interpret decode/mod.rs](https://docs.rs/crate/hayro-interpret/0.9.0/source/src/x_object/decode/mod.rs); [dct.rs](https://docs.rs/crate/hayro-syntax/0.9.0/source/src/filter/dct.rs)).
2. **Stream filters.** **Verdict: use directly** (pdf-filters, the family) (pdf-lint's `README.md`).
3. **DCT at PDF level** (`/ColorTransform`, APP14, YCCK, wrong SOF dimensions, the 65,535 cap). **Verdict: port** hayro-syntax `filter/dct.rs` (Apache-2.0 OR MIT, 181 lines) onto zune-jpeg. Its comment says the AI-generated marker scanner needs review, so write tests for it ([dct.rs](https://docs.rs/crate/hayro-syntax/0.9.0/source/src/filter/dct.rs)).
4. **CCITT parameters.** **Verdict: port** `filter/ccitt.rs`'s parameter mapping and partial-row recovery, but keep 1-bit packed output always ([ccitt.rs](https://docs.rs/crate/hayro-syntax/0.9.0/source/src/filter/ccitt.rs)).
5. **JBIG2 with `/JBIG2Globals`.** **Verdict: port** `filter/jbig2.rs`'s glue (globals decoded as data, then `Page::new_embedded`, then the black=1 inversion), with 1-bit output ([jbig2.rs](https://docs.rs/crate/hayro-syntax/0.9.0/source/src/filter/jbig2.rs)).
6. **JPX, `SMaskInData`, 16-bit.** **Verdict: port** `filter/jpx.rs`'s alpha split, reading hayro-jpeg2000's `components()` at native depth instead of `data_u8()`. Take SMaskInData 1 vs 2 semantics from pdf.js `image.js` (Apache-2.0) ([jpx.rs](https://docs.rs/crate/hayro-syntax/0.9.0/source/src/filter/jpx.rs); [image.js](https://github.com/mozilla/pdf.js/blob/master/src/core/image.js)).
7. **Samples in the image's own space** (1 to 16 bits, Indexed kept as indices, Separation and DeviceN kept as N channels, ICC kept with its profile). **Verdict: build our own**; no permissive Rust candidate exposes this (Q1). Port hayro-interpret's `unpack_samples` and `fix_image_length` (Apache-2.0 OR MIT). PDFBox's `getRawRaster` is the API model ([PDImage.java](https://github.com/apache/pdfbox/blob/trunk/pdfbox/src/main/java/org/apache/pdfbox/pdmodel/graphics/image/PDImage.java)).
8. **`/ImageMask`, `/SMask`, color-key `/Mask`, `/Matte`.** **Verdict: build our own**, porting pdf.js's Matte un-premultiply and corrupt-SMask fallback dimensions (Apache-2.0) ([image.js:105-140, 354-357](https://github.com/mozilla/pdf.js/blob/master/src/core/image.js)).
9. **The whole job by hayro-syntax or hayro-interpret.** **Verdict: study only, as an alternative**:
   - hayro-syntax would mean a second parser beside pdf-structure;
   - it widens CCITT, JBIG2 and JPX to 8-bit;
   - hayro-interpret outputs only 8-bit RGB or luma.

   **Use and contribute upstream** is plausible for one item: a public `Filter::apply` over raw bytes and a dictionary, or a 1-bit option. It would let others reuse the glue, but `pdf-images` shouldn't wait on it ([filter/mod.rs:65](https://docs.rs/crate/hayro-syntax/0.9.0/source/src/filter/mod.rs)).
10. **pdf-rs, lopdf, pdf_oxide, pdf-extract for images.** **Verdict: study only**:
    - pdf-rs panics on G3 and narrows widths to `u16`;
    - lopdf returns raw bytes only;
    - pdf_oxide has a DeviceN bug and pulls in C dependencies;
    - pdf-extract has no image support (Q1).
11. **Oracles.** **Verdict: use directly** PDFium through pdfium-render 0.9.5 (dev-only; C++ binary): bitmaps compared after PDFium's conversion, plus DPI, bpp and color-space metadata. **Study only**: PDFBox raw rasters (Java; an optional second oracle, not linked), Poppler's `pdfimages` and MuPDF (copyleft).

**`pdf-scan` writing:**

12. **JPEG passthrough with `/Rotate` from EXIF.** **Verdict: build our own** (move `sheaf_doc::images`' logic). Learn from img2pdf (study only) to treat EXIF Orientation 0 as "none" with a warning, and from krilla (MIT OR Apache-2.0) to write `/Decode [1 0 1 0 1 0 1 0]` for Adobe CMYK and YCCK JPEGs ([img2pdf README](https://gitlab.mister-muffin.de/josch/img2pdf/raw/branch/main/README.md); [krilla image.rs:497-500, 646](https://docs.rs/crate/krilla/0.8.2/source/src/graphics/image.rs)).
13. **Black and white as CCITTFaxDecode `K -1`; gray and RGB under Flate.** **Verdict: build our own**: dictionaries through pdf-structure, Flate through pdf-filters. Tesseract and img2pdf make the same choices ([pdfrenderer.cpp:700-760](https://github.com/tesseract-ocr/tesseract/blob/main/src/api/pdfrenderer.cpp)).
14. **The glyphless text layer.** **Verdict: port** Tesseract's design (Apache-2.0): Type0 with Identity-H, CIDFontType2, `/DW 500`, every CID to GID 1, and `pdf.ttf` (572 bytes, 2 glyphs, Apache-2.0) or an equivalent generated two-glyph font. Move sheaf's `Glyphless` with these fixes:
    - split the ToUnicode `bfrange` into per-high-byte ranges, at most 100 per block;
    - decide non-BMP handling (private CIDs with `bfchar`);
    - pick an RTL strategy (visual order, OCRmyPDF's) and test it;
    - keep `Tz` per `BT`;
    - never map to GID 0 ([pdfrenderer.cpp](https://github.com/tesseract-ocr/tesseract/blob/main/src/api/pdfrenderer.cpp); [glyphless.rs](https://github.com/sheaf-pdf/sheaf/blob/3232a35/crates/sheaf-doc/src/content/font/glyphless.rs); Q3).
15. **The layer emitted into a pdf-content-streams writer.** **Verdict: build our own** (the family's writer).
16. **A page at a time, memory flat.** **Verdict: build our own** on pdf-structure's `write::Parts`, following Tesseract's stream-then-`/Pages`-last layout. pdf-writer, krilla, lopdf and printpdf all buffer the whole document (Q2).
17. **The same inputs make the same bytes.** **Verdict: build our own**:
    - no clocks or random IDs;
    - `/ID` from a content hash, krilla's approach;
    - `/Producer` and `/CreationDate` from the caller;
    - avoid printpdf's global-counter pattern ([krilla chunk_container.rs:159-192](https://docs.rs/crate/krilla/0.8.2/source/src/chunk_container.rs); [printpdf utils.rs](https://docs.rs/crate/printpdf/0.12.8/source/src/utils.rs)).
18. **An end-to-end Rust writer as the alternative.** **Verdict: study only**:
    - krilla (missing `Tr 3`, 1-bit and CCITT; its glyph remapper conflicts with glyphless text);
    - pdf-writer (a second serializer, f32 numbers, whole-file buffer);
    - deepocr-core (`.notdef` mapping, one release);
    - pdf-ocr (AGPL);
    - printpdf and genpdf.

    krilla's PDF/A clause tables remain the reference for later PDF/A work (Q2; [entry 03](https://github.com/sheaf-pdf/sheaf/blob/3232a35/plan/research/build-or-adopt/03-writing-pdf-sheaf-s-serializer-against.md)).

**`pdf-scan` reading:**

19. **Is the page a scan; its image, placement and resolution.** **Verdict: build our own** on pdf-content-streams. Borrow pdf_oxide's signal list and thresholds (MIT OR Apache-2.0: ≥ 0.80 coverage summed over draws) and OCRmyPDF's ideas (study only): the unit-square exclusion, `/UserUnit`, max DPI ([auto.rs:646-910](https://docs.rs/crate/pdf_oxide/0.3.78/source/src/extractors/auto.rs); [_image.py](https://github.com/ocrmypdf/OCRmyPDF/blob/main/src/ocrmypdf/pdfinfo/_image.py)).
20. **Does it already have a text layer.** **Verdict: build our own**: render mode 3 or 7, plus `/BaseFont` `GlyphLessFont` or `Occulta` after the subset prefix, plus a ratio of invisible glyphs ([_contentstream.py](https://github.com/ocrmypdf/OCRmyPDF/blob/main/src/ocrmypdf/pdfinfo/_contentstream.py); [auto.rs](https://docs.rs/crate/pdf_oxide/0.3.78/source/src/extractors/auto.rs)).
21. **MRC.** **Verdict: study only now; build our own later** if ever. archive-pdf-tools is AGPL and didjvu GPL (Q4).
22. **PDF/A for scans.** **Verdict: study only for now** (OCRmyPDF, Ghostscript: copyleft); krilla's clause tables (permissive) when it comes. Run veraPDF as an external validator in tests when PDF/A starts, to catch `.notdef` and width rules (Q3, Q6).
23. **Structure check.** **Verdict: use directly** qpdf `--check` (already run by pdf-fixtures) and pdf-lint. Text round trips: PDFium extraction, plus pdf-extract (MIT) as a second, render-mode-blind extractor ([qpdf cli.rst](https://github.com/qpdf/qpdf/blob/main/manual/cli.rst); [pdf-extract lib.rs](https://docs.rs/crate/pdf-extract/0.12.1/source/src/lib.rs)).

### Inferences
- The ported pieces are small. hayro-syntax's four image filters are 181 (dct.rs) + 256 (ccitt.rs) + 112 (jbig2.rs) + 130 (jpx.rs) lines, and Tesseract's font block is about 150 lines. Porting, with attribution in the source, is cheaper than a dependency that brings a second parser.
- The riskiest verdicts:
  - The RTL strategy in the text layer: the two prior-art approaches disagree.
  - PDFium's handling of glyphless CJK text: OCRmyPDF's 17.13.0 regression, cause unknown.

  Both need tests before the layer is called done.

### Gaps
- I couldn't verify that the Chrome/PDFium regression affects Tesseract-style GID-1 fonts, as opposed to OCRmyPDF's Occulta.

## Q9. Holes in the ecosystem

### Takeaway
Six needs have no adequate permissive or pure-Rust implementation:
- native-sample PDF image decoding on a caller's object model;
- a correct, deterministic, streaming writer for searchable scanned PDFs;
- a permissive scan and OCR-layer classifier outside a kitchen-sink crate;
- any permissive MRC;
- a tested answer for non-BMP and RTL text in glyphless layers;
- PDF/A output for scans without copyleft tools.

### Cited Findings
- **Native samples:** no Rust crate returns an image's samples in its own color space, depth and /Decode for objects it didn't parse. hayro-syntax's `decoded_image` is tied to its own `Stream` and widens to 8-bit; hayro-interpret, PDFium and pdf.js output 8-bit RGB or RGBA; pdf_oxide converts and miscounts DeviceN. The only native-sample API found is PDFBox's `getRawRaster()`, in Java (Q1).
- **No input cap:** no candidate caps decode work from the dictionary before decoding, beyond hayro's 65,535-pixel DCT guard (Q1).
- **No writer:**
  - krilla lacks render mode 3, 1-bit and CCITT;
  - pdf-writer and the others buffer whole files;
  - printpdf's names aren't reproducible across a process;
  - deepocr-core maps to `.notdef`;
  - pdf-ocr is AGPL.

  So no Rust crate writes a searchable scanned PDF (CCITT for bilevel, glyphless Type0 layer, page at a time, deterministic) that we could use (Q2).
- **No classifier:** the only permissive scan and OCR-layer classifier found sits inside pdf_oxide, with its large C and ML dependency set (Q5).
- **No MRC:** the known implementations are AGPL or GPL (Q4).
- **Untested edge cases:** no source documents how PDFium, pdf.js and Poppler extract non-BMP text written as two Identity-H CIDs with an identity ToUnicode. OCRmyPDF reports, without details, that glyphless-font text "lost characters" in PDFium-based viewers (Q3).
- **No PDF/A path:** OCRmyPDF's route uses Ghostscript (AGPL) or pikepdf's validator (MPL-2.0); krilla writes PDF/A only for documents it creates (Q3, Q2).

### Inferences
- The first two holes are exactly `pdf-images` and `pdf-scan`'s writer. That confirms the plan: there is nothing to adopt in place of them, only pieces to port.
- The text-layer edge cases are the one hole that needs experiments, not code reading. A small matrix should be built before pdf-scan's text layer is declared done:
  - BMP vs non-BMP;
  - LTR vs RTL (visual vs logical order);
  - horizontal vs vertical;
  - PDFium vs pdf.js vs Poppler.

### Gaps
- I didn't search the C and C++ world beyond Tesseract, qpdf, PDFium, Poppler and MuPDF for a permissive MRC or native-sample library: for example Leptonica (BSD-2), which Tesseract uses to write its images. Leptonica's PDF writer (`l_generateCIDataForPdf`) is a candidate worth a later look.
