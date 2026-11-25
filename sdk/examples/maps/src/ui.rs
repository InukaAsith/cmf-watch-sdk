//! Offline Maps on the watch: the saved map drawn as Maps draws its own (render.rs, a few cells of pack.rs at a
//! time), you on it from the watch's GPS (on while the app is open), markers, guidance to a marker or along a path
//! (a route the phone found, or your track back to its start), all without the phone once the map is saved.
//!
//! Map: crown zooms, a drag pans, a tap moves there, a tap on a marker opens it, a hold on the map opens that place.
//! Buttons: ◎ (left) back to you; along the bottom Mark (a marker where you are), Track (record your path / stop),
//! Back (guide back along the track to its start), List (markers, stop guiding, clear the track, delete the map).

use crate::geo::{self, dist, follow, p32, Frame, OnPath, ARRIVED, OFF};
use crate::marks::{self, Marks};
use crate::pack::{cell_name, Pack};
use crate::ram::{self, ram, ROUTE};
use crate::render::{self, View, BG, ROUTE as BLUE};
use crate::scene::Scene;
use cmf_wp2_app::{Api, Event, BUTTON, CROWN, FILE_CHUNK, GPS, MESSAGE, MSG_MAX, PAUSE, RESUME, TOUCH_DOWN, TOUCH_UP};
use cmf_wp2_ui::color::{rgb565, Rgb, BLACK};
use cmf_wp2_ui::draw::{disc, line, ring, rrect};
use cmf_wp2_ui::font;
use cmf_wp2_ui::haptic::{self, Motor};
use cmf_wp2_ui::motion::{cos, sin};
use cmf_wp2_ui::text::{dots_centre, int};
use cmf_wp2_ui::{Surface, C};

cmf_wp2_app::entry!(main);
cmf_wp2_app::app_icon!();

const W: u16 = 0xFFFF;
const WHITE: Rgb = Rgb(0xFF, 0xFF, 0xFF);
const RED: Rgb = Rgb(0xD7, 0x19, 0x21);
const ORANGE: Rgb = Rgb(0xFF, 0x8C, 0x00);
const CARD: Rgb = Rgb(0x14, 0x14, 0x14);
const BTN: Rgb = Rgb(0x10, 0x10, 0x10);
const TRACK_C: Rgb = Rgb(0xFF, 0xD0, 0x40);
const GREY: u16 = rgb565(0x8A, 0x8A, 0x8A);
const DIM: Rgb = Rgb(0x50, 0x50, 0x50);

/// Zooms, px a metre × 256: the crown steps through them.
const ZOOMS: [i32; 9] = [6, 10, 16, 26, 42, 68, 110, 180, 290];
const START_ZOOM: usize = 6;
/// Buttons: (x, y, r).
const B_MARK: (i32, i32, i32) = (99, 367, 27);
const B_TRACK: (i32, i32, i32) = (184, 417, 27);
const B_BACK: (i32, i32, i32) = (282, 417, 27);
const B_LIST: (i32, i32, i32) = (367, 367, 27);
const B_ME: (i32, i32, i32) = (40, C, 26);
/// Panel boxes the buttons and the banner take: no names drawn under them.
const KEEP: [(i32, i32, i32, i32); 6] = [
    (72, 340, 54, 54),
    (157, 390, 54, 54),
    (255, 390, 54, 54),
    (340, 340, 54, 54),
    (14, C - 26, 52, 52),
    (C - 150, 16, 300, 80),
];
const DRAG: i32 = 14;
const HOLD_MS: u32 = 600;
const NOTE_MS: u32 = 3000;
/// Rows of the list.
const ROW_Y: i32 = 78;
const ROW_H: i32 = 56;
const ROWS: usize = 6;
/// Route points a `g` message carries.
const PER_MSG: usize = 30;

static mut SCENE: Scene = Scene::new();
static mut IDX: [u8; 64] = [0; 64];
static mut CHUNK: [u8; FILE_CHUNK] = [0; FILE_CHUNK];
static mut SAVE: [u8; marks::BYTES] = [0; marks::BYTES];
static mut MARKS: Marks = Marks::new();

fn scene() -> &'static mut Scene {
    unsafe { &mut *core::ptr::addr_of_mut!(SCENE) }
}
fn marks() -> &'static mut Marks {
    unsafe { &mut *core::ptr::addr_of_mut!(MARKS) }
}

#[derive(Clone, Copy, PartialEq)]
enum Screen {
    Map,
    /// a place: a marker (Some index) or a point held on the map
    Sheet,
    List,
    /// "Delete this map?"
    Confirm,
}

/// What the banner guides you to.
#[derive(Clone, Copy, PartialEq)]
enum Guide {
    None,
    /// straight to a point (a marker: its index)
    To((i32, i32), Option<usize>),
    /// along `ram().path`: a route from the phone, or the track backwards (`back`)
    Path { back: bool },
}

struct Incoming {
    name: [u8; 20],
    nlen: usize,
    len: usize,
    on: bool,
}

struct Ui {
    api: &'static Api,
    s: &'static Api,
    motor: Motor,
    screen: Screen,
    pack: Option<Pack>,
    /// the frame (the pack's, else round the first fix: track, markers and guidance work with no map)
    frame: Option<Frame>,
    zoom: usize,
    centre: (i32, i32),
    follow: bool,
    /// you: frame metres, the last fix's time; course (degrees) once you moved
    me: Option<(i32, i32)>,
    fix_at: u32,
    course: Option<u16>,
    sats: u8,
    gps_on: bool,
    tracking: bool,
    track_t0: u32,
    guide: Guide,
    seg: usize,
    on_path: Option<OnPath>,
    off_said: bool,
    arrived: bool,
    /// the open place: frame point, marker index
    sheet: ((i32, i32), Option<usize>),
    list_top: usize,
    down: Option<(i32, i32, u32)>,
    held: bool,
    note: [u8; 48],
    n_note: usize,
    note_at: u32,
    loading: Option<(u16, u16)>,
    inc: Incoming,
    /// a route coming: points expected, got
    route_want: usize,
    route_got: usize,
    route_metres: u32,
    dirty: bool,
}

fn k_of(z: usize) -> i32 {
    ZOOMS[z]
}

fn hit((x, y, r): (i32, i32, i32), px: i32, py: i32) -> bool {
    (px - x).pow(2) + (py - y).pow(2) <= (r + 8).pow(2)
}

