//! Where a worker is started: a private directory of its own under the work directory, and for
//! a SANE open, the stored id's one-backend configuration in it. Through the test worker's `env`
//! backend, whose devices say where it was started: its current directory as their name,
//! `SANE_CONFIG_DIR` as their maker, what the directory holds as their model.

mod support;

use std::{env, fs, path::PathBuf};

use scan_worker::scan_device::{DeviceId, DeviceInfo, Driver};
use scan_worker::{Client, RemoteDevice};
use serde_json::{Value, json};

use support::{AT_MOST, resolved, worker};

/// A client whose SANE worker is the `env` one, its directories made under a new work directory.
fn env_client() -> (Client, tempfile::TempDir) {
    let work = tempfile::tempdir().expect("a work directory");
    let client = Client::new()
        .worker(Driver::Sane, worker("env"))
        .work_dir(work.path());
    (client, work)
}

/// Where a device says its worker was started: the directory, `SANE_CONFIG_DIR`, and what the
/// directory held.
struct Started {
    dir: PathBuf,
    variable: String,
    held: Value,
}

fn started(info: &DeviceInfo) -> Started {
    assert_eq!(info.driver, Driver::Sane);
    Started {
        dir: PathBuf::from(&info.name),
        variable: info.maker.clone(),
        held: serde_json::from_str(&info.model).expect("the directory's contents"),
    }
}

/// The device `id` opened through `client`, and where its worker was started, which is checked
/// to be a directory of its own under `work`, and on Unix its owner's alone.
fn open_sane(client: &Client, work: &tempfile::TempDir, id: &str) -> (RemoteDevice, Started) {
    let stored = DeviceInfo::new(
        DeviceId(id.to_owned()),
        "Desk".to_owned(),
        "Maker".to_owned(),
        "Model".to_owned(),
        Driver::Sane,
    );
    let device = client.open(&stored, AT_MOST).expect("the device opened");
    assert_eq!(device.info().id, stored.id);
    let started = started(device.info());
    a_directory_of_its_own(&started.dir, work);
    (device, started)
}

/// `dir` a directory made for the worker alone, under `work`.
fn a_directory_of_its_own(dir: &PathBuf, work: &tempfile::TempDir) {
    let dir = resolved(dir);
    assert_eq!(
        dir.parent(),
        Some(resolved(work.path()).as_path()),
        "{}",
        dir.display()
    );
    let name = dir.file_name().unwrap().to_string_lossy().into_owned();
    assert!(name.starts_with("scan-worker-"), "{name}");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(&dir).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o700, "{mode:o}");
    }
}

/// `SANE_CONFIG_DIR` as a SANE open's worker has it: its directory, then `:`, so SANE reads
/// its own list after it.
fn names_its_directory_first(started: &Started) {
    let directory = started
        .variable
        .strip_suffix(':')
        .unwrap_or_else(|| panic!("{:?} ends in `:`", started.variable));
    assert_eq!(resolved(directory), resolved(&started.dir));
}

#[test]
fn a_sane_open_s_directory_names_the_stored_id_s_backend_alone() {
    let (client, work) = env_client();
    let (device, started) = open_sane(&client, &work, "sane:test:0");
    names_its_directory_first(&started);
    assert_eq!(
        started.held,
        json!({"entries": ["dll.conf", "dll.d"], "dll_conf": "test\n", "dll_d": []})
    );
    let dir = resolved(&started.dir);
    device.close().expect("closed");
    assert!(!dir.exists(), "{} is left", dir.display());
    assert_eq!(fs::read_dir(work.path()).unwrap().count(), 0);
}

#[test]
fn a_sane_open_names_no_backend_where_the_id_gives_none() {
    let (client, work) = env_client();
    // No `:` after the name; and a character past U+00FF, which SANE's names can't hold,
    // after a backend's.
    for id in ["sane:Desk scanner", "sane:test:\u{2603}"] {
        let (device, started) = open_sane(&client, &work, id);
        names_its_directory_first(&started);
        assert_eq!(
            started.held,
            json!({"entries": ["dll.conf", "dll.d"], "dll_conf": "", "dll_d": []}),
            "{id}"
        );
        let dir = resolved(&started.dir);
        drop(device);
        assert!(!dir.exists(), "{id}: {} is left", dir.display());
    }
}

#[test]
fn a_sane_listing_s_directory_is_empty_and_its_variable_the_app_s() {
    let (client, work) = env_client();
    let listed = client.list(Driver::Sane, AT_MOST).expect("a listing");
    assert_eq!(listed.len(), 1);
    let started = started(&listed[0]);
    let app_s = env::var("SANE_CONFIG_DIR").unwrap_or_else(|_| "<unset>".to_owned());
    assert_eq!(started.variable, app_s);
    assert_eq!(
        started.held,
        json!({"entries": [], "dll_conf": null, "dll_d": null})
    );
    a_directory_of_its_own_once(&started.dir, &work);
    // Removed once the listing's worker has exited.
    assert_eq!(fs::read_dir(work.path()).unwrap().count(), 0);
}

/// `dir` was under `work`: it's gone, so only its parent can be resolved.
fn a_directory_of_its_own_once(dir: &std::path::Path, work: &tempfile::TempDir) {
    let parent = dir.parent().expect("a parent");
    assert_eq!(resolved(parent), resolved(work.path()), "{}", dir.display());
    assert!(!dir.exists(), "{} is left", dir.display());
}

#[test]
fn the_system_s_temporary_directory_is_the_default() {
    let client = Client::new().worker(Driver::Sane, worker("env"));
    let listed = client.list(Driver::Sane, AT_MOST).expect("a listing");
    let dir = started(&listed[0]).dir;
    assert_eq!(
        resolved(dir.parent().expect("a parent")),
        resolved(env::temp_dir())
    );
}
