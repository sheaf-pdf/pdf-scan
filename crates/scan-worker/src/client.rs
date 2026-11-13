//! The client's side: a worker started for each listing and each open device, its messages read
//! and checked, and the model's calls made through it.

mod read;
mod spawn;
mod worker;

use std::{
    env,
    ffi::{OsStr, OsString},
    fmt,
    path::{Path, PathBuf},
    sync::mpsc::SyncSender,
    time::Duration,
};

use scan_device::in_progress::Progress;
use scan_device::{
    Caps, DeviceId, DeviceInfo, Done, Driver, Event, ScanError, Settings, WindowClosed, WindowOwner,
};

use crate::Error;
pub use worker::StopHandle;
use worker::Worker;

/// The result of [`Client::list_twain`]: the merged 64-bit and 32-bit TWAIN scanners, and any
/// failure from one bitness's worker when the other bitness succeeded.
#[derive(Debug)]
#[non_exhaustive]
pub struct TwainList {
    /// The TWAIN devices found, ordered 64-bit before 32-bit, with dual-installed sources
    /// matching `(maker, model, name)` deduplicated in favour of the 64-bit entry while keeping
    /// 32-bit-only drivers.
    pub devices: Vec<DeviceInfo>,
    /// When both 64-bit and 32-bit TWAIN workers were registered and one succeeded while the
    /// other failed with anything other than [`ScanError::NotInstalled`], the failed bitness
    /// (`32` or `64`) and its [`Error`].
    pub other_failed: Option<(u8, Error)>,
}

/// Starts a worker for each listing and each device it opens, and makes the device model's calls
/// through it: the app's side of drivers run in a process of their own.
///
/// It's given the worker to start for each [`Driver`] ([`Client::worker`]): a program, an
/// absolute path, that calls [`serve`](crate::serve) with that driver API's backends. A listing
/// ([`Client::list`]) starts one that lists, answers and is quit, so a slow listing never holds a
/// device's worker; an open ([`Client::open`]) starts one that keeps the device until it's
/// closed ([`RemoteDevice::close`]), so each open device has a process of its own.
///
/// `Send` and `Sync`: a listing can run on one thread while a device scans on another.
///
/// On Unix, the app keeps `SIGPIPE` ignored, as Rust's runtime leaves it: a write to a worker
/// that has just ended would otherwise end the app (the crate's docs, "The client").
///
/// ```no_run
/// use std::{sync::mpsc, thread, time::Duration};
///
/// use scan_worker::scan_device::in_progress::Tracker;
/// use scan_worker::scan_device::{Driver, Event, Format, Settings, Source};
/// use scan_worker::{Client, WorkerCommand};
///
/// // The app's own binary, run as its SANE worker: its `main` calls `scan_worker::serve` with the
/// // SANE backend when it's given `--scan-worker`.
/// let me = std::env::current_exe()?;
/// let client = Client::new().worker(Driver::Sane, WorkerCommand::new(me).arg("--scan-worker"));
///
/// let stored = client.list(Driver::Sane, Duration::from_secs(10))?.remove(0);
/// let mut device = client.open(&stored, Duration::from_secs(10))?;
/// let settings = Settings::new(Source::Feeder, 300, Format::Gray8);
/// let (events, received) = mpsc::sync_channel(2);
/// let pages = thread::spawn(move || {
///     received.iter().filter(|event| matches!(event, Event::Page(_))).count()
/// });
/// let done = device.scan(&settings, 1, &events, &Tracker::new())?;
/// drop(events);
/// println!("{done:?}, {} pages", pages.join().expect("the pages' thread"));
/// device.close()?;
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[derive(Clone, Debug, Default)]
pub struct Client {
    workers: Vec<(Driver, WorkerCommand)>,
    limits: Limits,
    timeouts: Timeouts,
    work_dir: Option<PathBuf>,
}

impl Client {
    /// A client with no worker given, the default [`Limits`] and [`Timeouts`], and the system's
    /// temporary directory ([`std::env::temp_dir`]) to make each worker's directory in.
    pub fn new() -> Client {
        Client::default()
    }

    /// The client with `command` as the worker for `driver`'s devices, in place of any given
    /// before. The program must be an absolute path, which is checked as a worker starts: it's
    /// never searched for.
    #[must_use = "it returns the client so made, the one it was called on moved into it"]
    pub fn worker(mut self, driver: Driver, command: WorkerCommand) -> Client {
        self.workers.retain(|(given, _)| *given != driver);
        self.workers.push((driver, command));
        self
    }

