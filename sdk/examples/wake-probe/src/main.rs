#![no_std]
#![no_main]

// Does the phone answer while its app is in the background? Sends "R1P" ‖ seq:u16le every PERIOD_MS, the phone
// answers "R1p" ‖ seq (FMC Companion, Wake probe screen). The screen counts sent, refused and answered; the phone
// keeps the log with timestamps and its app state. Runs on the page too, so it can sit next to the face.
use cmf_wp2_app::{Api, BUTTON, MSG_MAX, PAUSE, RESUME};

cmf_wp2_app::entry!(main);
cmf_wp2_app::app_features!(cmf_wp2_app::PAGE);

const PERIOD_MS: u32 = 20_000;

/// "label 123" into `buf`.
fn line<'a>(buf: &'a mut [u8; 32], label: &str, n: u32) -> &'a str {
    let mut i = label.len();
    buf[..i].copy_from_slice(label.as_bytes());
    let mut digits = [0u8; 10];
    let (mut d, mut v) = (0, n);
    loop {
        digits[d] = b'0' + (v % 10) as u8;
        (d, v) = (d + 1, v / 10);
        if v == 0 { break; }
    }
    while d > 0 {
        d -= 1;
        buf[i] = digits[d];
        i += 1;
    }
    core::str::from_utf8(&buf[..i]).unwrap_or("?")
}

fn draw(api: &Api, rows: [(&str, u32); 4]) {
    api.rect(0, 0, api.width, api.height, 0x0000);
    for (k, (label, n)) in rows.into_iter().enumerate() {
        let mut buf = [0u8; 32];
        api.text(80, 150 + 40 * k as i16, line(&mut buf, label, n), 0xFFFF);
    }
    api.show();
}

fn main(api: &'static Api) -> i32 {
    let (mut seq, mut refused, mut answered, mut last) = (0u16, 0u32, 0u32, 0u32);
    let mut next = api.now();
    let mut paused = false;
    loop {
        let now = api.now();
        if now.wrapping_sub(next) < u32::MAX / 2 {
            seq = seq.wrapping_add(1);
            let [lo, hi] = seq.to_le_bytes();
            // ponytail: sent while paused too; whether the stub delivers then is part of what this probes
            if api.send(&[b'R', b'1', b'P', lo, hi]) != 0 {
                refused += 1;
            }
            next = now.wrapping_add(PERIOD_MS);
        }
        let mut buf = [0u8; MSG_MAX];
        while let Some(m) = api.recv(&mut buf) {
            if let [b'R', b'1', b'p', lo, hi] = m {
                answered += 1;
                last = u16::from_le_bytes([*lo, *hi]) as u32;
            }
        }
        if !paused {
            draw(api, [("sent ", seq as u32), ("refused ", refused), ("answered ", answered), ("last #", last)]);
        }
        match api.event(1000).map(|e| e.kind) {
            Some(BUTTON) => return 0,
            Some(PAUSE) => paused = true,
            Some(RESUME) => paused = false,
            _ => {}
        }
    }
}
