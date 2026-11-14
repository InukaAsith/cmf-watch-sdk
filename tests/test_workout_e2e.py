"""The workout app in the emulator (docs/workout.md): push a program, walk a set and its rest (buzz), leave and
resume a workout, finish it (rotation, history). Needs target/{loader,workout}.wp2app. WP2_NETSIM_INSTANCE=95."""
import asyncio
from pathlib import Path
import struct
import tempfile
import threading
import unittest
from unittest import skipUnless

from tests.e2e import harness
from tests.test_api_e2e import KEY, session
from tests import test_back_e2e
from tests.test_back_e2e import back, tap_row
from tests.test_api_e2e import BTCALL_MSGBOX, MSGBOX_CLOSE, MSGBOX_POPUP, WAKE_BIT, WAKE_WORD
from tests.test_demo_e2e import save_png
from tests.test_e2e import START
from wp2apps import channel, paths, workout

OUT = paths.ROOT / "runs" / "workout-e2e"
ELF = paths.ROOT / "sdk/target/thumbv7em-none-eabi/release/workout"
VIEW, SETS, BUZZES, ACKED, NEXT, EX = range(6)  # STATS, main.rs
START_V, DAY, SET, REST, ASK, DONE, PREVIEW = range(7)
ROW = (233, 164)  # the list's first row (screen.rs: header 96, gap 8, stock::ROW_H 120)
ROW2 = (233, 292)  # the row under it
PILL = (233, 342)  # "Сделал" / "Пропустить" (pill at y 300, 84 high)
BEGIN = (233, 402)  # "Начать" under day 1's two exercises (rows 104-224, 232-352, pill from 360)
ASK_END = (233, 294)
ASK_LEAVE = (233, 198)
NOTES = {"Day 01.md": "## День 1: А\n\n- [ ] Жим: 2×5 отдых 0:05\n- Тяга: 1×8 (40 кг) отдых 0:05\n",
         "Day 02.md": "## День 2: Б\n\n- Присед: 1×5 отдых 0:05\n"}


def program() -> bytes:
    with tempfile.TemporaryDirectory() as tmp:
        for name, text in NOTES.items():
            (Path(tmp) / name).write_text(text)
        return workout.encode(workout.parse_dir(Path(tmp)))


HR_WEAR, HR_BPM = 0x3804B30C, 0x3804B310  # wp2_core::stock HR_*, as tests/test_hr_e2e.py
ZONES = [95, 110, 120, 140, 160]  # synthetic bounds: SYNTH_BPM is in zone 3 (green)
SYNTH_BPM = 123  # synthetic: real heart-rate values are personal data and never appear in tests


