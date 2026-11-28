//! Draws a scene (scene.rs): water and parks, the roads from paths up to motorways (wider and lighter as they
//! matter more), the route, street names along their streets, places and red pins. North up or heading up, any
//! zoom: nothing comes from the phone for a move.

use crate::scene::{self, Rec, Scene};
use cmf_wp2_app::Api;
use cmf_wp2_ui::Surface;
use cmf_wp2_ui::color::{Rgb, rgb565};
use cmf_wp2_ui::draw::{disc, line};
use cmf_wp2_ui::font;
use cmf_wp2_ui::motion::{angle, cos, isqrt, sin};

// a night map's colours: blue-grey land, teal parks, deep blue water, grey streets, amber main roads
pub const BG: Rgb = Rgb(0x1E, 0x26, 0x33);
const WATER: Rgb = Rgb(0x10, 0x23, 0x3F);
const PARK: Rgb = Rgb(0x1F, 0x3D, 0x34);
const RED: Rgb = Rgb(0xEA, 0x43, 0x35);
pub const ROUTE: Rgb = Rgb(0x42, 0x85, 0xF4);
const ROUTE_CASE: Rgb = Rgb(0x1A, 0x4A, 0xA8);
const STREET_TEXT: u16 = rgb565(0x9C, 0xA5, 0xB3);
const PLACE_TEXT: u16 = rgb565(0xD5, 0x95, 0x63);
const WATER_TEXT: u16 = rgb565(0x51, 0x7A, 0xB8);

/// Pin colours by kind of place (scene.rs POI kinds): other, food, shop, green, transit, health, learning and
/// worship, stay, culture, money, car; a searched place (red).
pub const KINDS: [Rgb; 12] = [
    Rgb(0x8A, 0x96, 0xA8),
    Rgb(0xF2, 0x9E, 0x3D),
    Rgb(0x4B, 0x8B, 0xF5),
    Rgb(0x34, 0xA8, 0x53),
    Rgb(0x3E, 0xA0, 0xE0),
    Rgb(0xEA, 0x43, 0x35),
    Rgb(0xA8, 0x8C, 0x7A),
    Rgb(0xE0, 0x6A, 0xB4),
    Rgb(0x13, 0xB5, 0xC7),
    Rgb(0x8E, 0x8C, 0xD8),
    Rgb(0x7E, 0x9C, 0xC8),
    RED,
];

/// (colour, width at 1 px a metre) per line class (scene.rs).
const LINES: [(Rgb, i32); 8] = [
    (Rgb(0x8A, 0x76, 0x58), 12), // motorway
    (Rgb(0x74, 0x68, 0x55), 11), // primary
    (Rgb(0x5E, 0x5A, 0x52), 10), // secondary
    (Rgb(0x4A, 0x52, 0x5E), 9),  // tertiary
    (Rgb(0x3A, 0x43, 0x50), 7),  // minor
    (Rgb(0x32, 0x3A, 0x46), 2),  // path
    (Rgb(0x48, 0x4E, 0x5A), 2),  // railway
    (Rgb(0x17, 0x34, 0x5E), 4),  // waterway
];
/// Drawn under to over.
const ORDER: [u8; 8] = [
    scene::WATERWAY,
    scene::RAIL,
    scene::PATH,
    scene::MINOR,
    scene::TERTIARY,
    scene::SECONDARY,
    scene::PRIMARY,
    scene::MOTORWAY,
];

/// Where the map is: `at` (metres in the frame) drawn at panel point `screen`, `k` px a metre × 256, `heading` up
/// (degrees, 0 north).
#[derive(Clone, Copy)]
pub struct View {
    pub at: (i32, i32),
    pub screen: (i32, i32),
    pub k: i32,
    pub heading: u16,
}

impl View {
    /// A frame point on the panel (as nav state.rs `project`, so the route lies on the streets).
    pub fn px(&self, p: (i32, i32)) -> (i32, i32) {
        let (dx, dy) = (p.0 - self.at.0, p.1 - self.at.1);
        let a = (u32::from(self.heading) * 65536 / 360) as u16;
        let (c, s) = (cos(a), sin(a));
        let xr = i64::from(dx) * i64::from(c) - i64::from(dy) * i64::from(s);
        let yr = i64::from(dx) * i64::from(s) + i64::from(dy) * i64::from(c);
        let k = |v: i64| ((v * i64::from(self.k)) >> 22).clamp(-30000, 30000) as i32;
        (self.screen.0 + k(xr), self.screen.1 - k(yr))
    }

