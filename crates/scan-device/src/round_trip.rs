//! The `serde` form: a value of every variant of every enum the feature encodes, and every struct
//! with each of its fields set, written as JSON and read back equal.
//!
//! Inside the crate, so each enum's `match` here is exhaustive whatever its `#[non_exhaustive]`:
//! a variant added to the model fails to compile here until it's numbered, and then fails its
//! test until a value of it is listed.

use std::{collections::BTreeSet, fmt::Debug, num::NonZeroU32};

use serde::{Serialize, de::DeserializeOwned};

use crate::{
    Applied, Caps, Clamped, DeviceId, DeviceInfo, Done, Driver, EncodedKind, Encoding, Format,
    Paper, Resolutions, ScanError, Settings, Side, SideKnown, SizeMm, Source, SourceCaps, Status,
    WindowClosed, WindowOwner,
};

/// `value` written as JSON and read back, equal; the JSON, for a test to read.
fn round_trip<T>(value: &T) -> String
where
    T: Serialize + DeserializeOwned + PartialEq + Debug,
{
    let json = serde_json::to_string(value)
        .unwrap_or_else(|error| panic!("{value:?} wasn't written: {error}"));
    let back: T =
        serde_json::from_str(&json).unwrap_or_else(|error| panic!("{json} wasn't read: {error}"));
    assert_eq!(&back, value, "{json}");
    json
}

/// `values`, once they're seen to hold a value of each of the type's `variants` variants:
/// `variant` numbers each value's from 0 by an exhaustive `match`.
fn every<T>(variants: usize, variant: fn(&T) -> usize, values: Vec<T>) -> Vec<T> {
    let seen: BTreeSet<usize> = values.iter().map(variant).collect();
    let all: BTreeSet<usize> = (0..variants).collect();
    assert_eq!(
        seen, all,
        "the variants with a value, and the variants numbered"
    );
    values
}

fn drivers() -> Vec<Driver> {
    every(
        5,
        |driver| match driver {
            Driver::Sane => 0,
            Driver::Twain { .. } => 1,
            Driver::Wia => 2,
            Driver::Ica => 3,
            Driver::Fake => 4,
        },
        vec![
            Driver::Sane,
            Driver::Twain { bits: 32 },
            Driver::Twain { bits: 64 },
            Driver::Wia,
            Driver::Ica,
            Driver::Fake,
        ],
    )
}

fn sources() -> Vec<Source> {
    every(
        2,
        |source| match source {
            Source::Flatbed => 0,
            Source::Feeder => 1,
        },
        vec![Source::Flatbed, Source::Feeder],
    )
}

fn resolutions() -> Vec<Resolutions> {
    every(
        2,
        |resolutions| match resolutions {
            Resolutions::List(_) => 0,
            Resolutions::Range { .. } => 1,
        },
        vec![
            Resolutions::List(vec![75, 150, 300, 600]),
            Resolutions::List(vec![u32::MAX]),
            Resolutions::Range {
                min: 50,
                max: 600,
                step: NonZeroU32::MIN,
            },
            Resolutions::Range {
                min: 0,
                max: u32::MAX,
                step: NonZeroU32::MAX,
            },
        ],
    )
}

fn papers() -> Vec<Paper> {
    every(
        2,
        |paper| match paper {
            Paper::Auto => 0,
            Paper::Size(_) => 1,
        },
        vec![
            Paper::Auto,
            Paper::Size(SizeMm::LETTER),
            Paper::Size(SizeMm::A4),
        ],
    )
}

fn formats() -> Vec<Format> {
    every(
        5,
        |format| match format {
            Format::Bw1 => 0,
            Format::Gray8 => 1,
            Format::Gray16 => 2,
            Format::Rgb8 => 3,
            Format::Rgb16 => 4,
        },
        vec![
            Format::Bw1,
            Format::Gray8,
            Format::Gray16,
            Format::Rgb8,
            Format::Rgb16,
        ],
    )
}

