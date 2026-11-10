use wp2_core::gfx::Canvas;
use wp2_core::layout::SLOTS;
use wp2_core::pkg::{check_slot, parse_header, HEADER_SIZE};

pub const ROWS: usize = SLOTS as usize;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Entry {
    pub slot: u8,
    pub name: [u8; 24],
    pub len: u8,
}

const X: i16 = 90;
const TOP: i16 = 120;
const ROW_H: i16 = 60;
const TEXT_DY: i16 = 12;
const WHITE: u16 = 0xFFFF;

pub fn entry(slot: u8, raw: &[u8; HEADER_SIZE]) -> Option<Entry> {
    let h = parse_header(raw).ok()?;
    check_slot(&h, slot).ok()?;
    let len = h.name.iter().position(|&b| b == 0).unwrap_or(h.name.len()) as u8;
    Some(Entry { slot, name: h.name, len })
}

// Entries are packed from index 0; row i shows entries[i].
#[cfg(target_os = "none")]
pub fn scan() -> [Option<Entry>; ROWS] {
    use wp2_core::{fs, layout::slot_path};
    let mut out = [None; ROWS];
    let mut n = 0;
    let mut file: fs::File = [0; 0x2C];
    for slot in 0..SLOTS {
        if fs::open(&mut file, &slot_path(slot, false), fs::READ) < 0 {
            continue;
        }
        let mut raw = [0u8; HEADER_SIZE];
        let ok = fs::read_exact(&mut file, &mut raw);
        fs::close(&mut file);
        if let Some(e) = entry(slot, &raw).filter(|_| ok) {
            out[n] = Some(e);
            n += 1;
        }
    }
    out
}

// ponytail: rows 6 and 7 fall below the round screen; scroll when 7+ apps get real
// `failed`: (row, code) shows "Error <code>" in place of that row's name
pub fn draw(c: &mut Canvas, entries: &[Option<Entry>; ROWS], selected: usize, failed: Option<(usize, i32)>) {
    use wp2_core::gfx::{H, W};
    c.fill_rect(0, 0, W as u16, H as u16, 0);
    c.text((W as i16 - 4 * 16) / 2, 60, b"Apps", WHITE);
    let mut shown = 0;
    let mut buf = [0u8; 18];
    for (i, e) in entries.iter().flatten().enumerate() {
        let y = TOP + ROW_H * i as i16 + TEXT_DY;
        if i == selected {
            c.text(X - 24, y, b">", WHITE);
        }
        let text = match failed {
            Some((row, code)) if row == i => error(code, &mut buf),
            _ => &e.name[..usize::from(e.len)],
        };
        c.text(X, y, text, WHITE);
        shown += 1;
    }
    if shown == 0 {
        c.text(X, TOP + TEXT_DY, b"No apps", WHITE);
    }
}

pub fn error(code: i32, buf: &mut [u8; 18]) -> &[u8] {
    buf[..6].copy_from_slice(b"Error ");
    let mut n = 6;
    if code < 0 {
        buf[n] = b'-';
        n += 1;
    }
    let mut digits = [0u8; 10];
    let mut v = code.unsigned_abs();
    let mut d = 0;
    loop {
        digits[d] = b'0' + (v % 10) as u8;
        d += 1;
        v /= 10;
        if v == 0 {
            break;
        }
    }
    for &b in digits[..d].iter().rev() {
        buf[n] = b;
        n += 1;
    }
    &buf[..n]
}

pub fn hit(y: i16) -> Option<usize> {
    let row = usize::try_from((y - TOP) / ROW_H).ok().filter(|_| y >= TOP)?;
    (row < ROWS).then_some(row)
}

#[cfg(test)]
mod tests {
    use super::*;
    use wp2_core::gfx::{H, W};
    use wp2_core::pkg::crc32;

    fn header(name: &[u8], kind: u8) -> [u8; HEADER_SIZE] {
        let mut raw = [0u8; HEADER_SIZE];
        raw[..4].copy_from_slice(b"WP2A");
        raw[4] = 1;
        raw[6] = kind;
        raw[8..8 + name.len()].copy_from_slice(name);
        raw[0x24..0x28].copy_from_slice(&0x100u32.to_le_bytes());
        let crc = crc32(0, &raw[..0x34]);
        raw[0x34..0x38].copy_from_slice(&crc.to_le_bytes());
        raw
    }

    fn lit(px: &[u16], x: usize, y: usize, w: usize, h: usize) -> usize {
        (y..y + h).map(|r| px[r * W + x..r * W + x + w].iter().filter(|&&p| p != 0).count()).sum()
    }

    #[test]
    fn entry_needs_valid_app_header() {
        let e = entry(2, &header(b"hello", 0)).unwrap();
        assert_eq!((e.slot, &e.name[..e.len as usize]), (2, &b"hello"[..]));
        assert_eq!(entry(2, &header(b"loader", 1)), None);
        let mut bad = header(b"hello", 0);
        bad[0x20] ^= 1;
        assert_eq!(entry(2, &bad), None);
    }

    #[test]
    fn rows_and_hits() {
        assert_eq!(hit(119), None);
        assert_eq!(hit(120), Some(0));
        assert_eq!(hit(179), Some(0));
        assert_eq!(hit(180), Some(1));
        assert_eq!(hit(120 + 60 * ROWS as i16), None);
        assert_eq!(hit(-5), None);
    }

    #[test]
    fn draws_title_and_rows() {
        let mut px = vec![0u16; W * H];
        let a = entry(0, &header(b"hello", 0));
        let b = entry(1, &header(b"vibration-lab", 0));
        let mut entries = [None; ROWS];
        entries[0] = a;
        entries[1] = b;
        draw(&mut Canvas { px: &mut px }, &entries, 1, None);
        assert!(lit(&px, 0, 60, W, 16) > 50);
        assert!(lit(&px, 90, 120, 300, 40) > 50);
        assert!(lit(&px, 90, 180, 300, 40) > 50);
        assert_eq!(lit(&px, 90, 240, 300, 40), 0);
        assert!(lit(&px, 0, 180, 90, 40) > 0, "selection marker left of row 1");
        assert_eq!(lit(&px, 0, 120, 90, 40), 0);
    }

    #[test]
    fn error_text() {
        let mut buf = [0u8; 18];
        assert_eq!(error(-12, &mut buf), b"Error -12");
        assert_eq!(error(i32::MIN, &mut buf), b"Error -2147483648");
        assert_eq!(error(5, &mut buf), b"Error 5");
    }

    #[test]
    fn error_replaces_the_row_name() {
        let mut entries = [None; ROWS];
        entries[0] = entry(0, &header(b"hello", 0));
        let mut plain = vec![0u16; W * H];
        draw(&mut Canvas { px: &mut plain }, &entries, 0, None);
        let mut px = vec![0u16; W * H];
        draw(&mut Canvas { px: &mut px }, &entries, 0, Some((0, -12)));
        assert!(lit(&px, 90, 120, 300, 40) > lit(&plain, 90, 120, 300, 40), "\"Error -12\" is longer than \"hello\"");
        let mut other = vec![0u16; W * H];
        draw(&mut Canvas { px: &mut other }, &entries, 0, Some((1, -12)));
        assert_eq!(other, plain);
    }

    #[test]
    fn empty_menu_says_no_apps() {
        let mut px = vec![0xFFFFu16; W * H];
        draw(&mut Canvas { px: &mut px }, &[None; ROWS], 0, None);
        assert!(lit(&px, 90, 120, 300, 40) > 50);
        assert_eq!(lit(&px, 0, 180, W, H - 180), 0);
    }
}
