//! Offline map pack on the watch (loader FILE service; hosts/mapbuild.py writes it):
//!
//! - file `idx`: `WMP1`, levels: u8, palette 16 × u16 (RGB565, shared by every tile), then per level (ascending z)
//!   `z: u8, x0: u32, y0: u32, w: u8, h: u8, mm_per_px: u32` (the tile grid in OSM numbering; ground mm per pixel
//!   at the centre, for the scale bar), then `x: u32, y: u32`: the centre the pack was built round, in world pixels
//!   at zoom 20 (no floats on the watch: positions are always z20 world pixels).
//! - files `t<level>_<col>_<row>`: one 256 × 256 px tile, `scale: u8` (1, or 2 for a 128² tile drawn doubled) then
//!   pixview run tokens (rle.rs).
//!
//! Phone → watch: `F name…` a new file, `f data…` its next bytes, `c` file complete (watch acks `a`),
//! `P done:u16 total:u16` progress, `Z` pack complete, `X x:u32 y:u32` the phone's position (z20 world px).
//! Watch → phone: `R` ready, `a` file stored, `k` pack loaded.

pub const TILE: i32 = 256;
pub const WORLD_Z: u8 = 20;
pub const MAX_LEVELS: usize = 6;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Level {
    pub z: u8,
    pub x0: u32,
    pub y0: u32,
    pub w: u8,
    pub h: u8,
    pub mm_per_px: u32,
}

pub struct Pack {
    pub palette: [u16; 16],
    pub levels: [Level; MAX_LEVELS],
    pub n: usize,
    pub centre: (u32, u32),
}

impl Pack {
    pub const fn empty() -> Self {
        Pack { palette: [0; 16], levels: [Level { z: 0, x0: 0, y0: 0, w: 0, h: 0, mm_per_px: 0 }; MAX_LEVELS], n: 0, centre: (0, 0) }
    }

    pub fn parse(b: &[u8]) -> Option<Pack> {
        if b.len() < 5 + 32 || &b[..4] != b"WMP1" {
            return None;
        }
        let n = usize::from(b[4]).min(MAX_LEVELS);
        let mut p = Pack::empty();
        for (i, c) in p.palette.iter_mut().enumerate() {
            *c = u16::from_le_bytes([b[5 + 2 * i], b[6 + 2 * i]]);
        }
        let u = |i: usize| u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]]);
        let mut at = 37;
        for l in p.levels[..n].iter_mut() {
            if b.len() < at + 15 {
                return None;
            }
            *l = Level { z: b[at], x0: u(at + 1), y0: u(at + 5), w: b[at + 9], h: b[at + 10], mm_per_px: u(at + 11) };
            if l.z > WORLD_Z {
                return None;
            }
            at += 15;
        }
        if b.len() >= at + 8 {
            p.centre = (u(at), u(at + 4));
        }
        p.n = n;
        (n > 0).then_some(p)
    }

    /// The `I` header pixview's decoder wants for a tile of this pack.
    pub fn header(&self, scale: u8) -> [u8; crate::rle::HEADER] {
        let mut h = [0u8; crate::rle::HEADER];
        let n = (TILE / i32::from(scale.max(1))) as u16;
        h[0] = b'I';
        h[1..3].copy_from_slice(&n.to_le_bytes());
        h[3..5].copy_from_slice(&n.to_le_bytes());
        h[5] = scale;
        for (i, c) in self.palette.iter().enumerate() {
            h[10 + 2 * i..12 + 2 * i].copy_from_slice(&c.to_le_bytes());
        }
        h
    }
}

/// `t<level>_<col>_<row>` into `out`.
pub fn tile_name(level: usize, col: u32, row: u32, out: &mut [u8; 20]) -> usize {
    let mut n = 0;
    let mut put = |b: u8| {
        out[n] = b;
        n += 1;
    };
    put(b't');
    for (i, v) in [level as u32, col, row].into_iter().enumerate() {
        if i > 0 {
            put(b'_');
        }
        let mut d = [0u8; 10];
        let mut k = 0;
        let mut v = v;
        loop {
            d[k] = b'0' + (v % 10) as u8;
            k += 1;
            v /= 10;
            if v == 0 {
                break;
            }
        }
        for j in (0..k).rev() {
            put(d[j]);
        }
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;
    extern crate std;

    #[test]
    fn names() {
        let mut b = [0u8; 20];
        let n = tile_name(2, 13, 0, &mut b);
        assert_eq!(&b[..n], b"t2_13_0");
    }

    #[test]
    fn idx_round_trip() {
        let mut b = std::vec::Vec::from(*b"WMP1");
        b.push(2);
        for i in 0..16u16 {
            b.extend((i * 3).to_le_bytes());
        }
        for (z, x0, y0, w, h) in [(15u8, 16370u32, 10896u32, 4u8, 3u8), (16, 32740, 21792, 8, 6)] {
            b.push(z);
            b.extend(x0.to_le_bytes());
            b.extend(y0.to_le_bytes());
            b.extend([w, h]);
            b.extend(1490u32.to_le_bytes());
        }
        b.extend(134_103_000u32.to_le_bytes());
        b.extend(89_255_000u32.to_le_bytes());
        let p = Pack::parse(&b).unwrap();
        assert_eq!(p.n, 2);
        assert_eq!(p.palette[2], 6);
        assert_eq!(p.levels[1], Level { z: 16, x0: 32740, y0: 21792, w: 8, h: 6, mm_per_px: 1490 });
        assert_eq!(p.centre, (134_103_000, 89_255_000));
        assert!(Pack::parse(b"WMP0").is_none());
    }
}
