//! The speaker side of YouTube Music: the stream of sdk/examples/speaker (ADPCM 10.67 kHz, 2 → 3 upsampling,
//! the 4.8 kHz anti-image low-pass, the exciter, credit flow), plus what a player needs: streams by generation
//! (a skip drops what is still on the way), songs marked in the stream (shown when they play), pause, and the
//! position counted from what was played. Host-tested against a fake speaker.
//!
//! The buffer holds the stream still compressed (`STORE` blocks, ~7 s) and decodes a block just before it plays
//! (a small PCM ring for the loader): 4× the time in the same memory as decoded sound. A stream starts once the
//! buffer is full, and after running dry it fills up again before it goes on: a slow moment of the link has to
//! last ~7 s to be heard. `fill()` is how full it is, for the circle the screen fills meanwhile.

use crate::model::Song;
use cmf_wp2_app::adpcm;

/// One decoded block (122 code bytes → 488 B of PCM), and upsampled 2 → 3.
pub const BLOCK_PCM: usize = 488;
pub const UP_PCM: usize = BLOCK_PCM * 3 / 2;
/// A message's block as it came: predictor, index, 122 code bytes.
pub const BLOCK: usize = 125;
/// Compressed blocks held: 320 × 244 samples at 10.67 kHz = 7.3 s (40 KB; the app image is 128 KB in all).
pub const STORE: usize = 320;
/// Decoded sound ahead of the loader: ~0.25 s at 16 kHz.
pub const PCM: usize = 8 * 1024;
/// Blocks that may come before one refused and sent again (the phone's window is 4).
const EARLY: usize = 8;
/// Played once this many blocks are held: all but the last few the phone has in flight.
pub const FULL: usize = STORE - 8;
/// the volume is applied as the samples go to the loader, so a crown turn is heard at once
const CHUNK: usize = 512;

/// The loader's audio track (Api in the app, a recorder in tests).
pub trait Sink {
    fn open(&mut self, khz: u8) -> i32;
    fn write(&mut self, pcm: &[u8]) -> i32;
    fn queued(&self) -> i32;
    fn close(&mut self);
}

/// 4th-order Butterworth low-pass at 4.8 kHz (16 kHz), Q14: removes the upsampling's mirror image.
const LOWPASS: [[i32; 5]; 2] = [[5708, 11416, 5708, 5390, 1058], [7862, 15724, 7862, 7424, 7640]];
/// Exciter: full-wave rectified 3-8 kHz, what lands above 5.5 kHz mixed back in (shimmer the stream lacks).
const HIGHPASS: [[i32; 5]; 2] = [[2059, -4118, 2059, 10296, 2148], [2762, -5524, 2762, 13811, 8475]];
const SPLIT_3K: i32 = 22680;

#[derive(Clone, Copy, Default)]
struct Biquad {
    x1: i32,
    x2: i32,
    y1: i32,
    y2: i32,
}

impl Biquad {
    fn run(&mut self, k: &[i32; 5], x: i32) -> i32 {
        let y = (k[0] * x + k[1] * self.x1 + k[2] * self.x2 - k[3] * self.y1 - k[4] * self.y2) >> 14;
        let y = y.clamp(-32768, 32767);
        (self.x2, self.x1, self.y2, self.y1) = (self.x1, x, self.y1, y);
        y
    }
}

#[derive(Clone, Copy, Default)]
struct Exciter {
    low: i32,
    hp: [Biquad; 2],
}

impl Exciter {
    fn run(&mut self, x: i32, amount: i32) -> i32 {
        self.low += ((x - self.low) * SPLIT_3K) >> 15;
        let top = (x - self.low).abs();
        let e = self.hp[0].run(&HIGHPASS[0], top);
        let e = self.hp[1].run(&HIGHPASS[1], e);
        (x + ((e * amount) >> 7)).clamp(-32768, 32767)
    }
}

const MARKS: usize = 4;

