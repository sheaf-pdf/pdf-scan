# S1: Pages

**The pixel crates cut from sheaf, sheaf's output held byte for byte.** Needs nothing unbuilt: no scanner,
no PDF crate.

1. **The four crates join the workspace, and the harness.** The workspace itself comes with S0 slice 1; this
   slice adds `scan-fixtures`, `scan-image`, `scan-cleanup` and `patch-codes`, empty, with their MSRVs, the
   pages layer's gates, and the harness: an export of sheaf at a pinned commit beside the crates, as
   pdf-lint's `scratch/harness` is, so each crate's output is compared with sheaf's own code's on the same
   inputs. Started once on 2026-11-08 before S0 took the lead, its work kept in `scratch/stash-s1-1/` (and git's
   stash): the MSRVs measured then (`scan-image` 1.92, for hayro-ccitt 0.5.0; the other three 1.89, the
   family's floor; each over its own dependency graph), the gates and each shown to fail on its violation, and
   the harness building sheaf's `sheaf-image` at `3232a35` (`scratch/harness/`). Checked: as then, again on
   the workspace S0 leaves.
2. **scan-fixtures.** `codes::fixture` moved: the synthetic pages, the degradations, codes drawn. Checked:
   the same pixels as sheaf's for every seed and step, through the harness.
3. **scan-image.** `io` and `encode` moved, anyhow to thiserror, with BUG-1 to BUG-4 fixed (sheaf's B24,
   B25, B15, B14) and FEAT-1's page count. Checked: sheaf's tests, moved; the harness byte for byte on
   sheaf's test images and on images made by `scan-fixtures` in every kind the crate reads; for B24, a G4
   page 70,000 pixels wide round-trips with overflow checks on (on `fax`'s #31 fix; or widths past 32,767
   refused, said, until it's released), read back through hayro-ccitt; for B25, G3 1D, G3 2D and Modified Huffman
   TIFFs read; for B14, a TIFF with Orientation 6 the same by every path; fuzzing of the TIFF and JPEG paths
   with dimensions capped before decoding.
4. **scan-cleanup.** `clean` and `blank` moved, and the trait for a caller's models, with BUG-5 fixed
   (sheaf's B39). Checked: sheaf's tests, moved; the harness byte for byte under both profiles, step by step,
   on the fixtures and on a sample of the corpus's scans drawn at 300 dpi, parting from sheaf only where
   Otsu overflowed; a blank 600 dpi Legal page (42.8 million pixels) given the threshold a `u64` count gives,
   in a debug build; the speed of each step recorded per page (not gated here).
5. **patch-codes.** `codes::patch` moved, and the drawing of Kodak's layout. Checked: sheaf's tests, moved;
   M5 slice 3's matrix (each type at 150, 200 and 300 dpi, gray and 1-bit, turned, skewed, with noise and
   blur); the corpus's scans read no patch code.

**Done when** the four crates pass on every runner, each has its README and docs, and each gives sheaf's
output. sheaf's swap is sheaf's slice (below).
