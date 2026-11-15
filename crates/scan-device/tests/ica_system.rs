//! The ImageCaptureCore backend through the real framework, on macOS: a listing on the main
//! thread; a stored id that isn't there, `Gone` once the framework's local enumeration ends; and
//! a listing from another thread, refused at once whatever the main thread does: once with the
//! main thread blocked in a `join`, and once with it running its run loop (where a browser started
//! for such a call was seen to get no callbacks). Off macOS each case says it skipped.
//!
//! A harness of its own (`harness = false`), so `main` is the process's main thread, as a
//! worker's is, and the cases run on it in turn. It answers `--list` as libtest does (`<name>:
//! test` a line, then a count but in the terse format, `--format terse` or `--quiet`), takes name
//! filters (`--exact` and `--skip` too), and ignores libtest's other flags (`--nocapture`,
//! `--test-threads` and the rest) rather than reading them as filters.
//!
//! With no scanner (CI's runner), a listing answers `Ok` and empty, 2 s after the framework's
//! local enumeration ends. `SCAN_DEVICE_REQUIRE_ICA=1` (CI's macOS check job) makes that answer
//! required within 20 s of the 30 s deadline: the local enumeration's callback came through the
//! backend's run of the main run loop. Without it, a listing the framework never finishes
//! (`TimedOut`) is said as a skip.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::process::ExitCode;
use std::time::Instant;

/// What a case came to, where it didn't fail (a failure is a panic).
enum Outcome {
    /// Off macOS every case skips.
    #[cfg_attr(not(all(feature = "ica", target_os = "macos")), allow(dead_code))]
    Passed,
    Skipped(String),
}

struct Case {
    name: &'static str,
    run: fn() -> Outcome,
}

const CASES: &[Case] = &[
    Case {
        name: "a_listing_on_the_main_thread_answers_before_its_deadline",
        run: system::listing_on_the_main_thread,
    },
    Case {
        name: "a_stored_id_that_isn_t_there_is_gone_once_the_local_enumeration_ends",
        run: system::a_stored_id_not_there,
    },
    Case {
        name: "a_listing_off_the_main_thread_while_main_waits_in_a_join_is_refused_at_once",
        run: system::off_the_main_thread_joined,
    },
    Case {
        name: "a_listing_off_the_main_thread_while_main_runs_its_run_loop_is_refused_at_once",
        run: system::off_the_main_thread_serving,
    },
];

/// libtest's command line, as far as this harness reads it.
#[derive(Default)]
struct Args {
    list: bool,
    /// `--quiet` or `-q`: the terse format where `--format` names none.
    quiet: bool,
    /// `--format`'s value.
    format: Option<String>,
    exact: bool,
    ignored_only: bool,
    filters: Vec<String>,
    skips: Vec<String>,
}

impl Args {
    fn parse(args: impl IntoIterator<Item = String>) -> Args {
        let mut parsed = Args::default();
        let mut args = args.into_iter();
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--list" => parsed.list = true,
                "--quiet" | "-q" => parsed.quiet = true,
                "--exact" => parsed.exact = true,
                "--ignored" => parsed.ignored_only = true,
                "--skip" => parsed.skips.extend(args.next()),
                "--format" => parsed.format = args.next(),
                // libtest's flags that take the next argument as their value: it's theirs, not a
                // filter.
                "--test-threads" | "--color" | "--logfile" | "--shuffle-seed" | "-Z" => {
                    args.next();
                }
                _ => {
                    if let Some(skip) = arg.strip_prefix("--skip=") {
                        parsed.skips.push(skip.to_owned());
                    } else if let Some(format) = arg.strip_prefix("--format=") {
                        parsed.format = Some(format.to_owned());
                    } else if !arg.starts_with('-') {
                        parsed.filters.push(arg);
                    }
                    // Every other flag of libtest's (`--nocapture`, `--test-threads=1`,
                    // `--include-ignored` and the rest) is ignored.
                }
            }
        }
        parsed
    }

    /// Whether the output is libtest's terse format: `--format terse`, or `--quiet` with no
    /// `--format`.
    fn terse(&self) -> bool {
        match self.format.as_deref() {
            Some(format) => format == "terse",
            None => self.quiet,
        }
    }

    fn matches(&self, pattern: &str, name: &str) -> bool {
        if self.exact {
            name == pattern
        } else {
            name.contains(pattern)
        }
    }

    /// Whether `name` is run (or listed): none of these cases is ignored, so `--ignored` runs
    /// none.
    fn selects(&self, name: &str) -> bool {
        !self.ignored_only
            && (self.filters.is_empty() || self.filters.iter().any(|f| self.matches(f, name)))
            && !self.skips.iter().any(|skip| self.matches(skip, name))
    }
}

