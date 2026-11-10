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
}
