//! Chats in the Nothing look: black, white, one red accent, dot-matrix headings.
//!
//! Screens: list (cards) → chat (bubbles, stickers, photos; bottom: keyboard · quick replies · dial) → quick replies,
//! QWERTY keyboard (Wear OS style, laid out for the round panel) or the rotary dial (letters round the rim: the crown
//! turns the selection, a tap on the middle types it, a tap on the rim types that letter). Taps are TOUCH_DOWN (its
//! position is the reliable one). The button goes back (own_back), and leaves the app from the list.

use crate::model::{Effect, Model, Msg, MAX_MSGS, STATUS_ROW};
use crate::rle::{self, Decoder};
use cmf_wp2_app::{Api, BUTTON, CROWN, MESSAGE, MSG_MAX, PAUSE, RESUME, TOUCH_DOWN, TOUCH_UP};
use cmf_wp2_ui::color::{rgb565, BLACK, Rgb};
use cmf_wp2_ui::dial::{Act, Dial, Key};
use cmf_wp2_ui::draw::{arc, disc, ring, rrect};
use cmf_wp2_ui::geom::{chord, polar};
use cmf_wp2_ui::haptic::{self, Motor};
use cmf_wp2_ui::motion::isqrt;
use cmf_wp2_ui::font::{self, centre, clip, fitted, wrap};
use cmf_wp2_ui::text::{dots, dots_centre, fit};
use cmf_wp2_ui::{C, Surface};

cmf_wp2_app::entry!(main);

#[path = "emoji.rs"]
mod emoji;
cmf_wp2_app::app_icon!();

static mut MODEL: Model = Model::new();

// Notes: the list and the last notes opened, kept on the watch (one storage blob, STORE_MAX) and shown at once on
// the next start, "Syncing..." until the phone answers. Kept as the phone's own messages, replayed through the model.
const LIST_CAP: usize = 1300;
const NOTE_CAP: usize = 900;
const NOTES_KEPT: usize = 3;

/// Messages, each after its length byte; what does not fit is left out (a long note keeps its first lines).
struct Rec<const N: usize> {
    b: [u8; N],
    n: usize,
    full: bool,
}

impl<const N: usize> Rec<N> {
    const fn new() -> Self {
        Rec { b: [0; N], n: 0, full: false }
    }

    fn clear(&mut self) {
        self.n = 0;
        self.full = false;
    }

    fn push(&mut self, m: &[u8]) {
        if self.full || m.len() > 255 || self.n + 1 + m.len() > N {
            self.full = true;
            return;
        }
        self.b[self.n] = m.len() as u8;
        self.b[self.n + 1..self.n + 1 + m.len()].copy_from_slice(m);
        self.n += 1 + m.len();
    }

    fn set(&mut self, bytes: &[u8]) {
        let n = bytes.len().min(N);
        self.b[..n].copy_from_slice(&bytes[..n]);
        self.n = n;
        self.full = false;
    }

    fn each(&self, mut f: impl FnMut(&[u8])) {
        let mut i = 0;
        while i < self.n {
            let len = self.b[i] as usize;
            if i + 1 + len > self.n {
                break;
            }
            f(&self.b[i + 1..i + 1 + len]);
            i += 1 + len;
        }
    }

    /// The first message (a note's `H id title`).
    fn first(&self) -> &[u8] {
        if self.n == 0 {
            return &[];
        }
        let len = (self.b[0] as usize).min(self.n - 1);
        &self.b[1..1 + len]
    }
}

struct Kept {
    list: Rec<LIST_CAP>,
    notes: [Rec<NOTE_CAP>; NOTES_KEPT],
    ids: [Option<u8>; NOTES_KEPT],
    age: [u32; NOTES_KEPT],
    clock: u32,
    /// the slot the open note goes into
    rec: Option<usize>,
    dirty: bool,
}

static mut KEPT: Kept = Kept {
    list: Rec::new(),
    notes: [const { Rec::new() }; NOTES_KEPT],
    ids: [None; NOTES_KEPT],
    age: [0; NOTES_KEPT],
    clock: 0,
    rec: None,
    dirty: false,
};
static mut STORE: [u8; cmf_wp2_app::STORE_MAX] = [0; cmf_wp2_app::STORE_MAX];

fn kept() -> &'static mut Kept {
    // SAFETY: one app thread
    unsafe { &mut *core::ptr::addr_of_mut!(KEPT) }
}

impl Kept {
    /// A message from the phone, recorded.
    fn keep(&mut self, m: &[u8]) {
        match m.first() {
            Some(b'L') => {
                self.list.clear();
                self.list.push(m);
            }
            Some(b'C' | b'Q') => self.list.push(m),
            Some(b'H') if m.len() >= 2 => {
                let id = m[1];
                let slot = (0..NOTES_KEPT).find(|&i| self.ids[i] == Some(id)).unwrap_or_else(|| {
                    (0..NOTES_KEPT).min_by_key(|&i| if self.ids[i].is_none() { 0 } else { self.age[i] }).unwrap_or(0)
                });
                self.clock += 1;
                self.ids[slot] = Some(id);
                self.age[slot] = self.clock;
                self.notes[slot].clear();
                self.notes[slot].push(m);
                self.rec = Some(slot);
            }
            Some(b'M' | b'A' | b'T') => {
                if let Some(i) = self.rec {
                    self.notes[i].push(m);
                }
            }
            _ => return,
        }
        self.dirty = true;
    }

    /// The kept note `id`, if its title is still the list's `name` (ids can change when the phone app restarts).
    fn note(&self, id: u8, name: &str) -> Option<usize> {
        (0..NOTES_KEPT).find(|&i| self.ids[i] == Some(id) && self.notes[i].first().get(2..) == Some(name.as_bytes()))
    }

    /// `K 1`, the list (u16 length, records), then per slot its id (255: none), length and records.
    fn save(&mut self, api: &Api) {
        if !self.dirty {
            return;
        }
        // SAFETY: one app thread
        let out = unsafe { &mut *core::ptr::addr_of_mut!(STORE) };
        let mut n = 0;
        let mut put = |b: &[u8], n: &mut usize| {
            out[*n..*n + b.len()].copy_from_slice(b);
            *n += b.len();
        };
        put(&[b'K', 1], &mut n);
        put(&(self.list.n as u16).to_le_bytes(), &mut n);
        put(&self.list.b[..self.list.n], &mut n);
        for i in 0..NOTES_KEPT {
            put(&[self.ids[i].unwrap_or(255)], &mut n);
            put(&(self.notes[i].n as u16).to_le_bytes(), &mut n);
            put(&self.notes[i].b[..self.notes[i].n], &mut n);
        }
        let _ = api.save(&out[..n]);
        self.dirty = false;
    }

    /// What the last run kept; whether there was a list.
    fn load(&mut self, api: &Api) -> bool {
        // SAFETY: one app thread
        let buf = unsafe { &mut *core::ptr::addr_of_mut!(STORE) };
        let n = api.load(buf);
        if n < 4 || buf[0] != b'K' || buf[1] != 1 {
            return false;
        }
        let d = &buf[..n as usize];
        let mut at = 2;
        let take = |at: &mut usize| -> Option<&[u8]> {
            let len = u16::from_le_bytes([*d.get(*at)?, *d.get(*at + 1)?]) as usize;
            let b = d.get(*at + 2..*at + 2 + len)?;
            *at += 2 + len;
            Some(b)
        };
        let Some(list) = take(&mut at) else { return false };
        self.list.set(list);
        for i in 0..NOTES_KEPT {
            let Some(&id) = d.get(at) else { break };
            at += 1;
            let Some(b) = take(&mut at) else { break };
            self.ids[i] = (id != 255).then_some(id);
            self.age[i] = i as u32;
            self.notes[i].set(b);
        }
        self.clock = NOTES_KEPT as u32;
        self.list.n > 0
    }
}

