//! Capability containers, decoded from the block a source fills and encoded
//! into the one sent to it.
//!
//! Pure: no memory but the slice given is read, every length checked before
//! it is. A driver's block is untrusted data; a block that doesn't hold up is
//! a [`ContainerError`], never a panic. Values are little-endian, as on every
//! Windows TWAIN runs on.

use core::fmt;
use core::mem::{offset_of, size_of};

use crate::ffi;

/// A capability's container: TWAIN's four.
#[derive(Clone, Debug, PartialEq)]
pub enum Container {
    /// `TWON_ONEVALUE`.
    One(Item),
    /// `TWON_ENUMERATION`: values to choose from, with the current and default
    /// ones' indices.
    Enumeration(Enumeration),
    /// `TWON_ARRAY`.
    Array(Items),
    /// `TWON_RANGE`.
    Range(Range),
}

impl From<Item> for Container {
    fn from(item: Item) -> Self {
        Container::One(item)
    }
}

/// `TWON_ENUMERATION`'s contents. A source's indices are kept as it sent them,
/// in range or not; one sent to a source must be in range.
#[derive(Clone, Debug, PartialEq)]
pub struct Enumeration {
    pub items: Items,
    pub current: u32,
    pub default: u32,
}

impl Enumeration {
    /// The item at the current index, if the index is in range.
    pub fn current_item(&self) -> Option<&Item> {
        self.items
            .as_slice()
            .get(usize::try_from(self.current).ok()?)
    }

    /// The item at the default index, if the index is in range.
    pub fn default_item(&self) -> Option<&Item> {
        self.items
            .as_slice()
            .get(usize::try_from(self.default).ok()?)
    }
}

/// TWAIN's item types (`TWTY_`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ItemType {
    Int8,
    Int16,
    Int32,
    UInt8,
    UInt16,
    UInt32,
    Bool,
    Fix32,
    Frame,
    Str32,
    Str64,
    Str128,
    Str255,
    Str1024,
    Uni512,
    /// `TWTY_HANDLE`: a handle the source allocated, pointer-sized.
    Handle,
}

impl ItemType {
    /// The item type `twain.h` gives this value, if any.
    pub fn from_raw(raw: u16) -> Option<Self> {
        Some(match raw {
            ffi::TWTY_INT8 => ItemType::Int8,
            ffi::TWTY_INT16 => ItemType::Int16,
            ffi::TWTY_INT32 => ItemType::Int32,
            ffi::TWTY_UINT8 => ItemType::UInt8,
            ffi::TWTY_UINT16 => ItemType::UInt16,
            ffi::TWTY_UINT32 => ItemType::UInt32,
            ffi::TWTY_BOOL => ItemType::Bool,
            ffi::TWTY_FIX32 => ItemType::Fix32,
            ffi::TWTY_FRAME => ItemType::Frame,
            ffi::TWTY_STR32 => ItemType::Str32,
            ffi::TWTY_STR64 => ItemType::Str64,
            ffi::TWTY_STR128 => ItemType::Str128,
            ffi::TWTY_STR255 => ItemType::Str255,
            ffi::TWTY_STR1024 => ItemType::Str1024,
            ffi::TWTY_UNI512 => ItemType::Uni512,
            ffi::TWTY_HANDLE => ItemType::Handle,
            _ => return None,
        })
    }

    /// Its `TWTY_` value.
    pub fn raw(self) -> u16 {
        match self {
            ItemType::Int8 => ffi::TWTY_INT8,
            ItemType::Int16 => ffi::TWTY_INT16,
            ItemType::Int32 => ffi::TWTY_INT32,
            ItemType::UInt8 => ffi::TWTY_UINT8,
            ItemType::UInt16 => ffi::TWTY_UINT16,
            ItemType::UInt32 => ffi::TWTY_UINT32,
            ItemType::Bool => ffi::TWTY_BOOL,
            ItemType::Fix32 => ffi::TWTY_FIX32,
            ItemType::Frame => ffi::TWTY_FRAME,
            ItemType::Str32 => ffi::TWTY_STR32,
            ItemType::Str64 => ffi::TWTY_STR64,
            ItemType::Str128 => ffi::TWTY_STR128,
            ItemType::Str255 => ffi::TWTY_STR255,
            ItemType::Str1024 => ffi::TWTY_STR1024,
            ItemType::Uni512 => ffi::TWTY_UNI512,
            ItemType::Handle => ffi::TWTY_HANDLE,
        }
    }

    /// An item's size in a container, in bytes: `TW_HANDLE`'s is the
    /// process's pointer size.
    pub fn size(self) -> usize {
        match self {
            ItemType::Int8 | ItemType::UInt8 => 1,
            ItemType::Int16 | ItemType::UInt16 | ItemType::Bool => 2,
            ItemType::Int32 | ItemType::UInt32 => 4,
            ItemType::Fix32 => size_of::<ffi::TW_FIX32>(),
            ItemType::Frame => size_of::<ffi::TW_FRAME>(),
            ItemType::Str32 => size_of::<ffi::TW_STR32>(),
            ItemType::Str64 => size_of::<ffi::TW_STR64>(),
            ItemType::Str128 => size_of::<ffi::TW_STR128>(),
            ItemType::Str255 => size_of::<ffi::TW_STR255>(),
            ItemType::Str1024 => size_of::<ffi::TW_STR1024>(),
            ItemType::Uni512 => size_of::<ffi::TW_UNI512>(),
            ItemType::Handle => size_of::<usize>(),
        }
    }

