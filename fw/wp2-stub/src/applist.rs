// 26th "Apps" row in the stock app list (list style), built like the stock rows (docs/recon.md).
use core::mem::transmute;

const STOCK_LAYOUT: u32 = 0x1005EEE5;
const OBJ_CREATE: u32 = 0x10127CFD;
const OBJ_GET_CHILD: u32 = 0x1012C3DD;
const OBJ_GET_CHILD_CNT: u32 = 0x1012C4AD;
const OBJ_SET_SIZE: u32 = 0x101294C9;
const OBJ_SET_X: u32 = 0x101290CD;
const OBJ_ALIGN_TO: u32 = 0x1012A87D;
const OBJ_ADD_FLAG: u32 = 0x10128D39;
const OBJ_ADD_EVENT_CB: u32 = 0x10125A95;
const OBJ_ADD_STYLE: u32 = 0x1012BB15;
const VIMG_CREATE: u32 = 0x1005A015;
const VIMG_SET_SRC: u32 = 0x1005A02D;
const VLABEL_CREATE: u32 = 0x1005B579;
const VLABEL_SET_LONG_MODE: u32 = 0x101ACBA5;
const VLABEL_SET_TEXT: u32 = 0x1005B70D;
const OPEN_VIEW: u32 = 0x101898BD;

const ROWS: u8 = 25;
const SHORT_CLICKED: u32 = 4;
const EVENT_BUBBLE: u32 = 0x4000;
const OUT_BOTTOM_LEFT: u32 = 0xD;
const OUT_RIGHT_MID: u32 = 0x14;
const ICON: u32 = 96;
const STRIDE: usize = (ICON as usize + 7) / 8;
const PIXELS: usize = 8 + STRIDE * ICON as usize;

type Obj = *mut u8;

#[repr(C)]
struct ImgDsc {
    header: u32,
    size: u32,
    data: *const u8,
}

#[repr(C)]
struct TextRec {
    text: *const u8,
    zero: u32,
}

unsafe impl Sync for ImgDsc {}
unsafe impl Sync for TextRec {}

// LV_IMG_CF_INDEXED_1BIT: palette (transparent, white) in B,G,R,A, then MSB-first rows.
static ICON_DATA: [u8; PIXELS] = icon();
static ICON_DSC: ImgDsc = ImgDsc { header: 7 | ICON << 10 | ICON << 21, size: PIXELS as u32, data: ICON_DATA.as_ptr() };
static LABEL: TextRec = TextRec { text: b"Apps\0".as_ptr(), zero: 0 };

// 2x2 grid of rounded squares.
const fn icon() -> [u8; PIXELS] {
    const S: usize = 32;
    const GAP: usize = 10;
    const R: usize = 8;
    const M: usize = (ICON as usize - 2 * S - GAP) / 2;
    let mut out = [0u8; PIXELS];
    let mut i = 4;
    while i < 8 {
        out[i] = 0xFF;
        i += 1;
    }
    let mut y = 0;
    while y < ICON as usize {
        let mut x = 0;
        while x < ICON as usize {
            let (cx, cy) = (x.wrapping_sub(M), y.wrapping_sub(M));
            let inside = cx < 2 * S + GAP && cy < 2 * S + GAP;
            if inside && cx % (S + GAP) < S && cy % (S + GAP) < S {
                let (px, py) = (cx % (S + GAP), cy % (S + GAP));
                let dx = if px < R { R - px } else if px >= S - R { px + 1 + R - S } else { 0 };
                let dy = if py < R { R - py } else if py >= S - R { py + 1 + R - S } else { 0 };
                if dx * dx + dy * dy <= R * R {
                    out[8 + y * STRIDE + x / 8] |= 0x80 >> (x % 8);
                }
            }
            x += 1;
        }
        y += 1;
    }
    out
}

macro_rules! stock {
    ($addr:expr, fn($($t:ty),*) -> $r:ty) => {
        unsafe { transmute::<usize, extern "C" fn($($t),*) -> $r>($addr as usize) }
    };
}

pub extern "C" fn wp2_applist_layout(view: *mut u8) -> i32 {
    let rc = stock!(STOCK_LAYOUT, fn(*mut u8) -> i32)(view);
    if rc == 0 {
        unsafe { add_row(*(view.add(0x14) as *const *mut u8)) };
    }
    rc
}

unsafe fn add_row(data: *mut u8) {
    let cont = unsafe { *(data.add(4) as *const Obj) };
    if cont.is_null() || unsafe { *data.add(8) } != ROWS || stock!(OBJ_GET_CHILD_CNT, fn(Obj) -> u32)(cont) != ROWS as u32 {
        return;
    }
    let last = stock!(OBJ_GET_CHILD, fn(Obj, i32) -> Obj)(cont, -1);
    let row = stock!(OBJ_CREATE, fn(Obj) -> Obj)(cont);
    if last.is_null() || row.is_null() {
        return;
    }
    let align_to = stock!(OBJ_ALIGN_TO, fn(Obj, Obj, u32, i32, i32) -> ());
    let add_flag = stock!(OBJ_ADD_FLAG, fn(Obj, u32) -> ());
    let set_size = stock!(OBJ_SET_SIZE, fn(Obj, i32, i32) -> ());
    set_size(row, 0x18A, ICON as i32);
    stock!(OBJ_ADD_EVENT_CB, fn(Obj, extern "C" fn(*mut u8), u32, *mut u8) -> *mut u8)(row, clicked, SHORT_CLICKED, core::ptr::null_mut());
    align_to(row, last, OUT_BOTTOM_LEFT, 0, 8);
    stock!(OBJ_SET_X, fn(Obj, i32) -> ())(row, 0x10);
    let img = stock!(VIMG_CREATE, fn(Obj) -> Obj)(row);
    add_flag(img, EVENT_BUBBLE);
    stock!(VIMG_SET_SRC, fn(Obj, *const ImgDsc) -> ())(img, &ICON_DSC);
    let lab = stock!(VLABEL_CREATE, fn(Obj) -> Obj)(row);
    set_size(lab, 0x172 - ICON as i32, unsafe { *(data.add(0x214) as *const i32) });
    stock!(OBJ_ADD_STYLE, fn(Obj, *mut u8, u32) -> ())(lab, unsafe { data.add(0x200) }, 0);
    stock!(VLABEL_SET_LONG_MODE, fn(Obj, u32) -> ())(lab, 3);
    stock!(VLABEL_SET_TEXT, fn(Obj, *const TextRec) -> ())(lab, &LABEL);
    add_flag(lab, EVENT_BUBBLE);
    align_to(lab, img, OUT_RIGHT_MID, 0x18, 0);
}

extern "C" fn clicked(_e: *mut u8) {
    stock!(OPEN_VIEW, fn(u32, u32) -> i32)(0xF0, 0);
}
