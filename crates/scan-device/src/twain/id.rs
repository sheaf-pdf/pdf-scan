//! A TWAIN source's id (`twain32:<ProductName>` or `twain64:<ProductName>`), the ids refused
//! before the Data Source Manager is loaded, and matching a stored [`DeviceInfo`] against the
//! DSM's listing.

use crate::{DeviceId, DeviceInfo, Driver, ScanError};

/// The maximum raw byte length of a `TW_STR32` string (`twain.h`: 32 content bytes plus its
/// terminating `NUL` and 1 alignment byte in `[u8; 34]`).
///
/// Because `twain-dsm` decodes `TW_STR32` via `String::from_utf8_lossy`, a single non-UTF-8
/// codepage byte (such as CP1252 `0xAE` `®`) expands to `U+FFFD` (3 UTF-8 bytes). Counting 1 raw
/// byte per `U+FFFD` and `c.len_utf8()` raw bytes per valid UTF-8 scalar accepts every 32-byte
/// `TW_STR32` string (including legacy codepage bytes) while rejecting any 33+ byte string as
/// [`ScanError::BadId`].
pub(super) const MAX_TW_STR32_RAW_BYTES: usize = 32;

/// The running process's TWAIN bitness (`32` or `64`).
pub(super) const fn host_bits() -> u8 {
    if usize::BITS == 32 { 32 } else { 64 }
}

/// The prefix of a TWAIN device id for `bits` (`32` or `64`).
fn prefix_for(bits: u8) -> Option<&'static str> {
    match bits {
        32 => Some("twain32:"),
        64 => Some("twain64:"),
        _ => None,
    }
}

/// Forms the [`DeviceId`] for `product_name` in a process of `bits` (`32` or `64`).
pub(super) fn id_for_bits(bits: u8, product_name: &str) -> DeviceId {
    let prefix = prefix_for(bits).expect("TWAIN bitness is 32 or 64");
    let mut id = String::with_capacity(prefix.len() + product_name.len());
    id.push_str(prefix);
    id.push_str(product_name);
    DeviceId(id)
}

/// Extracts and validates the `ProductName` from `stored` for a TWAIN backend of `bits` (`32` or
/// `64`), before loading `TWAINDSM.dll`.
///
/// Refused with [`ScanError::BadId`]:
/// - any `Driver` other than `Driver::Twain { bits }` matching `bits`;
/// - an id lacking the matching `twain32:` or `twain64:` prefix;
/// - an empty `ProductName`, one containing a `NUL` byte, or one past
///   `MAX_TW_STR32_RAW_BYTES` (`32`) `TW_STR32` bytes.
pub(super) fn product_name_for_bits(bits: u8, stored: &DeviceInfo) -> Result<&str, ScanError> {
    let prefix = prefix_for(bits).ok_or(ScanError::BadId)?;
    if stored.driver != (Driver::Twain { bits }) {
        return Err(ScanError::BadId);
    }
    let name = stored.id.0.strip_prefix(prefix).ok_or(ScanError::BadId)?;
    if !valid_product_name(name) {
        return Err(ScanError::BadId);
    }
    Ok(name)
}

/// Extracts and validates the `ProductName` from `stored` for the running process's pointer width.
pub(super) fn product_name(stored: &DeviceInfo) -> Result<&str, ScanError> {
    product_name_for_bits(host_bits(), stored)
}

/// Whether `name` fits a valid non-empty `TW_STR32` product name (`1..=32` `TW_STR32` bytes after
/// accounting for `String::from_utf8_lossy` `U+FFFD` replacement, with no `NUL`).
pub(super) fn valid_product_name(name: &str) -> bool {
    if name.is_empty() || name.as_bytes().contains(&0) {
        return false;
    }
    let min_raw_bytes: usize = name
        .chars()
        .map(|c| if c == '\u{FFFD}' { 1 } else { c.len_utf8() })
        .sum();
    min_raw_bytes <= MAX_TW_STR32_RAW_BYTES
}

/// Builds a [`DeviceInfo`] from a listed TWAIN source's three identity strings for `bits`, or
/// `None` if the source reported an unusable `ProductName` (empty, `NUL`-containing, or past
/// `MAX_TW_STR32_RAW_BYTES`), so `list` never returns a device `open` would refuse as
/// [`ScanError::BadId`].
pub(super) fn device_info_for_bits(
    bits: u8,
    product_name: String,
    manufacturer: String,
    product_family: String,
) -> Option<DeviceInfo> {
    if prefix_for(bits).is_none() || !valid_product_name(&product_name) {
        return None;
    }
    let id = id_for_bits(bits, &product_name);
    Some(DeviceInfo::new(
        id,
        product_name,
        manufacturer,
        product_family,
        Driver::Twain { bits },
    ))
}

