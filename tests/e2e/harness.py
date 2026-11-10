import json
import os
from pathlib import Path
import random
import socket
import struct
import subprocess
import tempfile
import time

from wp2apps import firmware, paths

E2E = os.environ.get("WP2_E2E") == "1"
QEMU = Path(os.environ.get("WP2_QEMU") or paths.ROOT.parent / "cmf-watch-utils-motor-log/runtime-motor/qemu-system-arm")
RUNS = paths.ROOT / "runs" / "e2e"
STUB_RAM = firmware.STUB_RAM
HEAP = 0x38043824
UI_MEM_ALIGNED_ALLOC = 0x101882C5
UI_MEM_FREE = 0x10188381
AWAKE_PTR = 0x38037D14


def free_port() -> int:
    with socket.socket() as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


def stock_nor() -> bytes:
    return (paths.cmf_watch_utils() / "fixtures" / "nor.bin").read_bytes()


class Qmp:
    def __init__(self, port: int):
        for _ in range(200):
            try:
                self.sock = socket.create_connection(("127.0.0.1", port))
                break
            except OSError:
                time.sleep(0.1)
        else:
            raise RuntimeError(f"QMP port {port} did not open")
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


class Emu:
    def __init__(self, name: str, *, nor: bytes, fixtures: Path | None = None, hci_port: int | None = None,
                 seconds: int = 900):
        stamp = time.strftime("%H%M%S")
        self.run = RUNS / f"{name}-{stamp}"
        self.inputs = RUNS / f"{name}-{stamp}-in"
        self.nor, self.hci_port, self.seconds = nor, hci_port, seconds
        self.fixtures = fixtures or paths.cmf_watch_utils() / "fixtures"
        self.qmp_port, self.gdb_port = free_port(), free_port()
        self.vm = self.qmp = None

    def __enter__(self):
        cmf = paths.cmf_watch_utils()
        self.inputs.mkdir(parents=True)
        (self.inputs / "in-nor.bin").write_bytes(self.nor)
        argv = ["uv", "run", "--locked", "cmf-watch-utils", "emu", "run", "--headless", "--allow-reboot",
                "--fixtures", str(self.fixtures), "--qemu", str(QEMU), "--rom", str(cmf / "runtime/compat.bin"),
                "--nor", str(self.inputs / "in-nor.bin"), "--qmp-port", str(self.qmp_port),
                "--seconds", str(self.seconds), "--output", str(self.run)]
        if self.hci_port:
            argv += ["--hci-port", str(self.hci_port)]
        self.vm = subprocess.Popen(argv, cwd=paths.ROOT)
        self.qmp = Qmp(self.qmp_port)
        return self

    def __exit__(self, *exc):
        try:
            self.qmp.request("quit")
        except (OSError, ValueError):
            pass
        try:
            self.vm.wait(timeout=30)
        except subprocess.TimeoutExpired:
            self.vm.terminate()
            self.vm.wait(timeout=10)

    def send(self, *lines, pause=0.3):
        with (self.run / "input.txt").open("a") as out:
            for line in lines:
                out.write(line + "\n")
                out.flush()
                time.sleep(0.15)
        time.sleep(pause)

    def gdb(self, *commands) -> str:
        self.qmp.hmp(f"gdbserver tcp::{self.gdb_port}")
        script = self.run / "e2e.gdb"
        script.write_text("\n".join(["set architecture armv7e-m", f"target remote :{self.gdb_port}",
                                     *commands, "detach"]) + "\n")
        try:
            result = subprocess.run(["gdb", "-batch", "-x", str(script)], capture_output=True, text=True, timeout=120)
        finally:
            self.qmp.hmp("gdbserver none")
        if result.returncode or "Error" in result.stderr:
            raise RuntimeError(f"gdb failed:\n{result.stdout}{result.stderr}")
        return result.stdout

    def peek(self, addr: int, size: int) -> bytes:
        dump = self.run / "peek.bin"
        self.qmp.request("stop")
        try:
            reply = self.qmp.request("pmemsave", val=addr, size=size, filename=str(dump))
            if "error" in reply:
                raise RuntimeError(reply["error"])
        finally:
            self.qmp.request("cont")
        for _ in range(100):
            if dump.is_file() and dump.stat().st_size == size:
                break
            time.sleep(0.1)
        return dump.read_bytes()

    def poke(self, addr: int, data: bytes):
        with tempfile.NamedTemporaryFile(dir=self.run, suffix=".bin") as f:
            f.write(data)
            f.flush()
            self.gdb(f"restore {f.name} binary {addr:#x}")

    def call(self, fn: int, *args: int) -> int:
        regs = [f"r{i}" for i in range(13)] + ["sp", "lr", "pc", "xpsr"]
        spin = STUB_RAM + 0xE00
        out = self.gdb(*(f"set $saved_{r} = ${r}" for r in regs),
                       f"set *(unsigned short *){spin:#x} = 0xe7fe",
                       *(f"set $r{i} = {a:#x}" for i, a in enumerate(args)),
                       "set $sp = ((unsigned) $sp - 256) & ~7",
                       f"set $lr = {spin | 1:#x}", f"set $pc = {fn & ~1:#x}",
                       "set $xpsr = $xpsr | 0x01000000",
                       f"tbreak *{spin:#x}", "continue", 'printf "returned %u\\n", $r0',
                       *(f"set ${r} = $saved_{r}" for r in regs))
        return int(out.split("returned ")[-1].split()[0])

    def keep_awake(self):
        for _ in range(10):
            out = self.gdb(f"set $c = *(unsigned *){AWAKE_PTR:#x}",
                           "if $c >= 0x38000000 && $c < 0x38800000\n set *(unsigned *)$c = 0xffffffff\n"
                           ' printf "awake ok\\n"\n end')
            if "awake ok" in out:
                return
            time.sleep(2)
        raise RuntimeError("auto-sleep timeout pointer never became valid")

    def frame(self) -> tuple[int, int, bytes]:
        data = (self.run / "frame.ppm").read_bytes()
        magic, size, maxval, pixels = data.split(b"\n", 3)
        w, h = map(int, size.split())
        return w, h, pixels[:w * h * 3]

    def uart(self) -> str:
        path = self.run / "uart0.log"
        return path.read_text(errors="replace") if path.is_file() else ""

    def wait_uart(self, needle: str, timeout: float):
        end = time.monotonic() + timeout
        while time.monotonic() < end:
            if needle in self.uart():
                return
            time.sleep(0.5)
        raise TimeoutError(f"{needle!r} not seen on UART within {timeout}s")

    def motor(self) -> list[tuple[int, int, int]]:
        path = self.run / "motor.log"
        rows = [line.split() for line in path.read_text().splitlines()] if path.is_file() else []
        return [(int(a), int(b), int(c)) for a, b, c in rows]