    /// The client with `limits` on what a worker may send.
    #[must_use = "it returns the client so made, the one it was called on moved into it"]
    pub fn limits(mut self, limits: Limits) -> Client {
        self.limits = limits;
        self
    }

    /// The client with `timeouts` for its workers.
    #[must_use = "it returns the client so made, the one it was called on moved into it"]
    pub fn timeouts(mut self, timeouts: Timeouts) -> Client {
        self.timeouts = timeouts;
        self
    }

    /// The client making each worker's private directory under `dir`, which must be an absolute
    /// path (checked as a worker starts).
    #[must_use = "it returns the client so made, the one it was called on moved into it"]
    pub fn work_dir(mut self, dir: impl Into<PathBuf>) -> Client {
        self.work_dir = Some(dir.into());
        self
    }

    /// The devices `driver`'s worker finds now, within `deadline`: a worker started for the
    /// listing alone, and quit once it answers.
    ///
    /// The call returns once that worker has exited, so its process and its directory are gone:
    /// a driver slow to close holds the answer up to [`Timeouts::grace`], and one that won't
    /// exit within it is killed and given the grace again to be reaped.
    ///
    /// The worker's backends' errors are [`Error::Scan`]; [`ScanError::TimedOut`], the listing
    /// past its deadline, also kills the worker at once, since its driver may still be at it. A
    /// worker that hasn't answered by the deadline and [`Timeouts::grace`] is killed
    /// ([`Error::Stopped`]); a listed device of another driver API than `driver` is
    /// [`Error::Protocol`]. Errors that come before the call ([`Error::NoWorker`],
    /// [`Error::Start`]) and the worker's start's are as [`Client::open`] has them.
    pub fn list(&self, driver: Driver, deadline: Duration) -> Result<Vec<DeviceInfo>, Error> {
        let mut worker = self.start(driver, None)?;
        let listed = worker.list(deadline);
        if matches!(listed, Err(Error::Scan(ScanError::TimedOut))) {
            worker.kill();
        } else {
            // The answer is had: how the worker then ends doesn't change it.
            let _ = worker.close();
        }
        listed
    }

    /// Lists TWAIN scanners across the registered 64-bit and 32-bit TWAIN workers concurrently,
    /// each given the full `deadline`.
    ///
    /// On 64-bit Windows, vendors ship some TWAIN sources in both `twain_64` and `twain_32` and
    /// others (such as older Fujitsu PaperStream IP packages) in `twain_32` alone. When both
    /// `Driver::Twain { bits: 64 }` and `Driver::Twain { bits: 32 }` workers are registered,
    /// both run in parallel: 64-bit entries come first, any 32-bit entry matching a 64-bit
    /// entry's `(maker, model, name)` is dropped in favour of 64-bit, and 32-bit-only entries
    /// follow.
    ///
    /// If one bitness answers [`ScanError::NotInstalled`] while the other succeeds, the missing
    /// DSM is ignored cleanly (`other_failed` is `None`). If one bitness crashes, times out, or
    /// fails to start while the other succeeds, the working bitness's scanners are returned with
    /// the failure recorded in [`TwainList::other_failed`]. If both bitnesses fail, a real failure
    /// takes precedence over [`ScanError::NotInstalled`], and the 64-bit error takes precedence
    /// when both fail with real errors (or both answer [`ScanError::NotInstalled`]). If only one
    /// TWAIN bitness is registered on this client, only that worker runs; if neither is
    /// registered, answers [`Error::NoWorker`].
    pub fn list_twain(&self, deadline: Duration) -> Result<TwainList, Error> {
        let d64 = Driver::Twain { bits: 64 };
        let d32 = Driver::Twain { bits: 32 };
        let has_64 = self.has_worker(d64);
        let has_32 = self.has_worker(d32);

        match (has_64, has_32) {
            (false, false) => {
                let host_bits = if cfg!(target_pointer_width = "32") {
                    32
                } else {
                    64
                };
                Err(Error::NoWorker(Driver::Twain { bits: host_bits }))
            }
            (true, false) => self.list(d64, deadline).map(|devices| TwainList {
                devices,
                other_failed: None,
            }),
            (false, true) => self.list(d32, deadline).map(|devices| TwainList {
                devices,
                other_failed: None,
            }),
            (true, true) => {
                let (res_64, res_32) = std::thread::scope(|scope| {
                    let list_32 = scope.spawn(|| self.list(d32, deadline));
                    let res_64 = self.list(d64, deadline);
                    let res_32 = list_32
                        .join()
                        .expect("32-bit TWAIN listing thread panicked");
                    (res_64, res_32)
                });
                match (res_64, res_32) {
                    (Ok(devs_64), Ok(devs_32)) => Ok(TwainList {
                        devices: merge_twain_devices(devs_64, devs_32),
                        other_failed: None,
                    }),
                    (Ok(devs_64), Err(Error::Scan(ScanError::NotInstalled { .. }))) => {
                        Ok(TwainList {
                            devices: devs_64,
                            other_failed: None,
                        })
                    }
                    (Err(Error::Scan(ScanError::NotInstalled { .. })), Ok(devs_32)) => {
                        Ok(TwainList {
                            devices: devs_32,
                            other_failed: None,
                        })
                    }
                    (Ok(devs_64), Err(err_32)) => Ok(TwainList {
                        devices: devs_64,
                        other_failed: Some((32, err_32)),
                    }),
                    (Err(err_64), Ok(devs_32)) => Ok(TwainList {
                        devices: devs_32,
                        other_failed: Some((64, err_64)),
                    }),
                    (Err(Error::Scan(ScanError::NotInstalled { .. })), Err(err_32)) => Err(err_32),
                    (Err(err_64), Err(_)) => Err(err_64),
                }
            }
        }
    }

