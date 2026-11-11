import asyncio
import unittest

from wp2apps import channel, link
from wp2apps.install import Status

KEY = bytes(range(16))


class FakeSession:
    def __init__(self, statuses):
        self.statuses, self.calls, self.key = list(statuses), [], KEY
        self.events = asyncio.Queue()

    async def request(self, c1, c2, payload, expect, timeout=6.0):
        self.calls.append((c2, payload, expect))
        if c2 == channel.PING:
            return bytes([0, 3, 1])
        return bytes([self.statuses.pop(0)])


class ChannelTest(unittest.IsolatedAsyncioTestCase):
    async def test_send_frames_slot_and_data(self):
        s = FakeSession([Status.OK])
        await channel.send(s, b"hi", slot=2)
        self.assertEqual(s.calls, [(0x90A5, b"\x02hi", 0xB0A5)])

    async def test_full_and_paused_are_retried(self):
        s = FakeSession([Status.FULL, Status.PAUSED, Status.OK])
        await channel.send(s, b"x", wait=5)
        self.assertEqual(len(s.calls), 3)

    async def test_retry_gives_up(self):
        with self.assertRaises(TimeoutError):
            await channel.send(FakeSession([Status.FULL] * 100), b"x", wait=0.1)

    async def test_no_app_and_refusals(self):
        with self.assertRaises(channel.NoApp):
            await channel.send(FakeSession([Status.NO_APP]), b"x")
        with self.assertRaisesRegex(ValueError, "BAD_REQUEST"):
            await channel.send(FakeSession([Status.BAD_REQUEST]), b"x")
        for bad in (b"", bytes(129)):
            with self.assertRaisesRegex(ValueError, "1..128"):
                await channel.send(FakeSession([]), bad)

    async def test_recv_decrypts_event(self):
        s = FakeSession([])
        s.events.put_nowait(link.encrypt(KEY, bytes([7, 2]) + b"pong"))
        self.assertEqual(await channel.recv(s, 1), channel.Message(7, 2, b"pong"))
        with self.assertRaises(TimeoutError):
            await channel.recv(s, 0.05, poll=0.01)
        self.assertTrue(s.calls and all(c[0] == channel.PING for c in s.calls))

    async def test_ping(self):
        self.assertEqual(await channel.ping(FakeSession([])), channel.State(3, 1))

    def test_events_routed_apart_from_replies(self):
        s = link.Session()
        s._route((link.SYSTEM, 0xB0A6, b"e"))
        s._route((link.SYSTEM, 0xB0A5, b"r"))
        self.assertEqual((s.events.get_nowait(), s.frames.get_nowait()), (b"e", (link.SYSTEM, 0xB0A5, b"r")))


class ReconnectTest(unittest.IsolatedAsyncioTestCase):
    async def test_lost_link_reconnects(self):
        sessions = [FakeSession([]), FakeSession([Status.OK])]

        async def lost(*a, **k):
            raise ConnectionError("gone")
        sessions[0].request = lost
        ch = channel.Channel(hci_port=1)
        connects = []

        async def connect():
            if ch.session is None:
                ch.session = sessions[len(connects)]
                connects.append(ch.session)
            return ch.session
        ch.connect = connect
        await ch.send(b"x", timeout=5)
        self.assertEqual(len(connects), 2)
        self.assertEqual(sessions[1].calls[0][1], b"\xffx")

    def test_needs_one_transport(self):
        for kw in ({}, {"address": "a", "hci_port": 1}):
            with self.assertRaises(ValueError):
                channel.Channel(**kw)


if __name__ == "__main__":
    unittest.main()
