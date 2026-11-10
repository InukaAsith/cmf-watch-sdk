import asyncio
import contextlib
import io
import json
import unittest
from unittest import mock

from wp2apps import blegatt, cli, link

ADDR = "AA:BB:CC:DD:EE:FF"


class FakeClient:
    mtu_size = 185

    def __init__(self, target, **_):
        self.target, self.writes, self.notify = target, [], {}

    async def __aenter__(self):
        return self

    async def __aexit__(self, *_):
        return False

    async def write_gatt_char(self, uuid, data, response):
        self.writes.append((uuid, data, response))

    async def start_notify(self, uuid, callback):
        self.notify[uuid] = callback


class AdapterTest(unittest.TestCase):
    def test_write_and_subscribe(self):
        got, clients = [], []

        def make(target, **kw):
            clients.append(FakeClient(target, **kw))
            return clients[0]

        async def run():
            with mock.patch.object(blegatt, "BleakClient", make):
                async with blegatt.gatt_over_ble(ADDR) as gatt:
                    await gatt.write(link.CMD_W, b"abc", response=False)
                    await gatt.write(link.SHELL_W, b"x")
                    await gatt.subscribe(link.CMD_N, got.append)
                    clients[0].notify[link.CMD_N](None, bytearray(b"\x01\x02"))
                    clients[0].mtu_size = 23
                    await gatt.write(link.CMD_W, bytes(200), response=False)  # long frame -> with response
                    await gatt.write(link.CMD_W, bytes(20), response=False)

        asyncio.run(run())
        self.assertEqual(clients[0].target, ADDR)
        self.assertEqual(clients[0].writes, [(link.CMD_W, b"abc", False), (link.SHELL_W, b"x", True),
                                            (link.CMD_W, bytes(200), True), (link.CMD_W, bytes(20), False)])
        self.assertEqual(got, [b"\x01\x02"])

    def test_name_not_found(self):
        async def run():
            with mock.patch.object(blegatt.BleakScanner, "find_device_by_filter", mock.AsyncMock(return_value=None)):
                async with blegatt.gatt_over_ble(None):
                    pass

        with self.assertRaises(ConnectionError):
            asyncio.run(run())


class CliTest(unittest.TestCase):
    def test_exactly_one_transport(self):
        for argv in (["list"], ["list", "--address", ADDR, "--hci-port", "1"]):
            with self.assertRaises(SystemExit), contextlib.redirect_stderr(io.StringIO()):
                cli.parser().parse_args(argv)
        self.assertEqual(cli.parser().parse_args(["list", "--address", ADDR]).address, ADDR)
        self.assertEqual(cli.parser().parse_args(["remove", "--slot", "1", "--hci-port", "9"]).hci_port, 9)

    def test_scan_redacts(self):
        found = [("CMF Watch Pro 2", -50, ADDR)]
        for argv, shown in ((["scan"], "<redacted>"), (["scan", "--show-address"], ADDR)):
            out = io.StringIO()
            with mock.patch.object(blegatt, "scan", mock.AsyncMock(return_value=found)), \
                    contextlib.redirect_stdout(out):
                self.assertEqual(cli.main(argv), 0)
            self.assertEqual(json.loads(out.getvalue())["devices"][0]["address"], shown)

    def test_name_filter(self):
        self.assertTrue(blegatt.looks_like_cmf("CMF Watch Pro 2"))
        self.assertFalse(blegatt.looks_like_cmf(None))
        self.assertFalse(blegatt.looks_like_cmf("Beats"))


if __name__ == "__main__":
    unittest.main()


class SplitNotificationTest(unittest.TestCase):
    def test_parser_reassembles_mtu23_notifications(self):
        data = link.frame(link.SYSTEM, 0x804D, bytes(range(100)), bytes(16))
        parser, out = link.Parser(), []
        for i in range(0, len(data), 20):
            out += parser.feed(data[i:i + 20])
        self.assertEqual([(c1, c2) for c1, c2, _ in out], [(link.SYSTEM, 0x804D)])
