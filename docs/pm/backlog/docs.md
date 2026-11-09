# Docs: documentation that's wrong or missing

IDs DOC-1; the next new one is **DOC-2**. Format and rules: `README.md`.

## For S1 (pages)

- **DOC-1** · patch-codes · S1 · open — **`patch.rs`'s doc comment mixes Kodak's two layouts.** It says bars are "at least 0.75 in long (2 in for Kodak's Patch-Reader scanners)" beside the 3.75 in band (`sheaf-image/src/codes/patch.rs:9-10`). In A-61599, 0.75 in belongs to the perpendicular-bar layout (i280, 3590C, i600, toggle patch only); the parallel layout the reader handles needs at least 2 in, 2.5 in recommended, within 3.75 in of the leading edge. sheaf's fixtures already draw A-61599's sheet 2.5 in long (`codes/fixture.rs:276-277`), so only the doc is wrong. Fix it in the cut; FEAT-6 decides the other layout. *(survey, patch codes and barcodes)*
