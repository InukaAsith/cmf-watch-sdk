//! A QWERTY keyboard laid out for the round panel (Wear OS style), the watch's emoji as a second page, and the text
//! being typed. With [`crate::dial`] (the rotary keyboard) and [`crate::dictate`] (voice typing) it gives any app
//! text input; `sdk/examples/keyboards` shows the three together.
//!
//! The app owns the loop: a TOUCH_DOWN goes to [`Keyboard::tap`], a crown turn to [`Keyboard::crown`] (the emoji
//! pages), and the [`Act`] says what to draw. A pressed key shows white for [`FLASH_MS`]: draw it with
//! [`Keyboard::draw_key`] `pressed`, then plain again when the time is up. The text is at most [`TEXT_MAX`] bytes.
//!
//! Emoji: call [`crate::emoji::init`] once; until then (or on a loader before 6) the keyboard has no emoji key.

use crate::color::{rgb565, Rgb, BLACK};
use crate::draw::{disc, ring, rrect};
use crate::font::{self, centre, clip, wrap};
use crate::geom::chord;
use crate::text::fit;
use crate::{dictate, emoji, Surface, C};

pub const TEXT_MAX: usize = 100;
/// A pressed key shows white this long.
pub const FLASH_MS: u32 = 110;
/// Key height, and the top of each of the four rows.
pub const KH: i32 = 58;
pub const ROW_Y: [i32; 4] = [184, 242, 300, 358];
/// The mic button beside the text: voice typing.
pub const MIC_X: i32 = 404;
pub const MIC_Y: i32 = 140;
/// The emoji page: COLS × ROWS cells of CELL px from TOP, the keys row below.
const EM_COLS: usize = 6;
const EM_ROWS: usize = 3;
const EM_PAGE: usize = EM_COLS * EM_ROWS;
const EM_CELL: i32 = 58;
const EM_TOP: i32 = 166;
const LINE: i32 = 25;

const CARD: Rgb = Rgb(0x17, 0x17, 0x17);
const KEYC: Rgb = Rgb(0x26, 0x26, 0x26);
const WHITE: Rgb = Rgb(0xFF, 0xFF, 0xFF);
const W: u16 = 0xFFFF;
const SOFT: u16 = rgb565(0xC8, 0xC8, 0xC8);
const GREY: u16 = rgb565(0x80, 0x80, 0x80);
const DIM: u16 = rgb565(0x3A, 0x3A, 0x3A);
/// The face on the emoji key.
const FACE: u32 = 0x1F603;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Key {
    Ch(u8),
    Shift,
    Del,
    Sym,
    Space,
    Send,
    /// to the emoji page and back
    Emoji,
    /// to the rotary keyboard
    Dial,
}

/// What a tap asks the app to do.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Act {
    None,
    /// draw the whole keyboard ([`Keyboard::draw`])
    Redraw,
    /// the text changed: [`Keyboard::draw_text`], then `key` pressed for [`FLASH_MS`]
    Typed(Key),
    /// an emoji added from the page: [`Keyboard::draw_text`]
    Text,
    /// done: [`Keyboard::text`] is the result
    Send,
    /// the rotary keyboard ([`crate::dial`]) instead
    Dial,
    /// voice typing ([`crate::dictate`]); its words go in with [`Keyboard::add_words`]
    Voice,
}

/// One keyboard row: up to 10 keys `(key, width)`, centred.
struct Row {
    keys: [(Key, i32); 10],
    n: usize,
}

fn chars_row(s: &[u8], w: i32) -> Row {
    let mut keys = [(Key::Space, 0); 10];
    for (i, &c) in s.iter().enumerate() {
        keys[i] = (Key::Ch(c), w);
    }
    Row { keys, n: s.len() }
}

