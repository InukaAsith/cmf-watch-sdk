//! Page view's page, kept compressed in RAM and scrolled here; only what is in view is drawn on each frame. Nothing
//! is kept after the app closes. Two ways, as the phone decides per page (CMFX HubKit Layout):
//! - drawn: the site as it is laid out, drawn here (its boxes, its text in the watch's font, its images as small
//!   pictures): sharp and small;
//! - a picture of it (a "fancy" page: mostly images, a canvas, a video wall): 16-colour bands.
//!
//! Phone → watch: `J h:u16` a new page, `h` its height in panel px; then entries top down, in paint order:
//! - a picture band: `I` (its y0 its place on the page) / `D`… / `E` (pixview rle.rs);
//! - draw ops: `o y0:u16 y1:u16 layer` (the rows they cover; layer 0 boxes, 1 text: all boxes are drawn, then
//!   the pictures, then all text, so a box never covers text from the slab above), then `p ops…`, each op:
//!   `r x:i16 y:u16 w:u16 h:u16 c:u16 bg:u16 radius:u8` a box (`bg` round its corners),
//!   `f x:i16 y:u16 w:u16 h:u16 c:u16` a frame (a 2 px border),
//!   `t x:i16 y:u16 w:u16 c:u16 flags:u8 n:u8 text` a line of text from its top left, cut to `w` (flags: 1 bold,
//!   2 underlined);
//! `j` all of it came.

use crate::rle::{Decoder, Surface as PicSurface, HEADER};
use cmf_wp2_ui::color::Rgb;
use cmf_wp2_ui::draw::rrect;
use cmf_wp2_ui::{font, Surface};

/// Shares its memory with the reader page (ui.rs): the two are never shown at once.
pub const STORE: usize = 56 * 1024;
pub const ENTRIES: usize = 200;
const NONE: u16 = 0xFFFF;
const BAND: u8 = 0;
const OPS: u8 = 1;
const TEXT: u8 = 2;

#[derive(Clone, Copy)]
struct Entry {
    kind: u8,
    /// its bytes in the store (a band: its `I` message, then its tokens)
    off: u16,
    len: u16,
    /// the rows it covers, page px
    y0: u16,
    y1: u16,
}

pub struct Long {
    store: [u8; STORE],
    used: usize,
    entries: [Entry; ENTRIES],
    n: usize,
    /// a band coming in: its next D seq; NONE when none (or it is dropped: no room). Integers only: the memory is
    /// shared and may hold the reader page's bytes until `clear`.
    open: u16,
    /// ops coming in: 1 (their entry is `n`, not counted yet)
    ops: u8,
    /// the page's height, panel px (what was announced; less came when it didn't fit)
    pub height: i32,
    pub done: u8,
}

/// What a message did: an entry ended that should be drawn (its top and bottom, page px).
pub enum Got {
    None,
    Begin,
    Band(i32, i32),
    Done,
}

fn u16_at(m: &[u8], i: usize) -> u16 {
    u16::from_le_bytes([m[i], m[i + 1]])
}

fn rgb(c: u16) -> Rgb {
    Rgb(((c >> 11) << 3) as u8, (((c >> 5) & 63) << 2) as u8, ((c & 31) << 3) as u8)
}

impl Long {
    pub const fn new() -> Self {
        Long {
            store: [0; STORE],
            used: 0,
            entries: [Entry { kind: 0, off: 0, len: 0, y0: 0, y1: 0 }; ENTRIES],
            n: 0,
            open: NONE,
            ops: 0,
            height: 0,
            done: 0,
        }
    }

    pub fn clear(&mut self) {
        self.used = 0;
        self.n = 0;
        self.open = NONE;
        self.ops = 0;
        self.height = 0;
        self.done = 0;
    }

    /// The part of the page that came, panel px.
    pub fn came(&self) -> i32 {
        self.entries[..self.n].iter().map(|b| i32::from(b.y1)).max().unwrap_or(0)
    }

    /// Ops being received: counted now (an entry ends with the next one, or the page).
    fn close_ops(&mut self) -> Got {
        if self.ops == 0 {
            return Got::None;
        }
        self.ops = 0;
        let e = self.entries[self.n];
        self.n += 1;
        Got::Band(i32::from(e.y0), i32::from(e.y1))
    }

