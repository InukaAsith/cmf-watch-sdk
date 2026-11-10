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
        count: usize,
        selected: usize,
        // (row, x, y) of the press
        armed: Option<(usize, i32, i32)>,
        failed: Option<(usize, i32)>,
        failed_at: u32,
        // millis() | 1 of a BUTTON for the running app, 0 if none
        button_at: u32,
    }

    // Lives in the loader block: zero again on every open of the view.
    static mut MENU: Menu = Menu { entries: [None; menu::ROWS], count: 0, selected: 0, armed: None, failed: None, failed_at: 0, button_at: 0 };

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
        let m = st();
        if e.kind == BUTTON && !app::paused() && m.button_at == 0 {
            m.button_at = app::millis() | 1;
        }
        crown(e);
        unsafe { *(data as *mut u8).add(4) = 1 };
    }

    // crown or button: to the app (dropped while paused), else the crown moves the menu selection
    fn crown(e: Event) {
        if app::running() {
            if !app::paused() {
                app::QUEUE.push(e);
            }
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
            if app::paused() {
                return;
            }
            let kind = if code == ui::PRESSED { TOUCH_DOWN } else { TOUCH_UP };
            let c = |v: i32| v.clamp(i16::MIN.into(), i16::MAX.into()) as i16;
            app::QUEUE.push(Event { kind, x: c(x), y: c(y), ..Event::default() });
            return;
        }
        let m = st();
        if code == ui::PRESSED {
            let row = i16::try_from(y).ok().and_then(menu::hit).map(|r| menu::first(m.selected) + r);
            m.armed = row.filter(|&r| r < m.count).map(|r| (r, x, y));
            if let Some((row, ..)) = m.armed {
                m.selected = row;
                redraw();
            }
            return;
        }
        // the pressed row runs: the release point of a short tap is off (e2e: pressed at y 140, released at y 103);
        // no launch after a drag (LVGL 8.3 sends SHORT_CLICKED after a drag too when nothing scrolls)
        let Some((row, px, py)) = m.armed.take() else { return };
        if !menu::is_tap((px, py), (x, y)) {
            return;
        }
        let Some(entry) = m.entries[row] else { return };
        if let Err(code) = app::launch(entry.slot) {
            m.failed = Some((row, code));
            m.failed_at = app::millis();
            redraw();
        }
    }

    extern "C" fn tick(_timer: *mut c_void) {
        let m = st();
        app::pause_tick();
        motor::watchdog();
        if app::running() {
            if let Some(code) = app::exited() {
                app::stop_app();
                if code != 0 {
                    m.failed = Some((m.selected, code));
                    m.failed_at = app::millis();
                }
                redraw();
            } else if m.button_at != 0 && app::millis().wrapping_sub(m.button_at) >= ESCAPE_MS {
                app::stop_app();
                redraw();
            }
        }
        if !app::running() {
            m.button_at = 0;
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
        // a fault reboots the watch: a hung ui_service would also stop the motor watchdog
        unsafe { core::arch::asm!("udf #0", options(noreturn)) }
    }
}
