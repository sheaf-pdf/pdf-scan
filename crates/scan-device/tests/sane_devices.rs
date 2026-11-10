//! The `sane` backend's devices through the fake libsane: listed (ids from Latin-1, strings read
//! as UTF-8 or Latin-1) under a deadline, opened as a stored device is (the backend listed first,
//! a name now another model's taken as gone, a USB connection's renamed one found by its maker
//! and model, ids no listing gives refused before SANE sees them), the options set by name, what
//! each of the fake's backends' layouts can do, and SANE shared by the backends of a process.
//! Each test runs in a child process of its own, and the fake's log is checked after it.

#[macro_use]
mod sane_support;

use std::time::{Duration, Instant};

use sane_support::{Fake, FakeLog, assert_fake_loaded};
use scan_device::sane::SaneBackend;
use scan_device::{
    Backend, Caps, DeviceId, DeviceInfo, Driver, Format, Opened, Resolutions, ScanError, SizeMm,
    Source, SourceCaps,
};

/// A deadline no listing here comes near.
const SOON: Duration = Duration::from_secs(10);

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

/// What an open gave: the id opened and the stored id it was renamed from.
fn ids(opened: &Opened) -> (&str, Option<&str>) {
    (
        opened.info.id.0.as_str(),
        opened.renamed_from.as_ref().map(|id| id.0.as_str()),
    )
}

/// Each `sane_open`'s name, as the log shows it, in order.
fn opened_names(log: &FakeLog) -> Vec<&str> {
    log.calls_of("sane_open")
        .into_iter()
        .map(|rest| rest.strip_prefix("name=").unwrap_or(rest))
        .collect()
}

/// Each `sane_open`'s status, in order.
fn open_statuses(log: &FakeLog) -> Vec<&str> {
    log.returns_of("sane_open")
        .into_iter()
        .map(|rest| rest.split(' ').next().unwrap_or(rest))
        .collect()
}

/// A source's caps: these resolutions and formats, its duplex and largest area.
fn source(
    source: Source,
    resolutions: Resolutions,
    formats: &[Format],
    duplex: bool,
    max_area: Option<(f32, f32)>,
) -> SourceCaps {
    let mut caps = SourceCaps::new(source, resolutions, formats.to_vec());
    caps.duplex = duplex;
    caps.max_area = max_area.map(|(width, height)| SizeMm { width, height });
    caps
}

fn range(min: u32, max: u32, step: u32) -> Resolutions {
    Resolutions::Range {
        min,
        max,
        step: std::num::NonZeroU32::new(step).unwrap(),
    }
}

/// Fails unless `caps` are `expected`, each largest area within a hundredth of a millimetre:
/// fixed point's rounding isn't a difference.
fn assert_caps(caps: &Caps, expected: &[SourceCaps]) {
    assert!(!caps.settings_window);
    let rounded = |caps: &[SourceCaps]| -> Vec<SourceCaps> {
        caps.iter()
            .map(|source| {
                let mut source = source.clone();
                source.max_area = source.max_area.map(|area| SizeMm {
                    width: (area.width * 100.0).round() / 100.0,
                    height: (area.height * 100.0).round() / 100.0,
                });
                source
            })
            .collect()
    };
    assert_eq!(rounded(&caps.sources), rounded(expected));
}

/// The one device of a script, opened.
fn open_fake(backend: &mut SaneBackend) -> Opened {
    let opened = backend
        .open(&stored("sane:fake:0", "Fake", "Scanner"), SOON)
        .unwrap();
    assert_fake_loaded();
    opened
}

#[test]
#[cfg_attr(not(unix), ignore = "loads the fake by SANE's name: none on Windows")]
fn a_listing_gives_ids_from_latin_1_and_strings_as_utf_8_or_latin_1() {
    let script = r#"
        device "fake:caf\xe9" "Caf\xe9 Scan" "Mod\xc3\xa8le" "flatbed scanner"
        device "fake:\xc3\xa9t\xc3\xa9" Vendor Model "sheetfed scanner"
        device fujitsu:libusb:001:004 FUJITSU fi-6130dj "sheetfed scanner"
    "#;
    let fake = Fake::new(
        "a_listing_gives_ids_from_latin_1_and_strings_as_utf_8_or_latin_1",
        script,
    );
    let ran = in_child!(
        a_listing_gives_ids_from_latin_1_and_strings_as_utf_8_or_latin_1,
        fake.child(),
        || {
            let listed = SaneBackend::new().list(SOON).unwrap();
            assert_fake_loaded();
            let expected = [
                DeviceInfo::new(
                    DeviceId("sane:fake:caf\u{e9}".to_owned()),
                    "fake:caf\u{e9}".to_owned(),
                    "Caf\u{e9} Scan".to_owned(),
                    "Mod\u{e8}le".to_owned(),
                    Driver::Sane,
                ),
                // The id is each byte's character; the name is read as the UTF-8 it is.
                DeviceInfo::new(
                    DeviceId("sane:fake:\u{c3}\u{a9}t\u{c3}\u{a9}".to_owned()),
                    "fake:\u{e9}t\u{e9}".to_owned(),
                    "Vendor".to_owned(),
                    "Model".to_owned(),
                    Driver::Sane,
                ),
                stored("sane:fujitsu:libusb:001:004", "FUJITSU", "fi-6130dj"),
            ];
            assert_eq!(listed, expected);
        }
    );
    if ran {
        let log = fake.log();
        log.assert_no_violations();
        assert_eq!(
            log.calls_but_options(),
            ["sane_init", "sane_get_devices", "sane_exit"]
        );
        // Network backends' devices too.
        assert_eq!(log.calls_of("sane_get_devices"), ["local_only=0"]);
    }
}

