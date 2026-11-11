//! Non-blocking haptic patterns. A [`Pattern`] is checked at compile time: it ends with the motor off and no
//! on-stretch reaches the loader's 2 s cap. [`Player`] is pure (feed it `now`); [`Motor`] drives the watch with it
//! and turns the motor off on `off()` (call it on PAUSE) and on drop (exit).

use cmf_wp2_app::Api;

/// The loader cuts a run longer than this and locks the motor.
pub const CAP_MS: u32 = 2000;
/// The stock motor frequency.
pub const HZ: u16 = 31;

/// `(duty ‰, ms)` steps; the last one is `(0, 0)`.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Pattern(&'static [(u16, u16)]);

impl Pattern {
    /// Panics (at compile time in a `const`) unless the steps end with the motor off, every duty is <= 1000 and no
    /// on-stretch lasts 1500 ms or more (500 ms of slack under the cap for a late poll).
    pub const fn new(steps: &'static [(u16, u16)]) -> Pattern {
        assert!(!steps.is_empty() && steps[steps.len() - 1].0 == 0, "a pattern ends with the motor off");
        let (mut i, mut on) = (0, 0u32);
        while i < steps.len() {
            assert!(steps[i].0 <= 1000, "duty is in ‰");
            on = if steps[i].0 > 0 { on + steps[i].1 as u32 } else { 0 };
            assert!(on < 1500, "an on-stretch must stay well under the 2 s cap");
            i += 1;
        }
        Pattern(steps)
    }

    pub const fn steps(self) -> &'static [(u16, u16)] {
        self.0
    }

    pub fn ms(self) -> u32 {
        self.0.iter().map(|s| s.1 as u32).sum()
    }
}

/// A light tap: tap feedback.
pub const TICK: Pattern = Pattern::new(&[(250, 20), (0, 0)]);
pub const DOUBLE: Pattern = Pattern::new(&[(400, 25), (0, 90), (400, 25), (0, 0)]);
/// Something needs you: three strong pulses (herdr-agents' "blocked").
pub const ALERT: Pattern = Pattern::new(&[(1000, 110), (0, 90), (1000, 110), (0, 90), (1000, 110), (0, 0)]);
/// Something finished: one long soft pulse (herdr-agents' "done").
pub const DONE: Pattern = Pattern::new(&[(650, 220), (0, 0)]);

/// Steps a pattern: [`Player::poll`] returns the duty to set when a step starts.
#[derive(Default)]
pub struct Player {
    steps: &'static [(u16, u16)],
    i: usize,
    next: u32,
    on: bool,
}

impl Player {
    pub fn start(&mut self, p: Pattern, now: u32) {
        self.steps = p.0;
        self.i = 0;
        self.next = now;
    }

    pub fn busy(&self) -> bool {
        self.i < self.steps.len()
    }

    /// ms until the next step, if one is pending: cap the event timeout with it.
    pub fn wait(&self, now: u32) -> Option<u32> {
        self.busy().then(|| (self.next.wrapping_sub(now) as i32).max(0) as u32)
    }

    pub fn poll(&mut self, now: u32) -> Option<u16> {
        if !self.busy() || (now.wrapping_sub(self.next) as i32) < 0 {
            return None;
        }
        let (duty, ms) = self.steps[self.i];
        self.i += 1;
        self.next = now.wrapping_add(ms as u32);
        self.on = duty > 0;
        Some(duty)
    }

    /// Ends the pattern; `Some(0)` if the motor may still be on.
    pub fn stop(&mut self) -> Option<u16> {
        self.i = self.steps.len();
        core::mem::take(&mut self.on).then_some(0)
    }
}

/// A [`Player`] wired to the watch motor at [`HZ`].
pub struct Motor {
    api: &'static Api,
    pub player: Player,
}

impl Motor {
    pub fn new(api: &'static Api) -> Self {
        Motor { api, player: Player::default() }
    }

    pub fn play(&mut self, p: Pattern) {
        self.player.start(p, self.api.now());
        self.update();
    }

    /// Call every loop turn; then wait at most [`Motor::wait`] ms for the next event.
    pub fn update(&mut self) {
        if let Some(d) = self.player.poll(self.api.now()) {
            self.set(d);
        }
    }

    pub fn wait(&self) -> Option<u32> {
        self.player.wait(self.api.now())
    }

    pub fn off(&mut self) {
        self.player.stop();
        self.set(0);
    }

    fn set(&self, duty: u16) {
        if self.api.vibrate(duty, HZ) != 0 {
            self.api.vibrate(0, HZ); // clears a -4 lock
        }
    }
}

impl Drop for Motor {
    fn drop(&mut self) {
        self.off();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // (time, duty) the motor is set to while a player runs `p` polled every `step` ms from t = 100
    fn timeline(p: Pattern, step: u32) -> std::vec::Vec<(u32, u16)> {
        let mut pl = Player::default();
        pl.start(p, 100);
        let mut out = std::vec::Vec::new();
        let mut t = 100;
        while pl.busy() {
            if let Some(d) = pl.poll(t) {
                out.push((t, d));
            }
            t += pl.wait(t).unwrap_or(0).min(step).max(1);
        }
        out
    }

    #[test]
    fn patterns_end_off_within_cap() {
        for p in [TICK, DOUBLE, ALERT, DONE] {
            let tl = timeline(p, 1000);
            assert_eq!(tl.last().unwrap().1, 0);
            assert!(p.ms() < CAP_MS);
            assert!(p.steps().iter().all(|s| s.0 <= 1000));
        }
        assert_eq!(timeline(ALERT, 1000), [(100, 1000), (210, 0), (300, 1000), (410, 0), (500, 1000), (610, 0)]);
        assert_eq!(timeline(DONE, 7), [(100, 650), (320, 0)]);
    }

    #[test]
    fn poll_wait_stop_and_wrap() {
        let mut p = Player::default();
        assert_eq!((p.poll(0), p.wait(0), p.stop()), (None, None, None));
        p.start(DONE, 100);
        assert_eq!(p.poll(100), Some(650));
        assert_eq!(p.poll(200), None);
        assert_eq!(p.wait(200), Some(120));
        assert_eq!(p.stop(), Some(0), "stopped while on: turn it off");
        assert!(!p.busy() && p.poll(400).is_none());
        p.start(TICK, u32::MAX - 5);
        assert_eq!(p.poll(u32::MAX - 5), Some(250));
        assert_eq!(p.wait(u32::MAX), Some(15));
        assert_eq!(p.poll(14), Some(0));
        assert_eq!(p.stop(), None);
    }
}
