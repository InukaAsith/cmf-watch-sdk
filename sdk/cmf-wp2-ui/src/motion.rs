//! Trig, easing and tweens in fixed point. Angles are u16 turns (65536 = 360°, y down); sin/cos are Q14;
//! easing maps 0..=ONE to 0..=ONE.

use crate::ONE;

// sin(i·π/128)·16384, quarter wave
const SIN: [i16; 65] = [
    0, 402, 804, 1205, 1606, 2006, 2404, 2801, 3196, 3590, 3981, 4370, 4756, 5139, 5520, 5897, 6270, 6639, 7005,
    7366, 7723, 8076, 8423, 8765, 9102, 9434, 9760, 10080, 10394, 10702, 11003, 11297, 11585, 11866, 12140, 12406,
    12665, 12916, 13160, 13395, 13623, 13842, 14053, 14256, 14449, 14635, 14811, 14978, 15137, 15286, 15426, 15557,
    15679, 15791, 15893, 15986, 16069, 16143, 16207, 16261, 16305, 16340, 16364, 16379, 16384,
];
// atan(i/32) in 1/256 turns
const ATAN: [u8; 33] = [
    0, 1, 3, 4, 5, 6, 8, 9, 10, 11, 12, 13, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 25, 26, 27, 28, 29, 29, 30,
    31, 31, 32,
];

pub const Q14: i32 = 1 << 14;

pub fn sin(a: u16) -> i32 {
    let q = a >> 14;
    let i = ((a >> 8) & 63) as usize;
    let f = (a & 255) as i32;
    let (x0, x1) = if q & 1 == 0 { (SIN[i], SIN[i + 1]) } else { (SIN[64 - i], SIN[63 - i]) };
    let v = x0 as i32 + (((x1 - x0) as i32 * f) >> 8);
    if q >= 2 { -v } else { v }
}

pub fn cos(a: u16) -> i32 {
    sin(a.wrapping_add(16384))
}

/// atan2 in turns (1/256 resolution), y down.
pub fn angle(dx: i32, dy: i32) -> u16 {
    let (ax, ay) = (dx.abs(), dy.abs());
    let (lo, hi) = if ax >= ay { (ay, ax) } else { (ax, ay) };
    if hi == 0 {
        return 0;
    }
    let t = ATAN[((lo * 32 + hi / 2) / hi) as usize] as i32;
    let o = if ax >= ay { t } else { 64 - t };
    let a = match (dx >= 0, dy >= 0) {
        (true, true) => o,
        (false, true) => 128 - o,
        (false, false) => 128 + o,
        (true, false) => 256 - o,
    };
    ((a & 255) as u16) << 8
}

pub fn isqrt(n: i32) -> i32 {
    if n <= 0 {
        return 0;
    }
    let (mut x, mut r, mut bit) = (n as u32, 0u32, 1u32 << 30);
    while bit > x {
        bit >>= 2;
    }
    while bit != 0 {
        if x >= r + bit {
            x -= r + bit;
            r = (r >> 1) + bit;
        } else {
            r >>= 1;
        }
        bit >>= 2;
    }
    r as i32
}

/// Pixels `dx >= 0` with `dx² <= m`; `h` keeps the last answer - 1, so neighbouring rows cost a few steps.
pub(crate) fn count(h: &mut i32, m: i32) -> i32 {
    if m < 0 {
        return 0;
    }
    while (*h + 1) * (*h + 1) <= m {
        *h += 1;
    }
    while *h * *h > m {
        *h -= 1;
    }
    *h + 1
}

/// (1 - cos(2π·t/period)) / 2: 0 → ONE → 0.
pub fn wave(t: u32, period: u32) -> i32 {
    let a = ((t % period) as u64 * 65536 / period as u64) as u16;
    (16384 - cos(a)) * ONE / 32768
}

pub fn lerp(a: i32, b: i32, u: i32) -> i32 {
    a + (b - a) * u.clamp(0, ONE) / ONE
}

pub fn linear(u: i32) -> i32 {
    u.clamp(0, ONE)
}

/// Cubic.
pub fn ease_in(u: i32) -> i32 {
    let u = u.clamp(0, ONE);
    u * u / ONE * u / ONE
}

/// Cubic.
pub fn ease_out(u: i32) -> i32 {
    ONE - ease_in(ONE - u)
}

pub fn ease_in_out(u: i32) -> i32 {
    let u = u.clamp(0, ONE);
    if u < ONE / 2 { ease_in(2 * u) / 2 } else { ONE / 2 + ease_out(2 * u - ONE) / 2 }
}

