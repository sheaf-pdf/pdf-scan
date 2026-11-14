//! End-to-end process-isolated tests for `Driver::Wia` over real `scan-worker` child processes:
//! cross-platform scripted `WiaBackend` scenarios (`wia-fake`, exercising `scan-worker` protocol
//! validation, noisy driver `stdout` isolation, WSD multi-`Download` duplex, mid-sheet duplex
//! cancel + `WIA_CMD_STOP_FEEDER`, mid-batch jam page preservation, `BadId`-before-`NotInstalled`,
//! and multi-backend `TwainBackend` + `WiaBackend` open routing) plus live OS `wia` worker checks
//! (including both 64-bit and 32-bit `i686` workers on Windows).

mod support;

use std::fs;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use scan_worker::scan_device::in_progress::{Tracker, Unit};
use scan_worker::scan_device::{
    DeviceId, DeviceInfo, Done, Driver, EncodedKind, Encoding, Event, Format, Paper, Pixels,
    ScanError, Settings, SideKnown, SizeMm, Source, Status,
};
use scan_worker::{Client, Error, WorkerCommand};

use support::{AT_MOST, is_scan, outline, pages, scan, soon, worker};

fn wia_fake(mode: &str) -> WorkerCommand {
    worker("wia-fake").arg(mode)
}

#[test]
fn wia_worker_lists_filters_cameras_reads_caps_and_scans_raw_encoded_and_duplex_with_noisy_stdout()
{
    let client = Client::new().worker(Driver::Wia, wia_fake("office").arg("--noisy-stdout"));

    let listed = client
        .list(Driver::Wia, AT_MOST)
        .expect("WIA worker listing");
    assert_eq!(listed.len(), 1, "camera node must be filtered out");
    let stored = &listed[0];
    assert_eq!(stored.driver, Driver::Wia);
    assert_eq!(
        stored.id.0,
        r"wia:{6BDD1FC6-810F-11D0-BEC7-08002BE2092F}\0042"
    );
    assert_eq!(stored.name, "Office WIA MFP");
    assert_eq!(stored.maker, "Microsoft");
    assert_eq!(stored.model, "WSD Scan Device");

    let mut device = client.open(stored, AT_MOST).expect("open WIA device");
    assert_eq!(device.renamed_from(), None);

    let caps = device.caps().expect("WIA caps pass protocol validation");
    assert!(!caps.settings_window);
    assert_eq!(caps.sources.len(), 2);
    assert_eq!(caps.sources[0].source, Source::Flatbed);
    assert!(!caps.sources[0].duplex);
    assert_eq!(
        caps.sources[0].encodings,
        vec![Encoding::Png, Encoding::Bmp]
    );
    assert_eq!(caps.sources[1].source, Source::Feeder);
    assert!(caps.sources[1].duplex);
    assert!(caps.sources[1].paper_auto);

    // WIA does not offer a standalone settings dialog.
    assert!(matches!(
        device.settings_window(None),
        Err(Error::Scan(ScanError::Unsupported))
    ));

    // 1. Flatbed raw `.bmp` scan -> `Done::OnePage`, BGR swapped + bottom-up flipped.
    let mut fb_raw = Settings::new(Source::Flatbed, 300, Format::Rgb8);
    fb_raw.paper = Paper::Size(SizeMm::LETTER);
    let (fb_events, fb_done) = scan(&mut device, &fb_raw, 1);
    assert_eq!(fb_done.expect("flatbed raw scan"), Done::OnePage);
    assert_eq!(outline(&fb_events), ["applied", "sheet 1", "page 1 front"]);
    let fb_page = pages(&fb_events)[0];
    assert_eq!(fb_page.dpi, (300.0, 300.0));
    assert_eq!(fb_page.side_known, SideKnown::Inferred);
    let Pixels::Raw(raw_img) = &fb_page.pixels else {
        panic!("expected raw flatbed image");
    };
    assert_eq!((raw_img.width(), raw_img.height()), (2, 2));
    assert_eq!(
        raw_img.data(),
        &[0xAA, 10, 20, 30, 40, 50, 1, 2, 3, 4, 5, 6]
    );

    // 2. Flatbed encoded PNG scan -> `Pixels::Encoded(Png)` passes `Client` encoding checks.
    let mut fb_enc = Settings::new(Source::Flatbed, 300, Format::Rgb8);
    fb_enc.paper = Paper::Size(SizeMm::LETTER);
    fb_enc.encodings = vec![Encoding::Jpeg, Encoding::Png];
    let (enc_events, enc_done) = scan(&mut device, &fb_enc, 1);
    assert_eq!(enc_done.expect("flatbed encoded scan"), Done::OnePage);
    let Pixels::Encoded(enc) = &pages(&enc_events)[0].pixels else {
        panic!("expected encoded PNG");
    };
    assert_eq!(enc.kind, EncodedKind::Png);
    assert!(enc.bytes.starts_with(b"\x89PNG\r\n\x1a\n"));

    // 3. WSD multi-`Download` duplex feeder scan (`first_sheet = 5`): emits `WarmingUp` once,
    // pairs two duplex sheets (4 pages), restores byte progress past the trailing empty stream,
    // and ends `Done::FeederEmpty`.
    let mut adf_dup = Settings::new(Source::Feeder, 300, Format::Rgb8);
    adf_dup.duplex = true;
    adf_dup.paper = Paper::Size(SizeMm::LETTER);
    let tracker = Tracker::new();
    let (tx, rx) = mpsc::sync_channel(32);
    let dup_done = device
        .scan(&adf_dup, 5, &tx, &tracker)
        .expect("WSD duplex feeder scan");
    drop(tx);
    assert_eq!(dup_done, Done::FeederEmpty);

    let dup_events: Vec<Event> = rx.into_iter().collect();
    assert_eq!(
        outline(&dup_events),
        [
            "applied",
            "status",
            "sheet 5",
            "page 5 front",
            "page 5 back",
            "sheet 6",
            "page 6 front",
            "page 6 back",
        ]
    );
    assert_eq!(dup_events[1], Event::Status(Status::WarmingUp));
    let tags: Vec<u8> = pages(&dup_events)
        .into_iter()
        .map(|p| match &p.pixels {
            Pixels::Raw(img) => img.data()[0],
            other => panic!("expected raw page, got {other:?}"),
        })
        .collect();
    assert_eq!(tags, [11, 12, 21, 22]);

    let snap = tracker.snapshot();
    assert_eq!(snap.unit, Unit::Bytes);
    assert_eq!(snap.done, 70);
    assert_eq!(snap.total, Some(70));

    device.close().expect("clean worker close");
}

