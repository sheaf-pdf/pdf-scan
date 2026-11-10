//! What the SANE backend's tests share: a child process per test, its skip code, the fake
//! libsane built and found, its script and log, and SANE configurations of the test's own.
//!
//! Follows `sane-dl`'s `tests/common/mod.rs` in this repository, adapted: the backend loads SANE
//! itself, so a child checks the fake's log is there once it has ([`assert_fake_loaded`]) rather
//! than loading it, and skips where the backend says SANE isn't installed ([`installed`]); and a
//! configuration may be a directory naming backends, then one standing for the system's
//! ([`stand_in_child`]).
//!
//! SANE is started once per process and reads `SANE_CONFIG_DIR` once, so each test that loads it
//! runs its body in a child process of its own (rusty-fork), its environment given at spawn and a
//! timeout on it. The child's output is captured into a file the harness shows only on a failure,
//! so a child that can't run here exits with [`SKIP`] after saying why, and the parent says it
//! again on its own standard error, which the harness doesn't capture, or fails if
//! `SANE_DL_REQUIRE_LIBSANE` is `1`.
//!
//! rusty-fork's child runs the test function again from its top, until it reaches the fork, so
//! whatever a test does before [`in_child!`] runs in the child too. Building the fake and writing
//! a test's files is the parent's alone: a [`Child`] carries that work as its preparation, which
//! runs as the child is spawned, in the parent only, and the helpers that do it refuse to run in
//! a child.
//!
//! The fake is found as SANE is: its parent copies it under SANE's own file name into a directory
//! of its own ([`fake_library_dir`]), and spawns the child with the system loader's search path
//! naming that directory first (`LD_LIBRARY_PATH`; `DYLD_LIBRARY_PATH` on macOS). Windows has no
//! default library, so no route to the fake: each test that loads it is ignored there, saying why.
//!
//! For batches: a batch run with its events taken on a thread of their own ([`batch_with`]) and
//! what they were; the fake's bytes as its crate docs give them ([`fake_pixels`], as `sane-dl`'s
//! helper has it); and a progress that records what a batch counts on it, paced for SANE's `test`
//! backend as `sane-dl`'s `tests/scan_real.rs` paces it where it's made so ([`Recorder`]).

// Each test binary uses some of these.
#![allow(dead_code)]

use std::ffi::{OsStr, OsString};
use std::fmt::Display;
use std::fs;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, OnceLock, PoisonError, mpsc};
use std::thread;
use std::time::Duration;

use sane_dl::{CONFIG_DIR_VARIABLE, ConfigDir, Sane, Search};
use scan_device::in_progress::{Level, Progress, Tracker, Unit};
use scan_device::{
    Applied, Device, Done, Event, Format, Pixels, RawPage, ScanError, Settings, Source,
};

/// The variable naming the directories the system loader searches first for a library loaded by
/// name, read as the process starts.
#[cfg(target_os = "macos")]
const LOADER_PATH: &str = "DYLD_LIBRARY_PATH";
#[cfg(not(target_os = "macos"))]
const LOADER_PATH: &str = "LD_LIBRARY_PATH";

/// The exit code of a child that can't run here.
pub const SKIP: i32 = 77;

/// How the child's line saying why it skipped starts.
const SKIP_MARK: &str = "scan-device-test-skip: ";

/// The variable whose `1` makes a test that needs libsane fail rather than skip where it isn't
/// there: the one `sane-dl`'s tests read, which CI's Ubuntu job sets.
pub const REQUIRE_LIBSANE: &str = "SANE_DL_REQUIRE_LIBSANE";

/// The variable set in every child, so the parent's work can refuse to run there.
const IN_CHILD: &str = "SCAN_DEVICE_TEST_CHILD";

/// Fails the test if this is a child: `what` is the parent's alone.
fn parent_only(what: &str) {
    assert!(
        std::env::var_os(IN_CHILD).is_none(),
        "{what} is the parent's work, and ran in the child: put it in the child's preparation"
    );
}

