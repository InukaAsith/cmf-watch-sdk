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
| MPU | enabled with `PRIVDEFENA` (`MPU_CTRL = 5`) at boot with three fixed regions, none on `ResFbHeap`; default map keeps `0x2000_0000–0x3FFF_FFFF` executable (see "Hardware: caches and MPU") | high | `FUN_100f6708`, `FUN_100f67cc` |
| PSRAM | 8 MiB at `0x38000000` (aliases `0x18000000`, `0x34000000` write-through, `0x3C000000` uncached) | high | emulator machine model + literal survey |
| Resource heap | `ResFbHeap` (JHeap): base `0x380A9340`, size `0x676B80`, end `0x3871FEC0`; init `FUN_101884e4` → `FUN_10146aa4`. The size literal occurs **three times**: `0x10188508` (init), `0x10188588` (getter `FUN_10188584`, `ldr r0,=0x676B80; bx lr`, no direct callers — likely reached through an ops table) and `0x101885F4` (second getter at `0x101885F0`) | high | decompile + literal search |
| `APP_BASE` | `0x386FF000` | decision | top of `ResFbHeap` |
| `APP_SIZE` | `0x20000` (128 KiB) | decision | see below |
| Reservation | plan 2's stub patches **all three** `ResFbHeap` size literals `0x676B80` → `0x655CC0`, so the heap ends at `0x386FF000` and `[APP_BASE, APP_BASE + APP_SIZE)` is never handed out. Not yet applied or verified: plan 2 patches, fills the region with a marker and walks the UI with a heavy face first | decision, unverified; **superseded by plan 2 revision: 4 KiB stub RAM only** | — |
| Display | 466×466 round AMOLED; UI is LVGL (`lv_obj_set_width(…, 0x1D2)` in `FUN_100bb954`) on the Actions view manager (`ui_view`, `view_create`) | high | decompile + emulator frame size |
| Motor drive | PWM device `"PWM"` (`0x101DF1C0`), registers `0x40060000 + 0x100·ch` (`+0x00` ctrl, `+0x14` period, `+0x18` duty), clock-divider `0x40001068 + 4·ch`, 4 channels; motor uses channel 0 | high | device struct + driver decompile |
| PWM clock for the motor | driver init `FUN_10121504` writes `0x303` to every channel's clock register. Actions SDK `pwm_acts.c` (pinned commit `26f51e58`): bits 9–10 select the source (0 = 32 kHz RC, 1 = HOSC 32 MHz, 2 = 64 MHz), divider field `0..255` = n+1, `256..260` = /512, /1024, /2048, /4096, /8192. `0x303` = HOSC /4096 = **7812.5 Hz**, which is the SDK's `PWM_PIN_CYCLES_PER_SEC` 8000 rounded to a power-of-two divider; `get_cycles_per_sec` (`FUN_10121468`) reports that 8000 | high | decompile + SDK source |
| Motor call | `FUN_100fb4b4(_, ch=0, period=0xFF, pulse=strength, flags=1)` → PWM `api[0]` `pin_set_cycles` (`FUN_1012197c`); `pulse = 0` → `api[4]` channel off (`FUN_101214ac`) | high | disassembly |
| Stock PWM frequency | 7812.5 / 255 ≈ **30.6 Hz**; duty = `strength / 255`. Not measured on hardware | high (code), unmeasured | arithmetic + emulator `motor.log` (decoder per SDK) |
| Motor type | **ERM** (single-ended unipolar PWM at ~31 Hz, strength by duty, patterns by on/off timing, no resonance tuning) | medium | driver shape; no datasheet |
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

- Task 4 frequency knob: the motor is ERM, but at the stock ~31 Hz a low duty is likely felt as
  chopping rather than a softer buzz, so the lab keeps the knob: `[31, 62, 125, 250, 500, 1000,
  4000, 16000]` Hz, default 31 (stock).
- `motor(duty, freq)` in the loader maps to `pin_set_cycles` on channel 0 with
  `period = 7812 / freq` while the clock register holds the stock `0x303` (≥ 2 cycles, so up to
  ~3.9 kHz with coarse duty). Other frequencies need the divider changed (frequency API
  `api[6]` = `FUN_10121680`); the loader must write `0x303` back before handing the motor to stock.

## Emulator check of the motor path

`scripts/recon/motor_check.py` boots stock in a QEMU built with the motor log (cmf-watch-utils
branch `emu-motor-log`, divider decoded per the SDK table above) and calls the property handlers
through gdb with a manual call frame; it fails unless `(980, 30)` and `(1000, 30)` appear.
The log confirms the register values and timings the firmware writes; the frequency column is only
as good as the decoder, which now follows the SDK. `motor.log` (virtual ms, duty in ‰, Hz):

| Trigger | Log | Matches |
|---|---|---|
| boot | `… 1000 30` → `… 0 0` | pattern 6 (60 ms) at 255 |
| `0x25` level 1 preview | `… 980 30` for 300 ms | 250/255, pattern 0 (300 ms) |
| `0x25` level 0 preview | `… 1000 30` for 700 ms | 255, pattern 2 (700 ms) |
| `0x2A` `SETMOTOR 1` | `1000 30` in 1000 ms blocks until switched off | 255, pattern 5 (`total_ms` = 86,400,000, i.e. 24 h) |

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

## Plan 2 findings

