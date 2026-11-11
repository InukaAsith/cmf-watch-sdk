"""herdr-agents in the emulator: the daemon (fake herdr) binds first, the app opens, the list renders, a blocked
transition buzzes, a tap reaches `herdr agent focus`, and a reopened app shows the stored list while it waits.
Needs target/{loader,herdr-agents}.wp2app. WP2_NETSIM_INSTANCE=87."""
import asyncio
import json
import os
import tempfile
import threading
import time
import unittest
from pathlib import Path
from unittest import skipUnless

from tests.e2e import harness
from tests.test_demo_e2e import save_png
from wp2apps import channel, herdr, paths

FAKE = Path(__file__).parent / "fixtures" / "fake_herdr.py"
OUT = paths.ROOT / "runs" / "herdr-agents"
RUNNING = harness.STUB_RAM + 6
LIST_TOP, ROW_H = 112, 58  # model.rs
DOT_X = [48, 23, 21, 43, 87]  # model.rs row_geometry
BLOCKED, DONE, WORKING, IDLE = "blocked", "done", "working", "idle"
WORKSPACES = [{"workspace_id": "w1", "label": "проект"}, {"workspace_id": "w2", "label": "infra"}]


def agent(pane, ws, status, title):
    return {"pane_id": f"{ws}:{pane}", "tab_id": f"{ws}:t{pane}", "workspace_id": ws, "agent": "claude",
            "agent_status": status, "terminal_title_stripped": title}


AGENTS = [agent("p1", "w1", "working", "Рефакторинг парсера"), agent("p2", "w1", "idle", "Обзор кода"),
          agent("p3", "w2", "done", "Write the release notes for version two"),
          agent("p4", "w2", "blocked", "Нужно подтверждение"), agent("p5", "w2", "working", "tests")]


class Daemon(threading.Thread):
    def __init__(self, port):
        super().__init__(daemon=True)
        self.logs = []
        self.d = herdr.Daemon(channel.Channel(hci_port=port), herdr=str(FAKE), log=self.logs.append)
        self.loop = asyncio.new_event_loop()

    def run(self):
        self.task = self.loop.create_task(self.d.run())
        try:
            self.loop.run_until_complete(self.task)
        except asyncio.CancelledError:
            pass
        self.loop.run_until_complete(self.d.ch.drop())

    def stop(self):
        self.loop.call_soon_threadsafe(self.task.cancel)
        self.join(15)

    def wait_log(self, needle, timeout=30.0):
        end = time.monotonic() + timeout
        while time.monotonic() < end:
            if any(needle in line for line in self.logs):
                return
            time.sleep(0.25)
        raise TimeoutError(f"daemon never logged {needle!r}: {self.logs}")


def hue(px):
    """Status colour of a pixel by hue: the panel capture scales brightness (white reads ~160)."""
    r, g, b = px
    if max(px) < 40:
        return None
    if r > 2 * g and r > 2 * b:
        return BLOCKED
    if g > 1.5 * r and g > 1.5 * b:
        return DONE
    if b > 1.5 * r and b > 1.2 * g:
        return WORKING
    if max(px) - min(px) < 20:
        return IDLE


@skipUnless(harness.E2E, "set WP2_E2E=1 (needs the emulator, netsimd, gdb and llvm-nm)")
class HerdrAgentsTest(unittest.TestCase):
    def frame(self, name):
        w, h, rgb = self.emu.frame()
        save_png(OUT / name, w, h, rgb)
        return lambda x, y: tuple(rgb[(y * w + x) * 3:][:3])

    def write(self, agents):
        self.state.write_text(json.dumps({"agents": agents, "workspaces": WORKSPACES}))

    def test_list_buzz_focus_storage(self):
        OUT.mkdir(parents=True, exist_ok=True)
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        self.state, focus_log = Path(tmp.name) / "state.json", Path(tmp.name) / "focus.log"
        os.environ["FAKE_HERDR_STATE"], os.environ["FAKE_HERDR_LOG"] = str(self.state), str(focus_log)
        self.write(AGENTS)
        with harness.installed(["loader", "herdr-agents"], name="herdr") as (self.emu, port):
            daemon = Daemon(port)
            daemon.start()
            try:
                daemon.wait_log("connected", 60)  # a full bind: the "Paired" popup, closed by open_app
                harness.open_app(self.emu, row=0)
                daemon.wait_log("asked for a snapshot")
                self.emu.sleep(2)
                px = self.frame("1-list.png")
                # blocked p4, done p3, working infra/tests, working проект, idle
                for row, colour in enumerate([BLOCKED, DONE, WORKING, WORKING, IDLE]):
                    got = px(DOT_X[row], LIST_TOP + row * ROW_H + ROW_H // 2)
                    self.assertEqual(hue(got), colour, f"row {row}: {got}")
                self.assertEqual({hue(px(x, 52)) for x in range(140, 330)} - {None, IDLE}, {BLOCKED, DONE, WORKING},
                                 "header dots missing")

                n = len(self.emu.motor())
                self.write([dict(a, agent_status="blocked") if a["pane_id"] == "w1:p1" else a for a in AGENTS])
                self.emu.sleep(4)
                buzz = self.emu.motor()[n:]
                print(f"\nblocked transition motor.log {buzz}")
                on = [e for e in buzz if e[1] > 0]
                self.assertTrue(on, "no buzz on the blocked transition")
                self.assertEqual(buzz[-1][1], 0, "motor left on")
                self.assertLess(buzz[-1][0] - on[0][0], 2000)
                px = self.frame("2-blocked.png")
                self.assertEqual(hue(px(DOT_X[1], LIST_TOP + ROW_H + ROW_H // 2)), BLOCKED, "p1 not in row 1")

                y = LIST_TOP + ROW_H + ROW_H // 2  # row 1: infra/p4 sorts before проект/p1
                self.emu.send(f"t 1 233 {y}", f"t 0 233 {y}", pause=0.2)
                self.frame("3-tap.png")
                daemon.wait_log("focus w1:p1", 20)
                self.assertEqual(focus_log.read_text(), "agent focus w1:p1\n")

                self.emu.send("c 1", "c 0", pause=2)  # BUTTON exits (saves the list)
                self.assertEqual(self.emu.peek(RUNNING, 1), b"\xff")
            finally:
                daemon.stop()
            # the menu still shows; the panel blanks ~15 s after the disconnect (harness.ready), so launch now
            self.assertEqual(self.emu.peek(harness.STUB_RAM + 4, 2), b"\x01\x01", "loader menu gone")
            self.emu.send("t 1 200 180", "t 0 200 180", pause=1.5)
            self.assertEqual(self.emu.peek(RUNNING, 1), b"\x00")
            px = self.frame("4-reopened-waiting.png")
            self.assertEqual(hue(px(DOT_X[0], LIST_TOP + ROW_H // 2)), BLOCKED, "stored list not shown")
            self.assertTrue(any(hue(px(x, y)) == BLOCKED for x in range(90, 380) for y in range(76, 92)),
                            "no 'waiting for Mac' line")
            self.emu.send("c 1", "c 0", pause=2)
            uart = self.emu.uart()
            self.assertNotIn("MllcFailedHook", uart)
            self.assertNotIn("ZEPHYR FATAL ERROR", uart)
            self.assertEqual(uart.count("*** Booting Zephyr"), 1, "guest rebooted")
            print("daemon log:\n" + "\n".join(daemon.logs))


if __name__ == "__main__":
    unittest.main()
