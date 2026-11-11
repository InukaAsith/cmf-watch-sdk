// Every cmf-wp2-ui primitive and widget, one page each; the crown pages through them (on the list page it scrolls
// first). Pure: the device loop feeds events and time, this draws through a Surface and says which haptic to play.

use cmf_wp2_ui::color::{BLACK, Rgb, WHITE};
use cmf_wp2_ui::draw::{arc, disc, gradient, line, ring, rrect};
use cmf_wp2_ui::geom::text_box;
use cmf_wp2_ui::haptic::{self, Pattern};
use cmf_wp2_ui::motion::{Tween, cos, ease_in_out, linear, sin, wave};
use cmf_wp2_ui::text::{big, big_width, centre, fitted, wrap};
use cmf_wp2_ui::widget::{Button, Label, List, ProgressRing, Toggle, page_dots};
use cmf_wp2_ui::{C, CHAR, ONE, SIZE, Surface};

pub const PAGES: usize = 6;
pub const TITLES: [&str; PAGES] = ["Фигуры", "Прогресс", "Список", "Кнопки", "Текст", "Движение"];
pub const FRAME_MS: u32 = 33;
const IDLE_MS: u32 = 1000;
const BG: u16 = 0;
const FG: u16 = 0xFFFF;
const DIM: u16 = 0x8C71;
const CYAN: Rgb = Rgb(0x22, 0xD3, 0xEE);
const BLUE: Rgb = Rgb(0x3B, 0x82, 0xF6);
const VIOLET: Rgb = Rgb(0x8B, 0x5C, 0xF6);
const MAGENTA: Rgb = Rgb(0xE8, 0x4C, 0xD8);
const GREEN: Rgb = Rgb(0x3C, 0xDC, 0x6E);
const ORANGE: Rgb = Rgb(0xFF, 0x8A, 0x3D);
const TRACK: Rgb = Rgb(0x2A, 0x2A, 0x34);
const HILITE: u16 = 0x2124;
const PROGRESS_MS: u32 = 4000;
const FLASH_MS: u32 = 300;

pub const ITEMS: [&str; 12] = ["Почта", "Календарь", "Погода", "Музыка", "Таймер", "Шаги", "Пульс", "Сон", "Notes",
    "Settings", "Очень длинное название пункта", "Последний"];
const HUES: [Rgb; 4] = [CYAN, BLUE, VIOLET, MAGENTA];
const PARAGRAPH: &str = "Круглый экран: каждая строка получает свою ширину, слова переносятся целиком, а то, что не \
    влезло, кончается многоточием.";
const MOTION_Y: [i32; 3] = [170, 250, 330];

pub struct App {
    page: usize,
    full: bool,
    title: Label<32>,
    ring: ProgressRing,
    pct: Label<8>,
    progress: Tween,
    list: List,
    flash: Option<(usize, u32)>,
    scrollbar: Option<(usize, usize)>,
    toggle: Toggle,
    buttons: [Button; 3],
    dots: [Option<(i32, i32)>; 3],
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}

impl App {
    pub fn new() -> Self {
        let b = |y, label| Button::new(C - 90, y, 180, 50, label, Rgb(0x2B, 0x4C, 0x9A), BLACK);
        let mut list = List::new(100, 56, 5, 10);
        list.set_len(ITEMS.len());
        let mut toggle = Toggle::new(270, 112, 84, 44, GREEN, Rgb(0x55, 0x55, 0x60), BLACK);
        toggle.jump(true);
        App {
            page: 0,
            full: true,
            title: Label::new(C, 40, 2, FG, BG),
            ring: ProgressRing::new(C, C, 120, 140, CYAN, TRACK, BLACK),
            pct: Label::new(C, C - 28, 6, FG, BG),
            progress: Tween::at(0),
            list,
            flash: None,
            scrollbar: None,
            toggle,
            buttons: [b(186, "Тик"), b(250, "Двойной"), b(314, "Тревога")],
            dots: [None; 3],
        }
    }

    pub fn page(&self) -> usize {
        self.page
    }

    pub fn redraw(&mut self) {
        self.full = true;
    }

    fn go(&mut self, page: usize, now: u32) {
        self.page = page;
        self.full = true;
        self.flash = None;
        if page == 1 {
            self.progress = Tween { ease: ease_in_out, ..Tween::at(0) };
            self.progress.go(ONE, now, PROGRESS_MS);
        }
    }

