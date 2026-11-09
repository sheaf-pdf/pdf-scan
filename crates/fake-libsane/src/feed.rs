//! The pages the fake scans: their frames, the feeder that holds them, the
//! statuses, hangs and abort scripted at a page, and the hostile answers no
//! real backend gives on purpose; and one handle's scan as it goes.
//!
//! Safe code only: the fake decides each call's answer here, and the
//! exports write it back to C.

use crate::ffi;
use crate::script::{Text, status, word};

/// How many bytes a frame sends before its `EOF`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Bytes {
    Count(u64),
    /// Never an `EOF`.
    Endless,
}

/// One frame of every page: its parameters, as `sane_get_parameters` gives
/// them, and what it sends.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameSpec {
    pub parameters: ffi::SANE_Parameters,
    pub bytes: Bytes,
}

impl FrameSpec {
    /// The bytes a line's pixels take, by the standard's formula, or the
    /// whole line where the parameters don't allow the formula.
    fn pixel_bytes(&self) -> u64 {
        let p = &self.parameters;
        let line = u64::try_from(p.bytes_per_line).unwrap_or(0);
        let channels = match p.format {
            ffi::SANE_FRAME_RGB => 3,
            ffi::SANE_FRAME_GRAY
            | ffi::SANE_FRAME_RED
            | ffi::SANE_FRAME_GREEN
            | ffi::SANE_FRAME_BLUE => 1,
            _ => return line,
        };
        let Ok(pixels) = u64::try_from(p.pixels_per_line) else {
            return line;
        };
        let needed = match p.depth {
            1 => pixels.div_ceil(8) * channels,
            8 => pixels * channels,
            16 => pixels * 2 * channels,
            _ => return line,
        };
        needed.min(line)
    }
}

/// What the script makes happen at a page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    /// Its `sane_start` answers this status.
    Start(i32),
    /// Once this many of its bytes are sent, every read answers this status.
    Read { status: i32, after: u64 },
    /// Once this many of its bytes are sent, a read blocks until
    /// `sane_cancel`, then answers `CANCELLED`.
    Hang { after: u64 },
    /// Once this many of its bytes are sent, a read blocks for good.
    HangForever { after: u64 },
    /// Once this many of its bytes are sent, every read answers `GOOD` with
    /// no bytes, cancelled or not.
    IgnoreCancel { after: u64 },
    /// Its `sane_start` aborts the process.
    Abort,
}

/// What every `GOOD` read claims to have given.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReadLength {
    Claim(i32),
    /// One byte more than asked for.
    Past,
}

/// The scan's part of a script.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Feed {
    /// Every page's frames, in order; without any, one gray frame of 4
    /// lines of 16 pixels.
    pub frames: Vec<FrameSpec>,
    /// Sheets in the feeder; without, no end.
    pub sheets: Option<usize>,
    /// Two pages a sheet, front then back.
    pub duplex: bool,
    /// What happens at each page, by its number from 1.
    pub events: Vec<(usize, Event)>,
    /// The most a read gives.
    pub read_max: Option<usize>,
    pub read_length: Option<ReadLength>,
    /// Whether `sane_set_io_mode` takes non-blocking.
    pub non_blocking: bool,
    /// Whether `sane_set_io_mode` answers `UNSUPPORTED` to either mode,
    /// frame or none, as some backends do.
    pub io_mode_unsupported: bool,
    /// What `sane_get_select_fd` gives; without, `UNSUPPORTED`.
    pub select_fd: Option<i32>,
    /// What `sane_get_parameters` answers in place of `GOOD`.
    pub parameters_status: Option<i32>,
    /// The options a `sane_start` answering `INVAL` makes inactive, without
    /// saying.
    pub inval_deactivates: Vec<Vec<u8>>,
}

/// The default page's one frame.
const DEFAULT_FRAME: FrameSpec = FrameSpec {
    parameters: ffi::SANE_Parameters {
        format: ffi::SANE_FRAME_GRAY,
        last_frame: ffi::SANE_TRUE,
        bytes_per_line: 16,
        pixels_per_line: 16,
        lines: 4,
        depth: 8,
    },
    bytes: Bytes::Count(64),
};

