# Загрузчик приложений WP2, часть 1: разведка и основа — план реализации

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Выяснить в стоковом `app.bin` всё, что нужно загрузчику, и построить то, что от прошивки не зависит: окружение репозитория, формат пакета приложения, Rust SDK, логику «лаборатории вибрации» и отображение мотора в эмуляторе.

**Architecture:** Спека делится на две части. Эта часть (план 1) — разведка плюс всё, что тестируется без патча прошивки. Часть 2 (заглушка в `app.bin`, загрузчик на NAND, меню «Apps», свой BLE-канал, пауза поверх стоковых событий) пишется отдельным планом **после** задачи 1, потому что её адреса и точки встраивания известны только из разведки. Приложения линкуются на один фиксированный адрес `APP_BASE` (одновременно работает одно приложение), поэтому PIC и релокации не нужны. Эмулятор и прошивка берутся из соседнего checkout'а `cmf-watch-utils`; правка эмулятора уходит туда PR'ом.

**Tech Stack:** Python 3.12 (uv, unittest), Rust `no_std` для `thumbv7em-none-eabi` (soft-float), `llvm-objcopy`, QEMU из `cmf-watch-utils` (C, Tcl/Tk viewer).

**Spec:** `docs/specs/2026-11-10-wp2-app-loader-design.md`

## Global Constraints

- Все правила `AGENTS.md` этого репозитория. Никаких записей на физическое устройство в этом плане вообще.
- Основа только JX402 `1.0.0.73`; адреса из этого `app.bin` не переносятся на другие сборки.
- Код из `cmf-watch-utils` не копируется: только зависимость (uv git, коммит `e67d88a87d61c2a65f1d4c024853ef9623b3ff05`) и пути к его `runtime/`, `fixtures/`, прошивке через `CMF_WATCH_UTILS` (по умолчанию `../cmf-watch-utils`).
- Не коммитить бинарники прошивки, собранные приложения (`*.bin`, `*.wp2app`), `target/`, `runs/`.
- Хост-инструменты — через `nix-shell -p …`, не Homebrew. Rust-таргет: `rustup target add thumbv7em-none-eabi`.
- Весь репозиторий — MIT.
- Тесты Python — `unittest` в `tests/`, запуск: `uv run --locked python -m unittest discover -s tests -v`. Тесты Rust-логики — на хосте: `cd sdk && cargo test --target aarch64-apple-darwin`.
- Защита мотора: непрерывная работа не дольше **2000 мс** за одну команду; скважность в промилле `0..=1000`, `0` = выкл.

## Review Focus

1. Имя приложения с кириллицей или длиннее поля → понятная `ValueError`, а не обрезка посреди UTF-8 символа. Тест — задача 2.
2. Обрезанный или испорченный пакет (CRC не совпадает, `image_size` больше данных, чужая магия) → `parse` отказывает. Тест — задача 2.
3. Образ больше области `APP_SIZE` или слинкованный не на `APP_BASE` → `pack` отказывает, а не создаёт пакет, который загрузчик запишет поверх чужой памяти. Тест — задача 2.
4. Колёсико прокручено далеко за границы → значения зажаты; скважность никогда не > 1000, ни один шаг не длиннее 2000 мс. Тест — задача 4.
5. Пауза (стоковое уведомление) посреди проигрывания рисунка → первая же команда — мотор выкл, проигрывание не продолжается после возобновления. Тест — задача 4.

---

### Task 0: Окружение репозитория и запуск эмулятора

**Files:**
- Create: `pyproject.toml`, `uv.lock`
- Create: `src/wp2apps/__init__.py`, `src/wp2apps/paths.py`
- Create: `scripts/emu` (исполняемый)
- Test: `tests/test_paths.py`

