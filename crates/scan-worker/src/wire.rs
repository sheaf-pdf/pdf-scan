//! The protocol's messages, a line read under a cap, and a message made a line.
//!
//! The crate's docs give the protocol whole ("The protocol"); the unit tests below pin each
//! message's form on a literal line.

use std::{
    io::{self, BufRead, Read},
    time::Duration,
};

use scan_device::in_progress::Level;
use scan_device::{
    Applied, Caps, DeviceId, DeviceInfo, Done, EncodedKind, Format, ScanError, Settings, Side,
    SideKnown, Status, WindowClosed, WindowOwner,
};
use serde::{Deserialize, Serialize};

/// The protocol's version: a worker's first line says it, and a client that speaks another
/// refuses the worker.
///
/// It changes whenever a message, or a type of the device model that a message carries, changes
/// form, so a worker and a client built apart never misread each other: the two ship together,
/// and a mismatch is an error, not a negotiation.
pub const PROTOCOL: u32 = 1;

/// The longest request line a worker reads, its `\n` aside: 1 MiB, far more than any request
/// needs.
pub(crate) const REQUEST_LIMIT: usize = 1 << 20;

/// A request, client to worker: one a line.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Request {
    /// The devices every backend finds, under one deadline: answered `devices` or `error`.
    List {
        /// The listing's deadline, shared by the backends in turn.
        deadline: Duration,
    },
    /// The stored device opened: answered `opened` or `error`.
    Open {
        /// The device as a listing gave it and the app stored it.
        device: DeviceInfo,
        /// The open's deadline, shared by the backends in turn.
        deadline: Duration,
    },
    /// What the open device can do: answered `caps` or `error`.
    Caps,
    /// A batch: answered with its events, then `done` or `error`.
    Scan {
        /// The settings asked for.
        settings: Settings,
        /// The first sheet's number.
        first_sheet: u32,
    },
    /// The driver's own settings window: answered `window` or `error`.
    SettingsWindow {
        /// The app's window, to own the driver's.
        owner: Option<WindowOwner>,
    },
    /// The running batch cancelled: never answered, the batch's end says it.
    Cancel,
    /// The worker ends, a running batch cancelled first.
    Quit,
}

/// A message, worker to client: one a line, a page's bytes after its line.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Reply {
    /// The worker's first line, unasked.
    Hello {
        /// The protocol it speaks, [`PROTOCOL`].
        protocol: u32,
        /// The crate's version it was built with, for a log.
        version: String,
    },
    /// A listing's devices, every backend's in turn.
    Devices(Vec<DeviceInfo>),
    /// The device opened.
    Opened {
        /// The device as the backend found it.
        info: DeviceInfo,
        /// The stored id, where another device was opened in its place.
        renamed_from: Option<DeviceId>,
    },
    /// What the open device can do.
    Caps(Caps),
    /// How the driver's settings window closed.
    Window(WindowClosed),
    /// A batch's settings applied.
    Applied(Applied),
    /// A batch's sheet begins.
    Sheet(u32),
    /// A batch's page: its bytes follow the line.
    Page(PageHeader),
    /// A batch's status.
    Status(Status),
    /// The bytes done of the page in flight, and its total while it's known.
    Progress {
        /// The bytes done.
        done: u64,
        /// The page's bytes, `None` while unknown.
        total: Option<u64>,
    },
    /// The latest message a batch's driver reported.
    Message {
        /// How serious it is.
        level: MessageLevel,
        /// What it says.
        text: String,
    },
    /// How a batch ended, when it ended well.
    Done(Done),
    /// A call's error, or the error that ended a batch.
    Error(ScanError),
    /// A request the worker didn't take, or an event it can't carry, and why.
    Refused {
        /// Why, for a log.
        reason: String,
    },
}

/// A page's line: its pixels' form, where it's from, and how many bytes follow.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct PageHeader {
    /// Raw, with its format and size, or encoded, with its kind.
    pub(crate) pixels: PixelsHeader,
    /// Its resolution across and down.
    pub(crate) dpi: (f32, f32),
    /// The sheet it's from.
    pub(crate) sheet: u32,
    /// The side it's from.
    pub(crate) side: Side,
    /// Whether the driver said the side.
    pub(crate) side_known: SideKnown,
    /// The bytes that follow the line: a raw page's data in the raw layout, or an encoded
    /// page's bytes.
    pub(crate) bytes: u64,
}

/// A page's pixels, as its line says them: the bytes travel after it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PixelsHeader {
    /// In the raw layout.
    Raw {
        /// The format.
        format: Format,
        /// The width, in pixels.
        width: u32,
        /// The height, in pixels.
        height: u32,
    },
    /// Encoded.
    Encoded {
        /// The form the bytes are in.
        kind: EncodedKind,
    },
}

