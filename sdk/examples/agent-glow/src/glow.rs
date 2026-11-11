pub const SIZE: i32 = 466;
pub const C: i32 = SIZE / 2;
pub const ONE: i32 = 1024;
pub const SCENES: usize = 4;
// scenes paint rows above this; the page indicator sits below
pub const SCENE_BOTTOM: i32 = 428;
const CHUNK: i32 = 8;

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

/// Angle `a` in 1/65536 turns, result in Q14.
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

/// atan2 in 1/65536 turns (1/256 resolution), y down.
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

/// Row `dy` of a disc of radius `r`: pixel `dx` is inside iff `|dx| < half(r, dy)`, i.e. `dx² + dy² < r²`.
pub fn half(r: i32, dy: i32) -> i32 {
    let d = r * r - dy * dy;
    let s = isqrt(d);
    if d <= 0 { 0 } else if s * s == d { s } else { s + 1 }
}

/// 0..=ONE: (1 - cos(2π·t/period)) / 2.
pub fn wave(t: u32, period: u32) -> i32 {
    let a = ((t % period) as u64 * 65536 / period as u64) as u16;
    (16384 - cos(a)) * ONE / 32768
}

pub fn ease_out(u: i32) -> i32 {
    let v = ONE - u.clamp(0, ONE);
    ONE - v * v / ONE * v / ONE
}

