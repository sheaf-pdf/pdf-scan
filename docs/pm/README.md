# docs/pm: pdf-scan's project management

The plan, its status, its decisions, its backlog and its research, kept in the repository (decided 2026-11-08:
"I want to keep the project management files in docs/pm"). **Open `status.md` first, then only what the work
needs.**

pdf-scan is the workspace for the scanning crates sheaf will depend on: scanners driven through each system's
driver API, page images cleaned up and read for codes, and scanned pages written to PDF and read back. Its PDF
crates are built on the PDF family in pdf-lint's workspace (github.com/sheaf-pdf/pdf-lint), as its consumer.

These files may name sheaf, its plan and its corpus; the code, crate docs, crate READMEs and commit messages
never do (ask 7; `just gates` checks it). The repository is public, so nothing private goes in here
(`decisions.md`). **Paths** in them are from `docs/pm/`, except those starting
`scratch/` (from the repository's root, local and gitignored).

## What's where

| Path | What | Open it |
|---|---|---|
| `status.md` | What's happening now, what's next, what waits on review. Short | Every session |
| `plan.md` | The goal, the crates' layers and graph, what's not in them, the milestones in order, the standing rules | Starting anything new |
| `decisions.md` | What's settled, in sheaf's plan, pdf-lint's or here, with where it was ruled | Before changing anything foundational |
| `asks.md` | Questions waiting on review, the recommended answer first | When work waits on a ruling |
| `crates/` | `README.md`: the layers and an index; then a file per crate: what it does, its boundary, its dependencies, its prior art, how it's tested, what's open | Planning or building a crate |
| `milestones/` | `README.md`, then a file per milestone (S0, S1, S2, `later.md`): its slices, what each builds and how it's checked | Planning or running a slice |
| `slices/` | The plans of slices while they run, one file each: S0 slice 9's, `s0-9-twain-backend.md` | Running a slice |
| `backlog/` | Everything open, a file per kind: BUG, FEAT, DOC, TEST, Q (the plan's questions), UP (contributions to other projects) | Planning a slice; when something turns up |
| `research/` | Surveys and assessments, a folder each (`README.md` the document, `notes/` behind it): `prior-art/` | Before deciding to build anything |
| `pdf-lint/` | Upstream: the PDF family as pdf-scan consumes it (versions, exact pins, keeping up) and how its shortcomings are proposed back; the proposals themselves are local, in `scratch/pdf-lint/` | Before touching a family crate; when one falls short |
| `sheaf/` | Downstream and origin: the rules for sheaf, `from-sheaf.md` (what each crate takes from it), `swaps.md` (what sheaf takes after each milestone) | Cutting a crate from sheaf; when a milestone ends |
| `archive/` | Done slices' plans (S0 slices 1, 1a, 2, 3, 4, 7 and 8 so far) and milestones, and `backlog-done.md` | Looking back |
| `scratch/reference/` | Local, not tracked: `scratch/reference/sheaf/`, the sheaf plan files cited here, copied as at `3232a35` and never edited | When a crate's spec is wanted |
| `scratch/pdf-lint/` | Local, not tracked: the family's fit (`pdf-family-fit/`) and the proposals to pdf-lint (`proposals/`), kept out of the public repository because they cite pdf-lint's own unpublished plan | Before touching a family crate; handing a proposal off |

Later, local: `scratch/harness/`, an export of sheaf at a pinned commit, to hold cut crates to sheaf's output
byte for byte.

## Citations

- This plan's own: `ask N` (`asks.md`); backlog items `BUG-n`, `FEAT-n`, `DOC-n`, `TEST-n`, `Q-n`, `UP-n`
  (`backlog/`); proposals to pdf-lint `PL-n` (`scratch/pdf-lint/proposals/`, local); "the survey" is
  `research/prior-art/`.
- sheaf's plan, each copied in `scratch/reference/sheaf/` under its path below `plan/`: `M5 proposal 6`, `M5
  ask 3`, `M5 slice 6` are in `m05-scan/`; `entry 33` is `research/build-or-adopt/33-*.md`; `D17` is that
  folder's `decisions.md`; `B24` and `F10` are sheaf's backlog entries, `backlog/B24.md`. sheaf itself
  (github.com/sheaf-pdf/sheaf) is never changed from here (`sheaf/README.md`).
- pdf-lint's plan: its `scratch/plan.md`, `scratch/decisions.md`, and sheaf's M18 files
  (sheaf's `plan/m18-pdf-layer/`).
- Read at sheaf `3232a35` (2026-10-28). pdf-lint keeps moving, so each assessment of it says the commit it
  read: the plan was first drawn at `8b5de62` (since amended away; it survives only in the reflog), and
  the family's fit (local, `scratch/pdf-lint/pdf-family-fit/`) read `d777a8f`.
