# Workout App Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A watch app `workout` that walks through the owner's three-day split (sets, rest timer, rotation,
history), fed from the owner's Markdown notes by `wp2apps workout push`, plus the loader changes it needs (the app
owns "back"; the app survives the screen going off).

**Architecture:** One binary format for the program, produced by Python (`src/wp2apps/workout.py`) and parsed by
the watch (`sdk/examples/workout/src/model.rs`); it travels in ≤ 128 B `"w1"` messages and is stored as-is in the
app's 4 KiB blob next to progress, rotation and history. The watch app is an LVGL app (ABI 1.3) in the stock look,
its logic in host-tested `model.rs`/`proto.rs`, its screens in `screen.rs`. The loader gains ABI 1.3 (`own_back`)
and keeps the app alive across screen-off (loader first, `app.bin` patch + OTA only if the loader cannot).

**Tech Stack:** Rust `no_std` (`thumbv7em-none-eabi`), LVGL via `cmf_wp2_app::lv`, Python 3.12 (uv, unittest,
asyncio), the QEMU emulator harness `tests/e2e/harness.py`.

**Spec:** `docs/specs/2026-11-13-workout-app-design.md`

## Global Constraints

- AGENTS.md rule 1: no writes to the physical watch (install, OTA) without the owner's explicit yes at that time.
  Everything is proven in the emulator first; the owner flashes once at the end.
- AGENTS.md rule 2: the owner's real program, notes, MAC/UUID and session keys never go into commits or logs.
  Test fixtures use made-up exercises.
- Rule 5: addresses are for JX402 `1.0.0.73` only.
- Stock look only: `stock::font`, `stock::theme`, `stock::focus_page`/`focus_row`, `stock::arc`; colours from
  `cmf_wp2_app::stock` (`ACCENT`, `GREY`, `ROW_BG`, `WHITE`, `BLACK`).
- UI strings (exact): "Тренировка", "Нет программы", "Следующая", "N упражнений" (Russian plural:
  упражнение/упражнения/упражнений), "Подход {i} из {n}", "Сделал", "Отдых", "Дальше: подход {i} из {n}",
  "Пропустить", "Завершить", "Готово", "Выйти", "Отмена", "Завершить тренировку?".
- Rest default 90 s. Auto-finish after 3 h untouched. A workout with ≥ 1 set done counts and advances the
  rotation; 0 sets: dropped, rotation unchanged.
- Program limits: ≤ 7 days, ≤ 12 exercises per day, names ≤ 32 B, weight text ≤ 16 B (UTF-8, cut on a char
  boundary), encoded program ≤ 2048 B. History ring: 64 entries.
- `cargo test --target aarch64-apple-darwin` for host tests; `nix-shell -p llvm --run 'scripts/build-app workout 1'`
  to build; e2e under `WP2_E2E=1 nix-shell -p gdb llvm --run '…'`, run in a pane split off the owner's (CLAUDE.md).

## Review Focus

1. **The press that wakes the screen** (button or tap on a dark/AOD panel) must not reach the app as `BUTTON` or a
   tap: otherwise waking opens the exit dialog or presses "Сделал". Test in Task 3.
2. **Rest ends while the panel is off or under AOD**: the end-of-rest buzz still fires. Test in Task 3.
3. **No wall clock** (`api.time()` is `None`): rest still counts and buzzes; history dates are 0. Test in Task 4.
4. **A new program while a workout is in progress**, with fewer days/exercises: the workout is finished into the
   history first, no index from the old program survives. Test in Task 4.
5. **Corrupt, foreign or truncated blob / transfer** (old magic, `program_len` past the end, missing chunk, bad
   CRC): treated as empty / dropped, never a panic. Tests in Tasks 4 and 5.

---

### Task 1: Font probe (gate)

Find out whether the system font `F32M` draws every Cyrillic letter, and which stock font draws large digits,
`–` and `:`. Throwaway probe, findings committed.

