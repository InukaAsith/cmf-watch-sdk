# Загрузчик приложений WP2, часть 2: заглушка, загрузчик, установка — план реализации

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Этапы 2–4 спеки. В стоковом `app.bin` 1.0.0.73 появляется маленькая заглушка. Загрузчик лежит на NAND и ставится по своему BLE-каналу вместе с приложениями. Через пункт «Apps» в эмуляторе запускаются `hello` и `vibration-lab`; стоковое всплывающее окно ставит приложение на паузу, битый загрузчик не мешает часам работать как сток.

**Architecture:** Патч `app.bin` состоит из подмены литералов и трёх инструкций `bl`, а сама заглушка дописывается в конец образа (XIP, `0x1025A080`). Перехватов при загрузке почти нет. Один `bl` в инициализации кучи обнуляет RAM заглушки, а загрузчик вызывается лениво: при открытии нового view `0xF0`. Заглушка регистрирует его в перенесённой таблице view, читает и проверяет `/NAND:C/wp2apps/L.pkg` и отдаёт ему все сообщения view. Если загрузчика нет или он битый, view не открывается, а часы остаются стоковыми. Свой BLE-канал (системные команды `0xFFFF:0x90A0…0x90A4`, расшифровка стоковым ключом сессии) живёт в заглушке, поэтому загрузчик всегда можно переустановить. Загрузчик рисует в свой RGB565-буфер, показанный стоковым `lv_img`, и обрабатывает сообщения view (layout/key/focus/defocus/delete). Приложение он запускает в отдельном потоке Zephyr и реализует ABI 1 `Api`.

**Tech Stack:** Rust `no_std` (`thumbv7em-none-eabi`): новый workspace `fw/` с крейтами `wp2-core` (логика, тестируется на хосте), `wp2-stub`, `wp2-loader`. Python 3.12 (uv, unittest, `pycryptodome`): патчер прошивки, BLE-клиент по H4 через Netsim, e2e-харнес эмулятора. Ghidra headless и capstone для разведки. QEMU с логом мотора из `../cmf-watch-utils-motor-log`, gdb из `nix-shell -p gdb`.

**Spec:** `docs/specs/2026-11-10-wp2-app-loader-design.md` (часть 1: `docs/plans/2026-11-10-wp2-app-loader-foundations.md`, разведка: `docs/recon.md`)

## Global Constraints

- Все правила `AGENTS.md`. Записывать что-либо на физические часы в этом плане запрещено. Последний пункт (этап 5) выполняется только после явного «да» владельца.
- В коммиты не попадают `app.bin`, пропатченный образ, NOR/NAND, `*.wp2app`, `*.pkg`, `runs/`, `target/`, `fw/target/`, секрет синтетического сопряжения. MAC в коде и тестах — только `AA:BB:CC:DD:EE:FF`.
- Код из `cmf-watch-utils` не копируется и не импортируется в `src/`. Его CLI (`emu run`, `emu pair-fixture`, `emu faces`) вызывается как внешняя программа, и только из e2e-харнеса. BLE-клиент и рукопожатие пишутся заново по описанию протокола из этого плана.
- Хост-инструменты берутся из `nix-shell -p …`; `netsimd` — из Android SDK (`$WP2_NETSIMD`, по умолчанию `~/Library/Android/sdk/emulator/netsimd`).
- Все адреса действительны только для JX402 `1.0.0.73`, sha256 `app.bin` начинается с `30e7978c39d8fb79`. Патчер сверяет старые байты в каждой точке патча и при расхождении отказывает.
- **Карта памяти (решение этого плана):**

  | Что | Адрес | Размер |
  |---|---|---|
  | заглушка (XIP, только чтение) | `STUB_BASE = 0x1025A080` (смещение в `app.bin` `0x25A080`) | `STUB_MAX = 0x4000` вместе с таблицей view |
  | `ResFbHeap` | `0x380A9340` | `0x676B80` → **`0x645CC0`**, конец `0x386EF000` |
  | загрузчик | `LOADER_BASE = 0x386EF000` | `LOADER_SIZE = 0x10000` |
  | приложение | `APP_BASE = 0x386FF000` | `APP_SIZE = 0x20000` |
  | RAM заглушки | `STUB_RAM = 0x3871F000` | `STUB_RAM_SIZE = 0xEC0` (до `0x3871FEC0`, где кончалась стоковая куча) |

  Отступление от постановки: вместо `0x655CC0` куча уменьшается до `0x645CC0`, на 64 KiB больше. Загрузчику нужен свой фиксированный адрес, а PIC/ROPI ради этого дороже. Всего отнимается 196 KiB, около 3 % `ResFbHeap`. Проверка — задача 1.
- **PWM мотора (поправка к части 1):** такт канала задаёт `CMU_PWMCLK` `0x40001068 + 4·ch`. Поле делителя `0..255` означает `/(n+1)`, значения `256..260` — `/512 … /8192`, биты 9–10 выбирают источник (`0` — RC 16K/32K, `1` — HOSC 32 MHz, `2` — 64M). Сток пишет `0x303` (HOSC/4096 = 7812,5 Гц) и period `0xFF`, отсюда **≈30,6 Гц**. Загрузчик восстанавливает `0x303` на канале 0 при выключении мотора, паузе и выходе.
- Защита мотора: непрерывная работа не дольше **2000 мс** (сторож с шагом 20 мс, допуск ≤ 2020 мс), скважность в промилле `0..=1000`, частота `1..=20000` Гц, вне диапазона `motor` возвращает `-1` и регистры не трогает. Пока приложение на паузе, `motor` возвращает `-3` и ничего не делает.
- **Пакет (поправка к части 1):** байт `0x07` — `abi_minor`, `u32` по `0x34` — `crc32(header[0:0x34])`, байты `0x38..0x40` нулевые. `Api`: `abi: u16` (мажорная версия, сравнение `==`), `abi_minor: u16` (у загрузчика `>=`, чем у приложения), `width: u16`, `height: u16`, затем указатели функций; новые функции только дописываются в конец. Загрузчик этого плана — ABI `1.0`.
- Тесты: `uv run --locked python -m unittest discover -s tests -v` (e2e пропускаются без `WP2_E2E=1`); `(cd sdk && cargo test --target aarch64-apple-darwin)`; `(cd fw && cargo test -p wp2-core --target aarch64-apple-darwin)`. E2E: `WP2_E2E=1 nix-shell -p gdb llvm --run 'uv run --locked python -m unittest tests.test_e2e -v'`. Один прогон e2e занимает минуты.

## Review Focus

1. Связь пропала посреди установки, или пришёл `DATA` с чужим смещением. Ожидается, что активная версия слота не меняется, `.tmp` не становится пакетом, следующий `BEGIN` начинает заново. Тест — задача 4 (`test_install_interrupted_keeps_old`).
2. Приложение включило мотор и зависло в цикле без `next_event`. Ожидается, что мотор всё равно выключится не позже 2020 мс, потому что сторож работает в UI-потоке. Тест — задача 7 (`test_motor_cap_without_event_loop`, пример `motor-hog`).
3. Стоковое окно появилось, когда мотор включён. Ожидается, что до показа окна мотор выключен, в `CMU_PWMCLK0` снова `0x303`, а `motor` на паузе возвращает `-3`. Тест — задача 8 (`test_popup_pauses_and_releases_motor`).
4. На NAND лежит загрузчик или приложение с чужим ABI, `abi_minor` больше, чем у загрузчика, испорченным заголовком или образом. Ожидается отказ при установке; если файл всё же оказался на NAND — отказ при загрузке, без прыжка в образ. Тесты — задача 2 (`pkg`), задача 4 (`test_install_rejects_corrupt`), задача 10 (`test_broken_loader_boots_stock`).
5. Свайп назад или закрытие view во время работы приложения. Ожидается, что поток приложения остановлен, мотор выключен, буфер и таймер освобождены, а следующее открытие «Apps» работает. Тест — задача 6 (`test_view_delete_stops_app`).