### Находки разведки

| Что | Адрес / значение | Увер. |
|---|---|---|
| ACTH | `+0x18` размер полезной нагрузки (сток `0x259C3C`), `+0x1C` = `0xFFFFFFFF − sum32(app[0x200:0x200+size])`, `+0x20` выравнивает `sum32(app[0:0x200]) = 0xFFFFFFFF`; пересчёт на стоке воспроизводит `0xDCF84B50`/`0x7929C5D6` | high |
| XIP | эмулятор отображает NOR `0x24000..+0x3DC000` на `0x10000000`, так что дописанные байты видны сразу | high |
| Инициализация кучи | `FUN_101884e4`: `bl 0x10146aa4` по `0x101884FA` (8 аргументов, 4 из них на стеке); литералы размера `0x10188508` (init), `0x10188588`, `0x101885F4` (геттеры) | high |
| BLE-диспетчер | `FUN_100d343c(frame)`, поля уже в host order; `c1 == 0xFFFF && c2 > 0x908B` → `default` по `0x100D34EA`: `ldrh r1,[r5,#1]` → `bl 0x100cc284` по **`0x100D34F0`** (`f8f7c8fe`), то есть чтение payload в `0x3804B0E6`, потом лог; `r5` — указатель на кадр | high |
| BLE-хелперы | `FUN_100cc284(buf, len)` — прочитать payload (0 = ok); `FUN_101b608c(&u16 out_len, buf, len, cmd)` → указатель на открытый текст или 0 (AES-CBC ключом сессии, CRC32); `FUN_101b605a(&u16 out_len, data, len, cmd)` → указатель на шифротекст; `FUN_101ddb66(dst, src, n, max)` = memcpy_s; `FUN_100cc2d4(frame, total)` — отправить; стоковый кадр ответа `0x3804B0DB` | medium |
| Ключ по команде | `FUN_100ccab4`: особый ключ только у `0x804B` и `(cmd & 0x7FFF) == 0x49`, остальные команды шифруются ключом сессии | high |
| Файлы (Zephyr fs, `struct fs_file_t` 0x2C Б) | open `0x10107030(file, path, flags)` (`READ 1`, `WRITE 2`, `CREATE 0x10`); read `0x10107238`; write `0x10107294`; seek `0x101072F0`; tell `0x10107350`; sync `0x101073A4`; close `0x101071E0`; unlink `0x1010764C(path)`; rename `0x10107704(from, to)`; поиск точки монтирования `0x10106FA0(&mp, path, 0)`; vtable `*(mp+0x1C)`: `+0x34` unlink, `+0x38` rename, `+0x3C` mkdir (обёртки `fs_mkdir` в образе нет) | high / mkdir medium |
| Таблица view | 162 записи по 0x18 Б, `0x100004B8..0x100013E8`: `{name, proc, presenter, 0, u16 id, u16 order, u16 w, u16 h}`; поиск `FUN_1018bc04(id)`; литералы начала: `0x101461AC`, `0x1018BC24`, `0x1018DF3C`; конца: `0x10055458`, `0x1018BC28`, `0x1018DF40`; открыть view: `FUN_101898bc(id, arg)`; id `0xF0` свободен | high |
| Сообщения view (стиль `proc`, как `goodbye_view` `0x10060A54`) | `proc(view_id, msg, data)`: 2 preload, 3 layout (`*(void**)data` — дисплей), 4 delete, 7 focus, 8 defocus, 0xB update, 0xD key (`data[0]` — код клавиши, байт `data+4 = 1` — «обработано») | medium |
| Коды клавиш (UART `view_key:`) | нажатие коронки `0x20000001` → `0x04000001`; поворот в списке `0xFFFF006F` / `0xFFFF006E` | medium |
| LVGL 8.3 | `lv_disp_get_scr_act 0x101255E0`, `lv_img_create 0x1013ED38`, `lv_img_set_src 0x1013ED50`, `lv_obj_set_pos 0x1012926C`, `lv_obj_set_size 0x101294C8`, `lv_obj_invalidate 0x10129FD8`, `lv_obj_del 0x1012C774`, `lv_obj_add_event_cb 0x10125A94`, `lv_timer_create 0x1013B10C`, `lv_timer_del 0x1013B1A4` (medium), `lv_event_get_indev 0x10125BD4` (low); события: `PRESSED 1`, `SHORT_CLICKED 4`, `RELEASED 8`, `SCROLL 0xB` | high, если не указано иное |
| Память UI | `ui_mem_aligned_alloc(type, align, size, caller) 0x101882C4` (типы 0..2); малые выделения `0x10146A3C(size, tag)` / free `0x101CE04A` | medium |
| ОС | `os_thread_create(buf, size, entry, p1, p2, p3, prio, opts, delay) 0x101BE1A2` (в начале `buf` лежит `k_thread` на 0x118 Б); `os_sleep(ms) 0x101BE1EC`; `k_thread_abort` — `0x10105C90` или `0x1019B2D8` | high / abort low |
| Всплывающие окна | `msgbox_cache_popup(id, udata) 0x100D5E34`; `set_popup_msgbox 0x100D6EB0` / `set_close_msgbox 0x100D6FB4` ведут флаги `*(u8*)0x3800FAF4` (бит 0 и бит 1 = окно открыто), id в `0x3800FAF6`/`0x3800FAF8` | high |
| PWM | `struct device` "PWM" `0x101DF1C0`, api `0x101FB190`: `[0] pin_set_cycles(dev, ch, period, pulse, flags) 0x1012197D`, `[4] off(dev, ch) 0x101214AD`, `[6] set(dev, ch, hz, duty%, pol) 0x10121681`; период — 16 бит | high |
| Список приложений | иконки из ресурсов сцены, построитель `FUN_1005e9d8` (число по `data+8`, записи по 3 слова с `data+0xC`, клик `0x1005EBA1`, событие 4); процедуры list `0x1005EEC9`, grid `0x1005EBC5`, waterfall `0x1005F101` | low, разбирает задача 9 |
| Эмулятор | ввод `input.txt`: `t down x y`, `c down` (коронка/питание), `r delta`, `p mV charger percent`; без стокового профиля `STOCK_AWAKE` не включается, поэтому харнес пишет `0xFFFFFFFF` в `**0x38037D14` (таймаут автосна, `system_set_auto_sleep_time`); NAND живёт в памяти QEMU и переживает сброс гостя | high |

