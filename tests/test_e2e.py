import struct
import unittest
from unittest import skipUnless

from tests.e2e import harness
from wp2apps import firmware

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
