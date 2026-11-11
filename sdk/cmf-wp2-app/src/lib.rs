#![no_std]

use core::sync::atomic::{AtomicPtr, Ordering};

pub mod lv;

pub const ABI: u16 = 1;
pub const ABI_MINOR: u16 = 2;
/// The package header's `abi_minor`: `wp2apps pack` reads this symbol from the ELF.
#[unsafe(no_mangle)]
pub static WP2_ABI_MINOR: u16 = ABI_MINOR;

pub const TOUCH_DOWN: u8 = 1;
pub const TOUCH_UP: u8 = 2;
pub const CROWN: u8 = 3;
pub const BUTTON: u8 = 4;
pub const PAUSE: u8 = 5;
pub const RESUME: u8 = 6;
/// ABI 1.1: a host message is in the inbox, `x` = its length; read it with [`Api::recv`].
pub const MESSAGE: u8 = 7;
/// ABI 1.2: an LVGL object event. `x` = the object's handle, `y` = the LVGL event code, `delta` = the value
/// (slider, arc, bar, roller); see [`lv`].
pub const LV: u8 = 8;

/// Longest message in either direction.
pub const MSG_MAX: usize = 128;
/// Largest blob [`Api::save`] keeps.
pub const STORE_MAX: usize = 4096;

/// `x`, `y`: touch point for `TOUCH_DOWN`/`TOUCH_UP`. `delta`: for `CROWN`, the direction of one detent,
/// `+1` or `-1` per event, never a magnitude (a fast turn is several events); 0 otherwise.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Event {
    pub kind: u8,
    pub _pad: u8,
    pub x: i16,
    pub y: i16,
    pub delta: i16,
}

#[repr(C)]
pub struct Api {
    pub abi: u16,
    pub abi_minor: u16,
    pub width: u16,
    pub height: u16,
    pub fill_rect: extern "C" fn(x: i16, y: i16, w: u16, h: u16, rgb565: u16),
    pub draw_text: extern "C" fn(x: i16, y: i16, text: *const u8, len: usize, rgb565: u16),
    pub present: extern "C" fn(),
    pub next_event: extern "C" fn(event: *mut Event, timeout_ms: u32) -> bool,
    pub motor: extern "C" fn(duty_permille: u16, freq_hz: u16) -> i32,
    pub millis: extern "C" fn() -> u32,
    pub exit: extern "C" fn(code: i32) -> !,
    // ABI 1.1 (docs/abi-1.1.md)
    pub send_msg: extern "C" fn(data: *const u8, len: usize) -> i32,
    pub recv_msg: extern "C" fn(buf: *mut u8, cap: usize) -> i32,
    pub load_data: extern "C" fn(buf: *mut u8, cap: usize) -> i32,
    pub save_data: extern "C" fn(data: *const u8, len: usize) -> i32,
    pub get_time: extern "C" fn(out: *mut Time) -> i32,
    pub get_battery: extern "C" fn() -> i32,
    pub screen_on: extern "C" fn(on: bool) -> i32,
    // ABI 1.2 (docs/specs/2026-11-11-lvgl-apps-design.md): never read when `abi_minor < 2`
    pub lv_submit: extern "C" fn(buf: *const u8, len: usize) -> i32,
}

/// Local wall-clock time as the watch shows it. `weekday` 0 = Sunday; `utc_offset_min` as set by the phone app.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Time {
    pub year: u16,
    pub month: u8,
    pub day: u8,
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
    pub weekday: u8,
    pub millis: u16,
    pub utc_offset_min: i16,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Battery {
    pub percent: u8,
    pub charging: bool,
    pub full: bool,
    pub charger: bool,
}

impl Api {
    pub fn rect(&self, x: i16, y: i16, w: u16, h: u16, rgb565: u16) {
        (self.fill_rect)(x, y, w, h, rgb565)
    }

    /// 8x8 font at 2x, 16 px per char. ASCII and Cyrillic (А-я, Ё, ё) have glyphs, any other char draws as `?`.
    pub fn text(&self, x: i16, y: i16, text: &str, rgb565: u16) {
        (self.draw_text)(x, y, text.as_ptr(), text.len(), rgb565)
    }

    pub fn show(&self) {
        (self.present)()
    }

