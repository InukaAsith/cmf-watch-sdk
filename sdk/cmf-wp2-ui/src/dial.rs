//! The rotary keyboard (WhatsApp, Notes, Gemini, Spotify search). Letters round the rim: the crown turns the
//! selection; a tap on a letter types it; grab a letter, slide round the rim and let go: the letter let go on is
//! typed. The middle types the picked letter (held: a capital), with `,` and `.` either side of it. Below:
//! delete, space, send.
//!
//! The app has no finger-move events (TOUCH_DOWN and TOUCH_UP only), so a slide counts from where the finger went
//! down to where it came up.
//!
//! The app keeps the typed text and draws it above y 170; [`Dial`] draws the rest and answers with an [`Act`].

use core::cell::Cell;

use crate::color::{rgb565, Rgb, BLACK};
use crate::draw::{disc, rrect};
use crate::geom::polar;
use crate::motion::{angle, isqrt};
use crate::{font, text, Surface, C};

pub const LETTERS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ?#";
pub const NUMBERS: &[u8] = b"0123456789?!@'-:()+/&#";
/// The letters' circle.
const R: i32 = 204;
const CELL: i32 = 19;
/// The bar under the middle.
pub const BAR_Y: i32 = 312;
const BAR_H: i32 = 40;
const BAR: [(u8, i32); 3] = [(b'<', 60), (b' ', 124), (b'>', 60)];
const GAP: i32 = 8;
/// `,` and `.` beside the middle letter: their x offset from the centre, radius.
const PUNCT_DX: i32 = 100;
const PUNCT_R: i32 = 26;
const PUNCT_Y: i32 = C - 6;
/// Top of the middle (typed text above).
const MID_Y: i32 = 176;
/// Held this long: a capital.
pub const HOLD_MS: u32 = 450;
/// Crown gain in 1/100 letter per detent: slow turn, fast spin.
const SLOW: i32 = 17;
const FAST: i32 = 60;

