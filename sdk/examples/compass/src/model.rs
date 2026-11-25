//! Compass state, no drawing: the phone's heading as it comes (CMFX `CompassService`, HubKit `CompassProto`).
//!
//! Phone → watch: `H` heading:u16 LE (tenths of a degree, 0..3599) accuracy:u8 (degrees, 255 unknown) flags:u8
//! (1 true north, 2 the phone wants a figure-8 calibration); `N` text (why there is no heading: location off …).
//! Watch → phone: `R` (start, and every few seconds while open: the phone stops when they stop coming), `Z` (closed).

/// No heading for this long: shown as stale.
pub const STALE_MS: u32 = 4000;
/// `R` again this often while open.
pub const PING_MS: u32 = 4000;

pub const TRUE_NORTH: u8 = 1;
pub const CALIBRATE: u8 = 2;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Msg<'a> {
    Heading { tenths: u16, accuracy: u8, flags: u8 },
    Note(&'a str),
}

pub fn parse(m: &[u8]) -> Option<Msg<'_>> {
    match m {
        [b'H', lo, hi, acc, flags] => {
            let tenths = u16::from_le_bytes([*lo, *hi]);
            (tenths < 3600).then_some(Msg::Heading { tenths, accuracy: *acc, flags: *flags })
        }
        [b'N', rest @ ..] => Some(Msg::Note(cmf_wp2_ui::text::fit(rest, 60))),
        _ => None,
    }
}

/// Whole degrees, 0..359.
pub fn degrees(tenths: u16) -> u16 {
    ((tenths as u32 + 5) / 10 % 360) as u16
}

/// The 8-point name of a heading in degrees.
pub fn cardinal(deg: u16) -> &'static str {
    const NAMES: [&str; 8] = ["N", "NE", "E", "SE", "S", "SW", "W", "NW"];
    NAMES[((deg as u32 * 2 + 45) / 90 % 8) as usize]
}

/// The heading passed north between `a` and `b` (degrees), the short way round.
pub fn crossed_north(a: u16, b: u16) -> bool {
    let d = (b as i32 - a as i32 + 540) % 360 - 180;
    if d == 0 {
        return false;
    }
    // walk from a by d: north is crossed by landing on it or stepping over it (leaving it is not)
    let end = a as i32 + d;
    if d > 0 { end >= 360 } else { a != 0 && end <= 0 }
}

/// A point at `r` px from the centre `c`, `rel` degrees clockwise from 12 o'clock.
pub fn polar(c: i32, r: i32, rel: i32) -> (i32, i32) {
    use cmf_wp2_ui::motion::{cos, sin};
    let t = (rel.rem_euclid(360) * 65536 / 360) as u16;
    (c + (r * sin(t) >> 14), c - (r * cos(t) >> 14))
}

/// The heading digits with a degree sign drawn apart ("7", "124").
pub fn digits(deg: u16, buf: &mut [u8; 3]) -> &str {
    let mut n = 0;
    if deg >= 100 {
        buf[n] = b'0' + (deg / 100) as u8;
        n += 1;
    }
    if deg >= 10 {
        buf[n] = b'0' + (deg / 10 % 10) as u8;
        n += 1;
    }
    buf[n] = b'0' + (deg % 10) as u8;
    core::str::from_utf8(&buf[..n + 1]).unwrap_or("")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_heading_and_note() {
        assert_eq!(parse(&[b'H', 0xD4, 0x04, 7, 3]), Some(Msg::Heading { tenths: 1236, accuracy: 7, flags: 3 }));
        assert_eq!(parse(&[b'H', 0x10, 0x0E, 7, 0]), None); // 3600: out of range
        assert_eq!(parse(&[b'H', 1, 2]), None);
        assert_eq!(parse(b"NLocation is off"), Some(Msg::Note("Location is off")));
        assert_eq!(parse(b"Q"), None);
    }

    #[test]
    fn rounds_and_names() {
        assert_eq!(degrees(3595), 0);
        assert_eq!(degrees(1244), 124);
        assert_eq!(cardinal(0), "N");
        assert_eq!(cardinal(22), "N");
        assert_eq!(cardinal(23), "NE");
        assert_eq!(cardinal(124), "SE");
        assert_eq!(cardinal(337), "NW");
        assert_eq!(cardinal(338), "N");
    }

    #[test]
    fn north_crossings() {
        assert!(crossed_north(355, 3));
        assert!(crossed_north(3, 355));
        assert!(crossed_north(359, 0));
        assert!(crossed_north(1, 0));
        assert!(!crossed_north(0, 1));
        assert!(!crossed_north(10, 20));
        assert!(!crossed_north(200, 170));
        assert!(!crossed_north(5, 5));
        assert!(!crossed_north(0, 355));
    }

    #[test]
    fn polar_points() {
        assert_eq!(polar(233, 100, 0), (233, 133));
        assert_eq!(polar(233, 100, 90), (333, 233));
        assert_eq!(polar(233, 100, 180), (233, 333));
        assert_eq!(polar(233, 100, -90), (133, 233));
    }

    #[test]
    fn digit_strings() {
        let mut b = [0u8; 3];
        assert_eq!(digits(7, &mut b), "7");
        assert_eq!(digits(40, &mut b), "40");
        assert_eq!(digits(124, &mut b), "124");
        assert_eq!(digits(300, &mut b), "300");
    }
}
