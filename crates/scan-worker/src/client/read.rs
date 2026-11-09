//! A worker's output read on a thread of its own: each line under its cap, each page's bytes
//! capped and checked before they're read, and every message handed on as it comes; and its
//! standard error drained, its end kept.

use std::{
    collections::VecDeque,
    io::{self, BufRead, BufReader, Read},
    sync::{
        Arc, Mutex, MutexGuard, PoisonError,
        mpsc::{self, Receiver, SyncSender},
    },
    thread,
    time::Duration,
};

use scan_device::{Encoded, Pixels, RawImage, RawPage};

use super::Limits;
use crate::ProtocolError;
use crate::wire::{self, Line, PageHeader, PixelsHeader, Reply};

/// What the reader hands the calling thread: each message as it comes, then how the output
/// ended, or what broke the protocol, after which it reads nothing more.
#[derive(Debug)]
pub(super) enum Incoming {
    /// A message, but a page.
    Message(Reply),
    /// A page, read whole and checked.
    Page(RawPage),
    /// What broke the protocol.
    Broken(ProtocolError),
    /// The output ended: at a line's start or within one, or within a page's bytes.
    End {
        /// Whether it ended within a page's bytes, after its line: that page is dropped.
        page_cut_short: bool,
    },
}

impl Incoming {
    /// Whether nothing comes after it.
    fn is_last(&self) -> bool {
        matches!(self, Incoming::Broken(_) | Incoming::End { .. })
    }
}

/// The reader: `output`'s messages handed on `messages` in turn, until it ends or breaks the
/// protocol, or the calling side is gone.
pub(super) fn read_output(output: impl Read, limits: Limits, messages: &SyncSender<Incoming>) {
    let mut output = BufReader::with_capacity(1 << 16, output);
    let mut line = Vec::new();
    loop {
        let incoming = next(&mut output, limits, &mut line);
        let last = incoming.is_last();
        if messages.send(incoming).is_err() || last {
            return;
        }
    }
}

/// `output`'s next message.
fn next(output: &mut impl BufRead, limits: Limits, line: &mut Vec<u8>) -> Incoming {
    match wire::read_line(output, limits.line, line) {
        Ok(Line::Read) => {}
        // The rest is never read: the worker is ended.
        Ok(Line::TooLong) => {
            return Incoming::Broken(ProtocolError::LineTooLong { limit: limits.line });
        }
        // A read that fails is as much the output's end as one that ends; a line cut short is
        // never a message.
        Ok(Line::End) | Err(_) => {
            return Incoming::End {
                page_cut_short: false,
            };
        }
    }
    match serde_json::from_slice::<Reply>(line) {
        Ok(Reply::Page(header)) => match read_page(output, &header, limits.page) {
            Ok(page) => Incoming::Page(page),
            Err(None) => Incoming::End {
                page_cut_short: true,
            },
            Err(Some(error)) => Incoming::Broken(error),
        },
        Ok(reply) => Incoming::Message(reply),
        Err(error) => Incoming::Broken(ProtocolError::Unreadable {
            message: error.to_string(),
        }),
    }
}

/// The page `header` states, its bytes read from `output` once they're capped at `limit` and,
/// raw, checked against its format and size; its `dpi` checked. Refused with what it broke, or
/// `None` where the output ended within its bytes.
fn read_page(
    output: &mut impl Read,
    header: &PageHeader,
    limit: u64,
) -> Result<RawPage, Option<ProtocolError>> {
    let broken = |error| Err(Some(error));
    let too_large = ProtocolError::PageTooLarge {
        bytes: header.bytes,
        limit,
    };
    if header.bytes > limit {
        return broken(too_large);
    }
    if let PixelsHeader::Raw {
        format,
        width,
        height,
    } = header.pixels
    {
        let expected = match RawImage::data_len(format, width, height) {
            Ok(expected) => expected as u64,
            Err(error) => return broken(ProtocolError::Layout(error)),
        };
        if expected != header.bytes {
            return broken(ProtocolError::PageLength {
                expected,
                stated: header.bytes,
            });
        }
    }
    let (across, down) = header.dpi;
    if ![across, down]
        .iter()
        .all(|dpi| dpi.is_finite() && *dpi > 0.0)
    {
        return broken(ProtocolError::Invalid {
            what: "a page whose dpi isn't finite and above 0",
        });
    }
    // Within the cap, which a 32-bit system's address space may not reach.
    let Ok(length) = usize::try_from(header.bytes) else {
        return broken(too_large);
    };
    let mut bytes = vec![0; length];
    if output.read_exact(&mut bytes).is_err() {
        return Err(None);
    }
    let pixels = match header.pixels {
        PixelsHeader::Raw {
            format,
            width,
            height,
        } => match RawImage::new(format, width, height, bytes) {
            Ok(image) => Pixels::Raw(image),
            Err(error) => return broken(ProtocolError::Layout(error)),
        },
        PixelsHeader::Encoded { kind } => Pixels::Encoded(Encoded { kind, bytes }),
    };
    Ok(RawPage::new(
        pixels,
        header.dpi,
        header.sheet,
        header.side,
        header.side_known,
    ))
}

