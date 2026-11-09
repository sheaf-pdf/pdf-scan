//! Options against the fake libsane: what SANE's own backends can't show (the
//! feeder and duplex layouts of particular backends, descriptors that change
//! after a set, and hostile descriptors), each refusal seen in the fake's log
//! as no call reaching it, and the log checked for SANE's rules each time.
//! Each test runs in a child process of its own.

#[macro_use]
mod common;

use std::time::Duration;

use common::Fake;
use sane_dl::{
    Call, Capabilities, Constraint, Error, Fixed, Handle, LimitKind, SaneString, Status, Unit,
    Value, ValueType,
};

/// A device to open, then the script's options.
const DEVICE: &str = "device fake:0 Fake Fake scanner\n";

fn index(handle: &Handle, name: &str) -> usize {
    handle
        .find_option(name)
        .unwrap_or_else(|| panic!("no option {name}"))
        .index
}

fn string(text: &str) -> Value {
    Value::String(text.into())
}

#[test]
#[cfg_attr(not(unix), ignore = "loads the fake by SANE's name: none on Windows")]
fn opening_reads_every_descriptor_and_its_count() {
    let script = format!(
        "{DEVICE}{}",
        r#"
        option @null GROUP 0 -
        title Standard
        option source STRING 16 soft-select+soft-detect
        title "Scan source"
        desc "Where the paper comes from."
        strings Flatbed ADF
        value ADF
        option res INT 4 soft-select+soft-detect+automatic
        unit DPI
        range 50 600 2
        value 300
        option tables INT 16 soft-select+soft-detect+advanced
        words 0 1 2
        value 1 2 3 4
        option go BUTTON 0 soft-select
        "#
    );
    let fake = Fake::new("opening_reads_every_descriptor_and_its_count", &script);
    in_child!(
        opening_reads_every_descriptor_and_its_count,
        fake.child(),
        || {
            let sane = common::load_fake();
            let mut handle = sane.open("fake:0").unwrap();
            let options = handle.options();
            assert_eq!(options.len(), 6);
            assert_eq!(options[0].title, "Number of options");
            assert!(options[1].is_group());
            assert_eq!(options[1].title, "Standard");
            let source = &options[2];
            assert_eq!(
                (
                    source.name.as_str(),
                    source.title.as_str(),
                    source.description.as_str()
                ),
                ("source", "Scan source", "Where the paper comes from.")
            );
            assert_eq!(
                (source.value_type, source.size, source.capabilities),
                (ValueType::String, 16, Capabilities(5))
            );
            assert_eq!(
                source.constraint,
                Constraint::StringList(vec!["Flatbed".into(), "ADF".into()])
            );
            let res = &options[3];
            assert_eq!(
                (res.unit, &res.constraint),
                (
                    Unit::Dpi,
                    &Constraint::Range {
                        min: 50,
                        max: 600,
                        quant: 2
                    }
                )
            );
            assert!(res.capabilities.automatic());
            assert_eq!(options[4].word_count(), 4);
            assert!(options[4].capabilities.advanced());
            assert_eq!(options[5].value_type, ValueType::Button);
            assert_eq!(handle.get(2).unwrap(), string("ADF"));
            assert_eq!(handle.get(4).unwrap(), Value::Int(vec![1, 2, 3, 4]));
        }
    );
    let log = fake.log();
    log.assert_no_violations();
    let calls = log.calls();
    // Option 0's descriptor, its count, then each other descriptor.
    assert_eq!(
        calls[..9],
        [
            "sane_init",
            "sane_open",
            "sane_get_option_descriptor",
            "sane_control_option",
            "sane_get_option_descriptor",
            "sane_get_option_descriptor",
            "sane_get_option_descriptor",
            "sane_get_option_descriptor",
            "sane_get_option_descriptor",
        ]
    );
    assert_eq!(
        log.controls()[0].split(' ').skip(1).collect::<Vec<_>>(),
        ["option=0", "action=GET"]
    );
    // Each get reads its descriptor again first, then the value.
    assert_eq!(
        calls[9..],
        [
            "sane_get_option_descriptor",
            "sane_control_option",
            "sane_get_option_descriptor",
            "sane_control_option",
            "sane_close",
            "sane_exit"
        ]
    );
}

