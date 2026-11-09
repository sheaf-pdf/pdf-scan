# sane-dl

Layer: Devices (`crates/README.md`). Paths are from `docs/pm/`.

- **What:** SANE for Rust, loaded at run time (M5 proposal 6, M5 ask 6, entry 5): `libsane.so.1` opened by
  `libloading` (0.9, ISC), or a library the caller names (sheaf's tests: `SHEAF_LIBSANE`; the env var is
  the app's). Without it, nothing fails until it's asked for, and then it says so.
  - Bindings of its own from SANE's public-domain `sane.h`: its 14 functions and their structs. The grant
    asks that the notice be kept and changes marked: both done in the bindings' header. No bindgen, no
    build-time link.
  - Statuses and frame types as integers with named constants, never Rust enums: statuses 12 and 13 are
    reserved under `#if 0`, and an enum read from FFI is undefined behaviour the day a backend returns one.
  - Safe types: the library (one `sane_init` per process), a device list under a deadline (it took 10.7 s
    on Greg's machine with nothing attached), a handle used from one thread, and a `Send` cancel token that
    calls `sane_cancel` from any thread (the standard allows it asynchronously). The token is synchronized
    with close: the standard forbids every other call, `sane_close` included, until the cancelled one
    returns, so a late cancel must never reach a closed handle (airbridge's alive flag, MIT OR Apache-2.0,
    is the pattern). `sane_cancel` after every batch and every error status.
  - Opening a known device fast: a private `SANE_CONFIG_DIR` whose `dll.conf` names that device's backend
    alone (NAPS2's practice), so it doesn't wait on every backend's probe.
  - Options typed: descriptors, constraints (ranges, word lists, string lists), units, and descriptors read
    again whenever a set says so (`SANE_INFO_RELOAD_OPTIONS`) or `sane_start` returns `SANE_STATUS_INVAL`.
  - Frames assembled: `lines = -1` read to EOF; padded rows; 1-bit rows with 1 black and the first pixel in
    the high bit; 16-bit samples in native order; three-pass frames into one image; `NO_DOCS` from
    `sane_read` as well as `sane_start`. NAPS2 handles 1- and 8-bit transfers only, so 16-bit, padded and
    unknown-length frames go beyond every frontend and are proven against the test backend.
- **For the seam** (`crates/scan-device-seam.md`): each frame's parameters as SANE gives them
  (format, last frame, bytes and pixels per line, lines, depth), each read's status, and each option
  reload, all exposed, so the `sane` backend decides what they mean and `sane-dl` stays SANE alone.
- **Prior art:** `sane-scan` 0.1.2 (2023), the one published binding, links at build time (bindgen, libclang,
  `-lsane`), passes a one-byte `bool` where backends read a four-byte word, sizes `lines = -1` buffers wrong,
  turns a mid-page `JAMMED` into a truncated `Ok`, and hasn't answered a PR since 2023. Three permissive
  apps load libsane as planned (airbridge, janus-scan, ruscan), each weeks old and none a library: read in
  full before a pattern is borrowed. printscan-sane (an open PR) has `protocol.rs` and `scan/frame.rs` to
  cross-check against.
- **Boundary:** SANE alone; no device model (that's `scan-device`'s `sane` backend), so it's useful to anyone
  who wants SANE.
- **Tested by:**
  - SANE's own `test` backend in CI (`libsane1` installed, `SANE_CONFIG_DIR` at a temporary `dll.conf`
    saying `test`): a 10-sheet feeder then `NO_DOCS`; gray and color, 1- and 16-bit; unknown height; short
    reads; three-pass frames in any channel order; padded rows (`ppl-loss`); 16-bit byte swapping; any
    status from every `sane_read` (`JAMMED`, `COVER_OPEN`, `NO_DOCS`, `IO_ERROR`); `SANE_INFO_INEXACT` and
    every constraint type; non-blocking reads with a select fd. It has no duplex.
  - SANE's `pnm` backend (built in Debian's package; Ubuntu's runners unchecked): `scan-fixtures`' pages
    scanned through real libsane, frames checked byte for byte.
  - `scan-fixtures`' fake libsane (a `cdylib` exporting SANE's API), narrowed to what those can't do: each
    backend's feeder and duplex names, a jam at sheet N, `2500x0`, `lines = -1` from a feeder, duplex sides,
    descriptors that change after a set, a backend that crashes or hangs.
  - The bindings checked against the installed `sane.h` in CI (sanetui's header-contract test, Apache-2.0,
    as the idea), statuses included: printscan's own fake gave `NO_DOCS` the code 6, which is `JAMMED`.
  - A gate that nothing links libsane (`! ldd … | grep libsane`). What's unchecked about these: TEST-2.
- **Open:** whether it builds and loads on macOS against Homebrew's sane-backends (no one asked; free to
  try in CI).
