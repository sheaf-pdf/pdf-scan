//! The `twain` scenario, and deterministic `twain-fake` stand-ins: `scan-device`'s TWAIN backend
//! served in a 64-bit or 32-bit worker process, plus a noisy-`stdout` wrapper proving that driver
//! chatter on standard output never reaches the JSON-lines wire protocol.

use std::{
    io::{self, Write},
    path::PathBuf,
    process::ExitCode,
    sync::mpsc::SyncSender,
    thread,
    time::Duration,
};

use scan_device::in_progress::Progress;
use scan_device::twain::{Delivery, TwainBackend};
use scan_device::{
    Backend, Caps, Device, DeviceId, DeviceInfo, Done, Driver, Event, Opened, ScanError, Settings,
    WindowClosed, WindowOwner,
};

/// `twain [--native] [--dsm PATH] [--delivery events|callback] [--noisy-stdout]`: serves
/// [`TwainBackend`] over `scan_worker::serve`.
pub fn main(args: &[String]) -> ExitCode {
    let mut backend = TwainBackend::new();
    let mut noisy = false;
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--native" => backend = backend.with_native_transfer(),
            "--noisy-stdout" => noisy = true,
            "--dsm" => match iter.next() {
                Some(path) => backend = backend.with_dsm_path(PathBuf::from(path)),
                None => return usage("--dsm takes PATH"),
            },
            "--delivery" => match iter.next().map(String::as_str) {
                Some("events") => backend = backend.with_delivery(Delivery::Events),
                Some("callback") => backend = backend.with_delivery(Delivery::Callback),
                _ => return usage("--delivery takes events or callback"),
            },
            other => return usage(&format!("no argument {other:?}")),
        }
    }
    if noisy {
        scan_worker::serve(vec![Box::new(NoisyBackend(Box::new(backend)))])
    } else {
        scan_worker::serve(vec![Box::new(backend)])
    }
}

fn usage(problem: &str) -> ExitCode {
    eprintln!(
        "scan-worker-fake: twain [--native] [--dsm PATH] [--delivery events|callback] \
         [--noisy-stdout]: {problem}"
    );
    ExitCode::from(2)
}

/// Wraps any [`Backend`] and writes 4 KiB blocks, partial lines without `\n`, and fake JSON
/// protocol lines to `std::io::stdout()` around every backend and device call.
pub struct NoisyBackend(pub Box<dyn Backend>);

