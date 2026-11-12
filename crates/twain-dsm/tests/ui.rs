//! The source's own window, shown: run by hand, in a desktop session
//! (`cargo test -p twain-dsm --test ui -- --ignored --nocapture`). Each
//! waits `TWAIN_DSM_UI_SECONDS` (5 if unset) for the user, prints what came,
//! and disables the source, which closes the window whatever the user did.

#![cfg(windows)]

mod support;

use std::time::{Duration, Instant};

use support::Outcome;
use twain_dsm::{Container, Delivery, Enabled, Event, Item, Next, Ui, ffi};

fn seconds() -> Duration {
    let n = std::env::var("TWAIN_DSM_UI_SECONDS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(5);
    Duration::from_secs(n)
}

/// Waits for the user, printing each event; the Enabled back at the end.
fn watch(mut enabled: Enabled<'_, ()>) -> Enabled<'_, ()> {
    let deadline = Instant::now() + seconds();
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        match enabled
            .next_event(left)
            .map_err(|r| r.into_error())
            .expect("an event")
        {
            Next::Ready(ready) => {
                println!("MSG_XFERREADY: the user asked for a scan; the batch dropped");
                return ready
                    .reset()
                    .map_err(|r| r.into_error())
                    .expect("MSG_RESET");
            }
            Next::Event(back, event) => {
                println!("{event:?}");
                if matches!(event, Event::CloseRequest | Event::CloseOk) {
                    return back;
                }
                enabled = back;
            }
            Next::Timeout(back) => {
                println!("no event in {:?}", seconds());
                return back;
            }
            Next::Request(back, ()) | Next::Quit(back, _) => enabled = back,
        }
    }
}

#[test]
#[ignore = "shows the sample's window: run by hand, in a desktop session"]
fn the_sources_window_shows_and_closes() {
    forked!(the_sources_window_shows_and_closes, &[], || {
        twain_dsm::run::<(), _>(|twain| {
            let mut opened = match support::open(twain) {
                Ok(opened) => opened,
                Err(outcome) => return outcome,
            };
            let sample = match support::sample(&opened) {
                Ok(sample) => sample,
                Err(outcome) => return outcome,
            };
            for delivery in [Delivery::Events, Delivery::Callback] {
                let source = opened
                    .open_source(&sample, delivery)
                    .map_err(|r| r.into_error())
                    .expect("opens");
                let enabled = source
                    .enable(Ui::Shown)
                    .map_err(|r| r.into_error())
                    .expect("enabled, shown");
                println!("{delivery:?}: the window is up");
                let source = watch(enabled)
                    .disable()
                    .map_err(|r| r.into_error())
                    .expect("MSG_DISABLEDS");
                opened = source
                    .close()
                    .map_err(|r| r.into_error())
                    .expect("MSG_CLOSEDS");
            }
            Outcome::Done
        })
        .expect("TWAIN runs")
        .value
    });
}

#[test]
#[ignore = "shows the sample's settings window: run by hand, in a desktop session"]
fn the_settings_window_shows_and_closes() {
    forked!(the_settings_window_shows_and_closes, &[], || {
        twain_dsm::run::<(), _>(|twain| {
            let opened = match support::open(twain) {
                Ok(opened) => opened,
                Err(outcome) => return outcome,
            };
            let sample = match support::sample(&opened) {
                Ok(sample) => sample,
                Err(outcome) => return outcome,
            };
            let source = opened
                .open_source(&sample, Delivery::Events)
                .map_err(|r| r.into_error())
                .expect("opens");
            match source.get_current(ffi::CAP_ENABLEDSUIONLY) {
                Ok(Container::One(Item::Bool(true))) => {}
                other => panic!("CAP_ENABLEDSUIONLY: {other:?}"),
            }
            let enabled = source
                .settings_ui(None)
                .map_err(|r| r.into_error())
                .expect("MSG_ENABLEDSUIONLY");
            assert!(enabled.settings_only());
            println!("the settings window is up");
            drop(
                watch(enabled)
                    .disable()
                    .map_err(|r| r.into_error())
                    .expect("MSG_DISABLEDS"),
            );
            Outcome::Done
        })
        .expect("TWAIN runs")
        .value
    });
}
