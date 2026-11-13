// Heart rate for apps (docs/abi-1.4.md, "Heart rate"): the app's SENSOR request is a wish; the UI tick starts the
// stock measurement while the app runs in the foreground and stops it on pause, exit, crash and force stop.
use crate::app::{millis, QUEUE};
use core::ptr::addr_of_mut;
use core::sync::atomic::{AtomicBool, Ordering};
use wp2_core::heart::{self, Heart, SENSOR_HEART};
use wp2_core::stock::{HR_BPM, HR_OWNER, HR_WEAR, PROP_HR_MODE, UX2SYS_SET};

const E_INVAL: i32 = -22;

static WANT: AtomicBool = AtomicBool::new(false);
static mut HEART: Heart = Heart::new();

fn byte(addr: u32) -> u8 {
    unsafe { (addr as *const u8).read_volatile() }
}

// app thread (lv_submit)
pub fn request(sensor: u8, on: bool) -> i32 {
    if sensor != SENSOR_HEART {
        return E_INVAL;
    }
    WANT.store(on, Ordering::Release);
    0
}

fn set_mode(on: u32) {
    stock!(UX2SYS_SET, fn(u32, *const u32) -> i32)(PROP_HR_MODE, &on);
}

// UI thread
pub fn tick(running: bool, paused: bool) {
    let want = WANT.load(Ordering::Acquire) && running && !paused;
    let s = unsafe { (*addr_of_mut!(HEART)).tick(millis(), want, byte(HR_OWNER) != 0) };
    if s.start {
        // as the stock heart view: no stale reading from an earlier measurement
        unsafe { (HR_BPM as *mut u8).write_volatile(0) };
        set_mode(1);
    }
    if s.stop {
        set_mode(0);
    }
    if s.emit {
        QUEUE.coalesce(heart::event(byte(HR_WEAR), byte(HR_BPM)));
    }
}

// UI thread, app thread aborted
pub fn stop() {
    WANT.store(false, Ordering::Release);
    tick(false, false);
}
