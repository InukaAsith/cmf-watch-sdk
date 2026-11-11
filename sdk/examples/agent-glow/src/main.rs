#![cfg_attr(any(target_os = "none", wp2_sim), no_main, cfg_attr(target_os = "none", no_std))]

mod glow;

#[cfg(not(any(target_os = "none", wp2_sim)))]
fn main() {}

#[cfg(any(target_os = "none", wp2_sim))]
mod device {
    use crate::glow::{App, SCENES, Surface};
    use cmf_wp2_app::{Api, BUTTON, CROWN, PAUSE, RESUME, TOUCH_DOWN};
    use core::sync::atomic::{AtomicU32, Ordering};

    cmf_wp2_app::entry!(main);
    cmf_wp2_app::app_icon!();

    const FRAME_MS: u32 = 33;
    // a light tick: one short pulse at the stock 31 Hz PWM, far inside the 2 s cap
    const TICK: (u16, u16, u32) = (250, 31, 20);

    // per scene: frames, draw ms, frame period ms, rect calls (sums); read by tests/test_demo_e2e.py
    #[unsafe(no_mangle)]
    static STATS: [AtomicU32; SCENES * 4] = [const { AtomicU32::new(0) }; SCENES * 4];

    struct Screen(&'static Api, u32);

    impl Surface for Screen {
        fn rect(&mut self, x: i32, y: i32, w: i32, h: i32, c: u16) {
            if w > 0 && h > 0 {
                self.0.rect(x as i16, y as i16, w as u16, h as u16, c);
                self.1 += 1;
            }
        }

        fn text(&mut self, x: i32, y: i32, s: &str, c: u16) {
            self.0.text(x as i16, y as i16, s, c);
        }
    }

    fn before(a: u32, b: u32) -> bool {
        (a.wrapping_sub(b) as i32) < 0
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
        let mut app = App::new();
        let mut screen = Screen(api, 0);
        let mut buzz: Option<u32> = None;
        let mut last: Option<u32> = None;
        loop {
            let t0 = api.now();
            screen.1 = 0;
            app.frame(t0, &mut screen);
            api.show();
            let s = app.scene() * 4;
            STATS[s].fetch_add(1, Ordering::Relaxed);
            STATS[s + 1].fetch_add(api.now().wrapping_sub(t0), Ordering::Relaxed);
            STATS[s + 3].fetch_add(screen.1, Ordering::Relaxed);
            if let Some(l) = last {
                STATS[s + 2].fetch_add(t0.wrapping_sub(l), Ordering::Relaxed);
            }
            last = Some(t0);
            let next = t0.wrapping_add(FRAME_MS);
            loop {
                let until = buzz.filter(|&off| before(off, next)).unwrap_or(next);
                let left = (until.wrapping_sub(api.now()) as i32).max(1) as u32;
                match api.event(left) {
                    None => {
                        if buzz.is_some_and(|off| !before(api.now(), off)) {
                            api.vibrate(0, TICK.1);
                            buzz = None;
                        }
                        if !before(api.now(), next) {
                            break;
                        }
                    }
                    Some(e) => match e.kind {
                        BUTTON => {
                            api.vibrate(0, TICK.1);
                            return 0;
                        }
                        PAUSE => {
                            api.vibrate(0, TICK.1);
                            buzz = None;
                            if !paused(api) {
                                return 0;
                            }
                            app.redraw();
                            last = None;
                            break;
                        }
                        RESUME => {
                            app.redraw();
                            break;
                        }
                        CROWN if e.delta != 0 => {
                            app.crown(e.delta);
                            last = None;
                        }
                        TOUCH_DOWN => {
                            let now = api.now();
                            app.tap(now);
                            if buzz.is_none() {
                                if api.vibrate(TICK.0, TICK.1) == 0 {
                                    buzz = Some(now.wrapping_add(TICK.2));
                                } else {
                                    api.vibrate(0, TICK.1); // clears a -4 lock
                                }
                            }
                        }
                        _ => {}
                    },
                }
            }
        }
    }
}
