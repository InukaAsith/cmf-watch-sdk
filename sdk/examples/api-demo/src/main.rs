// Exercises every ABI 1.1 call. Host messages starting with '!' are commands, the rest is echoed back:
// !time, !bat, !count, !inc (counter + 1, saved), !keep 0|1, !spam N (N messages back), !save N (N-byte blob).
// With LVGL (loader 1.2) every message goes into a crown-scrolled log (src/log.rs); without it (simulator, older
// loaders) the framebuffer screen shows the last one.
#![cfg_attr(any(target_os = "none", wp2_sim), no_main, cfg_attr(target_os = "none", no_std))]

#[cfg_attr(not(target_os = "none"), allow(dead_code))]
mod log;

#[cfg(not(any(target_os = "none", wp2_sim)))]
fn main() {}

#[cfg(any(target_os = "none", wp2_sim))]
mod device {
    use crate::log::{self, Log};
    use cmf_wp2_app::lv::{Batch, Lv};
    use cmf_wp2_app::{stock, Api, BUTTON, MESSAGE, MSG_MAX, RESUME, STORE_MAX, TOUCH_UP};
    use core::fmt::Write;

    cmf_wp2_app::entry!(main);
    cmf_wp2_app::app_icon!();

    const BG: u16 = 0x0000;
    const FG: u16 = 0xFFFF;
    const DIM: u16 = 0x8410;
    const ACCENT: u16 = 0x07FF;

    // e2e (tests/test_api_e2e.py): received, echoed, send errors, counter, last load result
    #[unsafe(no_mangle)]
    static mut STATS: [i32; 5] = [0; 5];
    // !save: the blob and its read-back (the app thread has an 8 KiB stack)
    static mut SCRATCH: [u8; 2 * STORE_MAX + 1] = [0; 2 * STORE_MAX + 1];

    struct Buf<const N: usize> {
        b: [u8; N],
        n: usize,
    }

    impl<const N: usize> Buf<N> {
        fn new() -> Self {
            Buf { b: [0; N], n: 0 }
        }
        fn bytes(&self) -> &[u8] {
            &self.b[..self.n]
        }
        fn str(&self) -> &str {
            core::str::from_utf8(self.bytes()).unwrap_or("?")
        }
    }

    impl<const N: usize> Write for Buf<N> {
        fn write_str(&mut self, s: &str) -> core::fmt::Result {
            let take = s.len().min(N - self.n);
            self.b[self.n..self.n + take].copy_from_slice(&s.as_bytes()[..take]);
            self.n += take;
            Ok(())
        }
    }

    fn stat(i: usize, f: impl FnOnce(i32) -> i32) {
        unsafe {
            let p = &raw mut STATS[i];
            p.write_volatile(f(p.read_volatile()));
        }
    }

    struct Ui {
        lv: Lv,
        b: Batch,
        log: Log,
    }

    impl Ui {
        // None without LVGL: the simulator's lv_submit answers -95 to the font batch
        fn new(api: &Api) -> Option<Ui> {
            let lv = api.lv()?;
            let mut b = Batch::new()?;
            stock::font(&mut b).ok()?;
            b.submit(&lv).ok()?;
            let log = log::build(&mut b).ok()?;
            b.submit(&lv).ok()?;
            Some(Ui { lv, b, log })
        }
    }

    struct App {
        counter: u32,
        last: Buf<MSG_MAX>,
        ui: Option<Ui>,
    }

    impl App {
        fn load(api: &Api) -> Self {
            let mut raw = [0u8; 4];
            let rc = api.load(&mut raw);
            stat(4, |_| rc);
            let counter = if rc == 4 { u32::from_le_bytes(raw) } else { 0 };
            stat(3, |_| counter as i32);
            App { counter, last: Buf::new(), ui: Ui::new(api) }
        }

        fn reply(&self, api: &Api, data: &[u8]) {
            let mut rc = api.send(data);
            // outbox full: wait for the stub to drain it (one frame per ~10 ms, longer while the BLE stream is full)
            for _ in 0..500 {
                if rc != -11 {
                    break;
                }
                api.event(10);
                rc = api.send(data);
            }
            if rc == 0 { stat(1, |n| n + 1) } else { stat(2, |n| n + 1) }
        }

