//! The options the fake plays: their descriptors as the script gives them,
//! the rules that change descriptors after a set, the built-in profiles of
//! particular backends' layouts, and what `sane_control_option` does with
//! them.
//!
//! Safe code only: the exports build the C descriptors from these and copy
//! values in and out.

use crate::ffi;
use crate::script::{Text, word};

/// A value as the fake keeps it: words, or a string's bytes (no `NUL`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RawValue {
    Words(Vec<i32>),
    Bytes(Vec<u8>),
}

/// An option's constraint, as the script gives it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConstraintSpec {
    None,
    Range(i32, i32, i32),
    /// The words, and what the list's first word claims (default: their
    /// count).
    Words(Vec<i32>, Option<i32>),
    Strings(Vec<Text>),
}

/// A descriptor as the fake gives it out: every field as the script set it,
/// any integer allowed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Descriptor {
    pub name: Text,
    pub title: Text,
    pub desc: Text,
    pub value_type: i32,
    pub unit: i32,
    pub size: i32,
    pub cap: i32,
    pub constraint_type: i32,
    pub constraint: ConstraintSpec,
}

/// What a rule changes, once its option is set to its value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Change {
    Activate(usize),
    Deactivate(usize),
    Range(usize, i32, i32, i32),
    Words(usize, Vec<i32>),
    Size(usize, i32),
    /// Its descriptor becomes null.
    Null(usize),
    /// The changes aren't said: no `RELOAD_OPTIONS`.
    Quiet,
}

/// A change after a set to a value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rule {
    pub when: RawValue,
    pub change: Change,
}

/// An option of the script: its descriptor, value and behaviour.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OptionSpec {
    pub descriptor: Descriptor,
    pub value: RawValue,
    /// The value `SET_AUTO` chooses; without one it keeps the value.
    pub auto: Option<RawValue>,
    pub rules: Vec<Rule>,
    /// A set says `RELOAD_PARAMS`.
    pub reload_params: bool,
    /// `sane_get_option_descriptor` gives null for it, though it's counted.
    pub null: bool,
}

/// The options a script gives every handle, after option 0, which the fake
/// makes; and option 0's value, if the script overrides the count.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Layout {
    pub options: Vec<OptionSpec>,
    pub count: Option<i32>,
    /// The statuses `sane_control_option` answers for options by number,
    /// in place of what it would do.
    pub statuses: Vec<(i32, i32)>,
}

/// A rule before its target is known by number, and its numbers parsed.
#[derive(Clone, Debug, PartialEq, Eq)]
struct PendingRule {
    /// Where the rule's line is, for its errors.
    place: String,
    option: usize,
    when: Text,
    action: String,
    target: Option<String>,
    args: Vec<String>,
}

/// Builds a layout from a script's option lines.
#[derive(Debug, Default)]
pub struct Builder {
    layout: Layout,
    rules: Vec<PendingRule>,
    /// Options to start inactive, by name, and where each was named.
    inactive: Vec<(String, String)>,
    /// Where the line being taken is, as its errors say it.
    pub place: String,
}

/// Keywords for each capability bit.
const CAPS: [(&str, i32); 7] = [
    ("soft-select", ffi::SANE_CAP_SOFT_SELECT),
    ("hard-select", ffi::SANE_CAP_HARD_SELECT),
    ("soft-detect", ffi::SANE_CAP_SOFT_DETECT),
    ("emulated", ffi::SANE_CAP_EMULATED),
    ("automatic", ffi::SANE_CAP_AUTOMATIC),
    ("inactive", ffi::SANE_CAP_INACTIVE),
    ("advanced", ffi::SANE_CAP_ADVANCED),
];

fn named(word: &str, names: &[(&str, i32)], what: &str) -> Result<i32, String> {
    if let Some((_, code)) = names.iter().find(|(name, _)| *name == word) {
        return Ok(*code);
    }
    word.parse()
        .map_err(|_| format!("{word:?} isn't a {what} or a number"))
}

fn value_type(word: &str) -> Result<i32, String> {
    named(
        word,
        &[
            ("BOOL", ffi::SANE_TYPE_BOOL),
            ("INT", ffi::SANE_TYPE_INT),
            ("FIXED", ffi::SANE_TYPE_FIXED),
            ("STRING", ffi::SANE_TYPE_STRING),
            ("BUTTON", ffi::SANE_TYPE_BUTTON),
            ("GROUP", ffi::SANE_TYPE_GROUP),
        ],
        "value type",
    )
}

fn unit(word: &str) -> Result<i32, String> {
    named(
        word,
        &[
            ("NONE", ffi::SANE_UNIT_NONE),
            ("PIXEL", ffi::SANE_UNIT_PIXEL),
            ("BIT", ffi::SANE_UNIT_BIT),
            ("MM", ffi::SANE_UNIT_MM),
            ("DPI", ffi::SANE_UNIT_DPI),
            ("PERCENT", ffi::SANE_UNIT_PERCENT),
            ("MICROSECOND", ffi::SANE_UNIT_MICROSECOND),
        ],
        "unit",
    )
}

