//! The fake's state: the script it plays, its log, and SANE's calling rules,
//! checked.
//!
//! Safe code only: the exports turn C's arguments into these calls and the
//! answers back into C's.

use std::collections::BTreeMap;
use std::ffi::CStr;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::time::Duration;

use crate::ffi;
use crate::script::{self, OpenAnswer, Script, Text};

/// Where the log goes: a file, or nowhere.
pub struct Log {
    file: Option<File>,
    /// Every line written, kept for unit tests.
    pub lines: Vec<String>,
    keep: bool,
}

impl Log {
    /// A log appending to `path`, if given.
    pub fn open(path: Option<&std::path::Path>) -> Log {
        let file =
            path.and_then(|path| OpenOptions::new().create(true).append(true).open(path).ok());
        Log {
            file,
            lines: Vec::new(),
            keep: false,
        }
    }

    /// A log kept in memory, for unit tests.
    #[cfg(test)]
    pub fn memory() -> Log {
        Log {
            file: None,
            lines: Vec::new(),
            keep: true,
        }
    }

    fn line(&mut self, line: String) {
        if let Some(file) = &mut self.file {
            // One write per line, appended, so lines from several threads
            // never interleave. A log that can't be written can't say so.
            let _ = file.write_all(format!("{line}\n").as_bytes());
        }
        if self.keep {
            self.lines.push(line);
        }
    }
}

/// A handle the fake gave.
struct HandleState {
    open: bool,
    /// The call whose error status the next call must answer with
    /// `sane_cancel` or `sane_close`, and the status.
    must_cancel: Option<(&'static str, i32)>,
}

/// The first handle's value; each next one is 16 above the last, and none is
/// ever given twice.
const FIRST_HANDLE: usize = 0x5a00;

/// What a listing is to do.
pub struct Listing {
    pub delay: Duration,
    pub status: i32,
    pub null: bool,
    pub devices: Vec<[Text; 4]>,
}

/// The fake, its script and what it has seen.
pub struct Fake {
    script: Script,
    pub log: Log,
    /// Violations of SANE's rules so far, kept for unit tests.
    pub violations: Vec<String>,
    /// Whether the script couldn't be read or parsed: then `sane_init`
    /// fails, and the log's note says why.
    pub script_failed: bool,
    inits: usize,
    listings: usize,
    initialised: bool,
    /// The calls in progress other than `sane_cancel`, with their threads.
    in_progress: Vec<(&'static str, String)>,
    handles: BTreeMap<usize, HandleState>,
    next_handle: usize,
}

impl Fake {
    pub fn new(script: Script, log: Log) -> Fake {
        Fake {
            script,
            log,
            violations: Vec::new(),
            script_failed: false,
            inits: 0,
            listings: 0,
            initialised: false,
            in_progress: Vec::new(),
            handles: BTreeMap::new(),
            next_handle: FIRST_HANDLE,
        }
    }

    /// Notes something the fake did or found that isn't a call, such as a
    /// script that didn't parse.
    pub fn note(&mut self, thread: &str, text: &str) {
        self.log.line(format!("note {thread} {text}"));
    }

    fn violation(&mut self, thread: &str, function: &str, what: String) {
        self.log
            .line(format!("violation {thread} {function} {what}"));
        self.violations.push(format!("{function}: {what}"));
    }

    /// A call starting: logged, and checked against SANE's rules.
    pub fn enter(
        &mut self,
        thread: &str,
        function: &'static str,
        handle: Option<usize>,
        args: &str,
    ) {
        self.log.line(
            format!("call {thread} {function} {args}")
                .trim_end()
                .to_owned(),
        );
        if function == "sane_init" {
            if self.initialised {
                self.violation(thread, function, "called again before sane_exit".to_owned());
            }
        } else if !self.initialised {
            self.violation(
                thread,
                function,
                "called outside sane_init and sane_exit".to_owned(),
            );
        }
        if function != "sane_cancel" {
            let others: Vec<String> = self
                .in_progress
                .iter()
                .map(|(other, other_thread)| format!("{other} on {other_thread}"))
                .collect();
            if !others.is_empty() {
                self.violation(thread, function, format!("overlaps {}", others.join(", ")));
            }
            self.in_progress.push((function, thread.to_owned()));
        }
        if let Some(id) = handle {
            match self.handles.get_mut(&id) {
                None => self.violation(
                    thread,
                    function,
                    format!("reached handle {id:#x}, which sane_open never gave"),
                ),
                Some(state) if !state.open => {
                    self.violation(
                        thread,
                        function,
                        format!("reached handle {id:#x}, which was closed"),
                    );
                }
                Some(state) => {
                    if let Some((after, status)) = state.must_cancel.take()
                        && function != "sane_cancel"
                        && function != "sane_close"
                    {
                        let status = script::status_name(status);
                        self.violation(
                            thread,
                            function,
                            format!("came after {after} returned {status}, with no sane_cancel"),
                        );
                    }
                }
            }
        }
        if function == "sane_exit" {
            let open: Vec<String> = self
                .handles
                .iter()
                .filter(|(_, state)| state.open)
                .map(|(id, _)| format!("{id:#x}"))
                .collect();
            if !open.is_empty() {
                self.violation(
                    thread,
                    function,
                    format!("came with handles open: {}", open.join(", ")),
                );
            }
        }
    }

