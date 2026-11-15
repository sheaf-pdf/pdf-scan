//! Cross-platform scripted WIA 2.0 integration tests exercising `WiaBackend` end to end across
//! listing, opening, capabilities, WDK property negotiation, all 5 raw pixel formats, encoded
//! pass-through, WSD multi-`Download` duplex sheets, `BACK_FIRST` side pairing, deferred
//! `Event::Sheet`, `WarmingUp` retries, `TYMED_CALLBACK` -> `TYMED_FILE` fallback, mid-sheet
//! duplex cancellation (`S_FALSE` + `WIA_CMD_STOP_FEEDER`), channel backpressure, receiver drop,
//! stream overflow, and dual SDK / Microsoft Learn `HRESULT` mapping.

#![cfg(all(feature = "wia", feature = "fake"))]

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::sync_channel;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use scan_device::in_progress::{Tracker, Unit};
use scan_device::wia::WiaBackend;
use scan_device::wia::fake_wia::*;
use scan_device::{
    Backend, Clamped, DeviceId, DeviceInfo, Done, Driver, EncodedKind, Encoding, Event, Format,
    Paper, Pixels, ScanError, Settings, Side, SideKnown, SizeMm, Source, Status,
};

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

fn scanner_props(dev_id: &str) -> RawDeviceProps {
    RawDeviceProps {
        dev_id: dev_id.to_owned(),
        dev_type: Some(0x0001_0001),
        dev_name: Some("Office ADF".to_owned()),
        vend_desc: Some("Acme".to_owned()),
        dev_desc: Some("WIA-9000".to_owned()),
        ..RawDeviceProps::default()
    }
}

fn make_bmp_24bit_1x1(r: u8, g: u8, b: u8) -> Vec<u8> {
    let mut out = Vec::with_capacity(58);
    out.extend_from_slice(b"BM");
    out.extend_from_slice(&58u32.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&54u32.to_le_bytes());
    out.extend_from_slice(&40u32.to_le_bytes());
    out.extend_from_slice(&1i32.to_le_bytes());
    out.extend_from_slice(&1i32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&24u16.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&4u32.to_le_bytes());
    out.extend_from_slice(&[0u8; 16]);
    out.extend_from_slice(&[b, g, r, 0]);
    out
}

#[allow(clippy::too_many_arguments)]
fn make_wraw(
    dpi: u32,
    width: u32,
    height: u32,
    bpp: u32,
    channels: u32,
    data_type: i32,
    bpc: u8,
    photometric: i32,
    payload: &[u8],
) -> Vec<u8> {
    let mut out = Vec::with_capacity(80 + payload.len());
    out.extend_from_slice(b"WRAW");
    out.extend_from_slice(&0x0001_0000u32.to_le_bytes());
    out.extend_from_slice(&80u32.to_le_bytes());
    out.extend_from_slice(&dpi.to_le_bytes());
    out.extend_from_slice(&dpi.to_le_bytes());
    out.extend_from_slice(&width.to_le_bytes());
    out.extend_from_slice(&height.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes()); // BytesPerLine = 0 -> packed
    out.extend_from_slice(&bpp.to_le_bytes());
    out.extend_from_slice(&channels.to_le_bytes());
    out.extend_from_slice(&(data_type as u32).to_le_bytes());
    out.extend_from_slice(&[bpc, bpc, bpc, 0, 0, 0, 0, 0]);
    out.extend_from_slice(&0u32.to_le_bytes()); // WIA_COMPRESSION_NONE
    out.extend_from_slice(&(photometric as u32).to_le_bytes());
    out.extend_from_slice(&WIA_LINE_ORDER_TOP_TO_BOTTOM.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes()); // RawDataOffset
    out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(payload);
    out
}

fn base_feeder_item(duplex_flags: i32) -> FakeItem {
    FakeItem::new(WIA_ITEM_TYPE_IMAGE | WIA_ITEM_TYPE_TRANSFER)
        .with_prop(
            WIA_IPA_ITEM_CATEGORY,
            PropValue::Guid(WIA_CATEGORY_FEEDER_GUID),
            None,
        )
        .with_prop(
            WIA_DPS_DOCUMENT_HANDLING_CAPABILITIES,
            PropValue::I4(FEED | DUP),
            None,
        )
        .with_prop(
            WIA_IPS_DOCUMENT_HANDLING_SELECT,
            PropValue::I4(FEEDER),
            rw_flags(FEEDER, duplex_flags),
        )
        .with_prop(WIA_IPS_PAGES, PropValue::I4(0), rw_range(0, 0, 100, 1))
        .with_prop(
            WIA_IPS_XRES,
            PropValue::I4(300),
            rw_list(300, vec![150, 300, 600]),
        )
        .with_prop(
            WIA_IPS_YRES,
            PropValue::I4(300),
            rw_list(300, vec![150, 300, 600]),
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
            rw_list(24, vec![1, 8, 16, 24, 48]),
        )
        .with_prop(
            WIA_IPA_TYMED,
            PropValue::I4(TYMED_FILE),
            rw_list(TYMED_FILE, vec![TYMED_FILE, TYMED_CALLBACK]),
        )
        .with_prop(
            WIA_IPA_FORMAT,
            PropValue::Guid(WIA_IMG_FMT_BMP_GUID),
            rw_guids(
                WIA_IMG_FMT_BMP_GUID,
                vec![
                    WIA_IMG_FMT_BMP_GUID,
                    WIA_IMG_FMT_RAW_GUID,
                    WIA_IMG_FMT_MEMORYBMP_GUID,
                    WIA_IMG_FMT_PNG_GUID,
                ],
            ),
        )
        .with_prop(WIA_IPS_MAX_HORIZONTAL_SIZE, PropValue::I4(8500), None)
        .with_prop(WIA_IPS_MAX_VERTICAL_SIZE, PropValue::I4(11000), None)
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
        .with_formats(vec![
            FormatPair {
                format: WIA_IMG_FMT_BMP_GUID,
                tymed: TYMED_FILE,
            },
            FormatPair {
                format: WIA_IMG_FMT_RAW_GUID,
                tymed: TYMED_FILE,
            },
            FormatPair {
                format: WIA_IMG_FMT_MEMORYBMP_GUID,
                tymed: TYMED_CALLBACK,
            },
            FormatPair {
                format: WIA_IMG_FMT_PNG_GUID,
                tymed: TYMED_FILE,
            },
        ])
}