    /// One detent: scrolls the list page, else turns the page.
    pub fn crown(&mut self, delta: i16, now: u32) {
        if self.page == 2 && self.list.crown(delta) {
            return;
        }
        let p = (self.page as i32 + delta.signum() as i32).rem_euclid(PAGES as i32) as usize;
        self.go(p, now);
    }

    /// A touch; returns the haptic to play.
    pub fn tap(&mut self, x: i32, y: i32, now: u32) -> Option<Pattern> {
        match self.page {
            1 => {
                self.progress.go(ONE, now, PROGRESS_MS);
                self.progress.from = 0;
                Some(haptic::TICK)
            }
            2 => {
                let i = self.list.hit(y)?;
                if let Some((old, _)) = self.flash.replace((i, now.wrapping_add(FLASH_MS))) {
                    self.list.invalidate(old);
                }
                self.list.invalidate(i);
                Some(haptic::TICK)
            }
            3 => {
                if let Some(on) = self.toggle.tap(x, y, now) {
                    return on.then_some(haptic::TICK);
                }
                let k = self.buttons.iter_mut().position(|b| b.tap(x, y, now))?;
                self.toggle.on().then_some([haptic::TICK, haptic::DOUBLE, haptic::ALERT][k])
            }
            _ => None,
        }
    }

    /// ms until the next frame is due.
    pub fn wait(&self, now: u32) -> u32 {
        let busy = match self.page {
            1 => !self.progress.done(now),
            2 => self.flash.is_some(),
            3 => self.toggle.animating(now) || self.buttons.iter().any(|b| b.is_pressed(now)),
            5 => true,
            _ => false,
        };
        if busy || self.full { FRAME_MS } else { IDLE_MS }
    }

    /// Draws what changed; `true` if anything was drawn (then `show`).
    pub fn frame(&mut self, s: &mut impl Surface, now: u32) -> bool {
        let full = core::mem::take(&mut self.full);
        if full {
            s.rect(0, 0, SIZE, SIZE, BG);
            page_dots(s, 440, PAGES, self.page, Rgb(0xE9, 0xE3, 0xFF), Rgb(0x4B, 0x3F, 0x72));
            self.title.invalidate();
            self.title.set(TITLES[self.page]);
            self.title.draw(s);
            match self.page {
                1 => {
                    self.ring.invalidate();
                    self.pct.invalidate();
                    centre(s, C, C + 40, "загрузка", DIM);
                }
                2 => {
                    self.list.invalidate_all();
                    self.scrollbar = None;
                }
                3 => {
                    self.toggle.invalidate();
                    self.buttons.iter_mut().for_each(Button::invalidate);
                    s.text(96, 126, "Вибро", FG);
                }
                5 => self.dots = [None; 3],
                _ => {}
            }
        }
        let drew = match self.page {
            0 => full && shapes(s),
            1 => self.progress_page(s, now),
            2 => self.list_page(s, now),
            3 => {
                let mut any = self.toggle.draw(s, now);
                for b in &mut self.buttons {
                    any |= b.draw(s, now);
                }
                any
            }
            4 => full && text_page(s),
            _ => self.motion(s, now),
        };
        full || drew
    }

    fn progress_page(&mut self, s: &mut impl Surface, now: u32) -> bool {
        let v = self.progress.value(now);
        self.ring.set(v);
        self.pct.set_int(v * 100 / ONE);
        let pct = self.pct.draw(s);
        self.ring.draw(s) | pct
    }

