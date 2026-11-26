#![cfg_attr(any(target_os = "none", wp2_sim), no_main, cfg_attr(target_os = "none", no_std))]

//! Compass: the iPhone's heading on the watch (the watch has no magnetometer). A dial that turns so north stays
//! north, the heading big in dots in the middle, its name under it. CMFX sends the heading (`model.rs` has the
//! protocol); a tick buzz each time north passes the mark at the top. Button: exit.

#[cfg_attr(not(any(target_os = "none", wp2_sim)), allow(dead_code))]
mod model;

#[cfg(not(any(target_os = "none", wp2_sim)))]
fn main() {}

#[cfg(any(target_os = "none", wp2_sim))]
mod app {
    use crate::model::{self, Msg, CALIBRATE, PING_MS, STALE_MS, TRUE_NORTH};
    use cmf_wp2_app::{Api, BUTTON, MESSAGE, MSG_MAX, PAUSE, RESUME};
    use cmf_wp2_ui::color::{rgb565, Rgb, BLACK};
    use cmf_wp2_ui::draw::{line, ring};
    use cmf_wp2_ui::font;
    use cmf_wp2_ui::haptic::{self, Motor};
    use cmf_wp2_ui::text::dots;
    use cmf_wp2_ui::{Surface, C};

    cmf_wp2_app::entry!(main);
    cmf_wp2_app::app_icon!();

    const WHITE: Rgb = Rgb(0xFF, 0xFF, 0xFF);
    const RED: Rgb = Rgb(0xD7, 0x19, 0x21);
    const TICK: Rgb = Rgb(0x70, 0x70, 0x70);
    const GREY: u16 = rgb565(0x80, 0x80, 0x80);
    const DIM: u16 = rgb565(0x50, 0x50, 0x50);

    /// The dial: ticks every 10°, long ones every 30°, from R_IN (long) / R_MID (short) to R_OUT.
    const R_OUT: i32 = 222;
    const R_MID: i32 = 208;
    const R_IN: i32 = 194;
    /// N E S W in dots (pitch 4: a 32 px box) this far from the centre.
    const R_LETTER: i32 = 168;
    const LETTER_PITCH: i32 = 4;
    /// The heading digits: dots at pitch 10 (80 px tall) from this row.
    const DIGITS_Y: i32 = 168;
    const DIGITS_PITCH: i32 = 10;
    const NAME_Y: i32 = 262;
    const STATUS_Y: i32 = 296;
    const STATUS_W: i32 = 200;
    /// The panel takes a frame some ms after `show`: the dial turns at most this often.
    const FRAME_MS: u32 = 60;

    struct Ui {
        api: &'static Api,
        /// the dial as drawn (degrees), None: not drawn yet
        dial: Option<u16>,
        /// the centre as drawn: digits, name, status, stale
        centre: Option<(u16, bool)>,
        status: [u8; 64],
        status_len: usize,
        drawn_status: Option<([u8; 64], usize)>,
    }

    fn main(api: &'static Api) -> i32 {
        api.keep_screen_on(true);
        let mut ui = Ui { api, dial: None, centre: None, status: [0; 64], status_len: 0, drawn_status: None };
        let mut motor = Motor::new(api);
        let mut heading: Option<u16> = None;
        let mut heard_at = api.now();
        let mut ping_at = api.now();
        let mut shown_at = api.now().wrapping_sub(FRAME_MS);
        let mut due = true;
        let mut msg = [0u8; MSG_MAX];
        ui.set_status("Waiting for iPhone\u{2026}");
        let _ = api.send(b"R");
        loop {
            let now = api.now();
            let stale = heading.is_some() && now.wrapping_sub(heard_at) >= STALE_MS;
            if due && now.wrapping_sub(shown_at) >= FRAME_MS {
                ui.draw(heading, stale);
                shown_at = now;
                due = false;
            }
            let ping_left = PING_MS.saturating_sub(now.wrapping_sub(ping_at));
            let frame_left = due.then(|| FRAME_MS.saturating_sub(now.wrapping_sub(shown_at)));
            let stale_left = (heading.is_some() && !stale).then(|| STALE_MS.saturating_sub(now.wrapping_sub(heard_at)));
            let wait = [Some(ping_left), frame_left, stale_left, motor.wait()].into_iter().flatten().min().unwrap_or(1000);
            let e = api.event(wait);
            motor.update();
            let now = api.now();
            if now.wrapping_sub(ping_at) >= PING_MS {
                // the phone stops sending once these stop coming
                let _ = api.send(b"R");
                ping_at = now;
            }
            let stale = heading.is_some() && now.wrapping_sub(heard_at) >= STALE_MS;
            if stale && ui.centre.map(|c| !c.1).unwrap_or(false) {
                ui.set_status("No iPhone heading");
                due = true;
            }
            let Some(e) = e else { continue };
            match e.kind {
                BUTTON => {
                    motor.off();
                    let _ = api.send(b"Z");
                    api.keep_screen_on(false);
                    return 0;
                }
                PAUSE => {
                    motor.off();
                    let _ = api.send(b"Z");
                    loop {
                        match api.event(u32::MAX) {
                            Some(e) if e.kind == RESUME => break,
                            Some(e) if e.kind == BUTTON => {
                                api.keep_screen_on(false);
                                return 0;
                            }
                            _ => {}
                        }
                    }
                    let _ = api.send(b"R");
                    ping_at = api.now();
                    // the loader repaints the framebuffer as it was: ours is still right
                    due = true;
                }
                MESSAGE => {
                    while let Some(m) = api.recv(&mut msg) {
                        match model::parse(m) {
                            Some(Msg::Heading { tenths, accuracy, flags }) => {
                                let deg = model::degrees(tenths);
                                if let Some(old) = heading {
                                    if model::crossed_north(old, deg) {
                                        motor.play(haptic::TICK);
                                    }
                                }
                                heading = Some(deg);
                                heard_at = api.now();
                                if flags & CALIBRATE != 0 {
                                    ui.set_status("Calibrate iPhone");
                                } else if accuracy != 255 && accuracy > 20 {
                                    ui.set_accuracy(accuracy, flags & TRUE_NORTH != 0);
                                } else if flags & TRUE_NORTH != 0 {
                                    ui.set_status("True north");
                                } else {
                                    ui.set_status("Magnetic north");
                                }
                                due = true;
                            }
                            Some(Msg::Note(t)) => {
                                ui.set_status(t);
                                due = true;
                            }
                            None => {}
                        }
                    }
                }
                _ => {}
            }
        }
    }

