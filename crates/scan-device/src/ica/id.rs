//! An ImageCaptureCore device's id (`ica:` and the framework's own id for it), the refusals made
//! before the framework is reached ([`ScanError::BadId`]), and a listed device's [`DeviceInfo`].

use super::driver::{DEVICE_SCANNER, DeviceProps, LOCATION_LOCAL};
use crate::{DeviceId, DeviceInfo, Driver, ScanError};

/// The prefix of every ImageCaptureCore device's id.
const PREFIX: &str = "ica:";

/// The most bytes of a device's native id, from a caller or the framework.
pub(super) const MAX_ID_BYTES: usize = 4096;

/// The most bytes of a name or a message the backend keeps of what the framework gives.
pub(super) const MAX_TEXT_BYTES: usize = 1024;

/// `text` cut to at most `max` bytes, on a character boundary.
pub(super) fn capped(text: &str, max: usize) -> String {
    if text.len() <= max {
        return text.to_owned();
    }
    let mut end = max;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_owned()
}

/// A string the framework gave, as the glue copied its UTF-16 code units, at most `max` bytes of
/// it, cut on a character boundary: an unpaired surrogate (which an `NSString` may hold, and which
/// no UTF-8 copy of it can) becomes U+FFFD. The glue copies at most `max` units, enough for `max`
/// bytes, since each is at least a byte; a pair cut at the last of them is a U+FFFD past the cap,
/// so it's dropped with the rest.
#[cfg_attr(
    not(any(target_os = "macos", test)),
    allow(dead_code, reason = "the glue's, on macOS alone")
)]
pub(super) fn text_of_units(units: &[u16], max: usize) -> String {
    capped(&String::from_utf16_lossy(units), max)
}

/// An id the framework gave, as the glue copied its UTF-16 code units whole, or `None` where they
/// aren't valid UTF-16 (an unpaired surrogate): an id is never changed, since a changed one could
/// name another device.
#[cfg_attr(
    not(any(target_os = "macos", test)),
    allow(dead_code, reason = "the glue's, on macOS alone")
)]
pub(super) fn id_of_units(units: &[u16]) -> Option<String> {
    String::from_utf16(units).ok()
}

/// `ica:<native>`.
pub(super) fn id_of(native: &str) -> DeviceId {
    let mut full = String::with_capacity(PREFIX.len() + native.len());
    full.push_str(PREFIX);
    full.push_str(native);
    DeviceId(full)
}

/// Whether `native` is an id this backend forms: not empty, no `NUL`, at most
/// [`MAX_ID_BYTES`].
fn valid_native(native: &str) -> bool {
    !native.is_empty() && native.len() <= MAX_ID_BYTES && !native.contains('\0')
}

/// The native id in `stored`, refused as [`ScanError::BadId`] before the framework is reached
/// (and before [`ScanError::NotInstalled`] where it isn't): another driver's device, an id
/// without `ica:`, or a native id empty, holding a `NUL`, or past [`MAX_ID_BYTES`].
pub(super) fn native_id(stored: &DeviceInfo) -> Result<&str, ScanError> {
    if stored.driver != Driver::Ica {
        return Err(ScanError::BadId);
    }
    let native = stored.id.0.strip_prefix(PREFIX).ok_or(ScanError::BadId)?;
    if !valid_native(native) {
        return Err(ScanError::BadId);
    }
    Ok(native)
}

/// A device's native id: its persistent id (`persistentIDString`) where it gives one this backend
/// can form an id of, else its UUID (`UUIDString`); `None` where it gives neither, and then it
/// isn't listed.
pub(super) fn native_of(props: &DeviceProps) -> Option<&str> {
    [props.persistent_id.as_deref(), props.uuid.as_deref()]
        .into_iter()
        .flatten()
        .find(|native| valid_native(native))
}

/// Whether the device is a scanner: its type says so, and the glue found it a scanner device.
pub(super) fn is_scanner(props: &DeviceProps) -> bool {
    props.scanner_class && props.type_bits & DEVICE_SCANNER != 0
}

/// Whether the device is attached to this Mac, rather than found on the network or shared.
pub(super) fn is_local(props: &DeviceProps) -> bool {
    props.type_bits & LOCATION_LOCAL != 0
}

