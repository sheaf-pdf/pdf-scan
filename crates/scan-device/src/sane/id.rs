//! A SANE device's id and the name it holds; the names refused before SANE sees them; SANE's
//! strings read as text; and the choice of a device renamed, the fallback's.

use sane_dl::backend_of;

use crate::{DeviceId, DeviceInfo, Driver, ScanError};

/// What a SANE device's id starts with.
const PREFIX: &str = "sane:";

/// The names SANE's `dll` reads as its testing modes, which replay a device's USB traffic from a
/// file the rest of the name gives, or record it into one: no backend lists a name so.
const TESTING_PREFIXES: [&[u8]; 3] = [b"fakeusb:", b"fakeusbdev:", b"fakeusbout:"];

/// What a USB connection's name holds after its backend's: SANE's name for a USB device,
/// `libusb:<bus>:<device>`, its numbers given again as the device is plugged in.
const USB_CONNECTION: &[u8] = b"libusb:";

/// The id of the device SANE names `name`: `sane:`, then each of the name's bytes as the
/// character of its number (Latin-1, the charset SANE's standard gives its strings), so any name
/// goes back to SANE byte for byte.
pub(super) fn id_of(name: &[u8]) -> DeviceId {
    let mut id = String::with_capacity(PREFIX.len() + 2 * name.len());
    id.push_str(PREFIX);
    id.extend(name.iter().copied().map(char::from));
    DeviceId(id)
}

/// The SANE name a stored device's id holds, as SANE's bytes; or [`ScanError::BadId`] for an id
/// no listing gives, which never reaches SANE: a device of another driver, an id not `sane:`, a
/// character past U+00FF (not Latin-1), a `NUL`, a name longer than any listing gives
/// ([`name_cap`]), the empty name, a name whose backend's part is empty (a leading `:`), and the
/// names of `dll`'s testing modes.
///
/// A backend's name alone (`fujitsu`) and an empty name after it (`fujitsu:`) are taken: `dll`
/// opens that backend's first device, within the backend the id names.
pub(super) fn name_of(device: &DeviceInfo) -> Result<Vec<u8>, ScanError> {
    if device.driver != Driver::Sane {
        return Err(ScanError::BadId);
    }
    let text = device.id.0.strip_prefix(PREFIX).ok_or(ScanError::BadId)?;
    // A character of the name is at most two bytes of the id, so an id this long holds a name past
    // the cap, refused before anything is made of it.
    if text.len() > 2 * name_cap() {
        return Err(ScanError::BadId);
    }
    let name: Vec<u8> = text
        .chars()
        .map(|character| u8::try_from(character).ok())
        .collect::<Option<_>>()
        .ok_or(ScanError::BadId)?;
    if refused(&name) {
        return Err(ScanError::BadId);
    }
    Ok(name)
}

/// The longest name a stored id may hold, in bytes: the longest string `sane-dl` takes from a
/// listing by default (its `Limits::string_bytes`). SANE's `dll` copies a name it's given to the
/// heap, and sizes a buffer on the stack by its backend's part, so a name past this never reaches
/// it.
pub(super) fn name_cap() -> usize {
    sane_dl::Limits::default().string_bytes
}

/// Whether `name` is one no listing gives, so it's refused before SANE sees it.
fn refused(name: &[u8]) -> bool {
    name.is_empty()
        || name.len() > name_cap()
        || name.contains(&0)
        || name.starts_with(b":")
        || TESTING_PREFIXES
            .iter()
            .any(|prefix| name.starts_with(prefix))
}

/// SANE's bytes as text: as UTF-8 where they are, which some backends send, else each byte the
/// character of its number (Latin-1, the charset SANE's standard gives its strings).
pub(super) fn text(bytes: &[u8]) -> String {
    match std::str::from_utf8(bytes) {
        Ok(text) => text.to_owned(),
        Err(_) => bytes.iter().copied().map(char::from).collect(),
    }
}

/// A device as a listing gave it: its name, as SANE's bytes, and its info.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Listed {
    /// Its name, which `sane_open` takes.
    pub name: Vec<u8>,
    /// Its info, as the backend gives it.
    pub info: DeviceInfo,
}

/// Whether `found` has `stored`'s maker and model, each as the listing reads it.
pub(super) fn same_device(found: &DeviceInfo, stored: &DeviceInfo) -> bool {
    found.maker == stored.maker && found.model == stored.model
}

