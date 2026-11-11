pub const SIZE: i32 = 466;
pub const C: i32 = SIZE / 2;
pub const ONE: i32 = 1024;
pub const SCENES: usize = 4;
// scenes paint rows above this; the page indicator sits below
pub const SCENE_BOTTOM: i32 = 428;

pub use cmf_wp2_ui::Surface;
use cmf_wp2_ui::color::Rgb;
use cmf_wp2_ui::draw::{Band, GRAD, Paint, radial, stroke};
use cmf_wp2_ui::motion::{cos, ease_out, sin, spring, wave};

pub const BLACK: u16 = 0;
pub const DARK: Rgb = Rgb(0, 0, 0);
pub const PALETTE: [Rgb; 4] = [Rgb(0x22, 0xD3, 0xEE), Rgb(0x3B, 0x82, 0xF6), Rgb(0x8B, 0x5C, 0xF6), Rgb(0xE8, 0x4C, 0xD8)];
const HI: Rgb = Rgb(0xF4, 0xF0, 0xFF);
const MID: Rgb = Rgb(0xA7, 0x8B, 0xFA);
const LO: Rgb = Rgb(0x5B, 0x21, 0xD6);
const WHITE: Rgb = Rgb(0xFF, 0xFF, 0xFF);

/// What lies under a pixel the app paints over: the black background.
pub fn black(_: i32, _: i32) -> Rgb {
    DARK
}

/// Cyclic cyan → blue → violet → magenta, in 1/256 turn steps.
pub fn hue(a: u16) -> Rgb {
    HUES[(a >> 8) as usize]
}

const fn conic(rot: u16, level: i32, tail: bool) -> Paint {
    Paint::Conic { hue: &HUES, rot, level, tail }
}

const fn lerp(a: u8, b: u8, f: i32) -> u8 {
    (a as i32 + (b as i32 - a as i32) * f / ONE) as u8
}

const HUES: [Rgb; 256] = {
    let mut t = [Rgb(0, 0, 0); 256];
    let mut j = 0;
    while j < 256 {
        let (p, q, f) = (PALETTE[j / 64], PALETTE[(j / 64 + 1) & 3], (j % 64) as i32 * 16);
        t[j] = Rgb(lerp(p.0, q.0, f), lerp(p.1, q.1, f), lerp(p.2, q.2, f));
        j += 1;
    }
    t
};

/// Antialiased bar of width `BAR_W` from radius `r0` to `r1` (px) at angle `th` around C, coloured by
/// `shade(distance from C, Q14)`; `old` is the previous `r1`: whatever of the old bar the new one does not cover
/// turns black.
fn bar(s: &mut impl Surface, th: u16, r0: i32, r1: i32, old: i32, shade: impl Fn(i32) -> Rgb) {
    stroke(s, C, C, cos(th), sin(th), r0, r1, BAR_W, old, shade, DARK);
}

struct Bands<const N: usize> {
    b: [Band; N],
    n: usize,
}

impl<const N: usize> Bands<N> {
    fn new() -> Self {
        Bands { b: [Band { r: 0, paint: Paint::Skip, soft: 0 }; N], n: 0 }
    }