**Files:**
- Create (not committed): `sdk/examples/font-probe/` (copy `lv-demo`'s Cargo.toml shape; add to workspace members
  only locally)
- Create: `docs/workout.md` (section "Fonts")

- [ ] **Step 1:** Write a probe app: for each `StockFont` (`F32M`, `F36M`, `F44L`, `F48M`, `F36N`) one label with
  `"АБВГДЕЁЖЗИЙКЛМНОПРСТУФХЦЧШЩЪЫЬЭЮЯ абвгдеёжзийклмнопрстуфхцчшщъыьэюя 0123456789 – : ×"`, wrapped, one font per
  screen, crown switches screens.
- [ ] **Step 2:** Build, install into the emulator (`scripts/emu`, `wp2apps install --hci-port`), screenshot each
  screen into `runs/font-probe/`.
- [ ] **Step 3:** Write `docs/workout.md` "Fonts": per font, missing glyphs (or "complete"), and the chosen font for
  body text and for the big numbers.
- [ ] **Step 4: Gate.** If `F32M` misses any Cyrillic letter, stop and bring the owner the screenshots and options
  (own font on NAND, Latin names). Otherwise continue.
- [ ] **Step 5:** Commit `docs/workout.md` only: `docs(workout): stock font coverage`.

### Task 2: Loader ABI 1.3: the app owns "back"

**Files:**
- Modify: `fw/wp2-core/src/events.rs` (escape rule), `fw/wp2-loader/src/main.rs:38,171` (`ESCAPE_MS` use),
  `fw/wp2-loader/src/app.rs` (`API` table, `next_event`), `sdk/cmf-wp2-app/src/lib.rs` (`Api`, `ABI_MINOR`)
- Create: `docs/abi-1.3.md`
- Modify: `docs/app-guide.md` (`BUTTON` row, System table)
- Test: `fw/wp2-core/src/events.rs` (host test), `tests/test_back_e2e.py` (new test)

**Interfaces:**
- Produces: `Api.own_back: extern "C" fn(on: bool)` appended after `lv_submit`; `cmf_wp2_app::ABI_MINOR = 3`;
  `Api::own_back(&self, on: bool)` (no-op when `abi_minor < 3`, so 1.2 apps still run); loader table
  `abi_minor: 3`.
- Produces: `wp2_core::events::escape_due(owned: bool, button_at: u32, consumed: bool, now: u32) -> bool`.

Escape rule: not owned → as today (force stop `ESCAPE_MS` = 1000 ms after `BUTTON`). Owned → a `BUTTON` the app
pops from the queue (`next_event` returns it) disarms the escape; a `BUTTON` still unconsumed after 1000 ms (hung
app) force-stops. So a dialog that waits for a second press never trips it.

- [ ] **Step 1: Failing host test** in `events.rs`:

```rust
#[test]
fn escape_rule() {
    assert!(escape_due(false, 1, false, 1001));   // not owned: 1000 ms after the press
    assert!(!escape_due(false, 1, false, 1000));
    assert!(!escape_due(true, 1, true, 5000));    // owned and consumed: never
    assert!(escape_due(true, 1, false, 1001));    // owned, hung: still killable
    assert!(!escape_due(true, 0, false, 9999));   // no press
}
```

- [ ] **Step 2:** `(cd fw && cargo test -p wp2-core --target aarch64-apple-darwin escape_rule)` → FAIL (undefined).
- [ ] **Step 3:** Implement `escape_due`; in the loader keep `OWN_BACK: AtomicBool` (reset on `launch`) and
  `BUTTON_CONSUMED: AtomicBool` (set by `next_event` when it returns a `BUTTON`, cleared when `key()` arms
  `button_at`); `tick` calls `escape_due`. Add `own_back` to the table and the SDK.