#[test]
#[cfg_attr(not(unix), ignore = "loads the fake by SANE's name: none on Windows")]
fn a_listing_past_its_deadline_is_timed_out_and_so_is_every_call_while_it_holds_sane() {
    let script = "listing-delay 5000\ndevice fake:0 Fake Scanner T\n";
    let fake = Fake::new(
        "a_listing_past_its_deadline_is_timed_out_and_so_is_every_call_while_it_holds_sane",
        script,
    );
    let ran = in_child!(
        a_listing_past_its_deadline_is_timed_out_and_so_is_every_call_while_it_holds_sane,
        fake.child(),
        || {
            let started = Instant::now();
            let mut backend = SaneBackend::new();
            assert_eq!(
                backend.list(Duration::from_millis(200)),
                Err(ScanError::TimedOut)
            );
            assert_fake_loaded();
            // The listing still holds SANE, and the backend says so at once.
            assert_eq!(backend.list(SOON), Err(ScanError::TimedOut));
            assert_eq!(
                backend
                    .open(&stored("sane:fake:0", "Fake", "Scanner"), SOON)
                    .map(|opened| opened.info),
                Err(ScanError::TimedOut)
            );
            assert!(started.elapsed() < Duration::from_secs(3));
        }
    );
    if ran {
        let log = fake.log();
        log.assert_no_violations();
        assert_eq!(log.calls_of("sane_get_devices").len(), 1);
        assert!(log.calls_of("sane_open").is_empty());
    }
}

#[test]
#[cfg_attr(not(unix), ignore = "loads the fake by SANE's name: none on Windows")]
fn an_open_lists_the_backend_first_then_opens_the_name_byte_for_byte() {
    let script = r#"device "fake:caf\xe9" "Caf\xe9 Scan" Model "flatbed scanner""#;
    let fake = Fake::new(
        "an_open_lists_the_backend_first_then_opens_the_name_byte_for_byte",
        script,
    );
    let ran = in_child!(
        an_open_lists_the_backend_first_then_opens_the_name_byte_for_byte,
        fake.child(),
        || {
            let mut backend = SaneBackend::new();
            let device = stored("sane:fake:caf\u{e9}", "Caf\u{e9} Scan", "Model");
            let mut opened = backend.open(&device, SOON).unwrap();
            assert_eq!(ids(&opened), ("sane:fake:caf\u{e9}", None));
            assert_eq!(opened.info, device);
            // A device with no options: one flatbed, its format from the parameters, 72 dpi,
            // no area; and no settings window.
            let caps = opened.device.caps().unwrap();
            assert_caps(
                &caps,
                &[source(
                    Source::Flatbed,
                    Resolutions::List(vec![72]),
                    &[Format::Gray8],
                    false,
                    None,
                )],
            );
            assert_eq!(
                opened.device.settings_window(None),
                Err(ScanError::Unsupported)
            );
        }
    );
    if ran {
        let log = fake.log();
        log.assert_no_violations();
        assert_eq!(
            log.calls_but_options(),
            [
                "sane_init",
                "sane_get_devices",
                "sane_open",
                "sane_get_parameters",
                "sane_close",
                "sane_exit"
            ]
        );
        // The fake opens a name only as it listed it, byte for byte; the log shows it as text.
        assert_eq!(opened_names(&log), ["\"fake:caf\u{fffd}\""]);
        assert_eq!(open_statuses(&log), ["status=GOOD"]);
    }
}

#[test]
#[cfg_attr(not(unix), ignore = "loads the fake by SANE's name: none on Windows")]
fn a_stored_name_listed_as_another_model_is_gone_and_never_opened() {
    let script = "device fake:0 Fake Other T\n";
    let fake = Fake::new(
        "a_stored_name_listed_as_another_model_is_gone_and_never_opened",
        script,
    );
    let ran = in_child!(
        a_stored_name_listed_as_another_model_is_gone_and_never_opened,
        fake.child(),
        || {
            let opened = SaneBackend::new().open(&stored("sane:fake:0", "Fake", "Scanner"), SOON);
            assert_eq!(opened.map(|opened| opened.info), Err(ScanError::Gone));
            assert_fake_loaded();
        }
    );
    if ran {
        let log = fake.log();
        log.assert_no_violations();
        assert_eq!(log.calls_of("sane_get_devices").len(), 1);
        assert!(opened_names(&log).is_empty());
    }
}

