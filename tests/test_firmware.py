import os
import struct
import unittest
from unittest import skipUnless

from wp2apps import firmware, paths

try:
    STOCK = paths.stock_app()
except (FileNotFoundError, ImportError):
    STOCK = None
HAVE_STOCK = STOCK is not None


def fake_stub(boot=0x1025A0A1, ble=0x1025A0C1, view_proc=0x1025A0E1, view_name=0x1025A100, applist=0, idle=0):
    return b"WP2S" + struct.pack("<IIIIIII", 1, boot, ble, view_proc, view_name, applist, idle) + bytes(0x100)


class FirmwareTest(unittest.TestCase):
    def test_thumb_bl_known_encoding(self):
        self.assertEqual(firmware.thumb_bl(0x100D34F0, 0x100CC284), bytes.fromhex("f8f7c8fe"))

    def test_thumb_range_checked(self):
        with self.assertRaises(ValueError):
            firmware.thumb_bl(0x10000000, 0x12000000)

    def test_thumb_b_w_roundtrip(self):
        self.assertEqual(firmware.thumb_b_w(0x10000000, 0x10000004), bytes.fromhex("00f000b8"))

    @skipUnless(HAVE_STOCK, "needs JX402_V1.0.0.73.bin")
    def test_thumb_bl_matches_stock_encoding(self):
        self.assertEqual(firmware.thumb_bl(0x101884FA, 0x10146AA4), STOCK[0x1884FA:0x1884FE])

    def test_fix_acth_rule(self):
        app = bytearray(b"ACTHHTCA" + bytes(0x1F8) + os.urandom(4096))
        firmware.fix_acth(app)
        size, checksum = struct.unpack_from("<II", app, 0x18)
        self.assertEqual(size, len(app) - 0x200)
        self.assertEqual(firmware.sum32(app[:0x200]), 0xFFFFFFFF)
        self.assertEqual((firmware.sum32(app[0x200:0x200 + size]) + checksum) & 0xFFFFFFFF, 0xFFFFFFFF)

    def test_fix_acth_needs_alignment(self):
        with self.assertRaises(ValueError):
            firmware.fix_acth(bytearray(0x203))

    @skipUnless(HAVE_STOCK, "needs JX402_V1.0.0.73.bin")
    def test_fix_acth_reproduces_stock(self):
        size = struct.unpack_from("<I", STOCK, 0x18)[0]
        app = bytearray(STOCK[:0x200 + size])
        firmware.fix_acth(app)
        self.assertEqual(bytes(app), STOCK[:0x200 + size])

    @skipUnless(HAVE_STOCK, "needs JX402_V1.0.0.73.bin")
    def test_patch_keeps_stock_acth_semantics(self):
        firmware.check_acth(STOCK)
        plain = firmware.patch(STOCK)
        self.assertEqual(plain[:0x1C], STOCK[:0x1C])
        self.assertEqual(plain[0x24:0x200], STOCK[0x24:0x200])
        with_stub = firmware.patch(STOCK, fake_stub())
        firmware.check_acth(with_stub)
        self.assertEqual(struct.unpack_from("<I", with_stub, 0x18)[0], len(with_stub) - 0x200)
        broken = bytearray(with_stub)
        broken[-1] ^= 1
        with self.assertRaises(ValueError):
            firmware.check_acth(broken)

    def test_put_checks_old_bytes(self):
        app = bytearray(b"abcd")
        firmware.put(app, 0x10000000, b"ab", b"xy")
        self.assertEqual(app, b"xycd")
        with self.assertRaisesRegex(ValueError, "0x10000002"):
            firmware.put(app, 0x10000002, b"zz", b"..")

    @skipUnless(HAVE_STOCK, "needs JX402_V1.0.0.73.bin")
    def test_heap_literals_patched(self):
        out = firmware.patch(STOCK)
        for off in firmware.HEAP_LITERALS:
            self.assertEqual(struct.unpack_from("<I", out, off)[0], firmware.HEAP_NEW)
        diff = [i for i in range(0x200, len(STOCK)) if out[i] != STOCK[i]]
        self.assertEqual({i & ~3 for i in diff}, set(firmware.HEAP_LITERALS))

    @skipUnless(HAVE_STOCK, "needs JX402_V1.0.0.73.bin")
    def test_patch_refuses_other_bytes(self):
        bad = bytearray(STOCK)
        bad[0x188508] ^= 1
        with self.assertRaisesRegex(ValueError, "0x10188508"):
            firmware.patch(bytes(bad), _verify=False)
        with self.assertRaisesRegex(ValueError, "sha256"):
            firmware.patch(bytes(bad))

    @skipUnless(HAVE_STOCK, "needs JX402_V1.0.0.73.bin")
    def test_stub_patch_points(self):
        stub = fake_stub(boot=0x1025A0A1, ble=0x1025A0C1, view_proc=0x1025A0E1, view_name=0x1025A100)
        out = firmware.patch(STOCK, stub)
        self.assertEqual(out[0x25A080:0x25A080 + len(stub)], stub)
        self.assertEqual(out[0x1884FA:0x1884FE], firmware.thumb_bl(0x101884FA, 0x1025A0A0))
        self.assertEqual(out[0xD34F0:0xD34F4], firmware.thumb_bl(0x100D34F0, 0x1025A0C0))
        table = struct.unpack_from("<I", out, 0x1018BC24 - 0x10000000)[0]
        end = struct.unpack_from("<I", out, 0x1018BC28 - 0x10000000)[0]
        self.assertEqual(table % 8, 0)
        self.assertEqual((end - table) // 0x18, 163)
        self.assertEqual(out[table - 0x10000000:end - 0x18 - 0x10000000], STOCK[0x4B8:0x13E8])
        last = out[end - 0x18 - 0x10000000:end - 0x10000000]
        self.assertEqual(struct.unpack_from("<IIIIHHHH", last), (0x1025A100, 0x1025A0E1, 0, 0, 0xF0, 0x102, 0x1D2, 0x1D2))
        self.assertEqual(struct.unpack_from("<I", out, 0x1018DF3C - 0x10000000)[0], table)
        self.assertEqual(struct.unpack_from("<I", out, 0x1018DF40 - 0x10000000)[0], end)
        # neighbouring tables that merely share a bound with the view table (docs/recon.md)
        for lit in (0x101461AC, 0x10055458):
            self.assertEqual(out[lit - 0x10000000:lit - 0x10000000 + 4], STOCK[lit - 0x10000000:lit - 0x10000000 + 4])

    @skipUnless(HAVE_STOCK, "needs JX402_V1.0.0.73.bin")
    def test_applist_patch(self):
        out = firmware.patch(STOCK, fake_stub(applist=0x1025A121))
        desc = 0x101EB154 - 0x10000000
        self.assertEqual(struct.unpack_from("<I", out, desc + 0xC)[0], 0x1025A121)
        self.assertEqual(out[desc:desc + 0xC] + out[desc + 0x10:desc + 0x30],
                         STOCK[desc:desc + 0xC] + STOCK[desc + 0x10:desc + 0x30])
        base = firmware.patch(STOCK, fake_stub())
        self.assertEqual(base[desc:desc + 0x30], STOCK[desc:desc + 0x30])
        diff = {i & ~3 for i in range(0x200, 0x25A080) if out[i] != base[i]}
        self.assertEqual(diff, {desc + 0xC})

    @skipUnless(HAVE_STOCK, "needs JX402_V1.0.0.73.bin")
    def test_ble_idle_patch(self):
        out = firmware.patch(STOCK, fake_stub(idle=0x1025A141))
        at = firmware.BLE_IDLE_CALL - 0x10000000
        self.assertEqual(STOCK[at:at + 4], firmware.thumb_bl(firmware.BLE_IDLE_CALL, firmware.STOCK_OS_SLEEP))
        self.assertEqual(out[at:at + 4], firmware.thumb_bl(firmware.BLE_IDLE_CALL, 0x1025A140))
        base = firmware.patch(STOCK, fake_stub())
        self.assertEqual({i & ~3 for i in range(0x200, 0x25A080) if out[i] != base[i]}, {at & ~3})
        with self.assertRaisesRegex(ValueError, "stub"):
            firmware.patch(STOCK, fake_stub(idle=0x1025A140))

    @skipUnless(HAVE_STOCK, "needs JX402_V1.0.0.73.bin")
    def test_applist_needs_thumb(self):
        with self.assertRaisesRegex(ValueError, "stub"):
            firmware.patch(STOCK, fake_stub(applist=0x1025A120))

    @skipUnless(HAVE_STOCK, "needs JX402_V1.0.0.73.bin")
    def test_stub_too_big_rejected(self):
        with self.assertRaises(ValueError):
            firmware.patch(STOCK, fake_stub() + bytes(0x4000))

    @skipUnless(HAVE_STOCK, "needs JX402_V1.0.0.73.bin")
    def test_stub_header_checked(self):
        with self.assertRaisesRegex(ValueError, "stub"):
            firmware.patch(STOCK, b"XXXX" + fake_stub()[4:])
        with self.assertRaisesRegex(ValueError, "stub"):
            firmware.patch(STOCK, fake_stub(boot=0x1025A0A0))
        for bad in (dict(ble=0x1025A0C0), dict(view_proc=0x10000001), dict(view_name=0x1025A1A0),
                    dict(applist=0x1025A081 + 0x4000)):
            with self.subTest(**bad), self.assertRaisesRegex(ValueError, "stub"):
                firmware.patch(STOCK, fake_stub(**bad))

    def test_heap_shrinks_by_stub_ram_only(self):
        self.assertEqual(firmware.HEAP_OLD - firmware.HEAP_NEW, 0x1000)
        self.assertEqual(firmware.STUB_RAM, 0x3871EEC0)
        self.assertEqual(firmware.STUB_RAM + firmware.STUB_RAM_SIZE, 0x380A9340 + 0x676B80)

    def test_stage_nor(self):
        nor = firmware.stage_nor(b"\0" * 0x400000, b"\1" * 16)
        self.assertEqual(nor[0x24000:0x24010], b"\1" * 16)
        self.assertEqual(nor[0x24010:0x24000 + 0x34B000], b"\xff" * (0x34B000 - 16))
        self.assertEqual(len(nor), 0x400000)

    def test_stage_nor_too_big(self):
        with self.assertRaises(ValueError):
            firmware.stage_nor(b"\0" * 0x400000, b"\1" * 0x34B001)


if __name__ == "__main__":
    unittest.main()
