//! The LVGL screen in the stock look: a crown-scrolled page with the round scroll bar, a status line and every host
//! message as a log line, newest on top (a reversed flex column: the SDK has no scroll-to, so the newest line is
//! where the page opens).

use cmf_wp2_app::lv::*;
use cmf_wp2_app::stock;

/// Log lines kept; the oldest goes when a new one comes (192-object budget, GUI heap).
pub const KEEP: usize = 40;
const LINE_W: i32 = 360; // fits the round panel around its middle

pub struct Log {
    pub status: Obj,
    list: Obj,
    lines: [Obj; KEEP],
    next: usize, // ring slot of the next line; holds the oldest once the ring is full
    full: bool,
}

/// The screen batch (after `stock::font` in its own batch).
pub fn build(b: &mut Batch) -> Result<Log, LvError> {
    stock::theme(b)?;
    let (page, _bar) = stock::page(b)?;
    stock::title(b, page, "API demo")?;
    let status = b.create(Class::Label, page)?;
    b.size(status, stock::ROW_W, CONTENT)?;
    b.style(status, Prop::TextAlign, 0, 2)?; // LV_TEXT_ALIGN_CENTER
    b.style(status, Prop::TextColor, 0, stock::GREY)?;
    let list = b.create(Class::Obj, page)?;
    b.size(list, stock::ROW_W, CONTENT)?;
    for p in [Prop::PadTop, Prop::PadBottom, Prop::PadLeft, Prop::PadRight, Prop::BorderWidth, Prop::BgOpa] {
        b.style(list, p, 0, 0)?;
    }
    b.style(list, Prop::PadRow, 0, 12)?;
    b.flex(list, Flow::ColumnReverse, FlexAlign::Start, FlexAlign::Center, FlexAlign::Center)?; // track centred: lines are narrower than the list
    b.flags(list, &[], &[Flag::Scrollable])?; // the page scrolls, not the list
    Ok(Log { status, list, lines: [ROOT; KEEP], next: 0, full: false })
}

impl Log {
    /// A new line on top of the log (`text` ≤ 128 B, a host message is); drops the oldest past [`KEEP`].
    pub fn push(&mut self, b: &mut Batch, text: &str) -> Result<(), LvError> {
        if self.full {
            b.del(self.lines[self.next])?;
        }
        let l = b.create(Class::Label, self.list)?;
        b.size(l, LINE_W, CONTENT)?; // wraps
        b.text(l, text)?;
        self.lines[self.next] = l;
        self.next = (self.next + 1) % KEEP;
        self.full |= self.next == 0;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use std::boxed::Box;
    use wp2_core::lvcmd::{self, Table};

    fn apply(t: &mut Table, records: &[u8]) {
        for r in lvcmd::records(records) {
            lvcmd::apply(t, &r.unwrap());
        }
    }

    #[test]
    fn screen_and_a_long_log_stay_valid() {
        let mut b = Batch::with_buf(Box::leak(Box::new([0; BUF])));
        stock::font(&mut b).unwrap();
        assert_eq!(lvcmd::validate(b.bytes(), &Table::new(), 0), Ok(()));
        b.clear();
        let mut t = Table::new();
        let mut log = build(&mut b).unwrap();
        b.text(log.status, "12:00:00  87%  count 3").unwrap();
        assert_eq!(lvcmd::validate(b.bytes(), &t, 1 << stock::FONT), Ok(()));
        apply(&mut t, b.bytes());
        // far past the 192-object budget: only the ring's deletes keep it valid
        let long = "ж".repeat(64);
        for i in 0..5 * KEEP {
            b.clear();
            log.push(&mut b, if i % 2 == 0 { &long } else { "spam 7" }).unwrap();
            assert_eq!(lvcmd::validate(b.bytes(), &t, 1 << stock::FONT), Ok(()), "push {i}");
            apply(&mut t, b.bytes());
        }
        assert_eq!(log.lines.iter().filter(|l| t.class(l.0).is_some()).count(), KEEP);
    }
}
