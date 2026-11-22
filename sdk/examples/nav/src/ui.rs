//! Turn by turn in the Nothing look. Turn view (default): the next manoeuvre as a big arrow, the distance in dots,
//! the street, arrival time. Map view (tap to switch): the route heading-up around a red dot, drawn here from the
//! route points, so it turns and zooms (crown) at once. Buzz at 200 m and at the turn (state.rs SOON_M, NOW_M; view.rs SOON, NOW). Button: exit.
//! No route on: start one here (start.rs), as on the phone.

use crate::state::{Effect, Nav};
use crate::view::{self, ZOOMS};
use crate::render::{self, View};
use crate::scene::{Ev, Scene};
use crate::start::{Screen, Start, Tap};
use cmf_wp2_app::{Api, BUTTON, CROWN, MESSAGE, MSG_MAX, PAUSE, RESUME, TOUCH_DOWN, TOUCH_UP};
use cmf_wp2_ui::color::{Rgb, BLACK};
use cmf_wp2_ui::dial::Act;
use cmf_wp2_ui::draw::{disc, line};
use cmf_wp2_ui::haptic::{self, Motor};
use cmf_wp2_ui::{Surface, C};

const WHITE: Rgb = Rgb(0xFF, 0xFF, 0xFF);

cmf_wp2_app::entry!(main);
cmf_wp2_app::app_icon!();

static mut NAV: Nav = Nav::new();
static mut SCENE: Scene = Scene::new();

fn scene() -> &'static mut Scene {
    unsafe { &mut *core::ptr::addr_of_mut!(SCENE) }
}

/// The map view's boxes the marks take (no street names under them): the time pill, the next turn, us, the ✕.
const KEEP: [(i32, i32, i32, i32); 4] = [
    (C - 142, 38, C + 142, 86),
    (106, 346, 360, 430),
    (C - 30, view::ME_Y - 30, C + 30, view::ME_Y + 30),
    (C - 24, 424, C + 24, 466),
];
/// A street map asked for and not come: asked again after this long.
const MAP_AGAIN_MS: u32 = 10_000;

fn nav() -> &'static mut Nav {
    unsafe { &mut *core::ptr::addr_of_mut!(NAV) }
}


struct Ui {
    api: &'static Api,
    map: bool,
    zoom: usize,
    motor: Motor,
    /// What is on the panel now, so the next frame changes only what moved (no full clear: no flicker)
    drawn: Drawn,
    /// when the last frame was shown; a message's redraw waits till FRAME_MS after it
    shown_at: u32,
    due: bool,
    /// no route on: where to (start.rs)
    start: Start,
    /// the ✕ tapped once: red, the next tap on it stops the route (any other tap: not)
    armed: bool,
    /// the street map: the last asked (its id, round where, when)
    map_id: u8,
    map_asked: Option<((i32, i32), u32)>,
    /// a street map is on the panel: while the next comes in, only the marks are drawn over it
    map_up: bool,
}

/// The ✕ at the bottom: stops the route (tapped twice), or leaves the arrival.
const STOP_X: (i32, i32, i32) = (C, 444, 17);

#[derive(Clone, Copy, PartialEq)]
enum Drawn {
    Other,
    Turn([u8; 6]),
    /// the route drawn at this place and heading, this zoom
    Map((i16, i16), u16, usize),
}

/// The panel takes a frame some ms after `show`, from the buffer we draw into: the next frame waits this long.
const FRAME_MS: u32 = 80;