- [ ] **Step 4:** Host tests pass: `(cd fw && cargo test -p wp2-core --target aarch64-apple-darwin)` and
  `(cd sdk && cargo test --target aarch64-apple-darwin)`.
- [ ] **Step 5: e2e** `tests/test_back_e2e.py::OwnBackTest`: a tiny app in `sdk/examples/api-demo` mode (or a new
  `back-owner` example) calls `own_back(true)`, counts `BUTTON`s in a `#[no_mangle] static mut STATS`, exits on the
  third. Press the button twice 1.5 s apart → app still running, count 2; third press → app exits with code 0.
  A second variant that never calls `event()` after `own_back(true)` is force-stopped 1 s after a press.
- [ ] **Step 6:** `docs/abi-1.3.md` (the call, the escape rule, compatibility), `docs/app-guide.md` rows.
- [ ] **Step 7:** Commit `feat(loader): ABI 1.3 own_back, the app handles the button`.

### Task 3: The app survives the screen going off

Research task with a decision gate; the acceptance tests are fixed, the mechanism is not.

**Files:**
- Modify: `fw/wp2-loader/src/*.rs` (preferred) or `src/wp2apps/firmware.py` (stock patch, fallback)
- Modify: `docs/recon.md` (new section "Screen-off and the app view"), `docs/app-guide.md:303` (lifetime note)
- Test: `tests/test_back_e2e.py::ScreenOffTest`

- [ ] **Step 1: Reproduce.** In the emulator open an app, let auto-sleep blank the panel (set a short timeout via
  ux2sys `0x07`), log the view messages to view 240 (gdb breakpoint on `loader_main`) and the view-stack calls
  (`docs/recon.md` "Back navigation"). Record who sends `DELETE`, and what a wake shows.
- [ ] **Step 2: Compare with a stock view that survives a blank** (stopwatch or a sport view): find the field or
  call that keeps it on the stack and brings it back on wake. Record addresses in `docs/recon.md`.
- [ ] **Step 3: Write the failing e2e test** `ScreenOffTest.test_app_survives_blank`: open `herdr-agents` (or the
  Task 2 test app), blank the panel, wake with the button → `RUNNING` byte (`STUB_RAM + 6`) is still the app's slot,
  the frame shows the app, the app's `BUTTON` counter did not move (Review Focus 1). Second test
  `test_buzz_while_dark`: the app starts a 3 s timer, the panel blanks, `emu.motor()` shows the buzz at 3 s
  (Review Focus 2).
- [ ] **Step 4: Gate.** Fix in the loader if Step 2 found a per-view mechanism the loader can use. If it needs the
  stock code changed: add a `wp2apps firmware` option for it with a `tests/test_firmware.py` case, prove it with
  `scripts/emu-ota` (`docs/ota.md`), and tell the owner that this part needs an OTA flash (they agreed in principle;
  the flash itself waits for their yes). If both fail: stop and report; the spec's fallback is a
  "screen on for the whole workout" setting.
- [ ] **Step 5:** Both e2e tests pass; the existing `tests/test_back_e2e.py` and `tests/test_herdr_e2e.py` still pass.
- [ ] **Step 6:** Commit `feat(loader): keep the app across screen-off` (or `feat(firmware): …`).

### Task 4: Watch model: program, workout, rotation, history, blob

**Files:**
- Create: `sdk/examples/workout/{Cargo.toml,icon.svg,src/main.rs,src/model.rs}` (Cargo.toml and main.rs shaped like
  `herdr-agents`; main is an empty host `fn main` for now); add `examples/workout` to `sdk/Cargo.toml` members
- Create: `tests/fixtures/workout/Day 01.md`, `Day 02.md` (made-up program, includes a `отдых 2:00`, a no-weight
  line and a 40-byte Cyrillic name), `tests/fixtures/workout/program.bin` (written by Task 6's encoder; until then
  the Rust tests build their bytes with a local `enc()` helper matching the format below)

