"""LVGL inventory of stock app.bin (JX402 1.0.0.73) for docs/recon-lvgl.md. No Ghidra needed.

Three sources, each printed as markdown rows `| name | address | how |`:
1. `__func__` strings: LV_USE_ASSERT_OBJ is on, so every asserting LVGL function loads its own name.
   The function is the last `push {…, lr}` before the literal load.
2. Widget classes: `lv_xxx_create` is `lv_obj_class_create_obj(&class, parent)` + `lv_obj_class_init_obj`;
   every `lv_obj_class_t` (0x20 B: base, ctor, dtor, user_data, event_cb, i32 w, i32 h, bits) reachable
   from `lv_obj_class` is listed with its instance size.
3. `lv_obj_set_style_*`: tail calls to `lv_obj_set_local_style_prop` with `movs r1, #prop`.
Usage: uv run python scripts/recon/extract.py; uv run --locked python scripts/recon/lvgl_inventory.py
"""
import re
import struct
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
B = 0x10000000
IMG = (ROOT / "runs/recon/app.bin").read_bytes()
H = struct.unpack(f"<{len(IMG) // 2}H", IMG[: len(IMG) // 2 * 2])

OBJ_CLASS = 0x101FB27C
CREATE_OBJ = 0x10128E70  # lv_obj_class_create_obj, named by its __func__ string
INIT_OBJ = 0x101C5534  # lv_obj_class_init_obj: called right after CREATE_OBJ in every create function
SET_LOCAL_STYLE_PROP = 0x101C6F9A
# LVGL v8.3.0..v8.3.10 lv_style.h numbering (v8.3.11 renumbered); only ids seen in the image
PROPS = {1: "width", 4: "height", 6: "max_height", 7: "x", 8: "y", 9: "align", 10: "layout", 11: "radius",
         16: "pad_top", 17: "pad_bottom", 18: "pad_left", 19: "pad_right", 20: "pad_row", 21: "pad_column",
         22: "base_dir", 32: "bg_color", 33: "bg_opa", 34: "bg_grad_color", 40: "bg_img_src", 45: "?45",
         48: "border_color", 50: "border_width", 53: "outline_width", 54: "outline_color", 55: "outline_opa",
         70: "img_opa", 71: "img_recolor", 72: "img_recolor_opa", 80: "arc_width", 81: "arc_rounded",
         82: "arc_color", 83: "arc_opa", 84: "arc_img_src", 85: "text_color", 86: "text_opa", 87: "text_font",
         89: "text_line_space", 91: "text_align", 92: "?92", 93: "?93", 96: "opa", 102: "anim_speed",
         107: "translate_x", 108: "translate_y"}


def u32(a):
    return struct.unpack_from("<I", IMG, a - B)[0]


def branches():
    """(site, target, is_bl) for every Thumb-2 BL / B.W (T4) encoding in the image."""
    for i in range(len(H) - 1):
        h1, h2 = H[i], H[i + 1]
        if h1 & 0xF800 != 0xF000 or h2 & 0xD000 not in (0xD000, 0x9000):
            continue
        s = (h1 >> 10) & 1
        off = (s << 24) | ((1 - (((h2 >> 13) & 1) ^ s)) << 23) | ((1 - (((h2 >> 11) & 1) ^ s)) << 22)
        off |= ((h1 & 0x3FF) << 12) | ((h2 & 0x7FF) << 1)
        site = B + 2 * i
        yield site, site + 4 + off - (s << 25), h2 & 0xD000 == 0xD000


def literal_loads():
    """(site, literal address) for `ldr rX, [pc, #imm]` (T1 and T2)."""
    for i, h in enumerate(H):
        site = B + 2 * i
        if h & 0xF800 == 0x4800:
            yield site, ((site + 4) & ~3) + (h & 0xFF) * 4
        elif h & 0xFF7F == 0xF85F and i + 1 < len(H):
            imm = H[i + 1] & 0xFFF
            yield site, ((site + 4) & ~3) + (imm if h & 0x80 else -imm)


def func_start(site):
    a = site
    while a > site - 0x400:
        h = H[(a - B) // 2]
        if h & 0xFF00 == 0xB500 or (h == 0xE92D and H[(a - B) // 2 + 1] & 0x4000):
            return a - 2 if H[(a - B) // 2 - 1] == 0x4601 else a  # create functions: `mov r1, r0` before the push
        a -= 2
    return None


def func_names(loads):
    by_lit = {}
    for site, lit in loads:
        by_lit.setdefault(lit, []).append(site)
    for m in re.finditer(rb"(?<=\0)(lv_[a-z0-9_]+)\0", IMG):
        name, addr = m.group(1).decode(), B + m.start()
        word = struct.pack("<I", addr)
        sites = [s for p in re.finditer(re.escape(word), IMG) if p.start() % 4 == 0 for s in by_lit.get(B + p.start(), [])]
        starts = sorted({func_start(s) for s in sites} - {None})
        yield name, starts


def classes():
    found, todo = {OBJ_CLASS: None}, True
    while todo:
        todo = False
        for o in range(0, len(IMG) - 0x20, 4):
            a = B + o
            if a in found or struct.unpack_from("<I", IMG, o)[0] not in found:
                continue
            ctor, dtor, _, ev = struct.unpack_from("<4I", IMG, o + 4)
            if all(p == 0 or (B <= p < B + len(IMG) and p & 1) for p in (ctor, dtor, ev)):
                found[a], todo = struct.unpack_from("<I", IMG, o)[0], True
    return found


def main():
    loads = list(literal_loads())
    br = list(branches())
    targets = {t for _, t, _ in br}
    print("| function | address (Thumb, +1 to call) | how |\n|---|---|---|")
    for name, starts in sorted(func_names(loads)):
        if starts:
            # not a BL/B.W target: a GCC .part/.cold clone, a variadic `push {r1-r3}` prologue or only reached by
            # pointer; take the entry from Ghidra or the callers instead
            how = "`__func__` literal" + ("" if set(starts) & targets else ", **not a call target**")
            print(f"| `{name}` | {', '.join(f'`{s:#010x}`' for s in starts)} | {how} |")
    print("\n| class | base | instance size | create | how |\n|---|---|---|---|---|")
    calls = {}
    for site, target, bl in br:
        if bl and target == CREATE_OBJ:
            calls[site] = func_start(site)
    lit_at = dict(loads)
    for cls, base in sorted(classes().items()):
        bits = u32(cls + 28)
        creates = sorted({f for s, f in calls.items() if f and any(
            u32(lit_at[x]) == cls for x in range(f, s, 2) if x in lit_at)})
        print(f"| `{cls:#010x}` | `{(base or 0):#010x}` | {bits >> 4 & 0xFFFF} | "
              f"{', '.join(f'`{c:#010x}`' for c in creates) or '—'} | class scan |")
    print("\n| `lv_obj_set_style_*` | address | how |\n|---|---|---|")
    for site, target, bl in br:
        if target != SET_LOCAL_STYLE_PROP or bl:
            continue
        i = (site - B) // 2
        prop = next((H[i - k] & 0xFF for k in range(1, 8) if H[i - k] & 0xFF00 == 0x2100), None)
        # plain setter: mov r3, r2; mov r2, r1; movs r1, #prop; b.w — colour setters pack the value after a push
        start = (site - 6 if H[i - 3: i - 1] == (0x4613, 0x460A) else  # flex: ldr r1, =id; ldrh r1, [r1]
                 site - 8 if H[i - 4: i - 2] == (0x4613, 0x460A) else func_start(site))
        print(f"| `{PROPS.get(prop, 'flex (registered prop id)')}` | `{start:#010x}` | b.w set_local_style_prop |")
    return 0


if __name__ == "__main__":
    sys.exit(main())
