//! Chats in the Nothing look: black, white, one red accent, dot-matrix headings.
//!
//! Screens: list (cards) → chat (bubbles, stickers, photos; bottom: keyboard · quick replies · dial) → quick replies,
//! QWERTY keyboard (Wear OS style, laid out for the round panel) or the rotary dial (letters round the rim: the crown
//! turns the selection, a tap on the middle types it, a tap on the rim types that letter). Taps are TOUCH_DOWN (its
//! position is the reliable one). The button goes back (own_back), and leaves the app from the list.

use crate::model::{Effect, Model, Msg, MAX_MSGS};
use crate::rle::{self, Decoder};
use cmf_wp2_app::{Api, BUTTON, CROWN, MESSAGE, MSG_MAX, PAUSE, RESUME, TOUCH_DOWN, TOUCH_UP};
use cmf_wp2_ui::color::{rgb565, BLACK, Rgb};
use cmf_wp2_ui::draw::{disc, ring, rrect};
use cmf_wp2_ui::geom::{chord, polar};
use cmf_wp2_ui::haptic::{self, Motor};
use cmf_wp2_ui::motion::{angle, isqrt};
use cmf_wp2_ui::font::{self, centre, clip, fitted, wrap};
use cmf_wp2_ui::text::{dots, dots_centre, fit};
use cmf_wp2_ui::{C, Surface};

cmf_wp2_app::entry!(main);
cmf_wp2_app::app_icon!();

static mut MODEL: Model = Model::new();

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
/// Widest text line in a bubble, px.
const TEXT_W: i32 = 262;
const GAP: i32 = 10;
const BTN_Y: i32 = 424;
/// Dial letters per crown detent, in 1/100s: a slow turn, a fast spin (ui.crown).
/// Moved more than this between down and up: a drag, not a tap.
const DRAG: i32 = 14;
const DIAL_SLOW: i32 = 17;
const DIAL_FAST: i32 = 60;
/// The space bar under the dial's centre letter: top and height.
const SPACE_Y: i32 = 312;
const SPACE_H: i32 = 40;
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
const LETTERS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ.?_<>#";
const NUMBERS: &[u8] = b"0123456789.,?!@'-:()+_<>#";
/// Held this long, a dial letter types as a capital.
const HOLD_MS: u32 = 450;
const DIAL_R: i32 = 204;

#[derive(Clone, Copy, PartialEq)]
enum Screen {
    List,
    Chat,
    Quick,
    Qwerty,
    Dial,
}

#[derive(Clone, Copy, PartialEq)]
enum K {
    Ch(u8),
    Shift,
    Del,
    Sym,
    Space,
    Send,
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
            row.keys[0] = (K::Sym, 62);
            row.keys[1] = (K::Space, 122);
            row.keys[2] = (K::Send, 72);
            row.n = 3;
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
    sel: usize,
    /// a key drawn pressed, to draw plain again at this time
    flash: Option<(K, u32)>,
    /// a dial letter under the finger: index, when it went down, whether the hold made it a capital
    press: Option<(usize, u32, bool)>,
    motor: Motor,
    /// crown detents since the dial last moved a letter
    crown_acc: i32,
    /// when the last crown detent came (its speed)
    crown_at: u32,
    /// the detent gap averaged, ms
    crown_gap: i32,
    /// where a touch on a scrolling screen went down
    down: Option<(i32, i32)>,
    /// Gemini: a question sent, no answer yet: when, and whether it was sent again
    pending: Option<(u32, bool)>,
    /// Gemini: the phone has the question, the answer has not started
    thinking: bool,
    asked: [u8; MSG_MAX],
    asked_n: usize,
    /// the dial letter's dot pitch as last drawn
    dial_pitch: core::cell::Cell<i32>,
}