    fn list_page(&mut self, s: &mut impl Surface, now: u32) -> bool {
        if let Some((i, t)) = self.flash
            && (now.wrapping_sub(t) as i32) >= 0
        {
            self.flash = None;
            self.list.invalidate(i);
        }
        let flash = self.flash;
        let mut any = self.list.draw(s, BG, |s, i, row| {
            if flash.is_some_and(|(f, _)| f == i) {
                rrect(s, row.x0, row.y + 4, row.x1 - row.x0, row.h - 8, 14, Rgb::from565(HILITE), BLACK);
            }
            let under = Rgb::from565(if flash.is_some_and(|(f, _)| f == i) { HILITE } else { BG });
            disc(s, row.x0 + 16, row.y + row.h / 2, 7, HUES[i % 4], under);
            fitted(s, row.x0 + 32, row.y + row.h / 2 - CHAR / 2, ITEMS[i], row.cells(row.x0 + 32).saturating_sub(1), FG);
        });
        // scroll position on an arc along the right edge
        let bar = (self.list.scroll(), self.list.len());
        if self.scrollbar != Some(bar) || any {
            self.scrollbar = Some(bar);
            const SPAN: u32 = 10923; // 60°
            let start = 65536u32 - SPAN / 2;
            let (len, at) = (SPAN * 5 / bar.1.max(5) as u32, SPAN * bar.0 as u32 / bar.1.max(1) as u32);
            arc(s, C, C, 219, 225, start as u16, SPAN, TRACK, None, BLACK);
            arc(s, C, C, 219, 225, (start + at) as u16, len, WHITE, None, BLACK);
            any = true;
        }
        any
    }

    fn motion(&mut self, s: &mut impl Surface, now: u32) -> bool {
        let u = wave(now, 2400);
        let span = 220;
        let xs = [
            C - span / 2 + span * linear(u) / ONE,
            C - span / 2 + span * ease_in_out(u) / ONE,
            C + (span / 2) * sin((now % 1600 * 65536 / 1600) as u16) / 16384,
        ];
        let mut any = false;
        for (k, &x) in xs.iter().enumerate() {
            let y = MOTION_Y[k];
            if self.dots[k] == Some((x, y)) {
                continue;
            }
            if self.dots[k].is_none() {
                centre(s, C, y - 40, ["linear", "ease in-out", "sin"][k], DIM);
                rrect(s, C - span / 2 - 14, y - 2, span + 28, 4, 2, TRACK, BLACK);
            }
            if let Some((ox, oy)) = self.dots[k] {
                s.rect(ox - 14, oy - 14, 29, 29, BG);
                rrect(s, ox - 14, y - 2, 29, 4, 0, TRACK, BLACK);
            }
            disc(s, x, y, 12, HUES[k + 1], BLACK);
            self.dots[k] = Some((x, y));
            any = true;
        }
        any
    }
}

fn shapes(s: &mut impl Surface) -> bool {
    disc(s, 130, 140, 34, CYAN, BLACK);
    ring(s, C, 140, 24, 36, VIOLET, BLACK);
    arc(s, 336, 140, 22, 36, 49152, 49152, MAGENTA, Some(TRACK), BLACK);
    for k in 0..8u16 {
        let a = k * 8192;
        let (dx, dy) = (cos(a) * 56 / 16384, sin(a) * 56 / 16384);
        line(s, 150 + dx / 4, 262 + dy / 4, 150 + dx, 262 + dy, 1 + k as i32 % 4 * 2, Rgb::from565(FG).mix(BLUE, k as i32 * ONE / 8), BLACK);
    }
    rrect(s, 248, 220, 130, 44, 22, GREEN, BLACK);
    rrect(s, 248, 276, 130, 44, 10, ORANGE, BLACK);
    gradient(s, 110, 344, 246, 36, BLUE, MAGENTA, false);
    gradient(s, 186, 386, 94, 34, CYAN, BLACK, true);
    true
}

