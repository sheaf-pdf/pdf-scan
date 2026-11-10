//! The test worker: a worker serving the scripted backend, and workers that break the protocol on
//! purpose, for this crate's tests alone. Built only with the `fake` feature, never for a
//! shipped build: its scenarios hang, crash and exit by design.
//!
//! Its first argument names a scenario, a second a count of sheets where one takes it (3 if
//! none). Through `serve`: those of `scenarios.rs`; `env`, a backend reporting where it was
//! started; `sane`, SANE's backend, with the arguments `sane.rs` gives; `slow-listing`, whose
//! listing takes a minute and whose exit is held; `slow-exit`, whose device opens at once and
//! whose exit is held, a minute or the seconds its second argument says. By hand, the protocol
//! written as literal lines: `version`, `later-hello`,
//! `not-json`, `no-hello`, `silent`, `deaf`, `long-line`, `out-of-place`, `refuses`,
//! `opened-another`, `renamed-elsewhere`, `no-source`, `big-page`, `page-length`, `padding`,
//! `zero-dpi`, `cut-short`, `page-then-end`, `page-first`, `applied-late`, `sheet-out-of-turn`,
//! `empty-after-page`, `encoded-unasked`, `huge-paper`, `helper-window` and `helper-batch`.
//! And `held`, the process the last two start: it holds the pipes it inherits until its input
//! ends.

#![forbid(unsafe_code)]

mod sane;
mod scenarios;

use std::{
    env, fs,
    io::{self, BufRead, Write},
    process::{Command, ExitCode},
    thread,
    time::Duration,
};

use sane_dl::CONFIG_DIR_VARIABLE;
use scan_device::fake::{FakeBackend, FakeDevice};
use scan_device::{Backend, DeviceId, DeviceInfo, Driver, Opened, ScanError};
use serde_json::{Value, json};

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    let name = args.first().map_or("", String::as_str);
    let count = args
        .get(1)
        .and_then(|count| count.parse().ok())
        .unwrap_or(3);
    if matches!(name, "hang" | "crash" | "exit") {
        eprintln!("{}", scenarios::FAILING);
    }
    if let Some(backend) = scenarios::backend(name, count) {
        return scan_worker::serve(vec![backend]);
    }
    match name {
        "env" => scan_worker::serve(vec![Box::new(Env)]),
        "sane" => sane::main(&args[1..]),
        "slow-listing" => {
            let backend = FakeBackend::new()
                .with(scenarios::desk())
                .listing_takes(MINUTE);
            scan_worker::serve(vec![Box::new(HeldExit(backend, MINUTE))])
        }
        "slow-exit" => {
            let backend = FakeBackend::new().with(scenarios::desk());
            let held = args
                .get(1)
                .and_then(|seconds| seconds.parse().ok())
                .map_or(MINUTE, Duration::from_secs);
            scan_worker::serve(vec![Box::new(HeldExit(backend, held))])
        }
        "held" => held(),
        _ => by_hand(name),
    }
}

/// A backend whose devices say where their worker was started: its current directory as their
/// name, `SANE_CONFIG_DIR` as their maker (`<unset>` where it isn't set), and as their model, in
/// JSON, what the directory holds and the value of [`GIVEN`] (`null` where it isn't set); SANE's,
/// so a client starts it as it starts a SANE worker. Opened, a device is the scripted one, with
/// an empty feeder.
struct Env;

/// The variable the `env` backend's devices report, which a test sets on the worker's command.
const GIVEN: &str = "SCAN_WORKER_TEST_GIVEN";

impl Backend for Env {
    fn list(&mut self, _deadline: Duration) -> Result<Vec<DeviceInfo>, ScanError> {
        Ok(vec![started(DeviceId("sane:env".to_owned()))])
    }

    fn open(&mut self, stored: &DeviceInfo, deadline: Duration) -> Result<Opened, ScanError> {
        let mut fake = FakeBackend::new().with(FakeDevice::new("env", "Maker", "Model"));
        let stand_in = fake.list(deadline)?.remove(0);
        let opened = fake.open(&stand_in, deadline)?;
        Ok(Opened {
            device: opened.device,
            info: started(stored.id.clone()),
            renamed_from: None,
        })
    }
}

/// The device `id`, saying where its worker was started.
fn started(id: DeviceId) -> DeviceInfo {
    let here = env::current_dir();
    let name = match &here {
        Ok(dir) => dir.to_string_lossy().into_owned(),
        Err(error) => format!("<no current directory: {error}>"),
    };
    let maker = env::var_os(CONFIG_DIR_VARIABLE).map_or_else(
        || "<unset>".to_owned(),
        |value| value.to_string_lossy().into_owned(),
    );
    let entries = |dir: &std::path::Path| -> Value {
        match fs::read_dir(dir) {
            Ok(entries) => {
                let mut names: Vec<String> = entries
                    .filter_map(Result::ok)
                    .map(|entry| entry.file_name().to_string_lossy().into_owned())
                    .collect();
                names.sort();
                json!(names)
            }
            Err(_) => Value::Null,
        }
    };
    let given = env::var_os(GIVEN).map(|value| value.to_string_lossy().into_owned());
    let model = match &here {
        Ok(dir) => json!({
            "entries": entries(dir),
            "dll_conf": fs::read_to_string(dir.join("dll.conf")).ok(),
            "dll_d": entries(&dir.join("dll.d")),
            "given": given,
        }),
        Err(_) => json!({ "given": given }),
    };
    DeviceInfo::new(id, name, maker, model.to_string(), Driver::Sane)
}

