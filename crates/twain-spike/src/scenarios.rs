//! The scenarios: what each one drives, and what it expects.

use crate::ffi::TW_IDENTITY;
use crate::session::{Session, describe};

pub struct Scenario {
    pub name: &'static str,
    pub about: &'static str,
    pub run: fn(&mut Session),
}

/// Every scenario, in the order `all` runs them.
pub const SCENARIOS: &[Scenario] = &[Scenario {
    name: "list",
    about: "states 1 to 3 and back; every source's identity",
    run: list,
}];

/// The sample source's identity, as its code declares it.
const SAMPLE_MANUFACTURER: &str = "TWAIN Working Group";
const SAMPLE_PRODUCT: &str = "TWAIN2 Software Scanner";

fn is_sample(id: &TW_IDENTITY) -> bool {
    id.Manufacturer.text() == SAMPLE_MANUFACTURER && id.ProductName.text() == SAMPLE_PRODUCT
}

/// States 1 to 3 and back: the DSM loaded and opened, its memory functions
/// driven, every source listed, the sample found exactly once.
fn list(s: &mut Session) {
    if !s.load_dsm() {
        return;
    }
    if s.open_dsm() {
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
        s.close_dsm();
    }
    s.unload_dsm();
}
