//! Capability containers as bytes: the `TW_ONEVALUE` a `MSG_SET` sends, and
//! the containers a source answers `MSG_GETCURRENT` with, decoded.
//!
//! Parsed from a byte slice, little-endian as Windows is, so nothing has to be
//! aligned: a `TW_ENUMERATION`'s items start at offset 14 of a block the
//! source allocated. The source's bytes are untrusted, and every length is
//! checked before it's read.

use std::fmt;

use crate::ffi::{
    TWON_ENUMERATION, TWON_ONEVALUE, TWTY_BOOL, TWTY_FIX32, TWTY_INT8, TWTY_INT16, TWTY_INT32,
    TWTY_UINT8, TWTY_UINT16, TWTY_UINT32,
};

/// The most items an enumeration may claim.
const MAX_ITEMS: u32 = 65_536;

/// A capability's current value, as its container gave it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Current {
    /// `TWON_ONEVALUE` or `TWON_ENUMERATION`.
    pub container: u16,
    pub item_type: u16,
    /// The item's bytes, zero-extended: its value is [`Current::value`].
    pub raw: u32,
    /// How many items the container held (1 for a one-value).
    pub items: u32,
}

/// A value of the item types the spike reads.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Value {
    Int(i64),
    Bool(bool),
    Fix32(f64),
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Int(v) => write!(f, "{v}"),
            Value::Bool(v) => write!(f, "{v}"),
            Value::Fix32(v) => write!(f, "{v}"),
        }
    }
}

impl Current {
    pub fn value(&self) -> Value {
        let raw = self.raw;
        match self.item_type {
            TWTY_INT8 => Value::Int(i64::from(raw as u8 as i8)),
            TWTY_INT16 => Value::Int(i64::from(raw as u16 as i16)),
            TWTY_INT32 => Value::Int(i64::from(raw as i32)),
            TWTY_BOOL => Value::Bool(raw as u16 != 0),
            // TW_FIX32: Whole (i16) first, then Frac (u16).
            TWTY_FIX32 => {
                Value::Fix32(f64::from(raw as u16 as i16) + f64::from((raw >> 16) as u16) / 65536.0)
            }
            _ => Value::Int(i64::from(raw)),
        }
    }

    /// The container's and the item type's names, for the report.
    pub fn kind(&self) -> String {
        let container = match self.container {
            TWON_ONEVALUE => "TW_ONEVALUE",
            TWON_ENUMERATION => "TW_ENUMERATION",
            _ => "container",
        };
        format!(
            "{container} of {} item(s), {}",
            self.items,
            type_name(self.item_type)
        )
    }
}

/// Why a container's bytes were refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    Short { need: usize, have: usize },
    ItemType(u16),
    Container(u16),
    Index { index: u32, items: u32 },
    TooMany(u32),
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Refusal::Short { need, have } => {
                write!(f, "the container is {have} bytes, and needs {need}")
            }
            Refusal::ItemType(t) => write!(f, "item type {t} isn't one the spike reads"),
            Refusal::Container(c) => write!(f, "container type {c} isn't one the spike reads"),
            Refusal::Index { index, items } => write!(f, "current index {index} of {items} items"),
            Refusal::TooMany(n) => write!(f, "{n} items claimed, more than {MAX_ITEMS}"),
        }
    }
}

/// An item type's size in a container, for the types the spike reads.
pub fn item_size(item_type: u16) -> Option<usize> {
    match item_type {
        TWTY_INT8 | TWTY_UINT8 => Some(1),
        TWTY_INT16 | TWTY_UINT16 | TWTY_BOOL => Some(2),
        TWTY_INT32 | TWTY_UINT32 | TWTY_FIX32 => Some(4),
        _ => None,
    }
}

fn type_name(item_type: u16) -> &'static str {
    match item_type {
        TWTY_INT8 => "TWTY_INT8",
        TWTY_INT16 => "TWTY_INT16",
        TWTY_INT32 => "TWTY_INT32",
        TWTY_UINT8 => "TWTY_UINT8",
        TWTY_UINT16 => "TWTY_UINT16",
        TWTY_UINT32 => "TWTY_UINT32",
        TWTY_BOOL => "TWTY_BOOL",
        TWTY_FIX32 => "TWTY_FIX32",
        _ => "another type",
    }
}

/// A `TW_ONEVALUE`'s 6 bytes: `ItemType`, then `Item`.
pub fn onevalue(item_type: u16, raw: u32) -> [u8; 6] {
    let mut bytes = [0u8; 6];
    bytes[..2].copy_from_slice(&item_type.to_le_bytes());
    bytes[2..].copy_from_slice(&raw.to_le_bytes());
    bytes
}

