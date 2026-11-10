#![no_std]

use core::sync::atomic::{AtomicPtr, Ordering};

pub const ABI: u16 = 1;
pub const ABI_MINOR: u16 = 0;

pub const TOUCH_DOWN: u8 = 1;
pub const TOUCH_UP: u8 = 2;
pub const CROWN: u8 = 3;
pub const BUTTON: u8 = 4;
pub const PAUSE: u8 = 5;
pub const RESUME: u8 = 6;

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
}

impl Api {
    pub fn rect(&self, x: i16, y: i16, w: u16, h: u16, rgb565: u16) {
        (self.fill_rect)(x, y, w, h, rgb565)
    }

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
}

static API: AtomicPtr<Api> = AtomicPtr::new(core::ptr::null_mut());

pub fn api() -> Option<&'static Api> {
    unsafe { API.load(Ordering::Relaxed).as_ref() }
}

#[doc(hidden)]
pub fn __start(api: &'static Api, main: fn(&'static Api) -> i32) -> i32 {
    if api.abi != ABI || api.abi_minor < ABI_MINOR {
        return -2;
    }
    API.store(api as *const Api as *mut Api, Ordering::Relaxed);
    main(api)
}

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

    fn table(abi: u16, abi_minor: u16) -> &'static Api {
        Box::leak(Box::new(Api {
            abi, abi_minor, width: 466, height: 466, fill_rect: rect, draw_text: text, present,
            next_event: next, motor, millis, exit,
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
}
