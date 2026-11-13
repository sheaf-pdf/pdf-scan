//! Integration tests for [`StopHandle`], `serve` `stdout` isolation, Windows Job Object
//! grandchild containment, and Windows `WindowOwner` validation and `EnableWindow` recovery.

mod support;

use std::{
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

use scan_worker::scan_device::in_progress::Tracker;
use scan_worker::scan_device::{Done, Driver, Event, Format, Source};
use scan_worker::{Client, Error, Timeouts};

use support::{
    AT_MOST, client, client_of, entries, open, outline, pages_are_pattern_s, scan, settings, soon,
    stored, worker, worker_of,
};

#[test]
fn stop_handle_aborts_a_hung_settings_window_and_reaps_the_worker() {
    let dir = tempfile::tempdir().unwrap();
    let work = tempfile::tempdir().unwrap();
    let signal = dir.path().join("entered");

    let cmd = worker("hang-window").env("SCAN_WORKER_TEST_SIGNAL_FILE", &signal);
    let client = Client::new()
        .worker(Driver::Fake, cmd)
        .work_dir(work.path());
    let mut device = open(&client);
    let stop = device.stop_handle();
    assert!(format!("{stop:?}").contains("StopHandle"));

    let stopper = thread::spawn(move || {
        assert!(
            soon(|| signal.exists()),
            "worker never entered settings_window"
        );
        stop.stop();
        // Calling `stop` repeatedly is harmless.
        stop.stop();
    });

    let start = Instant::now();
    let ended = device.settings_window(None);
    assert!(start.elapsed() < AT_MOST, "{:?}", start.elapsed());
    assert!(matches!(ended, Err(Error::Aborted)), "{ended:?}");
    stopper.join().unwrap();

    // Every subsequent call answers `Error::Ended`, and `close` succeeds cleanly.
    assert!(matches!(device.caps(), Err(Error::Ended)));
    device.close().expect("closed after abort");
    assert_eq!(entries(work.path()), 0);
}

#[test]
fn stop_handle_aborts_a_hung_batch_keeping_earlier_pages() {
    let mut timeouts = Timeouts::default();
    timeouts.silence = Duration::from_secs(30);
    let client = Client::new()
        .worker(Driver::Fake, worker_of("hang", 5))
        .timeouts(timeouts);
    let mut device = open(&client);
    let stop = device.stop_handle();

    let (events_tx, events_rx) = mpsc::sync_channel(64);
    let collector = thread::spawn(move || {
        let mut collected = Vec::new();
        while let Ok(event) = events_rx.recv() {
            let pages_so_far = collected
                .iter()
                .filter(|ev| matches!(ev, Event::Page(_)))
                .count()
                + usize::from(matches!(event, Event::Page(_)));
            collected.push(event);
            // 2 duplex sheets = 4 pages; sheet 3 hangs midway.
            if pages_so_far == 4 {
                thread::sleep(Duration::from_millis(50));
                stop.stop();
            }
        }
        collected
    });

    let start = Instant::now();
    let ended = device.scan(
        &settings(Source::Feeder, 100, Format::Gray8, true),
        1,
        &events_tx,
        &Tracker::new(),
    );
    drop(events_tx);
    let elapsed = start.elapsed();
    assert!(elapsed < AT_MOST, "{elapsed:?}");
    assert!(matches!(ended, Err(Error::Aborted)), "{ended:?}");

    let events = collector.join().unwrap();
    let before = [
        "applied",
        "sheet 1",
        "page 1 front",
        "page 1 back",
        "sheet 2",
        "page 2 front",
        "page 2 back",
    ];
    let got = outline(&events);
    assert!(
        got == before || got == [&before[..], &["sheet 3"]].concat(),
        "{got:?}"
    );
    pages_are_pattern_s(&events);
    assert!(matches!(device.caps(), Err(Error::Ended)));
    device.close().expect("closed");
}

#[test]
fn stop_handle_unblocks_scan_stuck_on_a_full_event_channel() {
    let mut device = open(&client_of("feeder", 2));
    let stop = device.stop_handle();
    // Rendezvous channel (`0`) with nobody reading `events_rx`: `Worker::scan` blocks trying to
    // hand off `Event::Applied` until `stop.stop()` wakes it.
    let (events_tx, _events_rx) = mpsc::sync_channel::<Event>(0);

    let stopper = thread::spawn(move || {
        thread::sleep(Duration::from_millis(120));
        stop.stop();
    });

    let start = Instant::now();
    let ended = device.scan(
        &settings(Source::Feeder, 100, Format::Gray8, false),
        1,
        &events_tx,
        &Tracker::new(),
    );
    assert!(start.elapsed() < AT_MOST, "{:?}", start.elapsed());
    assert!(matches!(ended, Err(Error::Aborted)), "{ended:?}");
    stopper.join().unwrap();
    assert!(matches!(device.caps(), Err(Error::Ended)));
}

#[test]
fn idle_stop_reports_aborted_once_then_ended_and_idle_close_skips_grace() {
    let work = tempfile::tempdir().unwrap();
    let client = client("feeder").work_dir(work.path());
    let mut device = open(&client);
    device.stop_handle().stop();
    assert!(matches!(device.caps(), Err(Error::Aborted)));
    assert!(matches!(device.caps(), Err(Error::Ended)));
    device.close().expect("closed");
    assert_eq!(entries(work.path()), 0);

    // An idle-stopped `"slow-exit"` worker (which would otherwise sleep 60 s on `"quit"`) is
    // reaped immediately by `close` without waiting `grace`.
    let mut timeouts = Timeouts::default();
    timeouts.grace = Duration::from_secs(30);
    let slow = Client::new()
        .worker(Driver::Fake, worker("slow-exit"))
        .timeouts(timeouts)
        .work_dir(work.path());
    let device = open(&slow);
    device.stop_handle().stop();
    let start = Instant::now();
    device.close().expect("idle-aborted close succeeds");
    assert!(
        start.elapsed() < Duration::from_secs(5),
        "{:?}",
        start.elapsed()
    );
    assert_eq!(entries(work.path()), 0);
}

#[test]
fn stop_after_a_prior_crash_or_close_keeps_ended() {
    let mut device = open(&client_of("crash", 3));
    let stop = device.stop_handle();
    let (_events, ended) = scan(
        &mut device,
        &settings(Source::Feeder, 100, Format::Gray8, false),
        1,
    );
    assert!(matches!(ended, Err(Error::Crashed { .. })), "{ended:?}");

    // Worker has already ended; `stop.stop()` is a no-op and does not turn `Ended` into `Aborted`.
    stop.stop();
    assert!(matches!(device.caps(), Err(Error::Ended)));
    device.close().expect("closed");
    stop.stop();

    // Un-reaped idle exit: the worker exits on its own *before* any client call reaps `live`, then
    // `stop.stop()` runs. `stop` must see `try_wait() == Ok(Some(_))` and leave `aborted` false so
    // the next call surfaces `Error::Crashed` (with status + stderr) rather than `Error::Aborted`!
    let mut exited_idle = open(&client("exit-after-open"));
    let idle_stop = exited_idle.stop_handle();
    thread::sleep(Duration::from_millis(150));
    idle_stop.stop();
    match exited_idle.caps() {
        Err(Error::Crashed { status, stderr, .. }) => {
            assert_eq!(status.and_then(|s| s.code()), Some(9));
            assert!(stderr.contains("idle crash"), "stderr={stderr:?}");
        }
        other => panic!("expected Crashed after un-reaped idle exit, got {other:?}"),
    }
    assert!(matches!(exited_idle.caps(), Err(Error::Ended)));
    exited_idle.close().expect("closed");
}

#[test]
fn noisy_driver_stdout_never_corrupts_the_wire_protocol() {
    let client = Client::new().worker(Driver::Fake, worker_of("noisy-stdout", 3));
    let listed = client.list(Driver::Fake, AT_MOST).expect("noisy list");
    assert_eq!(listed, [stored()]);

    let mut device = client.open(&listed[0], AT_MOST).expect("noisy open");
    let caps = device.caps().expect("noisy caps");
    assert!(!caps.sources.is_empty());

    let (events, done) = scan(
        &mut device,
        &settings(Source::Feeder, 150, Format::Rgb8, true),
        1,
    );
    assert_eq!(done.expect("noisy scan"), Done::FeederEmpty);
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
            "sheet 3",
            "page 3 front",
            "page 3 back",
        ]
    );
    pages_are_pattern_s(&events);
    device.close().expect("noisy close");
}

