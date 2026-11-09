# The milestones

S0, S1 and S2, in order, each a set of slices (one branch each, as pdf-lint's are: planned, reviewed, built in
steps, code-reviewed, fixed). Every slice here is `planned`. A slice's plan goes in `slices/` when it starts.

A slice's plan goes in `slices/` while it runs and to `archive/slices/` when it's done; a milestone's file goes
to `archive/milestones/` when its last slice is. What sheaf takes after each is `sheaf/swaps.md`.

| File | Milestone |
|---|---|
| `s0-devices.md` | S0: scan devices: SANE, the model (provisional) and the worker on Linux; TWAIN and WIA on Windows; the model frozen |
| `s1-pages.md` | S1: the pixel crates cut from sheaf, sheaf's output held byte for byte |
| `s2-pdf.md` | S2: scanned pages to PDF and back, on the family |
| `later.md` | After S2: macOS, a faster cleanup, a second barcode engine |