/// Runs a test's body in a child process: `in_child!(test_fn, child, || body)`.
#[allow(unused_macros)]
macro_rules! in_child {
    ($test:ident, $child:expr, $body:expr) => {
        $child.run(
            rusty_fork::rusty_fork_test_name!($test),
            rusty_fork::rusty_fork_id!(),
            $body,
        )
    };
}

/// Whether `variable` is `1`.
pub fn required(variable: &str) -> bool {
    std::env::var_os(variable).is_some_and(|value| value == "1")
}

/// Says why a test can't run here, on the standard error the harness doesn't capture, or fails
/// if `require` is `1`.
pub fn report_skip(test: &str, reason: &str, require: Option<&str>) {
    if let Some(variable) = require
        && required(variable)
    {
        panic!("{test} can't run here ({reason}), and {variable}=1 says it must");
    }
    let _ = writeln!(io::stderr(), "{test}: skipped: {reason}");
}

/// In the child: says why the test can't run here, and exits with [`SKIP`].
pub fn skip(reason: impl Display) -> ! {
    println!("{SKIP_MARK}{reason}");
    let _ = io::stdout().flush();
    std::process::exit(SKIP);
}

/// In the child: `result`'s value, or a skip where it says SANE isn't installed, as a call that
/// loads the system's SANE does where there's no libsane.
pub fn installed<T>(result: Result<T, ScanError>) -> Result<T, ScanError> {
    match result {
        Err(ScanError::NotInstalled { what }) => skip(format!("{what} isn't installed here")),
        other => other,
    }
}

/// Work for the parent alone, run as each child is spawned.
type Preparation = Box<dyn FnMut(&mut Command)>;

/// A child process to run a test's body in.
pub struct Child {
    env: Vec<(OsString, OsString)>,
    prepare: Vec<Preparation>,
    timeout: Duration,
    require: Option<&'static str>,
    attempts: u32,
}

impl Default for Child {
    fn default() -> Child {
        Child {
            env: Vec::new(),
            prepare: Vec::new(),
            timeout: Duration::from_secs(60),
            require: None,
            attempts: 1,
        }
    }
}

impl Child {
    pub fn new() -> Child {
        Child::default()
    }

    /// Sets a variable in the child's environment.
    pub fn env(mut self, key: impl AsRef<OsStr>, value: impl AsRef<OsStr>) -> Child {
        self.env
            .push((key.as_ref().to_owned(), value.as_ref().to_owned()));
        self
    }

    /// Work for the parent alone (files to write, the fake to build), run as each child is
    /// spawned, given the child's command to set variables on. Never run in the child.
    pub fn prepare(mut self, work: impl FnMut(&mut Command) + 'static) -> Child {
        self.prepare.push(Box::new(work));
        self
    }

    /// How long the child may run before it's killed and the test fails.
    pub fn timeout(mut self, timeout: Duration) -> Child {
        self.timeout = timeout;
        self
    }

    /// The variable whose `1` turns the child's skip into a failure.
    pub fn require(mut self, variable: &'static str) -> Child {
        self.require = Some(variable);
        self
    }

    /// Starts the child again where it runs past its timeout, up to `attempts` times in all,
    /// each hang said on the standard error: for a test that meets a hang in the library it
    /// drives, not its own, which it can't keep clear of. Any other failure fails the test at
    /// once, and a hang at the last attempt fails it as any child's does.
    pub fn retry_hangs(mut self, attempts: u32) -> Child {
        self.attempts = attempts.max(1);
        self
    }

