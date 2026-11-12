# twain-dsm

TWAIN scanner access for Rust on Windows, through the system's Data Source Manager (`TWAINDSM.dll`) loaded at run time.

[TWAIN](https://twain.org/) is the classic scanner API of Windows: an application opens a connection to the Data Source Manager, which loads each scanner's driver (`.ds`) and routes triplets (`DG_*` / `DAT_*` / `MSG_*`) to it. This crate's API is safe Rust: TWAIN's states are owned types checked by the compiler, capability containers and extended image info blocks are decoded into owned values with every length checked first, and the `unsafe` code that calls Win32 and the DSM stays inside the crate.

It is TWAIN alone: no cross-platform device model and no policy. Capabilities, image info, native DIB blocks and memory transfer strips come as TWAIN gives them, so the caller above decides how to interpret and assemble them.

This is an alpha: any alpha of 0.1.0 may change the API, its changelog saying what changed. Depend on it by the exact version, `twain-dsm = "=0.1.0-alpha.1"`, since cargo's default takes any later alpha too.

## An example

```rust,no_run
use std::time::Duration;
use twain_dsm::{App, Delivery, Item, Load, Next, Pending, Ui, ffi};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let report = twain_dsm::run::<(), _>(|twain| -> Result<usize, twain_dsm::Error> {
        let opened = twain
            .load(Load::System)?
            .open(App::new("Example", "Scanning", "Example")?)
            .map_err(|r| r.into_error())?;
        let id = opened
            .sources()?
            .into_iter()
            .next()
            .ok_or(twain_dsm::Error::NoSuchSource)?;
        let delivery = if id.is_twain2() {
            Delivery::Callback
        } else {
            Delivery::Events
        };
        let source = opened
            .open_source(&id, delivery)
            .map_err(|r| r.into_error())?;
        source.set(ffi::ICAP_XFERMECH, Item::UInt16(ffi::TWSX_MEMORY))?;
        let mut enabled = source.enable(Ui::Hidden).map_err(|r| r.into_error())?;
        let mut ready = loop {
            match enabled
                .next_event(Duration::from_secs(30))
                .map_err(|r| r.into_error())?
            {
                Next::Ready(ready) => break ready,
                Next::Event(back, _)
                | Next::Request(back, _)
                | Next::Timeout(back)
                | Next::Quit(back, _) => enabled = back,
            }
        };
        let mut pages = 0;
        loop {
            let mut transfer = ready.memory().map_err(|r| r.into_error())?;
            let mut buffer = vec![0; transfer.preferred_size()];
            while let Some(strip) = transfer.next_buffer(&mut buffer)? {
                let _ = (strip.x_offset, strip.y_offset, strip.bytes.len());
            }
            pages += 1;
            match transfer.end().map_err(|r| r.into_error())? {
                Pending::More(next, _) => ready = next,
                Pending::Done(_) => return Ok(pages),
            }
        }
    })?;
    let pages = report.value?;
    println!("transferred {pages} pages");
    Ok(())
}
```

## What it does

- **Scoped TWAIN thread (`run`).** Every DSM call must come from one thread in a single-threaded COM apartment with a top-level parent window and message loop. `twain_dsm::run` sets up the apartment and a hidden top-level window on the calling thread, passes `&Twain<T>` to a closure, and tears down whatever the closure left open—even forgotten states—before returning. States borrow `&Twain<T>` (`!Send`, `!Sync`), so none can leave the closure or cross threads; other threads post requests of type `T` through a `Remote<T>`.
- **Verified loading (`Twain::load`).** Loads `TWAINDSM.dll` from `GetSystemDirectoryW` (`Load::System`, `SysWOW64` in 32-bit processes) or an explicit path (`Load::Path`), holding the file open against writes and deletes while checking its PE machine, offline Authenticode signature, and signer (`O=TWAIN Working Group`) before `LoadLibraryExW` runs, and verifying the loaded module matches the checked file. Once loaded, the DSM stays loaded for the life of the process. One `run` at a time per process across copies of the crate, coordinated by a process-named semaphore.
- **TWAIN's states as types.** `Loaded` (state 2), `Opened` (state 3), `Source` (state 4), `Enabled` (state 5), `Ready` (state 6), `Transferred` (state 7) and `MemoryTransfer` (states 6–7). Each step up consumes the previous state and hands it back inside `Refused<S>` if TWAIN refuses; dropping any state steps down from wherever TWAIN's real state stands (including resynchronising on `TWCC_SEQERROR`).
- **Capabilities, layout and extended image info.** `Container` (`One`, `Enumeration`, `Array`, `Range`) of `Item`s (`Int8`..`UInt32`, `Bool`, `Fix32`, `Frame`, `Str32`..`Str1024`, `Uni512`, `Handle`), `ImageInfo` (`DAT_IMAGEINFO`), `ImageLayout` (`DAT_IMAGELAYOUT` `MSG_GET`, `MSG_GETDEFAULT`, `MSG_RESET`, `MSG_SET`), `ExtInfo` (`DAT_EXTIMAGEINFO`, freeing any handles the source allocated once), and `status` (`DAT_STATUS`).
- **Native and memory transfers, and driver UI.** Native transfers hand over a locked `Dib` (`Send`, unlocked and `GlobalFree`d on drop); memory transfers fill caller-provided slices checked against `TW_SETUPMEMXFER` (`MinBufSize`, `MaxBufSize`, alignment and even length). Sources may run hidden (`Ui::Hidden`), parented to the hidden window (`Ui::Shown`, `settings_ui(None)`), or parented to a validated Win32 `HWND` (`Ui::ShownOwned(hwnd)`, `settings_ui(Some(hwnd))`), re-enabling the owner window on step-down if a modal driver left it disabled.

## Run TWAIN in a process of its own

Data sources (`.ds` DLLs) are third-party native code loaded into the calling process (`MSG_OPENDSM` loads installed sources to list them, changing the process's current directory to each source's folder as it goes; `32`-bit `.ds` drivers under `C:\Windows\twain_32` load only into a 32-bit process, and `64`-bit drivers under `C:\Windows\twain_64` only into a 64-bit process). Some drivers write debug text to standard output, hang inside modal calls, or crash. Load TWAIN in dedicated 32-bit and 64-bit worker processes under a watchdog rather than inside a main application process.

## Platforms

Windows (`x86_64-pc-windows-msvc` and `i686-pc-windows-msvc`) has the full runtime API (`run`, `Twain`, states, transfers, window loop). Off Windows, the crate's pure types and definitions (`ffi`, `ConditionCode`, `ReturnCode`, `Triplet`, `Container`, `Item`, `Fix32`, `Frame`, `ExtInfo`) build without Windows headers so cross-platform code can inspect them.

## License

Licensed under either of Apache License, Version 2.0
([LICENSE-APACHE](https://github.com/sheaf-pdf/pdf-scan/blob/master/LICENSE-APACHE)) or MIT license
([LICENSE-MIT](https://github.com/sheaf-pdf/pdf-scan/blob/master/LICENSE-MIT)) at your option.

The constants in `src/ffi/` are generated from `TWAINH.cs` in the TWAIN Working Group's `twain-cs` under Kodak Alaris's MIT notice, kept in `src/ffi/consts.rs`, and checked against `twain.h` 2.5. As the TWAIN License asks of software based on the TWAIN Toolkit:

> The TWAIN Toolkit is distributed as is. The developer and distributors of the TWAIN Toolkit expressly disclaim all implied, express or statutory warranties including, without limitation, the implied warranties of merchantability, noninfringement of third party rights and fitness for a particular purpose. Neither the developers nor the distributors will be liable for damages, whether direct, indirect, special, incidental, or consequential, as a result of the reproduction, modification, distribution or other use of the TWAIN Toolkit.

`TWAINDSM.dll` itself is no part of this crate: it is loaded at run time, never linked.
