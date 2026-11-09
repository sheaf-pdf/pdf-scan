# scan-worker

Layer: Devices (`crates/README.md`). Paths are from `docs/pm/`.

**As built** in S0 slice 4 (`slices/s0-4-scan-worker.md`, which has the design, its decisions and what each step
checked): `/crates/scan-worker/`, 0.0.0, provisional and unpublished (`publish = false`), on `scan-device`'s
model and its `fake`; MSRV 1.89 (`decisions.md`). No `unsafe` (`#![forbid(unsafe_code)]`: spawning, pipes,
killing and process groups are `std`'s safe API), and every public item documented (`#![warn(missing_docs)]`).

- **What:** drivers in a process of their own
  ([M5 proposal 4](https://github.com/sheaf-pdf/sheaf/blob/3232a35/plan/m05-scan/proposals/p04-scanner-drivers-run-worker-process.md)).
  Everyone who drives 32-bit TWAIN from a 64-bit app does it this way (NAPS2's workers, VintaSoft's service,
  LEADTOOLS' thunk server, RSWebTWAIN's sidecar), and nobody publishes the broker as a library: this crate fills
  that hole (crates.io searched again 2026-11-09, still nothing of the kind). Two halves that ship together:
  `serve`, a worker binary's `main`, and the `Client`, in the app. The crate re-exports `scan_device`, so an
  app names the model's version its API takes.
  - **The protocol:** JSON lines on the worker's standard input and output (UTF-8, each line ended by `\n`; a
    message an object whose one key names it, or the name alone where it carries nothing), the model's types
    in `scan-device`'s `serde` form; a `page` line followed by exactly the bytes it states. First, unasked, the
    worker's `hello` with the protocol's version (`PROTOCOL`, 1, kept while the crate is 0.0.x, so a worker and
    its client come from one build) and the crate's; a client that speaks another refuses it before writing
    anything (a mismatch is an error, not a negotiation: worker and app ship together), reading
    `hello.protocol` alone where the rest isn't this protocol's, the one field every protocol's hello keeps. Requests: `list` (a deadline), `open` (the stored `DeviceInfo` and a deadline), `caps`, `scan`
    (the settings and `first_sheet`), `settings_window` (the owner, or `null`), `cancel` and `quit`. Answers:
    `devices`, `opened` (the `info` found and `renamed_from`), `caps`, `window` (`WindowClosed`), `done` and
    `error` (a `ScanError`); in a batch, `applied`, `status`, `sheet`, `page`, `progress` (the bytes done and
    the total of the page in flight) and `message` (a level and the driver's text); `refused` for a request
    out of turn, unreadable or past 1 MiB, the worker going on. A page's line holds its pixels (raw, with its
    format, width and height; or encoded, with its kind), `dpi`, sheet, side, `side_known` and `bytes`; a raw
    page's bytes are the model's raw layout, so `RawImage::data_len` gives their length from the line. The
    crate docs give the protocol whole, every message on a literal line that unit tests pin and an exchange a
    run printed, so another implementation could speak it. The proposal's list (`list`, `caps`, `scan`,
    `cancel`, `quit`; `device`, `caps`, `sheet`, `page`, `status`, `error`, `done`) became this through the
    seam note's "What slice 4 needs of it".
  - **The worker's side:** `serve(backends) -> ExitCode`, the whole `main` of a worker binary over any
    `scan-device` backends of one driver API: listed in turn and joined, a device opened through the first
    that doesn't answer `BadId`, the request's one deadline shared among them. Every backend and device call
    is made on its main thread, in order (a driver's thread is the one that opened it); a reader thread admits
    each request (one call at a time, one device a worker, no listing beside it), cancels at once, and makes a
    batch's tracker as it reads `scan`, so a `cancel` written right after it is never missed; a drain thread
    for each batch writes its events from a channel that holds none (`sync_channel(0)`), so a slow pipe holds
    the backend back and the sheets before the one in flight are whole in the pipe. Progress is read from the
    batch's tracker at each event, every 100 ms and as the batch ends, and written only when it moved, in
    bytes alone; a driver stuck mid-page goes silent. The tracker keeps the latest message, so one replaced
    between two reads is lost (said in the docs). `quit` drops the device and the backends, so the driver is
    closed, before the output ends; the end of its input is a cancel and a `quit`, and 10 s later, should a
    driver's call, or its close, not have returned, the worker aborts (`std::process::abort`, never `exit`,
    whose exit handlers and libraries' finalizers could hang on a lock the hung driver holds). A client keeps
    the input open after `quit` until the worker exits, so the abort is only ever a client gone. The binaries themselves
    (`sheaf-scan-worker`, `sheaf-scan-worker-x86.exe`) are the app's; an app may be its own worker, as the
    crate docs' example is.
  - **The client:** `Client` (`new`, `worker(driver, WorkerCommand)`, `limits`, `timeouts`, `work_dir`, `list`,
    `open`), `Send` and `Sync`; `RemoteDevice` (`info`, `renamed_from`, `process_id`, `caps`, `scan` with a
    `SyncSender<Event>` and a `&dyn Progress`, `settings_window`, `close`), `Send`. It mirrors the model's calls
    with an error of its own, `Error` (`Scan` holding the device's `ScanError`, `NoWorker`, `Start` for the
    program, `WorkDir` for the work directory, `Stopped` with the `Timeout` that ran out and how long it waited,
    `Crashed` with the exit status and whether a page was cut short, `Protocol` with a `ProtocolError`,
    `Unsendable`, `Ended`; the struct variants `#[non_exhaustive]`), and doesn't implement `Backend` and
    `Device`, whose `ScanError` can't carry its failures. A worker for each listing (quit once it answers, so a slow probe never holds a scan) and for each
    open device, chosen by the device's `Driver`; its program and the work directory absolute paths, refused
    otherwise. Each starts in a private directory of its own (`tempfile`, under a random name, 0700 on Unix,
    removed once the worker has exited); a SANE open's holds the stored id's one-backend `ConfigDir`
    (`sane_dl::backend_of`, `Search::BeforeSystem`), given as `SANE_CONFIG_DIR`. On Unix a worker leads a
    process group of its own, so a terminal's interrupt reaches the app; on Windows it starts with
    `CREATE_NO_WINDOW`. Every line is capped (1 MiB) before it's read whole; every page capped (1 GiB) and
    checked against `RawImage::data_len` before anything is allocated, then made through `RawImage::new`; the
    model's rules checked (a `dpi` finite and above 0; caps with a source, each with a format, resolutions
    above 0, a list not empty, a range's `min` not past its `max`, a largest paper finite and above 0; a batch
    in the model's order, its settings applied once and first with a paper finite and above 0 and each change
    said with what was asked, its sheets numbered on, each page of the sheet begun and a side once, encoded
    only in a form asked for, `FeederEmpty` only before pages), each listed or opened device's driver the
    worker's, an open's device the stored one or another said renamed from its id, each message in its place;
    any breach kills the worker. A number past an `f32`'s range reads as an infinity, so sizes are checked as
    they're read. A request is written only if its line is within the worker's 1 MiB and reads back from it
    equal (`Unsendable` otherwise, nothing sent). Standard error is drained by a thread, its last 8 KiB kept
    in `Stopped` and `Crashed`. `close` and drop send `quit` and give the grace (5 s), the input kept open
    meanwhile, then kill; a worker that can't be reaped even then is handed, with its directory, to a thread
    that waits for it, so the caller waits at most twice the grace and a moment. A listing returns once its
    worker has exited. On Unix the app keeps `SIGPIPE` ignored, as Rust's runtime leaves it (the docs say so);
    making a worker's end harmless whatever the app's disposition is FEAT-16. On Windows, the 64- and 32-bit workers' lists merged and each source used through
    its own bitness: slice 9's (`Driver::Twain { bits }` picks the worker already).
  - **The watchdog:** silence is counted from the last line read, and only while the caller's thread waits on
    the worker, so time spent handing an event to the caller's full channel is the caller's, never a hang:
    `start` (10 s) for the `hello`; `silence` (60 s, configurable, a flatbed's warm-up included) in a batch and
    for `caps`; once a batch is cancelled, `cancel` (10 s) counted from the later of the cancel and the last
    line, so the sheet in flight, front and back, finishes while it sends (the proposal's 10 s from the cancel
    alone could cut a slow sheet short: the seam note); a listing's or an open's deadline and the grace; the
    settings window never, nothing ending it from another thread yet (FEAT-17). A worker silent past its time
    is killed, `Stopped`, the pages so far kept. While a call waits on a silent worker, its process is asked
    after every 50 ms, so its end is seen even where a process its driver started holds its pipes: what the
    reader holds is taken, and its quiet 250 ms after is the output's end. The
    caller's cancel is read from its `Progress` every 50 ms and at each message and sent once; a caller's
    receiver gone is a cancel too, the rest of the batch read and dropped. A crash keeps the pages so far and
    says how the worker exited (on Unix an abort's `SIGABRT`, on Windows its `0xC0000409`), a page it cut
    short dropped and said.
  - **Hygiene** (NAPS2's practice, study only): the worker's standard output and error always drained (built).
    On Windows, workers in a Job object with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`, so none outlives the app,
    and a tree kill: FEAT-14, slice 9. Meanwhile a worker exits at the end of its input and aborts 10 s later
    if a driver's call hangs; a process a driver started is outside both. An optional spare worker kept warm:
    FEAT-15.
- **The test worker:** `scan-worker-fake`, a binary behind the `fake` feature (`required-features`, `test =
  false`, `doc = false`), built for the crate's own tests by a dev-dependency on the crate itself and found by
  `CARGO_BIN_EXE_scan-worker-fake`; a plain build leaves it out. Its first argument names a scenario: the
  fake's scripts through `serve` (a feeder, encodings, jams, a double feed, an open cover, gone, warming up, a
  settings window, a renamed device, not installed, a hang, a crash, an exit, a cancel awaited mid-page, a
  minute's listing, a slow exit), a backend that reports its environment as `Driver::Sane` (the SANE spawn
  tested without SANE), and workers that break the protocol by hand, one for each `ProtocolError` and, since
  the code review, for each of the model's rules the client checks, and two whose driver starts a process
  holding the worker's pipes (`held`, the test worker again) and then aborts or exits. Never for a shipped
  build: its scenarios hang, crash and exit by design.
- **Boundary:** no image codec, imageproc, PDF crate or GUI toolkit under it, and nothing links libsane (the
  client never loads it): the gates' devices loop reads its graph with every feature on (31 packages), and
  the libsane gate reads every `bin` target's executable since this slice, the test worker the first. Its
  dependencies, vetted in the slice's plan: `scan-device` (path, with `serde`; public, re-exported), `sane-dl`
  (path; `backend_of`, `ConfigDir`, `Search` and `CONFIG_DIR_VARIABLE` alone), `serde` and `serde_json` (the
  workspace's), `tempfile` 3 and `thiserror` (each the workspace's). `sane-dl`'s and `serde_json`'s errors stay out
  of its API. Weighed and not taken: `process-wrap` 10.0.1 (its `std` job object is made without
  kill-on-close, and a group kill serves nothing yet), `win32job` (slice 9's, with FEAT-14), `shared_child`,
  `wait-timeout` (a process-wide `SIGCHLD` handler), `command-group` (superseded), a binary format.
- **MSRV:** 1.89, the family's floor (`decisions.md`): the highest `rust-version` in its own graph (normal,
  build and dev, on every target; 31 packages) is `scan-device`'s and `sane-dl`'s 1.89, then `libloading`'s
  1.88. It takes let chains (a language feature, 1.88), and from `std` `BufRead::skip_until` (1.83) and, in its
  tests, `std::io::pipe` (1.87). CI tests it on 1.89 (`scan-worker-msrv`).
- **Tested by** (S0 slice 4; 109 since its code review: 47 unit tests, 59 integration tests through the test
  worker, 3 doctests):
  every message on a literal line; `serve`'s core over pipes with the fake (batches, refusals, cancel, the
  end of input, several backends); through a real worker process, listings and opens (renamed, `BadId`,
  `NotInstalled`, past their deadline), batches in each format, raw and encoded, every page equal to the
  in-process script's and the raw ones `pattern`'s byte for byte, each error ending a batch with the pages
  before it, the SANE spawn's directory (paths compared resolved), each `ProtocolError`; a hang `Stopped`
  with sheets 1 and 2 kept, a silent start, a caller slower than `silence` not stopped; a crash and an exit
  `Crashed` with their status and standard error; a page cut short; a cancel mid-page (the sheet finished,
  none fed after, the next batch giving the rest); a worker whose input ends exiting 0, or aborting in a
  hang no sooner than its 10 s; `close` and drop reaping; since the code review, a batch out of the model's
  order, caps and applied settings the model forbids, an open giving another device unsaid, another
  protocol's hello, a request past 1 MiB, a worker ending with its pipes held by a process its driver started
  (seen within 5 s, its status kept), and a close given a grace past the worker's own 10 s. The throughput share of M5 slice 6's speed gate,
  in a test binary of its own on every run: 24 duplex sheets of 300 dpi Letter through one worker, in color
  (about 25 MB a page raw) then black and white, the rate written on standard error and asserted at 2.3
  pages a second (about 58 MB/s). Here, in the debug build the tests run, about 29 pages a second in color
  (740 MB/s, the pipe the bound), 13 times the gate; 15.8 at the lowest, under outside load. On Windows and
  macOS: CI's run of the slice's pull request (TEST-16).
- **Stability:** provisional with `scan-device`'s model, whose types its protocol carries; frozen with it at
  0.1.0 in S0 slice 11, the protocol's version bumped once then. Its `serde` form and the three things it
  carries by name (a page's header, `Event`'s and `Pixels`' variants, a message's level), with fields added
  to the model's growing structs after data is stored: Q-8.
- **Open:**
  - On Linux, `saned -l -o` spawned on localhost as the worker, spoken to over SANE's public-domain network
    protocol with no FFI at all (a pure-Rust client exists as printscan's open PR #20). Untested: it needs
    `sane-utils` beside `libsane1`, and whether an unprivileged per-scan saned works on Debian and Ubuntu is
    unchecked. An idea, not the plan; kept.
  - The Job object, kill-on-close and a tree kill on Windows (FEAT-14), with the 32-bit worker
    (`i686-pc-windows-msvc`) and the merge of TWAIN's two lists: slice 9.
  - A spare worker kept warm (FEAT-15): it can't serve a SANE open, whose configuration is the device's.
  - Anything in a worker that writes to its standard output (a backend's `println!`, a driver's library)
    breaks the stream, which the client says as a protocol error; moving the protocol off standard output
    needs descriptors passed at the spawn. Slice 5 sees whether SANE's backends do.
  - How the packaged crate's tests get the test worker, once slice 11 makes it ready to publish (Q-5).
  - A worker's end harmless whatever the app's `SIGPIPE` disposition (FEAT-16): today the app keeps it ignored,
    as Rust's runtime leaves it.
  - A stop handle, to end a worker, its settings window above all, from another thread (FEAT-17): slice 9.
