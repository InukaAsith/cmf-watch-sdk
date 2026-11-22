//! Maps: search a place, see it, get a route (drive, bike, walk), check the road turn by turn, then navigate. The
//! phone does the work (MapKit search, OpenStreetMap or Apple routes, OpenStreetMap's streets as data); the watch
//! draws the map itself (scene.rs, render.rs).
//! Every message ≤ 128 B, little endian.
//!
//! Watch → phone:
//! - `R` ready (the phone answers `A`).
//! - `H id here x:i16 y:i16` the map (scene.rs) as scene `id`: round where we are (`here` 1: the phone may start a
//!   new frame there) or round frame point (x, y) (`here` 0: the watch moved past the scene's edge).
//! - `Q id text…` search near where the map is (else near us); the map round the results comes as scene `id`, in a
//!   new frame.
//! - `C id cat` places of a kind near there (CATEGORIES), as `Q`.
//! - `Z id text…` an area: its map as scene `id`, in a new frame with the area at 0, 0 (it is the one result).
//! - `P x:i16 y:i16 name…` a place on the map (a pin tapped, or a pin dropped: no name) becomes result `PICKED`.
//! - `I idx` what is known about result `idx` (answered by `i`).
//! - `K` the search's places cleared (✕): the phone forgets them.
//! - `F id idx` a mixed search's other group: the map round result `idx`'s group (a new frame) as scene `id`, the
//!   rows again with their places in it (`L n near`).
//! - `U idx` how long to result `idx` by car, bike and on foot (answered by three `u`).
//! - `D mode idx id [alt]` a route to result `idx` (mode 0 drive, 1 bike, 2 walk), its map as scene `id`. With two
//!   or three ways there the phone first sends `O` and the watch asks again with the one picked (`alt`).
//! - `S` start navigating the route; `X` stop.
//! - `g id` scene `id` again (part of it never came).
//!
//! Phone → watch:
//! - `A` hello. `E text…` something went wrong (shown where the answer was due).
//! - `L n [near]` n results (8 at most; `near`: a mixed search, the first `near` round us, the rest round a town
//!   far away, unplaced: x = y = -32768, till `F`), then `l idx dist:u32 x:i16 y:i16 name 0x00 sub…` each (metres from us, where it
//!   is in the frame); `l PICKED …` the place `P` asked about. Sent again in a new frame when the frame changes.
//! - `i idx k text…` line k (0 kind, 1 address, 2 phone, 3 more) about result idx; k 4 and up: its description,
//!   in pieces in order (ABOUT bytes at most).
//! - `O n`, then `o k secs:u32 metres:u32 hh mm via…` each: the ways there to pick from (n 2 or 3).
//! - `w k part unit:u16 (x:i16 y:i16)…` way k's line, `unit` frame metres a step, in parts of 30 points (part 0
//!   starts it; ALT_PTS at most).
//! - `u mode ok secs:u32 metres:u32 hh mm` the way there by `mode`; ok 0: no route that way.
//! - The map: `S`, `d` (scene.rs). After `D` the route's points come as the nav app's `G`/`g` and a scene round the
//!   route, all in the route's frame (its start at 0, 0).
//! - `T mode secs:u32 metres:u32 n:u8 hh mm` the route (n steps, the arrival time), then `t k kind dir dist:u32
//!   name…` each step k (0-based; `dist` metres from the start to it; kind/dir as the nav app's `V`).
//! - Navigating: the nav app's `V`, `G`/`g`, `X`, `W`, `r` (examples/nav/src/state.rs), `N` it started.

use cmf_wp2_ui::text::fit;

pub const MAX_RESULTS: usize = 8;
/// The place a pin on the map stands for (`P`), after the search's.
pub const PICKED: usize = MAX_RESULTS;
pub const INFO: usize = 4;
pub const MAX_ALTS: usize = 3;
/// Points of a way's line drawn on the pick map
pub const ALT_PTS: usize = 60;
/// A place's description, bytes
pub const ABOUT: usize = 480;
pub const VIA: usize = 40;
/// `C`'s kinds of places.
pub const CATEGORIES: [&str; 8] = [
    "Food", "Caf\u{e9}", "Fuel", "Hotels", "Shops", "ATM", "Pharmacy", "Hospital",
];
pub const NAME: usize = 44;
pub const SUB: usize = 44;
pub const MAX_STEPS: usize = 48;
pub const STEP_NAME: usize = 36;

pub const MODES: [&str; 3] = ["Drive", "Bike", "Walk"];

