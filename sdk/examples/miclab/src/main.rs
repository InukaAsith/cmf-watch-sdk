#![no_std]
#![no_main]

//! Mic lab: tests the loader's mic (docs/abi-1.4.md, "Mic") on a watch with no phone. Tap: records 3 s, showing
//! the level, how many bytes a second come (16 kHz mono = 32000, two channels 64000) and how many channels that
//! makes, then plays it back on the speaker at 8 kHz (cmf_wp2_app::mic, with its automatic gain).
//! The numbers stay on screen. Side button: exit.

use cmf_wp2_app::mic::Voice;
use cmf_wp2_app::{adpcm, Api, BUTTON, TOUCH_DOWN};

cmf_wp2_app::entry!(main);

const BLACK: u16 = 0x0000;
const WHITE: u16 = 0xFFFF;
const GREY: u16 = 0x7BEF;
const RED: u16 = 0xD8C4;
const GREEN: u16 = 0x07E0;

const SECS: usize = 3;
/// 8 kHz i16: pairs of the 16 kHz samples averaged
const KEEP: usize = SECS * 8000 * 2;
static mut KEPT: [u8; KEEP] = [0; KEEP];

fn num(mut v: i64, out: &mut [u8; 24]) -> &str {
    let neg = v < 0;
    v = v.abs();
    let mut i = out.len();
    loop {
        i -= 1;
        out[i] = b'0' + (v % 10) as u8;
        v /= 10;
        if v == 0 {
            break;
        }
    }
    if neg {
        i -= 1;
        out[i] = b'-';
    }
    core::str::from_utf8(&out[i..]).unwrap_or("?")
}

fn line(api: &Api, y: i16, label: &str, v: i64) {
    let mut b = [0u8; 24];
    api.rect(60, y - 4, 346, 26, BLACK);
    api.text(80, y, label, GREY);
    api.text(300, y, num(v, &mut b), WHITE);
}

/// Sends `m`, waiting up to 10 s while the outbox is full (-11) for the host to take it; another error (no host)
/// at once.
fn send_wait(api: &Api, m: &[u8]) -> i32 {
    let t = api.now();
    loop {
        let rc = api.send(m);
        if rc != -11 || api.now().wrapping_sub(t) > 10_000 {
            return rc;
        }
        api.event(20);
    }
}

fn main(api: &'static Api) -> i32 {
    let kept = unsafe { &mut *core::ptr::addr_of_mut!(KEPT) };
    api.rect(0, 0, 466, 466, BLACK);
    api.text(170, 60, "MIC LAB", WHITE);
    api.text(120, 400, "tap: record 3 s", GREY);
    api.show();
    loop {
        let Some(e) = api.event(u32::MAX) else { continue };
        match e.kind {
            BUTTON => return 0,
            TOUCH_DOWN => {}
            _ => continue,
        }
        api.rect(0, 100, 466, 330, BLACK);
        api.rect(120, 395, 240, 30, BLACK);
        api.text(150, 400, "recording...", RED);
        let (mut voice, open) = Voice::start(api);
        line(api, 120, "open", open as i64);
        api.show();
        let t0 = api.now();
        let (mut total, mut kept_n, mut peak, mut reads, mut err) = (0i64, 0usize, 0i32, 0i64, 0i64);
        let mut last_draw = t0;
        while api.now().wrapping_sub(t0) < (SECS as u32) * 1000 + 500 {
            if let Some(e) = api.event(10) {
                if e.kind == BUTTON {
                    Voice::stop(api);
                    return 0;
                }
            }
            let n = voice.pump(api, |v| {
                if kept_n + 2 <= KEEP {
                    kept[kept_n..kept_n + 2].copy_from_slice(&v.to_le_bytes());
                    kept_n += 2;
                }
            });
            if n < 0 {
                err = n as i64;
                continue;
            }
            if n > 0 {
                reads += 1;
                total += n as i64;
            }
            if api.now().wrapping_sub(last_draw) > 100 {
                last_draw = api.now();
                let l = voice.take_level();
                peak = peak.max(l);
                api.rect(83, 330, 300, 20, 0x2104);
                api.rect(83, 330, (l * 300 / 32768).clamp(1, 300) as u16, 20, GREEN);
                line(api, 160, "bytes", total);
                line(api, 200, "reads", reads);
                line(api, 240, "error", err);
                api.show();
            }
        }
        Voice::stop(api);
        line(api, 160, "bytes", total);
        line(api, 200, "reads", reads);
        line(api, 240, "error", err);
        line(api, 280, "bytes/s", voice.rate(api) as i64);
        line(api, 300 + 20, "channels", voice.channels as i64);
        line(api, 360, "peak", peak as i64);
        // the recording as it came, to a listening host (`wp2apps listen`): `a seq:u16` + one IMA ADPCM block of
        // 240 samples at 8 kHz, then `e count:u16`
        api.rect(120, 395, 240, 30, BLACK);
        api.text(150, 400, "sending...", GREY);
        api.show();
        let samples = kept_n / 2;
        let mut st = adpcm::State::default();
        let mut seq = 0u16;
        let mut block = [0i16; 240];
        let mut msg = [0u8; 3 + adpcm::BLOCK_HEADER + 120];
        for at in (0..samples).step_by(240) {
            let k = (samples - at).min(240) & !1;
            for (i, b) in block[..k].iter_mut().enumerate() {
                *b = i16::from_le_bytes([kept[(at + i) * 2], kept[(at + i) * 2 + 1]]);
            }
            msg[0] = b'a';
            msg[1..3].copy_from_slice(&seq.to_le_bytes());
            let n = 3 + adpcm::encode_block(&mut st, &block[..k], &mut msg[3..]);
            let t = api.now();
            if send_wait(api, &msg[..n]) < 0 {
                break;
            }
            seq += 1;
        }
        let mut end = [b'e', 0, 0];
        end[1..].copy_from_slice(&seq.to_le_bytes());
        let _ = send_wait(api, &end);
        api.rect(120, 395, 240, 30, BLACK);
        api.text(150, 400, "playing back", GREEN);
        api.show();
        // back at 8 kHz
        if kept_n > 0 && api.audio_open(8) == 0 {
            api.audio_volume(100);
            let mut at = 0;
            let t1 = api.now();
            while at < kept_n && api.now().wrapping_sub(t1) < 6000 {
                let w = api.audio_write(&kept[at..kept_n]);
                if w > 0 {
                    at += w as usize;
                } else {
                    api.event(20);
                }
            }
            while api.audio_queued() > 0 && api.now().wrapping_sub(t1) < 7000 {
                api.event(20);
            }
            api.audio_close();
        }
        api.rect(120, 395, 240, 30, BLACK);
        api.text(120, 400, "tap: record again", GREY);
        api.show();
    }
}
