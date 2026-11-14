//! The "w1" protocol (docs/workout.md, Protocol): the Mac pushes a program in B/C/E messages, asks for the history
//! with Q; the app asks with R, acks with A and answers Q with Y messages.

use crate::model::{Entry, PROGRAM_MAX};

#[derive(Debug, PartialEq)]
pub enum Got<'a> {
    Nothing,
    Program(&'a [u8]),
    Resync,
    History,
    Trace,
    /// D: a set done on the phone (ex 0xFF: the workout only runs).
    Set { start: u32, day: u8, version: u32, ex: u8, t: u16 },
    /// F: the phone finished that workout.
    Finish { start: u32, end: u32 },
    /// H: a workout done on the phone alone.
    Entry(Entry),
    /// Z: heart-rate zones, the lowest bpm of zones 1..5.
    Zones([u8; 5]),
    /// U: the phone took a set back.
    Unset { start: u32, ex: u8, t: u16 },
}

const MAGIC: &[u8; 2] = b"w1";

/// Reassembles one program transfer. Any gap (seq), misplaced chunk, oversize or bad end drops it: `Resync`, and
/// the app asks again with R.
pub struct Transfer {
    buf: [u8; PROGRAM_MAX],
    got: usize,
    // bytes announced by B; None while no transfer runs
    total: Option<usize>,
    next_seq: u8,
}

impl Transfer {
    pub const fn new() -> Transfer {
        Transfer { buf: [0; PROGRAM_MAX], got: 0, total: None, next_seq: 0 }
    }

    pub fn apply(&mut self, msg: &[u8]) -> Got<'_> {
        let [m0, m1, kind, seq, body @ ..] = msg else { return Got::Nothing };
        if [*m0, *m1] != *MAGIC {
            return Got::Nothing;
        }
        let (kind, seq) = (*kind, *seq);
        match kind {
            b'Q' => Got::History,
            b'T' => Got::Trace,
            b'D' => match body {
                &[a, b, c, d, day, v0, v1, v2, v3, ex, t0, t1] => Got::Set {
                    start: u32::from_le_bytes([a, b, c, d]), day, version: u32::from_le_bytes([v0, v1, v2, v3]), ex,
                    t: u16::from_le_bytes([t0, t1]),
                },
                _ => Got::Nothing,
            },
            b'Z' => match body {
                &[a, b, c, d, e] => Got::Zones([a, b, c, d, e]),
                _ => Got::Nothing,
            },
            b'U' => match body {
                &[a, b, c, d, ex, t0, t1] => Got::Unset { start: u32::from_le_bytes([a, b, c, d]), ex, t: u16::from_le_bytes([t0, t1]) },
                _ => Got::Nothing,
            },
            b'F' => match body {
                &[a, b, c, d, e, f, g, h] => Got::Finish { start: u32::from_le_bytes([a, b, c, d]), end: u32::from_le_bytes([e, f, g, h]) },
                _ => Got::Nothing,
            },
            b'H' => match body {
                &[a, b, c, d, d0, d1, day, done, planned] => Got::Entry(Entry {
                    start: u32::from_le_bytes([a, b, c, d]), dur_s: u16::from_le_bytes([d0, d1]), day, done, planned,
                }),
                _ => Got::Nothing,
            },
            b'B' => match body {
                [a, b] if (1..=PROGRAM_MAX).contains(&usize::from(u16::from_le_bytes([*a, *b]))) => {
                    (self.got, self.total, self.next_seq) = (0, Some(usize::from(u16::from_le_bytes([*a, *b]))), seq.wrapping_add(1));
                    Got::Nothing
                }
                _ => self.drop(),
            },
            b'C' | b'E' => {
                let Some(total) = self.total.filter(|_| seq == self.next_seq) else { return self.drop() };
                self.next_seq = seq.wrapping_add(1);
                match (kind, body) {
                    (b'C', [a, b, data @ ..]) if usize::from(u16::from_le_bytes([*a, *b])) == self.got && self.got + data.len() <= total => {
                        self.buf[self.got..self.got + data.len()].copy_from_slice(data);
                        self.got += data.len();
                        Got::Nothing
                    }
                    (b'E', [a, b, c, d]) => {
                        let bytes = &self.buf[..total];
                        let ok = self.got == total && crate::model::crc32(bytes) == u32::from_le_bytes([*a, *b, *c, *d])
                            && crate::model::Program::check(bytes).is_some();
                        self.total = None;
                        if ok { Got::Program(&self.buf[..total]) } else { Got::Resync }
                    }
                    _ => self.drop(),
                }
            }
            _ => Got::Nothing,
        }
    }

    fn drop(&mut self) -> Got<'static> {
        self.total = None;
        Got::Resync
    }
}