/// "850 m", "1.2 km", "12 km".
fn fmt_dist(m: i32, buf: &mut [u8; 16]) -> &str {
    let mut b = [0u8; 11];
    let mut n = 0;
    let mut put = |t: &str, n: &mut usize| {
        for &c in t.as_bytes() {
            if *n < 16 {
                buf[*n] = c;
                *n += 1;
            }
        }
    };
    let m = m.max(0);
    if m < 1000 {
        put(int(m, &mut b), &mut n);
        put(" m", &mut n);
    } else if m < 10_000 {
        put(int(m / 1000, &mut b), &mut n);
        put(".", &mut n);
        put(int(m % 1000 / 100, &mut b), &mut n);
        put(" km", &mut n);
    } else {
        put(int(m / 1000, &mut b), &mut n);
        put(" km", &mut n);
    }
    core::str::from_utf8(&buf[..n]).unwrap_or("")
}

/// The part of a→b inside the panel (grown by m), as render.rs clips: None if it misses.
fn clip(a: (i32, i32), b: (i32, i32), m: i32) -> Option<((i32, i32), (i32, i32))> {
    let (lo, hi) = (i64::from(-m), i64::from(466 + m));
    let (x0, y0) = (i64::from(a.0), i64::from(a.1));
    let (dx, dy) = (i64::from(b.0) - x0, i64::from(b.1) - y0);
    let (mut t0, mut t1) = (0i64, 65536i64);
    for (p, q) in [(-dx, x0 - lo), (dx, hi - x0), (-dy, y0 - lo), (dy, hi - y0)] {
        if p == 0 {
            if q < 0 {
                return None;
            }
        } else {
            let r = q * 65536 / p;
            if p < 0 {
                t0 = t0.max(r);
            } else {
                t1 = t1.min(r);
            }
            if t0 > t1 {
                return None;
            }
        }
    }
    let at = |t: i64| ((x0 + dx * t / 65536) as i32, (y0 + dy * t / 65536) as i32);
    Some((at(t0), at(t1)))
}

impl Ui {
    fn k(&self) -> i32 {
        k_of(self.zoom)
    }

    fn view(&self) -> View {
        View { at: self.centre, screen: (C, C), k: self.k(), heading: 0 }
    }

    fn send(&self, m: &[u8]) {
        let _ = self.api.send(m);
    }

    fn set_note(&mut self, t: &str) {
        let t = cmf_wp2_ui::text::fit(t.as_bytes(), self.note.len());
        self.note[..t.len()].copy_from_slice(t.as_bytes());
        self.n_note = t.len();
        self.note_at = self.api.now();
    }

    fn note(&self) -> Option<&str> {
        (self.n_note > 0 && self.api.now().wrapping_sub(self.note_at) < NOTE_MS)
            .then(|| core::str::from_utf8(&self.note[..self.n_note]).unwrap_or(""))
    }

    // --- the saved map

    fn load_pack(&mut self) {
        let idx = unsafe { &mut *core::ptr::addr_of_mut!(IDX) };
        let n = self.api.file_read("idx", 0, idx);
        self.pack = if n > 0 { Pack::parse(&idx[..n as usize]) } else { None };
        if let Some(p) = self.pack {
            self.frame = Some(p.frame);
            if self.me.is_none() {
                self.centre = (0, 0);
            }
        }
    }

    fn delete_pack(&mut self) {
        // a picture map from Offline Maps 3 (`WMP1`: its tiles t<level>_<col>_<row>) goes too
        let idx = unsafe { &mut *core::ptr::addr_of_mut!(IDX) };
        let got = self.api.file_read("idx", 0, idx);
        if got > 37 && &idx[..4] == b"WMP1" {
            let mut name = [0u8; 20];
            for li in 0..usize::from(idx[4]).min(6) {
                let at = 37 + li * 15;
                if (got as usize) < at + 15 {
                    break;
                }
                let (w, h) = (i32::from(idx[at + 9]), i32::from(idx[at + 10]));
                for r in 0..h {
                    for c in 0..w {
                        let n = cell_name(li, c, r, &mut name);
                        name[0] = b't';
                        let _ = self.api.file_delete(core::str::from_utf8(&name[..n]).unwrap_or(""));
                    }
                }
            }
        }
        if let Some(p) = self.pack.take() {
            let mut name = [0u8; 20];
            for (li, l) in p.levels[..p.n].iter().enumerate() {
                for r in 0..i32::from(l.rows) {
                    for c in 0..i32::from(l.cols) {
                        let n = cell_name(li, c, r, &mut name);
                        let _ = self.api.file_delete(core::str::from_utf8(&name[..n]).unwrap_or(""));
                    }
                }
            }
        }
        let _ = self.api.file_delete("idx");
    }

    fn save_marks(&self) {
        let b = unsafe { &mut *core::ptr::addr_of_mut!(SAVE) };
        let n = marks().bytes(b);
        let _ = self.api.save(&b[..n]);
    }

    fn mark_xy(&self, i: usize) -> Option<(i32, i32)> {
        let m = marks().v.get(i)?;
        Some(self.frame?.xy(m.lat_e7, m.lon_e7))
    }

    // --- drawing

    fn draw(&mut self) {
        self.dirty = false;
        match (self.screen, self.loading) {
            (_, Some((d, t))) => self.draw_loading(d, t),
            (Screen::Map, _) => self.draw_map(),
            (Screen::Sheet, _) => self.draw_sheet(),
            (Screen::List, _) => self.draw_list(),
            (Screen::Confirm, _) => self.draw_confirm(),
        }
        self.api.show();
    }

    fn draw_map(&mut self) {
        let mut s = self.s;
        let v = self.view();
        Surface::rect(&mut s, 0, 0, 466, 466, BG.rgb565());
        match self.pack {
            Some(p) => self.draw_cells(&p, &v),
            None => self.grid(&v),
        }
        self.draw_lines(&v);
        // markers
        for i in 0..marks().n {
            if let Some(p) = self.mark_xy(i) {
                let (x, y) = v.px(p);
                if (-20..486).contains(&x) && (-20..486).contains(&y) {
                    render::pin(&mut s, x, y);
                }
            }
        }
        // a point guided to straight
        if let Guide::To(p, None) = self.guide {
            let (x, y) = v.px(p);
            render::pin_in(&mut s, x, y, ORANGE);
        }
        self.draw_me(&v);
        self.draw_top();
        self.draw_buttons();
        self.draw_zoom();
    }

