//! The rules the glue's executor keeps to reach the main thread, in safe Rust, so they're tested
//! on every system: a call posted to the main queue and the handshake that lets its caller give
//! up on it only before it has begun; a posted body's panic caught as its answer; and when a wait
//! on the main thread, which runs the main run loop in slices, is refused at once rather than
//! spun.
//!
//! The glue (`objc.rs`, macOS only) posts with Grand Central Dispatch and pumps with Core
//! Foundation's run loop; what's decided about either is here.

use std::any::Any;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, SyncSender};
use std::time::Duration;

use super::error::driver_msg;
use crate::ScanError;

/// The longest slice of the main run loop a wait on the main thread runs at once, so its
/// callbacks' events are taken between slices.
pub(super) const SLICE: Duration = Duration::from_millis(50);

/// The most slices a wait on the main thread runs, each ending with another source handled,
/// before the no-op it posted to the main queue has run: past it the main queue isn't being
/// served there.
pub(super) const MAX_PROBE_SLICES: u32 = 64;

// What a slice of the run loop answered (`CFRunLoopRunResult`), as Core Foundation numbers it:
// held against the bindings' own on macOS.

/// The mode has nothing to run: inside a block of the main queue, the queue doesn't count.
pub(super) const RUN_FINISHED: i32 = 1;
/// The run loop was stopped.
pub(super) const RUN_STOPPED: i32 = 2;
/// The slice passed with nothing handled.
pub(super) const RUN_TIMED_OUT: i32 = 3;
/// A source was handled (the main queue among them).
pub(super) const RUN_HANDLED_SOURCE: i32 = 4;

/// A call posted to the main queue: its state, which the block and its caller move.
///
/// The block moves it from pending to running before it does anything, and does nothing where
/// it finds it abandoned; the caller, giving up, moves it from pending to abandoned, and where it
/// finds it running waits for the answer instead. So a call is either never run or waited for:
/// nothing is made, opened or started with nobody to own it.
#[derive(Debug, Default)]
pub(super) struct Handshake(AtomicU8);

const PENDING: u8 = 0;
const RUNNING: u8 = 1;
const ABANDONED: u8 = 2;

impl Handshake {
    /// A call not yet begun.
    pub(super) fn new() -> Handshake {
        Handshake(AtomicU8::new(PENDING))
    }

