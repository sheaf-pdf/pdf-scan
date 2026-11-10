//! Devices, deadlines and handles, against the fake libsane: what SANE's own
//! backends can't do (a slow listing, hostile lists and answers), and the
//! fake's log checked for SANE's rules each time. Each test runs in a child
//! process of its own.

#[macro_use]
mod common;

use std::thread;
use std::time::{Duration, Instant};

use common::Fake;
use sane_dl::{Call, Device, DeviceName, Error, LimitKind, LoadError, Sane, SaneString, Status};

/// Waits until a late listing has given the library back, then lists again.
fn list_once_free(sane: &Sane) -> Result<Vec<Device>, Error> {
    let until = Instant::now() + Duration::from_secs(20);
    loop {
        match sane.devices(false, Duration::from_secs(10)) {
            Err(Error::ListingPending) if Instant::now() < until => {
                thread::sleep(Duration::from_millis(20))
            }
            other => return other,
        }
    }
}

#[test]
#[cfg_attr(not(unix), ignore = "loads the fake by SANE's name: none on Windows")]
fn devices_are_copied_as_sane_lists_them() {
    // A vendor in Latin-1, as the standard has SANE's strings, and a model in
    // UTF-8, as some backends send them: each comes back as its bytes.
    let script = r#"
        device fake:0 "Fake Co" "Model One" "flatbed scanner"
        device "fake:\xff" @null @null @null
        device fake:1 "Caf\xe9 Scan" "Mod\xc3\xa8le" "scanner \x01"
    "#;
    let fake = Fake::new("devices_are_copied_as_sane_lists_them", script);
    in_child!(devices_are_copied_as_sane_lists_them, fake.child(), || {
        let sane = common::load_fake();
        let first = sane.devices(false, Duration::from_secs(10)).unwrap();
        let expected = vec![
            Device {
                name: DeviceName::from("fake:0"),
                vendor: SaneString::from("Fake Co"),
                model: SaneString::from("Model One"),
                kind: SaneString::from("flatbed scanner"),
            },
            Device {
                name: DeviceName::new(b"fake:\xff".to_vec()),
                vendor: SaneString::default(),
                model: SaneString::default(),
                kind: SaneString::default(),
            },
            Device {
                name: DeviceName::from("fake:1"),
                vendor: SaneString::new(b"Caf\xe9 Scan".to_vec()),
                model: SaneString::new("Mod\u{e8}le".as_bytes()),
                kind: SaneString::new(b"scanner \x01".to_vec()),
            },
        ];
        assert_eq!(first, expected);
        assert_eq!(first[2].vendor.as_bytes(), b"Caf\xe9 Scan");
        assert_eq!(first[2].model.as_bytes(), b"Mod\xc3\xa8le");
        // Read as UTF-8, the Latin-1 byte is replaced, and the UTF-8 kept.
        assert_eq!(first[2].vendor.to_string_lossy(), "Caf\u{fffd} Scan");
        assert_eq!(first[2].model.to_string_lossy(), "Mod\u{e8}le");
        // The fake overwrites a list once it's stale: the first copy holds.
        let second = sane.devices(true, Duration::from_secs(10)).unwrap();
        assert_eq!(second, expected);
        assert_eq!(first, expected);
        assert_eq!(first[1].name.backend(), Some("fake"));
    });
    let log = fake.log();
    log.assert_no_violations();
    assert_eq!(
        log.calls_but_options(),
        [
            "sane_init",
            "sane_get_devices",
            "sane_get_devices",
            "sane_exit"
        ]
    );
    assert_eq!(
        log.nth("call", "sane_get_devices", 0).1.rest,
        "local_only=0"
    );
    assert_eq!(
        log.nth("call", "sane_get_devices", 1).1.rest,
        "local_only=1"
    );
    assert_eq!(
        log.nth("call", "sane_exit", 0).1.thread,
        log.nth("call", "sane_init", 0).1.thread,
        "the test's own drop ends SANE, not a listing's thread"
    );
}

