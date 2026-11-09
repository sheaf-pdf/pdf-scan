//! Loading SANE: what's refused, and that a failed load leaves nothing
//! behind. Each test runs in a child process of its own. What needs a path
//! of its own (a library missing SANE's functions, a path where nothing is)
//! is tested inside the crate, over its private loader.

#[macro_use]
mod common;

use common::Fake;
use sane_dl::{LoadError, Version};

#[cfg(windows)]
#[test]
fn there_is_no_default_library_on_windows() {
    in_child!(
        there_is_no_default_library_on_windows,
        common::Child::new(),
        || {
            assert!(matches!(sane_dl::Sane::load(), Err(LoadError::NoDefault)));
        }
    );
}

#[test]
#[cfg_attr(not(unix), ignore = "loads the fake by SANE's name: none on Windows")]
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
                common::try_load_fake(),
                Err(LoadError::AlreadyLoaded)
            ));
            let clone = sane.clone();
            let handle = sane.open("fake:0").unwrap();
            drop(sane);
            drop(clone);
            // The handle keeps SANE started.
            assert!(matches!(
                common::try_load_fake(),
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
#[cfg_attr(not(unix), ignore = "loads the fake by SANE's name: none on Windows")]
fn a_refused_version_is_exited_and_a_later_load_succeeds() {
    let fake = Fake::new(
        "a_refused_version_is_exited_and_a_later_load_succeeds",
        "init GOOD 2.0.0\ninit GOOD 1.4.7\n",
    );
    in_child!(
        a_refused_version_is_exited_and_a_later_load_succeeds,
        fake.child(),
        || {
            match common::try_load_fake() {
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
#[cfg_attr(not(unix), ignore = "loads the fake by SANE's name: none on Windows")]
fn a_failed_init_isnt_exited_and_a_later_load_succeeds() {
    let fake = Fake::new(
        "a_failed_init_isnt_exited_and_a_later_load_succeeds",
        "init IO_ERROR\ninit GOOD 1.0.3\n",
    );
    in_child!(
        a_failed_init_isnt_exited_and_a_later_load_succeeds,
        fake.child(),
        || {
            match common::try_load_fake() {
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
#[cfg_attr(not(unix), ignore = "loads the fake by SANE's name: none on Windows")]
fn an_unknown_init_status_is_kept() {
    let fake = Fake::new("an_unknown_init_status_is_kept", "init 42\n");
    in_child!(an_unknown_init_status_is_kept, fake.child(), || {
        match common::try_load_fake() {
            Err(LoadError::Init(status)) => assert_eq!(status, sane_dl::Status(42)),
            other => panic!("expected Init, got {other:?}"),
        }
    });
    fake.log().assert_no_violations();
}
