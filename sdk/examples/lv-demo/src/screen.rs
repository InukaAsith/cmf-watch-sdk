//! The lv-demo screen as batches; tests/test_lv_e2e.py reads the layout constants.

use cmf_wp2_app::lv::*;

pub const ROWS: usize = 12;
pub const LIST_Y: i16 = 76;
pub const ROW_H: i32 = 50;
pub const ROW_GAP: i32 = 8;
const LIST_W: i32 = 300;
const LIST_H: i32 = 190;
const WHITE: i32 = 0xFFFF;
const ROW_BG: i32 = 0x198E; // dark blue
const ACCENT: i32 = 0x07FF; // cyan
const TRACK: i32 = 0x4208; // dark grey
const CIRCLE: i32 = 0x7FFF; // LV_RADIUS_CIRCLE
const INDICATOR: u32 = 0x02_0000; // LV_PART_INDICATOR
const KNOB_PART: u32 = 0x03_0000; // LV_PART_KNOB
const SELECTED: u32 = 0x04_0000; // LV_PART_SELECTED
/// Font slot of the title.
const TITLE_FONT: u8 = 0;
pub const ARC_START: i32 = 70;

pub struct Screen {
    pub title: Obj,
    pub rows: [Obj; ROWS],
    pub bar: Obj,
    pub slider: Obj,
    pub arc: Obj,
    pub arc_label: Obj,
    pub roller: Obj,
}

/// First batch: fonts on their own (a missing font file stops the batch there).
pub fn fonts(b: &mut Batch) -> Result<(), LvError> {
    b.font(TITLE_FONT, StockFont::F32M)
}

/// Second batch: the whole screen. Fits one 2048 B buffer (host test).
pub fn build(b: &mut Batch) -> Result<Screen, LvError> {
    // text colour is inherited: every label below is white. The firmware's theme styles nothing, so the slider
    // and the arc get their parts styled here or they stay invisible
    b.style(ROOT, Prop::TextColor, 0, WHITE)?;
    let title = b.create(Class::Label, ROOT)?;
    b.style(title, Prop::TextFont, 0, TITLE_FONT as i32)?;
    b.text(title, "LVGL demo")?;
    b.align(title, Align::TopMid, 0, 28)?;

    // a bare flex column: no padding, border or background, so row i sits at LIST_Y + i * (ROW_H + ROW_GAP)
    let list = b.create(Class::Obj, ROOT)?;
    b.size(list, LIST_W, LIST_H)?;
    b.align(list, Align::TopMid, 0, LIST_Y)?;
    for p in [Prop::PadTop, Prop::PadBottom, Prop::PadLeft, Prop::PadRight, Prop::BorderWidth, Prop::Radius, Prop::BgOpa] {
        b.style(list, p, 0, 0)?;
    }
    b.style(list, Prop::PadRow, 0, ROW_GAP)?;
    b.flex(list, Flow::Column, FlexAlign::Start, FlexAlign::Center, FlexAlign::Start)?;
    let mut rows = [ROOT; ROWS];
    for (i, row) in rows.iter_mut().enumerate() {
        let btn = b.create(Class::Btn, list)?;
        b.size(btn, pct(100), ROW_H)?;
        b.style(btn, Prop::BgColor, 0, ROW_BG)?;
        b.style(btn, Prop::BgOpa, 0, 255)?;
        b.style(btn, Prop::Radius, 0, 12)?;
        b.events(btn, &[Ev::Clicked])?;
        let label = b.create(Class::Label, btn)?;
        let mut text = *b"Row 00";
        text[4] += (i / 10) as u8;
        text[5] += (i % 10) as u8;
        b.text(label, core::str::from_utf8(&text).unwrap_or("Row"))?;
        b.align(label, Align::LeftMid, 8, 0)?;
        *row = btn;
    }
    let bar = b.scroll_bar(list)?;

    let slider = b.create(Class::Slider, ROOT)?;
    b.size(slider, 260, 16)?;
    b.align(slider, Align::TopMid, 0, 290)?;
    b.range(slider, 0, 100)?;
    b.value(slider, ARC_START, false)?;
    for (part, colour) in [(0, TRACK), (INDICATOR, ACCENT), (KNOB_PART, WHITE)] {
        b.style(slider, Prop::BgColor, part, colour)?;
        b.style(slider, Prop::BgOpa, part, 255)?;
        b.style(slider, Prop::Radius, part, CIRCLE)?;
    }
    b.events(slider, &[Ev::ValueChanged])?;

    // arc range is fixed 0..100 in the firmware: the slider's range matches it
    let arc = b.create(Class::Arc, ROOT)?;
    b.size(arc, 120, 120)?;
    b.align(arc, Align::BottomMid, -70, -24)?;
    b.value(arc, ARC_START, false)?;
    for (part, colour) in [(0, TRACK), (INDICATOR, ACCENT)] {
        b.style(arc, Prop::ArcColor, part, colour)?;
        b.style(arc, Prop::ArcOpa, part, 255)?;
        b.style(arc, Prop::ArcWidth, part, 10)?;
        b.style(arc, Prop::ArcRounded, part, 1)?;
    }
    b.flags(arc, &[], &[Flag::Clickable])?; // display only: the slider drives it
    let arc_label = b.create(Class::Label, arc)?;
    b.align(arc_label, Align::Center, 0, 0)?;
    b.text(arc_label, "70")?;

    let roller = b.create(Class::Roller, ROOT)?;
    b.options(roller, "Tick\nDouble\nAlert\nDone", false)?;
    b.size(roller, 110, 100)?;
    b.align(roller, Align::BottomMid, 65, -34)?;
    b.style(roller, Prop::BgColor, SELECTED, ROW_BG)?;
    b.style(roller, Prop::BgOpa, SELECTED, 255)?;
    b.events(roller, &[Ev::ValueChanged])?;
    // creating a roller makes it the knob object: hand the crown back to the list (it scrolls, the bar shows)
    b.knob(list)?;
    Ok(Screen { title, rows, bar, slider, arc, arc_label, roller })
}