---

## Находки разведки для этого плана

Источники: `scripts/recon/decomp` (Ghidra), capstone, `runs/window-174132/uart0.log`. Уверенность обозначена так же, как в `docs/recon.md`. В `docs/recon.md` эти находки **не** переносятся, пока владелец правит часть 1. Это делает шаг 1 задачи 1.

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

---

### Task 1: Патчер прошивки, e2e-харнес и проверка резервирования

**Files:**
- Create: `src/wp2apps/firmware.py`
- Modify: `src/wp2apps/cli.py` (подкоманды `firmware`, `nor`), `src/wp2apps/paths.py` (`stock_app()`), `src/wp2apps/package.py` (`LOADER_BASE`, `LOADER_SIZE`, границы для `KIND_LOADER`), `docs/recon.md` (раздел «Plan 2 findings» из таблицы выше, плюс три литерала кучи)
- Create: `tests/e2e/__init__.py`, `tests/e2e/harness.py`
- Test: `tests/test_firmware.py`, `tests/test_e2e.py`

**Interfaces:**
- Produces (`firmware.py`):
  - `STUB_OFFSET = 0x25A080`, `HEAP_LITERALS = (0x188508, 0x188588, 0x1885F4)`, `HEAP_OLD = 0x676B80`, `HEAP_NEW = 0x645CC0`
  - `sum32(data: bytes) -> int`, `fix_acth(app: bytearray) -> None` — размер = `len(app) − 0x200`, затем `+0x1C`, затем `+0x20`; `len(app) % 4 == 0` обязательно
  - `thumb_bl(at: int, target: int) -> bytes`, `thumb_b_w(at: int, target: int) -> bytes` — адреса виртуальные (`0x1…`)
  - `put(app: bytearray, addr: int, old: bytes, new: bytes) -> None` — если по адресу не `old`, `ValueError(f"unexpected bytes at {addr:#x}")`
  - `patch(app: bytes, stub: bytes | None = None) -> bytes` — проверяет sha256 стока; без `stub` меняет только три литерала и пересчитывает ACTH (задачи 3 и 9 добавляют свои точки)
  - `stage_nor(nor: bytes, app: bytes) -> bytes` — `app` в `0x24000`, раздел `0x34B000` дополняется `0xFF`
  - CLI: `wp2apps firmware --stub STUB --output OUT` (сток берётся через `paths.stock_app()`), `wp2apps nor NOR --app APP --output OUT`
- Produces (`paths.py`): `stock_app() -> bytes` — `app.bin` из `paths.firmware()` через `fwkit` (как в `scripts/recon/extract.py`), с проверкой sha256.
- Produces (`tests/e2e/harness.py`, без кода `cmf-watch-utils`):
  - `E2E = os.environ.get("WP2_E2E") == "1"`; `QEMU = $WP2_QEMU` или `../cmf-watch-utils-motor-log/runtime-motor/qemu-system-arm`
  - `class Emu` (контекст-менеджер): `Emu(name: str, *, nor: bytes, fixtures: Path | None = None, hci_port: int | None = None, seconds: int = 900)`. Запускает `uv run --locked cmf-watch-utils emu run --headless --allow-reboot --nor <run>/in-nor.bin --qmp-port <free> [--hci-port] --seconds … --output runs/e2e/<name>-<ts>`. Методы: `send(*lines, pause=0.3)`, `gdb(*commands) -> str`, `peek(addr, size) -> bytes` (QMP `pmemsave`), `poke(addr, data)`, `call(fn, *args) -> int` (ручной кадр gdb, спин-заглушка `0xE7FE` по `STUB_RAM + 0xE00`), `keep_awake()`, `frame() -> tuple[int, int, bytes]` (`frame.ppm`, RGB888), `uart() -> str`, `wait_uart(needle, timeout)`, `motor() -> list[tuple[int, int, int]]`
  - `stock_nor() -> bytes` (из `<cmf>/fixtures/nor.bin`), `heavy_face_fixtures() -> Path` (см. шаг 6)

- [ ] **Step 1: Перенести находки в `docs/recon.md`.** Делать после того, как владелец закончит правку части 1. Добавить в конец раздел «Plan 2 findings» с таблицей «Находки разведки» из этого плана. Отдельной строкой — три литерала `ResFbHeap` и что пропатчены все три.

- [ ] **Step 2: Write the failing tests** (`tests/test_firmware.py`)

```python
class FirmwareTest(unittest.TestCase):
    def test_thumb_bl_matches_stock_encoding(self):
        self.assertEqual(firmware.thumb_bl(0x100D34F0, 0x100CC284), bytes.fromhex("f8f7c8fe"))
        self.assertEqual(firmware.thumb_bl(0x101884FA, 0x10146AA4), STOCK[0x1884FA:0x1884FE])  # skip без стока

    def test_fix_acth_rule(self):
        app = bytearray(b"ACTHHTCA" + bytes(0x1F8) + os.urandom(4096))
        firmware.fix_acth(app)
        size, checksum = struct.unpack_from("<II", app, 0x18)
        self.assertEqual(size, len(app) - 0x200)
        self.assertEqual(firmware.sum32(app[:0x200]), 0xFFFFFFFF)
        self.assertEqual((firmware.sum32(app[0x200:0x200 + size]) + checksum) & 0xFFFFFFFF, 0xFFFFFFFF)

    @skipUnless(HAVE_STOCK, "needs JX402_V1.0.0.73.bin")
    def test_heap_literals_patched(self):
        out = firmware.patch(STOCK)
        for off in firmware.HEAP_LITERALS:
            self.assertEqual(struct.unpack_from("<I", out, off)[0], 0x645CC0)
        diff = [i for i in range(0x200, len(STOCK)) if out[i] != STOCK[i]]
        self.assertEqual({i & ~3 for i in diff}, set(firmware.HEAP_LITERALS))

    @skipUnless(HAVE_STOCK, "needs JX402_V1.0.0.73.bin")
    def test_patch_refuses_other_bytes(self):
        bad = bytearray(STOCK); bad[0x188508] ^= 1
        with self.assertRaisesRegex(ValueError, "0x10188508"):
            firmware.patch(bytes(bad))   # проверка sha256 отключается внутренним параметром только в тесте

    def test_stage_nor(self):
        nor = firmware.stage_nor(b"\0" * 0x400000, b"\1" * 16)
        self.assertEqual(nor[0x24000:0x24010], b"\1" * 16)
        self.assertEqual(nor[0x24010:0x24000 + 0x34B000], b"\xff" * (0x34B000 - 16))

    def test_loader_package_bounds(self):
        package.pack(b"\0" * 16, name="loader", version=1, kind=package.KIND_LOADER, load_addr=package.LOADER_BASE)
        with self.assertRaises(ValueError):
            package.pack(b"\0" * 16, name="loader", version=1, kind=package.KIND_LOADER, load_addr=package.APP_BASE)
        with self.assertRaises(ValueError):
            package.pack(b"\0" * (package.LOADER_SIZE + 1), name="loader", version=1,
                         kind=package.KIND_LOADER, load_addr=package.LOADER_BASE)
```

- [ ] **Step 3: Run to verify failure**

Run: `uv run --locked python -m unittest tests.test_firmware -v`
Expected: FAIL — `ImportError: cannot import name 'firmware'`.

- [ ] **Step 4: Implement `firmware.py`, `paths.stock_app`, loader bounds in `package.py`, CLI**

`thumb_bl`: `off = target − (at + 4)`, стандартная кодировка T1 BL (`S, J1, J2, imm10, imm11`), с проверкой диапазона ±16 MiB. `patch` вызывает `put` для каждой точки и затем `fix_acth`; длина дополняется нулями до кратной 4.

