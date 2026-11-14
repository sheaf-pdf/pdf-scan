//! A worker's process started: its private directory, its command's variables, a SANE open's
//! configuration in the directory, and the process itself, apart from the app's terminal or
//! console (and on Windows inside a kill-on-close Job Object).

use std::{
    env,
    ffi::{OsStr, OsString},
    fs, io,
    path::Path,
    process::{Command, Stdio},
};

use scan_device::{DeviceId, Driver};
use tempfile::TempDir;

use super::WorkerCommand;
use super::worker::LiveProcess;

/// The environment variable SANE reads its configuration directories from, once, in `sane_init`.
const CONFIG_DIR_VARIABLE: &str = "SANE_CONFIG_DIR";

/// Why a worker wasn't started.
#[derive(Debug)]
pub(super) enum NotStarted {
    /// Its program: not an absolute path, or the system couldn't start it.
    Program(io::Error),
    /// The work directory: not an absolute path, or the worker's directory, or its SANE
    /// configuration, couldn't be made there.
    WorkDir(io::Error),
}

/// `command` started for `driver`, to list or to open `opens`, in a private directory made under
/// `work_dir`, its three pipes the client's; and the directory, to remove once it has exited.
pub(super) fn spawn(
    command: &WorkerCommand,
    work_dir: &Path,
    driver: Driver,
    opens: Option<&DeviceId>,
) -> Result<(LiveProcess, TempDir), NotStarted> {
    // A bare name would be searched for, on Windows from the current directory first.
    if !command.program.is_absolute() {
        return Err(NotStarted::Program(invalid(
            "the worker's program isn't an absolute path",
        )));
    }
    // A relative one would be read from wherever the app then stands.
    if !work_dir.is_absolute() {
        return Err(NotStarted::WorkDir(invalid(
            "the work directory isn't an absolute path",
        )));
    }
    let dir = private_dir(work_dir).map_err(NotStarted::WorkDir)?;
    let mut process = Command::new(&command.program);
    if driver == Driver::Sane {
        // SANE backends format decimal options and parse configuration under the C locale; set
        // before `command.env` so `WorkerCommand::env("LC_ALL", ...)` still overrides it.
        process.env("LC_ALL", "C");
    }
    process
        .args(&command.args)
        // After the app's own environment (and SANE's default `LC_ALL=C`), in order, so the last
        // given of a variable is its value.
        .envs(command.env.iter().map(|(key, value)| (key, value)))
        .current_dir(dir.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if driver == Driver::Sane
        && let Some(id) = opens
    {
        // SANE reads this directory first, so its `dll.conf` and empty `dll.d` load the id's
        // backend alone; then what a listing's worker reads, where each backend finds its own
        // configuration. Set after the command's variables, so it takes the place of one the
        // command gives.
        let name = sane_name(id);
        let backend = name.as_deref().and_then(backend_of);
        write_sane_config(dir.path(), backend).map_err(NotStarted::WorkDir)?;
        let listing = listing_config(command, env::var_os(CONFIG_DIR_VARIABLE));
        process.env(
            CONFIG_DIR_VARIABLE,
            open_config(dir.path(), listing.as_deref()),
        );
    }
    #[cfg(windows)]
    let job = crate::sys::Job::new().map_err(NotStarted::Program)?;
    set_apart(&mut process);
    #[cfg(windows)]
    let mut child = process.spawn().map_err(NotStarted::Program)?;
    #[cfg(not(windows))]
    let child = process.spawn().map_err(NotStarted::Program)?;
    #[cfg(windows)]
    if let Err(error) = job.assign(&mut child) {
        let _ = child.kill();
        let _ = child.wait();
        return Err(NotStarted::Program(error));
    }
    Ok((
        LiveProcess {
            child,
            #[cfg(windows)]
            job,
        },
        dir,
    ))
}

/// Writes `dll.conf` (naming `backend`, or empty when `None`) and an empty `dll.d` into `dir`.
fn write_sane_config(dir: &Path, backend: Option<&str>) -> io::Result<()> {
    if !dir.is_absolute() {
        return Err(invalid("the worker's directory isn't an absolute path"));
    }
    if cfg!(unix) && dir.as_os_str().as_encoded_bytes().contains(&b':') {
        return Err(invalid(
            "the worker's directory holds `:`, which separates SANE's directories",
        ));
    }
    let mut conf = String::new();
    if let Some(backend) = backend {
        if !is_backend_name(backend) {
            return Err(invalid("invalid SANE backend name"));
        }
        conf.push_str(backend);
        conf.push('\n');
    }
    let snippets = dir.join("dll.d");
    let snippets_there = check_dll_d(&snippets)?;
    fs::write(dir.join("dll.conf"), conf)?;
    if !snippets_there {
        match fs::create_dir(&snippets) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                check_dll_d(&snippets)?;
            }
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

/// Ensures `snippets` (`dll.d`) is either absent (`Ok(false)`) or an empty directory (`Ok(true)`).
fn check_dll_d(snippets: &Path) -> io::Result<bool> {
    match fs::metadata(snippets) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error),
        Ok(metadata) if !metadata.is_dir() => {
            return Err(invalid("dll.d is there already, and isn't a directory"));
        }
        Ok(_) => {}
    }
    match fs::read_dir(snippets)?.next() {
        None => Ok(true),
        Some(_) => Err(invalid("dll.d is there already, and isn't empty")),
    }
}

