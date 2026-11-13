//! The engine's object table (`GUObjectArray`): every live `UObject`, by index.
//!
//! Like the name pool, it's found by shape rather than by signature: a chunked array in
//! the executable's data whose counts agree with each other, and whose objects know their
//! own index.

use crate::mem::{self, Image};

/// Objects per chunk (`NumElementsPerChunk`).
const CHUNK: usize = 64 * 1024;
/// `UObjectBase::InternalIndex`.
pub const OBJECT_INDEX: usize = 0xC;

pub struct Objects {
    /// Address of `FChunkedFixedUObjectArray` (`GUObjectArray.ObjObjects`).
    array: usize,
    /// Size of an `FUObjectItem`.
    stride: usize,
    /// Where an item keeps its object: first, or after a 64-bit flags and reference
    /// count (UE 5.5 and later).
    object_at: usize,
}

fn ceil_div(a: usize, b: usize) -> usize {
    a.div_ceil(b)
}

impl Objects {
    pub fn find(image: &Image) -> Option<Self> {
        for section in image.data_sections() {
            let mut at = section.start;
            while at + 0x20 <= section.start + section.len {
                if let Some(objects) = Self::at(at) {
                    return Some(objects);
                }
                at += 8;
            }
        }
        None
    }

    fn at(array: usize) -> Option<Self> {
        let chunks = mem::try_read::<usize>(array)?;
        let [max, num, max_chunks, num_chunks] = mem::try_read::<[i32; 4]>(array + 0x10)?;
        if num < 0x1000 || num > max || max > 0x1000_0000 || max_chunks <= 0 || num_chunks <= 0 {
            return None;
        }
        let (max, num, max_chunks, num_chunks) = (max as usize, num as usize, max_chunks as usize, num_chunks as usize);
        // Chunks may be allocated ahead of use.
        if num_chunks < ceil_div(num, CHUNK) || num_chunks > max_chunks || !(max_chunks == ceil_div(max, CHUNK) || max_chunks == max / CHUNK + 1) {
            return None;
        }
        if !mem::readable(chunks, num_chunks * 8) {
            return None;
        }
        let first = mem::try_ptr(chunks)?;
        [(24, 0), (24, 8), (16, 0), (32, 0), (32, 8)]
            .into_iter()
            .find(|&(stride, object_at)| items_know_their_index(first, stride, object_at))
            .map(|(stride, object_at)| Self { array, stride, object_at })
    }

    /// How many slots are in use (some may be empty).
    pub fn len(&self) -> usize {
        mem::try_read::<i32>(self.array + 0x14).unwrap_or(0).max(0) as usize
    }

    /// The `FUObjectItem` for slot `index`.
    fn item(&self, index: usize) -> Option<usize> {
        if index >= self.len() {
            return None;
        }
        let chunks = mem::try_read::<usize>(self.array)?;
        let chunk = mem::try_read::<usize>(chunks + index / CHUNK * 8).filter(|&c| c != 0)?;
        Some(chunk + index % CHUNK * self.stride)
    }

    /// The object in slot `index`, if any.
    pub fn get(&self, index: usize) -> Option<usize> {
        let item = self.item(index)?;
        mem::try_read::<usize>(item + self.object_at).filter(|&o| o != 0)
    }
}

/// Whether the objects in the first slots of `chunk` report those slots as their index.
fn items_know_their_index(chunk: usize, stride: usize, object_at: usize) -> bool {
    let mut seen = 0;
    for i in 0..64 {
        let Some(obj) = mem::try_read::<usize>(chunk + i * stride + object_at) else { return false };
        if obj == 0 {
            continue;
        }
        if mem::try_read::<i32>(obj + OBJECT_INDEX) != Some(i as i32) {
            return false;
        }
        seen += 1;
    }
    seen >= 32
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Fake objects that know their index, in one chunk of `stride`-byte items.
    fn table(count: usize, stride: usize, object_at: usize) -> (Vec<[u8; 0x28]>, Vec<u8>, Vec<usize>, Vec<usize>) {
        let mut objects = vec![[0u8; 0x28]; count];
        for (i, o) in objects.iter_mut().enumerate() {
            o[OBJECT_INDEX..OBJECT_INDEX + 4].copy_from_slice(&(i as i32).to_le_bytes());
        }
        let mut items = vec![0u8; count * stride];
        for (i, o) in objects.iter().enumerate() {
            let at = i * stride + object_at;
            items[at..at + 8].copy_from_slice(&(o.as_ptr() as usize).to_le_bytes());
        }
        let chunks = vec![items.as_ptr() as usize];
        let max = 0x20000usize;
        // Two chunks allocated, though one is in use.
        let header = vec![chunks.as_ptr() as usize, 0, max | (count << 32), (max / CHUNK + 1) | (2 << 32)];
        (objects, items, chunks, header)
    }

    #[test]
    fn finds_the_table_and_its_item_size() {
        for (stride, object_at) in [(16, 0), (24, 0), (24, 8), (32, 8)] {
            let (objects, items, chunks, header) = table(0x1200, stride, object_at);
            let found = Objects::at(header.as_ptr() as usize).expect("table");
            assert_eq!((found.stride, found.object_at), (stride, object_at));
            assert_eq!(found.len(), 0x1200);
            assert_eq!(found.get(0x11FF), Some(objects[0x11FF].as_ptr() as usize));
            assert_eq!(found.get(0x1200), None);
            drop((items, chunks));
        }
    }

    #[test]
    fn rejects_counts_that_disagree() {
        let (_objects, _items, _chunks, mut header) = table(0x1200, 24, 0);
        header[3] = 7 | (1 << 32);
        assert!(Objects::at(header.as_ptr() as usize).is_none());
    }
}
