//! State 4 through the API: the sample source opened with each delivery, its
//! capabilities read and set in all four containers, its refusals reported
//! with its condition code; a delivery changed when it's opened again.

#![cfg(windows)]

mod support;

use support::Outcome;
use twain_dsm::{
    ConditionCode, Container, Delivery, Error, Fix32, Frame, Item, ItemType, SetOutcome, Source,
    Ui, ffi,
};

fn one(source: &Source<'_, ()>, cap: u16) -> Item {
    match source.get_current(cap) {
        Ok(Container::One(item)) => item,
        other => panic!("MSG_GETCURRENT of {cap:#06x}: {other:?}"),
    }
}

/// The sample's answers, as its code gives them.
fn the_samples_answers(source: &Source<'_, ()>) {
    // Set, then read back.
    let set = |cap, item: Item| {
        let outcome = source
            .set(cap, item.clone())
            .unwrap_or_else(|e| panic!("MSG_SET {cap:#06x}: {e}"));
        assert_eq!(outcome, SetOutcome::Exact, "MSG_SET {cap:#06x} {item:?}");
    };
    set(ffi::ICAP_XFERMECH, Item::UInt16(ffi::TWSX_MEMORY));
    assert_eq!(
        one(source, ffi::ICAP_XFERMECH),
        Item::UInt16(ffi::TWSX_MEMORY)
    );
    set(ffi::CAP_XFERCOUNT, Item::Int16(3));
    assert_eq!(one(source, ffi::CAP_XFERCOUNT), Item::Int16(3));
    set(ffi::CAP_INDICATORS, Item::Bool(false));
    assert_eq!(one(source, ffi::CAP_INDICATORS), Item::Bool(false));

    // As the sample starts, and its defaults.
    assert_eq!(one(source, ffi::CAP_FEEDERLOADED), Item::Bool(true));
    match source.get_default(ffi::ICAP_PIXELTYPE) {
        Ok(Container::One(Item::UInt16(_))) => {}
        other => panic!("MSG_GETDEFAULT of ICAP_PIXELTYPE: {other:?}"),
    }
    assert_eq!(
        one(source, ffi::ICAP_PIXELTYPE),
        Item::UInt16(ffi::TWPT_RGB)
    );
    assert_eq!(
        one(source, ffi::ICAP_XRESOLUTION),
        Item::Fix32(Fix32::from_int(200))
    );
    assert_eq!(
        one(source, ffi::ICAP_BITORDER),
        Item::UInt16(ffi::TWBO_LSBFIRST)
    );
    assert_eq!(
        one(source, ffi::ICAP_PIXELFLAVOR),
        Item::UInt16(ffi::TWPF_CHOCOLATE)
    );

    // An enumeration: MSG_GET of a pixel type, and of a boolean (the sample
    // answers a DF_APP2 application so).
    match source.get(ffi::ICAP_PIXELTYPE) {
        Ok(Container::Enumeration(e)) => {
            assert_eq!(e.items.item_type(), ItemType::UInt16);
            for pt in [ffi::TWPT_BW, ffi::TWPT_GRAY, ffi::TWPT_RGB] {
                assert!(
                    e.items.as_slice().contains(&Item::UInt16(pt)),
                    "{pt} offered: {e:?}"
                );
            }
            assert_eq!(e.current_item(), Some(&Item::UInt16(ffi::TWPT_RGB)));
        }
        other => panic!("MSG_GET of ICAP_PIXELTYPE: {other:?}"),
    }
    match source.get(ffi::CAP_INDICATORS) {
        Ok(Container::Enumeration(e)) => assert_eq!(e.current_item(), Some(&Item::Bool(false))),
        other => panic!("MSG_GET of CAP_INDICATORS: {other:?}"),
    }
    // An array.
    match source.get(ffi::CAP_SUPPORTEDCAPS) {
        Ok(Container::Array(caps)) => {
            assert_eq!(caps.item_type(), ItemType::UInt16);
            for cap in [ffi::ICAP_XFERMECH, ffi::CAP_XFERCOUNT, ffi::ICAP_THRESHOLD] {
                assert!(
                    caps.as_slice().contains(&Item::UInt16(cap)),
                    "{cap:#06x} listed"
                );
            }
        }
        other => panic!("MSG_GET of CAP_SUPPORTEDCAPS: {other:?}"),
    }
    // A range, set within and read back.
    match source.get(ffi::ICAP_THRESHOLD) {
        Ok(Container::Range(r)) => {
            assert_eq!(r.item_type(), ItemType::Fix32);
            assert_eq!(r.min(), &Item::Fix32(Fix32::from_int(0)));
            assert_eq!(r.max(), &Item::Fix32(Fix32::from_int(255)));
            assert_eq!(r.step(), &Item::Fix32(Fix32::from_int(1)));
            assert_eq!(r.current(), &Item::Fix32(Fix32::from_int(128)));
        }
        other => panic!("MSG_GET of ICAP_THRESHOLD: {other:?}"),
    }
    set(ffi::ICAP_THRESHOLD, Item::Fix32(Fix32::from_int(100)));
    assert_eq!(
        one(source, ffi::ICAP_THRESHOLD),
        Item::Fix32(Fix32::from_int(100))
    );
    let reset = source
        .reset(ffi::ICAP_THRESHOLD)
        .expect("MSG_RESET of ICAP_THRESHOLD");
    println!("ICAP_THRESHOLD reset: {reset:?}");
    assert_eq!(
        one(source, ffi::ICAP_THRESHOLD),
        Item::Fix32(Fix32::from_int(128))
    );

    let support = source
        .query_support(ffi::ICAP_XFERMECH)
        .expect("MSG_QUERYSUPPORT");
    let wanted = u32::from(ffi::TWQC_GET | ffi::TWQC_SET | ffi::TWQC_GETCURRENT);
    assert_eq!(
        support & wanted,
        wanted,
        "ICAP_XFERMECH's support: {support:#x}"
    );

    // Refused by the source, with its condition code.
    let refused = |result: Result<(), Error>, what: &str| match result {
        Err(Error::Failed {
            by_source, by_dsm, ..
        }) => {
            assert_eq!(by_source, Some(ConditionCode::CAPUNSUPPORTED), "{what}");
            assert_eq!(by_dsm, None, "{what}: the DSM has nothing of its own");
        }
        other => panic!("{what}: {other:?}"),
    };
    refused(
        source
            .get_current(ffi::ICAP_SUPPORTEDEXTIMAGEINFO)
            .map(drop),
        "MSG_GETCURRENT of ICAP_SUPPORTEDEXTIMAGEINFO",
    );
    refused(
        source
            .set(ffi::ICAP_EXTIMAGEINFO, Item::Bool(true))
            .map(drop),
        "MSG_SET of ICAP_EXTIMAGEINFO",
    );
    // Refused before it's sent: a string too long for its type.
    match source.set(ffi::CAP_AUTHOR, Item::Str128(vec![b'a'; 129])) {
        Err(Error::Container(_)) => {}
        other => panic!("a string too long: {other:?}"),
    }

    // DAT_STATUS in state 4: cleared after the refusals above (`None` = TWCC_SUCCESS).
    let _ = source.status();
    assert_eq!(source.status(), None, "DAT_STATUS after clear");

    // DAT_IMAGELAYOUT: default, get, set crop frame, readback, reset, and InvalidFrame refusal.
    source
        .set(ffi::ICAP_UNITS, Item::UInt16(ffi::TWUN_INCHES))
        .expect("ICAP_UNITS = TWUN_INCHES");
    let def_layout = source
        .default_image_layout()
        .expect("DAT_IMAGELAYOUT/MSG_GETDEFAULT");
    let cur_layout = source.image_layout().expect("DAT_IMAGELAYOUT/MSG_GET");
    assert_eq!(cur_layout.frame, def_layout.frame);

    let crop = Frame {
        left: Fix32::from_int(0),
        top: Fix32::from_int(0),
        right: Fix32::from_int(2),
        bottom: Fix32::from_int(3),
    };
    source
        .set_image_layout(crop)
        .expect("DAT_IMAGELAYOUT/MSG_SET");
    let after_crop = source.image_layout().expect("DAT_IMAGELAYOUT after set");
    assert_eq!(after_crop.frame, crop);

    let reset_layout = source
        .reset_image_layout()
        .expect("DAT_IMAGELAYOUT/MSG_RESET");
    assert_eq!(reset_layout.frame, def_layout.frame);
    assert_eq!(
        source.image_layout().expect("after reset").frame,
        def_layout.frame
    );

    for bad_frame in [
        Frame {
            left: Fix32::from_int(2),
            top: Fix32::from_int(0),
            right: Fix32::from_int(2),
            bottom: Fix32::from_int(3),
        },
        Frame {
            left: Fix32::from_int(3),
            top: Fix32::from_int(0),
            right: Fix32::from_int(2),
            bottom: Fix32::from_int(3),
        },
        Frame {
            left: Fix32::from_int(0),
            top: Fix32::from_int(4),
            right: Fix32::from_int(2),
            bottom: Fix32::from_int(3),
        },
    ] {
        match source.set_image_layout(bad_frame) {
            Err(Error::InvalidFrame { frame }) => assert_eq!(frame, bad_frame),
            other => panic!("bad frame {bad_frame:?}: {other:?}"),
        }
    }
}

