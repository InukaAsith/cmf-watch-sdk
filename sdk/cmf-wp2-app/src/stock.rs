//! The stock look for LVGL apps (ABI 1.2): the system font, the stock colours and the rows of the loader's Apps menu,
//! which copy the stock Settings list (`fw/wp2-loader/src/menu.rs`). LVGL apps always use these (AGENTS.md): the
//! firmware has no theme, so an unstyled label falls back to LVGL's small built-in font.
//!
//! ```ignore
//! stock::font(&mut b)?;                     // a batch of its own
//! b.submit(&lv)?;
//! stock::theme(&mut b)?;                    // first in the screen batch
//! let (page, _bar) = stock::page(&mut b)?;
//! stock::title(&mut b, page, "Settings")?;
//! let row = stock::row(&mut b, page, "W", "Wi-Fi", "On")?;
//! b.submit(&lv)?;
//! ```

use crate::lv::*;

/// Font slot of the system font.
pub const FONT: u8 = 0;
/// The system font: the one of the stock lists and of the Apps menu.
pub const SYSTEM_FONT: StockFont = StockFont::F32M;

// RGB565, read off the stock Settings list in the emulator (fw/wp2-loader/src/menu.rs)
pub const BLACK: i32 = 0x0000;
pub const WHITE: i32 = 0xFFFF;
/// Secondary text: #8E9196.
pub const GREY: i32 = 0x8C92;
/// The stock accent (titles, active parts).
pub const ACCENT: i32 = 0xFA23;
pub const ROW_BG: i32 = 0x2966;
pub const ROW_PRESSED: i32 = 0x4229;
/// Letter circle of a row, and inactive tracks.
pub const LETTER_BG: i32 = 0x52AB;

// geometry of the 466x466 round screen, as the Apps menu
pub const SCREEN: i32 = 466;
pub const TITLE_Y: i32 = 56;
/// Top of the first row under a title.
pub const TOP: i32 = 120;
pub const ROW_W: i32 = 434;
pub const ROW_H: i32 = 120;
pub const ROW_GAP: i32 = 8;
pub const RADIUS: i32 = 44;
pub const ICON: i32 = 96;
pub const ICON_X: i32 = (ROW_H - ICON) / 2;
pub const TEXT_X: i32 = ICON_X + ICON + 20;
pub const PAD_R: i32 = 28;

pub const CIRCLE: i32 = 0x7FFF; // LV_RADIUS_CIRCLE
pub const PRESSED: u32 = 0x20; // LV_STATE_PRESSED
pub const INDICATOR: u32 = 0x02_0000; // LV_PART_INDICATOR
pub const KNOB: u32 = 0x03_0000; // LV_PART_KNOB
pub const SELECTED: u32 = 0x04_0000; // LV_PART_SELECTED

/// Opens the system font into slot [`FONT`]. Submit it as a batch of its own, before the screen.
pub fn font(b: &mut Batch) -> Result<(), LvError> {
    b.font(FONT, SYSTEM_FONT)
}

/// Every label on the screen in the system font, white (inherited from the root). First in the screen batch.
pub fn theme(b: &mut Batch) -> Result<(), LvError> {
    b.style(ROOT, Prop::TextFont, 0, FONT as i32)?;
    b.style(ROOT, Prop::TextColor, 0, WHITE)
}

/// A full-screen page like the Apps menu: a centred column that the crown scrolls, with the round scroll bar.
/// The first child sits at [`TITLE_Y`]; the bottom padding lets the last child scroll up to the title.
/// Returns (page, bar).
pub fn page(b: &mut Batch) -> Result<(Obj, Obj), LvError> {
    let page = b.create(Class::Obj, ROOT)?;
    b.size(page, SCREEN, SCREEN)?;
    for p in [Prop::PadLeft, Prop::PadRight, Prop::BorderWidth, Prop::Radius, Prop::BgOpa] {
        b.style(page, p, 0, 0)?;
    }
    b.style(page, Prop::PadTop, 0, TITLE_Y)?;
    b.style(page, Prop::PadBottom, 0, SCREEN - TITLE_Y - ROW_H)?;
    b.style(page, Prop::PadRow, 0, ROW_GAP)?;
    b.flex(page, Flow::Column, FlexAlign::Start, FlexAlign::Center, FlexAlign::Start)?;
    let bar = b.scroll_bar(page)?;
    b.knob(page)?;
    Ok((page, bar))
}

/// The accent title at the top of a page; the next child lands at [`TOP`].
pub fn title(b: &mut Batch, parent: Obj, text: &str) -> Result<Obj, LvError> {
    let t = b.create(Class::Label, parent)?;
    b.size(t, ROW_W, TOP - TITLE_Y - ROW_GAP)?;
    b.style(t, Prop::TextAlign, 0, 2)?; // LV_TEXT_ALIGN_CENTER
    b.style(t, Prop::TextColor, 0, ACCENT)?;
    b.text(t, text)?;
    Ok(t)
}

