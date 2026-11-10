import json
from pathlib import Path
import random
import socket
import subprocess
import sys
import time

from wp2apps import paths

BASE, SIZE = 0x38000000, 0x800000
MARK = bytes.fromhex("a55ac33c")
QMP_PORT, GDB_PORT = 45551, 45552


class Qmp:
    def __init__(self, port):
        for _ in range(100):
            try:
                self.sock = socket.create_connection(("127.0.0.1", port))
                break
            except OSError:
                time.sleep(0.1)
        self.stream = self.sock.makefile("rwb", buffering=0)
        self.stream.readline()
        self.request("qmp_capabilities")

    def request(self, command, **arguments):
        self.stream.write(json.dumps({"execute": command, "arguments": arguments}).encode() + b"\n")
        while True:
            reply = json.loads(self.stream.readline())
            if "return" in reply or "error" in reply:
                return reply

    def hmp(self, line):
        return self.request("human-monitor-command", **{"command-line": line})


def gdb(*commands):
    script = paths.ROOT / "runs" / "recon" / "probe.gdb"
    script.write_text("\n".join(["set architecture armv7e-m", f"target remote :{GDB_PORT}", *commands, "detach"]) + "\n")
    result = subprocess.run(["gdb", "-batch", "-x", str(script)], capture_output=True, text=True, timeout=60)
    if result.returncode or "Error" in result.stderr:
        raise RuntimeError(f"gdb failed:\n{result.stdout}{result.stderr}")


def walk(events, seconds):
    rng = random.Random(1)
    end = time.monotonic() + seconds
    with events.open("a") as out:
        def send(*lines, pause=1.5):
            for line in lines:
                out.write(line + "\n")
                out.flush()
                time.sleep(0.15)
            time.sleep(pause)
        while time.monotonic() < end:
            send("c 1", "c 0")
            send(*["r 80"] * rng.randint(0, 12))
            send("t 1 233 233", "t 0 233 233", pause=4)
            for _ in range(rng.randint(1, 3)):
                x0, y0, x1, y1 = rng.choice(((380, 233, 80, 233), (80, 233, 380, 233), (233, 380, 233, 80), (233, 80, 233, 380)))
                send(f"t 1 {x0} {y0}", f"t 1 {(x0 + x1) // 2} {(y0 + y1) // 2}", f"t 1 {x1} {y1}", f"t 0 {x1} {y1}", pause=2)
            send("t 1 233 300", "t 0 233 300", pause=3)
            send("c 1", "c 0", "c 1", "c 0", pause=2)


def runs(dump):
    found, start = [], None
    for offset in range(0, len(dump), 0x1000):
        page = dump[offset:offset + 0x1000]
        intact = page == MARK * (0x1000 // 4)
        if intact and start is None:
            start = offset
        if not intact and start is not None:
            found.append((start, offset - start))
            start = None
    if start is not None:
        found.append((start, len(dump) - start))
    return sorted(found, key=lambda r: -r[1])


def main():
    walk_seconds = int(sys.argv[1]) if len(sys.argv) > 1 else 180
    cmf = paths.cmf_watch_utils()
    out = paths.ROOT / "runs" / "recon" / f"probe-{time.strftime('%H%M%S')}"
    pattern = out.parent / "psram-mark.bin"
    pattern.write_bytes(MARK * (SIZE // 4))
    vm = subprocess.Popen(["uv", "run", "--locked", "cmf-watch-utils", "emu", "run", "--headless", "--allow-reboot",
                           "--fixtures", str(cmf / "fixtures"), "--qemu", str(cmf / "runtime/qemu-system-arm"),
                           "--rom", str(cmf / "runtime/compat.bin"), "--qmp-port", str(QMP_PORT),
                           "--seconds", str(walk_seconds + 300), "--output", str(out)], cwd=paths.ROOT)
    qmp = Qmp(QMP_PORT)
    qmp.request("stop")
    qmp.hmp(f"gdbserver tcp::{GDB_PORT}")
    gdb(f"restore {pattern} binary {BASE:#x}")
    qmp.request("system_reset")
    qmp.request("cont")
    for _ in range(3):
        time.sleep(4)
        gdb("set $c = *(unsigned *)0x38037d14",
            "if $c >= 0x38000000 && $c < 0x38800000\n set *(unsigned *)$c = 0xffffffff\n end")
    qmp.hmp("gdbserver none")
    walk(out / "input.txt", walk_seconds)
    qmp.request("stop")
    dump = out / "psram.bin"
    print(qmp.hmp(f"pmemsave {BASE:#x} {SIZE:#x} \"{dump}\""), flush=True)
    time.sleep(2)
    qmp.request("quit")
    vm.wait(timeout=30)
    data = dump.read_bytes()
    regions = runs(data)
    print(json.dumps({"run": str(out), "untouched_total_kib": sum(n for _, n in regions) // 1024,
                      "largest": [{"start": f"{BASE + s:#x}", "kib": n // 1024} for s, n in regions[:12]]}, indent=2))


if __name__ == "__main__":
    main()