    /// Runs `body` in a child process, through [`in_child!`]. False where the child skipped
    /// ([`skip`]), so a parent with checks of its own after the child leaves them out.
    pub fn run(self, test: &str, id: rusty_fork::RustyForkId, body: impl FnOnce()) -> bool {
        let Child {
            env,
            mut prepare,
            timeout,
            require,
            attempts,
        } = self;
        // rusty-fork calls this in the parent alone, just before it spawns each child.
        let mut configure = |command: &mut Command| {
            for work in &mut prepare {
                work(command);
            }
            for (key, value) in &env {
                command.env(key, value);
            }
            command.env(IN_CHILD, "1");
        };
        let supervise = |child: &mut rusty_fork::ChildWrapper, output: &mut fs::File| {
            let status = child
                .wait_timeout(timeout)
                .expect("couldn't wait for the child");
            // A child still running is killed as `fork` returns.
            let status = status?;
            if status.code() == Some(SKIP) {
                let mut text = String::new();
                let _ = output.seek(SeekFrom::Start(0));
                let _ = output.read_to_string(&mut text);
                let reason = text
                    .lines()
                    .find_map(|line| line.strip_prefix(SKIP_MARK))
                    .unwrap_or("it gave no reason");
                report_skip(test, reason, require);
                return Some(false);
            }
            assert!(
                status.success(),
                "{test}: the child failed ({status}); its output is above"
            );
            Some(true)
        };
        // The child runs the body as its first `fork` is called there; the parent never runs it.
        let mut body = Some(body);
        for attempt in 1..=attempts {
            let once = body.take();
            let ran = rusty_fork::fork(test, &id, &mut configure, supervise, move || {
                once.expect("a child runs its test's body once")();
            })
            .expect("couldn't start the child");
            match ran {
                Some(ran) => return ran,
                None if attempt < attempts => {
                    let _ = writeln!(
                        io::stderr(),
                        "{test}: the child ran past its timeout of {timeout:?} (attempt {attempt} of {attempts}); started again"
                    );
                }
                None => {}
            }
        }
        panic!(
            "{test}: the child ran past its timeout of {timeout:?}, at each of {attempts} attempts"
        )
    }
}

/// The directory for one test's files, under the tests' temporary directory. Only its path:
/// [`fresh_dir`] makes it.
pub fn test_dir(test: &str) -> PathBuf {
    Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("scan-device")
        .join(env!("CARGO_CRATE_NAME"))
        .join(test)
}

/// Makes `dir`, emptied first. In the parent only.
pub fn fresh_dir(dir: &Path) {
    parent_only("emptying a test's directory");
    match fs::remove_dir_all(dir) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => panic!("couldn't empty {}: {error}", dir.display()),
    }
    fs::create_dir_all(dir).expect("couldn't make the test's directory");
}

/// The directory the fake libsane is loaded from as SANE, holding a copy of it under SANE's own
/// file name alone, one directory per test binary.
///
/// The fake is built once per test binary by the cargo that built the tests, into a target
/// directory of its own, without the flags or target of the run that built them (a sanitizer's or
/// a cross build's). Then it's copied as the file name of [`Sane::DEFAULT_LIBRARIES`]' first entry
/// (`libsane.so.1`; `libsane.1.dylib` on macOS), written under a temporary name holding this
/// process's id and renamed into place, so no child maps half a copy, and a copy a running child
/// maps is replaced, never written over. A child whose loader searches this directory first loads
/// the fake as SANE. In the parent only, as a child's preparation.
pub fn fake_library_dir() -> &'static Path {
    parent_only("building the fake libsane and its directory");
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
            .join(concat!("scan-device-", env!("CARGO_CRATE_NAME")));
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

/// The loader's search path for a child that loads the fake: `dir` first, then whatever this
/// process was given. An empty entry is left out, since the loader would read it as the working
/// directory.
fn loader_path(dir: &Path) -> OsString {
    let given = std::env::var_os(LOADER_PATH).unwrap_or_default();
    let given = std::env::split_paths(&given).filter(|entry| !entry.as_os_str().is_empty());
    std::env::join_paths(std::iter::once(dir.to_owned()).chain(given))
        .expect("the fake's directory can't be put on the loader's search path")
}

/// One test's run of the fake: its script and log, in the test's directory.
pub struct Fake {
    dir: PathBuf,
    text: String,
    script: PathBuf,
    log: PathBuf,
}

impl Fake {
    /// A test's run of the fake with `script`. Nothing is written yet: the child's preparation
    /// writes it.
    pub fn new(test: &str, script: &str) -> Fake {
        let dir = test_dir(test);
        Fake {
            script: dir.join("script"),
            log: dir.join("log"),
            text: script.to_owned(),
            dir,
        }
    }

