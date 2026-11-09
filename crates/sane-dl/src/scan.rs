//! Scanning: a frame's parameters and their checks, frames assembled into a
//! page's image, and a batch of pages.
//!
//! Safe code only. The library module makes SANE's scanning calls one at a
//! time ([`Handle::start`], [`Handle::parameters`], [`Handle::read`]); this
//! module decides what their answers mean: whether a frame's parameters can
//! stand for a frame at all ([`Parameters::check`]), how a page's frames fit
//! together, and how their rows become one image, all unit-tested from
//! chunks without a library.

use std::fmt;
use std::time::Duration;

use crate::{Call, Error, FrameFormat, Handle, Limits, Status, ffi};

/// The longest pause between reads that gave no bytes, in a page's reading
/// ([`Batch::next_page`]).
const EMPTY_READ_PAUSE: Duration = Duration::from_millis(10);

/// A frame's parameters, as `sane_get_parameters` gives them: each field
/// the integer SANE wrote, unchecked.
///
/// Before [`Handle::start`] they're the backend's estimate of the next
/// frame; after it, the frame's own. The accessors read each field as the
/// standard defines it, `None` for a value it doesn't allow, and
/// [`check`](Self::check) says whether the whole can stand for a frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Parameters {
    /// What the frame holds.
    pub format: FrameFormat,
    /// Whether it's the image's last frame: `SANE_TRUE` (1) or
    /// `SANE_FALSE` (0), as a word.
    pub last_frame: i32,
    /// Bytes in a line, padding included.
    pub bytes_per_line: i32,
    /// Pixels in a line.
    pub pixels_per_line: i32,
    /// Lines in the frame, or -1 if they aren't known until it ends.
    pub lines: i32,
    /// Bits in a sample: 1, 8 or 16.
    pub depth: i32,
}

/// What a read gave ([`Handle::read`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ReadOutcome {
    /// `GOOD`, and this many bytes, at most what was asked for: none only
    /// in non-blocking mode, when no byte is ready yet, or from a backend
    /// that gives none.
    Bytes(usize),
    /// `EOF`: the frame has ended.
    Eof,
}

/// How many lines a frame has, as its parameters say.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Height {
    /// This many, at least one.
    Known(usize),
    /// Not known until the frame ends (`lines` is -1), as from a hand
    /// scanner or a feeder that measures each sheet.
    Unknown,
}

impl Parameters {
    pub(crate) fn from_raw(raw: ffi::SANE_Parameters) -> Parameters {
        Parameters {
            format: FrameFormat(raw.format),
            last_frame: raw.last_frame,
            bytes_per_line: raw.bytes_per_line,
            pixels_per_line: raw.pixels_per_line,
            lines: raw.lines,
            depth: raw.depth,
        }
    }

    /// Whether this is the image's last frame, or `None` for a word other
    /// than 0 or 1.
    pub fn is_last_frame(&self) -> Option<bool> {
        match self.last_frame {
            ffi::SANE_FALSE => Some(false),
            ffi::SANE_TRUE => Some(true),
            _ => None,
        }
    }

    /// The frame's height, or `None` for a `lines` the standard doesn't
    /// allow (0, or below -1).
    pub fn height(&self) -> Option<Height> {
        match self.lines {
            -1 => Some(Height::Unknown),
            lines if lines >= 1 => usize::try_from(lines).ok().map(Height::Known),
            _ => None,
        }
    }

    /// The samples in each pixel of the frame: 3 for `RGB`, 1 for `GRAY`,
    /// `RED`, `GREEN` and `BLUE`, `None` for any other format.
    pub fn channels(&self) -> Option<usize> {
        match self.format {
            FrameFormat::RGB => Some(3),
            FrameFormat::GRAY | FrameFormat::RED | FrameFormat::GREEN | FrameFormat::BLUE => {
                Some(1)
            }
            _ => None,
        }
    }

    /// The bytes a line's pixels take, by the standard's formula: a byte
    /// for every 8 pixels (rounded up) of each channel at depth 1, and a
    /// sample's bytes for each channel of each pixel at depths 8 and 16.
    /// `None` for an unknown format, a depth other than 1, 8 or 16, or no
    /// pixels.
    pub fn pixel_bytes(&self) -> Option<usize> {
        let channels = self.channels()?;
        let pixels = usize::try_from(self.pixels_per_line)
            .ok()
            .filter(|&pixels| pixels >= 1)?;
        let per_channel = match self.depth {
            1 => pixels.div_ceil(8),
            8 => pixels,
            16 => pixels.checked_mul(2)?,
            _ => return None,
        };
        per_channel.checked_mul(channels)
    }

    /// Whether these parameters can stand for a frame, within `limits`: a
    /// format SANE 1 defines (not one of the reserved ones), `last_frame` 0
    /// or 1 and 1 for a gray or RGB frame, a depth of 1, 8 or 16, at least
    /// one pixel a line, at least the bytes a line's pixels need and no
    /// more than [`Limits::line_bytes`], and `lines` -1 or at least 1, a
    /// known frame's bytes within [`Limits::frame_bytes`]. Anything else is
    /// [`Error::BadParameters`], carrying them and saying what's wrong.
    pub fn check(&self, limits: &Limits) -> Result<(), Error> {
        self.problem(limits).map_or(Ok(()), |reason| {
            Err(Error::BadParameters {
                parameters: *self,
                reason,
            })
        })
    }

