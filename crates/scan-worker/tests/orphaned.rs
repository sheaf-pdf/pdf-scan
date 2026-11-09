//! A worker whose client has gone, spoken to directly, without a client: at the end of its
//! input it exits, and where a driver's call never returns, its abort ends it 10 seconds after.

mod support;

use std::{
    io::{BufRead, BufReader, Read, Write},
    process::{Child, ChildStdin, Command, ExitStatus, Stdio},
    sync::mpsc::{self, Receiver},
    thread,
    time::{Duration, Instant},
};

use scan_worker::scan_device::{Format, Source};
use serde_json::{Value, json};

use support::{AT_MOST, aborted, settings, stored};

/// How long a worker whose input has ended waits on a call that hasn't returned before it
/// aborts: `serve`'s.
const ORPHAN_GRACE: Duration = Duration::from_secs(10);

/// What the abort may take past it, on a slow or busy system.
const MARGIN: Duration = Duration::from_secs(10);

/// The test worker, started by hand.
struct Direct {
    child: Child,
    input: Option<ChildStdin>,
    /// Each message it writes, a page's bytes skipped.
    messages: Receiver<Value>,
}

impl Direct {
    /// The test worker running `scenario` with `count` sheets, its `hello` taken.
    fn start(scenario: &str, count: usize) -> Direct {
        let mut child = Command::new(env!("CARGO_BIN_EXE_scan-worker-fake"))
            .args([scenario, &count.to_string()])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("the worker started");
        let input = child.stdin.take();
        let output = BufReader::new(child.stdout.take().expect("its output"));
        let (sent, messages) = mpsc::channel();
        thread::spawn(move || read_messages(output, &sent));
        let worker = Direct {
            child,
            input,
            messages,
        };
        let hello = worker.next();
        assert_eq!(hello["hello"]["protocol"], 1, "{hello}");
        worker
    }

    /// `request` written on a line of its own.
    fn send(&mut self, request: &Value) {
        let input = self.input.as_mut().expect("its input open");
        writeln!(input, "{request}")
            .and_then(|()| input.flush())
            .expect("the request written");
    }

    /// Its next message.
    fn next(&self) -> Value {
        self.messages.recv_timeout(AT_MOST).expect("a message")
    }

    /// Its messages, up to the first `last` holds.
    fn until(&self, last: impl Fn(&Value) -> bool) {
        while !last(&self.next()) {}
    }

    /// The stored device opened.
    fn open(&mut self) {
        self.send(&json!({"open": {"device": stored(), "deadline": AT_MOST}}));
        let opened = self.next();
        assert!(opened.get("opened").is_some(), "{opened}");
    }

    /// Its input ended, as at its client's end: when.
    fn end_input(&mut self) -> Instant {
        let ended = Instant::now();
        drop(self.input.take());
        ended
    }

    /// How it exited, waiting at most `limit`; `None` where it hasn't, and is killed.
    fn exit(&mut self, limit: Duration) -> Option<ExitStatus> {
        let start = Instant::now();
        while start.elapsed() < limit {
            if let Some(status) = self.child.try_wait().expect("its status") {
                return Some(status);
            }
            thread::sleep(Duration::from_millis(10));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
        None
    }
}

impl Drop for Direct {
    /// A worker a failed test leaves is killed, and reaped.
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// `output`'s messages sent on `sent`, each page's bytes read past, until it ends.
fn read_messages(mut output: impl BufRead, sent: &mpsc::Sender<Value>) {
    let mut line = Vec::new();
    loop {
        line.clear();
        match output.read_until(b'\n', &mut line) {
            Ok(0) | Err(_) => return,
            Ok(_) => {}
        }
        let Ok(message) = serde_json::from_slice::<Value>(&line) else {
            return;
        };
        if let Some(bytes) = message["page"]["bytes"].as_u64() {
            let skipped = std::io::copy(&mut (&mut output).take(bytes), &mut std::io::sink());
            if skipped.ok() != Some(bytes) {
                return;
            }
        }
        if sent.send(message).is_err() {
            return;
        }
    }
}

#[test]
fn a_worker_whose_input_ends_while_idle_exits_with_success() {
    // With no device, and with one open.
    for opens in [false, true] {
        let mut worker = Direct::start("feeder", 1);
        if opens {
            worker.open();
        }
        worker.end_input();
        let status = worker.exit(AT_MOST).expect("exited");
        assert_eq!(status.code(), Some(0), "opens: {opens}");
    }
}

#[test]
fn a_worker_whose_input_ends_in_a_driver_s_hang_aborts_after_its_grace() {
    let mut worker = Direct::start("hang", 5);
    worker.open();
    worker.send(&json!({"scan": {
        "settings": settings(Source::Feeder, 100, Format::Gray8, true),
        "first_sheet": 1,
    }}));
    // The driver hangs midway through sheet 3's front: half its 850 by 1100 bytes counted.
    worker.until(|message| message["sheet"] == 3);
    worker.until(|message| message["progress"]["done"] == 850 * 550);
    let ended = worker.end_input();
    let status = worker
        .exit(ORPHAN_GRACE + MARGIN)
        .expect("ended within the grace and the margin");
    let after = ended.elapsed();
    assert!(aborted(&status), "{status}");
    // Never before the grace: its call might yet have returned.
    assert!(after >= ORPHAN_GRACE, "{after:?}");
}
