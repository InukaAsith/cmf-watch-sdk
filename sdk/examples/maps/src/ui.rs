//! Offline maps: tiles from the app's own files on the watch (pack.rs has the format), drawn with pixview's run
//! decoder. Crown zooms between the pack's levels, a drag pans, a tap recentres, a long press goes back to the
//! phone's position (or the pack's centre). Nothing comes over the air while browsing.

use crate::pack::{tile_name, Pack, TILE, WORLD_Z};
use crate::rle::{self, Decoder};
use cmf_wp2_app::{Api, Event, BUTTON, CROWN, MESSAGE, MSG_MAX, PAUSE, RESUME, TOUCH_DOWN, TOUCH_UP};
use cmf_wp2_app::FILE_CHUNK;
use cmf_wp2_ui::color::{rgb565, Rgb, BLACK};
use cmf_wp2_ui::draw::{disc, rrect};
use cmf_wp2_ui::font;
use cmf_wp2_ui::text::{dots_centre, int};
use cmf_wp2_ui::{C, Surface};

cmf_wp2_app::entry!(main);
cmf_wp2_app::app_icon!();

const W: u16 = 0xFFFF;
const RED: Rgb = Rgb(0xD7, 0x19, 0x21);
const GREY: u16 = rgb565(0x8A, 0x8A, 0x8A);
const DIM: u16 = rgb565(0x3A, 0x3A, 0x3A);
const EMPTY: u16 = rgb565(0x0E, 0x0E, 0x0E);
const PILL: Rgb = Rgb(0x10, 0x10, 0x10);

/// Biggest tile file (scale byte + tokens); the pack builder keeps every tile under it.
const TILE_CAP: usize = 32 * 1024;
static mut TILE_BUF: [u8; TILE_CAP] = [0; TILE_CAP];
static mut IDX: [u8; 256] = [0; 256];
static mut PACK: Pack = Pack::empty();
static mut CHUNK: [u8; FILE_CHUNK] = [0; FILE_CHUNK];

/// Moved more than this between down and up: a drag, not a tap.
const DRAG: i32 = 14;
const HOLD_MS: u32 = 600;
/// Held this long: "Delete map?".
const DELETE_MS: u32 = 2000;

struct Screen(&'static Api);
impl rle::Surface for Screen {
    fn rect(&mut self, x: i32, y: i32, w: i32, h: i32, c: u16) {
        let mut a = self.0;
        Surface::rect(&mut a, x, y, w, h, c);
    }
}

/// A file arriving from the phone, written to NAND a chunk at a time.
struct Incoming {
    name: [u8; 20],
    nlen: usize,
    len: usize,
    on: bool,
}

struct Ui {
    api: &'static Api,
    have: bool,
    level: usize,
    /// past the most detailed level: its tiles drawn 2× or 4× (labels big enough to read)
    mag: u32,
    /// the how-to line until the first touch
    hint: bool,
    /// "Delete map?" is open (a 2 s hold)
    confirm: bool,
    /// view centre, z20 world pixels
    cx: u32,
    cy: u32,
    pos: Option<(u32, u32)>,
    follow: bool,
    down: Option<(i32, i32, u32)>,
    loading: Option<(u16, u16)>,
    inc: Incoming,
}

fn pack() -> &'static Pack {
    unsafe { &*core::ptr::addr_of!(PACK) }
}

impl Ui {
    fn shift(&self) -> u32 {
        u32::from(WORLD_Z - pack().levels[self.level].z)
    }

    fn load_pack(&mut self) {
        let idx = unsafe { &mut *core::ptr::addr_of_mut!(IDX) };
        let n = self.api.file_read("idx", 0, idx);
        self.have = false;
        if n > 0 {
            if let Some(p) = Pack::parse(&idx[..n as usize]) {
                unsafe { PACK = p };
                self.have = true;
                // the middle level: a street-level view to start
                self.level = self.level.min(pack().n - 1).max(pack().n / 2);
                (self.cx, self.cy) = self.pos.unwrap_or(pack().centre);
            }
        }
    }

