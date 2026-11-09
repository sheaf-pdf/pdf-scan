//! The script a test gives the fake: what each call returns.
//!
//! Its format is in the crate's documentation.

use std::time::Duration;

use crate::feed::Feed;
use crate::ffi;
use crate::options::{Builder, Layout};

/// A string the fake hands to C: null, or bytes (a `NUL` added when given).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Text {
    Null,
    Bytes(Vec<u8>),
}

/// What `sane_open` answers for a name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpenAnswer {
    /// This status; a handle with `GOOD`.
    Status(i32),
    /// `GOOD`, and a null handle.
    NullHandle,
}

/// One `sane_init`'s answer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Init {
    pub status: i32,
    pub version_code: i32,
}

/// A parsed script.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Script {
    /// One per `sane_init`, in order; the last repeats.
    pub inits: Vec<Init>,
    /// What `sane_get_devices` returns.
    pub devices_status: i32,
    /// Whether it gives a null list with `GOOD`.
    pub devices_null: bool,
    /// The devices listed: name, vendor, model, type.
    pub devices: Vec<[Text; 4]>,
    /// How long each listing takes, in order; the last repeats.
    pub listing_delays: Vec<Duration>,
    /// `sane_open`'s answers for particular names.
    pub opens: Vec<(Vec<u8>, OpenAnswer)>,
    /// Whether `sane_strstatus` returns null.
    pub strstatus_null: bool,
    /// The options every handle has, after option 0.
    pub layout: Layout,
    /// What scanning gives.
    pub feed: Feed,
}

impl Default for Script {
    fn default() -> Script {
        Script {
            inits: vec![Init {
                status: ffi::SANE_STATUS_GOOD,
                version_code: ffi::SANE_VERSION_CODE(1, 0, 0),
            }],
            devices_status: ffi::SANE_STATUS_GOOD,
            devices_null: false,
            devices: Vec::new(),
            listing_delays: vec![Duration::ZERO],
            opens: Vec::new(),
            strstatus_null: false,
            layout: Layout::default(),
            feed: Feed::default(),
        }
    }
}

impl Script {
    /// The answer to the `n`th `sane_init`, counting from 0.
    pub fn init(&self, n: usize) -> Init {
        self.inits[n.min(self.inits.len() - 1)]
    }

    /// How long the `n`th listing takes, counting from 0.
    pub fn listing_delay(&self, n: usize) -> Duration {
        self.listing_delays[n.min(self.listing_delays.len() - 1)]
    }

    /// What `sane_open` answers for `name`: the script's answer, or `GOOD`
    /// for a device it lists and `INVAL` for any other.
    pub fn open(&self, name: &[u8]) -> OpenAnswer {
        if let Some((_, answer)) = self.opens.iter().find(|(given, _)| given == name) {
            return *answer;
        }
        let listed = self
            .devices
            .iter()
            .any(|device| device[0] == Text::Bytes(name.to_vec()));
        OpenAnswer::Status(if listed {
            ffi::SANE_STATUS_GOOD
        } else {
            ffi::SANE_STATUS_INVAL
        })
    }

