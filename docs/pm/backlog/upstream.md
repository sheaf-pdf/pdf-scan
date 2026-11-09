# Upstream: contributions to other projects

IDs UP-1 to UP-6; the next new one is **UP-7**. Format and rules: `README.md`.

Nothing is reported or offered upstream without Greg's OK (plan.md, "Standing rules"), so each is waiting on
him. None blocks a slice: the plan works around each. From the prior-art survey
(`research/prior-art/README.md`, 2026-11-08).

## For S1 (pages)

- **UP-1** · imageproc · S1 · waiting on Greg — **Otsu and `adaptive_threshold` overflow `u32`.** `contrast::otsu_level` in 0.27.0 overflows once one gray level has more than 16,843,009 pixels (BUG-5); `adaptive_threshold`'s `u32` integral image overflows once a page's total brightness passes 2³². No issue is filed; PR #740 (float precision) was closed unmerged. An issue with both, or a PR in `u64`. *(survey, page cleanup)*
- **UP-2** · tiff (image-rs) · S1 · waiting on Greg — **Fax gaps in `tiff`.** `main`'s G3 path reads 1D only and never `T4Options` (PR #363); the Modified Huffman PR #414 has had no review since 2026-09-04; the G4 reader refuses dimensions past 65,535 (`u16::try_from`; `main` fixed height only). `scan-image` routes CCITT through hayro-ccitt regardless, so offer `T4Options` and `u32` G4 only when cheap. *(survey, image codecs)*
- **UP-3** · fax · S1 · open — **A release with #31.** The fix for the 32,767 px panic (issue #30) merged on 2026-11-05 and isn't released. Watch for 0.3.1; until then BUG-1 pins the commit or refuses wide rows. Asking for a release is a report, so Greg's OK first. *(survey, image codecs)*
- **UP-4** · rxing · S1 · waiting on Greg — **Negative tests, fuzzing, and the Data Matrix writer.** rxing has ZXing's false-positive tests only as untranslated Java, no fuzz targets, and CI that tests no feature combinations; its Data Matrix writer panics past 144×144 (#106; its fix PR #108 closed unmerged). `scan-fixtures` draws codes with rxing's encoders, and sheaf reads with it. Port the negative tests, add fuzz targets and a CI matrix. *(survey, patch codes and barcodes)*

## For S2 (PDF)

- **UP-5** · hayro (hayro-syntax, hayro-ccitt, hayro-jbig2, hayro-jpeg2000) · S2 · waiting on Greg — **A public filter API, and fuzz findings.** `Filter::apply` is crate-private and `Stream::decoded_image` widens 1-bit CCITT and JBIG2 to 8 bits: a public filter entry or a keep-1-bit option would let callers with their own parser use hayro's glue directly. And whatever TEST-4's fuzzing through our adapters finds, reported; none of the codecs is on OSS-Fuzz. *(survey, pdf images and writing; image codecs)*

## Later

- **UP-6** · objc2-image-capture-core · later · waiting on Greg — **Missing bindings, when `ica` is built.** The bindings are current (0.3.2) but raw; whatever the `ica` backend needs and finds missing goes upstream rather than into a private binding. *(survey, TWAIN, WIA and ICA)*