@skipUnless(harness.E2E, "set WP2_E2E=1 (needs the emulator, netsimd, gdb and llvm-nm)")
class WorkoutTest(unittest.TestCase):
    """One watch for the whole walk; the tests run in name order and build on each other."""

    @classmethod
    def setUpClass(cls):
        OUT.mkdir(parents=True, exist_ok=True)
        cls.ctx = harness.installed(["loader", "workout"], name="workout")
        cls.emu, cls.port = cls.ctx.__enter__()
        asyncio.run(session(cls.port).__aenter__())  # bind before the app opens; later sessions resume the key
        harness.ready(cls.emu)
        harness.open_apps(cls.emu)

    @classmethod
    def tearDownClass(cls):
        cls.ctx.__exit__(None, None, None)

    def stats(self):
        addr = harness.u32(self.emu, START + harness.APP_BLOCK_OFF) + harness.symbol(ELF, "STATS")
        return struct.unpack("<6i", self.emu.peek(addr, 24))

    def tap(self, xy, pause=1.5):
        self.emu.send(f"t 1 {xy[0]} {xy[1]}", f"t 0 {xy[0]} {xy[1]}", pause=pause)

    def button(self, pause=1.5):
        self.emu.send("c 1", "c 0", pause=pause)

    def shot(self, name):
        w, h, rgb = self.emu.frame()
        save_png(OUT / f"{name}.png", w, h, rgb)

    def running(self):
        return self.emu.peek(START + 6, 1) != b"\xff"

    def with_app(self, action):
        """Runs `action(ch)` against the app on its own thread while the test drives the emulator."""
        out, done = [], threading.Event()

        def run():
            async def go():
                ch = channel.Channel(hci_port=self.port)
                ch.key = KEY[self.port]
                try:
                    out.append(await action(ch))
                finally:
                    await ch.drop()
            try:
                asyncio.run(go())
            except Exception as e:  # noqa: BLE001 - reported below
                out.append(e)
            done.set()
        threading.Thread(target=run, daemon=True).start()
        return out, done

    def wait(self, done, seconds=90):
        for _ in range(int(seconds * 2)):
            if done.is_set():
                return
            self.emu.sleep(0.5)
        self.fail("host side did not finish")

    def test_1_push_and_walk(self):
        p = program()
        async def push(ch):
            await workout.push(ch, p, log=lambda *_: None)
            await ch.send(workout.zones_msg(ZONES))
        out, done = self.with_app(push)
        tap_row(self.emu, 0)
        self.wait(done)
        self.assertNotIsInstance(out[0], Exception, out)
        self.emu.sleep(1)
        st = self.stats()
        self.assertEqual((st[VIEW], st[ACKED] & 0xFFFFFFFF, st[NEXT]), (START_V, workout.version(p), 0))
        self.shot("1-start")
        self.tap(ROW)  # day 1: its exercises and "Начать", nothing started yet
        self.assertEqual(self.stats()[VIEW], PREVIEW)
        self.assertFalse(self.stats()[SETS])
        self.shot("1-preview")
        self.tap(BEGIN)
        self.assertEqual(self.stats()[VIEW], DAY)
        # a synthetic reading in the stock cell (no PPG model; tests/test_hr_e2e.py): the header shows it
        self.emu.poke(HR_WEAR, b"\x01")
        self.emu.poke(HR_BPM, bytes([SYNTH_BPM]))
        self.emu.sleep(2.5)
        self.shot("1-day")
        # the live state for the phone: P every 5 s during a workout
        out, done = self.with_app(lambda ch: workout._expect(ch, b"P", 15))
        self.wait(done, 30)
        self.assertNotIsInstance(out[0], Exception, out)
        p = out[0]
        self.assertEqual((len(p), p[7], p[9]), (27, 0, 0xFF), p.hex())  # bpm: maybe sent before the first reading
        self.tap(ROW)  # its first exercise
        self.assertEqual(self.stats()[VIEW], SET)
        self.assertEqual(self.stats()[EX], 0)
        self.shot("1-set")
        self.overlay()  # AOD on hardware: before the first set its close went to the day list
        self.assertEqual(self.stats()[:1] + self.stats()[5:], (SET, 0), "the overlay's close left the set screen")
        # the set view is a pager of the day's open exercises: the crown and a vertical swipe go to the next one
        self.emu.send("r -80", pause=1.5)
        self.assertEqual(self.stats()[:1] + self.stats()[5:], (SET, 1), "the crown did not page to Тяга")
        self.shot("1-set-paged")
        self.emu.send("r 80", pause=1.5)
        self.assertEqual(self.stats()[EX], 0)
        # past half a page: the emulator's swipe has no throw, so the tileview snaps to the nearer page
        harness.swipe(self.emu, 233, 420, 233, 110, steps=8, pause=2)
        self.assertEqual(self.stats()[EX], 1, "swipe up did not page")
        harness.swipe(self.emu, 233, 110, 233, 420, steps=8, pause=2)
        self.assertEqual(self.stats()[EX], 0, "swipe down did not page")
        mark = len(self.emu.motor())
        self.tap(PILL)  # "Сделал"
        st = self.stats()
        self.assertEqual((st[VIEW], st[SETS]), (REST, 1))
        self.shot("1-rest")
        self.emu.sleep(7)  # rest 5 s
        st = self.stats()
        self.assertEqual((st[VIEW], st[BUZZES]), (SET, 1))
        self.assertTrue(any(duty > 0 for _, duty, _ in self.emu.motor()[mark:]), "no buzz at the end of the rest")
        self.shot("1-next-set")

    def overlay(self):
        """A stock overlay over the app (a message box here, AOD on hardware): PAUSE, then RESUME on its close."""
        self.emu.call(MSGBOX_POPUP, BTCALL_MSGBOX, 0)
        harness.wait_popup(self.emu, True)
        self.emu.sleep(0.5)
        self.emu.call(MSGBOX_CLOSE, BTCALL_MSGBOX, 0)
        harness.wait_popup(self.emu, False)
        self.emu.sleep(1)

    def wake_held(self):
        return bool(harness.u32(self.emu, WAKE_WORD) & WAKE_BIT)

    def test_1b_double_tap_and_lock(self):
        """A double tap on "Сделал" does not also skip the rest; leaving the rest by the button drops the lock."""
        self.assertEqual(self.stats()[VIEW], SET)
        sets = self.stats()[SETS]
        self.emu.send(f"t 1 {PILL[0]} {PILL[1]}", f"t 0 {PILL[0]} {PILL[1]}",
                      f"t 1 {PILL[0]} {PILL[1]}", f"t 0 {PILL[0]} {PILL[1]}", pause=1.5)
        st = self.stats()
        self.assertEqual((st[VIEW], st[SETS]), (REST, sets + 1), "the second tap skipped the rest")
        self.assertTrue(self.wake_held())
        self.button()  # rest -> day
        self.assertEqual(self.stats()[VIEW], DAY)
        self.emu.sleep(0.5)
        self.assertFalse(self.wake_held(), "the day list keeps the screen on")
        self.next_exercise()

    def test_1c_tap_that_wakes(self):
        """The tap that wakes a dark panel on the set screen lands on "Сделал": it must not count a set."""
        sets = self.stats()[SETS]
        test_back_e2e.ScreenOffTest.blank_and_wake(self.emu, self.port, wake=(f"t 1 {PILL[0]} {PILL[1]}", f"t 0 {PILL[0]} {PILL[1]}"))
        st = self.stats()
        self.assertEqual((st[VIEW], st[SETS]), (SET, sets), "the waking tap counted a set")

    def test_1d_swipe_back_on_ask(self):
        """The back swipe crosses the dialog's pills: it goes back to the day, it does not leave the app."""
        self.button()  # set -> day
        self.button()  # day -> ask
        self.assertEqual(self.stats()[VIEW], ASK)
        back(self.emu)
        self.shot("1d-after-swipe")
        self.assertTrue(self.running(), "the swipe left the app")
        self.assertEqual(self.stats()[VIEW], DAY)
        self.next_exercise()

    def next_exercise(self):
        """Day list: the first exercise is complete; a tap on the row under the centred one opens the second."""
        self.tap(ROW2)
        self.assertEqual(self.stats()[VIEW], SET)

    def test_2_leave_and_resume(self):
        sets = self.stats()[SETS]
        self.button()  # set -> day
        self.assertEqual(self.stats()[VIEW], DAY)
        self.button()  # day -> "Завершить тренировку?"
        self.assertEqual(self.stats()[VIEW], ASK)
        self.shot("2-ask")
        self.tap(ASK_LEAVE)  # "Выйти": the workout stays
        self.assertFalse(self.running())
        tap_row(self.emu, 0)  # the loader menu is up: open the app again
        self.emu.sleep(1)
        st = self.stats()
        self.assertEqual((st[VIEW], st[SETS]), (SET, sets), "the open workout did not come back")
        self.shot("2-resumed")

    def test_2b_push_held_while_running(self):
        """R says a workout runs: push refuses (a new program would finish it)."""
        other = workout.encode([workout.Day("В", (workout.Exercise("Жим", 1, (5, 5), "", 5),))])
        out, done = self.with_app(lambda ch: workout.push(ch, other, log=lambda *_: None, wait=60))
        self.button()  # set -> day
        self.button()  # day -> ask
        self.tap(ASK_LEAVE)
        tap_row(self.emu, 0)  # reopen: the app asks with R, flag set
        self.wait(done, 60)
        self.assertIsInstance(out[0], RuntimeError, out)
        self.assertNotEqual(self.stats()[ACKED] & 0xFFFFFFFF, workout.version(other))
        self.assertGreater(self.stats()[SETS], 0, "the workout must still be open")

    def test_3_finish_rotates(self):
        self.button()  # set -> day
        self.button()  # day -> ask
        self.tap(ASK_END)  # "Завершить": two sets done -> history, summary
        self.assertEqual(self.stats()[VIEW], DONE)
        self.shot("3-done")
        self.button(pause=2)  # leaves the app
        self.assertFalse(self.running())
        tap_row(self.emu, 0)
        self.emu.sleep(1)
        st = self.stats()
        self.assertEqual((st[VIEW], st[NEXT]), (START_V, 1), "the start screen does not offer day 2")
        out, done = self.with_app(workout.history)
        self.wait(done, 60)
        self.assertNotIsInstance(out[0], Exception, out)
        self.assertEqual([(e.day, e.done, e.planned) for e in out[0]], [(0, 2, 3)])
        start = out[0][0].start
        self.__class__.last_start = start
        out, done = self.with_app(workout.trace)  # the finished workout's sets and heart-rate samples
        self.wait(done, 60)
        self.assertNotIsInstance(out[0], Exception, out)
        t_start, sets, hr = out[0]
        self.assertEqual(t_start, start)
        self.assertEqual([ex for ex, _ in sets], [0, 0])
        self.assertTrue(0 < sets[0][1] <= sets[1][1], sets)
        self.assertIn(SYNTH_BPM, hr, "the synthetic reading reached the trace")


    def test_4_phone_drives(self):
        """The phone's workout (docs/specs/2026-11-14-shared-workout-design.md): D adopts it and counts a set, the
        watch's sets come back in X, F finishes it, H moves the rotation."""
        self.assertEqual(self.stats()[VIEW], START_V)
        last = self.__class__.last_start
        start, v = last + 120, workout.version(program())  # later than the last one: not stale, not known

        async def adopt(ch):
            await workout._request(ch, 30)  # the slot
            await ch.send(workout.set_msg(start, 1, v, 0xFF, 0))
            await asyncio.sleep(2)
            await ch.send(workout.set_msg(start, 1, v, 0, 5))
            await ch.send(workout.set_msg(start, 1, v, 0, 5))  # again: one set
            return ch.slot
        out, done = self.with_app(adopt)
        self.wait(done, 60)
        self.assertNotIsInstance(out[0], Exception, out)
        slot = out[0]
        self.emu.sleep(2)
        st = self.stats()
        self.assertEqual((st[VIEW], st[SETS]), (DAY, 1), "the phone's workout was not adopted")
        self.shot("4-adopted")

        async def sets_then_finish(ch):
            ch.slot = slot
            t = await workout.trace(ch, 0)
            await ch.send(workout.finish_msg(start, start + 60))
            await asyncio.sleep(2)
            await ch.send(workout.entry_msg(workout.Entry(start + 600, 900, 0, 3, 3)))
            return t
        out, done = self.with_app(sets_then_finish)
        self.wait(done, 60)
        self.assertNotIsInstance(out[0], Exception, out)
        t_start, sets, _ = out[0]
        self.assertEqual((t_start, sets), (start, [(0, 5)]))
        self.emu.sleep(2)
        self.assertEqual(self.stats()[VIEW], DONE)
        self.button(pause=2)  # leaves
        tap_row(self.emu, 0)
        self.emu.sleep(1)
        self.assertEqual(self.stats()[NEXT], 1, "H (day 0, the newest) did not move the rotation")
        out, done = self.with_app(workout.history)
        self.wait(done, 60)
        self.assertEqual([(e.start, e.day) for e in out[0]][:3], [(start + 600, 0), (start, 1), (last, 0)])


if __name__ == "__main__":
    unittest.main()