    /// Opens `device`, as a listing gave it and the app stored it, within `deadline`, in a
    /// worker of its own: the worker for its [`DeviceInfo::driver`], which keeps the device
    /// until it's closed.
    ///
    /// A worker is started in a private directory of its own, made under the work directory
    /// ([`Client::work_dir`]): for a SANE device, the directory also holds a SANE configuration
    /// naming only the backend the stored id names (the text after `sane:` up to its first `:`;
    /// none where the id gives none), which SANE reads first (`SANE_CONFIG_DIR`), so the device
    /// opens without waiting on every backend's probe; then what a listing's worker reads, so
    /// the backend finds its own configuration as it did when it listed the device: the
    /// `SANE_CONFIG_DIR` the worker's command gives ([`WorkerCommand::env`]), else the app's,
    /// else SANE's own list (the directory itself, then the system's). The worker's backend is
    /// the one to refuse an id it doesn't form.
    ///
    /// Errors: [`Error::NoWorker`] where no worker is given for the driver; [`Error::Start`]
    /// where the worker can't be started (a program that isn't an absolute path among it), and
    /// [`Error::WorkDir`] where its directory can't be made (a work directory that isn't an
    /// absolute path among it); the worker's start's, as a call's ([`Error::Stopped`] where it
    /// says nothing within [`Timeouts::start`], [`Error::Crashed`], [`Error::Protocol`], its
    /// [`PROTOCOL`](crate::PROTOCOL) another's among them); [`Error::Unsendable`] for a stored
    /// device whose request is longer than a worker reads; and the backend's, as
    /// [`Error::Scan`]. A worker whose open failed is quit, the call waiting for its exit as
    /// [`Client::list`]'s does, or killed at once where it answered [`ScanError::TimedOut`]. A
    /// worker that hasn't answered by the deadline and [`Timeouts::grace`] is killed
    /// ([`Error::Stopped`]). The device opened must be of the stored one's driver API, and the
    /// stored device itself or another said renamed from the stored id
    /// ([`RemoteDevice::renamed_from`]); else [`Error::Protocol`].
    pub fn open(&self, device: &DeviceInfo, deadline: Duration) -> Result<RemoteDevice, Error> {
        let mut worker = self.start(device.driver, Some(&device.id))?;
        match worker.open(device, deadline) {
            Ok((info, renamed_from)) => Ok(RemoteDevice {
                worker,
                info,
                renamed_from,
            }),
            Err(error) => {
                if matches!(error, Error::Scan(ScanError::TimedOut)) {
                    worker.kill();
                } else {
                    let _ = worker.close();
                }
                Err(error)
            }
        }
    }

    /// Whether a worker command is registered for `driver`.
    fn has_worker(&self, driver: Driver) -> bool {
        self.workers.iter().any(|(given, _)| *given == driver)
    }

