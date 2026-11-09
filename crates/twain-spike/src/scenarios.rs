//! The scenarios: what each one drives, and what it expects.

use crate::container::Value;
use crate::ffi::*;
use crate::image::{self, DibFacts, Format, Image, Polarity, Tile};
use crate::session::{Mode, Session, describe};

pub struct Scenario {
    pub name: &'static str,
    pub about: &'static str,
    pub run: fn(&mut Session),
}

/// Every scenario, in the order `all` runs them.
pub const SCENARIOS: &[Scenario] = &[
    Scenario {
        name: "list",
        about: "states 1 to 3 and back; every source's identity",
        run: list,
    },
    Scenario {
        name: "native-event",
        about: "native transfers, the source's messages through MSG_PROCESSEVENT",
        run: |s| batch(s, TWSX_NATIVE, Mode::Event),
    },
    Scenario {
        name: "memory-event",
        about: "memory transfers, the source's messages through MSG_PROCESSEVENT",
        run: |s| batch(s, TWSX_MEMORY, Mode::Event),
    },
    Scenario {
        name: "native-callback",
        about: "native transfers, the source's messages through a DAT_CALLBACK2 callback",
        run: |s| batch(s, TWSX_NATIVE, Mode::Callback),
    },
    Scenario {
        name: "memory-callback",
        about: "memory transfers, the source's messages through a DAT_CALLBACK2 callback",
        run: |s| batch(s, TWSX_MEMORY, Mode::Callback),
    },
    Scenario {
        name: "compare",
        about: "one page native and one through memory, for RGB, gray and black and white, pixel for pixel",
        run: compare,
    },
];

/// The sample source's identity, as its code declares it.
const SAMPLE_MANUFACTURER: &str = "TWAIN Working Group";
const SAMPLE_PRODUCT: &str = "TWAIN2 Software Scanner";

/// What `CAP_XFERCOUNT` in the environment makes the sample feed: that many if
/// it's a positive number, 2 if it's unset, and `None`, not judged, if it's
/// anything else (0 or less is a random count).
fn expected_pages() -> Option<u32> {
    match std::env::var("CAP_XFERCOUNT") {
        Err(std::env::VarError::NotPresent) => Some(2),
        Ok(v) => v
            .trim()
            .parse::<u32>()
            .ok()
            .filter(|&n| (1..=1000).contains(&n)),
        Err(_) => None,
    }
}

fn is_sample(id: &TW_IDENTITY) -> bool {
    id.Manufacturer.text() == SAMPLE_MANUFACTURER && id.ProductName.text() == SAMPLE_PRODUCT
}

/// States 1 to 3 and back: the DSM loaded and opened, its memory functions
/// driven, every source listed, the sample found exactly once.
fn list(s: &mut Session) {
    if s.load_dsm() && s.open_dsm() {
        s.memory_round_trip();
        let sources = s.list_sources();
        for id in &sources {
            let mark = if is_sample(id) { " (the sample)" } else { "" };
            s.out().line(format!("source: {}{mark}", describe(id)));
        }
        let found = sources.iter().filter(|id| is_sample(id)).count();
        s.check(
            found == 1,
            format!("the sample source is listed {found} times, not once"),
        );
    }
    s.walk_down();
}

/// The DSM loaded and opened, and the sample's identity, found exactly once.
fn open_to_sample(s: &mut Session) -> Option<TW_IDENTITY> {
    if !s.load_dsm() || !s.open_dsm() {
        return None;
    }
    let sources = s.list_sources();
    let samples: Vec<&TW_IDENTITY> = sources.iter().filter(|id| is_sample(id)).collect();
    match samples.as_slice() {
        [one] => Some(**one),
        _ => {
            s.out().fail(format!(
                "the sample source is listed {} times, not once",
                samples.len()
            ));
            None
        }
    }
}

/// A page as the spike received it.
struct Page {
    image: Image,
    dib: Option<DibFacts>,
}

/// What one batch asks of the source.
struct Run {
    mechanism: u16,
    mode: Mode,
    /// The capability `CAP_XFERCOUNT`: -1 for all the feeder has.
    count: i16,
    pixel_type: Option<u16>,
}