/// A frame format by its name without `SANE_FRAME_`, or by its number.
fn format(word: &str) -> Result<i32, String> {
    let names = [
        ("GRAY", ffi::SANE_FRAME_GRAY),
        ("RGB", ffi::SANE_FRAME_RGB),
        ("RED", ffi::SANE_FRAME_RED),
        ("GREEN", ffi::SANE_FRAME_GREEN),
        ("BLUE", ffi::SANE_FRAME_BLUE),
    ];
    if let Some((_, code)) = names.iter().find(|(name, _)| *name == word) {
        return Ok(*code);
    }
    word.parse()
        .map_err(|_| format!("{word:?} isn't a frame format"))
}

fn integer<T: std::str::FromStr>(word: &str, what: &str) -> Result<T, String> {
    word.parse().map_err(|_| format!("{word:?} isn't {what}"))
}

impl Feed {
    /// Takes a line if its keyword is a scanning keyword: `Ok(false)` if it
    /// isn't one.
    pub fn line(&mut self, keyword: &str, args: &[Text]) -> Result<bool, String> {
        let words = args
            .iter()
            .map(word)
            .collect::<Result<Vec<String>, String>>();
        let want = |n: usize| {
            if args.len() == n {
                Ok(())
            } else {
                Err(format!("{keyword} takes {n} values, not {}", args.len()))
            }
        };
        match keyword {
            "frame" => {
                let words = words?;
                if !(6..=7).contains(&words.len()) {
                    return Err(
                        "frame takes FORMAT LAST BPL PPL LINES DEPTH, then BYTES or endless if you like"
                            .to_owned(),
                    );
                }
                let n = |i: usize| integer::<i32>(&words[i], "an integer");
                let parameters = ffi::SANE_Parameters {
                    format: format(&words[0])?,
                    last_frame: n(1)?,
                    bytes_per_line: n(2)?,
                    pixels_per_line: n(3)?,
                    lines: n(4)?,
                    depth: n(5)?,
                };
                let line = i64::from(parameters.bytes_per_line).max(0);
                let rows = if parameters.lines == -1 {
                    4
                } else {
                    i64::from(parameters.lines).max(0)
                };
                let bytes = match words.get(6).map(String::as_str) {
                    None => Bytes::Count((line * rows) as u64),
                    Some("endless") => Bytes::Endless,
                    Some(count) => Bytes::Count(integer(count, "a count of bytes")?),
                };
                self.frames.push(FrameSpec { parameters, bytes });
            }
            "sheets" => {
                want(1)?;
                self.sheets = Some(integer(&words?[0], "a count")?);
            }
            "duplex" => {
                want(0)?;
                self.duplex = true;
            }
            "at" => {
                let words = words?;
                let (page, kind, rest) = match words.as_slice() {
                    [page, kind, rest @ ..] => (integer::<usize>(page, "a page")?, kind, rest),
                    _ => return Err("at takes a page, then what happens there".to_owned()),
                };
                if page == 0 {
                    return Err("pages count from 1".to_owned());
                }
                let after = |rest: &[String]| match rest {
                    [] => Ok(0),
                    [bytes] => integer(bytes, "a count of bytes"),
                    _ => Err(format!("at … {kind} takes a count of bytes at most")),
                };
                let event = match (kind.as_str(), rest) {
                    ("start", [answer]) => Event::Start(status(answer)?),
                    ("read", [answer, rest @ ..]) => Event::Read {
                        status: status(answer)?,
                        after: after(rest)?,
                    },
                    ("hang", rest) => Event::Hang {
                        after: after(rest)?,
                    },
                    ("hang-forever", rest) => Event::HangForever {
                        after: after(rest)?,
                    },
                    ("ignore-cancel", rest) => Event::IgnoreCancel {
                        after: after(rest)?,
                    },
                    ("abort", []) => Event::Abort,
                    _ => {
                        return Err(format!(
                            "{kind:?} isn't something that happens at a page, or takes other values"
                        ));
                    }
                };
                self.events.push((page, event));
            }
            "read-max" => {
                want(1)?;
                self.read_max = Some(integer(&words?[0], "a count of bytes")?);
            }
            "read-length" => {
                want(1)?;
                let words = words?;
                self.read_length = Some(match words[0].as_str() {
                    "past" => ReadLength::Past,
                    claim => ReadLength::Claim(integer(claim, "a length")?),
                });
            }
            "non-blocking" => {
                want(0)?;
                self.non_blocking = true;
            }
            "io-mode-unsupported" => {
                want(0)?;
                self.io_mode_unsupported = true;
            }
            "select-fd" => {
                want(1)?;
                self.select_fd = Some(integer(&words?[0], "a file descriptor")?);
            }
            "parameters-status" => {
                want(1)?;
                self.parameters_status = Some(status(&words?[0])?);
            }
            "start-inval-deactivates" => {
                for arg in args {
                    let Text::Bytes(name) = arg else {
                        return Err("start-inval-deactivates takes names".to_owned());
                    };
                    self.inval_deactivates.push(name.clone());
                }
            }
            _ => return Ok(false),
        }
        Ok(true)
    }

