# S0: Scan devices

**The device layer, before the pages and the PDF: SANE on Linux, then TWAIN and WIA on Windows; the device
model frozen once both have been through it.** The maintainer's order (2026-11-08: "we want to add a s0
milestone for the scan devices"): S0, then S1 pages, then S2 PDF (`decisions.md`). Needs nothing unbuilt; the
hand checks need scanners, and the Windows slices a Windows machine (ask 4's point, now answered: device work
starts before a scanner is at hand).

Bindings first, the model drafted beside them and kept provisional (`decisions.md`, "Made here"): `sane-dl`
follows SANE alone and needs no model, so it comes first; the model's hard points (how a page arrives, its
side, pull or push, cancel) show up while writing a driver layer, so the seam note, first written from the
research, is revised after it; and the model isn't frozen until the `twain` backend has been through it.
Slices that wait on hardware (the hand checks, and the Windows slices without a Windows machine) don't hold
S1 back: S1 can start once the Linux slices are done.

## On Linux

1. **sane-dl, and the workspace it starts.** First the workspace itself, from S1 slice 1's work (stashed,
   and copied into `scratch/stash-s1-1/`), reshaped so `sane-dl` is its first member: `Cargo.toml`,
   `LICENSE-MIT` and `LICENSE-APACHE`, `.gitattributes`, the root `README.md`, the justfile (`ci`, `fmt`,
   `clippy`, `test`, `doc`, `gates`, the MSRV recipes), CI on Ubuntu, Windows and macOS, and the gates for
   what exists (the devices layer takes no image codec, imageproc, PDF crate or GUI toolkit; nothing links
   libsane; no file outside `docs/pm/` names sheaf). Then `sane-dl`: loading, bindings, the safe handle, the
   cancel token (synchronized with close), options, frames, and the fake libsane, `fake-libsane`, a
   never-published `cdylib` crate of its own for good (not moved into `scan-fixtures` with S1:
   `crates/fake-libsane.md`, `decisions.md`), narrowed to what SANE's own backends can't do. It exposes what
   the seam needs (`crates/sane-dl.md`, "For the seam") as SANE gives it. Its MSRV measured as S1 slice 1
   measured the pixel crates'. Checked: `just ci`; SANE's `test` and `pnm` backends in CI and the bindings
   against the installed `sane.h` (TEST-2); the fake's cases; missing libsane said, not a crash; each gate
   failing on a violation made for the check.
   **Done, and merged 2026-11-09** (pull request #2, squashed as `3ebea15`; `archive/slices/s0-1-sane-dl.md`).
   Done on the branch 2026-11-08: branch `s0-1-sane-dl`, from `137be1a`
   to its head (the code as of `493b0c7`, its privacy commit `9b8bd5a`, then the record). `sane-dl`
   0.1.0-alpha.1, ready to publish and not published, MSRV 1.89, with `fake-libsane`; its five steps checked
   and its code review's 22 findings fixed. For review: the plan's "Decisions taken alone, for review" (72)
   and "For review (security)" (10), above all the code review's API changes before the first publish
   (`Sane::load_from` an `unsafe fn`, which slice 1a removed, `DEFAULT_LIBRARIES` with macOS's absolute
   paths, the `links` key and its empty build script, `LimitExceeded`, `ReloadAfterSet`, `options_generation`,
   `SetInfo` non-exhaustive, `clear_cancel`). CI's run on the three systems is
   green (pull request #2, run 42582929367, 2026-11-09), which closed TEST-2 and TEST-8.

**1a. sane-dl's public API made safe.** A follow-up to slice 1, asked for on 2026-11-09 ("i think that we
should try to define safe apis where possible"), which confirms `decisions.md`'s ruling of 2026-11-08, "Public
APIs are safe Rust; `unsafe` contracts stay inside a crate", recorded by slice 8 after slice 1's code review had
made `Sane::load_from` an `unsafe fn`. Every public item of `sane-dl` whose contract a caller keeps given a safe
shape, the API's shapes weighed in the plan before one is taken, or named as an exception with why: the library
chosen by the system's loader alone (`Sane::load_from` removed; another SANE loaded through the loader's own
search path, set as the process starts); the tests loading `fake-libsane` through `Sane::load` itself, the loader
pointed at it by each child's environment, and what needs a path of its own tested inside the crate; `ffi`'s
function-pointer types inside the crate; one SANE per process whatever copies of the crate it holds (on Linux a
process-wide slot); BUG-7 (`Image`'s derived `Debug` printing every byte); and a gate over every public item,
read from rustdoc's JSON on a pinned nightly. `sane-dl` stays 0.1.0-alpha.1, unpublished, its changelog saying
what changed. Checked: `just ci`; the soundness review (the code review's soundness lens: every `unsafe` block
and `SAFETY:` comment, the private loader's contract and its call sites, each public item's docs); the gate over
the public API (no `unsafe` function, method or trait, and no raw or unsafe function pointer in a public
signature), made to fail on violations made for the check; the publish dry run. Plan:
`archive/slices/s0-1a-sane-dl-safe-api.md`.
**Done, and merged 2026-11-09** (pull request #7, squashed as `42e4227`; `archive/slices/s0-1a-sane-dl-safe-api.md`):
branch `s0-1a-sane-dl-safe-api`, from `3f33be5`, the code to `1a94bee`: four steps, each checked, then the code review's 22 findings, all fixed. No
public item of `sane-dl` is `unsafe`: `Sane::load_from` is gone, the library the system loader's choice alone
(shape 1 of the eight weighed), another SANE loaded through the loader's own search path; the tests load
`fake-libsane` through `Sane::load()`, the fake's tests ignored on Windows, saying why; `ffi`'s function types
inside the crate; on Linux a process-wide slot (an abstract socket named for the process, its name published)
keeping SANE to one per process across copies of the crate built apart, waiting out a spawned child's moment
between fork and exec, a probe with two copies showing it; `Image`'s `Debug` by hand (BUG-7 done); `just
public-api`, the gate over every public item, widened by the code review, in `ci` and a CI job of its own. The
code review found what `Sane::load`'s safety rests on beyond the loader: the process's
environment, kept to two rules its docs now give (the search path's entries absolute; the library path's
variables never changed inside a process using SANE, whose `dll` reads them at each backend load). To review:
the plan's "Decisions taken alone, for review" (40; decision 33, that environment rule, above all) and "For
review (security)" (11), Q-7 and UP-14. CI's run of pull request #7 is green on all eleven jobs (run
42648083514), which closed TEST-15: on Windows the fake's tests ignored with their reason, on macOS run through
`DYLD_LIBRARY_PATH` and passing (TEST-7 stays open for Homebrew's SANE). Its TEST-12 is TEST-14, slice 3 having
merged first.

2. **The seam note revised** (`crates/scan-device-seam.md`, first written 2026-11-08 from the survey and M5
   proposals 4 to 10): what slice 1 showed of SANE folded in, its SANE unknowns answered (names that change
   on replugging, stopping after the page in flight), the raw layout's polarity and byte order fixed, and the
   unknowns TWAIN is to settle kept. Checked: reviewed before slice 3 starts, since it fixes slice 3's API.
   **Done, and merged 2026-11-09** (pull request #3, squashed as `70d7b20`; `archive/slices/s0-2-seam-note.md`).
   Done on the branch 2026-11-09, as it waited on review: branch `s0-2-seam-note`,
   from slice 1's tip `4624760` to its head (the fixes at `6e89de8`, then the record), docs only, three steps,
   each checked, then its code review's 25 findings fixed (one deferred in part, BUG-7). The note's SANE rows
   from slice 1 and the slice's probes, the two SANE unknowns answered (what only slice 6 settles is TEST-9), the
   raw layout fixed and held beside `scanimage`'s pages, and the model written down for slice 3 ("The model, for
   slice 3"), revised by the review before slice 3 builds it: `open` taking the stored device and a deadline,
   SANE's fallback listing first and only from a USB connection's name, a bounded event channel, `Stopped` moved
   to the worker's client, four new `ScanError`s, the growing structs `#[non_exhaustive]`. The plan's review and
   its code review are the review this entry asks for; then its pull request, stacked on slice 1's, reviewed by
   the maintainer out of band, and slice 3 starts once it's open. For review: the plan's "Decisions taken alone,
   for review" (41, of which 27 to 41 the code review's) and "For review (security)" (6).
3. **scan-device, provisional:** the model as the seam note's "The model, for slice 3" has it (its types, the
   `Backend` and `Device` traits, `RawImage`'s checked constructor, cancel and progress through `in-progress`,
   taken here: vetted in slice 2's plan, and `plan.md`'s graph updated), the constructors of the structs marked
   `#[non_exhaustive]`, `Debug` by hand for `RawImage` and `Encoded`, settings clamped and said, the `fake`
   backend and its scripts; 0.0.x, unpublished. `scan-device` joins the gates' devices-layer loop (`justfile`,
   `for crate in sane-dl`), its graph read with `in-progress` in it and the gate's output naming it. Checked: the
   scripts (proposal 22) on every runner, among them a cancel after a duplex front (its back still delivered), a
   stored device opened as a renamed one found by its maker and model, a stored name now listed as another model
   taken as gone, and a batch held by a full channel; `RawImage::new`'s checks unit-tested (the length, a 1-bit
   row's padding bits, a size past `u64` or a 32-bit `usize` refused, never wrapped); the gate made to fail on a
   copy (an image codec added to `scan-device`).
   **Done, merged 2026-11-09** as pull request #6 (squashed `5a4bff2`; `archive/slices/s0-3-scan-device.md`), CI
   green on it (the run on `master` at `5a4bff2`, 42644985306, all ten jobs: TEST-13 closed): branch
   `s0-3-scan-device`, from `master` at `3f33be5`, to `5a5c367` (the code review's fixes), then the record, and
   `master` merged in after #5; four steps, each checked, then its code review's 18 findings (15 distinct, 1
   major) all fixed.
   `scan-device` 0.0.0, unpublished, MSRV 1.89: the note's model type for type, nothing renamed, with a
   constructor for each `#[non_exhaustive]` struct, `RawImage::data_len` and `Debug` by hand for `Opened` besides
   (`crates/scan-device.md`); the `fake` feature, turned on for the crate's tests by a dev-dependency on itself,
   its scripts Rust values, every script of proposal 22 tested (here on Linux; on Windows and macOS by CI on
   pull request #6: TEST-13), the crash and the exit in child processes; the gates reading each devices crate's
   graph, and building the binaries they read for libsane, with every feature on, `scan-device` among them, and failing
   on a codec added as a dependency and as one behind a feature. The code review's major finding: `RawImage`'s
   sizes now held to what one buffer can hold (`isize::MAX`), where a 32-bit reader sizing its buffer by
   `data_len` could panic; and `Paper` made `#[non_exhaustive]`, a change to the note's block. For review: the
   plan's "Decisions taken alone, for review" (50, of which 42 to 50 the code review's) and "For review
   (security)" (6), above all decisions 42 (`Paper`), 43 (`open`'s order) and 47 (a cancel's and a receiver's
   rules said, the behaviour kept), and Q-6 (what else to settle before slice 11 freezes the model).
