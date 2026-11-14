//! The workout model: the program as the Mac encodes it, the workout in progress, the day rotation and the history,
//! all kept in one storage blob (docs/plans/2026-11-13-workout-app.md, Task 4, has both formats).

use cmf_wp2_app::Time;

pub const PROGRAM_MAX: usize = 2048;
pub const DAYS_MAX: usize = 7;
pub const EX_MAX: usize = 12;
pub const NAME_MAX: usize = 32;
pub const WEIGHT_MAX: usize = 32;
pub const HISTORY_MAX: usize = 64;
pub const STALE_S: u32 = 3 * 3600;
/// The last workout's trace (docs/workout.md, Trace): its sets and its heart rate, one sample per HR_STEP_S. Sized
/// so the biggest program, the full history and a full trace stay under STORE_MAX (4096).
pub const SETS_MAX: usize = 96;
pub const HR_STEP_S: u32 = 10;
pub const HR_MAX: usize = 1000;
#[allow(dead_code)] // the default the Mac side writes; the tests check it
pub const REST_DEFAULT_S: u16 = 90;
const MAGIC: &[u8; 2] = b"W1";
const NONE: u8 = 0xFF;

/// Seconds since 2000-01-01 00:00 local.
pub fn epoch_s(t: &Time) -> u32 {
    // days from civil (Howard Hinnant), shifted to 2000-03-01-based eras
    let (m, d) = (u32::from(t.month), u32::from(t.day));
    let y = u32::from(t.year) - u32::from(m <= 2);
    let era = y / 400;
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 730_425; // days from 0000-03-01 to 2000-01-01
    days * 86_400 + u32::from(t.hour) * 3600 + u32::from(t.minute) * 60 + u32::from(t.second)
}

/// CRC-32/IEEE (zlib.crc32): the program's version.
pub fn crc32(bytes: &[u8]) -> u32 {
    let mut c = !0u32;
    for &b in bytes {
        c ^= u32::from(b);
        for _ in 0..8 {
            c = if c & 1 != 0 { (c >> 1) ^ 0xEDB8_8320 } else { c >> 1 };
        }
    }
    !c
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Exercise<'a> {
    pub name: &'a str,
    pub sets: u8,
    pub reps: (u8, u8),
    pub rest_s: u16,
    pub weight: &'a str,
}

struct Cur<'a> {
    b: &'a [u8],
    i: usize,
}

impl<'a> Cur<'a> {
    fn u8(&mut self) -> Option<u8> {
        let v = *self.b.get(self.i)?;
        self.i += 1;
        Some(v)
    }
    fn u16(&mut self) -> Option<u16> {
        Some(u16::from_le_bytes([self.u8()?, self.u8()?]))
    }
    fn u32(&mut self) -> Option<u32> {
        Some(u32::from(self.u16()?) | u32::from(self.u16()?) << 16)
    }
    fn bytes(&mut self, n: usize) -> Option<&'a [u8]> {
        let s = self.b.get(self.i..self.i + n)?;
        self.i += n;
        Some(s)
    }
    fn str(&mut self, max: usize) -> Option<&'a str> {
        let n = usize::from(self.u8()?);
        if n > max {
            return None;
        }
        core::str::from_utf8(self.bytes(n)?).ok()
    }
    fn day(&mut self) -> Option<(&'a str, usize)> {
        let name = self.str(NAME_MAX)?;
        let n = usize::from(self.u8()?);
        (1..=EX_MAX).contains(&n).then_some((name, n))
    }
    fn exercise(&mut self) -> Option<Exercise<'a>> {
        let name = self.str(NAME_MAX)?;
        let (sets, lo, hi) = (self.u8()?, self.u8()?, self.u8()?);
        let rest_s = self.u16()?;
        let weight = self.str(WEIGHT_MAX)?;
        (sets >= 1 && lo <= hi).then_some(Exercise { name, sets, reps: (lo, hi), rest_s, weight })
    }
}

pub struct Program {
    bytes: [u8; PROGRAM_MAX],
    len: usize,
}

impl Program {
    const EMPTY: Program = Program { bytes: [0; PROGRAM_MAX], len: 0 };

    /// Checks every length and limit; anything else is `None`.
    #[cfg(test)]
    pub fn parse(bytes: &[u8]) -> Option<Program> {
        Program::check(bytes)?;
        let mut p = Program::EMPTY;
        p.bytes[..bytes.len()].copy_from_slice(bytes);
        p.len = bytes.len();
        Some(p)
    }

    /// What `parse` checks, without building a Program (2 KB: the app stack is 8 KiB).
    pub fn check(bytes: &[u8]) -> Option<()> {
        if bytes.len() > PROGRAM_MAX {
            return None;
        }
        let mut c = Cur { b: bytes, i: 0 };
        let days = usize::from(c.u8()?);
        if !(1..=DAYS_MAX).contains(&days) {
            return None;
        }
        for _ in 0..days {
            let (_, n) = c.day()?;
            for _ in 0..n {
                c.exercise()?;
            }
        }
        (c.i == bytes.len()).then_some(())
    }

    pub fn version(&self) -> u32 {
        crc32(self.bytes())
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes[..self.len]
    }

    pub fn days(&self) -> usize {
        usize::from(self.bytes[0])
    }

