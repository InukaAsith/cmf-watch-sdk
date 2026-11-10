from concurrent.futures import ThreadPoolExecutor
from contextlib import contextmanager, redirect_stdout
import io
import json
import os
from pathlib import Path
import shlex
import socket
import struct
import subprocess
import tempfile
import time

from wp2apps import cli as wp2cli, firmware, paths

E2E = os.environ.get("WP2_E2E") == "1"
QEMU = Path(os.environ.get("WP2_QEMU") or paths.ROOT.parent / "cmf-watch-utils-motor-log/runtime-motor/qemu-system-arm")
RUNS = paths.ROOT / "runs" / "e2e"
STUB_RAM = firmware.STUB_RAM
HEAP = 0x38043824
UI_MEM_ALIGNED_ALLOC = 0x101882C5
UI_MEM_FREE = 0x10188381
AWAKE_PTR = 0x38037D14
OS_MUTEX_LOCK = 0x101BE219
OPEN_VIEW = 0x101898BD
FS_FIND_MOUNT = 0x10106FA1
TRACE_OFF = 0x44
LOADER_BLOCK_OFF = 0x4E8
APP_BLOCK_OFF = 0x4F0
ALLOC_LIMIT_OFF = 0x4F8
GUEST_CLOCK = 0x40004118  # low 26 bits: QEMU_CLOCK_VIRTUAL in 125 ms steps (ats3089c.c acts_read)
WALK = "tour1"  # names the walk in cache keys; bump when walk() changes
PARALLEL = 3  # stock control walks at once during heavy-face selection


def fast_qemu(directory: Path, qemu: Path = QEMU) -> Path:
    """QEMU wrapper adding icount sleep=off: idle guest time is skipped instead of slept (1.5x, see e2e-speed-report)."""
    # ponytail: rewrites the CLI's fixed icount string; drop once cmf-watch-utils takes an icount option
    wrapper = directory / "qemu"
    wrapper.write_text('#!/bin/sh\nfor a; do shift; [ "$a" = shift=3,align=off ] && a="$a,sleep=off"; set -- "$@" "$a"; done\n'
                       f"exec {shlex.quote(str(qemu))} \"$@\"\n")
    wrapper.chmod(0o755)
    return wrapper


def guest_seconds(xp: str) -> float:
    return (int(xp.split()[-1], 16) & 0x3FFFFFF) / 8


def free_port() -> int:
    with socket.socket() as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


def stock_nor() -> bytes:
    return (paths.cmf_watch_utils() / "fixtures" / "nor.bin").read_bytes()


def patched_nor() -> bytes:
    """Stock NOR with target/app-wp2.bin (heap reservation and stub) from scripts/build-fw."""
    app = paths.ROOT / "target" / "app-wp2.bin"
    if not app.is_file():
        raise FileNotFoundError(f"{app} is missing: run scripts/build-fw")
    return firmware.stage_nor(stock_nor(), app.read_bytes())


def patched_paired_nor() -> bytes:
    """Paired stock NOR (synthetic secret) with target/app-wp2.bin."""
    from .netsim import paired_nor
    app = paths.ROOT / "target" / "app-wp2.bin"
    if not app.is_file():
        raise FileNotFoundError(f"{app} is missing: run scripts/build-fw")
    return firmware.stage_nor(paired_nor(), app.read_bytes())


def cli(args) -> int:
    return wp2cli.main([str(a) for a in args])


def cli_out(args) -> str:
    out = io.StringIO()
    with redirect_stdout(out):
        code = cli(args)
    if code:
        raise RuntimeError(f"wp2apps {' '.join(map(str, args))} exited with {code}")
    return out.getvalue()


