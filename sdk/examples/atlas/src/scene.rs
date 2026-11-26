//! The map as data, drawn by the watch (render.rs): areas, roads, names and labels in metres east (x) and north (y)
//! of the phone's frame origin, as i16 (±32 km). The phone cuts it from OpenStreetMap vector tiles round a point and
//! sends it as one blob:
//!
//! - `S id total:u16 me_x:i16 me_y:i16 cx:i16 cy:i16 r:u16` a new scene of `total` bytes: where we are, and the
//!   square it covers (centre, half its side), all in metres.
//! - `d id seq:u16 bytes…` its bytes at seq × 124, in any order, maybe twice (as the old pictures).
//!
//! The blob is records, one after another, each drawn as soon as it and all before it have come:
//!
//! - `0x10 | c, n:u16, pts` an area (ring, closed, 512 points at most), c: 0 water, 1 park or green.
//! - `0x20 | c, name:u8, n:u8, pts` a line, c: 0 motorway or trunk, 1 primary, 2 secondary, 3 tertiary, 4 minor
//!   street, 5 path, 6 railway, 7 waterway; `name` an index into the names (0xFF none). With 8 added to c the line
//!   is not drawn: it only carries its street's name (OpenStreetMap's street names come as lines of their own).
//! - `0x30, len:u8, text` the next name (indices count from 0 in the order they come).
//! - `0x40 | c, x:i16, y:i16, rank:u8, len:u8, text` a label, c: 0 place (suburb, village …), 1 place of interest
//!   (a pin), 2 water, 3 … 13 a place of interest of a kind (`poi_kind`: its pin's colour; 13 searched). Lower `rank` shows
//!   first.
//!
//! `pts`: the first point `x:i16 y:i16`, then each next one as `dx:i8 dy:i8` from the one before, or, when either
//! is beyond ±127, `0x80` then `dx:i16 dy:i16`.

/// The scene's buffer: each app that draws scenes says how big (Maps 26 KB; Offline Maps' cells are kept under
/// 16 KB by the phone).
pub const CAP: usize = crate::SCENE_CAP;
pub const CHUNK: usize = 124;
const SEQS: usize = CAP.div_ceil(CHUNK);
pub const MAX_NAMES: usize = 255;

pub const AREA: u8 = 0x10;
pub const LINE: u8 = 0x20;
pub const NAME: u8 = 0x30;
pub const LABEL: u8 = 0x40;

pub const WATER: u8 = 0;
pub const PARK: u8 = 1;
pub const MOTORWAY: u8 = 0;
pub const PRIMARY: u8 = 1;
pub const SECONDARY: u8 = 2;
pub const TERTIARY: u8 = 3;
pub const MINOR: u8 = 4;
pub const PATH: u8 = 5;
pub const RAIL: u8 = 6;
pub const WATERWAY: u8 = 7;
/// Added to a line's class: it carries a name and isn't drawn.
pub const CARRIER: u8 = 8;
pub const PLACE: u8 = 0;
pub const POI: u8 = 1;
pub const WATER_NAME: u8 = 2;
/// Label classes 3 … 12: places of interest by kind (render.rs colours their pins): food, shop, green, transit,
/// health, learning and worship, stay, culture, money, car; 13 a place the last search found (a red pin). POI (1)
/// is any other.
pub const POI_KINDS: u8 = 3;

/// A label class as a kind of place of interest (0 other, 1 … 10 as POI_KINDS), or None: not one.
pub fn poi_kind(class: u8) -> Option<u8> {
    match class {
        POI => Some(0),
        c if c >= POI_KINDS => Some(c - POI_KINDS + 1),
        _ => None,
    }
}

fn i16_at(b: &[u8], i: usize) -> i16 {
    i16::from_le_bytes([b[i], b[i + 1]])
}

/// The points of an area or a line.
#[derive(Clone)]
pub struct Pts<'a> {
    b: &'a [u8],
    left: usize,
    at: (i32, i32),
    first: bool,
}

impl Iterator for Pts<'_> {
    type Item = (i32, i32);
    fn next(&mut self) -> Option<(i32, i32)> {
        if self.left == 0 {
            return None;
        }
        self.left -= 1;
        if self.first {
            self.first = false;
            self.at = (i32::from(i16_at(self.b, 0)), i32::from(i16_at(self.b, 2)));
            self.b = &self.b[4..];
        } else if self.b[0] == 0x80 {
            self.at.0 += i32::from(i16_at(self.b, 1));
            self.at.1 += i32::from(i16_at(self.b, 3));
            self.b = &self.b[5..];
        } else {
            self.at.0 += i32::from(self.b[0] as i8);
            self.at.1 += i32::from(self.b[1] as i8);
            self.b = &self.b[2..];
        }
        Some(self.at)
    }
}

