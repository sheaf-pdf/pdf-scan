//! Options: what a device lets a frontend read and set, as SANE describes
//! them, and their values.
//!
//! Safe code only. The library module copies each descriptor SANE gives into
//! a [`RawDescriptor`], reading a constraint's pointer only as the member
//! [`constraint_member`] allows; this module decides what the copy means
//! ([`decode`]), refuses a call SANE shouldn't see, sizes the buffer a call
//! passes, and reads the value back.

use std::borrow::Cow;
use std::ffi::c_void;
use std::fmt;

use crate::library::CText;
use crate::{Error, LimitKind, Limits, ffi};

/// How an option's value is read: SANE's value type, checked.
///
/// A code SANE doesn't define is kept as [`Unknown`](Self::Unknown), and an
/// option of that type can't be read or set.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ValueType {
    /// A boolean, which travels as a word.
    Bool,
    /// One or more integers.
    Int,
    /// One or more fixed-point numbers ([`Fixed`]).
    Fixed,
    /// A string.
    String,
    /// A button: no value, but setting it does something.
    Button,
    /// The heading of a group of the options after it: no value.
    Group,
    /// A code SANE doesn't define.
    Unknown(i32),
}

impl ValueType {
    /// The type of a code, as `sane.h` numbers them.
    pub fn from_code(code: i32) -> ValueType {
        match code {
            ffi::SANE_TYPE_BOOL => ValueType::Bool,
            ffi::SANE_TYPE_INT => ValueType::Int,
            ffi::SANE_TYPE_FIXED => ValueType::Fixed,
            ffi::SANE_TYPE_STRING => ValueType::String,
            ffi::SANE_TYPE_BUTTON => ValueType::Button,
            ffi::SANE_TYPE_GROUP => ValueType::Group,
            other => ValueType::Unknown(other),
        }
    }

    /// The type's code.
    pub fn code(self) -> i32 {
        match self {
            ValueType::Bool => ffi::SANE_TYPE_BOOL,
            ValueType::Int => ffi::SANE_TYPE_INT,
            ValueType::Fixed => ffi::SANE_TYPE_FIXED,
            ValueType::String => ffi::SANE_TYPE_STRING,
            ValueType::Button => ffi::SANE_TYPE_BUTTON,
            ValueType::Group => ffi::SANE_TYPE_GROUP,
            ValueType::Unknown(code) => code,
        }
    }

    /// Whether the value is a vector of words: `INT` and `FIXED`.
    fn is_numeric(self) -> bool {
        matches!(self, ValueType::Int | ValueType::Fixed)
    }
}

/// The type as SANE names it (`BOOL`, `FIXED`), or its code.
impl fmt::Display for ValueType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ValueType::Bool => f.write_str("BOOL"),
            ValueType::Int => f.write_str("INT"),
            ValueType::Fixed => f.write_str("FIXED"),
            ValueType::String => f.write_str("STRING"),
            ValueType::Button => f.write_str("BUTTON"),
            ValueType::Group => f.write_str("GROUP"),
            ValueType::Unknown(code) => write!(f, "type {code}"),
        }
    }
}

/// The physical unit of an option's value: SANE's unit, checked.
///
/// A code SANE doesn't define is kept as [`Other`](Self::Other); the option
/// is still usable, since the unit only says what its numbers mean.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Unit {
    /// No unit, such as a count.
    None,
    /// Pixels.
    Pixel,
    /// Bits.
    Bit,
    /// Millimetres: SANE gives every length in them.
    Mm,
    /// Dots per inch.
    Dpi,
    /// A percentage.
    Percent,
    /// Microseconds.
    Microsecond,
    /// A code SANE doesn't define.
    Other(i32),
}

impl Unit {
    /// The unit of a code, as `sane.h` numbers them.
    pub fn from_code(code: i32) -> Unit {
        match code {
            ffi::SANE_UNIT_NONE => Unit::None,
            ffi::SANE_UNIT_PIXEL => Unit::Pixel,
            ffi::SANE_UNIT_BIT => Unit::Bit,
            ffi::SANE_UNIT_MM => Unit::Mm,
            ffi::SANE_UNIT_DPI => Unit::Dpi,
            ffi::SANE_UNIT_PERCENT => Unit::Percent,
            ffi::SANE_UNIT_MICROSECOND => Unit::Microsecond,
            other => Unit::Other(other),
        }
    }

    /// The unit's code.
    pub fn code(self) -> i32 {
        match self {
            Unit::None => ffi::SANE_UNIT_NONE,
            Unit::Pixel => ffi::SANE_UNIT_PIXEL,
            Unit::Bit => ffi::SANE_UNIT_BIT,
            Unit::Mm => ffi::SANE_UNIT_MM,
            Unit::Dpi => ffi::SANE_UNIT_DPI,
            Unit::Percent => ffi::SANE_UNIT_PERCENT,
            Unit::Microsecond => ffi::SANE_UNIT_MICROSECOND,
            Unit::Other(code) => code,
        }
    }
}

/// What an option can do: SANE's `SANE_CAP_*` bits, kept as they came.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub struct Capabilities(pub i32);

impl Capabilities {
    /// Software can set the value.
    pub const SOFT_SELECT: i32 = ffi::SANE_CAP_SOFT_SELECT;
    /// The user sets the value on the device itself.
    pub const HARD_SELECT: i32 = ffi::SANE_CAP_HARD_SELECT;
    /// Software can read the value.
    pub const SOFT_DETECT: i32 = ffi::SANE_CAP_SOFT_DETECT;
    /// The backend emulates the option; the device doesn't do it.
    pub const EMULATED: i32 = ffi::SANE_CAP_EMULATED;
    /// The backend can choose the value itself.
    pub const AUTOMATIC: i32 = ffi::SANE_CAP_AUTOMATIC;
    /// The option is inactive for now.
    pub const INACTIVE: i32 = ffi::SANE_CAP_INACTIVE;
    /// The option is for advanced users.
    pub const ADVANCED: i32 = ffi::SANE_CAP_ADVANCED;

    fn has(self, bit: i32) -> bool {
        self.0 & bit != 0
    }

    /// Software can set the value ([`set`](crate::Handle::set)).
    pub fn soft_select(self) -> bool {
        self.has(Self::SOFT_SELECT)
    }

    /// The user sets the value on the device itself.
    pub fn hard_select(self) -> bool {
        self.has(Self::HARD_SELECT)
    }

