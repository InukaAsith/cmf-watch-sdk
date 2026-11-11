//! ABI 1.2: build LVGL screens with the watch's own widgets (docs/specs/2026-11-11-lvgl-apps-design.md).
//!
//! The app never calls LVGL. It fills a [`Batch`] (the spec's command records), then [`Batch::submit`] hands the
//! whole buffer to the loader, which validates it and runs it on the UI thread. Handles are picked here: the lowest
//! free one, at most 192 live objects; [`ROOT`] is the loader's 466x466 root. A bad batch changes nothing on the
//! watch but its handles are then out of step, so treat a `Code` error from `submit` as a bug.
//!
//! Open fonts in a batch of their own (`font`, `submit`): a font file that fails to open stops the batch there.

use core::sync::atomic::{AtomicBool, Ordering};

/// Batch buffer size; a record that does not fit gives [`LvError::Full`]: submit and continue.
pub const BUF: usize = 2048;
const MAX_OBJS: usize = 192;
const FREE: u8 = 0xFF;
const SCROLLBAR: u8 = 7;
/// What the loader answers to a record it rejects.
const INVALID: LvError = LvError::Code(-22);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Obj(pub u16);
/// The app root: 466x466, black, not scrollable. Not a valid target of DEL, SIZE, ALIGN or EVENTS.
pub const ROOT: Obj = Obj(0);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Class {
    Obj = 0,
    Label = 1,
    Btn = 2,
    /// Fixed range 0..=100 (no `range` for arcs), value is an `i16`.
    Arc = 3,
    Bar = 4,
    Slider = 5,
    Roller = 6,
}

/// `lv_align_t`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Align {
    Default = 0,
    TopLeft,
    TopMid,
    TopRight,
    BottomLeft,
    BottomMid,
    BottomRight,
    LeftMid,
    RightMid,
    Center,
    OutTopLeft,
    OutTopMid,
    OutTopRight,
    OutBottomLeft,
    OutBottomMid,
    OutBottomRight,
    OutLeftTop,
    OutLeftMid,
    OutLeftBottom,
    OutRightTop,
    OutRightMid,
    OutRightBottom,
}

/// The `lv_obj_flag_t` bits the loader accepts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum Flag {
    Hidden = 1 << 0,
    Clickable = 1 << 1,
    Checkable = 1 << 3,
    Scrollable = 1 << 4,
    ScrollElastic = 1 << 5,
    ScrollMomentum = 1 << 6,
}

/// Object events delivered as `Event { kind: LV, x: handle, .. }` once subscribed with [`Batch::events`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Ev {
    Pressed = 0,
    Clicked = 1,
    LongPressed = 2,
    Released = 3,
    ValueChanged = 4,
}

/// `lv_flex_flow_t`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Flow {
    Row = 0,
    Column = 1,
    RowWrap = 4,
    ColumnWrap = 5,
    RowReverse = 8,
    ColumnReverse = 9,
    RowWrapReverse = 12,
    ColumnWrapReverse = 13,
}

/// `lv_flex_align_t`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum FlexAlign {
    Start = 0,
    End = 1,
    Center = 2,
    SpaceEvenly = 3,
    SpaceAround = 4,
    SpaceBetween = 5,
}

/// The 48 `lv_obj_set_style_*` setters of the stock image, by their 8.3.10 style property id
/// (`wp2_core::stock::STYLE_PROPS`; a wp2-core test keeps both lists equal). `Actions*` are ids the Actions fork
/// added, name unknown. `Opa` and `AnimSpeed` are the two names left for ids 96 and 102 (by upstream order).
/// `TextFont` takes a font slot opened with [`Batch::font`], not a pointer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u16)]
pub enum Prop {
    Width = 1,
    Height = 4,
    MaxHeight = 6,
    X = 7,
    Y = 8,
    Align = 9,
    Layout = 10,
    Radius = 11,
    PadTop = 16,
    PadBottom = 17,
    PadLeft = 18,
    PadRight = 19,
    PadRow = 20,
    PadColumn = 21,
    BaseDir = 22,
    BgColor = 32,
    BgOpa = 33,
    BgGradColor = 34,
    BgImgSrc = 40,
    Actions45 = 45,
    BorderColor = 48,
    BorderWidth = 50,
    OutlineWidth = 53,
    OutlineColor = 54,
    OutlineOpa = 55,
    ImgOpa = 70,
    ImgRecolor = 71,
    ImgRecolorOpa = 72,
    ArcWidth = 80,
    ArcRounded = 81,
    ArcColor = 82,
    ArcOpa = 83,
    ArcImgSrc = 84,
    TextColor = 85,
    TextOpa = 86,
    TextFont = 87,
    TextLineSpace = 89,
    TextAlign = 91,
    Actions92 = 92,
    Actions93 = 93,
    Opa = 96,
    AnimSpeed = 102,
    TranslateX = 107,
    TranslateY = 108,
    FlexFlow = 113,
    FlexMainPlace = 114,
    FlexCrossPlace = 115,
    FlexTrackPlace = 116,
}

