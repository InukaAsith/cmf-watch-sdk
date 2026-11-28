//! Typing: Spotify's keyboard (qwerty, 123, a "dial" key for the rotary one; examples/spotify), the words above it.
//! Tapping the words on the rotary keyboard goes back to the keys.

use cmf_wp2_app::Api;
use cmf_wp2_ui::color::{rgb565, Rgb, BLACK};
use cmf_wp2_ui::dial::{Act, Dial, Key};
use cmf_wp2_ui::dictate::{mic_button, mic_hit, Dictate, Out};
use cmf_wp2_ui::draw::rrect;
use cmf_wp2_ui::font;
use cmf_wp2_ui::repeat::Repeat;
use cmf_wp2_ui::{C, Surface};

pub const MAX: usize = 100;

const ROW_Y: [i32; 4] = [184, 242, 300, 358];
const KH: i32 = 58;
/// the voice typing button, right of the words
const MIC_X: i32 = 404;
const MIC_Y: i32 = 120;
const W: u16 = 0xFFFF;
const GREY: u16 = rgb565(0x80, 0x80, 0x80);
const KEYC: Rgb = Rgb(0x2A, 0x2A, 0x2A);
const CARD: Rgb = Rgb(0x1C, 0x1C, 0x1E);
const WHITE: Rgb = Rgb(0xFF, 0xFF, 0xFF);
const RED: Rgb = Rgb(0xD7, 0x19, 0x21);

#[derive(Clone, Copy, PartialEq)]
pub enum K {
    Ch(u8),
    Del,
    Sym,
    Space,
    Go,
    Dial,
}

/// What a touch did.
pub enum Tap {
    /// drawn: show
    Shown,
    /// a key typed: tick and show
    Typed,
    /// draw the whole screen
    Redraw,
    /// Go: the words are done
    Go,
}

pub struct Keys {
    pub text: [u8; MAX],
    pub n: usize,
    sym: bool,
    lit: Option<K>,
    /// the rotary keyboard is up
    pub rotary: bool,
    pub dial: Dial,
    /// shown while nothing is typed
    pub hint: &'static str,
    /// voice typing (cmf_wp2_ui::dictate): its screen is up while it runs
    pub dict: &'static mut Dictate,
    /// delete held on the keys: it repeats
    pub rep: Repeat,
}

impl Keys {
    pub fn new() -> Self {
        Keys { text: [0; MAX], n: 0, sym: false, lit: None, rotary: false, dial: Dial::new(), hint: "", dict: cmf_wp2_ui::dictate::kept(), rep: Repeat::new() }
    }

    pub fn open(&mut self, hint: &'static str) {
        self.n = 0;
        self.sym = false;
        self.lit = None;
        self.rotary = false;
        self.dial.cancel();
        self.rep.stop();
        self.hint = hint;
    }

    pub fn text(&self) -> &str {
        core::str::from_utf8(&self.text[..self.n]).unwrap_or("")
    }

    pub fn draw(&self, api: &'static Api) {
        let mut s = api;
        if self.dict.active() {
            return self.dict.draw(&mut s);
        }
        self.draw_text(&mut s);
        if self.rotary {
            self.dial.draw(&mut s);
        } else {
            let sym = self.sym;
            each_key(sym, |k, _, _, _| draw_key(&mut s, sym, k, false));
            mic_button(&mut s, MIC_X, MIC_Y);
        }
    }

    /// What the dictation screen asks for, as a touch's answer: the words onto the text.
    pub fn voice(&mut self, o: Out) -> Tap {
        match o {
            Out::None => Tap::Shown,
            Out::Redraw => Tap::Redraw,
            Out::Done | Out::Cancel => {
                if o == Out::Done {
                    let t = self.dict.text().trim();
                    if self.n > 0 && self.text[self.n - 1] != b' ' && self.n < MAX && !t.is_empty() {
                        self.text[self.n] = b' ';
                        self.n += 1;
                    }
                    let mut k = t.len().min(MAX - self.n);
                    while !t.is_char_boundary(k) {
                        k -= 1;
                    }
                    self.text[self.n..self.n + k].copy_from_slice(&t.as_bytes()[..k]);
                    self.n += k;
                }
                Tap::Redraw
            }
        }
    }

