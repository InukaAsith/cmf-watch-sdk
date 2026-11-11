#![cfg_attr(target_os = "none", no_std, no_main)]

#[cfg_attr(not(target_os = "none"), allow(dead_code))]
mod model;

#[cfg(not(target_os = "none"))]
fn main() {}

#[cfg(target_os = "none")]
mod device {
    use crate::model::*;
    use cmf_wp2_app::{Api, BUTTON, CROWN, MSG_MAX, PAUSE, RESUME, STORE_MAX, TOUCH_DOWN};

    cmf_wp2_app::entry!(main);
    cmf_wp2_app::app_icon!();

    const BG: u16 = 0x0000;
    const FG: u16 = 0xFFFF;
    const DIM: u16 = 0x8C71;
    const HILITE: u16 = 0x2124;
    const MOTOR_HZ: u16 = 31;
    const ASK_MS: u32 = 3000;
    // the daemon beats every 10 s
    const STALE_MS: u32 = 25_000;
    const AWAKE_MS: u32 = 20_000;
    const ANIM_MS: u32 = 80;
    const FLASH_MS: u32 = 300;
    static mut STORE: [u8; STORE_MAX] = [0; STORE_MAX];

    fn rgb(r: u32, g: u32, b: u32) -> u16 {
        ((r >> 3) << 11 | (g >> 2) << 5 | b >> 3) as u16
    }

    fn colour(s: Status, level: u32) -> u16 {
        let (r, g, b) = match s {
            Status::Blocked => (255, 80, 40),
            Status::Done => (60, 220, 110),
            Status::Working => (60, 140, 255),
            Status::Idle => (110, 110, 110),
            Status::Unknown => (60, 60, 60),
        };
        rgb(r * level / 256, g * level / 256, b * level / 256)
    }

    fn after(now: u32, t: u32) -> bool {
        (now.wrapping_sub(t) as i32) >= 0
    }

    /// Antialiased disc on black: 1/4 px coverage at the row ends.
    fn dot(api: &Api, cx: i32, cy: i32, r: i32, s: Status, level: u32) {
        let r4 = r * 4 + 2;
        for dy in -r..=r {
            let h4 = isqrt(r4 * r4 - dy * dy * 16);
            let (full, frac) = (h4 / 4, (h4 % 4) as u32);
            let y = (cy + dy) as i16;
            api.rect((cx - full) as i16, y, (2 * full + 1) as u16, 1, colour(s, level));
            let edge = colour(s, level * frac / 4);
            api.rect((cx - full - 1) as i16, y, 1, 1, edge);
            api.rect((cx + full + 1) as i16, y, 1, 1, edge);
        }
    }

    fn ellipsis(api: &Api, x: i32, y: i32, c: u16) {
        for i in 0..3 {
            api.rect((x + 1 + i * 5) as i16, (y + 12) as i16, 2, 2, c);
        }
    }

    fn text_clipped(api: &Api, x: i32, y: i32, s: &str, chars: usize, c: u16) {
        let (shown, cut) = clip(s, chars);
        if cut {
            let (shown, _) = clip(shown, chars - 1);
            api.text(x as i16, y as i16, shown, c);
            ellipsis(api, x + (chars as i32 - 1) * CHAR, y, c);
        } else {
            api.text(x as i16, y as i16, shown, c);
        }
    }

    fn number(n: usize, buf: &mut [u8; 3]) -> &str {
        let n = n.min(999);
        let mut i = 3;
        let mut v = n;
        loop {
            i -= 1;
            buf[i] = b'0' + (v % 10) as u8;
            v /= 10;
            if v == 0 {
                break;
            }
        }
        core::str::from_utf8(&buf[i..]).unwrap_or("?")
    }

    struct Ui {
        model: Model,
        scroll: usize,
        heard: Option<u32>,
        flash: Option<(usize, u32)>,
    }

