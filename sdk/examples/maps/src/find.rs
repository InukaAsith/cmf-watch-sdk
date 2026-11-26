//! Offline Maps: finding places and planning a route.
//!
//! - Names: the map's place names (towns, places of interest, water, streets) in the file `n` the phone saves with
//!   the map (`WMN1`, then `kind:u8 x:i16 y:i16 len:u8 text` each; kind 0 place, 1 and 3 … 12 places of
//!   interest (scene.rs), 2 water, 20 a street). Search reads it with no phone; a tap on the map finds the name
//!   drawn there.
//! - Search: a QWERTY keyboard (Maps'), then the places that match nearest first (Results, crown or drag scroll).
//! - Directions and turn by turn: trip.rs.

use super::*;

/// A searched name.
#[derive(Clone, Copy)]
pub struct Hit {
    pub p: (i32, i32),
    pub kind: u8,
    pub name: [u8; 32],
    pub n: u8,
}

impl Hit {
    pub const NONE: Hit = Hit { p: (0, 0), kind: 0, name: [0; 32], n: 0 };
    pub fn name(&self) -> &str {
        cmf_wp2_ui::text::fit(&self.name[..usize::from(self.n)], 32)
    }
}

pub const HITS: usize = 12;
pub const QUERY: usize = 24;
pub const STREET: u8 = 20;
const KH: i32 = 58;
const KEY_Y: [i32; 4] = [184, 242, 300, 358];
const KEYC: Rgb = Rgb(0x2A, 0x2A, 0x2A);

#[derive(Clone, Copy, PartialEq)]
pub enum K {
    Ch(u8),
    Del,
    Sym,
    Space,
    Go,
}

fn row(r: usize, sym: bool) -> ([(K, i32); 10], usize) {
    let mut keys = [(K::Space, 0); 10];
    let chars = |s: &[u8], keys: &mut [(K, i32); 10]| {
        for (i, &c) in s.iter().enumerate() {
            keys[i] = (K::Ch(c), 44);
        }
        s.len()
    };
    let n = match (r, sym) {
        (0, false) => chars(b"qwertyuiop", &mut keys),
        (1, false) => chars(b"asdfghjkl", &mut keys),
        (0, true) => chars(b"1234567890", &mut keys),
        (1, true) => chars(b"&-'.,!?()", &mut keys),
        (2, _) => {
            let n = chars(if sym { b"#+/:\"@*" } else { b"zxcvbnm" }, &mut keys);
            keys[n] = (K::Del, 58);
            n + 1
        }
        _ => {
            keys[0] = (K::Sym, 70);
            keys[1] = (K::Space, 150);
            keys[2] = (K::Go, 76);
            3
        }
    };
    (keys, n)
}

fn each_key(sym: bool, mut f: impl FnMut(K, i32, i32, i32)) {
    for (r, &y) in KEY_Y.iter().enumerate() {
        let (keys, n) = row(r, sym);
        let total: i32 = keys[..n].iter().map(|k| k.1).sum();
        let mut x = C - total / 2;
        for &(k, w) in &keys[..n] {
            f(k, x, y, w);
            x += w;
        }
    }
}

/// What a kind of name is called on its card.
pub fn kind_name(k: u8) -> &'static str {
    const POI: [&str; 10] = ["Food", "Shop", "Park", "Transit", "Health", "Public place", "Stay", "Culture", "Money", "Car"];
    match k {
        0 => "Place",
        2 => "Water",
        STREET => "Street",
        3..=12 => POI[usize::from(k - 3)],
        _ => "Place of interest",
    }
}

fn kind_colour(k: u8) -> Rgb {
    match k {
        1 | 3..=12 => render::KINDS[usize::from(if k == 1 { 0 } else { k - 2 })],
        2 => Rgb(0x51, 0x7A, 0xB8),
        STREET => Rgb(0x9C, 0xA5, 0xB3),
        _ => Rgb(0xD5, 0x95, 0x63),
    }
}

fn lower(c: u8) -> u8 {
    c.to_ascii_lowercase()
}

/// `q` (lower case) in `t`, ASCII letters in any case.
fn contains(t: &[u8], q: &[u8]) -> bool {
    !q.is_empty() && t.len() >= q.len() && t.windows(q.len()).any(|w| w.iter().zip(q).all(|(&a, &b)| lower(a) == b))
}

