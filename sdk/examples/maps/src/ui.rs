//! Offline Maps on the watch: the saved map drawn as Maps draws its own (render.rs, a few cells of pack.rs at a
//! time), you on it from the watch's GPS (on while the app is open; the iPhone's, `l`/`L`, while it has no fix), markers, guidance to a marker or along a path
//! (a route the phone found, or your track back to its start), all without the phone once the map is saved.
//!
//! Map: crown zooms, a drag pans, a tap moves there, a tap on a marker or a named place opens its card (Directions:
//! car, bike or walk, up to three routes to pick; Go straight; a pin), a hold on the map opens that place.
//! Buttons: ◎ (left) back to you (held: the watch's GPS only, or back to the iPhone's and the watch's); search
//! (find.rs); along the bottom Search, Track (record your path / stop), Back
//! (guide back along the track to its start), List (markers, stop guiding, clear the track, delete the map).

use crate::geo::{self, dist, follow, p32, Frame, OnPath, ARRIVED, OFF};
use crate::marks::{self, Marks};
use crate::pack::{cell_name, Pack};
use crate::ram::{self, ram, ROUTE};
use crate::render::{self, View, BG, ROUTE as BLUE};
use crate::scene::Scene;
use cmf_wp2_app::storage::{Clean, Cleaned};
use cmf_wp2_app::{Api, Event, BUTTON, CROWN, FILE_CHUNK, GPS, MESSAGE, MSG_MAX, PAUSE, RESUME, TOUCH_DOWN, TOUCH_UP};
use cmf_wp2_ui::color::{rgb565, Rgb, BLACK};
use cmf_wp2_ui::draw::{disc, line, ring, rrect};
use cmf_wp2_ui::font;
use cmf_wp2_ui::haptic::{self, Motor};
use cmf_wp2_ui::motion::{cos, sin};
use cmf_wp2_ui::text::int;
use cmf_wp2_ui::{Surface, C};

#[path = "find.rs"]
mod find;
use find::{Hit, HITS, QUERY};
#[path = "trip.rs"]
mod trip;
use trip::Trip;

cmf_wp2_app::entry!(main);
cmf_wp2_app::app_icon!();

const W: u16 = 0xFFFF;
const WHITE: Rgb = Rgb(0xFF, 0xFF, 0xFF);
const RED: Rgb = Rgb(0xD7, 0x19, 0x21);
const ORANGE: Rgb = Rgb(0xFF, 0x8C, 0x00);
/// How often the phone is asked where it is (`l`).
const ASK_MS: u32 = 2000;
const CARD: Rgb = Rgb(0x14, 0x14, 0x14);
const BTN: Rgb = Rgb(0x10, 0x10, 0x10);
const TRACK_C: Rgb = Rgb(0xFF, 0xD0, 0x40);
const GREY: u16 = rgb565(0x8A, 0x8A, 0x8A);
const DIM: Rgb = Rgb(0x50, 0x50, 0x50);