fn constraint_type(word: &str) -> Result<i32, String> {
    named(
        word,
        &[
            ("NONE", ffi::SANE_CONSTRAINT_NONE),
            ("RANGE", ffi::SANE_CONSTRAINT_RANGE),
            ("WORD_LIST", ffi::SANE_CONSTRAINT_WORD_LIST),
            ("STRING_LIST", ffi::SANE_CONSTRAINT_STRING_LIST),
        ],
        "constraint type",
    )
}

fn caps(word: &str) -> Result<i32, String> {
    if word == "-" {
        return Ok(0);
    }
    if let Ok(number) = word.parse() {
        return Ok(number);
    }
    word.split('+')
        .map(|name| named(name, &CAPS, "capability"))
        .try_fold(0, |bits, bit| bit.map(|bit| bits | bit))
}

/// A number for an option of `value_type`: a fixed-point option's in
/// decimal, rounded to the nearest 1/65536; any other's an integer.
fn number(value_type: i32, word: &str) -> Result<i32, String> {
    if value_type == ffi::SANE_TYPE_FIXED {
        let value: f64 = word
            .parse()
            .map_err(|_| format!("{word:?} isn't a number"))?;
        let steps = (value * 65536.0).round();
        if steps >= f64::from(i32::MIN) && steps <= f64::from(i32::MAX) {
            Ok(steps as i32)
        } else {
            Err(format!("{word:?} is out of a fixed-point word's range"))
        }
    } else {
        word.parse()
            .map_err(|_| format!("{word:?} isn't an integer"))
    }
}

/// A value for an option: a string's text, or numbers, as many as given,
/// for the rest.
fn value(descriptor: &Descriptor, args: &[Text]) -> Result<RawValue, String> {
    if descriptor.value_type == ffi::SANE_TYPE_STRING {
        let [text] = args else {
            return Err("a string value takes one word".to_owned());
        };
        let Text::Bytes(bytes) = text else {
            return Err("a string value can't be @null".to_owned());
        };
        return Ok(RawValue::Bytes(bytes.clone()));
    }
    let words = args
        .iter()
        .map(|arg| number(descriptor.value_type, &word(arg)?))
        .collect::<Result<Vec<i32>, String>>()?;
    Ok(RawValue::Words(words))
}

impl Builder {
    fn last(&mut self, keyword: &str) -> Result<&mut OptionSpec, String> {
        self.layout
            .options
            .last_mut()
            .ok_or_else(|| format!("{keyword} needs an option line before it"))
    }

