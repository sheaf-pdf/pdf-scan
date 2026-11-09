//! States dropped or forgotten anywhere, cancels, and requests: each state
//! steps down from where the source really is, a state forgotten is stepped
//! down at the end of the run, and the sample scans again after.

#![cfg(windows)]

mod support;

use std::time::Duration;

use support::Outcome;
use twain_dsm::{
    Delivery, Enabled, Error, Item, Native, Next, Opened, Pending, Ready, SetOutcome, Source,
    SourceId, Twain, Ui, ffi,
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
            Pending::More(next, _) => ready = next,
            Pending::Done(enabled) => break enabled,
        }
    };
    let source = enabled
        .disable()
        .map_err(|r| r.into_error())
        .expect("MSG_DISABLEDS");
    in_state_4(&source);
    (
        pages,
        source
            .close()
            .map_err(|r| r.into_error())
            .expect("MSG_CLOSEDS"),
    )
}

/// The source is in state 4: a capability set there is taken, where an
/// enabled source refuses it (`TWCC_SEQERROR`).
fn in_state_4(source: &Source<'_, u32>) {
    let outcome = source
        .set(ffi::CAP_INDICATORS, Item::Bool(false))
        .unwrap_or_else(|e| panic!("MSG_SET in state 4: {e}"));
    assert_eq!(outcome, SetOutcome::Exact);
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
                        Pending::More(ready, _) => ready,
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
                    in_state_4(&source);
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
                        Pending::More(next, _) => ready = next,
                        Pending::Done(enabled) => break enabled,
                    }
                };
                // The feeder empty, the source enabled: nothing more comes
                // from it, and a request does.
                let remote = twain.remote();
                std::thread::spawn(move || remote.post(7).expect("posted"));
                let started = std::time::Instant::now();
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
                assert!(
                    started.elapsed() < Duration::from_secs(2),
                    "the request took {:?}",
                    started.elapsed()
                );
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

/// Another message loop running on the TWAIN thread after the enable (a
/// driver's dialog, a message box: here, one that takes and dispatches every
/// message for 300 ms) loses nothing: the source's `MSG_XFERREADY` is handed
/// over all the same, at once, in either delivery.
#[test]
fn another_message_loop_on_the_thread_loses_nothing() {
    forked!(
        another_message_loop_on_the_thread_loses_nothing,
        PAGES,
        || {
            twain_dsm::run::<u32, _>(|twain| {
                for delivery in [Delivery::Callback, Delivery::Events] {
                    let (opened, sample) = match connect(twain) {
                        Ok(c) => c,
                        Err(outcome) => return outcome,
                    };
                    let enabled = enable_batch(opened, &sample, delivery, ffi::TWSX_MEMORY);
                    foreign_loop(Duration::from_millis(300));
                    let started = std::time::Instant::now();
                    let ready = next_ready(enabled);
                    assert!(
                        started.elapsed() < Duration::from_secs(2),
                        "{delivery:?}: the transfer ready took {:?}",
                        started.elapsed()
                    );
                    drop(ready);
                    assert!(twain.drop_errors().is_empty());
                }
                Outcome::Done
            })
            .expect("TWAIN runs")
            .value
        }
    );
}

