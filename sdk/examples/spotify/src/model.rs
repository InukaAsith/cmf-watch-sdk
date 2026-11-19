//! Spotify on the watch: the phone's hub app talks to the Spotify Web API; the watch shows and asks.
//!
//! Phone → watch (each ≤ 128 B):
//! - `N flags:u8 pos:u16 dur:u16 vol:u8 tlen:u8 title… alen:u8 artist… device…` now playing (seconds; flags:
//!   1 playing, 2 premium, 4 liked, 8 shuffle, 16 nothing playing)
//! - `I` / `D` / `E`: the album art, pixview run tokens (rle.rs), 152 × 152
//! - `L kind:u8 n:u8` a list starts (kind: b'P' playlists, b'T' tracks, b'S' search results, b'D' devices,
//!   b'A' playlists to add the song to); `l idx:u8 flags:u8 tlen:u8 title… sub…` one row (flags 1: active)
//! - `X text…` a short note ("Premium needed", "Added")
//!
//! Watch → phone: `R` ready · `kp` play/pause · `kn` next · `kb` previous · `v vol:u8` · `h` like ·
//! `o` + kind (b'P' playlists, b'D' devices, b'A' add to playlist) · `s query…` · `p kind:u8 idx:u8` pick.

#[derive(Clone, Copy)]
pub struct Str<const N: usize> {
    b: [u8; N],
    n: usize,
}

impl<const N: usize> Str<N> {
    pub const fn new() -> Self {
        Str { b: [0; N], n: 0 }
    }

    pub fn set(&mut self, s: &[u8]) {
        let mut n = s.len().min(N);
        while n > 0 && core::str::from_utf8(&s[..n]).is_err() {
            n -= 1;
        }
        self.b[..n].copy_from_slice(&s[..n]);
        self.n = n;
    }

    pub fn as_str(&self) -> &str {
        core::str::from_utf8(&self.b[..self.n]).unwrap_or("")
    }
}

pub const PLAYING: u8 = 1;
pub const PREMIUM: u8 = 2;
pub const LIKED: u8 = 4;
pub const SHUFFLE: u8 = 8;
pub const IDLE: u8 = 16;

pub struct Now {
    pub flags: u8,
    pub pos: u16,
    pub dur: u16,
    pub vol: u8,
    pub title: Str<48>,
    pub artist: Str<40>,
    pub device: Str<24>,
}

pub const MAX_ROWS: usize = 24;

#[derive(Clone, Copy)]
pub struct Row {
    pub title: Str<40>,
    pub sub: Str<32>,
    pub active: bool,
}

pub struct List {
    pub kind: u8,
    pub rows: [Row; MAX_ROWS],
    pub n: usize,
    /// rows announced (`L`): more may still be coming
    pub want: usize,
}

pub struct Model {
    pub now: Now,
    pub list: List,
    pub note: Str<40>,
}

#[derive(Debug, PartialEq)]
pub enum Change {
    None,
    Now,
    List,
    Note,
}

impl Model {
    pub const fn new() -> Self {
        const ROW: Row = Row { title: Str::new(), sub: Str::new(), active: false };
        Model {
            now: Now { flags: IDLE, pos: 0, dur: 0, vol: 50, title: Str::new(), artist: Str::new(), device: Str::new() },
            list: List { kind: 0, rows: [ROW; MAX_ROWS], n: 0, want: 0 },
            note: Str::new(),
        }
    }

    pub fn message(&mut self, m: &[u8]) -> Change {
        match m {
            [b'N', flags, p0, p1, d0, d1, vol, rest @ ..] => {
                let Some((title, rest)) = field(rest) else { return Change::None };
                let Some((artist, device)) = field(rest) else { return Change::None };
                let n = &mut self.now;
                n.flags = *flags;
                n.pos = u16::from_le_bytes([*p0, *p1]);
                n.dur = u16::from_le_bytes([*d0, *d1]);
                n.vol = (*vol).min(100);
                n.title.set(title);
                n.artist.set(artist);
                n.device.set(device);
                Change::Now
            }
            [b'L', kind, n] => {
                self.list.kind = *kind;
                self.list.n = 0;
                self.list.want = usize::from(*n).min(MAX_ROWS);
                Change::List
            }
            [b'l', idx, flags, rest @ ..] => {
                let i = usize::from(*idx);
                let Some((title, sub)) = field(rest) else { return Change::None };
                if i >= MAX_ROWS {
                    return Change::None;
                }
                let r = &mut self.list.rows[i];
                r.title.set(title);
                r.sub.set(sub);
                r.active = flags & 1 != 0;
                self.list.n = self.list.n.max(i + 1);
                Change::List
            }
            [b'X', text @ ..] => {
                self.note.set(text);
                Change::Note
            }
            _ => Change::None,
        }
    }

    pub fn playing(&self) -> bool {
        self.now.flags & PLAYING != 0
    }
}

/// `len:u8 bytes…` → (bytes, rest)
fn field(b: &[u8]) -> Option<(&[u8], &[u8])> {
    let (&n, rest) = b.split_first()?;
    let n = usize::from(n);
    (rest.len() >= n).then(|| rest.split_at(n))
}

#[cfg(test)]
mod tests {
    use super::*;
    extern crate std;
    use std::vec::Vec;

    fn now(flags: u8, title: &str, artist: &str, device: &str) -> Vec<u8> {
        let mut m = std::vec![b'N', flags];
        m.extend(83u16.to_le_bytes());
        m.extend(215u16.to_le_bytes());
        m.push(60);
        m.push(title.len() as u8);
        m.extend(title.as_bytes());
        m.push(artist.len() as u8);
        m.extend(artist.as_bytes());
        m.extend(device.as_bytes());
        m
    }

    #[test]
    fn now_playing() {
        let mut m = Model::new();
        assert_eq!(m.message(&now(PLAYING | PREMIUM, "Midnight City", "M83", "iPhone")), Change::Now);
        assert!(m.playing());
        assert_eq!((m.now.pos, m.now.dur, m.now.vol), (83, 215, 60));
        assert_eq!(m.now.title.as_str(), "Midnight City");
        assert_eq!(m.now.artist.as_str(), "M83");
        assert_eq!(m.now.device.as_str(), "iPhone");
        // a short field is dropped, not half read
        assert_eq!(m.message(&[b'N', 1, 0, 0, 0, 0, 5, 9, b'a']), Change::None);
    }

    #[test]
    fn lists() {
        let mut m = Model::new();
        m.message(&[b'L', b'P', 2]);
        let mut row = std::vec![b'l', 1, 1, 5];
        row.extend(b"Chill");
        row.extend(b"42 songs");
        assert_eq!(m.message(&row), Change::List);
        assert_eq!((m.list.kind, m.list.n, m.list.want), (b'P', 2, 2));
        assert_eq!(m.list.rows[1].title.as_str(), "Chill");
        assert_eq!(m.list.rows[1].sub.as_str(), "42 songs");
        assert!(m.list.rows[1].active);
        assert_eq!(m.message(&[b'l', 200, 0, 1, b'x']), Change::None);
    }

    #[test]
    fn utf8_cut_on_a_boundary() {
        let mut s: Str<4> = Str::new();
        s.set("abcé".as_bytes());
        assert_eq!(s.as_str(), "abc");
    }
}
