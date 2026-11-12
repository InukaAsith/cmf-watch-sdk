"""Every example in the host simulator (scripts/sim, docs/app-guide.md): headless, scripted input, frames are not
empty, BUTTON ends the app (or the loader's 1 s escape stops the ones that ignore it)."""
import asyncio
import json
import shutil
import socket
import struct
import subprocess
import tempfile
import unittest
import zlib
from pathlib import Path

from wp2apps.channel import SimChannel

ROOT = Path(__file__).resolve().parent.parent
SCRIPT = "wait 300; t 233 233; wait 300; c 1; wait 300; c -1; wait 300; m hi; wait 300; shot mid; b"
# end reason, draws something. LVGL apps (lv-demo, herdr-agents) need the watch's LVGL: not in the simulator yet
APPS = {"hello": ("exit", True), "vibration-lab": ("exit", True), "agent-glow": ("exit", True),
        "api-demo": ("exit", True),
        "motor-hog": ("killed", False), "motor-hold": ("killed", True)}


def lit_pixels(png: Path) -> int:
    """Non-black pixels of an unfiltered RGB8 PNG as wp2-sim writes it."""
    data, pos, idat = png.read_bytes(), 8, b""
    while pos < len(data):
        n, kind = struct.unpack(">I4s", data[pos:pos + 8])
        if kind == b"IHDR":
            w, h = struct.unpack(">II", data[pos + 8:pos + 16])
        elif kind == b"IDAT":
            idat += data[pos + 8:pos + 8 + n]
        pos += 12 + n
    raw, stride = zlib.decompress(idat), 1 + 3 * w
    rows = [raw[y * stride + 1:(y + 1) * stride] for y in range(h)]
    assert all(raw[y * stride] == 0 for y in range(h)), "filtered PNG"
    return sum(any(row[x:x + 3]) for row in rows for x in range(0, 3 * w, 3))


@unittest.skipUnless(shutil.which("cargo"), "needs cargo")
class SimTest(unittest.TestCase):
    def test_every_example(self):
        with tempfile.TemporaryDirectory() as tmp:
            for app, (reason, draws) in APPS.items():
                with self.subTest(app=app):
                    out = Path(tmp) / app
                    run = subprocess.run([ROOT / "scripts/sim", app, "--headless", "--store", out, "--frames", out,
                                          "--do", SCRIPT], capture_output=True, text=True, timeout=600)
                    log = [json.loads(line) for line in (out / "log.jsonl").read_text().splitlines()]
                    end = log[-1] if log[-1]["ev"] == "end" else next(r for r in log if r["ev"] == "end")
                    self.assertEqual(end["reason"], reason, run.stderr)
                    self.assertEqual(run.returncode, 0 if reason == "exit" else 2)
                    if reason == "exit":
                        self.assertEqual(end["code"], 0)
                        self.assertLess(end["t"], 1200 + 1000)
                    if draws:
                        self.assertGreater(lit_pixels(out / "mid.png"), 500)
                        self.assertGreater(end["shows"], 0)
                    self.assertLess(end["wall_ms"], 10_000, "virtual clock: a 1.5 s script runs faster than real time")

    def test_channel_over_tcp(self):
        with socket.socket() as s:
            s.bind(("127.0.0.1", 0))
            port = s.getsockname()[1]
        with tempfile.TemporaryDirectory() as tmp:
            sim = subprocess.Popen([ROOT / "scripts/sim", "api-demo", "--realtime", "--store", tmp, "--tcp", str(port),
                                    "--until", "60000"], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            try:
                async def talk():
                    async with SimChannel(port) as ch:
                        await ch.send("эхо".encode())
                        first = await ch.recv(timeout=30)
                        await ch.send(b"!count")
                        return first.data, (await ch.recv(timeout=5)).data
                self.assertEqual(asyncio.run(asyncio.wait_for(talk(), 120)), ("эхо".encode(), b"count 0"))
            finally:
                sim.kill()
                sim.wait()


if __name__ == "__main__":
    unittest.main()
