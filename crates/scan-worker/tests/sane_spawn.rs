//! Where a worker is started: a private directory of its own under the work directory, and for
//! a SANE open, the stored id's one-backend configuration in it; and the variables its command
//! sets, a SANE open's `SANE_CONFIG_DIR` after them. Through the test worker's `env` backend,
//! whose devices say where it was started: its current directory as their name,
//! `SANE_CONFIG_DIR` as their maker, what the directory holds and a variable a test sets as their
//! model.

mod support;

use std::{env, fs, path::PathBuf};

use scan_worker::scan_device::{DeviceId, DeviceInfo, Driver};
use scan_worker::{Client, RemoteDevice, WorkerCommand};
use serde_json::{Value, json};

use support::{AT_MOST, resolved, worker};

/// A client whose SANE worker is the `env` one, its directories made under a new work directory.
fn env_client() -> (Client, tempfile::TempDir) {
    env_client_of(worker("env"))
}

/// A client whose SANE worker is `command`, the `env` one, its directories made under a new work
/// directory.
fn env_client_of(command: WorkerCommand) -> (Client, tempfile::TempDir) {
    let work = tempfile::tempdir().expect("a work directory");
    let client = Client::new()
        .worker(Driver::Sane, command)
        .work_dir(work.path());
    (client, work)
}

/// The variable the `env` backend reports.
const GIVEN: &str = "SCAN_WORKER_TEST_GIVEN";

/// Where a device says its worker was started: the directory, `SANE_CONFIG_DIR`, what the
/// directory held, and [`GIVEN`]'s value.
struct Started {
    dir: PathBuf,
    variable: String,
    held: Value,
    given: Value,
}

fn started(info: &DeviceInfo) -> Started {
    assert_eq!(info.driver, Driver::Sane);
    let mut held: Value = serde_json::from_str(&info.model).expect("the directory's contents");
    let given = held
        .as_object_mut()
        .and_then(|held| held.remove("given"))
        .expect("the variable's value, or null");
    Started {
        dir: PathBuf::from(&info.name),
        variable: info.maker.clone(),
        held,
        given,
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

/// `SANE_CONFIG_DIR` as a SANE open's worker has it: its directory, then what a listing's worker
/// reads, `listing` (the command's, else the app's): `:` and its directories, or the directory
/// alone where it names none; where there's none, `:`, so SANE reads its own list after it.
fn names_its_directory_then(started: &Started, listing: Option<&str>) {
    let after = match listing {
        None => ":".to_owned(),
        Some("") => String::new(),
        Some(listing) => format!(":{listing}"),
    };
    let directory = started
        .variable
        .strip_suffix(&after)
        .unwrap_or_else(|| panic!("{:?} ends in {after:?}", started.variable));
    assert_eq!(resolved(directory), resolved(&started.dir));
}

/// The app's own `SANE_CONFIG_DIR`, which an open's worker reads after its directory where the
/// command gives none.
fn the_app_s() -> Option<String> {
    env::var("SANE_CONFIG_DIR").ok()
}

#[test]
fn a_sane_open_s_directory_names_the_stored_id_s_backend_alone() {
    let (client, work) = env_client();
    let (device, started) = open_sane(&client, &work, "sane:test:0");
    names_its_directory_then(&started, the_app_s().as_deref());
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
        names_its_directory_then(&started, the_app_s().as_deref());
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

#[test]
fn a_variable_the_command_sets_reaches_the_worker_the_last_given_its_value() {
    let (client, work) = env_client();
    // None given.
    let listed = client.list(Driver::Sane, AT_MOST).expect("a listing");
    assert_eq!(started(&listed[0]).given, Value::Null);
    let (device, opened) = open_sane(&client, &work, "sane:test:0");
    assert_eq!(opened.given, Value::Null);
    device.close().expect("closed");
    // Given, and given twice: the later is its value, for a listing and an open alike.
    let command = worker("env")
        .env(GIVEN, "first")
        .env(GIVEN, "a value, with spaces")
        .env("SCAN_WORKER_TEST_OTHER", "unread");
    assert_eq!(
        command.envs().map(|(key, _)| key).collect::<Vec<_>>(),
        [GIVEN, GIVEN, "SCAN_WORKER_TEST_OTHER"]
    );
    let (client, work) = env_client_of(command);
    let listed = client.list(Driver::Sane, AT_MOST).expect("a listing");
    assert_eq!(started(&listed[0]).given, json!("a value, with spaces"));
    let (device, opened) = open_sane(&client, &work, "sane:test:0");
    assert_eq!(opened.given, json!("a value, with spaces"));
    device.close().expect("closed");
}

#[test]
fn a_command_s_sane_config_dir_is_a_listing_s_and_an_open_s_after_the_client_s() {
    let given = env::temp_dir().join("given-by-the-command");
    let command = worker("env")
        .env("SANE_CONFIG_DIR", "/given-first")
        .env("SANE_CONFIG_DIR", &given);
    let (client, work) = env_client_of(command);
    // A listing's worker keeps the command's, its directory empty.
    let listed = client.list(Driver::Sane, AT_MOST).expect("a listing");
    let listing = started(&listed[0]);
    assert_eq!(PathBuf::from(&listing.variable), given);
    assert_eq!(
        listing.held,
        json!({"entries": [], "dll_conf": null, "dll_d": null})
    );
    // An open's has the client's directory, naming the id's backend, then the command's.
    let (device, opened) = open_sane(&client, &work, "sane:test:0");
    names_its_directory_then(&opened, Some(given.to_str().expect("a path in UTF-8")));
    assert_eq!(
        opened.held,
        json!({"entries": ["dll.conf", "dll.d"], "dll_conf": "test\n", "dll_d": []})
    );
    device.close().expect("closed");
}
