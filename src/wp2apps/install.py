from collections.abc import Iterator
from dataclasses import dataclass
import enum
import struct
import zlib

from . import package
from .link import SYSTEM

MAX_DATA = 128
SLOTS = 8
LOADER_SLOT = 0xFF
INFO, BEGIN, DATA, END, DELETE = 0x90A0, 0x90A1, 0x90A2, 0x90A3, 0x90A4


class Status(enum.IntEnum):
    OK = 0
    BAD_REQUEST = 1
    IO = 2
    SEQUENCE = 3
    CRC = 4
    BUSY = 5
    INVALID = 6


@dataclass(frozen=True)
class Header:
    name: str
    version: int
    kind: int
    abi_minor: int


def chunks(data: bytes) -> Iterator[tuple[int, bytes]]:
    for offset in range(0, len(data), MAX_DATA):
        yield offset, data[offset:offset + MAX_DATA]


async def _call(s, c2: int, payload: bytes) -> bytes:
    reply = await s.request(SYSTEM, c2, payload, c2 | 0xA000)
    if not reply:
        raise ValueError("empty reply")
    if reply[0] != Status.OK:
        try:
            name = Status(reply[0]).name
        except ValueError:
            name = f"status {reply[0]}"
        raise ValueError(f"watch refused command {c2:#06x}: {name}")
    return reply[1:]


async def info(s, slot: int) -> Header | None:
    raw = await _call(s, INFO, bytes([slot]))
    if not raw:
        return None
    if len(raw) != package.HEADER_SIZE:
        raise ValueError(f"header is {len(raw)} bytes")
    _, _, kind, abi_minor, name, version, *_ = package.HEADER.unpack(raw)
    return Header(name.rstrip(b"\0").decode("utf-8", "replace"), version, kind, abi_minor)


async def listing(s) -> dict[int, Header]:
    found = {}
    for slot in (*range(SLOTS), LOADER_SLOT):
        header = await info(s, slot)
        if header:
            found[slot] = header
    return found


async def install(s, data: bytes, slot: int | None = None) -> int:
    pkg = package.parse(data)
    if pkg.kind == package.KIND_LOADER:
        slot = LOADER_SLOT
    elif slot is None:
        used = await listing(s)
        same = [n for n, h in used.items() if n < SLOTS and h.name == pkg.name]
        free = [n for n in range(SLOTS) if n not in used]
        if not same and not free:
            raise ValueError("no free slot")
        slot = (same or free)[0]
    await _call(s, BEGIN, struct.pack("<BII", slot, len(data), zlib.crc32(data)))
    for offset, part in chunks(data):
        reply = await _call(s, DATA, struct.pack("<I", offset) + part)
        if struct.unpack("<I", reply)[0] != offset + len(part):
            raise ValueError(f"watch expects offset {struct.unpack('<I', reply)[0]}, sent {offset + len(part)}")
    await _call(s, END, b"")
    return slot


async def remove(s, slot: int) -> None:
    await _call(s, DELETE, bytes([slot]))