fn main(api: &'static Api) -> i32 {
    let mut ui = Ui { api, map: false, zoom: 2, motor: Motor::new(api), drawn: Drawn::Other, shown_at: 0, due: false, start: Start::new(), armed: false, map_id: 0, map_asked: None, map_up: false };
    // the button is ours: back from the keyboard or the list (else the loader stops the app a second later)
    api.own_back(true);
    api.keep_screen_on(true);
    ui.draw();
    let _ = api.send(b"R");
    let mut ask = cmf_wp2_ui::ask::Ask::new(api.now(), 10);
    let mut msg = [0u8; MSG_MAX];
    loop {
        let pace = ui.due.then(|| FRAME_MS.saturating_sub(api.now().wrapping_sub(ui.shown_at)));
        let typing = !nav().active && ui.start.screen == Screen::Dial;
        let hold = typing.then(|| ui.start.dial.hold_wait(api.now())).flatten();
        let w = [ui.motor.wait(), ask.wait(api.now()), pace, hold].into_iter().flatten().min().unwrap_or(u32::MAX);
        let e = api.event(w);
        ui.motor.update();
        if typing && ui.start.dial.tick(&mut ui.api, api.now()) == Act::Show {
            // held: a capital
            ui.motor.play(haptic::TICK);
            api.show();
        }
        if ui.due && api.now().wrapping_sub(ui.shown_at) >= FRAME_MS {
            ui.draw();
        }
        if ask.due(api.now()) {
            let _ = api.send(b"R");
        }
        let Some(e) = e else { continue };
        if e.kind == MESSAGE {
            ask.answered();
        }
        match e.kind {
            BUTTON if !nav().active && ui.start.back() => {
                // the rotary keyboard: back to the keys; the keys or the list: to the start, the recent places again
                if ui.start.screen == Screen::Start {
                    let _ = api.send(b"R");
                }
                ui.draw();
            }
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
                ui.drawn = Drawn::Other;
                ui.draw();
            }
            TOUCH_DOWN if !nav().active => {
                if ui.start.on_query(e.x.into(), e.y.into()) {
                    // the typed words on the rotary keyboard: back to the keys
                    ui.motor.play(haptic::TICK);
                    ui.start.to_keys();
                    ui.draw();
                } else if ui.start.screen == Screen::Keys {
                    let mut out = [0u8; 64];
                    match ui.start.keys_down(api, e.x.into(), e.y.into(), &mut out) {
                        Tap::Shown => ui.shown(),
                        Tap::Typed => {
                            ui.motor.play(haptic::TICK);
                            ui.shown();
                        }
                        Tap::Redraw => {
                            ui.motor.play(haptic::TICK);
                            ui.draw();
                        }
                        Tap::Send(n) => {
                            ui.motor.play(haptic::TICK);
                            let _ = api.send(&out[..n]);
                            ui.draw();
                        }
                    }
                } else if ui.start.screen == Screen::Dial {
                    let a = ui.start.dial.down(&mut ui.api, e.x.into(), e.y.into(), api.now());
                    ui.dial_act(a);
                } else if let Some(id) = ui.start.hit(e.x.into(), e.y.into()) {
                    ui.motor.play(haptic::TICK);
                    let mut out = [0u8; 64];
                    let n = ui.start.act(id, &mut out);
                    if n > 0 {
                        let _ = api.send(&out[..n]);
                    }
                    ui.draw();
                }
            }
            TOUCH_UP if !nav().active && ui.start.screen == Screen::Dial => {
                let a = ui.start.dial.up(&mut ui.api, e.x.into(), e.y.into());
                ui.dial_act(a);
            }
            CROWN if !nav().active => {
                if ui.start.screen == Screen::Dial {
                    let a = ui.start.dial.crown(&mut ui.api, e.delta.signum().into(), api.now());
                    ui.dial_act(a);
                } else if ui.start.crown(e.delta.signum().into()) {
                    ui.draw();
                }
            }
            TOUCH_DOWN if ui.on_x(e.x.into(), e.y.into()) => {
                ui.motor.play(haptic::TICK);
                if ui.armed || nav().arrived {
                    // the phone stops too (an arrival is over there already); then where to next
                    if !nav().arrived {
                        let _ = api.send(b"C");
                    }
                    ui.ended();
                } else {
                    ui.armed = true;
                    ui.draw_x();
                    ui.shown();
                }
            }
            TOUCH_DOWN => {
                if ui.armed {
                    ui.armed = false;
                    ui.draw_x();
                }
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
                    // the map view's streets (Maps' scene): drawn once all of it is here
                    if matches!(m.first(), Some(b'S' | b'd')) {
                        if scene().message(m) == Ev::Done && ui.map && nav().active {
                            ui.map_asked = None;
                            ui.map_up = false;
                            redraw = true;
                        }
                        continue;
                    }
                    // where to: places, notes
                    if ui.start.apply(m) {
                        redraw |= !nav().active;
                        continue;
                    }
                    if matches!(m.first(), Some(b'G' | b'g')) {
                        // a new route: the old line can't be wiped by drawing it again
                        ui.drawn = Drawn::Other;
                    }
                    if m.first() == Some(&b'G') {
                        // its frame may be new: the streets asked again in it
                        ui.forget_map();
                    }
                    match nav().apply(m) {
                        Effect::Turn { buzz } => {
                            view::buzz(&mut ui.motor, buzz);
                            redraw = true;
                        }
                        Effect::Arrived => {
                            ui.motor.play(view::THERE);
                            redraw = true;
                        }
                        // while no route is on, route points need no frame each (no flashing start screen)
                        Effect::Pos | Effect::Reroute => redraw |= nav().active,
                        Effect::Ended => {
                            ui.ended();
                            continue;
                        }
                        Effect::None => {}
                    }
                }
                if redraw {
                    if api.now().wrapping_sub(ui.shown_at) >= FRAME_MS {
                        ui.draw();
                    } else {
                        ui.due = true;
                    }
                }
            }
            _ => {}
        }
    }
}

