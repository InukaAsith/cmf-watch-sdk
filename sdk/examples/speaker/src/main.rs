#![no_std]
#![no_main]

//! Plays what the phone streams through the watch's speaker (docs/abi-1.4.md, "Audio"; host: `wp2apps play`).
//! Phone → watch: `o khz` open, `a seq:u16 block…` one IMA ADPCM block (cmf_wp2_app::adpcm), `v percent` volume,
//! `t text` the title, `x amount` exciter shimmer (1/128), `c` stop. Watch → phone: `R` ready, `V percent` after a crown turn, `F next:u16 room:u8`
//! credit: the next block it expects and how many more blocks it has room for. The phone sends only that, so the
//! stream follows the watch's own clock (its DAC), not the phone's.
//! The app keeps 1 s of sound ahead of the loader; once that is full it reads no messages, so the phone's sends
//! wait (FULL) instead of piling up: the stream plays at the speaker's pace.

use cmf_wp2_app::{adpcm, Api, BUTTON, CROWN, MSG_MAX, PAUSE, RESUME};

cmf_wp2_app::entry!(main);
cmf_wp2_app::app_icon!();

const BLACK: u16 = 0x0000;
const WHITE: u16 = 0xFFFF;
const GREY: u16 = 0x7BEF;
const RED: u16 = 0xD8C4;

// one decoded block (122 code bytes → 488 bytes of PCM)
const BLOCK_PCM: usize = 4 * (MSG_MAX - 6);
static mut BLOCK: [u8; BLOCK_PCM] = [0; BLOCK_PCM];
// `o khz 1`: blocks come at 2/3 of `khz` (10.67 kHz for 16: what a Mac's BLE link carries) and are upsampled 2 → 3
const UP_PCM: usize = BLOCK_PCM * 3 / 2;
static mut UP: [u8; UP_PCM] = [0; UP_PCM];
// 1 s at 16 kHz ahead of the loader's queue (a real watch's link has slow moments), so the phone's messages are
// always read; a static: audio_write takes only the app block
const RING: usize = 32 * 1024;
/// PCM held back before playing, at the start and after a dropout: a slow moment of the link costs one pause,
/// not a stutter every few blocks. 16 kHz mono i16 = 32 B a ms: 0.4 s.
const PREBUFFER: usize = 12800;
/// 0.8 s before the first sound after open
const PREBUFFER_START: usize = 25600;
static mut PCM: [u8; RING] = [0; RING];

struct Player {
    open: bool,
    volume: u8,
    title: [u8; 40],
    title_len: usize,
    blocks: u32,
    lost: u32,
    next_seq: Option<u16>,
    // free-running byte counts of PCM: decoded in, given to the loader
    head: usize,
    tail: usize,
    error: i32,
    untold: u32,
    told_at: u32,
    buffering: bool,
    up: bool,
    // the last input pair, played once the next pair's first sample is known (linear interpolation)
    pend: Option<(i32, i32)>,
    lp: [Biquad; 2],
    ex: Exciter,
    // 1/128 of the exciter's shimmer: 160 puts 5.5-8 kHz near -23 dB, where the original song has it, `x amount` from the phone
    excite: i32,
    // the first fill after open waits for more: a real link can start slow
    first: bool,
}

fn pcm() -> &'static mut [u8; RING] {
    unsafe { &mut *core::ptr::addr_of_mut!(PCM) }
}

fn block() -> &'static mut [u8; BLOCK_PCM] {
    unsafe { &mut *core::ptr::addr_of_mut!(BLOCK) }
}

/// 4th-order Butterworth low-pass at 4.8 kHz (16 kHz), Q14: removes the mirror image the 2 → 3 upsampling leaves
/// at 5.3-8 kHz, which a real watch played as harsh, noisy treble (recorded 2026-11-21).
const LOWPASS: [[i32; 5]; 2] = [[5708, 11416, 5708, 5390, 1058], [7862, 15724, 7862, 7424, 7640]];

/// Exciter: the stream stops at 5.3 kHz, which sounds like a phone call (recording 8). The 3-8 kHz part is
/// full-wave rectified, which doubles its frequencies, and only what lands above 5.5 kHz (4th-order high-pass) is
/// mixed back in: shimmer that follows the music, where the stream has none.
const HIGHPASS: [[i32; 5]; 2] = [[2059, -4118, 2059, 10296, 2148], [2762, -5524, 2762, 13811, 8475]];
const SPLIT_3K: i32 = 22680; // one-pole low-pass coefficient at 3 kHz, Q15

#[derive(Clone, Copy, Default)]
struct Exciter {
    low: i32,
    hp: [Biquad; 2],
}

