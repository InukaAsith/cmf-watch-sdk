//! workout: the owner's training program on the wrist (docs/workout.md). Days, sets, a rest timer that buzzes, the
//! day rotation and a history, fed by `wp2apps workout push` over the "w1" protocol. ABI 1.3: the app owns the
//! button ("back") and asks before it leaves a workout.
#![cfg_attr(target_os = "none", no_std, no_main)]

#[cfg_attr(not(target_os = "none"), allow(dead_code))]
mod model;
#[cfg_attr(not(target_os = "none"), allow(dead_code))]
mod proto;
#[cfg_attr(not(target_os = "none"), allow(dead_code))]
mod screen;

#[cfg(not(target_os = "none"))]
fn main() {}

#[cfg(target_os = "none")]
mod device {
    use crate::model::{epoch_s, Model, Next, Program};
    use crate::proto::{ack, history_msg, request, Got, Transfer};
    use crate::screen::{Hit, Screen, View};
    use cmf_wp2_app::lv::{Batch, LvError};
    use cmf_wp2_app::{Api, BUTTON, LV, MSG_MAX, PAUSE, RESUME, STORE_MAX};
    use cmf_wp2_ui::haptic::{Motor, ALERT};

    cmf_wp2_app::entry!(main);
    cmf_wp2_app::app_icon!();

    const CLICKED: i16 = 7;
    const ASK_MS: u32 = 3000;
    const AWAKE_MS: u32 = 20_000;
    const TICK_MS: u32 = 200;
    // taps this soon after a view change are dropped (a double tap on "Сделал" must not also hit "Пропустить")
    const SETTLE_MS: u32 = 600;
    // a tap is acted on this late, and dropped if a BUTTON (the back swipe) comes within GESTURE_MS of it: a
    // swipe across a pill is also a tap on it
    const DEFER_MS: u32 = 150;
    const GESTURE_MS: u32 = 400;

    static mut STORE: [u8; STORE_MAX] = [0; STORE_MAX];
    static mut MODEL: Model = Model::new();
    static mut TRANSFER: Transfer = Transfer::new();
    static mut SCREEN: Screen = Screen::empty();

    // read by tests/test_workout_e2e.py: view (0 start, 1 day, 2 set, 3 rest, 4 ask, 5 done), sets done in the
    // workout, buzzes, last acked version, the next day of the rotation; non-zero initialiser: .data, fresh at every
    // launch
    const VIEW: usize = 0;
    const SETS: usize = 1;
    const BUZZES: usize = 2;
    const ACKED: usize = 3;
    const NEXT: usize = 4;
    #[unsafe(no_mangle)]
    static mut STATS: [i32; 5] = [-1, 0, 0, 0, 0];

    fn set(i: usize, v: i32) {
        unsafe { (&raw mut STATS[i]).write_volatile(v) }
    }
    fn get(i: usize) -> i32 {
        unsafe { (&raw const STATS[i]).read_volatile() }
    }

    /// Wall-clock seconds since 2000, or boot seconds without a clock (a stored rest deadline is then off after a
    /// reboot; the next set still works).
    fn now_s(api: &Api) -> u32 {
        api.time().map_or(api.now() / 1000, |t| epoch_s(&t))
    }

    fn save(api: &Api, m: &Model) {
        let buf = unsafe { &mut *&raw mut STORE };
        let n = m.save(buf);
        api.save(&buf[..n]);
    }

    fn version(m: &Model) -> u32 {
        m.program.as_ref().map_or(0, Program::version)
    }

    fn sets_done(m: &Model) -> i32 {
        m.workout.map_or(0, |w| w.done.iter().map(|&d| i32::from(d)).sum())
    }

    fn view_code(v: View) -> i32 {
        match v {
            View::Start => 0,
            View::Day => 1,
            View::Set { .. } => 2,
            View::Rest => 3,
            View::Ask => 4,
            View::Done => 5,
        }
    }

