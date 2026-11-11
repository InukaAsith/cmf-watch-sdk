import hashlib
import struct

STOCK_SHA256_PREFIX = "30e7978c39d8fb79"
STUB_OFFSET = 0x25A080
HEAP_LITERALS = (0x188508, 0x188588, 0x1885F4)
HEAP_OLD = 0x676B80
HEAP_BASE = 0x380A9340
HEAP_NEW = 0x675B80
STUB_RAM = HEAP_BASE + HEAP_NEW
STUB_RAM_SIZE = 0x1000
APP_VBASE = 0x10000000
NOR_APP_OFFSET = 0x24000
NOR_APP_SIZE = 0x34B000
STUB_MAX = 0x4000
STUB_HEADER = struct.Struct("<4sIIIIIII")
VIEW_TABLE, VIEW_TABLE_END = 0x100004B8, 0x100013E8
# 0x101461AC and 0x10055458 hold the same values but bound neighbouring tables (docs/recon.md)
VIEW_START_LITERALS = (0x1018BC24, 0x1018DF3C)
VIEW_END_LITERALS = (0x1018BC28, 0x1018DF40)
VIEW_ID = 0xF0
BOOT_CALL = 0x101884FA
STOCK_HEAP_INIT = 0x10146AA4
BLE_CALL = 0x100D34F0
STOCK_BLE_READ = 0x100CC284
# list style descriptor +0xC: its layout; waterfall (0x101EB184) is not covered
APPLIST_LAYOUT = 0x101EB160
STOCK_APPLIST_LAYOUT = 0x1005EEE5
# the only stock call that switches the motor on (docs/recon.md); 0x100FB168 and 0x100FB2A8 pass pulse 0
MOTOR_CALL = 0x100FB398
STOCK_MOTOR = 0x100FB4B4


def sum32(data: bytes) -> int:
    data = bytes(data) + bytes(-len(data) % 4)
    return sum(struct.unpack(f"<{len(data) // 4}I", data)) & 0xFFFFFFFF


def fix_acth(app: bytearray, size: int | None = None) -> None:
    if len(app) % 4:
        raise ValueError("app length must be a multiple of 4")
    size = len(app) - 0x200 if size is None else size
    struct.pack_into("<I", app, 0x18, size)
    struct.pack_into("<I", app, 0x1C, 0)
    struct.pack_into("<I", app, 0x20, 0)
    struct.pack_into("<I", app, 0x1C, (0xFFFFFFFF - sum32(app[0x200:0x200 + size])) & 0xFFFFFFFF)
    struct.pack_into("<I", app, 0x20, (0xFFFFFFFF - sum32(app[:0x200])) & 0xFFFFFFFF)


def check_acth(app: bytes, max_len: int = NOR_APP_SIZE) -> None:
    # app_image_check() of the Actions SDK bootloader (header at 0, vectors at 0x200)
    size, checksum = struct.unpack_from("<II", app, 0x18)
    hdr_size, flags = struct.unpack_from("<H", app, 0x24)[0], struct.unpack_from("<I", app, 0x2C)[0]
    reset = struct.unpack_from("<I", app, 0x204)[0]
    if (app[:8] != b"ACTHHTCA" or hdr_size != 0x200 or flags or not reset & 1
            or not APP_VBASE <= reset <= APP_VBASE + max_len or not 0 < size <= len(app) - 0x200
            or size % 4 or len(app) > max_len or sum32(app[:0x200]) != 0xFFFFFFFF
            or (sum32(app[0x200:0x200 + size]) + checksum) & 0xFFFFFFFF != 0xFFFFFFFF):
        raise ValueError("ACTH header fails the bootloader image check")


def _thumb_branch(at: int, target: int, second: int) -> bytes:
    off = target - (at + 4)
    if off % 2 or not -(1 << 24) <= off < 1 << 24:
        raise ValueError(f"branch {at:#x} -> {target:#x} is out of range")
    s = (off >> 24) & 1
    i1, i2 = (off >> 23) & 1, (off >> 22) & 1
    j1, j2 = (i1 ^ 1) ^ s, (i2 ^ 1) ^ s
    hw1 = 0xF000 | s << 10 | (off >> 12) & 0x3FF
    hw2 = second | j1 << 13 | j2 << 11 | (off >> 1) & 0x7FF
    return struct.pack("<HH", hw1, hw2)