/// A minute.
const MINUTE: Duration = Duration::from_secs(60);

/// A backend whose drop takes as long as it says, so a worker asked to quit holds its exit that
/// long: a test sees whether its client killed it instead, or let it finish.
struct HeldExit(FakeBackend, Duration);

impl Drop for HeldExit {
    fn drop(&mut self) {
        thread::sleep(self.1);
    }
}

impl Backend for HeldExit {
    fn list(&mut self, deadline: Duration) -> Result<Vec<DeviceInfo>, ScanError> {
        self.0.list(deadline)
    }

    fn open(&mut self, device: &DeviceInfo, deadline: Duration) -> Result<Opened, ScanError> {
        self.0.open(device, deadline)
    }
}

/// A process a driver started, which inherits the worker's standard input, output and error and
/// holds them, until its input ends, as the client lets the worker's go; a minute at most.
fn held() -> ExitCode {
    thread::spawn(|| {
        thread::sleep(MINUTE);
        std::process::exit(0);
    });
    let _ = io::copy(&mut io::stdin().lock(), &mut io::sink());
    ExitCode::SUCCESS
}

/// The worker's driver's process, a `held` one, started with the worker's own standard input,
/// output and error.
fn start_held() {
    let started = env::current_exe().and_then(|me| {
        Command::new(me)
            .arg("held")
            // Not the worker's directory, which the client removes as the worker ends.
            .current_dir(env::temp_dir())
            .spawn()
    });
    if let Err(error) = started {
        eprintln!("scan-worker-fake: couldn't start the held process: {error}");
    }
}

/// The `hello` a worker of this protocol writes.
const HELLO: &str = r#"{"hello":{"protocol":1,"version":"0.0.0"}}"#;

