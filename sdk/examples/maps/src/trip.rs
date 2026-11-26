//! Offline Maps: directions and turn by turn, as Maps looks (atlas Ways and Routes, Navigation's view.rs).
//!
//! - Ways: car, bike and on foot at once, each with the time, arrival (ETA) and distance; a tap on one with two or
//!   three routes lists them (Routes), else that route comes.
//! - Route: the whole route on the map, the time, distance and arrival, Start.
//! - Navigating: the turn view (time and arrival, the arrow, what to do, how far, buzzes as a turn comes) or, after
//!   a tap, the map heading-up round where you are. All worked out here from where you are (the watch's GPS, or
//!   the iPhone's as well in hybrid): the phone only finds the route, so turn by turn goes on without it. Off the
//!   route with the phone there, it is asked for a new one; without it, the way back to the route.
//!
//! Phone protocol: `D mode …` (one each mode) → `O mode n` (0: no route), `o mode k secs:u32 metres:u32` each;
//! `p mode k` picks one → `S n`, `s k kind dir x:i16 y:i16 street…` (the turns, nav state.rs kinds), then the
//! route's points `G` + `g` (ui.rs).

use super::*;
use crate::state::Effect;
use crate::view;

pub const MODES: [&str; 3] = ["Car", "Bike", "Walk"];
/// The phone answers within this, else "No answer".
const ANSWER_MS: u32 = 20_000;
/// Off the route this long (with the phone there): a new route is asked.
const REROUTE_MS: u32 = 6_000;
const WAY_Y: [i32; 3] = [112, 204, 296];
const WAY_H: i32 = 84;
const START_Y: i32 = 364;
const ROW_C: Rgb = Rgb(0x3A, 0x3A, 0x3A);

#[derive(Clone, Copy, PartialEq)]
pub enum Way {
    Asking,
    Got,
    NoRoute,
    NoPhone,
    NoAnswer,
}

pub struct Trip {
    pub way: [Way; 3],
    pub alts: [[(u32, u32); 3]; 3],
    pub n_alts: [u8; 3],
    asked: u32,
    pub to: (i32, i32),
    /// the route picked (mode, alternative), its time and length
    pub mode: u8,
    secs: u32,
    metres: u32,
    /// a picked route on its way (asked at)
    pub loading: Option<u32>,
    pub steps_want: usize,
    pub n_steps: usize,
    pub driving: bool,
    pub turn_view: bool,
    pub zoom: usize,
    off_since: Option<u32>,
    reroute_at: Option<u32>,
    key: [u8; 6],
    shown: bool,
}

impl Trip {
    pub const fn new() -> Trip {
        Trip {
            way: [Way::NoRoute; 3],
            alts: [[(0, 0); 3]; 3],
            n_alts: [0; 3],
            asked: 0,
            to: (0, 0),
            mode: 0,
            secs: 0,
            metres: 0,
            loading: None,
            steps_want: 0,
            n_steps: 0,
            driving: false,
            turn_view: true,
            zoom: 2,
            off_since: None,
            reroute_at: None,
            key: [0; 6],
            shown: false,
        }
    }
}

/// A line of text built in place.
struct Line {
    b: [u8; 48],
    n: usize,
}

impl Line {
    fn new() -> Line {
        Line { b: [0; 48], n: 0 }
    }
    fn s(&mut self, t: &str) -> &mut Line {
        let k = t.len().min(48 - self.n);
        self.b[self.n..self.n + k].copy_from_slice(&t.as_bytes()[..k]);
        self.n += k;
        self
    }
    fn num(&mut self, v: u32) -> &mut Line {
        let mut ib = [0u8; 11];
        self.s(int(v as i32, &mut ib))
    }
    fn two(&mut self, v: u8) -> &mut Line {
        if v < 10 {
            self.s("0");
        }
        self.num(u32::from(v))
    }
    fn mins(&mut self, secs: u32) -> &mut Line {
        let m = (secs + 59) / 60;
        if m >= 60 {
            self.num(m / 60).s(" h ").num(m % 60).s(" min")
        } else {
            self.num(m.max(1)).s(" min")
        }
    }
    fn dist(&mut self, metres: u32) -> &mut Line {
        let mut b = [0u8; 16];
        self.s(fmt_dist(metres as i32, &mut b))
    }
    fn get(&self) -> &str {
        core::str::from_utf8(&self.b[..self.n]).unwrap_or("")
    }
}