fn main() -> ExitCode {
    let args = Args::parse(std::env::args().skip(1));
    let selected: Vec<&Case> = CASES
        .iter()
        .filter(|case| args.selects(case.name))
        .collect();
    if args.list {
        for case in &selected {
            println!("{}: test", case.name);
        }
        // As libtest: the count but in the terse format, whose lines are the names alone.
        if !args.terse() {
            println!();
            println!("{} tests, 0 benchmarks", selected.len());
        }
        return ExitCode::SUCCESS;
    }
    let start = Instant::now();
    println!();
    println!(
        "running {} test{}",
        selected.len(),
        if selected.len() == 1 { "" } else { "s" }
    );
    let (mut passed, mut skipped, mut failed) = (0, 0, Vec::new());
    for case in &selected {
        let began = Instant::now();
        match catch_unwind(AssertUnwindSafe(case.run)) {
            Ok(Outcome::Passed) => {
                passed += 1;
                println!(
                    "test {} ... ok ({:.2} s)",
                    case.name,
                    began.elapsed().as_secs_f32()
                );
            }
            Ok(Outcome::Skipped(why)) => {
                skipped += 1;
                println!("test {} ... skipped: {why}", case.name);
            }
            Err(_) => {
                println!("test {} ... FAILED", case.name);
                failed.push(case.name);
            }
        }
    }
    println!();
    if !failed.is_empty() {
        println!("failures:");
        for name in &failed {
            println!("    {name}");
        }
        println!();
    }
    println!(
        "test result: {}. {passed} passed; {} failed; {skipped} skipped; 0 ignored; 0 measured; \
         {} filtered out; finished in {:.2}s",
        if failed.is_empty() { "ok" } else { "FAILED" },
        failed.len(),
        CASES.len() - selected.len(),
        start.elapsed().as_secs_f32()
    );
    println!();
    if failed.is_empty() {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(101)
    }
}

#[cfg(not(all(feature = "ica", target_os = "macos")))]
mod system {
    use super::Outcome;

    fn elsewhere() -> Outcome {
        Outcome::Skipped("ImageCaptureCore is macOS's".to_owned())
    }

    pub fn listing_on_the_main_thread() -> Outcome {
        elsewhere()
    }

    pub fn a_stored_id_not_there() -> Outcome {
        elsewhere()
    }

    pub fn off_the_main_thread_joined() -> Outcome {
        elsewhere()
    }

    pub fn off_the_main_thread_serving() -> Outcome {
        elsewhere()
    }
}

#[cfg(all(feature = "ica", target_os = "macos"))]
mod system {
    use std::panic::{AssertUnwindSafe, catch_unwind};
    use std::sync::mpsc;
    use std::thread;
    use std::time::{Duration, Instant};

    use dispatch2::DispatchQueue;
    use objc2_core_foundation::CFRunLoop;
    use scan_device::ica::IcaBackend;
    use scan_device::{Backend, DeviceId, DeviceInfo, Driver, ScanError};

    use super::Outcome;

    /// Each listing's and open's deadline.
    const DEADLINE: Duration = Duration::from_secs(30);
    /// How soon one must answer where [`REQUIRE`] is `1`.
    const REQUIRED_WITHIN: Duration = Duration::from_secs(20);
    /// How soon a call off the main thread must be refused: at once, the framework not reached.
    const REFUSED_WITHIN: Duration = Duration::from_secs(1);

    const REQUIRE: &str = "SCAN_DEVICE_REQUIRE_ICA";

    fn required() -> bool {
        std::env::var_os(REQUIRE).is_some_and(|value| value == "1")
    }

