//! lv-demo: an ABI 1.2 app on the watch's own LVGL widgets in the stock look (`cmf_wp2_app::stock`): a
//! crown-scrolled page with the title, a slider that drives an arc, and a row per haptic pattern (a tap plays it);
//! BUTTON exits.
//! Host messages for tests/test_lv_e2e.py: `!bad` submits a batch the loader must reject, `!del` deletes the list
//! (rows, labels and scroll bar go with it), `!list` builds it again, `!spin` submits forever and never reads BUTTON
//! again (the loader force stops it).
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
    const PATTERNS: [Pattern; screen::ROWS] = [TICK, DOUBLE, ALERT, DONE]; // screen::NAMES

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
    const LIST: usize = 12;
    const MSGS: usize = 13; // !bad, !del and !list handled; MSG_RC holds the last one's result
    const ROWS: usize = 14; // .. 4 row handles
    // FONT_RC and SCREEN_RC start at NOT_YET (no lv_submit code), so the e2e sees whether the submits returned;
    // the non-zero initialiser puts STATS in .data, copied fresh at every launch
    const NOT_YET: i32 = 1;
    #[unsafe(no_mangle)]
    static mut STATS: [i32; ROWS + screen::ROWS] = {
        let mut s = [0; ROWS + screen::ROWS];
        (s[FONT_RC], s[SCREEN_RC]) = (NOT_YET, NOT_YET);
        s
    };

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

    fn publish(s: &screen::Screen) {
        set(LIST, s.list.0 as i32);
        set(BAR, s.bar.0 as i32);
        for (i, r) in s.rows.iter().enumerate() {
            set(ROWS + i, r.0 as i32);
        }
    }

    fn main(api: &'static Api) -> i32 {
        let Some(lv) = api.lv() else { return -95 }; // loader older than ABI 1.2
        let Some(mut b) = Batch::new() else { return -1 };
        set(FONT_RC, code(screen::fonts(&mut b).and_then(|_| b.submit(&lv))));
        let mut s = match screen::build(&mut b) {
            Ok(s) => s,
            Err(e) => {
                set(SCREEN_RC, code(Err(e)));
                return 1;
            }
        };
        set(SCREEN_RC, code(b.submit(&lv).and_then(|_| screen::list(&mut b, &mut s)).and_then(|_| b.submit(&lv))));
        set(ARC_VALUE, ARC_START);
        set(ARC, s.arc.0 as i32);
        publish(&s);
        set(SLIDER, s.slider.0 as i32); // last: the e2e waits for it

        let mut motor = Motor::new(api);
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
                    if let Some(i) = s.rows.iter().position(|r| r.0 == h).filter(|_| e.y == CLICKED) {
                        motor.play(PATTERNS[i]);
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
                            b"!bad" | b"!del" | b"!list" => {
                                let r = match m {
                                    b"!bad" => screen::bad(&mut b, &s),
                                    b"!del" => screen::del_list(&mut b, &s),
                                    _ => screen::list(&mut b, &mut s),
                                };
                                set(MSG_RC, code(r.and_then(|_| b.submit(&lv))));
                                publish(&s);
                                set(MSGS, get(MSGS) + 1);
                            }
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