    /// The block's side: true where it may run the call, which it then answers; false where its
    /// caller has given up on it.
    pub(super) fn begin(&self) -> bool {
        self.0
            .compare_exchange(PENDING, RUNNING, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    }

    /// The caller's side, past its wait: true where the call hadn't begun and never will; false
    /// where it has begun, so its answer comes.
    pub(super) fn abandon(&self) -> bool {
        self.0
            .compare_exchange(PENDING, ABANDONED, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    }
}

/// A panic's message, its payload dropped where a panic in that drop can't escape.
pub(super) fn panic_message(payload: Box<dyn Any + Send>) -> String {
    catch_unwind(AssertUnwindSafe(move || {
        let message = if let Some(text) = payload.downcast_ref::<&str>() {
            (*text).to_owned()
        } else if let Some(text) = payload.downcast_ref::<String>() {
            text.clone()
        } else {
            "a panic with no message".to_owned()
        };
        drop(payload);
        message
    }))
    .unwrap_or_else(|payload| {
        // A payload whose drop panicked: its own payload is leaked, never unwound further.
        std::mem::forget(payload);
        "a panic whose payload panicked as it was dropped".to_owned()
    })
}

/// `work` run with any panic in it caught and its payload leaked: what a block posted to the main
/// queue runs, since the queue runs it inside a C function, where an unwind would abort the
/// process.
pub(super) fn contained(work: impl FnOnce()) {
    if let Err(payload) = catch_unwind(AssertUnwindSafe(work)) {
        std::mem::forget(payload);
    }
}

/// The posted block's side of a call: `body` run where `handshake` lets it begin, its answer, or
/// its panic as [`ScanError::Driver`], sent to the caller.
pub(super) fn run_posted<T>(
    handshake: &Handshake,
    answer: &SyncSender<Result<T, ScanError>>,
    body: impl FnOnce() -> Result<T, ScanError>,
) {
    if !handshake.begin() {
        return;
    }
    let result = catch_unwind(AssertUnwindSafe(body)).unwrap_or_else(|payload| {
        Err(driver_msg(format!(
            "the backend's call to ImageCaptureCore on the main thread panicked: {}",
            panic_message(payload)
        )))
    });
    let _ = answer.send(result);
}

/// The caller's side of a posted call: its answer, waited for at most `post`; past it, where the
/// call hasn't begun, given up on and refused (the main thread not serving its queue), and where
/// it has, its answer waited for, since a posted call is a request or a read and ends at once.
pub(super) fn await_posted<T>(
    handshake: &Handshake,
    answered: &Receiver<Result<T, ScanError>>,
    post: Duration,
) -> Result<T, ScanError> {
    match answered.recv_timeout(post) {
        Ok(result) => result,
        Err(RecvTimeoutError::Timeout) => {
            if handshake.abandon() {
                return Err(unserved_off_main(post));
            }
            answered.recv().unwrap_or_else(|_| Err(lost()))
        }
        Err(RecvTimeoutError::Disconnected) => Err(lost()),
    }
}

fn lost() -> ScanError {
    driver_msg("the backend's call to ImageCaptureCore on the main thread ended with no answer")
}

/// The answer where the main thread didn't run a posted call within `post`.
pub(super) fn unserved_off_main(post: Duration) -> ScanError {
    driver_msg(format!(
        "ImageCaptureCore delivers to the main thread, which didn't run the backend's call within \
         {} s: call the backend on the main thread, or while the main thread runs its run loop",
        post.as_secs_f32()
    ))
}

/// The answer where the backend is called on the main thread from inside a block of the main
/// queue, which can't then be served.
pub(super) fn unserved_in_block() -> ScanError {
    driver_msg(
        "ImageCaptureCore delivers to the main thread's queue, which can't run while the backend \
         is called from a block on it: call the backend on the main thread outside the main \
         queue's blocks, or from another thread while the main thread runs its run loop",
    )
}

/// What a wait on the main thread does after a slice of the run loop.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Verdict {
    /// Goes on waiting.
    Wait,
    /// Ends, refused: the main queue isn't served where the backend was called.
    Refuse,
}

/// A wait on the main thread, judging each slice by the run loop's answer and by whether the no-op
/// it posted to the main queue as it began has run.
///
/// A slice that passes with nothing handled is evidence only where it was a whole [`SLICE`]:
/// Core Foundation's run looks at the main queue's port on its own only from its second pass, and
/// a slice of no time, or of very little, makes one pass, whose one receive may take another
/// port's message (a timer due, a wake-up) before the no-op's. So a short slice's time-out goes
/// unjudged, and the wait ends at its end as any other does.
#[derive(Debug, Default)]
pub(super) struct MainWait {
    /// Whether the no-op has been seen to run.
    served: bool,
    /// The slices run before it did.
    probed: u32,
    /// Whether a slice answered that the run loop was stopped.
    stopped: bool,
}

impl MainWait {
    /// A wait whose no-op has just been posted.
    pub(super) fn new() -> MainWait {
        MainWait::default()
    }

    /// After a slice that answered `answer`, `whole` where it was asked to run a whole
    /// [`SLICE`], with the no-op run by then or not: refused at once where the mode had nothing to
    /// run (the main queue not counted, as inside one of its blocks), or where the no-op hasn't
    /// run though a whole slice passed with nothing handled, or hasn't after
    /// [`MAX_PROBE_SLICES`] slices of other sources; else the wait goes on.
    pub(super) fn after_slice(&mut self, answer: i32, whole: bool, probe_ran: bool) -> Verdict {
        match answer {
            RUN_FINISHED => return Verdict::Refuse,
            RUN_STOPPED => self.stopped = true,
            RUN_TIMED_OUT | RUN_HANDLED_SOURCE => {}
            // An answer Core Foundation doesn't give: judged as one with something handled.
            _ => {}
        }
        if self.served || probe_ran {
            self.served = true;
            return Verdict::Wait;
        }
        if answer == RUN_TIMED_OUT {
            return if whole {
                Verdict::Refuse
            } else {
                Verdict::Wait
            };
        }
        self.probed = self.probed.saturating_add(1);
        if self.probed >= MAX_PROBE_SLICES {
            Verdict::Refuse
        } else {
            Verdict::Wait
        }
    }

