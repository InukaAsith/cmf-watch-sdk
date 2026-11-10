import asyncio
import json
import os
import struct
import types
import unittest
import zlib
from unittest import skipUnless

from tests.e2e import harness
from wp2apps import cli, firmware, install, link, package, paths

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


def repack(data: bytes, version: int) -> bytes:
    pkg = package.parse(data)
    return package.pack(pkg.image, name=pkg.name, version=version, entry=pkg.entry, kind=pkg.kind, relocs=pkg.relocs)


def with_watch(port, action):
    return asyncio.run(cli.with_watch(types.SimpleNamespace(hci_port=port), action))


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
        minor[7] = 1
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


POPUP_FLAGS = 0x3800FAF4  # bits 0/1: a stock popup ("Paired" after the first connection) is open


def bright_pixels(rgb: bytes, w: int, box) -> int:
    """Grey/white pixels (all channels > 128; the emulator shows 0xFFFF as a0a0a0), not coloured stock icons."""
    x, y, bw, bh = box
    return sum(1 for row in range(y, y + bh) for col in range(x, x + bw)
               if min(rgb[3 * (row * w + col):3 * (row * w + col) + 3]) > 128)


def dismiss_popup(emu):
    """The first connection pops "Paired" over every view and sets auto-sleep to 60 s; the button closes it."""
    if emu.peek(POPUP_FLAGS, 1)[0] & 3:
        emu.send("c 1", "c 0", pause=2)
        emu.keep_awake()
    if emu.peek(POPUP_FLAGS, 1)[0] & 3:
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
            self.assertGreater(bright_pixels(rgb, w, (90, 120, 300, 40)), 50)  # "hello"
            self.assertGreater(bright_pixels(rgb, w, (90, 180, 300, 40)), 50)  # "vibration-lab"
            self.assertEqual(rgb[300 * w * 3:], bytes(len(rgb) - 300 * w * 3), "framebuffer does not cover the face")
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
    STOCK_SAMPLE = ((False, 130, 0x6A), (True, 132, 0x43), (True, 236, 0x44))  # Exercise, row 23, Settings

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
            harness.to_app_list(emu, bottom=False)  # the list with the Apps row is built and unloaded again
            emu.send("c 1", "c 0", pause=3)
            self.assertNotEqual(harness.focused(emu), harness.APPLIST_VIEW)
            uart = emu.uart()
            self.assertNotIn("MllcFailedHook", uart)
            self.assertNotIn("ZEPHYR FATAL ERROR", uart)
            self.assertEqual(uart.count("*** Booting Zephyr"), 1, "guest rebooted")