/// How much of a worker's standard error is kept: its last 8 KiB.
const TAIL: usize = 8 << 10;

/// A worker's standard error, drained by a thread of its own as it comes, so a worker that
/// writes much there is never held; its last [`TAIL`] bytes kept.
pub(super) struct Stderr {
    tail: Arc<Mutex<VecDeque<u8>>>,
    /// Disconnected once the thread has read to the end.
    ended: Receiver<()>,
}

impl Stderr {
    /// `stderr` drained, on a thread of its own.
    pub(super) fn drain(stderr: impl Read + Send + 'static) -> io::Result<Stderr> {
        let tail = Arc::new(Mutex::new(VecDeque::with_capacity(TAIL)));
        let (alive, ended) = mpsc::channel::<()>();
        let kept = Arc::clone(&tail);
        thread::Builder::new()
            .name("scan-worker stderr".to_owned())
            .spawn(move || {
                let _alive = alive;
                keep_tail(stderr, &kept);
            })?;
        Ok(Stderr { tail, ended })
    }

    /// The end of what was written, once the worker's standard error has ended or `wait` has
    /// passed: lossy where it isn't UTF-8.
    pub(super) fn tail(&self, wait: Duration) -> String {
        // Only ever disconnected, or timed out: nothing is sent.
        let _ = self.ended.recv_timeout(wait);
        let mut tail = lock(&self.tail);
        String::from_utf8_lossy(tail.make_contiguous()).into_owned()
    }
}

