//! The herdr-agents screen on the watch's own widgets (ABI 1.2, `cmf_wp2_app::stock`): a black header with the
//! blocked / done / working counts and the status line, under it a focus page of agents (the stock app list's
//! fisheye: the agent nearest the centre sits on a pill, the others shift right and fade; the crown scrolls).
//! A row: status dot (the part the fisheye scales), title, workspace in grey. Rows are kept in step with the model
//! by `sync`, which only sends what changed.

use crate::model::{Model, Status, MAX};
use cmf_wp2_app::lv::*;
use cmf_wp2_app::stock;

/// Font slots: the system font for titles, a smaller stock font for workspaces, counts and the status line.
pub const SMALL: u8 = 1;
pub const DOT: i32 = 36;
const DOT_X: i16 = 24;
const TEXT_X: i16 = DOT_X + DOT as i16 + 20;
const TEXT_W: i32 = stock::ROW_W - TEXT_X as i32 - 40;
pub const HEADER_H: i32 = 104;
/// The list's page: under the header; its centre line is where the selected agent sits.
pub const PAGE_H: i32 = stock::SCREEN - HEADER_H;
// the status line: blocked red at 200/256
const WARN: i32 = 0xC1E3;
// bytes one row update can take: a submit first when fewer are left
const ROW_BYTES: usize = 420;

pub fn colour(s: Status, level: u32) -> i32 {
    let (r, g, b) = match s {
        Status::Blocked => (255, 80, 40),
        Status::Done => (60, 220, 110),
        Status::Working => (60, 140, 255),
        Status::Idle => (110, 110, 110),
        Status::Unknown => (60, 60, 60),
    };
    let (r, g, b) = (r * level / 256, g * level / 256, b * level / 256);
    ((r >> 3) << 11 | (g >> 2) << 5 | b >> 3) as i32
}

/// Working dots pulse between 150/256 and full over 1.2 s.
pub fn pulse(now: u32) -> u32 {
    let t = (now % 1200) as i32;
    (150 + (106 * (600 - (t - 600).abs()) / 600)) as u32
}

