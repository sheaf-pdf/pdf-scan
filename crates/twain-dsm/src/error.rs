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
    /// A memory transfer's buffer larger than the source's largest
    /// (`MaxBufSize`).
    #[error("a buffer of {len} bytes, and the source takes {max} at most")]
    BufferTooLarge { len: usize, max: u32 },
    /// A memory transfer's buffer of an odd length, or not at a 4-byte
    /// boundary: TWAIN asks both.
    #[error("a buffer of {len} bytes of odd length or off a 4-byte boundary")]
    BufferMisaligned { len: usize },
    /// A memory transfer's buffer of another length than its first: TWAIN
    /// asks one for the whole transfer.
    #[error("a buffer of {len} bytes, after one of {first}")]
    BufferSizeChanged { len: usize, first: usize },
    /// The source's buffer sizes no buffer can meet: its smallest past 64 MiB,
    /// or larger than its largest.
    #[error("the source's buffer sizes can't be met (min {min}, max {max}, preferred {preferred})")]
    BufferSizes { min: u32, max: u32, preferred: u32 },
    /// The source asked to be closed (`MSG_CLOSEDSREQ`) during a transfer, and
    /// is back in state 5: the user cancelled on its window, or it failed.
    #[error("the source asked to be closed")]
    CloseRequested,
    /// Image info or extended image info asked of a transfer that didn't end
    /// with `TWRC_XFERDONE`: TWAIN allows them only then.
    #[error("the transfer didn't end with TWRC_XFERDONE")]
    TransferNotDone,
    /// A connection or a source is still open from before, the DSM having
    /// refused to close it: nothing more is opened until it is.
    #[error("a connection or a source is still open, the DSM having refused to close it")]
    StillOpen,
}

fn by(label: &str, code: &Option<ConditionCode>) -> String {
    code.map(|c| format!("{label}{c})")).unwrap_or_default()
}

/// Why the DSM wasn't loaded. Its file is checked before any of its code runs:
/// held open against change, its machine, signature and signer read. So every
/// refusal comes before the DLL is loaded but `Loader` (the loader ran it and
/// refused), `NotTheFileChecked` and `MissingEntry` (it's loaded, and stays
/// so).
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
    /// The DSM is loaded in this process already by something else (another
    /// TWAIN client, another copy of this crate): using it too would race
    /// whatever uses it, as the DSM has no locks.
    #[error("the DSM is in use in this process already, loaded from {}", loaded.display())]
    InUse { loaded: PathBuf },
    /// The file has no signature that verifies: none in it, and none found for
    /// it in the system's catalogs (a check of the file may or may not find a
    /// catalog's: Windows' own files come back so, or as `WrongSigner`).
    #[error("the file has no signature")]
    Unsigned,
    /// The signature doesn't verify: `WinVerifyTrust`'s answer.
    #[error("the signature doesn't verify ({code:#010x})")]
    BadSignature { code: i32 },
    /// Signed and valid, but not by the TWAIN Working Group (a Windows file
    /// whose catalog signature is found is Microsoft's).
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
    /// Another `run` is going on in this process, on this thread or another,
    /// by this copy of the crate or another: the DSM has no locks, so one
    /// thread uses it at a time. Also every `run` after one whose teardown
    /// couldn't close the DSM's connection.
    #[error("TWAIN is already running in this process")]
    AlreadyRunning,
    /// The thread couldn't be made a single-threaded apartment: it's in a
    /// multithreaded one already, or COM refused (its `HRESULT`).
    #[error("the thread couldn't enter a single-threaded apartment ({code:#010x})")]
    Apartment { code: i32 },
    /// The hidden window couldn't be made: the error.
    #[error("the hidden window couldn't be made (error {code})")]
    Window { code: u32 },
    /// The process's run lock couldn't be made: the error.
    #[error("the run lock couldn't be made (error {code})")]
    Lock { code: u32 },
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
