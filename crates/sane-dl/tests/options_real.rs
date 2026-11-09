//! Options, typed, through SANE's own backends: the `test` backend with its
//! test options enabled (every value type and constraint, an inexact set,
//! the capabilities, the button, the groups, the gamma tables), and `pnm`'s
//! file name and automatic resolution. Each test runs in a child process
//! whose configuration is a private, isolated directory, never the
//! system's; where there's no libsane each says it skipped, and with
//! `SANE_DL_REQUIRE_LIBSANE=1` fails instead.

#[macro_use]
mod common;

use sane_dl::{
    Constraint, Error, Fixed, Handle, OptionDescriptor, SaneString, Unit, Value, ValueType,
};

/// The `test` backend's configuration, with its test options on.
const TEST_CONF: &[(&str, &str)] = &[("test.conf", "enable-test-options true\n")];

/// An option's number, by its name.
fn index(handle: &Handle, name: &str) -> usize {
    handle
        .find_option(name)
        .unwrap_or_else(|| panic!("no option {name}"))
        .index
}

fn descriptor<'a>(handle: &'a Handle, name: &str) -> &'a OptionDescriptor {
    &handle.options()[index(handle, name)]
}

/// Whether a fixed-point word is `value`, to within a step.
fn near(word: i32, value: f64) -> bool {
    (Fixed(word).to_f64() - value).abs() <= 1.0 / 65536.0
}

#[test]
fn the_test_backends_descriptors_are_typed() {
    let child = common::real_child_with(
        "the_test_backends_descriptors_are_typed",
        &["test"],
        TEST_CONF,
    );
    in_child!(the_test_backends_descriptors_are_typed, child, || {
        let sane = common::load_real();
        let mut handle = sane.open("test:0").unwrap();
        let options = handle.options().to_vec();
        // Option 0 counts them all, itself included.
        assert_eq!(
            handle.get(0).unwrap(),
            Value::Int(vec![options.len() as i32])
        );
        assert_eq!(options[0].title, "Number of options");
        for (number, option) in options.iter().enumerate() {
            assert_eq!(option.index, number);
            assert_eq!(option.unusable, None, "{option:?}");
        }
        let groups: Vec<&str> = options
            .iter()
            .filter(|option| option.is_group())
            .map(|option| option.title.as_str())
            .collect();
        assert_eq!(
            groups,
            [
                "Scan Mode",
                "Special Options",
                "Geometry",
                "Bool test options",
                "Int test options",
                "Fixed test options",
                "String test options",
                "Button test options"
            ]
        );
        for value_type in [
            ValueType::Bool,
            ValueType::Int,
            ValueType::Fixed,
            ValueType::String,
            ValueType::Button,
            ValueType::Group,
        ] {
            assert!(
                options.iter().any(|option| option.value_type == value_type),
                "no {value_type} option"
            );
        }
        for unit in [
            Unit::None,
            Unit::Pixel,
            Unit::Bit,
            Unit::Mm,
            Unit::Dpi,
            Unit::Percent,
            Unit::Microsecond,
        ] {
            assert!(
                options.iter().any(|option| option.unit == unit),
                "no option in {unit:?}"
            );
        }
        let range = descriptor(&handle, "int-constraint-range");
        assert_eq!(
            (range.value_type, range.unit, range.size, &range.constraint),
            (
                ValueType::Int,
                Unit::Pixel,
                4,
                &Constraint::Range {
                    min: 4,
                    max: 192,
                    quant: 2
                }
            )
        );
        let words = descriptor(&handle, "int-constraint-word-list");
        assert_eq!(
            (words.unit, &words.constraint),
            (
                Unit::Bit,
                &Constraint::WordList(vec![
                    -42,
                    -8,
                    0,
                    17,
                    42,
                    256,
                    65536,
                    16_777_216,
                    1_073_741_824
                ])
            )
        );
        let fixed = descriptor(&handle, "fixed-constraint-range");
        assert_eq!(
            (fixed.value_type, fixed.unit),
            (ValueType::Fixed, Unit::Microsecond)
        );
        let Constraint::Range { min, max, quant } = fixed.constraint else {
            panic!("{fixed:?}")
        };
        assert!(near(min, -42.17) && near(max, 32767.9999) && quant == 2 << 16);
        let Constraint::WordList(list) =
            &descriptor(&handle, "fixed-constraint-word-list").constraint
        else {
            panic!()
        };
        let expected = [-32.7, 12.1, 42.0, 129.5];
        assert_eq!(list.len(), expected.len());
        assert!(
            list.iter()
                .zip(expected)
                .all(|(&word, value)| near(word, value))
        );
        let strings = descriptor(&handle, "string-constraint-string-list");
        let Constraint::StringList(entries) = &strings.constraint else {
            panic!("{strings:?}")
        };
        assert_eq!(
            entries[..2],
            [SaneString::from("First entry"), "Second entry".into()]
        );
        assert!(
            entries[2]
                .to_string()
                .starts_with("This is the very long third entry.")
        );
        let long = descriptor(&handle, "string-constraint-long-string-list");
        let Constraint::StringList(entries) = &long.constraint else {
            panic!("{long:?}")
        };
        assert_eq!(entries.len(), 46);
        assert_eq!(descriptor(&handle, "string").constraint, Constraint::None);
        // The gamma tables are arrays of words, each in a range.
        for name in ["red-gamma-table", "green-gamma-table", "blue-gamma-table"] {
            let table = descriptor(&handle, name);
            assert_eq!(table.value_type, ValueType::Int);
            assert_eq!(table.word_count(), 256, "{name}");
            assert_eq!(
                table.constraint,
                Constraint::Range {
                    min: 0,
                    max: 255,
                    quant: 1
                }
            );
        }
        assert_eq!(descriptor(&handle, "int-constraint-array").word_count(), 6);
        let button = descriptor(&handle, "button");
        assert_eq!(
            (button.value_type, button.word_count()),
            (ValueType::Button, 0)
        );
    });
}

