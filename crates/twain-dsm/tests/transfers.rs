//! States 5 to 7 through the API: the sample enabled, its pages transferred
//! native and through memory with each delivery, the pending counts it gives,
//! and the two mechanisms' images compared pixel for pixel.

#![cfg(windows)]

#[path = "support/image.rs"]
#[allow(dead_code)] // kept whole: not all of it is used here
mod image;
mod support;

use std::time::Duration;

use image::{Assembler, Format, Image, Polarity, Shape};
use support::Outcome;
use twain_dsm::{
    ConditionCode, Delivery, Enabled, Item, MemoryTransfer, Native, Next, Opened, Pending, Ready,
    SourceId, Ui, ffi,
};

/// The sample's feeder, through the environment the DSM loads it in.
const PAGES: &[(&str, &str)] = &[("CAP_XFERCOUNT", "3")];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mechanism {
    Native,
    Memory,
}

/// The source opened, set up for a batch, enabled, and its first transfer
/// ready.
fn ready<'t>(
    opened: Opened<'t, ()>,
    sample: &SourceId,
    delivery: Delivery,
    mechanism: Mechanism,
    count: i16,
    pixel_type: u16,
) -> Ready<'t, ()> {
    let source = opened
        .open_source(sample, delivery)
        .map_err(|r| r.into_error())
        .expect("the sample opens");
    let mech = match mechanism {
        Mechanism::Native => ffi::TWSX_NATIVE,
        Mechanism::Memory => ffi::TWSX_MEMORY,
    };
    for (cap, item) in [
        (ffi::ICAP_XFERMECH, Item::UInt16(mech)),
        (ffi::CAP_XFERCOUNT, Item::Int16(count)),
        (ffi::CAP_INDICATORS, Item::Bool(false)),
        (ffi::ICAP_PIXELTYPE, Item::UInt16(pixel_type)),
    ] {
        source
            .set(cap, item)
            .unwrap_or_else(|e| panic!("MSG_SET {cap:#06x}: {e}"));
    }
    let enabled = source
        .enable(Ui::Hidden)
        .map_err(|r| r.into_error())
        .expect("the sample enables");
    first_ready(enabled)
}

fn first_ready<'t>(mut enabled: Enabled<'t, ()>) -> Ready<'t, ()> {
    loop {
        match enabled
            .next_event(Duration::from_secs(20))
            .map_err(|r| r.into_error())
            .expect("an event")
        {
            Next::Ready(ready) => return ready,
            Next::Event(_, event) => panic!("the sample sent {event:?}, not MSG_XFERREADY"),
            Next::Timeout(_) => panic!("no MSG_XFERREADY in 20 seconds"),
            Next::Request(back, ()) | Next::Quit(back, _) => enabled = back,
        }
    }
}

/// One page through `mechanism`, decoded, and what follows it.
fn page<'t>(mut ready: Ready<'t, ()>, mechanism: Mechanism) -> (Image, Pending<'t, ()>) {
    // The sample has no feeder to stop.
    refused(
        ready.stop_feeder(),
        ConditionCode::BADPROTOCOL,
        "MSG_STOPFEEDER",
    );
    let info = ready.image_info().expect("DAT_IMAGEINFO");
    let (width, length) = (info.width, info.length);
    let format = Format::from_image_info(
        info.pixel_type,
        info.samples_per_pixel,
        info.bits_per_sample[0],
    )
    .expect("a format the sample has");
    assert!(width > 0 && length > 0, "the image is {width} by {length}");
    match mechanism {
        Mechanism::Native => match ready
            .native()
            .map_err(|r| r.into_error())
            .expect("a native transfer")
        {
            Native::Done(dib, transferred) => {
                let (image, _) = image::decode_dib(dib.bytes()).expect("the DIB decodes");
                assert_eq!((image.width, image.height), (width as u32, length as u32));
                drop(dib);
                refused(
                    transferred
                        .ext_image_info(&[ffi::TWEI_DOCUMENTNUMBER])
                        .map(drop),
                    ConditionCode::CAPUNSUPPORTED,
                    "DAT_EXTIMAGEINFO",
                );
                (
                    image,
                    transferred
                        .end()
                        .map_err(|r| r.into_error())
                        .expect("MSG_ENDXFER"),
                )
            }
            Native::Cancelled(_) => panic!("the sample cancelled a native transfer"),
        },
        Mechanism::Memory => {
            let mut transfer = ready
                .memory()
                .map_err(|r| r.into_error())
                .expect("DAT_SETUPMEMXFER");
            let image = strips(&mut transfer, (width as u32, length as u32, format));
            refused(
                transfer
                    .ext_image_info(&[ffi::TWEI_DOCUMENTNUMBER])
                    .map(drop),
                ConditionCode::CAPUNSUPPORTED,
                "DAT_EXTIMAGEINFO",
            );
            (
                image,
                transfer
                    .end()
                    .map_err(|r| r.into_error())
                    .expect("MSG_ENDXFER"),
            )
        }
    }
}

/// A triplet the sample refuses, with its own code.
fn refused(result: Result<(), twain_dsm::Error>, code: ConditionCode, what: &str) {
    match result {
        Err(twain_dsm::Error::Failed { by_source, .. }) => {
            assert_eq!(by_source, Some(code), "{what}")
        }
        other => panic!("{what}: {other:?}"),
    }
}