`ResFbHeap`: литерал размера `0x676B80` встречается трижды (`0x10188508`, `0x10188588`, `0x101885F4`); патчер меняет **все три** на `0x675B80`: куча кончается на `0x3871EEC0`, выше лежит только RAM заглушки (`0x1000`). Загрузчик и приложения получают блоки кучи на время работы.

Stock 1.0.0.73 crashes (`MllcFailedHook`, 651,548-byte block, then a reboot) when the face carousel is scrolled with face `hvk9m1hmpyuheyd`; reproduced on unpatched stock, so the e2e walk does not scroll the carousel.

## Plan 2 revision: memory while running

Read-only recon for the owner's 2026-11-10 memory decision (loader and app allocated from the stock heap
only while running). Sources: `scripts/recon/decomp` on `0x10188200–0x10188700`, `0x10147124`,
`MllcFailedHook`; table dump of `0x101FD7F0`; a scratch build of `sdk/` with `ORIGIN = 0`.

| Item | Value | Confidence |
|---|---|---|
| `ui_mem` type table | `*0x1018837C = 0x101FD7F0`, 3 entries × 0x24 B: `{name, init, alloc, aligned_alloc, realloc, free, +0x18 dump, +0x1C, +0x20 total}`. Type 0 `FB`, 1 `GUI` (LVGL heap, 0x18000 B), 2 `RES`. `uimem dump [0\|1\|2], where 0-FB, 1-LVGL, 2-RES` | high |
| `ui_mem` API | `ui_mem_alloc(type, size, caller)` `0x10188234`; `ui_mem_aligned_alloc(type, align, size, caller)` `0x101882C4` (align must be a power of two, asserts otherwise); `ui_mem_realloc` `0x101883C4`; `ui_mem_free(type, ptr)` `0x10188380`. `caller` is a C string used only in the failure log | high |
| `FB` and `RES` share `ResFbHeap` | RES: init `0x101884E4` (the `ResFbHeap` init), alloc `0x10188518`, aligned_alloc `0x1018854C`, free `0x10188578`, dump `0x1018858C`, total `0x10188584`. FB: aligned_alloc `0x101D78DC` (align ≤ 0x40, else returns 0) → `0x101885A8` (align 0x40, retries 15× with `os_sleep(2)`), free `0x101D78EA` → `0x10188578`, total `0x101885F0`. Both end in JHeap alloc `0x10147124` on the heap struct `0x38043824` | high |
| JHeap struct `ResFbHeap` (`0x38043824`) | `+0` name, `+4` total, `+8` free bytes (alloc subtracts, free adds), `+0xC` minimum free ever, `+0x14` flags/alignment bits, `+0x60` free-list head, `+0x68` end marker. Free block: `{next: u32, size: u32 & 0xFFFFFF}` (size includes the header) | medium |
| JHeap allocation policy | first fit from `+0x60`; a request of exactly `0x6A088` B (466·466·2, a full RGB565 frame) takes the last fitting block instead (best-from-end) | medium |
| **JHeap never returns NULL** | when `free < need` or no free block fits, `0x10147124` calls `MllcFailedHook` `0x101470C0`, which prints the heap dump, calls `0x100F6D84(0)` and spins forever → the watch resets. This is the `MllcFailedHook at line: 353` reboot seen with the heavy face. So loader, app and framebuffer allocations must be checked against the free list **before** calling `ui_mem_*` | high |
| Allocation size overhead | `need = size + hdr(type) + (flags & 0x3F) [+ 4 debug guard] [+ align − heap_align]`, rounded up to the heap alignment (4). Plan 2 uses the conservative bound `size + align + 0x100`; plan 2 task 1 measures the real overhead in the emulator | medium |
| Lock | JHeap alloc/free raise `BASEPRI` to `0x20` around list updates (no mutex). Walking the free list and allocating under the same `BASEPRI` makes check-then-alloc atomic against other threads | medium |
| Heap statistics | dump `FUN_10146BA4(heap, 1, 0)` (RES `+0x18` = `0x1018858C`) prints `Total:%u, Free:%u (%u%%), Water:%u (%u%%), UsedMax:%u (%u%%), MaxBlock:%u` | high |
| Rust codegen for relocatable images | with `ORIGIN = 0` and `-C link-arg=--emit-relocs`, rustc 1.97 emits `R_ARM_THM_MOVW_ABS_NC`/`R_ARM_THM_MOVT_ABS` pairs for addresses (hello: 4 + 4, vibration-lab: 20 + 20, plus 6 `R_ARM_ABS32`). With `-C target-feature=+no-movt` addresses go to literal pools: only `R_ARM_ABS32` remain (hello 3, vibration-lab 28), plus PC-relative `R_ARM_THM_CALL`/`R_ARM_THM_JUMP24` that need no fix-up. `e_entry = 0x1`. rustc warns that `no-movt` is an unknown/unstable feature, but honours it | high |
| Cache maintenance for code in PSRAM | `0x101BEE52 (addr, size)`: SPI1 (PSRAM) cache write-buffer flush + write back and invalidate of the range; `wp2_core::load::load` calls it. See "Hardware: caches and MPU" | high |

