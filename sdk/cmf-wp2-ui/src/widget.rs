//! Widgets with dirty redraw: each keeps what it last drew, `draw` paints only what changed and returns whether it
//! painted (then call `show`). They own no event loop: feed them crown deltas and touch points, they say what
//! happened. Touch is TOUCH_DOWN (its `y` is the reliable one).

use crate::color::{BLACK, Rgb};
use crate::draw::{arc, disc, radial_n, rrect, Band, Paint};
use crate::geom::chord;
use crate::motion::{ease_out, Tween};
use crate::text::{big, big_width, clip, ellipsis, int, width};
use crate::{C, CHAR, ONE, Surface};

/// One visible list row: its band and the part of it inside the round screen.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Row {
    pub y: i32,
    pub h: i32,
    pub x0: i32,
    pub x1: i32,
}

impl Row {
    /// Chars that fit from `x` to the row's right end.
    pub fn cells(&self, x: i32) -> usize {
        ((self.x1 - x).max(0) / CHAR) as usize
    }
}

/// A vertical list of `len` items, `rows` visible from `top`, rows `row_h` high that follow the round edge (`margin`
/// px in). The crown scrolls a row per detent. Dirty bits are per visible row.
pub struct List {
    pub top: i32,
    pub row_h: i32,
    pub rows: usize,
    pub margin: i32,
    len: usize,
    scroll: usize,
    dirty: u32,
}

impl List {
    pub const fn new(top: i32, row_h: i32, rows: usize, margin: i32) -> Self {
        List { top, row_h, rows, margin, len: 0, scroll: 0, dirty: u32::MAX }
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn scroll(&self) -> usize {
        self.scroll
    }

    pub fn max_scroll(&self) -> usize {
        self.len.saturating_sub(self.rows)
    }

    /// Items below the last visible row.
    pub fn more(&self) -> usize {
        self.len.saturating_sub(self.scroll + self.rows)
    }

    /// New item count (keeps the scroll inside); every row is redrawn.
    pub fn set_len(&mut self, n: usize) {
        self.len = n;
        self.scroll = self.scroll.min(self.max_scroll());
        self.invalidate_all();
    }

    /// One crown detent; `true` if the list moved.
    pub fn crown(&mut self, delta: i16) -> bool {
        let s = (self.scroll as i32 + delta.signum() as i32).clamp(0, self.max_scroll() as i32) as usize;
        let moved = s != self.scroll;
        if moved {
            self.scroll = s;
            self.invalidate_all();
        }
        moved
    }

    /// Item under a touch at `y`.
    pub fn hit(&self, y: i32) -> Option<usize> {
        let r = (y - self.top).div_euclid(self.row_h);
        let i = self.scroll + r as usize;
        (y >= self.top && (r as usize) < self.rows && i < self.len).then_some(i)
    }

    /// Geometry of visible row `r` (0..rows).
    pub fn row(&self, r: usize) -> Row {
        let y = self.top + r as i32 * self.row_h;
        let (x0, x1) = chord(y, y + self.row_h, self.margin);
        Row { y, h: self.row_h, x0, x1 }
    }

    pub fn invalidate(&mut self, item: usize) {
        if let Some(r) = item.checked_sub(self.scroll).filter(|&r| r < self.rows) {
            self.dirty |= 1 << r;
        }
    }

    pub fn invalidate_all(&mut self) {
        self.dirty = u32::MAX;
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty & ((1u64 << self.rows) - 1) as u32 != 0
    }

    /// Repaints dirty rows: each is cleared to `bg` over its chord ([x0 - margin, x1 + margin)), then
    /// `item(s, index, row)` draws it inside that band (rows past the end stay clear).
    pub fn draw<S: Surface>(&mut self, s: &mut S, bg: u16, mut item: impl FnMut(&mut S, usize, Row)) -> bool {
        let mut any = false;
        for r in 0..self.rows {
            if self.dirty >> r & 1 == 0 {
                continue;
            }
            let row = self.row(r);
            s.rect(row.x0 - self.margin, row.y, row.x1 - row.x0 + 2 * self.margin, row.h, bg);
            if self.scroll + r < self.len {
                item(s, self.scroll + r, row);
            }
            any = true;
        }
        self.dirty = 0;
        any
    }
}

/// Glide of a [`FocusList`] to the next selection.
pub const GLIDE_MS: u32 = 160;

/// Where a [`FocusList`] item is this frame: its centre line `y`, the round screen's chord over its band, its
/// distance `d` from the focus line and `t`, ONE on the focus line falling to 0 one pitch away.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Focus {
    pub y: i32,
    pub x0: i32,
    pub x1: i32,
    pub d: i32,
    pub t: i32,
}