fn chatter(stage: &str) {
    let mut stdout = io::stdout().lock();
    // Partial line with no newline, fake protocol messages, and >4 KiB of chatter.
    let _ = write!(stdout, "[driver stdout {stage}: partial");
    let _ = writeln!(stdout, r#"] {{"error":"Jammed"}}"#);
    let block = vec![b'X'; 4096];
    let _ = stdout.write_all(&block);
    let _ = writeln!(stdout, "\n{{\"hello\":{{\"protocol\":99}}}}");
    let _ = stdout.flush();
}

impl Backend for NoisyBackend {
    fn list(&mut self, deadline: Duration) -> Result<Vec<DeviceInfo>, ScanError> {
        chatter("before-list");
        let res = self.0.list(deadline);
        chatter("after-list");
        res
    }

    fn open(&mut self, device: &DeviceInfo, deadline: Duration) -> Result<Opened, ScanError> {
        chatter("before-open");
        let mut opened = self.0.open(device, deadline)?;
        chatter("after-open");
        opened.device = Box::new(NoisyDevice(opened.device));
        Ok(opened)
    }
}

struct NoisyDevice(Box<dyn Device>);

impl Device for NoisyDevice {
    fn caps(&mut self) -> Result<Caps, ScanError> {
        chatter("before-caps");
        let res = self.0.caps();
        chatter("after-caps");
        res
    }

    fn scan(
        &mut self,
        settings: &Settings,
        first_sheet: u32,
        events: &SyncSender<Event>,
        progress: &dyn Progress,
    ) -> Result<Done, ScanError> {
        chatter("before-scan");
        let (inner_tx, inner_rx) = std::sync::mpsc::sync_channel::<Event>(0);
        let res = thread::scope(|scope| {
            let forward = scope.spawn(|| {
                for event in inner_rx {
                    chatter("mid-scan-event");
                    if events.send(event).is_err() {
                        break;
                    }
                }
            });
            let done = self.0.scan(settings, first_sheet, &inner_tx, progress);
            drop(inner_tx);
            let _ = forward.join();
            done
        });
        chatter("after-scan");
        res
    }

    fn settings_window(&mut self, owner: Option<WindowOwner>) -> Result<WindowClosed, ScanError> {
        chatter("before-window");
        let res = self.0.settings_window(owner);
        chatter("after-window");
        res
    }
}

/// `twain-fake <32|64> <mode>`: deterministic stand-in backend for `Client::list_twain` tests on
/// every platform.
pub fn fake_main(args: &[String]) -> ExitCode {
    let Some(bits) = args.first().and_then(|s| s.parse::<u8>().ok()) else {
        eprintln!("scan-worker-fake: twain-fake <32|64> <mode>: missing bits");
        return ExitCode::from(2);
    };
    let mode = args
        .get(1)
        .map_or("both-and-own", String::as_str)
        .to_owned();
    scan_worker::serve(vec![Box::new(FakeTwain { bits, mode })])
}

struct FakeTwain {
    bits: u8,
    mode: String,
}

impl FakeTwain {
    fn dev(&self, product: &str, maker: &str) -> DeviceInfo {
        let prefix = if self.bits == 32 {
            "twain32:"
        } else {
            "twain64:"
        };
        DeviceInfo::new(
            DeviceId(format!("{prefix}{product}")),
            format!("{maker} {product}"),
            maker.to_owned(),
            product.to_owned(),
            Driver::Twain { bits: self.bits },
        )
    }
}

impl Backend for FakeTwain {
    fn list(&mut self, _deadline: Duration) -> Result<Vec<DeviceInfo>, ScanError> {
        match self.mode.as_str() {
            "both-and-own" => {
                // If `SCAN_WORKER_TEST_RENDEZVOUS_DIR` is set, signal arrival and wait for the
                // other bitness's worker to arrive concurrently (proving parallel execution
                // deterministically without wall-clock flakiness).
                if let Some(dir) = std::env::var_os("SCAN_WORKER_TEST_RENDEZVOUS_DIR") {
                    let dir = PathBuf::from(dir);
                    let mine = dir.join(format!("{}.ready", self.bits));
                    let other_bits = if self.bits == 64 { 32 } else { 64 };
                    let theirs = dir.join(format!("{other_bits}.ready"));
                    let _ = std::fs::write(&mine, b"1");
                    let start = std::time::Instant::now();
                    while !theirs.exists() {
                        if start.elapsed() > Duration::from_secs(5) {
                            return Err(ScanError::TimedOut);
                        }
                        thread::sleep(Duration::from_millis(5));
                    }
                }
                if self.bits == 64 {
                    Ok(vec![
                        self.dev("Shared Scanner", "Maker"),
                        self.dev("Modern 64 Scanner", "Maker"),
                    ])
                } else {
                    Ok(vec![
                        self.dev("Shared Scanner", "Maker"),
                        self.dev("Legacy 32 Scanner", "Maker"),
                    ])
                }
            }
            "not-installed" => Err(ScanError::NotInstalled {
                what: "TWAIN".to_owned(),
            }),
            "timed-out" => Err(ScanError::TimedOut),
            "crash" => std::process::exit(9),
            other => Err(ScanError::Driver {
                message: format!("unknown twain-fake mode {other:?}"),
                shown: false,
            }),
        }
    }

    fn open(&mut self, _device: &DeviceInfo, _deadline: Duration) -> Result<Opened, ScanError> {
        Err(ScanError::Unsupported)
    }
}