Heap probes (emulator, patched image): `ui_mem_aligned_alloc(2, 8, 0x10000, 0)` costs 0x10018 bytes of `free` (`u32(0x38043824 + 8)`), i.e. a JHeap overhead of `0x18`, within the plan's `SLACK`. `FUN_1018858D` is not a dump helper but the stock failure path (prints `Dump:ResFbHeap … MaxBlock:0`, then `save_all_data` and panics), so the tests never call it and check the free list (`u32(0x38043824 + 0x60)` chain) instead.

Heavy-face selection for the e2e walk (candidates in `emu faces --sort downloads` order, first whose stock control walk of 240 s has one boot, no `MllcFailedHook`, no fatal):

| Face | Control on unpatched stock | Verdict |
|---|---|---|
| `hvk9m1hmpyuheyd` (NMS 176, 1267 downloads) | 2 boots, 1 `MllcFailedHook` (651,548 B) | rejected |
| `kcumc43wn4cb8e9` (Wavy Seconds, 534) | `emu run --fmc-face` fails: QEMU stops on an unimplemented write at `0x40070504` during boot | rejected |
| `w23jrco8sqew3nv` (Concentric Hal, 476) | 1 boot, no `MllcFailedHook`, no fatal | chosen |

## Plan 2, task 4: view table bounds

Disassembly of every PC-relative load of the six literals listed under "Таблица view" (`scripts/recon` + `llvm-objdump`):

| Literal | Reader | Meaning |
|---|---|---|
| `0x1018BC24` / `0x1018BC28` | `FUN_1018bc04(id)`: `for (e = start; e < end; e += 0x18) if (e->id == id) return e;` | view table start / end |
| `0x1018DF3C` / `0x1018DF40` | `FUN_1018dc9c` (view create), the same inline loop with step `0x18` | view table start / end |
| `0x101461AC` | `FUN_10146060`: loop `0x100003A0 .. 0x100004B8`, step `0x1C`, entries go to `os_thread_create` | **end of the preceding table**, not the view table start |
| `0x10055458` | `FUN_10055420`: `0x100013E8 .. 0x100015E8`, count `(end − start) >> 4` passed to `0x100D5D64` | **start of the following table**, not the view table end |

The view table sits between two linker-generated tables, so their bounds share values with it. The patcher
therefore rewrites only the four view literals; `0x101461AC` and `0x10055458` stay stock (rewriting them would
walk the thread table over the new view table and drop the following table). No other code reads the
table: no aligned word in the image points into `0x100004B8..0x100013E8` except two bit-mask constants
(`0x10000800` at `0x1017324C`, `0x10000808` in data at `0x101EC848`), no `movw` of `0x04B8`/`0x13E8`/`0xF30`,
and the count `0xA2` is never used — both readers are bounded by the end pointer. The view manager
`0x10187000..0x10190000` has no `cmp rN, #0xEE..0xF0`; ids are stored as `u16`; the highest stock id is
`0xED`. Byte `+0x13` of an entry (high byte of the `0x0102` field) indexes a 3-entry display table at
`0x38043A78` (`FUN_1018e580`, must be ≤ 2).

Preload result (`FUN_1018dc9c`, `0x1018DE08`): the view is created and its scene prepared
(`FUN_1018e580`, `FUN_1018e6e4`), then `proc(id, 2, view+0x1C)` (or `presenter(id, view+0x1C, 2, …)` when
`+8` is set). A non-zero result logs, calls the scene cleanup `FUN_1018e6bc`, frees the view and returns the
code to `FUN_101898bc`, so the previous view stays on screen. The stub returns `-2` there when the loader
is missing, corrupt or does not fit.

### Mounts and `mkdir` (emulator probe, `fs_get_mnt_point` + mount list walk)

| Mount | Type | API (`mp+0x1C`) | Writable |
|---|---|---|---|
| `/NAND:` | 0 (FAT) | `0x101F6AC0`: all 20 entries set, `mkdir` (`+0x3C`) = `0x101079F1` | yes |
| `/NAND:A`, `B`, `C`, `E`, `K` | 2 | `0x101F5CD8`: open, read, lseek, tell, close, mount, unmount, stat, statvfs only; `write`, `truncate`, `sync`, `opendir…closedir`, `unlink`, `rename`, `mkdir` are 0 | no |

