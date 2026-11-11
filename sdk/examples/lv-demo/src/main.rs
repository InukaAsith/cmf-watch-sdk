//! lv-demo: an ABI 1.2 app on the watch's own LVGL widgets. A title in a stock font, a crown-scrolled list of 12
//! rows (a tap plays the haptic pattern the roller picks), a slider that drives an arc; BUTTON exits.
//! Host messages for tests/test_lv_e2e.py: `!bad` submits a batch the loader must reject, `!spin` submits forever
//! and never reads BUTTON again (the loader force stops it).
#![cfg_attr(target_os = "none", no_std, no_main)]

#[cfg_attr(not(target_os = "none"), allow(dead_code))]
mod screen;

#[cfg(not(target_os = "none"))]
fn main() {}

#[cfg(target_os = "none")]
mod device {
    use crate::screen::{self, ARC_START};
    use cmf_wp2_app::lv::{Batch, LvError};
    use cmf_wp2_app::{Api, BUTTON, LV, MESSAGE, MSG_MAX, PAUSE};
    use cmf_wp2_ui::haptic::{ALERT, DONE, DOUBLE, Motor, Pattern, TICK};

    cmf_wp2_app::entry!(main);
    cmf_wp2_app::app_icon!();

    const CLICKED: i16 = 7; // LVGL 8.3 event codes
    const VALUE_CHANGED: i16 = 28;
    const PATTERNS: [Pattern; 4] = [TICK, DOUBLE, ALERT, DONE]; // the roller's options

    // read by tests/test_lv_e2e.py: submit results, the last LV event, handles
    const FONT_RC: usize = 0;
    const SCREEN_RC: usize = 1;
    const MSG_RC: usize = 2;
    const EVENTS: usize = 3;
    const LAST_H: usize = 4; // .. LAST_CODE 5, LAST_DELTA 6
    const ARC_VALUE: usize = 7;
    const SLIDER: usize = 8;
    const ARC: usize = 9;
    const BAR: usize = 10;
    const SPINS: usize = 11;
    const ROWS: usize = 12; // .. 12 row handles
    #[unsafe(no_mangle)]
    static mut STATS: [i32; ROWS + screen::ROWS] = [0; ROWS + screen::ROWS];

    fn set(i: usize, v: i32) {
        unsafe { (&raw mut STATS[i]).write_volatile(v) }
    }
    fn get(i: usize) -> i32 {
        unsafe { (&raw const STATS[i]).read_volatile() }
    }
    fn code(r: Result<(), LvError>) -> i32 {
        match r {
            Ok(()) => 0,
            Err(LvError::Code(c)) => c,
            Err(_) => -1000, // Full or Budget: the screen outgrew the batch
        }
    }

    fn main(api: &'static Api) -> i32 {
        let Some(lv) = api.lv() else { return -95 }; // loader older than ABI 1.2
        let Some(mut b) = Batch::new() else { return -1 };
        set(FONT_RC, code(screen::fonts(&mut b).and_then(|_| b.submit(&lv))));
        let s = match screen::build(&mut b) {
            Ok(s) => s,
            Err(e) => {
                set(SCREEN_RC, code(Err(e)));
                return 1;
            }
        };
        set(SCREEN_RC, code(b.submit(&lv)));
        set(ARC_VALUE, ARC_START);
        set(ARC, s.arc.0 as i32);
        set(BAR, s.bar.0 as i32);
        for (i, r) in s.rows.iter().enumerate() {
            set(ROWS + i, r.0 as i32);
        }
        set(SLIDER, s.slider.0 as i32); // last: the e2e waits for it

        let mut motor = Motor::new(api);
        let mut pattern = TICK;
        let mut msg = [0u8; MSG_MAX];
        loop {
            motor.update();
            let Some(e) = api.event(motor.wait().unwrap_or(u32::MAX).max(1)) else { continue };
            match e.kind {
                BUTTON => break,
                PAUSE => motor.off(),
                LV => {
                    set(EVENTS, get(EVENTS) + 1);
                    set(LAST_H, e.x.into());
                    set(LAST_H + 1, e.y.into());
                    set(LAST_H + 2, e.delta.into());
                    let h = e.x as u16;
                    if e.y == CLICKED && s.rows.iter().any(|r| r.0 == h) {
                        motor.play(pattern);
                    } else if e.y == VALUE_CHANGED && h == s.roller.0 {
                        pattern = PATTERNS[(e.delta as usize).min(PATTERNS.len() - 1)];
                    } else if e.y == VALUE_CHANGED && h == s.slider.0 {
                        // events coalesce: delta is the slider's latest value
                        let v = (e.delta as i32).clamp(0, 100);
                        if code(screen::set_arc(&mut b, &s, v).and_then(|_| b.submit(&lv))) == 0 {
                            set(ARC_VALUE, v);
                        }
                    }
                }
                MESSAGE => {
                    while let Some(m) = api.recv(&mut msg) {
                        match m {
                            b"!bad" => set(MSG_RC, code(screen::bad(&mut b, &s).and_then(|_| b.submit(&lv)))),
                            b"!spin" => {
                                motor.off();
                                let mut v = 0;
                                loop {
                                    v = (v + 1) % 101;
                                    let _ = screen::set_arc(&mut b, &s, v).and_then(|_| b.submit(&lv));
                                    set(SPINS, get(SPINS) + 1);
                                }
                            }
                            _ => {}
                        }
                    }
                }
                _ => {}
            }
        }
        motor.off();
        0
    }
}
