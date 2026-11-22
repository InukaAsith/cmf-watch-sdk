//! Turn by turn in the Nothing look. Turn view (default): the next manoeuvre as a big arrow, the distance in dots,
//! the street, arrival time. Map view (tap to switch): the route heading-up around a red dot, drawn here from the
//! route points, so it turns and zooms (crown) at once. Buzz at 200 m and at the turn (state.rs SOON_M, NOW_M; view.rs SOON, NOW). Button: exit.

use crate::state::{Effect, Nav};
use crate::view::{self, ZOOMS};
use cmf_wp2_app::{Api, BUTTON, CROWN, MESSAGE, MSG_MAX, PAUSE, RESUME, TOUCH_DOWN};
use cmf_wp2_ui::color::{rgb565, BLACK, Rgb};
use cmf_wp2_ui::draw::disc;
use cmf_wp2_ui::haptic::{self, Motor};
use cmf_wp2_ui::text::{centre, dots_centre};
use cmf_wp2_ui::{C, Surface};

cmf_wp2_app::entry!(main);
cmf_wp2_app::app_icon!();

static mut NAV: Nav = Nav::new();

fn nav() -> &'static mut Nav {
    unsafe { &mut *core::ptr::addr_of_mut!(NAV) }
}

const RED: Rgb = Rgb(0xD7, 0x19, 0x21);
const W: u16 = 0xFFFF;
const GREY: u16 = rgb565(0x80, 0x80, 0x80);

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
}

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
    let mut ui = Ui { api, map: false, zoom: 2, motor: Motor::new(api), drawn: Drawn::Other, shown_at: 0, due: false };
    api.keep_screen_on(true);
    ui.draw();
    let _ = api.send(b"R");
    let mut ask = cmf_wp2_ui::ask::Ask::new(api.now(), 10);
    let mut msg = [0u8; MSG_MAX];
    loop {
        let pace = ui.due.then(|| FRAME_MS.saturating_sub(api.now().wrapping_sub(ui.shown_at)));
        let w = [ui.motor.wait(), ask.wait(api.now()), pace].into_iter().flatten().min().unwrap_or(u32::MAX);
        let e = api.event(w);
        ui.motor.update();
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
                    if matches!(m.first(), Some(b'G' | b'g')) {
                        // a new route: the old line can't be wiped by drawing it again
                        ui.drawn = Drawn::Other;
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
                        Effect::Pos | Effect::Reroute => redraw = true,
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
        match (self.drawn, now) {
            (Drawn::Turn(a), Drawn::Turn(b)) if a == b => {
                view::turn_live(self.api, n);
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
                return self.shown();
            }
            _ => {}
        }
        self.drawn = now;
        Surface::rect(&mut s, 0, 0, 466, 466, 0);
        if !n.active {
            dots_centre(&mut s, C, 180, "NAV", 6, 5, W);
            disc(&mut s, C + 84, 222, 6, RED, BLACK);
            centre(&mut s, C, 260, "Start a route", GREY);
            centre(&mut s, C, 284, "on your phone", GREY);
        } else if n.arrived {
            view::arrived(self.api, n);
        } else if self.map {
            view::map(self.api, n, ZOOMS[self.zoom]);
        } else {
            view::turn(self.api, n);
        }
        self.shown();
    }

    fn shown(&mut self) {
        self.api.show();
        self.shown_at = self.api.now();
    }
}
