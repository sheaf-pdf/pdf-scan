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

/// The variable that hands the fake's path to the child.
const FAKE_PATH: &str = "SANE_DL_TEST_FAKE";

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
}

impl Default for Child {
    fn default() -> Child {
        Child {
            env: Vec::new(),
            prepare: Vec::new(),
            timeout: Duration::from_secs(60),
            require: None,
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

    /// Runs `body` in a child process, through [`in_child!`].
    pub fn run(self, test: &str, id: rusty_fork::RustyForkId, body: impl FnOnce()) {
        let Child {
            env,
            prepare,
            timeout,
            require,
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

/// The fake libsane, built once per test binary by the cargo that built the
/// tests, into a target directory of its own, without the flags or target
/// of the run that built them (a sanitizer's or a cross build's). In the
/// parent only, as a child's preparation: the child is given the path.
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
        let path = target.join("debug").join(name);
        assert!(
            path.is_file(),
            "the fake libsane isn't at {}",
            path.display()
        );
        path
    })
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
    /// script there and builds the fake, if it hasn't yet.
    pub fn child(&self) -> Child {
        let (dir, script, text) = (self.dir.clone(), self.script.clone(), self.text.clone());
        Child::new()
            .prepare(move |command| {
                fresh_dir(&dir);
                fs::write(&script, text).expect("couldn't write the fake's script");
                command.env(FAKE_PATH, fake_library());
            })
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

/// In the child: the path of the fake its parent built.
pub fn fake_path() -> PathBuf {
    std::env::var_os(FAKE_PATH)
        .expect("the parent names the fake libsane")
        .into()
}

/// In the child: loads the fake its parent built, failing the test if it
/// can't.
pub fn load_fake() -> Sane {
    Sane::load_from(fake_path())
        .unwrap_or_else(|error| panic!("couldn't load the fake: {}", chain(&error)))
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

/// In the child: the system's libsane, or the one `SANE_DL_LIBSANE` names;
/// skips where it isn't there, and fails on any other error.
pub fn load_real() -> Sane {
    let loaded = match std::env::var_os("SANE_DL_LIBSANE") {
        Some(path) => Sane::load_from(path),
        None => Sane::load(),
    };
    match loaded {
        Ok(sane) => sane,
        Err(error @ (LoadError::NotFound { .. } | LoadError::NoDefault)) => skip(format!(
            "there's no libsane to load here ({})",
            chain(&error)
        )),
        Err(error) => panic!("couldn't load libsane: {}", chain(&error)),
    }
}