/// One batch, from state 3 to 3: the source opened, its capabilities set and
/// read, enabled, every page transferred while the count isn't 0, disabled,
/// closed. The pages, or `None` if the batch failed (the session walks down).
fn scan(s: &mut Session, sample: &TW_IDENTITY, run: &Run) -> Option<Vec<Page>> {
    if !s.open_source(sample) {
        return None;
    }
    if run.mode == Mode::Callback && !s.register_callback() {
        return None;
    }
    let mechanism = match run.mechanism {
        TWSX_NATIVE => "native",
        _ => "memory",
    };
    s.out().line(format!(
        "asking for {mechanism} transfers, CAP_XFERCOUNT {}",
        run.count
    ));
    let set = s.set_cap(
        "ICAP_XFERMECH",
        ICAP_XFERMECH,
        TWTY_UINT16,
        u32::from(run.mechanism),
    ) && s.set_cap(
        "CAP_XFERCOUNT",
        CAP_XFERCOUNT,
        TWTY_INT16,
        run.count as i32 as u32,
    ) && s.set_cap("CAP_INDICATORS", CAP_INDICATORS, TWTY_BOOL, 0)
        && run.pixel_type.is_none_or(|pt| {
            s.set_cap("ICAP_PIXELTYPE", ICAP_PIXELTYPE, TWTY_UINT16, u32::from(pt))
        });
    if !set {
        return None;
    }
    let indicators = s.get_current("CAP_INDICATORS", CAP_INDICATORS);
    s.check(
        indicators.map(|c| c.value()) == Some(Value::Bool(false)),
        "CAP_INDICATORS isn't false after setting it",
    );
    let loaded = s.get_current("CAP_FEEDERLOADED", CAP_FEEDERLOADED);
    s.check(
        loaded.map(|c| c.value()) == Some(Value::Bool(true)),
        "CAP_FEEDERLOADED isn't true",
    );
    let want_type = run.pixel_type.unwrap_or(TWPT_RGB);
    let pixel_type = s.get_current("ICAP_PIXELTYPE", ICAP_PIXELTYPE);
    s.check(
        pixel_type.map(|c| c.raw) == Some(u32::from(want_type)),
        format!("ICAP_PIXELTYPE isn't {want_type}"),
    );
    let resolution = s.get_current("ICAP_XRESOLUTION", ICAP_XRESOLUTION);
    s.check(
        resolution.map(|c| c.value()) == Some(Value::Fix32(200.0)),
        "ICAP_XRESOLUTION isn't 200",
    );
    let order = s.get_current("ICAP_BITORDER", ICAP_BITORDER);
    s.check(
        order.map(|c| c.raw) == Some(u32::from(TWBO_LSBFIRST)),
        "ICAP_BITORDER isn't TWBO_LSBFIRST",
    );
    let flavor = s.get_current("ICAP_PIXELFLAVOR", ICAP_PIXELFLAVOR);
    let polarity = match flavor.map(|c| c.raw) {
        Some(f) if f == u32::from(TWPF_CHOCOLATE) => Polarity::ZeroBlack,
        Some(f) if f == u32::from(TWPF_VANILLA) => Polarity::ZeroWhite,
        _ => {
            s.out()
                .fail("ICAP_PIXELFLAVOR is neither TWPF_CHOCOLATE nor TWPF_VANILLA");
            return None;
        }
    };
    s.check(
        polarity == Polarity::ZeroBlack,
        "ICAP_PIXELFLAVOR isn't TWPF_CHOCOLATE",
    );
    s.get_current_refused(
        "ICAP_SUPPORTEDEXTIMAGEINFO",
        ICAP_SUPPORTEDEXTIMAGEINFO,
        TWCC_CAPUNSUPPORTED,
    );
    s.set_cap_refused(
        "ICAP_EXTIMAGEINFO",
        ICAP_EXTIMAGEINFO,
        TWTY_BOOL,
        1,
        TWCC_CAPUNSUPPORTED,
    );

    if !s.enable() {
        return None;
    }
    match s.wait_for_source(run.mode) {
        Some(MSG_XFERREADY) => s.transfer_ready(),
        Some(other) => {
            s.out()
                .fail(format!("the source sent {other:#06x}, not MSG_XFERREADY"));
            return None;
        }
        None => return None,
    }
    let mut pages = Vec::new();
    loop {
        let page = transfer_one(s, run.mechanism, polarity, pages.len() + 1)?;
        pages.push(page);
        s.ext_image_info();
        match s.end_transfer() {
            Some(0) => break,
            Some(_) if pages.len() >= 50 => {
                s.out().fail("50 pages and still pending: stopped");
                s.reset();
                break;
            }
            Some(_) => {}
            None => return None,
        }
    }
    if !s.disable() || !s.close_source() {
        return None;
    }
    Some(pages)
}

