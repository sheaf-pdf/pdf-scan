//! A WIA 2.0 device's id (`wia:<WIA_DIP_DEV_ID>`), pre-COM validation (`ScanError::BadId`),
//! `StiDeviceTypeScanner` filtering, WSD metadata fallback, and root-item identity verification.

use crate::{DeviceId, DeviceInfo, Driver, ScanError};

/// Prefix of every WIA device id.
const PREFIX: &str = "wia:";

/// Upper bound on raw `WIA_DIP_DEV_ID` UTF-8 length accepted from callers or listings.
pub(super) const MAX_DEV_ID_BYTES: usize = 4096;

/// `StiDeviceTypeScanner` (`Sti.h`).
const STI_DEVICE_TYPE_SCANNER: u16 = 1;

/// Raw root/enumeration properties read for a WIA device node.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct RawDeviceProps {
    pub dev_id: String,
    pub dev_type: Option<i32>,
    pub dev_name: Option<String>,
    pub vend_desc: Option<String>,
    pub dev_desc: Option<String>,
    pub wsd_manufacturer: Option<String>,
    pub wsd_model_name: Option<String>,
    pub wsd_friendly_name: Option<String>,
}

/// Forms `wia:<native_id>`.
pub(super) fn id_of(native_id: &str) -> DeviceId {
    let mut full = String::with_capacity(PREFIX.len() + native_id.len());
    full.push_str(PREFIX);
    full.push_str(native_id);
    DeviceId(full)
}

/// Whether `native_id` is a valid non-empty, NUL-free WIA device identifier within
/// [`MAX_DEV_ID_BYTES`].
pub(super) fn valid_native_id(native_id: &str) -> bool {
    !native_id.is_empty()
        && native_id.len() <= MAX_DEV_ID_BYTES
        && !native_id.as_bytes().contains(&0)
}

/// Validates `stored` before touching COM (on every OS, including off Windows before
/// `NotInstalled`), returning the native `WIA_DIP_DEV_ID` slice or [`ScanError::BadId`].
pub(super) fn native_id(stored: &DeviceInfo) -> Result<&str, ScanError> {
    if stored.driver != Driver::Wia {
        return Err(ScanError::BadId);
    }
    let raw = stored.id.0.strip_prefix(PREFIX).ok_or(ScanError::BadId)?;
    if !valid_native_id(raw) {
        return Err(ScanError::BadId);
    }
    Ok(raw)
}

/// Whether `WIA_DIP_DEV_TYPE` identifies a scanner (`StiDeviceTypeScanner == 1`).
///
/// `Sti.h` packs the major STI device type in `HIWORD(dwDeviceType)` (`GET_STIDEVICE_TYPE`) and a
/// subtype in `LOWORD`. Standard WIA scanners report `0x0001_0001`; drivers that store an
/// unshifted `1` (`0x0000_0001`) or omit `WIA_DIP_DEV_TYPE` altogether are accepted, while cameras
/// (`2`) and video devices (`3`) are filtered out.
pub(super) fn is_scanner_dev_type(dev_type: Option<i32>) -> bool {
    let Some(raw) = dev_type else {
        return true;
    };
    let u = raw.cast_unsigned();
    let hi = (u >> 16) as u16;
    let kind = if hi != 0 { hi } else { (u & 0xFFFF) as u16 };
    kind == STI_DEVICE_TYPE_SCANNER
}

fn clean_non_empty(opt: Option<&str>) -> Option<String> {
    let raw = opt?;
    let cleaned = if raw.contains('\0') {
        raw.replace('\0', "")
    } else {
        raw.to_owned()
    };
    (!cleaned.trim().is_empty()).then_some(cleaned)
}

/// Converts raw enumeration/root properties into a [`DeviceInfo`], or `None` if the node is not a
/// scanner or has an unusable `WIA_DIP_DEV_ID`.
///
/// Falls back to `WIA_WSD_MANUFACTURER` / `WIA_WSD_MODEL_NAME` / `WIA_WSD_FRIENDLY_NAME` when the
/// standard `WIA_DIP_VEND_DESC` / `WIA_DIP_DEV_DESC` / `WIA_DIP_DEV_NAME` strings are empty or
/// missing.
pub(super) fn device_info_from_props(props: &RawDeviceProps) -> Option<DeviceInfo> {
    if !is_scanner_dev_type(props.dev_type) || !valid_native_id(&props.dev_id) {
        return None;
    }
    let maker = clean_non_empty(props.vend_desc.as_deref())
        .or_else(|| clean_non_empty(props.wsd_manufacturer.as_deref()))
        .unwrap_or_default();
    let model = clean_non_empty(props.dev_desc.as_deref())
        .or_else(|| clean_non_empty(props.wsd_model_name.as_deref()))
        .unwrap_or_default();
    let name = clean_non_empty(props.dev_name.as_deref())
        .or_else(|| clean_non_empty(props.wsd_friendly_name.as_deref()))
        .or_else(|| (!model.is_empty()).then(|| model.clone()))
        .unwrap_or_else(|| props.dev_id.clone());

    Some(DeviceInfo::new(
        id_of(&props.dev_id),
        name,
        maker,
        model,
        Driver::Wia,
    ))
}

