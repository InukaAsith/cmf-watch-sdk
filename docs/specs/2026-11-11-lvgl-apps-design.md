# LVGL apps (ABI 1.2): design

Apps get the watch's own LVGL 8.3 widgets, fonts and scrolling instead of drawing everything with `rect`. This is
the "ABI 2" track of `docs/recon-lvgl.md`; it ships as a compatible extension, ABI **1.2**: old apps keep working,
an app may stay pixel-based (ABI 1, `cmf-wp2-ui`) or build an LVGL tree.

## Decisions

1. **The app keeps its own thread and event loop.** LVGL is only ever touched on `ui_service` (it has no lock,
   `docs/recon-lvgl.md` § UI thread). The app never calls LVGL: it fills a **command buffer** and calls
   `api.lv_submit(buf, len)`. The loader's 10 ms UI tick (already on `ui_service`) validates and executes the whole
   buffer, then `lv_submit` returns. Why not app callbacks on `ui_service`: an app bug would freeze or crash the
   UI thread, the 1000 ms force stop on BUTTON would stop working, and the ABI 1 loop model would split in two.
2. **Handles, not pointers.** The app names objects with `u16` handles it picks itself (1..=`MAX_OBJS`), so a
   batch needs no round trip for "create, then style it". Handle 0 is the app root. The loader maps handles to
   `lv_obj_t *`, so a bad app can never hand LVGL a wild pointer.
3. **Validate the whole batch, then execute.** A batch is checked against a copy of the handle table (structure,
   lengths, handle state, class of the target, whitelisted style props and flags) before the first LVGL call. A bad
   batch changes nothing and returns an error code.
4. **The loader owns the lifetime.** A root `lv_obj` (466×466, black, not scrollable) is created on the first
   submit, and the framebuffer image is hidden. On exit, crash, force stop or view delete the loader deletes the
   root (and with it every app object), closes the fonts it opened and clears the handle table, before the app
   block is freed. Leaks into the 98 KB GUI heap are impossible by construction.
5. **Budgets** keep the GUI heap safe (≥ 58 KB free on the worst stock screen, recon § GUI heap):
   `MAX_OBJS = 192`, `MAX_FONTS = 4`, label text ≤ 128 B, roller options ≤ 240 B, batch ≤ 2048 B.
6. **Events come back through the existing queue** as `Event { kind: LV = 8, x: handle, y: lv event code,
   delta: value }`. BUTTON, PAUSE, RESUME, MESSAGE, CROWN (crown over the root or a `KNOB` target) and raw
   TOUCH_DOWN/UP on the root's background keep their ABI 1 meaning.
7. **Fonts are the stock NAND fonts**, opened by index from a fixed table (`/NAND:A/font{18M,24L,24M,24R,28M,28R,
   30R,32M,32R,34R,36L,36M,36N,36R,44L,48M}.font`) into one of 4 slots; a style's `TEXT_FONT` value is the slot.
8. **Widgets in v1:** `obj`, `label`, `btn`, `arc`, `bar`, `slider`, `roller`, plus the vendor round scroll bar
   (`jx_scroll_bar`, crown scrolling) . Not in the image, so not offered: switch, checkbox, list, msgbox (compose
   them from `obj`/`arc`/`bar` + styles in the SDK later). Images, animations, `lv_style_t` sharing: later.

## Command format

Little-endian records, back to back: `op: u8, len: u8, handle: u16, payload[len]`.

| op | name | payload | LVGL |
|---|---|---|---|
| 0x01 | CREATE | `class u8, parent u16` (classes: 0 obj, 1 label, 2 btn, 3 arc, 4 bar, 5 slider, 6 roller) | `lv_<class>_create(parent)` |
| 0x02 | DEL | – | `lv_obj_del`; frees the handle and every descendant handle |
| 0x03 | SIZE | `w i32, h i32` (raw `lv_coord_t`: px, `LV_PCT(n)` = `0x2000_0000 \| n`, `CONTENT` = `0x2000_07D1`) | `lv_obj_set_size` |
| 0x04 | ALIGN | `align u8 (0..=21), x i16, y i16` | `lv_obj_align` |
| 0x05 | FLAGS | `set u32, clear u32`, bits ⊆ HIDDEN 0, CLICKABLE 1, CHECKABLE 3, SCROLLABLE 4, SCROLL_ELASTIC 5, SCROLL_MOMENTUM 6 | `lv_obj_add_flag` / `clear_flag` |
| 0x06 | STYLE | `prop u16, selector u32, value i32` (prop whitelist = the 50 stock setters' ids; `TEXT_FONT` value = font slot) | `lv_obj_set_local_style_prop` |
| 0x07 | FONT | `slot u8 (0..4), font u8 (0..16)`; handle 0 | `lvgl_bitmap_font_open` into a loader `lv_font_t` |
| 0x08 | TEXT | UTF-8 ≤ 128 B (label) | `lv_label_set_text` (copies) |
| 0x09 | VALUE | `v i32, anim u8` (arc, bar, slider; roller: selected) | `lv_arc_set_value`, `lv_bar_set_value`, `lv_roller_set_selected` |
| 0x0A | RANGE | `min i32, max i32` (arc, bar, slider) | `lv_arc_set_range` / `lv_bar_set_range` |
| 0x0B | OPTIONS | `mode u8`, then `\n`-separated UTF-8 ≤ 240 B (roller) | `lv_roller_set_options` (copies) |
| 0x0C | EVENTS | `mask u8`: PRESSED 0, CLICKED 1, LONG_PRESSED 2, RELEASED 3, VALUE_CHANGED 4 | `lv_obj_add_event_cb` with the loader trampoline |
| 0x0D | FLEX | `flow u8, main u8, cross u8, track u8` | `lv_obj_set_flex_flow` / `_align` |
| 0x0E | SCROLLBAR | `target u16`; handle = the new bar | `jx_scroll_bar` create on root, `set_scroll_obj(target)`, `set_knob(1)` |
| 0x0F | KNOB | – | crown goes to this object (`lv_disp_add_knob_obj`); a scrollable target scrolls, others send CROWN |
| 0x10 | ANGLES | `bg_start u16, bg_end u16, rotation u16` (arc) | `lv_arc_set_bg_angles`, `lv_arc_set_rotation` |

`lv_submit` returns 0, or: `-1` null/oversized buffer, `-22` malformed or invalid batch (nothing executed), `-12`
over a budget, `-2` a font file failed to open (the batch stops there; earlier records stay applied, the SDK
opens fonts in their own batch), `-95` the loader is older than 1.2 (returned by the SDK, which never reads the
`lv_submit` field when `abi_minor < 2`).

## Compatibility

- `Api` gains `lv_submit` after `screen_on`; `ABI_MINOR = 2`; `LOADER_ABI_MINOR = 2` in the stub and the loader
  (the stub refuses packages it does not know, so the firmware is rebuilt). As with 1.1, an app built with the 1.2
  SDK requires a 1.2 loader.
- The simulator fills the 1.2 table with an `lv_submit` that returns `-95`; LVGL apps in the simulator are a
  separate plan (LVGL 8.3 built for the host with stand-in fonts).
- `cmf-wp2-ui` stays the kit for pixel apps and the simulator.

## Risks

An LVGL assert or GUI-heap exhaustion resets the watch (budgets above, heap checked in the e2e); entries found by
shape (medium confidence) are verified in the emulator before use; addresses hold for JX402 `1.0.0.73` only;
submits cost up to one 10 ms tick each, so screens are built in one batch.
