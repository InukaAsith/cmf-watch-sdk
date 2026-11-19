#![cfg_attr(any(target_os = "none", wp2_sim), no_main, cfg_attr(target_os = "none", no_std))]

mod rle;

#[cfg(not(any(target_os = "none", wp2_sim)))]
fn main() {}

/// Shows images the phone streams (maps, photos). Crown: `Z±1` to the phone (zoom). Tap: `T x y` (centre there).
/// Button: exit. App → phone: `R` ready, `K rects ms` image shown, `G seq` resend from the start.
///
/// The last image stays in `STORE`, so a crown turn or a tap answers at once: the stored image is drawn again zoomed
/// or shifted, with a dot at the top while the phone's new image streams in over it.
#[cfg(any(target_os = "none", wp2_sim))]
mod device {
    use crate::rle::{Decoder, HEADER, Status, Surface};
    use cmf_wp2_app::{Api, BUTTON, CROWN, MESSAGE, MSG_MAX, PAUSE, RESUME, TOUCH_UP};
    use core::fmt::Write;

    cmf_wp2_app::entry!(main);
    cmf_wp2_app::app_icon!();

    const STORE_CAP: usize = 40 * 1024;
    const C: i32 = 233;

    /// The last image: its `I` header and token stream (None if it did not fit).
    struct Store {
        header: [u8; HEADER],
        data: [u8; STORE_CAP],
        len: usize,
        ok: bool,
    }
    static mut STORE: Store = Store { header: [0; HEADER], data: [0; STORE_CAP], len: 0, ok: false };

    fn store() -> &'static mut Store {
        // one app thread, no reentrancy
        unsafe { &mut *core::ptr::addr_of_mut!(STORE) }
    }

    struct Screen(&'static Api);
    impl Surface for Screen {
        fn rect(&mut self, x: i32, y: i32, w: i32, h: i32, c: u16) {
            if w > 0 && h > 0 {
                self.0.rect(x as i16, y as i16, w as u16, h as u16, c);
            }
        }
    }

    struct Buf<const N: usize>([u8; N], usize);
    impl<const N: usize> Write for Buf<N> {
        fn write_str(&mut self, s: &str) -> core::fmt::Result {
            let b = s.as_bytes();
            let n = b.len().min(N - self.1);
            self.0[self.1..self.1 + n].copy_from_slice(&b[..n]);
            self.1 += n;
            Ok(())
        }
    }

    fn reply(api: &Api, args: core::fmt::Arguments) {
        let mut b = Buf([0u8; 32], 0);
        let _ = b.write_fmt(args);
        let _ = api.send(&b.0[..b.1]);
    }

    fn waiting(api: &Api) {
        api.rect(0, 0, api.width, api.height, 0x0000);
        api.text(233 - 3 * 16, 217, "Photos", 0xFFFF);
        api.text(233 - 8 * 16 - 8, 249, "waiting for phone", 0x8410);
        api.show();
    }

    /// The stored image again at `x0, y0, scale`, black around it, and the loading dot.
    fn redraw(api: &'static Api, x0: i32, y0: i32, scale: u8) -> bool {
        let st = store();
        if !st.ok {
            return false;
        }
        let mut d = Decoder::new();
        if !d.start(&st.header, x0, y0, scale) {
            return false;
        }
        api.rect(0, 0, api.width, api.height, 0x0000);
        d.feed(&st.data[..st.len], &mut Screen(api));
        loading(api);
        api.show();
        true
    }

    fn loading(api: &Api) {
        api.rect(C as i16 - 7, 24, 14, 14, 0xFFFF);
        api.rect(C as i16 - 5, 26, 10, 10, 0x07E0);
    }

    fn main(api: &'static Api) -> i32 {
        let mut d = Decoder::new();
        let mut msg = [0u8; MSG_MAX];
        let mut began = 0;
        let mut screen = Screen(api);
        // where the stored image sits now (moved by local zooms and pans until the next image comes)
        let mut view: Option<(i32, i32, u8)> = None;
        waiting(api);
        let _ = api.send(b"R");
        loop {
            let Some(e) = api.event(u32::MAX) else { continue };
            match e.kind {
                BUTTON => return 0,
                PAUSE => loop {
                    match api.event(u32::MAX) {
                        Some(e) if e.kind == RESUME => break,
                        Some(e) if e.kind == BUTTON => return 0,
                        _ => {}
                    }
                },
                CROWN => {
                    reply(api, format_args!("Z{}", if e.delta > 0 { "+1" } else { "-1" }));
                    if let Some((x0, y0, s)) = view {
                        // zoom about the centre: in doubles the scale, out halves it (down to 1)
                        let next = if e.delta > 0 { (s * 2).min(16) } else { (s / 2).max(1) };
                        if next != s {
                            let (x, y) = if next > s {
                                (C - (C - x0) * 2, C - (C - y0) * 2)
                            } else {
                                (C - (C - x0) / 2, C - (C - y0) / 2)
                            };
                            if redraw(api, x, y, next) {
                                view = Some((x, y, next));
                            }
                        }
                    }
                }
                TOUCH_UP => {
                    reply(api, format_args!("T {} {}", e.x, e.y));
                    if let Some((x0, y0, s)) = view {
                        let (x, y) = (x0 + C - e.x as i32, y0 + C - e.y as i32);
                        if redraw(api, x, y, s) {
                            view = Some((x, y, s));
                        }
                    }
                }
                MESSAGE => {
                    let mut drew = false;
                    while let Some(m) = api.recv(&mut msg) {
                        let st = store();
                        match m.first() {
                            Some(b'I') if m.len() == HEADER => {
                                st.header.copy_from_slice(m);
                                st.len = 0;
                                st.ok = false;
                            }
                            Some(b'D') if m.len() >= 2 => {
                                let t = &m[2..];
                                if st.len + t.len() <= STORE_CAP {
                                    st.data[st.len..st.len + t.len()].copy_from_slice(t);
                                }
                                st.len += t.len();
                            }
                            _ => {}
                        }
                        match d.message(m, &mut screen) {
                            Status::Begin => {
                                began = api.now();
                                view = None;
                            }
                            Status::Done(n) => {
                                st.ok = st.len <= STORE_CAP;
                                view = Some(d.origin());
                                reply(api, format_args!("K {} {}", n, api.now().wrapping_sub(began)));
                            }
                            Status::Gap(s) => reply(api, format_args!("G {}", s)),
                            Status::Bad if m.first() == Some(&b'?') => {
                                let _ = api.send(b"R");
                            }
                            _ => {}
                        }
                        drew = true;
                    }
                    if drew {
                        api.show();
                    }
                }
                _ => {}
            }
        }
    }
}
