# Stock app.bin reconnaissance for the app loader

Firmware: JX402 `1.0.0.73`, `app.bin` sha256 prefix `30e7978c39d8fb79` (2,465,920 B, base
`0x10000000`). All addresses are virtual. Tools: Ghidra 12.1.2 headless (`scripts/recon/decomp`,
`scripts/recon/ghidra/Dump.java`), capstone, and the emulator probe `scripts/recon/psram_probe.py`.
Extract the image with `uv run python scripts/recon/extract.py` (writes `runs/recon/`, not tracked).

Confidence: **high** = read from code and consistent with a second source; **medium** = read from
code only; **low** = inferred.

## Values the plan depends on

| Item | Value | Confidence | How |
|---|---|---|---|
| Core | Cortex-M33 (ARMv8-M MPU RBAR/RLAR format at `FUN_100f6708`), FPU present (S-registers in QEMU state) | high | decompile + emulator registers |
| MPU | enabled with `PRIVDEFENA` (`MPU_CTRL = 5`); regions only added on demand for guards; default map keeps `0x2000_0000–0x3FFF_FFFF` executable | medium | `FUN_100f6708` |
| PSRAM | 8 MiB at `0x38000000` (aliases `0x18000000`, `0x34000000` write-through, `0x3C000000` uncached) | high | emulator machine model + literal survey |
| Resource heap | `ResFbHeap` (JHeap): base `0x380A9340`, size `0x676B80` (literal at `0x10188508`), end `0x3871FEC0`; init `FUN_101884e4` → `FUN_10146aa4` | high | decompile |
| `APP_BASE` | `0x386FF000` | decision | top of `ResFbHeap` |
| `APP_SIZE` | `0x20000` (128 KiB) | decision | see below |
| Reservation | plan 2's stub patches the `ResFbHeap` size literal `0x676B80` → `0x655CC0`, so the heap ends at `0x386FF000` and `[APP_BASE, APP_BASE + APP_SIZE)` is never handed out | decision | — |
| Display | 466×466 round AMOLED; UI is LVGL (`lv_obj_set_width(…, 0x1D2)` in `FUN_100bb954`) on the Actions view manager (`ui_view`, `view_create`) | high | decompile + emulator frame size |
| Motor drive | PWM device `"PWM"` (`0x101DF1C0`), registers `0x40060000 + 0x100·ch` (`+0x00` ctrl, `+0x14` period, `+0x18` duty), clock-divider `0x40001068 + 4·ch`, 4 channels; motor uses channel 0 | high | device struct + driver decompile |
| PWM clock for the motor | driver init `FUN_10121504` writes `0x303` to every channel's clock register: source 32 MHz (`0x200`), divider 259+1 → **≈123 kHz**. `get_cycles_per_sec` (`FUN_10121468`) reports `0x1F40` = 8000 from the config, which does not match the hardware setting and is not used by the motor path | high | decompile + emulator `motor.log` |
| Motor call | `FUN_100fb4b4(_, ch=0, period=0xFF, pulse=strength, flags=1)` → PWM `api[0]` `pin_set_cycles` (`FUN_1012197c`); `pulse = 0` → `api[4]` channel off (`FUN_101214ac`) | high | disassembly |
| Stock PWM frequency | 123,077 / 255 ≈ **482 Hz**; duty = `strength / 255` | high | arithmetic + emulator `motor.log` |
| Motor type | **ERM** (single-ended unipolar PWM at 482 Hz, strength by duty, patterns by on/off timing, no resonance tuning) | medium | driver shape; no datasheet |
| Vibration levels | table `0x101F4A00` = `[255, 250, 0]`; level 0 is forced to 255 with pattern 2 (700 ms), level 1 = **250/255 = 98 %** with pattern 0 (300 ms), level 2 = 0 (off) | high | `FUN_100e17cc` (property `0x25`) |
| Why "minimum is too strong" | the weaker stock level is 98 % duty; the levels differ mainly by duration | high | follows from the table |
| Motor driver API | Zephyr device API at `0x101F5E7C`: `+0x0C` vibrate(dev, strength, pattern) = `FUN_100fb330`, `+0x10` set pattern table = `FUN_100fb0ec`, `+0x14` init = `FUN_100fb1fc`; driver handle at `*0x3801B164`; manager init `FUN_100e0cd8` | high | decompile |
| Pattern table | `0x101F4A0C`, 10 entries × 12 B: `u16 on_ms, u16 off_ms, u32 repeat, u32 total_ms`; count `u16` at `+0x78` (10). Entries: 300; 500; 700; 300/1700 for 60 s (×2, calls); 1000; 60; 150/850 for 2 s; 20; empty | high | dump |
| Properties | `ux2sys` table `0x101F4274`, 12 B each `{set, get, u16 size}`: `0x24` stored level (`+0x2C` of `0x380077F8`), `0x25` level preview → vibrate, `0x2A` (`SETMOTOR`) → vibrate 255 with pattern 5 (1000 ms); set `FUN_100e08b8`/`FUN_100e07b8`, get `FUN_100e082c` | high | decompile |