fn way_icon(s: &mut &'static Api, mode: usize, cx: i32, cy: i32, c: Rgb, bg: Rgb) {
    match mode {
        0 => {
            rrect(s, cx - 10, cy - 13, 20, 12, 4, c, bg);
            rrect(s, cx - 17, cy - 4, 34, 13, 4, c, bg);
            disc(s, cx - 10, cy + 10, 5, c, bg);
            disc(s, cx + 10, cy + 10, 5, c, bg);
        }
        1 => {
            ring(s, cx - 11, cy + 6, 6, 9, c, bg);
            ring(s, cx + 11, cy + 6, 6, 9, c, bg);
            line(s, cx - 11, cy + 6, cx - 3, cy - 6, 3, c, bg);
            line(s, cx - 3, cy - 6, cx + 8, cy - 6, 3, c, bg);
            line(s, cx + 8, cy - 6, cx + 11, cy + 6, 3, c, bg);
            line(s, cx - 3, cy - 6, cx + 1, cy + 6, 3, c, bg);
            disc(s, cx + 1, cy - 14, 3, c, bg);
        }
        _ => {
            disc(s, cx + 2, cy - 15, 5, c, bg);
            line(s, cx + 1, cy - 8, cx - 2, cy + 5, 5, c, bg);
            line(s, cx - 2, cy + 5, cx - 9, cy + 17, 4, c, bg);
            line(s, cx - 2, cy + 5, cx + 6, cy + 17, 4, c, bg);
            line(s, cx, cy - 5, cx - 9, cy + 1, 3, c, bg);
            line(s, cx, cy - 5, cx + 9, cy - 1, 3, c, bg);
        }
    }
}

impl Ui {
    /// The clock `secs` from now (h, m), if the watch knows the time.
    fn arrival(&self, secs: u32) -> Option<(u8, u8)> {
        let t = self.api.time()?;
        let s = u32::from(t.hour) * 3600 + u32::from(t.minute) * 60 + u32::from(t.second) + secs;
        Some((((s / 3600) % 24) as u8, ((s / 60) % 60) as u8))
    }

    /// "ETA 14:32 · 4.2 km".
    fn eta_line(&self, secs: u32, metres: u32) -> Line {
        let mut l = Line::new();
        if let Some((h, m)) = self.arrival(secs) {
            l.s("ETA ").num(u32::from(h)).s(":").two(m).s(" \u{b7} ");
        }
        l.dist(metres);
        l
    }

    // --- Ways

    /// Directions to the sheet's place: every mode asked of the phone at once.
    pub fn ask_ways(&mut self) {
        let to = self.sheet.0;
        self.trip.to = to;
        self.trip.asked = self.api.now();
        self.trip.loading = None;
        for m in 0..3 {
            self.trip.n_alts[m] = 0;
            self.trip.way[m] = if self.ask_route(m as u8, to) { Way::Asking } else { Way::NoPhone };
        }
        self.screen = Screen::Ways;
        self.dirty = true;
    }

    fn title(&self) -> &str {
        if self.place.n > 0 {
            self.place.name()
        } else if let Some(i) = self.sheet.1 {
            marks().v[i].name()
        } else {
            "Directions"
        }
    }