    fn problem(&self, limits: &Limits) -> Option<String> {
        if self.channels().is_none() {
            return Some(if self.format.is_reserved() {
                format!("its format, {}, is reserved for a later SANE", self.format)
            } else {
                format!("its format, {}, isn't one SANE defines", self.format)
            });
        }
        let Some(last) = self.is_last_frame() else {
            return Some(format!(
                "its last_frame, {}, is neither 0 nor 1",
                self.last_frame
            ));
        };
        if !last && matches!(self.format, FrameFormat::GRAY | FrameFormat::RGB) {
            return Some(format!(
                "a {} frame is a whole image, but it isn't the last",
                self.format
            ));
        }
        if !matches!(self.depth, 1 | 8 | 16) {
            return Some(format!("its depth, {}, isn't 1, 8 or 16", self.depth));
        }
        if self.pixels_per_line < 1 {
            return Some(format!(
                "it has {} pixels a line, fewer than 1",
                self.pixels_per_line
            ));
        }
        let Some(needed) = self.pixel_bytes() else {
            return Some(
                "its lines' pixels take more bytes than this machine can count".to_owned(),
            );
        };
        if usize::try_from(self.bytes_per_line).map_or(true, |bytes| bytes < needed) {
            return Some(format!(
                "it has {} bytes a line, fewer than its pixels need ({needed})",
                self.bytes_per_line
            ));
        }
        let bytes_per_line = usize::try_from(self.bytes_per_line).unwrap_or(usize::MAX);
        if bytes_per_line > limits.line_bytes {
            return Some(format!(
                "it has {bytes_per_line} bytes a line, more than the limit of {}",
                limits.line_bytes
            ));
        }
        match self.height() {
            None => Some(format!(
                "it has {} lines, where SANE allows -1 or at least 1",
                self.lines
            )),
            Some(Height::Known(lines)) => {
                let bytes = (lines as u128) * (bytes_per_line as u128);
                (bytes > limits.frame_bytes as u128).then(|| {
                    format!(
                        "its {lines} lines of {bytes_per_line} bytes are more than the limit of {} a frame",
                        limits.frame_bytes
                    )
                })
            }
            Some(Height::Unknown) => None,
        }
    }

    /// The frame's bytes, padding included, if its height is known; `None`
    /// if it isn't, or the parameters don't [`check`](Self::check).
    fn frame_bytes(&self) -> Option<u64> {
        let Some(Height::Known(lines)) = self.height() else {
            return None;
        };
        let bytes_per_line = u64::try_from(self.bytes_per_line).ok()?;
        (lines as u64).checked_mul(bytes_per_line)
    }
}

/// `GRAY (0), last 1, 16 bytes and 16 pixels a line, 4 lines, depth 8`.
impl fmt::Display for Parameters {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}, last {}, {} bytes and {} pixels a line, {} lines, depth {}",
            self.format,
            self.last_frame,
            self.bytes_per_line,
            self.pixels_per_line,
            self.lines,
            self.depth
        )
    }
}

/// A page's image, its frames assembled: rows top down, each exactly its
/// pixels' bytes (a backend's padding dropped), a three-pass image's red,
/// green and blue frames interleaved into one RGB image.
///
/// The samples are as SANE encodes them, unchanged:
///
/// - at depth 1, eight pixels a byte, the leftmost in the high bit; in a
///   gray image 1 is black and 0 white; in an RGB image each byte holds
///   eight pixels of one channel, the bytes interleaved red, green, blue
///   (as SANE's own 1-bit RGB frames are), 0 the least intensity;
/// - at depth 8, a byte a sample, 0 the least intensity;
/// - at depth 16, two bytes a sample in this machine's byte order, 0 the
///   least intensity.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Image {
    /// [`FrameFormat::GRAY`] or [`FrameFormat::RGB`].
    pub format: FrameFormat,
    /// Bits in a sample: 1, 8 or 16.
    pub depth: u8,
    /// Pixels in a row.
    pub width: usize,
    /// Rows.
    pub height: usize,
    /// Bytes in a row: exactly its pixels'.
    pub stride: usize,
    /// The rows, top down: `height` × `stride` bytes.
    pub data: Vec<u8>,
    /// Each frame's parameters, as SANE gave them after its start, in the
    /// order they came.
    pub frames: Vec<Parameters>,
}

/// How far the page being read has come, for a progress bar.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Progress {
    /// Bytes read of the page so far, every frame's, padding included.
    pub bytes_read: u64,
    /// The page's bytes, where its frames' heights are known: those of the
    /// frames read so far, and the frame being read counted again for each
    /// still to come in a three-pass page. Never below `bytes_read`, and
    /// exact once the page's last frame has begun.
    pub bytes_expected: Option<u64>,
}

/// What [`Batch::next_page`] gives.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Page {
    /// A page, its frames read to their end and assembled.
    Image(Image),
    /// The feeder had no more documents: `NO_DOCS` from `sane_start`, or
    /// from the first read of a page, before any of its bytes. Whether an
    /// empty feeder at the first page is an error is the caller's to say.
    NoDocs {
        /// The function that said so.
        from: Call,
        /// The pages the batch delivered before it.
        pages: usize,
    },
}

/// A frame being read: its parameters, and its rows so far.
struct Frame {
    parameters: Parameters,
    bytes_per_line: usize,
    pixel_bytes: usize,
    /// The frame's bytes, if its height is known.
    expected: Option<u64>,
    /// Bytes read so far, padding included.
    read: u64,
    /// The pixels' bytes of every row so far, padding dropped.
    rows: Vec<u8>,
}

/// A page's frames, checked as they come and assembled when the last ends.
pub(crate) struct Assembly {
    limits: Limits,
    /// The frames that have ended, in order.
    done: Vec<Frame>,
    current: Option<Frame>,
    /// Bytes read of the page, every frame's.
    page_read: u64,
}

/// The channel a single-channel frame holds: 0 red, 1 green, 2 blue.
fn channel_of(format: FrameFormat) -> Option<usize> {
    match format {
        FrameFormat::RED => Some(0),
        FrameFormat::GREEN => Some(1),
        FrameFormat::BLUE => Some(2),
        _ => None,
    }
}

impl Assembly {
    pub(crate) fn new(limits: Limits) -> Assembly {
        Assembly {
            limits,
            done: Vec::new(),
            current: None,
            page_read: 0,
        }
    }

