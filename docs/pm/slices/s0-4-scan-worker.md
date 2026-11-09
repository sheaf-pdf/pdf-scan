# S0 slice 4: scan-worker

Status: **running, on branch `s0-4-scan-worker`** (from `master` at `5a4bff2`), from 2026-11-09. Milestone:
`milestones/s0-devices.md`, slice 4.

Paths: `docs/pm/` files from `docs/pm/`, as everywhere here; the code's from the repository's root, written
with a leading `/` (`/crates/scan-worker/`, `/justfile`); `scratch/` from the root, local and never tracked.

## What it builds

`scan-worker` 0.0.0, unpublished (`publish = false`, provisional with the model it carries until slice 11
freezes both): scanner drivers run in a process of their own ([M5 proposal
4](https://github.com/sheaf-pdf/sheaf/blob/3232a35/plan/m05-scan/proposals/p04-scanner-drivers-run-worker-process.md)),
as the milestone's slice 4 has it and the seam note's "What slice 4 needs of it" (`crates/scan-device-seam.md`)
fixes it:

- **The protocol:** JSON lines over the worker's standard input and output, a `hello` with a version first (a
  mismatch an error, not a negotiation), an `open` carrying the stored `DeviceInfo` and a deadline and answered
  with the device found and `renamed_from`, `caps`, `scan`, an `applied` event, progress lines from the
  worker's `Progress`, `cancel` cancelling the batch's tracker, a `settings_window` carrying the owner, each
  page's format and size on a line before its bytes; every line and page capped before it's read.
- **The worker's side:** `serve(backends)`, the whole `main` of a worker binary over any `scan-device`
  backends, its batch's bounded channel drained as the pipe is written.
- **The client:** a worker started for each open device and for each listing, chosen by the device's
  `Driver`; the model's calls mirrored (`list`, `open`, `caps`, `scan` with a `SyncSender<Event>` and a
  `&dyn Progress`, `settings_window`) with an error of its own wrapping `ScanError` beside its failures; every
  page decoded through `RawImage::new` after its size is capped; a SANE worker that opens a stored id started
  with that id's one-backend `ConfigDir` (`Search::BeforeSystem`, naming none for an id with no backend), in a
  directory it trusts.
- **The watchdog:** a worker silent past its time killed with the pages so far kept; the kill after a cancel
  counted from the worker's last line, so the sheet in flight can finish; a crash's pages kept and its exit said.
- **`scan-device`'s changes** (the control file: slice 4's changes to the model, said): a `serde` feature on
  the model's plain data, and the fake's `Fault::AwaitCancel`, so a cancel lands mid-sheet in a worker process
  deterministically.

Checked as the milestone says: the `fake` backend through a real worker process (a hang killed with the pages
kept, a crash with the pages kept and the exit said, a cancel mid-page with the sheet finished and none fed
after, a version mismatch, a page cut short), the raw layout across the pipe byte for byte, and the throughput
of 300 dpi pages in color and black and white at 2.3 a second ([M5 slice
6](https://github.com/sheaf-pdf/sheaf/blob/3232a35/plan/m05-scan/slices/s06-scanners-worker-sane.md)'s gate,
the pipe's share), measured on every run.

**Not here:** the `sane` backend (slice 5), which the SANE spawn is built for and tested without; the `twain`
backend, the 32-bit worker and the merge of TWAIN's two lists (slice 9); a Windows Job object and a tree kill
(FEAT-14, slice 9); a spare worker kept warm (FEAT-15); the freeze and the protocol's version bump (slice 11).
Nothing is cut from sheaf (its worker is plan material only, `sheaf/from-sheaf.md`), so no harness; nothing
touches the PDF family.

**Later slices build on it:** slice 5 runs the `sane` backend in a worker this client starts (the one-backend
directory already passed); slice 9 adds `Driver::Twain { bits }` workers, the 32-bit one built for
`i686-pc-windows-msvc`, and the Windows hygiene; slice 11 bumps the protocol's version once and settles Q-8;
sheaf's intake takes a `Client` and its own worker binary calling `serve`.

## Taken as it starts

- **No ask bears on it:** `asks.md` holds ask 5 alone (where `pdf-images` lives), S2's. M5 ask 5 (drivers
  always in a worker) is sheaf's kickoff answer (`decisions.md`).
- **The seam note's leanings, taken:** the model's types encoded by a `serde` feature of `scan-device` (not
  wire types), and the kill after a cancel counted from the worker's last line ("What slice 4 needs of it").
  Its decisions behind them (S0 slice 2's 27 to 41, slice 3's 42 to 50) stay open to review where they are.
- **The base: `master` at `5a4bff2`,** the run's. S0 slices 1 (#2), 2 (#3), 3 (#6), 7 (#1) and 8 (#4, closed
  by #5) are merged there, each squashed. Slice 3's plan is still at `slices/s0-3-scan-device.md`; moving it to
  `archive/` is this run's fix stage's (the control file), not a step's. The branch is made by the plan review
  if it's missing.
- **`master` moved as this plan was drafted: S0 slice 1a merged as #7** (`42e4227`, 2026-11-09, `sane-dl`'s
  public API made safe), its branch and worktree gone. The steps are written for `5a4bff2`; bringing `master` in
  is the main session's, as it was for slice 3 (#5). What 1a changed that this slice meets: `sane-dl` (its
  `Sane::load_from` removed, `ffi`'s function-pointer types private, a process-wide slot on Linux), none of which
  this slice uses, while the four items it does use (`backend_of`, `ConfigDir`, `Search`,
  `CONFIG_DIR_VARIABLE`) are unchanged (`git diff 5a4bff2 42e4227 -- crates/sane-dl/src/config.rs` is empty, and
  `lib.rs` still exports them); the lock not at all (`git diff 5a4bff2 42e4227 -- Cargo.lock` is empty); the
  justfile (a `public-api` recipe in `ci`, a comment in the gates) and CI (a `public-api` job), each in hunks
  apart from this slice's; and `status.md`, `decisions.md`, the milestone, `plan.md`, `crates/README.md`, the
  seam note's section 6 and the backlog, a line or two each. `scan-worker` stays off the `public-api` gate's
  list until it's made ready to publish (slice 11), as `scan-device` is. Nothing here changes
  `/crates/sane-dl/` or `crates/sane-dl.md` (decision 37).
