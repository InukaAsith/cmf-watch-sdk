// The screen of herdr-agents on the UI kit: header counts, the status line, the agents in a focus list (the selected
// one on the centre line, highlighted, with a bigger title; the crown moves the selection), "n/N" below. Each part
// is repainted only when it changes; working dots pulse on their own.

use crate::model::{Model, Status};
use cmf_wp2_ui::color::Rgb;
use cmf_wp2_ui::draw::{disc, rrect};
use cmf_wp2_ui::text::{big, clip, ellipsis, fitted, int};
use cmf_wp2_ui::widget::{Focus, FocusList, GLIDE_MS};
use cmf_wp2_ui::{C, CHAR, ONE, SIZE, Surface};

// the list band, under the status line and above the footer; the focus line is its middle
pub const LIST_TOP: i32 = 98;
pub const LIST_BOTTOM: i32 = 402;
pub const FOCUS_Y: i32 = (LIST_TOP + LIST_BOTTOM) / 2;
pub const PITCH: i32 = 60;
pub const FLASH_MS: u32 = 300;
// the daemon beats every 10 s
pub const STALE_MS: u32 = 25_000;

const BG: u16 = 0x0000;
const FG: u16 = 0xFFFF;
const DIM: u16 = 0x8C71;
// the selected row: the stock row colour; pressed: the stock pressed row
const SELECTED: Rgb = Rgb(41, 44, 49);
const PRESSED: Rgb = Rgb(66, 69, 74);
// the status line: blocked red at 200/256
const WARN: u16 = 0xC1E3;
// old dots were drawn 1/2 px wider than r = 7; 8 keeps their size
const DOT_R: i32 = 8;
const BLACK: Rgb = Rgb(0, 0, 0);
// the selected title is drawn at 3 px per font pixel (24 px chars), the rest at 2
const BIG: i32 = 3;
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
    pub list: FocusList,
    pub heard: Option<u32>,
    // the selected agent's id: the selection follows it when the list is reordered
    sel_id: Option<u8>,
    flash: Option<(usize, u32)>,
    full: bool,
    counts: Option<(usize, usize, usize)>,
    line: Option<Line>,
    footer: Option<(usize, usize)>,
}

/// Where item `i` draws its dot, and the colour behind it.
fn dot_at(f: &Focus) -> i32 {
    f.x0 + 22
}

fn row_bg(f: &Focus, lit: bool) -> Rgb {
    if lit { PRESSED } else { BLACK.mix(SELECTED, f.t) }
}

impl Ui {
    pub const fn new() -> Self {
        Ui {
            model: Model::new(),
            list: FocusList::new(LIST_TOP, LIST_BOTTOM, FOCUS_Y, PITCH, 6),
            heard: None,
            sel_id: None,
            flash: None,
            full: true,
            counts: None,
            line: None,
            footer: None,
        }
    }