- [ ] **Step 5: Run to verify pass**

Run: `uv run --locked python -m unittest discover -s tests -v`
Expected: PASS (тесты на стоке пропускаются только без прошивки).

- [ ] **Step 6: Харнес и фикстура с тяжёлым циферблатом**

`heavy_face_fixtures()` запускает `cmf-watch-utils emu faces --sort downloads` и берёт первый id. Затем стоковым профилем: `emu run --headless --fmc-face <id> --seconds 20 --output runs/e2e/face-<id>`. Строит каталог `runs/e2e/fixtures-heavy/` со ссылками на `nor.bin` и `nand-id.bin` из `<cmf>/fixtures`, а также `nand-defaults.bin → runs/e2e/face-<id>/nand.bin` и `wfc-index.txt → runs/e2e/face-<id>/wfc-index.txt`. Результат кешируется, id и размер циферблата записываются в `runs/e2e/fixtures-heavy/face.json`. В тесте ниже `Emu` получает `fixtures=heavy_face_fixtures()`.

- [ ] **Step 7: Write the e2e test** (`tests/test_e2e.py`, класс `ReservationTest`, `@skipUnless(harness.E2E, …)`)

```python
def test_reservation_survives_ui_walk(self):
    nor = firmware.stage_nor(harness.stock_nor(), firmware.patch(paths.stock_app()))
    with harness.Emu("reserve", nor=nor, fixtures=harness.heavy_face_fixtures()) as emu:
        emu.wait_uart("launcher enter", 60)
        emu.keep_awake()
        emu.poke(0x386EF000, MARK * ((0x3871FEC0 - 0x386EF000) // 4))
        harness.walk(emu, seconds=240)        # маршрут psram_probe.walk: коронка, повороты, тапы, свайпы, смена циферблатов
        self.assertEqual(emu.peek(0x386EF000, 0x3871FEC0 - 0x386EF000),
                         MARK * ((0x3871FEC0 - 0x386EF000) // 4))
        self.assertNotIn("ZEPHYR FATAL ERROR", emu.uart())
```

`harness.walk(emu, seconds)` повторяет последовательность ввода `scripts/recon/psram_probe.py:walk`. Её надо переписать в харнесе, а не импортировать. Добавить долгое нажатие на циферблат (`t 1 233 233`, пауза 1,5 с, `t 0 233 233`) и прокрутку карусели циферблатов, чтобы тяжёлый циферблат грузился несколько раз.

- [ ] **Step 8: Run e2e**

Run: `WP2_E2E=1 nix-shell -p gdb llvm --run 'uv run --locked python -m unittest tests.test_e2e.ReservationTest -v'`
Expected: PASS — маркер в `[0x386EF000, 0x3871FEC0)` цел, фатальных ошибок нет. Если тест падает, план останавливается: значит, резервирование неверно.

- [ ] **Step 9: Commit**

```bash
git add src/wp2apps/firmware.py src/wp2apps/cli.py src/wp2apps/paths.py src/wp2apps/package.py tests/test_firmware.py tests/e2e tests/test_e2e.py docs/recon.md
git commit -m "feat: firmware patcher with ResFbHeap reservation and e2e harness"
```

---

### Task 2: Крейт `wp2-core` (общая логика, тесты на хосте)

**Files:**
- Create: `fw/Cargo.toml` (workspace: `wp2-core`, `wp2-stub`, `wp2-loader`; профиль release как в `sdk/Cargo.toml`)
- Create: `fw/.cargo/config.toml` (`[build] target = "thumbv7em-none-eabi"`, `rustflags = ["-C", "link-arg=-Tlink.ld"]`)
- Create: `fw/wp2-core/{Cargo.toml,src/lib.rs,src/layout.rs,src/pkg.rs,src/pwm.rs,src/gfx.rs,src/font8x8.rs,src/events.rs,src/stock.rs}`
- Modify: `AGENTS.md` (Verification: команда `cd fw && cargo test -p wp2-core --target aarch64-apple-darwin`; Layout: строка `fw/`)
- Modify: `.gitignore` (`fw/target/`)

**Interfaces:**
- Consumes: формат пакета (Global Constraints), `cmf_wp2_app::{Api, Event}` — только как `dev-dependency` для проверки раскладки.
- Produces:

```rust
// layout.rs — константы из Global Constraints и:
pub const VIEW_ID: u16 = 0xF0;
pub const SLOTS: u8 = 8;
pub const LOADER_SLOT: u8 = 0xFF;
pub const LOADER_ABI_MINOR: u16 = 0;
pub fn slot_path(slot: u8, tmp: bool) -> [u8; 24];    // b"/NAND:C/wp2apps/L.pkg\0", b"/NAND:C/wp2apps/3.tmp\0"
pub const DIR: &[u8] = b"/NAND:C/wp2apps\0";
#[repr(C)] pub struct StubRam {
    pub magic: u32, pub loader_state: u8, pub loader_active: u8, pub running_slot: u8, pub xfer_state: u8,
    pub xfer_slot: u8, pub _pad: [u8; 3], pub xfer_size: u32, pub xfer_crc: u32, pub xfer_written: u32,
    pub file: [u8; 0x2C], pub trace_head: u32, pub trace: [u32; 32], pub scratch: [u8; 0x200], pub reply: [u8; 0x220],
}                                                      // const assert: size_of::<StubRam>() <= 0xE00
// pkg.rs
pub const HEADER_SIZE: usize = 0x40;
pub struct Header { pub kind: u8, pub abi: u16, pub abi_minor: u8, pub name: [u8; 24], pub version: u32,
                    pub image_size: u32, pub entry: u32, pub load_addr: u32, pub image_crc: u32 }
#[derive(Debug, PartialEq)] pub enum PkgError { Magic, HeaderCrc, Abi, Kind, Bounds, Name }
pub fn parse_header(raw: &[u8; HEADER_SIZE]) -> Result<Header, PkgError>;
pub fn check_slot(h: &Header, slot: u8) -> Result<(), PkgError>;   // слот L ⇔ KIND_LOADER@LOADER_BASE≤LOADER_SIZE, иначе KIND_APP@APP_BASE≤APP_SIZE; entry < image_size; abi == 1; abi_minor <= LOADER_ABI_MINOR
pub fn crc32(init: u32, data: &[u8]) -> u32;                         // совместим с zlib, по частям
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
// stock.rs — только `pub const NAME: u32 = 0x…;` из таблицы «Находки разведки» (без Thumb-бита у данных, с битом у функций)
```

`pwm_params`: `None` вне `1..=20000`. Если `round(7812.5 / f) ≥ 100` (то есть `f ≤ 78`), результат `(0x303, round(7812.5 / f))`. Иначе `n = ceil(32_000_000 / (f · 65535)) − 1`, результат `(0x200 | n, 32_000_000 / ((n + 1) · f))`. Шрифт — `font8x8_basic` Daniel Hepper (public domain, источник и лицензия указаны в комментарии-заголовке `font8x8.rs`).

- [ ] **Step 1: Write the failing tests** (`#[cfg(test)]` в модулях)