    pub fn message(&mut self, m: &[u8]) -> Got {
        match m.first() {
            Some(b'J') if m.len() == 3 => {
                self.clear();
                self.height = i32::from(u16_at(m, 1));
                Got::Begin
            }
            Some(b'j') => {
                self.close_ops();
                self.done = 1;
                self.open = NONE;
                Got::Done
            }
            Some(b'o') if m.len() == 6 => {
                let got = self.close_ops();
                if self.n < ENTRIES {
                    let kind = if m[5] == 1 { TEXT } else { OPS };
                    self.entries[self.n] = Entry { kind, off: self.used as u16, len: 0, y0: u16_at(m, 1), y1: u16_at(m, 3) };
                    self.ops = 1;
                }
                got
            }
            Some(b'p') => {
                let t = &m[1..];
                if self.ops == 1 && self.used + t.len() <= STORE {
                    self.store[self.used..self.used + t.len()].copy_from_slice(t);
                    self.used += t.len();
                    self.entries[self.n].len += t.len() as u16;
                }
                Got::None
            }
            Some(b'I') if m.len() == HEADER => {
                let got = self.close_ops();
                self.open = NONE;
                if self.n >= ENTRIES || self.used + HEADER > STORE {
                    return got;
                }
                let (h, scale, y0) = (u16_at(m, 3), u16::from(m[5].max(1)), u16_at(m, 8));
                self.store[self.used..self.used + HEADER].copy_from_slice(m);
                self.entries[self.n] =
                    Entry { kind: BAND, off: self.used as u16, len: HEADER as u16, y0, y1: y0.saturating_add(h * scale) };
                self.used += HEADER;
                self.open = 0;
                got
            }
            Some(b'D') if m.len() >= 2 => {
                if self.open == NONE {
                    return Got::None;
                }
                let t = &m[2..];
                if u16::from(m[1]) != self.open || self.used + t.len() > STORE {
                    // out of order or no room: the band goes
                    self.open = NONE;
                    self.used = usize::from(self.entries[self.n].off);
                    return Got::None;
                }
                self.store[self.used..self.used + t.len()].copy_from_slice(t);
                self.used += t.len();
                self.entries[self.n].len += t.len() as u16;
                self.open = (self.open + 1) & 0xFF;
                Got::None
            }
            Some(b'E') => {
                if core::mem::replace(&mut self.open, NONE) == NONE {
                    return Got::None;
                }
                let b = self.entries[self.n];
                self.n += 1;
                Got::Band(i32::from(b.y0), i32::from(b.y1))
            }
            _ => Got::None,
        }
    }

    /// What meets panel rows `top..bottom` of the view scrolled to `scroll`, in the order it came (paint order).
    pub fn draw(&self, dec: &mut Decoder, pic: &mut impl PicSurface, s: &mut impl Surface, scroll: i32, top: i32, bottom: i32) {
        // boxes, then pictures, then text
        for pass in [OPS, BAND, TEXT] {
            self.draw_pass(pass, dec, pic, s, scroll, top, bottom);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_pass(&self, pass: u8, dec: &mut Decoder, pic: &mut impl PicSurface, s: &mut impl Surface, scroll: i32, top: i32, bottom: i32) {
        for e in &self.entries[..self.n] {
            let (y0, y1) = (i32::from(e.y0) - scroll, i32::from(e.y1) - scroll);
            if e.kind != pass || y1 <= top || y0 >= bottom {
                continue;
            }
            let (o, l) = (usize::from(e.off), usize::from(e.len));
            if e.kind == BAND {
                let hdr = &self.store[o..o + HEADER];
                let x0 = i32::from(i16::from_le_bytes([hdr[6], hdr[7]]));
                if dec.start(hdr, x0, y0, hdr[5]) {
                    dec.feed(&self.store[o + HEADER..o + l], pic);
                }
            } else {
                draw_ops(&self.store[o..o + l], s, scroll, top, bottom);
            }
        }
    }
}

/// One entry's ops, those in rows `top..bottom` (panel) of the view.
fn draw_ops(ops: &[u8], s: &mut impl Surface, scroll: i32, top: i32, bottom: i32) {
    let mut i = 0;
    while i < ops.len() {
        let m = &ops[i..];
        let k = m[0];
        let need = match k {
            b'r' => 14,
            b'f' => 11,
            b't' if m.len() >= 11 => 11 + usize::from(m[10]),
            _ => return,
        };
        if m.len() < need {
            return;
        }
        i += need;
        let x = i32::from(u16_at(m, 1) as i16);
        let y = i32::from(u16_at(m, 3)) - scroll;
        let w = i32::from(u16_at(m, 5));
        match k {
            b'r' | b'f' => {
                let h = i32::from(u16_at(m, 7));
                let c = u16_at(m, 9);
                // in view: cut to it (a tall box only where it shows)
                let (a, b) = (y.max(top), (y + h).min(bottom));
                if b <= a {
                    continue;
                }
                if k == b'f' {
                    if y >= top {
                        s.rect(x, y, w, 2, c);
                    }
                    if y + h <= bottom {
                        s.rect(x, y + h - 2, w, 2, c);
                    }
                    s.rect(x, a, 2, b - a, c);
                    s.rect(x + w - 2, a, 2, b - a, c);
                } else {
                    let r = i32::from(m[13]);
                    if r > 2 && a == y && b == y + h {
                        rrect(s, x, y, w, h, r.min(h / 2), rgb(c), rgb(u16_at(m, 11)));
                    } else {
                        s.rect(x, a, w, b - a, c);
                    }
                }
            }
            _ => {
                if y + font::HEIGHT <= top || y >= bottom {
                    continue;
                }
                let c = u16_at(m, 7);
                let flags = m[9];
                let t = cmf_wp2_ui::text::fit(&m[11..need], 255);
                font::fitted(s, x, y, t, w.max(8), c);
                if flags & 1 != 0 {
                    // bold: once more, a pixel over
                    font::fitted(s, x + 1, y, t, w.max(8), c);
                }
                if flags & 2 != 0 {
                    s.rect(x, y + font::HEIGHT, font::width(t).min(w), 1, c);
                }
            }
        }
    }
}