#[test]
fn lists_filters_cameras_verifies_slot_identity_and_scans_flatbed_raw_and_encoded() {
    let fake = FakeWiaHandle::new();

    // Camera (`StiDeviceTypeDigitalCamera == 2`) is filtered out of listings.
    fake.add_device(FakeDeviceSpec::new(RawDeviceProps {
        dev_id: r"{6BDD1FC6}\CAM0".to_owned(),
        dev_type: Some(0x0002_0002),
        dev_name: Some("Camera".to_owned()),
        vend_desc: Some("Acme".to_owned()),
        dev_desc: Some("Cam".to_owned()),
        ..RawDeviceProps::default()
    }));

    // WSD scanner with empty `VEND_DESC`/`DEV_DESC` falls back to `WIA_WSD_*` properties.
    let mut spec = FakeDeviceSpec::new(RawDeviceProps {
        dev_id: r"\\?\wsd#scanner#0001".to_owned(),
        dev_type: Some(0x0001_0001),
        vend_desc: Some(String::new()),
        dev_desc: Some(String::new()),
        wsd_manufacturer: Some("Contoso".to_owned()),
        wsd_model_name: Some("NetScan 400".to_owned()),
        wsd_friendly_name: Some("Lobby NetScan".to_owned()),
        ..RawDeviceProps::default()
    });
    let flatbed = FakeItem::new(WIA_ITEM_TYPE_IMAGE | WIA_ITEM_TYPE_TRANSFER)
        .with_prop(
            WIA_IPA_ITEM_CATEGORY,
            PropValue::Guid(WIA_CATEGORY_FLATBED_GUID),
            None,
        )
        .with_prop(WIA_IPS_XRES, PropValue::I4(300), rw_list(300, vec![300]))
        .with_prop(WIA_IPS_YRES, PropValue::I4(300), rw_list(300, vec![300]))
        .with_prop(
            WIA_IPA_DATATYPE,
            PropValue::I4(WIA_DATA_COLOR),
            rw_list(WIA_DATA_COLOR, vec![WIA_DATA_COLOR]),
        )
        .with_prop(WIA_IPA_DEPTH, PropValue::I4(24), None)
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
        .with_prop(WIA_IPS_MAX_HORIZONTAL_SIZE, PropValue::I4(8500), None)
        .with_prop(WIA_IPS_MAX_VERTICAL_SIZE, PropValue::I4(11000), None)
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
        .with_downloads([
            // Call 1: raw BMP scan; even if a buggy flatbed offers a 2nd stream, batch stops at 1!
            FakeDownloadCall::ok(vec![
                FakeTransferEvent::Page {
                    chunks: vec![20, 58],
                    bytes: make_bmp_24bit_1x1(0x12, 0x34, 0x56),
                },
                FakeTransferEvent::Page {
                    chunks: vec![58],
                    bytes: make_bmp_24bit_1x1(0xFF, 0xFF, 0xFF),
                },
            ]),
            // Call 2: PNG encoded scan.
            FakeDownloadCall::ok(vec![FakeTransferEvent::Page {
                chunks: vec![12],
                bytes: b"\x89PNG\r\n\x1a\nIHDR".to_vec(),
            }]),
        ]);
    spec.add_child(ItemId::ROOT, flatbed);
    fake.add_device(spec);

    let mut backend = WiaBackend::from_fake(&fake).unwrap();
    let mut shared = WiaBackend::from_fake(&fake).unwrap();
    let listed = backend.list(Duration::from_secs(2)).unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].maker, "Contoso");
    assert_eq!(listed[0].model, "NetScan 400");
    assert_eq!(listed[0].name, "Lobby NetScan");

    // Stored record with mismatched model is refused with `ScanError::Gone`.
    let wrong_model = DeviceInfo::new(
        listed[0].id.clone(),
        listed[0].name.clone(),
        "Contoso".to_owned(),
        "Other Model".to_owned(),
        Driver::Wia,
    );
    assert_eq!(
        backend.open(&wrong_model, Duration::from_secs(2)).err(),
        Some(ScanError::Gone)
    );

    let mut opened = backend.open(&listed[0], Duration::from_secs(2)).unwrap();
    assert_eq!(
        shared.open(&listed[0], Duration::from_secs(2)).err(),
        Some(ScanError::Busy)
    );

    // 1. Raw flatbed scan -> `Done::OnePage` with 1 page.
    let (tx, rx) = sync_channel(8);
    let tracker = Tracker::new();
    let settings = Settings::new(Source::Flatbed, 300, Format::Rgb8);
    let done = opened.device.scan(&settings, 7, &tx, &tracker).unwrap();
    drop(tx);
    assert_eq!(done, Done::OnePage);
    let evs: Vec<Event> = rx.into_iter().collect();
    assert_eq!(evs.len(), 3);
    assert!(matches!(&evs[0], Event::Applied(a) if a.clamped == vec![Clamped::Paper(Paper::Auto)]));
    assert_eq!(evs[1], Event::Sheet(7));
    let Event::Page(p1) = &evs[2] else {
        panic!("expected Page");
    };
    assert_eq!(p1.sheet, 7);
    assert_eq!(p1.side, Side::Front);
    assert_eq!(p1.side_known, SideKnown::Inferred);
    assert_eq!(p1.dpi, (300.0, 300.0));
    let Pixels::Raw(img) = &p1.pixels else {
        panic!("expected Pixels::Raw");
    };
    assert_eq!(img.data(), &[0x12, 0x34, 0x56]);

    // 2. Encoded PNG flatbed scan -> `Pixels::Encoded(Png)`.
    let (tx2, rx2) = sync_channel(8);
    let mut enc_settings = Settings::new(Source::Flatbed, 300, Format::Rgb8);
    enc_settings.paper = Paper::Size(SizeMm::LETTER);
    enc_settings.encodings = vec![Encoding::Jpeg, Encoding::Png];
    let done2 = opened
        .device
        .scan(&enc_settings, 1, &tx2, &Tracker::new())
        .unwrap();
    drop(tx2);
    assert_eq!(done2, Done::OnePage);
    let evs2: Vec<Event> = rx2.into_iter().collect();
    let Event::Applied(app2) = &evs2[0] else {
        panic!("expected Applied");
    };
    assert_eq!(app2.settings.encodings, vec![Encoding::Png]);
    assert_eq!(
        app2.clamped,
        vec![Clamped::Encodings(vec![Encoding::Jpeg, Encoding::Png])]
    );
    let Event::Page(p2) = &evs2[2] else {
        panic!("expected Page");
    };
    let Pixels::Encoded(enc) = &p2.pixels else {
        panic!("expected Encoded");
    };
    assert_eq!(enc.kind, EncodedKind::Png);
}

