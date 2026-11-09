//! The fake backend's devices: listed under a deadline, opened as a stored device is (a renamed
//! one found by its maker and model, a name now another model's taken as gone), their caps and
//! their settings window.

use std::{
    num::NonZeroU32,
    panic::{self, AssertUnwindSafe},
    time::{Duration, Instant},
};

use scan_device::{
    Backend, Caps, DeviceId, DeviceInfo, Driver, Format, Opened, Resolutions, ScanError, SizeMm,
    Source, SourceCaps, WindowClosed, WindowOwner,
    fake::{FakeBackend, FakeDevice, Sheet},
};

/// A deadline no listing here comes near.
const SOON: Duration = Duration::from_secs(5);

fn id(id: &str) -> DeviceId {
    DeviceId(id.to_owned())
}

/// The `DeviceInfo` an app stored for `fake:<name>`, of `maker` and `model`.
fn stored(name: &str, maker: &str, model: &str) -> DeviceInfo {
    FakeDevice::new(name, maker, model).info()
}

/// Opens `device` through `backend`, giving the id opened and the stored id it was renamed from.
fn open(
    backend: &mut FakeBackend,
    device: &DeviceInfo,
) -> Result<(DeviceId, Option<DeviceId>), ScanError> {
    backend
        .open(device, SOON)
        .map(|opened| (opened.info.id, opened.renamed_from))
}

fn assert_send<T: Send>() {}
fn assert_send_sync<T: Send + Sync>() {}

#[test]
fn the_backend_and_its_devices_are_send() {
    assert_send::<FakeBackend>();
    assert_send_sync::<FakeDevice>();
    assert_send::<Sheet>();
    // The device it opens, as the trait has every device.
    let mut backend = FakeBackend::new().with(FakeDevice::new("a", "M", "N"));
    let opened: Opened = backend.open(&stored("a", "M", "N"), SOON).unwrap();
    std::thread::spawn(move || drop(opened)).join().unwrap();
}

#[test]
fn a_listing_gives_each_listed_device_s_info() {
    let mut backend = FakeBackend::default();
    assert_eq!(backend.list(SOON), Ok(Vec::new()));

    let mut backend = FakeBackend::new()
        .with(FakeDevice::new("libusb:001:004", "Fujitsu", "fi-6130dj"))
        .with(FakeDevice::new("alias", "Maker", "Hidden").unlisted())
        .with(FakeDevice::new("0", "Noname", "flatbed scanner"));
    let listed = backend.list(SOON).unwrap();
    let ids: Vec<&str> = listed.iter().map(|info| info.id.0.as_str()).collect();
    assert_eq!(ids, ["fake:libusb:001:004", "fake:0"]);
    let first = &listed[0];
    assert_eq!(first.name, "Fujitsu fi-6130dj");
    assert_eq!(
        (first.maker.as_str(), first.model.as_str()),
        ("Fujitsu", "fi-6130dj")
    );
    assert_eq!(first.driver, Driver::Fake);
    assert_eq!(*first, stored("libusb:001:004", "Fujitsu", "fi-6130dj"));
    assert_eq!(listed[1].name, "Noname flatbed scanner");
}

#[test]
fn a_listing_answers_after_its_time_under_the_deadline() {
    let mut backend = FakeBackend::new()
        .with(FakeDevice::new("a", "M", "N"))
        .listing_takes(Duration::from_millis(50));
    let started = Instant::now();
    assert_eq!(backend.list(Duration::from_secs(5)).unwrap().len(), 1);
    assert!(started.elapsed() >= Duration::from_millis(50));
}

#[test]
fn a_listing_slower_than_its_deadline_waits_it_out_and_times_out() {
    // Ten times slower than the deadline, so a slow runner can't make it in time.
    let deadline = Duration::from_millis(200);
    let mut backend = FakeBackend::new()
        .with(FakeDevice::new("a", "M", "N"))
        .listing_takes(deadline * 10);
    let started = Instant::now();
    assert_eq!(backend.list(deadline), Err(ScanError::TimedOut));
    let waited = started.elapsed();
    assert!(waited >= deadline, "{waited:?}");
    assert!(waited < deadline * 10, "{waited:?}");

    // And in `open`, whose listing comes before the device opens.
    let started = Instant::now();
    assert_eq!(
        backend.open(&stored("a", "M", "N"), deadline).unwrap_err(),
        ScanError::TimedOut
    );
    assert!(started.elapsed() >= deadline);

    // A listing as long as its deadline is past it.
    let mut backend = FakeBackend::new().listing_takes(deadline);
    assert_eq!(backend.list(deadline), Err(ScanError::TimedOut));
}