fn with_u32(kind: u8, v: u32) -> [u8; 7] {
    let b = v.to_le_bytes();
    [MAGIC[0], MAGIC[1], kind, b[0], b[1], b[2], b[3]]
}

/// "Send me the program unless I have `version`" (0 = none); `running`: a workout is in progress, so a host holds a
/// new program back until it ends (flags bit 0).
pub fn request(version: u32, running: bool) -> [u8; 8] {
    let r = with_u32(b'R', version);
    [r[0], r[1], r[2], r[3], r[4], r[5], r[6], running as u8]
}

/// "Stored `version`."
pub fn ack(version: u32) -> [u8; 7] {
    with_u32(b'A', version)
}

/// One history entry, `index` of `count`, newest first.
pub fn history_msg(index: u8, count: u8, e: &Entry) -> [u8; 14] {
    let (s, d) = (e.start.to_le_bytes(), e.dur_s.to_le_bytes());
    [MAGIC[0], MAGIC[1], b'Y', index, count, s[0], s[1], s[2], s[3], d[0], d[1], e.day, e.done, e.planned]
}

/// Trace bytes per X message.
pub const X_CHUNK: usize = 100;

/// Part `index` of `count` of the last workout's trace (`Trace::encode`) for the workout that started at `start`.
pub fn trace_msg(index: u8, count: u8, start: u32, chunk: &[u8], out: &mut [u8; 9 + X_CHUNK]) -> usize {
    let s = start.to_le_bytes();
    out[..9].copy_from_slice(&[MAGIC[0], MAGIC[1], b'X', index, count, s[0], s[1], s[2], s[3]]);
    out[9..9 + chunk.len()].copy_from_slice(chunk);
    9 + chunk.len()
}

