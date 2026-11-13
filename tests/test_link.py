import hashlib
import struct
import unittest
import zlib

from wp2apps import link


class LinkTest(unittest.TestCase):
    def test_frame_layout(self):
        f = link.frame(0xFFFF, 0x90A0, b"\x01", key=None)
        self.assertEqual(f[:11], bytes.fromhex("f5") + struct.pack(">HHHHH", 5, 0xFFFF, 1, 1, 0x90A0))
        self.assertEqual(f[11:], b"\x01" + struct.pack("<I", zlib.crc32(b"\x01")))

    def test_encrypt_roundtrip(self):
        key = bytes(range(16))
        body = link.encrypt(key, b"hello")
        self.assertEqual(len(body) % 16, 0)
        self.assertEqual(link.decrypt(key, body), b"hello")

    def test_decrypt_rejects_bad_crc(self):
        key = bytes(range(16))
        with self.assertRaises(ValueError):
            link.decrypt(key, link.encrypt(bytes(16), b"hello"))

    def test_encrypted_frame_length_is_ciphertext(self):
        key = bytes(range(16))
        f = link.frame(0xFFFF, 0x90A2, b"x" * 20, key=key)
        self.assertEqual(struct.unpack_from(">H", f, 1)[0], len(f) - 11)
        self.assertEqual(link.decrypt(key, f[11:]), b"x" * 20)

    def test_parser_plaintext_without_crc(self):
        raw = bytes.fromhex("f5") + struct.pack(">HHHHH", 52, 0xFFFF, 0, 0, 0x0048) + bytes(48)
        self.assertEqual(link.Parser().feed(raw), [(0xFFFF, 0x0048, bytes(48))])

    def test_parser_reassembles_split_notifications(self):
        raw = bytes.fromhex("f5") + struct.pack(">HHHHH", 16, 0xFFFF, 0, 0, 0xB0A0) + bytes(16)
        p = link.Parser()
        self.assertEqual(p.feed(raw[:7]), [])
        self.assertEqual(p.feed(raw[7:20]), [])
        self.assertEqual(p.feed(raw[20:]), [(0xFFFF, 0xB0A0, bytes(16))])

    def test_parser_skips_garbage_before_head(self):
        raw = b"\x00\x01" + bytes.fromhex("f5") + struct.pack(">HHHHH", 16, 0xFFFF, 0, 0, 0xB0A0) + bytes(16)
        self.assertEqual(len(link.Parser().feed(raw)), 1)

    def test_session_key(self):
        self.assertEqual(link.session_key(bytes(16), bytes(16)), hashlib.sha256(bytes(32)).digest()[:16])

    def test_authkey(self):
        r1, r2, s = bytes(16), bytes([1] * 16), bytes([2] * 16)
        self.assertEqual(link.authkey(r1, r2, s), hashlib.sha256(r1 + r2 + s).digest()[:16])


class FakeWatch:
    """Stock handshake: 0x8047 sets the authkey and arms the bind popup, which swallows the next 0x8049;
    `confirm` presses its button then, so the stock clears the authkey and answers with it (0x100D09E6)."""

    def __init__(self, confirm=0, late=0):
        """`late`: taps after 0x804B, so the authkey is zero and the stub stays silent."""
        self.secret, self.confirm, self.auth, self.notify, self.shell = bytes(range(16)), confirm, None, None, None
        self.late = late
        self.popup, self.session = False, None

    async def subscribe(self, uuid, callback):
        if uuid == link.CMD_N:
            self.notify = callback
        if uuid == link.SHELL_N:
            self.shell = callback

    def send(self, c2, plain, key):
        self.notify(link.frame(link.SYSTEM, c2, plain, key))

    async def write(self, uuid, data, response=True):
        if uuid == link.SHELL_W:
            self.shell(f"GETSECRET:{self.secret.hex()},OK".encode())
            return
        c2, body = struct.unpack_from(">H", data, 9)[0], data[11:]
        if c2 == 0x8047:
            rnd1, rnd2 = body[:16], bytes([7] * 16)
            self.auth, self.popup = link.authkey(rnd1, rnd2, self.secret), True
            reply = rnd2 + hashlib.sha256(rnd2 + self.secret).digest()
            self.notify(link.HEADER.pack(link.HEAD, len(reply) + 4, link.SYSTEM, 1, 1, 0x0048) + reply)
            return
        try:
            link.decrypt(self.auth if c2 in (0x8049, 0x804B) else self.session, body)
        except ValueError:
            return  # check_extra_data fault: dropped
        if c2 == 0x8049 and self.popup:
            self.popup = False
            if self.confirm:
                self.confirm -= 1
                self.auth = bytes(16)
                self.send(0x0049, b"\x01", self.auth)
        elif c2 == 0x8049:
            self.send(0x0049, b"\x5a", self.auth)
        elif c2 == 0x804B:
            self.session = link.session_key(bytes([9] * 16), self.auth)
            self.send(0x004C, bytes([9] * 16), self.auth)
        elif c2 == 0x804D:
            self.send(0x0004, b"\x5a", self.session)
            if self.late:
                self.late -= 1
                self.auth = bytes(16)
        elif c2 == link.STUB_INFO and any(self.auth):
            self.send(link.STUB_INFO | 0xA000, b"\x00", self.session)


class HandshakeTest(unittest.IsolatedAsyncioTestCase):
    async def asyncSetUp(self):
        self.sleep = link.asyncio.sleep
        link.asyncio.sleep = lambda _: self.sleep(0)

    async def asyncTearDown(self):
        link.asyncio.sleep = self.sleep

    async def test_plain_handshake(self):
        watch, s = FakeWatch(), link.Session()
        await s.handshake(watch)
        self.assertEqual(s.key, watch.session)
        self.assertNotEqual(watch.auth, bytes(16))

    async def test_confirmed_popup_restarts_bind(self):
        watch, s = FakeWatch(confirm=1), link.Session()
        await s.handshake(watch)
        self.assertEqual(s.key, watch.session)
        self.assertNotEqual(watch.auth, bytes(16))

    async def test_late_tap_rebinds(self):
        watch, s = FakeWatch(late=1), link.Session()
        await s.handshake(watch)
        self.assertEqual(s.key, watch.session)
        self.assertNotEqual(watch.auth, bytes(16))

    async def test_confirmed_every_time_is_an_error(self):
        with self.assertRaisesRegex(ValueError, "bind popup"):
            await link.Session().handshake(FakeWatch(confirm=3))


class ChannelKeyTest(unittest.IsolatedAsyncioTestCase):
    def test_channel_key(self):
        secret = "00112233445566778899aabbccddeeff"
        self.assertEqual(link.channel_key(secret), bytes.fromhex(secret))

    async def test_open_channel_no_bind(self):
        sleep = link.asyncio.sleep
        link.asyncio.sleep = lambda _: sleep(0)
        try:
            watch, s = FakeWatch(), link.Session()
            await s.open_channel(watch)
        finally:
            link.asyncio.sleep = sleep
        self.assertEqual(s.key, link.channel_key(watch.secret.hex()))
        self.assertIsNone(watch.auth)  # no 0x8047
