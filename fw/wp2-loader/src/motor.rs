use crate::app::millis;
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use wp2_core::pwm::{over_cap, pulse, pwm_params, STOCK_CLOCK};
use wp2_core::stock::{CMU_PWMCLK, PWM_DEVICE};

const CH: u32 = 0;
const PIN_SET_CYCLES: usize = 0;
const PIN_OFF: usize = 4;

// set while the app is paused (Task 9)
pub static PAUSED: AtomicBool = AtomicBool::new(false);
// millis() | 1 of the last start, 0 while off
static ON_SINCE: AtomicU32 = AtomicU32::new(0);

fn driver(i: usize) -> usize {
    unsafe { *(*((PWM_DEVICE + 8) as *const *const usize)).add(i) }
}

fn set_clock(v: u32) {
    unsafe { (CMU_PWMCLK as *mut u32).write_volatile(v) }
}

// App thread. Races with the UI thread's off() are accepted: either side leaves the motor in a defined state.
pub extern "C" fn motor(duty_permille: u16, freq_hz: u16) -> i32 {
    let params = pwm_params(freq_hz);
    if duty_permille > 1000 || (duty_permille > 0 && params.is_none()) {
        return -1;
    }
    if PAUSED.load(Ordering::Acquire) {
        return -3;
    }
    let Some((clk, period)) = params.filter(|_| duty_permille > 0) else {
        off();
        return 0;
    };
    let now = millis();
    set_clock(clk);
    let set = stock!(driver(PIN_SET_CYCLES), fn(u32, u32, u32, u32, u32) -> i32);
    let rc = set(PWM_DEVICE, CH, period.into(), pulse(period, duty_permille).into(), 1);
    // also on a driver error: the watchdog or stop_app restores the stock clock
    ON_SINCE.store(now | 1, Ordering::Release);
    rc.min(0)
}

pub fn off() {
    stock!(driver(PIN_OFF), fn(u32, u32) -> i32)(PWM_DEVICE, CH);
    set_clock(STOCK_CLOCK);
    ON_SINCE.store(0, Ordering::Release);
}

// UI tick (20 ms): continuous run is capped at CAP_MS
pub fn watchdog() {
    let since = ON_SINCE.load(Ordering::Acquire);
    if since != 0 && over_cap(millis(), since) {
        off();
    }
}