**Program format** (little-endian; the version is not inside, it is the CRC-32/IEEE of these bytes,
`zlib.crc32`):

```
program  := ndays:u8 day*
day      := name_len:u8 name ex_count:u8 exercise*
exercise := name_len:u8 name sets:u8 reps_lo:u8 reps_hi:u8 rest_s:u16 weight_len:u8 weight
```

**Blob format** (`STORE_MAX` 4096):

```
"W1" version:u32 program_len:u16 program
last_day:u8 (0xFF none)
active:u8 (0/1) day:u8 started:u32 touched:u32 rest_until:u32 rest_ex:u8 done[12]:u8
hist_n:u8 hist_head:u8 entry[64]   entry := start:u32 dur_s:u16 day:u8 done:u8 planned:u8
```

Times are seconds since 2000-01-01 00:00 local (`epoch_s`), or `millis/1000` since boot when `time()` is `None`.

**Interfaces (produces, `model.rs`):**

```rust
pub const PROGRAM_MAX: usize = 2048; pub const DAYS_MAX: usize = 7; pub const EX_MAX: usize = 12;
pub const NAME_MAX: usize = 32; pub const WEIGHT_MAX: usize = 16; pub const HISTORY_MAX: usize = 64;
pub const STALE_S: u32 = 3 * 3600; pub const REST_DEFAULT_S: u16 = 90;
pub fn epoch_s(t: &cmf_wp2_app::Time) -> u32;
pub fn crc32(bytes: &[u8]) -> u32;
pub struct Exercise<'a> { pub name: &'a str, pub sets: u8, pub reps: (u8, u8), pub rest_s: u16, pub weight: &'a str }
pub struct Program { /* bytes + len + version */ }
impl Program {
    pub fn parse(bytes: &[u8]) -> Option<Program>;          // validates every length and limit
    pub fn version(&self) -> u32; pub fn bytes(&self) -> &[u8];
    pub fn days(&self) -> usize; pub fn day_name(&self, d: usize) -> &str;
    pub fn exercises(&self, d: usize) -> usize; pub fn exercise(&self, d: usize, i: usize) -> Exercise<'_>;
}
pub struct Workout { pub day: u8, pub started: u32, pub touched: u32, pub rest_until: u32, pub rest_ex: u8, pub done: [u8; EX_MAX] }
pub struct Entry { pub start: u32, pub dur_s: u16, pub day: u8, pub done: u8, pub planned: u8 }
pub enum Next { Set { ex: usize, set: u8 }, Finished }
pub struct Model { pub program: Option<Program>, pub last_day: Option<u8>, pub workout: Option<Workout>, /* ring */ }
impl Model {
    pub const fn new() -> Model;
    pub fn load(&mut self, blob: &[u8]);                  // anything unparsable → Model::new()
    pub fn save(&self, buf: &mut [u8]) -> usize;
    pub fn next_day(&self) -> usize;                       // (last_day + 1) % days, 0 without history
    pub fn start(&mut self, day: usize, now: u32);
    pub fn done_set(&mut self, ex: usize, now: u32);       // done[ex] += 1, rest_until = now + rest_s, rest_ex = ex
    pub fn after_rest(&self) -> Next;                      // rest_ex if it has open sets, else the first exercise
                                                           // from the top with open sets, else Finished
    pub fn finish(&mut self, now: u32);                    // ≥ 1 set → history + last_day; 0 → dropped
    pub fn expire(&mut self, now: u32);                    // touched + STALE_S < now → finish(touched)
    pub fn replace_program(&mut self, p: Program, now: u32); // finish first, then swap
    pub fn history(&self) -> impl Iterator<Item = Entry> + '_; // newest first
}
```

