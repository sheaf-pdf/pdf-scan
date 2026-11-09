//! States dropped or forgotten anywhere, cancels, and requests: each state
//! steps down from where the source really is, a state forgotten is stepped
//! down at the end of the run, and the sample scans again after.

#![cfg(windows)]

mod support;

use std::time::Duration;

use support::Outcome;
use twain_dsm::{
    Delivery, Enabled, Item, Native, Next, Opened, Pending, Ready, SourceId, Twain, Ui, ffi,
};

/// The sample's feeder, through the environment the DSM loads it in.
const PAGES: &[(&str, &str)] = &[("CAP_XFERCOUNT", "3")];

/// The source opened, set for a batch of all the feeder has, and enabled.
fn enable_batch<'t>(
    opened: Opened<'t, u32>,
    sample: &SourceId,
    delivery: Delivery,
    mech: u16,
) -> Enabled<'t, u32> {
    let source = opened
        .open_source(sample, delivery)
        .map_err(|r| r.into_error())
        .expect("the sample opens");
    for (cap, item) in [
        (ffi::ICAP_XFERMECH, Item::UInt16(mech)),
        (ffi::CAP_XFERCOUNT, Item::Int16(-1)),
        (ffi::CAP_INDICATORS, Item::Bool(false)),
    ] {
        source
            .set(cap, item)
            .unwrap_or_else(|e| panic!("MSG_SET {cap:#06x}: {e}"));
    }
    source
        .enable(Ui::Hidden)
        .map_err(|r| r.into_error())
        .expect("the sample enables")
}

