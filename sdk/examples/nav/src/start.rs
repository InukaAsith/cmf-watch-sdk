//! Starting a route from the watch, as the phone's Navigation page does (the page then shows it too, and its Stop
//! ends it). With no route on: "Where to?" (the rotary keyboard, the phone searches), walking or the roads, and
//! the last places gone to. A place tapped: `S idx walking`; the route's turns follow as usual.
//!
//! The phone's side (CMFX NavService, HubKit NavProto): `P n walking`, then `p idx name 0 detail` each: the places
//! (what was searched for, or the recent ones); `E text`: what it is doing or why it couldn't. Asks: `R` (the
//! recent places when no route is on), `Q text`, `S idx walking`.

use cmf_wp2_app::Api;
use cmf_wp2_ui::color::{rgb565, Rgb, BLACK};
use cmf_wp2_ui::dial::{Act, Dial, Key};
use cmf_wp2_ui::draw::{disc, rrect};
use cmf_wp2_ui::font;
use cmf_wp2_ui::geom::chord;
use cmf_wp2_ui::text::{dots_centre, fit};
use cmf_wp2_ui::{C, Surface};

const PLACES: usize = 8;
const NAME: usize = 40;
const DETAIL: usize = 60;
const QUERY: usize = 40;

const W: u16 = 0xFFFF;
const GREY: u16 = rgb565(0x80, 0x80, 0x80);
const SOFT: u16 = rgb565(0xC8, 0xC8, 0xC8);
const CARD: Rgb = Rgb(0x1C, 0x1C, 0x1E);
const WHITE: Rgb = Rgb(0xFF, 0xFF, 0xFF);

/// Start: the pill, the two modes, the recent rows.
const WHERE: (i32, i32, i32) = (96, 56, 260);
const MODE_Y: i32 = 172;
const MODE_H: i32 = 44;
const ROWS_Y: i32 = 252;
const ROW_H: i32 = 60;
/// List: the rows under the query.
const LIST_Y: i32 = 86;
const LIST_ROWS: usize = 5;

#[derive(Clone, Copy, PartialEq)]
pub enum Screen {
    Start,
    Dial,
    List,
}

#[derive(Clone, Copy)]
struct Place {
    name: [u8; NAME],
    n_name: usize,
    detail: [u8; DETAIL],
    n_detail: usize,
}

impl Place {
    const fn new() -> Self {
        Place { name: [0; NAME], n_name: 0, detail: [0; DETAIL], n_detail: 0 }
    }

    fn name(&self) -> &str {
        core::str::from_utf8(&self.name[..self.n_name]).unwrap_or("")
    }

    fn detail(&self) -> &str {
        core::str::from_utf8(&self.detail[..self.n_detail]).unwrap_or("")
    }
}

pub struct Start {
    pub screen: Screen,
    places: [Place; PLACES],
    n: usize,
    /// the places are what was searched for (else the recent ones)
    searched: bool,
    pub walking: bool,
    query: [u8; QUERY],
    n_query: usize,
    pub dial: Dial,
    note: [u8; 60],
    n_note: usize,
    top: usize,
    pressed: Option<u8>,
}

impl Start {
    pub const fn new() -> Self {
        Start {
            screen: Screen::Start,
            places: [Place::new(); PLACES],
            n: 0,
            searched: false,
            walking: true,
            query: [0; QUERY],
            n_query: 0,
            dial: Dial::new(),
            note: [0; 60],
            n_note: 0,
            top: 0,
            pressed: None,
        }
    }

    fn note(&self) -> &str {
        core::str::from_utf8(&self.note[..self.n_note]).unwrap_or("")
    }

    fn set_note(&mut self, t: &[u8]) {
        let t = fit(t, 60).as_bytes();
        self.note[..t.len()].copy_from_slice(t);
        self.n_note = t.len();
    }