    /// A worker for `driver` started, and its `hello` taken: for a listing, or to open `opens`.
    fn start(&self, driver: Driver, opens: Option<&DeviceId>) -> Result<Worker, Error> {
        let command = self
            .workers
            .iter()
            .find(|(given, _)| *given == driver)
            .map(|(_, command)| command)
            .ok_or(Error::NoWorker(driver))?;
        let work_dir = self.work_dir.clone().unwrap_or_else(env::temp_dir);
        Worker::start(
            command,
            &work_dir,
            driver,
            opens,
            self.limits,
            self.timeouts,
        )
    }
}

/// A worker's program, its arguments, and the variables set on its process.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkerCommand {
    program: PathBuf,
    args: Vec<OsString>,
    env: Vec<(OsString, OsString)>,
}

impl WorkerCommand {
    /// The worker `program`, with no argument and no variable of its own: an absolute path, since
    /// it's never searched for (checked as a worker starts).
    pub fn new(program: impl Into<PathBuf>) -> WorkerCommand {
        WorkerCommand {
            program: program.into(),
            args: Vec::new(),
            env: Vec::new(),
        }
    }

    /// The command with `arg` after its arguments.
    #[must_use = "it returns the command so made, the one it was called on moved into it"]
    pub fn arg(mut self, arg: impl Into<OsString>) -> WorkerCommand {
        self.args.push(arg.into());
        self
    }

    /// The command with the variable `key` set to `value` on the worker's process, after the
    /// variables given before.
    ///
    /// A worker inherits the app's environment; its command's variables are set after it, in
    /// order, so one the app has, or one given twice, takes the value given last. A listing's
    /// SANE worker reads the `SANE_CONFIG_DIR` given here; one that opens a stored device has the
    /// client's set after the variables given here, its own directory naming the stored id's
    /// backend first, then the directories given here (the [crate's docs](crate), "The client").
    ///
    /// For what a worker's process reads as it starts, such as the system loader's search path
    /// that chooses the SANE library a worker loads (`LD_LIBRARY_PATH`, `DYLD_LIBRARY_PATH` on
    /// macOS), and the configuration directories of an app's own SANE (`SANE_CONFIG_DIR`), which
    /// no code in a worker may change once it runs. The client sets each as given, unchecked: the
    /// app's own choice, as its program is. A search path holds absolute entries alone, since the
    /// loader reads an empty or relative one from the working directory, as `sane-dl`'s
    /// `Sane::load` says.
    #[must_use = "it returns the command so made, the one it was called on moved into it"]
    pub fn env(mut self, key: impl Into<OsString>, value: impl Into<OsString>) -> WorkerCommand {
        self.env.push((key.into(), value.into()));
        self
    }

    /// Its program.
    pub fn program(&self) -> &Path {
        &self.program
    }

    /// Its arguments, in order.
    pub fn args(&self) -> impl Iterator<Item = &OsStr> {
        self.args.iter().map(OsString::as_os_str)
    }

    /// Its variables, each a key and its value, in the order given.
    pub fn envs(&self) -> impl Iterator<Item = (&OsStr, &OsStr)> {
        self.env
            .iter()
            .map(|(key, value)| (key.as_os_str(), value.as_os_str()))
    }
}

/// What a worker may send, each refused before it's read whole: the worker is untrusted, since
/// a driver's code runs in it.
///
/// Its fields are set on [`Limits::default`]'s.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Limits {
    /// The longest line, its `\n` aside: 1 MiB by default.
    pub line: usize,
    /// The largest page's bytes: 1 GiB by default, which holds a Letter page at 1200 dpi in
    /// 16-bit color and a Legal one at 600 dpi.
    pub page: u64,
}

impl Default for Limits {
    fn default() -> Limits {
        Limits {
            line: 1 << 20,
            page: 1 << 30,
        }
    }
}

/// How long a worker may be silent before it's killed, and how long it's given to end.
///
/// Silence is counted from the last line the client read, and only while the calling thread
/// waits on the worker: time it spends handing an event to the caller's full channel is the
/// caller's. Its fields are set on [`Timeouts::default`]'s.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Timeouts {
    /// For its `hello`, as it starts: 10 s by default.
    pub start: Duration,
    /// Within a batch, and for [`RemoteDevice::caps`]: 60 s by default, enough for a flatbed's
    /// lamp to warm up.
    pub silence: Duration,
    /// Within a batch once it's cancelled, counted from the later of the cancel and the last
    /// line, so the sheet in flight can finish while it sends: 10 s by default.
    pub cancel: Duration,
    /// Past a listing's or an open's deadline before the worker is killed; and for a worker to
    /// exit once it's asked to (its input kept open meanwhile, so a worker never takes its
    /// client for gone and aborts while its driver closes), or once its output has ended, before
    /// it's killed: 5 s by default.
    pub grace: Duration,
}