    pub fn draw_ways(&mut self) {
        let mut s = self.s;
        Surface::rect(&mut s, 0, 0, 466, 466, 0);
        font::fitted(&mut s, C - font::width(self.title()).min(280) / 2, 40, self.title(), 280, W);
        if let Some(me) = self.me {
            let mut l = Line::new();
            l.dist(dist(me, self.trip.to) as u32).s(" away");
            font::centre(&mut s, C, 72, l.get(), GREY);
        }
        for (i, &y) in WAY_Y.iter().enumerate() {
            let (x0, x1) = cmf_wp2_ui::geom::chord(y, y + WAY_H, 16);
            rrect(&mut s, x0, y + 3, x1 - x0, WAY_H - 6, (WAY_H - 6) / 2, CARD, BLACK);
            way_icon(&mut s, i, x0 + 48, y + WAY_H / 2, BLUE, CARD);
            let (tx, tw) = (x0 + 88, x1 - x0 - 112);
            if self.trip.way[i] == Way::Got {
                let (secs, metres) = self.trip.alts[i][0];
                let mut l = Line::new();
                l.mins(secs);
                if self.trip.n_alts[i] > 1 {
                    l.s("  \u{b7}  ").num(u32::from(self.trip.n_alts[i])).s(" routes");
                }
                font::fitted(&mut s, tx, y + 14, l.get(), tw, W);
                font::fitted(&mut s, tx, y + 44, self.eta_line(secs, metres).get(), tw, GREY);
                continue;
            }
            let why = match self.trip.way[i] {
                Way::Asking => "Finding\u{2026}",
                Way::NoRoute => "No route found",
                Way::NoPhone => "iPhone not connected",
                _ => "No answer from iPhone",
            };
            font::fitted(&mut s, tx, y + 14, MODES[i], tw, W);
            font::fitted(&mut s, tx, y + 44, why, tw, GREY);
        }
        self.draw_loading_note();
    }

    fn draw_loading_note(&self) {
        if self.trip.loading.is_some() {
            let mut s = self.s;
            rrect(&mut s, C - 130, 392, 260, 44, 22, BLUE, BLACK);
            font::centre(&mut s, C, 402, "Getting the route\u{2026}", W);
        }
    }

    pub fn ways_tap(&mut self, y: i32) {
        let Some(i) = WAY_Y.iter().position(|&w| (w..w + WAY_H).contains(&y)) else { return };
        self.motor.play(haptic::TICK);
        match self.trip.way[i] {
            Way::Got if self.trip.n_alts[i] > 1 => {
                self.trip.mode = i as u8;
                self.screen = Screen::Routes;
            }
            Way::Got => self.pick(i as u8, 0),
            Way::NoPhone | Way::NoAnswer => self.ask_ways(),
            _ => {}
        }
    }

    // --- Routes: one mode's alternatives

    pub fn draw_routes(&mut self) {
        let mut s = self.s;
        Surface::rect(&mut s, 0, 0, 466, 466, 0);
        let m = usize::from(self.trip.mode);
        let mut l = Line::new();
        l.s(MODES[m]).s(" \u{b7} ").num(u32::from(self.trip.n_alts[m])).s(" routes");
        font::centre(&mut s, C, 56, l.get(), W);
        for k in 0..usize::from(self.trip.n_alts[m]) {
            let y = WAY_Y[k];
            let (x0, x1) = cmf_wp2_ui::geom::chord(y, y + WAY_H, 16);
            rrect(&mut s, x0, y + 3, x1 - x0, WAY_H - 6, (WAY_H - 6) / 2, if k == 0 { ROW_C } else { CARD }, BLACK);
            let (secs, metres) = self.trip.alts[m][k];
            let (tx, tw) = (x0 + 40, x1 - x0 - 80);
            let mut l = Line::new();
            l.mins(secs);
            font::fitted(&mut s, tx, y + 14, l.get(), tw, W);
            if k == 0 {
                font::draw(&mut s, x1 - 40 - font::width("Fastest"), y + 14, "Fastest", rgb565(0x34, 0xA8, 0x53));
            }
            font::fitted(&mut s, tx, y + 44, self.eta_line(secs, metres).get(), tw, GREY);
        }
        self.draw_loading_note();
    }

    pub fn routes_tap(&mut self, y: i32) {
        let m = usize::from(self.trip.mode);
        if let Some(k) = WAY_Y.iter().position(|&w| (w..w + WAY_H).contains(&y)) {
            if k < usize::from(self.trip.n_alts[m]) {
                self.motor.play(haptic::TICK);
                self.pick(m as u8, k as u8);
            }
        }
    }

    /// That route of the phone's, its turns and points.
    fn pick(&mut self, mode: u8, k: u8) {
        self.trip.mode = mode;
        (self.trip.secs, self.trip.metres) = self.trip.alts[usize::from(mode)][usize::from(k)];
        if self.api.send(&[b'p', mode, k]) < 0 {
            self.set_note("iPhone not connected");
            self.trip.way[usize::from(mode)] = Way::NoPhone;
        } else {
            self.trip.loading = Some(self.api.now());
            self.trip.n_steps = 0;
            self.trip.steps_want = 0;
        }
        self.dirty = true;
    }

