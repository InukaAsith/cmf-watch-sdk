#![cfg_attr(any(target_os = "none", wp2_sim), no_main, cfg_attr(target_os = "none", no_std))]

#[cfg_attr(not(any(target_os = "none", wp2_sim)), allow(dead_code))]
mod gallery;

#[cfg(not(any(target_os = "none", wp2_sim)))]
fn main() {}

#[cfg(any(target_os = "none", wp2_sim))]
mod device {
    use crate::gallery::{App, PAGES};
    use cmf_wp2_app::{Api, BUTTON, CROWN, PAUSE, RESUME, TOUCH_DOWN};
    use cmf_wp2_ui::haptic::Motor;
    use core::sync::atomic::{AtomicU32, Ordering};

    cmf_wp2_app::entry!(main);
    cmf_wp2_app::app_icon!();

    // per page: frames drawn, draw ms; read by tests/test_uikit_e2e.py
    #[unsafe(no_mangle)]
    static STATS: [AtomicU32; PAGES * 2] = [const { AtomicU32::new(0) }; PAGES * 2];

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
        let mut app = App::new();
        let mut screen = api;
        let mut motor = Motor::new(api);
        loop {
            motor.update();
            let t0 = api.now();
            if app.frame(&mut screen, t0) {
                api.show();
                let p = app.page() * 2;
                STATS[p].fetch_add(1, Ordering::Relaxed);
                STATS[p + 1].fetch_add(api.now().wrapping_sub(t0), Ordering::Relaxed);
            }
            let now = api.now();
            let wait = app.wait(now).min(motor.wait().unwrap_or(u32::MAX)).max(1);
            let Some(e) = api.event(wait) else { continue };
            match e.kind {
                BUTTON => break,
                PAUSE => {
                    motor.off();
                    if !paused(api) {
                        break;
                    }
                    app.redraw();
                }
                RESUME => app.redraw(),
                CROWN if e.delta != 0 => app.crown(e.delta, api.now()),
                TOUCH_DOWN => {
                    if let Some(p) = app.tap(e.x as i32, e.y as i32, api.now()) {
                        motor.play(p);
                    }
                }
                _ => {}
            }
        }
        motor.off();
        0
    }
}
