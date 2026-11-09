# sane-dl

SANE scanner access for Rust, with libsane loaded at run time.

[SANE](http://www.sane-project.org/) is the scanner API of Linux and the other Unix-like systems. This crate
opens libsane only when asked to, never at build time or start-up, so a program that takes it builds and runs
where SANE isn't installed, and hears so as a typed error when it tries to scan. It keeps its own bindings
(`sane_dl::ffi`), written from SANE's public-domain `sane.h`, and checks every integer, pointer and length
that crosses from C before it becomes a Rust value.

It's SANE alone: no device model and no policy. It says what SANE says, each status with the function that
answered and each frame's parameters as SANE gave them, so the program above decides what they mean.

This is an alpha: any alpha of 0.1.0 may change the API, its changelog saying what changed. Depend on it by
the exact version, `sane-dl = "=0.1.0-alpha.1"`, since cargo's default takes any later alpha too.

## An example

```rust,no_run
use std::time::Duration;

use sane_dl::{Page, Sane};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let sane = Sane::load()?;
    for device in sane.devices(false, Duration::from_secs(30))? {
        println!("{} ({} {})", device.name, device.vendor, device.model);
    }
    let mut handle = sane.open("test:0")?;
    if let Some(mode) = handle.find_option("mode") {
        let mode = mode.index;
        println!("mode: {:?}", handle.get(mode)?);
    }
    // A cancel button on another thread would hold a clone of this.
    let _token = handle.cancel_token();
    let mut batch = handle.batch();
    loop {
        match batch.next_page(|progress| println!("{progress:?}"))? {
            Page::Image(image) => println!("{} by {}, depth {}", image.width, image.height, image.depth),
            Page::NoDocs { pages, .. } => {
                println!("{pages} pages");
                break;
            }
        }
    }
    drop(batch);
    handle.close();
    Ok(())
}
```

## What it does

- **Loading.** `Sane::load` opens the system's library by its versioned name through the system loader's own
  search: `libsane.so.1` on Linux and the other Unix-like systems, `libsane.1.dylib` on macOS. Windows has
  none, since SANE isn't its scanner API. `Sane::load_from` opens the library at a path the caller gives.
  Then `sane_init`, with no authorization callback, so a backend that asks for a password gets none.
- **Devices under a deadline.** `Sane::devices` lists them on a thread of its own, and stops waiting at the
  deadline: listing can take seconds, as every backend loaded looks for its devices, and some hang.
- **A known device, fast.** `Sane::open` opens a device by its name, with no listing first. `ConfigDir`
  writes a configuration that loads only the backends it names (`backend_of` gives a device's), for the
  process that loads SANE to be given as `SANE_CONFIG_DIR`, so a listing doesn't wait on every backend.
  The crate never sets an environment variable itself: the caller gives the value to the process it starts.
- **Options, typed.** A device's options are read as it opens, each descriptor's type, unit, size,
  capabilities and constraint, and read again whenever SANE says they changed. `get`, `set` and `set_auto`
  take and give a `Value`, and refuse a call SANE's rules don't allow (an inactive option, one software
  can't set, a value of another type or length, a string too long) before the backend sees it.
- **Scanning, step by step** (`start`, `parameters`, `read`, `set_io_mode`, `select_fd` on Unix, `cancel`),
  or **a page at a time:** `Handle::batch` gives a `Batch`, whose `next_page` reads every frame of a page to
  its end and assembles them into an `Image`, or says the feeder is empty (`Page::NoDocs`, from either call
  that can say so). Rows come top down with any padding dropped, a three-pass image's frames are interleaved
  into one RGB image, and the samples are as SANE encodes them: at depth 1, eight pixels a byte with the
  leftmost in the high bit, and in a gray image 1 is black; at depth 16, two bytes a sample in this machine's
  byte order. A status mid-page, such as a jam or an open cover, is an error, and the page is dropped. A
  page reads blocking, and a read that gives no bytes is made again after a pause of up to 10 ms, until
  the backend gives bytes or a status, or a token cancels.

## Loading, and what can fail

Loading happens only when asked for, and fails with a `LoadError`:

- `NotFound`: no library could be opened by the names tried, which it lists (SANE isn't installed, or the
  path is wrong);
- `NoDefault`: this system has no default library (Windows); `load_from` still loads one by its path;
- `MissingSymbol`: the library lacks one of SANE's functions, so it isn't SANE;
- `Init`: `sane_init` answered a status other than `GOOD`;
- `UnsupportedVersion`: the library follows a SANE other than version 1 (it's ended before this returns);
- `AlreadyLoaded`: SANE is already loaded in this process.

There's one `Sane` per process, since SANE is started once per process; once it and every handle opened from
it are gone, SANE is ended (`sane_exit`) and another may load. A load that fails leaves nothing behind. The
library is never unloaded, since backends may leave threads or exit handlers in it.

Loading a library runs its code in this process, so the path given to `load_from` must name a library
trusted as much as the program itself, never one an untrusted party can write.

## Threads, and cancelling

- **One call at a time.** SANE lets no call overlap another but `sane_cancel`, so every call through a
  `Sane` and its handles takes one lock, whatever thread it comes from and whichever device it's for. `Sane`
  is `Send` and `Sync`, and its clones share it; calls from several threads wait on each other.
- **A handle is used from one thread at a time:** `Handle` is `Send`, not `Sync`. It closes its device when
  dropped or closed.
- **A listing past its deadline** returns `DeadlineExceeded` and keeps the library until it returns:
  meanwhile every call returns `ListingPending` rather than block, and a handle dropped then is closed by the
  listing's thread once it returns, so a drop never blocks on a backend that hangs.
- **Cancel from any thread.** `Handle::cancel_token` gives a `CancelToken` (`Clone`, `Send`, `Sync`), whose
  `cancel` calls `sane_cancel` at once, while the handle's own thread may be blocked in a read. It never
  reaches a closed handle: a cancel after the close does nothing, and one in flight finishes before the
  close begins. It also flags the cancel, so a backend that ignores `sane_cancel` still stops being read:
  the next start or read returns `Error::Cancelled`, and so does a read in flight when it returns.
  The flag stays until a call returns `Cancelled` for it, so a cancel between pages stops the next page.
- **Stopping after the page in flight** is the caller's: start no next page, and drop the batch. A batch
  ends with `sane_cancel` however it ends, as SANE asks, and `sane_cancel` follows every error status from
  a start, the parameters or a read, as SANE's frontends do.
- **Progress** is told between reads with no lock held, so the callback may use the cancel token.

## Caps on what SANE gives

A backend is C code that may return anything. `Limits` caps what this crate accepts, each checked before
the work it caps, so a malformed or hostile answer is an error, never an unbounded allocation or read. The
defaults are far above what a real backend gives:

| Cap | Default |
|---|---|
| Devices in a list | 1,024 |
| Bytes in a string SANE gives (a name, a title, a status's words, a list's entry) | 64 KiB |
| Options a device has | 4,096 |
| Entries in an option's word list or string list | 65,536 |
| Bytes in an option's value | 1 MiB |
| Bytes in a frame (a 1200 dpi 16-bit color Letter page is about 0.75 GiB) | 1 GiB |
| Bytes in a frame's line | 16 MiB |
| Bytes asked for in one read | 256 KiB |

Change them on a copy of `sane.limits()`, given back with `sane.set_limits`, for the calls that start
after. Beyond the caps, what makes no sense is refused with what SANE said: parameters no frame can have
(`BadParameters`: a reserved frame type, a depth other than 1, 8 or 16, too few bytes a line, a gray frame
that isn't the last), frames that end early or run late (`ShortFrame`, `LongFrame`, `PartialRow`), a read
length past what was asked (`BadReadLength`), and descriptors SANE doesn't allow, which make their option
unusable rather than read.

## Run SANE in a process of its own

Backends are C code running in the process that loads them. This crate checks what crosses from them, but
it can't contain one that corrupts memory or crashes, and some do; some hang, too, past any cancel, and only
ending the process stops them. SANE is also started once per process and reads its configuration as it
starts, and a listing and a scan wait on each other. So load SANE in a worker process of its own, under a
watchdog that ends it when it stops answering, and list devices there rather than beside a scan.

SANE opens any installed backend whose name a device's name starts with, whatever its configuration lists,
so the name a worker opens should come from a listing or the user's own choice, never from an untrusted
party.

## Platforms

The crate builds on every system `libloading` does, and needs nothing of SANE's to build: no header, no
library, no build script. On Linux it loads the distribution's libsane (`libsane1` on Debian and Ubuntu). On
Windows `Sane::load` is `NoDefault`; `select_fd` exists on Unix only. Its tests, in the repository, run
against SANE's own `test` and `pnm` backends and a stand-in library of their own, and check the bindings
against the installed `sane.h`; on macOS, Homebrew's sane-backends hasn't been tried.

## License

Licensed under either of Apache License, Version 2.0
([LICENSE-APACHE](https://github.com/sheaf-pdf/pdf-scan/blob/master/LICENSE-APACHE)) or MIT license
([LICENSE-MIT](https://github.com/sheaf-pdf/pdf-scan/blob/master/LICENSE-MIT)) at your option.

The bindings in `src/ffi.rs` are written from SANE's `sane.h`, which is in the public domain: they keep its
notice, and say what was changed. libsane itself is no part of this crate: it's the system's, loaded at run
time, never linked.
