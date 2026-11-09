# The milestones

S1 to S4, in order, each a set of slices (one branch each, as pdf-lint's are: planned, reviewed, built in
steps, code-reviewed, fixed). Every slice here is `planned`. A slice's plan goes in `slices/` when it starts.

A slice's plan goes in `slices/` while it runs and to `archive/slices/` when it's done; a milestone's file goes
to `archive/milestones/` when its last slice is. What sheaf takes after each is `sheaf/swaps.md`.

| File | Milestone |
|---|---|
| `s1-pages.md` | S1: the pixel crates cut from sheaf, sheaf's output held byte for byte |
| `s2-pdf.md` | S2: scanned pages to PDF and back, on the family |
| `s3-linux-devices.md` | S3: SANE, then the device model (provisional) and the worker |
| `s4-windows.md` | S4: TWAIN in both bitnesses, and WIA; the device model frozen |
| `later.md` | After S4: macOS, a faster cleanup, a second barcode engine |
