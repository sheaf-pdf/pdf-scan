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
drivers.

Each driver API (SANE, TWAIN, WIA, ...) is a backend behind the same two traits, each behind a feature of its
own. This version has the model, and a scripted backend for tests; none for a driver API yet.

Provisional: 0.0.x and unpublished, while the model is tried against each driver API; it's frozen at 0.1.0
once a second one has been through it. No `unsafe` code.

## Features

None is on by default.

- `fake`: `scan_device::fake`, a scripted backend for tests, on every system and with no driver. Its devices
  list and open as a script says, a stored one opened as a backend for a driver API would open it (a renamed
  one found by its maker and model, a name now another model's taken as gone), and its batches take sheets
  from a feeder or the glass, clamp their settings and say what they changed, and draw their pages at those
  settings, the same bytes on every system. A sheet's side can be scripted to fault as it's read: a driver's
  error (a jam, a double feed, an open cover, the device gone, ...), the sheet then waiting at the head of the
  feeder to be scanned again, or a hang, a crash or an exit. For tests only, never for a shipped build: its
  faults can block a thread for good and end the process.

## License

Licensed under either of Apache License, Version 2.0
([LICENSE-APACHE](https://github.com/sheaf-pdf/pdf-scan/blob/master/LICENSE-APACHE)) or MIT license
([LICENSE-MIT](https://github.com/sheaf-pdf/pdf-scan/blob/master/LICENSE-MIT)) at your option.
