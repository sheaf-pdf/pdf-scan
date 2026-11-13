//! Integration tests for [`Client::list_twain`] across all platforms (using deterministic
//! `twain-fake` workers), plus live Windows end-to-end tests driving both 64-bit (`x86_64`) and
//! 32-bit (`i686`) TWAIN workers from a 64-bit `Client` against `TWAIN2 Software Scanner`.

mod support;

use scan_worker::scan_device::{Driver, ScanError};
use scan_worker::{Client, Error, WorkerCommand};

use support::{AT_MOST, worker};

fn twain_fake(bits: u8, mode: &str) -> WorkerCommand {
    worker("twain-fake")
        .arg(bits.to_string())
        .arg(mode.to_owned())
}

#[test]
fn list_twain_runs_both_bitnesses_concurrently_prefers_64_bit_and_keeps_32_bit_only() {
    // Setting `SCAN_WORKER_TEST_RENDEZVOUS_DIR` makes the 64-bit and 32-bit workers wait for each
    // other to arrive inside `list()`: sequential execution would time out after 5 s, while
    // concurrent execution completes immediately.
    let rendezvous = tempfile::tempdir().unwrap();
    let client = Client::new()
        .worker(
            Driver::Twain { bits: 64 },
            twain_fake(64, "both-and-own")
                .env("SCAN_WORKER_TEST_RENDEZVOUS_DIR", rendezvous.path()),
        )
        .worker(
            Driver::Twain { bits: 32 },
            twain_fake(32, "both-and-own")
                .env("SCAN_WORKER_TEST_RENDEZVOUS_DIR", rendezvous.path()),
        );

    let listed = client
        .list_twain(AT_MOST)
        .expect("concurrent 64-bit + 32-bit listing");
    assert!(listed.other_failed.is_none(), "{:?}", listed.other_failed);
    let ids: Vec<&str> = listed.devices.iter().map(|d| d.id.0.as_str()).collect();
    assert_eq!(
        ids,
        [
            "twain64:Shared Scanner",
            "twain64:Modern 64 Scanner",
            "twain32:Legacy 32 Scanner",
        ]
    );
}

#[test]
fn list_twain_ignores_single_side_not_installed_and_errors_when_both_missing() {
    // 64-bit installed, 32-bit missing -> 64-bit devices returned with `other_failed: None`.
    let only_64 = Client::new()
        .worker(Driver::Twain { bits: 64 }, twain_fake(64, "both-and-own"))
        .worker(Driver::Twain { bits: 32 }, twain_fake(32, "not-installed"));
    let listed_64 = only_64.list_twain(AT_MOST).expect("64-bit succeeds");
    assert!(listed_64.other_failed.is_none());
    assert_eq!(listed_64.devices.len(), 2);
    assert!(
        listed_64
            .devices
            .iter()
            .all(|d| d.driver == Driver::Twain { bits: 64 })
    );

    // 64-bit missing, 32-bit installed -> 32-bit devices returned with `other_failed: None`.
    let only_32 = Client::new()
        .worker(Driver::Twain { bits: 64 }, twain_fake(64, "not-installed"))
        .worker(Driver::Twain { bits: 32 }, twain_fake(32, "both-and-own"));
    let listed_32 = only_32.list_twain(AT_MOST).expect("32-bit succeeds");
    assert!(listed_32.other_failed.is_none());
    assert_eq!(listed_32.devices.len(), 2);
    assert!(
        listed_32
            .devices
            .iter()
            .all(|d| d.driver == Driver::Twain { bits: 32 })
    );

    // Both missing -> `Err(Error::Scan(ScanError::NotInstalled { .. }))`.
    let neither = Client::new()
        .worker(Driver::Twain { bits: 64 }, twain_fake(64, "not-installed"))
        .worker(Driver::Twain { bits: 32 }, twain_fake(32, "not-installed"));
    assert!(matches!(
        neither.list_twain(AT_MOST),
        Err(Error::Scan(ScanError::NotInstalled { .. }))
    ));
}

#[test]
fn list_twain_surfaces_single_side_crash_or_timeout_in_other_failed() {
    // 64-bit works, 32-bit crashes -> returns 64-bit devices + `other_failed: Some((32, Crashed))`.
    let crash_32 = Client::new()
        .worker(Driver::Twain { bits: 64 }, twain_fake(64, "both-and-own"))
        .worker(Driver::Twain { bits: 32 }, twain_fake(32, "crash"));
    let listed = crash_32.list_twain(AT_MOST).expect("64-bit still returned");
    assert_eq!(listed.devices.len(), 2);
    assert!(matches!(
        listed.other_failed,
        Some((32, Error::Crashed { .. }))
    ));

    // 64-bit times out, 32-bit works -> returns 32-bit devices + `other_failed: Some((64, Scan(TimedOut)))`.
    let timeout_64 = Client::new()
        .worker(Driver::Twain { bits: 64 }, twain_fake(64, "timed-out"))
        .worker(Driver::Twain { bits: 32 }, twain_fake(32, "both-and-own"));
    let listed = timeout_64
        .list_twain(AT_MOST)
        .expect("32-bit still returned");
    assert_eq!(listed.devices.len(), 2);
    assert!(matches!(
        listed.other_failed,
        Some((64, Error::Scan(ScanError::TimedOut)))
    ));

    // 64-bit not installed AND 32-bit crashes -> surfaces the real crash rather than `NotInstalled`.
    let missing_64_crash_32 = Client::new()
        .worker(Driver::Twain { bits: 64 }, twain_fake(64, "not-installed"))
        .worker(Driver::Twain { bits: 32 }, twain_fake(32, "crash"));
    assert!(matches!(
        missing_64_crash_32.list_twain(AT_MOST),
        Err(Error::Crashed { .. })
    ));
}

