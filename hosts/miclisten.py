"""Saves what Mic lab (sdk/examples/miclab) sends after a recording as a WAV (8 kHz mono): run it, then record on
the watch.

  uv run python hosts/miclisten.py --address UUID out.wav     (a real watch; the iPhone's Bluetooth off)
"""
import argparse, asyncio, json, pathlib, sys, wave

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parents[1] / "src"))
from wp2apps import adpcm, channel  # noqa: E402

ap = argparse.ArgumentParser()
ap.add_argument("--address", required=True)
ap.add_argument("--seconds", type=int, default=120)
ap.add_argument("out")
a = ap.parse_args()
blocks = {}


async def run():
    # the K channel, as CMFX: app messages reach a stock-bound Mac only with the authkey, which a confirmed bind
    # popup zeroes
    ch = channel.Channel(address=a.address, channel_key=True)
    try:
        async with asyncio.timeout(a.seconds):
            print("listening", flush=True)
            while True:
                d = (await ch.recv()).data
                if d[:1] == b"a":
                    blocks[int.from_bytes(d[1:3], "little")] = d[3:]
                elif d[:1] == b"e":
                    print(f"got {len(blocks)} of {int.from_bytes(d[1:3], 'little')} blocks", flush=True)
                    return
    except TimeoutError:
        pass
    finally:
        await ch.drop()


asyncio.run(run())
pcm = []
for k in sorted(blocks):
    pcm += adpcm.decode(blocks[k])
with wave.open(a.out, "wb") as w:
    w.setnchannels(1), w.setsampwidth(2), w.setframerate(8000)
    w.writeframes(b"".join(int(v).to_bytes(2, "little", signed=True) for v in pcm))
peak = max((abs(v) for v in pcm), default=0)
rms = (sum(v * v for v in pcm) / max(len(pcm), 1)) ** 0.5
print(json.dumps({"samples": len(pcm), "seconds": len(pcm) / 8000, "peak": peak, "rms": round(rms)}))
