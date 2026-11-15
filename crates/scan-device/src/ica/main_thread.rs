//! The rules the glue's executor keeps on the main thread, in safe Rust, so they're tested on
//! every system: a call refused at once off the main thread; a panic caught where it would
//! otherwise unwind into the framework or the main queue; and when a wait on the main thread,
//! which runs the main run loop in slices, is refused at once rather than spun.
//!
//! The glue (`objc.rs`, macOS only) pumps with Core Foundation's run loop, and posts a drop's
//! clean-up with Grand Central Dispatch; what's decided about either is here.

use std::any::Any;
use std::panic::{AssertUnwindSafe, catch_unwind};
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

/// The answer where the backend is called off the main thread. ImageCaptureCore delivers to the
/// main thread, and a browser started there for a call made on another thread, the main thread
/// running its run loop, was seen to get no callbacks (CI's macOS runner, 2026-11-15), so the
/// backend makes no call to the framework but on the main thread.
pub(super) fn off_main_call() -> ScanError {
    driver_msg(
        "ImageCaptureCore delivers to the main thread, and the backend is called there only: call \
         it on the main thread (a worker's `main`), outside the main queue's blocks",
    )
}

/// The answer where the backend is called on the main thread from inside a block of the main
/// queue, which can't then be served.
pub(super) fn unserved_in_block() -> ScanError {
    driver_msg(
        "ImageCaptureCore delivers to the main thread's queue, which can't run while the backend \
         is called from a block on it: call the backend on the main thread outside the main \
         queue's blocks",
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
    use super::*;

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
        for err in [off_main_call(), unserved_in_block()] {
            let ScanError::Driver { message, shown } = err else {
                panic!("a Driver error");
            };
            assert!(!shown);
            assert!(message.starts_with("ImageCaptureCore delivers to the main thread"));
            assert!(
                message.contains("on the main thread")
                    && message.contains("outside the main queue's blocks"),
                "{message}"
            );
        }
    }
}
