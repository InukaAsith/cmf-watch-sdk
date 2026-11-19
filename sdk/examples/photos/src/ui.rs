//! Photos on the watch (nothing kept: its files go when it closes, and at the next start after a swipe closed it): the phone's gallery as a grid of thumbnails or a list of names (tap the heading to switch),
//! scrolled by a drag or the crown; a tap opens the picture full screen, where the crown or a sideways drag goes to
//! the next or previous one, a double tap zooms in there (zoom mode: the crown zooms from ½× to 8×, a drag pans, a double tap or the button
//! leaves)
//! and the button goes back. Thumbnails are kept in the app's files: asked for once,
//! only when they scroll into view.

use crate::proto::{thumb_name, Gallery, MAX};
use crate::rle::{self, Decoder, Status, HEADER};
use cmf_wp2_app::{Api, Event, BUTTON, CROWN, FILE_CHUNK, MESSAGE, MSG_MAX, PAUSE, RESUME, TOUCH_DOWN, TOUCH_UP};
use cmf_wp2_ui::color::{rgb565, Rgb, BLACK};
use cmf_wp2_ui::draw::{disc, rrect};
use cmf_wp2_ui::font::{self, fitted};
use cmf_wp2_ui::geom::chord;
use cmf_wp2_ui::haptic::{self, Motor};
use cmf_wp2_ui::text::dots_centre;
use cmf_wp2_ui::{C, Surface};

cmf_wp2_app::entry!(main);
cmf_wp2_app::app_icon!();

const W: u16 = 0xFFFF;
const RED: Rgb = Rgb(0xD7, 0x19, 0x21);
const GREY: u16 = rgb565(0x8A, 0x8A, 0x8A);
const CARD: Rgb = Rgb(0x16, 0x16, 0x16);

const HEAD: i32 = 96;
const COLS: i32 = 3;
const CELL: i32 = 104;
const PITCH: i32 = 114;
const GX0: i32 = C - PITCH - CELL / 2;
const ROW: i32 = 56;
/// Moved more than this between down and up: a drag, not a tap.
const DRAG: i32 = 14;
const SWIPE: i32 = 70;

const STORE_CAP: usize = 40 * 1024;
const THUMB_CAP: usize = 12 * 1024;

struct Store {
    header: [u8; HEADER],
    data: [u8; STORE_CAP],
    len: usize,
    ok: bool,
}
static mut STORE: Store = Store { header: [0; HEADER], data: [0; STORE_CAP], len: 0, ok: false };
static mut THUMB: [u8; THUMB_CAP] = [0; THUMB_CAP];
static mut CHUNK: [u8; FILE_CHUNK] = [0; FILE_CHUNK];
static mut GALLERY: Gallery = Gallery::new();

fn store() -> &'static mut Store {
    unsafe { &mut *core::ptr::addr_of_mut!(STORE) }
}
fn gallery() -> &'static mut Gallery {
    unsafe { &mut *core::ptr::addr_of_mut!(GALLERY) }
}