const RED: Rgb = Rgb(0xD7, 0x19, 0x21);
const WHITE: Rgb = Rgb(0xFF, 0xFF, 0xFF);
const KEY: Rgb = Rgb(0x2A, 0x2A, 0x2A);
const W: u16 = 0xFFFF;
const SOFT: u16 = rgb565(0xC8, 0xC8, 0xC8);
const GREY: u16 = rgb565(0x80, 0x80, 0x80);
const DIM: u16 = rgb565(0x3A, 0x3A, 0x3A);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Key {
    Ch(u8),
    Space,
    Del,
    Send,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Act {
    None,
    /// something was drawn: tick and show
    Show,
    /// the other set of keys: draw the whole screen
    Redraw,
    /// typed (a letter already in its case)
    Key(Key),
}

pub struct Dial {
    pub sym: bool,
    /// the next letter a capital
    pub shift: bool,
    pub sel: usize,
    /// the letter that types at lift: index, when the finger went down, whether the hold made it a capital
    press: Option<(usize, u32, bool)>,
    /// a letter grabbed on the rim
    grab: bool,
    acc: i32,
    at: u32,
    gap: i32,
    pitch: Cell<i32>,
}

impl Default for Dial {
    fn default() -> Self {
        Self::new()
    }
}

impl Dial {
    pub const fn new() -> Self {
        Dial { sym: false, shift: true, sel: 0, press: None, grab: false, acc: 0, at: 0, gap: 400, pitch: Cell::new(0) }
    }

    fn keys(&self) -> &'static [u8] {
        if self.sym { NUMBERS } else { LETTERS }
    }

    fn at(&self, i: usize) -> (i32, i32) {
        // from 12 o'clock, clockwise
        polar(R, (49152u32 + (i as u32 * 65536) / self.keys().len() as u32) as u16)
    }

    fn label(&self, i: usize) -> &'static str {
        let k = self.keys();
        match k[i] {
            b'#' if self.sym => "ab",
            b'#' => "12",
            _ => core::str::from_utf8(&k[i..i + 1]).unwrap_or("?"),
        }
    }

    /// The letter nearest an angle.
    fn index_at(&self, a: u16) -> usize {
        let n = self.keys().len() as u32;
        ((a.wrapping_sub(49152) as u32 * n + 32768) / 65536 % n) as usize
    }

    fn bar_x0() -> i32 {
        let total: i32 = BAR.iter().map(|b| b.1).sum::<i32>() + GAP * (BAR.len() as i32 - 1);
        C - total / 2
    }

    fn bar_key(x: i32, y: i32) -> Option<u8> {
        if !(BAR_Y - 6..BAR_Y + BAR_H + 6).contains(&y) {
            return None;
        }
        let mut bx = Self::bar_x0();
        for &(k, w) in &BAR {
            if (bx - GAP / 2..bx + w + GAP / 2).contains(&x) {
                return Some(k);
            }
            bx += w + GAP;
        }
        None
    }

    pub fn draw(&self, s: &mut impl Surface) {
        for (dx, label) in [(-PUNCT_DX, ","), (PUNCT_DX, ".")] {
            disc(s, C + dx, PUNCT_Y, PUNCT_R, KEY, BLACK);
            font::centre(s, C + dx, PUNCT_Y - 12, label, W);
        }
        self.draw_bar(s);
        for i in 0..self.keys().len() {
            self.draw_cell(s, i);
        }
        self.draw_centre(s);
    }

    fn draw_bar(&self, s: &mut impl Surface) {
        let mut bx = Self::bar_x0();
        for &(k, w) in &BAR {
            let send = k == b'>';
            rrect(s, bx, BAR_Y, w, BAR_H, BAR_H / 2, if send { RED } else { KEY }, BLACK);
            let label = match k {
                b' ' => "space",
                b'<' => "<",
                _ => ">",
            };
            font::centre(s, bx + w / 2, BAR_Y + 9, label, W);
            bx += w + GAP;
        }
    }

    pub fn draw_cell(&self, s: &mut impl Surface, i: usize) {
        let (x, y) = self.at(i);
        if i == self.sel {
            disc(s, x, y, CELL, WHITE, BLACK);
            font::centre(s, x, y - 8, self.label(i), 0);
        } else {
            // the old selection's disc goes
            disc(s, x, y, CELL, BLACK, BLACK);
            let c = match self.keys()[i] {
                b'#' => W,
                b'0'..=b'9' | b'A'..=b'Z' => SOFT,
                _ => GREY,
            };
            font::centre(s, x, y - 8, self.label(i), c);
        }
    }

    fn upper(&self) -> bool {
        self.shift || matches!(self.press, Some((_, _, true)))
    }

    pub fn draw_centre(&self, s: &mut impl Surface) {
        let mut b = [0u8; 4];
        let label = match self.keys()[self.sel] {
            b'#' if self.sym => "ABC",
            b'#' => "123",
            c if c.is_ascii_alphabetic() && !self.upper() => (c.to_ascii_lowercase() as char).encode_utf8(&mut b),
            _ => self.label(self.sel),
        };
        let pitch = if label.len() == 1 { 8 } else { 3 };
        let h = 8 * pitch;
        let w = label.chars().count() as i32 * 8 * pitch;
        let (x, y) = (C - w / 2, C - h / 2 - 6);
        // painted over the last letter dot for dot (off dots black): no clear, no flicker; a change of size clears
        if self.pitch.get() != pitch {
            s.rect(C - 60, MID_Y, 120, 100, 0);
            self.pitch.set(pitch);
        } else {
            s.rect(C - 60, MID_Y, x - (C - 60), 100, 0);
            s.rect(x + w, MID_Y, C + 60 - x - w, 100, 0);
        }
        text::dots_opaque(s, x, y, label, pitch, pitch - 1, W, 0);
        font::centre(s, C, 280, "tap  hold: Aa", DIM);
    }

    fn select(&mut self, s: &mut impl Surface, i: usize) {
        let old = self.sel;
        self.sel = i;
        self.draw_cell(s, old);
        self.draw_cell(s, i);
        self.draw_centre(s);
    }

    /// One crown detent (±1) at `now`: the speed ramps with the turn's pace (a quarter letter a detent when slow,
    /// one and a half when spun); a turn back starts over.
    pub fn crown(&mut self, s: &mut impl Surface, d: i32, now: u32) -> Act {
        let d = d.signum();
        let gap = now.wrapping_sub(self.at).min(400) as i32;
        self.at = now;
        self.gap = (self.gap + gap) / 2;
        let g = self.gap.clamp(30, 220);
        let gain = SLOW + (FAST - SLOW) * (220 - g) / 190;
        if (self.acc > 0) != (d > 0) {
            self.acc = 0;
        }
        self.acc += d * gain;
        if self.acc.abs() < 100 {
            return Act::None;
        }
        let step = self.acc / 100;
        self.acc %= 100;
        let n = self.keys().len() as i32;
        self.select(s, (self.sel as i32 + step).rem_euclid(n) as usize);
        Act::Show
    }

    /// TOUCH_DOWN: a bar key types at once; a letter is picked; the middle and the grip wait for the lift.
    pub fn down(&mut self, s: &mut impl Surface, x: i32, y: i32, now: u32) -> Act {
        self.press = None;
        self.grab = false;
        if let Some(k) = Self::bar_key(x, y) {
            return Act::Key(match k {
                b'<' => Key::Del,
                b'>' => Key::Send,
                _ => Key::Space,
            });
        }
        for (dx, c) in [(-PUNCT_DX, b','), (PUNCT_DX, b'.')] {
            let (ex, ey) = (x - C - dx, y - PUNCT_Y);
            if ex * ex + ey * ey <= (PUNCT_R + 8) * (PUNCT_R + 8) {
                self.shift = false;
                return Act::Key(Key::Ch(c));
            }
        }
        let (dx, dy) = (x - C, y - C);
        if isqrt(dx * dx + dy * dy) > R - 34 {
            let i = self.index_at(angle(dx, dy));
            self.press = Some((i, now, false));
            self.grab = true;
            self.select(s, i);
            return Act::Show;
        }
        if (MID_Y..BAR_Y - 6).contains(&y) {
            self.press = Some((self.sel, now, false));
        }
        Act::None
    }

    /// TOUCH_UP: the letter pressed types; a letter grabbed and slid round the rim types the one let go on.
    pub fn up(&mut self, s: &mut impl Surface, x: i32, y: i32) -> Act {
        let (grab, press) = (core::mem::take(&mut self.grab), self.press.take());
        let Some((mut i, _, upper)) = press else { return Act::None };
        if grab {
            let (dx, dy) = (x - C, y - C);
            if isqrt(dx * dx + dy * dy) > 90 {
                let i1 = self.index_at(angle(dx, dy));
                if i1 != i {
                    i = i1;
                    self.select(s, i);
                }
            }
        }
        let c = self.keys()[i];
        if c == b'#' {
            self.sym = !self.sym;
            self.sel = 0;
            return Act::Redraw;
        }
        let c = if c.is_ascii_alphabetic() && !(self.shift || upper) { c.to_ascii_lowercase() } else { c };
        self.shift = false;
        self.draw_centre(s);
        Act::Key(Key::Ch(c))
    }

    /// When a held letter turns capital (ms from `now`), for the app's event timeout.
    pub fn hold_wait(&self, now: u32) -> Option<u32> {
        match self.press {
            Some((_, t, false)) => Some(t.wrapping_add(HOLD_MS).wrapping_sub(now).min(HOLD_MS)),
            _ => None,
        }
    }

    /// Held long enough: the letter becomes a capital (shown in the middle). `Show` when it did.
    pub fn tick(&mut self, s: &mut impl Surface, now: u32) -> Act {
        if let Some((i, t, false)) = self.press {
            if now.wrapping_sub(t) >= HOLD_MS {
                self.press = Some((i, t, true));
                self.draw_centre(s);
                return Act::Show;
            }
        }
        Act::None
    }

    /// The finger's lift will never come (the screen changed).
    pub fn cancel(&mut self) {
        self.press = None;
        self.grab = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Fb;

    fn fb() -> std::vec::Vec<u16> {
        std::vec![0u16; (crate::SIZE * crate::SIZE) as usize]
    }

    #[test]
    fn bar_keys() {
        let x0 = Dial::bar_x0();
        assert_eq!(Dial::bar_key(x0 + 20, BAR_Y + 20), Some(b'<'));
        assert_eq!(Dial::bar_key(C, BAR_Y + 20), Some(b' '));
        assert_eq!(Dial::bar_key(C, BAR_Y - 40), None);
        let mut px = fb();
        let mut s = Fb::new(&mut px);
        let mut d = Dial::new();
        assert_eq!(d.down(&mut s, C, BAR_Y + 20, 0), Act::Key(Key::Space));
        assert_eq!(d.up(&mut s, C, BAR_Y + 20), Act::None);
        assert_eq!(d.down(&mut s, x0 + 20, BAR_Y + 20, 0), Act::Key(Key::Del));
        assert_eq!(d.down(&mut s, C - PUNCT_DX, PUNCT_Y, 0), Act::Key(Key::Ch(b',')));
        assert_eq!(d.down(&mut s, C + PUNCT_DX, PUNCT_Y + 10, 0), Act::Key(Key::Ch(b'.')));
    }

    #[test]
    fn tap_the_middle_types_the_pick_then_lower_case() {
        let mut px = fb();
        let mut s = Fb::new(&mut px);
        let mut d = Dial::new();
        d.sel = 7; // H
        assert_eq!(d.down(&mut s, C, C, 0), Act::None);
        assert_eq!(d.up(&mut s, C, C), Act::Key(Key::Ch(b'H')));
        d.down(&mut s, C, C, 10);
        assert_eq!(d.up(&mut s, C, C), Act::Key(Key::Ch(b'h')));
        // held: a capital
        d.down(&mut s, C, C, 100);
        assert_eq!(d.tick(&mut s, 100 + HOLD_MS), Act::Show);
        assert_eq!(d.up(&mut s, C, C), Act::Key(Key::Ch(b'H')));
    }

    #[test]
    fn rim_tap_types_rim_slide_types_where_let_go() {
        let mut px = fb();
        let mut s = Fb::new(&mut px);
        let mut d = Dial::new();
        let (x, y) = d.at(2);
        assert_eq!(d.down(&mut s, x, y, 0), Act::Show);
        assert_eq!(d.up(&mut s, x, y), Act::Key(Key::Ch(b'C')));
        let (x2, y2) = d.at(5);
        d.down(&mut s, x, y, 0);
        assert_eq!(d.up(&mut s, x2, y2), Act::Key(Key::Ch(b'f')));
        assert_eq!(d.sel, 5);
        // # switches to the numbers
        let i = LETTERS.len() - 1;
        let (hx, hy) = d.at(i);
        d.down(&mut s, hx, hy, 0);
        assert_eq!(d.up(&mut s, hx, hy), Act::Redraw);
        assert!(d.sym && d.sel == 0);
    }
}