    /// The longest string an item of this type holds, without its NUL: in
    /// bytes, or in UTF-16 units for `Uni512`. `None` for the other types.
    pub fn max_len(self) -> Option<usize> {
        match self {
            ItemType::Str32 => Some(32),
            ItemType::Str64 => Some(64),
            ItemType::Str128 => Some(128),
            ItemType::Str255 => Some(255),
            ItemType::Str1024 => Some(1024),
            ItemType::Uni512 => Some(511),
            _ => None,
        }
    }

    /// Whether a range may hold it: the integers and `FIX32`.
    pub fn is_number(self) -> bool {
        matches!(
            self,
            ItemType::Int8
                | ItemType::Int16
                | ItemType::Int32
                | ItemType::UInt8
                | ItemType::UInt16
                | ItemType::UInt32
                | ItemType::Fix32
        )
    }
}

impl fmt::Display for ItemType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let raw = i64::from(self.raw());
        match ffi::constants().find(|&(n, v)| v == raw && n.starts_with("TWTY_")) {
            Some((name, _)) => f.write_str(name),
            None => write!(f, "TWTY {raw:#06x}"),
        }
    }
}

/// `TW_FIX32`: `whole + frac / 65536`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Fix32 {
    pub whole: i16,
    pub frac: u16,
}

impl Fix32 {
    /// A whole number.
    pub const fn from_int(whole: i16) -> Self {
        Fix32 { whole, frac: 0 }
    }

    /// Its value: exact, as every `FIX32` is an `f64`.
    pub fn to_f64(self) -> f64 {
        f64::from(self.whole) + f64::from(self.frac) / 65536.0
    }

    /// The nearest `FIX32` to `value`, half a 65536th rounded away from zero;
    /// `None` if it's not a number or out of `FIX32`'s range (-32768 to just
    /// under 32768).
    pub fn from_f64(value: f64) -> Option<Self> {
        if !value.is_finite() {
            return None;
        }
        let scaled = (value * 65536.0).round();
        if !(f64::from(i32::MIN)..=f64::from(i32::MAX)).contains(&scaled) {
            return None;
        }
        // In range, checked above, so the cast is exact.
        let bits = scaled as i32;
        Some(Fix32::from_bits(bits as u32))
    }

    /// From its 32 bits, `whole` high: the value times 65536, two's complement.
    fn from_bits(bits: u32) -> Self {
        Fix32 {
            whole: (bits >> 16) as u16 as i16,
            frac: bits as u16,
        }
    }

    /// As it lies in memory, `whole` first: so in the low half of a `TW_UINT32`
    /// slot read little-endian.
    fn to_le_bytes(self) -> [u8; 4] {
        let [w0, w1] = self.whole.to_le_bytes();
        let [f0, f1] = self.frac.to_le_bytes();
        [w0, w1, f0, f1]
    }

    fn from_le_bytes([w0, w1, f0, f1]: [u8; 4]) -> Self {
        Fix32 {
            whole: i16::from_le_bytes([w0, w1]),
            frac: u16::from_le_bytes([f0, f1]),
        }
    }
}

/// `TW_FRAME`: a rectangle in `ICAP_UNITS`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Frame {
    pub left: Fix32,
    pub top: Fix32,
    pub right: Fix32,
    pub bottom: Fix32,
}

/// One item. A string is its bytes up to its NUL, as TWAIN gives them: in
/// the system's code page, not UTF-8; `Uni512`'s, UTF-16 units.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Item {
    Int8(i8),
    Int16(i16),
    Int32(i32),
    UInt8(u8),
    UInt16(u16),
    UInt32(u32),
    Bool(bool),
    Fix32(Fix32),
    Frame(Frame),
    Str32(Vec<u8>),
    Str64(Vec<u8>),
    Str128(Vec<u8>),
    Str255(Vec<u8>),
    Str1024(Vec<u8>),
    Uni512(Vec<u16>),
    /// A handle a source sent. Its value only: a source's handle is never
    /// read through here, and none is sent to a source.
    Handle(usize),
}

impl Item {
    /// Its type.
    pub fn item_type(&self) -> ItemType {
        match self {
            Item::Int8(_) => ItemType::Int8,
            Item::Int16(_) => ItemType::Int16,
            Item::Int32(_) => ItemType::Int32,
            Item::UInt8(_) => ItemType::UInt8,
            Item::UInt16(_) => ItemType::UInt16,
            Item::UInt32(_) => ItemType::UInt32,
            Item::Bool(_) => ItemType::Bool,
            Item::Fix32(_) => ItemType::Fix32,
            Item::Frame(_) => ItemType::Frame,
            Item::Str32(_) => ItemType::Str32,
            Item::Str64(_) => ItemType::Str64,
            Item::Str128(_) => ItemType::Str128,
            Item::Str255(_) => ItemType::Str255,
            Item::Str1024(_) => ItemType::Str1024,
            Item::Uni512(_) => ItemType::Uni512,
            Item::Handle(_) => ItemType::Handle,
        }
    }

