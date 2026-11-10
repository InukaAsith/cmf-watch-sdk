#![cfg_attr(target_os = "none", no_std, no_main)]

mod menu;
#[cfg(target_os = "none")]
mod ui;

#[cfg(target_os = "none")]
mod view {
    use crate::{menu, ui};
    use core::ffi::c_void;
    use core::ptr::addr_of_mut;
    use wp2_core::events::{map_key, CROWN};

    const PRELOAD: u32 = 2;
    const LAYOUT: u32 = 3;
    const DELETE: u32 = 4;
    const KEY: u32 = 0xD;

    struct Menu {
        entries: [Option<menu::Entry>; menu::ROWS],
        count: usize,
        selected: usize,
        chosen: Option<usize>,
        dirty: bool,
    }

    // Lives in the loader block: zero again on every open of the view.
    static mut MENU: Menu = Menu { entries: [None; menu::ROWS], count: 0, selected: 0, chosen: None, dirty: false };

    fn st() -> &'static mut Menu {
        unsafe { &mut *addr_of_mut!(MENU) }
    }

    #[unsafe(no_mangle)]
    #[unsafe(link_section = ".entry")]
    pub extern "C" fn loader_main(view_id: u32, msg: u32, data: *mut c_void) -> i32 {
        match msg {
            PRELOAD => ui::preload(view_id),
            LAYOUT => {
                let rc = ui::layout(data, touch_cb, tick);
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
                ui::delete();
                0
            }
            _ => 0,
        }
    }

    fn redraw() {
        let m = st();
        menu::draw(&mut ui::canvas(), &m.entries, m.selected);
        m.dirty = true;
    }

    // data: u32 key code, byte +4 = handled
    fn key(data: *mut c_void) {
        let m = st();
        let Some(e) = map_key(unsafe { *(data as *const u32) }).filter(|e| e.kind == CROWN) else {
            return;
        };
        if m.count > 0 {
            m.selected = m.selected.saturating_add_signed(e.delta.into()).min(m.count - 1);
            redraw();
        }
        unsafe { *(data as *mut u8).add(4) = 1 };
    }

    extern "C" fn touch_cb(e: *mut c_void) {
        let code = ui::event_code(e);
        if code != ui::PRESSED && code != ui::RELEASED {
            return;
        }
        let m = st();
        let (_, y) = ui::touch_point(e);
        let Some(row) = i16::try_from(y).ok().and_then(menu::hit).filter(|&r| r < m.count) else {
            return;
        };
        if code == ui::PRESSED {
            m.selected = row;
            redraw();
        } else if row == m.selected {
            m.chosen = Some(row);
        }
    }

    extern "C" fn tick(_timer: *mut c_void) {
        let m = st();
        if m.dirty {
            m.dirty = false;
            ui::invalidate();
        }
    }

    #[panic_handler]
    fn panic(_: &core::panic::PanicInfo) -> ! {
        loop {}
    }
}