#[test]
fn wia_worker_mid_sheet_duplex_cancel_finishes_sheet_and_issues_stop_feeder() {
    let dir = tempfile::tempdir().unwrap();
    let signal_file = dir.path().join("front.ready");
    let go_file = dir.path().join("cancel.go");
    let summary_file = dir.path().join("summary.json");

    let cmd = wia_fake("await-cancel")
        .env("SCAN_WORKER_TEST_SIGNAL_FILE", &signal_file)
        .env("SCAN_WORKER_TEST_GO_FILE", &go_file)
        .env("SCAN_WORKER_TEST_SUMMARY_FILE", &summary_file);

    let client = Client::new().worker(Driver::Wia, cmd);
    let stored = client.list(Driver::Wia, AT_MOST).unwrap().remove(0);
    let mut device = client.open(&stored, AT_MOST).unwrap();

    let mut settings = Settings::new(Source::Feeder, 300, Format::Rgb8);
    settings.duplex = true;
    settings.paper = Paper::Size(SizeMm::LETTER);

    let tracker = Tracker::new();
    let cancel_handle = tracker.clone();
    let (tx, rx) = mpsc::sync_channel(16);

    let batch_thread = thread::spawn(move || {
        let res = device.scan(&settings, 1, &tx, &tracker);
        drop(tx);
        device.close().expect("worker closes cleanly after cancel");
        res
    });

    // Wait until Sheet 1 Front has finished inside the worker's `Download` callback.
    assert!(
        soon(|| signal_file.exists()),
        "worker never reached Sheet 1 Front signal"
    );

    // Cancel from the parent `Client` side, give the client's 50 ms cancel poller a moment to
    // write `"cancel"` to the worker's stdin, then release the hook so Sheet 1 Back runs.
    cancel_handle.cancel();
    thread::sleep(Duration::from_millis(150));
    fs::write(&go_file, b"go").unwrap();

    let done = batch_thread
        .join()
        .expect("batch thread panicked")
        .expect("cancelled batch returns Ok");
    assert_eq!(done, Done::Cancelled);

    let events: Vec<Event> = rx.into_iter().collect();
    assert_eq!(
        outline(&events),
        ["applied", "sheet 1", "page 1 front", "page 1 back"]
    );

    let summary: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&summary_file).expect("worker wrote summary"))
            .unwrap();
    assert_eq!(summary["stop_feeder"], true);
    assert_eq!(summary["sheet2_fed"], 0);
}