/// Damped bounce: out, back past zero, settles at 0.
pub fn spring(u: i32) -> i32 {
    let v = ONE - u.clamp(0, ONE);
    v * v / ONE * sin((u.clamp(0, ONE) * 160) as u16) / 16384
}

/// `from` → `to` over `ms` from `start`, shaped by `ease`. Wrapping-safe millis.
#[derive(Clone, Copy)]
pub struct Tween {
    pub from: i32,
    pub to: i32,
    pub start: u32,
    pub ms: u32,
    pub ease: fn(i32) -> i32,
}

impl Tween {
    /// Already at `v`.
    pub const fn at(v: i32) -> Tween {
        Tween { from: v, to: v, start: 0, ms: 0, ease: linear }
    }

    /// Restarts from the current value, so a retarget mid-way does not jump.
    pub fn go(&mut self, to: i32, now: u32, ms: u32) {
        *self = Tween { from: self.value(now), to, start: now, ms, ease: self.ease };
    }

    pub fn progress(&self, now: u32) -> i32 {
        let t = now.wrapping_sub(self.start);
        if self.ms == 0 || t as i32 >= self.ms as i32 { ONE } else { (t as i64 * ONE as i64 / self.ms as i64) as i32 }
    }

    pub fn value(&self, now: u32) -> i32 {
        self.from + ((self.to - self.from) as i64 * (self.ease)(self.progress(now)) as i64 / ONE as i64) as i32
    }

    pub fn done(&self, now: u32) -> bool {
        self.progress(now) == ONE
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sin_matches_float() {
        for a in (0..65536).step_by(97) {
            let want = (a as f64 / 65536.0 * std::f64::consts::TAU).sin() * 16384.0;
            assert!((sin(a as u16) as f64 - want).abs() <= 3.0, "{a}");
        }
    }

    #[test]
    fn angle_matches_float() {
        for (dx, dy) in [(1, 0), (0, 1), (-1, 0), (0, -1), (5, 3), (-7, 2), (-4, -9), (8, -1), (100, 99)] {
            let want = (dy as f64).atan2(dx as f64).rem_euclid(std::f64::consts::TAU) / std::f64::consts::TAU * 256.0;
            let got = (angle(dx, dy) >> 8) as f64;
            let diff = (got - want).abs();
            assert!(diff.min(256.0 - diff) <= 1.0, "({dx},{dy}) {got} {want}");
        }
    }

    #[test]
    fn isqrt_and_count() {
        for n in 0..5000 {
            let r = isqrt(n);
            assert!(r * r <= n && (r + 1) * (r + 1) > n, "{n}");
        }
        assert_eq!(isqrt(i32::MAX), 46340);
        let mut h = 0;
        for m in (-50..3000).chain((-50..3000).rev()).chain([40000, 7, -1, 90000]) {
            let want = if m < 0 { 0 } else { isqrt(m) + 1 };
            assert_eq!(count(&mut h, m), want, "{m}");
        }
    }

    #[test]
    fn easing_ranges() {
        assert_eq!((wave(0, 1000), wave(500, 1000)), (0, ONE));
        for f in [linear, ease_in, ease_out, ease_in_out] {
            assert_eq!((f(0), f(ONE), f(-5), f(ONE + 5)), (0, ONE, 0, ONE));
            assert!((0..ONE).all(|u| f(u) <= f(u + 1)), "monotonic");
        }
        assert_eq!(ease_in_out(ONE / 2), ONE / 2);
        assert!(ease_out(ONE / 4) > ONE / 4 && ease_in(ONE / 4) < ONE / 4);
        assert_eq!((spring(0), spring(ONE)), (0, 0));
        assert!((0..=ONE).all(|u| spring(u).abs() <= ONE));
    }

    #[test]
    fn tween_runs_retargets_and_wraps() {
        let mut t = Tween::at(10);
        assert!(t.done(5) && t.value(5) == 10);
        t.go(110, 1000, 100);
        assert_eq!((t.value(1000), t.value(1050), t.value(1100), t.value(9999)), (10, 60, 110, 110));
        t.go(10, 1050, 100); // at 60 mid-way: no jump
        assert_eq!((t.value(1050), t.value(1100), t.value(1150)), (60, 35, 10));
        let mut t = Tween { ease: ease_out, ..Tween::at(0) };
        t.go(ONE, u32::MAX - 10, 40);
        assert!(t.value(9) > ONE / 2 && !t.done(9) && t.done(30));
    }
}
