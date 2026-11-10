#![no_std]
#![no_main]

use core::ptr::{addr_of_mut, write_volatile};

use cmf_wp2_app::Api;

cmf_wp2_app::entry!(main);

// last vibrate() result, read by MotorTest
#[unsafe(no_mangle)]
static mut RC: i32 = 0;

// never reads events: re-switches the motor every 100 ms and ignores the button
fn main(api: &'static Api) -> i32 {
    let mut at = api.now().wrapping_sub(100);
    // no spin_loop: QEMU leaves its CPU loop on every YIELD and guest time nearly stops (task 8 e2e)
    loop {
        if api.now().wrapping_sub(at) >= 100 {
            at = api.now();
            unsafe { write_volatile(addr_of_mut!(RC), api.vibrate(1000, 31)) };
        }
    }
}
