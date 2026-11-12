"""kit-gallery (cmf-wp2-ui) in the emulator: every page renders, the crown pages (and scrolls the list first),
taps play the kit's haptic patterns and end with the motor off, PAUSE stops frames, BUTTON exits.
Needs target/{loader,kit-gallery}.wp2app. WP2_NETSIM_INSTANCE=89."""
import struct
import unittest
from unittest import skipUnless

from tests.e2e import harness
from tests.test_demo_e2e import save_png
from wp2apps import paths

START = harness.STUB_RAM
ELF = paths.ROOT / "sdk/target/thumbv7em-none-eabi/release/kit-gallery"
OUT = paths.ROOT / "runs" / "uikit"
PAGES = ["shapes", "progress", "list", "buttons", "text", "motion"]
DOTS = [(233 + (2 * i - 5) * 8, 440) for i in range(6)]  # page_dots in gallery.rs
CROWN_NEXT = "r -80"  # one detent, delta +1
LIST_ROWS, ITEMS = 5, 12  # gallery.rs


def hue(px):
    r, g, b = px
    if max(px) < 40:
        return None
    if g > 1.4 * r and g > 1.4 * b:
        return "green"
    if g > 1.4 * r and b > 1.4 * r:
        return "cyan"
    if r > 1.4 * g and b > 1.4 * g:
        return "magenta"
    if r > 1.4 * b and g > 1.4 * b:
        return "yellow"
    if max(px) - min(px) < 25:
        return "grey"
    return "other"


@skipUnless(harness.E2E, "set WP2_E2E=1 (needs the emulator, netsimd, gdb and llvm-nm)")
class UiGalleryTest(unittest.TestCase):
    def assertDuties(self, log, want, msg):
        """motor.log duties match the pattern up to PWM quantisation (400 logs as 396)."""
        self.assertEqual(len(log), len(want), f"{msg}: {log}")
        for (_, got, _), duty in zip(log, want):
            self.assertLessEqual(abs(got - duty), 10, f"{msg}: {log}")

    def test_pages_haptics_pause_button(self):
        OUT.mkdir(parents=True, exist_ok=True)
        with harness.installed(["loader", "kit-gallery"], name="uikit") as (emu, port):
            harness.open_app(emu, row=0)
            stats_at = harness.u32(emu, START + harness.APP_BLOCK_OFF) + harness.symbol(ELF, "STATS")
            stats = lambda: struct.unpack("<12I", emu.peek(stats_at, 48))  # per page: frames, draw ms

            def frame(name):
                w, h, rgb = emu.frame()
                save_png(OUT / name, w, h, rgb)
                at = lambda x, y: tuple(rgb[(y * w + x) * 3:][:3])
                self.assertEqual(max(range(6), key=lambda i: sum(at(*DOTS[i]))), page, f"{name}: page dot")
                return at

            page = 0
            emu.sleep(1)
            px = frame("0-shapes.png")
            self.assertEqual(hue(px(130, 140)), "cyan", "disc")
            self.assertIsNotNone(hue(px(233, 140 - 30)), "ring")
            self.assertEqual(hue(px(336 + 29, 140)), "magenta", "arc")
            self.assertEqual(hue(px(300, 242)), "green", "pill")
            self.assertIsNone(hue(px(233, 140)), "ring hole")

            page = 1
            emu.send(CROWN_NEXT, pause=1.5)
            a = frame("1-progress.png")
            emu.sleep(1)
            b = frame("1-progress-later.png")
            self.assertNotEqual([a(x, y) for x in range(170, 300) for y in range(206, 254, 4)],
                                [b(x, y) for x in range(170, 300) for y in range(206, 254, 4)], "number moves")
            self.assertEqual(hue(b(240, 233 - 130)), "cyan", "ring on just after 12 o'clock")
            self.assertNotEqual(b(233 - 130, 233), (0, 0, 0), "track at 9 o'clock")

            page = 2
            n = len(emu.motor())
            emu.send(CROWN_NEXT, pause=1)
            y = 100 + 56 + 28  # row 1
            emu.send(f"t 1 233 {y}", pause=0)
            px = frame("2-list.png")
            emu.send(f"t 0 233 {y}", pause=0)
            self.assertNotEqual(px(150, y - 20), (0, 0, 0), "row highlight")
            emu.sleep(1)
            tick = emu.motor()[n:]
            print(f"\nlist tap motor.log {tick}")
            self.assertDuties(tick, [250, 0], "tick")
            self.assertLessEqual(tick[1][0] - tick[0][0], 60)
            emu.send(*[CROWN_NEXT] * (ITEMS - LIST_ROWS), pause=1)
            frame("2-list-scrolled.png")

            page = 3
            emu.send(CROWN_NEXT, pause=1)
            px = frame("3-buttons.png")
            self.assertEqual(hue(px(285, 134)), "green", "toggle on")
            for label, y, want in [("double", 250 + 25, [400, 0, 400, 0]), ("alert", 314 + 25, [1000, 0] * 3)]:
                n = len(emu.motor())
                emu.send(f"t 1 233 {y}", f"t 0 233 {y}", pause=1.5)
                buzz = emu.motor()[n:]
                print(f"{label} motor.log {buzz}")
                self.assertDuties(buzz, want, label)
                self.assertLess(buzz[-1][0] - buzz[0][0], 2000)
            frame("3-buttons-after.png")

            page = 4
            emu.send(CROWN_NEXT, pause=1)
            px = frame("4-text.png")
            self.assertTrue(any(hue(px(x, 296)) == "yellow" for x in range(150, 320)), "big clock")
            self.assertTrue(any(px(x, y) != (0, 0, 0) for x in range(60, 400) for y in range(84, 100)), "paragraph")

            page = 5
            emu.send(CROWN_NEXT, pause=1)
            a = frame("5-motion.png")
            emu.sleep(0.5)
            b = frame("5-motion-later.png")
            self.assertNotEqual([a(x, 250) for x in range(100, 366)], [b(x, 250) for x in range(100, 366)], "dots move")

            s = stats()
            for i, name in enumerate(PAGES):
                frames, draw = s[2 * i:2 * i + 2]
                self.assertGreater(frames, 0, name)
                print(f"{name}: {frames} frames, draw {draw / frames:.1f} ms")

            harness.popup(emu)
            emu.sleep(1)
            paused = sum(stats()[::2])
            emu.sleep(1)
            self.assertEqual(sum(stats()[::2]), paused, "frames while paused")
            harness.close_popup(emu)
            emu.sleep(1.5)
            self.assertGreater(sum(stats()[::2]), paused, "no frames after RESUME")

            emu.send("c 1", "c 0", pause=2)  # BUTTON: main returns 0
            self.assertEqual(emu.peek(START + 6, 1), b"\xff")
            self.assertEqual(emu.motor()[-1][1], 0)
            uart = emu.uart()
            self.assertNotIn("MllcFailedHook", uart)
            self.assertNotIn("ZEPHYR FATAL ERROR", uart)
            self.assertEqual(uart.count("*** Booting Zephyr"), 1, "guest rebooted")


if __name__ == "__main__":
    unittest.main()
