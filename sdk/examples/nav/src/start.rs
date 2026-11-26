//! Starting a route from the watch, as the phone's Navigation page does (the page then shows it too, and its Stop
//! ends it). With no route on: "Where to?" (a keyboard as Spotify's, its "dial" key the rotary one; the phone
//! searches), walk or drive, and the last places gone to. A place tapped: `S idx walking`; the route's turns follow as usual.
//!
//! The phone's side (CMFX NavService, HubKit NavProto): `P n walking`, then `p idx name 0 detail` each: the places
//! (what was searched for, or the recent ones); `E text`: what it is doing or why it couldn't. Asks: `R` (the
//! recent places when no route is on), `Q text`, `S idx walking`.

use cmf_wp2_ui::dictate::{Dictate, Out as DictOut};
use cmf_wp2_app::Api;
use cmf_wp2_ui::color::{rgb565, Rgb, BLACK};
use cmf_wp2_ui::dial::{Act, Dial, Key};
use cmf_wp2_ui::draw::rrect;
use cmf_wp2_ui::font;
use cmf_wp2_ui::geom::chord;
use cmf_wp2_ui::text::fit;
use cmf_wp2_ui::{C, Surface};

const PLACES: usize = 8;
const NAME: usize = 40;
const DETAIL: usize = 60;
const QUERY: usize = 40;
/// the voice typing button, right of the typed words
const MIC_X: i32 = 404;
const MIC_Y: i32 = 120;

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
/// Keys (as Spotify's): rows' tops, key height.
const ROW_Y: [i32; 4] = [184, 242, 300, 358];
const KH: i32 = 58;
const KEYC: Rgb = Rgb(0x2A, 0x2A, 0x2A);
const RED: Rgb = Rgb(0xD7, 0x19, 0x21);
/// List: the rows under the query.
const LIST_Y: i32 = 86;
const LIST_ROWS: usize = 5;

#[derive(Clone, Copy, PartialEq)]
pub enum Screen {
    Start,
    /// the keyboard
    Keys,
    /// the rotary keyboard
    Dial,
    List,
    /// voice typing into the query (cmf_wp2_ui::dictate)
    Voice,
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
    /// the keyboard's numbers and signs
    sym: bool,
    /// the key lit since it was typed (till the next touch)
    lit: Option<K>,
    pub dict: &'static mut Dictate,
}

#[derive(Clone, Copy, PartialEq)]
pub enum K {
    Ch(u8),
    Del,
    Sym,
    Space,
    Go,
    Dial,
}

/// What a touch on the keyboard did.
pub enum Tap {
    /// drawn: show
    Shown,
    /// a key typed: tick and show
    Typed,
    /// draw the whole screen
    Redraw,
    /// send `out[..n]` (the search), then draw the whole screen
    Send(usize),
}

impl Start {
    pub fn new() -> Self {
        Start {
            screen: Screen::Start,
            places: [Place::new(); PLACES],
            n: 0,
            searched: false,
            walking: true,
            query: [0; QUERY],
            dict: cmf_wp2_ui::dictate::kept(),
            n_query: 0,
            dial: Dial::new(),
            note: [0; 60],
            n_note: 0,
            top: 0,
            pressed: None,
            sym: false,
            lit: None,
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
            Screen::Keys => {
                self.draw_query(&mut s);
                let sym = self.sym;
                each_key(sym, |k, _, _, _| draw_key(&mut s, sym, k, false));
                cmf_wp2_ui::dictate::mic_button(&mut s, MIC_X, MIC_Y);
            }
            Screen::Voice => self.dict.draw(&mut s),
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
        cmf_wp2_ui::logo::title(s, 34, "Navigation", crate::ACCENT);
        let (y, h, w) = WHERE;
        self.pill(s, C, y, h, w, "Where to?", false, 1);
        self.pill(s, C - 66, MODE_Y, MODE_H, 120, "Walk", self.walking, 2);
        self.pill(s, C + 66, MODE_Y, MODE_H, 120, "Drive", !self.walking, 3);
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
        if q.is_empty() {
            return centre_fit(s, C, 110, "Where to?", 300, GREY);
        }
        // the end of a long query stays in view
        let max = if self.screen == Screen::Dial { 200 } else { 250 };
        let mut start = 0;
        while font::width(&q[start..]) > max && start < q.len() {
            start += 1;
            while !q.is_char_boundary(start) {
                start += 1;
            }
        }
        centre_fit(s, C, 110, &q[start..], max, W);
    }

    /// On the rotary keyboard, a touch on the typed words: back to the keys.
    pub fn on_query(&self, x: i32, y: i32) -> bool {
        self.screen == Screen::Dial && (90..150).contains(&y) && (x - C).abs() < 140
    }

    pub fn to_keys(&mut self) {
        self.dial.cancel();
        self.screen = Screen::Keys;
    }

    /// A touch on the keyboard: typed at once, the key lit till the next touch (as Spotify's).
    pub fn keys_down(&mut self, api: &'static Api, x: i32, y: i32, out: &mut [u8; 64]) -> Tap {
        let mut s = api;
        let sym = self.sym;
        if let Some(old) = self.lit.take() {
            draw_key(&mut s, sym, old, false);
        }
        let mut hit = None;
        each_key(sym, |k, kx, ky, w| {
            if (kx..kx + w).contains(&x) && (ky..ky + KH).contains(&y) {
                hit = Some(k);
            }
        });
        if cmf_wp2_ui::dictate::mic_hit(MIC_X, MIC_Y, x, y) {
            self.lit = None;
            self.dict.start(api, false);
            self.screen = Screen::Voice;
            return Tap::Redraw;
        }
        let Some(k) = hit else { return Tap::Shown };
        match k {
            K::Ch(c) if self.n_query < QUERY => {
                // the first letter a capital, as the dial does
                self.query[self.n_query] = if self.n_query == 0 { c.to_ascii_uppercase() } else { c };
                self.n_query += 1;
            }
            K::Space if self.n_query > 0 && self.n_query < QUERY && self.query[self.n_query - 1] != b' ' => {
                self.query[self.n_query] = b' ';
                self.n_query += 1;
            }
            K::Del => self.n_query = self.n_query.saturating_sub(1),
            K::Sym => {
                self.sym = !self.sym;
                return Tap::Redraw;
            }
            K::Dial => {
                self.dial.shift = self.n_query == 0;
                self.dial.cancel();
                self.screen = Screen::Dial;
                return Tap::Redraw;
            }
            K::Go => {
                let n = self.send_query(out);
                return if n > 0 { Tap::Send(n) } else { Tap::Shown };
            }
            _ => {}
        }
        self.draw_query(&mut s);
        draw_key(&mut s, sym, k, true);
        self.lit = Some(k);
        Tap::Typed
    }

    /// `Q text` into `out`, to the list (0: nothing typed).
    fn send_query(&mut self, out: &mut [u8; 64]) -> usize {
        if self.n_query == 0 {
            return 0;
        }
        out[0] = b'Q';
        out[1..1 + self.n_query].copy_from_slice(&self.query[..self.n_query]);
        self.n = 0;
        self.n_note = 0;
        self.searched = true;
        self.lit = None;
        self.dial.cancel();
        self.screen = Screen::List;
        1 + self.n_query
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
            Screen::Keys | Screen::Dial | Screen::Voice => None,
        }
    }

