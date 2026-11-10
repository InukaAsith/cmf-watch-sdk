import subprocess
import sys
import time
from pathlib import Path

from psram_probe import GDB_PORT, QMP_PORT, Qmp, gdb

from wp2apps import paths

SCRATCH = 0x386FF000
EXPECTED = {(980, 30), (1000, 30)}
CALLS = (("level 1 preview", 0x25, 1, 0x100E17CD), ("level 0 preview", 0x25, 0, 0x100E17CD),
         ("SETMOTOR 1", 0x2A, 1, 0x101B9AB1))


def call(handler, scratch, value):
    regs = [f"r{i}" for i in range(13)] + ["sp", "lr", "pc", "xpsr"]
    stub = scratch + 0x10
    return [*(f"set $saved_{r} = ${r}" for r in regs),
            f"set *(int *){scratch:#x} = {value}",
            f"set *(unsigned short *){stub:#x} = 0xe7fe",
            f"set $r0 = {scratch:#x}", "set $r1 = 0", "set $r2 = 0",
            "set $sp = ((unsigned) $sp - 256) & ~7",
            f"set $lr = {stub | 1:#x}",
            f"set $pc = {handler & ~1:#x}",
            "set $xpsr = $xpsr | 0x01000000",
            f"tbreak *{stub:#x}", "continue", "printf \"returned %d\\n\", $r0",
            *(f"set ${r} = $saved_{r}" for r in regs)]


def main():
    worktree, qemu = Path(sys.argv[1]).resolve(), Path(sys.argv[2]).resolve()
    cmf = paths.cmf_watch_utils()
    out = paths.ROOT / "runs" / "recon" / f"motor-{time.strftime('%H%M%S')}"
    vm = subprocess.Popen(["uv", "run", "--locked", "cmf-watch-utils", "emu", "run", "--headless", "--allow-reboot",
                           "--fixtures", str(cmf / "fixtures"), "--qemu", str(qemu),
                           "--rom", str(cmf / "runtime/compat.bin"), "--qmp-port", str(QMP_PORT),
                           "--seconds", "300", "--output", str(out)], cwd=worktree)
    qmp = Qmp(QMP_PORT)
    try:
        qmp.hmp(f"gdbserver tcp::{GDB_PORT}")
        for _ in range(3):
            time.sleep(4)
            gdb("set $c = *(unsigned *)0x38037d14",
                "if $c >= 0x38000000 && $c < 0x38800000\n set *(unsigned *)$c = 0xffffffff\n end")
        time.sleep(15)
        for label, prop, value, handler in CALLS:
            print(label, flush=True)
            gdb(*call(handler, SCRATCH, value))
            time.sleep(3)
    finally:
        qmp.request("quit")
        vm.wait(timeout=30)
    log = (out / "motor.log").read_text()
    print(log)
    seen = {tuple(map(int, line.split()[1:])) for line in log.splitlines() if line.split()[1] != "0"}
    missing = EXPECTED - seen
    if missing:
        sys.exit(f"motor check FAILED: expected {sorted(missing)} in motor.log, saw {sorted(seen)}")
    print("motor check OK")


if __name__ == "__main__":
    main()
