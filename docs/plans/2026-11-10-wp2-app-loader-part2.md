# Загрузчик приложений WP2, часть 2: заглушка, загрузчик, установка — план реализации

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Этапы 2–4 спеки. В стоковом `app.bin` 1.0.0.73 появляется маленькая заглушка. Загрузчик лежит на NAND и ставится по своему BLE-каналу вместе с приложениями. Через пункт «Apps» в эмуляторе запускаются `hello` и `vibration-lab`; стоковое всплывающее окно ставит приложение на паузу, битый загрузчик и нехватка памяти не мешают часам работать как сток.

**Architecture:** Патч `app.bin` состоит из подмены литералов и трёх инструкций `bl`, а сама заглушка дописывается в конец образа (XIP, `0x1025A080`). `ResFbHeap` уменьшается на `0x1000`, и под ней постоянно лежит только RAM заглушки. Загрузчик и приложения — перемещаемые пакеты: образ слинкован с базой 0, к нему приложена таблица смещений `R_ARM_ABS32`. Заглушка при открытии view `0xF0` берёт блок из `ResFbHeap` через стоковый `ui_mem`, копирует в него загрузчик с NAND, правит адреса и отдаёт ему сообщения view; при закрытии view блок возвращается. Так же загрузчик поступает с приложением при запуске и выходе. Стоковая JHeap при нехватке памяти не возвращает NULL, а перезагружает часы. Поэтому каждое выделение сначала проверяется по списку свободных блоков под тем же `BASEPRI`, что и у JHeap. Если блок не помещается, view не открывается, а приложение показывает `Error -12`. Свой BLE-канал (системные команды `0xFFFF:0x90A0…0x90A4`, расшифровка стоковым ключом сессии) живёт в заглушке, поэтому загрузчик всегда можно переустановить. Загрузчик рисует в свой RGB565-буфер, показанный стоковым `lv_img`, приложение запускает в отдельном потоке Zephyr и реализует ABI 1 `Api`.

**Tech Stack:** Rust `no_std` (`thumbv7em-none-eabi`): новый workspace `fw/` с крейтами `wp2-core` (логика, тестируется на хосте), `wp2-stub`, `wp2-loader`. Python 3.12 (uv, unittest, `pycryptodome`): упаковщик с релокациями, патчер прошивки, BLE-клиент по H4 через Netsim, e2e-харнес эмулятора. Ghidra headless и capstone для разведки. QEMU с логом мотора из `../cmf-watch-utils-motor-log`, gdb и llvm из `nix-shell -p gdb llvm`.

**Spec:** `docs/specs/2026-11-10-wp2-app-loader-design.md` (часть 1: `docs/plans/2026-11-10-wp2-app-loader-foundations.md`, разведка: `docs/recon.md`, раздел «Plan 2 revision: memory while running» — аллокатор)

**Нумерация после ревизии:** добавлена задача 2 (перемещаемые пакеты). Старые задачи 2–11 стали 3–12. Решения в журнале контроллера с номерами `T2…T11` относятся к задачам `3…12`. Решение «T3 добавляет `--kind` в `wp2apps pack`» переехало в задачу 2.

## Global Constraints

- Все правила `AGENTS.md`. Записывать что-либо на физические часы в этом плане запрещено. Последний пункт (этап 5) выполняется только после явного «да» владельца.
- В коммиты не попадают `app.bin`, пропатченный образ, NOR/NAND, `*.wp2app`, `*.pkg`, `runs/`, `target/`, `fw/target/`, секрет синтетического сопряжения. MAC в коде и тестах — только `AA:BB:CC:DD:EE:FF`.
- Код из `cmf-watch-utils` не копируется и не импортируется в `src/`. Его CLI (`emu run`, `emu pair-fixture`, `emu faces`) вызывается как внешняя программа, и только из e2e-харнеса. BLE-клиент и рукопожатие пишутся заново по описанию протокола из этого плана.
- Хост-инструменты берутся из `nix-shell -p …`; `netsimd` — из Android SDK (`$WP2_NETSIMD`, по умолчанию `~/Library/Android/sdk/emulator/netsimd`).
- Все адреса действительны только для JX402 `1.0.0.73`, sha256 `app.bin` начинается с `30e7978c39d8fb79`. Патчер сверяет старые байты в каждой точке патча и при расхождении отказывает.
- **Карта памяти (решение владельца 2026-11-10, заменяет резерв 196 KiB):**

  | Что | Где | Размер |
  |---|---|---|
  | заглушка (XIP, только чтение) | `STUB_BASE = 0x1025A080` (смещение в `app.bin` `0x25A080`) | `STUB_MAX = 0x4000` вместе с таблицей view |
  | `ResFbHeap` | `0x380A9340` | `0x676B80` → **`0x675B80`** (все три литерала), конец `0x3871EEC0` |
  | RAM заглушки | `STUB_RAM = 0x3871EEC0` | `STUB_RAM_SIZE = 0x1000` (до `0x3871FEC0`, где кончалась стоковая куча): `StubRam` ≤ `0xE00`, по `STUB_RAM + 0xE00` спин-заглушка харнеса |
  | загрузчик | блок `ui_mem` типа `RES` (2), выделяется на preload view `0xF0`, освобождается после delete | образ ≤ `LOADER_SIZE = 0x10000` |
  | приложение | блок `RES`, выделяется при запуске, освобождается при остановке | образ ≤ `APP_SIZE = 0x20000` |
  | буфер кадра | блок `FB` (0), `466·466·2 = 0x6A088` Б, выделяется на preload, освобождается на delete | — |

  Фиксированных адресов у загрузчика и приложений нет. `LOADER_BASE` и `APP_BASE` удаляются отовсюду.
- **Память (разведка, `docs/recon.md`):** `ui_mem_aligned_alloc(type, align, size, caller) = 0x101882C5`, `ui_mem_free(type, ptr) = 0x10188381`. Тип `0` FB (align ≤ `0x40`), тип `2` RES; оба берут память из `ResFbHeap` (JHeap `HEAP = 0x38043824`: `+8` свободно байт, `+0x60` голова списка свободных блоков `{next, size & 0xFFFFFF}`). JHeap при неудаче вызывает `MllcFailedHook` и перезагружает часы. Поэтому `ui_mem_*` вызываются только через `wp2_core::heap::alloc`. Он под `BASEPRI = 0x20` проверяет `free ≥ need` и наличие блока `≥ need`, где `need = size + align + 0x100`. Если проверка не прошла, возвращается NULL и `ui_mem` не вызывается. `caller` = указатель на `b"wp2\0"`.
- **Перемещаемые образы:** линковка с `ORIGIN = 0`, флаги `-C link-arg=--emit-relocs -C target-feature=+no-movt` (без них rustc выдаёт пары MOVW/MOVT, см. `docs/recon.md`). Поддерживается только `R_ARM_ABS32`: загрузка прибавляет базу блока к 32-битному слову по каждому смещению. PC-относительные релокации внутри образа (`R_ARM_THM_CALL`, `THM_JUMP24/19/11/8`, `THM_PC8/12`, `THM_ALU_PREL_11_0`, `REL32`, `PREL31`, `NONE`) пропускаются. Остальные типы и любая релокация на символ `SHN_ABS` — ошибка упаковщика. Стоковый код загрузчик и приложения вызывают только через константы адресов (`stock.rs`), а не через символы линкера.
- **PWM мотора (поправка к части 1):** такт канала задаёт `CMU_PWMCLK` `0x40001068 + 4·ch`. Поле делителя `0..255` означает `/(n+1)`, значения `256..260` — `/512 … /8192`, биты 9–10 выбирают источник (`0` — RC 16K/32K, `1` — HOSC 32 MHz, `2` — 64M). Сток пишет `0x303` (HOSC/4096 = 7812,5 Гц) и period `0xFF`, отсюда **≈30,6 Гц**. Загрузчик восстанавливает `0x303` на канале 0 при выключении мотора, паузе и выходе.
- Защита мотора: непрерывная работа не дольше **2000 мс** (сторож с шагом 20 мс, допуск ≤ 2020 мс), скважность в промилле `0..=1000`, частота `1..=20000` Гц, вне диапазона `motor` возвращает `-1` и регистры не трогает. Пока приложение на паузе, `motor` возвращает `-3` и ничего не делает.
- **Пакет (заменяет формат части 1, на устройствах его ещё нет):** заголовок 0x40 Б, `struct.Struct("<4sHBB24sIIIIII8s")`:

  | off | поле |
  |---|---|
  | 0x00 | magic `WP2A` |
  | 0x04 | `u16 abi` (= 1) |
  | 0x06 | `u8 kind` (0 app, 1 loader) |
  | 0x07 | `u8 abi_minor` |
  | 0x08 | `name[24]`, UTF-8, 1..=23 Б, дополнено NUL |
  | 0x20 | `u32 version` |
  | 0x24 | `u32 image_size` (с `.bss`, он внутри образа) |
  | 0x28 | `u32 entry` (смещение, `< image_size`) |
  | 0x2C | `u32 reloc_count` (было `load_addr`) |
  | 0x30 | `u32 crc32(image ‖ relocs)` |
  | 0x34 | `u32 crc32(header[0:0x34])` |
  | 0x38 | 8 нулевых байт |

  За заголовком идёт `image`, за ним `relocs`: `reloc_count` значений `u32` LE, строго возрастающих, каждое `+ 4 ≤ image_size`. Длина файла равна `0x40 + image_size + 4·reloc_count`. Ограничения: `kind == 1` ⇒ `image_size ≤ LOADER_SIZE`, `kind == 0` ⇒ `image_size ≤ APP_SIZE`. `Api`: `abi: u16` (мажорная версия, сравнение `==`), `abi_minor: u16` (у загрузчика `>=`, чем у приложения), `width: u16`, `height: u16`, затем указатели функций; новые функции только дописываются в конец. Загрузчик этого плана — ABI `1.0`.
- Тесты: `uv run --locked python -m unittest discover -s tests -v` (e2e пропускаются без `WP2_E2E=1`); `(cd sdk && cargo test --target aarch64-apple-darwin)`; `(cd fw && cargo test -p wp2-core --target aarch64-apple-darwin)`. E2E: `WP2_E2E=1 nix-shell -p gdb llvm --run 'uv run --locked python -m unittest tests.test_e2e -v'`. Один прогон e2e занимает минуты; долгие команды идут в отдельную панель (CLAUDE.md владельца), логи — в `runs/e2e/`.

## Review Focus

1. Связь пропала посреди установки, или пришёл `DATA` с чужим смещением. Ожидается, что активная версия слота не меняется, `.tmp` не становится пакетом, следующий `BEGIN` начинает заново. Тест — задача 5 (`test_install_interrupted_keeps_old`).
2. В куче нет блока под загрузчик, буфер кадра или приложение. Ожидается, что `ui_mem` не вызывается (нет `MllcFailedHook`, ровно одна загрузка Zephyr). View «Apps» не открывается (`loader_state = 3`), приложение показывает `Error -12`, сток работает дальше. Тесты — задача 6 (`test_loader_no_memory_keeps_stock`), задача 7 (`test_app_no_memory_shows_error`, `test_big_app_starved_heap`).
3. Блоки не возвращаются в кучу после выхода из приложения, закрытия view или удаления. Ожидается, что после пяти циклов «открыть → запустить → выйти → закрыть» свободная память `ResFbHeap` (`*(HEAP + 8)`) отличается от первого цикла меньше чем на `0x1000`, а `app_block` и `loader_block` в `StubRam` равны 0. Тест — задача 7 (`test_blocks_freed_and_relocated`).
4. Приложение включило мотор и зависло в цикле без `next_event`. Ожидается, что мотор всё равно выключится не позже 2020 мс, потому что сторож работает в UI-потоке. Тест — задача 8 (`test_motor_cap_without_event_loop`, пример `motor-hog`).
5. Стоковое окно появилось, когда мотор включён. Ожидается, что до показа окна мотор выключен, в `CMU_PWMCLK0` снова `0x303`, а `motor` на паузе возвращает `-3`. Тест — задача 9 (`test_popup_pauses_and_releases_motor`).
6. На NAND лежит загрузчик или приложение с чужим ABI, `abi_minor` больше, чем у загрузчика, испорченным заголовком, образом или таблицей релокаций. Ожидается отказ при установке; если файл всё же оказался на NAND — отказ при загрузке, блок освобождён, прыжка в образ нет. Тесты — задача 3 (`pkg`, `relocate`), задача 5 (`test_install_rejects_corrupt`), задача 11 (`test_broken_loader_boots_stock`).
7. Свайп назад или закрытие view во время работы приложения. Ожидается, что поток приложения остановлен, мотор выключен, буфер, таймер, объект `lv_img` и оба блока освобождены, а следующее открытие «Apps» работает. Тест — задача 7 (`test_view_delete_stops_app`).

---

## Находки разведки для этого плана