@contextmanager
def installed(names):
    """Booted paired watch on Netsim with target/<name>.wp2app installed for each name; yields (emu, hci port)."""
    from .netsim import netsim
    nor = patched_paired_nor()  # runs its own Netsim: two netsimd on one instance exit
    with netsim() as port, Emu("installed", nor=nor, hci_port=port) as emu:
        emu.wait_uart("launcher enter", 60)
        emu.keep_awake()
        for name in names:
            cli_out(["install", paths.ROOT / "target" / f"{name}.wp2app", "--hci-port", port])
        yield emu, port


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
                "--fixtures", str(self.fixtures), "--qemu", str(fast_qemu(self.inputs)), "--rom", str(cmf / "runtime/compat.bin"),
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

    def clock(self) -> float:
        """Guest virtual time in seconds, 125 ms resolution."""
        return guest_seconds(self.qmp.hmp(f"xp /1wx {GUEST_CLOCK:#x}")["return"])

    def sleep(self, seconds: float):
        """Waits for guest time, so gestures mean the same at any host load."""
        target, end = self.clock() + seconds, time.monotonic() + 30 + seconds * 10
        while self.clock() < target:
            if time.monotonic() > end:
                raise TimeoutError(f"guest clock did not advance {seconds}s")
            time.sleep(0.05)

    def send(self, *lines, pause=0.3):
        """Input lines 125 ms (guest) apart, then `pause` guest seconds."""
        with (self.run / "input.txt").open("a") as out:
            for line in lines:
                out.write(line + "\n")
                out.flush()
                self.sleep(0.125)
        self.sleep(pause)

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
        """Runs fn(*args) in the next thread that enters os_mutex_lock: a plain stop may land in an ISR or
        the idle thread, where stock code that takes a mutex panics (docs/recon.md)."""
        # ponytail: that thread may hold another lock; fine for the short calls the tests make
        regs = [f"r{i}" for i in range(13)] + ["sp", "lr", "pc", "xpsr"]
        spin = STUB_RAM + 0xE00
        out = self.gdb(f"tbreak *{OS_MUTEX_LOCK & ~1:#x}", "continue",
                       *(f"set $saved_{r} = ${r}" for r in regs),
                       f"set *(unsigned short *){spin:#x} = 0xe7fe",
                       *(f"set $r{i} = {a:#x}" for i, a in enumerate(args)),
                       "set $sp = ((unsigned) $sp - 256) & ~7",
                       f"set $lr = {spin | 1:#x}", f"set $pc = {fn & ~1:#x}",
                       "set $xpsr = $xpsr | 0x01000000",
                       f"tbreak *{spin:#x}", "continue", 'printf "returned %u\\n", $r0',
                       *(f"set ${r} = $saved_{r}" for r in regs))
        return int(out.split("returned ")[-1].split()[0])

    def keep_awake(self):
        """The launcher sometimes enters twice at boot and re-arms the timeout, so the write is checked later."""
        for _ in range(10):
            out = self.gdb(f"set $c = *(unsigned *){AWAKE_PTR:#x}",
                           "if $c >= 0x38000000 && $c < 0x38800000\n set *(unsigned *)$c = 0xffffffff\n"
                           ' printf "awake ok\\n"\n end')
            if "awake ok" in out:
                self.sleep(1)
                if u32(self, u32(self, AWAKE_PTR)) == 0xFFFFFFFF:
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
    out = emu.gdb(f"set $free = *(unsigned *){HEAP + 8:#x}", f"set $p = *(unsigned *){HEAP + 0x60:#x}",
                  "set $max = 0", "set $n = 0",
                  "while $p != 0 && $n < 4096\n set $s = *(unsigned *)($p + 4) & 0xffffff\n"
                  " if $s > $max\n  set $max = $s\n end\n set $p = *(unsigned *)$p\n set $n = $n + 1\n end",
                  'printf "heap %u %u %u\\n", $free, $max, $n')
    free, biggest, steps = map(int, out.split("heap ")[-1].split()[:3])
    if steps >= 4096:
        raise RuntimeError("free list does not end")
    return free, biggest


def settle(emu: Emu, timeout: float = 60) -> tuple[int, int]:
    last, end = None, time.monotonic() + timeout
    while time.monotonic() < end:
        now = heap(emu)
        if now == last:
            return now
        last = now
        time.sleep(2)
    raise TimeoutError("heap never settled")


