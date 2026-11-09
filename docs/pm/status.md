# Status

As of 2026-11-09.

**S0 slice 1 is done on its branch, `s0-1-sane-dl`, pushed, in pull request #2,** waiting on review, CI green on
it. The workspace, then `sane-dl` 0.1.0-alpha.1 (SANE loaded at run time, its bindings checked against `sane.h`,
devices under a deadline, typed options, frames assembled into pages, batches, a cancel token any thread can
use), ready to publish and not published, with `fake-libsane` for its tests; its code review's 22 findings all
fixed. **S0 slice 7, the TWAIN spike,** ran on the Windows machine and is merged (pull request #1). It's a git
repository at github.com/sheaf-pdf/pdf-scan (public; `master`). **S0 slice 2, the seam note revised, is done on
`s0-2-seam-note`,** stacked on slice 1's branch, waiting on review, the push and its pull request; S0 slice 3
next, once that's open. **S0 slice 8, `twain-dsm`, is done on its branch, `s0-8-twain-dsm`,** on the Windows
machine, code-reviewed and fixed, in pull request #4, rebased onto `master` after #2 and #3.

- **Done:**
  - The first plan (`plan.md`, `crates/`, `milestones/`, `sheaf/from-sheaf.md`), drawn from sheaf's M5 scan
    plan, its build-or-adopt entries 5 to 16 and 33 to 38, `plan/libraries.md`, and pdf-lint's family as of
    `8b5de62`. Crate names checked free on crates.io the same day.
  - The maintainer's ruling: sheaf isn't changed from here, and anything in it may be copied out
    (`sheaf/README.md`). The 60 sheaf plan files this plan cites are copied as at `3232a35` into
    `scratch/reference/sheaf/`, and sheaf's 9 open entries a crate here fixes into `backlog/`.
  - `docs/pm/` laid out as the maintainer chose (2026-11-08): `crates/` and `milestones/` a file each,
    `slices/` and `archive/` as pdf-lint's, `sheaf/` (downstream) beside `pdf-lint/` (upstream), `research/` a
    folder per survey. `scratch/` keeps only local copies.
  - The prior-art survey (`research/prior-art/`): every crate is still a build, on dependencies used
    directly. Folded into the crates, `sheaf/from-sheaf.md`'s prior art, three slices, and the backlog (32
    items in six kinds; the upstream contributions each waiting on review).
  - pdf-lint as an upstream (`pdf-lint/README.md`; the maintainer's ruling in `decisions.md`): pdf-scan is the
    family's consumer, published alphas pinned exactly, unpublished crates by path, major shortcomings
    proposed back.
  - The family's fit as the PDF crates' base, read at pdf-lint `d777a8f` (local,
    `scratch/pdf-lint/pdf-family-fit/`): a strong base for `pdf-scan`'s writing, a thin one for `pdf-images`;
    four major shortcomings.
  - The fit folded in: `crates/pdf-images.md`, `crates/pdf-scan.md`, `milestones/s2-pdf.md`, `decisions.md`
    (the pixels rule made exact, the MSRV, D4's Flate backend) and ask 5; the plan re-pinned from `8b5de62`
    (amended away) to `d777a8f`.
  - Four proposals to pdf-lint, drafts kept local in `scratch/pdf-lint/proposals/`: PL-1 decode to the image
    filter, PL-2 a streaming writer, PL-3 one hayro-ccitt (and the MSRV route), PL-4 a bounded graphics-state
    walk. Each says what pdf-scan does meanwhile, so none blocks a slice.
  - The device layer's order (the maintainer): bindings first, `sane-dl` before `scan-device` in S3, a seam
    note between them (`crates/scan-device-seam.md`, first version written 2026-11-08), the model provisional
    until S4's `twain` backend; S4's spike made end to end; sheaf's adoption of the device crates after S4 at
    the earliest (`sheaf/swaps.md`).
  - S0 added, the scan devices first (the maintainer): S3 and S4 folded into `milestones/s0-devices.md`,
    twelve slices, Linux then Windows; S1 and S2 follow. The workspace S1 slice 1 had built (stashed; copied
    into `scratch/stash-s1-1/`) moves into S0 slice 1, reshaped around `sane-dl`. Asks 3 and 4 answered by it;
    6 and 7 taken as S0 slice 1 starts.
- **Done: S0 slice 7, the TWAIN spike** (merged 2026-11-08 as pull request #1, CI green;
  `archive/slices/s0-7-twain-spike.md`), on the Windows machine: the sample source installed silently, there
  and on `windows-latest`; TWAIN walked from state 1 to 7 and back in both bitnesses; native and memory
  transfers equal pixel for pixel; cancels from another thread; four code reviewers, every finding fixed; the
  seam note's TWAIN rows and `crates/twain-dsm.md` revised from it (TEST-3 done).
- **Done on its branch: S0 slice 8, `twain-dsm`** (on the Windows machine, branch `s0-8-twain-dsm`;
  `slices/s0-8-twain-dsm.md`): planned, reviewed (24 findings, all taken), its six steps built and pushed
  2026-11-08: the crate safe by construction (`decisions.md`; `crates/twain-dsm.md`), tested through the sample
  source in both bitnesses; the spike deleted. Three things the plan had wrong, found by its tests: the DSM
  doesn't close a source left open alone, its global condition code must be read first, and a catalog-signed
  Windows file can verify, as Microsoft's (refused either way). Code-reviewed by four reviewers in parallel and
  fixed, 2026-11-09 (the high findings: two copies of the crate in one process racing in the DSM, a source's
  message lost to another message loop on the thread, extended image info misread on 64-bit, a driver's large
  container panicking a 32-bit worker, tests that passed with the thing broken). Pull request #4, rebased onto
  `master` after #2 and #3 (its TEST-7 renumbered TEST-11, since `master` holds a TEST-7); CI on it and approval
  close it.
- **Done on its branch: S0 slice 1, `sane-dl` and the workspace it starts** (`slices/s0-1-sane-dl.md`;
  `137be1a` to the branch's head, `master` merged in after slice 7), through the `scan-slice` workflow
  (local): five steps, each checked, and the code review's fixes. Pushed, pull request #2.
- **Done on its branch: S0 slice 2, the seam note revised** (`slices/s0-2-seam-note.md`; `s0-2-seam-note`,
  from slice 1's tip, docs only): SANE's rows from slice 1 and its probes, the two SANE unknowns answered, the
  raw layout fixed, and the model written down for slice 3, then revised by its code review (25 findings
  fixed, BUG-7 deferred). Waits on the maintainer's review, the push and its pull request, stacked on slice
  1's; CI's run on the three systems then shows nothing of its own (docs only).
- **Waiting on review:** S0 slice 1 (its plan's "Decisions taken alone, for review", 72, and "For review
  (security)", 10; CI's run green on every job, which closed TEST-2 and TEST-8); S0 slice 2 (41 and 6); S0 slice
  8 (11 and 3). Also: `asks.md`, ask 5; UP-7 to UP-13, upstream reports; PL-1 to PL-3, ready to hand off on the
  maintainer's OK; PL-4, which first needs the maintainer's call on pdf-lint's "the interpreter stays out" line.
- **Next:** S0 slice 3, `scan-device` provisional, once slice 2's pull request is open; then slice 4 on
  Linux. On Windows, slice 9 (the `twain` backend), once slices 3 and 4 are done.