    /// The pack's cells round the view: the level for this zoom, a coarser one under it where it doesn't reach.
    fn draw_cells(&mut self, p: &Pack, v: &View) {
        let half = 233 * 256 / v.k.max(1) + 40;
        let l = p.level_for(v.k);
        let covers = |li: usize| {
            let lv = p.levels[li];
            let (lo, hi) = (lv.corner(0, 0), lv.corner(i32::from(lv.cols), i32::from(lv.rows)));
            v.at.0 - half >= lo.0 && v.at.0 + half <= hi.0 && v.at.1 - half >= lo.1 && v.at.1 + half <= hi.1
        };
        let mut first = l;
        while first > 0 && !covers(first) {
            first -= 1;
        }
        let mut name = [0u8; 20];
        for li in first..=l {
            let lv = p.levels[li];
            let (cols, rows) = lv.cells(v.at, half);
            for r in rows.clone() {
                for c in cols.clone() {
                    let n = cell_name(li, c, r, &mut name);
                    let name = core::str::from_utf8(&name[..n]).unwrap_or("");
                    let api = self.api;
                    scene().load(|buf| api.file_read(name, 0, buf).max(0) as usize);
                    if scene().total == 0 {
                        continue;
                    }
                    if li > first {
                        // over the coarser level: its own ground first (the cells are squares, north up)
                        let (x0, y0) = v.px(lv.corner(c, r));
                        let (x1, y1) = v.px(lv.corner(c + 1, r + 1));
                        let mut s = self.s;
                        Surface::rect(&mut s, x0.max(0), y1.max(0), (x1.min(466) - x0.max(0)).max(0), (y0.min(466) - y1.max(0)).max(0), BG.rgb565());
                    }
                    render::draw_over(true);
                    render::draw(self.api, scene(), v, &[], &KEEP);
                    render::draw_over(false);
                }
            }
        }
    }

    /// No map: a dot every 100 m (so a pan still shows movement).
    fn grid(&self, v: &View) {
        let mut s = self.s;
        let step = if v.k >= 68 { 100 } else if v.k >= 16 { 500 } else { 2000 };
        let half = 233 * 256 / v.k.max(1);
        let x0 = (v.at.0 - half).div_euclid(step) * step;
        let y0 = (v.at.1 - half).div_euclid(step) * step;
        let mut y = y0;
        while y <= v.at.1 + half {
            let mut x = x0;
            while x <= v.at.0 + half {
                let (px, py) = v.px((x, y));
                Surface::rect(&mut s, px, py, 2, 2, DIM.rgb565());
                x += step;
            }
            y += step;
        }
        if self.frame.is_none() {
            rrect(&mut s, C - 130, 176, 260, 70, 24, CARD, BG);
            font::centre(&mut s, C, 186, "No map saved", W);
            font::centre(&mut s, C, 212, "GPS, track and pins work", GREY);
        }
    }

    /// The track (amber) and the path guided along (blue).
    fn draw_lines(&self, v: &View) {
        let mut s = self.s;
        let r = ram();
        let path: &[(i16, i16)] = if matches!(self.guide, Guide::Path { .. }) { &r.path[..r.path_n] } else { &[] };
        for (pts, c, w) in [(&r.track.pts[..r.track.n], TRACK_C, 5), (path, BLUE, 7)] {
            for pair in pts.windows(2) {
                let (a, b) = (v.px(p32(pair[0])), v.px(p32(pair[1])));
                if let Some((a, b)) = clip(a, b, 8) {
                    line(&mut s, a.0, a.1, b.0, b.1, w, c, BG);
                }
            }
        }
        if let (Some(&st), true) = (r.track.pts[..r.track.n].first(), r.track.n > 1) {
            let (x, y) = v.px(p32(st));
            disc(&mut s, x, y, 7, TRACK_C, BG);
            disc(&mut s, x, y, 3, BLACK, TRACK_C);
        }
    }

    fn draw_me(&self, v: &View) {
        let mut s = self.s;
        let Some(p) = self.me else { return };
        let (x, y) = v.px(p);
        let (dx, dy) = (x - C, y - C);
        if dx * dx + dy * dy > 205 * 205 {
            // off the panel: a dot on the rim, towards you
            let d = geo::isqrt(i64::from(dx * dx + dy * dy)).max(1) as i32;
            disc(&mut s, C + dx * 212 / d, C + dy * 212 / d, 7, RED, BLACK);
            return;
        }
        let fresh = self.api.now().wrapping_sub(self.fix_at) < 5000;
        let c = if fresh { RED } else { DIM };
        if let Some(h) = self.course {
            // the way you go: a wedge out of the dot
            let a = (u32::from(h) * 65536 / 360) as u16;
            let (ux, uy) = (sin(a), -cos(a));
            let tip = (x + ux * 24 / 16384, y + uy * 24 / 16384);
            for side in [-1, 1] {
                let b = a.wrapping_add((side * 9000) as u16);
                let (bx, by) = (x + sin(b) * 10 / 16384, y - cos(b) * 10 / 16384);
                line(&mut s, tip.0, tip.1, bx, by, 4, c, BG);
            }
        }
        disc(&mut s, x, y, 12, WHITE, BG);
        disc(&mut s, x, y, 8, c, WHITE);
    }