    /// Parses a script, saying the line of the first error.
    pub fn parse(text: &str) -> Result<Script, String> {
        let mut script = Script::default();
        let mut inits = Vec::new();
        let mut delays = Vec::new();
        let mut options = Builder::default();
        for (number, line) in text.lines().enumerate() {
            let at = |error: String| format!("line {}: {error}", number + 1);
            options.place = format!("line {}", number + 1);
            let words = split(line).map_err(at)?;
            let Some((keyword, args)) = words.split_first() else {
                continue;
            };
            let keyword = word(keyword).map_err(at)?;
            let want = |n: usize| {
                if args.len() == n {
                    Ok(())
                } else {
                    Err(at(format!(
                        "{keyword} takes {n} values, not {}",
                        args.len()
                    )))
                }
            };
            match keyword.as_str() {
                "init" => {
                    if args.is_empty() || args.len() > 2 {
                        return Err(at(
                            "init takes a status, then a version if you like".to_owned()
                        ));
                    }
                    let status = status(&word(&args[0]).map_err(at)?).map_err(at)?;
                    let version_code = match args.get(1) {
                        Some(version) => self::version(&word(version).map_err(at)?).map_err(at)?,
                        None => ffi::SANE_VERSION_CODE(1, 0, 0),
                    };
                    inits.push(Init {
                        status,
                        version_code,
                    });
                }
                "devices" => {
                    want(1)?;
                    script.devices_status = status(&word(&args[0]).map_err(at)?).map_err(at)?;
                }
                "devices-null" => {
                    want(0)?;
                    script.devices_null = true;
                }
                "device" => {
                    want(4)?;
                    script.devices.push([
                        args[0].clone(),
                        args[1].clone(),
                        args[2].clone(),
                        args[3].clone(),
                    ]);
                }
                "device-repeat" => {
                    want(1)?;
                    let count: usize = word(&args[0])
                        .map_err(at)?
                        .parse()
                        .map_err(|_| at("not a count".to_owned()))?;
                    for _ in 0..count {
                        let name = format!("fake:{}", script.devices.len());
                        let bytes = |text: &str| Text::Bytes(text.as_bytes().to_vec());
                        script.devices.push([
                            bytes(&name),
                            bytes("Fake"),
                            bytes("Repeated"),
                            bytes("virtual device"),
                        ]);
                    }
                }
                "listing-delay" => {
                    if args.is_empty() {
                        return Err(at(
                            "listing-delay takes one or more times, in milliseconds".to_owned()
                        ));
                    }
                    for arg in args {
                        let ms: u64 = word(arg)
                            .map_err(at)?
                            .parse()
                            .map_err(|_| at("not milliseconds".to_owned()))?;
                        delays.push(Duration::from_millis(ms));
                    }
                }
                "open" => {
                    want(2)?;
                    let Text::Bytes(name) = &args[0] else {
                        return Err(at("open needs a name".to_owned()));
                    };
                    let answer = word(&args[1]).map_err(at)?;
                    let answer = if answer == "null-handle" {
                        OpenAnswer::NullHandle
                    } else {
                        OpenAnswer::Status(status(&answer).map_err(at)?)
                    };
                    script.opens.push((name.clone(), answer));
                }
                "strstatus-null" => {
                    want(0)?;
                    script.strstatus_null = true;
                }
                other => {
                    if !options.line(other, args).map_err(at)?
                        && !script.feed.line(other, args).map_err(at)?
                    {
                        return Err(at(format!("{other:?} isn't a keyword")));
                    }
                }
            }
        }
        script.layout = options.finish()?;
        if !inits.is_empty() {
            script.inits = inits;
        }
        if !delays.is_empty() {
            script.listing_delays = delays;
        }
        Ok(script)
    }
}

/// A word that must be text: not `@null`.
pub(crate) fn word(text: &Text) -> Result<String, String> {
    match text {
        Text::Null => Err("@null where a word is needed".to_owned()),
        Text::Bytes(bytes) => {
            String::from_utf8(bytes.clone()).map_err(|_| "a word that isn't UTF-8".to_owned())
        }
    }
}

/// A status by its name without `SANE_STATUS_`, or by its number.
pub fn status(word: &str) -> Result<i32, String> {
    let names = [
        ("GOOD", ffi::SANE_STATUS_GOOD),
        ("UNSUPPORTED", ffi::SANE_STATUS_UNSUPPORTED),
        ("CANCELLED", ffi::SANE_STATUS_CANCELLED),
        ("DEVICE_BUSY", ffi::SANE_STATUS_DEVICE_BUSY),
        ("INVAL", ffi::SANE_STATUS_INVAL),
        ("EOF", ffi::SANE_STATUS_EOF),
        ("JAMMED", ffi::SANE_STATUS_JAMMED),
        ("NO_DOCS", ffi::SANE_STATUS_NO_DOCS),
        ("COVER_OPEN", ffi::SANE_STATUS_COVER_OPEN),
        ("IO_ERROR", ffi::SANE_STATUS_IO_ERROR),
        ("NO_MEM", ffi::SANE_STATUS_NO_MEM),
        ("ACCESS_DENIED", ffi::SANE_STATUS_ACCESS_DENIED),
        ("WARMING_UP", ffi::SANE_STATUS_WARMING_UP),
        ("HW_LOCKED", ffi::SANE_STATUS_HW_LOCKED),
    ];
    if let Some((_, code)) = names.iter().find(|(name, _)| *name == word) {
        return Ok(*code);
    }
    word.parse().map_err(|_| format!("{word:?} isn't a status"))
}