#[test]
#[cfg_attr(not(unix), ignore = "loads the fake by SANE's name: none on Windows")]
fn the_last_drop_after_a_listing_in_time_ends_sane_at_once() {
    const ROUNDS: usize = 500;
    let fake = Fake::new(
        "the_last_drop_after_a_listing_in_time_ends_sane_at_once",
        "device fake:0 a b c\n",
    );
    in_child!(
        the_last_drop_after_a_listing_in_time_ends_sane_at_once,
        fake.child(),
        || {
            for round in 0..ROUNDS {
                let sane = common::load_fake();
                assert_eq!(
                    sane.devices(false, Duration::from_secs(10)).unwrap().len(),
                    1
                );
                drop(sane);
                // No listing's thread is left holding the library: the drop
                // ended SANE, so the process's slot is free at once.
                match common::try_load_fake() {
                    Ok(again) => drop(again),
                    Err(error) => panic!("round {round}: a load right after the drop: {error:?}"),
                }
            }
        }
    );
    let log = fake.log();
    log.assert_no_violations();
    let calls = log.calls_but_options();
    assert_eq!(calls.len(), ROUNDS * 5);
    for round in calls.chunks(5) {
        assert_eq!(
            round,
            [
                "sane_init",
                "sane_get_devices",
                "sane_exit",
                "sane_init",
                "sane_exit"
            ]
        );
    }
    let test_thread = &log.nth("call", "sane_init", 0).1.thread;
    for line in log.lines.iter().filter(|line| line.function == "sane_exit") {
        assert_eq!(
            &line.thread, test_thread,
            "every sane_exit is the test's own drop"
        );
    }
}

#[test]
#[cfg_attr(not(unix), ignore = "loads the fake by SANE's name: none on Windows")]
fn a_slow_listing_within_its_deadline_returns_its_list() {
    let fake = Fake::new(
        "a_slow_listing_within_its_deadline_returns_its_list",
        "listing-delay 300\ndevice fake:0 a b c\n",
    );
    in_child!(
        a_slow_listing_within_its_deadline_returns_its_list,
        fake.child(),
        || {
            let sane = common::load_fake();
            let devices = sane.devices(false, Duration::from_secs(20)).unwrap();
            assert_eq!(devices.len(), 1);
        }
    );
    fake.log().assert_no_violations();
}

#[test]
#[cfg_attr(not(unix), ignore = "loads the fake by SANE's name: none on Windows")]
fn a_listing_past_its_deadline_says_so_and_holds_the_library() {
    let fake = Fake::new(
        "a_listing_past_its_deadline_says_so_and_holds_the_library",
        "listing-delay 3000 0\ndevice fake:0 a b c\n",
    );
    in_child!(
        a_listing_past_its_deadline_says_so_and_holds_the_library,
        fake.child(),
        || {
            let sane = common::load_fake();
            let start = Instant::now();
            assert!(matches!(
                sane.devices(false, Duration::from_millis(200)),
                Err(Error::DeadlineExceeded)
            ));
            assert!(
                start.elapsed() < Duration::from_millis(2500),
                "it waited {:?}",
                start.elapsed()
            );
            // Every call says the listing is pending, at once.
            let start = Instant::now();
            assert!(matches!(sane.open("fake:0"), Err(Error::ListingPending)));
            assert!(matches!(
                sane.status_message(Status::JAMMED),
                Err(Error::ListingPending)
            ));
            assert!(matches!(
                sane.devices(false, Duration::from_secs(10)),
                Err(Error::ListingPending)
            ));
            assert!(
                start.elapsed() < Duration::from_millis(1000),
                "they waited {:?}",
                start.elapsed()
            );
            // Once the listing returns, the library is free again.
            let devices = list_once_free(&sane).unwrap();
            assert_eq!(devices[0].name, DeviceName::from("fake:0"));
            sane.open("fake:0").unwrap().close();
        }
    );
    let log = fake.log();
    log.assert_no_violations();
    assert_eq!(
        log.calls_but_options(),
        [
            "sane_init",
            "sane_get_devices",
            "sane_get_devices",
            "sane_open",
            "sane_close",
            "sane_exit"
        ]
    );
}

