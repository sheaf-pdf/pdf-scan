//! SANE's statuses, frame formats and version codes, as integers that may
//! hold values SANE doesn't define.

use core::fmt;

use crate::ffi;

/// A status, as a SANE call returns it.
///
/// An integer, never a Rust enum: a backend may return a code SANE doesn't
/// define, and it's kept as it came. The constants name the codes SANE
/// defines; [`WARMING_UP`](Self::WARMING_UP) and
/// [`HW_LOCKED`](Self::HW_LOCKED) are reserved for a later SANE, and no
/// backend should return them.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Status(pub i32);

impl Status {
    /// Everything went well.
    pub const GOOD: Status = Status(ffi::SANE_STATUS_GOOD);
    /// The operation isn't supported.
    pub const UNSUPPORTED: Status = Status(ffi::SANE_STATUS_UNSUPPORTED);
    /// The operation was cancelled.
    pub const CANCELLED: Status = Status(ffi::SANE_STATUS_CANCELLED);
    /// The device is busy; try again later.
    pub const DEVICE_BUSY: Status = Status(ffi::SANE_STATUS_DEVICE_BUSY);
    /// The data is invalid; from `sane_open`, there's no such device.
    pub const INVAL: Status = Status(ffi::SANE_STATUS_INVAL);
    /// No more data: the end of a frame.
    pub const EOF: Status = Status(ffi::SANE_STATUS_EOF);
    /// The document feeder jammed.
    pub const JAMMED: Status = Status(ffi::SANE_STATUS_JAMMED);
    /// The document feeder is out of documents.
    pub const NO_DOCS: Status = Status(ffi::SANE_STATUS_NO_DOCS);
    /// The scanner's cover is open.
    pub const COVER_OPEN: Status = Status(ffi::SANE_STATUS_COVER_OPEN);
    /// An error in the device's input or output.
    pub const IO_ERROR: Status = Status(ffi::SANE_STATUS_IO_ERROR);
    /// Out of memory.
    pub const NO_MEM: Status = Status(ffi::SANE_STATUS_NO_MEM);
    /// Access to a resource was denied.
    pub const ACCESS_DENIED: Status = Status(ffi::SANE_STATUS_ACCESS_DENIED);
    /// Reserved for a later SANE: the lamp isn't ready, so try again.
    pub const WARMING_UP: Status = Status(ffi::SANE_STATUS_WARMING_UP);
    /// Reserved for a later SANE: the scanner's mechanism is locked for
    /// transport.
    pub const HW_LOCKED: Status = Status(ffi::SANE_STATUS_HW_LOCKED);

    /// The status's name, without SANE's `SANE_STATUS_` prefix, or `None`
    /// for a code SANE doesn't define. The two reserved codes have names.
    pub fn name(self) -> Option<&'static str> {
        Some(match self.0 {
            ffi::SANE_STATUS_GOOD => "GOOD",
            ffi::SANE_STATUS_UNSUPPORTED => "UNSUPPORTED",
            ffi::SANE_STATUS_CANCELLED => "CANCELLED",
            ffi::SANE_STATUS_DEVICE_BUSY => "DEVICE_BUSY",
            ffi::SANE_STATUS_INVAL => "INVAL",
            ffi::SANE_STATUS_EOF => "EOF",
            ffi::SANE_STATUS_JAMMED => "JAMMED",
            ffi::SANE_STATUS_NO_DOCS => "NO_DOCS",
            ffi::SANE_STATUS_COVER_OPEN => "COVER_OPEN",
            ffi::SANE_STATUS_IO_ERROR => "IO_ERROR",
            ffi::SANE_STATUS_NO_MEM => "NO_MEM",
            ffi::SANE_STATUS_ACCESS_DENIED => "ACCESS_DENIED",
            ffi::SANE_STATUS_WARMING_UP => "WARMING_UP",
            ffi::SANE_STATUS_HW_LOCKED => "HW_LOCKED",
            _ => return None,
        })
    }

    /// Whether this is one of the two codes reserved for a later SANE.
    pub fn is_reserved(self) -> bool {
        matches!(
            self.0,
            ffi::SANE_STATUS_WARMING_UP | ffi::SANE_STATUS_HW_LOCKED
        )
    }

    /// Whether this is [`GOOD`](Self::GOOD).
    pub fn is_good(self) -> bool {
        self == Self::GOOD
    }
}