```rust
#[test] fn golden_header_from_python() {   // GOLDEN — 0x40 байт из `wp2apps pack` (шаг 2)
    let h = parse_header(&GOLDEN).unwrap();
    assert_eq!((h.kind, h.abi, h.abi_minor, h.version, h.image_size, h.load_addr), (0, 1, 0, 7, 16, 0x386F_F000));
    assert_eq!(h.image_crc, crc32(0, &[1u8; 16]));
}
#[test] fn header_crc_and_reserved_checked() {
    let mut raw = GOLDEN; raw[0x28] ^= 1; assert_eq!(parse_header(&raw), Err(PkgError::HeaderCrc));
    let mut raw = GOLDEN; raw[0x3A] = 1; assert!(parse_header(&raw).is_err());
}
#[test] fn slot_rules() {
    let h = parse_header(&GOLDEN).unwrap();
    assert!(check_slot(&h, 0).is_ok());
    assert_eq!(check_slot(&h, LOADER_SLOT), Err(PkgError::Kind));
    let mut newer = h; newer.abi_minor = 1; assert_eq!(check_slot(&newer, 0), Err(PkgError::Abi));
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

Run: `uv run --locked python -c "from wp2apps import package; print(package.pack(b'\x01'*16, name='golden', version=7)[:0x40].hex())"`. Вставить вывод константой `GOLDEN: [u8; 0x40]` в `pkg.rs`. Формат должен быть уже исправленным владельцем: CRC заголовка, `abi_minor`.

- [ ] **Step 3: Run to verify failure**

Run: `cd fw && cargo test -p wp2-core --target aarch64-apple-darwin`
Expected: FAIL — не компилируется (`parse_header` не определён).

- [ ] **Step 4: Implement `wp2-core`** (`#![no_std]` вне тестов, без аллокаций)

- [ ] **Step 5: Run to verify pass**

Run: `cd fw && cargo test -p wp2-core --target aarch64-apple-darwin`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add fw/Cargo.toml fw/Cargo.lock fw/.cargo fw/wp2-core AGENTS.md .gitignore
git commit -m "feat(fw): wp2-core with package checks, PWM params, canvas, event queue"
```

---

### Task 3: Заглушка: загрузка, view `0xF0`, запуск загрузчика с NAND

**Files:**
- Create: `fw/wp2-stub/{Cargo.toml,build.rs,link.ld,src/main.rs,src/fs.rs,src/view.rs}`
- Create: `scripts/build-fw` (сборка заглушки, загрузчика, пропатченного образа)
- Modify: `src/wp2apps/firmware.py` (точки P2, P4; дописывание заглушки и таблицы view)
- Test: `tests/test_firmware.py`, `tests/test_e2e.py` (класс `StubTest`)

**Interfaces:**
- Consumes: `wp2_core::{layout, pkg, stock}`; `firmware.patch`, `harness.Emu` (задача 1).
- Produces:
  - Заголовок заглушки по `STUB_BASE` (секция `.header`, первая): `#[repr(C)] struct StubHeader { magic: [u8; 4] = *b"WP2S", version: u32 = 1, boot: u32, ble: u32, view_proc: u32, view_name: u32, _r: [u32; 2] }`; адреса функций с битом Thumb.
  - `wp2_boot` (naked): `push {r0-r3, r4, lr}` → `bl wp2_boot_c` (обнуляет `StubRam`, `magic = 0x57503253`, `running_slot = 0xFF`) → `pop {r0-r3, r4, lr}` → `b.w 0x10146AA4`. Стековые аргументы исходного вызова не трогает.
  - `extern "C" fn wp2_view_proc(view_id: u32, msg: u32, data: *mut c_void) -> i32`. Пишет `(msg << 16) | view_id` в кольцо `trace`. На `msg == 2` (preload) загружает загрузчик: `L.pkg` → заголовок, `parse_header` + `check_slot(LOADER_SLOT)`, образ читается кусками прямо в `LOADER_BASE` с потоковым `crc32`. Удача — `loader_state = 1`, неудача — `2`. При `loader_state != 1` возвращает `-2` на любое сообщение. Иначе `loader_active = 1` и вызов `(LOADER_BASE + entry | 1)(view_id, msg, data)`; на `msg == 4` после вызова `loader_active = 0`.
  - `fs.rs`: `open(file: &mut [u8; 0x2C], path: &[u8], flags: u8) -> i32`, `read`, `write`, `close`, `unlink`, `rename`, `mkdir(path)` (через `0x10106FA0` и vtable `+0x3C`, `-134`, если указатель 0).
  - Патчер: `firmware.patch(app, stub)` дописывает `stub` (≤ `STUB_MAX` − таблица) по `0x25A080`, выравнивает до 8 и кладёт следом таблицу view: 162 стоковые записи плюс `{view_name, view_proc, 0, 0, 0xF0, 0x0102, 0x1D2, 0x1D2}`. Литералы начала (`0x101461AC`, `0x1018BC24`, `0x1018DF3C`) меняются на адрес новой таблицы, литералы конца (`0x10055458`, `0x1018BC28`, `0x1018DF40`) — на её конец. **P2:** `0x101884FA` `bl 0x10146AA4` → `bl header.boot`.
  - `harness.patched_nor() -> bytes` = `stage_nor(stock_nor(), target/app-wp2.bin)`.
  - `scripts/build-fw`: `cargo build --release -p wp2-stub -p wp2-loader` в `fw/`; `llvm-objcopy -O binary` (у загрузчика с `--set-section-flags .data=alloc,load,contents`); `wp2apps pack … --kind loader`; `wp2apps firmware --stub target/wp2-stub.bin --output target/app-wp2.bin`. Пишет `target/wp2-stub.bin`, `target/loader.wp2app`, `target/app-wp2.bin`.

`link.ld` заглушки: `MEMORY { FLASH (rx) : ORIGIN = 0x1025A080, LENGTH = 0x3000 }`, `.header` первой, `ASSERT(SIZEOF(.data) + SIZEOF(.bss) == 0, "stub must not have writable statics")`, `PROVIDE(stock_heap_init = 0x10146AA5)`. Состояние хранится только в `StubRam` по `STUB_RAM`.

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

- [ ] **Step 4: Implement the stub, `link.ld`, patch points P2/P4, `scripts/build-fw`**

Загрузчика ещё нет. Для этой задачи `scripts/build-fw` собирает только заглушку и образ, если в `fw/wp2-loader` пусто.

- [ ] **Step 5: Run unit tests**

Run: `uv run --locked python -m unittest discover -s tests -v && (cd fw && cargo test -p wp2-core --target aarch64-apple-darwin)`
Expected: PASS.

- [ ] **Step 6: Write the e2e tests** (`StubTest`)

```python
def test_patched_boots_and_view_without_loader_fails_cleanly(self):
    with harness.Emu("stub", nor=harness.patched_nor()) as emu:          # patched_nor = stage_nor(stock_nor, target/app-wp2.bin)
        emu.wait_uart("launcher enter", 60); emu.keep_awake()
        ram = emu.peek(STUB_RAM, 8)
        self.assertEqual(ram[:4], struct.pack("<I", 0x57503253))
        before = emu.frame()
        emu.call(OPEN_VIEW, 0xF0, 0)
        time.sleep(3)
        trace = emu.peek(STUB_RAM + TRACE_OFF, 4 + 32 * 4)
        self.assertIn((2 << 16) | 0xF0, struct.unpack_from("<32I", trace, 4))
        self.assertEqual(emu.peek(STUB_RAM + 4, 1), b"\x02")               # loader_state = bad/missing
        self.assertNotIn("ZEPHYR FATAL ERROR", emu.uart())
        self.assertEqual(emu.frame(), before)                               # сток остался на своём экране

def test_mkdir_vtable_present(self):
    # gdb: найти mp для "/NAND:C" через call(0x10106FA1, &mp, path, 0), прочитать *(*(mp+0x1C)+0x3C) != 0
```

Если view-менеджер на `-2` в preload ведёт себя иначе (пустой экран, зависание), записать это в `docs/recon.md`. Тогда в заглушке на preload возвращать `0`, на layout рисовать пустой экран и закрывать view через `FUN_101898bc` предыдущего id. Это поведение закрепить тестом.

- [ ] **Step 7: Run e2e**

