//! The lv-demo screen in the stock look (`cmf_wp2_app::stock`): a crown-scrolled page with the accent title, a card
//! with a slider that drives an arc, and a row per haptic pattern. tests/test_lv_e2e.py reads LIST_TOP.

use cmf_wp2_app::lv::*;
use cmf_wp2_app::stock;

pub const ROWS: usize = 4;
pub const NAMES: [(&str, &str); ROWS] = [("T", "Tick"), ("D", "Double"), ("A", "Alert"), ("F", "Done")];
const CARD_H: i32 = 220;
/// Top of the first row: under the title and the card.
pub const LIST_TOP: i32 = stock::TOP + CARD_H + stock::ROW_GAP;
pub const ARC_START: i32 = 70;

pub struct Screen {
    pub page: Obj,
    pub bar: Obj,
    pub title: Obj,
    pub slider: Obj,
    pub arc: Obj,
    pub arc_label: Obj,
    pub list: Obj,
    pub rows: [Obj; ROWS],
}

/// First batch: the system font on its own (a missing font file stops the batch there).
pub fn fonts(b: &mut Batch) -> Result<(), LvError> {
    stock::font(b)
}

/// Second batch: page, title and the card. Third: the rows ([`list`]).
pub fn build(b: &mut Batch) -> Result<Screen, LvError> {
    stock::theme(b)?;
    let (page, bar) = stock::page(b)?;
    let title = stock::title(b, page, "LVGL demo")?;
    let card = stock::card(b, page, CARD_H)?;
    let slider = b.create(Class::Slider, card)?;
    b.size(slider, 300, 16)?;
    b.align(slider, Align::TopMid, 0, 40)?;
    b.range(slider, 0, 100)?;
    b.value(slider, ARC_START, false)?;
    stock::slider(b, slider)?;
    b.events(slider, &[Ev::ValueChanged])?;
    // arc range is fixed 0..100 in the firmware: the slider's range matches it
    let arc = b.create(Class::Arc, card)?;
    b.size(arc, 120, 120)?;
    b.align(arc, Align::BottomMid, 0, -20)?;
    b.value(arc, ARC_START, false)?;
    stock::arc(b, arc, 10)?;
    b.flags(arc, &[], &[Flag::Clickable])?; // display only: the slider drives it
    let arc_label = b.create(Class::Label, arc)?;
    b.align(arc_label, Align::Center, 0, 0)?;
    b.text(arc_label, "70")?;
    Ok(Screen { page, bar, title, slider, arc, arc_label, list: ROOT, rows: [ROOT; ROWS] })
}

/// The rows, in a bare column under the card: a tap plays the row's pattern.
pub fn list(b: &mut Batch, s: &mut Screen) -> Result<(), LvError> {
    let list = b.create(Class::Obj, s.page)?;
    b.size(list, stock::ROW_W, CONTENT)?;
    for p in [Prop::PadTop, Prop::PadBottom, Prop::PadLeft, Prop::PadRight, Prop::BorderWidth, Prop::BgOpa] {
        b.style(list, p, 0, 0)?;
    }
    b.style(list, Prop::PadRow, 0, stock::ROW_GAP)?;
    b.flex(list, Flow::Column, FlexAlign::Start, FlexAlign::Center, FlexAlign::Start)?;
    b.flags(list, &[], &[Flag::Scrollable])?; // the page scrolls, not the list
    for (row, (letter, name)) in s.rows.iter_mut().zip(NAMES) {
        *row = stock::row(b, list, letter, name, "")?;
    }
    s.list = list;
    Ok(())
}

/// `!del`: the list goes, with its rows and their circles and labels.
pub fn del_list(b: &mut Batch, s: &Screen) -> Result<(), LvError> {
    b.del(s.list)
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
    fn apply(t: &mut Table, records: &[u8]) {
        for r in lvcmd::records(records) {
            lvcmd::apply(t, &r.unwrap());
        }
    }

    // the three batches of a launch, validated in order on one Batch (it owns the handle allocator, as in the app);
    // returns the batch, the screen and the loader's table
    fn launch() -> (Batch, Screen, Table) {
        let mut f = batch();
        fonts(&mut f).unwrap();
        assert_eq!(lvcmd::validate(f.bytes(), &Table::new(), 0), Ok(()));
        let mut t = Table::new();
        let mut b = batch();
        let mut s = build(&mut b).unwrap();
        let n = b.bytes().len();
        std::println!("screen batch: {n} B");
        // the theme needs the system font: refused without it
        assert!(lvcmd::validate(b.bytes(), &t, 0).is_err());
        assert_eq!(lvcmd::validate(b.bytes(), &t, 1 << stock::FONT), Ok(()));
        apply(&mut t, b.bytes());
        list(&mut b, &mut s).unwrap();
        let rows = &b.bytes()[n..];
        std::println!("list batch: {} B", rows.len());
        assert_eq!(lvcmd::validate(rows, &t, 1), Ok(()));
        apply(&mut t, rows);
        (b, s, t)
    }

    #[test]
    fn screen_validates_in_three_batches() {
        let (mut b, s, t) = launch();
        let n = b.bytes().len();
        for v in [0, 7, 42, 100, 250] {
            set_arc(&mut b, &s, v).unwrap();
        }
        assert_eq!(lvcmd::validate(&b.bytes()[n..], &t, 1), Ok(()));
        let n = b.bytes().len();
        bad(&mut b, &s).unwrap();
        assert!(lvcmd::validate(&b.bytes()[n..], &t, 1).is_err());
    }

    #[test]
    fn del_list_frees_rows_and_keeps_the_rest() {
        let (mut b, mut s, mut t) = launch();
        b.clear(); // stands in for the submit
        del_list(&mut b, &s).unwrap();
        assert_eq!(lvcmd::validate(b.bytes(), &t, 1), Ok(()));
        apply(&mut t, b.bytes());
        // the list, 4 rows and their circle, letter, name and detail: one run of handles
        for h in s.list.0..s.list.0 + 21 {
            assert_eq!(t.class(h), None, "{h}");
        }
        for o in [s.page, s.bar, s.title, s.slider, s.arc] {
            assert!(t.class(o.0).is_some(), "{o:?}");
        }
        // a fresh list takes the same handles again
        let first = s.list;
        b.clear();
        list(&mut b, &mut s).unwrap();
        assert_eq!(s.list, first);
        assert_eq!(lvcmd::validate(b.bytes(), &t, 1), Ok(()));
    }

    #[test]
    fn arc_label_text() {
        let (_, s, _) = launch();
        for (v, want) in [(0, "0"), (7, "7"), (42, "42"), (100, "100"), (-3, "0"), (250, "100")] {
            let mut b = batch();
            set_arc(&mut b, &s, v).unwrap();
            let bytes = b.bytes();
            assert_eq!(&bytes[bytes.len() - want.len()..], want.as_bytes(), "{v}");
        }
    }
}