    /// Deletes the saved map: every tile the index names, then the index (no listing on this filesystem).
    fn delete_pack(&mut self) {
        if self.have {
            let p = pack();
            let mut name = [0u8; 20];
            for li in 0..p.n {
                let l = p.levels[li];
                for r in 0..u32::from(l.h) {
                    for c in 0..u32::from(l.w) {
                        let n = tile_name(li, c, r, &mut name);
                        let _ = self.api.file_delete(core::str::from_utf8(&name[..n]).unwrap_or(""));
                    }
                }
            }
        }
        let _ = self.api.file_delete("idx");
        self.have = false;
        self.mag = 1;
    }

    /// Keeps the centre inside the current level's tiles.
    fn clamp(&mut self) {
        let l = pack().levels[self.level];
        let sh = self.shift();
        let x0 = (l.x0 * TILE as u32) << sh;
        let y0 = (l.y0 * TILE as u32) << sh;
        let x1 = x0 + ((u32::from(l.w) * TILE as u32) << sh);
        let y1 = y0 + ((u32::from(l.h) * TILE as u32) << sh);
        self.cx = self.cx.clamp(x0, x1.saturating_sub(1));
        self.cy = self.cy.clamp(y0, y1.saturating_sub(1));
    }

    fn draw(&mut self) {
        if self.confirm {
            return self.draw_confirm();
        }
        if let Some((done, total)) = self.loading {
            return self.draw_loading(done, total);
        }
        if !self.have {
            return self.draw_empty();
        }
        let api = self.api;
        let p = pack();
        let l = p.levels[self.level];
        let sh = self.shift();
        let mag = i64::from(self.mag);
        let (vx, vy) = (i64::from(self.cx >> sh) * mag, i64::from(self.cy >> sh) * mag);
        let (left, top) = (vx - i64::from(C), vy - i64::from(C));
        let t = i64::from(TILE) * mag;
        let buf = unsafe { &mut *core::ptr::addr_of_mut!(TILE_BUF) };
        let mut name = [0u8; 20];
        let mut screen = Screen(api);
        for tr in top.div_euclid(t)..=(top + 2 * i64::from(C)).div_euclid(t) {
            for tc in left.div_euclid(t)..=(left + 2 * i64::from(C)).div_euclid(t) {
                let (sx, sy) = ((tc * t - left) as i32, (tr * t - top) as i32);
                let (c, r) = (tc - i64::from(l.x0), tr - i64::from(l.y0));
                let mut drawn = false;
                if (0..i64::from(l.w)).contains(&c) && (0..i64::from(l.h)).contains(&r) {
                    let n = tile_name(self.level, c as u32, r as u32, &mut name);
                    let name = core::str::from_utf8(&name[..n]).unwrap_or("");
                    let got = api.file_read(name, 0, buf);
                    if got > 1 {
                        let mut d = Decoder::new();
                        if d.start(&p.header(buf[0]), sx, sy, buf[0] * self.mag as u8) {
                            d.feed(&buf[1..got as usize], &mut screen);
                            drawn = true;
                        }
                    }
                }
                if !drawn {
                    for ey in 0..self.mag as i32 {
                        for ex in 0..self.mag as i32 {
                            self.empty_tile(sx + ex * TILE, sy + ey * TILE);
                        }
                    }
                }
            }
        }
        self.overlay(left, top);
        api.show();
    }

    /// Outside the pack: dark with a dot grid, so a pan still shows movement.
    fn empty_tile(&self, x: i32, y: i32) {
        let mut s = self.api;
        Surface::rect(&mut s, x, y, TILE, TILE, EMPTY);
        for gy in (16..TILE).step_by(32) {
            for gx in (16..TILE).step_by(32) {
                Surface::rect(&mut s, x + gx, y + gy, 2, 2, DIM);
            }
        }
    }