impl Exciter {
    /// `amount` in 1/128 of the generated shimmer
    fn run(&mut self, x: i32, amount: i32) -> i32 {
        self.low += ((x - self.low) * SPLIT_3K) >> 15;
        let top = (x - self.low).abs();
        let e = self.hp[0].run(&HIGHPASS[0], top);
        let e = self.hp[1].run(&HIGHPASS[1], e);
        (x + ((e * amount) >> 7)).clamp(-32768, 32767)
    }
}

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

/// 2 → 3 linear upsampling of i16 LE `pcm` into `out` (one pair of delay, carried in `pend`); bytes written.
fn upsample(pend: &mut Option<(i32, i32)>, pcm: &[u8], out: &mut [u8]) -> usize {
    let mut w = 0;
    let mut put = |v: i32, w: &mut usize| {
        out[*w..*w + 2].copy_from_slice(&(v as i16).to_le_bytes());
        *w += 2;
    };
    for pair in pcm.chunks_exact(4) {
        let c = i32::from(i16::from_le_bytes([pair[0], pair[1]]));
        let d = i32::from(i16::from_le_bytes([pair[2], pair[3]]));
        if let Some((a, b)) = *pend {
            put(a, &mut w);
            put((a + 2 * b) / 3, &mut w);
            put((2 * b + c) / 3, &mut w);
        }
        *pend = Some((c, d));
    }
    w
}

impl Player {
    fn block_bytes(&self) -> usize {
        if self.up { UP_PCM } else { BLOCK_PCM }
    }

    fn queued(&self) -> usize {
        self.head - self.tail
    }

    /// Volume as a gain in Q15, on the samples: the stock track level is all or nothing on a real watch
    /// (2026-11-21), so it stays at full and the app scales. Squared, so the crown's steps sound even.
    fn gain(&self) -> i32 {
        let v = i32::from(self.volume);
        v * v * 32768 / 10000
    }

    fn push(&mut self, data: &[u8]) {
        let r = pcm();
        let g = self.gain();
        for (i, pair) in data.chunks_exact(2).enumerate() {
            let mut v = i32::from(i16::from_le_bytes([pair[0], pair[1]]));
            if self.up {
                v = self.lp[0].run(&LOWPASS[0], v);
                v = self.lp[1].run(&LOWPASS[1], v);
                v = self.ex.run(v, self.excite);
            }
            let v = (v * g) >> 15;
            let b = (v as i16).to_le_bytes();
            r[(self.head + 2 * i) % RING] = b[0];
            r[(self.head + 2 * i + 1) % RING] = b[1];
        }
        self.head += data.len();
    }

    /// Hands the loader what it takes; false on an error.
    fn feed(&mut self, api: &Api) -> bool {
        if self.buffering {
            if self.queued() < if self.first { PREBUFFER_START } else { PREBUFFER } {
                return true;
            }
            self.buffering = false;
            self.first = false;
        } else if self.queued() == 0 && api.audio_queued() == 0 {
            self.buffering = true; // ran dry: wait for PREBUFFER again
            return true;
        }
        while self.queued() > 0 {
            let at = self.tail % RING;
            let n = self.queued().min(RING - at);
            let r = api.audio_write(&pcm()[at..at + n]);
            if r < 0 {
                self.error = r;
                self.drop_queued();
                return false;
            }
            if r == 0 {
                break;
            }
            self.tail += r as usize;
        }
        true
    }

    fn drop_queued(&mut self) {
        self.tail = self.head;
        self.buffering = true;
    }
}

fn draw(api: &Api, p: &Player) {
    let (w, h) = (api.width as i16, api.height as i16);
    let centre = |y: i16, s: &str, c: u16| api.text(w / 2 - 4 * s.len() as i16, y, s, c);
    api.rect(0, 0, api.width, api.height, BLACK);
    centre(90, "SPEAKER", RED);
    let title = core::str::from_utf8(&p.title[..p.title_len]).unwrap_or("");
    centre(h / 2 - 40, if title.is_empty() { "Waiting for the phone" } else { title }, WHITE);
    let state = if p.error < 0 { "Speaker unavailable" } else if p.open { "Playing" } else { "Stopped" };
    centre(h / 2 - 10, state, GREY);
    // volume bar
    let (bx, by, bw) = (w / 2 - 100, h / 2 + 40, 200u16);
    api.rect(bx, by, bw, 12, 0x2104);
    api.rect(bx, by, (u32::from(bw) * u32::from(p.volume) / 100) as u16, 12, WHITE);
    let mut line = Line::default();
    let _ = core::fmt::write(&mut line, format_args!("Volume {}%  (crown)", p.volume));
    centre(by + 24, line.as_str(), GREY);
    let mut line = Line::default();
    let _ = core::fmt::write(&mut line, format_args!("{} blocks, {} lost", p.blocks, p.lost));
    centre(h - 110, line.as_str(), 0x4208);
    api.show();
}