    /// Takes a line if its keyword is an option keyword: `Ok(false)` if it
    /// isn't one.
    pub fn line(&mut self, keyword: &str, args: &[Text]) -> Result<bool, String> {
        let words = || {
            args.iter()
                .map(word)
                .collect::<Result<Vec<String>, String>>()
        };
        let want = |n: usize| {
            if args.len() == n {
                Ok(())
            } else {
                Err(format!("{keyword} takes {n} values, not {}", args.len()))
            }
        };
        match keyword {
            "option" => {
                want(4)?;
                let words = args[1..].iter().map(word).collect::<Result<Vec<_>, _>>()?;
                let value_type = value_type(&words[0])?;
                let size: i32 = words[1]
                    .parse()
                    .map_err(|_| format!("{:?} isn't a size", words[1]))?;
                let descriptor = Descriptor {
                    name: args[0].clone(),
                    title: args[0].clone(),
                    desc: Text::Bytes(Vec::new()),
                    value_type,
                    unit: ffi::SANE_UNIT_NONE,
                    size,
                    cap: caps(&words[2])?,
                    constraint_type: ffi::SANE_CONSTRAINT_NONE,
                    constraint: ConstraintSpec::None,
                };
                let value = if value_type == ffi::SANE_TYPE_STRING {
                    RawValue::Bytes(Vec::new())
                } else {
                    RawValue::Words(Vec::new())
                };
                self.layout.options.push(OptionSpec {
                    descriptor,
                    value,
                    auto: None,
                    rules: Vec::new(),
                    reload_params: false,
                    null: false,
                });
            }
            "option-null" => {
                want(0)?;
                self.layout.options.push(OptionSpec {
                    descriptor: Descriptor {
                        name: Text::Null,
                        title: Text::Null,
                        desc: Text::Null,
                        value_type: ffi::SANE_TYPE_INT,
                        unit: 0,
                        size: 4,
                        cap: 0,
                        constraint_type: 0,
                        constraint: ConstraintSpec::None,
                    },
                    value: RawValue::Words(Vec::new()),
                    auto: None,
                    rules: Vec::new(),
                    reload_params: false,
                    null: true,
                });
            }
            "option-count" => {
                want(1)?;
                let count = word(&args[0])?;
                self.layout.count = Some(
                    count
                        .parse()
                        .map_err(|_| format!("{count:?} isn't a count"))?,
                );
            }
            "control-status" => {
                want(2)?;
                let words = words()?;
                let option: i32 = words[0]
                    .parse()
                    .map_err(|_| format!("{:?} isn't an option's number", words[0]))?;
                let status = crate::script::status(&words[1])?;
                self.layout.statuses.push((option, status));
            }
            "title" | "desc" => {
                want(1)?;
                let option = self.last(keyword)?;
                let text = args[0].clone();
                if keyword == "title" {
                    option.descriptor.title = text;
                } else {
                    option.descriptor.desc = text;
                }
            }
            "unit" => {
                want(1)?;
                let code = unit(&word(&args[0])?)?;
                self.last(keyword)?.descriptor.unit = code;
            }
            "constraint" => {
                want(1)?;
                let code = constraint_type(&word(&args[0])?)?;
                self.last(keyword)?.descriptor.constraint_type = code;
            }
            "range" => {
                want(3)?;
                let words = words()?;
                let option = self.last(keyword)?;
                let value_type = option.descriptor.value_type;
                let n = |i: usize| number(value_type, &words[i]);
                option.descriptor.constraint = ConstraintSpec::Range(n(0)?, n(1)?, n(2)?);
                option.descriptor.constraint_type = ffi::SANE_CONSTRAINT_RANGE;
            }
            "words" => {
                let words = words()?;
                let option = self.last(keyword)?;
                let value_type = option.descriptor.value_type;
                let list = words
                    .iter()
                    .map(|w| number(value_type, w))
                    .collect::<Result<Vec<_>, _>>()?;
                option.descriptor.constraint = ConstraintSpec::Words(list, None);
                option.descriptor.constraint_type = ffi::SANE_CONSTRAINT_WORD_LIST;
            }
            "word-list-length" => {
                want(1)?;
                let claimed = word(&args[0])?;
                let claimed: i32 = claimed
                    .parse()
                    .map_err(|_| format!("{claimed:?} isn't a length"))?;
                let option = self.last(keyword)?;
                match &mut option.descriptor.constraint {
                    ConstraintSpec::Words(_, length) => *length = Some(claimed),
                    _ => return Err("word-list-length needs a words line before it".to_owned()),
                }
            }
            "strings" => {
                let option = self.last(keyword)?;
                option.descriptor.constraint = ConstraintSpec::Strings(args.to_vec());
                option.descriptor.constraint_type = ffi::SANE_CONSTRAINT_STRING_LIST;
            }
            "strings-repeat" => {
                want(1)?;
                let count = word(&args[0])?;
                let count: usize = count
                    .parse()
                    .map_err(|_| format!("{count:?} isn't a count"))?;
                let option = self.last(keyword)?;
                let ConstraintSpec::Strings(list) = &mut option.descriptor.constraint else {
                    return Err("strings-repeat needs a strings line before it".to_owned());
                };
                for _ in 0..count {
                    let entry = format!("s{}", list.len());
                    list.push(Text::Bytes(entry.into_bytes()));
                }
            }
            "value" | "auto" => {
                let option = self.last(keyword)?;
                let parsed = value(&option.descriptor, args)?;
                if keyword == "value" {
                    option.value = parsed;
                } else {
                    option.auto = Some(parsed);
                }
            }
            "reload-params" => {
                want(0)?;
                self.last(keyword)?.reload_params = true;
            }
            "when" => {
                if args.len() < 2 {
                    return Err("when takes a value, a change, and what it changes".to_owned());
                }
                let option = self
                    .layout
                    .options
                    .len()
                    .checked_sub(1)
                    .ok_or_else(|| "when needs an option line before it".to_owned())?;
                let rest = args[1..].iter().map(word).collect::<Result<Vec<_>, _>>()?;
                let (action, rest) = rest.split_first().expect("checked above");
                let (target, numbers) = match rest.split_first() {
                    Some((target, numbers)) => (Some(target.clone()), numbers.to_vec()),
                    None => (None, Vec::new()),
                };
                self.rules.push(PendingRule {
                    place: self.place.clone(),
                    option,
                    when: args[0].clone(),
                    action: action.clone(),
                    target,
                    args: numbers,
                });
            }
            "start-inactive" => {
                for name in words()? {
                    self.inactive.push((self.place.clone(), name));
                }
            }
            "profile" => {
                want(1)?;
                let name = word(&args[0])?;
                let text = profile(&name).ok_or_else(|| format!("{name:?} isn't a profile"))?;
                let outer = std::mem::take(&mut self.place);
                let taken = self.text(text, &format!("{outer}: profile {name}"));
                self.place = outer;
                taken.map_err(|error| format!("profile {name}: {error}"))?;
            }
            _ => return Ok(false),
        }
        Ok(true)
    }