impl Ui {
    fn draw(&mut self) {
        let mut s = self.api;
        self.due = false;
        let n = nav();
        let now = if !n.active || n.arrived {
            Drawn::Other
        } else if self.map {
            Drawn::Map(n.pos, n.heading, self.zoom)
        } else {
            Drawn::Turn(view::turn_key(n))
        };
        if !matches!(now, Drawn::Map(..)) {
            self.map_up = false;
        } else {
            self.want_map();
            let sc = scene();
            if sc.active && sc.done || self.map_up {
                // Maps' streets under the route and the marks; while the next map comes in, the old one stays and
                // only the marks are drawn again
                if sc.active && sc.done {
                    let v = View {
                        at: (i32::from(n.pos.0), i32::from(n.pos.1)),
                        screen: (C, view::ME_Y),
                        k: ZOOMS[self.zoom],
                        heading: n.heading,
                    };
                    render::draw(self.api, sc, &v, &n.pts[..n.n_pts], &KEEP);
                    self.map_up = true;
                }
                view::overlay(self.api, n);
                self.drawn = Drawn::Other;
                self.draw_x();
                return self.shown();
            }
        }
        match (self.drawn, now) {
            (Drawn::Turn(a), Drawn::Turn(b)) if a == b => {
                view::turn_live(self.api, n);
                self.draw_x();
                return self.shown();
            }
            (Drawn::Map(pos, heading, zoom), Drawn::Map(..)) => {
                // the old line wiped in black, then the new one: no frame of a cleared screen
                let (p, h) = (n.pos, n.heading);
                (n.pos, n.heading) = (pos, heading);
                view::map_erase(self.api, n, ZOOMS[zoom]);
                (n.pos, n.heading) = (p, h);
                view::map(self.api, n, ZOOMS[self.zoom]);
                self.drawn = now;
                self.draw_x();
                return self.shown();
            }
            _ => {}
        }
        self.drawn = now;
        Surface::rect(&mut s, 0, 0, 466, 466, 0);
        if !n.active {
            self.start.draw(self.api);
        } else if n.arrived {
            view::arrived(self.api, n);
        } else if self.map {
            view::map(self.api, n, ZOOMS[self.zoom]);
        } else {
            view::turn(self.api, n);
        }
        if n.active {
            self.draw_x();
        }
        self.shown();
    }

    /// The map view near its street map's edge (or none yet): the next one round where we are, asked of the phone
    /// (`M id x y`), again if it hasn't come in a while.
    fn want_map(&mut self) {
        let n = nav();
        let p = (i32::from(n.pos.0), i32::from(n.pos.1));
        let now = self.api.now();
        if let Some((_, at)) = self.map_asked {
            if now.wrapping_sub(at) < MAP_AGAIN_MS {
                return;
            }
        }
        let sc = scene();
        if sc.active && sc.id == sc.want {
            let reach = sc.r - 233 * 256 / ZOOMS[self.zoom].max(1);
            if (p.0 - sc.centre.0).abs() <= reach && (p.1 - sc.centre.1).abs() <= reach {
                return;
            }
        }
        self.map_id = self.map_id.wrapping_add(1).max(1);
        sc.want = self.map_id;
        let (x, y) = ((p.0.clamp(-32000, 32000) as i16).to_le_bytes(), (p.1.clamp(-32000, 32000) as i16).to_le_bytes());
        let _ = self.api.send(&[b'M', self.map_id, x[0], x[1], y[0], y[1]]);
        self.map_asked = Some((p, now));
    }

    fn on_x(&self, x: i32, y: i32) -> bool {
        let (cx, cy, r) = STOP_X;
        nav().active && (x - cx).abs() <= r + 22 && (y - cy).abs() <= r + 18
    }

    /// The ✕: dark, or red once tapped (tap again to stop).
    fn draw_x(&mut self) {
        let mut s = self.api;
        let (cx, cy, r) = STOP_X;
        let red = Rgb(0xD7, 0x19, 0x21);
        let fill = if self.armed { red } else { Rgb(0x2A, 0x2A, 0x2A) };
        disc(&mut s, cx, cy, r, fill, BLACK);
        let k = 6;
        line(&mut s, cx - k, cy - k, cx + k, cy + k, 3, WHITE, fill);
        line(&mut s, cx - k, cy + k, cx + k, cy - k, 3, WHITE, fill);
    }

    /// The route is over: the start screen, the recent places asked for.
    fn forget_map(&mut self) {
        scene().clear();
        self.map_asked = None;
        self.map_up = false;
    }

    fn ended(&mut self) {
        nav().end();
        self.forget_map();
        self.armed = false;
        self.map = false;
        self.motor.off();
        while self.start.back() {}
        self.drawn = Drawn::Other;
        let _ = self.api.send(b"R");
        self.draw();
    }

    /// What the keyboard did: a letter into the words (only they and the middle redrawn), or the search sent.
    fn dial_act(&mut self, a: Act) {
        match a {
            Act::None => {}
            Act::Show => {
                self.motor.play(haptic::TICK);
                self.shown();
            }
            Act::Redraw => self.draw(),
            Act::Key(_) => {
                self.motor.play(haptic::TICK);
                let mut out = [0u8; 64];
                let n = self.start.key(a, &mut out);
                if n > 0 {
                    let _ = self.api.send(&out[..n]);
                    self.draw();
                } else {
                    self.start.redraw_query(self.api);
                    self.shown();
                }
            }
        }
    }

    fn shown(&mut self) {
        self.api.show();
        self.shown_at = self.api.now();
    }
}
