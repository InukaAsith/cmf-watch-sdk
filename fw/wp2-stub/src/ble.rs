// Own install channel on c1 = 0xFFFF, c2 = 0x90A0..=0x90A4 (docs/specs; Status and requests in wp2_core::proto).
use core::mem::transmute;
use wp2_core::fs::{self, File};
use wp2_core::layout::{slot_path, stub_ram, StubRam, APP_SIZE, DIR, LOADER_SIZE, LOADER_SLOT};
use wp2_core::pkg::{check_slot, crc32, parse_header, HEADER_SIZE};
use wp2_core::proto::{parse, Request, Status, DELETE, INFO, MAX_DATA};

const READ_PAYLOAD: u32 = 0x100CC285;
const DECRYPT: u32 = 0x101B608D;
const ENCRYPT: u32 = 0x101B605B;
const SEND: u32 = 0x100CC2D5;
// stock key buffers (docs/recon.md): 0x804B answers with whatever authkey is set, all zero after boot or after the
// bind popup is confirmed, so a session counts only when both keys are non-zero. This only closes the zero-key
// path: any client in BLE range reads the pairing secret (AT GETSECRET, also on the real watch) and binds without
// a tap, so it can install silently; same exposure as stock OTA, accepted by the owner (docs/recon.md).
const AUTHKEY: u32 = 0x3804_8E08;
const SESSION_KEY: u32 = 0x3800_29A6;
const EEXIST: i32 = -17;
const HEADER_LEN: usize = 11;
// longest reply: status + 64-byte header
const MAX_REPLY: usize = 1 + HEADER_SIZE;

#[unsafe(naked)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn wp2_ble_default() {
    core::arch::naked_asm!("mov r2, r5", "b.w {c}", c = sym wp2_ble_c)
}

// Replaces `bl 0x100CC284` (read the payload into buf) in the stock default branch; frame is host-order.
extern "C" fn wp2_ble_c(buf: *mut u8, len: u16, frame: *const u8) -> i32 {
    let read: extern "C" fn(*mut u8, u32) -> i32 = unsafe { transmute(READ_PAYLOAD as usize) };
    let rc = read(buf, len as u32);
    let f = unsafe { core::slice::from_raw_parts(frame, HEADER_LEN) };
    let c1 = u16::from_le_bytes([f[3], f[4]]);
    let c2 = u16::from_le_bytes([f[9], f[10]]);
    if rc != 0 || c1 != 0xFFFF || !(INFO..=DELETE).contains(&c2) || !keyed(AUTHKEY) || !keyed(SESSION_KEY) {
        return rc;
    }
    let decrypt: extern "C" fn(*mut u16, *const u8, u32, u32) -> *const u8 = unsafe { transmute(DECRYPT as usize) };
    let mut n = 0u16;
    let plain = decrypt(&mut n, buf, len as u32, c2 as u32);
    if plain.is_null() {
        return rc;
    }
    let payload = unsafe { core::slice::from_raw_parts(plain, n as usize) };
    let mut out = [0u8; MAX_REPLY];
    let size = handle(stub_ram(), c2, payload, &mut out);
    reply(stub_ram(), c2, &out[..size]);
    rc
}

fn keyed(addr: u32) -> bool {
    unsafe { core::slice::from_raw_parts(addr as *const u8, 16) }.iter().any(|&b| b != 0)
}

fn reply(ram: &mut StubRam, c2: u16, plain: &[u8]) {
    let encrypt: extern "C" fn(*mut u16, *const u8, u32, u32) -> *const u8 = unsafe { transmute(ENCRYPT as usize) };
    let send: extern "C" fn(*const u8, u32) -> i32 = unsafe { transmute(SEND as usize) };
    let reply_c2 = c2 | 0xA000;
    let mut n = 0u16;
    let cipher = encrypt(&mut n, plain.as_ptr(), plain.len() as u32, reply_c2 as u32);
    if cipher.is_null() || HEADER_LEN + n as usize > ram.reply.len() {
        return;
    }
    let r = &mut ram.reply;
    r[0] = 0xF5;
    r[1..3].copy_from_slice(&n.to_be_bytes());
    r[3..5].copy_from_slice(&0xFFFFu16.to_be_bytes());
    r[5..9].fill(0);
    r[9..11].copy_from_slice(&reply_c2.to_be_bytes());
    unsafe { core::ptr::copy_nonoverlapping(cipher, r[HEADER_LEN..].as_mut_ptr(), n as usize) };
    send(r.as_ptr(), (HEADER_LEN + n as usize) as u32);
}

fn busy(ram: &StubRam, slot: u8) -> bool {
    if slot == LOADER_SLOT { ram.loader_active != 0 } else { slot == ram.running_slot }
}

fn status(out: &mut [u8], s: Status) -> usize {
    out[0] = s as u8;
    1
}

