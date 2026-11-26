//! Palette run-length images streamed from the phone, drawn as one rect per run.
//!
//! Wire format (every message ≤ 128 B, host → app):
//! - `I` w:u16 h:u16 scale:u8 x0:i16 y0:i16 palette:16×u16 (RGB565), all little endian: a new image. Bit 7 of
//!   `scale` (KEEP): colour 15 is not drawn, what is under it stays (a video frame's unchanged pixels).
//! - `D` seq:u8 tokens…: the next bytes of the run stream; `seq` counts D messages from 0, wrapping at 256.
//! - `E`: the image is complete.
//!
//! A token is one byte `cccc nnnn`: colour `c`, run `n + 1` (1..=15). `n == 15` takes one more byte `e`:
//! run `16 + e` (16..=271). Runs never cross a row; the encoder splits them.

pub const PALETTE: usize = 16;
/// Length of an `I` message.
pub const HEADER: usize = 10 + 2 * PALETTE;
/// `scale`'s bit: colour 15 keeps what is under it.
pub const KEEP: u8 = 0x80;

struct Null;
impl Surface for Null {
    fn rect(&mut self, _: i32, _: i32, _: i32, _: i32, _: u16) {}
}

pub trait Surface {
    fn rect(&mut self, x: i32, y: i32, w: i32, h: i32, c: u16);
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Status {
    /// Nothing to report.
    Ok,
    /// A new image started.
    Begin,
    /// The image is complete: rects drawn.
    Done(u32),
    /// A `D` came out of order: the expected seq. The image is dropped until the next `I`.
    Gap(u8),
    /// A malformed message.
    Bad,
}

pub struct Decoder {
    pub w: u16,
    pub h: u16,
    scale: u8,
    x0: i16,
    y0: i16,
    pal: [u16; PALETTE],
    x: u16,
    y: u16,
    seq: u8,
    pending: Option<u8>,
    active: bool,
    keep: bool,
    pub rects: u32,
}

impl Default for Decoder {
    fn default() -> Self {
        Self::new()
    }
}

impl Decoder {
    pub const fn new() -> Self {
        Self { w: 0, h: 0, scale: 1, x0: 0, y0: 0, pal: [0; PALETTE], x: 0, y: 0, seq: 0, pending: None, active: false, keep: false, rects: 0 }
    }

    pub fn active(&self) -> bool {
        self.active
    }

    pub fn message(&mut self, m: &[u8], s: &mut impl Surface) -> Status {
        match m.first() {
            Some(b'I') if m.len() == HEADER => {
                let u = |i: usize| u16::from_le_bytes([m[i], m[i + 1]]);
                self.w = u(1);
                self.h = u(3);
                self.scale = (m[5] & !KEEP).max(1);
                self.keep = m[5] & KEEP != 0;
                self.x0 = u(6) as i16;
                self.y0 = u(8) as i16;
                for (i, c) in self.pal.iter_mut().enumerate() {
                    *c = u(10 + 2 * i);
                }
                self.x = 0;
                self.y = 0;
                self.seq = 0;
                self.pending = None;
                self.rects = 0;
                self.active = self.w > 0 && self.h > 0;
                if self.active { Status::Begin } else { Status::Bad }
            }
            Some(b'D') if m.len() >= 2 => {
                if !self.active {
                    return Status::Ok;
                }
                if m[1] != self.seq {
                    self.active = false;
                    return Status::Gap(self.seq);
                }
                self.seq = self.seq.wrapping_add(1);
                for &b in &m[2..] {
                    self.token(b, s);
                }
                Status::Ok
            }
            Some(b'E') => {
                let done = self.active;
                self.active = false;
                if done { Status::Done(self.rects) } else { Status::Ok }
            }
            _ => Status::Bad,
        }
    }

    /// Draws a stored image again: `header` is its `I` message, then [`Decoder::feed`] its whole token stream.
    /// `x0`, `y0` and `scale` replace the header's, for an instant local pan or zoom.
    pub fn start(&mut self, header: &[u8], x0: i32, y0: i32, scale: u8) -> bool {
        if self.message(header, &mut Null) != Status::Begin {
            return false;
        }
        self.x0 = x0.clamp(i16::MIN as i32, i16::MAX as i32) as i16;
        self.y0 = y0.clamp(i16::MIN as i32, i16::MAX as i32) as i16;
        self.scale = (scale & !KEEP).max(1);
        true
    }

    pub fn feed(&mut self, tokens: &[u8], s: &mut impl Surface) {
        for &b in tokens {
            self.token(b, s);
        }
    }

    /// `(x0, y0, scale)` of the image being drawn.
    pub fn origin(&self) -> (i32, i32, u8) {
        (self.x0 as i32, self.y0 as i32, self.scale)
    }