#[test]
fn a_zero_deadline_times_out() {
    let mut backend = FakeBackend::new().with(FakeDevice::new("a", "M", "N"));
    assert_eq!(backend.list(Duration::ZERO), Err(ScanError::TimedOut));
    assert_eq!(
        backend
            .open(&stored("a", "M", "N"), Duration::ZERO)
            .unwrap_err(),
        ScanError::TimedOut
    );
}

#[test]
fn a_backend_not_installed_says_so_before_anything_else() {
    let mut backend = FakeBackend::new()
        .with(FakeDevice::new("a", "M", "N"))
        .not_installed();
    let not_installed = ScanError::NotInstalled {
        what: "the fake driver".to_owned(),
    };
    assert_eq!(backend.list(SOON), Err(not_installed.clone()));
    assert_eq!(
        backend.open(&stored("a", "M", "N"), SOON).unwrap_err(),
        not_installed
    );
    // Before the id is read, and before any listing.
    let mut bad = stored("a", "M", "N");
    bad.id = id("sane:test:0");
    assert_eq!(
        backend.open(&bad, Duration::ZERO).unwrap_err(),
        not_installed
    );
}

#[test]
fn the_stored_device_listed_opens_with_the_listing_s_info() {
    let mut backend = FakeBackend::new()
        .with(FakeDevice::new("libusb:001:004", "Maker", "Model"))
        .with(FakeDevice::new("libusb:001:005", "Maker", "Model"));
    // The app's stored copy, its name to show changed by the user.
    let mut kept = stored("libusb:001:004", "Maker", "Model");
    kept.name = "Front desk".to_owned();
    let opened = backend.open(&kept, SOON).unwrap();
    assert_eq!(opened.info, stored("libusb:001:004", "Maker", "Model"));
    assert_eq!(opened.renamed_from, None);
    let debug = format!("{opened:?}");
    assert!(debug.contains("fake:libusb:001:004"), "{debug}");
}

#[test]
fn a_renamed_usb_device_is_found_by_its_maker_and_model() {
    // Unplugged from 001:004 and plugged in again as 001:005.
    let mut backend = FakeBackend::new()
        .with(FakeDevice::new("libusb:001:005", "Fujitsu", "fi-6130dj"))
        .with(FakeDevice::new("libusb:002:001", "Fujitsu", "fi-7160"))
        .with(FakeDevice::new("fi-6130dj:12345", "Fujitsu", "fi-6130dj"));
    let kept = stored("libusb:001:004", "Fujitsu", "fi-6130dj");
    let opened = backend.open(&kept, SOON).unwrap();
    assert_eq!(
        opened.info,
        stored("libusb:001:005", "Fujitsu", "fi-6130dj")
    );
    assert_eq!(opened.renamed_from, Some(id("fake:libusb:001:004")));
    // From any bus and device numbers to any others.
    assert_eq!(
        open(&mut backend, &stored("libusb:3:17", "Fujitsu", "fi-7160")),
        Ok((id("fake:libusb:002:001"), Some(id("fake:libusb:3:17"))))
    );
}

#[test]
fn a_stored_name_now_listed_as_another_model_is_taken_as_gone() {
    // 001:004 is another scanner now, and the stored one isn't there.
    let mut backend = FakeBackend::new()
        .with(FakeDevice::new("libusb:001:004", "Canon", "DR-C240"))
        .with(FakeDevice::new("0", "Noname", "Other"));
    let kept = stored("libusb:001:004", "Fujitsu", "fi-6130dj");
    assert_eq!(open(&mut backend, &kept), Err(ScanError::Gone));
    // Another maker, the same model, is another device too; and so is an index.
    let kept = stored("libusb:001:004", "Fujitsu", "DR-C240");
    assert_eq!(open(&mut backend, &kept), Err(ScanError::Gone));
    let kept = stored("0", "Noname", "Flatbed");
    assert_eq!(open(&mut backend, &kept), Err(ScanError::Gone));
}

