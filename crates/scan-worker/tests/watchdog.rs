//! The watchdog: a worker silent past its time killed, the pages before it kept; and a caller
//! slower than the worker's time never taken for the worker's silence.

mod support;

use std::{
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

use scan_worker::scan_device::in_progress::Tracker;
use scan_worker::scan_device::{Done, Driver, Event, Format, Source};
use scan_worker::{Error, Timeouts};

use support::{
    AT_MOST, client, client_of, outline, pages_are_pattern_s, scan, scenarios, settings, stored,
};

#[test]
fn a_driver_that_hangs_is_stopped_after_the_silence_with_the_pages_before_it_kept() {
    let mut timeouts = Timeouts::default();
    timeouts.silence = Duration::from_millis(500);
    let mut device = client_of("hang", 5)
        .timeouts(timeouts)
        .open(&stored(), AT_MOST)
        .expect("opened");
    let start = Instant::now();
    // The driver hangs midway through sheet 3's front.
    let (events, ended) = scan(
        &mut device,
        &settings(Source::Feeder, 100, Format::Gray8, true),
        1,
    );
    let elapsed = start.elapsed();
    let Err(Error::Stopped { silence, stderr }) = ended else {
        panic!("{ended:?}");
    };
    assert!(
        silence >= Duration::from_millis(500) && silence < Duration::from_secs(5),
        "{silence:?}"
    );
    assert!(elapsed < AT_MOST, "{elapsed:?}");
    assert!(stderr.contains(scenarios::FAILING), "{stderr:?}");
    // Sheets 1 and 2 whole, and sheet 3 said, its front never sent.
    assert_eq!(
        outline(&events),
        [
            "applied",
            "sheet 1",
            "page 1 front",
            "page 1 back",
            "sheet 2",
            "page 2 front",
            "page 2 back",
            "sheet 3"
        ]
    );
    pages_are_pattern_s(&events);
    // Killed: every call after it answers `Ended`.
    assert!(matches!(device.caps(), Err(Error::Ended)));
    device.close().expect("closed already");
}

#[test]
fn a_worker_silent_as_it_starts_is_stopped_after_the_start_s_time() {
    let mut timeouts = Timeouts::default();
    timeouts.start = Duration::from_millis(300);
    let client = client("silent").timeouts(timeouts);
    for started in [
        client.list(Driver::Fake, AT_MOST).map(|_| ()),
        client.open(&stored(), AT_MOST).map(|_| ()),
    ] {
        let Err(Error::Stopped { silence, .. }) = started else {
            panic!("{started:?}");
        };
        assert_eq!(silence, Duration::from_millis(300));
    }
}

#[test]
fn a_caller_slower_than_the_silence_isn_t_taken_for_the_worker_s() {
    let mut timeouts = Timeouts::default();
    timeouts.silence = Duration::from_millis(250);
    let mut device = client_of("feeder", 1)
        .timeouts(timeouts)
        .open(&stored(), AT_MOST)
        .expect("opened");
    // A rendezvous channel, and a caller that takes each event twice the silence late: the
    // worker waits on it, never the other way.
    let late = Duration::from_millis(500);
    let (events, received) = mpsc::sync_channel(0);
    let caller = thread::spawn(move || {
        let mut taken = Vec::new();
        loop {
            thread::sleep(late);
            match received.recv() {
                Ok(event) => taken.push(event),
                Err(_) => return taken,
            }
        }
    });
    let start = Instant::now();
    let done = device.scan(
        &settings(Source::Feeder, 100, Format::Gray8, true),
        1,
        &events,
        &Tracker::new(),
    );
    drop(events);
    let taken: Vec<Event> = caller.join().expect("the caller");
    assert_eq!(done.expect("a batch"), Done::FeederEmpty);
    assert_eq!(
        outline(&taken),
        ["applied", "sheet 1", "page 1 front", "page 1 back"]
    );
    pages_are_pattern_s(&taken);
    // Each of the four events waited on the caller.
    assert!(start.elapsed() >= late * 4, "{:?}", start.elapsed());
    device.close().expect("closed");
}