#[test]
#[cfg_attr(not(unix), ignore = "loads the fake by SANE's name: none on Windows")]
fn a_name_not_listed_is_opened_by_name_its_info_the_stored_one() {
    let script = "device fake:0 Fake Scanner T\nopen fake:alias GOOD\n";
    let fake = Fake::new(
        "a_name_not_listed_is_opened_by_name_its_info_the_stored_one",
        script,
    );
    let ran = in_child!(
        a_name_not_listed_is_opened_by_name_its_info_the_stored_one,
        fake.child(),
        || {
            let device = stored("sane:fake:alias", "Alias Maker", "Alias Model");
            let opened = SaneBackend::new().open(&device, SOON).unwrap();
            assert_eq!(opened.info, device);
            assert_eq!(opened.renamed_from, None);
        }
    );
    if ran {
        let log = fake.log();
        log.assert_no_violations();
        assert_eq!(opened_names(&log), ["\"fake:alias\""]);
    }
}

#[test]
#[cfg_attr(not(unix), ignore = "loads the fake by SANE's name: none on Windows")]
fn a_usb_connection_renamed_is_found_by_its_maker_and_model() {
    let script = r#"
        device fujitsu:libusb:001:004 FUJITSU fi-7160 "sheetfed scanner"
        device fujitsu:libusb:001:005 FUJITSU fi-6130dj "sheetfed scanner"
    "#;
    let fake = Fake::new(
        "a_usb_connection_renamed_is_found_by_its_maker_and_model",
        script,
    );
    let ran = in_child!(
        a_usb_connection_renamed_is_found_by_its_maker_and_model,
        fake.child(),
        || {
            let mut backend = SaneBackend::new();
            let listed = backend.list(SOON).unwrap();
            // Its name now another model's: that one isn't opened.
            let renamed = stored("sane:fujitsu:libusb:001:004", "FUJITSU", "fi-6130dj");
            let opened = backend.open(&renamed, SOON).unwrap();
            assert_eq!(
                ids(&opened),
                (
                    "sane:fujitsu:libusb:001:005",
                    Some("sane:fujitsu:libusb:001:004")
                )
            );
            assert_eq!(opened.info, listed[1]);
            drop(opened);
            // Its name gone: SANE answers INVAL, and the fallback finds it.
            let gone = stored("sane:fujitsu:libusb:002:009", "FUJITSU", "fi-6130dj");
            let opened = backend.open(&gone, SOON).unwrap();
            assert_eq!(
                ids(&opened),
                (
                    "sane:fujitsu:libusb:001:005",
                    Some("sane:fujitsu:libusb:002:009")
                )
            );
        }
    );
    if ran {
        let log = fake.log();
        log.assert_no_violations();
        assert_eq!(
            opened_names(&log),
            [
                "\"fujitsu:libusb:001:005\"",
                "\"fujitsu:libusb:002:009\"",
                "\"fujitsu:libusb:001:005\""
            ]
        );
        assert_eq!(
            open_statuses(&log),
            ["status=GOOD", "status=INVAL", "status=GOOD"]
        );
    }
}

#[test]
#[cfg_attr(not(unix), ignore = "loads the fake by SANE's name: none on Windows")]
fn no_fallback_from_a_serial_a_network_name_or_another_backend_nor_to_one_of_two() {
    let script = r#"
        device fujitsu:libusb:001:005 FUJITSU fi-6130dj "sheetfed scanner"
        device fujitsu:libusb:001:006 FUJITSU fi-7160 "sheetfed scanner"
        device fujitsu:libusb:001:007 FUJITSU fi-7160 "sheetfed scanner"
        device canon_dr:libusb:002:001 Canon DR-C240 "sheetfed scanner"
        device epson2:libusb:003:001 Epson DS-310 "sheetfed scanner"
    "#;
    let fake = Fake::new(
        "no_fallback_from_a_serial_a_network_name_or_another_backend_nor_to_one_of_two",
        script,
    );
    let ran = in_child!(
        no_fallback_from_a_serial_a_network_name_or_another_backend_nor_to_one_of_two,
        fake.child(),
        || {
            let mut backend = SaneBackend::new();
            let gone = [
                // A serial, which survives a replugging, beside one USB connection of its model.
                stored("sane:fujitsu:fi-6130dj:12345", "FUJITSU", "fi-6130dj"),
                // A network address, beside one USB connection of its model.
                stored("sane:epson2:net:10.0.0.2", "Epson", "DS-310"),
                // Two of the stored maker and model.
                stored("sane:fujitsu:libusb:001:004", "FUJITSU", "fi-7160"),
                // The one of its maker and model under another backend.
                stored("sane:epson2:libusb:009:009", "Canon", "DR-C240"),
            ];
            for device in &gone {
                let opened = backend.open(device, SOON).map(|opened| opened.info);
                assert_eq!(opened, Err(ScanError::Gone), "{}", device.id.0);
            }
            // The same backend's one USB connection of the stored maker and model: found.
            let found = backend
                .open(
                    &stored("sane:epson2:libusb:009:009", "Epson", "DS-310"),
                    SOON,
                )
                .unwrap();
            assert_eq!(
                ids(&found),
                (
                    "sane:epson2:libusb:003:001",
                    Some("sane:epson2:libusb:009:009")
                )
            );
        }
    );
    if ran {
        let log = fake.log();
        log.assert_no_violations();
        assert_eq!(
            opened_names(&log),
            [
                "\"fujitsu:fi-6130dj:12345\"",
                "\"epson2:net:10.0.0.2\"",
                "\"fujitsu:libusb:001:004\"",
                "\"epson2:libusb:009:009\"",
                "\"epson2:libusb:009:009\"",
                "\"epson2:libusb:003:001\""
            ]
        );
        assert_eq!(log.calls_of("sane_get_devices").len(), 5);
    }
}