/// A loop of the thread's own, as a modal dialog runs: every message taken
/// and dispatched, for `time`.
fn foreign_loop(time: Duration) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        DispatchMessageW, MSG, PM_REMOVE, PeekMessageW, TranslateMessage,
    };
    let until = std::time::Instant::now() + time;
    while std::time::Instant::now() < until {
        // SAFETY: an all-zero MSG is a valid value of it.
        let mut msg: MSG = unsafe { core::mem::zeroed() };
        // SAFETY: `msg` is writable; any of this thread's messages.
        while unsafe { PeekMessageW(&mut msg, core::ptr::null_mut(), 0, 0, PM_REMOVE) } != 0 {
            // SAFETY: a message just taken on this thread.
            unsafe {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// A `MSG_XFERREADY` from an enable disabled before it was handed over is no
/// transfer of the next enable: after that enable's batch, nothing more is
/// ready, in either delivery.
#[test]
fn a_ready_from_an_earlier_enable_is_stale() {
    forked!(a_ready_from_an_earlier_enable_is_stale, PAGES, || {
        twain_dsm::run::<u32, _>(|twain| {
            for delivery in [Delivery::Callback, Delivery::Events] {
                let (opened, sample) = match connect(twain) {
                    Ok(c) => c,
                    Err(outcome) => return outcome,
                };
                // Enabled, and disabled at once: the source said its
                // transfer was ready, and nobody took it.
                let source = enable_batch(opened, &sample, delivery, ffi::TWSX_MEMORY)
                    .disable()
                    .map_err(|r| r.into_error())
                    .expect("MSG_DISABLEDS, after MSG_RESET");
                in_state_4(&source);
                // Enabled again: its batch, whole, and then nothing.
                let mut ready = next_ready(
                    source
                        .enable(Ui::Hidden)
                        .map_err(|r| r.into_error())
                        .expect("enables"),
                );
                let mut pages = 1;
                let mut enabled = loop {
                    match memory_page(ready) {
                        Pending::More(next, _) => {
                            ready = next;
                            pages += 1;
                        }
                        Pending::Done(enabled) => break enabled,
                    }
                };
                assert_eq!(pages, 3, "{delivery:?}");
                loop {
                    match enabled
                        .next_event(Duration::from_millis(500))
                        .map_err(|r| r.into_error())
                        .expect("next")
                    {
                        Next::Timeout(_) => break,
                        Next::Event(back, _) | Next::Request(back, _) | Next::Quit(back, _) => {
                            enabled = back
                        }
                        Next::Ready(_) => {
                            panic!("{delivery:?}: a stale transfer ready, in state 5")
                        }
                    }
                }
                assert!(twain.drop_errors().is_empty());
            }
            Outcome::Done
        })
        .expect("TWAIN runs")
        .value
    });
}

/// TWAIN's rules for a memory transfer's buffers are checked before the
/// source sees one: a buffer off a 4-byte boundary, of odd length, of
/// another length than the first, larger than the source's largest.
#[test]
fn a_buffer_against_the_rules_is_refused_before_it_is_sent() {
    forked!(
        a_buffer_against_the_rules_is_refused_before_it_is_sent,
        PAGES,
        || {
            twain_dsm::run::<u32, _>(|twain| {
                let (opened, sample) = match connect(twain) {
                    Ok(c) => c,
                    Err(outcome) => return outcome,
                };
                let ready = next_ready(enable_batch(
                    opened,
                    &sample,
                    Delivery::Callback,
                    ffi::TWSX_MEMORY,
                ));
                let mut transfer = ready
                    .memory()
                    .map_err(|r| r.into_error())
                    .expect("DAT_SETUPMEMXFER");
                let setup = transfer.setup();
                let (min, max) = (setup.MinBufSize, setup.MaxBufSize);
                let size = transfer.preferred_size();
                assert_eq!(size % 4, 0, "preferred {size}");
                let mut buffer = vec![0u8; size + 8];
                let misaligned = &mut buffer[1..size + 1];
                assert!(matches!(
                    transfer.next_buffer(misaligned),
                    Err(Error::BufferMisaligned { .. })
                ));
                if size > min as usize {
                    assert!(matches!(
                        transfer.next_buffer(&mut buffer[..size - 1]),
                        Err(Error::BufferMisaligned { .. })
                    ));
                }
                assert!(
                    transfer
                        .next_buffer(&mut buffer[..size])
                        .expect("a buffer")
                        .is_some()
                );
                if size >= min as usize + 4 {
                    assert!(matches!(
                        transfer.next_buffer(&mut buffer[..size - 4]),
                        Err(Error::BufferSizeChanged { .. })
                    ));
                }
                if max != 0 && max != ffi::TWON_DONTCARE32 {
                    let mut large = vec![0u8; max as usize + 4];
                    assert!(matches!(
                        transfer.next_buffer(&mut large),
                        Err(Error::BufferTooLarge { .. })
                    ));
                }
                while transfer
                    .next_buffer(&mut buffer[..size])
                    .expect("a buffer")
                    .is_some()
                {}
                drop(
                    transfer
                        .end()
                        .map_err(|r| r.into_error())
                        .expect("MSG_ENDXFER"),
                );
                assert!(twain.drop_errors().is_empty());
                Outcome::Done
            })
            .expect("TWAIN runs")
            .value
        }
    );
}