/// Whether an opened root item's [`DeviceInfo`] still represents the same hardware model as
/// `stored` (`id`, `maker`, and `model` match; friendly `name` may be updated by the user in
/// Windows). A reassigned STI instance index (`\NNNN` now pointing at another model) returns
/// `false` so `open` fails with [`ScanError::Gone`].
pub(super) fn matches_opened(stored: &DeviceInfo, opened: &DeviceInfo) -> bool {
    opened.id == stored.id && opened.maker == stored.maker && opened.model == stored.model
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stored(id: &str, maker: &str, model: &str) -> DeviceInfo {
        DeviceInfo::new(
            DeviceId(id.to_owned()),
            "Office Scanner".to_owned(),
            maker.to_owned(),
            model.to_owned(),
            Driver::Wia,
        )
    }

    #[test]
    fn valid_sti_and_pnp_ids_round_trip_and_invalid_ids_are_bad_id() {
        let sti = r"{6BDD1FC6-810F-11D0-BEC7-08002BE2092F}\0000";
        let info = stored(&format!("wia:{sti}"), "Microsoft", "WSD Scan Device");
        assert_eq!(native_id(&info), Ok(sti));
        assert_eq!(id_of(sti).0, format!("wia:{sti}"));

        let at_cap = format!("wia:{}", "x".repeat(MAX_DEV_ID_BYTES));
        let ok_cap = stored(&at_cap, "Maker", "Model");
        assert_eq!(native_id(&ok_cap).map(str::len), Ok(MAX_DEV_ID_BYTES));

        let past_cap = format!("wia:{}", "x".repeat(MAX_DEV_ID_BYTES + 1));
        assert_eq!(
            native_id(&stored(&past_cap, "Maker", "Model")),
            Err(ScanError::BadId)
        );

        for raw in [
            "",
            "wia",
            "wia:",
            "WIA:dev0",
            "sane:test:0",
            "twain64:Scanner",
            "wia:dev\0zero",
            "wia:\0",
        ] {
            assert_eq!(
                native_id(&stored(raw, "Maker", "Model")),
                Err(ScanError::BadId),
                "{raw:?}"
            );
        }

        let mut wrong_driver = stored(&format!("wia:{sti}"), "Maker", "Model");
        wrong_driver.driver = Driver::Twain { bits: 64 };
        assert_eq!(native_id(&wrong_driver), Err(ScanError::BadId));
    }

    #[test]
    fn sti_device_type_accepts_scanners_and_rejects_cameras_and_video() {
        assert!(is_scanner_dev_type(Some(0x0001_0001)));
        assert!(is_scanner_dev_type(Some(0x0001_0000)));
        assert!(is_scanner_dev_type(Some(0x0000_0001)));
        assert!(is_scanner_dev_type(None));

        assert!(!is_scanner_dev_type(Some(0)));
        assert!(!is_scanner_dev_type(Some(0x0002_0001))); // DigitalCamera
        assert!(!is_scanner_dev_type(Some(0x0000_0002)));
        assert!(!is_scanner_dev_type(Some(0x0003_0001))); // StreamingVideo
    }

    #[test]
    fn wsd_properties_fill_empty_vendor_model_and_friendly_name() {
        let props = RawDeviceProps {
            dev_id: r"{6BDD1FC6-810F-11D0-BEC7-08002BE2092F}\0001".to_owned(),
            dev_type: Some(0x0001_0001),
            dev_name: Some("   ".to_owned()),
            vend_desc: Some(String::new()),
            dev_desc: None,
            wsd_manufacturer: Some("Acme\0".to_owned()),
            wsd_model_name: Some("ScanJet Net 500".to_owned()),
            wsd_friendly_name: Some("Hallway Scanner".to_owned()),
        };
        let info = device_info_from_props(&props).expect("scanner props");
        assert_eq!(
            info.id.0,
            r"wia:{6BDD1FC6-810F-11D0-BEC7-08002BE2092F}\0001"
        );
        assert_eq!(info.name, "Hallway Scanner");
        assert_eq!(info.maker, "Acme");
        assert_eq!(info.model, "ScanJet Net 500");
        assert_eq!(info.driver, Driver::Wia);

        // Non-scanner node is skipped.
        let mut camera = props.clone();
        camera.dev_type = Some(0x0002_0001);
        assert!(device_info_from_props(&camera).is_none());

        // Empty or NUL-containing dev_id is skipped so list never yields a BadId.
        let mut bad_id = props;
        bad_id.dev_id = "dev\0bad".to_owned();
        assert!(device_info_from_props(&bad_id).is_none());
    }

    #[test]
    fn matches_opened_allows_friendly_rename_but_catches_reassigned_slot() {
        let orig = DeviceInfo::new(
            DeviceId(r"wia:{6BDD1FC6-810F-11D0-BEC7-08002BE2092F}\0000".to_owned()),
            "Old Friendly Name".to_owned(),
            "Acme".to_owned(),
            "DocScan 400".to_owned(),
            Driver::Wia,
        );
        let renamed_friendly = DeviceInfo::new(
            orig.id.clone(),
            "New Friendly Name".to_owned(),
            "Acme".to_owned(),
            "DocScan 400".to_owned(),
            Driver::Wia,
        );
        assert!(matches_opened(&orig, &renamed_friendly));

        let reassigned_model = DeviceInfo::new(
            orig.id.clone(),
            "Other Scanner".to_owned(),
            "OtherMaker".to_owned(),
            "OtherModel".to_owned(),
            Driver::Wia,
        );
        assert!(!matches_opened(&orig, &reassigned_model));
    }
}