    /// Metres a panel pixel.
    pub fn metres(&self, px: i32) -> i32 {
        px * 256 / self.k.max(1)
    }
}

/// The part of a→b inside the panel grown by `m` (Liang–Barsky); None if it misses. Keeps lines short enough
/// for draw::line (its length² × 256 is an i32).
fn clip(a: (i32, i32), b: (i32, i32), m: i32) -> Option<((i32, i32), (i32, i32))> {
    let (lo, hi) = (i64::from(-m), i64::from(466 + m));
    let (x0, y0) = (i64::from(a.0), i64::from(a.1));
    let (dx, dy) = (i64::from(b.0) - x0, i64::from(b.1) - y0);
    // t in 1/65536
    let (mut t0, mut t1) = (0i64, 65536i64);
    for (p, q) in [(-dx, x0 - lo), (dx, hi - x0), (-dy, y0 - lo), (dy, hi - y0)] {
        if p == 0 {
            if q < 0 {
                return None;
            }
        } else {
            let t = q * 65536 / p;
            if p < 0 {
                t0 = t0.max(t);
            } else {
                t1 = t1.min(t);
            }
        }
    }
    if t0 > t1 {
        return None;
    }
    let at = |t: i64| ((x0 + dx * t / 65536) as i32, (y0 + dy * t / 65536) as i32);
    Some((at(t0), at(t1)))
}

fn on_panel(p: (i32, i32), m: i32) -> bool {
    (-m..466 + m).contains(&p.0) && (-m..466 + m).contains(&p.1)
}

// --- areas: one scanline fill per ring

/// Points an area may have (the phone simplifies them to fit).
pub const MAX_RING: usize = 512;

fn ring() -> &'static mut [(i16, i16); MAX_RING] {
    &mut crate::ram::ram().draw.ring
}

fn fill(s: &mut &'static Api, pts: &[(i16, i16)], c: u16) {
    let (mut y0, mut y1) = (i32::MAX, i32::MIN);
    for p in pts {
        y0 = y0.min(i32::from(p.1));
        y1 = y1.max(i32::from(p.1));
    }
    let mut xs = [0i32; 48];
    for y in y0.max(0)..=y1.min(465) {
        let mut n = 0;
        let mut j = pts.len() - 1;
        for i in 0..pts.len() {
            let (a, b) = (pts[j], pts[i]);
            let (ay, by) = (i32::from(a.1), i32::from(b.1));
            if (ay <= y) != (by <= y) && n < xs.len() {
                let (ax, bx) = (i32::from(a.0), i32::from(b.0));
                xs[n] = ax + (bx - ax) * (y - ay) / (by - ay);
                n += 1;
            }
            j = i;
        }
        // insertion sort: a few crossings a row (core's sort costs 2 KB of image)
        for i in 1..n {
            let mut j = i;
            while j > 0 && xs[j - 1] > xs[j] {
                xs.swap(j - 1, j);
                j -= 1;
            }
        }
        for pair in xs[..n].chunks_exact(2) {
            let (x0, x1) = (pair[0].max(0), pair[1].min(466));
            if x1 > x0 {
                Surface::rect(s, x0, y, x1 - x0, 1, c);
            }
        }
    }
}

// --- text along a street: the label in a 1-bit buffer, read back turned

const TW: usize = 384;
const TH: usize = 24;

fn bits() -> &'static mut [u8; TW / 8 * TH] {
    &mut crate::ram::ram().draw.bits
}

struct Bits;
impl Surface for Bits {
    fn rect(&mut self, x: i32, y: i32, w: i32, h: i32, _c: u16) {
        let b = bits();
        for yy in y.max(0)..(y + h).min(TH as i32) {
            for xx in x.max(0)..(x + w).min(TW as i32) {
                b[yy as usize * TW / 8 + xx as usize / 8] |= 0x80 >> (xx % 8);
            }
        }
    }
    fn text(&mut self, _: i32, _: i32, _: &str, _: u16) {}
}

fn bit(u: i32, v: i32) -> bool {
    (0..TW as i32).contains(&u)
        && (0..TH as i32).contains(&v)
        && bits()[v as usize * TW / 8 + u as usize / 8] & (0x80 >> (u % 8)) != 0
}

/// Street names are drawn at 2/3 of the font's size (a map's small type): each panel pixel reads 1.5 font pixels.
const SHRINK: (i32, i32) = (3, 2);

/// A street name's width on the panel.
fn small_width(t: &str) -> i32 {
    font::width(t).min(TW as i32) * SHRINK.1 / SHRINK.0
}