/// How many bytes `n` points take at the start of `b`; None if they are not all there.
fn pts_len(b: &[u8], n: usize) -> Option<usize> {
    if n == 0 {
        return Some(0);
    }
    let mut i = 4;
    for _ in 1..n {
        i += if *b.get(i)? == 0x80 { 5 } else { 2 };
    }
    (i <= b.len()).then_some(i)
}

pub enum Rec<'a> {
    Area {
        class: u8,
        pts: Pts<'a>,
    },
    Line {
        class: u8,
        name: u8,
        pts: Pts<'a>,
    },
    Name(&'a str),
    Label {
        class: u8,
        x: i32,
        y: i32,
        rank: u8,
        text: &'a str,
    },
}

/// The records in `b`, as far as they are whole.
pub struct Recs<'a> {
    b: &'a [u8],
}

fn text(b: &[u8]) -> &str {
    match core::str::from_utf8(b) {
        Ok(s) => s,
        Err(e) => core::str::from_utf8(&b[..e.valid_up_to()]).unwrap_or(""),
    }
}

impl<'a> Iterator for Recs<'a> {
    type Item = Rec<'a>;
    fn next(&mut self) -> Option<Rec<'a>> {
        let b = self.b;
        let t = *b.first()?;
        let class = t & 15;
        let (rec, used) = match t & 0xF0 {
            AREA => {
                let n = usize::from(u16::from_le_bytes([*b.get(1)?, *b.get(2)?]));
                let len = pts_len(&b[3..], n)?;
                (
                    Rec::Area {
                        class,
                        pts: Pts {
                            b: &b[3..3 + len],
                            left: n,
                            at: (0, 0),
                            first: true,
                        },
                    },
                    3 + len,
                )
            }
            LINE => {
                let (name, n) = (*b.get(1)?, usize::from(*b.get(2)?));
                let len = pts_len(&b[3..], n)?;
                (
                    Rec::Line {
                        class,
                        name,
                        pts: Pts {
                            b: &b[3..3 + len],
                            left: n,
                            at: (0, 0),
                            first: true,
                        },
                    },
                    3 + len,
                )
            }
            NAME => {
                let n = usize::from(*b.get(1)?);
                (Rec::Name(text(b.get(2..2 + n)?)), 2 + n)
            }
            LABEL => {
                let n = usize::from(*b.get(6)?);
                let s = text(b.get(7..7 + n)?);
                let (x, y) = (i32::from(i16_at(b, 1)), i32::from(i16_at(b, 3)));
                (
                    Rec::Label {
                        class,
                        x,
                        y,
                        rank: b[5],
                        text: s,
                    },
                    7 + n,
                )
            }
            _ => {
                // not ours: the rest can't be read
                self.b = &[];
                return None;
            }
        };
        self.b = &b[used..];
        Some(rec)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Ev {
    None,
    /// a new scene started
    Begin,
    /// more of it is whole
    More,
    /// all of it came
    Done,
}


/// The scene being received and drawn.
pub struct Scene {
    pub id: u8,
    pub total: usize,
    /// where we are, metres in the frame
    pub me: (i32, i32),
    /// the square it covers: centre, half the side
    pub centre: (i32, i32),
    pub r: i32,
    got: [u8; SEQS.div_ceil(8)],
    data: [u8; CAP],
    /// the bytes from 0 that have all come
    pub avail: usize,
    /// names seen in `data[..scanned]`: their offsets (the length byte) in `data`
    names: [u16; MAX_NAMES],
    pub n_names: usize,
    scanned: usize,
    pub active: bool,
    pub done: bool,
    /// the id the app waits for (others are dropped)
    pub want: u8,
}

impl Scene {
    pub const fn new() -> Self {
        Scene {
            id: 0,
            total: 0,
            me: (0, 0),
            centre: (0, 0),
            r: 0,
            got: [0; SEQS.div_ceil(8)],
            data: [0; CAP],
            avail: 0,
            names: [0; MAX_NAMES],
            n_names: 0,
            scanned: 0,
            active: false,
            done: false,
            want: 0,
        }
    }

