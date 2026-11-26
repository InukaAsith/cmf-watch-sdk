//! Turn by turn drawing, shared by Navigation and Maps. `turn`: time left and arrival on top, the manoeuvre's arrow
//! big, what to do in words, the distance to it. `map`: the route heading-up round where we are, the same top line,
//! the next turn's arrow and distance at the bottom. `arrived`. And the buzzes as a turn comes (`buzz`).

use crate::state::{Nav, instruction};
use cmf_wp2_app::Api;
use cmf_wp2_ui::color::{BLACK, Rgb, rgb565};
use cmf_wp2_ui::draw::{disc, line, ring, rrect};
use cmf_wp2_ui::font;
use cmf_wp2_ui::geom::chord;
use cmf_wp2_ui::haptic::{Motor, Pattern};
use cmf_wp2_ui::motion::{cos, sin};
use cmf_wp2_ui::text::{dots, int};
use cmf_wp2_ui::{C, Surface};

const WHITE: Rgb = Rgb(0xFF, 0xFF, 0xFF);
/// The app's accent (Navigation red, Maps blue): each sets `crate::ACCENT`.
const RED: Rgb = crate::ACCENT;
#[allow(dead_code)] // Navigation's map view (Maps draws its own)
const ROUTE: Rgb = Rgb(0xE8, 0xE8, 0xE8);
const PILL: Rgb = Rgb(0x14, 0x14, 0x14);
const W: u16 = 0xFFFF;
const GREY: u16 = rgb565(0x80, 0x80, 0x80);
/// Where we are on the map view.
pub const ME_Y: i32 = 280;
pub const ZOOMS: [i32; 5] = [96, 160, 256, 416, 640]; // px per metre × 256

/// The turn is close (state.rs SOON_M): two firm pulses.
pub const SOON: Pattern = Pattern::new(&[(900, 140), (0, 130), (900, 140), (0, 0)]);
/// Turn now (NOW_M): three long strong pulses.
pub const NOW: Pattern = Pattern::new(&[
    (1000, 260),
    (0, 120),
    (1000, 260),
    (0, 120),
    (1000, 260),
    (0, 0),
]);
/// Arrived: one long pulse.
pub const THERE: Pattern = Pattern::new(&[(1000, 500), (0, 0)]);

/// What `Effect::Turn { buzz }` asks for.
pub fn buzz(m: &mut Motor, buzz: u8) {
    match buzz {
        1 => m.play(SOON),
        2 => m.play(NOW),
        _ => {}
    }
}

/// Arrived: the target ring and the street.
pub fn arrived(api: &'static Api, n: &Nav) {
    let mut s = api;
    arrow(&mut s, C, 160, 130, 2, 0);
    font::centre(&mut s, C, 250, "You have arrived", W);
    let (x0, x1) = chord(280, 304, 40);
    let st = if n.street() == "Arrive" {
        ""
    } else {
        n.street()
    };
    let w = font::width(st).min(x1 - x0);
    font::fitted(&mut s, C - w / 2, 284, st, x1 - x0, GREY);
}

/// Distance as a number and a unit: "450" "m", "1.2" "km".
pub fn distance(m: u32, buf: &mut [u8; 8]) -> (&str, &'static str) {
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
pub fn arrow(s: &mut &'static Api, cx: i32, cy: i32, size: i32, kind: u8, dir: u8) {
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
        head(s, side, cy + size / 6, 16384, size);
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
        ring(
            s,
            cx,
            cy,
            size / 5,
            size / 5 + w / 2,
            Rgb(0x90, 0x90, 0x90),
            BLACK,
        );
    }
    line(s, base.0, base.1, mid.0, mid.1, w, WHITE, BLACK);
    line(s, mid.0, mid.1, end.0, end.1, w, WHITE, BLACK);
    disc(s, mid.0, mid.1, w / 2, WHITE, BLACK);
    head(s, end.0, end.1, a, size);
}