    fn overlay(&self, left: i64, top: i64) {
        let mut s = self.api;
        let sh = self.shift();
        // you
        if let Some((px, py)) = self.pos {
            let mag = i64::from(self.mag);
            let (x, y) = ((i64::from(px >> sh) * mag - left) as i32, (i64::from(py >> sh) * mag - top) as i32);
            let (dx, dy) = (x - C, y - C);
            if dx * dx + dy * dy < 205 * 205 {
                disc(&mut s, x, y, 12, Rgb(0xFF, 0xFF, 0xFF), BLACK);
                disc(&mut s, x, y, 8, RED, Rgb(0xFF, 0xFF, 0xFF));
            } else {
                // off screen: a red dot on the rim, towards you
                let d = isqrt(dx * dx + dy * dy).max(1);
                disc(&mut s, C + dx * 212 / d, C + dy * 212 / d, 7, RED, BLACK);
            }
        } else if !self.follow {
            // browsing without a position: a small cross marks the centre
            Surface::rect(&mut s, C - 8, C - 1, 16, 2, W);
            Surface::rect(&mut s, C - 1, C - 8, 2, 16, W);
        }
        // zoom ladder on the right rim, zoomed in at the top: the pack's levels, then the two magnified steps
        let n = pack().n as i32 + 2;
        let at = self.level as i32 + if self.mag == 4 { 2 } else if self.mag == 2 { 1 } else { 0 };
        let y0 = C - (n - 1) * 9;
        rrect(&mut s, 426, y0 - 12, 20, (n - 1) * 18 + 24, 10, PILL, BLACK);
        for i in 0..n {
            let y = y0 + (n - 1 - i) * 18;
            if i == at {
                disc(&mut s, 436, y, 5, Rgb(0xFF, 0xFF, 0xFF), PILL);
            } else {
                Surface::rect(&mut s, 435, y - 1, 3, 3, if i >= pack().n as i32 { DIM } else { GREY });
            }
        }
        if self.hint {
            rrect(&mut s, C - 118, 40, 236, 34, 17, PILL, BLACK);
            font::centre(&mut s, C, 46, "tap: move  hold: you", W);
        }
        self.scale_bar();
    }

    fn scale_bar(&self) {
        let mm = (pack().levels[self.level].mm_per_px / self.mag).max(1);
        // the longest round distance that fits 130 px
        let mut best = 0;
        for m in [5u32, 10, 20, 50, 100, 200, 500, 1000, 2000, 5000, 10000, 20000, 50000] {
            if m * 1000 / mm <= 130 {
                best = m;
            }
        }
        if best == 0 {
            return;
        }
        let len = (best * 1000 / mm) as i32;
        let mut b = [0u8; 11];
        let (num, unit) = if best >= 1000 { (int((best / 1000) as i32, &mut b), " km") } else { (int(best as i32, &mut b), " m") };
        let mut label = [0u8; 16];
        let k = num.len();
        label[..k].copy_from_slice(num.as_bytes());
        label[k..k + unit.len()].copy_from_slice(unit.as_bytes());
        let label = core::str::from_utf8(&label[..k + unit.len()]).unwrap_or("");
        let mut s = self.api;
        let w = len.max(font::width(label)) + 28;
        rrect(&mut s, C - w / 2, 386, w, 46, 14, PILL, BLACK);
        let x = C - len / 2;
        Surface::rect(&mut s, x, 422, len, 3, W);
        Surface::rect(&mut s, x, 416, 2, 9, W);
        Surface::rect(&mut s, x + len - 2, 416, 2, 9, W);
        font::centre(&mut s, C, 390, label, W);
    }