pub struct Player {
    store: [[u8; BLOCK]; STORE],
    lens: [u8; STORE],
    // blocks of this stream: stored, decoded (free-running from its `o`)
    s_head: usize,
    s_tail: usize,
    pcm: [u8; PCM],
    block: [u8; BLOCK_PCM],
    scratch: [u8; CHUNK],
    // PCM bytes of this stream: decoded into the ring, given to the loader
    head: usize,
    tail: usize,
    buffering: bool,
    up: bool,
    khz: u8,
    pend: Option<(i32, i32)>,
    lp: [Biquad; 2],
    ex: Exciter,
    excite: i32,
    pub volume: u8,
    next_seq: Option<u16>,
    /// blocks next_seq + 1 + k already here (bit k), waiting for next_seq
    early: u8,
    /// the stream asked for last; blocks of other streams are dropped
    pub gn: u8,
    /// its `o` came: blocks are taken
    pub live: bool,
    pub paused: bool,
    /// the popup is up (the loader dropped its queue): nothing is written until it closes
    pub held: bool,
    opened: bool,
    /// a stream was asked for and has not ended
    asked: bool,
    /// it has played: a later fill is a refill
    started: bool,
    /// how far the phone got preparing the song (`r gen pct`), 0..=100
    prep: u8,
    /// (block index, song): the songs of the stream, shown once their first block is heard
    marks: [(usize, Song); MARKS],
    n_marks: usize,
    end_at: Option<usize>,
    /// the song playing, and the PCM byte its blocks start at
    pub now: Option<Song>,
    start: usize,
    pub error: i32,
    /// bytes the loader still holds (not heard yet), as of the last feed
    lq: usize,
    pub blocks: u32,
    pub lost: u32,
    untold: u32,
    told_at: u32,
}

impl Player {
    pub const fn new() -> Self {
        Player {
            store: [[0; BLOCK]; STORE],
            lens: [0; STORE],
            s_head: 0,
            s_tail: 0,
            pcm: [0; PCM],
            block: [0; BLOCK_PCM],
            scratch: [0; CHUNK],
            head: 0,
            tail: 0,
            buffering: true,
            up: false,
            khz: 16,
            pend: None,
            lp: [Biquad { x1: 0, x2: 0, y1: 0, y2: 0 }; 2],
            ex: Exciter { low: 0, hp: [Biquad { x1: 0, x2: 0, y1: 0, y2: 0 }; 2] },
            excite: 40,
            volume: 70,
            next_seq: None,
            early: 0,
            gn: 0,
            live: false,
            paused: false,
            held: false,
            opened: false,
            asked: false,
            started: false,
            prep: 0,
            marks: [(0, Song::new()); MARKS],
            n_marks: 0,
            end_at: None,
            now: None,
            start: 0,
            error: 0,
            lq: 0,
            blocks: 0,
            lost: 0,
            untold: 0,
            told_at: 0,
        }
    }

    /// Compressed blocks held.
    pub fn stored(&self) -> usize {
        self.s_head - self.s_tail
    }

    fn pcm_queued(&self) -> usize {
        self.head - self.tail
    }

    /// PCM bytes one block becomes.
    fn per_block(&self) -> usize {
        if self.up { UP_PCM } else { BLOCK_PCM }
    }

    /// Room for one more block: the app reads the phone only then.
    pub fn has_room(&self) -> bool {
        self.stored() < STORE
    }

    /// Something is to be played (the main loop wakes every 5 ms for it).
    pub fn busy(&self) -> bool {
        self.live && !self.buffering && !self.paused && !self.held && (self.stored() > 0 || self.pcm_queued() > 0)
    }

    /// The phone prepares the song (fetch, decode, tune, encode): nothing of it here yet. 0..=65536, how far.
    pub fn preparing(&self) -> Option<u32> {
        if !self.asked || self.error != 0 || self.started || self.stored() > 0 {
            return None;
        }
        Some(u32::from(self.prep.min(100)) * 65536 / 100)
    }

    /// How full the buffer is while the watch waits for it (its first blocks came), 0..=65536; None otherwise.
    pub fn fill(&self) -> Option<u32> {
        if !self.asked || self.error != 0 || self.live && !self.buffering || self.preparing().is_some() {
            return None;
        }
        Some((self.stored().min(FULL) as u32 * 65536 / FULL as u32).min(65536))
    }