impl Ui {
    /// Every name of the map in turn: kind, frame point, text. Read a file part at a time.
    pub fn scan_names(&self, mut f: impl FnMut(u8, (i32, i32), &[u8])) {
        let buf = unsafe { &mut *core::ptr::addr_of_mut!(super::CHUNK) };
        let mut off = 0usize;
        loop {
            let got = self.api.file_read("n", off, buf);
            if got <= 0 {
                return;
            }
            let got = got as usize;
            let mut i = if off == 0 {
                if got < 4 || &buf[..4] != b"WMN1" {
                    return;
                }
                4
            } else {
                0
            };
            while i + 6 <= got {
                let len = usize::from(buf[i + 5]);
                if i + 6 + len > got {
                    break;
                }
                let x = i32::from(i16::from_le_bytes([buf[i + 1], buf[i + 2]]));
                let y = i32::from(i16::from_le_bytes([buf[i + 3], buf[i + 4]]));
                f(buf[i], (x, y), &buf[i + 6..i + 6 + len]);
                i += 6 + len;
            }
            if i == 0 || got < buf.len() {
                return;
            }
            off += i;
        }
    }

    /// The name drawn nearest a tap at panel (x, y), if one is near.
    pub fn name_at(&self, x: i32, y: i32) -> Option<Hit> {
        let v = self.view();
        let mut best: Option<(i32, Hit)> = None;
        self.scan_names(|kind, p, t| {
            // streets aren't tapped (their names run along them); places of interest only where they are drawn
            if kind == STREET || (kind == 1 || kind >= 3) && v.k < 60 {
                return;
            }
            let (px, py) = v.px(p);
            let dy = if kind == 1 || kind >= 3 { py - 13 - y } else { py - y };
            let d = (px - x).pow(2) + dy.pow(2);
            if d < 30 * 30 && best.is_none_or(|(b, _)| d < b) {
                best = Some((d, make_hit(kind, p, t)));
            }
        });
        best.map(|(_, h)| h)
    }

    fn search(&mut self) {
        let mut q = [0u8; QUERY];
        let n = self.qn;
        for i in 0..n {
            q[i] = lower(self.query[i]);
        }
        let q = &q[..n];
        let from = self.me.unwrap_or(self.centre);
        let mut hits = [Hit::NONE; HITS];
        let mut dists = [i32::MAX; HITS];
        let mut count = 0usize;
        self.scan_names(|kind, p, t| {
            if !contains(t, q) {
                return;
            }
            // the same name nearby: already there
            let d = dist(from, p);
            for h in &hits[..count] {
                if &h.name[..usize::from(h.n)] == &t[..t.len().min(32)] && dist(h.p, p) < 400 {
                    return;
                }
            }
            // nearest first: in place of the farthest
            let at = if count < HITS {
                count += 1;
                count - 1
            } else {
                let (far, &fd) = dists.iter().enumerate().max_by_key(|&(_, &d)| d).unwrap();
                if d >= fd {
                    return;
                }
                far
            };
            hits[at] = make_hit(kind, p, t);
            dists[at] = d;
        });
        // sort by distance
        for i in 1..count {
            let mut j = i;
            while j > 0 && dists[j - 1] > dists[j] {
                dists.swap(j - 1, j);
                hits.swap(j - 1, j);
                j -= 1;
            }
        }
        self.hits = hits;
        self.n_hits = count;
        self.list_top = 0;
        self.screen = Screen::Results;
    }