`fs_get_mnt_point` takes the longest prefix, so `/NAND:C/…` reaches the read-only type 2 fs. Stock writes go to
`/NAND:/…` (`/NAND:/epo.bin`, `/NAND:/record.pcm`). The plan's `/NAND:C/wp2apps` (and its fallback, files in
the root of `/NAND:C`) cannot be written; `wp2_core::layout::DIR` is `/NAND:/wp2apps` instead, on the FAT,
which has `mkdir`.

### Calling stock code from gdb

A plain QMP/gdb stop can land in an ISR or the idle thread. Calling a function that takes a mutex there
(`fs_get_mnt_point`, `FUN_101898bc` → `os_mutex_lock 0x101BE218`) panics with `Fault during interrupt
handling`. The harness therefore breaks on the entry of `os_mutex_lock` (only threads call it) and runs the
call from that thread. `os_sleep` is not a usable hook: nothing calls it while the watch idles.

## Plan 2, task 6: LVGL colour format and touch input

Ghidra (`scripts/recon/decomp`) on `0x1013E9AC`, `0x1013E52C`, `0x1012F38C`, `0x10125BD4`, `0x101C4C80..0x101C5000`,
`0x1012EE94`; view delete path disassembled at `0x1018D7FC..0x1018D8AA`.

| Item | Value | Confidence |
|---|---|---|
| `LV_COLOR_DEPTH` | **16**: `lv_canvas_fill_bg` (`0x1013E9AC`) clears a `TRUE_COLOR` buffer with `memset(data, 0, w·h·2)`; stock icons are `TRUE_COLOR_ALPHA` 96×96 in 27648 B (3 B/px) | high |
| `LV_COLOR_16_SWAP` | **0**: the built-in decoder (`0x1012F38C`) turns palette entries into `b>>3 \| (g>>2)<<5 \| (r>>3)<<11`, plain RGB565 | high |
| Framebuffer | `FB_BYTES = 466·466·2 = 0x6A088`, type `FB` (0), align `0x40`; `lv_img_dsc_t.header = 4 \| 466<<10 \| 466<<21` | high |
| `lv_coord_t` | **32 bit** (`LV_USE_LARGE_COORD`): `lv_indev_get_point` copies two words; `lv_point_t` = `{i32 x, i32 y}` | high |
| `lv_obj_add_flag(obj, flags)` | `0x10128D39` (`obj+0x24 \|= flags`); stock calls it with `2` (`LV_OBJ_FLAG_CLICKABLE`) next to `lv_obj_add_event_cb` (`0x10125A95`) | high |
| `0x10125BD4` | **not** `lv_event_get_indev`: it is `lv_obj_remove_event_cb(obj, cb)` (walks `spec_attr` event list, returns bool) | high |
| `lv_event_get_target / current_target / code / param / user_data` | `0x101C4C93` (`e+0`) / `0x101C4C97` (`+4`) / `0x101C4C9B` (`+8 & 0x7F`) / `0x101C4CA3` (`+0x10`) / `0x101C4CA7` (`+0xC`) | high |
| Event parameter of `PRESSED`/`RELEASED` | the active indev (`*0x3802AED4`), as in LVGL 8.3 (`lv_event_send(obj, 1/8, indev_act)` in the indev proc) | high |
| `lv_indev_get_point(indev, &point)` | `0x101C4F57`: pointer/button type → `act_point` (`indev+8`, `+0xC`), else `-1,-1` | high |
| Image cache | `LV_IMG_CACHE_DEF_SIZE = 0`: `_lv_img_cache_open` (`0x1012EE94`) reopens a single static entry per draw and `lv_draw_img` closes it, so no cache entry keeps `&DSC` or the framebuffer | medium |
| View delete order (`FUN_1018DC9C` area) | display op `+0x10` → `proc(id, 4, view+0x1C)` → … → scene cleanup `FUN_1018E6BC` (display op `+4`). The view's own `lv_obj_del` in msg 4 runs before the scene is torn down | medium |
| Layout after preload | the view manager sends msg 3 only after a `{9, 3, id}` message: `ui_view_layout(id)` `0x101D72FF` (`0x10182060(id, 3, 0, 0)`). Stock preload handlers (`goodbye_view`) end with `return ui_view_layout(id)`; a preload that just returns 0 leaves the view created and focused but never laid out (e2e: no `view 240 layout started`) | high |

## Plan 2, task 10: "Apps" row in the stock app list

Ghidra (`scripts/recon/decomp`) on `0x1005E790..0x1005F300`, emulator probes on stock and patched NOR.

### Stock app list