#[test]
#[cfg_attr(not(unix), ignore = "loads the fake by SANE's name: none on Windows")]
fn waiting_for_another_call_counts_against_the_deadline() {
    let fake = Fake::new(
        "waiting_for_another_call_counts_against_the_deadline",
        "listing-delay 1500 0\n",
    );
    in_child!(
        waiting_for_another_call_counts_against_the_deadline,
        fake.child(),
        || {
            let sane = common::load_fake();
            thread::scope(|scope| {
                let first = scope.spawn(|| sane.devices(false, Duration::from_secs(20)));
                thread::sleep(Duration::from_millis(200));
                // The first listing holds the library, within its deadline.
                let start = Instant::now();
                assert!(matches!(
                    sane.devices(false, Duration::from_millis(100)),
                    Err(Error::DeadlineExceeded)
                ));
                assert!(
                    start.elapsed() < Duration::from_millis(1000),
                    "it waited {:?}",
                    start.elapsed()
                );
                assert!(first.join().unwrap().is_ok());
            });
        }
    );
    let log = fake.log();
    log.assert_no_violations();
    assert_eq!(
        log.calls_but_options(),
        ["sane_init", "sane_get_devices", "sane_exit"]
    );
    assert_eq!(
        log.nth("call", "sane_exit", 0).1.thread,
        log.nth("call", "sane_init", 0).1.thread
    );
}

#[test]
#[cfg_attr(not(unix), ignore = "loads the fake by SANE's name: none on Windows")]
fn a_handle_dropped_during_a_late_listing_is_closed_by_the_listing() {
    let fake = Fake::new(
        "a_handle_dropped_during_a_late_listing_is_closed_by_the_listing",
        "listing-delay 2000 0\ndevice fake:0 a b c\n",
    );
    in_child!(
        a_handle_dropped_during_a_late_listing_is_closed_by_the_listing,
        fake.child(),
        || {
            let sane = common::load_fake();
            let handle = sane.open("fake:0").unwrap();
            assert!(matches!(
                sane.devices(false, Duration::from_millis(100)),
                Err(Error::DeadlineExceeded)
            ));
            let start = Instant::now();
            drop(handle);
            assert!(
                start.elapsed() < Duration::from_millis(1000),
                "the drop waited {:?}",
                start.elapsed()
            );
            list_once_free(&sane).unwrap();
        }
    );
    let log = fake.log();
    log.assert_no_violations();
    assert_eq!(
        log.calls_but_options(),
        [
            "sane_init",
            "sane_open",
            "sane_get_devices",
            "sane_close",
            "sane_get_devices",
            "sane_exit"
        ]
    );
    let (listed, listing) = log.nth("return", "sane_get_devices", 0);
    let (closed, close) = log.nth("call", "sane_close", 0);
    assert!(
        closed > listed,
        "the close came before the listing returned"
    );
    assert_eq!(
        close.thread, listing.thread,
        "the listing's thread closes the handle"
    );
    let test_thread = &log.nth("call", "sane_open", 0).1.thread;
    assert_ne!(&close.thread, test_thread);
    // The second listing returned in time, so the test's own drop ends SANE.
    assert_eq!(&log.nth("call", "sane_exit", 0).1.thread, test_thread);
}

