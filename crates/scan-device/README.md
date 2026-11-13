# scan-device

A device model for document scanners in Rust: one shape, whatever driver API reaches them.

A `Backend` lists the devices its driver API finds and opens the one an app stored, saying when it opened
the same scanner under a new name; the `Device` it opens says what each of its sources can do, scans a batch
with the settings it's given, clamping them and saying what it changed, and opens its driver's own settings
window where it has one. A batch sends its events (the settings applied, each sheet, each page, the scanner
warming up) on a bounded channel, so a backend waits while nobody takes its pages, and is cancelled through
an [`in-progress`](https://crates.io/crates/in-progress) tracker between sheets, so the sheet in flight is
finished, front and back. Pages come raw, in one stated layout that `RawImage::new` checks (its length, sizes
in checked arithmetic against what one buffer can hold, the padding bits of black and white rows), or encoded
in a form the app asked for. The types are plain data, so they can cross a pipe to a process that runs the
drivers, encoded through the `serde` feature.

Each driver API (SANE, TWAIN, WIA, ...) is a backend behind the same two traits, each behind a feature of its
own. This version has the model, a scripted backend for tests, SANE's scanners on Unix-like systems, and
TWAIN's scanners on Windows (32- and 64-bit).

Provisional: 0.0.x and unpublished, while the model is tried against each driver API; it's frozen at 0.1.0
once the Windows backends are complete. No `unsafe` code.

## Features

None is on by default.

- `fake`: `scan_device::fake`, a scripted backend for tests, on every system and with no driver. Its devices
  list and open as a script says, a stored one opened as a backend for a driver API would open it (a renamed
  one found by its maker and model, a name now another model's taken as gone), and its batches take sheets
  from a feeder or the glass, clamp their settings and say what they changed, and draw their pages at those
  settings, the same bytes on every system. A sheet's side can be scripted to fault as it's read: a driver's
  error (a jam, a double feed, an open cover, the device gone, ...), the sheet then waiting at the head of the
  feeder to be scanned again, or a hang, a crash or an exit; or the side can wait for the batch's cancel, so a
  test puts a cancel mid-sheet by its script rather than by timing. For tests only, never for a shipped build:
  its faults can block a thread for good and end the process.
- `serde`: `Serialize` and `Deserialize` for the model's plain data (devices and their ids, what a device can
  do, the settings asked and applied, formats and encodings, a page's side, a batch's status and how it ended,
  the errors, the settings window's owner and how it closed), so it crosses a pipe to a process that runs the
  drivers and an app can store a device to open again later. In serde's default form: Rust's names, an enum
  tagged by its variant's name. Not a page or an event, which may carry one: a page's bytes travel beside the
  text, not in it. Provisional, as the model is.
- `sane`: `scan_device::sane::SaneBackend`, SANE's scanners on Linux and the other Unix-like systems, through
  libsane loaded at run time and never linked, so a program built with it runs where SANE isn't installed and
  hears so when it lists or opens a device (as on Windows). A device's id is `sane:` and SANE's name for it,
  each byte the character of its number, so it goes back to SANE byte for byte; its name, maker and model are
  read as UTF-8 where they are, else as Latin-1. A stored device is opened after a listing of its backend:
  a name now another model's is taken as gone, a USB connection's name that a replugging changed is found by
  its maker and model, and ids no listing gives (those SANE would read as its testing modes, or hand to a
  backend the id never named) are refused before SANE sees them. What each source can do is read through
  tables of the names backends give a feeder, duplex and a scan mode, each with where it's seen. A batch sets
  its settings in order, each clamped to what the device allows as it's set, then reads them back, so what
  the device moved is said as applied; its pages come in the raw layout (black and white inverted, 16-bit
  samples little-endian, 1-bit color expanded), each sheet's event with its front, since SANE says a sheet is
  there only by giving its page. Options the model doesn't carry are set by their SANE names, as the app
  chooses (`with_option`), at each open and each batch. SANE's backends are C code that runs in the process
  that loads them, so an app runs this one in a worker process of its own.
- `twain`: `scan_device::twain::TwainBackend`, TWAIN's scanners on Windows in both 64-bit (`twain64:`) and
  32-bit (`twain32:`), through `twain-dsm` and `TWAINDSM.dll` loaded at run time after its Authenticode
  signature is verified (off Windows, listing and opening report `ScanError::NotInstalled`). Drives a lazy
  process-wide STA session; auto-selects TWAIN 2 callbacks (`DF_DS2`) or TWAIN 1.x window-message delivery;
  negotiates units, sources, duplex, resolutions, formats, sticky processing resets, and mutual exclusion
  between `Paper::Auto` border detection and `Paper::Size` `DAT_IMAGELAYOUT` frames; assembles memory strips,
  tiles, undefined-size transfers (`width` or `length` `-1`), and native DIBs into the checked raw layout;
  reads `TWEI_PAGESIDE`, maps condition codes and unexpected `CloseRequest` statuses to `ScanError`, cancels
  between sheets, and opens the driver's own settings window (`settings_window`). TWAIN data sources are
  in-process `.ds` DLLs of their own bitness, so a 64-bit app drives both 64-bit and 32-bit TWAIN workers
  through `scan-worker`.

## License

Licensed under either of Apache License, Version 2.0
([LICENSE-APACHE](https://github.com/sheaf-pdf/pdf-scan/blob/master/LICENSE-APACHE)) or MIT license
([LICENSE-MIT](https://github.com/sheaf-pdf/pdf-scan/blob/master/LICENSE-MIT)) at your option.
