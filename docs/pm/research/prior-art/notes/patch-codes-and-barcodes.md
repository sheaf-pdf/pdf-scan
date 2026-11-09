# Prior art: Kodak patch codes, barcodes on scanned pages, and separator sheets

As of 2026-11-08. This checks and extends the 2026-10-23 audit: `scratch/reference/sheaf/research/build-or-adopt/12-kodak-patch-codes.md`, sheaf's `plan/research/build-or-adopt/10-barcode-reading.md` and `11-barcode-writing.md`, and `scratch/reference/sheaf/m05-scan/research/separators.md`, `reading-writing-codes.md` and proposal p14. Each finding below that changes the audit starts with **Correction** or **New**. Versions, dates and download counts come from the crates.io API, read 2026-11-08 (generic User-Agent). Crate sources were read in place under `~/.cargo/registry/src/` or downloaded into the scratchpad. Nothing was built or run.

What the crates need (`docs/pm/crates.md`, `patch-codes`; `docs/pm/plan.md`, "Not in the family"):

- `patch-codes` finds Kodak patch codes 1, 2, 3, 4, 6 and T along any edge, reports the edge, and draws them in A-61599's geometry, on `image` alone.
- The barcode reader stays in sheaf, behind a small trait (rxing first, zxing-cpp a possible second engine). Separator sheets also need a barcode writer.

## Q1. Kodak patch codes: open-source readers, commercial SDKs, and the specification

### Takeaway
No open-source patch code reader or writer turned up, under any license or in any language. crates.io, npm and NuGet have none, and the web and GitHub searches found only the sheaf PR itself. ZXing, zxing-cpp, rxing, zedbar, bardecoder and barcoders have no patch code support. About eight closed commercial .NET/C++/JS SDKs read them, and none publishes its method. Kodak's A-61599 (October 2006) is freely downloadable, but the only terms it states are a Kodak copyright notice. Its zone tables confirm sheaf's bar table exactly. They also show that the "at least 0.75 in long" figure belongs to Kodak's *perpendicular*-bar layout (i280, 3590C, i600), which sheaf's reader does not read; for the parallel-bar Patch Reader layout the minimum is 2 in.

