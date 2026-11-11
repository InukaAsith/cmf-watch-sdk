import shutil
import struct
import unittest
from unittest import mock, skipUnless

from tests.e2e import harness
from wp2apps import firmware, icon, package, paths

START = firmware.STUB_RAM
APPS = ["vibration-lab", "agent-glow", "api-demo"]
# wp2-loader menu.rs: icon at row x 12, y 12; rows from (16, 120), pitch 128; the row colour, the letter circle
X0, Y0, PITCH, S = 16 + 12, 120 + 12, 128, icon.SIDE
ROW_BG, LETTER_BG = 0x2966, 0x52AB
K = 160 / 255  # the emulator frame shows full white as 160


def rgb(c: int) -> tuple[int, int, int]:
    return (c >> 11) * 255 // 31, (c >> 5 & 63) * 255 // 63, (c & 31) * 255 // 31


def expected(name: str) -> list[tuple[float, float, float]]:
    """The app's icon over the row colour, full-scale RGB."""
    data = package.parse((paths.ROOT / "target" / f"{name}.wp2app").read_bytes()).image
    _, _, px = icon.parse(data)
    bg, out = rgb(ROW_BG), []
    for i in range(S * S):
        c, a = struct.unpack_from("<HB", px, 3 * i)
        out.append(tuple((f * a + b * (255 - a)) / 255 for f, b in zip(rgb(c), bg)))
    return out


def box(frame: bytes, x0: int, y0: int) -> list[tuple[int, int, int]]:
    return [tuple(frame[3 * ((y0 + y) * 466 + x0 + x):3 * ((y0 + y) * 466 + x0 + x) + 3]) for y in range(S) for x in range(S)]


def match(got, want) -> tuple[float, float]:
    """(gain, mean abs error after the gain): the emulator frame shows full white as 160."""
    k = sum(g * w for p, q in zip(got, want) for g, w in zip(p, q)) / sum(w * w for q in want for w in q)
    return k, sum(abs(g - k * w) for p, q in zip(got, want) for g, w in zip(p, q)) / (3 * len(want))


def find(frame: bytes, want) -> tuple[int, float, float]:
    """(y, gain, error) of the best match of `want` in the icon column; a coarse pass on every 4th pixel."""
    idx = range(0, S * S, 4)
    def at(y):
        got = box(frame, X0, y)
        return sum(abs(g - K * w) for i in idx for g, w in zip(got[i], want[i]))
    y = min(range(0, 466 - S + 1), key=at)
    return (y, *match(box(frame, X0, y), want))


def broken(hello: bytes) -> bytes:
    """hello behind a descriptor that claims a 96x96 icon the image is too short for."""
    pkg = package.parse(hello)
    pad = icon.DESC.size
    image = bytearray(icon.DESC.pack(icon.MAGIC, 1, 5, S, S, 0, S * S * 3) + pkg.image)
    relocs = [r + pad for r in pkg.relocs]
    for r in relocs:
        struct.pack_into("<I", image, r, struct.unpack_from("<I", image, r)[0] + pad)
    with mock.patch.object(package.icon, "parse"):  # the host refuses it; the watch must cope
        return package.pack(bytes(image), name="broken", version=1, entry=pkg.entry + pad, relocs=relocs,
                            abi_minor=pkg.abi_minor)


def letter_rows(frame: bytes) -> int:
    """Rows showing the letter circle: vertical runs of its colour at the circle's left edge column."""
    want = [v * 160 // 255 for v in rgb(LETTER_BG)]
    runs, run = 0, 0
    for y in range(466):
        p = frame[3 * (y * 466 + X0 + 10):3 * (y * 466 + X0 + 10) + 3]
        run = run + 1 if all(abs(a - b) <= 8 for a, b in zip(p, want)) else 0
        runs += run == 40
    return runs


@skipUnless(harness.E2E, "set WP2_E2E=1 (needs the emulator, netsimd and gdb)")
class IconTest(unittest.TestCase):
    def test_menu_shows_icons_and_letters(self):
        out = paths.ROOT / "runs" / "appicons"
        out.mkdir(parents=True, exist_ok=True)
        with harness.installed(["loader", *APPS, "hello"], "icons") as (emu, port):
            bad = emu.run / "broken.wp2app"
            bad.write_bytes(broken((paths.ROOT / "target" / "hello.wp2app").read_bytes()))
            with mock.patch.object(package.icon, "parse"):
                harness.cli_out(["install", bad, "--hci-port", port])
            for _ in range(3):  # the first connection's "Paired" popup
                if not emu.peek(0x3800FAF4, 1)[0] & 3:
                    break
                emu.send("c 1", "c 0", pause=2)
                emu.keep_awake()
            free = harness.u32(emu, harness.HEAP + 8)
            emu.call(harness.OPEN_VIEW, 0xF0, 0)
            emu.sleep(3)
            self.assertEqual(emu.peek(START + 4, 2), b"\x01\x01")  # loader loaded, active
            shutil.copy(emu.run / "frame.ppm", out / "menu.ppm")
            frame = emu.frame()[2]
            for row, name in enumerate(APPS[:2]):  # row 2 runs off the screen
                k, err = match(box(frame, X0, Y0 + PITCH * row), expected(name))
                print(f"{name}: gain {k:.3f}, error {err:.2f}")
                self.assertTrue(0.5 < k < 0.8 and err < 4, f"{name}: gain {k:.3f}, error {err:.2f}")
            emu.send("r -80", "r -80", "r -80", "r -80", pause=2.5)  # the crown scrolls row 2 into view
            shutil.copy(emu.run / "frame.ppm", out / "menu-crown.ppm")
            y, k, err = find(emu.frame()[2], expected(APPS[2]))
            print(f"{APPS[2]}: y {y}, gain {k:.3f}, error {err:.2f}")
            self.assertTrue(0.5 < k < 0.8 and err < 4, f"{APPS[2]}: y {y}, gain {k:.3f}, error {err:.2f}")
            # at the end hello and broken show the letter circle
            for _ in range(5):
                emu.send("r -80", pause=1)
            shutil.copy(emu.run / "frame.ppm", out / "menu-end.ppm")
            self.assertEqual(letter_rows(emu.frame()[2]), 2)
            emu.send("c 1", "c 0", pause=2)  # button: stock closes the view
            self.assertEqual(emu.peek(START + 4, 2), b"\x00\x00")
            self.assertEqual(harness.u32(emu, harness.HEAP + 8), free, "icon blocks freed with the menu")
            uart = emu.uart()
            self.assertNotIn("MllcFailedHook", uart)
            self.assertNotIn("ZEPHYR FATAL ERROR", uart)
            self.assertEqual(uart.count("*** Booting Zephyr"), 1, "guest rebooted")


if __name__ == "__main__":
    unittest.main()
