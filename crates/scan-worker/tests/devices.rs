//! Devices listed and opened through a worker, what they can do, and the calls refused before a
//! worker starts.

mod support;

use std::{
    io,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use scan_worker::scan_device::{
    DeviceId, DeviceInfo, Driver, ScanError, WindowClosed, WindowOwner,
};
use scan_worker::{Client, Error, ProtocolError, RemoteDevice, Timeout, Timeouts, WorkerCommand};

use support::{AT_MOST, client, client_of, open, scenarios, stored, worker};

#[test]
fn a_listing_gives_the_worker_s_devices() {
    let listed = client_of("feeder", 1)
        .list(Driver::Fake, AT_MOST)
        .expect("a listing");
    let mut in_process = scenarios::backend("feeder", 1).unwrap();
    assert_eq!(listed, in_process.list(AT_MOST).unwrap());
    assert_eq!(listed, [stored()]);
}

#[test]
fn a_device_of_another_driver_than_the_worker_s_is_refused() {
    // The worker table set up wrong: the fake's worker given for TWAIN's 32-bit devices.
    let twain = Driver::Twain { bits: 32 };
    let client = Client::new().worker(twain, worker("feeder"));
    let listed = client.list(twain, AT_MOST);
    assert!(
        matches!(
            listed,
            Err(Error::Protocol(ProtocolError::Invalid { what }))
                if what.contains("another driver")
        ),
        "{listed:?}"
    );
    let mut stored = stored();
    stored.driver = twain;
    let opened = client.open(&stored, AT_MOST);
    assert!(
        matches!(
            opened,
            Err(Error::Protocol(ProtocolError::Invalid { what }))
                if what.contains("another driver")
        ),
        "{opened:?}"
    );
}

#[test]
fn a_stored_device_opens_as_stored_or_renamed() {
    let client = client("renamed");
    let found = DeviceInfo::new(
        DeviceId("fake:libusb:001:005".to_owned()),
        "Maker Model".to_owned(),
        "Maker".to_owned(),
        "Model".to_owned(),
        Driver::Fake,
    );
    let as_stored = client.open(&found, AT_MOST).expect("the stored device");
    assert_eq!(as_stored.info(), &found);
    assert_eq!(as_stored.renamed_from(), None);
    as_stored.close().expect("closed");
    // Stored under the name it had before it was plugged in again.
    let mut before = found.clone();
    before.id = DeviceId("fake:libusb:001:004".to_owned());
    let renamed = client.open(&before, AT_MOST).expect("the device renamed");
    assert_eq!(renamed.info(), &found);
    assert_eq!(renamed.renamed_from(), Some(&before.id));
    renamed.close().expect("closed");
    // Gone, and an id the backend doesn't form, as its backend says.
    let mut gone = found;
    gone.id = DeviceId("fake:desk".to_owned());
    assert!(matches!(
        client.open(&gone, AT_MOST),
        Err(Error::Scan(ScanError::Gone))
    ));
    gone.id = DeviceId("sane:test:0".to_owned());
    assert!(matches!(
        client.open(&gone, AT_MOST),
        Err(Error::Scan(ScanError::BadId))
    ));
}

#[test]
fn a_driver_not_installed_says_so() {
    let client = client("not-installed");
    for answer in [
        client.list(Driver::Fake, AT_MOST).map(|_| ()),
        client.open(&stored(), AT_MOST).map(|_| ()),
    ] {
        assert!(
            matches!(
                &answer,
                Err(Error::Scan(ScanError::NotInstalled { what })) if what == "the fake driver"
            ),
            "{answer:?}"
        );
    }
}

#[test]
fn a_listing_past_its_deadline_times_out_and_its_worker_is_killed() {
    // The worker's exit, once it's asked to quit, takes a minute: only a kill ends it before
    // the long grace given here.
    let mut timeouts = Timeouts::default();
    timeouts.grace = Duration::from_secs(30);
    let client = client("slow-listing").timeouts(timeouts);
    let start = Instant::now();
    let listed = client.list(Driver::Fake, Duration::from_millis(200));
    assert!(
        matches!(listed, Err(Error::Scan(ScanError::TimedOut))),
        "{listed:?}"
    );
    assert!(start.elapsed() < AT_MOST, "{:?}", start.elapsed());
    // An open past its deadline the same.
    let start = Instant::now();
    let opened = client.open(&stored(), Duration::from_millis(200));
    assert!(
        matches!(opened, Err(Error::Scan(ScanError::TimedOut))),
        "{opened:?}"
    );
    assert!(start.elapsed() < AT_MOST, "{:?}", start.elapsed());
}

#[test]
fn a_worker_that_never_answers_a_listing_is_stopped_past_its_deadline_and_grace() {
    let mut timeouts = Timeouts::default();
    timeouts.grace = Duration::from_millis(300);
    let client = client("deaf").timeouts(timeouts);
    let start = Instant::now();
    let listed = client.list(Driver::Fake, Duration::from_millis(200));
    let Err(Error::Stopped {
        timeout, waited, ..
    }) = listed
    else {
        panic!("{listed:?}");
    };
    assert_eq!(
        (timeout, waited),
        (Timeout::Deadline, Duration::from_millis(500))
    );
    assert!(start.elapsed() < AT_MOST, "{:?}", start.elapsed());
    let opened = client.open(&stored(), Duration::from_millis(200));
    assert!(matches!(opened, Err(Error::Stopped { .. })), "{opened:?}");
}

#[test]
fn caps_and_the_settings_window_are_the_device_s() {
    let mut device = open(&client("window"));
    let mut in_process = support::open_in_process("window", 0);
    let caps = device.caps().expect("caps");
    assert!(caps.settings_window);
    assert_eq!(caps, in_process.caps().unwrap());
    assert_eq!(
        device
            .settings_window(Some(WindowOwner(u64::MAX)))
            .expect("the window"),
        WindowClosed::Accepted
    );
    assert_eq!(
        device.settings_window(None).unwrap(),
        WindowClosed::Accepted
    );
    // And the device goes on.
    assert_eq!(device.caps().unwrap(), caps);
    device.close().expect("closed");
    // A device without one.
    let mut plain = open(&client("feeder"));
    assert!(!plain.caps().unwrap().settings_window);
    assert!(matches!(
        plain.settings_window(None),
        Err(Error::Scan(ScanError::Unsupported))
    ));
    assert!(plain.caps().is_ok());
}

#[test]
fn each_open_device_has_a_worker_of_its_own() {
    let client = client("feeder");
    let first = open(&client);
    let second = open(&client);
    assert_ne!(first.process_id(), second.process_id());
    assert_ne!(first.process_id(), std::process::id());
    assert!(format!("{first:?}").contains("process_id"), "{first:?}");
    first.close().unwrap();
    second.close().unwrap();
}

#[test]
fn a_driver_with_no_worker_is_refused() {
    let client = Client::new().worker(Driver::Fake, worker("feeder"));
    assert!(matches!(
        client.list(Driver::Wia, AT_MOST),
        Err(Error::NoWorker(Driver::Wia))
    ));
    let mut stored = stored();
    stored.driver = Driver::Sane;
    assert!(matches!(
        client.open(&stored, AT_MOST),
        Err(Error::NoWorker(Driver::Sane))
    ));
    // The last command given for a driver is its worker.
    let replaced = client.worker(Driver::Fake, worker("not-installed"));
    assert!(matches!(
        replaced.list(Driver::Fake, AT_MOST),
        Err(Error::Scan(ScanError::NotInstalled { .. }))
    ));
}

/// The start of a listing and an open through `client` refused, each with an error of `kind`:
/// the program `program`'s where it's given, else the work directory `dir`'s.
fn refused_at_start(client: &Client, program: Option<&PathBuf>, dir: &Path, kind: io::ErrorKind) {
    for started in [
        client.list(Driver::Fake, AT_MOST).map(|_| ()),
        client.open(&stored(), AT_MOST).map(|_| ()),
    ] {
        match (started, program) {
            (
                Err(Error::Start {
                    program: refused,
                    source,
                    ..
                }),
                Some(program),
            ) => {
                assert_eq!(&refused, program);
                assert_eq!(source.kind(), kind, "{source}");
            }
            (
                Err(Error::WorkDir {
                    dir: refused,
                    source,
                    ..
                }),
                None,
            ) => {
                assert_eq!(refused, dir);
                assert_eq!(source.kind(), kind, "{source}");
            }
            (other, _) => panic!("{other:?}"),
        }
    }
}

#[test]
fn a_program_or_a_work_directory_not_absolute_or_missing_is_refused_at_start() {
    let relative = PathBuf::from("scan-worker-fake");
    let client = Client::new().worker(Driver::Fake, WorkerCommand::new(&relative).arg("feeder"));
    let temp = std::env::temp_dir();
    refused_at_start(&client, Some(&relative), &temp, io::ErrorKind::InvalidInput);
    let work = tempfile::tempdir().unwrap();
    let missing = work.path().join("no-such-worker");
    let client = Client::new().worker(Driver::Fake, WorkerCommand::new(&missing));
    refused_at_start(&client, Some(&missing), &temp, io::ErrorKind::NotFound);
    let relative_work = Path::new("relative/work");
    let client = support::client("feeder").work_dir(relative_work);
    refused_at_start(&client, None, relative_work, io::ErrorKind::InvalidInput);
    // A work directory that isn't there.
    let not_there = work.path().join("not-there");
    let client = support::client("feeder").work_dir(&not_there);
    refused_at_start(&client, None, &not_there, io::ErrorKind::NotFound);
    // Nothing was left in the work directory.
    assert_eq!(std::fs::read_dir(work.path()).unwrap().count(), 0);
}

#[test]
fn a_worker_command_keeps_its_program_and_arguments() {
    let command = WorkerCommand::new("/bin/worker")
        .arg("--scan-worker")
        .arg("sane");
    assert_eq!(command.program(), std::path::Path::new("/bin/worker"));
    assert_eq!(
        command.args().collect::<Vec<_>>(),
        ["--scan-worker", "sane"]
    );
}

#[test]
fn a_client_is_send_and_sync_and_an_open_device_send() {
    fn send_and_sync<T: Send + Sync>() {}
    fn send<T: Send>() {}
    send_and_sync::<Client>();
    send::<RemoteDevice>();
    send_and_sync::<scan_worker::StopHandle>();
    send_and_sync::<scan_worker::TwainList>();
    send_and_sync::<Error>();
}
