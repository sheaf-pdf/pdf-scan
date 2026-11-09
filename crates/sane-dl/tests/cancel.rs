//! Cancelling, against the fake libsane: a cancel from another thread while
//! a read is blocked, one between pages, cancels racing a close, a backend
//! that ignores the cancel, one that hangs for good, and one that aborts.
//! The fake's log is checked for SANE's rules each time: above all, that no
//! cancel reaches a closed handle. Each test runs in a child process of its
//! own; these are the tests run under ThreadSanitizer.

#[macro_use]
mod common;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Barrier};
use std::thread;
use std::time::{Duration, Instant};

use common::Fake;
use sane_dl::{Error, Page};

const DEVICE: &str = "device fake:0 Fake Scanner flatbed\n";

fn script(lines: &str) -> String {
    format!("{DEVICE}{lines}")
}

/// In the child: waits until the fake's log holds `needle`.
fn wait_for_log(needle: &str) {
    let path = std::env::var_os("FAKE_LIBSANE_LOG").expect("the parent names the log");
    let until = Instant::now() + Duration::from_secs(20);
    while !std::fs::read_to_string(&path).is_ok_and(|log| log.contains(needle)) {
        assert!(Instant::now() < until, "the log never said {needle:?}");
        thread::sleep(Duration::from_millis(5));
    }
}

#[test]
#[cfg_attr(not(unix), ignore = "loads the fake by SANE's name: none on Windows")]
fn a_cancel_during_a_blocked_read_returns_cancelled() {
    let fake = Fake::new(
        "a_cancel_during_a_blocked_read_returns_cancelled",
        &script("at 1 hang 20\nread-max 20\n"),
    );
    in_child!(
        a_cancel_during_a_blocked_read_returns_cancelled,
        fake.child(),
        || {
            let sane = common::load_fake();
            let mut handle = sane.open("fake:0").unwrap();
            let token = handle.cancel_token();
            let canceller = thread::spawn(move || {
                wait_for_log("hang sane_read waits for sane_cancel");
                token.cancel();
                token
            });
            let mut heard = 0;
            match handle.batch().next_page(|_| heard += 1) {
                Err(Error::Cancelled) => {}
                other => panic!("{other:?}"),
            }
            assert_eq!(heard, 1);
            let token = canceller.join().unwrap();
            // The read that returned took the cancel.
            assert!(!token.is_requested());
            // The batch may start again.
            match handle.batch().next_page(|_| {}) {
                Ok(Page::Image(_)) => {}
                other => panic!("{other:?}"),
            }
        }
    );
    let log = fake.log();
    log.assert_no_violations();
    // The cancel came from another thread while the read waited, the read
    // returned CANCELLED, and the reading thread ended the scan.
    let (hung, read) = log.nth("call", "sane_read", 1);
    let (cancelled, cancel) = log.nth("call", "sane_cancel", 0);
    let (returned, answer) = log.nth("return", "sane_read", 1);
    assert!(hung < cancelled && cancelled < returned, "{:#?}", log.lines);
    assert_ne!(cancel.thread, read.thread);
    assert_eq!(answer.rest, "status=CANCELLED");
    let (ended, end) = log.nth("call", "sane_cancel", 1);
    assert!(returned < ended);
    assert_eq!(end.thread, read.thread);
}