    // a cursor right after day d's header, with its name and exercise count; parse() proved the bytes
    fn seek(&self, d: usize) -> (Cur<'_>, &str, usize) {
        let mut c = Cur { b: self.bytes(), i: 1 };
        for _ in 0..d {
            let (_, n) = c.day().unwrap();
            for _ in 0..n {
                c.exercise().unwrap();
            }
        }
        let (name, n) = c.day().unwrap();
        (c, name, n)
    }

    pub fn day_name(&self, d: usize) -> &str {
        self.seek(d).1
    }

    pub fn exercises(&self, d: usize) -> usize {
        self.seek(d).2
    }

    pub fn exercise(&self, d: usize, i: usize) -> Exercise<'_> {
        let (mut c, _, _) = self.seek(d);
        for _ in 0..i {
            c.exercise().unwrap();
        }
        c.exercise().unwrap()
    }

    fn planned(&self, d: usize) -> u8 {
        (0..self.exercises(d)).map(|i| self.exercise(d, i).sets).fold(0u8, u8::saturating_add)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Workout {
    pub day: u8,
    pub started: u32,
    pub touched: u32,
    pub rest_until: u32,
    pub rest_ex: u8,
    pub done: [u8; EX_MAX],
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Entry {
    pub start: u32,
    pub dur_s: u16,
    pub day: u8,
    pub done: u8,
    pub planned: u8,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Next {
    Set { ex: usize, set: u8 },
    Finished,
}

/// The sets (exercise, seconds from the start to "Сделал") and heart rate (bpm, 0 = no reading) of the workout that
/// started at `start`, kept until the next one starts. 0 = none.
pub struct Trace {
    pub start: u32,
    pub sets: [(u8, u16); SETS_MAX],
    pub n_sets: u8,
    pub hr: [u8; HR_MAX],
    pub n_hr: u16,
}

impl Trace {
    const fn new(start: u32) -> Trace {
        Trace { start, sets: [(0, 0); SETS_MAX], n_sets: 0, hr: [0; HR_MAX], n_hr: 0 }
    }

    pub fn sets(&self) -> &[(u8, u16)] {
        &self.sets[..usize::from(self.n_sets)]
    }

    pub fn hr(&self) -> &[u8] {
        &self.hr[..usize::from(self.n_hr)]
    }

    /// `n_sets u8, (ex u8, t u16) × n, n_hr u16, bpm × n`: the blob tail and the X payload.
    pub fn encode(&self, out: &mut [u8]) -> usize {
        let mut o = Out { b: out, i: 0 };
        o.put(&[self.n_sets]);
        for &(ex, t) in self.sets() {
            o.put(&[ex]);
            o.put(&t.to_le_bytes());
        }
        o.put(&self.n_hr.to_le_bytes());
        o.put(self.hr());
        o.i
    }

    /// Empty, for the workout that started at `start`; in place (the app stack is 8 KiB, no copies of this).
    fn reset(&mut self, start: u32) {
        (self.start, self.n_sets, self.n_hr) = (start, 0, 0);
    }

    /// In place; any failure leaves it empty.
    fn parse(&mut self, c: &mut Cur, start: u32) -> Option<()> {
        self.reset(0);
        let t = self;
        t.n_sets = c.u8()?;
        if usize::from(t.n_sets) > SETS_MAX {
            return None;
        }
        for i in 0..usize::from(t.n_sets) {
            t.sets[i] = (c.u8()?, c.u16()?);
        }
        t.n_hr = c.u16()?;
        let n = usize::from(t.n_hr);
        if n > HR_MAX {
            return None;
        }
        t.hr[..n].copy_from_slice(c.bytes(n)?);
        t.start = start;
        Some(())
    }
}

pub struct Model {
    pub program: Option<Program>,
    pub trace: Trace,
    /// The latest heart rate (0 = none); not saved.
    pub bpm: u8,
    /// Heart-rate zones from the phone: the lowest bpm of zones 1..5, all 0 = none (docs/workout.md, Zones).
    pub zones: [u8; 5],
    pub last_day: Option<u8>,
    pub workout: Option<Workout>,
    hist: [Entry; HISTORY_MAX],
    hist_n: u8,
    // where the next entry goes
    hist_head: u8,
}

struct Out<'a> {
    b: &'a mut [u8],
    i: usize,
}

impl Out<'_> {
    fn put(&mut self, s: &[u8]) {
        self.b[self.i..self.i + s.len()].copy_from_slice(s);
        self.i += s.len();
    }
}

impl Model {
    pub const fn new() -> Model {
        let e = Entry { start: 0, dur_s: 0, day: 0, done: 0, planned: 0 };
        Model { program: None, trace: Trace::new(0), bpm: 0, zones: [0; 5], last_day: None, workout: None, hist: [e; HISTORY_MAX], hist_n: 0, hist_head: 0 }
    }

    /// Anything that does not parse leaves an empty model.
    // not inlined: Program::parse's 2 KB temporary stays out of the app's main frame (8 KiB stack)
    #[inline(never)]
    pub fn load(&mut self, blob: &[u8]) {
        if self.parse(blob).is_none() {
            self.clear();
        }
    }

    // field by field: `*self = Model::new()` would build a 4 KB Model on the 8 KiB stack
    fn clear(&mut self) {
        (self.program, self.last_day, self.workout, self.hist_n, self.hist_head, self.bpm) = (None, None, None, 0, 0, 0);
        self.zones = [0; 5];
        self.trace.reset(0);
    }

    fn parse(&mut self, blob: &[u8]) -> Option<()> {
        self.clear();
        let mut c = Cur { b: blob, i: 0 };
        if c.bytes(2)? != MAGIC {
            return None;
        }
        let version = c.u32()?;
        let len = usize::from(c.u16()?);
        if len > 0 {
            let bytes = c.bytes(len)?;
            Program::check(bytes)?;
            if crc32(bytes) != version {
                return None;
            }
            self.put_program(bytes);
        }
        let days = self.program.as_ref().map_or(0, Program::days);
        self.last_day = Some(c.u8()?).filter(|&d| usize::from(d) < days);
        let active = c.u8()?;
        let mut w = Workout { day: c.u8()?, started: c.u32()?, touched: c.u32()?, rest_until: c.u32()?, rest_ex: c.u8()?, done: [0; EX_MAX] };
        w.done.copy_from_slice(c.bytes(EX_MAX)?);
        if active == 1 && usize::from(w.day) < days {
            self.workout = Some(w);
        }
        let n = c.u8()?;
        if usize::from(n) > HISTORY_MAX {
            return None;
        }
        for _ in 0..n {
            let e = Entry { start: c.u32()?, dur_s: c.u16()?, day: c.u8()?, done: c.u8()?, planned: c.u8()? };
            self.push(e);
        }
        // the trace came later: a blob without it is still whole
        if c.i < blob.len() {
            let start = c.u32()?;
            self.trace.parse(&mut c, start)?;
        }
        // the zones came after it (an empty trace stands in when there is none)
        if c.i < blob.len() {
            self.zones.copy_from_slice(c.bytes(5)?);
        }
        (c.i == blob.len()).then_some(())
    }

    /// The blob for `api.save`; `buf` holds at least STORE_MAX (4096) bytes.
    #[inline(never)]
    pub fn save(&self, buf: &mut [u8]) -> usize {
        let mut o = Out { b: buf, i: 0 };
        let (version, bytes) = self.program.as_ref().map_or((0, &[][..]), |p| (p.version(), p.bytes()));
        o.put(MAGIC);
        o.put(&version.to_le_bytes());
        o.put(&(bytes.len() as u16).to_le_bytes());
        o.put(bytes);
        o.put(&[self.last_day.unwrap_or(NONE)]);
        let w = self.workout.unwrap_or(Workout { day: 0, started: 0, touched: 0, rest_until: 0, rest_ex: NONE, done: [0; EX_MAX] });
        o.put(&[u8::from(self.workout.is_some()), w.day]);
        for v in [w.started, w.touched, w.rest_until] {
            o.put(&v.to_le_bytes());
        }
        o.put(&[w.rest_ex]);
        o.put(&w.done);
        o.put(&[self.hist_n]);
        let mut old: [Entry; HISTORY_MAX] = [Entry::default(); HISTORY_MAX];
        let n = usize::from(self.hist_n);
        for (i, e) in self.history().enumerate() {
            old[n - 1 - i] = e; // oldest first
        }
        for e in &old[..n] {
            o.put(&e.start.to_le_bytes());
            o.put(&e.dur_s.to_le_bytes());
            o.put(&[e.day, e.done, e.planned]);
        }
        let zones = self.zones != [0; 5];
        if self.trace.start != 0 || zones {
            o.put(&self.trace.start.to_le_bytes());
            o.i += self.trace.encode(&mut o.b[o.i..]);
        }
        if zones {
            o.put(&self.zones);
        }
        o.i
    }

    /// A heart-rate reading at `now` (0 = none) into the running workout's trace, one sample per HR_STEP_S; a gap
    /// stays 0. True when it took a new sample (time to save).
    pub fn hr_sample(&mut self, bpm: u8, now: u32) -> bool {
        let Some(w) = self.workout else { return false };
        let t = &mut self.trace;
        let i = (now.wrapping_sub(w.started) / HR_STEP_S) as usize;
        if t.start != w.started || i >= HR_MAX || i < usize::from(t.n_hr) {
            return false;
        }
        t.hr[i] = bpm;
        t.n_hr = i as u16 + 1;
        true
    }

    /// The zone of `bpm`: 1..=5, 0 below zone 1, without zones or without a reading.
    pub fn zone(&self, bpm: u8) -> u8 {
        if bpm == 0 || self.zones == [0; 5] {
            return 0;
        }
        self.zones.iter().filter(|&&lo| bpm >= lo).count() as u8
    }

    pub fn next_day(&self) -> usize {
        match (&self.program, self.last_day) {
            (Some(p), Some(d)) => (usize::from(d) + 1) % p.days(),
            _ => 0,
        }
    }

    pub fn start(&mut self, day: usize, now: u32) {
        if self.program.as_ref().is_some_and(|p| day < p.days()) {
            self.workout = Some(Workout { day: day as u8, started: now, touched: now, rest_until: now, rest_ex: NONE, done: [0; EX_MAX] });
            self.trace.reset(now);
        }
    }

    /// Marks the next set of `ex` done and starts its rest.
    pub fn done_set(&mut self, ex: usize, now: u32) {
        let (Some(p), Some(w)) = (&self.program, &mut self.workout) else { return };
        let d = usize::from(w.day);
        if ex >= p.exercises(d) {
            return;
        }
        let e = p.exercise(d, ex);
        if w.done[ex] < e.sets {
            w.done[ex] += 1;
            w.rest_until = now.wrapping_add(u32::from(e.rest_s));
            w.rest_ex = ex as u8;
            w.touched = now;
            let t = &mut self.trace;
            if t.start == w.started && usize::from(t.n_sets) < SETS_MAX {
                t.sets[usize::from(t.n_sets)] = (ex as u8, now.wrapping_sub(w.started).min(u32::from(u16::MAX)) as u16);
                t.n_sets += 1;
            }
        }
    }

    /// What follows the rest: the same exercise while it has sets left, else the first one from the top with sets
    /// left (the owner's "in order"), else the end.
    pub fn after_rest(&self) -> Next {
        let (Some(p), Some(w)) = (&self.program, &self.workout) else { return Next::Finished };
        let d = usize::from(w.day);
        let open = |i: usize| w.done[i] < p.exercise(d, i).sets;
        let r = usize::from(w.rest_ex);
        let ex = if r < p.exercises(d) && open(r) { Some(r) } else { (0..p.exercises(d)).find(|&i| open(i)) };
        ex.map_or(Next::Finished, |ex| Next::Set { ex, set: w.done[ex] })
    }

    /// Seconds of rest still to go: 0 without a running rest, and 0 when the clock says more than the exercise's rest
    /// is left (it went back: a reboot without a wall clock, a time sync), so a rest never outlasts its length.
    pub fn rest_left(&self, now: u32) -> u32 {
        let (Some(p), Some(w)) = (&self.program, &self.workout) else { return 0 };
        let d = usize::from(w.day);
        if usize::from(w.rest_ex) >= p.exercises(d) {
            return 0;
        }
        let rest = u32::from(p.exercise(d, usize::from(w.rest_ex)).rest_s);
        let left = w.rest_until.wrapping_sub(now) as i32;
        if left <= 0 || left as u32 > rest { 0 } else { left as u32 }
    }

    /// Ends the workout: with at least one set done it goes to the history and the rotation; else it is dropped.
    pub fn finish(&mut self, now: u32) {
        let (Some(p), Some(w)) = (&self.program, self.workout.take()) else { return };
        let done = w.done.iter().fold(0u8, |a, &b| a.saturating_add(b));
        if done == 0 {
            return;
        }
        let dur = now.wrapping_sub(w.started).min(u32::from(u16::MAX)) as u16;
        let planned = p.planned(usize::from(w.day));
        self.push(Entry { start: w.started, dur_s: dur, day: w.day, done, planned });
        self.last_day = Some(w.day);
    }

    /// Finishes a workout nobody touched for STALE_S, as of its last touch.
    pub fn expire(&mut self, now: u32) {
        if let Some(w) = self.workout.filter(|w| now.wrapping_sub(w.touched) > STALE_S && now > w.touched) {
            self.finish(w.touched);
        }
    }

    /// A set done elsewhere (the phone, docs/specs/2026-11-14-shared-workout-design.md): `(ex, t)` of the workout that
    /// started at `start` on `day`; `ex` NONE only says it runs. With no workout open it is adopted unless stale or
    /// already in the history. A known event, another workout or a complete exercise change nothing. True if changed.
    pub fn merge_set(&mut self, start: u32, day: u8, ex: u8, t: u16, now: u32) -> bool {
        let Some(p) = &self.program else { return false };
        let mut changed = false;
        if self.workout.is_none() {
            let stale = now.wrapping_sub(start) > STALE_S && now > start;
            if stale || usize::from(day) >= p.days() || self.history().any(|e| e.start == start) {
                return false;
            }
            self.workout = Some(Workout { day, started: start, touched: now, rest_until: start, rest_ex: NONE, done: [0; EX_MAX] });
            self.trace.reset(start);
            changed = true;
        }
        let Some(w) = &mut self.workout else { return changed };
        let (d, i) = (usize::from(w.day), usize::from(ex));
        if w.started != start || i >= p.exercises(d) {
            return changed;
        }
        let e = p.exercise(d, i);
        let tr = &mut self.trace;
        if w.done[i] >= e.sets || tr.sets().contains(&(ex, t)) || usize::from(tr.n_sets) >= SETS_MAX {
            return changed;
        }
        // kept in time order; only the newest event starts a rest
        let at = tr.sets().iter().position(|&(_, u)| u > t).unwrap_or(usize::from(tr.n_sets));
        tr.sets.copy_within(at..usize::from(tr.n_sets), at + 1);
        tr.sets[at] = (ex, t);
        tr.n_sets += 1;
        w.done[i] += 1;
        w.touched = now;
        if at + 1 == usize::from(tr.n_sets) {
            w.rest_until = start.wrapping_add(u32::from(t)).wrapping_add(u32::from(e.rest_s));
            w.rest_ex = ex;
        }
        true
    }

    /// Takes back the set `(ex, t)` of the workout that started at `start` (undone on the phone); a rest it started
    /// ends. True if it was there.
    pub fn remove_set(&mut self, start: u32, ex: u8, t: u16) -> bool {
        let (Some(w), tr) = (&mut self.workout, &mut self.trace) else { return false };
        let Some(at) = tr.sets().iter().position(|&s| s == (ex, t)).filter(|_| w.started == start) else { return false };
        let n = usize::from(tr.n_sets);
        tr.sets.copy_within(at + 1..n, at);
        tr.n_sets -= 1;
        let i = usize::from(ex);
        if i < EX_MAX {
            w.done[i] = w.done[i].saturating_sub(1);
        }
        if at + 1 == n {
            w.rest_until = w.started; // over
        }
        true
    }

    /// Finishes the open workout if it started at `start` (the phone ended it). True if it did.
    pub fn finish_at(&mut self, start: u32, end: u32) -> bool {
        let open = self.workout.is_some_and(|w| w.started == start);
        if open {
            self.finish(end);
        }
        open
    }

    /// A workout done on the phone alone, once per start; the rotation follows it when it is the newest.
    // ponytail: an older entry still goes in at the head; the phone merges by start, so only the order is off
    pub fn add_entry(&mut self, e: Entry) -> bool {
        let days = self.program.as_ref().map_or(0, Program::days);
        if self.history().any(|h| h.start == e.start) || self.workout.is_some_and(|w| w.started == e.start) {
            return false;
        }
        if self.history().all(|h| h.start < e.start) && usize::from(e.day) < days {
            self.last_day = Some(e.day);
        }
        self.push(e);
        true
    }

    #[cfg(test)]
    pub fn replace_program(&mut self, p: Program, now: u32) {
        self.take_program(p.bytes(), now);
    }

    /// A new program from the phone, if it checks: finishes a running workout first. Its version, or None.
    #[inline(never)]
    pub fn take_program(&mut self, bytes: &[u8], now: u32) -> Option<u32> {
        Program::check(bytes)?;
        self.finish(now);
        self.last_day = self.last_day.filter(|&d| d < bytes[0]);
        self.put_program(bytes);
        Some(crc32(bytes))
    }

    // in place: no 2 KB Program temporary on the 8 KiB app stack; `bytes` passed Program::check
    fn put_program(&mut self, bytes: &[u8]) {
        if self.program.is_none() {
            self.program = Some(Program::EMPTY);
        }
        if let Some(p) = &mut self.program {
            p.bytes[..bytes.len()].copy_from_slice(bytes);
            p.len = bytes.len();
        }
    }

    /// Newest first.
    pub fn history(&self) -> impl Iterator<Item = Entry> + '_ {
        let (n, head) = (usize::from(self.hist_n), usize::from(self.hist_head));
        (0..n).map(move |k| self.hist[(head + HISTORY_MAX - 1 - k) % HISTORY_MAX])
    }

    fn push(&mut self, e: Entry) {
        self.hist[usize::from(self.hist_head)] = e;
        self.hist_head = ((usize::from(self.hist_head) + 1) % HISTORY_MAX) as u8;
        self.hist_n = (self.hist_n + 1).min(HISTORY_MAX as u8);
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;

    // (name, sets, lo, hi, rest_s, weight)
    pub type Ex<'a> = (&'a str, u8, u8, u8, u16, &'a str);

    /// The Task 4 program format, built by hand (Task 6's encoder writes the same bytes).
    pub fn enc(days: &[(&str, &[Ex])]) -> Vec<u8> {
        let mut v = vec![days.len() as u8];
        let s = |v: &mut Vec<u8>, t: &str| {
            v.push(t.len() as u8);
            v.extend_from_slice(t.as_bytes());
        };
        for (name, exs) in days {
            s(&mut v, name);
            v.push(exs.len() as u8);
            for &(n, sets, lo, hi, rest, w) in exs.iter() {
                s(&mut v, n);
                v.extend_from_slice(&[sets, lo, hi]);
                v.extend_from_slice(&rest.to_le_bytes());
                s(&mut v, w);
            }
        }
        v
    }

    fn two_days() -> Vec<u8> {
        enc(&[
            ("Жим и присед", &[("Жим лежа", 4, 5, 8, 120, "90 кг"), ("Икры", 4, 10, 15, 90, "")]),
            ("Тяга", &[("Румынская тяга", 4, 6, 8, 90, "")]),
        ])
    }

    fn three_days() -> Program {
        Program::parse(&enc(&[
            ("A", &[("a1", 2, 5, 5, 60, ""), ("a2", 1, 5, 5, 60, ""), ("a3", 2, 5, 5, 60, "")]),
            ("B", &[("b1", 3, 8, 10, 90, "")]),
            ("C", &[("c1", 3, 8, 10, 90, "")]),
        ]))
        .unwrap()
    }

    fn model(p: Program) -> Model {
        let mut m = Model::new();
        m.program = Some(p);
        m
    }

    #[test]
    fn parse_roundtrip() {
        let bytes = two_days();
        let p = Program::parse(&bytes).unwrap();
        assert_eq!(p.days(), 2);
        assert_eq!(p.day_name(0), "Жим и присед");
        assert_eq!(p.exercises(0), 2);
        assert_eq!(p.exercise(0, 0), Exercise { name: "Жим лежа", sets: 4, reps: (5, 8), rest_s: 120, weight: "90 кг" });
        assert_eq!(p.exercise(0, 1).rest_s, 90);
        assert_eq!(p.exercise(0, 1).weight, "");
        assert_eq!(p.exercise(1, 0).name, "Румынская тяга");
        assert_eq!(p.bytes(), &bytes[..]);
        assert_eq!(p.version(), crc32(&bytes));
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    }

    #[test]
    fn weight_up_to_32_bytes() {
        let w = "гантели по 8–10 кг"; // 31 B
        assert_eq!(Program::parse(&enc(&[("d", &[("e", 1, 1, 1, 60, w)])])).unwrap().exercise(0, 0).weight, w);
        let long = "я".repeat(17); // 34 B
        assert!(Program::parse(&enc(&[("d", &[("e", 1, 1, 1, 60, &long)])])).is_none());
    }

    #[test]
    fn parse_rejects() {
        let ok = two_days();
        assert!(Program::parse(&ok[..ok.len() - 1]).is_none());
        let mut extra = ok.clone();
        extra.push(0);
        assert!(Program::parse(&extra).is_none());
        let eight: Vec<(&str, &[Ex])> = (0..8).map(|_| ("d", &[("e", 1, 1, 1, 60, "")][..])).collect();
        assert!(Program::parse(&enc(&eight)).is_none());
        let thirteen = [("e", 1, 1, 1, 60, ""); 13];
        assert!(Program::parse(&enc(&[("d", &thirteen)])).is_none());
        let long = "x".repeat(33);
        assert!(Program::parse(&enc(&[("d", &[(&long, 1, 1, 1, 60, "")])])).is_none());
        let mut bad = enc(&[("ab", &[("e", 1, 1, 1, 60, "")])]);
        bad[2] = 0xFF; // invalid UTF-8 in the day name
        assert!(Program::parse(&bad).is_none());
        assert!(Program::parse(&enc(&[("d", &[("e", 1, 9, 5, 60, "")])])).is_none()); // reps lo > hi
        assert!(Program::parse(&enc(&[("d", &[("e", 0, 1, 1, 60, "")])])).is_none()); // no sets
        assert!(Program::parse(&enc(&[])).is_none()); // no days
        assert!(Program::parse(&[0u8; PROGRAM_MAX + 1]).is_none());
    }

    #[test]
    fn rotation() {
        let mut m = model(three_days());
        assert_eq!(m.next_day(), 0);
        m.start(0, 100);
        m.done_set(0, 110);
        m.finish(200);
        assert_eq!(m.next_day(), 1);
        m.start(2, 300);
        m.done_set(0, 310);
        m.finish(400);
        assert_eq!(m.next_day(), 0);
        assert_eq!(Model::new().next_day(), 0);
    }

    #[test]
    fn zero_sets_dropped() {
        let mut m = model(three_days());
        m.start(1, 100);
        m.finish(200);
        assert!(m.workout.is_none());
        assert_eq!(m.history().count(), 0);
        assert_eq!(m.next_day(), 0);
    }

    #[test]
    fn after_rest_order() {
        let mut m = model(three_days());
        m.start(0, 0);
        m.done_set(0, 10);
        assert_eq!(m.workout.unwrap().rest_until, 70);
        assert_eq!(m.after_rest(), Next::Set { ex: 0, set: 1 });
        m.done_set(0, 20);
        assert_eq!(m.after_rest(), Next::Set { ex: 1, set: 0 });
        m.done_set(2, 30);
        m.done_set(2, 40);
        assert_eq!(m.after_rest(), Next::Set { ex: 1, set: 0 });
        m.done_set(1, 50);
        assert_eq!(m.after_rest(), Next::Finished);
        m.done_set(1, 60); // already complete: no extra set
        assert_eq!(m.workout.unwrap().done[1], 1);
    }

    #[test]
    fn expire_after_3h() {
        let mut m = model(three_days());
        m.start(0, 500);
        m.done_set(0, 1000);
        m.expire(1000 + STALE_S);
        assert!(m.workout.is_some());
        m.expire(1001 + STALE_S);
        assert!(m.workout.is_none());
        let e = m.history().next().unwrap();
        assert_eq!((e.start, e.dur_s, e.day, e.done, e.planned), (500, 500, 0, 1, 5));
    }

    #[test]
    fn replace_program_finishes_first() {
        let mut m = model(three_days());
        m.start(2, 100);
        m.done_set(0, 110);
        let one = Program::parse(&enc(&[("Solo", &[("s", 1, 1, 1, 60, "")])])).unwrap();
        m.replace_program(one, 200);
        assert!(m.workout.is_none());
        assert_eq!(m.history().next().map(|e| (e.day, e.done)), Some((2, 1)));
        assert_eq!(m.next_day(), 0);
        assert_eq!(m.program.as_ref().unwrap().days(), 1);
    }

    #[test]
    fn blob_roundtrip() {
        let mut m = model(three_days());
        for i in 0..70u32 {
            m.start((i % 3) as usize, i * 1000);
            m.done_set(0, i * 1000 + 10);
            m.finish(i * 1000 + 500);
        }
        m.start(1, 90_000);
        m.done_set(0, 90_010);
        let mut buf = [0u8; 4096];
        let n = m.save(&mut buf);
        let mut back = Model::new();
        back.load(&buf[..n]);
        assert_eq!(back.program.as_ref().map(|p| p.bytes()), m.program.as_ref().map(|p| p.bytes()));
        assert_eq!((back.last_day, back.workout), (m.last_day, m.workout));
        let h: Vec<Entry> = back.history().collect();
        assert_eq!(h.len(), HISTORY_MAX);
        assert_eq!(h[0].start, 69_000);
        assert_eq!(h, m.history().collect::<Vec<_>>());

        // the biggest program still leaves room
        let name = "x".repeat(NAME_MAX);
        let exs: Vec<Ex> = (0..EX_MAX).map(|_| (name.as_str(), 9, 9, 9, 90, "1234567890123456")).collect();
        let mut days: Vec<(&str, &[Ex])> = vec![];
        for _ in 0..DAYS_MAX {
            days.push((name.as_str(), &exs[..]));
        }
        let mut big = enc(&days);
        big.truncate(PROGRAM_MAX); // over the limit as a whole: parse refuses it
        assert!(Program::parse(&big).is_none());
        let fit = enc(&days[..2]); // 2 × 12 exercises with 32 B names and 16 B weights: 1389 B
        assert!(fit.len() <= PROGRAM_MAX);
        let mut m = model(Program::parse(&fit).unwrap());
        for i in 0..HISTORY_MAX as u32 {
            m.start(0, 1 + i * 10);
            m.done_set(0, 2 + i * 10);
            m.finish(3 + i * 10);
        }
        m.start(0, 1_000_000);
        m.trace.n_sets = SETS_MAX as u8;
        m.trace.n_hr = HR_MAX as u16;
        assert!(m.save(&mut buf) <= 4096);
        // the biggest program the editor can make (PROGRAM_MAX) with a full history and trace
        assert!(PROGRAM_MAX + 36 + 1 + HISTORY_MAX * 9 + 4 + 1 + SETS_MAX * 3 + 2 + HR_MAX <= 4096);
    }

    #[test]
    fn trace() {
        let mut m = model(three_days());
        assert!(!m.hr_sample(70, 5), "no workout");
        m.start(1, 1000);
        assert!(m.hr_sample(70, 1000));
        assert!(!m.hr_sample(71, 1005), "same step");
        assert!(m.hr_sample(90, 1031)); // step 3: steps 1 and 2 stay 0
        m.done_set(0, 1040);
        m.done_set(0, 1100);
        assert_eq!(m.trace.hr(), &[70, 0, 0, 90]);
        assert_eq!(m.trace.sets(), &[(0, 40), (0, 100)]);
        m.finish(1200);
        assert!(!m.hr_sample(80, 1210), "finished");
        let mut buf = [0u8; 4096];
        let n = m.save(&mut buf);
        let mut back = Model::new();
        back.load(&buf[..n]);
        assert_eq!((back.trace.start, back.trace.sets(), back.trace.hr()), (1000, m.trace.sets(), m.trace.hr()));
        // an older blob without a trace still loads
        let mut old = model(three_days());
        old.start(0, 1);
        old.trace.reset(0);
        let n = old.save(&mut buf);
        back.load(&buf[..n]);
        assert!(back.program.is_some() && back.trace.start == 0);
        // the next workout starts a new trace
        m.start(2, 5000);
        assert_eq!((m.trace.start, m.trace.n_sets, m.trace.n_hr), (5000, 0, 0));
    }

    #[test]
    fn blob_garbage() {
        let mut good = model(three_days());
        good.start(0, 1);
        let mut buf = [0u8; 4096];
        let n = good.save(&mut buf);
        let cases: [&[u8]; 4] = [b"H1abc", &buf[..10], &[], &buf[..n - 1]];
        for c in cases {
            let mut m = model(three_days());
            m.load(c);
            assert!(m.program.is_none() && m.workout.is_none() && m.last_day.is_none(), "{c:?}");
        }
        let mut bad = buf;
        bad[6] = 0xFF; // program_len past the end
        bad[7] = 0x7F;
        let mut m = Model::new();
        m.load(&bad[..n]);
        assert!(m.program.is_none());
        let mut flip = buf;
        flip[9] ^= 0x55; // program bytes no longer match the version
        m.load(&flip[..n]);
        assert!(m.program.is_none());
    }

    #[test]
    fn fixture_decodes() {
        // written by tests/test_workout.py (WP2_REGEN=1) from tests/fixtures/workout/*.md
        let bytes = include_bytes!("../../../../tests/fixtures/workout-program.bin");
        let p = Program::parse(bytes).unwrap();
        assert_eq!((p.days(), p.day_name(0), p.day_name(1)), (2, "Верх", "Ноги"));
        assert_eq!(p.exercise(0, 0), Exercise { name: "Жим лежа", sets: 4, reps: (5, 8), rest_s: 120, weight: "80 кг" });
        assert_eq!(p.exercise(0, 1).weight, "свой вес");
        assert!(p.exercise(0, 2).name.len() <= NAME_MAX && p.exercise(0, 2).name.starts_with("Очень длинное"));
        assert_eq!(p.exercise(1, 1).reps, (20, 20));
        assert_eq!(p.exercise(1, 1).rest_s, REST_DEFAULT_S);
    }

    #[test]
    fn rest_left_is_bounded() {
        let mut m = model(three_days());
        assert_eq!(m.rest_left(0), 0); // no workout
        m.start(0, 50_000);
        assert_eq!(m.rest_left(50_000), 0); // no set yet
        m.done_set(0, 50_000); // rest 60 s
        assert_eq!((m.rest_left(50_000), m.rest_left(50_030), m.rest_left(50_060)), (60, 30, 0));
        // the clock went back (a reboot without a wall clock, a time sync): the rest is over, not 14 hours long
        assert_eq!(m.rest_left(30), 0);
        assert_eq!(m.rest_left(49_999), 0);
    }

    #[test]
    fn no_clock() {
        // boot seconds instead of a wall clock: small numbers work the same
        let mut m = model(three_days());
        m.start(0, 3);
        m.done_set(0, 5);
        assert_eq!(m.workout.unwrap().rest_until, 65);
        m.finish(9);
        assert_eq!(m.history().next().map(|e| (e.start, e.dur_s)), Some((3, 6)));
        let t = Time { year: 2026, month: 11, day: 13, hour: 10, ..Time::default() };
        assert_eq!(epoch_s(&t), 847_879_200);
        assert_eq!(epoch_s(&Time { year: 2000, month: 1, day: 1, ..Time::default() }), 0);
        assert_eq!(epoch_s(&Time { year: 2024, month: 3, day: 1, ..Time::default() }) - epoch_s(&Time { year: 2024, month: 2, day: 28, ..Time::default() }), 2 * 86400);
    }

    #[test]
    fn merge_set_adopts_and_dedupes() {
        let mut m = model(three_days());
        assert!(m.merge_set(1000, 0, NONE, 0, 1005)); // only "it runs": adopted, no sets
        let w = m.workout.unwrap();
        assert_eq!((w.day, w.started, w.done[0]), (0, 1000, 0));
        assert!(m.merge_set(1000, 0, 0, 30, 1031));
        assert!(!m.merge_set(1000, 0, 0, 30, 1032), "the same event twice");
        assert_eq!(m.workout.unwrap().done[0], 1);
        assert_eq!((m.workout.unwrap().rest_until, m.workout.unwrap().rest_ex), (1090, 0)); // 1000 + 30 + 60
        assert_eq!(m.trace.sets(), &[(0, 30)]);
    }

    #[test]
    fn merge_set_older_event_keeps_rest_and_order() {
        let mut m = model(three_days());
        m.start(0, 1000);
        m.done_set(2, 1100); // t 100, rest until 1160
        assert!(m.merge_set(1000, 0, 0, 40, 1101));
        assert_eq!((m.workout.unwrap().rest_until, m.workout.unwrap().rest_ex), (1160, 2));
        assert_eq!(m.trace.sets(), &[(0, 40), (2, 100)]);
    }

    #[test]
    fn merge_set_refuses() {
        let mut m = model(three_days());
        assert!(!m.merge_set(1000, 0, 0, 10, 1000 + STALE_S + 1), "stale");
        assert!(!m.merge_set(1000, 5, 0, 10, 1010), "no such day");
        m.start(0, 2000);
        assert!(!m.merge_set(1000, 0, 0, 10, 2010), "another workout");
        assert!(!m.merge_set(2000, 0, 9, 10, 2010), "no such exercise");
        assert!(m.merge_set(2000, 0, 1, 10, 2010));
        assert!(!m.merge_set(2000, 0, 1, 20, 2020), "a1 has one set");
        m.finish(2030);
        assert!(!m.merge_set(2000, 0, 0, 40, 2040), "already in the history");
        assert!(m.workout.is_none());
    }

    #[test]
    fn finish_at_and_add_entry() {
        let mut m = model(three_days());
        m.start(1, 1000);
        m.done_set(0, 1010);
        assert!(!m.finish_at(999, 1100));
        assert!(m.finish_at(1000, 1100));
        assert_eq!(m.history().next().unwrap().dur_s, 100);
        let e = Entry { start: 5000, dur_s: 600, day: 2, done: 3, planned: 3 };
        assert!(m.add_entry(e));
        assert!(!m.add_entry(e), "once per start");
        assert_eq!((m.history().count(), m.next_day()), (2, 0)); // day 2 was the newest: day 0 next
        assert!(m.add_entry(Entry { start: 100, day: 0, ..e })); // older: kept, the rotation stays
        assert_eq!(m.next_day(), 0);
    }

    #[test]
    fn remove_set_takes_back_and_ends_the_rest() {
        let mut m = model(three_days());
        m.start(0, 1000);
        m.done_set(0, 1010);
        m.done_set(2, 1020);
        assert!(!m.remove_set(999, 2, 20), "another workout");
        assert!(!m.remove_set(1000, 2, 21), "no such set");
        assert!(m.remove_set(1000, 2, 20));
        assert_eq!((m.workout.unwrap().done[2], m.rest_left(1021)), (0, 0));
        assert_eq!(m.trace.sets(), &[(0, 10)]);
        assert!(m.merge_set(1000, 0, 2, 20, 1030), "it can come back as a new press");
    }

    #[test]
    fn zones_survive_the_blob_and_classify() {
        let mut m = model(three_days());
        assert_eq!(m.zone(150), 0, "no zones");
        m.zones = [95, 114, 133, 152, 171]; // synthetic bounds
        let mut buf = vec![0u8; 4096];
        let n = m.save(&mut buf);
        let mut back = Model::new();
        back.load(&buf[..n]);
        assert_eq!(back.zones, m.zones, "saved without a trace");
        assert_eq!([back.zone(0), back.zone(90), back.zone(95), back.zone(140), back.zone(200)], [0, 0, 1, 3, 5]);
        back.start(0, 1000);
        back.done_set(0, 1010);
        let n = back.save(&mut buf);
        let mut again = Model::new();
        again.load(&buf[..n]);
        assert_eq!((again.zones, again.trace.sets().len()), (m.zones, 1), "and with one");
    }
}
