//! TWAIN's return codes, condition codes and triplets, named.

use core::fmt;

use crate::ffi;

/// The first constant whose name starts with `prefix` and whose value is
/// `value`: `twain.h`'s own name before an alias, as the bindings list them.
fn name_of(prefix: &str, value: i64) -> Option<&'static str> {
    ffi::constants()
        .find(|&(name, v)| v == value && name.starts_with(prefix))
        .map(|(name, _)| name)
}

/// What a triplet returned: `TWRC_SUCCESS`, `TWRC_FAILURE` and the rest.
/// Kept whole, so a code no version of TWAIN defines (a driver's own, from
/// `TWRC_CUSTOMBASE` up) says what it was.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct ReturnCode(pub u16);

impl ReturnCode {
    /// `TWRC_SUCCESS`.
    pub const SUCCESS: Self = Self(ffi::TWRC_SUCCESS);
    /// `TWRC_FAILURE`.
    pub const FAILURE: Self = Self(ffi::TWRC_FAILURE);
    /// `TWRC_CHECKSTATUS`.
    pub const CHECKSTATUS: Self = Self(ffi::TWRC_CHECKSTATUS);
    /// `TWRC_CANCEL`.
    pub const CANCEL: Self = Self(ffi::TWRC_CANCEL);
    /// `TWRC_DSEVENT`.
    pub const DSEVENT: Self = Self(ffi::TWRC_DSEVENT);
    /// `TWRC_NOTDSEVENT`.
    pub const NOTDSEVENT: Self = Self(ffi::TWRC_NOTDSEVENT);
    /// `TWRC_XFERDONE`.
    pub const XFERDONE: Self = Self(ffi::TWRC_XFERDONE);
    /// `TWRC_ENDOFLIST`.
    pub const ENDOFLIST: Self = Self(ffi::TWRC_ENDOFLIST);
    /// `TWRC_INFONOTSUPPORTED`.
    pub const INFONOTSUPPORTED: Self = Self(ffi::TWRC_INFONOTSUPPORTED);
    /// `TWRC_DATANOTAVAILABLE`.
    pub const DATANOTAVAILABLE: Self = Self(ffi::TWRC_DATANOTAVAILABLE);
    /// `TWRC_BUSY`.
    pub const BUSY: Self = Self(ffi::TWRC_BUSY);
    /// `TWRC_SCANNERLOCKED`.
    pub const SCANNERLOCKED: Self = Self(ffi::TWRC_SCANNERLOCKED);

    /// `twain.h`'s name for it, if it has one (`"TWRC_SUCCESS"`).
    pub fn name(self) -> Option<&'static str> {
        name_of("TWRC_", self.0.into())
    }
}

/// Why a triplet failed, as `DAT_STATUS` reports it: `TWCC_SEQERROR`,
/// `TWCC_BADCAP` and the rest. Kept whole, as [`ReturnCode`] is.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct ConditionCode(pub u16);