/// The name of a status SANE defines, or its number.
pub fn status_name(code: i32) -> String {
    let names = [
        "GOOD",
        "UNSUPPORTED",
        "CANCELLED",
        "DEVICE_BUSY",
        "INVAL",
        "EOF",
        "JAMMED",
        "NO_DOCS",
        "COVER_OPEN",
        "IO_ERROR",
        "NO_MEM",
        "ACCESS_DENIED",
        "WARMING_UP",
        "HW_LOCKED",
    ];
    usize::try_from(code)
        .ok()
        .and_then(|index| names.get(index))
        .map_or_else(|| code.to_string(), |name| (*name).to_owned())
}

/// A version code from `major.minor.build`.
fn version(word: &str) -> Result<i32, String> {
    let parts: Vec<&str> = word.split('.').collect();
    let [major, minor, build] = parts[..] else {
        return Err(format!("{word:?} isn't major.minor.build"));
    };
    let number = |part: &str, max: i32| match part.parse::<i32>() {
        Ok(n) if (0..=max).contains(&n) => Ok(n),
        _ => Err(format!("{word:?} isn't major.minor.build")),
    };
    Ok(ffi::SANE_VERSION_CODE(
        number(major, 0xff)?,
        number(minor, 0xff)?,
        number(build, 0xffff)?,
    ))
}