**Interfaces:**
- Produces:
  - пакет `wp2apps`, CLI-точка `wp2apps = "wp2apps.cli:main"` (сам `cli.py` — задача 2);
  - `paths.cmf_watch_utils() -> Path` — `CMF_WATCH_UTILS` или `<repo>/../cmf-watch-utils`, `FileNotFoundError` с понятным текстом, если каталога нет;
  - `paths.firmware() -> Path` — `<cmf>/dumps/binaries/firmware/JX402_V1.0.0.73.bin`, `FileNotFoundError`, если файл меньше 1 MiB (LFS-указатель);
  - `scripts/emu [ARGS…]` — `nix-shell -p tk --run 'uv run cmf-watch-utils emu run --fixtures <cmf>/fixtures --qemu <cmf>/runtime/qemu-system-arm --rom <cmf>/runtime/compat.bin --output runs/<timestamp> ARGS…'`.

`pyproject.toml`: `name = "wp2apps"`, `requires-python = ">=3.12"`, зависимости `cmf-watch-utils` и `cmf-watch-fwkit`; `[tool.uv.sources]` — `cmf-watch-utils = { git = "https://github.com/arnav-kr/cmf-watch-utils", rev = "e67d88a87d61c2a65f1d4c024853ef9623b3ff05" }`, `cmf-watch-fwkit` — то же с `subdirectory = "tooling"`. Сборка — hatchling, `packages = ["src/wp2apps"]`.

- [ ] **Step 1: Write the failing tests**

```python
class PathsTest(unittest.TestCase):
    def test_env_override(self):
        with tempfile.TemporaryDirectory() as d, patch.dict(os.environ, {"CMF_WATCH_UTILS": d}):
            self.assertEqual(paths.cmf_watch_utils(), Path(d).resolve())

    def test_missing_checkout(self):
        with patch.dict(os.environ, {"CMF_WATCH_UTILS": "/nonexistent/cmf"}):
            with self.assertRaises(FileNotFoundError):
                paths.cmf_watch_utils()

    def test_lfs_pointer_rejected(self):
        with tempfile.TemporaryDirectory() as d, patch.dict(os.environ, {"CMF_WATCH_UTILS": d}):
            fw = Path(d) / "dumps/binaries/firmware/JX402_V1.0.0.73.bin"
            fw.parent.mkdir(parents=True)
            fw.write_bytes(b"version https://git-lfs.github.com/spec/v1\n")
            with self.assertRaises(FileNotFoundError):
                paths.firmware()
```

- [ ] **Step 2: Run to verify failure**

Run: `uv run python -m unittest tests.test_paths -v`
Expected: FAIL — `ModuleNotFoundError: No module named 'wp2apps'`.

- [ ] **Step 3: Implement `pyproject.toml`, `paths.py`, `scripts/emu`; run `uv lock`**

- [ ] **Step 4: Verify**

Run: `uv run --locked python -m unittest discover -s tests -v` — Expected: PASS.
Run: `scripts/emu --headless` — Expected: JSON с `"panel_frame_captured": true`.
Run: `scripts/emu` — Expected: окно с циферблатом без артефактов.

- [ ] **Step 5: Commit**

```bash
git add pyproject.toml uv.lock src/wp2apps tests/test_paths.py scripts/emu
git commit -m "build: Python workspace pinned to cmf-watch-utils and emulator wrapper"
```

---

### Task 1: Разведка стокового `app.bin`

**Files:**
- Create: `docs/recon.md`
- Create (по необходимости): `scripts/recon/*.py` — разовые скрипты анализа, которые нужны для воспроизведения выводов

**Interfaces:**
- Consumes: `paths.firmware()` (задача 0); `app.bin` извлекается через `fwkit` (зависимость), а не копируется в репозиторий.
- Produces: `docs/recon.md` с таблицей ответов, на которую ссылаются задачи 2, 3, 4, 5 и план 2. Обязательны: `APP_BASE`, `APP_SIZE`, разрешение экрана, PWM-канал мотора и его регистры, тип мотора, стоковые уровни вибрации. Остальное — с адресами функций или пометкой «не найдено» и что пробовали.