#[test]
fn the_test_backends_values_are_got_and_set() {
    let child = common::real_child_with(
        "the_test_backends_values_are_got_and_set",
        &["test"],
        TEST_CONF,
    );
    in_child!(the_test_backends_values_are_got_and_set, child, || {
        let sane = common::load_real();
        let mut handle = sane.open("test:0").unwrap();
        let (int, fixed, string, list) = (
            index(&handle, "int"),
            index(&handle, "fixed"),
            index(&handle, "string"),
            index(&handle, "string-constraint-string-list"),
        );
        assert_eq!(handle.get(int).unwrap(), Value::Int(vec![42]));
        assert_eq!(
            handle.get(fixed).unwrap(),
            Value::Fixed(vec![Fixed(42 << 16)])
        );
        let Value::String(text) = handle.get(string).unwrap() else {
            panic!()
        };
        assert!(
            text.to_string()
                .starts_with("This is the contents of the string option.")
        );
        assert_eq!(
            handle
                .get(index(&handle, "bool-soft-select-soft-detect"))
                .unwrap(),
            Value::Bool(false)
        );
        // Each set read back.
        let sets = [
            (int, Value::Int(vec![-7])),
            (fixed, Value::Fixed(vec![Fixed::from_f64(-12.5).unwrap()])),
            (string, Value::String("written".into())),
            (list, Value::String("Second entry".into())),
            (
                index(&handle, "bool-soft-select-soft-detect"),
                Value::Bool(true),
            ),
            (
                index(&handle, "int-constraint-array"),
                Value::Int(vec![1, 2, 3, 4, 5, 6]),
            ),
            (
                index(&handle, "red-gamma-table"),
                Value::Int((0..256).rev().collect()),
            ),
        ];
        for (number, value) in sets {
            let info = handle.set(number, &value).unwrap();
            assert!(!info.inexact, "option {number}");
            assert_eq!(handle.get(number).unwrap(), value, "option {number}");
        }
        // The resolution, which the descriptor types as fixed-point.
        let resolution = index(&handle, "resolution");
        assert!(matches!(handle.get(resolution).unwrap(), Value::Fixed(v) if v.len() == 1));
        handle
            .set(
                resolution,
                &Value::Fixed(vec![Fixed::from_f64(300.0).unwrap()]),
            )
            .unwrap();
        assert_eq!(
            handle.get(resolution).unwrap(),
            Value::Fixed(vec![Fixed(300 << 16)])
        );
        // A value the backend changes says so.
        let inexact = index(&handle, "int-inexact");
        let info = handle.set(inexact, &Value::Int(vec![9])).unwrap();
        assert!(info.inexact, "{info:?}");
        assert_ne!(handle.get(inexact).unwrap(), Value::Int(vec![9]));
        // The button is pressed, and has no value to read.
        let button = index(&handle, "button");
        handle.set(button, &Value::Button).unwrap();
        assert!(matches!(handle.get(button), Err(Error::NoValue { .. })));
        assert!(
            matches!(handle.get(1), Err(Error::NoValue { .. })),
            "a group"
        );
    });
}

