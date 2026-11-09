//! TWAIN's definitions: the subset the spike calls, for Windows.
//!
//! Ported from `TWAINH.cs` in the TWAIN Working Group's twain-cs (commit
//! `74fe02ed78a6`, the C# translation of `twain.h`), each value checked
//! against `twain.h` 2.5 (twain-dsm commit `a50f7326a191`). Changed in the
//! port: written as Rust declarations; enums as integer constants with
//! `twain.h`'s names; each struct `#[repr(C, packed(2))]`, as `twain.h`'s
//! `#pragma pack (2)` has it on Windows; a union of same-typed members as one
//! field, named for its first. Where the two differ, `twain.h` wins:
//! `TWAINH.cs`'s Windows identity (`TW_IDENTITY_LEGACY`) ends in 8 bytes of
//! padding ("accounts for Id and SupportedGroups") that `twain.h` hasn't, so
//! [`TW_IDENTITY`] is 156 bytes; and `TWAINH.cs` types the entry points as
//! bare pointers, so their signatures here are `twain.h`'s, `TW_CALLINGSTYLE`
//! being `PASCAL`: `__stdcall` on x86, `extern "system"` in Rust.
//!
//! Fields of a packed struct are read by copy, never by reference.
//
// The notice TWAINH.cs carries:
//
//  Copyright (C) 2013-2021 Kodak Alaris Inc.
//
//  Permission is hereby granted, free of charge, to any person obtaining a
//  copy of this software and associated documentation files (the "Software"),
//  to deal in the Software without restriction, including without limitation
//  the rights to use, copy, modify, merge, publish, distribute, sublicense,
//  and/or sell copies of the Software, and to permit persons to whom the
//  Software is furnished to do so, subject to the following conditions:
//
//  The above copyright notice and this permission notice shall be included in
//  all copies or substantial portions of the Software.
//
//  THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
//  IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
//  FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL
//  THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
//  LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING
//  FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER
//  DEALINGS IN THE SOFTWARE.
//
// The TWAIN Toolkit's disclaimer, which the TWAIN License asks of software
// based on the TWAIN Toolkit:
//
//  The TWAIN Toolkit is distributed as is. The developer and distributors of
//  the TWAIN Toolkit expressly disclaim all implied, express or statutory
//  warranties including, without limitation, the implied warranties of
//  merchantability, noninfringement of third party rights and fitness for a
//  particular purpose. Neither the developers nor the distributors will be
//  liable for damages, whether direct, indirect, special, incidental, or
//  consequential, as a result of the reproduction, modification, distribution
//  or other use of the TWAIN Toolkit.

// A bindings module: names as twain.h has them, and more declared than any one
// scenario uses.
#![allow(
    dead_code,
    non_camel_case_types,
    non_snake_case,
    clippy::upper_case_acronyms
)]

use core::ffi::c_void;
use core::ptr::null_mut;

// Data groups and supported-group flags.
pub const DG_CONTROL: u32 = 1;
pub const DG_IMAGE: u32 = 2;
pub const DF_DSM2: u32 = 0x10000000;
pub const DF_APP2: u32 = 0x20000000;
pub const DF_DS2: u32 = 0x40000000;

// Data argument types.
pub const DAT_NULL: u16 = 0;
pub const DAT_CAPABILITY: u16 = 1;
pub const DAT_EVENT: u16 = 2;
pub const DAT_IDENTITY: u16 = 3;
pub const DAT_PARENT: u16 = 4;
pub const DAT_PENDINGXFERS: u16 = 5;
pub const DAT_SETUPMEMXFER: u16 = 6;
pub const DAT_STATUS: u16 = 8;
pub const DAT_USERINTERFACE: u16 = 9;
pub const DAT_CALLBACK: u16 = 0x10;
pub const DAT_CALLBACK2: u16 = 0x12;
pub const DAT_ENTRYPOINT: u16 = 0x403;
pub const DAT_IMAGEINFO: u16 = 0x101;
pub const DAT_IMAGEMEMXFER: u16 = 0x103;
pub const DAT_IMAGENATIVEXFER: u16 = 0x104;
pub const DAT_EXTIMAGEINFO: u16 = 0x10b;