impl Default for Timeouts {
    fn default() -> Timeouts {
        Timeouts {
            start: Duration::from_secs(10),
            silence: Duration::from_secs(60),
            cancel: Duration::from_secs(10),
            grace: Duration::from_secs(5),
        }
    }
}

/// A device open in a worker of its own: the model's calls on a [`Device`](scan_device::Device),
/// with the client's [`Error`].
///
/// One call at a time (`&mut self`); `Send`, so it can be opened on one thread and scan on
/// another. Closed by [`RemoteDevice::close`], or as it's dropped, which does the same and drops
/// its errors.
pub struct RemoteDevice {
    worker: Worker,
    info: DeviceInfo,
    renamed_from: Option<DeviceId>,
}

impl fmt::Debug for RemoteDevice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RemoteDevice")
            .field("info", &self.info)
            .field("renamed_from", &self.renamed_from)
            .field("process_id", &self.process_id())
            .finish_non_exhaustive()
    }
}

impl RemoteDevice {
    /// The device as the worker's backend found it.
    pub fn info(&self) -> &DeviceInfo {
        &self.info
    }

    /// The stored id, where the backend opened another device in its place (the same scanner
    /// under a new name); `None` where it opened the stored one.
    pub fn renamed_from(&self) -> Option<&DeviceId> {
        self.renamed_from.as_ref()
    }

    /// The worker's process id, for an app's log.
    pub fn process_id(&self) -> u32 {
        self.worker.process_id()
    }

    /// A [`StopHandle`] that can terminate this device's worker from another thread while a call
    /// (such as an untimed [`RemoteDevice::settings_window`] or a hung batch) is in flight.
    pub fn stop_handle(&self) -> StopHandle {
        self.worker.stop_handle()
    }

    /// What the device can do, per source.
    ///
    /// The worker's answer is checked as the model's rules have it: at least one source, each
    /// with at least one format, a list of resolutions not empty and holding no 0, a range's
    /// `min` above 0 and not past its `max`, and a largest paper finite and above 0; caps that
    /// break one are [`Error::Protocol`]. A worker silent past [`Timeouts::silence`] is killed
    /// ([`Error::Stopped`]).
    pub fn caps(&mut self) -> Result<Caps, Error> {
        self.worker.caps()
    }

    /// Scans a batch with `settings`, numbering its sheets on from `first_sheet`, and blocks until
    /// it ends, as [`Device::scan`](scan_device::Device::scan) does: each event sent on `events`
    /// as it comes, each page whole, and each page's progress counted on `progress` in its bytes,
    /// the driver's messages with it.
    ///
    /// Each page's bytes are capped ([`Limits::page`]) and checked against its format and size
    /// before they're read, and a raw page is made through
    /// [`RawImage::new`](scan_device::RawImage::new), which checks them again; its `dpi` must be
    /// finite and above 0. The events must keep the model's order: the settings applied once,
    /// before the first sheet, their paper finite and above 0 and each setting said changed with
    /// what was asked; the sheets numbered on from `first_sheet`; each page of the sheet begun
    /// last, a side of it once, raw or encoded in a form `settings` asked for; and
    /// [`ScanError::FeederEmpty`] only before any page. The pages before any failure are the
    /// caller's already.
    ///
    /// `progress` is read for a cancel every 50 ms and at each message, and a cancel is sent to
    /// the worker once: the worker finishes the sheet in flight, front and back, feeds no more,
    /// and the batch ends [`Done::Cancelled`]. A `send` on `events` that fails, the receiver
    /// gone, is a cancel too: the rest of the batch is read and dropped, and the batch ends
    /// [`Done::Cancelled`], whatever the worker's end, with the worker in step for the next call;
    /// unlike a backend in process, the worker finishes the sheet in flight first.
    ///
    /// Errors: the backend's as [`Error::Scan`], the worker going on; [`Error::Unsendable`] for
    /// settings that can't cross (a paper size that isn't finite, or encodings past the
    /// protocol's 1 MiB line), nothing sent; and the worker's failures, each ending it: silent
    /// past [`Timeouts::silence`], or once cancelled past [`Timeouts::cancel`] counted from the
    /// later of the cancel and its last line ([`Error::Stopped`]); its output ending, or its
    /// process exiting ([`Error::Crashed`], a page cut short dropped and said); what the protocol
    /// or the model refuses ([`Error::Protocol`]).
    pub fn scan(
        &mut self,
        settings: &Settings,
        first_sheet: u32,
        events: &SyncSender<Event>,
        progress: &dyn Progress,
    ) -> Result<Done, Error> {
        self.worker.scan(settings, first_sheet, events, progress)
    }

