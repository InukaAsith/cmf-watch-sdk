//! ABI 1.3 test app for tests/test_back_e2e.py: owns "back", counts BUTTONs and exits on the third; a crown detent
//! makes it stop reading events (hung), so the loader's escape must still stop it (OwnBackTest). A tap buzzes the
//! motor 200 ms, 4 s later (ScreenOffTest: the buzz while the panel is off).
#![no_std]
#![no_main]

use cmf_wp2_app::{Api, BUTTON, CROWN, TOUCH_DOWN};

cmf_wp2_app::entry!(main);

// BUTTONs read; non-zero initialiser: in .data, fresh at every launch
#[unsafe(no_mangle)]
static mut PRESSES: i32 = -1;

fn main(api: &'static Api) -> i32 {
    api.own_back(true);
    unsafe { (&raw mut PRESSES).write_volatile(0) };
    let mut buzz_at: Option<u32> = None;
    loop {
        let now = api.now();
        if let Some(t) = buzz_at {
            let left = t.wrapping_sub(now) as i32;
            if left <= -200 {
                api.vibrate(0, 0);
                buzz_at = None;
            } else if left <= 0 {
                api.vibrate(1000, 31);
            }
        }
        let wait = if buzz_at.is_some() { 20 } else { u32::MAX };
        match api.event(wait).map(|e| e.kind) {
            Some(TOUCH_DOWN) => buzz_at = Some(api.now().wrapping_add(4000)),
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
