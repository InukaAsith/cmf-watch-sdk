//! Markers: places you keep (a car, a tent, a trailhead), 24 at most, in the app's 4 KiB save as latitude and
//! longitude (they stay right when the map is built again round somewhere else). The only thing Offline Maps keeps
//! besides the map itself; the track and a route live in RAM and go when the app closes.
//!
//! Save: `OM1`, n, then per marker `lat:i32 lon:i32 len:u8 name[NAME]`.

pub const MAX: usize = 24;
pub const NAME: usize = 14;
const REC: usize = 9 + NAME;
pub const BYTES: usize = 4 + MAX * REC;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Mark {
    pub lat_e7: i32,
    pub lon_e7: i32,
    pub name: [u8; NAME],
    pub len: u8,
}

impl Mark {
    pub fn name(&self) -> &str {
        core::str::from_utf8(&self.name[..usize::from(self.len)]).unwrap_or("")
    }
}

pub struct Marks {
    pub v: [Mark; MAX],
    pub n: usize,
    /// the next "Pin N"
    pub next: u16,
}

const EMPTY: Mark = Mark { lat_e7: 0, lon_e7: 0, name: [0; NAME], len: 0 };

impl Marks {
    pub const fn new() -> Self {
        Marks { v: [EMPTY; MAX], n: 0, next: 1 }
    }

    pub fn parse(&mut self, b: &[u8]) {
        self.n = 0;
        if b.len() < 4 || &b[..3] != b"OM1" {
            return;
        }
        let n = usize::from(b[3]).min(MAX);
        for i in 0..n {
            let at = 4 + i * REC;
            if b.len() < at + REC {
                break;
            }
            let r = &b[at..at + REC];
            let mut m = EMPTY;
            m.lat_e7 = i32::from_le_bytes([r[0], r[1], r[2], r[3]]);
            m.lon_e7 = i32::from_le_bytes([r[4], r[5], r[6], r[7]]);
            m.len = r[8].min(NAME as u8);
            m.name.copy_from_slice(&r[9..9 + NAME]);
            self.v[self.n] = m;
            self.n += 1;
        }
        self.next = self.n as u16 + 1;
    }

    pub fn bytes(&self, out: &mut [u8; BYTES]) -> usize {
        out[..3].copy_from_slice(b"OM1");
        out[3] = self.n as u8;
        for (i, m) in self.v[..self.n].iter().enumerate() {
            let at = 4 + i * REC;
            out[at..at + 4].copy_from_slice(&m.lat_e7.to_le_bytes());
            out[at + 4..at + 8].copy_from_slice(&m.lon_e7.to_le_bytes());
            out[at + 8] = m.len;
            out[at + 9..at + 9 + NAME].copy_from_slice(&m.name);
        }
        4 + self.n * REC
    }

    /// A new marker "Pin N"; false when full.
    pub fn add(&mut self, lat_e7: i32, lon_e7: i32) -> bool {
        if self.n == MAX {
            return false;
        }
        let mut m = EMPTY;
        m.lat_e7 = lat_e7;
        m.lon_e7 = lon_e7;
        let mut b = [0u8; 11];
        let num = cmf_wp2_ui::text::int(i32::from(self.next), &mut b);
        let mut k = 0;
        for part in ["Pin ", num] {
            for &c in part.as_bytes() {
                if k < NAME {
                    m.name[k] = c;
                    k += 1;
                }
            }
        }
        m.len = k as u8;
        self.v[self.n] = m;
        self.n += 1;
        self.next += 1;
        true
    }

    pub fn remove(&mut self, i: usize) {
        if i < self.n {
            self.v.copy_within(i + 1..self.n, i);
            self.n -= 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_save_load_remove() {
        let mut m = Marks::new();
        assert!(m.add(69_271_000, 798_612_000));
        assert!(m.add(-1, 2));
        assert_eq!(m.v[1].name(), "Pin 2");
        let mut b = [0u8; BYTES];
        let n = m.bytes(&mut b);
        let mut back = Marks::new();
        back.parse(&b[..n]);
        assert_eq!(back.n, 2);
        assert_eq!(back.v[0], m.v[0]);
        assert_eq!(back.next, 3);
        back.remove(0);
        assert_eq!((back.n, back.v[0].lat_e7), (1, -1));
        for _ in 0..MAX {
            back.add(0, 0);
        }
        assert_eq!(back.n, MAX);
        assert!(!back.add(0, 0));
        assert!(BYTES <= 4096);
    }
}