Инструменты: Ghidra (руководство — `../cmf-watch-utils/site/src/content/docs/devices/cmf-watch-pro-2/guides/ghidra.md`) или capstone (`uv run --with capstone`), эмулятор (`scripts/emu`, лог UART в `runs/<run>/uart0.log`, GDB из `nix-shell -p gdb`). Известная справка: `../cmf-watch-utils/site/src/content/docs/devices/cmf-watch-pro-2/internals/{appbin,hardware}.md`, `protocol/ble.md`.

- [ ] **Step 1: Память для приложения.** Найти область RAM/PSRAM, которую сток не использует и из которой Cortex-M может исполнять код. Записать `APP_BASE` (выровнен на 0x1000) и `APP_SIZE` (≥ 64 KiB, если есть). Проверить в эмуляторе: заполнить область маркером через GDB после загрузки стока, поработать в меню, убедиться, что маркер цел. Записать ядро (M4F/M33) и наличие FPU.
- [ ] **Step 2: Мотор.** От строк `_motor_vibration_strength` (литерал на `0x100FB4B0`), `motor_pwm` (`0x101DF564`), `vibration_level` (`0x100BBB50`), `ux2sys set vibrate strength` (`0x100E1780`) и обработчика сообщения `0x2a` (из `SETMOTOR`, `0x100EA4A4`) найти: функцию «включить с силой», таблицу «уровень → скважность/частота», PWM-канал и регистры (окно `0x60000..0x60818`, в `../cmf-watch-utils/emulator/hw/ats3089c.c` сейчас принимается без модели). Определить тип мотора (ERM/LRA). Записать стоковые значения для каждого уровня — это объясняет, почему минимум слишком сильный.
- [ ] **Step 3: Экран и ввод.** Разрешение панели (кадр эмулятора 466×466, в `hardware.md` указано 410×502 — выяснить, что верно), формат кадрового буфера, функции рисования прямоугольника/текста или LVGL-объекты; есть ли в стоковом шрифте кириллица. Где приходят события касания, колёсика, кнопки.
- [ ] **Step 4: Меню и оверлей.** Как строится список приложений (меню-билдер, ресурсы иконки/строки) и как добавить пункт. Как стоковый UI показывает уведомление/звонок поверх текущего экрана — функция и событие.
- [ ] **Step 5: NAND, BLE, место для заглушки.** Функции открытия/чтения файла на `/NAND:*`. Таблица диспетчеризации BLE-опкодов и свободный номер для своего канала (с той же аутентификацией сессии). Свободное место в образе `app.bin`/NOR для заглушки и точка перехода в неё при старте UI.
- [ ] **Step 6: Документ.** Записать ответы таблицей «вопрос → адрес/значение → уверенность → как проверено».
- [ ] **Step 7: Commit**

```bash
git add docs/recon.md scripts/recon
git commit -m "docs: stock app.bin reconnaissance for the app loader"
```

---

### Task 2: Формат пакета приложения и `wp2apps pack` / `wp2apps info`

**Files:**
- Create: `src/wp2apps/package.py`, `src/wp2apps/cli.py`
- Test: `tests/test_package.py`

**Interfaces:**
- Consumes: `APP_BASE`, `APP_SIZE` из `docs/recon.md` (задача 1) — константы в `package.py`.
- Produces:
  - `HEADER_SIZE = 0x40`, `MAGIC = b"WP2A"`, `ABI = 1`, `KIND_APP = 0`, `KIND_LOADER = 1`, `APP_BASE`, `APP_SIZE`
  - `@dataclass(frozen=True) class Package: kind: int; abi: int; name: str; version: int; load_addr: int; entry: int; image: bytes`
  - `pack(image: bytes, *, name: str, version: int, entry: int = 0, kind: int = KIND_APP, load_addr: int = APP_BASE) -> bytes`
  - `parse(data: bytes) -> Package` — `ValueError` при любой ошибке
  - `cli.main(argv=None) -> int`: `wp2apps pack BIN --name NAME --version N --output OUT` и `wp2apps info FILE` (JSON в stdout без `image`, с `image_size` и `crc32` в hex). Ошибки `ValueError`/`OSError` → `Error: …` в stderr и код 1.