/// Builds a [`DeviceInfo`] from a listed `SourceId` for the running process's pointer width.
#[cfg(windows)]
pub(super) fn device_info_of(source: &twain_dsm::SourceId) -> Option<DeviceInfo> {
    device_info_for_bits(
        host_bits(),
        source.product_name(),
        source.manufacturer(),
        source.product_family(),
    )
}

/// Whether a listed source's `(ProductName, Manufacturer, ProductFamily)` matches `stored` (whose
/// `ProductName` was already validated to `expected_name`).
pub(super) fn matches_identity(
    expected_name: &str,
    stored: &DeviceInfo,
    product_name: &str,
    manufacturer: &str,
    product_family: &str,
) -> bool {
    product_name == expected_name && manufacturer == stored.maker && product_family == stored.model
}

/// Finds the listed `SourceId` matching `stored` by `(ProductName, Manufacturer, ProductFamily)`,
/// together with its canonical [`DeviceInfo`].
#[cfg(windows)]
pub(super) fn find_matching<'a>(
    expected_name: &str,
    stored: &DeviceInfo,
    sources: &'a [twain_dsm::SourceId],
) -> Option<(&'a twain_dsm::SourceId, DeviceInfo)> {
    for source in sources {
        let name = source.product_name();
        let maker = source.manufacturer();
        let family = source.product_family();
        if matches_identity(expected_name, stored, &name, &maker, &family) {
            let info = device_info_for_bits(host_bits(), name, maker, family)?;
            return Some((source, info));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stored(bits: u8, id: &str, maker: &str, model: &str) -> DeviceInfo {
        DeviceInfo::new(
            DeviceId(id.to_owned()),
            "Display".to_owned(),
            maker.to_owned(),
            model.to_owned(),
            Driver::Twain { bits },
        )
    }

    #[test]
    fn host_bitness_matches_target_pointer_width() {
        if cfg!(target_pointer_width = "32") {
            assert_eq!(host_bits(), 32);
        } else {
            assert_eq!(host_bits(), 64);
        }
    }

    #[test]
    fn valid_32_and_64_bit_ids_round_trip() {
        let info32 = device_info_for_bits(
            32,
            "PaperStream IP fi-6130dj".to_owned(),
            "FUJITSU".to_owned(),
            "PaperStream IP".to_owned(),
        )
        .expect("valid 32-bit info");
        assert_eq!(info32.id.0, "twain32:PaperStream IP fi-6130dj");
        assert_eq!(info32.name, "PaperStream IP fi-6130dj");
        assert_eq!(info32.maker, "FUJITSU");
        assert_eq!(info32.model, "PaperStream IP");
        assert_eq!(info32.driver, Driver::Twain { bits: 32 });
        assert_eq!(
            product_name_for_bits(32, &info32),
            Ok("PaperStream IP fi-6130dj")
        );

        let info64 = device_info_for_bits(
            64,
            "TWAIN2 Software Scanner".to_owned(),
            "TWAIN Working Group".to_owned(),
            "Software Scan".to_owned(),
        )
        .expect("valid 64-bit info");
        assert_eq!(info64.id.0, "twain64:TWAIN2 Software Scanner");
        assert_eq!(info64.driver, Driver::Twain { bits: 64 });
        assert_eq!(
            product_name_for_bits(64, &info64),
            Ok("TWAIN2 Software Scanner")
        );
    }

    #[test]
    fn boundary_lengths_accept_utf8_lossy_tw_str32_and_refuse_empty_or_past_cap() {
        // 32 ASCII bytes is accepted; 33 ASCII bytes is `BadId`.
        let ascii_32 = "12345678901234567890123456789012";
        assert_eq!(ascii_32.len(), MAX_TW_STR32_RAW_BYTES);
        let at_ascii_cap = stored(64, &format!("twain64:{ascii_32}"), "Maker", "Family");
        assert_eq!(product_name_for_bits(64, &at_ascii_cap), Ok(ascii_32));

        let ascii_33 = "123456789012345678901234567890123";
        assert_eq!(ascii_33.len(), MAX_TW_STR32_RAW_BYTES + 1);
        let past_ascii_cap = stored(64, &format!("twain64:{ascii_33}"), "Maker", "Family");
        assert_eq!(
            product_name_for_bits(64, &past_ascii_cap),
            Err(ScanError::BadId)
        );
        assert!(
            device_info_for_bits(64, ascii_33.to_owned(), String::new(), String::new()).is_none()
        );

        // A 31-byte CP1252 `TW_STR32` with two high bytes (`®` -> `U+FFFD`, 3 UTF-8 bytes each)
        // expands to 35 UTF-8 bytes while representing 31 raw `TW_STR32` bytes (`<= 32`), and must
        // be accepted so real scanners with trademark/localized bytes list and open cleanly.
        let with_two = "Canon DR-C225 II\u{FFFD}\u{FFFD} TWAIN Driver";
        assert_eq!(with_two.len(), 35);
        assert!(
            device_info_for_bits(64, with_two.to_owned(), "CANON".into(), "DR".into()).is_some()
        );

        let exact_lossy_cap = "\u{FFFD}".repeat(MAX_TW_STR32_RAW_BYTES);
        assert_eq!(exact_lossy_cap.len(), MAX_TW_STR32_RAW_BYTES * 3);
        let at_cap = stored(64, &format!("twain64:{exact_lossy_cap}"), "Maker", "Family");
        assert_eq!(
            product_name_for_bits(64, &at_cap),
            Ok(exact_lossy_cap.as_str())
        );

        let too_long = format!("{exact_lossy_cap}x");
        let past_cap = stored(64, &format!("twain64:{too_long}"), "Maker", "Family");
        assert_eq!(product_name_for_bits(64, &past_cap), Err(ScanError::BadId));
        assert!(device_info_for_bits(64, too_long, String::new(), String::new()).is_none());

        let empty = stored(64, "twain64:", "Maker", "Family");
        assert_eq!(product_name_for_bits(64, &empty), Err(ScanError::BadId));
        assert!(device_info_for_bits(64, String::new(), String::new(), String::new()).is_none());
    }

    #[test]
    fn wrong_prefix_wrong_bitness_other_driver_and_nul_are_bad_id() {
        // 32-bit device handed to 64-bit backend and vice versa.
        let d32 = stored(32, "twain32:Scanner", "Maker", "Family");
        let d64 = stored(64, "twain64:Scanner", "Maker", "Family");
        assert_eq!(product_name_for_bits(64, &d32), Err(ScanError::BadId));
        assert_eq!(product_name_for_bits(32, &d64), Err(ScanError::BadId));

        // Prefix and Driver::Twain { bits } disagreeing with each other.
        let mismatch1 = stored(64, "twain32:Scanner", "Maker", "Family");
        let mismatch2 = stored(32, "twain64:Scanner", "Maker", "Family");
        assert_eq!(product_name_for_bits(64, &mismatch1), Err(ScanError::BadId));
        assert_eq!(product_name_for_bits(32, &mismatch2), Err(ScanError::BadId));

        // Invalid bitness, non-TWAIN driver, wrong case, missing colon, interior NUL.
        let bad_bits = stored(16, "twain16:Scanner", "Maker", "Family");
        assert_eq!(product_name_for_bits(16, &bad_bits), Err(ScanError::BadId));
        assert_eq!(product_name_for_bits(64, &bad_bits), Err(ScanError::BadId));

        let mut sane = stored(64, "twain64:Scanner", "Maker", "Family");
        sane.driver = Driver::Sane;
        assert_eq!(product_name_for_bits(64, &sane), Err(ScanError::BadId));

        for raw in [
            "sane:Scanner",
            "wia:Scanner",
            "TWAIN64:Scanner",
            "twain64",
            "twain64:Scan\0ner",
            "twain64:\0",
        ] {
            let bad = stored(64, raw, "Maker", "Family");
            assert_eq!(
                product_name_for_bits(64, &bad),
                Err(ScanError::BadId),
                "{raw:?}"
            );
        }
    }

    #[test]
    fn identity_matching_checks_product_name_manufacturer_and_family() {
        let s = stored(64, "twain64:DeskScan", "Acme", "ProSeries");
        let name = product_name_for_bits(64, &s).unwrap();
        assert!(matches_identity(name, &s, "DeskScan", "Acme", "ProSeries"));
        assert!(!matches_identity(
            name,
            &s,
            "OtherScan",
            "Acme",
            "ProSeries"
        ));
        assert!(!matches_identity(
            name,
            &s,
            "DeskScan",
            "OtherMaker",
            "ProSeries"
        ));
        assert!(!matches_identity(
            name,
            &s,
            "DeskScan",
            "Acme",
            "OtherFamily"
        ));
    }
}