#[test]
fn wsd_multi_download_duplex_back_first_warming_up_progress_and_deferred_sheet() {
    let fake = FakeWiaHandle::new();
    let mut spec = FakeDeviceSpec::new(scanner_props(r"\\?\wsd#adf#0002"));

    // WSD feeder rejects `WIA_IPS_PAGES = 0` (`range 2..=2, step 2`: 1 duplex sheet per Download)
    // and reports `BACK_FIRST` when `DUPLEX` is selected!
    let feeder = base_feeder_item(FEEDER | DUPLEX | BACK_FIRST)
        .with_prop(
            WIA_IPS_DOCUMENT_HANDLING_SELECT,
            PropValue::I4(FEEDER | BACK_FIRST),
            rw_flags(FEEDER | BACK_FIRST, FEEDER | DUPLEX | BACK_FIRST),
        )
        .with_prop(WIA_IPS_PAGES, PropValue::I4(2), rw_range(2, 2, 2, 2))
        .with_downloads([
            // Call 1: lamp warming up (`WIA_STATUS_WARMING_UP_HR`, 0 streams) -> emits Status once & retries!
            FakeDownloadCall::with_hr(Vec::new(), WIA_STATUS_WARMING_UP_HR),
            // Call 2: second warmup tick (`WIA_ERROR_WARMING_UP_HR`) -> does NOT duplicate Status!
            FakeDownloadCall::with_hr(Vec::new(), WIA_ERROR_WARMING_UP_HR),
            // Call 3: Sheet 10 (`Side::Back` then `Side::Front`, 2 streams == capped_n -> continues!).
            FakeDownloadCall::ok(vec![
                FakeTransferEvent::Page {
                    chunks: vec![30, 58],
                    bytes: make_bmp_24bit_1x1(1, 0, 0),
                },
                FakeTransferEvent::Page {
                    chunks: vec![58],
                    bytes: make_bmp_24bit_1x1(2, 0, 0),
                },
            ]),
            // Call 4: Sheet 11 (`Side::Back` then `Side::Front`, 2 streams == capped_n -> continues!).
            FakeDownloadCall::ok(vec![
                FakeTransferEvent::Page {
                    chunks: vec![58],
                    bytes: make_bmp_24bit_1x1(3, 0, 0),
                },
                FakeTransferEvent::Page {
                    chunks: vec![58],
                    bytes: make_bmp_24bit_1x1(4, 0, 0),
                },
            ]),
            // Call 5: Driver calls `GetNextStream`, discovers feeder is empty (`WIA_ERROR_PAPER_EMPTY_HR`),
            // and returns `E_FAIL_HR`! Must NOT emit an orphan `Event::Sheet(12)` and must restore
            // `Tracker` byte progress to 58!
            FakeDownloadCall::with_hr(
                vec![FakeTransferEvent::EmptyStreamError {
                    hr_status: WIA_ERROR_PAPER_EMPTY_HR,
                }],
                E_FAIL_HR,
            ),
            // Call 6 (for a separate scan on empty feeder): 0-byte `EmptyStreamError` -> `Err(FeederEmpty)`.
            FakeDownloadCall::with_hr(
                vec![FakeTransferEvent::EmptyStreamError {
                    hr_status: WIA_ERROR_PAPER_EMPTY_HR,
                }],
                E_FAIL_HR,
            ),
        ]);
    spec.add_child(ItemId::ROOT, feeder);
    fake.add_device(spec);

    let mut backend = WiaBackend::from_fake(&fake).unwrap();
    let dev_info = &backend.list(Duration::from_secs(2)).unwrap()[0];
    let mut opened = backend.open(dev_info, Duration::from_secs(2)).unwrap();

    let mut settings = Settings::new(Source::Feeder, 300, Format::Rgb8);
    settings.duplex = true;
    settings.paper = Paper::Size(SizeMm::LETTER);

    let (tx, rx) = sync_channel(16);
    let tracker = Tracker::new();
    let done = opened.device.scan(&settings, 10, &tx, &tracker).unwrap();
    drop(tx);
    assert_eq!(done, Done::FeederEmpty);

    // Byte progress restored to last completed page's 58 bytes despite trailing EmptyStreamError!
    let snap = tracker.snapshot();
    assert_eq!(snap.unit, Unit::Bytes);
    assert_eq!(snap.done, 58);
    assert_eq!(snap.total, Some(58));

    let events: Vec<Event> = rx.into_iter().collect();
    // Applied + 1 WarmingUp + Sheet(10) + Back + Front + Sheet(11) + Back + Front = 8 events (no Sheet(12)!).
    assert_eq!(events.len(), 8);
    assert!(matches!(events[0], Event::Applied(_)));
    assert_eq!(events[1], Event::Status(Status::WarmingUp));
    assert_eq!(events[2], Event::Sheet(10));
    assert!(matches!(&events[3], Event::Page(p) if p.sheet == 10 && p.side == Side::Back));
    assert!(matches!(&events[4], Event::Page(p) if p.sheet == 10 && p.side == Side::Front));
    assert_eq!(events[5], Event::Sheet(11));
    assert!(matches!(&events[6], Event::Page(p) if p.sheet == 11 && p.side == Side::Back));
    assert!(matches!(&events[7], Event::Page(p) if p.sheet == 11 && p.side == Side::Front));

    // Empty feeder before any page returns `Err(ScanError::FeederEmpty)` after `Event::Applied` only.
    let (tx_empty, rx_empty) = sync_channel(8);
    let err = opened
        .device
        .scan(&settings, 1, &tx_empty, &Tracker::new())
        .unwrap_err();
    drop(tx_empty);
    assert_eq!(err, ScanError::FeederEmpty);
    let empty_events: Vec<Event> = rx_empty.into_iter().collect();
    assert_eq!(empty_events.len(), 1);
    assert!(matches!(empty_events[0], Event::Applied(_)));
}

