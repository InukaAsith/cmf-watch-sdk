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