/// A message's level, by name: `in-progress`'s [`Level`] has no serde form.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum MessageLevel {
    /// [`Level::Info`].
    Info,
    /// [`Level::Warn`].
    Warn,
    /// [`Level::Error`].
    Error,
}

impl From<Level> for MessageLevel {
    fn from(level: Level) -> MessageLevel {
        match level {
            Level::Info => MessageLevel::Info,
            Level::Warn => MessageLevel::Warn,
            Level::Error => MessageLevel::Error,
        }
    }
}

/// `message` as its line: its JSON and a `\n`.
pub(crate) fn line(message: &impl Serialize) -> io::Result<Vec<u8>> {
    // The messages hold no map, so nothing here fails to encode; an error is kept all the same.
    let mut line = serde_json::to_vec(message).map_err(io::Error::other)?;
    line.push(b'\n');
    Ok(line)
}

/// What [`read_line`] read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Line {
    /// A line, its `\n` taken off, in the buffer.
    Read,
    /// A line past the cap: the cap and one more byte were read, the rest of the line skipped
    /// unkept, and the buffer left empty.
    TooLong,
    /// The input ended, at a line's start or within one (the buffer holding what came).
    End,
}

/// The next line of `input`, into `line`, read only up to `limit` bytes and its `\n`: a longer
/// line is never held whole, only skipped.
pub(crate) fn read_line(
    input: &mut impl BufRead,
    limit: usize,
    line: &mut Vec<u8>,
) -> io::Result<Line> {
    line.clear();
    let most = u64::try_from(limit).unwrap_or(u64::MAX).saturating_add(1);
    input.by_ref().take(most).read_until(b'\n', line)?;
    if line.last() == Some(&b'\n') {
        line.pop();
        return Ok(Line::Read);
    }
    if line.len() > limit {
        line.clear();
        // Through its `\n`, or to the input's end.
        input.skip_until(b'\n')?;
        return Ok(Line::TooLong);
    }
    Ok(Line::End)
}

#[cfg(test)]
mod tests {
    use std::{io::Cursor, num::NonZeroU32};

    use scan_device::{Clamped, Driver, Encoding, Paper, Resolutions, SizeMm, Source, SourceCaps};

    use super::*;

    fn device() -> DeviceInfo {
        DeviceInfo::new(
            DeviceId("fake:desk".to_owned()),
            "Maker Model".to_owned(),
            "Maker".to_owned(),
            "Model".to_owned(),
            Driver::Fake,
        )
    }

    const DEVICE: &str = r#"{"id":"fake:desk","name":"Maker Model","maker":"Maker","model":"Model","driver":"Fake"}"#;

    /// `request` read from `line`, and written as it.
    fn request_is(line: &str, request: Request) {
        let read: Request =
            serde_json::from_str(line).unwrap_or_else(|error| panic!("{line}: {error}"));
        assert_eq!(read, request, "{line}");
        assert_eq!(serde_json::to_string(&request).unwrap(), line);
    }

    /// `reply` written as `line`, and read from it.
    fn reply_is(reply: Reply, line: &str) {
        assert_eq!(serde_json::to_string(&reply).unwrap(), line);
        let read: Reply =
            serde_json::from_str(line).unwrap_or_else(|error| panic!("{line}: {error}"));
        assert_eq!(read, reply, "{line}");
    }