#[cfg(windows)]
mod windows {
    use core::ptr::{null, null_mut};
    use std::fs;
    use std::sync::mpsc;
    use std::thread;
    use std::time::Duration;

    use scan_worker::scan_device::{Driver, ScanError, WindowOwner};
    use scan_worker::{Client, Error};
    use windows_sys::Win32::Foundation::{CloseHandle, HWND, WAIT_OBJECT_0};
    use windows_sys::Win32::System::Threading::{
        GetCurrentThreadId, OpenProcess, WaitForSingleObject,
    };
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{EnableWindow, IsWindowEnabled};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CW_USEDEFAULT, CreateWindowExW, DestroyWindow, MSG, PM_REMOVE, PeekMessageW,
        PostThreadMessageW, WM_QUIT, WS_OVERLAPPEDWINDOW,
    };

    use super::support::{aborted, open, soon, worker};

    const SYNCHRONIZE: u32 = 0x0010_0000;

    #[test]
    fn job_object_terminates_grandchild_on_stop_and_on_worker_exit() {
        let dir = tempfile::tempdir().unwrap();

        // 1. `StopHandle::stop` terminates both the worker and its `held` grandchild.
        let pid_file_1 = dir.path().join("held1.pid");
        let sig_file_1 = dir.path().join("sig1");
        let cmd_1 = worker("helper-hang-window")
            .env("SCAN_WORKER_TEST_PID_FILE", &pid_file_1)
            .env("SCAN_WORKER_TEST_SIGNAL_FILE", &sig_file_1);
        let client_1 = Client::new().worker(Driver::Fake, cmd_1);
        let mut dev_1 = open(&client_1);
        let stop_1 = dev_1.stop_handle();

        let waiter_1 = thread::spawn(move || {
            assert!(soon(|| sig_file_1.exists() && pid_file_1.exists()));
            let grandchild_pid: u32 = fs::read_to_string(&pid_file_1)
                .unwrap()
                .trim()
                .parse()
                .unwrap();
            // SAFETY: `SYNCHRONIZE` opens a waitable handle to the live grandchild process.
            let handle = unsafe { OpenProcess(SYNCHRONIZE, 0, grandchild_pid) };
            assert!(
                !handle.is_null(),
                "grandchild {grandchild_pid} should be running"
            );
            stop_1.stop();
            // SAFETY: waits up to 5 s for the Job Object to terminate the grandchild, then closes
            // our wait handle.
            let wait = unsafe {
                let rc = WaitForSingleObject(handle, 5_000);
                CloseHandle(handle);
                rc
            };
            assert_eq!(
                wait, WAIT_OBJECT_0,
                "Job Object did not terminate grandchild"
            );
        });

        assert!(matches!(dev_1.settings_window(None), Err(Error::Aborted)));
        waiter_1.join().unwrap();
        dev_1.close().unwrap();

        // 2. When a worker crashes after spawning `held` (`helper-window`), `Worker::end` captures
        // the worker's abort exit status AND terminates the Job Object so `held` dies too.
        let pid_file_2 = dir.path().join("held2.pid");
        let cmd_2 = worker("helper-window").env("SCAN_WORKER_TEST_PID_FILE", &pid_file_2);
        let client_2 = Client::new().worker(Driver::Fake, cmd_2);
        let mut dev_2 = open(&client_2);
        let Err(Error::Crashed { status, .. }) = dev_2.settings_window(None) else {
            panic!("expected Crashed");
        };
        assert!(aborted(&status.expect("worker exit status")));
        let grandchild_pid: u32 = fs::read_to_string(&pid_file_2)
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        // SAFETY: if the grandchild has not already vanished from the process table, waiting on it
        // must report immediate termination.
        unsafe {
            let handle = OpenProcess(SYNCHRONIZE, 0, grandchild_pid);
            if !handle.is_null() {
                let rc = WaitForSingleObject(handle, 5_000);
                CloseHandle(handle);
                assert_eq!(rc, WAIT_OBJECT_0, "grandchild survived worker crash");
            }
        }
    }

    #[test]
    fn window_guard_refuses_same_thread_hwnd_and_restores_cross_thread_hwnd() {
        let class: Vec<u16> = "STATIC\0".encode_utf16().collect();

        // 1. Same-thread live HWND refused immediately without blocking or killing the worker.
        // SAFETY: creates a hidden `"STATIC"` window on this thread.
        let same_hwnd = unsafe {
            CreateWindowExW(
                0,
                class.as_ptr(),
                class.as_ptr(),
                WS_OVERLAPPEDWINDOW,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                100,
                100,
                null_mut(),
                null_mut(),
                null_mut(),
                null(),
            )
        };
        assert!(!same_hwnd.is_null());
        let mut dev = open(&Client::new().worker(Driver::Fake, worker("hang-window")));
        let refused = dev.settings_window(Some(WindowOwner(same_hwnd as usize as u64)));
        assert!(
            matches!(
                refused,
                Err(Error::Scan(ScanError::Driver { shown: false, .. }))
            ),
            "{refused:?}"
        );
        // SAFETY: destroys our hidden window once on its owning thread.
        unsafe {
            DestroyWindow(same_hwnd);
        }
        // Worker was never blocked or killed by the same-thread refusal.
        assert!(dev.caps().is_ok());
        dev.close().unwrap();

        // 2. Cross-thread live HWND pumped on a dedicated UI thread (with a Drop guard so unwind
        // always sends `WM_QUIT` and joins).
        struct UiPump {
            tid: u32,
            handle: Option<thread::JoinHandle<()>>,
        }
        impl Drop for UiPump {
            fn drop(&mut self) {
                // SAFETY: posts `WM_QUIT` to shut down `self.tid` and joins the thread.
                unsafe {
                    PostThreadMessageW(self.tid, WM_QUIT, 0, 0);
                }
                if let Some(h) = self.handle.take() {
                    let _ = h.join();
                }
            }
        }

        let (ui_tx, ui_rx) = mpsc::channel();
        let ui_thread = thread::spawn(move || {
            // SAFETY: queries current thread id and creates a hidden top-level window pumped on
            // this UI thread until `WM_QUIT`.
            let (tid, hwnd) = unsafe {
                let tid = GetCurrentThreadId();
                let hwnd = CreateWindowExW(
                    0,
                    class.as_ptr(),
                    class.as_ptr(),
                    WS_OVERLAPPEDWINDOW,
                    CW_USEDEFAULT,
                    CW_USEDEFAULT,
                    100,
                    100,
                    null_mut(),
                    null_mut(),
                    null_mut(),
                    null(),
                );
                assert!(!hwnd.is_null());
                (tid, hwnd)
            };
            ui_tx.send((tid, hwnd as usize)).unwrap();
            // SAFETY: pumps messages on the window's owning thread until `WM_QUIT`.
            unsafe {
                let mut msg: MSG = core::mem::zeroed();
                loop {
                    while PeekMessageW(&mut msg, null_mut(), 0, 0, PM_REMOVE) != 0 {
                        if msg.message == WM_QUIT {
                            DestroyWindow(hwnd);
                            return;
                        }
                    }
                    thread::sleep(Duration::from_millis(5));
                }
            }
        });

        let (ui_tid, raw_usize) = ui_rx.recv().unwrap();
        let _ui_pump = UiPump {
            tid: ui_tid,
            handle: Some(ui_thread),
        };
        let hwnd = raw_usize as HWND;
        let owner = Some(WindowOwner(raw_usize as u64));
        let dir = tempfile::tempdir().unwrap();

        // 2a. Enabled window disabled during `settings_window`, then aborted via `StopHandle` ->
        // re-enabled by `WindowGuard`.
        let sig_a = dir.path().join("sig_a");
        let mut dev_a = open(&Client::new().worker(
            Driver::Fake,
            worker("hang-window").env("SCAN_WORKER_TEST_SIGNAL_FILE", &sig_a),
        ));
        let stop_a = dev_a.stop_handle();
        let actor_a = thread::spawn(move || {
            assert!(soon(|| sig_a.exists()));
            let h = raw_usize as HWND;
            // SAFETY: `h` is live on `ui_thread`.
            unsafe {
                EnableWindow(h, 0);
                assert_eq!(IsWindowEnabled(h), 0);
            }
            stop_a.stop();
        });
        assert!(matches!(dev_a.settings_window(owner), Err(Error::Aborted)));
        actor_a.join().unwrap();
        // SAFETY: `hwnd` is live on `ui_thread`.
        let enabled_a = unsafe { IsWindowEnabled(hwnd) };
        assert_ne!(enabled_a, 0, "WindowGuard must re-enable owner");

        // 2b. Enabled window disabled during `settings_window`, then worker crashes -> re-enabled.
        let sig_b = dir.path().join("sig_b");
        let go_b = dir.path().join("go_b");
        let mut dev_b = open(
            &Client::new().worker(
                Driver::Fake,
                worker("crash-window")
                    .env("SCAN_WORKER_TEST_SIGNAL_FILE", &sig_b)
                    .env("SCAN_WORKER_TEST_GO_FILE", &go_b),
            ),
        );
        let go_b_actor = go_b.clone();
        let actor_b = thread::spawn(move || {
            assert!(soon(|| sig_b.exists()));
            let h = raw_usize as HWND;
            // SAFETY: `h` is live on `ui_thread`.
            unsafe {
                EnableWindow(h, 0);
                assert_eq!(IsWindowEnabled(h), 0);
            }
            fs::write(go_b_actor, b"go").unwrap();
        });
        assert!(matches!(
            dev_b.settings_window(owner),
            Err(Error::Crashed { .. })
        ));
        actor_b.join().unwrap();
        // SAFETY: `hwnd` is live on `ui_thread`.
        let enabled_b = unsafe { IsWindowEnabled(hwnd) };
        assert_ne!(enabled_b, 0, "WindowGuard must re-enable after crash");

        // 2c. Window already disabled beforehand stays disabled after abort.
        // SAFETY: `hwnd` is live on `ui_thread`.
        unsafe {
            EnableWindow(hwnd, 0);
        }
        let sig_c = dir.path().join("sig_c");
        let mut dev_c = open(&Client::new().worker(
            Driver::Fake,
            worker("hang-window").env("SCAN_WORKER_TEST_SIGNAL_FILE", &sig_c),
        ));
        let stop_c = dev_c.stop_handle();
        let actor_c = thread::spawn(move || {
            assert!(soon(|| sig_c.exists()));
            stop_c.stop();
        });
        assert!(matches!(dev_c.settings_window(owner), Err(Error::Aborted)));
        actor_c.join().unwrap();
        // SAFETY: `hwnd` is live on `_ui_pump`.
        let enabled_c = unsafe { IsWindowEnabled(hwnd) };
        assert_eq!(enabled_c, 0, "pre-disabled window must remain disabled");
    }
}