Источники: `scripts/recon/decomp` (Ghidra), capstone, `runs/window-174132/uart0.log`. Уверенность обозначена так же, как в `docs/recon.md`. Задача 1 уже перенесла таблицу в `docs/recon.md` («Plan 2 findings», в рабочем дереве). Аллокатор описан там же, в разделе «Plan 2 revision: memory while running».

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
| Память | `ui_mem_aligned_alloc 0x101882C4`, `ui_mem_free 0x10188380`, типы `0 FB`, `1 GUI`, `2 RES`; FB и RES — одна `ResFbHeap` (JHeap `0x38043824`); JHeap не возвращает NULL, а перезагружает часы через `MllcFailedHook`; дамп статистики `0x1018858C` | high (подробности — `docs/recon.md`) |
| ОС | `os_thread_create(buf, size, entry, p1, p2, p3, prio, opts, delay) 0x101BE1A2` (в начале `buf` лежит `k_thread` на 0x118 Б); `os_sleep(ms) 0x101BE1EC`; `k_thread_abort` — `0x10105C90` или `0x1019B2D8` | high / abort low |
| Всплывающие окна | `msgbox_cache_popup(id, udata) 0x100D5E34`; `set_popup_msgbox 0x100D6EB0` / `set_close_msgbox 0x100D6FB4` ведут флаги `*(u8*)0x3800FAF4` (бит 0 и бит 1 = окно открыто), id в `0x3800FAF6`/`0x3800FAF8` | high |
| PWM | `struct device` "PWM" `0x101DF1C0`, api `0x101FB190`: `[0] pin_set_cycles(dev, ch, period, pulse, flags) 0x1012197D`, `[4] off(dev, ch) 0x101214AD`, `[6] set(dev, ch, hz, duty%, pol) 0x10121681`; период — 16 бит | high |
| Список приложений | иконки из ресурсов сцены, построитель `FUN_1005e9d8` (число по `data+8`, записи по 3 слова с `data+0xC`, клик `0x1005EBA1`, событие 4); процедуры list `0x1005EEC9`, grid `0x1005EBC5`, waterfall `0x1005F101` | low, разбирает задача 10 |
| Эмулятор | ввод `input.txt`: `t down x y`, `c down` (коронка/питание), `r delta`, `p mV charger percent`; без стокового профиля `STOCK_AWAKE` не включается, поэтому харнес пишет `0xFFFFFFFF` в `**0x38037D14` (таймаут автосна, `system_set_auto_sleep_time`); NAND живёт в памяти QEMU и переживает сброс гостя | high |

---

### Task 1: Патчер прошивки, e2e-харнес, проверка резерва 4 KiB и кучи

Задача уже начата: в рабочем дереве лежат незакоммиченные `firmware.py`, `paths.stock_app`, CLI `firmware`/`nor`, `tests/test_firmware.py`, `tests/e2e/harness.py`, `tests/test_e2e.py`, заметки в `docs/recon.md` и фиксация `cmf-watch-utils` на `freethinkel/cmf-watch-utils@0f768dc`. Отчёт: `.superpowers/sdd/2026-11-10-wp2-app-loader-part2/task-1-report.md`. Продолжать на этом дереве, ничего не переписывать заново.

**Files:**
- Modify: `src/wp2apps/firmware.py` (`HEAP_NEW`, `HEAP_BASE`, `STUB_RAM`, `STUB_RAM_SIZE`)
- Revert: `src/wp2apps/package.py` (`git restore src/wp2apps/package.py`; границы загрузчика переезжают в задачу 2 без `LOADER_BASE`)
- Modify: `tests/test_firmware.py` (новое значение кучи; `test_loader_package_bounds` удалить — его новая форма в задаче 2)
- Modify: `tests/e2e/harness.py` (`STUB_RAM` из `firmware`, `heap()`, константы памяти; дописать выбор тяжёлого циферблата)
- Modify: `tests/test_e2e.py` (`ReservationTest` на 4 KiB, новый `HeapTest`)
- Modify: `docs/recon.md` (устаревшие строки про `0x655CC0`/`0x645CC0` и 196 KiB; отклонённые циферблаты; измеренный оверхед JHeap)

**Interfaces:**
- Produces (`firmware.py`, к уже написанному):
  - `HEAP_BASE = 0x380A9340`, `HEAP_NEW = 0x675B80`, `STUB_RAM = HEAP_BASE + HEAP_NEW` (`0x3871EEC0`), `STUB_RAM_SIZE = 0x1000`
  - остальное без изменений: `STUB_OFFSET`, `HEAP_LITERALS`, `HEAP_OLD`, `sum32`, `fix_acth`, `thumb_bl`, `thumb_b_w`, `put`, `patch(app, stub=None, *, _verify=True)`, `stage_nor`, CLI `wp2apps firmware --stub STUB --output OUT`, `wp2apps nor NOR --app APP --output OUT`
- Produces (`tests/e2e/harness.py`, к уже написанному `Emu`, `walk`, `swipe`, `stock_nor`, `face_fixtures`, `control_walk`, `heavy_face_fixtures`):
  - `STUB_RAM = firmware.STUB_RAM`; `HEAP = 0x38043824`; `UI_MEM_ALIGNED_ALLOC = 0x101882C5`; `UI_MEM_FREE = 0x10188381`; `HEAP_DUMP = 0x1018858D`
  - `heap(emu) -> tuple[int, int]` — `(free, max_block)`: `free = u32(HEAP + 8)`; обход `p = u32(HEAP + 0x60)`, пока `p != 0`: `size = u32(p + 4) & 0xFFFFFF`, `p = u32(p)`; не больше 4096 шагов, иначе `RuntimeError`
  - `u32(emu, addr) -> int` — `struct.unpack("<I", emu.peek(addr, 4))[0]`

- [ ] **Step 1: Привести дерево к новому решению о памяти**

`git restore src/wp2apps/package.py`; в `tests/test_firmware.py` удалить `test_loader_package_bounds` и заменить `0x645CC0` на `firmware.HEAP_NEW`; добавить в `firmware.py` константы из Interfaces; в харнесе `STUB_RAM = firmware.STUB_RAM`. В `docs/recon.md` заменить предложение про `0x645CC0`/`0x386EF000` на: патчер меняет все три литерала на `0x675B80`, куча кончается на `0x3871EEC0`, выше лежит только RAM заглушки (`0x1000`). Строку «Reservation» части 1 (`0x655CC0`) пометить «superseded by plan 2 revision: 4 KiB stub RAM only».

- [ ] **Step 2: Write the failing tests** (`tests/test_firmware.py`)

```python
def test_heap_shrinks_by_stub_ram_only(self):
    self.assertEqual(firmware.HEAP_OLD - firmware.HEAP_NEW, 0x1000)
    self.assertEqual(firmware.STUB_RAM, 0x3871EEC0)
    self.assertEqual(firmware.STUB_RAM + firmware.STUB_RAM_SIZE, 0x380A9340 + 0x676B80)

@skipUnless(HAVE_STOCK, "needs JX402_V1.0.0.73.bin")
def test_heap_literals_patched(self):                       # существующий тест, новое значение
    out = firmware.patch(STOCK)
    for off in firmware.HEAP_LITERALS:
        self.assertEqual(struct.unpack_from("<I", out, off)[0], 0x675B80)
```

- [ ] **Step 3: Run to verify failure**

Run: `uv run --locked python -m unittest tests.test_firmware -v`
Expected: FAIL — `AttributeError: … 'STUB_RAM'`.

- [ ] **Step 4: Implement the constants**

- [ ] **Step 5: Run to verify pass**

Run: `uv run --locked python -m unittest discover -s tests -v`
Expected: PASS (тесты на стоке пропускаются только без прошивки).

- [ ] **Step 6: Дописать выбор тяжёлого циферблата** (решение контроллера в журнале)

`heavy_face_fixtures()` уже переписан: берёт топ-10 по загрузкам и перебирает их по убыванию размера. Для каждого запускается контрольный обход стока (`control_walk`, 240 с). Берётся первый, у которого ровно одна загрузка Zephyr, нет `MllcFailedHook` и нет `FATAL`. Выбор и результаты контроля кешируются в `runs/e2e/fixtures-heavy/face.json`. Прокрутки карусели в `walk` нет. Осталось одно: в `docs/recon.md` под заметкой про `hvk9m1hmpyuheyd` перечислить отклонённые id, их контрольные результаты из `face.json` и выбранный циферблат.

- [ ] **Step 7: Write the e2e tests** (`tests/test_e2e.py`)

```python
MARK = bytes.fromhex("a55ac33c")
START, END = firmware.STUB_RAM, firmware.STUB_RAM + firmware.STUB_RAM_SIZE

class ReservationTest(unittest.TestCase):
    def test_reservation_survives_ui_walk(self):
        nor = firmware.stage_nor(harness.stock_nor(), firmware.patch(paths.stock_app()))
        with harness.Emu("reserve", nor=nor, fixtures=harness.heavy_face_fixtures()) as emu:
            emu.wait_uart("launcher enter", 60)
            emu.keep_awake()
            emu.poke(START, MARK * ((END - START) // 4))
            harness.walk(emu, seconds=240)
            uart = emu.uart()
            self.assertEqual(uart.count("*** Booting Zephyr"), 1, "guest rebooted")
            self.assertEqual(emu.peek(START, END - START), MARK * ((END - START) // 4))
            self.assertNotIn("MllcFailedHook", uart)
            self.assertNotIn("ZEPHYR FATAL ERROR", uart)

class HeapTest(unittest.TestCase):
    def test_walk_matches_stock_dump(self):
        # patched image, default fixtures; до трёх попыток, хотя бы одна совпадает точно:
        #   emu.call(HEAP_DUMP); (free, biggest) = harness.heap(emu)
        #   последняя строка UART r"Free:(\d+).*MaxBlock:(\d+)" == (free, biggest)

    def test_alloc_free_roundtrip(self):
        # free0 = heap(emu)[0]; p = emu.call(UI_MEM_ALIGNED_ALLOC, 2, 8, 0x10000, 0)
        # p != 0, p % 8 == 0, 0x380A9340 <= p < firmware.STUB_RAM
        # 0x10000 <= free0 - heap(emu)[0] <= 0x10000 + 8 + 0x100      # граница SLACK из Global Constraints
        # emu.call(UI_MEM_FREE, 2, p); heap(emu)[0] == free0
```

Если строки `MaxBlock` в UART нет (дамп печатает в другой канал), `test_walk_matches_stock_dump` проверяет только, что `biggest ≤ free` и что блок `biggest − 0x200` выделяется и освобождается через `call`. Это записывается в `docs/recon.md`. Фактический оверхед `free0 − free1 − 0x10000` тоже записать в `docs/recon.md`. Если он больше `0x108`, остановиться: `SLACK` в Global Constraints неверен.

- [ ] **Step 8: Run e2e**

Run (в отдельной панели): `WP2_E2E=1 nix-shell -p gdb llvm --run 'uv run --locked python -m unittest tests.test_e2e.ReservationTest tests.test_e2e.HeapTest -v' 2>&1 | tee runs/e2e/task1.log`
Expected: PASS. Если `ReservationTest` падает, а контроль стока на том же циферблате проходит, план останавливается: резерв 4 KiB ломает сток.

- [ ] **Step 9: Commit**

```bash
git add pyproject.toml uv.lock src/wp2apps/firmware.py src/wp2apps/cli.py src/wp2apps/paths.py tests/test_firmware.py tests/e2e tests/test_e2e.py docs/recon.md
git commit -m "feat: firmware patcher with 4 KiB stub RAM reservation, heap probes and e2e harness"
```

---

### Task 2: Перемещаемые пакеты (упаковщик, SDK, тесты части 1)

**Files:**
- Modify: `src/wp2apps/package.py`, `src/wp2apps/cli.py` (`pack --elf` обязателен, `--kind {app,loader}`; `info` печатает `relocs` вместо `load_addr`)
- Modify: `sdk/cmf-wp2-app/app.ld` (`ORIGIN = 0`), `sdk/.cargo/config.toml` (rustflags); `scripts/build-app` уже передаёт `--elf` и не меняется
- Test: `tests/test_package.py`, `tests/test_sdk.py`

**Interfaces:**
- Produces (`package.py`):
  - `HEADER = struct.Struct("<4sHBB24sIIIIII8s")`, `HEADER_CRC_SPAN = 0x34`, `APP_SIZE = 0x20000`, `LOADER_SIZE = 0x10000`; `APP_BASE`, `LOADER_BASE` удалены
  - `@dataclass(frozen=True) class Package: kind; abi; abi_minor; name; version; entry; image: bytes; relocs: tuple[int, ...]`
  - `pack(image: bytes, *, name: str, version: int, entry: int = 0, kind: int = KIND_APP, relocs: Sequence[int] = ()) -> bytes`
  - `parse(data: bytes) -> Package` — проверки из Global Constraints, `ValueError` с причиной
  - `relocations(elf: bytes) -> list[int]` — смещения `R_ARM_ABS32` по возрастанию
  - `check_elf(image: bytes, elf: bytes) -> None` — `e_entry == 1`; конец alloc-секций равен `len(image)`
  - CLI: `wp2apps pack IMAGE --elf ELF --name N --version V [--kind app|loader] --output OUT` (relocs из `relocations(elf)`)

- [ ] **Step 1: Write the failing tests** (`tests/test_package.py`; существующие тесты правятся так)

