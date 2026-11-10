pub const VIEW_ID: u16 = 0xF0;
pub const SLOTS: u8 = 8;
pub const LOADER_SLOT: u8 = 0xFF;
pub const LOADER_ABI_MINOR: u16 = 0;
pub const STUB_RAM: u32 = 0x3871_EEC0;
pub const STUB_RAM_SIZE: u32 = 0x1000;
pub const LOADER_SIZE: u32 = 0x1_0000;
pub const APP_SIZE: u32 = 0x2_0000;
pub const E_BAD: i32 = -2;
pub const E_IO: i32 = -5;
pub const E_NOMEM: i32 = -12;

// /NAND:A..K are read-only mounts; /NAND: is the writable FAT (docs/recon.md)
pub const DIR: &[u8] = b"/NAND:/wp2apps\0";

pub fn slot_path(slot: u8, tmp: bool) -> [u8; 24] {
    let mut path = [0u8; 24];
    let name = if slot == LOADER_SLOT { b'L' } else { b'0' + slot % 10 };
    let ext: &[u8] = if tmp { b"tmp" } else { b"pkg" };
    let dir = &DIR[..DIR.len() - 1];
    path[..dir.len()].copy_from_slice(dir);
    path[dir.len()] = b'/';
    path[dir.len() + 1] = name;
    path[dir.len() + 2] = b'.';
    path[dir.len() + 3..dir.len() + 6].copy_from_slice(ext);
    path
}

// loader_state: 0 none, 1 loaded, 2 missing or corrupt, 3 out of memory
#[repr(C)]
pub struct StubRam {
    pub magic: u32,
    pub loader_state: u8,
    pub loader_active: u8,
    pub running_slot: u8,
    pub xfer_state: u8,
    pub xfer_slot: u8,
    pub _pad: [u8; 3],
    pub xfer_size: u32,
    pub xfer_crc: u32,
    pub xfer_written: u32,
    pub file: [u8; 0x2C],
    pub trace_head: u32,
    pub trace: [u32; 32],
    pub scratch: [u8; 0x200],
    pub reply: [u8; 0x220],
    pub loader_block: u32,
    pub loader_size: u32,
    pub app_block: u32,
    pub app_size: u32,
    pub alloc_limit: u32,
    pub loader_entry: u32,
}

const _: () = assert!(core::mem::size_of::<StubRam>() <= 0xE00);

#[cfg(target_os = "none")]
pub fn stub_ram() -> &'static mut StubRam {
    unsafe { &mut *(STUB_RAM as *mut StubRam) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::mem::{offset_of, size_of};

    #[test]
    fn stub_ram_offsets() {
        assert_eq!(offset_of!(StubRam, trace_head), 0x44);
        assert_eq!(offset_of!(StubRam, loader_block), 0x4E8);
        assert_eq!(offset_of!(StubRam, app_block), 0x4F0);
        assert_eq!(offset_of!(StubRam, alloc_limit), 0x4F8);
        assert_eq!(offset_of!(StubRam, loader_entry), 0x4FC);
        assert!(size_of::<StubRam>() <= 0xE00);
    }

    #[test]
    fn slot_paths() {
        assert_eq!(&slot_path(LOADER_SLOT, false)[..21], b"/NAND:/wp2apps/L.pkg\0");
        assert_eq!(&slot_path(3, true)[..21], b"/NAND:/wp2apps/3.tmp\0");
        assert!(slot_path(3, true)[21..].iter().all(|&b| b == 0));
        assert_eq!(&DIR[..], b"/NAND:/wp2apps\0");
    }
}
