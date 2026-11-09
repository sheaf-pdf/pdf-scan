//! What the integration tests share.
//!
//! Every test that calls `run` does so in a child process of its own
//! (rusty-fork): one `run` at a time per process, and the DSM, once loaded,
//! stays loaded, so each test starts from a process that never had it. A test
//! that needs what isn't installed (the DSM, the sample source) says why and
//! exits 77; the parent says it skipped, and passes, unless
//! `TWAIN_DSM_REQUIRE_SAMPLE=1`, when the child fails instead.

#![allow(dead_code)] // each test file uses its own part

use std::hash::Hash;
use std::time::Duration;

use twain_dsm::{App, Load, LoadError, Opened, SourceId, Twain};

/// The exit code of a child that skipped.
const SKIPPED: i32 = 77;

/// How a child's test ended, short of a panic.
pub enum Outcome {
    Done,
    Skip(String),
}

/// The sample source's identity, as its code gives it.
pub const SAMPLE_MANUFACTURER: &str = "TWAIN Working Group";
pub const SAMPLE_PRODUCT: &str = "TWAIN2 Software Scanner";

pub fn required(var: &str) -> bool {
    std::env::var(var).is_ok_and(|v| v == "1")
}

/// Runs `body` in a child process: `name` is the test's name as the harness
/// knows it, `id` the call site's (`rusty_fork_id!()`), `env` set in the
/// child's environment at its start.
pub fn fork(name: &str, id: impl Hash, env: &[(&str, &str)], body: impl FnOnce() -> Outcome) {
    rusty_fork::fork(
        name,
        id,
        |command| {
            for (key, value) in env {
                command.env(key, value);
            }
        },
        |child, _| {
            let status = child
                .wait_timeout(Duration::from_secs(120))
                .expect("the child could be waited for")
                .unwrap_or_else(|| panic!("{name}: the child ran past 120 seconds"));
            match status.code() {
                Some(0) => {}
                Some(SKIPPED) => eprintln!("{name}: skipped (the child's output says why)"),
                _ => panic!("{name}: the child failed ({status:?})"),
            }
        },
        || match body() {
            Outcome::Done => {}
            Outcome::Skip(why) => {
                assert!(
                    !required("TWAIN_DSM_REQUIRE_SAMPLE"),
                    "{why}, and TWAIN_DSM_REQUIRE_SAMPLE=1"
                );
                println!("skipped: {why}");
                std::process::exit(SKIPPED);
            }
        },
    )
    .expect("the child process could be started");
}

/// `fork` for the test function named `$name`, from its own call site.
#[macro_export]
macro_rules! forked {
    ($name:ident, $env:expr, $body:expr) => {
        support::fork(
            rusty_fork::rusty_fork_test_name!($name),
            rusty_fork::rusty_fork_id!(),
            $env,
            $body,
        )
    };
}

/// The application the tests name themselves as.
pub fn app() -> App {
    App::new("pdf-scan", "twain-dsm tests", "twain-dsm tests").expect("the names fit")
}

/// The system's DSM loaded and opened, or why the test can't go on.
pub fn open<T>(twain: &Twain<T>) -> Result<Opened<'_, T>, Outcome> {
    let loaded = match twain.load(Load::System) {
        Ok(loaded) => loaded,
        Err(LoadError::NotFound { path }) => {
            return Err(Outcome::Skip(format!(
                "no DSM at {}: run scripts/twain-sample.ps1 Install",
                path.display()
            )));
        }
        Err(e) => panic!("the system's DSM didn't load: {e}"),
    };
    Ok(loaded
        .open(app())
        .map_err(|r| r.into_error())
        .expect("the connection opens"))
}

/// The sample source, if the DSM lists it.
pub fn sample<T>(opened: &Opened<'_, T>) -> Result<SourceId, Outcome> {
    let sources = opened.sources().expect("the DSM lists its sources");
    sources
        .into_iter()
        .find(|s| s.manufacturer() == SAMPLE_MANUFACTURER && s.product_name() == SAMPLE_PRODUCT)
        .ok_or_else(|| {
            Outcome::Skip(
                "the sample source isn't installed: run scripts/twain-sample.ps1 Install".into(),
            )
        })
}