impl Prop {
    pub const ALL: [Prop; 48] = {
        use Prop::*;
        [
            Width, Height, MaxHeight, X, Y, Align, Layout, Radius, PadTop, PadBottom, PadLeft, PadRight, PadRow,
            PadColumn, BaseDir, BgColor, BgOpa, BgGradColor, BgImgSrc, Actions45, BorderColor, BorderWidth,
            OutlineWidth, OutlineColor, OutlineOpa, ImgOpa, ImgRecolor, ImgRecolorOpa, ArcWidth, ArcRounded,
            ArcColor, ArcOpa, ArcImgSrc, TextColor, TextOpa, TextFont, TextLineSpace, TextAlign, Actions92,
            Actions93, Opa, AnimSpeed, TranslateX, TranslateY, FlexFlow, FlexMainPlace, FlexCrossPlace,
            FlexTrackPlace,
        ]
    };
}

/// The stock NAND fonts `/NAND:A/font<name>.font`, in the loader's table order (the discriminant is the index).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum StockFont {
    F18M,
    F24L,
    F24M,
    F24R,
    F28M,
    F28R,
    F30R,
    F32M,
    F32R,
    F34R,
    F36L,
    F36M,
    F36N,
    F36R,
    F44L,
    F48M,
}

/// `LV_PCT(n)` for a size, x or y value.
pub const fn pct(n: i32) -> i32 {
    0x2000_0000 | if n < 0 { 1000 - n } else { n }
}

/// `LV_SIZE_CONTENT`.
pub const CONTENT: i32 = 0x2000_07D1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LvError {
    /// The record does not fit: [`Batch::submit`], then repeat the call.
    Full,
    /// 192 handles are in use.
    Budget,
    /// The loader's return code of `lv_submit` (`-22` invalid, `-12` budget, `-2` font file), or `-22` for a
    /// record the SDK refuses itself (nothing is written then).
    Code(i32),
}

/// The loader's command channel, from [`crate::Api::lv`].
#[derive(Clone, Copy)]
pub struct Lv(pub(crate) extern "C" fn(*const u8, usize) -> i32);

impl Lv {
    /// Raw `lv_submit`: `0` or an error code of the spec.
    pub fn submit(&self, records: &[u8]) -> i32 {
        (self.0)(records.as_ptr(), records.len())
    }
}

static mut BUFFER: [u8; BUF] = [0; BUF];
static TAKEN: AtomicBool = AtomicBool::new(false);

/// Records of one screen update, plus the handle bookkeeping (class and parent per handle, like the loader's).
pub struct Batch {
    buf: &'static mut [u8; BUF],
    len: usize,
    class: [u8; MAX_OBJS + 1],
    parent: [u16; MAX_OBJS + 1],
}

impl Batch {
    /// The app's batch over the static 2 KiB buffer; `None` the second time (one batch per app).
    pub fn new() -> Option<Batch> {
        if TAKEN.swap(true, Ordering::Relaxed) {
            return None;
        }
        // SAFETY: TAKEN guarantees this is the only reference to BUFFER
        Some(Batch::with_buf(unsafe { &mut *core::ptr::addr_of_mut!(BUFFER) }))
    }

    pub fn with_buf(buf: &'static mut [u8; BUF]) -> Batch {
        let mut class = [FREE; MAX_OBJS + 1];
        class[0] = Class::Obj as u8;
        Batch { buf, len: 0, class, parent: [0; MAX_OBJS + 1] }
    }

    /// The records so far.
    pub fn bytes(&self) -> &[u8] {
        &self.buf[..self.len]
    }