    /// Where an open workout continues: its rest if it still runs, else the next set, else the day list.
    fn resume_view(m: &Model, now: u32) -> View {
        let Some(w) = m.workout else { return View::Start };
        if w.rest_ex == 0xFF {
            View::Day
        } else if m.rest_left(now) > 0 {
            View::Rest
        } else {
            match m.after_rest() {
                Next::Set { ex, .. } => View::Set { ex },
                Next::Finished => View::Day,
            }
        }
    }

    /// Ends the workout: the summary if it had sets (it went to the history), else the start screen.
    fn finish(api: &Api, m: &mut Model, now: u32) -> View {
        let had = sets_done(m) > 0;
        m.finish(now);
        save(api, m);
        api.send(&request(version(m), false)); // a host holding a program back sends it now
        if had { View::Done } else { View::Start }
    }

    /// The view after a rest: the next set, or (all sets done) the finished workout.
    fn after_rest(api: &Api, m: &mut Model, now: u32) -> View {
        match m.after_rest() {
            Next::Set { ex, .. } => View::Set { ex },
            Next::Finished => finish(api, m, now),
        }
    }

    fn main(api: &'static Api) -> i32 {
        let Some(lv) = api.lv() else { return -95 }; // loader older than ABI 1.2
        let Some(mut b) = Batch::new() else { return -1 };
        api.own_back(true);
        let m = unsafe { &mut *&raw mut MODEL };
        let t = unsafe { &mut *&raw mut TRANSFER };
        let s = unsafe { &mut *&raw mut SCREEN };
        let buf = unsafe { &mut *&raw mut STORE };
        let n = api.load(buf);
        m.load(if n > 0 { &buf[..n as usize] } else { &[] });
        let now = now_s(api);
        let had = m.workout.is_some();
        m.expire(now);
        if had && m.workout.is_none() {
            save(api, m);
        }
        if Screen::fonts(&mut b).and_then(|_| b.submit(&lv)).is_err() {
            return 1;
        }
        let go = |s: &mut Screen, b: &mut Batch, m: &Model, v: View, now: u32| -> Result<(), LvError> {
            set(VIEW, view_code(v));
            set(NEXT, m.next_day() as i32);
            s.show(b, &mut |b| b.submit(&lv), m, v, now)
        };
        if Screen::theme(&mut b).and_then(|_| go(s, &mut b, m, resume_view(m, now), now)).is_err() {
            return 2;
        }
        set(SETS, sets_done(m));

        let mut motor = Motor::new(api);
        let mut asked: Option<u32> = None;
        let mut awake: Option<u32> = None;
        // next history entry to send (Q): the outbox holds 4, the host collects them as it polls
        let mut hist: Option<u8> = None;
        // the wake lock: on during a rest and AWAKE_MS after it, off anywhere else
        let mut locked = false;
        let mut shown_at = api.now();
        let mut button_at = api.now().wrapping_sub(GESTURE_MS);
        let mut pending: Option<(Hit, u32)> = None;
        loop {
            let ms = api.now();
            let now = now_s(api);
            motor.update();
            if awake.is_some_and(|a| ms.wrapping_sub(a) as i32 >= 0) {
                awake = None;
            }
            let want = s.view == View::Rest || awake.is_some();
            if want != locked {
                api.keep_screen_on(want);
                locked = want;
            }
            // R on start, then every 3 s on the start screen (push and history wait for it)
            if asked.is_none_or(|a| s.view == View::Start && ms.wrapping_sub(a) >= ASK_MS) {
                api.send(&request(version(m), m.workout.is_some()));
                asked = Some(ms);
            }
            // rest over: buzz, then the next set
            if s.view == View::Rest && m.rest_left(now) == 0 {
                motor.play(ALERT);
                set(BUZZES, get(BUZZES) + 1);
                awake = Some(ms.wrapping_add(AWAKE_MS));
                let v = after_rest(api, m, now);
                let _ = go(s, &mut b, m, v, now);
                shown_at = api.now();
            }
            let _ = s.tick(&mut b, &mut |b| b.submit(&lv), m, now);

            while let Some(i) = hist {
                let n = m.history().count() as u8;
                let e = m.history().nth(usize::from(i)).unwrap_or_default();
                let rc = api.send(&history_msg(i, n, &e));
                if rc == -11 {
                    break; // full: next turn
                }
                hist = (rc == 0 && i + 1 < n).then_some(i + 1);
            }
            let busy = hist.is_some() || pending.is_some();
            let wait = motor.wait().unwrap_or(TICK_MS).min(if busy { 50 } else { TICK_MS });
            let mut to: Option<View> = None;
            let mut hit = None;
            match api.event(wait) {
                Some(e) if e.kind == BUTTON => {
                    button_at = api.now();
                    pending = None;
                    match s.view {
                        View::Set { .. } | View::Rest | View::Ask => to = Some(View::Day),
                        View::Day => to = Some(View::Ask),
                        View::Start | View::Done => break,
                    }
                }
                Some(e) if e.kind == PAUSE => {
                    motor.off();
                    while !api.event(u32::MAX).is_some_and(|e| e.kind == RESUME) {}
                    asked = None;
                    // a rest that ended meanwhile: the next set, no late buzz
                    to = Some(resume_view(m, now_s(api)));
                }
                Some(e) if e.kind == RESUME => to = Some(s.view),
                Some(e) if e.kind == LV && e.y == CLICKED => {
                    let t = api.now();
                    let settled = t.wrapping_sub(shown_at) >= SETTLE_MS && t.wrapping_sub(button_at) >= GESTURE_MS;
                    if let Some(h) = s.hit_of(e.x as u16).filter(|_| settled) {
                        pending = Some((h, t));
                    }
                }
                _ => {}
            }
            if pending.is_some_and(|(_, t)| api.now().wrapping_sub(t) >= DEFER_MS) {
                hit = pending.take().map(|(h, _)| h);
            }
            match hit {
                    Some(Hit::Day(d)) => {
                        m.start(d, now);
                        save(api, m);
                        to = Some(View::Day);
                    }
                    Some(Hit::Exercise(i)) => {
                        let open = matches!((&m.program, m.workout),
                            (Some(p), Some(w)) if w.done[i] < p.exercise(usize::from(w.day), i).sets);
                        if open {
                            to = Some(View::Set { ex: i });
                        }
                    }
                    Some(Hit::Done) => {
                        if let View::Set { ex } = s.view {
                            m.done_set(ex, now);
                            save(api, m);
                            set(SETS, sets_done(m));
                            awake = None;
                            to = Some(View::Rest);
                        }
                    }
                    Some(Hit::Skip) => {
                        awake = Some(ms.wrapping_add(AWAKE_MS));
                        to = Some(after_rest(api, m, now));
                    }
                    Some(Hit::Finish) | Some(Hit::End) => to = Some(finish(api, m, now)),
                    Some(Hit::Leave) => break,
                    Some(Hit::Cancel) => to = Some(View::Day),
                    None => {}
            }

            let mut msg = [0u8; MSG_MAX];
            while let Some(data) = api.recv(&mut msg) {
                match t.apply(data) {
                    Got::Program(bytes) => {
                        if let Some(p) = Program::parse(bytes) {
                            let v = p.version();
                            m.replace_program(p, now_s(api));
                            save(api, m);
                            api.send(&ack(v));
                            set(ACKED, v as i32);
                            set(SETS, 0);
                            to = Some(View::Start);
                        }
                    }
                    Got::History => hist = Some(0), // an empty history: one Y with count 0
                    Got::Resync => {
                        api.send(&request(version(m), m.workout.is_some()));
                    }
                    Got::Nothing => {}
                }
            }
            if let Some(v) = to {
                let _ = go(s, &mut b, m, v, now_s(api));
                shown_at = api.now();
                pending = None;
            }
        }
        motor.off();
        api.keep_screen_on(false);
        0
    }
}
