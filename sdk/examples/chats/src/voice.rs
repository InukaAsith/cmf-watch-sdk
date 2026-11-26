//! WhatsApp's voice notes on the watch's speaker. A tap on one asks the phone (`t idx`, as for a picture); the
//! phone sends it decoded and shaped for the speaker, as IMA ADPCM at 8 kHz:
//!   phone → watch: `O` a note starts (what was held goes), `B seq:u16 block` one block (3 + 122 bytes: 244
//!   samples), `E` the last block went.
//!   watch → phone: `F next:u16 room:u8` credit (the next block expected, how many more fit), `x` stop.
//! The blocks wait compressed in what the emoji leave of the icon's memory (STORE of them, ~1.7 s) and are decoded
//! one at a time just before the loader takes them. Playing starts once the store is full (or the note ended),
//! and waits for it to fill again if the link runs dry.

use cmf_wp2_app::{adpcm, Api};

const BLOCK: usize = 125;
const BLOCK_PCM: usize = 488;
const STORE: usize = 56;
/// a credit at least this often while a note plays (one may be lost)
const TELL_MS: u32 = 400;

#[repr(C)]
struct Mem {
    store: [[u8; BLOCK]; STORE],
    pcm: [u8; BLOCK_PCM],
}

#[cfg(target_os = "none")]
const _: () = assert!(!crate::VOICE || core::mem::size_of::<Mem>() <= super::emoji::SPARE, "voice notes outgrow the icon");

#[cfg(target_os = "none")]
fn mem() -> &'static mut Mem {
    unsafe { &mut *(super::emoji::spare() as *mut Mem) }
}

#[cfg(not(target_os = "none"))]
fn mem() -> &'static mut Mem {
    static mut SIM: Mem = Mem { store: [[0; BLOCK]; STORE], pcm: [0; BLOCK_PCM] };
    unsafe { &mut *core::ptr::addr_of_mut!(SIM) }
}

pub struct Voice {
    /// the note playing: its message's idx
    pub idx: Option<u8>,
    next: u16,
    /// blocks stored, blocks decoded (free-running)
    head: usize,
    tail: usize,
    pcm_at: usize,
    pcm_n: usize,
    ended: bool,
    playing: bool,
    open: bool,
    told: Option<(u16, usize)>,
    told_at: u32,
}

impl Voice {
    pub const fn new() -> Self {
        Voice { idx: None, next: 0, head: 0, tail: 0, pcm_at: 0, pcm_n: 0, ended: false, playing: false, open: false, told: None, told_at: 0 }
    }

    fn reset(&mut self) {
        (self.next, self.head, self.tail, self.pcm_n) = (0, 0, 0, 0);
        (self.ended, self.playing, self.told) = (false, false, None);
    }

    /// Asks for note `idx` (another one playing stops).
    pub fn start(&mut self, api: &Api, idx: u8) {
        self.stop(api);
        self.reset();
        self.idx = Some(idx);
        let _ = api.send(&[b't', idx]);
    }

    pub fn stop(&mut self, api: &Api) {
        if self.idx.take().is_some() && !self.ended {
            let _ = api.send(b"x");
        }
        if self.open {
            api.audio_close();
            self.open = false;
        }
    }

    /// A message of ours: true.
    pub fn message(&mut self, m: &[u8]) -> bool {
        match m {
            [b'O'] => {
                if self.idx.is_some() {
                    self.reset();
                }
            }
            [b'B', lo, hi, block @ ..] if block.len() == BLOCK => {
                if self.idx.is_some() && u16::from_le_bytes([*lo, *hi]) == self.next && self.head - self.tail < STORE {
                    mem().store[self.head % STORE].copy_from_slice(block);
                    self.head += 1;
                    self.next = self.next.wrapping_add(1);
                }
            }
            [b'E'] => self.ended = true,
            _ => return false,
        }
        true
    }

    /// How long the app may wait: a note plays on.
    pub fn wait(&self) -> Option<u32> {
        self.idx.map(|_| 20)
    }

    /// The speaker fed, the phone told what fits. true: the note is over (its bubble drawn again).
    pub fn tick(&mut self, api: &Api) -> bool {
        if self.idx.is_none() {
            return false;
        }
        let held = self.head - self.tail;
        if !self.playing && (held >= STORE - 4 || self.ended) {
            self.playing = true;
            if !self.open {
                self.open = api.audio_open(8) == 0;
            }
        }
        if self.playing {
            loop {
                if self.pcm_n == 0 {
                    if self.tail == self.head {
                        break;
                    }
                    let m = mem();
                    self.pcm_n = adpcm::decode_block(&m.store[self.tail % STORE], &mut m.pcm);
                    self.pcm_at = 0;
                    self.tail += 1;
                }
                let r = api.audio_write(&mem().pcm[self.pcm_at..self.pcm_at + self.pcm_n]);
                if r < 0 {
                    // no speaker: over
                    self.stop(api);
                    return true;
                }
                if r == 0 {
                    break;
                }
                self.pcm_at += r as usize;
                self.pcm_n -= r as usize;
            }
            if self.tail == self.head && self.pcm_n == 0 {
                if self.ended {
                    if api.audio_queued() <= 0 {
                        self.stop(api);
                        return true;
                    }
                } else if api.audio_queued() <= 0 {
                    // the link ran dry: fill up again
                    self.playing = false;
                }
            }
        }
        let room = STORE - (self.head - self.tail);
        let now = api.now();
        let due = match self.told {
            None => true,
            Some((_, r)) => r + 8 <= room || now.wrapping_sub(self.told_at) >= TELL_MS,
        };
        if !self.ended && due {
            let n = self.next.to_le_bytes();
            if api.send(&[b'F', n[0], n[1], room as u8]) >= 0 {
                self.told = Some((self.next, room));
                self.told_at = now;
            }
        }
        false
    }
}
