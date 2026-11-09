//! Workers that break the protocol, written by hand: each refused as what it broke, the worker
//! killed, and every call after it on the device answered `Ended`.

mod support;

use scan_worker::scan_device::{Format, LayoutError, Source};
use scan_worker::{Error, PROTOCOL, ProtocolError, RemoteDevice};

use support::{AT_MOST, client, scan, settings, stored};

/// What opening the stored device through `scenario`'s worker breaks.
fn open_breaks(scenario: &str) -> ProtocolError {
    match client(scenario).open(&stored(), AT_MOST) {
        Err(Error::Protocol(error)) => error,
        other => panic!("{scenario}: {other:?}"),
    }
}

/// The device opened through `scenario`'s worker, which then breaks the protocol.
fn opened(scenario: &str) -> RemoteDevice {
    client(scenario)
        .open(&stored(), AT_MOST)
        .unwrap_or_else(|error| panic!("{scenario}: {error}"))
}

/// What a batch through `scenario`'s worker breaks; and every call after it `Ended`.
fn scan_breaks(scenario: &str) -> ProtocolError {
    let mut device = opened(scenario);
    let (_, ended) = scan(
        &mut device,
        &settings(Source::Feeder, 300, Format::Gray8, false),
        1,
    );
    let Err(Error::Protocol(error)) = ended else {
        panic!("{scenario}: {ended:?}");
    };
    ended_after(device, scenario);
    error
}

/// Every call on `device` answered `Ended`, and its close `Ok`.
fn ended_after(mut device: RemoteDevice, scenario: &str) {
    assert!(
        matches!(device.caps(), Err(Error::Ended)),
        "{scenario}: caps"
    );
    assert!(
        matches!(device.settings_window(None), Err(Error::Ended)),
        "{scenario}: the window"
    );
    let (_, ended) = scan(
        &mut device,
        &settings(Source::Feeder, 300, Format::Gray8, false),
        1,
    );
    assert!(matches!(ended, Err(Error::Ended)), "{scenario}: {ended:?}");
    device.close().expect("closed already");
}

#[test]
fn a_worker_of_another_protocol_is_refused_before_anything_is_sent() {
    assert_eq!(
        open_breaks("version"),
        ProtocolError::Version {
            worker: 2,
            client: PROTOCOL
        }
    );
}

#[test]
fn a_later_protocol_s_hello_is_read_for_its_protocol_alone() {
    assert_eq!(
        open_breaks("later-hello"),
        ProtocolError::Version {
            worker: 2,
            client: PROTOCOL
        }
    );
}

#[test]
fn an_open_that_gives_another_device_unsaid_or_renamed_from_another_id_is_refused() {
    for (scenario, what) in [
        (
            "opened-another",
            "an opened device of another id than the stored one's, not said renamed",
        ),
        (
            "renamed-elsewhere",
            "an opened device said renamed from another id than the stored one's",
        ),
    ] {
        assert_eq!(open_breaks(scenario), ProtocolError::Invalid { what });
    }
}

#[test]
fn a_batch_out_of_the_model_s_order_is_refused() {
    let invalid = |what| ProtocolError::Invalid { what };
    for (scenario, broken) in [
        ("page-first", ProtocolError::Unexpected { got: "page" }),
        ("applied-late", ProtocolError::Unexpected { got: "applied" }),
        ("sheet-out-of-turn", invalid("a sheet numbered out of turn")),
        (
            "empty-after-page",
            invalid("an empty feeder as an error after pages, where the batch is done"),
        ),
        (
            "encoded-unasked",
            invalid("a page encoded in a form the settings didn't ask for"),
        ),
        (
            "huge-paper",
            invalid("settings applied whose paper isn't finite and above 0"),
        ),
    ] {
        assert_eq!(scan_breaks(scenario), broken, "{scenario}");
    }
}

#[test]
fn a_first_line_that_isn_t_its_hello_is_refused() {
    assert_eq!(open_breaks("no-hello"), ProtocolError::NoHello);
    assert!(matches!(
        open_breaks("not-json"),
        ProtocolError::Unreadable { .. }
    ));
}

#[test]
fn a_line_past_the_cap_is_refused_unread() {
    assert_eq!(
        open_breaks("long-line"),
        ProtocolError::LineTooLong { limit: 1 << 20 }
    );
}

#[test]
fn an_answer_out_of_its_place_or_a_refusal_breaks_the_protocol() {
    assert_eq!(
        open_breaks("out-of-place"),
        ProtocolError::Unexpected { got: "devices" }
    );
    assert_eq!(
        open_breaks("refuses"),
        ProtocolError::Refused {
            reason: "not today".to_owned()
        }
    );
}

#[test]
fn caps_with_no_source_are_refused() {
    let mut device = opened("no-source");
    let caps = device.caps();
    assert!(
        matches!(
            caps,
            Err(Error::Protocol(ProtocolError::Invalid {
                what: "caps with no source"
            }))
        ),
        "{caps:?}"
    );
    ended_after(device, "no-source");
}

#[test]
fn a_page_past_the_cap_is_refused_before_its_bytes_are_read() {
    assert_eq!(
        scan_breaks("big-page"),
        ProtocolError::PageTooLarge {
            bytes: 1_200_000_000,
            limit: 1 << 30
        }
    );
}

#[test]
fn a_page_whose_length_isn_t_its_format_and_size_s_is_refused() {
    assert_eq!(
        scan_breaks("page-length"),
        ProtocolError::PageLength {
            expected: 100,
            stated: 99
        }
    );
}

#[test]
fn a_black_and_white_page_with_its_padding_bits_set_is_refused() {
    assert_eq!(
        scan_breaks("padding"),
        ProtocolError::Layout(LayoutError::Padding { row: 0 })
    );
}

#[test]
fn a_page_at_0_dpi_is_refused() {
    assert!(matches!(
        scan_breaks("zero-dpi"),
        ProtocolError::Invalid { what } if what.contains("dpi")
    ));
}