struct Line([u8; 40], usize);
impl Default for Line {
    fn default() -> Self {
        Line([0; 40], 0)
    }
}
impl Line {
    fn as_str(&self) -> &str {
        core::str::from_utf8(&self.0[..self.1]).unwrap_or("")
    }
}
impl core::fmt::Write for Line {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        let n = s.len().min(self.0.len() - self.1);
        self.0[self.1..self.1 + n].copy_from_slice(&s.as_bytes()[..n]);
        self.1 += n;
        Ok(())
    }
}

/// One host message; true when the screen should change.
fn handle(api: &Api, p: &mut Player, m: &[u8]) -> bool {
    match m {
        [b'o', khz, mode @ ..] => {
            p.up = mode == [1];
            p.pend = None;
            p.lp = [Biquad::default(); 2];
            p.ex = Exciter::default();
            p.first = true;
            p.error = api.audio_open(*khz);
            p.open = p.error == 0;
            p.next_seq = None;
            p.drop_queued();
            p.blocks = 0;
            p.lost = 0;
            api.audio_volume(100);
            true
        }
        [b'a', s0, s1, block @ ..] if p.open => {
            let seq = u16::from_le_bytes([*s0, *s1]);
            if let Some(want) = p.next_seq {
                let ahead = seq.wrapping_sub(want);
                if ahead >= 0x8000 {
                    return false; // old or repeated: already played
                }
                p.lost += u32::from(ahead);
            }
            p.next_seq = Some(seq.wrapping_add(1));
            let n = adpcm::decode_block(block, self::block());
            if p.up {
                let up = unsafe { &mut *core::ptr::addr_of_mut!(UP) };
                let m = upsample(&mut p.pend, &self::block()[..n], up);
                p.push(&up[..m]);
            } else {
                p.push(&self::block()[..n]);
            }
            p.blocks += 1;
            p.blocks % 64 == 0
        }
        [b'x', amount] => {
            p.excite = i32::from(*amount);
            false
        }
        [b'v', pct] => {
            p.volume = (*pct).min(100);
            true
        }
        [b't', text @ ..] => {
            let n = text.len().min(p.title.len());
            p.title[..n].copy_from_slice(&text[..n]);
            p.title_len = core::str::from_utf8(&p.title[..n]).map_or_else(|e| e.valid_up_to(), |s| s.len());
            true
        }
        [b'c'] => {
            api.audio_close();
            p.open = false;
            p.drop_queued();
            true
        }
        _ => false,
    }
}

/// `F next room` every 4 blocks taken, and every 100 ms while open (a lost one must not stall the phone).
fn credit(api: &Api, p: &mut Player, took: bool) {
    let now = api.now();
    if took {
        p.untold += 1;
    }
    if p.untold < 4 && now.wrapping_sub(p.told_at) < 100 {
        return;
    }
    let next = p.next_seq.unwrap_or(0).to_le_bytes();
    let room = ((RING - p.queued()) / p.block_bytes()).min(255) as u8;
    if api.send(&[b'F', next[0], next[1], room]) == 0 {
        p.untold = 0;
        p.told_at = now;
    }
}

fn main(api: &'static Api) -> i32 {
    let mut p = Player { open: false, volume: 70, title: [0; 40], title_len: 0, blocks: 0, lost: 0, next_seq: None,
                         head: 0, tail: 0, error: 0, untold: 0, told_at: 0, buffering: true, up: false, pend: None,
                         lp: [Biquad::default(); 2], ex: Exciter::default(), excite: 160, first: true };
    let mut msg = [0u8; MSG_MAX];
    api.send(b"R");
    draw(api, &p);
    loop {
        if !p.feed(api) {
            draw(api, &p);
        }
        let mut redraw = false;
        // read the phone while a whole block still fits
        let before = p.blocks;
        while p.queued() + p.block_bytes() <= RING {
            let Some(m) = api.recv(&mut msg) else { break };
            redraw |= handle(api, &mut p, m);
        }
        p.feed(api);
        if p.open {
            let took = p.blocks != before;
            credit(api, &mut p, took);
        }
        if redraw {
            draw(api, &p);
        }
        // 5 ms while sound waits for the loader, else sleep until an event
        let wait = if p.queued() > 0 { 5 } else if p.open { 20 } else { u32::MAX };
        let Some(e) = api.event(wait) else { continue };
        match e.kind {
            BUTTON => {
                api.audio_close();
                return 0;
            }
            CROWN if e.delta != 0 => {
                p.volume = (i16::from(p.volume) + 5 * e.delta.signum()).clamp(0, 100) as u8;
                api.send(&[b'V', p.volume]);
                draw(api, &p);
            }
            // the loader dropped the queue on pause and opens again by itself on resume
            PAUSE => p.drop_queued(),
            RESUME => draw(api, &p),
            _ => {}
        }
    }
}