        fn message(&mut self, api: &Api, m: &[u8]) {
            stat(0, |n| n + 1);
            let text = core::str::from_utf8(m).unwrap_or("<binary>");
            match &mut self.ui {
                Some(u) => {
                    let _ = u.log.push(&mut u.b, text).and_then(|_| u.b.submit(&u.lv));
                }
                None => {
                    self.last = Buf::new();
                    let _ = self.last.write_str(text);
                }
            }
            let Some(cmd) = m.strip_prefix(b"!") else {
                self.reply(api, m);
                return;
            };
            let mut out = Buf::<MSG_MAX>::new();
            let arg = |p: &[u8]| core::str::from_utf8(p).ok().and_then(|s| s.trim().parse::<u32>().ok()).unwrap_or(0);
            match cmd {
                b"time" => match api.time() {
                    Some(t) => {
                        let _ = write!(out, "time {:04}-{:02}-{:02} {:02}:{:02}:{:02}.{:03} wd{} tz{}", t.year, t.month,
                            t.day, t.hour, t.minute, t.second, t.millis, t.weekday, t.utc_offset_min);
                    }
                    None => {
                        let _ = out.write_str("time none");
                    }
                },
                b"bat" => match api.battery() {
                    Some(b) => {
                        let (c, f, d) = (b.charging as u8, b.full as u8, b.charger as u8);
                        let _ = write!(out, "bat {} c{} f{} d{}", b.percent, c, f, d);
                    }
                    None => {
                        let _ = out.write_str("bat none");
                    }
                },
                b"count" => {
                    let _ = write!(out, "count {}", self.counter);
                }
                b"inc" => {
                    self.counter += 1;
                    let rc = api.save(&self.counter.to_le_bytes());
                    stat(3, |_| self.counter as i32);
                    let _ = write!(out, "inc {} {}", self.counter, rc);
                }
                _ if cmd.starts_with(b"keep ") => {
                    api.keep_screen_on(arg(&cmd[5..]) != 0);
                    let _ = write!(out, "keep {}", arg(&cmd[5..]));
                }
                _ if cmd.starts_with(b"spam ") => {
                    for i in 0..arg(&cmd[5..]) {
                        let mut m = Buf::<MSG_MAX>::new();
                        let _ = write!(m, "spam {}", i);
                        self.reply(api, m.bytes());
                    }
                    let _ = out.write_str("spam done");
                }
                _ if cmd.starts_with(b"save ") => {
                    let n = (arg(&cmd[5..]) as usize).min(STORE_MAX + 1);
                    let (blob, back) = unsafe { (*&raw mut SCRATCH).split_at_mut(STORE_MAX + 1) };
                    blob.fill(0xA5);
                    back.fill(0);
                    let rc = api.save(&blob[..n]);
                    let got = api.load(back);
                    let ok = got >= 0 && back[..got as usize].iter().all(|&b| b == 0xA5);
                    let _ = write!(out, "save {} {} {} {}", n, rc, got, ok as u8);
                    // keep the counter as the stored blob
                    api.save(&self.counter.to_le_bytes());
                }
                _ => {
                    let _ = out.write_str("unknown");
                }
            }
            self.reply(api, out.bytes());
        }

        fn draw(&mut self, api: &Api) {
            if let Some(u) = &mut self.ui {
                let mut line = Buf::<64>::new();
                if let Some(t) = api.time() {
                    let _ = write!(line, "{:02}:{:02}:{:02}  ", t.hour, t.minute, t.second);
                }
                match api.battery() {
                    Some(b) => {
                        let _ = write!(line, "{}%{}  ", b.percent, if b.charging { "+" } else { "" });
                    }
                    None => {
                        let _ = line.write_str("?%  ");
                    }
                }
                let _ = write!(line, "count {}", self.counter);
                let _ = u.b.text(u.log.status, line.str()).and_then(|_| u.b.submit(&u.lv));
                return;
            }
            api.rect(0, 0, api.width, api.height, BG);
            let mut line = Buf::<64>::new();
            if let Some(t) = api.time() {
                let _ = write!(line, "{:02}:{:02}:{:02}", t.hour, t.minute, t.second);
                api.text(169, 120, line.str(), FG);
                line = Buf::new();
                let _ = write!(line, "{:04}-{:02}-{:02}", t.year, t.month, t.day);
                api.text(153, 150, line.str(), DIM);
            }
            line = Buf::new();
            match api.battery() {
                Some(b) => {
                    let _ = write!(line, "battery {}%{}", b.percent, if b.charging { " +" } else { "" });
                }
                None => {
                    let _ = line.write_str("battery ?");
                }
            }
            api.text(137, 200, line.str(), FG);
            line = Buf::new();
            let _ = write!(line, "count {}  (tap +1)", self.counter);
            api.text(105, 240, line.str(), ACCENT);
            api.text(40, 290, self.last.str(), DIM);
            api.show();
        }
    }

    fn main(api: &'static Api) -> i32 {
        let mut app = App::load(api);
        let mut drawn = 0u32;
        app.draw(api);
        loop {
            let event = api.event(200);
            // every message, also those whose MESSAGE event reply() consumed while it waited for the outbox
            let mut buf = [0u8; MSG_MAX];
            let mut got = false;
            loop {
                let n = api.recv(&mut buf).map_or(0, |m| m.len());
                if n == 0 {
                    break;
                }
                app.message(api, &buf[..n]);
                got = true;
            }
            match event.map(|e| e.kind) {
                Some(BUTTON) => return 0,
                Some(TOUCH_UP) => {
                    app.counter += 1;
                    api.save(&app.counter.to_le_bytes());
                    stat(3, |_| app.counter as i32);
                }
                Some(MESSAGE | RESUME) => {}
                _ if got || api.now().wrapping_sub(drawn) >= 1000 => {}
                _ => continue,
            }
            app.draw(api);
            drawn = api.now();
        }
    }
}
