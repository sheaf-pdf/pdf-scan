//! What can go wrong: a triplet TWAIN refused, a DSM that can't be loaded or
//! trusted, a TWAIN thread that couldn't start.

use core::fmt;
use std::path::PathBuf;

use crate::codes::{ConditionCode, ReturnCode, Triplet};
use crate::container::ContainerError;

/// A step TWAIN refused, or data that doesn't hold up.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// A triplet returned other than it should have. The condition codes are
    /// each party's own, each asked once, the DSM first: `None` if it had
    /// none, or wasn't asked (the source, when none was open).
    #[error("{triplet}: {rc}{}{}", by(" (source: ", .by_source), by(" (DSM: ", .by_dsm))]
    Failed {
        triplet: Triplet,
        rc: ReturnCode,
        by_source: Option<ConditionCode>,
        by_dsm: Option<ConditionCode>,
    },
    /// The DSM couldn't be loaded.
    #[error(transparent)]
    Load(#[from] LoadError),
    /// A container or block, a source's or one to send, that doesn't hold
    /// up.
    #[error(transparent)]
    Container(#[from] ContainerError),
    /// No source of that identity.
    #[error("no such source")]
    NoSuchSource,
    /// Callback delivery asked of a source that can't call back: one of
    /// TWAIN 1.x, without `DF_DS2`, signals only through `MSG_PROCESSEVENT`.
    #[error("the source isn't a TWAIN 2 source, and can't call back")]
    CallbackUnsupported,
    /// A block for the DSM couldn't be allocated.
    #[error("a block of {size} bytes couldn't be allocated")]
    Allocation { size: usize },
    /// A memory transfer's buffer smaller than the source's smallest
    /// (`TW_SETUPMEMXFER`'s `MinBufSize`).
    #[error("a buffer of {len} bytes, and the source takes {min} at least")]
    BufferTooSmall { len: usize, min: u32 },
}

fn by(label: &str, code: &Option<ConditionCode>) -> String {
    code.map(|c| format!("{label}{c})")).unwrap_or_default()
}

/// Why the DSM wasn't loaded. Its file is checked before any of its code runs:
/// held open against change, its signature and signer read, its machine read;
/// so each refusal but the last two happens before it's loaded.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum LoadError {
    /// No file there.
    #[error("{} isn't there", path.display())]
    NotFound { path: PathBuf },
    /// The file couldn't be opened for reading, others kept from writing or
    /// deleting it while it's checked and loaded.
    #[error("{} couldn't be opened: {error}", path.display())]
    Open {
        path: PathBuf,
        error: std::io::Error,
    },
    /// A DLL of that name is already loaded in this process, from another
    /// file: loading would hand back that one.
    #[error("another {name} is loaded already, from {}", loaded.display())]
    Elsewhere { name: String, loaded: PathBuf },
    /// The file has no signature: none in it, and none for it in the
    /// system's catalogs.
    #[error("the file has no signature")]
    Unsigned,
    /// The signature doesn't verify: `WinVerifyTrust`'s answer.
    #[error("the signature doesn't verify ({code:#010x})")]
    BadSignature { code: i32 },
    /// Signed and valid, but not by the TWAIN Working Group: Windows' own
    /// DLLs, signed through catalogs, are Microsoft's.
    #[error("signed by {subject:?}, not the TWAIN Working Group")]
    WrongSigner { subject: String },
    /// Built for another machine than this process's.
    #[error("built for machine {found:#06x?}, and this process is {expected:#06x}")]
    Machine { found: Option<u16>, expected: u16 },
    /// `LoadLibraryExW` refused it: its error.
    #[error("the loader refused it (error {code})")]
    Loader { code: u32 },
    /// Loaded, the module isn't the file checked.
    #[error("the module loaded is {}, not the file checked", loaded.display())]
    NotTheFileChecked { loaded: PathBuf },
    /// It exports no `DSM_Entry`.
    #[error("it exports no DSM_Entry")]
    MissingEntry,
    /// This TWAIN thread has the DSM loaded already: one at a time.
    #[error("the DSM is loaded already on this TWAIN thread")]
    AlreadyLoaded,
}

/// Why a TWAIN thread couldn't start.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum RunError {
    /// Another `run` is going on in this process, on this thread or another:
    /// the DSM has no locks, so one thread uses it at a time. Also every `run`
    /// after one whose teardown couldn't close the DSM's connection.
    #[error("TWAIN is already running in this process")]
    AlreadyRunning,
    /// The thread couldn't be made a single-threaded apartment: it's in a
    /// multithreaded one already, or COM refused (its `HRESULT`).
    #[error("the thread couldn't enter a single-threaded apartment ({code:#010x})")]
    Apartment { code: i32 },
    /// The hidden window couldn't be made: the error.
    #[error("the hidden window couldn't be made (error {code})")]
    Window { code: u32 },
}

/// A step refused: the error, and the state it was asked of, unchanged.
/// Dropping it drops the state, which steps down.
#[must_use = "dropping it steps the state down; `into_state` keeps it"]
pub struct Refused<S> {
    pub(crate) state: S,
    pub(crate) error: Error,
}

impl<S> Refused<S> {
    /// The state, to try again or do otherwise; the error dropped.
    pub fn into_state(self) -> S {
        self.state
    }

    /// The error; the state dropped, so stepped down.
    pub fn into_error(self) -> Error {
        self.error
    }

    /// Both.
    pub fn into_parts(self) -> (S, Error) {
        (self.state, self.error)
    }

    pub fn error(&self) -> &Error {
        &self.error
    }
}

impl<S> fmt::Debug for Refused<S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Refused")
            .field("error", &self.error)
            .finish_non_exhaustive()
    }
}

impl<S> fmt::Display for Refused<S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.error, f)
    }
}

impl<S> std::error::Error for Refused<S> {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.error)
    }
}

/// A request posted after its TWAIN thread's `run` ended, handed back.
pub struct Gone<T>(pub T);

impl<T> fmt::Debug for Gone<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Gone(..)")
    }
}

impl<T> fmt::Display for Gone<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("the TWAIN thread's run has ended")
    }
}

impl<T> std::error::Error for Gone<T> {}