    /// The banner: guidance, else the GPS and the track, else a note.
    fn draw_top(&mut self) {
        let mut s = self.s;
        let mut b = [0u8; 16];
        if self.guide != Guide::None {
            rrect(&mut s, C - 150, 18, 300, 76, 26, CARD, BG);
            let (dist_m, label, off) = self.guide_text();
            match (self.me, self.on_path.map(|o| o.head).or(self.target())) {
                (Some(me), Some(head)) if !self.arrived => {
                    // the arrow: north up, as the map
                    let (dx, dy) = (head.0 - me.0, head.1 - me.1);
                    let d = geo::isqrt(i64::from(dx) * i64::from(dx) + i64::from(dy) * i64::from(dy)).max(1) as i32;
                    let (ax, ay) = (C - 108, 56);
                    disc(&mut s, ax, ay, 26, if off { RED } else { BLUE }, CARD);
                    let (ux, uy) = (dx * 16 / d, -dy * 16 / d);
                    line(&mut s, ax - ux, ay - uy, ax + ux, ay + uy, 5, WHITE, BLUE);
                    line(&mut s, ax + ux, ay + uy, ax + ux - ux / 2 - uy / 2, ay + uy - uy / 2 + ux / 2, 5, WHITE, BLUE);
                    line(&mut s, ax + ux, ay + uy, ax + ux - ux / 2 + uy / 2, ay + uy - uy / 2 - ux / 2, 5, WHITE, BLUE);
                }
                _ => {
                    disc(&mut s, C - 108, 56, 26, DIM, CARD);
                }
            }
            let x = C - 72;
            if self.arrived {
                font::draw(&mut s, x, 28, "Arrived", W);
            } else if self.me.is_none() {
                font::draw(&mut s, x, 28, "Waiting for GPS", W);
            } else {
                font::draw(&mut s, x, 28, fmt_dist(dist_m, &mut b), W);
            }
            let c = if off { RED.rgb565() } else { GREY };
            font::fitted(&mut s, x, 56, label, 210, c);
            return;
        }
        let mut line_b = [0u8; 48];
        let mut n = 0;
        let mut put = |t: &str, n: &mut usize| {
            for &ch in t.as_bytes() {
                if *n < 48 {
                    line_b[*n] = ch;
                    *n += 1;
                }
            }
        };
        let note = self.note();
        let (text, dot) = if let Some(t) = note {
            (t, None)
        } else if self.tracking {
            let mins = self.api.now().wrapping_sub(self.track_t0) / 60_000;
            put(fmt_dist(ram().track.metres, &mut b), &mut n);
            put("  \u{b7}  ", &mut n);
            let mut ib = [0u8; 11];
            put(int(mins as i32, &mut ib), &mut n);
            put(" min", &mut n);
            (core::str::from_utf8(&line_b[..n]).unwrap_or(""), Some(RED))
        } else if !self.gps_on {
            ("GPS needs loader 4", Some(DIM))
        } else if self.me.is_some() && self.api.now().wrapping_sub(self.fix_at) < 5000 {
            let mut ib = [0u8; 11];
            put("GPS \u{b7} ", &mut n);
            put(int(i32::from(self.sats), &mut ib), &mut n);
            put(" satellites", &mut n);
            (core::str::from_utf8(&line_b[..n]).unwrap_or(""), Some(Rgb(0x34, 0xA8, 0x53)))
        } else {
            ("Looking for GPS\u{2026}", Some(ORANGE))
        };
        let w = font::width(text).min(176) + if dot.is_some() { 52 } else { 36 };
        rrect(&mut s, C - w / 2, 30, w, 36, 18, CARD, BG);
        let mut x = C - w / 2 + 18;
        if let Some(c) = dot {
            disc(&mut s, x + 6, 48, 6, c, CARD);
            x += 18;
        }
        font::fitted(&mut s, x, 37, text, 176, W);
    }