/// The panel, rows above `top` left alone (the heading stays over scrolled thumbnails).
struct Clip(&'static Api, i32);

/// A round picture drawn smaller than the panel: only its disc (its corners are filler colour, not black).
struct Round {
    api: &'static Api,
    cx: i32,
    cy: i32,
    r: i32,
}
impl rle::Surface for Round {
    fn rect(&mut self, x: i32, y: i32, w: i32, h: i32, c: u16) {
        let dy = (y + h / 2 - self.cy).abs();
        if dy > self.r {
            return;
        }
        let dx = cmf_wp2_ui::motion::isqrt(self.r * self.r - dy * dy);
        let (x0, x1) = (x.max(self.cx - dx).max(0), (x + w).min(self.cx + dx).min(466));
        let (y0, y1) = (y.max(0), (y + h).min(466));
        if x1 > x0 && y1 > y0 {
            let mut a = self.api;
            Surface::rect(&mut a, x0, y0, x1 - x0, y1 - y0, c);
        }
    }
}
impl rle::Surface for Clip {
    fn rect(&mut self, x: i32, y: i32, w: i32, h: i32, c: u16) {
        // clipped to the panel here: a zoomed picture's runs reach far off it, past what i16 holds
        let (x0, x1) = (x.max(0), (x + w).min(466));
        let (y0, y1) = (y.max(self.1), (y + h).min(466));
        if x1 > x0 && y1 > y0 {
            let mut a = self.0;
            Surface::rect(&mut a, x0, y0, x1 - x0, y1 - y0, c);
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Screen {
    Gallery,
    Viewer,
}

struct Incoming {
    name: [u8; 20],
    nlen: usize,
    len: usize,
    on: bool,
}

struct Ui {
    api: &'static Api,
    screen: Screen,
    grid: bool,
    scroll: i32,
    asked: [bool; MAX],
    open: usize,
    /// the opened picture as drawn now (moved by local zooms until the next comes)
    view: Option<(i32, i32, u8)>,
    /// where the picture sits unzoomed (as it came)
    base: (i32, i32, u8),
    /// the last single tap in the viewer, for a double tap
    last_tap: Option<(i32, i32, u32)>,
    /// zoom mode (double tap): the crown zooms instead of going through the pictures
    zooming: bool,
    dec: Decoder,
    loaded: bool,
    inc: Incoming,
    save_list: bool,
    down: Option<(i32, i32)>,
    motor: Motor,
}

impl Ui {
    // --- gallery

    fn rows(&self) -> i32 {
        let n = gallery().n as i32;
        if self.grid { (n + COLS - 1) / COLS } else { n }
    }

    fn max_scroll(&self) -> i32 {
        let content = if self.grid { self.rows() * PITCH } else { self.rows() * ROW };
        (content - (466 - HEAD - 60)).max(0)
    }

    fn cell(&self, i: usize) -> (i32, i32) {
        let i = i as i32;
        if self.grid {
            (GX0 + (i % COLS) * PITCH, HEAD + 6 + (i / COLS) * PITCH - self.scroll)
        } else {
            (0, HEAD + 4 + i * ROW - self.scroll)
        }
    }

    /// The gallery, each cell painted over the last frame's (no black frame between: that flickers on the panel);
    /// only the gaps are cleared. `head`: the heading too (a scroll leaves it).
    fn draw_gallery_(&mut self, head: bool) {
        let api = self.api;
        let mut s = api;
        let g = gallery();
        if g.n == 0 {
            Surface::rect(&mut s, 0, HEAD, 466, 466 - HEAD, 0);
            font::centre(&mut s, C, 210, if self.loaded { "No photos yet" } else { "Waiting for phone" }, W);
            font::centre(&mut s, C, 240, "Pick photos in CMFX", GREY);
        } else if self.grid {
            let mut cl = Clip(api, HEAD);
            let right = GX0 + (COLS - 1) * PITCH + CELL;
            for r in 0..self.rows() {
                let top = HEAD + r * PITCH - self.scroll;
                if top + PITCH <= HEAD || top >= 466 {
                    continue;
                }
                cl.rect_(0, top, GX0, PITCH, 0);
                cl.rect_(right, top, 466 - right, PITCH, 0);
                cl.rect_(GX0, top, right - GX0, 6, 0);
                cl.rect_(GX0, top + 6 + CELL, right - GX0, PITCH - 6 - CELL, 0);
                for k in 0..COLS - 1 {
                    cl.rect_(GX0 + CELL + k * PITCH, top + 6, PITCH - CELL, CELL, 0);
                }
                for c in 0..COLS {
                    let i = (r * COLS + c) as usize;
                    if i < g.n {
                        self.draw_item(i);
                    } else {
                        cl.rect_(GX0 + c * PITCH, top + 6, CELL, CELL, 0);
                    }
                }
            }
            let end = HEAD + self.rows() * PITCH - self.scroll;
            cl.rect_(0, end, 466, 466 - end, 0);
        } else {
            let mut cl = Clip(api, HEAD);
            for i in 0..g.n {
                let (_, y) = self.cell(i);
                if y + ROW <= HEAD || y >= 466 {
                    continue;
                }
                self.draw_item(i);
            }
            cl.rect_(0, HEAD, 466, 4 - self.scroll, 0);
            let end = HEAD + 4 + g.n as i32 * ROW - self.scroll;
            cl.rect_(0, end, 466, 466 - end, 0);
        }
        if head {
            self.draw_head();
        }
        api.show();
    }

    fn draw_gallery(&mut self) {
        self.draw_gallery_(true);
    }

    fn draw_head(&self) {
        let mut s = self.api;
        Surface::rect(&mut s, 0, 0, 466, HEAD, 0);
        dots_centre(&mut s, C - 8, 22, "PHOTOS", 3, 2, W);
        disc(&mut s, C + 66, 22 + 18, 4, RED, BLACK);
        // the view switch: the open one in white
        let (gw, lw) = (font::width("Grid"), font::width("List"));
        let x = C - (gw + 24 + lw) / 2;
        font::draw(&mut s, x, 56, "Grid", if self.grid { W } else { GREY });
        disc(&mut s, x + gw + 12, 56 + 11, 2, Rgb(0x55, 0x55, 0x55), BLACK);
        font::draw(&mut s, x + gw + 24, 56, "List", if self.grid { GREY } else { W });
    }

    fn draw_item(&mut self, i: usize) {
        let (x, y) = self.cell(i);
        let mut s = self.api;
        if !self.grid {
            if y + ROW <= HEAD || y >= 466 {
                return;
            }
            let (x0, x1) = chord(y.max(HEAD), (y + ROW - 8).min(466), 24);
            if x1 - x0 < 120 {
                Clip(self.api, HEAD).rect_(0, y, 466, ROW, 0);
                return;
            }
            let top = y.max(HEAD);
            let bottom = (y + ROW - 8).min(466);
            let mut cl = Clip(self.api, HEAD);
            cl.rect_(0, y, x0, ROW, 0);
            cl.rect_(x1, y, 466 - x1, ROW, 0);
            cl.rect_(x0, y + ROW - 8, x1 - x0, 8, 0);
            if bottom > top {
                Surface::rect(&mut s, x0, top, x1 - x0, bottom - top, rgb565(0x12, 0x12, 0x12));
            }
            if y >= HEAD {
                disc(&mut s, x0 + 20, y + 24, 4, if i == self.open { RED } else { Rgb(0x55, 0x55, 0x55) }, Rgb(0x12, 0x12, 0x12));
                fitted(&mut s, x0 + 36, y + 13, gallery().items[i].name(), x1 - x0 - 52, W);
            }
            return;
        }
        let mut b = [0u8; 6];
        let name = thumb_name(i, &mut b);
        let buf = unsafe { &mut *core::ptr::addr_of_mut!(THUMB) };
        let n = self.api.file_read(name, 0, buf);
        let mut d = Decoder::new();
        if n > HEADER as i32 && d.start(&buf[..HEADER], x, y, buf[5].max(1)) {
            d.feed(&buf[HEADER..n as usize], &mut Clip(self.api, HEAD));
        } else {
            if y >= HEAD {
                // the corners black first: a picture drawn here before would show through them
                for (cx, cy) in [(x, y), (x + CELL - 14, y), (x, y + CELL - 14), (x + CELL - 14, y + CELL - 14)] {
                    Surface::rect(&mut s, cx, cy, 14, 14, 0);
                }
                rrect(&mut s, x, y, CELL, CELL, 14, CARD, BLACK);
            } else {
                Clip(self.api, HEAD).rect_(x, y, CELL, CELL, rgb565(0x16, 0x16, 0x16));
            }
            if !self.asked[i] && self.loaded {
                self.asked[i] = true;
                let _ = self.api.send(&[b'q', i as u8]);
            }
        }
    }

    fn at(&self, x: i32, y: i32) -> Option<usize> {
        if y < HEAD {
            return None;
        }
        let i = if self.grid {
            let col = (x - GX0).div_euclid(PITCH);
            let row = (y - HEAD - 6 + self.scroll).div_euclid(PITCH);
            if !(0..COLS).contains(&col) || row < 0 {
                return None;
            }
            row * COLS + col
        } else {
            (y - HEAD - 4 + self.scroll).div_euclid(ROW)
        };
        (i >= 0 && (i as usize) < gallery().n).then_some(i as usize)
    }

    fn scroll_by(&mut self, d: i32) {
        let next = (self.scroll + d).clamp(0, self.max_scroll());
        if next != self.scroll {
            self.scroll = next;
            self.draw_gallery_(false);
        }
    }

    fn gallery_tap(&mut self, x: i32, y: i32) {
        if y < HEAD {
            // the heading: grid ⇄ list, keeping the top item in view
            self.motor.play(haptic::TICK);
            let top = if self.grid { (self.scroll / PITCH) * COLS } else { self.scroll / ROW };
            self.grid = !self.grid;
            self.scroll = (if self.grid { (top / COLS) * PITCH } else { top * ROW }).clamp(0, self.max_scroll());
            let _ = self.api.save(&[self.grid as u8]);
            return self.draw_gallery();
        }
        if let Some(i) = self.at(x, y) {
            self.motor.play(haptic::TICK);
            self.open_picture(i);
        }
    }

    // --- viewer

    fn open_picture(&mut self, i: usize) {
        self.zooming = false;
        self.open = i;
        self.screen = Screen::Viewer;
        self.view = None;
        store().ok = false;
        let mut s = self.api;
        Surface::rect(&mut s, 0, 0, 466, 466, 0);
        font::centre(&mut s, C, 222, gallery().items[i].name(), GREY);
        self.api.show();
        let _ = self.api.send(&[b'o', i as u8]);
    }

    fn redraw(&self, x0: i32, y0: i32, scale: u8) -> bool {
        let st = store();
        if !st.ok {
            return false;
        }
        let mut d = Decoder::new();
        if !d.start(&st.header, x0, y0, scale) {
            return false;
        }
        let mut s = self.api;
        Surface::rect(&mut s, 0, 0, 466, 466, 0);
        if scale < self.base.2 {
            let r = i32::from(d.w) * i32::from(scale) / 2;
            d.feed(&st.data[..st.len], &mut Round { api: self.api, cx: x0 + r, cy: y0 + r, r });
        } else {
            d.feed(&st.data[..st.len], &mut Clip(self.api, 0));
        }
        self.api.show();
        true
    }

    fn zoomed(&self) -> bool {
        self.zooming
    }

    /// Double tap: into zoom mode at 2× about (tx, ty); in it, back to the whole picture and out.
    fn zoom_at(&mut self, tx: i32, ty: i32) {
        let Some((x0, y0, s)) = self.view else { return };
        self.motor.play(haptic::TICK);
        if self.zooming {
            return self.leave_zoom();
        }
        self.zooming = true;
        let (x, y, n) = (C - (tx - x0) * 2, C - (ty - y0) * 2, (s * 2).min(8));
        if self.redraw(x, y, n) {
            self.view = Some((x, y, n));
            self.pan(0, 0);
            self.zoom_label();
        }
    }

    fn leave_zoom(&mut self) {
        self.zooming = false;
        let (x, y, n) = self.base;
        if self.redraw(x, y, n) {
            self.view = Some(self.base);
        }
    }

    /// Crown in zoom mode: 2× more or less about the panel's centre, from half the whole picture to 8×.
    fn zoom_by(&mut self, d: i32) {
        let Some((x0, y0, s)) = self.view else { return };
        let min = (self.base.2 / 2).max(1);
        let next = if d > 0 { (s * 2).min(8) } else { (s / 2).max(min) };
        if next == s {
            return;
        }
        self.motor.play(haptic::TICK);
        let (x, y) = if next > s { (C - (C - x0) * 2, C - (C - y0) * 2) } else { (C - (C - x0) / 2, C - (C - y0) / 2) };
        if self.redraw(x, y, next) {
            self.view = Some((x, y, next));
            self.pan(0, 0);
            self.zoom_label();
        }
    }

    /// The zoom level at the top while in zoom mode.
    fn zoom_label(&self) {
        let Some((_, _, s)) = self.view else { return };
        let b = self.base.2.max(1);
        let t = match (s * 4 / b) as u32 {
            0..=2 => "\u{bd}\u{d7}",
            3..=4 => "1\u{d7}",
            5..=8 => "2\u{d7}",
            9..=16 => "4\u{d7}",
            _ => "8\u{d7}",
        };
        let mut a = self.api;
        rrect(&mut a, C - 28, 14, 56, 30, 15, Rgb(0x10, 0x10, 0x10), BLACK);
        font::centre(&mut a, C, 18, t, W);
        self.api.show();
    }

    /// Moves a zoomed picture, never past its edges.
    fn pan(&mut self, dx: i32, dy: i32) {
        let Some((x0, y0, s)) = self.view else { return };
        let st = store();
        let w = i32::from(u16::from_le_bytes([st.header[1], st.header[2]])) * i32::from(s);
        let h = i32::from(u16::from_le_bytes([st.header[3], st.header[4]])) * i32::from(s);
        let x = (x0 + dx).clamp((466 - w).min(0), 0.max(466 - w));
        let y = (y0 + dy).clamp((466 - h).min(0), 0.max(466 - h));
        if (x, y) != (x0, y0) && self.redraw(x, y, s) {
            self.view = Some((x, y, s));
            self.zoom_label();
        }
    }

    fn step(&mut self, d: i32) {
        let n = gallery().n;
        if n > 1 {
            self.motor.play(haptic::TICK);
            let next = if d > 0 { (self.open + 1) % n } else { (self.open + n - 1) % n };
            self.open_picture(next);
        }
    }

    // --- messages

    fn messages(&mut self) {
        let mut buf = [0u8; MSG_MAX];
        let mut list = false;
        let mut drew = false;
        while let Some(m) = self.api.recv(&mut buf) {
            let g = gallery();
            if m.first() == Some(&b'L') && m.len() == 6 {
                let key = u32::from_le_bytes([m[2], m[3], m[4], m[5]]);
                if key != self.saved_key() {
                    // another set of pictures: the old thumbnails go
                    for i in 0..MAX {
                        let mut b = [0u8; 6];
                        let _ = self.api.file_delete(thumb_name(i, &mut b));
                    }
                    let _ = self.api.file_write("key", &key.to_le_bytes(), false);
                    self.save_list = true;
                    let _ = self.api.file_write("list", &[m.len() as u8], false);
                    let _ = self.api.file_write("list", m, true);
                } else {
                    self.save_list = false;
                }
                self.asked = [false; MAX];
                self.loaded = true;
            }
            if m.first() == Some(&b'l') && self.save_list {
                // kept with the thumbnails: the gallery opens without the phone
                let _ = self.api.file_write("list", &[m.len() as u8], true);
                let _ = self.api.file_write("list", m, true);
            }
            if g.apply(m) {
                list = true;
                continue;
            }
            match m.first() {
                Some(b'F') if (2..=21).contains(&m.len()) => {
                    self.flush();
                    let n = m.len() - 1;
                    self.inc.name[..n].copy_from_slice(&m[1..]);
                    self.inc.nlen = n;
                    self.inc.on = true;
                    let _ = self.api.file_write(self.name(), &[], false);
                }
                Some(b'f') if self.inc.on => {
                    let chunk = unsafe { &mut *core::ptr::addr_of_mut!(CHUNK) };
                    for &b in &m[1..] {
                        if self.inc.len == FILE_CHUNK {
                            self.flush();
                        }
                        chunk[self.inc.len] = b;
                        self.inc.len += 1;
                    }
                }
                Some(b'c') => {
                    self.flush();
                    self.inc.on = false;
                    let _ = self.api.send(b"a");
                    // a thumbnail: drawn where it belongs
                    let nm = &self.inc.name[..self.inc.nlen];
                    if self.screen == Screen::Gallery && self.grid && nm.starts_with(b"th") {
                        let mut i = 0usize;
                        for &d in &nm[2..] {
                            i = i * 10 + (d - b'0') as usize;
                        }
                        if i < g.n {
                            self.draw_item(i);
                            self.draw_head();
                            drew = true;
                        }
                    }
                }
                Some(b'?') => {
                    let _ = self.api.send(b"R");
                }
                Some(b'I' | b'D' | b'E') if self.screen == Screen::Viewer => {
                    let st = store();
                    match m[0] {
                        b'I' if m.len() == HEADER => {
                            st.header.copy_from_slice(m);
                            st.len = 0;
                            st.ok = false;
                        }
                        b'D' if m.len() >= 2 => {
                            let t = &m[2..];
                            if st.len + t.len() <= STORE_CAP {
                                st.data[st.len..st.len + t.len()].copy_from_slice(t);
                            }
                            st.len += t.len();
                        }
                        _ => {}
                    }
                    match self.dec.message(m, &mut Clip(self.api, 0)) {
                        Status::Begin => {
                            let mut s = self.api;
                            Surface::rect(&mut s, 0, 0, 466, 466, 0);
                        }
                        Status::Done(_) => {
                            st.ok = st.len <= STORE_CAP;
                            self.view = Some(self.dec.origin());
                            self.base = self.dec.origin();
                        }
                        Status::Gap(s) => {
                            let _ = self.api.send(&[b'G', s]);
                        }
                        _ => {}
                    }
                    drew = true;
                }
                _ => {}
            }
        }
        if list && self.screen == Screen::Gallery {
            self.scroll = self.scroll.clamp(0, self.max_scroll());
            self.draw_gallery();
        } else if drew {
            self.api.show();
        }
    }

    /// Every file this app keeps (thumbnails, the list, the key): Photos leaves nothing on the watch. At the start
    /// too, for a session the system ended (a swipe closes the app without a word to it).
    fn wipe(&self) {
        for i in 0..MAX {
            let mut b = [0u8; 6];
            let _ = self.api.file_delete(thumb_name(i, &mut b));
        }
        for f in ["key", "list"] {
            let _ = self.api.file_delete(f);
        }
    }

    fn saved_key(&self) -> u32 {
        let mut b = [0u8; 4];
        if self.api.file_read("key", 0, &mut b) == 4 { u32::from_le_bytes(b) } else { u32::MAX }
    }

    fn name(&self) -> &str {
        core::str::from_utf8(&self.inc.name[..self.inc.nlen]).unwrap_or("x")
    }

    fn flush(&mut self) {
        if self.inc.on && self.inc.len > 0 {
            let chunk = unsafe { &*core::ptr::addr_of!(CHUNK) };
            let _ = self.api.file_write(self.name(), &chunk[..self.inc.len], true);
        }
        self.inc.len = 0;
    }

    // --- input

    fn event(&mut self, e: Event) -> bool {
        match e.kind {
            BUTTON => {
                if self.screen == Screen::Viewer && self.zooming {
                    self.leave_zoom();
                    return false;
                }
                if self.screen == Screen::Viewer {
                    self.screen = Screen::Gallery;
                    self.draw_gallery();
                    return false;
                }
                self.wipe();
                return true;
            }
            CROWN => match self.screen {
                Screen::Gallery => {
                    let step = if self.grid { PITCH / 2 } else { ROW };
                    self.scroll_by(i32::from(e.delta).clamp(-4, 4) * step);
                }
                // the crown goes through the pictures; on a zoomed one (double tap) it zooms
                Screen::Viewer if self.zoomed() => self.zoom_by(i32::from(e.delta)),
                Screen::Viewer => self.step(i32::from(e.delta)),
            },
            TOUCH_DOWN => self.down = Some((i32::from(e.x), i32::from(e.y))),
            TOUCH_UP => {
                let Some((x, y)) = self.down.take() else { return false };
                let (dx, dy) = (i32::from(e.x) - x, i32::from(e.y) - y);
                match self.screen {
                    Screen::Gallery => {
                        if dy.abs() > DRAG {
                            self.scroll_by(-dy);
                        } else if dx.abs() <= DRAG {
                            self.gallery_tap(x, y);
                        }
                    }
                    Screen::Viewer => {
                        if self.zoomed() && (dx.abs() > DRAG || dy.abs() > DRAG) {
                            self.pan(dx, dy);
                        } else if !self.zoomed() && dx.abs() > SWIPE && dx.abs() > dy.abs() {
                            self.step(if dx < 0 { 1 } else { -1 });
                        } else if dx.abs() <= DRAG && dy.abs() <= DRAG {
                            let now = self.api.now();
                            match self.last_tap.take() {
                                Some((px, py, t)) if now.wrapping_sub(t) < 400 && (px - x).abs() < 50 && (py - y).abs() < 50 => {
                                    self.zoom_at(x, y)
                                }
                                _ => self.last_tap = Some((x, y, now)),
                            }
                        }
                    }
                }
            }
            MESSAGE => self.messages(),
            _ => {}
        }
        false
    }
}

fn main(api: &'static Api) -> i32 {
    let mut saved = [1u8; 1];
    let grid = api.load(&mut saved) != 1 || saved[0] != 0;
    let mut ui = Ui {
        api,
        screen: Screen::Gallery,
        grid,
        scroll: 0,
        asked: [false; MAX],
        open: 0,
        view: None,
        base: (0, 0, 1),
        last_tap: None,
        zooming: false,
        dec: Decoder::new(),
        loaded: false,
        inc: Incoming { name: [0; 20], nlen: 0, len: 0, on: false },
        save_list: false,
        down: None,
        motor: Motor::new(api),
    };
    // the button is ours: back from a picture or out of zoom; from the gallery it leaves the app
    api.own_back(true);
    ui.wipe();
    ui.draw_gallery();
    let _ = api.send(b"R");
    let mut ahead: Option<Event> = None;
    loop {
        let e = match ahead.take() {
            Some(e) => Some(e),
            None => api.event(ui.motor.wait().unwrap_or(u32::MAX)),
        };
        ui.motor.update();
        let Some(mut e) = e else { continue };
        if e.kind == CROWN {
            // detents queued while a frame drew: one step
            while let Some(n) = api.event(0) {
                if n.kind == CROWN && (n.delta > 0) == (e.delta > 0) {
                    e.delta = e.delta.saturating_add(n.delta);
                } else {
                    ahead = Some(n);
                    break;
                }
            }
        }
        match e.kind {
            PAUSE => {
                ui.motor.off();
                loop {
                    match api.event(u32::MAX) {
                        Some(e) if e.kind == RESUME => break,
                        Some(e) if e.kind == BUTTON => {
                            ui.wipe();
                            return 0;
                        }
                        _ => {}
                    }
                }
                if ui.screen == Screen::Gallery {
                    ui.draw_gallery();
                }
            }
            _ => {
                if ui.event(e) {
                    return 0;
                }
            }
        }
    }
}

impl Clip {
    fn rect_(&mut self, x: i32, y: i32, w: i32, h: i32, c: u16) {
        rle::Surface::rect(self, x, y, w, h, c);
    }
}
