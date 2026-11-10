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
}
