//! Maps on the watch, as Google Maps behaves.
//!
//! - Home: the map round where we are, full screen. ◎ (right) comes back to us, the bottom button searches, the left
//!   one (after a search) lists its places, which are red pins on the map; ✕ (top) clears them and the map round us
//!   comes back with its own places. A tap on a pin opens that place; a long
//!   press drops a pin there and opens it. A tap near the rim moves the map that way (the right swipe is the watch's
//!   back, so the map can't be dragged); the crown zooms, and stops at the nearest and the farthest zoom.
//! - Find: Search and Area (a town or a district: the map goes there), then kinds of places near where the map is
//!   (food, cafés, fuel, hotels …).
//! - Search: a QWERTY keyboard; its "dial" key turns it into the rotary one (a tap on the text comes back), as Spotify.
//! - Results: the places as red pins on the map, the one the crown is on in a sheet at the bottom (a tap opens it);
//!   pulled up, the sheet is the list of them all (List).
//! - Place: the map round it with its pin, and a card: name, kind and distance, address, Directions.
//! - Ways: car, bike and on foot with the time and arrival. Tap one; with two or three routes that way they are
//!   listed to pick from (Routes), as Google's alternatives.
//! - Route: the whole route on the map with the time, distance and arrival; the crown steps through the turns, each
//!   in words on the map round it; Start.
//! - Navigating: the full turn view (time and arrival, the arrow, what to do, how far) or, after a tap, the route
//!   heading-up. It buzzes as a turn comes (nav view.rs). The button asks before it ends navigation.
//!
//! The button goes back a screen; on Home it closes Maps. The phone sends the map as data the watch draws (proto.rs).

use crate::proto::{CATEGORIES, Got, MODES, Model, PICKED, Place, Way};
use crate::render::{self, View};
use crate::scene::{Ev, Scene};
use crate::state::{Effect, Nav, instruction};
use crate::view::{self, ZOOMS};
use cmf_wp2_app::{Api, BUTTON, CROWN, MESSAGE, MSG_MAX, PAUSE, RESUME, TOUCH_DOWN, TOUCH_UP};
use cmf_wp2_ui::ask::Ask;
use cmf_wp2_ui::color::{BLACK, Rgb, rgb565};
use cmf_wp2_ui::dial::{Act, Dial, Key};
use cmf_wp2_ui::draw::{arc, disc, line, ring, rrect};
use cmf_wp2_ui::font;
use cmf_wp2_ui::geom::chord;
use cmf_wp2_ui::haptic::{self, Motor};
use cmf_wp2_ui::text::int;
use cmf_wp2_ui::{C, Surface};

cmf_wp2_app::entry!(main);
cmf_wp2_app::app_icon!();

const W: u16 = 0xFFFF;
const RED: Rgb = Rgb(0xD7, 0x19, 0x21);
/// Maps' accent, a map app's blue (Exit stays red).
const BLUE: Rgb = render::ROUTE;
const WHITE: Rgb = Rgb(0xFF, 0xFF, 0xFF);
const CARD: Rgb = Rgb(0x1C, 0x1C, 0x1C);
const KEYC: Rgb = Rgb(0x2A, 0x2A, 0x2A);
const BUTTONC: Rgb = Rgb(0x10, 0x10, 0x10);
const SOFT: u16 = rgb565(0xC8, 0xC8, 0xC8);
const GREY: u16 = rgb565(0x8A, 0x8A, 0x8A);
const DIM: u16 = rgb565(0x3A, 0x3A, 0x3A);

static mut SCENE: Scene = Scene::new();

fn scene() -> &'static mut Scene {
    unsafe { &mut *core::ptr::addr_of_mut!(SCENE) }
}
fn model() -> &'static mut Model {
    &mut crate::ram::ram().model
}
fn nav() -> &'static mut Nav {
    &mut crate::ram::ram().nav
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Screen {
    Home,
    Search,
    Dial,
    Results,
    List,
    Find,
    Place,
    Ways,
    Pick,
    Route,
    Nav,
    /// a place's details, pulled up from its card
    About,
}

/// Text wrapped into About's lines.
fn push<'a>(lines: &mut [(&'a str, u16); 40], n: &mut usize, t: &'a str, c: u16) {
    let mut out = [""; 16];
    let k = if t.is_empty() { 1 } else { font::wrap(t, ABOUT_W, &mut out) };
    for l in &out[..k] {
        if *n < lines.len() {
            lines[*n] = (l, c);
            *n += 1;
        }
    }
}

/// The screens that show the map.
fn has_map(s: Screen) -> bool {
    matches!(
        s,
        Screen::Home | Screen::Results | Screen::Place | Screen::Pick | Screen::Route | Screen::Nav
    )
}

// --- Home: the list (after a search), ◎ and the bottom button (centre x, centre y, radius)
const LIST_L: (i32, i32, i32) = (64, C, 36);
/// ✕: the search's pins cleared (Home and Results, while there are any).
const CLEAR: (i32, i32, i32) = (C, 50, 26);
const ME: (i32, i32, i32) = (402, C, 36);
const LIST: (i32, i32, i32, i32) = (C, 412, 120, 56);
/// A tap this far from the centre (off the buttons) moves the map a third of the panel that way.
const EDGE_R: i32 = 120;
/// Home's zooms: px a metre × 256 (5 m … 0.8 m a pixel).
const HOME_K: [i32; 5] = [48, 80, 128, 208, 320];
const HOME_ZOOMS: u8 = HOME_K.len() as u8;
/// The route's steps up close.
const STEP_K: i32 = 208;
// --- Route
const START: (i32, i32, i32) = (384, 50, 170);
const ROUTE_TOP: i32 = 104;
// --- Results
const ROW_Y: i32 = 82;
const ROW_H: i32 = 66;
const ROWS: usize = 5;
// --- Place: the three ways
const WAY_Y: [i32; 3] = [118, 202, 286];
const WAY_H: i32 = 76;
// --- keyboard (Spotify's)
const KH: i32 = 58;
const KEY_Y: [i32; 4] = [184, 242, 300, 358];
const QUERY: usize = 40;
/// `asked` for the route's map (it comes with `D`, round the route).
const ROUTE_SCENE: (i32, i32) = (i32::MIN, i32::MIN);
/// `asked` for a map round a place or an area (`F`, `Z`): it is at the scene's centre.
const FOCUS_SCENE: (i32, i32) = (i32::MIN, i32::MAX);
/// A touch held this long on the map drops a pin.
const HOLD_MS: u32 = 600;
// --- Results: the sheet under the map
const SHEET_Y: i32 = 318;
// --- Place: the card under the map
const CARD_Y: i32 = 262;
const DIRS: (i32, i32, i32) = (384, 56, 230);
/// Route: "+" beside Start, a stop on the way (searched for, then "Add stop" on its card)
const STOP_BTN: (i32, i32, i32) = (C - 132, 409, 25);
/// The place on the map over the card.
const PLACE_Y: i32 = 150;
// --- Find: Search and Area, then the kinds of places (2 a row)
const FIND_Y: i32 = 70;
const FIND_H: i32 = 54;
const CAT_Y: i32 = 138;
const CAT_H: i32 = 62;
const CAT_W: i32 = 166;
const CAT_C: [Rgb; 8] = [
    Rgb(0xF2, 0x9E, 0x3D),
    Rgb(0xC8, 0x8A, 0x5A),
    Rgb(0x4B, 0x8B, 0xF5),
    Rgb(0xE0, 0x6A, 0xB4),
    Rgb(0x34, 0xA8, 0x53),
    Rgb(0x8E, 0x8C, 0xD8),
    Rgb(0x13, 0xB5, 0xC7),
    Rgb(0xEA, 0x43, 0x35),
];
// --- Routes to pick
/// Pick: the ways on a map above, listed under it
const PICK_Y: i32 = 262;
const PICK_H: i32 = 58;
/// About: a line's height, the column's width (where the panel is that wide: ABOUT_Y)
const ABOUT_H: i32 = 30;
const ABOUT_W: i32 = 300;
const ABOUT_Y: (i32, i32) = (56, 372);
/// No part of a scene for this long: ask for it again.
const STALL_MS: u32 = 4000;
/// While a scene comes, the map is drawn again at most this often.
/// Navigation's frames: the next waits this long after the last (the panel takes a frame from the buffer we draw
/// into some ms after `show`).
const FRAME_MS: u32 = 80;
const REDRAW_MS: u32 = 600;

#[derive(Clone, Copy, PartialEq)]
enum K {
    Ch(u8),
    Del,
    Sym,
    Space,
    Go,
    Dial,
}

/// One keyboard row: up to 10 keys `(key, width)`, centred.
fn row(r: usize, sym: bool) -> ([(K, i32); 10], usize) {
    let mut keys = [(K::Space, 0); 10];
    let chars = |s: &[u8], keys: &mut [(K, i32); 10]| {
        for (i, &c) in s.iter().enumerate() {
            keys[i] = (K::Ch(c), 44);
        }
        s.len()
    };
    let n = match (r, sym) {
        (0, false) => chars(b"qwertyuiop", &mut keys),
        (1, false) => chars(b"asdfghjkl", &mut keys),
        (0, true) => chars(b"1234567890", &mut keys),
        (1, true) => chars(b"&-'.,!?()", &mut keys),
        (2, _) => {
            let n = chars(if sym { b"#+/:\"@*" } else { b"zxcvbnm" }, &mut keys);
            keys[n] = (K::Del, 58);
            n + 1
        }
        _ => {
            keys[0] = (K::Sym, 62);
            keys[1] = (K::Dial, 62);
            keys[2] = (K::Space, 108);
            keys[3] = (K::Go, 66);
            4
        }
    };
    (keys, n)
}

fn each_key(sym: bool, mut f: impl FnMut(K, i32, i32, i32)) {
    for (r, &y) in KEY_Y.iter().enumerate() {
        let (keys, n) = row(r, sym);
        let total: i32 = keys[..n].iter().map(|k| k.1).sum();
        let mut x = C - total / 2;
        for &(k, w) in &keys[..n] {
            f(k, x, y, w);
            x += w;
        }
    }
}

/// A short line built without a heap.
struct Line {
    b: [u8; 64],
    n: usize,
}

