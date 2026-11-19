//! Spotify: now playing with the album art, a red progress ring on the rim, controls, and the crown for the
//! volume; Library (playlists → tracks), Search (QWERTY) and Devices (Spotify Connect) as lists. The phone does
//! every Web API call; the watch counts the progress itself, so nothing comes over the air while a song plays.

use crate::model::{Change, Model, LIKED, MAX_ROWS, PREMIUM};
use crate::rle::{self, Decoder, HEADER};
use cmf_wp2_app::{Api, Event, BUTTON, CROWN, MESSAGE, MSG_MAX, PAUSE, RESUME, TOUCH_DOWN, TOUCH_UP};
use cmf_wp2_ui::color::{rgb565, Rgb, BLACK};
use cmf_wp2_ui::draw::{arc, disc, ring, rrect};
use cmf_wp2_ui::font::{self, centre, clip, fitted};
use cmf_wp2_ui::geom::chord;
use cmf_wp2_ui::haptic::{self, Motor};
use cmf_wp2_ui::text::dots;
use cmf_wp2_ui::{C, Surface};

cmf_wp2_app::entry!(main);
cmf_wp2_app::app_icon!();

static mut MODEL: Model = Model::new();

fn model() -> &'static mut Model {
    // one app thread
    unsafe { &mut *core::ptr::addr_of_mut!(MODEL) }
}

const WHITE: Rgb = Rgb(0xFF, 0xFF, 0xFF);
const RED: Rgb = Rgb(0xD7, 0x19, 0x21);
const CARD: Rgb = Rgb(0x17, 0x17, 0x17);
const KEYC: Rgb = Rgb(0x26, 0x26, 0x26);
const TRACK: Rgb = Rgb(0x2A, 0x2A, 0x2A);
const W: u16 = 0xFFFF;
const SOFT: u16 = rgb565(0xC8, 0xC8, 0xC8);
const GREY: u16 = rgb565(0x80, 0x80, 0x80);
const DIM: u16 = rgb565(0x3A, 0x3A, 0x3A);

// --- now playing
const ART: i32 = 152;
const ART_X: i32 = C - ART / 2;
const ART_Y: i32 = 60;
const RING: (i32, i32) = (224, 229);
const CTRL_Y: i32 = 312;
const CTRL_X: [i32; 3] = [C - 96, C, C + 96];
const TOOL_Y: i32 = 392;
/// Taps this far from the centre (outside the tools) seek on the progress ring.
const SEEK_R: i32 = 196;
const TOOL_X: [i32; 4] = [C - 99, C - 33, C + 33, C + 99];
// --- lists
const LIST_TOP: i32 = 100;
const ROW_H: i32 = 70;
const PITCH: i32 = 78;
// --- keyboard
const KH: i32 = 58;
const ROW_Y: [i32; 4] = [184, 242, 300, 358];
const QUERY_MAX: usize = 60;

/// Album art: the last `I` header and its run tokens, drawn again on every full redraw.
const ART_CAP: usize = 24 * 1024;
struct Art {
    header: [u8; HEADER],
    data: [u8; ART_CAP],
    len: usize,
    ok: bool,
}
static mut ART_STORE: Art = Art { header: [0; HEADER], data: [0; ART_CAP], len: 0, ok: false };

fn art() -> &'static mut Art {
    unsafe { &mut *core::ptr::addr_of_mut!(ART_STORE) }
}

#[derive(Clone, Copy, PartialEq)]
enum Screen {
    Now,
    List,
    Search,
}

#[derive(Clone, Copy, PartialEq)]
enum K {
    Ch(u8),
    Del,
    Sym,
    Space,
    Go,
}