    /// A new stream is wanted (a pick, a skip, a seek): what is held and on the way is dropped. The gen goes in
    /// the request to the phone.
    pub fn want(&mut self) -> u8 {
        self.gn = self.gn.wrapping_add(1);
        self.live = false;
        self.asked = true;
        self.paused = false;
        self.prep = 0;
        self.reset_stream();
        self.gn
    }

    fn reset_stream(&mut self) {
        self.s_head = 0;
        self.s_tail = 0;
        self.head = 0;
        self.tail = 0;
        self.n_marks = 0;
        self.end_at = None;
        self.next_seq = None;
        self.early = 0;
        self.buffering = true;
        self.started = false;
        self.pend = None;
        self.start = 0;
    }

    /// The phone went silent while a song was coming: nothing is waited for any more.
    pub fn give_up(&mut self) {
        self.gn = self.gn.wrapping_add(1); // a stream that comes late is not played
        self.asked = false;
        self.live = false;
        self.prep = 0;
        self.reset_stream();
    }

    /// A seek asked for: the song shows at `pos` while the stream from there comes.
    pub fn show_at(&mut self, pos: u16) {
        if let Some(s) = &mut self.now {
            s.pos = pos;
        }
        self.start = 0;
    }

    /// Played so far of the song, s.
    pub fn pos(&self) -> u16 {
        let Some(s) = &self.now else { return 0 };
        let rate = usize::from(self.khz) * 2000;
        let played = (self.heard().saturating_sub(self.start) / rate) as u16;
        s.pos.saturating_add(played).min(s.dur.max(s.pos))
    }

    /// Loading: a stream was asked for and has not started to play.
    pub fn loading(&self) -> bool {
        self.asked && self.error == 0 && (!self.live || !self.started)
    }

    /// Filling up again after the link ran dry.
    pub fn refilling(&self) -> bool {
        self.live && self.started && self.buffering
    }

    /// One phone message for the player; true when it was one (else it is the UI's).
    pub fn message(&mut self, m: &[u8], sink: &mut impl Sink) -> bool {
        match m {
            [b'o', khz, mode, gn] => {
                if *gn == self.gn {
                    self.live = true;
                    self.up = *mode == 1;
                    self.khz = *khz;
                    self.lp = [Biquad::default(); 2];
                    self.ex = Exciter::default();
                    self.reset_stream();
                    self.blocks = 0;
                    self.lost = 0;
                    if !self.opened {
                        self.error = sink.open(*khz);
                        self.opened = self.error == 0;
                    }
                }
            }
            [b'N', ..] => {
                if let (true, Some(s)) = (self.live, Song::parse(m)) {
                    if self.n_marks == MARKS {
                        self.marks.copy_within(1.., 0);
                        self.n_marks -= 1;
                    }
                    self.marks[self.n_marks] = (self.s_head, s);
                    self.n_marks += 1;
                }
            }
            [b'a', s0, s1, block @ ..] => {
                if self.live {
                    self.audio(u16::from_le_bytes([*s0, *s1]), block);
                }
            }
            [b'x', amount] => self.excite = i32::from(*amount),
            [b'r', gn, pct] => {
                if *gn == self.gn {
                    self.prep = (*pct).max(self.prep);
                }
            }
            [b'z'] => {
                if self.live {
                    self.end_at = Some(self.s_head);
                }
            }
            [b'c'] => {
                self.live = false;
                self.asked = false;
                self.reset_stream();
                self.now = None;
            }
            _ => return false,
        }
        true
    }