/// How a profile's feeder scans both sides.
enum Duplex {
    /// A value of `source`.
    Source(&'static str),
    /// Another option, set to this value.
    Option(&'static str, Value),
}

/// Checks a profile as `sane-dl` reads it: its source and duplex typed,
/// the page size active only for the feeder, and duplex set.
fn check_profile(handle: &mut Handle, flatbed: Option<&str>, feeder: &str, duplex: Duplex) {
    let source = index(handle, "source");
    let descriptor = &handle.options()[source];
    assert_eq!(descriptor.value_type, ValueType::String);
    let Constraint::StringList(sources) = &descriptor.constraint else {
        panic!("{descriptor:?}")
    };
    assert!(sources.contains(&SaneString::from(feeder)), "{sources:?}");
    for name in ["page-height", "page-width"] {
        let page = &handle.options()[index(handle, name)];
        assert_eq!((page.value_type, page.unit), (ValueType::Fixed, Unit::Mm));
        assert!(matches!(page.constraint, Constraint::Range { .. }));
    }
    let height = index(handle, "page-height");
    let resolution = &handle.options()[index(handle, "resolution")];
    assert_eq!(
        (resolution.value_type, resolution.unit),
        (ValueType::Int, Unit::Dpi)
    );
    if let Some(flatbed) = flatbed {
        handle.set(source, &string(flatbed)).unwrap();
        assert!(handle.options()[height].capabilities.inactive());
        assert!(matches!(handle.get(height), Err(Error::Inactive { .. })));
    }
    let info = handle.set(source, &string(feeder)).unwrap();
    assert!(handle.options()[height].capabilities.is_active());
    assert_eq!(info.reload_options, flatbed.is_some(), "{info:?}");
    assert_eq!(
        handle.get(height).unwrap(),
        Value::Fixed(vec![Fixed::from_f64(279.4).unwrap()])
    );
    match duplex {
        Duplex::Source(value) => {
            handle.set(source, &string(value)).unwrap();
            assert_eq!(handle.get(source).unwrap(), string(value));
        }
        Duplex::Option(name, value) => {
            let option = index(handle, name);
            assert!(handle.options()[option].capabilities.is_active());
            assert_eq!(
                handle.options()[option].value_type,
                value.value_type(),
                "{name}"
            );
            handle.set(option, &value).unwrap();
            assert_eq!(handle.get(option).unwrap(), value);
        }
    }
    handle
        .set(height, &Value::Fixed(vec![Fixed::from_f64(300.0).unwrap()]))
        .unwrap();
}

macro_rules! profile_test {
    ($test:ident, $profile:literal, $flatbed:expr, $feeder:expr, $duplex:expr) => {
        #[test]
        #[cfg_attr(not(unix), ignore = "loads the fake by SANE's name: none on Windows")]
        fn $test() {
            let script = format!("{DEVICE}profile {}\n", $profile);
            let fake = Fake::new(stringify!($test), &script);
            in_child!($test, fake.child(), || {
                let sane = common::load_fake();
                let mut handle = sane.open("fake:0").unwrap();
                for option in handle.options() {
                    assert_eq!(option.unusable, None, "{option:?}");
                }
                check_profile(&mut handle, $flatbed, $feeder, $duplex);
            });
            fake.log().assert_no_violations();
        }
    };
}

profile_test!(
    fujitsus_sources_name_the_sides,
    "fujitsu",
    Some("Flatbed"),
    "ADF Front",
    Duplex::Source("ADF Duplex")
);
profile_test!(
    airscans_sources_name_the_duplex,
    "airscan",
    Some("Flatbed"),
    "ADF",
    Duplex::Source("ADF Duplex")
);
profile_test!(
    brothers_long_sources_are_read_whole,
    "brother",
    Some("FlatBed"),
    "Automatic Document Feeder(left aligned)",
    Duplex::Source("Automatic Document Feeder(left aligned,Duplex)")
);
profile_test!(
    canon_drs_scan_mode_is_its_own_option,
    "canon-dr",
    None,
    "ADF",
    Duplex::Option("ScanMode", string("Duplex"))
);
profile_test!(
    lexmarks_both_sides_is_a_bool,
    "lexmark",
    Some("Flatbed"),
    "Automatic Document Feeder",
    Duplex::Option("scan-both-sides", Value::Bool(true))
);
profile_test!(
    epsons_adf_mode_is_its_own_option,
    "epson",
    Some("Flatbed"),
    "Automatic Document Feeder",
    Duplex::Option("adf-mode", string("Duplex"))
);
profile_test!(
    kodaks_duplex_is_a_string,
    "kodak",
    None,
    "ADF",
    Duplex::Option("duplex", string("both"))
);

#[test]
#[cfg_attr(not(unix), ignore = "loads the fake by SANE's name: none on Windows")]
fn a_constraint_that_changes_is_read_again() {
    let fake = Fake::new(
        "a_constraint_that_changes_is_read_again",
        &format!("{DEVICE}profile airscan\n"),
    );
    in_child!(
        a_constraint_that_changes_is_read_again,
        fake.child(),
        || {
            let sane = common::load_fake();
            let mut handle = sane.open("fake:0").unwrap();
            let source = index(&handle, "source");
            let resolution = index(&handle, "resolution");
            let all = Constraint::WordList(vec![75, 100, 150, 200, 300, 600]);
            assert_eq!(handle.options()[resolution].constraint, all);
            let info = handle.set(source, &string("ADF Duplex")).unwrap();
            assert!(info.reload_options && info.reload_params);
            assert_eq!(
                handle.options()[resolution].constraint,
                Constraint::WordList(vec![100, 200, 300])
            );
            // The backend's fit of a value no longer listed is said.
            let info = handle.set(resolution, &Value::Int(vec![600])).unwrap();
            assert!(info.inexact);
            assert_eq!(handle.get(resolution).unwrap(), Value::Int(vec![300]));
            handle.set(source, &string("ADF")).unwrap();
            assert_eq!(handle.options()[resolution].constraint, all);
        }
    );
    // The fake's rule check: every descriptor was read again before the
    // next option call.
    fake.log().assert_no_violations();
}

#[test]
#[cfg_attr(not(unix), ignore = "loads the fake by SANE's name: none on Windows")]
fn each_refusal_reaches_no_backend() {
    let script = format!(
        "{DEVICE}{}",
        r#"
        option @null GROUP 0 -
        option off INT 4 soft-select+soft-detect+inactive
        option ro INT 4 soft-detect
        option hw BOOL 4 hard-select
        option manual INT 4 soft-select+soft-detect
        option pair INT 8 soft-select+soft-detect
        option text STRING 4 soft-select+soft-detect
        option go BUTTON 0 soft-select+soft-detect
        option odd 9 4 soft-select+soft-detect
        option bool BOOL 4 soft-select+soft-detect
        "#
    );
    let fake = Fake::new("each_refusal_reaches_no_backend", &script);
    in_child!(each_refusal_reaches_no_backend, fake.child(), || {
        let sane = common::load_fake();
        let mut handle = sane.open("fake:0").unwrap();
        let one = Value::Int(vec![1]);
        let refusals: Vec<(usize, Result<(), Error>)> = vec![
            (1, handle.get(1).map(drop)),
            (2, handle.get(2).map(drop)),
            (2, handle.set(2, &one).map(drop)),
            (2, handle.set_auto(2).map(drop)),
            (3, handle.set(3, &one).map(drop)),
            (3, handle.set_auto(3).map(drop)),
            (4, handle.get(4).map(drop)),
            (4, handle.set(4, &Value::Bool(true)).map(drop)),
            (5, handle.set_auto(5).map(drop)),
            (5, handle.set(5, &Value::Fixed(vec![Fixed(1)])).map(drop)),
            (6, handle.set(6, &one).map(drop)),
            (6, handle.set(6, &Value::Int(vec![1, 2, 3])).map(drop)),
            (7, handle.set(7, &string("four")).map(drop)),
            (7, handle.set(7, &string("a\0")).map(drop)),
            (8, handle.get(8).map(drop)),
            (8, handle.set(8, &Value::Bool(true)).map(drop)),
            (9, handle.get(9).map(drop)),
            (10, handle.set(10, &one).map(drop)),
            (11, handle.get(11).map(drop)),
            (99, handle.set_auto(99).map(drop)),
        ];
        let expected = [
            "NoValue",
            "Inactive",
            "Inactive",
            "Inactive",
            "NotSettable",
            "NotSettable",
            "NotReadable",
            "NotSettable",
            "NotAutomatic",
            "WrongType",
            "WrongLength",
            "WrongLength",
            "StringTooLong",
            "NulInString",
            "NoValue",
            "WrongType",
            "Unusable",
            "WrongType",
            "NoSuchOption",
            "NoSuchOption",
        ];
        for ((option, refusal), expected) in refusals.into_iter().zip(expected) {
            let error = refusal.expect_err(expected);
            let shown = format!("{error:?}");
            assert!(shown.starts_with(expected), "option {option}: {shown}");
        }
        // What fits gets through.
        handle.set(5, &one).unwrap();
        handle.set(6, &Value::Int(vec![1, 2])).unwrap();
        handle.set(7, &string("abc")).unwrap();
        handle.set(8, &Value::Button).unwrap();
        handle.set(10, &Value::Bool(true)).unwrap();
        assert_eq!(handle.get(10).unwrap(), Value::Bool(true));
        assert_eq!(handle.get(3).unwrap(), Value::Int(vec![0]));
    });
    let log = fake.log();
    log.assert_no_violations();
    for option in [1, 2, 4, 9, 11, 99] {
        assert_eq!(
            log.controls_of(option),
            Vec::<&str>::new(),
            "option {option}"
        );
    }
    assert_eq!(log.controls_of(3).len(), 1, "only the get that fits");
    assert_eq!(log.controls_of(5).len(), 1);
    // The values as they reach the backend: an array whole, a string with
    // its NUL inside the option's size, a button and a BOOL as words.
    let detail = |option: usize| {
        log.controls_of(option)[0]
            .split_once(" option=")
            .unwrap()
            .1
            .to_owned()
    };
    assert_eq!(detail(6), "6 action=SET value=[1,2]");
    assert_eq!(detail(7), "7 action=SET value=\"abc\"");
    assert_eq!(detail(8), "8 action=SET value=[1]");
    assert_eq!(detail(10), "10 action=SET value=[1]");
}

#[test]
#[cfg_attr(not(unix), ignore = "loads the fake by SANE's name: none on Windows")]
fn hostile_descriptors_make_options_unusable() {
    let script = format!(
        "{DEVICE}{}",
        r#"
        option odd-type 9 4 soft-select+soft-detect
        option odd-unit INT 4 soft-select+soft-detect
        unit 99
        option odd-constraint INT 4 soft-select+soft-detect
        constraint 7
        option strings-on-int INT 4 soft-select+soft-detect
        strings a b
        constraint STRING_LIST
        option range-on-string STRING 8 soft-select+soft-detect
        range 0 1 0
        option null-range INT 4 soft-select+soft-detect
        constraint RANGE
        option null-words INT 4 soft-select+soft-detect
        constraint WORD_LIST
        option null-strings STRING 8 soft-select+soft-detect
        constraint STRING_LIST
        option size-0 INT 0 soft-select+soft-detect
        option size-negative INT -4 soft-select+soft-detect
        option size-odd INT 6 soft-select+soft-detect
        option size-huge INT 1073741824 soft-select+soft-detect
        option bool-size BOOL 8 soft-select+soft-detect
        option string-size STRING 0 soft-select+soft-detect
        option words-claim-max INT 4 soft-select+soft-detect
        words 1 2
        word-list-length 2147483647
        option words-claim-min INT 4 soft-select+soft-detect
        words 1 2
        word-list-length -2147483648
        option long-list STRING 8 soft-select+soft-detect
        strings a
        strings-repeat 70000
        option long-entry STRING 8 soft-select+soft-detect
        strings a @long:70000
        option @null INT 4 soft-select+soft-detect
        title @null
        desc @null
        option long-text INT 4 soft-select+soft-detect
        title @long:70000
        desc @long:70000
        option bool-2 BOOL 4 soft-select+soft-detect
        value 2
        option no-nul STRING 4 soft-select+soft-detect
        value abcd
        "#
    );
    let fake = Fake::new("hostile_descriptors_make_options_unusable", &script);
    in_child!(
        hostile_descriptors_make_options_unusable,
        fake.child(),
        || {
            let sane = common::load_fake();
            let mut handle = sane.open("fake:0").unwrap();
            let found: Vec<(String, Option<String>)> = handle
                .options()
                .iter()
                .skip(1)
                .map(|option| (option.name.clone(), option.unusable.clone()))
                .collect();
            let expected: [(&str, Option<&str>); 22] = [
                ("odd-type", Some("its value type 9 isn't SANE's")),
                ("odd-unit", None),
                ("odd-constraint", Some("its constraint type 7 isn't SANE's")),
                (
                    "strings-on-int",
                    Some("it has a string list constraint, which its type, INT, can't have"),
                ),
                (
                    "range-on-string",
                    Some("it has a range constraint, which its type, STRING, can't have"),
                ),
                ("null-range", Some("its range is null")),
                ("null-words", Some("its word list is null")),
                ("null-strings", Some("its string list is null")),
                ("size-0", Some("its size is 0")),
                ("size-negative", Some("its size is -4")),
                ("size-odd", Some("its size is 6, not a multiple of a word")),
                (
                    "size-huge",
                    Some("its size is 1073741824, more than 1048576 bytes"),
                ),
                ("bool-size", Some("its size is 8, and a BOOL's is a word")),
                ("string-size", Some("its size is 0")),
                (
                    "words-claim-max",
                    Some("its word list claims 2147483647 entries, more than 65536"),
                ),
                (
                    "words-claim-min",
                    Some("its word list claims -2147483648 entries"),
                ),
                (
                    "long-list",
                    Some("its string list has more than 65536 entries"),
                ),
                (
                    "long-entry",
                    Some("entry 1 of its string list is longer than 65536 bytes"),
                ),
                ("", None),
                ("long-text", None),
                ("bool-2", None),
                ("no-nul", None),
            ];
            let expected: Vec<(String, Option<String>)> = expected
                .iter()
                .map(|(name, reason)| ((*name).to_owned(), reason.map(str::to_owned)))
                .collect();
            assert_eq!(found, expected);
            let options = handle.options();
            assert_eq!(options[2].unit, Unit::Other(99));
            assert_eq!(options[1].value_type, ValueType::Unknown(9));
            // Null strings are empty; text past the cap is cut there.
            let nulls = &options[19];
            assert_eq!((nulls.name.as_str(), nulls.title.as_str()), ("", ""));
            let long = &options[20];
            assert_eq!(long.title.len(), 65536);
            assert_eq!(long.description.len(), 65536);
            // None of the unusable ones can be used.
            for option in 1..=18 {
                if option == 2 {
                    continue;
                }
                assert!(
                    matches!(handle.get(option), Err(Error::Unusable { .. })),
                    "option {option}"
                );
                assert!(matches!(
                    handle.set(option, &Value::Int(vec![1])),
                    Err(Error::Unusable { .. })
                ));
            }
            handle.get(2).unwrap();
            // Values that can't be trusted.
            match handle.get(21) {
                Err(Error::BadValue { option: 21, reason }) => {
                    assert_eq!(reason, "a BOOL word of 2, neither 0 nor 1")
                }
                other => panic!("{other:?}"),
            }
            match handle.get(22) {
                Err(Error::BadValue { option: 22, reason }) => {
                    assert_eq!(reason, "a string with no NUL in its 4 bytes")
                }
                other => panic!("{other:?}"),
            }
        }
    );
    let log = fake.log();
    log.assert_no_violations();
    for option in (1..=18).filter(|&option| option != 2) {
        assert_eq!(
            log.controls_of(option),
            Vec::<&str>::new(),
            "option {option}"
        );
    }
}

/// A test that opening a device with `script`'s options fails with
/// `BadDescriptor` for `option`, its reason holding `reason`, and closes
/// the device again.
macro_rules! bad_descriptor_test {
    ($test:ident, $script:expr, $option:expr, $reason:expr) => {
        #[test]
        #[cfg_attr(not(unix), ignore = "loads the fake by SANE's name: none on Windows")]
        fn $test() {
            let fake = Fake::new(stringify!($test), &format!("{DEVICE}{}", $script));
            in_child!($test, fake.child(), || {
                let sane = common::load_fake();
                match sane.open("fake:0") {
                    Err(Error::BadDescriptor { option, reason }) => {
                        assert_eq!(option, $option);
                        assert!(reason.contains($reason), "{reason}");
                    }
                    other => panic!("expected BadDescriptor, got {other:?}"),
                }
            });
            let log = fake.log();
            log.assert_no_violations();
            assert_eq!(
                log.calls_but_options(),
                ["sane_init", "sane_open", "sane_close", "sane_exit"]
            );
        }
    };
}

#[test]
#[cfg_attr(not(unix), ignore = "loads the fake by SANE's name: none on Windows")]
fn a_count_of_a_billion_options_is_refused_unread() {
    let fake = Fake::new(
        "a_count_of_a_billion_options_is_refused_unread",
        &format!("{DEVICE}option-count 1000000000\n"),
    );
    in_child!(
        a_count_of_a_billion_options_is_refused_unread,
        fake.child(),
        || {
            let sane = common::load_fake();
            match sane.open("fake:0") {
                Err(Error::LimitExceeded {
                    limit: LimitKind::Options,
                    value: 1_000_000_000,
                    cap: 4096,
                }) => {}
                other => panic!("expected LimitExceeded, got {other:?}"),
            }
        }
    );
    let log = fake.log();
    log.assert_no_violations();
    assert_eq!(
        log.calls_but_options(),
        ["sane_init", "sane_open", "sane_close", "sane_exit"]
    );
}
bad_descriptor_test!(
    a_count_of_none_is_refused,
    "option-count 0\n",
    0,
    "it says there are 0 options, and option 0 is one"
);
bad_descriptor_test!(
    a_negative_count_is_refused,
    "option-count -1\n",
    0,
    "it says there are -1 options"
);
bad_descriptor_test!(
    a_count_past_the_descriptors_finds_a_null_one,
    "option a INT 4 soft-detect\noption-count 3\n",
    2,
    "gave none, though option 0 counts it"
);
bad_descriptor_test!(
    a_null_descriptor_below_the_count_is_refused,
    "option a INT 4 soft-detect\noption-null\noption b INT 4 soft-detect\n",
    2,
    "gave none, though option 0 counts it"
);
bad_descriptor_test!(
    a_name_past_the_cap_is_refused,
    "option @long:70000 INT 4 soft-detect\n",
    1,
    "its name is longer than 65536 bytes"
);

#[test]
#[cfg_attr(not(unix), ignore = "loads the fake by SANE's name: none on Windows")]
fn caps_hold_at_their_limit_and_refuse_one_past() {
    let script = format!(
        "{DEVICE}{}",
        r#"
        option w INT 4 soft-select+soft-detect
        words 1 2 3
        option s STRING 8 soft-select+soft-detect
        strings a b c
        option z INT 12 soft-select+soft-detect
        option xxxx INT 4 soft-select+soft-detect
        "#
    );
    let fake = Fake::new("caps_hold_at_their_limit_and_refuse_one_past", &script);
    in_child!(
        caps_hold_at_their_limit_and_refuse_one_past,
        fake.child(),
        || {
            let sane = common::load_fake();
            let mut at = sane.limits();
            at.options = 5;
            at.list_entries = 3;
            at.option_bytes = 12;
            at.string_bytes = 4;
            sane.set_limits(at);
            let mut handle = sane.open("fake:0").unwrap();
            for option in handle.options() {
                assert_eq!(option.unusable, None, "{option:?}");
            }
            assert_eq!(handle.get(3).unwrap(), Value::Int(vec![0; 3]));
            drop(handle);
            let mut under = at;
            under.list_entries = 2;
            under.option_bytes = 8;
            sane.set_limits(under);
            let handle = sane.open("fake:0").unwrap();
            let reasons: Vec<Option<&str>> = handle
                .options()
                .iter()
                .map(|option| option.unusable.as_deref())
                .collect();
            assert_eq!(
                reasons,
                [
                    None,
                    Some("its word list claims 3 entries, more than 2"),
                    Some("its string list has more than 2 entries"),
                    Some("its size is 12, more than 8 bytes"),
                    None
                ]
            );
            drop(handle);
            let mut fewer = at;
            fewer.options = 4;
            sane.set_limits(fewer);
            assert!(matches!(
                sane.open("fake:0"),
                Err(Error::LimitExceeded {
                    limit: LimitKind::Options,
                    value: 5,
                    cap: 4
                })
            ));
            let mut shorter = at;
            shorter.string_bytes = 3;
            sane.set_limits(shorter);
            assert!(matches!(
                sane.open("fake:0"),
                Err(Error::BadDescriptor { option: 4, .. })
            ));
        }
    );
    fake.log().assert_no_violations();
}

#[test]
#[cfg_attr(not(unix), ignore = "loads the fake by SANE's name: none on Windows")]
fn a_groups_undefined_fields_are_never_read() {
    // The standard defines only a group's title and type: its name and
    // description here point to no memory, which reading would crash on.
    let script = format!(
        "{DEVICE}{}",
        r#"
        option @dangling GROUP 0 -
        title Standard
        desc @dangling
        option res INT 4 soft-select+soft-detect
        value 300
        "#
    );
    let fake = Fake::new("a_groups_undefined_fields_are_never_read", &script);
    in_child!(
        a_groups_undefined_fields_are_never_read,
        fake.child(),
        || {
            let sane = common::load_fake();
            let mut handle = sane.open("fake:0").unwrap();
            let group = &handle.options()[1];
            assert!(group.is_group());
            assert_eq!(
                (
                    group.title.as_str(),
                    group.name.as_str(),
                    group.description.as_str()
                ),
                ("Standard", "", "")
            );
            assert_eq!(handle.get(2).unwrap(), Value::Int(vec![300]));
            handle.reload_options().unwrap();
            assert_eq!(handle.options()[1].title, "Standard");
        }
    );
    fake.log().assert_no_violations();
}

#[test]
#[cfg_attr(not(unix), ignore = "loads the fake by SANE's name: none on Windows")]
fn a_descriptor_changed_without_a_word_is_read_before_the_call() {
    // Setting `s` makes `t` two words, and the backend doesn't say so.
    let script = format!(
        "{DEVICE}{}",
        r#"
        option s STRING 8 soft-select+soft-detect
        when wide size t 8
        when wide quiet
        when off deactivate t
        when off quiet
        option t INT 4 soft-select+soft-detect
        value 7
        "#
    );
    let fake = Fake::new(
        "a_descriptor_changed_without_a_word_is_read_before_the_call",
        &script,
    );
    in_child!(
        a_descriptor_changed_without_a_word_is_read_before_the_call,
        fake.child(),
        || {
            let sane = common::load_fake();
            let mut handle = sane.open("fake:0").unwrap();
            assert_eq!(handle.get(2).unwrap(), Value::Int(vec![7]));
            let info = handle.set(1, &string("wide")).unwrap();
            assert!(!info.reload_options);
            assert_eq!(handle.options()[2].size, 4, "not read again yet");
            assert_eq!(handle.options_generation(), 0);
            // The get reads t's descriptor first, sees it changed, reads them
            // all, and sizes its buffer to two words; the count says so.
            assert_eq!(handle.get(2).unwrap(), Value::Int(vec![7, 7]));
            assert_eq!(handle.options()[2].size, 8);
            assert_eq!(handle.options_generation(), 1);
            assert_eq!(handle.get(2).unwrap(), Value::Int(vec![7, 7]));
            assert_eq!(handle.options_generation(), 1, "nothing changed");
            // A set sized from the copy is refused once the copy is current.
            handle.set(1, &string("off")).unwrap();
            assert_eq!(handle.options_generation(), 1, "a quiet set");
            assert!(matches!(
                handle.set(2, &Value::Int(vec![1, 2])),
                Err(Error::Inactive { option: 2 })
            ));
            // Refused for what the re-read before it found, and counted.
            assert_eq!(handle.options_generation(), 2);
            assert!(handle.options()[2].capabilities.inactive());
        }
    );
    let log = fake.log();
    log.assert_no_violations();
    let gets = log.controls_of(2);
    assert_eq!(gets.len(), 3, "{gets:?}");
    assert!(gets[1].ends_with("action=GET"));
}

#[test]
#[cfg_attr(not(unix), ignore = "loads the fake by SANE's name: none on Windows")]
fn descriptors_that_cant_be_read_again_leave_none() {
    let script = format!(
        "{DEVICE}{}",
        r#"
        option s STRING 8 soft-select+soft-detect
        when gone null t
        option t INT 4 soft-select+soft-detect
        "#
    );
    let fake = Fake::new("descriptors_that_cant_be_read_again_leave_none", &script);
    in_child!(
        descriptors_that_cant_be_read_again_leave_none,
        fake.child(),
        || {
            let sane = common::load_fake();
            let mut handle = sane.open("fake:0").unwrap();
            // The set was made, and said so, though the re-read failed.
            match handle.set(1, &string("gone")) {
                Err(Error::ReloadAfterSet { info, reload }) => {
                    assert!(info.reload_options);
                    assert!(matches!(*reload, Error::BadDescriptor { option: 2, .. }));
                }
                other => panic!("expected ReloadAfterSet, got {other:?}"),
            }
            assert_eq!(handle.options_generation(), 1);
            assert!(handle.options().is_empty());
            assert!(matches!(
                handle.get(1),
                Err(Error::NoSuchOption { option: 1 })
            ));
            assert!(matches!(
                handle.reload_options(),
                Err(Error::BadDescriptor { option: 2, .. })
            ));
        }
    );
    let log = fake.log();
    log.assert_no_violations();
    assert_eq!(log.controls_of(1).len(), 1);
}

#[test]
#[cfg_attr(not(unix), ignore = "loads the fake by SANE's name: none on Windows")]
fn a_count_the_backend_wont_give_is_its_status() {
    let fake = Fake::new(
        "a_count_the_backend_wont_give_is_its_status",
        &format!("{DEVICE}control-status 0 IO_ERROR\n"),
    );
    in_child!(
        a_count_the_backend_wont_give_is_its_status,
        fake.child(),
        || {
            let sane = common::load_fake();
            match sane.open("fake:0") {
                Err(Error::Status {
                    status: Status::IO_ERROR,
                    call: Call::ControlOption,
                }) => {}
                other => panic!("expected IO_ERROR from sane_control_option, got {other:?}"),
            }
        }
    );
    let log = fake.log();
    log.assert_no_violations();
    assert_eq!(
        log.calls(),
        [
            "sane_init",
            "sane_open",
            "sane_get_option_descriptor",
            "sane_control_option",
            "sane_close",
            "sane_exit"
        ]
    );
}

#[test]
#[cfg_attr(not(unix), ignore = "loads the fake by SANE's name: none on Windows")]
fn a_value_the_backend_refuses_is_its_status() {
    let fake = Fake::new(
        "a_value_the_backend_refuses_is_its_status",
        &format!("{DEVICE}option s STRING 8 soft-select+soft-detect\nstrings a b\nvalue a\n"),
    );
    in_child!(
        a_value_the_backend_refuses_is_its_status,
        fake.child(),
        || {
            let sane = common::load_fake();
            let mut handle = sane.open("fake:0").unwrap();
            match handle.set(1, &string("c")) {
                Err(Error::Status {
                    status: Status::INVAL,
                    call: Call::ControlOption,
                }) => {}
                other => panic!("expected INVAL from sane_control_option, got {other:?}"),
            }
            assert_eq!(handle.get(1).unwrap(), string("a"));
        }
    );
    fake.log().assert_no_violations();
}

#[test]
#[cfg_attr(not(unix), ignore = "loads the fake by SANE's name: none on Windows")]
fn option_calls_say_a_late_listing_holds_sane() {
    let fake = Fake::new(
        "option_calls_say_a_late_listing_holds_sane",
        &format!("{DEVICE}listing-delay 1500 0\noption a INT 4 soft-select+soft-detect\n"),
    );
    in_child!(
        option_calls_say_a_late_listing_holds_sane,
        fake.child(),
        || {
            let sane = common::load_fake();
            let mut handle = sane.open("fake:0").unwrap();
            assert!(matches!(
                sane.devices(false, Duration::from_millis(100)),
                Err(Error::DeadlineExceeded)
            ));
            assert!(matches!(handle.get(1), Err(Error::ListingPending)));
            assert!(matches!(
                handle.set(1, &Value::Int(vec![1])),
                Err(Error::ListingPending)
            ));
            assert!(matches!(
                handle.reload_options(),
                Err(Error::ListingPending)
            ));
            // Once the listing returns, the calls go through.
            let until = std::time::Instant::now() + Duration::from_secs(20);
            loop {
                match handle.get(1) {
                    Err(Error::ListingPending) if std::time::Instant::now() < until => {
                        std::thread::sleep(Duration::from_millis(20))
                    }
                    other => break assert_eq!(other.unwrap(), Value::Int(vec![0])),
                }
            }
            drop(sane);
        }
    );
    fake.log().assert_no_violations();
}
