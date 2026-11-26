//! A page in reader view: its title and blocks of text (headings, paragraphs, links, list items) as the phone pulls
//! them out of the page, laid out once into lines the crown scrolls. Kept in RAM only, while the app runs.
//!
//! Phone → watch: `N title` a new page; `t kind link text` a block (kind 0 heading, 1 paragraph, 2 link, 3 list
//! item, 4 more of the block before; `link` the link's index on the page, 255 none); `F` all of it came.

use cmf_wp2_ui::font;

pub const TEXT: usize = 12 * 1024;
pub const BLOCKS: usize = 220;
pub const LINES: usize = 900;
const TITLE: usize = 80;
/// The text's width and line height on the panel.
pub const WIDTH: i32 = 330;
pub const LH: i32 = 28;
/// A list item's text starts this far right of the bullet.
pub const INDENT: i32 = 22;

pub const HEADING: u8 = 0;
pub const PARA: u8 = 1;
pub const LINK: u8 = 2;
pub const ITEM: u8 = 3;
pub const MORE: u8 = 4;

#[derive(Clone, Copy)]
pub struct Block {
    pub kind: u8,
    pub link: u8,
}

#[derive(Clone, Copy)]
pub struct Line {
    pub off: u16,
    pub len: u16,
    pub block: u8,
    /// from the page's top, px
    pub y: u16,
    /// the block's first line (a list item's bullet goes on it): 1 (an integer: the memory is shared, ui.rs)
    pub first: u8,
}

pub struct Page {
    title: [u8; TITLE],
    n_title: usize,
    text: [u8; TEXT],
    n_text: usize,
    pub blocks: [Block; BLOCKS],
    pub n_blocks: usize,
    pub lines: [Line; LINES],
    pub n_lines: usize,
    /// the laid out text's height, px
    pub height: i32,
    pub done: u8,
}

impl Page {
    #[allow(dead_code)]
    pub const fn new() -> Self {
        Page {
            title: [0; TITLE],
            n_title: 0,
            text: [0; TEXT],
            n_text: 0,
            blocks: [Block { kind: 0, link: 255 }; BLOCKS],
            n_blocks: 0,
            lines: [Line { off: 0, len: 0, block: 0, y: 0, first: 0 }; LINES],
            n_lines: 0,
            height: 0,
            done: 0,
        }
    }

    pub fn title(&self) -> &str {
        cmf_wp2_ui::text::fit(&self.title[..self.n_title], TITLE)
    }

    pub fn line_text(&self, l: &Line) -> &str {
        cmf_wp2_ui::text::fit(&self.text[usize::from(l.off)..usize::from(l.off) + usize::from(l.len)], TEXT)
    }

    pub fn clear(&mut self) {
        self.n_title = 0;
        self.n_text = 0;
        self.n_blocks = 0;
        self.n_lines = 0;
        self.height = 0;
        self.done = 0;
    }

    /// A phone's message for the page (N, t, F): true when it was one.
    pub fn apply(&mut self, m: &[u8]) -> bool {
        match m.first() {
            Some(b'N') => {
                self.clear();
                let t = cmf_wp2_ui::text::fit(&m[1..], TITLE).as_bytes();
                self.title[..t.len()].copy_from_slice(t);
                self.n_title = t.len();
                true
            }
            Some(b't') if m.len() >= 4 => {
                self.add(m[1], m[2], &m[3..]);
                true
            }
            Some(b'F') => {
                self.done = 1;
                true
            }
            _ => false,
        }
    }

    fn add(&mut self, kind: u8, link: u8, text: &[u8]) {
        let t = cmf_wp2_ui::text::fit(text, TEXT).as_bytes();
        if self.n_blocks >= BLOCKS || self.n_text + t.len() > TEXT || t.is_empty() {
            return;
        }
        // more of the block before: its kind, no gap
        let (kind, link, more) = match (kind, self.n_blocks) {
            (MORE, 1..) => {
                let b = self.blocks[self.n_blocks - 1];
                (b.kind, b.link, true)
            }
            (MORE, _) => (PARA, 255, false),
            _ => (kind, link, false),
        };
        let off = self.n_text;
        self.text[off..off + t.len()].copy_from_slice(t);
        self.n_text += t.len();
        let idx = self.n_blocks;
        self.blocks[idx] = Block { kind, link };
        self.n_blocks += 1;
        if !more && self.n_lines > 0 {
            self.height += match kind {
                HEADING => 16,
                PARA => 10,
                _ => 4,
            };
        }
        let width = if kind == ITEM { WIDTH - INDENT } else { WIDTH };
        // wrapped into lines: their places in the text (the last slot takes the rest)
        let mut spans = [(0u16, 0u16); 32];
        let n = {
            let s = core::str::from_utf8(&self.text[off..off + t.len()]).unwrap_or("");
            let mut out = [""; 32];
            let n = font::wrap(s, width, &mut out);
            let base = self.text.as_ptr() as usize;
            for (k, l) in out[..n].iter().enumerate() {
                spans[k] = ((l.as_ptr() as usize - base) as u16, l.len() as u16);
            }
            n
        };
        for (k, &(o, len)) in spans[..n].iter().enumerate() {
            if self.n_lines >= LINES {
                break;
            }
            self.lines[self.n_lines] =
                Line { off: o, len, block: idx as u8, y: self.height.min(65000) as u16, first: u8::from(k == 0 && !more) };
            self.n_lines += 1;
            self.height += LH;
        }
    }

    /// The line at `y` px from the page's top.
    pub fn line_at(&self, y: i32) -> Option<&Line> {
        self.lines[..self.n_lines].iter().find(|l| (i32::from(l.y)..i32::from(l.y) + LH).contains(&y))
    }
}
