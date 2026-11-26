//! The big buffers in the icon's memory (as Maps, atlas ram.rs): once the app runs the loader shows its own copy of
//! the icon, so the 27 KB of icon pixels at the front of the image are free RAM. Every launch reloads the image, so
//! nothing here outlives the app (the track and the route go when it closes).

use crate::geo::Track;
use crate::render::Buffers;

pub const TRACK: usize = 1500;
pub const ROUTE: usize = 1500;
pub const STEPS: usize = 64;

/// A turn of the route (trip.rs): nav state.rs kind and direction, where (frame metres), its point on the path.
#[derive(Clone, Copy)]
pub struct Step {
    pub kind: u8,
    pub dir: u8,
    pub p: (i16, i16),
    pub idx: u16,
    pub name: [u8; 32],
    pub n: u8,
}

#[repr(C)]
pub struct Ram {
    pub draw: Buffers,
    pub track: Track<TRACK>,
    /// the path followed: a route from the phone, or the track backwards
    pub path: [(i16, i16); ROUTE],
    pub path_n: usize,
    pub steps: [Step; STEPS],
    /// turn by turn, as Navigation's (all zero is its empty state)
    pub nav: crate::state::Nav,
}

const ROOM: usize = 96 * 96 * 3;
const _: () = assert!(core::mem::size_of::<Ram>() + 8 <= ROOM, "Offline Maps' buffers outgrow the icon");

#[cfg(target_os = "none")]
fn base() -> *mut u8 {
    unsafe { (core::ptr::addr_of_mut!(crate::ui::WP2_ICON) as *mut u8).add(16) }
}

#[cfg(not(target_os = "none"))]
fn base() -> *mut u8 {
    static mut SIM: [u64; ROOM / 8] = [0; ROOM / 8];
    core::ptr::addr_of_mut!(SIM) as *mut u8
}

fn ptr() -> *mut Ram {
    let b = base();
    unsafe { b.add(b.align_offset(core::mem::align_of::<Ram>())) as *mut Ram }
}

pub fn init() {
    unsafe { core::ptr::write_bytes(ptr() as *mut u8, 0, core::mem::size_of::<Ram>()) };
    ram().track.clear();
}

pub fn ram() -> &'static mut Ram {
    unsafe { &mut *ptr() }
}
