//! The unit tests' share of what `tests/support` does for the integration
//! tests: each test that calls `run` in a child process of its own, exit 77
//! for a skip (a failure under `TWAIN_DSM_REQUIRE_SAMPLE=1`), 78 for done, so
//! a child that ran no test (its name mistyped) fails rather than passing.

use std::hash::Hash;
use std::io::Write as _;
use std::time::Duration;

const SKIPPED: i32 = 77;
const DONE: i32 = 78;

pub(crate) enum Outcome {
    Done,
    Skip(String),
}

pub(crate) fn fork(name: &str, id: impl Hash, body: impl FnOnce() -> Outcome) {
    rusty_fork::fork(
        name,
        id,
        |_| {},
        |child, _| {
            let status = child
                .wait_timeout(Duration::from_secs(120))
                .expect("the child could be waited for")
                .unwrap_or_else(|| panic!("{name}: the child ran past 120 seconds"));
            match status.code() {
                Some(DONE) => {}
                // Past the test harness's capture, so a skip shows.
                Some(SKIPPED) => {
                    let _ = writeln!(
                        std::io::stderr(),
                        "{name}: skipped (the child's output says why)"
                    );
                }
                Some(0) => panic!("{name}: the child ran no test (is the name right?)"),
                _ => panic!("{name}: the child failed ({status:?})"),
            }
        },
        || match body() {
            Outcome::Done => std::process::exit(DONE),
            Outcome::Skip(why) => {
                let required = std::env::var("TWAIN_DSM_REQUIRE_SAMPLE").is_ok_and(|v| v == "1");
                assert!(!required, "{why}, and TWAIN_DSM_REQUIRE_SAMPLE=1");
                println!("skipped: {why}");
                std::process::exit(SKIPPED);
            }
        },
    )
    .expect("the child process could be started");
}
