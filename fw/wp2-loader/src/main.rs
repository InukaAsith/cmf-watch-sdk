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
    const FOCUS: u32 = 7;
    const DEFOCUS: u32 = 8;
    const KEY: u32 = 0xD;
    const ERROR_MS: u32 = 2000;
    // an app still running this long after a BUTTON is stopped
    const ESCAPE_MS: u32 = 1000;

    struct Menu {
        entries: [Option<menu::Entry>; menu::ROWS],
        // row of the running (or last) app
        launched: usize,
        failed: Option<usize>,
        failed_at: u32,
        // millis() | 1 of a BUTTON for the running app, 0 if none
        button_at: u32,
    }

    // Lives in the loader block: zero again on every open of the view.
    static mut MENU: Menu = Menu { entries: [None; menu::ROWS], launched: 0, failed: None, failed_at: 0, button_at: 0 };

    fn st() -> &'static mut Menu {
        unsafe { &mut *addr_of_mut!(MENU) }
    }

    #[unsafe(no_mangle)]
    #[unsafe(link_section = ".entry")]
    pub extern "C" fn loader_main(view_id: u32, msg: u32, data: *mut c_void) -> i32 {
        match msg {
            PRELOAD => ui::preload(view_id),
            LAYOUT => {
                let scr = ui::layout(data, input_cb, tick);
                let m = st();
                m.entries = menu::scan();
                menu::lv::build(scr, &m.entries, row_cb);
                0
            }
            FOCUS | DEFOCUS => {
                app::DEFOCUSED.store(msg == DEFOCUS, Ordering::Release);
                0
            }
            KEY => {
                key(data);
                0
            }
            DELETE => {
                app::stop_app();
                menu::lv::delete();
                ui::delete();
                0
            }
            _ => 0,
        }
    }

    fn show_app(on: bool) {
        ui::show(on);
        menu::lv::show(!on);
    }

    fn fail(row: usize, code: i32) {
        let m = st();
        if let Some(prev) = m.failed.replace(row) {
            restore(prev);
        }
        m.failed_at = app::millis();
        menu::lv::set_row_text(row, &menu::error_text(code));
    }

    fn restore(row: usize) {
        if let Some(e) = st().entries[row] {
            menu::lv::set_row_text(row, &menu::name_text(&e));
        }
    }

    // data: u32 key code, byte +4 = handled. In the menu the crown scrolls through LVGL and the button
    // stays with the stock (it closes the view).
    fn key(data: *mut c_void) {
        let Some(e) = map_key(unsafe { *(data as *const u32) }) else { return };
        if !app::running() {
            return;
        }
        let m = st();
        if e.kind == BUTTON && !app::paused() && m.button_at == 0 {
            m.button_at = app::millis() | 1;
        }
        to_app(e);
        unsafe { *(data as *mut u8).add(4) = 1 };
    }

    // dropped while paused
    fn to_app(e: Event) {
        if app::running() && !app::paused() {
            app::QUEUE.push(e);
        }
    }

    // the image: only shown while an app runs
    extern "C" fn input_cb(e: *mut c_void) {
        let code = ui::event_code(e);
        if code == ui::KNOB {
            let step = ui::knob_step(e).signum() as i16;
            if step != 0 {
                to_app(Event { kind: CROWN, delta: step, ..Event::default() });
            }
            return;
        }
        if code != ui::PRESSED && code != ui::RELEASED {
            return;
        }
        let (x, y) = ui::touch_point(e);
        let kind = if code == ui::PRESSED { TOUCH_DOWN } else { TOUCH_UP };
        let c = |v: i32| v.clamp(i16::MIN.into(), i16::MAX.into()) as i16;
        to_app(Event { kind, x: c(x), y: c(y), ..Event::default() });
    }

    // CLICKED on a menu row (LVGL sends none after a scroll drag)
    extern "C" fn row_cb(e: *mut c_void) {
        let row = menu::lv::row_of(e);
        let m = st();
        if app::running() {
            return;
        }
        let Some(entry) = m.entries.get(row).copied().flatten() else { return };
        m.launched = row;
        match app::launch(entry.slot) {
            Ok(()) => show_app(true),
            Err(code) => fail(row, code),
        }
    }

    extern "C" fn tick(_timer: *mut c_void) {
        let m = st();
        app::pause_tick();
        motor::watchdog();
        if app::running() {
            let exited = app::exited();
            let escape = m.button_at != 0 && app::millis().wrapping_sub(m.button_at) >= ESCAPE_MS;
            if exited.is_some() || escape {
                app::stop_app();
                show_app(false);
                if let Some(code) = exited.filter(|&c| c != 0) {
                    fail(m.launched, code);
                }
            }
        }
        if !app::running() {
            m.button_at = 0;
        }
        if let Some(row) = m.failed.filter(|_| app::millis().wrapping_sub(m.failed_at) >= ERROR_MS) {
            m.failed = None;
            restore(row);
        }
        ui::flush(ui::PUBLISHED.swap(wp2_core::gfx::EMPTY, Ordering::Acquire));
    }

    #[panic_handler]
    fn panic(_: &core::panic::PanicInfo) -> ! {
        // a fault reboots the watch: a hung ui_service would also stop the motor watchdog
        unsafe { core::arch::asm!("udf #0", options(noreturn)) }
    }
}