fn next_ready(mut enabled: Enabled<'_, u32>) -> Ready<'_, u32> {
    loop {
        match enabled
            .next_event(Duration::from_secs(20))
            .map_err(|r| r.into_error())
            .expect("an event")
        {
            Next::Ready(ready) => return ready,
            Next::Event(_, event) => panic!("the sample sent {event:?}, not MSG_XFERREADY"),
            Next::Timeout(_) => panic!("no MSG_XFERREADY in 20 seconds"),
            Next::Request(_, n) => panic!("request {n} before the transfer ready"),
            Next::Quit(back, _) => enabled = back,
        }
    }
}

/// One page through memory, the buffers thrown away.
fn memory_page(ready: Ready<'_, u32>) -> Pending<'_, u32> {
    let mut transfer = ready
        .memory()
        .map_err(|r| r.into_error())
        .expect("DAT_SETUPMEMXFER");
    let mut buffer = vec![0; transfer.preferred_size()];
    while transfer
        .next_buffer(&mut buffer)
        .expect("DAT_IMAGEMEMXFER")
        .is_some()
    {}
    transfer
        .end()
        .map_err(|r| r.into_error())
        .expect("MSG_ENDXFER")
}

/// A whole batch, its page count; the source back in state 4, then 3.
fn scan<'t>(
    opened: Opened<'t, u32>,
    sample: &SourceId,
    delivery: Delivery,
) -> (usize, Opened<'t, u32>) {
    let mut ready = next_ready(enable_batch(opened, sample, delivery, ffi::TWSX_MEMORY));
    let mut pages = 0;
    let enabled = loop {
        pages += 1;
        match memory_page(ready) {
            Pending::More(next) => ready = next,
            Pending::Done(enabled) => break enabled,
        }
    };
    let source = enabled
        .disable()
        .map_err(|r| r.into_error())
        .expect("MSG_DISABLEDS");
    (
        pages,
        source
            .close()
            .map_err(|r| r.into_error())
            .expect("MSG_CLOSEDS"),
    )
}

/// Opens the system's DSM and finds the sample, or why not.
fn connect(twain: &Twain<u32>) -> Result<(Opened<'_, u32>, SourceId), Outcome> {
    let opened = support::open(twain)?;
    let sample = support::sample(&opened)?;
    Ok((opened, sample))
}

/// Where a state is dropped or forgotten.
#[derive(Clone, Copy, Debug)]
enum Point {
    /// Right after `MSG_ENABLEDS`, before the pump has seen the source's
    /// `MSG_XFERREADY`: the record says 5, the source is in 6.
    Enabled,
    /// A transfer ready.
    Ready,
    /// A native transfer done, not ended.
    Transferred,
    /// A memory transfer begun, no buffer taken.
    MemoryBegun,
    /// A memory transfer after its first buffer: state 7.
    MemoryMidway,
}

const POINTS: [Point; 5] = [
    Point::Enabled,
    Point::Ready,
    Point::Transferred,
    Point::MemoryBegun,
    Point::MemoryMidway,
];

/// Any state, boxed, to drop or forget whole.
trait State {}
impl<S> State for S {}

/// The sample taken to `point`, and the state there handed to `leave`
/// (dropped or forgotten), with the chain beneath it down to the connection.
fn to_point<'t>(
    opened: Opened<'t, u32>,
    sample: &SourceId,
    delivery: Delivery,
    point: Point,
    leave: impl FnOnce(Box<dyn State + 't>),
) {
    let mech = match point {
        Point::Transferred => ffi::TWSX_NATIVE,
        _ => ffi::TWSX_MEMORY,
    };
    let enabled = enable_batch(opened, sample, delivery, mech);
    if let Point::Enabled = point {
        return leave(Box::new(enabled));
    }
    let ready = next_ready(enabled);
    match point {
        Point::Ready => leave(Box::new(ready)),
        Point::Transferred => match ready.native().map_err(|r| r.into_error()).expect("native") {
            Native::Done(dib, transferred) => {
                drop(dib);
                leave(Box::new(transferred))
            }
            Native::Cancelled(_) => panic!("the sample cancelled"),
        },
        Point::MemoryBegun => leave(Box::new(
            ready.memory().map_err(|r| r.into_error()).expect("memory"),
        )),
        Point::MemoryMidway => {
            let mut transfer = ready.memory().map_err(|r| r.into_error()).expect("memory");
            let mut buffer = vec![0; transfer.preferred_size()];
            assert!(
                transfer
                    .next_buffer(&mut buffer)
                    .expect("a buffer")
                    .is_some()
            );
            leave(Box::new(transfer))
        }
        Point::Enabled => unreachable!(),
    }
}

#[test]
fn a_state_dropped_anywhere_steps_down_from_where_the_source_is() {
    forked!(
        a_state_dropped_anywhere_steps_down_from_where_the_source_is,
        PAGES,
        || {
            twain_dsm::run::<u32, _>(|twain| {
                for delivery in [Delivery::Callback, Delivery::Events] {
                    for point in POINTS {
                        let (opened, sample) = match connect(twain) {
                            Ok(c) => c,
                            Err(outcome) => return outcome,
                        };
                        // The state owns the chain down to the connection, so the
                        // drop walks it all the way down.
                        to_point(opened, &sample, delivery, point, drop);
                        let errors = twain.drop_errors();
                        assert!(errors.is_empty(), "{delivery:?}, {point:?}: {errors:?}");
                        // And the sample scans whole after.
                        let (opened, sample) =
                            connect(twain).unwrap_or_else(|_| panic!("connects again"));
                        let (pages, opened) = scan(opened, &sample, delivery);
                        assert_eq!(pages, 3, "{delivery:?}, after {point:?}");
                        drop(opened);
                    }
                }
                Outcome::Done
            })
            .expect("TWAIN runs")
            .value
        }
    );
}

#[test]
fn a_state_forgotten_is_stepped_down_at_the_end_of_the_run() {
    forked!(
        a_state_forgotten_is_stepped_down_at_the_end_of_the_run,
        PAGES,
        || {
            for delivery in [Delivery::Callback, Delivery::Events] {
                for point in POINTS {
                    let report = twain_dsm::run::<u32, _>(|twain| {
                        let (opened, sample) = connect(twain)?;
                        to_point(opened, &sample, delivery, point, std::mem::forget);
                        Ok(())
                    })
                    .expect("TWAIN runs");
                    if let Err(outcome) = report.value {
                        return outcome;
                    }
                    assert!(
                        report.source_left.is_none(),
                        "{delivery:?}, {point:?}: {:?}",
                        report.source_left
                    );
                    assert!(
                        report.stranded.is_none(),
                        "{delivery:?}, {point:?}: {:?}",
                        report.stranded
                    );
                    // A second run, in the same process, scans whole.
                    twain_dsm::run::<u32, _>(|twain| {
                        let (opened, sample) =
                            connect(twain).unwrap_or_else(|_| panic!("connects"));
                        let (pages, _) = scan(opened, &sample, delivery);
                        assert_eq!(pages, 3, "{delivery:?}, after {point:?} forgotten");
                    })
                    .expect("TWAIN runs again");
                }
            }
            Outcome::Done
        }
    );
}

#[test]
fn a_cancel_stops_the_batch_at_the_next_page_or_before_the_first() {
    forked!(
        a_cancel_stops_the_batch_at_the_next_page_or_before_the_first,
        PAGES,
        || {
            twain_dsm::run::<u32, _>(|twain| {
                for delivery in [Delivery::Callback, Delivery::Events] {
                    // During page 1, from another thread: the page ends, then the
                    // batch.
                    let (opened, sample) = match connect(twain) {
                        Ok(c) => c,
                        Err(outcome) => return outcome,
                    };
                    let ready =
                        next_ready(enable_batch(opened, &sample, delivery, ffi::TWSX_MEMORY));
                    let mut transfer = ready.memory().map_err(|r| r.into_error()).expect("memory");
                    let mut buffer = vec![0; transfer.preferred_size()];
                    assert!(
                        transfer
                            .next_buffer(&mut buffer)
                            .expect("a buffer")
                            .is_some()
                    );
                    let remote = twain.remote();
                    std::thread::spawn(move || remote.post(1).expect("posted"))
                        .join()
                        .expect("posted");
                    while transfer
                        .next_buffer(&mut buffer)
                        .expect("a buffer")
                        .is_some()
                    {}
                    let ready = match transfer
                        .end()
                        .map_err(|r| r.into_error())
                        .expect("MSG_ENDXFER")
                    {
                        Pending::More(ready) => ready,
                        Pending::Done(_) => {
                            panic!("{delivery:?}: no pages left after the first of 3")
                        }
                    };
                    assert_eq!(twain.take_request(), Some(1), "{delivery:?}: the cancel");
                    let enabled = ready
                        .reset()
                        .map_err(|r| r.into_error())
                        .expect("MSG_RESET");
                    let source = enabled
                        .disable()
                        .map_err(|r| r.into_error())
                        .expect("MSG_DISABLEDS");
                    let opened = source
                        .close()
                        .map_err(|r| r.into_error())
                        .expect("MSG_CLOSEDS");

                    // Before the enable: no page at all.
                    let remote = twain.remote();
                    std::thread::spawn(move || remote.post(2).expect("posted"))
                        .join()
                        .expect("posted");
                    // The source's message comes first, the request after.
                    let ready =
                        next_ready(enable_batch(opened, &sample, delivery, ffi::TWSX_MEMORY));
                    assert_eq!(
                        twain.take_request(),
                        Some(2),
                        "{delivery:?}: the early cancel"
                    );
                    let enabled = ready
                        .reset()
                        .map_err(|r| r.into_error())
                        .expect("MSG_RESET");
                    drop(enabled);
                    assert!(twain.drop_errors().is_empty());
                }
                Outcome::Done
            })
            .expect("TWAIN runs")
            .value
        }
    );
}

#[test]
fn requests_come_through_next_event() {
    forked!(requests_come_through_next_event, PAGES, || {
        twain_dsm::run::<u32, _>(|twain| {
            for delivery in [Delivery::Callback, Delivery::Events] {
                let (opened, sample) = match connect(twain) {
                    Ok(c) => c,
                    Err(outcome) => return outcome,
                };
                let mut ready =
                    next_ready(enable_batch(opened, &sample, delivery, ffi::TWSX_MEMORY));
                let mut enabled = loop {
                    match memory_page(ready) {
                        Pending::More(next) => ready = next,
                        Pending::Done(enabled) => break enabled,
                    }
                };
                // The feeder empty, the source enabled: nothing more comes
                // from it, and a request does.
                let remote = twain.remote();
                std::thread::spawn(move || remote.post(7).expect("posted"));
                let got = loop {
                    match enabled
                        .next_event(Duration::from_secs(10))
                        .map_err(|r| r.into_error())
                        .expect("next")
                    {
                        Next::Request(back, n) => {
                            enabled = back;
                            break n;
                        }
                        Next::Event(back, event) => {
                            println!("{delivery:?}: {event:?} after the batch");
                            enabled = back;
                        }
                        Next::Ready(_) => {
                            panic!("{delivery:?}: a transfer ready with the feeder empty")
                        }
                        Next::Timeout(_) => panic!("{delivery:?}: no request in 10 seconds"),
                        Next::Quit(..) => panic!("WM_QUIT"),
                    }
                };
                assert_eq!(got, 7);
                match enabled
                    .next_event(Duration::from_millis(200))
                    .map_err(|r| r.into_error())
                    .expect("next")
                {
                    Next::Timeout(back) | Next::Event(back, _) => enabled = back,
                    _ => panic!("{delivery:?}: neither a timeout nor an event"),
                }
                drop(enabled);
                assert!(twain.drop_errors().is_empty());
            }
            Outcome::Done
        })
        .expect("TWAIN runs")
        .value
    });
}
