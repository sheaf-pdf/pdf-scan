//! `DAT_EXTIMAGEINFO`'s block: the request written, and the source's answer
//! read, every entry's bounds checked, and its data decoded. Pure; reading
//! the memory an entry's handle names is the DSM module's.
//!
//! An entry's `Item` holds its data inline if it's four bytes or fewer (the
//! item type's size times `NumItems`), and else is the handle of a block
//! holding it, which the application frees, in either bitness: the TWAIN 2.4
//! specification's `TW_INFO` says four bytes, and its example of two barcodes
//! (two `TW_UINT32`, behind a handle) and the TWAIN Working Group's sample
//! application agree; its `TW_EXTIMAGEINFO` says `sizeof(TW_HANDLE)`, which
//! on 64-bit Windows would read eight bytes of a driver's handle as data. A
//! `TWTY_HANDLE` entry's `Item` is always such a handle, its block the data's
//! bytes (barcode text, in the specification's example).

use core::mem::{offset_of, size_of};

use crate::codes::ReturnCode;
use crate::container::{ContainerError, ItemType, Items};
use crate::ffi;

/// One item of extended image info, as the source answered it.
#[derive(Clone, Debug, PartialEq)]
pub struct ExtInfo {
    /// The `TWEI_` asked for.
    pub id: u16,
    pub value: ExtValue,
}

/// What the source gave for an item of extended image info.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum ExtValue {
    /// Its items, decoded.
    Items(Items),
    /// A `TWTY_HANDLE` item's block, its bytes as the source wrote them.
    Bytes(Vec<u8>),
    /// None, and why: `TWRC_INFONOTSUPPORTED`, `TWRC_DATANOTAVAILABLE`.
    Code(ReturnCode),
    /// An answer that doesn't hold up (an unknown item type, a block too
    /// short for its items). An unknown type's handle, if it was one, can't
    /// be told from data, so it isn't freed.
    Malformed(ContainerError),
}

/// Where an entry's data is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Data {
    /// None: the source gave none.
    None,
    /// A null handle for data a handle should hold.
    Missing,
    /// In `Item` itself: these bytes of it.
    Inline(Vec<u8>),
    /// In the block this handle names.
    Handle(usize),
    /// An item type no version of TWAIN defines: where can't be told.
    Unknown,
}

/// An entry and its data, read out.
pub(crate) struct Answer {
    pub(crate) entry: Entry,
    pub(crate) data: Read,
}

/// An entry's data, read: inline, or from its handle's block (freed).
pub(crate) enum Read {
    None,
    Missing,
    Bytes(Vec<u8>),
    /// A handle another entry named first: read (and freed) with it.
    Repeated,
    /// A block that doesn't hold up (too large to read).
    Refused(ContainerError),
    Unknown,
}

/// The most bytes an entry's data is inline in `Item`.
const INLINE: u64 = 4;

/// One `TW_INFO` as the source filled it in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Entry {
    pub(crate) id: u16,
    pub(crate) item_type: u16,
    pub(crate) num_items: u16,
    pub(crate) return_code: u16,
    pub(crate) item: usize,
}

const ENTRIES: usize = offset_of!(ffi::TW_EXTIMAGEINFO, Info);
const ENTRY: usize = size_of::<ffi::TW_INFO>();
const ID: usize = offset_of!(ffi::TW_INFO, InfoID);
const ITEM_TYPE: usize = offset_of!(ffi::TW_INFO, ItemType);
const NUM_ITEMS: usize = offset_of!(ffi::TW_INFO, NumItems);
const RETURN_CODE: usize = offset_of!(ffi::TW_INFO, ReturnCode);
const ITEM: usize = offset_of!(ffi::TW_INFO, Item);

impl Entry {
    /// Where its data is, by the rule above.
    pub(crate) fn data(&self) -> Data {
        if ReturnCode(self.return_code) != ReturnCode::SUCCESS {
            return Data::None;
        }
        let Some(item_type) = ItemType::from_raw(self.item_type) else {
            return Data::Unknown;
        };
        let size = (item_type.size() as u64) * u64::from(self.num_items);
        let handle = item_type == ItemType::Handle || size > INLINE;
        match (handle, self.item) {
            (true, 0) if self.num_items == 0 => Data::None,
            (true, 0) => Data::Missing,
            (true, item) => Data::Handle(item),
            // `size` is at most 4 here, and a usize at least 4 bytes.
            (false, item) => Data::Inline(item.to_le_bytes()[..size as usize].to_vec()),
        }
    }
}