/// A list whose selected item sits on the focus line `cy`, as in the stock app list: the crown moves the selection
/// a step and the items glide there ([`GLIDE_MS`]); a tap selects the item under it. Items are `pitch` apart and
/// drawn only while their whole band is inside `top..bottom`, which the list owns: each redraw clears it. The app
/// paints an item from its [`Focus`] (highlight and bigger text near `t = ONE`, dimmer with `d`).
pub struct FocusList {
    pub top: i32,
    pub bottom: i32,
    pub cy: i32,
    pub pitch: i32,
    pub margin: i32,
    len: usize,
    sel: usize,
    pos: Tween,
    drawn: Option<i32>,
}

impl FocusList {
    pub const fn new(top: i32, bottom: i32, cy: i32, pitch: i32, margin: i32) -> Self {
        FocusList { top, bottom, cy, pitch, margin, len: 0, sel: 0, pos: Tween::at(0), drawn: None }
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn selected(&self) -> usize {
        self.sel
    }

    /// New item count; the selection stays where it is if it still exists. Everything is redrawn.
    pub fn set_len(&mut self, n: usize) {
        self.len = n;
        if self.sel >= n {
            let s = n.saturating_sub(1);
            self.sel = s;
            self.pos = Tween::at(s as i32 * self.pitch);
        }
        self.invalidate();
    }

    /// Selects `i` at once, no glide (e.g. the same item after the list was reordered).
    pub fn jump(&mut self, i: usize) {
        self.sel = i.min(self.len.saturating_sub(1));
        self.pos = Tween::at(self.sel as i32 * self.pitch);
        self.invalidate();
    }

    /// Selects `i` with a glide; `true` if the selection changed.
    pub fn select(&mut self, i: usize, now: u32) -> bool {
        let i = i.min(self.len.saturating_sub(1));
        if i == self.sel {
            return false;
        }
        self.sel = i;
        self.pos.ease = ease_out;
        self.pos.go(i as i32 * self.pitch, now, GLIDE_MS);
        true
    }

    /// One crown detent: the next or previous item; `true` if the selection moved.
    pub fn crown(&mut self, delta: i16, now: u32) -> bool {
        let i = self.sel as i32 + delta.signum() as i32;
        i >= 0 && self.select(i as usize, now)
    }

    /// The item drawn under a touch at `y`.
    pub fn hit(&self, y: i32, now: u32) -> Option<usize> {
        (0..self.len).find(|&i| self.slot(i, now).is_some_and(|f| (y - f.y).abs() * 2 < self.pitch))
    }

    /// Item `i` this frame, or `None` while its band is not wholly inside `top..bottom`.
    pub fn slot(&self, i: usize, now: u32) -> Option<Focus> {
        let y = self.cy + i as i32 * self.pitch - self.pos.value(now);
        let (y0, y1) = (y - self.pitch / 2, y + self.pitch / 2);
        if i >= self.len || y0 < self.top || y1 > self.bottom {
            return None;
        }
        let (x0, x1) = chord(y0, y1, self.margin);
        let d = (y - self.cy).abs();
        Some(Focus { y, x0, x1, d, t: (ONE - d * ONE / self.pitch).max(0) })
    }

    /// `true` while the items glide.
    pub fn moving(&self, now: u32) -> bool {
        !self.pos.done(now)
    }

    pub fn invalidate(&mut self) {
        self.drawn = None;
    }

    /// Repaints the band when the items moved or after [`invalidate`](Self::invalidate): clears it to `bg`, then
    /// `item(s, index, focus)` paints each item that fits. `true` if it painted.
    pub fn draw<S: Surface>(&mut self, s: &mut S, now: u32, bg: u16, mut item: impl FnMut(&mut S, usize, Focus)) -> bool {
        let p = self.pos.value(now);
        if self.drawn == Some(p) {
            return false;
        }
        self.drawn = Some(p);
        s.rect(0, self.top, crate::SIZE, self.bottom - self.top, bg);
        for i in 0..self.len {
            if let Some(f) = self.slot(i, now) {
                item(s, i, f);
            }
        }
        true
    }
}

/// Centred text (a header, a title, a big number) of at most `N` bytes; `scale` 2 is normal text, more is big.
/// Clipped to the round screen with an ellipsis. Redraws only when the text changes, clearing only what it drew.
pub struct Label<const N: usize> {
    pub cx: i32,
    pub y: i32,
    pub scale: i32,
    pub fg: u16,
    pub bg: u16,
    buf: [u8; N],
    len: usize,
    drawn: Option<(i32, i32)>,
    dirty: bool,
}

impl<const N: usize> Label<N> {
    pub const fn new(cx: i32, y: i32, scale: i32, fg: u16, bg: u16) -> Self {
        Label { cx, y, scale, fg, bg, buf: [0; N], len: 0, drawn: None, dirty: true }
    }

