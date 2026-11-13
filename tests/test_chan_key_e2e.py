"""The stub's own channel key K in the emulator (docs/abi-1.4.md): a host with K only (no bind, no popup) talks to
the app and installs, next to a stock-keyed host on the same link (stands in for Nothing X on an iPhone).
Needs target/{loader,api-demo}.wp2app. WP2_NETSIM_INSTANCE=61."""
import asyncio
from contextlib import ExitStack, asynccontextmanager
import unittest
from unittest import skipUnless

from tests.e2e import harness
from wp2apps import channel, h4, install, link, package, paths

RUNNING = harness.STUB_RAM + 6
DEMO = paths.ROOT / "target" / "api-demo.wp2app"


class Tee:
    """One link, several sessions: what two iOS apps on the system's shared connection see. Every subscriber gets
    every notification."""

    def __init__(self, gatt):
        self.gatt, self.callbacks = gatt, {}

    async def write(self, uuid, data, response=True):
        await self.gatt.write(uuid, data, response)

    async def subscribe(self, uuid, callback):
        if uuid not in self.callbacks:
            self.callbacks[uuid] = []
            await self.gatt.subscribe(uuid, lambda data: [cb(data) for cb in self.callbacks[uuid]])
        self.callbacks[uuid].append(callback)


@asynccontextmanager
async def k_session(gatt):
    s = link.Session()
    await s.open_channel(gatt)
    yield s


async def echo(s, data: bytes) -> bytes:
    await channel.send(s, data)
    while True:
        m = await channel.recv(s, 10)
        if m.data == data:
            return m.data


@skipUnless(harness.E2E, "set WP2_E2E=1 (needs the emulator, netsimd, gdb and llvm-nm)")
class ChannelKeyTest(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.stack = ExitStack()
        cls.emu, cls.port = cls.stack.enter_context(harness.installed(["loader", "api-demo"], name="chankey"))
        cls.stock_key = None

        async def bind():
            async with h4.gatt_over_h4(cls.port) as gatt:
                s = link.Session()
                await s.handshake(gatt)
                cls.stock_key = s.key

        asyncio.run(bind())
        harness.open_app(cls.emu, row=0)  # closes the "Paired" popup of that bind first

    @classmethod
    def tearDownClass(cls):
        uart = cls.emu.uart()
        cls.stack.close()
        for bad in ("MllcFailedHook", "ZEPHYR FATAL ERROR"):
            assert bad not in uart, bad

    def popup(self) -> bool:
        return bool(self.emu.peek(harness.POPUP_FLAGS, 1)[0] & 3)

    def test_1_k_host_without_bind(self):
        async def run():
            async with h4.gatt_over_h4(self.port) as gatt, k_session(gatt) as s:
                self.assertFalse(self.popup(), "popup after connect + GETSECRET")
                self.assertEqual(await channel.ping(s), channel.State(0, 1))
                self.assertFalse(self.popup(), "popup after a K PING")
                self.assertEqual(await echo(s, b"k" * 128), b"k" * 128)

        asyncio.run(run())
        self.assertFalse(self.popup(), "a K session must not open the bind popup")
        self.assertEqual(self.emu.peek(RUNNING, 1)[0], 0, "the app must still run")

    def test_2_stock_and_k_hosts_coexist(self):
        async def run():
            async with h4.gatt_over_h4(self.port) as raw:
                gatt = Tee(raw)
                stock = link.Session()
                await stock.attach(gatt)
                self.assertTrue(await stock.resume(self.stock_key), "stock session must resume (no bind)")
                async with k_session(gatt) as k:
                    self.assertEqual(await echo(k, b"one"), b"one")
                    await stock.request(link.SYSTEM, link.STUB_INFO, b"\xff", link.STUB_INFO | 0xA000)
                    self.assertEqual(await echo(k, b"two"), b"two")
                    self.assertEqual(await echo(stock, b"three"), b"three")
                    self.assertEqual(await echo(k, b"four"), b"four")

        asyncio.run(run())
        self.assertFalse(self.popup())
        self.assertEqual(self.emu.peek(RUNNING, 1)[0], 0)

    def test_3_k_install(self):
        data = DEMO.read_bytes()

        async def run():
            async with h4.gatt_over_h4(self.port) as gatt, k_session(gatt) as s:
                await install.upload(s, 5, data)
                header = await install.info(s, 5)
                await install.remove(s, 5)
                return header, await install.info(s, 5)

        header, gone = asyncio.run(run())
        self.assertEqual(header.name, package.parse(data).name)
        self.assertIsNone(gone)
        self.assertFalse(self.popup())

    def test_4_unsolicited_event(self):
        """App messages reach a K host with no host frame after them: the loader kicks the stub on send."""
        async def run():
            async with h4.gatt_over_h4(self.port) as gatt, k_session(gatt) as s:
                await channel.send(s, b"!spam 40")  # the app answers later, from its own thread
                got = []
                async with asyncio.timeout(10):  # no PING: only unsolicited EVENTs can arrive
                    while len(got) < 40:
                        m = channel.message(s.key, await s.events.get())
                        if m.data.startswith(b"spam"):
                            got.append(m)
                return got

        got = asyncio.run(run())
        self.assertEqual([m.data for m in got], [f"spam {i}".encode() for i in range(40)])
        self.assertEqual([m.seq for m in got], [(got[0].seq + i) % 256 for i in range(40)])
