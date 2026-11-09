//! The header contract: `twain.h` compiled with MSVC for the test's own
//! target, and every struct's size, its fields' offsets and its integer
//! fields' signedness, and every numeric constant's value, compared with the
//! bindings'. A `#define` with a value it can't read fails it, so a form it
//! doesn't know can't leave a constant unchecked.
//!
//! The header grants no licence, so it's never committed: the test reads it
//! from `TWAIN_DSM_TWAIN_H` (twain-dsm commit `a50f7326a191`'s, which CI
//! fetches and checks the hash of). Without it, or without MSVC, the test
//! says so on stderr and passes, unless `TWAIN_DSM_REQUIRE_HEADER=1`, when it
//! fails.

use core::mem::{offset_of, size_of};
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::ffi;

/// Each struct's name, size and fields' offsets and kinds, from the Rust
/// side.
macro_rules! layouts {
    ($($t:ident { $($f:ident),* $(,)? })*) => {
        vec![$((
            stringify!($t),
            size_of::<ffi::$t>(),
            vec![$((stringify!($f), offset_of!(ffi::$t, $f), kind_of(|s: &ffi::$t| s.$f))),*],
        )),*]
    };
}

type Layout = (&'static str, usize, Vec<(&'static str, usize, Kind)>);

/// What a field is, for the signedness check: an integer, an array of them
/// (its first element checked), or something else.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Int { signed: bool },
    IntArray { signed: bool },
    Other,
}

trait FieldKind {
    const KIND: Kind;
}

macro_rules! ints {
    ($($t:ty => $signed:expr),* $(,)?) => {
        $(impl FieldKind for $t {
            const KIND: Kind = Kind::Int { signed: $signed };
        })*
    };
}

ints!(i8 => true, i16 => true, i32 => true, i64 => true, u8 => false, u16 => false, u32 => false, u64 => false, usize => false);

impl FieldKind for [i16; 8] {
    const KIND: Kind = Kind::IntArray { signed: true };
}
// Strings and item lists: bytes, whatever the compiler makes `char`.
impl<const N: usize> FieldKind for [u8; N] {
    const KIND: Kind = Kind::Other;
}
impl FieldKind for [ffi::TW_INFO; 1] {
    const KIND: Kind = Kind::Other;
}
impl FieldKind for ffi::TW_FIX32 {
    const KIND: Kind = Kind::Other;
}
impl FieldKind for ffi::TW_VERSION {
    const KIND: Kind = Kind::Other;
}
impl FieldKind for ffi::TW_MEMORY {
    const KIND: Kind = Kind::Other;
}
impl<T> FieldKind for *mut T {
    const KIND: Kind = Kind::Other;
}
impl<T> FieldKind for Option<T> {
    const KIND: Kind = Kind::Other;
}

/// The kind of the field `field` reads.
fn kind_of<S, F: FieldKind>(_field: impl Fn(&S) -> F) -> Kind {
    F::KIND
}

/// `twain.h`'s `#define`s with a value that are no constant: the calling
/// convention's macros.
const NOT_CONSTANTS: [&str; 3] = ["PASCAL", "TW_HUGE", "TW_CALLINGSTYLE"];

fn layouts() -> Vec<Layout> {
    layouts! {
        TW_STR32 {}
        TW_STR64 {}
        TW_STR128 {}
        TW_STR255 {}
        TW_STR1024 {}
        TW_UNI512 {}
        TW_FIX32 { Whole, Frac }
        TW_FRAME { Left, Top, Right, Bottom }
        TW_VERSION { MajorNum, MinorNum, Language, Country, Info }
        TW_IDENTITY { Id, Version, ProtocolMajor, ProtocolMinor, SupportedGroups, Manufacturer, ProductFamily, ProductName }
        TW_ONEVALUE { ItemType, Item }
        TW_ENUMERATION { ItemType, NumItems, CurrentIndex, DefaultIndex, ItemList }
        TW_ARRAY { ItemType, NumItems, ItemList }
        TW_RANGE { ItemType, MinValue, MaxValue, StepSize, DefaultValue, CurrentValue }
        TW_CAPABILITY { Cap, ConType, hContainer }
        TW_USERINTERFACE { ShowUI, ModalUI, hParent }
        TW_EVENT { pEvent, TWMessage }
        TW_PENDINGXFERS { Count, EOJ }
        TW_IMAGEINFO {
            XResolution, YResolution, ImageWidth, ImageLength, SamplesPerPixel, BitsPerSample, BitsPerPixel, Planar,
            PixelType, Compression,
        }
        TW_SETUPMEMXFER { MinBufSize, MaxBufSize, Preferred }
        TW_MEMORY { Flags, Length, TheMem }
        TW_IMAGEMEMXFER { Compression, BytesPerRow, Columns, Rows, XOffset, YOffset, BytesWritten, Memory }
        TW_STATUS { ConditionCode, Data }
        TW_INFO { InfoID, ItemType, NumItems, ReturnCode, Item }
        TW_EXTIMAGEINFO { NumInfos, Info }
        TW_DEVICEEVENT {
            Event, DeviceName, BatteryMinutes, BatteryPercentage, PowerSupply, XResolution, YResolution, FlashUsed2,
            AutomaticCapture, TimeBeforeFirstCapture, TimeBetweenCaptures,
        }
        TW_ENTRYPOINT { Size, DSM_Entry, DSM_MemAllocate, DSM_MemFree, DSM_MemLock, DSM_MemUnlock }
        TW_CALLBACK2 { CallBackProc, RefCon, Message }
    }
}