    /// Whether a slice answered that the run loop was stopped. The stop was meant for whoever
    /// runs the main run loop around the backend's call, and Core Foundation keeps it for the run
    /// it's made in, the backend's own slice, which takes it; so the glue makes it again as the
    /// wait ends, for that outer run to see.
    pub(super) fn stopped(&self) -> bool {
        self.stopped
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::mpsc::sync_channel;
    use std::thread;
    use std::time::Instant;

    use super::*;

    #[test]
    fn a_call_never_begun_is_given_up_on_and_never_runs_after() {
        let handshake = Handshake::new();
        assert!(handshake.abandon());
        // The block, run late, finds it abandoned and does nothing.
        assert!(!handshake.begin());
        let (answer, answered) = sync_channel::<Result<u32, ScanError>>(1);
        let mut ran = false;
        run_posted(&handshake, &answer, || {
            ran = true;
            Ok(1)
        });
        assert!(!ran);
        assert!(answered.try_recv().is_err());
    }

    #[test]
    fn a_call_begun_is_never_given_up_on() {
        let handshake = Handshake::new();
        assert!(handshake.begin());
        assert!(!handshake.abandon());
        // Begun once only.
        assert!(!handshake.begin());
    }

    #[test]
    fn a_wait_past_its_bound_for_a_block_never_run_is_refused_naming_the_main_thread() {
        let handshake = Handshake::new();
        let (_answer, answered) = sync_channel::<Result<u32, ScanError>>(1);
        let start = Instant::now();
        let err = await_posted(&handshake, &answered, Duration::from_millis(30)).unwrap_err();
        assert!(start.elapsed() >= Duration::from_millis(30));
        let ScanError::Driver { message, shown } = err else {
            panic!("a Driver error: {err:?}");
        };
        assert!(!shown);
        assert!(message.contains("main thread"), "{message}");
        assert!(!handshake.begin(), "the late block must not run");
    }

    #[test]
    fn a_block_begun_as_its_caller_gives_up_is_waited_for() {
        let handshake = Arc::new(Handshake::new());
        let (answer, answered) = sync_channel::<Result<u32, ScanError>>(1);
        let (began, has_begun) = sync_channel(1);
        let block = {
            let handshake = Arc::clone(&handshake);
            thread::spawn(move || {
                run_posted(&handshake, &answer, || {
                    began.send(()).expect("the test waits");
                    // Past the caller's bound, as a call begun just before it would be.
                    thread::sleep(Duration::from_millis(80));
                    Ok(7)
                });
            })
        };
        has_begun.recv().expect("the block began");
        let result = await_posted(&handshake, &answered, Duration::from_millis(10));
        assert_eq!(result, Ok(7));
        block.join().expect("the block's thread");
    }

    #[test]
    fn a_posted_body_s_panic_is_its_answer() {
        let handshake = Handshake::new();
        let (answer, answered) = sync_channel::<Result<u32, ScanError>>(1);
        run_posted(&handshake, &answer, || panic!("a selector mistyped"));
        let err = await_posted(&handshake, &answered, Duration::from_secs(5)).unwrap_err();
        let ScanError::Driver { message, .. } = err else {
            panic!("a Driver error: {err:?}");
        };
        assert!(message.contains("panicked"), "{message}");
        assert!(message.contains("a selector mistyped"), "{message}");
    }

    #[test]
    fn contained_work_never_unwinds() {
        let mut after = false;
        contained(|| panic!("inside a block"));
        contained(|| after = true);
        assert!(after);
        assert_eq!(
            panic_message(Box::new(String::from("owned"))),
            "owned".to_owned()
        );
        assert_eq!(panic_message(Box::new(3_u8)), "a panic with no message");
    }

    #[test]
    fn a_mode_with_nothing_to_run_is_refused_at_once() {
        // Inside a block of the main queue, with nothing else in the mode, whatever the slice.
        for whole in [true, false] {
            assert_eq!(
                MainWait::new().after_slice(RUN_FINISHED, whole, false),
                Verdict::Refuse
            );
        }
        // And after the queue was seen served, too: no spin.
        let mut wait = MainWait::new();
        assert_eq!(
            wait.after_slice(RUN_HANDLED_SOURCE, true, true),
            Verdict::Wait
        );
        assert_eq!(
            wait.after_slice(RUN_FINISHED, false, false),
            Verdict::Refuse
        );
    }

    #[test]
    fn a_whole_slice_passing_with_the_probe_unrun_is_refused() {
        // A UI's sources in the mode, the main queue unserved.
        assert_eq!(
            MainWait::new().after_slice(RUN_TIMED_OUT, true, false),
            Verdict::Refuse
        );
    }

    #[test]
    fn a_short_slice_passing_with_the_probe_unrun_is_not_judged() {
        // A wait of no time (a snapshot's drain), or the last few milliseconds of one: its one
        // pass may have taken a timer's or a wake-up's message before the no-op's.
        let mut wait = MainWait::new();
        for _ in 0..MAX_PROBE_SLICES * 2 {
            assert_eq!(wait.after_slice(RUN_TIMED_OUT, false, false), Verdict::Wait);
        }
        // They count for nothing against the probe's slices, and a whole one is judged after.
        assert_eq!(
            wait.after_slice(RUN_HANDLED_SOURCE, true, false),
            Verdict::Wait
        );
        assert_eq!(
            wait.after_slice(RUN_TIMED_OUT, true, false),
            Verdict::Refuse
        );
    }

    #[test]
    fn once_the_probe_has_run_the_wait_goes_on_whatever_the_slices_answer() {
        let mut wait = MainWait::new();
        assert_eq!(
            wait.after_slice(RUN_HANDLED_SOURCE, true, true),
            Verdict::Wait
        );
        for answer in [
            RUN_TIMED_OUT,
            RUN_HANDLED_SOURCE,
            RUN_STOPPED,
            RUN_TIMED_OUT,
        ] {
            assert_eq!(wait.after_slice(answer, true, false), Verdict::Wait);
        }
    }

    #[test]
    fn other_sources_handled_first_are_given_a_bounded_number_of_slices() {
        for whole in [true, false] {
            let mut wait = MainWait::new();
            for _ in 1..MAX_PROBE_SLICES {
                assert_eq!(
                    wait.after_slice(RUN_HANDLED_SOURCE, whole, false),
                    Verdict::Wait
                );
            }
            assert_eq!(
                wait.after_slice(RUN_HANDLED_SOURCE, whole, false),
                Verdict::Refuse
            );
        }

        let mut stopped = MainWait::new();
        assert_eq!(stopped.after_slice(RUN_STOPPED, true, false), Verdict::Wait);
        assert_eq!(
            stopped.after_slice(RUN_HANDLED_SOURCE, true, true),
            Verdict::Wait
        );
    }

    #[test]
    fn a_stop_taken_by_a_slice_is_said_so_it_is_made_again() {
        let mut wait = MainWait::new();
        assert!(!wait.stopped());
        assert_eq!(
            wait.after_slice(RUN_HANDLED_SOURCE, true, true),
            Verdict::Wait
        );
        assert!(!wait.stopped());
        // The stop goes on being said, whatever the slices after answer.
        assert_eq!(wait.after_slice(RUN_STOPPED, false, false), Verdict::Wait);
        assert_eq!(wait.after_slice(RUN_TIMED_OUT, true, false), Verdict::Wait);
        assert!(wait.stopped());
        // And a refused wait still says it.
        let mut refused = MainWait::new();
        assert_eq!(refused.after_slice(RUN_STOPPED, true, false), Verdict::Wait);
        assert_eq!(
            refused.after_slice(RUN_FINISHED, true, false),
            Verdict::Refuse
        );
        assert!(refused.stopped());
    }

    #[test]
    fn a_slice_is_at_most_50_ms_and_the_probe_s_slices_bounded() {
        assert_eq!(SLICE, Duration::from_millis(50));
        // A probe never outlasts a few seconds of slices, however each ends.
        assert!(SLICE * MAX_PROBE_SLICES <= Duration::from_secs(5));
    }

    #[test]
    fn the_refusals_name_the_main_thread_and_what_to_do() {
        for err in [
            unserved_off_main(Duration::from_secs(5)),
            unserved_in_block(),
        ] {
            let ScanError::Driver { message, shown } = err else {
                panic!("a Driver error");
            };
            assert!(!shown);
            assert!(message.starts_with("ImageCaptureCore delivers to the main thread"));
            assert!(message.contains("run loop"), "{message}");
        }
        let ScanError::Driver { message, .. } = unserved_off_main(Duration::from_secs(5)) else {
            unreachable!()
        };
        assert!(message.contains("within 5 s"), "{message}");
    }
}