#[test]
fn the_test_backends_capabilities_are_kept_to() {
    let child = common::real_child_with(
        "the_test_backends_capabilities_are_kept_to",
        &["test"],
        TEST_CONF,
    );
    in_child!(the_test_backends_capabilities_are_kept_to, child, || {
        let sane = common::load_real();
        let mut handle = sane.open("test:0").unwrap();
        let on = Value::Bool(true);
        // Set on the device, read by software.
        let hard_soft = index(&handle, "bool-hard-select-soft-detect");
        assert!(handle.options()[hard_soft].capabilities.hard_select());
        assert!(matches!(handle.get(hard_soft).unwrap(), Value::Bool(_)));
        assert!(matches!(
            handle.set(hard_soft, &on),
            Err(Error::NotSettable { .. })
        ));
        // Set on the device, and not readable by software.
        let hard = index(&handle, "bool-hard-select");
        assert!(!handle.options()[hard].capabilities.soft_detect());
        assert!(matches!(handle.get(hard), Err(Error::NotReadable { .. })));
        assert!(matches!(
            handle.set(hard, &on),
            Err(Error::NotSettable { .. })
        ));
        // Read only.
        let read_only = index(&handle, "bool-soft-detect");
        assert!(matches!(handle.get(read_only).unwrap(), Value::Bool(_)));
        assert!(matches!(
            handle.set(read_only, &on),
            Err(Error::NotSettable { .. })
        ));
        let emulated = index(&handle, "bool-soft-select-soft-detect-emulated");
        assert!(handle.options()[emulated].capabilities.emulated());
        handle.set(emulated, &on).unwrap();
        // Automatic, and not.
        let auto = index(&handle, "bool-soft-select-soft-detect-auto");
        assert!(handle.options()[auto].capabilities.automatic());
        handle.set_auto(auto).unwrap();
        assert!(matches!(handle.get(auto).unwrap(), Value::Bool(_)));
        assert!(matches!(
            handle.set_auto(index(&handle, "int")),
            Err(Error::NotAutomatic { .. })
        ));
        // Inactive in gray, so neither read nor set.
        let three_pass = index(&handle, "three-pass");
        assert!(handle.options()[three_pass].capabilities.inactive());
        assert!(matches!(
            handle.get(three_pass),
            Err(Error::Inactive { .. })
        ));
        assert!(matches!(
            handle.set(three_pass, &on),
            Err(Error::Inactive { .. })
        ));
    });
}

#[test]
fn a_set_that_changes_other_options_reads_them_again() {
    let child = common::real_child_with(
        "a_set_that_changes_other_options_reads_them_again",
        &["test"],
        TEST_CONF,
    );
    in_child!(
        a_set_that_changes_other_options_reads_them_again,
        child,
        || {
            let sane = common::load_real();
            let mut handle = sane.open("test:0").unwrap();
            let mode = index(&handle, "mode");
            let three_pass = index(&handle, "three-pass");
            assert!(handle.options()[three_pass].capabilities.inactive());
            // Color makes three-pass meaningful.
            let info = handle.set(mode, &Value::String("Color".into())).unwrap();
            assert!(info.reload_options, "{info:?}");
            assert!(handle.options()[three_pass].capabilities.is_active());
            assert_eq!(handle.get(three_pass).unwrap(), Value::Bool(false));
            handle.set(three_pass, &Value::Bool(true)).unwrap();
            assert!(
                handle.options()[index(&handle, "three-pass-order")]
                    .capabilities
                    .is_active()
            );
            let info = handle.set(mode, &Value::String("Gray".into())).unwrap();
            assert!(info.reload_options);
            assert!(handle.options()[three_pass].capabilities.inactive());
            // And read again on asking.
            let before = handle.options().to_vec();
            handle.reload_options().unwrap();
            assert_eq!(handle.options(), &before[..]);
        }
    );
}

