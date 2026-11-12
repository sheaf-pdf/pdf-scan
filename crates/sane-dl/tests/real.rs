//! The real libsane, through SANE's own `test` and `pnm` backends, each test
//! in a child process whose configuration is a private, isolated directory
//! naming them, never the system's.
//!
//! The library is the system's (`Sane::load`), as the system loader finds
//! it: to test another libsane, put its directory first on the loader's
//! search path for the run (`LD_LIBRARY_PATH`), which every child inherits.
//! Where there's none (Windows, macOS, a Linux without it) each test says it
//! skipped, and passes; with `SANE_DL_REQUIRE_LIBSANE=1` it fails instead.

#[macro_use]
mod common;

use std::time::Duration;

use sane_dl::{Call, Device, DeviceName, Error, SaneString, Status};

const BACKENDS: &[&str] = &["test", "pnm"];

fn device(name: &str, model: &str) -> Device {
    Device {
        name: DeviceName::from(name),
        vendor: SaneString::from("Noname"),
        model: SaneString::from(model),
        kind: SaneString::from("virtual device"),
    }
}

#[test]
fn the_test_and_pnm_devices_are_listed() {
    let child = common::real_child("the_test_and_pnm_devices_are_listed", BACKENDS);
    in_child!(the_test_and_pnm_devices_are_listed, child, || {
        let sane = common::load_real();
        assert_eq!(sane.version().major, 1);
        let expected = vec![
            device("pnm:0", "PNM file reader"),
            device("pnm:1", "PNM file reader"),
            device("test:0", "frontend-tester"),
            device("test:1", "frontend-tester"),
        ];
        for local_only in [false, true] {
            let mut devices = sane.devices(local_only, Duration::from_secs(30)).unwrap();
            devices.sort_by(|a, b| a.name.cmp(&b.name));
            assert_eq!(devices, expected, "local_only {local_only}");
        }
        for device in &expected {
            assert_eq!(
                device.name.backend(),
                device.name.to_string().split(':').next()
            );
        }
    });
}

#[test]
fn each_device_opens_and_closes() {
    let child = common::real_child("each_device_opens_and_closes", BACKENDS);
    in_child!(each_device_opens_and_closes, child, || {
        let sane = common::load_real();
        for name in ["test:0", "test:1", "pnm:0", "pnm:1"] {
            let handle = sane
                .open(name)
                .unwrap_or_else(|error| panic!("{name}: {}", common::chain(&error)));
            assert_eq!(handle.name(), &DeviceName::from(name));
            handle.close();
        }
        // And by the names a listing gives, the test backend's both open at
        // once. The pnm backend of SANE 1.4.0 opens one device at a time,
        // answering a second with DEVICE_BUSY.
        let devices = sane.devices(false, Duration::from_secs(30)).unwrap();
        let handles: Vec<_> = devices
            .iter()
            .filter(|device| device.name.backend() == Some("test"))
            .map(|device| sane.open(&device.name).unwrap())
            .collect();
        assert_eq!(handles.len(), 2);
        let pnm = sane.open("pnm:0").unwrap();
        match sane.open("pnm:1") {
            Ok(_)
            | Err(Error::Status {
                status: Status::DEVICE_BUSY,
                call: Call::Open,
            }) => {}
            other => panic!("expected pnm:1 to open, or DEVICE_BUSY, got {other:?}"),
        }
        drop(pnm);
    });
}

#[test]
fn a_name_thats_not_there_is_inval_from_sane_open() {
    let child = common::real_child("a_name_thats_not_there_is_inval_from_sane_open", BACKENDS);
    in_child!(
        a_name_thats_not_there_is_inval_from_sane_open,
        child,
        || {
            let sane = common::load_real();
            // A device the backend doesn't have, and a backend not configured.
            for name in ["test:99", "fujitsu:fi-6130dj:0"] {
                match sane.open(name) {
                    Err(Error::Status {
                        status,
                        call: Call::Open,
                    }) => assert_eq!(status, Status::INVAL, "{name}"),
                    other => panic!("{name}: expected INVAL from sane_open, got {other:?}"),
                }
            }
        }
    );
}

#[test]
fn status_messages_are_libsanes() {
    let child = common::real_child("status_messages_are_libsanes", BACKENDS);
    in_child!(status_messages_are_libsanes, child, || {
        let sane = common::load_real();
        let jammed = sane
            .status_message(Status::JAMMED)
            .unwrap()
            .expect("libsane names JAMMED");
        assert!(jammed.to_lowercase().contains("jam"), "{jammed:?}");
        // A code SANE doesn't define still gets words, or none: never a
        // crash.
        let _ = sane.status_message(Status(42)).unwrap();
    });
}