    fn draw_confirm(&self) {
        let mut s = self.api;
        Surface::rect(&mut s, 0, 0, 466, 466, 0);
        font::centre(&mut s, C, 150, "Delete this map?", W);
        font::centre(&mut s, C, 182, "It is saved on the watch", GREY);
        rrect(&mut s, C - 150, 240, 140, 64, 32, RED, BLACK);
        font::centre(&mut s, C - 80, 260, "Delete", W);
        rrect(&mut s, C + 10, 240, 140, 64, 32, PILL, BLACK);
        font::centre(&mut s, C + 80, 260, "Keep", W);
        self.api.show();
    }

    fn draw_empty(&self) {
        let mut s = self.api;
        Surface::rect(&mut s, 0, 0, 466, 466, 0);
        heading(&mut s, 150);
        font::centre(&mut s, C, 218, "No offline map", W);
        font::centre(&mut s, C, 252, "Pick an area in", GREY);
        font::centre(&mut s, C, 276, "CMFX on the iPhone", GREY);
        self.api.show();
    }

    fn draw_loading(&self, done: u16, total: u16) {
        let mut s = self.api;
        Surface::rect(&mut s, 0, 0, 466, 466, 0);
        heading(&mut s, 150);
        font::centre(&mut s, C, 222, "Saving map", W);
        let total = i32::from(total.max(1));
        let w = 260 * i32::from(done).min(total) / total;
        rrect(&mut s, C - 132, 262, 264, 12, 6, Rgb(0x26, 0x26, 0x26), BLACK);
        if w > 4 {
            rrect(&mut s, C - 130, 264, w, 8, 4, RED, Rgb(0x26, 0x26, 0x26));
        }
        let (mut a, mut b) = ([0u8; 11], [0u8; 11]);
        let (a, b) = (int(i32::from(done), &mut a), int(total, &mut b));
        let mut line = [0u8; 32];
        let mut n = 0;
        for part in [a, " / ", b, " tiles"] {
            line[n..n + part.len()].copy_from_slice(part.as_bytes());
            n += part.len();
        }
        font::centre(&mut s, C, 288, core::str::from_utf8(&line[..n]).unwrap_or(""), GREY);
        self.api.show();
    }

    fn zoom(&mut self, up: bool) -> bool {
        let top = pack().n - 1;
        if up {
            if self.level < top {
                self.level += 1;
            } else if self.mag < 4 {
                self.mag *= 2;
            } else {
                return false;
            }
        } else if self.mag > 1 {
            self.mag /= 2;
        } else if self.level > 0 {
            self.level -= 1;
        } else {
            return false;
        }
        true
    }

    fn home(&mut self) {
        self.follow = true;
        (self.cx, self.cy) = self.pos.unwrap_or(pack().centre);
    }

    /// One event; true when the map needs drawing again.
    fn handle(&mut self, e: Event) -> bool {
        if self.loading.is_some() && e.kind != MESSAGE {
            return false;
        }
        match e.kind {
            CROWN if self.have => self.zoom(e.delta > 0),
            TOUCH_DOWN => {
                self.down = Some((i32::from(e.x), i32::from(e.y), self.api.now()));
                false
            }
            TOUCH_UP if self.confirm => {
                self.down = None;
                let (x, y) = (i32::from(e.x), i32::from(e.y));
                if (230..320).contains(&y) && x < C {
                    self.delete_pack();
                }
                self.confirm = false;
                true
            }
            TOUCH_UP if self.have => {
                let Some((x, y, t)) = self.down.take() else { return false };
                if self.api.now().wrapping_sub(t) >= DELETE_MS && (i32::from(e.x) - x).abs() + (i32::from(e.y) - y).abs() <= DRAG {
                    self.confirm = true;
                    return true;
                }
                let (dx, dy) = (i32::from(e.x) - x, i32::from(e.y) - y);
                let sh = self.shift();
                let mag = i64::from(self.mag);
                self.hint = false;
                if dx.abs() + dy.abs() > DRAG {
                    // the map follows the finger
                    self.cx = (i64::from(self.cx) - (i64::from(dx) << sh) / mag).max(0) as u32;
                    self.cy = (i64::from(self.cy) - (i64::from(dy) << sh) / mag).max(0) as u32;
                    self.follow = false;
                } else if self.api.now().wrapping_sub(t) >= HOLD_MS {
                    self.home();
                } else {
                    self.cx = (i64::from(self.cx) + (i64::from(x - C) << sh) / mag).max(0) as u32;
                    self.cy = (i64::from(self.cy) + (i64::from(y - C) << sh) / mag).max(0) as u32;
                    self.follow = false;
                }
                self.clamp();
                true
            }
            MESSAGE => self.messages(),
            _ => false,
        }
    }

