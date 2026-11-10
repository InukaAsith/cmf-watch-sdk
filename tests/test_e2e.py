import unittest
from unittest import skipUnless

from tests.e2e import harness
from wp2apps import firmware, paths

MARK = bytes.fromhex("a55ac33c")
START, END = firmware.STUB_RAM, firmware.STUB_RAM + firmware.STUB_RAM_SIZE


@skipUnless(harness.E2E, "set WP2_E2E=1 (needs the emulator and gdb)")
class ReservationTest(unittest.TestCase):
    def test_reservation_survives_ui_walk(self):
        nor = firmware.stage_nor(harness.stock_nor(), firmware.patch(paths.stock_app()))
        with harness.Emu("reserve", nor=nor, fixtures=harness.heavy_face_fixtures()) as emu:
            emu.wait_uart("launcher enter", 60)
            emu.keep_awake()
            emu.poke(START, MARK * ((END - START) // 4))
            harness.walk(emu, seconds=240)
            uart = emu.uart()
            self.assertEqual(uart.count("*** Booting Zephyr"), 1, "guest rebooted")
            self.assertEqual(emu.peek(START, END - START), MARK * ((END - START) // 4))
            self.assertNotIn("MllcFailedHook", uart)
            self.assertNotIn("ZEPHYR FATAL ERROR", uart)


def patched_nor():
    return firmware.stage_nor(harness.stock_nor(), firmware.patch(paths.stock_app()))


@skipUnless(harness.E2E, "set WP2_E2E=1 (needs the emulator and gdb)")
class HeapTest(unittest.TestCase):
    def test_walk_heap_is_consistent(self):
        # 0x1018858D (HEAP_DUMP) is the stock failure path: it dumps, saves and panics, so it is not called
        with harness.Emu("heap-walk", nor=patched_nor()) as emu:
            emu.wait_uart("launcher enter", 60)
            emu.keep_awake()
            harness.walk(emu, seconds=60)
            free, biggest = harness.heap(emu)
            self.assertLessEqual(biggest, free)
            size = biggest - 0x200
            p = emu.call(harness.UI_MEM_ALIGNED_ALLOC, 2, 8, size, 0)
            self.assertTrue(0x380A9340 <= p < firmware.STUB_RAM, hex(p))
            emu.call(harness.UI_MEM_FREE, 2, p)
            self.assertEqual(harness.heap(emu)[0], free)
            self.assertNotIn("ZEPHYR FATAL ERROR", emu.uart())

    def test_alloc_free_roundtrip(self):
        with harness.Emu("heap-alloc", nor=patched_nor()) as emu:
            emu.wait_uart("launcher enter", 60)
            emu.keep_awake()
            free0, _ = harness.settle(emu)
            p = emu.call(harness.UI_MEM_ALIGNED_ALLOC, 2, 8, 0x10000, 0)
            free1, _ = harness.heap(emu)
            print(f"\nJHeap overhead for 0x10000: {free0 - free1 - 0x10000:#x}")
            self.assertTrue(0x380A9340 <= p < firmware.STUB_RAM and p % 8 == 0, hex(p))
            self.assertTrue(0x10000 <= free0 - free1 <= 0x10000 + 8 + 0x100, hex(free0 - free1))
            emu.call(harness.UI_MEM_FREE, 2, p)
            self.assertEqual(harness.heap(emu)[0], free0)
