//! How a worker ends. Unasked (a crash, an exit, its output cut short within a page): the pages
//! before it kept, how it ended said, and the end of its standard error. Asked (`close`, and
//! drop): reaped within the grace, killed where it won't exit, its directory removed.

mod support;

use std::{
    sync::mpsc,
    time::{Duration, Instant},
};

use scan_worker::scan_device::in_progress::Tracker;

use scan_worker::scan_device::Driver;
use scan_worker::scan_device::{Event, Format, Pixels, Source};
use scan_worker::{Client, Error, RemoteDevice, Timeout, Timeouts};

use support::{
    AT_MOST, aborted, client, client_of, entries, open, outline, pages_are_pattern_s, reaped, scan,
    scenarios, settings, stored, worker_of,
};

/// A duplex batch through `scenario`'s worker of five sheets, whose driver ends its process
/// midway through sheet 3's front: the events, and the error.
fn ended_at_sheet_3(scenario: &str) -> (Vec<Event>, Error) {
    let mut device = open(&client_of(scenario, 5));
    let (events, ended) = scan(
        &mut device,
        &settings(Source::Feeder, 100, Format::Gray8, true),
        1,
    );
    let Err(error) = ended else {
        panic!("{scenario}: {ended:?}");
    };
    // Every call after it answers `Ended`.
    assert!(matches!(device.caps(), Err(Error::Ended)), "{scenario}");
    device.close().expect("closed already");
    (events, error)
}

/// Sheets 1 and 2 whole among `events`, then at most sheet 3's `Sheet`: written or not as the
/// worker ended, since its driver takes the sheet as the batch goes on.
fn sheets_1_and_2_kept(scenario: &str, events: &[Event]) {
    let before = [
        "applied",
        "sheet 1",
        "page 1 front",
        "page 1 back",
        "sheet 2",
        "page 2 front",
        "page 2 back",
    ];
    let outline = outline(events);
    assert!(
        outline == before || outline == [&before[..], &["sheet 3"]].concat(),
        "{scenario}: {outline:?}"
    );
    pages_are_pattern_s(events);
}

#[test]
fn a_driver_s_crash_keeps_the_pages_before_it_and_says_the_abort() {
    let (events, error) = ended_at_sheet_3("crash");
    sheets_1_and_2_kept("crash", &events);
    let Error::Crashed {
        status,
        page_cut_short,
        stderr,
        ..
    } = error
    else {
        panic!("{error:?}");
    };
    let status = status.expect("exited within the grace");
    assert!(aborted(&status), "{status}");
    assert!(!page_cut_short);
    assert!(stderr.contains(scenarios::FAILING), "{stderr:?}");
}

#[test]
fn a_driver_s_exit_keeps_the_pages_before_it_and_says_its_code() {
    let (events, error) = ended_at_sheet_3("exit");
    sheets_1_and_2_kept("exit", &events);
    let Error::Crashed {
        status,
        page_cut_short,
        stderr,
        ..
    } = error
    else {
        panic!("{error:?}");
    };
    assert_eq!(status.expect("exited within the grace").code(), Some(42));
    assert!(!page_cut_short);
    assert!(stderr.contains(scenarios::FAILING), "{stderr:?}");
}

#[test]
fn a_page_cut_short_is_dropped_and_said() {
    let mut device = open(&client("cut-short"));
    let (events, ended) = scan(
        &mut device,
        &settings(Source::Feeder, 300, Format::Gray8, false),
        1,
    );
    assert!(events.is_empty(), "{events:?}");
    let Err(Error::Crashed {
        status,
        page_cut_short,
        ..
    }) = ended
    else {
        panic!("{ended:?}");
    };
    assert!(page_cut_short);
    assert!(status.expect("exited within the grace").success());
    assert!(matches!(device.caps(), Err(Error::Ended)));
}

#[test]
fn a_whole_page_just_before_the_output_ends_is_kept() {
    let mut device = open(&client("page-then-end"));
    let (events, ended) = scan(
        &mut device,
        &settings(Source::Feeder, 300, Format::Gray8, false),
        1,
    );
    let [Event::Applied(_), Event::Sheet(1), Event::Page(page)] = &events[..] else {
        panic!("{events:?}");
    };
    let Pixels::Raw(image) = &page.pixels else {
        panic!("a raw page");
    };
    assert_eq!((image.width(), image.height()), (100, 100));
    assert!(image.data().iter().all(|byte| *byte == 7));
    assert!(
        matches!(
            ended,
            Err(Error::Crashed {
                page_cut_short: false,
                ..
            })
        ),
        "{ended:?}"
    );
}