    /// Software can read the value ([`get`](crate::Handle::get)).
    pub fn soft_detect(self) -> bool {
        self.has(Self::SOFT_DETECT)
    }

    /// The backend emulates the option; the device doesn't do it.
    pub fn emulated(self) -> bool {
        self.has(Self::EMULATED)
    }

    /// The backend can choose the value itself
    /// ([`set_auto`](crate::Handle::set_auto)).
    pub fn automatic(self) -> bool {
        self.has(Self::AUTOMATIC)
    }

    /// The option is inactive for now: it can't be read or set until
    /// another option's value makes it active.
    pub fn inactive(self) -> bool {
        self.has(Self::INACTIVE)
    }

    /// The option is for advanced users.
    pub fn advanced(self) -> bool {
        self.has(Self::ADVANCED)
    }

    /// The option is active.
    pub fn is_active(self) -> bool {
        !self.inactive()
    }
}

/// The values an option may take, as its descriptor gives them.
///
/// Words are kept as they came: integers for an `INT` option, [`Fixed`]
/// words for a `FIXED` one (`Fixed(min).to_f64()`). Whether a value fits is
/// the backend's to say (it answers `INVAL`, or rounds the value and says
/// so); this crate passes it on unchecked.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Constraint {
    /// Any value of the option's type.
    None,
    /// From `min` to `max`, both included, in steps of `quant` from `min`
    /// (any step if `quant` is 0).
    Range {
        /// The least value.
        min: i32,
        /// The greatest value.
        max: i32,
        /// The step, or 0.
        quant: i32,
    },
    /// One of these words.
    WordList(Vec<i32>),
    /// One of these strings.
    StringList(Vec<SaneString>),
}

/// An option, as its descriptor said when it was last read.
///
/// Read when the device is opened, and again whenever SANE says the
/// descriptors changed ([`Handle::reload_options`](crate::Handle::reload_options)).
/// The strings for people are text (any bytes that aren't UTF-8 replaced, a
/// null string empty, one longer than [`Limits::string_bytes`] cut there).
/// A group's heading has only its title: its other fields are empty, as
/// the standard defines nothing else of a group's descriptor.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct OptionDescriptor {
    /// Its number, which [`get`](crate::Handle::get) and
    /// [`set`](crate::Handle::set) take. Option 0 is the count of options.
    pub index: usize,
    /// Its name, which is how frontends and scripts know it (`resolution`,
    /// `source`); empty for option 0 and for groups.
    pub name: String,
    /// Its title, one line for people.
    pub title: String,
    /// Its description, for people; a newline starts a paragraph.
    pub description: String,
    /// How its value is read.
    pub value_type: ValueType,
    /// Its value's unit.
    pub unit: Unit,
    /// Its value's size in bytes, as SANE gave it: a word per number for
    /// `INT` and `FIXED`, the longest string with its `NUL` for `STRING`.
    pub size: i32,
    /// What it can do.
    pub capabilities: Capabilities,
    /// The values it may take.
    pub constraint: Constraint,
    /// Why it can't be read or set, if it can't: its descriptor holds what
    /// SANE doesn't allow (an unknown type, a size that doesn't fit its
    /// type, a constraint that doesn't fit its type or can't be read). Its
    /// other fields hold what could be read; its constraint is
    /// [`Constraint::None`] when the constraint was what couldn't be.
    pub unusable: Option<String>,
}

impl OptionDescriptor {
    /// How many numbers its value holds: its size in words for `INT` and
    /// `FIXED`, 1 for `BOOL`, and 0 for the rest or an unusable option.
    pub fn word_count(&self) -> usize {
        if self.unusable.is_some() {
            return 0;
        }
        match self.value_type {
            ValueType::Bool => 1,
            ValueType::Int | ValueType::Fixed => words_in(self.size),
            _ => 0,
        }
    }

    /// Whether it's a group's heading.
    pub fn is_group(&self) -> bool {
        self.value_type == ValueType::Group
    }
}

/// The size of a word, in the bytes SANE counts sizes in.
const WORD: usize = size_of::<ffi::SANE_Word>();

/// Words in a size already checked to be a positive multiple of a word.
fn words_in(size: i32) -> usize {
    usize::try_from(size).unwrap_or(0) / WORD
}

/// A string as SANE's bytes: an option's string value, an entry of a string
/// list, or a device's vendor, model or kind ([`Device`](crate::Device)).
///
/// Kept as bytes, never decoded: an option's value goes back to the backend
/// exactly as it came, and a device's strings are as the backend sent them,
/// which the standard makes Latin-1 and some backends send as UTF-8, so
/// what they mean is the caller's to say. It holds no `NUL` when SANE gives
/// it; one given to [`set`](crate::Handle::set) with a `NUL` inside is
/// refused.
#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct SaneString(Vec<u8>);

impl SaneString {
    /// A string from its bytes.
    pub fn new(bytes: impl Into<Vec<u8>>) -> SaneString {
        SaneString(bytes.into())
    }

    /// The string's bytes.
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    /// The string's bytes, owned.
    pub fn into_bytes(self) -> Vec<u8> {
        self.0
    }

    /// The string as text, any bytes that aren't UTF-8 replaced.
    pub fn to_string_lossy(&self) -> Cow<'_, str> {
        String::from_utf8_lossy(&self.0)
    }
}

impl AsRef<[u8]> for SaneString {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

impl From<&str> for SaneString {
    fn from(text: &str) -> SaneString {
        SaneString(text.as_bytes().to_vec())
    }
}

impl From<String> for SaneString {
    fn from(text: String) -> SaneString {
        SaneString(text.into_bytes())
    }
}

impl From<&[u8]> for SaneString {
    fn from(bytes: &[u8]) -> SaneString {
        SaneString(bytes.to_vec())
    }
}

impl From<Vec<u8>> for SaneString {
    fn from(bytes: Vec<u8>) -> SaneString {
        SaneString(bytes)
    }
}

/// The string as text, any bytes that aren't UTF-8 replaced.
impl fmt::Display for SaneString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_string_lossy())
    }
}

/// The string's bytes, as a byte string is written in Rust (`SaneString(b"Caf\xe9")`): ASCII's
/// printable characters as they are, every other byte escaped, so bytes that aren't UTF-8 are
/// shown as they came.
impl fmt::Debug for SaneString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "SaneString(b\"{}\")", self.0.escape_ascii())
    }
}

/// SANE's fixed-point number: a word whose low 16 bits are the fraction, so
/// from -32768 to just under 32768 in steps of 1/65536.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct Fixed(pub i32);