```python
def test_round_trip(self):
    image = bytes(range(256)) * 4
    pkg = package.parse(package.pack(image, name="Vibration Lab", version=3, relocs=[0, 8]))
    self.assertEqual((pkg.name, pkg.version, pkg.kind, pkg.abi, pkg.entry, pkg.image, pkg.relocs),
                     ("Vibration Lab", 3, package.KIND_APP, 1, 0, image, (0, 8)))

def test_size_limits(self):                                  # заменяет test_recon_values
    self.assertEqual((package.APP_SIZE, package.LOADER_SIZE), (0x20000, 0x10000))
    package.pack(b"\0" * 16, name="loader", version=1, kind=package.KIND_LOADER)
    for kind, size in ((package.KIND_APP, 0x20001), (package.KIND_LOADER, 0x10001)):
        with self.assertRaises(ValueError):
            package.pack(b"\0" * size, name="x", version=1, kind=kind)

def test_relocs_checked(self):
    for relocs in ([8, 0], [0, 0], [13]):                    # не по возрастанию, дубль, выход за образ (16 Б)
        with self.assertRaises(ValueError):
            package.pack(b"\0" * 16, name="x", version=1, relocs=relocs)

def test_reloc_table_covered_by_crc(self):
    data = bytearray(package.pack(b"\0" * 16, name="x", version=1, relocs=[4]))
    data[-1] ^= 1
    with self.assertRaises(ValueError):
        package.parse(bytes(data))

def test_relocations_from_elf(self):
    elf = fake_elf(1, [(0, 0x30)], rels=[(0x10, R_ARM_ABS32, TEXT), (0x04, R_ARM_THM_CALL, TEXT),
                                         (0x20, R_ARM_ABS32, TEXT)])
    self.assertEqual(package.relocations(elf), [0x10, 0x20])

def test_relocations_reject_movw_and_absolute_symbols(self):
    with self.assertRaisesRegex(ValueError, "no-movt"):
        package.relocations(fake_elf(1, [(0, 0x30)], rels=[(0x10, 47, TEXT)]))      # R_ARM_THM_MOVW_ABS_NC
    with self.assertRaisesRegex(ValueError, "absolute"):
        package.relocations(fake_elf(1, [(0, 0x30)], rels=[(0x10, R_ARM_ABS32, SHN_ABS)]))

def test_elf_linked_at_zero(self):                           # заменяет три теста check_elf
    package.check_elf(b"\0" * 0x30, fake_elf(1, [(0, 0x30)]))
    with self.assertRaises(ValueError):
        package.check_elf(b"\0" * 0x30, fake_elf(0x386FF001, [(0x386FF000, 0x30)]))
    with self.assertRaises(ValueError):
        package.check_elf(b"\0" * 0x30, fake_elf(1, [(0, 0x30), (0x30, 0x8)]))      # .bss вне образа
```

`fake_elf(entry, sections, rels=())` расширяется: секция `.symtab` с одним символом на каждую пару `(тип, shndx)` и секция `SHT_REL` (`sh_info` → первая alloc-секция, `sh_link` → `.symtab`). `TEXT = 1` — индекс первой alloc-секции, `SHN_ABS = 0xFFF1`. `test_cli_pack_info` передаёт `--elf` с `fake_elf(1, [(0, 40)])`; `test_entry_size_and_address_bounds` теряет проверку `load_addr`.

`tests/test_sdk.py`:

```python
pkg = package.parse((ROOT / "target" / f"{name}.wp2app").read_bytes())
self.assertEqual((pkg.name, pkg.entry), (name, 0))
self.assertTrue(pkg.relocs)
for off in pkg.relocs:                                       # каждое исправляемое слово указывает в образ
    self.assertLess(struct.unpack_from("<I", pkg.image, off)[0] & ~1, len(pkg.image))
```

- [ ] **Step 2: Run to verify failure**

Run: `uv run --locked python -m unittest tests.test_package tests.test_sdk -v`
Expected: FAIL — `pack() got an unexpected keyword argument 'relocs'`.

- [ ] **Step 3: Implement**

`relocations`: ELF32 LE, обход заголовков секций. Учитываются только `SHT_REL` (9), у которых целевая секция `sh_info` имеет `SHF_ALLOC`; `SHT_RELA` — ошибка. Запись: `r_offset`, `type = r_info & 0xFF`, `sym = r_info >> 8`. `st_shndx` символа берётся из `.symtab` по `sh_link`. Для `ABS32` и для PC-относительных типов из Global Constraints `st_shndx == SHN_ABS` даёт ошибку со словом `absolute`. Неизвестный тип даёт ошибку с `type`, смещением и подсказкой `build with -C target-feature=+no-movt`. `sdk/.cargo/config.toml`: `rustflags = ["-C", "link-arg=-Tapp.ld", "-C", "link-arg=--emit-relocs", "-C", "target-feature=+no-movt"]` в `[target.thumbv7em-none-eabi]`. Предупреждение rustc `unknown and unstable feature … no-movt` ожидаемо и не подавляется.

- [ ] **Step 4: Run to verify pass**

Run: `nix-shell -p llvm --run 'uv run --locked python -m unittest discover -s tests -v' && (cd sdk && cargo test --target aarch64-apple-darwin)`
Expected: PASS; `test_sdk` собирает `hello` и `vibration-lab`, в каждом есть релокации, все указывают внутрь образа.

- [ ] **Step 5: Commit**

```bash
git add src/wp2apps/package.py src/wp2apps/cli.py sdk/cmf-wp2-app/app.ld sdk/.cargo/config.toml tests/test_package.py tests/test_sdk.py
git commit -m "feat: relocatable packages linked at 0 with an ABS32 table"
```

---

### Task 3: Крейт `wp2-core` (общая логика, тесты на хосте)

**Files:**
- Create: `fw/Cargo.toml` (workspace: `wp2-core`, `wp2-stub`, `wp2-loader`; профиль release как в `sdk/Cargo.toml`)
- Create: `fw/.cargo/config.toml` (`[build] target = "thumbv7em-none-eabi"`; `[target.thumbv7em-none-eabi] rustflags = ["-C", "link-arg=-Tlink.ld", "-C", "link-arg=--emit-relocs", "-C", "target-feature=+no-movt"]` — под `[target…]`, чтобы хост-тесты не получали `-Tlink.ld`)
- Create: `fw/wp2-core/{Cargo.toml,src/lib.rs,src/layout.rs,src/pkg.rs,src/heap.rs,src/pwm.rs,src/gfx.rs,src/font8x8.rs,src/events.rs,src/stock.rs}`
- Modify: `AGENTS.md` (Verification: команда `cd fw && cargo test -p wp2-core --target aarch64-apple-darwin`; Layout: строка `fw/`)
- Modify: `.gitignore` (`fw/target/`)

**Interfaces:**
- Consumes: формат пакета (Global Constraints, задача 2), `cmf_wp2_app::{Api, Event}` — только как `dev-dependency` для проверки раскладки.
- Produces:

```rust
// layout.rs
pub const VIEW_ID: u16 = 0xF0;
pub const SLOTS: u8 = 8;
pub const LOADER_SLOT: u8 = 0xFF;
pub const LOADER_ABI_MINOR: u16 = 0;
pub const STUB_RAM: u32 = 0x3871_EEC0; pub const STUB_RAM_SIZE: u32 = 0x1000;
pub const LOADER_SIZE: u32 = 0x1_0000; pub const APP_SIZE: u32 = 0x2_0000;
pub const E_BAD: i32 = -2; pub const E_IO: i32 = -5; pub const E_NOMEM: i32 = -12;
pub fn slot_path(slot: u8, tmp: bool) -> [u8; 24];    // b"/NAND:C/wp2apps/L.pkg\0", b"/NAND:C/wp2apps/3.tmp\0"
pub const DIR: &[u8] = b"/NAND:C/wp2apps\0";
#[repr(C)] pub struct StubRam {
    pub magic: u32, pub loader_state: u8, pub loader_active: u8, pub running_slot: u8, pub xfer_state: u8,
    pub xfer_slot: u8, pub _pad: [u8; 3], pub xfer_size: u32, pub xfer_crc: u32, pub xfer_written: u32,
    pub file: [u8; 0x2C], pub trace_head: u32, pub trace: [u32; 32], pub scratch: [u8; 0x200], pub reply: [u8; 0x220],
    pub loader_block: u32, pub loader_size: u32, pub app_block: u32, pub app_size: u32, pub alloc_limit: u32,
}   // const assert: size_of <= 0xE00; offset_of!(trace_head) == 0x44, (loader_block) == 0x4E8, (app_block) == 0x4F0, (alloc_limit) == 0x4F8
// loader_state: 0 нет, 1 загружен, 2 нет файла/битый, 3 нет памяти
pub fn stub_ram() -> &'static mut StubRam;                // только target
// pkg.rs
pub const HEADER_SIZE: usize = 0x40;
pub struct Header { pub kind: u8, pub abi: u16, pub abi_minor: u8, pub name: [u8; 24], pub version: u32,
                    pub image_size: u32, pub entry: u32, pub reloc_count: u32, pub body_crc: u32 }
#[derive(Debug, PartialEq)] pub enum PkgError { Magic, HeaderCrc, Abi, Kind, Bounds, Name, Reloc }
pub fn parse_header(raw: &[u8; HEADER_SIZE]) -> Result<Header, PkgError>;
pub fn check_slot(h: &Header, slot: u8) -> Result<(), PkgError>;   // слот L ⇔ KIND_LOADER, image_size ≤ LOADER_SIZE, иначе KIND_APP ≤ APP_SIZE; entry < image_size; reloc_count ≤ image_size / 4; abi == 1; abi_minor ≤ LOADER_ABI_MINOR
pub fn file_size(h: &Header) -> u32;                               // 0x40 + image_size + 4·reloc_count
pub fn relocate(image: &mut [u8], base: u32, relocs: &[u8]) -> Result<(), PkgError>;  // relocs: LE u32, кусок таблицы; off + 4 > len → Reloc; слово += base (wrapping, без выравнивания)
pub fn crc32(init: u32, data: &[u8]) -> u32;                       // совместим с zlib, по частям
// heap.rs
pub const HEAP: u32 = 0x3804_3824; pub const SLACK: u32 = 0x100; pub const FB: u32 = 0; pub const RES: u32 = 2;
pub fn fits(free: u32, blocks: impl IntoIterator<Item = u32>, size: u32, align: u32) -> bool;  // need = size + align + SLACK (переполнение → false); free ≥ need и есть блок ≥ need
#[cfg(target_os = "none")] pub fn alloc(ty: u32, size: u32, align: u32) -> *mut u8;
#[cfg(target_os = "none")] pub unsafe fn free(ty: u32, p: *mut u8);
// pwm.rs
pub const STOCK_CLOCK: u32 = 0x303;
pub fn pwm_params(freq_hz: u16) -> Option<(u32, u16)>;              // (значение CMU_PWMCLK, period)
pub fn pulse(period: u16, duty_permille: u16) -> u16;               // period * duty / 1000
// gfx.rs
pub const W: usize = 466; pub const H: usize = 466;
pub struct Canvas<'a> { pub px: &'a mut [u16] }                     // RGB565, W*H
impl Canvas<'_> { pub fn fill_rect(&mut self, x: i16, y: i16, w: u16, h: u16, c: u16);
                  pub fn text(&mut self, x: i16, y: i16, s: &[u8], c: u16) -> i16; } // ячейка 16×16 (8×8 ×2), не-ASCII → '?'
// events.rs
#[repr(C)] #[derive(Clone, Copy, Default, PartialEq, Debug)] pub struct Event { pub kind: u8, pub _pad: u8, pub x: i16, pub y: i16, pub delta: i16 }
pub struct Queue { /* [Event; 16], AtomicU8 head/tail */ }
impl Queue { pub const fn new() -> Self; pub fn push(&self, e: Event) -> bool; pub fn pop(&self) -> Option<Event>; pub fn clear(&self); }
pub fn map_key(value: u32) -> Option<Event>;                        // 0xFFFF006F → CROWN +1, 0xFFFF006E → CROWN −1, 0x04000001 → BUTTON
#[repr(C)] pub struct Api { pub abi: u16, pub abi_minor: u16, pub width: u16, pub height: u16,
    pub fill_rect: extern "C" fn(i16, i16, u16, u16, u16), pub draw_text: extern "C" fn(i16, i16, *const u8, usize, u16),
    pub present: extern "C" fn(), pub next_event: extern "C" fn(*mut Event, u32) -> bool,
    pub motor: extern "C" fn(u16, u16) -> i32, pub millis: extern "C" fn() -> u32, pub exit: extern "C" fn(i32) -> ! }
// stock.rs — только `pub const NAME: u32 = 0x…;` из таблицы «Находки разведки» (без Thumb-бита у данных, с битом у функций), в том числе UI_MEM_ALIGNED_ALLOC = 0x101882C5, UI_MEM_FREE = 0x10188381
```

`heap::alloc` (target): поднять `BASEPRI` до `0x20` (как JHeap, сохранить старое значение). Если `alloc_limit != 0 && size > alloc_limit`, вернуть NULL — это внедрение отказа для e2e, по умолчанию 0. Затем обойти список от `*(HEAP + 0x60)` (не больше 4096 шагов; превышение — NULL) и вызвать `fits(*(HEAP + 8), …)`. Если не помещается — NULL, иначе `ui_mem_aligned_alloc(ty, align, size, b"wp2\0".as_ptr())`. В конце вернуть `BASEPRI`. `free(ty, p)` ничего не делает при `p == null`.

`pwm_params`: `None` вне `1..=20000`. Если `round(7812.5 / f) ≥ 100` (то есть `f ≤ 78`), результат `(0x303, round(7812.5 / f))`. Иначе `n = ceil(32_000_000 / (f · 65535)) − 1`, результат `(0x200 | n, 32_000_000 / ((n + 1) · f))`. Шрифт — `font8x8_basic` Daniel Hepper (public domain, источник и лицензия указаны в комментарии-заголовке `font8x8.rs`).

- [ ] **Step 1: Write the failing tests** (`#[cfg(test)]` в модулях)