// Messages.
pub const MSG_GET: u16 = 1;
pub const MSG_GETCURRENT: u16 = 2;
pub const MSG_GETDEFAULT: u16 = 3;
pub const MSG_GETFIRST: u16 = 4;
pub const MSG_GETNEXT: u16 = 5;
pub const MSG_SET: u16 = 6;
pub const MSG_RESET: u16 = 7;
pub const MSG_XFERREADY: u16 = 0x101;
pub const MSG_CLOSEDSREQ: u16 = 0x102;
pub const MSG_CLOSEDSOK: u16 = 0x103;
pub const MSG_DEVICEEVENT: u16 = 0x104;
pub const MSG_OPENDSM: u16 = 0x301;
pub const MSG_CLOSEDSM: u16 = 0x302;
pub const MSG_OPENDS: u16 = 0x401;
pub const MSG_CLOSEDS: u16 = 0x402;
pub const MSG_DISABLEDS: u16 = 0x501;
pub const MSG_ENABLEDS: u16 = 0x502;
pub const MSG_PROCESSEVENT: u16 = 0x601;
pub const MSG_ENDXFER: u16 = 0x701;
pub const MSG_STOPFEEDER: u16 = 0x702;
pub const MSG_REGISTER_CALLBACK: u16 = 0x902;

// Return codes.
pub const TWRC_SUCCESS: u16 = 0;
pub const TWRC_FAILURE: u16 = 1;
pub const TWRC_CHECKSTATUS: u16 = 2;
pub const TWRC_CANCEL: u16 = 3;
pub const TWRC_DSEVENT: u16 = 4;
pub const TWRC_NOTDSEVENT: u16 = 5;
pub const TWRC_XFERDONE: u16 = 6;
pub const TWRC_ENDOFLIST: u16 = 7;
pub const TWRC_INFONOTSUPPORTED: u16 = 8;
pub const TWRC_DATANOTAVAILABLE: u16 = 9;
pub const TWRC_BUSY: u16 = 0xa;
pub const TWRC_SCANNERLOCKED: u16 = 0xb;

// Condition codes.
pub const TWCC_SUCCESS: u16 = 0;
pub const TWCC_BUMMER: u16 = 1;
pub const TWCC_LOWMEMORY: u16 = 2;
pub const TWCC_NODS: u16 = 3;
pub const TWCC_MAXCONNECTIONS: u16 = 4;
pub const TWCC_OPERATIONERROR: u16 = 5;
pub const TWCC_BADCAP: u16 = 6;
pub const TWCC_BADPROTOCOL: u16 = 9;
pub const TWCC_BADVALUE: u16 = 0xa;
pub const TWCC_SEQERROR: u16 = 0xb;
pub const TWCC_BADDEST: u16 = 0xc;
pub const TWCC_CAPUNSUPPORTED: u16 = 0xd;
pub const TWCC_CAPBADOPERATION: u16 = 0xe;
pub const TWCC_CAPSEQERROR: u16 = 0xf;
pub const TWCC_DENIED: u16 = 0x10;
pub const TWCC_FILEEXISTS: u16 = 0x11;
pub const TWCC_FILENOTFOUND: u16 = 0x12;
pub const TWCC_NOTEMPTY: u16 = 0x13;
pub const TWCC_PAPERJAM: u16 = 0x14;
pub const TWCC_PAPERDOUBLEFEED: u16 = 0x15;
pub const TWCC_FILEWRITEERROR: u16 = 0x16;
pub const TWCC_CHECKDEVICEONLINE: u16 = 0x17;
pub const TWCC_INTERLOCK: u16 = 0x18;
pub const TWCC_DAMAGEDCORNER: u16 = 0x19;
pub const TWCC_FOCUSERROR: u16 = 0x1a;
pub const TWCC_DOCTOOLIGHT: u16 = 0x1b;
pub const TWCC_DOCTOODARK: u16 = 0x1c;
pub const TWCC_NOMEDIA: u16 = 0x1d;

// Container types.
pub const TWON_ARRAY: u16 = 3;
pub const TWON_ENUMERATION: u16 = 4;
pub const TWON_ONEVALUE: u16 = 5;
pub const TWON_RANGE: u16 = 6;

