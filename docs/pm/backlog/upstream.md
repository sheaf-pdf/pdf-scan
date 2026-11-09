# Upstream: contributions to other projects

IDs UP-1 to UP-10; the next new one is **UP-11**. Format and rules: `README.md`.

Nothing is reported or offered upstream without Greg's OK (plan.md, "Standing rules"), so each is waiting on
him. None blocks a slice: the plan works around each. From the prior-art survey
(`research/prior-art/README.md`, 2026-11-08), and from S0 slice 1: UP-7 to UP-10, each seen on SANE 1.4.0,
the code behind it unchanged in sane-backends' master at `a113b257` (2026-11-08; read, as GPL code is, only to
study).

## For S0 (scan devices)

- **UP-7** · sane-backends (`test` backend) · S0 · waiting on review — **The `test` backend's resolution starts at 50/65536 dpi.** Its `resolution` is a `FIXED` option (a 16.16 word), but its default is the integer 50 (`init_resolution = 50`, where `SANE_FIX (50.0)` was meant), so a frontend that doesn't set it reads 0.000763 dpi (`scanimage -A` shows `[0.000762939]`) and scans 1×1 pixels: below the option's own range, 1 to 1200 dpi. A resolution given in its `test.conf` is read as fixed-point, so only the built-in default is wrong. A one-line fix or an issue. `sane-dl`'s tests set the resolution. *(S0 slice 1, Baselines)*
- **UP-8** · sane-backends (`test` backend) · S0 · waiting on review — **The `test` backend hangs now and then as a frame ends.** Built with threads, as Debian and Ubuntu build it, it reads each frame's picture from a reader thread through a pipe, and ends that thread with `pthread_cancel` (`sanei_thread_kill`) at the read that reaches the frame's end and at `sane_cancel`. Now and then the next frame's reader never writes and the frontend blocks reading the pipe for good. Seen on 1.4.0: about 1 in 2,000 cancels at once after a start, in one process; 1 to 4 processes of 12 or 24 reading 3,000 whole pages each, in parallel; none in 144,000 pages read 64 bytes at a time with a 1 ms pause after each frame's first read, which is how `sane-dl`'s tests read it. gdb sessions at the hang (S0 slice 1's step 4, their output not kept) showed the new reader blocked in `malloc`'s `get_free_list` and the frontend in `sane_test_read`; step 4 took the cause for an asynchronous cancel landing inside the allocator, but `sanei_thread` cancels in deferred mode, glibc's default, by its own comment, so the cause wants confirming, with backtraces kept, before a report. The probe is local, `scratch/work/s0-1/step-4/probe-hang/`. *(S0 slice 1, step 4)*
- **UP-9** · sane-backends (`test` backend) · S0 · waiting on review — **In non-blocking mode, an empty pipe at a frame's first read ends the frame, with a length of -1.** `read` gives -1 (`EAGAIN`), and the end-of-frame test adds it, as a `size_t`, to the bytes read so far, which wraps while none have been read: the frame is ended (its reader cancelled) and `sane_read` answers `GOOD` with `*length = -1`. The `EAGAIN` branch after it, which answers `GOOD` with no bytes, is reached only once some bytes have come. `sane-dl` refuses the length (`BadReadLength`), as it should; its non-blocking test lets the reader fill the pipe first. The fix: test for a negative `read` before the end-of-frame test. *(S0 slice 1, step 4)*
- **UP-10** · sane-backends (`scanimage`) · S0 · waiting on review — **`scanimage` aborts on 16-bit three-pass color.** It asserts that a red, green or blue frame is 8-bit (`assert (parm.depth == 8)` in `scan_it`), so a backend's 16-bit three-pass scan (the `test` backend's `three-pass` at depth 16) ends it by `SIGABRT` instead of writing the image or saying it can't; one-pass 16-bit color is written. The fix: interleave two bytes a sample at 16 bits, or refuse with a message. *(S0 slice 1, step 4's comparison with `scanimage`)*

## For S1 (pages)

- **UP-1** · imageproc · S1 · waiting on Greg — **Otsu and `adaptive_threshold` overflow `u32`.** `contrast::otsu_level` in 0.27.0 overflows once one gray level has more than 16,843,009 pixels (BUG-5); `adaptive_threshold`'s `u32` integral image overflows once a page's total brightness passes 2³². No issue is filed; PR #740 (float precision) was closed unmerged. An issue with both, or a PR in `u64`. *(survey, page cleanup)*
- **UP-2** · tiff (image-rs) · S1 · waiting on Greg — **Fax gaps in `tiff`.** `main`'s G3 path reads 1D only and never `T4Options` (PR #363); the Modified Huffman PR #414 has had no review since 2026-09-04; the G4 reader refuses dimensions past 65,535 (`u16::try_from`; `main` fixed height only). `scan-image` routes CCITT through hayro-ccitt regardless, so offer `T4Options` and `u32` G4 only when cheap. *(survey, image codecs)*
- **UP-3** · fax · S1 · open — **A release with #31.** The fix for the 32,767 px panic (issue #30) merged on 2026-11-05 and isn't released. Watch for 0.3.1; until then BUG-1 pins the commit or refuses wide rows. Asking for a release is a report, so Greg's OK first. *(survey, image codecs)*
- **UP-4** · rxing · S1 · waiting on Greg — **Negative tests, fuzzing, and the Data Matrix writer.** rxing has ZXing's false-positive tests only as untranslated Java, no fuzz targets, and CI that tests no feature combinations; its Data Matrix writer panics past 144×144 (#106; its fix PR #108 closed unmerged). `scan-fixtures` draws codes with rxing's encoders, and sheaf reads with it. Port the negative tests, add fuzz targets and a CI matrix. *(survey, patch codes and barcodes)*

## For S2 (PDF)

- **UP-5** · hayro (hayro-syntax, hayro-ccitt, hayro-jbig2, hayro-jpeg2000) · S2 · waiting on Greg — **A public filter API, and fuzz findings.** `Filter::apply` is crate-private and `Stream::decoded_image` widens 1-bit CCITT and JBIG2 to 8 bits: a public filter entry or a keep-1-bit option would let callers with their own parser use hayro's glue directly. And whatever TEST-4's fuzzing through our adapters finds, reported; none of the codecs is on OSS-Fuzz. *(survey, pdf images and writing; image codecs)*

## Later

- **UP-6** · objc2-image-capture-core · later · waiting on Greg — **Missing bindings, when `ica` is built.** The bindings are current (0.3.2) but raw; whatever the `ica` backend needs and finds missing goes upstream rather than into a private binding. *(survey, TWAIN, WIA and ICA)*