- [ ] **Step 1: Failing tests** in `model.rs` (`#[cfg(test)]`), one per behaviour:
  - `parse_roundtrip`: `enc()` of 2 days → `Program::parse` → names, sets, reps `(5, 8)`, `rest_s` 120 and 90,
    weight `"90 кг"` and `""`; `version()` = `crc32(bytes)`.
  - `parse_rejects`: truncated bytes, `ndays` 8, `ex_count` 13, a name of 33 B, invalid UTF-8 → `None`.
  - `rotation`: `next_day()` 0 → after `finish` of day 0 with 1 set → 1 → after day 2 of 3 → 0.
  - `zero_sets_dropped`: `start(1)`, `finish` → history empty, `next_day()` unchanged.
  - `after_rest_order`: day with 3 exercises (sets 2, 1, 2): `done_set(0)` → `Set{ex:0,set:1}`; `done_set(0)` →
    `Set{ex:1,set:0}`; `done_set(2)`, `done_set(2)` → `Set{ex:1,set:0}`; `done_set(1)` → `Finished`.
  - `expire_after_3h`: `touched` = 1000, `expire(1000 + STALE_S)` keeps it, `expire(1001 + STALE_S)` finishes with
    `dur_s` = `touched - started`.
  - `replace_program_finishes_first` (Review Focus 4): workout on day 2 with sets done, replace with a 1-day
    program → history has the old workout, `workout` is `None`, `next_day()` is 0 and in range.
  - `blob_roundtrip`: program + workout + 70 finished workouts → `save` → `load` → equal; history length 64,
    newest first; `save` length ≤ 4096 with a 2048 B program.
  - `blob_garbage` (Review Focus 5): `b"H1…"`, `program_len` past the end, a valid header with a bad program, empty
    → `Model::new()` state, no panic.
  - `no_clock` (Review Focus 3): times from boot seconds (small values) work the same; `epoch_s` of
    2026-11-13 10:00:00 = 847_879_200.
- [ ] **Step 2:** `(cd sdk && cargo test -p workout --target aarch64-apple-darwin)` → FAIL.
- [ ] **Step 3:** Implement `model.rs`. `Program` keeps its bytes (`[u8; PROGRAM_MAX]`) and parses on access; the
  `Model` lives in a `static mut` in the device build (≈ 3 KB, off the 8 KiB stack).
- [ ] **Step 4:** Tests pass.
- [ ] **Step 5:** Commit `feat(workout): watch model, program and blob format`.

### Task 5: Watch protocol `"w1"`

**Files:**
- Create: `sdk/examples/workout/src/proto.rs`
- Create: `docs/workout.md` section "Protocol" (the table below, the plain-data exception from the spec)

**Wire** (every message `"w1"`, kind, then for host → app a `seq` byte; ≤ 128 B):

| Kind | Dir | Body |
|---|---|---|
| `B` | host → app | `total:u16` (program bytes) |
| `C` | host → app | `offset:u16`, ≤ 120 data bytes |
| `E` | host → app | `crc32:u32` (= version) |
| `Q` | host → app | – (send the history) |
| `R` | app → host | `version:u32` (0 = no program) |
| `A` | app → host | `version:u32` (stored) |
| `Y` | app → host | `index:u8 count:u8 start:u32 dur_s:u16 day:u8 done:u8 planned:u8`; empty history: `Y 0 0` alone |

`seq` counts host messages mod 256 and restarts at `B`. A gap, a `C` whose offset is not the bytes received so far,
`total` > 2048, or an `E` whose CRC does not match or whose bytes `Program::parse` rejects → the transfer is
dropped and the app sends `R` again.

**Interfaces (produces, `proto.rs`):**

```rust
pub enum Got<'a> { Nothing, Program(&'a [u8]), Resync, History }
pub struct Transfer { /* buf [u8; PROGRAM_MAX], got, total, next_seq */ }
impl Transfer { pub const fn new() -> Transfer; pub fn apply(&mut self, msg: &[u8]) -> Got<'_>; }
pub fn request(version: u32) -> [u8; 7];
pub fn ack(version: u32) -> [u8; 7];
pub fn history_msg(index: u8, count: u8, e: &Entry) -> [u8; 14];
```

