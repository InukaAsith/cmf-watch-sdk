//! Turn by turn in the Nothing look. Turn view (default): the next manoeuvre as a big arrow, the distance in dots,
//! the street, arrival time. Map view (tap to switch): the route heading-up around a red dot, drawn here from the
//! route points, so it turns and zooms (crown) at once. Buzz at 200 m and at the turn. Button: exit.

use crate::state::{Effect, Nav};
use cmf_wp2_app::{Api, BUTTON, CROWN, MESSAGE, MSG_MAX, PAUSE, RESUME, TOUCH_DOWN};
use cmf_wp2_ui::color::{rgb565, BLACK, Rgb};
use cmf_wp2_ui::draw::{disc, line, ring};
use cmf_wp2_ui::geom::chord;
use cmf_wp2_ui::haptic::{self, Motor};
use cmf_wp2_ui::motion::{cos, sin};
use cmf_wp2_ui::text::{centre, clip, dots, dots_centre, int};
use cmf_wp2_ui::{C, CHAR, Surface};

cmf_wp2_app::entry!(main);
cmf_wp2_app::app_icon!();

static mut NAV: Nav = Nav::new();

fn nav() -> &'static mut Nav {
    unsafe { &mut *core::ptr::addr_of_mut!(NAV) }
}

const WHITE: Rgb = Rgb(0xFF, 0xFF, 0xFF);
const RED: Rgb = Rgb(0xD7, 0x19, 0x21);
const ROUTE: Rgb = Rgb(0xE8, 0xE8, 0xE8);
const W: u16 = 0xFFFF;
const SOFT: u16 = rgb565(0xC8, 0xC8, 0xC8);
const GREY: u16 = rgb565(0x80, 0x80, 0x80);
const DIM: u16 = rgb565(0x3A, 0x3A, 0x3A);
/// Where we are on the map view.
const ME_Y: i32 = 300;
const ZOOMS: [i32; 5] = [96, 160, 256, 416, 640]; // px per metre × 256

struct Ui {
    api: &'static Api,
    map: bool,
    zoom: usize,
    motor: Motor,
}