impl Fixed {
    /// How many steps make 1.
    const ONE: f64 = (1 << ffi::SANE_FIXED_SCALE_SHIFT) as f64;

    /// The number, exactly (every fixed-point word is an `f64`).
    pub fn to_f64(self) -> f64 {
        f64::from(self.0) / Self::ONE
    }

    /// The fixed-point word nearest `value` (a half step rounded away from
    /// zero), or `None` if `value` isn't a number or is out of the range a
    /// word holds.
    pub fn from_f64(value: f64) -> Option<Fixed> {
        let steps = (value * Self::ONE).round();
        // NaN fails both comparisons; the infinities fail one.
        if steps >= f64::from(i32::MIN) && steps <= f64::from(i32::MAX) {
            // In range and whole, so the cast is exact.
            Some(Fixed(steps as i32))
        } else {
            None
        }
    }
}

/// The number, as `f64` displays it.
impl fmt::Display for Fixed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.to_f64(), f)
    }
}

/// An option's value, as [`get`](crate::Handle::get) reads it and
/// [`set`](crate::Handle::set) takes it.
///
/// A vector holds as many numbers as the option's size says
/// ([`OptionDescriptor::word_count`]), one for most options; a gamma table
/// holds hundreds.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Value {
    /// A `BOOL` option's value.
    Bool(bool),
    /// An `INT` option's values.
    Int(Vec<i32>),
    /// A `FIXED` option's values.
    Fixed(Vec<Fixed>),
    /// A `STRING` option's value.
    String(SaneString),
    /// What a `BUTTON` option is set with: pressing it.
    Button,
}

impl Value {
    /// The type of option this value is for.
    pub fn value_type(&self) -> ValueType {
        match self {
            Value::Bool(_) => ValueType::Bool,
            Value::Int(_) => ValueType::Int,
            Value::Fixed(_) => ValueType::Fixed,
            Value::String(_) => ValueType::String,
            Value::Button => ValueType::Button,
        }
    }
}

/// What SANE said about a set.
///
/// Set with [`set`](crate::Handle::set) or
/// [`set_auto`](crate::Handle::set_auto); bits SANE doesn't define are
/// ignored. Made outside this crate from [`Default`], its fields then set,
/// as more may come.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub struct SetInfo {
    /// The backend chose a value other than the one given (a resolution
    /// rounded to one it has): read the option to learn which.
    pub inexact: bool,
    /// Other options changed (became active or inactive, or took other
    /// constraints): the descriptors were read again before the call
    /// returned.
    pub reload_options: bool,
    /// The scan's parameters may have changed.
    pub reload_params: bool,
}

impl SetInfo {
    /// The info word's bits.
    pub(crate) fn from_word(word: i32) -> SetInfo {
        SetInfo {
            inexact: word & ffi::SANE_INFO_INEXACT != 0,
            reload_options: word & ffi::SANE_INFO_RELOAD_OPTIONS != 0,
            reload_params: word & ffi::SANE_INFO_RELOAD_PARAMS != 0,
        }
    }
}

/// Which member of a descriptor's constraint union may be read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Member {
    /// None: the option is unconstrained.
    None,
    Range,
    WordList,
    StringList,
}

/// The member a value type and constraint type allow reading, or why none
/// may be: ranges and word lists only on `INT` and `FIXED` options, string
/// lists only on `STRING` ones, as the standard has them.
pub(crate) fn constraint_member(value_type: i32, constraint_type: i32) -> Result<Member, String> {
    let value_type = ValueType::from_code(value_type);
    let member = match constraint_type {
        ffi::SANE_CONSTRAINT_NONE => return Ok(Member::None),
        ffi::SANE_CONSTRAINT_RANGE => Member::Range,
        ffi::SANE_CONSTRAINT_WORD_LIST => Member::WordList,
        ffi::SANE_CONSTRAINT_STRING_LIST => Member::StringList,
        other => return Err(format!("its constraint type {other} isn't SANE's")),
    };
    let fits = match member {
        Member::Range | Member::WordList => value_type.is_numeric(),
        Member::StringList => value_type == ValueType::String,
        Member::None => true,
    };
    if fits {
        Ok(member)
    } else {
        let what = match member {
            Member::Range => "a range",
            Member::WordList => "a word list",
            _ => "a string list",
        };
        Err(format!(
            "it has {what} constraint, which its type, {value_type}, can't have"
        ))
    }
}

/// How many entries a word list's first word claims, if it's within the cap.
pub(crate) fn word_list_length(claimed: i32, limits: Limits) -> Result<usize, String> {
    match usize::try_from(claimed) {
        Ok(length) if length <= limits.list_entries => Ok(length),
        Ok(_) => Err(format!(
            "its word list claims {claimed} entries, more than {}",
            limits.list_entries
        )),
        Err(_) => Err(format!("its word list claims {claimed} entries")),
    }
}

/// A descriptor's constraint as copied.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum RawConstraint {
    None,
    Range(ffi::SANE_Range),
    WordList(Vec<i32>),
    StringList(Vec<Vec<u8>>),
    /// Not read, or not read whole, and why.
    Unread(String),
}

/// A descriptor as copied from SANE: its integers as they came, its
/// strings read up to the cap, and its constraint read only as
/// [`constraint_member`] allows. A group's name, description and
/// constraint aren't read (the standard defines only its title and type),
/// so they're [`CText::Null`] and [`RawConstraint::None`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RawDescriptor {
    pub name: CText,
    pub title: CText,
    pub desc: CText,
    pub value_type: i32,
    pub unit: i32,
    pub size: i32,
    pub cap: i32,
    pub constraint: RawConstraint,
}

