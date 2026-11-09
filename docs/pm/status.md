# Status

As of 2026-11-08.

**Building S0 slice 1.** On its branch, the workspace and `sane-dl`'s first, empty cut (step 1 of 5). It's a
git repository, pushed on 2026-11-08 to github.com/sheaf-pdf/pdf-scan (private; `master`), holding
`docs/pm/` alone.

- **Done:**
  - The first plan (`plan.md`, `crates/`, `milestones/`, `sheaf/from-sheaf.md`), drawn from sheaf's M5 scan
    plan, its build-or-adopt entries 5 to 16 and 33 to 38, `plan/libraries.md`, and pdf-lint's family as of
    `8b5de62`. Crate names checked free on crates.io the same day.
  - Greg's ruling: sheaf isn't changed from here, and anything in it may be copied out (`sheaf/README.md`).
    The 60 sheaf plan files this plan cites are copied as at `3232a35` into `scratch/reference/sheaf/`, and
    sheaf's 9 open entries a crate here fixes into `backlog/`.
  - `docs/pm/` laid out as Greg chose (2026-11-08): `crates/` and `milestones/` a file each, `slices/` and
    `archive/` as pdf-lint's, `sheaf/` (downstream) beside `pdf-lint/` (upstream), `research/` a folder per
    survey. `scratch/` keeps only local copies.
  - The prior-art survey (`research/prior-art/`): every crate is still a build, on dependencies used
    directly. Folded into the crates, `sheaf/from-sheaf.md`'s prior art, three slices, and the backlog (32
    items in six kinds; the upstream contributions each waiting on Greg).
  - pdf-lint as an upstream (`pdf-lint/README.md`; Greg's ruling in `decisions.md`): pdf-scan is the family's
    consumer, published alphas pinned exactly, unpublished crates by path, major shortcomings proposed back.
  - The family's fit as the PDF crates' base, read at pdf-lint `d777a8f` (local,
    `scratch/pdf-lint/pdf-family-fit/`): a strong base for `pdf-scan`'s writing, a thin one for `pdf-images`;
    four major shortcomings.
  - The fit folded in: `crates/pdf-images.md`, `crates/pdf-scan.md`, `milestones/s2-pdf.md`, `decisions.md`
    (the pixels rule made exact, the MSRV, D4's Flate backend) and ask 5; the plan re-pinned from `8b5de62`
    (amended away) to `d777a8f`.
  - Four proposals to pdf-lint, drafts kept local in `scratch/pdf-lint/proposals/`: PL-1 decode to the image
    filter, PL-2 a streaming writer, PL-3 one hayro-ccitt (and the MSRV route), PL-4 a bounded graphics-state
    walk. Each says what pdf-scan does meanwhile, so none blocks a slice.
  - The device layer's order (Greg): bindings first, `sane-dl` before `scan-device` in S3, a seam note between
    them (`crates/scan-device-seam.md`, first version written 2026-11-08), the model provisional until S4's
    `twain` backend; S4's spike made end to end; sheaf's adoption of the device crates after S4 at the
    earliest (`sheaf/swaps.md`).
  - S0 added, the scan devices first (Greg): S3 and S4 folded into `milestones/s0-devices.md`, twelve slices,
    Linux then Windows; S1 and S2 follow. The workspace S1 slice 1 had built (stashed; copied into
    `scratch/stash-s1-1/`) moves into S0 slice 1, reshaped around `sane-dl`. Asks 3 and 4 answered by it; 6
    and 7 taken as S0 slice 1 starts.
- **Running: S0 slice 1, `sane-dl` and the workspace it starts**, on branch `s0-1-sane-dl`, through the
  `scan-slice` workflow (`.claude/workflows/scan-slice.js`, local): its plan in `slices/`, a commit per step
  on the branch, nothing pushed.
- **Running too: S0 slice 7, the TWAIN spike**, on the Windows machine, on branch `s0-7-twain-spike` (a draft
  pull request; `slices/s0-7-twain-spike.md`): its four steps done 2026-11-08: the sample source installed
  silently (here and on `windows-latest`), TWAIN walked from state 1 to 7 and back in both bitnesses, native
  and memory transfers equal pixel for pixel, a cancel from another thread; the seam note's TWAIN rows and
  `crates/twain-dsm.md` revised from it. CI green on every push, its scans on `windows-latest` included
  (TEST-3 done). Its code review and fixes next.
- **Next:** slice 1's run, its code review and fixes; then its review, and once approved the push and the
  pull request, CI on the three systems closing the slice. Then S0 slice 2, the seam note's SANE rows revised;
  slice 8 (`twain-dsm`) can follow slice 7 on Windows.
- **Waiting on Greg:** the slice's review and its push; `asks.md`, ask 5 (ask 2 taken as S0 slice 1 started,
  `decisions.md`); PL-1 to PL-3, ready to hand off on his OK; PL-4, which first needs his call on pdf-lint's
  "the interpreter stays out" line.