    fn put(&mut self, op: u8, h: u16, payload: &[&[u8]]) -> Result<(), LvError> {
        let n: usize = payload.iter().map(|p| p.len()).sum();
        if n > 255 {
            return Err(INVALID);
        }
        if self.len + 4 + n > BUF {
            return Err(LvError::Full);
        }
        let mut at = self.len;
        for part in [&[op, n as u8][..], &h.to_le_bytes()].iter().chain(payload) {
            self.buf[at..at + part.len()].copy_from_slice(part);
            at += part.len();
        }
        self.len = at;
        Ok(())
    }

    fn free_handle(&self) -> Result<u16, LvError> {
        (1..=MAX_OBJS).find(|&h| self.class[h] == FREE).map(|h| h as u16).ok_or(LvError::Budget)
    }

    fn live(&self, o: Obj) -> bool {
        self.class.get(o.0 as usize).is_some_and(|&c| c != FREE)
    }

    pub fn create(&mut self, class: Class, parent: Obj) -> Result<Obj, LvError> {
        let h = self.free_handle()?;
        self.put(0x01, h, &[&[class as u8], &parent.0.to_le_bytes()])?;
        self.class[h as usize] = class as u8;
        self.parent[h as usize] = parent.0;
        Ok(Obj(h))
    }

    /// Deletes `obj`, its children and scroll bars; their handles are free again.
    pub fn del(&mut self, obj: Obj) -> Result<(), LvError> {
        if obj == ROOT || !self.live(obj) {
            return Err(INVALID);
        }
        self.put(0x02, obj.0, &[])?;
        self.class[obj.0 as usize] = FREE;
        // ponytail: fixpoint scan, O(192^2) worst case per del; fine at this size
        loop {
            let mut changed = false;
            for h in 1..=MAX_OBJS {
                if self.class[h] != FREE && self.class[self.parent[h] as usize] == FREE {
                    self.class[h] = FREE;
                    changed = true;
                }
            }
            if !changed {
                return Ok(());
            }
        }
    }

    /// `w`, `h`: pixels, [`pct`] or [`CONTENT`].
    pub fn size(&mut self, obj: Obj, w: i32, h: i32) -> Result<(), LvError> {
        self.put(0x03, obj.0, &[&w.to_le_bytes(), &h.to_le_bytes()])
    }

    pub fn align(&mut self, obj: Obj, align: Align, x: i16, y: i16) -> Result<(), LvError> {
        self.put(0x04, obj.0, &[&[align as u8], &x.to_le_bytes(), &y.to_le_bytes()])
    }

    pub fn flags(&mut self, obj: Obj, set: &[Flag], clear: &[Flag]) -> Result<(), LvError> {
        let bits = |f: &[Flag]| f.iter().fold(0u32, |m, &f| m | f as u32);
        self.put(0x05, obj.0, &[&bits(set).to_le_bytes(), &bits(clear).to_le_bytes()])
    }

    /// `sel` = LVGL part | state (0 = main part, default state). `value`: a number, a 16-bit RGB565 colour, or for
    /// [`Prop::TextFont`] a font slot opened before.
    pub fn style(&mut self, obj: Obj, prop: Prop, sel: u32, value: i32) -> Result<(), LvError> {
        self.put(0x06, obj.0, &[&(prop as u16).to_le_bytes(), &sel.to_le_bytes(), &value.to_le_bytes()])
    }

    /// Opens a stock font into `slot` (0..4) for [`Prop::TextFont`].
    pub fn font(&mut self, slot: u8, font: StockFont) -> Result<(), LvError> {
        self.put(0x07, 0, &[&[slot, font as u8]])
    }

    /// A label's text, at most 128 bytes (the loader refuses more).
    pub fn text(&mut self, obj: Obj, text: &str) -> Result<(), LvError> {
        self.put(0x08, obj.0, &[text.as_bytes()])
    }

    /// Arc (`i16`), bar, slider; a roller's selected row.
    pub fn value(&mut self, obj: Obj, v: i32, anim: bool) -> Result<(), LvError> {
        self.put(0x09, obj.0, &[&v.to_le_bytes(), &[anim as u8]])
    }

    /// Bar and slider only: the arc's range is fixed at 0..=100 and the other classes have none.
    pub fn range(&mut self, obj: Obj, min: i32, max: i32) -> Result<(), LvError> {
        if !self.live(obj) || ![Class::Bar as u8, Class::Slider as u8].contains(&self.class[obj.0 as usize]) {
            return Err(INVALID);
        }
        self.put(0x0A, obj.0, &[&min.to_le_bytes(), &max.to_le_bytes()])
    }

