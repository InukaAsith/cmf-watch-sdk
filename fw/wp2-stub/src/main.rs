#![no_std]
#![no_main]

mod ble;
mod view;

use core::ffi::c_void;
use wp2_core::layout::{stub_ram, StubRam};

#[repr(C)]
pub struct StubHeader {
    magic: [u8; 4],
    version: u32,
    boot: unsafe extern "C" fn(),
    ble: unsafe extern "C" fn(),
    view_proc: extern "C" fn(u32, u32, *mut c_void) -> i32,
    view_name: *const u8,
    _r: [u32; 2],
}

unsafe impl Sync for StubHeader {}

#[unsafe(link_section = ".header")]
#[used]
static HEADER: StubHeader = StubHeader {
    magic: *b"WP2S",
    version: 1,
    boot: wp2_boot,
    ble: ble::wp2_ble_default,
    view_proc: view::wp2_view_proc,
    view_name: b"wp2_view\0".as_ptr(),
    _r: [0; 2],
};

// Replaces `bl 0x10146AA4` (ResFbHeap init, 4 of its 8 arguments on the stack) at 0x101884FA.
#[unsafe(naked)]
#[unsafe(no_mangle)]
unsafe extern "C" fn wp2_boot() {
    core::arch::naked_asm!(
        "push {{r0-r4, lr}}",
        "bl {init}",
        "pop {{r0-r4, lr}}",
        "b.w stock_heap_init",
        init = sym wp2_boot_c,
    )
}

extern "C" fn wp2_boot_c() {
    let ram = stub_ram();
    unsafe { core::ptr::write_bytes(ram as *mut StubRam, 0, 1) };
    ram.magic = 0x5750_3253;
    ram.running_slot = 0xFF;
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    loop {}
}
