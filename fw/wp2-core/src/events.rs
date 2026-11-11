use core::{
    cell::UnsafeCell,
    sync::atomic::{AtomicU8, Ordering},
};

use crate::stock::{KEY_BACK, KEY_CROWN_CCW, KEY_CROWN_CW, KEY_CROWN_PRESS};

pub const TOUCH_DOWN: u8 = 1;
pub const TOUCH_UP: u8 = 2;
pub const CROWN: u8 = 3;
pub const BUTTON: u8 = 4;
pub const PAUSE: u8 = 5;
pub const RESUME: u8 = 6;
// ABI 1.1: a host message is in the inbox, x = its length
pub const MESSAGE: u8 = 7;

#[repr(C)]
#[derive(Clone, Copy, Default, PartialEq, Debug)]
pub struct Event {
    pub kind: u8,
    pub _pad: u8,
    pub x: i16,
    pub y: i16,
    pub delta: i16,
}

const LEN: u8 = 16;

pub struct Queue {
    buf: UnsafeCell<[Event; LEN as usize]>,
    head: AtomicU8,
    tail: AtomicU8,
}

// one producer (push, clear) and one consumer (pop)
unsafe impl Sync for Queue {}

impl Queue {
    pub const fn new() -> Self {
        const EMPTY: Event = Event { kind: 0, _pad: 0, x: 0, y: 0, delta: 0 };
        Queue { buf: UnsafeCell::new([EMPTY; LEN as usize]), head: AtomicU8::new(0), tail: AtomicU8::new(0) }
    }

    pub fn push(&self, e: Event) -> bool {
        let tail = self.tail.load(Ordering::Relaxed);
        let next = (tail + 1) % LEN;
        if next == self.head.load(Ordering::Acquire) {
            return false;
        }
        unsafe { (self.buf.get() as *mut Event).add(usize::from(tail)).write(e) };
        self.tail.store(next, Ordering::Release);
        true
    }

    pub fn pop(&self) -> Option<Event> {
        loop {
            let head = self.head.load(Ordering::Acquire);
            if head == self.tail.load(Ordering::Acquire) {
                return None;
            }
            let e = unsafe { (self.buf.get() as *const Event).add(usize::from(head)).read() };
            // a clear() that preempted us moved head: drop e and read again
            if self.head.compare_exchange(head, (head + 1) % LEN, Ordering::AcqRel, Ordering::Acquire).is_ok() {
                return Some(e);
            }
        }
    }

    pub fn clear(&self) {
        self.head.store(self.tail.load(Ordering::Acquire), Ordering::Release);
    }
}

pub fn map_key(value: u32) -> Option<Event> {
    let (kind, delta) = match value {
        KEY_CROWN_CW => (CROWN, 1),
        KEY_CROWN_CCW => (CROWN, -1),
        KEY_CROWN_PRESS => (BUTTON, 0),
        // the app leaves as on the button and the loader shows its menu, not the stock app list
        v if v & 0xFFFF == KEY_BACK => (BUTTON, 0),
        _ => return None,
    };
    Some(Event { kind, delta, ..Event::default() })
}

#[repr(C)]
pub struct Api {
    pub abi: u16,
    pub abi_minor: u16,
    pub width: u16,
    pub height: u16,
    pub fill_rect: extern "C" fn(i16, i16, u16, u16, u16),
    pub draw_text: extern "C" fn(i16, i16, *const u8, usize, u16),
    pub present: extern "C" fn(),
    pub next_event: extern "C" fn(*mut Event, u32) -> bool,
    pub motor: extern "C" fn(u16, u16) -> i32,
    pub millis: extern "C" fn() -> u32,
    pub exit: extern "C" fn(i32) -> !,
    // ABI 1.1 (docs/abi-1.1.md)
    pub send_msg: extern "C" fn(*const u8, usize) -> i32,
    pub recv_msg: extern "C" fn(*mut u8, usize) -> i32,
    pub load_data: extern "C" fn(*mut u8, usize) -> i32,
    pub save_data: extern "C" fn(*const u8, usize) -> i32,
    pub get_time: extern "C" fn(*mut Time) -> i32,
    pub get_battery: extern "C" fn() -> i32,
    pub screen_on: extern "C" fn(bool) -> i32,
}

#[repr(C)]
#[derive(Clone, Copy, Default, PartialEq, Debug)]
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

#[cfg(test)]
mod tests {
    use super::*;
    use core::mem::{offset_of, size_of};

    #[test]
    fn queue_spsc() {
        let q = Queue::new();
        assert_eq!(q.pop(), None);
        for i in 0..15 {
            assert!(q.push(Event { kind: 1, x: i, ..Event::default() }));
        }
        assert!(!q.push(Event::default()));
        for i in 0..15 {
            assert_eq!(q.pop().map(|e| e.x), Some(i));
        }
        assert_eq!(q.pop(), None);
        for _ in 0..40 {
            assert!(q.push(Event::default()));
            assert!(q.pop().is_some());
        }
        q.push(Event::default());
        q.clear();
        assert_eq!(q.pop(), None);
    }

    #[test]
    fn key_mapping() {
        assert_eq!(map_key(0xFFFF_006F).map(|e| (e.kind, e.delta)), Some((3, 1)));
        assert_eq!(map_key(0xFFFF_006E).map(|e| (e.kind, e.delta)), Some((3, -1)));
        assert_eq!(map_key(0x0400_0001).map(|e| e.kind), Some(4));
        assert_eq!(map_key(0xFFFF_0071).map(|e| e.kind), Some(4));
        assert_eq!(map_key(0x2000_0001), None);
    }

    #[test]
    fn api_layout_matches_sdk() {
        assert_eq!(size_of::<Api>(), size_of::<cmf_wp2_app::Api>());
        assert_eq!(offset_of!(Api, motor), offset_of!(cmf_wp2_app::Api, motor));
        assert_eq!(offset_of!(Api, exit), offset_of!(cmf_wp2_app::Api, exit));
        assert_eq!(offset_of!(Api, send_msg), offset_of!(cmf_wp2_app::Api, send_msg));
        assert_eq!(offset_of!(Api, screen_on), offset_of!(cmf_wp2_app::Api, screen_on));
        assert_eq!(size_of::<Time>(), size_of::<cmf_wp2_app::Time>());
        assert_eq!(offset_of!(Time, utc_offset_min), offset_of!(cmf_wp2_app::Time, utc_offset_min));
        assert_eq!(size_of::<Event>(), size_of::<cmf_wp2_app::Event>());
    }
}