    pub fn text(&self) -> &str {
        crate::text::fit(&self.buf[..self.len], N)
    }

    pub fn set(&mut self, s: &str) {
        let s = crate::text::fit(s.as_bytes(), N);
        if s != self.text() {
            self.buf[..s.len()].copy_from_slice(s.as_bytes());
            self.len = s.len();
            self.dirty = true;
        }
    }

    pub fn set_int(&mut self, n: i32) {
        let mut b = [0u8; 11];
        self.set(int(n, &mut b));
    }

    pub fn set_colour(&mut self, fg: u16) {
        self.dirty |= fg != self.fg;
        self.fg = fg;
    }

    pub fn invalidate(&mut self) {
        self.dirty = true;
        self.drawn = None;
    }

    pub fn draw(&mut self, s: &mut impl Surface) -> bool {
        if !self.dirty {
            return false;
        }
        self.dirty = false;
        let h = 8 * self.scale;
        if let Some((x, w)) = self.drawn.take() {
            s.rect(x, self.y, w, h, self.bg);
        }
        let (x0, x1) = chord(self.y, self.y + h, 0);
        let cell = 8 * self.scale;
        let fits = ((x1 - x0) / cell).max(0) as usize;
        let (shown, cut) = clip(self.text(), fits);
        let (shown, cells) = if cut { (clip(shown, fits.saturating_sub(1)).0, fits) } else { (shown, crate::text::chars(shown)) };
        let w = cells as i32 * cell;
        let x = self.cx - w / 2;
        if self.scale == 2 {
            s.text(x, self.y, shown, self.fg);
            if cut {
                ellipsis(s, x + w - CHAR, self.y, self.fg);
            }
        } else {
            big(s, x, self.y, shown, self.scale, self.fg);
            if cut {
                big(s, x + w - cell, self.y, "~", self.scale, self.fg);
            }
        }
        debug_assert!(big_width(shown, self.scale) <= w);
        self.drawn = Some((x, w));
        true
    }
}

/// Progress ring: `value` 0..=ONE clockwise from 12 o'clock, `on` over an `off` track. ponytail: redraws the whole
/// ring on change (~1.5k rects at r 150); redraw only the changed sector if a ring ever updates every frame.
pub struct ProgressRing {
    pub cx: i32,
    pub cy: i32,
    pub r0: i32,
    pub r1: i32,
    pub on: Rgb,
    pub off: Rgb,
    pub bg: Rgb,
    value: i32,
    drawn: Option<i32>,
}

impl ProgressRing {
    pub const fn new(cx: i32, cy: i32, r0: i32, r1: i32, on: Rgb, off: Rgb, bg: Rgb) -> Self {
        ProgressRing { cx, cy, r0, r1, on, off, bg, value: 0, drawn: None }
    }

    pub fn value(&self) -> i32 {
        self.value
    }

    pub fn set(&mut self, v: i32) {
        self.value = v.clamp(0, ONE);
    }

    pub fn invalidate(&mut self) {
        self.drawn = None;
    }

    pub fn draw(&mut self, s: &mut impl Surface) -> bool {
        let v = self.value;
        if self.drawn == Some(v) {
            return false;
        }
        self.drawn = Some(v);
        arc(s, self.cx, self.cy, self.r0, self.r1, 49152, (v * (65536 / ONE)) as u32, self.on, Some(self.off), self.bg);
        true
    }
}

fn hit_box(x: i32, y: i32, (bx, by, bw, bh): (i32, i32, i32, i32), slop: i32) -> bool {
    x >= bx - slop && x < bx + bw + slop && y >= by - slop && y < by + bh + slop
}

/// On/off switch: a pill with a knob that slides (150 ms ease-out). `tap` flips it.
pub struct Toggle {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    pub on_colour: Rgb,
    pub off_colour: Rgb,
    pub knob: Rgb,
    pub bg: Rgb,
    on: bool,
    pos: Tween,
    drawn: Option<i32>,
}

impl Toggle {
    pub const SLIDE_MS: u32 = 150;

