# Slices running

The plan of each slice while it runs, one file each (`s1-1-workspace.md`, …), as pdf-lint's `scratch/slices/`
holds them: planned, reviewed, built in steps, code-reviewed, fixed. When a slice is done its plan moves to
`archive/slices/`. Its milestone's file (`milestones/`) says what it builds and how it's checked; the plan
here says how.

Done on its branch, waiting on review: [`s0-1-sane-dl.md`](s0-1-sane-dl.md), S0 slice 1, `sane-dl` and the
workspace it starts (its five steps built and checked, its code review's findings fixed; pull request #2,
and it moves to `archive/slices/` once merged).

Done on its branch, its code review's findings fixed: [`s0-2-seam-note.md`](s0-2-seam-note.md), S0 slice 2,
the seam note revised (three steps, docs only; its pull request to be stacked on slice 1's).

Done: S0 slice 7, the TWAIN spike, on the Windows machine, merged (pull request #1); its plan is in
[`archive/slices/s0-7-twain-spike.md`](../archive/slices/s0-7-twain-spike.md).

Done on its branch, its code review's findings fixed: [`s0-8-twain-dsm.md`](s0-8-twain-dsm.md), S0 slice 8,
`twain-dsm`, on the Windows machine (six steps, each pushed; pull request #4, rebased onto `master` after #2 and
#3).