fn row(r: usize, sym: bool, emoji: bool, dial: bool) -> Row {
    match (r, sym) {
        (0, false) => chars_row(b"qwertyuiop", 44),
        (1, false) => chars_row(b"asdfghjkl", 44),
        (0, true) => chars_row(b"1234567890", 44),
        (1, true) => chars_row(b"@#&-+()/'", 44),
        (2, _) => {
            let mut row = chars_row(if sym { b"\"*:;!?," } else { b"zxcvbnm" }, 42);
            row.keys.copy_within(0..7, 1);
            row.keys[0] = (if sym { Key::Ch(b'.') } else { Key::Shift }, 46);
            row.keys[8] = (Key::Del, 46);
            row.n = 9;
            row
        }
        _ => {
            // the bottom row: the switches the app has; space takes what is left of 286 px (the chord at the row's foot is ~288)
            let mut row = chars_row(b"", 0);
            let mut keys = [(Key::Sym, 54), (Key::Emoji, 46), (Key::Dial, 46), (Key::Space, 0), (Key::Send, 58)];
            let shown = |k: Key| match k {
                Key::Emoji => emoji,
                Key::Dial => dial,
                _ => true,
            };
            let used: i32 = keys.iter().filter(|k| shown(k.0)).map(|k| k.1).sum();
            keys[3].1 = 286 - used;
            for k in keys.into_iter().filter(|k| shown(k.0)) {
                row.keys[row.n] = k;
                row.n += 1;
            }
            row
        }
    }
}

/// The emoji page's bottom row.
fn each_emoji_key(mut f: impl FnMut(Key, i32, i32, i32)) {
    let keys = [(Key::Emoji, 62), (Key::Del, 52), (Key::Space, 90), (Key::Send, 72)];
    let mut x = C - keys.iter().map(|k| k.1).sum::<i32>() / 2;
    for (k, w) in keys {
        f(k, x, ROW_Y[3], w);
        x += w;
    }
}

fn rule(s: &mut impl Surface, y: i32) {
    let (x0, x1) = chord(y, y + 2, 40);
    let mut x = x0;
    while x < x1 {
        s.rect(x, y, 2, 2, DIM);
        x += 8;
    }
}

/// Draws through a surface with body text in Inter ([`crate::font`]).
struct Inter<'a, S: Surface>(&'a mut S);

impl<S: Surface> Surface for Inter<'_, S> {
    fn rect(&mut self, x: i32, y: i32, w: i32, h: i32, c: u16) {
        self.0.rect(x, y, w, h, c);
    }

    fn text(&mut self, x: i32, y: i32, s: &str, c: u16) {
        font::draw(self.0, x, y, s, c);
    }
}

pub struct Keyboard {
    /// numbers and symbols instead of letters
    pub sym: bool,
    /// the next letter a capital (at the start, and after the end of a sentence)
    pub shift: bool,
    /// the emoji page shown, and which of its pages
    pub emoji_page: Option<usize>,
    /// a key for the rotary keyboard ([`Act::Dial`])
    pub dial: bool,
    /// Send's colour and the caret's
    pub accent: Rgb,
    /// shown while nothing is typed
    pub hint: &'static str,
    text: [u8; TEXT_MAX],
    n: usize,
}

impl Keyboard {
    pub const fn new(accent: Rgb) -> Self {
        Keyboard { sym: false, shift: true, emoji_page: None, dial: true, accent, hint: "Message", text: [0; TEXT_MAX], n: 0 }
    }

    pub fn text(&self) -> &str {
        fit(&self.text[..self.n], TEXT_MAX)
    }

    pub fn clear(&mut self) {
        self.n = 0;
        self.shift = true;
    }

    /// Words from voice typing (or anywhere) after what is typed, a space between; cut at a char to fit.
    pub fn add_words(&mut self, words: &str) {
        let words = words.trim();
        if words.is_empty() {
            return;
        }
        if self.n > 0 && self.text[self.n - 1] != b' ' && self.n < TEXT_MAX {
            self.text[self.n] = b' ';
            self.n += 1;
        }
        let fits = fit(words.as_bytes(), TEXT_MAX - self.n);
        self.text[self.n..self.n + fits.len()].copy_from_slice(fits.as_bytes());
        self.n += fits.len();
        self.shift = false;
    }

    /// A key that types (the rotary keyboard's too): a char, space or delete.
    pub fn type_key(&mut self, k: Key) {
        match k {
            Key::Del => {
                // a whole char, emoji included
                self.n = self.text().char_indices().last().map_or(0, |(i, _)| i);
                self.shift = self.n == 0;
            }
            Key::Space | Key::Ch(_) if self.n < TEXT_MAX => {
                let c = match k {
                    Key::Ch(c) if self.shift => c.to_ascii_uppercase(),
                    Key::Ch(c) => c,
                    _ => b' ',
                };
                self.text[self.n] = c;
                self.n += 1;
                // capital after the end of a sentence
                self.shift = c == b' ' && self.n >= 2 && matches!(self.text[self.n - 2], b'.' | b'?' | b'!');
            }
            _ => {}
        }
    }

