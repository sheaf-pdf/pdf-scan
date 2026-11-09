# Asks for review

Each with its options, the recommended answer first. Under the maintainer's standing instruction, a
recommended answer is taken rather than waited on, and stays open to review until the slice that depends on it
starts. When one is ruled, the ruling goes in `decisions.md` and the ask goes.

5. **Where does `pdf-images` live?** (`crates/pdf-images.md`)
   - **Here.** Recommended, first for the MSRV and the release pipeline: the hayro codecs need Rust 1.92,
     above the family's 1.89, and pdf-lint releases slice by slice, so `pdf-images` here moves at its own
     pace. Also: the family writes no codec and keeps no samples (decisions.md), scanning is its first user,
     and sheaf's M11, M13 and M14 take it from here as they'd take it from anywhere. PL-1 is needed wherever
     it lives.
   - In pdf-lint's family, the one crate there that decodes pixels: one less workspace for M11 to depend on;
     but it costs the family MSRV 1.92, or a per-feature MSRV line, which pdf-lint's policy dropped, and it
     crosses a scope line the maintainer has bent once already (`image-ends`).
   - In `sheaf-image`, as entry 33 has it: no crate to publish, but nobody else gets it.
