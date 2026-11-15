//! Live Windows COM integration tests for [`scan_device::wia::WiaBackend`] against OS `stisvc`
//! (`CLSID_WiaDevMgr2`) in both 64-bit (`x86_64-pc-windows-msvc`) and 32-bit
//! (`i686-pc-windows-msvc`).
//!
//! Direct COM vtable tests for `BoundedStream`, `TransferCallbackCom` (`IAgileObject` refusal,
//! `RPC_E_WRONG_THREAD` STA-affinity, `S_FALSE` `[out]` pointer zeroing, `STG_E_MEDIUMFULL`
//! overflow, negative `hrErrorStatus` latching, and `catch_unwind` panic containment), and
//! `PROPVARIANT` round-tripping live in `crates/scan-device/src/wia/com.rs` unit tests; this
//! integration suite exercises the public `WiaBackend` surface end-to-end against Windows COM.

#![cfg(all(windows, feature = "wia"))]

use std::thread;
use std::time::Duration;

use scan_device::wia::WiaBackend;
use scan_device::{Backend, DeviceId, DeviceInfo, Driver, ScanError};

const DEADLINE: Duration = Duration::from_secs(15);

fn make_info(id: &str, driver: Driver) -> DeviceInfo {
    DeviceInfo::new(
        DeviceId(id.to_owned()),
        "Scanner".to_owned(),
        "Maker".to_owned(),
        "Model".to_owned(),
        driver,
    )
}

#[test]
fn open_validates_bad_ids_before_touching_com() {
    let mut backend = WiaBackend::default();

    for bad in [
        make_info(
            "wia:{6BDD1FC6-810F-11D0-BEC7-08002BE2092F}\\0000",
            Driver::Fake,
        ),
        make_info(
            "wia:{6BDD1FC6-810F-11D0-BEC7-08002BE2092F}\\0000",
            Driver::Sane,
        ),
        make_info(
            "wia:{6BDD1FC6-810F-11D0-BEC7-08002BE2092F}\\0000",
            Driver::Twain { bits: 64 },
        ),
        make_info(
            "wia:{6BDD1FC6-810F-11D0-BEC7-08002BE2092F}\\0000",
            Driver::Twain { bits: 32 },
        ),
        make_info(
            "wia:{6BDD1FC6-810F-11D0-BEC7-08002BE2092F}\\0000",
            Driver::Ica,
        ),
        make_info("{6BDD1FC6-810F-11D0-BEC7-08002BE2092F}\\0000", Driver::Wia),
        make_info("sane:epjitsu:libusb:001:002", Driver::Wia),
        make_info("wia:", Driver::Wia),
        make_info("wia:{6BDD1FC6}\0trailing", Driver::Wia),
        make_info(&format!("wia:{}", "x".repeat(4097)), Driver::Wia),
    ] {
        assert_eq!(
            backend.open(&bad, Duration::ZERO).err(),
            Some(ScanError::BadId),
            "expected BadId for {:?}",
            bad.id
        );
    }
}

#[test]
fn live_wia_dev_mgr2_lists_across_threads_and_maps_missing_device_to_gone() {
    let mut backend = WiaBackend::new();
    match backend.list(DEADLINE) {
        Ok(devices) => {
            for dev in &devices {
                assert_eq!(dev.driver, Driver::Wia);
                assert!(
                    dev.id.0.starts_with("wia:") && dev.id.0.len() > 4,
                    "unexpected WIA DeviceId: {:?}",
                    dev.id
                );
                assert!(!dev.name.is_empty());
            }

            // Concurrent `WiaBackend` instances on worker threads share the process-wide lazy STA
            // session without apartment errors or proxy-teardown faults.
            let handles: Vec<_> = (0..3)
                .map(|_| {
                    thread::spawn(|| {
                        let mut b = WiaBackend::new();
                        let listed = b.list(DEADLINE)?;
                        for d in &listed {
                            assert_eq!(d.driver, Driver::Wia);
                        }
                        Ok::<usize, ScanError>(listed.len())
                    })
                })
                .collect();
            for h in handles {
                assert!(h.join().expect("worker thread panicked").is_ok());
            }

            // Opening a nonexistent STI scanner node (`0x80210015 == WIA_S_NO_DEVICE_AVAILABLE`
            // from `IWiaDevMgr2::CreateDevice`) must map to `ScanError::Gone` rather than the
            // numerically identical `WIA_ERROR_MAXIMUM_PRINTER_ENDORSER_COUNTER` transfer error.
            let missing = make_info(
                r"wia:{6BDD1FC6-810F-11D0-BEC7-08002BE2092F}\9999",
                Driver::Wia,
            );
            assert_eq!(
                backend.open(&missing, DEADLINE).err(),
                Some(ScanError::Gone)
            );
        }
        Err(ScanError::NotInstalled { what }) => {
            // Stripped Windows Server Core / container images without `stisvc` running report
            // `NotInstalled { what: "WIA" }`.
            assert_eq!(what, "WIA");
        }
        Err(other) => panic!("unexpected live WiaBackend::list error: {other:?}"),
    }
}