#[test]
#[cfg_attr(not(unix), ignore = "loads the fake by SANE's name: none on Windows")]
fn a_cancel_between_pages_stops_the_next_one() {
    let fake = Fake::new("a_cancel_between_pages_stops_the_next_one", DEVICE);
    in_child!(
        a_cancel_between_pages_stops_the_next_one,
        fake.child(),
        || {
            let sane = common::load_fake();
            let mut handle = sane.open("fake:0").unwrap();
            let token = handle.cancel_token();
            let mut batch = handle.batch();
            assert!(matches!(batch.next_page(|_| {}), Ok(Page::Image(_))));
            let other = token.clone();
            thread::spawn(move || other.cancel()).join().unwrap();
            assert!(token.is_requested());
            match batch.next_page(|_| {}) {
                Err(Error::Cancelled) => {}
                other => panic!("{other:?}"),
            }
            assert!(!token.is_requested());
            // The cancel was taken: the next page comes.
            let Ok(Page::Image(image)) = batch.next_page(|_| {}) else {
                panic!()
            };
            assert_eq!(image.data, common::fake_pixels(2, false, 0, 16, 4));
            drop(batch);
            handle.close();
            // A cancel after the close reaches nothing.
            token.cancel();
        }
    );
    let log = fake.log();
    log.assert_no_violations();
    let calls: Vec<(&str, &str)> = log
        .lines
        .iter()
        .filter(|line| line.kind == "call" && line.function != "sane_read")
        .filter(|line| line.function.starts_with("sane_") && !line.function.contains("option"))
        .map(|line| (line.function.as_str(), line.thread.as_str()))
        .collect();
    let main = log.nth("call", "sane_start", 0).1.thread.as_str();
    let functions: Vec<&str> = calls.iter().map(|(function, _)| *function).collect();
    assert_eq!(
        functions,
        [
            "sane_init",
            "sane_open",
            "sane_start",
            "sane_get_parameters",
            "sane_cancel",
            "sane_cancel",
            "sane_start",
            "sane_get_parameters",
            "sane_cancel",
            "sane_close",
            "sane_exit"
        ]
    );
    // The token's cancel, from its thread, then the next page's own, and
    // no start between them.
    assert_ne!(calls[4].1, main);
    assert_eq!(calls[5].1, main);
}

#[test]
#[cfg_attr(not(unix), ignore = "loads the fake by SANE's name: none on Windows")]
fn a_cancel_left_from_the_last_batch_can_be_cleared() {
    let fake = Fake::new(
        "a_cancel_left_from_the_last_batch_can_be_cleared",
        &script("sheets 2\n"),
    );
    in_child!(
        a_cancel_left_from_the_last_batch_can_be_cleared,
        fake.child(),
        || {
            let sane = common::load_fake();
            let mut handle = sane.open("fake:0").unwrap();
            let token = handle.cancel_token();
            let mut batch = handle.batch();
            assert!(matches!(batch.next_page(|_| {}), Ok(Page::Image(_))));
            // The cancel comes as the batch ends, too late for it.
            token.cancel();
            drop(batch);
            assert!(handle.clear_cancel());
            assert!(!token.is_requested());
            assert!(!handle.clear_cancel(), "nothing left to clear");
            // The next batch starts afresh.
            let mut batch = handle.batch();
            let Ok(Page::Image(image)) = batch.next_page(|_| {}) else {
                panic!()
            };
            assert_eq!(image.data, common::fake_pixels(2, false, 0, 16, 4));
        }
    );
    fake.log().assert_no_violations();
}

#[test]
#[cfg_attr(not(unix), ignore = "loads the fake by SANE's name: none on Windows")]
fn a_cancel_racing_close_never_reaches_a_closed_handle() {
    const ROUNDS: usize = 10_000;
    let fake = Fake::new(
        "a_cancel_racing_close_never_reaches_a_closed_handle",
        DEVICE,
    );
    let child = fake.child().timeout(Duration::from_secs(600));
    in_child!(
        a_cancel_racing_close_never_reaches_a_closed_handle,
        child,
        || {
            let sane = common::load_fake();
            for _ in 0..ROUNDS {
                let mut handle = sane.open("fake:0").unwrap();
                handle.start().unwrap();
                let token = handle.cancel_token();
                let start = Arc::new(Barrier::new(2));
                let closed = Arc::new(AtomicBool::new(false));
                let canceller = {
                    let (start, closed) = (Arc::clone(&start), Arc::clone(&closed));
                    thread::spawn(move || {
                        start.wait();
                        // Cancels as the close begins, during it, and
                        // after it.
                        for _ in 0..16 {
                            if closed.load(Ordering::Acquire) {
                                break;
                            }
                            token.cancel();
                        }
                        while !closed.load(Ordering::Acquire) {
                            thread::yield_now();
                        }
                        token.cancel();
                    })
                };
                start.wait();
                handle.close();
                closed.store(true, Ordering::Release);
                canceller.join().unwrap();
            }
        }
    );
    let log = fake.log();
    log.assert_no_violations();
    let closes = log
        .lines
        .iter()
        .filter(|line| line.kind == "call" && line.function == "sane_close")
        .count();
    assert_eq!(closes, ROUNDS);
    let cancels = log
        .lines
        .iter()
        .filter(|line| line.kind == "call" && line.function == "sane_cancel")
        .count();
    let _ = writeln_stderr(&format!(
        "a_cancel_racing_close_never_reaches_a_closed_handle: {ROUNDS} rounds, {cancels} cancels reached open handles, none a closed one"
    ));
}

