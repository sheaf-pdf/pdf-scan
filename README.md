# pdf-scan

Rust crates for scanning paper into PDF: driving scanners through each system's own driver API, holding and
cleaning up the page images they produce, reading the patch codes that separate documents, and writing
scanned pages to PDF and reading them back. Every crate is MIT OR Apache-2.0. Unsafe code sits only where a
driver's C API is called, in as few modules as it can; the PDF crates build on the
[pdf-lint](https://github.com/sheaf-pdf/pdf-lint) family.

Early days: the workspace exists and its first crate is being built. None is published yet.

## Scanner access

| Crate | What it does |
|---|---|
| `sane-dl` | SANE, the scanner API of Linux and the other Unix-like systems, with libsane loaded at run time: its own bindings, a safe handle, typed options, frames assembled into images, a cancel token any thread can use. Being built |
| `fake-libsane` | A stand-in for libsane that `sane-dl`'s tests load: scripted devices, hostile values, SANE's calling rules checked (never published) |
| `twain-spike` | A throwaway program that drives TWAIN on Windows through the TWAIN Working Group's sample data source, in both bitnesses, and checks what it sees; replaced by `twain-dsm`'s own tests (never published) |

Next: `scan-device`, a device model over each system's driver API (SANE, then TWAIN and WIA) with a fake for
tests; `scan-worker`, which runs drivers in a process of their own; and `twain-dsm`, TWAIN through the
system's Data Source Manager loaded at run time.

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