fn handle(ram: &mut StubRam, c2: u16, payload: &[u8], out: &mut [u8; MAX_REPLY]) -> usize {
    if c2 == INFO || c2 == DELETE {
        abort(ram); // a host only sends these between transfers, so an open one was abandoned
    }
    match parse(c2, payload) {
        Err(s) => status(out, s),
        Ok(Request::Info(slot)) => info(slot, out),
        Ok(Request::Begin { slot, size, crc }) => {
            let s = begin(ram, slot, size, crc);
            status(out, s)
        }
        Ok(Request::Data { offset, bytes }) => {
            let s = data(ram, offset, bytes);
            out[1..5].copy_from_slice(&ram.xfer_written.to_le_bytes());
            out[0] = s as u8;
            5
        }
        Ok(Request::End) => {
            let s = end(ram);
            status(out, s)
        }
        Ok(Request::Delete(slot)) => {
            let s = if busy(ram, slot) {
                Status::Busy
            } else if fs::unlink(&slot_path(slot, false)) < 0 {
                Status::Io
            } else {
                Status::Ok
            };
            status(out, s)
        }
    }
}

fn info(slot: u8, out: &mut [u8; MAX_REPLY]) -> usize {
    let mut file: File = [0; 0x2C];
    out[0] = Status::Ok as u8;
    if fs::open(&mut file, &slot_path(slot, false), fs::READ) < 0 {
        return 1;
    }
    let ok = fs::read_exact(&mut file, &mut out[1..]);
    fs::close(&mut file);
    if ok { MAX_REPLY } else { 1 }
}

fn abort(ram: &mut StubRam) {
    if ram.xfer_state != 0 {
        fs::close(&mut ram.file);
        fs::unlink(&slot_path(ram.xfer_slot, true));
        ram.xfer_state = 0;
    }
}

fn begin(ram: &mut StubRam, slot: u8, size: u32, crc: u32) -> Status {
    if busy(ram, slot) {
        return Status::Busy;
    }
    let limit = if slot == LOADER_SLOT { LOADER_SIZE } else { APP_SIZE };
    if (size as usize) < HEADER_SIZE || size > HEADER_SIZE as u32 + 2 * limit {
        return Status::BadRequest;
    }
    abort(ram);
    let rc = fs::mkdir(DIR);
    if rc < 0 && rc != EEXIST {
        return Status::Io;
    }
    let tmp = slot_path(slot, true);
    fs::unlink(&tmp);
    if fs::open(&mut ram.file, &tmp, fs::WRITE | fs::CREATE) < 0 {
        return Status::Io;
    }
    ram.xfer_state = 1;
    ram.xfer_slot = slot;
    ram.xfer_size = size;
    ram.xfer_crc = crc;
    ram.xfer_written = 0;
    Status::Ok
}

fn data(ram: &mut StubRam, offset: u32, bytes: &[u8]) -> Status {
    if ram.xfer_state != 1 {
        return Status::Sequence;
    }
    if offset != ram.xfer_written {
        return Status::Sequence;
    }
    if bytes.len() > MAX_DATA || ram.xfer_written as usize + bytes.len() > ram.xfer_size as usize {
        return Status::BadRequest;
    }
    if fs::write(&mut ram.file, bytes) != bytes.len() as i32 {
        abort(ram);
        return Status::Io;
    }
    ram.xfer_written += bytes.len() as u32;
    Status::Ok
}

fn end(ram: &mut StubRam) -> Status {
    if ram.xfer_state != 1 || ram.xfer_written != ram.xfer_size {
        return Status::Sequence;
    }
    fs::close(&mut ram.file);
    let slot = ram.xfer_slot;
    let tmp = slot_path(slot, true);
    let s = match verify(ram, &tmp) {
        Status::Ok => {
            fs::unlink(&slot_path(slot, false));
            if fs::rename(&tmp, &slot_path(slot, false)) < 0 { Status::Io } else { Status::Ok }
        }
        s => s,
    };
    if s != Status::Ok {
        fs::unlink(&tmp);
    }
    ram.xfer_state = 0;
    s
}

// Re-reads the closed file: CRC of all of it, header checks, exact size, CRC of image and relocation table.
fn verify(ram: &mut StubRam, tmp: &[u8]) -> Status {
    if fs::open(&mut ram.file, tmp, fs::READ) < 0 {
        return Status::Io;
    }
    let s = scan(ram);
    fs::close(&mut ram.file);
    s
}

fn scan(ram: &mut StubRam) -> Status {
    let mut raw = [0u8; HEADER_SIZE];
    if !fs::read_exact(&mut ram.file, &mut raw) {
        return Status::Io;
    }
    let (mut all, mut body) = (crc32(0, &raw), 0u32);
    let mut left = ram.xfer_size as usize - HEADER_SIZE;
    while left > 0 {
        let chunk = &mut ram.scratch[..left.min(0x200)];
        if !fs::read_exact(&mut ram.file, chunk) {
            return Status::Io;
        }
        all = crc32(all, chunk);
        body = crc32(body, chunk);
        left -= chunk.len();
    }
    if all != ram.xfer_crc {
        return Status::Crc;
    }
    let Ok(h) = parse_header(&raw) else { return Status::Invalid };
    let exact = HEADER_SIZE as u64 + h.image_size as u64 + 4 * h.reloc_count as u64;
    if check_slot(&h, ram.xfer_slot).is_err() || exact != ram.xfer_size as u64 || body != h.body_crc {
        return Status::Invalid;
    }
    Status::Ok
}

