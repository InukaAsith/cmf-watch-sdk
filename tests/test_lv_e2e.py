"""lv-demo (ABI 1.2, stock LVGL widgets in the stock look) in the emulator: the screen, LV events, crown scrolling
with the round scroll bar, a rejected batch, DEL of the list, GUI heap after exit and a force stop while the app sits
in lv_submit.
Needs target/{loader,lv-demo}.wp2app. WP2_NETSIM_INSTANCE=93."""
import asyncio
from contextlib import ExitStack
import struct
import unittest
from unittest import skipUnless

from scripts.recon.gui_heap_probe import GUI_HEAP, GUI_SIZE, walk_heap
from tests.e2e import harness
from tests.test_api_e2e import session, unpaused
from tests.test_demo_e2e import save_png
from wp2apps import channel, paths

S = harness.STUB_RAM
RUNNING = S + 6
ELF = paths.ROOT / "sdk/target/thumbv7em-none-eabi/release/lv-demo"
LOADER_ELF = paths.ROOT / "fw/target/thumbv7em-none-eabi/release/wp2-loader"
OUT = paths.ROOT / "runs" / "lv"
# sdk/examples/lv-demo/src/screen.rs and cmf_wp2_app::stock
LIST_TOP, ROW_H, ROW_GAP = 348, 120, 8
PAGE, BAR, TITLE, CARD = 1, 2, 3, 4  # handles in creation order
ROW_X = 330  # right of a row's name, inside the round screen
CLICKED, VALUE_CHANGED = 7, 28
# STATS indices (lv-demo main.rs)
(FONT_RC, SCREEN_RC, MSG_RC, EVENTS, LAST_H, LAST_CODE, LAST_DELTA, ARC_VALUE, SLIDER, ARC, BAR_STAT, SPINS, LIST, MSGS,
 ROWS) = range(15)
N_ROWS = 4
N_STATS = ROWS + N_ROWS
LIST_HANDLES = 1 + N_ROWS * 5  # the list, then per row: row, circle, letter, name, detail
COORDS, FLAGS = 0x14, 0x24  # lv_obj_t: i32 x1 y1 x2 y2; flags (bit 0 HIDDEN), docs/recon-lvgl.md
CROWN_DOWN = "r -80"  # one detent


def accent(p) -> bool:
    r, g, b = p
    return r > g + 60 and r > b + 60


def row_bg(p) -> bool:
    """The stock row colour: dark, nearly grey, not black."""
    return 10 < max(p) < 90 and max(p) - min(p) < 24


