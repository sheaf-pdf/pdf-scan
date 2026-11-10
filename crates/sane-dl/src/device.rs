//! Devices as SANE lists them.

use std::borrow::Cow;
use std::fmt;

use crate::SaneString;
use crate::config::backend_of;

/// A device's name, as SANE's bytes.
///
/// Kept as bytes, never decoded, since it goes back to `sane_open` exactly as
/// it came. With SANE's own library the name is the backend's, then `:`,
/// then the backend's name for the device (`test:0`, `fujitsu:fi-6130dj:…`):
/// [`backend`](Self::backend) gives the first part.
#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct DeviceName(Vec<u8>);

impl DeviceName {
    /// A name from its bytes.
    pub fn new(bytes: impl Into<Vec<u8>>) -> DeviceName {
        DeviceName(bytes.into())
    }

    /// The name's bytes.
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    /// The name's bytes, owned.
    pub fn into_bytes(self) -> Vec<u8> {
        self.0
    }

    /// The name as text, any bytes that aren't UTF-8 replaced.
    pub fn to_string_lossy(&self) -> Cow<'_, str> {
        String::from_utf8_lossy(&self.0)
    }

    /// The backend the name is for: the text before its first `:`, if
    /// that's a backend's name ([`backend_of`]).
    pub fn backend(&self) -> Option<&str> {
        backend_of(&self.0)
    }
}

impl AsRef<[u8]> for DeviceName {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

impl From<&str> for DeviceName {
    fn from(name: &str) -> DeviceName {
        DeviceName(name.as_bytes().to_vec())
    }
}

impl From<String> for DeviceName {
    fn from(name: String) -> DeviceName {
        DeviceName(name.into_bytes())
    }
}

impl From<&[u8]> for DeviceName {
    fn from(name: &[u8]) -> DeviceName {
        DeviceName(name.to_vec())
    }
}

impl From<Vec<u8>> for DeviceName {
    fn from(name: Vec<u8>) -> DeviceName {
        DeviceName(name)
    }
}

/// The name as text, any bytes that aren't UTF-8 replaced.
impl fmt::Display for DeviceName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_string_lossy())
    }
}

impl fmt::Debug for DeviceName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "DeviceName({:?})", self.to_string_lossy())
    }
}

/// A device as `sane_get_devices` lists it.
///
/// Each string is kept as SANE's bytes, never decoded: the name, which goes
/// back to `sane_open` as it came, and the vendor, model and kind, for
/// people, empty where the backend gave none. The standard makes SANE's
/// strings Latin-1, but backends send UTF-8 too, so what the bytes mean is
/// the caller's to say ([`SaneString::to_string_lossy`] reads them as
/// UTF-8, any bytes that aren't replaced).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Device {
    /// The device's name, which [`Sane::open`](crate::Sane::open) takes.
    pub name: DeviceName,
    /// Its vendor.
    pub vendor: SaneString,
    /// Its model.
    pub model: SaneString,
    /// What kind of device it is, such as "flatbed scanner".
    pub kind: SaneString,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_keep_their_bytes() {
        let name = DeviceName::new(b"odd:\xff\x01".to_vec());
        assert_eq!(name.as_bytes(), b"odd:\xff\x01");
        assert_eq!(name.to_string_lossy(), "odd:\u{fffd}\u{1}");
        assert_eq!(name.backend(), Some("odd"));
        assert_eq!(name.clone().into_bytes(), b"odd:\xff\x01".to_vec());
        assert_eq!(DeviceName::from("test:0").to_string(), "test:0");
        assert_eq!(
            format!("{:?}", DeviceName::from("test:0")),
            "DeviceName(\"test:0\")"
        );
        assert_eq!(
            DeviceName::from("test:0"),
            DeviceName::from(b"test:0".as_slice())
        );
        assert_eq!(
            DeviceName::from(String::from("pnm:1")).backend(),
            Some("pnm")
        );
    }
}
