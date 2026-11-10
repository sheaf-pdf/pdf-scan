# scan-worker

Scanner drivers run in a process of their own, in Rust: a worker serving any `scan-device` backends over its
standard input and output, and the client that starts one for each device, watches it and reads its pages.

A scanner's driver is code its maker wrote: one that hangs holds the thread that called it, one that crashes
takes its process with it, and a 32-bit driver loads only into a 32-bit process. Run in a worker process, a
driver can do any of that while the app goes on, keeping the pages the worker sent before it failed.

`serve(backends)` is the whole `main` of a worker binary. It reads requests (list the devices, open the one
stored, its capabilities, a batch, the driver's settings window, cancel, quit) as JSON lines on its standard
input, makes every call on its main thread in order, and writes the answers and a batch's events (the settings
applied, each sheet, each page, its progress and the driver's messages) on its standard output, each page's
bytes after its line in the device model's raw layout. A batch's events pass through a channel that holds
none, so a slow pipe holds the driver back; a cancel is taken at once, and finishes the sheet in flight; a
request out of turn, unreadable or past its 1 MiB cap is refused and the worker goes on; the end of its input
ends it, and aborts it if a driver's call never returns. The crate's docs give the protocol whole, so another
implementation could speak it; its first line is a `hello` with the protocol's version, and a mismatch is an
error, not a negotiation.

A `Client`, given the worker binary for each driver API, starts a worker for each listing and each device it
opens, and makes the device model's calls through it, with an error of its own beside the device's. Each worker
starts in a private directory of its own, where a SANE worker opening a stored device finds a configuration
naming only that device's backend, and inherits the app's environment with its command's own variables set
after it (the loader's search path that chooses the SANE library it loads, say). The client trusts nothing a worker says: every line and page is capped
before it's read, a page checked against its format and size before anything is allocated, and the model's
rules checked, a batch's order among them. A watchdog kills a worker silent past its time, a cancel allowed to
finish the sheet in flight, and a silent worker's process is watched, so its end is seen even where a process
its driver started holds its pipes; the pages sent before any failure are the caller's, and a crash says how the
worker exited and keeps the end of its standard error. On Unix, an app keeps `SIGPIPE` ignored, as Rust's
runtime leaves it.

## Features

- `fake`: `scan-worker-fake`, a test worker serving `scan-device`'s scripted backend, its SANE backend (the
  scenario this crate's tests run SANE's own `test` backend and a stand-in for libsane through), and workers
  that break the protocol on purpose, for this crate's tests. Never for a shipped build: its scenarios hang,
  crash and exit by design. It turns on `scan-device`'s own `fake` and `sane`, which then appear through the
  re-exported `scan_worker::scan_device`; the rest of the library is the same with it or without.

Provisional: 0.0.x and unpublished, with the device model whose types its protocol carries; both are frozen
together. No `unsafe` code.

## License

Licensed under either of Apache License, Version 2.0
([LICENSE-APACHE](https://github.com/sheaf-pdf/pdf-scan/blob/master/LICENSE-APACHE)) or MIT license
([LICENSE-MIT](https://github.com/sheaf-pdf/pdf-scan/blob/master/LICENSE-MIT)) at your option.
