# fake-libsane

Layer: Tests (`crates/README.md`). Paths are from `docs/pm/`.

**Built** in S0 slice 1 (`archive/slices/s0-1-sane-dl.md`): `/crates/fake-libsane/`, a `cdylib`, `publish = false`,
MIT OR Apache-2.0, no dependencies, MSRV 1.89. A crate of its own for good, not part of `scan-fixtures`
(`decisions.md`, "Made here"): `scan-fixtures` will be a library of pages that crates take as a
dev-dependency, with image codecs behind it, while the fake is a shared object loaded by its path, and must
carry none of that into the process that loads it.

- **What:** a stand-in for libsane that exports SANE's 14 functions, loaded by path in place of the
  system's. It plays what SANE's own `test` and `pnm` backends can't (the survey's narrowing,
  `research/prior-art/notes/sane-and-network-scanning.md`, section 6): seven backends' feeder and duplex
  option layouts (fujitsu, airscan, Brother, Canon DR, Lexmark, Epson, Kodak) with their page sizes active
  only for the feeder; duplex pages marked front or back in their pixels; descriptors that change after a
  set, and `INVAL` from `sane_start` with descriptors changed behind it; a feeder that ends where a test
  says; a jam or an open cover at sheet N, from a start or mid-page; Brother's `2500x0`; reads that hang
  until `sane_cancel`, or for good, or ignore it; an abort at sheet N; a slow listing; `sane_set_io_mode`
  answering `UNSUPPORTED` to either mode, as several real backends do (the code review; the `fujitsu`,
  `canon-dr`, `kodak` and `epson` profiles imply it); and hostile values of every kind (versions, statuses
  SANE doesn't define, lengths, frames, descriptors, device lists, null pointers, and pointers to no memory,
  `@dangling`, for a field the standard leaves undefined, as a group's description).
- **It checks SANE's calling rules:** calls overlapping, anything reaching a closed handle (`sane_cancel`
  included), `sane_exit` with handles open, a call after an error status or a batch's `NO_DOCS` that isn't
  `sane_cancel` or `sane_close`, a stale device list read (it's overwritten, never freed), and the refusals
  `sane-dl` promises (an inactive option, one software can't read or set, a call before descriptors are
  read again). Each is a `violation` line in its log, and every test that loads it asserts there are none.
- **Driven by** two environment variables a test sets on the child it spawns: `FAKE_LIBSANE_SCRIPT` (a
  script: devices, options, profiles, frames, sheets, faults) and `FAKE_LIBSANE_LOG` (every call, with its
  thread). Both formats are in its crate docs, so `scan-device`'s `sane` backend (S0 slice 5) can drive it as
  `sane-dl`'s tests do.
- **Boundary:** never published, no crate depends on it, of any kind, and it takes no dependency (`just gates`
  checks all three, the last since the code review): tests build it themselves (a nested `cargo build -p
  fake-libsane` into their own target directory, with the cargo that built the test) and load the shared
  object by its path. Its declarations are `sane-dl`'s `ffi.rs`, taken by `#[path]`, never a second copy, and
  each export is asserted at compile time to have its binding's type, so the header contract that checks the
  bindings checks the fake's too. Unsafe code in one module, its exports; the rest denies it.
- **Tested by:** its own unit tests (43 as built), each rule check shown catching its violation; and every
  `sane-dl` test that loads it.
- **Open:** CI's run of this code, where it's built and loaded as a DLL on Windows and a dylib on macOS; it
  waits on the push.