fn text_page(s: &mut impl Surface) -> bool {
    // each line as wide as the round screen allows at its y; the last one ends in an ellipsis
    let mut rest = PARAGRAPH;
    let mut y = 84;
    while !rest.is_empty() && y < 84 + 6 * 22 {
        let (x, cells) = text_box(y, 14);
        let mut out = [""; 2];
        let last = y + 22 >= 84 + 6 * 22;
        if last || wrap(rest, cells, &mut out) < 2 {
            fitted(s, x, y, rest, cells, FG);
            break;
        }
        s.text(x, y, out[0], FG);
        rest = out[1];
        y += 22;
    }
    let (x, cells) = text_box(232, 40);
    fitted(s, x, 232, "fitted: строка длиннее места", cells.min(18), DIM);
    let clock = "12:30";
    big(s, C - big_width(clock, 6) / 2, 272, clock, 6, 0xFEA0);
    centre(s, C, 330, "big() x6", DIM);
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use cmf_wp2_ui::{Count, Fb};
    use std::vec;

    fn at(page: usize) -> App {
        let mut app = App::new();
        app.go(page, 0);
        app
    }

    #[test]
    fn crown_pages_and_scrolls_the_list() {
        let mut app = App::new();
        app.crown(-1, 0);
        assert_eq!(app.page(), PAGES - 1);
        app.crown(1, 0);
        app.crown(1, 0);
        app.crown(1, 0);
        assert_eq!(app.page(), 2);
        for _ in 0..ITEMS.len() - 5 {
            app.crown(1, 0);
            assert_eq!(app.page(), 2, "scrolls first");
        }
        app.crown(1, 0);
        assert_eq!(app.page(), 3);
    }

    #[test]
    fn every_page_draws_inside_the_screen_then_settles() {
        for page in 0..PAGES {
            let mut app = at(page);
            let mut px = vec![0x1234u16; (SIZE * SIZE) as usize];
            let mut fb = Fb::new(&mut px);
            assert!(app.frame(&mut fb, 0), "page {page}");
            assert!(fb.px.iter().all(|&p| p != 0x1234), "page {page} clears");
            let lit = fb.px.iter().enumerate().filter(|&(i, &p)| p != 0 && !cmf_wp2_ui::geom::inside(i as i32 % SIZE, i as i32 / SIZE)).count();
            assert_eq!(lit, 0, "page {page} draws outside the circle");
            assert!(fb.px.iter().filter(|&&p| p != 0).count() > 3000, "page {page} is empty");
            if ![1, 5].contains(&page) {
                assert!(!app.frame(&mut fb, 50), "page {page} redraws while idle");
                assert_eq!(app.wait(50), IDLE_MS);
            }
        }
    }

    #[test]
    fn taps_give_haptics() {
        let mut app = at(3);
        app.frame(&mut Count::default(), 0);
        assert_eq!(app.tap(C, 186 + 25, 10), Some(haptic::TICK));
        assert_eq!(app.tap(C, 250 + 25, 10), Some(haptic::DOUBLE));
        assert_eq!(app.tap(C, 314 + 25, 10), Some(haptic::ALERT));
        assert_eq!(app.tap(300, 130, 10), None, "toggle off: silent");
        assert_eq!(app.tap(C, 186 + 25, 20), None, "buttons silent while off");
        assert_eq!(app.wait(20), FRAME_MS);
        let mut list = at(2);
        list.frame(&mut Count::default(), 0);
        assert_eq!(list.tap(C, 100 + 56 + 10, 0), Some(haptic::TICK));
        assert_eq!(list.tap(C, 20, 0), None);
    }

    // like the e2e: 2 s per page, a tap at 0.5 s; `cargo test rect_counts -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn rect_counts() {
        for (page, title) in TITLES.iter().enumerate() {
            let mut app = at(page);
            let mut c = Count::default();
            app.frame(&mut c, 0);
            let first = c.rects;
            let (mut max, mut sum) = (0, 0);
            for f in 1..=60u32 {
                if f == 15 {
                    app.tap(C, 200, f * 33);
                }
                let mut c = Count::default();
                app.frame(&mut c, f * 33);
                max = max.max(c.rects);
                sum += c.rects;
            }
            std::println!("page {page} {title}: first frame {first} rects, then {} rects/frame (max {max})", sum / 60);
        }
    }

    // host renders of every page for a look: `cargo test dump_pages -- --ignored`, writes runs/uikit/host-*.ppm
    #[test]
    #[ignore]
    fn dump_pages() {
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../../runs/uikit");
        std::fs::create_dir_all(dir).unwrap();
        for page in 0..PAGES {
            let mut app = at(page);
            let mut px = vec![0u16; (SIZE * SIZE) as usize];
            let mut fb = Fb::new(&mut px);
            for t in [0, 400, 900] {
                if page == 2 && t == 400 {
                    app.crown(1, t);
                    app.tap(C, 100 + 56 + 10, t);
                }
                if page == 3 && t == 400 {
                    app.tap(C, 250 + 25, t);
                }
                app.frame(&mut fb, t);
            }
            let mut out = std::format!("P6 {SIZE} {SIZE} 255\n").into_bytes();
            for p in &px {
                let c = Rgb::from565(*p);
                out.extend([c.0, c.1, c.2]);
            }
            std::fs::write(std::format!("{dir}/host-{page}.ppm"), out).unwrap();
        }
    }
}