/// Draws through the Api; body text in Inter (cmf_wp2_ui::font), not the 8×8 font.
#[derive(Clone, Copy)]
struct Dot(&'static Api);

impl Surface for Dot {
    fn rect(&mut self, x: i32, y: i32, w: i32, h: i32, c: u16) {
        let mut a = self.0;
        Surface::rect(&mut a, x, y, w, h, c);
    }

    fn text(&mut self, x: i32, y: i32, s: &str, c: u16) {
        font::draw(self, x, y, s, c);
    }
}

/// The panel between rows `.1` and `.2`: a scroll redraws the feed or list without touching the heading or buttons.
struct Band(&'static Api, i32, i32);

impl Surface for Band {
    fn rect(&mut self, x: i32, y: i32, w: i32, h: i32, c: u16) {
        let (y0, y1) = (y.max(self.1), (y + h).min(self.2));
        if y1 > y0 && w > 0 {
            let mut a = self.0;
            Surface::rect(&mut a, x, y0, w, y1 - y0, c);
        }
    }

    fn text(&mut self, x: i32, y: i32, s: &str, c: u16) {
        font::draw(self, x, y, s, c);
    }
}

/// Black around a box drawn over the last frame: its rows left and right of it, and outside its rounded corners
/// (rrect leaves those untouched).
fn surround(s: &mut impl Surface, x: i32, y: i32, w: i32, h: i32, r: i32) {
    Surface::rect(s, 0, y, x, h, 0);
    Surface::rect(s, x + w, y, 466 - x - w, h, 0);
    let r = r.min(h / 2).min(w / 2);
    for dy in 0..r {
        let d = r - dy;
        let off = r - isqrt(r * r - d * d);
        if off > 0 {
            for yy in [y + dy, y + h - 1 - dy] {
                Surface::rect(s, x, yy, off, 1, 0);
                Surface::rect(s, x + w - off, yy, off, 1, 0);
            }
        }
    }
}

fn model() -> &'static mut Model {
    // one app thread
    unsafe { &mut *core::ptr::addr_of_mut!(MODEL) }
}

// --- the Nothing palette
const WHITE: Rgb = Rgb(0xFF, 0xFF, 0xFF);
const RED: Rgb = Rgb(0xD7, 0x19, 0x21);
const CARD: Rgb = Rgb(0x17, 0x17, 0x17);
const KEYC: Rgb = Rgb(0x26, 0x26, 0x26);
const W: u16 = 0xFFFF;
const SOFT: u16 = rgb565(0xC8, 0xC8, 0xC8);
const GREY: u16 = rgb565(0x80, 0x80, 0x80);
const DIM: u16 = rgb565(0x3A, 0x3A, 0x3A);
const GREEN: Rgb = Rgb(0x25, 0xD3, 0x66);

// --- list
const HEAD_H: i32 = 96;
const LIST_TOP: i32 = 104;
const CARD_H: i32 = 84;
const PITCH: i32 = 94;
// --- chat
const FEED_TOP: i32 = 80;
const FEED_END: i32 = 384;
const BUBBLE_X0: i32 = 76;
const BUBBLE_X1: i32 = 390;
const LINE: i32 = 25;
/// A loading picture's bubble width, and the spinner's frame time.
const SPIN_W: i32 = 64;
const SPIN_MS: u32 = 70;
/// Widest text line in a bubble, px.
const TEXT_W: i32 = 262;
const GAP: i32 = 10;
const BTN_Y: i32 = 424;
/// Dial letters per crown detent, in 1/100s: a slow turn, a fast spin (ui.crown).
/// Moved more than this between down and up: a drag, not a tap.
const DRAG: i32 = 14;
/// The space bar under the dial's centre letter: top and height.
const BTN_X: [i32; 3] = [C - 72, C, C + 72];
// --- quick replies
const Q_TOP: i32 = 100;
const Q_PITCH: i32 = 66;
// --- typing
const TYPED_MAX: usize = 100;
const KH: i32 = 58;
const ROW_Y: [i32; 4] = [184, 242, 300, 358];
// --- dial
/// The two dials: `#` on the rim switches between them.
/// Held this long, a dial letter types as a capital.

#[derive(Clone, Copy, PartialEq)]
enum Screen {
    List,
    Chat,
    Quick,
    Qwerty,
    Dial,
    /// the watch's emoji, a page of EM_PAGE at a time
    Emoji,
}

#[derive(Clone, Copy, PartialEq)]
enum K {
    Ch(u8),
    Shift,
    Del,
    Sym,
    Space,
    Send,
    /// to the emoji page and back
    Emoji,
}

/// The emoji page: EM_COLS × EM_ROWS cells of EM_CELL px from EM_TOP, the keys row below.
const EM_COLS: usize = 6;
const EM_ROWS: usize = 3;
const EM_PAGE: usize = EM_COLS * EM_ROWS;
const EM_CELL: i32 = 58;
const EM_TOP: i32 = 166;

/// The emoji page's bottom row.
fn each_emoji_key(mut f: impl FnMut(K, i32, i32, i32)) {
    let keys = [(K::Emoji, 62), (K::Del, 52), (K::Space, 90), (K::Send, 72)];
    let mut x = C - keys.iter().map(|k| k.1).sum::<i32>() / 2;
    for (k, w) in keys {
        f(k, x, ROW_Y[3], w);
        x += w;
    }
}

/// One keyboard row: up to 10 keys `(key, width)`, centred.
struct Row {
    keys: [(K, i32); 10],
    n: usize,
}

fn chars_row(s: &[u8], w: i32) -> Row {
    let mut keys = [(K::Space, 0); 10];
    for (i, &c) in s.iter().enumerate() {
        keys[i] = (K::Ch(c), w);
    }
    Row { keys, n: s.len() }
}

fn row(r: usize, sym: bool) -> Row {
    match (r, sym) {
        (0, false) => chars_row(b"qwertyuiop", 44),
        (1, false) => chars_row(b"asdfghjkl", 44),
        (0, true) => chars_row(b"1234567890", 44),
        (1, true) => chars_row(b"@#&-+()/'", 44),
        (2, _) => {
            let mut row = chars_row(if sym { b"\"*:;!?," } else { b"zxcvbnm" }, 42);
            row.keys.copy_within(0..7, 1);
            row.keys[0] = (if sym { K::Ch(b'.') } else { K::Shift }, 46);
            row.keys[8] = (K::Del, 46);
            row.n = 9;
            row
        }
        _ => {
            let mut row = chars_row(b"", 0);
            if emoji::all().is_empty() {
                row.keys[0] = (K::Sym, 62);
                row.keys[1] = (K::Space, 122);
                row.keys[2] = (K::Send, 72);
                row.n = 3;
            } else {
                row.keys[0] = (K::Sym, 60);
                row.keys[1] = (K::Emoji, 50);
                row.keys[2] = (K::Space, 100);
                row.keys[3] = (K::Send, 66);
                row.n = 4;
            }
            row
        }
    }
}

/// Every key with its rect, for drawing and for hit tests.
fn each_key(sym: bool, mut f: impl FnMut(K, i32, i32, i32)) {
    for (r, &y) in ROW_Y.iter().enumerate() {
        let row = row(r, sym);
        let total: i32 = row.keys[..row.n].iter().map(|k| k.1).sum();
        let mut x = C - total / 2;
        for &(k, w) in &row.keys[..row.n] {
            f(k, x, y, w);
            x += w;
        }
    }
}

struct Ui {
    api: &'static Api,
    screen: Screen,
    list_scroll: i32,
    chat_scroll: i32,
    chat_max: i32,
    quick_scroll: i32,
    typed: [u8; TYPED_MAX],
    n_typed: usize,
    sym: bool,
    shift: bool,
    /// a key drawn pressed, to draw plain again at this time
    flash: Option<(K, u32)>,
    /// the emoji page shown
    em_page: usize,
    /// the rotary keyboard (cmf_wp2_ui::dial)
    dial: Dial,
    motor: Motor,
    /// where a touch on a scrolling screen went down
    down: Option<(i32, i32)>,
    /// Gemini: a question sent, no answer yet: when, and whether it was sent again
    pending: Option<(u32, u8)>,
    /// Gemini: the phone has the question, the answer has not started
    thinking: bool,
    asked: [u8; MSG_MAX],
    asked_n: usize,
    /// Notes: the kept list is shown, the phone has not answered yet
    syncing: bool,
    /// a picture loading: the spinner's next frame and its angle
    spin: (u32, u16),
    /// when the last frame was shown; the phone's messages (a chat or a picture coming in pieces) redraw at most
    /// once in FRAME_MS, the rest waits in `due` (the panel takes a frame from the buffer we draw into)
    shown_at: u32,
    due: bool,
    /// when the list was last asked for: back on the list, and every LIST_MS while it is open, the phone sends it
    /// again (new chats while the app stays open; the phone may have gone quiet meanwhile)
    list_ask: u32,
}

const LIST_MS: u32 = 30_000;

/// Chats and Mail ask for their list again; Notes keep their own, Gemini has none from the phone.
const RELIST: bool = !crate::PLAIN && !crate::ASSISTANT;
/// WhatsApp also every LIST_MS while the list is open (Mail's phone side fetches the server for each ask: on return
/// only).
const PERIODIC: bool = RELIST && matches!(crate::TITLE.as_bytes(), b"WHATSAPP");

/// The least time between two frames that messages bring.
const FRAME_MS: u32 = 80;

fn main(api: &'static Api) -> i32 {
    api.own_back(true);
    emoji::init(api);
    let mut ui = Ui {
        api,
        screen: Screen::List,
        list_scroll: 0,
        chat_scroll: 0,
        chat_max: 0,
        quick_scroll: 0,
        typed: [0; TYPED_MAX],
        n_typed: 0,
        sym: false,
        em_page: 0,
        shift: true,
        flash: None,
        dial: Dial::new(),
        motor: Motor::new(api),
        down: None,
        pending: None,
        thinking: false,
        asked: [0; MSG_MAX],
        asked_n: 0,
        syncing: false,
        spin: (0, 0),
        shown_at: 0,
        due: false,
        list_ask: api.now(),
    };
    if crate::PLAIN && kept().load(api) {
        // the last run's list at once, then the phone's
        kept().list.each(|m| {
            model().apply(m);
        });
        ui.syncing = true;
    }
    ui.draw();
    let _ = api.send(b"R");
    // the phone may still be starting (iOS relaunching CMFX loses the first ask): ask again every 3 s until it
    // answers, at most 20 times
    const RETRY_MS: u32 = 3000;
    /// Gemini: no word of an answer after this long, ask again (once), then say the phone did not answer
    const ASK_AGAIN_MS: u32 = 5000;
    /// times a question is sent again before the watch says the phone did not answer
    const ASK_TIMES: u8 = 3;
    let mut retry: Option<(u32, u8)> = Some((api.now().wrapping_add(RETRY_MS), 20));
    let mut msg = [0u8; MSG_MAX];
    // an event read ahead while crown detents were merged
    let mut ahead: Option<cmf_wp2_app::Event> = None;
    loop {
        let now = api.now();
        let flash = ui.flash.map(|(_, t)| t.wrapping_sub(now).min(1000));
        let hold = if ui.screen == Screen::Dial { ui.dial.hold_wait(now) } else { None };
        let again = retry.map(|(t, _)| t.wrapping_sub(now).min(RETRY_MS));
        let asked = ui.pending.map(|(t, _)| t.wrapping_add(ASK_AGAIN_MS).wrapping_sub(now).min(ASK_AGAIN_MS));
        let spin = ui.spinning().then(|| {
            let due = ui.spin.0.wrapping_sub(now);
            if due == 0 || due > SPIN_MS {
                ui.spin_frame();
                ui.spin.0 = now.wrapping_add(SPIN_MS);
            }
            SPIN_MS
        });
        if ui.due && now.wrapping_sub(ui.shown_at) >= FRAME_MS {
            ui.refresh();
        }
        if PERIODIC && ui.screen == Screen::List && now.wrapping_sub(ui.list_ask) >= LIST_MS {
            ui.ask_list();
        }
        let relist = (PERIODIC && ui.screen == Screen::List)
            .then(|| LIST_MS.saturating_sub(now.wrapping_sub(ui.list_ask)).max(1));
        let paced = ui.due.then(|| FRAME_MS.saturating_sub(now.wrapping_sub(ui.shown_at)).max(1));
        let wait = [ui.motor.wait(), flash, hold, again, asked, spin, paced, relist].into_iter().flatten().min().unwrap_or(u32::MAX);
        let mut e = match ahead.take() {
            Some(e) => Some(e),
            None => api.event(wait),
        };
        // crown detents queued while the last frame was drawn become one step: no backlog, no stutter
        if let Some(c) = e.as_mut().filter(|c| c.kind == CROWN && ui.screen != Screen::Dial) {
            while let Some(n) = api.event(0) {
                if n.kind == CROWN && (n.delta > 0) == (c.delta > 0) {
                    c.delta = c.delta.saturating_add(n.delta);
                } else {
                    ahead = Some(n);
                    break;
                }
            }
        }
        ui.motor.update();
        if let Some((t, left)) = retry {
            if api.now().wrapping_sub(t) as i32 >= 0 {
                let _ = api.send(b"R");
                retry = (left > 1).then(|| (api.now().wrapping_add(RETRY_MS), left - 1));
            }
        }
        if let Some((t, again)) = ui.pending {
            if api.now().wrapping_sub(t) >= ASK_AGAIN_MS {
                if again < ASK_TIMES {
                    // the phone may have been asleep and lost it (iOS relaunching CMFX): again
                    let _ = api.send(&ui.asked[..ui.asked_n]);
                    ui.pending = Some((api.now(), again + 1));
                } else {
                    ui.pending = None;
                    let mut m = [0u8; 64];
                    let t = b"No answer from the phone. Is CMFX open?";
                    m[..3].copy_from_slice(&[b'A', 0, 0]);
                    let n = t.len().min(61);
                    m[3..3 + n].copy_from_slice(&t[..n]);
                    model().apply(&m[..3 + n]);
                    ui.motor.play(haptic::DOUBLE);
                    if ui.screen == Screen::Chat {
                        ui.draw();
                    }
                }
            }
        }
        if let Some((k, t)) = ui.flash {
            if api.now().wrapping_sub(t) as i32 >= 0 {
                ui.flash = None;
                if matches!(ui.screen, Screen::Qwerty | Screen::Emoji) {
                    ui.draw_key(k, false);
                    api.show();
                }
            }
        }
        if ui.screen == Screen::Dial && ui.dial.tick(&mut Dot(api), api.now()) == Act::Show {
            // held: the letter becomes a capital, shown in the middle
            ui.motor.play(haptic::TICK);
            api.show();
        }
        let Some(e) = e else { continue };
        match e.kind {
            TOUCH_UP if ui.screen == Screen::Dial => {
                let a = ui.dial.up(&mut Dot(api), e.x as i32, e.y as i32);
                ui.dial_act(a);
            }
            BUTTON => {
                ui.motor.off();
                ui.dial.cancel();
                match ui.screen {
                    Screen::List => {
                        if crate::PLAIN {
                            kept().save(api);
                        }
                        return 0;
                    }
                    Screen::Chat => ui.go(Screen::List),
                    Screen::Quick | Screen::Qwerty | Screen::Dial => ui.go(Screen::Chat),
                    Screen::Emoji => ui.go(Screen::Qwerty),
                }
            }
            PAUSE => {
                ui.motor.off();
                if crate::PLAIN {
                    kept().save(api);
                }
                loop {
                    match api.event(u32::MAX) {
                        Some(e) if e.kind == RESUME => break,
                        Some(e) if e.kind == BUTTON => return 0,
                        _ => {}
                    }
                }
            }
            CROWN => ui.crown(e.delta),
            TOUCH_DOWN if ui.scrolls() => ui.down = Some((e.x as i32, e.y as i32)),
            TOUCH_UP if ui.scrolls() => {
                if let Some((x, y)) = ui.down.take() {
                    let dy = e.y as i32 - y;
                    if dy.abs() > DRAG {
                        ui.drag(dy);
                    } else {
                        ui.tap(x, y);
                    }
                }
            }
            TOUCH_DOWN => ui.tap(e.x as i32, e.y as i32),
            MESSAGE => {
                retry = None;
                let waiting = ui.pending.is_some() || ui.thinking;
                // any message: the phone is there (no asking again); Gemini's own words: the answer has started
                ui.pending = None;
                let mut answer = false;
                let (mut list, mut chat, mut head) = (false, false, false);
                if ui.syncing {
                    // the phone answered: its list replaces the kept one
                    ui.syncing = false;
                    list = true;
                }
                while let Some(m) = api.recv(&mut msg) {
                    if m.first() == Some(&b'T') || (m.first() == Some(&b'A') && m.get(1) == Some(&0)) {
                        answer = true;
                    }
                    if crate::PLAIN {
                        kept().keep(m);
                    }
                    if m.first() == Some(&b'P') {
                        // the online dot is in the header
                        head = true;
                    }
                    match model().apply(m) {
                        Effect::List => list = true,
                        Effect::Chat => chat = true,
                        Effect::Buzz(_) => ui.motor.play(haptic::DOUBLE),
                        Effect::None => {}
                    }
                }
                ui.thinking = waiting && !answer;
                if waiting && answer {
                    // the answer is coming: the header says Gemini again
                    if ui.screen == Screen::Chat {
                        ui.draw();
                    }
                } else if head && ui.screen == Screen::Chat {
                    ui.draw();
                } else if (list && ui.screen == Screen::List) || (chat && ui.screen == Screen::Chat) {
                    if api.now().wrapping_sub(ui.shown_at) >= FRAME_MS {
                        ui.refresh();
                    } else {
                        ui.due = true;
                    }
                }
            }
            _ => {}
        }
    }
}

/// A dotted rule across the panel at `y`.
fn rule(s: &mut impl Surface, y: i32) {
    let (x0, x1) = chord(y, y + 2, 40);
    let mut x = x0;
    while x < x1 {
        Surface::rect(s, x, y, 2, 2, DIM);
        x += 8;
    }
}

/// A heading in dots with the red dot after it.
fn heading(s: &mut impl Surface, y: i32, text: &str, pitch: i32) {
    let w = text.chars().count() as i32 * 8 * pitch;
    dots(s, C - w / 2, y, text, pitch, pitch - 1, W);
    disc(s, C + w / 2 + 8, y + 7 * pitch - 3, 4, RED, BLACK);
}

fn upper(s: &str, out: &mut [u8; 24]) -> usize {
    let mut n = 0;
    for b in s.bytes().take(24) {
        out[n] = b.to_ascii_uppercase();
        n += 1;
    }
    n
}

impl Ui {
    /// The phone's list again (`R`: it answers with the chats as they are now).
    fn ask_list(&mut self) {
        self.list_ask = self.api.now();
        let _ = self.api.send(b"R");
    }

    fn go(&mut self, to: Screen) {
        if RELIST && to == Screen::List && self.screen != Screen::List {
            self.ask_list();
        }
        self.screen = to;
        match to {
            Screen::Chat => self.chat_scroll = 0,
            Screen::Quick => self.quick_scroll = 0,
            Screen::Emoji => self.flash = None,
            Screen::Qwerty | Screen::Dial => {
                self.sym = false;
                self.shift = self.n_typed == 0;
                self.flash = None;
                self.dial.shift = self.shift;
                self.dial.cancel();
            }
            Screen::List => {}
        }
        self.draw();
    }

    fn draw(&mut self) {
        let mut s = Dot(self.api);
        // the list and a chat clear each part as they draw it (heading, cards or feed, buttons): no frame of a
        // black panel, which flickers
        if !matches!(self.screen, Screen::List | Screen::Chat) {
            Surface::rect(&mut s, 0, 0, 466, 466, 0);
        }
        self.due = false;
        match self.screen {
            Screen::List => self.draw_list(),
            Screen::Chat => self.draw_chat(),
            Screen::Quick => self.draw_quick(),
            Screen::Qwerty => self.draw_qwerty(),
            Screen::Dial => self.draw_dial(),
            Screen::Emoji => self.draw_emoji(),
        }
        self.api.show();
        self.shown_at = self.api.now();
    }

    // --- list

    fn list_max(&self) -> i32 {
        (model().n_chats as i32 * PITCH - (466 - LIST_TOP - 30)).max(0)
    }

    fn draw_list(&self) {
        self.list_cards();
        let mut s = Dot(self.api);
        // the heading over whatever scrolled under it, and the scroll dots on the right rim
        Surface::rect(&mut s, 0, 0, 466, HEAD_H, 0);
        heading(&mut s, 34, crate::TITLE, 3);
        self.unread_line(&mut s);
        self.scroll_dots(self.list_scroll, self.list_max());
    }

    fn unread_line(&self, s: &mut Dot) {
        let m = model();
        if self.syncing {
            centre(s, C, 70, "Syncing...", RED.rgb565());
            return;
        }
        let unread = m.chats[..m.n_chats].iter().filter(|c| c.unread > 0 && c.unread != STATUS_ROW).count();
        if unread > 0 {
            let mut n = [0u8; 11];
            let t = cmf_wp2_ui::text::int(unread as i32, &mut n);
            let w = font::width(t) + font::width(" unread");
            let x = font::draw(s, C - w / 2, 70, t, GREY);
            font::draw(s, x, 70, " unread", GREY);
        }
    }

    /// The cards, each over the last frame's, the gaps cleared: no black frame between (it flickers).
    fn list_cards(&self) {
        let mut s = Band(self.api, HEAD_H, 466);
        let m = model();
        Surface::rect(&mut s, 0, HEAD_H, 466, LIST_TOP - self.list_scroll - HEAD_H, 0);
        for i in 0..m.n_chats {
            let y = LIST_TOP + i as i32 * PITCH - self.list_scroll;
            if y + PITCH < HEAD_H || y > 466 {
                continue;
            }
            Surface::rect(&mut s, 0, y + CARD_H, 466, PITCH - CARD_H, 0);
            let (x0, x1) = chord(y.max(HEAD_H), (y + CARD_H).min(466), 18);
            if x1 - x0 < 160 {
                Surface::rect(&mut s, 0, y, 466, CARD_H, 0);
                continue;
            }
            let c = &m.chats[i];
            // a status line (Syncing...): red, no badge
            let status = c.unread == STATUS_ROW;
            let unread = if status { 0 } else { c.unread };
            surround(&mut s, x0, y, x1 - x0, CARD_H, CARD_H / 2);
            rrect(&mut s, x0, y, x1 - x0, CARD_H, CARD_H / 2, CARD, BLACK);
            let (ax, ay) = (x0 + 46, y + CARD_H / 2);
            ring(&mut s, ax, ay, 25, 27, if status { RED } else if unread > 0 { WHITE } else { Rgb(0x70, 0x70, 0x70) }, CARD);
            let mut b = [0u8; 4];
            let ch = c.name.as_str().chars().next().unwrap_or('?').to_ascii_uppercase();
            dots_centre(&mut s, ax, ay - 12, ch.encode_utf8(&mut b), 3, 2, W);
            let tx = x0 + 86;
            let right = x1 - if unread > 0 { 52 } else { 24 };
            let ink = if status { RED.rgb565() } else if unread > 0 { W } else { SOFT };
            fitted(&mut s, tx, y + 16, c.name.as_str(), right - tx, ink);
            fitted(&mut s, tx, y + 46, c.preview.as_str(), x1 - 30 - tx, GREY);
            if unread > 0 {
                disc(&mut s, x1 - 32, y + 26, 13, RED, CARD);
                let mut n = [0u8; 11];
                centre(&mut s, x1 - 32, y + 18, cmf_wp2_ui::text::int(unread.min(9) as i32, &mut n), W);
            }
        }
        let end = LIST_TOP + m.n_chats as i32 * PITCH - self.list_scroll;
        Surface::rect(&mut s, 0, end.max(HEAD_H), 466, 466 - end.max(HEAD_H), 0);
        if m.n_chats == 0 {
            centre(&mut s, C, C - 8, "Waiting for phone", GREY);
        }
    }

    /// Seven dots on the right rim; the lit one shows how far down the list is.
    fn scroll_dots(&self, at: i32, max: i32) {
        if max == 0 {
            return;
        }
        let mut s = Dot(self.api);
        let lit = (at * 6 + max / 2) / max;
        for i in 0..7 {
            let a = (65536 - 6 * 512 + i * 1024) as u16;
            let (x, y) = polar(C - 12, a);
            let c = if i == lit { WHITE } else { Rgb(0x40, 0x40, 0x40) };
            if i != lit {
                Surface::rect(&mut s, x - 4, y - 4, 9, 9, 0);
            }
            disc(&mut s, x, y, if i == lit { 4 } else { 3 }, c, BLACK);
        }
    }

    // --- chat

    fn draw_chat(&mut self) {
        self.feed();
        let mut s = Dot(self.api);
        let m = model();
        Surface::rect(&mut s, 0, 0, 466, FEED_TOP, 0);
        let name = if crate::ASSISTANT {
            if self.pending.is_some() || self.thinking { "Thinking..." } else { "Gemini" }
        } else {
            m.open.and_then(|id| m.chat(id)).map(|c| c.name.as_str()).unwrap_or(m.title.as_str())
        };
        if crate::PLAIN {
            // a note's title, as written
            let (t, _) = clip(name, 240);
            centre(&mut s, C, 30, t, W);
            rule(&mut s, 66);
            Surface::rect(&mut s, 0, FEED_END, 466, 466 - FEED_END, 0);
            return self.chat_buttons(&mut s);
        }
        let mut up = [0u8; 24];
        let mut n = upper(name, &mut up);
        // a long name: its first word, if that fits the round top (Priya Perera → PRIYA)
        if n > 9 {
            if let Some(sp) = up[..n].iter().position(|&c| c == b' ').filter(|&sp| sp >= 2) {
                n = sp;
            }
        }
        let (shown, _) = cmf_wp2_ui::text::clip(fit(&up[..n], 24), 9);
        heading(&mut s, 26, shown, 3);
        if m.online {
            // online: a green dot before the name
            let w = shown.chars().count() as i32 * 24;
            disc(&mut s, C - w / 2 - 16, 37, 7, GREEN, BLACK);
        }
        rule(&mut s, 66);
        Surface::rect(&mut s, 0, FEED_END, 466, 466 - FEED_END, 0);
        self.chat_buttons(&mut s);
    }

    /// The messages, each over the last frame's, the gaps between cleared; the heading and buttons left alone.
    fn chat_buttons(&self, s: &mut Dot) {
        // keyboard · quick replies · dial
        ring(s, BTN_X[0], BTN_Y, 24, 26, WHITE, BLACK);
        centre(s, BTN_X[0], BTN_Y - 8, "Aa", W);
        disc(s, BTN_X[1], BTN_Y, 28, RED, BLACK);
        for dx in [-10, 0, 10] {
            Surface::rect(s, BTN_X[1] + dx - 2, BTN_Y - 2, 4, 4, W);
        }
        ring(s, BTN_X[2], BTN_Y, 24, 26, WHITE, BLACK);
        ring(s, BTN_X[2], BTN_Y, 10, 12, WHITE, BLACK);
        disc(s, BTN_X[2], BTN_Y - 17, 3, RED, BLACK);
    }

    /// Where the newest message ends: the feed's foot (chats), or a page's text from the top while it fits (notes).
    fn spinning(&self) -> bool {
        let m = model();
        self.screen == Screen::Chat && m.msgs[..m.n_msgs].iter().any(|msg| msg.pic == 1)
    }

    /// The loading pictures' spinners, one step on (over the bubble the feed drew).
    fn spin_frame(&mut self) {
        let m = model();
        self.spin.1 = self.spin.1.wrapping_add((65536 / 12) as u16);
        let mut s = Dot(self.api);
        let mut bottom = self.feed_bottom();
        for i in (0..m.n_msgs).rev() {
            let h = bubble_h(&m.msgs[i]);
            let top = bottom - h;
            if bottom < FEED_TOP {
                break;
            }
            let msg = &m.msgs[i];
            if msg.pic == 1 && top >= FEED_TOP && bottom <= FEED_END {
                let cx = if msg.me { BUBBLE_X1 - SPIN_W / 2 } else { BUBBLE_X0 + SPIN_W / 2 };
                let cy = top + (LINE + 18) / 2;
                arc(&mut s, cx, cy, 9, 13, 0, 65536, Rgb(0x40, 0x40, 0x40), None, CARD);
                arc(&mut s, cx, cy, 9, 13, self.spin.1, 65536 / 4, WHITE, None, CARD);
            }
            bottom = top - GAP;
        }
        self.api.show();
    }

    fn feed_bottom(&self) -> i32 {
        let m = model();
        if crate::PLAIN {
            let total: i32 = m.msgs[..m.n_msgs].iter().map(|msg| bubble_h(msg) + GAP).sum();
            if total <= FEED_END - FEED_TOP - 16 {
                return FEED_TOP + 14 + total - GAP;
            }
        }
        FEED_END - 6 + self.chat_scroll
    }

    fn feed(&mut self) {
        let mut s = Band(self.api, FEED_TOP, FEED_END);
        let m = model();
        let mut bottom = self.feed_bottom();
        Surface::rect(&mut s, 0, bottom, 466, FEED_END - bottom, 0);
        let mut total = 0;
        for msg in m.msgs[..m.n_msgs].iter().rev() {
            let h = bubble_h(msg);
            let top = bottom - h;
            if top < FEED_END && bottom > FEED_TOP {
                draw_bubble(&mut s, m, msg, top);
            }
            Surface::rect(&mut s, 0, top - GAP, 466, GAP, 0);
            bottom = top - GAP;
            total += h + GAP;
        }
        Surface::rect(&mut s, 0, FEED_TOP, 466, bottom - FEED_TOP, 0);
        self.chat_max = (total - (FEED_END - FEED_TOP)).max(0);
        if m.n_msgs == 0 {
            centre(&mut s, C, C - 8, crate::EMPTY, GREY);
        }
    }

    /// Redraws what changed on the open screen without a clear (scrolls, new messages).
    fn refresh(&mut self) {
        match self.screen {
            Screen::List => {
                self.list_cards();
                let mut s = Dot(self.api);
                Surface::rect(&mut s, 0, 66, 466, 30, 0);
                self.unread_line(&mut s);
                self.scroll_dots(self.list_scroll, self.list_max());
            }
            Screen::Chat => self.feed(),
            _ => return self.draw(),
        }
        self.due = false;
        self.api.show();
        self.shown_at = self.api.now();
    }

    // --- quick replies

    fn draw_quick(&self) {
        let mut s = Dot(self.api);
        let m = model();
        for i in 0..m.n_quick {
            let y = Q_TOP + i as i32 * Q_PITCH - self.quick_scroll;
            if y + 54 < 90 || y > 466 {
                continue;
            }
            let (x0, x1) = chord(y.max(90), (y + 54).min(466), 26);
            if x1 - x0 < 120 {
                continue;
            }
            rrect(&mut s, x0, y, x1 - x0, 54, 27, WHITE, BLACK);
            rrect(&mut s, x0 + 2, y + 2, x1 - x0 - 4, 50, 25, BLACK, WHITE);
            let (t, _) = clip(m.quick[i].as_str(), x1 - x0 - 28);
            centre(&mut s, C, y + 19, t, W);
        }
        Surface::rect(&mut s, 0, 0, 466, 90, 0);
        heading(&mut s, 34, crate::QUICK_TITLE, 4);
    }

    fn quick_max(&self) -> i32 {
        (model().n_quick as i32 * Q_PITCH - (466 - Q_TOP - 40)).max(0)
    }

    // --- what is typed (both keyboards)

    fn draw_typed(&self, y0: i32, rows: usize, px: i32) {
        let mut s = Dot(self.api);
        let typed = fit(&self.typed[..self.n_typed], TYPED_MAX);
        if typed.is_empty() {
            centre(&mut s, C, y0, "Message", DIM);
            Surface::rect(&mut s, C - 44, y0 - 2, 3, 20, RED.rgb565());
            return;
        }
        let mut lines = [""; 8];
        let n = wrap(typed, px, &mut lines);
        // the last `rows` lines, the newest at the bottom
        let first = n.saturating_sub(rows);
        let mut last_w = 0;
        let mut last_y = y0;
        for (i, l) in lines[first..n].iter().enumerate() {
            let (l, _) = clip(l, px);
            let w = font::width(l);
            let y = y0 + i as i32 * LINE;
            Surface::text(&mut s, C - w / 2, y, l, W);
            last_w = w;
            last_y = y;
        }
        let trailing = typed.ends_with(' ') as i32 * font::advance(' ');
        Surface::rect(&mut s, C + last_w / 2 + trailing + 2, last_y - 2, 3, 20, RED.rgb565());
    }

    fn redraw_typed(&self) {
        let mut s = Dot(self.api);
        match self.screen {
            Screen::Qwerty => {
                Surface::rect(&mut s, 0, 64, 466, 108, 0);
                self.draw_typed(76, 3, 320);
            }
            Screen::Emoji => {
                Surface::rect(&mut s, 0, 64, 466, 84, 0);
                self.draw_typed(76, 2, 300);
            }
            _ => {
                Surface::rect(&mut s, 100, 104, 266, 54, 0);
                self.draw_typed(108, 2, 230);
            }
        }
    }

    // --- QWERTY

    fn draw_qwerty(&self) {
        let mut s = Dot(self.api);
        let m = model();
        let to = if crate::ASSISTANT { "Gemini" } else { m.open.and_then(|id| m.chat(id)).map(|c| c.name.as_str()).unwrap_or("") };
        let (to, _) = clip(to, 200);
        let w = font::width("To ") + font::width(to);
        let x = font::draw(&mut s, C - w / 2, 32, "To ", GREY);
        font::draw(&mut s, x, 32, to, SOFT);
        self.draw_typed(76, 3, 320);
        rule(&mut s, 172);
        each_key(self.sym, |k, _, _, _| self.draw_key(k, false));
    }

    fn draw_key(&self, key: K, pressed: bool) {
        let mut s = Dot(self.api);
        let (sym, shift) = (self.sym, self.shift);
        let each = |f: &mut dyn FnMut(K, i32, i32, i32)| {
            if self.screen == Screen::Emoji {
                each_emoji_key(|k, x, y, w| f(k, x, y, w));
            } else {
                each_key(sym, |k, x, y, w| f(k, x, y, w));
            }
        };
        each(&mut |k, x, y, w| {
            if k != key {
                return;
            }
            let fill = match (pressed, k) {
                (true, _) => WHITE,
                (_, K::Send) => RED,
                (_, K::Shift) if shift => WHITE,
                (_, K::Ch(_)) => KEYC,
                _ => CARD,
            };
            Surface::rect(&mut s, x, y, w, KH, 0);
            rrect(&mut s, x + 3, y + 3, w - 6, KH - 6, 14, fill, BLACK);
            let ink = if pressed || (k == K::Shift && shift) { 0 } else { W };
            let mut b = [0u8; 4];
            let label: &str = match k {
                K::Ch(c) => {
                    let c = if shift && !sym { c.to_ascii_uppercase() } else { c };
                    (c as char).encode_utf8(&mut b)
                }
                K::Shift => "^",
                K::Del => "<",
                K::Sym if sym => "abc",
                K::Sym => "?12",
                K::Space => "space",
                K::Send => ">",
                K::Emoji if self.screen == Screen::Emoji => "abc",
                K::Emoji => "",
            };
            if k == K::Emoji && self.screen != Screen::Emoji {
                // a small smiling face
                let cx = x + w / 2;
                let cy = y + KH / 2;
                let face = 0x1F603;
                if !emoji::draw_scaled(face, cx - emoji::SIZE / 2, cy - emoji::SIZE / 2, 1, &mut |x, y, w, h, c| {
                    Surface::rect(&mut s, x, y, w, h, c)
                }) {
                    centre(&mut s, cx, cy - 8, ":)", ink);
                }
                return;
            }
            let ink = if k == K::Space && !pressed { GREY } else { ink };
            centre(&mut s, x + w / 2, y + KH / 2 - 8, label, ink);
        });
    }

    fn qwerty_tap(&mut self, x: i32, y: i32) {
        let mut hit = None;
        each_key(self.sym, |k, kx, ky, w| {
            if (kx..kx + w).contains(&x) && (ky..ky + KH).contains(&y) {
                hit = Some(k);
            }
        });
        let Some(k) = hit else { return };
        self.motor.play(haptic::TICK);
        // a key still shown pressed goes plain first
        if let Some((old, _)) = self.flash.take() {
            self.draw_key(old, false);
        }
        match k {
            K::Send => return self.send_typed(),
            K::Emoji => {
                self.em_page = 0;
                return self.go(Screen::Emoji);
            }
            K::Sym => {
                self.sym = !self.sym;
                return self.draw();
            }
            K::Shift => {
                self.shift = !self.shift;
                return self.draw();
            }
            _ => {}
        }
        let was_shift = self.shift;
        self.type_key(k);
        // a press shows as a white key for a moment; the shifted letters change back after one
        if was_shift != self.shift {
            self.draw();
        } else {
            self.redraw_typed();
        }
        self.draw_key(k, true);
        self.api.show();
        self.flash = Some((k, self.api.now().wrapping_add(110)));
    }

    fn type_key(&mut self, k: K) {
        match k {
            K::Del => {
                let t = fit(&self.typed[..self.n_typed], TYPED_MAX);
                self.n_typed = t.char_indices().last().map_or(0, |(i, _)| i);
                self.shift = self.n_typed == 0;
            }
            K::Space | K::Ch(_) if self.n_typed < TYPED_MAX => {
                let c = match k {
                    K::Ch(c) if self.shift => c.to_ascii_uppercase(),
                    K::Ch(c) => c,
                    _ => b' ',
                };
                self.typed[self.n_typed] = c;
                self.n_typed += 1;
                // capital after the end of a sentence
                self.shift = c == b' ' && self.n_typed >= 2 && matches!(self.typed[self.n_typed - 2], b'.' | b'?' | b'!');
            }
            _ => {}
        }
    }

    // --- emoji

    fn em_pages(&self) -> usize {
        emoji::all().len().div_ceil(EM_PAGE).max(1)
    }

    fn draw_emoji(&self) {
        let mut s = Dot(self.api);
        self.draw_typed(76, 2, 300);
        let all = emoji::all();
        let from = self.em_page * EM_PAGE;
        let x0 = C - EM_CELL * EM_COLS as i32 / 2;
        let big = emoji::SIZE * 2;
        for (i, &cp) in all.iter().enumerate().skip(from).take(EM_PAGE) {
            let (col, row) = ((i - from) % EM_COLS, (i - from) / EM_COLS);
            let x = x0 + col as i32 * EM_CELL + (EM_CELL - big) / 2;
            let y = EM_TOP + row as i32 * EM_CELL + (EM_CELL - big) / 2;
            emoji::draw_scaled(cp, x, y, 2, &mut |x, y, w, h, c| Surface::rect(&mut s, x, y, w, h, c));
        }
        // which page: dots, the crown or a drag turns them
        let n = self.em_pages() as i32;
        for p in 0..n {
            let on = p as usize == self.em_page;
            Surface::rect(&mut s, C - n * 8 + p * 16 + 4, EM_TOP - 12, 6, 6, if on { W } else { GREY });
        }
        each_emoji_key(|k, _, _, _| self.draw_key(k, false));
    }

    fn em_turn(&mut self, d: i32) {
        let p = (self.em_page as i32 + d).clamp(0, self.em_pages() as i32 - 1) as usize;
        if p != self.em_page {
            self.em_page = p;
            self.motor.play(haptic::TICK);
            self.draw();
        }
    }

    fn emoji_tap(&mut self, x: i32, y: i32) {
        if y >= ROW_Y[3] {
            let mut hit = None;
            each_emoji_key(|k, kx, ky, w| {
                if (kx..kx + w).contains(&x) && (ky..ky + KH).contains(&y) {
                    hit = Some(k);
                }
            });
            let Some(k) = hit else { return };
            self.motor.play(haptic::TICK);
            if let Some((old, _)) = self.flash.take() {
                self.draw_key(old, false);
            }
            match k {
                K::Send => return self.send_typed(),
                K::Emoji => return self.go(Screen::Qwerty),
                _ => {}
            }
            self.type_key(k);
            self.redraw_typed();
            self.draw_key(k, true);
            self.api.show();
            self.flash = Some((k, self.api.now().wrapping_add(110)));
            return;
        }
        let x0 = C - EM_CELL * EM_COLS as i32 / 2;
        let (col, row) = ((x - x0).div_euclid(EM_CELL), (y - EM_TOP).div_euclid(EM_CELL));
        if !(0..EM_COLS as i32).contains(&col) || !(0..EM_ROWS as i32).contains(&row) {
            return;
        }
        let i = self.em_page * EM_PAGE + row as usize * EM_COLS + col as usize;
        let Some(&cp) = emoji::all().get(i) else { return };
        let Some(ch) = char::from_u32(cp) else { return };
        let mut b = [0u8; 4];
        let b = ch.encode_utf8(&mut b).as_bytes();
        if self.n_typed + b.len() > TYPED_MAX {
            return;
        }
        self.typed[self.n_typed..self.n_typed + b.len()].copy_from_slice(b);
        self.n_typed += b.len();
        self.shift = false;
        self.motor.play(haptic::TICK);
        self.redraw_typed();
        self.api.show();
    }

    // --- rotary dial

    fn draw_dial(&self) {
        let mut s = Dot(self.api);
        rule(&mut s, 170);
        self.draw_typed(110, 2, 14);
        self.dial.draw(&mut s);
    }

    /// What the keyboard said: typed into the text here (sentence capitals kept), or sent.
    fn dial_act(&mut self, a: Act) {
        match a {
            Act::None => {}
            Act::Show => {
                self.motor.play(haptic::TICK);
                self.api.show();
            }
            Act::Redraw => {
                self.motor.play(haptic::TICK);
                self.draw();
            }
            Act::Key(k) => {
                self.motor.play(haptic::TICK);
                match k {
                    Key::Send => return self.send_typed(),
                    Key::Del => self.type_key(K::Del),
                    Key::Space => {
                        self.shift = false;
                        self.type_key(K::Space);
                    }
                    Key::Ch(c) => {
                        // already in its case
                        self.shift = false;
                        self.type_key(K::Ch(c));
                    }
                }
                self.dial.shift = self.shift;
                self.redraw_typed();
                self.dial.draw_centre(&mut Dot(self.api));
                self.api.show();
            }
        }
    }

    // --- sending

    fn send_bytes(&mut self, text: &[u8]) {
        let Some(id) = model().open else { return };
        let n = text.len().min(MSG_MAX - 2);
        if n == 0 {
            return;
        }
        let mut out = [0u8; MSG_MAX];
        out[0] = b's';
        out[1] = id;
        out[2..2 + n].copy_from_slice(&text[..n]);
        let _ = self.api.send(&out[..2 + n]);
        if crate::ASSISTANT {
            self.asked = out;
            self.asked_n = 2 + n;
            self.pending = Some((self.api.now(), 0));
        }
        if !crate::PLAIN {
            model().append_local(&text[..n]);
        }
        self.motor.play(haptic::DONE);
        self.go(Screen::Chat);
    }

    fn send_typed(&mut self) {
        let (t, n) = (self.typed, self.n_typed);
        self.n_typed = 0;
        self.send_bytes(&t[..n]);
    }

    // --- input

    /// Screens that scroll: a drag scrolls them, a tap acts on TOUCH_UP.
    fn scrolls(&self) -> bool {
        matches!(self.screen, Screen::List | Screen::Chat | Screen::Quick | Screen::Emoji)
    }

    /// A drag of `dy` px (down > 0): the content follows the finger.
    fn drag(&mut self, dy: i32) {
        match self.screen {
            Screen::List => {
                let next = (self.list_scroll - dy).clamp(0, self.list_max());
                if next == self.list_scroll {
                    return;
                }
                self.list_scroll = next;
            }
            Screen::Chat => {
                // chat_scroll counts up towards older messages, which sit above
                let next = (self.chat_scroll + dy).clamp(0, self.chat_max);
                if next == self.chat_scroll {
                    return;
                }
                self.chat_scroll = next;
            }
            Screen::Quick => {
                let next = (self.quick_scroll - dy).clamp(0, self.quick_max());
                if next == self.quick_scroll {
                    return;
                }
                self.quick_scroll = next;
            }
            // up: the next page
            Screen::Emoji => return self.em_turn(if dy < 0 { 1 } else { -1 }),
            _ => return,
        }
        self.refresh();
    }

    fn crown(&mut self, d: i16) {
        // merged detents (main loop) scroll as far as they add up, a few at most
        let d = if self.screen == Screen::Dial { d.signum() as i32 } else { (d as i32).clamp(-4, 4) };
        match self.screen {
            Screen::List => {
                let next = (self.list_scroll + 47 * d).clamp(0, self.list_max());
                if next == self.list_scroll {
                    return;
                }
                self.list_scroll = next;
            }
            Screen::Chat => {
                // up = older
                let next = (self.chat_scroll + 60 * d).clamp(0, self.chat_max);
                if next == self.chat_scroll {
                    return;
                }
                self.chat_scroll = next;
            }
            Screen::Quick => {
                let next = (self.quick_scroll + Q_PITCH * d).clamp(0, self.quick_max());
                if next == self.quick_scroll {
                    return;
                }
                self.quick_scroll = next;
            }
            Screen::Dial => {
                if self.dial.crown(&mut Dot(self.api), d, self.api.now()) == Act::Show {
                    self.motor.play(haptic::TICK);
                    self.api.show();
                }
                return;
            }
            Screen::Qwerty => return,
            Screen::Emoji => return self.em_turn(d.signum()),
        }
        self.refresh();
    }

    fn tap(&mut self, x: i32, y: i32) {
        match self.screen {
            Screen::List => {
                if y < HEAD_H {
                    return;
                }
                let i = (y - LIST_TOP + self.list_scroll).div_euclid(PITCH);
                let m = model();
                if i < 0 || i as usize >= m.n_chats || (y - LIST_TOP + self.list_scroll) % PITCH > CARD_H {
                    return;
                }
                let i = i as usize;
                if m.chats[i].unread == STATUS_ROW {
                    return;
                }
                let id = m.chats[i].id;
                m.chats[i].unread = 0;
                m.open = Some(id);
                m.n_msgs = 0;
                let _ = self.api.send(&[b'o', id]);
                if crate::PLAIN && self.syncing {
                    // the phone has not answered: the kept copy until it does
                    let k = kept();
                    if let Some(slot) = k.note(id, m.chats[i].name.as_str()) {
                        k.notes[slot].each(|msg| {
                            model().apply(msg);
                        });
                    }
                }
                self.motor.play(haptic::TICK);
                self.go(Screen::Chat);
            }
            Screen::Chat => {
                if y < FEED_TOP {
                    self.go(Screen::List);
                    return;
                }
                if y < FEED_END {
                    // a tap on a message: the phone decides (Notes: ticks a reminder; Mail, chats: nothing)
                    let m = model();
                    let mut bottom = self.feed_bottom();
                    for i in (0..m.n_msgs).rev() {
                        let top = bottom - bubble_h(&m.msgs[i]);
                        if (top..bottom).contains(&y) {
                            let _ = self.api.send(&[b't', i as u8]);
                            self.motor.play(haptic::TICK);
                            if matches!(m.msgs[i].kind, 1 | 2) && m.msgs[i].img.is_none() {
                                m.msgs[i].pic = 1;
                                self.refresh();
                            }
                            break;
                        }
                        bottom = top - GAP;
                    }
                    return;
                }
                let to = [Screen::Qwerty, Screen::Quick, Screen::Dial];
                if let Some(b) = BTN_X.iter().position(|&bx| (x - bx).abs() < 34) {
                    self.motor.play(haptic::TICK);
                    self.go(to[b]);
                }
            }
            Screen::Quick => {
                let i = (y - Q_TOP + self.quick_scroll).div_euclid(Q_PITCH);
                let m = model();
                if y < 90 || i < 0 || i as usize >= m.n_quick {
                    return;
                }
                let q = m.quick[i as usize];
                self.send_bytes(q.as_str().as_bytes());
            }
            Screen::Qwerty => self.qwerty_tap(x, y),
            Screen::Emoji => self.emoji_tap(x, y),
            Screen::Dial => {
                let a = self.dial.down(&mut Dot(self.api), x, y, self.api.now());
                self.dial_act(a);
            }
        }
    }
}

struct Lines<'a> {
    l: [&'a str; 8],
    n: usize,
}

/// Notes: text from x 72, 322 px wide (the round screen's width at the feed's top and bottom); a checklist line
/// starts at 106.
const PLAIN_X: i32 = 72;
const PLAIN_W: i32 = 322;
const CHECK_X: i32 = 106;

fn text_w(msg: &Msg) -> i32 {
    if !crate::PLAIN {
        TEXT_W
    } else if matches!(msg.kind, 3 | 4) {
        PLAIN_X + PLAIN_W - CHECK_X
    } else {
        PLAIN_W
    }
}

fn lines(msg: &Msg) -> Lines<'_> {
    let mut l = [""; 8];
    let n = wrap(msg.text.as_str(), text_w(msg), &mut l);
    Lines { l, n }
}

