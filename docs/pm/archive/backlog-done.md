# Backlog items done

Each moved here from `backlog/` when a slice closed it, with "Done: where" before its source
(`backlog/README.md`, "Keeping it"). Nothing here is edited after it's moved in.

## Tests

- **TEST-3** · twain-dsm · S0 · done — **TWAIN's sample source on a hosted runner.** Whether `Twain_DS_sample01` (BSD-3) installs silently on `windows-latest` (`msiexec /qn`), or runs from yushulx/virtual-scanner's portable zip, or works with the DSM and the source copied into `twain_32` and `twain_64` (if the runner lets us write `C:\Windows\twain_*`); whether it scans with a hidden window in both bitnesses; `CAP_XFERCOUNT` from its environment variable sets the page count. **S0 slice 7, 2026-11-08:** the MSI installs silently there (`scripts/twain-sample.ps1`; CI's `twain-spike` job, step 1) and the DSM lists the source with a hidden window in both bitnesses; it scans so here, `CAP_XFERCOUNT` setting the count (2 when unset, the code's default, not the README's 1). CI then ran `twain-spike`'s scenarios there in both bitnesses, green on every push of the slice (2026-11-08; runs 42557972395 (step 1, `list`), 42559459205 (step 2, six scenarios), 42559970065 (step 3, seven), 42560458700, 42560706170 and 42561129071 (docs)): it scans with a hidden window on the runner. **Done:** S0 slice 7 (`slices/s0-7-twain-spike.md`), 2026-11-08. *(survey, TWAIN, WIA and ICA)*
