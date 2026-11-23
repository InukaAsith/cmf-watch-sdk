//! Browser: the phone loads the web, the watch reads it. Reader view (default): the page's text, the crown or a
//! drag scrolls, a link tapped opens; the round button at the bottom switches to page view: a picture of the page
//! as the phone's browser shows it (its cookies, its logins), the whole page sent once (long.rs) and scrolled here by
//! the crown or a drag, taps go to it, a text field tapped brings up the keyboard. A video playing there: its sound on the phone, its picture here, small and in few
//! colours, as fast as Bluetooth takes it. The back swipe goes back a page (none: the start screen; on the start
//! screen it closes); the crown press closes the app from anywhere (a loader before 3 can't tell them apart: both
//! close). Nothing is kept on the watch.
//!
//! Phone → watch (and page.rs): `!text` what it is doing or why it couldn't; `H idx title` a recent page for the
//! start screen; `Z` no page before this one; `K` a text field was tapped in page view (the keyboard up); `M 0|1` a
//! video is playing (its frames: `I`/`D`…/`E`, examples/pixview rle.rs, drawn as they come); page view's page:
//! `J h`, its bands, `j` (long.rs: shown once all of it came, a spinner till then, so it scrolls without waits);
//! `G` go to page view (a page with no text, a video page); `Y` a tap changed nothing; `A n`, `a first (x y w h:i16 kind)…` the page's
//! tappable places, page px (links 0, fields 1, buttons 2): a tap goes to the nearest one within reach, outlined.
//! Watch → phone: `R` ready; `Q text` a search or an address; `O idx` a recent page; `L idx` a link; `B` back;
//! `V 0|1` reader or page view; `T x:u16 y:u16` a tap on the page view, page px (from the page's top); `W text`
//! typed into the tapped field (then Enter); `X` the start screen (the phone stops the page's sound and pictures).

use crate::keys::{Keys, Tap};
use crate::long::{self, Long};
use crate::page::{self, Page};
use crate::rle::{self, Decoder, Status};
use cmf_wp2_app::{Api, Event, BUTTON, CROWN, MESSAGE, MSG_MAX, PAUSE, RESUME, TOUCH_DOWN, TOUCH_UP};
use cmf_wp2_ui::color::{rgb565, Rgb, BLACK};
use cmf_wp2_ui::dial::Act;
use cmf_wp2_ui::draw::{disc, rrect};
use cmf_wp2_ui::font;
use cmf_wp2_ui::haptic::{self, Motor};
use cmf_wp2_ui::text::dots_centre;
use cmf_wp2_ui::{C, Surface};

cmf_wp2_app::entry!(main);
cmf_wp2_app::app_icon!();

/// The reader page and page view's picture share their memory (never both on screen; the phone sends the one that
/// is wanted again on a switch). Whichever is asked for and isn't held is cleared first. Both hold only integers, so
/// any bytes are a valid one of them.
#[repr(C)]
union Shared {
    page: core::mem::ManuallyDrop<Page>,
    long: core::mem::ManuallyDrop<Long>,
}
static mut SHARED: Shared = Shared { long: core::mem::ManuallyDrop::new(Long::new()) };
/// what SHARED holds: 0 page view's picture, 1 the reader page
static mut HOLDS: u8 = 0;

fn long() -> &'static mut Long {
    unsafe {
        let l = &mut **core::ptr::addr_of_mut!(SHARED.long);
        if HOLDS != 0 {
            HOLDS = 0;
            l.clear();
        }
        l
    }
}

fn page() -> &'static mut Page {
    unsafe {
        let p = &mut **core::ptr::addr_of_mut!(SHARED.page);
        if HOLDS != 1 {
            HOLDS = 1;
            p.clear();
        }
        p
    }
}