    pub const fn new(x: i32, y: i32, w: i32, h: i32, on_colour: Rgb, off_colour: Rgb, bg: Rgb) -> Self {
        Toggle { x, y, w, h, on_colour, off_colour, knob: crate::color::WHITE, bg, on: false, pos: Tween { ease: ease_out, ..Tween::at(0) }, drawn: None }
    }

    pub fn on(&self) -> bool {
        self.on
    }

    pub fn set(&mut self, on: bool, now: u32) {
        if on != self.on {
            self.on = on;
            self.pos.go(if on { ONE } else { 0 }, now, Self::SLIDE_MS);
        }
    }

    /// Sets the state without the slide.
    pub fn jump(&mut self, on: bool) {
        self.on = on;
        self.pos = Tween { ease: ease_out, ..Tween::at(if on { ONE } else { 0 }) };
        self.drawn = None;
    }

    /// `Some(new state)` if the touch hit it.
    pub fn tap(&mut self, x: i32, y: i32, now: u32) -> Option<bool> {
        hit_box(x, y, (self.x, self.y, self.w, self.h), 12).then(|| {
            self.set(!self.on, now);
            self.on
        })
    }

    pub fn animating(&self, now: u32) -> bool {
        !self.pos.done(now)
    }

    pub fn invalidate(&mut self) {
        self.drawn = None;
    }

    pub fn draw(&mut self, s: &mut impl Surface, now: u32) -> bool {
        let p = self.pos.value(now);
        if self.drawn == Some(p) {
            return false;
        }
        self.drawn = Some(p);
        let track = self.off_colour.mix(self.on_colour, p);
        rrect(s, self.x, self.y, self.w, self.h, self.h / 2, track, self.bg);
        let r = self.h / 2 - 4;
        let cx = self.x + self.h / 2 + (self.w - self.h) * p / ONE;
        disc(s, cx, self.y + self.h / 2, r, self.knob, track);
        true
    }
}

/// Push button: a pill with a centred label. `tap` returns true on a hit and shows the pressed colour for
/// `FEEDBACK_MS` (pair it with [`crate::haptic::TICK`]).
pub struct Button {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    pub label: &'static str,
    pub colour: Rgb,
    pub pressed: Rgb,
    pub fg: u16,
    pub bg: Rgb,
    until: Option<u32>,
    drawn: Option<bool>,
}

impl Button {
    pub const FEEDBACK_MS: u32 = 150;

    pub const fn new(x: i32, y: i32, w: i32, h: i32, label: &'static str, colour: Rgb, bg: Rgb) -> Self {
        let pressed = Rgb(colour.0 / 2 + 128, colour.1 / 2 + 128, colour.2 / 2 + 128);
        Button { x, y, w, h, label, colour, pressed, fg: 0xFFFF, bg, until: None, drawn: None }
    }

    pub fn tap(&mut self, x: i32, y: i32, now: u32) -> bool {
        let hit = hit_box(x, y, (self.x, self.y, self.w, self.h), 8);
        if hit {
            self.until = Some(now.wrapping_add(Self::FEEDBACK_MS));
        }
        hit
    }

    pub fn is_pressed(&self, now: u32) -> bool {
        self.until.is_some_and(|t| (now.wrapping_sub(t) as i32) < 0)
    }

    /// ms until the pressed look ends (cap the event timeout with it).
    pub fn wait(&self, now: u32) -> Option<u32> {
        self.is_pressed(now).then(|| self.until.map_or(0, |t| t.wrapping_sub(now)))
    }

    pub fn invalidate(&mut self) {
        self.drawn = None;
    }