    /// An integer item, `Bool` among them, as an `i64`.
    pub fn as_i64(&self) -> Option<i64> {
        Some(match *self {
            Item::Int8(v) => v.into(),
            Item::Int16(v) => v.into(),
            Item::Int32(v) => v.into(),
            Item::UInt8(v) => v.into(),
            Item::UInt16(v) => v.into(),
            Item::UInt32(v) => v.into(),
            Item::Bool(v) => v.into(),
            _ => return None,
        })
    }

    /// Read from exactly `item_type.size()` bytes.
    fn read(item_type: ItemType, b: &[u8]) -> Result<Item, ContainerError> {
        let short = || ContainerError::Short {
            len: b.len(),
            needed: item_type.size() as u64,
        };
        if b.len() != item_type.size() {
            return Err(short());
        }
        let u16_at = |i: usize| u16::from_le_bytes([b[i], b[i + 1]]);
        let fix_at = |i: usize| Fix32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]]);
        Ok(match item_type {
            ItemType::Int8 => Item::Int8(i8::from_le_bytes([b[0]])),
            ItemType::Int16 => Item::Int16(u16_at(0) as i16),
            ItemType::Int32 => Item::Int32(i32::from_le_bytes([b[0], b[1], b[2], b[3]])),
            ItemType::UInt8 => Item::UInt8(b[0]),
            ItemType::UInt16 => Item::UInt16(u16_at(0)),
            ItemType::UInt32 => Item::UInt32(u32::from_le_bytes([b[0], b[1], b[2], b[3]])),
            ItemType::Bool => Item::Bool(u16_at(0) != 0),
            ItemType::Fix32 => Item::Fix32(fix_at(0)),
            ItemType::Frame => Item::Frame(Frame {
                left: fix_at(0),
                top: fix_at(4),
                right: fix_at(8),
                bottom: fix_at(12),
            }),
            ItemType::Str32 => Item::Str32(to_nul(b)),
            ItemType::Str64 => Item::Str64(to_nul(b)),
            ItemType::Str128 => Item::Str128(to_nul(b)),
            ItemType::Str255 => Item::Str255(to_nul(b)),
            ItemType::Str1024 => Item::Str1024(to_nul(b)),
            ItemType::Uni512 => {
                let (units, _) = b.as_chunks::<2>();
                Item::Uni512(
                    units
                        .iter()
                        .map(|&u| u16::from_le_bytes(u))
                        .take_while(|&u| u != 0)
                        .collect(),
                )
            }
            ItemType::Handle => {
                let mut bytes = [0; size_of::<usize>()];
                bytes.copy_from_slice(b);
                Item::Handle(usize::from_le_bytes(bytes))
            }
        })
    }

    /// Written into exactly `self.item_type().size()` bytes, zeroed first.
    fn write(&self, out: &mut [u8]) -> Result<(), ContainerError> {
        out.fill(0);
        let item_type = self.item_type();
        let string = |s: &[u8], out: &mut [u8]| -> Result<(), ContainerError> {
            check_string(item_type, s.len(), s.iter().position(|&b| b == 0))?;
            out[..s.len()].copy_from_slice(s);
            Ok(())
        };
        match self {
            Item::Int8(v) => out.copy_from_slice(&v.to_le_bytes()),
            Item::Int16(v) => out.copy_from_slice(&v.to_le_bytes()),
            Item::Int32(v) => out.copy_from_slice(&v.to_le_bytes()),
            Item::UInt8(v) => out.copy_from_slice(&v.to_le_bytes()),
            Item::UInt16(v) => out.copy_from_slice(&v.to_le_bytes()),
            Item::UInt32(v) => out.copy_from_slice(&v.to_le_bytes()),
            Item::Bool(v) => out.copy_from_slice(&u16::from(*v).to_le_bytes()),
            Item::Fix32(v) => out.copy_from_slice(&v.to_le_bytes()),
            Item::Frame(f) => {
                for (i, v) in [f.left, f.top, f.right, f.bottom].into_iter().enumerate() {
                    out[i * 4..i * 4 + 4].copy_from_slice(&v.to_le_bytes());
                }
            }
            Item::Str32(s)
            | Item::Str64(s)
            | Item::Str128(s)
            | Item::Str255(s)
            | Item::Str1024(s) => string(s, out)?,
            Item::Uni512(s) => {
                check_string(item_type, s.len(), s.iter().position(|&u| u == 0))?;
                for (slot, u) in out.as_chunks_mut::<2>().0.iter_mut().zip(s) {
                    *slot = u.to_le_bytes();
                }
            }
            Item::Handle(_) => return Err(ContainerError::Handle),
        }
        Ok(())
    }

    /// From a range's `TW_UINT32` slot, where an item lies in the low bytes.
    fn from_slot(item_type: ItemType, slot: u32) -> Result<Item, ContainerError> {
        Ok(match item_type {
            ItemType::Int8 => Item::Int8(slot as u8 as i8),
            ItemType::Int16 => Item::Int16(slot as u16 as i16),
            ItemType::Int32 => Item::Int32(slot as i32),
            ItemType::UInt8 => Item::UInt8(slot as u8),
            ItemType::UInt16 => Item::UInt16(slot as u16),
            ItemType::UInt32 => Item::UInt32(slot),
            ItemType::Fix32 => Item::Fix32(Fix32::from_le_bytes(slot.to_le_bytes())),
            other => return Err(ContainerError::RangeOf(other)),
        })
    }

    /// Into a `TW_UINT32` slot, as C's `(TW_UINT32)value` puts it: a signed
    /// integer sign-extended, a `FIX32` as it lies in memory.
    fn to_slot(&self) -> Option<u32> {
        Some(match *self {
            Item::Int8(v) => i32::from(v) as u32,
            Item::Int16(v) => i32::from(v) as u32,
            Item::Int32(v) => v as u32,
            Item::UInt8(v) => v.into(),
            Item::UInt16(v) => v.into(),
            Item::UInt32(v) => v,
            Item::Bool(v) => v.into(),
            Item::Fix32(v) => u32::from_le_bytes(v.to_le_bytes()),
            _ => return None,
        })
    }
}

