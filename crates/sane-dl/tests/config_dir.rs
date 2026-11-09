//! Writing a configuration directory: what's written, and what's refused
//! before anything is.

use std::fs;

use sane_dl::{ConfigDir, ConfigError, Search};

/// A temporary directory under the tests' own.
fn temp_dir() -> std::io::Result<tempfile::TempDir> {
    tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR"))
}

#[test]
fn writes_dll_conf_and_an_empty_dll_d() {
    let dir = temp_dir().unwrap();
    let config = ConfigDir::write(dir.path(), ["test", "pnm"], Search::Isolated).unwrap();
    assert_eq!(
        fs::read_to_string(dir.path().join("dll.conf")).unwrap(),
        "test\npnm\n"
    );
    assert!(
        fs::read_dir(dir.path().join("dll.d"))
            .unwrap()
            .next()
            .is_none()
    );
    assert_eq!(config.path(), dir.path());
    assert_eq!(config.search(), Search::Isolated);
    assert_eq!(config.value(), dir.path().as_os_str());
}

#[test]
fn before_system_ends_in_a_colon() {
    let dir = temp_dir().unwrap();
    let config = ConfigDir::write(dir.path(), ["fujitsu"], Search::BeforeSystem).unwrap();
    let mut expected = dir.path().as_os_str().to_owned();
    expected.push(":");
    assert_eq!(config.value(), expected);
    assert_eq!(
        fs::read_to_string(dir.path().join("dll.conf")).unwrap(),
        "fujitsu\n"
    );
}

#[test]
fn writing_again_replaces_dll_conf_and_keeps_an_empty_dll_d() {
    let dir = temp_dir().unwrap();
    ConfigDir::write(dir.path(), ["test"], Search::Isolated).unwrap();
    ConfigDir::write(dir.path(), ["pnm"], Search::Isolated).unwrap();
    assert_eq!(
        fs::read_to_string(dir.path().join("dll.conf")).unwrap(),
        "pnm\n"
    );
}

#[test]
fn a_dll_d_with_snippets_is_refused() {
    let dir = temp_dir().unwrap();
    fs::create_dir(dir.path().join("dll.d")).unwrap();
    fs::write(dir.path().join("dll.d").join("airscan"), "airscan\n").unwrap();
    let error = ConfigDir::write(dir.path(), ["test"], Search::Isolated).unwrap_err();
    assert!(
        matches!(&error, ConfigError::Io { path, .. } if path.ends_with("dll.d")),
        "{error:?}"
    );
    assert!(!dir.path().join("dll.conf").exists());
    // A dll.conf already there is left as it was.
    fs::write(dir.path().join("dll.conf"), "pnm\n").unwrap();
    ConfigDir::write(dir.path(), ["test"], Search::Isolated).unwrap_err();
    assert_eq!(
        fs::read_to_string(dir.path().join("dll.conf")).unwrap(),
        "pnm\n"
    );
}

#[test]
fn a_dll_d_that_isnt_a_directory_is_refused_before_anything_is_written() {
    let dir = temp_dir().unwrap();
    fs::write(dir.path().join("dll.d"), "airscan\n").unwrap();
    let error = ConfigDir::write(dir.path(), ["test"], Search::Isolated).unwrap_err();
    assert!(
        matches!(&error, ConfigError::Io { path, .. } if path.ends_with("dll.d")),
        "{error:?}"
    );
    assert!(!dir.path().join("dll.conf").exists());
}

#[test]
fn bad_backend_names_are_refused_before_anything_is_written() {
    let dir = temp_dir().unwrap();
    for name in ["", "Test", "te st", "../x", "x\ny", "te-st", "tést"] {
        let error = ConfigDir::write(dir.path(), ["test", name], Search::Isolated).unwrap_err();
        assert!(
            matches!(&error, ConfigError::BadBackendName { name: given } if given == name),
            "{error:?}"
        );
    }
    assert!(!dir.path().join("dll.conf").exists());
    assert!(!dir.path().join("dll.d").exists());
}

#[test]
fn relative_paths_are_refused() {
    let error = ConfigDir::write("relative/dir", ["test"], Search::Isolated).unwrap_err();
    assert!(matches!(error, ConfigError::BadPath { .. }), "{error:?}");
}

#[cfg(unix)]
#[test]
fn paths_holding_a_colon_are_refused() {
    let dir = temp_dir().unwrap();
    let odd = dir.path().join("a:b");
    fs::create_dir(&odd).unwrap();
    let error = ConfigDir::write(&odd, ["test"], Search::Isolated).unwrap_err();
    assert!(matches!(error, ConfigError::BadPath { .. }), "{error:?}");
    assert!(!odd.join("dll.conf").exists());
}

#[test]
fn a_missing_directory_is_an_io_error() {
    let dir = temp_dir().unwrap();
    let error =
        ConfigDir::write(dir.path().join("missing"), ["test"], Search::Isolated).unwrap_err();
    assert!(
        matches!(&error, ConfigError::Io { path, .. } if path.ends_with("dll.conf")),
        "{error:?}"
    );
}
