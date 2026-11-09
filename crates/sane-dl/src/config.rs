//! A configuration directory that loads only the backends named, so a known
//! device opens without waiting on every backend's probe.

use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// The environment variable SANE reads its configuration directories from,
/// once, in `sane_init`.
pub const CONFIG_DIR_VARIABLE: &str = "SANE_CONFIG_DIR";

/// The text before a device name's first `:`, if it's a backend's name: with
/// SANE's own library every device name starts with its backend's
/// (`test:0`, `net:host:test:0`), which is what a [`ConfigDir`] names.
///
/// `None` if the name has no `:`, or what comes before it isn't a backend's
/// name (ASCII lower-case letters, digits and `_`, at least one).
pub fn backend_of(device_name: &[u8]) -> Option<&str> {
    let end = device_name.iter().position(|&byte| byte == b':')?;
    let backend = std::str::from_utf8(&device_name[..end]).ok()?;
    is_backend_name(backend).then_some(backend)
}

fn is_backend_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
}

/// Where SANE looks for configuration besides a [`ConfigDir`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Search {
    /// Only the directory: SANE reads no other configuration, so a backend
    /// runs on its built-in defaults. What tests want.
    Isolated,
    /// The directory, then SANE's own list: the process's current directory,
    /// then the system's (`/etc/sane.d`), so a backend still finds its own
    /// `<backend>.conf` there, while the directory's `dll.conf` and empty
    /// `dll.d` decide which backends load. Since SANE then also reads the
    /// current directory, run the process that loads it from a directory it
    /// trusts.
    BeforeSystem,
}

/// A directory of SANE configuration naming the backends to load, and the
/// value of `SANE_CONFIG_DIR` that points SANE at it.
///
/// SANE's own library loads every backend its `dll.conf` and `dll.d`
/// snippets name and asks each for its devices, which can take seconds with
/// nothing attached. A directory naming only the backend of a device already
/// known (from [`backend_of`]) opens it at once. [`write`](Self::write)
/// writes a `dll.conf` with those backends and an empty `dll.d`, into a
/// directory the caller chooses: one private to the process, since whoever
/// can write it chooses which libraries SANE loads.
///
/// This crate never sets the variable: SANE reads it once, in `sane_init`,
/// and setting an environment variable in a running process isn't
/// thread-safe. The caller gives [`value`](Self::value) to the process that
/// loads SANE, as it spawns it (`Command::env(CONFIG_DIR_VARIABLE, …)`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConfigDir {
    dir: PathBuf,
    search: Search,
}

impl ConfigDir {
    /// Writes the configuration into `dir`, which must exist and be an
    /// absolute path: a `dll.conf` naming `backends`, one a line, replacing
    /// any there, and an empty `dll.d` (an existing one must be empty).
    ///
    /// Everything is checked before anything is written: each backend's
    /// name (ASCII lower-case letters, digits and `_`), the path (on Unix it
    /// mustn't hold `:`, which separates the directories in SANE's
    /// variable), and a `dll.d` already there, which must be empty. Nothing
    /// is written if any check fails.
    pub fn write<I, S>(
        dir: impl Into<PathBuf>,
        backends: I,
        search: Search,
    ) -> Result<ConfigDir, ConfigError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let dir = dir.into();
        if !dir.is_absolute() {
            return Err(ConfigError::BadPath {
                path: dir,
                reason: "it isn't absolute",
            });
        }
        if cfg!(unix) && dir.as_os_str().as_encoded_bytes().contains(&b':') {
            return Err(ConfigError::BadPath {
                path: dir,
                reason: "it holds `:`, which separates SANE's directories",
            });
        }
        let mut conf = String::new();
        for backend in backends {
            let backend = backend.as_ref();
            if !is_backend_name(backend) {
                return Err(ConfigError::BadBackendName {
                    name: backend.to_owned(),
                });
            }
            conf.push_str(backend);
            conf.push('\n');
        }
        let snippets = dir.join("dll.d");
        let snippets_there = check_dll_d(&dir, &snippets)?;
        let conf_path = dir.join("dll.conf");
        fs::write(&conf_path, conf).map_err(|source| ConfigError::Io {
            path: conf_path,
            source,
        })?;
        if !snippets_there {
            match fs::create_dir(&snippets) {
                Ok(()) => {}
                // Made meanwhile by someone else: still refused unless empty.
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                    check_dll_d(&dir, &snippets)?;
                }
                Err(source) => {
                    return Err(ConfigError::Io {
                        path: snippets,
                        source,
                    });
                }
            }
        }
        Ok(ConfigDir { dir, search })
    }

    /// The directory.
    pub fn path(&self) -> &Path {
        &self.dir
    }

    /// Where SANE looks besides the directory.
    pub fn search(&self) -> Search {
        self.search
    }

    /// The value for [`CONFIG_DIR_VARIABLE`]: the directory alone for
    /// [`Search::Isolated`], and with a trailing `:` for
    /// [`Search::BeforeSystem`], which SANE reads as "then my own list".
    pub fn value(&self) -> OsString {
        let mut value = self.dir.clone().into_os_string();
        if self.search == Search::BeforeSystem {
            value.push(":");
        }
        value
    }
}