/// The bytes before the first NUL, or all of them.
fn to_nul(b: &[u8]) -> Vec<u8> {
    let end = b.iter().position(|&c| c == 0).unwrap_or(b.len());
    b[..end].to_vec()
}

fn check_string(item_type: ItemType, len: usize, nul: Option<usize>) -> Result<(), ContainerError> {
    if let Some(at) = nul {
        return Err(ContainerError::Nul { at });
    }
    let max = item_type.max_len().unwrap_or(0);
    if len > max {
        return Err(ContainerError::TooLong {
            item_type,
            len,
            max,
        });
    }
    Ok(())
}

/// Items of one type, as an enumeration or an array holds them: the type
/// kept apart, so an empty list has one.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Items {
    item_type: ItemType,
    items: Vec<Item>,
}

impl Items {
    /// Items of `item_type`; refused if one is of another.
    pub fn new(item_type: ItemType, items: Vec<Item>) -> Result<Self, ContainerError> {
        if let Some(other) = items.iter().find(|i| i.item_type() != item_type) {
            return Err(ContainerError::MixedTypes {
                expected: item_type,
                found: other.item_type(),
            });
        }
        Ok(Items { item_type, items })
    }

    pub fn item_type(&self) -> ItemType {
        self.item_type
    }

    pub fn as_slice(&self) -> &[Item] {
        &self.items
    }

    pub fn into_vec(self) -> Vec<Item> {
        self.items
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// `count` items read from the start of `bytes`, which must hold them.
    pub(crate) fn read(
        item_type: ItemType,
        count: u64,
        bytes: &[u8],
    ) -> Result<Self, ContainerError> {
        let size = item_type.size();
        let needed = count.saturating_mul(size as u64);
        if needed > bytes.len() as u64 {
            return Err(ContainerError::Short {
                len: bytes.len(),
                needed,
            });
        }
        // `needed` fits in the slice's length, so in a usize, and so does
        // `count`.
        let items = bytes[..needed as usize]
            .chunks_exact(size)
            .map(|b| Item::read(item_type, b))
            .collect::<Result<_, _>>()?;
        Ok(Items { item_type, items })
    }

    fn write(&self, out: &mut [u8]) -> Result<(), ContainerError> {
        let size = self.item_type.size();
        for (item, slot) in self.items.iter().zip(out.chunks_exact_mut(size)) {
            item.write(slot)?;
        }
        Ok(())
    }
}

/// `TWON_RANGE`'s contents: numbers of one type, the integers' or `FIX32`'s.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Range {
    item_type: ItemType,
    min: Item,
    max: Item,
    step: Item,
    default: Item,
    current: Item,
}

impl Range {
    /// A range; refused unless the five are numbers of one type.
    pub fn new(
        min: Item,
        max: Item,
        step: Item,
        default: Item,
        current: Item,
    ) -> Result<Self, ContainerError> {
        let item_type = min.item_type();
        if !item_type.is_number() {
            return Err(ContainerError::RangeOf(item_type));
        }
        for other in [&max, &step, &default, &current] {
            if other.item_type() != item_type {
                return Err(ContainerError::MixedTypes {
                    expected: item_type,
                    found: other.item_type(),
                });
            }
        }
        Ok(Range {
            item_type,
            min,
            max,
            step,
            default,
            current,
        })
    }

    pub fn item_type(&self) -> ItemType {
        self.item_type
    }
    pub fn min(&self) -> &Item {
        &self.min
    }
    pub fn max(&self) -> &Item {
        &self.max
    }
    pub fn step(&self) -> &Item {
        &self.step
    }
    pub fn default(&self) -> &Item {
        &self.default
    }
    pub fn current(&self) -> &Item {
        &self.current
    }
}

