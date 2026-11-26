//! YouTube Music on the watch: the phone finds and fetches the songs, the watch plays them on its own speaker.
//! Only sound crosses the link while a song plays: no artwork, no progress (the watch counts what it played).
//!
//! Phone → watch (each ≤ 128 B):
//! - `o khz mode gen` a stream starts (a song picked, a skip, a seek): blocks follow from seq 0. The watch takes
//!   only the stream it last asked for (`gen`), so blocks of the old one still on the way are dropped.
//! - `N pos:u16 dur:u16 tlen title… artist…` the song the blocks after it are (seconds; shown once they play)
//! - `a seq:u16 block…` one IMA ADPCM block (cmf_wp2_app::adpcm) of 2/3 `khz` when mode 1 (upsampled on the watch)
//! - `x amount` exciter shimmer (1/128) · `z` nothing after the blocks sent (the queue ended) · `c` stop
//! - `L kind:u8 n:u8` a list starts (b'S' search results, b'Q' up next, b'H' history); `l idx:u8 flags:u8 tlen:u8 title… sub…`
//!   one row (flags 1: playing) · `X text…` a note · `?` say R
//!
//! Watch → phone: `R` ready · `s query…` · `p gen kind idx` play a row · `n gen` next · `b gen` previous ·
//! `j gen pos:u16` seek · `o kind` a list (b'Q' up next, b'H' the songs played before, newest first, b'P' YouTube Music's mixes
//! or Spotify's playlists; `p gen P idx` plays one) · `q` the app closed · `F next:u16 room:u8 gen:u8` credit: the
//! next block it expects and how many more it has room for (the phone sends no more than that).

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

    pub fn is_empty(&self) -> bool {
        self.n == 0
    }
}

#[derive(Clone, Copy)]
pub struct Song {
    /// where the blocks start, s
    pub pos: u16,
    pub dur: u16,
    pub title: Str<48>,
    pub artist: Str<40>,
}

impl Song {
    pub const fn new() -> Self {
        Song { pos: 0, dur: 0, title: Str::new(), artist: Str::new() }
    }

    /// `N pos dur title artist`
    pub fn parse(m: &[u8]) -> Option<Song> {
        let [b'N', p0, p1, d0, d1, rest @ ..] = m else { return None };
        let (title, artist) = field(rest)?;
        let mut s = Song::new();
        s.pos = u16::from_le_bytes([*p0, *p1]);
        s.dur = u16::from_le_bytes([*d0, *d1]);
        s.title.set(title);
        s.artist.set(artist);
        Some(s)
    }
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
    pub list: List,
    pub note: Str<40>,
}

#[derive(Debug, PartialEq)]
pub enum Change {
    None,
    List,
    Note,
}

impl Model {
    pub const fn new() -> Self {
        const ROW: Row = Row { title: Str::new(), sub: Str::new(), active: false };
        Model { list: List { kind: 0, rows: [ROW; MAX_ROWS], n: 0, want: 0 }, note: Str::new() }
    }

    pub fn message(&mut self, m: &[u8]) -> Change {
        match m {
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

    #[test]
    fn song() {
        let mut m = std::vec![b'N', 30, 0, 215, 0, 4];
        m.extend(b"Sobani");
        m.truncate(6 + 4);
        m.extend(b"Chanuka Mora");
        let s = Song::parse(&m).unwrap();
        assert_eq!((s.pos, s.dur, s.title.as_str(), s.artist.as_str()), (30, 215, "Soba", "Chanuka Mora"));
        assert!(Song::parse(&[b'N', 1, 0, 0, 0, 9, b'a']).is_none());
    }

    #[test]
    fn lists() {
        let mut m = Model::new();
        m.message(&[b'L', b'S', 2]);
        let mut row = std::vec![b'l', 1, 1, 5];
        row.extend(b"Chill");
        row.extend(b"Artist - 3:12");
        assert_eq!(m.message(&row), Change::List);
        assert_eq!((m.list.kind, m.list.n, m.list.want), (b'S', 2, 2));
        assert_eq!(m.list.rows[1].title.as_str(), "Chill");
        assert!(m.list.rows[1].active);
        assert_eq!(m.message(&[b'l', 200, 0, 1, b'x']), Change::None);
    }
}
