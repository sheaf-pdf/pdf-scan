# Asks for Greg

Each with its options, the recommended answer first. Under Greg's standing instruction, a recommended answer
is taken rather than waited on, and stays open to his review until the slice that depends on it starts.
When one is ruled, the ruling goes in `decisions.md` and the ask goes.

2. **The cut: which crates?** (`crates/`)
   - **Ten: `scan-device` (model and backends behind features), `sane-dl`, `twain-dsm`, `scan-worker`,
     `scan-image`, `scan-cleanup`, `patch-codes`, `pdf-images`, `pdf-scan`, and `scan-fixtures` (never
     published).** Recommended: SANE and TWAIN bindings are useful alone, and nobody has them; patch codes
     are the crate nobody else has; each layer's gate is simple.
   - Fewer: one `scanning` crate with features for the backends, and the pixel crates as one. Less to
     publish; but a SANE user takes TWAIN's types, and the gates blur.
   - The barcode reader too, with patch codes in one `page-codes` crate: separator finding's reads in one
     place for other tools; but it's thin over rxing, and its policy is sheaf's separation's.
5. **Where does `pdf-images` live?** (`crates/pdf-images.md`)
   - **Here.** Recommended, first for the MSRV and the release pipeline: the hayro codecs need Rust 1.92,
     above the family's 1.89, and pdf-lint releases slice by slice, so `pdf-images` here moves at its own
     pace. Also: the family writes no codec and keeps no samples (decisions.md), scanning is its first user,
     and sheaf's M11, M13 and M14 take it from here as they'd take it from anywhere. PL-1 is needed wherever
     it lives.
   - In pdf-lint's family, the one crate there that decodes pixels: one less workspace for M11 to depend on;
     but it costs the family MSRV 1.92, or a per-feature MSRV line, which pdf-lint's policy dropped, and it
     crosses a scope line Greg has bent once already (`image-ends`).
   - In `sheaf-image`, as entry 33 has it: no crate to publish, but nobody else gets it.