#[test]
#[cfg_attr(not(unix), ignore = "loads the fake by SANE's name: none on Windows")]
fn ids_no_listing_gives_are_bad_ids_that_never_reach_sane_open() {
    let script = r#"device test:0 Noname frontend-tester "virtual device""#;
    let fake = Fake::new(
        "ids_no_listing_gives_are_bad_ids_that_never_reach_sane_open",
        script,
    );
    let ran = in_child!(
        ids_no_listing_gives_are_bad_ids_that_never_reach_sane_open,
        fake.child(),
        || {
            let mut backend = SaneBackend::new();
            // SANE loaded, so a refusal can't be put down to its not being there.
            assert_eq!(backend.list(SOON).unwrap().len(), 1);
            assert_fake_loaded();
            let long = format!("sane:test:{}", "x".repeat(70_000));
            let mut refused: Vec<DeviceInfo> = [
                "sane:fakeusb:replay.xml",
                "sane:fakeusbdev:replay.xml",
                "sane:fakeusbout:record.xml:test:0",
                "sane::0",
                "sane:",
                "sane:test:\u{100}",
                "sane:te\u{0}st:0",
                long.as_str(),
                "fake:test:0",
                "test:0",
            ]
            .iter()
            .map(|id| stored(id, "Noname", "frontend-tester"))
            .collect();
            // Another driver's device, though its id looks like SANE's.
            let mut fake_driver = stored("sane:test:0", "Noname", "frontend-tester");
            fake_driver.driver = Driver::Fake;
            refused.push(fake_driver);
            for device in &refused {
                let opened = backend.open(device, SOON).map(|opened| opened.info);
                assert_eq!(opened, Err(ScanError::BadId), "{:.40}", device.id.0);
            }
        }
    );
    if ran {
        let log = fake.log();
        log.assert_no_violations();
        assert!(opened_names(&log).is_empty(), "{:?}", opened_names(&log));
        // Refused before the backend lists anything, too.
        assert_eq!(log.calls_of("sane_get_devices").len(), 1);
    }
}

#[test]
#[cfg_attr(not(unix), ignore = "loads the fake by SANE's name: none on Windows")]
fn a_bad_id_loads_nothing() {
    let fake = Fake::new("a_bad_id_loads_nothing", "device test:0 Noname tester T\n");
    let ran = in_child!(a_bad_id_loads_nothing, fake.child(), || {
        let opened = SaneBackend::new().open(&stored("sane::0", "Noname", "tester"), SOON);
        assert_eq!(opened.map(|opened| opened.info), Err(ScanError::BadId));
    });
    if ran {
        // No call at all: the fake writes its log at its first.
        assert!(!fake.logged());
    }
}

#[test]
#[cfg_attr(not(unix), ignore = "loads the fake by SANE's name: none on Windows")]
fn names_within_the_id_s_backend_reach_sane() {
    let fake = Fake::new(
        "names_within_the_id_s_backend_reach_sane",
        "device test:0 Noname tester T\n",
    );
    let ran = in_child!(
        names_within_the_id_s_backend_reach_sane,
        fake.child(),
        || {
            let mut backend = SaneBackend::new();
            for id in ["sane:test:", "sane:test"] {
                let opened = backend.open(&stored(id, "Noname", "tester"), SOON);
                // The fake has no device of either name, so SANE answers INVAL.
                assert_eq!(
                    opened.map(|opened| opened.info),
                    Err(ScanError::Gone),
                    "{id}"
                );
            }
        }
    );
    if ran {
        let log = fake.log();
        log.assert_no_violations();
        assert_eq!(opened_names(&log), ["\"test:\"", "\"test\""]);
    }
}

#[test]
#[cfg_attr(not(unix), ignore = "loads the fake by SANE's name: none on Windows")]
fn statuses_at_open_say_what_they_say_of_the_scanner() {
    let script = r#"
        device fake:denied Fake Scanner T
        device fake:busy Fake Scanner T
        device fake:io Fake Scanner T
        device fake:nomem Fake Scanner T
        open fake:denied ACCESS_DENIED
        open fake:busy DEVICE_BUSY
        open fake:io IO_ERROR
        open fake:nomem NO_MEM
    "#;
    let fake = Fake::new("statuses_at_open_say_what_they_say_of_the_scanner", script);
    let ran = in_child!(
        statuses_at_open_say_what_they_say_of_the_scanner,
        fake.child(),
        || {
            let mut backend = SaneBackend::new();
            let mut open = |name: &str| {
                backend
                    .open(
                        &stored(&format!("sane:fake:{name}"), "Fake", "Scanner"),
                        SOON,
                    )
                    .map(|opened| opened.info)
            };
            assert_eq!(open("denied"), Err(ScanError::AccessDenied));
            assert_eq!(open("busy"), Err(ScanError::Busy));
            assert_eq!(open("io"), Err(ScanError::Gone));
            assert_eq!(
                open("nomem"),
                Err(ScanError::Driver {
                    message: "sane_open returned NO_MEM (10)".to_owned(),
                    shown: false,
                })
            );
        }
    );
    if ran {
        let log = fake.log();
        log.assert_no_violations();
        assert_eq!(
            open_statuses(&log),
            [
                "status=ACCESS_DENIED",
                "status=DEVICE_BUSY",
                "status=IO_ERROR",
                "status=NO_MEM"
            ]
        );
    }
}