/// Splits a line into words: bare, or quoted with `"` (`\"`, `\\` and `\xHH`
/// escapes inside), `@null` for a null pointer and `@long:N` for `N` bytes
/// of `x`; `#` starts a comment.
pub(crate) fn split(line: &str) -> Result<Vec<Text>, String> {
    let mut words = Vec::new();
    let mut chars = line.chars().peekable();
    while let Some(&c) = chars.peek() {
        if c.is_whitespace() {
            chars.next();
        } else if c == '#' {
            break;
        } else if c == '"' {
            chars.next();
            let mut bytes = Vec::new();
            loop {
                match chars.next() {
                    None => return Err("a quoted word isn't closed".to_owned()),
                    Some('"') => break,
                    Some('\\') => match chars.next() {
                        Some('"') => bytes.push(b'"'),
                        Some('\\') => bytes.push(b'\\'),
                        Some('x') => {
                            let hex: String = chars.by_ref().take(2).collect();
                            let byte = u8::from_str_radix(&hex, 16)
                                .map_err(|_| format!("\\x{hex} isn't a byte"))?;
                            bytes.push(byte);
                        }
                        other => return Err(format!("\\{} isn't an escape", other.unwrap_or(' '))),
                    },
                    Some(other) => {
                        let mut buffer = [0; 4];
                        bytes.extend_from_slice(other.encode_utf8(&mut buffer).as_bytes());
                    }
                }
            }
            words.push(Text::Bytes(bytes));
        } else {
            let mut bare = String::new();
            while let Some(&c) = chars.peek() {
                if c.is_whitespace() {
                    break;
                }
                bare.push(c);
                chars.next();
            }
            words.push(if bare == "@null" {
                Text::Null
            } else if let Some(count) = bare.strip_prefix("@long:") {
                let count: usize = count
                    .parse()
                    .map_err(|_| format!("{bare:?} needs a count"))?;
                Text::Bytes(vec![b'x'; count])
            } else {
                Text::Bytes(bare.into_bytes())
            });
        }
    }
    Ok(words)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bytes(text: &str) -> Text {
        Text::Bytes(text.as_bytes().to_vec())
    }

    #[test]
    fn an_empty_script_gives_sane_1_and_no_devices() {
        let script = Script::parse("# nothing\n\n").unwrap();
        assert_eq!(script, Script::default());
        assert_eq!(
            script.init(5),
            Init {
                status: 0,
                version_code: 0x0100_0000
            }
        );
        assert_eq!(
            script.open(b"fake:0"),
            OpenAnswer::Status(ffi::SANE_STATUS_INVAL)
        );
    }

    #[test]
    fn every_keyword_parses() {
        let script = Script::parse(
            r#"
            init GOOD 2.0.0
            init IO_ERROR
            init 0 1.4.7  # a comment
            devices ACCESS_DENIED
            devices-null
            device fake:0 "Fake Co" @null "flatbed scanner"
            device "odd\x01\"\\" v m t
            device-repeat 2
            listing-delay 1500 0
            open fake:0 DEVICE_BUSY
            open "odd" null-handle
            strstatus-null
            "#,
        )
        .unwrap();
        assert_eq!(
            script.inits,
            vec![
                Init {
                    status: 0,
                    version_code: 0x0200_0000
                },
                Init {
                    status: 9,
                    version_code: 0x0100_0000
                },
                Init {
                    status: 0,
                    version_code: 0x0104_0007
                },
            ]
        );
        assert_eq!(script.init(7).version_code, 0x0104_0007);
        assert_eq!(script.devices_status, ffi::SANE_STATUS_ACCESS_DENIED);
        assert!(script.devices_null);
        assert_eq!(
            script.devices[0],
            [
                bytes("fake:0"),
                bytes("Fake Co"),
                Text::Null,
                bytes("flatbed scanner")
            ]
        );
        assert_eq!(script.devices[1][0], Text::Bytes(b"odd\x01\"\\".to_vec()));
        assert_eq!(script.devices[2][0], bytes("fake:2"));
        assert_eq!(script.devices.len(), 4);
        assert_eq!(script.listing_delay(0), Duration::from_millis(1500));
        assert_eq!(script.listing_delay(9), Duration::ZERO);
        assert_eq!(
            script.open(b"fake:0"),
            OpenAnswer::Status(ffi::SANE_STATUS_DEVICE_BUSY)
        );
        assert_eq!(script.open(b"odd"), OpenAnswer::NullHandle);
        assert_eq!(
            script.open(b"fake:2"),
            OpenAnswer::Status(ffi::SANE_STATUS_GOOD)
        );
        assert!(script.strstatus_null);
    }

    #[test]
    fn long_words_and_numbered_statuses() {
        let script = Script::parse("device @long:70000 v m t\ninit -1\n").unwrap();
        assert_eq!(script.devices[0][0], Text::Bytes(vec![b'x'; 70000]));
        assert_eq!(script.inits[0].status, -1);
    }

    #[test]
    fn errors_say_their_line() {
        for (text, line) in [
            ("init\n", 1),
            ("\ninit NOPE\n", 2),
            ("device a b c\n", 1),
            ("frobnicate\n", 1),
            ("init GOOD 1.0\n", 1),
            ("init GOOD 1.0.70000\n", 1),
            ("device \"open a b c\n", 1),
            ("device \"\\q\" a b c\n", 1),
            ("listing-delay soon\n", 1),
            ("open @null GOOD\n", 1),
            ("device-repeat many\n", 1),
        ] {
            let error = Script::parse(text).unwrap_err();
            assert!(
                error.starts_with(&format!("line {line}: ")),
                "{text:?}: {error}"
            );
        }
    }

    #[test]
    fn statuses_have_names() {
        assert_eq!(status_name(7), "NO_DOCS");
        assert_eq!(status_name(42), "42");
        assert_eq!(status_name(-1), "-1");
        assert_eq!(status("NO_DOCS"), Ok(7));
    }
}