fn encodings() -> Vec<Encoding> {
    every(
        5,
        |encoding| match encoding {
            Encoding::Jpeg => 0,
            Encoding::Png => 1,
            Encoding::Tiff => 2,
            Encoding::Bmp => 3,
            Encoding::CcittG4 => 4,
        },
        vec![
            Encoding::Jpeg,
            Encoding::Png,
            Encoding::Tiff,
            Encoding::Bmp,
            Encoding::CcittG4,
        ],
    )
}

fn clamped() -> Vec<Clamped> {
    let mut values = vec![
        Clamped::Duplex(true),
        Clamped::Duplex(false),
        Clamped::Dpi(1200),
        Clamped::Encodings(encodings()),
        Clamped::Encodings(Vec::new()),
    ];
    values.extend(sources().into_iter().map(Clamped::Source));
    values.extend(formats().into_iter().map(Clamped::Format));
    values.extend(papers().into_iter().map(Clamped::Paper));
    every(
        6,
        |clamped| match clamped {
            Clamped::Source(_) => 0,
            Clamped::Duplex(_) => 1,
            Clamped::Dpi(_) => 2,
            Clamped::Format(_) => 3,
            Clamped::Paper(_) => 4,
            Clamped::Encodings(_) => 5,
        },
        values,
    )
}

fn encoded_kinds() -> Vec<EncodedKind> {
    every(
        5,
        |kind| match kind {
            EncodedKind::Jpeg => 0,
            EncodedKind::Png => 1,
            EncodedKind::Tiff => 2,
            EncodedKind::Bmp => 3,
            EncodedKind::CcittG4 { .. } => 4,
        },
        vec![
            EncodedKind::Jpeg,
            EncodedKind::Png,
            EncodedKind::Tiff,
            EncodedKind::Bmp,
            EncodedKind::CcittG4 {
                width: 2550,
                height: 3300,
                black_is_1: true,
            },
            EncodedKind::CcittG4 {
                width: u32::MAX,
                height: 1,
                black_is_1: false,
            },
        ],
    )
}

fn sides() -> Vec<Side> {
    every(
        2,
        |side| match side {
            Side::Front => 0,
            Side::Back => 1,
        },
        vec![Side::Front, Side::Back],
    )
}

fn sides_known() -> Vec<SideKnown> {
    every(
        2,
        |known| match known {
            SideKnown::Reported => 0,
            SideKnown::Inferred => 1,
        },
        vec![SideKnown::Reported, SideKnown::Inferred],
    )
}

fn statuses() -> Vec<Status> {
    every(
        1,
        |status| match status {
            Status::WarmingUp => 0,
        },
        vec![Status::WarmingUp],
    )
}

fn dones() -> Vec<Done> {
    every(
        3,
        |done| match done {
            Done::FeederEmpty => 0,
            Done::OnePage => 1,
            Done::Cancelled => 2,
        },
        vec![Done::FeederEmpty, Done::OnePage, Done::Cancelled],
    )
}

fn scan_errors() -> Vec<ScanError> {
    every(
        13,
        |error| match error {
            ScanError::FeederEmpty => 0,
            ScanError::Jammed => 1,
            ScanError::DoubleFeed => 2,
            ScanError::CoverOpen => 3,
            ScanError::Busy => 4,
            ScanError::Gone => 5,
            ScanError::BadId => 6,
            ScanError::SettingsRefused => 7,
            ScanError::AccessDenied => 8,
            ScanError::TimedOut => 9,
            ScanError::Unsupported => 10,
            ScanError::NotInstalled { .. } => 11,
            ScanError::Driver { .. } => 12,
        },
        vec![
            ScanError::FeederEmpty,
            ScanError::Jammed,
            ScanError::DoubleFeed,
            ScanError::CoverOpen,
            ScanError::Busy,
            ScanError::Gone,
            ScanError::BadId,
            ScanError::SettingsRefused,
            ScanError::AccessDenied,
            ScanError::TimedOut,
            ScanError::Unsupported,
            ScanError::NotInstalled {
                what: "SANE".to_owned(),
            },
            ScanError::Driver {
                message: "Lamp failure: call \"service\"\n\u{e9}\u{1f5a8}".to_owned(),
                shown: true,
            },
            ScanError::Driver {
                message: String::new(),
                shown: false,
            },
        ],
    )
}