#[test]
fn a_stored_name_now_another_model_s_falls_back_to_the_stored_model_under_another() {
    // The two swapped numbers as they were plugged in again.
    let mut backend = FakeBackend::new()
        .with(FakeDevice::new("libusb:001:004", "Canon", "DR-C240"))
        .with(FakeDevice::new("libusb:001:005", "Fujitsu", "fi-6130dj"));
    assert_eq!(
        open(
            &mut backend,
            &stored("libusb:001:004", "Fujitsu", "fi-6130dj")
        ),
        Ok((id("fake:libusb:001:005"), Some(id("fake:libusb:001:004"))))
    );
    assert_eq!(
        open(&mut backend, &stored("libusb:001:005", "Canon", "DR-C240")),
        Ok((id("fake:libusb:001:004"), Some(id("fake:libusb:001:005"))))
    );
}

#[test]
fn the_fallback_needs_the_same_maker_and_the_same_model() {
    for (maker, model) in [("Fujitsu", "fi-7160"), ("Ricoh", "fi-6130dj"), ("", "")] {
        let mut backend = FakeBackend::new().with(FakeDevice::new("libusb:001:005", maker, model));
        let kept = stored("libusb:001:004", "Fujitsu", "fi-6130dj");
        assert_eq!(
            open(&mut backend, &kept),
            Err(ScanError::Gone),
            "{maker} {model}"
        );
    }
}

#[test]
fn the_fallback_is_never_to_one_of_two() {
    let mut backend = FakeBackend::new()
        .with(FakeDevice::new("libusb:001:005", "Fujitsu", "fi-6130dj"))
        .with(FakeDevice::new("libusb:002:001", "Fujitsu", "fi-6130dj"));
    let kept = stored("libusb:001:004", "Fujitsu", "fi-6130dj");
    assert_eq!(open(&mut backend, &kept), Err(ScanError::Gone));
    // Each of the two opens under its own name.
    assert_eq!(
        open(
            &mut backend,
            &stored("libusb:002:001", "Fujitsu", "fi-6130dj")
        ),
        Ok((id("fake:libusb:002:001"), None))
    );
}

#[test]
fn the_fallback_is_only_from_and_to_a_usb_connection_s_name() {
    let fi = |name: &str| FakeDevice::new(name, "Fujitsu", "fi-6130dj");
    // From a serial, which survives replugging, or a network name: none.
    let mut backend = FakeBackend::new().with(fi("libusb:001:005"));
    for name in [
        "fi-6130dj:12345",
        "net:192.0.2.1:fujitsu:fi-6130dj",
        "0",
        "libusb:001",
        "libusb:001:x",
    ] {
        assert_eq!(
            open(&mut backend, &stored(name, "Fujitsu", "fi-6130dj")),
            Err(ScanError::Gone),
            "{name}"
        );
    }
    // To a name of another form, the one device of the stored maker and model: none.
    let kept = stored("libusb:001:004", "Fujitsu", "fi-6130dj");
    for name in ["fi-6130dj:12345", "net:192.0.2.1:0", "libusb:001"] {
        let mut backend = FakeBackend::new().with(fi(name));
        assert_eq!(open(&mut backend, &kept), Err(ScanError::Gone), "{name}");
    }
    // Nor to a device that isn't listed.
    let mut backend = FakeBackend::new().with(fi("libusb:001:005").unlisted());
    assert_eq!(open(&mut backend, &kept), Err(ScanError::Gone));
}

#[test]
fn an_unlisted_device_opens_by_its_name_with_the_stored_info() {
    let mut backend =
        FakeBackend::new().with(FakeDevice::new("Desk scanner", "Maker", "Model").unlisted());
    assert_eq!(backend.list(SOON), Ok(Vec::new()));
    // Unchecked: the stored maker and model are kept, whatever they are.
    let kept = stored("Desk scanner", "Another", "Name");
    let opened = backend.open(&kept, SOON).unwrap();
    assert_eq!(opened.info, kept);
    assert_eq!(opened.renamed_from, None);
}

#[test]
fn a_name_not_there_is_gone() {
    let mut backend = FakeBackend::new().with(FakeDevice::new("a", "M", "N"));
    assert_eq!(
        open(&mut backend, &stored("b", "M", "N")),
        Err(ScanError::Gone)
    );
    assert_eq!(
        open(&mut FakeBackend::new(), &stored("a", "M", "N")),
        Err(ScanError::Gone)
    );
}