/// What a copied descriptor means: the descriptor, which may be unusable, or
/// [`Error::BadDescriptor`] if it can't stand for an option at all (a name
/// longer than the cap, which can't be cut, since the name is how the
/// option is known).
pub(crate) fn decode(
    index: usize,
    raw: RawDescriptor,
    limits: Limits,
) -> Result<OptionDescriptor, Error> {
    let text = |text: CText| match text {
        CText::Null => String::new(),
        CText::Text(bytes) | CText::TooLong(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
    };
    let name = match raw.name {
        CText::TooLong(_) => {
            return Err(Error::BadDescriptor {
                option: index,
                reason: format!("its name is longer than {} bytes", limits.string_bytes),
            });
        }
        other => text(other),
    };
    let value_type = ValueType::from_code(raw.value_type);
    let mut descriptor = OptionDescriptor {
        index,
        name,
        title: text(raw.title),
        description: text(raw.desc),
        value_type,
        unit: Unit::None,
        size: 0,
        capabilities: Capabilities(0),
        constraint: Constraint::None,
        unusable: None,
    };
    if value_type == ValueType::Group {
        descriptor.name = String::new();
        descriptor.description = String::new();
        return Ok(descriptor);
    }
    descriptor.unit = Unit::from_code(raw.unit);
    descriptor.size = raw.size;
    descriptor.capabilities = Capabilities(raw.cap);
    let mut unusable = Vec::new();
    if let ValueType::Unknown(code) = value_type {
        unusable.push(format!("its value type {code} isn't SANE's"));
    }
    if let Some(problem) = size_problem(value_type, raw.size, limits) {
        unusable.push(problem);
    }
    match raw.constraint {
        RawConstraint::None => {}
        RawConstraint::Range(range) => {
            descriptor.constraint = Constraint::Range {
                min: range.min,
                max: range.max,
                quant: range.quant,
            };
        }
        RawConstraint::WordList(words) => descriptor.constraint = Constraint::WordList(words),
        RawConstraint::StringList(strings) => {
            descriptor.constraint =
                Constraint::StringList(strings.into_iter().map(SaneString).collect());
        }
        RawConstraint::Unread(reason) => unusable.push(reason),
    }
    if !unusable.is_empty() {
        descriptor.unusable = Some(unusable.join("; "));
    }
    Ok(descriptor)
}

/// What's wrong with an option's size for its type, if anything: a positive
/// multiple of a word for `INT` and `FIXED`, exactly a word for `BOOL`, at
/// least one byte (its `NUL`) for `STRING`, none past the cap. A button's
/// size means nothing.
fn size_problem(value_type: ValueType, size: i32, limits: Limits) -> Option<String> {
    let bytes = usize::try_from(size).ok().filter(|&bytes| bytes > 0);
    match value_type {
        ValueType::Bool if bytes != Some(WORD) => {
            Some(format!("its size is {size}, and a BOOL's is a word"))
        }
        ValueType::Int | ValueType::Fixed | ValueType::String => match bytes {
            None => Some(format!("its size is {size}")),
            Some(bytes) if bytes > limits.option_bytes => Some(format!(
                "its size is {size}, more than {} bytes",
                limits.option_bytes
            )),
            Some(bytes) if value_type != ValueType::String && bytes % WORD != 0 => {
                Some(format!("its size is {size}, not a multiple of a word"))
            }
            Some(_) => None,
        },
        _ => None,
    }
}

/// Checks that option 0 can hold the count of options: an `INT` of one
/// word, usable. Anything else is [`Error::BadDescriptor`], for option 0.
pub(crate) fn check_count_option(zero: &OptionDescriptor) -> Result<(), Error> {
    if zero.value_type == ValueType::Int && zero.unusable.is_none() && zero.word_count() == 1 {
        return Ok(());
    }
    Err(Error::BadDescriptor {
        option: 0,
        reason: format!(
            "option 0, the count of options, is a {} of {} bytes{}",
            zero.value_type,
            zero.size,
            zero.unusable
                .as_ref()
                .map(|reason| format!(" ({reason})"))
                .unwrap_or_default()
        ),
    })
}

/// The number of options, from option 0's value: at least 1 (it counts
/// itself) and within the cap, or [`Error::BadDescriptor`], for option 0.
pub(crate) fn option_count(value: i32, limits: Limits) -> Result<usize, Error> {
    match usize::try_from(value) {
        Ok(count) if (1..=limits.options).contains(&count) => Ok(count),
        Ok(count) if count > limits.options => Err(Error::LimitExceeded {
            limit: LimitKind::Options,
            value: count as u64,
            cap: limits.options,
        }),
        _ => Err(Error::BadDescriptor {
            option: 0,
            reason: format!("it says there are {value} options, and option 0 is one"),
        }),
    }
}

/// The memory a call on an option passes: words for every type but
/// `STRING`, whose value is bytes. Sized from the option's descriptor, as the
/// backend reads and writes the whole of it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Buffer {
    Words(Vec<ffi::SANE_Word>),
    Bytes(Vec<u8>),
}

impl Buffer {
    /// The pointer `sane_control_option` takes, valid while the buffer is
    /// borrowed and neither moved nor resized.
    pub fn as_mut_ptr(&mut self) -> *mut c_void {
        match self {
            Buffer::Words(words) => words.as_mut_ptr().cast(),
            Buffer::Bytes(bytes) => bytes.as_mut_ptr().cast(),
        }
    }
}

/// What a call on an option asks of it.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Request<'a> {
    Get,
    Set(&'a Value),
    Auto,
}

impl Request<'_> {
    /// The action `sane_control_option` takes.
    pub fn action(self) -> ffi::SANE_Action {
        match self {
            Request::Get => ffi::SANE_ACTION_GET_VALUE,
            Request::Set(_) => ffi::SANE_ACTION_SET_VALUE,
            Request::Auto => ffi::SANE_ACTION_SET_AUTO,
        }
    }
}

