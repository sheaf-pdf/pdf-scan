//! The `sane` scenario: `scan-device`'s SANE backend served, for tests through SANE's own `test`
//! backend and through the fake libsane, which a test gives the worker by the loader's search
//! path on its command.
//!
//! `sane [--paced] [--option NAME=VALUE]...`: a `SaneBackend` setting each option by name, in the
//! order given, on every device it opens and at each batch. `--paced`, for SANE's `test` backend,
//! pauses a millisecond after each page's first read: with its reads limited to a few bytes (its
//! `read-limit` and `read-limit-size`, in the test's `test.conf`), a page then ends long after
//! the backend's reader thread has written it and exited, clear of a race in that backend that
//! can block its next page for good.
//!
//! **A stand-in for the system's configuration,** a test's own: where the worker's environment
//! names a directory as `SCAN_WORKER_TEST_SANE_SYSTEM`, and its `SANE_CONFIG_DIR` is the one the
//! client gives a worker that opens a stored device (its directory, then `:`, so SANE reads its
//! own list after it), the worker first runs itself again in place (`exec`, so the process, its
//! pipes and its directory stay the client's), with `SANE_CONFIG_DIR` the client's directory then
//! that one, and without the variable: SANE reads the stored id's one-backend configuration, then
//! the stand-in, as it would the system's, and never the system's own. Once, before its `hello`,
//! since the process it becomes lacks the variable; and never for a listing's worker, whose
//! `SANE_CONFIG_DIR` is its command's own. Unix alone: elsewhere it says it can't, and exits.

use std::{
    env,
    ffi::OsString,
    process::ExitCode,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::SyncSender,
    },
    thread,
    time::Duration,
};

use sane_dl::CONFIG_DIR_VARIABLE;
use scan_device::in_progress::{Level, Progress, Unit};
use scan_device::sane::SaneBackend;
use scan_device::{
    Backend, Caps, Device, DeviceInfo, Done, Event, Opened, ScanError, Settings, WindowClosed,
    WindowOwner,
};

/// The variable naming the directory that stands for the system's SANE configuration.
const SYSTEM: &str = "SCAN_WORKER_TEST_SANE_SYSTEM";

/// The scenario, given the arguments after its name.
pub fn main(args: &[String]) -> ExitCode {
    if let Some(ended) = stand_in() {
        return ended;
    }
    let mut backend = SaneBackend::new();
    let mut paced = false;
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--paced" => paced = true,
            "--option" => match args.next().and_then(|option| option.split_once('=')) {
                Some((name, value)) => backend = backend.with_option(name, value),
                None => return usage("--option takes NAME=VALUE"),
            },
            other => return usage(&format!("no argument {other:?}")),
        }
    }
    if paced {
        scan_worker::serve(vec![Box::new(PacedBackend(backend))])
    } else {
        scan_worker::serve(vec![Box::new(backend)])
    }
}

fn usage(problem: &str) -> ExitCode {
    eprintln!("scan-worker-fake: sane [--paced] [--option NAME=VALUE]...: {problem}");
    ExitCode::from(2)
}

/// Where the stand-in is asked for and this is an open's worker, the worker run again with it,
/// and how that failed; `None` where it isn't asked for, or this is a listing's worker.
fn stand_in() -> Option<ExitCode> {
    let system = env::var_os(SYSTEM)?;
    let config = env::var_os(CONFIG_DIR_VARIABLE)?;
    if !config.as_encoded_bytes().ends_with(b":") {
        return None;
    }
    // `<directory>:` becomes `<directory>:<stand-in>`, with nothing of SANE's own after it.
    let mut stood_in = config;
    stood_in.push(system);
    Some(run_again(stood_in))
}

/// This program run again in place, with its arguments, `SANE_CONFIG_DIR` set to `config`, and
/// the stand-in's variable gone; returns only if that failed, saying how.
#[cfg(unix)]
fn run_again(config: OsString) -> ExitCode {
    use std::os::unix::process::CommandExt;
    use std::process::Command;
    let error = match env::current_exe() {
        Ok(me) => Command::new(me)
            .args(env::args_os().skip(1))
            .env(CONFIG_DIR_VARIABLE, config)
            .env_remove(SYSTEM)
            .exec(),
        Err(error) => error,
    };
    eprintln!("scan-worker-fake: couldn't run again with the stand-in configuration: {error}");
    ExitCode::from(2)
}

/// Elsewhere, no process is run again in place: said, and the worker exits.
#[cfg(not(unix))]
fn run_again(_config: OsString) -> ExitCode {
    eprintln!(
        "scan-worker-fake: a stand-in for the system's SANE configuration runs the worker again \
         in place, which only Unix does"
    );
    ExitCode::from(2)
}

/// A SANE backend whose devices' batches are paced ([`Paced`]).
struct PacedBackend(SaneBackend);

impl Backend for PacedBackend {
    fn list(&mut self, deadline: Duration) -> Result<Vec<DeviceInfo>, ScanError> {
        self.0.list(deadline)
    }

    fn open(&mut self, device: &DeviceInfo, deadline: Duration) -> Result<Opened, ScanError> {
        let mut opened = self.0.open(device, deadline)?;
        opened.device = Box::new(PacedDevice(opened.device));
        Ok(opened)
    }
}

/// A [`PacedBackend`]'s device.
struct PacedDevice(Box<dyn Device>);

impl Device for PacedDevice {
    fn caps(&mut self) -> Result<Caps, ScanError> {
        self.0.caps()
    }

    fn scan(
        &mut self,
        settings: &Settings,
        first_sheet: u32,
        events: &SyncSender<Event>,
        progress: &dyn Progress,
    ) -> Result<Done, ScanError> {
        let paced = Paced {
            progress,
            read: AtomicBool::new(false),
        };
        self.0.scan(settings, first_sheet, events, &paced)
    }

    fn settings_window(&mut self, owner: Option<WindowOwner>) -> Result<WindowClosed, ScanError> {
        self.0.settings_window(owner)
    }
}

/// The batch's progress, pausing a millisecond after each page's first read: the backend counts
/// a page's bytes from 0 as it's started, then after each read.
struct Paced<'a> {
    progress: &'a dyn Progress,
    /// Whether the page being read has had its first read.
    read: AtomicBool,
}

impl Progress for Paced<'_> {
    fn set_total(&self, total: Option<u64>, unit: Unit) {
        self.progress.set_total(total, unit);
    }

    fn set(&self, done: u64) {
        self.progress.set(done);
        if done == 0 {
            // A page begins.
            self.read.store(false, Ordering::Relaxed);
        } else if !self.read.swap(true, Ordering::Relaxed) {
            thread::sleep(Duration::from_millis(1));
        }
    }

    fn inc(&self, n: u64) {
        self.progress.inc(n);
    }

    fn message(&self, level: Level, msg: &str) {
        self.progress.message(level, msg);
    }

    fn is_cancelled(&self) -> bool {
        self.progress.is_cancelled()
    }

    fn child(&self, name: &str, weight: f64) -> Box<dyn Progress> {
        self.progress.child(name, weight)
    }
}