- **Backlog IDs,** read on `origin/master` at `42e4227` (2026-11-09), which holds 1a's BUG-8, FEAT-13, Q-7,
  TEST-14, TEST-15 and UP-14 (each file's head names the next): free there, BUG-9, DOC-2, FEAT-14, Q-8, UP-15
  and TEST-16, as the control file has them. Read again as the last step starts.
- **sheaf, checked for drift:** still at `3232a35` (2026-10-28), so the cited plan files are as copied.
- **The survey's verdict re-checked:** still no published broker; crates.io searched 2026-11-09 for
  "scanner worker", "scan broker", "twain sidecar" and "sane worker" found nothing of the kind. Build.

## The design

### The crate

- **`/crates/scan-worker/Cargo.toml`:** version `0.0.0`, edition 2024, `rust-version` from the MSRV probe (expected
  1.89, the family's floor: the highest `rust-version` among the dependencies it will take is `libloading`'s 1.88,
  through `sane-dl`), `license = "MIT OR Apache-2.0"`, a one-line `description`, `repository`, `publish = false`.
  Dependencies: `scan-device` (path, `features = ["serde"]`), `sane-dl` (path, from step 3), `serde` and
  `serde_json` (`[workspace.dependencies]`, decision 37), `tempfile = "3"` (from step 3), `thiserror` (workspace).
  Feature `fake = ["scan-device/fake"]`, which builds the test worker (below) and nothing in the library.
  Dev-dependencies: the crate itself with `fake` (so its tests get the binary, probe B1) and `scan-device` with
  `fake` (its scripts and `pattern` in the tests). `[lints] workspace = true`.
- **`/crates/scan-worker/src/`:** `#![forbid(unsafe_code)]` (nothing here needs it: spawning, pipes, killing
  and process groups are `std`'s safe API, decision 22) and `#![warn(missing_docs)]`. Modules by concept,
  private, re-exported at the root: `wire` (the messages, reading a capped line, writing one), `serve`,
  `client` (with its spawn, its reader and its watchdog), `error`. `pub use scan_device;` so an app names the
  version the client's API does, as `scan-device` re-exports `in_progress`.
- **The crate docs** say, in our own words: why drivers run in a worker; the two halves (`serve` in the
  worker's `main`, `Client` in the app); the protocol whole (framing, every message and its fields, the
  version, the caps), so another implementation could speak it; the watchdog and cancel; what's kept when a
  worker dies; the private directory and SANE's configuration; that the model and the protocol are
  provisional. An example: a worker `main` (`no_run`) and a client over it. They never name the downstream
  app, its milestones or `scratch/`. They name `scan_device::fake`'s items in code only, never as intra-doc
  links: `just doc`'s first line builds with default features, where `scan-device`'s `fake` is off (a dev-
  dependency's feature isn't unified into a doc build), so a link to it fails `-D warnings`, as `scan-device`'s
  own docs found.
- **`/crates/scan-worker/README.md`:** short, as `scan-device`'s: what it is, provisional and unpublished,
  its feature, the licences at the root.

### The protocol

One JSON object a line (UTF-8, `\n`), externally tagged, the message names snake case; the model's types inside
as `scan-device`'s `serde` feature writes them (Rust's names). A `page` line is followed by exactly the bytes it
states. Pinned by unit tests that write literal lines.

| Client to worker | Answered with |
|---|---|
| `{"list":{"deadline":D}}` | `devices` (a list of `DeviceInfo`) or `error` |
| `{"open":{"device":DeviceInfo,"deadline":D}}` | `opened` (`info`, `renamed_from`) or `error` |
| `"caps"` | `caps` or `error` |
| `{"scan":{"settings":Settings,"first_sheet":N}}` | the batch's events, then `done` or `error` |
| `{"settings_window":{"owner":WindowOwner or null}}` | `window` (`WindowClosed`) or `error` |
| `"cancel"` | nothing: the batch's end says it |
| `"quit"` | the worker exits |

Worker to client: `{"hello":{"protocol":1,"version":"<crate version>"}}` first, unasked; the answers above;
during a batch `applied` (`Applied`), `sheet` (a number), `page` (below), `status` (`Status`), `progress`
(`done` and `total` bytes of the page in flight, the total `null` while unknown) and `message` (a level,
`info`, `warn` or `error`, and a text: the latest `Progress::message`); and `refused` (a reason) for a request
the worker didn't take. `D` is a `Duration` as serde writes it.

**A page:** `{"page":{"pixels":{"raw":{"format":F,"width":W,"height":H}} or {"encoded":{"kind":K}},"dpi":[x,y],
"sheet":N,"side":S,"side_known":K,"bytes":B}}`, then `B` bytes: a raw page's data in the model's layout as
`RawImage` holds it, or an encoded page's bytes as the driver gave them. The client caps `B` and checks it
against `RawImage::data_len(F, W, H)` before it allocates, reads exactly `B`, and makes the page through
`RawImage::new` (or `Encoded`) and `RawPage::new`.

**The version:** `PROTOCOL`, public, `1`; the client compares the `hello`'s and refuses any other, killing the
worker. Bumped once, by slice 11 (decision 7).

**What the worker takes, and when:** `list` and `open` only with no device open (one device a worker, and no
listing beside an open device: the seam note's section 1); `caps`, `scan` and `settings_window` only with one
open; `cancel` at any time (nothing to cancel, nothing done); `quit` at any time (a running batch cancelled
first). While a call runs, any other request is `refused` at once. A line it can't read, or past its 1 MiB cap,
is `refused` too, and it goes on.

### The worker: `serve`

`pub fn serve(backends: Vec<Box<dyn Backend>>) -> ExitCode`, a worker binary's whole `main`: its standard input
and output are the client's, and nothing else may write to standard output (the docs say so: a stray line is the
client's protocol error).

- **Threads** (decision 8): the main thread makes every backend and device call, in order (a driver's thread is
  the one that opened it: SANE's handle is used from one thread at a time, and the TWAIN backend's own STA
  thread is behind its calls); a reader thread takes requests, cancelling at once and handing the rest to the
  main thread; a drain thread for each batch takes its events and writes them. One writer, locked per message
  (a page's line and its bytes under one lock), flushed after each.
- **A batch:** the main thread calls `device.scan(&settings, first_sheet, &tx, &tracker)` with `tx` a
  rendezvous `sync_channel(0)` (decision 10) whose receiver is the drain's, so at most one page is being written
  and one held at the backend's `send`, and a slow pipe holds the backend back. The batch's `Tracker` is made
  by the reader as it reads `scan` (decision 9), so a `cancel` read after it always finds it. The drain waits on
  the channel at most 100 ms at a time and, at each wake (an event taken and written, 100 ms without one, and
  once more as the channel closes), writes a `progress` line from the tracker's snapshot if its `done` or
  `total` changed (bytes only, the model's unit: a step counted in another unit isn't forwarded; decision 11),
  and a `message` line with the snapshot's message if its `message_count` moved. A snapshot holds only the
  latest message (`in-progress` 0.1.0's `Snapshot::message`; "a jump of more than one means some were replaced
  before the poll"), so messages replaced between two wakes are lost, which the docs say. So a hung driver
  goes silent, and the caller's progress ends where the worker's did. When `scan` returns, the main thread
  drops `tx`, joins the drain (every event written), then writes `done` or `error`. Every page of the sheets
  before the one in flight is whole in the pipe before that sheet's next side is read (its `Event::Sheet` waits
  on the drain); the page being written when a worker dies is cut short (the docs say so).
- **The end:** `quit` ends it after the device and backends are dropped (the driver closed), with `0`. End of
  input (the client gone) is a cancel and a `quit`; if the main thread's call hasn't returned 10 s later (a
  driver's call that never does), the reader ends the process with `std::process::abort()`, so a worker never
  outlives its client by more than that (decision 12). Not `std::process::exit`: it runs `atexit` handlers and
  shared objects' finalizers (std's docs: "exit handlers registered with `atexit` as well as other
  platform-specific exit handlers (e.g. `fini` sections of ELF shared objects)"), and on Windows `ExitProcess`
  detaches every DLL, all while the hung driver's thread may hold a lock one of them takes, which would hang
  the very exit meant to bound it; an abort runs none (a crash report or a core where the system keeps them,
  said in the docs). That abort is `serve`'s alone: its core runs over any reader and writer, which the unit
  tests drive, and returns instead. Output that fails (the client gone mid-write) ends the batch at its next
  `send` and the worker with `1`. A backend's panic isn't caught: the worker ends as a panicking Rust program
  does, its message on standard error (decision 13).
- **Several backends** in one worker (decision 14), all of one driver (the spec's "one driver per worker"; the
  client refuses a device of another, decision 26): `list` lists each in order and joins them, the first error
  ending the listing; `open` goes to the first backend whose answer isn't `ScanError::BadId`; each given what's
  left of the request's one deadline.
- **Events it doesn't know:** `Event` and `Pixels` are `#[non_exhaustive]` across crates, so the drain matches
  each with a wildcard; a variant added later and not yet carried is written as `refused`, which the client
  takes as a protocol error: loud, not lost (decision 4).

### The client

```rust
pub struct Client { /* the workers by driver, limits, timeouts, the work directory */ }
impl Client {
    pub fn new() -> Client;
    pub fn worker(self, driver: Driver, command: WorkerCommand) -> Client;
    pub fn limits(self, limits: Limits) -> Client;
    pub fn timeouts(self, timeouts: Timeouts) -> Client;
    pub fn work_dir(self, dir: impl Into<PathBuf>) -> Client;
    pub fn list(&self, driver: Driver, deadline: Duration) -> Result<Vec<DeviceInfo>, Error>;
    pub fn open(&self, device: &DeviceInfo, deadline: Duration) -> Result<RemoteDevice, Error>;
}
pub struct WorkerCommand { /* program, arguments */ }    // new(program), arg(..)
#[non_exhaustive] pub struct Limits { pub line: usize, pub page: u64 }               // 1 MiB, 1 GiB
#[non_exhaustive] pub struct Timeouts { pub start: Duration, pub silence: Duration,
                                        pub cancel: Duration, pub grace: Duration }  // 10, 60, 10, 5 s
pub struct RemoteDevice { /* the worker, its pipes' threads, its directory */ }
impl RemoteDevice {
    pub fn info(&self) -> &DeviceInfo;
    pub fn renamed_from(&self) -> Option<&DeviceId>;
    pub fn process_id(&self) -> u32;                          // the worker's, for an app's log
    pub fn caps(&mut self) -> Result<Caps, Error>;
    pub fn scan(&mut self, settings: &Settings, first_sheet: u32, events: &SyncSender<Event>,
                progress: &dyn Progress) -> Result<Done, Error>;
    pub fn settings_window(&mut self, owner: Option<WindowOwner>) -> Result<WindowClosed, Error>;
    pub fn close(self) -> Result<(), Error>;
}
```

Names are the design's; the implementation may rename and says so. `Client` is `Send + Sync` (a listing on
another thread while a device scans: proposal 10's list filled in the background); `RemoteDevice` is `Send`.
The client doesn't implement the model's `Backend` and `Device` (decision 15): their calls return `ScanError`,
which can't carry the client's failures (S0 slice 2's decision 34).

- **Which worker:** the one the app gave for the device's `Driver` (`Driver` isn't `Hash`: a short list,
  compared); none is `Error::NoWorker`. `WorkerCommand`'s program and the work directory must be absolute
  paths, each refused at start otherwise (decision 38): `ConfigDir::write` refuses a relative directory ("it
  isn't absolute"), and a relative one would be read from wherever the app then stands. A listing has a
  short-lived worker of its own, quit once it answers; an open device keeps its worker until `close` or drop.
- **Starting it** (decisions 17, 18, 22): `std::process::Command`, its three pipes, its current directory a
  directory of its own made by `tempfile` under the work directory (the system's temporary directory by default),
  atomically under a random name, its mode 0700 asked for on Unix (`tempfile` leaves a directory at 0777 less the
  umask), and removed once the worker has exited. For a `Driver::Sane` open, that directory also holds the
  one-backend configuration: the stored id's SANE name (the id's text after `sane:`, each character up to U+00FF
  one byte; any other, or no `sane:`, gives no name) read by `sane_dl::backend_of`, and `ConfigDir::write(dir, that
  backend or none, Search::BeforeSystem)`, its `value()` given as `SANE_CONFIG_DIR`. SANE then reads that
  directory, then its current directory (the same one), then the system's. A SANE listing's directory is empty and
  no variable is set, so SANE reads the empty directory then the system's. Whatever the id, the worker's backend is
  the one to refuse it (`BadId`, slice 5): the client only picks the directory. On Unix the worker leads a process
  group of its own (`CommandExt::process_group(0)`), so a terminal's interrupt reaches the app, which decides, and
  not the worker mid-sheet; on Windows it's started with `CREATE_NO_WINDOW`, so a console worker under a windowed
  app shows no console. Both behind `cfg`.
- **Reading it:** a reader thread per worker reads its standard output through a `BufReader`, each line capped
  at `Limits::line` before it's read whole, each page's bytes capped at `Limits::page` and checked against
  `data_len` before anything is allocated, and hands each message on a rendezvous channel to the caller's
  thread; a thread drains standard error, keeping its last 8 KiB for the errors that end a worker (decision 24).
  The first message must be `hello`, within `Timeouts::start`: another message first is
  `ProtocolError::NoHello`, silence past `start` is `Error::Stopped`, and output that ends first is
  `Error::Crashed`.
- **What it checks** (the seam note's list; decision 26): a page's `dpi` finite and above 0; `Caps` with at
  least one source, each with at least one format, a `Resolutions::List` not empty and a `Range`'s `min` not past
  its `max`; a listed or opened device's `driver` the one the worker was started for (a worker table set up
  wrong is said, not followed); a message in its place (an answer to the request made, events only in a batch).
  Any breach is `Error::Protocol`, and the worker is killed: it can't be trusted to be in step.
- **A request it can't send:** each is encoded, decoded again and compared before it's written; one that
  doesn't come back equal (a `NaN` or an infinite paper size, which JSON writes as `null`) is
  `Error::Unsendable`, and nothing is sent (decision 6; probe F1).
- **A batch** (`scan`): `events` and `progress` as `Device::scan` takes them. Each event is sent on as it
  comes, each page whole, so the pages before any failure are the caller's already. `progress` gets
  `set_total(total, Unit::Bytes)` when the total changes and `set(done)` on each `progress` line, and each
  `message`. The caller's cancel is read from `progress.is_cancelled()` every 50 ms and at each message, and
  sent once as `cancel`; a `send` on `events` that fails (the receiver gone) is a cancel too, the rest of the
  batch read and dropped, ending as `Done::Cancelled` (decision 21). A `message` line goes to
  `progress.message` with its level.
- **The end of a worker** (decisions 23, 28): `close` sends `quit`, waits up to `Timeouts::grace` for it to
  exit, then kills it; drop does the same, its errors dropped. A worker that won't be reaped within the grace
  (a process in uninterruptible sleep) is handed, with its directory, to a thread that waits for it, so the
  caller is never held. After a failure that ended the worker, every call answers `Error::Ended`.

### The watchdog and cancel

Silence is counted from the last line the client read, and only while the caller's thread waits on the worker:
time spent blocked handing an event to the caller's full channel doesn't count (decision 20).

| While | The worker is killed after | And the call answers |
|---|---|---|
| Starting | `Timeouts::start` (10 s) without a line | `Error::Stopped` (another line first: `ProtocolError::NoHello`) |
| A batch | `Timeouts::silence` (60 s, a flatbed's warm-up included) without a line | `Error::Stopped` |
| A batch, once cancelled | `Timeouts::cancel` (10 s) without a line, counted from the later of the cancel and the last line, so a sheet in flight that's still sending finishes | `Error::Stopped` |
| `list`, `open` | their deadline and `Timeouts::grace` (5 s) | `Error::Stopped`; and `open`'s `ScanError::TimedOut` kills its worker too (the backend dropped with it: the seam note's section 1) |
| `caps` | `Timeouts::silence` | `Error::Stopped` |
| `settings_window` | never: the user's dialog | |

A `progress` line counts as a line; the worker writes one only when the count changed, so a driver stuck mid-page
goes silent. A worker whose output ends before its answer (a crash, an exit, a signal) is `Error::Crashed` with
how it exited, after the pages it finished.

### Errors

```rust
#[non_exhaustive] pub enum Error {
    Scan(ScanError),            // the device's or driver's, as the worker's backend said it
    NoWorker(Driver),           // no worker given for this driver
    Start { program: PathBuf, source: io::Error },  // not started: its directory, SANE's configuration, the spawn
    Stopped { silence: Duration, stderr: String },  // the watchdog's: killed, the pages so far kept
    Crashed { status: Option<ExitStatus>, page_cut_short: bool, stderr: String },  // ended unasked
    Protocol(ProtocolError),    // it said what the client can't take; killed
    Unsendable,                 // a request JSON can't hold
    Ended,                      // a call after a failure ended the worker
}
#[non_exhaustive] pub enum ProtocolError {
    Version { worker: u32, client: u32 }, NoHello, LineTooLong { limit: usize },
    Unreadable { message: String }, PageTooLarge { bytes: u64, limit: u64 },
    PageLength { expected: u64, stated: u64 }, Layout(LayoutError), Invalid { what: &'static str },
    Unexpected { got: &'static str }, Refused { reason: String },
}
```

`thiserror`, messages in our own words; `serde_json`'s and `sane-dl`'s errors not in the API (a message, or an
`io::Error` with the source kept: decision 19). `NoHello` is a first line that's another message; a worker
that says nothing is `Stopped`, one whose output ends first `Crashed`. A page cut short is `Crashed` with
`page_cut_short`, since only an ended worker cuts one; the seam note's "a page cut short" among the client's
failures is that.

### `scan-device`'s changes

- **A `serde` feature** (decisions 2, 3): `serde = { workspace = true, optional = true }`, `serde =
  ["dep:serde"]`; `#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]` on the
  model's plain data: `DeviceId`, `DeviceInfo`, `Driver`, `Caps`, `SourceCaps`, `Source`, `Resolutions`,
  `SizeMm`, `Settings`, `Paper`, `Applied`, `Clamped`, `Format`, `Encoding`, `EncodedKind`, `Side`,
  `SideKnown`, `Status`, `Done`, `ScanError`, `WindowClosed`, `WindowOwner`. Not `RawImage`, `Encoded`,
  `Pixels`, `RawPage` or `Event`: a page's bytes don't go into JSON, and a transport carries them beside it, as
  this protocol does. serde's defaults (Rust's names, externally tagged); `NonZeroU32` refuses 0 as it decodes.
  The self dev-dependency takes `["fake", "serde"]`, and `serde_json` joins the dev-dependencies. **Tests,
  unit tests inside the crate** (so `#[non_exhaustive]` doesn't apply and each enum's `match` is exhaustive): a
  value of every variant of every enum above, each struct with every field set, through `serde_json` and back,
  equal; a variant added later fails to compile there until it's listed. `SizeMm`'s constants and the `f32`
  edges (`-0.0`, the smallest subnormal, `f32::MAX`) come back bit for bit; a `NaN` is written `null` and
  refused on reading. The crate docs' "Features" and the README gain the feature.
- **`Fault::AwaitCancel`** in the fake (decision 30; `Fault` is `#[non_exhaustive]`, so nothing breaks): at a
  side's start or midway, as any fault, the side waits, sending nothing and holding no lock, until the batch's
  progress is cancelled (checked each millisecond), then goes on: the side is finished, its back too in
  duplex, and the check before the next sheet ends the batch as `Done::Cancelled`. A scanner slow mid-page, so a
  cancel lands while a sheet is in flight, in a test's process or a worker's, by the script and not by timing.
  Fires once. Tests in `tests/fake_faults.rs`: at the front's midway in duplex (the test waits for the
  tracker's snapshot to stand at half the page, then cancels: both sides delivered, `Done::Cancelled`, `fed()`
  that sheet's number), and at a back's start (the front delivered first). The module docs' "Faults" list it.

### The test worker

`/crates/scan-worker/src/bin/scan-worker-fake.rs`, `[[bin]] name = "scan-worker-fake"`, `required-features =
["fake"]`, `test = false`, `doc = false` (decision 29): built for the crate's integration tests by the dev
dependency on itself and found by `env!("CARGO_BIN_EXE_scan-worker-fake")` (probe B1); a plain `cargo build`
leaves it out. Its first argument names a scenario, a second a count where one wants it:

- **Through `serve` and the fake:** a feeder of `n` Letter sheets (duplex caps by default); one with encodings
  offered; jams (sheet 4's front at its start; a back midway), a double feed, an open cover, an empty feeder,
  the device gone mid-batch; warming up; a settings window; a stored device renamed (two USB names); a slow
  listing; the driver not installed; `Hang`, `Crash` and `Exit(42)` at sheet 3's front midway, each scenario
  first writing a known line to standard error (an abort and an exit write none, so the line is what shows
  the client kept the tail); `AwaitCancel` at sheet 2's front midway and at its back's start; the throughput
  feeder.
- **`env`:** a backend of the binary's own, wrapping the fake, whose listing and opened `info` report its
  current directory (`name`), `SANE_CONFIG_DIR` (`maker`) and that directory's `dll.conf` (`model`), as
  `Driver::Sane`: the SANE spawn tested without SANE.
- **Hand-written** (literal lines, which also pin the wire's shape): a wrong version, a first line that isn't
  JSON, a first line that's another message (`done`, not `hello`), nothing at all while it stays alive (the
  silent start, step 4), a line past the cap, a page past the cap, a page whose length disagrees with its
  format and size, a `Bw1` page with its padding bits set, caps with no source, a page at 0 dpi, a page cut
  short (half its bytes, then exit 0).

### The workspace around it

- **`/Cargo.toml`:** `crates/scan-worker` a member, after `crates/scan-device`; `[workspace.dependencies]`
  gains `serde = { version = "1", features = ["derive"] }` and `serde_json = "1"`, each taken by two members.
  `/Cargo.lock` changes at steps 1 (serde; serde_json for tests), 2 (`scan-worker`) and 3 (`tempfile` for it,
  `sane-dl` as its dependency, the bin's feature).
- **`/justfile`:** the gates' devices-layer loop `for crate in sane-dl twain-dsm scan-device scan-worker`
  (step 2); the libsane gate's `jq` also reading every `bin` target's executable, which nothing built until
  `scan-worker-fake` (step 3; decision 32); `scan_worker_msrv` read from the manifest, `scan-worker-msrv`
  (`cargo +{{ scan_worker_msrv }} test --locked -p scan-worker`, which is also the seam note's `-p` run), and
  `msrv` with it (step 2). Only should decision 33's fallback be taken (step 4): a `throughput` recipe in `ci`.
  These edits merge cleanly with `master`'s justfile at `42e4227` (`git merge-file`, the plan review's trial:
  its `public_api_nightly` lines come after the blank line that follows `scan_device_msrv`, this slice's
  variable before it).
- **CI:** a `scan-worker-msrv` job after `scan-device-msrv`, as it is (`dtolnay/rust-toolchain@<MSRV>`, a note to
  keep it in step, and that the crate's Windows `cfg` branch, `creation_flags` (stable since 1.16), isn't
  compiled on that Rust there, as `scan-device-msrv`'s note says of its crash test). The `check` job's `just
  test` runs everything else on the three systems as it is; under decision 33's fallback, its Ubuntu, Windows and
  macOS legs also run `just throughput`.
- **`/README.md`:** a `scan-worker` row after `scan-device`'s, and its "Next" line (slice 5's `sane` backend).

### For later slices

- **Slice 5:** `serve(vec![Box::new(<the sane backend>)])` is a SANE worker; this client starts it with the
  stored id's one-backend `ConfigDir` already, so slice 5 tests its `BadId`s, fallback and TEST-10 through a
  worker as well as in process. Whatever the `sane` backend finds of the protocol, it says.
- **Slice 9:** `Client::worker(Driver::Twain { bits: 32 }, ..)` and `bits: 64`; a 32-bit worker binary (the
  test one built for `i686-pc-windows-msvc` in CI); the lists' merge; the Job object (FEAT-14); `WindowOwner`
  passed through already; TEST-12 (`RawImage`'s 32-bit layout tests) in its 32-bit job, where a 32-bit process
  makes pages.
- **Slice 11:** the protocol's version bumped once; Q-8 (the serde names; a field added to a `#[non_exhaustive]`
  struct, which serde's default refuses to decode from data stored before it, a stored `DeviceInfo` among it,
  unless the field takes `#[serde(default)]`; and the three things carried by name); Q-5's question asked of
  `scan-worker`'s dev-dependency on itself too.

### Caps on hostile input

The worker is untrusted by the client: a driver's code runs in it.

- **A line:** at most `Limits::line` (1 MiB) read before it's refused (`take`, never a whole unbounded line);
  JSON nesting bounded by `serde_json`'s recursion limit.
- **A page:** `bytes` at most `Limits::page` (1 GiB) and, raw, equal to `RawImage::data_len` (which refuses
  what one buffer can't hold), all before anything is allocated; exactly that many read; `RawImage::new` checks
  the padding bits; the model's rules checked (above).
- **Silence and death:** the watchdog's limits; a crash's half page never a page.
- **Requests** (the worker's input, from its own client): capped at 1 MiB, refused when unreadable, never ending
  it.
- **The worker's directory:** private, made atomically, holding only what the client wrote; the program path
  absolute.

### Dependencies, vetted

Read 2026-11-09 from crates.io's index and API and the sources in `scratch/cargo-home` (the plan's probe,
`scratch/work/s0-4/plan-probe/deps/`):

- **`serde` 1.0.229** (with `serde_derive`, `serde_core`): MIT OR Apache-2.0, 1.67 billion downloads; MSRV 1.56,
  `serde_derive`'s 1.71. Already in the cargo home, unused till now.
- **`serde_json` 1.0.154**: MIT OR Apache-2.0, 1.58 billion downloads, MSRV 1.71; pulls `itoa`, `memchr`
  (Unlicense OR MIT), `serde_core` and `zmij` 1.0.23 (MIT, the same author's float formatter, MSRV 1.71). `unsafe`
  appears 16 times in its sources, and in its formatters. Probe F1: every finite `f32` back bit for bit.
- **`tempfile` 3.27.0**: MIT OR Apache-2.0, 976 million downloads, MSRV 1.63; pulls `fastrand`, `getrandom`
  0.4.3 (MSRV 1.85), `once_cell`, and `rustix` on Unix or `windows-sys` on Windows; a directory at 0777 less the
  umask unless asked otherwise (the client asks 0700). `sane-dl`'s tests take it already.
- **`sane-dl`** (this workspace): for `backend_of`, `ConfigDir`, `Search` and `CONFIG_DIR_VARIABLE` only;
  the client never loads libsane. Pulls `libloading` 0.9 (MSRV 1.88).
- **Weighed and not taken:** **`process-wrap` 10.0.1** (MIT OR Apache-2.0, 25 million downloads, used by
  `rust-analyzer`'s flycheck and `watchexec`; MSRV 1.87; `nix` 0.31 on Unix, `windows` 0.62.2 on Windows):
  process groups killed whole and Windows jobs, but its `std` `JobObject` makes its job without
  `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` (`make_job_object(handle, false)`, `src/std/job_object.rs`; only its
  tokio side's `KillOnDrop` sets it), so it doesn't give what the spec asks on Windows, and a group kill
  serves a worker that starts no process of its own nothing yet: slice 9 weighs it again (FEAT-14).
  **`win32job` 2.0.3** (kill-on-close; on `windows` 0.61, beside the 0.62 the `wia` backend will take):
  slice 9's too. **`shared_child`** (waits and kills from two threads): not needed, one thread owns the child.
  **`wait-timeout`**: installs a process-wide `SIGCHLD` handler on Unix; a bounded `try_wait` instead.
  **`command-group`**: superseded by `process-wrap`. A binary format (`bincode`, `postcard`): the spec's JSON
  lines, and a page's bytes travel raw beside them anyway.

## Steps

**At every step**, the checking agent runs the core, once: `just ci` with `SANE_DL_REQUIRE_LIBSANE=1` and
`SANE_DL_REQUIRE_HEADER=1` (fmt, clippy `-D warnings`, test, doc `-D warnings`, the gates, `sane-dl-package`,
which asks the registry and is recorded as not run, never passed, if the network is down, and the MSRV recipes
on 1.89, installed here); `cargo test --locked --workspace -- --list` recorded under `scratch/work/s0-4/` and
compared with the step before's (none lost; the base's recorded by step 1's check on a worktree at `5a4bff2`);
the commit's message and diff scanned for local paths, the username, the maintainer's name, the downstream app's
name and `scratch/` outside `docs/pm/`, and pdf-lint's unpublished plan; the branch holding `5a4bff2` and,
after it, this slice's commits alone (and any merge of `master` the main session makes, said in "So far");
`/crates/sane-dl/` and `crates/sane-dl.md` untouched. The control file is read before each long run. Below,
each "Done when" says only what the step adds; a check whose subject the step doesn't touch is left out.

1. **`scan-device`'s changes: the `serde` feature and `Fault::AwaitCancel`.** As "`scan-device`'s changes" has
   them: `[workspace.dependencies]` gains `serde` and `serde_json`; `scan-device`'s manifest, the derives, the
   unit tests of every variant, the fault and its two tests, the crate docs, the module docs and the README; the
   lock.
   **Done when:** the core; the MSRV probe over `scan-device`'s own graph again (`serde` and `serde_json` join
   it), recorded under `scratch/work/s0-4/msrv-probe/scan-device/` as S1 slice 1's decision 2 measured (each
   package's `rust-version` from `cargo metadata` over `cargo tree -p scan-device -e normal,build,dev --target
   all`), expected 1.89; `cargo tree -p scan-device -e normal,build --target all --all-features` read, `serde`,
   `serde_derive` and `serde_core` the only packages new, and the gate's count for `scan-device` with it; the
   exhaustiveness shown once on a worktree under `scratch/work/s0-4/`: a variant added to `Status` and one to
   `ScanError`, each failing the unit tests' build, the worktree removed and the branch unchanged; the two
   `AwaitCancel` tests run 30 times in a loop, none failing. Left out: other targets (no `cfg` code); the gates'
   make-fails (no gate changed).
2. **`scan-worker`, its protocol and its worker side.** The crate (manifest without `sane-dl`, `tempfile`, the
   `fake` feature and the dev-dependency on itself yet, which step 3 adds with the binary; its one
   dev-dependency now `scan-device` with `["fake"]`, for the unit tests' fake; crate docs with the protocol
   whole, README), `wire`, `serve`; the workspace: member, lock, justfile (the loop, the MSRV variable, recipe
   and `msrv`), CI's job, the root README's row. Unit tests, the worker's core driven over `std::io::pipe` pairs
   with the fake: `hello` first (a literal line); `list` (and its deadline's `TimedOut`), `open` as stored and
   renamed, `caps`, `settings_window`; batches simplex and duplex, events in order, every page's line before its
   bytes and the bytes `pattern`'s, `progress` lines within each page's size and the last one the last page's
   whole size, `done` last; a `message` line from a test's own backend whose progress reports one; a jam's
   `error` after the pages before it; `cancel` through `AwaitCancel` (the sheet finished, `done` `Cancelled`),
   and `scan` and `cancel` written together (`applied`, then `done` `Cancelled`, no sheet: decision 9); requests
   refused out of state, while a call runs, unreadable, and past the cap, the worker going on; the end of input
   cancelling a batch and returning; `quit`; two backends, listed joined and opened by the first that doesn't
   answer `BadId`.
   **Done when:** the core; the MSRV probe over `scan-worker`'s graph (normal, build and dev, every target),
   under `scratch/work/s0-4/msrv-probe/scan-worker/`, the highest never below 1.89, and `just scan-worker-msrv`
   passing on that toolchain (in the core's run); `cargo tree -p scan-worker -e normal,build --target all
   --all-features` read: `scan-device`, `in-progress`, `serde`, `serde_json` and theirs, `thiserror`, no codec,
   PDF, OCR or GUI crate, and the gate's output naming `scan-worker`; **the devices loop's make-fails,** once,
   here where the loop changes, on a worktree under `scratch/work/s0-4/` with its lock made again first (`cargo
   update --offline -w`; `png` 0.18.1 is in the cargo home), as S0 slice 3's were: `png` as a normal dependency
   of `scan-worker`, and `png` behind a feature of its own (read only through `--all-features`), each failing
   the gate with `scan-worker` and `png` named, the worktree removed and the branch unchanged; the crate docs'
   protocol read against `wire` and the literal lines, a row per message in the check's record; three mutations
   of `serve` on a worktree, each failing a test: `done` written before the drain is joined, a `cancel` read
   and not passed to the tracker, a refused request ending the worker. Left out: other targets (no `cfg` code
   yet).
3. **The client, its normal paths and its refusals.** `Client`, `WorkerCommand`, `Limits`, `Timeouts`,
   `RemoteDevice`, `Error`, `ProtocolError`; starting a worker (its directory, the SANE configuration, the
   process group, `CREATE_NO_WINDOW`), the reader and standard error's drain, the checks, `close` and drop; the
   whole loop of a call, the watchdog's limits and cancel included (step 4 tests their failures); `sane-dl` and
   `tempfile` in the manifest; the `fake` feature and `scan-worker-fake`; the libsane gate reading `bin`
   targets. Integration tests (`tests/`, through the binary): listings and their driver checked; opening as
   stored and renamed (`renamed_from`), `NotInstalled`, a listing past its deadline (`TimedOut`, the worker
   killed); caps and the settings window; batches in each format, raw and encoded, every page equal to the fake's
   (`pattern` byte for byte, `dpi`, sheet, side, `side_known`), "Scan More" from `first_sheet`; each error ending
   a batch with the pages before it (jam then Continue in the same worker, double feed, open cover, empty feeder,
   gone and every call after it `Gone`); warming up; progress reaching the caller's tracker (once the batch
   returns, its total and count the last page's bytes); the SANE spawn (`env`: for `sane:test:0` the
   directory's `dll.conf` naming `test`, for `sane:Desk scanner` and an id not Latin-1 naming none,
   `SANE_CONFIG_DIR` the directory and `:`, the worker's current directory that directory, under the work
   directory the test gave, 0700 on Unix, gone after `close`; a SANE listing's directory empty), every path
   compared after `fs::canonicalize` on both sides, since the worker's `current_dir()` is the resolved path and
   `SANE_CONFIG_DIR` the client's (on macOS the temporary directory is under `/var`, a link to `/private/var`;
   a Windows runner's `%TEMP%` may hold a short name); the hand-written workers each refused as its
   `ProtocolError`; a `NaN` paper size `Unsendable` with nothing sent and the next scan fine; no worker for a
   driver; a relative program, a missing one and a relative work directory refused at start.
   **Done when:** the core; the MSRV probe again (`sane-dl` and `tempfile` join the graph), and `cargo tree` read
   again, the new packages listed; the libsane gate's output counting `scan-worker-fake` among the binaries read
   (its list in the check's record); **the libsane gate's make-fail,** once, here where its `bin` branch comes,
   on a worktree under `scratch/work/s0-4/`: a build script in `scan-worker` linking libsane into its binaries
   alone (`cargo:rustc-link-arg-bins=-Wl,--no-as-needed` then `cargo:rustc-link-arg-bins=-lsane`; a plain
   `cargo:rustc-link-lib=sane` gives no `NEEDED` entry at all, rustc linking `--as-needed`, and would show
   nothing: the plan review's probe, `scratch/work/s0-4/review/link-probe/`), failing the gate with
   `scan-worker-fake` named, and the same build script passing the gate as it stood before this step (its test
   executables don't link it, since the binary is `test = false`), so the `bin` branch is what catches it; the
   worktree removed and the branch unchanged; `cargo clippy --locked -p scan-worker --all-targets
   --all-features --target x86_64-pc-windows-gnu -- -D warnings` and the same for `x86_64-apple-darwin` (the
   `cfg` code); three mutations on a worktree, each failing a test: a page's size not checked against
   `Limits::page` before it's allocated, the request's round trip not checked, the listed device's driver not
   compared. Left out: the timing loops (step 4).
4. **When a worker fails: the watchdog, cancel, crashes, and the throughput gate.** Tests through the binary, each
   bounded: a hang at sheet 3 `Stopped` after `silence` (set short in the test) with sheets 1 and 2's pages kept; a
   silent start `Stopped` after `start`; a consumer slower than `silence` not stopped; a crash and an exit at sheet
   3 `Crashed`, sheets 1 and 2's pages kept, the status a signal (`SIGABRT`) on Unix and `0xC0000409` on Windows
   for the abort, 42 for the exit (behind `cfg`), the scenario's known line in the error's standard error; a
   page cut short `Crashed` with `page_cut_short`; a cancel mid-page through `AwaitCancel` (cancelled once the
   caller's tracker stands at half the page: that sheet finished front and back, `Done::Cancelled`, no sheet
   after, and a second batch in the same worker giving the rest, so none was fed and lost); a cancel after a
   duplex front (its back delivered); the receiver dropped mid-batch (`Done::Cancelled`, the worker usable
   after); a worker whose input ends, spoken to directly: idle, exiting `0`; in a hang, ended by its abort
   within the 10 s grace and a margin; `close` and drop reaping. `tests/throughput.rs`, a test binary of its own
   so nothing runs beside it: 24 duplex sheets, 300 dpi Letter, `Rgb8` then `Bw1`, the rate over the pages
   after the first 8 in pages and MB a second written on standard error outside the harness's capture on every
   run, and asserted at 2.3 pages a second (decision 33).
   **Done when:** the core; the same two clippy targets again (the crash tests' expected statuses behind
   `cfg`); the timing tests (hang, silent start, slow consumer, cancel, the end of input) run 30 times in a loop
   once, under the memory scope, none failing; the throughput test's figures recorded three times in the debug
   build `just test` runs and once in release, and its margin over 2.3 stated (decision 33's rule applied, and
   its fallback's recipe and CI lines added and run here if it's taken); three mutations on a worktree, each
   failing a test: the watchdog counting time spent handing an event to the caller, the cancel never sent, the
   reader dropping a whole page read just before the end of output. Left out: the MSRV probe and `cargo tree`
   (no package added); the gates' make-fails (no gate changed).
5. **The plan's files; the slice's end.** `crates/scan-worker.md` as built (the protocol, `serve`, the client,
   the watchdog, the dependencies, the MSRV; "Open": the `saned` idea kept, the Job object FEAT-14, the spare
   worker FEAT-15); `crates/scan-device.md` (the `serde` feature and `AwaitCancel`, said as slice 4's changes);
   one sentence at the head of the seam note's "What slice 4 needs of it" saying slice 4 built it and where,
   off every TWAIN line; `crates/README.md`'s line; `plan.md`'s graph (`scan-worker` on `scan-device` and
   `sane-dl`) and its family table's row; `decisions.md`, one bullet: `scan-worker`'s MSRV, after
   `scan-device`'s; the milestone's slice 4 entry (done on the branch, the counts of this plan's decisions and
   security items); `status.md` and `slices/README.md`, a line each (slice 3's lines are the fix stage's, the
   control file); the backlog: FEAT-14, FEAT-15, TEST-16 (this slice's code on CI's Windows and macOS, closed by
   CI's run of its pull request), Q-8 (as "For later slices" has it), TEST-12's line (wanted by slice 4, left to
   slice 9's 32-bit job, decision 34) and the README's "by slice 4 or 9" with it, and whatever the steps found,
   with IDs read again on `origin/master`, the README's table counted again or its counts left out.
   **Done when:** the core, but `just ci` only if the step's diff reaches outside `docs/pm/` (no build, test or
   gate reads it); the publish dry run not run, said (`publish = false` until slice 11), while
   `sane-dl-package` runs in the core; no new backlog ID found on `origin/master` as it then is; every commit
   message of the branch read for the
   maintainer's name, local paths, `scratch/`, the downstream app and pdf-lint's plan; `git grep -i` for the
   maintainer's name finding only `LICENSE-MIT`; every backlog line in the README's format; every path the
   edited files cite found. **What only CI shows,** said: both crates' new tests on Windows and macOS (spawning,
   pipes, the private directory there and its paths compared resolved, process groups on macOS,
   `CREATE_NO_WINDOW`, the crash statuses, the timing tests and the throughput gate on slower runners and
   through Windows' pipes), `scan-worker-msrv` as CI's job, and the gates on `ubuntu-latest` reading the binary
   (TEST-16). Left out: the gates' make-fails (steps 2 and 3 ran them where each gate changed; nothing since
   touched them).

## Baselines

- **The base, `master` at `5a4bff2`:** CI's run on it (42644985306) green on all ten jobs (the control file,
  2026-11-09); here, step 1's check records its test list on a worktree before comparing.
- **The fake's page rate** (S0 slice 3's step 2 check, `scratch/work/s0-3/flow/checks.md`): 300 dpi Letter in a
  release build, about 48 pages a second at its slowest (duplex `Rgb8`), twenty times the gate. Step 4 measures
  the pipe's share on top of it.
- **Probe B1** (this plan's, `scratch/work/s0-4/plan-probe/binexe/`): a binary behind a feature, the feature
  turned on for the package's tests by a dev-dependency on itself, found by `CARGO_BIN_EXE_<name>` in an
  integration test, on stable and 1.89, with `-p` and in the workspace; a plain `cargo build` builds no binary;
  clippy clean both ways.
- **Probe F1** (this plan's, `scratch/work/s0-4/plan-probe/deps/f32-sweep.txt`): every finite `f32`,
  4,278,190,080 of them, through `serde_json` 1.0.154 and back, bit for bit equal (23 s on 16 threads, release);
  `NaN` and both infinities written `null` and refused on reading. So the model's `f32`s (paper sizes, a page's
  `dpi`) cross exactly, and the non-finite can't cross at all (decision 6).
- **The MSRV probes** (steps 1 to 3): each crate's own graph, normal, build and dev, every target.
- **The spec, item by item:** the seam note's "What slice 4 needs of it", the milestone's slice 4 entry and
  `crates/scan-worker.md`, a row each in step 5's check, each met or said where it went.
- **No third party:** no SANE run (slice 5's); the fake is the model's own behaviour.
- **sheaf:** no comparison (nothing cut); its plan files checked for drift ("Taken as it starts").

## Decisions taken alone, for review

1. **Five steps:** `scan-device`'s two changes first, so the worker is built on them; the worker's side over
   pipes in process; the client's whole loop with its normal paths and refusals; its failures, timing and the
   throughput gate; then the bookkeeping. Each gate's make-fails once, at the step that changes it: the devices
   loop's at step 2, the libsane gate's `bin` branch at step 3 (the plan review moved them from step 5).
2. **The model's types cross by a `serde` feature of `scan-device`** (the seam note's leaning), not wire types
   of the worker's own: a variant added to the model is carried without a change here, an app stores a
   `DeviceInfo` with the same feature, and there's one definition. The page types and `Event` aren't serde:
   their bytes don't belong in JSON. serde's defaults (Rust's names, externally tagged), left for slice 11 to
   weigh before the freeze (Q-8).
3. **Every variant round-tripped in `scan-device`'s unit tests,** each enum matched exhaustively there, so a
   variant added fails to compile until it's listed: the seam note's "every variant round-tripped in a test",
   made to notice the next one.
4. **A page's header, `Event`'s and `Pixels`' variants, and a message's `Level` are carried by name** in
   `scan-worker` (a page isn't serde, `Event` and `Pixels` are `#[non_exhaustive]` across crates, and
   `in-progress`'s `Level` has no serde): a variant the drain doesn't know is written `refused`, a protocol
   error at the client, loud rather than dropped; a field added to `RawPage` is added to the header with it
   (Q-8).
5. **JSON lines through `serde_json`,** message names snake case, a page's line then its bytes: the spec's
   shape, readable, and the bytes never in JSON.
6. **A request is checked by its own round trip before it's sent;** one that doesn't come back equal is
   `Unsendable`. JSON can't hold `NaN` or an infinity (probe F1: `null`), so the model's "settings clamped
   whatever their values" stops at the pipe; refusing says so where clamping a value the worker never saw
   couldn't.
7. **The version is the worker's `hello` alone,** compared by the client before it sends anything; `PROTOCOL`
   starts at 1 and slice 11 bumps it once. The worker doesn't check the client's: the client never writes to a
   worker it refused.
8. **The worker's threads:** every backend and device call on the main thread (a driver's thread affinity), a
   reader taking requests, a drain per batch, one writer locked per message and flushed after each.
9. **The batch's tracker is made by the reader as it reads `scan`,** so a `cancel` read after it can't miss it
   while the main thread is still starting the batch.
10. **A rendezvous channel (`sync_channel(0)`) in the worker:** the least memory (a page written, one held),
    which a 32-bit worker will want; a sheet's `Event::Sheet` waits on the drain, so the sheets before the one
    in flight are whole in the pipe before it's read. The page being written when a worker dies is cut short,
    said in the docs rather than paid for with an acknowledgement the model's `SyncSender` can't give.
11. **Progress in bytes only,** the model's unit (`Unit::Count` holds a `&'static str` a decoder can't make, so
    a step counted so isn't forwarded); polled from the batch's `Tracker` at each event, every 100 ms without
    one, and once more as the batch's channel closes, and written only when changed, so a stuck driver goes
    silent and the caller's progress ends where the worker's did. `Progress::message`s forwarded, since they're
    part of the `Progress` the caller gave: the latest at each poll, as the snapshot holds it, so one replaced
    before a poll is lost (said in the docs). Implementing `Progress` in the worker to push every message was
    weighed: its children too would need wrapping, for messages that are display text.
12. **End of input is a cancel and a quit,** and 10 s later, if a call still hasn't returned, the worker's
    reader aborts the process (`std::process::abort`): a worker never outlives its client by more than that,
    without the platform calls (a parent-death signal, a job object) that need `unsafe`. An abort, not an exit:
    `exit` runs `atexit` handlers and libraries' finalizers (on Windows, every DLL's detach) beside the hung
    driver's thread, whose locks one of them may need, so it could hang where it should end (the plan review).
    On Windows an abort may be reported by Windows Error Reporting; slice 9's Job object (FEAT-14) ends a
    worker with its app there, before this ever runs.
13. **A backend's panic isn't caught:** the worker ends as a panicking program does, its message on standard
    error, which the client keeps in `Crashed`. A worker in an unknown state is worth nothing.
14. **Several backends in one worker,** as the spec's `serve(backends)` has it, all of one driver (the spec's
    "one driver per worker", which the client's check holds, decision 26): listings joined in order (the first
    error ending it), an open going to the first that doesn't answer `BadId`, the model's rule that an id and a
    driver that disagree never open; the request's one deadline shared among them, each given what's left.
15. **The client mirrors the model's calls with its own error** and doesn't implement `Backend`/`Device`,
    which can't return its failures (S0 slice 2's decision 34).
16. **A worker per open device and per listing,** chosen by the device's `Driver` from the commands the app
    gives (the binaries are the app's: the spec), each a program and its arguments, so an app may be its own
    worker (`app --scan-worker`).
17. **Every worker starts in a private directory of its own,** made by `tempfile` and removed once it has exited:
    SANE reads its current directory (`.`) with no `SANE_CONFIG_DIR` and after a `BeforeSystem` one, so every SANE
    worker needs one it trusts, and the same rule for every driver is simpler and costs nothing. A SANE open's
    holds the one-backend `ConfigDir`, written `BeforeSystem`, which SANE reads first, then `.` (the same), then
    the system's; a SANE listing's is empty and sets no variable, leaving the app's own `SANE_CONFIG_DIR`, if any,
    to the listing. An open's `BeforeSystem` value replaces an app's own variable; said in the docs.
18. **The client reads a `sane:` id's name for `backend_of` alone** (its characters up to U+00FF as bytes),
    privately: the worker's backend stays the one to refuse an id (`BadId`, slice 5), and the client only
    chooses the directory, naming none where the id gives no backend, as the seam note says.
19. **`sane-dl` and `serde_json` are private dependencies:** their errors become a message or an `io::Error`
    with its source kept, so neither's version is in `scan-worker`'s API. `scan-device` is public, and
    re-exported.
20. **The watchdog counts silence only while the client waits on the worker,** from the last line read: time
    blocked handing an event to the caller's full channel is the caller's, and backpressure isn't a hang. Its
    limits: 10 s to start, 60 s in a batch (the spec's), 10 s after a cancel counted from the later of the
    cancel and the last line (the seam note's suggestion, so the sheet in flight, front and back, can finish
    while it sends), a listing's or an open's deadline and 5 s, `caps` the batch's, the settings window none.
21. **The caller's cancel is polled** from its `Progress` every 50 ms and at each message (`in-progress` calls
    nothing when cancelled), and a failed `send` to the caller is a cancel too, the rest of the batch read and
    dropped and the batch `Done::Cancelled`, the worker kept in step for the next call. Through a worker, that
    batch finishes its sheet in flight rather than stopping at the failed `send`; said in the docs.
22. **Processes through `std` alone:** a kill is the worker's process (`Child::kill`); on Unix it leads a
    process group of its own (`process_group(0)`), so a terminal's interrupt reaches the app and not the worker
    mid-sheet; on Windows `CREATE_NO_WINDOW`. `process-wrap` was vetted and not taken ("Dependencies, vetted"):
    its `std` job lacks kill-on-close and a group kill serves nothing yet. The Job object, kill-on-close and a
    tree kill are slice 9's (FEAT-14), where Windows drivers are tried.
23. **Reaping never holds the caller:** `try_wait` within the grace, then a kill, and a worker still not reaped
    is handed with its directory to a thread that waits for it.
24. **Standard error drained by a thread, its last 8 KiB kept** in `Stopped` and `Crashed` (lossy UTF-8), for
    the app's log; nothing written to the app's own standard error.
25. **Limits: a line 1 MiB, a page 1 GiB, configurable:** 1 GiB holds a 1200 dpi Letter page in any format
    (808 MB in 16-bit color) and a 600 dpi Legal one; the fake draws none past 512 MiB.
26. **The client checks the seam note's rules and the driver:** a listed or opened device's `driver` must be
    the worker's, so a worker table set up wrong (a 64-bit TWAIN worker given for 32-bit devices) is said at once.
27. **The errors:** `Scan`, `NoWorker`, `Start`, `Stopped`, `Crashed` (a page cut short a flag of it, since only
    an ended worker cuts one), `Protocol` (with `ProtocolError`), `Unsendable`, `Ended`.
28. **`close` and drop do the same,** `quit` then the grace then a kill: graceful, so a SANE device is closed
    (`sane_close`), and bounded. Drop drops its errors.
29. **The test worker is a binary of `scan-worker`, `scan-worker-fake`, behind a `fake` feature** (which turns
    on `scan-device/fake`; a binary can't take a dev-dependency), built for the tests by a dev-dependency on the
    crate itself and found by `CARGO_BIN_EXE_scan-worker-fake` (probe B1): cargo builds it before any
    integration test and gives its path, unlike an example's or a crate of its own built by a nested cargo. Its
    scenarios are named by its argument; some write the protocol by hand. Never for a shipped build; the docs
    and README say so.
30. **`Fault::AwaitCancel`, the fake's one addition** (the control file allows slice 4's changes to
    `scan-device`): through a worker, a fast fake runs a few events ahead of what the client has seen, so a
    cancel timed by the test lands a sheet or two later than meant. A side that waits for the cancel puts it
    mid-page by the script: the milestone's "cancel mid-page (the sheet finished, none fed after)" checked
    exactly, in a worker as in process.
31. **`pub use scan_device`,** so an app names the version the client's API takes, as `scan-device` re-exports
    `in_progress`.
32. **The libsane gate reads `bin` targets' executables too:** it read only test executables and shared
    libraries, and `scan-worker-fake` is the first binary of the workspace and the kind a backend's link would
    reach.
33. **The throughput gate is asserted everywhere at 2.3 pages a second** and written on standard error on every
    run (outside the harness's capture, as `sane-dl`'s skips are), in a test binary of its own: Windows, where
    the TWAIN workers will run, is where the pipe's share matters most. If step 4 measures less than four times
    the gate in the debug build here, the debug run only measures, and the assertion moves to a release build of
    that one test, which is what an app ships: a `throughput` recipe (`cargo test --locked --release -p
    scan-worker --test throughput` with `SCAN_WORKER_GATE_THROUGHPUT=1`, the variable the test asserts under) in
    `ci`, and a step running it in CI's `check` job on the three systems; M5 slice 6's own gate is so, measured
    on every run and asserted under a variable. Said in "So far". (The draft set the variable in CI's Ubuntu
    `just test`, the same debug build that would have measured short here: the plan review.)
34. **Left for later, each a backlog item:** a spare worker (FEAT-15: it can't serve a SANE open, whose
    configuration is the device's); the TWAIN lists' merge (slice 9); TEST-12 (slice 9's 32-bit job, where a
    32-bit process makes pages; this client is 64-bit).
35. **Backlog IDs free on `origin/master` at `42e4227`:** FEAT-14, FEAT-15, TEST-16, Q-8; slice 1a's merge took
    BUG-8, FEAT-13, Q-7, TEST-14, TEST-15 and UP-14, as the control file says.
36. **The seam note gets one sentence,** at the head of "What slice 4 needs of it", off every TWAIN line (the
    control file: the Windows machine edits those).
37. **`serde` and `serde_json` join `[workspace.dependencies]`** (two members each); **`tempfile` doesn't
    yet:** joining would change `sane-dl`'s manifest (`tempfile = "3"` in its dev-dependencies), which the
    control file keeps this slice off (slice 1a was changing that crate as the run began). `scan-worker` takes
    `tempfile = "3"` itself; it joins at the next slice that edits `sane-dl`'s manifest.
38. **A worker's program and the work directory must be absolute paths,** refused at start otherwise: a bare
    program would be searched for, and on Windows the search starts in the current directory; a relative work
    directory would be read from wherever the app stands when it opens a device, and `ConfigDir::write`
    refuses one (the plan review added the directory).
39. **Requests during a running call are refused at once** (but `cancel` and `quit`), rather than queued
    behind it: the client never sends them, and a queued answer after a batch's `done` would only confuse.
40. **No `#[non_exhaustive]` on `WorkerCommand` and `Client`** (fields private, built by methods); `Limits`
    and `Timeouts` are, with public fields set after `Default`, as the model's growing structs are.

## For review (security)

1. **A worker is untrusted by its client:** a driver's code runs in it. Every line is capped before it's read
   whole, every page's size capped and checked against `RawImage::data_len` before anything is allocated, every
   raw page made through `RawImage::new`, the model's rules and the driver checked, JSON's nesting bounded by
   `serde_json`; any breach kills the worker, and a crash's half page is never a page.
2. **SANE reads its current directory:** every worker starts in a directory of its own, made atomically under a
   random name (0700 on Unix, asked of `tempfile`; the user's own temporary directory on Windows), holding nothing
   but what the client wrote (a SANE open's `dll.conf` naming only the id's backend, and an empty `dll.d`), removed
   once the worker has exited. Whoever could write it would choose what SANE loads (`sane-dl`'s `ConfigDir` docs).
3. **A worker's program is an absolute path,** never searched for on `PATH` or, on Windows, in the current
   directory.
4. **`scan-worker-fake` can crash, hang and exit by design:** built only with the `fake` feature, its scenarios
   chosen by its argument, never in a shipped build (the docs say so).
5. **`serve` aborts its process** 10 s after its client is gone if a call hasn't returned: a library function
   that ends the process, documented as a worker's `main` and nothing else; its core, which tests drive, never
   does. An abort runs no exit handler beside the hung driver (decision 12).
6. **The worker inherits the app's environment** (plus `SANE_CONFIG_DIR` for a SANE open): it's the app's own
   binary, so nothing crosses a trust line there; an open replaces an app's own `SANE_CONFIG_DIR` (decision 17).
7. **What can outlive a crashed app:** a worker exits at the end of its input, and aborts within 10 s if a
   driver's call hangs (an exit there could itself hang on a lock the driver holds: decision 12); a process a
   driver started is outside both, until slice 9's Job object (FEAT-14) on Windows.
8. **A driver's text reaches the app's errors:** the last 8 KiB of a worker's standard error, kept in
   `Stopped` and `Crashed`, bounded, for the app to log or show.

## Risks

1. **Timing in tests** (the watchdog, a silent start, a slow consumer, the end of input): each limit is set
   short in its test and asserted with a wide margin, or waits on a state it can see (`AwaitCancel`'s midway
   progress); step 4 runs them 30 times under load once; Windows' and macOS' runners are CI's to show.
2. **The throughput gate in a debug build on CI's runners:** the fake's slowest is twenty times the gate in
   release; the pipe and the debug build take some of that, measured in step 4, and decision 33 says what
   happens below four times.
3. **Processes on Windows and macOS** (pipes, exit statuses, `CREATE_NO_WINDOW`, a process group on macOS): built
   for both with clippy here, run only by CI (TEST-16).
4. **Something in a worker writes to standard output** (a backend's `println!`, a driver library): the protocol's
   stream breaks, which the client reports as a protocol error. Moving the protocol off standard output needs
   descriptors passed at spawn (`unsafe` or another crate); slice 5 sees whether SANE's backends do it.
5. **Three things carried by name** (a page's header, `Event`'s and `Pixels`' variants, a message's level): a
   field or a variant added to the model and not to the protocol is loud for an event and silent for a page's
   field; Q-8, and slice 11's freeze.
6. **The serde form becomes stored data** once an app stores a `DeviceInfo` with it, before slice 11 has
   weighed its names and a field added later, which serde's default refuses to decode from data stored before
   it (Q-8); provisional until then, as the model is.
7. **`master` moves** (the Windows machine's pull requests; slice 1a merged as #7 while this plan was drafted):
   this slice keeps off `/crates/sane-dl/` and `crates/sane-dl.md`, edits `status.md`, `decisions.md`, the backlog
   and the milestone only where it must, takes IDs free on `origin/master`, and reads them again in step 5; a merge
   after the run is the main session's.
8. **A fast backend runs ahead of the client** by a few events through the pipe, so a cancel lands later than
   the user saw it with the fake; with a real scanner the driver is slower than the pipe. Tests put the cancel
   in place with `AwaitCancel`, and the docs say a cancel finishes the sheet the worker is on.
9. **A Windows worker hung in a driver after its app has crashed** stays until the 10 s abort (which Windows
   Error Reporting may report), and a process the driver started stays after it: FEAT-14.

## Plan review

2026-11-09, adversarial, through three lenses (fidelity to the milestone's slice 4, `crates/scan-worker.md`, the
seam note's "What slice 4 needs of it" and the rules; mechanics, each step green on its own on three systems;
behaviour and proof). Each finding was checked against the sources or a run before it was acted on; the trials
are local (`scratch/work/s0-4/review/`): a crate linking libsane two ways, its executables read with `readelf`
(`link-probe/`), and this slice's justfile edits merged with `master`'s at `42e4227` by `git merge-file`
(`merge-trial/`). The draft is kept as `scratch/work/s0-4/plan.before-review.md`. No FFI here
(`#![forbid(unsafe_code)]` in both crates), so the third lens's FFI questions don't arise; what the processes
need (spawning, pipes, `process_group`, `creation_flags`, `kill`) is `std`'s safe API.

**Accepted:**

1. *Behaviour, major: the orphaned worker's backstop could hang.* Decision 12 ended a worker whose client was
   gone and whose driver's call hung with `std::process::exit`, which runs exit handlers (std's docs: "this
   function will execute exit handlers registered with `atexit` as well as other platform-specific exit handlers
   (e.g. `fini` sections of ELF shared objects)"; on Windows `ExitProcess` detaches every DLL), all beside the
   hung driver's thread, whose locks a handler may take: the exit meant to bound the worker could hang it.
   **Changed:** `std::process::abort()`, which runs none ("The worker: `serve`", decision 12, security items 5
   and 7, Risk 9; step 4's test expects an abort there).
2. *Proof, major: the libsane gate's make-fail would have shown nothing.* Step 5 linked libsane with
   `cargo:rustc-link-lib=sane`; the probe finds no `NEEDED` libsane in any executable so built (rustc links
   `--as-needed`, and nothing calls into it), and a link that did reach every target would be caught by the test
   executables the gate read already, never by the `bin` branch this slice adds. **Changed:** the make-fail
   links the binaries alone (`cargo:rustc-link-arg-bins=-Wl,--no-as-needed` and `-lsane`: `NEEDED` in the bin
   only, in the probe), and fails the new gate naming `scan-worker-fake` while passing the gate as it stood, so
   the branch is shown to be what catches it (step 3).
3. *Mechanics, major (only CI would show it): the `env` test's paths compared as written.* The worker's
   `current_dir()` is the resolved path (POSIX `getcwd`), `SANE_CONFIG_DIR` the client's: on macOS the temporary
   directory is `/var/folders/…` and `/var` a link to `/private/var`, so they differ there, and a Windows
   runner's `%TEMP%` may hold a short name. **Changed:** compared after `fs::canonicalize` on both sides (step 3;
   step 5's "What only CI shows").
4. *Behaviour, minor: "a `message` line for each new message" can't be had by polling.* `in-progress` 0.1.0's
   `Snapshot` holds "The latest message from anywhere in the job" and a count ("a jump of more than one means
   some were replaced before the poll"). **Changed:** the latest message when the count moves, the loss said in
   the docs and the alternative weighed (decision 11); `Level` carried by name (decision 4); a step counted in
   another unit than bytes not forwarded.
5. *Proof, minor: "progress reaching the caller's tracker" had no fixed end to assert.* The drain polled "at each
   wake", saying neither that an event is one nor that the channel's close is. **Changed:** both said; step 2
   asserts the last `progress` line is the last page's whole size, step 3 that the caller's tracker ends there.
6. *Behaviour, minor: decision 33's fallback asserted in the build that measured short.* Below four times the
   gate in the debug build here, the draft set the variable in CI's Ubuntu `just test`, the same debug build on
   a slower runner. **Changed:** the fallback asserts in a release build of the one test (a `throughput` recipe
   in `ci`, run by CI's `check` job on the three systems), the debug run measuring alone, as M5 slice 6's gate
   is measured on every run and asserted under a variable.
7. *Behaviour, minor: a worker that never says `hello` had two answers.* The hand-written "no `hello` at all"
   was a `ProtocolError`, step 4's silent start `Stopped`. **Changed:** another message first is `NoHello` (that
   scenario now writes `done` first), silence past `start` is `Stopped`, output ending first `Crashed` (the
   client, the watchdog's table, the errors, the test worker).
8. *Proof, minor: "the tail of the worker's standard error in the error" checked nothing:* an abort and an exit
   write nothing there. **Changed:** the crash and exit scenarios write a known line to standard error first,
   which step 4's tests find in `Crashed`.
9. *Proof, minor: the gates' make-fails three steps after the loop changes.* The devices loop changes in step 2
   and the `bin` branch in step 3; step 5 ran both, after the client had been built on them (S0 slice 3's plan
   review moved its own the same way, its finding 11). **Changed:** each once, at its step (decision 1); step 5
   leaves them out, said.
10. *Mechanics, minor: `Pixels` is `#[non_exhaustive]` too* (the model's block), so the drain matching a page's
    pixels needs a wildcard, as for `Event`. **Changed:** "Events it doesn't know", decision 4, Risk 5.
11. *Mechanics, minor: a link to the fake from `scan-worker`'s docs would fail `just doc`,* whose first line
    builds with default features, a dev-dependency's `fake` not unified into it. **Changed:** the crate docs
    name the fake's items in code only ("The crate").
12. *Mechanics, minor: a relative work directory.* `ConfigDir::write` refuses one ("it isn't absolute",
    `sane-dl`'s `config.rs`), and the worker's directory would depend on where the app stands. **Changed:**
    refused at start with the program's rule (decision 38; step 3 tests it).
13. *Fidelity, minor: several backends against the driver check.* Decision 26 refuses a device of a driver not
    the worker's, so backends of two drivers in one worker always fail; the spec says "one driver per worker".
    The deadline across them was unsaid. **Changed:** a worker's backends one driver's, the deadline shared
    (`serve`'s bullet, decision 14).
14. *Fidelity, minor: TEST-12 left without a word.* Its line says "Wanted by slice 4", the backlog README "by
    slice 4 or 9"; decision 34 leaves it to slice 9. **Changed:** step 5 updates both.
15. *Fidelity, minor: stored data and fields added later.* serde's derive refuses a struct missing a field, so a
    field added to `DeviceInfo` (`#[non_exhaustive]` so that it may grow) breaks every `DeviceInfo` stored
    before it, unless the field takes `#[serde(default)]`. **Changed:** Q-8 asks it, and Q-5's question of
    `scan-worker`'s dev-dependency on itself ("For later slices", Risk 6).
16. *Fidelity, minor: slice 1a's IDs.* "Taken as it starts" and decision 35 named four of its six new IDs and a
    control-file line since rewritten; `master`'s files and the control file have BUG-8 and FEAT-13 too. The
    free IDs were right. **Changed:** both.
17. *Fidelity, minor: `RemoteDevice::id()`* reads as the device's id beside `info().id`. **Changed:**
    `process_id()`.
18. *Mechanics, minor: the MSRV job's platform note.* On Ubuntu it doesn't compile the Windows `cfg` branch
    (`creation_flags`, stable since 1.16, so nothing for 1.89 to break). **Changed:** its comment says so, as
    `scan-device-msrv`'s does.
19. *Mechanics, minor: step 2's dev-dependencies unsaid.* Its unit tests take the fake, while the `fake`
    feature and the dev-dependency on itself come in step 3. **Changed:** step 2 names `scan-device` with
    `["fake"]`.

**Rejected:**

1. *`Fault::AwaitCancel` as scope the slice could avoid* (the test worker's own backend, wrapping the fake's
   progress, could hold mid-page). The control file allows slice 4's changes to `scan-device`; the script sits
   with the fake's other faults, usable in a test's process as in a worker; a wrapper would count the midway the
   fake counts already.
2. *The justfile's edits conflicting with `master`'s at the merge.* Tried: `git merge-file` of this slice's
   edits (the loop, the variable, the recipe, `msrv`, the `jq` branch) with `master`'s at `42e4227` merges with
   no conflict. Said in "The workspace around it"; nothing else to change.
3. *The Job object left out against the spec's hygiene.* The milestone's slice 4 entry doesn't ask it; FEAT-14
   carries it to slice 9, where Windows drivers are tried, and `process-wrap`'s `std` job lacks kill-on-close
   (`src/std/job_object.rs`). Kept as planned.
4. *The throughput gate asserted on Windows and macOS too.* Windows' pipes may be slower, but Windows is where
   the TWAIN workers run, so that's where the gate matters most; CI shows it (Risk 2), and decision 33's
   fallback moves the assertion to a release build, never off.
5. *Step 3 too large, to be split.* The client's loop is one thing, its failures tested in step 4; split, a
   step would hold a client that can't finish a call.

## So far

**Step 1, `scan-device`'s changes: the `serde` feature and `Fault::AwaitCancel`** (2026-11-09; one commit, "Give
scan-device a serde feature and a fake fault that awaits the cancel", on `73f7b02`, the plan's commit). The checks
before it: none (the plan review's are in "Plan review"; no step had been checked).

- **The workspace:** `[workspace.dependencies]` gains `serde = { version = "1", features = ["derive"] }` and
  `serde_json = "1"` (decision 37); until step 2's `scan-worker` takes them, `scan-device` is the one member
  that does, which the manifest's comment ("joins once a second member takes it") is true of again from step 2.
  `/Cargo.lock` gains seven packages, nothing else changed ("Locking 7 packages to highest Rust 1.89 compatible
  versions", offline): `serde`, `serde_core` and `serde_derive` 1.0.229, `serde_json` 1.0.154, `itoa` 1.0.19,
  `memchr` 2.8.3, `zmij` 1.0.23; `syn` stays at 3.0.9.
- **`scan-device`'s manifest:** `serde = { workspace = true, optional = true }` and the feature `serde =
  ["dep:serde"]`, with a comment; the self dev-dependency takes `["fake", "serde"]`, and `serde_json` joins the
  dev-dependencies.
- **The derives:** `#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]` on the 22
  types "`scan-device`'s changes" names, each beside its `#[derive]`; serde's defaults, no attribute else.
- **The tests:** six unit tests in `/crates/scan-device/src/round_trip.rs`, a module of its own behind
  `#[cfg(all(test, feature = "serde"))]`, one place listing every type the feature encodes: a function for each
  enum giving its values (a value of every variant, those with fields at their edges, `Clamped` over every
  source, format and paper, `Driver::Twain` at 32 and 64), passing them through `every`, which numbers each
  value's variant by an exhaustive `match` and asserts every number has a value, so a variant added fails the
  build until it's numbered and the test until it has a value; every variant of the 14 enums through JSON and
  back, equal; every struct with each field set (none as its `new` leaves it; ids and messages with quotes,
  backslashes and characters past ASCII; `WindowOwner` at 0 and `u64::MAX`), equal; `SizeMm`'s constants and
  the `f32` edges (`0.0`, `-0.0`, the smallest subnormal, the smallest normal, `EPSILON`, `MAX`, `-MAX`) bit
  for bit, alone and in `Paper::Size`; `NaN` and both infinities written `null` and refused on reading; the
  form pinned on literal JSON (a `DeviceInfo`, `Driver::Twain`, `Source`, a `Range`, both `Paper`s, two
  `ScanError`s, a `WindowOwner`); and what the model can't hold refused as it's read (a step of 0, a variant
  it hasn't, a `DeviceInfo` missing a field). Two in `tests/fake_faults.rs` through one helper: `AwaitCancel`
  at sheet 2's front midway in duplex, and at its back's start; each takes the events up to the wait on a
  rendezvous channel, waits for the tracker to stand where the side waits (half the front's bytes; the front's
  whole), sees nothing sent for 300 ms, the batch unfinished and `fed()` answering 2 (no lock held), then
  cancels: the rest of sheet 2 delivered (both sides midway, the back alone at the back's start), each page
  `pattern`'s, `Done::Cancelled`, `fed()` 2 and one sheet left in the feeder.
- **The fault:** `Fault::AwaitCancel` in `/crates/scan-device/src/fake/`, fired as the others are (once, at a
  side's start or midway), polling `progress.check()` each millisecond (`AWAIT_CANCEL_POLL`, the check the
  batch makes between sheets), holding nothing: the sheet is the batch's own, out of the device's paper.
- **The docs:** the crate docs' plain-data paragraph points at the feature, and "Features" gains `serde` (the
  types, serde's default form with two literal examples, no page or event and why, a step of 0 refused,
  JSON's `null` for a size not finite, provisional as the model is) and the cancel's wait in `fake`'s bullet;
  the fake module's "Faults" list and `Fault`'s docs gain `AwaitCancel`; the README's paragraph and
  "Features" both. No CHANGELOG: `scan-device` takes one when it's made ready to publish (its manifest).
- **The MSRV probe** (`scratch/work/s0-4/msrv-probe/scan-device/`, its README): over `cargo tree -e
  normal,build,dev --target all`, the probe's graph (newest versions, resolved offline) and the workspace's own
  alike, 32 packages, the highest `rust-version` still 1.85 (`in-progress`, and `getrandom` through
  `rusty-fork`'s `tempfile`); the new packages 1.71 at most (`serde_derive`, `serde_json`, `zmij`). So 1.89
  stays. `cargo tree -p scan-device -e normal,build --target all --all-features`: 11 packages, slice 3's 8 and
  `serde`, `serde_core` and `serde_derive`, the only new ones. `cargo +1.89 check --locked -p scan-device
  --all-targets` clean, as a quick check.
- **Make-fails** (`scratch/work/s0-4/step-1/make-fail.md`, its script beside it), on a worktree there, removed
  after, the branch unchanged; each failed as it should, and the base passed after: a variant added to
  `Status`, and one to `ScanError`, each building the library and failing the unit tests' build (`E0004` in
  `round_trip.rs`); a variant numbered with no value, failing `every`; `SourceCaps::paper_auto` skipped by
  serde; sizes written to three decimals (the subnormal lost); `null` read as `NaN`; `Driver` renamed snake
  case (the literal form); and `AwaitCancel` a no-op, ending the side with an error once cancelled, and holding
  the paper locked while it waits, each failing both new tests. The two `AwaitCancel` tests run 30 times in a
  loop under the memory scope, none failing (`scratch/work/s0-4/step-1/await-cancel-loop.txt`, 0.3 s a run).
- **Quick checks:** `cargo fmt --all -- --check`; `cargo clippy --locked --workspace --all-targets -- -D
  warnings`, the same with `--all-features`, `--lib`, and `-p scan-device --lib --features serde` (the feature
  alone); `cargo doc` with `-D warnings`, default features, all, and `serde` alone; `cargo test --locked -p
  scan-device` (31 units, 6 of them new; `fake_faults` 21, 2 new; the rest as they were). The core (`just
  ci`, the test list, the commit's scans) is the step's check's.
- **Departures from the plan, each small:** the serde tests sit in one module rather than beside each type,
  and do more than the plan listed (`every`'s runtime half, the form pinned, the refusals, more `f32` edges
  and both infinities); the `AwaitCancel` tests also see the side send nothing while it waits and the paper
  unlocked, and compare the pages with `pattern`'s. The fault's docs say a batch from the feeder ends
  `Done::Cancelled` after the wait; on the glass the batch has its one page as any cancel there leaves it
  (`Done::OnePage`, the fake as slice 3 built it), which nothing here tests or changes.
- **Decisions, for review with the plan's:** `round_trip.rs` named so a module called `serde` doesn't shadow
  the crate in the derives' paths; `check()` polled rather than `is_cancelled()`, as the batch's own checks
  are, so a `Progress` whose `check` says more is read the same; no `#[serde(...)]` attribute anywhere, the
  plan's "serde's defaults" kept for slice 11 to weigh (Q-8).

**Step 2, `scan-worker`, its protocol and its worker side** (2026-11-09; one commit, "Add scan-worker: the
worker's side of drivers run in a process of their own", on `afc363c`). The checks before it: step 1's check
of `afc363c` passed (`scratch/work/s0-4/flow/checks.md`): `just ci` green with both `SANE_DL_REQUIRE_*` set
(695 tests, the three MSRV recipes on 1.89, `sane-dl-package` run); the test list 361 to 369, none lost; the
gates holding, `scan-device`'s graph 11 packages; 1.89 standing (32 packages, the highest 1.85); the
exhaustiveness pair run again on a worktree (`E0004` both); the two `AwaitCancel` tests 30 of 30. Its two
remarks: the `[workspace.dependencies]` comment, true again from this step (`scan-worker` takes `serde` and
`serde_json`); and `round_trip.rs`'s hand-kept count in `every`, for the code review.

- **The crate:** `/crates/scan-worker/`, 0.0.0, `publish = false`, `rust-version` 1.89; dependencies
  `scan-device` (path, `serde`), `serde` and `serde_json` (workspace); one dev-dependency, `scan-device` with
  `fake` and `serde`. `#![forbid(unsafe_code)]`, `#![warn(missing_docs)]`; modules `wire` and `serve`,
  private; `serve` and `PROTOCOL` (1) public, `pub use scan_device`.
- **`wire`:** `Request` (`list`, `open`, `caps`, `scan`, `settings_window`, `cancel`, `quit`) and `Reply`
  (`hello`, `devices`, `opened`, `caps`, `window`, `applied`, `sheet`, `page`, `status`, `progress`,
  `message`, `done`, `error`, `refused`), externally tagged, snake case, serde's defaults otherwise; a page's
  header (`pixels` raw with format and size or encoded with its kind, `dpi`, `sheet`, `side`, `side_known`,
  `bytes`) and a message's level by name (`info`, `warn`, `error`; decision 4); `line` (a message's JSON and
  its `\n`); `read_line` (a line under a cap, `take` of the cap and one byte, a longer line skipped with
  `BufRead::skip_until` and never held; the cap counts the line without its `\n`).
- **`serve`:** as "The worker: `serve`" has it. The reader admits each request against the shared state
  (busy, a device open, the batch's tracker) in one place, `State::admit`: `cancel` and `quit` at any time,
  anything else refused while a call runs, `list` and `open` only with no device, the device's calls only with
  one; the batch's `Tracker` made there as `scan` is read (decision 9). The main thread makes the calls, clears
  the state before it writes the answer (so the client's next request, sent once it has it, is never refused
  as early), and for a batch runs `device.scan` with a `sync_channel(0)` whose drain thread writes each event
  (a page's line and bytes under one lock) and polls the tracker after each, every 100 ms, and once more as the
  channel closes; `done` or `error` only after the drain is joined. The end: `quit`, or the end of input (a
  cancel and a quit), drops the device and the backends, then closes the output, so a client sees the output
  end only once the driver is closed; a failed write drops the writer, ends the batch at its next `send` and
  the worker with 1. The orphan backstop: after the input ends, the reader waits up to 10 s on a channel held
  by the core, and calls `std::process::abort` only if the core hasn't returned (decision 12); the core's
  `orphaned` is `None` in the tests, so they never abort.
- **The docs:** the crate docs give why, the worker's side, and the protocol whole (framing, the version,
  every request and answer in a table, a batch's messages and the progress rule with its lost messages, a page's
  line and bytes, what a worker takes and when, the end, and an exchange whose lines a run of the core
  printed); `serve`'s docs say standard output is the protocol's, the thread rule, the backends' order, the
  exits and the abort, with a `no_run` worker `main`. The README says the same, short, and that the client
  comes next. No CHANGELOG: one comes when it's made ready to publish (its manifest).
- **The workspace:** member after `scan-device`; the lock gains `scan-worker` alone; the justfile's
  `scan_worker_msrv`, `scan-worker-msrv` (`cargo +1.89 test --locked -p scan-worker`), `msrv` with it, and
  the devices loop over `scan-worker` too; CI's `scan-worker-msrv` job after `scan-device-msrv`; the root
  README's row and its "Next" line.
- **The tests,** 28 unit tests: in `wire`, every request and every reply on a literal line both ways (the
  form pinned), what isn't a request refused, a line's newline its only one, and the cap (at it read, one past
  it skipped, the input's end mid-line, a long line never held); in `serve`, the core over `std::io::pipe`
  pairs with the fake, each wait bounded at 10 s: `hello` first as a literal line; a listing (and its
  deadline's `TimedOut`, the worker going on); an open as stored, renamed (`renamed_from`), refused, gone and
  `BadId`, no device open after them; `caps` and the settings window (and `Unsupported` without one); batches
  simplex (`Bw1`, its padding) and duplex (`Rgb8`, sides reported), the events in order, every page's bytes
  `pattern`'s, every progress line within the page's size and the last its whole, `done` last and the next
  answer next; `applied` and `status` before the first sheet, a clamp said; an encoded page (CCITT G4) as the
  driver gave it, on a literal line; a jam's `error` after the pages before it, and Continue in the same
  worker; a cancel through `AwaitCancel` at sheet 2's front midway (nothing written while it waits, the sheet
  finished front and back, `done` `Cancelled`, one sheet left, which the next batch takes); `scan` and
  `cancel` in one write; refusals out of state, while a batch runs, unreadable (not JSON, not a request,
  trailing data, not UTF-8) and past the cap (one line at exactly 1 MiB taken, one byte more refused, 4 MiB
  refused once), the worker going on each time; `quit` (the device and backend dropped before the output
  ends) and `quit` mid-batch; the end of input mid-batch and idle; output that fails mid-batch (`Exit::
  OutputFailed`, the feeder not run out) and before `hello`; a test backend's message and byte counts carried
  (and a count in another unit not); the progress poll alone against a tracker (bytes only, only when moved,
  the latest message alone); several backends listed in turn, opened through the first that doesn't answer
  `BadId`, the first error ending a listing, and the deadline shared.
- **Make-fails** (`scratch/work/s0-4/step-2/make-fail.md`, its script beside it), on a worktree there at a
  local commit of the step, removed after, the branch unchanged; 14 cases, each failing as it should, the base
  passing after: the plan's three (`done` written without the drain joined, failing 5 of 5 runs, nine to eleven
  tests each; a `cancel` not passed to the tracker; a refused request ending the worker) and a line unreadable
  or past the cap ending it; the tracker stored by the main thread (decision 9; 5 of 5); the busy check gone;
  the input's end, and `quit`, not cancelling the batch; each backend given the whole deadline; an open
  stopping at the first `BadId`; progress in any unit; messages never carried; a page's bytes before its line;
  the cap without its byte more.
- **The MSRV probe** (`scratch/work/s0-4/msrv-probe/scan-worker/`, its README): 16 packages, normal, build and
  dev, every target, in the probe's newest-versions graph and the workspace's alike; the highest
  `rust-version` 1.89, `scan-device`'s own (then `thiserror`'s 1.77, `in-progress`' 1.85), so 1.89. `cargo
  tree -p scan-worker -e normal,build --target all --all-features`: `scan-worker`, `scan-device`,
  `in-progress`, `serde`, `serde_core`, `serde_derive`, `serde_json` (`itoa`, `memchr`, `zmij`),
  `thiserror` and `thiserror-impl` (through `scan-device`), the proc-macro crates; no codec, PDF, OCR or GUI
  crate. What 1.89 is needed for here: let chains (1.88); `std::io::pipe` (1.87, the tests); `skip_until`
  (1.83).
- **Quick checks:** `cargo fmt --all -- --check`; `cargo clippy --locked --workspace --all-targets
  --all-features -- -D warnings` and `--lib`; `cargo +1.89 check` and `clippy -D warnings` of `-p scan-worker
  --all-targets`; `cargo doc` with `-D warnings`, the workspace's default features and all, and `-p
  scan-worker` alone; `cargo test --locked -p scan-worker` (28 units, 1 doctest), and the unit suite 30 times
  in a loop under the memory scope, none failing (`scratch/work/s0-4/step-2/loop.txt`). The core (`just ci`,
  the test list, the gates and their make-fails, `just scan-worker-msrv`, the scans) is the step's check's.
- **Departures from the plan:** `thiserror` isn't in the manifest yet: nothing here has an error type, so it
  comes with the client's in step 3 (it's in the graph through `scan-device` all the same). The test of `scan`
  and `cancel` in one write takes either end: no sheet, or the first sheet finished (`done` `Cancelled` both
  ways), where the plan said no sheet. Decision 9 holds either way (the cancel always finds the batch's
  tracker); whether the batch's first check comes before the reader has read the buffered `cancel` is a race
  between the worker's two threads, lost 4 times in 2000 under eight batches at once (`loop.txt`). So every
  sheet's front waits for the cancel (`AwaitCancel` at its start), a missed cancel holding the batch until the
  test's bound, and the test runs 20 batches in one worker, which the tracker made on the main thread fails 5
  runs of 5. The README's row and "Next", and CI's job comment, say the crate as it stands at this step (the
  client next; no platform code yet), for step 3 to bring up to date. More tests than the plan listed (above).
- **Decisions, for review with the plan's:** one admission function for every refusal, on the reader, so a
  request is never queued behind a call (decision 39); the state cleared before the answer is written; the
  reader goes on reading after `quit`, so the end of input still arms the backstop when a client sends `quit`
  and dies; the backstop aborts only if the core hasn't
  returned (a channel the core holds), never on a timer alone; a failed write drops the writer, so nothing
  follows a message cut short; the output closed only after the device and backends are dropped; a request
  after `quit` while idle is taken and never answered (the worker is ending); a line cut short by the end of
  input is the end, never a request; `ExitCode::from(1)` for a failed output, `SUCCESS` otherwise; the
  refusals' reasons are for a log, not matched by a client; the test harness, `Piped`, reads the worker's lines
  on a thread of its own so every wait is bounded.

**Step 3, the client, its normal paths and its refusals** (2026-11-09; one commit, "Add scan-worker's client:
a worker for each device, its pages capped and checked", on `ee4ef2f`). The checks
before it: step 2's check of `ee4ef2f` passed (`scratch/work/s0-4/flow/checks.md`): `just ci` green with both
`SANE_DL_REQUIRE_*` set (753 tests, the four MSRV recipes on 1.89, `just scan-worker-msrv` among them,
`sane-dl-package` run); the test list 369 to 398, none lost; the gates holding, `scan-worker`'s graph 16
packages, 26 binaries read for libsane; 1.89 standing (16 packages, the highest `scan-device`'s 1.89); the
devices loop's make-fails on a worktree, `png` as a normal dependency and behind a feature of its own, each
failing the gate naming `scan-worker` and `png`; the crate docs' 12 literal lines read back through `wire`'s
types byte for byte, a row per message agreeing; the three `serve` mutations failing (`done` before the join 5
runs of 5). Its two remarks, for the code review: `serve` writes through `io::stdout()`, a `LineWriter`, so a
page's bytes holding `\n` go out in pieces before the flush (correct; step 4's throughput run shows the cost);
and the shared-deadline test's 1.8 s bound, for CI's slower runners.

- **The manifest:** `sane-dl` (path), `tempfile = "3"` and `thiserror` (workspace) join the dependencies; the
  `fake` feature (`["scan-device/fake"]`, the library unchanged by it); the dev-dependency on the crate itself
  with `fake`; `[[bin]] scan-worker-fake`, `required-features = ["fake"]`, `test = false`, `doc = false`. The
  description says the client. `/Cargo.lock`: `scan-worker`'s list gains `sane-dl`, `scan-worker`, `tempfile`
  and `thiserror`, no package added (each was there for `sane-dl` and its tests).
- **The client** (`/crates/scan-worker/src/client.rs`, and `client/` `spawn.rs`, `read.rs`, `worker.rs`):
  `Client` (`new`, `worker`, `limits`, `timeouts`, `work_dir`, `list`, `open`), `WorkerCommand` (`new`, `arg`,
  `program`, `args`), `Limits` and `Timeouts` (`#[non_exhaustive]`, public fields, `Default` 1 MiB and 1 GiB;
  10, 60, 10 and 5 s), `RemoteDevice` (`info`, `renamed_from`, `process_id`, `caps`, `scan`, `settings_window`,
  `close`; `Debug` by hand), as "The client" has them. `spawn`: the program and the work directory absolute or
  `InvalidInput`; the directory by `tempfile::Builder` (`scan-worker-` and a random name, `permissions(0o700)`
  on Unix, so it's made at that mode); for a `Driver::Sane` open, the id's name (after `sane:`, each character
  up to U+00FF its byte, else none), `backend_of`, `ConfigDir::write(.., Search::BeforeSystem)` and its
  `value()` as `SANE_CONFIG_DIR`; `process_group(0)` on Unix, `CREATE_NO_WINDOW` on Windows, nothing elsewhere.
  `read`: the reader thread (each line under `Limits::line`, a page's bytes capped, checked against `data_len`
  and its `dpi` before a byte is allocated, read whole, made through `RawImage::new` or `Encoded`, then
  `RawPage::new`), handing each message on a `sync_channel(0)`, its last message the end (whether a page was cut
  short) or what broke the protocol; standard error's drain, its last 8 KiB kept. `worker`: the `hello` under
  `Timeouts::start`; each call's answer in its place, the driver compared on every listed and opened device, the
  caps checked; the batch loop (each event sent on as it comes, the watchdog's time counted only while waiting
  on the worker and restarted after each handing on, the cancel read every 50 ms and at each message and sent
  once, a failed `send` a cancel with the rest dropped and `Done::Cancelled`, progress as `set_total` on a
  changed total and `set`, messages to `progress.message`); every request round-tripped before it's written
  (`Unsendable`); the ends (`Protocol` and `Stopped` kill at once, `Crashed` gives the grace to exit, `close`
  and drop `quit` then the grace then a kill), each reaped by `try_wait` with growing pauses, or handed with its
  directory to a thread; every call after an end `Ended`. `/crates/scan-worker/src/error.rs`: `Error` and
  `ProtocolError` as "Errors" has them, `thiserror`, `Scan` transparent and `Start`'s `io::Error` its source.
- **`wire`'s changes:** `read_line` no longer skips the rest of a line past the cap (the reader that found it
  decides: `serve`'s skips it with `skip_line`, the client's reads no more and kills the worker, so a worker
  writing an endless line is refused at once rather than read to its end); `Reply::name`; `MessageLevel` into
  `Level`.
- **The test worker** (`/crates/scan-worker/src/bin/scan-worker-fake/`): `main.rs` (the scenario from the first
  argument, a count of sheets from the second, 3 by default; `env`; `slow-listing`; the hand-written ones) and
  `scenarios.rs` (the scripted ones), which the integration tests take too (`#[path]`), so every batch through a
  worker is compared with the same script run in the test's process. Scenarios: `feeder`, `encodings`, `jams`
  (sheet 2's back midway, sheet 4's front at its start), `double-feed`, `cover-open`, `gone`, `warm-up`,
  `reports-sides`, `window`, `renamed`, `not-installed`, `hang`, `crash` and `exit` (at sheet 3's front midway,
  `FAILING` written to standard error first), `await-cancel-midway` and `await-cancel-back` (sheet 2), and
  `scan-more` and `talking` (new sheets at each batch; the driver's warning `SAID` as each starts); `env`
  (devices of `Driver::Sane` saying the current directory, `SANE_CONFIG_DIR` and the directory's contents in
  JSON); `slow-listing` (a minute's listing, and a backend whose drop takes a minute, so only a kill ends it
  soon); by hand: `version`, `not-json`, `no-hello`, `silent`, `deaf`, `long-line`, `out-of-place`,
  `refuses`, `no-source`, `big-page` (a line stating 1.2 GB raw, none sent), `page-length`, `padding`,
  `zero-dpi`, `cut-short`. Step 4's (`hang`, `crash`, `exit`, `await-cancel-*`, `silent`, `cut-short`) are
  built and not yet tested.
- **The tests:** 11 unit tests (`read`: messages and pages in order then the end, the line cap, the page cap and
  each check before the bytes, the ends within a page and a line, lines that aren't messages, the 8 KiB tail;
  `spawn`: a `sane:` id's name and backend; `worker`: `Unsendable` and the JSON edges sent, the caps rules,
  the watchdog's deadline). 39 integration tests through the binary: `devices.rs` 12 (a listing equal to the
  script's; another driver's device refused listed and opened; opened as stored, renamed, gone and `BadId`;
  `NotInstalled`; a listing and an open past their deadline `TimedOut` and killed, shown by a 30 s grace never
  waited; a worker that never answers `Stopped` at the deadline and the grace; caps and the settings window,
  and `Unsupported`; a worker each open device; `NoWorker`; a relative program, a missing one, a relative work
  directory and a missing one refused at start, nothing left; `WorkerCommand`'s parts; `Send` and `Sync`);
  `batches.rs` 14 (each of the five formats, duplex, every event equal to the in-process script's and every
  raw page `pattern`'s; CCITT G4, JPEG, PNG and TIFF pages as the driver gave them; the flatbed's page numbered
  as asked; reported sides; "Scan More" from `first_sheet`; a jam at a back then a front with Continue twice
  in one worker; a double feed and an open cover; an empty feeder; gone, and every call after it `Gone`;
  warming up; progress ending at the last page's bytes; a driver's message at the caller's tracker; `NaN` and
  infinite sizes `Unsendable`, nothing sent, the next batch whole; a page past a lowered cap refused, the
  events before it kept, every call after `Ended`); `protocol.rs` 9 (each hand-written worker refused as its
  `ProtocolError`, the calls after `Ended`); `sane_spawn.rs` 4 (`sane:test:0`'s directory naming `test`,
  `sane:Desk scanner` and `sane:test:☃` naming none, `SANE_CONFIG_DIR` the directory and `:`, the worker's
  current directory that directory, under the test's work directory, 0700 on Unix, gone after `close` and
  drop; a listing's empty with the app's variable; the system's temporary directory by default; every path
  through `fs::canonicalize`). 2 doctests (the crate's worker-and-client `main` and `Client`'s, `no_run`). So
  the test list should go from 398 to 450.
- **The docs:** the crate docs gain the two halves, an example of a binary that's its own worker and client,
  "The client" (which worker and where it starts, the SANE configuration, the process group and console, what's
  checked and refused, `Unsendable`), "The watchdog, and cancel" (its table) and "When a worker fails"; each
  public item documented. The README says the client and the `fake` feature. The root README's row says the
  client, and "Next" is slice 5's `sane` backend. CI's `scan-worker-msrv` comment says the Windows branch
  (`creation_flags`, stable since 1.16) isn't compiled on that Rust there. No CHANGELOG (unpublished).
- **The libsane gate** (`/justfile`): the `jq` reads every `bin` target's `.executable` too, its comment saying
  so. Read here on a plain `cargo build --workspace --all-features`: `scan-worker-fake` listed beside
  `fake-libsane`'s library, its `NEEDED` libc, ld-linux and libgcc_s alone. The gate's make-fail (the build
  script linking libsane into the binaries alone, `-Wl,--no-as-needed` then `-lsane`, failing the gate naming
  `scan-worker-fake` and passing the gate as it stood) is the checking agent's, with the gates' other runs.
- **The MSRV probe** (`scratch/work/s0-4/msrv-probe/scan-worker-step-3/`, its README): 31 packages in the
  probe's newest-versions graph and the workspace's alike; the highest `rust-version` 1.89, `sane-dl`'s and
  `scan-device`'s, then `libloading`'s 1.88, so 1.89 stands. `cargo tree -p scan-worker -e normal,build --target
  all --all-features`: 31 packages, step 2's 16 and `sane-dl`, `libloading`, `tempfile`, `fastrand`,
  `getrandom`, `once_cell`, `rustix`, `errno`, `bitflags`, `libc`, `linux-raw-sys`, `cfg-if`, `r-efi`,
  `windows-sys`, `windows-link`; no codec, PDF, OCR or GUI crate.
- **Make-fails** (`scratch/work/s0-4/step-3/make-fail.md`, its script beside it), on a worktree there at a local
  commit of the step, its own target directory, the branch unchanged; 21 cases, each failing as it should, the
  base passing after: the plan's three (a page's size not checked against `Limits::page`, failing the
  hand-written and the lowered cap's tests and the reader's unit test; the request's round trip not checked; the
  listed device's driver not compared), and the opened device's driver not compared; a timed-out listing, and
  open, quit rather than killed; `SANE_CONFIG_DIR` not set; a `sane:` id read as its UTF-8 bytes; the directory
  at the umask's mode; the worker not started in it; it kept after the worker exits; the caps unchecked; a
  `dpi` checked for finiteness alone; a raw page's length unchecked; the `hello`'s protocol not compared; a
  refusal taken as an answer; a progress line's count, and a message, not given to the caller; `first_sheet`
  sent as 1; a call after an end written to the worker; standard error's first 8 KiB kept.
- **Quick checks:** `cargo fmt --all -- --check`; `cargo clippy --locked --workspace --all-targets` with and
  without `--all-features`, and `--lib`, `-D warnings`; `cargo clippy --locked -p scan-worker --all-targets
  --all-features -- -D warnings` for `x86_64-pc-windows-gnu` and `x86_64-apple-darwin`, clean (the `cfg` code);
  `cargo +1.89 check --locked -p scan-worker --all-targets`; `cargo doc` with `-D warnings`, the workspace's
  default features and all; `cargo test --locked -p scan-worker` (39 units, 39 integration tests, 3 doctests),
  and the units and integration tests 10 times in a loop, none failing (`scratch/work/s0-4/step-3/loop.txt`);
  `cargo test --workspace --no-run`. `cargo +1.89 clippy -p scan-worker` now lints `sane-dl` too, and 1.89's
  clippy refuses `sane-dl`'s own `DEFAULT_LIBRARIES.is_empty()` (`const_is_empty`), as it does for `-p sane-dl`
  without this step: `sane-dl`'s, left alone (the control file); CI's clippy is stable's. The core (`just ci`,
  the test list, the gates and the libsane gate's make-fail, `just scan-worker-msrv`, the scans) is the
  step's check's.
- **Departures from the plan:** the test worker is a directory, `src/bin/scan-worker-fake/` (`main.rs` and
  `scenarios.rs`), not one file, so the integration tests take its scripts and compare every batch with the same
  script in process; more scenarios than "The test worker" listed (`deaf`, `out-of-place`, `refuses`,
  `reports-sides`, `scan-more`, `talking`, and `slow-listing`'s held exit), so every `ProtocolError` variant is
  reached through a worker and each claim has a test; the `await-cancel` pair named by their place.
  `wire::read_line` changed (above), step 2's unit test of it skipping through `skip_line` now. A test of a
  listing's and an open's watchdog (`deaf`) is here, beside the refusals, since step 4's list doesn't hold
  it. The libsane gate's make-fail is left to the step's check, with the gates' other runs (the workflow's
  rule), the `bin` branch's reading of the binary shown here.
- **Decisions, for review with the plan's:** `close` after an earlier failure is `Ok` (the failure was said,
  and nothing is left to close), and a worker that won't exit within the grace after `quit` is `Stopped` with
  the grace as its silence, one that exits with a failure `Crashed`; `Crashed`'s status is `None` where the
  worker hadn't exited within the grace of its output's end and was killed (its signal would say nothing of the
  driver); a killed worker is given the grace again to be reaped before it's handed to a thread, so the caller
  waits at most twice the grace; the error's standard error is read once it has ended, or the grace has passed
  (a process the driver started may hold it); a listing's worker is quit whatever its answer but `TimedOut`, its
  `close` errors dropped (the answer is had); an open that fails quits its worker, but `TimedOut` kills it; a
  reply other than `hello` first, a page among them, is `NoHello`, and a refusal as the answer to a request is
  `Refused`; a too-long time (`Duration::MAX`) is never, not a panic on the clock; the reader hands each message
  on a rendezvous channel, so the client holds two pages at most, one being handed to the caller and one read
  and waiting; `Limits`' and `Timeouts`' defaults live with the client, the worker's own
  request cap staying in `wire`; a page of 0 width or height is `Layout(Empty)`, said by `data_len` before the
  length is compared.
