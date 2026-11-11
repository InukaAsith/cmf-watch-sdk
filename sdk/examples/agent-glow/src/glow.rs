pub const SIZE: i32 = 466;
pub const C: i32 = SIZE / 2;
pub const ONE: i32 = 1024;
pub const SCENES: usize = 4;
// scenes paint rows above this; the page indicator sits below
pub const SCENE_BOTTOM: i32 = 428;
const CHUNK: i32 = 8;
// coverage steps of an edge pixel
const LEVELS: i32 = 4;
// `soft` for a gradient across the whole band (clamped to the band widths)
const GRAD: i32 = 64;

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

/// Pixels `dx >= 0` with `dx² <= m`; `h` keeps the last answer - 1, so neighbouring rows cost a few steps.
fn count(h: &mut i32, m: i32) -> i32 {
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

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Paint {
    /// not painted: what is underneath shows through
    Skip,
    Solid(Rgb),
    /// already on screen in this colour: not repainted, only blended into the neighbouring bands
    Keep(Rgb),
    /// hue by angle + `rot` at brightness `level`; `tail`: brightness also falls off behind `rot` (a comet arc)
    Conic { rot: u16, level: i32, tail: bool },
}

impl Paint {
    fn at(self, a: u16) -> Option<Rgb> {
        match self {
            Paint::Skip => None,
            Paint::Solid(c) | Paint::Keep(c) => Some(c),
            Paint::Conic { rot, level, tail } => {
                let k = if tail { level * (a.wrapping_sub(rot) >> 6) as i32 / ONE } else { level };
                Some(hue(a.wrapping_add(rot)).scale(k))
            }
        }
    }
}

/// `soft`: blend width of the outer edge in px: 0 hard, 1 antialiased, more is a gradient into the next band.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Band {
    pub r: i32,
    pub paint: Paint,
    pub soft: i32,
}

pub trait Sink {
    fn rect(&mut self, x: i32, y: i32, w: i32, h: i32, c: u16);
}

/// Merges adjacent same-colour pieces of one row (or column, `vert`) into one rect.
struct Run<'a, S: Sink> {
    s: &'a mut S,
    at: i32,
    vert: bool,
    from: i32,
    len: i32,
    c: u16,
}

impl<'a, S: Sink> Run<'a, S> {
    fn new(s: &'a mut S, at: i32, vert: bool) -> Self {
        Run { s, at, vert, from: 0, len: 0, c: 0 }
    }

    fn put(&mut self, from: i32, len: i32, c: u16) {
        if len <= 0 {
            return;
        }
        if self.len > 0 && from == self.from + self.len && c == self.c {
            self.len += len;
            return;
        }
        self.flush();
        (self.from, self.len, self.c) = (from, len, c);
    }

    fn flush(&mut self) {
        if self.len > 0 {
            if self.vert {
                self.s.rect(self.at, self.from, 1, self.len, self.c);
            } else {
                self.s.rect(self.from, self.at, self.len, 1, self.c);
            }
            self.len = 0;
        }
    }
}

impl<S: Sink> Drop for Run<'_, S> {
    fn drop(&mut self) {
        self.flush();
    }
}

fn span<S: Sink>(run: &mut Run<S>, x: i32, w: i32, cx: i32, cy: i32, e: &Edge) {
    if w <= 0 {
        return;
    }
    let p = e.inner;
    match p {
        Paint::Skip | Paint::Keep(_) => {}
        Paint::Solid(_) => run.put(x, w, e.fill),
        Paint::Conic { level, .. } => {
            // dim rings change colour slowly along the row: longer chunks
            let step = if level < ONE / 8 { 4 * CHUNK } else if level < ONE / 2 { 2 * CHUNK } else { CHUNK };
            let mut at = x;
            while at < x + w {
                let n = step.min(x + w - at);
                run.put(at, n, p.at(angle(at + n / 2 - cx, run.at - cy)).unwrap_or(DARK).rgb565());
                at += n;
            }
        }
    }
}

/// Blend zone of one band edge: pixels with `lo < 4·d² <= hi`, outer weight linear in d².
#[derive(Clone, Copy)]
struct Edge {
    inner: Paint,
    outer: Paint,
    conic: bool,
    kept: bool,
    fill: u16,
    lo: i32,
    hi: i32,
}

/// Colours of an edge at angle `a`, and its blends by coverage step (bit set in `done` once computed).
#[derive(Clone, Copy)]
struct Tint {
    a: u32,
    inner: Option<Rgb>,
    outer: Option<Rgb>,
    done: u32,
    c: [u16; LEVELS as usize + 1],
}