    pub fn event(&self, timeout_ms: u32) -> Option<Event> {
        let mut event = Event::default();
        (self.next_event)(&mut event, timeout_ms).then_some(event)
    }

    /// `duty_permille` 0..=1000 (0 = off), `freq_hz` 1..=20000. Returns 0, or:
    /// `-1` out of range; `-3` paused under a stock popup (between `PAUSE` and `RESUME`);
    /// `-4` locked: the loader cut a run longer than 2000 ms, and the motor stays off until `vibrate(0, _)`;
    /// another negative value is a PWM driver error.
    pub fn vibrate(&self, duty_permille: u16, freq_hz: u16) -> i32 {
        (self.motor)(duty_permille, freq_hz)
    }

    pub fn now(&self) -> u32 {
        (self.millis)()
    }

    /// Queues a message (1..=[`MSG_MAX`] bytes) for the host. `0`, or `-1` bad length, `-11` outbox full
    /// (4 queued), `-107` no wp2apps host connected.
    pub fn send(&self, data: &[u8]) -> i32 {
        (self.send_msg)(data.as_ptr(), data.len())
    }

    /// Takes the oldest host message into `buf`. `None` if the inbox is empty or `buf` is shorter than the message
    /// (then it stays queued; [`MSG_MAX`] always fits).
    pub fn recv<'a>(&self, buf: &'a mut [u8]) -> Option<&'a [u8]> {
        let n = (self.recv_msg)(buf.as_mut_ptr(), buf.len());
        (n > 0).then(|| &buf[..n as usize])
    }

    /// The app's stored blob: its length (`0` = nothing stored), `-1` if `buf` is too short, `-5` I/O error.
    pub fn load(&self, buf: &mut [u8]) -> i32 {
        (self.load_data)(buf.as_mut_ptr(), buf.len())
    }

    /// Replaces the stored blob atomically (at most [`STORE_MAX`] bytes; empty deletes it). Writes NAND: call on
    /// change, not per frame. `0`, `-1` too long, `-5` I/O error.
    pub fn save(&self, data: &[u8]) -> i32 {
        (self.save_data)(data.as_ptr(), data.len())
    }

    pub fn time(&self) -> Option<Time> {
        let mut t = Time::default();
        ((self.get_time)(&mut t) == 0).then_some(t)
    }

    pub fn battery(&self) -> Option<Battery> {
        let b = (self.get_battery)();
        (b >= 0).then(|| Battery {
            percent: b as u8,
            charging: b & 0x100 != 0,
            full: b & 0x200 != 0,
            charger: b & 0x400 != 0,
        })
    }

    /// The LVGL command channel, `None` on a loader older than 1.2 (the table has no `lv_submit` then).
    pub fn lv(&self) -> Option<lv::Lv> {
        (self.abi_minor >= 2).then(|| lv::Lv(self.lv_submit))
    }

    /// Keeps the display on while this app runs in the foreground; the loader drops it on pause and exit.
    pub fn keep_screen_on(&self, on: bool) {
        (self.screen_on)(on);
    }
}

static API: AtomicPtr<Api> = AtomicPtr::new(core::ptr::null_mut());

pub fn api() -> Option<&'static Api> {
    unsafe { API.load(Ordering::Relaxed).as_ref() }
}

#[doc(hidden)]
pub fn __start(api: &'static Api, main: fn(&'static Api) -> i32) -> i32 {
    // the volatile read keeps WP2_ABI_MINOR in the linked image for the packager
    if api.abi != ABI || api.abi_minor < unsafe { core::ptr::read_volatile(&WP2_ABI_MINOR) } {
        return -2;
    }
    API.store(api as *const Api as *mut Api, Ordering::Relaxed);
    main(api)
}

#[cfg(not(wp2_sim))]
#[macro_export]
macro_rules! entry {
    ($main:path) => {
        #[unsafe(no_mangle)]
        #[unsafe(link_section = ".entry")]
        pub extern "C" fn _start(api: &'static $crate::Api) -> i32 {
            $crate::__start(api, $main)
        }
    };
}