Run: `nix-shell -p llvm --run scripts/build-fw && WP2_E2E=1 nix-shell -p gdb llvm --run 'uv run --locked python -m unittest tests.test_e2e.StubTest tests.test_e2e.ReservationTest -v'`
Expected: PASS. `ReservationTest` повторяется на образе с заглушкой: `harness.patched_nor()` вместо `patch(stock)`.

- [ ] **Step 8: Commit**

```bash
git add fw/wp2-stub scripts/build-fw src/wp2apps/firmware.py tests/test_firmware.py tests/test_e2e.py tests/e2e docs/recon.md
git commit -m "feat(fw): stub with boot hook, view 0xF0 and NAND loader launch"
```

---

### Task 4: Свой BLE-канал установки (`wp2apps install/remove/list`)

**Files:**
- Create: `fw/wp2-core/src/proto.rs`; `fw/wp2-stub/src/ble.rs`
- Modify: `fw/wp2-stub/src/main.rs` (`wp2_ble_default`), `src/wp2apps/firmware.py` (P3)
- Create: `src/wp2apps/link.py` (кадр, CRC, AES, рукопожатие), `src/wp2apps/h4.py` (GATT-клиент по H4), `src/wp2apps/install.py`
- Modify: `src/wp2apps/cli.py`, `pyproject.toml` (`pycryptodome` прямой зависимостью), `uv.lock`
- Create: `tests/e2e/netsim.py` (запуск `netsimd`, синтетическое сопряжение)
- Test: `tests/test_link.py`, `tests/test_install.py`, `#[cfg(test)]` в `proto.rs`, `tests/test_e2e.py` (класс `InstallTest`)