    /// The model changed: the list is redrawn, the selection stays on the same agent.
    pub fn changed(&mut self) {
        self.list.set_len(self.model.len());
        let n = self.model.len();
        if let Some(i) = self.sel_id.and_then(|id| (0..n).find(|&i| self.model.get(i).is_some_and(|a| a.id == id))) {
            self.list.jump(i);
        }
        self.sel_id = self.model.get(self.list.selected()).map(|a| a.id);
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

    pub fn crown(&mut self, delta: i16, now: u32) -> bool {
        let moved = self.list.crown(delta, now);
        self.sel_id = self.model.get(self.list.selected()).map(|a| a.id);
        moved
    }

    /// The agent id under a touch at `y`: it becomes the selection and its row lights up for [`FLASH_MS`].
    pub fn tap(&mut self, y: i32, now: u32) -> Option<u8> {
        let i = self.list.hit(y, now)?;
        self.list.select(i, now);
        self.flash = Some((i, now.wrapping_add(FLASH_MS)));
        self.list.invalidate();
        self.sel_id = self.model.get(i).map(|a| a.id);
        self.sel_id
    }

    fn flashed(&self, i: usize) -> bool {
        self.flash.is_some_and(|(f, _)| f == i)
    }

    /// Paints what changed; `true` if anything was drawn (then `show`).
    pub fn draw(&mut self, s: &mut impl Surface, now: u32) -> bool {
        if let Some((_, t)) = self.flash
            && (now.wrapping_sub(t) as i32) >= 0
        {
            self.flash = None;
            self.list.invalidate();
        }
        let mut any = false;
        if core::mem::take(&mut self.full) {
            s.rect(0, 0, SIZE, SIZE, BG);
            (self.counts, self.line, self.footer) = (None, None, None);
            self.list.invalidate();
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
        any |= self.list.draw(s, now, BG, |s, i, f| {
            let Some(a) = model.get(i) else { return };
            let lit = flash.is_some_and(|(j, _)| j == i);
            let bg = row_bg(&f, lit);
            if f.t > 0 || lit {
                rrect(s, f.x0, f.y - PITCH / 2 + 2, f.x1 - f.x0, PITCH - 4, PITCH / 2, bg, BLACK);
            }
            let (dx, tx) = (dot_at(&f), f.x0 + 40);
            let level = if a.status == Status::Working { pulse(now) } else { 256 };
            disc(s, dx, f.y, DOT_R, colour(a.status, level), bg);
            // brighter near the focus line, DIM two pitches away
            let fg = Rgb::from565(DIM).mix(Rgb::from565(FG), ONE - (f.d * ONE / (2 * PITCH)).min(ONE)).rgb565();
            if f.t > ONE / 2 {
                let cells = ((f.x1 - 8 - tx) / (8 * BIG)).max(0) as usize;
                let (shown, cut) = clip(a.title(), cells);
                if cut {
                    let (shown, _) = clip(shown, cells.saturating_sub(1));
                    big(s, tx, f.y - 26, shown, BIG, fg);
                    ellipsis(s, tx + (cells as i32 - 1) * 8 * BIG, f.y - 22, fg);
                } else {
                    big(s, tx, f.y - 26, shown, BIG, fg);
                }
                fitted(s, tx, f.y + 2, a.ws(), ((f.x1 - 8 - tx) / CHAR).max(0) as usize, DIM);
            } else {
                let cells = ((f.x1 - 8 - tx) / CHAR).max(0) as usize;
                fitted(s, tx, f.y - 17, a.title(), cells, fg);
                fitted(s, tx, f.y + 1, a.ws(), cells, DIM);
            }
        });
        let footer = (self.list.selected(), self.list.len());
        if self.footer != Some(footer) {
            self.footer = Some(footer);
            s.rect(0, FOOTER[0], SIZE, FOOTER[1], BG);
            if footer.1 > 1 {
                let (mut b0, mut b1) = ([0u8; 11], [0u8; 11]);
                let (n, total) = (int(footer.0 as i32 + 1, &mut b0), int(footer.1.min(999) as i32, &mut b1));
                let x = C - (n.len() + 1 + total.len()) as i32 * CHAR / 2;
                s.text(x, 410, n, DIM);
                s.text(x + n.len() as i32 * CHAR, 410, "/", DIM);
                s.text(x + (n.len() + 1) as i32 * CHAR, 410, total, DIM);
            }
            any = true;
        }
        any
    }

    /// Repaints only the dots of visible working agents; `true` if any. Skipped while the list glides (`draw`
    /// repaints it every frame then).
    pub fn animate(&self, s: &mut impl Surface, now: u32) -> bool {
        if self.list.moving(now) {
            return false;
        }
        let mut any = false;
        for i in 0..self.model.len() {
            let (Some(a), Some(f)) = (self.model.get(i), self.list.slot(i, now)) else { continue };
            if a.status == Status::Working && !self.flashed(i) {
                disc(s, dot_at(&f), f.y, DOT_R, colour(a.status, pulse(now)), row_bg(&f, false));
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
        let mut px = vec![0x1234u16; (SIZE * SIZE) as usize];
        load(&mut fresh, &AGENTS[..ui.model.len()]);
        fresh.heard = ui.heard;
        fresh.list.jump(ui.list.selected());
        fresh.flash = ui.flash;
        fresh.draw(&mut Fb::new(&mut px), now);
        px
    }

    fn selected_id(ui: &Ui) -> u8 {
        ui.model.get(ui.list.selected()).unwrap().id
    }

    // tests/test_herdr_e2e.py samples these: the selected item on the focus line, the next two below it
    #[test]
    fn focus_geometry() {
        let mut ui = Ui::new();
        load(&mut ui, AGENTS);
        let at: Vec<(i32, i32)> = (0..3).map(|i| ui.list.slot(i, 0).map(|f| (dot_at(&f), f.y)).unwrap()).collect();
        assert_eq!(at, [(33, 250), (55, 310), (99, 370)]);
        assert!(ui.list.slot(3, 0).is_none());
    }

    #[test]
    fn selection_follows_the_agent() {
        let mut ui = Ui::new();
        load(&mut ui, AGENTS);
        ui.crown(1, 0);
        ui.crown(1, 0);
        assert_eq!(selected_id(&ui), 5); // blocked 4, done 3, working infra/tests 5
        // agent 5 goes idle: it sorts after the working ones, the selection goes with it
        let mut b = Vec::from([5u8, 0, 5]);
        b.extend(b"infratests".iter());
        ui.model.apply(&msg(b'U', 30, &b));
        ui.changed();
        assert_eq!(selected_id(&ui), 5);
        assert_ne!(ui.list.selected(), 2);
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
        assert!(ui.crown(1, 300));
        assert!(ui.draw(&mut fb, 380), "mid-glide");
        ui.draw(&mut fb, 300 + GLIDE_MS);
        assert!(!ui.draw(&mut fb, 500), "still");
        // a tap on the item under the focus line: it is selected (glide) and sent
        assert_eq!(ui.tap(FOCUS_Y + PITCH, 600), Some(5));
        assert_eq!(selected_id(&ui), 5);
        let settled = 600 + GLIDE_MS;
        ui.draw(&mut fb, settled);
        ui.animate(&mut fb, settled);
        assert!(fb.px == full(&ui, settled).as_slice(), "flash");
        ui.draw(&mut fb, 1000);
        assert!(ui.flash.is_none() && ui.animate(&mut fb, 1000));
        assert!(fb.px == full(&ui, 1000).as_slice(), "flash ended");
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
