//! The main thread: which scenarios to run, each on a TWAIN thread of its own,
//! their events printed, a watchdog over each, and the exit code.

use std::io::Write;
use std::sync::mpsc::RecvTimeoutError;
use std::time::Duration;

use windows_sys::Win32::System::Threading::{GetCurrentProcess, TerminateProcess};

use crate::scenarios::{SCENARIOS, Scenario};
use crate::twain_thread::{self, Event};

/// How long a scenario may go without an event before its thread is taken
/// for hung.
const WATCHDOG: Duration = Duration::from_secs(30);

pub fn main() -> i32 {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let chosen: Vec<&'static Scenario> = match args.as_slice() {
        [all] if all == "all" => SCENARIOS.iter().collect(),
        [name] => match SCENARIOS.iter().find(|s| s.name == name) {
            Some(s) => vec![s],
            None => return usage(&format!("no scenario named {name:?}")),
        },
        _ => return usage("name one scenario, or all"),
    };
    let bits = usize::BITS;
    println!("twain-spike, {bits}-bit");
    let mut failed = 0;
    for scenario in &chosen {
        println!("== {} ({bits}-bit): {}", scenario.name, scenario.about);
        let failures = run_one(scenario);
        println!(
            "== {}: {}",
            scenario.name,
            if failures == 0 {
                "passed".to_owned()
            } else {
                format!("{failures} failures")
            }
        );
        if failures > 0 {
            failed += 1;
        }
    }
    println!("{} scenarios, {failed} failed", chosen.len());
    i32::from(failed > 0)
}

fn run_one(scenario: &'static Scenario) -> u32 {
    let (handle, events) = match twain_thread::spawn(scenario) {
        Ok(started) => started,
        Err(e) => {
            println!("  FAIL starting the TWAIN thread: {e}");
            return 1;
        }
    };
    let mut failures = 0;
    let state = loop {
        match events.recv_timeout(WATCHDOG) {
            Ok(Event::Ready { window, thread }) => {
                println!("  TWAIN thread {thread}, window {window:#x}")
            }
            Ok(Event::Line(text)) => println!("  {text}"),
            Ok(Event::Fail(text)) => {
                println!("  FAIL {text}");
                failures += 1;
            }
            Ok(Event::Finished { state }) => break state,
            Err(RecvTimeoutError::Timeout) => {
                println!(
                    "  FAIL no event for {} seconds: the TWAIN thread is taken for hung",
                    WATCHDOG.as_secs()
                );
                terminate(1);
            }
            Err(RecvTimeoutError::Disconnected) => {
                println!("  FAIL the TWAIN thread ended without finishing (a panic?)");
                terminate(1);
            }
        }
    };
    if handle.join().is_err() {
        println!("  FAIL the TWAIN thread panicked after finishing");
        failures += 1;
    }
    if state != 1 {
        println!("  FAIL the scenario ended in state {state}, not 1");
        failures += 1;
    }
    failures
}

fn usage(problem: &str) -> i32 {
    eprintln!("twain-spike: {problem}");
    eprintln!("usage: twain-spike <scenario> | all");
    for s in SCENARIOS {
        eprintln!("  {:<16} {}", s.name, s.about);
    }
    2
}

/// Ends the process at once. Not `std::process::exit`: `ExitProcess` runs
/// every loaded DLL's detach code after the other threads are killed, and a
/// hung TWAIN thread may hold a lock that code takes (the sample source's
/// `DllMain` unloads the DSM on detach), so exiting could hang too.
fn terminate(code: u32) -> ! {
    let _ = std::io::stdout().flush();
    // SAFETY: the pseudo-handle for this process; ends it with `code`.
    unsafe { TerminateProcess(GetCurrentProcess(), code) };
    std::process::abort()
}