**Interfaces:**
- Consumes: `StubRam`, `fs.rs`, `pkg` (задачи 2–3).
- Produces — протокол: системные команды `c1 = 0xFFFF`. Payload зашифрован ключом сессии (CRC32 LE + PKCS#7, AES-CBC, IV `5051525354555657606162636465665A`). Ответ — `c2 | 0xA000` (`0x90A0` → `0xB0A0`), первый байт ответа — `Status`.

  | c2 | Запрос | Ответ |
  |---|---|---|
  | `0x90A0` INFO | `u8 slot` | `status`, затем 64 байта заголовка, если слот занят |
  | `0x90A1` BEGIN | `u8 slot, u32 size, u32 crc32(всего файла)` | `status` |
  | `0x90A2` DATA | `u32 offset, ≤ 128 B` | `status, u32 next_offset` |
  | `0x90A3` END | — | `status` |
  | `0x90A4` DELETE | `u8 slot` | `status` |

  `Status`: `Ok 0, BadRequest 1, Io 2, Sequence 3, Crc 4, Busy 5, Invalid 6`. `slot`: `0..=7` или `0xFF` (загрузчик). `Busy` — если слот совпадает с `running_slot` или это загрузчик при `loader_active`. `BEGIN` при незавершённой передаче отменяет старую (удаляет `.tmp`). `END` закрывает файл и перечитывает его: потоковый CRC, `parse_header`, `check_slot`, размер `= 0x40 + image_size`. Затем `unlink` старого и `rename(.tmp, .pkg)`; при ошибке `.tmp` удаляется.
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
  - `installed(names) -> ContextManager[tuple[Emu, int]]` — Netsim + `Emu` + `wait_uart("launcher enter")` + `keep_awake()` + `install target/<name>.wp2app` для каждого имени;
  - константы `OPEN_VIEW = 0x101898BD` и `TRACE_OFF = 0x44` (смещение `trace_head` в `StubRam`).

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

При первом `BEGIN` заглушка создаёт каталог через `fs::mkdir(DIR)`, `-17` (EEXIST) не считается ошибкой. Если в задаче 3 указатель mkdir оказался нулевым, вместо каталога используются имена `/NAND:C/wp2apps-<slot>.pkg` в корне `/NAND:C`: `slot_path` меняется в одном месте и попадает в Rulings.

- [ ] **Step 4: Run unit tests**

Run: `uv run --locked python -m unittest discover -s tests -v && (cd fw && cargo test -p wp2-core --target aarch64-apple-darwin)`
Expected: PASS.

- [ ] **Step 5: Write the e2e tests** (`InstallTest`; Netsim + сопряжённый NOR)

```python
def test_install_list_remove(self):
    with netsim() as port, harness.Emu("install", nor=harness.patched_paired_nor(), hci_port=port) as emu:
        emu.wait_uart("launcher enter", 60); emu.keep_awake()
        hello = (ROOT / "target/hello.wp2app").read_bytes()
        self.assertEqual(cli(["install", "target/hello.wp2app", "--hci-port", str(port)]), 0)
        listed = json.loads(cli_out(["list", "--hci-port", str(port)]))
        self.assertEqual(listed["slots"]["0"]["name"], "hello")
        self.assertEqual(cli(["remove", "--slot", "0", "--hci-port", str(port)]), 0)
        self.assertNotIn("0", json.loads(cli_out(["list", "--hci-port", str(port)]))["slots"])

def test_install_rejects_corrupt(self):
    # пакет с испорченным байтом образа (CRC файла пересчитан хостом под испорченные байты): END → Status.Invalid, слот пуст
    # пакет с abi_minor = 1 (заголовочный CRC пересчитан): END → Status.Invalid

def test_install_interrupted_keeps_old(self):
    # install hello v1; затем BEGIN v2 + половина DATA, разрыв связи (закрыть GATT);
    # list → слот 0 по-прежнему v1; новый BEGIN v2 + все DATA + END → v2
    # DATA с offset != next_offset → Status.Sequence, next_offset не меняется

def test_unauthenticated_frame_ignored(self):
    # кадр 0x90A0 зашифрован случайным ключом → ответа 0xB0A0 нет за 3 с, стоковые команды после этого работают
```

- [ ] **Step 6: Run e2e**

Run: `nix-shell -p llvm --run 'scripts/build-fw && scripts/build-app hello 1' && WP2_E2E=1 nix-shell -p gdb llvm --run 'uv run --locked python -m unittest tests.test_e2e.InstallTest -v'`
Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add fw/wp2-core/src/proto.rs fw/wp2-stub src/wp2apps/link.py src/wp2apps/h4.py src/wp2apps/install.py src/wp2apps/cli.py src/wp2apps/firmware.py pyproject.toml uv.lock tests/test_link.py tests/test_install.py tests/e2e tests/test_e2e.py
git commit -m "feat: own BLE install channel in the stub with wp2apps install/remove/list"
```

---

### Task 5: Загрузчик: view, буфер кадра, меню установленных приложений

**Files:**
- Create: `fw/wp2-loader/{Cargo.toml,build.rs,link.ld,src/main.rs,src/ui.rs,src/menu.rs,src/stock.rs}`
- Modify: `scripts/build-fw` (упаковка `loader.wp2app`)
- Test: `tests/test_e2e.py` (класс `LoaderTest`)

**Interfaces:**
- Consumes: `wp2_core::{gfx, pkg, layout, stock, events}`, заглушка (задача 3), установка (задача 4).
- Produces:
  - `#[unsafe(no_mangle)] #[unsafe(link_section = ".entry")] extern "C" fn loader_main(view_id: u32, msg: u32, data: *mut c_void) -> i32` — первая функция образа (`entry = 0`). `link.ld`: `ORIGIN = 0x386EF000, LENGTH = 0x10000`, `.entry`, `.text`, `.rodata`, `.data` + `.bss` внутри образа (как `sdk/cmf-wp2-app/app.ld`).
  - `ui.rs`: `fn layout(data: *mut c_void) -> i32` (msg 3). Получает `scr = lv_disp_get_scr_act(*(data as *const *mut c_void))` и выделяет буфер `W*H*2` через `ui_mem_aligned_alloc(FB_MEM_TYPE, 4, size, 0)`; при неудаче возвращает `-12`. Создаёт `img = lv_img_create(scr)`, `lv_img_set_src(img, &DSC)`, где `DSC: lv_img_dsc_t` с `header = cf | (466 << 10) | (466 << 21)` и `cf = LV_IMG_CF_TRUE_COLOR (4)`. Вешает `lv_obj_add_event_cb(scr, touch_cb, 0, null)` и `TIMER = lv_timer_create(tick, 20, null)`. Также `fn delete()` (msg 4; освобождает таймер и буфер, объекты LVGL не удаляет — так делает `goodbye_view`), `fn invalidate()` (`lv_obj_invalidate(img)`), `fn canvas() -> Canvas<'static>`.
  - `menu.rs`: `fn scan() -> [Option<Entry>; 8]` (`Entry { slot: u8, name: [u8; 24], len: u8 }`, заголовок из `n.pkg`, только прошедшие `parse_header` + `check_slot`), `fn draw(c: &mut Canvas, entries, selected: usize)` (заголовок `Apps` по `y = 60`, строки высотой 60 с `y = 120`, слева отступ `x = 90`; без приложений — `No apps`), `fn hit(y: i16) -> Option<usize>`.
  - Сообщения: 2 → `0`; 3 → `layout` + `menu::draw`; 0xD → поворот коронки двигает выделение (событие помечается обработанным), остальное не трогается (стоковая кнопка выходит из view); `touch_cb` с `PRESSED`/`RELEASED` в меню при `hit` запоминает выбор (запуск — задача 6); 4 → `delete`.
  - Глобальные `FB_MEM_TYPE`, `LV_COLOR_DEPTH` берутся из шага 1.

- [ ] **Step 1: Разведка формата цвета и памяти (только чтение).**
  - `LV_COLOR_DEPTH` и `LV_COLOR_16_SWAP`: декомпилировать `lv_canvas_fill_bg` (`0x1013E9AC`) и `lv_img_buf_get_img_size` (вызывается из `lv_canvas_set_buffer` `0x1013E52C`), найти множитель байт на пиксель для `TRUE_COLOR`. Если 32 бита, `Canvas` из задачи 2 хранит `u32` ARGB8888 с переводом из RGB565 на записи; тесты задачи 2 правятся под это в этом же коммите.
  - Тип памяти для буфера кадра: в `ui_mem.c` (`0x101882C4`, таблица `*0x1018837C`, по `0x24` Б на тип) найти имена типов. Взять тип, который сток использует для `lv_img`/canvas-буферов, и парную функцию освобождения. Записать в `docs/recon.md`.
  - `lv_indev_get_point`: в вызывающих `0x10125BD4` найти следующий вызов вида `(indev, lv_point_t*)`. Проверить, что `0x10125BD4` действительно `lv_event_get_indev`.
- [ ] **Step 2: Write the e2e test** (`LoaderTest`)

```python
def test_menu_lists_installed_apps(self):
    with netsim() as port, harness.Emu("loader", nor=harness.patched_paired_nor(), hci_port=port) as emu:
        emu.wait_uart("launcher enter", 60); emu.keep_awake()
        for pkg in ("target/loader.wp2app", "target/hello.wp2app", "target/vibration-lab.wp2app"):
            self.assertEqual(cli(["install", pkg, "--hci-port", str(port)]), 0)
        emu.call(OPEN_VIEW, 0xF0, 0); time.sleep(3)
        self.assertEqual(emu.peek(STUB_RAM + 4, 2), b"\x01\x01")          # loader loaded, active
        w, h, rgb = emu.frame()
        self.assertTrue(bright_pixels(rgb, w, box=(90, 120, 300, 40)) > 50)    # строка "hello"
        self.assertTrue(bright_pixels(rgb, w, box=(90, 180, 300, 40)) > 50)    # строка "vibration-lab"
        emu.send("c 1", "c 0", pause=2)                                     # кнопка → сток закрывает view
        self.assertEqual(emu.peek(STUB_RAM + 5, 1), b"\x00")
```

- [ ] **Step 3: Run to verify failure**

Run: `WP2_E2E=1 nix-shell -p gdb llvm --run 'uv run --locked python -m unittest tests.test_e2e.LoaderTest -v'`
Expected: FAIL — `target/loader.wp2app` нет.

- [ ] **Step 4: Implement the loader (view, canvas, menu)**

- [ ] **Step 5: Run to verify pass**

Run: `nix-shell -p llvm --run 'scripts/build-fw && scripts/build-app hello 1 && scripts/build-app vibration-lab 1' && WP2_E2E=1 nix-shell -p gdb llvm --run 'uv run --locked python -m unittest tests.test_e2e.LoaderTest -v'`
Expected: PASS. Кадр меню сохранить в `runs/e2e/loader-*/menu.ppm` для ревью.

- [ ] **Step 6: Commit**

```bash
git add fw/wp2-loader scripts/build-fw tests/test_e2e.py tests/e2e docs/recon.md
git commit -m "feat(fw): NAND loader with framebuffer view and installed-apps menu"
```

---

### Task 6: Запуск приложения и ABI 1 `Api` (экран, ввод, время, выход)

**Files:**
- Create: `fw/wp2-loader/src/app.rs`
- Modify: `fw/wp2-loader/src/{main.rs,ui.rs,menu.rs}`
- Test: `tests/test_e2e.py` (класс `HelloTest`)

**Interfaces:**
- Consumes: `wp2_core::{events::{Queue, map_key, Event}, Api, pkg, layout}`; `StubRam.running_slot`.
- Produces:
  - `fn launch(slot: u8) -> Result<(), i32>` (UI-поток). Читает `n.pkg`, проверяет (`parse_header`, `check_slot`, потоковый CRC), копирует образ в `APP_BASE`, ставит `running_slot = slot`, очищает очередь и кадр. Вызывает `os_thread_create(&mut THREAD_BUF, THREAD_BUF.len(), app_thread, entry, &API, 0, APP_PRIO, 0, 0)`; буфер `THREAD_BUF: [u8; 0x118 + 0x2000]` (выровнен на 8) лежит в `.bss` загрузчика.
  - `extern "C" fn app_thread(entry: u32, api: *const Api, _: u32)`: `let code = entry(api); EXITED.store(code | FLAG)`; дальше поток засыпает в `loop { os_sleep(1000) }`, чтобы UI-поток мог его остановить.
  - `static API: Api = Api { abi: 1, abi_minor: 0, width: 466, height: 466, … }`. Функции:
    - `fill_rect`, `draw_text`: рисуют в `Canvas`, без LVGL; длина текста ограничена 64 байтами.
    - `present`: `DIRTY.store(true)`; перерисовку делает таймер `tick` через `lv_obj_invalidate`.
    - `next_event(ev, timeout_ms)`: `pop`, иначе `os_sleep(5)` до таймаута; `u32::MAX` — ждать бесконечно; возвращает только `0`/`1`.
    - `millis`: из шага 1.
    - `exit(code)`: `EXITED.store`, затем тот же сон.
  - `tick` (UI-поток, 20 мс): если `EXITED`, то `stop_app()` и меню. `stop_app()`: мотор выкл (задача 7), `k_thread_abort(&THREAD_BUF)`, `running_slot = 0xFF`.
  - Ввод в режиме приложения: `touch_cb` → `TOUCH_DOWN`/`TOUCH_UP` с координатами `lv_indev_get_point`. Сообщение 0xD → `map_key`: на `Some` событие ставится в очередь и помечается обработанным (`*(data as *mut u8).add(4) = 1`).
  - Сообщение 4 (delete) с работающим приложением: `push(BUTTON)`, затем `stop_app()` сразу, без ожидания (UI-поток нельзя блокировать).
  - Меню: тап по строке → `launch(slot)`. Ошибка → строка `Error <code>` на 2 с.

- [ ] **Step 1: Разведка времени, приоритета, abort (только чтение).**
  - `millis`: найти `k_uptime_get_32`/`k_uptime_ticks` — например, в функции, которая ставит метку времени в стоковый лог (`[8460][os]…`), или по syscall-обёртке рядом с `0x1019B268` (`k_sleep`). Записать адрес и единицы (мс или тики, `CONFIG_SYS_CLOCK_TICKS_PER_SEC`).
  - `k_thread_abort`: выбрать из `0x10105C90` и `0x1019B2D8` (сигнатура `(k_tid_t)`, вызов из `z_thread_abort`).
  - `APP_PRIO`: приоритет UI-потока (`lvglworkq` / `ui_service`) — по вызову `os_thread_create` или `k_work_queue_start` с этим именем. `APP_PRIO` = на 1 ниже (численно больше), чтобы зависшее приложение не останавливало UI.
  - Направление поворота: задача 2 приняла `0x…6F = +1`. Подтвердить по `view_key:` в UART при `r 80` и `r -80`; если наоборот — поправить `map_key` и тест.
- [ ] **Step 2: Write the e2e tests** (`HelloTest`)

```python
def test_hello_draws_reacts_and_exits(self):
    with installed(["loader", "hello"]) as (emu, port):          # фикстура: netsim + Emu + install
        emu.call(OPEN_VIEW, 0xF0, 0); time.sleep(2)
        emu.send("t 1 200 140", "t 0 200 140", pause=2)              # строка 0 → hello
        self.assertEqual(emu.peek(STUB_RAM + 6, 1), b"\x00")         # running_slot = 0
        w, h, rgb = emu.frame()
        self.assertEqual(pixel(rgb, w, 233, 100), (0, 0, 0))          # чёрный фон
        self.assertTrue(bright_pixels(rgb, w, box=(213, 225, 60, 20)) > 20)  # "hello" у центра
        emu.send("r 80", pause=1.5)
        r, g, b = pixel(emu.frame()[2], w, 233, 100)
        self.assertTrue(r < 16 and g < 16 and b > 200)                # 0x001F → синий
        emu.send("c 1", "c 0", pause=2)                               # BUTTON → main возвращает 0
        self.assertEqual(emu.peek(STUB_RAM + 6, 1), b"\xff")
        self.assertTrue(bright_pixels(emu.frame()[2], w, box=(90, 120, 300, 40)) > 50)  # снова меню

def test_view_delete_stops_app(self):
    # запустить hello; закрыть view свайпом вправо (t 1 30 233 … t 0 400 233) или call(OPEN_VIEW, 0x10, 0);
    # running_slot == 0xFF, loader_active == 0, нет FATAL; снова OPEN_VIEW 0xF0 → меню рисуется, hello запускается

def test_millis_matches_virtual_time(self):
    # gdb: call(API.millis) дважды с паузой 2 с хост-времени; разница в пределах [1500, 2500] при icount shift=3
```

- [ ] **Step 3: Run to verify failure**

Run: `WP2_E2E=1 nix-shell -p gdb llvm --run 'uv run --locked python -m unittest tests.test_e2e.HelloTest -v'`
Expected: FAIL — тап по строке ничего не запускает.

- [ ] **Step 4: Implement `app.rs` and input wiring**

- [ ] **Step 5: Run to verify pass**

Run: `nix-shell -p llvm --run scripts/build-fw && WP2_E2E=1 nix-shell -p gdb llvm --run 'uv run --locked python -m unittest tests.test_e2e.HelloTest tests.test_e2e.LoaderTest -v'`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add fw/wp2-loader tests/test_e2e.py tests/e2e docs/recon.md
git commit -m "feat(fw): run apps on their own thread with ABI 1 display, input, time and exit"
```

---

### Task 7: Мотор: `Api.motor`, частота, сторож 2000 мс

**Files:**
- Create: `fw/wp2-loader/src/motor.rs`; `sdk/examples/motor-hog/{Cargo.toml,src/main.rs}`
- Modify: `fw/wp2-loader/src/{app.rs,ui.rs}`, `sdk/Cargo.toml` (member `examples/motor-hog`)
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
  - `pub fn off()`: `api[4](dev, 0)`, `CMU_PWMCLK0 = 0x303`, `ON_SINCE.store(0)`. Вызывается из `stop_app`, паузы (задача 8) и сторожа.
  - Сторож в `tick`: `ON_SINCE != 0 && millis() − ON_SINCE ≥ 2000` → `off()`. Гонку с записью из потока приложения принимаем: обе стороны приводят мотор в определённое состояние.
  - `motor-hog`: `main` вызывает `api.vibrate(1000, 31)` и крутится в `loop { core::hint::spin_loop() }`, не вызывая `event`.

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
```

`installed(…)`, `open_app(emu, row)`, `lab_play(emu)` — помощники в `tests/e2e/harness.py`. `open_app` открывает view `0xF0` (до задачи 9 через `call`) и тапает по строке `row`.

- [ ] **Step 2: Run to verify failure**

Run: `nix-shell -p llvm --run 'scripts/build-app vibration-lab 1 && scripts/build-app motor-hog 1' && WP2_E2E=1 nix-shell -p gdb llvm --run 'uv run --locked python -m unittest tests.test_e2e.MotorTest -v'`
Expected: FAIL — `motor` загрузчика пока заглушка `-1`, в `motor.log` пусто.

- [ ] **Step 3: Implement `motor.rs`, the watchdog, `motor-hog`**

- [ ] **Step 4: Run to verify pass**

Run: `nix-shell -p llvm --run scripts/build-fw && WP2_E2E=1 nix-shell -p gdb llvm --run 'uv run --locked python -m unittest tests.test_e2e.MotorTest -v'`
Expected: PASS. QEMU должен быть собран с исправленной расшифровкой делителя (`motor.log` для стока даёт ≈30–31 Гц). Если в `motor.log` 482 Гц, PR лога мотора ещё не исправлен: остановиться и сообщить владельцу.

- [ ] **Step 5: Commit**

```bash
git add fw/wp2-loader sdk/examples/motor-hog sdk/Cargo.toml sdk/Cargo.lock tests/test_e2e.py tests/e2e
git commit -m "feat(fw): motor API with PWM clock handling and 2 s continuous-run cap"
```

---

### Task 8: Пауза и продолжение под стоковым всплывающим окном

**Files:**
- Modify: `fw/wp2-loader/src/{ui.rs,app.rs,motor.rs}`, `sdk/Cargo.toml` (member `examples/motor-hold`)
- Create: `sdk/examples/motor-hold/{Cargo.toml,src/main.rs}`
- Test: `tests/test_e2e.py` (класс `PauseTest`)

**Interfaces:**
- Consumes: сообщения view 7/8, флаги окна `*(u8*)0x3800FAF4 & 3`.
- Produces: `fn overlay() -> bool = DEFOCUSED || (*(0x3800FAF4 as *const u8) & 3) != 0`. В `tick`:
  - переход `false → true`: `PAUSED = true`, `motor::off()`, очистить очередь, `push(PAUSE)`;
  - переход `true → false`: `PAUSED = false`, `push(RESUME)`, `invalidate()`.
  
  На паузе `motor` возвращает `-3`; рисовать в буфер можно, `present` ничего не делает до `RESUME`.

- [ ] **Step 1: Разведка (эмулятор): что показывает окно.** На стоковом образе: открыть любое приложение из списка, отправить `p 4000 1 60`, через 5 с `p 4000 0 60`. Записать в `docs/recon.md`:
  - есть ли в `uart0.log` `msgbox_cache popup` и `set_popup_msgbox` и с каким `mid`;
  - получает ли текущий view `defocus`/`focus` (строки `----->view:`);
  - меняется ли байт `0x3800FAF4` (через `peek`).
  
  Если зарядка не открывает окно, взять низкий заряд `p 3400 0 4`. Если и он не работает — вызвать `call(0x100D5E35, <mid из таблицы msgbox>, 0)` для одного стокового id (оповещение будильника или низкий заряд). Таблицу id найти от `FUN_100d72a8(mid)` и записать. Выбранный триггер записать в харнес как `emu.popup()` / `emu.close_popup()`.
- [ ] **Step 2: Write the e2e test** (`PauseTest`)

```python
def test_popup_pauses_and_releases_motor(self):
    with installed(["loader", "motor-hold"]) as (emu, port):   # см. ниже
        open_app(emu, row=0); time.sleep(0.5)
        self.assertGreater(emu.motor()[-1][1], 0)
        emu.popup(); time.sleep(1)
        self.assertEqual(emu.motor()[-1][1], 0)
        self.assertEqual(struct.unpack("<I", emu.peek(0x40001068, 4))[0], 0x303)
        self.assertEqual(emu.peek(STUB_RAM + 6, 1), b"\x00")          # приложение живо
        emu.close_popup(); time.sleep(1.5)
        self.assertEqual(read_marker(emu), [PAUSE, RESUME])            # приложение записало, что получило
        self.assertTrue(bright_pixels(emu.frame()[2], 466, box=(150, 200, 166, 60)) > 20)  # кадр приложения вернулся

def test_lab_stops_measurement_on_pause(self):
    # vibration-lab: PLAY с Pattern=Long; popup → в motor.log сразу duty 0; после close_popup мотор не включается сам
```

`motor-hold` — пример `sdk/examples/motor-hold`. Он вызывает `vibrate(1000, 31)` и затем в цикле `event(1000)`, а каждый полученный `kind` дописывает по фиксированному адресу `APP_BASE + 0x1F000` (`[count: u32, kinds: [u8; 16]]`). После `RESUME` мотор сам не включает. `read_marker` читает этот журнал через `peek`.

- [ ] **Step 3: Run to verify failure**

Run: `WP2_E2E=1 nix-shell -p gdb llvm --run 'uv run --locked python -m unittest tests.test_e2e.PauseTest -v'`
Expected: FAIL — мотор не выключается при показе окна.

- [ ] **Step 4: Implement pause/resume**

- [ ] **Step 5: Run to verify pass**

Run: `nix-shell -p llvm --run 'scripts/build-fw && scripts/build-app motor-hold 1' && WP2_E2E=1 nix-shell -p gdb llvm --run 'uv run --locked python -m unittest tests.test_e2e.PauseTest tests.test_e2e.MotorTest -v'`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add fw/wp2-loader sdk/examples/motor-hold sdk/Cargo.toml sdk/Cargo.lock tests/test_e2e.py tests/e2e docs/recon.md
git commit -m "feat(fw): pause apps under stock popups and hand the motor back to stock"
```

---

### Task 9: Пункт «Apps» в стоковом списке приложений

**Files:**
- Modify: `src/wp2apps/firmware.py` (точка P5), `fw/wp2-stub/src/main.rs` (если нужен код), `docs/recon.md`
- Test: `tests/test_firmware.py`, `tests/test_e2e.py` (класс `AppListTest`), `tests/e2e/harness.py` (`open_apps` через UI)

**Interfaces:**
- Consumes: view `0xF0` (задача 3).
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

Run: `uv run --locked python -m unittest tests.test_firmware -v && nix-shell -p llvm --run scripts/build-fw && WP2_E2E=1 nix-shell -p gdb llvm --run 'uv run --locked python -m unittest tests.test_e2e.AppListTest -v'`
Expected: PASS; кадр списка с новым пунктом сохранить в `runs/e2e/applist-*/list.ppm`.

- [ ] **Step 7: Commit**

```bash
git add src/wp2apps/firmware.py fw/wp2-stub tests docs/recon.md
git commit -m "feat: Apps entry in the stock app list"
```

---

### Task 10: Приёмка этапов 2–4 в эмуляторе

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
    # AT REBOOT по shell; после загрузки: "launcher enter", нет FATAL; Apps → view не открылся (loader_state == 2),
    # стоковые экраны (часы, список, настройки) открываются; `wp2apps install target/loader.wp2app` чинит, Apps работает

def test_install_survives_reboot(self):
    # install loader + hello; AT REBOOT; open_apps → "hello" в меню и запускается

def test_loader_update_over_ble(self):
    # install loader v1; install loader v2 (version в заголовке 2); list → slot L версия 2; Apps открывается;
    # install loader при открытом Apps → Status.Busy

def test_vibration_lab_all_knobs(self):
    # Strength: 3 щелчка коронки вверх → duty 350‰ в motor.log при PLAY
    # Freq: выбор строки, щелчки → каждое значение из списка лаборатории даёт свою Hz в motor.log
    #   (частоты > 78 Гц идут через CMU_PWMCLK ≠ 0x303; после стопа снова 0x303)
    # Pattern Tick / Double / Long: длительности включений в motor.log = pulse_ms, pulse/80/pulse, 1000 мс (±20 мс)
    # Pulse: 5 мс и 100 мс на краях
    # кадр лаборатории сохранить в runs/e2e/acceptance-*/lab.ppm
```

- [ ] **Step 2: Run the whole suite**

Run: `uv run --locked python -m unittest discover -s tests -v && (cd sdk && cargo test --target aarch64-apple-darwin) && (cd fw && cargo test -p wp2-core --target aarch64-apple-darwin) && nix-shell -p llvm --run 'scripts/build-fw && scripts/build-app hello 1 && scripts/build-app vibration-lab 1 && scripts/build-app motor-hog 1 && scripts/build-app motor-hold 1' && WP2_E2E=1 nix-shell -p gdb llvm --run 'uv run --locked python -m unittest tests.test_e2e -v'`
Expected: всё PASS.

- [ ] **Step 3: Ручная проверка в окне (владелец или исполнитель).** Запустить `scripts/emu --nor runs/e2e/<run>/in-nor.bin --hci-port <netsim>`, установить загрузчик и лабораторию, пройти «Apps → vibration-lab». Убедиться, что строка `Motor: …` в окне viewer'а меняется вместе с ручками.

- [ ] **Step 4: Commit**

```bash
git add tests/test_e2e.py tests/e2e README.md
git commit -m "test: emulator acceptance for loader, install, pause and vibration lab"
```

---

### Task 11: Этап 5 — физические часы (только с явного разрешения владельца)

- [ ] **Получить явное «да» владельца на каждое действие этого пункта.** Без него ничего из списка ниже не выполнять.
  - До разрешения допустимы только чтения с Mac: `scan` и `info`. Цель — убедиться, что BLE на macOS работает. Это отдельная задача: нужен физический транспорт (`bleak`) в `wp2apps`.
  - Аппаратные риски, которые эмулятор не проверяет. Исполнение из PSRAM `0x386F_F000`: MPU с `PRIVDEFENA` и кеши — после копирования образа нужна чистка D-cache и инвалидация I-cache; функцию для этого в стоке ещё надо найти. Реальный такт PWM: замерить, что сток — около 30 Гц. Сторож Zephyr при зависшем приложении. Как восстановить часы, если пропатченный `app.bin` не загрузится: процедура не доказана.
  - Пропатченный `app.bin` упаковывается в AOTA и проходит `cmf-watch-utils emu preflight`; стоковая OTA-последовательность (INIT2 в `ff01`, обязательный end-ack `0x9041 A5`) не меняется.

---

## После этого плана

- Владелец может вернуть кириллицу в подписи — потребуется шрифт с кириллицей вместо `font8x8_basic`.
- Минорные версии ABI добавляют функции (датчики, хранилище) в конец `Api` и поднимают `LOADER_ABI_MINOR`. Прошивку при этом менять не нужно.
- Постоянные мягкие уровни вибрации в стоковом меню делаются отдельным дизайном после выбора параметров в лаборатории.
