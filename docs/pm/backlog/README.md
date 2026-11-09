# The backlog

Everything open, in one place, as pdf-lint's `scratch/backlog/` is (there, local; here, tracked). Started
2026-11-08 with sheaf's open backlog entries that a crate here fixes or takes part of, copied out (Greg,
2026-11-08: "no changes in sheaf from here, but free to copy items out"). Each entry stays open in sheaf: it
closes there when sheaf takes the crate (`sheaf/swaps.md`). The entries as sheaf wrote them are
in `scratch/reference/sheaf/backlog/`. The prior-art survey (`research/prior-art/README.md`,
2026-11-08) revised those and added the rest: its corrections, its unverified points that an experiment
settles, and its upstream contributions.

| Kind | File | S1 | S2 | S3 | S4 | later | Open | Waiting on Greg |
|---|---|---|---|---|---|---|---|---|
| bug (BUG) | `bugs.md` | 5 | 1 | | | | 6 | |
| feature (FEAT) | `features.md` | 2 | 2 | | 1 | 5 | 10 | |
| docs (DOC) | `docs.md` | 1 | | | | | 1 | |
| test (TEST) | `tests.md` | 2 | 1 | 1 | 1 | 1 | 6 | |
| question (Q) | `questions.md` | 1 | 2 | | | | 3 | |
| upstream (UP) | `upstream.md` | 4 | 1 | | | 1 | 6 | 5 |
| **all** | | **15** | **7** | **1** | **2** | **7** | **32** | **5** |

## Where to start

- **S1:** BUG-1 to BUG-5 and DOC-1 are fixed in the cut; FEAT-1 and FEAT-8 are built in it; TEST-4 and
  TEST-6 run in it; Q-1 is S1 slice 1's MSRV measure.
- **S2:** BUG-6, FEAT-2, FEAT-3; TEST-1, the viewer matrix, early, which settles Q-2 and Q-3.
- **Upstream:** `upstream.md`, each waiting on Greg's OK; none blocks a slice.

Not copied, because they stay sheaf's: B16 (the command line's progress), B29 (search), B2, B33, B34, B37,
B56, B62 (the app, recovery and the serializer); F16 (QR codes as links: the barcode reader is sheaf's) and F21
(an OCR engine). The rest a search for scanning's words turned up (F1, F5, F9, F11, F12, F17, F18, F26,
F27) don't touch these crates.

## An item

One line each, so `rg` finds it whole:

`- **ID** · crate · for · status — **Title.** What's left and why, with the numbers that matter. *(source)*`

- **ID**: the kind's prefix and a number, never reused. Each file's head says the next free one. sheaf's own
  entries are cited as sheaf writes them (`B24`, `F10`), which no ID here clashes with.
- **for**: the milestone, `S1` to `S4`, or `later`.
- **status**: `open`, or `waiting on Greg` where only he can move it.
- **source**: where the item came from: sheaf's entry, an entry of build-or-adopt, the survey (by its notes'
  topic), a slice here.

## Keeping it

- A slice that leaves something open adds an item, in its kind's file under its heading, with the next ID.
- A slice that closes an item moves its line to `archive/backlog-done.md`, adding "Done: where" before the
  source. Nothing is deleted. An item copied from sheaf says in its done line that sheaf's entry is still
  open there, for sheaf's swap to close.
- The table above is counted by hand when a slice ends, or its counts are left out rather than left wrong.