/// `t` small, centred on (cx, cy), its baseline along `a` (turns, y down), never upside down.
fn text_along(s: &mut &'static Api, cx: i32, cy: i32, t: &str, a: u16, c: u16) {
    let a = if (16384..49152).contains(&a) {
        a.wrapping_add(32768)
    } else {
        a
    };
    *bits() = [0; TW / 8 * TH];
    let w = font::draw(&mut Bits, 0, 2, t, 0).min(TW as i32);
    let (co, si) = (cos(a), sin(a));
    // half sizes on the panel
    let (hw, hh) = (
        w * SHRINK.1 / SHRINK.0 / 2,
        TH as i32 * SHRINK.1 / SHRINK.0 / 2,
    );
    let ex = (hw * co.abs() + hh * si.abs()) >> 14;
    let ey = (hw * si.abs() + hh * co.abs()) >> 14;
    for y in (cy - ey - 1).max(0)..=(cy + ey + 1).min(465) {
        let mut run: Option<i32> = None;
        for x in (cx - ex - 1).max(0)..=(cx + ex + 2).min(466) {
            let (dx, dy) = (x - cx, y - cy);
            // panel → font pixels: turned back, then grown 1.5×; a pixel is ink if any font pixel under it is
            let u = (((dx * co + dy * si) >> 14) + hw) * SHRINK.0 / SHRINK.1;
            let v = (((-dx * si + dy * co) >> 14) + hh) * SHRINK.0 / SHRINK.1;
            let ink = bit(u, v) || bit(u + 1, v) || bit(u, v + 1) || bit(u + 1, v + 1);
            match (ink && x <= cx + ex + 1, run) {
                (true, None) => run = Some(x),
                (false, Some(x0)) => {
                    Surface::rect(s, x0, y, x - x0, 1, c);
                    run = None;
                }
                _ => {}
            }
        }
        if let Some(x0) = run {
            Surface::rect(s, x0, y, (cx + ex + 2).min(466) - x0, 1, c);
        }
    }
}

// --- labels that don't overlap

const MAX_BOXES: usize = 96;
struct Boxes {
    b: [(i32, i32, i32, i32); MAX_BOXES],
    n: usize,
}

impl Boxes {
    /// On the round panel and clear of the first `n` boxes.
    fn free(&self, n: usize, x0: i32, y0: i32, x1: i32, y1: i32) -> bool {
        let inside = |x: i32, y: i32| (x - 233) * (x - 233) + (y - 233) * (y - 233) < 222 * 222;
        inside(x0, y0)
            && inside(x1, y0)
            && inside(x0, y1)
            && inside(x1, y1)
            && !self.b[..n]
                .iter()
                .any(|&(a0, b0, a1, b1)| x0 < a1 && a0 < x1 && y0 < b1 && b0 < y1)
    }

    /// Takes the box if it is free and on the round panel.
    fn take(&mut self, x0: i32, y0: i32, x1: i32, y1: i32) -> bool {
        if self.n == MAX_BOXES || !self.free(self.n, x0, y0, x1, y1) {
            return false;
        }
        self.b[self.n] = (x0, y0, x1, y1);
        self.n += 1;
        true
    }

    /// Takes all of `bs` or none (they may overlap each other, not what was taken before).
    fn take_all(&mut self, bs: &[(i32, i32, i32, i32)]) -> bool {
        let n = self.n;
        if n + bs.len() > MAX_BOXES
            || !bs
                .iter()
                .all(|&(x0, y0, x1, y1)| self.free(n, x0, y0, x1, y1))
        {
            return false;
        }
        self.b[n..n + bs.len()].copy_from_slice(bs);
        self.n += bs.len();
        true
    }
}

type Best = (u16, (i16, i16), (i16, i16), u8);

/// The drawing's buffers, kept in the icon's memory (ram.rs).
#[repr(C)]
pub struct Buffers {
    ring: [(i16, i16); MAX_RING],
    bits: [u8; TW / 8 * TH],
    best: [Best; scene::MAX_NAMES],
    pins: [((i16, i16), (i32, i32)); MAX_PINS],
}
/// Per name: its street's longest piece on the panel (length² / 16, ends, its road's class).

fn best() -> &'static mut [Best; scene::MAX_NAMES] {
    &mut crate::ram::ram().draw.best
}