| Item | Value | Confidence |
|---|---|---|
| Entries | `0x101EAF40`, flash, **25 × 12 B** `{u32 icon_res_id, u32 label_res_id, u32 target}`; the ops table follows at `0x101EB06C`, no room. Literals: `0x1005E7E4` (click), `0x1005E9C4` (grid), `0x1005EC40` (list text), `0x1005ED58` (list preload), `0x1005F170` (waterfall preload) | high |
| `target` | `≤ 0xF4` → `FUN_101898bc(target, 0)`, else a Thumb function. Views: 0 `0x6A` sportlist, 3 `0x73`, 10 `0xDC`, 11 `0xC5`, 12 `0x90`, 13 `0x88`, 15 `0x8C`, 16 `0xCF`, 17 `0x1C`, 18 `0x61`, 19 `0x81`, 20 `0xCD`, 21 `0xCE`, 22 `0x86`, 23 `0x43`, 24 `0x44` setlist; functions at 1, 2, 4–9, 14 | high |
| Icons, labels | hash ids into the scene resources `/NAND:A/bt_watch.sty`/`.res` and `/NAND:A/bt_watch.<lang>`; loaded by `FUN_101848d4(scene, ids, out_img_dsc, 0, n)` / `FUN_10184a5c(scene, ids, out_text, n)`. There is no "Apps" string | high |
| Count | 25 is an immediate everywhere (`0x1005ECB8`, `0x1005ED02`, `0x1005F134`, `0x1005F218`, `0x1005F230`, `0x1005F2CE`, click `0x1005E7B0` `cmp r4,#0x18`); `data+8` only stores it | high |
| View data (list) | `applist_view` id `0x12`, desc `0x101EB07C` (`+4` size `0x230`). `data = *(view+0x14)`: `+0` screen, `+4` scroll container (rows 0..24 in order), `+8` u8 count, `+0xC` 25 × `lv_img_dsc_t`, `+0x138` 25 × `TextRec {const char *text; u32 0}`, `+0x200` label `lv_style_t`, `+0x20C` font `/NAND:A/font32M.font`, `+0x214` label height (39). Arrays are packed, so a table entry cannot be added without moving ≈ 20 offsets | high |
| Styles | preload `0x1005E848` reads property `0x2C`; `*(0x101EB0AC + 4·style)`: 0 list desc `0x101EB154` (**default**), 1 waterfall `0x101EB184`. Grid (`0x101EB0B4`) is dead in 1.0.0.73: `applist_grid_view` shares id `0x12` and the lookup returns the first entry | high |
| Style desc | `{u8 flags, u8, u16 view_id, u16 data_size, …, +0xC layout, +0x14 focus, +0x1C preload, +0x20 unload, +0x24 res reload, +0x28 key, +0x2C misc}`; the layout is called only through the RAM pointer to the selected desc (`0x1005E8A4`) | medium |
| Click | event 4 (SHORT_CLICKED) on a row → `0x1005ED75` → `FUN_1005e790(row->user_data)`: `index < 25`, `table[index].target`. Emulator: tap on row 0 → `view:106` (sportlist) | high |
| Row (layout `0x1005EEE4`) | `row = lv_obj_create(cont)`, `user_data = i`, size `0x18A × icon.h`, cb `0x1005ED75` on 4, first row `lv_obj_align(1, 0x10, 0x50)`, then `lv_obj_align_to(row, prev, 0xD, 0, 8)`; vendor image create `0x1005A015` + flag `0x4000` + set_src `0x1005A02D`; vendor label create `0x1005B579`, size `(0x172 − icon.w) × data[+0x214]`, style `data+0x200`, long mode `0x101ACBA5(3)`, set_text `0x1005B70D(&TextRec)`, flag `0x4000`, `lv_obj_align_to(lab, img, 0x14, 0x18, 0)`. After the loop: restore scroll, fisheye once, vendor scrollbar, return 0 | high |
| Unload `0x1005EC51` | save scroll → `lv_obj_clean(scr)` (all rows) → `lv_style_reset(+0x200)` → free 25 images and 25 texts by `data[8]` → close font. `data[8]` must never change | high |

LVGL addresses (Thumb): `lv_obj_create 0x10127CFD`, `lv_obj_get_child(obj, i) 0x1012C3DD` (negative from the end),
`lv_obj_get_child_cnt 0x1012C4AD`, `lv_obj_clean 0x1012C881`, `lv_obj_set_size 0x101294C9`, `lv_obj_set_x 0x101290CD`,
`lv_obj_align 0x101C5DE5`, `lv_obj_align_to 0x1012A87D` (5th argument on the stack), `lv_obj_add_flag 0x10128D39`,
`lv_obj_add_event_cb 0x10125A95`, `lv_obj_add_style 0x1012BB15`, `lv_obj_get_scroll_y 0x101C6515`,
vendor image `set_zoom 0x1005A2D1`.

### The patch (P5, option D)

The rodata word `0x101EB160` (list desc `+0xC`, stock `0x1005EEE5`) points to the stub's `wp2_applist_layout`. It
calls the stock layout and, when that returns 0 with `data[8] == 25` and 25 children in the container, appends a 26th
row built exactly like the stock rows. Its own cb on event 4 calls `FUN_101898bc(0xF0, 0)`. The table, the count, the
indices and the view data stay stock; the row dies with the screen in the stock unload. Only the list style is
covered: with property `0x2C == 1` (waterfall) there is no "Apps" row.

- Icon: own `LV_IMG_CF_INDEXED_1BIT` 96×96 in stub flash (`header = 0x0C018007`, 1160 B: palette `00000000`
  transparent, `FFFFFFFF` white as B,G,R,A, then 12 B per row, MSB = left pixel), a 2×2 grid of rounded squares.
  The stock LVGL 8.3 decoder (`0x1012F921`) handles INDEXED_1/2/4/8 and ALPHA_1/2/4/8; stock icons are
  `TRUE_COLOR_ALPHA` 96×96 (27648 B), too big for the stub.
- **Never zoom the 1-bit icon**: `set_zoom(img, 0xC0)` stops QEMU on an unimplemented Display Engine write
  (`0x4006C020`). The row is therefore outside the stock fisheye (which walks `data[8]` rows) and stays at `x = 0x10`.