#[cfg(wp2_sim)]
#[macro_export]
macro_rules! entry {
    ($main:path) => {
        extern "C" fn _start(api: &'static $crate::Api) -> i32 {
            $crate::__start(api, $main)
        }
        #[unsafe(export_name = "main")]
        extern "C" fn __wp2_sim_main() -> i32 {
            $crate::__sim_run(_start)
        }
    };
}

// `scripts/sim` builds apps for the host with `--cfg wp2_sim` (docs/app-guide.md). Keep it below the code with
// panics: their line numbers are in the watch build.
#[cfg(wp2_sim)]
extern crate std;

/// Runs the app in the host simulator (`sim/`, linked by `scripts/sim`).
#[cfg(wp2_sim)]
#[doc(hidden)]
pub fn __sim_run(start: extern "C" fn(&'static Api) -> i32) -> i32 {
    #[link(name = "wp2_sim")]
    unsafe extern "C" {
        fn wp2_sim_run(start: extern "C" fn(&'static Api) -> i32) -> i32;
    }
    // like the panic handler below: motor off, exit(-1)
    std::panic::set_hook(std::boxed::Box::new(|info| {
        std::eprintln!("{info}");
        if let Some(api) = api() {
            api.vibrate(0, 0);
            (api.exit)(-1)
        }
    }));
    unsafe { wp2_sim_run(start) }
}

/// The app's menu icon, in front of the code (docs/app-icons.md). `scripts/build-app` renders the crate's
/// `icon.svg` and passes the descriptor in `WP2_ICON`.
#[cfg(not(wp2_sim))]
#[macro_export]
macro_rules! app_icon {
    () => {
        #[used]
        #[unsafe(link_section = ".wp2info")]
        static WP2_ICON: [u8; include_bytes!(env!("WP2_ICON")).len()] = *include_bytes!(env!("WP2_ICON"));
    };
}

#[cfg(wp2_sim)]
#[macro_export]
macro_rules! app_icon {
    () => {};
}

#[cfg(target_os = "none")]
#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    if let Some(api) = api() {
        api.vibrate(0, 0);
        (api.exit)(-1)
    }
    loop {}
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use std::boxed::Box;

    extern "C" fn rect(_: i16, _: i16, _: u16, _: u16, _: u16) {}
    extern "C" fn text(_: i16, _: i16, _: *const u8, _: usize, _: u16) {}
    extern "C" fn present() {}
    extern "C" fn next(_: *mut Event, _: u32) -> bool {
        false
    }
    extern "C" fn motor(_: u16, _: u16) -> i32 {
        0
    }
    extern "C" fn millis() -> u32 {
        0
    }
    extern "C" fn exit(_: i32) -> ! {
        panic!("exit")
    }
    extern "C" fn bytes(_: *const u8, _: usize) -> i32 {
        0
    }
    extern "C" fn bytes_mut(_: *mut u8, _: usize) -> i32 {
        0
    }
    extern "C" fn time(_: *mut Time) -> i32 {
        0
    }
    extern "C" fn battery() -> i32 {
        0x1_40
    }
    extern "C" fn screen(_: bool) -> i32 {
        0
    }

    pub(crate) fn table(abi: u16, abi_minor: u16) -> &'static Api {
        Box::leak(Box::new(Api {
            abi, abi_minor, width: 466, height: 466, fill_rect: rect, draw_text: text, present,
            next_event: next, motor, millis, exit, send_msg: bytes, recv_msg: bytes_mut, load_data: bytes_mut,
            save_data: bytes, get_time: time, get_battery: battery, screen_on: screen, lv_submit: bytes,
        }))
    }

    fn app(_: &'static Api) -> i32 {
        7
    }

    #[test]
    fn runs_on_same_major_and_newer_minor() {
        assert_eq!(__start(table(ABI, ABI_MINOR + 1), app), 7);
    }

    #[test]
    fn rejects_other_major() {
        assert_eq!(__start(table(ABI + 1, ABI_MINOR), app), -2);
    }

    #[test]
    fn rejects_older_minor() {
        assert_eq!(__start(table(ABI, ABI_MINOR - 1), app), -2);
    }

    #[test]
    fn battery_bits() {
        let b = table(ABI, ABI_MINOR).battery().unwrap();
        assert_eq!((b.percent, b.charging, b.full, b.charger), (0x40, true, false, false));
    }
}