/// A memory transfer's strips, assembled.
fn strips(
    transfer: &mut MemoryTransfer<'_, ()>,
    (width, height, format): (u32, u32, Format),
) -> Image {
    let mut assembler = Assembler::new(width, height, format).expect("an image of that size");
    // The smallest buffer is refused below it.
    let min = transfer.setup().MinBufSize;
    if min > 1 && min != ffi::TWON_DONTCARE32 {
        let mut small = vec![0; min as usize - 1];
        assert!(matches!(
            transfer.next_buffer(&mut small),
            Err(twain_dsm::Error::BufferTooSmall { .. })
        ));
    }
    let mut buffer = vec![0; transfer.preferred_size()];
    let mut n = 0;
    while let Some(strip) = transfer.next_buffer(&mut buffer).expect("DAT_IMAGEMEMXFER") {
        assert_eq!(strip.compression, ffi::TWCP_NONE);
        let shape = Shape {
            x: strip.x_offset,
            y: strip.y_offset,
            columns: strip.columns,
            rows: strip.rows,
            bytes_per_row: strip.bytes_per_row,
        };
        assembler
            .place(shape, strip.bytes)
            .expect("the strip fits the image");
        n += 1;
    }
    assert!(
        !transfer.cancelled(),
        "the sample cancelled a memory transfer"
    );
    assert!(n > 0, "no strips");
    assembler
        .finish(Polarity::ZeroBlack)
        .expect("every pixel covered")
}

/// Every page of a batch, with the pending count after each: the sample's
/// 0xFFFF ("some, how many unknown") until the last, then none.
fn batch<'t>(
    mut ready: Ready<'t, ()>,
    mechanism: Mechanism,
    pages: usize,
    counted: Option<u16>,
) -> (Vec<Image>, Enabled<'t, ()>) {
    let mut images = Vec::new();
    loop {
        let (image, pending) = page(ready, mechanism);
        images.push(image);
        match pending {
            Pending::More(next, pending) => {
                assert!(
                    images.len() < pages,
                    "page {} of {pages} and more pending",
                    images.len()
                );
                // With CAP_XFERCOUNT -1 the sample doesn't say how many
                // (0xFFFF); with a count, how many are left of it.
                let left = counted.map(|n| n - images.len() as u16);
                assert_eq!(pending, left, "pending after page {}", images.len());
                ready = next;
            }
            Pending::Done(enabled) => {
                assert_eq!(
                    images.len(),
                    pages,
                    "the batch ended after {} pages",
                    images.len()
                );
                return (images, enabled);
            }
        }
    }
}

#[test]
fn pages_come_native_and_through_memory_with_either_delivery() {
    forked!(
        pages_come_native_and_through_memory_with_either_delivery,
        PAGES,
        || {
            twain_dsm::run::<(), _>(|twain| {
                let mut opened = match support::open(twain) {
                    Ok(opened) => opened,
                    Err(outcome) => return outcome,
                };
                let sample = match support::sample(&opened) {
                    Ok(sample) => sample,
                    Err(outcome) => return outcome,
                };
                for delivery in [Delivery::Callback, Delivery::Events] {
                    for (mechanism, count) in [
                        (Mechanism::Native, -1),
                        (Mechanism::Memory, -1),
                        (Mechanism::Memory, 3),
                    ] {
                        let first =
                            ready(opened, &sample, delivery, mechanism, count, ffi::TWPT_RGB);
                        let counted = (count > 0).then_some(count as u16);
                        let (images, enabled) = batch(first, mechanism, 3, counted);
                        println!(
                            "{delivery:?}, {mechanism:?}: {} pages of {}x{}",
                            images.len(),
                            images[0].width,
                            images[0].height
                        );
                        let source = enabled
                            .disable()
                            .map_err(|r| r.into_error())
                            .expect("MSG_DISABLEDS");
                        opened = source
                            .close()
                            .map_err(|r| r.into_error())
                            .expect("MSG_CLOSEDS");
                    }
                }
                Outcome::Done
            })
            .expect("TWAIN runs")
            .value
        }
    );
}

#[test]
fn native_and_memory_pages_are_the_same_pixels() {
    forked!(native_and_memory_pages_are_the_same_pixels, PAGES, || {
        twain_dsm::run::<(), _>(|twain| {
            let opened = match support::open(twain) {
                Ok(opened) => opened,
                Err(outcome) => return outcome,
            };
            let sample = match support::sample(&opened) {
                Ok(sample) => sample,
                Err(outcome) => return outcome,
            };
            let mut opened = Some(opened);
            for pixel_type in [ffi::TWPT_RGB, ffi::TWPT_GRAY, ffi::TWPT_BW] {
                let mut one = |mechanism| {
                    let first = ready(
                        opened_take(&mut opened),
                        &sample,
                        Delivery::Events,
                        mechanism,
                        1,
                        pixel_type,
                    );
                    let (mut images, enabled) = batch(first, mechanism, 1, Some(1));
                    let source = enabled
                        .disable()
                        .map_err(|r| r.into_error())
                        .expect("MSG_DISABLEDS");
                    opened = Some(
                        source
                            .close()
                            .map_err(|r| r.into_error())
                            .expect("MSG_CLOSEDS"),
                    );
                    images.remove(0)
                };
                let native = one(Mechanism::Native);
                let memory = one(Mechanism::Memory);
                image::compare(&native, &memory).unwrap_or_else(|e| {
                    panic!("pixel type {pixel_type}: native and memory differ: {e}")
                });
                println!(
                    "pixel type {pixel_type}: equal, {}x{}",
                    native.width, native.height
                );
            }
            Outcome::Done
        })
        .expect("TWAIN runs")
        .value
    });
}

fn opened_take<'t>(opened: &mut Option<Opened<'t, ()>>) -> Opened<'t, ()> {
    opened.take().expect("the connection is open")
}