#[test]
fn values_that_dont_fit_are_refused_before_the_backend() {
    let child = common::real_child_with(
        "values_that_dont_fit_are_refused_before_the_backend",
        &["test"],
        TEST_CONF,
    );
    in_child!(
        values_that_dont_fit_are_refused_before_the_backend,
        child,
        || {
            let sane = common::load_real();
            let mut handle = sane.open("test:0").unwrap();
            let (int, array, mode) = (
                index(&handle, "int"),
                index(&handle, "int-constraint-array"),
                index(&handle, "mode"),
            );
            assert_eq!(handle.options()[mode].size, 6);
            assert!(matches!(
                handle.set(int, &Value::Fixed(vec![Fixed(1)])),
                Err(Error::WrongType {
                    value_type: ValueType::Int,
                    given: ValueType::Fixed,
                    ..
                })
            ));
            assert!(matches!(
                handle.set(array, &Value::Int(vec![1])),
                Err(Error::WrongLength {
                    expected: 6,
                    given: 1,
                    ..
                })
            ));
            assert!(matches!(
                handle.set(mode, &Value::String("Colour".into())),
                Err(Error::StringTooLong {
                    size: 6,
                    given: 6,
                    ..
                })
            ));
            assert!(matches!(
                handle.set(mode, &Value::String("Gr\0y".into())),
                Err(Error::NulInString { .. })
            ));
            let past = handle.options().len();
            assert!(matches!(
                handle.get(past),
                Err(Error::NoSuchOption { option }) if option == past
            ));
            // A string the list doesn't have is the backend's to refuse.
            assert!(matches!(
                handle.set(mode, &Value::String("Blue".into())),
                Err(Error::Status { .. })
            ));
            assert_eq!(handle.get(mode).unwrap(), Value::String("Gray".into()));
        }
    );
}

#[test]
fn pnm_takes_a_file_name_and_chooses_its_resolution() {
    let child = common::real_child("pnm_takes_a_file_name_and_chooses_its_resolution", &["pnm"]);
    in_child!(
        pnm_takes_a_file_name_and_chooses_its_resolution,
        child,
        || {
            let sane = common::load_real();
            let mut handle = sane.open("pnm:0").unwrap();
            let filename = index(&handle, "filename");
            assert_eq!(handle.options()[filename].value_type, ValueType::String);
            let path = common::test_dir("pnm_takes_a_file_name_and_chooses_its_resolution")
                .join("page.pgm");
            let path = SaneString::from(path.to_str().expect("the test's path is UTF-8"));
            handle.set(filename, &Value::String(path.clone())).unwrap();
            assert_eq!(handle.get(filename).unwrap(), Value::String(path));
            let resolution = index(&handle, "resolution");
            let descriptor = &handle.options()[resolution];
            assert_eq!(
                (descriptor.value_type, descriptor.unit),
                (ValueType::Int, Unit::Dpi)
            );
            assert!(
                matches!(&descriptor.constraint, Constraint::WordList(list) if list.contains(&75))
            );
            assert!(descriptor.capabilities.automatic());
            handle.set_auto(resolution).unwrap();
            assert!(matches!(handle.get(resolution).unwrap(), Value::Int(v) if v.len() == 1));
            handle.set(resolution, &Value::Int(vec![150])).unwrap();
            assert_eq!(handle.get(resolution).unwrap(), Value::Int(vec![150]));
            // The other options can't choose their own value.
            let brightness = index(&handle, "brightness");
            assert!(!handle.options()[brightness].capabilities.automatic());
            assert!(matches!(
                handle.set_auto(brightness),
                Err(Error::NotAutomatic { .. })
            ));
            let read_only = index(&handle, "read-only");
            assert!(matches!(handle.get(read_only).unwrap(), Value::Int(_)));
            assert!(matches!(
                handle.set(read_only, &Value::Int(vec![1])),
                Err(Error::NotSettable { .. })
            ));
        }
    );
}
