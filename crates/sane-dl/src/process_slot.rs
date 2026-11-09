//! The process's one SANE across every copy of this crate it holds (Linux).
//!
//! Each copy of the crate counts its own loads, and a process can hold more
//! than one copy: each shared library built with the crate holds its own.
//! So beside a copy's own count, every copy takes one slot for the whole
//! process: a Unix datagram socket bound to a name in the abstract
//! namespace, formed from the process's pid namespace and its pid. Whichever
//! copy binds it first holds SANE; another's bind is refused
//! (`AddrInUse`), and its load is [`LoadError::AlreadyLoaded`]. The name
//! belongs to a socket, not a file: the kernel frees it when the socket
//! closes, and when the process ends, however it ends, so nothing is left
//! behind to clean up. std makes the socket close-on-exec, so a program the
//! process runs never holds it.
//!
//! The socket is bound and never read: nothing is sent on it, and what
//! another process sends it queues within the kernel's small bound for a
//! datagram socket.

#![forbid(unsafe_code)]

use std::fs;
use std::io;
use std::os::linux::net::SocketAddrExt;
use std::os::unix::fs::MetadataExt;
use std::os::unix::net::{SocketAddr, UnixDatagram};

use crate::LoadError;

/// The process's slot, held while bound: from before the library opens
/// until after `sane_exit`, and freed when dropped.
pub(crate) struct ProcessSlot {
    _socket: UnixDatagram,
}

impl ProcessSlot {
    /// Binds the process's name: [`LoadError::AlreadyLoaded`] if it's bound
    /// already, by this copy or another, and [`LoadError::SlotRefused`] if
    /// the system refuses the socket for another reason, so SANE is never
    /// loaded unguarded.
    pub(crate) fn take() -> Result<ProcessSlot, LoadError> {
        let name = name(pid_namespace(), std::process::id());
        let address =
            SocketAddr::from_abstract_name(name.as_bytes()).map_err(LoadError::SlotRefused)?;
        match UnixDatagram::bind_addr(&address) {
            Ok(socket) => Ok(ProcessSlot { _socket: socket }),
            Err(error) if error.kind() == io::ErrorKind::AddrInUse => Err(LoadError::AlreadyLoaded),
            Err(error) => Err(LoadError::SlotRefused(error)),
        }
    }
}

/// The process's pid namespace, as the device and inode of
/// `/proc/self/ns/pid`, which together name it; `None` where it can't be
/// read (no `/proc`).
fn pid_namespace() -> Option<(u64, u64)> {
    let namespace = fs::metadata("/proc/self/ns/pid").ok()?;
    Some((namespace.dev(), namespace.ino()))
}

/// The slot's name for the process with `pid` in the pid namespace given.
///
/// Fixed for good: every version of this crate forms the same name, so each
/// finds every other's slot. Never change it. The pid alone isn't enough,
/// since processes in different pid namespaces (containers) can share one
/// network namespace, where abstract names live, and one pid; where the
/// namespace can't be read, the pid alone is all there is.
fn name(namespace: Option<(u64, u64)>, pid: u32) -> String {
    match namespace {
        Some((device, inode)) => format!("sane-dl/slot/pidns/{device}/{inode}/pid/{pid}"),
        None => format!("sane-dl/slot/pidns/none/pid/{pid}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_name_is_the_one_every_version_forms() {
        assert_eq!(
            name(Some((4, 4_026_531_836)), 1234),
            "sane-dl/slot/pidns/4/4026531836/pid/1234"
        );
        assert_eq!(name(None, 1), "sane-dl/slot/pidns/none/pid/1");
    }

    #[test]
    fn the_longest_name_is_an_abstract_address() {
        let longest = name(Some((u64::MAX, u64::MAX)), u32::MAX);
        assert!(
            SocketAddr::from_abstract_name(longest.as_bytes()).is_ok(),
            "{longest} ({} bytes) is too long",
            longest.len()
        );
    }

    #[test]
    fn this_process_has_a_pid_namespace() {
        // Every Linux this crate's tests run on has `/proc`.
        assert!(pid_namespace().is_some());
    }
}
