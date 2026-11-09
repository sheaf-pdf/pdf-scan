# The backlog

Everything open, in one place, as pdf-lint's `scratch/backlog/` is (there, local; here, tracked). Started
2026-11-08 with sheaf's open backlog entries that a crate here fixes or takes part of, copied out (decided
2026-11-08: "no changes in sheaf from here, but free to copy items out"). Each entry stays open in sheaf: it
closes there when sheaf takes the crate (`sheaf/swaps.md`). The entries as sheaf wrote them are
in `scratch/reference/sheaf/backlog/`. The prior-art survey (`research/prior-art/README.md`,
2026-11-08) revised those and added the rest: its corrections, its unverified points that an experiment
settles, and its upstream contributions.

| Kind | File | S0 | S1 | S2 | later | Open | Waiting on review |
|---|---|---|---|---|---|---|---|
| bug (BUG) | `bugs.md` | | 5 | 1 | 1 | 7 | |
| feature (FEAT) | `features.md` | 2 | 2 | 2 | 7 | 13 | |
| docs (DOC) | `docs.md` | | 1 | | | 1 | |
| test (TEST) | `tests.md` | 6 | 2 | 1 | 1 | 10 | |
| question (Q) | `questions.md` | 2 | 1 | 2 | | 5 | 1 |
| upstream (UP) | `upstream.md` | 8 | 4 | 1 | 1 | 14 | 13 |
| **all** | | **18** | **15** | **7** | **10** | **50** | **14** |

## Where to start

- **S0:** TEST-7 (Homebrew's SANE on macOS, and since slice 1a the fake's tests there through
  `DYLD_LIBRARY_PATH`) when a slice next touches `sane-dl`'s CI (TEST-2 and TEST-8, slice 1's, closed by CI's
  run of its pull request: `archive/backlog-done.md`); TEST-15 (slice 1a's code on CI's three systems), closed
  by CI's run of its pull request; Q-7 (whether `Sane::load` refuses an empty or relative entry of the loader's
  search path) and UP-14 (SANE's `dll` and the library path in a set-id process), slice 1a's code review's,
  wait on review; TEST-14 (`twain-dsm`'s public API through the gate, on Windows; TEST-12 on slice 1a's
  branch) when a slice next touches `twain-dsm`'s CI; FEAT-5 (patch codes the driver reports) with the Windows
  backends;
  UP-7 to UP-11, defects slice 1 found in SANE's own `test` backend, `scanimage` and several backends'
  `sane_set_io_mode`, wait on review. TEST-3 (TWAIN's sample source) was done by slice 7
  (`archive/backlog-done.md`); from slice 8, TEST-11 (`twain-dsm`'s decoders fuzzed), for a slice with nightly
  at hand. From slice 2: FEAT-12 (stored ids refused before `sane_open`), TEST-10 (the
  stored-id worker's configuration, tested without the system's) and Q-4 (a device's strings as bytes) in slice
  5; TEST-9 (what only the fi-6130 shows of names and cancels) in slice 6's hand check; UP-12 and UP-13
  (`scanimage`, sane-dll(5)) wait on review. BUG-7 (`sane-dl`'s `Image` printing its bytes in `Debug`) was
  done by slice 1a.
- **S1:** BUG-1 to BUG-5 and DOC-1 are fixed in the cut; FEAT-1 and FEAT-8 are built in it; TEST-4 and
  TEST-6 run in it; Q-1's second half (one hayro-ccitt across both workspaces) is PL-3's.
- **S2:** BUG-6, FEAT-2, FEAT-3; TEST-1, the viewer matrix, early, which settles Q-2 and Q-3.
- **Upstream:** `upstream.md`, each waiting on the maintainer's OK; none blocks a slice.

## An item

One line each, so `rg` finds it whole:

`- **ID** · crate · for · status — **Title.** What's left and why, with the numbers that matter. *(source)*`

- **ID**: the kind's prefix and a number, never reused. Each file's head says the next free one. sheaf's own
  entries are cited as sheaf writes them (`B24`, `F10`), which no ID here clashes with.
- **for**: the milestone, `S0`, `S1` or `S2`, or `later`.
- **status**: `open`, or `waiting on review` where only the maintainer can move it.
- **source**: where the item came from: sheaf's entry, an entry of build-or-adopt, the survey (by its notes'
  topic), a slice here.

## Keeping it

- A slice that leaves something open adds an item, in its kind's file under its heading, with the next ID.
- A slice that closes an item moves its line to `archive/backlog-done.md`, adding "Done: where" before the
  source. Nothing is deleted. An item copied from sheaf says in its done line that sheaf's entry is still
  open there, for sheaf's swap to close.
- The table above is counted by hand when a slice ends, or its counts are left out rather than left wrong.
