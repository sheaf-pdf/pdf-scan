# fake-libsane

Layer: Tests (`crates/README.md`). Paths are from `docs/pm/`.

**Built** in S0 slice 1 (`archive/slices/s0-1-sane-dl.md`): `/crates/fake-libsane/`, a `cdylib`, `publish = false`,
MIT OR Apache-2.0, no dependencies, MSRV 1.89. A crate of its own for good, not part of `scan-fixtures`
(`decisions.md`, "Made here"): `scan-fixtures` will be a library of pages that crates take as a
dev-dependency, with image codecs behind it, while the fake is a shared object loaded as SANE, and must carry
none of that into the process that loads it.

- **What:** a stand-in for libsane that exports SANE's 14 functions, loaded as SANE in place of the system's,
  through the system loader's search path (S0 slice 1a; below). It plays what SANE's own `test` and `pnm`
  backends can't (the survey's narrowing, `research/prior-art/notes/sane-and-network-scanning.md`, section 6):
  seven backends' feeder and duplex option layouts (fujitsu, airscan, Brother, Canon DR, Lexmark, Epson, Kodak)
  with their page sizes active only for the feeder; duplex pages marked front or back in their pixels;
  descriptors that change after a set, and `INVAL` from `sane_start` with descriptors changed behind it; a feeder
  that ends where a test says; a jam or an open cover at sheet N, from a start or mid-page; Brother's `2500x0`;
  reads that hang until `sane_cancel`, or for good, or ignore it; an abort at sheet N; a slow listing;
  `sane_set_io_mode` answering `UNSUPPORTED` to either mode, as several real backends do (the code review; the
  `fujitsu`, `canon-dr`, `kodak` and `epson` profiles imply it); and hostile values of every kind (versions,
  statuses SANE doesn't define, lengths, frames, descriptors, device lists, null pointers, and pointers to no
  memory, `@dangling`, for a field the standard leaves undefined, as a group's description).
- **It checks SANE's calling rules:** calls overlapping, anything reaching a closed handle (`sane_cancel`
  included), `sane_exit` with handles open, a call after an error status or a batch's `NO_DOCS` that isn't
  `sane_cancel` or `sane_close`, a stale device list read (it's overwritten, never freed), and the refusals
  `sane-dl` promises (an inactive option, one software can't read or set, a call before descriptors are
  read again). Each is a `violation` line in its log, and every test that loads it asserts there are none.
- **Driven by** two environment variables a test sets on the child it spawns: `FAKE_LIBSANE_SCRIPT` (a
  script: devices, options, profiles, frames, sheets, faults) and `FAKE_LIBSANE_LOG` (every call, with its
  thread). Both formats are in its crate docs, so `scan-device`'s `sane` backend (S0 slice 5) can drive it as
  `sane-dl`'s tests do.
- **How tests load it** (S0 slice 1a, `slices/s0-1a-sane-dl-safe-api.md`): through `Sane::load()`, which
  takes no path, so no route a published API exposes. A test builds the fake, copies it under SANE's file name
  (`libsane.so.1`, `libsane.1.dylib` on macOS) into a directory of its own per test binary, written under a
  temporary name and renamed, and spawns each child with the loader's variable (`LD_LIBRARY_PATH`,
  `DYLD_LIBRARY_PATH` on macOS) naming that directory first; the child's `Sane::load()` then finds the fake
  ahead of any installed libsane, and its log is checked after every load, so a test can't pass on the
  system's. Windows has no default library, so no route: there the tests that load it are ignored, each
  saying why. `scan-device`'s `sane` backend (S0 slice 5) copies the small helper.
- **Boundary:** never published, no crate depends on it, of any kind, and it takes no dependency (`just gates`
  checks all three, the last since the code review): tests build it themselves (a nested `cargo build -p
  fake-libsane` into their own target directory, with the cargo that built the test) and load it as SANE
  (above). Its declarations are `sane-dl`'s `ffi.rs`, taken by `#[path]`, never a second copy, and
  each export is asserted at compile time to have its binding's type, so the header contract that checks the
  bindings checks the fake's too. Unsafe code in one module, its exports; the rest denies it.
- **Tested by:** its own unit tests (43 as built), each rule check shown catching its violation; and every
  `sane-dl` test that loads it.
- **Open:** CI's run of this code on macOS, where it's built as a dylib and found through
  `DYLD_LIBRARY_PATH` (TEST-7); on Windows it's built, and the tests that would load it are ignored. Both wait
  on the push.
