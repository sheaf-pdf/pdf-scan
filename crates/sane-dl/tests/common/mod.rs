//! What the tests that load a library share: a child process per test, its
//! skip code, the fake libsane built and found, and its script and log.
//!
//! SANE is started once per process and reads `SANE_CONFIG_DIR` once, so
//! each test that loads a library runs its body in a child process of its
//! own (rusty-fork), its environment given at spawn and a timeout on it. The
//! child's output is captured into a file the harness shows only on a
//! failure, so a child that can't run here exits with [`SKIP`] after saying
//! why, and the parent says it again on its own standard error, which the
//! harness doesn't capture, or fails if a `SANE_DL_REQUIRE_*` variable is
//! `1`.
//!
//! rusty-fork's child runs the test function again from its top, until it
//! reaches the fork, so whatever a test does before [`in_child!`] runs in
//! the child too. Building the fake and writing a test's files is the
//! parent's alone: a [`Child`] carries that work as its preparation, which
//! runs as the child is spawned, in the parent only, and the helpers that
//! do it refuse to run in a child.
//!
//! Every library is loaded through [`Sane::load`], the crate's one loader.
//! The fake is found there as SANE is: its parent copies it under SANE's
//! own file name into a directory of its own ([`fake_library`]), and spawns
//! the child with the system loader's search path naming that directory
//! first (`LD_LIBRARY_PATH`; `DYLD_LIBRARY_PATH` on macOS). Windows has no
//! default library, so no route to the fake: each test that loads it is
//! ignored there, saying why.

// Each test binary uses some of these.
#![allow(dead_code)]

use std::ffi::{OsStr, OsString};
use std::fmt::Display;
use std::fs;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;
use std::time::Duration;

use sane_dl::{CONFIG_DIR_VARIABLE, ConfigDir, LoadError, Sane, Search};

/// The variable naming the directories the system loader searches first for
/// a library loaded by name, read as the process starts.
#[cfg(target_os = "macos")]
const LOADER_PATH: &str = "DYLD_LIBRARY_PATH";
#[cfg(not(target_os = "macos"))]
const LOADER_PATH: &str = "LD_LIBRARY_PATH";

/// The exit code of a child that can't run here.
pub const SKIP: i32 = 77;

/// How the child's line saying why it skipped starts.
const SKIP_MARK: &str = "sane-dl-test-skip: ";

/// The variable whose `1` makes a test that needs libsane fail rather than
/// skip where it isn't there.
pub const REQUIRE_LIBSANE: &str = "SANE_DL_REQUIRE_LIBSANE";

/// The variable whose `1` makes the header contract fail rather than skip
/// where `sane.h` or a C compiler isn't there.
pub const REQUIRE_HEADER: &str = "SANE_DL_REQUIRE_HEADER";

/// The variable set in every child, so the parent's work can refuse to run
/// there.
const IN_CHILD: &str = "SANE_DL_TEST_CHILD";

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

/// Says why a test can't run here, on the standard error the harness doesn't
/// capture, or fails if `require` is `1`.
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

/// An error and its sources, on one line.
pub fn chain(error: &dyn std::error::Error) -> String {
    let mut text = error.to_string();
    let mut source = error.source();
    while let Some(error) = source {
        text.push_str(&format!(": {error}"));
        source = error.source();
    }
    text
}

/// Work for the parent alone, run as the child is spawned.
type Preparation = Box<dyn FnOnce(&mut Command)>;

/// A child process to run a test's body in.
pub struct Child {
    env: Vec<(OsString, OsString)>,
    prepare: Vec<Preparation>,
    timeout: Duration,
    require: Option<&'static str>,
    /// Whether the child is to end by aborting, rather than pass.
    aborts: bool,
}

impl Default for Child {
    fn default() -> Child {
        Child {
            env: Vec::new(),
            prepare: Vec::new(),
            timeout: Duration::from_secs(60),
            require: None,
            aborts: false,
        }
    }
}

/// Signal 6, which `abort` raises on Unix.
#[cfg(unix)]
const SIGABRT: i32 = 6;

/// The exit code a process that aborts ends with on Windows: Rust's abort
/// is a fast fail there, `STATUS_STACK_BUFFER_OVERRUN`.
#[cfg(windows)]
const ABORTED: i32 = 0xC000_0409_u32 as i32;

