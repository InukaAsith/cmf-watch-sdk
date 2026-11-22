"""Prototype: stock audio_track_* called over gdb in the emulator, speaker recorded to audio.wav."""
import math, struct, subprocess, sys, time, wave
from tests.e2e import harness
from tests.e2e.harness import Emu, STUB_RAM, CURRENT_THREAD, OS_MUTEX_LOCK, patched_nor

CREATE, START, WRITE, STOP, DESTROY, GET_STREAM, SPACE = 0x1014EDEC, 0x1014F3FC, 0x1014F294, 0x1014F1C8, 0x1014F118, 0x1014F554, 0x101CE410
MALLOC, TAG = 0x101CE046, 0x1024338B

class WavEmu(Emu):
    def __enter__(self):
        real = subprocess.Popen
        def popen(argv, **kw):
            i = argv.index("--headless")
            argv = argv[:i + 1] + ["--wav"] + argv[i + 1:]
            return real(argv, **kw)
        subprocess.Popen = popen
        try:
            return super().__enter__()
        finally:
            subprocess.Popen = real

    def call(self, fn, *args, thread=b""):
        regs = [f"r{i}" for i in range(13)] + ["sp", "lr", "pc", "xpsr"]
        spin = STUB_RAM + 0xE00
        stack = [f"set *(unsigned *)($sp + {4 * k}) = {a:#x}" for k, a in enumerate(args[4:])]
        out = self.gdb(f"tbreak *{OS_MUTEX_LOCK & ~1:#x}", "continue",
                       *(f"set $saved_{r} = ${r}" for r in regs),
                       f"set *(unsigned short *){spin:#x} = 0xe7fe",
                       *(f"set $r{i} = {a:#x}" for i, a in enumerate(args[:4])),
                       "set $sp = ((unsigned) $sp - 256) & ~7", *stack,
                       f"set $lr = {spin | 1:#x}", f"set $pc = {fn & ~1:#x}",
                       "set $xpsr = $xpsr | 0x01000000",
                       f"tbreak *{spin:#x}", "continue", 'printf "returned %u\\n", $r0',
                       *(f"set ${r} = $saved_{r}" for r in regs))
        return int(out.split("returned ")[-1].split()[0])

RATE = 16000
pcm = b"".join(struct.pack("<h", int(12000 * math.sin(2 * math.pi * 440 * i / RATE))) for i in range(RATE // 440 * 40 * 10 // 10))
with WavEmu("audio-proto", nor=patched_nor(), seconds=600) as emu:
    emu.wait_uart("launcher enter", 90)
    emu.keep_awake(); emu.sleep(6)
    buf = emu.call(MALLOC, len(pcm), TAG); print("buf", hex(buf), flush=True)
    emu.poke(buf, pcm)
    track = emu.call(CREATE, 5, 16, 1, 1, 0, 0, 0); print("track", hex(track), flush=True)
    stream = emu.call(GET_STREAM, track); print("stream", hex(stream), flush=True)
    print("space before start", emu.call(SPACE, stream), flush=True)
    print("start", emu.call(START, track), flush=True)
    t0 = emu.clock(); written = 0; pos = 0
    while emu.clock() - t0 < 3:
        space = emu.call(SPACE, stream)
        n = min(space, len(pcm) - pos) & ~1
        if n:
            written += emu.call(WRITE, track, buf + pos, n); pos = (pos + n) % len(pcm)
        print(f"t={emu.clock() - t0:.2f} space={space} wrote {written}", flush=True)
    print("stop", emu.call(STOP, track), "destroy", emu.call(DESTROY, track), flush=True)
    emu.sleep(1)
    print("RUN", emu.run, flush=True)