    // --- the phone's answers (true: handled)

    pub fn trip_message(&mut self, m: &[u8]) -> bool {
        let u32_at = |i: usize| u32::from_le_bytes([m[i], m[i + 1], m[i + 2], m[i + 3]]);
        let i16_at = |i: usize| i16::from_le_bytes([m[i], m[i + 1]]);
        match m.first() {
            Some(b'O') if m.len() == 3 && m[1] < 3 => {
                let i = usize::from(m[1]);
                self.trip.n_alts[i] = 0;
                self.trip.way[i] = if m[2] == 0 { Way::NoRoute } else { Way::Asking };
                if m[2] == 0 && self.trip.reroute_at.is_some() && m[1] == self.trip.mode {
                    self.trip.reroute_at = None;
                }
            }
            Some(b'o') if m.len() == 11 && m[1] < 3 && m[2] < 3 => {
                let (i, k) = (usize::from(m[1]), usize::from(m[2]));
                self.trip.alts[i][k] = (u32_at(3), u32_at(7));
                self.trip.n_alts[i] = self.trip.n_alts[i].max(k as u8 + 1);
                self.trip.way[i] = Way::Got;
                // a new route while navigating: the fastest, straight on
                if self.trip.driving && self.trip.reroute_at.is_some() && m[1] == self.trip.mode && k == 0 {
                    (self.trip.secs, self.trip.metres) = self.trip.alts[i][0];
                    let _ = self.api.send(&[b'p', m[1], 0]);
                    self.trip.n_steps = 0;
                }
            }
            Some(b'S') if m.len() == 2 => {
                self.trip.steps_want = usize::from(m[1]).min(ram::STEPS);
                self.trip.n_steps = 0;
            }
            Some(b's') if m.len() >= 8 && usize::from(m[1]) < ram::STEPS => {
                let k = usize::from(m[1]);
                let st = &mut ram().steps[k];
                st.kind = m[2];
                st.dir = m[3];
                st.p = (i16_at(4), i16_at(6));
                let t = cmf_wp2_ui::text::fit(&m[8..], 32).as_bytes();
                st.name[..t.len()].copy_from_slice(t);
                st.n = t.len() as u8;
                self.trip.n_steps = self.trip.n_steps.max(k + 1);
            }
            _ => return false,
        }
        self.dirty = true;
        true
    }

    /// The picked route's points have all come (ui.rs `g`): the turns put on it, then its preview, or on with it
    /// when it is a new route while navigating.
    pub fn route_ready(&mut self) {
        let r = ram();
        let (path, steps) = (&r.path[..r.path_n], &mut r.steps[..self.trip.n_steps]);
        let mut from = 0;
        for st in steps.iter_mut() {
            let mut best = (i64::MAX, from);
            for (i, &q) in path.iter().enumerate().skip(from) {
                let (dx, dy) = (i64::from(q.0) - i64::from(st.p.0), i64::from(q.1) - i64::from(st.p.1));
                if dx * dx + dy * dy < best.0 {
                    best = (dx * dx + dy * dy, i);
                }
            }
            st.idx = best.1 as u16;
            from = best.1;
        }
        self.trip.loading = None;
        if self.trip.driving {
            self.trip.reroute_at = None;
            self.trip.off_since = None;
            self.start_drive();
        } else {
            self.screen = Screen::Preview;
        }
        self.dirty = true;
    }

    // --- Route preview

    fn preview_view(&self) -> View {
        let r = ram();
        let (mut lo, mut hi) = ((i32::MAX, i32::MAX), (i32::MIN, i32::MIN));
        for &q in &r.path[..r.path_n] {
            let q = p32(q);
            lo = (lo.0.min(q.0), lo.1.min(q.1));
            hi = (hi.0.max(q.0), hi.1.max(q.1));
        }
        if lo.0 > hi.0 {
            return View { at: self.trip.to, screen: (C, C), k: 110, heading: 0 };
        }
        let span = (hi.0 - lo.0).max(hi.1 - lo.1).max(50);
        View { at: ((lo.0 + hi.0) / 2, (lo.1 + hi.1) / 2), screen: (C, C + 10), k: (250 * 256 / span).clamp(2, 290), heading: 0 }
    }