/// One way to a place, as `u` said.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Way {
    Asking,
    None,
    Ok {
        secs: u32,
        metres: u32,
        arrive: (u8, u8),
    },
}

fn copy(dst: &mut [u8], src: &[u8]) -> u8 {
    let t = fit(src, dst.len()).as_bytes();
    dst[..t.len()].copy_from_slice(t);
    t.len() as u8
}

fn u16_at(m: &[u8], i: usize) -> u16 {
    u16::from_le_bytes([m[i], m[i + 1]])
}

fn u32_at(m: &[u8], i: usize) -> u32 {
    u32::from_le_bytes([m[i], m[i + 1], m[i + 2], m[i + 3]])
}

#[derive(Clone, Copy)]
pub struct Place {
    pub dist: u32,
    /// frame metres
    pub at: (i32, i32),
    name: [u8; NAME],
    n_name: u8,
    sub: [u8; SUB],
    n_sub: u8,
}

/// A result's position when it is not on this map (the far group of a mixed search: CMFX AtlasProto).
pub const UNPLACED: i32 = -32768;

impl Place {
    /// On the map now (else in the other group: `F` brings the map there).
    pub fn placed(&self) -> bool {
        self.at.0 != UNPLACED
    }

    pub const EMPTY: Place = Place {
        dist: 0,
        at: (0, 0),
        name: [0; NAME],
        n_name: 0,
        sub: [0; SUB],
        n_sub: 0,
    };

    pub fn new(name: &str, at: (i32, i32)) -> Place {
        let mut p = Place::EMPTY;
        p.at = at;
        p.n_name = copy(&mut p.name, name.as_bytes());
        p
    }

    pub fn name(&self) -> &str {
        fit(&self.name[..self.n_name as usize], NAME)
    }

    pub fn sub(&self) -> &str {
        fit(&self.sub[..self.n_sub as usize], SUB)
    }
}

#[derive(Clone, Copy)]
pub struct Step {
    pub kind: u8,
    pub dir: u8,
    pub at: u32,
    name: [u8; STEP_NAME],
    n_name: u8,
}

impl Step {
    pub const EMPTY: Step = Step {
        kind: 0,
        dir: 0,
        at: 0,
        name: [0; STEP_NAME],
        n_name: 0,
    };

    pub fn name(&self) -> &str {
        fit(&self.name[..self.n_name as usize], STEP_NAME)
    }
}

/// A way there to pick (`o`).
#[derive(Clone, Copy)]
pub struct Alt {
    pub secs: u32,
    pub metres: u32,
    pub arrive: (u8, u8),
    via: [u8; VIA],
    n_via: u8,
}

impl Alt {
    pub const EMPTY: Alt = Alt {
        secs: 0,
        metres: 0,
        arrive: (0, 0),
        via: [0; VIA],
        n_via: 0,
    };

    pub fn via(&self) -> &str {
        fit(&self.via[..self.n_via as usize], VIA)
    }
}

/// What the phone said about places and routes.
pub struct Model {
    pub n_results: usize,
    /// a mixed search: results before `near` are round us, the rest (from the town found far away, row `near`)
    /// round it; 0: one group
    pub near: usize,
    /// the search's, then PICKED
    pub results: [Place; MAX_RESULTS + 1],
    /// `i` lines about this result
    pub info_for: usize,
    info: [[u8; SUB]; INFO],
    n_info: [u8; INFO],
    pub n_alts: usize,
    pub alts: [Alt; MAX_ALTS],
    /// the ways' lines (frame metres / alt_unit)
    pub alt_pts: [[(i16, i16); ALT_PTS]; MAX_ALTS],
    pub n_alt_pts: [usize; MAX_ALTS],
    pub alt_unit: i32,
    pub about: [u8; ABOUT],
    pub n_about: usize,
    /// a `L` came for the last search
    pub searched: bool,
    pub mode: u8,
    pub secs: u32,
    pub metres: u32,
    pub arrive: (u8, u8),
    pub n_steps: usize,
    pub steps: [Step; MAX_STEPS],
    pub routed: bool,
    /// car, bike, foot to the open place
    pub ways: [Way; 3],
    pub error: [u8; 60],
    pub n_error: usize,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Got {
    None,
    Hello,
    Results,
    Route,
    Ways,
    Info,
    /// a piece of the open place's description
    About,
    Alts,
    Error,
    Nav,
}

impl Model {
    pub const fn new() -> Self {
        Model {
            n_results: 0,
            near: 0,
            results: [Place::EMPTY; MAX_RESULTS + 1],
            info_for: usize::MAX,
            info: [[0; SUB]; INFO],
            n_info: [0; INFO],
            n_alts: 0,
            alts: [Alt::EMPTY; MAX_ALTS],
            alt_pts: [[(0, 0); ALT_PTS]; MAX_ALTS],
            n_alt_pts: [0; MAX_ALTS],
            alt_unit: 1,
            about: [0; ABOUT],
            n_about: 0,
            searched: false,
            mode: 0,
            secs: 0,
            metres: 0,
            arrive: (0, 0),
            n_steps: 0,
            steps: [Step::EMPTY; MAX_STEPS],
            routed: false,
            ways: [Way::Asking; 3],
            error: [0; 60],
            n_error: 0,
        }
    }