    fn has_emoji(&self) -> bool {
        !emoji::all().is_empty()
    }

    fn em_pages(&self) -> usize {
        emoji::all().len().div_ceil(EM_PAGE).max(1)
    }

    /// Every key with its rect `(key, x, y, w)`, for drawing and hit tests.
    pub fn each_key(&self, mut f: impl FnMut(Key, i32, i32, i32)) {
        if self.emoji_page.is_some() {
            return each_emoji_key(f);
        }
        for (r, &y) in ROW_Y.iter().enumerate() {
            let row = row(r, self.sym, self.has_emoji(), self.dial);
            let total: i32 = row.keys[..row.n].iter().map(|k| k.1).sum();
            let mut x = C - total / 2;
            for &(k, w) in &row.keys[..row.n] {
                f(k, x, y, w);
                x += w;
            }
        }
    }

    /// The whole screen: `title` on top (empty: none), the text, the keys or the emoji page, the mic.
    pub fn draw(&self, s: &mut impl Surface, title: &str) {
        let mut s = Inter(s);
        s.rect(0, 0, crate::SIZE, crate::SIZE, 0);
        if !title.is_empty() {
            let (t, _) = clip(title, 240);
            centre(&mut s, C, 32, t, SOFT);
        }
        match self.emoji_page {
            None => {
                self.draw_text(s.0);
                rule(&mut s, 172);
            }
            Some(page) => {
                self.draw_text(s.0);
                let all = emoji::all();
                let from = page * EM_PAGE;
                let x0 = C - EM_CELL * EM_COLS as i32 / 2;
                let big = emoji::SIZE * 2;
                for (i, &cp) in all.iter().enumerate().skip(from).take(EM_PAGE) {
                    let (col, row) = ((i - from) % EM_COLS, (i - from) / EM_COLS);
                    let x = x0 + col as i32 * EM_CELL + (EM_CELL - big) / 2;
                    let y = EM_TOP + row as i32 * EM_CELL + (EM_CELL - big) / 2;
                    emoji::draw_scaled(cp, x, y, 2, &mut |x, y, w, h, c| s.rect(x, y, w, h, c));
                }
                // which page: dots, the crown turns them
                let n = self.em_pages() as i32;
                for p in 0..n {
                    let on = p as usize == page;
                    s.rect(C - n * 8 + p * 16 + 4, EM_TOP - 12, 6, 6, if on { W } else { GREY });
                }
            }
        }
        self.each_key(|k, _, _, _| self.draw_key(s.0, k, false));
        dictate::mic_button(&mut s, MIC_X, MIC_Y);
    }

    /// Only the text (after a key typed): its box cleared and drawn again.
    pub fn draw_text(&self, s: &mut impl Surface) {
        let rows = if self.emoji_page.is_some() { 2 } else { 3 };
        s.rect(0, 64, 380, if rows == 2 { 84 } else { 108 }, 0);
        let mut s = Inter(s);
        let (y0, px) = (76, 290);
        let typed = self.text();
        let caret = self.accent.rgb565();
        if typed.is_empty() {
            centre(&mut s, C, y0, self.hint, DIM);
            s.rect(C - font::width(self.hint) / 2 - 8, y0 - 2, 3, 20, caret);
            return;
        }
        let mut lines = [""; 8];
        let n = wrap(typed, px, &mut lines);
        // the last `rows` lines, the newest at the bottom
        let first = n.saturating_sub(rows);
        let (mut last_w, mut last_y) = (0, y0);
        for (i, l) in lines[first..n].iter().enumerate() {
            let (l, _) = clip(l, px);
            let w = font::width(l);
            let y = y0 + i as i32 * LINE;
            s.text(C - w / 2, y, l, W);
            (last_w, last_y) = (w, y);
        }
        let trailing = typed.ends_with(' ') as i32 * font::advance(' ');
        s.rect(C + last_w / 2 + trailing + 2, last_y - 2, 3, 20, caret);
    }

