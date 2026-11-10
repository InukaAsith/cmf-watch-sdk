import asyncio
import hashlib
import os
import struct
import time
import zlib

from Crypto.Cipher import AES
from Crypto.Util.Padding import pad, unpad

IV = bytes.fromhex("5051525354555657606162636465665A")
HEAD = 0xF5
HEADER = struct.Struct(">BHHHHH")
SHELL_W = "77d4ff01-2fe2-2334-0d35-9ccd078f529c"
SHELL_N = "77d4ff02-2fe2-2334-0d35-9ccd078f529c"
CMD_W = "0000fff2-0000-1000-8000-00805f9b34fb"
CMD_N = "0000fff1-0000-1000-8000-00805f9b34fb"
SYSTEM = 0xFFFF
# watch→phone frames in the clear declare a length with the CRC but omit it
PLAIN_REPLIES = frozenset({0x0048})


def encrypt(key: bytes, data: bytes) -> bytes:
    body = data + struct.pack("<I", zlib.crc32(data))
    return AES.new(key, AES.MODE_CBC, IV).encrypt(pad(body, 16))


def decrypt(key: bytes, data: bytes) -> bytes:
    if not data or len(data) % 16:
        raise ValueError("ciphertext is not a whole number of blocks")
    try:
        body = unpad(AES.new(key, AES.MODE_CBC, IV).decrypt(data), 16)
    except ValueError as error:
        raise ValueError("bad padding") from error
    if len(body) < 4 or zlib.crc32(body[:-4]) != struct.unpack("<I", body[-4:])[0]:
        raise ValueError("payload CRC mismatch")
    return body[:-4]


def frame(c1: int, c2: int, payload: bytes, key: bytes | None) -> bytes:
    body = encrypt(key, payload) if key else payload + struct.pack("<I", zlib.crc32(payload))
    return HEADER.pack(HEAD, len(body), c1, 1, 1, c2) + body


class Parser:
    def __init__(self):
        self.buf = b""

    def feed(self, data: bytes) -> list[tuple[int, int, bytes]]:
        self.buf += data
        out = []
        while True:
            start = self.buf.find(bytes([HEAD]))
            if start < 0:
                self.buf = b""
                break
            self.buf = self.buf[start:]
            if len(self.buf) < HEADER.size:
                break
            _, length, c1, _, _, c2 = HEADER.unpack_from(self.buf)
            size = length - 4 if c1 == SYSTEM and c2 in PLAIN_REPLIES else length
            if len(self.buf) < HEADER.size + size:
                break
            out.append((c1, c2, self.buf[HEADER.size:HEADER.size + size]))
            self.buf = self.buf[HEADER.size + size:]
        return out


def authkey(rnd1: bytes, rnd2: bytes, secret: bytes) -> bytes:
    return hashlib.sha256(rnd1 + rnd2 + secret).digest()[:16]


def session_key(nonce: bytes, auth: bytes) -> bytes:
    return hashlib.sha256(nonce[:16] + auth).digest()[:16]


class Session:
    def __init__(self):
        self.gatt = None
        self.key = None
        self.frames = asyncio.Queue()
        self.shell = asyncio.Queue()
        self.parser = Parser()

    async def _open(self, gatt):
        self.gatt = gatt
        await self.gatt.subscribe(CMD_N, lambda data: [self.frames.put_nowait(f) for f in self.parser.feed(data)])
        await self.gatt.subscribe(SHELL_N, self.shell.put_nowait)

    async def at(self, command: str, timeout: float = 6.0) -> bytes:
        await self.gatt.write(SHELL_W, command.encode() + b"\r\n")
        reply = b""
        async with asyncio.timeout(timeout):
            while not reply.rstrip().endswith((b",OK", b"ERROR")):
                reply += await self.shell.get()
        return reply

    async def _exchange(self, c2, payload, expect, key, timeout):
        await self.gatt.write(CMD_W, frame(SYSTEM, c2, payload, key), response=False)
        async with asyncio.timeout(timeout):
            while True:
                c1, got, body = await self.frames.get()
                if c1 == SYSTEM and got == expect:
                    return body

    async def request(self, c1: int, c2: int, payload: bytes, expect: int, timeout: float = 6.0,
                      key: bytes | None = None) -> bytes:
        if c1 != SYSTEM:
            raise ValueError("only system commands are supported")
        key = key or self.key
        return decrypt(key, await self._exchange(c2, payload, expect, key, timeout))

    async def handshake(self, gatt) -> None:
        await self._open(gatt)
        reply = await self.at("AT GETSECRET")
        text = reply.decode(errors="replace").strip()
        if not text.startswith("GETSECRET:") or not text.endswith(",OK"):
            raise ValueError(f"unexpected GETSECRET reply {text!r}")
        secret = bytes.fromhex(text[len("GETSECRET:"):-len(",OK")])
        rnd1 = os.urandom(16)
        reply = await self._exchange(0x8047, rnd1 + hashlib.sha256(rnd1 + secret).digest(), 0x0048, None, 6.0)
        rnd2 = reply[:16]
        if reply[16:48] != hashlib.sha256(rnd2 + secret).digest():
            raise ValueError("watch failed the pairing challenge")
        auth = authkey(rnd1, rnd2, secret)
        await self.request(SYSTEM, 0x8049, b"\xa5" + b"CMF Watch Pro 2", 0x0049, key=auth)
        nonce = await self.request(SYSTEM, 0x804B, b"\xa5", 0x004C, key=auth)
        self.key = session_key(nonce, auth)
        await self.request(SYSTEM, 0x804D, b"\xa5", 0x0004)
        await self.gatt.write(CMD_W, frame(SYSTEM, 0x8004, struct.pack(">ii", int(time.time()), 0), self.key),
                              response=False)
