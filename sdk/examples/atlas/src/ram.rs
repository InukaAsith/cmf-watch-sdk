//! Maps' big buffers in the icon's memory. The loader shows its own copy of an app's icon in the menu
//! (docs/app-icons.md, "Rendering"), so once the app runs, the 27 KB of icon pixels at the front of the image are
//! RAM nobody reads. Keeping the state and drawing buffers there instead of in `.bss` (which is written into the
//! image as zeros) frees that much of the 128 KiB image; the icon in the package and on the menu is unchanged.
//! Every launch reloads the image from NAND, so the icon is whole again next time.

use crate::proto::{Model, Way};
use crate::render::Buffers;
use crate::state::Nav;

#[repr(C)]
pub struct Ram {
    pub model: Model,
    pub nav: Nav,
    pub draw: Buffers,
    /// voice typing's sound on its way (not on the 8 KB stack)
    pub dict: cmf_wp2_ui::dictate::Dictate,
}

/// The icon's pixels: 96 × 96 × 3 after the 16-byte descriptor.
const ROOM: usize = 96 * 96 * 3;
const _: () = assert!(core::mem::size_of::<Ram>() + 8 <= ROOM, "Maps' buffers outgrow the icon");

#[cfg(target_os = "none")]
fn base() -> *mut u8 {
    unsafe { (core::ptr::addr_of_mut!(crate::ui::WP2_ICON) as *mut u8).add(16) }
}

/// The simulator has no icon in front of its code: a plain buffer.
#[cfg(not(target_os = "none"))]
fn base() -> *mut u8 {
    static mut SIM: [u64; ROOM / 8] = [0; ROOM / 8];
    core::ptr::addr_of_mut!(SIM) as *mut u8
}

fn ptr() -> *mut Ram {
    let b = base();
    unsafe { b.add(b.align_offset(core::mem::align_of::<Ram>())) as *mut Ram }
}

/// Once, before anything else: the buffers as `Model::new()`, `Nav::new()` and zeros.
pub fn init() {
    let p = ptr();
    unsafe {
        core::ptr::write_bytes(p as *mut u8, 0, core::mem::size_of::<Ram>());
        let m = core::ptr::addr_of_mut!((*p).model);
        core::ptr::addr_of_mut!((*m).info_for).write(usize::MAX);
        core::ptr::addr_of_mut!((*m).alt_unit).write(1);
        core::ptr::addr_of_mut!((*m).ways).write([Way::Asking, Way::Asking, Way::Asking]);
        core::ptr::addr_of_mut!((*p).dict).write(cmf_wp2_ui::dictate::Dictate::new());
    }
}

pub fn ram() -> &'static mut Ram {
    unsafe { &mut *ptr() }
}