    /// Guidance: metres to go, what to, off the path.
    fn guide_text(&self) -> (i32, &'static str, bool) {
        match self.guide {
            Guide::To(p, i) => {
                let d = self.me.map(|m| dist(m, p)).unwrap_or(0);
                let name = i.and_then(|i| marks().v.get(i)).map(|m| m.name()).unwrap_or("this place");
                // (the name lives in MARKS, 'static)
                (d, unsafe { core::mem::transmute::<&str, &'static str>(name) }, false)
            }
            Guide::Path { back } => {
                let o = self.on_path;
                let off = o.is_some_and(|o| o.off > OFF);
                let label = if off { "Off the path" } else if back { "Back to the start" } else { "Along the route" };
                (o.map(|o| o.left).unwrap_or(0), label, off)
            }
            Guide::None => (0, "", false),
        }
    }

    fn target(&self) -> Option<(i32, i32)> {
        match self.guide {
            Guide::To(p, _) => Some(p),
            _ => None,
        }
    }

    fn button(&self, (x, y, r): (i32, i32, i32), on: bool) {
        let mut s = self.s;
        disc(&mut s, x, y, r, if on { WHITE } else { BTN }, BG);
        ring(&mut s, x, y, r - 1, r, Rgb(0x33, 0x33, 0x33), BG);
    }

    fn draw_buttons(&self) {
        let mut s = self.s;
        // Mark: a pin
        self.button(B_MARK, false);
        let (x, y, _) = B_MARK;
        disc(&mut s, x, y - 5, 8, WHITE, BTN);
        disc(&mut s, x, y - 5, 3, BTN, WHITE);
        line(&mut s, x - 5, y, x, y + 10, 3, WHITE, BTN);
        line(&mut s, x + 5, y, x, y + 10, 3, WHITE, BTN);
        // Track: record / stop
        self.button(B_TRACK, false);
        let (x, y, _) = B_TRACK;
        if self.tracking {
            Surface::rect(&mut s, x - 8, y - 8, 16, 16, RED.rgb565());
        } else {
            disc(&mut s, x, y, 10, RED, BTN);
        }
        // Back: a U-turn arrow (to the start)
        self.button(B_BACK, matches!(self.guide, Guide::Path { back: true }));
        let (x, y, _) = B_BACK;
        let ink = if ram().track.n > 1 { if matches!(self.guide, Guide::Path { back: true }) { BLACK } else { WHITE } } else { DIM };
        let bg = if matches!(self.guide, Guide::Path { back: true }) { WHITE } else { BTN };
        line(&mut s, x + 8, y + 9, x + 8, y - 4, 3, ink, bg);
        line(&mut s, x + 8, y - 4, x, y - 9, 3, ink, bg);
        line(&mut s, x, y - 9, x - 8, y - 4, 3, ink, bg);
        line(&mut s, x - 8, y - 4, x - 8, y + 6, 3, ink, bg);
        line(&mut s, x - 8, y + 9, x - 13, y + 3, 3, ink, bg);
        line(&mut s, x - 8, y + 9, x - 3, y + 3, 3, ink, bg);
        // List
        self.button(B_LIST, false);
        let (x, y, _) = B_LIST;
        for k in -1..=1 {
            Surface::rect(&mut s, x - 10, y + k * 7 - 1, 20, 3, W);
        }
        // ◎ back to you: red with a fix and following, white not following, grey with no fix
        self.button(B_ME, false);
        let (x, y, _) = B_ME;
        let fresh = self.me.is_some() && self.api.now().wrapping_sub(self.fix_at) < 5000;
        let c = if !fresh { DIM } else if self.follow { RED } else { WHITE };
        ring(&mut s, x, y, 9, 12, c, BTN);
        disc(&mut s, x, y, 5, c, BTN);
    }

    fn draw_zoom(&self) {
        let mut s = self.s;
        let n = ZOOMS.len() as i32;
        let y0 = C - (n - 1) * 8;
        rrect(&mut s, 426, y0 - 12, 20, (n - 1) * 16 + 24, 10, BTN, BG);
        for i in 0..n {
            let y = y0 + (n - 1 - i) * 16;
            if i == self.zoom as i32 {
                disc(&mut s, 436, y, 5, WHITE, BTN);
            } else {
                Surface::rect(&mut s, 435, y - 1, 3, 3, GREY);
            }
        }
    }

    fn draw_sheet(&mut self) {
        let mut s = self.s;
        Surface::rect(&mut s, 0, 0, 466, 466, 0);
        let (p, i) = self.sheet;
        let title = match i {
            Some(i) => marks().v[i].name(),
            None => "This place",
        };
        font::centre(&mut s, C, 92, title, W);
        let mut b = [0u8; 16];
        if let Some(me) = self.me {
            font::centre(&mut s, C, 124, fmt_dist(dist(me, p), &mut b), GREY);
        } else {
            font::centre(&mut s, C, 124, "No GPS fix yet", GREY);
        }
        for (k, label) in self.sheet_buttons().iter().enumerate() {
            let (x, y) = Self::sheet_box(k);
            let red = label.starts_with("Delete");
            rrect(&mut s, x, y, 150, 62, 31, if red { RED } else { CARD }, BLACK);
            font::centre(&mut s, x + 75, y + 20, label, W);
        }
    }

    fn sheet_buttons(&self) -> [&'static str; 4] {
        match self.sheet.1 {
            Some(_) => ["Go there", "Walk route", "Drive route", "Delete"],
            None => ["Go there", "Walk route", "Drive route", "Add a pin"],
        }
    }

    fn sheet_box(k: usize) -> (i32, i32) {
        (if k % 2 == 0 { C - 158 } else { C + 8 }, 176 + (k / 2) as i32 * 80)
    }

    /// The list's rows: markers, then what can be done.
    fn rows(&self) -> ([(u8, usize); marks::MAX + 3], usize) {
        let mut out = [(0u8, 0usize); marks::MAX + 3];
        let mut n = 0;
        for i in 0..marks().n {
            out[n] = (0, i);
            n += 1;
        }
        if self.guide != Guide::None {
            out[n] = (1, 0);
            n += 1;
        }
        if ram().track.n > 0 && !self.tracking {
            out[n] = (2, 0);
            n += 1;
        }
        if self.pack.is_some() {
            out[n] = (3, 0);
            n += 1;
        }
        (out, n)
    }

    fn draw_list(&mut self) {
        let mut s = self.s;
        Surface::rect(&mut s, 0, 0, 466, 466, 0);
        font::centre(&mut s, C, 34, "Pins", W);
        let (rows, n) = self.rows();
        if n == 0 {
            font::centre(&mut s, C, 200, "No pins yet", GREY);
            font::centre(&mut s, C, 228, "Mark drops one where you are", GREY);
            return;
        }
        let mut b = [0u8; 16];
        for (k, &(kind, i)) in rows[..n].iter().enumerate().skip(self.list_top).take(ROWS) {
            let y = ROW_Y + (k - self.list_top) as i32 * ROW_H;
            let (x0, x1) = cmf_wp2_ui::geom::chord(y, y + ROW_H - 8, 30);
            let fill = if kind == 3 { Rgb(0x3A, 0x10, 0x12) } else { CARD };
            rrect(&mut s, x0, y, x1 - x0, ROW_H - 8, 20, fill, BLACK);
            let t = match kind {
                0 => marks().v[i].name(),
                1 => "Stop guiding",
                2 => "Clear the track",
                _ => "Delete the map",
            };
            font::fitted(&mut s, x0 + 22, y + 13, t, x1 - x0 - 140, W);
            if kind == 0 {
                if let (Some(me), Some(p)) = (self.me, self.mark_xy(i)) {
                    let d = fmt_dist(dist(me, p), &mut b);
                    font::draw(&mut s, x1 - 22 - font::width(d), y + 13, d, GREY);
                }
            }
        }
    }

    fn draw_confirm(&self) {
        let mut s = self.s;
        Surface::rect(&mut s, 0, 0, 466, 466, 0);
        font::centre(&mut s, C, 150, "Delete this map?", W);
        font::centre(&mut s, C, 182, "Pins and the track stay", GREY);
        rrect(&mut s, C - 150, 240, 140, 64, 32, RED, BLACK);
        font::centre(&mut s, C - 80, 260, "Delete", W);
        rrect(&mut s, C + 10, 240, 140, 64, 32, CARD, BLACK);
        font::centre(&mut s, C + 80, 260, "Keep", W);
    }

    fn draw_loading(&self, done: u16, total: u16) {
        let mut s = self.s;
        Surface::rect(&mut s, 0, 0, 466, 466, 0);
        dots_centre(&mut s, C - 8, 150, "MAPS", 4, 3, W);
        disc(&mut s, C + 66, 174, 5, RED, BLACK);
        font::centre(&mut s, C, 222, "Saving map", W);
        let total = i32::from(total.max(1));
        let w = 260 * i32::from(done).min(total) / total;
        rrect(&mut s, C - 132, 262, 264, 12, 6, Rgb(0x26, 0x26, 0x26), BLACK);
        if w > 4 {
            rrect(&mut s, C - 130, 264, w, 8, 4, RED, Rgb(0x26, 0x26, 0x26));
        }
        let (mut a, mut b) = ([0u8; 11], [0u8; 11]);
        let (a, b) = (int(i32::from(done), &mut a), int(total, &mut b));
        let mut line_b = [0u8; 32];
        let mut n = 0;
        for part in [a, " / ", b, " parts"] {
            line_b[n..n + part.len()].copy_from_slice(part.as_bytes());
            n += part.len();
        }
        font::centre(&mut s, C, 288, core::str::from_utf8(&line_b[..n]).unwrap_or(""), GREY);
    }

    // --- GPS and guidance

    fn gps(&mut self) {
        let Some(f) = self.api.gps_fix() else { return };
        self.sats = f.sats;
        if f.fix == 0 {
            self.dirty |= self.screen == Screen::Map && self.api.now().wrapping_sub(self.fix_at) > 5000;
            return;
        }
        if self.frame.is_none() {
            // no map: a frame round this first fix
            let a = ((i64::from(f.lat_e7) * 65536 / 3_600_000_000).rem_euclid(65536)) as u16;
            let mm = (111_320_000i64 * i64::from(cos(a)) / 16384) as u32;
            self.frame = Some(Frame { lat0_e7: f.lat_e7, lon0_e7: f.lon_e7, mm_per_deg_lon: mm });
        }
        let p = self.frame.unwrap().xy(f.lat_e7, f.lon_e7);
        if let Some(old) = self.me {
            if dist(old, p) >= 4 {
                self.course = Some(geo::bearing(old, p));
            }
        }
        let first = self.me.is_none();
        self.me = Some(p);
        self.fix_at = self.api.now();
        if self.follow || first {
            self.centre = p;
        }
        if self.tracking {
            ram().track.add(p);
        }
        self.guide_update();
        self.dirty |= self.screen == Screen::Map;
    }

    fn guide_update(&mut self) {
        let Some(me) = self.me else { return };
        let (left, off) = match self.guide {
            Guide::None => return,
            Guide::To(p, _) => (dist(me, p), false),
            Guide::Path { .. } => {
                let r = ram();
                let o = follow(&r.path[..r.path_n], me, self.seg);
                self.on_path = o;
                let Some(o) = o else { return };
                self.seg = o.seg;
                (o.left, o.off > OFF)
            }
        };
        if off && !self.off_said {
            self.motor.play(haptic::ALERT);
        }
        self.off_said = off;
        if left < ARRIVED && !self.arrived {
            self.arrived = true;
            self.motor.play(haptic::DONE);
        }
    }

    fn start_guide(&mut self, g: Guide) {
        (self.guide, self.seg, self.on_path, self.off_said, self.arrived) = (g, 0, None, false, false);
        self.follow = true;
        if let Some(me) = self.me {
            self.centre = me;
        }
        self.guide_update();
        self.motor.play(haptic::DOUBLE);
    }

    /// Back to the start along the track (no phone).
    fn backtrack(&mut self) {
        let r = ram();
        if r.track.n < 2 {
            self.set_note("No track yet");
            return;
        }
        let n = r.track.n.min(ROUTE);
        for i in 0..n {
            r.path[i] = r.track.pts[r.track.n - 1 - i];
        }
        r.path_n = n;
        self.start_guide(Guide::Path { back: true });
    }

    /// A route to `to` from the phone (mode 0 drive, 2 walk); it comes as `G` + `g`.
    fn ask_route(&mut self, mode: u8, to: (i32, i32)) {
        let Some(f) = self.frame else { return };
        let mut m = [0u8; 18];
        m[0] = b'D';
        m[1] = mode;
        m[2..6].copy_from_slice(&f.lat0_e7.to_le_bytes());
        m[6..10].copy_from_slice(&f.lon0_e7.to_le_bytes());
        let (fx, fy) = self.me.map(|p| (p.0.clamp(-32767, 32767) as i16, p.1.clamp(-32767, 32767) as i16)).unwrap_or((i16::MIN, i16::MIN));
        m[10..12].copy_from_slice(&fx.to_le_bytes());
        m[12..14].copy_from_slice(&fy.to_le_bytes());
        m[14..16].copy_from_slice(&(to.0.clamp(-32767, 32767) as i16).to_le_bytes());
        m[16..18].copy_from_slice(&(to.1.clamp(-32767, 32767) as i16).to_le_bytes());
        self.send(&m);
        self.route_want = 0;
        self.set_note("Asking iPhone\u{2026}");
    }

    // --- input

    fn press(&mut self, x: i32, y: i32) -> bool {
        if hit(B_MARK, x, y) {
            let p = self.me.unwrap_or(self.centre);
            match self.frame {
                Some(f) => {
                    let (lat, lon) = f.lat_lon(p);
                    if marks().add(lat, lon) {
                        self.save_marks();
                        self.motor.play(haptic::DOUBLE);
                        let i = marks().n - 1;
                        let mut t = [0u8; 40];
                        let name = marks().v[i].name();
                        let mut n = 0;
                        for part in [name, if self.me.is_some() { " here" } else { " at centre" }] {
                            for &c in part.as_bytes() {
                                if n < 40 {
                                    t[n] = c;
                                    n += 1;
                                }
                            }
                        }
                        self.set_note(core::str::from_utf8(&t[..n]).unwrap_or(""));
                    } else {
                        self.set_note("Pins full (24)");
                    }
                }
                None => self.set_note("Waiting for GPS"),
            }
        } else if hit(B_TRACK, x, y) {
            self.motor.play(haptic::TICK);
            if self.tracking {
                self.tracking = false;
                self.set_note("Track stopped");
            } else {
                if !matches!(self.guide, Guide::Path { back: true }) {
                    ram().track.clear();
                }
                self.tracking = true;
                self.track_t0 = self.api.now();
                if let Some(me) = self.me {
                    ram().track.add(me);
                }
            }
        } else if hit(B_BACK, x, y) {
            self.motor.play(haptic::TICK);
            if matches!(self.guide, Guide::Path { back: true }) {
                self.guide = Guide::None;
            } else {
                self.backtrack();
            }
        } else if hit(B_LIST, x, y) {
            self.motor.play(haptic::TICK);
            self.list_top = 0;
            self.screen = Screen::List;
        } else if hit(B_ME, x, y) {
            self.motor.play(haptic::TICK);
            self.follow = true;
            if let Some(me) = self.me {
                self.centre = me;
            } else if self.pack.is_some() {
                self.centre = (0, 0);
            }
        } else if self.guide != Guide::None && (C - 150..C + 150).contains(&x) && (16..96).contains(&y) {
            // the banner: guiding ends
            self.motor.play(haptic::TICK);
            self.guide = Guide::None;
        } else {
            return false;
        }
        true
    }

    fn tap_map(&mut self, x: i32, y: i32) {
        let v = self.view();
        // a marker near the tap
        let mut best: Option<(i32, usize)> = None;
        for i in 0..marks().n {
            if let Some(p) = self.mark_xy(i) {
                let (px, py) = v.px(p);
                let d = (px - x).pow(2) + (py - 13 - y).pow(2);
                if d < 30 * 30 && best.is_none_or(|(b, _)| d < b) {
                    best = Some((d, i));
                }
            }
        }
        if let Some((_, i)) = best {
            self.motor.play(haptic::TICK);
            self.sheet = (self.mark_xy(i).unwrap_or(self.centre), Some(i));
            self.screen = Screen::Sheet;
            return;
        }
        // else: the map moves there
        let m = v.metres(1);
        self.centre = (self.centre.0 + (x - C) * 256 / v.k.max(1), self.centre.1 - (y - C) * 256 / v.k.max(1));
        let _ = m;
        self.follow = false;
    }

    fn sheet_tap(&mut self, x: i32, y: i32) {
        for k in 0..4 {
            let (bx, by) = Self::sheet_box(k);
            if (bx..bx + 150).contains(&x) && (by..by + 62).contains(&y) {
                self.motor.play(haptic::TICK);
                let (p, i) = self.sheet;
                self.screen = Screen::Map;
                match (k, i) {
                    (0, _) => self.start_guide(Guide::To(p, i)),
                    (1, _) => self.ask_route(2, p),
                    (2, _) => self.ask_route(0, p),
                    (3, Some(i)) => {
                        marks().remove(i);
                        self.save_marks();
                        if matches!(self.guide, Guide::To(_, Some(_))) {
                            self.guide = Guide::None;
                        }
                    }
                    (3, None) => {
                        if let Some(f) = self.frame {
                            let (lat, lon) = f.lat_lon(p);
                            if marks().add(lat, lon) {
                                self.save_marks();
                            }
                        }
                    }
                    _ => {}
                }
                return;
            }
        }
    }

    fn list_tap(&mut self, y: i32) {
        let (rows, n) = self.rows();
        let r = (y - ROW_Y).div_euclid(ROW_H);
        if y < ROW_Y || r < 0 || r as usize >= ROWS {
            return;
        }
        let k = self.list_top + r as usize;
        if k >= n {
            return;
        }
        self.motor.play(haptic::TICK);
        match rows[k] {
            (0, i) => {
                self.sheet = (self.mark_xy(i).unwrap_or(self.centre), Some(i));
                self.screen = Screen::Sheet;
            }
            (1, _) => {
                self.guide = Guide::None;
                self.screen = Screen::Map;
            }
            (2, _) => {
                ram().track.clear();
                if matches!(self.guide, Guide::Path { back: true }) {
                    self.guide = Guide::None;
                }
                self.screen = Screen::Map;
            }
            _ => self.screen = Screen::Confirm,
        }
    }

    fn handle(&mut self, e: Event) {
        let now = self.api.now();
        match e.kind {
            GPS => self.gps(),
            MESSAGE => self.messages(),
            CROWN if self.screen == Screen::Map && self.loading.is_none() => {
                let z = (self.zoom as i32 + i32::from(e.delta).signum()).clamp(0, ZOOMS.len() as i32 - 1) as usize;
                if z != self.zoom {
                    self.zoom = z;
                    self.dirty = true;
                }
            }
            CROWN if self.screen == Screen::List => {
                let (_, n) = self.rows();
                let t = (self.list_top as i32 + i32::from(e.delta).signum()).clamp(0, (n as i32 - ROWS as i32).max(0));
                self.list_top = t as usize;
                self.dirty = true;
            }
            TOUCH_DOWN => {
                self.down = Some((i32::from(e.x), i32::from(e.y), now));
                self.held = false;
            }
            TOUCH_UP => {
                let Some((x, y, _)) = self.down.take() else { return };
                if self.held || self.loading.is_some() {
                    return;
                }
                let (ux, uy) = if (e.x, e.y) == (0, 0) { (x, y) } else { (i32::from(e.x), i32::from(e.y)) };
                let (dx, dy) = (ux - x, uy - y);
                match self.screen {
                    Screen::Map if dx.abs() + dy.abs() > DRAG => {
                        let k = self.k().max(1);
                        self.centre = (self.centre.0 - dx * 256 / k, self.centre.1 + dy * 256 / k);
                        self.follow = false;
                    }
                    Screen::Map => {
                        if !self.press(x, y) {
                            self.tap_map(x, y);
                        }
                    }
                    Screen::Sheet => self.sheet_tap(x, y),
                    Screen::List => self.list_tap(y),
                    Screen::Confirm => {
                        if (230..320).contains(&y) && x < C {
                            self.delete_pack();
                            self.set_note("Map deleted");
                        }
                        self.screen = Screen::Map;
                    }
                }
                self.dirty = true;
            }
            _ => {}
        }
    }

    /// A finger held on the map: that place's sheet.
    fn timers(&mut self) {
        let now = self.api.now();
        if let Some((x, y, t)) = self.down {
            if !self.held && self.screen == Screen::Map && now.wrapping_sub(t) >= HOLD_MS && self.loading.is_none() {
                self.held = true;
                let v = self.view();
                let p = (self.centre.0 + (x - C) * 256 / v.k.max(1), self.centre.1 - (y - C) * 256 / v.k.max(1));
                self.motor.play(haptic::TICK);
                self.sheet = (p, None);
                self.screen = Screen::Sheet;
                self.dirty = true;
            }
        }
        // a note that ran out
        if self.n_note > 0 && now.wrapping_sub(self.note_at) >= NOTE_MS {
            self.n_note = 0;
            self.dirty |= self.screen == Screen::Map;
        }
    }

    fn wait(&self) -> u32 {
        let now = self.api.now();
        let left = |t: u32| (t.wrapping_sub(now) as i32).max(0) as u32;
        let mut w = self.motor.wait().unwrap_or(u32::MAX);
        if let Some((_, _, t)) = self.down {
            if !self.held {
                w = w.min(left(t.wrapping_add(HOLD_MS)));
            }
        }
        if self.n_note > 0 {
            w = w.min(left(self.note_at.wrapping_add(NOTE_MS)));
        }
        w
    }

    // --- the phone

    fn messages(&mut self) {
        let mut buf = [0u8; MSG_MAX];
        while let Some(m) = self.api.recv(&mut buf) {
            let u16_at = |i: usize| u16::from_le_bytes([m[i], m[i + 1]]);
            let u32_at = |i: usize| u32::from_le_bytes([m[i], m[i + 1], m[i + 2], m[i + 3]]);
            match m.first() {
                Some(b'W') => {
                    self.flush();
                    self.inc.on = false;
                    self.delete_pack();
                    self.send(b"w");
                    self.dirty = true;
                }
                Some(b'P') if m.len() == 5 => {
                    if self.loading.is_none() {
                        // a new map: the old one goes first (not all its cells may be replaced)
                        self.delete_pack();
                    }
                    self.loading = Some((u16_at(1), u16_at(3)));
                    self.dirty = true;
                }
                Some(b'F') if (2..=21).contains(&m.len()) => {
                    self.flush();
                    let n = m.len() - 1;
                    self.inc.name[..n].copy_from_slice(&m[1..]);
                    self.inc.nlen = n;
                    self.inc.on = true;
                    let _ = self.api.file_write(self.name(), &[], false);
                }
                Some(b'f') if self.inc.on => {
                    let chunk = unsafe { &mut *core::ptr::addr_of_mut!(CHUNK) };
                    for &b in &m[1..] {
                        if self.inc.len == FILE_CHUNK {
                            self.flush();
                        }
                        chunk[self.inc.len] = b;
                        self.inc.len += 1;
                    }
                }
                Some(b'c') => {
                    self.flush();
                    self.inc.on = false;
                    self.send(b"a");
                }
                Some(b'Z') => {
                    self.loading = None;
                    self.load_pack();
                    if self.me.is_none() {
                        self.centre = (0, 0);
                    }
                    self.send(b"k");
                    self.dirty = true;
                }
                Some(b'G') if m.len() == 11 => {
                    self.route_want = usize::from(u16_at(1)).min(ROUTE);
                    self.route_got = 0;
                    self.route_metres = u32_at(3);
                    ram().path_n = 0;
                }
                Some(b'g') if m.len() >= 2 && self.route_want > 0 => {
                    let at = usize::from(m[1]) * PER_MSG;
                    let r = ram();
                    for (k, c) in m[2..].chunks_exact(4).enumerate() {
                        let i = at + k;
                        if i < self.route_want {
                            r.path[i] = (i16::from_le_bytes([c[0], c[1]]), i16::from_le_bytes([c[2], c[3]]));
                            self.route_got += 1;
                        }
                    }
                    if self.route_got >= self.route_want {
                        r.path_n = self.route_want;
                        self.route_want = 0;
                        let mut b = [0u8; 16];
                        let mut t = [0u8; 40];
                        let d = fmt_dist(self.route_metres as i32, &mut b);
                        let mut n = 0;
                        for part in ["Route: ", d] {
                            t[n..n + part.len()].copy_from_slice(part.as_bytes());
                            n += part.len();
                        }
                        self.set_note(core::str::from_utf8(&t[..n]).unwrap_or(""));
                        self.start_guide(Guide::Path { back: false });
                        self.dirty = true;
                    }
                }
                Some(b'E') => {
                    let t = cmf_wp2_ui::text::fit(&m[1..], 40);
                    let mut b = [0u8; 40];
                    b[..t.len()].copy_from_slice(t.as_bytes());
                    self.set_note(core::str::from_utf8(&b[..t.len()]).unwrap_or(""));
                    self.dirty = true;
                }
                Some(b'?') => self.send(b"R"),
                _ => {}
            }
        }
    }

    fn name(&self) -> &str {
        core::str::from_utf8(&self.inc.name[..self.inc.nlen]).unwrap_or("x")
    }

    fn flush(&mut self) {
        if self.inc.on && self.inc.len > 0 {
            let chunk = unsafe { &*core::ptr::addr_of!(CHUNK) };
            let _ = self.api.file_write(self.name(), &chunk[..self.inc.len], true);
        }
        self.inc.len = 0;
    }

    /// Back (the side button): to the map, else closes.
    fn back(&mut self) -> bool {
        if self.screen == Screen::Map {
            return false;
        }
        self.screen = Screen::Map;
        self.dirty = true;
        true
    }
}

fn main(api: &'static Api) -> i32 {
    ram::init();
    let mut ui = Ui {
        api,
        s: api,
        motor: Motor::new(api),
        screen: Screen::Map,
        pack: None,
        frame: None,
        zoom: START_ZOOM,
        centre: (0, 0),
        follow: true,
        me: None,
        fix_at: 0,
        course: None,
        sats: 0,
        gps_on: false,
        tracking: false,
        track_t0: 0,
        guide: Guide::None,
        seg: 0,
        on_path: None,
        off_said: false,
        arrived: false,
        sheet: ((0, 0), None),
        list_top: 0,
        down: None,
        held: false,
        note: [0; 48],
        n_note: 0,
        note_at: 0,
        loading: None,
        inc: Incoming { name: [0; 20], nlen: 0, len: 0, on: false },
        route_want: 0,
        route_got: 0,
        route_metres: 0,
        dirty: true,
    };
    {
        let b = unsafe { &mut *core::ptr::addr_of_mut!(SAVE) };
        let n = api.load(b);
        if n > 0 {
            marks().parse(&b[..n as usize]);
        }
    }
    api.own_back(true);
    ui.load_pack();
    // the watch's own GPS while the app is open (the loader turns it off on every way out)
    ui.gps_on = api.gps(true) >= 0;
    ui.draw();
    ui.send(b"R");
    loop {
        let e = api.event(ui.wait());
        ui.motor.update();
        ui.timers();
        if let Some(e) = e {
            match e.kind {
                BUTTON => {
                    if !ui.back() {
                        ui.motor.off();
                        let _ = api.gps(false);
                        return 0;
                    }
                }
                PAUSE => loop {
                    match api.event(u32::MAX) {
                        Some(e) if e.kind == RESUME => {
                            ui.dirty = true;
                            break;
                        }
                        Some(e) if e.kind == BUTTON => return 0,
                        _ => {}
                    }
                },
                _ => ui.handle(e),
            }
        }
        // a burst (crown, messages, GPS): one redraw for all of it
        while let Some(e) = api.event(0) {
            if e.kind == BUTTON {
                if !ui.back() {
                    let _ = api.gps(false);
                    return 0;
                }
                continue;
            }
            ui.handle(e);
        }
        if ui.dirty {
            ui.draw();
        }
    }
}
