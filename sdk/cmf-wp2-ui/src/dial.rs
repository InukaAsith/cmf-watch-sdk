//! The rotary keyboard (WhatsApp, Notes, Gemini, Spotify search). Letters round the rim: the crown turns the
//! selection, a tap on a letter types it, a slide along the rim lands on the letter under the finger at lift. A grip
//! circle inside: slide round it and lift, the letters turn by as much. The middle types the picked letter (held:
//! a capital). Below, a bar: `,` delete, space, send, `.`.
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
/// The grip circle.
const GRIP_R: i32 = 158;
/// The bar under the middle.
pub const BAR_Y: i32 = 312;
const BAR_H: i32 = 40;
const BAR: [(u8, i32); 5] = [(b',', 36), (b'<', 48), (b' ', 96), (b'>', 52), (b'.', 36)];
const GAP: i32 = 6;
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
const GRIP: u16 = rgb565(0x5A, 0x5A, 0x5A);

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
    /// where a slide began: its angle, and whether on the letters (else on the grip)
    grab: Option<(u16, bool)>,
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
        Dial { sym: false, shift: true, sel: 0, press: None, grab: None, acc: 0, at: 0, gap: 400, pitch: Cell::new(0) }
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
        // the grip: two arcs between the typed text and the bar
        for k in 0..96u32 {
            let (x, y) = polar(GRIP_R, (k * 65536 / 96) as u16);
            if (MID_Y + 2..BAR_Y - 14).contains(&y) {
                s.rect(x - 2, y - 2, 4, 4, GRIP);
            }
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
                b'<' => "del",
                b'>' => "send",
                b',' => ",",
                _ => ".",
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
        self.grab = None;
        if let Some(k) = Self::bar_key(x, y) {
            return Act::Key(match k {
                b'<' => Key::Del,
                b'>' => Key::Send,
                b' ' => Key::Space,
                c => {
                    self.shift = false;
                    Key::Ch(c)
                }
            });
        }
        let (dx, dy) = (x - C, y - C);
        let r = isqrt(dx * dx + dy * dy);
        let a = angle(dx, dy);
        if r > R - 34 {
            let i = self.index_at(a);
            self.press = Some((i, now, false));
            self.grab = Some((a, true));
            self.select(s, i);
            return Act::Show;
        }
        if r >= 120 {
            self.grab = Some((a, false));
        }
        if (MID_Y..BAR_Y - 6).contains(&y) {
            self.press = Some((self.sel, now, false));
        }
        Act::None
    }

    /// TOUCH_UP: a slide moves the selection (nothing typed); else the pressed letter types.
    pub fn up(&mut self, s: &mut impl Surface, x: i32, y: i32) -> Act {
        let (grab, press) = (self.grab.take(), self.press.take());
        if let Some((a0, rim)) = grab {
            let (dx, dy) = (x - C, y - C);
            let a1 = angle(dx, dy);
            if rim {
                let i1 = self.index_at(a1);
                if isqrt(dx * dx + dy * dy) > 90 && press.is_some_and(|p| p.0 != i1) {
                    self.select(s, i1);
                    return Act::Show;
                }
            } else {
                let n = self.keys().len() as i32;
                let k = turned(a0, a1, n);
                if k != 0 {
                    self.select(s, (self.sel as i32 + k).rem_euclid(n) as usize);
                    return Act::Show;
                }
            }
        }
        let Some((i, _, upper)) = press else { return Act::None };
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
        self.grab = None;
    }
}

/// Letters turned by a slide from angle `a0` to `a1` (`n` letters round), rounded; a half turn at most each way.
fn turned(a0: u16, a1: u16, n: i32) -> i32 {
    let d = a1.wrapping_sub(a0) as i16 as i32;
    (d * n + if d >= 0 { 32768 } else { -32768 }) / 65536
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Fb;

    fn fb() -> std::vec::Vec<u16> {
        std::vec![0u16; (crate::SIZE * crate::SIZE) as usize]
    }

    #[test]
    fn a_slide_turns_by_its_angle() {
        let n = LETTERS.len() as i32;
        assert_eq!(turned(0, 0, n), 0);
        assert_eq!(turned(0, (65536 / n) as u16, n), 1);
        assert_eq!(turned((65536 / n) as u16 * 3, 0, n), -3);
        // a sixth of a letter is no turn
        assert_eq!(turned(0, (65536 / n / 6) as u16, n), 0);
        // over 12 o'clock
        assert_eq!(turned(65000, 1500, n), 1);
    }

    #[test]
    fn bar_keys() {
        let x0 = Dial::bar_x0();
        assert_eq!(Dial::bar_key(x0 + 20, BAR_Y + 20), Some(b','));
        assert_eq!(Dial::bar_key(C, BAR_Y + 20), Some(b' '));
        assert_eq!(Dial::bar_key(C, BAR_Y - 40), None);
        let mut px = fb();
        let mut s = Fb::new(&mut px);
        let mut d = Dial::new();
        assert_eq!(d.down(&mut s, C, BAR_Y + 20, 0), Act::Key(Key::Space));
        assert_eq!(d.up(&mut s, C, BAR_Y + 20), Act::None);
        assert_eq!(d.down(&mut s, x0 + 20, BAR_Y + 20, 0), Act::Key(Key::Ch(b',')));
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
    fn grip_slide_turns_without_typing() {
        let mut px = fb();
        let mut s = Fb::new(&mut px);
        let mut d = Dial::new();
        d.shift = false;
        // down on the grip at 9 o'clock (left), up at 12 o'clock: a quarter turn clockwise
        d.down(&mut s, C - GRIP_R, C, 0);
        assert_eq!(d.up(&mut s, C, C - GRIP_R), Act::Show);
        assert_eq!(d.sel, LETTERS.len() / 4);
        // a tap on the grip in the middle band types
        d.down(&mut s, C - GRIP_R, C + 20, 0);
        assert!(matches!(d.up(&mut s, C - GRIP_R, C + 20), Act::Key(Key::Ch(_))));
    }

    #[test]
    fn rim_tap_types_rim_slide_picks() {
        let mut px = fb();
        let mut s = Fb::new(&mut px);
        let mut d = Dial::new();
        let (x, y) = d.at(2);
        assert_eq!(d.down(&mut s, x, y, 0), Act::Show);
        assert_eq!(d.up(&mut s, x, y), Act::Key(Key::Ch(b'C')));
        let (x2, y2) = d.at(5);
        d.down(&mut s, x, y, 0);
        assert_eq!(d.up(&mut s, x2, y2), Act::Show);
        assert_eq!(d.sel, 5);
        // # switches to the numbers
        let i = LETTERS.len() - 1;
        let (hx, hy) = d.at(i);
        d.down(&mut s, hx, hy, 0);
        assert_eq!(d.up(&mut s, hx, hy), Act::Redraw);
        assert!(d.sym && d.sel == 0);
    }
}
