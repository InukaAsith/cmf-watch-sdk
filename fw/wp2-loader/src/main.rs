#![cfg_attr(target_os = "none", no_std, no_main)]

#[cfg(target_os = "none")]
macro_rules! stock {
    ($addr:expr, fn($($t:ty),*) $(-> $r:ty)?) => {
        unsafe { core::mem::transmute::<usize, extern "C" fn($($t),*) $(-> $r)?>($addr as usize) }
    };
}

#[cfg(target_os = "none")]
mod app;
mod menu;
#[cfg(target_os = "none")]
mod motor;
#[cfg(target_os = "none")]
mod ui;

#[cfg(target_os = "none")]
mod view {
    use crate::{app, menu, motor, ui};
    use core::ffi::c_void;
    use core::ptr::addr_of_mut;
    use core::sync::atomic::Ordering;
    use wp2_core::events::{map_key, Event, BUTTON, CROWN, TOUCH_DOWN, TOUCH_UP};

    const PRELOAD: u32 = 2;
    const LAYOUT: u32 = 3;
    const DELETE: u32 = 4;
    const KEY: u32 = 0xD;
    const ERROR_MS: u32 = 2000;

    struct Menu {
        entries: [Option<menu::Entry>; menu::ROWS],
        count: usize,
        selected: usize,
        armed: Option<usize>,
        failed: Option<(usize, i32)>,
        failed_at: u32,
    }

    // Lives in the loader block: zero again on every open of the view.
    static mut MENU: Menu = Menu { entries: [None; menu::ROWS], count: 0, selected: 0, armed: None, failed: None, failed_at: 0 };

    fn st() -> &'static mut Menu {
        unsafe { &mut *addr_of_mut!(MENU) }
    }

    #[unsafe(no_mangle)]
    #[unsafe(link_section = ".entry")]
    pub extern "C" fn loader_main(view_id: u32, msg: u32, data: *mut c_void) -> i32 {
        match msg {
            PRELOAD => ui::preload(view_id),
            LAYOUT => {
                let rc = ui::layout(data, input_cb, tick);
                let m = st();
                m.entries = menu::scan();
                m.count = m.entries.iter().flatten().count();
                redraw();
                rc
            }
            KEY => {
                key(data);
                0
            }
            DELETE => {
                if app::running() {
                    app::QUEUE.push(Event { kind: BUTTON, ..Event::default() });
                    app::stop_app();
                }
                ui::delete();
                0
            }
            _ => 0,
        }
    }

    fn redraw() {
        let m = st();
        menu::draw(&mut ui::canvas(), &m.entries, m.selected, m.failed);
        ui::DIRTY.store(true, Ordering::Release);
    }

    // data: u32 key code, byte +4 = handled
    fn key(data: *mut c_void) {
        let Some(e) = map_key(unsafe { *(data as *const u32) }) else { return };
        if !app::running() && e.kind != CROWN {
            return;
        }
        crown(e);
        unsafe { *(data as *mut u8).add(4) = 1 };
    }

    // crown or button: to the app, else the crown moves the menu selection
    fn crown(e: Event) {
        if app::running() {
            app::QUEUE.push(e);
            return;
        }
        let m = st();
        if e.kind == CROWN && m.count > 0 {
            m.selected = m.selected.saturating_add_signed(e.delta.into()).min(m.count - 1);
            redraw();
        }
    }

    extern "C" fn input_cb(e: *mut c_void) {
        let code = ui::event_code(e);
        if code == ui::KNOB {
            let step = ui::knob_step(e).signum() as i16;
            if step != 0 {
                crown(Event { kind: CROWN, delta: step, ..Event::default() });
            }
            return;
        }
        if code != ui::PRESSED && code != ui::RELEASED {
            return;
        }
        let (x, y) = ui::touch_point(e);
        if app::running() {
            let kind = if code == ui::PRESSED { TOUCH_DOWN } else { TOUCH_UP };
            let c = |v: i32| v.clamp(i16::MIN.into(), i16::MAX.into()) as i16;
            app::QUEUE.push(Event { kind, x: c(x), y: c(y), ..Event::default() });
            return;
        }
        let m = st();
        if code == ui::PRESSED {
            m.armed = i16::try_from(y).ok().and_then(menu::hit).filter(|&r| r < m.count);
            if let Some(row) = m.armed {
                m.selected = row;
                redraw();
            }
            return;
        }
        // the release point of a short tap is off (e2e: pressed at y 140, released at y 103), so the pressed row runs
        let Some(row) = m.armed.take() else { return };
        let Some(entry) = m.entries[row] else { return };
        if let Err(code) = app::launch(entry.slot) {
            m.failed = Some((row, code));
            m.failed_at = app::millis();
            redraw();
        }
    }

    extern "C" fn tick(_timer: *mut c_void) {
        let m = st();
        motor::watchdog();
        if app::running() && app::exited() {
            app::stop_app();
            redraw();
        }
        if m.failed.is_some() && app::millis().wrapping_sub(m.failed_at) >= ERROR_MS {
            m.failed = None;
            redraw();
        }
        if ui::DIRTY.swap(false, Ordering::AcqRel) {
            ui::invalidate();
        }
    }

    #[panic_handler]
    fn panic(_: &core::panic::PanicInfo) -> ! {
        loop {}
    }
}
