# twain-dsm

Layer: Devices (`crates/README.md`). Paths are from `docs/pm/`; the code's from the repository's root with a
leading `/`.

**As built by S0 slice 8** (`slices/s0-8-twain-dsm.md`, its "So far" the record), from what the TWAIN spike (S0
slice 7, `archive/slices/s0-7-twain-spike.md`) learned against the TWAIN Working Group's sample source.

- **What:** TWAIN through the Data Source Manager, loaded at run time (M5 proposal 7, M5 ask 3, entry 6, D17):
  the system's `TWAINDSM.dll`, else one the caller names (the app's bundled copy). The specification puts
  installing it on the application ("Applications that wish to use access the Data Source Manager, must install
  it themselves"); drivers often install it too, so a machine may have one or not, and an old driver's may be
  old.
  - **Safe by construction** (`decisions.md`, 2026-11-08): no `unsafe fn` and no raw pointer in the public API;
    each contract the spike left to its caller is bound by the compiler or kept inside. The DSM is never
    unloaded, so its entry point and memory functions live as long as the process; TWAIN's states are owned
    values that borrow the TWAIN thread's `Twain`, which is neither `Send` nor `Sync`, inside a closure
    higher-ranked over that borrow, so no state leaves the closure or the thread (E0521, E0277, checked by
    `compile_fail` doctests whose codes nightly's rustdoc checks); every handle a driver gives is read and
    freed inside `dsm.rs`, but a native transfer's DIB, an owner (`Dib`, `Send` and `Sync`, locked for its life,
    freed on `Drop`). `unsafe` is in `dsm.rs` and `window.rs` only (and two test modules), every block with
    its `SAFETY` comment. Safe assumes a DSM and drivers that keep TWAIN's calling rules, as any FFI does; their
    data stays untrusted.
  - **The TWAIN thread is the caller's:** `run(|twain| ..)` makes the calling thread a single-threaded apartment
    with a hidden top-level window (the parent the DSM and drivers are given; never shown, its procedure
    `DefWindowProcW`, so no Rust code runs inside one) for the closure, then undoes both. One `run` per process
    at a time, for the DSM's lack of locks (an atomic flag, released by a guard, panics included). Other threads
    post requests of the caller's own type through `Remote<T>` (`Send + Sync`), which wakes the loop with a
    message to the window, under a lock the teardown takes to clear the window's handle, so a post never
    reaches a reused handle; after the run, a post is handed back (`Gone`). `Twain::wait` pumps the thread's
    messages in any state.
  - **The record, apart from the states:** `Twain` keeps what's open (the DSM loaded, the connection, the
    source and its state 4 to 7), set at each triplet that moves it, and every step down walks from there: each
    state's `Drop`, and the end of `run`, which closes the source and then the connection whatever became of
    the states (a state forgotten with `mem::forget` included). A step refused with `TWCC_SEQERROR` restarts
    the walk once from state 7, as `TWAIN.cs`'s rollback does; `MSG_XFERREADY` moves the record to state 6
    as it's received (the sample is there before `MSG_ENABLEDS` returns). What a `Drop` couldn't do is in
    `Twain::drop_errors`. If the DSM refuses `MSG_CLOSEDSM` at the end, the run reports it
    (`RunReport::stranded`) and leaves the application's identity, the window, the apartment and the flag as
    they are: no `run` follows in that process. A source that won't close is `RunReport::source_left`.
  - **Loading:** `Load::System` (`<system folder>\TWAINDSM.dll`, SysWOW64's in a 32-bit process, which reaches
    it by the System32 path) or `Load::Path`. The file is held open against writes and deletes; its PE machine,
    then its Authenticode signature (offline) and its signer's organization (`O=TWAIN Working Group`, by name,
    not certificate: 2.5.0 and 2.5.1 have different certificates and one signer) are checked through that
    handle before any of its code runs; it's loaded by its full path while the handle is held, and the module
    loaded compared with the file checked. A module of the same name loaded from another file is refused
    (`Elsewhere`). Files are compared by identity (volume and file index), never by path: a 32-bit process
    may be told either name for a system file. `WinVerifyTrust` verifies a catalog-signed file through its
    catalog, so Windows' own DLLs are Microsoft's (`WrongSigner`), not `Unsigned`. One `Loaded` at a time.
    `TWAINDSM_LOG` naming a file, with `TWAINDSM_MODE=w`, makes the DSM log every triplet.
  - **What the DSM does that the crate must know** (the spike, and slice 8 in the DSM's source): it loads every
    source's library at `MSG_OPENDSM`, so opening it runs each installed driver's code; it skips Windows' own
    `wiatwain.ds`; it changes the process's current directory to each source's folder while loading it. At
    `MSG_CLOSEDSM` it closes sources left open in a loop that stops before the last one listed (`apps.cpp`,
    `RemoveApp`), so a source installed alone is never closed, and stays connected (`TWCC_MAXCONNECTIONS` at
    the next open): the crate closes its own first. It keeps a condition code for refusals it can't pin on an
    application, and clears it whenever an application reads its own. A triplet to a source it lists but
    hasn't open is its own `TWCC_OPERATIONERROR`. It keeps a callback registration for a source's id across
    closing and opening it again, and accepts a null procedure, which clears it.
  - **Bindings** generated by `/scripts/twain-bindings.py` from the TWAIN Working Group's own C# translation of
    `twain.h`, `TWAINH.cs` (MIT, Kodak Alaris, its notice kept), both fetched at pinned commits, each value
    checked against `twain.h` 2.5: 1192 constants, 36 more written by hand (`ffi/by_hand.rs`, checked the same
    way), `twain.h`'s 1205 numeric defines and 23 aliases in all. A header contract compiles `twain.h` with the
    MSVC `find-msvc-tools` finds, in both bitnesses, and compares 28 structs' sizes and 98 offsets and every
    constant; the header is never committed (it grants no licence), fetched in CI with its hash checked. Where
    the two differ `twain.h` wins: `TWAINH.cs`'s Windows identity ends in 8 bytes of padding that `twain.h`
    hasn't (`TW_IDENTITY` is 156 bytes); its function pointers are `IntPtr`s, so their signatures are
    `twain.h`'s, `TW_CALLINGSTYLE` being `__stdcall` on x86. The function-pointer types and the structs that
    hold them stay inside the crate. The TWAIN Toolkit's warranty disclaimer is in the source and the docs.
  - **TWAIN's seven states as types** (RSWebTWAIN's design, MIT OR Apache-2.0, taken as a design, not its code):
    `Loaded`, `Opened`, `Source`, `Enabled`, `Ready`, `Transferred`, `MemoryTransfer`, each owning the one
    beneath. A step up consumes the state; a refused step hands it back in `Refused<S>` (`into_state`,
    `into_error`, `#[must_use]`, no blanket `From`, so `?` never walks a chain down unseen). One source at a
    time.
  - **Messages from the source:** delivery fixed per open source. `Delivery::Callback` registers
    `DAT_CALLBACK2`'s procedure, a safe `extern "system" fn` that posts the message to the window (its handle
    and a generation in process-wide atomics, so it can't dangle) and returns `TWRC_SUCCESS`, for TWAIN 2
    sources only (`DF_DS2`); `Delivery::Events` registers a null procedure and offers each message the loop
    takes, while the source is enabled, to `MSG_PROCESSEVENT`. `Enabled::next_event` hands over
    `Next::Ready`, `Event` (`CloseRequest`, `CloseOk`, `Device` with `DAT_DEVICEEVENT`'s answer), `Request`,
    `Timeout` or `Quit`, the source's first: every queued message is taken before a request is handed over.
  - **Capabilities:** `get`, `get_current`, `get_default`, `reset`, `set`, `query_support` on a `Source`, as a
    `Container` (one value, an enumeration with its indices as sent, an array, a range) of `Item`s of every
    item type `twain.h` has; a list's one type checked when it's made; strings TWAIN's bytes, checked for
    length and NUL before they're sent; handles decoded to their value, never sent. Blocks allocated with the
    DSM's `DSM_MemAllocate`, a source's measured with `GlobalSize` and copied out, every length checked
    first. `set` answers `SetOutcome::Exact` or `Adjusted` (`TWRC_CHECKSTATUS`).
  - **The driver's window:** `Source::enable(Ui::Shown)` (parented to the hidden window; the application's own
    window as owner is slice 9's), and `Source::settings_ui()` (`MSG_ENABLEDSUIONLY`) for its settings alone,
    `Event::CloseOk` when they're saved; both run by hand with the sample (`tests/ui.rs`).
  - **Transfers:** `Ready::native` (`Native::Done(Dib, Transferred)` or `Cancelled`) and `Ready::memory`, whose
    `next_buffer` fills the caller's buffer (refused under `MinBufSize`) and hands back a `Strip` of what the
    source wrote, `BytesWritten` checked; `preferred_size` resolves `TWON_DONTCARE32` and caps at 64 MiB.
    `image_info` gives `TW_IMAGEINFO` as TWAIN does (only its first `BitsPerSample` to be trusted). One
    `MSG_XFERREADY` per enable: `end` (`MSG_ENDXFER`) gives `Pending::More(Ready)` or `Done(Enabled)`, the
    count kept (`Ready::pending`, `None` for 0xFFFF, "don't know"). Extended image info in state 7
    (`ext_image_info`), by the specification's rule (TWAIN 2.4, `TW_EXTIMAGEINFO`): an entry's `Item` is the
    handle of a block, which the application frees, when the data is larger than a `TW_HANDLE`, else the data
    inline; a `TWTY_HANDLE` entry's is always a handle to its bytes.
  - **Cancel by state:** a request posted from another thread is taken in the loop; at the next state-6 point
    (before the first transfer, or after a page's `MSG_ENDXFER`) `Ready::reset` (`MSG_RESET`) ends the batch;
    `Ready::stop_feeder` where a driver has it (the sample refuses it, `TWCC_BADPROTOCOL`).
  - **Condition codes:** a failure's are each party's, each asked once: the DSM's for no application first (an
    identity whose `Id` is out of range, which takes the read path that checks it), then the application's,
    which wins; then the source's, if the triplet was to the source open. `Error::Failed { triplet, rc,
    by_source, by_dsm }`. Codes such as `TWCC_PAPERJAM`, `TWCC_PAPERDOUBLEFEED`, `TWCC_NOMEDIA` reach the
    backend so.
  - **Both bitnesses:** built and tested for `x86_64-pc-windows-msvc` and `i686-pc-windows-msvc`. A source loads
    only into a process of its own bitness, and the office's fi-6130s have a 32-bit driver (M5 ask 12).
- **For the seam** (`crates/scan-device-seam.md`): per transfer, `DAT_IMAGEINFO` as TWAIN gives it; the pending
  count after each (0xFFFF until the last, for the sample); extended image info as offered (a page's side,
  patch codes for FEAT-5); each memory buffer's shape and bytes, and a native transfer's DIB block, not
  assembled (the backend assembles and decodes: the spike's `image` module, now in the tests' support, is its
  start); events with how they came; condition codes from both parties; requests served in every state;
  the teardown's report.
- **Prior art:** `twain` 0.1.0 (2021, the only TWAIN crate) loads a DLL by a relative path, passes an
  immutable local as an out-pointer, never calls `GlobalLock`, and bundles the DSM and Qt's `qwindows.dll`
  without their LGPL text: study only. TWAIN Direct has had no commit since 2022: study only. pytwain is
  GPL-2.0 and Saraff.Twain LGPL: study only. NAPS2's 8.6.0 cured its TWAIN hangs with an STA thread (study
  only).
- **Boundary:** TWAIN alone, Windows alone (TWAIN on macOS is gone, M5 research).
- **Tested by:** unit tests of the pure parts on every system (codes, containers both ways, extended image info,
  the PE reader, random input that decodes or is refused, never a panic); the header contract with MSVC in
  both bitnesses; and the TWAIN Working Group's sample data source (`Twain_DS_sample01`, BSD-3; it reads its
  images from disk, its page count from a `CAP_XFERCOUNT` environment variable), installed silently by
  `/scripts/twain-sample.ps1` (the DSM 2.5.0 with it, both bitnesses), through the crate's API in child
  processes, here and in CI's `twain-dsm` job on `windows-latest`, in both bitnesses: listing, the signature
  cases, capabilities in all four containers, transfers both ways and both deliveries, native and memory
  pixels equal, states dropped and forgotten anywhere, cancels, requests, and a stranded teardown. By hand: the
  source's window; a real driver (slice 12). The spike's own program is gone; its record is history.
- **Open:**
  - `twain.h`'s license, settled 2026-11-08: no copy grants a license (copyright notices, "All rights
    reserved", outside the DSM's LGPL); the only grant is twain.org's TWAIN License, whose reach to a Rust
    transcription is for counsel. `TWAINH.cs` makes it moot.
  - Whether the fi-6130's FUJITSU TWAIN32 needs a visible parent window, a modal enable, or delivery by events
    (a TWAIN 1.x driver can't call back): the hardware, slice 12.
  - The application's own window as the driver's window's owner: slice 9 (seam, section 9).
  - Whether the crate offers the 64-bit app's view of 32-bit sources (it can't load them), or leaves the
    merge to `scan-worker` (leaning: the worker, proposal 4).
  - Fuzzing the decoders with `cargo-fuzz` (TEST-11); the API's first user, the `twain` backend (slice 9),
    before it's made ready to publish.