#[test]
fn list_twain_handles_single_registered_bitness_and_no_registered_worker() {
    let empty = Client::new();
    assert!(matches!(
        empty.list_twain(AT_MOST),
        Err(Error::NoWorker(Driver::Twain { .. }))
    ));

    let single_32 =
        Client::new().worker(Driver::Twain { bits: 32 }, twain_fake(32, "both-and-own"));
    let listed = single_32.list_twain(AT_MOST).expect("single 32-bit worker");
    assert!(listed.other_failed.is_none());
    assert_eq!(listed.devices.len(), 2);
}

#[cfg(windows)]
mod windows_live {
    use std::io::Write as _;
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use std::sync::OnceLock;
    use std::time::Duration;

    use scan_worker::scan_device::{Done, Driver, Format, Pixels, ScanError, Settings, Source};
    use scan_worker::{Client, Error, WorkerCommand};

    use super::support::{pages, scan, worker};

    const SAMPLE_NAME: &str = "TWAIN2 Software Scanner";
    const DEADLINE: Duration = Duration::from_secs(20);

    fn require_sample() -> bool {
        std::env::var("TWAIN_DSM_REQUIRE_SAMPLE").is_ok_and(|v| v == "1")
    }

    /// Builds (or uses `SCAN_WORKER_I686_EXE`) the 32-bit `i686-pc-windows-msvc` test worker
    /// binary so a 64-bit `Client` can drive both 64-bit and 32-bit TWAIN workers in one test.
    ///
    /// Returns `Err` only when the `i686-pc-windows-msvc` target toolchain isn't installed; any
    /// real compile failure panics immediately.
    fn i686_worker_exe() -> Result<&'static Path, String> {
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
                let target = Path::new(env!("CARGO_TARGET_TMPDIR")).join("i686-scan-worker");
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

    fn twain_worker_64() -> WorkerCommand {
        worker("twain")
            .arg("--noisy-stdout")
            .env("TWAIN_SHOWCONSOLEMESSAGES", "1")
            .env("CAP_XFERCOUNT", "10")
    }

    fn twain_worker_32(exe: &Path) -> WorkerCommand {
        WorkerCommand::new(exe)
            .arg("twain")
            .arg("--noisy-stdout")
            .env("TWAIN_SHOWCONSOLEMESSAGES", "1")
            .env("CAP_XFERCOUNT", "10")
    }

