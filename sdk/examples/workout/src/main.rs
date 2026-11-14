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
    use crate::model::{epoch_s, Model, Next, Program, HR_MAX, SETS_MAX};
    use crate::proto::{ack, history_msg, pulse, request, trace_msg, Got, Transfer, X_CHUNK};
    use crate::screen::{Hit, Screen, View};
    use cmf_wp2_app::lv::{Batch, LvError};
    use cmf_wp2_app::stock::PAGE_CHANGED;
    use cmf_wp2_app::{Api, BUTTON, HEART, LV, MSG_MAX, PAUSE, RESUME, STORE_MAX};
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
    const WAKE_KEY_MS: u32 = 500;
    /// A P to the phone at least this often during a workout (sooner when the exercise or the sets change).
    const PULSE_MS: u32 = 5000;
    const TRACE_MAX: usize = 1 + SETS_MAX * 3 + 2 + HR_MAX;

    static mut STORE: [u8; STORE_MAX] = [0; STORE_MAX];
    static mut MODEL: Model = Model::new();
    static mut TRANSFER: Transfer = Transfer::new();
    static mut SCREEN: Screen = Screen::empty();
    static mut TRACE: [u8; TRACE_MAX] = [0; TRACE_MAX];

    // read by tests/test_workout_e2e.py: view (0 start, 1 day, 2 set, 3 rest, 4 ask, 5 done), sets done in the
    // workout, buzzes, last acked version, the next day of the rotation, the exercise on screen (0xFF none);
    // non-zero initialiser: .data, fresh at every launch
    const VIEW: usize = 0;
    const SETS: usize = 1;
    const BUZZES: usize = 2;
    const ACKED: usize = 3;
    const NEXT: usize = 4;
    const EX: usize = 5;
    #[unsafe(no_mangle)]
    static mut STATS: [i32; 6] = [-1, 0, 0, 0, 0, 0];

    fn set(i: usize, v: i32) {
        unsafe { (&raw mut STATS[i]).write_volatile(v) }
    }
    fn get(i: usize) -> i32 {
        unsafe { (&raw const STATS[i]).read_volatile() }
    }

    // (wall-clock s, millis) read once per launch
    static mut CLOCK: Option<(u32, u32)> = None;

    /// Wall-clock seconds since 2000 (boot seconds without a clock: a stored rest deadline is then off after a
    /// reboot; the next set still works). The RTC is read once per launch and then advanced by `now()`: a phone
    /// time sync does not jump a running rest, and the emulator's slow RTC (docs/recon.md) does not stretch it.
    fn now_s(api: &Api) -> u32 {
        let ms = api.now();
        let (s, at) = *unsafe { &mut *&raw mut CLOCK }
            .get_or_insert_with(|| (api.time().map_or(ms / 1000, |t| epoch_s(&t)), ms));
        s.wrapping_add(ms.wrapping_sub(at) / 1000)
    }

    // not inlined: each copy would add Model::save's 768 B history buffer to main's frame (8 KiB stack)
    #[inline(never)]
    fn save(api: &Api, m: &Model) {
        let buf = unsafe { &mut *&raw mut STORE };
        let n = m.save(buf);
        api.save(&buf[..n]);
    }

    /// The exercise on screen (set or rest), 0xFF otherwise.
    fn open_ex(v: View, m: &Model) -> u8 {
        match v {
            View::Set { ex } => ex as u8,
            View::Rest => m.workout.map_or(0xFF, |w| w.rest_ex),
            _ => 0xFF,
        }
    }

    // not inlined: its 2 KB Program temporaries stay out of main's frame (8 KiB stack, no guard)
    #[inline(never)]
    fn take_program(api: &Api, m: &mut Model, bytes: &[u8]) -> bool {
        let Some(v) = m.take_program(bytes, now_s(api)) else { return false };
        save(api, m);
        api.send(&ack(v));
        set(ACKED, v as i32);
        set(SETS, 0);
        true
    }

    static mut BATCH: Option<Batch> = None;

    // the 2 KB Batch lives in a static and is built here, so neither it nor its temporary sits in main's frame
    #[inline(never)]
    fn batch() -> Option<&'static mut Batch> {
        let slot = unsafe { &mut *&raw mut BATCH };
        *slot = Some(Batch::new()?);
        slot.as_mut()
    }

    fn version(m: &Model) -> u32 {
        m.program.as_ref().map_or(0, Program::version)
    }

    /// P's heart-rate diagnostics for the phone (the measurement is unproven on hardware): heart_rate()'s result,
    /// HEART events seen, and the stock cells the loader reads (wp2_core::stock HR_WEAR, HR_OWNER, HR_BPM; JX402
    /// 1.0.0.73). Read only.
    fn hr_diag(rc: i8, events: u8, ui: [u8; 8]) -> [u8; 13] {
        let cell = |a: u32| unsafe { (a as *const u8).read_volatile() };
        let mut d = [0; 13];
        d[..5].copy_from_slice(&[rc as u8, events, cell(0x3804_B30C), cell(0x3804_B30F), cell(0x3804_B310)]);
        d[5..].copy_from_slice(&ui);
        d
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
            View::Preview { .. } => 6,
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
        let Some(b) = batch() else { return -1 };
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
        if Screen::fonts(b).and_then(|_| b.submit(&lv)).is_err() {
            return 1;
        }
        let go = |s: &mut Screen, b: &mut Batch, m: &Model, v: View, now: u32| -> Result<(), LvError> {
            set(VIEW, view_code(v));
            set(NEXT, m.next_day() as i32);
            s.show(b, &mut |b| b.submit(&lv), m, v, now)
        };
        if Screen::theme(b).and_then(|_| go(s, b, m, resume_view(m, now), now)).is_err() {
            return 2;
        }
        set(SETS, sets_done(m));

        let mut motor = Motor::new(api);
        let mut asked: Option<u32> = None;
        let mut awake: Option<u32> = None;
        // next history entry to send (Q): the outbox holds 4, the host collects them as it polls
        let mut hist: Option<u8> = None;
        // X messages left: (next index, count, trace bytes)
        let mut trace: Option<(u8, u8, usize)> = None;
        let mut pulsed: Option<(u32, u8, u8, bool)> = None; // (ms, ex, done, resting) of the last P
        let mut hr_on = false;
        let (mut hr_rc, mut hr_events) = (0i8, 0u8); // for P's diagnostics // the measurement runs while a workout does (docs/abi-1.4.md, Heart rate)
        // the wake lock: on during a rest and AWAKE_MS after it, off anywhere else
        let mut locked = false;
        let mut shown_at = api.now();
        let mut resumed_at = api.now().wrapping_sub(WAKE_KEY_MS);
        let mut button_at = api.now().wrapping_sub(GESTURE_MS);
        let mut pending: Option<(Hit, u32)> = None;
        // P's UI diagnostics (the set screen leaves for the day list on hardware, not in the emulator):
        // BUTTONs, PAUSEs, RESUMEs, set-screen leaves, the cause of the last leave (1 button, 2 pause, 3 resume,
        // 4 phone, 5 tap, 0 other), ms from the last RESUME to the last BUTTON (u16), minutes since the app started
        let t0 = api.now();
        let mut ui = [0u8; 8];
        loop {
            let mut cause = 0u8;
            ui[7] = (api.now().wrapping_sub(t0) / 60_000).min(255) as u8;
            let ms = api.now();
            let now = now_s(api);
            motor.update();
            set(EX, i32::from(open_ex(s.view, m)));
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
                let _ = go(s, b, m, v, now);
                shown_at = api.now();
            }
            if m.workout.is_some() != hr_on {
                hr_on = !hr_on;
                hr_rc = api.heart_rate(hr_on) as i8; // -22 on an older loader: no heart rate, the rest works
                m.bpm = 0;
            }
            // saved with the next set or the finish, not on its own: a 4 KB save can hold the loop past the loader's
            // 1 s BUTTON deadline; a crash loses the samples since the last set
            m.hr_sample(m.bpm, now);
            if let Some(w) = m.workout {
                let (ex, done) = (open_ex(s.view, m), sets_done(m) as u8);
                let ex_done = w.done.get(usize::from(ex)).copied().unwrap_or(0);
                let rest = if s.view == View::Rest { (w.rest_until.wrapping_sub(now) as i32).clamp(0, 0xFFFF) as u16 } else { 0 };
                if pulsed.is_none_or(|(t, e, d, r)| ms.wrapping_sub(t) >= PULSE_MS || (e, d, r) != (ex, done, rest > 0)) {
                    api.send(&pulse(w.started, w.day, m.bpm, ex, done, ex_done, rest, hr_diag(hr_rc, hr_events, ui)));
                    pulsed = Some((ms, ex, done, rest > 0));
                }
            }
            let _ = s.tick(b, &mut |b| b.submit(&lv), m, now);

            while let Some(i) = hist {
                let n = m.history().count() as u8;
                let e = m.history().nth(usize::from(i)).unwrap_or_default();
                let rc = api.send(&history_msg(i, n, &e));
                if rc == -11 {
                    break; // full: next turn
                }
                hist = (rc == 0 && i + 1 < n).then_some(i + 1);
            }
            while let Some((i, n, len)) = trace {
                let tb = unsafe { &*&raw const TRACE };
                let chunk = &tb[(usize::from(i) * X_CHUNK).min(len)..((usize::from(i) + 1) * X_CHUNK).min(len)];
                let mut out = [0u8; 9 + X_CHUNK];
                let k = trace_msg(i, n, m.trace.start, chunk, &mut out);
                let rc = api.send(&out[..k]);
                if rc == -11 {
                    break;
                }
                trace = (rc == 0 && i + 1 < n).then_some((i + 1, n, len));
            }
            let busy = hist.is_some() || trace.is_some() || pending.is_some();
            let wait = motor.wait().unwrap_or(TICK_MS).min(if busy { 50 } else { TICK_MS });
            let mut to: Option<View> = None;
            let mut hit = None;
            match api.event(wait) {
                // the press that ends AOD can follow its RESUME: not a "back"
                Some(e) if e.kind == BUTTON && api.now().wrapping_sub(resumed_at) < WAKE_KEY_MS => {}
                Some(e) if e.kind == BUTTON => {
                    button_at = api.now();
                    cause = 1;
                    ui[0] = ui[0].wrapping_add(1);
                    let gap = button_at.wrapping_sub(resumed_at).min(0xFFFF) as u16;
                    ui[5..7].copy_from_slice(&gap.to_le_bytes());
                    pending = None;
                    match s.view {
                        View::Set { .. } | View::Rest | View::Ask => to = Some(View::Day),
                        View::Day => to = Some(View::Ask),
                        View::Preview { .. } => to = Some(View::Start),
                        View::Start | View::Done => break,
                    }
                }
                Some(e) if e.kind == PAUSE => {
                    cause = 2;
                    ui[1] = ui[1].wrapping_add(1);
                    ui[2] = ui[2].wrapping_add(1); // its RESUME is awaited here
                    motor.off();
                    while !api.event(u32::MAX).is_some_and(|e| e.kind == RESUME) {}
                    resumed_at = api.now();
                    asked = None;
                    // a rest that ended meanwhile: the next set, no late buzz; any other view stays (AOD pauses too)
                    to = Some(if s.view == View::Rest { resume_view(m, now_s(api)) } else { s.view });
                }
                Some(e) if e.kind == RESUME => {
                    cause = 3;
                    ui[2] = ui[2].wrapping_add(1);
                    resumed_at = api.now();
                    to = Some(s.view);
                }
                // x: worn; delta: bpm, 0 = no reading yet
                Some(e) if e.kind == HEART => {
                    hr_events = hr_events.wrapping_add(1);
                    m.bpm = if e.x == 1 { e.delta.clamp(0, 255) as u8 } else { 0 };
                }
                // the set view's pager settled on another exercise: it is the open one now (P follows)
                Some(e) if e.kind == LV && e.y == PAGE_CHANGED => {
                    if s.paged(b, &e).unwrap_or(false) {
                        let _ = b.submit(&lv);
                        pending = None;
                    }
                }
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
                    Some(Hit::Day(d)) => to = Some(View::Preview { day: d }),
                    Some(Hit::Begin) => {
                        if let View::Preview { day } = s.view {
                            m.start(day, now);
                            save(api, m);
                            to = Some(View::Day);
                        }
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
                    Some(Hit::Leave) => {
                        save(api, m); // keeps the heart-rate samples since the last set
                        break;
                    }
                    Some(Hit::Cancel) => to = Some(View::Day),
                    None => {}
            }

            let mut msg = [0u8; MSG_MAX];
            while let Some(data) = api.recv(&mut msg) {
                cause = 4;
                match t.apply(data) {
                    Got::Program(bytes) => {
                        if take_program(api, m, bytes) {
                            to = Some(View::Start);
                        }
                    }
                    Got::History => hist = Some(0), // an empty history: one Y with count 0
                    Got::Trace => {
                        let len = m.trace.encode(unsafe { &mut *&raw mut TRACE });
                        trace = Some((0, len.div_ceil(X_CHUNK) as u8, len));
                    }
                    Got::Set { start, day, version: v, ex, t: at } => {
                        if v == version(m) && m.merge_set(start, day, ex, at, now) {
                            save(api, m);
                            set(SETS, sets_done(m));
                            // the phone's set on this exercise (or during this rest) moves the watch on as its own would
                            to = match s.view {
                                View::Set { ex: e } if e == usize::from(ex) => Some(resume_view(m, now)),
                                View::Rest => Some(resume_view(m, now)),
                                View::Start | View::Done | View::Preview { .. } => Some(View::Day),
                                View::Ask => to,
                                v => Some(v),
                            };
                            if to == Some(View::Rest) {
                                awake = None;
                            }
                        }
                    }
                    Got::Zones(z) => {
                        if m.zones != z {
                            m.zones = z;
                            save(api, m);
                            to = Some(s.view); // the heart takes its zone colour
                        }
                    }
                    Got::Unset { start, ex, t: at } => {
                        if m.remove_set(start, ex, at) {
                            save(api, m);
                            set(SETS, sets_done(m));
                            to = match s.view {
                                View::Rest => Some(resume_view(m, now)),
                                View::Ask => to,
                                v => Some(v),
                            };
                        }
                    }
                    Got::Finish { start, end } => {
                        let had = sets_done(m) > 0;
                        if m.finish_at(start, end) {
                            save(api, m);
                            set(SETS, 0);
                            api.send(&request(version(m), false));
                            to = Some(if had { View::Done } else { View::Start });
                        }
                    }
                    Got::Entry(e) => {
                        if m.add_entry(e) {
                            save(api, m);
                            if s.view == View::Start {
                                to = Some(View::Start); // the next day may have moved
                            }
                        }
                    }
                    Got::Resync => {
                        api.send(&request(version(m), m.workout.is_some()));
                    }
                    Got::Nothing => {}
                }
            }
            if let Some(v) = to {
                if matches!(s.view, View::Set { .. }) && !matches!(v, View::Set { .. } | View::Rest) {
                    ui[3] = ui[3].wrapping_add(1);
                    ui[4] = if cause == 0 && hit.is_some() { 5 } else { cause };
                }
                let _ = go(s, b, m, v, now_s(api));
                shown_at = api.now();
                pending = None;
            }
        }
        motor.off();
        api.keep_screen_on(false);
        0
    }
}
