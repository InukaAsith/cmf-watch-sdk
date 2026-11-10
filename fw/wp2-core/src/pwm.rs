use core::sync::atomic::{AtomicU32, Ordering};

pub const STOCK_CLOCK: u32 = 0x303;
const HOSC: u32 = 32_000_000;

pub fn pwm_params(freq_hz: u16) -> Option<(u32, u16)> {
    if !(1..=20000).contains(&freq_hz) {
        return None;
    }
    let f = u32::from(freq_hz);
    let slow = (15625 + f) / (2 * f);
    if slow >= 100 {
        return Some((STOCK_CLOCK, slow as u16));
    }
    let n = HOSC.div_ceil(f * 65535) - 1;
    Some((0x200 | n, (HOSC / ((n + 1) * f)) as u16))
}

pub const CAP_MS: u32 = 2000;
// the watchdog runs every 20 ms and trips one step early, so a run ends by CAP_MS
const TRIP_MS: u32 = CAP_MS - 20;

// signed: `since` is millis | 1 and may be 1 ms ahead of `now`
pub fn over_cap(now: u32, since: u32) -> bool {
    now.wrapping_sub(since) as i32 >= TRIP_MS as i32
}

// the run is timed from the off->on switch: a call while on keeps the first start
pub fn mark_on(since: &AtomicU32, now: u32) {
    let _ = since.compare_exchange(0, now | 1, Ordering::AcqRel, Ordering::Acquire);
}

pub const E_RANGE: i32 = -1;
pub const E_PAUSED: i32 = -3;
pub const E_LOCKED: i32 = -4;

// motor(duty, freq): Ok(None) switches off, Ok(Some((clock, period))) on; `locked` after a cap cut until motor(0)
pub fn decide(duty_permille: u16, freq_hz: u16, paused: bool, locked: bool) -> Result<Option<(u32, u16)>, i32> {
    let params = pwm_params(freq_hz);
    if duty_permille > 1000 || (duty_permille > 0 && params.is_none()) {
        return Err(E_RANGE);
    }
    if paused {
        return Err(E_PAUSED);
    }
    if duty_permille == 0 {
        return Ok(None);
    }
    if locked {
        return Err(E_LOCKED);
    }
    Ok(params)
}

pub fn pulse(period: u16, duty_permille: u16) -> u16 {
    (u32::from(period) * u32::from(duty_permille.min(1000)) / 1000) as u16
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pwm_table() {
        assert_eq!(pwm_params(31), Some((0x303, 252)));
        assert_eq!(pwm_params(30), Some((0x303, 260)));
        assert_eq!(pwm_params(250), Some((0x201, 64000)));
        assert_eq!(pwm_params(1000), Some((0x200, 32000)));
        assert_eq!(pwm_params(16000), Some((0x200, 2000)));
        assert_eq!(pwm_params(0), None);
        assert_eq!(pwm_params(20001), None);
        assert_eq!(pulse(252, 500), 126);
        assert_eq!(pulse(255, 1000), 255);
    }

    #[test]
    fn cap_after_2000_ms() {
        assert!(!over_cap(1000, 1001)); // since = millis | 1 can be 1 ms ahead of the watchdog's clock
        assert!(!over_cap(2980, 1001));
        assert!(over_cap(2981, 1001));
        assert!(over_cap(1979, 0xFFFF_FFFF)); // wrap
    }

    #[test]
    fn motor_decision() {
        assert_eq!(decide(1001, 31, false, false), Err(E_RANGE));
        assert_eq!(decide(500, 0, true, true), Err(E_RANGE));
        assert_eq!(decide(0, 0, false, true), Ok(None));
        assert_eq!(decide(500, 31, true, false), Err(E_PAUSED));
        assert_eq!(decide(0, 31, true, true), Err(E_PAUSED));
        assert_eq!(decide(500, 31, false, true), Err(E_LOCKED));
        assert_eq!(decide(500, 31, false, false), Ok(Some((0x303, 252))));
    }

    #[test]
    fn clock_starts_at_first_switch_on() {
        let since = AtomicU32::new(0);
        mark_on(&since, 1000);
        mark_on(&since, 1500);
        assert_eq!(since.load(Ordering::Relaxed), 1001);
        assert!(over_cap(2981, since.load(Ordering::Relaxed)));
        since.store(0, Ordering::Relaxed); // off
        mark_on(&since, 4000);
        assert_eq!(since.load(Ordering::Relaxed), 4001);
    }
}