fn windows_closed() -> Vec<WindowClosed> {
    every(
        2,
        |closed| match closed {
            WindowClosed::Accepted => 0,
            WindowClosed::Dismissed => 1,
        },
        vec![WindowClosed::Accepted, WindowClosed::Dismissed],
    )
}

/// A source with each of its fields set, none as `SourceCaps::new` leaves it.
fn feeder() -> SourceCaps {
    SourceCaps {
        source: Source::Feeder,
        duplex: true,
        resolutions: Resolutions::Range {
            min: 50,
            max: 600,
            step: NonZeroU32::MIN,
        },
        formats: formats(),
        encodings: encodings(),
        max_area: Some(SizeMm::LEGAL),
        paper_auto: true,
    }
}

/// Settings with each field set, none as `Settings::new` leaves it.
fn settings() -> Settings {
    Settings {
        source: Source::Feeder,
        duplex: true,
        dpi: 600,
        format: Format::Rgb16,
        paper: Paper::Size(SizeMm::A4),
        encodings: encodings(),
    }
}

#[test]
fn every_variant_of_every_enum_comes_back_equal() {
    for value in drivers() {
        round_trip(&value);
    }
    for value in sources() {
        round_trip(&value);
    }
    for value in resolutions() {
        round_trip(&value);
    }
    for value in papers() {
        round_trip(&value);
    }
    for value in clamped() {
        round_trip(&value);
    }
    for value in formats() {
        round_trip(&value);
    }
    for value in encodings() {
        round_trip(&value);
    }
    for value in encoded_kinds() {
        round_trip(&value);
    }
    for value in sides() {
        round_trip(&value);
    }
    for value in sides_known() {
        round_trip(&value);
    }
    for value in statuses() {
        round_trip(&value);
    }
    for value in dones() {
        round_trip(&value);
    }
    for value in scan_errors() {
        round_trip(&value);
    }
    for value in windows_closed() {
        round_trip(&value);
    }
}

#[test]
fn every_struct_with_each_field_set_comes_back_equal() {
    for driver in drivers() {
        round_trip(&DeviceInfo {
            id: DeviceId("sane:\u{e9}pson2:net:192.168.0.2 \"desk\"\\".to_owned()),
            name: "Desk scanner".to_owned(),
            maker: "Maker".to_owned(),
            model: "Model".to_owned(),
            driver,
        });
    }
    round_trip(&DeviceId(String::new()));
    let flatbed = SourceCaps {
        source: Source::Flatbed,
        duplex: false,
        resolutions: Resolutions::List(vec![75, 150, 300, 600]),
        formats: vec![Format::Gray8],
        encodings: Vec::new(),
        max_area: None,
        paper_auto: false,
    };
    round_trip(&feeder());
    round_trip(&flatbed);
    for settings_window in [true, false] {
        round_trip(&Caps {
            sources: vec![feeder(), flatbed.clone()],
            settings_window,
        });
    }
    round_trip(&settings());
    for format in formats() {
        for source in sources() {
            round_trip(&Settings::new(source, 300, format));
        }
    }
    round_trip(&Applied {
        settings: settings(),
        clamped: clamped(),
    });
    round_trip(&Applied::new(settings(), Vec::new()));
    for size in [SizeMm::LETTER, SizeMm::LEGAL, SizeMm::A4] {
        round_trip(&size);
    }
    for owner in [0, 0x0001_0a2c, u64::MAX] {
        round_trip(&WindowOwner(owner));
    }
}

