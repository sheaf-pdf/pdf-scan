//! What the integration tests share: the test worker, its scenarios' scripts, and a batch run
//! through a worker or in process.

// Each test binary takes what it needs of these.
#![allow(dead_code)]

#[path = "../../src/bin/scan-worker-fake/scenarios.rs"]
pub mod scenarios;

use std::{fs, path::PathBuf, sync::mpsc, time::Duration};

use scan_worker::scan_device::fake::pattern;
use scan_worker::scan_device::in_progress::Tracker;
use scan_worker::scan_device::{
    Device, DeviceInfo, Done, Driver, Event, Format, Pixels, RawPage, ScanError, Settings, Source,
};
use scan_worker::{Client, Error, RemoteDevice, WorkerCommand};

/// The longest a test lets a listing or an open take: far past any it should.
pub const AT_MOST: Duration = Duration::from_secs(10);

/// The test worker, running `scenario`.
pub fn worker(scenario: &str) -> WorkerCommand {
    WorkerCommand::new(env!("CARGO_BIN_EXE_scan-worker-fake")).arg(scenario)
}

/// The test worker, running `scenario` with `count` sheets.
pub fn worker_of(scenario: &str, count: usize) -> WorkerCommand {
    worker(scenario).arg(count.to_string())
}

/// A client whose fake devices' worker runs `scenario`.
pub fn client(scenario: &str) -> Client {
    Client::new().worker(Driver::Fake, worker(scenario))
}

/// A client whose fake devices' worker runs `scenario` with `count` sheets.
pub fn client_of(scenario: &str, count: usize) -> Client {
    Client::new().worker(Driver::Fake, worker_of(scenario, count))
}

/// `fake:desk` as a listing gives it and an app stores it.
pub fn stored() -> DeviceInfo {
    scenarios::desk().info()
}

/// `fake:desk` opened through `client`.
pub fn open(client: &Client) -> RemoteDevice {
    client.open(&stored(), AT_MOST).expect("the device opened")
}

/// `fake:desk` opened in process, in `scenario`'s script with `count` sheets.
pub fn open_in_process(scenario: &str, count: usize) -> Box<dyn Device> {
    let mut backend = scenarios::backend(scenario, count).expect("a scenario");
    backend
        .open(&stored(), AT_MOST)
        .expect("the device opened")
        .device
}

pub fn settings(source: Source, dpi: u32, format: Format, duplex: bool) -> Settings {
    let mut settings = Settings::new(source, dpi, format);
    settings.duplex = duplex;
    settings
}

/// A batch through `device`'s worker: its events, and how it ended.
pub fn scan(
    device: &mut RemoteDevice,
    settings: &Settings,
    first_sheet: u32,
) -> (Vec<Event>, Result<Done, Error>) {
    let (events, received) = mpsc::sync_channel(1024);
    let ended = device.scan(settings, first_sheet, &events, &Tracker::new());
    drop(events);
    (received.iter().collect(), ended)
}

/// The same batch in process: its events, and how it ended.
pub fn scan_in_process(
    device: &mut dyn Device,
    settings: &Settings,
    first_sheet: u32,
) -> (Vec<Event>, Result<Done, ScanError>) {
    let (events, received) = mpsc::sync_channel(1024);
    let ended = device.scan(settings, first_sheet, &events, &Tracker::new());
    drop(events);
    (received.iter().collect(), ended)
}

/// The pages among `events`.
pub fn pages(events: &[Event]) -> Vec<&RawPage> {
    events
        .iter()
        .filter_map(|event| match event {
            Event::Page(page) => Some(page),
            _ => None,
        })
        .collect()
}

/// Each raw page among `events` the one the scripted backend draws for its sheet and side.
pub fn pages_are_pattern_s(events: &[Event]) {
    for page in pages(events) {
        let Pixels::Raw(image) = &page.pixels else {
            continue;
        };
        let drawn = pattern(
            image.format(),
            image.width(),
            image.height(),
            page.sheet,
            page.side,
        )
        .expect("a page the fake draws");
        assert!(
            image.data() == drawn.data(),
            "sheet {}'s {:?} isn't the fake's",
            page.sheet,
            page.side
        );
    }
}

/// `path` resolved: the worker's current directory is, so every path compared is.
pub fn resolved(path: impl Into<PathBuf>) -> PathBuf {
    let path = path.into();
    fs::canonicalize(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

/// Whether `ended` is the device's `error`.
pub fn is_scan(ended: &Result<Done, Error>, error: &ScanError) -> bool {
    matches!(ended, Err(Error::Scan(got)) if got == error)
}