    /// What a backend's profile says of scanning: the backends whose
    /// `sane_set_io_mode` answers `UNSUPPORTED` to either mode (SANE
    /// 1.4.0's `fujitsu`, `canon_dr`, `kodak` and `epson2`) do so here.
    pub fn profile(&mut self, name: &str) {
        if matches!(name, "fujitsu" | "canon-dr" | "kodak" | "epson") {
            self.io_mode_unsupported = true;
        }
    }

    /// Every page's frames.
    pub fn frames(&self) -> &[FrameSpec] {
        if self.frames.is_empty() {
            std::slice::from_ref(&DEFAULT_FRAME)
        } else {
            &self.frames
        }
    }

    /// The pages the feeder holds, if it ends.
    fn pages(&self) -> Option<usize> {
        self.sheets
            .map(|sheets| if self.duplex { sheets * 2 } else { sheets })
    }

    fn event(&self, page: usize, wanted: impl Fn(&Event) -> bool) -> Option<Event> {
        self.events
            .iter()
            .find(|(at, event)| *at == page && wanted(event))
            .map(|(_, event)| *event)
    }
}

/// A byte of a frame as the fake sends it: padding (past a line's pixels) is
/// `0xEE`; the frame's first byte is `F` on a front and `B` on a back; every
/// other is `(page × 31 + frame × 7 + line × 3 + column) mod 251`, the page
/// counted from 1 and the frame, line and column (in bytes) from 0.
pub fn byte_at(page: usize, back: bool, frame: usize, spec: &FrameSpec, at: u64) -> u8 {
    let line = u64::try_from(spec.parameters.bytes_per_line)
        .ok()
        .filter(|&line| line > 0);
    let (row, column) = match line {
        Some(line) => (at / line, at % line),
        None => (0, at),
    };
    if column >= spec.pixel_bytes() && line.is_some() {
        return 0xEE;
    }
    if row == 0 && column == 0 {
        return if back { b'B' } else { b'F' };
    }
    ((page as u64 * 31 + frame as u64 * 7 + row * 3 + column) % 251) as u8
}

/// A page being scanned.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PageState {
    /// Its number, from 1.
    pub number: usize,
    /// The frame begun last.
    pub frame: usize,
    /// Whether that frame has started and not reached `EOF`.
    pub frame_open: bool,
    /// Bytes of that frame sent.
    pub frame_sent: u64,
    /// Bytes of the page sent.
    pub page_sent: u64,
    /// A status every read answers now.
    pub failed: Option<i32>,
    /// Whether reads answer `GOOD` with no bytes, for good.
    pub ignoring: bool,
}

/// One handle's scan.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ScanState {
    /// Pages taken from the feeder.
    pub taken: usize,
    pub page: Option<PageState>,
    /// Whether `sane_cancel` came since the last `sane_start`.
    pub cancelled: bool,
}

/// `sane_start`'s answer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StartAnswer {
    Status(i32),
    /// Start a frame before the one in progress reached `EOF`.
    Early,
    Abort,
}

/// `sane_read`'s answer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReadAnswer {
    /// `GOOD`, these bytes, and the length to claim.
    Data {
        bytes: Vec<u8>,
        claim: i32,
    },
    Status(i32),
    /// Block until `sane_cancel`.
    Hang,
    /// Block for good.
    HangForever,
    /// No frame has started.
    NotStarted,
}

