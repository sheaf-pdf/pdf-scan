# pdf-scan

Layer: PDF (`crates/README.md`). Paths are from `docs/pm/`.

- **What:** scanned pages written to PDF, and scanned PDFs' page images found and read back.
  - **Writing pages:** `scan-image` pages as PDF pages (`sheaf_doc::images`): a JPEG as it is under
    DCTDecode, its EXIF turn as `/Rotate` (Orientation 0 read as none), Adobe CMYK and YCCK with an
    inverting `/Decode` (krilla's approach); black and white as CCITTFaxDecode, `K -1`; gray and RGB under
    Flate (pdf-filters; the backend by measurement: miniz_oxide forbids `unsafe`, zlib-rs is faster with
    about 456 `unsafe` sites, and D4 sets zlib-rs for sheaf; named at every call, since pdf-structure
    defaults to miniz_oxide and pdf-content-streams forces zlib-rs for inline images). Each page sized from
    its resolution or fitted to a paper size. Page and image dictionaries are ours, on the family's object
    writer.
  - **The text layer:** words with boxes (from any engine) as invisible text, render mode 3, in a Type0 font
    over Identity-H with an embedded glyphless TrueType, `/DW 500` (every CID advances half an em), every
    CID sent to glyph 1, never glyph 0 (Acrobat "gets upset", and veraPDF's PDF/A-2 rule 6.2.11.8-1 fails
    any `.notdef` reference), each word fitted to its box by horizontal scaling alone, so any Unicode text
    round-trips. Tesseract's design and its `pdf.ttf` (Apache-2.0), as sheaf's `Glyphless` has them, with
    these changes from sheaf's (OCRmyPDF's 17.x release notes, MPL-2.0, study only, are the catalogue):
    - ToUnicode as 256 ranges, one per high byte, at most 100 entries to a block (BUG-6): sheaf's single
      `<0000> <FFFF> <0000>` crosses 255 high-byte boundaries, which Chromium's PDF writer forbids, and
      Ghostscript 9.56 to 10.04 drops a whole map with any block over 100.
    - `Tz` set again after every `BT`: Poppler doesn't carry it across `BT`/`ET`. sheaf writes one `BT`
      per layer; a caller's writer may not.
    - Right-to-left words (Q-2) and text beyond the Basic Multilingual Plane (Q-3) by rules the viewer
      matrix decides (TEST-1).
    - Emitted into a pdf-content-streams writer the caller gives, so a tool with content of its own (sheaf,
      after M18 slice 19) writes the layer through the same calls. Every operator it uses is there; `TJ`
      only through `operation`; numbers f32 to 4 decimals. The font's objects and its CMap, and its
      `ResourceNeeds` bound into the page's `/Resources`, are ours: the family writes no fonts, and nothing
      binds resources yet.
  - **Through pdf-structure's writer:** a whole file (`write::full`); a page at a time, so memory stays flat
    over a feeder's worth, in Tesseract's layout (pages streamed out as made, `/Pages` written last); or
    appended to an existing file as an incremental update. `full` and `update` hold every body, and
    `write::Parts` writes bodies only, so the streamed layout (header, offsets, cross-reference section,
    trailer, `/ID` hashed as written to match `file_id`) is assembled here, about 200 lines held byte-equal to
    `full`, until PL-2 lands; a streamed update is the same over `Section::append` and `Base`.
  - **The page tree** is ours until the family writes one.
  - **The same inputs make the same bytes.** sheaf's memory sources rely on it: an insert lowered again finds
    the pages it made (M5 `in-place.md`). No clock and no random id inside: the family writer already makes `/ID`'s
    second string from SHA-256 of what's written, and gives a new file in the clear no `/ID` unless the
    trailer has a first string, so we give one; `/Producer` and `/CreationDate` from the caller (FEAT-3).
    Same bytes in the clear only: encrypted output never is (its AES IVs come from the OS).
  - **Reading back:** whether a page is a scan (its content one image over the page, maybe with a text
    layer, as copiers, ScanSnap, OCRmyPDF and Tesseract write them), the image, its placement and resolution,
    and its samples through `pdf-images`; whether it already has a text layer. So blank pages, separators and
    OCR on a scanned PDF needn't render it (M5 `risks.md`: "decoding a scanned page's own image instead of
    drawing it … is the fallback if the gate isn't met"). Page attributes from pdf-semantics' page tree
    (MediaBox, CropBox, Rotate, UserUnit), operators from pdf-content-streams; the walk of the CTM through
    `q`/`Q`/`cm` and forms is ours, about 200 lines, bounded, since the family keeps the interpreter out
    (PL-4).
    - A scan: image area, summed over every draw, at least 0.80 of the page (pdf_oxide's threshold,
      permissive); images drawn into a 1×1 unit square left out of the resolution; `/UserUnit` scaling it.
    - A text layer: text in render mode 3 or 7, or in a font whose `/BaseFont`, subset prefix stripped, is
      `GlyphLessFont` or `Occulta` (OCRmyPDF's facts about others' files, not its code); an invisible-glyph
      share of at least 0.50 over a scan (pdf_oxide). That answers most of FEAT-4.
- **Prior art:** no Rust writer fits. krilla 0.8.2 has no render mode 3, no 1-bit or CCITT images, and one
  CID per glyph; pdf-writer 0.15.0 is a second serializer that holds the whole file and writes f32 numbers;
  printpdf names resources from a process-wide counter, so its bytes depend on what the process made
  before; genpdf hasn't released since 2021; deepocr-core sends every CID to `.notdef`; pdf-ocr is AGPL.
- **For sheaf's entries** (`backlog/features.md`): a page at a time (FEAT-2, B18); `/Producer` and
  `/CreationDate` from the caller (FEAT-3, B30); later, another tool's text layer found (FEAT-4, F10).
- **Not here:** bookmarks and splits (sheaf's ops); PDF/A (a later question, after the family's
  conformance milestones; krilla's clause tables are the permissive reference, veraPDF the check, and
  OCRmyPDF's route goes through copyleft tools); mixed raster content (only AGPL and GPL implementations
  exist: archive-pdf-tools, didjvu).
- **Tested by:** pages rendered by PDFium equal the images that made them; PDFium's text extraction gives the
  words back in their boxes; the font read back through pdf-fonts (CIDs, `/DW`, ToUnicode's text); pdf-lint
  checks every file it writes, clean (Arlington's dictionaries, not samples); qpdf `--check` (pdf-fixtures
  runs it, path-only); pdf-extract (MIT) as a second extractor that ignores render modes; the viewer matrix
  (TEST-1: PDFium, pdf.js, Poppler's `pdftotext` as a process; right to left, CJK, combining marks, beyond the
  BMP, vertical lines, a word across a `BT`); veraPDF for `.notdef`; the same bytes across processes; the
  reader over the corpus's 709 scans, against PDFium's render at the image's resolution.
- **Publishes** once pdf-structure, pdf-content-streams and pdf-semantics are on crates.io: crates.io takes
  no path dependencies.
- **Open:** sheaf's own text layer stays sheaf's (page content in its model, M10.5 §3.4, written by its
  content writer) until its content is on pdf-content-streams; then it can call this crate's layer.
