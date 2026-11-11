import struct
import unittest
import zlib
from unittest import skipUnless

from tests.e2e import harness
from wp2apps import firmware, paths

START = firmware.STUB_RAM
ELF = paths.ROOT / "sdk/target/thumbv7em-none-eabi/release/agent-glow"
LOADER_ELF = paths.ROOT / "fw/target/thumbv7em-none-eabi/release/wp2-loader"
FULL = 466 * 466 * 2
OUT = paths.ROOT / "runs" / "agent-glow"
SCENES = ["orb", "thinking", "voice", "orbit"]
DOTS = [(233 + (2 * i - 3) * 8, 440) for i in range(4)]  # page indicator, glow.rs App::frame
CROWN_NEXT = "r -80"  # one detent, delta +1 (test_e2e LAB_UP)


def save_png(path, w, h, rgb):
    raw = b"".join(b"\0" + rgb[y * w * 3:(y + 1) * w * 3] for y in range(h))
    chunk = lambda kind, data: struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data))
    path.write_bytes(b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, 2, 0, 0, 0))
                     + chunk(b"IDAT", zlib.compress(raw)) + chunk(b"IEND", b""))


def scene_of(w, rgb) -> int:
    return max(range(4), key=lambda i: sum(rgb[(DOTS[i][1] * w + DOTS[i][0]) * 3:][:3]))


@skipUnless(harness.E2E, "set WP2_E2E=1 (needs the emulator, netsimd, gdb and llvm-nm)")
class AgentGlowTest(unittest.TestCase):
    def test_scenes_tap_pause_button(self):
        OUT.mkdir(parents=True, exist_ok=True)
        with harness.installed(["loader", "agent-glow"]) as (emu, port):
            harness.open_app(emu, row=0)
            stats_at = harness.u32(emu, START + harness.APP_BLOCK_OFF) + harness.symbol(ELF, "STATS")
            stats = lambda: struct.unpack("<16I", emu.peek(stats_at, 64))  # per scene: frames, draw, period, rects
            flush_at = harness.u32(emu, START + harness.LOADER_BLOCK_OFF) + harness.symbol(LOADER_ELF, "FLUSH_STATS")
            flush = lambda: struct.unpack("<4I", emu.peek(flush_at, 16))  # presents, flushes, bytes, pixels
            f0 = flush()
            n = len(emu.motor())
            for scene, name in enumerate(SCENES):
                if scene:
                    emu.send(CROWN_NEXT, pause=1)
                w, h, a = emu.frame()
                emu.sleep(0.5)
                b = emu.frame()[2]
                self.assertNotEqual(a, b, f"{name}: frame did not change")
                self.assertEqual(scene_of(w, b), scene, f"crown did not switch to {name}")
                emu.send("t 1 233 233", "t 0 233 233", pause=0.25)
                save_png(OUT / f"{scene}-{name}.png", w, h, emu.frame()[2])
                emu.sleep(1.5)
            presents, flushes, nbytes, pixels = (b - a for a, b in zip(f0, flush()))
            print(f"\nflush: {presents} presents, {flushes} flushes, {nbytes / flushes:.0f} B/flush "
                  f"({100 * nbytes / flushes / FULL:.0f}% of full), {pixels / flushes:.0f} px/flush")
            self.assertLess(nbytes / flushes, 0.8 * FULL, "dirty box flush")
            s = stats()
            for scene, name in enumerate(SCENES):
                frames, draw, period, rects = s[4 * scene:4 * scene + 4]
                self.assertGreater(frames, 10, name)
                print(f"\n{name}: {frames} frames, draw {draw / frames:.1f} ms, period {period / max(frames - 1, 1):.1f} ms, "
                      f"{rects / frames:.0f} rects/frame")
            taps = emu.motor()[n:]
            print(f"tap motor.log {taps}")
            on = [i for i, e in enumerate(taps) if e[1] > 0]
            self.assertTrue(on, "no haptic tick")
            for i in on:
                self.assertLess(i + 1, len(taps), "motor left on")
                self.assertEqual(taps[i + 1][1], 0)
                self.assertLessEqual(taps[i + 1][0] - taps[i][0], 60)

            harness.popup(emu)
            emu.sleep(1)
            paused = sum(stats()[::4])
            emu.sleep(1)
            self.assertEqual(sum(stats()[::4]), paused, "frames while paused")
            harness.close_popup(emu)
            emu.sleep(1.5)
            self.assertGreater(sum(stats()[::4]), paused, "no frames after RESUME")
            self.assertEqual(emu.peek(START + 6, 1), b"\x00")

            emu.send("c 1", "c 0", pause=2)  # BUTTON: main returns 0
            self.assertEqual(emu.peek(START + 6, 1), b"\xff")
            self.assertEqual(emu.motor()[-1][1], 0)
            uart = emu.uart()
            self.assertNotIn("MllcFailedHook", uart)
            self.assertNotIn("ZEPHYR FATAL ERROR", uart)
            self.assertEqual(uart.count("*** Booting Zephyr"), 1, "guest rebooted")