    /// A call returning: logged, and what it means for the calls after.
    /// `status` is what it returned, if it returns one.
    pub fn leave(
        &mut self,
        thread: &str,
        function: &'static str,
        handle: Option<usize>,
        status: Option<i32>,
        more: &str,
    ) {
        let status_text = status
            .map(|status| format!("status={}", script::status_name(status)))
            .unwrap_or_default();
        self.log.line(
            format!("return {thread} {function} {status_text} {more}")
                .trim_end()
                .to_owned(),
        );
        if function != "sane_cancel"
            && let Some(position) = self
                .in_progress
                .iter()
                .position(|(other, other_thread)| *other == function && other_thread == thread)
        {
            self.in_progress.remove(position);
        }
        if let (Some(id), Some(status)) = (handle, status)
            && matches!(function, "sane_start" | "sane_get_parameters" | "sane_read")
            && status != ffi::SANE_STATUS_GOOD
            && status != ffi::SANE_STATUS_EOF
            && let Some(state) = self.handles.get_mut(&id)
        {
            state.must_cancel = Some((function, status));
        }
    }

    /// Records a null pointer where SANE needs one to write to.
    pub fn null_argument(&mut self, thread: &str, function: &'static str, what: &str) {
        self.violation(thread, function, format!("was given a null {what}"));
    }

    /// `sane_init`'s answer: its status and version code.
    pub fn init(&mut self) -> script::Init {
        let answer = self.script.init(self.inits);
        self.inits += 1;
        if answer.status == ffi::SANE_STATUS_GOOD {
            self.initialised = true;
        }
        answer
    }

    /// `sane_exit`.
    pub fn exit(&mut self) {
        self.initialised = false;
    }

    /// What the next listing is to do.
    pub fn listing(&mut self) -> Listing {
        let delay = self.script.listing_delay(self.listings);
        self.listings += 1;
        Listing {
            delay,
            status: self.script.devices_status,
            null: self.script.devices_null,
            devices: self.script.devices.clone(),
        }
    }

    /// `sane_open`'s answer for `name`: its status, and the handle to give
    /// (none for a null one).
    pub fn open(&mut self, name: &[u8]) -> (i32, Option<usize>) {
        match self.script.open(name) {
            OpenAnswer::NullHandle => (ffi::SANE_STATUS_GOOD, None),
            OpenAnswer::Status(ffi::SANE_STATUS_GOOD) => {
                let id = self.next_handle;
                self.next_handle += 0x10;
                self.handles.insert(
                    id,
                    HandleState {
                        open: true,
                        must_cancel: None,
                    },
                );
                (ffi::SANE_STATUS_GOOD, Some(id))
            }
            OpenAnswer::Status(status) => (status, None),
        }
    }

    /// `sane_close`.
    pub fn close(&mut self, id: usize) {
        if let Some(state) = self.handles.get_mut(&id) {
            state.open = false;
            state.must_cancel = None;
        }
    }