    /// A block by its number. One the watch refused comes again after later ones (the phone keeps 4 in flight):
    /// those wait in their places (up to EARLY ahead) until it fills the gap, so nothing is skipped. A gap
    /// wider than that is given up (counted lost).
    fn audio(&mut self, seq: u16, block: &[u8]) {
        if block.len() > BLOCK {
            return;
        }
        let mut want = self.next_seq.unwrap_or(seq);
        let mut ahead = seq.wrapping_sub(want);
        if ahead >= 0x8000 {
            return; // old or repeated
        }
        if usize::from(ahead) > EARLY {
            self.lost += u32::from(ahead);
            self.early = 0;
            want = seq;
            ahead = 0;
        }
        let at = self.s_head + usize::from(ahead);
        if at - self.s_tail >= STORE {
            return; // more than the credit allowed: never from a phone that follows it
        }
        let slot = at % STORE;
        self.store[slot][..block.len()].copy_from_slice(block);
        self.lens[slot] = block.len() as u8;
        self.blocks += 1;
        if ahead > 0 {
            self.early |= 1 << (ahead - 1);
            self.next_seq = Some(want);
            return;
        }
        // in place: it and the ones that came early behind it are held now
        self.s_head += 1;
        want = want.wrapping_add(1);
        while self.early & 1 == 1 {
            self.early >>= 1;
            self.s_head += 1;
            want = want.wrapping_add(1);
        }
        self.early >>= 1;
        self.next_seq = Some(want);
    }

    /// The next held block into the PCM ring: decoded, upsampled, filtered.
    fn decode_one(&mut self) {
        let at = self.s_tail % STORE;
        let n = adpcm::decode_block(&self.store[at][..usize::from(self.lens[at])], &mut self.block);
        self.s_tail += 1;
        if self.up {
            for i in (0..n).step_by(4) {
                let c = i32::from(i16::from_le_bytes([self.block[i], self.block[i + 1]]));
                let d = i32::from(i16::from_le_bytes([self.block[i + 2], self.block[i + 3]]));
                if let Some((a, b)) = self.pend {
                    for v in [a, (a + 2 * b) / 3, (2 * b + c) / 3] {
                        let v = self.lp[0].run(&LOWPASS[0], v);
                        let v = self.lp[1].run(&LOWPASS[1], v);
                        let v = self.ex.run(v, self.excite);
                        self.put(v);
                    }
                }
                self.pend = Some((c, d));
            }
        } else {
            for i in (0..n).step_by(2) {
                let v = i32::from(i16::from_le_bytes([self.block[i], self.block[i + 1]]));
                self.put(v);
            }
        }
    }

    fn put(&mut self, v: i32) {
        let b = (v as i16).to_le_bytes();
        self.pcm[self.head % PCM] = b[0];
        self.pcm[(self.head + 1) % PCM] = b[1];
        self.head += 2;
    }

    /// Volume as a gain in Q15, squared so the crown's steps sound even (the stock track level is all or nothing
    /// on a real watch: it stays at full).
    fn gain(&self) -> i32 {
        let v = i32::from(self.volume);
        v * v * 32768 / 10000
    }

    /// Decodes what the PCM ring has room for and hands the loader what it takes; then the song that starts at
    /// what was heard is the one shown. true when the song shown changed.
    pub fn feed(&mut self, sink: &mut impl Sink) -> bool {
        if self.live && !self.paused && !self.held && self.error == 0 {
            self.play(sink);
        }
        self.advance(sink)
    }

    fn play(&mut self, sink: &mut impl Sink) {
        if self.buffering {
            if self.stored() < FULL && self.end_at.is_none() {
                return;
            }
            self.buffering = false;
            self.started = true;
        }
        while self.stored() > 0 && PCM - self.pcm_queued() >= UP_PCM {
            self.decode_one();
        }
        if self.stored() == 0 && self.pcm_queued() == 0 {
            if self.end_at.is_none() && sink.queued() == 0 {
                self.buffering = true; // ran dry: fill up again
            }
            return;
        }
        let g = self.gain();
        while self.pcm_queued() > 0 {
            let at = self.tail % PCM;
            let n = self.pcm_queued().min(PCM - at).min(CHUNK) & !1;
            if n == 0 {
                break;
            }
            for i in (0..n).step_by(2) {
                let v = i32::from(i16::from_le_bytes([self.pcm[at + i], self.pcm[at + i + 1]]));
                let v = ((v * g) >> 15) as i16;
                self.scratch[i..i + 2].copy_from_slice(&v.to_le_bytes());
            }
            let r = sink.write(&self.scratch[..n]);
            if r < 0 {
                self.error = r;
                return;
            }
            if r == 0 {
                break;
            }
            self.tail += r as usize & !1;
        }
    }

