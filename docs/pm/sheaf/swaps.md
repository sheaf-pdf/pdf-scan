# sheaf's side: what sheaf takes after each milestone

sheaf's own slices, done in sheaf when Greg says; nothing here writes to sheaf (`decisions.md`, "Made here").
Each swap deletes sheaf's copy in the same PR, closes the sheaf entries the crates fixed, and keeps sheaf's
checks as they are (its byte-identical tests, `gate_7k`'s scan gates). "When" is the earliest a swap could
be; Greg decides when it is, and may hold any later.

The device crates are held back furthest. Their model is provisional until S0's `twain` backend has been
through it (`milestones/s0-devices.md`, slice 11), so sheaf takes none of them before then, and may wait longer
still (Greg, 2026-11-08: "for swaps we may delay sheafs adoption even further"): for instance until the hand
checks on the office's scanners are recorded, or until sheaf's M5 resumes.

| When | sheaf takes | Replacing | Closes in sheaf's backlog |
|---|---|---|---|
| After S1 | `scan-image`, `scan-cleanup`, `patch-codes`; `scan-fixtures` in its tests | `sheaf_image::{io, encode, clean, blank, codes::patch, codes::fixture}`; `sheaf-image` keeps `raster` and the barcode reader, or folds them elsewhere | B14, B15, B24, B25 (`scan-image`), B39 (`scan-cleanup`); B17 with its dialog's change |
| After S2 | `pdf-scan` for images as pages | `sheaf_doc::images` behind `MemoryKind::Images` (a small PDF made in memory: bytes, so it doesn't wait on M18's sheaf slices) | B18 and B30, with images-to-pdf's and scan's changes |
| After S2 and M18 slice 19 | `pdf-scan`'s text layer | `sheaf_doc::content::Glyphless` and the layer's drawing, once sheaf's content is on pdf-content-streams | |
| After S0 at the earliest, the device model frozen; possibly later | `scan-device` (the `sane`, `twain` and `wia` backends), `scan-worker`; `sane-dl` and `twain-dsm` beneath them | M5 slice 6's `sheaf-scanner` and its worker, and M5 slices 8 and 9, none built in sheaf | |