    /// `sane_strstatus`'s answer, or `None` for a null one.
    pub fn strstatus(&self, status: i32) -> Option<&'static CStr> {
        if self.script.strstatus_null {
            return None;
        }
        Some(match status {
            ffi::SANE_STATUS_GOOD => c"Success",
            ffi::SANE_STATUS_UNSUPPORTED => c"Operation not supported",
            ffi::SANE_STATUS_CANCELLED => c"Operation was cancelled",
            ffi::SANE_STATUS_DEVICE_BUSY => c"Device busy",
            ffi::SANE_STATUS_INVAL => c"Invalid argument",
            ffi::SANE_STATUS_EOF => c"End of file reached",
            ffi::SANE_STATUS_JAMMED => c"Document feeder jammed",
            ffi::SANE_STATUS_NO_DOCS => c"Document feeder out of documents",
            ffi::SANE_STATUS_COVER_OPEN => c"Scanner cover is open",
            ffi::SANE_STATUS_IO_ERROR => c"Error during device I/O",
            ffi::SANE_STATUS_NO_MEM => c"Out of memory",
            ffi::SANE_STATUS_ACCESS_DENIED => c"Access to resource has been denied",
            _ => c"Unknown SANE status code",
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake(script: &str) -> Fake {
        Fake::new(Script::parse(script).unwrap(), Log::memory())
    }

    /// A call that starts and returns at once.
    fn call(
        fake: &mut Fake,
        thread: &str,
        function: &'static str,
        handle: Option<usize>,
        status: Option<i32>,
    ) {
        fake.enter(thread, function, handle, "");
        fake.leave(thread, function, handle, status, "");
    }

    fn initialised(script: &str) -> Fake {
        let mut fake = fake(script);
        fake.enter("t1", "sane_init", None, "");
        let init = fake.init();
        fake.leave("t1", "sane_init", None, Some(init.status), "");
        fake
    }

    fn opened(fake: &mut Fake) -> usize {
        fake.enter("t1", "sane_open", None, "");
        let (status, id) = fake.open(b"fake:0");
        fake.leave("t1", "sane_open", None, Some(status), "");
        id.unwrap()
    }

    const SCRIPT: &str = "device fake:0 Fake Fake scanner\n";

    #[test]
    fn a_well_behaved_caller_breaks_no_rule() {
        let mut fake = initialised(SCRIPT);
        let id = opened(&mut fake);
        call(
            &mut fake,
            "t1",
            "sane_start",
            Some(id),
            Some(ffi::SANE_STATUS_JAMMED),
        );
        call(&mut fake, "t2", "sane_cancel", Some(id), None);
        call(
            &mut fake,
            "t1",
            "sane_start",
            Some(id),
            Some(ffi::SANE_STATUS_GOOD),
        );
        call(
            &mut fake,
            "t1",
            "sane_read",
            Some(id),
            Some(ffi::SANE_STATUS_EOF),
        );
        call(
            &mut fake,
            "t1",
            "sane_start",
            Some(id),
            Some(ffi::SANE_STATUS_NO_DOCS),
        );
        call(&mut fake, "t1", "sane_close", Some(id), None);
        fake.close(id);
        call(&mut fake, "t1", "sane_exit", None, None);
        assert_eq!(fake.violations, Vec::<String>::new());
        assert!(
            fake.log
                .lines
                .iter()
                .all(|line| !line.starts_with("violation")),
            "{:?}",
            fake.log.lines
        );
    }

    #[test]
    fn overlapping_calls_are_caught() {
        let mut fake = initialised(SCRIPT);
        fake.enter("t1", "sane_get_devices", None, "");
        call(
            &mut fake,
            "t2",
            "sane_open",
            None,
            Some(ffi::SANE_STATUS_INVAL),
        );
        fake.leave(
            "t1",
            "sane_get_devices",
            None,
            Some(ffi::SANE_STATUS_GOOD),
            "",
        );
        assert_eq!(
            fake.violations,
            vec!["sane_open: overlaps sane_get_devices on t1".to_owned()]
        );
        assert!(
            fake.log
                .lines
                .contains(&"violation t2 sane_open overlaps sane_get_devices on t1".to_owned())
        );
    }

    #[test]
    fn a_cancel_may_overlap_any_call() {
        let mut fake = initialised(SCRIPT);
        let id = opened(&mut fake);
        fake.enter("t1", "sane_read", Some(id), "");
        call(&mut fake, "t2", "sane_cancel", Some(id), None);
        fake.leave(
            "t1",
            "sane_read",
            Some(id),
            Some(ffi::SANE_STATUS_CANCELLED),
            "",
        );
        call(&mut fake, "t1", "sane_cancel", Some(id), None);
        assert_eq!(fake.violations, Vec::<String>::new());
    }

    #[test]
    fn a_closed_handle_reached_is_caught_even_by_a_cancel() {
        let mut fake = initialised(SCRIPT);
        let id = opened(&mut fake);
        call(&mut fake, "t1", "sane_close", Some(id), None);
        fake.close(id);
        call(&mut fake, "t2", "sane_cancel", Some(id), None);
        call(
            &mut fake,
            "t1",
            "sane_start",
            Some(id),
            Some(ffi::SANE_STATUS_GOOD),
        );
        call(&mut fake, "t1", "sane_close", Some(0x1234), None);
        assert_eq!(
            fake.violations,
            vec![
                "sane_cancel: reached handle 0x5a00, which was closed".to_owned(),
                "sane_start: reached handle 0x5a00, which was closed".to_owned(),
                "sane_close: reached handle 0x1234, which sane_open never gave".to_owned(),
            ]
        );
    }

    #[test]
    fn exit_with_handles_open_is_caught() {
        let mut fake = initialised(SCRIPT);
        let id = opened(&mut fake);
        call(&mut fake, "t1", "sane_exit", None, None);
        assert_eq!(
            fake.violations,
            vec![format!("sane_exit: came with handles open: {id:#x}")]
        );
    }

    #[test]
    fn a_call_after_an_error_status_without_a_cancel_is_caught() {
        for function in ["sane_start", "sane_get_parameters", "sane_read"] {
            let mut fake = initialised(SCRIPT);
            let id = opened(&mut fake);
            call(
                &mut fake,
                "t1",
                function,
                Some(id),
                Some(ffi::SANE_STATUS_IO_ERROR),
            );
            call(
                &mut fake,
                "t1",
                "sane_control_option",
                Some(id),
                Some(ffi::SANE_STATUS_GOOD),
            );
            assert_eq!(
                fake.violations,
                vec![format!(
                    "sane_control_option: came after {function} returned IO_ERROR, with no sane_cancel"
                )]
            );
        }
    }

    #[test]
    fn a_new_start_after_a_batchs_no_docs_without_a_cancel_is_caught() {
        let mut fake = initialised(SCRIPT);
        let id = opened(&mut fake);
        call(
            &mut fake,
            "t1",
            "sane_start",
            Some(id),
            Some(ffi::SANE_STATUS_NO_DOCS),
        );
        call(
            &mut fake,
            "t1",
            "sane_start",
            Some(id),
            Some(ffi::SANE_STATUS_NO_DOCS),
        );
        assert_eq!(
            fake.violations,
            vec![
                "sane_start: came after sane_start returned NO_DOCS, with no sane_cancel"
                    .to_owned()
            ]
        );
    }

    #[test]
    fn a_close_answers_an_error_status_too() {
        let mut fake = initialised(SCRIPT);
        let id = opened(&mut fake);
        call(
            &mut fake,
            "t1",
            "sane_read",
            Some(id),
            Some(ffi::SANE_STATUS_JAMMED),
        );
        call(&mut fake, "t1", "sane_close", Some(id), None);
        assert_eq!(fake.violations, Vec::<String>::new());
    }

    #[test]
    fn calls_outside_init_and_exit_are_caught() {
        let mut fake = fake(SCRIPT);
        call(
            &mut fake,
            "t1",
            "sane_get_devices",
            None,
            Some(ffi::SANE_STATUS_GOOD),
        );
        fake.enter("t1", "sane_init", None, "");
        fake.init();
        fake.leave("t1", "sane_init", None, Some(ffi::SANE_STATUS_GOOD), "");
        call(
            &mut fake,
            "t1",
            "sane_init",
            None,
            Some(ffi::SANE_STATUS_GOOD),
        );
        assert_eq!(
            fake.violations,
            vec![
                "sane_get_devices: called outside sane_init and sane_exit".to_owned(),
                "sane_init: called again before sane_exit".to_owned(),
            ]
        );
    }

    #[test]
    fn a_failed_init_leaves_sane_unstarted() {
        let mut fake = initialised("init IO_ERROR\ninit GOOD\n");
        call(
            &mut fake,
            "t1",
            "sane_get_devices",
            None,
            Some(ffi::SANE_STATUS_GOOD),
        );
        assert_eq!(
            fake.violations,
            vec!["sane_get_devices: called outside sane_init and sane_exit".to_owned()]
        );
    }

    #[test]
    fn handles_are_never_given_twice() {
        let mut fake =
            initialised("device a x y z\ndevice b x y z\nopen b null-handle\nopen c JAMMED\n");
        let (_, first) = fake.open(b"a");
        fake.close(first.unwrap());
        let (_, second) = fake.open(b"a");
        assert_ne!(first, second);
        assert_eq!(fake.open(b"b"), (ffi::SANE_STATUS_GOOD, None));
        assert_eq!(fake.open(b"c"), (ffi::SANE_STATUS_JAMMED, None));
        assert_eq!(fake.open(b"d"), (ffi::SANE_STATUS_INVAL, None));
    }

    #[test]
    fn listings_take_their_delays_in_turn() {
        let mut fake = initialised("listing-delay 30 0\n");
        assert_eq!(fake.listing().delay, Duration::from_millis(30));
        assert_eq!(fake.listing().delay, Duration::ZERO);
        assert_eq!(fake.listing().delay, Duration::ZERO);
    }

    #[test]
    fn strstatus_gives_text_or_null() {
        let fake = initialised("");
        assert_eq!(fake.strstatus(6), Some(c"Document feeder jammed"));
        assert_eq!(fake.strstatus(42), Some(c"Unknown SANE status code"));
        assert_eq!(initialised("strstatus-null\n").strstatus(6), None);
    }
}