fn main(api: &'static Api) -> i32 {
    api.own_back(true);
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
        shift: true,
        sel: 0,
        flash: None,
        press: None,
        motor: Motor::new(api),
        crown_acc: 0,
        crown_at: 0,
        crown_gap: 400,
        down: None,
        pending: None,
        thinking: false,
        asked: [0; MSG_MAX],
        asked_n: 0,
        dial_pitch: core::cell::Cell::new(0),
    };
    ui.draw();
    let _ = api.send(b"R");
    // the phone may still be starting (iOS relaunching CMFX loses the first ask): ask again every 3 s until it
    // answers, at most 20 times
    const RETRY_MS: u32 = 3000;
    /// Gemini: no word of an answer after this long, ask again (once), then say the phone did not answer
    const ASK_AGAIN_MS: u32 = 9000;
    let mut retry: Option<(u32, u8)> = Some((api.now().wrapping_add(RETRY_MS), 20));
    let mut msg = [0u8; MSG_MAX];
    // an event read ahead while crown detents were merged
    let mut ahead: Option<cmf_wp2_app::Event> = None;
    loop {
        let now = api.now();
        let flash = ui.flash.map(|(_, t)| t.wrapping_sub(now).min(1000));
        let hold = match ui.press {
            Some((_, t, false)) => Some(t.wrapping_add(HOLD_MS).wrapping_sub(now).min(HOLD_MS)),
            _ => None,
        };
        let again = retry.map(|(t, _)| t.wrapping_sub(now).min(RETRY_MS));
        let asked = ui.pending.map(|(t, _)| t.wrapping_add(ASK_AGAIN_MS).wrapping_sub(now).min(ASK_AGAIN_MS));
        let wait = [ui.motor.wait(), flash, hold, again, asked].into_iter().flatten().min().unwrap_or(u32::MAX);
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
                if !again {
                    // the phone may have been asleep and lost it (iOS relaunching CMFX): once more
                    let _ = api.send(&ui.asked[..ui.asked_n]);
                    ui.pending = Some((api.now(), true));
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
                if ui.screen == Screen::Qwerty {
                    ui.draw_key(k, false);
                    api.show();
                }
            }
        }
        if let Some((i, t, false)) = ui.press {
            if api.now().wrapping_sub(t) >= HOLD_MS {
                // held: the letter becomes a capital, shown in the middle
                ui.press = Some((i, t, true));
                ui.motor.play(haptic::TICK);
                ui.draw_dial_centre();
                api.show();
            }
        }
        let Some(e) = e else { continue };
        match e.kind {
            TOUCH_UP if ui.screen == Screen::Dial => {
                if let Some((i, _, upper)) = ui.press.take() {
                    ui.dial_type(i, upper);
                }
            }
            BUTTON => {
                ui.motor.off();
                ui.press = None;
                match ui.screen {
                    Screen::List => return 0,
                    Screen::Chat => ui.go(Screen::List),
                    Screen::Quick | Screen::Qwerty | Screen::Dial => ui.go(Screen::Chat),
                }
            }
            PAUSE => {
                ui.motor.off();
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
                let (mut list, mut chat) = (false, false);
                while let Some(m) = api.recv(&mut msg) {
                    if m.first() == Some(&b'T') || (m.first() == Some(&b'A') && m.get(1) == Some(&0)) {
                        answer = true;
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
                } else if (list && ui.screen == Screen::List) || (chat && ui.screen == Screen::Chat) {
                    ui.refresh();
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
    fn go(&mut self, to: Screen) {
        self.screen = to;
        match to {
            Screen::Chat => self.chat_scroll = 0,
            Screen::Quick => self.quick_scroll = 0,
            Screen::Qwerty | Screen::Dial => {
                self.sym = false;
                self.shift = self.n_typed == 0;
                self.flash = None;
            }
            Screen::List => {}
        }
        self.draw();
    }

    fn draw(&mut self) {
        let mut s = Dot(self.api);
        Surface::rect(&mut s, 0, 0, 466, 466, 0);
        match self.screen {
            Screen::List => self.draw_list(),
            Screen::Chat => self.draw_chat(),
            Screen::Quick => self.draw_quick(),
            Screen::Qwerty => self.draw_qwerty(),
            Screen::Dial => self.draw_dial(),
        }
        self.api.show();
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
        let unread = m.chats[..m.n_chats].iter().filter(|c| c.unread > 0).count();
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
            surround(&mut s, x0, y, x1 - x0, CARD_H, CARD_H / 2);
            rrect(&mut s, x0, y, x1 - x0, CARD_H, CARD_H / 2, CARD, BLACK);
            let (ax, ay) = (x0 + 46, y + CARD_H / 2);
            ring(&mut s, ax, ay, 25, 27, if c.unread > 0 { WHITE } else { Rgb(0x70, 0x70, 0x70) }, CARD);
            let mut b = [0u8; 4];
            let ch = c.name.as_str().chars().next().unwrap_or('?').to_ascii_uppercase();
            dots_centre(&mut s, ax, ay - 12, ch.encode_utf8(&mut b), 3, 2, W);
            let tx = x0 + 86;
            let right = x1 - if c.unread > 0 { 52 } else { 24 };
            fitted(&mut s, tx, y + 16, c.name.as_str(), right - tx, if c.unread > 0 { W } else { SOFT });
            fitted(&mut s, tx, y + 46, c.preview.as_str(), x1 - 30 - tx, GREY);
            if c.unread > 0 {
                disc(&mut s, x1 - 32, y + 26, 13, RED, CARD);
                let mut n = [0u8; 11];
                centre(&mut s, x1 - 32, y + 18, cmf_wp2_ui::text::int(c.unread.min(9) as i32, &mut n), W);
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
        self.api.show();
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
        each_key(sym, |k, x, y, w| {
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
            };
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

    // --- rotary dial

    fn dial(&self) -> &'static [u8] {
        if self.sym { NUMBERS } else { LETTERS }
    }

    fn dial_at(&self, i: usize) -> (i32, i32) {
        // from 12 o'clock, clockwise
        let a = (49152u32 + (i as u32 * 65536) / self.dial().len() as u32) as u16;
        polar(DIAL_R, a)
    }

    fn dial_label(&self, i: usize) -> &'static str {
        let d = self.dial();
        match d[i] {
            b'#' if self.sym => "ab",
            b'#' => "12",
            _ => core::str::from_utf8(&d[i..i + 1]).unwrap_or("?"),
        }
    }

    fn draw_dial_cell(&self, i: usize) {
        let mut s = Dot(self.api);
        let (x, y) = self.dial_at(i);
        if i != self.sel {
            // the selection's disc goes; the letter's cell is painted, not cleared then drawn
            disc(&mut s, x, y, 19, BLACK, BLACK);
        }
        if i == self.sel {
            disc(&mut s, x, y, 19, if self.dial()[i] == b'>' { RED } else { WHITE }, BLACK);
            centre(&mut s, x, y - 8, self.dial_label(i), if self.dial()[i] == b'>' { W } else { 0 });
        } else {
            let c = match self.dial()[i] {
                b'>' => RED.rgb565(),
                b'#' => W,
                b'0'..=b'9' | b'A'..=b'Z' => SOFT,
                _ => GREY,
            };
            centre(&mut s, x, y - 8, self.dial_label(i), c);
        }
    }

    fn draw_dial_centre(&self) {
        let mut s = Dot(self.api);
        let upper = self.shift || matches!(self.press, Some((_, _, true)));
        let mut b = [0u8; 4];
        let label = match self.dial()[self.sel] {
            b'_' => "SPC",
            b'<' => "DEL",
            b'>' => "SEND",
            b'#' if self.sym => "ABC",
            b'#' => "123",
            c if c.is_ascii_alphabetic() && !upper => (c.to_ascii_lowercase() as char).encode_utf8(&mut b),
            _ => self.dial_label(self.sel),
        };
        let pitch = if label.len() == 1 { 8 } else { 3 };
        let h = 8 * pitch;
        let w = label.chars().count() as i32 * 8 * pitch;
        let (x, y) = (C - w / 2, C - h / 2 - 6);
        // painted over the last letter dot for dot (off dots black): no clear, no flicker; a change of size clears
        if self.dial_pitch.get() != pitch {
            Surface::rect(&mut s, C - 60, 176, 120, 112, 0);
            self.dial_pitch.set(pitch);
        } else {
            Surface::rect(&mut s, C - 60, 176, x - (C - 60), 112, 0);
            Surface::rect(&mut s, x + w, 176, C + 60 - x - w, 112, 0);
        }
        let c = if self.dial()[self.sel] == b'>' { RED.rgb565() } else { W };
        cmf_wp2_ui::text::dots_opaque(&mut s, x, y, label, pitch, pitch - 1, c, 0);
        centre(&mut s, C, 280, "tap  hold: Aa", DIM);
    }

    fn draw_dial(&self) {
        let mut s = Dot(self.api);
        rule(&mut s, 170);
        self.draw_typed(110, 2, 14);
        rrect(&mut s, C - 64, SPACE_Y, 128, SPACE_H, SPACE_H / 2, Rgb(0x2a, 0x2a, 0x2a), BLACK);
        centre(&mut s, C, SPACE_Y + 9, "space", W);
        for i in 0..self.dial().len() {
            self.draw_dial_cell(i);
        }
        self.draw_dial_centre();
    }

    fn dial_type(&mut self, i: usize, upper: bool) {
        self.motor.play(haptic::TICK);
        match self.dial()[i] {
            b'>' => return self.send_typed(),
            b'#' => {
                // the other dial
                self.sym = !self.sym;
                self.sel = 0;
                return self.draw();
            }
            b'<' => self.type_key(K::Del),
            b'_' => self.type_key(K::Space),
            c => {
                let c = if self.shift || upper { c } else { c.to_ascii_lowercase() };
                self.shift = false;
                self.type_key(K::Ch(c));
            }
        }
        self.redraw_typed();
        self.draw_dial_centre();
        self.api.show();
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
            self.pending = Some((self.api.now(), false));
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
        matches!(self.screen, Screen::List | Screen::Chat | Screen::Quick)
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
                // dynamic speed: each detent adds a fraction of a letter (in 1/100s) that grows smoothly with the
                // turn's speed, from 1/4 letter when slow (gap >= 220 ms) to 1.5 letters when spun (gap <= 30 ms); the
                // gap is averaged over the last detents so the speed ramps instead of jumping. A turn back starts over.
                let now = self.api.now();
                let gap = now.wrapping_sub(self.crown_at).min(400) as i32;
                self.crown_at = now;
                self.crown_gap = (self.crown_gap + gap) / 2;
                let g = self.crown_gap.clamp(30, 220);
                let gain = DIAL_SLOW + (DIAL_FAST - DIAL_SLOW) * (220 - g) / 190;
                if (self.crown_acc > 0) != (d > 0) {
                    self.crown_acc = 0;
                }
                self.crown_acc += d * gain;
                if self.crown_acc.abs() < 100 {
                    return;
                }
                let step = self.crown_acc / 100;
                self.crown_acc %= 100;
                let old = self.sel;
                self.sel = (self.sel as i32 + step).rem_euclid(self.dial().len() as i32) as usize;
                self.motor.play(haptic::TICK);
                self.draw_dial_cell(old);
                self.draw_dial_cell(self.sel);
                self.draw_dial_centre();
                self.api.show();
                return;
            }
            Screen::Qwerty => return,
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
                let id = m.chats[i].id;
                m.chats[i].unread = 0;
                m.open = Some(id);
                m.n_msgs = 0;
                let _ = self.api.send(&[b'o', id]);
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
            Screen::Dial => {
                let (dx, dy) = (x - C, y - C);
                let r = isqrt(dx * dx + dy * dy);
                if r > DIAL_R - 34 {
                    // the letter under the finger: nearest by angle
                    let a = angle(dx, dy).wrapping_sub(49152) as u32;
                    let n = self.dial().len() as u32;
                    let i = ((a * n + 32768) / 65536 % n) as usize;
                    let old = self.sel;
                    self.sel = i;
                    self.draw_dial_cell(old);
                    self.draw_dial_cell(i);
                    self.press = Some((i, self.api.now(), false));
                    self.draw_dial_centre();
                    self.api.show();
                } else if (SPACE_Y - 6..SPACE_Y + SPACE_H + 6).contains(&y) && (x - C).abs() < 80 {
                    // the space bar: types a space, the selection stays
                    if let Some(i) = self.dial().iter().position(|&c| c == b'_') {
                        self.press = Some((i, self.api.now(), false));
                    }
                } else if (176..SPACE_Y - 6).contains(&y) {
                    self.press = Some((self.sel, self.api.now(), false));
                }
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
    let placeholder = match msg.kind {
        1 => "Sticker",
        2 => "Photo",
        _ => "",
    };
    let ls = if placeholder.is_empty() { lines(msg) } else { Lines { l: [placeholder, "", "", "", "", "", "", ""], n: 1 } };
    let n = ls.n.max(1);
    let widest = ls.l[..n].iter().map(|l| font::width(l)).max().unwrap_or(0).min(TEXT_W);
    let (w, h) = (widest.max(24) + 32, n as i32 * LINE + 18);
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