fn bubble_h(msg: &Msg) -> i32 {
    match msg.img.filter(|i| i.ok) {
        Some(img) => img.size().1 + if msg.text.as_str().is_empty() { 0 } else { LINE + 8 },
        None if crate::PLAIN => lines(msg).n.max(1) as i32 * LINE + if matches!(msg.kind, 3 | 4) { 12 } else { 2 },
        None => lines(msg).n.max(1) as i32 * LINE + 20,
    }
}

fn draw_bubble(s: &mut Band, m: &Model, msg: &Msg, top: i32) {
    if let Some(img) = msg.img.filter(|i| i.ok) {
        let (w, h) = img.size();
        let x = if msg.me { BUBBLE_X1 - w } else { BUBBLE_X0 };
        surround(s, x, top, w, h, 0);
        let rest = bubble_h(msg) - h;
        if rest > 0 {
            Surface::rect(s, 0, top + h, 466, rest, 0);
        }
        let mut d = Decoder::new();
        if d.start(&img.header, x, top, img.header[5]) {
            d.feed(m.tokens(&img), &mut Rle(s.0));
        }
        if !msg.text.as_str().is_empty() {
            fitted(s, x, top + h + 6, msg.text.as_str(), w.max(120), GREY);
        }
        return;
    }
    if crate::PLAIN {
        // a page: plain lines, a checklist line with its box; the band is cleared under it
        let h = bubble_h(msg);
        Surface::rect(s, 0, top, 466, h, 0);
        let ls = lines(msg);
        let check = matches!(msg.kind, 3 | 4);
        let x = if check { CHECK_X } else { PLAIN_X };
        if check {
            let cy = top + LINE / 2 + 2;
            ring(s, PLAIN_X + 12, cy, 9, 11, WHITE, BLACK);
            if msg.kind == 4 {
                disc(s, PLAIN_X + 12, cy, 6, RED, BLACK);
            }
        }
        let c = if msg.kind == 4 { GREY } else { W };
        for (i, l) in ls.l[..ls.n].iter().enumerate() {
            fitted(s, x, top + i as i32 * LINE, l, text_w(msg), c);
        }
        return;
    }
    // a tap asks the phone for the picture
    let placeholder = match (msg.kind, msg.pic) {
        (1 | 2, 1) => "",
        (1 | 2, 2) => "No picture",
        (1, _) => "Sticker: tap",
        (2, _) => "Photo: tap",
        _ => "",
    };
    let ls = if placeholder.is_empty() { lines(msg) } else { Lines { l: [placeholder, "", "", "", "", "", "", ""], n: 1 } };
    let n = ls.n.max(1);
    let widest = ls.l[..n].iter().map(|l| font::width(l)).max().unwrap_or(0).min(TEXT_W);
    let (w, h) = (if msg.pic == 1 && matches!(msg.kind, 1 | 2) { SPIN_W } else { widest.max(24) + 32 }, n as i32 * LINE + 18);
    let x = if msg.me { BUBBLE_X1 - w } else { BUBBLE_X0 };
    let (fill, ink) = if msg.me { (WHITE, 0) } else { (CARD, W) };
    surround(s, x, top, w, h, 20);
    let rest = bubble_h(msg) - h;
    if rest > 0 {
        Surface::rect(s, 0, top + h, 466, rest, 0);
    }
    rrect(s, x, top, w, h, 20, fill, BLACK);
    for (i, l) in ls.l[..n].iter().enumerate() {
        let c = if placeholder.is_empty() { ink } else { GREY };
        fitted(s, x + 16, top + 8 + i as i32 * LINE, l, TEXT_W, c);
    }
    if msg.me && !crate::ASSISTANT {
        // at the top, left of our bubble: . delivered, .. read
        for k in 0..msg.state.min(2) as i32 {
            disc(s, x - 10 - k * 12, top + 10, 4, WHITE, BLACK);
        }
    }
}

/// pixview's decoder drawing through the Api, kept inside the feed.
struct Rle(&'static Api);
impl rle::Surface for Rle {
    fn rect(&mut self, x: i32, y: i32, w: i32, h: i32, c: u16) {
        if y + h > FEED_TOP && y < FEED_END {
            Surface::rect(&mut self.0, x, y, w, h, c);
        }
    }
}

const _: () = assert!(MAX_MSGS <= 255);
