//! The fake's state: the script it plays, its log, and SANE's calling rules,
//! checked.
//!
//! Safe code only: the exports turn C's arguments into these calls and the
//! answers back into C's.

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::CStr;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::time::Duration;

use crate::ffi;
use crate::options::{
    Change, ConstraintSpec, Descriptor, OptionSpec, RawValue, fit, sized, well_formed,
};
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
    /// Its options, option 0 first, each with how many times its descriptor
    /// changed.
    options: Vec<(OptionSpec, u64)>,
    /// The options whose descriptors haven't been read since a set said
    /// `RELOAD_OPTIONS`.
    stale: BTreeSet<usize>,
}

/// What a set gave the fake, read from the caller's buffer as the option's
/// size says.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Given {
    /// Nothing read: not a set, a null value, or an option the fake doesn't
    /// read for (unknown, or not well formed).
    Nothing,
    Value(RawValue),
    /// A string with no `NUL` within the option's size.
    Unterminated(usize),
}

/// `sane_control_option`'s answer: its status, its info word, and a value
/// to write to the caller's buffer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Control {
    pub status: i32,
    pub info: i32,
    pub write: Option<RawValue>,
}

impl Control {
    fn inval() -> Control {
        Control {
            status: ffi::SANE_STATUS_INVAL,
            info: 0,
            write: None,
        }
    }
}