/// A device with options of each type, to set by name.
const OPTIONS: &str = r#"
    device fake:0 Fake Scanner T
    option filename STRING 64 soft-select+soft-detect
    option speed INT 4 soft-select+soft-detect
    option gamma FIXED 4 soft-select+soft-detect
    option preview BOOL 4 soft-select+soft-detect
    option calibrate BUTTON 0 soft-select
    option hidden INT 4 soft-select+soft-detect+inactive
    option source STRING 16 soft-select+soft-detect
    strings Flatbed ADF
    value Flatbed
"#;

#[test]
#[cfg_attr(not(unix), ignore = "loads the fake by SANE's name: none on Windows")]
fn options_by_name_are_set_in_order_where_the_device_has_them_and_they_re_active() {
    let fake = Fake::new(
        "options_by_name_are_set_in_order_where_the_device_has_them_and_they_re_active",
        OPTIONS,
    );
    let ran = in_child!(
        options_by_name_are_set_in_order_where_the_device_has_them_and_they_re_active,
        fake.child(),
        || {
            let mut backend = SaneBackend::new()
                .with_option("filename", "page.pgm")
                .with_option("missing", "1")
                .with_option("hidden", "2")
                .with_option("speed", "3")
                .with_option("gamma", "1.5")
                .with_option("preview", "yes")
                .with_option("calibrate", "")
                .with_option("source", "ADF");
            open_fake(&mut backend);
        }
    );
    if ran {
        let log = fake.log();
        log.assert_no_violations();
        // 1.5 is 98304 in fixed point; a button is pressed with a word of 1.
        assert_eq!(
            log.sets(),
            ["\"page.pgm\"", "[3]", "[98304]", "[1]", "[1]", "\"ADF\""]
        );
    }
}

#[test]
#[cfg_attr(not(unix), ignore = "loads the fake by SANE's name: none on Windows")]
fn an_option_by_name_that_won_t_read_or_is_refused_fails_the_open_and_closes_the_device() {
    let fake = Fake::new(
        "an_option_by_name_that_won_t_read_or_is_refused_fails_the_open_and_closes_the_device",
        OPTIONS,
    );
    let ran = in_child!(
        an_option_by_name_that_won_t_read_or_is_refused_fails_the_open_and_closes_the_device,
        fake.child(),
        || {
            let device = stored("sane:fake:0", "Fake", "Scanner");
            let mut unread = SaneBackend::new().with_option("speed", "fast");
            match unread.open(&device, SOON).map(|opened| opened.info) {
                Err(ScanError::Driver {
                    message,
                    shown: false,
                }) => assert_eq!(
                    message,
                    "couldn't set the scanner's option \"speed\" to \"fast\": it takes a whole number"
                ),
                other => panic!("{other:?}"),
            }
            drop(unread);
            let mut refused = SaneBackend::new().with_option("source", "Tray");
            match refused.open(&device, SOON).map(|opened| opened.info) {
                Err(ScanError::Driver {
                    message,
                    shown: false,
                }) => assert!(
                    message
                        .starts_with("couldn't set the scanner's option \"source\" to \"Tray\": "),
                    "{message}"
                ),
                other => panic!("{other:?}"),
            }
        }
    );
    if ran {
        let log = fake.log();
        log.assert_no_violations();
        assert_eq!(
            log.calls_but_options(),
            [
                "sane_init",
                "sane_get_devices",
                "sane_open",
                "sane_close",
                "sane_exit",
                "sane_init",
                "sane_get_devices",
                "sane_open",
                "sane_close",
                "sane_exit"
            ]
        );
        // The string not in its list was offered, and refused.
        assert_eq!(log.sets(), ["\"Tray\""]);
    }
}

/// A test of a profile's caps: the device it opens with its options, what it can do, and the
/// values its source option is set to as they're read, the last the value it was found at.
macro_rules! profile_caps {
    ($test:ident, $profile:literal, $expected:expr, $sets:expr) => {
        #[test]
        #[cfg_attr(not(unix), ignore = "loads the fake by SANE's name: none on Windows")]
        fn $test() {
            let script = format!(
                "device fake:0 Fake Scanner \"sheetfed scanner\"\nprofile {}\n",
                $profile
            );
            let fake = Fake::new(stringify!($test), &script);
            let ran = in_child!($test, fake.child(), || {
                let mut opened = open_fake(&mut SaneBackend::new());
                let expected: Vec<SourceCaps> = $expected;
                assert_caps(&opened.device.caps().unwrap(), &expected);
            });
            if ran {
                let log = fake.log();
                log.assert_no_violations();
                let sets: &[&str] = &$sets;
                assert_eq!(log.sets(), sets);
            }
        }
    };
}

