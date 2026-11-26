//! Chats state, filled by the phone. Every message ≤ 128 B. Phone → watch:
//!
//! - `L n` the chat list has n rows; `C idx id unread nlen name… plen preview…` one row.
//! - `H id title…` a chat opened: its messages follow and replace what was shown.
//! - `M idx me kind text…` message `idx` (0 = oldest shown); `A me kind text…` one more at the end (the oldest drops
//!   when full). `kind`: 0 text, 1 sticker, 2 photo; for 1 and 2 `text` is the caption or empty.
//! - `i idx <I message>`, `d idx seq tokens…`, `e idx`: the picture of message idx, in pixview's run format
//!   (rle.rs), kept in a pool until the next `H`.
//! - `T text…` more text for the last message (a streamed answer).
//! - `N id` a new message in chat id (buzz). `Q idx text…` quick reply idx.
//! - `M`/`A`'s `me` byte: bit 0 ours; bits 1–2 for ours its state: 0 sent, 1 delivered, 2 read. `S idx state` a
//!   new state for message idx. `P online` the open chat's contact is online (1) or not (0); `H` resets it.
//! - `W text…` the open chat's line under its name ("last seen today at 17:20", "online"); `G part px…` its
//!   picture: 12 × 12 RGB565 LE in 3 parts of 4 rows (96 B each); `H` resets both.
//! - A `C` row with `unread` 255 is a status line ("Syncing..."), not a chat: drawn in red, not opened (a
//!   "Syncing" one is the list's filling bar instead); `unread` 254 is the new-chat row (New chat, New note,
//!   New mail): the + button at the bottom opens it.
//! - `K title…` the list that follows is a picker (whom to write to), under that title; the watch's back asks
//!   the usual list again (`R`). `Y pct` how far the sync is (the bar).
//!
//! Watch → phone: `R` ready, `o id` open chat, `s id text…` send, `t idx` a message tapped (Notes ticks a
//! reminder; WhatsApp sends a sticker's or a photo's picture).

use crate::rle::HEADER;
use cmf_wp2_ui::text::fit;

pub const MAX_CHATS: usize = 12;
pub const MAX_MSGS: usize = 12;
pub const MAX_QUICK: usize = 6;
/// `C`'s unread for a status row
pub const STATUS_ROW: u8 = 255;
/// `C`'s unread for the new-chat row (the + button)
pub const NEW_ROW: u8 = 254;
/// (only WhatsApp gets pictures: the other apps keep a token pool)
pub const POOL: usize = if crate::VOICE { 23 * 1024 } else { 256 };

#[derive(Clone, Copy)]
pub struct Str<const N: usize> {
    b: [u8; N],
    n: u8,
}

impl<const N: usize> Str<N> {
    pub const EMPTY: Self = Str { b: [0; N], n: 0 };

    pub fn set(&mut self, s: &[u8]) {
        let t = fit(s, N).as_bytes();
        self.b[..t.len()].copy_from_slice(t);
        self.n = t.len() as u8;
    }

    pub fn as_str(&self) -> &str {
        fit(&self.b[..self.n as usize], N)
    }

    /// Appends what fits (cut on a char boundary).
    pub fn push(&mut self, s: &[u8]) {
        let at = self.n as usize;
        let t = fit(s, N - at).as_bytes();
        self.b[at..at + t.len()].copy_from_slice(t);
        self.n += t.len() as u8;
    }
}

#[derive(Clone, Copy)]
pub struct Chat {
    pub id: u8,
    pub unread: u8,
    pub name: Str<24>,
    pub preview: Str<40>,
}

#[derive(Clone, Copy)]
pub struct Img {
    pub header: [u8; HEADER],
    pub off: usize,
    pub len: usize,
    seq: u8,
    pub ok: bool,
}

impl Img {
    /// Width and height on screen, px.
    pub fn size(&self) -> (i32, i32) {
        let u = |i: usize| u16::from_le_bytes([self.header[i], self.header[i + 1]]) as i32;
        let k = self.header[5].max(1) as i32;
        (u(1) * k, u(3) * k)
    }
}