```rust
#[test] fn golden_package_from_python() {   // GOLDEN — пакет из `wp2apps` (шаг 2), 0x40 + 16 + 8 байт
    let h = parse_header(GOLDEN[..0x40].try_into().unwrap()).unwrap();
    assert_eq!((h.kind, h.abi, h.abi_minor, h.version, h.image_size, h.reloc_count), (0, 1, 0, 7, 16, 2));
    assert_eq!(file_size(&h) as usize, GOLDEN.len());
    assert_eq!(h.body_crc, crc32(0, &GOLDEN[0x40..]));
}
#[test] fn relocate_adds_base() {
    let mut image = GOLDEN[0x40..0x50].to_vec();
    relocate(&mut image, 0x3810_0000, &GOLDEN[0x50..]).unwrap();
    assert_eq!(image, [0u32, 0x3810_0010, 1, 0x3810_0008].iter().flat_map(|w| w.to_le_bytes()).collect::<Vec<_>>());
    assert_eq!(relocate(&mut image, 0, &13u32.to_le_bytes()), Err(PkgError::Reloc));
}
#[test] fn header_crc_and_reserved_checked() {
    let mut raw: [u8; 0x40] = GOLDEN[..0x40].try_into().unwrap(); raw[0x28] ^= 1; assert_eq!(parse_header(&raw), Err(PkgError::HeaderCrc));
    let mut raw: [u8; 0x40] = GOLDEN[..0x40].try_into().unwrap(); raw[0x3A] = 1; assert!(parse_header(&raw).is_err());
}
#[test] fn slot_rules() {
    let h = parse_header(GOLDEN[..0x40].try_into().unwrap()).unwrap();
    assert!(check_slot(&h, 0).is_ok());
    assert_eq!(check_slot(&h, LOADER_SLOT), Err(PkgError::Kind));
    let mut newer = h; newer.abi_minor = 1; assert_eq!(check_slot(&newer, 0), Err(PkgError::Abi));
    let mut big = h; big.image_size = APP_SIZE + 1; assert_eq!(check_slot(&big, 0), Err(PkgError::Bounds));
    let mut many = h; many.reloc_count = 5; assert_eq!(check_slot(&many, 0), Err(PkgError::Bounds));
}
#[test] fn heap_fit() {
    assert!(fits(0x2_0000, [0x100, 0x1_0000 + 8 + SLACK], 0x1_0000, 8));
    assert!(!fits(0x2_0000, [0x1_0000 + 8 + SLACK - 1], 0x1_0000, 8));
    assert!(!fits(0x1_0000, [0x2_0000], 0x1_0000, 8));            // счётчик свободного меньше need
    assert!(!fits(u32::MAX, [u32::MAX], u32::MAX - 4, 8));          // переполнение need
}
#[test] fn stub_ram_offsets() {
    assert_eq!(offset_of!(StubRam, trace_head), 0x44); assert_eq!(offset_of!(StubRam, loader_block), 0x4E8);
    assert_eq!(offset_of!(StubRam, app_block), 0x4F0); assert_eq!(offset_of!(StubRam, alloc_limit), 0x4F8);
}
#[test] fn pwm_table() {
    assert_eq!(pwm_params(31), Some((0x303, 252)));
    assert_eq!(pwm_params(30), Some((0x303, 260)));
    assert_eq!(pwm_params(250), Some((0x201, 64000)));
    assert_eq!(pwm_params(1000), Some((0x200, 32000)));
    assert_eq!(pwm_params(16000), Some((0x200, 2000)));
    assert_eq!(pwm_params(0), None); assert_eq!(pwm_params(20001), None);
    assert_eq!(pulse(252, 500), 126); assert_eq!(pulse(255, 1000), 255);
}
#[test] fn fill_rect_clips() {
    let mut px = vec![0u16; W * H]; let mut c = Canvas { px: &mut px };
    c.fill_rect(-10, -10, 20, 20, 0xFFFF); c.fill_rect(460, 460, 50, 50, 0xFFFF);
    assert_eq!(px.iter().filter(|&&p| p == 0xFFFF).count(), 100 + 36);
}
#[test] fn text_stays_in_cells() {
    let mut px = vec![0u16; W * H]; let mut c = Canvas { px: &mut px };
    assert_eq!(c.text(100, 100, b"A\xD0", 0xFFFF), 132);
    assert!(px.iter().enumerate().all(|(i, &p)| p == 0 || (100..132).contains(&(i % W)) && (100..116).contains(&(i / W))));
}
#[test] fn queue_spsc() { /* 16 push → 16-й false (ёмкость 15), pop по порядку, clear опустошает */ }
#[test] fn key_mapping() {
    assert_eq!(map_key(0xFFFF_006F).map(|e| (e.kind, e.delta)), Some((3, 1)));
    assert_eq!(map_key(0xFFFF_006E).map(|e| (e.kind, e.delta)), Some((3, -1)));
    assert_eq!(map_key(0x0400_0001).map(|e| e.kind), Some(4));
    assert_eq!(map_key(0x2000_0001), None);
}
#[test] fn api_layout_matches_sdk() {
    assert_eq!(size_of::<Api>(), size_of::<cmf_wp2_app::Api>());
    assert_eq!(offset_of!(Api, motor), offset_of!(cmf_wp2_app::Api, motor));
    assert_eq!(size_of::<Event>(), size_of::<cmf_wp2_app::Event>());
}
```

- [ ] **Step 2: Сгенерировать `GOLDEN`**

Run: `uv run --locked python -c "import struct; from wp2apps import package; print(package.pack(struct.pack('<4I', 0, 16, 1, 8), name='golden', version=7, relocs=[4, 12]).hex())"`. Вставить вывод константой `GOLDEN: [u8; 88]` в `pkg.rs`.

- [ ] **Step 3: Run to verify failure**

Run: `cd fw && cargo test -p wp2-core --target aarch64-apple-darwin`
Expected: FAIL — не компилируется (`parse_header` не определён).

- [ ] **Step 4: Implement `wp2-core`** (`#![no_std]` вне тестов, без аллокаций)

- [ ] **Step 5: Run to verify pass**

Run: `cd fw && cargo test -p wp2-core --target aarch64-apple-darwin && cargo build -p wp2-core --release`
Expected: PASS, сборка под `thumbv7em-none-eabi` без ошибок (проверяет target-only `heap::alloc`).

- [ ] **Step 6: Commit**

```bash
git add fw/Cargo.toml fw/Cargo.lock fw/.cargo fw/wp2-core AGENTS.md .gitignore
git commit -m "feat(fw): wp2-core with package checks, relocation, heap fit, PWM params, canvas, event queue"
```

---

### Task 4: Заглушка: загрузка, view `0xF0`, загрузчик в блоке кучи

**Files:**
- Create: `fw/wp2-stub/{Cargo.toml,build.rs,link.ld,src/main.rs,src/view.rs}`
- Create: `fw/wp2-core/src/{fs.rs,load.rs}` (только target; ими пользуются и заглушка, и загрузчик)
- Create: `scripts/build-fw` (сборка заглушки, загрузчика, пропатченного образа)
- Modify: `src/wp2apps/firmware.py` (точки P2, P4; дописывание заглушки и таблицы view)
- Modify: `tests/e2e/harness.py` (`patched_nor`, `OPEN_VIEW`, `TRACE_OFF`, `LOADER_BLOCK_OFF`, `APP_BLOCK_OFF`, `ALLOC_LIMIT_OFF`)
- Test: `tests/test_firmware.py`, `tests/test_e2e.py` (класс `StubTest`)

**Interfaces:**
- Consumes: `wp2_core::{layout, pkg, heap, stock}`; `firmware.patch`, `harness.Emu`, `harness.heap` (задача 1).
- Produces:
  - Заголовок заглушки по `STUB_BASE` (секция `.header`, первая): `#[repr(C)] struct StubHeader { magic: [u8; 4] = *b"WP2S", version: u32 = 1, boot: u32, ble: u32, view_proc: u32, view_name: u32, _r: [u32; 2] }`; адреса функций с битом Thumb.
  - `wp2_boot` (naked): `push {r0-r3, r4, lr}` → `bl wp2_boot_c` (обнуляет `StubRam`, `magic = 0x57503253`, `running_slot = 0xFF`) → `pop {r0-r3, r4, lr}` → `b.w 0x10146AA4`. Стековые аргументы исходного вызова не трогает.
  - `wp2_core::fs`: `open(file: &mut [u8; 0x2C], path: &[u8], flags: u8) -> i32`, `read`, `write`, `close`, `unlink`, `rename`, `mkdir(path)` (через `0x10106FA0` и vtable `+0x3C`; `-134`, если указатель 0).
  - `wp2_core::load::load(slot: u8, file: &mut [u8; 0x2C], scratch: &mut [u8]) -> Result<(*mut u8, Header), i32>`: открыть `slot_path(slot)`, прочитать заголовок, `parse_header` + `check_slot`. Затем `heap::alloc(RES, image_size, 8)`; NULL → `E_NOMEM`. Образ читается прямо в блок с потоковым `crc32`. Таблица релокаций читается кусками по `scratch.len()` (кратно 4): CRC считается дальше, к каждому куску применяется `relocate(block, block as u32, chunk)`. Если CRC не сошёлся, есть ошибка релокации или ошибка чтения, блок освобождается и возвращается `E_BAD`/`E_IO`. Файл закрывается на всех путях.
  - `extern "C" fn wp2_view_proc(view_id: u32, msg: u32, data: *mut c_void) -> i32`. Пишет `(msg << 16) | view_id` в кольцо `trace`. На `msg == 2` (preload): если `loader_block != 0`, сначала освободить его. Затем `load(LOADER_SLOT, …, &mut scratch)`. Удача: `loader_block`, `loader_size`, `loader_state = 1`. `E_NOMEM` — `loader_state = 3`, остальное — `2`. При `loader_state != 1` на любое сообщение возвращается `-2`. Иначе `loader_active = 1` и вызов `(loader_block + entry | 1)(view_id, msg, data)`. Если на `msg == 2` загрузчик вернул отрицательное значение (не хватило буфера кадра), заглушка освобождает блок, ставит `loader_state = 3`, `loader_active = 0` и возвращает `-2`. На `msg == 4` после вызова: `heap::free(RES, loader_block)`, `loader_block = 0`, `loader_active = 0`, `loader_state = 0`.
  - Патчер: `firmware.patch(app, stub)` дописывает `stub` (≤ `STUB_MAX` − таблица) по `0x25A080`, выравнивает до 8 и кладёт следом таблицу view: 162 стоковые записи плюс `{view_name, view_proc, 0, 0, 0xF0, 0x0102, 0x1D2, 0x1D2}`. Литералы начала (`0x101461AC`, `0x1018BC24`, `0x1018DF3C`) меняются на адрес новой таблицы, литералы конца (`0x10055458`, `0x1018BC28`, `0x1018DF40`) — на её конец. **P2:** `0x101884FA` `bl 0x10146AA4` → `bl header.boot`.
  - Харнес (решение журнала: константы определяет эта задача): `patched_nor() -> bytes` = `stage_nor(stock_nor(), target/app-wp2.bin)`; `OPEN_VIEW = 0x101898BD`; `TRACE_OFF = 0x44`; `LOADER_BLOCK_OFF = 0x4E8`; `APP_BLOCK_OFF = 0x4F0`; `ALLOC_LIMIT_OFF = 0x4F8`.
  - `scripts/build-fw`: `cargo build --release -p wp2-stub -p wp2-loader` в `fw/`. Затем `llvm-objcopy -O binary` (у загрузчика с `--set-section-flags .data=alloc,load,contents`) и `wp2apps pack target/loader.bin --elf … --kind loader --name loader --version ${WP2_LOADER_VERSION:-1}`. В конце `wp2apps firmware --stub target/wp2-stub.bin --output target/app-wp2.bin`. Пишет `target/wp2-stub.bin`, `target/loader.wp2app`, `target/app-wp2.bin`.

`link.ld` заглушки: `MEMORY { FLASH (rx) : ORIGIN = 0x1025A080, LENGTH = 0x3000 }`, `.header` первой, `ASSERT(SIZEOF(.data) + SIZEOF(.bss) == 0, "stub must not have writable statics")`, `PROVIDE(stock_heap_init = 0x10146AA5)`. Заглушка не перемещается. Состояние хранится только в `StubRam` по `STUB_RAM`.

- [ ] **Step 1: Разведка перед кодом (только чтение).** Дизассемблировать функции, которые читают литералы `0x101461AC`, `0x10055458`, `0x1018DF3C/40`. Подтвердить, что значения используются как границы таблицы с шагом `0x18`, и что нигде нет числа записей константой (`0xA2`) или размера (`0xF30`). Проверить, что view-менеджер не индексирует массивы по `view_id` с границей `< 0xF1`: найти `cmp rN, #imm` с `imm ∈ [0xEE, 0xF0]` рядом с вызовами `FUN_1018bc04`. Результат — абзац в `docs/recon.md`. Если число записей зашито, добавить его подмену в P4 и тест на неё.

- [ ] **Step 2: Write the failing tests** (`tests/test_firmware.py`, на стоке)

```python
def test_stub_patch_points(self):
    stub = fake_stub(boot=0x1025A0A1, ble=0x1025A0C1, view_proc=0x1025A0E1, view_name=0x1025A100)  # b"WP2S"…
    out = firmware.patch(STOCK, stub)
    self.assertEqual(out[0x25A080:0x25A080 + len(stub)], stub)
    self.assertEqual(out[0x1884FA:0x1884FE], firmware.thumb_bl(0x101884FA, 0x1025A0A0))
    table = struct.unpack_from("<I", out, 0x1018BC24 - 0x10000000)[0]
    end = struct.unpack_from("<I", out, 0x1018BC28 - 0x10000000)[0]
    self.assertEqual((end - table) // 0x18, 163)
    last = out[end - 0x18 - 0x10000000:end - 0x10000000]
    self.assertEqual(struct.unpack_from("<IIIIHHHH", last), (0x1025A100, 0x1025A0E1, 0, 0, 0xF0, 0x102, 0x1D2, 0x1D2))
    for lit in (0x101461AC, 0x1018DF3C): self.assertEqual(struct.unpack_from("<I", out, lit - 0x10000000)[0], table)
    for lit in (0x10055458, 0x1018DF40): self.assertEqual(struct.unpack_from("<I", out, lit - 0x10000000)[0], end)

def test_stub_too_big_rejected(self):
    with self.assertRaises(ValueError):
        firmware.patch(STOCK, fake_stub() + bytes(0x4000))
```