    /// A roller's rows, `\n`-separated, at most 240 bytes; `infinite` wraps around.
    pub fn options(&mut self, obj: Obj, options: &str, infinite: bool) -> Result<(), LvError> {
        self.put(0x0B, obj.0, &[&[infinite as u8], options.as_bytes()])
    }

    /// Subscribes to these events of `obj` (replaces the previous set).
    pub fn events(&mut self, obj: Obj, events: &[Ev]) -> Result<(), LvError> {
        let mask = events.iter().fold(0u8, |m, &e| m | 1 << e as u8);
        self.put(0x0C, obj.0, &[&[mask]])
    }

    pub fn flex(&mut self, obj: Obj, flow: Flow, main: FlexAlign, cross: FlexAlign, track: FlexAlign) -> Result<(), LvError> {
        self.put(0x0D, obj.0, &[&[flow as u8, main as u8, cross as u8, track as u8]])
    }

    /// The vendor round scroll bar for `target` (crown scrolls it). Its handle goes with `target`.
    pub fn scroll_bar(&mut self, target: Obj) -> Result<Obj, LvError> {
        let h = self.free_handle()?;
        self.put(0x0E, h, &[&target.0.to_le_bytes()])?;
        self.class[h as usize] = SCROLLBAR;
        self.parent[h as usize] = target.0;
        Ok(Obj(h))
    }

    /// The crown goes to `obj`: a scrollable one scrolls, others send `CROWN` events.
    pub fn knob(&mut self, obj: Obj) -> Result<(), LvError> {
        self.put(0x0F, obj.0, &[])
    }

    /// Arc background angles and rotation, degrees 0..=360.
    pub fn angles(&mut self, obj: Obj, start: u16, end: u16, rotation: u16) -> Result<(), LvError> {
        self.put(0x10, obj.0, &[&start.to_le_bytes(), &end.to_le_bytes(), &rotation.to_le_bytes()])
    }

    /// Sends the records and empties the buffer, even on an error. An empty batch sends nothing.
    pub fn submit(&mut self, lv: &Lv) -> Result<(), LvError> {
        let r = if self.len == 0 { 0 } else { lv.submit(self.bytes()) };
        self.len = 0;
        if r == 0 { Ok(()) } else { Err(LvError::Code(r)) }
    }
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use std::{boxed::Box, vec::Vec};

    fn batch() -> Batch {
        Batch::with_buf(Box::leak(Box::new([0; BUF])))
    }

    /// One record of every op (CREATE … ANGLES) after the spec's example batch.
    pub(crate) fn golden() -> Batch {
        let mut b = batch();
        let l = b.create(Class::Label, ROOT).unwrap();
        b.font(0, StockFont::F32M).unwrap();
        b.style(l, Prop::TextFont, 0, 0).unwrap();
        b.text(l, "Привет").unwrap();
        b.align(l, Align::Center, 0, 0).unwrap();
        b.events(l, &[Ev::Clicked]).unwrap();
        let o = b.create(Class::Obj, ROOT).unwrap();
        let _btn = b.create(Class::Btn, o).unwrap();
        let arc = b.create(Class::Arc, o).unwrap();
        let bar = b.create(Class::Bar, o).unwrap();
        let _sl = b.create(Class::Slider, o).unwrap();
        let roller = b.create(Class::Roller, o).unwrap();
        b.size(o, pct(100), CONTENT).unwrap();
        b.flags(o, &[Flag::Scrollable], &[Flag::Hidden]).unwrap();
        b.value(arc, -5, true).unwrap();
        b.range(bar, 0, 100).unwrap();
        b.options(roller, "a\nb\nc", true).unwrap();
        b.flex(o, Flow::Column, FlexAlign::Center, FlexAlign::Center, FlexAlign::Start).unwrap();
        b.scroll_bar(o).unwrap();
        b.knob(o).unwrap();
        b.angles(arc, 135, 45, 10).unwrap();
        let tmp = b.create(Class::Obj, ROOT).unwrap();
        b.del(tmp).unwrap();
        b
    }

