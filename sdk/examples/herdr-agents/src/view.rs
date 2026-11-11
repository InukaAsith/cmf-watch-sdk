// The screen of herdr-agents on the UI kit: header counts, the status line, a 5-row list that follows the round
// edge, "+N" below. Each part is repainted only when it changes; working dots pulse on their own.

use crate::model::{Model, Status};
use cmf_wp2_ui::color::Rgb;
use cmf_wp2_ui::draw::disc;
use cmf_wp2_ui::text::{fitted, int};
use cmf_wp2_ui::widget::List;
use cmf_wp2_ui::{C, CHAR, SIZE, Surface};

pub const LIST_TOP: i32 = 112;
pub const ROW_H: i32 = 58;
pub const ROWS: usize = 5;
pub const FLASH_MS: u32 = 300;
// the daemon beats every 10 s
pub const STALE_MS: u32 = 25_000;

const BG: u16 = 0x0000;
const FG: u16 = 0xFFFF;
const DIM: u16 = 0x8C71;
const HILITE: u16 = 0x2124;
// the status line: blocked red at 200/256
const WARN: u16 = 0xC1E3;
// old dots were drawn 1/2 px wider than r = 7; 8 keeps their size
const DOT_R: i32 = 8;
const BLACK: Rgb = Rgb(0, 0, 0);
// parts of the screen: [y, h]
const HEADER: [i32; 2] = [36, 28];
const LINE: [i32; 2] = [72, 24];
const FOOTER: [i32; 2] = [406, 24];

fn colour(s: Status, level: u32) -> Rgb {
    let (r, g, b) = match s {
        Status::Blocked => (255, 80, 40),
        Status::Done => (60, 220, 110),
        Status::Working => (60, 140, 255),
        Status::Idle => (110, 110, 110),
        Status::Unknown => (60, 60, 60),
    };
    let (r, g, b) = (r * level / 256, g * level / 256, b * level / 256);
    // the app's original truncating 565, widened back exactly
    Rgb::from565(((r >> 3) << 11 | (g >> 2) << 5 | b >> 3) as u16)
}

fn pulse(now: u32) -> u32 {
    let t = (now % 1200) as i32;
    (150 + (106 * (600 - (t - 600).abs()) / 600)) as u32
}

