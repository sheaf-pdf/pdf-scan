# S0: Scan devices

**The device layer, before the pages and the PDF: SANE on Linux, then TWAIN and WIA on Windows; the device
model frozen once both have been through it.** The maintainer's order (2026-11-08: "we want to add a s0
milestone for the scan devices"): S0, then S1 pages, then S2 PDF (`decisions.md`). Needs nothing unbuilt; the
hand checks need scanners, and the Windows slices a Windows machine (ask 4's point, now answered: device work
starts before a scanner is at hand).

Bindings first, the model drafted beside them and kept provisional (`decisions.md`, "Made here"): `sane-dl`
follows SANE alone and needs no model, so it comes first; the model's hard points (how a page arrives, its
side, pull or push, cancel) show up while writing a driver layer, so the seam note, first written from the
research, is revised after it; and the model isn't frozen until the `twain` backend has been through it.
Slices that wait on hardware (the hand checks, and the Windows slices without a Windows machine) don't hold
S1 back: S1 can start once the Linux slices are done.

## On Linux

1. **sane-dl, and the workspace it starts.** First the workspace itself, from S1 slice 1's work (stashed,
   and copied into `scratch/stash-s1-1/`), reshaped so `sane-dl` is its first member: `Cargo.toml`,
   `LICENSE-MIT` and `LICENSE-APACHE`, `.gitattributes`, the root `README.md`, the justfile (`ci`, `fmt`,
   `clippy`, `test`, `doc`, `gates`, the MSRV recipes), CI on Ubuntu, Windows and macOS, and the gates for
   what exists (the devices layer takes no image codec, imageproc, PDF crate or GUI toolkit; nothing links
   libsane; no file outside `docs/pm/` names sheaf). Then `sane-dl`: loading, bindings, the safe handle, the
   cancel token (synchronized with close), options, frames, and the fake libsane, `fake-libsane`, a
   never-published `cdylib` crate of its own for good (not moved into `scan-fixtures` with S1:
   `crates/fake-libsane.md`, `decisions.md`), narrowed to what SANE's own backends can't do. It exposes what
   the seam needs (`crates/sane-dl.md`, "For the seam") as SANE gives it. Its MSRV measured as S1 slice 1
   measured the pixel crates'. Checked: `just ci`; SANE's `test` and `pnm` backends in CI and the bindings
   against the installed `sane.h` (TEST-2); the fake's cases; missing libsane said, not a crash; each gate
   failing on a violation made for the check.
   **Done on the branch** (2026-11-08; `slices/s0-1-sane-dl.md`): branch `s0-1-sane-dl`, from `137be1a`
   to its head (the code as of `493b0c7`, its privacy commit `9b8bd5a`, then the record). `sane-dl`
   0.1.0-alpha.1, ready to publish and not published, MSRV 1.89, with `fake-libsane`; its five steps checked
   and its code review's 22 findings fixed. For review: the plan's "Decisions taken alone, for review" (72)
   and "For review (security)" (10), above all the code review's API changes before the first publish
   (`Sane::load_from` an `unsafe fn`, `DEFAULT_LIBRARIES` with macOS's absolute paths, the `links` key and its
   empty build script, `LimitExceeded`, `ReloadAfterSet`, `options_generation`, `SetInfo` non-exhaustive,
   `clear_cancel`); then the push and the pull request, and CI's run on the three systems (TEST-2, TEST-8),
   which closes the slice.
2. **The seam note revised** (`crates/scan-device-seam.md`, first written 2026-11-08 from the survey and M5
   proposals 4 to 10): what slice 1 showed of SANE folded in, its SANE unknowns answered (names that change
   on replugging, stopping after the page in flight), the raw layout's polarity and byte order fixed, and the
   unknowns TWAIN is to settle kept. Checked: reviewed before slice 3 starts, since it fixes slice 3's API.
   **Done on the branch** (2026-11-09; `slices/s0-2-seam-note.md`), waiting on review: branch
   `s0-2-seam-note`, from slice 1's tip `4624760`, docs only, three steps, each checked. The note's SANE rows
   from slice 1 and the slice's probes, the two SANE unknowns answered (what only slice 6 settles is TEST-9),
   the raw layout fixed and held beside `scanimage`'s pages, and the model written down for slice 3 ("The
   model, for slice 3"); the backlog's items for slices 5 and 6. Its code review next, which with the plan's
   review is the review this entry asks for; then its pull request, stacked on slice 1's, reviewed by the
   maintainer out of band, and slice 3 starts once it's open. For review: the plan's "Decisions taken alone,
   for review" (26) and "For review (security)" (6).
3. **scan-device, provisional:** the model as the seam note's "The model, for slice 3" has it (its types, the
   `Backend` and `Device` traits, `RawImage`'s checked constructor, cancel and progress through `in-progress`,
   taken here: vetted in slice 2's plan, the devices layer's gate holding it, and `plan.md`'s graph updated),
   settings clamped and said, the `fake` backend and its scripts; 0.0.x, unpublished. Checked: the scripts
   (proposal 22) on every runner, among them a cancel after a duplex front (its back still delivered) and a
   stored id opened as a renamed device; `RawImage::new`'s checks unit-tested (the length, a 1-bit row's
   padding bits, a size past `u64` or a 32-bit `usize` refused, never wrapped).
