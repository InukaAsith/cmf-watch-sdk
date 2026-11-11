#![no_std]
#![no_main]

mod applist;
mod ble;
mod view;

use core::ffi::c_void;
use wp2_core::layout::{stub_ram, StubRam};
use wp2_core::pwm::SYSTEM_SCALE_Q10;

#[repr(C)]
pub struct StubHeader {
    magic: [u8; 4],
    version: u32,
    boot: unsafe extern "C" fn(),
    ble: unsafe extern "C" fn(),
    view_proc: extern "C" fn(u32, u32, *mut c_void) -> i32,
    view_name: *const u8,
    applist_layout: extern "C" fn(*mut u8) -> i32,
    motor: unsafe extern "C" fn(),
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
    applist_layout: applist::wp2_applist_layout,
    motor: wp2_motor,
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

// Replaces `bl 0x100FB4B4` at 0x100FB398, the only stock call that switches the motor on
// (r0 dev, r1 ch, r2 period 0xFF, r3 pulse = strength, flags on the stack). Scales the pulse and tail-calls stock.
#[unsafe(naked)]
#[unsafe(no_mangle)]
unsafe extern "C" fn wp2_motor() {
    core::arch::naked_asm!(
        "movw r12, #{k}",
        "mul r3, r3, r12",
        "lsrs r3, r3, #10",
        "b.w stock_motor",
        k = const SYSTEM_SCALE_Q10,
    )
}

extern "C" fn wp2_boot_c() {
    let ram = stub_ram();
    unsafe { core::ptr::write_bytes(ram as *mut StubRam, 0, 1) };
    ram.magic = 0x5750_3253;
    ram.running_slot = 0xFF;
    ram.api = 1;
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    // a fault goes through the Zephyr fatal handler and reboots the watch instead of hanging a stock thread
    unsafe { core::arch::asm!("udf #0", options(noreturn)) }
}