/// The buffer for a request on `option`, or the refusal that keeps it from
/// the backend: no such option, unusable, no value, inactive, not readable
/// (a get) or not settable (a set), not automatic (`set_auto`), or, for a
/// set, a value of another type, of another length, too long for the
/// option's size, or holding a `NUL`.
pub(crate) fn prepare(
    options: &[OptionDescriptor],
    option: usize,
    request: Request<'_>,
) -> Result<Buffer, Error> {
    let descriptor = options.get(option).ok_or(Error::NoSuchOption { option })?;
    if let Some(reason) = &descriptor.unusable {
        return Err(Error::Unusable {
            option,
            reason: reason.clone(),
        });
    }
    let value_type = descriptor.value_type;
    let has_value = !matches!(value_type, ValueType::Button | ValueType::Group);
    if value_type == ValueType::Group || (matches!(request, Request::Get) && !has_value) {
        return Err(Error::NoValue { option, value_type });
    }
    let caps = descriptor.capabilities;
    if caps.inactive() {
        return Err(Error::Inactive { option });
    }
    match request {
        Request::Get if !caps.soft_detect() => return Err(Error::NotReadable { option }),
        Request::Set(_) | Request::Auto if !caps.soft_select() => {
            return Err(Error::NotSettable { option });
        }
        Request::Auto if !caps.automatic() => return Err(Error::NotAutomatic { option }),
        _ => {}
    }
    let words = || vec![0; descriptor.word_count().max(1)];
    let string_bytes = || vec![0; usize::try_from(descriptor.size).unwrap_or(0)];
    let Request::Set(value) = request else {
        // A get, or `set_auto`, whose value SANE ignores: a buffer of the
        // option's full size all the same, in case the backend writes the
        // value it chose there.
        return Ok(match value_type {
            ValueType::String => Buffer::Bytes(string_bytes()),
            _ => Buffer::Words(words()),
        });
    };
    let wrong_length = |given: usize| Error::WrongLength {
        option,
        expected: descriptor.word_count(),
        given,
    };
    match (value_type, value) {
        (ValueType::Bool, Value::Bool(on)) => Ok(Buffer::Words(vec![if *on {
            ffi::SANE_TRUE
        } else {
            ffi::SANE_FALSE
        }])),
        (ValueType::Int, Value::Int(numbers)) => {
            if numbers.len() != descriptor.word_count() {
                return Err(wrong_length(numbers.len()));
            }
            Ok(Buffer::Words(numbers.clone()))
        }
        (ValueType::Fixed, Value::Fixed(numbers)) => {
            if numbers.len() != descriptor.word_count() {
                return Err(wrong_length(numbers.len()));
            }
            Ok(Buffer::Words(numbers.iter().map(|fixed| fixed.0).collect()))
        }
        (ValueType::String, Value::String(text)) => {
            let bytes = text.as_bytes();
            if bytes.contains(&0) {
                return Err(Error::NulInString { option });
            }
            let mut buffer = string_bytes();
            // The string and its `NUL` must fit.
            if bytes.len() >= buffer.len() {
                return Err(Error::StringTooLong {
                    option,
                    size: buffer.len(),
                    given: bytes.len(),
                });
            }
            buffer[..bytes.len()].copy_from_slice(bytes);
            Ok(Buffer::Bytes(buffer))
        }
        // A button is pressed with a word, though SANE ignores it.
        (ValueType::Button, Value::Button) => Ok(Buffer::Words(vec![ffi::SANE_TRUE])),
        _ => Err(Error::WrongType {
            option,
            value_type,
            given: value.value_type(),
        }),
    }
}

/// The value a get read into `buffer`: a `BOOL` word that's neither 0 nor 1,
/// or a string with no `NUL` within the option's size, is
/// [`Error::BadValue`].
pub(crate) fn read_value(descriptor: &OptionDescriptor, buffer: Buffer) -> Result<Value, Error> {
    let option = descriptor.index;
    match (descriptor.value_type, buffer) {
        (ValueType::Bool, Buffer::Words(words)) => match words.first() {
            Some(&ffi::SANE_FALSE) => Ok(Value::Bool(false)),
            Some(&ffi::SANE_TRUE) => Ok(Value::Bool(true)),
            other => Err(Error::BadValue {
                option,
                reason: format!("a BOOL word of {}, neither 0 nor 1", other.unwrap_or(&0)),
            }),
        },
        (ValueType::Int, Buffer::Words(words)) => Ok(Value::Int(words)),
        (ValueType::Fixed, Buffer::Words(words)) => {
            Ok(Value::Fixed(words.into_iter().map(Fixed).collect()))
        }
        (ValueType::String, Buffer::Bytes(mut bytes)) => match bytes.iter().position(|&b| b == 0) {
            Some(end) => {
                bytes.truncate(end);
                Ok(Value::String(SaneString(bytes)))
            }
            None => Err(Error::BadValue {
                option,
                reason: format!("a string with no NUL in its {} bytes", bytes.len()),
            }),
        },
        (value_type, _) => Err(Error::NoValue { option, value_type }),
    }
}

