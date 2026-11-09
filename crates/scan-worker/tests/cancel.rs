//! A batch cancelled through a worker: the sheet in flight finished, front and back, none fed
//! after, and the worker in step for the next batch; and a caller whose receiver is gone.

mod support;

use std::{
    sync::mpsc::{self, Receiver},
    thread::{self, JoinHandle},
};

use scan_worker::scan_device::in_progress::Tracker;
use scan_worker::scan_device::{Done, Event, Format, Settings, Source};
use scan_worker::{Error, RemoteDevice};

use support::{AT_MOST, client_of, open, outline, pages_are_pattern_s, scan, settings, soon};

/// Duplex in gray at 100 dpi: Letter is 850 by 1100 pixels, a byte each.
fn gray() -> Settings {
    settings(Source::Feeder, 100, Format::Gray8, true)
}

/// A page's bytes, at [`gray`]'s settings.
const PAGE: u64 = 850 * 1100;

/// A batch running on a thread of its own: the device, and how the batch ended, once it has.
type Running = JoinHandle<(RemoteDevice, Result<Done, Error>)>;

/// A batch through `device` from sheet 1 on a thread of its own, counted on `tracker`: its
/// events as they come, and the batch.
fn batch(mut device: RemoteDevice, tracker: &Tracker) -> (Receiver<Event>, Running) {
    let (events, received) = mpsc::sync_channel(64);
    let tracker = tracker.clone();
    let running = thread::spawn(move || {
        let ended = device.scan(&gray(), 1, &events, &tracker);
        (device, ended)
    });
    (received, running)
}

/// Events from `received` up to and with `last`.
fn up_to(received: &Receiver<Event>, last: &str) -> Vec<String> {
    let mut taken = Vec::new();
    while taken.last().map(String::as_str) != Some(last) {
        let event = received.recv_timeout(AT_MOST).expect("an event");
        taken.extend(outline(&[event]));
    }
    taken
}

/// After a cancelled batch, the next one through `device`, from sheet 3: the two sheets left in
/// the feeder, so the cancelled batch fed none it didn't give.
fn the_rest(mut device: RemoteDevice) {
    let (events, ended) = scan(&mut device, &gray(), 3);
    assert_eq!(ended.expect("the next batch"), Done::FeederEmpty);
    assert_eq!(
        outline(&events),
        [
            "applied",
            "sheet 3",
            "page 3 front",
            "page 3 back",
            "sheet 4",
            "page 4 front",
            "page 4 back"
        ]
    );
    pages_are_pattern_s(&events);
    device.close().expect("closed");
}

#[test]
fn a_cancel_midway_through_a_front_finishes_its_sheet_and_feeds_no_more() {
    // Four sheets; sheet 2's front holds midway until the batch is cancelled.
    let device = open(&client_of("await-cancel-midway", 4));
    let tracker = Tracker::new();
    let (received, running) = batch(device, &tracker);
    assert_eq!(
        up_to(&received, "sheet 2"),
        [
            "applied",
            "sheet 1",
            "page 1 front",
            "page 1 back",
            "sheet 2"
        ]
    );
    // The caller's tracker stands at half sheet 2's front: the cancel lands mid-page.
    assert!(
        soon(|| {
            let snapshot = tracker.snapshot();
            snapshot.total == Some(PAGE) && snapshot.done == PAGE / 2
        }),
        "{:?}",
        tracker.snapshot()
    );
    tracker.cancel();
    let after: Vec<Event> = received.iter().collect();
    let (device, ended) = running.join().expect("the batch");
    assert_eq!(ended.expect("a batch"), Done::Cancelled);
    // Sheet 2 whole, front and back; no sheet after it.
    assert_eq!(outline(&after), ["page 2 front", "page 2 back"]);
    pages_are_pattern_s(&after);
    the_rest(device);
}

#[test]
fn a_cancel_after_a_duplex_front_still_delivers_its_back() {
    // Four sheets; sheet 2's back holds as it starts until the batch is cancelled.
    let device = open(&client_of("await-cancel-back", 4));
    let tracker = Tracker::new();
    let (received, running) = batch(device, &tracker);
    assert_eq!(
        up_to(&received, "page 2 front"),
        [
            "applied",
            "sheet 1",
            "page 1 front",
            "page 1 back",
            "sheet 2",
            "page 2 front"
        ]
    );
    tracker.cancel();
    let after: Vec<Event> = received.iter().collect();
    let (device, ended) = running.join().expect("the batch");
    assert_eq!(ended.expect("a batch"), Done::Cancelled);
    assert_eq!(outline(&after), ["page 2 back"]);
    pages_are_pattern_s(&after);
    the_rest(device);
}

#[test]
fn a_receiver_gone_mid_batch_cancels_it_and_the_worker_goes_on() {
    let sheets = 8;
    let mut device = open(&client_of("feeder", sheets));
    // A rendezvous channel, whose receiver takes the first page and goes.
    let (events, received) = mpsc::sync_channel(0);
    let caller = thread::spawn(move || {
        let mut taken = Vec::new();
        while taken.len() < 3 {
            taken.push(received.recv().expect("an event"));
        }
        taken
    });
    let ended = device.scan(&gray(), 1, &events, &Tracker::new());
    drop(events);
    let taken = caller.join().expect("the caller");
    assert_eq!(outline(&taken), ["applied", "sheet 1", "page 1 front"]);
    assert_eq!(ended.expect("a batch"), Done::Cancelled);
    // The worker in step: the next batch takes what's left in the feeder, the sheet in flight
    // finished and none fed after the cancel landed.
    let (events, ended) = scan(&mut device, &gray(), 1);
    assert_eq!(ended.expect("the next batch"), Done::FeederEmpty);
    let left = outline(&events)
        .iter()
        .filter(|event| event.starts_with("sheet "))
        .count();
    assert!((1..sheets).contains(&left), "{:?}", outline(&events));
    assert_eq!(outline(&events).len(), 1 + 3 * left);
    pages_are_pattern_s(&events);
    device.close().expect("closed");
}