/// A worker written by hand, as `name` says, then nothing more until its input ends.
fn by_hand(name: &str) -> ExitCode {
    let mut out = io::stdout().lock();
    let mut input = io::stdin().lock();
    let mut say = |line: &str, bytes: &[u8]| {
        // A client gone ends the scenario as its input's end does.
        let _ = out
            .write_all(line.as_bytes())
            .and_then(|()| out.write_all(b"\n"))
            .and_then(|()| out.write_all(bytes))
            .and_then(|()| out.flush());
    };
    match name {
        // Another protocol's.
        "version" => say(r#"{"hello":{"protocol":2,"version":"9.9.9"}}"#, &[]),
        // A later protocol's, which keeps its `protocol` alone.
        "later-hello" => say(
            r#"{"hello":{"protocol":2,"build":{"version":[9,9,9]}}}"#,
            &[],
        ),
        "not-json" => say("hello, client", &[]),
        // Another message first.
        "no-hello" => say(r#"{"done":"FeederEmpty"}"#, &[]),
        // Nothing at all, while it lives.
        "silent" => {}
        // Its hello, and never an answer.
        "deaf" => say(HELLO, &[]),
        "long-line" | "out-of-place" | "refuses" | "opened-another" | "renamed-elsewhere"
        | "no-source" | "big-page" | "page-length" | "padding" | "zero-dpi" | "cut-short"
        | "page-then-end" | "page-first" | "applied-late" | "sheet-out-of-turn"
        | "empty-after-page" | "encoded-unasked" | "huge-paper" | "helper-window"
        | "helper-batch" => {
            say(HELLO, &[]);
            let Some(open) = request(&mut input) else {
                return ExitCode::SUCCESS;
            };
            match name {
                // Its answer to `open`, past the cap.
                "long-line" => say(&"x".repeat((1 << 20) + 1), &[]),
                "out-of-place" => say(r#"{"devices":[]}"#, &[]),
                "refuses" => say(r#"{"refused":{"reason":"not today"}}"#, &[]),
                // Another device than the stored one, not said renamed; and one said renamed
                // from another id than the stored one's.
                "opened-another" | "renamed-elsewhere" => {
                    let mut info = open["open"]["device"].clone();
                    info["id"] = json!("fake:somebody-else");
                    let renamed_from = match name {
                        "renamed-elsewhere" => json!("fake:never-stored"),
                        _ => Value::Null,
                    };
                    let opened = json!({"opened": {"info": info, "renamed_from": renamed_from}});
                    say(&opened.to_string(), &[]);
                }
                _ => {
                    let device = &open["open"]["device"];
                    say(
                        &json!({"opened": {"info": device, "renamed_from": null}}).to_string(),
                        &[],
                    );
                    // `caps`, `scan` or `settings_window`, answered as one would be.
                    let Some(asked) = request(&mut input) else {
                        return ExitCode::SUCCESS;
                    };
                    let asked = &asked["scan"]["settings"];
                    let applied = |settings: &Value| {
                        json!({"applied": {"settings": settings, "clamped": []}}).to_string()
                    };
                    let page = |pixels: &str, dpi: &str, bytes: usize| {
                        format!(
                            r#"{{"page":{{"pixels":{pixels},"dpi":{dpi},"sheet":1,"side":"Front","side_known":"Inferred","bytes":{bytes}}}}}"#
                        )
                    };
                    let gray = |width: u32, height: u32| {
                        format!(
                            r#"{{"raw":{{"format":"Gray8","width":{width},"height":{height}}}}}"#
                        )
                    };
                    match name {
                        "no-source" => {
                            say(r#"{"caps":{"sources":[],"settings_window":false}}"#, &[])
                        }
                        // Its line, and none of its 1.2 GB.
                        "big-page" => say(
                            &page(
                                r#"{"raw":{"format":"Rgb8","width":20000,"height":20000}}"#,
                                "[300.0,300.0]",
                                1_200_000_000,
                            ),
                            &[],
                        ),
                        // Ten by ten in gray is 100 bytes, not 99.
                        "page-length" => {
                            say(&page(&gray(10, 10), "[300.0,300.0]", 99), &[0; 99]);
                        }
                        // Nine pixels a row is two bytes, its last seven bits 0.
                        "padding" => say(
                            &page(
                                r#"{"raw":{"format":"Bw1","width":9,"height":1}}"#,
                                "[300.0,300.0]",
                                2,
                            ),
                            &[0xff, 0xff],
                        ),
                        "zero-dpi" => say(&page(&gray(1, 1), "[0.0,300.0]", 1), &[0]),
                        // Out of the model's order, each where its line says.
                        "page-first" => say(&page(&gray(1, 1), "[300.0,300.0]", 1), &[0]),
                        "applied-late" => {
                            say(&applied(asked), &[]);
                            say(r#"{"sheet":1}"#, &[]);
                            say(&applied(asked), &[]);
                        }
                        "sheet-out-of-turn" => {
                            say(&applied(asked), &[]);
                            say(r#"{"sheet":1}"#, &[]);
                            say(r#"{"sheet":3}"#, &[]);
                        }
                        "empty-after-page" => {
                            say(&applied(asked), &[]);
                            say(r#"{"sheet":1}"#, &[]);
                            say(&page(&gray(1, 1), "[300.0,300.0]", 1), &[0]);
                            say(r#"{"error":"FeederEmpty"}"#, &[]);
                        }
                        // A JPEG, where the settings ask for raw pages alone.
                        "encoded-unasked" => {
                            say(&applied(asked), &[]);
                            say(r#"{"sheet":1}"#, &[]);
                            say(
                                &page(r#"{"encoded":{"kind":"Jpeg"}}"#, "[300.0,300.0]", 4),
                                b"JPEG",
                            );
                        }
                        // A paper too large for its number: it reads as an infinity.
                        "huge-paper" => {
                            let mut settings = asked.clone();
                            settings["paper"] = json!({"Size": {"width": 1e39, "height": 297.0}});
                            say(&applied(&settings), &[]);
                        }
                        // The driver starts a process that holds the worker's pipes, then ends the
                        // worker: an abort as its window opens, an exit with 7 within a batch.
                        "helper-window" => {
                            start_held();
                            std::process::abort();
                        }
                        "helper-batch" => {
                            say(&applied(asked), &[]);
                            say(r#"{"sheet":1}"#, &[]);
                            start_held();
                            std::process::exit(7);
                        }
                        // Half its bytes, then the end.
                        "cut-short" => {
                            say(&page(&gray(100, 100), "[300.0,300.0]", 10_000), &[0; 5000]);
                            return ExitCode::SUCCESS;
                        }
                        // A whole page, then the end, with no `done`.
                        "page-then-end" => {
                            say(&applied(asked), &[]);
                            say(r#"{"sheet":1}"#, &[]);
                            say(
                                &page(&gray(100, 100), "[300.0,300.0]", 10_000),
                                &[7; 10_000],
                            );
                            return ExitCode::SUCCESS;
                        }
                        _ => unreachable!("every scenario above is matched"),
                    }
                }
            }
        }
        _ => {
            eprintln!("scan-worker-fake: no scenario {name:?}");
            return ExitCode::from(2);
        }
    }
    // Its input read to the end, as a worker's whose client has gone.
    let _ = io::copy(&mut input, &mut io::sink());
    ExitCode::SUCCESS
}

/// The client's next request, as JSON; `None` once its input has ended.
fn request(input: &mut impl BufRead) -> Option<Value> {
    let mut line = String::new();
    match input.read_line(&mut line) {
        Ok(0) | Err(_) => None,
        Ok(_) => serde_json::from_str(&line).ok().or(Some(Value::Null)),
    }
}
