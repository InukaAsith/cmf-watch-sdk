//! Photos: a gallery of the phone's pictures (grid of thumbnails, or a list of names), a picture opened full screen.
//!
//! Phone → watch:
//! - `L n:u8 key:u32` the gallery has n pictures (64 at most); `key` changes when the set does (the watch then drops
//!   its cached thumbnails).
//! - `l idx:u8 name…` a picture's name.
//! - `F name`, `f data…`, `c` a file (thumbnail `th<idx>`: an `I` header then run tokens, rle.rs); the watch acks `a`.
//! - `P n` then n bands, each `I`, `D`…, `E` (rle.rs; its own 16 colours, its header's y0 the band's row): the
//!   opened picture. A lone `I`/`D`/`E` is a one-band picture. `?` asks for `R`.
//! - then the sharp version, 256 colours, shown with a progress bar and replacing the bands: `S total:u16 w:u16
//!   h:u16 scale x0:i16 y0:i16`, `K first count rgb565…` (the palette), `Y seq tokens…` (a byte < 255 one pixel of
//!   that colour, `255 c n` n pixels; runs stay in a row), `Z`. A gap: the watch sends `G`, the phone sends it again.
//!
//! Watch → phone: `R` ready (send the gallery), `q idx` a thumbnail it has not got, `o idx` open a picture (stepping through: the one it stops on), `g` back in the gallery (thumbnails go on), `a` file
//! stored, `G seq` a gap in the picture's stream.

pub const MAX: usize = 64;
pub const NAME: usize = 40;

#[derive(Clone, Copy)]
pub struct Entry {
    pub name: [u8; NAME],
    pub len: u8,
}

impl Entry {
    pub const EMPTY: Entry = Entry { name: [0; NAME], len: 0 };

    pub fn name(&self) -> &str {
        let b = &self.name[..self.len as usize];
        match core::str::from_utf8(b) {
            Ok(s) => s,
            Err(e) => core::str::from_utf8(&b[..e.valid_up_to()]).unwrap_or(""),
        }
    }
}

pub struct Gallery {
    pub n: usize,
    pub key: u32,
    pub items: [Entry; MAX],
}

impl Gallery {
    pub const fn new() -> Self {
        Self { n: 0, key: 0, items: [Entry::EMPTY; MAX] }
    }

    /// `L` or `l`: true when it was one.
    pub fn apply(&mut self, m: &[u8]) -> bool {
        match m.first() {
            Some(b'L') if m.len() == 6 => {
                self.n = (m[1] as usize).min(MAX);
                self.key = u32::from_le_bytes([m[2], m[3], m[4], m[5]]);
                for e in self.items.iter_mut() {
                    *e = Entry::EMPTY;
                }
                true
            }
            Some(b'l') if m.len() >= 2 && (m[1] as usize) < MAX => {
                let e = &mut self.items[m[1] as usize];
                let t = &m[2..m.len().min(2 + NAME)];
                e.name[..t.len()].copy_from_slice(t);
                e.len = t.len() as u8;
                true
            }
            _ => false,
        }
    }
}

/// `th<idx>` into `out`.
pub fn thumb_name(idx: usize, out: &mut [u8; 6]) -> &str {
    out[0] = b't';
    out[1] = b'h';
    let mut n = 2;
    if idx >= 10 {
        out[n] = b'0' + (idx / 10 % 10) as u8;
        n += 1;
    }
    out[n] = b'0' + (idx % 10) as u8;
    n += 1;
    core::str::from_utf8(&out[..n]).unwrap_or("th0")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gallery_messages() {
        let mut g = Gallery::new();
        assert!(g.apply(&[b'L', 2, 1, 0, 0, 0]));
        assert_eq!((g.n, g.key), (2, 1));
        assert!(g.apply(b"l\x01Beach.jpg"));
        assert_eq!(g.items[1].name(), "Beach.jpg");
        assert!(!g.apply(b"x"));
        let mut b = [0u8; 6];
        assert_eq!(thumb_name(7, &mut b), "th7");
        assert_eq!(thumb_name(42, &mut b), "th42");
    }
}
