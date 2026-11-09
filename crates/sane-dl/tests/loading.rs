//! Loading SANE: what's tried, what's refused, and that a failed load leaves
//! nothing behind. Each test runs in a child process of its own.

#[macro_use]
mod common;

use std::path::Path;

use common::{Child, Fake};
use sane_dl::{LoadError, Version};

#[test]
fn a_missing_library_is_not_found_with_what_was_tried() {
    let missing = Path::new(env!("CARGO_TARGET_TMPDIR")).join("no-such-libsane");
    in_child!(
        a_missing_library_is_not_found_with_what_was_tried,
        Child::new(),
        || {
            match common::load_from(&missing) {
                Err(LoadError::NotFound { tried, .. }) => assert_eq!(tried, vec![missing.clone()]),
                other => panic!("expected NotFound, got {other:?}"),
            }
            // Nothing was left behind: a later load gets as far as the loader.
            assert!(matches!(
                common::load_from(&missing),
                Err(LoadError::NotFound { .. })
            ));
        }
    );
}

#[cfg(windows)]
#[test]
fn there_is_no_default_library_on_windows() {
    in_child!(there_is_no_default_library_on_windows, Child::new(), || {
        assert!(matches!(sane_dl::Sane::load(), Err(LoadError::NoDefault)));
    });
}

/// The system's C library, by its name on each system: a library that
/// loads, and has none of SANE's functions.
fn c_library() -> Option<&'static str> {
    if cfg!(target_os = "linux") {
        Some("libc.so.6")
    } else if cfg!(target_os = "macos") {
        Some("/usr/lib/libSystem.B.dylib")
    } else if cfg!(windows) {
        Some("msvcrt.dll")
    } else {
        None
    }
}

#[test]
fn a_library_without_sanes_functions_is_refused() {
    let Some(library) = c_library() else {
        common::report_skip(
            "a_library_without_sanes_functions_is_refused",
            "no C library is known here",
            None,
        );
        return;
    };
    in_child!(
        a_library_without_sanes_functions_is_refused,
        Child::new(),
        || {
            match common::load_from(library) {
                Err(LoadError::MissingSymbol { name, .. }) => assert_eq!(name, "sane_init"),
                other => panic!("expected MissingSymbol, got {other:?}"),
            }
            assert!(
                matches!(
                    common::load_from(library),
                    Err(LoadError::MissingSymbol { .. })
                ),
                "the slot was freed"
            );
        }
    );
}

#[test]
fn one_sane_per_process_until_it_and_its_handles_are_gone() {
    let fake = Fake::new(
        "one_sane_per_process_until_it_and_its_handles_are_gone",
        "device fake:0 Fake One scanner\n",
    );
    in_child!(
        one_sane_per_process_until_it_and_its_handles_are_gone,
        fake.child(),
        || {
            let sane = common::load_fake();
            assert!(matches!(
                common::load_from(common::fake_path()),
                Err(LoadError::AlreadyLoaded)
            ));
            let clone = sane.clone();
            let handle = sane.open("fake:0").unwrap();
            drop(sane);
            drop(clone);
            // The handle keeps SANE started.
            assert!(matches!(
                common::load_from(common::fake_path()),
                Err(LoadError::AlreadyLoaded)
            ));
            drop(handle);
            let again = common::load_fake();
            assert_eq!(
                again.version(),
                Version {
                    major: 1,
                    minor: 0,
                    build: 0
                }
            );
        }
    );
    let log = fake.log();
    log.assert_no_violations();
    assert_eq!(
        log.calls_but_options(),
        [
            "sane_init",
            "sane_open",
            "sane_close",
            "sane_exit",
            "sane_init",
            "sane_exit"
        ]
    );
}

#[test]
fn a_refused_version_is_exited_and_a_later_load_succeeds() {
    let fake = Fake::new(
        "a_refused_version_is_exited_and_a_later_load_succeeds",
        "init GOOD 2.0.0\ninit GOOD 1.4.7\n",
    );
    in_child!(
        a_refused_version_is_exited_and_a_later_load_succeeds,
        fake.child(),
        || {
            match common::load_from(common::fake_path()) {
                Err(LoadError::UnsupportedVersion(version)) => {
                    assert_eq!(
                        version,
                        Version {
                            major: 2,
                            minor: 0,
                            build: 0
                        }
                    )
                }
                other => panic!("expected UnsupportedVersion, got {other:?}"),
            }
            let sane = common::load_fake();
            assert_eq!(
                sane.version(),
                Version {
                    major: 1,
                    minor: 4,
                    build: 7
                }
            );
        }
    );
    let log = fake.log();
    log.assert_no_violations();
    assert_eq!(
        log.calls_but_options(),
        ["sane_init", "sane_exit", "sane_init", "sane_exit"]
    );
}

#[test]
fn a_failed_init_isnt_exited_and_a_later_load_succeeds() {
    let fake = Fake::new(
        "a_failed_init_isnt_exited_and_a_later_load_succeeds",
        "init IO_ERROR\ninit GOOD 1.0.3\n",
    );
    in_child!(
        a_failed_init_isnt_exited_and_a_later_load_succeeds,
        fake.child(),
        || {
            match common::load_from(common::fake_path()) {
                Err(LoadError::Init(status)) => assert_eq!(status, sane_dl::Status::IO_ERROR),
                other => panic!("expected Init, got {other:?}"),
            }
            assert_eq!(common::load_fake().version().build, 3);
        }
    );
    let log = fake.log();
    log.assert_no_violations();
    assert_eq!(
        log.calls_but_options(),
        ["sane_init", "sane_init", "sane_exit"]
    );
}

#[test]
fn an_unknown_init_status_is_kept() {
    let fake = Fake::new("an_unknown_init_status_is_kept", "init 42\n");
    in_child!(an_unknown_init_status_is_kept, fake.child(), || {
        match common::load_from(common::fake_path()) {
            Err(LoadError::Init(status)) => assert_eq!(status, sane_dl::Status(42)),
            other => panic!("expected Init, got {other:?}"),
        }
    });
    fake.log().assert_no_violations();
}