type Line = Option<(&'static str, u32)>;

pub struct Ui {
    pub model: Model,
    pub list: List,
    pub heard: Option<u32>,
    flash: Option<(usize, u32)>,
    full: bool,
    counts: Option<(usize, usize, usize)>,
    line: Option<Line>,
    more: Option<usize>,
}

impl Ui {
    pub fn new() -> Self {
        Ui { model: Model::new(), list: List::new(LIST_TOP, ROW_H, ROWS, 6), heard: None, flash: None, full: true, counts: None, line: None, more: None }
    }

    /// The model changed: the list is redrawn.
    pub fn changed(&mut self) {
        self.list.set_len(self.model.len());
    }

    /// Everything is repainted on the next draw (start, resume).
    pub fn redraw(&mut self) {
        self.full = true;
    }

    pub fn status_line(&self, now: u32) -> Line {
        match self.heard {
            None => Some(("waiting for Mac...", 0)),
            Some(t) if now.wrapping_sub(t) > STALE_MS => Some(("stale", now.wrapping_sub(t) / 1000)),
            _ => None,
        }
    }

    pub fn crown(&mut self, delta: i16) -> bool {
        self.list.crown(delta)
    }

    /// The agent id under a touch at `y`; its row lights up for [`FLASH_MS`].
    pub fn tap(&mut self, y: i32, now: u32) -> Option<u8> {
        let i = self.list.hit(y)?;
        if let Some((old, _)) = self.flash {
            self.list.invalidate(old);
        }
        self.flash = Some((i, now.wrapping_add(FLASH_MS)));
        self.list.invalidate(i);
        self.model.get(i).map(|a| a.id)
    }

    fn flashed(&self, i: usize) -> bool {
        self.flash.is_some_and(|(f, _)| f == i)
    }

    /// Paints what changed; `true` if anything was drawn (then `show`).
    pub fn draw(&mut self, s: &mut impl Surface, now: u32) -> bool {
        if let Some((i, t)) = self.flash
            && (now.wrapping_sub(t) as i32) >= 0
        {
            self.flash = None;
            self.list.invalidate(i);
        }
        let mut any = false;
        if core::mem::take(&mut self.full) {
            s.rect(0, 0, SIZE, SIZE, BG);
            (self.counts, self.line, self.more) = (None, None, None);
            self.list.invalidate_all();
            any = true;
        }
        let counts = self.model.counts();
        if self.counts != Some(counts) {
            self.counts = Some(counts);
            s.rect(0, HEADER[0], SIZE, HEADER[1], BG);
            let (w, b, d) = counts;
            let mut x = C - 3 * 56 / 2;
            for (n, st) in [(b, Status::Blocked), (d, Status::Done), (w, Status::Working)] {
                disc(s, x + 8, 52, DOT_R, colour(st, 256), BLACK);
                let mut buf = [0u8; 11];
                s.text(x + 20, 44, int(n.min(999) as i32, &mut buf), if n > 0 { FG } else { DIM });
                x += 56;
            }
            any = true;
        }
        let line = self.status_line(now);
        if self.line != Some(line) {
            self.line = Some(line);
            s.rect(0, LINE[0], SIZE, LINE[1], BG);
            if let Some((label, secs)) = line {
                let mut buf = [0u8; 11];
                let n = if secs > 0 { int(secs.min(999) as i32, &mut buf) } else { "" };
                let chars = label.len() + if secs > 0 { n.len() + 2 } else { 0 };
                let x0 = C - chars as i32 * CHAR / 2;
                let c = WARN;
                s.text(x0, 76, label, c);
                if secs > 0 {
                    let x1 = x0 + (label.len() as i32 + 1) * CHAR;
                    s.text(x1, 76, n, c);
                    s.text(x1 + n.len() as i32 * CHAR, 76, "s", c);
                }
            }
            any = true;
        }
        let (model, flash) = (&self.model, self.flash);
        any |= self.list.draw(s, BG, |s, i, row| {
            let Some(a) = model.get(i) else { return };
            let (dx, tx) = (row.x0 + 8, row.x0 + 22);
            let cells = row.cells(tx);
            let lit = flash.is_some_and(|(f, _)| f == i);
            if lit {
                s.rect(dx - 14, row.y + 2, tx + cells as i32 * CHAR - dx + 20, ROW_H - 4, HILITE);
            }
            let level = if a.status == Status::Working { pulse(now) } else { 256 };
            disc(s, dx, row.y + ROW_H / 2, DOT_R, colour(a.status, level), Rgb::from565(if lit { HILITE } else { BG }));
            fitted(s, tx, row.y + 9, a.title(), cells, FG);
            fitted(s, tx, row.y + 31, a.ws(), cells, DIM);
        });
        let more = self.list.more();
        if self.more != Some(more) {
            self.more = Some(more);
            s.rect(0, FOOTER[0], SIZE, FOOTER[1], BG);
            if more > 0 {
                let mut buf = [0u8; 11];
                let n = int(more.min(999) as i32, &mut buf);
                let x = C - (n.len() as i32 + 1) * CHAR / 2;
                s.text(x, 410, "+", DIM);
                s.text(x + CHAR, 410, n, DIM);
            }
            any = true;
        }
        any
    }

    /// Repaints only the dots of visible working agents; `true` if any.
    pub fn animate(&self, s: &mut impl Surface, now: u32) -> bool {
        let mut any = false;
        for r in 0..ROWS {
            let i = self.list.scroll() + r;
            let Some(a) = self.model.get(i) else { break };
            if a.status == Status::Working && !self.flashed(i) {
                let row = self.list.row(r);
                disc(s, row.x0 + 8, row.y + ROW_H / 2, DOT_R, colour(a.status, pulse(now)), BLACK);
                any = true;
            }
        }
        any
    }
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use cmf_wp2_ui::Fb;
    use std::vec;
    use std::vec::Vec;

    fn msg(kind: u8, seq: u8, body: &[u8]) -> Vec<u8> {
        let mut m = Vec::from(&b"h1"[..]);
        m.extend([kind, seq]);
        m.extend(body);
        m
    }

    fn load(ui: &mut Ui, agents: &[(u8, u8, &str, &str)]) {
        ui.model = Model::new();
        ui.model.apply(&msg(b'B', 10, &[]));
        for (k, &(id, st, ws, t)) in agents.iter().enumerate() {
            let mut b = Vec::from([id, st, ws.len() as u8]);
            b.extend(ws.bytes());
            b.extend(t.bytes());
            ui.model.apply(&msg(b'U', 11 + k as u8, &b));
        }
        ui.model.apply(&msg(b'E', 11 + agents.len() as u8, &[agents.len() as u8]));
        ui.changed();
    }

    const AGENTS: &[(u8, u8, &str, &str)] = &[(1, 1, "проект", "Рефакторинг парсера"), (2, 0, "проект", "Обзор кода"),
        (3, 3, "infra", "Write the release notes for version two"), (4, 2, "infra", "Нужно подтверждение"),
        (5, 1, "infra", "tests"), (6, 1, "x", "six"), (7, 0, "x", "seven")];

    fn full(ui: &Ui, now: u32) -> Vec<u16> {
        let mut fresh = Ui::new();
        fresh.model = Model::new();
        let mut px = vec![0x1234u16; (SIZE * SIZE) as usize];
        load(&mut fresh, &AGENTS[..ui.model.len()]);
        fresh.heard = ui.heard;
        for _ in 0..ui.list.scroll() {
            fresh.crown(1);
        }
        fresh.flash = ui.flash;
        fresh.draw(&mut Fb::new(&mut px), now);
        px
    }

    // tests/test_herdr_e2e.py samples these
    #[test]
    fn rows_follow_the_round_edge() {
        let ui = Ui::new();
        let dots: Vec<i32> = (0..ROWS).map(|r| ui.list.row(r).x0 + 8).collect();
        assert_eq!(dots, [48, 23, 21, 43, 87]);
        for r in 0..ROWS {
            let row = ui.list.row(r);
            assert!(row.cells(row.x0 + 22) >= 14, "row {r}");
        }
    }

    // every change repaints only its part, and the screen ends up as one full redraw of the same state
    #[test]
    fn partial_redraws_match_a_full_redraw() {
        let mut ui = Ui::new();
        let mut px = vec![0u16; (SIZE * SIZE) as usize];
        let mut fb = Fb::new(&mut px);
        assert!(ui.draw(&mut fb, 0) && !ui.draw(&mut fb, 10));
        load(&mut ui, &AGENTS[..5]);
        ui.draw(&mut fb, 100);
        ui.heard = Some(100);
        fb.rects = 0;
        assert!(ui.draw(&mut fb, 200) && fb.rects < 10, "status line only: {} rects", fb.rects);
        load(&mut ui, AGENTS);
        assert!(ui.crown(1));
        ui.draw(&mut fb, 300);
        assert_eq!(ui.tap(LIST_TOP + ROW_H + 5, 300), Some(5));
        fb.rects = 0;
        ui.draw(&mut fb, 310);
        assert!(fb.rects < 200, "one row: {} rects", fb.rects);
        ui.animate(&mut fb, 310);
        assert!(fb.px == full(&ui, 310).as_slice(), "flash");
        ui.draw(&mut fb, 700);
        assert!(ui.flash.is_none() && ui.animate(&mut fb, 700));
        assert!(fb.px == full(&ui, 700).as_slice(), "flash ended");
        let stale = 100 + STALE_MS + 5000;
        ui.draw(&mut fb, stale);
        ui.animate(&mut fb, stale);
        assert!(fb.px == full(&ui, stale).as_slice(), "stale");
        load(&mut ui, &AGENTS[..2]);
        ui.draw(&mut fb, stale);
        ui.animate(&mut fb, stale);
        assert!(fb.px == full(&ui, stale).as_slice(), "shrunk");
    }
}