#[derive(Clone, Copy)]
pub struct Msg {
    pub me: bool,
    /// ours: 0 sent, 1 delivered, 2 read
    pub state: u8,
    pub kind: u8,
    pub text: Str<120>,
    pub img: Option<Img>,
    /// a picture asked for: 0 not, 1 loading, 2 none came
    pub pic: u8,
    /// the reaction on it (an emoji's code point; 0 none)
    pub react: u32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Effect {
    None,
    List,
    Chat,
    Buzz(u8),
}

pub struct Model {
    pub chats: [Chat; MAX_CHATS],
    pub n_chats: usize,
    pub open: Option<u8>,
    pub title: Str<24>,
    pub msgs: [Msg; MAX_MSGS],
    pub n_msgs: usize,
    pub pool: [u8; POOL],
    used: usize,
    pub quick: [Str<24>; MAX_QUICK],
    pub n_quick: usize,
    /// the open chat's contact is online
    pub online: bool,
    /// the open chat's line under its name
    pub seen: Str<40>,
    /// the open chat's H came (its messages, if any, with it): an empty chat is empty, not loading
    pub headed: bool,
    /// the list is a picker under this title (empty: the usual list)
    pub picker: Str<24>,
    /// how far the phone's sync is, 0..=100
    pub sync: u8,
    /// the open chat's picture, 12 × 12 RGB565, and which of its 3 parts came (bits)
    pub dp: [u16; DP * DP],
    pub dp_parts: u8,
    /// WhatsApp: the list's pictures, as `dp`, by chat id (`g id part px…`); parts 0: a free slot
    pub face_id: [u8; FACES],
    pub face_parts: [u8; FACES],
    pub faces: [[u16; DP * DP]; FACES],
}

/// Pictures kept for the list: WhatsApp's (every row), none in the other apps.
pub const FACES: usize = if crate::VOICE { MAX_CHATS } else { 0 };

/// A message slot whose `M` has not come yet (the phone sends a chat newest first).
pub const NOT_YET: u8 = 0xFE;

/// The chat picture's side, px (shown 4× as a disc).
pub const DP: usize = 12;

const CHAT: Chat = Chat { id: 0, unread: 0, name: Str::EMPTY, preview: Str::EMPTY };
const MSG: Msg = Msg { me: false, state: 0, kind: 0, text: Str::EMPTY, img: None, pic: 0, react: 0 };

impl Model {
    pub const fn new() -> Self {
        Model {
            chats: [CHAT; MAX_CHATS],
            n_chats: 0,
            open: None,
            title: Str::EMPTY,
            msgs: [MSG; MAX_MSGS],
            n_msgs: 0,
            pool: [0; POOL],
            used: 0,
            quick: [Str::EMPTY; MAX_QUICK],
            n_quick: 0,
            online: false,
            seen: Str::EMPTY,
            headed: false,
            picker: Str::EMPTY,
            sync: 0,
            dp: [0; DP * DP],
            dp_parts: 0,
            face_id: [0; FACES],
            face_parts: [0; FACES],
            faces: [[0; DP * DP]; FACES],
        }
    }

    /// The list's picture of chat `id`, once all of it came.
    pub fn face(&self, id: u8) -> Option<&[u16; DP * DP]> {
        (0..FACES).find(|&k| self.face_parts[k] == 7 && self.face_id[k] == id).map(|k| &self.faces[k])
    }

    pub fn chat(&self, id: u8) -> Option<&Chat> {
        self.chats[..self.n_chats].iter().find(|c| c.id == id)
    }

    /// The tokens of a complete picture.
    pub fn tokens(&self, img: &Img) -> &[u8] {
        &self.pool[img.off..img.off + img.len]
    }

    /// Our own reply, shown at once (the phone confirms nothing).
    pub fn append_local(&mut self, text: &[u8]) {
        self.push(1, 0, text);
    }

    fn push(&mut self, me: u8, kind: u8, text: &[u8]) {
        if self.n_msgs == MAX_MSGS {
            self.msgs.copy_within(1.., 0);
            self.n_msgs -= 1;
        }
        let m = &mut self.msgs[self.n_msgs];
        m.me = me & 1 != 0;
        m.state = me >> 1 & 3;
        m.kind = kind;
        m.text.set(text);
        m.img = None;
        m.react = 0;
        self.n_msgs += 1;
    }

