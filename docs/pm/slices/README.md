# Slices running

The plan of each slice while it runs, one file each (`s1-1-workspace.md`, …), as pdf-lint's `scratch/slices/`
holds them: planned, reviewed, built in steps, code-reviewed, fixed. When a slice is done its plan moves to
`archive/slices/`. Its milestone's file (`milestones/`) says what it builds and how it's checked; the plan
here says how.

Done: S0 slices 1 (`sane-dl` and the workspace it starts), 2 (the seam note revised) and 3 (`scan-device`
provisional), merged (pull requests #2, #3 and #6, CI green on each); their plans are in
[`archive/slices/s0-1-sane-dl.md`](../archive/slices/s0-1-sane-dl.md),
[`archive/slices/s0-2-seam-note.md`](../archive/slices/s0-2-seam-note.md) and
[`archive/slices/s0-3-scan-device.md`](../archive/slices/s0-3-scan-device.md).

Done on its branch, its code review's findings fixed, waiting on review, the push and its pull request:
[`s0-4-scan-worker.md`](s0-4-scan-worker.md), S0 slice 4, `scan-worker` (five steps, each checked, then 20
findings, 18 fixed whole and two in part, their other halves FEAT-16 and FEAT-17; branch `s0-4-scan-worker`,
from `master` at `5a4bff2`).

Done: S0 slice 7, the TWAIN spike, on the Windows machine, merged (pull request #1); its plan is in
[`archive/slices/s0-7-twain-spike.md`](../archive/slices/s0-7-twain-spike.md).

Done: S0 slice 8, `twain-dsm`, on the Windows machine, merged (pull request #4); its plan is in
[`archive/slices/s0-8-twain-dsm.md`](../archive/slices/s0-8-twain-dsm.md).
