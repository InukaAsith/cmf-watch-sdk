"""Back navigation (docs/recon.md "Back navigation"): app -> Custom Apps menu -> stock app list, like stock apps."""
import shutil
import unittest
from unittest import skipUnless

from tests.e2e import harness
from tests.test_e2e import CMU_PWMCLK0, START, bright_pixels, dismiss_popup, same_screen
from wp2apps import paths

OUT = paths.ROOT / "runs" / "back"
CUSTOM_APPS = (170, 108, 200, 40)  # "Custom Apps" label, list scrolled to the top
CLOCK_VIEW = 16


def back(emu):
    """Right swipe with 8 moves, closer to a finger than the 2-move swipe of the harness."""
    harness.swipe(emu, 60, 233, 420, 233, steps=8, pause=3)


def save(emu, name):
    OUT.mkdir(parents=True, exist_ok=True)
    shutil.copy(emu.run / "frame.ppm", OUT / f"{name}.ppm")


def tap_row(emu, row, pause=2):
    y = 180 + 128 * row  # wp2-loader menu.rs
    emu.send(f"t 1 200 {y}", f"t 0 200 {y}", pause=pause)
    if emu.peek(START + 6, 1) == b"\xff":
        raise RuntimeError(f"app in row {row} did not start")


@skipUnless(harness.E2E, "set WP2_E2E=1 (needs the emulator, netsimd and gdb)")
class BackTest(unittest.TestCase):
    def assert_menu(self, emu, mark):
        self.assertEqual(emu.peek(START + 4, 3), b"\x01\x01\xff")  # loader active, no app
        self.assertEqual(harness.u32(emu, START + harness.APP_BLOCK_OFF), 0)
        self.assertEqual(emu.motor()[-1][1], 0)
        self.assertNotIn("view_stack: jump", emu.uart()[mark:])
        self.assertEqual(harness.focused(emu), 240)

    def assert_clean(self, emu):
        uart = emu.uart()
        self.assertNotIn("ZEPHYR FATAL ERROR", uart)
        self.assertNotIn("MllcFailedHook", uart)
        self.assertEqual(uart.count("*** Booting Zephyr"), 1, "guest rebooted")

    def test_back_app_menu_list(self):
        with harness.installed(["loader", "hello", "motor-hog"]) as (emu, port):
            dismiss_popup(emu)
            harness.ready(emu)
            harness.open_apps(emu)
            shutil.copy(emu.run / "list.ppm", OUT / "list-before.ppm")
            self.assertEqual(harness.focused(emu), 240)

            tap_row(emu, 0)  # hello: returns on BUTTON
            mark = len(emu.uart())
            back(emu)
            self.assert_menu(emu, mark)
            save(emu, "hello-back-menu")

            tap_row(emu, 1, pause=0.5)  # motor-hog: never reads events, motor on
            self.assertGreater(emu.motor()[-1][1], 0)
            mark, start = len(emu.uart()), emu.clock()
            harness.swipe(emu, 60, 233, 420, 233, steps=8, pause=0)
            while emu.peek(START + 6, 1) != b"\xff" and emu.clock() - start < 4:
                emu.sleep(0.125)
            print(f"\nback escape after {emu.clock() - start:.3f} s")
            self.assertLessEqual(emu.clock() - start, 3)
            emu.sleep(1)
            self.assert_menu(emu, mark)
            self.assertEqual(harness.u32(emu, CMU_PWMCLK0), 0x303)  # stock PWM clock back
            save(emu, "hog-back-menu")

            mark = len(emu.uart())
            back(emu)
            uart = emu.uart()[mark:]
            self.assertIn("view_stack: jump 240 -> 18", uart)
            self.assertEqual(harness.focused(emu), harness.APPLIST_VIEW)
            self.assertEqual(emu.peek(START + 4, 2), b"\x00\x00")  # loader unloaded
            save(emu, "menu-back-list")
            w, h, rgb = emu.frame()
            self.assertGreater(bright_pixels(rgb, w, CUSTOM_APPS), 50)
            self.assertTrue(same_screen(emu.frame(), harness_frame(OUT / "list-before.ppm")), "list moved")
            self.assert_clean(emu)

    def test_button_unchanged(self):
        with harness.installed(["loader", "hello"]) as (emu, port):
            dismiss_popup(emu)
            harness.ready(emu)
            harness.open_apps(emu)
            tap_row(emu, 0)
            mark = len(emu.uart())
            emu.send("c 1", "c 0", pause=3)  # app: BUTTON, hello returns, menu
            self.assert_menu(emu, mark)
            save(emu, "button-app-menu")
            mark = len(emu.uart())
            emu.send("c 1", "c 0", pause=3)  # menu: stock closes the view, clock
            self.assertIn("view_stack: jump 240 -> 16", emu.uart()[mark:])
            self.assertEqual(harness.focused(emu), CLOCK_VIEW)
            self.assertEqual(emu.peek(START + 4, 2), b"\x00\x00")
            save(emu, "button-menu-clock")
            self.assert_clean(emu)


def harness_frame(path):
    magic, size, maxval, pixels = path.read_bytes().split(b"\n", 3)
    w, h = map(int, size.split())
    return w, h, pixels[:w * h * 3]


if __name__ == "__main__":
    unittest.main()