// Protocol version this application speaks (twain.h's TWON_PROTOCOLMAJOR and
// TWON_PROTOCOLMINOR).
pub const TWON_PROTOCOLMAJOR: u16 = 2;
pub const TWON_PROTOCOLMINOR: u16 = 5;

// Don't-care values.
pub const TWON_DONTCARE16: u16 = 0xffff;
pub const TWON_DONTCARE32: u32 = 0xffffffff;

// Item types.
pub const TWTY_INT8: u16 = 0;
pub const TWTY_INT16: u16 = 1;
pub const TWTY_INT32: u16 = 2;
pub const TWTY_UINT8: u16 = 3;
pub const TWTY_UINT16: u16 = 4;
pub const TWTY_UINT32: u16 = 5;
pub const TWTY_BOOL: u16 = 6;
pub const TWTY_FIX32: u16 = 7;
pub const TWTY_FRAME: u16 = 8;
pub const TWTY_STR32: u16 = 9;

// Transfer mechanisms.
pub const TWSX_NATIVE: u16 = 0;
pub const TWSX_FILE: u16 = 1;
pub const TWSX_MEMORY: u16 = 2;

// Pixel types.
pub const TWPT_BW: u16 = 0;
pub const TWPT_GRAY: u16 = 1;
pub const TWPT_RGB: u16 = 2;

// Pixel flavors.
pub const TWPF_CHOCOLATE: u16 = 0;
pub const TWPF_VANILLA: u16 = 1;

// Bit orders.
pub const TWBO_LSBFIRST: u16 = 0;
pub const TWBO_MSBFIRST: u16 = 1;

// Memory flags.
pub const TWMF_APPOWNS: u32 = 1;
pub const TWMF_DSMOWNS: u32 = 2;
pub const TWMF_DSOWNS: u32 = 4;
pub const TWMF_POINTER: u32 = 8;
pub const TWMF_HANDLE: u32 = 0x10;

// Extended image info ids.
pub const TWEI_DOCUMENTNUMBER: u16 = 0x123a;
pub const TWEI_PAGENUMBER: u16 = 0x123b;
pub const TWEI_PAGESIDE: u16 = 0x1245;

// Language and country.
pub const TWLG_USA: u16 = 0xd;
pub const TWCY_USA: u16 = 1;

// Capabilities.
pub const CAP_XFERCOUNT: u16 = 1;
pub const ICAP_COMPRESSION: u16 = 0x100;
pub const ICAP_PIXELTYPE: u16 = 0x101;
pub const ICAP_XFERMECH: u16 = 0x103;
pub const CAP_SUPPORTEDCAPS: u16 = 0x1005;
pub const CAP_FEEDERENABLED: u16 = 0x1002;
pub const CAP_FEEDERLOADED: u16 = 0x1003;
pub const CAP_AUTOFEED: u16 = 0x1007;
pub const CAP_INDICATORS: u16 = 0x100b;
pub const CAP_DUPLEXENABLED: u16 = 0x1013;
pub const ICAP_XRESOLUTION: u16 = 0x1118;
pub const ICAP_BITORDER: u16 = 0x111c;
pub const ICAP_PIXELFLAVOR: u16 = 0x111f;
pub const ICAP_BITDEPTH: u16 = 0x112b;
pub const ICAP_EXTIMAGEINFO: u16 = 0x112f;
pub const ICAP_SUPPORTEDEXTIMAGEINFO: u16 = 0x115e;

/// `DSMENTRYPROC`: the DSM's one entry point.
pub type DsmEntryProc = unsafe extern "system" fn(
    origin: *mut TW_IDENTITY,
    dest: *mut TW_IDENTITY,
    dg: u32,
    dat: u16,
    msg: u16,
    data: *mut c_void,
) -> u16;
/// `TWAINCALLBACKPROC`: the application's callback, with the entry point's
/// signature.
pub type TwainCallbackProc = DsmEntryProc;
/// `DSM_MEMALLOCATE`.
pub type DsmMemAllocate = unsafe extern "system" fn(size: u32) -> *mut c_void;
/// `DSM_MEMFREE`.
pub type DsmMemFree = unsafe extern "system" fn(handle: *mut c_void);
/// `DSM_MEMLOCK`.
pub type DsmMemLock = unsafe extern "system" fn(handle: *mut c_void) -> *mut c_void;
/// `DSM_MEMUNLOCK`.
pub type DsmMemUnlock = unsafe extern "system" fn(handle: *mut c_void);

