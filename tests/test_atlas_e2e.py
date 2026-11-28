"""Maps (sdk/examples/atlas) with its stand-in phone (hosts/atlashost.py), in the emulator.

  WP2_E2E=1 uv run --locked python -m unittest tests.test_atlas_e2e -v

As a user would: the home map (drawn by the watch from the scene the phone sends), a tap at its top edge (it
moves north at once, nothing asked), ◎ (back to us), search, Search, "park" on the QWERTY keyboard, the results as
pins with the first in the sheet, its card, Directions, its ways (car, bike, foot), Drive, the first of the two routes
there, the whole route on its map, the crown to its first turn, Start, the turn view, the
map view, the button ("Exit navigation?"), Exit. Each scene's chunks are shuffled over the sliding window. Frames
in runs/atlas/.
"""
import asyncio, threading, unittest

from tests.e2e.harness import E2E, installed, open_app
from tests.test_demo_e2e import save_png
from tests.test_throughput_e2e import bind
from wp2apps import paths
from wp2apps.channel import Channel

import sys
sys.path.insert(0, str(paths.ROOT / "hosts"))
import atlashost  # noqa: E402

OUT = paths.ROOT / "runs" / "atlas"
# atlas ui.rs: Home's buttons, Find's Search, the QWERTY keys (Spotify's layout), the sheet, Directions, the Drive way,
# the first route, Start, Exit
TOP_EDGE, ME, SEARCH, FIND_SEARCH = (233, 30), (402, 233), (233, 412), (149, 97)
SHEET, DIRECTIONS, ROUTE0 = (233, 380), (233, 410), (233, 290)
KEYS = {"p": (431, 213), "a": (57, 271), "r": (167, 213), "k": (365, 271)}
GO, DRIVE, START, EXIT = (349, 387), (233, 156), (233, 384), (233, 228)


def tap(emu, xy, pause=0.6):
    emu.send(f"t 1 {xy[0]} {xy[1]}", f"t 0 {xy[0]} {xy[1]}", pause=pause)


def shot(emu, name):
    OUT.mkdir(parents=True, exist_ok=True)
    save_png(OUT / f"{name}.png", *emu.frame())


def colours(emu, y0, y1) -> int:
    """Distinct colours in rows y0..y1 near the middle: a map has several, a blank band one."""
    w, h, rgb = emu.frame()
    return len({rgb[(y * w + x) * 3:(y * w + x) * 3 + 3] for y in range(y0, y1, 3) for x in range(120, 346, 3)})


@unittest.skipUnless(E2E, "WP2_E2E=1")
class AtlasTest(unittest.TestCase):
    def test_search_route_check_start(self):
        log: list[str] = []
        with installed(["loader", "atlas"], name="atlas") as (emu, port):
            key = asyncio.run(bind(port))
            open_app(emu, 0)

            def host():
                async def run():
                    ch = Channel(hci_port=port)
                    ch.key = key
                    async with ch:
                        await atlashost.Host(ch, log=lambda *a: log.append(" ".join(map(str, a)))).run()
                try:
                    asyncio.run(run())
                except Exception as e:  # noqa: BLE001 (the emulator going away ends it)
                    log.append(f"host ended: {e!r}")

            threading.Thread(target=host, daemon=True).start()
            for _ in range(40):  # the watch asks again till the phone answers; then the home map
                emu.sleep(0.5)
                if colours(emu, 100, 300) > 3:
                    break
            shot(emu, "1-home")
            self.assertGreater(colours(emu, 100, 300), 3, log)

            home = emu.frame()[2]
            tap(emu, TOP_EDGE, pause=2)
            shot(emu, "1b-moved")
            moved = emu.frame()[2]
            self.assertNotEqual(moved, home, "the map moved")
            self.assertEqual(sum("scene" in l for l in log), 1, log)  # within the scene: nothing asked
            tap(emu, ME, pause=2)
            self.assertNotEqual(emu.frame()[2], moved, "back to us")

            # a swipe moves the map with the finger (no tap, no long press: the home map stays, nothing opens)
            swiped_from = emu.frame()[2]
            emu.send("t 1 300 300", "t 1 260 280", "t 1 220 260", pause=0.7)
            emu.send("t 0 180 240", pause=2)
            shot(emu, "1c-swiped")
            self.assertNotEqual(emu.frame()[2], swiped_from, "the swipe moved the map")
            self.assertFalse(any("place" in l or "ways to" in l for l in log), log)
            tap(emu, ME, pause=2)

            tap(emu, SEARCH)
            tap(emu, FIND_SEARCH)
            for ch in "park":
                tap(emu, KEYS[ch], pause=0.3)
            shot(emu, "2-typed")
            tap(emu, GO, pause=3)
            shot(emu, "3-results")
            self.assertTrue(any("search 'park'" in l for l in log), log)

            tap(emu, SHEET, pause=2)
            shot(emu, "4-place")
            tap(emu, DIRECTIONS, pause=3)
            self.assertTrue(any("ways to" in l for l in log), log)

            tap(emu, DRIVE, pause=3)
            shot(emu, "4b-routes")
            self.assertTrue(any("ways there to pick" in l for l in log), log)
            tap(emu, ROUTE0, pause=1)
            for _ in range(30):
                emu.sleep(0.5)
                if colours(emu, 150, 350) > 3:
                    break
            shot(emu, "5-route")
            self.assertTrue(any("route mode 0" in l for l in log), log)
            self.assertGreater(colours(emu, 150, 350), 3, log)

            emu.send("r -80", pause=2)  # one detent, delta +1: the first turn on the map, drawn at once
            shot(emu, "6-step")
            self.assertGreater(colours(emu, 150, 350), 3, log)
            self.assertEqual(sum("scene" in l for l in log), 3, log)  # home's, the search's and the route's

            tap(emu, START, pause=3)
            shot(emu, "7-nav")
            self.assertIn("start", log)
            tap(emu, (233, 233), pause=2)
            shot(emu, "7b-nav-map")
            self.assertGreater(colours(emu, 100, 250), 3, log)  # the streets under the route

            emu.send("c 1", "c 0", pause=1.5)  # the button: "Exit navigation?"
            shot(emu, "8-confirm")
            self.assertNotIn("stop", log)
            tap(emu, EXIT, pause=3)
            shot(emu, "9-back")
            self.assertIn("stop", log)
            uart = emu.uart()
            for bad in ("MllcFailedHook", "ZEPHYR FATAL ERROR"):
                self.assertNotIn(bad, uart)