impl ConditionCode {
    /// `TWCC_SUCCESS`.
    pub const SUCCESS: Self = Self(ffi::TWCC_SUCCESS);
    /// `TWCC_BUMMER`.
    pub const BUMMER: Self = Self(ffi::TWCC_BUMMER);
    /// `TWCC_LOWMEMORY`.
    pub const LOWMEMORY: Self = Self(ffi::TWCC_LOWMEMORY);
    /// `TWCC_NODS`.
    pub const NODS: Self = Self(ffi::TWCC_NODS);
    /// `TWCC_MAXCONNECTIONS`.
    pub const MAXCONNECTIONS: Self = Self(ffi::TWCC_MAXCONNECTIONS);
    /// `TWCC_OPERATIONERROR`.
    pub const OPERATIONERROR: Self = Self(ffi::TWCC_OPERATIONERROR);
    /// `TWCC_BADCAP`.
    pub const BADCAP: Self = Self(ffi::TWCC_BADCAP);
    /// `TWCC_BADPROTOCOL`.
    pub const BADPROTOCOL: Self = Self(ffi::TWCC_BADPROTOCOL);
    /// `TWCC_BADVALUE`.
    pub const BADVALUE: Self = Self(ffi::TWCC_BADVALUE);
    /// `TWCC_SEQERROR`.
    pub const SEQERROR: Self = Self(ffi::TWCC_SEQERROR);
    /// `TWCC_BADDEST`.
    pub const BADDEST: Self = Self(ffi::TWCC_BADDEST);
    /// `TWCC_CAPUNSUPPORTED`.
    pub const CAPUNSUPPORTED: Self = Self(ffi::TWCC_CAPUNSUPPORTED);
    /// `TWCC_CAPBADOPERATION`.
    pub const CAPBADOPERATION: Self = Self(ffi::TWCC_CAPBADOPERATION);
    /// `TWCC_CAPSEQERROR`.
    pub const CAPSEQERROR: Self = Self(ffi::TWCC_CAPSEQERROR);
    /// `TWCC_DENIED`.
    pub const DENIED: Self = Self(ffi::TWCC_DENIED);
    /// `TWCC_FILEEXISTS`.
    pub const FILEEXISTS: Self = Self(ffi::TWCC_FILEEXISTS);
    /// `TWCC_FILENOTFOUND`.
    pub const FILENOTFOUND: Self = Self(ffi::TWCC_FILENOTFOUND);
    /// `TWCC_NOTEMPTY`.
    pub const NOTEMPTY: Self = Self(ffi::TWCC_NOTEMPTY);
    /// `TWCC_PAPERJAM`.
    pub const PAPERJAM: Self = Self(ffi::TWCC_PAPERJAM);
    /// `TWCC_PAPERDOUBLEFEED`.
    pub const PAPERDOUBLEFEED: Self = Self(ffi::TWCC_PAPERDOUBLEFEED);
    /// `TWCC_FILEWRITEERROR`.
    pub const FILEWRITEERROR: Self = Self(ffi::TWCC_FILEWRITEERROR);
    /// `TWCC_CHECKDEVICEONLINE`.
    pub const CHECKDEVICEONLINE: Self = Self(ffi::TWCC_CHECKDEVICEONLINE);
    /// `TWCC_INTERLOCK`.
    pub const INTERLOCK: Self = Self(ffi::TWCC_INTERLOCK);
    /// `TWCC_DAMAGEDCORNER`.
    pub const DAMAGEDCORNER: Self = Self(ffi::TWCC_DAMAGEDCORNER);
    /// `TWCC_FOCUSERROR`.
    pub const FOCUSERROR: Self = Self(ffi::TWCC_FOCUSERROR);
    /// `TWCC_DOCTOOLIGHT`.
    pub const DOCTOOLIGHT: Self = Self(ffi::TWCC_DOCTOOLIGHT);
    /// `TWCC_DOCTOODARK`.
    pub const DOCTOODARK: Self = Self(ffi::TWCC_DOCTOODARK);
    /// `TWCC_NOMEDIA`.
    pub const NOMEDIA: Self = Self(ffi::TWCC_NOMEDIA);

    /// `twain.h`'s name for it, if it has one (`"TWCC_SEQERROR"`).
    pub fn name(self) -> Option<&'static str> {
        name_of("TWCC_", self.0.into())
    }
}

/// A call's three parts: data group, data argument type and message
/// (`DG_CONTROL`/`DAT_PARENT`/`MSG_OPENDSM`).
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Triplet {
    /// Data group (`DG_`).
    pub dg: u32,
    /// Data argument type (`DAT_`).
    pub dat: u16,
    /// Message (`MSG_`).
    pub msg: u16,
}

impl Triplet {
    /// A triplet from its three parts.
    pub const fn new(dg: u32, dat: u16, msg: u16) -> Self {
        Triplet { dg, dat, msg }
    }
}

