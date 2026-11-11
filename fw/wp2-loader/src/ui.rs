use core::ffi::c_void;
use core::ptr::{addr_of_mut, null, null_mut};
use core::sync::atomic::AtomicBool;
use wp2_core::gfx::{Canvas, H, W};
use wp2_core::heap::{self, FB};
use wp2_core::layout::E_NOMEM;
use wp2_core::stock::*;

// LV_COLOR_DEPTH 16, LV_COLOR_16_SWAP 0 (docs/recon.md)
pub const FB_BYTES: u32 = (W * H * 2) as u32;
const CF_TRUE_COLOR: u32 = 4;
const FLAG_CLICKABLE: u32 = 2;
pub const PRESSED: u32 = 1;
pub const RELEASED: u32 = 8;
// vendor event: param points to the i32 crown step
pub const KNOB: u32 = 0x2D;

pub type Cb = extern "C" fn(*mut c_void);
type Obj = *mut c_void;

// set by the menu and the app's present(); the tick invalidates the image
pub static DIRTY: AtomicBool = AtomicBool::new(false);

#[repr(C)]
struct ImgDsc {
    header: u32,
    data_size: u32,
    data: *const u8,
}

struct Ui {
    fb: *mut u8,
    img: Obj,
    timer: *mut c_void,
    dsc: ImgDsc,
}

static mut UI: Ui = Ui { fb: null_mut(), img: null_mut(), timer: null_mut(), dsc: ImgDsc { header: 0, data_size: 0, data: null() } };

fn ui() -> &'static mut Ui {
    unsafe { &mut *addr_of_mut!(UI) }
}

pub fn preload(view_id: u32) -> i32 {
    let fb = heap::alloc(FB, FB_BYTES, 0x40);
    if fb.is_null() {
        return E_NOMEM;
    }
    let rc = stock!(VIEW_LAYOUT, fn(u32) -> i32)(view_id);
    if rc < 0 {
        unsafe { heap::free(FB, fb) };
        return rc;
    }
    ui().fb = fb;
    rc
}

pub fn layout(data: *mut c_void, input: Cb, tick: Cb) -> i32 {
    let u = ui();
    let scr = stock!(LV_DISP_GET_SCR_ACT, fn(*mut c_void) -> Obj)(unsafe { *(data as *const *mut c_void) });
    u.dsc = ImgDsc { header: CF_TRUE_COLOR | (W as u32) << 10 | (H as u32) << 21, data_size: FB_BYTES, data: u.fb };
    u.img = stock!(LV_IMG_CREATE, fn(Obj) -> Obj)(scr);
    stock!(LV_IMG_SET_SRC, fn(Obj, *const ImgDsc))(u.img, &u.dsc);
    stock!(LV_OBJ_ADD_FLAG, fn(Obj, u32))(u.img, FLAG_CLICKABLE);
    // a KNOB filter also registers the image as the display's knob object (crown events), docs/recon.md
    for filter in [PRESSED, RELEASED, KNOB] {
        stock!(LV_OBJ_ADD_EVENT_CB, fn(Obj, Cb, u32, *mut c_void) -> *mut c_void)(u.img, input, filter, null_mut());
    }
    u.timer = stock!(LV_TIMER_CREATE, fn(Cb, u32, *mut c_void) -> *mut c_void)(tick, 20, null_mut());
    0
}

// The loader block is freed right after this returns: LVGL must keep no pointer into it.
pub fn delete() {
    let u = ui();
    if !u.timer.is_null() {
        stock!(LV_TIMER_DEL, fn(*mut c_void))(u.timer);
    }
    if !u.img.is_null() {
        stock!(LV_OBJ_DEL, fn(Obj))(u.img);
    }
    unsafe { heap::free(FB, u.fb) };
    u.fb = null_mut();
    u.img = null_mut();
    u.timer = null_mut();
}

pub fn invalidate() {
    let u = ui();
    if !u.img.is_null() {
        wp2_core::load::sync_psram(u.fb, FB_BYTES);
        stock!(LV_OBJ_INVALIDATE, fn(Obj))(u.img);
    }
}

pub fn canvas() -> Canvas<'static> {
    Canvas { px: unsafe { core::slice::from_raw_parts_mut(ui().fb as *mut u16, W * H) } }
}

pub fn event_code(e: *mut c_void) -> u32 {
    stock!(LV_EVENT_GET_CODE, fn(*mut c_void) -> u32)(e)
}

pub fn knob_step(e: *mut c_void) -> i32 {
    let p = stock!(LV_EVENT_GET_PARAM, fn(*mut c_void) -> *const i32)(e);
    if p.is_null() { 0 } else { unsafe { *p } }
}

// For PRESSED/RELEASED the event parameter is the active indev.
pub fn touch_point(e: *mut c_void) -> (i32, i32) {
    let indev = stock!(LV_EVENT_GET_PARAM, fn(*mut c_void) -> *mut c_void)(e);
    let mut p = [-1i32; 2];
    stock!(LV_INDEV_GET_POINT, fn(*mut c_void, *mut i32))(indev, p.as_mut_ptr());
    (p[0], p[1])
}
