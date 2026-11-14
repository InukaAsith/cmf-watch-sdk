//! The workout screens on the watch's own widgets in the stock look (`cmf_wp2_app::stock`). Each view is one page
//! under ROOT; a view change deletes the page (and every object with it) and builds the next. `tick` updates only the
//! clock, the rest ring and its time. Texts in the system font (F32M), which lacks `·`, `×`, `●`; big numbers in
//! F48M, which has digits, `:` and `-` but no `–` (docs/workout.md, Fonts).

use crate::model::{Model, Next};
use cmf_wp2_app::lv::*;
use cmf_wp2_app::stock::{self, ACCENT, BLACK, CIRCLE, GREY, LETTER_BG, ROW_BG, ROW_H, ROW_PRESSED, ROW_W, SCREEN, WHITE};
use core::fmt::Write;

/// Font slot of the big numbers.
pub const BIG: u8 = 1;
pub const HEADER_H: i32 = 96;
const PAGE_H: i32 = SCREEN - HEADER_H;
/// The stock heart's red (RGB565).
const HEART: i32 = 0xF8A2;
const TEXT_X: i16 = 40;
const TEXT_W: i32 = ROW_W - TEXT_X as i32 - 40;
const DOT: i32 = 14;
// one line of the system font (line height 33)
const LINE_H: i32 = 36;
const DOTS_MAX: u8 = 8;
const HITS: usize = 16;
// bytes one row or widget group can take: flush first when fewer are left
const GROUP: usize = 1100;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum View {
    Start,
    Day,
    Set { ex: usize },
    Rest,
    Ask,
    Done,
}

/// What a tap on a handle means.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Hit {
    Day(usize),
    Exercise(usize),
    /// The day list's "Завершить" row.
    Finish,
    /// "Сделал".
    Done,
    Skip,
    Leave,
    End,
    Cancel,
}

pub struct Screen {
    pub view: View,
    page: Obj,
    header: Obj,
    hits: [(Obj, Hit); HITS],
    n_hits: usize,
    clock: Obj,
    arc: Obj,
    time: Obj,
    shown: Option<u32>,
    // heart rate: the label and the bpm it shows (None: not drawn yet)
    hr: Obj,
    hr_shown: Option<u8>,
}

/// A short text in a fixed buffer.
pub struct Txt<const N: usize> {
    b: [u8; N],
    n: usize,
}

impl<const N: usize> Txt<N> {
    pub fn new() -> Self {
        Txt { b: [0; N], n: 0 }
    }
    pub fn as_str(&self) -> &str {
        core::str::from_utf8(&self.b[..self.n]).unwrap_or("")
    }
}

impl<const N: usize> Write for Txt<N> {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        // cut on a char boundary when full
        let mut end = s.len().min(N - self.n);
        while !s.is_char_boundary(end) {
            end -= 1;
        }
        self.b[self.n..self.n + end].copy_from_slice(&s.as_bytes()[..end]);
        self.n += end;
        Ok(())
    }
}

/// "m:ss", or "h:mm:ss" from an hour.
pub fn clock(secs: u32, out: &mut Txt<16>) {
    let (h, m, s) = (secs / 3600, secs / 60 % 60, secs % 60);
    if h > 0 {
        let _ = write!(out, "{h}:{m:02}:{s:02}");
    } else {
        let _ = write!(out, "{m}:{s:02}");
    }
}

/// упражнение, упражнения, упражнений
pub fn exercises_word(n: usize) -> &'static str {
    match (n % 10, n % 100) {
        (1, r) if r != 11 => "упражнение",
        (2..=4, r) if !(12..=14).contains(&r) => "упражнения",
        _ => "упражнений",
    }
}

/// "из N подхода/подходов"
pub fn sets_word(n: usize) -> &'static str {
    if n % 10 == 1 && n % 100 != 11 { "подхода" } else { "подходов" }
}

fn reps(lo: u8, hi: u8, dash: &str, out: &mut Txt<16>) {
    if lo == hi {
        let _ = write!(out, "{lo}");
    } else {
        let _ = write!(out, "{lo}{dash}{hi}");
    }
}

fn bare(b: &mut Batch, o: Obj) -> Result<(), LvError> {
    for p in [Prop::PadTop, Prop::PadBottom, Prop::PadLeft, Prop::PadRight, Prop::BorderWidth, Prop::Radius, Prop::BgOpa] {
        b.style(o, p, 0, 0)?;
    }
    b.flags(o, &[], &[Flag::Scrollable])
}

