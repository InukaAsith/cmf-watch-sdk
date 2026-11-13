"""Heart-rate recon in the emulator (docs/recon.md, "Heart rate"); synthetic values only, frames in runs/hr/.

  nix-shell -p gdb llvm --run 'uv run --locked python -m scripts.recon.hr_probe heart|strength'

heart: the stock heart view 0x14, a poked reading, then hr_callback(5, {0, 72}). strength: Strength training's GO
screen and run screen with a poked reading. Needs target/app-wp2.bin (scripts/build-fw)."""
import re
import struct
import sys

from tests.e2e import harness
from tests.test_demo_e2e import save_png
from wp2apps import paths

OUT = paths.ROOT / "runs" / "hr"
WEAR, OWNER, BPM, REQUESTS = 0x3804B30C, 0x3804B30F, 0x3804B310, 0x38048D16 + 5
SPORT_HR = 0x3800CABC
HR_CALLBACK = 0x100E2EE9
DATA = harness.STUB_RAM + 0xF00
HEART_VIEW, SPORT_LIST = 0x14, 0x6A
STRENGTH = (200, 198)  # Strength training after 7 swipes up the sport list (runs/hr/s2-7.png)
GO = (233, 243)


def state(emu, tag, since):
    print(f"[{tag}] wear={emu.peek(WEAR, 1)[0]} owner={emu.peek(OWNER, 1)[0]} requests={emu.peek(REQUESTS, 1)[0]} "
          f"bpm={emu.peek(BPM, 1)[0]} sport={emu.peek(SPORT_HR, 1)[0]} view={harness.focused(emu)}", flush=True)
    for line in emu.uart()[since:].splitlines():
        if re.search(r"change_hr_mode|heart senor|VC9202 St|algo_ret 5\)", line):
            print("   ", line.strip(), flush=True)
    return len(emu.uart())


def shot(emu, name):
    emu.sleep(0.5)
    w, h, rgb = emu.frame()
    save_png(OUT / f"{name}.png", w, h, rgb)


def tap(emu, xy):
    emu.send(f"t 1 {xy[0]} {xy[1]}", f"t 0 {xy[0]} {xy[1]}", pause=2)


def heart(emu, mark):
    emu.call(harness.OPEN_VIEW, HEART_VIEW, 0)
    emu.sleep(3)
    mark = state(emu, "heart view", mark)
    shot(emu, "1-heart-open")
    emu.poke(BPM, bytes([67]))
    emu.sleep(3)
    mark = state(emu, "poked 67", mark)
    shot(emu, "2-heart-67")
    emu.poke(DATA, struct.pack("<II", 0, 72))
    emu.call(HR_CALLBACK, 5, DATA, thread=b"ui_s")  # a big stack; another thread stopped the guest once
    emu.sleep(3)
    mark = state(emu, "hr_callback 72", mark)
    shot(emu, "3-heart-72")
    emu.send("c 1", "c 0", pause=3)
    state(emu, "closed", mark)


def strength(emu, mark):
    emu.call(harness.OPEN_VIEW, SPORT_LIST, 0)
    emu.sleep(2)
    for _ in range(7):
        harness.swipe(emu, 233, 380, 233, 120, pause=1.5)
    tap(emu, STRENGTH)
    mark = state(emu, "GO screen", mark)
    emu.poke(BPM, bytes([67]))
    emu.sleep(2)
    shot(emu, "s3-4")
    tap(emu, GO)
    emu.sleep(6)
    mark = state(emu, "running", mark)
    emu.poke(BPM, bytes([68]))
    emu.poke(SPORT_HR, bytes([69]))
    emu.send("c 1", "c 0", pause=1)  # wakes the panel
    shot(emu, "s4-run")
    state(emu, "poked", mark)


def main(what):
    OUT.mkdir(parents=True, exist_ok=True)
    with harness.Emu("hr-probe", nor=harness.patched_nor()) as emu:
        emu.wait_uart("launcher enter", 90)
        emu.keep_awake()
        emu.sleep(5)
        mark = state(emu, "boot", 0)
        {"heart": heart, "strength": strength}[what](emu, mark)


if __name__ == "__main__":
    main(sys.argv[1] if len(sys.argv) > 1 else "heart")