    /// PCM bytes of the stream the speaker has played: given to the loader and gone from its queue.
    fn heard(&self) -> usize {
        self.tail.saturating_sub(self.lq)
    }

    fn advance(&mut self, sink: &mut impl Sink) -> bool {
        self.lq = sink.queued().max(0) as usize;
        let per = self.per_block();
        let mut changed = false;
        while self.n_marks > 0 && self.marks[0].0 * per <= self.heard() {
            self.now = Some(self.marks[0].1);
            self.start = self.marks[0].0 * per;
            self.marks.copy_within(1.., 0);
            self.n_marks -= 1;
            changed = true;
        }
        if let Some(end) = self.end_at {
            if self.s_tail >= end && self.pcm_queued() == 0 && self.lq == 0 {
                self.end_at = None;
                self.live = false;
                self.asked = false;
                self.now = None;
                changed = true;
            }
        }
        changed
    }

    /// `F next room gen held:u16`, every 4 blocks taken and every 100 ms while a stream runs (a lost one must not stall
    /// the phone). Room: free compressed blocks; held: blocks taken and not heard yet (the phone's song is the
    /// one at `next - held`: with 7 s held, what it sent last is not what plays).
    pub fn credit(&mut self, now: u32, took: bool) -> Option<[u8; 7]> {
        if !self.live {
            return None;
        }
        if took {
            self.untold += 1;
        }
        if self.untold < 4 && now.wrapping_sub(self.told_at) < 100 {
            return None;
        }
        let next = self.next_seq.unwrap_or(0).to_le_bytes();
        let room = (STORE - self.stored()).min(255) as u8;
        let held = (self.s_head - (self.heard() / self.per_block()).min(self.s_head)).min(0xFFFF) as u16;
        let h = held.to_le_bytes();
        Some([b'F', next[0], next[1], room, self.gn, h[0], h[1]])
    }

    /// The credit went out.
    pub fn told(&mut self, now: u32) {
        self.untold = 0;
        self.told_at = now;
    }

