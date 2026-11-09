//! The unit tests' share of what `tests/support` does for the integration
//! tests: each test that calls `run` in a child process of its own, exit 77
//! for a skip (a failure under `TWAIN_DSM_REQUIRE_SAMPLE=1`).

use std::hash::Hash;
use std::time::Duration;

const SKIPPED: i32 = 77;

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
                Some(0) => {}
                Some(SKIPPED) => eprintln!("{name}: skipped (the child's output says why)"),
                _ => panic!("{name}: the child failed ({status:?})"),
            }
        },
        || match body() {
            Outcome::Done => {}
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