/// The name and the code: `JAMMED (6)`; `42 (not a SANE status)` for a code
/// SANE doesn't define.
impl fmt::Display for Status {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.name() {
            Some(name) if self.is_reserved() => write!(f, "{name} ({}, reserved)", self.0),
            Some(name) => write!(f, "{name} ({})", self.0),
            None => write!(f, "{} (not a SANE status)", self.0),
        }
    }
}

impl fmt::Debug for Status {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.name() {
            Some(name) => write!(f, "Status::{name}"),
            None => write!(f, "Status({})", self.0),
        }
    }
}

/// What a frame holds, as SANE's parameters give it.
///
/// An integer, never a Rust enum, like [`Status`]. SANE 1 defines five
/// formats; the rest of the constants are reserved for a later SANE (JPEG,
/// CCITT, infrared and others), and no backend should send them.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FrameFormat(pub i32);

impl FrameFormat {
    /// Gray: one band covering what the eye sees.
    pub const GRAY: FrameFormat = FrameFormat(ffi::SANE_FRAME_GRAY);
    /// Red, green and blue, interleaved pixel by pixel.
    pub const RGB: FrameFormat = FrameFormat(ffi::SANE_FRAME_RGB);
    /// The red band alone, one pass of three.
    pub const RED: FrameFormat = FrameFormat(ffi::SANE_FRAME_RED);
    /// The green band alone, one pass of three.
    pub const GREEN: FrameFormat = FrameFormat(ffi::SANE_FRAME_GREEN);
    /// The blue band alone, one pass of three.
    pub const BLUE: FrameFormat = FrameFormat(ffi::SANE_FRAME_BLUE);
    /// Reserved: text, specific to the backend.
    pub const TEXT: FrameFormat = FrameFormat(ffi::SANE_FRAME_TEXT);
    /// Reserved: a complete baseline JPEG file.
    pub const JPEG: FrameFormat = FrameFormat(ffi::SANE_FRAME_JPEG);
    /// Reserved: CCITT Group 3, one-dimensional.
    pub const G31D: FrameFormat = FrameFormat(ffi::SANE_FRAME_G31D);
    /// Reserved: CCITT Group 3, two-dimensional.
    pub const G32D: FrameFormat = FrameFormat(ffi::SANE_FRAME_G32D);
    /// Reserved: CCITT Group 4.
    pub const G42D: FrameFormat = FrameFormat(ffi::SANE_FRAME_G42D);
    /// Reserved: an infrared band alone.
    pub const IR: FrameFormat = FrameFormat(ffi::SANE_FRAME_IR);
    /// Reserved: red, green, blue and infrared.
    pub const RGBI: FrameFormat = FrameFormat(ffi::SANE_FRAME_RGBI);
    /// Reserved: gray and infrared.
    pub const GRAYI: FrameFormat = FrameFormat(ffi::SANE_FRAME_GRAYI);
    /// Reserved: XML of no set schema.
    pub const XML: FrameFormat = FrameFormat(ffi::SANE_FRAME_XML);