### Cited Findings
**Open-source search (none found):**
- crates.io searches for "patch code", "patchcode", "patch-code" and "kodak" return nothing related: `kodak` 0.0.1 is an image-manipulation crate and `sighook` is an instrumentation crate — [crates.io API search](https://crates.io/api/v1/crates?q=patchcode)
- npm searches for "patchcode", "patch code kodak" and "kodak patch" return no patch code reader. The only barcode hits are generic: zxing-wasm, barcode-detector, bwip-js, and a Dynamsoft Capacitor plugin — [npm registry search](https://registry.npmjs.org/-/v1/search?text=patchcode)
- NuGet "patchcode" hits are all commercial: Aspose.BarCode 26.10.0, Dynamsoft.DotNet.BarcodeReader.Bundle 11.6.5000, BarcodeLib.BarcodeReader.*.PatchCode 7.0.0.78 (BarcodeLib.com's commercial reader, not the MIT BarcodeLib), RasterEdge/XImage, OnBarcode samples, and Bytescout.BarCode.Reader — [NuGet search API](https://azuresearch-usnc.nuget.org/query?q=patchcode)
- A web search for "Kodak patch code reader open source github" finds only the sheaf-pdf/sheaf PR #47, i.e. the in-house reader — [GitHub PR](https://github.com/sheaf-pdf/sheaf/pull/47)
- A grep of zxing-cpp 0.5.3 (core and bundled zint), rxing 0.9.3, zedbar 0.7.0, bardecoder 0.5.0 and barcoders 2.0.0 for `patch.?code|patchcode|PATCHT` finds nothing. None of them supports the symbology — local sources in `~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/` ([zxing-cpp crate](https://crates.io/crates/zxing-cpp), [rxing crate](https://crates.io/crates/rxing))
- NAPS2 (GPL, study only) does not read patch bars. `BarcodeDetector.cs` runs ZXing.Net with `TryHarder = true` and, in "Patch-T only" mode, `PossibleFormats = [CODE_39]`. It reads the Code 39 text printed on patch sheets, not the bars — [NAPS2 BarcodeDetector.cs](https://github.com/cyanfish/naps2/blob/master/NAPS2.Sdk/Scan/BarcodeDetector.cs)
- An old NAPS2 tree had a `PatchCode.cs` type, the enum of which code was detected, which a SourceForge thread cites. I did not open it (GPL, study only) — [NAPS2 SourceForge thread](https://sourceforge.net/p/naps2/discussion/general/thread/b7d74806/)

**Commercial SDKs that read patch codes, and what they claim:**
- Accusoft Barcode Xpress (Java v14.0 docs):
  - six types with bit patterns 1 = 1100, 2 = 1001, 3 = 1010, 4/Toggle = 0110, 6 = 0011, T/Transfer = 0101;
  - "fixed-length, binary 1D barcode with no checksum";
  - "minimum bar width for Patch Codes is 5";
  - a quiet zone "at least the width of 10 narrow bars";
  - "Patch Codes must be analyzed separately. Other barcodes cannot be analyzed at the same time."
  - Docs for v9–13.x list only Patch 1–3.
  - Source: [Accusoft Barcode Xpress Java: Patch Code](https://help.accusoft.com/BarcodeXpress/latest/BxJava/patch-code.html)
- Dynamsoft Barcode Reader added PatchCode in v7.2, released 2019-09-25 — [ComponentSource release note](https://www.componentsource.com/news/feature-releases/2019/09/25/barcode-reader). Its page claims:
  - a patch code and a Code 39 or Code 128 are read from one page image;
  - sheets carry patches on all four edges and read "even if the page is upside down or rotated";
  - missed reads come from resolution below 200 DPI, excessive skew, low contrast from photocopied separators, and wrong bar sizes;
  - no accuracy figures.
  - Source: [Dynamsoft Patch Code page](https://www.dynamsoft.com/Barcode-Types/Patch-Code.aspx)
- A later Dynamsoft release note (reported by the search summary, not opened) says PatchCode decoding was optimized to find codes "far away from the edge of the image" — [Dynamsoft Web TWAIN FAQ](https://www.dynamsoft.com/web-twain/docs/faq/does-barcode-addon-support-patch-code.html)
- LEADTOOLS has `BarcodeSymbology.PatchCode` with `PatchCodeBarcodeReadOptions`. Its options are `SearchDirection` ("horizontal, vertical or both") and `Granularity` (scan lines to skip), and a writer exists — [LEADTOOLS PatchCodeBarcodeReadOptions](https://www.leadtools.com/help/sdk/dh/ba/patchcodebarcodereadoptions.html)
- Aspose.BarCode for .NET reads PatchCode; 6.5.0 improved PatchCode recognition, including JPEG input. A later release added encoding — [Aspose blog 6.5.0](https://blog2.aspose.com/?p=16653), [Aspose patch code generation docs](https://docs.aspose.com/barcode/net/patch-code-barcodes/)
- VintaSoft Barcode .NET SDK 2.2 added reading of patch codes 1, 2, 3, 4, 6 and T — [VintaSoft forum](https://www.vintasoft.com/forums/viewtopic.php?p=71)
- Other readers:
  - RasterEdge ("any orientation") — [RasterEdge](https://www.rasteredge.com/how-to/csharp-imaging/barcode-reading-patchcode/)
  - OnBarcode — [OnBarcode](https://onbarcode.com/csharp/patch-code-reader.html)
  - ByteScout, whose vendor says the SDK is being sunset — [ByteScout](https://bytescout.com/articles/barcode-reader-sdk-c-decode-patch-code)
  - Nutrient GdPicture.NET 14 (`PatchCodeReaderGetCodeType`) — [GdPicture API](https://www.nutrient.io/api/gdpicture/GdPicture.NET.14~GdPicture14.GdPictureImaging~PatchCodeReaderGetCodeType.html)
- Not found: Atalasoft, Inlite ClearImage, ABBYY FineReader Engine and Kofax VRS patch code reading. A search named them and returned nothing (see Gaps).

**Kodak's specification (A-61599):**
- Identity:
  - "Patch Code Information for Kodak Scanners", A-61599, CAT No. 845 9380, October 2006, Eastman Kodak Company Document Imaging, "© Kodak, 2006".
  - No license or reproduction terms are stated.
  - The packet says its camera-ready patch pages "adhere to the specifications provided and may be used by a printer when creating patch documents."
  - Source: [A-61599 copy on archive.org](https://archive.org/download/manualsonline-id-e24f73ef-f1cf-4f31-bc70-8aae8572f136/e24f73ef-f1cf-4f31-bc70-8aae8572f136.pdf). Wikipedia cites the Kodak original via the Wayback Machine: [Wikipedia: Patch Code](https://en.wikipedia.org/wiki/Patch_Code)
- Dimensions:
  - wide bars 0.20 in (5 mm) ±0.01 in;
  - narrow bars and spaces 0.08 in (2.03 mm) ±0.01 in;
  - maximum patch width 0.80 in (20 mm) ±0.01 in;
  - "The minimum overall length of the patch bars is 2 inches (50 mm)" for the Patch Reader scanners (9XYZ, 5XYZ, 7XYZ, i800/i1800 with image addressing, Microimager 70);
  - a "standard 2.5-inch (62.5 mm) patch code is recommended."
  - Source: [A-61599](https://archive.org/download/manualsonline-id-e24f73ef-f1cf-4f31-bc70-8aae8572f136/e24f73ef-f1cf-4f31-bc70-8aae8572f136.pdf)
- Two orientations:
  - The Patch Reader "recognizes patches with bars that are parallel to the lead edge".
  - The i280, 3590C and i600/i1800 (without image addressing) "recognize patches with bars that are perpendicular to the lead edge" and "will only recognize the toggle patch".
  - For that perpendicular layout:
    - "The minimum overall length of the patch bars is 0.75 inches (19 mm)";
    - "The Patch Reading Area ends 2.0 inches from the lead edge … At least 0.75 inches of the patch code must appear within the Patch Reading Area";
    - the patch must be at least 0.5 in from the lead edge.
  - Source: [A-61599](https://archive.org/download/manualsonline-id-e24f73ef-f1cf-4f31-bc70-8aae8572f136/e24f73ef-f1cf-4f31-bc70-8aae8572f136.pdf)
- Placement and print quality:
  - parallel layout: at least 0.25 in (6 mm) from the right, left and leading edges, and no more than 3.75 in (94 mm) from the leading edge (1.6 in on 5XYZ/7XYZ);
  - at least 0.25 in from any other print;
  - bars reflect "less than 20%" and paper "at least 65%";
  - "Avoid photocopying patches";
  - carbon-based ink, not soy-based, and no glossy paper.
  - Source: [A-61599](https://archive.org/download/manualsonline-id-e24f73ef-f1cf-4f31-bc70-8aae8572f136/e24f73ef-f1cf-4f31-bc70-8aae8572f136.pdf)
- Zone tables (bar and space boundaries in inches, from the first edge):
  - Patch 1: 0.20/0.28/0.48/0.56/0.64/0.72/0.80
  - Patch 2: 0.20/0.28/0.36/0.44/0.52/0.60/0.80
  - Patch 3: 0.20/0.28/0.36/0.44/0.64/0.72/0.80
  - Patch 4: 0.08/0.16/0.36/0.44/0.64/0.72/0.80
  - Patch 6: 0.08/0.16/0.24/0.32/0.52/0.60/0.80
  - Patch T: 0.08/0.16/0.36/0.44/0.52/0.60/0.80
  - Source: [A-61599](https://archive.org/download/manualsonline-id-e24f73ef-f1cf-4f31-bc70-8aae8572f136/e24f73ef-f1cf-4f31-bc70-8aae8572f136.pdf)
- Functions:
  - Patch 2 "Assigns image level 2 to the current document"; Patch 3 assigns level 3;
  - Patch T "Assigns a predefined image level to the next document";
  - types 1, 4 and 6 are "post-scan image control" on the i800/i1800;
  - the Toggle patch switches bi-tonal/colour ("Color on the Fly").
  - Source: [A-61599](https://archive.org/download/manualsonline-id-e24f73ef-f1cf-4f31-bc70-8aae8572f136/e24f73ef-f1cf-4f31-bc70-8aae8572f136.pdf)
- A newer Kodak Alaris document, A-61801 "Patch Code Information" for the i5000 series (February 2013 per the search snippet), exists. Its snippet repeats "The patch reading area ends 2.0 inches from the lead edge." Kodak Alaris's server returned "Access Denied" and 404 to my fetches, so I did not read it — [A-61801 URL](https://www.alarisworld.com/-/media/files/di/uploadedfiles/a61801_i5000_patchcodeinfo_en.pdf)
- Patch Code is Kodak's own format. None of these sources calls it an ISO/AIM symbology: Accusoft calls it a binary 1D code without a checksum, LEADTOOLS a "miscellaneous" symbology — [Wikipedia](https://en.wikipedia.org/wiki/Patch_Code), [Accusoft](https://help.accusoft.com/BarcodeXpress/latest/BxJava/patch-code.html)

**Sheaf's existing reader (for comparison):**
- The doc comment of `sheaf-image/src/codes/patch.rs` (362 lines):
  - says the reader works at about 75 dpi, at an Otsu level, with lines about 0.04 in apart across 4.7 in bands on each edge;
  - accepts patches at 0.6 in long;
  - "Bars across the leading edge (Kodak's i280 layout) aren't read";
  - its bar table (1 WWNN, 2 WNNW, 3 WNWN, 4 NWWN, 6 NNWW, T NWNW) matches A-61599's zone tables and Accusoft's bit patterns.
  - Source: local file `https://github.com/sheaf-pdf/sheaf/blob/3232a35/crates/sheaf-image/src/codes/patch.rs`

### Inferences
- **Confirmed:** the audit's "no open-source detector" holds, and the search now covers npm and NuGet as well. The only prior art is closed commercial code. Nothing permissive exists to port or contribute to, so `patch-codes` stays build our own.
- **Correction (small):** the audit and `patch.rs` say "at least 0.75 in long (2 in for Kodak's Patch-Reader scanners)" next to "within 3.75 in of the leading edge". In A-61599 these belong to two different layouts:
  - 0.75 in and the 2.0 in reading area go with the perpendicular i280/3590C/i600 layout;
  - 2 in (2.5 in recommended) and the 3.75 in band go with the parallel layout.
  - The reader's 0.6 in floor is still a sensible tolerance, but the doc comment should keep the two sets of figures apart.
- **The ±0.01 in tolerance is tight:** wide 0.19–0.21, narrow 0.07–0.09. The reader's split of wide over 0.14 in sits at the midpoint between 0.08 and 0.20, which is right for degraded scans. The thing A-61599 warns about (photocopies fatten bars) argues for keeping the threshold relative rather than absolute.
- **Two design inputs from the commercial products:**
  - LEADTOOLS exposes "horizontal, vertical or both" search directions, a setting for the perpendicular layout the in-house reader skips.
  - Accusoft documents a quiet zone of 10 narrow bars (0.8 in), much wider than the reader's 0.18 in. Accusoft's figure is for its own reader, not Kodak's spec; A-61599 asks only 0.25 in from other print.
- **The specification can be cited and followed:** it is public, but no license is stated. Republishing the PDF is not ours to do; the geometry can be cited and implemented. This is an inference, not legal advice.

### Gaps
- I could not confirm whether Atalasoft DotImage, Inlite ClearImage, ABBYY FineReader Engine/FlexiCapture or Kofax VRS read patch codes in software. One search turned up nothing for them, so their absence above is not evidence that they lack it.
- I could not read Kodak Alaris's current A-61801 (2013) or check whether a newer patch specification replaces A-61599: Access Denied/404 from kodakalaris.com.
- Dynamsoft and the other vendors publish no accuracy numbers on patch codes, so there is no external benchmark to compare with 204/204 and 60/60.

## Q2. Scanner-side patch code detection (TWAIN, WIA, SANE, Fujitsu fi-series)

### Takeaway
TWAIN 2.x and WIA 2.0 both define patch code detection with per-image or per-job results. SANE defines nothing: the fujitsu and kodak backends carry protocol constants for patch fields but expose no option or metadata. Fujitsu's PaperStream IP detects patch codes in the host driver; PaperStream Capture consumes that result "for TWAIN only", and the sheets are printed from the driver. Per-model support on the fi-6130, fi-7160 and fi-8170 is not documented in what I could read. Driver-reported codes can be an optional fast path in a device backend, but pixel reading is the only path that works on every platform and source.

### Cited Findings
**TWAIN 2.5 (`twain.h`, Jan 2021):**
- Capabilities:
  - `ICAP_PATCHCODEDETECTIONENABLED` 0x113f, `ICAP_SUPPORTEDPATCHCODETYPES` 0x1140, `ICAP_PATCHCODEMAXSEARCHPRIORITIES` 0x1141, `ICAP_PATCHCODESEARCHPRIORITIES` 0x1142, `ICAP_PATCHCODESEARCHMODE` 0x1143, `ICAP_PATCHCODEMAXRETRIES` 0x1144, `ICAP_PATCHCODETIMEOUT` 0x1145.
  - The capability is ICAP_, not CAP_ as the assignment wrote it.
- Results: extended image info `TWEI_PATCHCODE` 0x1212, with values `TWPCH_PATCH1`…`TWPCH_PATCH4` = 0–3, `TWPCH_PATCH6` = 4, `TWPCH_PATCHT` = 5.
- Related:
  - `CAP_JOBCONTROL` 0x1017 (`TWJC_NONE/JSIC/JSIS/JSXC/JSXS`), and `TWEJ_PATCH1…TWEJ_PATCHT` (0x0002–0x0007) plus `TWEJ_MIDSEPARATOR`;
  - `CAP_PRINTERINDEXTRIGGER` values `TWCT_PATCH*` (added 2.3);
  - `TWAL_PATCHCODE` alarm.
- Barcode equivalents:
  - `ICAP_BARCODEDETECTIONENABLED` 0x1137 and friends;
  - `TWEI_BARCODETEXT/TYPE/COUNT/CONFIDENCE/ROTATION/X/Y`, and `TWEI_BARCODETEXT2` 0x1253;
  - `TWBT_*` types run from `TWBT_3OF9` (0) to `TWBT_QRCODE` (20). Data Matrix and Aztec are not in the standard list.
- Source: [twain-dsm twain.h](https://github.com/twain/twain-dsm/blob/master/TWAIN_DSM/src/twain.h)
- CAP_JOBCONTROL (TWAIN 1.7):
  - JSIC/JSIS keep the separator and JSXC/JSXS exclude it; C continues scanning, S stops.
  - The default after reset is `TWJC_NONE`.
  - The application should "check the EOJ field for one of the TWEJ_xxx patch codes of the PENDINGXFERS data."
  - Source: [Asprise CAP_JOBCONTROL](https://asprise.com/twain/CAP_JOBCONTROL)
- Application side: set `ICAP_PATCHCODEDETECTIONENABLED` true, then request `TWEI_PATCHCODE` through extended image info for each image — [VintaSoft TWAIN how-to](https://www.vintasoft.com/docs/vstwain-dotnet/Programming-Twain-How_To_Enable_Patch_Code_Detection_For_Twain_Image_Scanner.html); TWEI_PATCHCODE values — [Accusoft TwainPro constants](https://help.accusoft.com/TwainPro/v8.0/activex/AccusoftTwainPro8~TP_ExtInfoConstants.html)

**WIA 2.0:**
- A "Patch Code Reader" item exists:
  - `WIA_IPS_PATCH_CODE_READER` (VT_I4): `WIA_PATCH_CODE_READER_DISABLED`, the "required default", or `WIA_PATCH_CODE_READER_AUTO`, where "the patch code reader location is automatically selected by the device at runtime depending on the active scan input source";
  - metadata transfers as `WiaImgFmt_XmlPat` (XML per the "WIA Patch Code Metadata Schema") or `WiaImgFmt_RawPat`.
  - Source: [Microsoft Learn: WIA_IPS_PATCH_CODE_READER](https://learn.microsoft.com/en-us/windows-hardware/drivers/image/wia-ips-patch-code-reader)
- `WIA_IPS_SUPPORTED_PATCH_CODE_TYPES` (read-only vector) lists `WIA_PATCH_CODE_1, _2, _3, _4, _6, _T` and `WIA_PATCH_CODE_CUSTOM_BASE + N`. It is "required for all Patch Code Reader items" — [Microsoft Learn](https://learn.microsoft.com/en-us/windows-hardware/drivers/image/wia-ips-supported-patch-code-types)
- A `WIA_IPS_PATCH_CODE_SEARCH_DIRECTION` property exists — [Microsoft Learn](https://learn.microsoft.com/lb-lu/windows-hardware/drivers/image/wia-ips-patch-code-search-direction)
- Per the search summary, the raw format's `WIA_PATCH_CODES` header carries a 'WBAT' tag and version 0x00010000, lists codes in decode order, and is returned once per scan job — [NtDoc WIA_PATCH_CODES](https://ntdoc.m417z.com/wia_patch_codes)

**SANE (sane-backends master, read 2026-11-08):**
- fujitsu: `fujitsu-scsi.h` defines `MS_pc_patch 0x2e /* Patch code scanning */` as a mode-select page, but `fujitsu.c` never uses it. `fujitsu.c` reads an inquiry bit into `has_barcode` and only prints it, and `fujitsu.h` has `/*FIXME: barcode data? */` — [fujitsu-scsi.h](https://gitlab.com/sane-project/backends/-/blob/master/backend/fujitsu-scsi.h), [fujitsu.h](https://gitlab.com/sane-project/backends/-/blob/master/backend/fujitsu.h)
- kodak: `kodak-cmd.h` has accessors for an image-header patch type (`get_SR_ih_patch_type`) and patch settings (`get_SR_sc_T1_patch`…`T6`, `trans_patch`). `kodak.c` only logs the inquiry "Patch" bit — [kodak-cmd.h](https://gitlab.com/sane-project/backends/-/blob/master/backend/kodak-cmd.h), [kodak.c](https://gitlab.com/sane-project/backends/-/blob/master/backend/kodak.c)
- canon_dr, avision and the Panasonic kvs1025/kvs20xx/kvs40xx option files contain no "patch" or "barcode" at all — [canon_dr.c](https://gitlab.com/sane-project/backends/-/blob/master/backend/canon_dr.c)

**Fujitsu / Ricoh PFU:**
- PaperStream Capture 2.10's guide:
  - "Print the required number of patch code sheets provided by the PaperStream IP driver";
  - the profile's Separation tab offers "[Patch Code] for [Detect Doc Separator]" and a "[Delete Separator]" check box;
  - with the PaperStream IP (ISIS) driver only Patch1, Patch2, Patch3 and PatchT can be used.
  - Source: [PaperStream Capture 2.10 User's Guide P2WW-4050-06ENZ0](https://origin.pfultd.com/downloads/IMAGE/manual/psc/P2WW-4050-06ENZ0.pdf)
- PaperStream Capture 2.1.0 "Added the feature to split the batch by using [Patch Code Detection] of PaperStream IP (for TWAIN only)". It also dropped cutout job separator sheets: "Please use a different separation method such as Patch code" — [PaperStream Capture change history](https://www.pfu.ricoh.com/global/scanners/fi/dl/update/history/psc-250-change.pdf)
- The PaperStream IP (TWAIN x64) change history, latest 3.40.1:
  - says nothing about patch codes;
  - says "The 2D Barcode option is now included in the driver installer";
  - earlier versions added Data Matrix (1.7.0) and Aztec (1.10.0) to Barcode Detection, the latter requiring "2D Barcode for PaperStream 1.1.0".
  - Source: [PSIP TWAIN x64 change history 3.40.1](https://www.pfu.ricoh.com/global/scanners/fi/dl/setup/history/ps-ip-twain64-3401-change.pdf)
- University of Wisconsin KB (old): in PaperStream IP, barcode and patch code detection should be "turned off" globally and enabled only within the scanning profile, because "barcode and patch code pages will not be available in ImageNow if detection is enabled within PaperStream IP" — [UW KB 41423](https://kb.wisc.edu/ecms/41423), [UW KB 81458](https://kb.wisc.edu/helpdesk/81458)
- Kodak Capture Pro (A-61993, March 2022): "When patch code reading is enabled on a Kodak i800/i1800 Series Scanner, patch code reading with Capture Pro Software is not allowed", and the reverse. Kodak has both a hardware reader (the i800/i1800 Patch Reader) and a software one — [Capture Pro Admin Guide A-61993](https://www.kodakalaris.com/sites/default/files/2024-11/a61957_capture_pro_admin_guide_en.pdf)

**Rust:**
- The only `twain` crate is 0.1.0 (2021-07-24, 2,001 downloads). I did not inspect it — [crates.io twain](https://crates.io/crates/twain)

### Inferences
- **Confirmed and extended:** the audit's "SANE's fujitsu, kodak and canon_dr backends expose none" holds; the source shows unused constants, not options. **New:** WIA 2.0 does define patch code metadata, which the audit didn't mention.
- A `scan-device` backend could surface `TWEI_PATCHCODE` and `TW_PENDINGXFERS.EOJ` (TWAIN) or the WIA Patch Code Reader item's XML/raw metadata as an optional "device says patch N" hint. It cannot replace image analysis:
  - SANE has nothing;
  - drivers that remove the separator (TWJC_JSX*, PaperStream's Delete Separator) hide the page;
  - neither TWAIN nor WIA reports which edge the code lies along.
- Fujitsu's PaperStream IP prints its own patch sheets and detects patch codes in host software, not a firmware reader. That is consistent with the PaperStream Capture wording. The SANE fujitsu mode page 0x2e suggests some fi-series firmware has a patch scanning mode, but nothing confirms which models.

### Gaps
- Not found: per-model statements that the fi-6130, fi-7160 or fi-8170 report `TWEI_PATCHCODE` through PaperStream IP. The fi-7160 datasheet lists "barcode, patch" indexing only in general terms (search summary), and nothing turned up for the fi-8170.
- I did not find which Windows version introduced WIA's Patch Code Reader item. Windows 7's WIA 2.0 is likely but unverified.
- I did not read the TWAIN 2.5 specification's prose for ICAP_PATCHCODESEARCHMODE and the priorities; I have only the header constants.

## Q3. Barcode reading in Rust (rxing, zxing-cpp, rqrr, bardecoder, quircs, zbar, others)

### Takeaway
rxing is still the only pure-Rust, permissive, multi-symbology reader with real users. It is active (0.9.4 shipped today, outside contributors are fixing bugs every week, and v0.10 is planned for December to lift the exact `image` pin). Its weak spots are documented:

- no ported negative (false-positive) tests;
- no fuzzing;
- CI that doesn't test feature combinations;
- panics and hangs found by users throughout 2026;
- in zxing-cpp's author's benchmark, false reads on 10 of 25 no-code images with every common format on, and about 6.7x slower than zxing-cpp on a hard QR set.

zxing-cpp reads more and faster but is C++20, built with CMake behind a 48-`unsafe` FFI. Two new pure-Rust candidates the audit missed, `anyd` (MIT, from scratch, `forbid(unsafe_code)`) and `tapirscan` (MIT/Apache, generated Rust), are too new to adopt but worth a bench run.

### Cited Findings
**rxing:**
- **Correction:** the latest is **0.9.4, released 2026-11-08**, not 0.9.3:
  - Apache-2.0; MSRV 1.85; 788,543 downloads (241,799 recent); 35 reverse dependencies.
  - Repo: 334 stars, 20 open issues, pushed 2026-11-08.
  - Sources: [crates.io rxing](https://crates.io/crates/rxing), [crates.io API reverse deps](https://crates.io/api/v1/crates/rxing/reverse_dependencies), [GitHub rxing-core/rxing](https://github.com/rxing-core/rxing)
- What changed in 0.9.4 (commits 2026-09-22 to 2026-11-08):
  - Code 128: "remove artificial limit to 80 chars"; "ignore function code effects from Code 128 checksums"; "allow spaces in forced Code128 B".
  - Data Matrix: Base 256 bytes appended unchanged; ECIs above 899 treated as unknown.
  - PDF417: ECI 925/926 handled.
  - QR: honours the CharacterSet hint; guesses byte-mode charsets defaulting to ISO-8859-1.
  - MaxiCode: no longer loops forever on images narrower than 13 px.
  - imageproc pulled with `default-features = false`, and a new `rayon` feature that the defaults now turn on.
  - Source: [rxing commits](https://github.com/rxing-core/rxing/commits/main)
- **Still true in 0.9.4:** `image = "=0.25.8"` (exact) and `imageproc = "0.25"`, both optional and behind the `image` feature. sheaf's lockfile has image 0.25.10 and imageproc 0.27.0, so sheaf must keep `default-features = false` as it does today. Its sheaf-image `Cargo.toml` has `rxing = "0.9.3"` with `decoders, encoders, encoding_rs, oned, qrcode, datamatrix, pdf417`. Sources: [rxing 0.9.4 Cargo.toml](https://docs.rs/crate/rxing/0.9.4/source/Cargo.toml.orig); local `https://github.com/sheaf-pdf/sheaf/blob/3232a35/crates/sheaf-image/Cargo.toml` and `Cargo.lock`
- The audit's two image versions don't conflict: 10-barcode-reading's "sheaf is on 0.27" is imageproc, and reading-writing-codes' "0.25.10" is image. Both are right — local lockfile above.
- The pin's cause and plan:
  - the maintainer says the pin exists because "later versions [are incompatible] with the msrv";
  - on 2026-11-08 he wrote that it "is going to become part of v0.10.x … Minor versions will get a pinned rust version … v0.10.0 should be ready sometime in December";
  - PR #121 (bump to 0.25.10) is still open.
  - Sources: [issue #87](https://github.com/rxing-core/rxing/issues/87), [PR #121](https://github.com/rxing-core/rxing/pull/121)
- Coverage: rxing is "a port of the ZXing Java barcode library to pure Rust, converted by hand", with "enhancements to the QR Code, Datamatrix, and DX Film reader components" from zxing-cpp.
  - The README says: "Porting of the testing library is incomplete. Currently all positive tests are implemented. Negative verification tests are not implemented."
  - Decoding covers Aztec, Data Matrix, MaxiCode, PDF417, QR, Codabar, Code 39/93/128, ITF, EAN-8/13, UPC-A/E, RSS-14, RSS-Expanded, Telepen, Micro QR, rMQR and DX Film. MaxiCode, RSS, Micro QR, rMQR and DX Film have no encoder.
  - Source: [rxing README](https://github.com/rxing-core/rxing/blob/main/README.md)
- Tests, fuzzing and CI:
  - The packaged crate carries ZXing's negative tests only as untranslated Java (`tests/negative/FalsePositivesBlackBoxTestCase.java`, `FalsePositives2…`, `PartialBlackBoxTestCase.java`, `UnsupportedBlackBoxTestCase.java`).
  - The ported positive tests keep ZXing's thresholds; QR blackbox 1 is 17/14/17/14 at 0/90/180/270°.
  - The repo has no `fuzz/` directory.
  - CI (`rust.yml`) runs only check, build, test, and test `--no-default-features`.
  - Sources: [rxing tests](https://github.com/rxing-core/rxing/tree/main/tests), [rust.yml](https://github.com/rxing-core/rxing/blob/main/.github/workflows/rust.yml)
- Open CI issues:
  - #82 "Automated Tests Do Not Properly Cover All Features";
  - #107: the "minimal" CI run "runs with default features due to workspace feature unification".
  - Sources: [#82](https://github.com/rxing-core/rxing/issues/82), [#107](https://github.com/rxing-core/rxing/issues/107)
- Robustness bugs reported by users (all fixed unless stated):
  - #69: "infinite 100% cpu loop when 'try harder' is enabled", fixed in 0.8.2 within a day;
  - #92: hang ("deadlock") found by zxing-cpp's author with random samples, fixed in 0.9.2;
  - #78/#83: subtract-with-overflow panics;
  - #86: `unwrap()` on a zero-size image;
  - #96: Telepen index underflow;
  - #114: MaxiCode never returns on images narrower than 13 px;
  - #106, **open**: Data Matrix writer panics past 144x144;
  - #109, **open**: `no_character_set_support` panics on byte-mode QR;
  - #116, **open**: Aztec misses symbols zxing-cpp reads;
  - #74: small noisy QR unreadable, partly fixed in 0.9.1 with "filter input".
  - Sources: [rxing issues](https://github.com/rxing-core/rxing/issues?q=is%3Aissue), [#69](https://github.com/rxing-core/rxing/issues/69), [#92](https://github.com/rxing-core/rxing/issues/92), [#74](https://github.com/rxing-core/rxing/issues/74)
- Maintenance: releases are by hschimke. **Correction** to the audit's "hschimke alone": 2026 brought outside contributors with merged PRs (JasperDeSutter, porxyz/Samy, 514sw, forzafedor, jordiboehme and others). Most merged within 1–10 days, a few took weeks or months (#95 a month, #89 four months), and #108 was closed unmerged — [rxing releases](https://github.com/rxing-core/rxing/releases), [commits](https://github.com/rxing-core/rxing/commits/main)
- `unsafe`: none in `src/` of 0.9.4 (grep) — downloaded crate, [crates.io download](https://crates.io/api/v1/crates/rxing/0.9.4/download)

**zxing-cpp (C++ library and Rust wrapper):**
- Rust crate:
  - `zxing-cpp` 0.5.3, 2026-07-29, Apache-2.0; 38,535 downloads (22,915 recent); 2 reverse dependencies.
  - Bundles zxing-cpp v3.1.1.
  - `bundled` feature: `build.rs` runs CMake on `core` (static `ZXing`, C API, `ZXING_USE_BUNDLED_ZINT=ON`) and links `stdc++`/`c++`. Without it, `ZXING_CPP_LIB_DIR` must point at a prebuilt library.
  - `image = ">=0.24"` optional; 48 `unsafe` in `src/lib.rs`.
  - Sources: [crates.io zxing-cpp](https://crates.io/crates/zxing-cpp), crate source `build.rs` and `README.md` ([wrappers/rust](https://github.com/zxing-cpp/zxing-cpp/tree/master/wrappers/rust))
- The bundled zint backend (writers) is BSD-3-Clause, copyright Robin Stuart — crate source `zint/backend/*.h` ([zint](https://github.com/zint/zint))
- C++ releases:
  - v3.0.0 (Feb): "C++20 is now a build requirement for the library" and a new writer API on bundled libzint 2.16;
  - v3.1.0 (Jul): Telepen and MicroPDF417, 10–40% faster in places, and "Fix (at least substantially reduce) false positive DataBarStacked detections";
  - v3.1.1 (29 Jul) is the latest; the repo was pushed 2026-11-07.
  - Source: [zxing-cpp releases](https://github.com/zxing-cpp/zxing-cpp/releases)
- zxing-cpp has local libFuzzer targets (`test/fuzz`, clang `-fsanitize=address,undefined,fuzzer`); OSS-Fuzz has a `zxing` (Java) project but no `zxing-cpp` one — [zxing-cpp test/fuzz](https://github.com/zxing-cpp/zxing-cpp/tree/master/test/fuzz), [OSS-Fuzz zxing project.yaml](https://github.com/google/oss-fuzz/blob/master/projects/zxing/project.yaml)

**Published comparisons:**
- zxing-bench README by zxing-cpp's author, on an Apple M4 Pro; he calls it "my personal (and biased) conclusion"; date and versions not stated. With `--common` (the format set every library supports):
  - zxing-cpp's `falsepositives-2` set (25 images without codes): rxing 10 reads, zbar-rust, zedbar, bardecoder, rqrr and zxing-cpp 0;
  - `qrcode-2` (56 images): rxing 48, zxing-cpp 49, zbar-rust 38, zedbar 36, rqrr 19, bardecoder 18;
  - BoofCV's hard QR set (542 images): zxing-cpp 1,124 in 5.10 s, rxing 992 in 33.96 s, zedbar 791 (820 with retries, 192 s), zbar-rust 446, while bardecoder and rqrr crashed (index out of bounds, assertion);
  - one sample image with all formats: rxing 15 codes in 410 ms vs zxing-cpp 16 in 13 ms.
  - Sources: [axxel/zxing-bench](https://github.com/axxel/zxing-bench), dataset [BoofCV QR performance](https://boofcv.org/index.php?title=Performance:QrCode), [zxing-cpp falsepositives-2](https://github.com/zxing-cpp/zxing-cpp/tree/master/test/samples/falsepositives-2)
- The in-house bench (one thread, EPYC 7302P, 170 synthetic Letter pages at 300 dpi):
  - read rate: rxing with separator symbologies and TryHarder 149/170, zxing-cpp 159/170, rxing without TryHarder 73;
  - pages without codes: 44 ms (rxing) vs 30 ms (zxing-cpp) at 150 dpi, and 70 vs 47 ms at 200 dpi 1-bit;
  - false reads on 150 real scans: 0 with formats limited, about 22 with whole-page multi-code and every format.
  - Source: local `scratch/reference/sheaf/m05-scan/research/reading-writing-codes.md`
- Dynamsoft (a competitor) benchmarked zxing-cpp 3.1.x against its own reader on 7,894 BarBeR images. It reports zxing-cpp's precision as 92.35% vs DBR's 91.49%, with DBR ahead on recall — [Dynamsoft benchmark](https://www.dynamsoft.com/codepool/benchmark-barcode-reading-python-zxing-dynamsoft-barcode-reader.html)

**Other Rust readers:**

| Crate | Latest | License | Notes |
|---|---|---|---|
| `rqrr` | 0.11.0, 2026-09-02 | (MIT OR Apache-2.0) AND ISC | QR only; MSRV 1.85.1; 5.52M downloads, 84 reverse deps; 14 open issues. [crates.io rqrr](https://crates.io/crates/rqrr) |
| `quircs` | 0.10.3, 2025-10-20 | MIT | Port of quirc; QR only; MSRV 1.68; 335k downloads. [crates.io quircs](https://crates.io/crates/quircs) |
| `bardecoder` | 0.5.0, 2023-07-29 | MIT | QR only; stale; crashed in zxing-bench. [crates.io bardecoder](https://crates.io/crates/bardecoder) |
| `zedbar` | 0.7.0, 2026-10-02 | LGPL-3.0-or-later | Pure-Rust port of ZBar; no `unsafe` (grep); decoders for Codabar, Code 39/93/128, DataBar, EAN, I25, QR and SQ code; **no Data Matrix, no PDF417**. Copyleft: study only. [crates.io zedbar](https://crates.io/crates/zedbar), [GitHub](https://github.com/eventualbuddha/zedbar) |
| `zbar-rust` | 0.1.1, 2026-09-13 | LGPL-2.1 | Binding to C zbar; MSRV 1.89. Copyleft and C: study only. Same for `zbar-sys`/`zbar-src` 0.1.5 (LGPL-2.1-or-later) and `masuri` 0.1.2, "Rust port of zbar" (LGPL-2.1-or-later, Code 128/EAN/UPC). [zbar-rust](https://crates.io/crates/zbar-rust), [masuri](https://crates.io/crates/masuri) |
| **New:** `anyd` | 0.2.0, 2026-09-18 | MIT | First release 2026-07-11, 6 versions, 3,791 downloads, 2 stars, 1 reverse dependency; Karpelès Lab; MSRV 1.89. See below. [crates.io anyd](https://crates.io/crates/anyd), [GitHub KarpelesLab/anydcode](https://github.com/KarpelesLab/anydcode) |
| **New:** `tapirscan` | 1.5.0, 2026-10-24 | MIT OR Apache-2.0 | First release 2026-09-17, 115 downloads, 13 stars; MSRV 1.91. See below. [crates.io tapirscan](https://crates.io/crates/tapirscan), [GitHub](https://github.com/kleinicke/tapirscan) |
| **New:** `wxing` | 0.1.0, 2026-08-30 | Apache-2.0 | "Rust port of the ZXing fork used by WeChat"; QR only; one release. [crates.io wxing](https://crates.io/crates/wxing) |
| `code128` | 0.2.1, 2025-01-25 | Apache-2.0 OR MIT | Encodes and decodes Code 128 from module widths, not images. [crates.io code128](https://crates.io/crates/code128) |

- More on `anyd`:
  - "From-scratch encoding and decoding of 1D and 2D barcodes", `#![no_std]`, no image dependency (takes a luma buffer);
  - `#![cfg_attr(not(feature = "wasm"), forbid(unsafe_code))]`;
  - image detection for QR, Data Matrix, Aztec, Micro QR, rMQR, PDF417, MicroPDF417 and the common 1D codes;
  - 1D readings are "accepted by consensus … at least two scan lines must agree, the span must show real quiet zones";
  - robustness tests: `linear_fuzz.rs`, `qr_family_fuzz.rs`, `sampler_fuzz.rs`, `stacked_no_panic.rs`; about 55k lines;
  - aimed at live camera video.
  - Source: downloaded crate README, `src/lib.rs`, `tests/`
- More on `tapirscan`:
  - ships 8.5 MB of generated Rust (527 files) built from a monorepo with C/C++/Java/Python bindings; generated modules `#![forbid(unsafe_code)]`;
  - defaults to retail formats (EAN/UPC), with QR among the options;
  - says "All decoding and localization algorithms in this crate are project-owned" and lists the standard tables taken from Nayuki (MIT) and pdf417-py (MIT).
  - Source: downloaded crate `README.md`, `THIRD_PARTY_NOTICES.md`, `provenance.json`
- `datamatrix` 0.3.3 "decoding" is `DataMatrix::decode(pixels: &[bool], width)` over an already-sampled module grid. It has no image locator — crate source `src/lib.rs` ([crates.io datamatrix](https://crates.io/crates/datamatrix))

### Inferences
- **Use rxing and contribute upstream.** Contributions in priority order, matching sheaf's own risks:
  1. port ZXing's negative black-box tests (the Java files already sit in `tests/negative`);
  2. add `cargo-fuzz` targets for the readers sheaf turns on;
  3. a CI feature matrix (#82/#107);
  4. help v0.10 drop the exact `image` pin.

  Sheaf's own guards stay regardless (formats limited, one decode per symbology, `multi_barcode_readers` off). They are what made rxing's false reads 0 of 150 in the bench, against zxing-bench's 10 of 25 with every common format on.
- **zxing-cpp stays optional at most:** a C++20 toolchain, CMake, `stdc++` linking, 48 `unsafe` blocks, and no wasm32 target in the wrapper (inferred from the CMake/static C++ build; not tested). Its value is as a reference oracle in tests, not a production engine.
- **`anyd` is worth an hour in the existing bench:** pure Rust, `forbid(unsafe)`, permissive, consensus-gated 1D. It has no track record (2 stars, 4 months, one org), and its camera-oriented design is unproven on 1-bit 150–200 dpi pages. `tapirscan` is too new and its generated source is hard to review; watch only.
- rqrr, quircs and wxing are QR-only, which rules them out as the primary reader: separators need Code 39, Code 128, Data Matrix and PDF417.

### Gaps
- I had no time on today's rxing 0.9.4 in the bench; the bench numbers are for 0.9.3. The 0.9.4 Code 128 fixes may change the figures slightly.
- zxing-bench gives no date or crate versions, so whether rxing's 10 false positives predate 0.9.x is unknown.
- I did not check rqrr's or zxing-cpp's issue trackers in detail: the GitHub API rate limit was exhausted.
- No independent benchmark of decode rates on *document scans* (as opposed to photos) at 200–300 dpi exists apart from sheaf's own bench.

## Q4. Barcode writing for separator sheets

### Takeaway
Writing is well served by permissive pure-Rust crates. rxing's encoders (QR, Data Matrix, Code 128, Code 39, PDF417, Aztec) are already a dependency and fit the plan. `qrcode` (MIT/Apache, 25M downloads) and `datamatrix` (vector path output) are mature fallbacks. Every Rust Code 39 encoder checked (rxing, barcoders, anyd) draws a 2:1 wide:narrow ratio, so a printed Code 39 should be drawn from the pattern with the ratio sheaf chooses rather than taken as a bitmap.

### Cited Findings
- rxing 0.9.4 encodes Aztec, Data Matrix, PDF417, QR, Codabar, Code 39/93/128, ITF, EAN-8/13, UPC-A/E and Telepen — [rxing README](https://github.com/rxing-core/rxing/blob/main/README.md)
- Open rxing writer issue #106: "DataMatrix writer panics instead of returning an error when data exceeds 144x144 capacity". Its fix, PR #108, was **closed without merging** on 2026-10-12 with no comment, so the panic is presumably still in 0.9.4. Callers should cap the input length before encoding — [#106](https://github.com/rxing-core/rxing/issues/106), [PR #108](https://github.com/rxing-core/rxing/pull/108)
- rxing's `Code39Writer::toIntArray` sets each element to `1` or `2` (2:1). barcoders' Code 39 table is 12-module patterns with wide = 2 modules. anyd's Code 39 module tables use "`wide = 2` narrow modules", though its decoder accepts 2:1 to 3:1. Sources: [rxing code_39_writer.rs](https://github.com/rxing-core/rxing/blob/main/src/oned/code_39_writer.rs), [barcoders code39.rs](https://github.com/buntine/barcoders/blob/master/src/sym/code39.rs), anyd crate `src/codes/code39/mod.rs`
- Other writers:

  | Crate | Latest | License | Notes |
  |---|---|---|---|
  | `qrcode` | 0.14.1, 2024-07-05 | MIT OR Apache-2.0 | MSRV 1.67.1; 25.4M downloads; 618 reverse deps; repo pushed 2025-08-25, 33 open issues. [crates.io qrcode](https://crates.io/crates/qrcode) |
  | `fast_qr` | 0.14.0, 2026-09-01 | MIT | [crates.io fast_qr](https://crates.io/crates/fast_qr) |
  | `qrcodegen` | 1.8.0, 2022-04-17 | MIT | Nayuki; 11.4M downloads. [crates.io qrcodegen](https://crates.io/crates/qrcodegen) |
  | `datamatrix` | 0.3.3, 2026-05-31 | Apache-2.0 OR MIT | No `unsafe`; `Bitmap::path()` gives a vector outline "decomposed into Eulerian circuits"; 7 reverse deps. [crates.io datamatrix](https://crates.io/crates/datamatrix) |
  | `barcoders` | 2.0.0, 2024-04-03 | MIT OR Apache-2.0 | 1D only; 674k downloads. [crates.io barcoders](https://crates.io/crates/barcoders) |
  | `pdf417` | 0.2.1, 2023-10-16 | crates.io says "non-standard" (unchecked) | no-std encoder; not needed while rxing encodes PDF417. [crates.io pdf417](https://crates.io/crates/pdf417) |
  | `anyd` | 0.2.0 | MIT | Heap-free `encode_into` for 1D, QR, Micro QR and Data Matrix. [crates.io anyd](https://crates.io/crates/anyd) |
  | `bwipp-rs` | 0.1.1, 2026-05-30 | MIT OR Apache-2.0 | "Pure-Rust port of BWIPP"; depends on datamatrix and qrcode. [crates.io bwipp-rs](https://crates.io/crates/bwipp-rs) |
  | `krilla-rxing` | 0.1.1 | EUPL-1.2 | Copyleft: study only. [crates.io krilla-rxing](https://crates.io/crates/krilla-rxing) |

- zxing-cpp's v3 writer API wraps bundled libzint 2.16 (C, BSD-3) — [zxing-cpp releases](https://github.com/zxing-cpp/zxing-cpp/releases)
- No writer of any kind (Rust or other open source) draws Kodak patch codes. Patch writers exist only commercially: LEADTOOLS `PatchCodeBarcodeWriteOptions`, Accusoft (`BarcodeValue` 1–6), Aspose — [LEADTOOLS write options](https://leadtools.com/help/sdk/v22/dh/ba/patchcodebarcodewriteoptions.html), [Accusoft](https://help.accusoft.com/BarcodeXpress/latest/BxJava/patch-code.html), [Aspose](https://docs.aspose.com/barcode/net/patch-code-barcodes/)

### Inferences
- **Confirmed:** the audit's "adopt rxing's encoder; qrcode the fallback" holds. Add `datamatrix` as the choice if a vector Data Matrix outline is wanted for PDF output.
- Drawing patch codes is trivial geometry (four rectangles from A-61599's zone tables), so `patch-codes` should draw them itself. No crate is warranted.

### Gaps
- I did not measure rxing's or qrcode's output against a print and scan loop. The audit's "1D codes need 20 mil bars at 200 dpi in 1-bit" is from sheaf's bench, not re-verified here.

## Q5. Separator-sheet practice in capture software

### Takeaway
Separation by patch code or barcode is standard in production capture software:

- Kodak's own convention is Patch 2 kept, Patch 3 and T dropped.
- Enterprise suites make deletion an option, and Tungsten TotalAgility keeps the patch sheet by default.
- PaperStream Capture has a Delete Separator check box.
- The open-source tools (Paperless-ngx, NAPS2) don't read patch bars at all. They read the Code 39 `PATCHT` printed on the sheet, and both drop it by default.
- Kodak's own software reader searches only the top 4 inches after rotation, with an optional 180° retry. sheaf's all-four-edges reader is broader than Kodak's.

### Cited Findings
**Kodak Capture Pro (A-61993, March 2022):**
- Patch table:

  | Patch | Function | Kept? |
  |---|---|---|
  | 2 | separates pages into documents | yes |
  | 3 | separates documents into batches | no |
  | T (programmable) | documents, batches or attachments | no |

- Options are "Batch separation", "Document separation", "Create attachment", "Delete page" and "Rotate document based on patch".
- "The software always searches for patch codes in the first 4 inches (10 cm) of the top of the image after rotation, deskew, and cropping … If a patch is vertical, the image should be rotated with the Software rotation option."
- "Alternatively search on 180 rotated images", and "Alternatively search on back".
- Source: [Capture Pro Admin Guide A-61993](https://www.kodakalaris.com/sites/default/files/2024-11/a61957_capture_pro_admin_guide_en.pdf), [Capture Pro Help](https://resources.kodakalaris.com/support/Capture%20Pro%20Help/Content/Separating_batches_or_documents_with_patch_codes.htm)

**Tungsten (Kofax):**
- Tungsten TotalAgility 8.0 separation profile: methods "Bar code and patch code", "Fixed sheet", "Manual (default)". "Delete patch code separator sheet (default: Clear: This means the system retains the separator sheet)", and a separate "Delete bar code separator sheet" — [TotalAgility 8.0 docs](https://docshield.tungstenautomation.com/KTA/en_US/8.0.0-ivy45l9g96/help/Designer/All_Shared/Capture/t_profilesepforadocument.html)
- One search summary paraphrased the default as "deletes"; the 8.0 page text says retained. I trust the page — [TotalAgility deletion rules](https://docshield.tungstenautomation.com/KTA/en_US/8.0.0-ivy45l9g96/help/Designer/All_Shared/Capture/c_activitybarpatchcodeusage.html)
- Tungsten (Kofax) Capture 11.x:
  - one patch type is "reserved for batch separator sheets" and "printed on all batch separator sheets made with the Separator Sheet Utility";
  - "The bar code on the separator sheet sets the new batch's class name";
  - "A patch code reserved for batch separation cannot be used for document separation."
  - Source: [Kofax Capture: Batch Separation Patch Code window](https://docshield.tungstenautomation.com/kc/en_us/11.1.0-40hy9nfk91/help/main/KC_administration/c_batchseppatchcode.html)
- Kofax Express (via MetaTool help): "Delete separator", with front, back or both sides — [MetaTool separation](https://www.capturebites.com/metatool/metatool-help/separation)

**Other suites:**
- PaperStream Capture: patch code or barcode as "Detect Doc Separator", with "[Delete Separator]" optional. Under ISIS only Patch 1, 2, 3 and T — [PaperStream Capture 2.10 guide](https://origin.pfultd.com/downloads/IMAGE/manual/psc/P2WW-4050-06ENZ0.pdf)
- ABBYY:
  - FineReader Server 14 barcode separation has a setting for deleting barcode pages, and ships a sample separator sheet — [ABBYY FineReader Server 14](https://help.abbyy.com/en-us/finereaderserver/14/help/howto_configuredocumentseparation)
  - For FlexiCapture 12, a community answer has the patch page deleted through a script stage, which suggests no built-in toggle there — [ABBYY community](https://support.abbyy.com/hc/ja/community/posts/28657605084435-How-to-delete-a-page-in-document-which-contains-a-particular-type-of-barcode)
- Epson Document Capture Pro has a Patch Code screen under Job Separation settings — [Epson DCP help](https://download4.epson.biz/sec_pubs/dcp/webhelp/v3/en/2370.htm)
- ScanSnap Home: I found no barcode or patch separation. Its FAQ splits a scan into files by profile ("Split file save") — [ScanSnap FAQ](https://scansnap-faq.pfu.ricoh.com/hc/en-us/articles/21000580939801-How-can-I-scan-multiple-documents-and-save-them-as-separated-PDF-files-in-a-batch)

**Open-source tools:**
- Paperless-ngx, current 3.4.2 (changelog):
  - `PAPERLESS_CONSUMER_BARCODE_STRING` defaults to "PATCHT";
  - `…_RETAIN_SPLIT_PAGES` defaults false: "pages containing the separator barcode are discarded", and each split starts with the *next* page;
  - ASN barcodes and tag barcodes (with `TAG_BARCODE_SPLIT`) split *and keep* their page;
  - `…_BARCODE_DPI` defaults to 300 and `…_BARCODE_UPSCALE` to 0.0;
  - TIFF support is off by default;
  - the docs recommend Alliance Group's patch sheets.
  - Sources: [Paperless-ngx configuration.md](https://github.com/paperless-ngx/paperless-ngx/blob/main/docs/configuration.md#barcodes), [advanced_usage.md](https://github.com/paperless-ngx/paperless-ngx/blob/main/docs/advanced_usage.md#barcodes)
- Paperless reads with `zxingcpp.read_barcodes(image)` (all formats, defaults), pinned `zxing-cpp~=3.1.0`. The configuration docs no longer have a zbar/scanner setting — [barcodes.py](https://github.com/paperless-ngx/paperless-ngx/blob/main/src/documents/barcodes.py), [pyproject.toml](https://github.com/paperless-ngx/paperless-ngx/blob/main/pyproject.toml)
- NAPS2:
  - "Separate files by Patch-T"; "NAPS2 supports patch codes on all scanners with both WIA and TWAIN";
  - "a resolution of at least 300 DPI" if recognition fails;
  - printable sheets from Alliance Group.
  - The code reads Code 39 via ZXing.Net (GPL; study only).
  - Sources: [NAPS2 batch scan docs](https://www.naps2.com/doc/batch-scan), [BarcodeDetector.cs](https://github.com/cyanfish/naps2/blob/master/NAPS2.Sdk/Scan/BarcodeDetector.cs)
- Alliance Group offers Patch-T for A4 and US Letter and full sets for both, states no license or terms, and describes "wide/narrow 1D barcodes" — [Alliance Group patch codes](https://alliancegroup.co.uk/patch-codes.htm)
- OCRmyPDF has no barcode-splitting plugin that I found. Neighbours:
  - OCRthyPDF-Essentials: a QR separator, discarded in separator mode and kept in sticker mode — [OCRthyPDF README](https://github.com/digidigital/OCRthyPDF-Essentials/blob/main/README.md)
  - scanprep: separator text `SCANPREP_SEP` via zbar — [scanprep](https://pypi.org/project/scanprep)
  - docbarcodes 1.2.0 (2026-11-02): extracts barcodes from scanned PDFs via ZXing Java (`jpype1`, `zxingmod`) — [PyPI docbarcodes](https://pypi.org/project/docbarcodes/)

### Inferences
- **Confirmed** from Kodak's own manual: proposal p14's defaults (3 drops and starts a batch; T drops and starts a document; 2 keeps and starts a document; 1, 4 and 6 do nothing) match Capture Pro's conventions.
- **New:** enterprise suites default the other way on deletion (TotalAgility keeps), so "drop" must stay a per-rule option, as p14 already has it.
- **New:** Kodak's own software reader looks only at the top 4 in after its own rotation, with a 180° retry. Reading all four edges and reporting the edge, as `patch-codes` does, is a superset that also tells 3 from T and 1 from 6 without Kodak's rotate-then-search step.
- Reading the Code 39 `PATCHT` as patch T, as p14 proposes, is what makes sheets made for NAPS2 and Paperless work. Those two tools are the only open-source precedent, and neither reads bars.

### Gaps
- Not verified this pass: ABBYY FlexiCapture's own patch code options, and NAPS2's handling of the separator page (dropped or kept). The audit says dropped; I didn't find the line in source.
- The audit's claims about CCH, SurePrep, Stirling-PDF, ecoDMS and DocuWare were not re-checked here.

## Q6. Test material (patch sheets, barcode scans)

### Takeaway
No public dataset of scanned patch code pages turned up. Kodak's camera-ready pages (A-61599 packet), Kodak's 2012 "Patches for Printing" PDF, Alliance Group's PDFs and PaperStream IP's driver-supplied sheets are printable sources for synthetic and print-and-scan fixtures. For barcodes:

- zxing-cpp's `test/samples` (including `falsepositives-1/2`) and ZXing's blackbox sets are the reference corpora; rxing ports the positive ones;
- BoofCV's QR set is the hard QR benchmark;
- BarBeR (8,748 images, 9,818 annotated codes) covers localization, mostly photos.

### Cited Findings
- Kodak's "Patches for Printing" (2012):
  - a 6-page Letter PDF created 2012-01-17 ("Eastman Kodak 2012");
  - "These sheets were designed to help provide accurate patch codes for either software or hardware patch code reading systems";
  - print "in Black and White at 100% scaling or 'unscaled'"; "measurement bars have been included … to verify the correct scaling";
  - A4 and US Letter versions exist.
  - The copy hosted by Ascend Software has Letter pages only.
  - Source: [Patches for Printing (Ascend copy)](https://help.ascendsoftware.com/hc/article_attachments/360100776993)
- A-61599 includes "2.5-inch (62.5 mm) long patch codes" and "Full-page width and full-page length patch code documents … fed into the transport without regard to orientation" — [A-61599](https://archive.org/download/manualsonline-id-e24f73ef-f1cf-4f31-bc70-8aae8572f136/e24f73ef-f1cf-4f31-bc70-8aae8572f136.pdf)
- PaperStream IP supplies printable patch code sheets ("Print the required number of patch code sheets provided by the PaperStream IP driver") — [PaperStream Capture 2.10 guide](https://origin.pfultd.com/downloads/IMAGE/manual/psc/P2WW-4050-06ENZ0.pdf)
- Alliance Group: Patch-T and full sets, A4 and Letter — [Alliance Group](https://alliancegroup.co.uk/patch-codes.htm)
- zxing-cpp's false-positive sets (`test/samples/falsepositives-2`, 25 images) and QR sets are used by zxing-bench — [zxing-cpp samples](https://github.com/zxing-cpp/zxing-cpp/tree/master/test/samples/falsepositives-2), [zxing-bench](https://github.com/axxel/zxing-bench)
- rxing's repository holds `test_resources/` (not in the published crate). Its blackbox tests reproduce ZXing's thresholds — [rxing repo tree](https://github.com/rxing-core/rxing/tree/main/test_resources), [qr_code_blackbox_tests.rs](https://github.com/rxing-core/rxing/blob/main/tests/qr_code_blackbox_tests.rs)
- BarBeR (RRPR 2024; University of Modena and Reggio Emilia): 8,748 images with 9,818 annotated barcodes, VGG annotation format — [BarBeR paper](https://rrpr2024.sciencesconf.org/data/RRPR_2024_paper_10.pdf), [GitHub Henvezz95/BarBeR](https://github.com/Henvezz95/BarBeR)
- BoofCV's QR performance set (542 images in zxing-bench) — [BoofCV](https://boofcv.org/index.php?title=Performance:QrCode)

### Inferences
- Patch-code testing has to be synthetic plus print-and-scan, as `scan-fixtures` plans: Kodak's and Alliance's layouts drawn from A-61599's zone tables, and the corpus's real scans as negatives.
- A print-and-scan set of Kodak's and Alliance's sheets would fill the one hole: real 1-bit scans at 150–300 dpi, including photocopies (A-61599's known failure) and the perpendicular i280 layout. It would also be the first such public set.
- zxing-cpp's `falsepositives-1/2` are the cheapest regression gate for rxing's false-read rate in sheaf's trait tests.

### Gaps
- I did not check the licenses of the image files in zxing-cpp's and ZXing's sample sets (the repositories are Apache-2.0; the images' provenance varies) or BarBeR's data license. Check them before vendoring any into `scan-fixtures`.
- No public scanned-document barcode dataset at 150–300 dpi (as opposed to camera photos) turned up.

## Verdicts by need

### Takeaway

| Need | Verdict | Reason |
|---|---|---|
| Patch code reading (1, 2, 3, 4, 6, T on any edge, with the edge) | **Build our own** (keep the in-house reader) | Nothing open source exists under any license; only closed commercial SDKs. A-61599 is public and the reader already meets the bench. |
| Patch code drawing (A-61599 geometry) | **Build our own** | Four rectangles from the zone tables; no permissive writer exists. |
| Driver-reported patch codes (TWAIN `TWEI_PATCHCODE`/EOJ, WIA Patch Code Reader) | **Build our own**, optional, in a device backend | Standards are documented; no Rust crate wraps them; SANE has nothing. |
| Barcode reading | **Use rxing and contribute upstream** | Only pure-Rust permissive multi-symbology reader; real gaps (negative tests, fuzzing, image pin) are fixable upstream. |
| Second engine | zxing-cpp **study only / test oracle** (optional engine at most); `anyd` **evaluate in the bench**; zedbar, zbar-rust, masuri **study only** (LGPL) | zxing-cpp is C++20/CMake/FFI; anyd is promising but four months old. |
| Barcode writing for sheaf's sheet | **Use directly**: rxing encoders; `qrcode`/`datamatrix` as fallbacks | Mature, permissive, pure Rust; draw Code 39 from the pattern to pick the ratio. |

### Cited Findings
- Evidence for each row is under Q1–Q5 above, with sources there: A-61599; the npm, NuGet and crates.io searches; rxing README, issues and CI; zxing-cpp's build.rs and releases; anyd's crate source; twain.h; Microsoft WIA docs; SANE sources.

### Inferences
- `patch-codes` should add three things to its tests and docs:
  - the perpendicular-bar layout (A-61599's i280/3590C/i600, and LEADTOOLS' "vertical" search), as either a supported case or a documented non-goal;
  - A-61599's two sets of length and placement rules, kept apart;
  - Kodak's own reader behaviour (top 4 in after rotation, 180° retry) as the compatibility floor.
- Corrections to the 2026-10-23 audit:
  - rxing 0.9.4 (2026-11-08) is the latest, still pinning image =0.25.8, with v0.10 planned for December 2026;
  - rxing has outside contributors, not hschimke alone;
  - TWAIN's patch capability is `ICAP_PATCHCODEDETECTIONENABLED`;
  - WIA 2.0 has patch code metadata;
  - the two image versions in the audit are image 0.25.10 and imageproc 0.27, not a conflict;
  - A-61599's 0.75 in minimum belongs to the perpendicular layout;
  - new pure-Rust candidates: anyd, tapirscan, wxing.

### Gaps
- The `anyd` evaluation (and a rerun of rxing 0.9.4) in sheaf's bench is the one open measurement before the second-engine verdict can firm up.

## Holes in the ecosystem

### Takeaway
Seven needs have no adequate permissive or pure-Rust implementation:

1. a Kodak patch code reader or writer, in any language, under any open license;
2. a public patch code scan dataset;
3. a pure-Rust barcode reader with tested false-positive behaviour and fuzzing;
4. a pure-Rust reader at zxing-cpp's decode rate and speed;
5. any SANE channel for scanner-side patch or barcode results;
6. a maintained Rust wrapper of TWAIN or WIA patch code metadata;
7. a Rust Code 39 encoder with a settable wide:narrow ratio.

### Cited Findings
1. No open patch code reader or writer: crates.io, npm and NuGet searches, the GitHub/web search, and greps of zxing-cpp, rxing, zedbar, bardecoder and barcoders find none. Only commercial SDKs (Accusoft, Dynamsoft, LEADTOOLS, Aspose, VintaSoft, RasterEdge, OnBarcode, ByteScout, GdPicture) — see Q1 ([NuGet search](https://azuresearch-usnc.nuget.org/query?q=patchcode), [Accusoft](https://help.accusoft.com/BarcodeXpress/latest/BxJava/patch-code.html))
2. No public patch code scan dataset; only printable sheets — see Q6 ([Patches for Printing](https://help.ascendsoftware.com/hc/article_attachments/360100776993))
3. rxing: "Negative verification tests are not implemented"; no fuzz targets; 10 false reads on 25 no-code images in zxing-bench — [rxing README](https://github.com/rxing-core/rxing/blob/main/README.md), [zxing-bench](https://github.com/axxel/zxing-bench)
4. rxing found 992 vs zxing-cpp's 1,124 codes on BoofCV's QR set, in 33.96 s vs 5.10 s — [zxing-bench](https://github.com/axxel/zxing-bench)
5. SANE backends define patch fields but expose no option or metadata — [fujitsu-scsi.h](https://gitlab.com/sane-project/backends/-/blob/master/backend/fujitsu-scsi.h), [kodak-cmd.h](https://gitlab.com/sane-project/backends/-/blob/master/backend/kodak-cmd.h)
6. The only `twain` crate is 0.1.0 from 2021 (not inspected) — [crates.io twain](https://crates.io/crates/twain)
7. rxing, barcoders and anyd all write Code 39 at 2:1 — [rxing code_39_writer.rs](https://github.com/rxing-core/rxing/blob/main/src/oned/code_39_writer.rs), [barcoders code39.rs](https://github.com/buntine/barcoders/blob/master/src/sym/code39.rs)

### Inferences
- Holes 1 and 2 are why `patch-codes` is "the crate nobody else has". Publishing it permissively, with a small print-and-scan fixture set, would be the first open implementation anywhere (from what I found).
- Holes 3 and 4 are best closed upstream in rxing, starting with negative tests and fuzz targets, rather than by a new reader. Sheaf's format-limiting already contains the false-positive risk in practice (0 of 150 real scans).
- Hole 5 means pixel reading is mandatory on Linux. Holes 6 and 7 are small and in-scope for sheaf's own code: device metadata in `scan-device`, Code 39 drawn from the pattern.

### Gaps
- I did not verify whether the 2021 `twain` crate exposes extended image info.
- Not ruled out: a non-English or self-hosted (non-GitHub) open-source patch reader that my searches missed.
