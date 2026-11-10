#![no_std]
#![no_main]

use core::ptr::{addr_of_mut, read_volatile, write_volatile};

use cmf_wp2_app::Api;

cmf_wp2_app::entry!(main);

// [count: u32 LE, kinds: [u8; 16]]
#[unsafe(no_mangle)]
static mut JOURNAL: [u8; 20] = [0; 20];

// vibrate() right after PAUSE
#[unsafe(no_mangle)]
static mut PAUSED_RC: i32 = 0;

fn record(kind: u8) {
    let j = addr_of_mut!(JOURNAL) as *mut u8;
    unsafe {
        let mut count = 0u32;
        for i in 0..4 {
            count |= (read_volatile(j.add(i)) as u32) << (8 * i);
        }
        if count < 16 {
            write_volatile(j.add(4 + count as usize), kind);
        }
        count += 1;
        for i in 0..4 {
            write_volatile(j.add(i), (count >> (8 * i)) as u8);
        }
    }
}

fn main(api: &'static Api) -> i32 {
    // PauseTest checks this box is back on screen after RESUME
    api.rect(150, 200, 166, 60, 0xFFFF);
    api.show();
    api.vibrate(1000, 31);
    loop {
        if let Some(event) = api.event(1000) {
            record(event.kind);
            if event.kind == cmf_wp2_app::PAUSE {
                unsafe { write_volatile(addr_of_mut!(PAUSED_RC), api.vibrate(1000, 31)) };
            }
        }
    }
}