/// Black and white, gray and color, at 8 bits.
const THREE: [Format; 3] = [Format::Bw1, Format::Gray8, Format::Rgb8];
/// The fake's flatbed, Legal's height under Letter's width.
const GLASS: Option<(f32, f32)> = Some((215.9, 355.6));
/// The fake's feeder's page size at its largest.
const PAGE: Option<(f32, f32)> = Some((221.0, 355.6));

profile_caps!(
    fujitsu_s_sources_feeder_and_duplex_in_source,
    "fujitsu",
    vec![
        source(Source::Flatbed, range(50, 600, 1), &THREE, false, GLASS),
        source(Source::Feeder, range(50, 600, 1), &THREE, true, PAGE),
    ],
    // Found at `ADF Front`, read first; the flatbed read, then set back.
    ["\"Flatbed\"", "\"ADF Front\""]
);

profile_caps!(
    airscan_s_feeder_read_through_its_one_side,
    "airscan",
    {
        let all = Resolutions::List(vec![75, 100, 150, 200, 300, 600]);
        let formats = [Format::Gray8, Format::Rgb8];
        vec![
            source(Source::Flatbed, all.clone(), &formats, false, GLASS),
            // `ADF Duplex` offers 100 to 300 alone; the feeder's caps are `ADF`'s.
            source(Source::Feeder, all, &formats, true, PAGE),
        ]
    },
    ["\"ADF\"", "\"Flatbed\""]
);

profile_caps!(
    brother_s_aligned_feeders_and_long_mode_names,
    "brother",
    {
        let all = Resolutions::List(vec![100, 150, 200, 300, 400, 600, 1200]);
        vec![
            source(Source::Flatbed, all.clone(), &THREE, false, GLASS),
            source(Source::Feeder, all, &THREE, true, PAGE),
        ]
    },
    ["\"Automatic Document Feeder(left aligned)\"", "\"FlatBed\""]
);

profile_caps!(
    canon_dr_s_one_source_never_set_and_its_scan_mode,
    "canon-dr",
    vec![source(
        Source::Feeder,
        range(100, 600, 1),
        &THREE,
        true,
        PAGE
    )],
    []
);

profile_caps!(
    lexmark_s_scan_both_sides_active_for_the_feeder,
    "lexmark",
    {
        let all = Resolutions::List(vec![75, 150, 300, 600]);
        vec![
            source(Source::Flatbed, all.clone(), &THREE, false, GLASS),
            source(Source::Feeder, all, &THREE, true, PAGE),
        ]
    },
    ["\"Automatic Document Feeder\"", "\"Flatbed\""]
);

profile_caps!(
    epson_s_adf_mode_active_for_the_feeder,
    "epson",
    vec![
        source(Source::Flatbed, range(50, 600, 1), &THREE, false, GLASS),
        source(Source::Feeder, range(50, 600, 1), &THREE, true, PAGE),
    ],
    ["\"Automatic Document Feeder\"", "\"Flatbed\""]
);

profile_caps!(
    kodak_s_duplex_of_front_or_both,
    "kodak",
    vec![source(
        Source::Feeder,
        Resolutions::List(vec![100, 150, 200, 300, 400, 600]),
        &THREE,
        true,
        PAGE
    )],
    []
);

/// A test of a layout's caps, as `profile_caps!` has it, its options given whole.
macro_rules! layout_caps {
    ($test:ident, $options:expr, $expected:expr, $sets:expr) => {
        #[test]
        #[cfg_attr(not(unix), ignore = "loads the fake by SANE's name: none on Windows")]
        fn $test() {
            let script = format!("device fake:0 Fake Scanner T\n{}", $options);
            let fake = Fake::new(stringify!($test), &script);
            let ran = in_child!($test, fake.child(), || {
                let mut opened = open_fake(&mut SaneBackend::new());
                let expected: Result<Vec<SourceCaps>, ScanError> = $expected;
                match (opened.device.caps(), expected) {
                    (Ok(caps), Ok(expected)) => assert_caps(&caps, &expected),
                    (got, expected) => assert_eq!(got.map(|caps| caps.sources), expected),
                }
            });
            if ran {
                let log = fake.log();
                log.assert_no_violations();
                let sets: &[&str] = &$sets;
                assert_eq!(log.sets(), sets);
            }
        }
    };
}

layout_caps!(
    samsung_s_doc_source_and_modes,
    r#"
    option doc-source STRING 40 soft-select+soft-detect
    strings "Document Table" "ADF Duplex - Short-Edge Binding" "ADF Simplex" "ADF Duplex - Long-Edge Binding" "ADF Back" Auto
    value "ADF Back"
    option mode STRING 40 soft-select+soft-detect
    strings Halftone "Black and White - Line Art" Grayscale
    value Grayscale
    "#,
    {
        // No resolution option: 72 dpi; no area options: none.
        let formats = [Format::Bw1, Format::Gray8];
        Ok(vec![
            source(
                Source::Flatbed,
                Resolutions::List(vec![72]),
                &formats,
                false,
                None,
            ),
            source(
                Source::Feeder,
                Resolutions::List(vec![72]),
                &formats,
                true,
                None,
            ),
        ])
    },
    // Found at `ADF Back`, which the model leaves out: each read after a set, then set back.
    ["\"Document Table\"", "\"ADF Simplex\"", "\"ADF Back\""]
);