    fn messages(&mut self) -> bool {
        let mut buf = [0u8; MSG_MAX];
        let mut dirty = false;
        while let Some(m) = self.api.recv(&mut buf) {
            let u = |i: usize| u32::from_le_bytes([m[i], m[i + 1], m[i + 2], m[i + 3]]);
            match m.first() {
                Some(b'W') => {
                    self.flush();
                    self.inc.on = false;
                    self.delete_pack();
                    let _ = self.api.send(b"w");
                    dirty = true;
                }
                Some(b'P') if m.len() == 5 && self.loading.is_none() => {
                    // a new map: the old one goes first (its tiles may not all be replaced)
                    self.delete_pack();
                    self.loading = Some((u16::from_le_bytes([m[1], m[2]]), u16::from_le_bytes([m[3], m[4]])));
                    dirty = true;
                }
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
                }
                Some(b'P') if m.len() == 5 => {
                    self.loading = Some((u16::from_le_bytes([m[1], m[2]]), u16::from_le_bytes([m[3], m[4]])));
                    dirty = true;
                }
                Some(b'Z') => {
                    self.loading = None;
                    self.load_pack();
                    let _ = self.api.send(b"k");
                    dirty = true;
                }
                Some(b'X') if m.len() == 9 => {
                    self.pos = Some((u(1), u(5)));
                    if self.follow && self.have {
                        (self.cx, self.cy) = (u(1), u(5));
                        self.clamp();
                    }
                    dirty = true;
                }
                Some(b'?') => {
                    let _ = self.api.send(b"R");
                }
                _ => {}
            }
        }
        dirty
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
}

fn heading(s: &mut impl Surface, y: i32) {
    dots_centre(s, C - 8, y, "MAPS", 4, 3, W);
    disc(s, C + 66, y + 24, 5, RED, BLACK);
}

fn isqrt(v: i32) -> i32 {
    let mut r = 0;
    while (r + 1) * (r + 1) <= v {
        r += 1;
    }
    r
}

fn main(api: &'static Api) -> i32 {
    let mut ui = Ui {
        api,
        have: false,
        level: 0,
        mag: 1,
        hint: true,
        confirm: false,
        cx: 0,
        cy: 0,
        pos: None,
        follow: true,
        down: None,
        loading: None,
        inc: Incoming { name: [0; 20], nlen: 0, len: 0, on: false },
    };
    ui.load_pack();
    ui.draw();
    let _ = api.send(b"R");
    let mut ask = cmf_wp2_ui::ask::Ask::new(api.now(), 10);
    loop {
        let e = api.event(ask.wait(api.now()).unwrap_or(u32::MAX));
        if ask.due(api.now()) {
            let _ = api.send(b"R");
        }
        let Some(e) = e else { continue };
        if e.kind == MESSAGE {
            ask.answered();
        }
        let mut dirty = match e.kind {
            BUTTON => return 0,
            PAUSE => loop {
                match api.event(u32::MAX) {
                    Some(e) if e.kind == RESUME => break true,
                    Some(e) if e.kind == BUTTON => return 0,
                    _ => {}
                }
            },
            _ => ui.handle(e),
        };
        // a fast crown turn or a burst of messages: one redraw for all of it
        while let Some(e) = api.event(0) {
            if e.kind == BUTTON {
                return 0;
            }
            dirty |= ui.handle(e);
        }
        if dirty {
            ui.draw();
        }
    }
}