struct Screen565(&'static Api);
impl rle::Surface for Screen565 {
    fn rect(&mut self, x: i32, y: i32, w: i32, h: i32, c: u16) {
        let mut a = self.0;
        Surface::rect(&mut a, x, y, w, h, c);
    }
}

/// Body text in Inter.
#[derive(Clone, Copy)]
struct Ink(&'static Api);
impl Surface for Ink {
    fn rect(&mut self, x: i32, y: i32, w: i32, h: i32, c: u16) {
        let mut a = self.0;
        Surface::rect(&mut a, x, y, w, h, c);
    }
    fn text(&mut self, x: i32, y: i32, s: &str, c: u16) {
        font::draw(self, x, y, s, c);
    }
}

struct Ui {
    api: &'static Api,
    s: Ink,
    screen: Screen,
    scroll: i32,
    /// the progress ring as last drawn, 0..=65536
    drawn: u32,
    /// local clock for the progress: when pos was last advanced
    tick_at: u32,
    /// a note or the volume over the device line until then
    toast_until: Option<u32>,
    query: [u8; QUERY_MAX],
    n_query: usize,
    sym: bool,
    pressed: Option<K>,
    down: Option<(i32, i32)>,
    motor: Motor,
}

impl Ui {
    fn send(&self, m: &[u8]) {
        let _ = self.api.send(m);
    }

    fn draw(&mut self) {
        Surface::rect(&mut self.s, 0, 0, 466, 466, 0);
        match self.screen {
            Screen::Now => self.draw_now(),
            Screen::List => self.draw_list(),
            Screen::Search => self.draw_search(),
        }
        self.api.show();
    }

    // --- now playing

    fn draw_now(&mut self) {
        let m = model();
        let n = &m.now;
        self.draw_ring(true);
        self.draw_top();
        // art, or a dark square with a note while it comes
        let a = art();
        let mut drawn = false;
        if a.ok {
            let mut d = Decoder::new();
            if d.start(&a.header, ART_X, ART_Y, 1) {
                d.feed(&a.data[..a.len], &mut Screen565(self.api));
                drawn = true;
            }
        }
        if !drawn {
            rrect(&mut self.s, ART_X, ART_Y, ART, ART, 18, CARD, BLACK);
            Surface::rect(&mut self.s, C - 2, ART_Y + 52, 5, 44, DIM);
            disc(&mut self.s, C - 9, ART_Y + 98, 10, Rgb(0x3A, 0x3A, 0x3A), CARD);
        }
        let title = if n.flags & crate::model::IDLE != 0 { "Nothing playing" } else { n.title.as_str() };
        let (t, cut) = clip(title, 290);
        if cut {
            fitted(&mut self.s, C - 145, 222, title, 290, W);
        } else {
            centre(&mut self.s, C, 222, t, W);
        }
        let (ar, _) = clip(n.artist.as_str(), 260);
        centre(&mut self.s, C, 249, ar, GREY);
        self.draw_controls();
        self.draw_tools();
    }

    /// Device name (or the toast) at the top.
    fn draw_top(&mut self) {
        let m = model();
        // inside the ring: at y 30 the ring's inner edge is outside x 140..326
        Surface::rect(&mut self.s, 140, 28, 186, 28, 0);
        if self.toast_until.is_some() {
            let note = m.note.as_str();
            let w = font::width(note).min(150) + 28;
            rrect(&mut self.s, C - w / 2, 29, w, 26, 13, KEYC, BLACK);
            fitted(&mut self.s, C - w / 2 + 14, 31, note, 150, W);
            return;
        }
        let dev = m.now.device.as_str();
        if !dev.is_empty() {
            let (d, _) = clip(dev, 150);
            let w = font::width(d) + 16;
            disc(&mut self.s, C - w / 2 + 3, 42, 4, RED, BLACK);
            font::draw(&mut self.s, C - w / 2 + 14, 31, d, GREY);
        }
    }

    /// The progress ring; `full`: the track too, else only the part that moved since the last draw.
    fn draw_ring(&mut self, full: bool) {
        let n = &model().now;
        let want = if n.dur == 0 { 0 } else { (u32::from(n.pos.min(n.dur)) * 65536 / u32::from(n.dur)).min(65536) };
        let top = 49152u16;
        if full || want < self.drawn {
            arc(&mut self.s, C, C, RING.0, RING.1, top, 65536, TRACK, None, BLACK);
            self.drawn = 0;
        }
        if want > self.drawn {
            arc(&mut self.s, C, C, RING.0, RING.1, top.wrapping_add(self.drawn as u16), want - self.drawn, RED, None, BLACK);
            self.drawn = want;
        }
    }

    fn draw_controls(&mut self) {
        let s = &mut self.s;
        let playing = model().playing();
        // previous: bar + triangle pointing left; next: mirrored
        for (cx, dir) in [(CTRL_X[0], -1), (CTRL_X[2], 1)] {
            ring(s, cx, CTRL_Y, 27, 29, Rgb(0x50, 0x50, 0x50), BLACK);
            triangle(s, cx - dir * 2, CTRL_Y, 11, dir, W);
            Surface::rect(s, cx + dir * 10 - 2, CTRL_Y - 11, 4, 22, W);
        }
        disc(s, C, CTRL_Y, 36, RED, BLACK);
        if playing {
            Surface::rect(s, C - 11, CTRL_Y - 13, 8, 26, W);
            Surface::rect(s, C + 3, CTRL_Y - 13, 8, 26, W);
        } else {
            triangle(s, C + 3, CTRL_Y, 14, 1, W);
        }
    }

    fn draw_tools(&mut self) {
        let s = &mut self.s;
        let liked = model().now.flags & LIKED != 0;
        for (i, &x) in TOOL_X.iter().enumerate() {
            ring(s, x, TOOL_Y, 21, 23, Rgb(0x50, 0x50, 0x50), BLACK);
            let y = TOOL_Y;
            match i {
                // like: a heart from two discs and a triangle
                0 => {
                    let c = if liked { RED } else { WHITE };
                    disc(s, x - 5, y - 3, 6, c, BLACK);
                    disc(s, x + 5, y - 3, 6, c, BLACK);
                    for k in 0..10 {
                        Surface::rect(s, x - 10 + k, y - 1 + k, 20 - 2 * k, 1, c.rgb565());
                    }
                }
                // library: three lines
                1 => {
                    for k in 0..3 {
                        Surface::rect(s, x - 9, y - 8 + k * 7, 18, 3, W);
                    }
                }
                // search: lens + handle
                2 => {
                    ring(s, x - 3, y - 3, 6, 9, WHITE, BLACK);
                    for k in 0..6 {
                        Surface::rect(s, x + 3 + k, y + 3 + k, 3, 3, W);
                    }
                }
                // devices: a speaker
                _ => {
                    rrect(s, x - 8, y - 11, 16, 22, 4, WHITE, BLACK);
                    disc(s, x, y + 3, 4, BLACK, WHITE);
                    disc(s, x, y - 6, 2, BLACK, WHITE);
                }
            }
        }
    }

    fn now_tap(&mut self, x: i32, y: i32) {
        let near = |cx: i32, cy: i32, r: i32| (x - cx) * (x - cx) + (y - cy) * (y - cy) <= r * r;
        let premium = model().now.flags & PREMIUM != 0;
        if near(CTRL_X[1], CTRL_Y, 44) || near(CTRL_X[0], CTRL_Y, 38) || near(CTRL_X[2], CTRL_Y, 38) {
            self.motor.play(haptic::TICK);
            if !premium {
                return self.toast(b"Premium needed");
            }
            if near(CTRL_X[1], CTRL_Y, 44) {
                // at once on the watch; the phone confirms with N
                model().now.flags ^= crate::model::PLAYING;
                self.tick_at = self.api.now();
                self.send(b"kp");
                self.draw_controls();
                self.api.show();
            } else {
                self.send(if x < C { b"kb" } else { b"kn" });
            }
            return;
        }
        for (i, &tx) in TOOL_X.iter().enumerate() {
            if near(tx, TOOL_Y, 30) {
                self.motor.play(haptic::TICK);
                match i {
                    0 => {
                        model().now.flags ^= LIKED;
                        self.send(b"h");
                        self.draw_tools();
                        self.api.show();
                    }
                    1 => self.open_list(b'P'),
                    2 => {
                        self.screen = Screen::Search;
                        self.n_query = 0;
                        self.sym = false;
                        self.draw();
                    }
                    _ => self.open_list(b'D'),
                }
                return;
            }
        }
        // a tap on the art: add the song to a playlist
        if (ART_X..ART_X + ART).contains(&x) && (ART_Y..ART_Y + ART).contains(&y) {
            self.motor.play(haptic::TICK);
            self.open_list(b'A');
            return;
        }
        // a tap at the rim: seek to that point of the progress ring (clockwise from the top)
        let (dx, dy) = (x - C, y - C);
        if dx * dx + dy * dy >= SEEK_R * SEEK_R {
            self.seek(dx, dy);
        }
    }

    fn seek(&mut self, dx: i32, dy: i32) {
        let n = &mut model().now;
        if n.dur == 0 {
            return;
        }
        self.motor.play(haptic::TICK);
        if n.flags & PREMIUM == 0 {
            return self.toast(b"Premium needed");
        }
        let frac = u32::from(cmf_wp2_ui::motion::angle(dx, dy).wrapping_sub(49152));
        let pos = (frac * u32::from(n.dur) / 65536) as u16;
        n.pos = pos;
        self.tick_at = self.api.now();
        let p = pos.to_le_bytes();
        self.send(&[b'j', p[0], p[1]]);
        // the time where the device name is, for a moment
        let mut t = [0u8; 8];
        let mut k = 0;
        let (m, sec) = (pos / 60, pos % 60);
        if m >= 10 {
            t[k] = b'0' + (m / 10 % 10) as u8;
            k += 1;
        }
        t[k] = b'0' + (m % 10) as u8;
        t[k + 1] = b':';
        t[k + 2] = b'0' + (sec / 10) as u8;
        t[k + 3] = b'0' + (sec % 10) as u8;
        self.draw_ring(true);
        self.toast(&t[..k + 4]);
    }

    fn toast(&mut self, text: &[u8]) {
        model().note.set(text);
        self.toast_until = Some(self.api.now().wrapping_add(1800));
        if self.screen == Screen::Now {
            self.draw_top();
            self.api.show();
        }
    }

    fn volume(&mut self, delta: i32) {
        let n = &mut model().now;
        n.vol = (i32::from(n.vol) + delta * 5).clamp(0, 100) as u8;
        self.send(&[b'v', n.vol]);
        let mut b = [0u8; 11];
        let v = cmf_wp2_ui::text::int(i32::from(n.vol), &mut b);
        let mut t = [0u8; 16];
        t[..7].copy_from_slice(b"Volume ");
        t[7..7 + v.len()].copy_from_slice(v.as_bytes());
        let len = 7 + v.len();
        let t: [u8; 16] = t;
        self.toast(&t[..len]);
    }

    // --- lists

    fn open_list(&mut self, kind: u8) {
        let l = &mut model().list;
        l.kind = kind;
        l.n = 0;
        l.want = 1;
        self.scroll = 0;
        self.screen = Screen::List;
        self.send(&[b'o', kind]);
        self.draw();
    }

    fn draw_list(&mut self) {
        let l = &model().list;
        for i in 0..l.n {
            let y = LIST_TOP + i as i32 * PITCH - self.scroll;
            if y + ROW_H < 92 || y > 466 {
                continue;
            }
            let (x0, x1) = chord(y.max(92), (y + ROW_H).min(466), 22);
            if x1 - x0 < 150 {
                continue;
            }
            let r = &l.rows[i];
            rrect(&mut self.s, x0, y, x1 - x0, ROW_H, ROW_H / 2, CARD, BLACK);
            let tx = x0 + 30;
            let right = x1 - if r.active { 54 } else { 30 };
            fitted(&mut self.s, tx, y + 12, r.title.as_str(), right - tx, if r.active { W } else { SOFT });
            fitted(&mut self.s, tx, y + 38, r.sub.as_str(), right - tx, GREY);
            if r.active {
                disc(&mut self.s, x1 - 34, y + ROW_H / 2, 6, RED, CARD);
            }
        }
        if l.n < l.want {
            centre(&mut self.s, C, if l.n == 0 { C - 8 } else { LIST_TOP + l.n as i32 * PITCH - self.scroll + 20 }, "Loading", GREY);
        } else if l.n == 0 {
            centre(&mut self.s, C, C - 8, "Nothing here", GREY);
        }
        Surface::rect(&mut self.s, 0, 0, 466, 92, 0);
        let title = match l.kind {
            b'P' => "LIBRARY",
            b'T' => "TRACKS",
            b'S' => "RESULTS",
            b'D' => "DEVICES",
            _ => "ADD TO",
        };
        heading(&mut self.s, 36, title);
    }

    fn list_max(&self) -> i32 {
        (model().list.n as i32 * PITCH - (466 - LIST_TOP - 60)).max(0)
    }

    fn list_tap(&mut self, x: i32, y: i32) {
        let _ = x;
        let l = &model().list;
        if y < 92 {
            return;
        }
        let i = (y + self.scroll - LIST_TOP).div_euclid(PITCH);
        if !(0..l.n as i32).contains(&i) || (y + self.scroll - LIST_TOP) % PITCH > ROW_H {
            return;
        }
        self.motor.play(haptic::TICK);
        let kind = l.kind;
        self.send(&[b'p', kind, i as u8]);
        if kind == b'P' {
            // the playlist's tracks come next, in this screen
            let l = &mut model().list;
            l.kind = b'T';
            l.n = 0;
            l.want = 1;
            self.scroll = 0;
        } else {
            self.screen = Screen::Now;
        }
        self.draw();
    }

    // --- search

    fn draw_search(&mut self) {
        font::draw(&mut self.s, C - font::width("Search") / 2, 32, "Search", GREY);
        self.draw_query();
        let mut x = 40;
        while x < 426 {
            Surface::rect(&mut self.s, x, 168, 2, 2, DIM);
            x += 8;
        }
        let sym = self.sym;
        each_key(sym, |k, _, _, _| self.draw_key(k, false));
    }

    fn draw_query(&mut self) {
        Surface::rect(&mut self.s, 60, 70, 346, 90, 0);
        let q = core::str::from_utf8(&self.query[..self.n_query]).unwrap_or("");
        if q.is_empty() {
            centre(&mut self.s, C, 104, "Song, artist, album", DIM);
            return;
        }
        let w = font::width(q);
        // the end of a long query stays in view
        let mut start = 0;
        while font::width(&q[start..]) > 300 {
            start += 1;
        }
        let shown = &q[start..];
        let w2 = font::width(shown).min(w);
        font::draw(&mut self.s, C - w2 / 2, 104, shown, W);
        Surface::rect(&mut self.s, C + w2 / 2 + 2, 102, 3, 22, RED.rgb565());
    }

    fn draw_key(&mut self, key: K, pressed: bool) {
        let sym = self.sym;
        let s = &mut self.s;
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
            };
            let ink = if pressed { 0 } else if k == K::Space { GREY } else { W };
            centre(s, x + w / 2, y + KH / 2 - 9, label, ink);
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

    fn type_key(&mut self, k: K) {
        match k {
            K::Ch(c) => {
                if self.n_query < QUERY_MAX {
                    self.query[self.n_query] = c;
                    self.n_query += 1;
                }
            }
            K::Space => {
                if self.n_query > 0 && self.n_query < QUERY_MAX && self.query[self.n_query - 1] != b' ' {
                    self.query[self.n_query] = b' ';
                    self.n_query += 1;
                }
            }
            K::Del => self.n_query = self.n_query.saturating_sub(1),
            K::Sym => {
                self.sym = !self.sym;
                return self.draw();
            }
            K::Go => {
                if self.n_query == 0 {
                    return;
                }
                let mut m = [0u8; 1 + QUERY_MAX];
                m[0] = b's';
                m[1..1 + self.n_query].copy_from_slice(&self.query[..self.n_query]);
                self.send(&m[..1 + self.n_query]);
                let l = &mut model().list;
                l.kind = b'S';
                l.n = 0;
                l.want = 1;
                self.scroll = 0;
                self.screen = Screen::List;
                return self.draw();
            }
        }
        self.draw_query();
        self.draw_key(k, false);
        self.api.show();
    }

    // --- events

    /// true: the whole screen needs drawing
    fn event(&mut self, e: Event) -> bool {
        match e.kind {
            CROWN => match self.screen {
                Screen::Now => {
                    self.volume(i32::from(e.delta));
                    false
                }
                Screen::List => {
                    let next = (self.scroll + i32::from(e.delta) * 40).clamp(0, self.list_max());
                    let moved = next != self.scroll;
                    self.scroll = next;
                    moved
                }
                Screen::Search => false,
            },
            TOUCH_DOWN => {
                let (x, y) = (i32::from(e.x), i32::from(e.y));
                self.down = Some((x, y));
                if self.screen == Screen::Search {
                    // typed at once (as WhatsApp's keyboard): the key flashes until the next touch
                    if let Some(old) = self.pressed.take() {
                        self.draw_key(old, false);
                    }
                    if let Some(k) = self.key_at(x, y) {
                        self.motor.play(haptic::TICK);
                        self.type_key(k);
                        if self.screen == Screen::Search && k != K::Sym {
                            self.pressed = Some(k);
                            self.draw_key(k, true);
                            self.api.show();
                        }
                    }
                }
                false
            }
            TOUCH_UP => {
                let Some((x, y)) = self.down.take() else { return false };
                match self.screen {
                    Screen::Now => self.now_tap(x, y),
                    Screen::List => self.list_tap(x, y),
                    Screen::Search => {
                        if let Some(k) = self.pressed.take() {
                            self.draw_key(k, false);
                            self.api.show();
                        }
                    }
                }
                false
            }
            MESSAGE => self.messages(),
            _ => false,
        }
    }

    fn messages(&mut self) -> bool {
        let mut buf = [0u8; MSG_MAX];
        let mut dirty = false;
        while let Some(m) = self.api.recv(&mut buf) {
            let a = art();
            match m.first() {
                Some(b'I') if m.len() == HEADER => {
                    a.header.copy_from_slice(m);
                    a.len = 0;
                    a.ok = false;
                    continue;
                }
                Some(b'D') if m.len() >= 2 => {
                    let t = &m[2..];
                    if a.len + t.len() <= ART_CAP {
                        a.data[a.len..a.len + t.len()].copy_from_slice(t);
                        a.len += t.len();
                    }
                    continue;
                }
                Some(b'E') => {
                    a.ok = true;
                    dirty |= self.screen == Screen::Now;
                    continue;
                }
                Some(b'?') => {
                    self.send(b"R");
                    continue;
                }
                _ => {}
            }
            match model().message(m) {
                Change::Now => {
                    self.tick_at = self.api.now();
                    dirty |= self.screen == Screen::Now;
                }
                Change::List => dirty |= self.screen == Screen::List,
                Change::Note => {
                    let mut t = [0u8; 40];
                    let n = model().note.as_str().len();
                    t[..n].copy_from_slice(model().note.as_str().as_bytes());
                    self.toast(&t[..n]);
                }
                Change::None => {}
            }
        }
        dirty
    }

    /// Once a second while playing: the progress moves on the watch alone.
    fn tick(&mut self) {
        let now = self.api.now();
        if let Some(t) = self.toast_until {
            if now.wrapping_sub(t) < 1 << 31 {
                self.toast_until = None;
                if self.screen == Screen::Now {
                    self.draw_top();
                    self.api.show();
                }
            }
        }
        let m = model();
        if !m.playing() {
            self.tick_at = now;
            return;
        }
        let secs = now.wrapping_sub(self.tick_at) / 1000;
        if secs == 0 {
            return;
        }
        self.tick_at = self.tick_at.wrapping_add(secs * 1000);
        let n = &mut m.now;
        n.pos = (u32::from(n.pos) + secs).min(u32::from(n.dur)) as u16;
        if self.screen == Screen::Now {
            self.draw_ring(false);
            self.api.show();
        }
    }

    /// How long the next wait may be.
    fn wait(&self) -> u32 {
        let now = self.api.now();
        let mut ms = u32::MAX;
        if model().playing() {
            ms = 1000 - now.wrapping_sub(self.tick_at) % 1000;
        }
        if let Some(t) = self.toast_until {
            ms = ms.min(t.wrapping_sub(now).min(2000));
        }
        ms
    }
}

/// A filled triangle pointing left (`dir` -1) or right (1), its tip `r` px from `cx`.
fn triangle(s: &mut impl Surface, cx: i32, cy: i32, r: i32, dir: i32, c: u16) {
    for k in -r..=r {
        let len = r - k.abs();
        let len = len * 2 - len / 4;
        if dir > 0 {
            Surface::rect(s, cx - r + 2, cy + k, len, 1, c);
        } else {
            Surface::rect(s, cx + r - 2 - len, cy + k, len, 1, c);
        }
    }
}

fn heading(s: &mut impl Surface, y: i32, text: &str) {
    let pitch = 3;
    let w = text.chars().count() as i32 * 8 * pitch;
    dots(s, C - w / 2, y, text, pitch, pitch - 1, W);
    disc(s, C + w / 2 + 8, y + 7 * pitch - 3, 4, RED, BLACK);
}

/// One keyboard row: up to 10 keys `(key, width)`, centred.
fn row(r: usize, sym: bool) -> ([(K, i32); 10], usize) {
    let mut keys = [(K::Space, 0); 10];
    let chars = |s: &[u8], w: i32, keys: &mut [(K, i32); 10]| {
        for (i, &c) in s.iter().enumerate() {
            keys[i] = (K::Ch(c), w);
        }
        s.len()
    };
    let n = match (r, sym) {
        (0, false) => chars(b"qwertyuiop", 44, &mut keys),
        (1, false) => chars(b"asdfghjkl", 44, &mut keys),
        (0, true) => chars(b"1234567890", 44, &mut keys),
        (1, true) => chars(b"&-'.,!?()", 44, &mut keys),
        (2, _) => {
            let n = chars(if sym { b"#+/:\"@*" } else { b"zxcvbnm" }, 44, &mut keys);
            keys[n] = (K::Del, 58);
            n + 1
        }
        _ => {
            keys[0] = (K::Sym, 66);
            keys[1] = (K::Space, 134);
            keys[2] = (K::Go, 72);
            3
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

const _: () = assert!(MAX_ROWS <= 255);

fn main(api: &'static Api) -> i32 {
    api.own_back(true);
    let mut ui = Ui {
        api,
        s: Ink(api),
        screen: Screen::Now,
        scroll: 0,
        drawn: 0,
        tick_at: api.now(),
        toast_until: None,
        query: [0; QUERY_MAX],
        n_query: 0,
        sym: false,
        pressed: None,
        down: None,
        motor: Motor::new(api),
    };
    ui.draw();
    ui.send(b"R");
    let mut ask = cmf_wp2_ui::ask::Ask::new(api.now(), 10);
    loop {
        let e = api.event(ask.wait(api.now()).map_or(ui.wait(), |a| a.min(ui.wait())));
        if ask.due(api.now()) {
            ui.send(b"R");
        }
        if e.as_ref().is_some_and(|e| e.kind == MESSAGE) {
            ask.answered();
        }
        let mut dirty = false;
        if let Some(e) = e {
            match e.kind {
                BUTTON => {
                    if ui.screen == Screen::Now {
                        return 0;
                    }
                    ui.screen = Screen::Now;
                    dirty = true;
                }
                PAUSE => {
                    ui.motor.off();
                    loop {
                        match api.event(u32::MAX) {
                            Some(e) if e.kind == RESUME => break,
                            Some(e) if e.kind == BUTTON => return 0,
                            _ => {}
                        }
                    }
                    // the phone sends what changed meanwhile
                    ui.send(b"R");
                    dirty = true;
                }
                _ => dirty = ui.event(e),
            }
            while let Some(e) = api.event(0) {
                if e.kind == BUTTON {
                    if ui.screen == Screen::Now {
                        return 0;
                    }
                    ui.screen = Screen::Now;
                    dirty = true;
                    continue;
                }
                dirty |= ui.event(e);
            }
        }
        if dirty {
            ui.draw();
        }
        ui.tick();
    }
}