fn label(b: &mut Batch, parent: Obj, text: &str, colour: i32, align: Align, x: i16, y: i16, w: i32) -> Result<Obj, LvError> {
    let l = b.create(Class::Label, parent)?;
    if w > 0 {
        b.size(l, w, LINE_H)?;
        b.long_mode(l, LongMode::Dot)?;
        b.style(l, Prop::TextAlign, 0, 2)?; // centre inside the width
    }
    b.style(l, Prop::TextColor, 0, colour)?;
    b.text(l, text)?;
    b.align(l, align, x, y)?;
    Ok(l)
}

/// The heart rate as the stock workout shows it under the clock: a red mark (the font has no heart) and the bpm,
/// centred on `x`; returns the bpm label.
fn heart(b: &mut Batch, parent: Obj, x: i16, y: i16) -> Result<Obj, LvError> {
    let dot = b.create(Class::Obj, parent)?;
    b.size(dot, 16, 16)?;
    b.align(dot, Align::TopMid, x - 30, y + 12)?;
    b.style(dot, Prop::Radius, 0, CIRCLE)?;
    b.style(dot, Prop::BorderWidth, 0, 0)?;
    b.style(dot, Prop::BgColor, 0, HEART)?;
    b.style(dot, Prop::BgOpa, 0, 255)?;
    b.flags(dot, &[], &[Flag::Clickable])?;
    let l = label(b, parent, "--", WHITE, Align::TopLeft, 0, y, 0)?;
    b.align(l, Align::TopMid, x + 12, y)?;
    Ok(l)
}

/// One left-aligned line in a row, "..." when it does not fit.
fn line(b: &mut Batch, row: Obj, text: &str, colour: i32, x: i16, y: i16, w: i32) -> Result<Obj, LvError> {
    let l = b.create(Class::Label, row)?;
    b.size(l, w, LINE_H)?;
    b.long_mode(l, LongMode::Dot)?;
    b.style(l, Prop::TextColor, 0, colour)?;
    b.text(l, text)?;
    b.align(l, Align::LeftMid, x, y)?;
    Ok(l)
}

fn pill(b: &mut Batch, parent: Obj, text: &str, accent: bool, y: i16, w: i32) -> Result<Obj, LvError> {
    let p = b.create(Class::Obj, parent)?;
    b.size(p, w, 84)?;
    b.align(p, Align::TopMid, 0, y)?;
    b.style(p, Prop::Radius, 0, 42)?;
    b.style(p, Prop::BorderWidth, 0, 0)?;
    b.style(p, Prop::BgColor, 0, if accent { ACCENT } else { ROW_BG })?;
    b.style(p, Prop::BgOpa, 0, 255)?;
    b.style(p, Prop::BgColor, stock::PRESSED, ROW_PRESSED)?;
    b.flags(p, &[], &[Flag::Scrollable])?;
    b.events(p, &[Ev::Clicked])?;
    let l = b.create(Class::Label, p)?;
    b.style(l, Prop::TextColor, 0, WHITE)?;
    b.text(l, text)?;
    b.align(l, Align::Center, 0, 0)?;
    Ok(p)
}

impl Screen {
    /// The fonts, in a batch of their own.
    pub fn fonts(b: &mut Batch) -> Result<(), LvError> {
        stock::font(b)?;
        b.font(BIG, StockFont::F48M)
    }

    /// Every label in the system font, white; once, before the first view.
    pub fn theme(b: &mut Batch) -> Result<(), LvError> {
        stock::theme(b)
    }

    pub const fn empty() -> Screen {
        Screen { view: View::Start, page: ROOT, header: ROOT, hits: [(ROOT, Hit::Cancel); HITS], n_hits: 0, clock: ROOT, arc: ROOT, time: ROOT, shown: None, hr: ROOT, hr_shown: None }
    }