    #[test]
    fn client_drives_both_64_bit_and_32_bit_twain_sample_workers_with_noisy_stdout() {
        // When compiled for 32-bit Windows directly, `worker("twain")` is already 32-bit.
        if cfg!(target_pointer_width = "32") {
            let client = Client::new().worker(Driver::Twain { bits: 32 }, twain_worker_64());
            let listed = match client.list(Driver::Twain { bits: 32 }, DEADLINE) {
                Ok(devs) => devs,
                Err(Error::Scan(ScanError::NotInstalled { what })) => {
                    assert!(
                        !require_sample(),
                        "{what} missing with TWAIN_DSM_REQUIRE_SAMPLE=1"
                    );
                    let _ = writeln!(std::io::stderr(), "twain_worker (32-bit): skipped ({what})");
                    return;
                }
                Err(other) => panic!("32-bit list failed: {other:?}"),
            };
            let Some(sample_32) = listed.into_iter().find(|d| d.name == SAMPLE_NAME) else {
                assert!(
                    !require_sample(),
                    "{SAMPLE_NAME} missing in 32-bit with TWAIN_DSM_REQUIRE_SAMPLE=1"
                );
                return;
            };
            let mut dev_32 = client.open(&sample_32, DEADLINE).expect("open 32-bit");
            let settings = Settings::new(Source::Feeder, 100, Format::Rgb8);
            let (ev_32, done_32) = scan(&mut dev_32, &settings, 1);
            assert_eq!(done_32.expect("32-bit scan"), Done::FeederEmpty);
            assert_eq!(pages(&ev_32).len(), 10);
            dev_32.close().expect("close 32-bit");
            return;
        }

        // On runners without `TWAIN_DSM_REQUIRE_SAMPLE=1` (e.g. `check` or `msrv` on Windows,
        // where neither TWAIN nor the `i686-pc-windows-msvc` target is installed), check 64-bit
        // availability and `i686` target availability cleanly before requiring dual bitness.
        let exe_32 = match i686_worker_exe() {
            Ok(exe) => exe,
            Err(missing) => {
                assert!(
                    !require_sample(),
                    "{missing} with TWAIN_DSM_REQUIRE_SAMPLE=1"
                );
                let _ = writeln!(std::io::stderr(), "twain_worker: skipped ({missing})");
                return;
            }
        };

        let client = Client::new()
            .worker(Driver::Twain { bits: 64 }, twain_worker_64())
            .worker(Driver::Twain { bits: 32 }, twain_worker_32(exe_32));

        // 1. Merged listing deduplicates dual-installed `TWAIN2 Software Scanner` in favour of 64-bit.
        let merged = match client.list_twain(DEADLINE) {
            Ok(list) => list,
            Err(Error::Scan(ScanError::NotInstalled { what })) => {
                assert!(
                    !require_sample(),
                    "{what} not installed and TWAIN_DSM_REQUIRE_SAMPLE=1"
                );
                let _ = writeln!(std::io::stderr(), "twain_worker: skipped ({what})");
                return;
            }
            Err(other) => panic!("list_twain failed: {other:?}"),
        };
        assert!(merged.other_failed.is_none(), "{:?}", merged.other_failed);
        let sample_entries: Vec<_> = merged
            .devices
            .iter()
            .filter(|d| d.name == SAMPLE_NAME)
            .collect();
        if sample_entries.is_empty() {
            assert!(
                !require_sample(),
                "{SAMPLE_NAME} not installed and TWAIN_DSM_REQUIRE_SAMPLE=1"
            );
            let _ = writeln!(
                std::io::stderr(),
                "twain_worker: skipped (no sample source)"
            );
            return;
        }
        assert_eq!(
            sample_entries.len(),
            1,
            "dual-installed sample source must deduplicate to 1 entry: {sample_entries:?}"
        );

        // 2. Query 64-bit and 32-bit individually to verify whether both sample bitnesses exist,
        // and scan 10 pages through each!
        let sample_64 = client
            .list(Driver::Twain { bits: 64 }, DEADLINE)
            .ok()
            .and_then(|devs| devs.into_iter().find(|d| d.name == SAMPLE_NAME));
        let sample_32 = client
            .list(Driver::Twain { bits: 32 }, DEADLINE)
            .ok()
            .and_then(|devs| devs.into_iter().find(|d| d.name == SAMPLE_NAME));

        if require_sample() {
            assert!(
                sample_64.is_some() && sample_32.is_some(),
                "TWAIN_DSM_REQUIRE_SAMPLE=1 requires both 64-bit and 32-bit sample sources \
                 (got 64={sample_64:?}, 32={sample_32:?})"
            );
            // When both are installed, `list_twain` must pick `twain64:`.
            assert_eq!(sample_entries[0].driver, Driver::Twain { bits: 64 });
            assert_eq!(sample_entries[0].id.0, format!("twain64:{SAMPLE_NAME}"));
        }

        let settings = Settings::new(Source::Feeder, 100, Format::Rgb8);

        let mut pages_64_bytes: Option<Vec<Vec<u8>>> = None;
        if let Some(info_64) = sample_64 {
            let mut dev_64 = client.open(&info_64, DEADLINE).expect("open 64-bit worker");
            let caps_64 = dev_64.caps().expect("64-bit caps");
            assert!(!caps_64.sources.is_empty());
            let (events_64, done_64) = scan(&mut dev_64, &settings, 1);
            assert_eq!(done_64.expect("64-bit scan"), Done::FeederEmpty);
            let raw_pages = pages(&events_64);
            assert_eq!(raw_pages.len(), 10, "expected 10 pages from 64-bit worker");
            pages_64_bytes = Some(
                raw_pages
                    .iter()
                    .map(|p| match &p.pixels {
                        Pixels::Raw(img) => img.data().to_vec(),
                        other => panic!("expected raw pixels, got {other:?}"),
                    })
                    .collect(),
            );
            dev_64.close().expect("close 64-bit worker");
        }

        if let Some(info_32) = sample_32 {
            assert_eq!(info_32.id.0, format!("twain32:{SAMPLE_NAME}"));
            let mut dev_32 = client
                .open(&info_32, DEADLINE)
                .expect("open 32-bit worker from 64-bit client");
            let caps_32 = dev_32.caps().expect("32-bit caps");
            assert!(!caps_32.sources.is_empty());
            let (events_32, done_32) = scan(&mut dev_32, &settings, 1);
            assert_eq!(done_32.expect("32-bit scan"), Done::FeederEmpty);
            let raw_pages = pages(&events_32);
            assert_eq!(raw_pages.len(), 10, "expected 10 pages from 32-bit worker");
            if let Some(bytes_64) = pages_64_bytes {
                let bytes_32: Vec<Vec<u8>> = raw_pages
                    .iter()
                    .map(|p| match &p.pixels {
                        Pixels::Raw(img) => img.data().to_vec(),
                        other => panic!("expected raw pixels, got {other:?}"),
                    })
                    .collect();
                assert_eq!(
                    bytes_64, bytes_32,
                    "64-bit and 32-bit worker pixels must match byte-for-byte"
                );
            }
            dev_32.close().expect("close 32-bit worker");
        }
    }
}