/// A listed device's [`DeviceInfo`], or `None` where it isn't a scanner or has no id.
///
/// Its name is the framework's (capped, its `NUL`s dropped), or its native id where it gives
/// none; its maker and model are empty, since the framework gives neither.
pub(super) fn device_info(props: &DeviceProps) -> Option<DeviceInfo> {
    if !is_scanner(props) {
        return None;
    }
    let native = native_of(props)?;
    let name = props
        .name
        .as_deref()
        .map(|name| capped(&name.replace('\0', ""), MAX_TEXT_BYTES))
        .filter(|name| !name.trim().is_empty())
        .unwrap_or_else(|| native.to_owned());
    Some(DeviceInfo::new(
        id_of(native),
        name,
        String::new(),
        String::new(),
        Driver::Ica,
    ))
}

#[cfg(test)]
mod tests {
    use super::super::driver::{DEVICE_CAMERA, LOCATION_BONJOUR};
    use super::*;

    fn stored(id: &str, driver: Driver) -> DeviceInfo {
        DeviceInfo::new(
            DeviceId(id.to_owned()),
            "Desk scanner".to_owned(),
            String::new(),
            String::new(),
            driver,
        )
    }

    fn scanner(persistent_id: Option<&str>, uuid: Option<&str>) -> DeviceProps {
        DeviceProps {
            persistent_id: persistent_id.map(str::to_owned),
            uuid: uuid.map(str::to_owned),
            name: Some("Desk scanner".to_owned()),
            type_bits: DEVICE_SCANNER | LOCATION_LOCAL,
            scanner_class: true,
        }
    }

    #[test]
    fn a_stored_id_is_refused_for_each_thing_it_gets_wrong() {
        let longest = format!("ica:{}", "a".repeat(MAX_ID_BYTES));
        let one_past = format!("ica:{}", "a".repeat(MAX_ID_BYTES + 1));
        for bad in [
            stored("ica:0x1234", Driver::Sane),
            stored("ica:0x1234", Driver::Wia),
            stored("wia:0x1234", Driver::Ica),
            stored("0x1234", Driver::Ica),
            stored("ica:", Driver::Ica),
            stored("ica:0x12\u{0}34", Driver::Ica),
            stored(&one_past, Driver::Ica),
        ] {
            assert_eq!(native_id(&bad), Err(ScanError::BadId), "{:?}", bad.id);
        }
        assert_eq!(native_id(&stored("ica:0x1234", Driver::Ica)), Ok("0x1234"));
        assert_eq!(
            native_id(&stored(&longest, Driver::Ica)).map(str::len),
            Ok(MAX_ID_BYTES)
        );
    }

    #[test]
    fn the_persistent_id_is_taken_else_the_uuid_else_the_device_is_unlisted() {
        let both = scanner(Some("0x14100000046d0001"), Some("UUID-1"));
        assert_eq!(native_of(&both), Some("0x14100000046d0001"));
        assert_eq!(
            device_info(&both).map(|info| info.id),
            Some(DeviceId("ica:0x14100000046d0001".to_owned()))
        );

        for (persistent_id, uuid) in [
            (None, Some("UUID-1")),
            (Some(""), Some("UUID-1")),
            (Some("bad\u{0}id"), Some("UUID-1")),
        ] {
            let props = scanner(persistent_id, uuid);
            assert_eq!(native_of(&props), Some("UUID-1"), "{persistent_id:?}");
        }
        let too_long = "p".repeat(MAX_ID_BYTES + 1);
        assert_eq!(
            native_of(&scanner(Some(&too_long), Some("UUID-1"))),
            Some("UUID-1")
        );

        for (persistent_id, uuid) in [(None, None), (Some(""), Some("")), (None, Some("\u{0}"))] {
            let props = scanner(persistent_id, uuid);
            assert_eq!(native_of(&props), None);
            assert_eq!(device_info(&props), None);
        }
    }

