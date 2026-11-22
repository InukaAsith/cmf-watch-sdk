"""Pipelined host -> app: W MSG frames in flight, replies matched in order."""
import asyncio, os, time, unittest, collections
from tests.e2e.harness import E2E, installed, open_app
from tests.test_throughput_e2e import bind, picture
from wp2apps import link
from wp2apps.link import SYSTEM, CMD_W, frame, decrypt
from wp2apps.channel import Channel, MSG

class PipeTest(unittest.TestCase):
    def test_pipe(self):
        with installed(["loader", "pixview"], name="pipe") as (emu, port):
            key = asyncio.run(bind(port))
            open_app(emu, 0)
            hdr, data = picture()
            msgs = [hdr] + [b"D" + bytes([s & 0xFF]) + data[i:i+126] for s, i in enumerate(range(0, len(data), 126))] + [b"E"]
            async def run(W):
                ch = Channel(hci_port=port); ch.key = key
                async with ch:
                    s = await ch.connect()
                    stats = collections.Counter()
                    t0 = time.monotonic(); i = 0
                    while i < len(msgs):
                        batch = msgs[i:i+W]
                        for m in batch:
                            await s.gatt.write(CMD_W, frame(SYSTEM, MSG, b"\xff" + m, s.key), response=False)
                        st = []
                        try:
                          async with asyncio.timeout(5):
                            while len(st) < len(batch):
                                c1, c2, body = await s.frames.get()
                                if c2 == MSG | 0xA000:
                                    st.append(decrypt(s.key, body)[0])
                        except TimeoutError:
                            print(f"\nLOST at msg {i}: got {st} of {len(batch)}; uart tail:", emu.uart()[-600:], flush=True); raise
                        ok = 0
                        for x in st:
                            if x != 0: break
                            ok += 1
                        stats["full"] += len(st) - ok
                        stats["ooo"] += sum(1 for x in st[ok:] if x == 0)  # accepted after a refused one
                        i += ok
                        if ok < len(batch): await asyncio.sleep(0.005)
                    sent = time.monotonic() - t0
                    while True:
                        r = (await ch.recv(timeout=60)).data
                        if r.startswith(b"K") or r.startswith(b"G"):
                            return sent, time.monotonic() - t0, r, stats
            W = int(os.environ.get("W", "4"))
            sent, total, r, stats = asyncio.run(run(W))
            print(f"\nW={W}: {len(data)} B sent {sent:.2f}s drawn {total:.2f}s {len(data)/total:.0f} B/s {r!r} {dict(stats)}", flush=True)
