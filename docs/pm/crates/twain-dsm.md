# twain-dsm

Layer: Devices (`crates/README.md`). Paths are from `docs/pm/`; the code's from the repository's root with a
leading `/`.

**As built by S0 slice 8** (`archive/slices/s0-8-twain-dsm.md`) **and finished for its first publish (`0.1.0-alpha.1`)
in S0 slice 9** (`slices/s0-9-twain-backend.md`), from what the TWAIN spike (S0 slice 7,
`archive/slices/s0-7-twain-spike.md`) learned against the TWAIN Working Group's sample source.

- **What:** TWAIN through the Data Source Manager, loaded at run time (M5 proposal 7, M5 ask 3, entry 6, D17):
  the system's `TWAINDSM.dll`, else one the caller names (the app's bundled copy). The specification puts
  installing it on the application ("Applications that wish to use access the Data Source Manager, must install
  it themselves"); drivers often install it too, so a machine may have one or not, and an old driver's may be
  old.
  - **Safe by construction** (`decisions.md`, 2026-11-08): no `unsafe fn`, and no raw pointer in any public
    signature (`ffi`'s plain structs have pointer fields, which nothing public takes or returns); each contract
    the spike left to its caller is bound by the compiler or kept inside. The DSM is never
    unloaded, so its entry point and memory functions live as long as the process; TWAIN's states are owned
    values that borrow the TWAIN thread's `Twain`, which is neither `Send` nor `Sync`, inside a closure
    higher-ranked over that borrow, so no state leaves the closure or the thread (E0521, E0277, checked by
    `compile_fail` doctests whose codes nightly's rustdoc checks); every handle a driver gives is read and
    freed inside `dsm.rs`, but a native transfer's DIB, an owner (`Dib`, `Send` and `Sync`, locked for its life,
    freed on `Drop`). `unsafe` is in `dsm.rs` and `window.rs` only (and two test modules), every block with
    its `SAFETY` comment. Safe assumes a DSM and drivers that keep TWAIN's calling rules, as any FFI does; their
    data stays untrusted.
  - **The TWAIN thread is the caller's:** `run(|twain| ..)` makes the calling thread a single-threaded apartment
    with a hidden top-level window (the parent the DSM and drivers are given; never shown; its procedure the
    crate's own, which ignores `WM_CLOSE`, so only the crate destroys it, and the crate's wake-up, and passes the
    rest to `DefWindowProcW`) for the closure, then undoes both. One `run` per process at a time, whatever copies
    of the crate the process holds, for the DSM's lack of locks: the lock is a semaphore named for the process,
    given back at the end of a run (panics included) and kept for good by a stranded one. Other threads
    post requests of the caller's own type through `Remote<T>` (`Send + Sync`), which wakes the loop with a
    message to the window, under a lock the teardown takes to clear the window's handle, so a post never
    reaches a reused handle; after the run, a post is handed back (`Gone`). `Twain::wait` pumps the thread's
    messages in any state.
  - **The record, apart from the states:** `Twain` keeps what's open (the DSM loaded, the connection, the
    source and its state 4 to 7), set at each triplet that moves it, and every step down walks from there: each
    state's `Drop`, and the end of `run`, which closes the source and then the connection whatever became of
    the states (a state forgotten with `mem::forget` included). A `Drop` passes a refused step over to the next;
    `disable` and `reset` move the record only on a step the source took. A step refused with `TWCC_SEQERROR`
    restarts the walk higher, from `MSG_RESET`, then from `MSG_ENDXFER` (which in state 6 can make a feeder take
    another page), as TWAIN's resynchronization and `TWAIN.cs`'s rollback do, refusals before the floor reported
    only if it isn't reached. `MSG_XFERREADY` is taken only in state 5 (one from an earlier enable, or said
    twice, is no transfer), and moves the record to 6 as it's received (the sample is there before
    `MSG_ENABLEDS` returns); `MSG_CLOSEDSREQ` in 6 or 7 moves it to 5, and a transfer method then answers
    `Error::CloseRequested`. What a `Drop` couldn't do is in `Twain::drop_errors`. `RunReport` says what the end
    of the run had to close. If the DSM refuses `MSG_CLOSEDSM` at the end, the run reports it
    (`RunReport::stranded`) and leaves the application's identity, the window, the apartment and the flag as
    they are: no `run` follows in that process. A source that won't close is `RunReport::source_left`.
  - **Loading:** `Load::System` (`<system folder>\TWAINDSM.dll`, SysWOW64's in a 32-bit process, which reaches
    it by the System32 path) or `Load::Path`. The file is held open against writes and deletes; its PE machine,
    then its Authenticode signature (offline) and its signer's organization (`O=TWAIN Working Group`, by name,
    not certificate: 2.5.0 and 2.5.1 have different certificates and one signer) are checked through that
    handle before any of its code runs; it's loaded by its full path while the handle is held, and the module
    loaded compared with the file checked. A module of the same name loaded from another file is refused
    (`Elsewhere`), and so is the same file loaded by something else, another TWAIN client or another copy of the
    crate (`InUse`). It's loaded by the final path of the handle held (links and junctions resolved), and files
    are compared by identity (volume and 128-bit file id), never by path: a 32-bit process may be told either
    name for a system file. A Windows file signed through a catalog comes back as Microsoft's (`WrongSigner`)
    or as `Unsigned`, by what the check of the file finds (`kernel32.dll` the one, `cmd.exe` the other, here):
    refused either way. One `Loaded` at a time.
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
    `DAT_CALLBACK2`'s procedure, a safe `extern "system" fn` that keeps the message in a process-wide ring of
    atomics, tagged with a generation changed at every enable and disable, wakes the window and returns
    `TWRC_SUCCESS`, for TWAIN 2 sources only (`DF_DS2`); `Delivery::Events` registers a null procedure (again
    before each enable, which drops a message the DSM still holds from an earlier one). Either way each message
    the loop takes, but the crate's own, is offered to `MSG_PROCESSEVENT` while the source is enabled (the
    source's own windows need it), and each turn of the loop reads the ring and, for events, offers a made-up
    `WM_NULL`, so the DSM hands over a message it holds: another message loop on the thread (a driver's dialog,
    a message box) that takes the wake-up loses nothing. For a TWAIN 2 source by events, the carrier of a
    message the DSM held is dispatched still. `Enabled::next_event` hands over
    `Next::Ready`, `Event` (`CloseRequest`, `CloseOk`, `Device` with `DAT_DEVICEEVENT`'s answer), `Request`,
    `Timeout` or `Quit`, the source's first: every queued message is taken before a request is handed over.
  - **Capabilities:** `get`, `get_current`, `get_default`, `reset`, `set`, `query_support` on a `Source`, as a
    `Container` (one value, an enumeration with its indices as sent, an array, a range) of `Item`s of every
    item type `twain.h` has; a list's one type checked when it's made; strings TWAIN's bytes, checked for
    length and NUL before they're sent, and read to what their type holds, so what was read can be sent back;
    handles decoded to their value, never sent; ranges of the integers, `FIX32` and `BOOL`. Blocks allocated with
    the DSM's `DSM_MemAllocate`, a source's measured with `GlobalSize` and copied out, every length checked
    first: at most 16 MiB, a list at most 65,536 items, so no driver's block can exhaust a 32-bit worker. A one
    value sent is padded to the widest item, so a source that reads it as its capability's own type reads
    nothing past it. `set` answers `SetOutcome::Exact` or `Adjusted` (`TWRC_CHECKSTATUS`).
  - **The driver's window and image layout (completed in S0 slice 9):** `Source::enable(Ui::Shown)` (parented to
    the hidden window) or `Ui::ShownOwned(u64)`, and `Source::settings_ui(owner: Option<u64>)`
    (`MSG_ENABLEDSUIONLY`, `Event::CloseOk` when saved), where `owner` is validated for canonical 32-bit Win32
    `HWND` range (`Error::InvalidWindow { hwnd }`), `IsWindow(hwnd) != 0`, and non-owning caller thread
    (`window_belongs_to_current_thread`), with an `EnableWindow` restore guard on step-down/drop if a modal
    driver left `hParent` disabled. `DG_IMAGE / DAT_IMAGELAYOUT` (`ImageLayout`, `Source::image_layout`,
    `default_image_layout`, `reset_image_layout`, `set_image_layout` with `Error::InvalidFrame` pre-check, and
    `Ready::image_layout`) uses `MSG_GET` / `MSG_GETDEFAULT` / `MSG_RESET` / `MSG_SET`, and `Source::status` /
    `Enabled::status` read `DG_CONTROL / DAT_STATUS / MSG_GET`.
  - **Release packaging (`0.1.0-alpha.1`, S0 slice 9):** `#![warn(missing_docs)]` (with `#[allow(missing_docs)]`
    on `pub mod ffi`), `#![cfg_attr(docsrs, feature(doc_cfg))]`, `[package.metadata.docs.rs] default-target =
    "x86_64-pc-windows-msvc"`, `include = ["/src", "/README.md", "/CHANGELOG.md"]`, `README.md`, `CHANGELOG.md`,
    `just twain-dsm-package`, and Windows `just public-api` (`TEST-14`: 3,665 items read, 1,303 public by path,
    0 unsafe functions, 0 failures; only `Dib`'s `Send` and `Sync` listed).
  - **Transfers:** `Ready::native` (`Native::Done(Dib, Transferred)` or `Cancelled`; an empty `Dib` if the
    source said done and gave no block, or it wouldn't lock) and `Ready::memory` (refused if no buffer can meet
    the source's sizes), whose `next_buffer` fills the caller's buffer, refused unless TWAIN's rules hold (at
    least `MinBufSize`, at most `MaxBufSize`, of even length at a 4-byte boundary, one size for the transfer),
    and hands back a `Strip` of what the source wrote, `BytesWritten` checked; `preferred_size` resolves
    `TWON_DONTCARE32`, keeps within the source's sizes and 64 MiB, and is a multiple of 4. `image_info` gives
    `ImageInfo`, `DAT_IMAGEINFO`'s answer unpacked (only its first `bits_per_sample` to be trusted), in state 6,
    and in 7 after `TWRC_XFERDONE` (a feeder's measured length). One `MSG_XFERREADY` per enable: `end`
    (`MSG_ENDXFER`) gives `Pending::More(Ready, count)` or `Done(Enabled)`, the count `None` for 0xFFFF, "don't
    know". Extended image info in state 7 after `TWRC_XFERDONE` only (`ext_image_info`), an entry's `Item` the
    handle of a block (which the application frees) when its data is more than four bytes, else the data
    inline, in either bitness: `TW_INFO`'s rule in the TWAIN 2.4 specification, which its example and the
    TWAIN Working Group's sample application keep (its `TW_EXTIMAGEINFO` says `sizeof(TW_HANDLE)`, which on
    64-bit Windows would read eight bytes of a handle as data); a `TWTY_HANDLE` entry's is always a handle to
    its bytes. A handle named twice is freed once.
  - **Cancel by state:** a request posted from another thread is taken in the loop; at the next state-6 point
    (before the first transfer, or after a page's `MSG_ENDXFER`) `Ready::reset` (`MSG_RESET`) ends the batch
    (`Twain::requeue` puts a request taken that isn't for now back first);
    `Ready::stop_feeder` where a driver has it (the sample refuses it, `TWCC_BADPROTOCOL`).
  - **Condition codes:** a failure's are each party's, each asked once: the DSM's for no application first (an
    identity whose `Id` is out of range, which takes the read path that checks it, and is used for that
    triplet alone: the DSM's other paths index by the `Id` unchecked), then the application's,
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
  processes, in both bitnesses, here and in CI's `twain-dsm` job on `windows-latest` (green from pull request
  #4's run, 42617750221): listing, the signature cases, capabilities in all four containers, transfers
  both ways and both deliveries with their pending counts, native and memory pixels equal, states dropped and
  forgotten anywhere, cancels, requests (each timed, so a post that doesn't wake the loop fails), another
  message loop on the thread, a stale transfer ready, the buffer rules, and a stranded teardown; inside the
  crate, the triplets `reset` and `disable` send, the run lock shared with another copy, the hidden window
  outliving a `WM_CLOSE`. By hand: the source's window; a real driver (slice 12). The spike's own program is
  gone; its record is history.
- **Open:**
  - `twain.h`'s license, settled 2026-11-08: no copy grants a license (copyright notices, "All rights
    reserved", outside the DSM's LGPL); the only grant is twain.org's TWAIN License, whose reach to a Rust
    transcription is for counsel. `TWAINH.cs` makes it moot.
  - Whether the fi-6130's FUJITSU TWAIN32 needs a visible parent window, a modal enable, or delivery by events
    (a TWAIN 1.x driver can't call back): the hardware, slice 12.
  - Fuzzing the decoders with `cargo-fuzz` (TEST-11). (Owner windows and the 32/64-bit list merge in
    `scan-worker` were settled in S0 slice 9.)