    pub fn draw_preview(&mut self) {
        let mut s = self.s;
        let v = self.preview_view();
        Surface::rect(&mut s, 0, 0, 466, 466, BG.rgb565());
        if let Some(p) = self.pack {
            self.draw_cells(&p, &v);
        }
        self.route_line(&v);
        let (x, y) = v.px(self.trip.to);
        render::pin(&mut s, x, y);
        self.draw_me(&v);
        rrect(&mut s, C - 140, 34, 280, 76, 30, CARD, BG);
        let mut l = Line::new();
        l.mins(self.trip.secs).s("  ");
        let w = font::width(l.get()) + 28;
        way_icon(&mut s, usize::from(self.trip.mode), C - w / 2 + 2, 58, BLUE, CARD);
        font::draw(&mut s, C - w / 2 + 28, 44, l.get(), W);
        font::centre(&mut s, C, 76, self.eta_line(self.trip.secs, self.trip.metres).get(), GREY);
        rrect(&mut s, C - 90, START_Y, 180, 60, 30, BLUE, BG);
        font::centre(&mut s, C, START_Y + 18, "Start", W);
    }

    /// The route, blue.
    fn route_line(&self, v: &View) {
        let mut s = self.s;
        let r = ram();
        for pair in r.path[..r.path_n].windows(2) {
            let (a, b) = (v.px(p32(pair[0])), v.px(p32(pair[1])));
            if let Some((a, b)) = clip(a, b, 8) {
                line(&mut s, a.0, a.1, b.0, b.1, 7, BLUE, BG);
            }
        }
    }

    pub fn preview_tap(&mut self, y: i32) {
        if (START_Y - 10..START_Y + 70).contains(&y) {
            self.trip.turn_view = true;
            self.start_drive();
        }
    }

    // --- navigating

    fn start_drive(&mut self) {
        self.trip.driving = true;
        self.trip.shown = false;
        ram().nav = crate::state::Nav::new();
        self.start_guide(Guide::Path { back: false });
        self.screen = Screen::Drive;
        self.nav_update();
        self.dirty = true;
    }

    /// Navigation ended (List › stop guiding, the route's end left behind).
    pub fn end_drive(&mut self) {
        self.trip.driving = false;
        self.trip.reroute_at = None;
        ram().nav.end();
        if self.screen == Screen::Drive {
            self.screen = Screen::Map;
        }
    }

    /// The turn view's numbers from where you are now: the next turn along the route and how far to it, the time,
    /// distance and arrival; buzzes as a turn comes; off the route, a new one from the phone if it is there.
    pub fn nav_update(&mut self) {
        if !self.trip.driving {
            return;
        }
        let (Some(me), Some(o)) = (self.me, self.on_path) else { return };
        let now = self.api.now();
        let r = ram();
        let path = &r.path[..r.path_n];
        if path.len() < 2 {
            return;
        }
        let steps = &r.steps[..self.trip.n_steps];
        let next = steps.iter().position(|s| usize::from(s.idx) > o.seg);
        let (si, kind, dir, idx, name) = match next {
            Some(i) => {
                let s = &steps[i];
                (i, s.kind, s.dir, usize::from(s.idx), &s.name[..usize::from(s.n)])
            }
            None => (steps.len(), 2, 0, path.len() - 1, &b""[..]),
        };
        // along the route to the turn
        let mut d = dist(me, p32(path[(o.seg + 1).min(path.len() - 1)]));
        for i in o.seg + 1..idx.min(path.len() - 1) {
            d += dist(p32(path[i]), p32(path[i + 1]));
        }
        let left = o.left.max(0) as u32;
        let secs = if self.trip.metres > 0 { (u64::from(left) * u64::from(self.trip.secs) / u64::from(self.trip.metres)) as u32 } else { left };
        let (hh, mm) = self.arrival(secs).unwrap_or((0, 0));
        let mut m = [0u8; 12 + 32];
        m[0] = b'V';
        m[1] = si as u8;
        m[2] = kind;
        m[3] = dir;
        m[4..6].copy_from_slice(&(d.clamp(0, 65535) as u16).to_le_bytes());
        m[6..8].copy_from_slice(&(((secs + 59) / 60).min(65535) as u16).to_le_bytes());
        m[8..10].copy_from_slice(&((left / 10).min(65535) as u16).to_le_bytes());
        m[10] = hh;
        m[11] = mm;
        m[12..12 + name.len()].copy_from_slice(name);
        let n = &mut r.nav;
        if self.arrived {
            if !n.arrived {
                n.apply(b"W");
                self.motor.play(view::THERE);
            }
        } else if self.trip.reroute_at.is_none() {
            if let Effect::Turn { buzz } = n.apply(&m[..12 + name.len()]) {
                view::buzz(&mut self.motor, buzz);
            }
        }
        let mut x = [b'X', 0, 0, 0, 0, 0, 0];
        x[5..7].copy_from_slice(&self.course.unwrap_or(0).to_le_bytes());
        n.apply(&x);
        // off the route: a new one from the phone (else the way back to it, as the map shows)
        if o.off > OFF && !self.arrived {
            let since = *self.trip.off_since.get_or_insert(now);
            if now.wrapping_sub(since) >= REROUTE_MS && self.trip.reroute_at.is_none() && !self.watch_only {
                if self.ask_route(self.trip.mode, self.trip.to) {
                    self.trip.reroute_at = Some(now);
                    r.nav.apply(b"r");
                }
            }
        } else {
            self.trip.off_since = None;
        }
        self.dirty |= self.screen == Screen::Drive;
    }