    /// Deletes the current view and builds `view`; batches go out through `flush` (`Batch::submit` on the watch)
    /// whenever one fills and at the end.
    pub fn show(&mut self, b: &mut Batch, flush: &mut impl FnMut(&mut Batch) -> Result<(), LvError>, m: &Model,
                view: View, now: u32) -> Result<(), LvError> {
        let mut room = |b: &mut Batch| if b.bytes().len() + GROUP > BUF { flush(b) } else { Ok(()) };
        if self.page != ROOT {
            b.del(self.page)?;
        }
        if self.header != ROOT {
            b.del(self.header)?;
        }
        *self = Screen { view, ..Screen::empty() };
        let (Some(p), w) = (&m.program, m.workout) else {
            self.page = b.create(Class::Obj, ROOT)?;
            b.size(self.page, SCREEN, SCREEN)?;
            bare(b, self.page)?;
            label(b, self.page, "Нет программы", WHITE, Align::Center, 0, -24, 0)?;
            label(b, self.page, "wp2apps workout push", GREY, Align::Center, 0, 24, 0)?;
            drop(room);
            return flush(b);
        };
        match view {
            View::Start | View::Day => {
                let mut title = Txt::<64>::new();
                if view == View::Start {
                    let _ = title.write_str("Тренировка");
                } else if let Some(w) = w {
                    let _ = write!(title, "День {}", w.day + 1);
                }
                self.header = b.create(Class::Obj, ROOT)?;
                b.size(self.header, SCREEN, HEADER_H)?;
                b.align(self.header, Align::TopMid, 0, 0)?;
                bare(b, self.header)?;
                b.style(self.header, Prop::BgColor, 0, BLACK)?;
                b.style(self.header, Prop::BgOpa, 0, 255)?;
                b.flags(self.header, &[], &[Flag::Clickable])?;
                label(b, self.header, title.as_str(), ACCENT, Align::TopMid, 0, 22, 0)?;
                if view == View::Day {
                    self.clock = label(b, self.header, "", GREY, Align::TopMid, -60, 58, 0)?;
                    self.hr = heart(b, self.header, 60, 58)?;
                }
                // a plain list like Custom Apps: rows keep their size while it scrolls
                let (page, _bar) = stock::page(b)?;
                b.size(page, SCREEN, PAGE_H)?;
                b.align(page, Align::BottomMid, 0, 0)?;
                b.style(page, Prop::PadTop, 0, stock::ROW_GAP)?;
                b.style(page, Prop::PadBottom, 0, PAGE_H - ROW_H)?; // the last row scrolls up to the top
                b.style(page, Prop::PadLeft, 0, (SCREEN - ROW_W) / 2)?;
                b.flex(page, Flow::Column, FlexAlign::Start, FlexAlign::Start, FlexAlign::Start)?;
                self.page = page;
                if view == View::Start {
                    // the next day first: the focus list opens on its first row
                    let next = m.next_day();
                    for k in 0..p.days() {
                        room(b)?;
                        let d = (next + k) % p.days();
                        let (mut name, mut sub) = (Txt::<64>::new(), Txt::<64>::new());
                        let _ = write!(name, "День {}: {}", d + 1, p.day_name(d));
                        let n = p.exercises(d);
                        if k == 0 {
                            let _ = sub.write_str("Следующая");
                        } else {
                            let _ = write!(sub, "{n} {}", exercises_word(n));
                        }
                        let row = self.row(b, name.as_str(), WHITE)?;
                        line(b, row, sub.as_str(), if k == 0 { ACCENT } else { GREY }, TEXT_X, 22, TEXT_W)?;
                        self.hit(row, Hit::Day(d));
                    }
                } else if let Some(w) = w {
                    let d = usize::from(w.day);
                    for i in 0..p.exercises(d) {
                        room(b)?;
                        let e = p.exercise(d, i);
                        let complete = w.done[i] >= e.sets;
                        let row = self.row(b, e.name, if complete { GREY } else { WHITE })?;
                        let dots = e.sets.min(DOTS_MAX);
                        for k in 0..dots {
                            let dot = b.create(Class::Obj, row)?;
                            b.size(dot, DOT, DOT)?;
                            b.align(dot, Align::LeftMid, TEXT_X + 22 * k as i16, 22)?;
                            b.style(dot, Prop::Radius, 0, CIRCLE)?;
                            b.style(dot, Prop::BorderWidth, 0, 0)?;
                            b.style(dot, Prop::BgColor, 0, if k < w.done[i] { ACCENT } else { LETTER_BG })?;
                            b.style(dot, Prop::BgOpa, 0, 255)?;
                            b.flags(dot, &[], &[Flag::Clickable, Flag::Scrollable])?;
                        }
                        let mut sub = Txt::<64>::new();
                        let mut r = Txt::<16>::new();
                        reps(e.reps.0, e.reps.1, "–", &mut r);
                        let _ = sub.write_str(r.as_str());
                        if !e.weight.is_empty() {
                            let _ = write!(sub, ", {}", e.weight);
                        }
                        let x = TEXT_X + 22 * dots as i16 + 6;
                        line(b, row, sub.as_str(), GREY, x, 22, ROW_W - i32::from(x) - 40)?;
                        self.hit(row, Hit::Exercise(i));
                    }
                    room(b)?;
                    let end = pill(b, self.page, "Завершить", true, 0, ROW_W)?;
                    self.hit(end, Hit::Finish);
                }
            }
            View::Set { ex } => {
                let Some(w) = w else { return flush(b) };
                let e = p.exercise(usize::from(w.day), ex);
                self.plain(b)?;
                self.hr = heart(b, self.page, 0, 16)?;
                label(b, self.page, e.name, GREY, Align::TopMid, 0, 62, 340)?;
                let mut t = Txt::<64>::new();
                let _ = write!(t, "Подход {} из {}", (w.done[ex] + 1).min(e.sets), e.sets);
                label(b, self.page, t.as_str(), WHITE, Align::TopMid, 0, 110, 0)?;
                let mut r = Txt::<16>::new();
                reps(e.reps.0, e.reps.1, "-", &mut r);
                let big = label(b, self.page, r.as_str(), WHITE, Align::TopMid, 0, 160, 0)?;
                b.style(big, Prop::TextFont, 0, i32::from(BIG))?;
                if !e.weight.is_empty() {
                    label(b, self.page, e.weight, GREY, Align::TopMid, 0, 232, 340)?;
                }
                room(b)?;
                let done = pill(b, self.page, "Сделал", true, 300, 240)?;
                self.hit(done, Hit::Done);
            }
            View::Rest => {
                let Some(w) = w else { return flush(b) };
                self.plain(b)?;
                self.arc = b.create(Class::Arc, self.page)?;
                b.size(self.arc, 446, 446)?;
                b.align(self.arc, Align::Center, 0, 0)?;
                b.angles(self.arc, 0, 360, 270)?;
                stock::arc(b, self.arc, 14)?;
                b.flags(self.arc, &[], &[Flag::Clickable])?;
                self.hr = heart(b, self.page, 0, 44)?;
                label(b, self.page, "Отдых", GREY, Align::TopMid, 0, 96, 0)?;
                self.time = label(b, self.page, "", WHITE, Align::TopMid, 0, 150, 0)?;
                b.style(self.time, Prop::TextFont, 0, i32::from(BIG))?;
                let mut t = Txt::<96>::new();
                let d = usize::from(w.day);
                match m.after_rest() {
                    Next::Set { ex, set } if ex == usize::from(w.rest_ex) => {
                        let _ = write!(t, "Дальше: подход {} из {}", set + 1, p.exercise(d, ex).sets);
                    }
                    Next::Set { ex, .. } => {
                        let _ = write!(t, "Дальше: {}", p.exercise(d, ex).name);
                    }
                    Next::Finished => {
                        let _ = t.write_str("Последний подход");
                    }
                }
                label(b, self.page, t.as_str(), GREY, Align::TopMid, 0, 232, 400)?;
                room(b)?;
                let skip = pill(b, self.page, "Пропустить", false, 300, 240)?;
                self.hit(skip, Hit::Skip);
            }
            View::Ask => {
                self.plain(b)?;
                let q = label(b, self.page, "Завершить тренировку?", WHITE, Align::TopMid, 0, 64, 320)?;
                b.size(q, 320, 2 * LINE_H)?;
                b.long_mode(q, LongMode::Wrap)?;
                let leave = pill(b, self.page, "Выйти", false, 156, 300)?;
                self.hit(leave, Hit::Leave);
                room(b)?;
                let end = pill(b, self.page, "Завершить", true, 252, 300)?;
                self.hit(end, Hit::End);
                let cancel = pill(b, self.page, "Отмена", false, 348, 220)?;
                self.hit(cancel, Hit::Cancel);
            }
            View::Done => {
                self.plain(b)?;
                label(b, self.page, "Готово", ACCENT, Align::TopMid, 0, 80, 0)?;
                if let Some(e) = m.history().next() {
                    let mut t = Txt::<16>::new();
                    clock(u32::from(e.dur_s), &mut t);
                    let big = label(b, self.page, t.as_str(), WHITE, Align::TopMid, 0, 134, 0)?;
                    b.style(big, Prop::TextFont, 0, i32::from(BIG))?;
                    let mut s = Txt::<64>::new();
                    let _ = write!(s, "{} из {} {}", e.done, e.planned, sets_word(usize::from(e.planned)));
                    label(b, self.page, s.as_str(), GREY, Align::TopMid, 0, 214, 0)?;
                }
                room(b)?;
                let leave = pill(b, self.page, "Выйти", false, 300, 220)?;
                self.hit(leave, Hit::Leave);
            }
        }
        self.tick(b, &mut room, m, now)?;
        drop(room);
        flush(b)
    }