impl Line {
    fn new() -> Self {
        Line { b: [0; 64], n: 0 }
    }
    fn s(&mut self, t: &str) -> &mut Self {
        let k = t.len().min(64 - self.n);
        self.b[self.n..self.n + k].copy_from_slice(&t.as_bytes()[..k]);
        self.n += k;
        self
    }
    fn i(&mut self, v: u32) -> &mut Self {
        let mut b = [0u8; 11];
        let t = int(v.min(i32::MAX as u32) as i32, &mut b);
        self.s(t)
    }
    /// "450 m", "12.4 km", "120 km"
    fn dist(&mut self, m: u32) -> &mut Self {
        if m < 1000 {
            let r = if m < 100 { m / 5 * 5 } else { m / 10 * 10 };
            self.i(r).s(" m")
        } else if m < 100_000 {
            let t = (m + 50) / 100;
            self.i(t / 10).s(".").i(t % 10).s(" km")
        } else {
            self.i((m + 500) / 1000).s(" km")
        }
    }
    /// "25 min", "1 hr 20 min"
    fn mins(&mut self, secs: u32) -> &mut Self {
        let m = (secs + 30) / 60;
        if m < 60 {
            self.i(m.max(1)).s(" min")
        } else if m % 60 == 0 {
            self.i(m / 60).s(" hr")
        } else {
            self.i(m / 60).s(" hr ").i(m % 60).s(" min")
        }
    }
    fn hhmm(&mut self, h: u8, m: u8) -> &mut Self {
        let mut b = [0u8; 5];
        self.s(view::hhmm(h, m, &mut b))
    }
    fn get(&self) -> &str {
        match core::str::from_utf8(&self.b[..self.n]) {
            Ok(s) => s,
            Err(e) => core::str::from_utf8(&self.b[..e.valid_up_to()]).unwrap_or(""),
        }
    }
}

fn on_panel(p: (i32, i32)) -> bool {
    (0..466).contains(&p.0) && (0..466).contains(&p.1)
}

/// `t` centred on `cx`, cut to `px` with "…".
fn centre_fit(s: &mut &'static Api, cx: i32, y: i32, t: &str, px: i32, c: u16) {
    let w = font::width(t).min(px);
    font::fitted(s, cx - w / 2, y, t, px, c);
}

/// Car, bike and walker icons, ~40 px, centred on (cx, cy).
fn way_icon(s: &mut &'static Api, mode: usize, cx: i32, cy: i32, c: Rgb, bg: Rgb) {
    match mode {
        0 => {
            rrect(s, cx - 10, cy - 13, 20, 12, 4, c, bg);
            rrect(s, cx - 17, cy - 4, 34, 13, 4, c, bg);
            disc(s, cx - 10, cy + 10, 5, c, bg);
            disc(s, cx + 10, cy + 10, 5, c, bg);
        }
        1 => {
            ring(s, cx - 11, cy + 6, 6, 9, c, bg);
            ring(s, cx + 11, cy + 6, 6, 9, c, bg);
            line(s, cx - 11, cy + 6, cx - 3, cy - 6, 3, c, bg);
            line(s, cx - 3, cy - 6, cx + 8, cy - 6, 3, c, bg);
            line(s, cx + 8, cy - 6, cx + 11, cy + 6, 3, c, bg);
            line(s, cx - 3, cy - 6, cx + 1, cy + 6, 3, c, bg);
            disc(s, cx + 1, cy - 14, 3, c, bg);
        }
        _ => {
            disc(s, cx + 2, cy - 15, 5, c, bg);
            line(s, cx + 1, cy - 8, cx - 2, cy + 5, 5, c, bg);
            line(s, cx - 2, cy + 5, cx - 9, cy + 17, 4, c, bg);
            line(s, cx - 2, cy + 5, cx + 6, cy + 17, 4, c, bg);
            line(s, cx, cy - 5, cx - 9, cy + 1, 3, c, bg);
            line(s, cx, cy - 5, cx + 9, cy - 1, 3, c, bg);
        }
    }
}

struct Ui {
    api: &'static Api,
    s: &'static Api,
    screen: Screen,
    motor: Motor,
    dial: Dial,
    query: [u8; QUERY],
    n_query: usize,
    sym: bool,
    /// a QWERTY key flashing till the next touch
    key_on: Option<K>,
    zoom: u8,
    /// the middle of the home map, frame metres; None: where we are
    at: Option<(i32, i32)>,
    /// the map was last drawn then (a coming scene redraws it now and then)
    drawn_at: u32,
    /// the place opened (a result)
    sel: usize,
    /// first result row shown
    top: usize,
    mode: u8,
    /// the route's view: 0 all of it, k step k
    step: usize,
    next_id: u8,
    /// the scene asked for: round where we are, or round this point
    asked: Option<(i32, i32)>,
    /// last heard of (asks again when it stalls), tries left
    pic_at: u32,
    pic_tries: u8,
    /// waiting for the phone: what the screen says meanwhile
    busy: Option<&'static str>,
    nav_map: bool,
    nav_zoom: usize,
    /// Navigation's frame on the panel: the turn view's still parts (`view::turn_key`), or the map drawn whole once;
    /// the next frame then changes only what moved (a full clear each second flickers)
    nav_drawn: Option<[u8; 6]>,
    map_up: bool,
    /// a message's redraw waiting for FRAME_MS after the last frame
    nav_due: bool,
    /// navigating: "Exit navigation?" is up
    confirm: bool,
    /// a press on a button (it shows white)
    pressed: Option<u8>,
    /// the keyboard looks for an area (`Z`), not places
    area: bool,
    /// a touch on the map not lifted yet: where, since when (held: a pin)
    hold: Option<(i32, i32, u32)>,
    /// a pin dropped by a long press (frame metres)
    dropped: Option<(i32, i32)>,
    /// the open place on the map (frame metres); None: the scene's centre (a map asked round it)
    place_at: Option<(i32, i32)>,
    /// where Back goes from the place
    place_back: Screen,
    /// the result in the sheet
    cur: usize,
    /// Pick: the way drawn blue (the crown moves it)
    alt_sel: usize,
    /// About: scrolled this far, px
    about_top: i32,
    /// List of a mixed search: all the places round us shown (else two and "More nearby")
    expanded: bool,
    /// a stop being searched for from the route: the card's pill adds it (`J`), back returns to the route
    adding: bool,
    /// a row of the other group tapped: its map asked (`F`), it opens when the rows come placed
    pending: Option<usize>,
}

/// A line of the results list.
#[derive(Clone, Copy, PartialEq)]
enum Item {
    Row(usize),
    /// "More nearby": the rest of the places round us
    More(usize),
    /// "In <town>": the far group starts
    Sep,
}

impl Ui {
    // --- asking the phone

    fn send(&self, m: &[u8]) {
        let _ = self.api.send(m);
    }

    fn ask_map(&self, id: u8) {
        let (here, (x, y)) = match self.asked {
            None => (1, (0, 0)),
            Some(p) => (0, p),
        };
        let (x, y) = (
            (x.clamp(-32000, 32000) as i16).to_le_bytes(),
            (y.clamp(-32000, 32000) as i16).to_le_bytes(),
        );
        self.send(&[b'H', id, here, x[0], x[1], y[0], y[1]]);
    }

    /// A new scene: round where we are (None) or round a frame point.
    fn want_map(&mut self, round: Option<(i32, i32)>) {
        self.next_id = self.next_id.wrapping_add(1).max(1);
        self.asked = round;
        scene().want = self.next_id;
        self.ask_map(self.next_id);
        self.pic_at = self.api.now();
        self.pic_tries = 2;
    }

    /// Past the scene's edge (less than half a panel of it left round `p`): the next one round `p`.
    fn want_more(&mut self, p: (i32, i32), k: i32) {
        let sc = scene();
        if !sc.active || self.asked == Some(p) {
            return;
        }
        let half = 233 * 256 / k.max(1);
        let reach = sc.r - half;
        if (p.0 - sc.centre.0).abs() > reach || (p.1 - sc.centre.1).abs() > reach {
            self.want_map(Some(p));
        }
    }

    /// Nothing came: the same scene asked again, under the same id.
    fn resend(&mut self) {
        self.pic_at = self.api.now();
        if matches!(self.asked, Some(ROUTE_SCENE | FOCUS_SCENE)) {
            // the route's map comes with D: the phone sends that one again
            self.send(&[b'g', scene().want]);
        } else {
            self.ask_map(scene().want);
        }
    }

    fn timers(&mut self) {
        let now = self.api.now();
        if self.nav_due && now.wrapping_sub(self.drawn_at) >= FRAME_MS {
            self.draw();
        }
        if let Some((x, y, t)) = self.hold {
            if now.wrapping_sub(t) >= HOLD_MS {
                self.hold = None;
                self.long_press(x, y);
            }
        }
        let sc = scene();
        if sc.want != 0
            && !(sc.done && sc.id == sc.want)
            && self.pic_tries > 0
            && now.wrapping_sub(self.pic_at) >= STALL_MS
        {
            self.pic_tries -= 1;
            if sc.active && sc.id == sc.want {
                self.pic_at = now;
                self.send(&[b'g', sc.id]);
            } else {
                self.resend();
            }
        }
    }

    fn wait(&self, ask: &Ask) -> u32 {
        let now = self.api.now();
        let left = |t: u32| (t.wrapping_sub(now) as i32).max(0) as u32;
        let sc = scene();
        let mut w = u32::MAX;
        for v in [
            self.motor.wait(),
            ask.wait(now),
            (self.screen == Screen::Dial)
                .then(|| self.dial.hold_wait(now))
                .flatten(),
            (sc.want != 0 && !(sc.done && sc.id == sc.want) && self.pic_tries > 0)
                .then(|| left(self.pic_at.wrapping_add(STALL_MS))),
            self.hold.map(|h| left(h.2.wrapping_add(HOLD_MS))),
            self.nav_due.then(|| left(self.drawn_at.wrapping_add(FRAME_MS))),
            (self.screen == Screen::Nav).then(|| nav().quiet_wait(now)).flatten(),
        ]
        .into_iter()
        .flatten()
        {
            w = w.min(v);
        }
        w
    }

    // --- screens

    fn go(&mut self, s: Screen) {
        self.screen = s;
        (self.nav_drawn, self.map_up) = (None, false);
        self.pressed = None;
        self.key_on = None;
        self.dial.cancel();
        self.hold = None;
        if s == Screen::Home && self.asked == Some(ROUTE_SCENE) {
            // a fresh map round us (a route's scene is in its own frame)
            self.at = None;
            self.dropped = None;
            self.want_map(None);
        }
        self.draw();
    }

    fn draw(&mut self) {
        let mut s = self.s;
        self.drawn_at = self.api.now();
        self.nav_due = false;
        if !has_map(self.screen) {
            Surface::rect(&mut s, 0, 0, 466, 466, 0);
        }
        match self.screen {
            Screen::Home => self.draw_home(),
            Screen::Search => self.draw_keys(),
            Screen::Dial => self.draw_dial(),
            Screen::Results => self.draw_results(),
            Screen::List => self.draw_list(),
            Screen::Find => self.draw_find(),
            Screen::Place => self.draw_place(),
            Screen::Ways => self.draw_ways(),
            Screen::Pick => self.draw_pick(),
            Screen::About => self.draw_about(),
            Screen::Route => self.draw_route(),
            Screen::Nav => self.draw_nav(),
        }
        self.api.show();
    }

