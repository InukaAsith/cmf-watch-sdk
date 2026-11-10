from contextlib import asynccontextmanager

from bleak import BleakClient, BleakScanner

NAME = "CMF Watch Pro 2"
ATT_OVERHEAD = 3


def looks_like_cmf(name: str | None) -> bool:
    return bool(name) and "cmf" in name.lower()


class Gatt:
    """Same interface as h4.Gatt: write(uuid, data, response) and subscribe(uuid, callback)."""

    def __init__(self, client):
        self.client = client

    async def write(self, uuid: str, data: bytes, response: bool = True) -> None:
        # A frame longer than MTU-3 must go as a write with response: the OS then does an ATT long write
        # (Prepare/Execute) and the watch reassembles it. Split write commands do not work (emulator probe).
        if len(data) > self.client.mtu_size - ATT_OVERHEAD:
            response = True
        await self.client.write_gatt_char(uuid, data, response=response)

    async def subscribe(self, uuid: str, callback) -> None:
        # bleak picks notify or indicate from the characteristic's properties
        await self.client.start_notify(uuid, lambda _sender, data: callback(bytes(data)))


async def scan(timeout: float = 5.0):
    found = await BleakScanner.discover(timeout=timeout, return_adv=True)
    return [(d.name or adv.local_name, adv.rssi, d.address) for d, adv in found.values()
            if looks_like_cmf(d.name or adv.local_name)]


@asynccontextmanager
async def gatt_over_ble(address: str | None, name: str = NAME, timeout: float = 20.0):
    target = address
    if target is None:
        target = await BleakScanner.find_device_by_filter(
            lambda d, adv: (d.name or adv.local_name or "").lower() == name.lower(), timeout=timeout)
        if target is None:
            raise ConnectionError(f"no advertisement named {name!r}")
    async with BleakClient(target, timeout=timeout) as client:
        yield Gatt(client)
