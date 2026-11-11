use crate::font8x8::GLYPHS;

pub const W: usize = 466;
pub const H: usize = 466;

pub struct Canvas<'a> {
    pub px: &'a mut [u16],
}

impl Canvas<'_> {
    pub fn fill_rect(&mut self, x: i16, y: i16, w: u16, h: u16, c: u16) {
        let x0 = i32::from(x).max(0);
        let y0 = i32::from(y).max(0);
        let x1 = (i32::from(x) + i32::from(w)).min(W as i32);
        let y1 = (i32::from(y) + i32::from(h)).min(H as i32);
        if x0 >= x1 {
            return;
        }
        for row in y0..y1 {
            let base = row as usize * W;
            self.px[base + x0 as usize..base + x1 as usize].fill(c);
        }
    }

    pub fn text(&mut self, x: i16, y: i16, s: &[u8], c: u16) -> i16 {
        let mut cx = i32::from(x);
        for &ch in s {
            let ch = if (0x20..=0x7E).contains(&ch) { ch } else { b'?' };
            for (row, bits) in GLYPHS[usize::from(ch - 0x20)].iter().enumerate() {
                for col in 0..8 {
                    if bits >> col & 1 != 0 {
                        let px = i16::try_from(cx + 2 * col);
                        let py = i16::try_from(i32::from(y) + 2 * row as i32);
                        if let (Ok(px), Ok(py)) = (px, py) {
                            self.fill_rect(px, py, 2, 2, c);
                        }
                    }
                }
            }
            cx += 16;
        }
        cx.clamp(i16::MIN.into(), i16::MAX.into()) as i16
    }
}

// Dirty box of draw calls, packed into a u32 so it fits one atomic: bytes x0, y0, x1, y1 (inclusive)
// in 2 px units. ponytail: the 2 px grid flushes at most 1 extra px per side; exact needs 36 bits.
pub const EMPTY: u32 = 0x0000_FFFF;
pub const FULL: u32 = ((W as u32 - 1) / 2) << 16 | ((H as u32 - 1) / 2) << 24;

// the part of a fill_rect that lands on screen
pub fn rect_box(x: i16, y: i16, w: u16, h: u16) -> u32 {
    let x0 = i32::from(x).max(0);
    let y0 = i32::from(y).max(0);
    let x1 = (i32::from(x) + i32::from(w)).min(W as i32);
    let y1 = (i32::from(y) + i32::from(h)).min(H as i32);
    if x0 >= x1 || y0 >= y1 {
        return EMPTY;
    }
    (x0 / 2 | (y0 / 2) << 8 | ((x1 - 1) / 2) << 16 | ((y1 - 1) / 2) << 24) as u32
}

pub fn merge(a: u32, b: u32) -> u32 {
    let [a0, a1, a2, a3] = a.to_le_bytes();
    let [b0, b1, b2, b3] = b.to_le_bytes();
    u32::from_le_bytes([a0.min(b0), a1.min(b1), a2.max(b2), a3.max(b3)])
}

// pixels (x0, y0, x1, y1), inclusive
pub fn area(b: u32) -> Option<[usize; 4]> {
    let [x0, y0, x1, y1] = b.to_le_bytes().map(usize::from);
    (x0 <= x1 && y0 <= y1).then(|| [2 * x0, 2 * y0, (2 * x1 + 1).min(W - 1), (2 * y1 + 1).min(H - 1)])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fill_rect_clips() {
        let mut px = vec![0u16; W * H];
        let mut c = Canvas { px: &mut px };
        c.fill_rect(-10, -10, 20, 20, 0xFFFF);
        c.fill_rect(460, 460, 50, 50, 0xFFFF);
        assert_eq!(px.iter().filter(|&&p| p == 0xFFFF).count(), 100 + 36);
    }

    #[test]
    fn text_stays_in_cells() {
        let mut px = vec![0u16; W * H];
        let mut c = Canvas { px: &mut px };
        assert_eq!(c.text(100, 100, b"A\xD0", 0xFFFF), 132);
        assert!(px.iter().any(|&p| p != 0));
        assert!(px.iter().enumerate().all(|(i, &p)| p == 0 || (100..132).contains(&(i % W)) && (100..116).contains(&(i / W))));
    }

    #[test]
    fn dirty_box_clips_and_merges() {
        assert_eq!(area(EMPTY), None);
        assert_eq!(area(FULL), Some([0, 0, W - 1, H - 1]));
        assert_eq!(area(rect_box(-10, -10, 20, 20)), Some([0, 0, 9, 9]));
        assert_eq!(area(rect_box(460, 461, 50, 50)), Some([460, 460, W - 1, H - 1]));
        assert_eq!(area(rect_box(101, 7, 1, 1)), Some([100, 6, 101, 7]));
        for (x, y, w, h) in [(0, 0, 0, 5), (0, 0, 5, 0), (466, 0, 5, 5), (0, -20, 5, 20), (i16::MAX, 0, 9, 9)] {
            assert_eq!(rect_box(x, y, w, h), EMPTY);
        }
        assert_eq!(merge(EMPTY, EMPTY), EMPTY);
        let a = rect_box(10, 300, 4, 4);
        assert_eq!(merge(EMPTY, a), a);
        assert_eq!(area(merge(a, rect_box(200, 20, 10, 2))), Some([10, 20, 209, 303]));
        assert_eq!(merge(a, FULL), FULL);
    }
}