    #[test]
    fn each_request_s_line() {
        let five = Duration::from_secs(5);
        request_is(
            r#"{"list":{"deadline":{"secs":5,"nanos":0}}}"#,
            Request::List { deadline: five },
        );
        request_is(
            &format!(
                r#"{{"open":{{"device":{DEVICE},"deadline":{{"secs":1,"nanos":500000000}}}}}}"#
            ),
            Request::Open {
                device: device(),
                deadline: Duration::from_millis(1500),
            },
        );
        request_is(r#""caps""#, Request::Caps);
        let mut settings = Settings::new(Source::Feeder, 300, Format::Gray8);
        settings.duplex = true;
        request_is(
            r#"{"scan":{"settings":{"source":"Feeder","duplex":true,"dpi":300,"format":"Gray8","paper":"Auto","encodings":[]},"first_sheet":1}}"#,
            Request::Scan {
                settings: settings.clone(),
                first_sheet: 1,
            },
        );
        settings.paper = Paper::Size(SizeMm::A4);
        settings.encodings = vec![Encoding::Jpeg, Encoding::CcittG4];
        request_is(
            r#"{"scan":{"settings":{"source":"Feeder","duplex":true,"dpi":300,"format":"Gray8","paper":{"Size":{"width":210.0,"height":297.0}},"encodings":["Jpeg","CcittG4"]},"first_sheet":7}}"#,
            Request::Scan {
                settings,
                first_sheet: 7,
            },
        );
        request_is(
            r#"{"settings_window":{"owner":null}}"#,
            Request::SettingsWindow { owner: None },
        );
        request_is(
            r#"{"settings_window":{"owner":18446744073709551615}}"#,
            Request::SettingsWindow {
                owner: Some(WindowOwner(u64::MAX)),
            },
        );
        request_is(r#""cancel""#, Request::Cancel);
        request_is(r#""quit""#, Request::Quit);
    }

    #[test]
    fn each_reply_s_line() {
        reply_is(
            Reply::Hello {
                protocol: PROTOCOL,
                version: "0.0.0".to_owned(),
            },
            r#"{"hello":{"protocol":1,"version":"0.0.0"}}"#,
        );
        reply_is(Reply::Devices(Vec::new()), r#"{"devices":[]}"#);
        reply_is(
            Reply::Devices(vec![device(), device()]),
            &format!(r#"{{"devices":[{DEVICE},{DEVICE}]}}"#),
        );
        reply_is(
            Reply::Opened {
                info: device(),
                renamed_from: None,
            },
            &format!(r#"{{"opened":{{"info":{DEVICE},"renamed_from":null}}}}"#),
        );
        reply_is(
            Reply::Opened {
                info: device(),
                renamed_from: Some(DeviceId("fake:libusb:001:004".to_owned())),
            },
            &format!(r#"{{"opened":{{"info":{DEVICE},"renamed_from":"fake:libusb:001:004"}}}}"#),
        );
        let mut feeder = SourceCaps::new(
            Source::Feeder,
            Resolutions::Range {
                min: 50,
                max: 600,
                step: NonZeroU32::MIN,
            },
            vec![Format::Bw1, Format::Rgb8],
        );
        feeder.duplex = true;
        feeder.max_area = Some(SizeMm::A4);
        let flatbed = SourceCaps::new(
            Source::Flatbed,
            Resolutions::List(vec![300]),
            vec![Format::Gray16],
        );
        let mut caps = Caps::new(vec![feeder, flatbed]);
        caps.settings_window = true;
        reply_is(
            Reply::Caps(caps),
            concat!(
                r#"{"caps":{"sources":["#,
                r#"{"source":"Feeder","duplex":true,"resolutions":{"Range":{"min":50,"max":600,"step":1}},"formats":["Bw1","Rgb8"],"encodings":[],"max_area":{"width":210.0,"height":297.0},"paper_auto":false},"#,
                r#"{"source":"Flatbed","duplex":false,"resolutions":{"List":[300]},"formats":["Gray16"],"encodings":[],"max_area":null,"paper_auto":false}"#,
                r#"],"settings_window":true}}"#,
            ),
        );
        reply_is(
            Reply::Window(WindowClosed::Accepted),
            r#"{"window":"Accepted"}"#,
        );
        reply_is(
            Reply::Window(WindowClosed::Dismissed),
            r#"{"window":"Dismissed"}"#,
        );
        reply_is(
            Reply::Applied(Applied::new(
                Settings::new(Source::Flatbed, 600, Format::Bw1),
                vec![Clamped::Dpi(1200)],
            )),
            r#"{"applied":{"settings":{"source":"Flatbed","duplex":false,"dpi":600,"format":"Bw1","paper":"Auto","encodings":[]},"clamped":[{"Dpi":1200}]}}"#,
        );
        reply_is(Reply::Sheet(2), r#"{"sheet":2}"#);
        reply_is(
            Reply::Page(PageHeader {
                pixels: PixelsHeader::Raw {
                    format: Format::Rgb8,
                    width: 2550,
                    height: 3300,
                },
                dpi: (300.0, 300.0),
                sheet: 1,
                side: Side::Front,
                side_known: SideKnown::Inferred,
                bytes: 25_245_000,
            }),
            r#"{"page":{"pixels":{"raw":{"format":"Rgb8","width":2550,"height":3300}},"dpi":[300.0,300.0],"sheet":1,"side":"Front","side_known":"Inferred","bytes":25245000}}"#,
        );
        reply_is(
            Reply::Page(PageHeader {
                pixels: PixelsHeader::Encoded {
                    kind: EncodedKind::CcittG4 {
                        width: 2550,
                        height: 3300,
                        black_is_1: true,
                    },
                },
                dpi: (299.5, 200.25),
                sheet: 4,
                side: Side::Back,
                side_known: SideKnown::Reported,
                bytes: 33,
            }),
            r#"{"page":{"pixels":{"encoded":{"kind":{"CcittG4":{"width":2550,"height":3300,"black_is_1":true}}}},"dpi":[299.5,200.25],"sheet":4,"side":"Back","side_known":"Reported","bytes":33}}"#,
        );
        reply_is(
            Reply::Page(PageHeader {
                pixels: PixelsHeader::Encoded {
                    kind: EncodedKind::Jpeg,
                },
                dpi: (150.0, 150.0),
                sheet: 1,
                side: Side::Front,
                side_known: SideKnown::Inferred,
                bytes: 0,
            }),
            r#"{"page":{"pixels":{"encoded":{"kind":"Jpeg"}},"dpi":[150.0,150.0],"sheet":1,"side":"Front","side_known":"Inferred","bytes":0}}"#,
        );
        reply_is(
            Reply::Status(Status::WarmingUp),
            r#"{"status":"WarmingUp"}"#,
        );
        reply_is(
            Reply::Progress {
                done: 1024,
                total: Some(4096),
            },
            r#"{"progress":{"done":1024,"total":4096}}"#,
        );
        reply_is(
            Reply::Progress {
                done: 0,
                total: None,
            },
            r#"{"progress":{"done":0,"total":null}}"#,
        );
        for (level, name) in [
            (Level::Info, "info"),
            (Level::Warn, "warn"),
            (Level::Error, "error"),
        ] {
            reply_is(
                Reply::Message {
                    level: level.into(),
                    text: "lamp \"warm\"".to_owned(),
                },
                &format!(r#"{{"message":{{"level":"{name}","text":"lamp \"warm\""}}}}"#),
            );
        }
        reply_is(Reply::Done(Done::FeederEmpty), r#"{"done":"FeederEmpty"}"#);
        reply_is(Reply::Done(Done::Cancelled), r#"{"done":"Cancelled"}"#);
        reply_is(Reply::Error(ScanError::Jammed), r#"{"error":"Jammed"}"#);
        reply_is(
            Reply::Error(ScanError::Driver {
                message: "lamp failure".to_owned(),
                shown: false,
            }),
            r#"{"error":{"Driver":{"message":"lamp failure","shown":false}}}"#,
        );
        reply_is(
            Reply::Refused {
                reason: "no device is open".to_owned(),
            },
            r#"{"refused":{"reason":"no device is open"}}"#,
        );
    }

    #[test]
    fn a_message_s_line_ends_in_its_newline_alone() {
        assert_eq!(line(&Request::Caps).unwrap(), b"\"caps\"\n");
        let text = Reply::Message {
            level: MessageLevel::Info,
            text: "two\nlines".to_owned(),
        };
        let written = line(&text).unwrap();
        // A newline inside a string is escaped, so the line's own is the only one.
        assert_eq!(written.iter().filter(|&&byte| byte == b'\n').count(), 1);
        assert_eq!(written.last(), Some(&b'\n'));
    }

    #[test]
    fn what_isn_t_a_request_isn_t_read_as_one() {
        for line in [
            "",
            "not json",
            r#""lis""#,
            r#"{"list":{}}"#,
            r#"{"list":{"deadline":5}}"#,
            r#"{"scan":{"settings":{"source":"Feeder"},"first_sheet":1}}"#,
            r#"{"caps":null,"quit":null}"#,
            r#""caps" "quit""#,
        ] {
            assert!(
                serde_json::from_str::<Request>(line).is_err(),
                "{line:?} was read"
            );
        }
    }

    /// Each of `input`'s lines under `limit`, read in turn to its end.
    fn lines(input: &[u8], limit: usize) -> Vec<(Line, Vec<u8>)> {
        // A small buffer, so lines cross its refills.
        let mut input = io::BufReader::with_capacity(3, Cursor::new(input));
        let mut line = Vec::new();
        let mut read = Vec::new();
        loop {
            let got = read_line(&mut input, limit, &mut line).unwrap();
            read.push((got, line.clone()));
            if got == Line::End {
                return read;
            }
        }
    }

    #[test]
    fn a_line_at_the_cap_is_read_and_one_past_it_skipped() {
        let read = lines(b"12345\n123456\n\nabc\n1234567890123\nx", 5);
        assert_eq!(
            read,
            [
                (Line::Read, b"12345".to_vec()),
                (Line::TooLong, Vec::new()),
                (Line::Read, Vec::new()),
                (Line::Read, b"abc".to_vec()),
                (Line::TooLong, Vec::new()),
                (Line::End, b"x".to_vec()),
            ]
        );
        // Past the cap at the input's end, with no `\n`.
        assert_eq!(
            lines(b"123456", 5),
            [(Line::TooLong, Vec::new()), (Line::End, Vec::new())]
        );
        assert_eq!(lines(b"", 5), [(Line::End, Vec::new())]);
        // No more than the cap and a byte is held of a long line.
        let long = vec![b'a'; 1 << 20];
        let mut input = Cursor::new(&long);
        let mut line = Vec::new();
        assert_eq!(read_line(&mut input, 10, &mut line).unwrap(), Line::TooLong);
        assert!(line.capacity() < 1024, "{}", line.capacity());
    }
}
