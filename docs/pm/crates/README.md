# The crates

One file per crate: what it does, its boundary, what it depends on, its prior art, how it's tested, and
what's open. Names are provisional (ask 6); all were free on crates.io on 2026-11-08. Where a crate starts
from sheaf's code, `sheaf/from-sheaf.md` has the files. The layers' graph and the gates that keep them
apart are in `plan.md` ("The family"). Paths in these files are from `docs/pm/`.

Prior art for every need is in `research/prior-art/README.md` (2026-11-08, "the survey"):
nothing published stands in for any crate here, and almost everything beneath them is used directly. Its
corrections are folded in below. Crates the survey found too young to depend on (printscan, `leptonica`,
anyd, aktujo-scan and others, most from 2026) are read and ported from with credit, and checked again at
the slice that would use them, never taken as dependencies.

## Devices

No pixel work beyond assembling what a driver sends into a page, and no PDF. Nothing here runs a driver
in the app's process unless the app asks: `scan-worker` is how sheaf uses them (M5 proposal 4, M5 ask 5).

- [`scan-device`](scan-device.md): the device model (DeviceInfo, Caps, Settings, RawPage, ScanError) and its
  backends: fake, sane, twain, wia, ica. Its design note, how each driver API meets the model:
  [`scan-device-seam.md`](scan-device-seam.md). Built in S0 slice 3, the model and its `fake`: 0.0.0,
  provisional, not published.
- [`sane-dl`](sane-dl.md): SANE loaded at run time, with its own bindings, typed options and frame assembly.
  Built in S0 slice 1: 0.1.0-alpha.1, ready to publish, not published.
- [`twain-dsm`](twain-dsm.md): TWAIN through the system's DSM, its seven states as types, in both bitnesses.
- [`scan-worker`](scan-worker.md): drivers in a process of their own: the JSON-lines protocol, the worker, the
  client and its watchdog. Built in S0 slice 4, on `scan-device`'s model and fake: 0.0.0, provisional, not
  published.

## Pages

Pixels only: no device, no PDF, no OCR engine, no GUI. This is `sheaf-image`'s boundary today ("Image in,
image out … never on lopdf, pdfium, an OCR engine or model, or gpui"), split three ways.

- [`scan-image`](scan-image.md): a scanned page held compactly, and image files in and out (TIFF, CCITT, JPEG,
  PNG).
- [`scan-cleanup`](scan-cleanup.md): scans and photos of paper made clean, and blank pages.
- [`patch-codes`](patch-codes.md): Kodak patch codes read along any edge, and drawn.

## PDF

On pdf-lint's family, as its consumer (`pdf-lint/README.md`): published alphas pinned exactly, the rest by
path until they publish, shortcomings proposed back (`pdf-lint/README.md`). The family keeps pixels out of
itself ("Image codecs (DCT, JPX, CCITT, JBIG2) stay outside the family: entry 33", M18 `crates.md`), so the
crate that decodes them is here (ask 5).

- [`pdf-images`](pdf-images.md): a PDF image's samples in its own color space, depth and /Decode.
- [`pdf-scan`](pdf-scan.md): scanned pages written to PDF with an invisible text layer, and read back.

## Tests

- [`scan-fixtures`](scan-fixtures.md): seeded pages, degradations, codes drawn, device scripts (never
  published).
- [`fake-libsane`](fake-libsane.md): a stand-in for libsane, a `cdylib` that `sane-dl`'s tests build and load
  as SANE through the loader's search path (ignored on Windows), scripted, checking SANE's calling rules
  (never published; built in S0 slice 1).