impl ScanState {
    /// `sane_start`: the next frame of the page, or the next page.
    pub fn start(&mut self, feed: &Feed) -> StartAnswer {
        let frames = feed.frames();
        if let Some(page) = &mut self.page
            && !self.cancelled
            && !page.ignoring
        {
            if page.frame_open {
                return StartAnswer::Early;
            }
            if page.frame + 1 < frames.len() && page.failed.is_none() && !page.ignoring {
                page.frame += 1;
                page.frame_open = true;
                page.frame_sent = 0;
                return StartAnswer::Status(ffi::SANE_STATUS_GOOD);
            }
        }
        self.cancelled = false;
        self.page = None;
        let number = self.taken + 1;
        if feed.pages().is_some_and(|pages| number > pages) {
            return StartAnswer::Status(ffi::SANE_STATUS_NO_DOCS);
        }
        self.taken = number;
        if feed.event(number, |event| *event == Event::Abort).is_some() {
            return StartAnswer::Abort;
        }
        if let Some(Event::Start(status)) =
            feed.event(number, |event| matches!(event, Event::Start(_)))
        {
            return StartAnswer::Status(status);
        }
        self.page = Some(PageState {
            number,
            frame: 0,
            frame_open: true,
            frame_sent: 0,
            page_sent: 0,
            failed: None,
            ignoring: false,
        });
        StartAnswer::Status(ffi::SANE_STATUS_GOOD)
    }

    /// Whether a frame has started and not reached `EOF`.
    pub fn frame_open(&self) -> bool {
        !self.cancelled && self.page.as_ref().is_some_and(|page| page.frame_open)
    }

    /// The parameters `sane_get_parameters` gives: the frame in progress,
    /// or the next one.
    pub fn parameters(&self, feed: &Feed) -> ffi::SANE_Parameters {
        let frames = feed.frames();
        let index = match &self.page {
            Some(page) if page.frame_open && !self.cancelled => page.frame,
            Some(page) if page.frame + 1 < frames.len() && !self.cancelled => page.frame + 1,
            _ => 0,
        };
        frames[index].parameters
    }

    /// `sane_read`, asked for at most `asked` bytes.
    pub fn read(&mut self, feed: &Feed, asked: i32) -> ReadAnswer {
        if self.cancelled {
            return ReadAnswer::Status(ffi::SANE_STATUS_CANCELLED);
        }
        let duplex = feed.duplex;
        let frames = feed.frames();
        let Some(page) = &mut self.page else {
            return ReadAnswer::NotStarted;
        };
        if page.ignoring {
            return ReadAnswer::Data {
                bytes: Vec::new(),
                claim: 0,
            };
        }
        if let Some(status) = page.failed {
            return ReadAnswer::Status(status);
        }
        if !page.frame_open {
            return ReadAnswer::Status(ffi::SANE_STATUS_EOF);
        }
        let mut next_event = u64::MAX;
        for (at, event) in &feed.events {
            if *at != page.number {
                continue;
            }
            let after = match *event {
                Event::Read { after, .. }
                | Event::Hang { after }
                | Event::HangForever { after }
                | Event::IgnoreCancel { after } => after,
                Event::Start(_) | Event::Abort => continue,
            };
            if after > page.page_sent {
                next_event = next_event.min(after);
                continue;
            }
            match *event {
                Event::Read { status, .. } => {
                    page.failed = Some(status);
                    return ReadAnswer::Status(status);
                }
                Event::Hang { .. } => return ReadAnswer::Hang,
                Event::HangForever { .. } => return ReadAnswer::HangForever,
                Event::IgnoreCancel { .. } => {
                    page.ignoring = true;
                    return ReadAnswer::Data {
                        bytes: Vec::new(),
                        claim: 0,
                    };
                }
                Event::Start(_) | Event::Abort => {}
            }
        }
        let spec = &frames[page.frame];
        let remaining = match spec.bytes {
            Bytes::Count(count) => count.saturating_sub(page.frame_sent),
            Bytes::Endless => u64::MAX,
        };
        if remaining == 0 {
            page.frame_open = false;
            return ReadAnswer::Status(ffi::SANE_STATUS_EOF);
        }
        let asked = u64::try_from(asked).unwrap_or(0);
        let most = feed.read_max.map_or(asked, |max| asked.min(max as u64));
        let length = remaining
            .min(most)
            .min(next_event - page.page_sent)
            .min(1 << 24);
        let back = duplex && page.number % 2 == 0;
        let bytes: Vec<u8> = (0..length)
            .map(|offset| {
                byte_at(
                    page.number,
                    back,
                    page.frame,
                    spec,
                    page.frame_sent + offset,
                )
            })
            .collect();
        page.frame_sent += length;
        page.page_sent += length;
        let given = i32::try_from(length).unwrap_or(i32::MAX);
        let claim = match feed.read_length {
            None => given,
            Some(ReadLength::Claim(claim)) => claim,
            Some(ReadLength::Past) => i32::try_from(asked).unwrap_or(i32::MAX).saturating_add(1),
        };
        ReadAnswer::Data { bytes, claim }
    }