/// The arc and its number follow the slider.
pub fn set_arc(b: &mut Batch, s: &Screen, v: i32) -> Result<(), LvError> {
    let v = v.clamp(0, 100);
    b.value(s.arc, v, false)?;
    let mut text = [b' ', b' ', b'0'];
    let mut n = v;
    for d in text.iter_mut().rev() {
        *d = b'0' + (n % 10) as u8;
        n /= 10;
        if n == 0 {
            break;
        }
    }
    let start = text.iter().position(|&c| c != b' ').unwrap_or(2);
    b.text(s.arc_label, core::str::from_utf8(&text[start..]).unwrap_or("?"))
}

/// A batch the loader must reject whole: a valid title change, then a SIZE for a handle past the budget.
pub fn bad(b: &mut Batch, s: &Screen) -> Result<(), LvError> {
    b.text(s.title, "broken")?;
    b.size(Obj(500), 10, 10)
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use std::boxed::Box;
    use wp2_core::lvcmd::{self, Table};

    fn batch() -> Batch {
        Batch::with_buf(Box::leak(Box::new([0; BUF])))
    }

    // the loader's table after `records`
    fn table_after(records: &[u8]) -> Table {
        let mut t = Table::new();
        for r in lvcmd::records(records) {
            lvcmd::apply(&mut t, &r.unwrap());
        }
        t
    }

    #[test]
    fn screen_validates_and_fits_one_buffer() {
        let mut f = batch();
        fonts(&mut f).unwrap();
        assert_eq!(lvcmd::validate(f.bytes(), &Table::new(), 0), Ok(()));
        let mut b = batch();
        let s = build(&mut b).unwrap();
        let screen = b.bytes().to_vec();
        std::println!("screen batch: {} B", screen.len());
        assert!(screen.len() <= BUF);
        assert_eq!(lvcmd::validate(&screen, &Table::new(), 1 << TITLE_FONT), Ok(()));
        // without the font the title's TEXT_FONT is refused
        assert!(lvcmd::validate(&screen, &Table::new(), 0).is_err());
        let t = table_after(&screen);
        let mut b = batch();
        for v in [0, 7, 42, 100, 250] {
            set_arc(&mut b, &s, v).unwrap();
        }
        assert_eq!(lvcmd::validate(b.bytes(), &t, 1), Ok(()));
        let mut b = batch();
        bad(&mut b, &s).unwrap();
        assert!(lvcmd::validate(b.bytes(), &t, 1).is_err());
    }

    #[test]
    fn arc_label_text() {
        let s = build(&mut batch()).unwrap();
        for (v, want) in [(0, "0"), (7, "7"), (42, "42"), (100, "100"), (-3, "0"), (250, "100")] {
            let mut b = batch();
            set_arc(&mut b, &s, v).unwrap();
            let bytes = b.bytes();
            assert_eq!(&bytes[bytes.len() - want.len()..], want.as_bytes(), "{v}");
        }
    }
}