/// A name, or what it would stand for: the prefix and the value in hex.
fn write_named(
    f: &mut fmt::Formatter<'_>,
    name: Option<&str>,
    prefix: &str,
    value: impl fmt::LowerHex,
) -> fmt::Result {
    match name {
        Some(name) => f.write_str(name),
        None => write!(f, "{prefix} {value:#06x}"),
    }
}

impl fmt::Display for ReturnCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write_named(f, self.name(), "TWRC", self.0)
    }
}

impl fmt::Debug for ReturnCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ReturnCode({self})")
    }
}

impl fmt::Display for ConditionCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write_named(f, self.name(), "TWCC", self.0)
    }
}

impl fmt::Debug for ConditionCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ConditionCode({self})")
    }
}

impl fmt::Display for Triplet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write_named(f, name_of("DG_", self.dg.into()), "DG", self.dg)?;
        f.write_str("/")?;
        write_named(f, name_of("DAT_", self.dat.into()), "DAT", self.dat)?;
        f.write_str("/")?;
        write_named(f, name_of("MSG_", self.msg.into()), "MSG", self.msg)
    }
}

impl fmt::Debug for Triplet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Triplet({self})")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn codes_are_named_as_twain_h_names_them() {
        assert_eq!(ReturnCode::SUCCESS.to_string(), "TWRC_SUCCESS");
        assert_eq!(ReturnCode::SCANNERLOCKED.to_string(), "TWRC_SCANNERLOCKED");
        assert_eq!(ReturnCode(0x8000).to_string(), "TWRC_CUSTOMBASE");
        assert_eq!(ReturnCode(0x8001).to_string(), "TWRC 0x8001");
        assert_eq!(ConditionCode::SUCCESS.to_string(), "TWCC_SUCCESS");
        assert_eq!(ConditionCode::SEQERROR.to_string(), "TWCC_SEQERROR");
        assert_eq!(ConditionCode(11).name(), Some("TWCC_SEQERROR"));
        assert_eq!(ConditionCode(7).to_string(), "TWCC 0x0007");
        assert_eq!(
            format!("{:?}", ConditionCode::BADCAP),
            "ConditionCode(TWCC_BADCAP)"
        );
    }

    #[test]
    fn triplets_are_named_part_by_part() {
        let open = Triplet::new(ffi::DG_CONTROL, ffi::DAT_PARENT, ffi::MSG_OPENDSM);
        assert_eq!(open.to_string(), "DG_CONTROL/DAT_PARENT/MSG_OPENDSM");
        let memory = Triplet::new(ffi::DG_IMAGE, ffi::DAT_IMAGEMEMXFER, ffi::MSG_GET);
        assert_eq!(memory.to_string(), "DG_IMAGE/DAT_IMAGEMEMXFER/MSG_GET");
        assert_eq!(
            Triplet::new(8, 0x7777, ffi::MSG_OPENDSM).to_string(),
            "DG 0x0008/DAT 0x7777/MSG_OPENDSM"
        );
    }

    /// A name is found by the first constant with its prefix and value, so
    /// no two of a prefix the codes and triplets use may share a value: if
    /// a later `twain.h` gives two one value, the name they'd get is chosen
    /// here, not by the table's order.
    #[test]
    fn no_two_names_of_a_prefix_share_a_value() {
        for prefix in ["DG_", "DAT_", "MSG_", "TWRC_", "TWCC_"] {
            let mut seen: HashMap<i64, &str> = HashMap::new();
            for (name, value) in ffi::constants().filter(|(n, _)| n.starts_with(prefix)) {
                if let Some(first) = seen.insert(value, name) {
                    panic!("{first} and {name} are both {value:#x}");
                }
            }
        }
    }

    #[test]
    fn every_constant_is_named_once() {
        let mut seen = std::collections::HashSet::new();
        for (name, _) in ffi::constants() {
            assert!(seen.insert(name), "{name} is listed twice");
        }
        assert_eq!(seen.len(), 1192 + 36);
    }
}