/// An answer decoded.
pub(crate) fn decode(answer: Answer) -> ExtInfo {
    let Answer { entry, data } = answer;
    let rc = ReturnCode(entry.return_code);
    let value = if rc != ReturnCode::SUCCESS {
        ExtValue::Code(rc)
    } else {
        match (ItemType::from_raw(entry.item_type), data) {
            (None, _) | (_, Read::Unknown) => {
                ExtValue::Malformed(ContainerError::UnknownItemType(entry.item_type))
            }
            (Some(item_type), Read::Missing) => ExtValue::Malformed(ContainerError::Short {
                len: 0,
                needed: (item_type.size() as u64) * u64::from(entry.num_items),
            }),
            (_, Read::Repeated) => ExtValue::Malformed(ContainerError::RepeatedHandle),
            (_, Read::Refused(e)) => ExtValue::Malformed(e),
            (Some(ItemType::Handle), Read::Bytes(bytes)) => ExtValue::Bytes(bytes),
            (Some(ItemType::Handle), Read::None) => ExtValue::Bytes(Vec::new()),
            (Some(item_type), Read::Bytes(bytes)) => {
                match Items::read(item_type, entry.num_items.into(), &bytes) {
                    Ok(items) => ExtValue::Items(items),
                    Err(e) => ExtValue::Malformed(e),
                }
            }
            (Some(item_type), Read::None) => match Items::new(item_type, Vec::new()) {
                Ok(items) => ExtValue::Items(items),
                Err(e) => ExtValue::Malformed(e),
            },
        }
    };
    ExtInfo {
        id: entry.id,
        value,
    }
}

/// The block that asks for `ids`: `NumInfos`, then a `TW_INFO` for each with
/// its `InfoID` set and its `ReturnCode` `TWRC_INFONOTSUPPORTED`, so an entry
/// the source leaves alone doesn't read as an empty success.
pub(crate) fn request(ids: &[u16]) -> Result<Vec<u8>, ContainerError> {
    let count = u32::try_from(ids.len()).map_err(|_| ContainerError::TooMany(ids.len()))?;
    let len = ids
        .len()
        .checked_mul(ENTRY)
        .and_then(|n| n.checked_add(ENTRIES))
        .filter(|&n| u32::try_from(n).is_ok())
        .ok_or(ContainerError::TooMany(ids.len()))?;
    let mut block = vec![0; len];
    block[..4].copy_from_slice(&count.to_le_bytes());
    for (entry, id) in block[ENTRIES..]
        .as_chunks_mut::<ENTRY>()
        .0
        .iter_mut()
        .zip(ids)
    {
        entry[ID..ID + 2].copy_from_slice(&id.to_le_bytes());
        entry[RETURN_CODE..RETURN_CODE + 2]
            .copy_from_slice(&ffi::TWRC_INFONOTSUPPORTED.to_le_bytes());
    }
    Ok(block)
}

