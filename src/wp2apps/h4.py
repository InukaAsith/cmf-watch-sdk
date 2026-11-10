import asyncio
from contextlib import asynccontextmanager
import struct

BASE = "0000{:04x}-0000-1000-8000-00805f9b34fb"
CID_ATT = 4
ATT_MTU = 185
CCCD = 0x2902
CHARACTERISTIC = 0x2803


def uuid_str(raw: bytes) -> str:
    if len(raw) == 2:
        return BASE.format(int.from_bytes(raw, "little"))
    h = raw[::-1].hex()
    return f"{h[:8]}-{h[8:12]}-{h[12:16]}-{h[16:20]}-{h[20:]}"


def ad_name(data: bytes) -> bytes:
    i = 0
    while i + 1 < len(data) and data[i]:
        if data[i + 1] in (0x08, 0x09):
            return data[i + 2:i + 1 + data[i]]
        i += 1 + data[i]
    return b""


def adv_reports(params: bytes):
    count, i = params[1], 2
    for _ in range(count):
        addr_type, addr, n = params[i + 1], params[i + 2:i + 8], params[i + 8]
        yield addr_type, addr, params[i + 9:i + 9 + n]
        i += 10 + n


class Gatt:
    def __init__(self, reader, writer, mtu: int = ATT_MTU):
        self.reader, self.writer = reader, writer
        self.mtu = mtu  # 23 = the default MTU of a real watch: no Exchange MTU, long writes and split commands
        self.events = asyncio.Queue()
        self.att = asyncio.Queue()
        self.lock = asyncio.Lock()
        self.handle = None
        self.acl_size = 27
        self.chars: dict[str, tuple[int, int]] = {}
        self.decls: list[int] = []
        self.callbacks: dict[int, object] = {}
        self.partial = b""
        self.task = asyncio.create_task(self._read())

    async def _read(self):
        try:
            while True:
                kind = (await self.reader.readexactly(1))[0]
                if kind == 4:
                    code, n = await self.reader.readexactly(2)
                    self.events.put_nowait((code, await self.reader.readexactly(n)))
                elif kind == 2:
                    head, n = struct.unpack("<HH", await self.reader.readexactly(4))
                    self._acl(head, await self.reader.readexactly(n))
                else:
                    raise ConnectionError(f"unexpected H4 packet type {kind}")
        except (asyncio.IncompleteReadError, ConnectionError, asyncio.CancelledError):
            self.events.put_nowait((None, b""))

    def _acl(self, head, data):
        if head >> 12 & 3 == 2:
            self.partial = data
        else:
            self.partial += data
        if len(self.partial) < 4:
            return
        size, cid = struct.unpack_from("<HH", self.partial)
        if len(self.partial) < 4 + size:
            return
        pdu, self.partial = self.partial[4:4 + size], b""
        if cid != CID_ATT or not pdu:
            return
        if pdu[0] in (0x1B, 0x1D):
            callback = self.callbacks.get(struct.unpack_from("<H", pdu, 1)[0])
            if callback:
                callback(pdu[3:])
            if pdu[0] == 0x1D:
                self._l2cap(b"\x1e")
        elif pdu[0] == 0x02:
            self._l2cap(struct.pack("<BH", 0x03, self.mtu))
        else:
            self.att.put_nowait(pdu)

    async def _event(self, match, timeout):
        async with asyncio.timeout(timeout):
            while True:
                code, params = await self.events.get()
                if code is None:
                    raise ConnectionError("HCI transport closed")
                if match(code, params):
                    return params

    async def command(self, opcode: int, params: bytes = b"", timeout: float = 10.0) -> bytes:
        self.writer.write(struct.pack("<BHB", 1, opcode, len(params)) + params)
        await self.writer.drain()

        seen = []

        def mine(code, p):
            ok = (code == 0x0E and struct.unpack_from("<H", p, 1)[0] == opcode) or \
                 (code == 0x0F and struct.unpack_from("<H", p, 2)[0] == opcode)
            if ok:
                seen.append(code)
            return ok

        raw = await self._event(mine, timeout)
        status, rest = (raw[3], raw[4:]) if seen[0] == 0x0E else (raw[0], b"")
        if status:
            raise ConnectionError(f"HCI command {opcode:#06x} failed with status {status:#04x}")
        return rest

    async def connect(self, name: str):
        await self.command(0x0C03)
        await self.command(0x0C01, bytes.fromhex("ffffffffffffffff"))
        await self.command(0x2001, bytes.fromhex("ff00000000000000"))
        size = await self.command(0x2002)
        self.acl_size = struct.unpack_from("<H", size)[0] or 27
        await self.command(0x200B, struct.pack("<BHHBB", 1, 0x10, 0x10, 0, 0))
        await self.command(0x200C, bytes([1, 0]))
        want = name.lower().encode()
        names: dict[bytes, bytes] = {}
        found = None

        def advert(code, params):
            nonlocal found
            if code == 0x3E and params[0] == 0x02:
                for addr_type, addr, data in adv_reports(params):
                    names[addr] = names.get(addr, b"") + ad_name(data)
                    if want in names[addr].lower():
                        found = (addr_type, addr)
                        return True
            return False

        try:
            await self._event(advert, 30.0)
        except TimeoutError:
            raise ConnectionError(f"no advertisement named {name!r}") from None
        await self.command(0x200C, bytes([0, 0]))
        await self.command(0x200D, struct.pack("<HHBB6sBHHHHHH", 0x60, 0x30, 0, found[0], found[1], 0,
                                               0x18, 0x28, 0, 0xC8, 0, 0))
        params = await self._event(lambda c, p: c == 0x3E and p[0] in (1, 10), 30.0)
        if params[1]:
            raise ConnectionError(f"connection failed with status {params[1]:#04x}")
        self.handle = struct.unpack_from("<H", params, 2)[0]
        if self.mtu > 23:
            await self._att(struct.pack("<BH", 0x02, self.mtu))
        start = 1
        while start < 0xFFFF:
            try:
                reply = await self._att(struct.pack("<BHHH", 0x08, start, 0xFFFF, CHARACTERISTIC))
            except LookupError:
                break
            step = reply[1]
            for i in range(2, len(reply), step):
                decl, props, value = struct.unpack_from("<HBH", reply, i)
                self.chars[uuid_str(reply[i + 5:i + step])] = (value, props)
                self.decls.append(decl)
                start = decl + 1

    def _l2cap(self, pdu: bytes):
        data = struct.pack("<HH", len(pdu), CID_ATT) + pdu
        for i in range(0, len(data), self.acl_size):
            flag = 0 if i == 0 else 1
            part = data[i:i + self.acl_size]
            self.writer.write(struct.pack("<BHH", 2, self.handle | flag << 12, len(part)) + part)

    async def _att(self, pdu: bytes, timeout: float = 10.0) -> bytes:
        async with self.lock:
            self._l2cap(pdu)
            await self.writer.drain()
            async with asyncio.timeout(timeout):
                reply = await self.att.get()
        if reply[0] == 0x01:
            if reply[4] == 0x0A:
                raise LookupError("attribute not found")
            raise ConnectionError(f"ATT error {reply[4]:#04x} for opcode {reply[1]:#04x}")
        return reply

    async def write(self, uuid: str, data: bytes, response: bool = True) -> None:
        handle = self.chars[uuid][0]
        room = self.mtu - 3
        if response and len(data) <= room:
            await self._att(struct.pack("<BH", 0x12, handle) + data)
        elif response:  # Prepare Write / Execute Write, as an OS does for a long write
            for i in range(0, len(data), room - 2):
                await self._att(struct.pack("<BHH", 0x16, handle, i) + data[i:i + room - 2])
            await self._att(b"\x18\x01")
        else:  # consecutive Write Commands; the watch reassembles by the frame header length
            for i in range(0, len(data), room):
                self._l2cap(struct.pack("<BH", 0x52, handle) + data[i:i + room])
            await self.writer.drain()

    async def subscribe(self, uuid: str, callback) -> None:
        value = self.chars[uuid][0]
        later = [d for d in self.decls if d > value]
        end = (later[0] - 1) if later else 0xFFFF
        reply = await self._att(struct.pack("<BHH", 0x04, value + 1, end))
        if reply[1] != 1:
            raise ConnectionError("unexpected Find Information format")
        cccd = next((struct.unpack_from("<H", reply, i)[0] for i in range(2, len(reply), 4)
                     if struct.unpack_from("<H", reply, i + 2)[0] == CCCD), None)
        if cccd is None:
            raise ConnectionError(f"{uuid} has no CCCD")
        self.callbacks[value] = callback
        await self._att(struct.pack("<BHH", 0x12, cccd, 1))

    async def close(self):
        try:
            if self.handle is not None:
                self.writer.write(struct.pack("<BHBHB", 1, 0x0406, 3, self.handle, 0x13))
                await self.writer.drain()
        except OSError:
            pass
        self.task.cancel()
        self.writer.close()


@asynccontextmanager
async def gatt_over_h4(port: int, name: str = "cmf", mtu: int = ATT_MTU):
    reader, writer = await asyncio.open_connection("127.0.0.1", port)
    gatt = Gatt(reader, writer, mtu)
    try:
        await gatt.connect(name)
        yield gatt
    finally:
        await gatt.close()
