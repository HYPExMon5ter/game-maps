//! The engine's name table (`FNamePool`): every `FName` is an index into it.
//!
//! The pool is a static in the executable's data. It's found by its first two entries,
//! which are always `None` and `ByteProperty`, so no signature that a game update could
//! move is needed.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::mem::{self, Image};

/// An `FName` as stored in objects: an entry in the pool, plus a number suffix.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct FName {
    pub index: u32,
    pub number: u32,
}

/// Blocks hold this many 2-byte units (`FNameBlockOffsetBits`).
const BLOCK_OFFSET_BITS: u32 = 16;
const STRIDE: usize = 2;
const MAX_BLOCKS: u32 = 8192;
/// `FNameEntryAllocator`: lock, current block, byte cursor, then the block pointers.
const BLOCKS_OFFSET: usize = 0x10;
const MAX_LEN: usize = 1024;

pub struct Names {
    /// Address of `Blocks[0]` in the pool.
    blocks: usize,
    cache: Mutex<HashMap<u32, Arc<str>>>,
}

/// An entry's header: bit 0 wide, bits 1-5 hash, bits 6-15 length.
fn header(h: u16) -> (bool, usize) {
    (h & 1 != 0, (h >> 6) as usize)
}

/// Whether `entry` is an ANSI name entry spelling `name`.
fn entry_is(entry: usize, name: &str) -> bool {
    let Some(h) = mem::try_read::<u16>(entry) else { return false };
    let (wide, len) = header(h);
    !wide && len == name.len() && mem::readable(entry + 2, len) && unsafe { std::slice::from_raw_parts((entry + 2) as *const u8, len) } == name.as_bytes()
}

impl Names {
    /// Finds the pool in the executable's data.
    pub fn find(image: &Image) -> Option<Self> {
        for section in image.data_sections() {
            let mut at = section.start;
            while at + BLOCKS_OFFSET + 8 <= section.start + section.len {
                if let Some(names) = Self::at(at) {
                    return Some(names);
                }
                at += 8;
            }
        }
        None
    }

    fn at(pool: usize) -> Option<Self> {
        let block0 = mem::try_read::<usize>(pool + BLOCKS_OFFSET)?;
        if !entry_is(block0, "None") {
            return None;
        }
        // "None" is 2 + 4 bytes: the next entry starts right after it.
        if !entry_is(block0 + 6, "ByteProperty") {
            return None;
        }
        let current_block = mem::try_read::<u32>(pool + 8)?;
        (current_block < MAX_BLOCKS).then(|| Self { blocks: pool + BLOCKS_OFFSET, cache: Mutex::new(HashMap::new()) })
    }

    /// The text of pool entry `index`.
    pub fn entry(&self, index: u32) -> Option<Arc<str>> {
        if let Some(s) = self.cache.lock().unwrap().get(&index) {
            return Some(s.clone());
        }
        let block = index >> BLOCK_OFFSET_BITS;
        let offset = (index & ((1 << BLOCK_OFFSET_BITS) - 1)) as usize * STRIDE;
        if block >= MAX_BLOCKS {
            return None;
        }
        let base = mem::try_read::<usize>(self.blocks + block as usize * 8).filter(|&b| b != 0)?;
        let entry = base + offset;
        let (wide, len) = header(mem::try_read::<u16>(entry)?);
        if len > MAX_LEN {
            return None;
        }
        let text: Arc<str> = if wide {
            if !mem::readable(entry + 2, len * 2) {
                return None;
            }
            let units = unsafe { std::slice::from_raw_parts((entry + 2) as *const u16, len) };
            String::from_utf16_lossy(units).into()
        } else {
            if !mem::readable(entry + 2, len) {
                return None;
            }
            let bytes = unsafe { std::slice::from_raw_parts((entry + 2) as *const u8, len) };
            bytes.iter().map(|&b| b as char).collect::<String>().into()
        };
        self.cache.lock().unwrap().insert(index, text.clone());
        Some(text)
    }

    /// The name's text, with its number as UE writes it (`Name_3` for number 4).
    pub fn text(&self, name: FName) -> String {
        let base = self.entry(name.index).map(|s| s.to_string()).unwrap_or_default();
        if name.number == 0 {
            base
        } else {
            format!("{base}_{}", name.number - 1)
        }
    }

    /// Whether `name` is `text` without a number.
    pub fn is(&self, name: FName, text: &str) -> bool {
        name.number == 0 && self.entry(name.index).is_some_and(|s| &*s == text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A pool with entries laid out as the engine does.
    fn pool(entries: &[&str]) -> (Vec<u8>, Vec<usize>) {
        let mut block = Vec::new();
        for e in entries {
            let h = ((e.len() as u16) << 6) | (0b10110 << 1);
            block.extend_from_slice(&h.to_le_bytes());
            block.extend_from_slice(e.as_bytes());
            if block.len() % 2 == 1 {
                block.push(0);
            }
        }
        let allocator = vec![0usize, 0, block.as_ptr() as usize];
        (block, allocator)
    }

    #[test]
    fn finds_a_pool_by_its_first_entries() {
        let (block, allocator) = pool(&["None", "ByteProperty", "Actor"]);
        let names = Names::at(allocator.as_ptr() as usize).expect("pool");
        assert_eq!(&*names.entry(0).unwrap(), "None");
        assert_eq!(&*names.entry(3).unwrap(), "ByteProperty");
        // "ByteProperty" is 2 + 12 bytes: 7 units after it.
        assert_eq!(names.text(FName { index: 10, number: 0 }), "Actor");
        assert_eq!(names.text(FName { index: 10, number: 3 }), "Actor_2");
        assert!(names.is(FName { index: 10, number: 0 }, "Actor"));
        drop(block);
    }

    #[test]
    fn rejects_other_data() {
        let (block, allocator) = pool(&["Nope", "ByteProperty"]);
        assert!(Names::at(allocator.as_ptr() as usize).is_none());
        drop(block);
    }
}