    /// Takes every line of `text`, which holds option lines alone; `place`
    /// says where it is.
    fn text(&mut self, text: &str, place: &str) -> Result<(), String> {
        for (number, line) in text.lines().enumerate() {
            self.place = format!("{place}: line {}", number + 1);
            let at = |error: String| format!("line {}: {error}", number + 1);
            let words = crate::script::split(line).map_err(at)?;
            let Some((keyword, args)) = words.split_first() else {
                continue;
            };
            let keyword = word(keyword).map_err(at)?;
            if !self.line(&keyword, args).map_err(at)? {
                return Err(at(format!("{keyword:?} isn't an option keyword")));
            }
        }
        Ok(())
    }

    /// The layout, its rules' targets found by name and their numbers
    /// parsed by the targets' types.
    pub fn finish(mut self) -> Result<Layout, String> {
        for (place, name) in std::mem::take(&mut self.inactive) {
            let wanted = Text::Bytes(name.clone().into_bytes());
            let option = self
                .layout
                .options
                .iter_mut()
                .find(|option| option.descriptor.name == wanted)
                .ok_or_else(|| format!("{place}: start-inactive: no option {name:?}"))?;
            option.descriptor.cap |= ffi::SANE_CAP_INACTIVE;
        }
        for rule in std::mem::take(&mut self.rules) {
            let place = rule.place.clone();
            self.rule(rule)
                .map_err(|error| format!("{place}: {error}"))?;
        }
        Ok(self.layout)
    }

    /// Adds a rule to its option, its target found and numbers parsed.
    fn rule(&mut self, rule: PendingRule) -> Result<(), String> {
        {
            let options = &self.layout.options;
            let owner = &options[rule.option].descriptor;
            let when = value(owner, std::slice::from_ref(&rule.when))?;
            let target = |name: &Option<String>| -> Result<usize, String> {
                let name = name
                    .as_ref()
                    .ok_or_else(|| format!("when ... {} needs an option", rule.action))?;
                options
                    .iter()
                    .position(|option| {
                        option.descriptor.name == Text::Bytes(name.clone().into_bytes())
                    })
                    .ok_or_else(|| format!("when ... {}: no option {name:?}", rule.action))
            };
            let numbers = |index: usize| -> Result<Vec<i32>, String> {
                let value_type = options[index].descriptor.value_type;
                rule.args.iter().map(|w| number(value_type, w)).collect()
            };
            let mut changes = Vec::new();
            match rule.action.as_str() {
                "activate" | "deactivate" => {
                    let mut names = vec![rule.target.clone()];
                    names.extend(rule.args.iter().cloned().map(Some));
                    for name in &names {
                        let index = target(name)?;
                        changes.push(if rule.action == "activate" {
                            Change::Activate(index)
                        } else {
                            Change::Deactivate(index)
                        });
                    }
                }
                "range" => {
                    let index = target(&rule.target)?;
                    let [min, max, quant] = numbers(index)?[..] else {
                        return Err("when ... range takes an option and three numbers".to_owned());
                    };
                    changes.push(Change::Range(index, min, max, quant));
                }
                "words" => {
                    let index = target(&rule.target)?;
                    changes.push(Change::Words(index, numbers(index)?));
                }
                "size" => {
                    let index = target(&rule.target)?;
                    // A size is bytes, whatever the option's type.
                    let sizes = rule
                        .args
                        .iter()
                        .map(|w| number(ffi::SANE_TYPE_INT, w))
                        .collect::<Result<Vec<_>, _>>()?;
                    let [size] = sizes[..] else {
                        return Err("when ... size takes an option and a size".to_owned());
                    };
                    changes.push(Change::Size(index, size));
                }
                "null" => changes.push(Change::Null(target(&rule.target)?)),
                "quiet" => changes.push(Change::Quiet),
                other => return Err(format!("when ... {other:?} isn't a change")),
            }
            for change in changes {
                self.layout.options[rule.option].rules.push(Rule {
                    when: when.clone(),
                    change,
                });
            }
        }
        Ok(())
    }
}

/// A built-in profile's option lines: a backend's layout, as its options
/// are named and valued, with a feeder, its duplex and a page height active
/// only for the feeder.
pub fn profile(name: &str) -> Option<&'static str> {
    Some(match name {
        "fujitsu" => FUJITSU,
        "airscan" => AIRSCAN,
        "brother" => BROTHER,
        "canon-dr" => CANON_DR,
        "lexmark" => LEXMARK,
        "epson" => EPSON,
        "kodak" => KODAK,
        "geometry" => GEOMETRY,
        _ => return None,
    })
}