- [ ] **Step 1: Failing tests:** `transfer_ok` (B, three C, E → `Program` bytes equal, CRC matches);
  `transfer_gap` (skip a seq → `Resync`, then a fresh B works); `transfer_bad_crc` → `Resync`;
  `transfer_too_big` (`total` 2049) → `Resync`; `foreign_magic` (`h1…`) → `Nothing`; `query` (`w1Q`) → `History`;
  `encodings` (`request(0x01020304)` = `b"w1R\x04\x03\x02\x01"`, `history_msg` layout).
- [ ] **Step 2:** Run → FAIL. **Step 3:** Implement. **Step 4:** Run → PASS.
- [ ] **Step 5:** Commit `feat(workout): w1 protocol on the watch`.

### Task 6: Mac side: parse the notes, encode, push, history

**Files:**
- Create: `src/wp2apps/workout.py`
- Modify: `src/wp2apps/cli.py` (subcommand `workout` with `push` and `history`)
- Test: `tests/test_workout.py`; writes `tests/fixtures/workout/program.bin`, which Task 4's Rust test
  `fixture_decodes` then reads with `include_bytes!` (add that test here)

**Interfaces (produces):**

```python
NAME_MAX, WEIGHT_MAX, PROGRAM_MAX, DAYS_MAX, EX_MAX, REST_DEFAULT = 32, 16, 2048, 7, 12, 90
@dataclass(frozen=True)
class Exercise: name: str; sets: int; reps: tuple[int, int]; weight: str; rest_s: int
@dataclass(frozen=True)
class Day: name: str; exercises: tuple[Exercise, ...]
@dataclass(frozen=True)
class Entry: start: int; dur_s: int; day: int; done: int; planned: int
class ParseError(ValueError): ...            # message "<file>:<line>: <reason>"
def parse_day(text: str, file: str) -> Day
def parse_dir(path: Path) -> list[Day]       # *.md sorted by name; a file without "##" is skipped
def encode(days: list[Day]) -> bytes         # the Task 4 format; ValueError over the limits
def version(program: bytes) -> int           # zlib.crc32
def messages(program: bytes) -> list[bytes]  # B, C…, E with seq 0, 1, …
def decode_entry(msg: bytes) -> Entry | None
async def push(ch: channel.Channel, program: bytes, log) -> None   # waits for R, sends, waits for A
async def history(ch: channel.Channel) -> list[Entry]
def main(args) -> int
```

Line grammar: a list item, optional `[ ]`/`[x]`, then
`<name>[:] <sets>[×xXхХ]<lo>[–-<hi>] [(<weight>)] [отдых <m>:<ss>]`. Day name: the first `##` heading with a
leading `День N:` removed. A list item that does not match → `ParseError` (no default sets: the owner fixes the
note). Lines that are not list items (`&nbsp;`, blank) are ignored. Names and weights are clipped on a UTF-8 char
boundary (reuse `herdr.clip`).

- [ ] **Step 1: Failing tests** in `tests/test_workout.py`:
  - `test_lines`: `"- [x] Жим лёжа: 4×5–8 (90 кг)"` → `Exercise("Жим лёжа", 4, (5, 8), "90 кг", 90)`;
    `"- [ ] Скручивания на полу 3х20"` → `("Скручивания на полу", 3, (20, 20), "", 90)`;
    `"- Тяга: 3x8-10 (50–60 кг) отдых 2:00"` → rest 120; `"- Подтягивания: 4×6–10 (свой вес)"` → weight
    `"свой вес"`.
  - `test_no_sets_is_an_error`: `"- [x] Сгибание на бицепс"` in file `D.md` line 3 → `ParseError` whose message
    starts with `"D.md:3:"`.
  - `test_day_name`: `"## День 1: Жим и присед"` → `"Жим и присед"`; `"## Ноги"` → `"Ноги"`.
  - `test_encode_limits`: 8 days → `ValueError`; a 40-byte Cyrillic name is clipped to ≤ 32 B on a char boundary.
  - `test_messages`: a 300 B program → `B`, 3 × `C` (120, 120, 60 data bytes, offsets 0/120/240), `E` with
    `zlib.crc32`; every message ≤ 128 B; seq 0..4.
  - `test_fixture`: `encode(parse_dir(tests/fixtures/workout))` equals `program.bin` (regenerate with
    `WP2_REGEN=1`).
  - `test_push_flow`: a fake channel (as in `tests/test_herdr.py`) answers `R 0`, receives B/C/E, answers `A ver`
    → `push` returns; answering `R ver` (same) returns without sending.
