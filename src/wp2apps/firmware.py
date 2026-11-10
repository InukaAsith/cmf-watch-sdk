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


def sum32(data: bytes) -> int:
    data = bytes(data) + bytes(-len(data) % 4)
    return sum(struct.unpack(f"<{len(data) // 4}I", data)) & 0xFFFFFFFF


def fix_acth(app: bytearray) -> None:
    if len(app) % 4:
        raise ValueError("app length must be a multiple of 4")
    size = len(app) - 0x200
    struct.pack_into("<I", app, 0x18, size)
    struct.pack_into("<I", app, 0x1C, 0)
    struct.pack_into("<I", app, 0x20, 0)
    struct.pack_into("<I", app, 0x1C, (0xFFFFFFFF - sum32(app[0x200:0x200 + size])) & 0xFFFFFFFF)
    struct.pack_into("<I", app, 0x20, (0xFFFFFFFF - sum32(app[:0x200])) & 0xFFFFFFFF)


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
    for off in HEAP_LITERALS:
        put(out, APP_VBASE + off, struct.pack("<I", HEAP_OLD), struct.pack("<I", HEAP_NEW))
    out += bytes(-len(out) % 4)
    fix_acth(out)
    return bytes(out)


def stage_nor(nor: bytes, app: bytes) -> bytes:
    if len(app) > NOR_APP_SIZE:
        raise ValueError(f"app is {len(app)} bytes; the partition holds {NOR_APP_SIZE}")
    out = bytearray(nor)
    out[NOR_APP_OFFSET:NOR_APP_OFFSET + NOR_APP_SIZE] = app + b"\xff" * (NOR_APP_SIZE - len(app))
    return bytes(out)