    /// A phone's message for these screens (P, p, E): true when it was one (redraw).
    pub fn apply(&mut self, m: &[u8]) -> bool {
        match m.first() {
            Some(b'P') if m.len() >= 3 => {
                self.n = usize::from(m[1]).min(PLACES);
                self.walking = m[2] == 1;
                self.searched = self.screen != Screen::Start;
                self.top = 0;
                if self.n > 0 {
                    self.n_note = 0;
                }
                for p in &mut self.places[..self.n] {
                    p.n_name = 0;
                    p.n_detail = 0;
                }
                true
            }
            Some(b'p') if m.len() >= 2 && usize::from(m[1]) < PLACES => {
                let p = &mut self.places[usize::from(m[1])];
                let rest = &m[2..];
                let cut = rest.iter().position(|&b| b == 0).unwrap_or(rest.len());
                let name = fit(&rest[..cut], NAME).as_bytes();
                p.name[..name.len()].copy_from_slice(name);
                p.n_name = name.len();
                let detail = fit(rest.get(cut + 1..).unwrap_or(&[]), DETAIL).as_bytes();
                p.detail[..detail.len()].copy_from_slice(detail);
                p.n_detail = detail.len();
                true
            }
            Some(b'E') => {
                self.set_note(&m[1..]);
                true
            }
            _ => false,
        }
    }

    // --- drawing (the caller clears the panel and shows)

    pub fn draw(&self, api: &'static Api) {
        let mut s = api;
        match self.screen {
            Screen::Start => self.draw_start(&mut s),
            Screen::Dial => {
                self.draw_query(&mut s);
                self.dial.draw(&mut s);
            }
            Screen::List => self.draw_list(&mut s),
        }
    }

    fn pill(&self, s: &mut &'static Api, cx: i32, y: i32, h: i32, w: i32, text: &str, on: bool, id: u8) {
        let lit = on || self.pressed == Some(id);
        rrect(s, cx - w / 2, y, w, h, h / 2, if lit { WHITE } else { CARD }, BLACK);
        let tw = font::width(text).min(w - 24);
        font::fitted(s, cx - tw / 2, y + h / 2 - 12, text, w - 24, if lit { 0 } else { W });
    }

    fn draw_start(&self, s: &mut &'static Api) {
        dots_centre(s, C - 8, 34, "NAV", 3, 2, W);
        disc(s, C + 34, 52, 4, crate::ACCENT, BLACK);
        let (y, h, w) = WHERE;
        self.pill(s, C, y, h, w, "Where to?", false, 1);
        self.pill(s, C - 66, MODE_Y, MODE_H, 120, "Walk", self.walking, 2);
        self.pill(s, C + 66, MODE_Y, MODE_H, 120, "Roads", !self.walking, 3);
        if self.n_note > 0 {
            centre_fit(s, C, 230, self.note(), 300, SOFT);
        }
        if self.searched || self.n == 0 {
            if self.n_note == 0 {
                font::centre(s, C, 260, "Or start one on your phone", GREY);
            }
            return;
        }
        for i in 0..self.n.min(3) {
            self.row(s, ROWS_Y + i as i32 * ROW_H, i);
        }
    }

    fn row(&self, s: &mut &'static Api, y: i32, i: usize) {
        let (x0, x1) = chord(y, y + ROW_H - 6, 26);
        let on = self.pressed == Some(10 + i as u8);
        rrect(s, x0, y, x1 - x0, ROW_H - 6, 18, if on { Rgb(0x3A, 0x3A, 0x3A) } else { CARD }, BLACK);
        let p = &self.places[i];
        font::fitted(s, x0 + 20, y + 4, p.name(), x1 - x0 - 40, W);
        font::fitted(s, x0 + 20, y + 28, p.detail(), x1 - x0 - 40, GREY);
    }

    fn draw_query(&self, s: &mut &'static Api) {
        let q = core::str::from_utf8(&self.query[..self.n_query]).unwrap_or("");
        Surface::rect(s, 40, 100, 386, 40, 0);
        centre_fit(s, C, 110, if q.is_empty() { "Where to?" } else { q }, 300, if q.is_empty() { GREY } else { W });
    }

    fn draw_list(&self, s: &mut &'static Api) {
        let q = core::str::from_utf8(&self.query[..self.n_query]).unwrap_or("");
        centre_fit(s, C, 40, q, 220, SOFT);
        if self.n == 0 || !self.searched {
            centre_fit(s, C, 222, if self.n_note > 0 { self.note() } else { "Searching\u{2026}" }, 320, GREY);
            return;
        }
        for r in 0..LIST_ROWS {
            let i = self.top + r;
            if i >= self.n {
                break;
            }
            self.row(s, LIST_Y + r as i32 * (ROW_H + 4), i);
        }
        if self.n_note > 0 {
            centre_fit(s, C, 418, self.note(), 260, SOFT);
        }
    }

