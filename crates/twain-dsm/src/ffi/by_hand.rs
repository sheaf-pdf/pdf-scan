// The numeric constants of `twain.h` 2.5 (twain-dsm commit a50f7326a191) that
// `TWAINH.cs` doesn't define, written here by hand: `scripts/twain-bindings.py`
// checks each value against `twain.h`, and that every numeric constant there
// is in this file or in `consts.rs`. A line here has the form the script reads:
// `pub const NAME: TYPE = VALUE;`, the value a literal, or for an alias of
// `twain.h`'s, the name it stands for.

use super::consts::{TWSS_A4, TWSS_ISOB3, TWSS_ISOB4, TWSS_ISOB6, TWSS_JISB5};

// The custom ranges' bases, beside `TWRC_CUSTOMBASE`, `TWCC_CUSTOMBASE` and
// `CAP_CUSTOMBASE`.
pub const DAT_CUSTOMBASE: u16 = 0x8000;
pub const MSG_CUSTOMBASE: u16 = 0x8000;

// `MSG_QUERYSUPPORT`'s bits for the help and label messages.
pub const TWQC_GETHELP: u16 = 0x0100;
pub const TWQC_GETLABEL: u16 = 0x0200;
pub const TWQC_GETLABELENUM: u16 = 0x0400;

// From the section `twain.h` heads "Depreciated Items": item types, triplets,
// capabilities and values of TWAIN 1.x, kept so that a value a driver sends has
// a name.
pub const TWTY_STR1024: u16 = 0x000d;
pub const TWTY_UNI512: u16 = 0x000e;
pub const TWFF_JPN: u16 = 12;
pub const DAT_TWUNKIDENTITY: u16 = 0x000b;
pub const DAT_SETUPFILEXFER2: u16 = 0x0301;
pub const CAP_SUPPORTEDCAPSEXT: u16 = 0x100c;
pub const CAP_PAGEMULTIPLEACQUIRE: u16 = 0x1023;
pub const CAP_PAPERBINDING: u16 = 0x102f;
pub const CAP_PASSTHRU: u16 = 0x1031;
pub const CAP_POWERDOWNTIME: u16 = 0x1034;
pub const ACAP_AUDIOFILEFORMAT: u16 = 0x1201;
pub const MSG_CHECKSTATUS: u16 = 0x0201;
pub const MSG_INVOKE_CALLBACK: u16 = 0x0903;
pub const TWSX_FILE2: u16 = 3;
pub const TWFS_FILESYSTEM: u16 = 0;
pub const TWFS_RECURSIVEDELETE: u16 = 1;
pub const TWPT_SRGB64: u16 = 11;
pub const TWPT_BGR: u16 = 12;
pub const TWPT_CIELAB: u16 = 13;
pub const TWPT_CIELUV: u16 = 14;
pub const TWPT_YCBCR: u16 = 15;
pub const TWSS_B: u16 = 8;
pub const TWSS_A4LETTER: u16 = TWSS_A4;
pub const TWSS_B3: u16 = TWSS_ISOB3;
pub const TWSS_B4: u16 = TWSS_ISOB4;
pub const TWSS_B6: u16 = TWSS_ISOB6;
pub const TWSS_B5LETTER: u16 = TWSS_JISB5;
pub const TWAF_WAV: u16 = 0;
pub const TWAF_AIFF: u16 = 1;
pub const TWAF_AU: u16 = 3;
pub const TWAF_SND: u16 = 4;

/// Every constant above, by name, its value widened to `i64`.
pub(crate) const BY_HAND: &[(&str, i64)] = &[
    ("DAT_CUSTOMBASE", DAT_CUSTOMBASE as i64),
    ("MSG_CUSTOMBASE", MSG_CUSTOMBASE as i64),
    ("TWQC_GETHELP", TWQC_GETHELP as i64),
    ("TWQC_GETLABEL", TWQC_GETLABEL as i64),
    ("TWQC_GETLABELENUM", TWQC_GETLABELENUM as i64),
    ("TWTY_STR1024", TWTY_STR1024 as i64),
    ("TWTY_UNI512", TWTY_UNI512 as i64),
    ("TWFF_JPN", TWFF_JPN as i64),
    ("DAT_TWUNKIDENTITY", DAT_TWUNKIDENTITY as i64),
    ("DAT_SETUPFILEXFER2", DAT_SETUPFILEXFER2 as i64),
    ("CAP_SUPPORTEDCAPSEXT", CAP_SUPPORTEDCAPSEXT as i64),
    ("CAP_PAGEMULTIPLEACQUIRE", CAP_PAGEMULTIPLEACQUIRE as i64),
    ("CAP_PAPERBINDING", CAP_PAPERBINDING as i64),
    ("CAP_PASSTHRU", CAP_PASSTHRU as i64),
    ("CAP_POWERDOWNTIME", CAP_POWERDOWNTIME as i64),
    ("ACAP_AUDIOFILEFORMAT", ACAP_AUDIOFILEFORMAT as i64),
    ("MSG_CHECKSTATUS", MSG_CHECKSTATUS as i64),
    ("MSG_INVOKE_CALLBACK", MSG_INVOKE_CALLBACK as i64),
    ("TWSX_FILE2", TWSX_FILE2 as i64),
    ("TWFS_FILESYSTEM", TWFS_FILESYSTEM as i64),
    ("TWFS_RECURSIVEDELETE", TWFS_RECURSIVEDELETE as i64),
    ("TWPT_SRGB64", TWPT_SRGB64 as i64),
    ("TWPT_BGR", TWPT_BGR as i64),
    ("TWPT_CIELAB", TWPT_CIELAB as i64),
    ("TWPT_CIELUV", TWPT_CIELUV as i64),
    ("TWPT_YCBCR", TWPT_YCBCR as i64),
    ("TWSS_B", TWSS_B as i64),
    ("TWSS_A4LETTER", TWSS_A4LETTER as i64),
    ("TWSS_B3", TWSS_B3 as i64),
    ("TWSS_B4", TWSS_B4 as i64),
    ("TWSS_B6", TWSS_B6 as i64),
    ("TWSS_B5LETTER", TWSS_B5LETTER as i64),
    ("TWAF_WAV", TWAF_WAV as i64),
    ("TWAF_AIFF", TWAF_AIFF as i64),
    ("TWAF_AU", TWAF_AU as i64),
    ("TWAF_SND", TWAF_SND as i64),
];