/// Whether `dir`'s `dll.d` (`snippets`) is there: refused if it isn't an
/// empty directory ([`ConfigError::BadPath`], for `dir`), or can't be read
/// ([`ConfigError::Io`]).
fn check_dll_d(dir: &Path, snippets: &Path) -> Result<bool, ConfigError> {
    let io_error = |source| ConfigError::Io {
        path: snippets.to_owned(),
        source,
    };
    let refused = |reason| ConfigError::BadPath {
        path: dir.to_owned(),
        reason,
    };
    // Looked at before it's listed, so a file in its place is refused alike
    // on every system, whatever listing a file answers there.
    match fs::metadata(snippets) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(source) => return Err(io_error(source)),
        Ok(metadata) if !metadata.is_dir() => {
            return Err(refused("its dll.d is there already, and isn't a directory"));
        }
        Ok(_) => {}
    }
    match fs::read_dir(snippets).map_err(io_error)?.next() {
        None => Ok(true),
        Some(_) => Err(refused("its dll.d is there already, and isn't empty")),
    }
}

/// Why a [`ConfigDir`] couldn't be written.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ConfigError {
    /// A backend's name isn't one: ASCII lower-case letters, digits and `_`,
    /// at least one.
    #[error("{name:?} isn't a backend's name")]
    BadBackendName {
        /// The name given.
        name: String,
    },
    /// The directory can't be given to SANE: its path, or a `dll.d` in it
    /// already that isn't an empty directory. Nothing was written.
    #[error("{} can't be SANE's configuration directory: {reason}", path.display())]
    BadPath {
        /// The path given.
        path: PathBuf,
        /// Why not.
        reason: &'static str,
    },
    /// The system refused to write a file, or to read `dll.d`.
    #[error("couldn't write {}", path.display())]
    Io {
        /// The file or directory.
        path: PathBuf,
        /// What the system said.
        #[source]
        source: io::Error,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    // Writing a directory is tested in `tests/config_dir.rs`, whose files go
    // under the tests' own temporary directory.

    #[test]
    fn backends_come_before_the_first_colon() {
        assert_eq!(backend_of(b"test:0"), Some("test"));
        assert_eq!(backend_of(b"net:host:test:0"), Some("net"));
        assert_eq!(backend_of(b"canon_dr:libusb:001:002"), Some("canon_dr"));
        assert_eq!(backend_of(b"epson2:net:10.0.0.2"), Some("epson2"));
        assert_eq!(backend_of(b"test"), None);
        assert_eq!(backend_of(b":0"), None);
        assert_eq!(backend_of(b"Test:0"), None);
        assert_eq!(backend_of(b"te-st:0"), None);
        assert_eq!(backend_of(b"\xff:0"), None);
        assert_eq!(backend_of(b""), None);
    }
}