    /// No answer in time: the ways say so, a route that didn't come, a new route not coming (on with the old).
    pub fn trip_timers(&mut self, now: u32) {
        if self.trip.way.contains(&Way::Asking) && now.wrapping_sub(self.trip.asked) >= ANSWER_MS {
            for w in &mut self.trip.way {
                if *w == Way::Asking {
                    *w = Way::NoAnswer;
                }
            }
            self.dirty = true;
        }
        if self.trip.loading.is_some_and(|t| now.wrapping_sub(t) >= ANSWER_MS) {
            self.trip.loading = None;
            self.set_note("No answer from iPhone");
            self.dirty = true;
        }
        if self.trip.reroute_at.is_some_and(|t| now.wrapping_sub(t) >= ANSWER_MS) {
            self.trip.reroute_at = None;
            ram().nav.rerouting = false;
            self.dirty = true;
        }
        if self.trip.driving && self.guide != (Guide::Path { back: false }) {
            self.end_drive();
            self.dirty = true;
        }
    }

    pub fn draw_drive(&mut self) {
        let mut s = self.s;
        let n = &ram().nav;
        if n.arrived {
            Surface::rect(&mut s, 0, 0, 466, 466, 0);
            view::arrived(self.api, n);
            self.trip.shown = false;
            return;
        }
        if self.trip.turn_view {
            let key = view::turn_key(n);
            if self.trip.shown && key == self.trip.key {
                view::turn_live(self.api, n);
            } else {
                Surface::rect(&mut s, 0, 0, 466, 466, 0);
                view::turn(self.api, n);
                self.trip.key = key;
                self.trip.shown = true;
            }
            return;
        }
        self.trip.shown = false;
        let v = View {
            at: self.me.unwrap_or(self.centre),
            screen: (C, view::ME_Y),
            k: view::ZOOMS[self.trip.zoom],
            heading: self.course.unwrap_or(0),
        };
        Surface::rect(&mut s, 0, 0, 466, 466, BG.rgb565());
        if let Some(p) = self.pack {
            self.draw_cells(&p, &v);
        }
        self.route_line(&v);
        let (x, y) = v.px(self.trip.to);
        render::pin(&mut s, x, y);
        view::overlay(self.api, n);
    }

    pub fn drive_tap(&mut self) {
        if ram().nav.arrived {
            self.guide = Guide::None;
            self.end_drive();
            return;
        }
        self.trip.turn_view = !self.trip.turn_view;
        self.trip.shown = false;
        self.motor.play(haptic::TICK);
    }

    pub fn drive_crown(&mut self, d: i32) {
        if !self.trip.turn_view {
            self.trip.zoom = (self.trip.zoom as i32 + d).clamp(0, view::ZOOMS.len() as i32 - 1) as usize;
            self.dirty = true;
        }
    }
}