#[test]
#[cfg_attr(not(unix), ignore = "loads the fake by SANE's name: none on Windows")]
fn the_sane_dropped_during_a_late_listing_is_ended_by_the_listing() {
    let fake = Fake::new(
        "the_sane_dropped_during_a_late_listing_is_ended_by_the_listing",
        "listing-delay 2000 0\ndevice fake:0 a b c\n",
    );
    in_child!(
        the_sane_dropped_during_a_late_listing_is_ended_by_the_listing,
        fake.child(),
        || {
            let sane = common::load_fake();
            let handle = sane.open("fake:0").unwrap();
            assert!(matches!(
                sane.devices(false, Duration::from_millis(100)),
                Err(Error::DeadlineExceeded)
            ));
            let start = Instant::now();
            drop(handle);
            drop(sane);
            assert!(
                start.elapsed() < Duration::from_millis(1000),
                "the drops waited {:?}",
                start.elapsed()
            );
            // The process's slot is freed once the listing's thread has ended
            // SANE; until then a load is refused.
            let until = Instant::now() + Duration::from_secs(20);
            let again = loop {
                match common::try_load_fake() {
                    Err(LoadError::AlreadyLoaded) if Instant::now() < until => {
                        thread::sleep(Duration::from_millis(20))
                    }
                    other => break other.unwrap(),
                }
            };
            drop(again);
        }
    );
    let log = fake.log();
    log.assert_no_violations();
    assert_eq!(
        log.calls_but_options(),
        [
            "sane_init",
            "sane_open",
            "sane_get_devices",
            "sane_close",
            "sane_exit",
            "sane_init",
            "sane_exit"
        ]
    );
    let (listed, listing) = log.nth("return", "sane_get_devices", 0);
    let (closed, close) = log.nth("call", "sane_close", 0);
    let (exited, exit) = log.nth("call", "sane_exit", 0);
    assert!(
        listed < closed && closed < exited,
        "the listing returned, then the close, then the exit"
    );
    assert_eq!(close.thread, listing.thread);
    assert_eq!(
        exit.thread, listing.thread,
        "the listing's thread ends SANE"
    );
}

#[test]
#[cfg_attr(not(unix), ignore = "loads the fake by SANE's name: none on Windows")]
fn calls_from_several_threads_never_overlap() {
    let fake = Fake::new(
        "calls_from_several_threads_never_overlap",
        "listing-delay 20\ndevice fake:0 a b c\ndevice fake:1 a b c\n",
    );
    in_child!(
        calls_from_several_threads_never_overlap,
        fake.child(),
        || {
            let sane = common::load_fake();
            thread::scope(|scope| {
                for worker in 0..4 {
                    let sane = &sane;
                    scope.spawn(move || {
                        for _ in 0..10 {
                            let handle = sane.open(format!("fake:{}", worker % 2)).unwrap();
                            assert_eq!(
                                sane.devices(false, Duration::from_secs(30)).unwrap().len(),
                                2
                            );
                            assert!(sane.status_message(Status::GOOD).unwrap().is_some());
                            handle.close();
                        }
                    });
                }
            });
        }
    );
    let log = fake.log();
    log.assert_no_violations();
    assert_eq!(
        log.calls()
            .iter()
            .filter(|call| **call == "sane_get_devices")
            .count(),
        40
    );
}

#[test]
#[cfg_attr(not(unix), ignore = "loads the fake by SANE's name: none on Windows")]
fn a_null_list_is_refused() {
    let fake = Fake::new("a_null_list_is_refused", "devices-null\n");
    in_child!(a_null_list_is_refused, fake.child(), || {
        let sane = common::load_fake();
        let error = sane.devices(false, Duration::from_secs(10)).unwrap_err();
        assert!(
            matches!(&error, Error::BadDeviceList { reason } if reason.contains("null list")),
            "{error:?}"
        );
    });
    fake.log().assert_no_violations();
}

#[test]
#[cfg_attr(not(unix), ignore = "loads the fake by SANE's name: none on Windows")]
fn a_device_with_a_null_name_is_refused() {
    let fake = Fake::new(
        "a_device_with_a_null_name_is_refused",
        "device fake:0 a b c\ndevice @null a b c\n",
    );
    in_child!(a_device_with_a_null_name_is_refused, fake.child(), || {
        let sane = common::load_fake();
        let error = sane.devices(false, Duration::from_secs(10)).unwrap_err();
        assert!(
            matches!(&error, Error::BadDeviceList { reason } if reason == "device 1's name is null"),
            "{error:?}"
        );
    });
    fake.log().assert_no_violations();
}

