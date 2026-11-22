//! Navigation state, filled by the phone (every message ≤ 128 B). Phone → watch:
//!
//! - `V idx kind dir dist:u16 eta_min:u16 left_10m:u16 hh mm street…`: the next manoeuvre (step `idx`), `dist` m to
//!   it, minutes and tens of metres left, arrival time. `kind`: 0 turn, 1 roundabout, 2 arrive, 3 straight on,
//!   4 fork or ramp. `dir`: 0 straight, 1 slight left, 2 left, 3 sharp left, 4 slight right, 5 right, 6 sharp right,
//!   7 U-turn.
//! - `G n:u16` a new route of n points, then `g off:u16 (x:i16 y:i16)…` points in metres east/north of the route's
//!   origin.
//! - `X x:i16 y:i16 heading:u16` where we are (metres) and the heading in degrees (0 north, clockwise).
//! - `W` arrived, `r` rerouting.
//!
//! The watch draws the route itself, so the map turns and zooms without new pictures.

use cmf_wp2_ui::motion::{cos, sin};

pub const MAX_PTS: usize = 400;
/// Buzz once when the next turn comes within this many metres, and again (strong) at the turn. The phone sends a
/// fix about once a second: at 50 m a car at 50 km/h still has 3 s, so "now" isn't skipped between two fixes.
pub const SOON_M: u16 = 200;
pub const NOW_M: u16 = 50;

