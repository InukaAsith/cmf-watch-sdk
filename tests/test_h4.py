import asyncio
import struct
import unittest

from wp2apps import h4, link

ADDR = bytes([1, 2, 3, 4, 5, 6])
CHARS = {  # uuid, value handle
    link.CMD_W: 0x10, link.CMD_N: 0x12, link.SHELL_W: 0x20, link.SHELL_N: 0x22,
}


def uuid_raw(uuid):
    return bytes.fromhex(uuid.replace("-", ""))[::-1]


class FakeController:
    """Just enough of an HCI controller and ATT server for the client."""

    def __init__(self):
        self.writes = []
        self.writer = None

    def event(self, code, params):
        self.writer.write(bytes([4, code, len(params)]) + params)

    def acl(self, pdu):
        data = struct.pack("<HH", len(pdu), 4) + pdu
        self.writer.write(struct.pack("<BHH", 2, 0x0040, len(data)) + data)

    def complete(self, opcode, ret=b""):
        self.event(0x0E, struct.pack("<BHB", 1, opcode, 0) + ret)

    def command(self, opcode, params):
        if opcode == 0x2002:
            self.complete(opcode, struct.pack("<HB", 251, 8))
        elif opcode == 0x200C and params[0]:
            self.complete(opcode)
            name = b"\x0b\x09CMF Watch 2"
            self.event(0x3E, bytes([2, 1, 0, 0]) + ADDR + bytes([len(name)]) + name + b"\xc0")
        elif opcode == 0x200D:
            self.event(0x0F, struct.pack("<BBH", 0, 1, opcode))
            self.event(0x3E, struct.pack("<BBHB", 1, 0, 0x40, 0) + bytes(17))
        else:
            self.complete(opcode)

    def att(self, pdu):
        op = pdu[0]
        if op == 0x02:
            self.acl(struct.pack("<BH", 3, 185))
        elif op == 0x08:
            start = struct.unpack_from("<H", pdu, 1)[0]
            rows = [(v - 1, 0x1C, v, uuid_raw(u)) for u, v in CHARS.items() if v - 1 >= start]
            if not rows:
                self.acl(bytes([1, 8, pdu[1], pdu[2], 0x0A]))
            else:
                self.acl(bytes([9, 21]) + b"".join(struct.pack("<HBH", d, p, v) + u for d, p, v, u in rows))
        elif op == 0x04:
            value = struct.unpack_from("<H", pdu, 1)[0]
            self.acl(bytes([5, 1]) + struct.pack("<HH", value, h4.CCCD))
        elif op == 0x12:
            self.writes.append(pdu[1:])
            self.acl(bytes([0x13]))
        elif op == 0x03:
            self.writes.append(b"mtu")
        elif op == 0x52:
            self.writes.append(pdu[1:])
            handle = struct.unpack_from("<H", pdu, 1)[0]
            self.acl(struct.pack("<BH", 0x1B, 0x12) + b"hi" if handle == 0x10 else b"\x01\x00\x00\x00\x00")

    async def serve(self, reader, writer):
        self.writer = writer
        while True:
            try:
                kind = (await reader.readexactly(1))[0]
                if kind == 1:
                    opcode, n = struct.unpack("<HB", await reader.readexactly(3))
                    self.command(opcode, await reader.readexactly(n))
                else:
                    _, n = struct.unpack("<HH", await reader.readexactly(4))
                    self.att((await reader.readexactly(n))[4:])
            except asyncio.IncompleteReadError:
                return


class H4Test(unittest.IsolatedAsyncioTestCase):
    async def test_connect_subscribe_write_notify(self):
        fake = FakeController()
        server = await asyncio.start_server(fake.serve, "127.0.0.1", 0)
        port = server.sockets[0].getsockname()[1]
        got = asyncio.Queue()
        async with h4.gatt_over_h4(port, "cmf") as gatt:
            await gatt.subscribe(link.CMD_N, got.put_nowait)
            await gatt.write(link.SHELL_W, b"AT\r\n")
            fake.acl(struct.pack("<BH", 0x02, 247))
            await gatt.write(link.CMD_W, b"x", response=False)
            self.assertEqual(await asyncio.wait_for(got.get(), 2), b"hi")
        self.assertIn(b"mtu", fake.writes)
        self.assertIn(struct.pack("<H", 0x20) + b"AT\r\n", fake.writes)
        self.assertIn(struct.pack("<H", 0x13) + struct.pack("<H", 1), fake.writes)
        server.close()
