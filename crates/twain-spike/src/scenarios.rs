//! The scenarios: what each one drives, and what it expects.

use crate::container::Value;
use crate::ffi::*;
use crate::image::{self, DibFacts, Format, Image, Polarity, Shape};
use crate::session::{Mode, Session, describe, pending_text};
use crate::window::REQUEST_CANCEL;

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
    Scenario {
        name: "cancel",
        about: "a cancel posted from the main thread during page 1: the page finished, then the batch ended",
        run: cancel,
    },
    Scenario {
        name: "cancel-early",
        about: "a cancel queued before the enable, kept by the message loop, ending the batch before any transfer",
        run: cancel_early,
    },
];

/// The sample source's identity, as its code declares it.
const SAMPLE_MANUFACTURER: &str = "TWAIN Working Group";
const SAMPLE_PRODUCT: &str = "TWAIN2 Software Scanner";

/// What `CAP_XFERCOUNT` in the environment makes the sample feed: that many if
/// it's a positive number, 2 if it's unset. Anything else is refused: 0 or
/// less draws a random count, 0 and 1 included, which the checks can't judge.
fn expected_pages() -> Result<u32, String> {
    match std::env::var("CAP_XFERCOUNT") {
        Err(std::env::VarError::NotPresent) => Ok(2),
        Ok(v) => v
            .trim()
            .parse::<u32>()
            .ok()
            .filter(|&n| (1..=1000).contains(&n))
            .ok_or(format!(
                "CAP_XFERCOUNT is {v:?}: it must be unset or a page count from 1 to 1000, \
                 since 0 or less makes the sample draw a random count"
            )),
        Err(e) => Err(format!("CAP_XFERCOUNT: {e}")),
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
    /// The page whose memory transfer pauses after its first buffer for the
    /// main thread's request.
    pause_in_page: Option<usize>,
    /// Whether to wait, before enabling, until the main thread has queued a
    /// request.
    request_before_enable: bool,
}

impl Run {
    fn plain(mechanism: u16, mode: Mode, count: i16) -> Run {
        Run {
            mechanism,
            mode,
            count,
            pixel_type: None,
            pause_in_page: None,
            request_before_enable: false,
        }
    }
}

/// What a batch gave.
struct Batch {
    pages: Vec<Page>,
    /// Where a cancel ended it: after this many pages.
    cancelled_after: Option<usize>,
}

/// A batch begun, from state 3 to 6: the source opened, its capabilities set
/// and read, enabled, and its `MSG_XFERREADY` received. The polarity its
/// memory transfers use, or `None` if that failed (the session walks down).
fn prepare(s: &mut Session, sample: &TW_IDENTITY, run: &Run) -> Option<Polarity> {
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

    if run.request_before_enable && !s.await_request_queued() {
        return None;
    }
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
    Some(polarity)
}

/// One batch, from state 3 to 3: begun, every page transferred while the count
/// isn't 0, disabled, closed. A cancel request ends it at the next point the
/// source is in state 6: before the first transfer, or after a page's
/// `MSG_ENDXFER`. The pages, or `None` if the batch failed (the session walks
/// down). With `expected`, the pending count is checked after each page.
fn scan(s: &mut Session, sample: &TW_IDENTITY, run: &Run, expected: Option<u32>) -> Option<Batch> {
    let polarity = prepare(s, sample, run)?;
    let mut pages = Vec::new();
    let mut cancelled_after = None;
    if let Some(request) = s.take_request() {
        cancelled_after = Some(cancel_batch(s, run.mode, request, 0)?);
    }
    while cancelled_after.is_none() {
        let n = pages.len() + 1;
        let page = transfer_one(s, run.mechanism, polarity, n, run.pause_in_page == Some(n))?;
        pages.push(page);
        s.ext_image_info();
        let pending = s.end_transfer()?;
        if let Some(want) = pending_expected(run.count, expected, n) {
            s.check(
                pending == want,
                format!(
                    "pending after page {n}: {}, expected {}",
                    pending_text(pending),
                    pending_text(want)
                ),
            );
        }
        if pending == 0 {
            break;
        }
        if let Some(request) = s.take_request() {
            cancelled_after = Some(cancel_batch(s, run.mode, request, n)?);
        } else if n >= 50 {
            s.out().fail("50 pages and still pending: stopped");
            s.reset();
            break;
        }
    }
    if !s.disable() || !s.close_source() {
        return None;
    }
    Some(Batch {
        pages,
        cancelled_after,
    })
}

/// The pending count the sample gives after page `n`: with the capability -1
/// and `expected` pages, "don't know" until the last, then 0; with a count of
/// 1, 0. `None` where it isn't known.
fn pending_expected(count: i16, expected: Option<u32>, n: usize) -> Option<u16> {
    match (count, expected) {
        (-1, Some(e)) if n < e as usize => Some(TWON_DONTCARE16),
        (-1, Some(e)) if n == e as usize => Some(0),
        (1, _) => Some(0),
        _ => None,
    }
}

/// A request acted on in state 6, after `pages` pages: a cancel tries
/// `MSG_STOPFEEDER` (the sample refuses it), `MSG_RESET`s the batch (6 to 5),
/// and checks the source says nothing more; the disable that follows shows
/// it's in state 5, since the sample refuses `MSG_DISABLEDS` in any other.
/// `pages`, or `None` if it failed.
fn cancel_batch(s: &mut Session, mode: Mode, request: usize, pages: usize) -> Option<usize> {
    if !s.check(
        request == REQUEST_CANCEL,
        format!("request {request}, not a cancel"),
    ) {
        return None;
    }
    s.out().line(format!(
        "the cancel acted on after {pages} pages, the source in state 6"
    ));
    s.stop_feeder();
    if !s.reset() {
        return None;
    }
    if let Some(msg) = s.quiet_for(mode, 1000) {
        s.out()
            .fail(format!("the source sent {msg:#06x} after MSG_RESET"));
        return None;
    }
    Some(pages)
}

/// One page (state 6 to 7): its image info, the transfer, the image decoded and
/// checked against the info. With `pause`, a memory transfer pauses after its
/// first buffer for the main thread's request.
fn transfer_one(
    s: &mut Session,
    mechanism: u16,
    polarity: Polarity,
    n: usize,
    pause: bool,
) -> Option<Page> {
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
    let format = match Format::from_image_info(pixel_type, samples, bits[0]) {
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
        let (assembler, shapes) = s.memory((w, h, format), pause.then_some(n))?;
        s.out()
            .line(format!("page {n} memory: {}", describe_shapes(&shapes, w)));
        match assembler.finish(polarity) {
            Ok(image) => Page { image, dib: None },
            Err(e) => {
                s.out().fail(format!("page {n}: the buffers refused: {e}"));
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
fn describe_shapes(shapes: &[Shape], width: u32) -> String {
    let strips = shapes.iter().all(|t| t.x == 0 && t.columns == width);
    let in_order = shapes
        .windows(2)
        .all(|w| (w[0].y, w[0].x) < (w[1].y, w[1].x));
    let mut kinds: Vec<(u32, u32, u32)> = shapes
        .iter()
        .map(|t| (t.columns, t.rows, t.bytes_per_row))
        .collect();
    kinds.dedup();
    let listed: Vec<String> = kinds
        .iter()
        .take(4)
        .map(|(c, r, b)| format!("{c}x{r} ({b} bytes a row)"))
        .collect();
    format!(
        "{} {}, {}{}, {}",
        shapes.len(),
        if strips {
            "strips of full rows"
        } else {
            "tiles"
        },
        listed.join(", then "),
        if kinds.len() > 4 { ", ..." } else { "" },
        if in_order {
            "top to bottom"
        } else {
            "out of order"
        },
    )
}

/// A batch of every page the feeder holds, through `mechanism` and `mode`.
fn batch(s: &mut Session, mechanism: u16, mode: Mode) {
    let expected = match expected_pages() {
        Ok(n) => n,
        Err(e) => return s.out().fail(e),
    };
    if let Some(sample) = open_to_sample(s)
        && let Some(batch) = scan(s, &sample, &Run::plain(mechanism, mode, -1), Some(expected))
    {
        let n = batch.pages.len();
        s.check(
            n == expected as usize,
            format!("{n} pages, expected {expected}"),
        );
        s.check(
            batch.cancelled_after.is_none(),
            "the batch was cancelled, and nothing asked it to be",
        );
        s.out().line(format!("{n} pages transferred"));
    } else {
        s.out().fail("the batch didn't complete");
    }
    s.walk_down();
}

/// For each pixel type, a page native and a page through memory, compared.
fn compare(s: &mut Session) {
    if let Err(e) = expected_pages() {
        return s.out().fail(e);
    }
    let Some(sample) = open_to_sample(s) else {
        return s.walk_down();
    };
    for (pixel_type, format) in [
        (TWPT_RGB, Format::Rgb),
        (TWPT_GRAY, Format::Gray),
        (TWPT_BW, Format::Bw),
    ] {
        let mut one = |mechanism| {
            let run = Run {
                pixel_type: Some(pixel_type),
                ..Run::plain(mechanism, Mode::Event, 1)
            };
            let batch = scan(s, &sample, &run, Some(1))?;
            let n = batch.pages.len();
            s.check(n == 1, format!("{n} pages, expected 1"))
                .then(|| batch.pages.into_iter().next())
                .flatten()
        };
        let Some(native) = one(TWSX_NATIVE) else {
            s.out().fail("the batch didn't complete");
            break;
        };
        let Some(memory) = one(TWSX_MEMORY) else {
            s.out().fail("the batch didn't complete");
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

/// A cancel posted from the main thread while page 1 is in flight, at a fixed
/// point: the TWAIN thread pauses after the page's first buffer until the
/// request comes, finishes the page, ends the transfer, then acts on the
/// cancel (`scan`). Memory transfers, event mode; the feeder must hold two
/// pages or more.
fn cancel(s: &mut Session) {
    let expected = match expected_pages() {
        Ok(n) if n >= 2 => n,
        Ok(n) => {
            return s.out().fail(format!(
                "a cancel needs two pages or more in the feeder, and CAP_XFERCOUNT is {n}"
            ));
        }
        Err(e) => return s.out().fail(e),
    };
    let run = Run {
        pause_in_page: Some(1),
        ..Run::plain(TWSX_MEMORY, Mode::Event, -1)
    };
    if let Some(sample) = open_to_sample(s)
        && let Some(batch) = scan(s, &sample, &run, Some(expected))
    {
        let n = batch.pages.len();
        s.check(
            n == 1,
            format!("{n} pages transferred around the cancel, not 1"),
        );
        s.check(
            batch.cancelled_after == Some(1),
            format!(
                "the cancel acted on after {:?} pages, not 1",
                batch.cancelled_after
            ),
        );
        s.out().line(format!(
            "{n} page transferred; the cancel ended the batch after it, {} more pending",
            expected - 1
        ));
    } else {
        s.out().fail("the batch didn't complete");
    }
    s.walk_down();
}

/// A cancel the main thread posts before the enable, while the TWAIN thread
/// waits for it to be queued: the message loop that waits for the source's
/// `MSG_XFERREADY` meets it first and keeps it, and the batch ends before any
/// transfer. Callback mode, where the source's signal is posted inside
/// `MSG_ENABLEDS`, after the request, so the loop must pass the request on
/// its way.
fn cancel_early(s: &mut Session) {
    if let Err(e) = expected_pages() {
        return s.out().fail(e);
    }
    let run = Run {
        request_before_enable: true,
        ..Run::plain(TWSX_NATIVE, Mode::Callback, -1)
    };
    if let Some(sample) = open_to_sample(s)
        && let Some(batch) = scan(s, &sample, &run, None)
    {
        let n = batch.pages.len();
        s.check(
            s.kept_by_loop() == 1,
            format!("the message loop kept {} requests, not 1", s.kept_by_loop()),
        );
        s.check(n == 0, format!("{n} pages transferred, not 0"));
        s.check(
            batch.cancelled_after == Some(0),
            format!(
                "the cancel acted on after {:?} pages, not 0",
                batch.cancelled_after
            ),
        );
        s.out().line(
            "no page transferred; the cancel kept by the loop ended the batch before the first",
        );
    } else {
        s.out().fail("the batch didn't complete");
    }
    s.walk_down();
}