Заголовок, little-endian, 0x40 байт:

| off | тип | поле |
|---|---|---|
| 0x00 | 4s | magic `WP2A` |
| 0x04 | u16 | abi (= 1) |
| 0x06 | u8 | kind |
| 0x07 | u8 | 0 |
| 0x08 | 24s | name, UTF-8, дополнен NUL, минимум один NUL в конце |
| 0x20 | u32 | version |
| 0x24 | u32 | image_size |
| 0x28 | u32 | entry (смещение от начала образа) |
| 0x2C | u32 | load_addr |
| 0x30 | u32 | `zlib.crc32(image)` |
| 0x34 | 12s | нули |

- [ ] **Step 1: Write the failing tests**

```python
class PackageTest(unittest.TestCase):
    def test_round_trip(self):
        image = bytes(range(256)) * 4
        data = package.pack(image, name="Vibration Lab", version=3)
        self.assertEqual(len(data), package.HEADER_SIZE + len(image))
        pkg = package.parse(data)
        self.assertEqual((pkg.name, pkg.version, pkg.kind, pkg.abi, pkg.entry, pkg.load_addr, pkg.image),
                         ("Vibration Lab", 3, package.KIND_APP, 1, 0, package.APP_BASE, image))

    def test_utf8_name_fits(self):
        self.assertEqual(package.parse(package.pack(b"\0" * 4, name="Вибро", version=1)).name, "Вибро")

    def test_bad_names_rejected(self):
        for name in ("Я" * 12, ""):
            with self.assertRaises(ValueError):
                package.pack(b"\0" * 4, name=name, version=1)

    def test_corrupt_rejected(self):
        data = package.pack(b"\1" * 64, name="x", version=1)
        flipped = data[:-1] + bytes([data[-1] ^ 1])
        for broken in (b"XXXX" + data[4:], data[:-1], flipped, data[:package.HEADER_SIZE - 1]):
            with self.assertRaises(ValueError):
                package.parse(broken)

    def test_entry_size_and_address_bounds(self):
        with self.assertRaises(ValueError):
            package.pack(b"\0" * 16, name="x", version=1, entry=16)
        with self.assertRaises(ValueError):
            package.pack(b"\0" * (package.APP_SIZE + 1), name="x", version=1)
        with self.assertRaises(ValueError):
            package.pack(b"\0" * 16, name="x", version=1, load_addr=package.APP_BASE + 4)

    def test_cli_pack_info(self):
        # BIN во временном каталоге → cli.main(["pack", BIN, "--name", "hello", "--version", "2", "--output", OUT]) == 0;
        # cli.main(["info", OUT]) == 0, stdout JSON: name == "hello", version == 2, image_size == len(BIN)
        ...
```

- [ ] **Step 2: Run to verify failure**

Run: `uv run --locked python -m unittest tests.test_package -v`
Expected: FAIL — `ImportError: cannot import name 'package'`.

- [ ] **Step 3: Implement `package.py` and `cli.py`**

`struct.Struct("<4sHBB24sIIIII12s")`. Длина имени проверяется в байтах UTF-8 (1..=23). Для `KIND_APP` обязательно `load_addr == APP_BASE` и `len(image) <= APP_SIZE`; всегда `entry < len(image)`. `parse` проверяет магию, `abi == ABI`, `image_size == len(data) - HEADER_SIZE`, CRC. CLI — argparse с подкомандами.

- [ ] **Step 4: Run to verify pass**

