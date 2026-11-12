import asyncio
import json
import os
import struct
import types
import unittest
import zlib
from unittest import skipUnless

from tests.e2e import harness
from wp2apps import cli, firmware, h4, install, link, package, paths

MARK = bytes.fromhex("a55ac33c")
START, END = firmware.STUB_RAM, firmware.STUB_RAM + firmware.STUB_RAM_SIZE


@skipUnless(harness.E2E, "set WP2_E2E=1 (needs the emulator and gdb)")
class ReservationTest(unittest.TestCase):
    def test_reservation_survives_ui_walk(self):
        with harness.Emu("reserve", nor=harness.patched_nor(), fixtures=harness.heavy_face_fixtures()) as emu:
            emu.wait_uart("launcher enter", 60)
            emu.keep_awake()
            self.assertEqual(harness.u32(emu, harness.HEAP + 4), firmware.HEAP_NEW, "patch is not live in the guest")
            emu.poke(START, MARK * ((END - START) // 4))
            harness.walk(emu)
            uart = emu.uart()
            self.assertEqual(uart.count("*** Booting Zephyr"), 1, "guest rebooted")
            self.assertEqual(emu.peek(START, END - START), MARK * ((END - START) // 4))
            self.assertNotIn("MllcFailedHook", uart)
            self.assertNotIn("ZEPHYR FATAL ERROR", uart)


@skipUnless(harness.E2E, "set WP2_E2E=1 (needs the emulator and gdb)")
class HeapTest(unittest.TestCase):
    # one boot for the class: test_alloc_free_roundtrip (runs first) frees what it allocates
    @classmethod
    def setUpClass(cls):
        cls.emu = cls.enterClassContext(harness.Emu("heap", nor=harness.patched_nor()))
        cls.emu.wait_uart("launcher enter", 60)
        cls.emu.keep_awake()

    def test_walk_heap_is_consistent(self):
        # 0x1018858D (HEAP_DUMP) is the stock failure path: it dumps, saves and panics, so it is not called
        emu = self.emu
        harness.walk(emu)
        free, biggest = harness.settle(emu)
        self.assertLessEqual(biggest, free)
        size = biggest - 0x200
        p = emu.call(harness.UI_MEM_ALIGNED_ALLOC, 2, 8, size, 0)
        self.assertTrue(0x380A9340 <= p < firmware.STUB_RAM, hex(p))
        emu.call(harness.UI_MEM_FREE, 2, p)
        self.assertEqual(harness.heap(emu)[0], free)
        self.assertNotIn("ZEPHYR FATAL ERROR", emu.uart())

    def test_alloc_free_roundtrip(self):
        emu = self.emu
        free0, _ = harness.settle(emu)
        p = emu.call(harness.UI_MEM_ALIGNED_ALLOC, 2, 8, 0x10000, 0)
        free1, _ = harness.heap(emu)
        print(f"\nJHeap overhead for 0x10000: {free0 - free1 - 0x10000:#x}")
        self.assertTrue(0x380A9340 <= p < firmware.STUB_RAM and p % 8 == 0, hex(p))
        self.assertTrue(0x10000 <= free0 - free1 <= 0x10000 + 8 + 0x100, hex(free0 - free1))
        emu.call(harness.UI_MEM_FREE, 2, p)
        self.assertEqual(harness.heap(emu)[0], free0)


def same_screen(a, b) -> bool:
    # ponytail: a ticking clock on the face changes a few digits; a new view repaints most of the screen
    pixels = a[2], b[2]
    return a[:2] == b[:2] and sum(x != y for x, y in zip(*pixels)) < len(pixels[0]) // 20


@skipUnless(harness.E2E, "set WP2_E2E=1 (needs the emulator and gdb)")
class StubTest(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.emu = cls.enterClassContext(harness.Emu("stub", nor=harness.patched_nor()))
        cls.emu.wait_uart("launcher enter", 60)
        cls.emu.keep_awake()

    def test_mkdir_vtable_present(self):
        emu, mp_at, path_at = self.emu, START + 0xE10, START + 0xE20
        emu.poke(path_at, b"/NAND:/wp2apps\0")  # wp2_core::layout::DIR
        self.assertEqual(emu.call(harness.FS_FIND_MOUNT, mp_at, path_at, 0), 0)
        ops = harness.u32(emu, harness.u32(emu, mp_at) + 0x1C)
        self.assertNotEqual(ops, 0)
        self.assertNotEqual(harness.u32(emu, ops + 0x3C), 0)

    def test_weak_vibration_level(self):
        # `wp2apps firmware` default: stock strengths plus level 3 at 350 permille (89/255)
        emu, value = self.emu, START + 0xE30
        duty = {}
        for level in (1, 2, 3):
            n = len(emu.motor())
            emu.poke(value, struct.pack("<i", level))
            emu.call(0x100E17CD, value, 0, 0)  # property 0x25, level preview
            emu.sleep(1)
            duty[level] = [d for _, d, _ in emu.motor()[n:] if d > 0]
        print(f"\nlevel duty {duty}")
        self.assertTrue(duty[1] and all(abs(d - 980) <= 3 for d in duty[1]), duty)
        self.assertEqual(duty[2], [])
        self.assertTrue(duty[3] and all(abs(d - 349) <= 3 for d in duty[3]), duty)

    def test_vibration_roller_has_weak_row(self):
        from tests.test_demo_e2e import save_png
        emu, index = self.emu, 0x38019A8C  # setvib_view's roller index (the crown handler's global)
        emu.call(harness.OPEN_VIEW, 0x56, 0)
        emu.sleep(2)
        save_png(emu.run / "setvib-open.png", *emu.frame())
        self.assertEqual(harness.u32(emu, index), 1)  # Standard

        def step_down():
            # ponytail: one detent at a time until the row changes; crown ticks also pulse the motor at 980
            start = harness.u32(emu, index)
            for _ in range(6):
                emu.send("r -80", pause=1)
                if harness.u32(emu, index) != start:
                    break

        n = len(emu.motor())
        step_down()  # Weak: index 2, level 3
        save_png(emu.run / "setvib-weak.png", *emu.frame())
        duty = [d for _, d, _ in emu.motor()[n:] if d > 0]
        print(f"\nroller index {harness.u32(emu, index)}, preview duty {duty}, frames in {emu.run}")
        self.assertEqual(harness.u32(emu, index), 2)
        self.assertTrue(any(abs(d - 349) <= 3 for d in duty), duty)
        emu.send("t 1 233 398", "t 0 233 398", pause=2)  # the check button saves the level
        self.assertEqual(harness.u32(emu, 0x380077F8 + 0x2C), 3)
        emu.poke(START + 0xE30, struct.pack("<I", 0))
        emu.call(0x100E17F5, START + 0xE30, 0, 0)  # plays pattern *arg at the saved level's strength
        emu.sleep(1)
        played = [d for _, d, _ in emu.motor()[n:] if d > 0][len(duty):]
        self.assertTrue(played and abs(played[-1] - 349) <= 3, played)
        emu.call(harness.OPEN_VIEW, 0x44, 0)  # setlist_view: the Vibration row's subtitle label points at "Weak"
        emu.sleep(2)
        save_png(emu.run / "setlist-weak.png", *emu.frame())
        weak = struct.pack("<I", 0x10259EF8)  # firmware._subtitle_hook text
        self.assertIn(weak, emu.peek(0x38067300, 0x18000))  # GUI heap (scripts/recon/gui_heap_probe.py)
        emu.call(harness.OPEN_VIEW, 0x56, 0)  # reopened, the roller stands on Weak again
        emu.sleep(2)
        save_png(emu.run / "setvib-reopen.png", *emu.frame())
        self.assertEqual(harness.u32(emu, index), 2)
        n = len(emu.motor())
        step_down()  # Off: index 3, level 2, no weak preview
        self.assertEqual(harness.u32(emu, index), 3)
        log = emu.motor()[n - 1:]  # the stop path re-applies the last duty for 0 ms; count pulses that last
        off = [(d, t1 - t0) for (t0, d, _), (t1, _, _) in zip(log, log[1:]) if d > 0 and t1 - t0 >= 50]
        self.assertFalse(any(abs(d - 349) <= 3 for d, _ in off), off)

    def test_patched_boots_and_view_without_loader_fails_cleanly(self):
        emu = self.emu
        self.assertEqual(emu.peek(START, 4), struct.pack("<I", 0x57503253))
        before = emu.frame()
        emu.call(harness.OPEN_VIEW, 0xF0, 0)
        emu.sleep(3)
        trace = emu.peek(START + harness.TRACE_OFF, 4 + 32 * 4)
        self.assertIn((2 << 16) | 0xF0, struct.unpack_from("<32I", trace, 4))
        self.assertEqual(emu.peek(START + 4, 1), b"\x02")  # loader_state: missing
        self.assertEqual(harness.u32(emu, START + harness.LOADER_BLOCK_OFF), 0)
        uart = emu.uart()
        self.assertNotIn("ZEPHYR FATAL ERROR", uart)
        self.assertEqual(uart.count("*** Booting Zephyr"), 1, "guest rebooted")
        self.assertTrue(same_screen(emu.frame(), before), "the stock screen changed")


def assert_stock_vibration_scaled(test, emu, value):
    n = len(emu.motor())
    emu.poke(value, struct.pack("<i", 1))
    emu.call(0x100E17CD, value, 0, 0)  # property 0x25, level 1 preview: stock 250/255, 300 ms
    emu.sleep(1)
    log = emu.motor()
    print(f"\nstock motor.log {log}")
    boot = [d for _, d, _ in log[:n] if d > 0]
    preview = [(d, hz) for _, d, hz in log[n:] if d > 0]
    test.assertTrue(boot and preview)
    test.assertTrue(all(abs(d - 349) <= 2 for d in boot), boot)  # 89/255: `wp2apps firmware --motor 350`
    # level 1 is 250 -> 87/255; level 0 (what a clobbered argument gives) 255 -> 89/255
    test.assertTrue(all(min(abs(d - 341), abs(d - 349)) <= 2 and abs(hz - 31) <= 1 for d, hz in preview), preview)
    test.assertEqual(log[-1][1], 0)


@skipUnless(harness.E2E, "set WP2_E2E=1 (needs the emulator, netsimd and gdb)")
class MotorOnlyTest(unittest.TestCase):
    def test_motor_only_firmware(self):
        app = firmware.patch(paths.stock_app(), motor=350, reserve=False)
        with harness.Emu("motor-only", nor=firmware.stage_nor(harness.stock_nor(), app)) as emu:
            emu.wait_uart("launcher enter", 60)
            emu.keep_awake()
            uart = emu.uart()
            self.assertNotIn("ZEPHYR FATAL ERROR", uart)
            self.assertEqual(uart.count("*** Booting Zephyr"), 1, "guest rebooted")
            assert_stock_vibration_scaled(self, emu, 0x386FF000)  # app area, free without the loader


def repack(data: bytes, version: int) -> bytes:
    pkg = package.parse(data)
    return package.pack(pkg.image, name=pkg.name, version=version, entry=pkg.entry, kind=pkg.kind, relocs=pkg.relocs)


def with_watch(port, action):
    return asyncio.run(cli.with_watch(types.SimpleNamespace(hci_port=port, address=None), action))


@skipUnless(harness.E2E, "set WP2_E2E=1 (needs the emulator, netsimd and gdb)")
class InstallTest(unittest.TestCase):
    # one boot for the class; setUp empties every slot
    @classmethod
    def setUpClass(cls):
        cls.emu, cls.port = cls.enterClassContext(harness.installed([]))
        cls.hello = (paths.ROOT / "target" / "hello.wp2app").read_bytes()

    def args(self, *args):
        return [*args, "--hci-port", self.port]

    def listing(self):
        return json.loads(harness.cli_out(self.args("list")))["slots"]

    def setUp(self):
        for slot in self.listing():
            harness.cli_out(self.args("remove", "--slot", slot))

    def test_install_list_remove(self):
        self.assertEqual(harness.cli(self.args("install", paths.ROOT / "target" / "hello.wp2app")), 0)
        slots = self.listing()
        self.assertEqual(slots["0"]["name"], "hello")
        self.assertEqual(harness.cli(self.args("remove", "--slot", "0")), 0)
        self.assertNotIn("0", self.listing())

    def test_install_rejects_corrupt(self):
        image_byte = bytearray(self.hello)
        image_byte[package.HEADER_SIZE] ^= 1
        reloc_byte = bytearray(self.hello)
        reloc_byte[-1] ^= 1
        minor = bytearray(self.hello)
        minor[7] = 3  # one above LOADER_ABI_MINOR (ABI 1.2)
        struct.pack_into("<I", minor, package.HEADER_CRC_SPAN, zlib.crc32(bytes(minor[:package.HEADER_CRC_SPAN])))
        for name, data in (("image", image_byte), ("relocs", reloc_byte), ("abi_minor", minor)):
            with self.subTest(name):
                with self.assertRaisesRegex(ValueError, "INVALID"):
                    with_watch(self.port, lambda s: install.upload(s, 0, bytes(data)))
                self.assertNotIn("0", self.listing())

    def test_install_interrupted_keeps_old(self):
        v2 = repack(self.hello, 2)
        harness.cli_out(self.args("install", paths.ROOT / "target" / "hello.wp2app"))

        async def half(s):
            await install._call(s, install.BEGIN, struct.pack("<BII", 0, len(v2), zlib.crc32(v2)))
            for offset, part in list(install.chunks(v2))[:len(v2) // 256]:
                await install._call(s, install.DATA, struct.pack("<I", offset) + part)

        with_watch(self.port, half)
        self.assertEqual(self.listing()["0"]["version"], 1)

        async def sequence(s):
            await install._call(s, install.BEGIN, struct.pack("<BII", 0, len(v2), zlib.crc32(v2)))
            reply = await s.request(link.SYSTEM, install.DATA, struct.pack("<I", 8) + b"x", install.DATA | 0xA000)
            self.assertEqual((reply[0], struct.unpack("<I", reply[1:])[0]), (install.Status.SEQUENCE, 0))
            await install.upload(s, 0, v2)

        with_watch(self.port, sequence)
        self.assertEqual(self.listing()["0"]["version"], 2)

    def test_unauthenticated_frame_ignored(self):
        async def probe(s):
            bad = link.frame(link.SYSTEM, install.INFO, b"\0", os.urandom(16))
            await s.gatt.write(link.CMD_W, bad, response=False)
            with self.assertRaises(TimeoutError):
                async with asyncio.timeout(3):
                    while (await s.frames.get())[1] != install.INFO | 0xA000:
                        pass
            self.assertIsNone(await install.info(s, 0))
            self.assertIn(b"GETSECRET:", await s.at("AT GETSECRET"))  # a stock command still answers

        with_watch(self.port, probe)

    def test_unpaired_session_refused(self):
        # both stock keys as after boot (the authkey is also zero after a confirmed bind popup): 0x804B then hands
        # anyone a session key, so the stub must stay silent (docs/recon.md)
        self.emu.poke(AUTHKEY, bytes(16))
        self.emu.poke(SESSION_KEY, bytes(16))

        async def attack(gatt):
            s = link.Session()
            await s.attach(gatt)
            self.assertIsNone(await reply_to(s, install.INFO, b"\xff", link.ZERO_KEY))
            nonce = link.decrypt(link.ZERO_KEY, await s._exchange(0x804B, b"\xa5", 0x004C, link.ZERO_KEY, 6.0))
            self.assertIsNone(await reply_to(s, install.INFO, b"\xff", link.session_key(nonce, link.ZERO_KEY)))
            self.assertIn(b"GETSECRET:", await s.at("AT GETSECRET"))

        asyncio.run(raw_watch(self.port, attack))
        self.assertEqual(self.listing(), {})  # a real handshake still works

    def test_crc_and_data_beyond_size(self):
        data = self.hello

        async def run(s):
            await install._call(s, install.BEGIN, struct.pack("<BII", 0, len(data), zlib.crc32(data) ^ 1))
            for offset, part in install.chunks(data):
                await install._call(s, install.DATA, struct.pack("<I", offset) + part)
            with self.assertRaisesRegex(ValueError, "CRC"):
                await install._call(s, install.END, b"\0")
            await install._call(s, install.BEGIN, struct.pack("<BII", 0, package.HEADER_SIZE, 0))
            with self.assertRaisesRegex(ValueError, "BAD_REQUEST"):
                await install._call(s, install.DATA, struct.pack("<I", 0) + data[:package.HEADER_SIZE + 1])

        with_watch(self.port, run)
        self.assertEqual(self.emu.peek(START + 7, 1), b"\x01")  # xfer_state: the transfer above is still open
        self.assertNotIn("0", self.listing())  # INFO also drops the transfer left open above
        self.assertEqual(self.emu.peek(START + 7, 1), b"\x00")

    def test_loader_slot_while_apps_open(self):
        emu, loader, v2 = self.emu, (paths.ROOT / "target" / "loader.wp2app").read_bytes(), repack(self.hello, 2)
        harness.cli_out(self.args("install", paths.ROOT / "target" / "loader.wp2app"))
        dismiss_popup(emu)

        async def half(s):
            await install._call(s, install.BEGIN, struct.pack("<BII", 0, len(v2), zlib.crc32(v2)))
            await install._call(s, install.DATA, struct.pack("<I", 0) + v2[:install.MAX_DATA])

        def open_apps():
            dismiss_popup(emu)
            emu.call(harness.OPEN_VIEW, 0xF0, 0)
            emu.sleep(3)
            self.assertEqual(emu.peek(START + 4, 2), b"\x01\x01")

        with_watch(self.port, half)  # dropped mid-transfer; Apps opens with the BLE file still open
        ble_file = emu.peek(START + 7, 0x18 - 7 + 0x2C)  # xfer_state .. StubRam.file
        open_apps()
        self.assertEqual(emu.peek(START + 7, len(ble_file)), ble_file, "the loader load touched the BLE transfer")
        self.assertEqual(ble_file[0], 1)

        async def busy(s):  # the stock bind (0x8049) switches to the clock face, so Apps is reopened in-session
            await install.upload(s, 0, v2)
            await asyncio.to_thread(open_apps)
            for c2, payload in ((install.BEGIN, struct.pack("<BII", 0xFF, len(loader), zlib.crc32(loader))),
                                (install.DELETE, b"\xff")):
                with self.assertRaisesRegex(ValueError, "BUSY"):
                    await install._call(s, c2, payload)
            await asyncio.to_thread(emu.send, "t 1 200 140", "t 0 200 140", pause=2)  # hello (v2, slot 0) runs
            self.assertEqual(await asyncio.to_thread(emu.peek, START + 6, 1), b"\x00")
            for c2, payload in ((install.BEGIN, struct.pack("<BII", 0, len(v2), zlib.crc32(v2))), (install.DELETE, b"\x00")):
                with self.assertRaisesRegex(ValueError, "BUSY"):
                    await install._call(s, c2, payload)

        with_watch(self.port, busy)
        for _ in range(4):  # button: stock closes a popup, hello exits, then stock closes the view
            if emu.peek(START + 4, 2) == b"\x00\x00":
                break
            emu.send("c 1", "c 0", pause=2)
        self.assertEqual(emu.peek(START + 4, 2), b"\x00\x00")
        self.assertEqual(self.listing()["0"]["version"], 2)

        # an L.pkg on NAND that END accepts (all CRCs valid) but load rejects: a repeated relocation offset
        pkg = package.parse(loader)
        relocs = (pkg.relocs[0], *pkg.relocs[:-1])
        body = pkg.image + struct.pack(f"<{len(relocs)}I", *relocs)
        bad = bytearray(loader[:package.HEADER_SIZE] + body)
        struct.pack_into("<I", bad, 0x30, zlib.crc32(body))
        struct.pack_into("<I", bad, package.HEADER_CRC_SPAN, zlib.crc32(bytes(bad[:package.HEADER_CRC_SPAN])))
        with_watch(self.port, lambda s: install.upload(s, 0xFF, bytes(bad)))
        dismiss_popup(emu)
        before = emu.frame()
        emu.call(harness.OPEN_VIEW, 0xF0, 0)
        emu.sleep(3)
        self.assertEqual(emu.peek(START + 4, 2), b"\x02\x00")  # corrupt, not active
        self.assertEqual(harness.u32(emu, START + harness.LOADER_BLOCK_OFF), 0)
        self.assertTrue(same_screen(emu.frame(), before), "the stock screen changed")
        uart = emu.uart()
        self.assertNotIn("ZEPHYR FATAL ERROR", uart)
        self.assertEqual(uart.count("*** Booting Zephyr"), 1, "guest rebooted")


AUTHKEY, SESSION_KEY = 0x38048E08, 0x380029A6  # stock key buffers (docs/recon.md)


async def raw_watch(port, action):
    async with h4.gatt_over_h4(port) as gatt:
        return await action(gatt)


async def reply_to(s, c2: int, payload: bytes, key: bytes, timeout: float = 3.0) -> bytes | None:
    await s.gatt.write(link.CMD_W, link.frame(link.SYSTEM, c2, payload, key), response=False)
    try:
        async with asyncio.timeout(timeout):
            while (frame := await s.frames.get())[1] != c2 | 0xA000:
                pass
            return frame[2]
    except TimeoutError:
        return None


POPUP_FLAGS = 0x3800FAF4  # bits 0/1: a stock popup ("Paired" after the first connection) is open


def bright_pixels(rgb: bytes, w: int, box) -> int:
    """Grey/white pixels (all channels > 128; the emulator shows 0xFFFF as a0a0a0), not coloured stock icons."""
    x, y, bw, bh = box
    return sum(1 for row in range(y, y + bh) for col in range(x, x + bw)
               if min(rgb[3 * (row * w + col):3 * (row * w + col) + 3]) > 128)


def dismiss_popup(emu):
    """The first connection pops "Paired" over every view and sets auto-sleep to 60 s; the button closes it."""
    for _ in range(3):  # with the screen already off the stock spends the press on waking it ("drop key workup")
        if not emu.peek(POPUP_FLAGS, 1)[0] & 3:
            return
        emu.send("c 1", "c 0", pause=2)
        emu.keep_awake()
    raise RuntimeError("stock popup is still open")


@skipUnless(harness.E2E, "set WP2_E2E=1 (needs the emulator, netsimd and gdb)")
class LoaderTest(unittest.TestCase):
    def test_menu_lists_installed_apps(self):
        from tests.e2e.netsim import netsim
        nor = harness.patched_paired_nor()
        with netsim() as port, harness.Emu("loader", nor=nor, hci_port=port) as emu:
            emu.wait_uart("launcher enter", 60)
            emu.keep_awake()
            for name in ("loader", "hello", "vibration-lab"):
                self.assertEqual(harness.cli(["install", paths.ROOT / "target" / f"{name}.wp2app", "--hci-port", port]), 0)
            dismiss_popup(emu)
            emu.call(harness.OPEN_VIEW, 0xF0, 0)
            emu.sleep(3)
            self.assertEqual(emu.peek(START + 4, 2), b"\x01\x01")  # loader loaded, active
            block = harness.u32(emu, START + harness.LOADER_BLOCK_OFF)
            self.assertTrue(0x380A9340 <= block < START, hex(block))  # the loader lives in ResFbHeap
            (emu.run / "menu.ppm").write_bytes((emu.run / "frame.ppm").read_bytes())
            w, h, rgb = emu.frame()
            self.assertGreater(bright_pixels(rgb, w, ROW0), 50)  # "hello"
            self.assertGreater(bright_pixels(rgb, w, row_box(1)), 50)  # "vibration-lab"
            # below the last row, inside the round panel (a WP2_GUI run frames it with a grey bezel)
            self.assertFalse(any(pixel(rgb, w, x, y) != (0, 0, 0) for x in range(160, 306) for y in range(380, 430)),
                             "the menu does not cover the face")
            above = (90, 110, 300, 45)  # row 0 above its text
            self.assertEqual(bright_pixels(rgb, w, above), 0)
            emu.send("r -80", pause=1)  # the crown scrolls the list: row 0 moves up
            rgb = emu.frame()[2]
            self.assertGreater(bright_pixels(rgb, w, above), 50)
            emu.send("r 80", "r 80", pause=1.5)  # back to the top (clamped)
            rgb = emu.frame()[2]
            self.assertEqual(bright_pixels(rgb, w, above), 0)
            self.assertGreater(bright_pixels(rgb, w, ROW0), 50)
            emu.send("c 1", "c 0", pause=2)  # button: stock closes the view
            self.assertEqual(emu.peek(START + 4, 2), b"\x00\x00")
            self.assertEqual(harness.u32(emu, START + harness.LOADER_BLOCK_OFF), 0)
            uart = emu.uart()
            self.assertNotIn("ZEPHYR FATAL ERROR", uart)
            self.assertEqual(uart.count("*** Booting Zephyr"), 1, "guest rebooted")

    def test_loader_no_memory_keeps_stock(self):
        with harness.installed(["loader"]) as (emu, port):
            dismiss_popup(emu)
            before = emu.frame()
            for limit in (0x100, 0x60000):  # 1) no block for the loader; 2) loader fits, framebuffer does not
                emu.poke(START + harness.ALLOC_LIMIT_OFF, struct.pack("<I", limit))
                emu.call(harness.OPEN_VIEW, 0xF0, 0)
                emu.sleep(3)
                self.assertEqual(emu.peek(START + 4, 2), b"\x03\x00")  # no memory, not active
                self.assertEqual(harness.u32(emu, START + harness.LOADER_BLOCK_OFF), 0)
                self.assertTrue(same_screen(emu.frame(), before), f"stock screen changed (limit {limit:#x})")
            emu.poke(START + harness.ALLOC_LIMIT_OFF, bytes(4))
            emu.call(harness.OPEN_VIEW, 0xF0, 0)
            emu.sleep(3)
            self.assertEqual(emu.peek(START + 4, 2), b"\x01\x01")
            uart = emu.uart()
            self.assertNotIn("MllcFailedHook", uart)
            self.assertEqual(uart.count("*** Booting Zephyr"), 1)


@skipUnless(harness.E2E, "set WP2_E2E=1 (needs the emulator, netsimd and gdb)")
class AppListTest(unittest.TestCase):
    # (list scrolled to the end, row y, view id from the stock table 0x101EAF40)
    STOCK_SAMPLE = ((False, 232, 0x6A), (True, 236, 0x43), (True, 340, 0x44))  # Exercise, row 23, Settings

    def test_apps_entry_opens_menu(self):
        from tests.e2e.netsim import netsim
        nor = harness.patched_paired_nor()
        with netsim() as port, harness.Emu("applist", nor=nor, hci_port=port) as emu:
            emu.wait_uart("launcher enter", 60)
            emu.keep_awake()
            for name in ("loader", "hello"):
                self.assertEqual(harness.cli(["install", paths.ROOT / "target" / f"{name}.wp2app", "--hci-port", port]), 0)
            dismiss_popup(emu)
            for bottom, y, view in self.STOCK_SAMPLE:
                harness.to_app_list(emu, bottom=bottom)
                mark = len(emu.uart())
                emu.send(f"t 1 233 {y}", f"t 0 233 {y}", pause=3)
                self.assertIn(f"]: view {view} focused", emu.uart()[mark:], f"row at y={y}")
            mark = len(emu.uart())
            harness.open_apps(emu)
            self.assertIn("]: view 240 focused", emu.uart()[mark:])
            self.assertEqual(emu.peek(START + 4, 2), b"\x01\x01")  # loader loaded, active
            emu.send("c 1", "c 0", pause=3)
            self.assertEqual(emu.peek(START + 4, 2), b"\x00\x00")
            harness.to_app_list(emu, bottom=False)  # the list with the Custom Apps row is built and unloaded again
            emu.send("c 1", "c 0", pause=3)
            self.assertNotEqual(harness.focused(emu), harness.APPLIST_VIEW)
            uart = emu.uart()
            self.assertNotIn("MllcFailedHook", uart)
            self.assertNotIn("ZEPHYR FATAL ERROR", uart)
            self.assertEqual(uart.count("*** Booting Zephyr"), 1, "guest rebooted")


def pixel(rgb: bytes, w: int, x: int, y: int) -> tuple[int, int, int]:
    return tuple(rgb[3 * (y * w + x):3 * (y * w + x) + 3])


def row_box(row: int):
    """Text of menu row `row` (wp2-loader menu.rs: rows 120 px from y 120, pitch 128, text from x 101)."""
    return 90, 150 + 128 * row, 300, 50


ROW0 = row_box(0)


@skipUnless(harness.E2E, "set WP2_E2E=1 (needs the emulator, netsimd and gdb)")
class HelloTest(unittest.TestCase):
    def ready(self, emu):
        harness.ready(emu)

    def open_menu(self, emu):
        emu.call(harness.OPEN_VIEW, 0xF0, 0)
        emu.sleep(2)
        self.assertEqual(emu.peek(START + 4, 2), b"\x01\x01")  # loader loaded, active

    def tap_row0(self, emu, pause=2):
        emu.send("t 1 200 140", "t 0 200 140", pause=pause)

    def assert_hello(self, emu, slot=0) -> int:
        self.assertEqual(emu.peek(START + 6, 1), bytes([slot]))  # running_slot
        block = harness.u32(emu, START + harness.APP_BLOCK_OFF)
        self.assertTrue(0x380A9340 <= block < START, hex(block))
        w, h, rgb = emu.frame()
        self.assertEqual(pixel(rgb, w, 233, 100), (0, 0, 0))  # black background
        self.assertGreater(bright_pixels(rgb, w, (213, 225, 60, 20)), 20)  # "hello" near the centre
        return block

    def assert_menu(self, emu):
        self.assertEqual(emu.peek(START + 6, 1), b"\xff")
        self.assertEqual(harness.u32(emu, START + harness.APP_BLOCK_OFF), 0)
        w, h, rgb = emu.frame()
        self.assertGreater(bright_pixels(rgb, w, ROW0), 50)

    def row0_lit(self, emu, more_than: int) -> int:
        """Bright pixels in row 0, polled for up to 1.5 guest s until they exceed `more_than` (frame.ppm lags)."""
        for _ in range(6):
            lit = bright_pixels(emu.frame()[2], 466, ROW0)
            if lit > more_than:
                break
            emu.sleep(0.25)
        return lit

    def assert_clean(self, emu):
        uart = emu.uart()
        self.assertNotIn("MllcFailedHook", uart)
        self.assertNotIn("ZEPHYR FATAL ERROR", uart)
        self.assertEqual(uart.count("*** Booting Zephyr"), 1, "guest rebooted")

    def test_hello_draws_reacts_and_exits(self):
        with harness.installed(["loader", "hello"]) as (emu, port):
            self.ready(emu)
            self.open_menu(emu)
            harness.swipe(emu, 233, 140, 233, 400)  # a drag that starts on row 0 launches nothing
            self.assertEqual(emu.peek(START + 4, 3), b"\x01\x01\xff")
            self.tap_row0(emu)
            self.assert_hello(emu)
            (emu.run / "hello.ppm").write_bytes((emu.run / "frame.ppm").read_bytes())
            emu.send("r 80", pause=1.5)
            w, h, rgb = emu.frame()
            r, g, b = pixel(rgb, w, 233, 100)
            self.assertTrue(r < 16 and g < 16 and b > 128, (r, g, b))  # 0x001F; the emulator shows 0x1F as 160
            emu.send("c 1", "c 0", pause=2)  # BUTTON: main returns 0
            self.assert_menu(emu)
            self.assertEqual(emu.peek(START + 4, 2), b"\x01\x01")  # still in the loader view
            self.assert_clean(emu)

    def test_view_delete_stops_app(self):
        with harness.installed(["loader", "hello"]) as (emu, port):
            self.ready(emu)
            self.open_menu(emu)
            self.tap_row0(emu)
            self.assert_hello(emu)
            emu.call(harness.OPEN_VIEW, 0x10, 0)  # clock view: the stack jumps away from 0xF0
            emu.sleep(3)
            self.assertEqual(emu.peek(START + 4, 3), b"\x00\x00\xff")  # loader_state, loader_active, running_slot
            self.assertEqual(harness.u32(emu, START + harness.APP_BLOCK_OFF), 0)
            self.assertEqual(harness.u32(emu, START + harness.LOADER_BLOCK_OFF), 0)
            self.open_menu(emu)
            self.assert_menu(emu)
            self.tap_row0(emu)
            self.assert_hello(emu)
            harness.swipe(emu, 30, 233, 430, 233, steps=4, pause=3)  # swipe back: BUTTON, the app stops, menu stays
            self.assertEqual(emu.peek(START + 4, 3), b"\x01\x01\xff")
            self.assertEqual(harness.u32(emu, START + harness.APP_BLOCK_OFF), 0)
            self.assert_clean(emu)

    def test_blocks_freed_and_relocated(self):
        with harness.installed(["loader", "hello"]) as (emu, port):
            self.ready(emu)
            frees, blocks, hog, app_size = [], [], 0, 0
            for cycle in range(5):
                self.open_menu(emu)
                loader = harness.u32(emu, START + harness.LOADER_BLOCK_OFF)
                if cycle == 1:  # the old app place, freed with the face when the view opened (docs/recon.md)
                    hog = emu.call(harness.UI_MEM_ALIGNED_ALLOC, 2, 8, app_size, 0)
                self.tap_row0(emu)
                blocks.append((loader, self.assert_hello(emu)))
                app_size = app_size or harness.u32(emu, START + harness.APP_BLOCK_OFF + 4)
                emu.send("c 1", "c 0", pause=2)  # app exits
                self.assert_menu(emu)
                if cycle == 1:
                    emu.call(harness.UI_MEM_FREE, 2, hog)
                emu.send("c 1", "c 0", pause=2)  # stock closes the view
                emu.keep_awake()  # back on the face the launcher re-arms auto-sleep (e2e run 2: asleep 6 s later)
                self.assertEqual(emu.peek(START + 4, 3), b"\x00\x00\xff")
                self.assertEqual(harness.u32(emu, START + harness.LOADER_BLOCK_OFF), 0)
                self.assertEqual(harness.u32(emu, START + harness.APP_BLOCK_OFF), 0)
                frees.append(harness.settle(emu)[0])
            print(f"\nblocks {[(hex(a), hex(b)) for a, b in blocks]} free {[hex(f) for f in frees]}")
            self.assertNotEqual(blocks[1][1], blocks[0][1], "app block did not move")
            steady = [frees[i] for i in (0, 2, 3, 4)]
            self.assertLess(max(steady) - min(steady), 0x1000, [hex(f) for f in frees])
            self.assert_clean(emu)

    def test_app_no_memory_shows_error(self):
        with harness.installed(["loader", "hello"]) as (emu, port):
            self.ready(emu)
            self.open_menu(emu)
            w, h, rgb = emu.frame()
            name = bright_pixels(rgb, w, ROW0)
            emu.poke(START + harness.ALLOC_LIMIT_OFF, struct.pack("<I", 1))
            self.tap_row0(emu, pause=0.5)  # the error shows for 2 s
            self.assertEqual(emu.peek(START + 6, 1), b"\xff")
            self.assertEqual(harness.u32(emu, START + harness.APP_BLOCK_OFF), 0)
            (emu.run / "error.ppm").write_bytes((emu.run / "frame.ppm").read_bytes())
            self.assertGreater(self.row0_lit(emu, name), name)  # "Error -12" is longer than "hello"
            emu.sleep(2.5)
            self.assertEqual(bright_pixels(emu.frame()[2], w, ROW0), name)
            emu.poke(START + harness.ALLOC_LIMIT_OFF, bytes(4))
            self.tap_row0(emu)
            self.assert_hello(emu)
            self.assert_clean(emu)

    def test_big_app_starved_heap(self):
        (paths.ROOT / "target" / "big.wp2app").write_bytes(harness.padded("hello", 0x18000, "big"))
        with harness.installed(["loader", "big"]) as (emu, port):
            self.ready(emu)
            self.open_menu(emu)
            w, h, rgb = emu.frame()
            name = bright_pixels(rgb, w, ROW0)
            hogs = harness.hog(emu, keep=0x10000)
            try:
                self.tap_row0(emu, pause=0.5)
                self.assertEqual(emu.peek(START + 6, 1), b"\xff")
                self.assertEqual(harness.u32(emu, START + harness.APP_BLOCK_OFF), 0)
                self.assertGreater(self.row0_lit(emu, name), name)  # "Error -12"
            finally:
                harness.unhog(emu, hogs)
            emu.sleep(2.5)
            self.tap_row0(emu)
            self.assert_hello(emu)
            emu.send("c 1", "c 0", pause=2)
            self.assert_menu(emu)
            self.assert_clean(emu)


LOADER_ELF = paths.ROOT / "fw/target/thumbv7em-none-eabi/release/wp2-loader"
MOTOR_HOG_ELF = paths.ROOT / "sdk/target/thumbv7em-none-eabi/release/motor-hog"


def app_i32(emu, elf, name) -> int:
    """An i32 static of the running app (its symbol is an image offset)."""
    raw = harness.u32(emu, harness.u32(emu, START + harness.APP_BLOCK_OFF) + harness.symbol(elf, name))
    return struct.unpack("<i", struct.pack("<I", raw))[0]


def without_resets(log):
    """motor.log without the 0 line pin_set_cycles writes before it switches on again in the same ms."""
    return [e for i, e in enumerate(log) if e[1] or i + 1 == len(log) or log[i + 1][0] != e[0] or not log[i + 1][1]]
CMU_PWMCLK0 = 0x40001068
API_MOTOR = 0x18  # Api: abi, abi_minor, width, height (u16), then fill_rect, draw_text, present, next_event, motor


@skipUnless(harness.E2E, "set WP2_E2E=1 (needs the emulator, netsimd, gdb and llvm-nm)")
class MotorTest(unittest.TestCase):
    def assert_stock_clock(self, emu):
        self.assertEqual(harness.u32(emu, CMU_PWMCLK0), 0x303)

    def test_vibration_lab_drives_pwm(self):
        with harness.installed(["loader", "vibration-lab"]) as (emu, port):
            harness.open_app(emu, row=0)
            n = len(emu.motor())
            harness.lab_play(emu)
            log = emu.motor()[n:]
            print(f"\nlab motor.log {log}")
            on = [e for e in log if e[1] > 0]
            self.assertTrue(on)
            ms, duty, hz = on[-1]
            self.assertAlmostEqual(duty, 200, delta=5)  # pulse = period * 200 / 1000, the log rounds
            self.assertAlmostEqual(hz, 31, delta=1)  # lab.rs STOCK_FREQUENCY
            self.assertEqual(log[-1][1], 0)  # the 20 ms tick ended
            emu.send("c 1", "c 0", pause=2)  # BUTTON: the lab exits
            self.assertEqual(emu.peek(START + 6, 1), b"\xff")
            self.assertEqual(emu.motor()[-1][1], 0)
            self.assert_stock_clock(emu)

    def test_motor_cap_without_event_loop(self):
        with harness.installed(["loader", "motor-hog"]) as (emu, port):
            n = len(emu.motor())  # stock boot buzz (1000 30) is not the app's
            harness.open_app(emu, row=0)
            emu.sleep(5)
            log = emu.motor()[n:]
            print(f"\nhog motor.log {log}")
            runs = without_resets(log)  # the hog switches on again every 100 ms
            start = next(m for m, d, hz in runs if d == 1000 and hz == 31)
            stop = next(m for m, d, _ in runs if m > start and d == 0)
            print(f"cap: on at {start} ms, off at {stop} ms ({stop - start} ms)")
            self.assertLessEqual(stop - start, 2000)
            self.assertGreaterEqual(stop - start, 1960)
            self.assertFalse([e for e in log if e[0] > stop and e[1] > 0], "motor came back on")
            self.assertEqual(app_i32(emu, MOTOR_HOG_ELF, "RC"), -4)  # locked until vibrate(0)
            self.assert_stock_clock(emu)
            # the hog still spins at the lowest app priority: BLE (wewear, prio 13) must keep working
            self.assertIn("motor-hog", harness.cli_out(["list", "--hci-port", port]))
            self.assertNotEqual(emu.peek(START + 6, 1), b"\xff")

    def test_button_stops_unresponsive_app(self):
        with harness.installed(["loader", "motor-hog"]) as (emu, port):
            harness.open_app(emu, row=0, pause=0.5)
            self.assertGreater(emu.motor()[-1][1], 0)
            emu.send("c 1", "c 0", pause=0)  # the hog never reads its BUTTON
            start = emu.clock()
            while emu.peek(START + 6, 1) != b"\xff" and emu.clock() - start < 3:
                emu.sleep(0.125)
            print(f"\nbutton escape after {emu.clock() - start:.3f} s")
            self.assertLessEqual(emu.clock() - start, 2)
            self.assertEqual(emu.peek(START + 4, 3), b"\x01\x01\xff")  # back in the menu
            self.assertEqual(harness.u32(emu, START + harness.APP_BLOCK_OFF), 0)
            self.assertEqual(emu.motor()[-1][1], 0)
            self.assert_stock_clock(emu)

    def test_motor_rejects_out_of_range(self):
        with harness.installed(["loader", "hello"]) as (emu, port):
            harness.open_menu(emu)
            block = harness.u32(emu, START + harness.LOADER_BLOCK_OFF)
            motor = harness.u32(emu, block + harness.symbol(LOADER_ELF, "API") + API_MOTOR)
            self.assertTrue(block <= motor < block + 0x10000, hex(motor))
            n = len(emu.motor())
            for duty, hz in ((1001, 31), (500, 0), (500, 20001)):
                self.assertEqual(emu.call(motor, duty, hz), 0xFFFFFFFF, (duty, hz))  # -1
            emu.sleep(0.5)
            self.assertEqual(emu.motor()[n:], [])
            self.assert_stock_clock(emu)


PAUSE, RESUME = 5, 6  # wp2-core events.rs
MSGBOX_POPUP, MSGBOX_CLOSE, BTCALL_MSGBOX = 0x100D5E35, 0x100D6461, 0xF6  # docs/recon.md (task 9, fix-final)
DEFOCUS, FOCUS = 0x800F0, 0x700F0  # StubRam.trace entries: msg << 16 | view


def trace_since(emu, head: int) -> list[int]:
    """StubRam.trace entries the stub proc logged after trace_head was `head` (ring of 32)."""
    raw = emu.peek(START + harness.TRACE_OFF, 4 + 32 * 4)
    now, ring = struct.unpack_from("<I", raw)[0], struct.unpack_from("<32I", raw, 4)
    return [ring[i % 32] for i in range(max(head, now - 32), now)]


def cut_by_loader(test, log, start_hz=31):
    """(start, stop) of the app's first run; the stop is the loader's cut, not the stock popup's own 30 Hz buzz."""
    i = next(i for i, (m, d, hz) in enumerate(log) if d > 0 and hz == start_hz)
    j = next(j for j in range(i + 1, len(log)) if log[j][1] == 0)
    test.assertFalse([e for e in log[i:j] if e[2] == 30], "the stock buzz took the motor before the cut")
    return log[i][0], log[j][0]
LAB_TO_LONG = "r 80"  # knob step -1: Pattern Tick -> Long (runs/e2e/task9-knob.log)


@skipUnless(harness.E2E, "set WP2_E2E=1 (needs the emulator, netsimd, gdb and llvm-nm)")
class PauseTest(unittest.TestCase):
    def test_popup_pauses_and_releases_motor(self):
        with harness.installed(["loader", "motor-hold"]) as (emu, port):
            n = len(emu.motor())
            harness.open_app(emu, row=0, pause=0.25)  # popup well inside the 2 s cap
            self.assertGreater(emu.motor()[-1][1], 0)
            head = harness.u32(emu, START + harness.TRACE_OFF)
            harness.popup(emu)
            emu.sleep(1)
            log = emu.motor()[n:]
            print(f"\nhold motor.log {log}")
            start, stop = cut_by_loader(self, log)
            self.assertLess(stop - start, 1900)  # cut by the pause, not by the cap
            self.assertEqual(app_i32(emu, harness.MOTOR_HOLD_ELF, "PAUSED_RC"), -3)
            self.assertEqual(log[-1][1], 0)
            self.assertEqual(harness.u32(emu, CMU_PWMCLK0), 0x303)
            self.assertEqual(emu.peek(START + 6, 1), b"\x00")  # the app lives on
            self.assertNotEqual(harness.u32(emu, START + harness.APP_BLOCK_OFF), 0)
            harness.close_popup(emu)
            emu.sleep(1.5)
            self.assertEqual(harness.read_marker(emu), [PAUSE, RESUME])
            trace = trace_since(emu, head)
            self.assertIn(DEFOCUS, trace)  # the stub proc gets msg 8 and then 7, not only the flag byte
            self.assertIn(FOCUS, trace[trace.index(DEFOCUS):])
            self.assertEqual(bright_pixels(emu.frame()[2], 466, box=(150, 200, 166, 60)), 166 * 60)
            # the charge popup buzzes at 30 Hz itself; the app's lines are the 31 Hz ones
            self.assertFalse([e for e in emu.motor()[n:] if e[0] > stop and e[2] == 31], "app motor came back")

    def test_lab_stops_measurement_on_pause(self):
        with harness.installed(["loader", "vibration-lab"]) as (emu, port):
            harness.open_app(emu, row=0)
            emu.send("t 1 233 220", "t 0 233 220", pause=1)  # Pattern row
            emu.send(LAB_TO_LONG, pause=1)
            n = len(emu.motor())
            emu.send("t 1 233 360", "t 0 233 360", pause=0)  # PLAY: 1000 ms on
            harness.popup(emu)
            emu.sleep(1)
            log = emu.motor()[n:]
            print(f"\nlab motor.log {log}")
            start, stop = cut_by_loader(self, log)
            self.assertLess(stop - start, 900)  # Long runs 1000 ms
            self.assertEqual(log[-1][1], 0)
            harness.close_popup(emu)
            emu.sleep(2)
            self.assertFalse([e for e in emu.motor()[n:] if e[0] > stop and e[2] == 31], "lab motor came back")
            self.assertNotEqual(emu.peek(START + 6, 1), b"\xff")
            self.assertEqual(harness.u32(emu, CMU_PWMCLK0), 0x303)

    def test_incoming_call_keeps_app(self):
        # stock overlays share msgbox view 62 (notify_view); a message notification (mid 252) needs stock message
        # data, the incoming-call box (mid 246) opens without it (docs/recon.md)
        with harness.installed(["loader", "motor-hold"]) as (emu, port):
            n = len(emu.motor())
            harness.open_app(emu, row=0, pause=0.25)
            head = harness.u32(emu, START + harness.TRACE_OFF)
            emu.call(MSGBOX_POPUP, BTCALL_MSGBOX, 0)
            harness.wait_popup(emu, True)
            emu.sleep(1)
            start, stop = cut_by_loader(self, emu.motor()[n:])
            self.assertLess(stop - start, 1900)
            self.assertEqual(emu.peek(START + 4, 3), b"\x01\x01\x00")  # loader view and app alive
            self.assertEqual(app_i32(emu, harness.MOTOR_HOLD_ELF, "PAUSED_RC"), -3)
            emu.call(MSGBOX_CLOSE, BTCALL_MSGBOX, 0)
            harness.wait_popup(emu, False)
            emu.sleep(1.5)
            self.assertEqual(emu.peek(START + 6, 1), b"\x00")
            self.assertEqual(harness.read_marker(emu), [PAUSE, RESUME])
            trace = trace_since(emu, head)
            self.assertIn(DEFOCUS, trace)
            self.assertIn(FOCUS, trace[trace.index(DEFOCUS):])
            self.assertEqual(bright_pixels(emu.frame()[2], 466, box=(150, 200, 166, 60)), 166 * 60)
            self.assertEqual(harness.u32(emu, CMU_PWMCLK0), 0x303)


L_PKG = b"/NAND:/wp2apps/L.pkg"  # wp2_core::layout::slot_path(LOADER_SLOT)
LAB_UP, LAB_DOWN = "r -80", "r 80"  # one knob step +1 / -1 (LAB_TO_LONG)
LAB_FREQUENCIES = (31, 62, 125, 250, 500, 1000, 4000, 16000)  # lab.rs FREQUENCIES
FACE, SETTINGS = 16, 0x44


def motor_runs(log) -> list[tuple[int, int, int, int]]:
    """(start ms, length ms, duty, Hz) of each switch-on in motor.log lines."""
    runs, on = [], None
    for ms, duty, hz in log:
        if duty and on is None:
            on = (ms, duty, hz)
        elif not duty and on is not None:
            runs.append((on[0], ms - on[0], on[1], on[2]))
            on = None
    return runs


@skipUnless(harness.E2E, "set WP2_E2E=1 (needs the emulator, netsimd and gdb)")
class AcceptanceTest(unittest.TestCase):
    """Stages 2-4 of the design (docs/specs/2026-11-10-wp2-app-loader-design.md), each on a fresh paired boot."""

    def assert_clean(self, emu, boots=1):
        uart = emu.uart()
        self.assertNotIn("MllcFailedHook", uart)
        self.assertNotIn("ZEPHYR FATAL ERROR", uart)
        self.assertEqual(uart.count("*** Booting Zephyr"), boots)

    def open_apps(self, emu) -> int:
        """Taps "Custom Apps" in the stock list; returns the view the stock focused afterwards."""
        mark = len(emu.uart())
        harness.open_apps(emu)
        return 0xF0 if "]: view 240 focused" in emu.uart()[mark:] else harness.focused(emu)

    def assert_apps_work(self, emu, rows=1):
        self.assertEqual(self.open_apps(emu), 0xF0)
        self.assertEqual(emu.peek(START + 4, 2), b"\x01\x01")
        w, h, rgb = emu.frame()
        for row in range(rows):
            self.assertGreater(bright_pixels(rgb, w, row_box(row)), 50, f"menu row {row}")
        emu.send("c 1", "c 0", pause=3)
        self.assertEqual(emu.peek(START + 4, 2), b"\x00\x00")
        self.assertEqual(harness.u32(emu, START + harness.LOADER_BLOCK_OFF), 0)

    def assert_stock_screens(self, emu):
        # a failed preload of 240 leaves the stack on 240 with the list deleted; the next input jumps to the face
        emu.send("c 1", "c 0", pause=3)
        self.assertEqual(harness.focused(emu), FACE)
        harness.to_app_list(emu, bottom=True)
        mark = len(emu.uart())
        emu.send("t 1 233 340", "t 0 233 340", pause=3)  # Settings, the last row
        self.assertIn(f"]: view {SETTINGS} focused", emu.uart()[mark:])
        emu.send("c 1", "c 0", pause=3)
        self.assertEqual(harness.focused(emu), FACE)

    def test_broken_loader_boots_stock(self):
        path = paths.ROOT / "target" / "loader.wp2app"
        loader = path.read_bytes()
        # header CRC (entry byte), then body CRC (last relocation byte); END rejects both, so they are written on NAND
        with harness.installed(["loader"], "acceptance") as (emu, port):
            for boots, (name, offset) in enumerate((("header", 0x28), ("relocs", len(loader) - 1)), start=2):
                with self.subTest(name):
                    harness.ready(emu)
                    harness.write_file(emu, L_PKG, offset, bytes([loader[offset] ^ 1]))
                    harness.reboot(emu, port)
                    self.assert_clean(emu, boots)
                    self.assertNotEqual(self.open_apps(emu), 0xF0)
                    self.assertEqual(emu.peek(START + 4, 2), b"\x02\x00")  # corrupt, not active
                    self.assertEqual(harness.u32(emu, START + harness.LOADER_BLOCK_OFF), 0)
                    self.assert_stock_screens(emu)
                    harness.cli_out(["install", path, "--hci-port", port])  # the fix
                    harness.ready(emu)
                    self.assert_apps_work(emu, rows=0)
            self.assert_clean(emu, 3)

    def test_install_survives_reboot(self):
        with harness.installed(["loader", "hello"], "acceptance") as (emu, port):
            harness.ready(emu)
            harness.reboot(emu, port)
            self.assertEqual(self.open_apps(emu), 0xF0)
            self.assertEqual(emu.peek(START + 4, 2), b"\x01\x01")
            w, h, rgb = emu.frame()
            self.assertGreater(bright_pixels(rgb, w, ROW0), 50)  # "hello"
            emu.send("t 1 200 140", "t 0 200 140", pause=2)
            self.assertEqual(emu.peek(START + 6, 1), b"\x00")  # running_slot
            w, h, rgb = emu.frame()
            self.assertEqual(pixel(rgb, w, 233, 100), (0, 0, 0))
            self.assertGreater(bright_pixels(rgb, w, (213, 225, 60, 20)), 20)  # "hello" drawn by the app
            self.assert_clean(emu, 2)

    def test_loader_update_over_ble(self):
        # v2 differs in its image too (0x100 more zeros), so loader_size tells which one the stub loaded
        pkg = package.parse((paths.ROOT / "target" / "loader.wp2app").read_bytes())
        v2 = package.pack(pkg.image + bytes(0x100), name=pkg.name, version=2, entry=pkg.entry, kind=pkg.kind,
                          relocs=pkg.relocs)
        with harness.installed(["loader", "hello"], "acceptance") as (emu, port):
            listing = lambda: json.loads(harness.cli_out(["list", "--hci-port", port]))["slots"]
            self.assertEqual(listing()["L"]["version"], 1)
            (emu.run / "loader-v2.wp2app").write_bytes(v2)
            harness.cli_out(["install", emu.run / "loader-v2.wp2app", "--hci-port", port])
            self.assertEqual(listing()["L"]["version"], 2)
            harness.ready(emu)
            for _ in range(2):  # a block per opening, freed on close
                self.assertEqual(self.open_apps(emu), 0xF0)
                self.assertEqual(emu.peek(START + 4, 2), b"\x01\x01")
                block, size = struct.unpack("<II", emu.peek(START + harness.LOADER_BLOCK_OFF, 8))
                self.assertTrue(0x380A9340 <= block < START, hex(block))
                self.assertEqual(size, len(pkg.image) + 0x100)
                self.assertGreater(bright_pixels(emu.frame()[2], 466, ROW0), 50)  # "hello"
                emu.send("c 1", "c 0", pause=3)
                self.assertEqual(emu.peek(START + 4, 2), b"\x00\x00")
                self.assertEqual(harness.u32(emu, START + harness.LOADER_BLOCK_OFF), 0)
            self.assert_clean(emu)

    def play(self, emu, seconds=1.0):
        n = len(emu.motor())
        harness.lab_play(emu)
        emu.sleep(seconds)
        runs = motor_runs(emu.motor()[n:])
        self.assertEqual(harness.u32(emu, CMU_PWMCLK0), 0x303)  # back to stock after the stop
        return runs

    def lab_row(self, emu, row):
        y = 127 + 50 * row  # lab rows: Strength, Freq, Pattern, Pulse (vibration-lab main.rs)
        emu.send(f"t 1 233 {y}", f"t 0 233 {y}", pause=0.5)

    def test_vibration_lab_all_knobs(self):
        with harness.installed(["loader", "vibration-lab"], "acceptance") as (emu, port):
            harness.open_app(emu, row=0)
            (emu.run / "lab.ppm").write_bytes((emu.run / "frame.ppm").read_bytes())
            emu.send(LAB_UP, LAB_UP, LAB_UP, pause=0.5)  # Strength 20 % -> 35 %
            [(_, ms, duty, hz)] = self.play(emu)
            self.assertAlmostEqual(duty, 350, delta=5)
            self.assertEqual(hz, 31)
            self.lab_row(emu, 1)
            for i, freq in enumerate(LAB_FREQUENCIES):
                with self.subTest(freq=freq):
                    if i:
                        emu.send(LAB_UP, pause=0.5)
                    [(_, ms, duty, hz)] = self.play(emu)
                    self.assertAlmostEqual(hz, freq, delta=1)
                    self.assertAlmostEqual(duty, 350, delta=5)
            self.lab_row(emu, 2)
            emu.send(LAB_DOWN, pause=0.5)  # Tick -> Long
            n = len(emu.motor())
            emu.send("t 1 233 360", "t 0 233 360", pause=0)
            self.assertNotEqual(harness.u32(emu, CMU_PWMCLK0), 0x303)  # 16 kHz runs on the 32 MHz clock
            emu.sleep(1.5)
            [(_, ms, duty, hz)] = motor_runs(emu.motor()[n:])
            self.assertAlmostEqual(ms, 1000, delta=20)
            self.assertEqual(harness.u32(emu, CMU_PWMCLK0), 0x303)
            emu.send(LAB_DOWN, pause=0.5)  # Long -> Double
            [(s1, ms1, _, _), (s2, ms2, _, _)] = self.play(emu)
            self.assertAlmostEqual(ms1, 20, delta=20)
            self.assertAlmostEqual(s2 - s1 - ms1, 80, delta=20)
            self.assertAlmostEqual(ms2, 20, delta=20)
            emu.send(LAB_DOWN, pause=0.5)  # Double -> Tick
            [(_, ms, _, _)] = self.play(emu)
            self.assertAlmostEqual(ms, 20, delta=20)
            self.lab_row(emu, 3)
            for clicks, edge in ((LAB_DOWN, 5), (LAB_UP, 100)):
                with self.subTest(pulse=edge):
                    emu.send(*[clicks] * 20, pause=0.5)  # past the edge: clamped
                    [(_, ms, _, _)] = self.play(emu)
                    self.assertAlmostEqual(ms, edge, delta=20)
            emu.send("c 1", "c 0", pause=2)
            self.assertEqual(emu.peek(START + 6, 1), b"\xff")
            self.assertEqual(emu.motor()[-1][1], 0)
            self.assert_clean(emu)

    def test_heavy_face_with_apps(self):
        with harness.installed(["loader", "vibration-lab"], "acceptance",
                               fixtures=harness.heavy_face_fixtures()) as (emu, port):
            harness.ready(emu)
            self.assertEqual(self.open_apps(emu), 0xF0)
            emu.send("t 1 200 140", "t 0 200 140", pause=2)
            self.assertEqual(emu.peek(START + 6, 1), b"\x00")  # the lab runs
            emu.send("c 1", "c 0", pause=2)  # the lab exits
            self.assertEqual(emu.peek(START + 6, 1), b"\xff")
            emu.send("c 1", "c 0", pause=3)  # stock closes the view
            self.assertEqual(emu.peek(START + 4, 2), b"\x00\x00")
            emu.keep_awake()
            harness.walk(emu)
            self.assertEqual(harness.u32(emu, START + harness.LOADER_BLOCK_OFF), 0)
            self.assertEqual(harness.u32(emu, START + harness.APP_BLOCK_OFF), 0)
            self.assert_clean(emu)
