//! Page view's picture: the whole page (as much as fits) sent once, kept compressed in RAM and scrolled here; only
//! the bands in view are decoded on each draw. Nothing is kept after the app closes.
//!
//! Phone → watch: `J h:u16` a new page picture, `h` its height in panel px; then its bands, each `I` (its y0 is the
//! band's place on the page, panel px) / `D`… / `E` (pixview rle.rs, 16 colours a band); `j` all of it came.

use crate::rle::{Decoder, Surface, HEADER};

/// Shares its memory with the reader page (ui.rs): the two are never shown at once.
pub const STORE: usize = 56 * 1024;
const NONE: u16 = 0xFFFF;
pub const BANDS: usize = 96;

#[derive(Clone, Copy)]
struct Band {
    /// its `I` message, then its tokens, in the store
    off: u16,
    len: u16,
    y0: i16,
    h: u16,
}

pub struct Long {
    store: [u8; STORE],
    used: usize,
    bands: [Band; BANDS],
    n: usize,
    /// a band coming in: its next D seq; NONE when none (or it is dropped: no room). Integers only: the memory is
    /// shared and may hold the reader page's bytes until `clear`.
    open: u16,
    /// the page's height, panel px (what was announced; less came when it didn't fit)
    pub height: i32,
    pub done: u8,
}

/// What a message did: a band ended that should be drawn (its top and bottom, page px).
pub enum Got {
    None,
    Begin,
    Band(i32, i32),
    Done,
}

impl Long {
    pub const fn new() -> Self {
        Long {
            store: [0; STORE],
            used: 0,
            bands: [Band { off: 0, len: 0, y0: 0, h: 0 }; BANDS],
            n: 0,
            open: NONE,
            height: 0,
            done: 0,
        }
    }

    pub fn clear(&mut self) {
        self.used = 0;
        self.n = 0;
        self.open = NONE;
        self.height = 0;
        self.done = 0;
    }

    /// The part of the page that came, panel px.
    pub fn came(&self) -> i32 {
        self.bands[..self.n].iter().map(|b| i32::from(b.y0) + i32::from(b.h)).max().unwrap_or(0)
    }

    pub fn message(&mut self, m: &[u8]) -> Got {
        match m.first() {
            Some(b'J') if m.len() == 3 => {
                self.clear();
                self.height = i32::from(u16::from_le_bytes([m[1], m[2]]));
                Got::Begin
            }
            Some(b'j') => {
                self.done = 1;
                self.open = NONE;
                Got::Done
            }
            Some(b'I') if m.len() == HEADER => {
                self.open = NONE;
                if self.n >= BANDS || self.used + HEADER > STORE {
                    return Got::None;
                }
                let u = |i: usize| u16::from_le_bytes([m[i], m[i + 1]]);
                let (h, scale, y0) = (u(3), u16::from(m[5].max(1)), u(8) as i16);
                self.store[self.used..self.used + HEADER].copy_from_slice(m);
                self.bands[self.n] = Band { off: self.used as u16, len: HEADER as u16, y0, h: h * scale };
                self.used += HEADER;
                self.open = 0;
                Got::None
            }
            Some(b'D') if m.len() >= 2 => {
                if self.open == NONE {
                    return Got::None;
                }
                let t = &m[2..];
                if u16::from(m[1]) != self.open || self.used + t.len() > STORE {
                    // out of order or no room: the band goes
                    self.open = NONE;
                    self.used = usize::from(self.bands[self.n].off);
                    return Got::None;
                }
                self.store[self.used..self.used + t.len()].copy_from_slice(t);
                self.used += t.len();
                self.bands[self.n].len += t.len() as u16;
                self.open = (self.open + 1) & 0xFF;
                Got::None
            }
            Some(b'E') => {
                if core::mem::replace(&mut self.open, NONE) == NONE {
                    return Got::None;
                }
                let b = self.bands[self.n];
                self.n += 1;
                Got::Band(i32::from(b.y0), i32::from(b.y0) + i32::from(b.h))
            }
            _ => Got::None,
        }
    }

    /// The bands that meet panel rows `top..bottom` of the view scrolled to `scroll`.
    pub fn draw(&self, dec: &mut Decoder, s: &mut impl Surface, scroll: i32, top: i32, bottom: i32) {
        for b in &self.bands[..self.n] {
            let (y0, y1) = (i32::from(b.y0) - scroll, i32::from(b.y0) + i32::from(b.h) - scroll);
            if y1 <= top || y0 >= bottom {
                continue;
            }
            let (o, l) = (usize::from(b.off), usize::from(b.len));
            let hdr = &self.store[o..o + HEADER];
            let x0 = i32::from(i16::from_le_bytes([hdr[6], hdr[7]]));
            if dec.start(hdr, x0, y0, hdr[5]) {
                dec.feed(&self.store[o + HEADER..o + l], s);
            }
        }
    }
}
