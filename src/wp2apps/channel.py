"""Host side of the ABI 1.1 data channel (docs/abi-1.1.md): messages to the running app and its EVENT frames."""
import asyncio
from contextlib import AsyncExitStack
from dataclasses import dataclass

from bleak.exc import BleakError

from . import link
from .install import Status
from .link import SYSTEM

MSG, PING, EVENT = 0x90A5, 0x90A7, link.STUB_EVENT
ANY = 0xFF
MSG_MAX = 128
RETRY = (Status.FULL, Status.PAUSED)
LOST = (TimeoutError, ConnectionError, OSError, BleakError)


class NoApp(Exception):
    """No ABI 1.1 app runs in the requested slot."""


@dataclass(frozen=True)
class Message:
    seq: int
    slot: int
    data: bytes


@dataclass(frozen=True)
class State:
    running_slot: int  # 0xFF: none
    flags: int  # bit 0 ABI 1.1 app, bit 1 paused


def message(key: bytes, body: bytes) -> Message:
    plain = link.decrypt(key, body)
    if len(plain) < 3:
        raise ValueError(f"EVENT of {len(plain)} bytes")
    return Message(plain[0], plain[1], plain[2:])


async def ping(session, timeout: float = 6.0) -> State:
    reply = await session.request(SYSTEM, PING, b"\0", PING | 0xA000, timeout=timeout)
    if len(reply) != 3 or reply[0] != Status.OK:
        raise ValueError(f"bad PING reply {reply.hex()}")
    return State(reply[1], reply[2])


async def send(session, data: bytes, slot: int = ANY, wait: float = 10.0) -> None:
    """One message, acked by the stub; FULL and PAUSED are retried until `wait` runs out."""
    if not 1 <= len(data) <= MSG_MAX:
        raise ValueError(f"message must be 1..{MSG_MAX} bytes, got {len(data)}")
    loop = asyncio.get_running_loop()
    deadline, delay = loop.time() + wait, 0.02
    while True:
        reply = await session.request(SYSTEM, MSG, bytes([slot]) + data, MSG | 0xA000)
        status = Status(reply[0]) if reply and reply[0] in Status._value2member_map_ else reply[:1].hex()
        if status == Status.OK:
            return
        if status == Status.NO_APP:
            raise NoApp("no ABI 1.1 app runs" + ("" if slot == ANY else f" in slot {slot}"))
        if status not in RETRY:
            raise ValueError(f"watch refused the message: {getattr(status, 'name', status)}")
        if loop.time() + delay > deadline:
            raise TimeoutError(f"app inbox still {status.name} after {wait}s")
        await asyncio.sleep(delay)
        delay = min(delay * 2, 0.5)


async def recv(session, timeout: float | None = None) -> Message:
    async with asyncio.timeout(timeout):
        body = await session.events.get()
    return message(session.key, body)


class Channel:
    """Reconnecting link to the running app, for daemons. A lost link (BLE error, timeout) is dropped and the next
    call reconnects with backoff 1, 2, 4 ... 30 s and a fresh handshake."""

    def __init__(self, address: str | None = None, hci_port: int | None = None, slot: int = ANY,
                 keepalive: float = 5.0):
        if (address is None) == (hci_port is None):
            raise ValueError("give exactly one of address and hci_port")
        self.address, self.hci_port, self.slot, self.keepalive = address, hci_port, slot, keepalive
        self.session = None
        self._stack = None
        self._backoff = 1.0

    def _transport(self):
        from . import blegatt, h4
        return blegatt.gatt_over_ble(self.address) if self.address else h4.gatt_over_h4(self.hci_port)

    async def connect(self):
        while self.session is None:
            stack = AsyncExitStack()
            try:
                gatt = await stack.enter_async_context(self._transport())
                session = link.Session()
                await session.handshake(gatt)
            except (*LOST, ValueError):
                await stack.aclose()
                await asyncio.sleep(self._backoff)
                self._backoff = min(self._backoff * 2, 30.0)
                continue
            self.session, self._stack, self._backoff = session, stack, 1.0
        return self.session

    async def drop(self):
        stack, self.session, self._stack = self._stack, None, None
        if stack:
            try:
                await stack.aclose()
            except LOST:
                pass

    close = drop

    async def __aenter__(self):
        await self.connect()
        return self

    async def __aexit__(self, *exc):
        await self.drop()

    async def send(self, data: bytes, timeout: float = 10.0) -> None:
        loop = asyncio.get_running_loop()
        deadline = loop.time() + timeout
        while True:
            session = await asyncio.wait_for(self.connect(), max(deadline - loop.time(), 0.001))
            try:
                return await send(session, data, self.slot, wait=max(deadline - loop.time(), 0.0))
            except LOST:
                await self.drop()
                if loop.time() >= deadline:
                    raise TimeoutError("link lost and not back in time") from None

    async def recv(self, timeout: float | None = None) -> Message:
        """Next EVENT of the app (of `slot` unless ANY); a PING after `keepalive` idle seconds notices a dead link."""
        loop = asyncio.get_running_loop()
        deadline = None if timeout is None else loop.time() + timeout
        while True:
            session = await self.connect()
            left = None if deadline is None else deadline - loop.time()
            if left is not None and left <= 0:
                raise TimeoutError("no message")
            try:
                m = await recv(session, self.keepalive if left is None else min(left, self.keepalive))
            except TimeoutError:
                try:
                    await ping(session)
                except LOST:
                    await self.drop()
                continue
            except ValueError:
                continue  # not sealed with this session's key
            if self.slot in (ANY, m.slot):
                return m


def text(data: bytes) -> str | None:
    try:
        return data.decode("utf-8")
    except UnicodeDecodeError:
        return None