#[test]
#[cfg_attr(not(unix), ignore = "loads the fake by SANE's name: none on Windows")]
fn a_list_past_the_cap_is_refused() {
    let fake = Fake::new("a_list_past_the_cap_is_refused", "device-repeat 1025\n");
    in_child!(a_list_past_the_cap_is_refused, fake.child(), || {
        let sane = common::load_fake();
        assert_eq!(sane.limits().devices, 1024);
        let error = sane.devices(false, Duration::from_secs(10)).unwrap_err();
        assert!(
            matches!(
                &error,
                Error::LimitExceeded {
                    limit: LimitKind::Devices,
                    value: 1025,
                    cap: 1024
                }
            ),
            "{error:?}"
        );
        let mut limits = sane.limits();
        limits.devices = 1025;
        sane.set_limits(limits);
        assert_eq!(
            sane.devices(false, Duration::from_secs(10)).unwrap().len(),
            1025
        );
    });
    fake.log().assert_no_violations();
}

#[test]
#[cfg_attr(not(unix), ignore = "loads the fake by SANE's name: none on Windows")]
fn a_string_past_the_cap_is_refused() {
    let fake = Fake::new(
        "a_string_past_the_cap_is_refused",
        "device fake:0 @long:65537 b c\n",
    );
    in_child!(a_string_past_the_cap_is_refused, fake.child(), || {
        let sane = common::load_fake();
        assert_eq!(sane.limits().string_bytes, 65536);
        let error = sane.devices(false, Duration::from_secs(10)).unwrap_err();
        assert!(
            matches!(&error, Error::BadDeviceList { reason } if reason == "device 0's vendor is longer than 65536 bytes"),
            "{error:?}"
        );
        let mut limits = sane.limits();
        limits.string_bytes = 65537;
        sane.set_limits(limits);
        let devices = sane.devices(false, Duration::from_secs(10)).unwrap();
        assert_eq!(devices[0].vendor.as_bytes().len(), 65537);
    });
    fake.log().assert_no_violations();
}

#[test]
#[cfg_attr(not(unix), ignore = "loads the fake by SANE's name: none on Windows")]
fn a_listings_status_is_kept_with_its_call() {
    let fake = Fake::new(
        "a_listings_status_is_kept_with_its_call",
        "devices IO_ERROR\n",
    );
    in_child!(
        a_listings_status_is_kept_with_its_call,
        fake.child(),
        || {
            let sane = common::load_fake();
            match sane.devices(false, Duration::from_secs(10)) {
                Err(Error::Status {
                    status,
                    call: Call::GetDevices,
                }) => assert_eq!(status, Status::IO_ERROR),
                other => panic!("expected IO_ERROR from sane_get_devices, got {other:?}"),
            }
        }
    );
    fake.log().assert_no_violations();
}

#[test]
#[cfg_attr(not(unix), ignore = "loads the fake by SANE's name: none on Windows")]
fn a_listings_status_sane_doesnt_define_is_kept() {
    let fake = Fake::new(
        "a_listings_status_sane_doesnt_define_is_kept",
        "devices 42\n",
    );
    in_child!(
        a_listings_status_sane_doesnt_define_is_kept,
        fake.child(),
        || {
            let sane = common::load_fake();
            match sane.devices(false, Duration::from_secs(10)) {
                Err(Error::Status {
                    status,
                    call: Call::GetDevices,
                }) => assert_eq!(status, Status(42)),
                other => panic!("expected status 42 from sane_get_devices, got {other:?}"),
            }
        }
    );
    fake.log().assert_no_violations();
}

#[test]
#[cfg_attr(not(unix), ignore = "loads the fake by SANE's name: none on Windows")]
fn a_device_opens_by_name_without_a_list() {
    let fake = Fake::new(
        "a_device_opens_by_name_without_a_list",
        "device fake:0 a b c\n",
    );
    in_child!(a_device_opens_by_name_without_a_list, fake.child(), || {
        let sane = common::load_fake();
        let handle = sane.open("fake:0").unwrap();
        assert_eq!(handle.name(), &DeviceName::from("fake:0"));
        handle.close();
        let again = sane.open(DeviceName::from("fake:0")).unwrap();
        drop(again);
    });
    let log = fake.log();
    log.assert_no_violations();
    assert_eq!(
        log.calls_but_options(),
        [
            "sane_init",
            "sane_open",
            "sane_close",
            "sane_open",
            "sane_close",
            "sane_exit"
        ]
    );
}