4. **scan-worker:** the protocol, `serve`, the client, the watchdog. The protocol as the seam note's "What slice
   4 needs of it" has it: an `open` request carrying the stored `DeviceInfo` and a deadline, answered with
   `renamed_from` and the device found, before `caps` and `scan`, the worker chosen by the device's driver; an
   `applied` event; progress lines from the worker's `Progress`; `cancel` cancelling the batch's tracker, and the
   kill after it allowing for the sheet in flight; a `settings_window` request carrying the owner; the bounded
   channel drained as the pipe is written; a page's format and size before its bytes, and a `RawImage` decoded
   through `RawImage::new`; the model's types encoded by a `serde` feature of `scan-device` (leaning) or by wire
   types with every variant round-tripped in a test; the client's own error type (the watchdog's stop, a crash
   and its exit, a protocol mismatch, a page cut short) wrapping `ScanError`; and a SANE worker that opens a
   stored id spawned with that id's one-backend `ConfigDir` (`Search::BeforeSystem`, naming none for an id with
   no backend), in a directory it trusts. Checked: the `fake` backend through a real worker process: a hang
   killed with the pages kept, a crash with the pages kept and the exit said, a cancel mid-page (the sheet
   finished, none fed after), a version mismatch, a page cut short; the raw layout across the pipe byte for byte;
   throughput of 300 dpi pages in color and black and white at 2.3 a second (M5 slice 6's gate, its pipe's
   share), measured on every run.
   **Done on the branch** (2026-11-09; `slices/s0-4-scan-worker.md`): branch `s0-4-scan-worker`, from `master`
   at `5a4bff2`, to `510d349` (the code review's fixes), then the record; five steps, each checked, then its
   code review's 20 findings (5 major), 18 fixed whole and two in part, their other halves deferred (FEAT-16,
   FEAT-17), none refuted. Pushed with `master` merged in after #7, in pull request #8, waiting on review and
   CI. `scan-worker` 0.0.0, unpublished,
   MSRV 1.89, no `unsafe` (`crates/scan-worker.md`): the protocol as the seam note asks it, the model's types
   through a new `serde` feature of `scan-device` (every variant round-tripped, a variant added failing the
   build until it's listed), a page's header, `Event`'s and `Pixels`' variants and a message's level carried
   by name (Q-8); `serve`, its batch's events through a channel that holds none, the end of its input a
   cancel and a quit, and an abort 10 s later should a driver's call hang; the client, a worker for each
   listing and each open device in a private directory (a SANE open's holding the id's one-backend
   `ConfigDir`), every line and page capped and checked before it's read, its own `Error`, and its watchdog,
   the kill after a cancel counted from the later of the cancel and the worker's last line; and the fake's
   `Fault::AwaitCancel`, so a cancel lands mid-page by the script. Checked here through a test worker,
   `scan-worker-fake`: each check above, the raw layout byte for byte against the fake's `pattern`, and the
   throughput, asserted on every run, at about 29 pages a second in color in the debug build the tests run
   (13 times the gate; 15.8 at the lowest, under outside load). On Windows and macOS: CI's run of its pull
   request (TEST-16). The code review's fixes: a worker's process watched while a call waits, so its end is
   seen even where a process its driver started holds its pipes (a crash under such a helper had been taken
   for silence, or held the settings window for good); the worker's input kept open while it quits, so a grace
   past 10 s doesn't turn a slow close into the worker's abort; a request past 1 MiB `Unsendable`; caps,
   applied settings, a batch's order, encodings and an open's ids checked against the model's rules (the caps
   rules now in the model's docs); another protocol's hello read for its protocol; `Stopped`'s `Timeout` and
   `WorkDir`; each library linted and documented alone, so `scan-device` is built without `serde` again; the
   `SIGPIPE` requirement documented. Left: the Job object and a tree kill (FEAT-14, slice 9), a stop handle
   (FEAT-17, slice 9), a worker's end harmless whatever the app's `SIGPIPE` disposition (FEAT-16, before slice
   11), a spare worker kept warm (FEAT-15), TEST-12 to slice 9's 32-bit job. For review: the plan's
   "Decisions taken alone, for review" (55, of which 41 to 55 the code review's, and each step's own in its "So
   far"), above all 41 (how a worker's end is seen), 42 (the input kept open on quit), 43 (`SIGPIPE`) and 46
   (the model's rules checked, a clamped value against what was asked); and "For review (security)" (11).
5. **scan-device's `sane` backend**, mapping `sane-dl` onto the model as the seam note's SANE rows and its table
   in "The model, for slice 3" say. Checked: SANE's `test` backend in CI (a 10-sheet feeder then empty; gray,
   color, 1- and 16-bit; unknown height; three-pass; injected jam, cover open, no documents, I/O error: a jam
   ends the scan), 16-bit and 1-bit RGB in the model's raw layout (the 16-bit conversion a function of the
   source's byte order, so its swap is unit-tested on CI's little-endian hosts); pages through the `pnm` backend,
   a source without `source`, `mode` or area options (S0 slice 2's decision 41), its PBM compared after the
   inversion with the padding bits cleared on both sides (PBM is 1 black, the model 0; at a width not a multiple
   of 8, with set padding bits, as probe L1's 13-pixel PBM has), PGM and PPM byte for byte; ids from Latin-1,
   back to `sane_open` byte for byte; the open in a worker given the id's one-backend `ConfigDir`
   (`Search::BeforeSystem`, naming none for an id with no backend), listing before it opens: a stored name listed
   as another model taken as gone, the fallback on `INVAL` at open from a USB connection's name only (the fake
   listing a device under a new name; none from a serial or a network name, nor to one of two), and TEST-10's
   cases, an alias's target gone opened then listed in one process among them; `dll`'s testing prefixes, a
   leading `:`, the empty name and an id not Latin-1 refused before `sane_open` (`BadId`); `INVAL` from a start
   (the fake's `at PAGE start INVAL`) met by setting the settings again and starting once more, and
   `SettingsRefused` when it repeats; `ACCESS_DENIED` as `AccessDenied`, a listing past its deadline as
   `TimedOut`; the feeder keeping its sheets after a cancel; the fake libsane's feeder and duplex names for each
   backend. Whatever the backend finds the model lacking goes into the seam note and the model, said.
6. **By hand on Linux:** an fi-6130 through SANE's fujitsu backend on a Linux laptop at the office (M5 ask
   12; SANE rates it complete): 50 mixed sheets duplex, a forced jam, an open cover, unplugged mid-scan, a
   cancel. A row per scanner and driver in the notes. Waits on the scanner; doesn't hold the rest back.

## On Windows

7. **The TWAIN spike, end to end:** TWAIN's sample data source installed silently on `windows-latest`
   (`msiexec /qn`, or yushulx/virtual-scanner's portable zip, or the DSM and the source copied into
   `twain_32` and `twain_64`) and scanned with a hidden window, in both bitnesses (TEST-3).
   - If it installs: a throwaway scan through it, the DSM loaded and walked from state 1 to 7, one memory
     transfer and one native, reading each transfer's image info, the pending-transfer count and whatever
     extended image info the source offers. It answers the seam note's TWAIN unknowns
     (`crates/scan-device-seam.md`) before `twain-dsm`'s API is fixed, and CI scans through TWAIN from
     then on.
   - If it doesn't: a desk check of the seam note against `TWAINH.cs` and NAPS2's notes (study only), and the
     hand checks carry the rest.
   - Either way the seam note is revised, said, before slice 8.
   - **Done here, 2026-11-08, ahead of slices 2 to 6** (ruled 2026-11-08; `archive/slices/s0-7-twain-spike.md`): it
     installed silently, here and on `windows-latest`, and scanned with a hidden window in both bitnesses
     here: native and memory, two delivery modes, a pixel-for-pixel comparison, a cancel from another thread.
     The seam note is revised (its TWAIN rows). CI's `twain-spike` job ran the scenarios on
     `windows-latest` in both bitnesses, green on every push (TEST-3, done). **Closed 2026-11-08:** the code review and its fixes,
     CI green, merged as pull request #1.
8. **twain-dsm:** bindings ported from `TWAINH.cs` (MIT; `TWAIN.cs`, beside it, wraps it) and checked
   against `twain.h` 2.5, the state
   machine, the STA thread and hidden top-level window, capabilities, transfers, condition codes; built for
   both targets. It exposes what the seam needs (`crates/twain-dsm.md`, "For the seam").
   - **Built 2026-11-08** (`archive/slices/s0-8-twain-dsm.md`), safe by construction (`decisions.md`): its six steps
     on branch `s0-8-twain-dsm`, each pushed; generated bindings and a header contract in both bitnesses; the
     DSM loaded only after its signer is checked; TWAIN's states as owned types on the caller's thread, a
     record apart from them that the teardown trusts; capabilities, both transfers, both deliveries, cancels
     and requests, tested through the sample source in both bitnesses. `twain-spike` is deleted, its record
     kept. Code-reviewed and fixed 2026-11-09. **Closed 2026-11-09:** CI green on pull request #4, every job,
     merged.
9. **scan-device's `twain` backend** and a 32-bit worker in CI (`i686-pc-windows-msvc`, the worker alone),
   the client's merge of both lists. The model's changes the backend asks for are made here, in
   `scan-device`, the `fake` and `sane` backends and the worker's protocol together. Checked: 10 images
   through the sample source in both bitnesses (if the spike worked); the error mapping in unit tests; the
   Linux slices' checks again, on the changed model.
   - **A real driver first, 2026-11-09,** ahead of slice 12, on the Windows machine: an fi-6130 on PFU's
     PaperStream IP (TWAIN) 1.42.0.5685, which takes the plain fi-6130 as well as the fi-6130Z (an archived PFU
     page lists both; slice 12 assumed FUJITSU TWAIN32 for it), driven through `twain-dsm` by a throwaway 32-bit
     program outside the repository, gray at 200 dpi, native transfers, each page kept as a BMP: two runs of one
     page each (the second sheet jammed a little), its capabilities read on a fresh open with nothing set, then a
     batch of seven test sheets, the driver's settings as it opened but for gray and 200 dpi. What the backend
     takes from it:
     - **32-bit only, as installed.** The package puts one source, `twain_32\Fjicube\Fjic0400.ds`, and nothing
       under `twain_64`, so only the 32-bit worker lists this scanner; whether PaperStream IP (TWAIN x64) takes
       the plain fi-6130 is untried. The 32-bit DSM listed `PaperStream IP fi-6130dj` (family `PaperStream
       IP`, manufacturer `FUJITSU`, TWAIN 2.2) and the sample, not the two `.ds` files the package puts in
       `Fjicube\ExtraMode\` (`PSIPDiskImportMode.ds`, `PSIPReprocessMode.ds`). Its id, by the seam note's
       section 1: `twain32:PaperStream IP fi-6130dj`.
     - **Hidden, by callback.** `Ui::Hidden` scanned with no window shown, and `Delivery::Callback` brought
       `MSG_XFERREADY`: `crates/twain-dsm.md`'s open question (a visible parent, a modal enable, delivery by
       events) is answered for PaperStream IP; FUJITSU TWAIN32's waits for slice 12.
     - **Its sets.** `ICAP_XFERMECH` native, `CAP_XFERCOUNT` 1 and -1, `CAP_INDICATORS` off, `ICAP_PIXELTYPE`
       gray and both resolutions at 200 each answered `Exact`, and read back so; `CAP_FEEDERLOADED` read true
       before each enable. None outlasts the session: the next open read black and white at 300 dpi, the
       defaults.
     - **It opens with settings that aren't its defaults.** `CAP_DUPLEXENABLED` current true, default false;
       `ICAP_AUTODISCARDBLANKPAGES` current `TWBP_AUTO`, default `TWBP_DISABLE`. Neither was ever set from
       here: the driver keeps a configuration of its own. So the backend sets every capability the model
       covers, and the processing below, at every open, and takes neither the current value nor the default
       as what it wants.
     - **A batch, as it opens.** Seven sheets gave 11 transfers, the pending count 0xFFFF (`None`) after each
       until the last, as the sample's: duplex on, and three blank backs dropped. Every transfer had extended
       image info: `TWEI_PAGESIDE` `TWCS_TOP` (1) or `TWCS_BOTTOM` (2); `TWEI_PAGENUMBER` counting every side
       scanned, dropped ones too (1, 2, 3, 5, 7, 8, 9, 11, 12, 13, 14: 4, 6 and 10 the blanks); and
       `TWEI_DOCUMENTNUMBER` 1 throughout. So PaperStream IP reports the side (`SideKnown::Reported`; the seam
       note's section 4, FUJITSU TWAIN32's still open), and a gap in the page numbers says where a page was
       dropped. Dropping stays off in the backend all the same (section 2): blank pages are the app's call,
       through `scan-cleanup`, and not every driver says the side.
     - **Its own processing.** `ICAP_AUTOMATICBORDERDETECTION` is on, current and default: each page is
       cropped to its sheet, so the batch's letter sheets came 1692 to 1702 by 2201 to 2212, no two alike.
       `ICAP_AUTOMATICROTATE` is on too: the jammed sheet came back 2452 by 1269, wider than the feeder's 8.5
       inches (1700 at 200 dpi), so turned a quarter turn after the scan; the batch's pages all came upright.
       `ICAP_AUTOMATICDESKEW` reads off. Unsupported: `ICAP_AUTOSIZE`, `ICAP_AUTOMATICLENGTHDETECTION`,
       `ICAP_AUTOMATICCROPUSESFRAME`, `CAP_AUTOMATICSENSEMEDIUM`. `CAP_DOUBLEFEEDDETECTION` is an empty list,
       so nothing detects a double feed unless it's set (slice 12's double feed). `CAP_SUPPORTEDCAPS` lists
       230, 102 standard and 128 PFU's own (0x8000 and above), where the rest of PaperStream's processing
       lives. `Paper::Auto` is border detection here; rotation the model has no setting for, so the backend
       turns it off or the model gains one (slice 9's call).
     - **The DIB.** `DAT_IMAGEINFO` gave each page's size, 8 bits, gray, uncompressed, and each DIB agreed: a
       `BITMAPINFOHEADER`, 8 bits with a 256-entry palette, the block exactly its header, palette and rows
       (the first page's 3,759,764 bytes, each row padded to 1700).
     - **The driver writes to the process's standard output.** Its image-processing timings
       (`GetDocumentPosition took 32ms`, edges, rotation), about 25 lines a side, go to standard output and
       none to standard error (the streams apart, twice). C's buffer holds them and writes them in chunks of
       about 4 KB: in the batch twice mid-run and once at exit, one chunk ending mid-word (`GetMa`) with the
       program's next line written straight after it. In a worker that stream carries the protocol
       (`crates/scan-worker.md`; slice 4 builds it now), so a chunk can land between two messages or inside a
       page's bytes. The worker keeps its protocol on a handle of its own, and points standard output, both
       the Win32 handle and the C runtime's descriptor, at `NUL` (and `/dev/null` on Unix) before the DSM opens
       (`MSG_OPENDSM` loads every installed driver: the seam note's section 1).
     - **Not tried:** a memory transfer, a cancel, a jam the driver reports (the jammed sheet scanned all the
       same), an empty feeder: slice 9's checks and slice 12's.
10. **scan-device's `wia` backend:** its STA thread, the feeder and duplex, errors by code. Checked against a
    fake of the WIA calls behind a trait (no virtual WIA device installs without a person).
11. **The model frozen:** the seam note's unknowns all answered, `scan-device` and `scan-worker` out of 0.0.x
    (0.1.0), the protocol's version bumped once for it.
12. **By hand on Windows:** an fi-6130 on FUJITSU TWAIN32 through the 32-bit worker (and PaperStream IP x64
    if the office's are fi-6130Z), duplex, a jam, a double feed; a WIA scanner and a network copier. Waits on
    whether a development build may run on a work PC (M5 ask 12's open point; sheaf's
    `topics/code-signing.md`).

**Done when** the device crates pass on every runner, the model is frozen (slice 11), and the hand checks are
recorded. sheaf takes the device crates no sooner (`sheaf/swaps.md`).