    /// The scene, if any of it has come; else the map's ground and what is going on.
    fn draw_map(&mut self, v: &View, route: &[(i16, i16)]) {
        let sc = scene();
        // Pick: the map there is (the route's own is asked with the pick), in the same frame as the ways
        if sc.active && (sc.id == sc.want || (matches!(self.screen, Screen::Pick | Screen::Nav) && sc.done)) {
            // the buttons over each screen's map
            let keep: &[(i32, i32, i32, i32)] = match self.screen {
                Screen::Home => &[
                    (C - 30, 20, C + 30, 80),
                    (22, 191, 106, 275),
                    (360, 191, 444, 275),
                    (166, 378, 300, 446),
                ],
                Screen::Place => &[(0, CARD_Y - 6, 466, 466), (C - 40, PLACE_Y - 40, C + 40, PLACE_Y + 10)],
                Screen::Results => &[(0, SHEET_Y - 6, 466, 466), (C - 30, 20, C + 30, 80)],
                Screen::Pick => &[(0, PICK_Y - 8, 466, 466)],
                Screen::Route => &[
                    (0, 0, 466, ROUTE_TOP + 6),
                    (STOP_BTN.0 - STOP_BTN.2 - 4, START.0 - 10, 336, 466),
                ],
                _ => &[
                    (C - 142, 38, C + 142, 86),
                    (106, 346, 360, 430),
                    (C - 30, view::ME_Y - 30, C + 30, view::ME_Y + 30),
                ],
            };
            render::draw(self.api, sc, v, route, keep);
        } else {
            let mut s = self.s;
            Surface::rect(&mut s, 0, 0, 466, 466, render::BG.rgb565());
            if self.busy.is_none() && model().n_error == 0 {
                font::centre(&mut s, C, 222, "Loading map\u{2026}", GREY);
            }
        }
        if sc.active && !sc.done {
            let mut s = self.s;
            arc(
                &mut s,
                C,
                C,
                228,
                232,
                49152,
                65536,
                Rgb(0x30, 0x30, 0x30),
                None,
                render::BG,
            );
            let f = sc.progress();
            if f > 0 {
                arc(
                    &mut s,
                    C,
                    C,
                    228,
                    232,
                    49152,
                    f * 256,
                    WHITE,
                    None,
                    render::BG,
                );
            }
        }
    }

    fn home_view(&self) -> View {
        let sc = scene();
        View {
            at: self.at.unwrap_or(sc.me),
            screen: (C, C),
            k: HOME_K[usize::from(self.zoom)],
            heading: 0,
        }
    }

    /// We are here: a red dot in a white ring.
    fn me(&mut self, p: (i32, i32)) {
        let mut s = self.s;
        if on_panel(p) {
            disc(&mut s, p.0, p.1, 13, WHITE, render::BG);
            disc(&mut s, p.0, p.1, 9, render::ROUTE, WHITE);
        }
    }

    fn pill(&mut self, y: i32, h: i32, w: i32, text: &str, fill: Rgb, id: u8) {
        let mut s = self.s;
        let on = self.pressed == Some(id);
        let x = C - w / 2;
        rrect(
            &mut s,
            x,
            y,
            w,
            h,
            h / 2,
            if on { WHITE } else { fill },
            BLACK,
        );
        centre_fit(
            &mut s,
            C,
            y + h / 2 - 12,
            text,
            w - 24,
            if on { 0 } else { W },
        );
    }

    /// ✕ over the map while a search's pins are up.
    fn draw_clear(&mut self) {
        if model().n_results == 0 {
            return;
        }
        let mut s = self.s;
        let on = self.pressed == Some(5);
        let (x, y, r) = CLEAR;
        let (fill, ink) = if on { (WHITE, Rgb(0, 0, 0)) } else { (BUTTONC, WHITE) };
        disc(&mut s, x, y, r, fill, render::BG);
        line(&mut s, x - 8, y - 8, x + 8, y + 8, 4, ink, fill);
        line(&mut s, x - 8, y + 8, x + 8, y - 8, 4, ink, fill);
    }

    /// The map round us; the buttons over it.
    fn draw_home(&mut self) {
        let v = self.home_view();
        self.draw_map(&v, &[]);
        if scene().active {
            self.me(v.px(scene().me));
            self.result_pins(&v, None);
            if let Some(d) = self.dropped {
                let p = v.px(d);
                if on_panel(p) {
                    render::pin(&mut self.s, p.0, p.1);
                }
            }
        }
        self.draw_buttons();
        self.draw_clear();
    }

    fn draw_buttons(&mut self) {
        let mut s = self.s;
        let ink = |on: bool| if on { Rgb(0, 0, 0) } else { WHITE };
        let fill = |on: bool| if on { WHITE } else { BUTTONC };
        // the last search's places as a list
        if model().n_results > 0 {
            let on = self.pressed == Some(1);
            let (x, y, r) = LIST_L;
            disc(&mut s, x, y, r, fill(on), BLACK);
            for k in -1..=1 {
                disc(&mut s, x - 12, y + k * 10, 3, ink(on), fill(on));
                rrect(&mut s, x - 5, y + k * 10 - 2, 20, 5, 2, ink(on), fill(on));
            }
        }
        // ◎ back to us
        let on = self.pressed == Some(2);
        let (x, y, r) = ME;
        disc(&mut s, x, y, r, fill(on), BLACK);
        let c = if on {
            Rgb(0, 0, 0)
        } else if self.at.is_none() {
            BLUE
        } else {
            WHITE
        };
        ring(&mut s, x, y, 11, 14, c, fill(on));
        disc(&mut s, x, y, 6, c, fill(on));
        for (dx, dy) in [(0, -1), (0, 1), (-1, 0), (1, 0)] {
            line(
                &mut s,
                x + dx * 14,
                y + dy * 14,
                x + dx * 20,
                y + dy * 20,
                3,
                c,
                fill(on),
            );
        }
        // search
        let on = self.pressed == Some(3);
        let (cx, cy, w, h) = LIST;
        rrect(&mut s, cx - w / 2, cy - h / 2, w, h, h / 2, fill(on), BLACK);
        ring(&mut s, cx - 3, cy - 3, 9, 13, ink(on), fill(on));
        line(&mut s, cx + 6, cy + 6, cx + 14, cy + 14, 5, ink(on), fill(on));
        // zoom: five dots over the button
        for i in 0..HOME_ZOOMS {
            let col = if i == self.zoom {
                WHITE
            } else {
                Rgb(0x50, 0x50, 0x50)
            };
            disc(
                &mut s,
                C - 24 + i as i32 * 12,
                cy - h / 2 - 12,
                3,
                col,
                BLACK,
            );
        }
        if model().n_error > 0 {
            rrect(&mut s, C - 160, 52, 320, 40, 20, CARD, BLACK);
            centre_fit(&mut s, C, 60, model().error(), 290, SOFT);
        }
    }

    fn draw_query(&mut self) {
        let mut s = self.s;
        let (half, max) = if self.screen == Screen::Dial {
            (100, 180)
        } else {
            (170, 300)
        };
        Surface::rect(&mut s, C - half, 80, 2 * half, 80, 0);
        let q = core::str::from_utf8(&self.query[..self.n_query]).unwrap_or("");
        if q.is_empty() {
            let hint = if self.area { "Town, area" } else { "Place, address" };
            font::centre(&mut s, C, 104, hint, DIM);
            return;
        }
        let mut start = 0;
        while font::width(&q[start..]) > max {
            start += 1;
            while !q.is_char_boundary(start) {
                start += 1;
            }
        }
        let shown = &q[start..];
        let w = font::width(shown);
        font::draw(&mut s, C - w / 2, 104, shown, W);
        Surface::rect(&mut s, C + w / 2 + 2, 102, 3, 22, BLUE.rgb565());
    }

    fn draw_keys(&mut self) {
        font::centre(&mut self.s, C, 44, if self.area { "Area" } else { "Search" }, GREY);
        self.draw_query();
        let sym = self.sym;
        let mut keys = [(K::Space, 0, 0, 0); 40];
        let mut n = 0;
        each_key(sym, |k, x, y, w| {
            keys[n] = (k, x, y, w);
            n += 1;
        });
        for &(k, ..) in &keys[..n] {
            self.draw_key(k, self.key_on == Some(k));
        }
    }

    fn draw_key(&mut self, key: K, pressed: bool) {
        let sym = self.sym;
        let mut s = self.s;
        each_key(sym, |k, x, y, w| {
            if k != key {
                return;
            }
            let fill = match (pressed, k) {
                (true, _) => WHITE,
                (_, K::Go) => BLUE,
                (_, K::Ch(_)) => KEYC,
                _ => CARD,
            };
            Surface::rect(&mut s, x, y, w, KH, 0);
            rrect(&mut s, x + 3, y + 3, w - 6, KH - 6, 14, fill, BLACK);
            let mut b = [0u8; 4];
            let label: &str = match k {
                K::Ch(c) => (c as char).encode_utf8(&mut b),
                K::Del => "<",
                K::Sym if sym => "abc",
                K::Sym => "123",
                K::Space => "space",
                K::Go => "Go",
                K::Dial => "dial",
            };
            let ink = if pressed {
                0
            } else if k == K::Space {
                GREY
            } else {
                W
            };
            font::centre(&mut s, x + w / 2, y + KH / 2 - 12, label, ink);
        });
    }

    fn key_at(&self, x: i32, y: i32) -> Option<K> {
        let mut hit = None;
        each_key(self.sym, |k, kx, ky, w| {
            if (kx..kx + w).contains(&x) && (ky..ky + KH).contains(&y) {
                hit = Some(k);
            }
        });
        hit
    }

    fn draw_dial(&mut self) {
        self.draw_query();
        let mut s = self.s;
        let mut x = 60;
        while x < 406 {
            Surface::rect(&mut s, x, 168, 2, 2, DIM);
            x += 8;
        }
        self.dial.draw(&mut s);
    }

    fn type_key(&mut self, k: K) {
        match k {
            K::Ch(c) if self.n_query < QUERY => {
                self.query[self.n_query] = c;
                self.n_query += 1;
            }
            K::Space
                if self.n_query > 0
                    && self.n_query < QUERY
                    && self.query[self.n_query - 1] != b' ' =>
            {
                self.query[self.n_query] = b' ';
                self.n_query += 1;
            }
            K::Del => self.n_query = self.n_query.saturating_sub(1),
            K::Dial => {
                self.dial.shift = self.n_query == 0;
                self.go(Screen::Dial);
                return;
            }
            K::Sym => {
                self.sym = !self.sym;
                self.draw();
                return;
            }
            K::Go if self.n_query > 0 => {
                self.search();
                return;
            }
            _ => {}
        }
        if self.screen == Screen::Search {
            self.draw_query();
            self.draw_key(k, true);
            self.key_on = Some(k);
            self.api.show();
        }
    }