- [ ] **Step 3: Run to verify failure**

Run: `uv run --locked python -m unittest tests.test_firmware -v`
Expected: FAIL — `patch()` не принимает заглушку.

- [ ] **Step 4: Implement the stub, `fs`, `load`, `link.ld`, patch points P2/P4, `scripts/build-fw`**

Загрузчика ещё нет. Для этой задачи `scripts/build-fw` собирает только заглушку и образ, если в `fw/wp2-loader` пусто.

- [ ] **Step 5: Run unit tests**

Run: `uv run --locked python -m unittest discover -s tests -v && (cd fw && cargo test -p wp2-core --target aarch64-apple-darwin)`
Expected: PASS.

- [ ] **Step 6: Write the e2e tests** (`StubTest`)

```python
def test_patched_boots_and_view_without_loader_fails_cleanly(self):
    with harness.Emu("stub", nor=harness.patched_nor()) as emu:
        emu.wait_uart("launcher enter", 60); emu.keep_awake()
        self.assertEqual(emu.peek(STUB_RAM, 4), struct.pack("<I", 0x57503253))
        before = emu.frame()
        emu.call(OPEN_VIEW, 0xF0, 0)
        time.sleep(3)
        trace = emu.peek(STUB_RAM + TRACE_OFF, 4 + 32 * 4)
        self.assertIn((2 << 16) | 0xF0, struct.unpack_from("<32I", trace, 4))
        self.assertEqual(emu.peek(STUB_RAM + 4, 1), b"\x02")               # loader_state = bad/missing
        self.assertEqual(harness.u32(emu, STUB_RAM + LOADER_BLOCK_OFF), 0)   # ничего не выделено
        self.assertNotIn("ZEPHYR FATAL ERROR", emu.uart())
        self.assertEqual(emu.frame(), before)                               # сток остался на своём экране

def test_mkdir_vtable_present(self):
    # gdb: найти mp для "/NAND:C" через call(0x10106FA1, &mp, path, 0), прочитать *(*(mp+0x1C)+0x3C) != 0
```

Если view-менеджер на `-2` в preload ведёт себя иначе (пустой экран, зависание), записать это в `docs/recon.md`. Тогда в заглушке на preload возвращать `0`, на layout рисовать пустой экран и закрывать view через `FUN_101898bc` предыдущего id. Это поведение закрепить тестом; оно же действует для `loader_state = 3`.

- [ ] **Step 7: Run e2e**

Run (в отдельной панели): `nix-shell -p llvm --run scripts/build-fw && WP2_E2E=1 nix-shell -p gdb llvm --run 'uv run --locked python -m unittest tests.test_e2e.StubTest tests.test_e2e.ReservationTest -v'`
Expected: PASS. `ReservationTest` повторяется на образе с заглушкой: `harness.patched_nor()` вместо `patch(stock)`.

- [ ] **Step 8: Commit**

```bash
git add fw/wp2-stub fw/wp2-core scripts/build-fw src/wp2apps/firmware.py tests/test_firmware.py tests/test_e2e.py tests/e2e docs/recon.md
git commit -m "feat(fw): stub with boot hook, view 0xF0 and the NAND loader in a heap block"
```

---

### Task 5: Свой BLE-канал установки (`wp2apps install/remove/list`)

**Files:**
- Create: `fw/wp2-core/src/proto.rs`; `fw/wp2-stub/src/ble.rs`
- Modify: `fw/wp2-stub/src/main.rs` (`wp2_ble_default`), `src/wp2apps/firmware.py` (P3)
- Create: `src/wp2apps/link.py` (кадр, CRC, AES, рукопожатие), `src/wp2apps/h4.py` (GATT-клиент по H4), `src/wp2apps/install.py`
- Modify: `src/wp2apps/cli.py`, `pyproject.toml` (`pycryptodome` прямой зависимостью), `uv.lock`
- Create: `tests/e2e/netsim.py` (запуск `netsimd`, синтетическое сопряжение)
- Test: `tests/test_link.py`, `tests/test_install.py`, `#[cfg(test)]` в `proto.rs`, `tests/test_e2e.py` (класс `InstallTest`)

