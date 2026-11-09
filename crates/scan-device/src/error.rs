//! What can go wrong with a device.

/// Why a call on a [`Backend`](crate::Backend) or a [`Device`](crate::Device) failed.
///
/// A batch that fails keeps the pages it sent before the error.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum ScanError {
    /// The feeder was empty as the batch began, before any page; after pages, a batch ends as
    /// [`Done::FeederEmpty`](crate::Done::FeederEmpty) instead.
    #[error("the feeder is empty")]
    FeederEmpty,
    /// The paper jammed.
    #[error("the paper jammed")]
    Jammed,
    /// The scanner took two sheets at once.
    #[error("two sheets fed at once")]
    DoubleFeed,
    /// The scanner's cover is open.
    #[error("the cover is open")]
    CoverOpen,
    /// The scanner is busy, with another program or another batch.
    #[error("the scanner is busy")]
    Busy,
    /// The scanner isn't there: unplugged, switched off, or not found under its id.
    #[error("the scanner isn't there")]
    Gone,
    /// The device id isn't one this backend's driver forms. It says nothing of the id.
    #[error("the device id isn't one this driver gives")]
    BadId,
    /// The scanner wouldn't start with the settings, once they were set.
    #[error("the scanner wouldn't start with these settings")]
    SettingsRefused,
    /// The system refused access to the scanner.
    #[error("the scanner refused access")]
    AccessDenied,
    /// The driver didn't answer before the deadline.
    #[error("the driver didn't answer in time")]
    TimedOut,
    /// The scanner or its driver can't do what was asked.
    #[error("the scanner can't do that")]
    Unsupported,
    /// The driver API isn't installed on this system.
    #[error("{what} isn't installed")]
    NotInstalled {
        /// What isn't installed, as a user would know it.
        what: String,
    },
    /// The driver's own words, for what has no other variant.
    #[error("{message}")]
    Driver {
        /// What the driver said.
        message: String,
        /// Whether the driver has already shown it to the user, so the app needn't again.
        shown: bool,
    },
}