#[test]
fn cancel_mid_front_completes_duplex_sheet_and_issues_stop_feeder() {
    let fake = FakeWiaHandle::new();
    let mut spec = FakeDeviceSpec::new(scanner_props(r"{6BDD1FC6}\CANCEL"));
    let cmd_log = Arc::clone(&spec.command_log);

    let (front_sent_tx, front_sent_rx) = sync_channel::<()>(1);
    let (cancel_done_tx, cancel_done_rx) = sync_channel::<()>(1);
    let cancel_done_rx = Arc::new(Mutex::new(cancel_done_rx));
    let sheet2_fed = Arc::new(AtomicUsize::new(0));
    let sheet2_fed_clone = Arc::clone(&sheet2_fed);

    let feeder = base_feeder_item(FEEDER | DUPLEX).with_downloads([FakeDownloadCall::ok(vec![
        FakeTransferEvent::Page {
            chunks: vec![58],
            bytes: make_bmp_24bit_1x1(10, 20, 30),
        },
        FakeTransferEvent::Hook(Arc::new(move || {
            let _ = front_sent_tx.send(());
            let _ = cancel_done_rx.lock().unwrap().recv();
        })),
        FakeTransferEvent::Page {
            chunks: vec![58],
            bytes: make_bmp_24bit_1x1(40, 50, 60),
        },
        // Sheet 2: `on_next_stream` must return `Cancel` (`S_FALSE`) before feeding!
        FakeTransferEvent::Page {
            chunks: vec![58],
            bytes: make_bmp_24bit_1x1(70, 80, 90),
        },
        FakeTransferEvent::Hook(Arc::new(move || {
            sheet2_fed_clone.fetch_add(1, Ordering::SeqCst);
        })),
    ])]);
    spec.add_child(ItemId::ROOT, feeder);
    fake.add_device(spec);

    let mut backend = WiaBackend::from_fake(&fake).unwrap();
    let dev_info = backend.list(Duration::from_secs(2)).unwrap().remove(0);
    let mut opened = backend.open(&dev_info, Duration::from_secs(2)).unwrap();

    let mut settings = Settings::new(Source::Feeder, 300, Format::Rgb8);
    settings.duplex = true;
    settings.paper = Paper::Size(SizeMm::LETTER);

    let (tx, rx) = sync_channel(16);
    let tracker = Tracker::new();
    let cancel_handle = tracker.clone();

    let scan_thread = thread::spawn(move || opened.device.scan(&settings, 1, &tx, &tracker));

    // Wait until Sheet 1 Front has been delivered, then cancel mid-sheet!
    front_sent_rx.recv().unwrap();
    cancel_handle.cancel();
    cancel_done_tx.send(()).unwrap();

    let done = scan_thread.join().unwrap().unwrap();
    assert_eq!(done, Done::Cancelled);
    assert_eq!(sheet2_fed.load(Ordering::SeqCst), 0);
    assert_eq!(*cmd_log.lock().unwrap(), vec![WIA_CMD_STOP_FEEDER_GUID]);

    let events: Vec<Event> = rx.into_iter().collect();
    // Applied + Sheet(1) + Front + Back = 4 events (both sides of Sheet 1 delivered, zero of Sheet 2).
    assert_eq!(events.len(), 4);
    assert_eq!(events[1], Event::Sheet(1));
    assert!(matches!(&events[2], Event::Page(p) if p.sheet == 1 && p.side == Side::Front));
    assert!(matches!(&events[3], Event::Page(p) if p.sheet == 1 && p.side == Side::Back));
}