/// What a zoom shows: the farther out, the fewer roads (all of a town's side streets at 5 m a pixel are a mesh
/// and more drawing than a frame has time for).
fn shows_line(class: u8, k: i32) -> bool {
    match class {
        scene::PATH | scene::RAIL => k >= 200,
        scene::MINOR => k >= 80,
        _ => true,
    }
}

/// The map's own places (cafés, shops …) shown: not while a search's pins are up, as Google Maps.
static mut POIS: bool = true;

pub fn show_places(on: bool) {
    unsafe { POIS = on }
}

/// Street names from this zoom in; main roads' (tertiary and up) at every zoom, so far out still says where.
const NAMES_K: i32 = 80;

fn shows_name(class: u8, k: i32) -> bool {
    class <= scene::TERTIARY || k >= NAMES_K
}

/// The pins drawn last: (panel point, frame point), for a tap on one.
const MAX_PINS: usize = 16;
static mut N_PINS: usize = 0;

fn pins() -> (&'static mut [((i16, i16), (i32, i32)); MAX_PINS], &'static mut usize) {
    unsafe {
        (
            &mut crate::ram::ram().draw.pins,
            &mut *core::ptr::addr_of_mut!(N_PINS),
        )
    }
}

/// The pins the last draw put up: (panel point, frame point).
pub fn drawn_pins() -> &'static [((i16, i16), (i32, i32))] {
    let (p, n) = pins();
    &p[..*n]
}

/// The pin drawn nearest a tap at (x, y), if one is near: its frame point and name.
pub fn pin_at(sc: &'static Scene, x: i32, y: i32) -> Option<((i32, i32), &'static str)> {
    let (ps, n) = pins();
    let mut near: Option<(i32, (i32, i32))> = None;
    for &((px, py), at) in &ps[..*n] {
        // the head is 13 px over the point
        let d = (x - i32::from(px)).pow(2) + (y - i32::from(py) + 13).pow(2);
        if d < 34 * 34 && near.is_none_or(|(b, _)| d < b) {
            near = Some((d, at));
        }
    }
    let (_, at) = near?;
    for r in sc.recs() {
        if let Rec::Label { x, y, text, .. } = r {
            if (x, y) == at {
                return Some((at, text));
            }
        }
    }
    Some((at, ""))
}

/// A small red pin, its point at (x, y).
pub fn pin(s: &mut &'static Api, x: i32, y: i32) {
    pin_in(s, x, y, RED);
}

/// A pin in `c`, its point at (x, y).
pub fn pin_in(s: &mut &'static Api, x: i32, y: i32, c: Rgb) {
    disc(s, x, y - 13, 8, c, BG);
    for k in 0..8 {
        Surface::rect(
            s,
            x - 5 + k * 5 / 8,
            y - 8 + k,
            10 - k * 10 / 8,
            1,
            c.rgb565(),
        );
    }
    disc(s, x, y - 13, 3, Rgb(0xFF, 0xFF, 0xFF), c);
}

/// Draws over what is there instead of filling the panel first (Offline Maps draws a few cells into one picture).
static mut OVER: bool = false;

#[allow(dead_code)]
pub fn draw_over(on: bool) {
    unsafe { OVER = on };
}