const NO_TINT: Tint = Tint { a: u32::MAX, inner: None, outer: None, done: 0, c: [0; LEVELS as usize + 1] };

fn edge<S: Sink>(run: &mut Run<S>, x0: i32, x1: i32, cx: i32, cy: i32, e: &Edge, tint: &mut Tint, under: &impl Fn(i32, i32) -> Rgb) {
    if e.kept {
        return;
    }
    let dy = run.at - cy;
    for x in x0..x1 {
        let dx = x - cx;
        if (x - x0) % CHUNK == 0 {
            let a = if e.conic { angle(dx, dy) as u32 } else { 0 };
            if a != tint.a {
                *tint = Tint { a, inner: e.inner.at(a as u16), outer: e.outer.at(a as u16), ..NO_TINT };
            }
        }
        let t = ((4 * (dx * dx + dy * dy) - e.lo) * LEVELS + (e.hi - e.lo) / 2) / (e.hi - e.lo);
        let c = match (tint.inner, tint.outer) {
            (Some(i), Some(o)) => {
                if tint.done >> t & 1 == 0 {
                    tint.c[t as usize] = i.mix(o, t * ONE / LEVELS).rgb565();
                    tint.done |= 1 << t;
                }
                tint.c[t as usize]
            }
            (i, o) => {
                let u = under(x, run.at);
                i.unwrap_or(u).mix(o.unwrap_or(u), t * ONE / LEVELS).rgb565()
            }
        };
        run.put(x, 1, c);
    }
}

const MAX_BANDS: usize = 24;

/// Concentric bands (ascending `r`) around (cx, cy), every pixel written once, edges blended per `soft`.
/// `clear`: [x0, y0, x1, y1), containing the disc; the rest of each of its rows is painted black.
/// `under`: the colour below a pixel, for Skip bands and the outside of the last band.
pub fn radial(s: &mut impl Sink, cx: i32, cy: i32, bands: &[Band], clear: Option<[i32; 4]>, under: impl Fn(i32, i32) -> Rgb) {
    let n = bands.len().min(MAX_BANDS);
    if n == 0 {
        return;
    }
    let mut e = [Edge { inner: Paint::Skip, outer: Paint::Skip, conic: false, kept: false, fill: 0, lo: 0, hi: 0 }; MAX_BANDS];
    let mut prev = 0;
    for i in 0..n {
        let b = bands[i];
        let next = if i + 1 < n { Some(bands[i + 1]) } else { None };
        let k = b.soft.min(b.r - prev).min(next.map_or(i32::MAX, |o| o.r - b.r)).max(0);
        let r2 = 2 * b.r;
        let outer = next.map_or(Paint::Skip, |o| o.paint);
        let conic = matches!(b.paint, Paint::Conic { .. }) || matches!(outer, Paint::Conic { .. });
        let kept = matches!((b.paint, outer), (Paint::Keep(_), Paint::Keep(_)));
        let fill = b.paint.at(0).unwrap_or(DARK).rgb565();
        e[i] = Edge { inner: b.paint, outer, conic, kept, fill, lo: (r2 - k).pow(2), hi: (r2 + k).pow(2) };
        prev = b.r;
    }
    let reach = isqrt(e[n - 1].hi / 4);
    let (y0, y1) = clear.map_or((cy - reach, cy + reach + 1), |b| (b[1], b[3]));
    let mut h = [[0i32; 2]; MAX_BANDS];
    let mut tints = [[NO_TINT; 2]; MAX_BANDS];
    let (mut a, mut b) = ([0i32; MAX_BANDS], [0i32; MAX_BANDS]);
    for y in y0..y1 {
        let dy2 = (y - cy) * (y - cy);
        for i in 0..n {
            a[i] = count(&mut h[i][0], e[i].lo / 4 - dy2);
            b[i] = if e[i].hi == e[i].lo { a[i] } else { count(&mut h[i][1], e[i].hi / 4 - dy2) };
        }
        let out = b[n - 1];
        let mut run = Run::new(s, y, false);
        if let Some([x0, _, x1, _]) = clear {
            run.put(x0, if out == 0 { x1 } else { cx - out + 1 } - x0, BLACK);
        }
        // left half (dx >= 1) from the outside in, then the right half (dx >= 0) outwards
        for i in (0..n).rev() {
            let inner = if i == 0 { 1 } else { b[i - 1].max(1) };
            if b[i] > a[i].max(1) {
                edge(&mut run, cx - b[i] + 1, cx - a[i].max(1) + 1, cx, cy, &e[i], &mut tints[i][0], &under);
            }
            span(&mut run, cx - a[i] + 1, a[i] - inner, cx, cy, &e[i]);
        }
        for i in 0..n {
            let inner = if i == 0 { 0 } else { b[i - 1] };
            span(&mut run, cx + inner, a[i] - inner, cx, cy, &e[i]);
            if b[i] > a[i] {
                edge(&mut run, cx + a[i], cx + b[i], cx, cy, &e[i], &mut tints[i][1], &under);
            }
        }
        if let Some([_, _, x1, _]) = clear {
            if out > 0 {
                run.put(cx + out, x1 - cx - out, BLACK);
            }
        }
    }
}

