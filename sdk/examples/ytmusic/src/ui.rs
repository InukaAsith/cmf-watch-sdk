//! YouTube Music: now playing with the app's mark, a red progress ring on the rim, controls, and the crown for the
//! volume; Search (QWERTY or the rotary keyboard) and Up next as lists. The phone finds and fetches the songs;
//! the watch plays them on its speaker and counts the progress itself (player.rs), so only sound crosses the link.

use cmf_wp2_ui::dictate::{Dictate, Out as DictOut};
use crate::model::{Change, Model, MAX_ROWS};
use crate::player::{Player, Sink};
use cmf_wp2_app::{Api, Event, BUTTON, CROWN, MESSAGE, MSG_MAX, PAUSE, RESUME, TOUCH_DOWN, TOUCH_UP};
use cmf_wp2_ui::color::{rgb565, Rgb, BLACK};
use cmf_wp2_ui::dial::{Act, Dial, Key};
use cmf_wp2_ui::draw::{arc, disc, ring, rrect};
use cmf_wp2_ui::font::{self, centre, clip, fitted};
use cmf_wp2_ui::geom::chord;
use cmf_wp2_ui::haptic::{self, Motor};
use cmf_wp2_ui::text::dots;
use cmf_wp2_ui::{C, Surface};

cmf_wp2_app::entry!(main);
cmf_wp2_app::app_icon!();

static mut MODEL: Model = Model::new();
static mut PLAYER: Player = Player::new();

fn model() -> &'static mut Model {
    // one app thread
    unsafe { &mut *core::ptr::addr_of_mut!(MODEL) }
}

fn player() -> &'static mut Player {
    unsafe { &mut *core::ptr::addr_of_mut!(PLAYER) }
}