    #[test]
    fn a_listed_device_is_a_scanner_named_as_the_framework_names_it() {
        let props = scanner(Some("0x1234"), None);
        let info = device_info(&props).expect("listed");
        assert_eq!(info.name, "Desk scanner");
        assert_eq!((info.maker.as_str(), info.model.as_str()), ("", ""));
        assert_eq!(info.driver, Driver::Ica);
        // The id round-trips through what a caller stores.
        assert_eq!(native_id(&info), Ok("0x1234"));

        // A camera, or an object that isn't a scanner device, isn't listed.
        let camera = DeviceProps {
            type_bits: DEVICE_CAMERA | LOCATION_LOCAL,
            ..props.clone()
        };
        assert_eq!(device_info(&camera), None);
        let not_a_scanner_device = DeviceProps {
            scanner_class: false,
            ..props.clone()
        };
        assert_eq!(device_info(&not_a_scanner_device), None);

        // No name, a blank one, or one of NULs alone: the native id is its name.
        for name in [None, Some("  "), Some("\u{0}\u{0}")] {
            let unnamed = DeviceProps {
                name: name.map(str::to_owned),
                ..props.clone()
            };
            assert_eq!(device_info(&unnamed).expect("listed").name, "0x1234");
        }

        // A long name is capped on a character boundary.
        let long = DeviceProps {
            name: Some("é".repeat(MAX_TEXT_BYTES)),
            ..props.clone()
        };
        let name = device_info(&long).expect("listed").name;
        assert_eq!(name.len(), MAX_TEXT_BYTES);
        assert!(name.chars().all(|c| c == 'é'));
    }

    #[test]
    fn a_device_found_on_the_network_is_not_local() {
        let local = scanner(Some("0x1234"), None);
        assert!(is_local(&local));
        let network = DeviceProps {
            type_bits: DEVICE_SCANNER | LOCATION_BONJOUR,
            ..local
        };
        assert!(!is_local(&network));
        assert!(is_scanner(&network));
    }

    #[test]
    fn text_is_capped_on_a_character_boundary() {
        assert_eq!(capped("short", 10), "short");
        assert_eq!(capped("abcdef", 3), "abc");
        // 'é' is two bytes: a cap inside it falls back to the boundary before.
        assert_eq!(capped("aé", 2), "a");
        assert_eq!(capped("aé", 3), "aé");
        assert_eq!(capped("", 0), "");
    }

    fn utf16(text: &str) -> Vec<u16> {
        text.encode_utf16().collect()
    }

    #[test]
    fn a_string_with_an_unpaired_surrogate_is_copied_without_utf_8_from_the_framework() {
        // A name a module cut mid-pair: its lone half becomes U+FFFD, the rest kept.
        let mut cut = utf16("Desk");
        cut.push(0xD83D);
        cut.extend(utf16(" scanner"));
        assert_eq!(text_of_units(&cut, MAX_TEXT_BYTES), "Desk\u{FFFD} scanner");
        // A lone low half too, and at either end.
        assert_eq!(
            text_of_units(&[0xDC00, 0x41, 0xD800], 16),
            "\u{FFFD}A\u{FFFD}"
        );
        assert_eq!(text_of_units(&[], 16), "");
        // A pair whole is its character.
        assert_eq!(text_of_units(&utf16("a📠"), 16), "a📠");

        // An id isn't changed: one that isn't valid UTF-16 isn't taken at all.
        assert_eq!(
            id_of_units(&utf16("0x1234-abcd")),
            Some("0x1234-abcd".to_owned())
        );
        assert_eq!(id_of_units(&cut), None);
        assert_eq!(id_of_units(&[0xDC00]), None);
    }

    #[test]
    fn as_many_units_as_the_cap_s_bytes_are_enough_for_the_cap() {
        // `max` units give at least `max` bytes, so copying no more of a long string is the same
        // as copying it whole, a pair cut at the last unit copied included.
        let max = 8;
        for text in [
            "abcdefghijk",
            "abcdefg📠xyz",
            "abcdef📠xyz",
            "ééééééé",
            "📠📠📠📠📠",
        ] {
            let units = utf16(text);
            let whole = text_of_units(&units, max);
            let first = text_of_units(&units[..units.len().min(max)], max);
            assert_eq!(first, whole, "{text}");
            assert!(whole.len() <= max);
        }
    }
}