- [ ] **Step 2:** `uv run --locked python -m unittest tests.test_workout -v` → FAIL.
- [ ] **Step 3:** Implement `workout.py`; CLI:
  `wp2apps workout push DIR (--address A | --hci-port P) [--dry-run]` (dry run prints the parsed days as JSON and
  the encoded size, no Bluetooth) and `wp2apps workout history (--address | --hci-port)` (JSON lines). Both resume
  the cached session key like `herdr`, poll 0.25 s, give up after 120 s without an `R` with a message telling the
  owner to open the app.
- [ ] **Step 4:** Python tests and `(cd sdk && cargo test -p workout --target aarch64-apple-darwin)` (with
  `fixture_decodes`) pass.
- [ ] **Step 5:** `docs/workout.md` "Usage" (push, history, the note format, "start push, then open the app").
- [ ] **Step 6:** Commit `feat(cli): wp2apps workout push and history`.

### Task 7: Watch screens and the main loop

**Files:**
- Create: `sdk/examples/workout/src/screen.rs`
- Modify: `sdk/examples/workout/src/main.rs` (device module like `herdr-agents/src/main.rs`)
- Modify: `sdk/examples/workout/icon.svg`: the owner's GymMate icon (`~/Developer/pet/gymmate-mobile/assets/images/icon.png`,
  a white rounded "Λ" chevron on a blue `#3FA0FF → #0A72E0` gradient with a faint grid), redrawn as vector in the
  bundled style (`docs/app-guide.md` step 4: gradient circle filling the 96 px, white glyph, soft shadow); no grid
  at 96 px. Do not copy the PNG into the repository.

**Interfaces:**
- Consumes: Task 4 `Model`, `Next`, `epoch_s`; Task 5 `Transfer`, `Got`, `request`, `ack`, `history_msg`;
  Task 2 `Api::own_back`.
- Produces (`screen.rs`): `pub enum View { Start, Day, Set { ex: usize }, Rest, Ask, Done }` and
  `pub struct Screen` with `fn build(b, &Model, View, now) -> Result<Screen, LvError>` (rebuilds the page for a
  view) and `fn tick(&mut self, b, &Model, now)` (updates only the clock, the rest ring and its time);
  `fn hit(&self, handle: u16) -> Option<Hit>` with
  `pub enum Hit { Day(usize), Exercise(usize), Finish, Done, Skip, Leave, End, Cancel }`.

Screens, as in the spec and mockups: Start (focus page of days, focus on `next_day()`, "Следующая" on it; with no
program a single "Нет программы" line); Day (title `День {d+1} · {h:mm:ss}`, focus rows: name + `●●○○ · 5–8 · 60 кг`
subtitle, last row "Завершить"); Set (name, "Подход i из n", reps in the big font from Task 1, weight, accent
"Сделал"); Rest (`stock::arc` ring counting down, "Отдых", time, "Дальше: подход i из n", grey "Пропустить");
Ask ("Завершить тренировку?", rows "Выйти", "Завершить", "Отмена"); Done ("Готово", duration, sets done/planned).

