//! The offline map on the watch (loader FILE service), drawn by the watch itself as Maps draws its map: CMFX cuts
//! OpenStreetMap's vector tiles (OpenFreeMap) for an area into square cells, a few levels of them (fewer roads the
//! farther out), each cell a file of Maps' scene records (atlas scene.rs) in metres of the pack's frame, clipped to
//! its square.
//!
//! - file `idx`: `WMV1`, `lat0:i32 lon0:i32` (1e-7°, the frame's centre), `mm_per_deg_lon:u32`, `n:u8`, then per
//!   level, coarse to fine, `side:u16 x0:i16 y0:i16 cols:u8 rows:u8 min_k:u8` (metres; the grid's south-west corner;
//!   the level is drawn from zoom `min_k` on, px a metre × 256).
//! - files `v<level>_<col>_<row>` (col east, row north from the corner): the cell's records; none: nothing there.
//!
//! Phone → watch: `W` delete the map (`w` back), `F name…` a new file, `f data…` its next bytes, `c` file complete
//! (`a` back), `P done:u16 total:u16` progress, `Z` the map complete (`k` back); a route: `G n:u16 metres:u32
//! secs:u32` then `g seq:u8 (x:i16 y:i16)…` (30 points a message), `E text` it failed.
//! Watch → phone: `R` ready, `D mode lat0:i32 lon0:i32 from_x:i16 from_y:i16 to_x:i16 to_y:i16` a route (mode 0
//! drive, 2 walk; from i16::MIN: where the phone is), metres of the frame given.

use crate::geo::Frame;

pub const MAX_LEVELS: usize = 4;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Level {
    pub side: i32,
    pub x0: i32,
    pub y0: i32,
    pub cols: u8,
    pub rows: u8,
    pub min_k: i32,
}

impl Level {
    /// The cells (col, row) a square round `c` of half side `r` touches.
    pub fn cells(&self, c: (i32, i32), r: i32) -> (core::ops::Range<i32>, core::ops::Range<i32>) {
        let s = self.side.max(1);
        let lo = |v: i32, o: i32, n: u8| (v - o).div_euclid(s).clamp(0, i32::from(n));
        let hi = |v: i32, o: i32, n: u8| ((v - o).div_euclid(s) + 1).clamp(0, i32::from(n));
        (lo(c.0 - r, self.x0, self.cols)..hi(c.0 + r, self.x0, self.cols), lo(c.1 - r, self.y0, self.rows)..hi(c.1 + r, self.y0, self.rows))
    }

    /// Cell (col, row)'s south-west corner, frame metres.
    pub fn corner(&self, col: i32, row: i32) -> (i32, i32) {
        (self.x0 + col * self.side, self.y0 + row * self.side)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Pack {
    pub frame: Frame,
    pub levels: [Level; MAX_LEVELS],
    pub n: usize,
}

impl Pack {
    pub fn parse(b: &[u8]) -> Option<Pack> {
        if b.len() < 17 || &b[..4] != b"WMV1" {
            return None;
        }
        let i32_at = |i: usize| i32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]]);
        let i16_at = |i: usize| i32::from(i16::from_le_bytes([b[i], b[i + 1]]));
        let mut p = Pack {
            frame: Frame { lat0_e7: i32_at(4), lon0_e7: i32_at(8), mm_per_deg_lon: i32_at(12) as u32 },
            ..Pack::default()
        };
        p.n = usize::from(b[16]).min(MAX_LEVELS);
        for (k, l) in p.levels[..p.n].iter_mut().enumerate() {
            let at = 17 + 9 * k;
            if b.len() < at + 9 {
                return None;
            }
            *l = Level {
                side: i32::from(u16::from_le_bytes([b[at], b[at + 1]])),
                x0: i16_at(at + 2),
                y0: i16_at(at + 4),
                cols: b[at + 6],
                rows: b[at + 7],
                min_k: i32::from(b[at + 8]),
            };
        }
        (p.n > 0).then_some(p)
    }

    /// The finest level drawn at zoom `k`.
    pub fn level_for(&self, k: i32) -> usize {
        (0..self.n).rev().find(|&i| k >= self.levels[i].min_k).unwrap_or(0)
    }

    /// The area's square (all levels): centre and half side, metres.
    pub fn bounds(&self) -> ((i32, i32), (i32, i32)) {
        let mut lo = (i32::MAX, i32::MAX);
        let mut hi = (i32::MIN, i32::MIN);
        for l in &self.levels[..self.n] {
            lo = (lo.0.min(l.x0), lo.1.min(l.y0));
            hi = (hi.0.max(l.x0 + l.side * i32::from(l.cols)), hi.1.max(l.y0 + l.side * i32::from(l.rows)));
        }
        (lo, hi)
    }
}

/// `v<level>_<col>_<row>` into `out`; its length.
pub fn cell_name(level: usize, col: i32, row: i32, out: &mut [u8; 20]) -> usize {
    let mut n = 0;
    let mut put = |b: u8, n: &mut usize| {
        out[*n] = b;
        *n += 1;
    };
    put(b'v', &mut n);
    put(b'0' + level as u8, &mut n);
    for v in [col, row] {
        put(b'_', &mut n);
        let mut d = [0u8; 4];
        let mut k = 0;
        let mut v = v.max(0) as u32;
        loop {
            d[k] = b'0' + (v % 10) as u8;
            k += 1;
            v /= 10;
            if v == 0 || k == 4 {
                break;
            }
        }
        for i in (0..k).rev() {
            put(d[i], &mut n);
        }
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;
    extern crate std;

    fn idx() -> std::vec::Vec<u8> {
        let mut b = b"WMV1".to_vec();
        b.extend(69_271_000i32.to_le_bytes());
        b.extend(798_612_000i32.to_le_bytes());
        b.extend(110_511_000u32.to_le_bytes());
        b.push(2);
        // 6 km cells, 4 × 4 round the centre, from k 0; 1.5 km cells, 8 × 8, from k 80
        for (side, x0, n, k) in [(6000u16, -12000i16, 4u8, 0u8), (1500, -6000, 8, 80)] {
            b.extend(side.to_le_bytes());
            b.extend(x0.to_le_bytes());
            b.extend(x0.to_le_bytes());
            b.extend([n, n, k]);
        }
        b
    }

    #[test]
    fn parses_and_picks_levels() {
        let p = Pack::parse(&idx()).unwrap();
        assert_eq!(p.n, 2);
        assert_eq!(p.frame.lon0_e7, 798_612_000);
        assert_eq!(p.levels[1], Level { side: 1500, x0: -6000, y0: -6000, cols: 8, rows: 8, min_k: 80 });
        assert_eq!(p.level_for(30), 0);
        assert_eq!(p.level_for(80), 1);
        assert_eq!(p.bounds(), ((-12000, -12000), (12000, 12000)));
        assert!(Pack::parse(b"WMP1").is_none());
    }

    #[test]
    fn cells_round_a_point() {
        let l = Level { side: 1500, x0: -6000, y0: -6000, cols: 8, rows: 8, min_k: 80 };
        // the centre with 700 m round it: cells 3 and 4 each way
        assert_eq!(l.cells((0, 0), 700), (3..5, 3..5));
        // the edge: cut to the grid
        assert_eq!(l.cells((-5800, 5900), 400), (0..1, 7..8));
        // wholly outside: none
        assert_eq!(l.cells((-6500, 0), 400).0, 0..0);
        assert_eq!(l.corner(3, 4), (-1500, 0));
        let mut n = [0u8; 20];
        let k = cell_name(1, 12, 7, &mut n);
        assert_eq!(&n[..k], b"v1_12_7");
    }
}
