//! Raw memory access for finding the engine's tables: reads that check the memory is
//! there first, and the game executable's sections.

use std::cell::Cell;
use std::ffi::c_void;

use windows::Win32::System::Diagnostics::Debug::{IMAGE_NT_HEADERS64, IMAGE_SCN_MEM_EXECUTE, IMAGE_SCN_MEM_WRITE, IMAGE_SECTION_HEADER};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Memory::{VirtualQuery, MEMORY_BASIC_INFORMATION, MEM_COMMIT, PAGE_GUARD, PAGE_NOACCESS};
use windows::Win32::System::SystemServices::IMAGE_DOS_HEADER;

/// A section of the game's executable.
#[derive(Debug, Clone, Copy)]
pub struct Section {
    pub start: usize,
    pub len: usize,
    pub writable: bool,
    pub executable: bool,
}

impl Section {
    pub fn contains(&self, addr: usize) -> bool {
        addr >= self.start && addr < self.start + self.len
    }
}

/// The game's executable: where it's loaded and its sections.
#[derive(Debug, Clone)]
pub struct Image {
    pub base: usize,
    pub len: usize,
    pub sections: Vec<Section>,
}

impl Image {
    /// The process's main executable (Unreal games are one monolithic executable).
    pub fn main() -> Option<Self> {
        unsafe {
            let base = GetModuleHandleW(None).ok()?.0 as usize;
            let dos = &*(base as *const IMAGE_DOS_HEADER);
            let nt = &*((base + dos.e_lfanew as usize) as *const IMAGE_NT_HEADERS64);
            let len = nt.OptionalHeader.SizeOfImage as usize;
            let first = (&nt.OptionalHeader as *const _ as usize) + nt.FileHeader.SizeOfOptionalHeader as usize;
            let sections = (0..nt.FileHeader.NumberOfSections as usize)
                .map(|i| {
                    let s = &*((first + i * std::mem::size_of::<IMAGE_SECTION_HEADER>()) as *const IMAGE_SECTION_HEADER);
                    Section {
                        start: base + s.VirtualAddress as usize,
                        len: s.Misc.VirtualSize as usize,
                        writable: s.Characteristics.0 & IMAGE_SCN_MEM_WRITE.0 != 0,
                        executable: s.Characteristics.0 & IMAGE_SCN_MEM_EXECUTE.0 != 0,
                    }
                })
                .collect();
            Some(Self { base, len, sections })
        }
    }

    pub fn contains(&self, addr: usize) -> bool {
        addr >= self.base && addr < self.base + self.len
    }

    pub fn is_code(&self, addr: usize) -> bool {
        self.sections.iter().any(|s| s.executable && s.contains(addr))
    }

    pub fn data_sections(&self) -> impl Iterator<Item = &Section> {
        self.sections.iter().filter(|s| s.writable && !s.executable)
    }
}

/// Readable regions remembered per thread, so reads don't ask the system about every
/// pointer: objects are spread over many regions, and a walk over thousands of them would
/// otherwise query for each.
const CACHED_REGIONS: usize = 32;
/// How long a region is trusted without asking again. Memory the game frees goes back
/// to its allocator, which keeps it mapped, so a region rarely goes away; this bounds
/// how long a stale entry could be believed.
const REGION_TTL_MS: u64 = 250;

#[derive(Clone, Copy, Default)]
struct Region {
    start: usize,
    end: usize,
    seen_ms: u64,
}

thread_local! {
    static REGIONS: Cell<([Region; CACHED_REGIONS], usize)> = const { Cell::new(([Region { start: 0, end: 0, seen_ms: 0 }; CACHED_REGIONS], 0)) };
}

fn now_ms() -> u64 {
    unsafe { windows::Win32::System::SystemInformation::GetTickCount64() }
}

/// Whether `len` bytes at `addr` are committed, readable memory.
pub fn readable(addr: usize, len: usize) -> bool {
    if addr < 0x10000 || addr.checked_add(len).is_none() || addr >= 0x7FFF_FFFF_FFFF {
        return false;
    }
    let end = addr + len;
    let now = now_ms();
    let (mut regions, mut next) = REGIONS.with(Cell::get);
    if regions.iter().any(|r| addr >= r.start && end <= r.end && now.saturating_sub(r.seen_ms) <= REGION_TTL_MS) {
        return true;
    }
    let mut at = addr;
    while at < end {
        let mut info = MEMORY_BASIC_INFORMATION::default();
        let got = unsafe { VirtualQuery(Some(at as *const c_void), &mut info, std::mem::size_of::<MEMORY_BASIC_INFORMATION>()) };
        if got == 0 || info.State != MEM_COMMIT || info.Protect.0 & (PAGE_NOACCESS.0 | PAGE_GUARD.0) != 0 || info.Protect.0 == 0 {
            return false;
        }
        let region_end = info.BaseAddress as usize + info.RegionSize;
        regions[next] = Region { start: info.BaseAddress as usize, end: region_end, seen_ms: now };
        next = (next + 1) % CACHED_REGIONS;
        at = region_end;
    }
    REGIONS.with(|r| r.set((regions, next)));
    true
}

/// Reads a `T` at `addr` if the memory is there.
pub fn try_read<T: Copy>(addr: usize) -> Option<T> {
    readable(addr, std::mem::size_of::<T>()).then(|| unsafe { read(addr) })
}

/// Reads a `T` at `addr`.
///
/// # Safety
/// `addr` must point to `size_of::<T>()` readable bytes.
pub unsafe fn read<T: Copy>(addr: usize) -> T {
    std::ptr::read_unaligned(addr as *const T)
}

/// A pointer at `addr`, if both it and what it points to are readable.
pub fn try_ptr(addr: usize) -> Option<usize> {
    try_read::<usize>(addr).filter(|&p| readable(p, 8))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checks_memory_before_reading() {
        let value = 0x1234_5678_u32;
        assert_eq!(try_read::<u32>(&value as *const u32 as usize), Some(0x1234_5678));
        assert_eq!(try_read::<u32>(0x10), None);
        assert_eq!(try_read::<u32>(usize::MAX - 2), None);
    }

    #[test]
    fn reads_the_test_executable() {
        let image = Image::main().expect("main image");
        assert!(image.sections.iter().any(|s| s.executable));
        assert!(image.data_sections().count() > 0);
        assert!(image.is_code(reads_the_test_executable as *const () as usize));
    }
}