/// One page (state 6 to 7): its image info, the transfer, the image decoded and
/// checked against the info.
fn transfer_one(s: &mut Session, mechanism: u16, polarity: Polarity, n: usize) -> Option<Page> {
    let info = s.image_info()?;
    let (width, height) = (info.ImageWidth, info.ImageLength);
    let (samples, bits, per_pixel) = (info.SamplesPerPixel, info.BitsPerSample, info.BitsPerPixel);
    let (pixel_type, compression, planar) = (info.PixelType, info.Compression, info.Planar);
    let (x_res, y_res) = (info.XResolution.to_f64(), info.YResolution.to_f64());
    s.out().line(format!(
        "page {n}: {width}x{height}, {samples} samples of {:?} bits, {per_pixel} bits a pixel, pixel type {pixel_type}, \
         {x_res}x{y_res} ppi, compression {compression}, planar {planar}",
        &bits[..usize::try_from(samples).unwrap_or(0).min(8)]
    ));
    let format = match image::Format::from_image_info(pixel_type, samples, bits[0]) {
        Ok(f) => f,
        Err(e) => {
            s.out().fail(format!("page {n}: {e}"));
            return None;
        }
    };
    if !s.check(
        compression == TWCP_NONE && planar == 0,
        format!("page {n}: compressed or planar"),
    ) {
        return None;
    }
    let (Ok(w), Ok(h)) = (u32::try_from(width), u32::try_from(height)) else {
        s.out()
            .fail(format!("page {n}: an image of {width} by {height}"));
        return None;
    };
    let page = if mechanism == TWSX_NATIVE {
        let bytes = s.native()?;
        match image::decode_dib(&bytes) {
            Ok((image, facts)) => {
                s.out().line(format!(
                    "page {n} native: a DIB of {}x{}, {} bits, {}, palette {}, {} bytes",
                    image.width,
                    image.height,
                    facts.bits,
                    if facts.bottom_up {
                        "bottom up"
                    } else {
                        "top down"
                    },
                    image::describe_palette(&facts.palette),
                    bytes.len()
                ));
                Page {
                    image,
                    dib: Some(facts),
                }
            }
            Err(e) => {
                s.out().fail(format!("page {n}: the DIB refused: {e}"));
                return None;
            }
        }
    } else {
        let tiles = s.memory()?;
        s.out()
            .line(format!("page {n} memory: {}", describe_tiles(&tiles, w)));
        match image::assemble(w, h, format, polarity, &tiles) {
            Ok(image) => Page { image, dib: None },
            Err(e) => {
                s.out().fail(format!("page {n}: the tiles refused: {e}"));
                return None;
            }
        }
    };
    let i = &page.image;
    s.check(
        (i.width, i.height, i.format) == (w, h, format),
        format!(
            "page {n}: {}x{} {} received, {w}x{h} {} described",
            i.width,
            i.height,
            i.format.name(),
            format.name()
        ),
    );
    Some(page)
}

/// The shape of a memory transfer's buffers, in a line.
fn describe_tiles(tiles: &[Tile], width: u32) -> String {
    let strips = tiles.iter().all(|t| t.x == 0 && t.columns == width);
    let in_order = tiles
        .windows(2)
        .all(|w| (w[0].y, w[0].x) < (w[1].y, w[1].x));
    let mut shapes: Vec<(u32, u32, u32)> = tiles
        .iter()
        .map(|t| (t.columns, t.rows, t.bytes_per_row))
        .collect();
    shapes.dedup();
    let shape_list: Vec<String> = shapes
        .iter()
        .take(4)
        .map(|(c, r, b)| format!("{c}x{r} ({b} bytes a row)"))
        .collect();
    format!(
        "{} {}, {}{}, {}",
        tiles.len(),
        if strips {
            "strips of full rows"
        } else {
            "tiles"
        },
        shape_list.join(", then "),
        if shapes.len() > 4 { ", ..." } else { "" },
        if in_order {
            "top to bottom"
        } else {
            "out of order"
        },
    )
}

/// A batch of every page the feeder holds, through `mechanism` and `mode`.
fn batch(s: &mut Session, mechanism: u16, mode: Mode) {
    let expected = expected_pages();
    let Some(sample) = open_to_sample(s) else {
        return;
    };
    let run = Run {
        mechanism,
        mode,
        count: -1,
        pixel_type: None,
    };
    if let Some(pages) = scan(s, &sample, &run) {
        match expected {
            Some(n) => {
                s.check(
                    pages.len() == n as usize,
                    format!("{} pages, expected {n}", pages.len()),
                );
            }
            None => s
                .out()
                .line("the page count isn't judged: CAP_XFERCOUNT isn't a positive number"),
        }
        s.out().line(format!("{} pages transferred", pages.len()));
    }
    s.walk_down();
}

/// For each pixel type, a page native and a page through memory, compared.
fn compare(s: &mut Session) {
    let Some(sample) = open_to_sample(s) else {
        return;
    };
    for (pixel_type, format) in [
        (TWPT_RGB, Format::Rgb),
        (TWPT_GRAY, Format::Gray),
        (TWPT_BW, Format::Bw),
    ] {
        let mut one = |mechanism| {
            let run = Run {
                mechanism,
                mode: Mode::Event,
                count: 1,
                pixel_type: Some(pixel_type),
            };
            let pages = scan(s, &sample, &run)?;
            s.check(
                pages.len() == 1,
                format!("{} pages, expected 1", pages.len()),
            )
            .then(|| pages.into_iter().next())
            .flatten()
        };
        let Some(native) = one(TWSX_NATIVE) else {
            break;
        };
        let Some(memory) = one(TWSX_MEMORY) else {
            break;
        };
        let palette = native
            .dib
            .as_ref()
            .map_or("none".to_owned(), |d| image::describe_palette(&d.palette));
        match image::compare(&native.image, &memory.image) {
            Ok(()) => s.out().line(format!(
                "{}: native and memory equal pixel for pixel, {}x{} (the DIB's palette: {palette})",
                format.name(),
                native.image.width,
                native.image.height
            )),
            Err(e) => s
                .out()
                .fail(format!("{}: native and memory differ: {e}", format.name())),
        }
    }
    s.walk_down();
}