    /// Whether a byte of the page has been read.
    pub(crate) fn started(&self) -> bool {
        self.page_read > 0
    }

    /// Whether the last frame begun was the image's last.
    pub(crate) fn complete(&self) -> bool {
        self.done
            .last()
            .is_some_and(|frame| frame.parameters.last_frame == ffi::SANE_TRUE)
    }

    /// How far the page has come: the frames that have ended as they
    /// were read, and the frame being read expected for itself and for each
    /// frame still to come, since a three-pass page's frames may differ in
    /// padding.
    pub(crate) fn progress(&self) -> Progress {
        let latest = self.current.as_ref().or(self.done.last());
        let bytes_expected = latest.and_then(|frame| {
            let frames = if channel_of(frame.parameters.format).is_some() {
                3
            } else {
                1
            };
            let ended = self.done.len() as u64;
            let coming = if self.current.is_some() {
                frames - ended
            } else {
                frames.saturating_sub(ended)
            };
            let done: u64 = self.done.iter().map(|frame| frame.read).sum();
            frame.expected.map(|bytes| done + bytes * coming)
        });
        Progress {
            bytes_read: self.page_read,
            bytes_expected,
        }
    }

    /// A frame begins with these parameters: checked alone, then against
    /// the page's frames before it.
    pub(crate) fn begin_frame(&mut self, parameters: Parameters) -> Result<(), Error> {
        parameters.check(&self.limits)?;
        let bad = |reason: String| Error::BadParameters { parameters, reason };
        let channel = channel_of(parameters.format);
        if let Some(first) = self.done.first() {
            let Some(channel) = channel else {
                return Err(bad(format!(
                    "a {} frame follows a single-channel one",
                    parameters.format
                )));
            };
            let first = first.parameters;
            if (first.pixels_per_line, first.lines, first.depth)
                != (
                    parameters.pixels_per_line,
                    parameters.lines,
                    parameters.depth,
                )
            {
                return Err(bad(format!(
                    "its size or depth differs from the page's first frame ({first})"
                )));
            }
            if self
                .done
                .iter()
                .any(|frame| channel_of(frame.parameters.format) == Some(channel))
            {
                return Err(bad(format!(
                    "its channel, {}, came already in this image",
                    parameters.format
                )));
            }
        }
        if channel.is_some() {
            let channels = self.done.len() + 1;
            let last = parameters.last_frame == ffi::SANE_TRUE;
            if last && channels < 3 {
                return Err(bad(format!(
                    "the image ends with {channels} of its three channels"
                )));
            }
            if !last && channels == 3 {
                return Err(bad(
                    "all three channels came, but it isn't the image's last frame".to_owned(),
                ));
            }
        }
        // Checked: the parameters fit their limits, so these fit a usize.
        let bytes_per_line = usize::try_from(parameters.bytes_per_line).unwrap_or(usize::MAX);
        let pixel_bytes = parameters.pixel_bytes().unwrap_or(bytes_per_line);
        let expected = parameters.frame_bytes();
        let capacity = expected.map_or(0, |bytes| {
            usize::try_from(bytes / bytes_per_line as u64 * pixel_bytes as u64).unwrap_or(0)
        });
        self.current = Some(Frame {
            parameters,
            bytes_per_line,
            pixel_bytes,
            expected,
            read: 0,
            rows: Vec::with_capacity(capacity),
        });
        Ok(())
    }

    /// A read's bytes, kept but for each row's padding: [`Error::LongFrame`]
    /// past a known frame's bytes, [`Error::FrameTooLarge`] past
    /// [`Limits::frame_bytes`].
    pub(crate) fn take(&mut self, mut chunk: &[u8]) -> Result<(), Error> {
        let Some(frame) = &mut self.current else {
            return Ok(());
        };
        let length = chunk.len() as u64;
        if let Some(expected) = frame.expected
            && frame.read + length > expected
        {
            return Err(Error::LongFrame { expected });
        }
        if frame.read + length > self.limits.frame_bytes as u64 {
            return Err(Error::FrameTooLarge {
                limit: self.limits.frame_bytes,
            });
        }
        while !chunk.is_empty() {
            // `read` is past whole rows by at most a row, so this fits.
            let column = (frame.read % frame.bytes_per_line as u64) as usize;
            let in_row = (frame.bytes_per_line - column).min(chunk.len());
            let (row, rest) = chunk.split_at(in_row);
            if column < frame.pixel_bytes {
                let kept = (frame.pixel_bytes - column).min(in_row);
                frame.rows.extend_from_slice(&row[..kept]);
            }
            frame.read += in_row as u64;
            chunk = rest;
        }
        self.page_read += length;
        Ok(())
    }

    /// The frame has ended (`EOF`): [`Error::ShortFrame`] before a known
    /// frame's bytes, or before the first row of a frame of unknown height,
    /// [`Error::PartialRow`] partway into a later row of one, and a
    /// three-pass frame of unknown height whose rows differ from the
    /// first's [`Error::BadParameters`]. So every frame that ends well has
    /// a row at least, as `lines` promises when it's known.
    pub(crate) fn end_frame(&mut self) -> Result<(), Error> {
        let Some(frame) = self.current.take() else {
            return Ok(());
        };
        let bytes_per_line = frame.bytes_per_line as u64;
        match frame.expected {
            Some(expected) if frame.read < expected => {
                return Err(Error::ShortFrame {
                    expected,
                    read: frame.read,
                });
            }
            Some(_) => {}
            None if frame.read < bytes_per_line => {
                return Err(Error::ShortFrame {
                    expected: bytes_per_line,
                    read: frame.read,
                });
            }
            None => {
                let extra = frame.read % bytes_per_line;
                if extra != 0 {
                    return Err(Error::PartialRow {
                        rows: frame.read / bytes_per_line,
                        extra: extra as usize,
                        bytes_per_line: frame.bytes_per_line,
                    });
                }
            }
        }
        if let Some(first) = self.done.first()
            && first.read / first.bytes_per_line as u64 != frame.read / bytes_per_line
        {
            return Err(Error::BadParameters {
                parameters: frame.parameters,
                reason: format!(
                    "it ended after {} rows, where the page's first frame had {}",
                    frame.read / bytes_per_line,
                    first.read / first.bytes_per_line as u64
                ),
            });
        }
        self.done.push(frame);
        Ok(())
    }