/// The text before a SANE device name's first `:`, if it's a valid backend name (ASCII lowercase
/// letters, digits, and `_`, at least one).
fn backend_of(device_name: &[u8]) -> Option<&str> {
    let end = device_name.iter().position(|&byte| byte == b':')?;
    let backend = core::str::from_utf8(&device_name[..end]).ok()?;
    is_backend_name(backend).then_some(backend)
}

fn is_backend_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
}

/// The `SANE_CONFIG_DIR` a listing's worker reads: the last the command gives, else the one the
/// app has (`inherited`), which the worker inherits; `None` where neither gives one.
fn listing_config(command: &WorkerCommand, inherited: Option<OsString>) -> Option<OsString> {
    command
        .env
        .iter()
        .rev()
        .find(|(key, _)| key == CONFIG_DIR_VARIABLE)
        .map(|(_, value)| value.clone())
        .or(inherited)
}

/// A SANE open's `SANE_CONFIG_DIR`: `dir`, then the directories `listing` names, as a listing's
/// worker reads them (an empty value naming none); where there's no `listing`, SANE's own list
/// (its current directory, the worker's own, then the system's).
fn open_config(dir: &Path, listing: Option<&OsStr>) -> OsString {
    match listing {
        // The directory then `:`, which SANE reads as "then my own list".
        None => {
            let mut value = dir.as_os_str().to_owned();
            value.push(":");
            value
        }
        Some(listing) if listing.is_empty() => dir.as_os_str().to_owned(),
        Some(listing) => {
            let mut value = dir.as_os_str().to_owned();
            value.push(":");
            value.push(listing);
            value
        }
    }
}

fn invalid(reason: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, reason)
}

/// A directory of the worker's own under `work_dir`, made at once under a random name, and on
/// Unix made readable by its owner alone: whoever could write it would choose what SANE loads.
fn private_dir(work_dir: &Path) -> io::Result<TempDir> {
    let mut builder = tempfile::Builder::new();
    builder.prefix("scan-worker-");
    #[cfg(unix)]
    {
        use std::{fs::Permissions, os::unix::fs::PermissionsExt};
        // Made with this mode, so it's never more open than it, whatever the umask.
        builder.permissions(Permissions::from_mode(0o700));
    }
    builder.tempdir_in(work_dir)
}

/// The SANE device name a `sane:` id holds, as SANE's bytes: the id's text after `sane:`, each
/// character up to U+00FF the byte of its number. `None` where the id doesn't start `sane:` or
/// holds any other character, so it names no backend.
fn sane_name(id: &DeviceId) -> Option<Vec<u8>> {
    id.0.strip_prefix("sane:")?
        .chars()
        .map(|character| u8::try_from(character).ok())
        .collect()
}

/// On Unix, the worker leads a process group of its own, so a terminal's interrupt reaches the
/// app, which decides, and not the worker mid-sheet.
#[cfg(unix)]
fn set_apart(process: &mut Command) {
    use std::os::unix::process::CommandExt;
    process.process_group(0);
}