    fn push(&mut self, r: i32, paint: Paint, soft: i32) {
        if self.n < N && r > self.n.checked_sub(1).map_or(0, |i| self.b[i].r) {
            self.b[self.n] = Band { r, paint, soft };
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
/// `keep`: the core is on screen from the last frame (same `r` and `flash`). `smooth`: the core and glow steps blend into each other (costs a run per colour step). The last ring ends
/// within its radius + ½ px (antialiased only if bright enough to show), so a black band 1 px wider covers it.
#[allow(clippy::too_many_arguments)]
fn orb<const N: usize>(b: &mut Bands<N>, r: i32, rot: u16, flash: i32, glow: usize, w: i32, level: i32, keep: bool, smooth: bool) {
    const STEPS: [i32; 8] = [24, 38, 50, 61, 71, 80, 90, 100];
    let soft = if smooth { GRAD } else { 0 };
    for (k, pct) in STEPS.iter().enumerate() {
        let t = k as i32 * ONE / 7;
        let c = if t < ONE / 2 { HI.mix(MID, 2 * t) } else { MID.mix(LO, 2 * t - ONE) };
        let c = c.mix(WHITE, flash / 3);
        b.push(r * pct / 100, if keep { Paint::Keep(c) } else { Paint::Solid(c) }, if k == 7 { 1 } else { soft });
    }
    b.push(r + 4, conic(rot, ONE, false), 1);
    b.push(r + 8, conic(rot, ONE * 55 / 100, false), 1);
    const GLOW: [i32; 6] = [38, 25, 16, 10, 6, 3];
    for (k, g) in GLOW.iter().take(glow).enumerate() {
        let last = k + 1 == glow;
        b.push(r + 8 + w * (k as i32 + 1), conic(rot, ONE * g / 100 * level / ONE, false), if last { (*g >= 10) as i32 } else { soft });
    }
}

pub const BARS: usize = 40;
const BAR_R: i32 = 100;
const BAR_STEP: i32 = 3;
const BAR_W: i32 = 5;
const BAR_MAX: i32 = 26;
const ORBITS: usize = 3;
const PER_ORBIT: usize = 3;
const TRAIL: usize = 8;
pub const DOTS: usize = ORBITS * PER_ORBIT * TRAIL;
const TAP_MS: [u32; SCENES] = [1100, 700, 900, 1400];

#[derive(Clone, Copy)]
struct Dot {
    x: i32,
    y: i32,
    r: i32,
    z: i32,
    c: Rgb,
}

impl Default for Dot {
    fn default() -> Self {
        Dot { x: 0, y: 0, r: 0, z: 0, c: DARK }
    }
}

/// Dot `order[j]`, its edge blended over the dots drawn before it.
fn dot(s: &mut impl Surface, dots: &[Dot; DOTS], order: &[u8], j: usize) {
    let d = dots[order[j] as usize];
    let (mut below, mut m) = ([0u8; 8], 0);
    for &k in order[..j].iter().rev() {
        let o = &dots[k as usize];
        if m < below.len() && (o.x - d.x).abs() <= o.r + d.r && (o.y - d.y).abs() <= o.r + d.r {
            below[m] = k;
            m += 1;
        }
    }
    let under = |x: i32, y: i32| {
        let hit = below[..m].iter().map(|&k| &dots[k as usize]).find(|o| (x - o.x).pow(2) + (y - o.y).pow(2) <= o.r * o.r);
        hit.map_or(DARK, |o| o.c)
    };
    radial(s, d.x, d.y, &[Band { r: d.r, paint: Paint::Solid(d.c), soft: 1 }], None, under);
}

pub struct App {
    scene: usize,
    full: bool,
    tap: Option<u32>,
    extent: i32,
    // (r, flash) of the orb core on screen
    core: Option<(i32, i32)>,
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
        App { scene: 0, full: true, tap: None, extent: 0, core: None, bars: [0; BARS], dots: [Dot::default(); DOTS] }
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

    pub fn frame(&mut self, now: u32, s: &mut impl Surface) {
        if self.full {
            self.full = false;
            s.rect(0, 0, SIZE, SIZE, BLACK);
            self.extent = 0;
            self.core = None;
            self.bars = [0; BARS];
            self.dots = [Dot::default(); DOTS];
            for i in 0..SCENES {
                let (r, c) = if i == self.scene { (5, Rgb(0xE9, 0xE3, 0xFF)) } else { (4, Rgb(0x4B, 0x3F, 0x72)) };
                radial(s, C + (2 * i as i32 - 3) * 8, 440, &[Band { r, paint: Paint::Solid(c), soft: 1 }], None, black);
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

    fn orb_scene(&mut self, now: u32, u: Option<i32>, s: &mut impl Surface) {
        let breath = wave(now, 3600);
        let flash = u.map_or(0, |u| ONE - ease_out(u));
        let rot = ((now % 6000) * 65536 / 6000) as u16;
        let r = 74 + 12 * breath / ONE + 12 * flash / ONE;
        let mut b = Bands::<22>::new();
        let keep = self.core.replace((r, flash)) == Some((r, flash));
        orb(&mut b, r, rot, flash, 6, 8 + 5 * flash / ONE, ONE * 3 / 4 + breath / 4 + flash / 2, keep, false);
        if let Some(u) = u {
            let from = b.outer() + 6;
            let ring = from + (SCENE_BOTTOM - C - 1 - from) * ease_out(u) / ONE;
            b.push(ring - 5, Paint::Solid(DARK), 1);
            b.push(ring, conic(rot.wrapping_add(32768), (ONE - u) * 4 / 5, false), 1);
        }
        let lit = b.outer() + 1;
        b.push(self.extent, Paint::Solid(DARK), 0);
        radial(s, C, C, b.get(), None, black);
        self.extent = lit;
    }

    fn voice(&mut self, now: u32, u: Option<i32>, s: &mut impl Surface) {
        let burst = u.map_or(0, |u| ONE - ease_out(u));
        let env = (ONE / 5 + wave(now, 1700) * 2 / 5 + wave(now + 400, 610) / 4 + burst).min(ONE);
        let rot = ((now % 8000) * 65536 / 8000) as u16;
        let mut b = Bands::<16>::new();
        let r = 52 + 12 * env / ONE;
        let keep = self.core.replace((r, burst)) == Some((r, burst));
        orb(&mut b, r, rot, burst, 2, 6, ONE, keep, false);
        b.push(92, Paint::Solid(DARK), 0);
        radial(s, C, C, b.get(), None, black);
        let t = now as u64;
        for i in 0..BARS {
            let th = (i as u32 * 65536 / BARS as u32) as u16;
            let p = |m: u64, w: u64| sin((th as u64 * m + t * w) as u16);
            let m = (p(3, 41) + p(5, 67) * 3 / 5 + p(2, 23) * 2 / 5) / 2;
            let amp = env * (ONE * 3 / 10 + (m.abs() * ONE / 16384).min(ONE) * 7 / 10) / ONE;
            let n = (2 + amp * (BAR_MAX - 2) / ONE).clamp(1, BAR_MAX);
            let tint = hue(th.wrapping_add(rot));
            let shade = |along: i32| {
                let k = (((along >> 4) - BAR_R * ONE) / (BAR_STEP * BAR_MAX)).clamp(0, ONE);
                tint.scale(ONE * 35 / 100 + k * 65 / 100).mix(WHITE, burst * k / ONE / 3)
            };
            let r1 = BAR_R + BAR_STEP * (n - 1) + BAR_W / 2;
            bar(s, th, BAR_R - BAR_W / 2, r1, self.bars[i], shade);
            self.bars[i] = r1;
        }
    }

    fn orbit(&mut self, now: u32, u: Option<i32>, s: &mut impl Surface) {
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
                    self.dots[n] = Dot {
                        x: C + ((ex * cos(tilt) - ey * sin(tilt)) >> 14),
                        y: C + ((ex * sin(tilt) + ey * cos(tilt)) >> 14),
                        r: ((3 + 4 * near / ONE) * fade / ONE).max(1),
                        z,
                        c: hue(tilt.wrapping_add(th)).mix(WHITE, head).scale((ONE * 3 / 10 + near * 7 / 10) * fade / ONE),
                    };
                    n += 1;
                }
            }
        }
        // far dots, then the core, then near dots; each trail tail first, so heads lie on top
        let mut order = [0u8; DOTS];
        let (mut n, mut far) = (0, 0);
        for back in [true, false] {
            for k in (0..DOTS).rev().filter(|&k| (self.dots[k].z < 0) == back) {
                order[n] = k as u8;
                n += 1;
            }
            if back {
                far = n;
            }
        }
        for j in 0..far {
            dot(s, &self.dots, &order, j);
        }
        let mut b = Bands::<16>::new();
        let r = 20 + 4 * wave(now, 2600) / ONE + 4 * flash / ONE;
        let keep = self.core.replace((r, flash)) == Some((r, flash));
        orb(&mut b, r, precess.wrapping_mul(6), flash, 3, 4, ONE, keep, true);
        b.push(52, Paint::Solid(DARK), 0);
        radial(s, C, C, b.get(), None, black);
        for j in far..DOTS {
            dot(s, &self.dots, &order, j);
        }
    }
}

fn thinking(now: u32, u: Option<i32>, s: &mut impl Surface) {
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
            Band { r: r - 4, paint: Paint::Solid(core.mix(WHITE, ONE * 2 / 5)), soft: GRAD },
            Band { r, paint: Paint::Solid(core), soft: 1 },
            Band { r: r + 4, paint: Paint::Solid(tint.scale(glow / 2)), soft: GRAD },
            Band { r: r + 8, paint: Paint::Solid(tint.scale(glow / 4)), soft: GRAD },
            Band { r: r + 12, paint: Paint::Solid(tint.scale(glow / 10)), soft: GRAD },
        ];
        let x = C + dx;
        radial(s, x, y, &bands, Some([x - 38, C - 56, x + 38, C + 38]), black);
    }
    let rot = ((now % 2400) * 65536 / 2400) as u16;
    let ring = [
        Band { r: 136, paint: Paint::Skip, soft: 1 },
        Band { r: 140, paint: conic(rot, ONE * 3 / 4 + burst / 4, true), soft: 1 },
    ];
    radial(s, C, C, &ring, None, black);
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

        fn black() -> Self {
            let mut fb = Fb::new();
            fb.px.fill(BLACK);
            fb
        }

        fn at(&self, x: i32, y: i32) -> u16 {
            self.px[(y * SIZE + x) as usize]
        }
    }

