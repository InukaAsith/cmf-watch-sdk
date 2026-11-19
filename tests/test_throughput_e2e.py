"""Host → app throughput through the loader (stock vs the spill queue in fw/wp2-loader/src/svc.rs).

  WP2_E2E=1 WP2_LOADER=loader-stock uv run --locked python -m unittest tests.test_throughput_e2e -v
  WP2_E2E=1 WP2_LOADER=loader       ...

Streams a 233×233 picture of 2-px runs (≈27 KB, a photo's worst case) to pixview and times it until the app
answers `K` (drawn). Prints bytes, seconds and B/s.
"""
import asyncio, os, struct, time, unittest

from tests.e2e.harness import E2E, installed, open_app
from wp2apps import h4, link
from wp2apps.channel import Channel


async def bind(port):
    """One bind before the app opens; its key is reused, so no "Paired" popup sends the watch to the face."""
    async with h4.gatt_over_h4(port) as gatt:
        s = link.Session()
        await s.handshake(gatt)
        return s.key

N = 233


def picture():
    hdr = b"I" + struct.pack("<HHBhh", N, N, 2, 0, 0) + b"".join(struct.pack("<H", c * 0x1111) for c in range(16))
    row = bytearray()
    for i in range(N // 2):
        row.append((i % 16) << 4 | 1)  # a 2-px run
    row.append(0x00)  # the last px
    data = bytes(row) * N
    return hdr, data


@unittest.skipUnless(E2E, "WP2_E2E=1")
class ThroughputTest(unittest.TestCase):
    def test_picture(self):
        loader = os.environ.get("WP2_LOADER", "loader")
        with installed([loader, "pixview"], name=f"throughput-{loader}") as (emu, port):
            key = asyncio.run(bind(port))
            open_app(emu, 0)  # ready() closes the "Paired" popup of that bind first
            hdr, data = picture()
            msgs = [hdr] + [b"D" + bytes([s & 0xFF]) + data[i : i + 126] for s, i in enumerate(range(0, len(data), 126))]
            msgs.append(b"E")

            async def run():
                ch_ = Channel(hci_port=port)
                ch_.key = key
                async with ch_ as ch:
                    t0 = time.monotonic()
                    for m in msgs:
                        await ch.send(m)
                    sent = time.monotonic() - t0
                    while True:
                        r = (await ch.recv(timeout=120)).data
                        if r.startswith(b"K"):
                            return sent, time.monotonic() - t0, r

            sent, total, r = asyncio.run(run())
            print(f"\n{loader}: {len(data)} B in {len(msgs)} msgs: sent {sent:.1f}s, drawn {total:.1f}s, "
                  f"{len(data) / total:.0f} B/s, watch {r!r}", flush=True)
            self.assertTrue(r.startswith(b"K"))