def thumb_bl(at: int, target: int) -> bytes:
    return _thumb_branch(at, target, 0xD000)


def thumb_b_w(at: int, target: int) -> bytes:
    return _thumb_branch(at, target, 0x9000)


def put(app: bytearray, addr: int, old: bytes, new: bytes) -> None:
    off = addr - APP_VBASE
    if bytes(app[off:off + len(old)]) != old:
        raise ValueError(f"unexpected bytes at {addr:#x}")
    app[off:off + len(new)] = new


def patch(app: bytes, stub: bytes | None = None, *, _verify: bool = True) -> bytes:
    if _verify and not hashlib.sha256(app).hexdigest().startswith(STOCK_SHA256_PREFIX):
        raise ValueError("not the stock JX402 1.0.0.73 app.bin (sha256 mismatch)")
    out = bytearray(app)
    stock_size = struct.unpack_from("<I", out, 0x18)[0]
    for off in HEAP_LITERALS:
        put(out, APP_VBASE + off, struct.pack("<I", HEAP_OLD), struct.pack("<I", HEAP_NEW))
    if stub is not None:
        add_stub(out, stub)
    out += bytes(-len(out) % 4)
    # stock size stops before the appended ksdfs tail; appended code must be covered
    fix_acth(out, None if stub is not None else stock_size)
    check_acth(out)
    return bytes(out)


def add_stub(out: bytearray, stub: bytes) -> None:
    magic, version, boot, ble, view_proc, view_name, applist, motor = STUB_HEADER.unpack_from(stub)
    base = APP_VBASE + STUB_OFFSET
    code = (boot, view_proc) + tuple(p for p in (ble, applist, motor) if p)
    if (magic != b"WP2S" or version != 1 or not all(p & 1 for p in code)
            or not all(base <= p & ~1 < base + len(stub) for p in (*code, view_name))):
        raise ValueError("bad stub header")
    if len(out) != STUB_OFFSET:
        raise ValueError(f"app.bin is {len(out):#x} bytes; the stub goes at {STUB_OFFSET:#x}")
    out += stub
    out += bytes(-len(out) % 8)
    table = APP_VBASE + len(out)
    out += out[VIEW_TABLE - APP_VBASE:VIEW_TABLE_END - APP_VBASE]
    out += struct.pack("<IIIIHHHH", view_name, view_proc, 0, 0, VIEW_ID, 0x102, 466, 466)
    if len(out) - STUB_OFFSET > STUB_MAX:
        raise ValueError(f"stub and view table take {len(out) - STUB_OFFSET:#x} bytes; the limit is {STUB_MAX:#x}")
    for lit in VIEW_START_LITERALS:
        put(out, lit, struct.pack("<I", VIEW_TABLE), struct.pack("<I", table))
    for lit in VIEW_END_LITERALS:
        put(out, lit, struct.pack("<I", VIEW_TABLE_END), struct.pack("<I", APP_VBASE + len(out)))
    put(out, BOOT_CALL, thumb_bl(BOOT_CALL, STOCK_HEAP_INIT), thumb_bl(BOOT_CALL, boot & ~1))
    if ble:
        put(out, BLE_CALL, thumb_bl(BLE_CALL, STOCK_BLE_READ), thumb_bl(BLE_CALL, ble & ~1))
    if applist:
        put(out, APPLIST_LAYOUT, struct.pack("<I", STOCK_APPLIST_LAYOUT), struct.pack("<I", applist))
    if motor:
        put(out, MOTOR_CALL, thumb_bl(MOTOR_CALL, STOCK_MOTOR), thumb_bl(MOTOR_CALL, motor & ~1))


def stage_nor(nor: bytes, app: bytes) -> bytes:
    if len(app) > NOR_APP_SIZE:
        raise ValueError(f"app is {len(app)} bytes; the partition holds {NOR_APP_SIZE}")
    out = bytearray(nor)
    out[NOR_APP_OFFSET:NOR_APP_OFFSET + NOR_APP_SIZE] = app + b"\xff" * (NOR_APP_SIZE - len(app))
    return bytes(out)
