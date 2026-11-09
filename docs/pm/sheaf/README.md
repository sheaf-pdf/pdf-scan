# sheaf: where the crates come from, and who takes them

sheaf (`~/repos/sheaf`, github.com/sheaf-pdf/sheaf) is the PDF editor these crates are for: the source of
most of their code and their spec (its M5 scan plan), and their first consumer. pdf-lint (`pdf-lint/`) is
the upstream pdf-scan builds on; sheaf is downstream.

- **Never changed from here; anything in it may be copied out** (Greg, 2026-11-08: "no changes in sheaf from
  here, but free to copy items out"; `decisions.md`, "Made here"). Its plan files this plan cites are copied
  as at `3232a35` into `scratch/reference/sheaf/` (local, not tracked); its open entries a crate here fixes
  are copied into `backlog/` and stay open in sheaf; its code is taken into each crate at the commit each
  slice pins.
- **It moves on without knowing this plan exists:** its plan doesn't point here, so the copies are checked for
  drift before each slice (the command is in `scratch/reference/README.md`).
- **It takes each crate in a swap of its own,** done in sheaf when Greg says, never from here (`sheaf/swaps.md`).
- These files, and the rest of `docs/pm/`, name sheaf; whether tracked files may is ask 7.

| File | What | Open it |
|---|---|---|
| `sheaf/from-sheaf.md` | The code and plan files each crate starts from, by file and line count, what changes in the cut, what stays in sheaf, and the prior art a cut or a port takes from | Cutting a crate from sheaf |
| `sheaf/swaps.md` | What sheaf takes after each milestone, what it replaces, and which of sheaf's entries each swap closes | When a milestone ends |
