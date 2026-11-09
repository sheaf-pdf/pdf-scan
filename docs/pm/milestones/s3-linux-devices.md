# S3: Devices on Linux

**SANE, the device model and the worker.** Needs nothing unbuilt; the hand check needs a scanner (ask 4).
Bindings first, the model drafted beside them and kept provisional (`decisions.md`, "Made here"): `sane-dl`
follows SANE alone and needs no model, so it comes first; the model's hard points (how a page arrives, its
side, pull or push, cancel) show up while writing a driver layer, so the seam note, first written from the
research, is revised after it; and the model isn't frozen here, since only one real backend has been
through it.

1. **sane-dl:** loading, bindings, the safe handle, the cancel token (synchronized with close), options,
   frames; the fake libsane in `scan-fixtures`, narrowed to what SANE's own backends can't do. It exposes what
   the seam needs (`crates/sane-dl.md`, "For the seam") as SANE gives it. Checked: SANE's `test` and `pnm`
   backends in CI and the bindings against the installed `sane.h` (TEST-2); the fake's cases; missing libsane
   said, not a crash; nothing links libsane.
2. **The seam note revised** (`crates/scan-device-seam.md`, first written 2026-11-08 from the survey and M5
   proposals 4 to 10): what step 1 showed of SANE folded in, its SANE unknowns answered (names that change on
   replugging, stopping after the page in flight), the raw layout's polarity and byte order fixed, and the
   unknowns TWAIN is to settle in S4 kept. Checked: reviewed before step 3 starts, since it fixes step 3's
   API.
3. **scan-device, provisional:** the model, the `Backend` trait, settings clamped and said, the `fake`
   backend and its scripts, as the seam note has them; 0.0.x, unpublished. Checked: the scripts (proposal 22)
   on every runner.
4. **scan-worker:** the protocol, `serve`, the client, the watchdog. Checked: the `fake` backend through a
   real worker process: a hang killed with the pages kept, a crash with the pages kept and the exit said, a
   cancel mid-page, a version mismatch, a page cut short; throughput of 300 dpi pages in color and black and
   white at 2.3 a second (M5 slice 6's gate, its pipe's share), measured on every run.
5. **scan-device's `sane` backend.** Checked: SANE's `test` backend in CI (a 10-sheet feeder then empty;
   gray, color, 1- and 16-bit; unknown height; three-pass; injected jam, cover open, no documents, I/O error:
   a jam ends the scan); `scan-fixtures`' pages through the `pnm` backend, byte for byte; the fake libsane's
   feeder and duplex names for each backend. Whatever the backend finds the model lacking goes into the seam
   note and the model, said.
6. **By hand:** an fi-6130 through SANE's fujitsu backend on a Linux laptop at the office (M5 ask 12; SANE
   rates it complete): 50 mixed sheets duplex, a forced jam, an open cover, unplugged mid-scan, a cancel. A
   row per scanner and driver in the notes.

**Done when** the three crates pass on every runner and the hand check is recorded. The device model
stays provisional: it's frozen in S4, once the `twain` backend has been through it.