/// A container that doesn't hold up: a source's block, or one to send.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum ContainerError {
    #[error("container type {0:#06x} is none of TWAIN's four")]
    UnknownContainer(u16),
    #[error("item type {0:#06x} is none that TWAIN defines")]
    UnknownItemType(u16),
    #[error("the block holds {len} bytes, and its contents need {needed}")]
    Short { len: usize, needed: u64 },
    #[error("a range of {0}, which isn't a number")]
    RangeOf(ItemType),
    #[error("an item of {found} among items of {expected}")]
    MixedTypes { expected: ItemType, found: ItemType },
    #[error("a string of {len} is longer than {item_type} holds ({max})")]
    TooLong {
        item_type: ItemType,
        len: usize,
        max: usize,
    },
    #[error("a string with a NUL at {at}, where TWAIN would end it")]
    Nul { at: usize },
    #[error("index {index} of an enumeration of {len} items")]
    Index { index: u32, len: usize },
    #[error("{0} items: more than a container's 32-bit sizes hold")]
    TooMany(usize),
    #[error("a handle, which only a source gives")]
    Handle,
}

/// The offsets the containers' fields lie at, from the structs the header
/// contract checks.
const ONEVALUE_ITEM: usize = offset_of!(ffi::TW_ONEVALUE, Item);
const ENUMERATION_COUNT: usize = offset_of!(ffi::TW_ENUMERATION, NumItems);
const ENUMERATION_CURRENT: usize = offset_of!(ffi::TW_ENUMERATION, CurrentIndex);
const ENUMERATION_DEFAULT: usize = offset_of!(ffi::TW_ENUMERATION, DefaultIndex);
const ENUMERATION_ITEMS: usize = offset_of!(ffi::TW_ENUMERATION, ItemList);
const ARRAY_COUNT: usize = offset_of!(ffi::TW_ARRAY, NumItems);
const ARRAY_ITEMS: usize = offset_of!(ffi::TW_ARRAY, ItemList);
const RANGE_SLOTS: [usize; 5] = [
    offset_of!(ffi::TW_RANGE, MinValue),
    offset_of!(ffi::TW_RANGE, MaxValue),
    offset_of!(ffi::TW_RANGE, StepSize),
    offset_of!(ffi::TW_RANGE, DefaultValue),
    offset_of!(ffi::TW_RANGE, CurrentValue),
];

fn u32_at(block: &[u8], at: usize) -> Result<u32, ContainerError> {
    match block.get(at..at + 4) {
        Some(&[a, b, c, d]) => Ok(u32::from_le_bytes([a, b, c, d])),
        _ => Err(ContainerError::Short {
            len: block.len(),
            needed: at as u64 + 4,
        }),
    }
}

fn need(block: &[u8], needed: usize) -> Result<(), ContainerError> {
    if block.len() < needed {
        return Err(ContainerError::Short {
            len: block.len(),
            needed: needed as u64,
        });
    }
    Ok(())
}

impl Container {
    /// Its `TWON_` value.
    pub fn con_type(&self) -> u16 {
        match self {
            Container::One(_) => ffi::TWON_ONEVALUE,
            Container::Enumeration(_) => ffi::TWON_ENUMERATION,
            Container::Array(_) => ffi::TWON_ARRAY,
            Container::Range(_) => ffi::TWON_RANGE,
        }
    }

    /// Decoded from a source's block of `con_type`, all of it readable.
    pub(crate) fn decode(con_type: u16, block: &[u8]) -> Result<Container, ContainerError> {
        need(block, 2)?;
        let raw_type = u16::from_le_bytes([block[0], block[1]]);
        let item_type =
            ItemType::from_raw(raw_type).ok_or(ContainerError::UnknownItemType(raw_type))?;
        match con_type {
            ffi::TWON_ONEVALUE => {
                let end = ONEVALUE_ITEM + item_type.size();
                need(block, end)?;
                Ok(Container::One(Item::read(
                    item_type,
                    &block[ONEVALUE_ITEM..end],
                )?))
            }
            ffi::TWON_ENUMERATION => {
                need(block, ENUMERATION_ITEMS)?;
                let count = u32_at(block, ENUMERATION_COUNT)?;
                Ok(Container::Enumeration(Enumeration {
                    items: Items::read(item_type, count.into(), &block[ENUMERATION_ITEMS..])?,
                    current: u32_at(block, ENUMERATION_CURRENT)?,
                    default: u32_at(block, ENUMERATION_DEFAULT)?,
                }))
            }
            ffi::TWON_ARRAY => {
                need(block, ARRAY_ITEMS)?;
                let count = u32_at(block, ARRAY_COUNT)?;
                Ok(Container::Array(Items::read(
                    item_type,
                    count.into(),
                    &block[ARRAY_ITEMS..],
                )?))
            }
            ffi::TWON_RANGE => {
                need(block, size_of::<ffi::TW_RANGE>())?;
                let [min, max, step, default, current] = RANGE_SLOTS
                    .map(|at| u32_at(block, at).and_then(|slot| Item::from_slot(item_type, slot)));
                Ok(Container::Range(Range {
                    item_type,
                    min: min?,
                    max: max?,
                    step: step?,
                    default: default?,
                    current: current?,
                }))
            }
            other => Err(ContainerError::UnknownContainer(other)),
        }
    }