/// How a child that was to abort ended, if it did: by `SIGABRT` on Unix, by
/// its exit code on Windows.
fn aborted(status: &rusty_fork::ExitStatusWrapper) -> Option<String> {
    #[cfg(unix)]
    {
        (status.unix_signal() == Some(SIGABRT)).then(|| format!("by SIGABRT (signal {SIGABRT})"))
    }
    #[cfg(windows)]
    {
        (status.code() == Some(ABORTED)).then(|| format!("with exit code {ABORTED:#x}"))
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = status;
        None
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

    /// Work for the parent alone (files to write, the fake to build), run
    /// as the child is spawned, given the child's command to set variables
    /// on. Never run in the child.
    pub fn prepare(mut self, work: impl FnOnce(&mut Command) + 'static) -> Child {
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

    /// The child is to end by aborting (`SIGABRT` on Unix, its exit code on
    /// Windows): anything else fails the test, and the parent says how it
    /// ended on its own standard error.
    pub fn aborts(mut self) -> Child {
        self.aborts = true;
        self
    }

    /// Runs `body` in a child process, through [`in_child!`].
    pub fn run(self, test: &str, id: rusty_fork::RustyForkId, body: impl FnOnce()) {
        let Child {
            env,
            prepare,
            timeout,
            require,
            aborts,
        } = self;
        // rusty-fork calls this in the parent alone, just before it spawns
        // the child.
        let configure = |command: &mut Command| {
            for work in prepare {
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
            let Some(status) = status else {
                panic!("{test}: the child ran past its timeout of {timeout:?}")
            };
            if aborts {
                let Some(how) = aborted(&status) else {
                    panic!(
                        "{test}: the child was to abort, but ended {status}; its output is above"
                    )
                };
                let _ = writeln!(
                    io::stderr(),
                    "{test}: the child aborted, as it was to: it ended {how}"
                );
                return;
            }
            if status.code() == Some(SKIP) {
                let mut text = String::new();
                let _ = output.seek(SeekFrom::Start(0));
                let _ = output.read_to_string(&mut text);
                let reason = text
                    .lines()
                    .find_map(|line| line.strip_prefix(SKIP_MARK))
                    .unwrap_or("it gave no reason");
                report_skip(test, reason, require);
                return;
            }
            assert!(
                status.success(),
                "{test}: the child failed ({status}); its output is above"
            );
        };
        rusty_fork::fork(test, id, configure, supervise, body).expect("couldn't start the child");
    }
}

/// The directory for one test's files, under the tests' temporary
/// directory. Only its path: [`fresh_dir`] makes it.
pub fn test_dir(test: &str) -> PathBuf {
    Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("sane-dl")
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

/// The directory the fake libsane is loaded from as SANE, holding a copy of
/// it under SANE's own file name alone, one directory per test binary.
///
/// The fake is built once per test binary by the cargo that built the
/// tests, into a target directory of its own, without the flags or target
/// of the run that built them (a sanitizer's or a cross build's). Then it's
/// copied as the file name of [`Sane::DEFAULT_LIBRARIES`]' first entry
/// (`libsane.so.1`; `libsane.1.dylib` on macOS), written under a temporary
/// name holding this process's id and renamed into place, so no child maps
/// half a copy, and a copy a running child maps is replaced, never written
/// over. A child whose loader searches this directory first loads the fake
/// from [`Sane::load`]. In the parent only, as a child's preparation.
pub fn fake_library() -> &'static Path {
    parent_only("building the fake libsane");
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
        let dir = target.join("as-sane").join(env!("CARGO_CRATE_NAME"));
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

/// The loader's search path for a child that loads the fake: `dir` first,
/// then whatever this process was given.
fn loader_path(dir: &Path) -> OsString {
    let given = std::env::var_os(LOADER_PATH).unwrap_or_default();
    std::env::join_paths(std::iter::once(dir.to_owned()).chain(std::env::split_paths(&given)))
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
    /// A test's run of the fake with `script`. Nothing is written yet: the
    /// child's preparation writes it.
    pub fn new(test: &str, script: &str) -> Fake {
        let dir = test_dir(test);
        Fake {
            script: dir.join("script"),
            log: dir.join("log"),
            text: script.to_owned(),
            dir,
        }
    }

    /// A child that loads the fake with this script and log; as it's
    /// spawned, the parent makes the test's directory afresh, writes the
    /// script there, builds and copies the fake, if it hasn't yet, and puts
    /// the fake's directory first on the child's loader search path.
    ///
    /// The child's `SANE_CONFIG_DIR` is the test's directory too, which
    /// names no backend, so a load that reached the system's libsane by
    /// mistake would read none of the system's configuration before
    /// [`load_fake`] fails it.
    pub fn child(&self) -> Child {
        let (dir, script, text) = (self.dir.clone(), self.script.clone(), self.text.clone());
        Child::new()
            .prepare(move |command| {
                fresh_dir(&dir);
                fs::write(&script, text).expect("couldn't write the fake's script");
                command.env(LOADER_PATH, loader_path(fake_library()));
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
}

/// In the child: [`Sane::load`], in a child [`Fake::child`] made, where the
/// loader finds the fake as SANE; then fails the test unless the fake's log
/// is there, which its first call writes, so a test that loaded the
/// system's libsane by mistake fails rather than passing on it.
pub fn try_load_fake() -> Result<Sane, LoadError> {
    let loaded = Sane::load();
    let log = std::env::var_os("FAKE_LIBSANE_LOG").expect("the parent names the fake's log");
    if !Path::new(&log).is_file() {
        let what = match &loaded {
            Ok(sane) => format!("loaded {sane:?}"),
            Err(error) => chain(error),
        };
        panic!(
            "the fake's log isn't at {}, so what `Sane::load` reached wasn't the fake ({what}): \
             is the fake's directory first on {LOADER_PATH}?",
            Path::new(&log).display()
        );
    }
    loaded
}

/// In the child: loads the fake its parent built ([`try_load_fake`]),
/// failing the test if it can't.
pub fn load_fake() -> Sane {
    try_load_fake().unwrap_or_else(|error| panic!("couldn't load the fake: {}", chain(&error)))
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

    /// The functions called, in order, but the option calls (opening a
    /// device reads its descriptors).
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

    /// The calls of `sane_control_option`, each as the log has its details:
    /// `handle=… option=N action=GET|SET|AUTO`, then `value=…` for a set.
    pub fn controls(&self) -> Vec<&str> {
        self.lines
            .iter()
            .filter(|line| line.kind == "call" && line.function == "sane_control_option")
            .map(|line| line.rest.as_str())
            .collect()
    }

    /// The calls of `sane_control_option` on option `option`, as
    /// [`controls`](Self::controls) gives them.
    pub fn controls_of(&self, option: usize) -> Vec<&str> {
        let wanted = format!(" option={option} ");
        self.controls()
            .into_iter()
            .filter(|rest| rest.contains(&wanted))
            .collect()
    }

    /// Where the `n`th line of a kind for a function is, and the line.
    pub fn nth(&self, kind: &str, function: &str, n: usize) -> (usize, &LogLine) {
        self.lines
            .iter()
            .enumerate()
            .filter(|(_, line)| line.kind == kind && line.function == function)
            .nth(n)
            .unwrap_or_else(|| {
                panic!(
                    "no {kind} {function} number {n} in the log: {:#?}",
                    self.lines
                )
            })
    }
}

/// A child for a test of the real libsane: its `SANE_CONFIG_DIR` an isolated
/// configuration naming `backends`, in the test's directory, never the
/// system's, written by the parent as the child is spawned; a skip a
/// failure where `SANE_DL_REQUIRE_LIBSANE=1`.
pub fn real_child(test: &str, backends: &[&str]) -> Child {
    real_child_with(test, backends, &[])
}

/// A child as [`real_child`] makes, its configuration directory holding
/// `files` too (a backend's own configuration, such as `test.conf`), each a
/// name and its text.
pub fn real_child_with(test: &str, backends: &[&str], files: &[(&str, &str)]) -> Child {
    let dir = test_dir(test);
    let backends: Vec<String> = backends.iter().map(|&name| name.to_owned()).collect();
    let files: Vec<(String, String)> = files
        .iter()
        .map(|&(name, text)| (name.to_owned(), text.to_owned()))
        .collect();
    Child::new()
        .prepare(move |command| {
            fresh_dir(&dir);
            let config = dir.join("config");
            fs::create_dir(&config).expect("couldn't make the configuration directory");
            for (name, text) in &files {
                fs::write(config.join(name), text).expect("couldn't write a configuration file");
            }
            let config = ConfigDir::write(&config, &backends, Search::Isolated)
                .expect("couldn't write the configuration");
            command.env(CONFIG_DIR_VARIABLE, config.value());
        })
        .require(REQUIRE_LIBSANE)
}

/// In the child: the system's libsane ([`Sane::load`]); skips where it
/// isn't there, and fails on any other error. Another libsane is tested by
/// setting the loader's search path for the run, which every child
/// inherits.
pub fn load_real() -> Sane {
    match Sane::load() {
        Ok(sane) => sane,
        Err(error @ (LoadError::NotFound { .. } | LoadError::NoDefault)) => skip(format!(
            "there's no libsane to load here ({})",
            chain(&error)
        )),
        Err(error) => panic!("couldn't load libsane: {}", chain(&error)),
    }
}

/// The fake's bytes of a frame, its padding dropped, as its crate docs say
/// it sends them: page `page` (from 1), a back if `back`, frame `frame`
/// (from 0), `rows` rows of `pixel_bytes` bytes.
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

/// In the child: the most memory this process has held, in bytes, where
/// the system says (Linux).
pub fn peak_memory() -> Option<u64> {
    let status = fs::read_to_string("/proc/self/status").ok()?;
    let line = status.lines().find(|line| line.starts_with("VmHWM:"))?;
    let kib: u64 = line.split_whitespace().nth(1)?.parse().ok()?;
    Some(kib * 1024)
}
