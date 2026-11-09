# scan-worker

Layer: Devices (`crates/README.md`). Paths are from `docs/pm/`.

- **What:** drivers in a process of their own (proposal 4). Everyone who drives 32-bit TWAIN from a 64-bit
  app does it this way (NAPS2's workers, VintaSoft's service, LEADTOOLS' thunk server, RSWebTWAIN's
  sidecar), and nobody publishes the broker as a library: this crate fills that hole.
  - **The protocol:** requests in as JSON lines (`list`, `caps`, `scan`, `cancel`, `quit`), events out
    (`device`, `caps`, `sheet`, `page`, `status`, `error`, `done`), each `page` followed by its bytes; a
    `hello` line with a version first, and a mismatch is an error, not a negotiation (worker and app ship
    together). Message and page sizes capped before they're read.
  - **The worker's side:** `serve(backends)`, the whole `main` of a worker binary over any `scan-device`
    backends. The binaries themselves (`sheaf-scan-worker`, `sheaf-scan-worker-x86.exe`) are the app's.
  - **The client:** starts a worker it's given the path of; one driver per worker, kept while a dialog is
    open; `list` in a short-lived worker of its own with a deadline, so a slow probe never holds a scan; on
    Windows, the 64- and 32-bit workers' lists merged and each source used through its own bitness.
  - **The watchdog:** a scan that sends nothing for 60 seconds (configurable; a flatbed's warm-up included)
    is ended, the worker killed, and the pages so far kept; a cancel is a message, and a worker that hasn't
    stopped 10 seconds later is killed; a crash keeps the pages so far and says how the worker exited.
  - **Hygiene** (NAPS2's practice, study only): on Windows, workers in a Job object with
    `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`, so none outlives the app; the worker's stdout and stderr always
    drained (undrained pipes stall a worker; NAPS2 has a TODO for it); an optional spare worker kept warm.
- **Stability:** provisional with `scan-device`'s model, whose types its protocol carries; frozen with it at
  0.1.0 in S4, the protocol's version bumped once then.
- **Tested by:** the `fake` backend through a real worker process on every runner: a hang killed with pages
  kept, a crash with pages kept, a cancel mid-page, a protocol mismatch, a truncated page. The throughput
  share of M5 slice 6's speed gate: 300 dpi color pages (about 25 MB raw) at 2.3 a second through the pipe,
  about 58 MB/s.
- **Open:** on Linux, `saned -l -o` spawned on localhost as the worker, spoken to over SANE's public-domain
  network protocol with no FFI at all (a pure-Rust client exists as printscan's open PR #20). Untested: it
  needs `sane-utils` beside `libsane1`, and whether an unprivileged per-scan saned works on Debian and
  Ubuntu is unchecked. An idea, not the plan.