/// `input` read to its end, its last [`TAIL`] bytes kept in `tail`.
fn keep_tail(mut input: impl Read, tail: &Mutex<VecDeque<u8>>) {
    let mut chunk = [0; 4096];
    loop {
        match input.read(&mut chunk) {
            Ok(0) => return,
            Ok(read) => {
                let mut tail = lock(tail);
                tail.extend(&chunk[..read]);
                let over = tail.len().saturating_sub(TAIL);
                tail.drain(..over);
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(_) => return,
        }
    }
}

/// `mutex` locked, whatever thread panicked holding it: the tail is whole between statements.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use scan_device::{EncodedKind, Format, LayoutError, Side, SideKnown};

    use super::*;

    /// Every message `output` holds, read under `limits`, through its end.
    fn read_all(output: &[u8], limits: Limits) -> Vec<Incoming> {
        let (sent, received) = mpsc::sync_channel(0);
        let output = output.to_vec();
        let reader = thread::spawn(move || read_output(Cursor::new(output), limits, &sent));
        let read = received.iter().collect();
        reader.join().expect("the reader");
        read
    }

    fn page_line(pixels: &str, dpi: &str, bytes: u64) -> String {
        format!(
            r#"{{"page":{{"pixels":{pixels},"dpi":{dpi},"sheet":1,"side":"Front","side_known":"Inferred","bytes":{bytes}}}}}"#
        ) + "\n"
    }

    fn gray(width: u32, height: u32) -> String {
        format!(r#"{{"raw":{{"format":"Gray8","width":{width},"height":{height}}}}}"#)
    }

    #[test]
    fn messages_and_pages_come_in_order_then_the_end() {
        let mut output = b"{\"sheet\":1}\n".to_vec();
        output.extend(page_line(&gray(3, 2), "[300.0,150.0]", 6).as_bytes());
        output.extend(b"\n\x01\x02\x03\x04\x05");
        output.extend(page_line(r#"{"encoded":{"kind":"Jpeg"}}"#, "[300.0,300.0]", 4).as_bytes());
        output.extend(b"JPEG{\"done\":\"FeederEmpty\"}\n");
        let read = read_all(&output, Limits::default());
        assert_eq!(read.len(), 5, "{read:?}");
        assert!(matches!(read[0], Incoming::Message(Reply::Sheet(1))));
        let Incoming::Page(raw) = &read[1] else {
            panic!("{:?}", read[1]);
        };
        let expected = RawImage::new(Format::Gray8, 3, 2, b"\n\x01\x02\x03\x04\x05".to_vec());
        assert_eq!(raw.pixels, Pixels::Raw(expected.unwrap()));
        assert_eq!(raw.dpi, (300.0, 150.0));
        assert_eq!(
            (raw.sheet, raw.side, raw.side_known),
            (1, Side::Front, SideKnown::Inferred)
        );
        let Incoming::Page(encoded) = &read[2] else {
            panic!("{:?}", read[2]);
        };
        let jpeg = Encoded {
            kind: EncodedKind::Jpeg,
            bytes: b"JPEG".to_vec(),
        };
        assert_eq!(encoded.pixels, Pixels::Encoded(jpeg));
        assert!(matches!(read[3], Incoming::Message(Reply::Done(_))));
        assert!(matches!(
            read[4],
            Incoming::End {
                page_cut_short: false
            }
        ));
    }

    /// What reading `output` under `limits` ends with.
    fn ends(output: &[u8], limits: Limits) -> Incoming {
        read_all(output, limits).pop().expect("an end")
    }

    #[test]
    fn a_line_past_the_cap_breaks_the_protocol_unread() {
        // `{"sheet":1}` is 11 bytes.
        let limits = Limits {
            line: 11,
            ..Limits::default()
        };
        assert!(matches!(
            ends(b"{\"sheet\":1}\n{\"sheet\":12}\n", limits),
            Incoming::Broken(ProtocolError::LineTooLong { limit: 11 })
        ));
        // At the cap, read.
        assert!(matches!(
            read_all(b"{\"sheet\":1}\n", limits)[0],
            Incoming::Message(Reply::Sheet(1))
        ));
    }

    #[test]
    fn a_page_is_capped_and_checked_before_its_bytes_are_read() {
        let limits = Limits {
            page: 100,
            ..Limits::default()
        };
        let past_the_cap = page_line(&gray(101, 1), "[300.0,300.0]", 101);
        assert_eq!(
            format!("{:?}", ends(past_the_cap.as_bytes(), limits)),
            format!(
                "{:?}",
                Incoming::Broken(ProtocolError::PageTooLarge {
                    bytes: 101,
                    limit: 100
                })
            )
        );
        let at_the_cap = page_line(&gray(100, 1), "[300.0,300.0]", 100) + &"x".repeat(100);
        assert!(matches!(
            read_all(at_the_cap.as_bytes(), limits)[0],
            Incoming::Page(_)
        ));
        let encoded_past = page_line(r#"{"encoded":{"kind":"Png"}}"#, "[300.0,300.0]", 101);
        assert!(matches!(
            ends(encoded_past.as_bytes(), limits),
            Incoming::Broken(ProtocolError::PageTooLarge { .. })
        ));
        let length = page_line(&gray(3, 3), "[300.0,300.0]", 8) + "12345678";
        assert!(matches!(
            ends(length.as_bytes(), Limits::default()),
            Incoming::Broken(ProtocolError::PageLength {
                expected: 9,
                stated: 8
            })
        ));
        let empty = page_line(&gray(0, 3), "[300.0,300.0]", 0);
        assert!(matches!(
            ends(empty.as_bytes(), Limits::default()),
            Incoming::Broken(ProtocolError::Layout(LayoutError::Empty))
        ));
        for dpi in ["[0.0,300.0]", "[300.0,-1.0]", "[-0.0,300.0]"] {
            let line = page_line(&gray(1, 1), dpi, 1) + "x";
            assert!(
                matches!(
                    ends(line.as_bytes(), Limits::default()),
                    Incoming::Broken(ProtocolError::Invalid { .. })
                ),
                "{dpi}"
            );
        }
        // Bits set past a black and white row's last pixel.
        let padding = page_line(
            r#"{"raw":{"format":"Bw1","width":9,"height":1}}"#,
            "[300.0,300.0]",
            2,
        );
        let mut output = padding.into_bytes();
        output.extend([0xff, 0xff]);
        assert!(matches!(
            ends(&output, Limits::default()),
            Incoming::Broken(ProtocolError::Layout(LayoutError::Padding { row: 0 }))
        ));
    }

    #[test]
    fn an_end_within_a_page_or_a_line_is_never_a_message() {
        let cut = page_line(&gray(4, 4), "[300.0,300.0]", 16) + "12345678";
        let read = read_all(cut.as_bytes(), Limits::default());
        assert!(matches!(
            read[..],
            [Incoming::End {
                page_cut_short: true
            }]
        ));
        let read = read_all(b"{\"sheet\":1}\n{\"sheet\":2}", Limits::default());
        assert!(matches!(
            read[..],
            [
                Incoming::Message(Reply::Sheet(1)),
                Incoming::End {
                    page_cut_short: false
                }
            ]
        ));
    }

    #[test]
    fn a_line_that_isn_t_a_message_breaks_the_protocol() {
        for line in [
            "not json\n",
            "{\"sheet\":-1}\n",
            "{\"hello\":{}}\n",
            "\"page\"\n",
        ] {
            assert!(
                matches!(
                    ends(line.as_bytes(), Limits::default()),
                    Incoming::Broken(ProtocolError::Unreadable { .. })
                ),
                "{line:?}"
            );
        }
    }

    #[test]
    fn standard_error_s_last_8_kib_are_kept() {
        let mut written = vec![b'a'; 10_000];
        written.extend(b"the end");
        let stderr = Stderr::drain(Cursor::new(written)).unwrap();
        let tail = stderr.tail(Duration::from_secs(10));
        assert_eq!(tail.len(), TAIL);
        assert!(tail.ends_with("aaathe end"), "{tail}");
        // Lossy where it isn't UTF-8.
        let stderr = Stderr::drain(Cursor::new(b"bad \xff byte".to_vec())).unwrap();
        assert_eq!(stderr.tail(Duration::from_secs(10)), "bad \u{fffd} byte");
    }
}