    impl Surface for Fb {
        fn text(&mut self, _: i32, _: i32, _: &str, _: u16) {}

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

    fn green(c: u16) -> f64 {
        ((c >> 5) & 63) as f64 / 63.0
    }

    // dots: a blended edge over an earlier dot mixes with that dot's colour, not black
    #[test]
    fn dot_edge_blends_over_dot_below() {
        let mut dots = [Dot::default(); DOTS];
        dots[0] = Dot { x: 100, y: 100, r: 7, z: 0, c: WHITE };
        dots[1] = Dot { x: 108, y: 100, r: 5, z: 0, c: WHITE };
        let mut fb = Fb::black();
        dot(&mut fb, &dots, &[0, 1], 0);
        dot(&mut fb, &dots, &[0, 1], 1);
        assert_eq!(fb.at(103, 100), 0xFFFF);
        assert!(green(fb.at(113, 100)) < 1.0);
    }

    fn at_scene(scene: usize) -> App {
        let mut app = App::new();
        for _ in 0..scene {
            app.crown(1);
        }
        app
    }

    // pixels on a soft edge: lit, between a black neighbour and a brighter one on the other side
    fn soft_pixels(fb: &Fb) -> usize {
        let mut n = 0;
        for y in 1..SIZE - 1 {
            for x in 1..SIZE - 1 {
                let g = green(fb.at(x, y));
                let pairs = [(fb.at(x - 1, y), fb.at(x + 1, y)), (fb.at(x, y - 1), fb.at(x, y + 1))];
                n += (g > 0.0 && pairs.iter().any(|&(a, b)| (a == BLACK && green(b) > g) || (b == BLACK && green(a) > g))) as usize;
            }
        }
        n
    }

