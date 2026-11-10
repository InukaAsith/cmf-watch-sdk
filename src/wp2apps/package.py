from dataclasses import dataclass
import struct
import zlib

HEADER = struct.Struct("<4sHBB24sIIIII12s")
HEADER_SIZE = HEADER.size
MAGIC = b"WP2A"
ABI = 1
KIND_APP = 0
KIND_LOADER = 1
APP_BASE = 0x386FF000
APP_SIZE = 0x20000
NAME_BYTES = 23


@dataclass(frozen=True)
class Package:
    kind: int
    abi: int
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
    header = HEADER.pack(MAGIC, ABI, kind, 0, encoded, version, len(image), entry, load_addr,
                         zlib.crc32(image), b"")
    return header + image


def parse(data: bytes) -> Package:
    if len(data) < HEADER_SIZE:
        raise ValueError("package is shorter than its header")
    magic, abi, kind, _, raw_name, version, size, entry, load_addr, crc, _ = HEADER.unpack_from(data)
    if magic != MAGIC:
        raise ValueError("not a WP2 app package")
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
    return Package(kind, abi, name, version, load_addr, entry, image)