    /// The block to send to a source: its bytes, each item checked.
    pub(crate) fn encode(&self) -> Result<Vec<u8>, ContainerError> {
        let list = |head: usize, items: &Items| -> Result<Vec<u8>, ContainerError> {
            let len = items.len();
            let total = (len as u64)
                .checked_mul(items.item_type().size() as u64)
                .and_then(|n| n.checked_add(head as u64))
                .filter(|&n| n <= u64::from(u32::MAX))
                .ok_or(ContainerError::TooMany(len))?;
            // At most u32::MAX, checked above.
            let mut block = vec![0; total as usize];
            block[..2].copy_from_slice(&items.item_type().raw().to_le_bytes());
            items.write(&mut block[head..])?;
            Ok(block)
        };
        let count = |items: &Items| {
            u32::try_from(items.len()).map_err(|_| ContainerError::TooMany(items.len()))
        };
        match self {
            Container::One(item) => {
                let size = item.item_type().size();
                let mut block = vec![0; size_of::<ffi::TW_ONEVALUE>().max(ONEVALUE_ITEM + size)];
                block[..2].copy_from_slice(&item.item_type().raw().to_le_bytes());
                match item.to_slot() {
                    Some(slot) => {
                        block[ONEVALUE_ITEM..ONEVALUE_ITEM + 4].copy_from_slice(&slot.to_le_bytes())
                    }
                    None => item.write(&mut block[ONEVALUE_ITEM..ONEVALUE_ITEM + size])?,
                }
                Ok(block)
            }
            Container::Enumeration(e) => {
                let len = e.items.len();
                for index in [e.current, e.default] {
                    if usize::try_from(index).is_ok_and(|i| i >= len) {
                        return Err(ContainerError::Index { index, len });
                    }
                }
                let n = count(&e.items)?;
                let mut block = list(ENUMERATION_ITEMS, &e.items)?;
                block[ENUMERATION_COUNT..ENUMERATION_COUNT + 4].copy_from_slice(&n.to_le_bytes());
                block[ENUMERATION_CURRENT..ENUMERATION_CURRENT + 4]
                    .copy_from_slice(&e.current.to_le_bytes());
                block[ENUMERATION_DEFAULT..ENUMERATION_DEFAULT + 4]
                    .copy_from_slice(&e.default.to_le_bytes());
                Ok(block)
            }
            Container::Array(items) => {
                let n = count(items)?;
                let mut block = list(ARRAY_ITEMS, items)?;
                block[ARRAY_COUNT..ARRAY_COUNT + 4].copy_from_slice(&n.to_le_bytes());
                Ok(block)
            }
            Container::Range(r) => {
                let mut block = vec![0; size_of::<ffi::TW_RANGE>()];
                block[..2].copy_from_slice(&r.item_type.raw().to_le_bytes());
                for (at, item) in RANGE_SLOTS
                    .into_iter()
                    .zip([&r.min, &r.max, &r.step, &r.default, &r.current])
                {
                    let slot = item.to_slot().ok_or(ContainerError::RangeOf(r.item_type))?;
                    block[at..at + 4].copy_from_slice(&slot.to_le_bytes());
                }
                Ok(block)
            }
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    #[test]
    fn a_onevalue_of_each_small_type_reads_from_the_item_field() {
        // TW_ONEVALUE: ItemType, then Item, an item narrower than 4 bytes in
        // its low bytes.
        let block = [0x04, 0x00, 0x34, 0x12, 0, 0];
        assert_eq!(
            Container::decode(ffi::TWON_ONEVALUE, &block),
            Ok(Container::One(Item::UInt16(0x1234)))
        );
        let block = [0x01, 0x00, 0xfe, 0xff, 0xff, 0xff];
        assert_eq!(
            Container::decode(ffi::TWON_ONEVALUE, &block),
            Ok(Container::One(Item::Int16(-2)))
        );
        let block = [0x06, 0x00, 0x01, 0x00, 0, 0];
        assert_eq!(
            Container::decode(ffi::TWON_ONEVALUE, &block),
            Ok(Container::One(Item::Bool(true)))
        );
    }

    #[test]
    fn a_signed_onevalue_is_sign_extended_into_its_item_field() {
        let block = Container::One(Item::Int16(-2)).encode().unwrap();
        assert_eq!(block, [0x01, 0x00, 0xfe, 0xff, 0xff, 0xff]);
        let block = Container::One(Item::UInt8(0xff)).encode().unwrap();
        assert_eq!(block, [0x03, 0x00, 0xff, 0, 0, 0]);
    }

    #[test]
    fn a_wide_onevalue_runs_on_past_the_struct() {
        let item = Item::Str255(b"TWAIN2 Software Scanner".to_vec());
        let block = Container::One(item.clone()).encode().unwrap();
        assert_eq!(block.len(), 2 + 256);
        assert_eq!(&block[2..25], b"TWAIN2 Software Scanner");
        assert!(block[25..].iter().all(|&b| b == 0));
        assert_eq!(
            Container::decode(ffi::TWON_ONEVALUE, &block),
            Ok(Container::One(item))
        );
        assert_eq!(
            Container::decode(ffi::TWON_ONEVALUE, &block[..257]),
            Err(ContainerError::Short {
                len: 257,
                needed: 258
            })
        );
    }

    #[test]
    fn an_enumeration_keeps_its_indices_as_sent() {
        let mut block = vec![0x04, 0x00, 3, 0, 0, 0, 1, 0, 0, 0, 9, 0, 0, 0];
        block.extend([10, 0, 20, 0, 30, 0]);
        let Ok(Container::Enumeration(e)) = Container::decode(ffi::TWON_ENUMERATION, &block) else {
            panic!("not an enumeration");
        };
        assert_eq!(
            e.items.as_slice(),
            &[Item::UInt16(10), Item::UInt16(20), Item::UInt16(30)]
        );
        assert_eq!(e.current_item(), Some(&Item::UInt16(20)));
        assert_eq!((e.default, e.default_item()), (9, None));
        // Sent, an index out of range is refused.
        assert_eq!(
            Container::Enumeration(e).encode(),
            Err(ContainerError::Index { index: 9, len: 3 })
        );
    }

    #[test]
    fn a_count_past_the_block_is_refused_before_anything_is_allocated() {
        let block = [0x09, 0x00, 0xff, 0xff, 0xff, 0xff, 0, 0];
        assert_eq!(
            Container::decode(ffi::TWON_ARRAY, &block),
            Err(ContainerError::Short {
                len: 2,
                needed: 0xffff_ffff * 34
            })
        );
    }

    #[test]
    fn a_range_holds_numbers_in_its_slots() {
        let r = Range::new(
            Item::Fix32(Fix32::from_int(50)),
            Item::Fix32(Fix32::from_int(600)),
            Item::Fix32(Fix32 {
                whole: 0,
                frac: 0x8000,
            }),
            Item::Fix32(Fix32::from_int(-1)),
            Item::Fix32(Fix32::from_int(300)),
        )
        .unwrap();
        let block = Container::Range(r.clone()).encode().unwrap();
        assert_eq!(block.len(), 22);
        assert_eq!(&block[2..6], &[50, 0, 0, 0]);
        assert_eq!(&block[10..14], &[0, 0, 0, 0x80]);
        assert_eq!(&block[14..18], &[0xff, 0xff, 0, 0]);
        assert_eq!(
            Container::decode(ffi::TWON_RANGE, &block),
            Ok(Container::Range(r))
        );
        let strings = Range::new(
            Item::Str32(vec![]),
            Item::Str32(vec![]),
            Item::Str32(vec![]),
            Item::Str32(vec![]),
            Item::Str32(vec![]),
        );
        assert_eq!(strings, Err(ContainerError::RangeOf(ItemType::Str32)));
        let mut block = vec![0; 22];
        block[0] = 0x06;
        assert_eq!(
            Container::decode(ffi::TWON_RANGE, &block),
            Err(ContainerError::RangeOf(ItemType::Bool))
        );
    }

    #[test]
    fn strings_are_checked_before_they_are_sent() {
        let long = Container::One(Item::Str32(vec![b'a'; 33]));
        assert_eq!(
            long.encode(),
            Err(ContainerError::TooLong {
                item_type: ItemType::Str32,
                len: 33,
                max: 32
            })
        );
        assert_eq!(
            Container::One(Item::Str64(b"a\0b".to_vec())).encode(),
            Err(ContainerError::Nul { at: 1 })
        );
        assert_eq!(
            Container::One(Item::Handle(1)).encode(),
            Err(ContainerError::Handle)
        );
        let wide = Container::One(Item::Uni512("Scanner".encode_utf16().collect()));
        let block = wide.encode().unwrap();
        assert_eq!(block.len(), 2 + 1024);
        assert_eq!(Container::decode(ffi::TWON_ONEVALUE, &block), Ok(wide));
    }

    #[test]
    fn items_of_two_types_are_refused() {
        assert_eq!(
            Items::new(ItemType::UInt16, vec![Item::UInt16(1), Item::UInt32(2)]),
            Err(ContainerError::MixedTypes {
                expected: ItemType::UInt16,
                found: ItemType::UInt32
            })
        );
    }

    #[test]
    fn fix32_converts_both_ways() {
        assert_eq!(
            Fix32 {
                whole: 1,
                frac: 0x8000
            }
            .to_f64(),
            1.5
        );
        assert_eq!(
            Fix32 {
                whole: -1,
                frac: 0x8000
            }
            .to_f64(),
            -0.5
        );
        assert_eq!(
            Fix32::from_f64(-0.5),
            Some(Fix32 {
                whole: -1,
                frac: 0x8000
            })
        );
        assert_eq!(Fix32::from_f64(300.0), Some(Fix32::from_int(300)));
        assert_eq!(
            Fix32::from_f64(32767.99999),
            Some(Fix32 {
                whole: 32767,
                frac: 0xffff
            })
        );
        assert_eq!(Fix32::from_f64(32768.0), None);
        assert_eq!(Fix32::from_f64(-32768.0), Some(Fix32::from_int(-32768)));
        assert_eq!(Fix32::from_f64(f64::NAN), None);
    }

    #[test]
    fn item_types_are_named_as_twain_h_names_them() {
        assert_eq!(ItemType::UInt16.to_string(), "TWTY_UINT16");
        assert_eq!(ItemType::Uni512.to_string(), "TWTY_UNI512");
        for raw in 0..=0x20 {
            if let Some(t) = ItemType::from_raw(raw) {
                assert_eq!(t.raw(), raw);
            }
        }
    }

    /// SplitMix64: a fixed-seed generator for the random-input tests, so a
    /// failure repeats.
    pub(crate) struct Random(u64);

    impl Random {
        pub(crate) fn new(seed: u64) -> Self {
            Random(seed)
        }

        pub(crate) fn next(&mut self) -> u64 {
            self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
            let mut z = self.0;
            z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
            z ^ (z >> 31)
        }

        pub(crate) fn below(&mut self, n: u64) -> u64 {
            self.next() % n
        }

        pub(crate) fn bytes(&mut self, len: usize) -> Vec<u8> {
            (0..len).map(|_| self.next() as u8).collect()
        }
    }

    /// A header a decoder will read on from: a known item type mostly, and
    /// a small count.
    fn plausible_block(r: &mut Random) -> Vec<u8> {
        let len = r.below(1200) as usize;
        let mut block = r.bytes(len);
        if block.len() >= 2 && r.below(4) != 0 {
            let raw = r.below(0x11) as u16;
            block[..2].copy_from_slice(&raw.to_le_bytes());
        }
        if block.len() >= 6 && r.below(2) == 0 {
            let count = r.below(40) as u32;
            block[2..6].copy_from_slice(&count.to_le_bytes());
        }
        block
    }

    #[test]
    fn random_blocks_decode_or_are_refused_never_a_panic() {
        let mut r = Random::new(0x7a1e_5eed);
        let mut decoded = 0;
        for _ in 0..60_000 {
            let con_type = if r.below(10) == 0 {
                r.next() as u16
            } else {
                3 + r.below(4) as u16
            };
            let block = plausible_block(&mut r);
            if let Ok(c) = Container::decode(con_type, &block) {
                decoded += 1;
                // What decoded encodes, unless it holds what can't be sent.
                if let Ok(again) = c.encode() {
                    let back = Container::decode(c.con_type(), &again);
                    assert_eq!(back.as_ref(), Ok(&c), "{c:?}");
                }
            }
        }
        // The generator reaches past the headers often enough to matter.
        assert!(decoded > 10_000, "only {decoded} decoded");
    }

    fn random_item(r: &mut Random, item_type: ItemType) -> Item {
        let mut b = r.bytes(item_type.size());
        if let Some(max) = item_type.max_len() {
            // A string of random length, NUL-free, within its type's limit.
            let unit = if item_type == ItemType::Uni512 { 2 } else { 1 };
            let len = r.below(max as u64 + 1) as usize * unit;
            for c in &mut b[..len] {
                *c |= 1;
            }
            b[len..].fill(0);
        }
        Item::read(item_type, &b).unwrap()
    }

    #[test]
    fn random_containers_encode_and_decode_to_themselves() {
        let types = [
            ItemType::Int8,
            ItemType::Int16,
            ItemType::Int32,
            ItemType::UInt8,
            ItemType::UInt16,
            ItemType::UInt32,
            ItemType::Bool,
            ItemType::Fix32,
            ItemType::Frame,
            ItemType::Str32,
            ItemType::Str64,
            ItemType::Str128,
            ItemType::Str255,
            ItemType::Str1024,
            ItemType::Uni512,
        ];
        let mut r = Random::new(0xc0_47a1_4e25);
        for _ in 0..20_000 {
            let item_type = types[r.below(types.len() as u64) as usize];
            let n = r.below(6) as usize;
            let items = Items::new(
                item_type,
                (0..n).map(|_| random_item(&mut r, item_type)).collect(),
            )
            .unwrap();
            let c = match r.below(4) {
                0 => Container::One(random_item(&mut r, item_type)),
                1 if n > 0 => Container::Enumeration(Enumeration {
                    current: r.below(n as u64) as u32,
                    default: r.below(n as u64) as u32,
                    items,
                }),
                2 if item_type.is_number() => {
                    let [a, b, c, d, e] = [(); 5].map(|_| random_item(&mut r, item_type));
                    Container::Range(Range::new(a, b, c, d, e).unwrap())
                }
                _ => Container::Array(items),
            };
            let block = c.encode().unwrap();
            assert_eq!(Container::decode(c.con_type(), &block).as_ref(), Ok(&c));
        }
    }
}