    fn token(&mut self, b: u8, s: &mut impl Surface) {
        let (c, run) = match self.pending.take() {
            Some(c) => (c, 16 + b as u16),
            None => {
                let (c, n) = (b >> 4, b & 15);
                if n == 15 {
                    self.pending = Some(c);
                    return;
                }
                (c, n as u16 + 1)
            }
        };
        if self.y >= self.h {
            return;
        }
        let run = run.min(self.w - self.x);
        let k = self.scale as i32;
        let (x, y, w) = (self.x0 as i32 + self.x as i32 * k, self.y0 as i32 + self.y as i32 * k, run as i32 * k);
        // off the panel: skip the call (a local zoom draws mostly outside)
        if !(self.keep && c == 15) && y + k > 0 && y < 466 && x + w > 0 && x < 466 {
            s.rect(x, y, w, k, self.pal[c as usize]);
            self.rects += 1;
        }
        self.x += run;
        if self.x >= self.w {
            self.x = 0;
            self.y += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    extern crate std;
    use std::vec::Vec;

    #[derive(Default)]
    struct Rec(Vec<(i32, i32, i32, i32, u16)>);
    impl Surface for Rec {
        fn rect(&mut self, x: i32, y: i32, w: i32, h: i32, c: u16) {
            self.0.push((x, y, w, h, c));
        }
    }

    fn header(w: u16, h: u16, scale: u8, x0: i16, y0: i16) -> Vec<u8> {
        let mut m = std::vec![b'I'];
        m.extend(w.to_le_bytes());
        m.extend(h.to_le_bytes());
        m.push(scale);
        m.extend(x0.to_le_bytes());
        m.extend(y0.to_le_bytes());
        for i in 0..16u16 {
            m.extend((i * 0x1000 + i).to_le_bytes());
        }
        m
    }

    #[test]
    fn short_and_long_runs_wrap_rows() {
        let (mut d, mut s) = (Decoder::new(), Rec::default());
        assert_eq!(d.message(&header(20, 2, 2, 10, 5), &mut s), Status::Begin);
        // row 0: colour 1 × 3, colour 2 × 17 (long); row 1: colour 3 × 20 (long)
        assert_eq!(d.message(&[b'D', 0, 0x12, 0x2F, 1], &mut s), Status::Ok);
        assert_eq!(d.message(&[b'D', 1, 0x3F, 4], &mut s), Status::Ok);
        assert_eq!(d.message(b"E", &mut s), Status::Done(3));
        assert_eq!(s.0, [(10, 5, 6, 2, 0x1001), (16, 5, 34, 2, 0x2002), (10, 7, 40, 2, 0x3003)]);
    }

    #[test]
    fn long_run_split_across_messages() {
        let (mut d, mut s) = (Decoder::new(), Rec::default());
        d.message(&header(16, 1, 1, 0, 0), &mut s);
        d.message(&[b'D', 0, 0x5F], &mut s);
        d.message(&[b'D', 1, 0], &mut s);
        assert_eq!(s.0, [(0, 0, 16, 1, 0x5005)]);
    }

    #[test]
    fn gap_drops_the_image() {
        let (mut d, mut s) = (Decoder::new(), Rec::default());
        d.message(&header(4, 4, 1, 0, 0), &mut s);
        assert_eq!(d.message(&[b'D', 1, 0x03], &mut s), Status::Gap(0));
        assert_eq!(d.message(&[b'D', 0, 0x03], &mut s), Status::Ok);
        assert_eq!(d.message(b"E", &mut s), Status::Ok);
        assert!(s.0.is_empty());
    }

    #[test]
    fn overflow_is_clipped() {
        let (mut d, mut s) = (Decoder::new(), Rec::default());
        d.message(&header(4, 1, 1, 0, 0), &mut s);
        d.message(&[b'D', 0, 0x09, 0x01], &mut s); // 10 wide on a 4-wide image, then past the last row
        assert_eq!(s.0, [(0, 0, 4, 1, 0)]);
    }

    #[test]
    fn stored_redraw_moves_and_scales() {
        let (mut d, mut s) = (Decoder::new(), Rec::default());
        let h = header(4, 2, 2, 0, 0);
        assert!(d.start(&h, 100, 50, 4));
        d.feed(&[0x13, 0x23], &mut s);
        assert_eq!(s.0, [(100, 50, 16, 4, 0x1001), (100, 54, 16, 4, 0x2002)]);
    }

    #[test]
    fn offscreen_runs_are_skipped() {
        let (mut d, mut s) = (Decoder::new(), Rec::default());
        assert!(d.start(&header(4, 3, 1, 0, 0), 0, -1, 1));
        d.feed(&[0x03, 0x13, 0x23], &mut s);
        assert_eq!(s.0, [(0, 0, 4, 1, 0x1001), (0, 1, 4, 1, 0x2002)]);
    }

    #[test]
    fn bad_header() {
        let (mut d, mut s) = (Decoder::new(), Rec::default());
        assert_eq!(d.message(b"I", &mut s), Status::Bad);
        assert_eq!(d.message(&header(0, 4, 1, 0, 0), &mut s), Status::Bad);
    }
}
