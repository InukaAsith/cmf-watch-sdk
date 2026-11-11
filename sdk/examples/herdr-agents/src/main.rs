#![cfg_attr(any(target_os = "none", wp2_sim), no_main, cfg_attr(target_os = "none", no_std))]

#[cfg_attr(not(target_os = "none"), allow(dead_code))]
mod model;
#[cfg_attr(not(target_os = "none"), allow(dead_code))]
mod view;

#[cfg(not(any(target_os = "none", wp2_sim)))]
fn main() {}

#[cfg(any(target_os = "none", wp2_sim))]
mod device {
    use crate::model::*;
    use crate::view::Ui;
    use cmf_wp2_app::{Api, BUTTON, CROWN, MSG_MAX, PAUSE, RESUME, STORE_MAX, TOUCH_DOWN};
    use cmf_wp2_ui::haptic::Motor;

    cmf_wp2_app::entry!(main);
    cmf_wp2_app::app_icon!();

    const ASK_MS: u32 = 3000;
    const AWAKE_MS: u32 = 20_000;
    const ANIM_MS: u32 = 80;
    static mut STORE: [u8; STORE_MAX] = [0; STORE_MAX];
    // ~3 KB: off the 8 KiB app stack
    static mut UI: Ui = Ui::new();

    fn after(now: u32, t: u32) -> bool {
        (now.wrapping_sub(t) as i32) >= 0
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
        let ui = unsafe { &mut *&raw mut UI };
        let buf = unsafe { &mut *&raw mut STORE };
        let n = api.load(buf);
        if n > 0 {
            ui.model.load(&buf[..n as usize]);
            ui.changed();
        }
        let mut screen = api;
        let mut motor = Motor::new(api);
        let mut want = true;
        let mut asked: Option<u32> = None;
        let mut awake: Option<u32> = None;
        let mut unsaved = false;
        let mut animated = 0u32;
        loop {
            let now = api.now();
            motor.update();
            if awake.is_some_and(|t| after(now, t)) {
                api.keep_screen_on(false);
                awake = None;
            }
            if ui.draw(&mut screen, now) {
                api.show();
                animated = now;
            } else if now.wrapping_sub(animated) >= ANIM_MS {
                if ui.animate(&mut screen, now) {
                    api.show();
                }
                animated = now;
            }
            let stale = ui.heard.is_none_or(|t| now.wrapping_sub(t) > crate::view::STALE_MS);
            if (want || stale) && asked.is_none_or(|t| now.wrapping_sub(t) >= ASK_MS) {
                api.send(REQUEST);
                asked = Some(now);
            }

            let wait = motor.wait().unwrap_or(ANIM_MS).min(ANIM_MS);
            match api.event(wait).map(|e| (e.kind, e.y, e.delta)) {
                Some((BUTTON, _, _)) => break,
                Some((PAUSE, _, _)) => {
                    motor.off();
                    if !paused(api) {
                        break;
                    }
                    want = true;
                    asked = None;
                    ui.redraw();
                }
                Some((RESUME, _, _)) => {
                    want = true;
                    ui.redraw();
                }
                Some((CROWN, _, delta)) if delta != 0 => {
                    ui.crown(delta);
                }
                Some((TOUCH_DOWN, y, _)) => {
                    if let Some(id) = ui.tap(y as i32, api.now()) {
                        api.send(&[b'h', b'1', b'F', id]);
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
                if fx.changed {
                    ui.changed();
                }
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
            if let Some(b) = buzz {
                motor.play(b.pattern());
                api.keep_screen_on(true);
                awake = Some(api.now().wrapping_add(AWAKE_MS));
            }
        }
        motor.off();
        if unsaved {
            save(api, &ui.model);
        }
        0
    }
}