/// `device` ended by `end`, `close` or drop: its worker reaped and its directory, the one entry
/// in `work`, removed by the time `end` returns, within `limit`. Gives what `end` gave.
fn ends_reaped<T>(
    device: RemoteDevice,
    work: &tempfile::TempDir,
    limit: Duration,
    end: impl FnOnce(RemoteDevice) -> T,
) -> T {
    let process = device.process_id();
    assert_eq!(entries(work.path()), 1);
    let start = Instant::now();
    let ended = end(device);
    assert!(start.elapsed() < limit, "{:?}", start.elapsed());
    assert_eq!(entries(work.path()), 0);
    assert!(reaped(process), "the worker {process} is unreaped");
    ended
}

#[test]
fn close_and_drop_reap_a_worker_that_exits_when_asked() {
    let work = tempfile::tempdir().unwrap();
    let client = client("feeder").work_dir(work.path());
    let closed = ends_reaped(open(&client), &work, AT_MOST, RemoteDevice::close);
    closed.expect("closed");
    ends_reaped(open(&client), &work, AT_MOST, drop);
}

#[test]
fn close_and_drop_kill_and_reap_a_worker_that_won_t_exit_within_the_grace() {
    // The worker's exit, once it's asked to quit, takes a minute.
    let mut timeouts = Timeouts::default();
    timeouts.grace = Duration::from_millis(300);
    let work = tempfile::tempdir().unwrap();
    let client = client("slow-exit").timeouts(timeouts).work_dir(work.path());
    let opened = || client.open(&stored(), AT_MOST).expect("opened");
    // The grace to exit, then the kill, reaped within the grace again.
    let limit = Duration::from_secs(5);
    let closed = ends_reaped(opened(), &work, limit, RemoteDevice::close);
    assert!(
        matches!(
            closed,
            Err(Error::Stopped { timeout: Timeout::Exit, waited, .. }) if waited == timeouts.grace
        ),
        "{closed:?}"
    );
    ends_reaped(opened(), &work, limit, drop);
}

#[test]
fn a_grace_past_the_worker_s_own_10_s_lets_a_slow_driver_close() {
    // The worker's exit, once it's asked to quit, takes 11 s: past the 10 s after its input's
    // end at which a worker aborts, within the grace.
    let mut timeouts = Timeouts::default();
    timeouts.grace = Duration::from_secs(30);
    let client = Client::new()
        .worker(Driver::Fake, worker_of("slow-exit", 11))
        .timeouts(timeouts);
    let device = open(&client);
    let start = Instant::now();
    let closed = device.close();
    let elapsed = start.elapsed();
    assert!(closed.is_ok(), "{closed:?} after {elapsed:?}");
    assert!(
        elapsed >= Duration::from_secs(10) && elapsed < timeouts.grace,
        "{elapsed:?}"
    );
}

/// The worker ended unasked through `call`, by a driver that started a process holding the
/// worker's pipes: said at once, as how it exited, not when that process lets them go.
fn ended_with_its_pipes_held(
    call: impl FnOnce(&mut RemoteDevice) -> Result<(), Error>,
    scenario: &str,
) -> Option<std::process::ExitStatus> {
    let mut timeouts = Timeouts::default();
    timeouts.silence = Duration::from_secs(20);
    let mut device = open(&client(scenario).timeouts(timeouts));
    let start = Instant::now();
    let ended = call(&mut device);
    let elapsed = start.elapsed();
    assert!(elapsed < Duration::from_secs(5), "{scenario}: {elapsed:?}");
    let Err(Error::Crashed {
        status,
        page_cut_short,
        ..
    }) = ended
    else {
        panic!("{scenario}: {ended:?}");
    };
    assert!(!page_cut_short);
    assert!(matches!(device.caps(), Err(Error::Ended)));
    status
}

#[test]
fn a_worker_that_ends_with_its_output_held_open_is_seen_ended() {
    let status = ended_with_its_pipes_held(
        |device| device.settings_window(None).map(|_| ()),
        "helper-window",
    );
    let status = status.expect("exited");
    assert!(aborted(&status), "{status}");
    let status = ended_with_its_pipes_held(
        |device| {
            let (events, received) = mpsc::sync_channel(16);
            let ended = device.scan(
                &settings(Source::Feeder, 300, Format::Gray8, false),
                1,
                &events,
                &Tracker::new(),
            );
            drop(events);
            let outline = outline(&received.iter().collect::<Vec<_>>());
            assert_eq!(outline, ["applied", "sheet 1"]);
            ended.map(|_| ())
        },
        "helper-batch",
    );
    assert_eq!(status.expect("exited").code(), Some(7));
}
