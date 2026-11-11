import asyncio
import json
import os
from pathlib import Path
import tempfile
import unittest

from wp2apps import channel, herdr

FAKE = Path(__file__).parent / "fixtures" / "fake_herdr.py"
WORKSPACES = [{"workspace_id": "w1", "label": "проект"}, {"workspace_id": "w2", "label": "infra"}]


def agent(pane, status, title, ws="w1", **extra):
    return {"pane_id": pane, "tab_id": pane.replace("p", "t"), "workspace_id": ws, "agent": "claude",
            "agent_status": status, "terminal_title_stripped": title, "cwd": "/tmp/x", **extra}


def state(*agents):
    return (json.dumps({"result": {"agents": list(agents)}}), json.dumps({"result": {"workspaces": WORKSPACES}}))


class ParseTest(unittest.TestCase):
    def test_fields_statuses_and_truncation(self):
        long = "Очень длинный заголовок сессии про что-то важное и ещё немного"
        agents, tabs = herdr.parse(*state(agent("w1:p1", "blocked", long), agent("w2:p2", "weird", "", ws="w2", name="n"),
                                          agent("w3:p3", "done", "", ws="w3")))
        a = agents["w1:p1"]
        self.assertEqual((a.status, a.ws.decode()), (2, "проект"))
        self.assertLessEqual(len(a.title), herdr.TITLE_MAX)
        self.assertTrue(long.startswith(a.title.decode()))
        self.assertEqual(agents["w2:p2"], herdr.Agent(4, b"infra", b"n"))
        self.assertEqual(agents["w3:p3"], herdr.Agent(3, b"", b"claude"))
        self.assertEqual(tabs["w1:p1"], "w1:t1")

    def test_clip_on_char_boundary(self):
        self.assertEqual(herdr.clip("привет", 5), "пр".encode())
        self.assertEqual(herdr.clip("abc", 5), b"abc")


class SyncTest(unittest.TestCase):
    def test_wire_layout(self):
        s = herdr.Sync()
        msgs = s.snapshot({"p": herdr.Agent(2, b"ab", b"c")})
        # same bytes as model.rs decode_rejects_foreign_and_short
        self.assertEqual(msgs, [b"h1B\x01", b"h1U\x02\x00\x02\x02abc", b"h1E\x03\x01"])
        self.assertEqual(s.beat(), b"h1H\x04")

    def test_delta_and_ids(self):
        s = herdr.Sync()
        a, b = herdr.Agent(1, b"w", b"a"), herdr.Agent(0, b"w", b"b")
        s.snapshot({"p1": a, "p2": b})
        self.assertEqual(s.delta({"p1": a, "p2": b}), [])
        blocked = herdr.Agent(2, b"w", b"a")
        out = s.delta({"p1": blocked, "p3": b})
        self.assertEqual([m[2:3] for m in out], [b"D", b"U", b"U"])
        self.assertEqual(out[0][4], 1)  # p2 had id 1
        self.assertEqual(out[1][4:6], bytes([0, 2]))
        self.assertEqual(s.pane(out[2][4]), "p3")
        self.assertNotEqual(out[2][4], 1, "freed id reused at once")
        self.assertEqual([m[3] for m in out], [5, 6, 7])
        for m in out:
            self.assertLessEqual(len(m), channel.MSG_MAX)

    def test_largest_message_fits_and_cap(self):
        s = herdr.Sync()
        big = herdr.Agent(1, herdr.clip("я" * 99, herdr.WS_MAX), herdr.clip("я" * 99, herdr.TITLE_MAX))
        agents = {f"p{i}": big for i in range(40)} | {"blk": herdr.Agent(2, b"w", b"x")}
        msgs = s.snapshot(agents)
        self.assertEqual(len(msgs), herdr.SHOWN + 2)
        self.assertIn("blk", s.sent)
        self.assertTrue(all(len(m) <= channel.MSG_MAX for m in msgs))


class FakeChannel:
    def __init__(self, fail=None):
        self.sent, self.slot, self.key, self.poll, self.fail = [], channel.ANY, b"k", 0.25, fail
        self.inbox = asyncio.Queue()

    async def send(self, data, timeout=10):
        if self.fail:
            raise self.fail
        self.sent.append(data)

    async def recv(self, timeout=None):
        async with asyncio.timeout(timeout):
            return await self.inbox.get()


class DaemonTest(unittest.IsolatedAsyncioTestCase):
    def setUp(self):
        self.dir = tempfile.TemporaryDirectory()
        d = Path(self.dir.name)
        self.state, self.log = d / "state.json", d / "focus.log"
        os.environ["FAKE_HERDR_STATE"], os.environ["FAKE_HERDR_LOG"] = str(self.state), str(self.log)
        self.write(agent("w1:p1", "working", "a"), agent("w1:p2", "idle", "b"))

    def tearDown(self):
        self.dir.cleanup()

    def write(self, *agents):
        self.state.write_text(json.dumps({"agents": list(agents), "workspaces": WORKSPACES}))

    async def test_waits_for_request_then_deltas_and_focus(self):
        ch = FakeChannel()
        d = herdr.Daemon(ch, herdr=str(FAKE), period=0.05, log=lambda _: None)
        task = asyncio.create_task(d.run())
        try:
            await asyncio.sleep(0.3)
            self.assertEqual(ch.sent, [], "sent before the app asked")
            await ch.inbox.put(channel.Message(0, 3, b"h1R"))
            await asyncio.sleep(0.2)
            self.assertEqual([m[2:3] for m in ch.sent], [b"B", b"U", b"U", b"E"])
            self.assertEqual(ch.slot, 3)
            ch.sent.clear()
            self.write(agent("w1:p1", "blocked", "a"), agent("w1:p2", "idle", "b"))
            await asyncio.sleep(0.3)
            self.assertEqual([(m[2:3], m[5]) for m in ch.sent], [(b"U", 2)])
            pid = d.sync.ids["w1:p1"]
            await ch.inbox.put(channel.Message(1, 3, b"h1F" + bytes([pid])))
            await asyncio.sleep(0.3)
            self.assertEqual(self.log.read_text(), "agent focus w1:p1\n")
            await ch.inbox.put(channel.Message(2, 3, b"other app"))
            await ch.inbox.put(channel.Message(2, 3, b"h1F\xee"))  # unknown id: logged, nothing run
            await asyncio.sleep(0.2)
            self.assertEqual(self.log.read_text(), "agent focus w1:p1\n")
        finally:
            task.cancel()

    async def test_no_app_unsyncs_and_beats(self):
        ch = FakeChannel()
        d = herdr.Daemon(ch, herdr=str(FAKE), period=0.05, beat=0.2, log=lambda _: None)
        task = asyncio.create_task(d.run())
        try:
            await ch.inbox.put(channel.Message(0, 0, b"h1R"))
            await asyncio.sleep(0.5)
            self.assertIn(b"H", [m[2:3] for m in ch.sent])
            ch.fail = channel.NoApp("x")
            await asyncio.sleep(0.3)
            self.assertFalse(d.synced)
            self.assertEqual((ch.slot, ch.poll), (channel.ANY, 1.0))
        finally:
            task.cancel()


if __name__ == "__main__":
    unittest.main()