    /// A tap that was on `id`: what to send the phone (search words, a start), if anything.
    pub fn act(&mut self, id: u8, out: &mut [u8; 64]) -> usize {
        self.pressed = None;
        match id {
            1 => {
                self.n_query = 0;
                self.sym = false;
                self.lit = None;
                self.screen = Screen::Keys;
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
            Key::Send if self.n_query > 0 => return self.send_query(out),
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

    /// Back: the rotary keyboard to the keys, the keys or the list to the start (then the recent places are asked
    /// again: `R`); false on the start.
    /// What the dictation screen asks for: the words into the query. True: draw again.
    pub fn voice_out(&mut self, o: DictOut) -> bool {
        match o {
            DictOut::None => false,
            DictOut::Redraw => true,
            DictOut::Done | DictOut::Cancel => {
                if o == DictOut::Done {
                    let t = self.dict.text().trim().trim_end_matches('.');
                    if self.n_query > 0 && self.query[self.n_query - 1] != b' ' && self.n_query < QUERY && !t.is_empty() {
                        self.query[self.n_query] = b' ';
                        self.n_query += 1;
                    }
                    let mut k = t.len().min(QUERY - self.n_query);
                    while !t.is_char_boundary(k) {
                        k -= 1;
                    }
                    self.query[self.n_query..self.n_query + k].copy_from_slice(&t.as_bytes()[..k]);
                    self.n_query += k;
                }
                self.screen = Screen::Keys;
                true
            }
        }
    }

    pub fn back(&mut self) -> bool {
        match self.screen {
            Screen::Start => return false,
            Screen::Dial => {
                self.to_keys();
                return true;
            }
            _ => {}
        }
        self.dial.cancel();
        self.lit = None;
        self.screen = Screen::Start;
        self.searched = false;
        self.n = 0;
        self.n_note = 0;
        true
    }
}

/// One keyboard row: up to 10 keys `(key, width)`, centred (Spotify's layout).
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
    for (r, &y) in ROW_Y.iter().enumerate() {
        let (keys, n) = row(r, sym);
        let total: i32 = keys[..n].iter().map(|k| k.1).sum();
        let mut x = C - total / 2;
        for &(k, w) in &keys[..n] {
            f(k, x, y, w);
            x += w;
        }
    }
}

fn draw_key(s: &mut &'static Api, sym: bool, key: K, pressed: bool) {
    each_key(sym, |k, x, y, w| {
        if k != key {
            return;
        }
        let fill = match (pressed, k) {
            (true, _) => WHITE,
            (_, K::Go) => RED,
            (_, K::Ch(_)) => KEYC,
            _ => CARD,
        };
        Surface::rect(s, x, y, w, KH, 0);
        rrect(s, x + 3, y + 3, w - 6, KH - 6, 14, fill, BLACK);
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
        let ink = if pressed { 0 } else if k == K::Space { GREY } else { W };
        let lw = font::width(label);
        font::draw(s, x + w / 2 - lw / 2, y + KH / 2 - 12, label, ink);
    });
}

fn centre_fit(s: &mut &'static Api, cx: i32, y: i32, text: &str, px: i32, c: u16) {
    let w = font::width(text).min(px);
    font::fitted(s, cx - w / 2, y, text, px, c);
}
