//! ABI 1.3 test app for tests/test_back_e2e.py (OwnBackTest): owns "back", counts BUTTONs and exits on the third;
//! a crown detent makes it stop reading events (hung), so the loader's escape must still stop it.
#![no_std]
#![no_main]

use cmf_wp2_app::{Api, BUTTON, CROWN};

cmf_wp2_app::entry!(main);

// BUTTONs read; non-zero initialiser: in .data, fresh at every launch
#[unsafe(no_mangle)]
static mut PRESSES: i32 = -1;

fn main(api: &'static Api) -> i32 {
    api.own_back(true);
    unsafe { (&raw mut PRESSES).write_volatile(0) };
    loop {
        match api.event(u32::MAX).map(|e| e.kind) {
            Some(BUTTON) => unsafe {
                let n = (&raw const PRESSES).read_volatile() + 1;
                (&raw mut PRESSES).write_volatile(n);
                if n == 3 {
                    return 0;
                }
            },
            // no spin_loop: QEMU leaves its CPU loop on every YIELD and guest time nearly stops
            Some(CROWN) => loop {
                let _ = api.now();
            },
            _ => {}
        }
    }
}
