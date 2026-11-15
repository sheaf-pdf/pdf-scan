//! The `wia` and `wia-fake` scenarios: `scan-device`'s WIA 2.0 backend served in a worker process
//! (`wia` against live OS `IWiaDevMgr2` on Windows, or `NotInstalled` elsewhere; `wia-fake`
//! driving `WiaBackend::from_fake` end-to-end across JSON-lines pipes on every OS and bitness).

use std::{
    env, fs,
    process::ExitCode,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
        mpsc::SyncSender,
    },
    thread,
    time::{Duration, Instant},
};

use scan_device::in_progress::Progress;
use scan_device::twain::TwainBackend;
use scan_device::wia::WiaBackend;
use scan_device::wia::fake_wia::{
    ALL_PAGES, DUPLEX, E_FAIL_HR, FEEDER, FakeDeviceSpec, FakeDownloadCall, FakeItem,
    FakeTransferEvent, FakeWiaHandle, FormatPair, GuidBytes, ItemId, PropAttr, PropConstraint,
    PropValue, REGDB_E_CLASSNOTREG_HR, RawDeviceProps, S_OK_HR, TYMED_FILE,
    WIA_CATEGORY_FEEDER_GUID, WIA_CATEGORY_FLATBED_GUID, WIA_CMD_STOP_FEEDER_GUID, WIA_DATA_COLOR,
    WIA_DATA_GRAYSCALE, WIA_DATA_THRESHOLD, WIA_ERROR_PAPER_EMPTY_HR, WIA_ERROR_PAPER_JAM_HR,
    WIA_IMG_FMT_BMP_GUID, WIA_IMG_FMT_PNG_GUID, WIA_IPA_DATATYPE, WIA_IPA_DEPTH, WIA_IPA_FORMAT,
    WIA_IPA_ITEM_CATEGORY, WIA_IPA_TYMED, WIA_IPS_DOCUMENT_HANDLING_SELECT,
    WIA_IPS_MAX_HORIZONTAL_SIZE, WIA_IPS_MAX_VERTICAL_SIZE, WIA_IPS_PAGE_SIZE, WIA_IPS_PAGES,
    WIA_IPS_XEXTENT, WIA_IPS_XPOS, WIA_IPS_XRES, WIA_IPS_YEXTENT, WIA_IPS_YPOS, WIA_IPS_YRES,
    WIA_ITEM_TYPE_IMAGE, WIA_ITEM_TYPE_TRANSFER, WIA_PAGE_AUTO, WIA_PAGE_CUSTOM, WIA_PAGE_LETTER,
    WIA_PROP_FLAG, WIA_PROP_LIST, WIA_PROP_RANGE, WIA_PROP_RW, WIA_STATUS_WARMING_UP_HR,
};
use scan_device::{
    Backend, Caps, Device, DeviceInfo, Done, Event, Opened, ScanError, Settings, WindowClosed,
    WindowOwner,
};

use crate::twain::NoisyBackend;

/// `wia [--noisy-stdout]`: serves live [`WiaBackend::new()`] over `scan_worker::serve`.
pub fn main(args: &[String]) -> ExitCode {
    let mut noisy = false;
    for arg in args {
        match arg.as_str() {
            "--noisy-stdout" => noisy = true,
            other => {
                eprintln!("scan-worker-fake: wia [--noisy-stdout]: no argument {other:?}");
                return ExitCode::from(2);
            }
        }
    }
    let backend: Box<dyn Backend> = Box::new(WiaBackend::new());
    if noisy {
        scan_worker::serve(vec![Box::new(NoisyBackend(backend))])
    } else {
        scan_worker::serve(vec![backend])
    }
}