- Label: `TextRec {"Apps", 0}` in stub flash. The vendor label keeps the pointer (static flag) and re-reads
  `rec->text` on every refresh (language change), so both must live as long as the label.
- RES heap: the label canvas (A4, 280×39) takes ≈ 5.5 KiB while the list is open and is freed by its destructor
  (`0x101ACB9F`); no leak after unload (emulator).
- The saved scroll position includes the extra row, but the stock layout restores it before the row exists; at
  most the list opens slightly above the end.
- Harness: after 16 swipes `(233,400)→(233,120)` the "Apps" row is last, at `y ≈ 340`; rows 24/23 at `y ≈ 236/132`.
  The stock view manager logs `view <id> focused` for every view (including `0xF0`); `_view_focus:1` is logged
  only by stock procs.

Rejected alternatives: a settings entry (`sysmenulist_view` `0x4C`, same static table shape), a long crown press in
the list (desc `+0x28` key handler; the long-press code and whether the system swallows it are unknown), own
resources (labels and icons come from vendor `/NAND:A` resources).

### BLE install channel (task 5)

- The stock decrypt `0x101B608D` logs `check_extra_data fault` and drops frames with an empty plaintext, so END
  carries one dummy byte.
- `uart0.log` logs every incoming frame as `head_id/ability_id/cmd_id/length_id` and every reply as
  `wewear send ability:… cmd:…`; handy for probes.
- `0x804D` shows the "ble_connect" popup and sets `set_autosleep_time:5000`.
- With the clock off by more than 2 minutes, `0x8004` time sync makes the stock app log `TIMESTAMP OVER 2min
  RECOVERED` and reboot through a factory recovery; the host handshake skips `0x8004`.

## Plan 2, task 7: app thread, time, crown and touch

Ghidra (`scripts/recon/decomp`) on `0x101BE1A2`, `0x10146060`, `0x10105C90`, `0x1019B2D8`, `0x1018B234`, the knob
functions below; disassembly of `0x101842A4`, `0x101BA1C8`, `0x101DB2A6`, `0x1019B7E0`, `0x100D5C90`. Probes:
`runs/e2e/task7-probe{1,2,3}.log`.

| Item | Value | Confidence |
|---|---|---|
| `os_thread_create(buf, size, entry, p1, p2, p3, prio, options, delay_ms)` | `0x101BE1A3` → `k_thread_create(buf, buf + 0x118, size − 0x118, …)`, delay `-1` = never, returns the thread; entry gets `(p1, p2, p3)` | high |
| `k_thread_abort(tid)` | **`0x10105C91`**: `z_thread_entry` (`0x101BA1C8`) calls it with `k_current_get()` (`0x1019B2CD`) after the entry returns; it unpends, aborts the timeout and unlinks the thread. `0x1019B2D8` is a bool "current thread is preemptible" check, not abort | high |
| Thread priorities | stock thread table `0x100003A0` (0x1C B: name, stack, `u16` size, `u8` prio at `+0xA`, p1..p3, entry): created at `-16`, then `k_thread_priority_set(+0xA)`. `ui_service` **3** (its loop `0x1018B234` runs the view messages, so view procs, LVGL timers and event callbacks), `lvglworkq` 2 (`k_work_queue_start` at `0x101842BC`), `bluetooth` 2, `media` 1, `gps_service` 4, `sensor_service` 5, `logsrv` 0xB, `vendor`/`ble_fate`/`wewear`/`wewear_ftp` 0xD. Apps run at **4** | high |
| Time | `sys_clock_tick_get()` `0x1019B7E1` returns `u64` ticks; 1 tick = 1 ms (`os_sleep(ms)` hands ms to `k_sleep` unscaled; the shell `uptime` prints the same value as ms via `0x101DB2A6`) | high |
| Crown rotation | `view_key 0xFFFF006F/6E` reaches only list views. Any other view gets rotation only through an LVGL object registered as the display's knob object: `lv_obj_add_event_cb(obj, cb, 0x2D, …)` registers it (`0x10125A94` → `lv_disp_add_knob_obj` `0x10128C50`; also `lv_obj_add_flag` bit 30), and the object then gets event `0x2D` whose param points to the `i32` step. `lv_obj_del` unregisters it (`0x1012C644` → `0x10127D14`). The loader maps step > 0 to `+1`; `r 80` → `0x6F` (+1) in a list view, the sign of `0x2D` itself is not observed | high / sign low |
| Event callback filter | filter 0 (`LV_EVENT_ALL`) also delivers every draw event (`0x12`–`0x2C`) each frame and does not register the knob; the loader registers one callback per code (1 `PRESSED`, 8 `RELEASED`, `0x2D`) | high |
| Short tap | `PRESSED` at `(200, 140)` but `RELEASED` reports y = 103 for a 125 ms tap (correct after a 0.5 s hold): the menu starts the row that was pressed. Apps see `TOUCH_UP` with that point | high (emulator) |
| Button | unhandled `0x20000001` is ignored; unhandled `0x04000001` makes the stock jump back to the clock (`view_stack: jump 240 -> 16`). The loader handles `0x04000001` only while an app runs | high |
| `OPEN_VIEW(0x10, 0)` | jumps the view stack to the clock and deletes view 0xF0 (msg 4) | high |
| Block placement | opening 0xF0 deletes the face, and the loader (`0x380A9A38`) and app (`0x380A9678`) take the freed face region every time; a block allocated from outside while the face shows cannot move the loader. A block of `app_size` allocated with the menu open moves the app (`0x380A9758`) | high (emulator) |
| Screen-off after BLE | the panel blanks ~5.7 s after `Ble wake lock 0` (the install connection closing) although `keep_awake` set the timeout to `0xFFFFFFFF`; with the screen off no thread enters `os_mutex_lock` and `emu.call` hangs. `HelloTest.ready` waits for it and wakes the watch with the button | medium |