    /// Opens the driver's own settings window, owned by `owner`'s window where it gives one, and
    /// answers how it closed. The worker is never timed while it's open: it's the user's.
    ///
    /// Only the window's closing, [`StopHandle::stop`] from another thread ([`Error::Aborted`]),
    /// or the worker's end ([`Error::Crashed`]) ends the call. On Windows, passing a live `HWND`
    /// owned by the calling thread is refused before waiting on the worker
    /// ([`ScanError::Driver`]), since blocking the window's own thread would deadlock synchronous
    /// Win32 owner messages, and an enabled cross-thread `HWND` left disabled by a failed or
    /// aborted modal driver is re-enabled before this call returns.
    pub fn settings_window(&mut self, owner: Option<WindowOwner>) -> Result<WindowClosed, Error> {
        self.worker.settings_window(owner)
    }

    /// Closes the device: the worker is asked to quit, which closes its driver, given
    /// [`Timeouts::grace`] to exit, and killed if it hasn't; its directory is removed once it
    /// has exited.
    ///
    /// Its input is kept open while it exits: a worker takes its input's end for its client gone,
    /// and aborts 10 s later if it hasn't finished, so a grace longer than that still lets a slow
    /// driver close.
    ///
    /// Errors: a worker that exits with a failure, [`Error::Crashed`]; one killed past the grace,
    /// [`Error::Stopped`] ([`Timeout::Exit`](crate::Timeout::Exit)). A worker an earlier failure
    /// or [`StopHandle::stop`] ended is closed already, so `Ok`. A worker that can't be reaped
    /// even once killed (a process stuck in the system) is left to a thread that waits for it and
    /// then removes its directory, so the caller is held at most twice the grace, and a moment
    /// more for the end of its standard error.
    pub fn close(mut self) -> Result<(), Error> {
        self.worker.close()
    }
}

/// Orders `devs_64` before `devs_32`, dropping any 32-bit entry whose `(maker, model, name)`
/// already appears among `devs_64`.
fn merge_twain_devices(devs_64: Vec<DeviceInfo>, devs_32: Vec<DeviceInfo>) -> Vec<DeviceInfo> {
    let mut merged = devs_64;
    let n64 = merged.len();
    for dev in devs_32 {
        let dup = merged[..n64]
            .iter()
            .any(|d64| d64.maker == dev.maker && d64.model == dev.model && d64.name == dev.name);
        if !dup {
            merged.push(dev);
        }
    }
    merged
}

#[cfg(test)]
mod tests {
    use super::*;

    fn twain_dev(bits: u8, product: &str, maker: &str, name: &str) -> DeviceInfo {
        let prefix = if bits == 32 { "twain32:" } else { "twain64:" };
        DeviceInfo::new(
            DeviceId(format!("{prefix}{product}")),
            name.to_owned(),
            maker.to_owned(),
            product.to_owned(),
            Driver::Twain { bits },
        )
    }

    #[test]
    fn merge_twain_devices_prefers_64_bit_and_preserves_32_bit_only_sources() {
        let devs_64 = vec![
            twain_dev(
                64,
                "TWAIN2 Software Scanner",
                "TWAIN Working Group",
                "TWAIN Working Group TWAIN2 Software Scanner",
            ),
            twain_dev(64, "fi-8170", "PFU", "PFU fi-8170"),
        ];
        let devs_32 = vec![
            twain_dev(
                32,
                "TWAIN2 Software Scanner",
                "TWAIN Working Group",
                "TWAIN Working Group TWAIN2 Software Scanner",
            ),
            twain_dev(
                32,
                "PaperStream IP fi-6130Zdj",
                "FUJITSU",
                "FUJITSU PaperStream IP fi-6130Zdj",
            ),
        ];
        let merged = merge_twain_devices(devs_64, devs_32);
        let ids: Vec<&str> = merged.iter().map(|d| d.id.0.as_str()).collect();
        assert_eq!(
            ids,
            [
                "twain64:TWAIN2 Software Scanner",
                "twain64:fi-8170",
                "twain32:PaperStream IP fi-6130Zdj",
            ]
        );
    }
}