/// The names of the backends' profiles.
#[cfg(test)]
pub const PROFILES: [&str; 7] = [
    "fujitsu", "airscan", "brother", "canon-dr", "lexmark", "epson", "kodak",
];

/// The scan area and a feeder's page size, in millimetres: what every
/// profile has. The page size is active only while a feeder is the source.
const GEOMETRY: &str = r#"
option @null GROUP 0 -
title Geometry
option tl-x FIXED 4 soft-select+soft-detect
title "Top-left x"
unit MM
range 0 215.9 0
value 0
option tl-y FIXED 4 soft-select+soft-detect
title "Top-left y"
unit MM
range 0 355.6 0
value 0
option br-x FIXED 4 soft-select+soft-detect
title "Bottom-right x"
unit MM
range 0 215.9 0
value 215.9
option br-y FIXED 4 soft-select+soft-detect
title "Bottom-right y"
unit MM
range 0 355.6 0
value 279.4
option page-width FIXED 4 soft-select+soft-detect+advanced
title "Page width"
desc "The width of the paper in the feeder."
unit MM
range 0 221 0
value 215.9
option page-height FIXED 4 soft-select+soft-detect+advanced
title "Page height"
desc "The length of the paper in the feeder."
unit MM
range 0 355.6 0
value 279.4
"#;

/// A feeder that scans the front, the back or both, named in `source`, and
/// a flatbed; the page size active only for the feeder.
const FUJITSU: &str = r#"
option @null GROUP 0 -
title Standard
option source STRING 16 soft-select+soft-detect
title "Scan source"
strings Flatbed "ADF Front" "ADF Back" "ADF Duplex"
value "ADF Front"
reload-params
when Flatbed deactivate page-width page-height
when "ADF Front" activate page-width page-height
when "ADF Back" activate page-width page-height
when "ADF Duplex" activate page-width page-height
option mode STRING 16 soft-select+soft-detect
title "Scan mode"
strings Lineart Halftone Gray Color
value Lineart
reload-params
option resolution INT 4 soft-select+soft-detect
title "Scan resolution"
unit DPI
range 50 600 1
value 300
reload-params
profile geometry
"#;

/// A flatbed, a feeder and its duplex, named in `source`; fewer resolutions
/// in duplex, so a constraint changes with the source.
const AIRSCAN: &str = r#"
option @null GROUP 0 -
title Standard
option source STRING 16 soft-select+soft-detect
title "Scan source"
strings Flatbed ADF "ADF Duplex"
value Flatbed
reload-params
when Flatbed deactivate page-width page-height
when Flatbed words resolution 75 100 150 200 300 600
when ADF activate page-width page-height
when ADF words resolution 75 100 150 200 300 600
when "ADF Duplex" activate page-width page-height
when "ADF Duplex" words resolution 100 200 300
option mode STRING 8 soft-select+soft-detect
title "Scan mode"
strings Color Gray
value Color
reload-params
option resolution INT 4 soft-select+soft-detect
title "Scan resolution"
unit DPI
words 75 100 150 200 300 600
value 300
reload-params
profile geometry
start-inactive page-width page-height
"#;

/// A feeder's alignment and duplex in long `source` strings.
const BROTHER: &str = r#"
option @null GROUP 0 -
title "Scan mode"
option mode STRING 32 soft-select+soft-detect
title "Scan mode"
strings "Black & White" "Gray[Error Diffusion]" "True Gray" "24bit Color" "24bit Color[Fast]"
value "24bit Color"
reload-params
option resolution INT 4 soft-select+soft-detect
title "Resolution"
unit DPI
words 100 150 200 300 400 600 1200
value 200
reload-params
option source STRING 64 soft-select+soft-detect
title "Scan source"
strings FlatBed "Automatic Document Feeder(left aligned)" "Automatic Document Feeder(left aligned,Duplex)" "Automatic Document Feeder(centrally aligned)" "Automatic Document Feeder(centrally aligned,Duplex)"
value FlatBed
reload-params
when FlatBed deactivate page-width page-height
when "Automatic Document Feeder(left aligned)" activate page-width page-height
when "Automatic Document Feeder(left aligned,Duplex)" activate page-width page-height
when "Automatic Document Feeder(centrally aligned)" activate page-width page-height
when "Automatic Document Feeder(centrally aligned,Duplex)" activate page-width page-height
profile geometry
start-inactive page-width page-height
"#;

