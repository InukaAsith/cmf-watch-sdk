"""Offline maps on the emulated watch: the loader's FILE service (lv_submit op 0x16) end to end.

  WP2_E2E=1 uv run --locked python -m unittest tests.test_maps_e2e -v

Installs the loader and maps, sends a small pack over the channel (hosts/mapbuild.py send_pack: F/f/c per file,
acked, idx last), then checks the map is drawn, and drawn again from NAND after the app was closed and reopened.
Frames go to runs/maps-e2e-*.png.
"""
import asyncio, pathlib, shutil, subprocess, sys, time, unittest

from tests.e2e.harness import E2E, STUB_RAM, installed, open_app
from tests.test_throughput_e2e import bind
from wp2apps.channel import Channel

ROOT = pathlib.Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "hosts"))
PACK = ROOT / "runs/mappack/small"


def colours(frame: pathlib.Path) -> int:
    """Distinct colours in the middle 200×200 of a PPM frame (a map has many, a text screen two or three)."""
    data = frame.read_bytes()
    # P6\n466 466\n255\n
    head = data.split(b"\n", 3)
    w = int(head[1].split()[0])
    px = head[3]
    seen = set()
    for y in range(133, 333, 4):
        for x in range(133, 333, 4):
            i = 3 * (y * w + x)
            seen.add(px[i:i + 3])
    return len(seen)


@unittest.skipUnless(E2E, "WP2_E2E=1")
class MapsTest(unittest.TestCase):
    def test_pack_sent_stored_and_drawn(self):
        if not (PACK / "idx").exists():
            subprocess.run(["uv", "run", "-q", "--with", "pillow", "python", "hosts/mapbuild.py", "build", "--lat",
                            "51.5080", "--lon", "-0.1281", "--radius", "0.4", "--min-zoom", "15", "--max-zoom", "16",
                            "--out", str(PACK)], cwd=ROOT, check=True)
        from mapbuild import send_pack

        with installed(["loader", "maps"], name="maps") as (emu, port):
            key = asyncio.run(bind(port))
            open_app(emu, 0)
            shutil.copy(emu.run / "frame.ppm", ROOT / "runs/maps-e2e-0-empty.ppm")

            async def run():
                ch_ = Channel(hci_port=port)
                ch_.key = key
                async with ch_ as ch:
                    t0 = time.monotonic()
                    n, total = await send_pack(ch, PACK)
                    return n, total, time.monotonic() - t0

            n, total, dt = asyncio.run(run())
            print(f"\nmaps: {n} files, {total} B in {dt:.1f} s ({total / dt:.0f} B/s)", flush=True)
            emu.sleep(3)
            shutil.copy(emu.run / "frame.ppm", ROOT / "runs/maps-e2e-1-sent.ppm")
            self.assertGreater(colours(emu.run / "frame.ppm"), 5, "no map after the pack arrived")

            # closed and opened again: everything comes from NAND
            emu.send("c 1", "c 0", pause=3)
            self.assertEqual(emu.peek(STUB_RAM + 6, 1), b"\xff", "app did not close")
            open_app(emu, 0, pause=6)
            shutil.copy(emu.run / "frame.ppm", ROOT / "runs/maps-e2e-2-reopened.ppm")
            self.assertGreater(colours(emu.run / "frame.ppm"), 5, "no map from NAND after reopening")
            # crown zoom: another level, still a map
            emu.send("r 80", "r 80", pause=4)
            shutil.copy(emu.run / "frame.ppm", ROOT / "runs/maps-e2e-3-zoom.ppm")
            uart = emu.uart()
            for bad in ("MllcFailedHook", "ZEPHYR FATAL ERROR"):
                self.assertNotIn(bad, uart)