    fn draw_text(&self, s: &mut &'static Api) {
        // inside the rotary keyboard's letters when it is up
        let half = if self.rotary { 106 } else { 193 };
        Surface::rect(s, C - half, 100, 2 * half, 40, 0);
        let q = self.text();
        if q.is_empty() {
            let w = font::width(self.hint).min(300);
            font::fitted(s, C - w / 2, 110, self.hint, 300, GREY);
            return;
        }
        // the end of a long text stays in view
        let max = if self.rotary { 200 } else { 250 };
        let mut start = 0;
        while font::width(&q[start..]) > max && start < q.len() {
            start += 1;
            while !q.is_char_boundary(start) {
                start += 1;
            }
        }
        let w = font::width(&q[start..]).min(max);
        font::fitted(s, C - w / 2, 110, &q[start..], max, W);
    }

    /// The rotary keyboard's typed words were touched: back to the keys.
    pub fn on_text(&self, x: i32, y: i32) -> bool {
        self.rotary && (90..150).contains(&y) && (x - C).abs() < 140
    }

    pub fn to_keys(&mut self) {
        self.dial.cancel();
        self.rotary = false;
    }

    /// A touch on the keys (not the rotary one): typed at once, the key lit till the next touch.
    pub fn down(&mut self, api: &'static Api, x: i32, y: i32) -> Tap {
        if self.dict.active() {
            let o = self.dict.tap(api, x, y);
            return self.voice(o);
        }
        if mic_hit(MIC_X, MIC_Y, x, y) {
            self.lit = None;
            self.dict.start(api, false);
            return Tap::Redraw;
        }
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
        let Some(k) = hit else { return Tap::Shown };
        match k {
            K::Ch(c) if self.n < MAX => {
                self.text[self.n] = c;
                self.n += 1;
            }
            K::Space if self.n > 0 && self.n < MAX && self.text[self.n - 1] != b' ' => {
                self.text[self.n] = b' ';
                self.n += 1;
            }
            K::Del => {
                self.n = self.n.saturating_sub(1);
                self.rep.start(api.now());
            }
            K::Sym => {
                self.sym = !self.sym;
                return Tap::Redraw;
            }
            K::Dial => {
                self.dial.shift = self.n == 0;
                self.dial.cancel();
                self.rotary = true;
                return Tap::Redraw;
            }
            K::Go => {
                self.lit = None;
                return if self.n > 0 { Tap::Go } else { Tap::Shown };
            }
            _ => {}
        }
        self.draw_text(&mut s);
        draw_key(&mut s, sym, k, true);
        self.lit = Some(k);
        Tap::Typed
    }

    /// Delete held on the keys, due again: true when it deleted (drawn: tick and show).
    pub fn repeat(&mut self, api: &'static Api) -> bool {
        if self.rotary || !self.rep.due(api.now()) {
            return false;
        }
        if self.n == 0 {
            self.rep.stop();
            return false;
        }
        self.n -= 1;
        self.draw_text(&mut { api });
        true
    }

    /// What the rotary keyboard did: a key into the words (true: Go with something typed).
    pub fn dial_key(&mut self, api: &'static Api, a: Act) -> bool {
        let Act::Key(k) = a else { return false };
        match k {
            Key::Ch(c) if self.n < MAX => {
                self.text[self.n] = c;
                self.n += 1;
            }
            Key::Space if self.n > 0 && self.n < MAX => {
                self.text[self.n] = b' ';
                self.n += 1;
            }
            Key::Del => self.n = self.n.saturating_sub(1),
            Key::Send if self.n > 0 => return true,
            _ => {}
        }
        let mut s = api;
        self.draw_text(&mut s);
        self.dial.draw_centre(&mut s);
        false
    }
}

/// One row: up to 10 keys `(key, width)`, centred (Spotify's layout).
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
            let n = chars(if sym { b"#+/:_@*" } else { b"zxcvbnm" }, &mut keys);
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