fn main(api: &'static Api) -> i32 {
    let mut ui = Ui { api, map: false, zoom: 2, motor: Motor::new(api) };
    api.keep_screen_on(true);
    ui.draw();
    let _ = api.send(b"R");
    let mut msg = [0u8; MSG_MAX];
    loop {
        let e = api.event(ui.motor.wait().unwrap_or(u32::MAX));
        ui.motor.update();
        let Some(e) = e else { continue };
        match e.kind {
            BUTTON => {
                ui.motor.off();
                api.keep_screen_on(false);
                return 0;
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
            TOUCH_DOWN => {
                ui.map = !ui.map;
                ui.motor.play(haptic::TICK);
                ui.draw();
            }
            CROWN if ui.map => {
                let z = (ui.zoom as i32 + e.delta.signum() as i32).clamp(0, ZOOMS.len() as i32 - 1) as usize;
                if z != ui.zoom {
                    ui.zoom = z;
                    ui.motor.play(haptic::TICK);
                    ui.draw();
                }
            }
            MESSAGE => {
                let mut redraw = false;
                while let Some(m) = api.recv(&mut msg) {
                    match nav().apply(m) {
                        Effect::Turn { buzz } => {
                            match buzz {
                                1 => ui.motor.play(haptic::DOUBLE),
                                2 => ui.motor.play(haptic::ALERT),
                                _ => {}
                            }
                            redraw = true;
                        }
                        Effect::Arrived => {
                            ui.motor.play(haptic::DONE);
                            redraw = true;
                        }
                        Effect::Pos | Effect::Reroute => redraw = true,
                        Effect::None => {}
                    }
                }
                if redraw {
                    ui.draw();
                }
            }
            _ => {}
        }
    }
}

/// Distance as a number and a unit: "450" "m", "1.2" "km".
fn distance(m: u32, buf: &mut [u8; 8]) -> (&str, &'static str) {
    if m < 1000 {
        let r = if m < 100 { m / 5 * 5 } else { m / 10 * 10 };
        let mut b = [0u8; 11];
        let t = int(r as i32, &mut b);
        buf[..t.len()].copy_from_slice(t.as_bytes());
        (core::str::from_utf8(&buf[..t.len()]).unwrap_or(""), "m")
    } else {
        let tenths = (m + 50) / 100;
        let mut b = [0u8; 11];
        let t = int((tenths / 10) as i32, &mut b);
        let n = t.len();
        buf[..n].copy_from_slice(t.as_bytes());
        buf[n] = b'.';
        buf[n + 1] = b'0' + (tenths % 10) as u8;
        (core::str::from_utf8(&buf[..n + 2]).unwrap_or(""), "km")
    }
}

/// The manoeuvre arrow, `size` px tall, centred on (cx, cy).
fn arrow(s: &mut &'static Api, cx: i32, cy: i32, size: i32, kind: u8, dir: u8) {
    let w = (size / 7).max(4);
    let (base, mid) = ((cx, cy + size / 2), (cx, cy));
    if kind == 2 {
        ring(s, cx, cy, size / 4, size / 4 + w / 2, WHITE, BLACK);
        disc(s, cx, cy, size / 6, RED, BLACK);
        return;
    }
    if dir == 7 {
        // U-turn: up, over, back down
        let (top, side) = (cy - size / 4, cx - size / 3);
        line(s, base.0, base.1, cx, top, w, WHITE, BLACK);
        line(s, cx, top, side, top, w, WHITE, BLACK);
        line(s, side, top, side, cy + size / 6, w, WHITE, BLACK);
        disc(s, cx, top, w / 2, WHITE, BLACK);
        disc(s, side, top, w / 2, WHITE, BLACK);
        head(s, side, cy + size / 6, 16384, size, w);
        return;
    }
    // direction of the exit in turns, y down (up = 49152)
    let a: u16 = match dir {
        1 => 40960,
        2 => 32768,
        3 => 24576,
        4 => 57344,
        5 => 0,
        6 => 8192,
        _ => 49152,
    };
    let len = size * 3 / 10;
    let end = (cx + (len * cos(a) >> 14), cy + (len * sin(a) >> 14));
    if kind == 1 {
        ring(s, cx, cy, size / 5, size / 5 + w / 2, Rgb(0x90, 0x90, 0x90), BLACK);
    }
    line(s, base.0, base.1, mid.0, mid.1, w, WHITE, BLACK);
    line(s, mid.0, mid.1, end.0, end.1, w, WHITE, BLACK);
    disc(s, mid.0, mid.1, w / 2, WHITE, BLACK);
    head(s, end.0, end.1, a, size, w);
}

/// A solid arrowhead whose base centre is (x, y), pointing along `a` (turns, y down).
fn head(s: &mut &'static Api, x: i32, y: i32, a: u16, size: i32, _w: i32) {
    let (ux, uy) = (cos(a), sin(a));
    let (len, half) = (size * 2 / 7, size / 5);
    let tip = (x + (len * ux >> 14), y + (len * uy >> 14));
    let l = (x + (half * -uy >> 14), y + (half * ux >> 14));
    let r = (x + (half * uy >> 14), y + (half * -ux >> 14));
    triangle(s, tip, l, r, W);
}

/// Filled triangle, one rect per scanline.
fn triangle(s: &mut &'static Api, a: (i32, i32), b: (i32, i32), c: (i32, i32), col: u16) {
    let (y0, y1) = (a.1.min(b.1).min(c.1), a.1.max(b.1).max(c.1));
    for y in y0..=y1 {
        let (mut lo, mut hi) = (i32::MAX, i32::MIN);
        for (p, q) in [(a, b), (b, c), (c, a)] {
            if (p.1 <= y && y <= q.1) || (q.1 <= y && y <= p.1) {
                let x = if p.1 == q.1 { p.0 } else { p.0 + (q.0 - p.0) * (y - p.1) / (q.1 - p.1) };
                lo = lo.min(x);
                hi = hi.max(x);
                if p.1 == q.1 {
                    lo = lo.min(q.0);
                    hi = hi.max(q.0);
                }
            }
        }
        if lo <= hi {
            Surface::rect(s, lo, y, hi - lo + 1, 1, col);
        }
    }
}

fn hhmm(h: u8, m: u8, b: &mut [u8; 5]) -> &str {
    *b = [b'0' + h / 10, b'0' + h % 10, b':', b'0' + m / 10, b'0' + m % 10];
    core::str::from_utf8(b).unwrap_or("")
}

impl Ui {
    fn draw(&mut self) {
        let mut s = self.api;
        Surface::rect(&mut s, 0, 0, 466, 466, 0);
        let n = nav();
        if !n.active {
            dots_centre(&mut s, C, 180, "NAV", 6, 5, W);
            disc(&mut s, C + 84, 222, 6, RED, BLACK);
            centre(&mut s, C, 260, "Start a route", GREY);
            centre(&mut s, C, 284, "on your phone", GREY);
        } else if n.arrived {
            arrow(&mut s, C, 170, 120, 2, 0);
            dots_centre(&mut s, C, 260, "ARRIVED", 4, 3, W);
            let (st, _) = clip(n.street(), 22);
            centre(&mut s, C, 310, st, SOFT);
        } else if self.map {
            self.draw_map();
        } else {
            self.draw_turn();
        }
        self.api.show();
    }

    fn eta(&self, y: i32) {
        let mut s = self.api;
        let n = nav();
        let mut t = [0u8; 5];
        dots_centre(&mut s, C, y, hhmm(n.arrive.0, n.arrive.1, &mut t), 3, 2, W);
        let mut mb = [0u8; 11];
        let mins = int(n.eta_min as i32, &mut mb);
        let mut db = [0u8; 8];
        let (d, unit) = distance(n.left_10m as u32 * 10, &mut db);
        // "12 min  3.4 km"
        let w = (mins.len() + 4 + 2 + d.len() + 1 + unit.len()) as i32 * CHAR;
        let mut x = C - w / 2;
        for (t, c) in [(mins, SOFT), (" min", GREY), ("  ", GREY), (d, SOFT), (" ", GREY), (unit, GREY)] {
            Surface::text(&mut s, x, y + 34, t, c);
            x += t.len() as i32 * CHAR;
        }
    }

    fn draw_turn(&self) {
        let mut s = self.api;
        let n = nav();
        if n.rerouting {
            dots_centre(&mut s, C, 200, "REROUTING", 3, 2, RED.rgb565());
            return;
        }
        arrow(&mut s, C, 128, 120, n.kind, n.dir);
        let mut db = [0u8; 8];
        let (d, unit) = distance(n.dist as u32, &mut db);
        let w = d.len() as i32 * 48 + 8 + unit.len() as i32 * CHAR;
        dots(&mut s, C - w / 2, 210, d, 6, 5, W);
        Surface::text(&mut s, C - w / 2 + d.len() as i32 * 48 + 8, 242, unit, GREY);
        let (x0, x1) = chord(282, 300, 20);
        let (st, _) = clip(n.street(), ((x1 - x0) / CHAR) as usize);
        centre(&mut s, C, 284, st, SOFT);
        let (x0, x1) = chord(336, 338, 70);
        let mut x = x0;
        while x < x1 {
            Surface::rect(&mut s, x, 336, 2, 2, DIM);
            x += 8;
        }
        self.eta(354);
    }

    fn draw_map(&self) {
        let mut s = self.api;
        let n = nav();
        let k = ZOOMS[self.zoom];
        // the route: segments with an end near the panel
        let mut prev: Option<(i32, i32)> = None;
        for &p in &n.pts[..n.n_pts] {
            let q = n.project(p, C, ME_Y, k);
            if let Some(o) = prev {
                let near = |a: (i32, i32)| (-200..666).contains(&a.0) && (-200..666).contains(&a.1);
                if near(o) || near(q) {
                    line(&mut s, o.0, o.1, q.0, q.1, 10, ROUTE, BLACK);
                }
            }
            prev = Some(q);
        }
        // we are here: a red dot in a white ring, pointing up
        ring(&mut s, C, ME_Y, 13, 17, WHITE, BLACK);
        disc(&mut s, C, ME_Y, 12, RED, BLACK);
        line(&mut s, C, ME_Y - 22, C, ME_Y - 34, 6, WHITE, BLACK);
        // the next turn on top, the arrival at the bottom
        Surface::rect(&mut s, 0, 0, 466, 112, 0);
        arrow(&mut s, C - 70, 60, 64, n.kind, n.dir);
        let mut db = [0u8; 8];
        let (d, unit) = distance(n.dist as u32, &mut db);
        dots(&mut s, C - 20, 40, d, 4, 3, W);
        Surface::text(&mut s, C - 20 + d.len() as i32 * 32 + 6, 58, unit, GREY);
        let (st, _) = clip(n.street(), 16);
        centre(&mut s, C, 90, st, SOFT);
        Surface::rect(&mut s, 0, 364, 466, 102, 0);
        self.eta(372);
    }
}