/// `TW_STR32`: 32 characters and a NUL, in 34 bytes.
#[repr(transparent)]
#[derive(Clone, Copy)]
pub struct TW_STR32(pub [u8; 34]);

impl Default for TW_STR32 {
    fn default() -> Self {
        TW_STR32([0; 34])
    }
}

impl TW_STR32 {
    /// The string, cut to 32 bytes, NUL-terminated.
    pub fn new(text: &str) -> Self {
        let mut s = TW_STR32::default();
        let bytes = text.as_bytes();
        let n = bytes.len().min(32);
        s.0[..n].copy_from_slice(&bytes[..n]);
        s
    }

    /// The bytes up to the first NUL (or all 34), read as UTF-8 with
    /// replacement: TWAIN's strings are in the system's code page.
    pub fn text(&self) -> String {
        let end = self.0.iter().position(|&b| b == 0).unwrap_or(self.0.len());
        String::from_utf8_lossy(&self.0[..end]).into_owned()
    }
}

#[repr(C, packed(2))]
#[derive(Clone, Copy, Default)]
pub struct TW_FIX32 {
    pub Whole: i16,
    pub Frac: u16,
}

impl TW_FIX32 {
    pub fn to_f64(self) -> f64 {
        let whole = self.Whole;
        let frac = self.Frac;
        f64::from(whole) + f64::from(frac) / 65536.0
    }
}

#[repr(C, packed(2))]
#[derive(Clone, Copy, Default)]
pub struct TW_VERSION {
    pub MajorNum: u16,
    pub MinorNum: u16,
    pub Language: u16,
    pub Country: u16,
    pub Info: TW_STR32,
}

#[repr(C, packed(2))]
#[derive(Clone, Copy, Default)]
pub struct TW_IDENTITY {
    pub Id: u32,
    pub Version: TW_VERSION,
    pub ProtocolMajor: u16,
    pub ProtocolMinor: u16,
    pub SupportedGroups: u32,
    pub Manufacturer: TW_STR32,
    pub ProductFamily: TW_STR32,
    pub ProductName: TW_STR32,
}

#[repr(C, packed(2))]
#[derive(Clone, Copy)]
pub struct TW_CAPABILITY {
    pub Cap: u16,
    pub ConType: u16,
    pub hContainer: *mut c_void,
}

impl Default for TW_CAPABILITY {
    fn default() -> Self {
        TW_CAPABILITY {
            Cap: 0,
            ConType: 0,
            hContainer: null_mut(),
        }
    }
}

#[repr(C, packed(2))]
#[derive(Clone, Copy, Default)]
pub struct TW_ONEVALUE {
    pub ItemType: u16,
    pub Item: u32,
}

/// `TW_ENUMERATION`, whose `ItemList` runs on past the struct's end, its
/// items not aligned: read them with `read_unaligned`.
#[repr(C, packed(2))]
#[derive(Clone, Copy, Default)]
pub struct TW_ENUMERATION {
    pub ItemType: u16,
    pub NumItems: u32,
    pub CurrentIndex: u32,
    pub DefaultIndex: u32,
    pub ItemList: [u8; 1],
}

#[repr(C, packed(2))]
#[derive(Clone, Copy)]
pub struct TW_USERINTERFACE {
    pub ShowUI: u16,
    pub ModalUI: u16,
    pub hParent: *mut c_void,
}

impl Default for TW_USERINTERFACE {
    fn default() -> Self {
        TW_USERINTERFACE {
            ShowUI: 0,
            ModalUI: 0,
            hParent: null_mut(),
        }
    }
}

#[repr(C, packed(2))]
#[derive(Clone, Copy)]
pub struct TW_EVENT {
    pub pEvent: *mut c_void,
    pub TWMessage: u16,
}

