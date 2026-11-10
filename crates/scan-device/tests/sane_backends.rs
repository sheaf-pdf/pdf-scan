//! The `sane` backend's devices through SANE's own `test` and `pnm` backends: listed, opened and
//! what each can do, an option set by name, a device opened once at a time; and the
//! configuration of a worker that opens a stored device, a directory naming its backend then one
//! standing for the system's, never the system's own: aliases opened through it, a stale index
//! gone, and an alias whose target is gone listed after it's opened. Each test runs in a child
//! process of its own; where there's no libsane, each says it skipped, or fails where
//! `SANE_DL_REQUIRE_LIBSANE=1`.

#[macro_use]
mod sane_support;

use std::fs;
use std::time::Duration;

use sane_support::{installed, real_child, stand_in_child, test_dir};
use scan_device::sane::SaneBackend;
use scan_device::{
    Backend, DeviceId, DeviceInfo, Driver, Format, Resolutions, ScanError, SizeMm, Source,
};

/// A deadline no listing of these backends comes near.
const SOON: Duration = Duration::from_secs(30);

/// The `DeviceInfo` an app stored for `id`, of `maker` and `model`: its name the id's after
/// `sane:`, as a listing gives it.
fn stored(id: &str, maker: &str, model: &str) -> DeviceInfo {
    DeviceInfo::new(
        DeviceId(id.to_owned()),
        id.strip_prefix("sane:").unwrap_or(id).to_owned(),
        maker.to_owned(),
        model.to_owned(),
        Driver::Sane,
    )
}

/// A `test` backend's device, as its listing gives it.
fn tester(id: &str) -> DeviceInfo {
    stored(id, "Noname", "frontend-tester")
}

/// Each listed device's id.
fn ids(listed: &[DeviceInfo]) -> Vec<&str> {
    listed.iter().map(|device| device.id.0.as_str()).collect()
}

#[test]
fn the_test_backend_is_listed_opened_and_read() {
    let child = real_child("the_test_backend_is_listed_opened_and_read", &["test"], &[]);
    in_child!(the_test_backend_is_listed_opened_and_read, child, || {
        let mut backend = SaneBackend::new();
        let listed = installed(backend.list(SOON)).unwrap();
        assert_eq!(listed, [tester("sane:test:0"), tester("sane:test:1")]);
        let mut opened = backend.open(&listed[0], SOON).unwrap();
        assert_eq!(opened.info, listed[0]);
        assert_eq!(opened.renamed_from, None);
        let caps = opened.device.caps().unwrap();
        assert!(!caps.settings_window);
        // Gray at 1, 8 and 16 bits, the first black and white; color at 8 and 16. A FIXED range of
        // 1 to 1200 dpi. Its scan area, 200 by 200 mm.
        let formats = [
            Format::Bw1,
            Format::Gray8,
            Format::Gray16,
            Format::Rgb8,
            Format::Rgb16,
        ];
        let sources: Vec<_> = caps
            .sources
            .iter()
            .map(|source| {
                (
                    source.source,
                    source.duplex,
                    source.formats.as_slice(),
                    &source.resolutions,
                    source.max_area,
                    source.paper_auto,
                    source.encodings.is_empty(),
                )
            })
            .collect();
        let resolutions = Resolutions::Range {
            min: 1,
            max: 1200,
            step: std::num::NonZeroU32::new(1).unwrap(),
        };
        let area = Some(SizeMm {
            width: 200.0,
            height: 200.0,
        });
        assert_eq!(
            sources,
            [
                (
                    Source::Flatbed,
                    false,
                    &formats[..],
                    &resolutions,
                    area,
                    false,
                    true
                ),
                (
                    Source::Feeder,
                    false,
                    &formats[..],
                    &resolutions,
                    area,
                    false,
                    true
                ),
            ]
        );
        assert_eq!(
            opened.device.settings_window(None),
            Err(ScanError::Unsupported)
        );
    });
}

/// A 13 by 3 PBM, its rows' padding bits set, as a scanner's padding might be.
const PBM: &[u8] = b"P4\n13 3\n\xaa\xbf\x55\x47\xff\xff";

#[test]
fn pnm_opens_with_its_file_set_by_name_and_one_device_at_a_time() {
    let test = "pnm_opens_with_its_file_set_by_name_and_one_device_at_a_time";
    let file = test_dir(test).join("page.pbm");
    let child = real_child(test, &["pnm"], &[]).prepare({
        let file = file.clone();
        move |_| fs::write(&file, PBM).expect("couldn't write the PBM")
    });
    in_child!(
        pnm_opens_with_its_file_set_by_name_and_one_device_at_a_time,
        child,
        || {
            let path = file.to_str().expect("the test's path is text");
            let mut backend = SaneBackend::new().with_option("filename", path);
            let listed = installed(backend.list(SOON)).unwrap();
            assert_eq!(
                listed,
                [
                    stored("sane:pnm:0", "Noname", "PNM file reader"),
                    stored("sane:pnm:1", "Noname", "PNM file reader"),
                ]
            );
            let mut opened = backend.open(&listed[0], SOON).unwrap();
            // No source, mode or area option: one flatbed, the file's format as the parameters
            // give it, its resolutions a word list, no largest area.
            let caps = opened.device.caps().unwrap();
            assert_eq!(caps.sources.len(), 1);
            let flatbed = &caps.sources[0];
            assert_eq!(flatbed.source, Source::Flatbed);
            assert!(!flatbed.duplex);
            assert_eq!(flatbed.formats, [Format::Bw1]);
            assert_eq!(
                flatbed.resolutions,
                Resolutions::List(vec![
                    75, 90, 100, 120, 135, 150, 165, 180, 195, 200, 210, 225, 240, 255, 270, 285,
                    300
                ])
            );
            assert_eq!(flatbed.max_area, None);
            // pnm opens one device at a time.
            for again in [&listed[0], &listed[1]] {
                let second = backend.open(again, SOON).map(|opened| opened.info);
                assert_eq!(second, Err(ScanError::Busy), "{}", again.id.0);
            }
            drop(opened);
            backend.open(&listed[1], SOON).unwrap();
        }
    );
}