    pub fn draw(&mut self, s: &mut impl Surface, now: u32) -> bool {
        let p = self.is_pressed(now);
        if self.drawn == Some(p) {
            return false;
        }
        self.drawn = Some(p);
        if !p {
            self.until = None;
        }
        rrect(s, self.x, self.y, self.w, self.h, self.h / 2, if p { self.pressed } else { self.colour }, self.bg);
        let tx = self.x + (self.w - width(self.label)) / 2;
        s.text(tx, self.y + (self.h - CHAR) / 2, self.label, self.fg);
        true
    }
}

/// Page dots along the bottom (agent-glow's indicator): the current page bright and a bit larger.
pub fn page_dots(s: &mut impl Surface, y: i32, pages: usize, current: usize, on: Rgb, off: Rgb) {
    for i in 0..pages {
        let (r, c) = if i == current { (5, on) } else { (4, off) };
        let x = C + (2 * i as i32 - (pages as i32 - 1)) * 8;
        radial_n::<1>(s, x, y, &[Band { r, paint: Paint::Solid(c), soft: 1 }], Some([x - 6, y - 6, x + 7, y + 7]), |_, _| BLACK);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::WHITE;
    use crate::{Count, Fb, SIZE};
    use std::vec;

    fn fb() -> std::vec::Vec<u16> {
        vec![0u16; (SIZE * SIZE) as usize]
    }

    #[test]
    fn focus_list_glides_selects_and_clips() {
        let mut l = FocusList::new(98, 404, 250, 60, 6);
        l.set_len(7);
        // the selection on the focus line, two items above it are out of the list (index < 0), two below fit
        let ys: std::vec::Vec<_> = (0..7).map(|i| l.slot(i, 0).map(|f| f.y)).collect();
        assert_eq!(ys, [Some(250), Some(310), Some(370), None, None, None, None]);
        assert_eq!(l.slot(0, 0).unwrap().t, ONE);
        assert_eq!(l.slot(1, 0).unwrap().t, 0);
        assert!(!l.crown(-1, 0), "no item above the first");
        assert!(l.crown(1, 1000) && l.moving(1000 + GLIDE_MS / 2) && !l.moving(1000 + GLIDE_MS));
        let mid = l.slot(1, 1000 + GLIDE_MS / 2).unwrap();
        assert!(mid.y > 250 && mid.y < 310 && mid.t > 0 && mid.t < ONE, "{mid:?}");
        let end = 1000 + GLIDE_MS;
        assert_eq!(l.slot(1, end).map(|f| (f.y, f.t)), Some((250, ONE)));
        assert_eq!(l.slot(0, end).map(|f| f.y), Some(190));
        // a tap selects what is drawn under it
        assert_eq!(l.hit(190, end), Some(0));
        assert_eq!(l.hit(250 + 29, end), Some(1));
        assert_eq!(l.hit(250 + 31, end), Some(2));
        assert_eq!(l.hit(60, end), None);
        for _ in 0..10 {
            l.crown(1, end);
        }
        assert_eq!(l.selected(), 6);
        l.set_len(3);
        assert_eq!((l.selected(), l.slot(2, end).map(|f| f.y)), (2, Some(250)));
        l.jump(0);
        assert_eq!(l.slot(0, end).map(|f| f.y), Some(250));
    }

    #[test]
    fn focus_list_redraws_only_when_it_moves() {
        let mut px = fb();
        let mut f = Fb::new(&mut px);
        let mut l = FocusList::new(98, 404, 250, 60, 6);
        l.set_len(3);
        let mut seen = std::vec::Vec::new();
        assert!(l.draw(&mut f, 0, 0, |_, i, _| seen.push(i)));
        assert_eq!(seen, [0, 1, 2]);
        assert!(!l.draw(&mut f, 50, 0, |_, _, _| {}));
        l.crown(1, 100);
        assert!(l.draw(&mut f, 150, 0, |_, _, _| {}) && l.draw(&mut f, 100 + GLIDE_MS, 0, |_, _, _| {}));
        assert!(!l.draw(&mut f, 400, 0, |_, _, _| {}));
    }

    #[test]
    fn list_scroll_hit_and_round_rows() {
        let mut l = List::new(112, 58, 5, 6);
        l.set_len(9);
        assert_eq!((l.max_scroll(), l.more()), (4, 4));
        assert_eq!(l.hit(111), None);
        assert_eq!(l.hit(112), Some(0));
        assert!(!l.crown(-1));
        assert!(l.crown(1) && l.crown(1));
        assert_eq!(l.hit(112 + 58 * 2 + 3), Some(4));
        assert_eq!(l.hit(112 + 58 * 5), None);
        for _ in 0..10 {
            l.crown(1);
        }
        assert_eq!(l.scroll(), 4);
        l.set_len(2);
        assert_eq!((l.scroll(), l.hit(112 + 58 * 2 + 3), l.more()), (0, None, 0));
        // herdr-agents' row geometry: dot at x0 + 8, text at x0 + 22
        let xs: std::vec::Vec<i32> = (0..5).map(|r| l.row(r).x0 + 8).collect();
        assert_eq!(xs, [48, 23, 21, 43, 87]);
        for r in 0..5 {
            let row = l.row(r);
            assert!(row.cells(row.x0 + 22) >= 14, "row {r}");
        }
    }

    #[test]
    fn list_redraws_only_dirty_rows() {
        let mut px = fb();
        let mut f = Fb::new(&mut px);
        let mut l = List::new(100, 50, 4, 6);
        l.set_len(10);
        let mut seen = std::vec::Vec::new();
        assert!(l.draw(&mut f, 0, |_, i, _| seen.push(i)));
        assert_eq!(seen, [0, 1, 2, 3]);
        seen.clear();
        assert!(!l.draw(&mut f, 0, |_, i, _| seen.push(i)));
        l.invalidate(2);
        l.invalidate(7); // not visible
        l.draw(&mut f, 0, |_, i, _| seen.push(i));
        assert_eq!(seen, [2]);
        seen.clear();
        l.crown(1);
        l.draw(&mut f, 0, |_, i, _| seen.push(i));
        assert_eq!(seen, [1, 2, 3, 4]);
    }

    #[test]
    fn label_redraws_on_change_and_clears_what_it_drew() {
        let mut px = fb();
        let mut f = Fb::new(&mut px);
        let mut l = Label::<16>::new(C, 200, 2, 0xFFFF, 0);
        l.set("длинная строка");
        assert!(l.draw(&mut f) && !l.draw(&mut f));
        l.set("длинная строка");
        assert!(!l.draw(&mut f));
        l.set("ok");
        assert!(l.draw(&mut f));
        // only "ok" is lit now
        let lit: std::vec::Vec<i32> = (0..SIZE).filter(|&x| (200..216).any(|y| f.at(x, y) != 0)).collect();
        assert!(lit.iter().all(|&x| (C - 16..C + 16).contains(&x)), "{lit:?}");
        // too long for the chord at the top: clipped with an ellipsis, inside the circle
        let mut t = Label::<64>::new(C, 20, 2, 0xFFFF, 0);
        t.set("Заголовок, который не влезает в круг");
        t.draw(&mut f);
        let (x0, x1) = chord(20, 36, 0);
        assert!((0..SIZE).filter(|&x| (20..36).any(|y| f.at(x, y) != 0)).all(|x| x >= x0 && x < x1));
        let mut big = Label::<8>::new(C, 300, 8, 0xFFFF, 0);
        big.set_int(42);
        let mut n = Count::default();
        assert!(big.draw(&mut n) && n.rects < 40, "{}", n.rects);
    }

    #[test]
    fn ring_draws_on_visible_change_only() {
        let mut r = ProgressRing::new(C, C, 130, 150, WHITE, Rgb(40, 40, 40), BLACK);
        let mut n = Count::default();
        r.set(ONE / 3);
        assert!(r.draw(&mut n) && !r.draw(&mut n));
        r.set(ONE / 3);
        assert!(!r.draw(&mut n));
        r.set(ONE / 2);
        assert!(r.draw(&mut n));
        r.set(5 * ONE);
        assert_eq!(r.value(), ONE);
    }

    #[test]
    fn toggle_flips_and_slides() {
        let mut px = fb();
        let mut f = Fb::new(&mut px);
        let mut t = Toggle::new(200, 200, 80, 40, Rgb(0, 200, 0), Rgb(80, 80, 80), BLACK);
        assert!(t.draw(&mut f, 0) && !t.draw(&mut f, 0));
        let knob_left = f.at(220, 220);
        assert_eq!(t.tap(10, 10, 100), None);
        assert_eq!(t.tap(270, 230, 100), Some(true));
        assert!(t.animating(150) && t.draw(&mut f, 150));
        assert!(t.draw(&mut f, 400) && !t.animating(400) && !t.draw(&mut f, 500));
        assert_eq!(f.at(260, 220), knob_left, "knob on the right");
        assert_eq!(f.at(220, 220), Rgb(0, 200, 0).rgb565());
    }

    #[test]
    fn button_feedback_ends() {
        let mut px = fb();
        let mut f = Fb::new(&mut px);
        let mut b = Button::new(150, 300, 160, 48, "Тик", Rgb(40, 90, 200), BLACK);
        assert!(b.draw(&mut f, 0));
        let idle = f.at(160, 324);
        assert!(!b.tap(150, 200, 10) && b.tap(200, 320, 10));
        assert_eq!(b.wait(20), Some(140));
        assert!(b.draw(&mut f, 20) && f.at(160, 324) != idle);
        assert!(!b.draw(&mut f, 100));
        assert!(b.draw(&mut f, 200) && f.at(160, 324) == idle && b.wait(200).is_none());
    }
}