impl Default for TW_EVENT {
    fn default() -> Self {
        TW_EVENT {
            pEvent: null_mut(),
            TWMessage: 0,
        }
    }
}

/// `TW_PENDINGXFERS`; `EOJ` stands for the union of `EOJ` and `Reserved`.
#[repr(C, packed(2))]
#[derive(Clone, Copy, Default)]
pub struct TW_PENDINGXFERS {
    pub Count: u16,
    pub EOJ: u32,
}

#[repr(C, packed(2))]
#[derive(Clone, Copy, Default)]
pub struct TW_IMAGEINFO {
    pub XResolution: TW_FIX32,
    pub YResolution: TW_FIX32,
    pub ImageWidth: i32,
    pub ImageLength: i32,
    pub SamplesPerPixel: i16,
    pub BitsPerSample: [i16; 8],
    pub BitsPerPixel: i16,
    pub Planar: u16,
    pub PixelType: i16,
    pub Compression: u16,
}

#[repr(C, packed(2))]
#[derive(Clone, Copy, Default)]
pub struct TW_SETUPMEMXFER {
    pub MinBufSize: u32,
    pub MaxBufSize: u32,
    pub Preferred: u32,
}

#[repr(C, packed(2))]
#[derive(Clone, Copy)]
pub struct TW_MEMORY {
    pub Flags: u32,
    pub Length: u32,
    pub TheMem: *mut c_void,
}

impl Default for TW_MEMORY {
    fn default() -> Self {
        TW_MEMORY {
            Flags: 0,
            Length: 0,
            TheMem: null_mut(),
        }
    }
}

#[repr(C, packed(2))]
#[derive(Clone, Copy, Default)]
pub struct TW_IMAGEMEMXFER {
    pub Compression: u16,
    pub BytesPerRow: u32,
    pub Columns: u32,
    pub Rows: u32,
    pub XOffset: u32,
    pub YOffset: u32,
    pub BytesWritten: u32,
    pub Memory: TW_MEMORY,
}

/// `TW_STATUS`; `Data` stands for the union of `Data` and `Reserved`.
#[repr(C, packed(2))]
#[derive(Clone, Copy, Default)]
pub struct TW_STATUS {
    pub ConditionCode: u16,
    pub Data: u16,
}

#[repr(C, packed(2))]
#[derive(Clone, Copy, Default)]
pub struct TW_ENTRYPOINT {
    pub Size: u32,
    pub DSM_Entry: Option<DsmEntryProc>,
    pub DSM_MemAllocate: Option<DsmMemAllocate>,
    pub DSM_MemFree: Option<DsmMemFree>,
    pub DSM_MemLock: Option<DsmMemLock>,
    pub DSM_MemUnlock: Option<DsmMemUnlock>,
}

/// `TW_CALLBACK2`; `twain.h` types `CallBackProc` as a `TW_MEMREF`, a pointer
/// of the same size as the function pointer here.
#[repr(C, packed(2))]
#[derive(Clone, Copy, Default)]
pub struct TW_CALLBACK2 {
    pub CallBackProc: Option<TwainCallbackProc>,
    pub RefCon: usize,
    pub Message: i16,
}

/// `TW_INFO`; `ReturnCode` stands for the union of `ReturnCode` and the
/// deprecated `CondCode`.
#[repr(C, packed(2))]
#[derive(Clone, Copy, Default)]
pub struct TW_INFO {
    pub InfoID: u16,
    pub ItemType: u16,
    pub NumItems: u16,
    pub ReturnCode: u16,
    pub Item: usize,
}

/// `TW_EXTIMAGEINFO` with room for one `TW_INFO`; a request for more is a
/// larger buffer laid out the same way.
#[repr(C, packed(2))]
#[derive(Clone, Copy, Default)]
pub struct TW_EXTIMAGEINFO {
    pub NumInfos: u32,
    pub Info: [TW_INFO; 1],
}

