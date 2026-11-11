// Pure logic of herdr-agents: wire protocol, agent list, sorting, haptic transitions, storage.
// Protocol: docs/herdr-agents.md.

pub const MAGIC: &[u8; 2] = b"h1";
pub const MAX: usize = 32;
pub const WS_MAX: usize = 24;
pub const TITLE_MAX: usize = 64;
pub const REQUEST: &[u8] = b"h1R";

use cmf_wp2_ui::haptic::{self, Pattern};
use cmf_wp2_ui::text::fit;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Status {
    Idle,
    Working,
    Blocked,
    Done,
    Unknown,
}

impl Status {
    pub fn from(b: u8) -> Status {
        match b {
            0 => Status::Idle,
            1 => Status::Working,
            2 => Status::Blocked,
            3 => Status::Done,
            _ => Status::Unknown,
        }
    }

    fn code(self) -> u8 {
        match self {
            Status::Idle => 0,
            Status::Working => 1,
            Status::Blocked => 2,
            Status::Done => 3,
            Status::Unknown => 4,
        }
    }

    fn rank(self) -> u8 {
        match self {
            Status::Blocked => 0,
            Status::Done => 1,
            Status::Working => 2,
            Status::Idle => 3,
            Status::Unknown => 4,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Buzz {
    Done,
    Blocked,
}

impl Buzz {
    pub fn pattern(self) -> Pattern {
        match self {
            Buzz::Blocked => haptic::ALERT,
            Buzz::Done => haptic::DONE,
        }
    }
}

#[derive(Clone, Copy)]
pub struct Agent {
    pub id: u8,
    pub status: Status,
    wl: u8,
    tl: u8,
    ws: [u8; WS_MAX],
    title: [u8; TITLE_MAX],
    seen: bool,
}

impl Agent {
    const EMPTY: Agent = Agent { id: 0, status: Status::Idle, wl: 0, tl: 0, ws: [0; WS_MAX], title: [0; TITLE_MAX], seen: false };

    fn new(id: u8, status: Status, ws: &[u8], title: &[u8]) -> Agent {
        let mut a = Agent { id, status, ..Agent::EMPTY };
        let (w, t) = (fit(ws, WS_MAX).as_bytes(), fit(title, TITLE_MAX).as_bytes());
        a.ws[..w.len()].copy_from_slice(w);
        a.title[..t.len()].copy_from_slice(t);
        (a.wl, a.tl) = (w.len() as u8, t.len() as u8);
        a
    }

    pub fn ws(&self) -> &str {
        fit(&self.ws[..self.wl as usize], WS_MAX)
    }

    pub fn title(&self) -> &str {
        fit(&self.title[..self.tl as usize], TITLE_MAX)
    }
}

#[derive(Debug, PartialEq)]
pub enum Msg<'a> {
    Begin,
    Upsert { id: u8, status: Status, ws: &'a [u8], title: &'a [u8] },
    Delete(u8),
    End(u8),
    Beat,
}

/// `h1`, kind, seq, body. `None`: not ours, other version, or malformed.
pub fn decode(m: &[u8]) -> Option<(u8, Msg<'_>)> {
    let (head, body) = m.split_at_checked(4)?;
    if &head[..2] != MAGIC {
        return None;
    }
    let msg = match (head[2], body) {
        (b'B', []) => Msg::Begin,
        (b'H', []) => Msg::Beat,
        (b'D', [id]) => Msg::Delete(*id),
        (b'E', [n]) => Msg::End(*n),
        (b'U', [id, st, wl, rest @ ..]) if *wl as usize <= rest.len() => {
            let (ws, title) = rest.split_at(*wl as usize);
            Msg::Upsert { id: *id, status: Status::from(*st), ws, title }
        }
        _ => return None,
    };
    Some((head[3], msg))
}

#[derive(Default, Debug, PartialEq)]
pub struct Effect {
    /// the list changed: redraw
    pub changed: bool,
    pub buzz: Option<Buzz>,
    /// a message is missing (seq gap, a delta before any snapshot, a bad snapshot): ask for a snapshot
    pub resync: bool,
    /// a snapshot ended: the list is current; save it
    pub synced: bool,
}

pub struct Model {
    agents: [Agent; MAX],
    n: usize,
    order: [u8; MAX],
    seq: Option<u8>,
    snapshot: bool,
}

impl Model {
    pub const fn new() -> Self {
        Model { agents: [Agent::EMPTY; MAX], n: 0, order: [0; MAX], seq: None, snapshot: false }
    }

    pub fn len(&self) -> usize {
        self.n
    }

    /// i-th agent in display order: blocked, done, working, idle, unknown; then workspace, title, id
    pub fn get(&self, i: usize) -> Option<&Agent> {
        (i < self.n).then(|| &self.agents[self.order[i] as usize])
    }

    /// (working, blocked, done)
    pub fn counts(&self) -> (usize, usize, usize) {
        let c = |s| self.agents[..self.n].iter().filter(|a| a.status == s).count();
        (c(Status::Working), c(Status::Blocked), c(Status::Done))
    }

    fn find(&self, id: u8) -> Option<usize> {
        self.agents[..self.n].iter().position(|a| a.id == id)
    }

    fn remove(&mut self, i: usize) {
        self.agents.copy_within(i + 1..self.n, i);
        self.n -= 1;
    }

    fn sort(&mut self) {
        for i in 0..self.n {
            self.order[i] = i as u8;
        }
        fn key(a: &Agent) -> (u8, &str, &str, u8) {
            (a.status.rank(), a.ws(), a.title(), a.id)
        }
        let (agents, order) = (&self.agents, &mut self.order[..self.n]);
        order.sort_unstable_by(|&x, &y| key(&agents[x as usize]).cmp(&key(&agents[y as usize])));
    }

    /// Applies one host message. Snapshots (`Begin` … `End`) never buzz: they follow a start, a resume or a gap.
    pub fn apply(&mut self, raw: &[u8]) -> Effect {
        let mut fx = Effect::default();
        let Some((seq, msg)) = decode(raw) else {
            return fx;
        };
        if msg == Msg::Begin {
            self.snapshot = true;
            for a in &mut self.agents[..self.n] {
                a.seen = false;
            }
        } else if self.seq != Some(seq.wrapping_sub(1)) {
            fx.resync = true;
            if self.seq.is_none() {
                return fx;
            }
        }
        self.seq = Some(seq);
        match msg {
            Msg::Begin | Msg::Beat => {}
            Msg::Upsert { id, status, ws, title } => {
                let mut a = Agent::new(id, status, ws, title);
                a.seen = true;
                let old = self.find(id).map(|i| self.agents[i].status);
                if !self.snapshot && old != Some(status) {
                    fx.buzz = match status {
                        Status::Blocked => Some(Buzz::Blocked),
                        Status::Done if old.is_some() => Some(Buzz::Done),
                        _ => None,
                    };
                }
                match self.find(id) {
                    Some(i) => self.agents[i] = a,
                    None if self.n < MAX => {
                        self.agents[self.n] = a;
                        self.n += 1;
                    }
                    None => {}
                }
                fx.changed = true;
            }
            Msg::Delete(id) => {
                if let Some(i) = self.find(id) {
                    self.remove(i);
                    fx.changed = true;
                }
            }
            Msg::End(n) => {
                if !self.snapshot {
                    fx.resync = true;
                } else {
                    let mut i = 0;
                    while i < self.n {
                        if self.agents[i].seen { i += 1 } else { self.remove(i) }
                    }
                    self.snapshot = false;
                    fx.changed = true;
                    fx.synced = n as usize == self.n.min(MAX) || (n as usize > MAX && self.n == MAX);
                    fx.resync |= !fx.synced;
                }
            }
        }
        if fx.changed {
            self.sort();
        }
        fx
    }

    /// `H1`, n, then per agent: id, status, ws len, ws, title len, title.
    pub fn save(&self, out: &mut [u8]) -> usize {
        out[..3].copy_from_slice(&[b'H', b'1', self.n as u8]);
        let mut p = 3;
        for a in &self.agents[..self.n] {
            let (w, t) = (a.ws().as_bytes(), a.title().as_bytes());
            out[p..p + 3].copy_from_slice(&[a.id, a.status.code(), w.len() as u8]);
            p += 3;
            out[p..p + w.len()].copy_from_slice(w);
            p += w.len();
            out[p] = t.len() as u8;
            p += 1;
            out[p..p + t.len()].copy_from_slice(t);
            p += t.len();
        }
        p
    }

    /// Fills the list from a saved blob (not synced: the host still sends a snapshot). Garbage loads nothing.
    pub fn load(&mut self, data: &[u8]) {
        // checked first, then filled in place: no second Model on the 8 KiB stack
        if Self::parse(data, |_| {}).is_some() {
            (self.n, self.seq, self.snapshot) = (0, None, false);
            Self::parse(data, |a| {
                self.agents[self.n] = a;
                self.n += 1;
            });
            self.sort();
        }
    }

    fn parse(data: &[u8], mut each: impl FnMut(Agent)) -> Option<()> {
        let [b'H', b'1', n, rest @ ..] = data else { return None };
        let mut rest: &[u8] = rest;
        for _ in 0..(*n as usize).min(MAX) {
            let [id, st, wl, r @ ..] = rest else { return None };
            let Some((ws, [tl, r @ ..])) = r.split_at_checked(*wl as usize) else { return None };
            let (title, r) = r.split_at_checked(*tl as usize)?;
            each(Agent::new(*id, Status::from(*st), ws, title));
            rest = r;
        }
        Some(())
    }
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use std::vec::Vec;

    fn msg(kind: u8, seq: u8, body: &[u8]) -> Vec<u8> {
        let mut m = Vec::from(&b"h1"[..]);
        m.extend([kind, seq]);
        m.extend(body);
        m
    }

    fn up(seq: u8, id: u8, st: u8, ws: &str, title: &str) -> Vec<u8> {
        let mut b = Vec::from([id, st, ws.len() as u8]);
        b.extend(ws.bytes());
        b.extend(title.bytes());
        msg(b'U', seq, &b)
    }

    fn snapshot(m: &mut Model, agents: &[(u8, u8, &str, &str)]) -> u8 {
        assert_eq!(m.apply(&msg(b'B', 10, &[])), Effect::default());
        let mut seq = 10;
        for &(id, st, ws, t) in agents {
            seq += 1;
            let fx = m.apply(&up(seq, id, st, ws, t));
            assert_eq!((fx.buzz, fx.resync), (None, false));
        }
        seq += 1;
        let fx = m.apply(&msg(b'E', seq, &[agents.len() as u8]));
        assert!(fx.synced && !fx.resync);
        seq
    }

    fn ids(m: &Model) -> Vec<u8> {
        (0..m.len()).map(|i| m.get(i).unwrap().id).collect()
    }

    #[test]
    fn decode_rejects_foreign_and_short() {
        assert_eq!(decode(b"hello"), None);
        assert_eq!(decode(b"h2B\x01"), None);
        assert_eq!(decode(b"h1U\x01\x05\x02\x09ab"), None); // ws length past the end
        assert_eq!(decode(b"h1H\x07"), Some((7, Msg::Beat)));
        assert_eq!(decode(b"h1U\x01\x05\x02\x02abc"), Some((1, Msg::Upsert { id: 5, status: Status::Blocked, ws: b"ab", title: b"c" })));
    }

    #[test]
    fn sorts_blocked_done_working_idle() {
        let mut m = Model::new();
        snapshot(&mut m, &[(1, 0, "a", "idle"), (2, 1, "a", "work"), (3, 3, "b", "done"), (4, 2, "z", "blk"),
            (5, 2, "a", "blk"), (6, 4, "a", "?")]);
        assert_eq!(ids(&m), [5, 4, 3, 2, 1, 6]);
        assert_eq!(m.counts(), (1, 2, 1));
    }

    #[test]
    fn transitions_buzz_only_outside_snapshots() {
        let mut m = Model::new();
        let s = snapshot(&mut m, &[(1, 1, "w", "a"), (2, 1, "w", "b")]);
        assert_eq!(m.apply(&up(s + 1, 1, 2, "w", "a")).buzz, Some(Buzz::Blocked));
        assert_eq!(m.apply(&up(s + 2, 1, 2, "w", "a renamed")).buzz, None); // same status
        assert_eq!(m.apply(&up(s + 3, 2, 3, "w", "b")).buzz, Some(Buzz::Done));
        assert_eq!(m.apply(&up(s + 4, 9, 3, "w", "new and done")).buzz, None);
        assert_eq!(m.apply(&up(s + 5, 8, 2, "w", "new and blocked")).buzz, Some(Buzz::Blocked));
        assert_eq!(m.apply(&up(s + 6, 2, 1, "w", "b")).buzz, None);
        assert_eq!(ids(&m), [1, 8, 9, 2]);
    }

    #[test]
    fn seq_gap_and_delta_before_snapshot_resync() {
        let mut m = Model::new();
        let fx = m.apply(&up(3, 1, 2, "w", "a"));
        assert!(fx.resync && !fx.changed && m.len() == 0);
        let s = snapshot(&mut m, &[(1, 1, "w", "a")]);
        let fx = m.apply(&up(s + 2, 1, 2, "w", "a"));
        assert!(fx.resync && fx.changed);
        assert!(m.apply(&msg(b'E', s + 3, &[1])).resync); // End without Begin
        assert!(!m.apply(&msg(b'H', s + 4, &[])).resync);
    }

    #[test]
    fn snapshot_replaces_list_and_counts() {
        let mut m = Model::new();
        let s = snapshot(&mut m, &[(1, 1, "w", "a"), (2, 1, "w", "b"), (3, 0, "w", "c")]);
        assert!(m.apply(&msg(b'D', s + 1, &[2])).changed);
        assert_eq!(m.len(), 2);
        snapshot(&mut m, &[(3, 0, "w", "c"), (4, 2, "x", "d")]);
        assert_eq!(ids(&m), [4, 3]);
        // a snapshot that lost an upsert does not claim to be synced
        m.apply(&msg(b'B', 50, &[]));
        m.apply(&up(51, 4, 2, "x", "d"));
        let fx = m.apply(&msg(b'E', 52, &[2]));
        assert!(!fx.synced && fx.resync);
    }

    #[test]
    fn wraps_seq() {
        let mut m = Model::new();
        m.apply(&msg(b'B', 255, &[]));
        assert!(!m.apply(&msg(b'E', 0, &[0])).resync);
    }

    #[test]
    fn utf8_cut_on_char_boundary() {
        let mut m = Model::new();
        let long = "ж".repeat(40);
        snapshot(&mut m, &[(1, 2, "рабочее пространство очень длинное", &long)]);
        let a = m.get(0).unwrap();
        assert_eq!(a.title(), "ж".repeat(32));
        assert_eq!(a.ws(), "рабочее прос");
    }

    #[test]
    fn storage_round_trip_and_garbage() {
        let mut m = Model::new();
        let agents: Vec<_> = (0..MAX as u8).map(|i| (i, i % 5, "рабочее", "заголовок очень длинный и ещё длиннее, чем влезет")).collect();
        snapshot(&mut m, &agents);
        let mut buf = [0u8; 4096];
        let n = m.save(&mut buf);
        assert!(n <= 4096);
        let mut back = Model::new();
        back.load(&buf[..n]);
        assert_eq!(ids(&back), ids(&m));
        assert_eq!(back.get(3).unwrap().title(), m.get(3).unwrap().title());
        // not synced: the first delta asks for a snapshot
        assert!(back.apply(&up(1, 0, 2, "w", "x")).resync);
        let mut bad = Model::new();
        bad.load(&buf[..n / 2]);
        assert_eq!(bad.len(), 0);
        bad.load(b"junk");
        assert_eq!(bad.len(), 0);
    }

    #[test]
    fn overflow_keeps_max() {
        let mut m = Model::new();
        m.apply(&msg(b'B', 0, &[]));
        for i in 0..40u8 {
            m.apply(&up(i + 1, i, 1, "w", "t"));
        }
        let fx = m.apply(&msg(b'E', 41, &[40]));
        assert!(fx.synced && m.len() == MAX);
    }

    #[test]
    fn buzz_patterns() {
        assert_eq!(Buzz::Blocked.pattern().steps(), &[(1000, 110), (0, 90), (1000, 110), (0, 90), (1000, 110), (0, 0)]);
        assert_eq!(Buzz::Done.pattern().steps(), &[(650, 220), (0, 0)]);
    }
}