/// The manoeuvre in words: "Turn left onto Galle Road", "At the roundabout, exit onto …", "Arrive at …". The
/// phone's placeholders ("Arrive", "Continue", "") name no street.
pub fn instruction<'a>(kind: u8, dir: u8, street: &str, out: &'a mut [u8; 112]) -> &'a str {
    let street = match street {
        "Arrive" | "Continue" => "",
        s => s,
    };
    let side = if dir <= 3 { "left" } else { "right" };
    let (a, b, c): (&str, &str, &str) = match (kind, dir) {
        (2, _) => ("Arrive", "", " at "),
        (1, _) => ("At the roundabout, exit", "", " onto "),
        (3, _) => ("Continue", "", " on "),
        (4, 0) => ("Keep straight", "", " onto "),
        (4, _) => ("Keep ", side, " onto "),
        (_, 7) => ("Make a U-turn", "", " onto "),
        (_, 1) | (_, 4) => ("Turn slight ", side, " onto "),
        (_, 3) | (_, 6) => ("Turn sharp ", side, " onto "),
        (_, 0) => ("Go straight", "", " onto "),
        _ => ("Turn ", side, " onto "),
    };
    let mut n = 0;
    for part in [a, b, if street.is_empty() { "" } else { c }, street] {
        let k = part.len().min(out.len() - n);
        out[n..n + k].copy_from_slice(&part.as_bytes()[..k]);
        n += k;
    }
    match core::str::from_utf8(&out[..n]) {
        Ok(s) => s,
        Err(e) => core::str::from_utf8(&out[..e.valid_up_to()]).unwrap_or(""),
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Effect {
    None,
    /// The manoeuvre changed; `buzz`: 1 the turn is close (SOON_M), 2 it is now (NOW_M).
    Turn {
        buzz: u8,
    },
    Pos,
    Arrived,
    Reroute,
    /// The route was stopped (on the phone, or the ✕ here): back to the start screen.
    Ended,
}

pub struct Nav {
    pub step: u8,
    pub kind: u8,
    pub dir: u8,
    pub dist: u16,
    pub eta_min: u16,
    pub left_10m: u16,
    pub arrive: (u8, u8),
    street: [u8; 48],
    n_street: usize,
    pub pts: [(i16, i16); MAX_PTS],
    pub n_pts: usize,
    pub pos: (i16, i16),
    pub heading: u16,
    pub active: bool,
    pub arrived: bool,
    pub rerouting: bool,
    /// the highest buzz already given for `step`
    buzzed: u8,
}

impl Nav {
    pub const fn new() -> Self {
        Nav {
            step: 0,
            kind: 0,
            dir: 0,
            dist: 0,
            eta_min: 0,
            left_10m: 0,
            arrive: (0, 0),
            street: [0; 48],
            n_street: 0,
            pts: [(0, 0); MAX_PTS],
            n_pts: 0,
            pos: (0, 0),
            heading: 0,
            active: false,
            arrived: false,
            rerouting: false,
            buzzed: 0,
        }
    }

    /// No route on (stopped here or on the phone).
    pub fn end(&mut self) {
        self.active = false;
        self.arrived = false;
        self.rerouting = false;
        self.n_pts = 0;
        self.buzzed = 0;
    }

    pub fn street(&self) -> &str {
        cmf_wp2_ui::text::fit(&self.street[..self.n_street], 48)
    }

    pub fn apply(&mut self, m: &[u8]) -> Effect {
        let Some((&op, a)) = m.split_first() else {
            return Effect::None;
        };
        let u = |i: usize| u16::from_le_bytes([a[i], a[i + 1]]);
        let s = |i: usize| i16::from_le_bytes([a[i], a[i + 1]]);
        match op {
            b'V' if a.len() >= 11 => {
                if a[0] != self.step {
                    self.buzzed = 0;
                }
                (self.step, self.kind, self.dir) = (a[0], a[1], a[2]);
                (self.dist, self.eta_min, self.left_10m) = (u(3), u(5), u(7));
                self.arrive = (a[9], a[10]);
                let t = cmf_wp2_ui::text::fit(&a[11..], 48).as_bytes();
                self.street[..t.len()].copy_from_slice(t);
                self.n_street = t.len();
                self.active = true;
                self.arrived = false;
                self.rerouting = false;
                let want = if self.dist <= NOW_M {
                    2
                } else if self.dist <= SOON_M {
                    1
                } else {
                    0
                };
                let buzz = if want > self.buzzed { want } else { 0 };
                self.buzzed = self.buzzed.max(want);
                Effect::Turn { buzz }
            }
            b'G' if a.len() == 2 => {
                self.n_pts = 0;
                Effect::None
            }
            b'g' if a.len() >= 2 => {
                let off = u(0) as usize;
                for (i, p) in a[2..].chunks_exact(4).enumerate() {
                    if off + i < MAX_PTS {
                        self.pts[off + i] = (
                            i16::from_le_bytes([p[0], p[1]]),
                            i16::from_le_bytes([p[2], p[3]]),
                        );
                        self.n_pts = self.n_pts.max(off + i + 1);
                    }
                }
                Effect::Pos
            }
            b'X' if a.len() == 6 => {
                self.pos = (s(0), s(2));
                self.heading = u(4) % 360;
                Effect::Pos
            }
            b'W' => {
                self.arrived = true;
                Effect::Arrived
            }
            b'r' => {
                self.rerouting = true;
                Effect::Reroute
            }
            b'Z' => {
                self.end();
                Effect::Ended
            }
            _ => Effect::None,
        }
    }

    /// A route point on screen, heading up: `(cx, cy)` is where we are, `px_q8` the scale in px per metre × 256.
    pub fn project(&self, p: (i16, i16), cx: i32, cy: i32, px_q8: i32) -> (i32, i32) {
        let (dx, dy) = (
            p.0 as i32 - self.pos.0 as i32,
            p.1 as i32 - self.pos.1 as i32,
        );
        let a = (self.heading as u32 * 65536 / 360) as u16;
        let (c, s) = (cos(a), sin(a));
        // rotate by -heading: right of us is +x', ahead is +y'
        let xr = dx * c - dy * s;
        let yr = dx * s + dy * c;
        let k = |v: i32| ((v >> 6) as i64 * px_q8 as i64 >> 16).clamp(-30000, 30000) as i32;
        (cx + k(xr), cy - k(yr))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    extern crate std;
    use std::boxed::Box;
    use std::vec::Vec;

    fn v(step: u8, dist: u16) -> Vec<u8> {
        let mut m = std::vec![b'V', step, 0, 2];
        m.extend(dist.to_le_bytes());
        m.extend(12u16.to_le_bytes());
        m.extend(340u16.to_le_bytes());
        m.extend([14, 32]);
        m.extend(b"Charing Cross Rd");
        m
    }

    #[test]
    fn manoeuvre_and_buzzes() {
        let mut n = Box::new(Nav::new());
        assert_eq!(n.apply(&v(1, 450)), Effect::Turn { buzz: 0 });
        assert_eq!(
            (n.dist, n.eta_min, n.left_10m, n.arrive),
            (450, 12, 340, (14, 32))
        );
        assert_eq!(n.street(), "Charing Cross Rd");
        assert_eq!(n.apply(&v(1, 190)), Effect::Turn { buzz: 1 });
        assert_eq!(n.apply(&v(1, 150)), Effect::Turn { buzz: 0 });
        assert_eq!(n.apply(&v(1, 45)), Effect::Turn { buzz: 2 });
        assert_eq!(n.apply(&v(1, 20)), Effect::Turn { buzz: 0 });
        // the next step starts over
        assert_eq!(n.apply(&v(2, 180)), Effect::Turn { buzz: 1 });
    }

    #[test]
    fn stopped() {
        let mut n = Box::new(Nav::new());
        n.apply(&v(1, 450));
        n.apply(b"W");
        assert!(n.active && n.arrived);
        assert_eq!(n.apply(b"Z"), Effect::Ended);
        assert!(!n.active && !n.arrived && n.n_pts == 0);
        // a new route's first turn: on again
        assert_eq!(n.apply(&v(0, 300)), Effect::Turn { buzz: 0 });
        assert!(n.active);
    }

    #[test]
    fn instructions() {
        let mut b = [0u8; 112];
        assert_eq!(
            instruction(0, 2, "Galle Road", &mut b),
            "Turn left onto Galle Road"
        );
        assert_eq!(instruction(0, 6, "", &mut b), "Turn sharp right");
        assert_eq!(
            instruction(1, 5, "Green Path", &mut b),
            "At the roundabout, exit onto Green Path"
        );
        assert_eq!(instruction(2, 0, "Arrive", &mut b), "Arrive");
        assert_eq!(instruction(3, 0, "Continue", &mut b), "Continue");
        assert_eq!(instruction(4, 4, "A1", &mut b), "Keep right onto A1");
        assert_eq!(instruction(0, 7, "", &mut b), "Make a U-turn");
    }

    #[test]
    fn route_points() {
        let mut n = Box::new(Nav::new());
        n.apply(&[b'G', 3, 0]);
        let mut g = std::vec![b'g', 0, 0];
        for (x, y) in [(0i16, 0i16), (100, 0), (100, -50)] {
            g.extend(x.to_le_bytes());
            g.extend(y.to_le_bytes());
        }
        n.apply(&g);
        assert_eq!(n.n_pts, 3);
        assert_eq!(n.pts[2], (100, -50));
    }

    #[test]
    fn heading_up_projection() {
        let mut n = Box::new(Nav::new());
        // 1 px per metre
        let k = 256;
        n.pos = (0, 0);
        n.heading = 0;
        assert_eq!(n.project((0, 100), 233, 300, k), (233, 200)); // north is up facing north
        n.heading = 90;
        assert_eq!(n.project((100, 0), 233, 300, k), (233, 200)); // east is up facing east
        assert_eq!(n.project((0, 100), 233, 300, k), (133, 300)); // and north is to the left
    }
}