Run: `uv run --locked python -m unittest discover -s tests -v`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/wp2apps/package.py src/wp2apps/cli.py tests/test_package.py
git commit -m "feat: app package format with pack/info CLI"
```

---

### Task 3: Rust SDK и приложение hello

**Files:**
- Create: `sdk/Cargo.toml` (workspace: `cmf-wp2-app`, `examples/hello`, `examples/vibration-lab`; профиль release: `opt-level = "s"`, `panic = "abort"`, `lto = true`)
- Create: `sdk/.cargo/config.toml` (`[build] target = "thumbv7em-none-eabi"`)
- Create: `sdk/cmf-wp2-app/{Cargo.toml,build.rs,app.ld,src/lib.rs}`
- Create: `sdk/examples/hello/{Cargo.toml,src/main.rs}`
- Create: `scripts/build-app` — `scripts/build-app <example> <version>` → `target/<example>.wp2app` в корне репозитория
- Test: `tests/test_sdk.py` (пропускается без `cargo`/`llvm-objcopy`)

**Interfaces:**
- Consumes: `APP_BASE`, `APP_SIZE` (задача 1) — в `app.ld`; `wp2apps pack` (задача 2).
- Produces (ABI 1, крейт `cmf_wp2_app`):

```rust
#[repr(C)]
pub struct Event { pub kind: u8, pub _pad: u8, pub x: i16, pub y: i16, pub delta: i16 }
pub const TOUCH_DOWN: u8 = 1; pub const TOUCH_UP: u8 = 2; pub const CROWN: u8 = 3;
pub const BUTTON: u8 = 4; pub const PAUSE: u8 = 5; pub const RESUME: u8 = 6;

#[repr(C)]
pub struct Api {
    pub abi: u32,
    pub width: u16,
    pub height: u16,
    pub fill_rect: extern "C" fn(x: i16, y: i16, w: u16, h: u16, rgb565: u16),
    pub draw_text: extern "C" fn(x: i16, y: i16, text: *const u8, len: usize, rgb565: u16),
    pub present: extern "C" fn(),
    pub next_event: extern "C" fn(event: *mut Event, timeout_ms: u32) -> bool,
    pub motor: extern "C" fn(duty_permille: u16, freq_hz: u16) -> i32,
    pub millis: extern "C" fn() -> u32,
    pub exit: extern "C" fn(code: i32) -> !,
}
pub const ABI: u32 = 1;
```

  - `cmf_wp2_app::entry!(path)` создаёт `#[no_mangle] #[link_section = ".entry"] extern "C" fn _start(api: &'static Api) -> i32`: если `api.abi != ABI` — возвращает `-2`, иначе сохраняет `api` в `static` и вызывает `path(api) -> i32`.
  - Безопасные обёртки — методы `Api`: `rect`, `text(&str)`, `show`, `event(timeout_ms) -> Option<Event>`, `vibrate(duty, freq) -> i32`, `now`.
  - `#[panic_handler]`: `motor(0, 0)`, затем `exit(-1)`.
  - `app.ld`: `MEMORY { APP : ORIGIN = APP_BASE, LENGTH = APP_SIZE }`; `.entry` — первая секция (поэтому `entry = 0`), затем `.text`, `.rodata`, `.data`, `.bss`, все внутри образа (`.bss` не `NOLOAD`, загрузчику не нужно обнулять память). `build.rs` кладёт `app.ld` в `OUT_DIR` и добавляет `-Tapp.ld`.
  - Контракт для загрузчика (план 2): `motor` вне `0..=1000` возвращает `-1`; загрузчик сам выключает мотор через 2000 мс непрерывной работы. `next_event` блокируется до события или таймаута (возвращает `false` по таймауту); на паузе отдаёт `PAUSE`, при возврате — `RESUME`. На `BUTTON` приложение должно вернуть управление.

- [ ] **Step 1: Write the failing test**

```python
@unittest.skipUnless(shutil.which("cargo") and shutil.which("llvm-objcopy"), "needs cargo and llvm-objcopy")
class SdkTest(unittest.TestCase):
    def test_hello_builds_and_packs(self):
        root = Path(__file__).resolve().parents[1]
        subprocess.run([str(root / "scripts" / "build-app"), "hello", "1"], check=True)
        pkg = package.parse((root / "target" / "hello.wp2app").read_bytes())
        self.assertEqual((pkg.name, pkg.entry, pkg.load_addr), ("hello", 0, package.APP_BASE))
        self.assertTrue(0 < len(pkg.image) < 16 * 1024)
```