/// A row of the Apps menu: a letter circle, the name, a grey `detail` on the right (may be empty). It sends
/// CLICKED. Returns the row; its children follow it in handle order (circle, letter, name, detail).
pub fn row(b: &mut Batch, parent: Obj, letter: &str, name: &str, detail: &str) -> Result<Obj, LvError> {
    let row = card(b, parent, ROW_H)?;
    b.style(row, Prop::BgColor, PRESSED, ROW_PRESSED)?;
    b.events(row, &[Ev::Clicked])?;
    let circle = b.create(Class::Obj, row)?;
    b.size(circle, ICON, ICON)?;
    b.align(circle, Align::LeftMid, ICON_X as i16, 0)?;
    b.style(circle, Prop::BgColor, 0, LETTER_BG)?;
    b.style(circle, Prop::BgOpa, 0, 255)?;
    b.style(circle, Prop::Radius, 0, CIRCLE)?;
    b.flags(circle, &[], &[Flag::Clickable, Flag::Scrollable])?; // a press on it goes to the row
    let l = b.create(Class::Label, circle)?;
    b.text(l, letter)?;
    b.align(l, Align::Center, 0, 0)?;
    let n = b.create(Class::Label, row)?;
    b.text(n, name)?;
    b.align(n, Align::LeftMid, TEXT_X as i16, 0)?;
    let d = b.create(Class::Label, row)?;
    b.text(d, detail)?;
    b.style(d, Prop::TextColor, 0, GREY)?;
    b.align(d, Align::RightMid, -PAD_R as i16, 0)?;
    Ok(row)
}

/// Row height of a focus page: the stock app list's 96 px icon and its 8 px gap.
pub const FOCUS_ROW_H: i32 = ICON + 8;

/// A full-screen page like the stock app list: the crown scrolls it, the round scroll bar shows, and its children
/// follow the stock fisheye ([`Batch::focus`]): the one nearest the centre line is CHECKED and the app gets
/// `FOCUSED` events. The padding lets the first and the last child reach the centre line. `icon` is the side of each
/// child's child 0 on the centre line (0: none). Returns (page, bar).
pub fn focus_page(b: &mut Batch, icon: u16) -> Result<(Obj, Obj), LvError> {
    let (page, bar) = page(b)?;
    let pad = (SCREEN - FOCUS_ROW_H) / 2;
    b.style(page, Prop::PadTop, 0, pad)?;
    b.style(page, Prop::PadBottom, 0, pad)?;
    b.style(page, Prop::PadRow, 0, 0)?;
    b.focus(page, icon)?;
    Ok((page, bar))
}

/// A row of a focus page, [`FOCUS_ROW_H`] high: transparent, a pill in the stock row colour while it is the centred
/// one (CHECKED), lighter while pressed. It sends CLICKED. Its first child is the one the fisheye scales.
pub fn focus_row(b: &mut Batch, parent: Obj) -> Result<Obj, LvError> {
    let row = b.create(Class::Obj, parent)?;
    b.size(row, ROW_W, FOCUS_ROW_H)?;
    for p in [Prop::PadTop, Prop::PadBottom, Prop::PadLeft, Prop::PadRight, Prop::BorderWidth, Prop::BgOpa] {
        b.style(row, p, 0, 0)?;
    }
    b.style(row, Prop::Radius, 0, FOCUS_ROW_H / 2)?;
    b.style(row, Prop::BgColor, CHECKED, ROW_BG)?;
    b.style(row, Prop::BgOpa, CHECKED, 255)?;
    b.style(row, Prop::BgColor, PRESSED, ROW_PRESSED)?;
    b.style(row, Prop::BgOpa, PRESSED, 255)?;
    b.flags(row, &[], &[Flag::Scrollable])?;
    b.events(row, &[Ev::Clicked])?;
    Ok(row)
}

/// A row-coloured, rounded panel of height `h` for controls; not scrollable, so a drag on it scrolls the page.
pub fn card(b: &mut Batch, parent: Obj, h: i32) -> Result<Obj, LvError> {
    let c = b.create(Class::Obj, parent)?;
    b.size(c, ROW_W, h)?;
    b.style(c, Prop::BgColor, 0, ROW_BG)?;
    b.style(c, Prop::BgOpa, 0, 255)?;
    b.style(c, Prop::Radius, 0, RADIUS)?;
    b.style(c, Prop::BorderWidth, 0, 0)?;
    b.flags(c, &[], &[Flag::Scrollable])?;
    Ok(c)
}

/// A slider in the stock colours: grey track, accent indicator, white knob.
pub fn slider(b: &mut Batch, obj: Obj) -> Result<(), LvError> {
    for (part, colour) in [(0, LETTER_BG), (INDICATOR, ACCENT), (KNOB, WHITE)] {
        b.style(obj, Prop::BgColor, part, colour)?;
        b.style(obj, Prop::BgOpa, part, 255)?;
        b.style(obj, Prop::Radius, part, CIRCLE)?;
    }
    Ok(())
}

/// An arc in the stock colours, `width` px thick.
pub fn arc(b: &mut Batch, obj: Obj, width: i32) -> Result<(), LvError> {
    for (part, colour) in [(0, LETTER_BG), (INDICATOR, ACCENT)] {
        b.style(obj, Prop::ArcColor, part, colour)?;
        b.style(obj, Prop::ArcOpa, part, 255)?;
        b.style(obj, Prop::ArcWidth, part, width)?;
        b.style(obj, Prop::ArcRounded, part, 1)?;
    }
    Ok(())
}

// ponytail: validated through lv-demo's host tests (they have wp2-core's validator), not here