    impl Ui {
        fn set_status(&mut self, t: &str) {
            let b = cmf_wp2_ui::text::fit(t.as_bytes(), self.status.len());
            self.status[..b.len()].copy_from_slice(b.as_bytes());
            self.status_len = b.len();
        }

        fn set_accuracy(&mut self, acc: u8, true_north: bool) {
            let mut b = [0u8; 64];
            let mut k = 0;
            let mut ib = [0u8; 11];
            let n = cmf_wp2_ui::text::int(acc as i32, &mut ib);
            for part in ["\u{b1}", n, "\u{b0} \u{b7} ", if true_north { "true" } else { "magnetic" }] {
                b[k..k + part.len()].copy_from_slice(part.as_bytes());
                k += part.len();
            }
            self.status[..k].copy_from_slice(&b[..k]);
            self.status_len = k;
        }

        fn draw(&mut self, heading: Option<u16>, stale: bool) {
            let mut s = self.api;
            let h = heading.unwrap_or(0);
            if self.dial != Some(h) {
                if let Some(old) = self.dial {
                    dial(&mut s, old, true);
                }
                dial(&mut s, h, false);
                // the mark at 12: where the iPhone points
                line(&mut s, C, 4, C, 46, 6, WHITE, BLACK);
                self.dial = Some(h);
            }
            let centre = heading.map(|d| (d, stale));
            if self.centre != centre || centre.is_none() && self.drawn_status.is_none() {
                Surface::rect(&mut s, C - 150, DIGITS_Y - 4, 300, NAME_Y + font::HEIGHT - DIGITS_Y + 8, 0);
                let c = if stale { DIM } else { 0xFFFF };
                match heading {
                    Some(d) => {
                        let mut b = [0u8; 3];
                        let t = model::digits(d, &mut b);
                        let w = t.len() as i32 * 8 * DIGITS_PITCH - (DIGITS_PITCH - 1) - 2 * DIGITS_PITCH;
                        let x = C - w / 2 - 12;
                        dots(&mut s, x, DIGITS_Y, t, DIGITS_PITCH, DIGITS_PITCH - 2, c);
                        // the degree sign, apart from the digits
                        ring(&mut s, x + w + 22, DIGITS_Y + 9, 5, 10, if stale { Rgb(0x50, 0x50, 0x50) } else { WHITE }, BLACK);
                        font::centre(&mut s, C, NAME_Y, model::cardinal(d), if stale { DIM } else { RED.rgb565() });
                    }
                    None => {
                        dots(&mut s, C - 3 * 8 * DIGITS_PITCH / 2 + DIGITS_PITCH, DIGITS_Y, "---", DIGITS_PITCH, DIGITS_PITCH - 2, DIM);
                    }
                }
                self.centre = centre;
            }
            let st = (self.status, self.status_len);
            if self.drawn_status != Some(st) {
                Surface::rect(&mut s, C - STATUS_W / 2 - 2, STATUS_Y - 2, STATUS_W + 4, font::HEIGHT + 4, 0);
                let t = core::str::from_utf8(&self.status[..self.status_len]).unwrap_or("");
                let (t, _) = font::clip(t, STATUS_W);
                let w = font::width(t);
                font::draw(&mut s, C - w / 2, STATUS_Y, t, GREY);
                self.drawn_status = Some(st);
            }
            self.api.show();
        }
    }

    /// The turning part: ticks and N E S W for heading `h`; `erase` paints the same shapes black.
    fn dial(s: &mut &'static Api, h: u16, erase: bool) {
        for b in (0..360).step_by(10) {
            let rel = b as i32 - h as i32;
            let long = b % 30 == 0;
            let (x0, y0) = model::polar(C, if long { R_IN } else { R_MID }, rel);
            let (x1, y1) = model::polar(C, R_OUT, rel);
            let c = if erase { BLACK } else if b == 0 { RED } else if long { WHITE } else { TICK };
            line(s, x0, y0, x1, y1, if long { 4 } else { 2 }, c, BLACK);
        }
        for (b, t) in [(0, "N"), (90, "E"), (180, "S"), (270, "W")] {
            let (x, y) = model::polar(C, R_LETTER, b - h as i32);
            let half = 4 * LETTER_PITCH;
            if erase {
                Surface::rect(s, x - half, y - half, 2 * half, 2 * half, 0);
            } else {
                let c = if b == 0 { RED.rgb565() } else { 0xFFFF };
                dots(s, x - half, y - half, t, LETTER_PITCH, LETTER_PITCH - 1, c);
            }
        }
    }
}