/// The map: fills the panel with it. `route`: frame points drawn on top of the streets.
/// `keep`: panel boxes the screen's buttons take (no names under them).
pub fn draw(
    api: &'static Api,
    sc: &Scene,
    v: &View,
    route: &[(i16, i16)],
    keep: &[(i32, i32, i32, i32)],
) {
    let mut s = api;
    if !unsafe { OVER } {
        Surface::rect(&mut s, 0, 0, 466, 466, BG.rgb565());
    }
    // areas
    for r in sc.recs() {
        if let Rec::Area { class, pts } = r {
            let ring = ring();
            let mut n = 0;
            let (mut lo, mut hi) = ((i32::MAX, i32::MAX), (i32::MIN, i32::MIN));
            for p in pts {
                if n < MAX_RING {
                    let q = v.px(p);
                    lo = (lo.0.min(q.0), lo.1.min(q.1));
                    hi = (hi.0.max(q.0), hi.1.max(q.1));
                    ring[n] = (q.0.clamp(-2000, 2000) as i16, q.1.clamp(-2000, 2000) as i16);
                    n += 1;
                }
            }
            if n >= 3 && hi.0 >= 0 && lo.0 < 466 && hi.1 >= 0 && lo.1 < 466 {
                fill(
                    &mut s,
                    &ring[..n],
                    if class == scene::WATER { WATER } else { PARK }.rgb565(),
                );
            }
        }
    }
    // roads, minor first
    let best = best();
    *best = [(0, (0, 0), (0, 0), 0); scene::MAX_NAMES];
    *pins().1 = 0;
    for class in ORDER {
        if !shows_line(class, v.k) {
            continue;
        }
        let (c, w1) = LINES[usize::from(class)];
        let w = (w1 * v.k / 256).clamp(if class <= scene::MINOR { 2 } else { 1 }, w1 + 2);
        for r in sc.recs() {
            let Rec::Line {
                class: lc,
                name,
                pts,
            } = r
            else {
                continue;
            };
            if lc != class {
                continue;
            }
            let mut prev: Option<(i32, i32)> = None;
            for p in pts {
                let q = v.px(p);
                if let Some(o) = prev {
                    if let Some((a, b)) = clip(o, q, w + 2) {
                        line(&mut s, a.0, a.1, b.0, b.1, w, c, BG);
                        if w >= 7 && on_panel(q, 0) {
                            disc(&mut s, q.0, q.1, w / 2 - 1, c, c);
                        }
                    }
                }
                prev = Some(q);
            }
            let _ = name;
        }
    }
    // each street's longest piece on the panel carries its name (the name lines, as the zoom shows their street)
    for r in sc.recs() {
        let Rec::Line { class, name, pts } = r else {
            continue;
        };
        // only main roads' names far out: no need for the others' pieces
        if class < scene::CARRIER
            || usize::from(name) >= scene::MAX_NAMES
            || !shows_line(class - scene::CARRIER, v.k)
            || !shows_name(class - scene::CARRIER, v.k)
        {
            continue;
        }
        // runs of segments that keep within ~12° of the run's first: the longest carries the name
        let mut prev: Option<(i32, i32)> = None;
        let mut run: Option<((i32, i32), (i32, i32), u16)> = None;
        for p in pts {
            let q = v.px(p);
            if let Some(o) = prev {
                match clip(o, q, -24) {
                    Some((a, c)) if c != a => {
                        let ang = angle(c.0 - a.0, c.1 - a.1);
                        run = match run {
                            Some((s0, e, a0))
                                if e == a
                                    && (ang.wrapping_sub(a0) as i16).unsigned_abs() < 3000 =>
                            {
                                Some((s0, c, a0))
                            }
                            _ => Some((a, c, ang)),
                        };
                        let (s0, e, _) = run.unwrap_or((a, c, ang));
                        let l2 = (((e.0 - s0.0).pow(2) + (e.1 - s0.1).pow(2)) >> 4) as u16;
                        let b = &mut best[usize::from(name)];
                        if l2 > b.0 {
                            *b = (
                                l2,
                                (s0.0 as i16, s0.1 as i16),
                                (e.0 as i16, e.1 as i16),
                                class - scene::CARRIER,
                            );
                        }
                    }
                    _ => run = None,
                }
            }
            prev = Some(q);
        }
    }
    // the route over the streets
    if route.len() >= 2 {
        let w = (14 * v.k / 256).clamp(5, 12);
        for pass in 0..2 {
            let (c, ww) = if pass == 0 {
                (ROUTE_CASE, w + 3)
            } else {
                (ROUTE, w)
            };
            let mut prev: Option<(i32, i32)> = None;
            for &p in route {
                let q = v.px((i32::from(p.0), i32::from(p.1)));
                if let Some(o) = prev {
                    if let Some((a, b)) = clip(o, q, ww + 2) {
                        line(&mut s, a.0, a.1, b.0, b.1, ww, c, BG);
                        if on_panel(q, 0) {
                            disc(&mut s, q.0, q.1, ww / 2 - 1, c, c);
                        }
                    }
                }
                prev = Some(q);
            }
        }
    }
    labels(api, sc, v, keep);
}