    /// The results' map fitted round them, over the sheet.
    fn results_view(&self) -> View {
        let m = model();
        if m.n_results == 0 {
            return self.home_view();
        }
        let (mut lo, mut hi) = ((i32::MAX, i32::MAX), (i32::MIN, i32::MIN));
        for p in m.results[..m.n_results].iter().filter(|p| p.placed()) {
            lo = (lo.0.min(p.at.0), lo.1.min(p.at.1));
            hi = (hi.0.max(p.at.0), hi.1.max(p.at.1));
        }
        if lo.0 > hi.0 {
            return self.home_view();
        }
        let (w, h) = ((hi.0 - lo.0).max(1), (hi.1 - lo.1).max(1));
        // as far out as the places need (past the map that came, the pins still show)
        let k = (330 * 256 / w)
            .min((SHEET_Y - 90) * 256 / h)
            .clamp(12, HOME_K[3]);
        View {
            at: ((lo.0 + hi.0) / 2, (lo.1 + hi.1) / 2),
            screen: (C, (SHEET_Y + 40) / 2),
            k,
            heading: 0,
        }
    }

    /// The search's places as red pins; `cur` ringed.
    fn result_pins(&mut self, v: &View, cur: Option<usize>) {
        let mut s = self.s;
        let m = model();
        for i in (0..m.n_results).filter(|&i| m.results[i].placed()) {
            let p = v.px(m.results[i].at);
            if on_panel(p) && Some(i) != cur {
                render::pin(&mut s, p.0, p.1);
            }
        }
        if let Some(i) = cur.filter(|&i| i < m.n_results && m.results[i].placed()) {
            let p = v.px(m.results[i].at);
            if on_panel(p) {
                disc(&mut s, p.0, p.1 - 13, 15, WHITE, render::BG);
                render::pin(&mut s, p.0, p.1);
            }
        }
    }

    /// Google's results: pins on the map, the one the crown is on in the sheet at the bottom.
    fn draw_results(&mut self) {
        let v = self.results_view();
        self.draw_map(&v, &[]);
        let sc = scene();
        if sc.active && sc.id == sc.want {
            self.me(v.px(sc.me));
            self.result_pins(&v, Some(self.cur));
        }
        self.draw_clear();
        let mut s = self.s;
        let m = model();
        Surface::rect(&mut s, 0, SHEET_Y, 466, 466 - SHEET_Y, 0);
        rrect(&mut s, C - 24, SHEET_Y + 8, 48, 5, 2, Rgb(0x50, 0x50, 0x50), BLACK);
        if let Some(b) = self.busy {
            font::centre(&mut s, C, SHEET_Y + 40, b, SOFT);
            return;
        }
        if m.n_error > 0 {
            centre_fit(&mut s, C, SHEET_Y + 40, m.error(), 340, SOFT);
            return;
        }
        if m.n_results == 0 {
            font::centre(&mut s, C, SHEET_Y + 40, "Nothing found", SOFT);
            return;
        }
        let p = &m.results[self.cur.min(m.n_results - 1)];
        let on = self.pressed == Some(8);
        centre_fit(&mut s, C, SHEET_Y + 24, p.name(), 330, if on { SOFT } else { W });
        let mut l = Line::new();
        l.dist(p.dist);
        if !p.sub().is_empty() {
            l.s(" \u{b7} ").s(p.sub());
        }
        centre_fit(&mut s, C, SHEET_Y + 54, l.get(), 330, GREY);
        let mut l = Line::new();
        l.i(self.cur as u32 + 1).s(" of ").i(m.n_results as u32);
        centre_fit(&mut s, C, SHEET_Y + 88, l.get(), 200, DIM);
    }

    /// The sheet pulled up: every place in a list.
    fn draw_list(&mut self) {
        let mut s = self.s;
        let m = model();
        // the query, a tap searches again
        let q = core::str::from_utf8(&self.query[..self.n_query]).unwrap_or("");
        let on = self.pressed == Some(9);
        rrect(
            &mut s,
            C - 110,
            24,
            220,
            44,
            22,
            if on { WHITE } else { CARD },
            BLACK,
        );
        ring(
            &mut s,
            C - 84,
            44,
            6,
            9,
            if on { Rgb(0, 0, 0) } else { WHITE },
            if on { WHITE } else { CARD },
        );
        font::fitted(&mut s, C - 66, 34, q, 160, if on { 0 } else { SOFT });
        if let Some(b) = self.busy {
            font::centre(&mut s, C, 210, b, SOFT);
            return;
        }
        if m.n_error > 0 {
            centre_fit(&mut s, C, 210, m.error(), 380, SOFT);
            return;
        }
        if m.n_results == 0 {
            font::centre(&mut s, C, 210, "Nothing found", SOFT);
            return;
        }
        let (items, n) = self.items();
        for r in 0..ROWS {
            let Some(&item) = items[..n].get(self.top + r) else {
                break;
            };
            let y = ROW_Y + r as i32 * ROW_H;
            let (x0, x1) = chord(y, y + ROW_H, 18);
            let i = match item {
                Item::Row(i) => i,
                Item::More(k) => {
                    let on = self.pressed == Some(35);
                    rrect(&mut s, x0, y + 3, x1 - x0, ROW_H - 6, 18, if on { Rgb(0x3A, 0x3A, 0x3A) } else { BLACK }, BLACK);
                    let mut l = Line::new();
                    l.s("More nearby (").i(k as u32).s(")");
                    font::centre(&mut s, C, y + 20, l.get(), BLUE.rgb565());
                    continue;
                }
                Item::Sep => {
                    Surface::rect(&mut s, x0 + 10, y + 12, x1 - x0 - 20, 1, DIM);
                    let mut l = Line::new();
                    l.s("In ").s(m.results[m.near].name());
                    centre_fit(&mut s, C, y + 26, l.get(), x1 - x0 - 40, GREY);
                    continue;
                }
            };
            let on = self.pressed == Some(10 + i as u8);
            rrect(
                &mut s,
                x0,
                y + 3,
                x1 - x0,
                ROW_H - 6,
                18,
                if on { Rgb(0x3A, 0x3A, 0x3A) } else { CARD },
                BLACK,
            );
            let p = &m.results[i];
            font::fitted(&mut s, x0 + 18, y + 10, p.name(), x1 - x0 - 36, W);
            let mut l = Line::new();
            l.dist(p.dist);
            if !p.sub().is_empty() {
                l.s(" \u{b7} ").s(p.sub());
            }
            font::fitted(&mut s, x0 + 18, y + 34, l.get(), x1 - x0 - 36, GREY);
        }
        if self.top + ROWS < n {
            disc(
                &mut s,
                C,
                ROW_Y + ROWS as i32 * ROW_H + 8,
                3,
                Rgb(0x80, 0x80, 0x80),
                BLACK,
            );
        }
    }

    /// The list's lines: the results, or for a mixed search two round us (all when expanded), then the town far away
    /// and its places under "In <town>".
    fn items(&self) -> ([Item; 12], usize) {
        let m = model();
        let mut out = [Item::Sep; 12];
        let mut n = 0;
        let mut put = |it: Item| {
            out[n] = it;
            n += 1;
        };
        if m.near == 0 || m.near >= m.n_results {
            (0..m.n_results).for_each(|i| put(Item::Row(i)));
        } else {
            let shown = if self.expanded || m.near <= 2 { m.near } else { 2 };
            (0..shown).for_each(|i| put(Item::Row(i)));
            if shown < m.near {
                put(Item::More(m.near - shown));
            }
            put(Item::Sep);
            (m.near..m.n_results).for_each(|i| put(Item::Row(i)));
        }
        (out, n)
    }

    /// A row of the other group of a mixed search: the map goes there (`F`), then it opens.
    fn focus_row(&mut self, i: usize) {
        let id = self.focus_scene();
        self.send(&[b'F', id, i as u8]);
        self.pending = Some(i);
        self.busy = Some("Loading\u{2026}");
        self.go(Screen::Results);
    }

