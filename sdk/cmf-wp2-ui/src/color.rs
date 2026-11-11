use crate::ONE;

/// 8-bit RGB; maths happens here, [`Rgb::rgb565`] rounds once at the end.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Rgb(pub u8, pub u8, pub u8);

pub const BLACK: Rgb = Rgb(0, 0, 0);
pub const WHITE: Rgb = Rgb(0xFF, 0xFF, 0xFF);

impl Rgb {
    pub const fn rgb565(self) -> u16 {
        rgb565(self.0, self.1, self.2)
    }

    /// The 565 colour widened back so that `from565(c).rgb565() == c` (white reads 251, 253, 251).
    pub const fn from565(c: u16) -> Rgb {
        let (r, g, b) = ((c >> 11) as u8, (c >> 5 & 63) as u8, (c & 31) as u8);
        Rgb(r << 3 | r >> 3, g << 2 | g >> 5, b << 3 | b >> 3)
    }

    /// Brightness `k` of [`ONE`] (above ONE brightens, saturating).
    pub fn scale(self, k: i32) -> Rgb {
        let s = |v: u8| (v as i32 * k.max(0) / ONE).min(255) as u8;
        Rgb(s(self.0), s(self.1), s(self.2))
    }

    /// `self` at t = 0 to `o` at t = ONE.
    pub fn mix(self, o: Rgb, t: i32) -> Rgb {
        let t = t.clamp(0, ONE);
        let m = |a: u8, b: u8| (a as i32 + (b as i32 - a as i32) * t / ONE) as u8;
        Rgb(m(self.0, o.0), m(self.1, o.1), m(self.2, o.2))
    }
}

/// Rounded, saturating.
pub const fn rgb565(r: u8, g: u8, b: u8) -> u16 {
    const fn q(v: u8, bits: u32) -> u16 {
        let x = (v as u16 + (1 << (7 - bits))) >> (8 - bits);
        let max = (1 << bits) - 1;
        if x > max { max } else { x }
    }
    q(r, 5) << 11 | q(g, 6) << 5 | q(b, 5)
}

/// `a` at t = 0 to `b` at t = ONE, both 565.
pub fn blend565(a: u16, b: u16, t: i32) -> u16 {
    Rgb::from565(a).mix(Rgb::from565(b), t).rgb565()
}

pub fn dim565(c: u16, k: i32) -> u16 {
    Rgb::from565(c).scale(k).rgb565()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rgb565_rounds_saturates_and_round_trips() {
        assert_eq!(WHITE.rgb565(), 0xFFFF);
        assert_eq!(BLACK.rgb565(), 0);
        assert_eq!(Rgb(4, 0, 0).rgb565(), 1 << 11);
        assert_eq!(Rgb(200, 200, 200).scale(2 * ONE), WHITE);
        for c in [0u16, 0xFFFF, 0xF800, 0x07E0, 0x001F, 0x8C71, 0x2124, 0x1234] {
            assert_eq!(Rgb::from565(c).rgb565(), c, "{c:04x}");
        }
        assert_eq!(blend565(0, 0xFFFF, ONE / 2), Rgb(127, 127, 127).rgb565());
        assert_eq!(dim565(0xFFFF, 0), 0);
    }
}