#[test]
fn sizes_come_back_bit_for_bit() {
    let edges = [
        0.0,
        -0.0,
        // The smallest subnormal, and the smallest normal.
        f32::from_bits(1),
        f32::MIN_POSITIVE,
        f32::EPSILON,
        f32::MAX,
        -f32::MAX,
        215.9,
        279.4,
        355.6,
    ];
    let mut sizes = vec![SizeMm::LETTER, SizeMm::LEGAL, SizeMm::A4];
    sizes.extend(
        edges
            .iter()
            .zip(edges.iter().rev())
            .map(|(&width, &height)| SizeMm { width, height }),
    );
    for size in sizes {
        let bits = (size.width.to_bits(), size.height.to_bits());
        let json = serde_json::to_string(&size).unwrap();
        let back: SizeMm = serde_json::from_str(&json).unwrap();
        assert_eq!(
            (back.width.to_bits(), back.height.to_bits()),
            bits,
            "{json}"
        );
        let json = serde_json::to_string(&Paper::Size(size)).unwrap();
        let Paper::Size(back) = serde_json::from_str(&json).unwrap() else {
            panic!("{json} read as another paper");
        };
        assert_eq!(
            (back.width.to_bits(), back.height.to_bits()),
            bits,
            "{json}"
        );
    }
}

#[test]
fn a_size_not_finite_is_written_null_and_refused_on_reading() {
    for not_finite in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        let size = SizeMm {
            width: not_finite,
            height: 279.4,
        };
        let json = serde_json::to_string(&size).unwrap();
        assert_eq!(json, r#"{"width":null,"height":279.4}"#);
        assert!(serde_json::from_str::<SizeMm>(&json).is_err(), "{json}");
        let json = serde_json::to_string(&Paper::Size(size)).unwrap();
        assert!(serde_json::from_str::<Paper>(&json).is_err(), "{json}");
    }
}

#[test]
fn the_form_is_serde_s_default_rust_s_names_and_externally_tagged() {
    let device = DeviceInfo::new(
        DeviceId("fake:desk".to_owned()),
        "Maker Model".to_owned(),
        "Maker".to_owned(),
        "Model".to_owned(),
        Driver::Fake,
    );
    assert_eq!(
        round_trip(&device),
        r#"{"id":"fake:desk","name":"Maker Model","maker":"Maker","model":"Model","driver":"Fake"}"#
    );
    assert_eq!(
        round_trip(&Driver::Twain { bits: 32 }),
        r#"{"Twain":{"bits":32}}"#
    );
    assert_eq!(round_trip(&Source::Feeder), r#""Feeder""#);
    assert_eq!(
        round_trip(&Resolutions::Range {
            min: 50,
            max: 600,
            step: NonZeroU32::MIN,
        }),
        r#"{"Range":{"min":50,"max":600,"step":1}}"#
    );
    assert_eq!(round_trip(&Paper::Auto), r#""Auto""#);
    assert_eq!(
        round_trip(&Paper::Size(SizeMm::LETTER)),
        r#"{"Size":{"width":215.9,"height":279.4}}"#
    );
    assert_eq!(
        round_trip(&ScanError::Driver {
            message: "Lamp failure".to_owned(),
            shown: true,
        }),
        r#"{"Driver":{"message":"Lamp failure","shown":true}}"#
    );
    assert_eq!(round_trip(&ScanError::Jammed), r#""Jammed""#);
    assert_eq!(round_trip(&WindowOwner(7)), "7");
}

#[test]
fn what_the_model_can_t_hold_is_refused_on_reading() {
    // A step of 0, which `NonZeroU32` can't hold.
    assert!(
        serde_json::from_str::<Resolutions>(r#"{"Range":{"min":50,"max":600,"step":0}}"#).is_err()
    );
    // A variant this version doesn't have.
    assert!(serde_json::from_str::<Source>(r#""Drum""#).is_err());
    // A field missing: a struct is read whole, every field there.
    assert!(
        serde_json::from_str::<DeviceInfo>(
            r#"{"id":"fake:desk","name":"Maker Model","maker":"Maker","model":"Model"}"#
        )
        .is_err()
    );
}