    fn open_place(&self) -> &'static Place {
        &model().results[self.sel.min(PICKED)]
    }

    /// The open place on the map, round it.
    fn place_view(&self) -> View {
        let sc = scene();
        View {
            at: self.place_at.unwrap_or(sc.centre),
            screen: (C, PLACE_Y),
            k: HOME_K[3],
            heading: 0,
        }
    }

    /// Google's place sheet: the map round the place with its pin, the card under it.
    fn draw_place(&mut self) {
        let v = self.place_view();
        self.draw_map(&v, &[]);
        let mut s = self.s;
        let sc = scene();
        if sc.active && sc.id == sc.want {
            self.me(v.px(sc.me));
            let p = v.px(v.at);
            render::pin(&mut s, p.0, p.1);
        }
        Surface::rect(&mut s, 0, CARD_Y, 466, 466 - CARD_Y, 0);
        // the handle: pull the card up for all of it
        rrect(&mut s, C - 22, CARD_Y + 2, 44, 5, 2, Rgb(0x5A, 0x5A, 0x5A), BLACK);
        let m = model();
        let p = self.open_place();
        centre_fit(&mut s, C, CARD_Y + 14, p.name(), 330, W);
        let mut l = Line::new();
        if !m.info(0).is_empty() {
            l.s(m.info(0)).s(" \u{b7} ");
        }
        l.dist(p.dist);
        centre_fit(&mut s, C, CARD_Y + 44, l.get(), 360, SOFT);
        let addr = if m.info(1).is_empty() { p.sub() } else { m.info(1) };
        centre_fit(&mut s, C, CARD_Y + 72, addr, 340, GREY);
        if !m.info(2).is_empty() {
            centre_fit(&mut s, C, CARD_Y + 98, m.info(2), 260, GREY);
        }
        let (y, h, w) = DIRS;
        self.pill(y, h, w, if self.adding { "Add stop" } else { "Directions" }, BLUE, 24);
    }

    /// The place: car, bike and foot, each with its time and arrival.
    fn draw_ways(&mut self) {
        let mut s = self.s;
        let m = model();
        let p = self.open_place();
        centre_fit(&mut s, C, 40, p.name(), 300, W);
        let mut l = Line::new();
        l.dist(p.dist);
        if !p.sub().is_empty() {
            l.s(" \u{b7} ").s(p.sub());
        }
        centre_fit(&mut s, C, 72, l.get(), 340, GREY);
        for (i, &y) in WAY_Y.iter().enumerate() {
            let (x0, x1) = chord(y, y + WAY_H, 16);
            let on = self.pressed == Some(20 + i as u8);
            let fill = if on { Rgb(0x3A, 0x3A, 0x3A) } else { CARD };
            rrect(
                &mut s,
                x0,
                y + 3,
                x1 - x0,
                WAY_H - 6,
                (WAY_H - 6) / 2,
                fill,
                BLACK,
            );
            way_icon(&mut s, i, x0 + 48, y + WAY_H / 2, BLUE, fill);
            let tx = x0 + 88;
            let tw = x1 - tx - 24;
            match m.ways[i] {
                Way::Ok {
                    secs,
                    metres,
                    arrive,
                } => {
                    let mut l = Line::new();
                    l.mins(secs);
                    font::fitted(&mut s, tx, y + 12, l.get(), tw, W);
                    let mut l = Line::new();
                    l.s("ETA ")
                        .hhmm(arrive.0, arrive.1)
                        .s(" \u{b7} ")
                        .dist(metres);
                    font::fitted(&mut s, tx, y + 40, l.get(), tw, GREY);
                }
                Way::None => {
                    font::fitted(&mut s, tx, y + 26, "No route found", tw, GREY);
                }
                Way::Asking => {
                    font::fitted(&mut s, tx, y + 12, MODES[i], tw, SOFT);
                    font::fitted(&mut s, tx, y + 40, "Finding\u{2026}", tw, DIM);
                }
            }
        }
    }

    /// The routes there to pick from: the time and distance, and the roads each goes by.
    /// The ways there on the map, fitted above the list.
    fn pick_view(&self) -> View {
        let m = model();
        let u = m.alt_unit;
        let (mut lo, mut hi) = ((i32::MAX, i32::MAX), (i32::MIN, i32::MIN));
        for k in 0..m.n_alts {
            for p in &m.alt_pts[k][..m.n_alt_pts[k]] {
                let p = (i32::from(p.0) * u, i32::from(p.1) * u);
                lo = (lo.0.min(p.0), lo.1.min(p.1));
                hi = (hi.0.max(p.0), hi.1.max(p.1));
            }
        }
        if lo.0 > hi.0 {
            return View {
                at: scene().me,
                screen: (C, 140),
                k: HOME_K[1],
                heading: 0,
            };
        }
        let (w, h) = ((hi.0 - lo.0).max(1), (hi.1 - lo.1).max(1));
        View {
            at: ((lo.0 + hi.0) / 2, (lo.1 + hi.1) / 2),
            screen: (C, 142),
            k: (300 * 256 / w).min(190 * 256 / h).clamp(1, HOME_K[3]),
            heading: 0,
        }
    }

    /// Google's route choice: the ways on the map (the one picked blue, the others grey), listed under it.
    fn draw_pick(&mut self) {
        let v = self.pick_view();
        self.draw_map(&v, &[]);
        let mut s = self.s;
        let m = model();
        let u = m.alt_unit;
        let sel = self.alt_sel.min(m.n_alts.saturating_sub(1));
        // the others first, the picked one over them
        for k in (0..m.n_alts).filter(|&k| k != sel).chain(core::iter::once(sel)) {
            let pts = &m.alt_pts[k][..m.n_alt_pts[k]];
            let (col, w) = if k == sel { (BLUE, 8) } else { (Rgb(0x8A, 0x8F, 0x98), 6) };
            for pair in pts.windows(2) {
                let a = v.px((i32::from(pair[0].0) * u, i32::from(pair[0].1) * u));
                let b = v.px((i32::from(pair[1].0) * u, i32::from(pair[1].1) * u));
                line(&mut s, a.0, a.1, b.0, b.1, w, col, render::BG);
            }
        }
        if let Some(p) = m.alt_pts[sel][..m.n_alt_pts[sel]].last() {
            let p = v.px((i32::from(p.0) * u, i32::from(p.1) * u));
            render::pin(&mut s, p.0, p.1);
        }
        Surface::rect(&mut s, 0, PICK_Y - 8, 466, 466 - PICK_Y + 8, 0);
        let fastest = (0..m.n_alts).min_by_key(|&k| m.alts[k].secs).unwrap_or(0);
        for k in 0..m.n_alts {
            let a = &m.alts[k];
            let y = PICK_Y + k as i32 * PICK_H;
            let (x0, x1) = chord(y, y + PICK_H, 14);
            let on = self.pressed == Some(60 + k as u8);
            let fill = if on { Rgb(0x3A, 0x3A, 0x3A) } else { CARD };
            rrect(&mut s, x0, y + 3, x1 - x0, PICK_H - 6, (PICK_H - 6) / 2, fill, BLACK);
            disc(&mut s, x0 + 26, y + PICK_H / 2, 7, if k == sel { BLUE } else { Rgb(0x8A, 0x8F, 0x98) }, fill);
            let mut l = Line::new();
            l.mins(a.secs).s(" \u{b7} ").dist(a.metres);
            if k == fastest {
                l.s(" \u{b7} Fastest");
            }
            let c = if k == fastest { rgb565(0x5F, 0xC9, 0x7E) } else { W };
            font::fitted(&mut s, x0 + 44, y + 6, l.get(), x1 - x0 - 64, c);
            let mut l = Line::new();
            l.s("via ").s(a.via());
            font::fitted(&mut s, x0 + 44, y + 30, l.get(), x1 - x0 - 64, GREY);
        }
    }

    /// A place's details: all that is known of it, the crown scrolls.
    fn draw_about(&mut self) {
        let mut s = self.s;
        let m = model();
        let p = self.open_place();
        let mut d = Line::new();
        if !m.info(0).is_empty() {
            d.s(m.info(0)).s(" \u{b7} ");
        }
        d.dist(p.dist);
        let mut lines: [(&str, u16); 40] = [("", 0); 40];
        let mut n = 0;
        push(&mut lines, &mut n, p.name(), W);
        push(&mut lines, &mut n, d.get(), SOFT);
        push(&mut lines, &mut n, "", 0);
        push(&mut lines, &mut n, if m.info(1).is_empty() { p.sub() } else { m.info(1) }, GREY);
        for k in 2..4 {
            if !m.info(k).is_empty() {
                push(&mut lines, &mut n, m.info(k), BLUE.rgb565());
            }
        }
        if !m.about().is_empty() {
            push(&mut lines, &mut n, "", 0);
            push(&mut lines, &mut n, m.about(), W);
        }
        let top = ABOUT_Y.0 + 14 - self.about_top;
        for (i, (t, c)) in lines[..n].iter().enumerate() {
            let y = top + i as i32 * ABOUT_H;
            if y < ABOUT_Y.0 || y > ABOUT_Y.1 || t.is_empty() {
                continue;
            }
            font::fitted(&mut s, C - ABOUT_W / 2, y, t, ABOUT_W, *c);
        }
    }

    /// How far About scrolls: its lines past the panel.
    fn about_max(&self) -> i32 {
        let m = model();
        let p = self.open_place();
        let mut out = [""; 24];
        let mut n = 4 + (2..4).filter(|&k| !m.info(k).is_empty()).count() as i32;
        n += font::wrap(p.name(), ABOUT_W, &mut out) as i32 - 1;
        n += font::wrap(if m.info(1).is_empty() { p.sub() } else { m.info(1) }, ABOUT_W, &mut out) as i32 - 1;
        if !m.about().is_empty() {
            n += 1 + font::wrap(m.about(), ABOUT_W, &mut out) as i32;
        }
        (n * ABOUT_H + 14 - (ABOUT_Y.1 - ABOUT_Y.0)).max(0)
    }

    /// Find: Search and Area, then the kinds of places.
    fn find_boxes(mut f: impl FnMut(u8, i32, i32, i32, i32)) {
        f(40, C - 164, FIND_Y, 160, FIND_H);
        f(41, C + 4, FIND_Y, 160, FIND_H);
        for i in 0..CATEGORIES.len() as i32 {
            let x = if i % 2 == 0 { C - CAT_W - 4 } else { C + 4 };
            f(50 + i as u8, x, CAT_Y + i / 2 * CAT_H, CAT_W, CAT_H - 6);
        }
    }

    fn draw_find(&mut self) {
        let mut s = self.s;
        let pressed = self.pressed;
        Self::find_boxes(|id, x, y, w, h| {
            let on = pressed == Some(id);
            let fill = match (on, id) {
                (true, _) => WHITE,
                (_, 40) => BLUE,
                _ => CARD,
            };
            rrect(&mut s, x, y, w, h, h / 2, fill, BLACK);
            let ink = if on { 0 } else { W };
            if id >= 50 {
                let i = usize::from(id - 50);
                disc(&mut s, x + 26, y + h / 2, 8, CAT_C[i], fill);
                font::fitted(&mut s, x + 42, y + h / 2 - 12, CATEGORIES[i], w - 52, ink);
            } else {
                let t = if id == 40 { "Search" } else { "Area" };
                font::centre(&mut s, x + w / 2, y + h / 2 - 12, t, ink);
            }
        });
    }

    /// The route's view: all of it (step 0) or round step k, up close.
    fn route_view(&self) -> View {
        let n = nav();
        let pts = &n.pts[..n.n_pts];
        let m = model();
        if self.step > 0 && self.step <= m.n_steps {
            let at = m.steps[self.step - 1].at as i32;
            if let Some(p) = render::along(pts, at) {
                return View {
                    at: p,
                    screen: (C, (ROUTE_TOP + START.0) / 2),
                    k: STEP_K,
                    heading: 0,
                };
            }
        }
        let (mut lo, mut hi) = ((i32::MAX, i32::MAX), (i32::MIN, i32::MIN));
        for p in pts {
            let p = (i32::from(p.0), i32::from(p.1));
            lo = (lo.0.min(p.0), lo.1.min(p.1));
            hi = (hi.0.max(p.0), hi.1.max(p.1));
        }
        if pts.is_empty() {
            return View {
                at: scene().me,
                screen: (C, C),
                k: 128,
                heading: 0,
            };
        }
        // the route's box in the panel between the header and Start
        let (w, h) = ((hi.0 - lo.0).max(1), (hi.1 - lo.1).max(1));
        let k = (300 * 256 / w)
            .min((START.0 - ROUTE_TOP - 50) * 256 / h)
            .clamp(8, 320);
        View {
            at: ((lo.0 + hi.0) / 2, (lo.1 + hi.1) / 2),
            screen: (C, (ROUTE_TOP + START.0) / 2),
            k,
            heading: 0,
        }
    }

    fn draw_route_head(&mut self) {
        let mut s = self.s;
        Surface::rect(&mut s, 0, 0, 466, ROUTE_TOP, 0);
        let m = model();
        if let Some(b) = self.busy {
            font::centre(&mut s, C, 56, b, SOFT);
            return;
        }
        if m.n_error > 0 && !m.routed {
            centre_fit(&mut s, C, 56, m.error(), 300, SOFT);
            return;
        }
        if !m.routed {
            return;
        }
        if self.step == 0 {
            let mut l = Line::new();
            l.mins(m.secs).s(" \u{b7} ").dist(m.metres);
            centre_fit(&mut s, C, 30, l.get(), 230, W);
            let mut l = Line::new();
            l.s(MODES[(m.mode as usize).min(2)])
                .s(" \u{b7} ETA ")
                .hhmm(m.arrive.0, m.arrive.1);
            centre_fit(&mut s, C, 58, l.get(), 300, GREY);
        } else {
            let st = &m.steps[(self.step - 1).min(m.n_steps.saturating_sub(1))];
            let mut l = Line::new();
            l.i(self.step as u32)
                .s(" of ")
                .i(m.n_steps as u32)
                .s(" \u{b7} at ")
                .dist(st.at);
            centre_fit(&mut s, C, 30, l.get(), 230, GREY);
            let mut ib = [0u8; 112];
            let what = instruction(st.kind, st.dir, st.name(), &mut ib);
            view::arrow(&mut s, C - 150, 70, 28, st.kind, st.dir);
            centre_fit(&mut s, C + 10, 58, what, 290, W);
        }
        // where in the steps: a red tick on a short line
        let n = m.n_steps as i32 + 1;
        let (x0, w) = (C - 60, 120);
        Surface::rect(&mut s, x0, 92, w, 2, DIM);
        let x = x0 + w * self.step as i32 / n.max(1);
        Surface::rect(&mut s, x, 90, (w / n).max(4), 6, BLUE.rgb565());
    }

    fn draw_route(&mut self) {
        if model().routed {
            let v = self.route_view();
            let n = nav();
            self.draw_map(&v, &n.pts[..n.n_pts]);
            if n.n_pts > 0 {
                let mut s = self.s;
                let last = n.pts[n.n_pts - 1];
                let end = v.px((i32::from(last.0), i32::from(last.1)));
                if on_panel(end) {
                    render::pin(&mut s, end.0, end.1);
                }
                self.me(v.px((i32::from(n.pts[0].0), i32::from(n.pts[0].1))));
                if self.step > 0 {
                    ring(&mut s, v.screen.0, v.screen.1, 16, 20, WHITE, render::BG);
                }
            }
        } else {
            let mut s = self.s;
            Surface::rect(&mut s, 0, 0, 466, 466, 0);
        }
        self.draw_route_head();
        let (y, h, w) = START;
        if model().routed {
            self.pill(y, h, w, "Start", BLUE, 4);
            // a stop on the way: a dark disc with a plus
            let mut s = self.s;
            let (cx, cy, r) = STOP_BTN;
            let on = self.pressed == Some(7);
            disc(&mut s, cx, cy, r, if on { WHITE } else { CARD }, render::BG);
            let ink = if on { 0 } else { W };
            Surface::rect(&mut s, cx - 10, cy - 2, 20, 4, ink);
            Surface::rect(&mut s, cx - 2, cy - 10, 4, 20, ink);
        }
    }

    fn draw_nav(&mut self) {
        let n = nav();
        let mut s = self.s;
        if self.confirm || !n.active || n.arrived {
            (self.nav_drawn, self.map_up) = (None, false);
        }
        if self.confirm {
            Surface::rect(&mut s, 0, 0, 466, 466, 0);
            font::centre(&mut s, C, 140, "Exit navigation?", W);
            self.pill(196, 64, 240, "Exit", RED, 30);
            self.pill(280, 64, 240, "Continue", CARD, 31);
        } else if !n.active {
            Surface::rect(&mut s, 0, 0, 466, 466, 0);
            font::centre(&mut s, C, 200, "Starting\u{2026}", W);
            font::centre(&mut s, C, 232, "Waiting for the phone", GREY);
        } else if n.arrived {
            Surface::rect(&mut s, 0, 0, 466, 466, 0);
            view::arrived(self.api, n);
            self.pill(340, 56, 160, "Done", CARD, 32);
        } else if self.nav_map {
            self.nav_drawn = None;
            let sc = scene();
            if self.map_up && sc.active && !sc.done {
                // the next map is coming in (it replaces the old one as it comes): the map as it was stays on the
                // panel, only the marks and numbers move, till it is all here
                view::north_erase(self.api);
                view::overlay(self.api, n);
                return;
            }
            let v = View {
                at: (i32::from(n.pos.0), i32::from(n.pos.1)),
                screen: (C, view::ME_Y),
                k: ZOOMS[self.nav_zoom],
                heading: n.heading,
            };
            self.draw_map(&v, &n.pts[..n.n_pts]);
            view::overlay(self.api, n);
            self.map_up = sc.active && sc.done;
        } else {
            self.map_up = false;
            let key = view::turn_key(n);
            if self.nav_drawn == Some(key) {
                view::turn_live(self.api, n);
            } else {
                Surface::rect(&mut s, 0, 0, 466, 466, 0);
                view::turn(self.api, n);
                self.nav_drawn = Some(key);
            }
        }
    }

    // --- input

    /// Navigation over: the phone stops, back to the route.
    fn end_nav(&mut self, to: Screen) {
        self.send(b"X");
        self.motor.off();
        self.api.keep_screen_on(false);
        let n = nav();
        (n.active, n.arrived, n.rerouting) = (false, false, false);
        self.confirm = false;
        self.step = 0;
        self.go(to);
    }

    fn back(&mut self) -> bool {
        match self.screen {
            Screen::Home | Screen::Find | Screen::Results if self.adding => self.stop_route(None),
            Screen::Home => return false,
            Screen::Find | Screen::Results => self.go(Screen::Home),
            Screen::Search => self.go(Screen::Find),
            Screen::Dial => self.go(Screen::Search),
            Screen::List => self.go(Screen::Results),
            Screen::Place => {
                // the map as it was before the place: its zoom, and its map asked again if the place's replaced it
                self.go(self.place_back);
                if matches!(self.screen, Screen::Home | Screen::Results) {
                    let v = self.map_view();
                    self.want_more(v.at, v.k);
                }
            }
            Screen::About => self.go(Screen::Place),
            Screen::Ways => self.go(Screen::Place),
            Screen::Pick => self.go(Screen::Ways),
            Screen::Route => {
                self.busy = None;
                self.go(Screen::Ways)
            }
            Screen::Nav => {
                if nav().arrived {
                    self.end_nav(Screen::Home);
                } else {
                    self.confirm = !self.confirm;
                    self.motor.play(haptic::TICK);
                    self.draw();
                }
            }
        }
        true
    }

    fn crown(&mut self, d: i32) {
        match self.screen {
            Screen::Home => {
                // stops at the nearest and the farthest zoom
                let z = (self.zoom as i32 + d).clamp(0, HOME_ZOOMS as i32 - 1) as u8;
                if z != self.zoom {
                    self.zoom = z;
                    self.motor.play(haptic::TICK);
                    self.draw();
                    let v = self.home_view();
                    self.want_more(v.at, v.k);
                }
            }
            Screen::Results => {
                // the next place on this map that way (a mixed search's other group is not on it)
                let m = model();
                let mut c = self.cur as i32 + d;
                while (0..m.n_results as i32).contains(&c) && !m.results[c as usize].placed() {
                    c += d;
                }
                let c = if (0..m.n_results as i32).contains(&c) { c as usize } else { self.cur };
                if c != self.cur {
                    self.cur = c;
                    self.motor.play(haptic::TICK);
                    self.draw();
                }
            }
            Screen::Dial => {
                let a = self.dial.crown(&mut self.s, d, self.api.now());
                self.dial_act(a);
            }
            Screen::List => {
                let n = self.items().1;
                let t = (self.top as i32 + d).clamp(0, n.saturating_sub(ROWS) as i32) as usize;
                if t != self.top {
                    self.top = t;
                    self.motor.play(haptic::TICK);
                    self.draw();
                }
            }
            Screen::Route if model().routed => {
                let k = (self.step as i32 + d).clamp(0, model().n_steps as i32) as usize;
                if k != self.step {
                    self.step = k;
                    self.motor.play(haptic::TICK);
                    self.draw();
                }
            }
            Screen::Pick => {
                let k = (self.alt_sel as i32 + d).clamp(0, model().n_alts.saturating_sub(1) as i32) as usize;
                if k != self.alt_sel {
                    self.alt_sel = k;
                    self.motor.play(haptic::TICK);
                    self.draw();
                }
            }
            Screen::About => {
                let t = (self.about_top + d * 60).clamp(0, self.about_max());
                if t != self.about_top {
                    self.about_top = t;
                    self.draw();
                }
            }
            Screen::Nav if self.nav_map && !self.confirm => {
                let z = (self.nav_zoom as i32 + d).clamp(0, ZOOMS.len() as i32 - 1) as usize;
                if z != self.nav_zoom {
                    self.nav_zoom = z;
                    self.motor.play(haptic::TICK);
                    self.draw();
                }
            }
            _ => {}
        }
    }

    /// Which button a touch is on, for the screen.
    fn hit(&self, x: i32, y: i32) -> Option<u8> {
        let disc_at =
            |(cx, cy, r): (i32, i32, i32)| (x - cx).pow(2) + (y - cy).pow(2) <= (r + 10).pow(2);
        let pill = |top: i32, h: i32, w: i32| {
            (top - 8..top + h + 8).contains(&y) && (C - w / 2 - 8..C + w / 2 + 8).contains(&x)
        };
        match self.screen {
            Screen::Results => (model().n_results > 0 && disc_at(CLEAR)).then_some(5),
            Screen::Home => {
                let (cx, cy, w, h) = LIST;
                if model().n_results > 0 && disc_at(CLEAR) {
                    Some(5)
                } else if model().n_results > 0 && disc_at(LIST_L) {
                    Some(1)
                } else if disc_at(ME) {
                    Some(2)
                } else if pill(cy - h / 2, h, w) && (x - cx).abs() < w {
                    Some(3)
                } else {
                    None
                }
            }
            Screen::Route if model().routed && disc_at(STOP_BTN) => Some(7),
            Screen::Route => (model().routed && pill(START.0, START.1, START.2)).then_some(4),
            Screen::List => {
                if y < 74 && (x - C).abs() < 130 {
                    return Some(9);
                }
                let r = (y - ROW_Y).div_euclid(ROW_H);
                if y < ROW_Y || !(0..ROWS as i32).contains(&r) || self.busy.is_some() {
                    return None;
                }
                let (items, n) = self.items();
                match items[..n].get(self.top + r as usize) {
                    Some(Item::Row(i)) => Some(10 + *i as u8),
                    Some(Item::More(_)) => Some(35),
                    _ => None,
                }
            }
            Screen::Find => {
                let mut hit = None;
                Self::find_boxes(|id, bx, by, w, h| {
                    if (bx..bx + w).contains(&x) && (by..by + h).contains(&y) {
                        hit = Some(id);
                    }
                });
                hit
            }
            Screen::Place => pill(DIRS.0, DIRS.1, DIRS.2).then_some(24),
            Screen::Ways => WAY_Y
                .iter()
                .position(|&wy| (wy..wy + WAY_H).contains(&y))
                .filter(|&i| matches!(model().ways[i], Way::Ok { .. }))
                .map(|i| 20 + i as u8),
            Screen::Pick => {
                let k = (y - PICK_Y).div_euclid(PICK_H);
                (y >= PICK_Y && (0..model().n_alts as i32).contains(&k)).then_some(60 + k as u8)
            }
            Screen::Nav if self.confirm => {
                if pill(196, 64, 240) {
                    Some(30)
                } else if pill(280, 64, 240) {
                    Some(31)
                } else {
                    None
                }
            }
            Screen::Nav if nav().arrived => pill(340, 56, 160).then_some(32),
            _ => None,
        }
    }

    fn down(&mut self, x: i32, y: i32) {
        match self.screen {
            Screen::Search => {
                if let Some(old) = self.key_on.take() {
                    self.draw_key(old, false);
                }
                if let Some(k) = self.key_at(x, y) {
                    self.motor.play(haptic::TICK);
                    self.type_key(k);
                } else {
                    self.api.show();
                }
            }
            Screen::Dial => {
                if (70..160).contains(&y) && (x - C).abs() < 150 {
                    // the typed text: back to the keys
                    self.go(Screen::Search);
                    return;
                }
                let a = self.dial.down(&mut self.s, x, y, self.api.now());
                self.dial_act(a);
            }
            Screen::Nav if !self.confirm && nav().active && !nav().arrived => {
                self.nav_map = !self.nav_map;
                self.motor.play(haptic::TICK);
                self.draw();
            }
            // the map: a tap, a long press or (Results) the sheet pulled up, known when the finger lifts
            Screen::Home | Screen::Results | Screen::Place if self.hit(x, y).is_none() => {
                self.hold = Some((x, y, self.api.now()));
            }
            _ => {
                if let Some(b) = self.hit(x, y) {
                    self.pressed = Some(b);
                    self.motor.play(haptic::TICK);
                    self.draw_pressed();
                    // acted on at once: TOUCH_UP is not always there to rely on
                    self.press(b);
                }
            }
        }
    }

    /// The finger lifted off the map before a long press.
    fn up(&mut self, x: i32, y: i32) {
        let Some((x0, y0, _)) = self.hold.take() else {
            return;
        };
        // TOUCH_UP's place is not reliable (docs/app-guide.md: the emulator gives a top-row y): a lift at the very top
        // or at 0, 0 is taken as where the finger went down (a tap); a pull ends lower, mid-panel
        let y = if (x, y) == (0, 0) || y < 60 { y0 } else { y };
        if self.screen == Screen::Place {
            // the card pulled up, or tapped: the place's details
            if y0 > CARD_Y - 20 {
                self.about_top = 0;
                self.motor.play(haptic::TICK);
                self.go(Screen::About);
            }
            return;
        }
        if self.screen == Screen::Results && y0 > SHEET_Y - 30 && y < y0 - 40 {
            // the sheet pulled up
            self.top = self.cur.saturating_sub(2);
            self.motor.play(haptic::TICK);
            self.go(Screen::List);
            return;
        }
        self.tap_map(x0, y0);
    }

    /// The map the screen shows.
    fn map_view(&self) -> View {
        if self.screen == Screen::Results {
            self.results_view()
        } else {
            self.home_view()
        }
    }

    /// A tap on the map: the sheet, a pin, else (Home) near the rim a move.
    fn tap_map(&mut self, x: i32, y: i32) {
        let m = model();
        if self.screen == Screen::Results && y > SHEET_Y - 10 {
            if m.n_results > 0 {
                self.open_result(self.cur.min(m.n_results - 1));
            }
            return;
        }
        if !scene().active {
            return;
        }
        let v = self.map_view();
        // a searched place's pin
        let near = (0..m.n_results)
            .filter(|&i| m.results[i].placed())
            .map(|i| {
                let p = v.px(m.results[i].at);
                ((x - p.0).pow(2) + (y - p.1 + 13).pow(2), i)
            })
            .min();
        if let Some((d, i)) = near {
            if d < 34 * 34 {
                self.cur = i;
                self.open_result(i);
                return;
            }
        }
        // a place on the map
        if let Some((at, name)) = render::pin_at(scene(), x, y) {
            self.open_picked(at, name);
            return;
        }
        if self.screen == Screen::Home {
            self.edge(x, y);
        }
    }

    /// Held on the map: a pin there, and the place opened to go to it.
    fn long_press(&mut self, x: i32, y: i32) {
        if !matches!(self.screen, Screen::Home | Screen::Results)
            || !scene().active
            || (self.screen == Screen::Results && y > SHEET_Y - 10)
        {
            return;
        }
        let v = self.map_view();
        let at = (
            v.at.0 + (x - v.screen.0) * 256 / v.k.max(1),
            v.at.1 - (y - v.screen.1) * 256 / v.k.max(1),
        );
        self.dropped = Some(at);
        self.motor.play(haptic::TICK);
        self.open_picked(at, "");
    }

    /// Result `i` opened: its card on the map round it.
    fn open_result(&mut self, i: usize) {
        self.sel = i;
        self.place_at = Some(model().results[i].at);
        self.place_back = self.screen;
        model().open(i);
        self.send(&[b'I', i as u8]);
        self.go(Screen::Place);
        self.want_more(model().results[i].at, HOME_K[3]);
    }

    /// A place on the map (a pin tapped; a dropped pin: no name) opened as `PICKED`.
    fn open_picked(&mut self, at: (i32, i32), name: &str) {
        let m = model();
        m.results[PICKED] = Place::new(if name.is_empty() { "Dropped pin" } else { name }, at);
        m.open(PICKED);
        self.sel = PICKED;
        self.place_at = Some(at);
        self.place_back = self.screen;
        let (x, y) = (
            (at.0.clamp(-32000, 32000) as i16).to_le_bytes(),
            (at.1.clamp(-32000, 32000) as i16).to_le_bytes(),
        );
        let mut msg = [0u8; 5 + 44];
        msg[..5].copy_from_slice(&[b'P', x[0], x[1], y[0], y[1]]);
        let n = name.len().min(44);
        msg[5..5 + n].copy_from_slice(&name.as_bytes()[..n]);
        self.send(&msg[..5 + n]);
        self.go(Screen::Place);
    }

    /// A tap near the rim of the home map: the map moves a third of the panel that way, at once.
    fn edge(&mut self, x: i32, y: i32) {
        let (dx, dy) = (x - C, y - C);
        if dx * dx + dy * dy < EDGE_R * EDGE_R || !scene().active {
            return;
        }
        let step = |d: i32| {
            if d > 70 {
                1
            } else if d < -70 {
                -1
            } else {
                0
            }
        };
        let v = self.home_view();
        let m = v.metres(155);
        let at = (v.at.0 + step(dx) * m, v.at.1 - step(dy) * m);
        self.at = Some(at);
        self.motor.play(haptic::TICK);
        self.draw();
        self.want_more(at, v.k);
    }

    /// Only what the press changed (the map under it stays).
    fn draw_pressed(&mut self) {
        match self.screen {
            Screen::Home => self.draw_home(),
            Screen::Results => self.draw_results(),
            Screen::Route => self.draw_route(),
            _ => {
                self.draw();
                return;
            }
        }
        self.api.show();
    }

    fn press(&mut self, b: u8) {
        self.pressed = None;
        match b {
            1 => self.go(Screen::Results),
            2 => {
                self.at = None;
                if self.asked == Some(FOCUS_SCENE) {
                    // the map is somewhere else (an area, a search): a new one round us
                    self.dropped = None;
                    self.want_map(None);
                    self.draw();
                } else {
                    // back to us: a fresh scene if we are off this one
                    self.draw();
                    let v = self.home_view();
                    self.want_more(v.at, v.k);
                }
            }
            3 => self.go(Screen::Find),
            5 => {
                // the search gone: the map round us again, with its own places
                let m = model();
                m.n_results = 0;
                m.searched = false;
                self.cur = 0;
                self.busy = None;
                self.send(b"K");
                self.at = None;
                self.dropped = None;
                render::show_places(true);
                self.want_map(None);
                self.screen = Screen::Home;
                self.draw();
            }
            4 => {
                let n = nav();
                (n.active, n.arrived, n.rerouting) = (false, false, false);
                self.nav_map = false;
                self.confirm = false;
                self.send(b"S");
                self.api.keep_screen_on(true);
                self.go(Screen::Nav);
            }
            9 => {
                self.area = false;
                self.go(Screen::Search)
            }
            10..=17 => {
                let i = (b - 10) as usize;
                if model().results[i].placed() {
                    self.open_result(i);
                } else {
                    self.focus_row(i);
                }
            }
            35 => {
                self.expanded = true;
                self.draw();
            }
            20..=22 => self.route(b - 20, None),
            24 if self.adding => self.stop_route(Some(self.sel as u8)),
            7 => {
                // a stop: searched for like any place, its card then adds it
                self.adding = true;
                self.go(Screen::Find);
            }
            24 => {
                model().ways = [Way::Asking; 3];
                self.send(&[b'U', self.sel as u8]);
                self.go(Screen::Ways);
            }
            30 => self.end_nav(Screen::Route),
            31 => {
                self.confirm = false;
                self.draw();
            }
            32 => self.end_nav(Screen::Home),
            40 | 41 => {
                self.area = b == 41;
                self.n_query = 0;
                self.go(Screen::Search);
            }
            50..=57 => {
                let i = usize::from(b - 50);
                let t = CATEGORIES[i].as_bytes();
                let n = t.len().min(QUERY);
                self.query[..n].copy_from_slice(&t[..n]);
                self.n_query = n;
                let id = self.focus_scene();
                self.send(&[b'C', id, i as u8]);
                self.searching();
            }
            60..=62 => self.route(self.mode, Some(b - 60)),
            _ => {}
        }
    }

    /// A route by `mode` to the open place (`alt`: the one picked of the ways there); its map comes with it.
    fn route(&mut self, mode: u8, alt: Option<u8>) {
        self.mode = mode;
        let m = model();
        m.routed = false;
        m.n_error = 0;
        m.n_alts = 0;
        self.step = 0;
        self.busy = Some("Finding a route\u{2026}");
        nav().n_pts = 0;
        self.next_id = self.next_id.wrapping_add(1).max(1);
        self.asked = Some(ROUTE_SCENE);
        scene().want = self.next_id;
        self.pic_at = self.api.now();
        self.pic_tries = 2;
        let d = [b'D', mode, self.sel as u8, self.next_id, alt.unwrap_or(0)];
        self.send(&d[..if alt.is_some() { 5 } else { 4 }]);
        self.go(Screen::Route);
    }

    /// The route again with result `idx` as a stop before the end (`J id idx`; None: as it was, its map again).
    fn stop_route(&mut self, idx: Option<u8>) {
        self.adding = false;
        let m = model();
        m.routed = false;
        m.n_error = 0;
        m.n_alts = 0;
        self.step = 0;
        self.busy = Some(if idx.is_some() { "Adding the stop\u{2026}" } else { "Finding a route\u{2026}" });
        nav().n_pts = 0;
        self.next_id = self.next_id.wrapping_add(1).max(1);
        self.asked = Some(ROUTE_SCENE);
        scene().want = self.next_id;
        self.pic_at = self.api.now();
        self.pic_tries = 2;
        self.send(&[b'J', self.next_id, idx.unwrap_or(255)]);
        self.go(Screen::Route);
    }

    /// A map that comes round what was asked for (a search's places, an area), in a new frame.
    fn focus_scene(&mut self) -> u8 {
        self.next_id = self.next_id.wrapping_add(1).max(1);
        self.asked = Some(FOCUS_SCENE);
        scene().want = self.next_id;
        self.pic_at = self.api.now();
        self.pic_tries = 2;
        self.dropped = None;
        self.next_id
    }

    /// A search sent: its places come on the map.
    fn searching(&mut self) {
        let md = model();
        md.n_results = 0;
        md.searched = false;
        md.n_error = 0;
        self.top = 0;
        self.cur = 0;
        self.busy = Some("Searching\u{2026}");
        self.go(Screen::Results);
    }

    fn dial_act(&mut self, a: Act) {
        match a {
            Act::None => {}
            Act::Show => {
                self.motor.play(haptic::TICK);
                self.api.show();
            }
            Act::Redraw => {
                self.motor.play(haptic::TICK);
                self.draw();
            }
            Act::Key(k) => {
                self.motor.play(haptic::TICK);
                match k {
                    Key::Ch(c) if self.n_query < QUERY => {
                        self.query[self.n_query] = c;
                        self.n_query += 1;
                    }
                    Key::Space if self.n_query > 0 && self.n_query < QUERY => {
                        self.query[self.n_query] = b' ';
                        self.n_query += 1;
                    }
                    Key::Del if self.n_query > 0 => {
                        self.n_query -= 1;
                        if self.n_query == 0 {
                            self.dial.shift = true;
                        }
                    }
                    Key::Send if self.n_query > 0 => {
                        self.search();
                        return;
                    }
                    _ => {}
                }
                self.draw_query();
                self.dial.draw_centre(&mut self.s);
                self.api.show();
            }
        }
    }

    /// Go on the keyboard: places (`Q`) or an area (`Z`), each with the map round it.
    fn search(&mut self) {
        let id = self.focus_scene();
        let mut m = [0u8; 2 + QUERY];
        m[0] = if self.area { b'Z' } else { b'Q' };
        m[1] = id;
        m[2..2 + self.n_query].copy_from_slice(&self.query[..self.n_query]);
        self.send(&m[..2 + self.n_query]);
        if self.area {
            model().n_error = 0;
            self.busy = None;
            self.go(Screen::Home);
        } else {
            self.searching();
        }
    }

    // --- messages

    fn messages(&mut self, ask: &mut Ask) {
        let mut buf = [0u8; MSG_MAX];
        let mut redraw = false;
        let show = false;
        while let Some(m) = self.api.recv(&mut buf) {
            ask.answered();
            if matches!(m.first(), Some(b'S' | b'd')) && !(m[0] == b'S' && m.len() != 14) {
                match scene().message(m) {
                    Ev::Begin => {
                        self.pic_at = self.api.now();
                        if self.asked.is_none() {
                            self.at = None;
                        } else if self.asked == Some(FOCUS_SCENE) && scene().id == scene().want {
                            // a search's or an area's map: Home shows where it is
                            self.at = Some(scene().centre);
                        }
                        redraw |= has_map(self.screen);
                    }
                    Ev::More => {
                        self.pic_at = self.api.now();
                        if has_map(self.screen)
                            && self.api.now().wrapping_sub(self.drawn_at) >= REDRAW_MS
                        {
                            redraw = true;
                        }
                    }
                    Ev::Done => redraw |= has_map(self.screen),
                    Ev::None => {}
                }
                continue;
            }
            if matches!(m.first(), Some(b'G' | b'g')) {
                // the route's points, for Route and Nav alike
                nav().apply(m);
                redraw |= matches!(self.screen, Screen::Route | Screen::Nav);
                continue;
            }
            match model().apply(m) {
                Got::Hello => {
                    // the phone woke after our first ask: the same map again (sent with R, it may have been lost)
                    let sc = scene();
                    if sc.want != 0
                        && !(sc.active && sc.id == sc.want)
                        && self.api.now().wrapping_sub(self.pic_at) >= 1000
                    {
                        self.resend();
                    }
                }
                Got::Results => {
                    let m = model();
                    render::show_places(m.n_results == 0);
                    if matches!(self.screen, Screen::Results | Screen::List) && m.searched {
                        let first = self.busy.is_some() && self.pending.is_none();
                        self.busy = None;
                        self.cur = self.cur.min(m.n_results.saturating_sub(1));
                        // the cur on the map: the first placed one
                        if !m.results[self.cur].placed() {
                            self.cur = (0..m.n_results).find(|&i| m.results[i].placed()).unwrap_or(0);
                        }
                        // places round us and a town far away: the list first (Google's), the pins behind it
                        if first && m.near > 0 && m.near < m.n_results && self.screen == Screen::Results {
                            self.expanded = false;
                            self.top = 0;
                            self.screen = Screen::List;
                        }
                    }
                    // the other group's row tapped: once its rows come placed, the town's map or the place's card
                    if let Some(i) = self.pending.filter(|&i| i < m.n_results && m.results[i].placed()) {
                        self.pending = None;
                        self.busy = None;
                        if i == m.near {
                            self.cur = i;
                            self.screen = Screen::Results;
                        } else {
                            self.open_result(i);
                        }
                    }
                    redraw |= matches!(
                        self.screen,
                        Screen::Results | Screen::List | Screen::Home | Screen::Place
                    );
                }
                Got::Ways => redraw |= self.screen == Screen::Ways,
                Got::Info => redraw |= matches!(self.screen, Screen::Place | Screen::About),
                Got::About => redraw |= self.screen == Screen::About,
                Got::Alts => {
                    if self.screen == Screen::Route && model().n_alts >= 2 {
                        self.busy = None;
                        let m = model();
                        self.alt_sel = (0..m.n_alts).min_by_key(|&k| m.alts[k].secs).unwrap_or(0);
                        self.screen = Screen::Pick;
                        redraw = true;
                    }
                    // each `o` as it comes
                    redraw |= self.screen == Screen::Pick;
                }
                Got::Route => {
                    if self.screen == Screen::Route {
                        if self.busy.is_some() {
                            self.step = 0;
                        }
                        self.busy = None;
                        redraw = true;
                    }
                }
                Got::Error => {
                    self.busy = None;
                    // an area or a search the phone couldn't map: the map there stays
                    let sc = scene();
                    if sc.active && sc.want != sc.id && self.asked == Some(FOCUS_SCENE) {
                        sc.want = sc.id;
                    }
                    redraw = true;
                }
                Got::Nav => {}
                Got::None => {
                    if self.screen != Screen::Nav {
                        continue;
                    }
                    let effect = nav().apply(m);
                    if !matches!(effect, Effect::None | Effect::Ended) && nav().heard(self.api.now()) {
                        // the phone is back: the screen without the notice
                        (self.nav_drawn, self.map_up) = (None, false);
                        redraw = true;
                    }
                    match effect {
                        Effect::Turn { buzz } => {
                            view::buzz(&mut self.motor, buzz);
                            redraw = true;
                        }
                        Effect::Arrived => {
                            self.motor.play(view::THERE);
                            self.api.keep_screen_on(false);
                            redraw = true;
                        }
                        Effect::Pos | Effect::Reroute => {
                            redraw = true;
                            // driving off the scene: the next one round where we are
                            if self.nav_map {
                                let n = nav();
                                self.want_more(
                                    (i32::from(n.pos.0), i32::from(n.pos.1)),
                                    ZOOMS[self.nav_zoom],
                                );
                            }
                        }
                        // the phone ended the route (Maps' phone side doesn't today): the screen says it waits
                        Effect::Ended => redraw = true,
                        Effect::None => {}
                    }
                }
            }
        }
        if redraw && self.screen == Screen::Nav && self.api.now().wrapping_sub(self.drawn_at) < FRAME_MS {
            // the panel may still be taking the last frame from the buffer: this one a moment later
            self.nav_due = true;
        } else if redraw {
            self.draw();
        } else if show {
            self.api.show();
        }
    }
}

