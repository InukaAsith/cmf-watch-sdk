//! Photos on the watch (nothing kept: its files go when it closes, and at the next start after a swipe closed it):
//! the phone's gallery as a grid of thumbnails or a list of names (tap the heading to switch), scrolled by a drag or
//! the crown. A tap opens a picture: the whole of it, a rounded rectangle, or (double tap, again to go back) filling
//! the round panel. It comes in 8 bands of 16 colours; a tap asks for the sharp version (256 colours), with a
//! progress ring on the rim. The crown or a sideways drag goes to the next or previous picture; the button goes back.
//! Thumbnails are kept in the app's files: asked for once, only when they scroll into view.

use crate::proto::{thumb_name, Gallery, MAX};
use crate::rle::{self, Decoder, Status, HEADER};
use cmf_wp2_app::{Api, Event, BUTTON, CROWN, FILE_CHUNK, MESSAGE, MSG_MAX, PAUSE, RESUME, TOUCH_DOWN, TOUCH_UP};
use cmf_wp2_ui::color::{rgb565, Rgb, BLACK};
use cmf_wp2_ui::draw::{arc, disc, rrect};
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

const HEAD: i32 = 106;
const COLS: i32 = 3;
const CELL: i32 = 104;
const PITCH: i32 = 114;
const GX0: i32 = C - PITCH - CELL / 2;
const ROW: i32 = 56;
/// Moved more than this between down and up: a drag, not a tap.
const DRAG: i32 = 14;
const SWIPE: i32 = 70;
/// Stepping pauses this long before the picture it stopped on is asked for.
const ASK_MS: u32 = 350;
/// A second tap within this: a double tap.
const DOUBLE_MS: u32 = 400;
/// The progress ring along the rim, from the top.
const RING: (i32, i32) = (225, 231);
const TOP: u16 = 49152;

const STORE_CAP: usize = 40 * 1024;
/// `R 1`: ready, and the sharp version may come as `y` chunks in any order (over the phone's sliding window).
const READY: &[u8] = b"R\x01";
/// A `y` chunk's bytes (all but the last).
const Y_CHUNK: usize = 124;
const Y_SEQS: usize = STORE_CAP.div_ceil(Y_CHUNK);
const THUMB_CAP: usize = 12 * 1024;

/// The open picture, kept to draw it again: up to BANDS horizontal bands, each a picture of its own (`I` header, its
/// own 16 colours, tokens), the first at the picture's origin and each next one `rows` picture rows below it.
struct Store {
    headers: [[u8; HEADER]; BANDS],
    rows: [i32; BANDS],
    /// tokens of band k: data[start[k]..start[k + 1]], the last ending at len
    start: [usize; BANDS + 1],
    n: usize,
    /// bands announced by `P n` (0: a single `I` picture, as before)
    expect: usize,
    data: [u8; STORE_CAP],
    len: usize,
    ok: bool,
    /// the sharp version (256 colours) replaced the bands: `hd` geometry and palette, its tokens in data
    hd: Option<Hd>,
    /// the sharp version on its way: (bytes so far, of total, next seq)
    loading: Option<(usize, usize, u8)>,
    /// its `y` chunks that came
    got: [u8; Y_SEQS.div_ceil(8)],
    pal: [u16; 256],
}

/// The 256-colour picture: w × h at scale, top left at (x0, y0). Its tokens: a byte < 255 is one pixel of that
/// colour; 255, c, n is n pixels of colour c; no run crosses a row.
#[derive(Clone, Copy)]
struct Hd {
    w: i32,
    h: i32,
    scale: u8,
    x0: i32,
    y0: i32,
}
const BANDS: usize = 8;
static mut STORE: Store =
    Store { headers: [[0; HEADER]; BANDS], rows: [0; BANDS], start: [0; BANDS + 1], n: 0, expect: 0, data: [0; STORE_CAP], len: 0, ok: false, hd: None, loading: None, got: [0; Y_SEQS.div_ceil(8)], pal: [0; 256] };