    /// The format's name, without SANE's `SANE_FRAME_` prefix, or `None`
    /// for a code SANE doesn't define. The reserved codes have names.
    pub fn name(self) -> Option<&'static str> {
        Some(match self.0 {
            ffi::SANE_FRAME_GRAY => "GRAY",
            ffi::SANE_FRAME_RGB => "RGB",
            ffi::SANE_FRAME_RED => "RED",
            ffi::SANE_FRAME_GREEN => "GREEN",
            ffi::SANE_FRAME_BLUE => "BLUE",
            ffi::SANE_FRAME_TEXT => "TEXT",
            ffi::SANE_FRAME_JPEG => "JPEG",
            ffi::SANE_FRAME_G31D => "G31D",
            ffi::SANE_FRAME_G32D => "G32D",
            ffi::SANE_FRAME_G42D => "G42D",
            ffi::SANE_FRAME_IR => "IR",
            ffi::SANE_FRAME_RGBI => "RGBI",
            ffi::SANE_FRAME_GRAYI => "GRAYI",
            ffi::SANE_FRAME_XML => "XML",
            _ => return None,
        })
    }

    /// Whether this is one of the codes reserved for a later SANE.
    pub fn is_reserved(self) -> bool {
        (ffi::SANE_FRAME_TEXT..=ffi::SANE_FRAME_XML).contains(&self.0)
    }
}

/// The name and the code: `RGB (1)`; `7 (not a SANE frame format)` for a
/// code SANE doesn't define.
impl fmt::Display for FrameFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.name() {
            Some(name) if self.is_reserved() => write!(f, "{name} ({}, reserved)", self.0),
            Some(name) => write!(f, "{name} ({})", self.0),
            None => write!(f, "{} (not a SANE frame format)", self.0),
        }
    }
}

impl fmt::Debug for FrameFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.name() {
            Some(name) => write!(f, "FrameFormat::{name}"),
            None => write!(f, "FrameFormat({})", self.0),
        }
    }
}

/// A SANE version, as `sane_init` gives its code: the major version of the
/// standard the library follows, then the backend's own minor and build
/// numbers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Version {
    /// The major version: 1 for every library this crate accepts.
    pub major: u8,
    /// The minor version.
    pub minor: u8,
    /// The build number.
    pub build: u16,
}

impl Version {
    /// The version a code holds: its top byte the major version, the next
    /// the minor, its low 16 bits the build.
    pub fn from_code(code: i32) -> Version {
        // Each part is masked to its width, so the casts lose nothing.
        Version {
            major: ffi::SANE_VERSION_MAJOR(code) as u8,
            minor: ffi::SANE_VERSION_MINOR(code) as u8,
            build: ffi::SANE_VERSION_BUILD(code) as u16,
        }
    }

    /// The version's code.
    pub fn code(self) -> i32 {
        ffi::SANE_VERSION_CODE(self.major.into(), self.minor.into(), self.build.into())
    }
}