/// A value as the log shows it: words in brackets, a string quoted.
pub fn show(value: &RawValue) -> String {
    match value {
        RawValue::Words(words) => {
            let words: Vec<String> = words.iter().map(i32::to_string).collect();
            format!("[{}]", words.join(","))
        }
        RawValue::Bytes(bytes) => format!("{:?}", String::from_utf8_lossy(bytes)),
    }
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
                let layout = &self.script.layout;
                let count = layout
                    .count
                    .unwrap_or_else(|| i32::try_from(layout.options.len() + 1).unwrap_or(i32::MAX));
                let zero = OptionSpec {
                    descriptor: Descriptor {
                        name: Text::Bytes(Vec::new()),
                        title: Text::Bytes(b"Number of options".to_vec()),
                        desc: Text::Bytes(b"How many options the device has.".to_vec()),
                        value_type: ffi::SANE_TYPE_INT,
                        unit: ffi::SANE_UNIT_NONE,
                        size: 4,
                        cap: ffi::SANE_CAP_SOFT_DETECT,
                        constraint_type: ffi::SANE_CONSTRAINT_NONE,
                        constraint: ConstraintSpec::None,
                    },
                    value: RawValue::Words(vec![count]),
                    auto: None,
                    rules: Vec::new(),
                    reload_params: false,
                    null: false,
                };
                let options = std::iter::once(zero)
                    .chain(layout.options.iter().cloned())
                    .map(|spec| (spec, 0))
                    .collect();
                self.handles.insert(
                    id,
                    HandleState {
                        open: true,
                        must_cancel: None,
                        options,
                        stale: BTreeSet::new(),
                    },
                );
                (ffi::SANE_STATUS_GOOD, Some(id))
            }
            OpenAnswer::Status(status) => (status, None),
        }
    }

    /// An open handle's option, by SANE's number.
    fn option(&self, id: usize, option: i32) -> Option<&(OptionSpec, u64)> {
        let handle = self.handles.get(&id).filter(|handle| handle.open)?;
        handle.options.get(usize::try_from(option).ok()?)
    }

    /// `sane_get_option_descriptor`: the descriptor to give and how many
    /// times it has changed, or `None` (a null one) for an option that
    /// doesn't exist or is scripted null, or a handle that isn't open.
    pub fn descriptor(&mut self, id: usize, option: i32) -> Option<(Descriptor, u64)> {
        let (spec, version) = self.option(id, option)?;
        let found = (!spec.null).then(|| (spec.descriptor.clone(), *version));
        // Asked for, so read again, even if it's null.
        if let (Some(handle), Ok(index)) = (self.handles.get_mut(&id), usize::try_from(option)) {
            handle.stale.remove(&index);
        }
        found
    }

    /// The type and size of an option the fake reads a set's value for: one
    /// that exists, isn't null and is well formed, of a size it can read.
    pub fn shape(&self, id: usize, option: i32) -> Option<(i32, i32)> {
        let (spec, _) = self.option(id, option)?;
        let descriptor = &spec.descriptor;
        (!spec.null && well_formed(descriptor) && descriptor.size <= 1 << 20)
            .then_some((descriptor.value_type, descriptor.size))
    }

    /// `sane_control_option`, after its call is logged: checked against what
    /// the caller should have refused, and answered.
    pub fn control(
        &mut self,
        thread: &str,
        id: usize,
        option: i32,
        action: i32,
        null_value: bool,
        given: Given,
    ) -> Control {
        const FUNCTION: &str = "sane_control_option";
        let Some(handle) = self.handles.get(&id).filter(|handle| handle.open) else {
            // The call's entry has said so.
            return Control::inval();
        };
        let Some(index) = usize::try_from(option)
            .ok()
            .filter(|&index| index < handle.options.len())
        else {
            self.violation(
                thread,
                FUNCTION,
                format!("reached option {option}, which doesn't exist"),
            );
            return Control::inval();
        };
        if index != 0 && !handle.stale.is_empty() {
            let stale: Vec<String> = handle.stale.iter().map(usize::to_string).collect();
            self.violation(
                thread,
                FUNCTION,
                format!(
                    "came after RELOAD_OPTIONS before every descriptor was read again (not {})",
                    stale.join(", ")
                ),
            );
            return Control::inval();
        }
        let (spec, _) = &handle.options[index];
        let descriptor = &spec.descriptor;
        let caps = descriptor.cap;
        let has = |bit: i32| caps & bit != 0;
        let value_type = descriptor.value_type;
        let refusal = if spec.null {
            Some("whose descriptor is null".to_owned())
        } else if !well_formed(descriptor) {
            Some("whose descriptor SANE doesn't allow".to_owned())
        } else if value_type == ffi::SANE_TYPE_GROUP
            || (action == ffi::SANE_ACTION_GET_VALUE && value_type == ffi::SANE_TYPE_BUTTON)
        {
            Some("which has no value".to_owned())
        } else if has(ffi::SANE_CAP_INACTIVE) {
            Some("which is inactive".to_owned())
        } else {
            match action {
                ffi::SANE_ACTION_GET_VALUE if !has(ffi::SANE_CAP_SOFT_DETECT) => {
                    Some("which software can't read".to_owned())
                }
                ffi::SANE_ACTION_SET_VALUE | ffi::SANE_ACTION_SET_AUTO
                    if !has(ffi::SANE_CAP_SOFT_SELECT) =>
                {
                    Some("which software can't set".to_owned())
                }
                ffi::SANE_ACTION_SET_AUTO if !has(ffi::SANE_CAP_AUTOMATIC) => {
                    Some("which can't be set automatically".to_owned())
                }
                ffi::SANE_ACTION_GET_VALUE
                | ffi::SANE_ACTION_SET_VALUE
                | ffi::SANE_ACTION_SET_AUTO => None,
                other => Some(format!("with action {other}, which isn't SANE's")),
            }
        };
        if let Some(refusal) = refusal {
            self.violation(
                thread,
                FUNCTION,
                format!("reached option {index}, {refusal}"),
            );
            return Control::inval();
        }
        if null_value && action != ffi::SANE_ACTION_SET_AUTO {
            self.null_argument(thread, FUNCTION, "value");
            return Control::inval();
        }
        if let Some((_, status)) = self
            .script
            .layout
            .statuses
            .iter()
            .find(|(n, _)| *n == option)
        {
            return Control {
                status: *status,
                info: 0,
                write: None,
            };
        }
        let descriptor = descriptor.clone();
        let spec = spec.clone();
        match action {
            ffi::SANE_ACTION_GET_VALUE => Control {
                status: ffi::SANE_STATUS_GOOD,
                info: 0,
                write: Some(sized(&spec.value, descriptor.size)),
            },
            ffi::SANE_ACTION_SET_VALUE if value_type == ffi::SANE_TYPE_BUTTON => Control {
                status: ffi::SANE_STATUS_GOOD,
                info: 0,
                write: None,
            },
            ffi::SANE_ACTION_SET_VALUE => {
                let value = match given {
                    Given::Value(value) => value,
                    Given::Unterminated(size) => {
                        self.violation(
                            thread,
                            FUNCTION,
                            format!("was given a string with no NUL in its {size} bytes"),
                        );
                        return Control::inval();
                    }
                    Given::Nothing => return Control::inval(),
                };
                if value_type == ffi::SANE_TYPE_BOOL
                    && let RawValue::Words(words) = &value
                    && words.iter().any(|word| !matches!(word, 0 | 1))
                {
                    self.violation(
                        thread,
                        FUNCTION,
                        format!("was given a BOOL as {}, not a word of 0 or 1", show(&value)),
                    );
                    return Control::inval();
                }
                let Some((value, inexact)) = fit(&descriptor, value) else {
                    return Control::inval();
                };
                let mut info = self.store(id, index, value.clone());
                if inexact {
                    info |= ffi::SANE_INFO_INEXACT;
                }
                Control {
                    status: ffi::SANE_STATUS_GOOD,
                    info,
                    write: inexact.then(|| sized(&value, descriptor.size)),
                }
            }
            _ => {
                let value = spec.auto.clone().unwrap_or(spec.value.clone());
                let info = self.store(id, index, sized(&value, descriptor.size));
                Control {
                    status: ffi::SANE_STATUS_GOOD,
                    info,
                    write: None,
                }
            }
        }
    }

    /// Stores an option's new value and plays the rules it sets off,
    /// returning the info bits they make: `RELOAD_OPTIONS` if a descriptor
    /// changed (unless a rule is quiet), `RELOAD_PARAMS` if the option says.
    fn store(&mut self, id: usize, index: usize, value: RawValue) -> i32 {
        let Some(handle) = self.handles.get_mut(&id) else {
            return 0;
        };
        let (spec, _) = &mut handle.options[index];
        spec.value = value.clone();
        let rules = spec.rules.clone();
        let mut info = if spec.reload_params {
            ffi::SANE_INFO_RELOAD_PARAMS
        } else {
            0
        };
        let compare = |raw: &RawValue| match (raw, &value) {
            (RawValue::Words(a), RawValue::Words(b)) => a.first() == b.first(),
            (a, b) => a == b,
        };
        let mut changed = false;
        let mut quiet = false;
        for rule in rules.iter().filter(|rule| compare(&rule.when)) {
            let target = match rule.change {
                Change::Quiet => {
                    quiet = true;
                    continue;
                }
                Change::Activate(target)
                | Change::Deactivate(target)
                | Change::Range(target, ..)
                | Change::Words(target, _)
                | Change::Size(target, _)
                | Change::Null(target) => target + 1,
            };
            let Some((spec, version)) = handle.options.get_mut(target) else {
                continue;
            };
            let before = (spec.descriptor.clone(), spec.null);
            if let Change::Null(_) = rule.change {
                spec.null = true;
            }
            let descriptor = &mut spec.descriptor;
            match &rule.change {
                Change::Activate(_) => descriptor.cap &= !ffi::SANE_CAP_INACTIVE,
                Change::Deactivate(_) => descriptor.cap |= ffi::SANE_CAP_INACTIVE,
                Change::Range(_, min, max, quant) => {
                    descriptor.constraint = ConstraintSpec::Range(*min, *max, *quant);
                    descriptor.constraint_type = ffi::SANE_CONSTRAINT_RANGE;
                }
                Change::Words(_, words) => {
                    descriptor.constraint = ConstraintSpec::Words(words.clone(), None);
                    descriptor.constraint_type = ffi::SANE_CONSTRAINT_WORD_LIST;
                }
                Change::Size(_, size) => descriptor.size = *size,
                Change::Null(_) | Change::Quiet => {}
            }
            if (spec.descriptor.clone(), spec.null) != before {
                *version += 1;
                changed = true;
            }
        }
        if changed && !quiet {
            info |= ffi::SANE_INFO_RELOAD_OPTIONS;
            handle.stale = (0..handle.options.len()).collect();
        }
        info
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

    const OPTIONS: &str = r#"
        device fake:0 Fake Fake scanner
        option source STRING 8 soft-select+soft-detect
        strings Flatbed ADF
        value Flatbed
        when ADF activate height
        when Flatbed deactivate height
        option height FIXED 4 soft-select+soft-detect+inactive
        range 0 300 0
        option res INT 4 soft-select+soft-detect+automatic
        range 50 600 0
        value 300
        auto 150
        reload-params
        option ro BOOL 4 soft-detect
        value 1
        option hw BOOL 4 hard-select
        option press BUTTON 0 soft-select+soft-detect
        option @null GROUP 0 -
        option bad INT 6 soft-select+soft-detect
        option-null
        option odd BOOL 4 soft-select+soft-detect
        value 2
    "#;

    /// A get, set or auto on an open handle, logged as the export logs it.
    fn control(fake: &mut Fake, id: usize, option: i32, action: i32, given: Given) -> Control {
        fake.enter("t1", "sane_control_option", Some(id), "");
        let answer = fake.control("t1", id, option, action, false, given);
        fake.leave(
            "t1",
            "sane_control_option",
            Some(id),
            Some(answer.status),
            "",
        );
        answer
    }

    fn words(words: &[i32]) -> Given {
        Given::Value(RawValue::Words(words.to_vec()))
    }

    fn text(text: &str) -> Given {
        Given::Value(RawValue::Bytes(text.as_bytes().to_vec()))
    }

    /// Reads every descriptor, as a caller does after `RELOAD_OPTIONS`.
    fn read_all(fake: &mut Fake, id: usize) {
        for option in 0..11 {
            fake.descriptor(id, option);
        }
    }

    #[test]
    fn options_are_played_as_scripted() {
        use ffi::{
            SANE_ACTION_GET_VALUE as GET, SANE_ACTION_SET_AUTO as AUTO,
            SANE_ACTION_SET_VALUE as SET,
        };
        let mut fake = initialised(OPTIONS);
        let id = opened(&mut fake);
        let (zero, _) = fake.descriptor(id, 0).unwrap();
        assert_eq!((zero.value_type, zero.size, zero.cap), (1, 4, 4));
        let count = control(&mut fake, id, 0, GET, Given::Nothing);
        assert_eq!(count.write, Some(RawValue::Words(vec![11])));
        assert_eq!(
            control(&mut fake, id, 1, GET, Given::Nothing).write,
            Some(RawValue::Bytes(b"Flatbed\0".to_vec()))
        );
        // A set that changes another descriptor says so, and the descriptor
        // has a new version.
        let (_, before) = fake.descriptor(id, 2).unwrap();
        let set = control(&mut fake, id, 1, SET, text("ADF"));
        assert_eq!((set.status, set.info), (0, ffi::SANE_INFO_RELOAD_OPTIONS));
        read_all(&mut fake, id);
        let (height, after) = fake.descriptor(id, 2).unwrap();
        assert_eq!(height.cap & ffi::SANE_CAP_INACTIVE, 0);
        assert_eq!(after, before + 1);
        // A value out of range is clamped, said inexact, and written back.
        let clamped = control(&mut fake, id, 3, SET, words(&[700]));
        assert_eq!(
            clamped.info,
            ffi::SANE_INFO_INEXACT | ffi::SANE_INFO_RELOAD_PARAMS
        );
        assert_eq!(clamped.write, Some(RawValue::Words(vec![600])));
        // A string not in the list is INVAL, which breaks no rule.
        assert_eq!(control(&mut fake, id, 1, SET, text("Glass")).status, 4);
        let auto = control(&mut fake, id, 3, AUTO, Given::Nothing);
        assert_eq!(auto.info, ffi::SANE_INFO_RELOAD_PARAMS);
        assert_eq!(
            control(&mut fake, id, 3, GET, Given::Nothing).write,
            Some(RawValue::Words(vec![150]))
        );
        assert_eq!(control(&mut fake, id, 4, GET, Given::Nothing).status, 0);
        assert_eq!(control(&mut fake, id, 6, SET, words(&[1])).status, 0);
        // A hostile value is given as scripted.
        assert_eq!(
            control(&mut fake, id, 10, GET, Given::Nothing).write,
            Some(RawValue::Words(vec![2]))
        );
        assert_eq!(fake.violations, Vec::<String>::new());
        assert_eq!(fake.descriptor(id, 9), None, "option-null gives null");
        assert_eq!(fake.descriptor(id, 11), None);
    }

    #[test]
    fn calls_a_caller_should_refuse_are_caught() {
        use ffi::{
            SANE_ACTION_GET_VALUE as GET, SANE_ACTION_SET_AUTO as AUTO,
            SANE_ACTION_SET_VALUE as SET,
        };
        let cases: [(i32, i32, Given, &str); 14] = [
            (
                2,
                GET,
                Given::Nothing,
                "reached option 2, which is inactive",
            ),
            (2, SET, words(&[1]), "reached option 2, which is inactive"),
            (
                4,
                SET,
                words(&[1]),
                "reached option 4, which software can't set",
            ),
            (
                5,
                GET,
                Given::Nothing,
                "reached option 5, which software can't read",
            ),
            (
                1,
                AUTO,
                Given::Nothing,
                "reached option 1, which can't be set automatically",
            ),
            (
                6,
                GET,
                Given::Nothing,
                "reached option 6, which has no value",
            ),
            (7, SET, words(&[1]), "reached option 7, which has no value"),
            (
                8,
                GET,
                Given::Nothing,
                "reached option 8, whose descriptor SANE doesn't allow",
            ),
            (
                9,
                GET,
                Given::Nothing,
                "reached option 9, whose descriptor is null",
            ),
            (
                11,
                GET,
                Given::Nothing,
                "reached option 11, which doesn't exist",
            ),
            (
                -1,
                GET,
                Given::Nothing,
                "reached option -1, which doesn't exist",
            ),
            (
                3,
                3,
                Given::Nothing,
                "reached option 3, with action 3, which isn't SANE's",
            ),
            (
                10,
                SET,
                words(&[2]),
                "was given a BOOL as [2], not a word of 0 or 1",
            ),
            (
                1,
                SET,
                Given::Unterminated(8),
                "was given a string with no NUL in its 8 bytes",
            ),
        ];
        for (option, action, given, expected) in cases {
            let mut fake = initialised(OPTIONS);
            let id = opened(&mut fake);
            let answer = control(&mut fake, id, option, action, given);
            assert_eq!(answer.status, ffi::SANE_STATUS_INVAL, "{expected}");
            assert_eq!(
                fake.violations,
                vec![format!("sane_control_option: {expected}")]
            );
        }
        let mut fake = initialised(OPTIONS);
        let id = opened(&mut fake);
        fake.enter("t1", "sane_control_option", Some(id), "");
        fake.control("t1", id, 3, GET, true, Given::Nothing);
        assert_eq!(
            fake.violations,
            vec!["sane_control_option: was given a null value".to_owned()]
        );
    }

    #[test]
    fn a_call_before_the_descriptors_are_read_again_is_caught() {
        use ffi::{SANE_ACTION_GET_VALUE as GET, SANE_ACTION_SET_VALUE as SET};
        let mut fake = initialised(OPTIONS);
        let id = opened(&mut fake);
        control(&mut fake, id, 1, SET, text("ADF"));
        // Option 0's count may be read in the middle of reading them.
        fake.descriptor(id, 0);
        assert_eq!(control(&mut fake, id, 0, GET, Given::Nothing).status, 0);
        fake.descriptor(id, 3);
        control(&mut fake, id, 3, GET, Given::Nothing);
        assert_eq!(fake.violations.len(), 1);
        assert!(
            fake.violations[0].starts_with(
                "sane_control_option: came after RELOAD_OPTIONS before every descriptor was read again (not 1, 2, 4"
            ),
            "{:?}",
            fake.violations
        );
        // Once all are read, calls are fine again.
        read_all(&mut fake, id);
        assert_eq!(control(&mut fake, id, 3, GET, Given::Nothing).status, 0);
        assert_eq!(fake.violations.len(), 1);
    }

    #[test]
    fn a_quiet_change_says_nothing() {
        let mut fake = initialised(
            "device fake:0 a b c\noption s STRING 8 soft-select+soft-detect\nwhen x size t 8\nwhen x quiet\noption t INT 4 soft-select+soft-detect\n",
        );
        let id = opened(&mut fake);
        let answer = control(&mut fake, id, 1, ffi::SANE_ACTION_SET_VALUE, text("x"));
        assert_eq!(answer.info, 0);
        assert_eq!(fake.descriptor(id, 2).unwrap().0.size, 8);
        assert_eq!(
            control(&mut fake, id, 2, ffi::SANE_ACTION_GET_VALUE, Given::Nothing).status,
            0
        );
        assert_eq!(fake.violations, Vec::<String>::new());
    }
}