    /// One key, plain or `pressed` (white).
    pub fn draw_key(&self, s: &mut impl Surface, key: Key, pressed: bool) {
        let mut s = Inter(s);
        let (sym, shift) = (self.sym, self.shift);
        self.each_key(|k, x, y, w| {
            if k != key {
                return;
            }
            let fill = match (pressed, k) {
                (true, _) => WHITE,
                (_, Key::Send) => self.accent,
                (_, Key::Shift) if shift => WHITE,
                (_, Key::Ch(_)) => KEYC,
                _ => CARD,
            };
            s.rect(x, y, w, KH, 0);
            rrect(&mut s, x + 3, y + 3, w - 6, KH - 6, 14, fill, BLACK);
            let ink = if pressed || (k == Key::Shift && shift) { 0 } else { W };
            let (cx, cy) = (x + w / 2, y + KH / 2);
            let mut b = [0u8; 4];
            let label: &str = match k {
                Key::Ch(c) => {
                    let c = if shift && !sym { c.to_ascii_uppercase() } else { c };
                    (c as char).encode_utf8(&mut b)
                }
                Key::Shift => "^",
                Key::Del => "<",
                Key::Sym if sym => "abc",
                Key::Sym => "?12",
                Key::Space => "space",
                Key::Send => ">",
                Key::Emoji if self.emoji_page.is_some() => "abc",
                Key::Dial => {
                    // the rotary keyboard: a ring with its marker
                    ring(&mut s, cx, cy, 9, 11, if pressed { BLACK } else { WHITE }, fill);
                    disc(&mut s, cx, cy - 14, 3, self.accent, fill);
                    return;
                }
                Key::Emoji => {
                    // a smiling face, from the watch's own emoji
                    let e = emoji::SIZE / 2;
                    if !emoji::draw_scaled(FACE, cx - e, cy - e, 1, &mut |x, y, w, h, c| s.rect(x, y, w, h, c)) {
                        centre(&mut s, cx, cy - 8, ":)", ink);
                    }
                    return;
                }
            };
            let ink = match k {
                Key::Space if !pressed => GREY,
                Key::Send if !pressed => 0,
                _ => ink,
            };
            centre(&mut s, cx, cy - 8, label, ink);
        });
    }

    /// A TOUCH_DOWN at (x, y).
    pub fn tap(&mut self, x: i32, y: i32) -> Act {
        if dictate::mic_hit(MIC_X, MIC_Y, x, y) {
            return Act::Voice;
        }
        if let Some(page) = self.emoji_page.filter(|_| y < ROW_Y[3]) {
            return self.emoji_tap(page, x, y);
        }
        let mut hit = None;
        self.each_key(|k, kx, ky, w| {
            if (kx..kx + w).contains(&x) && (ky..ky + KH).contains(&y) {
                hit = Some(k);
            }
        });
        let Some(k) = hit else { return Act::None };
        match k {
            Key::Send => Act::Send,
            Key::Dial => Act::Dial,
            Key::Emoji => {
                self.emoji_page = if self.emoji_page.is_some() { None } else { Some(0) };
                Act::Redraw
            }
            Key::Sym => {
                self.sym = !self.sym;
                Act::Redraw
            }
            Key::Shift => {
                self.shift = !self.shift;
                Act::Redraw
            }
            _ => {
                let was = self.shift;
                self.type_key(k);
                // the letters change case: all of them drawn again
                if was != self.shift && self.emoji_page.is_none() { Act::Redraw } else { Act::Typed(k) }
            }
        }
    }

    fn emoji_tap(&mut self, page: usize, x: i32, y: i32) -> Act {
        let x0 = C - EM_CELL * EM_COLS as i32 / 2;
        let (col, row) = ((x - x0).div_euclid(EM_CELL), (y - EM_TOP).div_euclid(EM_CELL));
        if !(0..EM_COLS as i32).contains(&col) || !(0..EM_ROWS as i32).contains(&row) {
            return Act::None;
        }
        let i = page * EM_PAGE + row as usize * EM_COLS + col as usize;
        let Some(ch) = emoji::all().get(i).and_then(|&cp| char::from_u32(cp)) else { return Act::None };
        let mut b = [0u8; 4];
        let b = ch.encode_utf8(&mut b).as_bytes();
        if self.n + b.len() > TEXT_MAX {
            return Act::None;
        }
        self.text[self.n..self.n + b.len()].copy_from_slice(b);
        self.n += b.len();
        self.shift = false;
        Act::Text
    }