    impl Ui {
        fn pulse(now: u32) -> u32 {
            let t = (now % 1200) as i32;
            (150 + (106 * (600 - (t - 600).abs()) / 600)) as u32
        }

        fn status_line(&self, now: u32) -> Option<(&'static str, u32)> {
            match self.heard {
                None => Some(("waiting for Mac...", 0)),
                Some(t) if now.wrapping_sub(t) > STALE_MS => Some(("stale", now.wrapping_sub(t) / 1000)),
                _ => None,
            }
        }

        fn draw(&self, api: &Api, now: u32) {
            api.rect(0, 0, api.width, api.height, BG);
            let (w, b, d) = self.model.counts();
            let mut x = C - 3 * 56 / 2;
            for (n, s) in [(b, Status::Blocked), (d, Status::Done), (w, Status::Working)] {
                dot(api, x + 8, 52, 7, s, 256);
                let mut buf = [0u8; 3];
                api.text((x + 20) as i16, 44, number(n, &mut buf), if n > 0 { FG } else { DIM });
                x += 56;
            }
            if let Some((label, secs)) = self.status_line(now) {
                let mut buf = [0u8; 3];
                let s = if secs > 0 { number(secs as usize, &mut buf) } else { "" };
                let chars = label.len() + if secs > 0 { s.len() + 2 } else { 0 };
                let x0 = C - chars as i32 * CHAR / 2;
                let c = colour(Status::Blocked, 200);
                api.text(x0 as i16, 76, label, c);
                if secs > 0 {
                    let x1 = x0 + (label.len() as i32 + 1) * CHAR;
                    api.text(x1 as i16, 76, s, c);
                    api.text((x1 + s.len() as i32 * CHAR) as i16, 76, "s", c);
                }
            }
            for r in 0..ROWS {
                let Some(a) = self.model.get(self.scroll + r) else { break };
                let (dx, tx, chars) = row_geometry(r);
                let y = LIST_TOP + r as i32 * ROW_H;
                if self.flash.is_some_and(|(i, _)| i == self.scroll + r) {
                    api.rect((dx - 14) as i16, (y + 2) as i16, (tx + chars as i32 * CHAR - dx + 20) as u16,
                        (ROW_H - 4) as u16, HILITE);
                }
                dot(api, dx, y + ROW_H / 2, 7, a.status, if a.status == Status::Working { Self::pulse(now) } else { 256 });
                text_clipped(api, tx, y + 9, a.title(), chars, FG);
                text_clipped(api, tx, y + 31, a.ws(), chars, DIM);
            }
            let more = self.model.len().saturating_sub(self.scroll + ROWS);
            if more > 0 {
                let mut buf = [0u8; 3];
                let s = number(more, &mut buf);
                let x = C - (s.len() as i32 + 1) * CHAR / 2;
                api.text(x as i16, 410, "+", DIM);
                api.text((x + CHAR) as i16, 410, s, DIM);
            }
            api.show();
        }

        /// Repaints only the dots of visible working agents.
        fn animate(&self, api: &Api, now: u32) -> bool {
            let mut any = false;
            for r in 0..ROWS {
                let Some(a) = self.model.get(self.scroll + r) else { break };
                if a.status == Status::Working && !self.flash.is_some_and(|(i, _)| i == self.scroll + r) {
                    let (dx, _, _) = row_geometry(r);
                    dot(api, dx, LIST_TOP + r as i32 * ROW_H + ROW_H / 2, 7, a.status, Self::pulse(now));
                    any = true;
                }
            }
            if any {
                api.show();
            }
            any
        }
    }

    fn motor(api: &Api, duty: u16) {
        if api.vibrate(duty, MOTOR_HZ) != 0 {
            api.vibrate(0, MOTOR_HZ); // clears a -4 lock
        }
    }

    fn save(api: &Api, model: &Model) {
        let buf = unsafe { &mut *&raw mut STORE };
        let n = model.save(buf);
        api.save(&buf[..n]);
    }