def u32(emu: Emu, addr: int) -> int:
    return struct.unpack("<I", emu.peek(addr, 4))[0]


def heap(emu: Emu) -> tuple[int, int]:
    free = u32(emu, HEAP + 8)
    biggest, p = 0, u32(emu, HEAP + 0x60)
    for _ in range(4096):
        if not p:
            return free, biggest
        biggest = max(biggest, u32(emu, p + 4) & 0xFFFFFF)
        p = u32(emu, p)
    raise RuntimeError("free list does not end")


def settle(emu: Emu, timeout: float = 60) -> tuple[int, int]:
    last, end = None, time.monotonic() + timeout
    while time.monotonic() < end:
        now = heap(emu)
        if now == last:
            return now
        last = now
        time.sleep(2)
    raise TimeoutError("heap never settled")


def swipe(emu: Emu, x0, y0, x1, y1, steps=4, pause=2):
    points = [(x0 + (x1 - x0) * i // steps, y0 + (y1 - y0) * i // steps) for i in range(steps + 1)]
    emu.send(*(f"t 1 {x} {y}" for x, y in points), f"t 0 {x1} {y1}", pause=pause)


def walk(emu: Emu, seconds: float):
    rng = random.Random(1)
    end = time.monotonic() + seconds
    while time.monotonic() < end:
        emu.send("c 1", "c 0", pause=1.5)
        emu.send(*["r 80"] * rng.randint(0, 12), pause=1.5)
        emu.send("t 1 233 233", "t 0 233 233", pause=4)
        for _ in range(rng.randint(1, 3)):
            swipe(emu, *rng.choice(((380, 233, 80, 233), (80, 233, 380, 233), (233, 380, 233, 80), (233, 80, 233, 380))),
                  steps=2)
        emu.send("t 1 233 300", "t 0 233 300", pause=3)
        emu.send("c 1", "c 0", "c 1", "c 0", pause=2)
        emu.send("t 1 233 233", pause=1.5)
        emu.send("t 0 233 233", pause=4)


def face_fixtures(directory: Path, face_id: str) -> Path:
    cmf = paths.cmf_watch_utils()
    run = RUNS / f"face-{face_id}"
    if not (run / "nand.bin").is_file():
        subprocess.run(["uv", "run", "--locked", "cmf-watch-utils", "emu", "run", "--headless", "--fmc-face", face_id,
                        "--fixtures", str(cmf / "fixtures"), "--qemu", str(QEMU), "--rom", str(cmf / "runtime/compat.bin"),
                        "--seconds", "8", "--output", str(run)], cwd=paths.ROOT, check=True)
    directory.mkdir(parents=True, exist_ok=True)
    links = (("nor.bin", cmf / "fixtures/nor.bin"), ("nand-id.bin", cmf / "fixtures/nand-id.bin"),
             ("nand-defaults.bin", run / "nand.bin"), ("wfc-index.txt", run / "wfc-index.txt"))
    for name, target in links:
        (directory / name).unlink(missing_ok=True)
        (directory / name).symlink_to(target)
    return directory


def control_walk(face_id: str, seconds: int = 240) -> dict:
    cache = RUNS / f"control-{face_id}.json"
    if cache.is_file():
        return json.loads(cache.read_text())
    try:
        fixtures = face_fixtures(RUNS / f"fixtures-{face_id}", face_id)
    except subprocess.CalledProcessError:
        result = {"error": "stock does not boot with this face (emu run --fmc-face failed)"}
    else:
        with Emu(f"control-{face_id}", nor=stock_nor(), fixtures=fixtures) as emu:
            emu.wait_uart("launcher enter", 60)
            emu.keep_awake()
            walk(emu, seconds=seconds)
            uart = emu.uart()
        result = {"boots": uart.count("*** Booting Zephyr"), "mllc_failed": uart.count("MllcFailedHook"),
                  "fatal": uart.count("ZEPHYR FATAL ERROR"), "run": emu.run.name}
    cache.write_text(json.dumps(result))
    return result


def heavy_face_fixtures() -> Path:
    fixtures = RUNS / "fixtures-heavy"
    if (fixtures / "face.json").is_file():
        return fixtures
    listing = subprocess.run(["uv", "run", "--locked", "cmf-watch-utils", "emu", "faces", "--sort", "downloads"],
                             cwd=paths.ROOT, capture_output=True, text=True, check=True).stdout
    rejected = []
    for face in json.loads(listing)[:10]:
        result = control_walk(face["id"])
        entry = {"id": face["id"], "name": face["name"], "downloads": face["downloads"], "control": result}
        if (result.get("boots"), result.get("mllc_failed"), result.get("fatal")) == (1, 0, 0):
            face_fixtures(fixtures, face["id"])
            (fixtures / "face.json").write_text(json.dumps({**entry, "rejected": rejected}, indent=2))
            return fixtures
        rejected.append(entry)
    raise RuntimeError(f"no top-10 face survives the stock walk: {rejected}")