#[test]
#[cfg_attr(not(unix), ignore = "loads the fake by SANE's name: none on Windows")]
fn a_name_sane_doesnt_know_is_sane_opens_status() {
    let fake = Fake::new(
        "a_name_sane_doesnt_know_is_sane_opens_status",
        "device fake:0 a b c\nopen fake:1 -1\n",
    );
    in_child!(
        a_name_sane_doesnt_know_is_sane_opens_status,
        fake.child(),
        || {
            let sane = common::load_fake();
            match sane.open("fake:9") {
                Err(Error::Status {
                    status,
                    call: Call::Open,
                }) => assert_eq!(status, Status::INVAL),
                other => panic!("expected INVAL from sane_open, got {other:?}"),
            }
            match sane.open("fake:1") {
                Err(Error::Status {
                    status,
                    call: Call::Open,
                }) => assert_eq!(status, Status(-1)),
                other => panic!("expected -1 from sane_open, got {other:?}"),
            }
        }
    );
    let log = fake.log();
    log.assert_no_violations();
    assert_eq!(
        log.calls_but_options(),
        ["sane_init", "sane_open", "sane_open", "sane_exit"]
    );
}

#[test]
#[cfg_attr(not(unix), ignore = "loads the fake by SANE's name: none on Windows")]
fn a_good_open_with_a_null_handle_is_refused() {
    let fake = Fake::new(
        "a_good_open_with_a_null_handle_is_refused",
        "device fake:0 a b c\nopen fake:0 null-handle\n",
    );
    in_child!(
        a_good_open_with_a_null_handle_is_refused,
        fake.child(),
        || {
            let sane = common::load_fake();
            assert!(matches!(sane.open("fake:0"), Err(Error::BadHandle)));
        }
    );
    let log = fake.log();
    log.assert_no_violations();
    assert_eq!(
        log.calls_but_options(),
        ["sane_init", "sane_open", "sane_exit"]
    );
}

#[test]
#[cfg_attr(not(unix), ignore = "loads the fake by SANE's name: none on Windows")]
fn a_name_holding_a_nul_never_reaches_sane() {
    let fake = Fake::new(
        "a_name_holding_a_nul_never_reaches_sane",
        "device fake:0 a b c\n",
    );
    in_child!(
        a_name_holding_a_nul_never_reaches_sane,
        fake.child(),
        || {
            let sane = common::load_fake();
            assert!(matches!(
                sane.open(b"fake:0\0trailing"),
                Err(Error::NulInName)
            ));
        }
    );
    let log = fake.log();
    log.assert_no_violations();
    assert_eq!(log.calls_but_options(), ["sane_init", "sane_exit"]);
}

#[test]
#[cfg_attr(not(unix), ignore = "loads the fake by SANE's name: none on Windows")]
fn status_messages_are_sanes_words() {
    let fake = Fake::new("status_messages_are_sanes_words", "");
    in_child!(status_messages_are_sanes_words, fake.child(), || {
        let sane = common::load_fake();
        assert_eq!(
            sane.status_message(Status::JAMMED).unwrap().as_deref(),
            Some("Document feeder jammed")
        );
        assert_eq!(
            sane.status_message(Status(42)).unwrap().as_deref(),
            Some("Unknown SANE status code")
        );
    });
    let log = fake.log();
    log.assert_no_violations();
    assert_eq!(
        log.calls_but_options(),
        ["sane_init", "sane_strstatus", "sane_strstatus", "sane_exit"]
    );
}

#[test]
#[cfg_attr(not(unix), ignore = "loads the fake by SANE's name: none on Windows")]
fn a_null_status_message_is_none() {
    let fake = Fake::new("a_null_status_message_is_none", "strstatus-null\n");
    in_child!(a_null_status_message_is_none, fake.child(), || {
        let sane = common::load_fake();
        assert_eq!(sane.status_message(Status::JAMMED).unwrap(), None);
    });
    fake.log().assert_no_violations();
}
