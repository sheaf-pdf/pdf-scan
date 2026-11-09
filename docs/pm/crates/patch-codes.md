# patch-codes

Layer: Pages (`crates/README.md`). Paths are from `docs/pm/`.

- **What:** Kodak patch codes (`sheaf_image::codes::patch`, entry 12): types 1, 2, 3, 4, 6 and T found along
  any of a page's edges, which edge they lie along (so 3 is told from T and 1 from 6 on a sheet fed turned or
  upside down), and drawn in Kodak's layout for sheets that carry them.
  - **The geometry** (A-61599, 2006, public with a copyright notice; its zone tables match sheaf's bar table
    exactly): bars parallel to the leading edge, wide 0.20 in and narrow 0.08 in, 0.80 in across, ±0.01 in;
    **at least 2 in long, 2.5 in recommended**, within 3.75 in of the leading edge. The "0.75 in" in
    `patch.rs`'s doc is A-61599's figure for its other layout, bars perpendicular to the edge (i280, 3590C,
    i600, toggle patch only), whose reading area ends 2 in from the edge (DOC-1; FEAT-6). The reader accepts
    lines that agree for 0.6 in, which takes skewed and clipped sheets and stays a superset.
  - Kodak's own software reader searches the top 4 in after rotation, with an optional retry at 180°: a
    four-edge reader that reports the edge does strictly more.
- **Boundary:** `image` alone. Nothing open reads or writes patch codes, in any language: NAPS2 and
  Paperless-ngx "support" patch T by reading the Code 39 text printed on the sheet, and about eight closed
  SDKs (Accusoft, Dynamsoft, LEADTOOLS, Aspose, VintaSoft and others) read the bars without publishing
  their accuracy. This is the crate nobody else has.
- **Tested by:** sheaf's tests, moved (the prototype read 204 of 204 synthetic pages and 60 of 60 renders of
  Kodak's sheets); the corpus's scans as the negative set (no patch bars found on any, M5 slice 3); the
  bench's degradations from `scan-fixtures`. Printable sources: A-61599's camera-ready pages, Kodak's 2012
  "Patches for Printing", Alliance Group's sheets; no public set of scanned patch-code pages exists.