    pub fn apply(&mut self, m: &[u8]) -> Effect {
        let Some((&op, a)) = m.split_first() else { return Effect::None };
        let on_open = |s: &Self| if s.open.is_some() { Effect::Chat } else { Effect::None };
        match op {
            b'L' if a.len() == 1 => {
                self.n_chats = (a[0] as usize).min(MAX_CHATS);
                Effect::List
            }
            b'C' if a.len() >= 4 => {
                let (idx, nlen) = (a[0] as usize, a[3] as usize);
                if idx >= MAX_CHATS || a.len() < 5 + nlen {
                    return Effect::None;
                }
                let plen = a[4 + nlen] as usize;
                let c = &mut self.chats[idx];
                c.id = a[1];
                c.unread = a[2];
                c.name.set(&a[4..4 + nlen]);
                c.preview.set(&a[5 + nlen..(5 + nlen + plen).min(a.len())]);
                self.n_chats = self.n_chats.max(idx + 1);
                Effect::List
            }
            b'H' if !a.is_empty() => {
                self.open = Some(a[0]);
                self.headed = true;
                self.online = false;
                self.seen = Str::EMPTY;
                self.dp_parts = 0;
                self.title.set(&a[1..]);
                self.n_msgs = 0;
                self.used = 0;
                // the phone sends the newest first: slots under them not come yet are left out
                for m in self.msgs.iter_mut() {
                    m.kind = NOT_YET;
                }
                Effect::Chat
            }
            b'M' if a.len() >= 3 => {
                let idx = a[0] as usize;
                if idx >= MAX_MSGS {
                    return Effect::None;
                }
                let msg = &mut self.msgs[idx];
                msg.me = a[1] & 1 != 0;
                msg.state = a[1] >> 1 & 3;
                msg.kind = a[2];
                msg.text.set(&a[3..]);
                msg.img = None;
                msg.react = 0;
                self.n_msgs = self.n_msgs.max(idx + 1);
                on_open(self)
            }
            b'X' if !a.is_empty() && (a[0] as usize) < self.n_msgs => {
                // a reaction on a message (empty: none)
                let cp = core::str::from_utf8(&a[1..]).ok().and_then(|t| t.chars().next()).map_or(0, |c| c as u32);
                self.msgs[a[0] as usize].react = cp;
                on_open(self)
            }
            b'A' if a.len() >= 2 => {
                self.push(a[0], a[1], &a[2..]);
                on_open(self)
            }
            b'S' if a.len() == 2 => {
                let idx = a[0] as usize;
                if idx >= self.n_msgs {
                    return Effect::None;
                }
                self.msgs[idx].state = a[1] & 3;
                on_open(self)
            }
            b'P' if a.len() == 1 => {
                self.online = a[0] != 0;
                on_open(self)
            }
            b'K' => {
                self.picker.set(a);
                Effect::List
            }
            b'Y' if a.len() == 1 => {
                self.sync = a[0].min(100);
                Effect::List
            }
            b'W' => {
                self.seen.set(a);
                on_open(self)
            }
            b'G' if a.len() == 1 + DP * 2 * 4 && a[0] < 3 => {
                let at = a[0] as usize * DP * 4;
                for (i, p) in a[1..].chunks_exact(2).enumerate() {
                    self.dp[at + i] = u16::from_le_bytes([p[0], p[1]]);
                }
                self.dp_parts |= 1 << a[0];
                on_open(self)
            }
            b'g' if FACES > 0 && a.len() == 2 + DP * 2 * 4 && a[1] < 3 => {
                // its slot, else a free one, else one whose chat left the list
                let id = a[0];
                let listed = |i: u8| self.chats[..self.n_chats].iter().any(|c| c.id == i);
                let k = (0..FACES)
                    .find(|&k| self.face_parts[k] != 0 && self.face_id[k] == id)
                    .or_else(|| (0..FACES).find(|&k| self.face_parts[k] == 0))
                    .or_else(|| (0..FACES).find(|&k| !listed(self.face_id[k])))
                    .unwrap_or(0);
                if self.face_id[k] != id {
                    self.face_parts[k] = 0;
                }
                self.face_id[k] = id;
                let at = a[1] as usize * DP * 4;
                for (i, p) in a[2..].chunks_exact(2).enumerate() {
                    self.faces[k][at + i] = u16::from_le_bytes([p[0], p[1]]);
                }
                self.face_parts[k] |= 1 << a[1];
                if self.face_parts[k] == 7 && self.open.is_none() { Effect::List } else { Effect::None }
            }
            b'i' if a.len() == 1 + HEADER && a[1] == b'I' => {
                let idx = a[0] as usize;
                if idx < MAX_MSGS {
                    let mut header = [0; HEADER];
                    header.copy_from_slice(&a[1..]);
                    self.msgs[idx].img = Some(Img { header, off: self.used, len: 0, seq: 0, ok: false });
                }
                Effect::None
            }
            b'd' if a.len() >= 2 => {
                let (idx, seq, t) = (a[0] as usize, a[1], &a[2..]);
                let Some(img) = self.msgs.get_mut(idx).and_then(|m| m.img.as_mut()) else { return Effect::None };
                let end = img.off + img.len;
                // a gap, or no room left in the pool: no picture (the bubble says so)
                if seq != img.seq || end != self.used || end + t.len() > POOL {
                    self.msgs[idx].img = None;
                    return Effect::None;
                }
                self.pool[end..end + t.len()].copy_from_slice(t);
                img.len += t.len();
                img.seq = seq.wrapping_add(1);
                self.used += t.len();
                Effect::None
            }
            b'e' if a.len() == 1 => {
                // `e` alone (no `i` before it, or a gap dropped it): the phone has no picture
                if let Some(m) = self.msgs.get_mut(a[0] as usize) {
                    match m.img.as_mut() {
                        Some(img) => {
                            img.ok = true;
                            m.pic = 0;
                        }
                        None => m.pic = 2,
                    }
                }
                on_open(self)
            }
            b'T' => {
                if self.n_msgs > 0 {
                    self.msgs[self.n_msgs - 1].text.push(a);
                }
                on_open(self)
            }
            b'N' if a.len() == 1 => Effect::Buzz(a[0]),
            b'Q' if !a.is_empty() => {
                let idx = a[0] as usize;
                if idx < MAX_QUICK {
                    self.quick[idx].set(&a[1..]);
                    self.n_quick = self.n_quick.max(idx + 1);
                }
                Effect::None
            }
            _ => Effect::None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    extern crate std;
    use std::boxed::Box;
    use std::vec::Vec;

    fn model() -> Box<Model> {
        Box::new(Model::new())
    }

    fn chat_row(idx: u8, id: u8, unread: u8, name: &str, preview: &str) -> Vec<u8> {
        let mut m = std::vec![b'C', idx, id, unread, name.len() as u8];
        m.extend(name.as_bytes());
        m.push(preview.len() as u8);
        m.extend(preview.as_bytes());
        m
    }

    #[test]
    fn chat_list() {
        let mut m = model();
        assert_eq!(m.apply(&[b'L', 2]), Effect::List);
        m.apply(&chat_row(0, 7, 3, "Alex", "see you at 6"));
        m.apply(&chat_row(1, 9, 0, "Mum", "Call me"));
        assert_eq!(m.n_chats, 2);
        assert_eq!(m.chat(7).unwrap().name.as_str(), "Alex");
        assert_eq!(m.chats[0].preview.as_str(), "see you at 6");
        assert_eq!(m.chats[0].unread, 3);
    }

    #[test]
    fn utf8_is_cut_on_a_boundary() {
        let mut s = Str::<4>::EMPTY;
        s.set("abcé".as_bytes()); // é is 2 bytes: 5 in all
        assert_eq!(s.as_str(), "abc");
    }

    #[test]
    fn open_messages_and_append() {
        let mut m = model();
        assert_eq!(m.apply(b"H\x07Alex"), Effect::Chat);
        m.apply(b"M\x00\x00\x00hi");
        m.apply(b"M\x01\x01\x00hello back");
        assert_eq!(m.n_msgs, 2);
        assert!(m.msgs[1].me);
        for i in 0..MAX_MSGS {
            m.apply(&[b'A', 0, 0, b'a' + i as u8]);
        }
        assert_eq!(m.n_msgs, MAX_MSGS);
        assert_eq!(m.msgs[0].text.as_str(), "a");
        assert_eq!(m.msgs[MAX_MSGS - 1].text.as_str(), "l");
    }

    fn header(w: u16, h: u16, scale: u8) -> Vec<u8> {
        let mut v = std::vec![b'I'];
        v.extend(w.to_le_bytes());
        v.extend(h.to_le_bytes());
        v.push(scale);
        v.extend([0u8; 4]);
        v.extend([0u8; 32]);
        v
    }

    #[test]
    fn sticker_into_the_pool() {
        let mut m = model();
        m.apply(b"H\x07Alex");
        m.apply(b"M\x00\x00\x01");
        let mut i = std::vec![b'i', 0];
        i.extend(header(64, 64, 2));
        m.apply(&i);
        m.apply(&[b'd', 0, 0, 0x3F, 0x30]);
        m.apply(&[b'd', 0, 1, 0x3F, 0x30]);
        assert_eq!(m.apply(b"e\x00"), Effect::Chat);
        let img = m.msgs[0].img.unwrap();
        assert!(img.ok);
        assert_eq!(img.size(), (128, 128));
        assert_eq!(m.tokens(&img), [0x3F, 0x30, 0x3F, 0x30]);
    }

    #[test]
    fn a_gap_drops_the_picture() {
        let mut m = model();
        m.apply(b"H\x07Alex");
        m.apply(b"M\x00\x00\x01");
        let mut i = std::vec![b'i', 0];
        i.extend(header(8, 8, 1));
        m.apply(&i);
        m.apply(&[b'd', 0, 1, 0x00]);
        assert!(m.msgs[0].img.is_none());
    }

    #[test]
    fn streamed_text_appends() {
        let mut m = model();
        m.apply(b"H\x01Gemini");
        m.apply(b"A\x00\x00");
        m.apply(b"THello");
        m.apply(b"T world");
        assert_eq!(m.msgs[0].text.as_str(), "Hello world");
    }

    #[test]
    fn states_and_online() {
        let mut m = model();
        m.apply(&[b'H', 3, b'S']);
        m.apply(&[b'M', 0, 1 | 1 << 1, 0, b'h', b'i']);
        m.apply(&[b'A', 0, 0, b'y', b'o']);
        assert!(m.msgs[0].me && m.msgs[0].state == 1);
        assert!(!m.msgs[1].me && m.msgs[1].state == 0);
        assert_eq!(m.apply(&[b'S', 0, 2]), Effect::Chat);
        assert_eq!(m.msgs[0].state, 2);
        assert_eq!(m.apply(&[b'S', 5, 2]), Effect::None);
        m.apply(&[b'P', 1]);
        assert!(m.online);
        m.apply(&[b'H', 4, b'T']);
        assert!(!m.online);
    }

    #[test]
    fn quick_replies_and_buzz() {
        let mut m = model();
        m.apply(b"Q\x00On my way");
        m.apply(b"Q\x01OK");
        assert_eq!(m.n_quick, 2);
        assert_eq!(m.quick[0].as_str(), "On my way");
        assert_eq!(m.apply(b"N\x07"), Effect::Buzz(7));
    }

    #[test]
    fn last_seen_and_picture() {
        let mut m = model();
        m.apply(&[b'H', 3, b'S']);
        m.apply(b"Wlast seen today at 17:20");
        assert_eq!(m.seen.as_str(), "last seen today at 17:20");
        let mut g = std::vec![b'G', 1];
        g.extend(std::iter::repeat_n([0x1F, 0xF8], 48).flatten());
        assert_eq!(m.apply(&g), Effect::Chat);
        assert_eq!(m.dp_parts, 2);
        assert_eq!(m.dp[48], 0xF81F);
        assert_eq!(m.dp[47], 0);
        // a short part is not taken
        assert_eq!(m.apply(&[b'G', 0, 1, 2]), Effect::None);
        m.apply(&[b'H', 4, b'T']);
        assert_eq!((m.dp_parts, m.seen.as_str()), (0, ""));
    }

    #[test]
    fn picker_and_sync() {
        let mut m = model();
        assert_eq!(m.apply(b"KNew chat"), Effect::List);
        assert_eq!(m.picker.as_str(), "New chat");
        assert_eq!(m.apply(&[b'Y', 140]), Effect::List);
        assert_eq!(m.sync, 100);
        m.apply(b"K");
        assert_eq!(m.picker.as_str(), "");
    }

    #[test]
    fn reactions() {
        let mut m = model();
        m.apply(&[b'H', 3, b'S']);
        m.apply(&[b'M', 0, 0, 0, b'h', b'i']);
        m.apply(&[&[b'X', 0][..], "👍".as_bytes()].concat());
        assert_eq!(m.msgs[0].react, 0x1F44D);
        m.apply(&[b'X', 0]);
        assert_eq!(m.msgs[0].react, 0);
        m.apply(&[b'X', 5, b'x']);
    }

    #[test]
    fn faces() {
        let mut m = model();
        m.apply(&[b'C', 0, 9, 0, 1, b'A', 0]);
        assert!(m.face(9).is_none());
        for part in 0..3u8 {
            let mut g = vec![b'g', 9, part];
            g.extend((0..DP * 4).flat_map(|_| [0x1F, 0xF8]));
            m.apply(&g);
        }
        assert_eq!(m.face(9).map(|f| f[143]), Some(0xF81F));
        assert!(m.face(8).is_none());
    }
}