impl Store {
    fn reset(&mut self, expect: usize) {
        (self.n, self.len, self.expect, self.ok) = (0, 0, expect, false);
        self.hd = None;
        self.loading = None;
    }

    /// The picture's origin and scale as sent (its first band's header).
    fn origin(&self) -> (i32, i32, u8) {
        if let Some(h) = self.hd {
            return (h.x0, h.y0, h.scale);
        }
        let h = &self.headers[0];
        let u = |i: usize| i16::from_le_bytes([h[i], h[i + 1]]) as i32;
        (u(6), u(8), h[5].max(1))
    }
}
static mut THUMB: [u8; THUMB_CAP] = [0; THUMB_CAP];
static mut CHUNK: [u8; FILE_CHUNK] = [0; FILE_CHUNK];
static mut GALLERY: Gallery = Gallery::new();

/// Draws the 256-colour picture's tokens at (x0, y0), `scale` px a pixel.
fn draw_hd(st: &Store, hd: Hd, x0: i32, y0: i32, scale: i32, s: &mut impl rle::Surface) {
    let (mut x, mut y) = (0, 0);
    let t = &st.data[..st.len];
    let mut i = 0;
    while i < t.len() && y < hd.h {
        let (c, n) = if t[i] == 255 && i + 2 < t.len() {
            i += 3;
            (t[i - 2], i32::from(t[i - 1]))
        } else {
            i += 1;
            (t[i - 1], 1)
        };
        let n = n.min(hd.w - x);
        s.rect(x0 + x * scale, y0 + y * scale, n * scale, scale, st.pal[usize::from(c)]);
        x += n;
        if x >= hd.w {
            x = 0;
            y += 1;
        }
    }
}

fn store() -> &'static mut Store {
    unsafe { &mut *core::ptr::addr_of_mut!(STORE) }
}
fn gallery() -> &'static mut Gallery {
    unsafe { &mut *core::ptr::addr_of_mut!(GALLERY) }
}

