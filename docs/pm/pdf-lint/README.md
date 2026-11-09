# pdf-lint: the PDF family pdf-scan consumes

pdf-scan's PDF crates (`pdf-images`, `pdf-scan`) are built on the PDF crate family in pdf-lint's workspace
(github.com/sheaf-pdf/pdf-lint, checked out at `~/repos/sheaf-arlington`). That family keeps moving, and most
of it is in alpha releases. pdf-scan is its consumer, not its co-author (Greg, 2026-11-08: "pdf-lint will
continue to move. it is currently in alpha releases for most of the family of libraries. As a consumer of
these crates if you identify any major shortcomings we should create proposals that can be handed off to them
for refactor"). So:

- **Nothing is changed in pdf-lint from here.** Its code, plan and backlog are read, never written.
- **A major shortcoming becomes a proposal** in `scratch/pdf-lint/proposals/`, written to be handed to
  pdf-lint as it stands,
  for pdf-lint to plan and do as a refactor in its own workspace. A minor one is a line in a proposal's
  "Smaller points", or waits until it's worth one.
- **Meanwhile pdf-scan works around it**, and the proposal says how, so no slice here waits on a handoff.
- **Proposals and assessments of the family stay local** (`scratch/pdf-lint/`, gitignored): they cite
  pdf-lint's own unpublished plan (its backlog, its slices, its audit), which this public repository never
  repeats. What's tracked here says only what pdf-scan needs and does.

## What pdf-scan consumes

As of 2026-11-08: crates.io (looked up the same day) and the checkout at `d777a8f`
(2026-11-08). The family's MSRV is 1.89 in every crate.

| Crate | On crates.io | In the checkout | Used by |
|---|---|---|---|
| pdf-objects | 0.1.0-alpha.2 (2026-11-07) | 0.1.0-alpha.2 | `pdf-images`, `pdf-scan` |
| pdf-filters | 0.1.0-alpha.2 (2026-11-07) | 0.1.0-alpha.2 | `pdf-images`, `pdf-scan` (Flate) |
| pdf-color | 0.1.0-alpha.1 (2026-11-07) | 0.1.0-alpha.1 | `pdf-images` |
| pdf-crypt | 0.1.0-alpha.1 (2026-11-07) | 0.1.0-alpha.2 | through pdf-structure (encrypted files read back) |
| pdf-tokenizer | 0.2.1 (2026-11-07) | 0.2.1 | through pdf-objects |
| pdf-structure | not published | 0.1.0-alpha.1 | `pdf-scan` (`write::full`, `write::Parts`, `write::update`; reading) |
| pdf-content-streams | not published | 0.1.0 | `pdf-scan` (the text layer's writer; reading a page's operators) |
| pdf-fonts | not published | 0.1.0 | maybe `pdf-scan` (the glyphless font) |
| pdf-semantics, pdf-linearize, pdf-arlington | not published | 0.1.0 | not planned |
| pdf-fixtures | never published | 0.0.0 | tests, by path |
| pdf-lint | 0.1.0 (2026-10-26) | 0.2.0 | tests: every file `pdf-scan` writes checked clean |

Which needs each crate covers, and how well, is folded into `crates/pdf-images.md` and `crates/pdf-scan.md`
from the family's fit (local, `scratch/pdf-lint/pdf-family-fit/`). When each unpublished crate is due is
pdf-lint's plan: read it before each slice that waits on one.

## How pdf-scan depends on it

- **A published crate is pinned exactly** (`pdf-objects = "=0.1.0-alpha.2"`; Greg, 2026-11-08: "pin alphas
  exactly is fine"): alphas may break between releases, so a bump is a change of its own, made after reading
  what moved.
- **An unpublished crate is taken by path** (or a git revision), and the crate that takes it can't publish
  until it's on crates.io: crates.io accepts no path or git dependencies. Today that holds `pdf-scan` back
  (pdf-structure, pdf-content-streams), not `pdf-images`.
- **One version of each shared dependency across both workspaces** where a build carries both (hayro-ccitt,
  zune-jpeg, the Flate backends): a split is a shortcoming to propose (Q-1).

## Keeping up with it

Before a slice that touches a family crate: read what moved since the version or commit last read (its
`CHANGELOG`, `git log <last>..HEAD -- crates/<crate>`, its release notes), note the new version or commit in
`status.md`, and see whether an open proposal was taken, changed or overtaken. Assessments here always say
the commit they read.

## Proposals

### When to write one

A shortcoming is major when one of these holds:

- a need of `pdf-images` or `pdf-scan` can't be met through the crate's public API without copying or
  re-deriving what the crate already does inside;
- a workaround here would duplicate family work that every consumer will want (a second writer, a second
  parser, a second color model);
- a design point would be costly to change after the crate's 0.1.0 (an API shape, an ownership or
  streaming model, determinism, limits on hostile input);
- two workspaces would carry two versions of one dependency, or the family's MSRV or features would force
  something on every consumer.

### How one is written
 One file each, `scratch/pdf-lint/proposals/plNN-<short-name>.md`, IDs `PL-1`, `PL-2`, … never reused. Written
for pdf-lint's readers, to be pasted into its plan as it stands: in pdf-lint's terms, naming pdf-scan and its
crates as the consumer, and never naming sheaf, its milestones or its corpus (pdf-lint's rule for what it
tracks). Evidence is cited at a commit, by file and line, so it can be checked after the family moves.

```markdown
# PL-N: <the change, in a line>

Status: draft | ready (Greg's OK to hand off) | handed off (date, how) | taken (where) | declined (why)
        | landed (version)
Crates: <the family crates it touches>
Read at: <pdf-lint commit>, <date>

## The need
What pdf-scan's crate needs, in the consumer's words, and why it's the family's to provide.

## What the family does today
The public API as it stands, with file:line at the commit above; what's missing or in the way.

## The proposal
The change: API, behaviour, limits; breaking or additive (alphas may break: say so either way).

## Alternatives
Other shapes, including doing nothing; why this one.

## How the family can check it
Tests, fuzz targets or oracle comparisons pdf-lint can run on its own, without pdf-scan.

## Meanwhile
What pdf-scan does until it lands, and what it drops when it does.

## Smaller points
Minor shortcomings found on the way, a line each.
```

### Handing one off

Greg's OK first, as for anything that leaves this workspace. Then Greg carries it to pdf-lint, or a pdf-lint
session reads it from here; nothing is written into pdf-lint's repository from here. Its status line and the
index below record each step.

## Index

From the family's fit (local), read at `d777a8f`. The files are in `scratch/pdf-lint/proposals/`. Each is
a draft until Greg's OK.

| ID | Change | Crates | Status |
|---|---|---|---|
| PL-1 | Decode a stream through its standard filters and stop at its image filter (`pl01-decode-to-the-image-filter.md`) | pdf-structure, pdf-objects; pdf-content-streams, pdf-color | draft |
| PL-2 | Write a file to an `io::Write` a body at a time, whole or as an update (`pl02-streaming-writer.md`) | pdf-structure | draft |
| PL-3 | One hayro-ccitt for every consumer: `image-ends` on 0.5, and the MSRV route (`pl03-one-hayro-ccitt.md`) | pdf-content-streams; MSRV policy | draft |
| PL-4 | A bounded graphics-state walk through q/Q/cm and forms (`pl04-graphics-state-walk.md`) | pdf-content-streams | draft; deferrable, and on the "interpreter stays out" line, so Greg's call first |

Smaller points not in a proposal, pdf-scan's to live with: pdf-fixtures is path-only and unpublished, so a
consumer's tests run their own qpdf or reach into pdf-lint's checkout; pdf-filters has no per-row PNG
predictor choice, a size point for scanned gray pages; pdf-lint's oracle renders nothing, so
bitmaps come from pdfium-render here (the PDFium pin can be shared); pdf-lint's checks cover dictionaries
(Arlington's image and filter-parameter tables), not samples.