    /// A crown turn: the emoji pages. `Redraw` when the page changed.
    pub fn crown(&mut self, d: i32) -> Act {
        let Some(page) = self.emoji_page else { return Act::None };
        let p = (page as i32 + d.signum()).clamp(0, self.em_pages() as i32 - 1) as usize;
        if p == page {
            return Act::None;
        }
        self.emoji_page = Some(p);
        Act::Redraw
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key_at(kb: &Keyboard, want: Key) -> (i32, i32) {
        let mut at = None;
        kb.each_key(|k, x, y, w| {
            if k == want {
                at = Some((x + w / 2, y + KH / 2));
            }
        });
        at.expect("key shown")
    }

    #[test]
    fn rows_fit_the_round_panel() {
        let kb = Keyboard::new(WHITE);
        kb.each_key(|_, x, y, w| {
            let (x0, x1) = chord(y, y + KH, 0);
            assert!(x >= x0 && x + w <= x1, "key at ({x}, {y}) w {w} outside the circle ({x0}..{x1})");
        });
    }

    #[test]
    fn typing_capitalises_the_first_letter_and_after_a_sentence() {
        let mut kb = Keyboard::new(WHITE);
        for k in [Key::Ch(b'h'), Key::Ch(b'i'), Key::Ch(b'.'), Key::Space, Key::Ch(b'o'), Key::Ch(b'k')] {
            kb.type_key(k);
        }
        assert_eq!(kb.text(), "Hi. Ok");
        kb.type_key(Key::Del);
        assert_eq!(kb.text(), "Hi. O");
    }

    #[test]
    fn taps_type_and_switch() {
        let mut kb = Keyboard::new(WHITE);
        let (x, y) = key_at(&kb, Key::Ch(b'q'));
        // the first letter changes the case of all of them
        assert_eq!(kb.tap(x, y), Act::Redraw);
        let (x, y) = key_at(&kb, Key::Ch(b'w'));
        assert_eq!(kb.tap(x, y), Act::Typed(Key::Ch(b'w')));
        assert_eq!(kb.text(), "Qw");
        let (x, y) = key_at(&kb, Key::Sym);
        assert_eq!(kb.tap(x, y), Act::Redraw);
        assert!(kb.sym);
        let (x, y) = key_at(&kb, Key::Dial);
        assert_eq!(kb.tap(x, y), Act::Dial);
        let (x, y) = key_at(&kb, Key::Send);
        assert_eq!(kb.tap(x, y), Act::Send);
        assert_eq!(kb.tap(MIC_X, MIC_Y), Act::Voice);
    }

    #[test]
    fn no_emoji_key_without_the_watch_font() {
        let kb = Keyboard::new(WHITE);
        let mut keys = 0;
        kb.each_key(|k, _, _, _| keys += (k == Key::Emoji) as i32);
        assert_eq!(keys, 0);
    }

    #[test]
    fn words_go_after_the_text_with_a_space() {
        let mut kb = Keyboard::new(WHITE);
        kb.type_key(Key::Ch(b'a'));
        kb.add_words("  coffee at ten ");
        assert_eq!(kb.text(), "A coffee at ten");
        let long = "x".repeat(200);
        kb.add_words(&long);
        assert_eq!(kb.text().len(), TEXT_MAX);
    }

    #[test]
    fn draws_without_panicking() {
        let mut px = vec![0u16; (crate::SIZE * crate::SIZE) as usize];
        let mut fb = crate::Fb::new(&mut px);
        let mut kb = Keyboard::new(WHITE);
        kb.draw(&mut fb, "Keyboards");
        kb.add_words("hello there");
        kb.draw_text(&mut fb);
        let (x, y) = key_at(&kb, Key::Ch(b'q'));
        assert_ne!(fb.at(x - 12, y), 0, "the keys are drawn");
        kb.draw_key(&mut fb, Key::Ch(b'q'), true);
        assert_eq!(fb.at(x - 12, y), 0xFFFF, "a pressed key is white");
    }
}