    /// The day clock and the rest countdown, once per second.
    pub fn tick(&mut self, b: &mut Batch, flush: &mut impl FnMut(&mut Batch) -> Result<(), LvError>, m: &Model,
                now: u32) -> Result<(), LvError> {
        let Some(w) = m.workout else { return Ok(()) };
        let mut t = Txt::<16>::new();
        if self.hr != ROOT && self.hr_shown != Some(m.bpm) {
            if m.bpm == 0 {
                let _ = t.write_str("--");
            } else {
                let _ = write!(t, "{}", m.bpm);
            }
            b.text(self.hr, t.as_str())?;
            self.hr_shown = Some(m.bpm);
            t = Txt::new();
            flush(b)?;
        }
        let shown = match self.view {
            View::Day if self.clock != ROOT => {
                let s = now.wrapping_sub(w.started).min(99 * 3600);
                if self.shown != Some(s) {
                    clock(s, &mut t);
                    b.text(self.clock, t.as_str())?;
                }
                s
            }
            View::Rest if self.time != ROOT => {
                let left = m.rest_left(now);
                if self.shown != Some(left) {
                    let total = m.program.as_ref().map_or(1, |p| {
                        let d = usize::from(w.day);
                        let ex = usize::from(w.rest_ex).min(p.exercises(d) - 1);
                        u32::from(p.exercise(d, ex).rest_s).max(1)
                    });
                    clock(left, &mut t);
                    b.text(self.time, t.as_str())?;
                    b.value(self.arc, (left.min(total) * 100 / total) as i32, false)?;
                }
                left
            }
            _ => return Ok(()),
        };
        if self.shown != Some(shown) {
            self.shown = Some(shown);
            flush(b)?;
        }
        Ok(())
    }

