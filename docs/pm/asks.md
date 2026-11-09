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
3. **In what order?** (`milestones/`)
   - **S1 pages, S2 PDF, S3 devices on Linux, S4 Windows.** Recommended: each is checked fully before the
     next, the first two need no hardware, and S1 is code that already works.
   - Devices first (S3 and S4), as M5's roadmap line lists scanners before separators: the platform risk
     earlier, and the most hand checks while there's still no scanner at hand.
4. **Does device work start before there's a scanner to check it on?** (`milestones/s3-linux-devices.md`,
   `milestones/s4-windows.md`)
   - **S3 builds on fakes and SANE's test backend now; its hand check waits for a Linux laptop at the office.
     S4 waits until a development build may run on a work PC.** Recommended: everything but the last step
     of S3 is checkable in CI, and M5's deferral was about time spent on what couldn't be checked.
   - All device work waits, as M5's does: S1 and S2 only for now.
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
6. **The names.** (`crates/`; all free on crates.io on 2026-11-08)
   - **`scan-device`, `sane-dl`, `twain-dsm`, `scan-worker`, `scan-image`, `scan-cleanup`, `patch-codes`,
     `pdf-images`, `pdf-scan`, `scan-fixtures`.** Recommended: `scan-` for the scanning family, `pdf-` for the
     two that are PDF, the bindings named for what they load.
   - Another prefix for all (`scanner-*`; `scanner` itself is taken), or `sane`/`twain` alone (`twain` is
     taken; `sane` is a serialization format).
7. **The standing rules:** pdf-lint's, carried over? (`plan.md`) Its first, tracked files never name sheaf,
   meets `docs/pm/`, which is tracked and names sheaf throughout.
   - **pdf-lint's, with the first narrowed: code, crate docs, crate READMEs and commit messages never name
     sheaf; `docs/pm/` may.** Then: nothing published, pushed or reported upstream without your word; typed
     errors; caps on hostile input. Recommended: the crates stand alone as pdf-lint's do, and the plan can
     say what it's for. If sheaf's plan isn't to be seen by whoever reads this repository, the next option.
   - pdf-lint's whole: `docs/pm/` written without naming sheaf ("the editor" for sheaf), its sheaf citations
     kept in `scratch/`.
   - pdf-lint's minus the first: these crates can say they're sheaf's, as sheaf's own crates do.
   (sheaf unchanged from here is already ruled: decisions.md, "Made here".)
