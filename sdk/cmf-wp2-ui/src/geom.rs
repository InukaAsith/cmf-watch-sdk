//! The panel is a circle of radius [`C`] around (C, C): what fits where.

use crate::motion::isqrt;
use crate::{C, CHAR};

/// Half the width of the visible chord at pixel row `y` (its centre line); 0 outside.
pub fn half(y: i32) -> i32 {
    let d = 2 * y + 1 - 2 * C;
    isqrt(C * C - d * d / 4)
}

/// [x0, x1) visible on every row of [y0, y1), `margin` px in from the edge.
pub fn chord(y0: i32, y1: i32, margin: i32) -> (i32, i32) {
    let dy = (y0 - C).abs().max((y1 - C).abs());
    let h = isqrt(C * C - dy * dy) - margin;
    (C - h, C + h)
}

/// A text line at `y` (one cell high): its left x and how many chars fit, `margin` px in from the edge.
pub fn text_box(y: i32, margin: i32) -> (i32, usize) {
    let (x0, x1) = chord(y, y + CHAR, margin);
    (x0, ((x1 - x0).max(0) / CHAR) as usize)
}

pub fn inside(x: i32, y: i32) -> bool {
    (2 * x + 1 - 2 * C).pow(2) + (2 * y + 1 - 2 * C).pow(2) <= 4 * C * C
}

/// x that centres `w` px on `cx`.
pub fn centred(cx: i32, w: i32) -> i32 {
    cx - w / 2
}

/// Point at distance `r` from (C, C) at angle `a` (turns, y down).
pub fn polar(r: i32, a: u16) -> (i32, i32) {
    use crate::motion::{cos, sin};
    (C + ((r * cos(a) + (1 << 13)) >> 14), C + ((r * sin(a) + (1 << 13)) >> 14))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chords_stay_inside() {
        assert_eq!(half(C), C);
        assert_eq!(half(-5), 0);
        for y0 in (0..466 - 58).step_by(7) {
            let (x0, x1) = chord(y0, y0 + 58, 6);
            for y in [y0, y0 + 58 - 1] {
                assert!(x0 >= 0 && inside(x0 - 5, y) && inside(x1 + 5 - 1, y), "{y0}");
            }
        }
        let (x, n) = text_box(40, 0);
        assert!(inside(x, 40) && inside(x + n as i32 * CHAR - 1, 40) && n >= 8, "{x} {n}");
        assert_eq!(text_box(C - 8, 0), (1, 29));
        assert_eq!(polar(100, 0), (C + 100, C));
        assert_eq!(polar(100, 16384), (C, C + 100));
        assert_eq!(centred(C, 32), C - 16);
    }
}