const W: u16 = 0xFFFF;
const GREY: u16 = rgb565(0x80, 0x80, 0x80);
const SOFT: u16 = rgb565(0xD0, 0xD0, 0xD0);
const LINKC: u16 = rgb565(0x8A, 0xB4, 0xF8);
const RED: Rgb = Rgb(0xD7, 0x19, 0x21);
const CARD: Rgb = Rgb(0x1C, 0x1C, 0x1E);
const WHITE: Rgb = Rgb(0xFF, 0xFF, 0xFF);

/// Reader: the text between these (title above, the button below).
const TOP: i32 = 64;
const BOT: i32 = 398;
/// The round button at the bottom: reader ⇄ page view.
/// The bottom bar in reader and page view: the search box (left), the Page / Text button (right): centre x, top, w, h.
const SEARCH: (i32, i32, i32, i32) = (C - 46, 396, 148, 44);
const SWITCH: (i32, i32, i32, i32) = (C + 78, 396, 84, 44);
/// The spinner's dots round its middle (radius 34), clockwise from the top.
const SPIN: [(i32, i32); 12] =
    [(0, -34), (17, -29), (29, -17), (34, 0), (29, 17), (17, 29), (0, 34), (-17, 29), (-29, 17), (-34, 0), (-29, -17), (-17, -29)];
/// Start: the pill, the recent rows.
const WHERE: (i32, i32, i32) = (96, 56, 270);
const ROWS_Y: i32 = 190;
const ROW_H: i32 = 58;
const RECENT: usize = 4;
/// Moved more than this from down to up: a drag.
const DRAG: i32 = 24;
const FRAME_MS: u32 = 80;
const BOXES: usize = 160;
/// A tap this near a tappable place (px) goes to it.
const REACH: i32 = 26;

#[derive(Clone, Copy, PartialEq)]
enum Screen {
    Start,
    Keys,
    Reader,
    Page,
}