layout_caps!(
    a_normal_flatbed_a_centred_feeder_hp_s_duplex_and_x_resolution,
    r#"
    option source STRING 48 soft-select+soft-detect
    strings Normal "Automatic Document Feeder(center aligned)" Duplex
    value Normal
    option x-resolution FIXED 4 soft-select+soft-detect
    unit DPI
    range 0 1200 0
    value 300
    option y-resolution FIXED 4 soft-select+soft-detect
    unit DPI
    range 0 1200 0
    value 300
    "#,
    {
        // No mode: the format the parameters give, the fake's gray frame.
        let formats = [Format::Gray8];
        Ok(vec![
            source(Source::Flatbed, range(1, 1200, 1), &formats, false, None),
            source(Source::Feeder, range(1, 1200, 1), &formats, true, None),
        ])
    },
    [
        "\"Automatic Document Feeder(center aligned)\"",
        "\"Normal\""
    ]
);

layout_caps!(
    a_feeder_that_scans_both_sides_alone,
    r#"
    option source STRING 16 soft-select+soft-detect
    strings Flatbed "Duplex ADF"
    value Flatbed
    "#,
    {
        let formats = [Format::Gray8];
        let at_72 = Resolutions::List(vec![72]);
        Ok(vec![
            source(Source::Flatbed, at_72.clone(), &formats, false, None),
            source(Source::Feeder, at_72, &formats, true, None),
        ])
    },
    ["\"Duplex ADF\"", "\"Flatbed\""]
);

layout_caps!(
    gray_and_color_at_each_depth_and_a_fractional_range,
    r#"
    option mode STRING 16 soft-select+soft-detect
    strings Gray Color "24bit Color"
    value Gray
    option depth INT 4 soft-select+soft-detect
    unit BIT
    words 1 8 16
    value 8
    option resolution FIXED 4 soft-select+soft-detect
    unit DPI
    range 74.5 600.75 0.5
    value 300
    "#,
    Ok(vec![source(
        Source::Flatbed,
        range(75, 600, 1),
        &[
            Format::Bw1,
            Format::Gray8,
            Format::Gray16,
            Format::Rgb8,
            Format::Rgb16
        ],
        false,
        None
    )]),
    []
);

layout_caps!(
    scan_resolution_and_an_area_in_pixels,
    r#"
    option scan-resolution INT 4 soft-select+soft-detect
    unit DPI
    words 300 150 300 75
    value 300
    option mode STRING 16 soft-select+soft-detect
    strings Lineart
    value Lineart
    option tl-x INT 4 soft-select+soft-detect
    unit PIXEL
    range 0 2550 1
    option tl-y INT 4 soft-select+soft-detect
    unit PIXEL
    range 0 3300 1
    option br-x INT 4 soft-select+soft-detect
    unit PIXEL
    range 0 2550 1
    value 2550
    option br-y INT 4 soft-select+soft-detect
    unit PIXEL
    range 0 3300 1
    value 3300
    "#,
    // Letter, 2550 by 3300 pixels at 300 dpi.
    Ok(vec![source(
        Source::Flatbed,
        Resolutions::List(vec![75, 150, 300]),
        &[Format::Bw1],
        false,
        Some((215.9, 279.4))
    )]),
    []
);

layout_caps!(
    epson_s_adf_mode_by_its_other_name,
    r#"
    option source STRING 16 soft-select+soft-detect
    strings Flatbed ADF
    value Flatbed
    when Flatbed deactivate adf_mode
    when ADF activate adf_mode
    option adf_mode STRING 8 soft-select+soft-detect+inactive
    strings Simplex Duplex
    value Simplex
    "#,
    {
        let formats = [Format::Gray8];
        let at_72 = Resolutions::List(vec![72]);
        Ok(vec![
            source(Source::Flatbed, at_72.clone(), &formats, false, None),
            source(Source::Feeder, at_72, &formats, true, None),
        ])
    },
    ["\"ADF\"", "\"Flatbed\""]
);

layout_caps!(
    a_feeder_alone_with_a_duplex_boolean,
    r#"
    option source STRING 8 soft-select+soft-detect
    strings ADF
    value ADF
    option duplex BOOL 4 soft-select+soft-detect
    value 0
    "#,
    Ok(vec![source(
        Source::Feeder,
        Resolutions::List(vec![72]),
        &[Format::Gray8],
        true,
        None
    )]),
    []
);

layout_caps!(
    a_source_software_can_t_set_is_read_where_it_is,
    r#"
    option source STRING 16 soft-detect
    strings Flatbed ADF
    value ADF
    "#,
    Ok(vec![source(
        Source::Feeder,
        Resolutions::List(vec![72]),
        &[Format::Gray8],
        false,
        None
    )]),
    []
);

layout_caps!(
    a_source_software_can_t_set_at_its_duplex_value_is_a_feeder_with_duplex,
    r#"
    option source STRING 16 soft-detect
    strings Flatbed ADF "ADF Duplex"
    value "ADF Duplex"
    "#,
    Ok(vec![source(
        Source::Feeder,
        Resolutions::List(vec![72]),
        &[Format::Gray8],
        true,
        None
    )]),
    []
);

