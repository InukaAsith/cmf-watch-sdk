from dataclasses import dataclass
import struct
import zlib

HEADER = struct.Struct("<4sHBB24sIIIIII8s")
HEADER_CRC_SPAN = 0x34
HEADER_SIZE = HEADER.size
MAGIC = b"WP2A"
ABI = 1
ABI_MINOR = 0
KIND_APP = 0
KIND_LOADER = 1
APP_BASE = 0x386FF000
APP_SIZE = 0x20000
NAME_BYTES = 23


@dataclass(frozen=True)
class Package:
    kind: int
    abi: int
    abi_minor: int
    name: str
    version: int
    load_addr: int
    entry: int
    image: bytes


def _check(image: bytes, name: bytes, kind: int, entry: int, load_addr: int) -> None:
    if not 1 <= len(name) <= NAME_BYTES:
        raise ValueError(f"name must be 1..{NAME_BYTES} UTF-8 bytes, got {len(name)}")
    if kind not in (KIND_APP, KIND_LOADER):
        raise ValueError(f"unknown kind {kind}")
    if not 0 <= entry < len(image):
        raise ValueError(f"entry {entry:#x} is outside the {len(image)}-byte image")
    if kind == KIND_APP and load_addr != APP_BASE:
        raise ValueError(f"apps load at {APP_BASE:#x}, not {load_addr:#x}")
    if kind == KIND_APP and len(image) > APP_SIZE:
        raise ValueError(f"image is {len(image)} bytes; the app region holds {APP_SIZE}")


def pack(image: bytes, *, name: str, version: int, entry: int = 0, kind: int = KIND_APP,
         load_addr: int = APP_BASE) -> bytes:
    encoded = name.encode("utf-8")
    _check(image, encoded, kind, entry, load_addr)
    fields = [MAGIC, ABI, kind, ABI_MINOR, encoded, version, len(image), entry, load_addr, zlib.crc32(image), 0, b""]
    fields[10] = zlib.crc32(HEADER.pack(*fields)[:HEADER_CRC_SPAN])
    return HEADER.pack(*fields) + image


def parse(data: bytes) -> Package:
    if len(data) < HEADER_SIZE:
        raise ValueError("package is shorter than its header")
    magic, abi, kind, abi_minor, raw_name, version, size, entry, load_addr, crc, header_crc, reserved = \
        HEADER.unpack_from(data)
    if magic != MAGIC:
        raise ValueError("not a WP2 app package")
    if zlib.crc32(data[:HEADER_CRC_SPAN]) != header_crc:
        raise ValueError("header CRC mismatch")
    if any(reserved):
        raise ValueError("reserved header bytes are not zero")
    if abi != ABI:
        raise ValueError(f"unsupported ABI {abi}")
    image = data[HEADER_SIZE:]
    if size != len(image):
        raise ValueError(f"header says {size} image bytes, package has {len(image)}")
    if zlib.crc32(image) != crc:
        raise ValueError("image CRC mismatch")
    encoded = raw_name.rstrip(b"\0")
    try:
        name = encoded.decode("utf-8")
    except UnicodeDecodeError as error:
        raise ValueError("name is not valid UTF-8") from error
    _check(image, encoded, kind, entry, load_addr)
    return Package(kind, abi, abi_minor, name, version, load_addr, entry, image)


def check_elf(image: bytes, elf: bytes) -> None:
    if elf[:4] != b"\x7fELF":
        raise ValueError("not an ELF file")
    entry, = struct.unpack_from("<I", elf, 0x18)
    shoff, = struct.unpack_from("<I", elf, 0x20)
    shentsize, shnum = struct.unpack_from("<HH", elf, 0x2E)
    end = 0
    for i in range(shnum):
        _, _, flags, addr, _, size = struct.unpack_from("<IIIIII", elf, shoff + i * shentsize)
        if flags & 2:
            end = max(end, addr + size)
    if entry != APP_BASE | 1:
        raise ValueError(f"ELF entry is {entry:#x}; apps must start at {APP_BASE | 1:#x} (link with app.ld)")
    if len(image) != end - APP_BASE:
        raise ValueError(f"image is {len(image)} bytes but the ELF occupies {end - APP_BASE}; .bss missing?")
