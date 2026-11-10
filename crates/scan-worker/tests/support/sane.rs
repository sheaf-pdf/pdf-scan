//! What the SANE worker's tests share: the test worker's `sane` scenario given the fake libsane by
//! its command's variables, the fake's script and log, configurations of the test's own for
//! SANE's real backends, a skip where there's no libsane, and the fake's bytes.
//!
//! Follows `scan-device`'s `tests/sane_support/mod.rs` in this repository, itself after `sane-dl`'s
//! `tests/common/mod.rs`, adapted: SANE is loaded by the worker, never by the test, so there's no
//! child process of the test's own, and the test writes its files and builds the fake itself.
//! What a worker loading SANE needs is set on its command ([`WorkerCommand::env`]): the loader's
//! search path naming the fake's directory first, the fake's script and log, and the
//! `SANE_CONFIG_DIR` a listing's worker reads, an open's worker being given the client's in its
//! place. A test that needs libsane says it skipped where the worker answers that SANE isn't
//! installed, on the standard error the harness doesn't capture, or fails where
//! `SANE_DL_REQUIRE_LIBSANE` is `1`.
//!
//! The fake is found as SANE is: copied under SANE's own file name into a directory of its own
//! ([`fake_library_dir`]), which the worker's loader searches first (`LD_LIBRARY_PATH`;
//! `DYLD_LIBRARY_PATH` on macOS). Windows has no default library, so no route to the fake: each
//! test that loads it is ignored there, saying why.

use std::ffi::OsString;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

use sane_dl::{CONFIG_DIR_VARIABLE, ConfigDir, Sane, Search};
use scan_worker::scan_device::{Driver, ScanError};
use scan_worker::{Client, Error, WorkerCommand};

/// The variable naming the directories the system loader searches first for a library loaded by
/// name, read as the process starts.
#[cfg(target_os = "macos")]
const LOADER_PATH: &str = "DYLD_LIBRARY_PATH";
#[cfg(not(target_os = "macos"))]
const LOADER_PATH: &str = "LD_LIBRARY_PATH";

/// The variable whose `1` makes a test that needs libsane fail rather than skip where it isn't
/// there: the one `sane-dl`'s tests read, which CI's Ubuntu job sets.
pub const REQUIRE_LIBSANE: &str = "SANE_DL_REQUIRE_LIBSANE";

/// The variable naming the directory the test worker's `sane` scenario takes for the system's
/// SANE configuration, in an open's worker.
pub const SYSTEM: &str = "SCAN_WORKER_TEST_SANE_SYSTEM";

/// Whether `variable` is `1`.
fn required(variable: &str) -> bool {
    std::env::var_os(variable).is_some_and(|value| value == "1")
}

/// `result`, or `None` where it says SANE isn't installed, as a worker loading the system's SANE
/// does where there's no libsane: then the test says it skipped, on the standard error the
/// harness doesn't capture, or fails if `SANE_DL_REQUIRE_LIBSANE` is `1`.
pub fn installed<T>(test: &str, result: Result<T, Error>) -> Option<Result<T, Error>> {
    match result {
        Err(Error::Scan(ScanError::NotInstalled { what })) => {
            let reason = format!("{what} isn't installed here");
            assert!(
                !required(REQUIRE_LIBSANE),
                "{test} can't run here ({reason}), and {REQUIRE_LIBSANE}=1 says it must"
            );
            let _ = writeln!(io::stderr(), "{test}: skipped: {reason}");
            None
        }
        other => Some(other),
    }
}

/// The directory for one test's files, under the tests' temporary directory, made afresh.
pub fn fresh_dir(test: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("scan-worker")
        .join(env!("CARGO_CRATE_NAME"))
        .join(test);
    match fs::remove_dir_all(&dir) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => panic!("couldn't empty {}: {error}", dir.display()),
    }
    fs::create_dir_all(&dir).expect("couldn't make the test's directory");
    dir
}

/// A client whose SANE worker is `command`, making its workers' directories under `dir`.
pub fn sane_client(command: WorkerCommand, dir: &Path) -> Client {
    let work = dir.join("work");
    fs::create_dir(&work).expect("couldn't make the work directory");
    Client::new().worker(Driver::Sane, command).work_dir(work)
}

/// The test worker's `sane` scenario, with `args` after it.
pub fn sane_worker(args: &[&str]) -> WorkerCommand {
    args.iter()
        .fold(super::worker("sane"), |command, &arg| command.arg(arg))
}

