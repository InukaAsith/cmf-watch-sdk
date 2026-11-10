use crate::{motor, ui};
use core::ptr::addr_of_mut;
use core::sync::atomic::{AtomicU32, Ordering};
use wp2_core::events::{Api, Event, Queue};
use wp2_core::heap::{self, RES};
use wp2_core::layout::stub_ram;
use wp2_core::load;
use wp2_core::stock::{K_THREAD_ABORT, OS_SLEEP, OS_THREAD_CREATE, SYS_CLOCK_TICK_GET};

pub const NONE: u8 = 0xFF;
// lowest preemptible priority (idle is 15): every stock thread but standby, actlog and shell_uart (also 14)
// runs first, so a busy app cannot stall BLE, sensors or the UI (docs/recon.md)
const APP_PRIO: i32 = 14;
const FLAG: u32 = 1 << 31;
const TEXT_MAX: usize = 64;

#[repr(C, align(8))]
struct ThreadBuf([u8; 0x118 + 0x2000]);

// k_thread (0x118) + stack; in the loader's .bss, so inside the loader block
static mut THREAD_BUF: ThreadBuf = ThreadBuf([0; 0x118 + 0x2000]);
pub static QUEUE: Queue = Queue::new();
static EXITED: AtomicU32 = AtomicU32::new(0);

static API: Api = Api {
    abi: 1,
    abi_minor: 0,
    width: 466,
    height: 466,
    fill_rect,
    draw_text,
    present,
    next_event,
    motor: motor::motor,
    millis,
    exit,
};

type Entry = extern "C" fn(*const Api) -> i32;
type ThreadFn = extern "C" fn(u32, *const Api, u32);

pub fn running() -> bool {
    stub_ram().running_slot != NONE
}

pub fn exited() -> bool {
    EXITED.load(Ordering::Acquire) & FLAG != 0
}

pub fn launch(slot: u8) -> Result<(), i32> {
    let mut file = [0u8; 0x2C];
    let mut scratch = [0u8; 0x200];
    let (block, h) = load::load(slot, &mut file, &mut scratch)?;
    let ram = stub_ram();
    ram.app_block = block as u32;
    ram.app_size = h.image_size;
    ram.running_slot = slot;
    EXITED.store(0, Ordering::Release);
    QUEUE.clear();
    ui::canvas().fill_rect(0, 0, 466, 466, 0);
    ui::DIRTY.store(true, Ordering::Release);
    let buf = unsafe { &mut *addr_of_mut!(THREAD_BUF) };
    let entry = (block as u32 + h.entry) | 1;
    // os_thread_create(buf, size, entry, p1, p2, p3, prio, options, delay_ms)
    stock!(OS_THREAD_CREATE, fn(*mut u8, u32, ThreadFn, u32, *const Api, u32, i32, u32, i32) -> u32)(
        buf.0.as_mut_ptr(), buf.0.len() as u32, app_thread, entry, &API, 0, APP_PRIO, 0, 0,
    );
    Ok(())
}

// UI thread only. The block is freed after the abort: the app thread never runs again.
pub fn stop_app() {
    if !running() {
        return;
    }
    // abort first: the app thread cannot start the motor again
    stock!(K_THREAD_ABORT, fn(*mut u8))(addr_of_mut!(THREAD_BUF) as *mut u8);
    motor::off();
    let ram = stub_ram();
    unsafe { heap::free(RES, ram.app_block as *mut u8) };
    ram.app_block = 0;
    ram.app_size = 0;
    ram.running_slot = NONE;
}

extern "C" fn app_thread(entry: u32, api: *const Api, _: u32) {
    let entry: Entry = unsafe { core::mem::transmute(entry as usize) };
    let code = entry(api);
    park(code)
}

fn park(code: i32) -> ! {
    EXITED.store(code as u32 | FLAG, Ordering::Release);
    loop {
        sleep(1000);
    }
}

fn sleep(ms: u32) {
    stock!(OS_SLEEP, fn(u32))(ms)
}

extern "C" fn fill_rect(x: i16, y: i16, w: u16, h: u16, c: u16) {
    ui::canvas().fill_rect(x, y, w, h, c)
}

extern "C" fn draw_text(x: i16, y: i16, text: *const u8, len: usize, c: u16) {
    if text.is_null() {
        return;
    }
    let s = unsafe { core::slice::from_raw_parts(text, len.min(TEXT_MAX)) };
    ui::canvas().text(x, y, s, c);
}

extern "C" fn present() {
    ui::DIRTY.store(true, Ordering::Release);
}

extern "C" fn next_event(ev: *mut Event, timeout_ms: u32) -> bool {
    let start = millis();
    loop {
        if let Some(e) = QUEUE.pop() {
            if !ev.is_null() {
                unsafe { ev.write(e) };
            }
            return true;
        }
        if timeout_ms != u32::MAX && millis().wrapping_sub(start) >= timeout_ms {
            return false;
        }
        sleep(5);
    }
}

pub extern "C" fn millis() -> u32 {
    stock!(SYS_CLOCK_TICK_GET, fn() -> u64)() as u32
}

extern "C" fn exit(code: i32) -> ! {
    park(code)
}