pub type Line = Option<(&'static str, u32)>;

// the daemon beats every 10 s
pub const STALE_MS: u32 = 25_000;

/// "waiting for Mac..." until the first message, "stale N s" after 25 s of silence.
pub fn status_line(heard: Option<u32>, now: u32) -> Line {
    match heard {
        None => Some(("waiting for Mac...", 0)),
        Some(t) if now.wrapping_sub(t) > STALE_MS => Some(("stale", now.wrapping_sub(t) / 1000)),
        _ => None,
    }
}

#[derive(Clone, Copy)]
struct Row {
    row: Obj,
    dot: Obj,
    title: Obj,
    ws: Obj,
    // what it shows: hash of id, status, title and workspace; the dot's colour
    shown: u32,
    dot_colour: i32,
}

pub struct Screen {
    pub page: Obj,
    pub bar: Obj,
    counts: [Obj; 3],
    line: Obj,
    rows: [Row; MAX],
    pub n: usize,
    last_counts: Option<(usize, usize, usize)>,
    last_line: Option<Line>,
}

fn hash(bytes: &[u8], h: u32) -> u32 {
    bytes.iter().fold(h, |h, &c| (h ^ c as u32).wrapping_mul(16_777_619))
}

impl Screen {
    /// The fonts, in a batch of their own.
    pub fn fonts(b: &mut Batch) -> Result<(), LvError> {
        stock::font(b)?;
        b.font(SMALL, StockFont::F24M)
    }

    /// The header, then the page under it (from the header down, so rows never pass under the header and the
    /// round scroll bar, created after both, stays whole); rows come with `sync`.
    pub fn build(b: &mut Batch) -> Result<Screen, LvError> {
        stock::theme(b)?;
        let header = b.create(Class::Obj, ROOT)?;
        b.size(header, stock::SCREEN, HEADER_H)?;
        b.align(header, Align::TopMid, 0, 0)?;
        for p in [Prop::PadTop, Prop::PadBottom, Prop::PadLeft, Prop::PadRight, Prop::BorderWidth, Prop::Radius] {
            b.style(header, p, 0, 0)?;
        }
        b.style(header, Prop::BgColor, 0, stock::BLACK)?;
        b.style(header, Prop::BgOpa, 0, 255)?;
        b.style(header, Prop::TextFont, 0, SMALL as i32)?;
        b.flags(header, &[], &[Flag::Scrollable, Flag::Clickable])?;
        let mut counts = [ROOT; 3];
        for (k, (count, st)) in counts.iter_mut().zip([Status::Blocked, Status::Done, Status::Working]).enumerate() {
            let x = -56 + 56 * k as i16;
            let dot = b.create(Class::Obj, header)?;
            b.size(dot, 16, 16)?;
            b.align(dot, Align::TopMid, x - 14, 44)?;
            b.style(dot, Prop::Radius, 0, stock::CIRCLE)?;
            b.style(dot, Prop::BgColor, 0, colour(st, 256))?;
            b.style(dot, Prop::BgOpa, 0, 255)?;
            b.style(dot, Prop::BorderWidth, 0, 0)?;
            *count = b.create(Class::Label, header)?;
            b.text(*count, "0")?;
            b.align(*count, Align::TopMid, x + 8, 38)?;
        }
        let line = b.create(Class::Label, header)?;
        b.style(line, Prop::TextColor, 0, WARN)?;
        b.text(line, "")?;
        b.align(line, Align::TopMid, 0, 70)?;
        let (page, bar) = stock::focus_page(b, DOT as u16)?;
        b.size(page, stock::SCREEN, PAGE_H)?;
        b.align(page, Align::BottomMid, 0, 0)?;
        // the first and the last row can reach the page's centre line
        b.style(page, Prop::PadTop, 0, (PAGE_H - stock::FOCUS_ROW_H) / 2)?;
        b.style(page, Prop::PadBottom, 0, (PAGE_H - stock::FOCUS_ROW_H) / 2)?;
        let none = Row { row: ROOT, dot: ROOT, title: ROOT, ws: ROOT, shown: 0, dot_colour: -1 };
        Ok(Screen { page, bar, counts, line, rows: [none; MAX], n: 0, last_counts: None, last_line: None })
    }

    /// Rows for every agent of `model`, then header and status line; sends only what changed, through `flush`
    /// (`Batch::submit` on the watch) whenever the batch fills and at the end. Working dots pulse with `now`.
    pub fn sync(&mut self, b: &mut Batch, flush: &mut impl FnMut(&mut Batch) -> Result<(), LvError>, model: &Model,
                line: Line, now: u32) -> Result<(), LvError> {
        let mut room = |b: &mut Batch| if b.bytes().len() + ROW_BYTES > BUF { flush(b) } else { Ok(()) };
        while self.n > model.len() {
            room(b)?;
            self.n -= 1;
            b.del(self.rows[self.n].row)?;
        }
        while self.n < model.len() {
            room(b)?;
            let row = stock::focus_row(b, self.page)?;
            let dot = b.create(Class::Obj, row)?;
            b.size(dot, DOT, DOT)?;
            b.align(dot, Align::LeftMid, DOT_X, 0)?;
            b.style(dot, Prop::Radius, 0, stock::CIRCLE)?;
            b.style(dot, Prop::BgOpa, 0, 255)?;
            b.style(dot, Prop::BorderWidth, 0, 0)?;
            b.flags(dot, &[], &[Flag::Clickable, Flag::Scrollable])?;
            let title = b.create(Class::Label, row)?;
            b.size(title, TEXT_W, 40)?;
            b.long_mode(title, LongMode::Dot)?;
            b.align(title, Align::LeftMid, TEXT_X, -18)?;
            let ws = b.create(Class::Label, row)?;
            b.size(ws, TEXT_W, 30)?;
            b.long_mode(ws, LongMode::Dot)?;
            b.align(ws, Align::LeftMid, TEXT_X, 22)?;
            b.style(ws, Prop::TextFont, 0, SMALL as i32)?;
            b.style(ws, Prop::TextColor, 0, stock::GREY)?;
            self.rows[self.n] = Row { row, dot, title, ws, shown: 0, dot_colour: -1 };
            self.n += 1;
        }
        for i in 0..self.n {
            let Some(a) = model.get(i) else { break };
            let r = &mut self.rows[i];
            let shown = hash(a.ws().as_bytes(), hash(a.title().as_bytes(), hash(&[a.id, a.status as u8], 2_166_136_261)));
            if r.shown != shown {
                room(b)?;
                b.text(r.title, a.title())?;
                b.text(r.ws, a.ws())?;
                r.shown = shown;
            }
            let c = colour(a.status, if a.status == Status::Working { pulse(now) } else { 256 });
            if r.dot_colour != c {
                room(b)?;
                b.style(r.dot, Prop::BgColor, 0, c)?;
                r.dot_colour = c;
            }
        }
        let counts = model.counts();
        if self.last_counts != Some(counts) {
            room(b)?;
            let (w, bl, d) = counts;
            for (label, n) in self.counts.iter().zip([bl, d, w]) {
                let mut buf = [0u8; 11];
                b.text(*label, cmf_wp2_ui::text::int(n.min(999) as i32, &mut buf))?;
            }
            self.last_counts = Some(counts);
        }
        if self.last_line != Some(line) {
            room(b)?;
            let mut buf = [0u8; 32];
            b.text(self.line, line_text(line, &mut buf))?;
            self.last_line = Some(line);
        }
        drop(room);
        flush(b)
    }

    /// The model index of a row handle (a CLICKED event's `x`).
    pub fn row_of(&self, h: u16) -> Option<usize> {
        self.rows[..self.n].iter().position(|r| r.row.0 == h)
    }
}

/// "waiting for Mac...", "stale 42s" or "".
pub fn line_text(line: Line, buf: &mut [u8; 32]) -> &str {
    let Some((label, secs)) = line else { return "" };
    let mut n = label.len();
    buf[..n].copy_from_slice(label.as_bytes());
    if secs > 0 {
        let mut d = [0u8; 11];
        let s = cmf_wp2_ui::text::int(secs.min(999) as i32, &mut d);
        buf[n] = b' ';
        buf[n + 1..n + 1 + s.len()].copy_from_slice(s.as_bytes());
        n += 1 + s.len();
        buf[n] = b's';
        n += 1;
    }
    core::str::from_utf8(&buf[..n]).unwrap_or("")
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use std::boxed::Box;
    use std::vec::Vec;
    use wp2_core::lvcmd::{self, Table};

    fn msg(kind: u8, seq: u8, body: &[u8]) -> Vec<u8> {
        let mut m = Vec::from(&b"h1"[..]);
        m.extend([kind, seq]);
        m.extend(body);
        m
    }

    fn model(n: usize) -> Model {
        let mut m = Model::new();
        m.apply(&msg(b'B', 10, &[]));
        for k in 0..n {
            let title = std::format!("Агент номер {k} с длинным заголовком");
            let mut b = Vec::from([k as u8 + 1, (k % 4) as u8, 5]);
            b.extend(b"infra");
            b.extend(title.bytes());
            m.apply(&msg(b'U', 11 + k as u8, &b));
        }
        m.apply(&msg(b'E', 11 + n as u8, &[n as u8]));
        m
    }

    // what the loader would do with each flushed batch: validate against its table, apply, count
    struct Loader {
        t: Table,
        batches: usize,
        bytes: usize,
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
            (self.batches, self.bytes) = (self.batches + 1, self.bytes + b.bytes().len());
            b.clear();
            Ok(())
        }
    }

    #[test]
    fn full_list_fits_the_budgets_and_updates_only_changes() {
        let mut b = Batch::with_buf(Box::leak(Box::new([0; BUF])));
        let mut f = Batch::with_buf(Box::leak(Box::new([0; BUF])));
        Screen::fonts(&mut f).unwrap();
        assert_eq!(lvcmd::validate(f.bytes(), &Table::new(), 0), Ok(()));
        let mut l = Loader { t: Table::new(), batches: 0, bytes: 0 };
        let mut s = Screen::build(&mut b).unwrap();
        l.flush(&mut b).unwrap();
        let m = model(MAX);
        s.sync(&mut b, &mut |b| l.flush(b), &m, status_line(None, 0), 0).unwrap();
        std::println!("{MAX} agents: {} batches, {} B", l.batches, l.bytes);
        assert_eq!(s.n, MAX);
        // nothing changed (idle dots, same second): nothing sent
        let before = l.batches;
        s.sync(&mut b, &mut |b| l.flush(b), &m, status_line(None, 0), 0).unwrap();
        // working dots pulse: only their colours
        s.sync(&mut b, &mut |b| l.flush(b), &m, status_line(None, 0), 300).unwrap();
        assert!(l.batches <= before + 1);
        // fewer agents: rows go (DEL), the rest keep their handles
        let first = s.rows[0].row;
        s.sync(&mut b, &mut |b| l.flush(b), &model(3), status_line(Some(0), 1), 1).unwrap();
        assert_eq!((s.n, s.rows[0].row), (3, first));
        assert_eq!(s.row_of(first.0), Some(0));
        assert_eq!(l.t.class(s.rows[3].row.0), None, "row 3 freed");
    }

    #[test]
    fn line_texts() {
        let mut buf = [0u8; 32];
        assert_eq!(line_text(status_line(None, 0), &mut buf), "waiting for Mac...");
        assert_eq!(line_text(status_line(Some(0), STALE_MS + 42_000), &mut buf), "stale 67s");
        assert_eq!(line_text(status_line(Some(0), 1000), &mut buf), "");
    }
}