    /// The page's image, once its last frame has ended.
    pub(crate) fn finish(self) -> Image {
        let frames: Vec<Parameters> = self.done.iter().map(|frame| frame.parameters).collect();
        let first = frames[0];
        let depth = u8::try_from(first.depth).unwrap_or(8);
        let width = usize::try_from(first.pixels_per_line).unwrap_or(0);
        let mut done = self.done;
        if done.len() == 1 {
            let frame = done.remove(0);
            let height = frame.rows.len() / frame.pixel_bytes;
            return Image {
                format: first.format,
                depth,
                width,
                height,
                stride: frame.pixel_bytes,
                data: frame.rows,
                frames,
            };
        }
        // Three single-channel frames, one of each, checked as they began:
        // put in red, green, blue order, then interleaved a sample at a time
        // (a byte of eight pixels at depth 1).
        done.sort_by_key(|frame| channel_of(frame.parameters.format));
        let plane = done[0].pixel_bytes;
        let height = done[0].rows.len() / plane;
        let unit = if depth == 16 { 2 } else { 1 };
        let mut data = Vec::with_capacity(done[0].rows.len() * 3);
        for at in (0..done[0].rows.len()).step_by(unit) {
            for frame in &done {
                data.extend_from_slice(&frame.rows[at..at + unit]);
            }
        }
        Image {
            format: FrameFormat::RGB,
            depth,
            width,
            height,
            stride: plane * 3,
            data,
            frames,
        }
    }
}

/// A batch of pages from one device: what a feeder holds, or a flatbed's
/// one page at a time, each read to its end and assembled
/// ([`next_page`](Self::next_page)).
///
/// SANE's batch ends with `sane_cancel`, which the batch calls however it
/// ends: when it's dropped, at `NO_DOCS`, and after any error. A batch may
/// go on after an error or `NO_DOCS` (the next page starts again, which is
/// how a frontend carries on after a jam once it's cleared); whether to is
/// the caller's call.
pub struct Batch<'a> {
    handle: &'a mut Handle,
    pages: usize,
}

impl Handle {
    /// A batch of pages ([`Batch`]).
    pub fn batch(&mut self) -> Batch<'_> {
        Batch {
            handle: self,
            pages: 0,
        }
    }
}

impl Batch<'_> {
    /// The pages the batch has delivered.
    pub fn pages(&self) -> usize {
        self.pages
    }

    /// Scans the next page and assembles it: every frame started, read in
    /// blocking mode to its end (`EOF`, whatever its height), checked
    /// ([`Parameters::check`], and against the page's frames before it),
    /// its padding dropped, and a three-pass image's frames interleaved
    /// ([`Image`]).
    ///
    /// A handle put in non-blocking mode ([`Handle::set_io_mode`]) is set
    /// back to blocking for the page; a backend that refuses (some answer
    /// `UNSUPPORTED` to either mode) is read as it is. A read that gives no
    /// bytes is made again after a pause that grows to 10 ms, for as long
    /// as the backend answers so: a byte, a status or a token's cancel ends
    /// the wait, as with a read that blocks.
    ///
    /// `on_progress` hears how far the page has come after each read that
    /// gives bytes, called with no lock held, so it may use the handle's
    /// [`CancelToken`](crate::CancelToken).
    ///
    /// `NO_DOCS` from `sane_start`, or from the page's first read before
    /// any byte, is [`Page::NoDocs`]. Any other status, from any call, and
    /// `NO_DOCS` once a byte has been read, is an error, and the page in
    /// flight is dropped: [`Error::Status`] with the call that answered;
    /// [`Error::Cancelled`] for a cancel through the token, even from a
    /// backend that ignores `sane_cancel` and goes on answering; and the
    /// frame errors ([`Error::BadParameters`], [`Error::ShortFrame`],
    /// [`Error::LongFrame`], [`Error::PartialRow`],
    /// [`Error::FrameTooLarge`], [`Error::BadReadLength`]), a frame of
    /// unknown height that ends before its first row among them
    /// (`ShortFrame`), so no page has no rows. After any of them
    /// `sane_cancel` has been called.
    pub fn next_page(&mut self, mut on_progress: impl FnMut(Progress)) -> Result<Page, Error> {
        let limits = self.handle.limits();
        let mut assembly = Assembly::new(limits);
        let mut buffer = vec![0; limits.read_chunk.max(1)];
        loop {
            match self.handle.start() {
                Ok(()) => {}
                Err(Error::Status {
                    status: Status::NO_DOCS,
                    call: Call::Start,
                }) if !assembly.started() => {
                    return Ok(Page::NoDocs {
                        from: Call::Start,
                        pages: self.pages,
                    });
                }
                Err(error) => return Err(error),
            }
            // A page reads blocking, as a device opens. Only a handle the
            // step-by-step calls put in non-blocking mode is set back, and
            // whatever that answers, the page goes on: some backends answer
            // `UNSUPPORTED` to either mode, and read blocking all the same,
            // and one left non-blocking is read as an empty read is, below.
            if self.handle.is_non_blocking() {
                let _ = self.handle.set_io_mode(false);
            }
            let begun = self
                .handle
                .parameters()
                .and_then(|parameters| assembly.begin_frame(parameters));
            if let Err(error) = begun {
                self.handle.cancel();
                return Err(error);
            }
            let mut pause = Duration::ZERO;
            loop {
                match self.handle.read(&mut buffer) {
                    Ok(ReadOutcome::Bytes(0)) => {
                        // A blocking read waits for a byte, but a backend may
                        // answer `GOOD` with none (one left non-blocking, or
                        // one that doesn't keep the rule): it's read again,
                        // after a pause that grows to `EMPTY_READ_PAUSE`, so
                        // waiting costs no processor. A byte, a status or a
                        // token's cancel ends the wait.
                        pause = (pause * 2).clamp(Duration::from_micros(100), EMPTY_READ_PAUSE);
                        std::thread::sleep(pause);
                    }
                    Ok(ReadOutcome::Bytes(length)) => {
                        pause = Duration::ZERO;
                        if let Err(error) = assembly.take(&buffer[..length]) {
                            self.handle.cancel();
                            return Err(error);
                        }
                        on_progress(assembly.progress());
                    }
                    Ok(ReadOutcome::Eof) => break,
                    Err(Error::Status {
                        status: Status::NO_DOCS,
                        call: Call::Read,
                    }) if !assembly.started() => {
                        return Ok(Page::NoDocs {
                            from: Call::Read,
                            pages: self.pages,
                        });
                    }
                    Err(error) => return Err(error),
                }
            }
            if let Err(error) = assembly.end_frame() {
                self.handle.cancel();
                return Err(error);
            }
            if assembly.complete() {
                break;
            }
        }
        self.pages += 1;
        Ok(Page::Image(assembly.finish()))
    }
}