const Q: i32 = 1 << 14;

/// Antialiased bar of width `BAR_W` from radius `r0` to `r1` (px) at angle `th` around C, coloured by
/// `shade(distance from C, Q14)`; `old` is the previous `r1`: whatever of the old bar the new one does not cover
/// turns black.
fn bar(s: &mut impl Sink, th: u16, r0: i32, r1: i32, old: i32, shade: impl Fn(i32) -> Rgb) {
    let (ux, uy) = (cos(th), sin(th));
    // walk the major axis, runs go across the bar
    let vert = ux.abs() >= uy.abs();
    let (dm, dn) = if vert { (ux, uy) } else { (uy, ux) };
    let half = (BAR_W + 1) * Q / 2;
    let (m0, m1) = (((r0 - 1) * dm) >> 14, ((r1.max(old) + 1) * dm) >> 14);
    let ext = half / dm.abs() + 1;
    let cover = |v: i32| (v.clamp(0, Q) * LEVELS + Q / 2) / Q;
    for m in m0.min(m1) - 4..=m0.max(m1) + 4 {
        let nc = m * dn / dm;
        let col = shade(m * dm + nc * dn);
        let full = col.rgb565();
        let mut run = Run::new(s, C + m, vert);
        for n in nc - ext..=nc + ext {
            let along = m * dm + n * dn;
            let base = (half - (n * dm - m * dn).abs()).min(along - r0 * Q + Q / 2);
            let k = cover(base.min(r1 * Q + Q / 2 - along));
            if k == LEVELS {
                run.put(C + n, 1, full);
            } else if k > 0 {
                run.put(C + n, 1, col.scale(k * ONE / LEVELS).rgb565());
            } else if cover(base.min(old * Q + Q / 2 - along)) > 0 {
                run.put(C + n, 1, BLACK);
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
    b.push(r + 4, Paint::Conic { rot, level: ONE, tail: false }, 1);
    b.push(r + 8, Paint::Conic { rot, level: ONE * 55 / 100, tail: false }, 1);
    const GLOW: [i32; 6] = [38, 25, 16, 10, 6, 3];
    for (k, g) in GLOW.iter().take(glow).enumerate() {
        let last = k + 1 == glow;
        b.push(r + 8 + w * (k as i32 + 1), Paint::Conic { rot, level: ONE * g / 100 * level / ONE, tail: false }, if last { (*g >= 10) as i32 } else { soft });
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
fn dot(s: &mut impl Sink, dots: &[Dot; DOTS], order: &[u8], j: usize) {
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

    pub fn frame(&mut self, now: u32, s: &mut impl Sink) {
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

    fn orb_scene(&mut self, now: u32, u: Option<i32>, s: &mut impl Sink) {
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
            b.push(ring, Paint::Conic { rot: rot.wrapping_add(32768), level: (ONE - u) * 4 / 5, tail: false }, 1);
        }
        let lit = b.outer() + 1;
        b.push(self.extent, Paint::Solid(DARK), 0);
        radial(s, C, C, b.get(), None, black);
        self.extent = lit;
    }

    fn voice(&mut self, now: u32, u: Option<i32>, s: &mut impl Sink) {
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
        Band { r: 140, paint: Paint::Conic { rot, level: ONE * 3 / 4 + burst / 4, tail: true }, soft: 1 },
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

    fn green(c: u16) -> f64 {
        ((c >> 5) & 63) as f64 / 63.0
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
    fn isqrt_and_count() {
        for n in 0..5000 {
            let r = isqrt(n);
            assert!(r * r <= n && (r + 1) * (r + 1) > n, "{n}");
        }
        let mut h = 0;
        for m in (-50..3000).chain((-50..3000).rev()).chain([40000, 7, -1, 90000]) {
            let want = if m < 0 { 0 } else { isqrt(m) + 1 };
            assert_eq!(count(&mut h, m), want, "{m}");
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
        let red = Rgb(0xFF, 0, 0);
        let bands = [
            Band { r: 10, paint: Paint::Solid(red), soft: 1 },
            Band { r: 25, paint: Paint::Conic { rot: 0, level: ONE, tail: false }, soft: GRAD },
            Band { r: 31, paint: Paint::Solid(WHITE), soft: 1 },
        ];
        radial(&mut fb, 100, 120, &bands, Some([60, 70, 150, 160]), black);
        for y in 0..SIZE {
            for x in 0..SIZE {
                let i = (y * SIZE + x) as usize;
                let inside = (60..150).contains(&x) && (70..160).contains(&y);
                assert_eq!(fb.writes[i], inside as u8, "({x},{y})");
                let d = (((x - 100) * (x - 100) + (y - 120) * (y - 120)) as f64).sqrt();
                if d >= 31.5 && inside {
                    assert_eq!(fb.px[i], BLACK);
                }
                if d < 9.5 {
                    assert_eq!(fb.px[i], red.rgb565());
                }
            }
        }
    }

    // a white disc on black: edge pixels carry the covered fraction (8 steps), a gradient band falls off monotonically
    #[test]
    fn radial_blends_edges() {
        let mut fb = Fb::black();
        radial(&mut fb, 100, 100, &[Band { r: 20, paint: Paint::Solid(WHITE), soft: 1 }], None, black);
        let mut blended = 0;
        for y in 70..130 {
            for x in 70..130 {
                let d = (((x - 100) * (x - 100) + (y - 100) * (y - 100)) as f64).sqrt();
                let cover = (20.5 - d).clamp(0.0, 1.0);
                let g = green(fb.at(x, y));
                assert!((g - cover).abs() <= 0.2, "({x},{y}) d {d:.2}: {g:.2} vs {cover:.2}");
                blended += (g > 0.0 && g < 1.0) as i32;
            }
        }
        assert!(blended > 100, "{blended}");
        let mut fb = Fb::black();
        let bands = [
            Band { r: 10, paint: Paint::Solid(WHITE), soft: GRAD },
            Band { r: 20, paint: Paint::Solid(DARK), soft: 1 },
        ];
        radial(&mut fb, 100, 100, &bands, None, black);
        let row: Vec<f64> = (100..125).map(|x| green(fb.at(x, 100))).collect();
        assert!(row.windows(2).all(|w| w[1] <= w[0]), "{row:?}");
        assert!(row[0] == 1.0 && (row[10] - 0.5).abs() < 0.15 && row[16] == 0.0, "{row:?}");
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

    fn bar_px(th: u16, r1: i32, old: i32, fb: &mut Fb) {
        bar(fb, th, 98, r1, old, |_| WHITE);
    }

    #[test]
    fn bar_antialiases_and_erases() {
        for th in [0u16, 5000, 16384, 30000, 47000] {
            let (mut grown, mut fresh) = (Fb::black(), Fb::black());
            bar_px(th, 170, 0, &mut grown);
            bar_px(th, 120, 170, &mut grown);
            bar_px(th, 120, 0, &mut fresh);
            assert!(grown.px == fresh.px, "th {th}: shrinking left pixels behind");
            assert!(fresh.writes.iter().all(|&w| w <= 1), "th {th}");
            let (ux, uy) = (cos(th) as f64 / 16384.0, sin(th) as f64 / 16384.0);
            let mut blended = 0;
            for y in 0..SIZE {
                for x in 0..SIZE {
                    let (px, py) = ((x - C) as f64, (y - C) as f64);
                    let (along, perp) = (px * ux + py * uy, (py * ux - px * uy).abs());
                    let cover = (3.0 - perp).min(along - 97.5).min(120.5 - along).clamp(0.0, 1.0);
                    let g = green(fresh.at(x, y));
                    assert!((g - cover).abs() <= 0.2, "th {th} ({x},{y}): {g:.2} vs {cover:.2}");
                    blended += (g > 0.0 && g < 1.0) as i32;
                }
            }
            assert!(blended >= 10, "th {th}: {blended}");
        }
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

    impl Sink for Count {
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