4. **scan-worker:** the protocol, `serve`, the client, the watchdog. The protocol as the seam note's "What
   slice 4 needs of it" has it: an `open` request answered with `renamed_from`, before `caps` and `scan`; an
   `applied` event; progress lines from the worker's `Progress`; `cancel` cancelling the batch's tracker, and
   the kill after it allowing for the sheet in flight; a `settings_window` request; a page's format and size
   before its bytes, and a `RawImage` decoded through `RawImage::new`; and a SANE worker that opens a stored id
   spawned with that id's one-backend `ConfigDir` (`Search::BeforeSystem`, naming none for an id with no
   backend), in a directory it trusts. Checked: the
   `fake` backend through a real worker process: a hang killed with the pages kept, a crash with the pages
   kept and the exit said, a cancel mid-page (the sheet finished, none fed after), a version mismatch, a page
   cut short; the raw layout across the pipe byte for byte; throughput of 300 dpi pages in color and black
   and white at 2.3 a second (M5 slice 6's gate, its pipe's share), measured on every run.
5. **scan-device's `sane` backend**, mapping `sane-dl` onto the model as the seam note's SANE rows and its
   table in "The model, for slice 3" say. Checked: SANE's `test` backend in CI (a 10-sheet feeder then empty;
   gray, color, 1- and 16-bit; unknown height; three-pass; injected jam, cover open, no documents, I/O error:
   a jam ends the scan), 16-bit and 1-bit RGB in the model's raw layout (the 16-bit conversion a function of
   the source's byte order, so its swap is unit-tested on CI's little-endian hosts); pages through the `pnm`
   backend, its PBM compared after the inversion (PBM is 1 black, the model 0), PGM and PPM byte for byte; ids
   from Latin-1, back to `sane_open` byte for byte; the fallback on `INVAL` at open in a worker given the id's
   one-backend `ConfigDir` (`Search::BeforeSystem`, naming none for an id with no backend), the fake listing a
   device under a new name; `dll`'s testing prefixes, a leading `:`, the empty name and an id not Latin-1
   refused before `sane_open` (`BadId`); `INVAL` from a start (the fake's `at PAGE start INVAL`) met by
   setting the settings again and starting once more, and `Gone` when it repeats; the feeder keeping its
   sheets after a cancel; the fake libsane's feeder and duplex names for
   each backend. Whatever the backend finds the model lacking goes into the seam note and the model, said.
6. **By hand on Linux:** an fi-6130 through SANE's fujitsu backend on a Linux laptop at the office (M5 ask
   12; SANE rates it complete): 50 mixed sheets duplex, a forced jam, an open cover, unplugged mid-scan, a
   cancel. A row per scanner and driver in the notes. Waits on the scanner; doesn't hold the rest back.

## On Windows

7. **The TWAIN spike, end to end:** TWAIN's sample data source installed silently on `windows-latest`
   (`msiexec /qn`, or yushulx/virtual-scanner's portable zip, or the DSM and the source copied into
   `twain_32` and `twain_64`) and scanned with a hidden window, in both bitnesses (TEST-3).
   - If it installs: a throwaway scan through it, the DSM loaded and walked from state 1 to 7, one memory
     transfer and one native, reading each transfer's image info, the pending-transfer count and whatever
     extended image info the source offers. It answers the seam note's TWAIN unknowns
     (`crates/scan-device-seam.md`) before `twain-dsm`'s API is fixed, and CI scans through TWAIN from
     then on.
   - If it doesn't: a desk check of the seam note against `TWAINH.cs` and NAPS2's notes (study only), and the
     hand checks carry the rest.
   - Either way the seam note is revised, said, before slice 8.
   - **Done here, 2026-11-08, ahead of slices 2 to 6** (ruled 2026-11-08; `slices/s0-7-twain-spike.md`): it
     installed silently, here and on `windows-latest`, and scanned with a hidden window in both bitnesses
     here: native and memory, two delivery modes, a pixel-for-pixel comparison, a cancel from another thread.
     The seam note is revised (its TWAIN rows). CI's `twain-spike` job ran the scenarios on
     `windows-latest` in both bitnesses, green on every push (TEST-3, done). The code review, its fixes, CI
     green on them and approval close the slice.
8. **twain-dsm:** bindings ported from `TWAINH.cs` (MIT; `TWAIN.cs`, beside it, wraps it) and checked
   against `twain.h` 2.5, the state
   machine, the STA thread and hidden top-level window, capabilities, transfers, condition codes; built for
   both targets. It exposes what the seam needs (`crates/twain-dsm.md`, "For the seam").
9. **scan-device's `twain` backend** and a 32-bit worker in CI (`i686-pc-windows-msvc`, the worker alone),
   the client's merge of both lists. The model's changes the backend asks for are made here, in
   `scan-device`, the `fake` and `sane` backends and the worker's protocol together. Checked: 10 images
   through the sample source in both bitnesses (if the spike worked); the error mapping in unit tests; the
   Linux slices' checks again, on the changed model.
10. **scan-device's `wia` backend:** its STA thread, the feeder and duplex, errors by code. Checked against a
    fake of the WIA calls behind a trait (no virtual WIA device installs without a person).
11. **The model frozen:** the seam note's unknowns all answered, `scan-device` and `scan-worker` out of 0.0.x
    (0.1.0), the protocol's version bumped once for it.
12. **By hand on Windows:** an fi-6130 on FUJITSU TWAIN32 through the 32-bit worker (and PaperStream IP x64
    if the office's are fi-6130Z), duplex, a jam, a double feed; a WIA scanner and a network copier. Waits on
    whether a development build may run on a work PC (M5 ask 12's open point; sheaf's
    `topics/code-signing.md`).

**Done when** the device crates pass on every runner, the model is frozen (slice 11), and the hand checks are
recorded. sheaf takes the device crates no sooner (`sheaf/swaps.md`).