/// A solid arrowhead whose base centre is (x, y), pointing along `a` (turns, y down).
fn head(s: &mut &'static Api, x: i32, y: i32, a: u16, size: i32) {
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
                let x = if p.1 == q.1 {
                    p.0
                } else {
                    p.0 + (q.0 - p.0) * (y - p.1) / (q.1 - p.1)
                };
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

pub fn hhmm(h: u8, m: u8, b: &mut [u8; 5]) -> &str {
    *b = [
        b'0' + h / 10 % 10,
        b'0' + h % 10,
        b':',
        b'0' + m / 10 % 10,
        b'0' + m % 10,
    ];
    core::str::from_utf8(b).unwrap_or("")
}

/// "20 min · 13:14" (time left, arrival) centred on `y`, red like the reference's accent line; with `left` the
/// distance left too: "20 min · 4.2 km · 13:14".
fn top_line(s: &mut &'static Api, n: &Nav, y: i32, left: bool) {
    let mut b = [0u8; 40];
    let mut k = 0;
    let mut put = |t: &str, k: &mut usize| {
        let m = t.len().min(40 - *k);
        b[*k..*k + m].copy_from_slice(&t.as_bytes()[..m]);
        *k += m;
    };
    let mut ib = [0u8; 11];
    let mins = n.eta_min as i32;
    if mins >= 60 {
        put(int(mins / 60, &mut ib), &mut k);
        put(" h ", &mut k);
        put(int(mins % 60, &mut ib), &mut k);
    } else {
        put(int(mins.max(1), &mut ib), &mut k);
    }
    put(" min \u{b7} ", &mut k);
    if left {
        let mut db = [0u8; 8];
        let (d, unit) = distance(n.left_10m as u32 * 10, &mut db);
        put(d, &mut k);
        put(" ", &mut k);
        put(unit, &mut k);
        put(" \u{b7} ", &mut k);
    }
    let mut t = [0u8; 5];
    put(hhmm(n.arrive.0, n.arrive.1, &mut t), &mut k);
    let text = core::str::from_utf8(&b[..k]).unwrap_or("");
    font::centre(s, C, y, text, RED.rgb565());
}

/// The distance to the turn in big dots and its unit, centred on `cx`.
fn big_distance(s: &mut &'static Api, n: &Nav, cx: i32, y: i32, pitch: i32) {
    let mut db = [0u8; 8];
    let (d, unit) = distance(n.dist as u32, &mut db);
    let dw = d.len() as i32 * 8 * pitch;
    let w = dw + 8 + font::width(unit);
    dots(s, cx - w / 2, y, d, pitch, pitch - 1, W);
    font::draw(
        s,
        cx - w / 2 + dw + 8,
        y + 7 * pitch - font::HEIGHT + 2,
        unit,
        GREY,
    );
}

/// The full turn view: time and arrival, the arrow, what to do, how far.
pub fn turn(api: &'static Api, n: &Nav) {
    let mut s = api;
    if n.rerouting {
        font::centre(&mut s, C, 200, "Rerouting\u{2026}", RED.rgb565());
        if n.lost {
            lost_notice(&mut s, false);
        }
        return;
    }
    top_line(&mut s, n, 40, false);
    if n.lost {
        lost_notice(&mut s, false);
    }
    arrow(&mut s, C, 140, 120, n.kind, n.dir);
    let mut ib = [0u8; 112];
    let what = instruction(n.kind, n.dir, n.street(), &mut ib);
    let mut lines = [""; 3];
    let (x0, x1) = chord(226, 316, 34);
    let k = font::wrap(what, x1 - x0, &mut lines);
    let y0 = 270 - (k as i32 * 30) / 2;
    for (i, l) in lines[..k].iter().enumerate() {
        let w = font::width(l).min(x1 - x0);
        font::fitted(&mut s, C - w / 2, y0 + i as i32 * 30, l, x1 - x0, W);
    }
    big_distance(&mut s, n, C, 344, 5);
    left_line(&mut s, n);
}

/// "4.2 km left" under the turn view's distance.
fn left_line(s: &mut &'static Api, n: &Nav) {
    let mut db = [0u8; 8];
    let (d, unit) = distance(n.left_10m as u32 * 10, &mut db);
    let mut b = [0u8; 20];
    let mut k = 0;
    for t in [d, " ", unit, " left"] {
        b[k..k + t.len()].copy_from_slice(t.as_bytes());
        k += t.len();
    }
    font::centre(s, C, 404, core::str::from_utf8(&b[..k]).unwrap_or(""), GREY);
}

/// What the turn view's still parts show (the arrow, the words): while it stays the same, `turn_live` redraws only
/// the numbers, not the whole screen (a full clear each second flickers: the panel may take the black half-way).
pub fn turn_key(n: &Nav) -> [u8; 6] {
    [n.step, n.kind, n.dir, n.rerouting as u8, n.street().len() as u8, n.street().bytes().fold(0u8, |a, b| a.wrapping_mul(31).wrapping_add(b))]
}

/// The turn view's numbers again (time and arrival, the distance to the turn, the distance left), each band cleared
/// just before it is drawn, over a turn view drawn with the same `turn_key`.
pub fn turn_live(api: &'static Api, n: &Nav) {
    let mut s = api;
    if n.rerouting {
        if n.lost {
            lost_notice(&mut s, false);
        }
        return;
    }
    Surface::rect(&mut s, C - 160, 36, 320, font::HEIGHT + 8, 0);
    top_line(&mut s, n, 40, false);
    if n.lost {
        lost_notice(&mut s, false);
    }
    Surface::rect(&mut s, 40, 340, 386, 7 * 5 + 8, 0);
    big_distance(&mut s, n, C, 344, 5);
    Surface::rect(&mut s, C - 160, 400, 320, font::HEIGHT + 6, 0);
    left_line(&mut s, n);
}

/// The map view's route as drawn at `n`'s place and heading, in black: what `map` drew, wiped without clearing
/// the screen.
#[allow(dead_code)] // Navigation's map view (Maps draws its own)
pub fn map_erase(api: &'static Api, n: &Nav, k: i32) {
    route_lines(api, n, k, BLACK);
    north_erase(api);
}

/// The north mark's place as last drawn (a red disc with N on the rim): wiped before the next.
static mut NORTH_AT: Option<(i32, i32)> = None;
const NORTH_R: i32 = 210;
const NORTH_DOT: i32 = 15;

/// The north mark off the panel, for a caller that draws over the old frame instead of clearing it.
pub fn north_erase(api: &'static Api) {
    let mut s = api;
    if let Some((x, y)) = unsafe { (*core::ptr::addr_of_mut!(NORTH_AT)).take() } {
        Surface::rect(&mut s, x - NORTH_DOT - 2, y - NORTH_DOT - 2, 2 * NORTH_DOT + 4, 2 * NORTH_DOT + 4, 0);
    }
}

/// Where north is on the heading-up map: a red disc with N on the rim, as a compass.
fn north(s: &mut &'static Api, heading: u16) {
    let a = ((360 - u32::from(heading) % 360) * 65536 / 360) as u16;
    let (x, y) = (C + (NORTH_R * sin(a) >> 14), C - (NORTH_R * cos(a) >> 14));
    disc(s, x, y, NORTH_DOT, RED, BLACK);
    let w = font::width("N");
    font::draw(s, x - w / 2, y - 10, "N", W);
    unsafe { *core::ptr::addr_of_mut!(NORTH_AT) = Some((x, y)) };
}

/// Nothing from the phone for a while (`Nav::lost`): a red notice where the time line is (`map`: the map view's
/// pill). The next normal frame paints over it.
fn lost_notice(s: &mut &'static Api, map: bool) {
    let t = "No update \u{b7} retrying";
    if map {
        rrect(s, C - 140, 42, 280, 40, 20, RED, BLACK);
        font::centre(s, C, 50, t, W);
    } else {
        rrect(s, C - 118, 38, 236, 26, 13, RED, BLACK);
        font::centre(s, C, 40, t, W);
    }
}

#[allow(dead_code)] // Navigation's map view (Maps draws its own)
fn route_lines(api: &'static Api, n: &Nav, k: i32, col: Rgb) {
    let mut s = api;
    let w = if col == BLACK { 16 } else { 12 };
    let mut prev: Option<(i32, i32)> = None;
    for &p in &n.pts[..n.n_pts] {
        let q = n.project(p, C, ME_Y, k);
        if let Some(o) = prev {
            let near = |a: (i32, i32)| (-200..666).contains(&a.0) && (-200..666).contains(&a.1);
            if near(o) || near(q) {
                line(&mut s, o.0, o.1, q.0, q.1, w, col, BLACK);
            }
        }
        prev = Some(q);
    }
}

/// The route heading-up round where we are, `k` px per metre × 256 (ZOOMS); time on top, the next turn below.
#[allow(dead_code)] // Navigation's map view (Maps draws its own)
pub fn map(api: &'static Api, n: &Nav, k: i32) {
    // the route: segments with an end near the panel
    route_lines(api, n, k, ROUTE);
    overlay(api, n);
}

/// The map view's marks over a map drawn by the caller (Maps draws the streets under them): where we are, the time
/// line on top, the next turn below.
pub fn overlay(api: &'static Api, n: &Nav) {
    overlay_as(api, n, RED);
}

/// The overlay with "you" in colour `me` (Offline Maps: where the position comes from).
pub fn overlay_as(api: &'static Api, n: &Nav, me: Rgb) {
    let mut s = api;
    // north on the rim, under the marks and pills
    north(&mut s, n.heading);
    // we are here: a white arrow on a red disc, pointing up
    ring(&mut s, C, ME_Y, 22, 26, WHITE, BLACK);
    disc(&mut s, C, ME_Y, 22, me, BLACK);
    triangle(
        &mut s,
        (C, ME_Y - 14),
        (C - 11, ME_Y + 12),
        (C + 11, ME_Y + 12),
        W,
    );
    // time left, distance left, arrival: low enough on the round panel for all three
    rrect(&mut s, C - 140, 42, 280, 40, 20, PILL, BLACK);
    top_line(&mut s, n, 50, true);
    if n.lost {
        lost_notice(&mut s, true);
    }
    // the next turn: a small arrow and the distance on a dark pill
    rrect(&mut s, C - 120, 352, 240, 72, 36, PILL, BLACK);
    arrow(&mut s, C - 70, 388, 52, n.kind, n.dir);
    big_distance(&mut s, n, C + 26, 374, 4);
}