Loop rules (`main.rs`):
- On start: `load`, `own_back(true)`, `expire(now)`; open Day if a workout is active, else Start. Send
  `request(version)` once on start and on `RESUME`, and every 3 s while Start is shown.
- `Сделал` → `done_set`, `save`, Rest view, `keep_screen_on(true)`. Rest end (or "Пропустить") → buzz
  (`haptic::DOUBLE`) only when it ended while the app was running, then the view `after_rest()` names; keep the
  screen on 20 s more, then `keep_screen_on(false)`.
- On reopen with `rest_until` already past: the `after_rest()` view, no buzz.
- `BUTTON`: Set/Rest → Day; Day → Ask; Ask → Day; Start/Done → exit (return 0). "Выйти" → return 0 (workout stays
  active and saved). "Завершить" → `finish`, `save`, Done.
- `Got::Program` → `replace_program`, `save`, `api.send(&ack(v))`, rebuild Start. `Got::History` → one
  `history_msg` per entry, newest first. `Got::Resync` → `request` now.
- `PAUSE`/`RESUME` as in herdr-agents (`paused()`), motor off on pause.
- `STATS` (`#[no_mangle] static mut [i32; 4]`) for the e2e test: current view, done sets, buzzes, last ack.

- [ ] **Step 1:** Build: `nix-shell -p llvm --run 'scripts/build-app workout 1'` → `target/workout.wp2app`.
- [ ] **Step 2:** Show it in the emulator viewer (`nix-shell -p tk`, windowed), push the fixture program with
  `wp2apps workout push tests/fixtures/workout --hci-port P`, walk Start → Day → Set → Rest → buzz → next set →
  back → Ask → Выйти → reopen → Day. Screenshots to `runs/workout/`.
- [ ] **Step 3:** Show the owner the screenshots; adjust layout until they say yes (memory: emulator first).
- [ ] **Step 4:** Commit `feat(workout): watch screens and loop`.

### Task 8: End-to-end test in the emulator

**Files:**
- Create: `tests/test_workout_e2e.py` (shape of `tests/test_herdr_e2e.py`; own `WP2_NETSIM_INSTANCE`)

- [ ] **Step 1: Write the tests** (they need `target/{loader,workout}.wp2app`):
  - `test_push_and_walk`: install, start `push` of the fixture in a thread, open the app → `push` returns, STATS
    last ack = version; tap day → tap first exercise → "Сделал" → rest view; after `rest_s` + 1 s `emu.motor()`
    shows the buzz and STATS view = Set, set 2.
  - `test_resume`: after one set, press the button twice (Day → Ask), tap "Выйти" → app exits; reopen → Day view
    with the set dot filled.
  - `test_finish_rotates`: finish via the "Завершить" row → Done; reopen → Start focused on day 2;
    `wp2apps workout history --hci-port` prints one entry with `done` 1.
  - `test_wake_press_ignored` is covered by Task 3's `ScreenOffTest`; here only check that the app is still in
    Rest after a blank and wake.
- [ ] **Step 2:** Run in a pane: `WP2_E2E=1 nix-shell -p gdb llvm --run 'uv run --locked python -m unittest
  tests.test_workout_e2e -v'` → PASS.
- [ ] **Step 3:** Full verification from AGENTS.md (unittest, sdk and fw cargo tests) → PASS.
- [ ] **Step 4:** Commit `test(workout): emulator end-to-end`.

### Task 9: Hardware (owner's yes required)

- [ ] **Step 1:** Ask the owner for the go-ahead, listing what gets written: the loader package (ABI 1.3 + screen-off
  fix), `workout.wp2app`, and an OTA image only if Task 3 needed one.
- [ ] **Step 2:** After the yes: install, `wp2apps workout push <their Gym folder>`; the owner checks AOD survival,
  the wake press and the buzz in a real session. Record results in `docs/workout.md` "Hardware" (no personal data).