#[test]
fn wia_worker_propagates_feeder_empty_mid_batch_jam_not_installed_bad_id_and_multi_backend_open() {
    // 1. Empty feeder before any page -> `Err(Error::Scan(ScanError::FeederEmpty))` after `Applied`.
    {
        let client = Client::new().worker(Driver::Wia, wia_fake("empty"));
        let stored = client.list(Driver::Wia, AT_MOST).unwrap().remove(0);
        let mut dev = client.open(&stored, AT_MOST).unwrap();
        let mut s = Settings::new(Source::Feeder, 300, Format::Rgb8);
        s.paper = Paper::Size(SizeMm::LETTER);
        let (evs, ended) = scan(&mut dev, &s, 1);
        assert!(is_scan(&ended, &ScanError::FeederEmpty), "{ended:?}");
        assert_eq!(outline(&evs), ["applied"]);
        dev.close().unwrap();
    }

    // 2. Mid-batch jam preserves Page 1 and returns `Err(Error::Scan(ScanError::Jammed))`.
    {
        let client = Client::new().worker(Driver::Wia, wia_fake("jam-mid-batch"));
        let stored = client.list(Driver::Wia, AT_MOST).unwrap().remove(0);
        let mut dev = client.open(&stored, AT_MOST).unwrap();
        let mut s = Settings::new(Source::Feeder, 300, Format::Rgb8);
        s.paper = Paper::Size(SizeMm::LETTER);
        let (evs, ended) = scan(&mut dev, &s, 1);
        assert!(is_scan(&ended, &ScanError::Jammed), "{ended:?}");
        assert_eq!(outline(&evs), ["applied", "sheet 1", "page 1 front"]);
        let Pixels::Raw(img) = &pages(&evs)[0].pixels else {
            panic!("expected Page 1 raw image");
        };
        assert_eq!(img.data()[0], 77);
        dev.close().unwrap();
    }

    // 3. `NotInstalled` on `list` and valid-id `open`, while malformed `wia:` IDs return `BadId` first!
    {
        let client = Client::new().worker(Driver::Wia, wia_fake("not-installed"));
        assert!(matches!(
            client.list(Driver::Wia, AT_MOST),
            Err(Error::Scan(ScanError::NotInstalled { ref what })) if what == "WIA"
        ));

        let valid_stored = DeviceInfo::new(
            DeviceId(r"wia:{6BDD1FC6-810F-11D0-BEC7-08002BE2092F}\0042".to_owned()),
            "Office WIA MFP".to_owned(),
            "Microsoft".to_owned(),
            "WSD Scan Device".to_owned(),
            Driver::Wia,
        );
        assert!(matches!(
            client.open(&valid_stored, AT_MOST),
            Err(Error::Scan(ScanError::NotInstalled { ref what })) if what == "WIA"
        ));

        let bad_stored = DeviceInfo::new(
            DeviceId("wia:".to_owned()),
            "Empty".to_owned(),
            "Maker".to_owned(),
            "Model".to_owned(),
            Driver::Wia,
        );
        assert!(matches!(
            client.open(&bad_stored, AT_MOST),
            Err(Error::Scan(ScanError::BadId))
        ));
    }

    // 4. Multi-backend worker (`TwainBackend` + `WiaBackend`): `open` skips `TwainBackend`'s
    // `BadId` and opens + scans the `Driver::Wia` device cleanly!
    {
        let info_client = Client::new().worker(Driver::Wia, wia_fake("office"));
        let stored = info_client.list(Driver::Wia, AT_MOST).unwrap().remove(0);

        let multi_client = Client::new().worker(Driver::Wia, wia_fake("multi-backend"));
        let mut dev = multi_client
            .open(&stored, AT_MOST)
            .expect("multi-backend worker opens WIA device past TwainBackend BadId");
        let mut fb = Settings::new(Source::Flatbed, 300, Format::Rgb8);
        fb.paper = Paper::Size(SizeMm::LETTER);
        let (evs, ended) = scan(&mut dev, &fb, 1);
        assert_eq!(ended.unwrap(), Done::OnePage);
        assert_eq!(pages(&evs).len(), 1);
        dev.close().unwrap();
    }
}