/// `wia-fake <office|await-cancel|empty|jam-mid-batch|not-installed|multi-backend> [--noisy-stdout]`
pub fn fake_main(args: &[String]) -> ExitCode {
    let mode = args.first().map_or("office", String::as_str);
    let mut noisy = false;
    for extra in args.iter().skip(1) {
        match extra.as_str() {
            "--noisy-stdout" => noisy = true,
            other => {
                eprintln!("scan-worker-fake: wia-fake {mode}: unknown flag {other:?}");
                return ExitCode::from(2);
            }
        }
    }

    let (fake, command_log, extra_fed, cancel_slot) = match build_fake(mode) {
        Ok(built) => built,
        Err(msg) => {
            eprintln!("scan-worker-fake: wia-fake: {msg}");
            return ExitCode::from(2);
        }
    };

    let wia = match WiaBackend::from_fake(&fake) {
        Ok(b) => b,
        Err(err) => {
            eprintln!("scan-worker-fake: wia-fake failed to spawn session: {err}");
            return ExitCode::from(2);
        }
    };

    let wia_backend: Box<dyn Backend> = if mode == "await-cancel" {
        Box::new(CancelWatchBackend {
            inner: Box::new(wia),
            slot: cancel_slot,
        })
    } else {
        Box::new(wia)
    };

    let mut backends: Vec<Box<dyn Backend>> = Vec::new();
    if mode == "multi-backend" {
        // Put `TwainBackend` ahead of `WiaBackend` so `serve`'s open loop must skip `BadId` on
        // `TwainBackend` and dispatch `Driver::Wia` cleanly to `WiaBackend`.
        backends.push(Box::new(TwainBackend::new()));
    }
    if noisy {
        backends.push(Box::new(NoisyBackend(wia_backend)));
    } else {
        backends.push(wia_backend);
    }

    let code = scan_worker::serve(backends);

    // For `await-cancel`, write a JSON summary to `SCAN_WORKER_TEST_SUMMARY_FILE` on clean exit so
    // the parent test can assert `WIA_CMD_STOP_FEEDER` ran and Sheet 2 was never fed!
    if let Some(summary_path) = env::var_os("SCAN_WORKER_TEST_SUMMARY_FILE") {
        let stop_feeder = command_log
            .lock()
            .unwrap()
            .contains(&WIA_CMD_STOP_FEEDER_GUID);
        let sheet2 = extra_fed.load(Ordering::SeqCst);
        let json = serde_json::json!({
            "stop_feeder": stop_feeder,
            "sheet2_fed": sheet2,
        });
        let _ = fs::write(summary_path, json.to_string());
    }

    code
}

fn rw_list(nom: i32, values: Vec<i32>) -> Option<PropAttr> {
    Some(PropAttr {
        access_flags: WIA_PROP_RW | WIA_PROP_LIST,
        constraint: PropConstraint::ListI4 { nom, values },
    })
}

fn rw_range(min: i32, nom: i32, max: i32, step: i32) -> Option<PropAttr> {
    Some(PropAttr {
        access_flags: WIA_PROP_RW | WIA_PROP_RANGE,
        constraint: PropConstraint::Range {
            min,
            nom,
            max,
            step,
        },
    })
}

fn rw_flags(nom: i32, valid_bits: i32) -> Option<PropAttr> {
    Some(PropAttr {
        access_flags: WIA_PROP_RW | WIA_PROP_FLAG,
        constraint: PropConstraint::Flags { nom, valid_bits },
    })
}

fn rw_guids(nom: GuidBytes, values: Vec<GuidBytes>) -> Option<PropAttr> {
    Some(PropAttr {
        access_flags: WIA_PROP_RW | WIA_PROP_LIST,
        constraint: PropConstraint::ListGuid { nom, values },
    })
}

