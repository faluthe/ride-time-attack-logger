//! Unreal FNamePool lookups, used to check that pointers land on the classes we expect.

use crate::offsets::DATA;
use crate::process::{is_ptr, Memory};

/// Location of `FNamePool::Blocks[]`.
pub struct Names {
    blocks: u64,
}

impl Names {
    /// Scan `.data` for the block table: block 0 starts with the entries "None", "ByteProperty".
    pub fn find(m: &impl Memory) -> Option<Self> {
        let mut data = vec![0u8; (DATA.1 - DATA.0) as usize];
        if !m.read(DATA.0, &mut data) {
            return None;
        }
        let mut head = [0u8; 40];
        for (i, c) in data.chunks_exact(8).enumerate() {
            let p = u64::from_le_bytes(c.try_into().unwrap());
            if is_ptr(p) && m.read(p, &mut head) && &head[2..6] == b"None"
                && head.windows(12).any(|w| w == b"ByteProperty")
            {
                return Some(Self { blocks: DATA.0 + 8 * i as u64 });
            }
        }
        None
    }

    /// Resolve an `FName` comparison index (block << 16 | offset / 2).
    pub fn get(&self, m: &impl Memory, index: i32) -> Option<String> {
        let index = index as u32;
        let base = m.u64(self.blocks + 8 * (index >> 16) as u64)?;
        let entry = base + 2 * (index & 0xffff) as u64;
        let header = m.u16(entry)?;
        let (wide, len) = (header & 1 == 1, (header >> 6) as usize);
        let mut raw = vec![0u8; if wide { 2 * len } else { len }];
        if !m.read(entry + 2, &mut raw) {
            return None;
        }
        Some(if wide {
            String::from_utf16_lossy(&raw.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect::<Vec<_>>())
        } else {
            raw.iter().map(|&b| b as char).collect()
        })
    }
}