/// A feeder alone, its duplex a `ScanMode` option of its own (a name with
/// capitals, which the standard doesn't allow and a backend gives anyway).
const CANON_DR: &str = r#"
option @null GROUP 0 -
title Standard
option source STRING 8 soft-select+soft-detect
title "Scan source"
strings ADF
value ADF
option ScanMode STRING 8 soft-select+soft-detect
title "Scan mode"
desc "Simplex or duplex."
strings Simplex Duplex
value Simplex
reload-params
option mode STRING 16 soft-select+soft-detect
title "Color mode"
strings Lineart Gray Color
value Gray
reload-params
option resolution INT 4 soft-select+soft-detect
title "Scan resolution"
unit DPI
range 100 600 1
value 200
reload-params
profile geometry
"#;

/// A flatbed and a feeder, its duplex a `BOOL` active only for the feeder.
const LEXMARK: &str = r#"
option @null GROUP 0 -
title Standard
option source STRING 32 soft-select+soft-detect
title "Scan source"
strings Flatbed "Automatic Document Feeder"
value Flatbed
reload-params
when Flatbed deactivate page-width page-height scan-both-sides
when "Automatic Document Feeder" activate page-width page-height scan-both-sides
option scan-both-sides BOOL 4 soft-select+soft-detect+inactive
title "Scan both sides"
value 0
reload-params
option mode STRING 16 soft-select+soft-detect
title "Scan mode"
strings Lineart Gray Color
value Color
reload-params
option resolution INT 4 soft-select+soft-detect
title "Scan resolution"
unit DPI
words 75 150 300 600
value 150
reload-params
profile geometry
start-inactive page-width page-height
"#;

/// A flatbed and a feeder, its duplex an `adf-mode` string active only for
/// the feeder.
const EPSON: &str = r#"
option @null GROUP 0 -
title Standard
option source STRING 32 soft-select+soft-detect
title "Scan source"
strings Flatbed "Automatic Document Feeder"
value Flatbed
reload-params
when Flatbed deactivate page-width page-height adf-mode
when "Automatic Document Feeder" activate page-width page-height adf-mode
option adf-mode STRING 8 soft-select+soft-detect+inactive
title "ADF Mode"
strings Simplex Duplex
value Simplex
reload-params
option mode STRING 16 soft-select+soft-detect
title "Scan mode"
strings Binary Gray Color
value Color
reload-params
option resolution INT 4 soft-select+soft-detect
title "Resolution"
unit DPI
range 50 600 1
value 300
reload-params
profile geometry
start-inactive page-width page-height
"#;

/// A feeder alone, its sides a `duplex` string: the front, the back, or
/// both.
const KODAK: &str = r#"
option @null GROUP 0 -
title Standard
option source STRING 8 soft-select+soft-detect
title "Scan source"
strings ADF
value ADF
option duplex STRING 8 soft-select+soft-detect
title "Duplex"
strings front back both
value front
reload-params
option mode STRING 16 soft-select+soft-detect
title "Scan mode"
strings Lineart Gray Color
value Color
reload-params
option resolution INT 4 soft-select+soft-detect
title "Scan resolution"
unit DPI
words 100 150 200 300 400 600
value 300
reload-params
profile geometry
"#;

/// Whether a descriptor holds only what SANE allows, so a caller may use
/// its option: a known value type, a size that fits it, and a constraint
/// that fits it.
pub fn well_formed(descriptor: &Descriptor) -> bool {
    let size = descriptor.size;
    let word = 4;
    let size_fits = match descriptor.value_type {
        ffi::SANE_TYPE_BOOL => size == word,
        ffi::SANE_TYPE_INT | ffi::SANE_TYPE_FIXED => size > 0 && size % word == 0,
        ffi::SANE_TYPE_STRING => size > 0,
        ffi::SANE_TYPE_BUTTON | ffi::SANE_TYPE_GROUP => true,
        _ => false,
    };
    let numeric = matches!(
        descriptor.value_type,
        ffi::SANE_TYPE_INT | ffi::SANE_TYPE_FIXED
    );
    let constraint_fits = match descriptor.constraint_type {
        ffi::SANE_CONSTRAINT_NONE => true,
        ffi::SANE_CONSTRAINT_RANGE => {
            numeric && matches!(descriptor.constraint, ConstraintSpec::Range(..))
        }
        ffi::SANE_CONSTRAINT_WORD_LIST => {
            numeric && matches!(descriptor.constraint, ConstraintSpec::Words(..))
        }
        ffi::SANE_CONSTRAINT_STRING_LIST => {
            descriptor.value_type == ffi::SANE_TYPE_STRING
                && matches!(descriptor.constraint, ConstraintSpec::Strings(..))
        }
        _ => false,
    };
    size_fits && constraint_fits
}