/// The loader's audio track.
struct Track(&'static Api);
impl Sink for Track {
    fn open(&mut self, khz: u8) -> i32 {
        let r = self.0.audio_open(khz);
        if r == 0 {
            // the level stays at full: the player scales the samples (the stock level is all or nothing)
            self.0.audio_volume(100);
        }
        r
    }
    fn write(&mut self, pcm: &[u8]) -> i32 {
        self.0.audio_write(pcm)
    }
    fn queued(&self) -> i32 {
        self.0.audio_queued()
    }
    fn close(&mut self) {
        self.0.audio_close();
    }
}

const WHITE: Rgb = Rgb(0xFF, 0xFF, 0xFF);
const RED: Rgb = Rgb(0xD7, 0x19, 0x21);
/// the phone preparing the song
const YELLOW: Rgb = Rgb(0xF5, 0xC5, 0x18);
const CARD: Rgb = Rgb(0x17, 0x17, 0x17);
const KEYC: Rgb = Rgb(0x26, 0x26, 0x26);
const TRACK: Rgb = Rgb(0x2A, 0x2A, 0x2A);
const W: u16 = 0xFFFF;
const SOFT: u16 = rgb565(0xC8, 0xC8, 0xC8);
const GREY: u16 = rgb565(0x80, 0x80, 0x80);
const DIM: u16 = rgb565(0x3A, 0x3A, 0x3A);

// --- now playing
/// the app's mark where Spotify Control has the album art (no art over the link: it is the sound's bandwidth)
const MARK_Y: i32 = 136;
const RING: (i32, i32) = (224, 229);
const CTRL_Y: i32 = 312;
const CTRL_X: [i32; 3] = [C - 96, C, C + 96];
const TOOL_Y: i32 = 392;
/// Taps this far from the centre (outside the tools) seek on the progress ring.
const SEEK_R: i32 = 196;
/// Search, up next, history, and YouTube Music's mixes / Spotify's library (playlists)
const TOOL_X: [i32; 4] = [C - 114, C - 38, C + 38, C + 114];
// --- lists
const LIST_TOP: i32 = 100;
const ROW_H: i32 = 70;
const PITCH: i32 = 78;
// --- keyboard
const KH: i32 = 58;
const ROW_Y: [i32; 4] = [184, 242, 300, 358];
const QUERY_MAX: usize = 60;
/// the voice typing button, right of the search words
const MIC_X: i32 = 400;
const MIC_Y: i32 = 116;

#[derive(Clone, Copy, PartialEq)]
enum Screen {
    Now,
    List,
    Search,
    /// search on the rotary keyboard (cmf_wp2_ui::dial)
    Dial,
    /// voice typing into the search (cmf_wp2_ui::dictate)
    Voice,
}

#[derive(Clone, Copy, PartialEq)]
enum K {
    Ch(u8),
    Del,
    Sym,
    Space,
    Go,
    /// to the rotary keyboard
    Dial,
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
    track: Track,
    screen: Screen,
    scroll: i32,
    /// the progress ring as last drawn, 0..=65536
    drawn: u32,
    /// the second last drawn, and the top line's state
    shown_pos: u16,
    shown_state: u8,
    /// the buffer's filling circle as last drawn (0..=65536), None: the plain mark
    filled: Option<u32>,
    /// a note or the volume over the top line until then
    toast_until: Option<u32>,
    /// when the phone's last message came (a song that waits on a silent phone is given up)
    phone_at: u32,
    dict: Dictate,
    query: [u8; QUERY_MAX],
    n_query: usize,
    sym: bool,
    pressed: Option<K>,
    down: Option<(i32, i32)>,
    motor: Motor,
    dial: Dial,
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
            Screen::Dial => self.draw_dial(),
            Screen::Voice => self.dict.draw(&mut self.s),
        }
        self.api.show();
    }

    // --- now playing

    fn draw_now(&mut self) {
        let p = player();
        self.draw_ring(true);
        self.draw_top();
        mark(&mut self.s, C, MARK_Y);
        self.filled = None;
        self.draw_fill();
        let (title, artist) = match &p.now {
            Some(s) => (s.title.as_str(), s.artist.as_str()),
            None if p.loading() => ("", ""),
            None => ("Nothing playing", "Search for a song"),
        };
        let (t, cut) = clip(title, 290);
        if cut {
            fitted(&mut self.s, C - 145, 222, title, 290, W);
        } else {
            centre(&mut self.s, C, 222, t, W);
        }
        let (ar, _) = clip(artist, 260);
        centre(&mut self.s, C, 249, ar, GREY);
        self.draw_controls();
        self.draw_tools();
    }

    /// 0 idle, 1 loading, 2 playing, 3 paused, 4 no speaker
    fn state(&self) -> u8 {
        let p = player();
        if p.error < 0 {
            4
        } else if p.preparing().is_some() {
            6
        } else if p.loading() {
            1
        } else if p.refilling() {
            5
        } else if p.paused && p.now.is_some() {
            3
        } else if p.now.is_some() {
            2
        } else {
            0
        }
    }

    /// The app's name, the state, or the toast at the top.
    fn draw_top(&mut self) {
        // inside the ring: at y 30 the ring's inner edge is outside x 140..326
        Surface::rect(&mut self.s, 140, 28, 186, 28, 0);
        self.shown_state = self.state();
        if self.toast_until.is_some() {
            let note = model().note.as_str();
            let w = font::width(note).min(150) + 28;
            rrect(&mut self.s, C - w / 2, 29, w, 26, 13, KEYC, BLACK);
            fitted(&mut self.s, C - w / 2 + 14, 31, note, 150, W);
            return;
        }
        let top = match self.shown_state {
            1 => "Buffering",
            6 => "Preparing",
            3 => "Paused",
            4 => "No speaker",
            5 => "Buffering",
            _ => crate::brand::NAME,
        };
        let w = font::width(top) + 16;
        disc(&mut self.s, C - w / 2 + 3, 42, 4, RED, BLACK);
        font::draw(&mut self.s, C - w / 2 + 14, 31, top, GREY);
    }

    /// The progress ring; `full`: the track too, else only the part that moved since the last draw.
    fn draw_ring(&mut self, full: bool) {
        let p = player();
        let dur = p.now.map_or(0, |s| s.dur);
        let pos = p.pos();
        self.shown_pos = pos;
        let want = if dur == 0 { 0 } else { (u32::from(pos.min(dur)) * 65536 / u32::from(dur)).min(65536) };
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
        let p = player();
        let playing = p.now.is_some() && !p.paused || p.loading();
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
        for (i, &x) in TOOL_X.iter().enumerate() {
            ring(s, x, TOOL_Y, 21, 23, Rgb(0x50, 0x50, 0x50), BLACK);
            let y = TOOL_Y;
            if i == 0 {
                // search: lens + handle
                ring(s, x - 3, y - 3, 6, 9, WHITE, BLACK);
                for k in 0..6 {
                    Surface::rect(s, x + 3 + k, y + 3 + k, 3, 3, W);
                }
            } else if i == 3 && crate::brand::SPOTIFY {
                // library: two books and a leaning one
                Surface::rect(s, x - 10, y - 10, 4, 20, W);
                Surface::rect(s, x - 3, y - 10, 4, 20, W);
                for k in 0..20 {
                    Surface::rect(s, x + 4 + k / 4, y - 10 + k, 4, 1, RED.rgb565());
                }
            } else if i == 3 {
                // mixes: a play triangle in a ring of two
                ring(s, x, y, 9, 11, WHITE, BLACK);
                triangle(s, x + 1, y, 5, 1, RED.rgb565());
            } else if i == 2 {
                // history: a clock
                ring(s, x, y, 9, 11, WHITE, BLACK);
                Surface::rect(s, x - 1, y - 7, 3, 8, W);
                Surface::rect(s, x - 1, y - 1, 7, 3, RED.rgb565());
            } else {
                // up next: three lines, the first red (playing)
                for k in 0..3 {
                    Surface::rect(s, x - 9, y - 8 + k * 7, 18, 3, if k == 0 { RED.rgb565() } else { W });
                }
            }
        }
    }

    /// Asks the phone for another stream: what is queued here stops at once.
    fn jump(&mut self, m: &[u8]) {
        let g = player().want();
        self.phone_at = self.api.now();
        let mut b = [0u8; 6];
        b[0] = m[0];
        b[1] = g;
        b[2..1 + m.len()].copy_from_slice(&m[1..]);
        self.send(&b[..1 + m.len()]);
    }

    fn now_tap(&mut self, x: i32, y: i32) {
        let near = |cx: i32, cy: i32, r: i32| (x - cx) * (x - cx) + (y - cy) * (y - cy) <= r * r;
        let p = player();
        if near(CTRL_X[1], CTRL_Y, 44) {
            self.motor.play(haptic::TICK);
            if p.now.is_none() && !p.loading() {
                return self.toast(b"Search for a song");
            }
            p.paused = !p.paused;
            self.draw_controls();
            self.draw_top();
            self.api.show();
            return;
        }
        if near(CTRL_X[0], CTRL_Y, 38) || near(CTRL_X[2], CTRL_Y, 38) {
            self.motor.play(haptic::TICK);
            self.jump(if x < C { b"b" } else { b"n" });
            self.draw();
            return;
        }
        for (i, &tx) in TOOL_X.iter().enumerate() {
            if near(tx, TOOL_Y, 30) {
                self.motor.play(haptic::TICK);
                if i == 0 {
                    self.screen = Screen::Search;
                    self.n_query = 0;
                    self.sym = false;
                    self.draw();
                } else {
                    self.open_list([b'Q', b'Q', b'H', b'P'][i]);
                }
                return;
            }
        }
        // a tap at the rim: seek to that point of the progress ring (clockwise from the top)
        let (dx, dy) = (x - C, y - C);
        if dx * dx + dy * dy >= SEEK_R * SEEK_R {
            self.seek(dx, dy);
        }
    }

    fn seek(&mut self, dx: i32, dy: i32) {
        let Some(s) = player().now else { return };
        if s.dur == 0 {
            return;
        }
        self.motor.play(haptic::TICK);
        let frac = u32::from(cmf_wp2_ui::motion::angle(dx, dy).wrapping_sub(49152));
        let pos = (frac * u32::from(s.dur) / 65536) as u16;
        let p = pos.to_le_bytes();
        self.jump(&[b'j', p[0], p[1]]);
        player().show_at(pos);
        // the time where the name is, for a moment
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

    /// The crown: the player's own gain, heard at once (nothing over the link).
    fn volume(&mut self, delta: i32) {
        let p = player();
        p.volume = (i32::from(p.volume) + delta * 5).clamp(0, 100) as u8;
        let mut b = [0u8; 11];
        let v = cmf_wp2_ui::text::int(i32::from(p.volume), &mut b);
        let mut t = [0u8; 16];
        t[..7].copy_from_slice(b"Volume ");
        t[7..7 + v.len()].copy_from_slice(v.as_bytes());
        let len = 7 + v.len();
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
            b'Q' => "UP NEXT",
            b'H' => "HISTORY",
            b'P' if crate::brand::SPOTIFY => "LIBRARY",
            b'P' => "MIXES",
            _ => "RESULTS",
        };
        heading(&mut self.s, 36, title);
    }

    fn list_max(&self) -> i32 {
        (model().list.n as i32 * PITCH - (466 - LIST_TOP - 60)).max(0)
    }

    fn list_tap(&mut self, y: i32) {
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
        self.jump(&[b'p', kind, i as u8]);
        self.screen = Screen::Now;
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
        cmf_wp2_ui::dictate::mic_button(&mut self.s, MIC_X, MIC_Y);
    }

    /// What the dictation screen asks for: the words into the search. True: draw again.
    fn voice_out(&mut self, o: DictOut) -> bool {
        match o {
            DictOut::None => false,
            DictOut::Redraw => true,
            DictOut::Done | DictOut::Cancel => {
                if o == DictOut::Done {
                    let t = self.dict.text().trim().trim_end_matches('.');
                    if self.n_query > 0 && self.query[self.n_query - 1] != b' ' && self.n_query < QUERY_MAX && !t.is_empty() {
                        self.query[self.n_query] = b' ';
                        self.n_query += 1;
                    }
                    let mut k = t.len().min(QUERY_MAX - self.n_query);
                    while !t.is_char_boundary(k) {
                        k -= 1;
                    }
                    self.query[self.n_query..self.n_query + k].copy_from_slice(&t.as_bytes()[..k]);
                    self.n_query += k;
                }
                self.screen = Screen::Search;
                true
            }
        }
    }

    /// The button: voice typing stops, else back to now playing; false: close.
    fn button(&mut self) -> bool {
        if self.screen == Screen::Voice {
            let o = self.dict.cancel(self.api);
            self.voice_out(o);
            return true;
        }
        if self.screen == Screen::Now {
            return false;
        }
        self.screen = Screen::Now;
        true
    }

    fn draw_dial(&mut self) {
        self.draw_query();
        let mut x = 60;
        while x < 406 {
            Surface::rect(&mut self.s, x, 168, 2, 2, DIM);
            x += 8;
        }
        self.dial.draw(&mut self.s);
    }

    /// What the rotary keyboard said.
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
                self.type_key(match k {
                    Key::Ch(c) => K::Ch(c),
                    Key::Space => K::Space,
                    Key::Del => K::Del,
                    Key::Send => K::Go,
                });
                if self.screen == Screen::Dial {
                    self.draw_query();
                    self.dial.draw_centre(&mut self.s);
                    self.api.show();
                }
            }
        }
    }

    fn draw_query(&mut self) {
        // inside the dial's letters when it is up
        let (half, max) = if self.screen == Screen::Dial { (100, 180) } else { (140, 250) };
        Surface::rect(&mut self.s, C - half, 80, 2 * half, 80, 0);
        let q = core::str::from_utf8(&self.query[..self.n_query]).unwrap_or("");
        if q.is_empty() {
            centre(&mut self.s, C, 104, "Song, artist, album", DIM);
            return;
        }
        let w = font::width(q);
        // the end of a long query stays in view
        let mut start = 0;
        while font::width(&q[start..]) > max {
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
                K::Dial => "dial",
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
            K::Dial => {
                self.screen = Screen::Dial;
                self.dial.shift = false;
                self.dial.cancel();
                return self.draw();
            }
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
        if self.screen == Screen::Dial {
            // the dial draws its own (dial_act)
            return;
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
                Screen::Search | Screen::Voice => false,
                Screen::Dial => {
                    let a = self.dial.crown(&mut self.s, i32::from(e.delta), self.api.now());
                    self.dial_act(a);
                    false
                }
            },
            TOUCH_DOWN => {
                let (x, y) = (i32::from(e.x), i32::from(e.y));
                self.down = Some((x, y));
                if self.screen == Screen::Dial {
                    if (70..160).contains(&y) && (x - C).abs() < 150 {
                        // the typed text: back to the keys
                        self.dial.cancel();
                        self.screen = Screen::Search;
                        return true;
                    }
                    let a = self.dial.down(&mut self.s, x, y, self.api.now());
                    self.dial_act(a);
                    return false;
                }
                if self.screen == Screen::Voice {
                    let o = self.dict.tap(self.api, x, y);
                    return self.voice_out(o);
                }
                if self.screen == Screen::Search && cmf_wp2_ui::dictate::mic_hit(MIC_X, MIC_Y, x, y) {
                    self.motor.play(haptic::TICK);
                    self.pressed = None;
                    self.dict.start(self.api, false);
                    self.screen = Screen::Voice;
                    return true;
                }
                if self.screen == Screen::Search {
                    // typed at once: the key flashes until the next touch
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
            TOUCH_UP if self.screen == Screen::Dial => {
                self.down = None;
                let a = self.dial.up(&mut self.s, i32::from(e.x), i32::from(e.y));
                self.dial_act(a);
                false
            }
            TOUCH_UP => {
                let Some((x, y)) = self.down.take() else { return false };
                match self.screen {
                    Screen::Now => self.now_tap(x, y),
                    Screen::List => self.list_tap(y),
                    Screen::Dial | Screen::Voice => {}
                    Screen::Search => {
                        if let Some(k) = self.pressed.take() {
                            self.draw_key(k, false);
                            self.api.show();
                        }
                    }
                }
                false
            }
            _ => false,
        }
    }

    /// The phone's messages, while the player has room for a block (else they wait in the inbox and the phone's
    /// sends with them: the stream runs at the speaker's pace). true: the whole screen needs drawing.
    fn messages(&mut self) -> bool {
        let mut buf = [0u8; MSG_MAX];
        let mut dirty = false;
        let p = player();
        while p.has_room() {
            let Some(m) = self.api.recv(&mut buf) else { break };
            self.phone_at = self.api.now();
            if let Some(o) = self.dict.message(m) {
                dirty |= self.voice_out(o);
                continue;
            }
            if m == b"?" {
                self.send(b"R");
                continue;
            }
            let live = p.live;
            if p.message(m, &mut self.track) {
                dirty |= self.screen == Screen::Now && (live != p.live || m.first() == Some(&b'c'));
                continue;
            }
            match model().message(m) {
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

    /// The motor, toasts, the dial's hold, and the progress once a second.
    fn tick(&mut self) {
        self.motor.update();
        let now = self.api.now();
        // preparing or buffering with nothing from the phone for 15 s: give up, so the watch stays usable
        let p = player();
        if p.loading() && now.wrapping_sub(self.phone_at) > 15_000 {
            p.give_up();
            self.toast(b"Phone not answering");
            if self.screen == Screen::Now {
                self.draw();
            }
        }
        if self.screen == Screen::Dial && self.dial.tick(&mut self.s, now) == Act::Show {
            self.motor.play(haptic::TICK);
            self.api.show();
        }
        if let Some(t) = self.toast_until {
            if now.wrapping_sub(t) < 1 << 31 {
                self.toast_until = None;
                if self.screen == Screen::Now {
                    self.draw_top();
                    self.api.show();
                }
            }
        }
        if self.screen != Screen::Now {
            return;
        }
        if self.state() != self.shown_state {
            self.draw_top();
            self.draw_controls();
            self.api.show();
        }
        if player().pos() != self.shown_pos {
            self.draw_ring(false);
            self.api.show();
        }
        if self.draw_fill() {
            self.api.show();
        }
    }

    /// The mark's ring fills clockwise from the top: yellow while the phone prepares the song, red as the buffer
    /// fills before it plays (and after the link ran dry); back to white once it plays. true when drawn.
    fn draw_fill(&mut self) -> bool {
        let p = player();
        // redrawn in 1/64 steps; bit 0 tells yellow from red
        let want = match (p.preparing(), p.fill()) {
            (Some(f), _) => Some(f & !1023 | 1),
            (None, Some(f)) => Some(f & !1023),
            _ => None,
        };
        if want == self.filled {
            return false;
        }
        let top = 49152u16;
        ring(&mut self.s, C, MARK_Y, 58, 67, WHITE, BLACK);
        if let Some(w) = want {
            // one arc from the top each time: pieces drawn one after another leave seams at their edges
            let f = w & !1;
            if f > 0 {
                arc(&mut self.s, C, MARK_Y, 58, 67, top, f, if w & 1 == 1 { YELLOW } else { RED }, None, BLACK);
            }
        }
        self.filled = want;
        true
    }

    /// How long the next wait may be.
    fn wait(&self) -> u32 {
        let now = self.api.now();
        let p = player();
        // sound waiting for the loader: 5 ms; a stream running or coming: 20 ms (credit, the buffer filling)
        let mut ms = if p.busy() { 5 } else if p.live || p.loading() { 20 } else { u32::MAX };
        ms = ms.min(self.motor.wait().unwrap_or(u32::MAX));
        if let Some(t) = self.toast_until {
            ms = ms.min(t.wrapping_sub(now).min(2000));
        }
        if self.screen == Screen::Dial {
            if let Some(h) = self.dial.hold_wait(now) {
                ms = ms.min(h);
            }
        }
        ms
    }
}

/// The app's mark, as its icon: a white ring and a red play triangle (YouTube Music), or three red waves
/// (Spotify), inside it.
fn mark(s: &mut impl Surface, cx: i32, cy: i32) {
    ring(s, cx, cy, 58, 67, WHITE, BLACK);
    if crate::brand::SPOTIFY {
        // arcs of circles centred under the mark, each 60° about the top (49152 in arc's 65536 a turn)
        for (r, w, half) in [(84, 9, 5600u32), (63, 8, 5300), (43, 7, 4900)] {
            arc(s, cx, cy + 62, r - w, r, (49152 - half) as u16, 2 * half, RED, None, BLACK);
        }
    } else {
        triangle(s, cx + 6, cy, 26, 1, RED.rgb565());
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

const _: () = assert!(MAX_ROWS <= 255);

fn main(api: &'static Api) -> i32 {
    api.own_back(true);
    let mut ui = Ui {
        api,
        s: Ink(api),
        track: Track(api),
        screen: Screen::Now,
        scroll: 0,
        drawn: 0,
        shown_pos: 0,
        shown_state: 0,
        filled: None,
        toast_until: None,
        phone_at: 0,
        query: [0; QUERY_MAX],
        n_query: 0,
        sym: false,
        pressed: None,
        down: None,
        motor: Motor::new(api),
        dial: Dial::new(),
        dict: Dictate::new(),
    };
    ui.draw();
    ui.send(b"R");
    let quit = |ui: &mut Ui| {
        player().close(&mut ui.track);
        ui.send(b"q");
    };
    loop {
        let p = player();
        let blocks = p.blocks;
        let mut dirty = p.feed(&mut ui.track) && ui.screen == Screen::Now;
        dirty |= ui.messages();
        dirty |= p.feed(&mut ui.track) && ui.screen == Screen::Now;
        let now = api.now();
        if let Some(f) = p.credit(now, p.blocks != blocks) {
            if api.send(&f) == 0 {
                p.told(now);
            }

        }
        if ui.dict.active() {
            let o = ui.dict.tick(api);
            dirty |= ui.voice_out(o);
        }
        // something to draw already (a message, voice typing): no waiting before it
        if let Some(e) = api.event(if dirty { 0 } else { ui.dict.wait_ms(ui.wait()) }) {
            match e.kind {
                BUTTON => {
                    if !ui.button() {
                        quit(&mut ui);
                        return 0;
                    }
                    dirty = true;
                }
                // the loader dropped its queue for the popup: hold the ring until it closes
                PAUSE => {
                    ui.motor.off();
                    if ui.dict.active() {
                        let o = ui.dict.cancel(api);
                        ui.voice_out(o);
                    }
                    player().held = true;
                }
                RESUME => {
                    player().held = false;
                    dirty = true;
                }
                MESSAGE => {}
                _ => dirty |= ui.event(e),
            }
        }
        if dirty {
            ui.draw();
        }
        ui.tick();
    }
}