#[test]
fn the_sample_answers_through_either_delivery() {
    forked!(the_sample_answers_through_either_delivery, &[], || {
        twain_dsm::run::<(), _>(|twain| {
            let mut opened = match support::open(twain) {
                Ok(opened) => opened,
                Err(outcome) => return outcome,
            };
            let sample = match support::sample(&opened) {
                Ok(sample) => sample,
                Err(outcome) => return outcome,
            };
            // Each delivery, then the other, on the source opened again.
            for delivery in [Delivery::Callback, Delivery::Events, Delivery::Callback] {
                let source = opened
                    .open_source(&sample, delivery)
                    .map_err(|r| r.into_error())
                    .expect("the sample opens");
                assert_eq!(source.delivery(), delivery);
                assert_eq!(source.id().product_name(), support::SAMPLE_PRODUCT);
                assert_ne!(source.id().id(), 0);
                the_samples_answers(&source);
                // Invalid parent HWND refused before calling DSM_Entry, preserving Source.
                let source = match source.enable(Ui::ShownOwned(0)) {
                    Err(refused) => {
                        assert!(matches!(refused.error(), Error::InvalidWindow { hwnd: 0 }));
                        refused.into_state()
                    }
                    Ok(_) => panic!("Ui::ShownOwned(0) should fail"),
                };
                let source = match source.settings_ui(Some(0xFFFF)) {
                    Err(refused) => {
                        assert!(matches!(
                            refused.error(),
                            Error::InvalidWindow { hwnd: 0xFFFF }
                        ));
                        refused.into_state()
                    }
                    Ok(_) => panic!("settings_ui(Some(0xFFFF)) should fail"),
                };
                opened = source
                    .close()
                    .map_err(|r| r.into_error())
                    .expect("the sample closes");
            }
            Outcome::Done
        })
        .expect("TWAIN runs")
        .value
    });
}

#[test]
fn a_source_left_open_is_closed_with_the_connection() {
    forked!(
        a_source_left_open_is_closed_with_the_connection,
        &[],
        || {
            for round in 0..2 {
                let report = twain_dsm::run::<(), _>(|twain| {
                    let opened = match support::open(twain) {
                        Ok(opened) => opened,
                        Err(outcome) => return Some(outcome),
                    };
                    let sample = match support::sample(&opened) {
                        Ok(sample) => sample,
                        Err(outcome) => return Some(outcome),
                    };
                    let source = opened
                        .open_source(&sample, Delivery::Callback)
                        .map_err(|r| r.into_error())
                        .expect("the sample opens");
                    std::mem::forget(source);
                    None
                })
                .expect("TWAIN runs again");
                if let Some(outcome) = report.value {
                    return outcome;
                }
                assert!(
                    report.source_left.is_none(),
                    "round {round}: the source left open is closed at the end"
                );
                assert!(
                    report.stranded.is_none(),
                    "round {round}: and then the connection"
                );
            }
            Outcome::Done
        }
    );
}