@skipUnless(harness.E2E, "set WP2_E2E=1 (needs the emulator, netsimd, gdb and llvm-nm)")
class LvDemoTest(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        OUT.mkdir(parents=True, exist_ok=True)
        cls.stack = ExitStack()
        cls.emu, cls.port = cls.stack.enter_context(harness.installed(["loader", "lv-demo"], name="lv"))

        async def bind():  # before the app opens: harness.ready closes the "Paired" popup of this bind
            async with session(cls.port):
                pass

        asyncio.run(bind())

    @classmethod
    def tearDownClass(cls):
        cls.stack.close()

    def tearDown(self):
        uart = self.emu.uart()
        self.assertNotIn("MllcFailedHook", uart)
        self.assertNotIn("ZEPHYR FATAL ERROR", uart)
        self.assertNotIn("unsupported API", uart)
        self.assertEqual(uart.count("*** Booting Zephyr"), 1, "guest rebooted")

    # --- helpers ---

    def running(self) -> int:
        return self.emu.peek(RUNNING, 1)[0]

    def in_menu(self) -> bool:
        return self.emu.peek(S + 4, 3) == b"\x01\x01\xff"

    def to_menu(self):
        if self.running() != 0xFF:
            self.emu.send("c 1", "c 0", pause=2)
        if not self.in_menu():
            harness.open_menu(self.emu)
        self.emu.sleep(1)

    def launch(self):
        """Fresh lv-demo from the menu (row 0); waits for its screen batch."""
        self.to_menu()
        self.emu.send("t 1 200 180", "t 0 200 180", pause=1.5)
        self.assertEqual(self.running(), 0, "lv-demo did not start")
        # FONT_RC and SCREEN_RC start at 1 in the app; SLIDER is set after the screen submit returned
        for _ in range(20):
            s = self.stats()
            if s[SLIDER]:
                break
            self.emu.sleep(0.25)
        self.assertEqual((s[FONT_RC], s[SCREEN_RC]), (0, 0), f"font and screen batches: STATS {s}")
        self.assertTrue(s[SLIDER], f"screen batch never returned: STATS {s}")
        self.assertEqual((s[EVENTS], s[MSG_RC], s[SPINS], s[MSGS]), (0, 0, 0, 0), f"STATS of a previous run: {s}")
        return s

    def stats(self):
        at = harness.u32(self.emu, S + harness.APP_BLOCK_OFF) + harness.symbol(ELF, "STATS")
        return struct.unpack(f"<{N_STATS}i", self.emu.peek(at, 4 * N_STATS))

    def obj(self, h: int) -> int:
        lv = harness.u32(self.emu, S + harness.LOADER_BLOCK_OFF) + harness.symbol(LOADER_ELF, "LV_STATE")
        return harness.u32(self.emu, lv + 4 * h)

    def coords(self, h: int):
        return struct.unpack("<4i", self.emu.peek(self.obj(h) + COORDS, 16))

    def hidden(self, h: int) -> bool:
        return bool(harness.u32(self.emu, self.obj(h) + FLAGS) & 1)

    def gui_free(self) -> int:
        ptr = harness.u32(self.emu, GUI_HEAP)
        return walk_heap(self.emu.peek(ptr, GUI_SIZE), ptr)["free"]

    def frame(self, name: str):
        w, h, rgb = self.emu.frame()
        save_png(OUT / name, w, h, rgb)
        return rgb, lambda x, y: tuple(rgb[(y * w + x) * 3:][:3])

    def message(self, text: bytes):
        async def run():
            async with session(self.port) as s:
                await unpaused(s)
                await channel.send(s, text)

        asyncio.run(run())

    def widgets_drawn(self, px) -> list[str]:
        """What is missing of the slider (accent indicator, left of its value) and the arc (accent indicator)."""
        st = self.stats()
        x0, y0, x1, y1 = self.coords(st[SLIDER])
        ax0, ay0, ax1, ay1 = self.coords(st[ARC])
        missing = []
        if not accent(px(x0 + 20, (y0 + y1) // 2)):
            missing.append("slider indicator")
        if not any(accent(px(x, y)) for x in range(ax0, ax1, 2) for y in range(ay0, ay1, 2)):
            missing.append("arc indicator")
        return missing

    def slider_y(self) -> int:
        _, y0, _, y1 = self.coords(self.stats()[SLIDER])
        return (y0 + y1) // 2

    def row_y(self, i: int) -> int:
        _, y0, _, y1 = self.coords(self.stats()[ROWS + i])
        return (y0 + y1) // 2

    def wait_stat(self, i: int, pred, what: str):
        for _ in range(40):
            if pred(self.stats()[i]):
                return self.stats()
            self.emu.sleep(0.25)
        self.fail(f"{what}: STATS {self.stats()}")

    # --- tests ---

    def test_screen_events_crown(self):
        s = self.launch()
        rgb, px = self.frame("screen.png")
        title = {px(x, y) for x in range(130, 336) for y in range(56, 112)}
        shades = {p for p in title if accent(p)}
        print(f"\ntitle: {len(shades)} accent shades")
        self.assertGreaterEqual(len(shades), 3, "title is not drawn with an anti-aliased stock font in the accent")
        self.assertTrue(row_bg(px(60, 180)), f"card background {px(60, 180)}")
        self.assertTrue(row_bg(px(ROW_X, LIST_TOP + 32)), f"row 0 background {px(ROW_X, LIST_TOP + 32)}")
        self.assertEqual(self.coords(s[ROWS])[1], LIST_TOP, "row 0 top")
        self.assertEqual(self.widgets_drawn(px), [])

        # slider: drag leftward (a rightward drag is the stock back gesture: the app gets BUTTON and exits)
        x0, _, x1, _ = self.coords(s[SLIDER])
        ax0, ay0, ax1, ay1 = self.coords(s[ARC])
        _, before = self.frame("before-slider.png")
        arc_before = self.stats()[ARC_VALUE]
        harness.swipe(self.emu, x1 - 50, self.slider_y(), x0 + 60, self.slider_y(), steps=4, pause=1)
        s = self.stats()
        _, after = self.frame("after-slider.png")
        print(f"slider: arc {arc_before} -> {s[ARC_VALUE]}, last event {s[LAST_H]} {s[LAST_CODE]} {s[LAST_DELTA]}")
        self.assertEqual(self.running(), 0, "the drag exited the app")
        self.assertEqual((s[LAST_H], s[LAST_CODE]), (s[SLIDER], VALUE_CHANGED), s)
        self.assertEqual(s[ARC_VALUE], s[LAST_DELTA])
        self.assertLess(s[ARC_VALUE], arc_before)
        self.assertNotEqual([before(x, y) for x in range(ax0, ax1, 3) for y in range(ay0, ay1, 3)],
                            [after(x, y) for x in range(ax0, ax1, 3) for y in range(ay0, ay1, 3)], "arc did not change")

        # tap on row 0 (Tick): CLICKED for its handle, one motor tick
        n = len(self.emu.motor())
        y = min(self.row_y(0), 440)
        self.emu.send(f"t 1 {ROW_X} {y}", f"t 0 {ROW_X} {y}", pause=1)
        s = self.stats()
        self.assertEqual((s[LAST_H], s[LAST_CODE]), (s[ROWS], CLICKED), s)
        tick = self.emu.motor()[n:]
        print(f"row tap motor.log {tick}")
        self.assertEqual([d for _, d, _ in tick], [250, 0], tick)
        self.assertLessEqual(tick[1][0] - tick[0][0], 60)

        # crown: the page scrolls, row 0 moves up, the round scroll bar shows
        y0 = self.coords(s[ROWS])[1]
        self.assertTrue(self.hidden(BAR), "scroll bar before the crown")
        self.emu.send(*[CROWN_DOWN] * 4, pause=0.25)
        shown = not self.hidden(BAR)
        self.frame("crown.png")
        y1 = self.coords(s[ROWS])[1]
        print(f"crown x4: row 0 top {y0} -> {y1}, scroll bar shown {shown}")
        self.assertLess(y1, y0, "crown did not scroll the page")
        self.assertTrue(shown, "round scroll bar not shown while scrolling")
        self.emu.sleep(1.5)
        self.assertTrue(self.hidden(BAR), "scroll bar did not hide again")

        self.emu.send("c 1", "c 0", pause=2)
        self.assertTrue(self.in_menu(), "BUTTON did not exit to the menu")

    def test_invalid_batch_changes_nothing(self):
        self.launch()
        self.emu.sleep(1)
        before, _ = self.frame("before-bad.png")
        self.message(b"!bad")
        s = self.wait_stat(MSG_RC, lambda v: v != 0, "no result for !bad")
        self.assertEqual(s[MSG_RC], -22)
        self.emu.sleep(1)
        after, _ = self.frame("after-bad.png")
        self.assertEqual(self.running(), 0)
        self.assertTrue(before == after, "a rejected batch changed the screen")
        self.emu.send("c 1", "c 0", pause=2)
        self.assertTrue(self.in_menu())

    def handled(self, text: bytes, n: int):
        """Sends a host message and waits until lv-demo has handled its n-th one; returns STATS."""
        self.message(text)
        return self.wait_stat(MSGS, lambda v: v >= n, f"no result for {text}")

    def test_del_list(self):
        """DEL of a subtree on the device: the list, its rows and their circles and labels."""
        s = self.launch()
        self.emu.sleep(1)
        doomed = range(s[LIST], s[LIST] + LIST_HANDLES)
        kept = (PAGE, BAR, TITLE, CARD, s[SLIDER], s[ARC])
        self.assertEqual(s[ROWS + N_ROWS - 1], doomed[-5], "rows are not the list's last handles")
        self.assertTrue(all(self.obj(h) for h in [*doomed, *kept]), "objects before DEL")
        with_list = self.gui_free()

        s = self.handled(b"!del", 1)
        self.assertEqual(s[MSG_RC], 0, s)
        self.emu.sleep(0.5)
        gone = self.gui_free()
        _, px = self.frame("after-del.png")
        print(f"\nGUI heap free: with the list {with_list}, after DEL {gone} (the list cost {gone - with_list})")
        self.assertEqual([h for h in doomed if self.obj(h)], [], "LV_STATE.objs of freed handles")
        self.assertTrue(all(self.obj(h) for h in kept), "DEL took objects outside the list")
        self.assertGreater(gone - with_list, 1000, "DEL freed too little GUI heap")
        self.assertEqual(px(ROW_X, LIST_TOP + 32), (0, 0, 0), "row 0 still drawn")
        self.assertEqual(self.widgets_drawn(px), [])

        # touch still works: the slider reports
        x0, _, x1, _ = self.coords(s[SLIDER])
        harness.swipe(self.emu, x1 - 50, self.slider_y(), x0 + 60, self.slider_y(), steps=4, pause=1)
        s = self.stats()
        self.assertEqual((s[LAST_H], s[LAST_CODE]), (s[SLIDER], VALUE_CHANGED), s)
        # the crown still scrolls the page, past the place of the freed rows
        self.emu.send(*[CROWN_DOWN] * 4, pause=0.25)
        self.emu.sleep(1.5)  # past the bar's 1 s hide timer
        self.assertEqual(self.running(), 0, "crown after DEL")

        # the same list again: the same handles, under the card, and the GUI heap of the first list
        s = self.handled(b"!list", 2)
        self.assertEqual(s[MSG_RC], 0, s)
        self.assertEqual(range(s[LIST], s[LIST] + LIST_HANDLES), doomed)
        self.emu.sleep(0.5)
        again = self.gui_free()
        print(f"GUI heap free with the list again: {again}")
        self.assertLessEqual(abs(again - with_list), 256)
        self.assertEqual(self.coords(s[ROWS])[1], self.coords(CARD)[3] + 1 + ROW_GAP, "rebuilt row 0 under the card")
        y0 = self.coords(s[ROWS])[1]
        self.emu.send(*[CROWN_DOWN] * 4, pause=0.25)
        self.assertLess(self.coords(s[ROWS])[1], y0, "crown does not scroll the rebuilt list")
        self.emu.send("c 1", "c 0", pause=2)
        self.assertTrue(self.in_menu())

    def test_exit_frees_gui_heap(self):
        self.to_menu()
        menu = self.gui_free()
        self.launch()
        self.emu.sleep(1)
        app = self.gui_free()
        self.emu.send("c 1", "c 0", pause=2)
        self.assertTrue(self.in_menu(), "menu not back after BUTTON")
        self.emu.sleep(1)
        back = self.gui_free()
        self.frame("menu-after-exit.png")
        print(f"\nGUI heap free: menu {menu}, lv-demo {app} (uses {menu - app}), after exit {back}")
        self.assertLessEqual(abs(menu - back), 256)

    def test_stop_inside_submit(self):
        self.to_menu()
        menu = self.gui_free()
        self.launch()
        self.message(b"!spin")
        self.wait_stat(SPINS, lambda v: v > 5, "!spin did not start submitting")
        spins = self.stats()[SPINS]
        self.emu.send("c 1", "c 0", pause=0)  # the spinning app never reads its BUTTON
        start = self.emu.clock()
        while self.running() != 0xFF and self.emu.clock() - start < 3:
            self.emu.sleep(0.125)
        took = self.emu.clock() - start
        print(f"\nforce stop after {took:.3f} s ({spins}+ submits)")
        self.assertEqual(self.running(), 0xFF, "app not stopped")
        self.assertLessEqual(took, 2)
        self.assertGreaterEqual(took, 0.875, "stopped before the 1000 ms force stop")
        self.emu.sleep(1)
        self.assertTrue(self.in_menu())
        back = self.gui_free()
        print(f"GUI heap free: menu {menu}, after force stop {back}")
        self.assertLessEqual(abs(menu - back), 256)
        self.assertEqual(self.emu.motor()[-1][1], 0)
        # the mailbox is idle again: a new launch builds its screen (launch asserts both submits returned 0)
        self.launch()
        self.emu.sleep(1)
        _, px = self.frame("relaunch.png")
        self.assertEqual(self.widgets_drawn(px), [], "relaunched screen")
        self.assertTrue(row_bg(px(ROW_X, LIST_TOP + 32)), "relaunch: row 0 background")
        self.assertEqual(self.coords(self.stats()[ROWS])[1], LIST_TOP, "relaunch: row 0 top")
        self.emu.send("c 1", "c 0", pause=2)
        self.assertTrue(self.in_menu())


if __name__ == "__main__":
    unittest.main()
