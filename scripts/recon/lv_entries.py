"""Runs every ABI 1.2 LVGL entry of fw/wp2-core/src/stock.rs on stock 1.0.0.73 in the emulator (docs/recon-lvgl.md
"Verified for ABI 1.2"). Calls land on ui_service, the LVGL thread. Each widget is created under a root on the active
screen, configured and read back (class pointer at +0, coords at +0x14..+0x20, values through the getters); the font
is opened and closed; everything is deleted. Prints `name address ok|FAIL` per entry, plus the runtime-registered
flex style ids. Run:
CMF_WATCH_UTILS=… WP2_QEMU=…/qemu-system-arm WP2_NETSIM_INSTANCE=91 nix-shell -p gdb --run \
    'uv run --locked python scripts/recon/lv_entries.py'
"""
import re
import struct
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT))
from tests.e2e.harness import UI_MEM_ALIGNED_ALLOC, UI_MEM_FREE, Emu, stock_nor  # noqa: E402

S = {m[1]: int(m[2].replace("_", ""), 16) for m in
     re.finditer(r"pub const (\w+): u32 = (0x[0-9A-Fa-f_]+);", (ROOT / "fw/wp2-core/src/stock.rs").read_text())}
LV_OBJ_GET_STYLE_PROP = 0x101C6E07  # (obj, part, prop) -> lv_style_value_t (recon: medium)
LV_OBJ_HAS_FLAG = 0x101283DD
# u16 ids registered by lv_flex_init at boot; the setters load them (`ldr r1, =var; ldrh r1, [r1]`), literals at
# 0x10133540 (flow), 0x10133570, 0x10133580, 0x10133590; LV_LAYOUT_FLEX from lv_obj_set_flex_flow's 0x10133560
FLEX_VARS = {"flow": 0x38047FEE, "main_place": 0x38047FF2, "cross_place": 0x38047FEC, "track_place": 0x38047FF4,
             "layout_flex": 0x38047FEA}
CLASS = {"LV_OBJ_CREATE": 0x101FB27C, "LV_BTN_CREATE": 0x101FB730, "LV_ARC_CREATE": 0x101FB6F0,
         "LV_BAR_CREATE": 0x101FB710, "LV_SLIDER_CREATE": 0x101FB810, "LV_ROLLER_CREATE": 0x101FB7D0,
         "JX_SCROLL_BAR_CREATE": 0x101F36BC}
KNOB = 1 << 30
FONT = b"/NAND:A/font32M.font\0"


