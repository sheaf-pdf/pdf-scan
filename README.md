# pdf-scan

Rust crates for scanning paper into PDF: driving scanners through each system's own driver API, holding and
cleaning up the page images they produce, reading the patch codes that separate documents, and writing
scanned pages to PDF and reading them back. Every crate is MIT OR Apache-2.0. Unsafe code sits only where a
driver's C API is called, in as few modules as it can; the PDF crates build on the
[pdf-lint](https://github.com/sheaf-pdf/pdf-lint) family.

Early days: the workspace exists, and its first crate, `sane-dl`, is built and ready to publish as an alpha;
`twain-dsm` is built too, kept unpublished until its first user, the `twain` backend, has tried its API. None is
published yet.

## Scanner access

| Crate | What it does |
|---|---|
| `sane-dl` | SANE, the scanner API of Linux and the other Unix-like systems, with libsane loaded at run time: its own bindings, a safe handle, typed options, frames assembled into images, a cancel token any thread can use. 0.1.0-alpha.1, not yet published ([README](crates/sane-dl/README.md)) |
| `fake-libsane` | A stand-in for libsane that `sane-dl`'s tests load: scripted devices, hostile values, SANE's calling rules checked (never published) |
| `twain-dsm` | TWAIN on Windows through the system's Data Source Manager, its API safe: the DSM loaded only after its signer is checked, its states as types the compiler checks, its bindings checked against `twain.h`, capabilities in all four containers, native and memory transfers, requests from other threads. Built; 0.0.0, kept unpublished until the `twain` backend (S0 slice 9) has used its API |

Next: `scan-device`, a device model over each system's driver API (SANE, then TWAIN and WIA) with a fake for
tests; and `scan-worker`, which runs drivers in a process of their own.

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