    // --- input

    /// What a touch at (x, y) is on: 1 "Where to?", 2 walk, 3 roads, 10 + i a place.
    pub fn hit(&self, x: i32, y: i32) -> Option<u8> {
        let pill = |cx: i32, top: i32, h: i32, w: i32| (top - 6..top + h + 6).contains(&y) && (x - cx).abs() <= w / 2 + 6;
        match self.screen {
            Screen::Start => {
                let (wy, wh, ww) = WHERE;
                if pill(C, wy, wh, ww) {
                    return Some(1);
                }
                if pill(C - 66, MODE_Y, MODE_H, 120) {
                    return Some(2);
                }
                if pill(C + 66, MODE_Y, MODE_H, 120) {
                    return Some(3);
                }
                let r = (y - ROWS_Y).div_euclid(ROW_H);
                (!self.searched && y >= ROWS_Y && (0..self.n.min(3) as i32).contains(&r)).then(|| 10 + r as u8)
            }
            Screen::List => {
                let r = (y - LIST_Y).div_euclid(ROW_H + 4);
                let i = self.top as i32 + r;
                (self.searched && y >= LIST_Y && (0..LIST_ROWS as i32).contains(&r) && (i as usize) < self.n)
                    .then(|| 10 + i as u8)
            }
            Screen::Dial => None,
        }
    }

    /// A tap that was on `id`: what to send the phone (search words, a start), if anything.
    pub fn act(&mut self, id: u8, out: &mut [u8; 64]) -> usize {
        self.pressed = None;
        match id {
            1 => {
                self.n_query = 0;
                self.dial.shift = true;
                self.dial.cancel();
                self.screen = Screen::Dial;
                0
            }
            2 | 3 => {
                self.walking = id == 2;
                0
            }
            10.. => {
                out[..3].copy_from_slice(&[b'S', id - 10, self.walking as u8]);
                self.set_note(b"Finding a route\xE2\x80\xA6");
                3
            }
            _ => 0,
        }
    }

    /// A key typed on the dial: the words to search for once sent (`Q text`).
    pub fn key(&mut self, a: Act, out: &mut [u8; 64]) -> usize {
        let Act::Key(k) = a else { return 0 };
        match k {
            Key::Ch(c) if self.n_query < QUERY => {
                self.query[self.n_query] = c;
                self.n_query += 1;
            }
            Key::Space if self.n_query > 0 && self.n_query < QUERY => {
                self.query[self.n_query] = b' ';
                self.n_query += 1;
            }
            Key::Del if self.n_query > 0 => self.n_query -= 1,
            Key::Send if self.n_query > 0 => {
                out[0] = b'Q';
                out[1..1 + self.n_query].copy_from_slice(&self.query[..self.n_query]);
                self.n = 0;
                self.n_note = 0;
                self.searched = true;
                self.screen = Screen::List;
                return 1 + self.n_query;
            }
            _ => {}
        }
        0
    }

    /// The typed words again, over what was there (no full redraw).
    pub fn redraw_query(&self, api: &'static Api) {
        let mut s = api;
        self.draw_query(&mut s);
        self.dial.draw_centre(&mut s);
    }

    pub fn crown(&mut self, d: i32) -> bool {
        if self.screen != Screen::List {
            return false;
        }
        let t = (self.top as i32 + d).clamp(0, self.n.saturating_sub(LIST_ROWS) as i32) as usize;
        let moved = t != self.top;
        self.top = t;
        moved
    }

    /// Back: the keyboard or the list to the start (the recent places asked again: `R`); false on the start.
    pub fn back(&mut self) -> bool {
        if self.screen == Screen::Start {
            return false;
        }
        self.dial.cancel();
        self.screen = Screen::Start;
        self.searched = false;
        self.n = 0;
        self.n_note = 0;
        true
    }
}

fn centre_fit(s: &mut &'static Api, cx: i32, y: i32, text: &str, px: i32, c: u16) {
    let w = font::width(text).min(px);
    font::fitted(s, cx - w / 2, y, text, px, c);
}
