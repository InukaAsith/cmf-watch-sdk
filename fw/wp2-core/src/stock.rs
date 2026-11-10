// JX402 1.0.0.73 only (docs/recon.md). Functions carry the Thumb bit, data does not.
pub const UI_MEM_ALIGNED_ALLOC: u32 = 0x101882C5;
pub const UI_MEM_FREE: u32 = 0x10188381;
// (addr, len): SPI1 (PSRAM) cache write-buffer flush + clean/invalidate range, no-op outside
// 0x34000000..0x3C000000; takes a k_mutex, thread context only
pub const PSRAM_CACHE_FLUSH_INVALIDATE: u32 = 0x101BEE53;

pub const FS_FIND_MOUNT: u32 = 0x10106FA1;
pub const FS_OPEN: u32 = 0x10107031;
pub const FS_CLOSE: u32 = 0x101071E1;
pub const FS_READ: u32 = 0x10107239;
pub const FS_WRITE: u32 = 0x10107295;
pub const FS_SEEK: u32 = 0x101072F1;
pub const FS_TELL: u32 = 0x10107351;
pub const FS_SYNC: u32 = 0x101073A5;
pub const FS_UNLINK: u32 = 0x1010764D;
pub const FS_RENAME: u32 = 0x10107705;

pub const OS_THREAD_CREATE: u32 = 0x101BE1A3;
pub const OS_SLEEP: u32 = 0x101BE1ED;
// z_thread_entry calls it with k_current_get() after the entry returns
pub const K_THREAD_ABORT: u32 = 0x10105C91;
// u64 ticks, CONFIG_SYS_CLOCK_TICKS_PER_SEC 1000 (os_sleep passes ms to k_sleep unscaled)
pub const SYS_CLOCK_TICK_GET: u32 = 0x1019B7E1;
pub const VIEW_OPEN: u32 = 0x101898BD;
// posts "layout" for a view id; a preload handler returns it, else msg 3 never comes
pub const VIEW_LAYOUT: u32 = 0x101D72FF;
pub const VIEW_TABLE: u32 = 0x100004B8;
pub const VIEW_TABLE_END: u32 = 0x100013E8;

pub const LV_DISP_GET_SCR_ACT: u32 = 0x101255E1;
pub const LV_IMG_CREATE: u32 = 0x1013ED39;
pub const LV_IMG_SET_SRC: u32 = 0x1013ED51;
pub const LV_OBJ_SET_POS: u32 = 0x1012926D;
pub const LV_OBJ_SET_SIZE: u32 = 0x101294C9;
pub const LV_OBJ_INVALIDATE: u32 = 0x10129FD9;
pub const LV_OBJ_DEL: u32 = 0x1012C775;
pub const LV_TIMER_CREATE: u32 = 0x1013B10D;
pub const LV_TIMER_DEL: u32 = 0x1013B1A5;
pub const LV_OBJ_ADD_FLAG: u32 = 0x10128D39;
pub const LV_OBJ_ADD_EVENT_CB: u32 = 0x10125A95;
pub const LV_EVENT_GET_CODE: u32 = 0x101C4C9B;
pub const LV_EVENT_GET_PARAM: u32 = 0x101C4CA3;
// lv_point_t is two i32 here (LV_USE_LARGE_COORD)
pub const LV_INDEV_GET_POINT: u32 = 0x101C4F57;

pub const PWM_DEVICE: u32 = 0x101DF1C0;
pub const PWM_PIN_SET_CYCLES: u32 = 0x1012197D;
pub const PWM_OFF: u32 = 0x101214AD;
pub const CMU_PWMCLK: u32 = 0x4000_1068;
pub const PWM_REGS: u32 = 0x4006_0000;

pub const POPUP_FLAGS: u32 = 0x3800FAF4;

pub const KEY_CROWN_PRESS: u32 = 0x0400_0001;
pub const KEY_CROWN_CW: u32 = 0xFFFF_006F;
pub const KEY_CROWN_CCW: u32 = 0xFFFF_006E;
