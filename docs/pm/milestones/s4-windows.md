# S4: Windows

**TWAIN in both bitnesses, and WIA; the device model frozen.** Needs S3; a Windows machine and a scanner for
the hand checks.

1. **The spike, end to end:** TWAIN's sample data source installed silently on `windows-latest` (`msiexec
   /qn`, or yushulx/virtual-scanner's portable zip, or the DSM and the source copied into `twain_32` and
   `twain_64`) and scanned with a hidden window, in both bitnesses (TEST-3).
   - If it installs: a throwaway scan through it, the DSM loaded and walked from state 1 to 7, one memory
     transfer and one native, reading each transfer's image info, the pending-transfer count and whatever
     extended image info the source offers. It answers the seam note's TWAIN unknowns
     (`crates/scan-device-seam.md`) before `twain-dsm`'s API is fixed, and CI scans through TWAIN from
     then on.
   - If it doesn't: a desk check of the seam note against `TWAIN.cs` and NAPS2's notes (study only), and the
     hand checks carry the rest.
   - Either way the seam note is revised, said, before step 2.
2. **twain-dsm:** bindings ported from `TWAIN.cs` (MIT) and checked against `twain.h` 2.5, the state
   machine, the STA thread and hidden top-level window, capabilities, transfers, condition codes; built for
   both targets. It exposes what the seam needs (`crates/twain-dsm.md`, "For the seam").
3. **scan-device's `twain` backend** and a 32-bit worker in CI (`i686-pc-windows-msvc`, the worker alone),
   the client's merge of both lists. The model's changes the backend asks for are made here, in
   `scan-device`, the `fake` and `sane` backends and the worker's protocol together. Checked: 10 images
   through the sample source in both bitnesses (if the spike worked); the error mapping in unit tests; S3's
   checks again, on the changed model.
4. **scan-device's `wia` backend:** its STA thread, the feeder and duplex, errors by code. Checked against a
   fake of the WIA calls behind a trait (no virtual WIA device installs without a person).
5. **The model frozen:** the seam note's unknowns all answered, `scan-device` and `scan-worker` out of 0.0.x
   (0.1.0), the protocol's version bumped once for it.
6. **By hand:** an fi-6130 on FUJITSU TWAIN32 through the 32-bit worker (and PaperStream IP x64 if the
   office's are fi-6130Z), duplex, a jam, a double feed; a WIA scanner and a network copier. Waits on whether
   a development build may run on a work PC (M5 ask 12's open point; sheaf's `topics/code-signing.md`).

**Done when** both backends pass on the Windows runners, the model is frozen, and the hand checks are
recorded. sheaf takes the device crates no sooner (`sheaf/swaps.md`).