/// The names `twain.h` defines as numbers, or as another such name, in its
/// order: read as text, their values left to the compiler; and those it
/// defines with a value that's neither (none but `NOT_CONSTANTS`, or the
/// contract can't vouch for them).
fn defined_numbers(header: &str) -> (Vec<String>, Vec<String>) {
    let mut names: Vec<String> = Vec::new();
    let mut unread: Vec<String> = Vec::new();
    for line in header.lines() {
        let Some(rest) = line.trim_start().strip_prefix('#') else {
            continue;
        };
        let Some(rest) = rest.trim_start().strip_prefix("define") else {
            continue;
        };
        let mut words = rest.split_whitespace();
        let (Some(name), Some(value)) = (words.next(), words.next()) else {
            continue;
        };
        let commented = value.starts_with("//") || value.starts_with("/*");
        let value = value.trim_start_matches('(');
        let numeric = value.starts_with(|c: char| c.is_ascii_digit() || c == '-');
        let alias = names.iter().any(|n| n == value);
        if (numeric || alias) && !names.iter().any(|n| n == name) {
            names.push(name.to_owned());
        } else if !numeric && !alias && !commented && !NOT_CONSTANTS.contains(&name) {
            unread.push(format!("{name} ({value})"));
        }
    }
    (names, unread)
}

fn program(header: &Path, constants: &[String]) -> String {
    let mut c = String::new();
    c.push_str(
        "#include <windows.h>\n#include <stdio.h>\n#include <stddef.h>\n#include <string.h>\n",
    );
    let _ = writeln!(
        c,
        "#include \"{}\"",
        header.display().to_string().replace('\\', "/")
    );
    c.push_str("int main(void) {\n");
    for (name, _, fields) in layouts() {
        let _ = writeln!(
            c,
            "  printf(\"size {name} %u\\n\", (unsigned)sizeof({name}));"
        );
        for (field, _, kind) in fields {
            let _ = writeln!(
                c,
                "  printf(\"offset {name}.{field} %u\\n\", (unsigned)offsetof({name}, {field}));"
            );
            // All bits set, an integer reads below zero if it's signed.
            let first = match kind {
                Kind::Int { .. } => "",
                Kind::IntArray { .. } => "[0]",
                Kind::Other => continue,
            };
            let _ = writeln!(
                c,
                "  {{ {name} v; memset(&v, 0xff, sizeof v); printf(\"sign {name}.{field} %d\\n\", (int)(v.{field}{first} < 0)); }}"
            );
        }
    }
    for name in constants {
        let _ = writeln!(
            c,
            "#ifdef {name}\n  printf(\"const {name} %lld\\n\", (long long)({name}));\n#else\n  printf(\"undefined {name}\\n\");\n#endif"
        );
    }
    c.push_str("  return 0;\n}\n");
    c
}

fn skip_or_fail(why: &str) {
    if std::env::var("TWAIN_DSM_REQUIRE_HEADER").is_ok_and(|v| v == "1") {
        panic!("the header contract couldn't run: {why}");
    }
    eprintln!("header contract skipped: {why}");
}

