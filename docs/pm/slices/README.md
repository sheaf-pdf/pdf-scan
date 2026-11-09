# Slices running

The plan of each slice while it runs, one file each (`s1-1-workspace.md`, …), as pdf-lint's `scratch/slices/`
holds them: planned, reviewed, built in steps, code-reviewed, fixed. When a slice is done its plan moves to
`archive/slices/`. Its milestone's file (`milestones/`) says what it builds and how it's checked; the plan
here says how.

Done: S0 slices 1 (`sane-dl` and the workspace it starts) and 2 (the seam note revised), merged (pull requests
#2 and #3); their plans are in [`archive/slices/s0-1-sane-dl.md`](../archive/slices/s0-1-sane-dl.md) and
[`archive/slices/s0-2-seam-note.md`](../archive/slices/s0-2-seam-note.md).

Done on its branch, its code review's findings fixed, pushed, in pull request #6, waiting on review and CI:
[`s0-3-scan-device.md`](s0-3-scan-device.md), S0 slice 3, `scan-device` provisional (four steps, each checked,
then 18 findings fixed; branch `s0-3-scan-device`, from `master` at `3f33be5`).

Done on its branch, its code review's findings fixed: [`s0-1a-sane-dl-safe-api.md`](s0-1a-sane-dl-safe-api.md),
S0 slice 1a, `sane-dl`'s public API made safe (four steps, each checked, and the review's 22 findings fixed;
pushed, in pull request #7, waiting on review and CI).

Done: S0 slice 7, the TWAIN spike, on the Windows machine, merged (pull request #1); its plan is in
[`archive/slices/s0-7-twain-spike.md`](../archive/slices/s0-7-twain-spike.md).

Done: S0 slice 8, `twain-dsm`, on the Windows machine, merged (pull request #4); its plan is in
[`archive/slices/s0-8-twain-dsm.md`](../archive/slices/s0-8-twain-dsm.md).
