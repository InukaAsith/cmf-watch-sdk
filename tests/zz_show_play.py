"""Live window: new loader + Speaker, opened, then streams a file (argv[1]) or an original synth demo; stays open."""
import asyncio, math, os, random, sys, time
os.environ["WP2_GUI"] = "1"
os.environ["WP2_REALTIME"] = "1"
from pathlib import Path
from tests.e2e import harness
from tests.e2e.harness import Emu, patched_paired_nor, cli_out, open_app
from tests.e2e.netsim import netsim
from tests.test_throughput_e2e import bind
from wp2apps import paths, play
from wp2apps.channel import Channel

R = play.RATE

def demo(seconds=32.0):
    """An original 128 BPM synth loop (A minor): kick, offbeat hats, a saw bass and a square arpeggio."""
    beat = 60 / 128
    n = int(seconds * R)
    out = [0.0] * n
    notes = {"A2": 110.0, "C3": 130.81, "D3": 146.83, "E3": 164.81, "G3": 196.0,
             "A4": 440.0, "C5": 523.25, "D5": 587.33, "E5": 659.25, "G5": 783.99, "A5": 880.0}
    bass = ["A2", "A2", "C3", "C3", "D3", "D3", "E3", "G3"]
    arp = [["A4", "C5", "E5", "A5"], ["C5", "E5", "G5", "E5"], ["D5", "A4", "D5", "E5"], ["E5", "G5", "A5", "G5"]]
    rnd = random.Random(7)
    t_beats = int(seconds / beat)
    for b in range(t_beats):
        start = int(b * beat * R)
        # kick: falling sine
        for i in range(int(0.18 * R)):
            if start + i >= n: break
            t = i / R
            out[start + i] += 0.9 * math.sin(2 * math.pi * (50 + 120 * math.exp(-t * 30)) * t) * math.exp(-t * 14)
        # hat on the offbeat
        hs = start + int(beat / 2 * R)
        for i in range(int(0.04 * R)):
            if hs + i >= n: break
            out[hs + i] += 0.12 * (rnd.random() * 2 - 1) * math.exp(-i / R * 90)
        # saw bass, eighth notes
        f = notes[bass[(b // 2) % len(bass)]]
        for half in range(2):
            s0 = start + int(half * beat / 2 * R)
            for i in range(int(beat / 2 * R * 0.9)):
                if s0 + i >= n: break
                ph = (f * (s0 + i) / R) % 1.0
                out[s0 + i] += 0.28 * (2 * ph - 1) * math.exp(-i / R * 6)
        # square arpeggio, sixteenths, from bar 4
        if b >= 16:
            chord = arp[(b // 4) % len(arp)]
            for q in range(4):
                s0 = start + int(q * beat / 4 * R)
                f = notes[chord[q]]
                for i in range(int(beat / 4 * R * 0.8)):
                    if s0 + i >= n: break
                    out[s0 + i] += 0.16 * (1 if (f * (s0 + i) / R) % 1.0 < 0.5 else -1) * math.exp(-i / R * 10)
    peak = max(abs(x) for x in out) or 1
    return [int(x / peak * 26000) for x in out]

path = Path(sys.argv[1]).expanduser() if len(sys.argv) > 1 else None
samples = play.pcm16k(path) if path else demo()
title = path.stem if path else "Synth demo (original)"
with netsim() as port, Emu("show", nor=patched_paired_nor(), hci_port=port, seconds=3600) as emu:
    emu.wait_uart("launcher enter", 120)
    emu.keep_awake()
    for n in ["loader", "speaker"]:
        print("installing", n, flush=True)
        cli_out(["install", paths.ROOT / "target" / f"{n}.wp2app", "--hci-port", port])
    key = asyncio.run(bind(port))
    open_app(emu, 0)
    print(f"PLAYING {title}: {len(samples) / R:.1f} s", flush=True)

    async def run():
        ch = Channel(hci_port=port); ch.key = key
        async with ch:
            return await play.play(await ch.connect(), samples, title, volume=80)

    print("RESULT", asyncio.run(run()), flush=True)
    uart = emu.uart()
    print("fatal" if "ZEPHYR FATAL ERROR" in uart else "no fault", "| boots:", uart.count("*** Booting Zephyr"), flush=True)
    print("DONE: window stays open, close it when finished", flush=True)
    emu.vm.wait()