/// The directory the fake libsane is loaded from as SANE, holding a copy of it under SANE's own
/// file name alone, one directory per test binary.
///
/// The fake is built once per test binary by the cargo that built the tests, into a target
/// directory of its own, without the flags or target of the run that built them (a sanitizer's or
/// a cross build's). Then it's copied as the file name of [`Sane::DEFAULT_LIBRARIES`]' first entry
/// (`libsane.so.1`; `libsane.1.dylib` on macOS), written under a temporary name holding this
/// process's id and renamed into place, so no worker maps half a copy, and a copy a running worker
/// maps is replaced, never written over.
pub fn fake_library_dir() -> &'static Path {
    static FAKE: OnceLock<PathBuf> = OnceLock::new();
    FAKE.get_or_init(|| {
        let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..");
        let target = Path::new(env!("CARGO_TARGET_TMPDIR")).join("fake-libsane");
        let output = Command::new(env!("CARGO"))
            .args([
                "build",
                "--locked",
                "--quiet",
                "-p",
                "fake-libsane",
                "--target-dir",
            ])
            .arg(&target)
            .current_dir(&workspace)
            .env_remove("RUSTFLAGS")
            .env_remove("CARGO_ENCODED_RUSTFLAGS")
            .env_remove("CARGO_BUILD_RUSTFLAGS")
            .env_remove("CARGO_BUILD_TARGET")
            .output()
            .expect("couldn't run cargo to build the fake libsane");
        assert!(
            output.status.success(),
            "couldn't build the fake libsane:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let name = format!(
            "{}fake_libsane{}",
            std::env::consts::DLL_PREFIX,
            std::env::consts::DLL_SUFFIX
        );
        let built = target.join("debug").join(name);
        assert!(
            built.is_file(),
            "the fake libsane isn't at {}",
            built.display()
        );
        let sanes_name = Sane::DEFAULT_LIBRARIES
            .first()
            .and_then(|default| Path::new(default).file_name())
            .expect("this system has no default library to stand in for");
        let dir = target
            .join("as-sane")
            .join(concat!("scan-worker-", env!("CARGO_CRATE_NAME")));
        fs::create_dir_all(&dir).expect("couldn't make the fake's directory");
        let mut temporary = OsString::from(".");
        temporary.push(sanes_name);
        temporary.push(format!(".{}.tmp", std::process::id()));
        let temporary = dir.join(temporary);
        if let Err(error) = fs::copy(&built, &temporary) {
            let _ = fs::remove_file(&temporary);
            panic!(
                "couldn't copy the fake libsane to {}: {error}",
                temporary.display()
            );
        }
        let copy = dir.join(sanes_name);
        if let Err(error) = fs::rename(&temporary, &copy) {
            let _ = fs::remove_file(&temporary);
            panic!(
                "couldn't rename the fake's copy to {}: {error}",
                copy.display()
            );
        }
        dir
    })
}

/// The loader's search path for a worker that loads the fake: `dir` first, then whatever this
/// process was given. An empty entry is left out, since the loader would read it as the working
/// directory.
fn loader_path(dir: &Path) -> OsString {
    let given = std::env::var_os(LOADER_PATH).unwrap_or_default();
    let given = std::env::split_paths(&given).filter(|entry| !entry.as_os_str().is_empty());
    std::env::join_paths(std::iter::once(dir.to_owned()).chain(given))
        .expect("the fake's directory can't be put on the loader's search path")
}

/// One test's run of the fake: its directory, script and log.
pub struct Fake {
    pub dir: PathBuf,
    script: PathBuf,
    log: PathBuf,
}

impl Fake {
    /// A test's run of the fake with `script`: the test's directory made afresh, the script written
    /// there, and the fake built and copied, if it hasn't been yet.
    pub fn new(test: &str, script: &str) -> Fake {
        let dir = fresh_dir(test);
        let script_path = dir.join("script");
        fs::write(&script_path, script).expect("couldn't write the fake's script");
        fake_library_dir();
        Fake {
            log: dir.join("log"),
            script: script_path,
            dir,
        }
    }

    /// `command`, a worker that loads SANE, given the fake as SANE: the fake's directory first on
    /// its loader's search path, its script and log, and as its `SANE_CONFIG_DIR` the test's
    /// directory, which names no backend, so a listing that reached the system's libsane by
    /// mistake would read none of the system's configuration.
    pub fn command(&self, command: WorkerCommand) -> WorkerCommand {
        command
            .env(LOADER_PATH, loader_path(fake_library_dir()))
            .env("FAKE_LIBSANE_SCRIPT", &self.script)
            .env("FAKE_LIBSANE_LOG", &self.log)
            .env(CONFIG_DIR_VARIABLE, &self.dir)
    }

    /// A client whose SANE worker is the `sane` scenario with `args`, given the fake.
    pub fn client(&self, args: &[&str]) -> Client {
        sane_client(self.command(sane_worker(args)), &self.dir)
    }