/// The current value from a container's bytes.
pub fn decode(container: u16, bytes: &[u8]) -> Result<Current, Refusal> {
    let need = |n: usize| {
        if bytes.len() < n {
            Err(Refusal::Short {
                need: n,
                have: bytes.len(),
            })
        } else {
            Ok(())
        }
    };
    match container {
        TWON_ONEVALUE => {
            need(6)?;
            let item_type = u16_at(bytes, 0);
            let size = item_size(item_type).ok_or(Refusal::ItemType(item_type))?;
            // The item sits in `Item`'s first bytes; the rest is whatever the
            // source left there.
            let raw = mask(u32_at(bytes, 2), size);
            Ok(Current {
                container,
                item_type,
                raw,
                items: 1,
            })
        }
        TWON_ENUMERATION => {
            need(14)?;
            let item_type = u16_at(bytes, 0);
            let items = u32_at(bytes, 2);
            let index = u32_at(bytes, 6);
            let size = item_size(item_type).ok_or(Refusal::ItemType(item_type))?;
            if items > MAX_ITEMS {
                return Err(Refusal::TooMany(items));
            }
            if index >= items {
                return Err(Refusal::Index { index, items });
            }
            need(14 + items as usize * size)?;
            let at = 14 + index as usize * size;
            let raw = match size {
                1 => u32::from(bytes[at]),
                2 => u32::from(u16_at(bytes, at)),
                _ => u32_at(bytes, at),
            };
            Ok(Current {
                container,
                item_type,
                raw,
                items,
            })
        }
        other => Err(Refusal::Container(other)),
    }
}

fn mask(raw: u32, size: usize) -> u32 {
    match size {
        1 => raw & 0xff,
        2 => raw & 0xffff,
        _ => raw,
    }
}

fn u16_at(bytes: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([bytes[at], bytes[at + 1]])
}

fn u32_at(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An enumeration's bytes, after `pad` bytes, so its items are misaligned
    /// as they are in a source's block.
    fn enumeration(
        pad: usize,
        item_type: u16,
        items: &[u32],
        size: usize,
        current: u32,
    ) -> Vec<u8> {
        let mut b = vec![0xaa; pad];
        b.extend_from_slice(&item_type.to_le_bytes());
        b.extend_from_slice(&(items.len() as u32).to_le_bytes());
        b.extend_from_slice(&current.to_le_bytes());
        b.extend_from_slice(&0u32.to_le_bytes());
        for &v in items {
            b.extend_from_slice(&v.to_le_bytes()[..size]);
        }
        b
    }

    #[test]
    fn a_onevalue_round_trips() {
        let bytes = onevalue(TWTY_UINT16, 2);
        let got = decode(TWON_ONEVALUE, &bytes).unwrap();
        assert_eq!((got.item_type, got.raw, got.items), (TWTY_UINT16, 2, 1));
        assert_eq!(got.value(), Value::Int(2));
    }

    #[test]
    fn a_onevalue_keeps_only_its_items_bytes() {
        // INT16 -1 with garbage above it in Item.
        let mut bytes = onevalue(TWTY_INT16, 0xdead_ffff);
        assert_eq!(
            decode(TWON_ONEVALUE, &bytes).unwrap().value(),
            Value::Int(-1)
        );
        bytes = onevalue(TWTY_BOOL, 0x1234_0000);
        assert_eq!(
            decode(TWON_ONEVALUE, &bytes).unwrap().value(),
            Value::Bool(false)
        );
    }

    #[test]
    fn fix32_is_whole_then_fraction() {
        let raw = 200u32 | (0x8000 << 16); // 200.5
        assert_eq!(
            decode(TWON_ONEVALUE, &onevalue(TWTY_FIX32, raw))
                .unwrap()
                .value(),
            Value::Fix32(200.5)
        );
    }

    #[test]
    fn an_enumerations_items_are_read_misaligned() {
        let b = enumeration(2, TWTY_UINT32, &[7, 0x0102_0304, 9], 4, 1);
        let got = decode(TWON_ENUMERATION, &b[2..]).unwrap();
        assert_eq!((got.raw, got.items), (0x0102_0304, 3));
        let b = enumeration(1, TWTY_BOOL, &[0, 1], 2, 1);
        assert_eq!(
            decode(TWON_ENUMERATION, &b[1..]).unwrap().value(),
            Value::Bool(true)
        );
    }

    #[test]
    fn short_or_strange_containers_are_refused() {
        assert_eq!(
            decode(TWON_ONEVALUE, &[0; 5]),
            Err(Refusal::Short { need: 6, have: 5 })
        );
        assert_eq!(
            decode(TWON_ONEVALUE, &onevalue(99, 0)),
            Err(Refusal::ItemType(99))
        );
        assert_eq!(decode(7, &[0; 16]), Err(Refusal::Container(7)));
        let b = enumeration(0, TWTY_UINT16, &[1, 2], 2, 2);
        assert_eq!(
            decode(TWON_ENUMERATION, &b),
            Err(Refusal::Index { index: 2, items: 2 })
        );
        let b = enumeration(0, TWTY_UINT16, &[1, 2, 3], 2, 0);
        assert_eq!(
            decode(TWON_ENUMERATION, &b[..b.len() - 1]),
            Err(Refusal::Short { need: 20, have: 19 })
        );
        let mut b = enumeration(0, TWTY_UINT16, &[1], 2, 0);
        b[2..6].copy_from_slice(&(MAX_ITEMS + 1).to_le_bytes());
        assert_eq!(
            decode(TWON_ENUMERATION, &b),
            Err(Refusal::TooMany(MAX_ITEMS + 1))
        );
    }
}