    pub fn clear(&mut self) {
        self.active = false;
        self.done = false;
        self.avail = 0;
        self.total = 0;
        self.n_names = 0;
        self.scanned = 0;
        self.got = [0; SEQS.div_ceil(8)];
    }

    /// A whole scene from a file (Offline Maps' cells): `fill` puts its bytes in and says how many.
    #[allow(dead_code)]
    pub fn load(&mut self, fill: impl FnOnce(&mut [u8]) -> usize) {
        self.clear();
        let n = fill(&mut self.data).min(CAP);
        (self.total, self.avail, self.active, self.done) = (n, n, true, true);
        self.scan();
    }

    /// 0 … 256 of it here.
    pub fn progress(&self) -> u32 {
        if self.total == 0 {
            0
        } else {
            (self.avail * 256 / self.total) as u32
        }
    }

    fn has(&self, seq: usize) -> bool {
        self.got[seq / 8] & (1 << (seq % 8)) != 0
    }

    pub fn message(&mut self, m: &[u8]) -> Ev {
        match m.first() {
            Some(b'S') if m.len() == 14 => {
                if m[1] != self.want {
                    return Ev::None;
                }
                self.clear();
                self.id = m[1];
                self.total = usize::from(u16::from_le_bytes([m[2], m[3]])).min(CAP);
                self.me = (i32::from(i16_at(m, 4)), i32::from(i16_at(m, 6)));
                self.centre = (i32::from(i16_at(m, 8)), i32::from(i16_at(m, 10)));
                self.r = i32::from(u16::from_le_bytes([m[12], m[13]]));
                self.active = true;
                self.done = self.total == 0;
                if self.done { Ev::Done } else { Ev::Begin }
            }
            Some(b'd') if m.len() > 4 && self.active && m[1] == self.id && !self.done => {
                let seq = usize::from(u16::from_le_bytes([m[2], m[3]]));
                let at = seq * CHUNK;
                if seq >= SEQS || at >= self.total || self.has(seq) {
                    return Ev::None;
                }
                let n = (m.len() - 4).min(self.total - at);
                self.data[at..at + n].copy_from_slice(&m[4..4 + n]);
                self.got[seq / 8] |= 1 << (seq % 8);
                let before = self.avail;
                while self.avail < self.total && self.has(self.avail / CHUNK) {
                    self.avail = ((self.avail / CHUNK + 1) * CHUNK).min(self.total);
                }
                if self.avail == self.total {
                    self.done = true;
                }
                self.scan();
                if self.done {
                    Ev::Done
                } else if self.avail > before {
                    Ev::More
                } else {
                    Ev::None
                }
            }
            _ => Ev::None,
        }
    }

    /// The names in what has come, so lines can find theirs.
    fn scan(&mut self) {
        let mut at = self.scanned;
        let mut recs = Recs {
            b: &self.data[at..self.avail],
        };
        loop {
            let before = recs.b.len();
            match recs.next() {
                Some(Rec::Name(_)) => {
                    if self.n_names < MAX_NAMES {
                        self.names[self.n_names] = (at + 1) as u16;
                        self.n_names += 1;
                    }
                }
                Some(_) => {}
                None => break,
            }
            at += before - recs.b.len();
        }
        self.scanned = at;
    }

    pub fn recs(&self) -> Recs<'_> {
        Recs {
            b: &self.data[..if self.active { self.scanned } else { 0 }],
        }
    }

