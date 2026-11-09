//! A PE file's machine, read from its first bytes: pure, so a file is
//! checked before it's loaded.

/// `IMAGE_FILE_MACHINE_I386`.
pub(crate) const I386: u16 = 0x014c;
/// `IMAGE_FILE_MACHINE_AMD64`.
pub(crate) const AMD64: u16 = 0x8664;
/// `IMAGE_FILE_MACHINE_ARM64`.
pub(crate) const ARM64: u16 = 0xaa64;

/// The machine this process runs as: the one a DLL it loads must be built
/// for.
pub(crate) const fn this_machine() -> u16 {
    if cfg!(target_arch = "x86") {
        I386
    } else if cfg!(target_arch = "aarch64") {
        ARM64
    } else {
        AMD64
    }
}

/// The `Machine` field of the PE header in `head`, the file's first bytes:
/// `None` unless they hold a DOS header, a PE signature where it points, and
/// the field after it.
pub(crate) fn machine(head: &[u8]) -> Option<u16> {
    let u16_at = |at: usize| Some(u16::from_le_bytes([*head.get(at)?, *head.get(at + 1)?]));
    let u32_at = |at: usize| {
        let b = head.get(at..at.checked_add(4)?)?;
        Some(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    };
    if u16_at(0)? != 0x5a4d {
        return None; // "MZ"
    }
    let nt = usize::try_from(u32_at(0x3c)?).ok()?;
    if u32_at(nt)? != 0x0000_4550 {
        return None; // "PE\0\0"
    }
    u16_at(nt.checked_add(4)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::container::tests::Random;

    fn header(machine: u16) -> Vec<u8> {
        let mut head = vec![0; 0x100];
        head[..2].copy_from_slice(b"MZ");
        head[0x3c..0x40].copy_from_slice(&0x80u32.to_le_bytes());
        head[0x80..0x84].copy_from_slice(b"PE\0\0");
        head[0x84..0x86].copy_from_slice(&machine.to_le_bytes());
        head
    }

    #[test]
    fn the_machine_is_read_after_the_pe_signature() {
        assert_eq!(machine(&header(AMD64)), Some(AMD64));
        assert_eq!(machine(&header(I386)), Some(I386));
        let mut bad = header(AMD64);
        bad[0x80] = b'X';
        assert_eq!(machine(&bad), None);
        assert_eq!(machine(&header(AMD64)[..0x85]), None);
        let mut far = header(AMD64);
        far[0x3c..0x40].copy_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(machine(&far), None);
    }

    #[test]
    fn random_heads_are_read_or_refused_never_a_panic() {
        let mut r = Random::new(0x09e0_57a7);
        for _ in 0..20_000 {
            let len = r.below(300) as usize;
            let mut head = r.bytes(len);
            if head.len() >= 0x40 && r.below(2) == 0 {
                head[..2].copy_from_slice(b"MZ");
                let at = r.below(320) as u32;
                head[0x3c..0x40].copy_from_slice(&at.to_le_bytes());
            }
            let _ = machine(&head);
        }
    }
}