layout_caps!(
    a_source_software_can_t_set_at_one_side_offers_no_duplex_it_can_t_be_set_to,
    r#"
    option source STRING 16 soft-detect
    strings Flatbed ADF "ADF Duplex"
    value ADF
    "#,
    Ok(vec![source(
        Source::Feeder,
        Resolutions::List(vec![72]),
        &[Format::Gray8],
        false,
        None
    )]),
    []
);

layout_caps!(
    an_area_in_pixels_is_read_down_the_page_at_y_resolution,
    r#"
    option x-resolution INT 4 soft-select+soft-detect
    unit DPI
    words 300
    value 300
    option y-resolution INT 4 soft-select+soft-detect
    unit DPI
    words 600
    value 600
    option tl-x INT 4 soft-select+soft-detect
    unit PIXEL
    range 0 2550 1
    option tl-y INT 4 soft-select+soft-detect
    unit PIXEL
    range 0 6600 1
    option br-x INT 4 soft-select+soft-detect
    unit PIXEL
    range 0 2550 1
    value 2550
    option br-y INT 4 soft-select+soft-detect
    unit PIXEL
    range 0 6600 1
    value 6600
    "#,
    // Letter: 2550 pixels across at 300 dpi, 6600 down at 600.
    Ok(vec![source(
        Source::Flatbed,
        Resolutions::List(vec![300]),
        &[Format::Gray8],
        false,
        Some((215.9, 279.4))
    )]),
    []
);

layout_caps!(
    an_inactive_resolution_gives_way_to_an_active_x_resolution,
    r#"
    option resolution INT 4 soft-select+soft-detect+inactive
    unit DPI
    words 100 200 300
    value 100
    option x-resolution INT 4 soft-select+soft-detect
    unit DPI
    words 150 300
    value 150
    option y-resolution INT 4 soft-select+soft-detect
    unit DPI
    words 150 300
    value 150
    "#,
    Ok(vec![source(
        Source::Flatbed,
        Resolutions::List(vec![150, 300]),
        &[Format::Gray8],
        false,
        None
    )]),
    []
);

layout_caps!(
    a_resolution_option_that_s_inactive_is_none,
    r#"
    option resolution INT 4 soft-select+soft-detect+inactive
    unit DPI
    words 100 200 300
    value 100
    "#,
    Ok(vec![source(
        Source::Flatbed,
        Resolutions::List(vec![72]),
        &[Format::Gray8],
        false,
        None
    )]),
    []
);

layout_caps!(
    a_device_with_no_mode_the_model_names_is_unsupported,
    r#"
    option mode STRING 16 soft-select+soft-detect
    strings Halftone Couleur
    value Couleur
    "#,
    Err(ScanError::Unsupported),
    []
);

#[test]
#[cfg_attr(not(unix), ignore = "loads the fake by SANE's name: none on Windows")]
fn the_backends_of_a_process_share_sane_and_load_it_again_once_all_are_gone() {
    let fake = Fake::new(
        "the_backends_of_a_process_share_sane_and_load_it_again_once_all_are_gone",
        "device fake:0 Fake Scanner T\n",
    );
    let ran = in_child!(
        the_backends_of_a_process_share_sane_and_load_it_again_once_all_are_gone,
        fake.child(),
        || {
            let mut first = SaneBackend::new();
            let mut second = SaneBackend::new();
            assert_eq!(first.list(SOON).unwrap().len(), 1);
            assert_eq!(second.list(SOON).unwrap().len(), 1);
            drop(first);
            drop(second);
            assert_eq!(SaneBackend::new().list(SOON).unwrap().len(), 1);
        }
    );
    if ran {
        let log = fake.log();
        log.assert_no_violations();
        assert_eq!(
            log.calls_but_options(),
            [
                "sane_init",
                "sane_get_devices",
                "sane_get_devices",
                "sane_exit",
                "sane_init",
                "sane_get_devices",
                "sane_exit"
            ]
        );
    }
}

#[test]
#[cfg_attr(not(unix), ignore = "loads the fake by SANE's name: none on Windows")]
fn a_device_kept_after_its_backend_keeps_sane_shared() {
    let fake = Fake::new(
        "a_device_kept_after_its_backend_keeps_sane_shared",
        "device fake:0 Fake Scanner T\n",
    );
    let ran = in_child!(
        a_device_kept_after_its_backend_keeps_sane_shared,
        fake.child(),
        || {
            let mut first = SaneBackend::new();
            let mut opened = open_fake(&mut first);
            drop(first);
            // SANE is still loaded through the device: a new backend shares it.
            let mut second = SaneBackend::new();
            assert_eq!(second.list(SOON).unwrap().len(), 1);
            opened.device.caps().unwrap();
            drop(second);
            drop(opened);
        }
    );
    if ran {
        let log = fake.log();
        log.assert_no_violations();
        assert_eq!(
            log.calls_but_options(),
            [
                "sane_init",
                "sane_get_devices",
                "sane_open",
                "sane_get_devices",
                "sane_get_parameters",
                "sane_close",
                "sane_exit"
            ]
        );
    }
}

#[test]
fn the_backend_is_send() {
    fn assert_send<T: Send>() {}
    assert_send::<SaneBackend>();
    // Made without loading anything.
    let backend = SaneBackend::new().with_option("filename", "page.pgm");
    assert!(format!("{backend:?}").contains("filename"));
}