    pub fn close(&mut self, sink: &mut impl Sink) {
        if self.opened {
            sink.close();
            self.opened = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    extern crate std;
    use std::vec::Vec;

    /// A loader that takes 1 KiB at a time and plays it at once when asked.
    #[derive(Default)]
    struct Fake {
        got: Vec<i16>,
        open: u32,
        queue: usize,
    }
    impl Sink for Fake {
        fn open(&mut self, _: u8) -> i32 {
            self.open += 1;
            0
        }
        fn write(&mut self, pcm: &[u8]) -> i32 {
            let n = pcm.len().min(1024 - self.queue.min(1024));
            self.got.extend(pcm[..n].chunks_exact(2).map(|c| i16::from_le_bytes([c[0], c[1]])));
            self.queue += n;
            n as i32
        }
        fn queued(&self) -> i32 {
            self.queue as i32
        }
        fn close(&mut self) {}
    }

    /// A block of one value (predictor at it, all codes zero: the output stays near it).
    fn block(seq: u16, v: i16) -> Vec<u8> {
        let mut m = std::vec![b'a'];
        m.extend(seq.to_le_bytes());
        m.extend(v.to_le_bytes());
        m.push(0);
        m.extend([0u8; 122]);
        m
    }

    fn song(pos: u16, dur: u16, title: &str) -> Vec<u8> {
        let mut m = std::vec![b'N'];
        m.extend(pos.to_le_bytes());
        m.extend(dur.to_le_bytes());
        m.push(title.len() as u8);
        m.extend(title.as_bytes());
        m.extend(b"Artist");
        m
    }

    /// The phone: messages as the credit (room) allows, the watch feeding and the loader playing between.
    fn run(p: &mut Player, f: &mut Fake, msgs: &[Vec<u8>], rounds: usize, mut each: impl FnMut(&Player, &Fake)) {
        let mut next = 0;
        for _ in 0..rounds {
            while next < msgs.len() && p.has_room() {
                p.message(&msgs[next], f);
                next += 1;
            }
            p.feed(f);
            f.queue = 0;
            each(p, f);
        }
    }

    #[test]
    fn only_the_stream_asked_for() {
        let (mut p, mut f) = (Player::new(), Fake::default());
        let g = p.want();
        // an old stream's open and blocks: dropped
        p.message(&[b'o', 16, 0, g.wrapping_sub(1)], &mut f);
        p.message(&block(0, 1000), &mut f);
        assert_eq!((p.live, p.stored()), (false, 0));
        p.message(&[b'o', 16, 0, g], &mut f);
        p.message(&song(0, 200, "One"), &mut f);
        for s in 0..60 {
            p.message(&block(s, 1000), &mut f);
        }
        assert_eq!(p.stored(), 60);
        assert!(p.loading());
        let skip = p.want();
        assert_eq!((p.stored(), p.live), (0, false));
        p.message(&block(61, 1000), &mut f);
        assert_eq!(p.stored(), 0);
        p.message(&[b'o', 16, 0, skip], &mut f);
        assert!(p.live);
        assert_eq!(f.open, 1, "the track is opened once");
    }

    #[test]
    fn waits_for_a_full_buffer_then_plays() {
        let (mut p, mut f) = (Player::new(), Fake::default());
        let g = p.want();
        p.message(&[b'o', 16, 0, g], &mut f);
        p.message(&song(0, 200, "One"), &mut f);
        let mut fills = Vec::new();
        for s in 0..FULL as u16 - 1 {
            p.message(&block(s, 1000), &mut f);
            p.feed(&mut f);
            if s % 50 == 0 {
                fills.push(p.fill().unwrap());
            }
        }
        assert!(f.got.is_empty(), "nothing plays before the buffer is full");
        assert!(p.preparing().is_none());
        assert!(fills.windows(2).all(|w| w[0] < w[1]) && fills[0] < 1000, "{fills:?}");
        p.message(&block(FULL as u16 - 1, 1000), &mut f);
        p.feed(&mut f);
        assert!(!f.got.is_empty() && p.fill().is_none() && !p.loading());
        assert_eq!(p.now.unwrap().title.as_str(), "One");
        // volume 70: gain 0.49
        let mid = f.got[f.got.len() / 2];
        assert!((440..=540).contains(&mid), "{mid}");
        // credit: the room left in the store
        let c = p.credit(1000, true).unwrap();
        assert_eq!((c[3] as usize, c[4]), (STORE - p.stored(), g));
        let held = u16::from_le_bytes([c[5], c[6]]) as usize;
        assert!(held >= p.stored() && held <= FULL, "{held}");
    }

    #[test]
    fn preparing_then_buffering() {
        let (mut p, mut f) = (Player::new(), Fake::default());
        let g = p.want();
        assert_eq!((p.preparing(), p.fill()), (Some(0), None));
        p.message(&[b'o', 16, 0, g], &mut f);
        p.message(&[b'r', g.wrapping_sub(1), 90], &mut f); // an old stream's
        p.message(&[b'r', g, 40], &mut f);
        assert_eq!(p.preparing(), Some(40 * 65536 / 100));
        p.message(&block(0, 0), &mut f);
        assert_eq!(p.preparing(), None);
        assert!(p.fill().unwrap() < 1000);
        p.give_up();
        assert!(!p.loading() && p.preparing().is_none() && p.fill().is_none());
        p.message(&[b'o', 16, 0, g], &mut f);
        assert!(!p.live, "the stream given up is not taken");
    }

    #[test]
    fn a_block_sent_again_after_later_ones_fills_its_gap() {
        let (mut p, mut f) = (Player::new(), Fake::default());
        let g = p.want();
        p.message(&[b'o', 16, 0, g], &mut f);
        // block 1 refused, sent again after 2, 3, 4
        for s in [0u16, 2, 3, 4, 1, 5] {
            p.message(&block(s, 100 * (s as i16 + 1)), &mut f);
        }
        assert_eq!((p.stored(), p.lost), (6, 0));
        assert_eq!(p.credit(1000, true).unwrap()[1], 6, "next is 6");
        // in order: the predictors are 100, 200, ... 600
        let pred: Vec<i16> = (0..6).map(|k| i16::from_le_bytes([p.store[k][0], p.store[k][1]])).collect();
        assert_eq!(pred, [100, 200, 300, 400, 500, 600]);
        // a repeat is dropped; a gap wider than EARLY is given up
        p.message(&block(3, 0), &mut f);
        p.message(&block(20, 0), &mut f);
        assert_eq!((p.stored(), p.lost), (7, 14));
    }

    #[test]
    fn songs_change_when_they_play_and_pos_counts() {
        let (mut p, mut f) = (Player::new(), Fake::default());
        let g = p.want();
        p.message(&[b'o', 16, 0, g], &mut f);
        // 2 s of the first song then 1 s of the second (16 kHz × 2 B = 32000 B a s)
        let per_s = 32000 / BLOCK_PCM + 1;
        let mut msgs = std::vec![song(30, 100, "First")];
        for s in 0..3 * per_s as u16 {
            if s == 2 * per_s as u16 {
                msgs.push(song(0, 50, "Second"));
            }
            msgs.push(block(s, 0));
        }
        msgs.push(std::vec![b'z']);
        let mut seen: Vec<std::string::String> = Vec::new();
        run(&mut p, &mut f, &msgs, 3000, |p, _| {
            if let Some(s) = p.now {
                if seen.last().map(|t| t.as_str()) != Some(s.title.as_str()) {
                    seen.push(s.title.as_str().into());
                }
                if s.title.as_str() == "First" {
                    assert!(p.pos() <= 32);
                }
            }
        });
        assert_eq!(seen, ["First", "Second"]);
        // the queue ended: nothing shown, the stream is over
        assert!(p.now.is_none() && !p.live);
    }

    #[test]
    fn runs_dry_then_fills_up_again() {
        let (mut p, mut f) = (Player::new(), Fake::default());
        let g = p.want();
        p.message(&[b'o', 16, 0, g], &mut f);
        let msgs: Vec<_> = (0..STORE as u16).map(|s| block(s, 500)).collect();
        run(&mut p, &mut f, &msgs, 2000, |_, _| {});
        let played = f.got.len();
        assert!(played > 0 && p.refilling(), "the link stopped: waiting for a full buffer");
        assert!(p.fill().unwrap() < 1000);
        // a few blocks: still waiting
        for s in STORE as u16..STORE as u16 + 10 {
            p.message(&block(s, 500), &mut f);
        }
        p.feed(&mut f);
        assert_eq!(f.got.len(), played);
    }

    #[test]
    fn pause_and_volume() {
        let (mut p, mut f) = (Player::new(), Fake::default());
        let g = p.want();
        p.message(&[b'o', 16, 0, g], &mut f);
        for s in 0..STORE as u16 {
            p.message(&block(s, 2000), &mut f);
        }
        p.paused = true;
        for _ in 0..100 {
            p.feed(&mut f);
            f.queue = 0;
        }
        assert!(f.got.is_empty());
        p.paused = false;
        p.volume = 100;
        for _ in 0..100 {
            p.feed(&mut f);
            f.queue = 0;
        }
        assert!((1900..=2100).contains(&f.got[5000]));
    }

    #[test]
    fn upsampled_block_is_three_halves() {
        let (mut p, mut f) = (Player::new(), Fake::default());
        let g = p.want();
        p.message(&[b'o', 16, 1, g], &mut f);
        p.message(&[b'z'], &mut f); // the end: plays without a full buffer
        for s in 0..3 {
            p.message(&block(s, 0), &mut f);
        }
        p.feed(&mut f);
        // the first block's last pair waits for the next one
        assert_eq!(f.got.len() * 2 + p.pcm_queued(), 3 * UP_PCM - 6);
    }
}