fn labels(api: &'static Api, sc: &Scene, v: &View, keep: &[(i32, i32, i32, i32)]) {
    let mut s = api;
    let mut boxes = Boxes {
        b: [(0, 0, 0, 0); MAX_BOXES],
        n: 0,
    };
    let k = keep.len().min(8);
    boxes.b[..k].copy_from_slice(&keep[..k]);
    boxes.n = k;
    // places (suburbs) when zoomed out, water names
    for r in sc.recs() {
        if let Rec::Label {
            class, x, y, text, ..
        } = r
        {
            if scene::poi_kind(class).is_some() || (class == scene::PLACE && v.k > 128) {
                continue;
            }
            let p = v.px((x, y));
            let w = font::width(text);
            if boxes.take(p.0 - w / 2 - 4, p.1 - 4, p.0 + w / 2 + 4, p.1 + 26) {
                let c = if class == scene::WATER_NAME {
                    WATER_TEXT
                } else {
                    PLACE_TEXT
                };
                font::draw(&mut s, p.0 - w / 2, p.1, text, c);
            }
        }
    }
    // street names along their streets, from the longest piece (best() holds only those this zoom names)
    {
        let best = best();
        // main roads' names first
        for class in scene::MOTORWAY..=scene::PATH {
            for i in 0..sc.n_names.min(scene::MAX_NAMES) {
                let (l2, a, b, c) = best[i];
                let l2 = i32::from(l2) << 4;
                if c != class || l2 == 0 {
                    continue;
                }
                let Some(t) = sc.name(i as u8) else { continue };
                let w = small_width(t);
                if l2 < (w + 10) * (w + 10) {
                    continue;
                }
                let (a, b) = (
                    (i32::from(a.0), i32::from(a.1)),
                    (i32::from(b.0), i32::from(b.1)),
                );
                let ang = angle(b.0 - a.0, b.1 - a.1);
                let (co, si) = (cos(ang), sin(ang));
                // the middle of the run, or slid along it to a clear spot
                let slack = isqrt(l2) / 2 - w / 2 - 4;
                for off in [0, slack / 2, -slack / 2, slack, -slack] {
                    let mx = (a.0 + b.0) / 2 + (off * co >> 14);
                    let my = (a.1 + b.1) / 2 + (off * si >> 14);
                    // the text as a row of small squares along its line
                    let n = (w / 16 + 1).min(16) as usize;
                    let mut bs = [(0, 0, 0, 0); 16];
                    for (k, bx) in bs[..n].iter_mut().enumerate() {
                        let d = -w / 2 + (k as i32 * w) / (n as i32 - 1).max(1);
                        let (x, y) = (mx + (d * co >> 14), my + (d * si >> 14));
                        *bx = (x - 9, y - 9, x + 9, y + 9);
                    }
                    if boxes.take_all(&bs[..n]) {
                        text_along(&mut s, mx, my, t, ang, STREET_TEXT);
                        break;
                    }
                }
            }
        }
    }
    // places of interest: a pin coloured by kind, the name beside it in that colour; the searched places first and
    // at any zoom, the others from close enough
    let mut shown = 0;
    for searched in [true, false] {
        if !searched && (v.k < 80 || !unsafe { POIS }) {
            break;
        }
        for r in sc.recs() {
            if let Rec::Label {
                class, x, y, text, ..
            } = r
            {
                let Some(kind) = scene::poi_kind(class) else {
                    continue;
                };
                if (usize::from(kind) == KINDS.len() - 1) != searched {
                    continue;
                }
                if shown == 10 + 6 * usize::from(searched) || shown == MAX_PINS {
                    break;
                }
                let p = v.px((x, y));
                let w = font::width(text).min(150);
                let right = p.0 + 12 + w < 440;
                let (x0, x1) = if right {
                    (p.0 - 9, p.0 + 12 + w)
                } else {
                    (p.0 - 12 - w, p.0 + 9)
                };
                if boxes.take(x0, p.1 - 24, x1, p.1 + 2) {
                    let c = KINDS[usize::from(kind).min(KINDS.len() - 1)];
                    pin_in(&mut s, p.0, p.1, c);
                    let tx = if right { p.0 + 12 } else { p.0 - 12 - w };
                    font::fitted(&mut s, tx, p.1 - 20, text, 150, c.rgb565());
                    pins().0[shown] = ((p.0 as i16, p.1 as i16), (x, y));
                    shown += 1;
                    *pins().1 = shown;
                }
            }
        }
    }
}

/// A point `d` metres along `pts` (frame metres), for a step of the route.
pub fn along(pts: &[(i16, i16)], d: i32) -> Option<(i32, i32)> {
    let mut left = d;
    for w in pts.windows(2) {
        let (a, b) = (
            (i32::from(w[0].0), i32::from(w[0].1)),
            (i32::from(w[1].0), i32::from(w[1].1)),
        );
        let len = isqrt((b.0 - a.0).pow(2) + (b.1 - a.1).pow(2));
        if left <= len && len > 0 {
            return Some((
                a.0 + (b.0 - a.0) * left / len,
                a.1 + (b.1 - a.1) * left / len,
            ));
        }
        left -= len;
    }
    pts.last().map(|p| (i32::from(p.0), i32::from(p.1)))
}