#[test]
fn bounded_channel_holds_backpressure_and_receiver_drop_cancels_cleanly() {
    let fake = FakeWiaHandle::new();
    let mut spec = FakeDeviceSpec::new(scanner_props(r"{6BDD1FC6}\BACKPRESSURE"));
    let cmd_log = Arc::clone(&spec.command_log);
    let pages_started = Arc::new(AtomicUsize::new(0));

    let mut events = Vec::new();
    for idx in 0..5u8 {
        let counter = Arc::clone(&pages_started);
        events.push(FakeTransferEvent::Hook(Arc::new(move || {
            counter.fetch_add(1, Ordering::SeqCst);
        })));
        events.push(FakeTransferEvent::Page {
            chunks: vec![58],
            bytes: make_bmp_24bit_1x1(idx, idx, idx),
        });
    }

    let feeder = base_feeder_item(FEEDER).with_downloads([FakeDownloadCall::ok(events)]);
    spec.add_child(ItemId::ROOT, feeder);
    fake.add_device(spec);

    let mut backend = WiaBackend::from_fake(&fake).unwrap();
    let dev_info = backend.list(Duration::from_secs(2)).unwrap().remove(0);
    let mut opened = backend.open(&dev_info, Duration::from_secs(2)).unwrap();

    let mut settings = Settings::new(Source::Feeder, 300, Format::Rgb8);
    settings.paper = Paper::Size(SizeMm::LETTER);

    // Channel bound 1: `Applied` takes the 1 buffer slot, `Sheet(1)` blocks in `send`!
    let (tx, rx) = sync_channel(1);
    let scan_thread = thread::spawn(move || opened.device.scan(&settings, 1, &tx, &Tracker::new()));

    // Give STA thread a moment to block at `Sheet(1)` send; Page 2 must NOT have started!
    assert!(matches!(rx.recv().unwrap(), Event::Applied(_)));
    assert_eq!(rx.recv().unwrap(), Event::Sheet(1));
    assert!(pages_started.load(Ordering::SeqCst) <= 2);

    // Drop the receiver mid-batch: batch ends immediately as `Ok(Done::Cancelled)` and stops feeder!
    drop(rx);
    let done = scan_thread.join().unwrap().unwrap();
    assert_eq!(done, Done::Cancelled);
    assert!(cmd_log.lock().unwrap().contains(&WIA_CMD_STOP_FEEDER_GUID));
    assert!(pages_started.load(Ordering::SeqCst) <= 2);
}

