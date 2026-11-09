# Status

As of 2026-11-09.

**S0 slice 4, `scan-worker`, is done on its branch, `s0-4-scan-worker`** (from `master` at `5a4bff2`, `master`
merged in after #7), its code review's findings fixed, pushed, in pull request #8, waiting on review and CI
("Running", below). **S0 slices 1, 1a, 2, 3 and 8 are merged** (pull requests #2, #7, #3, #6 and #4, squashed;
#5 recorded slice 8's close), as **S0 slice 7, the TWAIN spike,** is (#1); slices 7 and 8 from the Windows
machine. It's a git repository at github.com/sheaf-pdf/pdf-scan (public; `master`).

- **Running: S0 slice 4, done on `s0-4-scan-worker`** (`slices/s0-4-scan-worker.md`; the code to `510d349`):
  five steps, each checked, then its code review's 20 findings (5 major), 18 fixed whole and two in part, none
  refuted: `scan-worker` 0.0.0, unpublished, MSRV 1.89 (the protocol, `serve`, the client and its watchdog,
  through a test worker), and `scan-device`'s `serde` feature and `Fault::AwaitCancel`; the review's fixes
  watch a worker's process while a call waits, keep its input open while it quits, and check more of the
  model's rules. **Waits on:** the maintainer's review (55 decisions, 11 security items); CI's run of pull
  request #8 on Ubuntu, Windows and macOS (TEST-16). Left: FEAT-14 and FEAT-17 (slice 9), FEAT-16 (before
  slice 11), FEAT-15.
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
- **Done: S0 slice 8, `twain-dsm`** (merged 2026-11-09 as pull request #4, CI green on every job, run
  42617750221; `archive/slices/s0-8-twain-dsm.md`), on the Windows machine: planned, reviewed (24 findings, all
  taken), its six steps built and pushed 2026-11-08: the crate safe by construction (`decisions.md`;
  `crates/twain-dsm.md`), tested through the sample source in both bitnesses; the spike deleted. Three things
  the plan had wrong, found by its tests: the DSM doesn't close a source left open alone, its global condition
  code must be read first, and a catalog-signed Windows file can verify, as Microsoft's (refused either way).
  Code-reviewed by four reviewers in parallel and fixed, 2026-11-09 (the high findings: two copies of the crate
  in one process racing in the DSM, a source's message lost to another message loop on the thread, extended
  image info misread on 64-bit, a driver's large container panicking a 32-bit worker, tests that passed with the
  thing broken). Rebased onto `master` after #2 and #3 (its TEST-7 renumbered TEST-11, since `master` holds a
  TEST-7).
- **Done: S0 slice 1, `sane-dl` and the workspace it starts** (merged 2026-11-09 as pull request #2, squashed
  `3ebea15`, CI green; `archive/slices/s0-1-sane-dl.md`): `sane-dl` 0.1.0-alpha.1, ready to publish and not
  published, with `fake-libsane` for its tests; five steps, each checked, and its code review's 22 findings fixed.
- **Done: S0 slice 3, `scan-device` provisional** (merged 2026-11-09 as pull request #6, squashed `5a4bff2`;
  `archive/slices/s0-3-scan-device.md`): the seam note's model and the `fake` backend, every script of proposal
  22 tested; CI green on all ten jobs (run 42644985306), which closed TEST-13. Later: TEST-12, Q-5, Q-6.
- **Done: S0 slice 1a, `sane-dl`'s public API made safe** (merged 2026-11-09 as pull request #7, squashed
  `42e4227`; CI green on all eleven jobs, run 42648083514, which closed TEST-15;
  `archive/slices/s0-1a-sane-dl-safe-api.md`): four steps, each checked, and the code review's 22 findings all
  fixed. No public item of `sane-dl` is `unsafe`: `Sane::load_from` gone, the library the system loader's
  choice; on Linux one SANE per process across copies of the crate; `just public-api` over every public item;
  BUG-7 done. `Sane::load`'s safety rests on two rules of the environment, in its docs (decision 33; Q-7, UP-14).
- **Done: S0 slice 2, the seam note revised** (merged 2026-11-09 as pull request #3, squashed `70d7b20`;
  `archive/slices/s0-2-seam-note.md`; docs only): SANE's rows from slice 1 and its probes, the two SANE unknowns
  answered, the raw layout fixed, and the model written down for slice 3, then revised by its code review (25
  findings fixed, BUG-7 deferred).
- **Waiting on review:** S0 slice 1 (its plan's "Decisions taken alone, for review", 72, and "For review
  (security)", 10; CI's run green on every job, which closed TEST-2 and TEST-8); S0 slice 1a (40 and 11); S0
  slice 2 (41 and 6); S0 slice 3 (50 and 6); S0 slice 4 (55 and 11); S0 slice 8 (11 and 3). Also: `asks.md`, ask
  5; UP-7 to UP-13, upstream reports; PL-1 to PL-3, ready to hand off on the maintainer's OK; PL-4, which first
  needs the maintainer's call on pdf-lint's "the interpreter stays out" line.
- **Next:** S0 slice 5, `scan-device`'s `sane` backend, run in a worker the `scan-worker` client starts, once
  slice 4 is merged (its TEST-10, FEAT-12 and Q-4). On Windows, slice 9 (the `twain` backend), once slices 3
  and 4 are done.