/// Whether a descriptor SANE gives now still has the type, size and
/// capabilities of the copy, so a buffer sized from the copy still fits it.
pub(crate) fn still_current(
    descriptor: &OptionDescriptor,
    value_type: i32,
    size: i32,
    cap: i32,
) -> bool {
    descriptor.value_type.code() == value_type
        && descriptor.size == size
        && descriptor.capabilities.0 == cap
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(text: &str) -> CText {
        CText::Text(text.as_bytes().to_vec())
    }

    fn raw(value_type: i32, size: i32, cap: i32, constraint: RawConstraint) -> RawDescriptor {
        RawDescriptor {
            name: text("opt"),
            title: text("Title"),
            desc: text("What it does."),
            value_type,
            unit: ffi::SANE_UNIT_DPI,
            size,
            cap,
            constraint,
        }
    }

    const RW: i32 = ffi::SANE_CAP_SOFT_SELECT | ffi::SANE_CAP_SOFT_DETECT;

    fn decoded(raw: RawDescriptor) -> OptionDescriptor {
        decode(3, raw, Limits::default()).unwrap()
    }

    #[test]
    fn codes_round_trip_and_unknown_ones_are_kept() {
        for code in -1..9 {
            assert_eq!(ValueType::from_code(code).code(), code);
            assert_eq!(Unit::from_code(code).code(), code);
        }
        assert_eq!(ValueType::from_code(2), ValueType::Fixed);
        assert_eq!(ValueType::from_code(9), ValueType::Unknown(9));
        assert_eq!(Unit::from_code(4), Unit::Dpi);
        assert_eq!(Unit::from_code(99), Unit::Other(99));
        assert_eq!(ValueType::Fixed.to_string(), "FIXED");
        assert_eq!(ValueType::Unknown(9).to_string(), "type 9");
    }

    #[test]
    fn capabilities_read_their_bits() {
        let caps = Capabilities(RW | ffi::SANE_CAP_AUTOMATIC | ffi::SANE_CAP_ADVANCED);
        assert!(caps.soft_select() && caps.soft_detect() && caps.automatic() && caps.advanced());
        assert!(!caps.hard_select() && !caps.emulated() && !caps.inactive());
        assert!(caps.is_active());
        assert!(!Capabilities(ffi::SANE_CAP_INACTIVE).is_active());
    }

    #[test]
    fn fixed_point_converts_both_ways() {
        assert_eq!(Fixed(42 << 16).to_f64(), 42.0);
        assert_eq!(Fixed(-1).to_f64(), -1.0 / 65536.0);
        assert_eq!(Fixed::from_f64(42.0), Some(Fixed(42 << 16)));
        assert_eq!(Fixed::from_f64(-42.17), Some(Fixed(-2_763_653)));
        // The nearest word, where SANE_FIX would truncate.
        assert_eq!(Fixed::from_f64(0.1), Some(Fixed(6554)));
        assert_eq!(ffi::SANE_FIX(0.1), 6553);
        assert_eq!(Fixed::from_f64(-32768.0), Some(Fixed(i32::MIN)));
        assert_eq!(Fixed::from_f64(32767.99998), Some(Fixed(i32::MAX)));
        assert_eq!(Fixed::from_f64(32768.0), None);
        assert_eq!(Fixed::from_f64(-32768.00001), None);
        assert_eq!(Fixed::from_f64(f64::NAN), None);
        assert_eq!(Fixed::from_f64(f64::INFINITY), None);
        assert_eq!(Fixed::from_f64(f64::NEG_INFINITY), None);
        for word in [i32::MIN, -65536, -1, 0, 1, 2_740_000, i32::MAX] {
            assert_eq!(Fixed::from_f64(Fixed(word).to_f64()), Some(Fixed(word)));
        }
        assert_eq!(Fixed(41 << 16 | 0x8000).to_string(), "41.5");
    }

    #[test]
    fn strings_keep_their_bytes() {
        let string = SaneString::new(b"ADF \xff".to_vec());
        assert_eq!(string.as_bytes(), b"ADF \xff");
        assert_eq!(string.to_string(), "ADF \u{fffd}");
        assert_eq!(format!("{string:?}"), "SaneString(b\"ADF \\xff\")");
        // Latin-1 and UTF-8 alike, as they came.
        assert_eq!(
            format!("{:?}", SaneString::from(b"Caf\xe9 \"\xc3\xa8\"".as_slice())),
            "SaneString(b\"Caf\\xe9 \\\"\\xc3\\xa8\\\"\")"
        );
        assert_eq!(
            SaneString::from("Flatbed"),
            SaneString::from(b"Flatbed".as_slice())
        );
        assert_eq!(SaneString::from(String::from("x")).into_bytes(), b"x");
    }

    #[test]
    fn set_info_reads_three_bits_and_ignores_the_rest() {
        assert_eq!(SetInfo::from_word(0), SetInfo::default());
        assert_eq!(
            SetInfo::from_word(-1),
            SetInfo {
                inexact: true,
                reload_options: true,
                reload_params: true
            }
        );
        assert_eq!(
            SetInfo::from_word(ffi::SANE_INFO_RELOAD_OPTIONS | 0x100),
            SetInfo {
                inexact: false,
                reload_options: true,
                reload_params: false
            }
        );
    }

    #[test]
    fn constraints_are_read_only_where_their_type_allows() {
        use ffi::*;
        for value_type in [SANE_TYPE_INT, SANE_TYPE_FIXED] {
            assert_eq!(
                constraint_member(value_type, SANE_CONSTRAINT_RANGE),
                Ok(Member::Range)
            );
            assert_eq!(
                constraint_member(value_type, SANE_CONSTRAINT_WORD_LIST),
                Ok(Member::WordList)
            );
            assert!(constraint_member(value_type, SANE_CONSTRAINT_STRING_LIST).is_err());
        }
        assert_eq!(
            constraint_member(SANE_TYPE_STRING, SANE_CONSTRAINT_STRING_LIST),
            Ok(Member::StringList)
        );
        for value_type in [SANE_TYPE_BOOL, SANE_TYPE_STRING, SANE_TYPE_BUTTON, 9] {
            assert!(constraint_member(value_type, SANE_CONSTRAINT_RANGE).is_err());
            assert!(constraint_member(value_type, SANE_CONSTRAINT_WORD_LIST).is_err());
        }
        for value_type in [SANE_TYPE_BOOL, SANE_TYPE_INT, SANE_TYPE_BUTTON, 9] {
            assert!(constraint_member(value_type, SANE_CONSTRAINT_STRING_LIST).is_err());
        }
        for value_type in -1..9 {
            assert_eq!(
                constraint_member(value_type, SANE_CONSTRAINT_NONE),
                Ok(Member::None)
            );
            assert!(constraint_member(value_type, 4).is_err());
            assert!(constraint_member(value_type, -1).is_err());
        }
        assert_eq!(
            constraint_member(SANE_TYPE_INT, SANE_CONSTRAINT_STRING_LIST),
            Err("it has a string list constraint, which its type, INT, can't have".to_owned())
        );
    }

    #[test]
    fn word_list_lengths_are_capped_before_any_read() {
        let limits = Limits {
            list_entries: 4,
            ..Limits::default()
        };
        assert_eq!(word_list_length(0, limits), Ok(0));
        assert_eq!(word_list_length(4, limits), Ok(4));
        assert!(word_list_length(5, limits).is_err());
        assert!(word_list_length(-1, limits).is_err());
        assert!(word_list_length(i32::MAX, Limits::default()).is_err());
        assert!(word_list_length(i32::MIN, Limits::default()).is_err());
    }

    #[test]
    fn a_descriptor_decodes_as_typed() {
        let descriptor = decoded(raw(
            ffi::SANE_TYPE_INT,
            8,
            RW,
            RawConstraint::Range(ffi::SANE_Range {
                min: 4,
                max: 192,
                quant: 2,
            }),
        ));
        assert_eq!(
            descriptor,
            OptionDescriptor {
                index: 3,
                name: "opt".to_owned(),
                title: "Title".to_owned(),
                description: "What it does.".to_owned(),
                value_type: ValueType::Int,
                unit: Unit::Dpi,
                size: 8,
                capabilities: Capabilities(RW),
                constraint: Constraint::Range {
                    min: 4,
                    max: 192,
                    quant: 2
                },
                unusable: None,
            }
        );
        assert_eq!(descriptor.word_count(), 2);
        let list = decoded(raw(
            ffi::SANE_TYPE_STRING,
            32,
            RW,
            RawConstraint::StringList(vec![b"Flatbed".to_vec(), b"ADF".to_vec()]),
        ));
        assert_eq!(
            list.constraint,
            Constraint::StringList(vec!["Flatbed".into(), "ADF".into()])
        );
        assert_eq!(list.word_count(), 0);
    }

    #[test]
    fn null_strings_are_empty_and_text_past_the_cap_is_cut() {
        let mut descriptor = raw(ffi::SANE_TYPE_BOOL, 4, RW, RawConstraint::None);
        descriptor.name = CText::Null;
        descriptor.title = CText::Null;
        descriptor.desc = CText::TooLong(b"long".to_vec());
        let decoded = decoded(descriptor);
        assert_eq!(
            (
                decoded.name.as_str(),
                decoded.title.as_str(),
                decoded.description.as_str()
            ),
            ("", "", "long")
        );
        assert_eq!(decoded.unusable, None);
    }

    #[test]
    fn a_name_past_the_cap_is_a_bad_descriptor() {
        let mut descriptor = raw(ffi::SANE_TYPE_BOOL, 4, RW, RawConstraint::None);
        descriptor.name = CText::TooLong(b"xxxx".to_vec());
        match decode(7, descriptor, Limits::default()) {
            Err(Error::BadDescriptor { option: 7, reason }) => {
                assert!(reason.contains("name"), "{reason}")
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_group_keeps_only_its_title_and_type() {
        let mut descriptor = raw(ffi::SANE_TYPE_GROUP, -5, -1, RawConstraint::None);
        descriptor.name = CText::Null;
        descriptor.desc = CText::Text(b"never read".to_vec());
        let group = decoded(descriptor);
        assert!(group.is_group());
        assert_eq!(group.title, "Title");
        assert_eq!(
            (
                group.name.as_str(),
                group.description.as_str(),
                group.unit,
                group.size,
                group.capabilities,
                group.unusable
            ),
            ("", "", Unit::None, 0, Capabilities(0), None)
        );
    }

    #[test]
    fn what_sane_doesnt_allow_makes_an_option_unusable() {
        let limits = Limits {
            option_bytes: 64,
            ..Limits::default()
        };
        let unusable = |value_type, size, constraint| {
            decode(1, raw(value_type, size, RW, constraint), limits)
                .unwrap()
                .unusable
        };
        use ffi::*;
        for (value_type, size) in [
            (SANE_TYPE_INT, 0),
            (SANE_TYPE_INT, -4),
            (SANE_TYPE_INT, 6),
            (SANE_TYPE_FIXED, 68),
            (SANE_TYPE_BOOL, 8),
            (SANE_TYPE_BOOL, 0),
            (SANE_TYPE_STRING, 0),
            (SANE_TYPE_STRING, -1),
            (SANE_TYPE_STRING, 65),
            (9, 4),
        ] {
            assert!(
                unusable(value_type, size, RawConstraint::None).is_some(),
                "type {value_type}, size {size}"
            );
        }
        for (value_type, size) in [
            (SANE_TYPE_INT, 4),
            (SANE_TYPE_FIXED, 64),
            (SANE_TYPE_STRING, 1),
            (SANE_TYPE_STRING, 63),
            (SANE_TYPE_BUTTON, 0),
            (SANE_TYPE_BUTTON, -3),
        ] {
            assert_eq!(
                unusable(value_type, size, RawConstraint::None),
                None,
                "type {value_type}, size {size}"
            );
        }
        let unread = unusable(
            SANE_TYPE_INT,
            4,
            RawConstraint::Unread("its range is null".to_owned()),
        );
        assert_eq!(unread.as_deref(), Some("its range is null"));
        // Both problems said.
        let both = unusable(9, 0, RawConstraint::Unread("x".to_owned())).unwrap();
        assert_eq!(both, "its value type 9 isn't SANE's; x");
        // Unknown units are kept, and leave the option usable.
        let mut other = raw(SANE_TYPE_INT, 4, RW, RawConstraint::None);
        other.unit = 99;
        let other = decoded(other);
        assert_eq!((other.unit, other.unusable), (Unit::Other(99), None));
    }

    #[test]
    fn the_count_is_option_0s_value_within_the_cap() {
        let zero = OptionDescriptor {
            index: 0,
            name: String::new(),
            title: "Number of options".to_owned(),
            description: String::new(),
            value_type: ValueType::Int,
            unit: Unit::None,
            size: 4,
            capabilities: Capabilities(ffi::SANE_CAP_SOFT_DETECT),
            constraint: Constraint::None,
            unusable: None,
        };
        assert!(check_count_option(&zero).is_ok());
        for bad in [
            OptionDescriptor {
                value_type: ValueType::Fixed,
                ..zero.clone()
            },
            OptionDescriptor {
                size: 8,
                ..zero.clone()
            },
            OptionDescriptor {
                unusable: Some("x".to_owned()),
                ..zero.clone()
            },
        ] {
            assert!(matches!(
                check_count_option(&bad),
                Err(Error::BadDescriptor { option: 0, .. })
            ));
        }
        let limits = Limits {
            options: 10,
            ..Limits::default()
        };
        assert_eq!(option_count(1, limits).unwrap(), 1);
        assert_eq!(option_count(10, limits).unwrap(), 10);
        for value in [0, -1, i32::MIN] {
            assert!(matches!(
                option_count(value, limits),
                Err(Error::BadDescriptor { option: 0, .. })
            ));
        }
        for value in [11, i32::MAX] {
            assert!(matches!(
                option_count(value, limits),
                Err(Error::LimitExceeded {
                    limit: LimitKind::Options,
                    value: given,
                    cap: 10,
                }) if given == value as u64
            ));
        }
        assert!(option_count(1_000_000_000, Limits::default()).is_err());
    }

    fn option(value_type: ValueType, size: i32, cap: i32) -> OptionDescriptor {
        OptionDescriptor {
            index: 1,
            name: "opt".to_owned(),
            title: String::new(),
            description: String::new(),
            value_type,
            unit: Unit::None,
            size,
            capabilities: Capabilities(cap),
            constraint: Constraint::None,
            unusable: None,
        }
    }

    fn prepared(descriptor: OptionDescriptor, request: Request<'_>) -> Result<Buffer, Error> {
        let options = vec![
            option(ValueType::Int, 4, ffi::SANE_CAP_SOFT_DETECT),
            descriptor,
        ];
        prepare(&options, 1, request)
    }

    #[test]
    fn buffers_are_the_options_full_size() {
        use ValueType::*;
        assert_eq!(
            prepared(option(Int, 1024, RW), Request::Get).unwrap(),
            Buffer::Words(vec![0; 256])
        );
        assert_eq!(
            prepared(option(String, 32, RW), Request::Get).unwrap(),
            Buffer::Bytes(vec![0; 32])
        );
        assert_eq!(
            prepared(option(Bool, 4, RW | ffi::SANE_CAP_AUTOMATIC), Request::Auto).unwrap(),
            Buffer::Words(vec![0])
        );
        let mut expected = vec![0; 16];
        expected[..3].copy_from_slice(b"ADF");
        assert_eq!(
            prepared(
                option(String, 16, RW),
                Request::Set(&Value::String("ADF".into()))
            )
            .unwrap(),
            Buffer::Bytes(expected)
        );
        assert_eq!(
            prepared(option(Bool, 4, RW), Request::Set(&Value::Bool(true))).unwrap(),
            Buffer::Words(vec![1])
        );
        assert_eq!(
            prepared(option(Bool, 4, RW), Request::Set(&Value::Bool(false))).unwrap(),
            Buffer::Words(vec![0])
        );
        assert_eq!(
            prepared(
                option(Fixed, 8, RW),
                Request::Set(&Value::Fixed(vec![self::Fixed(1), self::Fixed(-2)]))
            )
            .unwrap(),
            Buffer::Words(vec![1, -2])
        );
        assert_eq!(
            prepared(option(Button, 0, RW), Request::Set(&Value::Button)).unwrap(),
            Buffer::Words(vec![1])
        );
    }

    #[test]
    fn requests_sane_shouldnt_see_are_refused() {
        use ValueType::*;
        fn refused(descriptor: OptionDescriptor, request: Request<'_>) -> Error {
            prepared(descriptor, request).unwrap_err()
        }
        assert!(matches!(
            prepare(&[], 0, Request::Get),
            Err(Error::NoSuchOption { option: 0 })
        ));
        let mut unusable = option(Int, 4, RW);
        unusable.unusable = Some("why".to_owned());
        assert!(matches!(
            refused(unusable, Request::Get),
            Error::Unusable { option: 1, .. }
        ));
        assert!(matches!(
            refused(option(Group, 0, 0), Request::Get),
            Error::NoValue {
                option: 1,
                value_type: Group
            }
        ));
        assert!(matches!(
            refused(option(Group, 0, 0), Request::Set(&Value::Button)),
            Error::NoValue { .. }
        ));
        assert!(matches!(
            refused(option(Button, 0, RW), Request::Get),
            Error::NoValue {
                value_type: Button,
                ..
            }
        ));
        assert!(matches!(
            refused(option(Int, 4, RW | ffi::SANE_CAP_INACTIVE), Request::Get),
            Error::Inactive { option: 1 }
        ));
        assert!(matches!(
            refused(
                option(Int, 4, RW | ffi::SANE_CAP_INACTIVE),
                Request::Set(&Value::Int(vec![1]))
            ),
            Error::Inactive { .. }
        ));
        assert!(matches!(
            refused(option(Bool, 4, ffi::SANE_CAP_HARD_SELECT), Request::Get),
            Error::NotReadable { option: 1 }
        ));
        assert!(matches!(
            refused(
                option(Bool, 4, ffi::SANE_CAP_SOFT_DETECT),
                Request::Set(&Value::Bool(true))
            ),
            Error::NotSettable { option: 1 }
        ));
        assert!(matches!(
            refused(
                option(Bool, 4, ffi::SANE_CAP_SOFT_DETECT | ffi::SANE_CAP_AUTOMATIC),
                Request::Auto
            ),
            Error::NotSettable { .. }
        ));
        assert!(matches!(
            refused(option(Bool, 4, RW), Request::Auto),
            Error::NotAutomatic { option: 1 }
        ));
        assert!(matches!(
            refused(
                option(Int, 4, RW),
                Request::Set(&Value::Fixed(vec![self::Fixed(1)]))
            ),
            Error::WrongType {
                option: 1,
                value_type: Int,
                given: Fixed
            }
        ));
        assert!(matches!(
            refused(option(Button, 0, RW), Request::Set(&Value::Bool(true))),
            Error::WrongType { .. }
        ));
        assert!(matches!(
            refused(option(Int, 8, RW), Request::Set(&Value::Int(vec![1]))),
            Error::WrongLength {
                option: 1,
                expected: 2,
                given: 1
            }
        ));
        assert!(matches!(
            refused(option(Fixed, 4, RW), Request::Set(&Value::Fixed(vec![]))),
            Error::WrongLength { .. }
        ));
        assert!(matches!(
            refused(
                option(String, 4, RW),
                Request::Set(&Value::String("abcd".into()))
            ),
            Error::StringTooLong {
                option: 1,
                size: 4,
                given: 4
            }
        ));
        assert!(
            prepared(
                option(String, 4, RW),
                Request::Set(&Value::String("abc".into()))
            )
            .is_ok()
        );
        assert!(matches!(
            refused(
                option(String, 8, RW),
                Request::Set(&Value::String("a\0b".into()))
            ),
            Error::NulInString { option: 1 }
        ));
    }

    #[test]
    fn values_are_read_back_checked() {
        use ValueType::*;
        let read = |value_type, size, buffer| read_value(&option(value_type, size, RW), buffer);
        assert_eq!(
            read(Bool, 4, Buffer::Words(vec![1])).unwrap(),
            Value::Bool(true)
        );
        assert_eq!(
            read(Bool, 4, Buffer::Words(vec![0])).unwrap(),
            Value::Bool(false)
        );
        for word in [2, -1, i32::MIN] {
            match read(Bool, 4, Buffer::Words(vec![word])) {
                Err(Error::BadValue { option: 1, reason }) => {
                    assert!(reason.contains(&word.to_string()), "{reason}")
                }
                other => panic!("{other:?}"),
            }
        }
        assert_eq!(
            read(Int, 8, Buffer::Words(vec![3, -4])).unwrap(),
            Value::Int(vec![3, -4])
        );
        assert_eq!(
            read(Fixed, 4, Buffer::Words(vec![65536])).unwrap(),
            Value::Fixed(vec![self::Fixed(65536)])
        );
        assert_eq!(
            read(String, 8, Buffer::Bytes(b"Gray\0xyz".to_vec())).unwrap(),
            Value::String("Gray".into())
        );
        assert!(matches!(
            read(String, 4, Buffer::Bytes(b"Gray".to_vec())),
            Err(Error::BadValue { option: 1, .. })
        ));
    }

    #[test]
    fn a_changed_type_size_or_capability_isnt_current() {
        let descriptor = option(ValueType::Int, 4, RW);
        assert!(still_current(&descriptor, ffi::SANE_TYPE_INT, 4, RW));
        assert!(!still_current(&descriptor, ffi::SANE_TYPE_FIXED, 4, RW));
        assert!(!still_current(&descriptor, ffi::SANE_TYPE_INT, 8, RW));
        assert!(!still_current(
            &descriptor,
            ffi::SANE_TYPE_INT,
            4,
            RW | ffi::SANE_CAP_INACTIVE
        ));
    }
}