#[test]
fn callback_to_file_fallback_all_five_raw_formats_overflow_and_error_mapping() {
    let fake = FakeWiaHandle::new();

    // Device 1: offers only `MEMORYBMP` over `TYMED_CALLBACK` + `TYMED_FILE`; refuses `TYMED_CALLBACK`
    // on `Download` with `E_INVALIDARG_HR`, then succeeds on `TYMED_FILE` fallback!
    let mut spec_fb = FakeDeviceSpec::new(scanner_props(r"{6BDD1FC6}\FALLBACK"));
    let mut raw_dib = Vec::new();
    raw_dib.extend_from_slice(&40u32.to_le_bytes());
    raw_dib.extend_from_slice(&1i32.to_le_bytes());
    raw_dib.extend_from_slice(&1i32.to_le_bytes());
    raw_dib.extend_from_slice(&1u16.to_le_bytes());
    raw_dib.extend_from_slice(&24u16.to_le_bytes());
    raw_dib.extend_from_slice(&0u32.to_le_bytes());
    raw_dib.extend_from_slice(&4u32.to_le_bytes());
    raw_dib.extend_from_slice(&[0u8; 16]);
    raw_dib.extend_from_slice(&[0x99, 0x88, 0x77, 0x00]);

    let fb_item = FakeItem::new(WIA_ITEM_TYPE_IMAGE | WIA_ITEM_TYPE_TRANSFER)
        .with_prop(
            WIA_IPA_ITEM_CATEGORY,
            PropValue::Guid(WIA_CATEGORY_FLATBED_GUID),
            None,
        )
        .with_prop(WIA_IPS_XRES, PropValue::I4(300), rw_list(300, vec![300]))
        .with_prop(WIA_IPS_YRES, PropValue::I4(300), rw_list(300, vec![300]))
        .with_prop(
            WIA_IPA_DATATYPE,
            PropValue::I4(WIA_DATA_COLOR),
            rw_list(WIA_DATA_COLOR, vec![WIA_DATA_COLOR]),
        )
        .with_prop(WIA_IPA_DEPTH, PropValue::I4(24), None)
        .with_prop(
            WIA_IPA_TYMED,
            PropValue::I4(TYMED_CALLBACK),
            rw_list(TYMED_CALLBACK, vec![TYMED_CALLBACK, TYMED_FILE]),
        )
        .with_prop(
            WIA_IPA_FORMAT,
            PropValue::Guid(WIA_IMG_FMT_MEMORYBMP_GUID),
            rw_guids(WIA_IMG_FMT_MEMORYBMP_GUID, vec![WIA_IMG_FMT_MEMORYBMP_GUID]),
        )
        .with_formats(vec![
            FormatPair {
                format: WIA_IMG_FMT_MEMORYBMP_GUID,
                tymed: TYMED_CALLBACK,
            },
            FormatPair {
                format: WIA_IMG_FMT_MEMORYBMP_GUID,
                tymed: TYMED_FILE,
            },
        ])
        .with_downloads([
            FakeDownloadCall {
                events: Vec::new(),
                return_hr: S_OK_HR,
                cancel_hr: None,
                require_tymed: Some((TYMED_FILE, E_INVALIDARG_HR)),
                stdout_chatter: None,
            },
            FakeDownloadCall::ok(vec![FakeTransferEvent::Page {
                chunks: vec![44],
                bytes: raw_dib,
            }]),
        ]);
    spec_fb.add_child(ItemId::ROOT, fb_item);
    fake.add_device(spec_fb);

    // Device 2: `WiaImgFmt_RAW` feeder exercising all 5 formats + mid-batch jam (`E_FAIL` with
    // callback `WIA_ERROR_PAPER_JAM_HR`), Learn `COVER_OPEN`, Learn `MULTI_FEED`, `StreamOverflow`,
    // and `0x8021_0015` endorser counter!
    let mut spec_raw = FakeDeviceSpec::new(scanner_props(r"{6BDD1FC6}\RAW5"));
    let raw_item = base_feeder_item(FEEDER)
        .with_prop(
            WIA_IPA_FORMAT,
            PropValue::Guid(WIA_IMG_FMT_RAW_GUID),
            rw_guids(WIA_IMG_FMT_RAW_GUID, vec![WIA_IMG_FMT_RAW_GUID]),
        )
        .with_formats(vec![FormatPair {
            format: WIA_IMG_FMT_RAW_GUID,
            tymed: TYMED_FILE,
        }])
        .with_downloads([
            // 1. Bw1
            FakeDownloadCall::with_hr(
                vec![FakeTransferEvent::Page {
                    chunks: vec![81],
                    bytes: make_wraw(
                        300,
                        8,
                        1,
                        1,
                        1,
                        WIA_DATA_THRESHOLD,
                        1,
                        WIA_PHOTO_WHITE_1,
                        &[0xA5],
                    ),
                }],
                WIA_STATUS_END_OF_MEDIA_HR,
            ),
            // 2. Gray8
            FakeDownloadCall::with_hr(
                vec![FakeTransferEvent::Page {
                    chunks: vec![82],
                    bytes: make_wraw(
                        300,
                        2,
                        1,
                        8,
                        1,
                        WIA_DATA_GRAYSCALE,
                        8,
                        WIA_PHOTO_WHITE_1,
                        &[0x10, 0xF0],
                    ),
                }],
                WIA_STATUS_END_OF_MEDIA_HR,
            ),
            // 3. Gray16
            FakeDownloadCall::with_hr(
                vec![FakeTransferEvent::Page {
                    chunks: vec![82],
                    bytes: make_wraw(
                        300,
                        1,
                        1,
                        16,
                        1,
                        WIA_DATA_GRAYSCALE,
                        16,
                        WIA_PHOTO_WHITE_1,
                        &[0x34, 0x12],
                    ),
                }],
                WIA_STATUS_END_OF_MEDIA_HR,
            ),
            // 4. Rgb8
            FakeDownloadCall::with_hr(
                vec![FakeTransferEvent::Page {
                    chunks: vec![83],
                    bytes: make_wraw(
                        300,
                        1,
                        1,
                        24,
                        3,
                        WIA_DATA_RAW_BGR,
                        8,
                        WIA_PHOTO_WHITE_1,
                        &[0x03, 0x02, 0x01],
                    ),
                }],
                WIA_STATUS_END_OF_MEDIA_HR,
            ),
            // 5. Rgb16
            FakeDownloadCall::with_hr(
                vec![FakeTransferEvent::Page {
                    chunks: vec![86],
                    bytes: make_wraw(
                        300,
                        1,
                        1,
                        48,
                        3,
                        WIA_DATA_RAW_RGB,
                        16,
                        WIA_PHOTO_WHITE_1,
                        &[1, 2, 3, 4, 5, 6],
                    ),
                }],
                WIA_STATUS_END_OF_MEDIA_HR,
            ),
            // 6. Page 1 delivered, then Page 2 jams (`hrErrorStatus = JAM`, `Download = E_FAIL`).
            FakeDownloadCall::with_hr(
                vec![
                    FakeTransferEvent::Page {
                        chunks: vec![83],
                        bytes: make_wraw(
                            300,
                            1,
                            1,
                            24,
                            3,
                            WIA_DATA_COLOR,
                            8,
                            WIA_PHOTO_WHITE_1,
                            &[9, 8, 7],
                        ),
                    },
                    FakeTransferEvent::EmptyStreamError {
                        hr_status: WIA_ERROR_PAPER_JAM_HR,
                    },
                ],
                E_FAIL_HR,
            ),
            // 7. Microsoft Learn misprinted `COVER_OPEN` (`0x8021_0016`).
            FakeDownloadCall::with_hr(Vec::new(), LEARN_WIA_ERROR_COVER_OPEN_HR),
            // 8. Microsoft Learn misprinted `MULTI_FEED` (`0x8021_0020`).
            FakeDownloadCall::with_hr(Vec::new(), LEARN_WIA_ERROR_MULTI_FEED_HR),
            // 9. Stream overflow (`STG_E_MEDIUMFULL`).
            FakeDownloadCall::ok(vec![FakeTransferEvent::StreamOverflow {
                return_hr: STG_E_MEDIUMFULL_HR,
            }]),
            // 10. `0x8021_0015` during `Download` -> `Driver` endorser counter (NOT `Gone`!).
            FakeDownloadCall::with_hr(Vec::new(), WIA_NO_DEVICE_OR_ENDORSER_HR),
        ]);
    spec_raw.add_child(ItemId::ROOT, raw_item);
    fake.add_device(spec_raw);

    // Device 3: `0x8021_0015` (`WIA_S_NO_DEVICE_AVAILABLE`) on `CreateDevice` -> `ScanError::Gone`.
    let mut spec_gone = FakeDeviceSpec::new(scanner_props(r"{6BDD1FC6}\GONE"));
    spec_gone.open_error_hr = Some(WIA_NO_DEVICE_OR_ENDORSER_HR);
    fake.add_device(spec_gone);

    let mut backend = WiaBackend::from_fake(&fake).unwrap();
    let listed = backend.list(Duration::from_secs(2)).unwrap();
    assert_eq!(listed.len(), 3);

    // Test Device 1 (`TYMED_CALLBACK` -> `TYMED_FILE` fallback).
    {
        let mut opened = backend.open(&listed[0], Duration::from_secs(2)).unwrap();
        let (tx, rx) = sync_channel(8);
        let done = opened
            .device
            .scan(
                &Settings::new(Source::Flatbed, 300, Format::Rgb8),
                1,
                &tx,
                &Tracker::new(),
            )
            .unwrap();
        drop(tx);
        assert_eq!(done, Done::OnePage);
        let evs: Vec<Event> = rx.into_iter().collect();
        let Event::Page(p) = &evs[2] else {
            panic!("expected Page");
        };
        let Pixels::Raw(img) = &p.pixels else {
            panic!("expected Raw");
        };
        assert_eq!(img.data(), &[0x77, 0x88, 0x99]);
    }

    // Test Device 2 (all 5 raw formats + error mappings).
    {
        let mut opened = backend.open(&listed[1], Duration::from_secs(2)).unwrap();
        for (fmt, expected_bytes) in [
            (Format::Bw1, &[0xA5u8][..]),
            (Format::Gray8, &[0x10, 0xF0][..]),
            (Format::Gray16, &[0x34, 0x12][..]),
            (Format::Rgb8, &[0x01, 0x02, 0x03][..]),
            (Format::Rgb16, &[1, 2, 3, 4, 5, 6][..]),
        ] {
            let mut s = Settings::new(Source::Feeder, 300, fmt);
            s.paper = Paper::Size(SizeMm::LETTER);
            let (tx, rx) = sync_channel(8);
            let done = opened.device.scan(&s, 1, &tx, &Tracker::new()).unwrap();
            drop(tx);
            assert_eq!(done, Done::FeederEmpty);
            let evs: Vec<Event> = rx.into_iter().collect();
            let Event::Page(p) = &evs[2] else {
                panic!("expected Page");
            };
            let Pixels::Raw(img) = &p.pixels else {
                panic!("expected Raw");
            };
            assert_eq!(img.format(), fmt);
            assert_eq!(img.data(), expected_bytes);
        }

        let mut s = Settings::new(Source::Feeder, 300, Format::Rgb8);
        s.paper = Paper::Size(SizeMm::LETTER);

        // 6. Mid-batch jam preserves Page 1 and returns `Err(ScanError::Jammed)`.
        let (tx_jam, rx_jam) = sync_channel(8);
        assert_eq!(
            opened.device.scan(&s, 1, &tx_jam, &Tracker::new()),
            Err(ScanError::Jammed)
        );
        drop(tx_jam);
        let jam_evs: Vec<Event> = rx_jam.into_iter().collect();
        assert_eq!(jam_evs.len(), 3); // Applied + Sheet(1) + Page(1)

        // 7 & 8. Learn `CoverOpen` and `DoubleFeed`.
        let (tx_cov, _rx_cov) = sync_channel(8);
        assert_eq!(
            opened.device.scan(&s, 1, &tx_cov, &Tracker::new()),
            Err(ScanError::CoverOpen)
        );
        let (tx_df, _rx_df) = sync_channel(8);
        assert_eq!(
            opened.device.scan(&s, 1, &tx_df, &Tracker::new()),
            Err(ScanError::DoubleFeed)
        );

        // 9. Stream overflow (`STG_E_MEDIUMFULL`) -> `Driver` naming lower resolution.
        let (tx_ov, _rx_ov) = sync_channel(8);
        let ov_err = opened
            .device
            .scan(&s, 1, &tx_ov, &Tracker::new())
            .unwrap_err();
        assert!(
            matches!(&ov_err, ScanError::Driver { message, shown: false } if message.contains("lower resolution")),
            "unexpected overflow error: {ov_err:?}"
        );

        // 10. `0x8021_0015` during `Download` -> `Driver` endorser counter.
        let (tx_end, _rx_end) = sync_channel(8);
        let end_err = opened
            .device
            .scan(&s, 1, &tx_end, &Tracker::new())
            .unwrap_err();
        assert!(
            matches!(&end_err, ScanError::Driver { message, shown: false } if message.contains("ENDORSER")),
            "unexpected 0x80210015 download error: {end_err:?}"
        );
    }

    // Test Device 3 (`0x8021_0015` on `CreateDevice` -> `ScanError::Gone`).
    assert_eq!(
        backend.open(&listed[2], Duration::from_secs(2)).err(),
        Some(ScanError::Gone)
    );

    // Invalid IDs return `BadId` immediately without touching the session.
    let bad = DeviceInfo::new(
        DeviceId("twain64:Sample".to_owned()),
        "Sample".to_owned(),
        "Maker".to_owned(),
        "Model".to_owned(),
        Driver::Wia,
    );
    assert_eq!(
        backend.open(&bad, Duration::from_secs(2)).err(),
        Some(ScanError::BadId)
    );
}