// The layouts, as MSVC lays out twain.h's structs under `#pragma pack (2)`:
// the same in both bitnesses unless they hold a pointer.
const _: () = {
    use core::mem::{offset_of, size_of};
    assert!(size_of::<TW_STR32>() == 34);
    assert!(size_of::<TW_FIX32>() == 4);
    assert!(size_of::<TW_VERSION>() == 42);
    assert!(size_of::<TW_IDENTITY>() == 156);
    assert!(offset_of!(TW_IDENTITY, Version) == 4);
    assert!(offset_of!(TW_IDENTITY, ProtocolMajor) == 46);
    assert!(offset_of!(TW_IDENTITY, SupportedGroups) == 50);
    assert!(offset_of!(TW_IDENTITY, Manufacturer) == 54);
    assert!(offset_of!(TW_IDENTITY, ProductFamily) == 88);
    assert!(offset_of!(TW_IDENTITY, ProductName) == 122);
    assert!(size_of::<TW_ONEVALUE>() == 6);
    assert!(offset_of!(TW_ONEVALUE, Item) == 2);
    assert!(size_of::<TW_ENUMERATION>() == 16);
    assert!(offset_of!(TW_ENUMERATION, ItemList) == 14);
    assert!(size_of::<TW_PENDINGXFERS>() == 6);
    assert!(size_of::<TW_IMAGEINFO>() == 42);
    assert!(offset_of!(TW_IMAGEINFO, BitsPerSample) == 18);
    assert!(offset_of!(TW_IMAGEINFO, PixelType) == 38);
    assert!(size_of::<TW_SETUPMEMXFER>() == 12);
    assert!(size_of::<TW_STATUS>() == 4);
    assert!(offset_of!(TW_CAPABILITY, hContainer) == 4);
    assert!(offset_of!(TW_USERINTERFACE, hParent) == 4);
    assert!(offset_of!(TW_IMAGEMEMXFER, Memory) == 26);
    assert!(offset_of!(TW_MEMORY, TheMem) == 8);
    assert!(offset_of!(TW_INFO, Item) == 8);
    assert!(offset_of!(TW_EXTIMAGEINFO, Info) == 4);
};

#[cfg(target_pointer_width = "64")]
const _: () = {
    use core::mem::{offset_of, size_of};
    assert!(size_of::<TW_CAPABILITY>() == 12);
    assert!(size_of::<TW_USERINTERFACE>() == 12);
    assert!(size_of::<TW_EVENT>() == 10);
    assert!(offset_of!(TW_EVENT, TWMessage) == 8);
    assert!(size_of::<TW_MEMORY>() == 16);
    assert!(size_of::<TW_IMAGEMEMXFER>() == 42);
    assert!(size_of::<TW_ENTRYPOINT>() == 44);
    assert!(offset_of!(TW_ENTRYPOINT, DSM_Entry) == 4);
    assert!(offset_of!(TW_ENTRYPOINT, DSM_MemAllocate) == 12);
    assert!(offset_of!(TW_ENTRYPOINT, DSM_MemFree) == 20);
    assert!(offset_of!(TW_ENTRYPOINT, DSM_MemLock) == 28);
    assert!(offset_of!(TW_ENTRYPOINT, DSM_MemUnlock) == 36);
    assert!(size_of::<TW_CALLBACK2>() == 18);
    assert!(offset_of!(TW_CALLBACK2, RefCon) == 8);
    assert!(offset_of!(TW_CALLBACK2, Message) == 16);
    assert!(size_of::<TW_INFO>() == 16);
    assert!(size_of::<TW_EXTIMAGEINFO>() == 20);
};

#[cfg(target_pointer_width = "32")]
const _: () = {
    use core::mem::{offset_of, size_of};
    assert!(size_of::<TW_CAPABILITY>() == 8);
    assert!(size_of::<TW_USERINTERFACE>() == 8);
    assert!(size_of::<TW_EVENT>() == 6);
    assert!(offset_of!(TW_EVENT, TWMessage) == 4);
    assert!(size_of::<TW_MEMORY>() == 12);
    assert!(size_of::<TW_IMAGEMEMXFER>() == 38);
    assert!(size_of::<TW_ENTRYPOINT>() == 24);
    assert!(offset_of!(TW_ENTRYPOINT, DSM_Entry) == 4);
    assert!(offset_of!(TW_ENTRYPOINT, DSM_MemAllocate) == 8);
    assert!(offset_of!(TW_ENTRYPOINT, DSM_MemFree) == 12);
    assert!(offset_of!(TW_ENTRYPOINT, DSM_MemLock) == 16);
    assert!(offset_of!(TW_ENTRYPOINT, DSM_MemUnlock) == 20);
    assert!(size_of::<TW_CALLBACK2>() == 10);
    assert!(offset_of!(TW_CALLBACK2, RefCon) == 4);
    assert!(offset_of!(TW_CALLBACK2, Message) == 8);
    assert!(size_of::<TW_INFO>() == 12);
    assert!(size_of::<TW_EXTIMAGEINFO>() == 16);
};