    /// A listing's answer: every device the framework's, with an `ica:` id, their count and the
    /// time said; where [`REQUIRE`] is `1`, an `Ok` within [`REQUIRED_WITHIN`]. A listing the
    /// framework never finished (`TimedOut`) is a skip without it.
    fn judge_listing(
        listed: Result<Vec<DeviceInfo>, ScanError>,
        took: Duration,
        how: &str,
    ) -> Outcome {
        match listed {
            Ok(devices) => {
                for device in &devices {
                    assert_eq!(device.driver, Driver::Ica, "{device:?}");
                    assert!(device.id.0.starts_with("ica:"), "{device:?}");
                }
                eprintln!(
                    "a listing {how}: {} scanners in {:.2} s",
                    devices.len(),
                    took.as_secs_f32()
                );
                if required() {
                    assert!(
                        took < REQUIRED_WITHIN,
                        "the listing {how} took {took:?}, past {REQUIRED_WITHIN:?}"
                    );
                }
                Outcome::Passed
            }
            Err(ScanError::TimedOut) if !required() => Outcome::Skipped(format!(
                "the listing {how} timed out after {:.1} s: the framework's local enumeration \
                 never ended here ({REQUIRE}=1 makes this fail)",
                took.as_secs_f32()
            )),
            Err(err) => panic!("a listing {how} answered {err:?} after {took:?}"),
        }
    }

    pub fn listing_on_the_main_thread() -> Outcome {
        let mut backend = IcaBackend::new();
        let start = Instant::now();
        let listed = backend.list(DEADLINE);
        judge_listing(listed, start.elapsed(), "on the main thread")
    }

    pub fn a_stored_id_not_there() -> Outcome {
        let mut backend = IcaBackend::new();
        let stored = DeviceInfo::new(
            DeviceId("ica:no-such-scanner-0000".to_owned()),
            "No such scanner".to_owned(),
            String::new(),
            String::new(),
            Driver::Ica,
        );
        let start = Instant::now();
        let opened = backend.open(&stored, DEADLINE);
        let took = start.elapsed();
        match opened {
            Err(ScanError::Gone) => {
                eprintln!("a stored id not there: Gone in {:.2} s", took.as_secs_f32());
                if required() {
                    assert!(
                        took < REQUIRED_WITHIN,
                        "Gone came after {took:?}, past {REQUIRED_WITHIN:?}"
                    );
                }
                Outcome::Passed
            }
            Err(ScanError::TimedOut) if !required() => Outcome::Skipped(format!(
                "the open timed out after {:.1} s: the framework's local enumeration never \
                 ended here ({REQUIRE}=1 makes this fail)",
                took.as_secs_f32()
            )),
            Ok(opened) => panic!("a device of a made-up id was opened: {:?}", opened.info),
            Err(err) => panic!("the open answered {err:?} after {took:?}"),
        }
    }

    /// A listing off the main thread's answer: refused at once, naming the main thread.
    fn judge_refusal(
        listed: Result<Vec<DeviceInfo>, ScanError>,
        took: Duration,
        how: &str,
    ) -> Outcome {
        match listed {
            Err(ScanError::Driver { message, shown }) => {
                assert!(!shown);
                assert!(message.contains("main thread"), "{message}");
                assert!(took < REFUSED_WITHIN, "refused after {took:?}, not at once");
                eprintln!(
                    "a listing off the main thread, {how}: refused in {:.3} s ({message})",
                    took.as_secs_f32()
                );
                Outcome::Passed
            }
            other => panic!("expected the main thread's refusal, got {other:?} after {took:?}"),
        }
    }

    pub fn off_the_main_thread_joined() -> Outcome {
        let listing = thread::spawn(|| {
            let start = Instant::now();
            let listed = IcaBackend::new().list(DEADLINE);
            (listed, start.elapsed())
        });
        // The main thread waits here, its queue unserved.
        let (listed, took) = listing.join().expect("the listing's thread");
        judge_refusal(listed, took, "joined")
    }

    pub fn off_the_main_thread_serving() -> Outcome {
        let (answer, answered) = mpsc::channel();
        let listing = thread::spawn(move || {
            let start = Instant::now();
            let listed = catch_unwind(AssertUnwindSafe(|| IcaBackend::new().list(DEADLINE)));
            let _ = answer.send((listed, start.elapsed()));
            // The main run loop stopped from a block on the main queue, which runs only once the
            // loop serves it: the stop can't come before the loop runs.
            DispatchQueue::main().exec_async(|| {
                if let Some(main) = CFRunLoop::main() {
                    main.stop();
                }
            });
        });
        CFRunLoop::run();
        listing.join().expect("the listing's thread");
        let (listed, took) = answered.recv().expect("the listing's answer");
        let listed = match listed {
            Ok(listed) => listed,
            Err(panic) => std::panic::resume_unwind(panic),
        };
        judge_refusal(listed, took, "while main runs its run loop")
    }
}
