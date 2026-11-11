use crate::fs::{self, File};
use crate::heap::{self, RES};
use crate::layout::{slot_path, E_BAD, E_IO, E_NOMEM};
use crate::pkg::{check_slot, crc32, parse_header, relocate, Header, HEADER_SIZE};
use crate::stock::PSRAM_CACHE_FLUSH_INVALIDATE;

// Reads the package of `slot` into a fresh RES heap block and relocates it there.
pub fn load(slot: u8, file: &mut File, scratch: &mut [u8]) -> Result<(*mut u8, Header), i32> {
    let path = slot_path(slot, false);
    if fs::open(file, &path, fs::READ) < 0 {
        return Err(E_IO);
    }
    let result = load_open(slot, file, scratch);
    fs::close(file);
    result
}

fn load_open(slot: u8, file: &mut File, scratch: &mut [u8]) -> Result<(*mut u8, Header), i32> {
    let raw: &mut [u8; HEADER_SIZE] = (&mut scratch[..HEADER_SIZE]).try_into().map_err(|_| E_BAD)?;
    if !fs::read_exact(file, raw) {
        return Err(E_IO);
    }
    let header = parse_header(raw).map_err(|_| E_BAD)?;
    check_slot(&header, slot).map_err(|_| E_BAD)?;
    let block = heap::alloc(RES, header.image_size, 8);
    if block.is_null() {
        return Err(E_NOMEM);
    }
    let image = unsafe { core::slice::from_raw_parts_mut(block, header.image_size as usize) };
    match fill(file, image, scratch, &header) {
        Ok(()) => {
            sync_psram(block, header.image_size);
            Ok((block, header))
        }
        Err(e) => {
            unsafe { heap::free(RES, block) };
            Err(e)
        }
    }
}

// Code or pixels written through the cached 0x38… PSRAM mapping must reach PSRAM before they are
// fetched or read by display DMA (docs/recon.md, "Hardware: caches and MPU"). Not from an ISR.
pub fn sync_psram(block: *mut u8, len: u32) {
    let f: extern "C" fn(*mut u8, u32) -> i32 =
        unsafe { core::mem::transmute(PSRAM_CACHE_FLUSH_INVALIDATE as usize) };
    unsafe { core::arch::asm!("dsb sy", options(nostack)) };
    f(block, len);
    unsafe { core::arch::asm!("dsb sy", "isb sy", options(nostack)) };
}

fn fill(file: &mut File, image: &mut [u8], scratch: &mut [u8], header: &Header) -> Result<(), i32> {
    if !fs::read_exact(file, image) {
        return Err(E_IO);
    }
    let mut crc = crc32(0, image);
    let base = image.as_ptr() as u32;
    let step = scratch.len() & !3;
    let mut left = 4 * header.reloc_count as usize;
    let mut next = 0;
    while left > 0 {
        let chunk = &mut scratch[..left.min(step)];
        if !fs::read_exact(file, chunk) {
            return Err(E_IO);
        }
        crc = crc32(crc, chunk);
        relocate(image, base, chunk, &mut next).map_err(|_| E_BAD)?;
        left -= chunk.len();
    }
    if crc != header.body_crc { Err(E_BAD) } else { Ok(()) }
}