- [ ] **Step 2: Run to verify failure**

Run: `rustup target add thumbv7em-none-eabi && nix-shell -p llvm --run 'uv run --locked python -m unittest tests.test_sdk -v'`
Expected: FAIL — `scripts/build-app` отсутствует.

- [ ] **Step 3: Implement the crate, `hello`, `scripts/build-app`**

`hello`: заливает экран чёрным, пишет `hello` по центру, `show`; ждёт события; на `CROWN` меняет цвет фона; на `BUTTON` возвращает `0`. `scripts/build-app`: `cargo build --release -p <example>` в `sdk/`, `llvm-objcopy -O binary`, `uv run wp2apps pack … --name <example>`.

- [ ] **Step 4: Run to verify pass**

Run: `nix-shell -p llvm --run 'uv run --locked python -m unittest discover -s tests -v'`
Expected: PASS; `nix-shell -p llvm --run 'llvm-objdump -d sdk/target/thumbv7em-none-eabi/release/hello'` показывает `_start` по адресу `APP_BASE`.

- [ ] **Step 5: Commit**

```bash
git add sdk scripts/build-app tests/test_sdk.py
git commit -m "feat(sdk): Rust SDK (ABI 1) with hello example"
```

---

### Task 4: Лаборатория вибрации (логика + экран)

**Files:**
- Create: `sdk/examples/vibration-lab/{Cargo.toml,src/main.rs,src/lab.rs}`
- Test: `#[cfg(test)]` в `sdk/examples/vibration-lab/src/lab.rs`

**Interfaces:**
- Consumes: `cmf_wp2_app::{Api, Event, entry!, CROWN, BUTTON, TOUCH_DOWN, PAUSE, RESUME}` (задача 3); тип мотора и стоковая частота из `docs/recon.md`.
- Produces (`lab.rs`, без зависимости от SDK, чтобы тестироваться на хосте):

```rust
#[derive(Clone, Copy, PartialEq, Debug)] pub enum Pattern { Continuous, Tick, DoubleTick }
#[derive(Clone, Copy, PartialEq, Debug)] pub enum Field { Strength, Frequency, Pattern, Pulse }
#[derive(Clone, Copy, PartialEq, Debug)] pub struct Step { pub duty_permille: u16, pub freq_hz: u16, pub ms: u16 }
#[derive(Clone, Copy, PartialEq, Debug)] pub struct Settings { pub duty_permille: u16, pub freq_hz: u16, pub pattern: Pattern, pub pulse_ms: u16 }
impl Default for Settings { /* 200, стоковая частота, Tick, 20 */ }
impl Settings {
    pub fn adjust(&mut self, field: Field, clicks: i16);
    pub fn steps(&self) -> ([Step; 3], usize);
}
pub struct Player { /* шаги, индекс, активен */ }
impl Player {
    pub fn start(settings: &Settings) -> Self;
    pub fn next(&mut self) -> Option<Step>;
    pub fn pause(&mut self) -> Step;
}
```

Значения: сила `50..=1000` шаг 50 (по умолчанию 200); частота `100..=400` Гц шаг 25 (по умолчанию — стоковая; если мотор ERM, поле `Frequency` не показывается и `adjust` его не меняет); импульс `5..=100` мс шаг 5 (по умолчанию 20). Рисунки: `Continuous` = один шаг 1000 мс; `Tick` = один шаг `pulse_ms`; `DoubleTick` = `pulse_ms`, пауза 80 мс (`duty 0`), `pulse_ms`. `Pattern` при `adjust` переключается по кругу.

- [ ] **Step 1: Write the failing tests**

