pub const HEAP: u32 = 0x3804_3824;
pub const SLACK: u32 = 0x100;
pub const FB: u32 = 0;
pub const RES: u32 = 2;

pub fn fits(free: u32, blocks: impl IntoIterator<Item = u32>, size: u32, align: u32) -> bool {
    let Some(need) = size.checked_add(align).and_then(|n| n.checked_add(SLACK)) else {
        return false;
    };
    free >= need && blocks.into_iter().any(|b| b >= need)
}

#[cfg(target_os = "none")]
mod target {
    use super::*;
    use crate::{layout::stub_ram, stock::{UI_MEM_ALIGNED_ALLOC, UI_MEM_FREE}};
    use core::arch::asm;

    const MAX_STEPS: u32 = 4096;

    struct Basepri(u32);

    impl Basepri {
        fn raise() -> Self {
            let old: u32;
            unsafe {
                asm!("mrs {}, basepri", "msr basepri_max, {}", out(reg) old, in(reg) 0x20u32, options(nostack));
            }
            Basepri(old)
        }
    }

    impl Drop for Basepri {
        fn drop(&mut self) {
            unsafe { asm!("msr basepri, {}", in(reg) self.0, options(nostack)) }
        }
    }

    pub fn alloc(ty: u32, size: u32, align: u32) -> *mut u8 {
        let limit = stub_ram().alloc_limit;
        if limit != 0 && size > limit {
            return core::ptr::null_mut();
        }
        let _guard = Basepri::raise();
        let free = unsafe { *((HEAP + 8) as *const u32) };
        let mut node = unsafe { *((HEAP + 0x60) as *const u32) };
        let mut steps = 0;
        let mut sizes = core::iter::from_fn(|| {
            if node == 0 {
                return None;
            }
            steps += 1;
            if steps > MAX_STEPS {
                return None;
            }
            let s = unsafe { *((node + 4) as *const u32) } & 0xFF_FFFF;
            node = unsafe { *(node as *const u32) };
            Some(s)
        });
        let ok = fits(free, &mut sizes, size, align);
        drop(sizes);
        if steps > MAX_STEPS || !ok {
            return core::ptr::null_mut();
        }
        let f: extern "C" fn(u32, u32, u32, *const u8) -> *mut u8 =
            unsafe { core::mem::transmute(UI_MEM_ALIGNED_ALLOC as usize) };
        f(ty, align, size, b"wp2\0".as_ptr())
    }

    pub unsafe fn free(ty: u32, p: *mut u8) {
        if p.is_null() {
            return;
        }
        let f: extern "C" fn(u32, *mut u8) = unsafe { core::mem::transmute(UI_MEM_FREE as usize) };
        f(ty, p)
    }
}

#[cfg(target_os = "none")]
pub use target::{alloc, free};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heap_fit() {
        assert!(fits(0x2_0000, [0x100, 0x1_0000 + 8 + SLACK], 0x1_0000, 8));
        assert!(!fits(0x2_0000, [0x1_0000 + 8 + SLACK - 1], 0x1_0000, 8));
        assert!(!fits(0x1_0000, [0x2_0000], 0x1_0000, 8));
        assert!(!fits(u32::MAX, [u32::MAX], u32::MAX - 4, 8));
    }
}