/// The `asked` entries of the answer in `block`.
pub(crate) fn parse(block: &[u8], asked: usize) -> Result<Vec<Entry>, ContainerError> {
    let needed = (asked as u64)
        .saturating_mul(ENTRY as u64)
        .saturating_add(ENTRIES as u64);
    if needed > block.len() as u64 {
        return Err(ContainerError::Short {
            len: block.len(),
            needed,
        });
    }
    let u16_at = |e: &[u8], at: usize| u16::from_le_bytes([e[at], e[at + 1]]);
    Ok(block[ENTRIES..]
        .as_chunks::<ENTRY>()
        .0
        .iter()
        .take(asked)
        .map(|e| {
            let mut item = [0; size_of::<usize>()];
            item.copy_from_slice(&e[ITEM..ITEM + size_of::<usize>()]);
            Entry {
                id: u16_at(e, ID),
                item_type: u16_at(e, ITEM_TYPE),
                num_items: u16_at(e, NUM_ITEMS),
                return_code: u16_at(e, RETURN_CODE),
                item: usize::from_le_bytes(item),
            }
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::container::tests::Random;

    #[test]
    fn a_request_names_each_id_and_parses_back() {
        let block = request(&[ffi::TWEI_DOCUMENTNUMBER, ffi::TWEI_PAGESIDE]).unwrap();
        assert_eq!(block.len(), 4 + 2 * ENTRY);
        assert_eq!(&block[..4], &[2, 0, 0, 0]);
        let entries = parse(&block, 2).unwrap();
        assert_eq!(entries[0].id, ffi::TWEI_DOCUMENTNUMBER);
        assert_eq!(entries[1].id, ffi::TWEI_PAGESIDE);
        assert_eq!(entries[1].item, 0);
    }

    #[test]
    fn an_answer_shorter_than_asked_is_refused() {
        let block = request(&[1, 2, 3]).unwrap();
        assert_eq!(
            parse(&block[..block.len() - 1], 3),
            Err(ContainerError::Short {
                len: block.len() - 1,
                needed: block.len() as u64
            })
        );
    }

    fn entry(item_type: u16, num_items: u16, item: usize) -> Entry {
        Entry {
            id: ffi::TWEI_BARCODECOUNT,
            item_type,
            num_items,
            return_code: 0,
            item,
        }
    }

    #[test]
    fn four_bytes_are_inline_and_more_are_behind_a_handle_in_either_bitness() {
        // One UINT32, inline; four UINT8, inline; five, behind a handle.
        assert_eq!(
            entry(ffi::TWTY_UINT32, 1, 2).data(),
            Data::Inline(vec![2, 0, 0, 0])
        );
        assert!(matches!(entry(ffi::TWTY_UINT8, 4, 7).data(), Data::Inline(b) if b.len() == 4));
        assert_eq!(
            entry(ffi::TWTY_UINT8, 5, 0x1234).data(),
            Data::Handle(0x1234)
        );
        // Two UINT32, the specification's two barcodes: behind a handle,
        // whatever the bitness (on 64-bit, Item could hold them, and doesn't).
        assert_eq!(
            entry(ffi::TWTY_UINT32, 2, 0x5678).data(),
            Data::Handle(0x5678)
        );
        // A string is wider than four bytes, and a TWTY_HANDLE item always
        // behind one; a null handle for items is missing, for none, none.
        assert_eq!(entry(ffi::TWTY_STR255, 1, 9).data(), Data::Handle(9));
        assert_eq!(entry(ffi::TWTY_HANDLE, 1, 9).data(), Data::Handle(9));
        assert_eq!(entry(ffi::TWTY_HANDLE, 2, 0).data(), Data::Missing);
        assert_eq!(entry(ffi::TWTY_UINT32, 0, 0).data(), Data::Inline(vec![]));
        // None for a code other than success, whatever Item holds; an
        // unknown type can't be told.
        let mut refused = entry(ffi::TWTY_UINT32, 1, 9);
        refused.return_code = ffi::TWRC_INFONOTSUPPORTED;
        assert_eq!(refused.data(), Data::None);
        assert_eq!(entry(0x77, 1, 9).data(), Data::Unknown);
    }

    #[test]
    fn answers_decode_into_items_or_say_why_not() {
        let decoded = decode(Answer {
            entry: entry(ffi::TWTY_UINT32, 1, 2),
            data: Read::Bytes(vec![2, 0, 0, 0]),
        });
        assert_eq!(decoded.id, ffi::TWEI_BARCODECOUNT);
        assert_eq!(
            decoded.value,
            ExtValue::Items(Items::new(ItemType::UInt32, vec![crate::Item::UInt32(2)]).unwrap())
        );
        let mut refused = entry(ffi::TWTY_UINT32, 0, 0);
        refused.return_code = ffi::TWRC_DATANOTAVAILABLE;
        assert_eq!(
            decode(Answer {
                entry: refused,
                data: Read::None
            })
            .value,
            ExtValue::Code(ReturnCode::DATANOTAVAILABLE)
        );
        let text = decode(Answer {
            entry: entry(ffi::TWTY_HANDLE, 1, 9),
            data: Read::Bytes(b"0123".to_vec()),
        });
        assert_eq!(text.value, ExtValue::Bytes(b"0123".to_vec()));
        let short = decode(Answer {
            entry: entry(ffi::TWTY_UINT32, 3, 9),
            data: Read::Bytes(vec![0; 11]),
        });
        assert!(matches!(
            short.value,
            ExtValue::Malformed(ContainerError::Short {
                len: 11,
                needed: 12
            })
        ));
    }

    #[test]
    fn random_answers_parse_or_are_refused_never_a_panic() {
        let mut r = Random::new(0x0e71_eaf0);
        let (mut inline, mut behind, mut decoded) = (0, 0, 0);
        for _ in 0..20_000 {
            let asked = r.below(30) as usize;
            let len = if r.below(4) == 0 {
                r.below(400) as usize
            } else {
                ENTRIES + asked * ENTRY
            };
            let mut block = r.bytes(len);
            // Most entries a success of a known type and a few items, so the
            // decoding is reached.
            if block.len() >= ENTRIES {
                for e in block[ENTRIES..].as_chunks_mut::<ENTRY>().0.iter_mut() {
                    if r.below(5) != 0 {
                        e[RETURN_CODE..RETURN_CODE + 2].copy_from_slice(&0u16.to_le_bytes());
                        let item_type = r.below(0x10) as u16;
                        e[ITEM_TYPE..ITEM_TYPE + 2].copy_from_slice(&item_type.to_le_bytes());
                        let count = r.below(8) as u16;
                        e[NUM_ITEMS..NUM_ITEMS + 2].copy_from_slice(&count.to_le_bytes());
                    }
                }
            }
            if let Ok(entries) = parse(&block, asked) {
                assert_eq!(entries.len(), asked);
                for entry in entries {
                    let data = match entry.data() {
                        Data::None => Read::None,
                        Data::Missing => Read::Missing,
                        Data::Unknown => Read::Unknown,
                        Data::Inline(bytes) => {
                            inline += 1;
                            Read::Bytes(bytes)
                        }
                        // A block of whatever length the handle's might be.
                        Data::Handle(_) => {
                            behind += 1;
                            let len = r.below(64) as usize;
                            Read::Bytes(r.bytes(len))
                        }
                    };
                    let value = decode(Answer { entry, data }).value;
                    if matches!(value, ExtValue::Items(_) | ExtValue::Bytes(_)) {
                        decoded += 1;
                    }
                }
            }
        }
        assert!(parse(&[], usize::MAX).is_err());
        // The input reaches the decoding, inline and behind a handle, often.
        assert!(
            inline > 10_000 && behind > 10_000 && decoded > 10_000,
            "{inline} inline, {behind} behind a handle, {decoded} decoded"
        );
    }
}