#[test]
fn live_wia_worker_handles_os_service_and_bad_ids() {
    let client = Client::new().worker(Driver::Wia, worker("wia").arg("--noisy-stdout"));

    // Regardless of OS, malformed `Driver::Wia` IDs are refused as `BadId` before COM / `NotInstalled`.
    let bad = DeviceInfo::new(
        DeviceId("wia:bad\0id".to_owned()),
        "Scanner".to_owned(),
        "Maker".to_owned(),
        "Model".to_owned(),
        Driver::Wia,
    );
    assert!(matches!(
        client.open(&bad, AT_MOST),
        Err(Error::Scan(ScanError::BadId))
    ));

    #[cfg(not(windows))]
    {
        assert!(matches!(
            client.list(Driver::Wia, AT_MOST),
            Err(Error::Scan(ScanError::NotInstalled { ref what })) if what == "WIA"
        ));
    }

    #[cfg(windows)]
    {
        check_windows_live_client(&client);

        // On 64-bit Windows, also verify a 32-bit `i686-pc-windows-msvc` WIA worker when the
        // `i686` target is installed.
        if cfg!(target_pointer_width = "64")
            && let Ok(exe_32) = i686_worker_exe()
        {
            let client_32 = Client::new().worker(
                Driver::Wia,
                WorkerCommand::new(exe_32).arg("wia").arg("--noisy-stdout"),
            );
            check_windows_live_client(&client_32);
        }
    }
}

#[cfg(windows)]
fn check_windows_live_client(client: &Client) {
    let deadline = Duration::from_secs(15);
    match client.list(Driver::Wia, deadline) {
        Ok(devices) => {
            for d in &devices {
                assert_eq!(d.driver, Driver::Wia);
                assert!(d.id.0.starts_with("wia:"));
            }
            let missing = DeviceInfo::new(
                DeviceId(r"wia:{6BDD1FC6-810F-11D0-BEC7-08002BE2092F}\9999".to_owned()),
                "Missing".to_owned(),
                "Maker".to_owned(),
                "Model".to_owned(),
                Driver::Wia,
            );
            assert!(matches!(
                client.open(&missing, deadline),
                Err(Error::Scan(ScanError::Gone))
            ));
        }
        Err(Error::Scan(ScanError::NotInstalled { what })) => {
            assert_eq!(what, "WIA");
        }
        Err(other) => panic!("unexpected live Windows WIA worker error: {other:?}"),
    }
}

#[cfg(windows)]
fn i686_worker_exe() -> Result<&'static std::path::Path, String> {
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use std::sync::OnceLock;

    static I686_EXE: OnceLock<Result<PathBuf, String>> = OnceLock::new();
    I686_EXE
        .get_or_init(|| {
            if let Some(prebuilt) = std::env::var_os("SCAN_WORKER_I686_EXE") {
                let path = PathBuf::from(prebuilt);
                assert!(
                    path.is_file(),
                    "SCAN_WORKER_I686_EXE is not a file: {}",
                    path.display()
                );
                return Ok(path);
            }
            let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..");
            let target = Path::new(env!("CARGO_TARGET_TMPDIR")).join("i686-scan-worker-wia");
            let output = Command::new(env!("CARGO"))
                .args([
                    "build",
                    "--locked",
                    "--quiet",
                    "-p",
                    "scan-worker",
                    "--features",
                    "fake",
                    "--bin",
                    "scan-worker-fake",
                    "--target",
                    "i686-pc-windows-msvc",
                    "--target-dir",
                ])
                .arg(&target)
                .current_dir(&workspace)
                .env_remove("RUSTFLAGS")
                .env_remove("CARGO_ENCODED_RUSTFLAGS")
                .env_remove("CARGO_BUILD_RUSTFLAGS")
                .env_remove("CARGO_BUILD_TARGET")
                .output()
                .expect("failed to invoke cargo for i686 scan-worker-fake");
            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                if stderr.contains("E0463") || stderr.contains("target may not be installed") {
                    return Err("i686-pc-windows-msvc target is not installed".to_owned());
                }
                panic!("failed to build i686 scan-worker-fake:\n{stderr}");
            }
            let built = target
                .join("i686-pc-windows-msvc")
                .join("debug")
                .join("scan-worker-fake.exe");
            assert!(
                built.is_file(),
                "built i686 worker missing at {}",
                built.display()
            );
            Ok(built)
        })
        .as_deref()
        .map_err(Clone::clone)
}
