//! What can go wrong with a worker, beside what goes wrong with its device.

use std::{io, path::PathBuf, process::ExitStatus, time::Duration};

use scan_device::{Driver, LayoutError, ScanError};

/// Why a call through a [`Client`](crate::Client) or a [`RemoteDevice`](crate::RemoteDevice)
/// failed: the device's own error, as the worker's backend gave it, or the worker's failure.
///
/// Every failure of the worker but [`Error::Unsendable`] ends it: it's killed if it hasn't
/// ended already, and every later call on the same device answers [`Error::Ended`]. A batch that
/// fails, whichever way, keeps the pages it sent before the failure: they're the caller's
/// already.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// The device's or the driver's error, as the worker's backend said it. The worker goes on.
    #[error(transparent)]
    Scan(#[from] ScanError),
    /// No worker was given for this driver API ([`Client::worker`](crate::Client::worker)).
    #[error("no worker is given for {0:?} devices")]
    NoWorker(Driver),
    /// The worker wasn't started: its program or the work directory isn't an absolute path, its
    /// private directory or its SANE configuration couldn't be written, or the system couldn't
    /// start it.
    #[error("couldn't start the worker {}", program.display())]
    Start {
        /// The worker's program.
        program: PathBuf,
        /// What went wrong.
        #[source]
        source: io::Error,
    },
    /// The worker said nothing for longer than its time allows ([`Timeouts`](crate::Timeouts)),
    /// and was killed.
    #[error("the worker said nothing for {silence:?}, and was ended")]
    Stopped {
        /// How long it had been silent.
        silence: Duration,
        /// The end of what it wrote to its standard error, its last 8 KiB at most, for a log.
        stderr: String,
    },
    /// The worker's output ended unasked: it crashed, exited or was ended by a signal.
    #[error("{}", crashed(.status, .page_cut_short))]
    Crashed {
        /// How it ended, where it had within [`Timeouts::grace`](crate::Timeouts::grace) of its
        /// output's end; `None` where it hadn't, and was killed.
        status: Option<ExitStatus>,
        /// Whether its output ended within a page's bytes: that page is dropped, never given
        /// as a page.
        page_cut_short: bool,
        /// The end of what it wrote to its standard error, its last 8 KiB at most, for a log.
        stderr: String,
    },
    /// The worker said what the client can't take, and was killed: it can't be trusted to be
    /// in step.
    #[error("the worker broke the protocol: {0}")]
    Protocol(ProtocolError),
    /// The request holds a value JSON can't carry, such as a paper size that isn't finite:
    /// nothing was sent, and the worker goes on.
    #[error("the request holds a value the protocol can't carry, such as a size that isn't finite")]
    Unsendable,
    /// An earlier failure ended the worker, so the device is closed.
    #[error("the worker ended at an earlier failure")]
    Ended,
}

/// `Crashed`'s message.
fn crashed(status: &Option<ExitStatus>, page_cut_short: &bool) -> String {
    let mut message = "the worker ended unasked".to_owned();
    match status {
        Some(status) => message.push_str(&format!(" ({status})")),
        None => message.push_str(" (it hadn't exited, and was killed)"),
    }
    if *page_cut_short {
        message.push_str(", a page cut short");
    }
    message
}

/// How a worker broke the protocol ([`Error::Protocol`]).
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum ProtocolError {
    /// The worker speaks another version of the protocol: a worker and its client ship
    /// together.
    #[error("the worker speaks protocol {worker}, and this client {client}")]
    Version {
        /// The worker's, from its `hello`.
        worker: u32,
        /// This client's, [`PROTOCOL`](crate::PROTOCOL).
        client: u32,
    },
    /// The worker's first line was another message than its `hello`.
    #[error("the worker's first line wasn't its hello")]
    NoHello,
    /// A line longer than the cap ([`Limits::line`](crate::Limits::line)), which was never read
    /// whole.
    #[error("the worker wrote a line longer than {limit} bytes")]
    LineTooLong {
        /// The cap.
        limit: usize,
    },
    /// A line that isn't a message.
    #[error("the worker wrote a line that isn't a message: {message}")]
    Unreadable {
        /// Why it isn't, for a log.
        message: String,
    },
    /// A page larger than the cap ([`Limits::page`](crate::Limits::page)), refused before any
    /// of its bytes was read.
    #[error("the worker sent a page of {bytes} bytes, past the {limit}-byte cap")]
    PageTooLarge {
        /// The bytes its line stated.
        bytes: u64,
        /// The cap.
        limit: u64,
    },
    /// A raw page whose line states another length than its format and size make.
    #[error("the worker sent a page of {stated} bytes, where its format and size make {expected}")]
    PageLength {
        /// The bytes its format and size make.
        expected: u64,
        /// The bytes its line stated.
        stated: u64,
    },
    /// A raw page the layout refuses: a size it can't hold, or in black and white, bits set past
    /// a row's last pixel.
    #[error("the worker sent a page outside the raw layout: {0}")]
    Layout(LayoutError),
    /// A value the device model forbids, such as caps with no source, a page at 0 dpi, or a
    /// device of another driver API than the worker's.
    #[error("the worker sent {what}")]
    Invalid {
        /// What it sent.
        what: &'static str,
    },
    /// A message out of its place: not the answer to the request made, or a batch's event
    /// outside a batch.
    #[error("the worker sent `{got}` where it had no place")]
    Unexpected {
        /// The message's name.
        got: &'static str,
    },
    /// The worker refused a request the client made, or said it can't carry a batch's event:
    /// the two are out of step.
    #[error("the worker refused a request: {reason}")]
    Refused {
        /// Its reason, for a log.
        reason: String,
    },
}
