//! The TWAIN thread: an apartment, a window and a session of its own, every
//! DSM call made on it, its events sent to the main thread on a channel.

use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::{self, JoinHandle};

use crate::scenarios::Scenario;
use crate::session::Session;
use crate::window::{Apartment, Window, thread_id};

/// What the TWAIN thread tells the main thread.
pub enum Event {
    /// The thread's window exists: requests are posted to it.
    Ready { window: usize, thread: u32 },
    /// Something seen, for the report.
    Line(String),
    /// Something that wasn't as expected.
    Fail(String),
    /// Paused within page `page`, waiting for a request posted to the window.
    Paused { page: usize },
    /// The scenario is over, in this TWAIN state (1 if it walked all the way
    /// down).
    Finished { state: u8 },
}

/// The TWAIN thread's side of the channel.
#[derive(Clone)]
pub struct Out(Sender<Event>);

impl Out {
    pub fn line(&self, text: impl Into<String>) {
        self.send(Event::Line(text.into()));
    }

    pub fn fail(&self, text: impl Into<String>) {
        self.send(Event::Fail(text.into()));
    }

    pub fn paused(&self, page: usize) {
        self.send(Event::Paused { page });
    }

    fn send(&self, event: Event) {
        // The main thread outlives every scenario; if it's gone there's no one
        // to tell.
        let _ = self.0.send(event);
    }
}

/// Starts a TWAIN thread running `scenario`.
pub fn spawn(scenario: &'static Scenario) -> std::io::Result<(JoinHandle<()>, Receiver<Event>)> {
    let (tx, rx) = mpsc::channel();
    let handle = thread::Builder::new()
        .name("twain".to_owned())
        .spawn(move || run(scenario, Out(tx)))?;
    Ok((handle, rx))
}

fn run(scenario: &'static Scenario, out: Out) {
    let state = match (Apartment::enter(), Window::create()) {
        (Ok(apartment), Ok(window)) => {
            out.send(Event::Ready {
                window: window.hwnd() as usize,
                thread: thread_id(),
            });
            let mut session = Session::new(out.clone(), window.hwnd());
            (scenario.run)(&mut session);
            let state = session.state();
            // The session walks down what's left before the window goes.
            drop(session);
            drop(window);
            drop(apartment);
            state
        }
        (Err(e), _) | (_, Err(e)) => {
            out.fail(e);
            1
        }
    };
    out.send(Event::Finished { state });
}