def swipe(emu: Emu, x0, y0, x1, y1, steps=2, pause=2):
    points = [(x0 + (x1 - x0) * i // steps, y0 + (y1 - y0) * i // steps) for i in range(steps + 1)]
    emu.send(*(f"t 1 {x} {y}" for x, y in points), f"t 0 {x1} {y1}", pause=pause)


def walk(emu: Emu):
    """Targeted tour from the watch face, ~60 guest s: every stock view the old 240 s random walk reached.
    Frames of each step: runs/e2e-speed/explore4/sheet.png (e2e-speed-report)."""
    emu.send("t 1 233 233", pause=1.5)
    emu.send("t 0 233 233", pause=3)  # long press: face picker
    emu.send("c 1", "c 0", pause=2)
    swipe(emu, 380, 233, 80, 233)  # widget cards
    swipe(emu, 80, 233, 380, 233)
    swipe(emu, 233, 80, 233, 380)  # control centre
    emu.send("c 1", "c 0", pause=2)
    swipe(emu, 233, 380, 233, 80)  # notifications
    emu.send("c 1", "c 0", pause=2)
    for y in (233, 130):  # Exercise Records, Exercise
        emu.send("c 1", "c 0", pause=2)  # app list
        emu.send(f"t 1 233 {y}", f"t 0 233 {y}", pause=3)
        emu.send("c 1", "c 0", pause=2)  # back to the face
    emu.send("c 1", "c 0", pause=2)
    swipe(emu, 233, 380, 233, 80)  # scroll the list
    emu.send("t 1 233 300", "t 0 233 300", pause=3)  # Activity
    swipe(emu, 380, 233, 80, 233)
    emu.send("c 1", "c 0", pause=2)
    emu.send("c 1", "c 0", pause=2)
    emu.send(*["r 80"] * 3, pause=1)  # crown rotation in the list
    emu.send("c 1", "c 0", pause=2)


def face_fixtures(directory: Path, face_id: str) -> Path:
    cmf = paths.cmf_watch_utils()
    run = RUNS / f"face-{face_id}"
    report = run / "report.json"  # the CLI writes it even on failure; nand.bin alone proves nothing
    if not report.is_file():
        subprocess.run(["uv", "run", "--locked", "cmf-watch-utils", "emu", "run", "--headless", "--fmc-face", face_id,
                        "--fixtures", str(cmf / "fixtures"), "--qemu", str(QEMU), "--rom", str(cmf / "runtime/compat.bin"),
                        "--seconds", "8", "--output", str(run)], cwd=paths.ROOT, check=True)
    if json.loads(report.read_text())["outcome"] != "completed":
        raise subprocess.CalledProcessError(1, f"emu run --fmc-face {face_id}")
    directory.mkdir(parents=True, exist_ok=True)
    links = (("nor.bin", cmf / "fixtures/nor.bin"), ("nand-id.bin", cmf / "fixtures/nand-id.bin"),
             ("nand-defaults.bin", run / "nand.bin"), ("wfc-index.txt", run / "wfc-index.txt"))
    for name, target in links:
        (directory / name).unlink(missing_ok=True)
        (directory / name).symlink_to(target)
    return directory


def control_walk(face_id: str) -> dict:
    cache = RUNS / f"control-{face_id}-{WALK}.json"
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
            walk(emu)
            uart = emu.uart()
        result = {"boots": uart.count("*** Booting Zephyr"), "mllc_failed": uart.count("MllcFailedHook"),
                  "fatal": uart.count("ZEPHYR FATAL ERROR"), "run": emu.run.name}
    cache.write_text(json.dumps(result))
    return result


def heavy_face_fixtures() -> Path:
    fixtures = RUNS / f"fixtures-heavy-{WALK}"
    if (fixtures / "face.json").is_file():
        return fixtures
    listing = subprocess.run(["uv", "run", "--locked", "cmf-watch-utils", "emu", "faces", "--sort", "downloads"],
                             cwd=paths.ROOT, capture_output=True, text=True, check=True).stdout
    faces, rejected = json.loads(listing)[:10], []
    with ThreadPoolExecutor(PARALLEL) as pool:  # PARALLEL walks at a time; the first survivor in download order wins
        for batch in (faces[i:i + PARALLEL] for i in range(0, len(faces), PARALLEL)):
            for face, result in zip(batch, pool.map(control_walk, [face["id"] for face in batch])):
                entry = {"id": face["id"], "name": face["name"], "downloads": face["downloads"], "control": result}
                if (result.get("boots"), result.get("mllc_failed"), result.get("fatal")) == (1, 0, 0):
                    face_fixtures(fixtures, face["id"])
                    (fixtures / "face.json").write_text(json.dumps({**entry, "walk": WALK, "rejected": rejected},
                                                                   indent=2))
                    return fixtures
                rejected.append(entry)
    raise RuntimeError(f"no top-10 face survives the stock walk: {rejected}")