fn make_bmp_24bit_2x2(tag: u8) -> Vec<u8> {
    // 2x2 24-bit BGR bottom-up `.bmp`: row bytes = 6, padded to 8 -> 16 pixel bytes + 54 header = 70 bytes.
    let mut out = Vec::with_capacity(70);
    out.extend_from_slice(b"BM");
    out.extend_from_slice(&70u32.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&54u32.to_le_bytes());
    out.extend_from_slice(&40u32.to_le_bytes());
    out.extend_from_slice(&2i32.to_le_bytes());
    out.extend_from_slice(&2i32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&24u16.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&11811i32.to_le_bytes());
    out.extend_from_slice(&11811i32.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    // Bottom row (becomes row 1 in top-down RawImage): BGR pixels.
    out.extend_from_slice(&[3, 2, 1, 6, 5, 4, 0, 0]);
    // Top row (becomes row 0 in top-down RawImage): first pixel carries `tag` in Red!
    out.extend_from_slice(&[20, 10, tag, 50, 40, 30, 0, 0]);
    out
}

fn make_png_bytes() -> Vec<u8> {
    let mut v = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    v.extend_from_slice(b"IHDR-worker-wia-png");
    v
}

fn scanner_props() -> RawDeviceProps {
    RawDeviceProps {
        dev_id: r"{6BDD1FC6-810F-11D0-BEC7-08002BE2092F}\0042".to_owned(),
        dev_type: Some(0x0001_0001), // StiDeviceTypeScanner
        dev_name: Some("Office WIA MFP".to_owned()),
        vend_desc: Some("Microsoft".to_owned()),
        dev_desc: Some("WSD Scan Device".to_owned()),
        wsd_manufacturer: Some("Contoso".to_owned()),
        wsd_model_name: Some("DocFeeder 500".to_owned()),
        wsd_friendly_name: Some("Front Desk WIA".to_owned()),
    }
}

fn base_source_item(category: GuidBytes, handling_valid: i32) -> FakeItem {
    let mut item = FakeItem::new(WIA_ITEM_TYPE_IMAGE | WIA_ITEM_TYPE_TRANSFER)
        .with_prop(WIA_IPA_ITEM_CATEGORY, PropValue::Guid(category), None)
        .with_prop(
            WIA_IPS_XRES,
            PropValue::I4(300),
            rw_list(300, vec![150, 300]),
        )
        .with_prop(
            WIA_IPS_YRES,
            PropValue::I4(300),
            rw_list(300, vec![150, 300]),
        )
        .with_prop(
            WIA_IPA_DATATYPE,
            PropValue::I4(WIA_DATA_COLOR),
            rw_list(
                WIA_DATA_COLOR,
                vec![WIA_DATA_THRESHOLD, WIA_DATA_GRAYSCALE, WIA_DATA_COLOR],
            ),
        )
        .with_prop(
            WIA_IPA_DEPTH,
            PropValue::I4(24),
            rw_list(24, vec![1, 8, 24]),
        )
        .with_prop(
            WIA_IPA_TYMED,
            PropValue::I4(TYMED_FILE),
            rw_list(TYMED_FILE, vec![TYMED_FILE]),
        )
        .with_prop(
            WIA_IPA_FORMAT,
            PropValue::Guid(WIA_IMG_FMT_BMP_GUID),
            rw_guids(
                WIA_IMG_FMT_BMP_GUID,
                vec![WIA_IMG_FMT_BMP_GUID, WIA_IMG_FMT_PNG_GUID],
            ),
        )
        .with_formats(vec![
            FormatPair {
                format: WIA_IMG_FMT_BMP_GUID,
                tymed: TYMED_FILE,
            },
            FormatPair {
                format: WIA_IMG_FMT_PNG_GUID,
                tymed: TYMED_FILE,
            },
        ])
        .with_prop(WIA_IPS_MAX_HORIZONTAL_SIZE, PropValue::I4(8500), None)
        .with_prop(WIA_IPS_MAX_VERTICAL_SIZE, PropValue::I4(11000), None)
        .with_prop(
            WIA_IPS_PAGE_SIZE,
            PropValue::I4(WIA_PAGE_AUTO),
            rw_list(
                WIA_PAGE_AUTO,
                vec![WIA_PAGE_LETTER, WIA_PAGE_CUSTOM, WIA_PAGE_AUTO],
            ),
        )
        .with_prop(WIA_IPS_XPOS, PropValue::I4(0), rw_range(0, 0, 8500, 1))
        .with_prop(WIA_IPS_YPOS, PropValue::I4(0), rw_range(0, 0, 11000, 1))
        .with_prop(
            WIA_IPS_XEXTENT,
            PropValue::I4(2550),
            rw_range(1, 2550, 2550, 1),
        )
        .with_prop(
            WIA_IPS_YEXTENT,
            PropValue::I4(3300),
            rw_range(1, 3300, 3300, 1),
        )
        .with_write_hook(|item, prop, val| {
            if prop == WIA_IPA_DATATYPE
                && let Some(dt) = val.as_i4()
            {
                let d = match dt {
                    WIA_DATA_THRESHOLD => 1,
                    WIA_DATA_GRAYSCALE => 8,
                    _ => 24,
                };
                if let Some(slot) = item.props.get_mut(&WIA_IPA_DEPTH) {
                    *slot = PropValue::I4(d);
                }
            }
            Ok(())
        });

    if handling_valid != 0 {
        item = item
            .with_prop(
                WIA_IPS_DOCUMENT_HANDLING_SELECT,
                PropValue::I4(FEEDER),
                rw_flags(FEEDER, handling_valid),
            )
            .with_prop(
                WIA_IPS_PAGES,
                PropValue::I4(ALL_PAGES),
                rw_range(0, 0, 100, 1),
            );
    }
    item
}

type CancelSlot = Arc<Mutex<Option<Box<dyn Progress>>>>;

struct CancelWatchBackend {
    inner: Box<dyn Backend>,
    slot: CancelSlot,
}

impl Backend for CancelWatchBackend {
    fn list(&mut self, deadline: Duration) -> Result<Vec<DeviceInfo>, ScanError> {
        self.inner.list(deadline)
    }

    fn open(&mut self, device: &DeviceInfo, deadline: Duration) -> Result<Opened, ScanError> {
        let mut opened = self.inner.open(device, deadline)?;
        opened.device = Box::new(CancelWatchDevice {
            inner: opened.device,
            slot: Arc::clone(&self.slot),
        });
        Ok(opened)
    }
}

struct CancelWatchDevice {
    inner: Box<dyn Device>,
    slot: CancelSlot,
}

impl Device for CancelWatchDevice {
    fn caps(&mut self) -> Result<Caps, ScanError> {
        self.inner.caps()
    }

    fn scan(
        &mut self,
        settings: &Settings,
        first_sheet: u32,
        events: &SyncSender<Event>,
        progress: &dyn Progress,
    ) -> Result<Done, ScanError> {
        *self.slot.lock().unwrap() = Some(progress.child("cancel-watch", 0.0));
        let res = self.inner.scan(settings, first_sheet, events, progress);
        *self.slot.lock().unwrap() = None;
        res
    }

    fn settings_window(&mut self, owner: Option<WindowOwner>) -> Result<WindowClosed, ScanError> {
        self.inner.settings_window(owner)
    }
}

type BuiltFake = (
    FakeWiaHandle,
    Arc<Mutex<Vec<GuidBytes>>>,
    Arc<AtomicUsize>,
    CancelSlot,
);

fn build_fake(mode: &str) -> Result<BuiltFake, String> {
    let fake = FakeWiaHandle::new();
    let extra_fed = Arc::new(AtomicUsize::new(0));
    let cancel_slot: CancelSlot = Arc::new(Mutex::new(None));

    if mode == "not-installed" {
        fake.state.lock().unwrap().list_error_hr = Some(REGDB_E_CLASSNOTREG_HR);
        let mut spec = FakeDeviceSpec::new(scanner_props());
        spec.open_error_hr = Some(REGDB_E_CLASSNOTREG_HR);
        let log = Arc::clone(&spec.command_log);
        fake.add_device(spec);
        return Ok((fake, log, extra_fed, cancel_slot));
    }

    // Add a non-scanner camera node to verify worker listings filter cameras out.
    let mut cam = scanner_props();
    cam.dev_id = r"{6BDD1FC6-810F-11D0-BEC7-08002BE2092F}\CAM0".to_owned();
    cam.dev_type = Some(0x0002_0001); // StiDeviceTypeDigitalCamera (HIWORD == 2)
    fake.add_device(FakeDeviceSpec::new(cam));

    // Emit noisy driver chatter directly to `stdout` during `list` as well!
    fake.state.lock().unwrap().list_stdout_chatter =
        Some("{\"hello\":{\"protocol\":999}}\n[wia-driver stdout chatter]".to_owned());

    let mut spec = FakeDeviceSpec::new(scanner_props());
    let cmd_log = Arc::clone(&spec.command_log);

    match mode {
        "office" | "multi-backend" => {
            let flatbed = base_source_item(WIA_CATEGORY_FLATBED_GUID, 0).with_downloads([
                // Call 1: raw BMP flatbed scan with mid-transfer stdout chatter.
                FakeDownloadCall {
                    events: vec![FakeTransferEvent::Page {
                        chunks: vec![35, 70],
                        bytes: make_bmp_24bit_2x2(0xAA),
                    }],
                    return_hr: S_OK_HR,
                    cancel_hr: None,
                    require_tymed: None,
                    stdout_chatter: Some("{\"error\":\"Jammed\"}\nWIA minidriver trace".to_owned()),
                },
                // Call 2: encoded PNG flatbed scan.
                FakeDownloadCall::ok(vec![FakeTransferEvent::Page {
                    chunks: vec![27],
                    bytes: make_png_bytes(),
                }]),
            ]);

            // WSD duplex feeder (`WIA_IPS_PAGES` constrained to `2..=2`): warms up once, then
            // delivers 2 duplex sheets (4 pages) across multiple `Download` calls, ending with
            // `EmptyStreamError(WIA_ERROR_PAPER_EMPTY_HR)` + `E_FAIL_HR`.
            let feeder = base_source_item(WIA_CATEGORY_FEEDER_GUID, FEEDER | DUPLEX)
                .with_prop(WIA_IPS_PAGES, PropValue::I4(2), rw_range(2, 2, 2, 2))
                .with_downloads([
                    FakeDownloadCall::with_hr(Vec::new(), WIA_STATUS_WARMING_UP_HR),
                    FakeDownloadCall::ok(vec![
                        FakeTransferEvent::Page {
                            chunks: vec![30, 70],
                            bytes: make_bmp_24bit_2x2(11),
                        },
                        FakeTransferEvent::Page {
                            chunks: vec![70],
                            bytes: make_bmp_24bit_2x2(12),
                        },
                    ]),
                    FakeDownloadCall::ok(vec![
                        FakeTransferEvent::Page {
                            chunks: vec![70],
                            bytes: make_bmp_24bit_2x2(21),
                        },
                        FakeTransferEvent::Page {
                            chunks: vec![70],
                            bytes: make_bmp_24bit_2x2(22),
                        },
                    ]),
                    FakeDownloadCall::with_hr(
                        vec![FakeTransferEvent::EmptyStreamError {
                            hr_status: WIA_ERROR_PAPER_EMPTY_HR,
                        }],
                        E_FAIL_HR,
                    ),
                ]);

            spec.add_child(ItemId::ROOT, flatbed);
            spec.add_child(ItemId::ROOT, feeder);
        }
        "await-cancel" => {
            let fed_clone = Arc::clone(&extra_fed);
            let slot_clone = Arc::clone(&cancel_slot);
            let feeder = base_source_item(WIA_CATEGORY_FEEDER_GUID, FEEDER | DUPLEX)
                .with_downloads([FakeDownloadCall::ok(vec![
                    // Sheet 1 Front
                    FakeTransferEvent::Page {
                        chunks: vec![70],
                        bytes: make_bmp_24bit_2x2(1),
                    },
                    // Signal parent test that Sheet 1 Front is done, then wait deterministically
                    // until `scan_worker::serve`'s stdin reader marks the batch `Progress`
                    // cancelled!
                    FakeTransferEvent::Hook(Arc::new(move || {
                        if let Some(sig) = env::var_os("SCAN_WORKER_TEST_SIGNAL_FILE") {
                            let _ = fs::write(sig, b"front-done");
                        }
                        let start = Instant::now();
                        while !slot_clone
                            .lock()
                            .unwrap()
                            .as_ref()
                            .is_some_and(|p| p.is_cancelled())
                            && start.elapsed() < Duration::from_secs(5)
                        {
                            thread::sleep(Duration::from_millis(5));
                        }
                    })),
                    // Sheet 1 Back (must still be delivered to complete the duplex sheet!)
                    FakeTransferEvent::Page {
                        chunks: vec![70],
                        bytes: make_bmp_24bit_2x2(2),
                    },
                    // Sheet 2 Front (must NOT be fed: `GetNextStream` returns `S_FALSE`!)
                    FakeTransferEvent::Page {
                        chunks: vec![70],
                        bytes: make_bmp_24bit_2x2(3),
                    },
                    FakeTransferEvent::Hook(Arc::new(move || {
                        fed_clone.fetch_add(1, Ordering::SeqCst);
                    })),
                ])]);
            spec.add_child(ItemId::ROOT, feeder);
        }
        "empty" => {
            let feeder = base_source_item(WIA_CATEGORY_FEEDER_GUID, FEEDER).with_downloads([
                FakeDownloadCall::with_hr(Vec::new(), WIA_ERROR_PAPER_EMPTY_HR),
            ]);
            spec.add_child(ItemId::ROOT, feeder);
        }
        "jam-mid-batch" => {
            let feeder = base_source_item(WIA_CATEGORY_FEEDER_GUID, FEEDER).with_downloads([
                FakeDownloadCall::with_hr(
                    vec![
                        FakeTransferEvent::Page {
                            chunks: vec![70],
                            bytes: make_bmp_24bit_2x2(77),
                        },
                        FakeTransferEvent::EmptyStreamError {
                            hr_status: WIA_ERROR_PAPER_JAM_HR,
                        },
                    ],
                    E_FAIL_HR,
                ),
            ]);
            spec.add_child(ItemId::ROOT, feeder);
        }
        other => return Err(format!("unknown wia-fake mode {other:?}")),
    }

    fake.add_device(spec);
    Ok((fake, cmd_log, extra_fed, cancel_slot))
}