    pub fn name(&self, i: u8) -> Option<&str> {
        let off = usize::from(*self.names[..self.n_names].get(usize::from(i))?);
        let n = usize::from(self.data[off]);
        Some(text(&self.data[off + 1..off + 1 + n]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    extern crate std;
    use std::boxed::Box;
    use std::vec::Vec;

    /// The encoder, as the phone's (HubKit AtlasScene) and the test host's.
    fn pts(p: &[(i32, i32)]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend((p[0].0 as i16).to_le_bytes());
        out.extend((p[0].1 as i16).to_le_bytes());
        for w in p.windows(2) {
            let (dx, dy) = (w[1].0 - w[0].0, w[1].1 - w[0].1);
            if (-127..=127).contains(&dx) && (-127..=127).contains(&dy) {
                out.extend([dx as i8 as u8, dy as i8 as u8]);
            } else {
                out.push(0x80);
                out.extend((dx as i16).to_le_bytes());
                out.extend((dy as i16).to_le_bytes());
            }
        }
        out
    }

    fn blob() -> Vec<u8> {
        let mut b = Vec::new();
        let water = [(0, 0), (100, 0), (100, 300), (0, 300)];
        b.push(AREA | WATER);
        b.extend((water.len() as u16).to_le_bytes());
        b.extend(pts(&water));
        b.extend([NAME, 10]);
        b.extend(b"Galle Road");
        let road = [(-500, -20), (-480, -10), (900, 40), (905, 45)];
        b.extend([LINE | PRIMARY, 0, road.len() as u8]);
        b.extend(pts(&road));
        b.push(LABEL | POI);
        b.extend(120i16.to_le_bytes());
        b.extend((-40i16).to_le_bytes());
        b.extend([3, 6]);
        b.extend(b"Cafe 1");
        b
    }

    fn feed(s: &mut Scene, b: &[u8], order: &[usize]) -> Vec<Ev> {
        s.want = 5;
        let mut h = std::vec![b'S', 5];
        h.extend((b.len() as u16).to_le_bytes());
        h.extend(7i16.to_le_bytes());
        h.extend((-3i16).to_le_bytes());
        h.extend([0, 0, 0, 0]);
        h.extend(1500u16.to_le_bytes());
        let mut evs = std::vec![s.message(&h)];
        let chunks: Vec<&[u8]> = b.chunks(CHUNK).collect();
        for &k in order {
            let mut d = std::vec![b'd', 5];
            d.extend((k as u16).to_le_bytes());
            d.extend(chunks[k]);
            evs.push(s.message(&d));
        }
        evs
    }

    #[test]
    fn records_and_points() {
        let mut s = Box::new(Scene::new());
        let b = blob();
        assert_eq!(feed(&mut s, &b, &[0]), [Ev::Begin, Ev::Done]);
        assert_eq!((s.me, s.r), ((7, -3), 1500));
        let recs: Vec<Rec> = s.recs().collect();
        assert_eq!(recs.len(), 4);
        match &recs[0] {
            Rec::Area { class, pts } => {
                assert_eq!(*class, WATER);
                assert_eq!(
                    pts.clone().collect::<Vec<_>>(),
                    [(0, 0), (100, 0), (100, 300), (0, 300)]
                );
            }
            _ => panic!("area"),
        }
        match &recs[2] {
            Rec::Line { class, name, pts } => {
                assert_eq!((*class, s.name(*name)), (PRIMARY, Some("Galle Road")));
                assert_eq!(
                    pts.clone().collect::<Vec<_>>(),
                    [(-500, -20), (-480, -10), (900, 40), (905, 45)]
                );
            }
            _ => panic!("line"),
        }
        match recs[3] {
            Rec::Label {
                class,
                x,
                y,
                rank,
                text,
            } => assert_eq!((class, x, y, rank, text), (POI, 120, -40, 3, "Cafe 1")),
            _ => panic!("label"),
        }
    }

    #[test]
    fn out_of_order_and_twice() {
        // a long scene: many roads, chunks backwards and repeated
        let mut b = Vec::new();
        for k in 0..60 {
            let road: Vec<(i32, i32)> = (0..8).map(|i| (k * 50 + i * 30, i * 200 - k)).collect();
            b.extend([LINE | MINOR, 0xFF, road.len() as u8]);
            b.extend(pts(&road));
        }
        let n = b.len().div_ceil(CHUNK);
        let mut s = Box::new(Scene::new());
        let mut order: Vec<usize> = (0..n).rev().collect();
        order.insert(3, n - 1);
        let evs = feed(&mut s, &b, &order);
        assert_eq!(
            evs[1..evs.len() - 1]
                .iter()
                .filter(|e| **e == Ev::More)
                .count(),
            0,
            "nothing whole till chunk 0"
        );
        assert_eq!(*evs.last().unwrap(), Ev::Done);
        assert_eq!(s.recs().count(), 60);
        // in order: each chunk makes more whole, the records drawn so far are whole ones
        let mut s = Box::new(Scene::new());
        let evs = feed(&mut s, &b, &(0..n).collect::<Vec<_>>());
        assert!(evs[1..n].iter().all(|e| *e == Ev::More));
        assert_eq!(*evs.last().unwrap(), Ev::Done);
    }

    #[test]
    fn other_ids_dropped() {
        let mut s = Box::new(Scene::new());
        s.want = 9;
        assert_eq!(
            s.message(&[b'S', 5, 10, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]),
            Ev::None
        );
        assert!(!s.active);
    }
}