/// `major.minor.build`.
impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.build)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn statuses_have_sanes_codes_and_names() {
        let named = [
            (Status::GOOD, 0, "GOOD"),
            (Status::UNSUPPORTED, 1, "UNSUPPORTED"),
            (Status::CANCELLED, 2, "CANCELLED"),
            (Status::DEVICE_BUSY, 3, "DEVICE_BUSY"),
            (Status::INVAL, 4, "INVAL"),
            (Status::EOF, 5, "EOF"),
            (Status::JAMMED, 6, "JAMMED"),
            (Status::NO_DOCS, 7, "NO_DOCS"),
            (Status::COVER_OPEN, 8, "COVER_OPEN"),
            (Status::IO_ERROR, 9, "IO_ERROR"),
            (Status::NO_MEM, 10, "NO_MEM"),
            (Status::ACCESS_DENIED, 11, "ACCESS_DENIED"),
            (Status::WARMING_UP, 12, "WARMING_UP"),
            (Status::HW_LOCKED, 13, "HW_LOCKED"),
        ];
        for (status, code, name) in named {
            assert_eq!(status.0, code);
            assert_eq!(status.name(), Some(name));
            assert_eq!(Status(code), status);
            assert_eq!(status.is_reserved(), code >= 12, "{status}");
        }
        assert!(Status::GOOD.is_good());
        assert!(!Status::EOF.is_good());
    }

    #[test]
    fn undefined_statuses_are_kept_and_unnamed() {
        for code in [14, 42, -1, i32::MIN, i32::MAX] {
            let status = Status(code);
            assert_eq!(status.name(), None);
            assert!(!status.is_reserved());
            assert_eq!(status.to_string(), format!("{code} (not a SANE status)"));
            assert_eq!(format!("{status:?}"), format!("Status({code})"));
        }
    }

    #[test]
    fn statuses_display_their_name_and_code() {
        assert_eq!(Status::JAMMED.to_string(), "JAMMED (6)");
        assert_eq!(Status::WARMING_UP.to_string(), "WARMING_UP (12, reserved)");
        assert_eq!(format!("{:?}", Status::NO_DOCS), "Status::NO_DOCS");
    }

    #[test]
    fn frame_formats_have_sanes_codes_and_names() {
        let defined = [
            (FrameFormat::GRAY, 0, "GRAY"),
            (FrameFormat::RGB, 1, "RGB"),
            (FrameFormat::RED, 2, "RED"),
            (FrameFormat::GREEN, 3, "GREEN"),
            (FrameFormat::BLUE, 4, "BLUE"),
        ];
        for (format, code, name) in defined {
            assert_eq!(format.0, code);
            assert_eq!(format.name(), Some(name));
            assert!(!format.is_reserved());
        }
        let reserved = [
            (FrameFormat::TEXT, 0x0A, "TEXT"),
            (FrameFormat::JPEG, 0x0B, "JPEG"),
            (FrameFormat::G31D, 0x0C, "G31D"),
            (FrameFormat::G32D, 0x0D, "G32D"),
            (FrameFormat::G42D, 0x0E, "G42D"),
            (FrameFormat::IR, 0x0F, "IR"),
            (FrameFormat::RGBI, 0x10, "RGBI"),
            (FrameFormat::GRAYI, 0x11, "GRAYI"),
            (FrameFormat::XML, 0x12, "XML"),
        ];
        for (format, code, name) in reserved {
            assert_eq!(format.0, code);
            assert_eq!(format.name(), Some(name));
            assert!(format.is_reserved());
            assert_eq!(format.to_string(), format!("{name} ({code}, reserved)"));
        }
        for code in [5, 9, 0x13, -1] {
            assert_eq!(FrameFormat(code).name(), None);
            assert!(!FrameFormat(code).is_reserved());
            assert_eq!(
                FrameFormat(code).to_string(),
                format!("{code} (not a SANE frame format)")
            );
        }
        assert_eq!(FrameFormat::RGB.to_string(), "RGB (1)");
    }

    #[test]
    fn versions_come_apart_and_back() {
        let version = Version::from_code(0x0104_0007);
        assert_eq!(
            version,
            Version {
                major: 1,
                minor: 4,
                build: 7
            }
        );
        assert_eq!(version.code(), 0x0104_0007);
        assert_eq!(version.to_string(), "1.4.7");
        // The top byte is the major version, whatever its sign as a word.
        let high = Version::from_code(0xFF02_FFFFu32 as i32);
        assert_eq!(
            high,
            Version {
                major: 255,
                minor: 2,
                build: 0xFFFF
            }
        );
        assert_eq!(high.code(), 0xFF02_FFFFu32 as i32);
        assert_eq!(
            Version::from_code(0),
            Version {
                major: 0,
                minor: 0,
                build: 0
            }
        );
    }

    #[test]
    fn fixed_point_macros_match_sanes_definitions() {
        assert_eq!(ffi::SANE_FIX(1.0), 65536);
        assert_eq!(ffi::SANE_FIX(1.5), 98304);
        assert_eq!(ffi::SANE_FIX(-2.25), -147456);
        assert_eq!(ffi::SANE_UNFIX(98304), 1.5);
        assert_eq!(ffi::SANE_UNFIX(50), 50.0 / 65536.0);
        assert!(ffi::SANE_OPTION_IS_ACTIVE(ffi::SANE_CAP_SOFT_SELECT));
        assert!(!ffi::SANE_OPTION_IS_ACTIVE(ffi::SANE_CAP_INACTIVE));
        assert!(ffi::SANE_OPTION_IS_SETTABLE(ffi::SANE_CAP_SOFT_SELECT));
        assert!(!ffi::SANE_OPTION_IS_SETTABLE(ffi::SANE_CAP_SOFT_DETECT));
    }
}