/// Live state of the workout that started at `start` on `day`: heart rate (0 = none), the open exercise (0xFF =
/// none), sets done so far.
pub fn pulse(start: u32, day: u8, bpm: u8, ex: u8, done: u8, ex_done: u8, rest_left: u16, diag: [u8; 13]) -> [u8; 27] {
    let (s, r) = (start.to_le_bytes(), rest_left.to_le_bytes());
    let mut p = [0; 27];
    p[..14].copy_from_slice(&[MAGIC[0], MAGIC[1], b'P', s[0], s[1], s[2], s[3], day, bpm, ex, done, ex_done, r[0], r[1]]);
    p[14..].copy_from_slice(&diag);
    p
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::crc32;
    use crate::model::tests::enc;

    fn msgs(program: &[u8], crc: u32) -> Vec<Vec<u8>> {
        let mut v = vec![[b"w1B".as_slice(), &[0], &(program.len() as u16).to_le_bytes()].concat()];
        for (i, chunk) in program.chunks(120).enumerate() {
            v.push([b"w1C".as_slice(), &[1 + i as u8], &((i * 120) as u16).to_le_bytes(), chunk].concat());
        }
        let seq = v.len() as u8;
        v.push([b"w1E".as_slice(), &[seq], &crc.to_le_bytes()].concat());
        v
    }

    fn program() -> Vec<u8> {
        let name = "Упражнение номер";
        let exs: Vec<crate::model::tests::Ex> = (0..7).map(|_| (name, 4, 5, 8, 90, "60 кг")).collect();
        enc(&[("День", &exs[..])])
    }

    fn run(t: &mut Transfer, m: &[Vec<u8>]) -> Vec<String> {
        m.iter().map(|x| format!("{:?}", t.apply(x)).chars().take(12).collect()).collect()
    }

    #[test]
    fn transfer_ok() {
        let p = program();
        assert!(p.len() > 240 && p.len() <= 360, "{}", p.len()); // three C messages
        let m = msgs(&p, crc32(&p));
        assert!(m.iter().all(|x| x.len() <= 128));
        let mut t = Transfer::new();
        for x in &m[..m.len() - 1] {
            assert_eq!(t.apply(x), Got::Nothing);
        }
        assert_eq!(t.apply(m.last().unwrap()), Got::Program(&p));
    }

    #[test]
    fn transfer_gap() {
        let p = program();
        let m = msgs(&p, crc32(&p));
        let mut t = Transfer::new();
        assert_eq!(t.apply(&m[0]), Got::Nothing);
        assert_eq!(t.apply(&m[2]), Got::Resync); // seq 1 lost
        assert_eq!(t.apply(&m[3]), Got::Resync); // nothing in progress any more
        for x in &m[..m.len() - 1] {
            assert_eq!(t.apply(x), Got::Nothing);
        }
        assert_eq!(t.apply(m.last().unwrap()), Got::Program(&p));
    }

    #[test]
    fn transfer_bad_crc() {
        let p = program();
        let m = msgs(&p, crc32(&p) ^ 1);
        let mut t = Transfer::new();
        assert_eq!(run(&mut t, &m).last().unwrap(), "Resync");
    }

    #[test]
    fn transfer_bad_program() {
        let p = vec![9u8; 10]; // nine days announced, garbage: parse refuses it
        let m = msgs(&p, crc32(&p));
        let mut t = Transfer::new();
        assert_eq!(run(&mut t, &m).last().unwrap(), "Resync");
    }

    #[test]
    fn transfer_too_big() {
        let mut t = Transfer::new();
        assert_eq!(t.apply(&[b"w1B".as_slice(), &[0], &2049u16.to_le_bytes()].concat()), Got::Resync);
        assert_eq!(t.apply(&[b"w1C".as_slice(), &[1], &0u16.to_le_bytes(), &[0u8; 120]].concat()), Got::Resync);
    }

    #[test]
    fn transfer_overflow_and_offset() {
        let mut t = Transfer::new();
        t.apply(&[b"w1B".as_slice(), &[0], &100u16.to_le_bytes()].concat());
        assert_eq!(t.apply(&[b"w1C".as_slice(), &[1], &0u16.to_le_bytes(), &[0u8; 120]].concat()), Got::Resync);
        t.apply(&[b"w1B".as_slice(), &[0], &300u16.to_le_bytes()].concat());
        assert_eq!(t.apply(&[b"w1C".as_slice(), &[1], &5u16.to_le_bytes(), &[0u8; 10]].concat()), Got::Resync);
    }

    #[test]
    fn foreign_magic() {
        let mut t = Transfer::new();
        assert_eq!(t.apply(b"h1B\0"), Got::Nothing);
        assert_eq!(t.apply(b"w1"), Got::Nothing);
        assert_eq!(t.apply(b"w1Z\0"), Got::Nothing);
    }

    #[test]
    fn query() {
        let mut t = Transfer::new();
        assert_eq!(t.apply(b"w1Q\x07"), Got::History);
        assert_eq!(t.apply(b"w1T\x00"), Got::Trace);
        assert_eq!(t.apply(b"w1D\x00\x0D\x0C\x0B\x0A\x01\x04\x03\x02\x01\x02\x1E\x00"),
                   Got::Set { start: 0x0A0B_0C0D, day: 1, version: 0x0102_0304, ex: 2, t: 30 });
        assert_eq!(t.apply(b"w1F\x00\x0D\x0C\x0B\x0A\x01\x00\x00\x00"), Got::Finish { start: 0x0A0B_0C0D, end: 1 });
        assert_eq!(t.apply(b"w1H\x00\x0D\x0C\x0B\x0A\x02\x01\x02\x07\x09"),
                   Got::Entry(Entry { start: 0x0A0B_0C0D, dur_s: 0x0102, day: 2, done: 7, planned: 9 }));
        assert_eq!(t.apply(b"w1D\x00\x01"), Got::Nothing, "short");
        assert_eq!(t.apply(b"w1Z\x00\x5F\x72\x85\x98\xAB"), Got::Zones([95, 114, 133, 152, 171]));
        assert_eq!(t.apply(b"w1U\x00\x0D\x0C\x0B\x0A\x02\x1E\x00"), Got::Unset { start: 0x0A0B_0C0D, ex: 2, t: 30 });
    }

    #[test]
    fn encodings() {
        assert_eq!(&request(0x0102_0304, true), b"w1R\x04\x03\x02\x01\x01");
        assert_eq!(&request(0x0102_0304, false), b"w1R\x04\x03\x02\x01\x00");
        assert_eq!(&ack(0x0102_0304), b"w1A\x04\x03\x02\x01");
        let e = Entry { start: 0x0A0B_0C0D, dur_s: 0x0102, day: 2, done: 7, planned: 9 };
        assert_eq!(&history_msg(1, 3, &e), b"w1Y\x01\x03\x0D\x0C\x0B\x0A\x02\x01\x02\x07\x09");
        assert_eq!(&pulse(0x0A0B_0C0D, 1, 72, 3, 5, 2, 0x0102, [9, 8, 7, 6, 5, 1, 2, 3, 4, 5, 6, 7, 8]),
                   b"w1P\x0D\x0C\x0B\x0A\x01\x48\x03\x05\x02\x02\x01\x09\x08\x07\x06\x05\x01\x02\x03\x04\x05\x06\x07\x08");
        let mut out = [0u8; 9 + X_CHUNK];
        let n = trace_msg(1, 2, 0x0A0B_0C0D, &[7, 8], &mut out);
        assert_eq!(&out[..n], b"w1X\x01\x02\x0D\x0C\x0B\x0A\x07\x08");
        assert!(9 + X_CHUNK <= 128);
    }
}