```rust
#[test] fn strength_clamps() {
    let mut s = Settings::default();
    s.adjust(Field::Strength, 1000);
    assert_eq!(s.duty_permille, 1000);
    s.adjust(Field::Strength, -1000);
    assert_eq!(s.duty_permille, 50);
}
#[test] fn pulse_clamps() {
    let mut s = Settings::default();
    s.adjust(Field::Pulse, -100);
    assert_eq!(s.pulse_ms, 5);
    s.adjust(Field::Pulse, 100);
    assert_eq!(s.pulse_ms, 100);
}
#[test] fn double_tick_steps() {
    let s = Settings { duty_permille: 300, freq_hz: 200, pattern: Pattern::DoubleTick, pulse_ms: 15 };
    let (steps, n) = s.steps();
    assert_eq!(&steps[..n], &[Step { duty_permille: 300, freq_hz: 200, ms: 15 },
                              Step { duty_permille: 0, freq_hz: 200, ms: 80 },
                              Step { duty_permille: 300, freq_hz: 200, ms: 15 }]);
}
#[test] fn no_step_exceeds_limits() {
    for pattern in [Pattern::Continuous, Pattern::Tick, Pattern::DoubleTick] {
        let mut s = Settings { pattern, ..Settings::default() };
        s.adjust(Field::Strength, i16::MAX);
        s.adjust(Field::Pulse, i16::MAX);
        let (steps, n) = s.steps();
        assert!(steps[..n].iter().all(|st| st.duty_permille <= 1000 && st.ms <= 2000));
    }
}
#[test] fn pattern_cycles() {
    let mut s = Settings { pattern: Pattern::DoubleTick, ..Settings::default() };
    s.adjust(Field::Pattern, 1);
    assert_eq!(s.pattern, Pattern::Continuous);
}
#[test] fn pause_stops_playback() {
    let s = Settings { duty_permille: 300, freq_hz: 200, pattern: Pattern::DoubleTick, pulse_ms: 15 };
    let mut p = Player::start(&s);
    assert!(p.next().is_some());
    assert_eq!(p.pause().duty_permille, 0);
    assert_eq!(p.next(), None);
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cd sdk && cargo test -p vibration-lab --target aarch64-apple-darwin`
Expected: FAIL — не компилируется (`Settings` не определён).

- [ ] **Step 3: Implement `lab.rs`**

Без аллокаций, совместим с `no_std`. `main.rs` — `#![cfg_attr(target_os = "none", no_std, no_main)]`; на хосте собирается только `lab` с тестами.

- [ ] **Step 4: Run to verify pass**

Run: `cd sdk && cargo test -p vibration-lab --target aarch64-apple-darwin`
Expected: PASS (6 tests).

- [ ] **Step 5: Implement the screen in `main.rs`**

Четыре строки (`Strength 20%`, `Freq 200Hz`, `Pattern Tick`, `Pulse 20ms`) и кнопка `PLAY` снизу; подписи латиницей, если в стоковом шрифте нет кириллицы (`docs/recon.md`). Тап по строке выделяет её, колёсико — `adjust` для выделенной, тап по `PLAY` проигрывает `Player`: на каждый шаг `vibrate(duty, freq)`, затем `event(step.ms)`; пришёл `PAUSE` → `vibrate` с `Player::pause()` и проигрывание прекращается. `BUTTON` → мотор выкл, `main` возвращает `0`. Перерисовка только после изменения.

- [ ] **Step 6: Verify the device build**

Run: `nix-shell -p llvm --run 'scripts/build-app vibration-lab 1' && uv run wp2apps info target/vibration-lab.wp2app`
Expected: JSON с `"name": "vibration-lab"` и `image_size` меньше `APP_SIZE`.

- [ ] **Step 7: Commit**

```bash
git add sdk/examples/vibration-lab sdk/Cargo.toml
git commit -m "feat(sdk): vibration lab example app"
```

---

### Task 5: Лог мотора в эмуляторе (PR в `cmf-watch-utils`)

Работа идёт в **`cmf-watch-utils`**, в отдельном worktree от `main` на ветке `emu-motor-log`, и оформляется PR'ом в `arnav-kr/cmf-watch-utils`. Там действуют его `AGENTS.md` (комментарии в коде только по запросу, стабильный вывод CLI, `pnpm build` для документации).

