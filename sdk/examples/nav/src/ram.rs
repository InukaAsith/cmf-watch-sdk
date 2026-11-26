//! The street map's drawing buffers (Maps' render.rs keeps them here: `crate::ram::ram().draw`).

use crate::render::Buffers;

pub struct Ram {
    pub draw: Buffers,
}

static mut RAM: core::mem::MaybeUninit<Ram> = core::mem::MaybeUninit::zeroed();

pub fn ram() -> &'static mut Ram {
    // all zeros is a valid Buffers (numbers and points only)
    unsafe { (*core::ptr::addr_of_mut!(RAM)).assume_init_mut() }
}
