# pdf-images

Layer: PDF (`crates/README.md`). Paths are from `docs/pm/`.

- **What:** an image XObject's or inline image's samples, in its own color space, bit depth and `/Decode`
  (entry 33), not PDFium's 8-bit gray or BGR (which turns CMYK, Indexed, Separation, DeviceN, ICCBased and
  16-bit images into something else, and returns DCT, JPX, JBIG2 and CCITT still encoded).
  - The filter chain read here and its standard filters run through pdf-filters, stopping at the codec.
    pdf-structure's chain reader is private (`filter.rs:138`) and refuses a chain that ends in a codec
    whole, and undecoded data is only on the concrete `Reading::data`; so `pdf-images` copies the chain
    reading (about 150 lines, with credit), expands inline images' abbreviations itself, and takes `&Reading`
    until PL-1 lands (`pdf-lint/README.md`).
  - Then the image codecs, through glue ported from hayro-syntax 0.9.0's `filter/` (MIT OR Apache-2.0):
    - DCT through zune-jpeg, after `dct.rs` (181 lines: the 65,535 guard, SOF dimension patching,
      `/ColorTransform`, Adobe's APP14, YCCK). Its marker scanner is marked AI-generated: tests of our own.
      12-bit JPEGs, which zune-jpeg refuses, are a typed refusal, as in `scan-image`.
    - CCITT through the hayro-ccitt adapter `scan-image` shares, after `ccitt.rs` (256 lines: parameter
      mapping, partial rows recovered), with 1-bit output kept packed where hayro widens it to 8 bits;
      `BlackIs1` is our inversion since hayro-ccitt 0.5.0.
    - JBIG2 through hayro-jbig2 0.4.0's `File`, `Page` and `Globals` (`/JBIG2Globals`), after `jbig2.rs`
      (112 lines).
    - JPX through hayro-jpeg2000 0.5.0's `components()`, which gives f32 samples at their native depth, so
      16-bit JPX keeps its 16 bits; `SMaskInData`'s alpha split after `jpx.rs` (130 lines), its semantics
      from pdf.js (Apache-2.0).
  - Samples unpacked after hayro-interpret's `unpack_samples`, short data repaired after its
    `fix_image_length`; `/Matte` un-premultiplied and corrupt soft masks' fallback dimensions after pdf.js.
    The API modelled on PDFBox's `getRawRaster` (Java, Apache-2.0), the one native-sample API found.
  - Color spaces through pdf-color: typed spaces, components as PDFium counts them, base and alternate
    spaces, Indexed's `hival` and where its lookup is (a stream lookup's data through `Streams::decoded`).
    Default `/Decode` per space and Lab's and ICCBased's `/Range` are ours: pdf-color holds none (PL-1's
    smaller points).
  - Inline images located by pdf-content-streams, as they are today.
  - Dimensions read before anything is decoded, to cap the work: `Width × Height × components × bpc`. No
    library caps this beyond hayro's DCT guard, so the cap is ours alone.
  - Never JBIG2 encoded (entry 37: lossy symbol matching swapped digits in Xerox's PDFs).
  - A codec's panic reaches its library caller: `pdf-images` fuzzes each codec and decides on
    `catch_unwind` itself.
- **Prior art:** none returns native samples over the caller's objects. hayro-syntax's public
  `Stream::decoded_image` works only on streams its own parser made (`Stream::new` is crate-private) and
  widens CCITT, JBIG2 and JPX to 8 bits; adopting it would put a second parser beside pdf-structure. pdf-rs
  panics on CCITT with `K >= 0` and narrows widths to `u16`; lopdf returns encoded bytes; pdf_oxide counts
  every DeviceN space as 4 components.
- **Users:** `pdf-scan`'s reader; sheaf's M11 redaction (slice 6: pixels replaced in the image's own
  samples), M13's image edits, M14's downsampling and recompression.
- **Tested by:** PDFium's bitmaps through pdfium-render 0.9.5 (a dev-dependency), compared after the
  conversion PDFium makes (pdf-lint's oracle runs a pinned PDFium over hand-made cases but renders nothing:
  the bitmap calls are ours, its PDFium pin can be shared); PDFBox's raw rasters as an optional
  second oracle, run as a process, never linked; the codecs' conformance sets (T.88 Annex K, openjpeg-data);
  the corpus (`scans/` has 464 files with DCT, 150 CCITT, 127 JBIG2, 52 JPX); fuzzing, the hayro codecs
  through our adapters (TEST-4).
- **Open:**
  - hayro-jbig2 0.4.0 still calls itself "under development", but reads multi-page files, shares
    `Globals`, has a strict mode and is fuzzed: the label is mostly cosmetic.
  - pdf-content-streams takes zune-jpeg and hayro-ccitt for `image-ends`, sharing versions, not code (its
    decoders are crate-private and keep nothing), and pins `hayro-ccitt = "0.3"`, two breaking releases
    behind 0.5.0: one version across the two workspaces is PL-3 (Q-1). Cargo unifies features, so turning on
    zune-jpeg's SIMD here turns it on for `image-ends` too.
  - Publishes once pdf-structure's first alpha is out, and pdf-content-streams' for inline images;
    pdf-objects, pdf-filters and pdf-color are on crates.io already, pinned exactly.
  - Whether hayro-jpeg2000's `components()` come before or after its color conversion and upsampling: read
    or test it.