**Interfaces:**
- Consumes: `StubRam`, `wp2_core::{fs, pkg}` (задачи 3–4).
- Produces — протокол: системные команды `c1 = 0xFFFF`. Payload зашифрован ключом сессии (CRC32 LE + PKCS#7, AES-CBC, IV `5051525354555657606162636465665A`). Ответ — `c2 | 0xA000` (`0x90A0` → `0xB0A0`), первый байт ответа — `Status`.

  | c2 | Запрос | Ответ |
  |---|---|---|
  | `0x90A0` INFO | `u8 slot` | `status`, затем 64 байта заголовка, если слот занят |
  | `0x90A1` BEGIN | `u8 slot, u32 size, u32 crc32(всего файла)` | `status` |
  | `0x90A2` DATA | `u32 offset, ≤ 128 B` | `status, u32 next_offset` |
  | `0x90A3` END | — | `status` |
  | `0x90A4` DELETE | `u8 slot` | `status` |

  `Status`: `Ok 0, BadRequest 1, Io 2, Sequence 3, Crc 4, Busy 5, Invalid 6`. `slot`: `0..=7` или `0xFF` (загрузчик). `Busy` — если слот совпадает с `running_slot` или это загрузчик при `loader_active`. `BEGIN` при незавершённой передаче отменяет старую (удаляет `.tmp`). `END` закрывает файл и перечитывает его: потоковый CRC всего файла, `parse_header`, `check_slot`, размер `= file_size(h)` (`0x40 + image_size + 4·reloc_count`), CRC `image ‖ relocs` = `body_crc`. Затем `unlink` старого и `rename(.tmp, .pkg)`; при ошибке `.tmp` удаляется. В кучу `END` ничего не грузит.
- `wp2_core::proto`: `pub enum Request<'a> { Info(u8), Begin { slot: u8, size: u32, crc: u32 }, Data { offset: u32, bytes: &'a [u8] }, End, Delete(u8) }`, `pub fn parse(c2: u16, payload: &[u8]) -> Result<Request<'_>, Status>`, `pub const MAX_DATA: usize = 128`.
- Заглушка (**P3**): `0x100D34F0` `bl 0x100CC284` → `bl header.ble`. `wp2_ble_default` (naked) делает `mov r2, r5; b.w wp2_ble_c`. Сигнатура `extern "C" fn wp2_ble_c(buf: *mut u8, len: u16, frame: *const u8) -> i32`: сначала вызывает стоковый `0x100CC285(buf, len)`; если `frame.c1 == 0xFFFF` и `c2 ∈ 0x90A0..=0x90A4` — расшифровывает (`0x101B608D`), обрабатывает и отвечает. Ответ: `0x101B605B(&n, plain, len, c2 | 0xA000)`, затем кадр в `StubRam.reply` (заголовок big-endian, как в ветке `0x9052`), затем `0x100CC2D5(reply, 11 + n)`. Возвращает результат стоковой функции.
- Python:
  - `link.py`: `frame(c1, c2, payload: bytes, key: bytes | None) -> bytes`; `class Parser` с `feed(data) -> list[tuple[int, int, bytes]]` (кадры от часов: открытый текст без CRC при заявленной длине с CRC, см. протокол); `encrypt(key, data)`, `decrypt(key, data)`; `class Session` с `async handshake(gatt) -> None` и `async request(c1, c2, payload, expect: int, timeout=6.0) -> bytes`.
  - Рукопожатие (из описания протокола, без чужого кода): `AT GETSECRET\r\n` в `77d4ff01-2fe2-2334-0d35-9ccd078f529c`, ответ `GETSECRET:<32 hex>,OK` в `…ff02`. Затем `0x8047` открытым текстом с `rnd1(16) + sha256(rnd1 + secret)` → `0x0048` с `rnd2(16) + sha256(rnd2 + secret)` (проверить). `authkey = sha256(rnd1 + rnd2 + secret)[:16]`. `0x8049` с `A5 + b"CMF Watch Pro 2"` (authkey) → `0x0049`. `0x804B` с `A5` (authkey) → `0x004C` nonce. `session = sha256(nonce[:16] + authkey)[:16]`. `0x804D` с `A5` → `0x0004`. `0x8004` с `>ii` (unix time, смещение в мс). Команды пишутся в `0000fff2-…`, ответы приходят в `0000fff1-…`.
  - `h4.py`: `async with gatt_over_h4(port: int, name: str = "cmf") as gatt:` — `write(uuid, data, response=True)`, `subscribe(uuid, callback)`. Реализация по спецификации Bluetooth Core: HCI Reset, Set Event Mask, LE Set Scan Parameters/Enable, поиск рекламы по имени, LE Create Connection; ATT Exchange MTU, Read By Type `0x2803`, Find Information для CCCD, Write Request/Command, Handle Value Notification.
  - `install.py`: `async info(s, slot) -> Header | None`, `async install(s, data: bytes, slot: int | None) -> int` (`None` — слот с тем же именем, иначе первый свободный; для `KIND_LOADER` всегда `0xFF`), `async remove(s, slot)`. Отдельно `chunks(data) -> Iterator[tuple[int, bytes]]` по `MAX_DATA`.
  - `Session.at(command: str) -> bytes` — AT-команда через shell-характеристику (нужна для `AT GETSECRET` и `AT REBOOT` в тестах).
  - CLI: `wp2apps install PKG [--slot N] --hci-port P`, `wp2apps remove --slot N|L --hci-port P`, `wp2apps list --hci-port P`. Вывод — JSON; `list` печатает `{"slots": {"<0..7|L>": {"name", "version", "kind", "abi_minor"}}}`, пустые слоты в него не попадают. Ошибки `Error: …`, код 1. Физический адрес (`--address`) в этом плане не поддерживается.
- `tests/e2e/netsim.py`: контекст `netsim() -> int` (порт) — `netsimd --instance 71 --hci-port <free> --no-cli-ui --no-web-ui`. Функция `paired_nor() -> bytes`: один раз запускает сток с `--hci-port` и `--qmp-port` и вызывает внешнюю `cmf-watch-utils emu pair-fixture --output runs/e2e/paired-nor.bin`. Харнесу добавляются:
  - `patched_paired_nor() = stage_nor(paired_nor(), target/app-wp2.bin)`;
  - `cli(args) -> int` (`wp2apps.cli.main`) и `cli_out(args) -> str` (то же со stdout);
  - `installed(names) -> ContextManager[tuple[Emu, int]]` — Netsim + `Emu` + `wait_uart("launcher enter")` + `keep_awake()` + `install target/<name>.wp2app` для каждого имени.

- [ ] **Step 1: Write the failing tests**

```rust
#[test] fn parse_requests() {
    assert_eq!(parse(0x90A1, &[3, 0x10, 0, 0, 0, 0xEF, 0xBE, 0xAD, 0xDE]), Ok(Request::Begin { slot: 3, size: 16, crc: 0xDEADBEEF }));
    assert_eq!(parse(0x90A1, &[8, 0, 0, 0, 0, 0, 0, 0, 0]), Err(Status::BadRequest));
    assert_eq!(parse(0x90A2, &[0; 4 + 129]), Err(Status::BadRequest));
    assert_eq!(parse(0x90A4, &[0xFF]), Ok(Request::Delete(0xFF)));
}
```

```python
class LinkTest(unittest.TestCase):
    def test_frame_layout(self):
        f = link.frame(0xFFFF, 0x90A0, b"\x01", key=None)
        self.assertEqual(f[:11], bytes.fromhex("f5") + struct.pack(">HHHHH", 5, 0xFFFF, 1, 1, 0x90A0))
        self.assertEqual(f[11:], b"\x01" + struct.pack("<I", zlib.crc32(b"\x01")))
    def test_encrypt_roundtrip(self):
        key = bytes(range(16)); body = link.encrypt(key, b"hello")
        self.assertEqual(len(body) % 16, 0); self.assertEqual(link.decrypt(key, body), b"hello")
    def test_parser_plaintext_without_crc(self):
        raw = bytes.fromhex("f5") + struct.pack(">HHHHH", 52, 0xFFFF, 0, 0, 0x0048) + bytes(48)
        self.assertEqual(link.Parser().feed(raw), [(0xFFFF, 0x0048, bytes(48))])
    def test_session_key(self):
        self.assertEqual(link.session_key(bytes(16), bytes(16)), hashlib.sha256(bytes(32)).digest()[:16])

class InstallChunksTest(unittest.TestCase):
    def test_chunks_cover_file(self):
        data = bytes(range(256)) * 3
        parts = list(install.chunks(data))
        self.assertEqual(b"".join(p for _, p in parts), data)
        self.assertTrue(all(len(p) <= 128 for _, p in parts))
        self.assertEqual([o for o, _ in parts], list(range(0, len(data), 128)))
```

- [ ] **Step 2: Run to verify failure**

Run: `uv run --locked python -m unittest tests.test_link tests.test_install -v && (cd fw && cargo test -p wp2-core --target aarch64-apple-darwin)`
Expected: FAIL — нет `link`, нет `proto`.

- [ ] **Step 3: Implement `proto.rs`, `ble.rs`, P3, `link.py`, `h4.py`, `install.py`, CLI**

При первом `BEGIN` заглушка создаёт каталог через `fs::mkdir(DIR)`, `-17` (EEXIST) не считается ошибкой. Если в задаче 4 указатель mkdir оказался нулевым, вместо каталога используются имена `/NAND:C/wp2apps-<slot>.pkg` в корне `/NAND:C`: `slot_path` меняется в одном месте и попадает в Rulings.

- [ ] **Step 4: Run unit tests**

Run: `uv run --locked python -m unittest discover -s tests -v && (cd fw && cargo test -p wp2-core --target aarch64-apple-darwin)`
Expected: PASS.

- [ ] **Step 5: Write the e2e tests** (`InstallTest`; Netsim + сопряжённый NOR)

```python
def test_install_list_remove(self):
    with netsim() as port, harness.Emu("install", nor=harness.patched_paired_nor(), hci_port=port) as emu:
        emu.wait_uart("launcher enter", 60); emu.keep_awake()
        self.assertEqual(cli(["install", "target/hello.wp2app", "--hci-port", str(port)]), 0)
        listed = json.loads(cli_out(["list", "--hci-port", str(port)]))
        self.assertEqual(listed["slots"]["0"]["name"], "hello")
        self.assertEqual(cli(["remove", "--slot", "0", "--hci-port", str(port)]), 0)
        self.assertNotIn("0", json.loads(cli_out(["list", "--hci-port", str(port)]))["slots"])

def test_install_rejects_corrupt(self):
    # пакет с испорченным байтом образа (CRC файла пересчитан хостом под испорченные байты): END → Status.Invalid, слот пуст
    # пакет с испорченным байтом таблицы релокаций (то же): END → Status.Invalid
    # пакет с abi_minor = 1 (заголовочный CRC пересчитан): END → Status.Invalid

def test_install_interrupted_keeps_old(self):
    # install hello v1; затем BEGIN v2 + половина DATA, разрыв связи (закрыть GATT);
    # list → слот 0 по-прежнему v1; новый BEGIN v2 + все DATA + END → v2
    # DATA с offset != next_offset → Status.Sequence, next_offset не меняется

def test_unauthenticated_frame_ignored(self):
    # кадр 0x90A0 зашифрован случайным ключом → ответа 0xB0A0 нет за 3 с, стоковые команды после этого работают
```

- [ ] **Step 6: Run e2e**

Run (в отдельной панели): `nix-shell -p llvm --run 'scripts/build-fw && scripts/build-app hello 1' && WP2_E2E=1 nix-shell -p gdb llvm --run 'uv run --locked python -m unittest tests.test_e2e.InstallTest -v'`
Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add fw/wp2-core/src/proto.rs fw/wp2-stub src/wp2apps/link.py src/wp2apps/h4.py src/wp2apps/install.py src/wp2apps/cli.py src/wp2apps/firmware.py pyproject.toml uv.lock tests/test_link.py tests/test_install.py tests/e2e tests/test_e2e.py
git commit -m "feat: own BLE install channel in the stub with wp2apps install/remove/list"
```

---

### Task 6: Загрузчик: view, буфер кадра, меню установленных приложений

**Files:**
- Create: `fw/wp2-loader/{Cargo.toml,build.rs,link.ld,src/main.rs,src/ui.rs,src/menu.rs}`
- Modify: `scripts/build-fw` (упаковка `loader.wp2app`)
- Modify: `tests/e2e/harness.py` (`hog`, `unhog`)
- Test: `tests/test_e2e.py` (класс `LoaderTest`)

**Interfaces:**
- Consumes: `wp2_core::{gfx, pkg, layout, heap, fs, stock, events}`, заглушка (задача 4), установка (задача 5).
- Produces:
  - `#[unsafe(no_mangle)] #[unsafe(link_section = ".entry")] extern "C" fn loader_main(view_id: u32, msg: u32, data: *mut c_void) -> i32` — первая функция образа (`entry = 0`). `link.ld`: `ORIGIN = 0, LENGTH = 0x10000`, `.entry`, `.text`, `.rodata`, `.data` + `.bss` внутри образа (как `sdk/cmf-wp2-app/app.ld`). Стоковые функции — только через константы `stock.rs`, приведённые к указателям (`PROVIDE`-символы запрещены, их отвергает упаковщик). Статики загрузчика живут в блоке и при каждом открытии view начинаются с нулей.
  - `ui.rs`: `fn preload() -> i32` (msg 2): `FB = heap::alloc(heap::FB, FB_BYTES, 0x40)`; NULL → `E_NOMEM` (заглушка тогда освобождает блок загрузчика). `fn layout(data: *mut c_void) -> i32` (msg 3): `scr = lv_disp_get_scr_act(*(data as *const *mut c_void))`, `img = lv_img_create(scr)`, `lv_img_set_src(img, &DSC)`, `DSC: lv_img_dsc_t` с `header = cf | (466 << 10) | (466 << 21)`, `cf = LV_IMG_CF_TRUE_COLOR (4)`, данные = `FB`. `lv_obj_add_flag(img, LV_OBJ_FLAG_CLICKABLE)` и `lv_obj_add_event_cb(img, touch_cb, 0, null)` — обработчик висит на своём объекте, не на экране. `TIMER = lv_timer_create(tick, 20, null)`. `fn delete()` (msg 4): `lv_timer_del(TIMER)`, `lv_obj_del(img)`, затем `heap::free(heap::FB, FB)`. После возврата блок загрузчика освобождается, поэтому в LVGL не должно остаться ни одного указателя в него: ни таймера, ни обработчика, ни `DSC`. Также `fn invalidate()` (`lv_obj_invalidate(img)`), `fn canvas() -> Canvas<'static>`.
  - `menu.rs`: `fn scan() -> [Option<Entry>; 8]` (`Entry { slot: u8, name: [u8; 24], len: u8 }`, заголовок из `n.pkg`, только прошедшие `parse_header` + `check_slot`), `fn draw(c: &mut Canvas, entries, selected: usize)` (заголовок `Apps` по `y = 60`, строки высотой 60 с `y = 120`, слева отступ `x = 90`; без приложений — `No apps`), `fn hit(y: i16) -> Option<usize>`.
  - Сообщения: 2 → `preload`; 3 → `layout` + `menu::draw`; 0xD → поворот коронки двигает выделение (событие помечается обработанным), остальное не трогается (стоковая кнопка выходит из view); `touch_cb` с `PRESSED`/`RELEASED` в меню при `hit` запоминает выбор (запуск — задача 7); 4 → `delete`.
  - `FB_BYTES = W * H * bytes_per_pixel` и `LV_COLOR_DEPTH` берутся из шага 1.
  - Харнес: `hog(emu, keep: int) -> list[int]` — пока `heap(emu)[1] > keep`, `p = emu.call(UI_MEM_ALIGNED_ALLOC, 2, 8, max_block − keep − 0x200, 0)`, не больше 64 раз; размер всегда меньше наибольшего блока, поэтому JHeap не падает. `unhog(emu, ptrs)` — `UI_MEM_FREE` для каждого.

- [ ] **Step 1: Разведка формата цвета и LVGL (только чтение).**
  - `LV_COLOR_DEPTH` и `LV_COLOR_16_SWAP`: декомпилировать `lv_canvas_fill_bg` (`0x1013E9AC`) и `lv_img_buf_get_img_size` (вызывается из `lv_canvas_set_buffer` `0x1013E52C`), найти множитель байт на пиксель для `TRUE_COLOR`. Если 32 бита, `Canvas` из задачи 3 хранит `u32` ARGB8888 с переводом из RGB565 на записи, `FB_BYTES = 0xD4110`; тесты задачи 3 правятся под это в этом же коммите.
  - Тип памяти для буфера кадра решён разведкой аллокатора: `FB` (0), выравнивание `0x40`. Буфер ровно `0x6A088` Б JHeap берёт из последнего подходящего блока; `heap::fits` это покрывает.
  - `lv_obj_add_flag(obj, flag)`: найти по вызовам с константой `2` (`LV_OBJ_FLAG_CLICKABLE`) рядом с `lv_obj_add_event_cb` в стоковых view. Записать адрес в `stock.rs` и `docs/recon.md`.
  - `lv_indev_get_point`: в вызывающих `0x10125BD4` найти следующий вызов вида `(indev, lv_point_t*)`. Проверить, что `0x10125BD4` действительно `lv_event_get_indev`.
- [ ] **Step 2: Write the e2e tests** (`LoaderTest`)

```python
def test_menu_lists_installed_apps(self):
    with netsim() as port, harness.Emu("loader", nor=harness.patched_paired_nor(), hci_port=port) as emu:
        emu.wait_uart("launcher enter", 60); emu.keep_awake()
        for pkg in ("target/loader.wp2app", "target/hello.wp2app", "target/vibration-lab.wp2app"):
            self.assertEqual(cli(["install", pkg, "--hci-port", str(port)]), 0)
        emu.call(OPEN_VIEW, 0xF0, 0); time.sleep(3)
        self.assertEqual(emu.peek(STUB_RAM + 4, 2), b"\x01\x01")          # loader loaded, active
        block = harness.u32(emu, STUB_RAM + LOADER_BLOCK_OFF)
        self.assertTrue(0x380A9340 <= block < STUB_RAM)                     # загрузчик живёт в ResFbHeap
        w, h, rgb = emu.frame()
        self.assertTrue(bright_pixels(rgb, w, box=(90, 120, 300, 40)) > 50)    # строка "hello"
        self.assertTrue(bright_pixels(rgb, w, box=(90, 180, 300, 40)) > 50)    # строка "vibration-lab"
        emu.send("c 1", "c 0", pause=2)                                     # кнопка → сток закрывает view
        self.assertEqual(emu.peek(STUB_RAM + 4, 2), b"\x00\x00")
        self.assertEqual(harness.u32(emu, STUB_RAM + LOADER_BLOCK_OFF), 0)

def test_loader_no_memory_keeps_stock(self):
    with harness.installed(["loader"]) as (emu, port):
        before = emu.frame()
        for limit in (0x100, 0x60000):                 # 1) нет блока под загрузчик; 2) загрузчик есть, буфера кадра нет
            emu.poke(STUB_RAM + ALLOC_LIMIT_OFF, struct.pack("<I", limit))
            emu.call(OPEN_VIEW, 0xF0, 0); time.sleep(3)
            self.assertEqual(emu.peek(STUB_RAM + 4, 2), b"\x03\x00")    # no memory, not active
            self.assertEqual(harness.u32(emu, STUB_RAM + LOADER_BLOCK_OFF), 0)
            self.assertEqual(emu.frame(), before)
        emu.poke(STUB_RAM + ALLOC_LIMIT_OFF, bytes(4))
        emu.call(OPEN_VIEW, 0xF0, 0); time.sleep(3)
        self.assertEqual(emu.peek(STUB_RAM + 4, 2), b"\x01\x01")
        uart = emu.uart()
        self.assertNotIn("MllcFailedHook", uart); self.assertEqual(uart.count("*** Booting Zephyr"), 1)
```

Лимит `0x60000` больше образа загрузчика (≤ `0x10000`) и меньше буфера кадра (≥ `0x6A088`), поэтому срабатывает второй путь: отказ в `preload` загрузчика. Если закрытый на preload view оставляет пустой экран (см. задачу 4, шаг 6), `emu.frame() == before` заменяется поведением, закреплённым там.

- [ ] **Step 3: Run to verify failure**

Run: `WP2_E2E=1 nix-shell -p gdb llvm --run 'uv run --locked python -m unittest tests.test_e2e.LoaderTest -v'`
Expected: FAIL — `target/loader.wp2app` нет.

- [ ] **Step 4: Implement the loader (view, canvas, menu)**

- [ ] **Step 5: Run to verify pass**

Run (в отдельной панели): `nix-shell -p llvm --run 'scripts/build-fw && scripts/build-app hello 1 && scripts/build-app vibration-lab 1' && WP2_E2E=1 nix-shell -p gdb llvm --run 'uv run --locked python -m unittest tests.test_e2e.LoaderTest -v'`
Expected: PASS. Кадр меню сохранить в `runs/e2e/loader-*/menu.ppm` для ревью.

- [ ] **Step 6: Commit**

```bash
git add fw/wp2-loader scripts/build-fw tests/test_e2e.py tests/e2e docs/recon.md
git commit -m "feat(fw): relocatable NAND loader with framebuffer view and installed-apps menu"
```

---

### Task 7: Запуск приложения и ABI 1 `Api` (экран, ввод, время, выход)

**Files:**
- Create: `fw/wp2-loader/src/app.rs`
- Modify: `fw/wp2-loader/src/{main.rs,ui.rs,menu.rs}`
- Modify: `tests/e2e/harness.py` (`padded(name, size, new_name) -> bytes`)
- Test: `tests/test_e2e.py` (класс `HelloTest`)

**Interfaces:**
- Consumes: `wp2_core::{events::{Queue, map_key, Event}, Api, pkg, layout, heap, load}`; `StubRam.{running_slot, app_block, app_size}`.
- Produces:
  - `fn launch(slot: u8) -> Result<(), i32>` (UI-поток): `load::load(slot, …)` даёт блок с перемещённым образом или `E_NOMEM`/`E_BAD`/`E_IO`. Затем `app_block = block`, `app_size = image_size`, `running_slot = slot`, очистить очередь и кадр. Вызывает `os_thread_create(&mut THREAD_BUF, THREAD_BUF.len(), app_thread, block + entry | 1, &API, 0, APP_PRIO, 0, 0)`; буфер `THREAD_BUF: [u8; 0x118 + 0x2000]` (выровнен на 8) лежит в `.bss` загрузчика, то есть в его блоке.
  - `extern "C" fn app_thread(entry: u32, api: *const Api, _: u32)`: `let code = entry(api); EXITED.store(code | FLAG)`; дальше поток засыпает в `loop { os_sleep(1000) }`, чтобы UI-поток мог его остановить.
  - `static API: Api = Api { abi: 1, abi_minor: 0, width: 466, height: 466, … }`. Функции:
    - `fill_rect`, `draw_text`: рисуют в `Canvas`, без LVGL; длина текста ограничена 64 байтами.
    - `present`: `DIRTY.store(true)`; перерисовку делает таймер `tick` через `lv_obj_invalidate`.
    - `next_event(ev, timeout_ms)`: `pop`, иначе `os_sleep(5)` до таймаута; `u32::MAX` — ждать бесконечно; возвращает только `0`/`1`.
    - `millis`: из шага 1.
    - `exit(code)`: `EXITED.store`, затем тот же сон.
  - `tick` (UI-поток, 20 мс): если `EXITED`, то `stop_app()` и меню. `stop_app()`: мотор выкл (задача 8), `k_thread_abort(&THREAD_BUF)`, `heap::free(RES, app_block)`, `app_block = 0`, `app_size = 0`, `running_slot = 0xFF`. Блок освобождается только после `abort`.
  - Ввод в режиме приложения: `touch_cb` → `TOUCH_DOWN`/`TOUCH_UP` с координатами `lv_indev_get_point`. Сообщение 0xD → `map_key`: на `Some` событие ставится в очередь и помечается обработанным (`*(data as *mut u8).add(4) = 1`).
  - Сообщение 4 (delete) с работающим приложением: `push(BUTTON)`, затем `stop_app()` сразу, без ожидания (UI-поток нельзя блокировать), затем `ui::delete()`.
  - Меню: тап по строке → `launch(slot)`. Ошибка → строка `Error <code>` (например `Error -12`) на 2 с в строке приложения.
  - Харнес: `padded(name, size, new_name)` — образ из `target/<name>.wp2app`, дополненный нулями до `size`, перепакованный `package.pack(…, relocs=pkg.relocs)` под именем `new_name`.

- [ ] **Step 1: Разведка времени, приоритета, abort (только чтение).**
  - `millis`: найти `k_uptime_get_32`/`k_uptime_ticks` — например, в функции, которая ставит метку времени в стоковый лог (`[8460][os]…`), или по syscall-обёртке рядом с `0x1019B268` (`k_sleep`). Записать адрес и единицы (мс или тики, `CONFIG_SYS_CLOCK_TICKS_PER_SEC`).
  - `k_thread_abort`: выбрать из `0x10105C90` и `0x1019B2D8` (сигнатура `(k_tid_t)`, вызов из `z_thread_abort`).
  - `APP_PRIO`: приоритет UI-потока (`lvglworkq` / `ui_service`) — по вызову `os_thread_create` или `k_work_queue_start` с этим именем. `APP_PRIO` = на 1 ниже (численно больше), чтобы зависшее приложение не останавливало UI.
  - Направление поворота: задача 3 приняла `0x…6F = +1`. Подтвердить по `view_key:` в UART при `r 80` и `r -80`; если наоборот — поправить `map_key` и тест.
- [ ] **Step 2: Write the e2e tests** (`HelloTest`)

```python
def test_hello_draws_reacts_and_exits(self):
    with installed(["loader", "hello"]) as (emu, port):
        emu.call(OPEN_VIEW, 0xF0, 0); time.sleep(2)
        emu.send("t 1 200 140", "t 0 200 140", pause=2)              # строка 0 → hello
        self.assertEqual(emu.peek(STUB_RAM + 6, 1), b"\x00")         # running_slot = 0
        self.assertNotEqual(harness.u32(emu, STUB_RAM + APP_BLOCK_OFF), 0)
        w, h, rgb = emu.frame()
        self.assertEqual(pixel(rgb, w, 233, 100), (0, 0, 0))          # чёрный фон
        self.assertTrue(bright_pixels(rgb, w, box=(213, 225, 60, 20)) > 20)  # "hello" у центра
        emu.send("r 80", pause=1.5)
        r, g, b = pixel(emu.frame()[2], w, 233, 100)
        self.assertTrue(r < 16 and g < 16 and b > 200)                # 0x001F → синий
        emu.send("c 1", "c 0", pause=2)                               # BUTTON → main возвращает 0
        self.assertEqual(emu.peek(STUB_RAM + 6, 1), b"\xff")
        self.assertEqual(harness.u32(emu, STUB_RAM + APP_BLOCK_OFF), 0)
        self.assertTrue(bright_pixels(emu.frame()[2], w, box=(90, 120, 300, 40)) > 50)  # снова меню

def test_view_delete_stops_app(self):
    # запустить hello; закрыть view свайпом вправо (t 1 30 233 … t 0 400 233) или call(OPEN_VIEW, 0x10, 0);
    # running_slot == 0xFF, loader_active == 0, app_block == 0, loader_block == 0, нет FATAL;
    # снова OPEN_VIEW 0xF0 → меню рисуется, hello запускается

def test_blocks_freed_and_relocated(self):
    # 5 циклов: OPEN_VIEW 0xF0 → тап hello → кадр hello (как выше) → "c 1","c 0" → кнопка ещё раз (view закрыт);
    #   после каждого цикла free = harness.heap(emu)[0], loader_block == app_block == 0
    # перед циклом 2 держать hog = emu.call(UI_MEM_ALIGNED_ALLOC, 2, 8, 0x1000, 0) (освободить после цикла 2):
    #   app_block и loader_block в цикле 2 отличаются от цикла 1, hello рисует так же
    # max(free) − min(free) < 0x1000 по циклам 1, 3, 4, 5

def test_app_no_memory_shows_error(self):
    # OPEN_VIEW; poke alloc_limit = 1; тап hello → running_slot 0xFF, app_block 0,
    #   bright_pixels в строке 0 (текст "Error -12"), через 2,5 с строка снова "hello"; poke alloc_limit = 0; тап → hello работает
    # нет MllcFailedHook, одна загрузка Zephyr

def test_big_app_starved_heap(self):
    # установить harness.padded("hello", 0x18000, "big"); OPEN_VIEW; hogs = harness.hog(emu, keep=0x10000)
    # тап по строке "big" → "Error -12", running_slot 0xFF; нет MllcFailedHook, одна загрузка Zephyr
    # harness.unhog(emu, hogs); тап → big работает (кадр hello), выход кнопкой
```

- [ ] **Step 3: Run to verify failure**

Run: `WP2_E2E=1 nix-shell -p gdb llvm --run 'uv run --locked python -m unittest tests.test_e2e.HelloTest -v'`
Expected: FAIL — тап по строке ничего не запускает.

- [ ] **Step 4: Implement `app.rs` and input wiring**

- [ ] **Step 5: Run to verify pass**

Run (в отдельной панели): `nix-shell -p llvm --run scripts/build-fw && WP2_E2E=1 nix-shell -p gdb llvm --run 'uv run --locked python -m unittest tests.test_e2e.HelloTest tests.test_e2e.LoaderTest -v'`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add fw/wp2-loader tests/test_e2e.py tests/e2e docs/recon.md
git commit -m "feat(fw): run relocated apps from heap blocks with ABI 1 display, input, time and exit"
```

---

### Task 8: Мотор: `Api.motor`, частота, сторож 2000 мс

**Files:**
- Create: `fw/wp2-loader/src/motor.rs`; `sdk/examples/motor-hog/{Cargo.toml,src/main.rs}`
- Modify: `fw/wp2-loader/src/{app.rs,ui.rs}`, `sdk/Cargo.toml` (member `examples/motor-hog`), `tests/e2e/harness.py` (`symbol`, `installed`-помощники `open_app`, `lab_play`)
- Test: `tests/test_e2e.py` (класс `MotorTest`)

**Interfaces:**
- Consumes: `wp2_core::pwm::{pwm_params, pulse, STOCK_CLOCK}`; PWM `dev = 0x101DF1C0`, `api = *(dev + 8)`; регистр `CMU_PWMCLK0 = 0x40001068`.
- Produces:
  - `extern "C" fn motor(duty_permille: u16, freq_hz: u16) -> i32` (поток приложения):
    - `duty > 1000` → `-1`.
    - `duty > 0 && pwm_params(freq) == None` → `-1`.
    - `PAUSED` → `-3`.
    - `duty == 0` → `off()`, `0`.
    - Иначе: записать `clk` в `CMU_PWMCLK0`, вызвать `api[0](dev, 0, period, pulse(period, duty), 1)`, `ON_SINCE.store(millis() | 1)`, вернуть `0`. Отрицательный код драйвера возвращается как есть.
  - `pub fn off()`: `api[4](dev, 0)`, `CMU_PWMCLK0 = 0x303`, `ON_SINCE.store(0)`. Вызывается из `stop_app`, паузы (задача 9) и сторожа.
  - Сторож в `tick`: `ON_SINCE != 0 && millis() − ON_SINCE ≥ 2000` → `off()`. Гонку с записью из потока приложения принимаем: обе стороны приводят мотор в определённое состояние.
  - `motor-hog`: `main` вызывает `api.vibrate(1000, 31)` и крутится в `loop { core::hint::spin_loop() }`, не вызывая `event`.
  - Харнес: `symbol(elf: Path, name: str) -> int` — значение символа из `llvm-nm`. Образы слинкованы с базой 0, поэтому это смещение в образе; адрес в эмуляторе = блок + смещение.

- [ ] **Step 1: Write the e2e tests** (`MotorTest`)

```python
def test_vibration_lab_drives_pwm(self):
    with installed(["loader", "vibration-lab"]) as (emu, port):
        open_app(emu, row=0)
        lab_play(emu)                                                  # тап по PLAY (y=330..390)
        on = [e for e in emu.motor() if e[1] > 0]
        self.assertTrue(on)
        ms, duty, hz = on[-1]
        self.assertAlmostEqual(duty, 200, delta=5)                     # 20 %: pulse = period·200/1000, лог округляет
        self.assertAlmostEqual(hz, LAB_DEFAULT_HZ, delta=1)            # STOCK_FREQUENCY из lab.rs после правки части 1
        emu.send("c 1", "c 0", pause=2)
        self.assertEqual(emu.motor()[-1][1], 0)
        self.assertEqual(struct.unpack("<I", emu.peek(0x40001068, 4))[0], 0x303)

def test_motor_cap_without_event_loop(self):
    with installed(["loader", "motor-hog"]) as (emu, port):
        open_app(emu, row=0); time.sleep(5)
        log = emu.motor()
        start = next(m for m, d, _ in log if d == 1000)
        stop = next(m for m, d, _ in log if m > start and d == 0)
        self.assertLessEqual(stop - start, 2020)
        self.assertEqual(struct.unpack("<I", emu.peek(0x40001068, 4))[0], 0x303)

def test_motor_rejects_out_of_range(self):
    # gdb: call(API.motor, 1001, 31) == -1; call(API.motor, 500, 0) == -1; call(API.motor, 500, 20001) == -1; motor.log без новых строк
    # указатель на motor: u32(loader_block + symbol(fw/target/thumbv7em-none-eabi/release/wp2-loader, "API") + 0x18)
```

`installed(…)`, `open_app(emu, row)`, `lab_play(emu)` — помощники в `tests/e2e/harness.py`. `open_app` открывает view `0xF0` (до задачи 10 через `call`) и тапает по строке `row`.

- [ ] **Step 2: Run to verify failure**

Run: `nix-shell -p llvm --run 'scripts/build-app vibration-lab 1 && scripts/build-app motor-hog 1' && WP2_E2E=1 nix-shell -p gdb llvm --run 'uv run --locked python -m unittest tests.test_e2e.MotorTest -v'`
Expected: FAIL — `motor` загрузчика пока заглушка `-1`, в `motor.log` пусто.

- [ ] **Step 3: Implement `motor.rs`, the watchdog, `motor-hog`**

- [ ] **Step 4: Run to verify pass**

Run (в отдельной панели): `nix-shell -p llvm --run scripts/build-fw && WP2_E2E=1 nix-shell -p gdb llvm --run 'uv run --locked python -m unittest tests.test_e2e.MotorTest -v'`
Expected: PASS. QEMU должен быть собран с исправленной расшифровкой делителя (`motor.log` для стока даёт ≈30–31 Гц). Если в `motor.log` 482 Гц, PR лога мотора ещё не исправлен: остановиться и сообщить владельцу.

- [ ] **Step 5: Commit**

```bash
git add fw/wp2-loader sdk/examples/motor-hog sdk/Cargo.toml sdk/Cargo.lock tests/test_e2e.py tests/e2e
git commit -m "feat(fw): motor API with PWM clock handling and 2 s continuous-run cap"
```

---

### Task 9: Пауза и продолжение под стоковым всплывающим окном

**Files:**
- Modify: `fw/wp2-loader/src/{ui.rs,app.rs,motor.rs}`, `sdk/Cargo.toml` (member `examples/motor-hold`)
- Create: `sdk/examples/motor-hold/{Cargo.toml,src/main.rs}`
- Modify: `tests/e2e/harness.py` (`read_marker`, `popup`, `close_popup`)
- Test: `tests/test_e2e.py` (класс `PauseTest`)

**Interfaces:**
- Consumes: сообщения view 7/8, флаги окна `*(u8*)0x3800FAF4 & 3`, `StubRam.app_block`.
- Produces: `fn overlay() -> bool = DEFOCUSED || (*(0x3800FAF4 as *const u8) & 3) != 0`. В `tick`:
  - переход `false → true`: `PAUSED = true`, `motor::off()`, очистить очередь, `push(PAUSE)`;
  - переход `true → false`: `PAUSED = false`, `push(RESUME)`, `invalidate()`.

  На паузе `motor` возвращает `-3`; рисовать в буфер можно, `present` ничего не делает до `RESUME`. Блок приложения на паузе остаётся выделенным.
- Харнес: `read_marker(emu) -> list[int]` (через `symbol` из задачи 8) — `u32(app_block)` + `symbol(sdk/target/thumbv7em-none-eabi/release/motor-hold, "JOURNAL")`, затем `peek` 20 байт `[count: u32, kinds: [u8; 16]]`.

- [ ] **Step 1: Разведка (эмулятор): что показывает окно.** На стоковом образе: открыть любое приложение из списка, отправить `p 4000 1 60`, через 5 с `p 4000 0 60`. Записать в `docs/recon.md`:
  - есть ли в `uart0.log` `msgbox_cache popup` и `set_popup_msgbox` и с каким `mid`;
  - получает ли текущий view `defocus`/`focus` (строки `----->view:`);
  - меняется ли байт `0x3800FAF4` (через `peek`).
  
  Если зарядка не открывает окно, взять низкий заряд `p 3400 0 4`. Если и он не работает — вызвать `call(0x100D5E35, <mid из таблицы msgbox>, 0)` для одного стокового id (оповещение будильника или низкий заряд). Таблицу id найти от `FUN_100d72a8(mid)` и записать. Выбранный триггер записать в харнес как `emu.popup()` / `emu.close_popup()`.
- [ ] **Step 2: Write the e2e test** (`PauseTest`)

```python
def test_popup_pauses_and_releases_motor(self):
    with installed(["loader", "motor-hold"]) as (emu, port):
        open_app(emu, row=0); time.sleep(0.5)
        self.assertGreater(emu.motor()[-1][1], 0)
        emu.popup(); time.sleep(1)
        self.assertEqual(emu.motor()[-1][1], 0)
        self.assertEqual(struct.unpack("<I", emu.peek(0x40001068, 4))[0], 0x303)
        self.assertEqual(emu.peek(STUB_RAM + 6, 1), b"\x00")          # приложение живо
        self.assertNotEqual(harness.u32(emu, STUB_RAM + APP_BLOCK_OFF), 0)
        emu.close_popup(); time.sleep(1.5)
        self.assertEqual(read_marker(emu), [PAUSE, RESUME])            # приложение записало, что получило
        self.assertTrue(bright_pixels(emu.frame()[2], 466, box=(150, 200, 166, 60)) > 20)  # кадр приложения вернулся

def test_lab_stops_measurement_on_pause(self):
    # vibration-lab: PLAY с Pattern=Long; popup → в motor.log сразу duty 0; после close_popup мотор не включается сам
```

`motor-hold` — пример `sdk/examples/motor-hold`. Он вызывает `vibrate(1000, 31)` и затем в цикле `event(1000)`. Каждый полученный `kind` он дописывает в `#[unsafe(no_mangle)] static mut JOURNAL: [u8; 20]` (`[count: u32, kinds: [u8; 16]]`, внутри своего образа). После `RESUME` мотор сам не включает. `read_marker` находит журнал как `app_block + symbol("JOURNAL")`.

- [ ] **Step 3: Run to verify failure**

Run: `WP2_E2E=1 nix-shell -p gdb llvm --run 'uv run --locked python -m unittest tests.test_e2e.PauseTest -v'`
Expected: FAIL — мотор не выключается при показе окна.

- [ ] **Step 4: Implement pause/resume**

- [ ] **Step 5: Run to verify pass**

Run (в отдельной панели): `nix-shell -p llvm --run 'scripts/build-fw && scripts/build-app motor-hold 1' && WP2_E2E=1 nix-shell -p gdb llvm --run 'uv run --locked python -m unittest tests.test_e2e.PauseTest tests.test_e2e.MotorTest -v'`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add fw/wp2-loader sdk/examples/motor-hold sdk/Cargo.toml sdk/Cargo.lock tests/test_e2e.py tests/e2e docs/recon.md
git commit -m "feat(fw): pause apps under stock popups and hand the motor back to stock"
```

---

### Task 10: Пункт «Apps» в стоковом списке приложений

**Files:**
- Modify: `src/wp2apps/firmware.py` (точка P5), `fw/wp2-stub/src/main.rs` (если нужен код), `docs/recon.md`
- Test: `tests/test_firmware.py`, `tests/test_e2e.py` (класс `AppListTest`), `tests/e2e/harness.py` (`open_apps` через UI)

**Interfaces:**
- Consumes: view `0xF0` (задача 4).
- Produces: в стоковом списке (все три вида: list, grid, waterfall, или хотя бы активный по умолчанию) появляется пункт, который открывает view `0xF0` через `FUN_101898bc(0xF0, 0)`. `harness.open_apps(emu)` теперь ходит через UI, а не через `call`.

- [ ] **Step 1: Разведка источника пунктов (только чтение, затем эмулятор).** От построителя `FUN_1005e9d8` и процедур `0x1005EEC9`/`0x1005EBC5`/`0x1005F101` найти:
  - массив id ресурсов иконок и подписей (вызов вида `FUN_10184a5c(scene, ids, out, n)`);
  - откуда берётся число пунктов (`data+8`);
  - соответствие «индекс → view_id» в обработчике клика `0x1005EBA1`;
  - размер структуры данных view (`thunk_FUN_10146a3c(size, …)`), то есть влезет ли ещё одна запись по 3 слова.
  
  Проверить в эмуляторе: тап по первому пункту даёт в UART `view:<id>` и совпадает с найденным соответствием. Записать в `docs/recon.md`.
- [ ] **Step 2: Точка решения.** Если массивы лежат во flash, а число и структура позволяют добавить запись, P5 переносит их копии в блоб заглушки (+1 запись: иконка — существующий id ресурса, подпись — `Apps`, если подписи строковые, или существующая строка) и переписывает литералы и константу числа. Сломать или заменить стоковый пункт нельзя (анти-цель спеки). **Если такого способа нет, остановиться и спросить владельца**, предложив варианты: пункт в списке настроек (`sysmenulist_view`), долгое нажатие коронки в списке приложений, отдельная иконка в ресурсах `/NAND:C`.
- [ ] **Step 3: Write the failing tests**

```python
def test_applist_patch(self):                 # tests/test_firmware.py, на стоке
    out = firmware.patch(STOCK, fake_stub())
    # число пунктов/литералы указывают на копию в блобе; копия = стоковый массив + 1 запись; старые записи не тронуты
```

```python
def test_apps_entry_opens_menu(self):          # AppListTest
    with installed(["loader", "hello"]) as (emu, port):
        harness.open_apps(emu)                 # коронка → список → прокрутка к последнему пункту → тап
        self.assertEqual(emu.peek(STUB_RAM + 4, 2), b"\x01\x01")
        for view in STOCK_SAMPLE:              # 3–4 стоковых пункта (часы, пульс, настройки) открываются как раньше
            self.assertIn(f"view:{view}", open_stock_item(emu, view))
```

- [ ] **Step 4: Run to verify failure**, **Step 5: Implement P5**, **Step 6: Run to verify pass**

Run (в отдельной панели): `uv run --locked python -m unittest tests.test_firmware -v && nix-shell -p llvm --run scripts/build-fw && WP2_E2E=1 nix-shell -p gdb llvm --run 'uv run --locked python -m unittest tests.test_e2e.AppListTest -v'`
Expected: PASS; кадр списка с новым пунктом сохранить в `runs/e2e/applist-*/list.ppm`.

- [ ] **Step 7: Commit**

```bash
git add src/wp2apps/firmware.py fw/wp2-stub tests docs/recon.md
git commit -m "feat: Apps entry in the stock app list"
```

---

### Task 11: Приёмка этапов 2–4 в эмуляторе

**Files:**
- Test: `tests/test_e2e.py` (класс `AcceptanceTest`)
- Modify: `README.md` (раздел «Firmware and loader in the emulator»: `scripts/build-fw`, `wp2apps install`, Netsim, e2e-команда)

**Interfaces:**
- Consumes: всё выше. Нового кода нет, если тесты проходят. Если что-то найдено, правка идёт в задачу, которой принадлежит код, и коммитится там же.

- [ ] **Step 1: Write the acceptance tests**

```python
def test_broken_loader_boots_stock(self):
    # установить loader; подменить его на NAND пакетом с правильным CRC файла, но испорченным CRC заголовка — через
    # заглушку это невозможно (END отвергает), поэтому порча идёт gdb-вызовом fs write по L.pkg (байт 0x28);
    # AT REBOOT по shell; после загрузки: "launcher enter", нет FATAL; Apps → view не открылся (loader_state == 2,
    # loader_block == 0), стоковые экраны (часы, список, настройки) открываются;
    # то же с испорченным последним байтом таблицы релокаций (CRC тела) → loader_state == 2;
    # `wp2apps install target/loader.wp2app` чинит, Apps работает

def test_install_survives_reboot(self):
    # install loader + hello; AT REBOOT; open_apps → "hello" в меню и запускается

def test_loader_update_over_ble(self):
    # install loader v1; install loader v2 (WP2_LOADER_VERSION=2 scripts/build-fw); list → slot L версия 2;
    # Apps открывается и грузит v2 из NAND (новый блок при каждом открытии); install loader при открытом Apps → Status.Busy

def test_vibration_lab_all_knobs(self):
    # Strength: 3 щелчка коронки вверх → duty 350‰ в motor.log при PLAY
    # Freq: выбор строки, щелчки → каждое значение из списка лаборатории даёт свою Hz в motor.log
    #   (частоты > 78 Гц идут через CMU_PWMCLK ≠ 0x303; после стопа снова 0x303)
    # Pattern Tick / Double / Long: длительности включений в motor.log = pulse_ms, pulse/80/pulse, 1000 мс (±20 мс)
    # Pulse: 5 мс и 100 мс на краях
    # кадр лаборатории сохранить в runs/e2e/acceptance-*/lab.ppm

def test_heavy_face_with_apps(self):
    # фикстуры heavy_face_fixtures(); install loader + vibration-lab; open_apps → лаборатория → выход → закрыть;
    # затем harness.walk(emu, 120): одна загрузка Zephyr, нет MllcFailedHook, loader_block == app_block == 0
```

- [ ] **Step 2: Run the whole suite**

Run (в отдельной панели): `uv run --locked python -m unittest discover -s tests -v && (cd sdk && cargo test --target aarch64-apple-darwin) && (cd fw && cargo test -p wp2-core --target aarch64-apple-darwin) && nix-shell -p llvm --run 'scripts/build-fw && scripts/build-app hello 1 && scripts/build-app vibration-lab 1 && scripts/build-app motor-hog 1 && scripts/build-app motor-hold 1' && WP2_E2E=1 nix-shell -p gdb llvm --run 'uv run --locked python -m unittest tests.test_e2e -v'`
Expected: всё PASS.

- [ ] **Step 3: Ручная проверка в окне (владелец или исполнитель).** Запустить `scripts/emu --nor runs/e2e/<run>/in-nor.bin --hci-port <netsim>`, установить загрузчик и лабораторию, пройти «Apps → vibration-lab». Убедиться, что строка `Motor: …` в окне viewer'а меняется вместе с ручками.

- [ ] **Step 4: Commit**

```bash
git add tests/test_e2e.py tests/e2e README.md
git commit -m "test: emulator acceptance for loader, install, pause, heap use and vibration lab"
```

---

### Task 12: Этап 5 — физические часы (только с явного разрешения владельца)

- [ ] **Получить явное «да» владельца на каждое действие этого пункта.** Без него ничего из списка ниже не выполнять.
  - До разрешения допустимы только чтения с Mac: `scan` и `info`. Цель — убедиться, что BLE на macOS работает. Это отдельная задача: нужен физический транспорт (`bleak`) в `wp2apps`.
  - Аппаратные риски, которые эмулятор не проверяет:
    - **Кеш.** Код исполняется из блока `ResFbHeap` в PSRAM (`0x380A9340…0x3871EEC0`, адрес каждый раз новый). После копирования и релокации нужна чистка D-cache и инвалидация I-cache. Функцию стока для этого ещё надо найти (кандидат на старт поиска — окрестности `lv_gpu_acts_flush_dcache` `0x10142C80`, Zephyr `sys_cache_*`). Загрузчик перемещает заглушка, а заглушка шьётся один раз, поэтому вызов кеша должен попасть в `wp2_core::load::load` **до** первой прошивки.
    - **MPU** с `PRIVDEFENA`: исполнение из `0x38…` разрешено картой по умолчанию; проверить, что сток не ставит регион XN на `ResFbHeap`.
    - **Реальный такт PWM:** замерить, что сток — около 30 Гц.
    - **Сторож Zephyr** при зависшем приложении.
    - **Нехватка памяти:** JHeap перезагружает часы так же, как в эмуляторе.
    - **Восстановление:** как вернуть часы, если пропатченный `app.bin` не загрузится, — процедура не доказана.
  - Пропатченный `app.bin` упаковывается в AOTA и проходит `cmf-watch-utils emu preflight`; стоковая OTA-последовательность (INIT2 в `ff01`, обязательный end-ack `0x9041 A5`) не меняется.

---

## После этого плана

- Владелец может вернуть кириллицу в подписи — потребуется шрифт с кириллицей вместо `font8x8_basic`.
- Минорные версии ABI добавляют функции (датчики, хранилище) в конец `Api` и поднимают `LOADER_ABI_MINOR`. Прошивку при этом менять не нужно.
- Если rustc сделает `target-feature=+no-movt` ошибкой, упаковщик и `relocate` получают парные релокации `R_ARM_THM_MOVW_ABS_NC`/`R_ARM_THM_MOVT_ABS`. Пара обрабатывается вместе: перенос из младшей половины нельзя применить к одной инструкции.
- Постоянные мягкие уровни вибрации в стоковом меню делаются отдельным дизайном после выбора параметров в лаборатории.