struct Screen565(&'static Api);
impl rle::Surface for Screen565 {
    fn rect(&mut self, x: i32, y: i32, w: i32, h: i32, c: u16) {
        let mut a = self.0;
        Surface::rect(&mut a, x, y, w, h, c);
    }
}

struct Ui {
    api: &'static Api,
    motor: Motor,
    screen: Screen,
    keys: Keys,
    /// the keyboard types into page view's tapped field (else a search or an address)
    field: bool,
    scroll: i32,
    down: Option<(i32, i32)>,
    note: [u8; 80],
    n_note: usize,
    recent: [([u8; 48], usize); RECENT],
    n_recent: usize,
    pic: Decoder,
    /// page view has a picture on the panel
    pictured: bool,
    video: bool,
    /// page view's scroll, panel px
    pscroll: i32,
    /// where the keyboard was opened from (back returns there)
    keys_from: Screen,
    /// page view's tappable places: x, y (page px), w, h, kind
    boxes: [(i16, i16, i16, i16, u8); BOXES],
    n_boxes: usize,
    shown_at: u32,
    due: bool,
}

fn main(api: &'static Api) -> i32 {
    api.own_back(true);
    api.keep_screen_on(true);
    let mut ui = Ui {
        api,
        motor: Motor::new(api),
        screen: Screen::Start,
        keys: Keys::new(),
        field: false,
        scroll: 0,
        down: None,
        note: [0; 80],
        n_note: 0,
        recent: [([0; 48], 0); RECENT],
        n_recent: 0,
        pic: Decoder::new(),
        pictured: false,
        video: false,
        pscroll: 0,
        keys_from: Screen::Start,
        boxes: [(0, 0, 0, 0, 0); BOXES],
        n_boxes: 0,
        shown_at: 0,
        due: false,
    };
    ui.draw();
    let _ = api.send(b"R");
    let mut msg = [0u8; MSG_MAX];
    loop {
        let typing = ui.screen == Screen::Keys && ui.keys.rotary;
        let hold = typing.then(|| ui.keys.dial.hold_wait(api.now())).flatten();
        // a spinner turns while something loads
        if ui.loading() {
            ui.due = true;
        }
        let pace = ui.due.then(|| FRAME_MS.saturating_sub(api.now().wrapping_sub(ui.shown_at)));
        let w = [ui.motor.wait(), hold, pace].into_iter().flatten().min().unwrap_or(u32::MAX);
        let e = api.event(w);
        ui.motor.update();
        if typing && ui.keys.dial.tick(&mut { api }, api.now()) == Act::Show {
            ui.motor.play(haptic::TICK);
            api.show();
        }
        if ui.due && api.now().wrapping_sub(ui.shown_at) >= FRAME_MS {
            ui.draw();
        }
        let Some(e) = e else { continue };
        match e.kind {
            BUTTON => {
                // the swipe (loader 3: x 1) goes back; the crown press closes
                if e.x != 1 || !ui.back() {
                    let _ = api.send(b"X");
                    ui.motor.off();
                    api.keep_screen_on(false);
                    return 0;
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
                ui.draw();
            }
            TOUCH_DOWN => ui.touch_down(e),
            TOUCH_UP => ui.touch_up(e),
            CROWN => ui.crown(i32::from(e.delta)),
            MESSAGE => {
                let mut redraw = false;
                let mut drew = false;
                while let Some(m) = api.recv(&mut msg) {
                    match ui.message(m) {
                        Got::Redraw => redraw = true,
                        Got::Drew => drew = true,
                        Got::None => {}
                    }
                }
                if redraw {
                    ui.later();
                } else if drew {
                    if ui.screen == Screen::Page {
                        ui.draw_switch();
                    }
                    ui.shown();
                }
            }
            _ => {}
        }
    }
}

enum Got {
    None,
    /// draw the screen again (paced)
    Redraw,
    /// drawn already (page view's picture): show
    Drew,
}

impl Ui {
    fn send(&self, m: &[u8]) {
        let _ = self.api.send(m);
    }

    fn set_note(&mut self, t: &[u8]) {
        let t = cmf_wp2_ui::text::fit(t, 80).as_bytes();
        self.note[..t.len()].copy_from_slice(t);
        self.n_note = t.len();
    }

    fn note(&self) -> &str {
        core::str::from_utf8(&self.note[..self.n_note]).unwrap_or("")
    }

    fn message(&mut self, m: &[u8]) -> Got {
        // the reader's text (page.rs); late in page view: not over page view's picture (they share memory)
        let text = matches!(m.first(), Some(b'N' | b't' | b'F'));
        if text && self.screen == Screen::Page {
            return Got::None;
        }
        if text && page().apply(m) {
            if m[0] == b'N' {
                self.scroll = 0;
                self.n_note = 0;
                if self.screen != Screen::Keys {
                    self.screen = Screen::Reader;
                }
            }
            return if self.screen == Screen::Reader { Got::Redraw } else { Got::None };
        }
        match m.first() {
            Some(b'!') => {
                self.set_note(&m[1..]);
                Got::Redraw
            }
            Some(b'H') if m.len() >= 2 && usize::from(m[1]) < RECENT => {
                let i = usize::from(m[1]);
                let t = cmf_wp2_ui::text::fit(&m[2..], 48).as_bytes();
                self.recent[i].0[..t.len()].copy_from_slice(t);
                self.recent[i].1 = t.len();
                self.n_recent = self.n_recent.max(i + 1);
                if i == 0 {
                    self.n_recent = 1;
                }
                if self.screen == Screen::Start { Got::Redraw } else { Got::None }
            }
            Some(b'Z') => {
                self.to_start();
                Got::Redraw
            }
            Some(b'K') if self.screen == Screen::Page => {
                self.field = true;
                self.keys.open("Type here");
                self.screen = Screen::Keys;
                Got::Redraw
            }
            Some(b'A') if m.len() == 2 => {
                self.n_boxes = usize::from(m[1]).min(BOXES);
                Got::None
            }
            Some(b'a') if m.len() >= 2 => {
                let first = usize::from(m[1]);
                for (k, r) in m[2..].chunks_exact(9).enumerate() {
                    let i = first + k;
                    if i < BOXES {
                        let v = |j: usize| i16::from_le_bytes([r[j], r[j + 1]]);
                        self.boxes[i] = (v(0), v(2), v(4), v(6), r[8]);
                    }
                }
                Got::None
            }
            Some(b'M') if m.len() == 2 => {
                let was = self.video;
                self.video = m[1] == 1;
                // stopped: the page again
                if was && !self.video && self.screen == Screen::Page { Got::Redraw } else { Got::None }
            }
            Some(b'Y') if self.screen == Screen::Page => Got::Redraw,
            Some(b'G') => {
                // page view, whether asked or not: no text here, or a video
                if self.screen != Screen::Keys {
                    self.screen = Screen::Page;
                }
                self.pictured = false;
                self.video = false;
                self.n_note = 0;
                Got::Redraw
            }
            Some(b'J' | b'j') | Some(b'I' | b'D' | b'E') if self.screen == Screen::Page && !self.video => {
                match long().message(m) {
                    long::Got::Begin => {
                        self.pscroll = 0;
                        self.n_boxes = 0;
                        self.pictured = false;
                        Got::None
                    }
                    // kept, not drawn: the spinner shows how much came; the page once all of it has
                    long::Got::Band(..) => Got::None,
                    long::Got::Done => {
                        self.pictured = true;
                        self.n_note = 0;
                        Got::Redraw
                    }
                    long::Got::None => Got::None,
                }
            }
            Some(b'I' | b'D' | b'E') if self.screen == Screen::Page => {
                let st = self.pic.message(m, &mut Screen565(self.api));
                if st == Status::Begin && !self.pictured {
                    // the first picture: the loading words go
                    self.pictured = true;
                    self.n_note = 0;
                }
                match st {
                    Status::Ok | Status::Begin | Status::Done(_) => Got::Drew,
                    _ => Got::None,
                }
            }
            _ => Got::None,
        }
    }

    fn later(&mut self) {
        if self.api.now().wrapping_sub(self.shown_at) >= FRAME_MS {
            self.draw();
        } else {
            self.due = true;
        }
    }

    fn shown(&mut self) {
        self.api.show();
        self.shown_at = self.api.now();
    }

    fn to_start(&mut self) {
        self.screen = Screen::Start;
        self.video = false;
        self.pictured = false;
        page().clear();
        long().clear();
    }

    /// Back: the keyboard to where it came from, a page to the one before (the phone answers `Z` when there is
    /// none); false on the start screen (close).
    fn back(&mut self) -> bool {
        match self.screen {
            Screen::Start => return false,
            Screen::Keys if self.field => self.screen = Screen::Page,
            Screen::Keys => self.screen = self.keys_from,
            Screen::Reader | Screen::Page => {
                self.send(b"B");
                self.set_note(b"Going back\xE2\x80\xA6");
            }
        }
        self.motor.play(haptic::TICK);
        self.draw();
        true
    }

    // --- drawing

    fn draw(&mut self) {
        self.due = false;
        let mut s = self.api;
        match self.screen {
            Screen::Start => {
                Surface::rect(&mut s, 0, 0, 466, 466, 0);
                self.draw_start();
            }
            Screen::Keys => {
                Surface::rect(&mut s, 0, 0, 466, 466, 0);
                self.keys.draw(self.api);
            }
            Screen::Reader => self.draw_reader(),
            Screen::Page => {
                if !self.pictured && !self.video {
                    Surface::rect(&mut s, 0, 0, 466, 466, 0);
                    if self.loading() {
                        self.draw_spinner();
                    } else {
                        let t = if self.n_note > 0 { self.note() } else { "Nothing to show" };
                        centre_fit(&mut s, C, 222, t, 320, GREY);
                    }
                } else if !self.video {
                    self.draw_long();
                }
                self.draw_switch();
            }
        }
        self.shown();
    }

    fn draw_start(&self) {
        let mut s = self.api;
        dots_centre(&mut s, C - 8, 34, "WEB", 3, 2, W);
        disc(&mut s, C + 34, 52, 4, RED, BLACK);
        let (y, h, w) = WHERE;
        rrect(&mut s, C - w / 2, y, w, h, h / 2, CARD, BLACK);
        centre_fit(&mut s, C, y + h / 2 - 12, "Search or address", w - 30, W);
        if self.n_note > 0 {
            centre_fit(&mut s, C, 166, self.note(), 320, SOFT);
        }
        for i in 0..self.n_recent {
            let y = ROWS_Y + 4 + i as i32 * ROW_H;
            let (x0, x1) = cmf_wp2_ui::geom::chord(y, y + ROW_H - 8, 30);
            rrect(&mut s, x0, y, x1 - x0, ROW_H - 8, 18, CARD, BLACK);
            let (t, n) = &self.recent[i];
            let t = core::str::from_utf8(&t[..*n]).unwrap_or("");
            font::fitted(&mut s, x0 + 18, y + 13, t, x1 - x0 - 36, SOFT);
        }
    }

    fn draw_reader(&mut self) {
        let mut s = self.api;
        let p = page();
        Surface::rect(&mut s, 0, 0, 466, 466, 0);
        let title = if p.n_blocks == 0 && self.n_note > 0 { "" } else { p.title() };
        centre_fit(&mut s, C, 28, title, 240, GREY);
        if p.n_blocks == 0 {
            if self.loading() {
                self.draw_spinner();
            } else {
                let t = if self.n_note > 0 { self.note() } else { "Loading\u{2026}" };
                centre_fit(&mut s, C, 222, t, 320, GREY);
            }
        }
        let x0 = C - page::WIDTH / 2;
        for l in &p.lines[..p.n_lines] {
            let y = TOP + i32::from(l.y) - self.scroll;
            if y < TOP - 4 || y + page::LH > BOT + 4 {
                continue;
            }
            let b = p.blocks[usize::from(l.block)];
            let (x, c) = match b.kind {
                page::HEADING => (x0, W),
                page::LINK => (x0, LINKC),
                page::ITEM => (x0 + page::INDENT, SOFT),
                _ => (x0, SOFT),
            };
            if b.kind == page::ITEM && l.first != 0 {
                disc(&mut s, x0 + 6, y + 11, 3, Rgb(0xA0, 0xA0, 0xA0), BLACK);
            }
            font::fitted(&mut s, x, y, p.line_text(l), page::WIDTH, c);
        }
        if p.n_blocks > 0 && self.n_note > 0 {
            // what the phone is doing (loading a link): over the text's top
            rrect(&mut s, C - 150, TOP - 8, 300, 36, 18, CARD, BLACK);
            centre_fit(&mut s, C, TOP, self.note(), 270, W);
        }
        self.draw_switch();
    }

    /// Something on its way: page view's page (till all of it came), or the reader's text.
    fn loading(&self) -> bool {
        match self.screen {
            // an error note ("Couldn't load") stops it
            Screen::Page => !self.pictured && !self.video && (self.n_note == 0 || self.note[..self.n_note].ends_with(b"\xE2\x80\xA6")),
            // a note that ends "…" is under way ("Loading…"); others ("Couldn't load") are not
            Screen::Reader => page().n_blocks == 0 && self.note[..self.n_note].ends_with(b"\xE2\x80\xA6"),
            _ => false,
        }
    }

    /// A turning ring of dots and what is loading (how much of a page came).
    fn draw_spinner(&self) {
        let mut s = self.api;
        let (cx, cy) = (C, 196);
        let head = (self.api.now() / FRAME_MS) as usize % SPIN.len();
        for (i, &(dx, dy)) in SPIN.iter().enumerate() {
            // brightest at the head, fading behind it
            let age = (head + SPIN.len() - i) % SPIN.len();
            let v = 255u32.saturating_sub(age as u32 * 22).max(40) as u8;
            let c = if age == 0 { RED } else { Rgb(v, v, v) };
            disc(&mut s, cx + dx, cy + dy, 6, c, BLACK);
        }
        let l = long();
        let mut buf = [0u8; 40];
        let t: &str = if self.n_note > 0 {
            self.note()
        } else if self.screen == Screen::Page && l.height > 0 {
            let pc = (l.came() * 100 / l.height).clamp(0, 99) as u32;
            let mut n = 0;
            for &b in b"Loading the page " {
                buf[n] = b;
                n += 1;
            }
            if pc >= 10 {
                buf[n] = b'0' + (pc / 10) as u8;
                n += 1;
            }
            buf[n] = b'0' + (pc % 10) as u8;
            buf[n + 1] = b'%';
            core::str::from_utf8(&buf[..n + 2]).unwrap_or("")
        } else {
            "Loading\u{2026}"
        };
        centre_fit(&mut s, C, 262, t, 320, GREY);
    }

    /// Page view's page where it is scrolled to; below what came, black (and why, at the end).
    fn draw_long(&mut self) {
        let mut s = self.api;
        let l = long();
        let came = l.came();
        let end = came - self.pscroll;
        if end < 466 {
            Surface::rect(&mut s, 0, end.max(0), 466, 466 - end.max(0), 0);
            if l.done != 0 && end < 360 {
                let t = if came < l.height { "That's all that fits on the watch" } else { "End of the page" };
                centre_fit(&mut s, C, end.max(0) + 30, t, 300, GREY);
            }
        }
        l.draw(&mut self.pic, &mut Screen565(self.api), self.pscroll, 0, end.min(466));
        if self.n_note > 0 {
            rrect(&mut s, C - 150, 20, 300, 36, 18, CARD, BLACK);
            centre_fit(&mut s, C, 28, self.note(), 270, W);
        }
    }

    /// The bottom bar: the search box, and the round button: "Page" in reader view, "Text" in page view.
    fn draw_switch(&self) {
        let mut s = self.api;
        let (cx, y, w, h) = SEARCH;
        rrect(&mut s, cx - w / 2, y, w, h, h / 2, CARD, BLACK);
        // a magnifier, then the words
        disc(&mut s, cx - w / 2 + 24, y + 19, 8, Rgb(0x80, 0x80, 0x80), CARD);
        disc(&mut s, cx - w / 2 + 24, y + 19, 5, CARD, Rgb(0x80, 0x80, 0x80));
        Surface::rect(&mut s, cx - w / 2 + 29, y + 25, 3, 7, GREY);
        font::fitted(&mut s, cx - w / 2 + 42, y + h / 2 - 12, "Search", w - 56, GREY);
        let (cx, y, w, h) = SWITCH;
        let label = if self.screen == Screen::Page { "Text" } else { "Page" };
        rrect(&mut s, cx - w / 2, y, w, h, h / 2, WHITE, BLACK);
        centre_fit(&mut s, cx, y + h / 2 - 12, label, w - 16, 0);
    }

    fn on_search(&self, x: i32, y: i32) -> bool {
        let (cx, top, w, h) = SEARCH;
        matches!(self.screen, Screen::Reader | Screen::Page) && (top - 8..top + h + 12).contains(&y) && (x - cx).abs() <= w / 2 + 4
    }

    /// The search box: the keyboard, back to here if left.
    fn search(&mut self) {
        self.motor.play(haptic::TICK);
        self.field = false;
        self.keys_from = self.screen;
        self.keys.open("Search or address");
        self.screen = Screen::Keys;
        self.draw();
    }

    // --- input

    fn on_switch(&self, x: i32, y: i32) -> bool {
        let (cx, top, w, h) = SWITCH;
        matches!(self.screen, Screen::Reader | Screen::Page) && (top - 8..top + h + 12).contains(&y) && (x - cx).abs() <= w / 2 + 4
    }

    fn touch_down(&mut self, e: Event) {
        let (x, y) = (i32::from(e.x), i32::from(e.y));
        self.down = Some((x, y));
        if self.screen != Screen::Keys {
            return;
        }
        // the keyboard types at the touch (as Spotify's)
        if self.keys.on_text(x, y) {
            self.motor.play(haptic::TICK);
            self.keys.to_keys();
            return self.draw();
        }
        if self.keys.rotary {
            let a = self.keys.dial.down(&mut { self.api }, x, y, self.api.now());
            return self.dial(a);
        }
        match self.keys.down(self.api, x, y) {
            Tap::Shown => self.shown(),
            Tap::Typed => {
                self.motor.play(haptic::TICK);
                self.shown();
            }
            Tap::Redraw => {
                self.motor.play(haptic::TICK);
                self.draw();
            }
            Tap::Go => self.go(),
        }
    }

    fn dial(&mut self, a: Act) {
        match a {
            Act::None => {}
            Act::Show => {
                self.motor.play(haptic::TICK);
                self.shown();
            }
            Act::Redraw => self.draw(),
            Act::Key(_) => {
                self.motor.play(haptic::TICK);
                if self.keys.dial_key(self.api, a) {
                    self.go();
                } else {
                    self.shown();
                }
            }
        }
    }

    /// The typed words: into the page's field, or a search or an address.
    fn go(&mut self) {
        self.motor.play(haptic::TICK);
        let mut m = [0u8; 1 + crate::keys::MAX];
        let n = self.keys.n;
        m[0] = if self.field { b'W' } else { b'Q' };
        m[1..1 + n].copy_from_slice(&self.keys.text[..n]);
        self.send(&m[..1 + n]);
        if self.field {
            self.field = false;
            self.screen = Screen::Page;
            self.pictured = false;
            self.set_note(b"Sending\xE2\x80\xA6");
        } else {
            page().clear();
            self.scroll = 0;
            self.screen = Screen::Reader;
            self.set_note(b"Loading\xE2\x80\xA6");
        }
        self.draw();
    }

    fn touch_up(&mut self, e: Event) {
        let Some((x, y)) = self.down.take() else { return };
        if self.screen == Screen::Keys {
            if self.keys.rotary {
                let a = self.keys.dial.up(&mut { self.api }, i32::from(e.x), i32::from(e.y));
                self.dial(a);
            }
            return;
        }
        let dy = i32::from(e.y) - y;
        let dragged = dy.abs() > DRAG && e.y != 0;
        match self.screen {
            Screen::Start if !dragged => self.start_tap(x, y),
            Screen::Reader if dragged => self.scroll_by(-dy),
            Screen::Page if dragged => self.page_scroll(-dy),
            _ if self.on_switch(x, y) => self.switch(),
            _ if self.on_search(x, y) => self.search(),
            Screen::Reader => self.reader_tap(y),
            Screen::Page => {
                self.motor.play(haptic::TICK);
                // to the nearest tappable place in reach (its middle), outlined till the phone answers
                let (x, y) = match self.nearest(x, y + self.pscroll) {
                    Some((bx, by, bw, bh)) => {
                        let by = by - self.pscroll;
                        let mut s = self.api;
                        let c = rgb565(0xD7, 0x19, 0x21);
                        Surface::rect(&mut s, bx - 2, by - 2, bw + 4, 3, c);
                        Surface::rect(&mut s, bx - 2, by + bh - 1, bw + 4, 3, c);
                        Surface::rect(&mut s, bx - 2, by - 2, 3, bh + 4, c);
                        Surface::rect(&mut s, bx + bw - 1, by - 2, 3, bh + 4, c);
                        self.shown();
                        (bx + bw / 2, by + bh / 2 + self.pscroll)
                    }
                    None => (x, y + self.pscroll),
                };
                let (a, b) = ((x.clamp(0, 465) as u16).to_le_bytes(), (y.clamp(0, 65535) as u16).to_le_bytes());
                self.send(&[b'T', a[0], a[1], b[0], b[1]]);
            }
            _ => {}
        }
    }

    /// The tappable place under (x, y) or nearest to it within REACH: its box on the panel.
    fn nearest(&self, x: i32, y: i32) -> Option<(i32, i32, i32, i32)> {
        let mut best: Option<((i32, i32, i32, i32), i32)> = None;
        for &(bx, by, bw, bh, _) in &self.boxes[..self.n_boxes] {
            let (bx, by, bw, bh) = (i32::from(bx), i32::from(by), i32::from(bw), i32::from(bh));
            let dx = (bx - x).max(x - (bx + bw)).max(0);
            let dy = (by - y).max(y - (by + bh)).max(0);
            let d = dx.max(dy);
            // inside several (a link in a button): the smallest
            let key = if d == 0 { -(10_000 - (bw * bh).min(9_999)) } else { d };
            if d <= REACH && best.is_none_or(|(_, k)| key < k) {
                best = Some(((bx, by, bw, bh), key));
            }
        }
        best.map(|(b, _)| b)
    }

    fn start_tap(&mut self, x: i32, y: i32) {
        let (wy, wh, ww) = WHERE;
        if (wy - 8..wy + wh + 8).contains(&y) && (x - C).abs() <= ww / 2 + 8 {
            self.motor.play(haptic::TICK);
            self.field = false;
            self.keys_from = Screen::Start;
            self.keys.open("Search or address");
            self.screen = Screen::Keys;
            return self.draw();
        }
        let r = (y - ROWS_Y).div_euclid(ROW_H);
        if y >= ROWS_Y && (0..self.n_recent as i32).contains(&r) {
            self.motor.play(haptic::TICK);
            self.send(&[b'O', r as u8]);
            page().clear();
            self.scroll = 0;
            self.screen = Screen::Reader;
            self.set_note(b"Loading\xE2\x80\xA6");
            self.draw();
        }
    }

    fn reader_tap(&mut self, y: i32) {
        if !(TOP..BOT).contains(&y) {
            return;
        }
        let p = page();
        let Some(l) = p.line_at(y - TOP + self.scroll) else { return };
        let b = p.blocks[usize::from(l.block)];
        if b.link == 255 {
            return;
        }
        self.motor.play(haptic::TICK);
        self.send(&[b'L', b.link]);
        self.set_note(b"Opening\xE2\x80\xA6");
        self.draw();
    }

    /// Reader ⇄ page view.
    fn switch(&mut self) {
        self.motor.play(haptic::TICK);
        if self.screen == Screen::Reader {
            self.screen = Screen::Page;
            self.pictured = false;
            self.n_note = 0;
            self.send(b"V\x01");
        } else {
            self.screen = Screen::Reader;
            self.video = false;
            self.send(b"V\x00");
        }
        self.draw();
    }

    fn scroll_by(&mut self, d: i32) {
        let max = (page().height - (BOT - TOP) + page::LH).max(0);
        let s = (self.scroll + d).clamp(0, max);
        if s != self.scroll {
            self.scroll = s;
            self.later();
        }
    }

    /// Page view scrolls here: the page came whole (as much as fits).
    fn page_scroll(&mut self, d: i32) {
        if self.video || !self.pictured {
            return;
        }
        let max = (long().came() - 300).max(0);
        let s = (self.pscroll + d).clamp(0, max);
        if s != self.pscroll {
            self.pscroll = s;
            self.later();
        }
    }

    fn crown(&mut self, d: i32) {
        match self.screen {
            Screen::Keys if self.keys.rotary => {
                let a = self.keys.dial.crown(&mut { self.api }, d.signum(), self.api.now());
                self.dial(a);
            }
            Screen::Reader => self.scroll_by(d.signum() * 3 * page::LH / 2),
            Screen::Page => self.page_scroll(d.signum() * 140),
            _ => {}
        }
    }
}

fn centre_fit(s: &mut &'static Api, cx: i32, y: i32, text: &str, px: i32, c: u16) {
    let w = font::width(text).min(px);
    font::fitted(s, cx - w / 2, y, text, px, c);
}