**Files (в `cmf-watch-utils`):**
- Modify: `emulator/hw/ats3089c.c` (запись PWM-регистров канала мотора → строка в файл из `QEMU_WP2_MOTOR`)
- Modify: `src/cmf_watch_utils/emu/vm.py` (`MOTOR=output / "motor.log"` в `runtime_env`)
- Modify: `src/cmf_watch_utils/emu/watch_window.tcl` (строка состояния мотора под экраном)
- Modify: `site/src/content/docs/emulator/first-boot.md` (одна фраза про `motor.log`)
- Test: `tests/test_emulator.py`

**Files (в `wp2-apps`, после слияния PR):**
- Modify: `pyproject.toml`, `uv.lock` — `rev` на коммит слияния.

**Interfaces:**
- Consumes: PWM-канал мотора, регистры и формулы скважности/частоты из `docs/recon.md`.
- Produces: `runs/<run>/motor.log`, по строке на каждое изменение: `"<ms> <duty_permille> <freq_hz>\n"`, `ms` — виртуальное время гостя, `duty 0` = выкл. Тот же формат план 2 использует для проверки API `motor`.

- [ ] **Step 1: Write the failing test** (в `cmf-watch-utils/tests/test_emulator.py`)

```python
def test_run_exports_motor_log_path(self):
    # по образцу соседних тестов vm: собрать env для emu run с временным output
    self.assertEqual(env["QEMU_WP2_MOTOR"], str(output / "motor.log"))
```

- [ ] **Step 2: Run to verify failure**

Run: `uv run --locked python -m unittest tests.test_emulator -v`
Expected: FAIL — `KeyError: 'QEMU_WP2_MOTOR'`.

- [ ] **Step 3: Implement env wiring, the C hook and the viewer line**

В C: открыть файл из `QEMU_WP2_MOTOR` один раз (как `QEMU_WP2_FRAME`); писать строку только при изменении пары (duty, freq). Viewer: в `refresh` читать последнюю строку, показывать `Motor: off` или `Motor: 20% 200 Hz`.

- [ ] **Step 4: Run to verify pass**

Run: `uv run --locked python -m unittest discover -s tests -v` и `cd site && pnpm build`
Expected: PASS, сайт собирается.

- [ ] **Step 5: Rebuild QEMU and check against stock**

Run: `nix-shell -p pkg-config ninja glib pixman dtc --run 'uv run --locked python emulator/build.py --component qemu --audio coreaudio'`, распаковать архив из `emulator/dist/` в `runtime/` основного checkout'а `cmf-watch-utils`, затем из `wp2-apps`: `scripts/emu`. В стоковых настройках вибрации переключить уровни.
Expected: в `runs/<run>/motor.log` на каждый уровень — строки с разной скважностью, совпадающие с `docs/recon.md`; в окне видна строка `Motor: …`. Наблюдения дописать в `wp2-apps/docs/recon.md`.

- [ ] **Step 6: Commit and open the PR** — только после «да» владельца на публикацию PR.

```bash
git add emulator/hw/ats3089c.c src/cmf_watch_utils/emu/vm.py src/cmf_watch_utils/emu/watch_window.tcl site/src/content/docs/emulator/first-boot.md tests/test_emulator.py
git commit -m "Log motor PWM state in the emulator and show it in the viewer"
gh pr create --repo arnav-kr/cmf-watch-utils --base main --head emu-motor-log
```

- [ ] **Step 7: После слияния — поднять `rev` в `wp2-apps`**, `uv lock`, `uv run --locked python -m unittest discover -s tests -v`, коммит `build: bump cmf-watch-utils for motor log`.

---

## После этого плана

План 2 пишется по результатам задачи 1 и закрывает этапы 2–4 спеки: заглушка в `app.bin`, загрузчик на NAND с таблицей `Api` (ABI 1 из задачи 3), пункт «Apps», свой BLE-канал установки (`wp2apps install`, в эмуляторе через loopback H4), пауза поверх стоковых событий, запуск `hello` и `vibration-lab` в эмуляторе. Этап 5 (физические часы) — только с разрешения владельца.