#[test]
fn bindings_match_twain_h_as_msvc_compiles_it() {
    let Some(header) = std::env::var_os("TWAIN_DSM_TWAIN_H").map(PathBuf::from) else {
        return skip_or_fail("TWAIN_DSM_TWAIN_H isn't set to twain.h's path");
    };
    let text = match std::fs::read(&header) {
        Ok(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
        Err(e) => return skip_or_fail(&format!("{} couldn't be read: {e}", header.display())),
    };
    let target = if cfg!(target_arch = "x86_64") {
        "x86_64-pc-windows-msvc"
    } else if cfg!(target_arch = "x86") {
        "i686-pc-windows-msvc"
    } else {
        return skip_or_fail("the test runs on x86 and x86-64 only");
    };
    let Some(cl) = find_msvc_tools::find_tool(target, "cl.exe") else {
        return skip_or_fail(&format!("no MSVC cl.exe for {target}"));
    };

    let (constants, unread) = defined_numbers(&text);
    let dir = std::env::temp_dir().join(format!(
        "twain-dsm-contract-{target}-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let source = dir.join("contract.c");
    let exe = dir.join("contract.exe");
    std::fs::write(&source, program(&header, &constants)).unwrap();
    let built = cl
        .to_command()
        .current_dir(&dir)
        .args(["/nologo", "/W3"])
        .arg(&source)
        .arg(format!("/Fe:{}", exe.display()))
        .output()
        .unwrap();
    assert!(
        built.status.success(),
        "cl.exe failed:\n{}",
        String::from_utf8_lossy(&built.stdout)
    );
    let ran = Command::new(&exe).output().unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    assert!(ran.status.success(), "the contract program failed");
    let out = String::from_utf8(ran.stdout).unwrap();

    let mut c_sizes = BTreeMap::new();
    let mut c_offsets = BTreeMap::new();
    let mut c_constants = BTreeMap::new();
    let mut c_signs = BTreeMap::new();
    let mut problems = Vec::new();
    for line in out.lines() {
        let words: Vec<&str> = line.split(' ').collect();
        match words[..] {
            ["size", name, n] => drop(c_sizes.insert(name, n.parse::<usize>().unwrap())),
            ["offset", field, n] => drop(c_offsets.insert(field, n.parse::<usize>().unwrap())),
            ["const", name, n] => drop(c_constants.insert(name, n.parse::<i64>().unwrap())),
            ["sign", field, n] => drop(c_signs.insert(field, n == "1")),
            ["undefined", name] => problems.push(format!("{name} isn't defined on Windows")),
            _ => panic!("the contract program printed {line:?}"),
        }
    }

    for (name, size, fields) in layouts() {
        if c_sizes.get(name) != Some(&size) {
            problems.push(format!(
                "{name}: {size} bytes here, {:?} in twain.h",
                c_sizes.get(name)
            ));
        }
        for (field, offset, kind) in fields {
            let key = format!("{name}.{field}");
            if c_offsets.get(key.as_str()) != Some(&offset) {
                problems.push(format!(
                    "{key}: at {offset} here, {:?} in twain.h",
                    c_offsets.get(key.as_str())
                ));
            }
            let signed = match kind {
                Kind::Int { signed } | Kind::IntArray { signed } => signed,
                Kind::Other => continue,
            };
            if c_signs.get(key.as_str()) != Some(&signed) {
                problems.push(format!(
                    "{key}: signed {signed} here, {:?} in twain.h",
                    c_signs.get(key.as_str())
                ));
            }
        }
    }
    for define in &unread {
        problems.push(format!(
            "twain.h defines {define}, which the contract can't read"
        ));
    }

    let ours: BTreeMap<&str, i64> = ffi::constants().collect();
    for (name, value) in &c_constants {
        match ours.get(name) {
            Some(v) if v == value => {}
            Some(v) => problems.push(format!("{name}: {v} here, {value} in twain.h")),
            None => problems.push(format!("{name} ({value}) is in twain.h and not here")),
        }
    }
    for name in ours.keys() {
        if !c_constants.contains_key(name) {
            problems.push(format!("{name} is here and not in twain.h"));
        }
    }
    assert!(
        problems.is_empty(),
        "{} differences from twain.h for {target}:\n{}",
        problems.len(),
        problems.join("\n")
    );
    eprintln!(
        "header contract for {target}: {} structs, {} offsets, {} integer fields' signs and {} constants match twain.h",
        layouts().len(),
        c_offsets.len(),
        c_signs.len(),
        c_constants.len()
    );
}