/// The backend of a USB connection's name, `<backend>:libusb:...`; `None` for a name of any other
/// form (an index, a serial, a network address, an alias, a name without a backend).
fn usb_backend(name: &[u8]) -> Option<&str> {
    let backend = backend_of(name)?;
    name[backend.len() + 1..]
        .starts_with(USB_CONNECTION)
        .then_some(backend)
}

/// The device a stored USB connection's name was renamed to, the fallback's choice: over the
/// listing, the one device under another USB connection's name of the stored name's backend, with
/// the stored maker and model, where exactly one is. `None` for a stored name of any other form
/// (an index, a serial, a network address or an alias, none of which a replugging renumbers), and
/// where none or several are listed so, or only under another backend.
pub(super) fn fallback<'a>(
    name: &[u8],
    stored: &DeviceInfo,
    listed: &'a [Listed],
) -> Option<&'a Listed> {
    let backend = usb_backend(name)?;
    let mut found = listed.iter().filter(|device| {
        device.name != name
            && usb_backend(&device.name) == Some(backend)
            && same_device(&device.info, stored)
    });
    let one = found.next()?;
    found.next().is_none().then_some(one)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stored(id: &str) -> DeviceInfo {
        DeviceInfo::new(
            DeviceId(id.to_owned()),
            id.to_owned(),
            "Maker".to_owned(),
            "Model".to_owned(),
            Driver::Sane,
        )
    }

    fn name(id: &str) -> Result<Vec<u8>, ScanError> {
        name_of(&stored(id))
    }

    #[test]
    fn every_byte_makes_an_id_and_comes_back() {
        for byte in 1..=255_u8 {
            let bytes = [b't', b':', byte];
            let id = id_of(&bytes);
            assert_eq!(id.0.chars().count(), "sane:".len() + 3);
            assert_eq!(id.0.chars().last(), Some(char::from(byte)));
            assert_eq!(name(&id.0), Ok(bytes.to_vec()), "byte {byte:#04x}");
        }
        // A whole name, each byte its character: Latin-1, not UTF-8.
        assert_eq!(id_of(b"test:0").0, "sane:test:0");
        assert_eq!(id_of(b"caf\xe9").0, "sane:caf\u{e9}");
        assert_eq!(id_of("caf\u{e9}".as_bytes()).0, "sane:caf\u{c3}\u{a9}");
        assert_eq!(
            name("sane:caf\u{c3}\u{a9}"),
            Ok("caf\u{e9}".as_bytes().to_vec())
        );
    }

    #[test]
    fn ids_no_listing_gives_are_refused() {
        let refused = [
            "fake:test:0",
            "sane",
            "SANE:test:0",
            "sane:",
            "sane::0",
            ":",
            "sane:test:\u{100}",
            "sane:test:\u{2603}",
            "sane:te\u{0}st",
            "sane:fakeusb:replay.xml",
            "sane:fakeusbdev:replay.xml",
            "sane:fakeusbout:record.xml:test:0",
        ];
        for id in refused {
            assert_eq!(name(id), Err(ScanError::BadId), "{id:?}");
        }
        // Another driver's device, whatever its id.
        let mut fake = stored("sane:test:0");
        fake.driver = Driver::Fake;
        assert_eq!(name_of(&fake), Err(ScanError::BadId));
    }

    #[test]
    fn a_name_past_the_cap_is_refused_and_one_at_it_taken() {
        let cap = name_cap();
        assert_eq!(cap, 65_536);
        let at_cap = format!("sane:t:{}", "x".repeat(cap - 2));
        assert_eq!(name(&at_cap).map(|name| name.len()), Ok(cap));
        let past = format!("sane:t:{}", "x".repeat(cap - 1));
        assert_eq!(name(&past), Err(ScanError::BadId));
        // Latin-1 letters, two bytes each in the id, one each in the name.
        let latin = format!("sane:t:{}", "\u{e9}".repeat(cap - 2));
        assert_eq!(name(&latin).map(|name| name.len()), Ok(cap));
        let latin_past = format!("sane:t:{}", "\u{e9}".repeat(cap - 1));
        assert_eq!(name(&latin_past), Err(ScanError::BadId));
    }

    #[test]
    fn names_within_the_backend_named_are_taken() {
        // An empty name after the backend, and the backend alone: its first device.
        assert_eq!(name("sane:fujitsu:"), Ok(b"fujitsu:".to_vec()));
        assert_eq!(name("sane:fujitsu"), Ok(b"fujitsu".to_vec()));
        // Names that only start like the testing modes'.
        assert_eq!(name("sane:fakeusb"), Ok(b"fakeusb".to_vec()));
        assert_eq!(name("sane:fakeusbx:0"), Ok(b"fakeusbx:0".to_vec()));
        assert_eq!(name("sane:Desk scanner"), Ok(b"Desk scanner".to_vec()));
    }

    #[test]
    fn sane_s_strings_read_as_utf_8_where_they_are_else_latin_1() {
        assert_eq!(text(b"Noname"), "Noname");
        assert_eq!(text("Mod\u{e8}le".as_bytes()), "Mod\u{e8}le");
        assert_eq!(text(b"Caf\xe9 Scan"), "Caf\u{e9} Scan");
        assert_eq!(text(b"\xff\xfe"), "\u{ff}\u{fe}");
        assert_eq!(text(b""), "");
    }

    fn listed(name: &str, maker: &str, model: &str) -> Listed {
        Listed {
            name: name.as_bytes().to_vec(),
            info: DeviceInfo::new(
                id_of(name.as_bytes()),
                name.to_owned(),
                maker.to_owned(),
                model.to_owned(),
                Driver::Sane,
            ),
        }
    }

    fn choice<'a>(stored_name: &str, listing: &'a [Listed]) -> Option<&'a str> {
        let device = stored(&format!("sane:{stored_name}"));
        fallback(stored_name.as_bytes(), &device, listing)
            .map(|found| std::str::from_utf8(&found.name).unwrap())
    }

    #[test]
    fn the_fallback_goes_from_a_usb_connection_to_the_one_of_its_maker_and_model() {
        let listing = [
            listed("fujitsu:libusb:001:004", "Maker", "Other"),
            listed("fujitsu:libusb:001:005", "Maker", "Model"),
            listed("fujitsu:fi-6130dj:12345", "Maker", "Model"),
            listed("canon_dr:libusb:001:006", "Maker", "Model"),
        ];
        assert_eq!(
            choice("fujitsu:libusb:001:004", &listing),
            Some("fujitsu:libusb:001:005")
        );
        assert_eq!(
            choice("fujitsu:libusb:009:009", &listing),
            Some("fujitsu:libusb:001:005")
        );
        // Never across backends: canon_dr's is the one listed for it.
        assert_eq!(
            choice("canon_dr:libusb:009:009", &listing),
            Some("canon_dr:libusb:001:006")
        );
        assert_eq!(choice("epson2:libusb:001:004", &listing), None);
    }

    #[test]
    fn the_fallback_goes_from_no_other_name_and_to_none_of_two() {
        let listing = [
            listed("fujitsu:libusb:001:005", "Maker", "Model"),
            listed("epson2:net:10.0.0.3", "Maker", "Model"),
            listed("test:1", "Maker", "Model"),
        ];
        // A serial, a network address, an index, an alias, a backend alone: none.
        for name in [
            "fujitsu:fi-6130dj:12345",
            "epson2:net:10.0.0.2",
            "test:9",
            "Desk scanner",
            "office:scan",
            "fujitsu",
            "fujitsu:",
            "fujitsu:usb:001:004",
        ] {
            assert_eq!(choice(name, &listing), None, "{name}");
        }
        // To one of two of the stored maker and model: none.
        let two = [
            listed("fujitsu:libusb:001:005", "Maker", "Model"),
            listed("fujitsu:libusb:001:006", "Maker", "Model"),
        ];
        assert_eq!(choice("fujitsu:libusb:001:004", &two), None);
        // None of the stored maker and model, or nothing listed.
        let other = [listed("fujitsu:libusb:001:005", "Maker", "Other")];
        assert_eq!(choice("fujitsu:libusb:001:004", &other), None);
        assert_eq!(choice("fujitsu:libusb:001:004", &[]), None);
        // The stored name itself is never its own fallback.
        let itself = [listed("fujitsu:libusb:001:004", "Maker", "Model")];
        assert_eq!(choice("fujitsu:libusb:001:004", &itself), None);
    }
}