    /// `sane_cancel`: the page in flight, if any, is dropped, and reads
    /// answer `CANCELLED` until the next start; but a page whose reads
    /// ignore cancels goes on as it was.
    pub fn cancel(&mut self) {
        match &self.page {
            Some(page) if page.ignoring => {}
            Some(_) => {
                self.cancelled = true;
                self.page = None;
            }
            None => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::script::Script;

    fn feed(text: &str) -> Feed {
        Script::parse(text).unwrap().feed
    }

    fn data(answer: ReadAnswer) -> Vec<u8> {
        match answer {
            ReadAnswer::Data { bytes, claim } => {
                assert_eq!(claim as usize, bytes.len());
                bytes
            }
            other => panic!("{other:?}"),
        }
    }

    const GOOD: StartAnswer = StartAnswer::Status(ffi::SANE_STATUS_GOOD);

    #[test]
    fn a_page_is_its_frames_bytes_then_eof() {
        let feed = feed("frame GRAY 1 5 3 2 8\n");
        let mut scan = ScanState::default();
        assert_eq!(scan.start(&feed), GOOD);
        assert_eq!(scan.parameters(&feed).bytes_per_line, 5);
        let first = data(scan.read(&feed, 4));
        let second = data(scan.read(&feed, 100));
        assert_eq!(first, [b'F', 31 + 1, 31 + 2, 0xEE]);
        assert_eq!(second, [0xEE, 34, 35, 36, 0xEE, 0xEE]);
        assert_eq!(
            scan.read(&feed, 100),
            ReadAnswer::Status(ffi::SANE_STATUS_EOF)
        );
        assert_eq!(
            scan.read(&feed, 100),
            ReadAnswer::Status(ffi::SANE_STATUS_EOF)
        );
        // The next start is the next page.
        assert_eq!(scan.start(&feed), GOOD);
        assert_eq!(scan.page.as_ref().unwrap().number, 2);
        assert_eq!(data(scan.read(&feed, 2)), [b'F', 63]);
    }

    #[test]
    fn a_feeder_ends_and_duplex_marks_its_backs() {
        let feed = feed("sheets 2\nduplex\nframe GRAY 1 2 2 1 8\nread-max 1\n");
        let mut scan = ScanState::default();
        let mut marks = Vec::new();
        for _ in 0..4 {
            assert_eq!(scan.start(&feed), GOOD);
            marks.push(data(scan.read(&feed, 10))[0]);
            assert_eq!(data(scan.read(&feed, 10)).len(), 1);
            assert_eq!(
                scan.read(&feed, 10),
                ReadAnswer::Status(ffi::SANE_STATUS_EOF)
            );
        }
        assert_eq!(marks, b"FBFB");
        assert_eq!(
            scan.start(&feed),
            StartAnswer::Status(ffi::SANE_STATUS_NO_DOCS)
        );
    }

    #[test]
    fn three_pass_frames_come_in_turn() {
        let feed = feed("frame RED 0 2 2 1 8\nframe GREEN 0 2 2 1 8\nframe BLUE 1 2 2 1 8\n");
        let mut scan = ScanState::default();
        for format in [
            ffi::SANE_FRAME_RED,
            ffi::SANE_FRAME_GREEN,
            ffi::SANE_FRAME_BLUE,
        ] {
            assert_eq!(scan.start(&feed), GOOD);
            assert_eq!(scan.parameters(&feed).format, format);
            data(scan.read(&feed, 10));
            assert_eq!(
                scan.read(&feed, 10),
                ReadAnswer::Status(ffi::SANE_STATUS_EOF)
            );
        }
        assert_eq!(scan.start(&feed), GOOD);
        assert_eq!(scan.page.as_ref().unwrap().number, 2);
        assert_eq!(scan.start(&feed), StartAnswer::Early);
    }

    #[test]
    fn events_happen_at_their_page_and_byte() {
        let feed = feed(
            "at 2 start JAMMED\nat 3 read COVER_OPEN 10\nat 4 hang 0\nat 5 hang-forever 4\nat 6 ignore-cancel\nat 7 abort\n",
        );
        let mut scan = ScanState::default();
        assert_eq!(scan.start(&feed), GOOD);
        scan.cancel();
        assert_eq!(
            scan.start(&feed),
            StartAnswer::Status(ffi::SANE_STATUS_JAMMED)
        );
        assert_eq!(scan.start(&feed), GOOD);
        assert_eq!(data(scan.read(&feed, 100)).len(), 10);
        assert_eq!(
            scan.read(&feed, 100),
            ReadAnswer::Status(ffi::SANE_STATUS_COVER_OPEN)
        );
        assert_eq!(
            scan.read(&feed, 100),
            ReadAnswer::Status(ffi::SANE_STATUS_COVER_OPEN)
        );
        scan.cancel();
        assert_eq!(
            scan.read(&feed, 100),
            ReadAnswer::Status(ffi::SANE_STATUS_CANCELLED)
        );
        assert_eq!(scan.start(&feed), GOOD);
        assert_eq!(scan.read(&feed, 100), ReadAnswer::Hang);
        scan.cancel();
        assert_eq!(scan.start(&feed), GOOD);
        assert_eq!(data(scan.read(&feed, 100)).len(), 4);
        assert_eq!(scan.read(&feed, 100), ReadAnswer::HangForever);
        scan.cancel();
        assert_eq!(scan.start(&feed), GOOD);
        assert_eq!(data(scan.read(&feed, 100)), []);
        scan.cancel();
        assert_eq!(data(scan.read(&feed, 100)), [], "cancelled or not");
        assert_eq!(scan.start(&feed), StartAnswer::Abort);
    }

    #[test]
    fn hostile_answers_are_given_as_scripted() {
        let feed = feed("frame 11 2 -3 0 -2 32 7\nread-length past\n");
        let mut scan = ScanState::default();
        assert_eq!(scan.start(&feed), GOOD);
        let parameters = scan.parameters(&feed);
        assert_eq!(
            (
                parameters.format,
                parameters.last_frame,
                parameters.bytes_per_line
            ),
            (11, 2, -3)
        );
        assert_eq!(
            (
                parameters.pixels_per_line,
                parameters.lines,
                parameters.depth
            ),
            (0, -2, 32)
        );
        let ReadAnswer::Data { bytes, claim } = scan.read(&feed, 5) else {
            panic!()
        };
        assert_eq!((bytes.len(), claim), (5, 6));
        let endless = feed_endless();
        let mut scan = ScanState::default();
        scan.start(&endless);
        for _ in 0..100 {
            assert_eq!(data(scan.read(&endless, 1000)).len(), 1000);
        }
    }

    fn feed_endless() -> Feed {
        feed("frame GRAY 1 10 10 -1 8 endless\n")
    }

    #[test]
    fn scan_lines_parse_and_say_their_errors() {
        let feed = feed(
            "frame RGB 1 9 3 -1 8\nsheets 10\nnon-blocking\nio-mode-unsupported\nselect-fd -1\nparameters-status IO_ERROR\nread-length -1\nstart-inval-deactivates a b\n",
        );
        assert_eq!(feed.frames[0].bytes, Bytes::Count(36));
        assert_eq!(feed.sheets, Some(10));
        assert!(feed.non_blocking);
        assert!(feed.io_mode_unsupported);
        assert_eq!(feed.select_fd, Some(-1));
        assert_eq!(feed.parameters_status, Some(ffi::SANE_STATUS_IO_ERROR));
        assert_eq!(feed.read_length, Some(ReadLength::Claim(-1)));
        assert_eq!(feed.inval_deactivates, [b"a".to_vec(), b"b".to_vec()]);
        for bad in [
            "frame GRAY 1 2\n",
            "frame BLACK 1 2 2 2 8\n",
            "frame GRAY 1 2 2 2 8 lots\n",
            "at 0 abort\n",
            "at 1 jump\n",
            "at 1 start\n",
            "at 1 read NOPE 1\n",
            "sheets some\n",
        ] {
            let error = Script::parse(bad).unwrap_err();
            assert!(error.starts_with("line 1: "), "{bad:?}: {error}");
        }
    }
}