/// A value as `size` bytes of words, or a string with its `NUL` if it fits:
/// what a get writes.
pub fn sized(value: &RawValue, size: i32) -> RawValue {
    let size = usize::try_from(size).unwrap_or(0);
    match value {
        RawValue::Words(words) => {
            let count = size / 4;
            let mut sized: Vec<i32> = words.iter().copied().take(count).collect();
            let last = words.last().copied().unwrap_or(0);
            sized.resize(count, last);
            RawValue::Words(sized)
        }
        RawValue::Bytes(bytes) => {
            let mut sized = bytes.clone();
            sized.push(0);
            sized.truncate(size);
            RawValue::Bytes(sized)
        }
    }
}

/// A value set, fitted to the constraint as a backend might: a range's
/// words clamped, a word list's moved to the nearest entry. `None` if it
/// can't be fitted (a string not in the list), and whether it changed.
pub fn fit(descriptor: &Descriptor, value: RawValue) -> Option<(RawValue, bool)> {
    match (&descriptor.constraint, value) {
        (ConstraintSpec::Range(min, max, _), RawValue::Words(words))
            if descriptor.constraint_type == ffi::SANE_CONSTRAINT_RANGE =>
        {
            let fitted: Vec<i32> = words
                .iter()
                .map(|w| (*w).clamp(*min, (*max).max(*min)))
                .collect();
            let changed = fitted != words;
            Some((RawValue::Words(fitted), changed))
        }
        (ConstraintSpec::Words(list, _), RawValue::Words(words))
            if descriptor.constraint_type == ffi::SANE_CONSTRAINT_WORD_LIST && !list.is_empty() =>
        {
            let fitted: Vec<i32> = words
                .iter()
                .map(|w| {
                    *list
                        .iter()
                        .min_by_key(|entry| (i64::from(**entry) - i64::from(*w)).abs())
                        .expect("not empty")
                })
                .collect();
            let changed = fitted != words;
            Some((RawValue::Words(fitted), changed))
        }
        (ConstraintSpec::Strings(list), RawValue::Bytes(bytes))
            if descriptor.constraint_type == ffi::SANE_CONSTRAINT_STRING_LIST =>
        {
            list.contains(&Text::Bytes(bytes.clone()))
                .then_some((RawValue::Bytes(bytes), false))
        }
        (_, value) => Some((value, false)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::script::Script;

    fn layout(text: &str) -> Layout {
        Script::parse(text).unwrap().layout
    }

    #[test]
    fn option_lines_build_descriptors() {
        let layout = layout(
            r#"
            option-count 7
            option res FIXED 8 soft-select+soft-detect+automatic
            title "Resolution"
            desc @null
            unit DPI
            range -1.5 600 0.5
            value 300 150.25
            auto 75
            reload-params
            option list INT 4 5
            words 1 2 3
            word-list-length 2147483647
            unit 99
            constraint 7
            option src STRING 8 -
            strings a b
            strings-repeat 2
            value b
            option-null
            "#,
        );
        assert_eq!(layout.count, Some(7));
        let res = &layout.options[0];
        assert_eq!(
            res.descriptor,
            Descriptor {
                name: Text::Bytes(b"res".to_vec()),
                title: Text::Bytes(b"Resolution".to_vec()),
                desc: Text::Null,
                value_type: ffi::SANE_TYPE_FIXED,
                unit: ffi::SANE_UNIT_DPI,
                size: 8,
                cap: 21,
                constraint_type: ffi::SANE_CONSTRAINT_RANGE,
                constraint: ConstraintSpec::Range(-98304, 600 << 16, 32768),
            }
        );
        assert_eq!(
            res.value,
            RawValue::Words(vec![300 << 16, (150 << 16) + 16384])
        );
        assert_eq!(res.auto, Some(RawValue::Words(vec![75 << 16])));
        assert!(res.reload_params && !res.null);
        let list = &layout.options[1].descriptor;
        assert_eq!((list.unit, list.cap, list.constraint_type), (99, 5, 7));
        assert_eq!(
            list.constraint,
            ConstraintSpec::Words(vec![1, 2, 3], Some(i32::MAX))
        );
        let src = &layout.options[2];
        assert_eq!(src.descriptor.cap, 0);
        let ConstraintSpec::Strings(entries) = &src.descriptor.constraint else {
            panic!()
        };
        assert_eq!(entries.len(), 4);
        assert_eq!(entries[3], Text::Bytes(b"s3".to_vec()));
        assert_eq!(src.value, RawValue::Bytes(b"b".to_vec()));
        assert!(layout.options[3].null);
    }

    #[test]
    fn rules_find_their_targets_after_the_fact() {
        let layout = layout(
            r#"
            option source STRING 8 soft-select+soft-detect
            strings Flatbed ADF
            when ADF activate height width
            when ADF words res 100 200
            when Flatbed size height 8
            when Flatbed quiet
            option height FIXED 4 soft-select+soft-detect+inactive
            option width FIXED 4 soft-select+soft-detect
            option res INT 4 soft-select+soft-detect
            when 100 range height 0 1.5 0
            "#,
        );
        let adf = RawValue::Bytes(b"ADF".to_vec());
        let flatbed = RawValue::Bytes(b"Flatbed".to_vec());
        assert_eq!(
            layout.options[0].rules,
            vec![
                Rule {
                    when: adf.clone(),
                    change: Change::Activate(1)
                },
                Rule {
                    when: adf.clone(),
                    change: Change::Activate(2)
                },
                Rule {
                    when: adf,
                    change: Change::Words(3, vec![100, 200])
                },
                Rule {
                    when: flatbed.clone(),
                    change: Change::Size(1, 8)
                },
                Rule {
                    when: flatbed,
                    change: Change::Quiet
                },
            ]
        );
        assert_eq!(
            layout.options[3].rules,
            vec![Rule {
                when: RawValue::Words(vec![100]),
                change: Change::Range(1, 0, 98304, 0)
            }]
        );
    }

    #[test]
    fn every_profile_parses_and_is_well_formed() {
        for name in PROFILES {
            let layout = layout(&format!("profile {name}\n"));
            assert!(
                layout
                    .options
                    .iter()
                    .all(|option| well_formed(&option.descriptor)),
                "{name}"
            );
            let names: Vec<&Text> = layout
                .options
                .iter()
                .map(|option| &option.descriptor.name)
                .collect();
            for wanted in ["source", "resolution", "page-height", "page-width"] {
                assert!(
                    names.contains(&&Text::Bytes(wanted.as_bytes().to_vec())),
                    "{name} has no {wanted}"
                );
            }
        }
    }

    #[test]
    fn option_errors_say_their_line() {
        for (text, line) in [
            ("title x\n", 1),
            ("option a BOOL four -\n", 1),
            ("option a NOPE 4 -\n", 1),
            ("option a BOOL 4 soft-sel\n", 1),
            ("option a INT 4 -\nvalue x\n", 2),
            ("option a INT 4 -\nword-list-length 3\n", 2),
            ("option a INT 4 -\nwhen 1 frob a\n", 2),
            ("option a INT 4 -\nwhen 1 activate b\n", 2),
            ("start-inactive b\n", 1),
            ("profile nope\n", 1),
        ] {
            let error = Script::parse(text).unwrap_err();
            assert!(
                error.starts_with(&format!("line {line}: ")),
                "{text:?}: {error}"
            );
        }
    }

    #[test]
    fn well_formed_holds_sanes_rules() {
        let base = layout("option a INT 4 -\n").options[0].descriptor.clone();
        assert!(well_formed(&base));
        for broken in [
            Descriptor {
                size: 6,
                ..base.clone()
            },
            Descriptor {
                size: 0,
                ..base.clone()
            },
            Descriptor {
                value_type: 9,
                ..base.clone()
            },
            Descriptor {
                constraint_type: ffi::SANE_CONSTRAINT_STRING_LIST,
                constraint: ConstraintSpec::Strings(vec![]),
                ..base.clone()
            },
            Descriptor {
                constraint_type: 7,
                ..base.clone()
            },
            Descriptor {
                constraint_type: ffi::SANE_CONSTRAINT_RANGE,
                ..base.clone()
            },
        ] {
            assert!(!well_formed(&broken), "{broken:?}");
        }
    }

    #[test]
    fn values_are_sized_and_fitted() {
        assert_eq!(
            sized(&RawValue::Words(vec![1, 2]), 16),
            RawValue::Words(vec![1, 2, 2, 2])
        );
        assert_eq!(
            sized(&RawValue::Bytes(b"abc".to_vec()), 8),
            RawValue::Bytes(b"abc\0".to_vec())
        );
        assert_eq!(
            sized(&RawValue::Bytes(b"abcd".to_vec()), 4),
            RawValue::Bytes(b"abcd".to_vec())
        );
        let layout = layout(
            "option r INT 4 -\nrange 0 10 0\noption w INT 4 -\nwords 1 5 9\noption s STRING 8 -\nstrings a b\n",
        );
        let [r, w, s] = [0, 1, 2].map(|i| layout.options[i].descriptor.clone());
        assert_eq!(
            fit(&r, RawValue::Words(vec![12])),
            Some((RawValue::Words(vec![10]), true))
        );
        assert_eq!(
            fit(&r, RawValue::Words(vec![3])),
            Some((RawValue::Words(vec![3]), false))
        );
        assert_eq!(
            fit(&w, RawValue::Words(vec![6])),
            Some((RawValue::Words(vec![5]), true))
        );
        assert_eq!(fit(&s, RawValue::Bytes(b"c".to_vec())), None);
        assert_eq!(
            fit(&s, RawValue::Bytes(b"b".to_vec())),
            Some((RawValue::Bytes(b"b".to_vec()), false))
        );
    }
}