/// On Windows, the worker is started with no console window, so a console worker under an app
/// with windows shows none.
#[cfg(windows)]
fn set_apart(process: &mut Command) {
    use std::os::windows::process::CommandExt;
    /// `CREATE_NO_WINDOW`, a process creation flag.
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    process.creation_flags(CREATE_NO_WINDOW);
}

/// Elsewhere, nothing to set apart.
#[cfg(not(any(unix, windows)))]
fn set_apart(_process: &mut Command) {}

#[cfg(test)]
mod tests {
    use super::*;

    fn name(id: &str) -> Option<Vec<u8>> {
        sane_name(&DeviceId(id.to_owned()))
    }

    #[test]
    fn a_sane_id_s_name_is_its_text_after_sane_each_character_a_byte() {
        assert_eq!(name("sane:test:0"), Some(b"test:0".to_vec()));
        assert_eq!(name("sane:Desk scanner"), Some(b"Desk scanner".to_vec()));
        assert_eq!(name("sane:"), Some(Vec::new()));
        // Up to U+00FF, the byte of its number: Latin-1.
        assert_eq!(
            name("sane:t\u{e9}st:\u{ff}"),
            Some(b"t\xe9st:\xff".to_vec())
        );
        // Any other character, anywhere, and the id names nothing.
        assert_eq!(name("sane:test:\u{100}"), None);
        assert_eq!(name("sane:test:\u{2603}"), None);
        assert_eq!(name("fake:test:0"), None);
        assert_eq!(name("SANE:test:0"), None);
        assert_eq!(name("test:0"), None);
    }

    #[test]
    fn an_open_s_configuration_is_its_directory_then_what_a_listing_reads() {
        let given = WorkerCommand::new("/bin/worker")
            .env(CONFIG_DIR_VARIABLE, "/first")
            .env("OTHER", "x")
            .env(CONFIG_DIR_VARIABLE, "/etc/app/sane:");
        let none = WorkerCommand::new("/bin/worker").env("OTHER", "x");
        let app = Some(OsString::from("/app"));
        // The command's last, else the app's.
        assert_eq!(
            listing_config(&given, app.clone()).as_deref(),
            Some(OsStr::new("/etc/app/sane:"))
        );
        assert_eq!(
            listing_config(&none, app).as_deref(),
            Some(OsStr::new("/app"))
        );
        assert_eq!(listing_config(&none, None), None);
        let dir = tempfile::tempdir().unwrap();
        write_sane_config(dir.path(), Some("test")).unwrap();
        assert_eq!(
            fs::read_to_string(dir.path().join("dll.conf")).unwrap(),
            "test\n"
        );
        assert!(dir.path().join("dll.d").is_dir());
        let path = dir.path().as_os_str().to_owned();
        let with = |after: &str| {
            let mut value = path.clone();
            value.push(after);
            value
        };
        assert_eq!(
            open_config(dir.path(), Some(OsStr::new("/etc/app/sane:"))),
            with(":/etc/app/sane:")
        );
        assert_eq!(
            open_config(dir.path(), Some(OsStr::new("/a:/b"))),
            with(":/a:/b")
        );
        // None given: SANE's own list after it.
        assert_eq!(open_config(dir.path(), None), with(":"));
        // An empty value, which names no directory: the directory alone.
        assert_eq!(open_config(dir.path(), Some(OsStr::new(""))), path);
    }

    #[test]
    fn only_a_sane_id_s_own_backend_is_named() {
        let backend = |id: &str| name(id).as_deref().and_then(backend_of).map(str::to_owned);
        assert_eq!(backend("sane:test:0").as_deref(), Some("test"));
        assert_eq!(backend("sane:net:host:test:0").as_deref(), Some("net"));
        assert_eq!(
            backend("sane:canon_dr:libusb:001:002").as_deref(),
            Some("canon_dr")
        );
        assert_eq!(
            backend("sane:epson2:net:10.0.0.2").as_deref(),
            Some("epson2")
        );
        assert_eq!(backend("sane:Desk scanner"), None);
        assert_eq!(backend("sane::0"), None);
        assert_eq!(backend("sane:Test:0"), None);
        assert_eq!(backend("sane:te-st:0"), None);
        assert_eq!(backend("sane:test:\u{2603}"), None);
        assert_eq!(backend("sane:t\u{e9}st:0"), None);
        assert_eq!(backend("fake:test:0"), None);
    }
}