/// Damped bounce for u in 0..=ONE: out, back past zero, settles.
pub fn spring(u: i32) -> i32 {
    let v = ONE - u.clamp(0, ONE);
    v * v / ONE * sin((u.clamp(0, ONE) * 160) as u16) / 16384
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Rgb(pub u8, pub u8, pub u8);

impl Rgb {
    pub fn rgb565(self) -> u16 {
        let q = |v: u8, bits: u32| ((v as u16 + (1 << (7 - bits))) >> (8 - bits)).min((1 << bits) - 1);
        q(self.0, 5) << 11 | q(self.1, 6) << 5 | q(self.2, 5)
    }

    pub fn scale(self, k: i32) -> Rgb {
        let s = |v: u8| (v as i32 * k.max(0) / ONE).min(255) as u8;
        Rgb(s(self.0), s(self.1), s(self.2))
    }

    pub fn mix(self, o: Rgb, t: i32) -> Rgb {
        let t = t.clamp(0, ONE);
        let m = |a: u8, b: u8| (a as i32 + (b as i32 - a as i32) * t / ONE) as u8;
        Rgb(m(self.0, o.0), m(self.1, o.1), m(self.2, o.2))
    }
}

pub const BLACK: u16 = 0;
pub const PALETTE: [Rgb; 4] = [Rgb(0x22, 0xD3, 0xEE), Rgb(0x3B, 0x82, 0xF6), Rgb(0x8B, 0x5C, 0xF6), Rgb(0xE8, 0x4C, 0xD8)];
const HI: Rgb = Rgb(0xF4, 0xF0, 0xFF);
const MID: Rgb = Rgb(0xA7, 0x8B, 0xFA);
const LO: Rgb = Rgb(0x5B, 0x21, 0xD6);
const WHITE: Rgb = Rgb(0xFF, 0xFF, 0xFF);

/// Cyclic cyan → blue → violet → magenta.
pub fn hue(a: u16) -> Rgb {
    let i = (a >> 14) as usize;
    PALETTE[i].mix(PALETTE[(i + 1) & 3], (a & 0x3FFF) as i32 >> 4)
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Paint {
    Skip,
    Solid(u16),
    /// hue by angle + `rot` at brightness `level`; `tail`: brightness also falls off behind `rot` (a comet arc)
    Conic { rot: u16, level: i32, tail: bool },
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Band {
    pub r: i32,
    pub paint: Paint,
}

pub trait Sink {
    fn rect(&mut self, x: i32, y: i32, w: i32, h: i32, c: u16);
}

fn span(s: &mut impl Sink, x: i32, y: i32, w: i32, cx: i32, cy: i32, p: Paint) {
    match p {
        Paint::Skip => {}
        Paint::Solid(c) => s.rect(x, y, w, 1, c),
        Paint::Conic { rot, level, tail } => {
            let mut at = x;
            while at < x + w {
                let n = CHUNK.min(x + w - at);
                let a = angle(at + n / 2 - cx, y - cy);
                let k = if tail { level * (a.wrapping_sub(rot) >> 6) as i32 / ONE } else { level };
                s.rect(at, y, n, 1, hue(a.wrapping_add(rot)).scale(k).rgb565());
                at += n;
            }
        }
    }
}

/// Concentric bands (ascending `r`) around (cx, cy), every pixel written once. `clear`: [x0, y0, x1, y1),
/// containing the disc; the rest of each of its rows is painted black.
pub fn radial(s: &mut impl Sink, cx: i32, cy: i32, bands: &[Band], clear: Option<[i32; 4]>) {
    let r = bands.last().map_or(0, |b| b.r);
    let (y0, y1) = clear.map_or((cy - r + 1, cy + r), |b| (b[1], b[3]));
    for y in y0..y1 {
        let dy = y - cy;
        let mut inner = 0;
        for band in bands {
            let h = half(band.r, dy);
            if h > inner {
                if inner == 0 {
                    span(s, cx - h + 1, y, 2 * h - 1, cx, cy, band.paint);
                } else {
                    span(s, cx - h + 1, y, h - inner, cx, cy, band.paint);
                    span(s, cx + inner, y, h - inner, cx, cy, band.paint);
                }
                inner = h;
            }
        }
        if let Some([x0, _, x1, _]) = clear {
            if inner == 0 {
                s.rect(x0, y, x1 - x0, 1, BLACK);
            } else {
                s.rect(x0, y, cx - inner + 1 - x0, 1, BLACK);
                s.rect(cx + inner, y, x1 - cx - inner, 1, BLACK);
            }
        }
    }
}

struct Bands<const N: usize> {
    b: [Band; N],
    n: usize,
}

impl<const N: usize> Bands<N> {
    fn new() -> Self {
        Bands { b: [Band { r: 0, paint: Paint::Skip }; N], n: 0 }
    }

    fn push(&mut self, r: i32, paint: Paint) {
        if self.n < N && r > self.n.checked_sub(1).map_or(0, |i| self.b[i].r) {
            self.b[self.n] = Band { r, paint };
            self.n += 1;
        }
    }

    fn get(&self) -> &[Band] {
        &self.b[..self.n]
    }

    fn outer(&self) -> i32 {
        self.get().last().map_or(0, |b| b.r)
    }
}

/// Glowing core of radius `r`: radial gradient, conic rim, `glow` fading conic rings of width `w`.
fn orb<const N: usize>(b: &mut Bands<N>, r: i32, rot: u16, flash: i32, glow: usize, w: i32, level: i32) {
    const STEPS: [i32; 8] = [24, 38, 50, 61, 71, 80, 90, 100];
    for (k, pct) in STEPS.iter().enumerate() {
        let t = k as i32 * ONE / 7;
        let c = if t < ONE / 2 { HI.mix(MID, 2 * t) } else { MID.mix(LO, 2 * t - ONE) };
        b.push(r * pct / 100, Paint::Solid(c.mix(WHITE, flash / 3).rgb565()));
    }
    b.push(r + 4, Paint::Conic { rot, level: ONE, tail: false });
    b.push(r + 8, Paint::Conic { rot, level: ONE * 55 / 100, tail: false });
    const GLOW: [i32; 6] = [38, 25, 16, 10, 6, 3];
    for (k, g) in GLOW.iter().take(glow).enumerate() {
        b.push(r + 8 + w * (k as i32 + 1), Paint::Conic { rot, level: ONE * g / 100 * level / ONE, tail: false });
    }
}

pub const BARS: usize = 40;
const BAR_R: i32 = 100;
const BAR_STEP: i32 = 3;
const BAR_SQ: i32 = 5;
const BAR_MAX: i32 = 26;
const ORBITS: usize = 3;
const PER_ORBIT: usize = 3;
const TRAIL: usize = 8;
pub const DOTS: usize = ORBITS * PER_ORBIT * TRAIL;
const TAP_MS: [u32; SCENES] = [1100, 700, 900, 1400];

#[derive(Clone, Copy, Default)]
struct Dot {
    x: i32,
    y: i32,
    r: i32,
    z: i32,
    c: u16,
}

pub struct App {
    scene: usize,
    full: bool,
    tap: Option<u32>,
    extent: i32,
    bars: [i32; BARS],
    dots: [Dot; DOTS],
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}

impl App {
    pub fn new() -> Self {
        App { scene: 0, full: true, tap: None, extent: 0, bars: [0; BARS], dots: [Dot::default(); DOTS] }
    }

    pub fn scene(&self) -> usize {
        self.scene
    }

    pub fn crown(&mut self, delta: i16) {
        self.scene = (self.scene as i32 + delta.signum() as i32).rem_euclid(SCENES as i32) as usize;
        self.tap = None;
        self.full = true;
    }

    pub fn tap(&mut self, now: u32) {
        self.tap = Some(now);
    }

    pub fn redraw(&mut self) {
        self.full = true;
    }

    /// Tap progress 0..ONE while the effect runs.
    fn tapped(&mut self, now: u32) -> Option<i32> {
        let t = now.wrapping_sub(self.tap?);
        let ms = TAP_MS[self.scene];
        if t >= ms {
            self.tap = None;
            return None;
        }
        Some(t as i32 * ONE / ms as i32)
    }

    pub fn frame(&mut self, now: u32, s: &mut impl Sink) {
        if self.full {
            self.full = false;
            s.rect(0, 0, SIZE, SIZE, BLACK);
            self.extent = 0;
            self.bars = [0; BARS];
            self.dots = [Dot::default(); DOTS];
            for i in 0..SCENES {
                let (r, c) = if i == self.scene { (5, Rgb(0xE9, 0xE3, 0xFF)) } else { (4, Rgb(0x4B, 0x3F, 0x72)) };
                radial(s, C + (2 * i as i32 - 3) * 8, 440, &[Band { r, paint: Paint::Solid(c.rgb565()) }], None);
            }
        }
        let u = self.tapped(now);
        match self.scene {
            0 => self.orb_scene(now, u, s),
            1 => thinking(now, u, s),
            2 => self.voice(now, u, s),
            _ => self.orbit(now, u, s),
        }
    }

    fn orb_scene(&mut self, now: u32, u: Option<i32>, s: &mut impl Sink) {
        let breath = wave(now, 3600);
        let flash = u.map_or(0, |u| ONE - ease_out(u));
        let rot = ((now % 6000) * 65536 / 6000) as u16;
        let r = 74 + 12 * breath / ONE + 12 * flash / ONE;
        let mut b = Bands::<22>::new();
        orb(&mut b, r, rot, flash, 6, 8 + 5 * flash / ONE, ONE * 3 / 4 + breath / 4 + flash / 2);
        if let Some(u) = u {
            let from = b.outer() + 6;
            let ring = from + (SCENE_BOTTOM - C - 1 - from) * ease_out(u) / ONE;
            b.push(ring - 5, Paint::Solid(BLACK));
            b.push(ring, Paint::Conic { rot: rot.wrapping_add(32768), level: (ONE - u) * 4 / 5, tail: false });
        }
        let lit = b.outer();
        b.push(self.extent, Paint::Solid(BLACK));
        radial(s, C, C, b.get(), None);
        self.extent = lit;
    }

    fn voice(&mut self, now: u32, u: Option<i32>, s: &mut impl Sink) {
        let burst = u.map_or(0, |u| ONE - ease_out(u));
        let env = (ONE / 5 + wave(now, 1700) * 2 / 5 + wave(now + 400, 610) / 4 + burst).min(ONE);
        let rot = ((now % 8000) * 65536 / 8000) as u16;
        let mut b = Bands::<16>::new();
        orb(&mut b, 52 + 12 * env / ONE, rot, burst, 2, 6, ONE);
        b.push(92, Paint::Solid(BLACK));
        radial(s, C, C, b.get(), None);
        let t = now as u64;
        for i in 0..BARS {
            let th = (i as u32 * 65536 / BARS as u32) as u16;
            let p = |m: u64, w: u64| sin((th as u64 * m + t * w) as u16);
            let m = (p(3, 41) + p(5, 67) * 3 / 5 + p(2, 23) * 2 / 5) / 2;
            let amp = env * (ONE * 3 / 10 + (m.abs() * ONE / 16384).min(ONE) * 7 / 10) / ONE;
            let n = (2 + amp * (BAR_MAX - 2) / ONE).clamp(1, BAR_MAX);
            let (dx, dy) = (cos(th), sin(th));
            let at = |k: i32| {
                let d = BAR_R + BAR_STEP * k;
                (C + ((d * dx) >> 14) - BAR_SQ / 2, C + ((d * dy) >> 14) - BAR_SQ / 2)
            };
            for k in n..self.bars[i] {
                let (x, y) = at(k);
                s.rect(x, y, BAR_SQ, BAR_SQ, BLACK);
            }
            let tint = hue(th.wrapping_add(rot));
            for k in 0..n {
                let (x, y) = at(k);
                let c = tint.scale(ONE * 35 / 100 + ONE * 65 / 100 * k / BAR_MAX).mix(WHITE, burst * k / BAR_MAX / 3);
                s.rect(x, y, BAR_SQ, BAR_SQ, c.rgb565());
            }
            self.bars[i] = n;
        }
    }

    fn orbit(&mut self, now: u32, u: Option<i32>, s: &mut impl Sink) {
        let scale = ONE + u.map_or(0, spring) * 2 / 5;
        let flash = u.map_or(0, |u| ONE - ease_out(u));
        for d in self.dots.iter().filter(|d| d.r > 0) {
            s.rect(d.x - d.r, d.y - d.r, 2 * d.r + 1, 2 * d.r + 1, BLACK);
        }
        let precess = ((now % 40000) as u64 * 65536 / 40000) as u16;
        const PERIOD: [u64; ORBITS] = [3200, 4100, 5300];
        let mut n = 0;
        for (o, period) in PERIOD.iter().enumerate() {
            let tilt = (o as u16).wrapping_mul(21845).wrapping_add(precess);
            let (a, b) = (150 * scale / ONE, 66 * scale / ONE);
            for p in 0..PER_ORBIT {
                for k in 0..TRAIL {
                    let t = now.wrapping_sub(k as u32 * 28) as u64;
                    let th = ((t % period) * 65536 / period) as u16;
                    let th = th.wrapping_add((p as u32 * 65536 / PER_ORBIT as u32) as u16).wrapping_add(o as u16 * 9000);
                    let (ex, ey) = ((a * cos(th)) >> 14, (b * sin(th)) >> 14);
                    let z = sin(th);
                    let near = (z + 16384) * ONE / 32768;
                    let fade = ONE * (TRAIL - k) as i32 / TRAIL as i32;
                    let head = if k == 0 { ONE / 3 } else { 0 };
                    let c = hue(tilt.wrapping_add(th)).mix(WHITE, head).scale((ONE * 3 / 10 + near * 7 / 10) * fade / ONE);
                    self.dots[n] = Dot {
                        x: C + ((ex * cos(tilt) - ey * sin(tilt)) >> 14),
                        y: C + ((ex * sin(tilt) + ey * cos(tilt)) >> 14),
                        r: ((3 + 4 * near / ONE) * fade / ONE).max(1),
                        z,
                        c: c.rgb565(),
                    };
                    n += 1;
                }
            }
        }
        let dot = |s: &mut _, d: &Dot| radial(s, d.x, d.y, &[Band { r: d.r, paint: Paint::Solid(d.c) }], None);
        for d in self.dots.iter().filter(|d| d.z < 0) {
            dot(s, d);
        }
        let mut b = Bands::<16>::new();
        orb(&mut b, 20 + 4 * wave(now, 2600) / ONE + 4 * flash / ONE, precess.wrapping_mul(6), flash, 3, 4, ONE);
        b.push(52, Paint::Solid(BLACK));
        radial(s, C, C, b.get(), None);
        for d in self.dots.iter().filter(|d| d.z >= 0) {
            dot(s, d);
        }
    }
}

fn thinking(now: u32, u: Option<i32>, s: &mut impl Sink) {
    const DX: [i32; 3] = [-76, 0, 76];
    const TINT: [usize; 3] = [0, 2, 3];
    let burst = u.map_or(0, |u| ONE - ease_out(u));
    for (i, dx) in DX.iter().enumerate() {
        let p = now.wrapping_add(3000).wrapping_sub(i as u32 * 170) % 1400;
        let lift = if p < 640 { wave(p, 640) } else { 0 }.max(burst);
        let r = 14 + 9 * lift / ONE;
        let y = C - 18 * lift / ONE;
        let core = Rgb(0x5B, 0x3F, 0xB8).mix(Rgb(0xE0, 0xF7, 0xFF), lift);
        let glow = ONE * 35 / 100 + lift * 65 / 100;
        let tint = PALETTE[TINT[i]];
        let bands = [
            Band { r: r - 4, paint: Paint::Solid(core.mix(WHITE, ONE * 2 / 5).rgb565()) },
            Band { r, paint: Paint::Solid(core.rgb565()) },
            Band { r: r + 4, paint: Paint::Solid(tint.scale(glow / 2).rgb565()) },
            Band { r: r + 8, paint: Paint::Solid(tint.scale(glow / 4).rgb565()) },
            Band { r: r + 12, paint: Paint::Solid(tint.scale(glow / 10).rgb565()) },
        ];
        let x = C + dx;
        radial(s, x, y, &bands, Some([x - 38, C - 56, x + 38, C + 38]));
    }
    let rot = ((now % 2400) * 65536 / 2400) as u16;
    let ring = [
        Band { r: 136, paint: Paint::Skip },
        Band { r: 140, paint: Paint::Conic { rot, level: ONE * 3 / 4 + burst / 4, tail: true } },
    ];
    radial(s, C, C, &ring, None);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::vec;
    use std::vec::Vec;

    struct Fb {
        px: Vec<u16>,
        writes: Vec<u8>,
        below: bool,
    }

    impl Fb {
        fn new() -> Self {
            Fb { px: vec![0xFFFF; (SIZE * SIZE) as usize], writes: vec![0; (SIZE * SIZE) as usize], below: false }
        }
    }

    impl Sink for Fb {
        fn rect(&mut self, x: i32, y: i32, w: i32, h: i32, c: u16) {
            for yy in y.max(0)..(y + h).min(SIZE) {
                for xx in x.max(0)..(x + w).min(SIZE) {
                    let i = (yy * SIZE + xx) as usize;
                    self.px[i] = c;
                    self.writes[i] = self.writes[i].saturating_add(1);
                    self.below |= yy >= SCENE_BOTTOM && w < SIZE;
                }
            }
        }
    }

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
    fn isqrt_and_half() {
        for n in 0..5000 {
            let r = isqrt(n);
            assert!(r * r <= n && (r + 1) * (r + 1) > n, "{n}");
        }
        for r in 0..40 {
            for dy in -40..40 {
                let h = half(r, dy);
                for dx in -45..45 {
                    assert_eq!(dx * dx + dy * dy < r * r, dx.abs() < h, "r {r} dy {dy} dx {dx}");
                }
            }
        }
    }

    #[test]
    fn easing_ranges() {
        assert_eq!(wave(0, 1000), 0);
        assert_eq!(wave(500, 1000), ONE);
        assert_eq!((ease_out(0), ease_out(ONE)), (0, ONE));
        assert_eq!((spring(0), spring(ONE)), (0, 0));
        assert!((0..=ONE).all(|u| spring(u).abs() <= ONE && ease_out(u) <= ONE));
    }

    #[test]
    fn rgb565_rounds_and_saturates() {
        assert_eq!(Rgb(255, 255, 255).rgb565(), 0xFFFF);
        assert_eq!(Rgb(0, 0, 0).rgb565(), 0);
        assert_eq!(Rgb(4, 0, 0).rgb565(), 1 << 11);
        assert_eq!(Rgb(200, 200, 200).scale(2 * ONE), Rgb(255, 255, 255));
    }

    #[test]
    fn radial_writes_each_pixel_once() {
        let mut fb = Fb::new();
        let bands = [
            Band { r: 10, paint: Paint::Solid(1) },
            Band { r: 25, paint: Paint::Conic { rot: 0, level: ONE, tail: false } },
            Band { r: 31, paint: Paint::Solid(3) },
        ];
        radial(&mut fb, 100, 120, &bands, Some([60, 70, 150, 160]));
        for y in 0..SIZE {
            for x in 0..SIZE {
                let i = (y * SIZE + x) as usize;
                let inside = (60..150).contains(&x) && (70..160).contains(&y);
                assert_eq!(fb.writes[i], inside as u8, "({x},{y})");
                let d = (x - 100) * (x - 100) + (y - 120) * (y - 120);
                if d >= 31 * 31 && inside {
                    assert_eq!(fb.px[i], BLACK);
                }
                if d < 100 {
                    assert_eq!(fb.px[i], 1);
                }
            }
        }
    }

    fn at_scene(scene: usize) -> App {
        let mut app = App::new();
        for _ in 0..scene {
            app.crown(1);
        }
        app
    }

    // incremental redraws (taps included) end on the same image as one full redraw at the last frame
    #[test]
    fn incremental_matches_full_redraw() {
        for scene in 0..SCENES {
            let mut app = at_scene(scene);
            let mut fb = Fb::new();
            assert_eq!(app.scene(), scene);
            let mut t = 1000u32;
            for f in 0..60 {
                if f == 10 || f == 41 {
                    app.tap(t);
                }
                app.frame(t, &mut fb);
                if f == 0 {
                    fb.below = false;
                }
                t += 33;
            }
            let last = t - 33;
            let mut fresh = at_scene(scene);
            if let Some(tap) = app.tap {
                fresh.tap(tap);
            }
            let mut full = Fb::new();
            fresh.frame(last, &mut full);
            let diff = fb.px.iter().zip(&full.px).filter(|(a, b)| a != b).count();
            assert_eq!(diff, 0, "scene {scene}");
            assert!(!fb.below, "scene {scene} painted below SCENE_BOTTOM");
            assert!(fb.px.iter().filter(|&&p| p != BLACK).count() > 2000, "scene {scene} is dark");
        }
    }

    #[test]
    fn crown_wraps_both_ways() {
        let mut app = App::new();
        app.crown(-1);
        assert_eq!(app.scene(), SCENES - 1);
        app.crown(1);
        app.crown(1);
        assert_eq!(app.scene(), 1);
    }

    #[test]
    fn frames_move() {
        for scene in 0..SCENES {
            let (mut a, mut b) = (Fb::new(), Fb::new());
            at_scene(scene).frame(1000, &mut a);
            at_scene(scene).frame(1400, &mut b);
            assert_ne!(a.px, b.px, "scene {scene}");
        }
    }

    #[test]
    fn redraw_clears_screen() {
        let mut app = App::new();
        let mut fb = Fb::new();
        app.frame(0, &mut fb);
        app.redraw();
        app.frame(40, &mut fb);
        assert_eq!(fb.writes.iter().filter(|&&w| w == 0).count(), 0);
        assert_eq!(fb.px[0], BLACK);
    }

    #[test]
    fn tap_effect_ends() {
        let mut app = App::new();
        app.tap(100);
        assert!(app.tapped(150).is_some());
        assert!(app.tapped(100 + TAP_MS[0]).is_none());
        assert!(app.tap.is_none());
    }
}