    /// A child that loads the fake with this script and log; as it's spawned, the parent makes
    /// the test's directory afresh, writes the script there, builds and copies the fake, if it
    /// hasn't yet, and puts the fake's directory first on the child's loader search path.
    ///
    /// The child's `SANE_CONFIG_DIR` is the test's directory too, which names no backend, so a
    /// load that reached the system's libsane by mistake would read none of the system's
    /// configuration before [`assert_fake_loaded`] fails it.
    pub fn child(&self) -> Child {
        let (dir, script, text) = (self.dir.clone(), self.script.clone(), self.text.clone());
        Child::new()
            .prepare(move |command| {
                fresh_dir(&dir);
                fs::write(&script, &text).expect("couldn't write the fake's script");
                command.env(LOADER_PATH, loader_path(fake_library_dir()));
            })
            .env(CONFIG_DIR_VARIABLE, &self.dir)
            .env("FAKE_LIBSANE_SCRIPT", &self.script)
            .env("FAKE_LIBSANE_LOG", &self.log)
    }

    /// The log, as the child left it.
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

    /// Whether the log is there: the fake writes it at its first call.
    pub fn logged(&self) -> bool {
        self.log.is_file()
    }
}

/// In the child, once the backend has loaded SANE: fails the test unless the fake's log is there,
/// which its first call writes, so a test that loaded the system's libsane by mistake fails
/// rather than passing on it.
pub fn assert_fake_loaded() {
    let log = std::env::var_os("FAKE_LIBSANE_LOG").expect("the parent names the fake's log");
    assert!(
        Path::new(&log).is_file(),
        "the fake's log isn't at {}, so what the backend loaded wasn't the fake: is the fake's \
         directory first on {LOADER_PATH}?",
        Path::new(&log).display()
    );
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

    /// The details of each return of `function`, in order.
    pub fn returns_of(&self, function: &str) -> Vec<&str> {
        self.lines
            .iter()
            .filter(|line| line.kind == "return" && line.function == function)
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

    /// Each set of an option, in order: the option's number and the value given, as the log
    /// shows it (a string quoted, numbers as `[n]`).
    pub fn option_sets(&self) -> Vec<(usize, &str)> {
        self.calls_of("sane_control_option")
            .into_iter()
            .filter(|rest| rest.contains(" action=SET"))
            .filter_map(|rest| {
                let (_, option) = rest.split_once("option=")?;
                let (number, _) = option.split_once(' ')?;
                let (_, value) = rest.split_once(" value=")?;
                Some((number.parse().ok()?, value))
            })
            .collect()
    }

    /// Where, among the log's lines, each `call` of `function` is.
    pub fn positions_of(&self, function: &str) -> Vec<usize> {
        self.lines
            .iter()
            .enumerate()
            .filter(|(_, line)| line.kind == "call" && line.function == function)
            .map(|(at, _)| at)
            .collect()
    }

    /// Where, among the log's lines, each set of option `option` is.
    pub fn positions_of_sets(&self, option: usize) -> Vec<usize> {
        let wanted = format!("option={option} action=SET");
        self.lines
            .iter()
            .enumerate()
            .filter(|(_, line)| {
                line.kind == "call"
                    && line.function == "sane_control_option"
                    && line.rest.contains(&wanted)
            })
            .map(|(at, _)| at)
            .collect()
    }

    /// How many reads answered `EOF`: a frame read to its end each.
    pub fn frames_ended(&self) -> usize {
        self.returns_of("sane_read")
            .into_iter()
            .filter(|rest| rest.starts_with("status=EOF"))
            .count()
    }
}

/// A child for a test of the real libsane: its `SANE_CONFIG_DIR` an isolated configuration
/// naming `backends`, in the test's directory, never the system's, holding `files` too (a
/// backend's own configuration, such as `test.conf`), each a name and its text, written by the
/// parent as the child is spawned; a skip a failure where `SANE_DL_REQUIRE_LIBSANE=1`.
pub fn real_child(test: &str, backends: &[&str], files: &[(&str, &str)]) -> Child {
    let dir = test_dir(test);
    let backends: Vec<String> = backends.iter().map(|&name| name.to_owned()).collect();
    let files = owned(files);
    Child::new()
        .prepare(move |command| {
            fresh_dir(&dir);
            let config = dir.join("config");
            fs::create_dir(&config).expect("couldn't make the configuration directory");
            write_files(&config, &files);
            let config = ConfigDir::write(&config, &backends, Search::Isolated)
                .expect("couldn't write the configuration");
            command.env(CONFIG_DIR_VARIABLE, config.value());
        })
        .require(REQUIRE_LIBSANE)
}

/// A child for a test of the real libsane whose `SANE_CONFIG_DIR` is a directory naming
/// `backends` (none for an empty list), as a worker that opens a stored device is given, then a
/// directory standing for the system's, holding `stand_in`'s files (`dll.aliases`, a backend's
/// `.conf`), each a name and its text: `<one>:<stand-in>`, so SANE reads the first directory, then
/// the second, and never the system's own. Written by the parent as the child is spawned, in the
/// test's directory; a skip a failure where `SANE_DL_REQUIRE_LIBSANE=1`.
pub fn stand_in_child(test: &str, backends: &[&str], stand_in: &[(&str, &str)]) -> Child {
    let dir = test_dir(test);
    let backends: Vec<String> = backends.iter().map(|&name| name.to_owned()).collect();
    let files = owned(stand_in);
    Child::new()
        .prepare(move |command| {
            fresh_dir(&dir);
            let one = dir.join("one");
            let system = dir.join("stand-in");
            for made in [&one, &system] {
                fs::create_dir(made).expect("couldn't make a configuration directory");
            }
            write_files(&system, &files);
            let one = ConfigDir::write(&one, &backends, Search::Isolated)
                .expect("couldn't write the configuration");
            let mut value = one.value();
            value.push(":");
            value.push(&system);
            command.env(CONFIG_DIR_VARIABLE, value);
        })
        .require(REQUIRE_LIBSANE)
}

fn owned(files: &[(&str, &str)]) -> Vec<(String, String)> {
    files
        .iter()
        .map(|&(name, text)| (name.to_owned(), text.to_owned()))
        .collect()
}

fn write_files(dir: &Path, files: &[(String, String)]) {
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

/// What a batch counted on a [`Recorder`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Count {
    /// `set_total`, in bytes.
    Total(Option<u64>),
    /// `set`.
    Done(u64),
}

/// A progress that records each count a batch makes on it, and is cancelled through its tracker;
/// paced, where it's made so, as `sane-dl`'s `tests/scan_real.rs` paces SANE's `test` backend.
///
/// Paced, it pauses a moment after each frame's first read: with the `test` backend's reads
/// limited to 64 bytes (its `read-limit` and `read-limit-size`, in the test's `test.conf`), a
/// frame's end comes long after the backend's reader thread has written the frame and exited,
/// clear of a race in the backend (1.4.0, built with threads) that can block its next frame for
/// good.
#[derive(Default)]
pub struct Recorder {
    pub tracker: Tracker,
    counts: Mutex<Vec<Count>>,
    /// Whether it's paced, and if so, the bytes of each frame of a page (a three-pass page's), or
    /// `None` for pages of one frame.
    pace: Option<Option<u64>>,
    /// The frame of the page being read whose first read has come.
    frame: Mutex<Option<u64>>,
}

impl Recorder {
    pub fn new() -> Recorder {
        Recorder::default()
    }

    /// A recorder paced for the `test` backend, each page's frames `frame_bytes` long where
    /// given, else each page one frame.
    pub fn paced(frame_bytes: Option<u64>) -> Recorder {
        Recorder {
            pace: Some(frame_bytes),
            ..Recorder::default()
        }
    }

    /// The counts, in order.
    pub fn counts(&self) -> Vec<Count> {
        self.counts
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    fn record(&self, count: Count) {
        self.counts
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(count);
    }
}

impl Progress for Recorder {
    fn set_total(&self, total: Option<u64>, unit: Unit) {
        assert_eq!(unit, Unit::Bytes, "a batch counts bytes");
        self.record(Count::Total(total));
        self.tracker.set_total(total, unit);
    }
    fn set(&self, done: u64) {
        self.record(Count::Done(done));
        self.tracker.set(done);
        let Some(frame_bytes) = self.pace else {
            return;
        };
        let mut last = self.frame.lock().unwrap_or_else(PoisonError::into_inner);
        if done == 0 {
            // A page begins.
            *last = None;
            return;
        }
        let frame = frame_bytes.map_or(0, |bytes| (done - 1) / bytes);
        if *last != Some(frame) {
            *last = Some(frame);
            thread::sleep(Duration::from_millis(1));
        }
    }
    fn inc(&self, n: u64) {
        self.tracker.inc(n);
    }
    fn message(&self, level: Level, msg: &str) {
        self.tracker.message(level, msg);
    }
    fn is_cancelled(&self) -> bool {
        self.tracker.is_cancelled()
    }
    fn child(&self, name: &str, weight: f64) -> Box<dyn Progress> {
        self.tracker.child(name, weight)
    }
}

/// Settings from `source`, at `dpi`, in `format`, `duplex` or not; the paper's size the driver's.
pub fn settings(source: Source, duplex: bool, dpi: u32, format: Format) -> Settings {
    let mut settings = Settings::new(source, dpi, format);
    settings.duplex = duplex;
    settings
}

/// A batch on a rendezvous channel, its events taken on a thread of their own as they come, each
/// given to `on_event` as it's taken; the events and how it ended. `on_event` answering false
/// drops the receiver there.
pub fn batch_with(
    device: &mut dyn Device,
    settings: &Settings,
    first_sheet: u32,
    progress: &dyn Progress,
    mut on_event: impl FnMut(&Event) -> bool + Send,
) -> (Vec<Event>, Result<Done, ScanError>) {
    let (events, received) = mpsc::sync_channel(0);
    thread::scope(|scope| {
        let taker = scope.spawn(move || {
            let mut taken = Vec::new();
            for event in received {
                let more = on_event(&event);
                taken.push(event);
                if !more {
                    break;
                }
            }
            taken
        });
        let done = device.scan(settings, first_sheet, &events, progress);
        drop(events);
        (taker.join().expect("the events' thread"), done)
    })
}

/// A batch whose events are all taken.
pub fn batch(
    device: &mut dyn Device,
    settings: &Settings,
    first_sheet: u32,
) -> (Vec<Event>, Result<Done, ScanError>) {
    batch_with(device, settings, first_sheet, &Tracker::new(), |_| true)
}

/// The events, briefly: `applied`, `sheet n`, `page n front`.
pub fn order(events: &[Event]) -> Vec<String> {
    events
        .iter()
        .map(|event| match event {
            Event::Applied(_) => "applied".to_owned(),
            Event::Sheet(sheet) => format!("sheet {sheet}"),
            Event::Page(page) => format!("page {} {:?}", page.sheet, page.side).to_lowercase(),
            other => format!("{other:?}"),
        })
        .collect()
}

/// The batch's `Applied`.
pub fn applied(events: &[Event]) -> &Applied {
    match events.first() {
        Some(Event::Applied(applied)) => applied,
        other => panic!("the first event isn't Applied: {other:?}"),
    }
}

/// The batch's pages.
pub fn pages(events: &[Event]) -> Vec<&RawPage> {
    events
        .iter()
        .filter_map(|event| match event {
            Event::Page(page) => Some(page),
            _ => None,
        })
        .collect()
}

/// A page's raw image: its format, width, height and bytes.
pub fn image(page: &RawPage) -> (Format, u32, u32, &[u8]) {
    let Pixels::Raw(image) = &page.pixels else {
        panic!("not a raw page: {page:?}");
    };
    (image.format(), image.width(), image.height(), image.data())
}