    /// What a CLICKED handle means here.
    pub fn hit_of(&self, h: u16) -> Option<Hit> {
        self.hits[..self.n_hits].iter().find(|(o, _)| o.0 == h).map(|&(_, hit)| hit)
    }

    fn hit(&mut self, o: Obj, hit: Hit) {
        if self.n_hits < HITS {
            self.hits[self.n_hits] = (o, hit);
            self.n_hits += 1;
        }
    }

    fn row(&mut self, b: &mut Batch, name: &str, colour: i32) -> Result<Obj, LvError> {
        let row = stock::focus_card(b, self.page, ROW_H)?; // a clickable card; the page is not a focus page
        line(b, row, name, colour, TEXT_X, -18, TEXT_W)?;
        Ok(row)
    }

    fn plain(&mut self, b: &mut Batch) -> Result<(), LvError> {
        self.page = b.create(Class::Obj, ROOT)?;
        b.size(self.page, SCREEN, SCREEN)?;
        b.align(self.page, Align::Center, 0, 0)?;
        bare(b, self.page)
    }
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use crate::model::tests::{enc, Ex};
    use crate::model::Program;
    use std::boxed::Box;
    use std::string::String;
    use wp2_core::lvcmd::{self, Table};

    struct Loader {
        t: Table,
        batches: usize,
    }

    impl Loader {
        fn flush(&mut self, b: &mut Batch) -> Result<(), LvError> {
            if b.bytes().is_empty() {
                return Ok(());
            }
            assert_eq!(lvcmd::validate(b.bytes(), &self.t, 0b11), Ok(()));
            for r in lvcmd::records(b.bytes()) {
                lvcmd::apply(&mut self.t, &r.unwrap());
            }
            self.batches += 1;
            b.clear();
            Ok(())
        }
    }

