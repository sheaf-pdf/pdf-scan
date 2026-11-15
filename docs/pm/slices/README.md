# Slices running

The plan of each slice while it runs, one file each (`s1-1-workspace.md`, …), as pdf-lint's `scratch/slices/`
holds them: planned, reviewed, built in steps, code-reviewed, fixed. When a slice is done its plan moves to
`archive/slices/`. Its milestone's file (`milestones/`) says what it builds and how it's checked; the plan
here says how.

Done: S0 slices 1 (`sane-dl` and the workspace it starts), 1a (`sane-dl`'s public API made safe), 2 (the seam
note revised), 3 (`scan-device` provisional), 4 (`scan-worker`), and 5 (`scan-device`'s `sane` backend), merged
(pull requests #2, #7, #3, #6, #8 and #9); their archived plans are in
[`archive/slices/s0-1-sane-dl.md`](../archive/slices/s0-1-sane-dl.md),
[`archive/slices/s0-1a-sane-dl-safe-api.md`](../archive/slices/s0-1a-sane-dl-safe-api.md),
[`archive/slices/s0-2-seam-note.md`](../archive/slices/s0-2-seam-note.md),
[`archive/slices/s0-3-scan-device.md`](../archive/slices/s0-3-scan-device.md), and
[`archive/slices/s0-4-scan-worker.md`](../archive/slices/s0-4-scan-worker.md).

Done: S0 slice 7, the TWAIN spike, on the Windows machine, merged (pull request #1); its plan is in
[`archive/slices/s0-7-twain-spike.md`](../archive/slices/s0-7-twain-spike.md).

Done: S0 slice 8, `twain-dsm`, on the Windows machine, merged (pull request #4); its plan is in
[`archive/slices/s0-8-twain-dsm.md`](../archive/slices/s0-8-twain-dsm.md).

Done: S0 slice 9, `scan-device`'s `twain` backend, 32-bit worker, and Windows hardening, merged (pull request
#11); its plan is in [`archive/slices/s0-9-twain-backend.md`](../archive/slices/s0-9-twain-backend.md).

Running on `s0-10-wia-backend`: [`s0-10-wia-backend.md`](s0-10-wia-backend.md), S0 slice 10 (`scan-device`'s
`wia` backend and `scan-worker`'s `wia` feature).