    /// The log, as the workers left it: each that loaded the fake appends to it.
    pub fn log(&self) -> FakeLog {
        let text = fs::read_to_string(&self.log).unwrap_or_default();
        let lines = text
            .lines()
            .map(|line| {
                let mut words = line.splitn(4, ' ');
                let mut next = || words.next().unwrap_or("").to_owned();
                LogLine {
                    kind: next(),
                    thread: next(),
                    function: next(),
                    rest: next(),
                }
            })
            .collect();
        FakeLog { lines }
    }
}

/// A line of the fake's log.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LogLine {
    /// `call`, `return`, `violation` or `note`.
    pub kind: String,
    pub thread: String,
    pub function: String,
    pub rest: String,
}

/// The fake's log.
#[derive(Debug)]
pub struct FakeLog {
    pub lines: Vec<LogLine>,
}

impl FakeLog {
    /// Fails if a line says a rule was broken.
    pub fn assert_no_violations(&self) {
        let broken: Vec<&LogLine> = self
            .lines
            .iter()
            .filter(|line| line.kind == "violation")
            .collect();
        assert!(
            broken.is_empty(),
            "the fake saw SANE's rules broken: {broken:#?}"
        );
    }

    /// The functions called, in order.
    pub fn calls(&self) -> Vec<&str> {
        self.lines
            .iter()
            .filter(|line| line.kind == "call")
            .map(|line| line.function.as_str())
            .collect()
    }

    /// The functions called, in order, but the option calls (opening a device reads its
    /// descriptors).
    pub fn calls_but_options(&self) -> Vec<&str> {
        self.calls()
            .into_iter()
            .filter(|function| {
                !matches!(
                    *function,
                    "sane_get_option_descriptor" | "sane_control_option"
                )
            })
            .collect()
    }

    /// The details of each call of `function`, in order.
    pub fn calls_of(&self, function: &str) -> Vec<&str> {
        self.lines
            .iter()
            .filter(|line| line.kind == "call" && line.function == function)
            .map(|line| line.rest.as_str())
            .collect()
    }

    /// The values each set of an option gave, in order: `sane_control_option`'s `value=…` for
    /// every `action=SET`.
    pub fn sets(&self) -> Vec<&str> {
        self.calls_of("sane_control_option")
            .into_iter()
            .filter(|rest| rest.contains(" action=SET"))
            .filter_map(|rest| rest.split_once(" value=").map(|(_, value)| value))
            .collect()
    }
}

/// A worker's command for SANE's real backends, its configuration the test's own, in `dir`: a
/// listing's worker reads a directory naming `backends` alone, holding `files` (a backend's own
/// configuration, such as `test.conf`), each a name and its text; an open's worker, given the
/// client's directory naming the stored id's backend, reads it then a directory standing for the
/// system's, holding `stand_in`'s files (`dll.aliases`, a backend's `.conf`), and never the
/// system's own, through the `sane` scenario's stand-in. The scenario is given `args`.
pub fn real_command(
    dir: &Path,
    backends: &[&str],
    files: &[(&str, &str)],
    stand_in: &[(&str, &str)],
    args: &[&str],
) -> WorkerCommand {
    let listing = dir.join("listing");
    let system = dir.join("stand-in");
    for made in [&listing, &system] {
        fs::create_dir(made).expect("couldn't make a configuration directory");
    }
    write_files(&listing, files);
    write_files(&system, stand_in);
    let listing = ConfigDir::write(&listing, backends, Search::Isolated)
        .expect("couldn't write the configuration");
    sane_worker(args)
        .env(CONFIG_DIR_VARIABLE, listing.value())
        .env(SYSTEM, &system)
}

fn write_files(dir: &Path, files: &[(&str, &str)]) {
    for (name, text) in files {
        fs::write(dir.join(name), text).expect("couldn't write a configuration file");
    }
}

/// The fake's bytes of a frame, its padding dropped, as its crate docs say it sends them: page
/// `page` (from 1, counted since the device opened), a back if `back`, frame `frame` (from 0),
/// `rows` rows of `pixel_bytes` bytes. As `sane-dl`'s helper has it.
pub fn fake_pixels(
    page: usize,
    back: bool,
    frame: usize,
    pixel_bytes: usize,
    rows: usize,
) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(rows * pixel_bytes);
    for row in 0..rows {
        for column in 0..pixel_bytes {
            bytes.push(if row == 0 && column == 0 {
                if back { b'B' } else { b'F' }
            } else {
                ((page * 31 + frame * 7 + row * 3 + column) % 251) as u8
            });
        }
    }
    bytes
}
