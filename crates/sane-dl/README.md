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
    // A device's strings are SANE's bytes: Latin-1 by SANE's standard, UTF-8 from some backends.
    // Read as UTF-8 where they are, else as Latin-1.
    let text = |bytes: &[u8]| match std::str::from_utf8(bytes) {
        Ok(text) => text.to_owned(),
        Err(_) => bytes.iter().copied().map(char::from).collect(),
    };
    for device in sane.devices(false, Duration::from_secs(30))? {
        let vendor = text(device.vendor.as_bytes());
        let model = text(device.model.as_bytes());
        println!("{} ({vendor} {model})", text(device.name.as_bytes()));
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

- **Loading.** `Sane::load` opens the system's library: `libsane.so.1` on Linux and the other Unix-like
  systems, through the system loader's own search, and on macOS `libsane.1.dylib` at the paths Homebrew and
  MacPorts install it (`Sane::DEFAULT_LIBRARIES`). Windows has none, since SANE isn't its scanner API, nor
  have Apple's other systems.
  Another SANE is loaded through the loader's own search path, given to the process as it starts (below).
  Then `sane_init`, with no authorization callback, so a backend that asks for a password gets none.
- **Devices under a deadline.** `Sane::devices` lists them on a thread of its own, and stops waiting at the
  deadline: listing can take seconds, as every backend loaded looks for its devices, and some hang. Each
  device's name, vendor, model and kind come as SANE's bytes, never decoded: the standard makes them Latin-1,
  and some backends send UTF-8, so what they mean is the program's to say.
- **A known device, fast.** `Sane::open` opens a device by its name, with no listing first. `ConfigDir`
  writes a configuration that loads only the backends it names (`backend_of` gives a device's), for the
  process that loads SANE to be given as `SANE_CONFIG_DIR`, so a listing doesn't wait on every backend.
  The crate never sets an environment variable itself: the caller gives the value to the process it starts.
- **Options, typed.** A device's options are read as it opens, each descriptor's type, unit, size,
  capabilities and constraint, and read again whenever SANE says they changed, or before a call on an option
  SANE now describes otherwise, each time counted (`Handle::options_generation`). `get`, `set` and
  `set_auto` take and give a `Value`, and refuse a call SANE's rules don't allow (an inactive option, one
  software can't set, a value of another type or length, a string too long) before the backend sees it.
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

- `NotFound`: no library could be opened by the names tried, which it lists (SANE isn't installed where the
  system's loader looks);
- `NoDefault`: this system has no default library (Windows, and Apple's systems but macOS), so nothing is
  loaded;
- `MissingSymbol`: the library found under SANE's name lacks one of SANE's functions, so it isn't SANE;
- `Init`: `sane_init` answered a status other than `GOOD`;
- `UnsupportedVersion`: the library follows a SANE other than version 1 (it's ended before this returns);
- `AlreadyLoaded`: SANE is already loaded in this process by this copy of the crate;
- `SlotTaken`: on Linux, the process's slot for SANE is held by someone other than this copy: another copy of
  the crate with SANE loaded, a child the process forked, or another process (below);
- `SlotRefused`: on Linux, the system refused the socket that keeps SANE to one per process (a sandbox that
  forbids Unix sockets or binding its name, or no file descriptor left), so nothing was loaded.

There's one `Sane` per process, since SANE is started once per process; once it and every handle opened from
it are gone, SANE is ended (`sane_exit`) and another may load. A load that fails leaves nothing behind. The
library is never unloaded, since backends may leave threads or exit handlers in it.

Loading a library runs its code in this process, and its functions are called as `sane.h` declares them.
`load` is safe because no code in the process chooses the library, this crate's or its caller's: it's the one
the system's loader finds under SANE's name, by the search the process started with, taken as SANE as a
library linked at build time would be, and never closed. On macOS that's at its installed paths, never by a
bare name, which dyld would look for in the working directory too; Apple's other systems have no default.

To load another SANE, one a program ships or a build of one's own, give the process that loads it the
loader's search path as it starts: `LD_LIBRARY_PATH` on Linux and the other Unix-like systems,
`DYLD_LIBRARY_PATH` on macOS (where dyld looks there for the file name of each installed path first). Name a
directory that holds SANE's own libraries alone, since the variable reaches every library the process loads
by name. Or build a run path into the binary or shared library that holds this crate, whose code calls the
loader (`$ORIGIN/…` on Linux, from that file's directory). So a program that runs SANE in a worker process
sets the variable on the worker's command, as it sets `SANE_CONFIG_DIR`. The loaders ignore these variables
in a set-id process, and dyld in a process it restricts, such as one under the hardened runtime.

That rests on the process's environment, as every library a process loads by name does, and holds while it
keeps to two rules, its starter's and the program's:

- **Every entry of the search path is absolute,** in the variable and in any run path built in. An empty
  entry (a leading, trailing or doubled `:`, as `LD_LIBRARY_PATH=/opt/sane/lib:$LD_LIBRARY_PATH` leaves where
  the variable was unset) means the working directory, and a relative entry is looked for from it, as the
  library loads; the program's own code may change its working directory, and so would choose the library.
  The C library's own lookups, which std uses, are open to such entries alike.
- **Nothing in the process changes `LD_LIBRARY_PATH`, `SHLIB_PATH` or `LIBPATH` while SANE is in use.** The
  system's loader reads the first as the process starts, so for SANE itself a change comes too late. But
  SANE's `dll`, the part of SANE that loads its backends, reads the first of the three that's set each time it
  loads one, at a listing or an open, and looks there first for the backend's library: a value set inside the
  process would choose libraries that then run in it.

In a set-id process, whose starter isn't trusted, the loader ignores its variables and glibc clears
`LD_LIBRARY_PATH`, but `dll` then reads `SHLIB_PATH` and `LIBPATH`, which nothing clears: so a set-id program
runs SANE in a process it starts with an environment of its own choosing, never in itself.

That holds across copies of this crate as far as each system lets it be seen. Its manifest's `links` key
makes cargo refuse two versions of it in one build, but every shared library built with it holds a copy of
its own, so a process that loads two such libraries holds two copies, each of which would start SANE as if
it were alone. On Linux every copy takes one slot for the whole process before the library opens: a Unix
datagram socket bound to the name `sane-dl/slot/pidns/<device>/<inode>/pid/<pid>` in the abstract socket
namespace, from the device and inode of the process's pid namespace (`/proc/self/ns/pid`, or `none` in their
place where it can't be read) and its pid. The name is fixed for good, so every version of the crate finds
every other's, and the system frees it when SANE has ended or the process ends. While someone other than this
copy holds it, a load is `SlotTaken`: a second copy's load, or one while a child the process forked still
holds it (until the child runs a program or ends), or after another process bound the name first. A child
spawned on any thread holds it from its fork until its program starts, a moment a load waits out, trying
again for a tenth of a second. Where the system refuses the socket the load is `SlotRefused`, never SANE
unguarded; a sandbox's profile that lets the process bind abstract names under `sane-dl/slot/` (AppArmor's
`@sane-dl/slot/**`) lets it load. The slot can't see across a change, between two copies' loads, of the
process's network namespace (where the name lives) or of its view of `/proc` (which the name is formed from),
so a program that makes one keeps to one copy. On every system but Linux a copy sees only its own loads, so
a program there keeps to one copy of this crate in a process. And the slot guards this crate's copies alone:
another user of libsane in the process, such as a library linked to it, shares the library this crate opens,
and is the program's to rule out.

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
  The flag stays until a call returns `Cancelled` for it, so a cancel between pages stops the next page;
  `Handle::clear_cancel` takes back one that came as a batch ended, before the next batch.
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
after. What passes a cap is `Error::LimitExceeded`, saying which (`LimitKind`), what SANE gave and the cap,
so a program can raise the cap or ask for less, as a lower resolution. Beyond the caps, what makes no sense
is refused with what SANE said: parameters no frame can have
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

The crate builds on every system `libloading` does, and needs nothing of SANE's to build: no header and no
library (its build script builds nothing, there for the `links` key). On Linux it loads the distribution's
libsane (`libsane1` on Debian and Ubuntu). On Windows, and Apple's systems but macOS, `Sane::load` is
`NoDefault`; `select_fd` exists on Unix only. Its tests, in the repository, run
against SANE's own `test` and `pnm` backends and a stand-in library of their own, and check the bindings
against the installed `sane.h`; on macOS, Homebrew's sane-backends hasn't been tried.

## License

Licensed under either of Apache License, Version 2.0
([LICENSE-APACHE](https://github.com/sheaf-pdf/pdf-scan/blob/master/LICENSE-APACHE)) or MIT license
([LICENSE-MIT](https://github.com/sheaf-pdf/pdf-scan/blob/master/LICENSE-MIT)) at your option.

The bindings in `src/ffi.rs` are written from SANE's `sane.h`, which is in the public domain: they keep its
notice, and say what was changed. libsane itself is no part of this crate: it's the system's, loaded at run
time, never linked.
