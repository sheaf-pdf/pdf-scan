# S2: PDF

**Scanned pages to PDF and back, on the family.** Needs S1's `scan-image` and the family: pdf-objects,
pdf-filters and pdf-color as their published alphas, pinned exactly; pdf-structure, pdf-content-streams and
pdf-semantics by path until their alphas are out. pdf-structure's writer API is still moving: read the
family again before slice 1 (`pdf-lint/README.md`, "Keeping up"). What goes back to the family: PL-1 to
PL-4 (`pdf-lint/README.md`).

1. **pdf-images.** Image dictionaries read (width, height, depth, color space, `/Decode`, `/ImageMask`,
   `/SMask`) and capped before decoding (`W × H × components × bpc`); the filter chain read here and the
   standard filters run through pdf-filters (until PL-1), then DCT, CCITT, JBIG2 and JPX, 12-bit DCT a typed
   refusal; default `/Decode` and Lab's and ICCBased's `/Range` here; samples in the image's own space and
   depth, 1 to 16 bits. Checked: PDFium's bitmaps after its conversion, through pdfium-render (the bitmap
   calls are ours; pdf-lint's oracle renders nothing); the corpus's images of each filter; fuzzing.
2. **pdf-scan, pages.** `scan-image` pages as PDF pages, sized or fitted, turned by EXIF, whole files and a
   page at a time, and pages appended to a file as an update. The streamed layout is assembled here around
   `write::Parts` (header, offsets, cross-reference section, trailer, `/ID` hashed as written) until PL-2,
   whole and over `Section::append` for an update; the page tree built here until the family writes one;
   the Flate backend named at every call. Checked: bytes equal to `write::full`'s for the same bodies; the
   same inputs make the same bytes (in the clear; encrypted output never is); each page drawn by PDFium equals
   its image (G4 and Flate exactly, a JPEG as the same decoder decodes it); page sizes and turns equal to
   sheaf's images-to-pdf on the same images; pdf-lint finds nothing in what's written (its checks are
   dictionaries, not samples); qpdf `--check`; 500 black-and-white pages written a page at a time stay under
   the memory sheaf's intake gate allows (400 MB added), which only the streamed layout can meet.
3. **pdf-scan, the text layer.** The glyphless font and its objects (the family writes no fonts), words
   grouped into lines, the layer emitted into a pdf-content-streams writer (`TJ` through `operation`), its
   `ResourceNeeds` bound into `/Resources` here. Checked: read back through pdf-fonts (CIDs, `/DW`,
   ToUnicode's text); PDFium's text and character boxes give back the words in their
   boxes; text in scripts beyond Latin (CJK, right to left, combining marks) round-trips; the layer over the
   same words lands where sheaf's does (PDFium's boxes compared).
4. **pdf-scan, reading.** A page that's a scan, its image, its placement and resolution, its samples, and
   whether it has a text layer: page attributes from pdf-semantics, operators from pdf-content-streams, and
   our own bounded walk of the CTM through `q`/`Q`/`cm` and forms (the family keeps the interpreter out;
   PL-4). The first run of any family crate over the corpus's scans. Checked: over the corpus's 709 scans, the
   pages read as scans counted and said, each image's samples against PDFium's render at the image's
   resolution; pages that aren't scans (vector content, two images) never read as one.

**Done when** both crates pass, and a scan written by `pdf-scan` and read back by it gives the same samples
and words. Publishing waits on the family's: `pdf-images` on pdf-structure's first alpha (and
pdf-content-streams', for inline images); `pdf-scan` on pdf-content-streams' and pdf-semantics' too. With
exact pins, each family alpha means a release here.