/// The panel, rows above `top` left alone (the heading stays over scrolled thumbnails).
struct Clip(&'static Api, i32);

impl rle::Surface for Clip {
    fn rect(&mut self, x: i32, y: i32, w: i32, h: i32, c: u16) {
        // clipped to the panel here
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
    /// the heading as it should be is on the panel: list updates and thumbnails leave it alone (clearing and
    /// drawing it again for each flickered while the photos loaded)
    head_up: bool,
    grid: bool,
    scroll: i32,
    asked: [bool; MAX],
    open: usize,
    /// the opened picture as drawn now
    view: Option<(i32, i32, u8)>,
    /// where the picture sits (as it came)
    base: (i32, i32, u8),
    /// the last single tap in the viewer, for a double tap
    last_tap: Option<(i32, i32, u32)>,
    /// round: the picture fills the circle (cropped); else the whole picture, a rounded rectangle (double tap)
    round: bool,
    /// a single tap waits this long for a second one (double tap) before it asks for the sharp version
    tap_due: Option<u32>,
    /// how much of the progress ring is drawn (u32::MAX: not yet its track)
    ring_drawn: u32,
    /// a picture shown by name while the crown or a swipe steps on: asked for once stepping pauses (this time)
    ask_at: Option<u32>,
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
        let head = !self.head_up;
        self.head_up = true;
        self.draw_gallery_(head);
    }

    fn draw_head(&self) {
        let mut s = self.api;
        Surface::rect(&mut s, 0, 0, 466, HEAD, 0);
        dots_centre(&mut s, C - 8, 34, "PHOTOS", 3, 2, W);
        disc(&mut s, C + 66, 34 + 18, 4, RED, BLACK);
        // the view switch: the open one in white
        let (gw, lw) = (font::width("Grid"), font::width("List"));
        let x = C - (gw + 24 + lw) / 2;
        font::draw(&mut s, x, 68, "Grid", if self.grid { W } else { GREY });
        disc(&mut s, x + gw + 12, 68 + 11, 2, Rgb(0x55, 0x55, 0x55), BLACK);
        font::draw(&mut s, x + gw + 24, 68, "List", if self.grid { GREY } else { W });
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
            self.head_up = false;
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
        self.show_name(i);
        self.ask_at = None;
        self.ask_open();
    }

    /// The viewer with the picture's name while it comes; nothing asked yet.
    fn show_name(&mut self, i: usize) {
        self.open = i;
        self.screen = Screen::Viewer;
        self.head_up = false;
        self.view = None;
        store().reset(0);
        let mut s = self.api;
        Surface::rect(&mut s, 0, 0, 466, 466, 0);
        font::centre(&mut s, C, 222, gallery().items[i].name(), GREY);
        self.api.show();
    }

    /// Stepping through: only the picture it stops on is asked for (ASK_MS after the last step).
    fn ask_due(&mut self) {
        if let Some(t) = self.ask_at {
            if self.api.now().wrapping_sub(t) as i32 >= 0 {
                self.ask_at = None;
                if self.screen == Screen::Viewer {
                    self.ask_open();
                }
            }
        }
    }

    /// `o idx mode` (mode 1: round, 0: the whole picture).
    fn ask_open(&mut self) {
        self.tap_due = None;
        let _ = self.api.send(&[b'o', self.open as u8, self.round as u8]);
    }

    /// A single tap that no second one followed: the sharp version of what is shown.
    fn tap_check(&mut self) {
        if let Some(t) = self.tap_due {
            if self.api.now().wrapping_sub(t) as i32 >= 0 {
                self.tap_due = None;
                let st = store();
                if self.screen == Screen::Viewer && st.ok && st.hd.is_none() && st.loading.is_none() {
                    self.motor.play(haptic::TICK);
                    let _ = self.api.send(b"h");
                    // the ring's track at once: the tap was taken (the phone prepares the picture first)
                    let mut a = self.api;
                    arc(&mut a, C, C, RING.0, RING.1, TOP, 65536, Rgb(0x30, 0x30, 0x30), None, BLACK);
                    self.api.show();
                }
            }
        }
    }

    /// ms to the next due ask, if any.
    fn ask_wait(&self) -> Option<u32> {
        let left = |t: u32| (t.wrapping_sub(self.api.now()) as i32).max(0) as u32;
        match (self.ask_at.map(left), self.tap_due.map(left)) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        }
    }

    fn redraw(&self, x0: i32, y0: i32, scale: u8) -> bool {
        let st = store();
        if !st.ok {
            return false;
        }
        let mut s = self.api;
        if let Some(hd) = st.hd {
            // black only round the picture: the sharp one is drawn straight over the soft one, no black frame
            let sc = i32::from(scale);
            let (x1, y1) = (x0 + hd.w * sc, y0 + hd.h * sc);
            Surface::rect(&mut s, 0, 0, 466, y0, 0);
            Surface::rect(&mut s, 0, y1, 466, 466 - y1, 0);
            Surface::rect(&mut s, 0, y0, x0, y1 - y0, 0);
            Surface::rect(&mut s, x1, y0, 466 - x1, y1 - y0, 0);
            draw_hd(st, hd, x0, y0, sc, &mut Clip(self.api, 0));
            self.api.show();
            return true;
        }
        Surface::rect(&mut s, 0, 0, 466, 466, 0);
        for k in 0..st.n {
            let mut d = Decoder::new();
            if !d.start(&st.headers[k], x0, y0 + st.rows[k] * i32::from(scale), scale) {
                continue;
            }
            let t = &st.data[st.start[k]..st.start[k + 1]];
            d.feed(t, &mut Clip(self.api, 0));
        }
        self.api.show();
        st.n > 0
    }

    /// The sharp version's progress: a ring along the rim, filling clockwise from the top.
    fn hd_ring(&mut self) {
        let Some((got, total, _)) = store().loading else { return };
        let want = (got.min(total) as u64 * 65536 / total.max(1) as u64) as u32;
        let mut s = self.api;
        if self.ring_drawn == u32::MAX {
            arc(&mut s, C, C, RING.0, RING.1, TOP, 65536, Rgb(0x30, 0x30, 0x30), None, BLACK);
            self.ring_drawn = 0;
        }
        if want > self.ring_drawn {
            arc(&mut s, C, C, RING.0, RING.1, TOP.wrapping_add(self.ring_drawn as u16), want - self.ring_drawn, Rgb(0xFF, 0xFF, 0xFF), None, BLACK);
            self.ring_drawn = want;
        }
    }

    fn step(&mut self, d: i32) {
        let n = gallery().n;
        if n > 1 {
            self.motor.play(haptic::TICK);
            let next = if d > 0 { (self.open + 1) % n } else { (self.open + n - 1) % n };
            self.show_name(next);
            self.ask_at = Some(self.api.now().wrapping_add(ASK_MS));
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
                            // clipped below the heading: the heading stays as it is
                            self.draw_item(i);
                            drew = true;
                        }
                    }
                }
                Some(b'?') => {
                    let _ = self.api.send(READY);
                }
                // the sharp version (256 colours) after the bands: `S total:u16 w:u16 h:u16 scale x0:i16 y0:i16`,
                // `K first count rgb565…`, `Y seq tokens…`, `Z`
                Some(b'S') if self.screen == Screen::Viewer && m.len() == 12 && store().ok => {
                    let u = |i: usize| i32::from(i16::from_le_bytes([m[i], m[i + 1]]));
                    let st = store();
                    let total = u16::from_le_bytes([m[1], m[2]]) as usize;
                    if total <= STORE_CAP {
                        st.hd = Some(Hd { w: u(3), h: u(5), scale: m[7].max(1), x0: u(8), y0: u(10) });
                        st.loading = Some((0, total, 0));
                        st.got = [0; Y_SEQS.div_ceil(8)];
                        self.ring_drawn = u32::MAX;
                        st.ok = false;
                        st.len = 0;
                        self.hd_ring();
                        drew = true;
                    }
                }
                Some(b'K') if m.len() >= 3 && store().loading.is_some() => {
                    let st = store();
                    let (first, count) = (usize::from(m[1]), usize::from(m[2]));
                    for k in 0..count.min((m.len() - 3) / 2) {
                        if first + k < 256 {
                            st.pal[first + k] = u16::from_le_bytes([m[3 + 2 * k], m[4 + 2 * k]]);
                        }
                    }
                }
                Some(b'Y') if m.len() >= 2 && store().loading.is_some() => {
                    let st = store();
                    let Some((got, total, seq)) = st.loading else { continue };
                    if m[1] != seq {
                        // a chunk lost: the phone sends the sharp version again
                        st.loading = None;
                        let _ = self.api.send(&[b'G', seq]);
                        continue;
                    }
                    let t = &m[2..];
                    if st.len + t.len() <= STORE_CAP {
                        st.data[st.len..st.len + t.len()].copy_from_slice(t);
                        st.len += t.len();
                    }
                    st.loading = Some((got + t.len(), total, seq.wrapping_add(1)));
                    self.hd_ring();
                    drew = true;
                }
                // `y seq:u16 tokens…`: the sharp version's bytes at seq × 124, in any order, maybe twice; it is
                // whole when every chunk came
                Some(b'y') if m.len() > 3 && store().loading.is_some() => {
                    let st = store();
                    let Some((got, total, _)) = st.loading else { continue };
                    let seq = usize::from(u16::from_le_bytes([m[1], m[2]]));
                    let at = seq * Y_CHUNK;
                    if seq >= Y_SEQS || at >= total || st.got[seq / 8] & (1 << (seq % 8)) != 0 {
                        continue;
                    }
                    let n = (m.len() - 3).min(total - at);
                    st.data[at..at + n].copy_from_slice(&m[3..3 + n]);
                    st.got[seq / 8] |= 1 << (seq % 8);
                    st.len = st.len.max(at + n);
                    st.loading = Some((got + n, total, 0));
                    if got + n >= total {
                        st.len = total;
                        self.sharp_done();
                    } else {
                        self.hd_ring();
                    }
                    drew = true;
                }
                Some(b'Z') if store().loading.is_some() => {
                    self.sharp_done();
                }
                Some(b'P') if self.screen == Screen::Viewer && m.len() == 2 => {
                    // a picture in m[1] bands follows (each `I`/`D`/`E`)
                    store().reset(usize::from(m[1]).clamp(1, BANDS));
                }
                Some(b'I' | b'D' | b'E') if self.screen == Screen::Viewer => {
                    let st = store();
                    match m[0] {
                        b'I' if m.len() == HEADER => {
                            if st.expect == 0 || st.n >= BANDS {
                                st.reset(0);
                            }
                            let k = st.n;
                            st.headers[k].copy_from_slice(m);
                            st.rows[k] = if k == 0 {
                                0
                            } else {
                                let (_, y0, sc) = st.origin();
                                (i32::from(i16::from_le_bytes([m[8], m[9]])) - y0) / i32::from(sc)
                            };
                            st.start[k] = st.len.min(STORE_CAP);
                            st.n += 1;
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
                    let first = st.n <= 1;
                    match self.dec.message(m, &mut Clip(self.api, 0)) {
                        Status::Begin if first => {
                            let mut s = self.api;
                            Surface::rect(&mut s, 0, 0, 466, 466, 0);
                        }
                        Status::Done(_) => {
                            st.start[st.n] = st.len.min(STORE_CAP);
                            if st.n >= st.expect.max(1) {
                                st.ok = st.len <= STORE_CAP;
                                self.view = Some(st.origin());
                                self.base = st.origin();
                            }
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

    /// The sharp version came: it replaces the bands.
    fn sharp_done(&mut self) {
        let st = store();
        st.loading = None;
        st.ok = true;
        self.base = st.origin();
        self.view = Some(self.base);
        let (x, y, sc) = self.base;
        self.redraw(x, y, sc);
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
                if self.screen == Screen::Viewer {
                    self.screen = Screen::Gallery;
                    self.ask_at = None;
                    // the phone goes back to the thumbnails it paused
                    let _ = self.api.send(b"g");
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
                // the crown goes through the pictures
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
                        if dx.abs() > SWIPE && dx.abs() > dy.abs() {
                            self.step(if dx < 0 { 1 } else { -1 });
                        } else if dx.abs() <= DRAG && dy.abs() <= DRAG {
                            let now = self.api.now();
                            match self.last_tap.take() {
                                // double tap: round ⇄ the whole picture
                                Some((px, py, t)) if now.wrapping_sub(t) < DOUBLE_MS && (px - x).abs() < 50 && (py - y).abs() < 50 => {
                                    self.tap_due = None;
                                    self.motor.play(haptic::TICK);
                                    self.round = !self.round;
                                    self.open_picture(self.open);
                                }
                                _ => {
                                    self.last_tap = Some((x, y, now));
                                    self.tap_due = Some(now.wrapping_add(DOUBLE_MS));
                                }
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
        head_up: false,
        grid,
        scroll: 0,
        asked: [false; MAX],
        open: 0,
        view: None,
        base: (0, 0, 1),
        last_tap: None,
        round: false,
        tap_due: None,
        ring_drawn: u32::MAX,
        ask_at: None,
        dec: Decoder::new(),
        loaded: false,
        inc: Incoming { name: [0; 20], nlen: 0, len: 0, on: false },
        save_list: false,
        down: None,
        motor: Motor::new(api),
    };
    // the button is ours: back from a picture; from the gallery it leaves the app
    api.own_back(true);
    ui.wipe();
    ui.draw_gallery();
    let _ = api.send(READY);
    let mut ask = cmf_wp2_ui::ask::Ask::new(api.now(), 10);
    let mut ahead: Option<Event> = None;
    loop {
        let e = match ahead.take() {
            Some(e) => Some(e),
            None => {
                let w = [ui.motor.wait(), ui.ask_wait(), ask.wait(api.now())].into_iter().flatten().min().unwrap_or(u32::MAX);
                api.event(w)
            }
        };
        if ask.due(api.now()) {
            let _ = api.send(READY);
        }
        if e.as_ref().is_some_and(|e| e.kind == MESSAGE) {
            ask.answered();
        }
        ui.motor.update();
        ui.ask_due();
        ui.tap_check();
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