## Hardware: caches and MPU

Ghidra (`scripts/recon/decomp`) on `0x10142C80`, `0x100F0084`, `0x101BEE00`, `0x101BEE52`, `0x10199CC8`,
`0x100F6708`, `0x100F67CC` and callers; capstone on `0x100F0072…0x100F00F4`, `0x100F6804`, `0x101053C0`,
`0x10105534…0x10105C88`; a literal scan for `0xE000EF50…0xE000EF7C`, `0xE000ED14`, `0xE000ED9x`; a scan for
`BL` to the functions below and for `MSR CONTROL`.

| Item | Value | Confidence |
|---|---|---|
| Core caches | none used. No literal in `0xE000EF50…0xE000EF7C` (ICIALLU, DCIMVAC, DCISW, DCCMVAC, DCCIMVAC), and the one `0xE000ED14` literal (`0x10105418`) is the `SHPR` base of `NVIC_SetPriority` (`0x101053E4`, `[0xE000ED14 + (irq & 0xF)]` for irq < 0), not a CCR write. The Cortex-M33 has no architectural L1 cache; Zephyr is built without cache support | high |
| PSRAM cache | the vendor SPI1 cache in front of PSRAM. Ops register `0x40018004` (bit 0 start/busy, bit 1 range, bit 3 invalidate, bit 4 write back; `0x1F` with bit 2 is the write-buffer flush), range `0x40018008` start / `0x4001800C` last byte, both CPU addresses. Driver `0x100F0084 (op, addr, size)`, no-op for `addr` outside `0x34000000…0x3FFFFFFF`: op 1 write back range (`0x13`), 2 invalidate range (`0x0B`), 3 write-buffer flush (`0x1F`), 4 write back all (`0x11`), 5 invalidate all (`0x09`), 6 write back + invalidate range (`0x1B`), 7 same for all (`0x19`); range ops and 3, 5 busy-wait on bit 0 (`0x100F0074`) | high (code), bit names medium |
| Mappings | `0x38…` is write-back cached: `0x101BEE00 (addr, size)` does op 3 + op 1 for `0x38000000…0x3BFFFFFF` and only op 3 for `0x34…`/`0x3C…` (write-through and uncached). This matches the alias table above | high |
| Stock routine used | **`0x101BEE52 (addr, size)`**: under the k_mutex `0x380040D0` (`0x10101BDC` → `k_mutex_lock` `0x10199CC8`, asserts when called from an ISR) it runs op 3 then op 6 for `0x34000000…0x3BFFFFFF`, op 3 alone for `0x3C…`, returns 0. Stock calls it from UI and GPU paths on PSRAM buffers before DMA (`0x100FFA70`, `0x10142574`, `0x1018898C`, `0x101CD670`) | high |
| `lv_gpu_acts_flush_dcache` (`0x10142C80`) | only widens a dirty-range record and forwards; no cache operation | high |
| Loader/app load | `wp2_core::load::load` calls `0x101BEE52(block, image_size)` after relocation and CRC, with `dsb` before and `dsb; isb` after. The stub loads the loader through it, the loader loads apps through it. Callers run in the UI thread, never in an ISR or under `BASEPRI` | decision |
| MPU setup | boot `0x100F6804` (prints `arm mpu init`) and PM resume `0x101BAC78` call `0x100F67CC`, which sets three regions through `0x100F6708 (idx, base, size, type)`. The first region enables the MPU: `MAIR0 = 0x44444444`, `MPU_CTRL = 5` (ENABLE + PRIVDEFENA). `RBAR = base | attr`, `RLAR = (base + size − 0x20) | 1`; attr `0x1F` type 0 (RO, XN), `0x1A` type 1 (RW), `0x1E` type 2 (RO, executable) | high |
| MPU regions | 0: `0x00000000…0x0000FFFF` RO exec; 1: `0x10000000…0x10FFFFFF` RO exec (NOR XIP); 2: `0x38000000…0x38000A77` RO exec. None is XN and none covers `ResFbHeap` `0x380A9340…0x3871EEC0`; that range falls to the privileged default map (SRAM region, executable). Other regions come only from the debug shell `mpu_set` (`0x100F3F7E`) | high |
| Thread privilege | all threads privileged: the only `MSR CONTROL` writes (`0x10105544`, `0x101055E0`, `0x10105C80`, `0x10253642`) clear FPCA or set SPSEL, never nPRIV; no userspace syscall strings | high |

Unverified on hardware: whether the SPI1 cache serves instruction fetches from `0x38…` through the same lines as
data (if it does, the write back alone would be enough; the invalidate costs only refills). The emulator does
not model the controller (stock calls it during normal drawing, so its writes are accepted there).