#[test]
fn an_id_the_fake_doesn_t_form_is_refused_before_the_listing() {
    // A listing that would time out: the id is refused first.
    let mut backend = FakeBackend::new()
        .with(FakeDevice::new("a", "M", "N"))
        .listing_takes(SOON);
    for bad in [
        "fake:",
        "fake",
        "",
        "sane:test:0",
        "Fake:a",
        " fake:a",
        "twain64:a",
    ] {
        let mut kept = stored("a", "M", "N");
        kept.id = id(bad);
        assert_eq!(
            backend.open(&kept, Duration::ZERO).unwrap_err(),
            ScanError::BadId,
            "{bad:?}"
        );
    }
    // The error says nothing of the id.
    assert_eq!(
        ScanError::BadId.to_string(),
        "the device id isn't one this driver gives"
    );
}

#[test]
fn a_device_scripted_to_refuse_answers_its_error_once_found() {
    let refusals = [
        ScanError::Busy,
        ScanError::AccessDenied,
        ScanError::Gone,
        ScanError::Driver {
            message: "Locked by another program".to_owned(),
            shown: true,
        },
    ];
    for refusal in refusals {
        let mut backend = FakeBackend::new()
            .with(FakeDevice::new("libusb:001:005", "Maker", "Model").refuses_open(refusal.clone()))
            .with(
                FakeDevice::new("unlisted", "Maker", "Model")
                    .unlisted()
                    .refuses_open(refusal.clone()),
            );
        let kept = stored("libusb:001:005", "Maker", "Model");
        assert_eq!(open(&mut backend, &kept), Err(refusal.clone()));
        // Found by the fallback too, and found unlisted.
        let kept = stored("libusb:001:004", "Maker", "Model");
        assert_eq!(open(&mut backend, &kept), Err(refusal.clone()));
        let kept = stored("unlisted", "Maker", "Model");
        assert_eq!(open(&mut backend, &kept), Err(refusal.clone()));
        // Not found: gone, whatever the device would answer.
        let kept = stored("libusb:001:005", "Maker", "Other");
        assert_eq!(open(&mut backend, &kept), Err(ScanError::Gone));
        // And still listed.
        assert_eq!(backend.list(SOON).unwrap().len(), 1);
    }
}

#[test]
fn a_device_has_a_feeder_with_duplex_and_a_flatbed_by_default() {
    let mut backend = FakeBackend::new().with(FakeDevice::new("a", "M", "N"));
    let mut device = backend.open(&stored("a", "M", "N"), SOON).unwrap().device;
    let caps = device.caps().unwrap();
    assert!(!caps.settings_window);
    let formats = [
        Format::Bw1,
        Format::Gray8,
        Format::Gray16,
        Format::Rgb8,
        Format::Rgb16,
    ];
    let [feeder, flatbed] = &caps.sources[..] else {
        panic!("{caps:?}");
    };
    assert_eq!(feeder.source, Source::Feeder);
    assert!(feeder.duplex && feeder.paper_auto);
    assert_eq!(
        feeder.resolutions,
        Resolutions::Range {
            min: 50,
            max: 600,
            step: NonZeroU32::new(1).unwrap()
        }
    );
    assert_eq!(feeder.formats, formats);
    assert!(feeder.encodings.is_empty());
    assert_eq!(feeder.max_area, Some(SizeMm::LEGAL));

    assert_eq!(flatbed.source, Source::Flatbed);
    assert!(!flatbed.duplex && !flatbed.paper_auto);
    assert_eq!(
        flatbed.resolutions,
        Resolutions::List(vec![75, 150, 300, 600])
    );
    assert_eq!(flatbed.formats, formats);
    assert!(flatbed.encodings.is_empty());
    assert_eq!(
        flatbed.max_area,
        Some(SizeMm {
            width: 215.9,
            height: 297.0
        })
    );
}

/// One flatbed source, at 300 dpi, in gray.
fn flatbed_caps() -> Caps {
    Caps::new(vec![SourceCaps::new(
        Source::Flatbed,
        Resolutions::List(vec![300]),
        vec![Format::Gray8],
    )])
}

#[test]
fn scripted_caps_replace_the_default_ones() {
    let mut caps = flatbed_caps();
    // The device's own window decides this, not the caps given.
    caps.settings_window = true;
    let mut backend = FakeBackend::new().with(FakeDevice::new("a", "M", "N").caps(caps));
    let mut device = backend.open(&stored("a", "M", "N"), SOON).unwrap().device;
    assert_eq!(device.caps(), Ok(flatbed_caps()));
}