    #[test]
    fn golden_batch_bytes() {
        let b = golden();
        if std::env::var_os("WP2_WRITE_GOLDEN").is_some() {
            std::fs::write(concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/fixtures/lvcmd-golden.bin"), b.bytes())
                .unwrap();
        }
        assert_eq!(b.bytes(), include_bytes!("../../../tests/fixtures/lvcmd-golden.bin"));
    }

    #[test]
    fn handles_reuse_after_del() {
        let mut b = batch();
        let h: Vec<Obj> = (0..3).map(|_| b.create(Class::Obj, ROOT).unwrap()).collect();
        assert_eq!(h, [Obj(1), Obj(2), Obj(3)]);
        b.del(h[1]).unwrap();
        assert_eq!(b.create(Class::Obj, ROOT), Ok(Obj(2)));
        // children and scroll bars go with their parent
        let p = b.create(Class::Obj, ROOT).unwrap();
        let c = b.create(Class::Obj, p).unwrap();
        let bar = b.scroll_bar(p).unwrap();
        b.del(p).unwrap();
        assert_eq!(b.create(Class::Obj, ROOT), Ok(p));
        assert_eq!(b.create(Class::Obj, ROOT), Ok(c));
        assert_eq!(b.create(Class::Obj, ROOT), Ok(bar));
    }

    #[test]
    fn budget_is_192_handles() {
        let mut b = batch();
        for i in 1..=192 {
            assert_eq!(b.create(Class::Obj, ROOT), Ok(Obj(i)));
        }
        let len = b.bytes().len();
        assert_eq!(b.create(Class::Obj, ROOT), Err(LvError::Budget));
        assert_eq!(b.scroll_bar(Obj(1)), Err(LvError::Budget));
        assert_eq!(b.bytes().len(), len);
    }

    #[test]
    fn full_buffer_is_reported() {
        let mut b = batch();
        let l = b.create(Class::Label, ROOT).unwrap();
        let s = "a".repeat(128);
        let mut n = 0;
        while b.text(l, &s) == Ok(()) {
            n += 1;
        }
        assert_eq!(b.text(l, &s), Err(LvError::Full));
        assert!(n > 10 && b.bytes().len() <= BUF);
        // a small record still fits in the gap, and every byte is a whole record
        let mut p = 0;
        while p < b.bytes().len() {
            p += 4 + b.bytes()[p + 1] as usize;
        }
        assert_eq!(p, b.bytes().len());
        // a failed Del frees nothing
        let mut b = batch();
        let o = b.create(Class::Obj, ROOT).unwrap();
        while b.text(o, &s) != Err(LvError::Full) {}
        while b.size(o, 1, 1) == Ok(()) {}
        assert_eq!(b.del(o), Err(LvError::Full));
        assert_eq!(b.create(Class::Obj, ROOT), Err(LvError::Full));
        b.submit(&Lv(ok)).unwrap();
        assert_eq!(b.create(Class::Obj, ROOT), Ok(Obj(2)));
    }

    #[test]
    fn range_is_for_bar_and_slider_only() {
        let mut b = batch();
        let arc = b.create(Class::Arc, ROOT).unwrap();
        let bar = b.create(Class::Bar, ROOT).unwrap();
        let sl = b.create(Class::Slider, ROOT).unwrap();
        let len = b.bytes().len();
        assert_eq!(b.range(arc, 0, 10), Err(LvError::Code(-22)));
        assert_eq!(b.range(Obj(9), 0, 10), Err(LvError::Code(-22)));
        assert_eq!(b.bytes().len(), len);
        assert_eq!(b.range(bar, 0, 10), Ok(()));
        assert_eq!(b.range(sl, 0, 10), Ok(()));
    }

    #[test]
    fn text_over_a_record_is_refused() {
        let mut b = batch();
        let l = b.create(Class::Label, ROOT).unwrap();
        assert_eq!(b.text(l, &"a".repeat(256)), Err(LvError::Code(-22)));
    }

    extern "C" fn ok(_: *const u8, _: usize) -> i32 {
        0
    }
    extern "C" fn nope(_: *const u8, _: usize) -> i32 {
        -22
    }

    #[test]
    fn submit_clears_and_reports_codes() {
        let mut b = batch();
        b.create(Class::Obj, ROOT).unwrap();
        assert_eq!(b.submit(&Lv(nope)), Err(LvError::Code(-22)));
        assert!(b.bytes().is_empty());
        assert_eq!(b.submit(&Lv(nope)), Ok(())); // empty: nothing to send
    }

    #[test]
    fn submit_refuses_old_table() {
        let t = |minor| crate::tests::table(crate::ABI, minor);
        assert!(t(1).lv().is_none());
        assert!(t(2).lv().is_some());
    }
}