/// The stand-in's aliases: one whose name names no backend, one whose prefix names none.
const ALIASES: &str = "alias office:scan test:0\nalias \"Desk scanner\" test:1\n";

/// The stand-in's `test.conf`: the `test` backend's own configuration, read from it.
const TEST_CONF: &str = "number_of_devices 2\n";

#[test]
fn an_alias_naming_no_backend_opens_through_the_stand_in_with_none_named() {
    let child = stand_in_child(
        "an_alias_naming_no_backend_opens_through_the_stand_in_with_none_named",
        &[],
        &[("dll.aliases", ALIASES), ("test.conf", TEST_CONF)],
    );
    in_child!(
        an_alias_naming_no_backend_opens_through_the_stand_in_with_none_named,
        child,
        || {
            let mut backend = SaneBackend::new();
            assert_eq!(installed(backend.list(SOON)).unwrap(), []);
            let desk = tester("sane:Desk scanner");
            let opened = backend.open(&desk, SOON).unwrap();
            assert_eq!(opened.info, desk);
            assert_eq!(opened.renamed_from, None);
        }
    );
}

#[test]
fn an_alias_whose_prefix_names_no_backend_opens_through_the_stand_in() {
    let child = stand_in_child(
        "an_alias_whose_prefix_names_no_backend_opens_through_the_stand_in",
        &["office"],
        &[("dll.aliases", ALIASES), ("test.conf", TEST_CONF)],
    );
    in_child!(
        an_alias_whose_prefix_names_no_backend_opens_through_the_stand_in,
        child,
        || {
            let mut backend = SaneBackend::new();
            assert_eq!(installed(backend.list(SOON)).unwrap(), []);
            let office = tester("sane:office:scan");
            let opened = backend.open(&office, SOON).unwrap();
            assert_eq!(opened.info, office);
            assert_eq!(opened.renamed_from, None);
        }
    );
}

#[test]
fn a_stale_index_of_the_named_backend_is_gone() {
    let child = stand_in_child(
        "a_stale_index_of_the_named_backend_is_gone",
        &["test"],
        &[("dll.aliases", ALIASES), ("test.conf", TEST_CONF)],
    );
    in_child!(a_stale_index_of_the_named_backend_is_gone, child, || {
        let mut backend = SaneBackend::new();
        // That backend's devices alone, as the stand-in's aliases name them.
        let listed = installed(backend.list(SOON)).unwrap();
        assert_eq!(ids(&listed), ["sane:office:scan", "sane:Desk scanner"]);
        let stale = backend.open(&tester("sane:test:9"), SOON);
        assert_eq!(stale.map(|opened| opened.info), Err(ScanError::Gone));
    });
}

#[test]
fn an_alias_naming_no_backend_whose_target_is_gone_is_gone_listed_first() {
    let child = stand_in_child(
        "an_alias_naming_no_backend_whose_target_is_gone_is_gone_listed_first",
        &[],
        &[
            ("dll.aliases", "alias \"Desk scanner\" test:9\n"),
            ("test.conf", TEST_CONF),
        ],
    );
    in_child!(
        an_alias_naming_no_backend_whose_target_is_gone_is_gone_listed_first,
        child,
        || {
            let mut backend = SaneBackend::new();
            let desk = installed(backend.open(&tester("sane:Desk scanner"), SOON));
            assert_eq!(desk.map(|opened| opened.info), Err(ScanError::Gone));
            // SANE added the alias's target's backend as it opened it, and lists it now; the open
            // listed before it opened, so its fallback saw none of it.
            let listed = backend.list(SOON).unwrap();
            assert!(ids(&listed).contains(&"sane:test:0"), "{:?}", ids(&listed));
        }
    );
}

#[test]
fn an_alias_whose_prefix_names_no_backend_and_whose_target_is_gone_is_gone_listed_first() {
    let child = stand_in_child(
        "an_alias_whose_prefix_names_no_backend_and_whose_target_is_gone_is_gone_listed_first",
        &["office"],
        &[
            ("dll.aliases", "alias office:scan test:9\n"),
            ("test.conf", TEST_CONF),
        ],
    );
    in_child!(
        an_alias_whose_prefix_names_no_backend_and_whose_target_is_gone_is_gone_listed_first,
        child,
        || {
            let mut backend = SaneBackend::new();
            let office = installed(backend.open(&tester("sane:office:scan"), SOON));
            assert_eq!(office.map(|opened| opened.info), Err(ScanError::Gone));
            let listed = backend.list(SOON).unwrap();
            assert!(ids(&listed).contains(&"sane:test:0"), "{:?}", ids(&listed));
        }
    );
}

#[test]
#[cfg(windows)]
fn on_windows_sane_isn_t_installed() {
    let not_installed = ScanError::NotInstalled {
        what: "SANE".to_owned(),
    };
    let mut backend = SaneBackend::new();
    assert_eq!(backend.list(SOON), Err(not_installed.clone()));
    // Tried again, and an open the same; a stored id no listing gives is refused first.
    assert_eq!(backend.list(SOON), Err(not_installed.clone()));
    let opened = backend.open(&tester("sane:test:0"), SOON);
    assert_eq!(opened.map(|opened| opened.info), Err(not_installed));
    let bad = backend.open(&tester("sane::0"), SOON);
    assert_eq!(bad.map(|opened| opened.info), Err(ScanError::BadId));
}
