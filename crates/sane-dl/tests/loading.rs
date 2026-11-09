//! Loading SANE: what's refused, and that a failed load leaves nothing
//! behind. Each test runs in a child process of its own. What needs a path
//! of its own (a library missing SANE's functions, a path where nothing is)
//! is tested inside the crate, over its private loader. On Linux, the
//! process's slot, which every copy of the crate in a process takes: bound
//! here by hand, as another copy would bind it.

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

/// The file name SANE is loaded by on Linux, and so the fake's copy.
#[cfg(target_os = "linux")]
const SANES_NAME: &str = "libsane.so.1";

/// In the child: binds the process's slot, as every copy of sane-dl binds
/// it. Its name is fixed for good, so every version finds every other's:
/// it's formed here by the same rule, and these tests fail if the crate's
/// rule ever changes.
#[cfg(target_os = "linux")]
fn bind_slot() -> std::io::Result<std::os::unix::net::UnixDatagram> {
    use std::os::linux::net::SocketAddrExt;
    use std::os::unix::fs::MetadataExt;
    use std::os::unix::net::{SocketAddr, UnixDatagram};
    let pid = std::process::id();
    let name = match std::fs::metadata("/proc/self/ns/pid") {
        Ok(namespace) => format!(
            "sane-dl/slot/pidns/{}/{}/pid/{pid}",
            namespace.dev(),
            namespace.ino()
        ),
        Err(_) => format!("sane-dl/slot/pidns/none/pid/{pid}"),
    };
    UnixDatagram::bind_addr(&SocketAddr::from_abstract_name(name.as_bytes())?)
}

/// In the child: whether a library by SANE's name is mapped into this
/// process, so whether one was opened.
#[cfg(target_os = "linux")]
fn sane_is_mapped() -> bool {
    std::fs::read_to_string("/proc/self/maps")
        .expect("couldn't read this process's maps")
        .contains(SANES_NAME)
}

/// In the child: that the fake was never started, its log not written.
#[cfg(target_os = "linux")]
fn assert_fake_not_started() {
    let log = std::env::var_os("FAKE_LIBSANE_LOG").expect("the parent names the fake's log");
    assert!(
        !std::path::Path::new(&log).exists(),
        "the fake's log is there, so it was started"
    );
}

#[cfg(target_os = "linux")]
#[test]
fn a_slot_bound_first_refuses_the_load_before_the_library_opens() {
    let fake = Fake::new(
        "a_slot_bound_first_refuses_the_load_before_the_library_opens",
        "",
    );
    in_child!(
        a_slot_bound_first_refuses_the_load_before_the_library_opens,
        fake.child(),
        || {
            // Another copy of the crate holding SANE would hold this.
            let held = bind_slot().expect("couldn't bind the slot");
            match sane_dl::Sane::load() {
                Err(LoadError::AlreadyLoaded) => {}
                other => panic!("expected AlreadyLoaded, got {other:?}"),
            }
            assert!(!sane_is_mapped(), "the library was opened");
            assert_fake_not_started();
            // Once it's free, this copy loads: its own slot was freed too.
            drop(held);
            let _sane = common::load_fake();
            assert!(sane_is_mapped(), "the fake isn't mapped");
        }
    );
    let log = fake.log();
    log.assert_no_violations();
    assert_eq!(log.calls_but_options(), ["sane_init", "sane_exit"]);
}