#[test]
fn callback_error_warming_up_preserves_same_call_page_and_flatbed_extra_e_abort_and_hw_cancel() {
    let fake = FakeWiaHandle::new();
    let mut spec = FakeDeviceSpec::new(scanner_props(r"{6BDD1FC6}\0042"));
    let cmd_log = Arc::clone(&spec.command_log);

    let mut second_flatbed_call = FakeDownloadCall::ok(vec![
        FakeTransferEvent::Status {
            msg: WIA_TRANSFER_MSG_STATUS,
            transferred_bytes: 0,
            hr_status: WIA_ERROR_WARMING_UP_HR,
        },
        FakeTransferEvent::Page {
            chunks: vec![58],
            bytes: make_bmp_24bit_1x1(0x44, 0x55, 0x66),
        },
        // Bogus second flatbed stream inside the same `Download`: `BatchSink` cancels with
        // `S_FALSE`, and this minidriver translates `S_FALSE` into `E_ABORT_HR`.
        FakeTransferEvent::Page {
            chunks: vec![58],
            bytes: make_bmp_24bit_1x1(0x99, 0x88, 0x77),
        },
    ]);
    second_flatbed_call.cancel_hr = Some(E_ABORT_HR);

    let flatbed = FakeItem::new(WIA_ITEM_TYPE_IMAGE | WIA_ITEM_TYPE_TRANSFER)
        .with_prop(
            WIA_IPA_ITEM_CATEGORY,
            PropValue::Guid(WIA_CATEGORY_FLATBED_GUID),
            None,
        )
        .with_prop(WIA_IPS_XRES, PropValue::I4(300), rw_list(300, vec![300]))
        .with_prop(WIA_IPS_YRES, PropValue::I4(300), rw_list(300, vec![300]))
        .with_prop(
            WIA_IPA_DATATYPE,
            PropValue::I4(WIA_DATA_COLOR),
            rw_list(WIA_DATA_COLOR, vec![WIA_DATA_COLOR]),
        )
        .with_prop(WIA_IPA_DEPTH, PropValue::I4(24), None)
        .with_prop(WIA_IPA_TYMED, PropValue::I4(TYMED_FILE), None)
        .with_prop(WIA_IPA_FORMAT, PropValue::Guid(WIA_IMG_FMT_BMP_GUID), None)
        .with_prop(WIA_IPS_MAX_HORIZONTAL_SIZE, PropValue::I4(8500), None)
        .with_prop(WIA_IPS_MAX_VERTICAL_SIZE, PropValue::I4(11000), None)
        .with_formats(vec![FormatPair {
            format: WIA_IMG_FMT_BMP_GUID,
            tymed: TYMED_FILE,
        }])
        .with_downloads([
            // Call 1: 0-page `WIA_ERROR_WARMING_UP_HR` callback status + generic `E_FAIL_HR`
            // return from `Download` -> emits `Status::WarmingUp` and retries!
            FakeDownloadCall::with_hr(
                vec![FakeTransferEvent::Status {
                    msg: WIA_TRANSFER_MSG_STATUS,
                    transferred_bytes: 0,
                    hr_status: WIA_ERROR_WARMING_UP_HR,
                }],
                E_FAIL_HR,
            ),
            second_flatbed_call,
        ]);

    let feeder = FakeItem::new(WIA_ITEM_TYPE_IMAGE | WIA_ITEM_TYPE_TRANSFER)
        .with_prop(
            WIA_IPA_ITEM_CATEGORY,
            PropValue::Guid(WIA_CATEGORY_FEEDER_GUID),
            None,
        )
        .with_prop(
            WIA_IPS_DOCUMENT_HANDLING_SELECT,
            PropValue::I4(FEEDER),
            rw_flags(FEEDER, FEEDER),
        )
        .with_prop(WIA_IPS_PAGES, PropValue::I4(0), rw_range(0, 0, 100, 1))
        .with_prop(WIA_IPS_XRES, PropValue::I4(300), rw_list(300, vec![300]))
        .with_prop(WIA_IPS_YRES, PropValue::I4(300), rw_list(300, vec![300]))
        .with_prop(
            WIA_IPA_DATATYPE,
            PropValue::I4(WIA_DATA_COLOR),
            rw_list(WIA_DATA_COLOR, vec![WIA_DATA_COLOR]),
        )
        .with_prop(WIA_IPA_DEPTH, PropValue::I4(24), None)
        .with_prop(WIA_IPA_TYMED, PropValue::I4(TYMED_FILE), None)
        .with_prop(WIA_IPA_FORMAT, PropValue::Guid(WIA_IMG_FMT_BMP_GUID), None)
        .with_prop(WIA_IPS_MAX_HORIZONTAL_SIZE, PropValue::I4(8500), None)
        .with_prop(WIA_IPS_MAX_VERTICAL_SIZE, PropValue::I4(11000), None)
        .with_formats(vec![FormatPair {
            format: WIA_IMG_FMT_BMP_GUID,
            tymed: TYMED_FILE,
        }])
        .with_downloads([
            // Scanner front-panel Stop button pressed between sheets: Page 1 finishes cleanly,
            // then `Download` returns `HRESULT_FROM_WIN32(ERROR_CANCELLED)`.
            FakeDownloadCall::with_hr(
                vec![FakeTransferEvent::Page {
                    chunks: vec![58],
                    bytes: make_bmp_24bit_1x1(0x12, 0x34, 0x56),
                }],
                ERROR_CANCELLED_HR,
            ),
        ]);

    spec.add_child(ItemId::ROOT, flatbed);
    spec.add_child(ItemId::ROOT, feeder);
    fake.add_device(spec);

    let mut backend = WiaBackend::from_fake(&fake).unwrap();
    let listed = backend.list(Duration::from_secs(2)).unwrap();
    let mut opened = backend.open(&listed[0], Duration::from_secs(2)).unwrap();

    // 1. Flatbed: 0-page `WIA_ERROR_WARMING_UP_HR` retries, same-call `WIA_ERROR_WARMING_UP_HR`
    // does not drop Page 1, and stopping a bogus 2nd flatbed stream that returns `E_ABORT_HR`
    // finishes cleanly as `Done::OnePage`!
    let mut fb = Settings::new(Source::Flatbed, 300, Format::Rgb8);
    fb.paper = Paper::Size(SizeMm::LETTER);
    let (tx, rx) = sync_channel(16);
    let done = opened
        .device
        .scan(&fb, 1, &tx, &Tracker::new())
        .expect("flatbed scan succeeds");
    drop(tx);
    assert_eq!(done, Done::OnePage);
    let fb_events: Vec<Event> = rx.into_iter().collect();
    assert!(matches!(fb_events[0], Event::Applied(_)));
    assert_eq!(fb_events[1], Event::Status(Status::WarmingUp));
    assert_eq!(fb_events[2], Event::Sheet(1));
    let Event::Page(fb_page) = &fb_events[3] else {
        panic!("expected Page(1), got {:?}", fb_events.get(3));
    };
    let Pixels::Raw(fb_img) = &fb_page.pixels else {
        panic!("expected raw BMP");
    };
    assert_eq!(fb_img.data(), &[0x44, 0x55, 0x66]);
    assert_eq!(fb_events.len(), 4);

    // 2. Feeder: hardware-panel `ERROR_CANCELLED_HR` between sheets preserves Page 1, issues
    // `WIA_CMD_STOP_FEEDER`, and returns `Ok(Done::Cancelled)`.
    let mut adf = Settings::new(Source::Feeder, 300, Format::Rgb8);
    adf.paper = Paper::Size(SizeMm::LETTER);
    let (tx2, rx2) = sync_channel(16);
    let adf_done = opened
        .device
        .scan(&adf, 1, &tx2, &Tracker::new())
        .expect("hardware cancel returns Ok(Done::Cancelled)");
    drop(tx2);
    assert_eq!(adf_done, Done::Cancelled);
    let adf_events: Vec<Event> = rx2.into_iter().collect();
    assert!(matches!(adf_events[0], Event::Applied(_)));
    assert_eq!(adf_events[1], Event::Sheet(1));
    assert!(matches!(adf_events[2], Event::Page(_)));
    assert_eq!(adf_events.len(), 3);
    assert!(cmd_log.lock().unwrap().contains(&WIA_CMD_STOP_FEEDER_GUID));
}
