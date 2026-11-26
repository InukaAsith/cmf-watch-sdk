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
//!   `g x:i16 y:u16 w:u16 h:u16 id:u8` the site's logo, built into this app (0 Google, 1 YouTube, 2 Wikipedia,
//!   3 Fandom: scripts/browser-logos), in the middle of the box;
//! `j` all of it came.

use crate::rle::{Decoder, Surface as PicSurface, HEADER};
use cmf_wp2_ui::color::Rgb;
use cmf_wp2_ui::draw::rrect;
use cmf_wp2_ui::{font, Surface};

/// The store is in two parts: `LOW` bytes here, which share their memory with the reader page (ui.rs: the two are
/// never shown at once), then `FAR` bytes in the icon's memory. The loader shows its own copy of an app's icon in the
/// menu (docs/app-icons.md), so once the app runs the icon's pixels at the front of the image are RAM nobody reads;
/// every launch reloads the image, so the icon is whole again next time. An entry is never split between the two:
/// one that would cross moves to the far part whole.
const LOW: usize = 28 * 1024;
const FAR: usize = 96 * 96 * 3;
pub const STORE: usize = LOW + FAR;
const _: () = assert!(STORE <= u16::MAX as usize);

/// The far part: the icon's pixels, after its 16-byte descriptor.
#[cfg(target_os = "none")]
fn far() -> *mut u8 {
    unsafe { (core::ptr::addr_of_mut!(crate::ui::WP2_ICON) as *mut u8).add(16) }
}

/// The simulator has no icon in front of its code: a plain buffer.
#[cfg(not(target_os = "none"))]
fn far() -> *mut u8 {
    static mut SIM: [u8; FAR] = [0; FAR];
    core::ptr::addr_of_mut!(SIM) as *mut u8
}
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
    low: [u8; LOW],
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
            low: [0; LOW],
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

    /// The bytes at `o..o + l` of the store (never across the two parts).
    fn bytes(&self, o: usize, l: usize) -> &[u8] {
        if o >= LOW {
            unsafe { core::slice::from_raw_parts(far().add(o - LOW), l) }
        } else {
            &self.low[o..o + l]
        }
    }

    /// `t` onto the end of the entry being received (`entries[n]`, its `off` set); false: no room.
    fn put(&mut self, t: &[u8]) -> bool {
        if self.used + t.len() > STORE {
            return false;
        }
        if self.used < LOW && self.used + t.len() > LOW {
            // it would cross into the far part: the entry so far moves there
            let start = usize::from(self.entries[self.n].off);
            let k = self.used - start;
            if LOW + k + t.len() > STORE {
                return false;
            }
            unsafe { core::ptr::copy_nonoverlapping(self.low.as_ptr().add(start), far(), k) };
            self.entries[self.n].off = LOW as u16;
            self.used = LOW + k;
        }
        if self.used >= LOW {
            unsafe { core::ptr::copy_nonoverlapping(t.as_ptr(), far().add(self.used - LOW), t.len()) };
        } else {
            self.low[self.used..self.used + t.len()].copy_from_slice(t);
        }
        self.used += t.len();
        true
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
                if self.ops == 1 && self.put(t) {
                    self.entries[self.n].len += t.len() as u16;
                }
                Got::None
            }
            Some(b'I') if m.len() == HEADER => {
                let got = self.close_ops();
                self.open = NONE;
                if self.n >= ENTRIES {
                    return got;
                }
                let (h, scale, y0) = (u16_at(m, 3), u16::from(m[5].max(1)), u16_at(m, 8));
                self.entries[self.n] =
                    Entry { kind: BAND, off: self.used as u16, len: HEADER as u16, y0, y1: y0.saturating_add(h * scale) };
                if !self.put(m) {
                    return got;
                }
                self.open = 0;
                got
            }
            Some(b'D') if m.len() >= 2 => {
                if self.open == NONE {
                    return Got::None;
                }
                let t = &m[2..];
                if u16::from(m[1]) != self.open || !self.put(t) {
                    // out of order or no room: the band goes
                    self.open = NONE;
                    self.used = usize::from(self.entries[self.n].off);
                    return Got::None;
                }
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
                let b = self.bytes(o, l);
                let hdr = &b[..HEADER];
                let x0 = i32::from(i16::from_le_bytes([hdr[6], hdr[7]]));
                if dec.start(hdr, x0, y0, hdr[5]) {
                    dec.feed(&b[HEADER..], pic);
                }
            } else {
                draw_ops(self.bytes(o, l), s, scroll, top, bottom);
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
            b'g' => 10,
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
            b'g' => {
                let h = i32::from(u16_at(m, 7));
                logo(s, m[9], x + w / 2, y + h / 2, top, bottom);
            }
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

/// The built-in logos (scripts/browser-logos; none when it wasn't run): `count`, an offset:u16 each, then each
/// `w h n palette565×n runs…`, a run a byte: colour (0 clear) high nibble, length − 1 low nibble, row after row.
static LOGOS: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/logos.bin"));

/// Logo `id` centred on `cx`, `cy` (panel), only its rows in `top..bottom`.
fn logo(s: &mut impl Surface, id: u8, cx: i32, cy: i32, top: i32, bottom: i32) {
    let n = usize::from(LOGOS[0]);
    let id = usize::from(id);
    if id >= n {
        return;
    }
    let o = usize::from(u16_at(LOGOS, 1 + 2 * id));
    let (w, h, k) = (i32::from(LOGOS[o]), i32::from(LOGOS[o + 1]), usize::from(LOGOS[o + 2]));
    let pal = o + 3;
    let (x0, y0) = (cx - w / 2, cy - h / 2);
    let (mut px, mut py) = (0, 0);
    for &b in &LOGOS[pal + 2 * k..] {
        if py >= h {
            break;
        }
        let (c, mut len) = (usize::from(b >> 4), i32::from(b & 15) + 1);
        while len > 0 {
            let run = len.min(w - px);
            let y = y0 + py;
            if c != 0 && c <= k && y >= top && y < bottom {
                s.rect(x0 + px, y, run, 1, u16_at(LOGOS, pal + 2 * (c - 1)));
            }
            px += run;
            len -= run;
            if px >= w {
                px = 0;
                py += 1;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 13 ops entries of 4 KB (7 fit here, 6 far): the one that would cross into the far part moves there whole, and every entry
    /// reads back as it came.
    #[test]
    fn entries_cross_into_the_far_part_whole() {
        let mut l = Box::new(Long::new());
        assert!(matches!(l.message(&[b'J', 0, 40]), Got::Begin));
        let mut kept = 0;
        for i in 0..13u8 {
            l.message(&[b'o', i, 0, i + 1, 0, 0]);
            for j in 0..32u8 {
                let mut m = [i ^ j; 128];
                m[0] = b'p';
                l.message(&m);
            }
            kept = l.n;
        }
        l.message(b"j");
        assert_eq!(l.n, 13, "{kept}");
        for (i, e) in l.entries[..l.n].iter().enumerate() {
            let (o, n) = (usize::from(e.off), usize::from(e.len));
            assert_eq!(n, 32 * 127);
            assert!(o + n <= LOW || o >= LOW, "entry {i} split");
            let b = l.bytes(o, n);
            for j in 0..32 {
                assert!(b[j * 127..(j + 1) * 127].iter().all(|&v| v == i as u8 ^ j as u8), "entry {i} piece {j}");
            }
        }
        assert!(l.entries[..l.n].iter().any(|e| usize::from(e.off) >= LOW));
    }
}