    // incremental redraws (taps included) end on the same antialiased image as one full redraw at that frame
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
                if [12, 25, 45, 59].contains(&f) {
                    let mut fresh = at_scene(scene);
                    if let Some(tap) = app.tap {
                        fresh.tap(tap);
                    }
                    let mut full = Fb::new();
                    fresh.frame(t, &mut full);
                    let diff = fb.px.iter().zip(&full.px).filter(|(a, b)| a != b).count();
                    assert_eq!(diff, 0, "scene {scene} frame {f}");
                    assert!(soft_pixels(&full) > 150, "scene {scene} frame {f}: {} soft edge pixels", soft_pixels(&full));
                }
                t += 33;
            }
            assert!(!fb.below, "scene {scene} painted below SCENE_BOTTOM");
            assert!(fb.px.iter().filter(|&&p| p != BLACK).count() > 2000, "scene {scene} is dark");
        }
    }

    struct Count(u64, u64, u64);

    impl Surface for Count {
        fn text(&mut self, _: i32, _: i32, _: &str, _: u16) {}

        fn rect(&mut self, _: i32, _: i32, w: i32, h: i32, _: u16) {
            self.0 += 1;
            self.1 += (w * h) as u64;
        }
    }

    // like the e2e: 2.25 s per scene, a tap at 0.5 s; `cargo test rect_counts -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn rect_counts() {
        for scene in 0..SCENES {
            let mut app = at_scene(scene);
            app.frame(0, &mut Count(0, 0, 0));
            let mut c = Count(0, 0, 0);
            let frames = 68;
            for f in 1..=frames {
                if f == 15 {
                    app.tap(f * 33);
                }
                let before = c.0;
                app.frame(f * 33, &mut c);
                c.2 = c.2.max(c.0 - before);
            }
            std::println!("scene {scene}: {} rects/frame (max {}), {} px/frame", c.0 / frames as u64, c.2, c.1 / frames as u64);
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