#[test]
fn the_settings_window_closes_as_scripted() {
    let mut backend = FakeBackend::new()
        .with(FakeDevice::new("none", "M", "N"))
        .with(FakeDevice::new("accepts", "M", "N").settings_window(WindowClosed::Accepted))
        .with(
            FakeDevice::new("dismisses", "M", "N")
                .settings_window(WindowClosed::Dismissed)
                .caps(flatbed_caps()),
        );
    let mut none = backend
        .open(&stored("none", "M", "N"), SOON)
        .unwrap()
        .device;
    assert!(!none.caps().unwrap().settings_window);
    assert_eq!(none.settings_window(None), Err(ScanError::Unsupported));
    assert_eq!(
        none.settings_window(Some(WindowOwner(0x1234))),
        Err(ScanError::Unsupported)
    );

    let mut accepts = backend
        .open(&stored("accepts", "M", "N"), SOON)
        .unwrap()
        .device;
    assert!(accepts.caps().unwrap().settings_window);
    assert_eq!(
        accepts.settings_window(Some(WindowOwner(0x1234))),
        Ok(WindowClosed::Accepted)
    );
    assert_eq!(accepts.settings_window(None), Ok(WindowClosed::Accepted));

    let mut dismisses = backend
        .open(&stored("dismisses", "M", "N"), SOON)
        .unwrap()
        .device;
    let caps = dismisses.caps().unwrap();
    assert!(caps.settings_window);
    assert_eq!(caps.sources, flatbed_caps().sources);
    assert_eq!(dismisses.settings_window(None), Ok(WindowClosed::Dismissed));
}

/// Whether `script` panics.
fn panics(script: impl FnOnce()) -> bool {
    panic::catch_unwind(AssertUnwindSafe(script)).is_err()
}

#[test]
fn caps_the_model_forbids_are_a_script_s_mistake() {
    let source = || {
        SourceCaps::new(
            Source::Feeder,
            Resolutions::List(vec![300]),
            vec![Format::Gray8],
        )
    };
    let with = |change: &dyn Fn(&mut SourceCaps)| {
        let mut source = source();
        change(&mut source);
        Caps::new(vec![source])
    };
    let range = |min, max| Resolutions::Range {
        min,
        max,
        step: NonZeroU32::new(1).unwrap(),
    };
    let area = |width, height| Some(SizeMm { width, height });
    let forbidden = [
        Caps::new(Vec::new()),
        with(&|source| source.formats.clear()),
        with(&|source| source.resolutions = Resolutions::List(Vec::new())),
        with(&|source| source.resolutions = Resolutions::List(vec![300, 0])),
        with(&|source| source.resolutions = range(0, 600)),
        with(&|source| source.resolutions = range(601, 600)),
        with(&|source| source.max_area = area(0.0, 297.0)),
        with(&|source| source.max_area = area(210.0, -1.0)),
        with(&|source| source.max_area = area(f32::NAN, 297.0)),
        with(&|source| source.max_area = area(210.0, f32::INFINITY)),
        Caps::new(vec![
            source(),
            with(&|source| source.formats.clear()).sources.remove(0),
        ]),
    ];
    for caps in forbidden {
        let shown = format!("{caps:?}");
        assert!(
            panics(|| drop(FakeDevice::new("a", "M", "N").caps(caps))),
            "{shown}"
        );
    }
    // At their edges, allowed.
    for caps in [
        with(&|source| source.resolutions = range(1, 1)),
        with(&|source| source.resolutions = range(600, 600)),
        with(&|source| source.max_area = area(0.1, f32::MAX)),
        Caps::new(vec![source(), source()]),
    ] {
        drop(FakeDevice::new("a", "M", "N").caps(caps));
    }
}

#[test]
fn other_mistakes_in_a_script_panic_as_it_s_written() {
    assert!(panics(|| drop(FakeDevice::new("", "M", "N"))));
    assert!(panics(|| {
        drop(
            FakeBackend::new()
                .with(FakeDevice::new("a", "M", "N"))
                .with(FakeDevice::new("a", "Other", "Device")),
        )
    }));
    for size in [
        SizeMm {
            width: 0.0,
            height: 279.4,
        },
        SizeMm {
            width: 215.9,
            height: f32::NAN,
        },
        SizeMm {
            width: f32::NEG_INFINITY,
            height: 279.4,
        },
    ] {
        assert!(panics(|| drop(Sheet::new().size(size))), "{size:?}");
    }
}
