//! The watch's own emoji: the stock font `/NAND:C/emoji28.font` the notifications draw with, read in place by the
//! loader (`Api::emoji_read`, nothing copied or kept): 52 emoji, 30 × 30 px ARGB6666 (3 bytes a pixel, A R G B 6
//! bits each from the top of a little-endian u24). Drawn inline in text at SIZE px (cmf_wp2_ui::font calls `draw`
//! for emoji code points); the others are boxes.
//!
//! File: u32 0, u32 count, 8 bytes, then per emoji 16 bytes (`cp:u32 w:u16 h:u16 offset:u32 u16 u16`), ascending
//! code points. The pictures are made smaller on first use and kept in a few slots of the icon's memory (free once
//! the app runs, as Offline Maps' ram.rs); the font has no seek, so emoji not kept are read in offset order.

use cmf_wp2_app::Api;

pub const SIZE: i32 = 22;
const SRC: usize = 30;
const MAX: usize = 64;
const SLOTS: usize = 22;
const PX: usize = (SIZE * SIZE) as usize;
/// A pixel not drawn.
const CLEAR: u16 = 0xF81F;

#[repr(C)]
struct Mem {
    px: [[u16; PX]; SLOTS],
    raw: [u8; SRC * SRC * 3],
}

const ROOM: usize = 96 * 96 * 3;
const _: () = assert!(core::mem::size_of::<Mem>() + 8 <= ROOM, "emoji outgrow the icon");

#[cfg(target_os = "none")]
fn mem() -> &'static mut Mem {
    let b = unsafe { (core::ptr::addr_of_mut!(super::WP2_ICON) as *mut u8).add(16) };
    unsafe { &mut *(b.add(b.align_offset(core::mem::align_of::<Mem>())) as *mut Mem) }
}

#[cfg(not(target_os = "none"))]
fn mem() -> &'static mut Mem {
    static mut SIM: Mem = Mem { px: [[0; PX]; SLOTS], raw: [0; SRC * SRC * 3] };
    unsafe { &mut *core::ptr::addr_of_mut!(SIM) }
}

struct Font {
    api: Option<&'static Api>,
    n: usize,
    cp: [u32; MAX],
    off: [u32; MAX],
    /// what each slot holds (0 none) and when it was last drawn
    slot_cp: [u32; SLOTS],
    used: [u32; SLOTS],
    tick: u32,
}

static mut FONT: Font = Font { api: None, n: 0, cp: [0; MAX], off: [0; MAX], slot_cp: [0; SLOTS], used: [0; SLOTS], tick: 0 };

fn font() -> &'static mut Font {
    // one app thread
    unsafe { &mut *core::ptr::addr_of_mut!(FONT) }
}

/// Reads the font's table and has cmf_wp2_ui::font draw emoji with it; without it (an older loader) emoji stay
/// boxes.
pub fn init(api: &'static Api) {
    let f = font();
    f.api = Some(api);
    let mut head = [0u8; 16 + MAX * 16];
    let got = api.emoji_read(0, &mut head);
    if got < 16 {
        return;
    }
    let u32_at = |b: &[u8], i: usize| u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]]);
    let n = (u32_at(&head, 4) as usize).min(MAX).min((got as usize - 16) / 16);
    for i in 0..n {
        let e = &head[16 + i * 16..32 + i * 16];
        if u16::from_le_bytes([e[4], e[5]]) as usize != SRC || u16::from_le_bytes([e[6], e[7]]) as usize != SRC {
            continue;
        }
        // U+1F53E is an empty frame in the stock font: not offered
        if u32_at(e, 0) == 0x1F53E {
            continue;
        }
        f.cp[f.n] = u32_at(e, 0);
        f.off[f.n] = u32_at(e, 8);
        f.n += 1;
    }
    if f.n > 0 {
        cmf_wp2_ui::font::set_emoji(draw);
    }
}

/// The emoji the watch has, for the emoji keyboard.
pub fn all() -> &'static [u32] {
    let f = font();
    &f.cp[..f.n]
}

/// The emoji's slot, read and made smaller if it isn't kept.
fn slot(cp: u32) -> Option<usize> {
    let f = font();
    let i = f.cp[..f.n].binary_search(&cp).ok()?;
    f.tick = f.tick.wrapping_add(1);
    if let Some(s) = f.slot_cp.iter().position(|&c| c == cp) {
        f.used[s] = f.tick;
        return Some(s);
    }
    // an empty slot, else the one drawn longest ago
    let s = (0..SLOTS).min_by_key(|&s| if f.slot_cp[s] == 0 { 0 } else { f.used[s].max(1) })?;
    let m = mem();
    if f.api?.emoji_read(f.off[i] as usize, &mut m.raw) != (SRC * SRC * 3) as i32 {
        return None;
    }
    shrink(&m.raw, &mut m.px[s]);
    f.slot_cp[s] = cp;
    f.used[s] = f.tick;
    Some(s)
}

/// 30 × 30 ARGB6666 → SIZE × SIZE RGB565, each pixel the mean of the source pixels under it (by alpha), edges
/// blended onto a dark ground (the app's black and cards), the faint ones left out.
fn shrink(raw: &[u8], out: &mut [u16; PX]) {
    let n = SIZE as usize;
    for dy in 0..n {
        let (y0, y1) = (dy * SRC / n, ((dy + 1) * SRC).div_ceil(n));
        for dx in 0..n {
            let (x0, x1) = (dx * SRC / n, ((dx + 1) * SRC).div_ceil(n));
            let (mut a, mut r, mut g, mut b, mut k) = (0u32, 0u32, 0u32, 0u32, 0u32);
            for y in y0..y1 {
                for x in x0..x1 {
                    let p = (y * SRC + x) * 3;
                    let v = u32::from(raw[p]) | u32::from(raw[p + 1]) << 8 | u32::from(raw[p + 2]) << 16;
                    let pa = (v >> 18) & 63;
                    a += pa;
                    r += ((v >> 12) & 63) * pa;
                    g += ((v >> 6) & 63) * pa;
                    b += (v & 63) * pa;
                    k += 1;
                }
            }
            out[dy * n + dx] = if a == 0 || a * 4 < k * 63 {
                CLEAR
            } else {
                // mean colour (0..63), then over the ground by the mean alpha
                let (r, g, b) = (r / a, g / a, b / a);
                let al = a / k;
                let mix = |c: u32| (c * al + 4 * (63 - al)) / 63;
                let (r, g, b) = (mix(r), mix(g), mix(b));
                ((r >> 1) << 11 | g << 5 | (b >> 1)) as u16
            };
        }
    }
}

/// cmf_wp2_ui::font's emoji hook: the picture as runs of one colour.
fn draw(cp: u32, x: i32, y: i32, rect: &mut dyn FnMut(i32, i32, i32, i32, u16)) -> bool {
    draw_scaled(cp, x, y, 1, rect)
}

/// The emoji `scale` times as big (the emoji keyboard).
pub fn draw_scaled(cp: u32, x: i32, y: i32, scale: i32, rect: &mut dyn FnMut(i32, i32, i32, i32, u16)) -> bool {
    let Some(s) = slot(cp) else { return false };
    let px = &mem().px[s];
    let n = SIZE as usize;
    for row in 0..n {
        let line = &px[row * n..row * n + n];
        let mut i = 0;
        while i < n {
            let c = line[i];
            let mut j = i + 1;
            while j < n && line[j] == c {
                j += 1;
            }
            if c != CLEAR {
                rect(x + i as i32 * scale, y + row as i32 * scale, (j - i) as i32 * scale, scale, c);
            }
            i = j;
        }
    }
    true
}