### Why `APP_BASE` / `APP_SIZE`

`psram_probe.py` filled PSRAM with a marker, reset the guest, walked the UI for 4 minutes (app list,
apps, swipes in all directions) and dumped PSRAM. The largest untouched block was
`0x38275000–0x383CF000` (1384 KiB), but it lies inside `ResFbHeap`, which serves resources and
frame buffers on demand, so "untouched during a walk" is not "free". No literal in the image points
into that range except ASCII coincidences. Taking the top 128 KiB of `ResFbHeap` and shrinking the
heap by the same amount is a guaranteed reservation at a cost of 2 % of the resource heap. Apps
link at `0x386FF000`, code and data in one image, executed directly from the `0x38…` mapping
(executable under the default map). The emulator does not model the MPU or caches; plan 2 verifies
execution there and repeats it on hardware only with approval.

### Consequences for plan 1

- Task 4 frequency knob: the motor is ERM and stock PWM already runs at 482 Hz, so the knob is
  narrowed to `[250, 482, 1000, 4000, 16000]` Hz with 482 as default (spec: narrow for ERM).
- `motor(duty, freq)` in the loader maps to `pin_set_cycles` on channel 0 with
  `period = 123077 / freq` while the clock register holds the stock `0x303`. Other frequencies
  with fine duty resolution need the divider changed (`api[6]` = `FUN_10121681`); the loader must
  write `0x303` back before handing the motor to stock.

## Emulator check of the motor path

`scripts/recon/motor_check.py` boots stock in a QEMU built with the motor log (cmf-watch-utils
branch `emu-motor-log`) and calls the property handlers through gdb with a manual call frame.
`motor.log` (virtual ms, duty in ‰, Hz):

| Trigger | Log | Matches |
|---|---|---|
| boot | `608 1000 482` → `669 0 0` | pattern 6 (60 ms) at 255 |
| `0x25` level 1 preview | `21951 980 482` → `22252 0 0` | 250/255, pattern 0 (300 ms) |
| `0x25` level 0 preview | `24275 1000 482` → `24976 0 0` | 255, pattern 2 (700 ms) |
| `0x2A` `SETMOTOR 1` | `1000 482` in 1000 ms blocks until switched off | 255, pattern 5 |

## Items for plan 2 (located, not yet worked out)

| Item | Where to start | Confidence |
|---|---|---|
| App list | `applist_view` / `applist_list_view` / `applist_grid_view` / `applist_waterfall_view` strings at `0x10203A4B…`; launcher `launcher_app.c` (`0x1020364D`), `_launcher_key_event_handle` | medium |
| App launch | `system_app_launch` (`0x10054CF4`, from cmf-watch-utils docs), `system_app_launch_init` | medium |
| Overlay for notifications/calls | msgbox popups: `msgbox_cache popup …` (`0x101DF7BC`), `set_popup_msgbox` (`0x101DFAB4`), `notify_view` (`0x101DFC98…`), `btcall_view` (`0x1021B155`) | medium |
| Input | crown: `_knob_event_handler` (`0x1021B3E0`), keys: `*_key_event_handler`, touch: LVGL pointer indev (`INPUT_DEV_TYPE_POINTER`, `0x101FE090`) | low |
| Drawing | LVGL objects/canvas on the Actions view manager; colour depth and canvas API not yet read | low |
| Cyrillic | no Cyrillic UI strings in `app.bin`; fonts live in `/NAND:A/*.font` and are not inspected. The lab uses ASCII labels | medium |
| Storage | Zephyr FS mounts `/NAND:A` (UI), `/NAND:B`, `/NAND:C` (user dials, fonts), `/NAND:E` (animations), `/NAND:K` (boot) at `0x1020185B…`; apps go to a new directory on `/NAND:C` | medium |
| BLE command dispatch | handler table and AT group dispatcher `0x101F4D70` are documented in cmf-watch-utils `protocol/ble.md`; a free opcode is chosen in plan 2 | low |
| Stub location | NOR app partition `0x24000`, size `0x34B000`; `app.bin` uses `0x25A080`, leaving ≈ 964 KiB to append the stub. ACTH payload and header `sum32` must be recomputed (the emulator checks them, `emu/image.py`) | high |
| Zero runs inside `app.bin` | `0x101E5BB2` (10,830 B) etc. are initialised-data images, not free space | medium |
