"""GUI (LVGL) heap on stock 1.0.0.73 in the emulator, screen by screen (docs/recon-lvgl.md).

Walks the stock UI, and on every screen dumps the Zephyr sys_heap behind ui_mem type 1 and reports free
bytes and the largest free chunk; also checks which thread runs lv_timer_handler and the display theme.
Frames of every step land in runs/recon/gui-heap-*/. Optional argument: a fixtures directory (the e2e
heavy face needs the QEMU it was made with, cmf-watch-utils-motor-log/runtime-motor). Run:
CMF_WATCH_UTILS=… WP2_QEMU=…/qemu-system-arm nix-shell -p gdb --run \
    'uv run --locked python scripts/recon/gui_heap_probe.py [fixtures]'
"""
import json
from pathlib import Path
import shutil
import struct
import sys
import time

sys.path.insert(0, str(Path(__file__).resolve().parents[2]))
from tests.e2e.harness import Emu, stock_nor, swipe, u32  # noqa: E402

from wp2apps import paths  # noqa: E402

GUI_HEAP = 0x3807F300  # struct sys_heap {z_heap *, init_mem, init_bytes}; only the pointer is set
GUI_SIZE = 0x18000  # ui_mem type 1 init 0x10188630: sys_heap_init(&heap, 0x38067300, 0x18000)
LV_TIMER_HANDLER = 0x1013B1D0
GUI_OWNER = 0x38043894  # first thread that allocated from GUI; gui_malloc warns on any other
DISP_DEF = 0x3802AF24  # _lv_disp_def; theme at disp + 8
UI_STACK = (0x2FFB6740, 0x4000)  # ui_service in the static thread table 0x10000464
OUT = paths.ROOT / "runs" / "recon" / f"gui-heap-{time.strftime('%H%M%S')}"


def walk_heap(mem: bytes, base: int) -> dict:
    """Zephyr small sys_heap (< 0x7FFF chunks): 8-byte chunks, u16 header fields {left_size, size_and_used}."""
    end_chunk = struct.unpack_from("<I", mem, 8)[0]
    c, free, biggest, used, n = 0, 0, 0, 0, 0
    while c < end_chunk:
        size_used = struct.unpack_from("<H", mem, c * 8 + 2)[0]
        size, inuse = size_used >> 1, size_used & 1
        if size == 0:
            raise RuntimeError(f"zero-size chunk {c} at {base + c * 8:#x}")
        if inuse:
            used += size * 8
        else:
            free += size * 8 - 4
            biggest = max(biggest, size * 8 - 4)
        c, n = c + size, n + 1
    if c != end_chunk:
        raise RuntimeError(f"chunk walk ended at {c}, end_chunk {end_chunk}")
    return {"free": free, "max_block": biggest, "used": used, "chunks": n,
            "hdr": [f"{w:#x}" for w in struct.unpack_from("<8I", mem, 0)]}


def measure(emu: Emu, name: str, rows: list):
    heap_ptr = u32(emu, GUI_HEAP)
    mem = emu.peek(heap_ptr, GUI_SIZE)
    row = {"screen": name, **walk_heap(mem, heap_ptr)}
    shutil.copy(emu.run / "frame.ppm", OUT / f"{len(rows):02d}-{name}.ppm")
    print(json.dumps(row), flush=True)
    rows.append(row)


def main():
    OUT.mkdir(parents=True, exist_ok=True)
    rows = []
    fixtures = Path(sys.argv[1]) if len(sys.argv) > 1 else None  # e.g. the e2e heavy-face fixtures
    with Emu("gui-heap", nor=stock_nor(), fixtures=fixtures, seconds=1500) as emu:
        emu.wait_uart("launcher enter", 60)
        emu.keep_awake()
        emu.sleep(5)
        out = emu.gdb(f"break *{LV_TIMER_HANDLER:#x}", "continue", 'printf "sp %x\\n", $sp',
                      f'printf "owner %x\\n", *(unsigned *){GUI_OWNER:#x}', "delete")
        sp = int(out.split("sp ")[-1].split()[0], 16)
        disp = u32(emu, DISP_DEF)
        print(json.dumps({"lv_timer_handler_sp": f"{sp:#x}",
                          "on_ui_service_stack": UI_STACK[0] <= sp < UI_STACK[0] + UI_STACK[1],
                          "gui_owner_thread": out.split("owner ")[-1].split()[0],
                          "disp": f"{disp:#x}", "theme": f"{u32(emu, disp + 8):#x}"}), flush=True)

        def step(name, *lines, pause=3):
            emu.send(*lines, pause=pause)
            measure(emu, name, rows)

        # transitions as seen in the frames of a first run: the crown toggles face <-> app list; list rows at
        # y = 130, 233, 336. Later steps drift (a scroll swipe may open an item): check the frames before quoting a row
        measure(emu, "face", rows)
        for y, name in ((233, "exercise-records"), (130, "exercise"), (336, "heart-rate")):
            step("app-list", "c 1", "c 0")
            step(name, f"t 1 233 {y}", f"t 0 233 {y}")
            step("face", "c 1", "c 0")
        swipe(emu, 380, 233, 80, 233)
        measure(emu, "face-swipe-left", rows)
        step("face", "c 1", "c 0")
        swipe(emu, 233, 80, 233, 380)
        measure(emu, "control-centre", rows)
        step("face", "c 1", "c 0")
        swipe(emu, 233, 380, 233, 80)
        measure(emu, "notifications", rows)
        step("face", "c 1", "c 0")
        step("app-list", "c 1", "c 0")
        swipe(emu, 233, 380, 233, 80)
        measure(emu, "app-list-scrolled", rows)
        step("app-list-crown", *["r 80"] * 3, pause=1)
        step("activity", "t 1 233 300", "t 0 233 300")
        swipe(emu, 380, 233, 80, 233)
        measure(emu, "activity-page2", rows)
        step("face", "c 1", "c 0")
        emu.send("t 1 233 233", pause=2.5)
        step("face-picker", "t 0 233 233")
        step("face", "c 1", "c 0")
        boots = emu.uart().count("*** Booting Zephyr")
    worst = min(rows, key=lambda r: r["free"])
    print(json.dumps({"min_free": worst["free"], "at": worst["screen"],
                      "min_max_block": min(r["max_block"] for r in rows), "boot_banners": boots}))
    (OUT / "rows.json").write_text(json.dumps(rows, indent=1))


if __name__ == "__main__":
    main()