def main():
    fails = 0
    with Emu("lv-entries", nor=stock_nor(), seconds=1500) as emu:
        emu.wait_uart("launcher enter", 60)
        # the panel blanks once after boot despite keep_awake, and an idle guest stalls emu.call (harness.ready)
        # ("launcher early-suspend" ~5 s after boot, deep sleep ~6 s later); a crown press wakes it
        emu.wait_uart("launcher early-suspend", 60)
        emu.send("c 1", "c 0", pause=1)
        emu.wait_uart("launcher late-resume", 30)
        emu.keep_awake()
        emu.sleep(8)
        uart = emu.uart()
        if uart.rfind("launcher early-suspend") > uart.rfind("launcher late-resume"):
            raise RuntimeError("the watch fell asleep again")

        def call(fn, *args):
            return emu.call(S.get(fn, fn), *(a & 0xFFFFFFFF for a in args), thread=b"ui_s")

        def check(name, ok, note=""):
            nonlocal fails
            fails += not ok
            print(f"{name} {S[name]:#010x} {'ok' if ok else 'FAIL'}{'  ' + note if note else ''}", flush=True)

        def words(addr, fmt):
            return struct.unpack(fmt, emu.peek(addr, struct.calcsize(fmt)))

        def create(name, parent, w, h, hidden=False):
            obj = call(name, parent)
            if hidden:
                call("LV_OBJ_ADD_FLAG", obj, 1)
            call("LV_OBJ_SET_SIZE", obj, w, h)
            return obj

        def made(name, obj, w, h):
            """Class pointer, and coords after a layout pass."""
            x1, y1, x2, y2 = words(obj + 0x14, "<4i")
            return obj != 0 and words(obj, "<I")[0] == CLASS[name] and (x2 - x1 + 1, y2 - y1 + 1) == (w, h)

        flex = {k: words(v, "<H")[0] for k, v in FLEX_VARS.items()}
        print("flex ids " + " ".join(f"{k}={v}" for k, v in flex.items()), flush=True)
        scratch = emu.call(UI_MEM_ALIGNED_ALLOC, 2, 8, 0x100, 0)  # RES heap: event, font, strings
        event, font, path, options = scratch, scratch + 0x20, scratch + 0x40, scratch + 0x80
        emu.poke(scratch, bytes(0x40) + FONT.ljust(0x40, b"\0") + b"one\ntwo\nthree\0")

        # hidden: a visible object over the face makes LVGL redraw its rotated images through sw_rotate, ending in
        # a compatibility-ROM API QEMU lacks ("unsupported API 0x524f4d"); layout still runs for hidden objects
        root = create("LV_OBJ_CREATE", call("LV_DISP_GET_SCR_ACT", 0), 466, 466, hidden=True)
        btn = create("LV_BTN_CREATE", root, 120, 60)
        arc = create("LV_ARC_CREATE", root, 200, 200)
        call("LV_ARC_SET_ROTATION", arc, 135)
        call("LV_ARC_SET_BG_ANGLES", arc, 10, 270)
        call("LV_ARC_SET_VALUE", arc, 40)
        arc_value = call("LV_ARC_GET_VALUE", arc)
        arc_fields = words(arc + 0x2C, "<H4xHH3h")  # rotation, bg_start, bg_end, value, min, max
        call("LV_ARC_SET_VALUE", arc, 150)
        arc_clamped = call("LV_ARC_GET_VALUE", arc)
        bar = create("LV_BAR_CREATE", root, 200, 20)
        call("LV_BAR_SET_RANGE", bar, -50, 150)
        call("LV_BAR_SET_VALUE", bar, 120, 0)
        bar_value = call("LV_BAR_GET_VALUE", bar)
        bar_fields = words(bar + 0x2C, "<3i")  # cur_value, min, max
        call("LV_BAR_SET_VALUE", bar, 500, 0)
        bar_clamped = call("LV_BAR_GET_VALUE", bar)
        slider = create("LV_SLIDER_CREATE", root, 200, 20)
        call("LV_BAR_SET_VALUE", slider, 30, 0)
        slider_value = call("LV_BAR_GET_VALUE", slider)
        roller = create("LV_ROLLER_CREATE", root, 150, 120)
        call("LV_ROLLER_SET_OPTIONS", roller, options, 0)
        call("LV_ROLLER_SET_SELECTED", roller, 1, 0)
        roller_selected = call("LV_ROLLER_GET_SELECTED", roller)
        call("LV_ROLLER_SET_SELECTED", roller, 5, 0)  # clamps to the last option: 2 with ours, 4 with the 5 defaults
        roller_last = call("LV_ROLLER_GET_SELECTED", roller)
        cont = create("LV_OBJ_CREATE", root, 200, 200)
        call("LV_OBJ_SET_FLEX_FLOW", cont, 1)  # COLUMN
        call("LV_OBJ_SET_FLEX_ALIGN", cont, 2, 1, 3)  # CENTER, END, SPACE_EVENLY
        a, b = create("LV_OBJ_CREATE", cont, 50, 30), create("LV_OBJ_CREATE", cont, 50, 30)
        style = {k: call(LV_OBJ_GET_STYLE_PROP, cont, 0, flex[k]) for k in ("flow", "main_place", "cross_place",
                                                                             "track_place")}
        layout = call(LV_OBJ_GET_STYLE_PROP, cont, 0, 10)  # LV_STYLE_LAYOUT
        scroll = create("LV_OBJ_CREATE", root, 200, 100)
        sbar = create("JX_SCROLL_BAR_CREATE", call("LV_DISP_GET_SCR_ACT", 0), 462, 462, hidden=True)
        call("JX_SCROLL_BAR_SET_SCROLL_OBJ", sbar, scroll)
        call("JX_SCROLL_BAR_SET_KNOB", sbar, 1)
        knob_on = call(LV_OBJ_HAS_FLAG, scroll, KNOB)
        call("JX_SCROLL_BAR_SET_KNOB", sbar, 0)
        knob_off = call(LV_OBJ_HAS_FLAG, scroll, KNOB)
        emu.poke(event, struct.pack("<II", btn, root))
        target = call("LV_EVENT_GET_TARGET", event)
        font_open = call("LVGL_BITMAP_FONT_OPEN", font, path)
        line_height, font_ctx = words(font + 8, "<i")[0], words(font + 0x1C, "<I")[0]
        call("LVGL_BITMAP_FONT_CLOSE", font)
        font_ctx_closed = words(font + 0x1C, "<I")[0]  # close frees the context in user_data and zeroes it

        emu.sleep(1)  # one layout and render pass over everything above
        a_y2, b_y1 = words(a + 0x20, "<i")[0], words(b + 0x18, "<i")[0]
        check("LV_BTN_CREATE", made("LV_BTN_CREATE", btn, 120, 60))
        check("LV_ARC_CREATE", made("LV_ARC_CREATE", arc, 200, 200))
        check("LV_ARC_SET_ROTATION", arc_fields[0] == 135, f"rotation {arc_fields[0]}")
        check("LV_ARC_SET_BG_ANGLES", arc_fields[1:3] == (10, 270), f"bg {arc_fields[1:3]}")
        check("LV_ARC_SET_VALUE", arc_fields[3] == 40 and arc_clamped == 100,
              f"value {arc_fields[3]}, 150 -> {arc_clamped} (range {arc_fields[4]}..{arc_fields[5]})")
        check("LV_ARC_GET_VALUE", arc_value == 40)
        check("LV_BAR_CREATE", made("LV_BAR_CREATE", bar, 200, 20))
        check("LV_BAR_SET_RANGE", bar_fields[1:] == (-50, 150), f"range {bar_fields[1:]}")
        check("LV_BAR_SET_VALUE", bar_fields[0] == 120 and bar_clamped == 150, f"500 -> {bar_clamped}")
        check("LV_BAR_GET_VALUE", bar_value == 120)
        check("LV_SLIDER_CREATE", made("LV_SLIDER_CREATE", slider, 200, 20) and slider_value == 30)
        check("LV_ROLLER_CREATE", made("LV_ROLLER_CREATE", roller, 150, 120))
        check("LV_ROLLER_SET_OPTIONS", roller_last == 2, f"3 options: select 5 -> {roller_last}")
        check("LV_ROLLER_SET_SELECTED", roller_selected == 1)
        check("LV_ROLLER_GET_SELECTED", roller_selected == 1)
        check("LV_OBJ_SET_FLEX_FLOW", style["flow"] == 1 and layout == flex["layout_flex"] and b_y1 > a_y2,
              f"layout {layout}, children y {a_y2} < {b_y1}")
        check("LV_OBJ_SET_FLEX_ALIGN", (style["main_place"], style["cross_place"], style["track_place"]) == (2, 1, 3))
        check("LV_EVENT_GET_TARGET", target == btn)
        check("JX_SCROLL_BAR_CREATE", made("JX_SCROLL_BAR_CREATE", sbar, 462, 462))
        check("JX_SCROLL_BAR_SET_SCROLL_OBJ", knob_on == 1, "set_knob reached the scroll obj")
        check("JX_SCROLL_BAR_SET_KNOB", knob_on == 1 and knob_off == 0)
        check("LVGL_BITMAP_FONT_OPEN", font_open == 0 and line_height > 0 and font_ctx != 0,
              f"line_height {line_height}")
        check("LVGL_BITMAP_FONT_CLOSE", font_ctx_closed == 0, f"user_data {font_ctx:#x} -> {font_ctx_closed:#x}")

        call("LV_OBJ_DEL", root)  # the scroll obj goes before its bar, as when a stock screen is deleted
        call("LV_OBJ_DEL", sbar)
        emu.call(UI_MEM_FREE, 2, scratch)
        emu.sleep(2)
        boots = emu.uart().count("*** Booting Zephyr")
        print(f"after delete: boot banners {boots}", flush=True)
        fails += boots != 1
    print("all ok" if not fails else f"{fails} FAIL")
    return 1 if fails else 0


if __name__ == "__main__":
    sys.exit(main())