fn writeln_stderr(text: &str) -> std::io::Result<()> {
    use std::io::Write;
    writeln!(std::io::stderr(), "{text}")
}

#[test]
#[cfg_attr(not(unix), ignore = "loads the fake by SANE's name: none on Windows")]
fn a_read_that_ignores_the_cancel_stops_on_the_tokens_flag() {
    let fake = Fake::new(
        "a_read_that_ignores_the_cancel_stops_on_the_tokens_flag",
        &script("at 1 ignore-cancel 10\n"),
    );
    let child = fake.child().timeout(Duration::from_secs(20));
    in_child!(
        a_read_that_ignores_the_cancel_stops_on_the_tokens_flag,
        child,
        || {
            let sane = common::load_fake();
            let mut handle = sane.open("fake:0").unwrap();
            let token = handle.cancel_token();
            let canceller = thread::spawn(move || {
                // The reads answer GOOD with no bytes by now, and go on, a
                // pause between them.
                wait_for_log("length=0 given=0");
                thread::sleep(Duration::from_millis(300));
                token.cancel();
            });
            match handle.batch().next_page(|_| {}) {
                Err(Error::Cancelled) => {}
                other => panic!("{other:?}"),
            }
            canceller.join().unwrap();
        }
    );
    let log = fake.log();
    log.assert_no_violations();
    // At most the read in flight comes after the token's cancel.
    let (cancelled, _) = log.nth("call", "sane_cancel", 0);
    let after = log.lines[cancelled..]
        .iter()
        .filter(|line| line.kind == "call" && line.function == "sane_read")
        .count();
    assert!(after <= 1, "{after} reads after the cancel");
    // The empty reads came after pauses that grow to 10 ms, not one after
    // another: about 30 in 300 ms, where reading at once makes thousands.
    let empty = log
        .lines
        .iter()
        .filter(|line| line.kind == "return" && line.rest.ends_with("length=0 given=0"))
        .count();
    assert!((2..200).contains(&empty), "{empty} empty reads");
}

#[test]
#[cfg_attr(not(unix), ignore = "loads the fake by SANE's name: none on Windows")]
fn a_read_that_hangs_for_good_ignores_the_cancel() {
    let fake = Fake::new(
        "a_read_that_hangs_for_good_ignores_the_cancel",
        &script("at 1 hang-forever 0\n"),
    );
    in_child!(
        a_read_that_hangs_for_good_ignores_the_cancel,
        fake.child(),
        || {
            let sane = common::load_fake();
            let mut handle = sane.open("fake:0").unwrap();
            let token = handle.cancel_token();
            let reader = thread::spawn(move || {
                let _ = handle.batch().next_page(|_| {});
            });
            wait_for_log("hang sane_read waits for good");
            token.cancel();
            thread::sleep(Duration::from_millis(300));
            // Still blocked: stopping it is a watchdog's, which kills the
            // process. This one ends as the test does.
            assert!(!reader.is_finished());
        }
    );
    let log = fake.log();
    log.assert_no_violations();
    let (_, read) = log.nth("call", "sane_read", 0);
    let (_, cancel) = log.nth("call", "sane_cancel", 0);
    assert_ne!(read.thread, cancel.thread);
    assert!(
        !log.lines
            .iter()
            .any(|line| line.kind == "return" && line.function == "sane_read")
    );
}

#[test]
#[cfg_attr(not(unix), ignore = "loads the fake by SANE's name: none on Windows")]
fn the_abort_ends_the_child() {
    let fake = Fake::new("the_abort_ends_the_child", &script("at 2 abort\n"));
    in_child!(the_abort_ends_the_child, fake.child().aborts(), || {
        let sane = common::load_fake();
        let mut handle = sane.open("fake:0").unwrap();
        let mut batch = handle.batch();
        assert!(matches!(batch.next_page(|_| {}), Ok(Page::Image(_))));
        let _ = batch.next_page(|_| {});
        unreachable!("the fake aborts at page 2's start");
    });
    let log = fake.log();
    log.assert_no_violations();
    let last = log.lines.last().unwrap();
    assert_eq!(
        (last.kind.as_str(), last.function.as_str()),
        ("note", "abort")
    );
    assert_eq!(
        log.lines
            .iter()
            .filter(|line| line.kind == "return" && line.function == "sane_start")
            .count(),
        1
    );
}
