# Changelog

## 0.1.0-alpha.1 (unreleased)

The first release, and an alpha: until a release says it's stable, any alpha may change the API, each saying
here what changed and how code follows. A crate that depends on an alpha names it exactly, `=0.1.0-alpha.1`:
cargo's default, `0.1.0-alpha.1`, takes any later alpha of 0.1.0 too. The heading takes its date when the
version is published.

- **SANE loaded at run time.** `Sane::load` opens the system's library by its versioned name
  (`libsane.so.1`, `libsane.1.dylib` on macOS; none on Windows, `LoadError::NoDefault`), and
  `Sane::load_from` one by its path; then `sane_init`, with no authorization callback, and a SANE other than
  version 1 refused after its `sane_exit`. `LoadError`: `NotFound` with the names tried, `NoDefault`,
  `MissingSymbol`, `Init`, `UnsupportedVersion`, `AlreadyLoaded`. One `Sane` per process, and another once
  it has ended; a failed load leaves nothing behind; the library is never unloaded. Nothing links libsane.
- **The bindings,** `sane_dl::ffi`: SANE 1.4.0's `sane.h` as Rust declarations, types and constants only,
  its public-domain notice kept and what was changed said. Every C enum is an integer type with constants,
  the values the header reserves among them, never a Rust enum. A test checks them against the installed
  header: every constant's value, every struct's size and field offsets, every function's type.
- **Statuses and frame types as SANE gives them.** `Status` and `FrameFormat` keep any integer, with a
  constant and a name for each SANE defines; `Version` from `sane_init`'s code. `Error::Status` carries the
  status and the function that answered it (`Call`).
- **One call at a time.** Every call but `sane_cancel` takes the instance's one lock, so no two SANE calls
  overlap. `Sane` is `Send` and `Sync`; `Handle` is `Send`, not `Sync`.
- **Devices under a deadline.** `Sane::devices(local_only, deadline)` lists on a thread of its own and
  copies the list there; past the deadline, `DeadlineExceeded`, and until the listing returns, calls are
  `ListingPending` and a handle dropped is closed by the listing's thread. `Device` keeps its name as SANE's
  bytes (`DeviceName`), for `sane_open`, and its vendor, model and type as text.
- **A known device, fast.** `Sane::open` by name, no listing needed. `ConfigDir` writes a `dll.conf`
  naming the backends given, checked, and an empty `dll.d`, and gives the value for `SANE_CONFIG_DIR`,
  alone (`Search::Isolated`) or before the system's directories (`Search::BeforeSystem`); `backend_of` gives
  a device name's backend. `ConfigError`.
- **Options, typed.** Each descriptor copied as the device opens (`OptionDescriptor`: `ValueType`, `Unit`,
  `Capabilities`, `Constraint`), the constraint read only as the member its type allows; one SANE doesn't
  allow makes its option unusable, said. `get`, `set` (`Value`: `Bool`, `Int`, `Fixed`, `String`,
  `Button`) and `set_auto`, each refusing before the backend sees it a call SANE's rules don't allow, and
  returning `SetInfo`; descriptors read again on `RELOAD_OPTIONS`, before any call on an option SANE now
  describes differently, and after `sane_start` answers `INVAL` (`Error::StartInvalid`). `Fixed`, SANE's
  16.16 word, with `to_f64` and a checked `from_f64`. `BOOL` words other than 0 or 1 are `BadValue`.
- **Scanning.** Step by step: `start`, `parameters` (`Parameters`, as SANE gives them, with checked
  accessors), `read` (`ReadOutcome`; a length below 0 or past what was asked is `BadReadLength`),
  `set_io_mode`, `select_fd` (Unix) and `cancel`, with `sane_cancel` after every error status from a start,
  the parameters or a read. By the page: `Handle::batch` and `Batch::next_page`, every frame read to its
  end, `lines = -1` included, assembled into an `Image` (rows top down, padding dropped, three-pass frames
  interleaved, samples as SANE encodes them), with progress told between reads; `Page::NoDocs` from either
  call that can say so, with the pages delivered; `sane_cancel` however a batch ends.
- **Strict frames.** Parameters no frame can have are `BadParameters`, carrying them; frames that end
  short or run long, and a partial last row, are `ShortFrame`, `LongFrame` and `PartialRow`; a stream past
  its cap is `FrameTooLarge`, its reading stopped.
- **Cancel from any thread.** `CancelToken` calls `sane_cancel` at once, under the handle's own lock and
  only while it's open, so a late cancel never reaches a closed handle; and flags the cancel, so the next
  start or read, and a read in flight, return `Error::Cancelled`, even from a backend that ignores it.
- **Caps.** `Limits` on devices, strings, options, list entries, option values, frames, lines and the read
  chunk, each checked before the work it caps; set with `Sane::set_limits`.
- **The package** holds the sources, this changelog and the README, whose licence links are the
  repository's URLs, a test holding them so. The tests stay in the repository: they build a stand-in
  libsane from the workspace and load it, beside SANE's own `test` and `pnm` backends.