fn main(api: &'static Api) -> i32 {
    crate::ram::init();
    let mut ui = Ui {
        api,
        s: api,
        screen: Screen::Home,
        motor: Motor::new(api),
        dial: Dial::new(),
        query: [0; QUERY],
        n_query: 0,
        sym: false,
        key_on: None,
        zoom: 2,
        at: None,
        drawn_at: 0,
        sel: 0,
        top: 0,
        mode: 0,
        step: 0,
        next_id: 0,
        asked: None,
        pic_at: api.now(),
        pic_tries: 0,
        busy: None,
        nav_map: false,
        nav_zoom: 2,
        nav_drawn: None,
        map_up: false,
        nav_due: false,
        confirm: false,
        pressed: None,
        area: false,
        hold: None,
        dropped: None,
        place_at: None,
        place_back: Screen::Home,
        cur: 0,
        alt_sel: 0,
        about_top: 0,
        expanded: false,
        adding: false,
        pending: None,
    };
    api.own_back(true);
    ui.draw();
    ui.send(b"R");
    // the map is asked for with the hello too: a phone that is awake answers both
    ui.want_map(None);
    let mut ask = Ask::new(api.now(), 10);
    loop {
        let e = api.event(ui.wait(&ask));
        ui.motor.update();
        if ask.due(api.now()) {
            ui.send(b"R");
        }
        // navigating and the phone went quiet: say so, and ask for the route and where we are again till it answers
        if ui.screen == Screen::Nav && nav().quiet(api.now()) {
            ui.send(b"N");
            (ui.nav_drawn, ui.map_up) = (None, false);
            ui.draw();
        }
        ui.timers();
        if ui.screen == Screen::Dial && ui.dial.tick(&mut ui.s, api.now()) == Act::Show {
            api.show();
        }
        let Some(e) = e else { continue };
        match e.kind {
            BUTTON => {
                if !ui.back() {
                    ui.motor.off();
                    api.keep_screen_on(false);
                    return 0;
                }
            }
            PAUSE => {
                ui.motor.off();
                loop {
                    match api.event(u32::MAX) {
                        Some(e) if e.kind == RESUME => break,
                        Some(e) if e.kind == BUTTON => {
                            if nav().active {
                                ui.send(b"X");
                            }
                            return 0;
                        }
                        _ => {}
                    }
                }
                (ui.nav_drawn, ui.map_up) = (None, false);
                ui.draw();
            }
            TOUCH_DOWN => ui.down(i32::from(e.x), i32::from(e.y)),
            TOUCH_UP if ui.screen == Screen::Dial => {
                let a = ui.dial.up(&mut ui.s, i32::from(e.x), i32::from(e.y));
                ui.dial_act(a);
            }
            TOUCH_UP => ui.up(i32::from(e.x), i32::from(e.y)),
            CROWN => ui.crown(i32::from(e.delta).signum()),
            MESSAGE => ui.messages(&mut ask),
            _ => {}
        }
    }
}