/// Zooms, px a metre × 256: the crown steps through them.
const ZOOMS: [i32; 9] = [6, 10, 16, 26, 42, 68, 110, 180, 290];
const START_ZOOM: usize = 6;
/// Buttons: (x, y, r).
const B_TRACK: (i32, i32, i32) = (184, 417, 27);
const B_BACK: (i32, i32, i32) = (282, 417, 27);
const B_LIST: (i32, i32, i32) = (367, 367, 27);
const B_ME: (i32, i32, i32) = (40, C, 26);
const B_FIND: (i32, i32, i32) = (99, 367, 27);
/// ◎ held this long: the watch's GPS only, or back to both.
const GPS_HOLD_MS: u32 = 800;
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
    /// "Delete this map?" (or, `clean`, everything)
    Confirm,
    /// deleting every file
    Cleaning,
    /// search's keyboard, its places, route planning (find.rs)
    Keys,
    Results,
    /// directions, a mode's routes, the route before Start, navigating (trip.rs)
    Ways,
    Routes,
    Preview,
    Drive,
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
    /// the last fix from the watch's GPS, the phone's (`L`), and when the phone was last asked (`l`)
    watch_at: Option<u32>,
    phone_at: Option<u32>,
    asked_at: u32,
    /// ◎ blinks while the watch's GPS looks for satellites (each GPS second)
    blink: bool,
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
    /// Confirm asks about cleaning everything, not the map
    confirm_clean: bool,
    /// the sheet's named place (n 0: none), search's query and places, route planning (find.rs)
    place: Hit,
    query: [u8; QUERY],
    qn: usize,
    sym: bool,
    hits: [Hit; HITS],
    n_hits: usize,
    /// directions and turn by turn
    trip: Trip,
    /// ◎ held: the watch's GPS only (the iPhone's isn't asked)
    watch_only: bool,
    clean: Option<Clean<'static>>,
    /// the phone asked for the clean (it is told how it went)
    clean_for_phone: bool,
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
        let _ = self.api.file_delete("n");
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
            (Screen::Cleaning, _) => self.draw_cleaning(),
            (Screen::Keys, _) => self.draw_keys(),
            (Screen::Results, _) => self.draw_results(),
            (Screen::Ways, _) => self.draw_ways(),
            (Screen::Routes, _) => self.draw_routes(),
            (Screen::Preview, _) => self.draw_preview(),
            (Screen::Drive, _) => self.draw_drive(),
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
        // (navigating: where you are low on the panel and the map turned, so further round it)
        let reach = if v.heading != 0 || v.screen != (C, C) { 330 } else { 233 };
        let half = reach * 256 / v.k.max(1) + 40;
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
                    if li > first && v.heading == 0 {
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
        let c = if self.watch_ok() || self.phone_ok() { self.source(false) } else { DIM };
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
        } else if self.watch_ok() {
            let mut ib = [0u8; 11];
            put("GPS \u{b7} ", &mut n);
            put(int(i32::from(self.sats), &mut ib), &mut n);
            put(" satellites", &mut n);
            (core::str::from_utf8(&line_b[..n]).unwrap_or(""), Some(self.source(false)))
        } else if self.phone_ok() {
            ("iPhone GPS", Some(self.source(false)))
        } else if !self.gps_on {
            ("GPS needs loader 4", Some(DIM))
        } else {
            ("Looking for GPS\u{2026}", Some(RED))
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
        // search: a magnifier
        self.button(B_FIND, false);
        let (x, y, _) = B_FIND;
        ring(&mut s, x - 3, y - 3, 7, 10, WHITE, BTN);
        line(&mut s, x + 4, y + 4, x + 11, y + 11, 4, WHITE, BTN);
        self.draw_me_button();
    }

    /// ◎ back to you, in the colour of where the position comes from (`source`); the centre dot while following.
    fn draw_me_button(&self) {
        let mut s = self.s;
        self.button(B_ME, false);
        let (x, y, _) = B_ME;
        let c = self.source(true);
        ring(&mut s, x, y, 9, 12, c, BTN);
        if self.follow {
            disc(&mut s, x, y, 5, c, BTN);
        }
    }

    /// ◎ held: the watch's GPS only, or the iPhone's and the watch's again.
    fn toggle_watch_only(&mut self) {
        self.watch_only = !self.watch_only;
        self.motor.play(haptic::DOUBLE);
        if self.watch_only {
            self.phone_at = None;
            self.set_note("Watch GPS only");
        } else {
            self.asked_at = self.api.now().wrapping_sub(ASK_MS);
            self.set_note("iPhone and watch GPS");
        }
        self.dirty = true;
    }

    /// The watch's GPS found us while it is on: its position holds from then on, fix or not.
    fn watch_ok(&self) -> bool {
        self.gps_on && self.watch_at.is_some()
    }

    /// A fix from the watch in the last 5 s.
    fn watch_live(&self) -> bool {
        self.watch_at.is_some_and(|t| self.api.now().wrapping_sub(t) < 5000)
    }

    fn phone_ok(&self) -> bool {
        self.phone_at.is_some_and(|t| self.api.now().wrapping_sub(t) < 6000)
    }

    /// Where the position comes from, as Maps' ◎: blue the iPhone's GPS (the watch's off), orange both (orange and
    /// blue in turn: the iPhone's while the watch looks for satellites), red the watch's only (blinking: it looks,
    /// no iPhone).
    /// `blink`: the looking ones blink.
    fn source(&self, blink: bool) -> Rgb {
        let (p, w) = (self.phone_ok(), self.watch_ok());
        let c = match (p, w) {
            (true, false) if !self.gps_on => BLUE,
            (true, _) => ORANGE,
            (false, _) if self.gps_on => RED,
            (false, _) => DIM,
        };
        match (blink && self.gps_on && !self.watch_live() && self.blink, p) {
            (true, true) => BLUE,
            (true, false) => DIM,
            _ => c,
        }
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
            None if self.place.n > 0 => self.place.name(),
            None => "This place",
        };
        font::centre(&mut s, C, 80, title, W);
        let mut b = [0u8; 16];
        let mut line = [0u8; 48];
        let mut n = 0;
        let kind = if self.place.n > 0 && i.is_none() { find::kind_name(self.place.kind) } else { "" };
        let d = match self.me {
            Some(me) => fmt_dist(dist(me, p), &mut b),
            None => "no GPS fix yet",
        };
        for part in [kind, if kind.is_empty() { "" } else { " \u{b7} " }, d] {
            let k = part.len().min(48 - n);
            line[n..n + k].copy_from_slice(&part.as_bytes()[..k]);
            n += k;
        }
        font::centre(&mut s, C, 114, core::str::from_utf8(&line[..n]).unwrap_or(""), GREY);
        for (k, label) in self.sheet_buttons().iter().enumerate() {
            if label.is_empty() {
                continue;
            }
            let (x, y) = Self::sheet_box(k);
            let red = label.starts_with("Delete");
            let fill = if red { RED } else if k == 0 { BLUE } else { CARD };
            rrect(&mut s, x, y, 150, 62, 31, fill, BLACK);
            font::centre(&mut s, x + 75, y + 20, label, W);
        }
    }

    fn sheet_buttons(&self) -> [&'static str; 4] {
        match self.sheet.1 {
            Some(_) => ["Directions", "Go straight", "Delete", ""],
            None => ["Directions", "Go straight", "Add a pin", ""],
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
        out[n] = (4, 0);
        n += 1;
        (out, n)
    }

    fn draw_list(&mut self) {
        let mut s = self.s;
        Surface::rect(&mut s, 0, 0, 466, 466, 0);
        font::centre(&mut s, C, 34, "Pins", W);
        let (rows, n) = self.rows();
        if marks().n == 0 && self.list_top == 0 && n <= 2 {
            font::centre(&mut s, C, 300, "No pins yet: Mark drops one", GREY);
        }
        let mut b = [0u8; 16];
        for (k, &(kind, i)) in rows[..n].iter().enumerate().skip(self.list_top).take(ROWS) {
            let y = ROW_Y + (k - self.list_top) as i32 * ROW_H;
            let (x0, x1) = cmf_wp2_ui::geom::chord(y, y + ROW_H - 8, 30);
            let fill = if kind >= 3 { Rgb(0x3A, 0x10, 0x12) } else { CARD };
            rrect(&mut s, x0, y, x1 - x0, ROW_H - 8, 20, fill, BLACK);
            let t = match kind {
                0 => marks().v[i].name(),
                1 => "Stop guiding",
                2 => "Clear the track",
                3 => "Delete the map",
                _ => "Clean storage",
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
        if self.confirm_clean {
            font::centre(&mut s, C, 134, "Clean all storage?", W);
            font::centre(&mut s, C, 166, "Every file Offline Maps keeps:", GREY);
            font::centre(&mut s, C, 192, "the map, leftovers and pins", GREY);
        } else {
            font::centre(&mut s, C, 150, "Delete this map?", W);
            font::centre(&mut s, C, 182, "Pins and the track stay", GREY);
        }
        rrect(&mut s, C - 150, 240, 140, 64, 32, RED, BLACK);
        font::centre(&mut s, C - 80, 260, "Delete", W);
        rrect(&mut s, C + 10, 240, 140, 64, 32, CARD, BLACK);
        font::centre(&mut s, C + 80, 260, "Keep", W);
    }

    fn draw_loading(&self, done: u16, total: u16) {
        let mut s = self.s;
        Surface::rect(&mut s, 0, 0, 466, 466, 0);
        // (the icon's memory holds the map here: the name alone)
        font::centre(&mut s, C, 160, "Offline Maps", W);
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
            // under cover: the watch's last position is held (as the stock workouts do), never dropped for the
            // iPhone's; ◎ blinks till the fix is back
            self.blink = !self.blink;
            let map = self.screen == Screen::Map && self.loading.is_none();
            if map && !self.dirty {
                // only ◎ blinks
                self.draw_me_button();
                self.api.show();
            }
            return;
        }
        self.watch_at = Some(self.api.now());
        self.place(f.lat_e7, f.lon_e7);
    }

    /// The phone's fix (`L`): used while the watch's GPS has none.
    fn phone_fix(&mut self, lat_e7: i32, lon_e7: i32) {
        let had = self.phone_ok();
        self.phone_at = Some(self.api.now());
        if !self.watch_ok() {
            self.place(lat_e7, lon_e7);
        } else if !had {
            // ◎ red to orange
            self.dirty |= self.screen == Screen::Map;
        }
    }

    /// You are at this place now (a fix from the watch or the phone).
    fn place(&mut self, lat_e7: i32, lon_e7: i32) {
        if self.frame.is_none() {
            // no map: a frame round this first fix
            let a = ((i64::from(lat_e7) * 65536 / 3_600_000_000).rem_euclid(65536)) as u16;
            let mm = (111_320_000i64 * i64::from(cos(a)) / 16384) as u32;
            self.frame = Some(Frame { lat0_e7: lat_e7, lon0_e7: lon_e7, mm_per_deg_lon: mm });
        }
        let p = self.frame.unwrap().xy(lat_e7, lon_e7);
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
            if !self.trip.driving {
                self.motor.play(haptic::DONE);
            }
        }
        self.nav_update();
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
        self.start_backtrack();
    }

    /// Routes to `to` from the phone (mode 0 car, 1 bike, 2 walk): their times (`O`, `o`), then the one picked as
    /// `G` + `g`.
    /// False: not sent (no iPhone).
    fn ask_route(&mut self, mode: u8, to: (i32, i32)) -> bool {
        let Some(f) = self.frame else { return false };
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
        self.route_want = 0;
        self.api.send(&m) >= 0
    }

    // --- input

    fn press(&mut self, x: i32, y: i32) -> bool {
        if hit(B_TRACK, x, y) {
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
                self.end_drive();
            } else {
                self.backtrack();
            }
        } else if hit(B_FIND, x, y) {
            self.motor.play(haptic::TICK);
            self.screen = Screen::Keys;
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
            // the banner: back to turn by turn, else guiding ends
            self.motor.play(haptic::TICK);
            if self.trip.driving {
                self.screen = Screen::Drive;
            } else {
                self.guide = Guide::None;
            }
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
            self.place = Hit::NONE;
            self.screen = Screen::Sheet;
            return;
        }
        // a named place drawn there: its card
        if let Some(h) = self.name_at(x, y) {
            self.motor.play(haptic::TICK);
            self.open_place(h);
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
                    (0, _) => self.ask_ways(),
                    (1, _) => self.start_guide(Guide::To(p, i)),
                    (2, Some(i)) => {
                        marks().remove(i);
                        self.save_marks();
                        if matches!(self.guide, Guide::To(_, Some(_))) {
                            self.guide = Guide::None;
                        }
                    }
                    (2, None) => {
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

    /// The list or the search's places by `by` rows.
    fn scroll(&mut self, by: i32) {
        let n = if self.screen == Screen::Results { self.n_hits } else { self.rows().1 };
        let t = (self.list_top as i32 + by).clamp(0, (n as i32 - ROWS as i32).max(0)) as usize;
        if t != self.list_top {
            self.list_top = t;
            self.dirty = true;
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
                self.place = Hit::NONE;
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
            (3, _) => {
                self.confirm_clean = false;
                self.screen = Screen::Confirm;
            }
            _ => {
                self.confirm_clean = true;
                self.screen = Screen::Confirm;
            }
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
            CROWN if matches!(self.screen, Screen::List | Screen::Results) => self.scroll(i32::from(e.delta).signum()),
            CROWN if self.screen == Screen::Drive => self.drive_crown(i32::from(e.delta).signum()),
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
                    // a drag scrolls the lists, a row at a time
                    Screen::List | Screen::Results if dy.abs() > DRAG => self.scroll(-(dy + dy.signum() * ROW_H / 2) / ROW_H),
                    Screen::List => self.list_tap(y),
                    Screen::Results => self.results_tap(y),
                    Screen::Keys => self.keys_tap(x, y),
                    Screen::Ways => self.ways_tap(y),
                    Screen::Routes => self.routes_tap(y),
                    Screen::Preview => self.preview_tap(y),
                    Screen::Drive => self.drive_tap(),
                    Screen::Confirm => {
                        self.screen = Screen::Map;
                        if (230..320).contains(&y) && x < C {
                            if self.confirm_clean {
                                self.start_clean(false);
                            } else {
                                self.delete_pack();
                                self.set_note("Map deleted");
                            }
                        }
                    }
                    Screen::Cleaning => {}
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
                if hit(B_ME, x, y) {
                    if now.wrapping_sub(t) >= GPS_HOLD_MS {
                        self.toggle_watch_only();
                    } else {
                        // (◎ toggles at GPS_HOLD_MS, not a place)
                        self.held = false;
                    }
                } else if ![B_TRACK, B_BACK, B_LIST, B_FIND].iter().any(|&b| hit(b, x, y)) {
                    self.motor.play(haptic::TICK);
                    self.sheet = (p, None);
                    self.place = Hit::NONE;
                    self.screen = Screen::Sheet;
                    self.dirty = true;
                }
            }
        }
        // where the phone is, every 2 s (it answers `L`; nothing while a map comes or the storage is cleaned, nor in
        // watch-only mode)
        if now.wrapping_sub(self.asked_at) >= ASK_MS && self.loading.is_none() && self.clean.is_none() && !self.watch_only {
            self.asked_at = now;
            self.send(b"l");
        }
        // the phone's fix gone stale: ◎ again
        if self.phone_at.is_some_and(|t| now.wrapping_sub(t) >= 6000) {
            self.phone_at = None;
            self.dirty |= self.screen == Screen::Map;
        }
        self.trip_timers(now);
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
        if let Some((x, y, t)) = self.down {
            if !self.held {
                w = w.min(left(t.wrapping_add(if hit(B_ME, x, y) && now.wrapping_sub(t) >= HOLD_MS { GPS_HOLD_MS } else { HOLD_MS })));
            }
        }
        if self.n_note > 0 {
            w = w.min(left(self.note_at.wrapping_add(NOTE_MS)));
        }
        w = w.min(left(self.asked_at.wrapping_add(ASK_MS)));
        if let Some(t) = self.phone_at {
            w = w.min(left(t.wrapping_add(6000)));
        }
        w
    }

    // --- the phone

    fn messages(&mut self) {
        let mut buf = [0u8; MSG_MAX];
        while let Some(m) = self.api.recv(&mut buf) {
            let u16_at = |i: usize| u16::from_le_bytes([m[i], m[i + 1]]);
            let u32_at = |i: usize| u32::from_le_bytes([m[i], m[i + 1], m[i + 2], m[i + 3]]);
            if self.trip_message(m) {
                continue;
            }
            match m.first() {
                Some(b'L') if !self.watch_only => {
                    // the phone's fix (lat, lon e7, accuracy m), or none
                    if m.len() == 11 {
                        let i32_at = |i: usize| u32_at(i) as i32;
                        self.phone_fix(i32_at(1), i32_at(5));
                    } else if self.phone_at.take().is_some() {
                        self.dirty |= self.screen == Screen::Map;
                    }
                }
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
                        self.route_ready();
                    }
                }
                Some(b'E') => {
                    self.trip.loading = None;
                    let t = cmf_wp2_ui::text::fit(&m[1..], 40);
                    let mut b = [0u8; 40];
                    b[..t.len()].copy_from_slice(t.as_bytes());
                    self.set_note(core::str::from_utf8(&b[..t.len()]).unwrap_or(""));
                    self.dirty = true;
                }
                Some(b'?') => self.send(b"R"),
                Some(b'X') => self.start_clean(true),
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

    /// Everything Offline Maps keeps goes: its files (the map, a v3 picture map, anything an older version left)
    /// and the pins. The track and a route are in RAM.
    fn start_clean(&mut self, phone: bool) {
        self.flush();
        self.inc.on = false;
        self.loading = None;
        self.clean_for_phone = phone;
        match Clean::new(self.api) {
            Ok(c) => {
                self.clean = Some(c);
                self.screen = Screen::Cleaning;
            }
            Err(_) => {
                // no storage service (an older loader): what this version knows of
                self.delete_pack();
                self.end_clean(Cleaned::Partial(0));
            }
        }
        self.dirty = true;
    }

    fn clean_step(&mut self) {
        let Some(c) = self.clean.as_mut() else { return };
        if let Some(r) = c.step() {
            self.clean = None;
            self.end_clean(r);
        }
    }

    fn end_clean(&mut self, r: Cleaned) {
        self.pack = None;
        *marks() = Marks::new();
        let _ = self.api.save(&[]);
        if matches!(self.guide, Guide::To(_, Some(_))) {
            self.guide = Guide::None;
        }
        let mut t = [0u8; 40];
        let mut b = [0u8; 11];
        let (head, n) = match r {
            Cleaned::Done(n) => ("Cleaned ", n),
            Cleaned::Partial(n) => ("Some left, cleaned ", n),
            Cleaned::Failed(_) => ("Only the map deleted", 0),
        };
        let mut k = 0;
        let num = int(i32::from(n), &mut b);
        for part in [head, if matches!(r, Cleaned::Failed(_)) { "" } else { num }, if matches!(r, Cleaned::Failed(_)) { "" } else { " files" }] {
            for &ch in part.as_bytes() {
                if k < 40 {
                    t[k] = ch;
                    k += 1;
                }
            }
        }
        if matches!(r, Cleaned::Failed(_)) {
            self.delete_pack();
        }
        self.set_note(core::str::from_utf8(&t[..k]).unwrap_or(""));
        if self.clean_for_phone {
            let mut m = [b'x', 0, 0, 0];
            m[1..3].copy_from_slice(&n.to_le_bytes());
            m[3] = match r {
                Cleaned::Done(_) => 0,
                Cleaned::Partial(_) => 1,
                Cleaned::Failed(_) => 2,
            };
            self.send(&m);
        }
        self.screen = Screen::Map;
        self.dirty = true;
    }

    fn draw_cleaning(&self) {
        let mut s = self.s;
        Surface::rect(&mut s, 0, 0, 466, 466, 0);
        font::centre(&mut s, C, 200, "Cleaning storage\u{2026}", W);
        let n = self.clean.as_ref().map(|c| c.deleted).unwrap_or(0);
        let mut b = [0u8; 11];
        font::centre(&mut s, C, 232, int(i32::from(n), &mut b), GREY);
    }

    /// Back (the side button): a screen back, on the map closes.
    fn back(&mut self) -> bool {
        self.screen = match self.screen {
            Screen::Cleaning => return true,
            Screen::Map => return false,
            Screen::Results => Screen::Keys,
            Screen::Ways => Screen::Sheet,
            Screen::Routes => Screen::Ways,
            Screen::Preview if self.trip.n_alts[usize::from(self.trip.mode)] > 1 => Screen::Routes,
            Screen::Preview => Screen::Ways,
            _ => Screen::Map,
        };
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
        watch_at: None,
        phone_at: None,
        asked_at: 0,
        blink: false,
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
        confirm_clean: false,
        place: Hit::NONE,
        query: [0; QUERY],
        qn: 0,
        sym: false,
        hits: [Hit::NONE; HITS],
        n_hits: 0,
        trip: Trip::new(),
        watch_only: false,
        clean: None,
        clean_for_phone: false,
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
        let e = api.event(if ui.clean.is_some() { 0 } else { ui.wait() });
        if ui.clean.is_some() {
            ui.clean_step();
            if ui.clean.is_some() && api.now() % 8 == 0 {
                ui.dirty = true;
            }
        }
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