    // true on RESUME, false on BUTTON
    fn paused(api: &Api) -> bool {
        loop {
            match api.event(u32::MAX).map(|e| e.kind) {
                Some(RESUME) => return true,
                Some(BUTTON) => return false,
                _ => {}
            }
        }
    }

    fn main(api: &'static Api) -> i32 {
        let mut ui = Ui { model: Model::new(), scroll: 0, heard: None, flash: None };
        let buf = unsafe { &mut *&raw mut STORE };
        let n = api.load(buf);
        if n > 0 {
            ui.model.load(&buf[..n as usize]);
        }
        let mut player = Player::default();
        let mut want = true;
        let mut asked: Option<u32> = None;
        let mut awake: Option<u32> = None;
        let mut unsaved = false;
        let mut dirty = true;
        let mut animated = 0u32;
        let mut shown_line = None;
        loop {
            let now = api.now();
            if let Some(d) = player.tick(now) {
                motor(api, d);
            }
            if ui.flash.is_some_and(|(_, t)| after(now, t)) {
                ui.flash = None;
                dirty = true;
            }
            if awake.is_some_and(|t| after(now, t)) {
                api.keep_screen_on(false);
                awake = None;
            }
            let line = ui.status_line(now);
            if line != shown_line {
                shown_line = line;
                dirty = true;
            }
            if dirty {
                ui.draw(api, now);
                dirty = false;
                animated = now;
            } else if now.wrapping_sub(animated) >= ANIM_MS {
                ui.animate(api, now);
                animated = now;
            }
            let stale = ui.heard.is_none_or(|t| now.wrapping_sub(t) > STALE_MS);
            if (want || stale) && asked.is_none_or(|t| now.wrapping_sub(t) >= ASK_MS) {
                api.send(REQUEST);
                asked = Some(now);
            }

            let wait = player.wait(now).unwrap_or(ANIM_MS).min(ANIM_MS);
            match api.event(wait).map(|e| (e.kind, e.y, e.delta)) {
                Some((BUTTON, _, _)) => break,
                Some((PAUSE, _, _)) => {
                    player.stop();
                    motor(api, 0);
                    if !paused(api) {
                        break;
                    }
                    want = true;
                    asked = None;
                    dirty = true;
                }
                Some((RESUME, _, _)) => {
                    want = true;
                    dirty = true;
                }
                Some((CROWN, _, delta)) if delta != 0 => {
                    let s = (ui.scroll as i32 + delta.signum() as i32).clamp(0, max_scroll(ui.model.len()) as i32);
                    dirty |= s as usize != ui.scroll;
                    ui.scroll = s as usize;
                }
                Some((TOUCH_DOWN, y, _)) => {
                    if let Some(i) = row_at(y as i32, ui.scroll, ui.model.len()) {
                        let id = ui.model.get(i).map_or(0, |a| a.id);
                        api.send(&[b'h', b'1', b'F', id]);
                        ui.flash = Some((i, api.now().wrapping_add(FLASH_MS)));
                        dirty = true;
                    }
                }
                _ => {}
            }

            let mut msg = [0u8; MSG_MAX];
            let mut buzz = None;
            while let Some(m) = api.recv(&mut msg) {
                let fx = ui.model.apply(m);
                if decode(m).is_some() {
                    ui.heard = Some(api.now());
                }
                dirty |= fx.changed;
                unsaved |= fx.changed;
                buzz = buzz.max(fx.buzz);
                if fx.resync {
                    want = true;
                    asked = None;
                }
                if fx.synced {
                    want = false;
                    save(api, &ui.model);
                    unsaved = false;
                }
            }
            ui.scroll = ui.scroll.min(max_scroll(ui.model.len()));
            if let Some(b) = buzz {
                let now = api.now();
                player.start(b.pattern(), now);
                api.keep_screen_on(true);
                awake = Some(now.wrapping_add(AWAKE_MS));
            }
        }
        player.stop();
        motor(api, 0);
        if unsaved {
            save(api, &ui.model);
        }
        0
    }
}