    fn big_model() -> Model {
        let name = "Жим гантелей лёжа"; // 30 B
        let exs: std::vec::Vec<Ex> = (0..12).map(|_| (name, 9, 10, 12, 90, "гантели по 8–10 кг")).collect();
        let mut m = Model::new();
        m.program = Program::parse(&enc(&[("Тяга и наклонный", &exs[..]), ("Б", &exs[..2])]));
        assert!(m.program.is_some());
        m
    }

    #[test]
    fn every_view_fits_the_budgets() {
        let mut b = Batch::with_buf(Box::leak(Box::new([0; BUF])));
        let mut f = Batch::with_buf(Box::leak(Box::new([0; BUF])));
        Screen::fonts(&mut f).unwrap();
        assert_eq!(lvcmd::validate(f.bytes(), &Table::new(), 0), Ok(()));
        let mut l = Loader { t: Table::new(), batches: 0 };
        let mut s = Screen::empty();
        let mut m = big_model();
        s.show(&mut b, &mut |b| l.flush(b), &m, View::Start, 0).unwrap();
        assert_eq!(s.hit_of(s.hits[0].0 .0), Some(Hit::Day(0)));
        m.start(0, 100);
        m.done_set(3, 110);
        for v in [View::Day, View::Set { ex: 3 }, View::Rest, View::Ask, View::Day, View::Start] {
            s.show(&mut b, &mut |b| l.flush(b), &m, v, 120).unwrap();
            assert_eq!(s.view, v);
        }
        // a day of 12 exercises: 12 rows and "Завершить"
        s.show(&mut b, &mut |b| l.flush(b), &m, View::Day, 120).unwrap();
        assert_eq!(s.n_hits, 13);
        assert_eq!(s.hit_of(s.hits[12].0 .0), Some(Hit::Finish));
        m.finish(4000);
        s.show(&mut b, &mut |b| l.flush(b), &m, View::Done, 4000).unwrap();
        // no program
        s.show(&mut b, &mut |b| l.flush(b), &Model::new(), View::Start, 0).unwrap();
        std::println!("{} batches", l.batches);
    }

    #[test]
    fn start_lists_the_next_day_first() {
        let mut b = Batch::with_buf(Box::leak(Box::new([0; BUF])));
        let mut l = Loader { t: Table::new(), batches: 0 };
        let mut m = big_model();
        m.start(0, 1);
        m.done_set(0, 2);
        m.finish(3);
        let mut s = Screen::empty();
        s.show(&mut b, &mut |b| l.flush(b), &m, View::Start, 0).unwrap();
        assert_eq!((s.hits[0].1, s.hits[1].1), (Hit::Day(1), Hit::Day(0)));
    }

    #[test]
    fn tick_sends_only_on_a_new_second() {
        let mut b = Batch::with_buf(Box::leak(Box::new([0; BUF])));
        let mut l = Loader { t: Table::new(), batches: 0 };
        let mut m = big_model();
        m.start(0, 100);
        m.done_set(0, 100);
        let mut s = Screen::empty();
        s.show(&mut b, &mut |b| l.flush(b), &m, View::Rest, 100).unwrap();
        let n = l.batches;
        s.tick(&mut b, &mut |b| l.flush(b), &m, 100).unwrap();
        assert_eq!(l.batches, n);
        s.tick(&mut b, &mut |b| l.flush(b), &m, 101).unwrap();
        assert_eq!(l.batches, n + 1);
    }

    #[test]
    fn texts() {
        let mut t = Txt::<16>::new();
        clock(67, &mut t);
        assert_eq!(t.as_str(), "1:07");
        let mut t = Txt::<16>::new();
        clock(3 * 3600 + 5, &mut t);
        assert_eq!(t.as_str(), "3:00:05");
        let words: std::vec::Vec<&str> = [1, 2, 5, 11, 21, 22].iter().map(|&n| exercises_word(n)).collect();
        assert_eq!(words, ["упражнение", "упражнения", "упражнений", "упражнений", "упражнение", "упражнения"]);
        assert_eq!((sets_word(21), sets_word(22), sets_word(11)), ("подхода", "подходов", "подходов"));
        let mut t = Txt::<4>::new();
        let _ = t.write_str("ая"); // 4 bytes fit
        let _ = t.write_str("б");
        assert_eq!(t.as_str(), "ая");
        let _: String = String::new();
    }
}
