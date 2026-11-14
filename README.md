# pdf-scan

Rust crates for scanning paper into PDF: driving scanners through each system's own driver API, holding and
cleaning up the page images they produce, reading the patch codes that separate documents, and writing
scanned pages to PDF and reading them back. Every crate is MIT OR Apache-2.0. Unsafe code sits only where a
driver's C API is called, in as few modules as it can; the PDF crates build on the
[pdf-lint](https://github.com/sheaf-pdf/pdf-lint) family.

Early days: the workspace exists, and both driver-binding crates, `sane-dl` and `twain-dsm`, are built and ready
to publish as `0.1.0-alpha.1` alphas; `scan-device` has its `fake`, `sane`, and `twain` backends, and
`scan-worker` drives them out of process on Unix and Windows (both 64-bit and 32-bit TWAIN workers). None is
published yet.

## Scanner access

| Crate | What it does |
|---|---|
| `sane-dl` | SANE, the scanner API of Linux and the other Unix-like systems, with libsane loaded at run time: its own bindings, a safe handle, typed options, frames assembled into images, a cancel token any thread can use. 0.1.0-alpha.1, not yet published ([README](crates/sane-dl/README.md)) |
| `fake-libsane` | A stand-in for libsane that the SANE crates' tests load (`sane-dl`'s, and those of `scan-device`'s `sane` backend, in process and through a worker): scripted devices, hostile values, SANE's calling rules checked (never published) |
| `twain-dsm` | TWAIN on Windows through the system's Data Source Manager, its API safe: the DSM loaded only after its signer is checked, its states as types the compiler checks, its bindings checked against `twain.h`, capabilities in all four containers, image layout (`DAT_IMAGELAYOUT`) and status (`DAT_STATUS`), native and memory transfers, owned settings windows, requests from other threads. 0.1.0-alpha.1, not yet published ([README](crates/twain-dsm/README.md)) |
| `scan-device` | A device model for scanners, one shape whatever driver API reaches them: devices listed and opened (a renamed one said), what each source can do, settings clamped and said, batches of pages on a bounded channel, cancelled between sheets through `in-progress`, pages in one raw layout checked as they're made; and behind features, `fake` (a scripted backend for tests), `sane` (SANE's scanners on Linux and other Unix-like systems through libsane loaded at run time), and `twain` (TWAIN's scanners on Windows in 32- and 64-bit through `twain-dsm`, memory and native transfers, capabilities and `DAT_IMAGELAYOUT`, condition codes, settings windows). Provisional and unpublished (0.0.0) ([README](crates/scan-device/README.md)) |
| `scan-worker` | Scanner drivers run in a process of their own: `serve`, a worker binary's whole `main` over any `scan-device` backends with its protocol output isolated from noisy driver `stdout` writes, speaking JSON lines on its standard input and output; and a `Client` that starts a worker for each listing and each open device (in a private directory, contained in a kill-on-close Job Object on Windows and its own process group on Unix), merges concurrent 64- and 32-bit TWAIN listings (`list_twain`), exposes a cross-thread `StopHandle` for hung driver dialogs, caps and checks every line and page before it's read, kills a worker silent past its time and keeps the pages sent before any failure. Provisional and unpublished (0.0.0) ([README](crates/scan-worker/README.md)) |

Next: the `sane` backend driven by hand through a real feeder scanner on Linux, and `scan-device`'s `wia` backend on Windows.

## Pages and PDF

Planned after scanner access: `scan-image` (a scanned page's image held compactly, and image files in and
out), `scan-cleanup` (scans and photos of paper pages made clean; blank pages), `patch-codes` (Kodak patch
codes read along any edge of a page, and drawn), and on the PDF side `pdf-images` (a PDF image's samples
decoded) and `pdf-scan` (scanned pages written to PDF with a searchable text layer, and read back).

The plan is in [`docs/pm/`](docs/pm/README.md).

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or [MIT license](LICENSE-MIT) at your
option. Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in the
work by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without any additional
terms or conditions.