    pub fn error(&self) -> &str {
        fit(&self.error[..self.n_error], 60)
    }

    /// The open place's description ("" if none came), whole characters only.
    pub fn about(&self) -> &str {
        let mut n = self.n_about;
        while n > 0 && core::str::from_utf8(&self.about[..n]).is_err() {
            n -= 1;
        }
        core::str::from_utf8(&self.about[..n]).unwrap_or("")
    }

    /// Line k about the open place ("" till it comes).
    pub fn info(&self, k: usize) -> &str {
        fit(&self.info[k][..self.n_info[k] as usize], SUB)
    }

    /// A place opened: what was known of the last one goes.
    pub fn open(&mut self, idx: usize) {
        self.info_for = idx;
        self.n_info = [0; INFO];
        self.n_about = 0;
        self.ways = [Way::Asking; 3];
        self.n_error = 0;
    }

    pub fn apply(&mut self, m: &[u8]) -> Got {
        match m.first() {
            Some(b'A') => Got::Hello,
            Some(b'N') => Got::Nav,
            Some(b'E') => {
                self.n_error = copy(&mut self.error, &m[1..]) as usize;
                Got::Error
            }
            Some(b'L') if m.len() == 2 || m.len() == 3 => {
                self.n_results = (m[1] as usize).min(MAX_RESULTS);
                self.near = if m.len() == 3 { usize::from(m[2]).min(self.n_results) } else { 0 };
                self.results[..MAX_RESULTS].fill(Place::EMPTY);
                self.searched = true;
                self.n_error = 0;
                Got::Results
            }
            Some(b'l') if m.len() >= 10 && (m[1] as usize) <= PICKED => {
                let p = &mut self.results[m[1] as usize];
                p.dist = u32_at(m, 2);
                let i16_at = |i: usize| i32::from(u16_at(m, i) as i16);
                p.at = (i16_at(6), i16_at(8));
                let rest = &m[10..];
                let cut = rest.iter().position(|&b| b == 0).unwrap_or(rest.len());
                p.n_name = copy(&mut p.name, &rest[..cut]);
                p.n_sub = copy(&mut p.sub, rest.get(cut + 1..).unwrap_or(&[]));
                Got::Results
            }
            Some(b'i') if m.len() >= 3 && m[1] as usize == self.info_for && (m[2] as usize) < INFO => {
                let k = m[2] as usize;
                self.n_info[k] = copy(&mut self.info[k], &m[3..]);
                Got::Info
            }
            Some(b'i') if m.len() >= 3 && m[1] as usize == self.info_for && m[2] as usize >= INFO => {
                let n = (m.len() - 3).min(ABOUT - self.n_about);
                self.about[self.n_about..self.n_about + n].copy_from_slice(&m[3..3 + n]);
                self.n_about += n;
                Got::About
            }
            Some(b'O') if m.len() == 2 => {
                self.n_alts = (m[1] as usize).min(MAX_ALTS);
                self.alts = [Alt::EMPTY; MAX_ALTS];
                self.n_alt_pts = [0; MAX_ALTS];
                Got::Alts
            }
            Some(b'w') if m.len() >= 5 && (m[1] as usize) < MAX_ALTS => {
                let k = m[1] as usize;
                if m[2] == 0 {
                    self.n_alt_pts[k] = 0;
                }
                self.alt_unit = i32::from(u16::from_le_bytes([m[3], m[4]])).max(1);
                for p in m[5..].chunks_exact(4) {
                    if self.n_alt_pts[k] < ALT_PTS {
                        let at = (i16::from_le_bytes([p[0], p[1]]), i16::from_le_bytes([p[2], p[3]]));
                        self.alt_pts[k][self.n_alt_pts[k]] = at;
                        self.n_alt_pts[k] += 1;
                    }
                }
                Got::Alts
            }
            Some(b'o') if m.len() >= 13 && (m[1] as usize) < MAX_ALTS => {
                let a = &mut self.alts[m[1] as usize];
                (a.secs, a.metres, a.arrive) = (u32_at(m, 2), u32_at(m, 6), (m[10], m[11]));
                a.n_via = copy(&mut a.via, &m[12..]);
                Got::Alts
            }
            Some(b'u') if m.len() == 13 && (m[1] as usize) < 3 => {
                self.ways[m[1] as usize] = if m[2] == 0 {
                    Way::None
                } else {
                    Way::Ok {
                        secs: u32_at(m, 3),
                        metres: u32_at(m, 7),
                        arrive: (m[11], m[12]),
                    }
                };
                Got::Ways
            }
            Some(b'T') if m.len() == 13 => {
                self.mode = m[1];
                self.secs = u32_at(m, 2);
                self.metres = u32_at(m, 6);
                self.n_steps = (m[10] as usize).min(MAX_STEPS);
                self.arrive = (m[11], m[12]);
                self.steps = [Step::EMPTY; MAX_STEPS];
                self.routed = true;
                self.n_error = 0;
                Got::Route
            }
            Some(b't') if m.len() >= 8 && (m[1] as usize) < MAX_STEPS => {
                let s = &mut self.steps[m[1] as usize];
                (s.kind, s.dir, s.at) = (m[2], m[3], u32_at(m, 4));
                s.n_name = copy(&mut s.name, &m[8..]);
                Got::Route
            }
            _ => Got::None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    extern crate std;
    use std::boxed::Box;

    #[test]
    fn results_and_route() {
        let mut m = Box::new(Model::new());
        assert_eq!(m.apply(&[b'L', 2]), Got::Results);
        let mut l = std::vec![b'l', 1];
        l.extend(1234u32.to_le_bytes());
        l.extend((-300i16).to_le_bytes());
        l.extend(450i16.to_le_bytes());
        l.extend(b"Galle Face\x00Colombo 3");
        m.apply(&l);
        assert_eq!((m.n_results, m.results[1].dist, m.results[1].at), (2, 1234, (-300, 450)));
        assert_eq!(
            (m.results[1].name(), m.results[1].sub()),
            ("Galle Face", "Colombo 3")
        );
        let mut t = std::vec![b'T', 2];
        t.extend(600u32.to_le_bytes());
        t.extend(800u32.to_le_bytes());
        t.extend([3, 14, 5]);
        assert_eq!(m.apply(&t), Got::Route);
        let mut s = std::vec![b't', 0, 0, 2];
        s.extend(120u32.to_le_bytes());
        s.extend(b"Galle Rd");
        m.apply(&s);
        assert_eq!(
            (m.n_steps, m.steps[0].at, m.steps[0].name(), m.arrive),
            (3, 120, "Galle Rd", (14, 5))
        );
    }

    #[test]
    fn info_and_alts() {
        let mut m = Box::new(Model::new());
        m.open(PICKED);
        assert_eq!(m.apply(b"i\x08\x01Galle Rd, Colombo"), Got::Info);
        assert_eq!(m.apply(b"i\x02\x00Caf\xc3\xa9"), Got::None);
        assert_eq!((m.info(0), m.info(1)), ("", "Galle Rd, Colombo"));
        assert_eq!(m.apply(&[b'O', 2]), Got::Alts);
        let mut o = std::vec![b'o', 1];
        o.extend(900u32.to_le_bytes());
        o.extend(5000u32.to_le_bytes());
        o.extend([10, 30]);
        o.extend(b"Duplication Rd");
        m.apply(&o);
        assert_eq!((m.n_alts, m.alts[1].secs, m.alts[1].via()), (2, 900, "Duplication Rd"));
        let mut l = std::vec![b'l', PICKED as u8];
        l.extend(10u32.to_le_bytes());
        l.extend([0, 0, 0, 0]);
        l.extend(b"Dropped pin\x00Galle Rd");
        m.apply(&l);
        assert_eq!(m.results[PICKED].sub(), "Galle Rd");
    }

    #[test]
    fn ways() {
        let mut m = Box::new(Model::new());
        let mut u = std::vec![b'u', 2, 1];
        u.extend(240u32.to_le_bytes());
        u.extend(300u32.to_le_bytes());
        u.extend([9, 34]);
        assert_eq!(m.apply(&u), Got::Ways);
        assert_eq!(
            m.ways[2],
            Way::Ok {
                secs: 240,
                metres: 300,
                arrive: (9, 34)
            }
        );
        u[1] = 1;
        u[2] = 0;
        m.apply(&u);
        assert_eq!((m.ways[0], m.ways[1]), (Way::Asking, Way::None));
    }
}