/// A return code's name.
pub fn rc_name(rc: u16) -> String {
    let name = match rc {
        TWRC_SUCCESS => "TWRC_SUCCESS",
        TWRC_FAILURE => "TWRC_FAILURE",
        TWRC_CHECKSTATUS => "TWRC_CHECKSTATUS",
        TWRC_CANCEL => "TWRC_CANCEL",
        TWRC_DSEVENT => "TWRC_DSEVENT",
        TWRC_NOTDSEVENT => "TWRC_NOTDSEVENT",
        TWRC_XFERDONE => "TWRC_XFERDONE",
        TWRC_ENDOFLIST => "TWRC_ENDOFLIST",
        TWRC_INFONOTSUPPORTED => "TWRC_INFONOTSUPPORTED",
        TWRC_DATANOTAVAILABLE => "TWRC_DATANOTAVAILABLE",
        TWRC_BUSY => "TWRC_BUSY",
        TWRC_SCANNERLOCKED => "TWRC_SCANNERLOCKED",
        other => return format!("TWRC {other:#06x}"),
    };
    name.to_owned()
}

/// A condition code's name.
pub fn cc_name(cc: u16) -> String {
    let name = match cc {
        TWCC_SUCCESS => "TWCC_SUCCESS",
        TWCC_BUMMER => "TWCC_BUMMER",
        TWCC_LOWMEMORY => "TWCC_LOWMEMORY",
        TWCC_NODS => "TWCC_NODS",
        TWCC_MAXCONNECTIONS => "TWCC_MAXCONNECTIONS",
        TWCC_OPERATIONERROR => "TWCC_OPERATIONERROR",
        TWCC_BADCAP => "TWCC_BADCAP",
        TWCC_BADPROTOCOL => "TWCC_BADPROTOCOL",
        TWCC_BADVALUE => "TWCC_BADVALUE",
        TWCC_SEQERROR => "TWCC_SEQERROR",
        TWCC_BADDEST => "TWCC_BADDEST",
        TWCC_CAPUNSUPPORTED => "TWCC_CAPUNSUPPORTED",
        TWCC_CAPBADOPERATION => "TWCC_CAPBADOPERATION",
        TWCC_CAPSEQERROR => "TWCC_CAPSEQERROR",
        TWCC_DENIED => "TWCC_DENIED",
        TWCC_FILEEXISTS => "TWCC_FILEEXISTS",
        TWCC_FILENOTFOUND => "TWCC_FILENOTFOUND",
        TWCC_NOTEMPTY => "TWCC_NOTEMPTY",
        TWCC_PAPERJAM => "TWCC_PAPERJAM",
        TWCC_PAPERDOUBLEFEED => "TWCC_PAPERDOUBLEFEED",
        TWCC_FILEWRITEERROR => "TWCC_FILEWRITEERROR",
        TWCC_CHECKDEVICEONLINE => "TWCC_CHECKDEVICEONLINE",
        TWCC_INTERLOCK => "TWCC_INTERLOCK",
        TWCC_DAMAGEDCORNER => "TWCC_DAMAGEDCORNER",
        TWCC_FOCUSERROR => "TWCC_FOCUSERROR",
        TWCC_DOCTOOLIGHT => "TWCC_DOCTOOLIGHT",
        TWCC_DOCTOODARK => "TWCC_DOCTOODARK",
        TWCC_NOMEDIA => "TWCC_NOMEDIA",
        other => return format!("TWCC {other:#06x}"),
    };
    name.to_owned()
}
