//! `DAT_EXTIMAGEINFO`'s block: the request written, and the source's answer
//! read, every entry's bounds checked. Pure; reading the memory an entry's
//! handle points to is the DSM module's.

use core::mem::{offset_of, size_of};

use crate::container::ContainerError;
use crate::ffi;

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

/// The block that asks for `ids`: `NumInfos`, then a `TW_INFO` for each with
/// only its `InfoID` set.
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

    #[test]
    fn random_answers_parse_or_are_refused_never_a_panic() {
        let mut r = Random::new(0x0e71_eaf0);
        for _ in 0..20_000 {
            let len = r.below(400) as usize;
            let block = r.bytes(len);
            let asked = r.below(30) as usize;
            if let Ok(entries) = parse(&block, asked) {
                assert_eq!(entries.len(), asked);
            }
        }
        assert!(parse(&[], usize::MAX).is_err());
    }
}
