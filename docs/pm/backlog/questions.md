# Questions: decisions still to make

IDs Q-1 to Q-3; the next new one is **Q-4**. Format and rules: `README.md`. A question for the maintainer alone goes in
`asks.md`; these are the plan's, settled by a measurement, a test or a slice, and any of them can be raised
to an ask.

## For S1 (pages)

- **Q-1** · scan-image, pdf-images; pdf-lint's family · S1 · open — **The MSRV, and one hayro-ccitt across both workspaces.** hayro-ccitt, hayro-jbig2 and hayro-jpeg2000 need Rust 1.92, above the 1.89 decisions.md gives for the family (sheaf's is 1.98). pdf-content-streams pins `hayro-ccitt = "0.3"`, two breaking releases behind 0.5.0, which this plan needs for G3 and MH. the MSRVs were measured on 2026-11-08, before S0 took the lead (`milestones/s1-pages.md`, slice 1): 1.92 for `scan-image` alone, 1.89 for the other pixel crates, so the first half is answered for S1; the version pdf-content-streams takes is pdf-lint's to change, through a proposal (`pdf-lint/README.md`), never from here. *(survey, conclusion; pdf-lint's `crates/pdf-content-streams/Cargo.toml`)*

## For S2 (PDF)

- **Q-2** · pdf-scan · S2 · open — **Right-to-left words in the invisible layer: visual order or a mirrored text matrix?** Text extractors run the bidi algorithm on glyph positions, so OCRmyPDF lays right-to-left words out in visual order; Tesseract mirrors the text matrix instead. Settled by TEST-1. *(survey, pdf images and writing)*
- **Q-3** · pdf-scan · S2 · open — **Text beyond the Basic Multilingual Plane.** As sheaf writes it, each such character becomes two Identity-H CIDs whose ToUnicode entries are lone surrogates; no source says how PDFium, pdf.js and Poppler join them. Options: keep the pair and test; map the pair's first CID to the whole character and the second to nothing; or another scheme. Settled by TEST-1. *(survey, pdf images and writing)*