#[cfg(target_os = "linux")]
#[test]
fn the_slot_is_held_until_sane_and_its_handles_are_gone() {
    let fake = Fake::new(
        "the_slot_is_held_until_sane_and_its_handles_are_gone",
        "device fake:0 Fake One scanner\n",
    );
    in_child!(
        the_slot_is_held_until_sane_and_its_handles_are_gone,
        fake.child(),
        || {
            let in_use = |what: &str| match bind_slot() {
                Err(error) if error.kind() == std::io::ErrorKind::AddrInUse => {}
                other => panic!("{what}: expected the slot in use, got {other:?}"),
            };
            let sane = common::load_fake();
            in_use("SANE loaded");
            let handle = sane.open("fake:0").unwrap();
            drop(sane);
            // The handle keeps SANE started.
            in_use("a handle open");
            drop(handle);
            let held = bind_slot().expect("the slot wasn't freed when SANE ended");
            match sane_dl::Sane::load() {
                Err(LoadError::AlreadyLoaded) => {}
                other => panic!("expected AlreadyLoaded, got {other:?}"),
            }
            drop(held);
        }
    );
    let log = fake.log();
    log.assert_no_violations();
    assert_eq!(
        log.calls_but_options(),
        ["sane_init", "sane_open", "sane_close", "sane_exit"]
    );
}

#[cfg(target_os = "linux")]
#[test]
fn a_refused_load_frees_the_slot() {
    let fake = Fake::new(
        "a_refused_load_frees_the_slot",
        "init IO_ERROR\ninit GOOD 2.0.0\n",
    );
    in_child!(a_refused_load_frees_the_slot, fake.child(), || {
        match common::try_load_fake() {
            Err(LoadError::Init(status)) => assert_eq!(status, sane_dl::Status::IO_ERROR),
            other => panic!("expected Init, got {other:?}"),
        }
        drop(bind_slot().expect("a failed sane_init didn't free the slot"));
        match common::try_load_fake() {
            Err(LoadError::UnsupportedVersion(version)) => assert_eq!(version.major, 2),
            other => panic!("expected UnsupportedVersion, got {other:?}"),
        }
        drop(bind_slot().expect("a refused version didn't free the slot"));
    });
    let log = fake.log();
    log.assert_no_violations();
    assert_eq!(
        log.calls_but_options(),
        ["sane_init", "sane_init", "sane_exit"]
    );
}

/// The system refusing the slot's socket, as a sandbox would: here by the
/// process having no file descriptor left, its limit lowered with
/// util-linux's `prlimit`, which a process can do to itself.
#[cfg(target_os = "linux")]
#[test]
fn a_socket_the_system_refuses_refuses_the_load() {
    let fake = Fake::new("a_socket_the_system_refuses_refuses_the_load", "");
    in_child!(
        a_socket_the_system_refuses_refuses_the_load,
        fake.child(),
        || {
            /// `EMFILE`: this process has no file descriptor left.
            const EMFILE: i32 = 24;
            /// The limit set, far above what the child has open.
            const LIMIT: usize = 256;
            let lowered = std::process::Command::new("prlimit")
                .arg(format!("--pid={}", std::process::id()))
                .arg(format!("--nofile={LIMIT}:"))
                .status();
            match lowered {
                Ok(status) if status.success() => {}
                Ok(status) => common::skip(format!("prlimit couldn't lower the limit ({status})")),
                Err(error) => common::skip(format!("there's no prlimit to run here ({error})")),
            }
            let mut files = Vec::new();
            let exhausted = loop {
                match std::fs::File::open("/dev/null") {
                    Ok(file) if files.len() < LIMIT => files.push(file),
                    Ok(_) => break false,
                    Err(error) if error.raw_os_error() == Some(EMFILE) => break true,
                    Err(error) => panic!("couldn't open /dev/null: {error}"),
                }
            };
            assert!(exhausted, "the limit of {LIMIT} descriptors didn't take");
            let loaded = sane_dl::Sane::load();
            drop(files);
            match loaded {
                Err(LoadError::SlotRefused(error)) => {
                    assert_eq!(error.raw_os_error(), Some(EMFILE))
                }
                other => panic!("expected SlotRefused, got {other:?}"),
            }
            assert!(!sane_is_mapped(), "the library was opened");
            assert_fake_not_started();
            // Nothing was left held: with descriptors again, it loads.
            let _sane = common::load_fake();
        }
    );
    let log = fake.log();
    log.assert_no_violations();
    assert_eq!(log.calls_but_options(), ["sane_init", "sane_exit"]);
}
