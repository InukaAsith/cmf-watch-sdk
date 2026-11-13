//! Heart rate for apps (docs/abi-1.4.md, "Heart rate"): when the loader starts, keeps and stops the stock
//! measurement and when it hands the app a HEART event. The loader does the stock calls (docs/recon.md).
use crate::events::{Event, HEART};

/// One HEART event per this many ms while the app holds the measurement.
pub const EMIT_MS: u32 = 1000;
/// SENSOR record value for the heart rate (lvcmd::sensor).
pub const SENSOR_HEART: u8 = 1;

/// The stock counts start and stop requests per algorithm (0x38048D16 + 5, docs/recon.md): every start needs
/// exactly one stop, so the loader starts once per hold and never repeats it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Step {
    /// clear the stock reading, then start the measurement
    pub start: bool,
    pub stop: bool,
    /// queue a HEART event
    pub emit: bool,
}

#[derive(Default)]
pub struct Heart {
    on: bool,
    held: bool,
    emit_at: u32,
}

impl Heart {
    pub const fn new() -> Heart {
        Heart { on: false, held: false, emit_at: 0 }
    }

    /// UI tick. `want`: the app asked for it and runs in the foreground (not paused). `stock_owns`: the stock's
    /// owner flag is set (its heart view or a workout measures). Then the loader only reads and never stops it.
    pub fn tick(&mut self, now: u32, want: bool, stock_owns: bool) -> Step {
        let mut s = Step::default();
        if want != self.on {
            self.on = want;
            if want {
                self.held = !stock_owns;
                s.start = self.held;
                self.emit_at = now;
            } else {
                s.stop = core::mem::take(&mut self.held);
            }
            return s;
        }
        if want && now.wrapping_sub(self.emit_at) >= EMIT_MS {
            s.emit = true;
            self.emit_at = now;
        }
        s
    }
}

/// HEART event from the stock cells: `x` = 1 on the wrist, `delta` = bpm, 0 = no reading (or off the wrist).
pub fn event(worn: u8, bpm: u8) -> Event {
    let worn = worn != 0;
    Event { kind: HEART, x: worn.into(), delta: if worn { bpm.into() } else { 0 }, ..Event::default() }
}

#[cfg(test)]
mod tests {
    use super::*;

    const fn step(start: bool, stop: bool, emit: bool) -> Step {
        Step { start, stop, emit }
    }

    #[test]
    fn start_emit_stop() {
        let mut h = Heart::new();
        assert_eq!(h.tick(0, false, false), Step::default());
        assert_eq!(h.tick(100, true, false), step(true, false, false));
        assert_eq!(h.tick(1099, true, false), Step::default());
        assert_eq!(h.tick(1100, true, true), step(false, false, true)); // our own start sets the flag
        for t in (2100..60_000).step_by(1000) {
            assert_eq!(h.tick(t, true, true), step(false, false, true), "one start per hold, at {t}");
        }
        assert_eq!(h.tick(60_120, false, true), step(false, true, false));
        assert_eq!(h.tick(20_000, false, false), Step::default());
    }

    #[test]
    fn stock_owner_is_never_stopped() {
        let mut h = Heart::new();
        assert_eq!(h.tick(0, true, true), Step::default());
        assert_eq!(h.tick(10_000, true, true), step(false, false, true));
        assert_eq!(h.tick(10_020, false, true), Step::default());
    }

    #[test]
    fn pause_and_resume_restart() {
        let mut h = Heart::new();
        h.tick(0, true, false);
        assert!(h.tick(500, false, true).stop);
        assert_eq!(h.tick(600, true, false), step(true, false, false));
    }

    #[test]
    fn clock_wraps() {
        let mut h = Heart::new();
        h.tick(u32::MAX - 500, true, false);
        assert!(h.tick(499, true, true).emit);
    }

    #[test]
    fn events() {
        assert_eq!(event(1, 67), Event { kind: HEART, x: 1, delta: 67, ..Event::default() });
        assert_eq!(event(0, 67), Event { kind: HEART, x: 0, delta: 0, ..Event::default() });
        assert_eq!(event(1, 0).delta, 0);
    }
}