    pub fn draw_keys(&mut self) {
        let mut s = self.s;
        Surface::rect(&mut s, 0, 0, 466, 466, 0);
        font::centre(&mut s, C, 44, if self.pack.is_some() { "Search the map" } else { "No map saved" }, GREY);
        rrect(&mut s, 60, 98, 346, 60, 30, CARD, BLACK);
        let q = core::str::from_utf8(&self.query[..self.qn]).unwrap_or("");
        let w = font::width(q).min(300);
        font::fitted(&mut s, C - w / 2, 114, q, 300, W);
        Surface::rect(&mut s, C + w / 2 + 2, 114, 3, 26, BLUE.rgb565());
        let sym = self.sym;
        each_key(sym, |k, x, y, w| {
            let fill = match k {
                K::Go => BLUE,
                K::Ch(_) => KEYC,
                _ => CARD,
            };
            rrect(&mut s, x + 3, y + 3, w - 6, KH - 6, 14, fill, BLACK);
            let mut b = [0u8; 4];
            let label: &str = match k {
                K::Ch(c) => (c as char).encode_utf8(&mut b),
                K::Del => "<",
                K::Sym if sym => "abc",
                K::Sym => "123",
                K::Space => "space",
                K::Go => "Go",
            };
            font::centre(&mut s, x + w / 2, y + KH / 2 - 12, label, if k == K::Space { GREY } else { W });
        });
    }

    pub fn keys_tap(&mut self, x: i32, y: i32) {
        let mut key = None;
        each_key(self.sym, |k, kx, ky, w| {
            if (kx..kx + w).contains(&x) && (ky..ky + KH).contains(&y) {
                key = Some(k);
            }
        });
        let Some(k) = key else { return };
        self.motor.play(haptic::TICK);
        match k {
            K::Ch(c) if self.qn < QUERY => {
                self.query[self.qn] = c;
                self.qn += 1;
            }
            K::Space if self.qn > 0 && self.qn < QUERY && self.query[self.qn - 1] != b' ' => {
                self.query[self.qn] = b' ';
                self.qn += 1;
            }
            K::Del => self.qn = self.qn.saturating_sub(1),
            K::Sym => self.sym = !self.sym,
            K::Go if self.qn > 0 => self.search(),
            _ => {}
        }
    }

    pub fn draw_results(&mut self) {
        let mut s = self.s;
        Surface::rect(&mut s, 0, 0, 466, 466, 0);
        font::centre(&mut s, C, 34, core::str::from_utf8(&self.query[..self.qn]).unwrap_or(""), W);
        if self.n_hits == 0 {
            font::centre(&mut s, C, 200, "Nothing found on this map", GREY);
            font::centre(&mut s, C, 232, if self.pack.is_some() { "Try another name" } else { "Save a map first" }, GREY);
            return;
        }
        let from = self.me.unwrap_or(self.centre);
        let mut b = [0u8; 16];
        for k in self.list_top..self.n_hits.min(self.list_top + ROWS) {
            let h = self.hits[k];
            let y = ROW_Y + (k - self.list_top) as i32 * ROW_H;
            let (x0, x1) = cmf_wp2_ui::geom::chord(y, y + ROW_H - 8, 30);
            rrect(&mut s, x0, y, x1 - x0, ROW_H - 8, 20, CARD, BLACK);
            disc(&mut s, x0 + 24, y + 24, 7, kind_colour(h.kind), CARD);
            let d = fmt_dist(dist(from, h.p), &mut b);
            let dw = font::width(d);
            font::fitted(&mut s, x0 + 40, y + 13, h.name(), x1 - x0 - 72 - dw, W);
            font::draw(&mut s, x1 - 20 - dw, y + 13, d, GREY);
        }
    }

    pub fn results_tap(&mut self, y: i32) {
        let r = (y - ROW_Y).div_euclid(ROW_H);
        if y < ROW_Y || r < 0 || r as usize >= ROWS {
            return;
        }
        let k = self.list_top + r as usize;
        if k >= self.n_hits {
            return;
        }
        self.motor.play(haptic::TICK);
        let h = self.hits[k];
        self.centre = h.p;
        self.follow = false;
        self.open_place(h);
    }

    /// A named place's card.
    pub fn open_place(&mut self, h: Hit) {
        self.sheet = (h.p, None);
        self.place = h;
        self.screen = Screen::Sheet;
    }
}

pub fn place_hit(kind: u8, p: (i32, i32), t: &[u8]) -> Hit {
    make_hit(kind, p, t)
}

fn make_hit(kind: u8, p: (i32, i32), t: &[u8]) -> Hit {
    let mut h = Hit { p, kind, ..Hit::NONE };
    let n = t.len().min(32);
    h.name[..n].copy_from_slice(&t[..n]);
    h.n = n as u8;
    h
}