impl Drop for Batch<'_> {
    fn drop(&mut self) {
        self.handle.end_scan();
    }
}

impl fmt::Debug for Batch<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Batch")
            .field("handle", &self.handle)
            .field("pages", &self.pages)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(
        format: FrameFormat,
        last: i32,
        bpl: i32,
        ppl: i32,
        lines: i32,
        depth: i32,
    ) -> Parameters {
        Parameters {
            format,
            last_frame: last,
            bytes_per_line: bpl,
            pixels_per_line: ppl,
            lines,
            depth,
        }
    }

    fn gray(bpl: i32, ppl: i32, lines: i32, depth: i32) -> Parameters {
        frame(FrameFormat::GRAY, 1, bpl, ppl, lines, depth)
    }

    fn reason(parameters: Parameters) -> String {
        match parameters.check(&Limits::default()) {
            Err(Error::BadParameters {
                reason,
                parameters: given,
            }) => {
                assert_eq!(given, parameters);
                reason
            }
            other => panic!("{parameters}: {other:?}"),
        }
    }

    /// Assembles a page from its frames, each given as parameters and the
    /// chunks read.
    fn assemble(frames: &[(Parameters, &[&[u8]])], limits: Limits) -> Result<Image, Error> {
        let mut assembly = Assembly::new(limits);
        for (parameters, chunks) in frames {
            assembly.begin_frame(*parameters)?;
            for chunk in *chunks {
                assembly.take(chunk)?;
            }
            assembly.end_frame()?;
        }
        assert!(assembly.complete());
        Ok(assembly.finish())
    }

    #[test]
    fn accessors_read_each_field_as_the_standard_does() {
        let parameters = gray(3, 20, 4, 1);
        assert_eq!(parameters.is_last_frame(), Some(true));
        assert_eq!(parameters.height(), Some(Height::Known(4)));
        assert_eq!(parameters.channels(), Some(1));
        assert_eq!(parameters.pixel_bytes(), Some(3));
        let rgb = frame(FrameFormat::RGB, 1, 120, 20, -1, 16);
        assert_eq!(rgb.height(), Some(Height::Unknown));
        assert_eq!(rgb.channels(), Some(3));
        assert_eq!(rgb.pixel_bytes(), Some(120));
        assert_eq!(
            frame(FrameFormat::RGB, 1, 9, 17, 1, 1).pixel_bytes(),
            Some(9)
        );
        assert_eq!(
            frame(FrameFormat::RED, 0, 17, 17, 1, 8).is_last_frame(),
            Some(false)
        );
        for odd in [
            frame(FrameFormat(9), 2, 1, 0, 0, 3),
            frame(FrameFormat::JPEG, -1, 1, 1, -2, 8),
        ] {
            assert_eq!(odd.is_last_frame(), None);
            assert_eq!(odd.height(), None);
            assert_eq!(odd.channels(), None);
            assert_eq!(odd.pixel_bytes(), None);
        }
        assert_eq!(
            gray(1, i32::MAX, 1, 16).pixel_bytes(),
            Some(i32::MAX as usize * 2)
        );
        assert_eq!(
            gray(10, 4, 2, 8).to_string(),
            "GRAY (0), last 1, 10 bytes and 4 pixels a line, 2 lines, depth 8"
        );
    }

    #[test]
    fn parameters_that_stand_for_a_frame_pass() {
        let limits = Limits::default();
        for parameters in [
            gray(2, 16, 4, 1),
            gray(3, 17, 1, 1),
            gray(16, 16, -1, 8),
            gray(19, 16, 9, 8),
            gray(32, 16, 4, 16),
            frame(FrameFormat::RGB, 1, 48, 16, 4, 8),
            frame(FrameFormat::RGB, 1, 6, 16, 4, 1),
            frame(FrameFormat::BLUE, 0, 16, 16, 4, 8),
            frame(FrameFormat::GREEN, 1, 16, 16, 4, 8),
        ] {
            assert_eq!(parameters.check(&limits).ok(), Some(()), "{parameters}");
        }
    }

    #[test]
    fn parameters_that_make_no_sense_are_refused_saying_why() {
        let cases = [
            (
                frame(FrameFormat(7), 1, 16, 16, 4, 8),
                "its format, 7 (not a SANE frame format), isn't one SANE defines",
            ),
            (
                frame(FrameFormat(-1), 1, 16, 16, 4, 8),
                "its format, -1 (not a SANE frame format), isn't one SANE defines",
            ),
            (
                frame(FrameFormat::JPEG, 1, 16, 16, 4, 8),
                "its format, JPEG (11, reserved), is reserved for a later SANE",
            ),
            (
                frame(FrameFormat::G42D, 1, 16, 16, 4, 8),
                "its format, G42D (14, reserved), is reserved for a later SANE",
            ),
            (
                frame(FrameFormat::GRAY, 2, 16, 16, 4, 8),
                "its last_frame, 2, is neither 0 nor 1",
            ),
            (
                frame(FrameFormat::GRAY, 0, 16, 16, 4, 8),
                "a GRAY (0) frame is a whole image, but it isn't the last",
            ),
            (
                frame(FrameFormat::RGB, 0, 48, 16, 4, 8),
                "a RGB (1) frame is a whole image, but it isn't the last",
            ),
            (gray(16, 16, 4, 0), "its depth, 0, isn't 1, 8 or 16"),
            (gray(16, 16, 4, 3), "its depth, 3, isn't 1, 8 or 16"),
            (gray(64, 16, 4, 32), "its depth, 32, isn't 1, 8 or 16"),
            (gray(16, 0, 4, 8), "it has 0 pixels a line, fewer than 1"),
            (gray(16, -5, 4, 8), "it has -5 pixels a line, fewer than 1"),
            (
                gray(15, 16, 4, 8),
                "it has 15 bytes a line, fewer than its pixels need (16)",
            ),
            (
                gray(1, 9, 4, 1),
                "it has 1 bytes a line, fewer than its pixels need (2)",
            ),
            (
                gray(31, 16, 4, 16),
                "it has 31 bytes a line, fewer than its pixels need (32)",
            ),
            (
                frame(FrameFormat::RGB, 1, 47, 16, 4, 8),
                "it has 47 bytes a line, fewer than its pixels need (48)",
            ),
            (
                gray(-16, 16, 4, 8),
                "it has -16 bytes a line, fewer than its pixels need (16)",
            ),
            (
                gray(2500, 2500, 0, 8),
                "it has 0 lines, where SANE allows -1 or at least 1",
            ),
            (
                gray(16, 16, -2, 8),
                "it has -2 lines, where SANE allows -1 or at least 1",
            ),
            (
                gray(16, 16, i32::MIN, 8),
                "it has -2147483648 lines, where SANE allows -1 or at least 1",
            ),
        ];
        for (parameters, expected) in cases {
            assert_eq!(reason(parameters), expected, "{parameters}");
        }
    }

    #[test]
    fn parameters_past_the_caps_are_refused() {
        let limits = Limits {
            line_bytes: 100,
            frame_bytes: 1000,
            ..Limits::default()
        };
        assert!(gray(100, 100, 10, 8).check(&limits).is_ok());
        assert!(gray(100, 100, -1, 8).check(&limits).is_ok());
        let Err(Error::BadParameters { reason, .. }) = gray(101, 100, 1, 8).check(&limits) else {
            panic!()
        };
        assert_eq!(
            reason,
            "it has 101 bytes a line, more than the limit of 100"
        );
        let Err(Error::BadParameters { reason, .. }) = gray(100, 100, 11, 8).check(&limits) else {
            panic!()
        };
        assert_eq!(
            reason,
            "its 11 lines of 100 bytes are more than the limit of 1000 a frame"
        );
        // The default caps: 16 MiB a line, 1 GiB a frame.
        let defaults = Limits::default();
        assert!(gray(16 << 20, 16 << 20, 64, 8).check(&defaults).is_ok());
        assert!(
            gray((16 << 20) + 1, 16 << 20, 1, 8)
                .check(&defaults)
                .is_err()
        );
        assert!(gray(16 << 20, 16 << 20, 65, 8).check(&defaults).is_err());
        assert!(gray(i32::MAX, 16, i32::MAX, 8).check(&defaults).is_err());
    }

    #[test]
    fn gray_rows_keep_their_pixels_and_drop_their_padding() {
        // 3 pixels of 8 bits in rows of 5 bytes, read in chunks that cross
        // rows anywhere.
        let rows = [
            1, 2, 3, 0xEE, 0xEE, 4, 5, 6, 0xEE, 0xEE, 7, 8, 9, 0xEE, 0xEE,
        ];
        let parameters = gray(5, 3, 3, 8);
        for split in [1, 2, 4, 5, 7, 15] {
            let chunks: Vec<&[u8]> = rows.chunks(split).collect();
            let image = assemble(&[(parameters, &chunks)], Limits::default()).unwrap();
            assert_eq!(
                image,
                Image {
                    format: FrameFormat::GRAY,
                    depth: 8,
                    width: 3,
                    height: 3,
                    stride: 3,
                    data: vec![1, 2, 3, 4, 5, 6, 7, 8, 9],
                    frames: vec![parameters],
                },
                "chunks of {split}"
            );
        }
    }

    #[test]
    fn one_bit_and_sixteen_bit_rows_stay_as_sane_gives_them() {
        // 10 pixels at depth 1: two bytes a row, the high bits first, one
        // byte of padding.
        let parameters = gray(3, 10, 2, 1);
        let image = assemble(
            &[(
                parameters,
                &[&[0b1010_1010, 0b1100_0000, 0xEE, 0xFF, 0xC0, 0xEE]],
            )],
            Limits::default(),
        )
        .unwrap();
        assert_eq!((image.stride, image.height, image.depth), (2, 2, 1));
        assert_eq!(image.data, [0b1010_1010, 0b1100_0000, 0xFF, 0xC0]);
        // Two 16-bit pixels, their bytes untouched.
        let parameters = gray(4, 2, 1, 16);
        let image = assemble(
            &[(parameters, &[&[0x34, 0x12, 0xFF, 0x00]])],
            Limits::default(),
        )
        .unwrap();
        assert_eq!((image.stride, image.width, image.depth), (4, 2, 16));
        assert_eq!(image.data, [0x34, 0x12, 0xFF, 0x00]);
    }

    #[test]
    fn an_unknown_height_is_found_at_the_end() {
        let parameters = gray(4, 3, -1, 8);
        let image = assemble(
            &[(parameters, &[&[1, 2, 3, 0], &[4, 5], &[6, 0, 7, 8, 9, 0]])],
            Limits::default(),
        )
        .unwrap();
        assert_eq!((image.height, image.data.len()), (3, 9));
        assert_eq!(image.frames[0].lines, -1);
    }

    #[test]
    fn three_pass_frames_are_interleaved_in_any_order() {
        let red: &[u8] = &[10, 11, 0xEE, 12, 13, 0xEE];
        let green: &[u8] = &[20, 21, 0xEE, 22, 23, 0xEE];
        let blue: &[u8] = &[30, 31, 0xEE, 32, 33, 0xEE];
        let expected = vec![10, 20, 30, 11, 21, 31, 12, 22, 32, 13, 23, 33];
        let orders = [
            [
                (FrameFormat::RED, red),
                (FrameFormat::GREEN, green),
                (FrameFormat::BLUE, blue),
            ],
            [
                (FrameFormat::RED, red),
                (FrameFormat::BLUE, blue),
                (FrameFormat::GREEN, green),
            ],
            [
                (FrameFormat::GREEN, green),
                (FrameFormat::BLUE, blue),
                (FrameFormat::RED, red),
            ],
            [
                (FrameFormat::GREEN, green),
                (FrameFormat::RED, red),
                (FrameFormat::BLUE, blue),
            ],
            [
                (FrameFormat::BLUE, blue),
                (FrameFormat::RED, red),
                (FrameFormat::GREEN, green),
            ],
            [
                (FrameFormat::BLUE, blue),
                (FrameFormat::GREEN, green),
                (FrameFormat::RED, red),
            ],
        ];
        for order in orders {
            let frames: Vec<(Parameters, Vec<&[u8]>)> = order
                .iter()
                .enumerate()
                .map(|(n, (format, bytes))| {
                    (frame(*format, i32::from(n == 2), 3, 2, 2, 8), vec![*bytes])
                })
                .collect();
            let frames: Vec<(Parameters, &[&[u8]])> =
                frames.iter().map(|(p, c)| (*p, c.as_slice())).collect();
            let image = assemble(&frames, Limits::default()).unwrap();
            assert_eq!(image.format, FrameFormat::RGB);
            assert_eq!((image.width, image.height, image.stride), (2, 2, 6));
            assert_eq!(image.data, expected, "{order:?}");
            assert_eq!(image.frames.len(), 3);
            assert_eq!(image.frames[0].format, order[0].0);
        }
    }

    #[test]
    fn three_pass_samples_interleave_by_their_size() {
        let frames16 = [
            (
                frame(FrameFormat::RED, 0, 4, 2, 1, 16),
                &[&[1u8, 2, 3, 4][..]][..],
            ),
            (
                frame(FrameFormat::GREEN, 0, 4, 2, 1, 16),
                &[&[5, 6, 7, 8][..]][..],
            ),
            (
                frame(FrameFormat::BLUE, 1, 4, 2, 1, 16),
                &[&[9, 10, 11, 12][..]][..],
            ),
        ];
        let image = assemble(&frames16, Limits::default()).unwrap();
        assert_eq!(image.data, [1, 2, 5, 6, 9, 10, 3, 4, 7, 8, 11, 12]);
        assert_eq!(image.stride, 12);
        // At depth 1 a byte holds eight pixels of one channel, so bytes
        // interleave, as SANE's own 1-bit RGB frames are laid out.
        let frames1 = [
            (
                frame(FrameFormat::RED, 0, 2, 9, 1, 1),
                &[&[0xA1u8, 0x80][..]][..],
            ),
            (
                frame(FrameFormat::GREEN, 0, 2, 9, 1, 1),
                &[&[0xB2, 0x00][..]][..],
            ),
            (
                frame(FrameFormat::BLUE, 1, 2, 9, 1, 1),
                &[&[0xC3, 0x80][..]][..],
            ),
        ];
        let image = assemble(&frames1, Limits::default()).unwrap();
        assert_eq!(image.data, [0xA1, 0xB2, 0xC3, 0x80, 0x00, 0x80]);
        assert_eq!((image.stride, image.width, image.depth), (6, 9, 1));
    }

    #[test]
    fn frames_that_dont_fit_together_are_refused() {
        let red = frame(FrameFormat::RED, 0, 2, 2, 1, 8);
        let begin = |frames: &[Parameters]| -> String {
            let mut assembly = Assembly::new(Limits::default());
            for (n, parameters) in frames.iter().enumerate() {
                let begun = assembly.begin_frame(*parameters);
                if n + 1 == frames.len() {
                    let Err(Error::BadParameters {
                        reason,
                        parameters: given,
                    }) = begun
                    else {
                        panic!("{frames:?}: {begun:?}")
                    };
                    assert_eq!(given, *parameters);
                    return reason;
                }
                begun.unwrap();
                assembly.take(&[1, 2]).unwrap();
                assembly.end_frame().unwrap();
            }
            unreachable!()
        };
        assert_eq!(
            begin(&[red, gray(2, 2, 1, 8)]),
            "a GRAY (0) frame follows a single-channel one"
        );
        assert_eq!(
            begin(&[red, frame(FrameFormat::RED, 0, 2, 2, 1, 8)]),
            "its channel, RED (2), came already in this image"
        );
        assert_eq!(
            begin(&[red, frame(FrameFormat::GREEN, 0, 3, 3, 1, 8)]),
            "its size or depth differs from the page's first frame (RED (2), last 0, 2 bytes and 2 pixels a line, 1 lines, depth 8)"
        );
        assert_eq!(
            begin(&[red, frame(FrameFormat::GREEN, 0, 4, 2, 1, 16)])
                .split(" (")
                .next(),
            Some("its size or depth differs from the page's first frame")
        );
        assert_eq!(
            begin(&[red, frame(FrameFormat::GREEN, 0, 2, 2, 2, 8)])
                .split(" (")
                .next(),
            Some("its size or depth differs from the page's first frame")
        );
        assert_eq!(
            begin(&[red, frame(FrameFormat::GREEN, 1, 2, 2, 1, 8)]),
            "the image ends with 2 of its three channels"
        );
        assert_eq!(
            begin(&[frame(FrameFormat::BLUE, 1, 2, 2, 1, 8)]),
            "the image ends with 1 of its three channels"
        );
        assert_eq!(
            begin(&[
                red,
                frame(FrameFormat::GREEN, 0, 2, 2, 1, 8),
                frame(FrameFormat::BLUE, 0, 2, 2, 1, 8)
            ]),
            "all three channels came, but it isn't the image's last frame"
        );
        // Padding may differ between a page's frames.
        let mut assembly = Assembly::new(Limits::default());
        assembly.begin_frame(red).unwrap();
        assembly.take(&[1, 2]).unwrap();
        assembly.end_frame().unwrap();
        assert!(
            assembly
                .begin_frame(frame(FrameFormat::GREEN, 0, 5, 2, 1, 8))
                .is_ok()
        );
    }

    #[test]
    fn frames_that_end_early_late_or_partway_are_refused() {
        let known = gray(4, 3, 2, 8);
        match assemble(&[(known, &[&[1, 2, 3, 0, 4, 5]])], Limits::default()) {
            Err(Error::ShortFrame {
                expected: 8,
                read: 6,
            }) => {}
            other => panic!("{other:?}"),
        }
        match assemble(
            &[(known, &[&[1, 2, 3, 0, 4, 5, 6, 0], &[7]])],
            Limits::default(),
        ) {
            Err(Error::LongFrame { expected: 8 }) => {}
            other => panic!("{other:?}"),
        }
        let unknown = gray(4, 3, -1, 8);
        match assemble(&[(unknown, &[&[1, 2, 3, 0, 4, 5]])], Limits::default()) {
            Err(Error::PartialRow {
                rows: 1,
                extra: 2,
                bytes_per_line: 4,
            }) => {}
            other => panic!("{other:?}"),
        }
        // A frame of unknown height with no rows, or part of one: no image,
        // as `lines = 0` isn't one.
        for chunks in [&[][..], &[&[1u8, 2][..]][..]] {
            match assemble(&[(unknown, chunks)], Limits::default()) {
                Err(Error::ShortFrame { expected: 4, read }) => {
                    assert_eq!(read, chunks.concat().len() as u64);
                }
                other => panic!("{other:?}"),
            }
        }
        let mut assembly = Assembly::new(Limits::default());
        assembly
            .begin_frame(frame(FrameFormat::RED, 0, 1, 1, -1, 8))
            .unwrap();
        assert!(matches!(
            assembly.end_frame(),
            Err(Error::ShortFrame {
                expected: 1,
                read: 0
            })
        ));
        // A three-pass page of unknown height whose frames end at different
        // rows.
        let mut assembly = Assembly::new(Limits::default());
        assembly
            .begin_frame(frame(FrameFormat::RED, 0, 1, 1, -1, 8))
            .unwrap();
        assembly.take(&[1, 2, 3]).unwrap();
        assembly.end_frame().unwrap();
        assembly
            .begin_frame(frame(FrameFormat::GREEN, 0, 1, 1, -1, 8))
            .unwrap();
        assembly.take(&[1, 2]).unwrap();
        let Err(Error::BadParameters { reason, .. }) = assembly.end_frame() else {
            panic!()
        };
        assert_eq!(
            reason,
            "it ended after 2 rows, where the page's first frame had 3"
        );
    }

    #[test]
    fn an_endless_stream_stops_at_the_frame_cap() {
        let limits = Limits {
            frame_bytes: 10,
            ..Limits::default()
        };
        let mut assembly = Assembly::new(limits);
        assembly.begin_frame(gray(2, 2, -1, 8)).unwrap();
        assembly.take(&[0; 6]).unwrap();
        assembly.take(&[0; 4]).unwrap();
        match assembly.take(&[0]) {
            Err(Error::FrameTooLarge { limit: 10 }) => {}
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn progress_counts_the_page_and_expects_what_its_frames_say() {
        let mut assembly = Assembly::new(Limits::default());
        assert_eq!(
            assembly.progress(),
            Progress {
                bytes_read: 0,
                bytes_expected: None
            }
        );
        assembly
            .begin_frame(frame(FrameFormat::GREEN, 0, 4, 3, 2, 8))
            .unwrap();
        assembly.take(&[0; 5]).unwrap();
        assert_eq!(
            assembly.progress(),
            Progress {
                bytes_read: 5,
                bytes_expected: Some(24)
            }
        );
        assert!(assembly.started());
        let mut unknown = Assembly::new(Limits::default());
        unknown.begin_frame(gray(4, 3, -1, 8)).unwrap();
        unknown.take(&[0; 4]).unwrap();
        assert_eq!(unknown.progress().bytes_expected, None);
        // Three passes whose padding differs: each frame as read, and the
        // one being read for itself and those to come, so the count never
        // passes what's expected.
        let mut padded = Assembly::new(Limits::default());
        let mut heard = Vec::new();
        for (format, last, bpl) in [
            (FrameFormat::RED, 0, 2),
            (FrameFormat::GREEN, 0, 6),
            (FrameFormat::BLUE, 1, 6),
        ] {
            padded
                .begin_frame(frame(format, last, bpl, 2, 2, 8))
                .unwrap();
            padded.take(&vec![0; bpl as usize * 2]).unwrap();
            let progress = padded.progress();
            heard.push((progress.bytes_read, progress.bytes_expected));
            padded.end_frame().unwrap();
        }
        assert_eq!(heard, [(4, Some(12)), (16, Some(28)), (28, Some(28))]);
        assert_eq!(padded.finish().data.len(), 12);
    }
}
