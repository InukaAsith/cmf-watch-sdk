# Workout app

The `workout` watch app and its Mac side. Design: `docs/specs/2026-11-13-workout-app-design.md`; plan:
`docs/plans/2026-11-13-workout-app.md`.

## Fonts

Probed 2026-11-13 in the emulator (a throwaway app, one stock font per screen with `А–Я`, `а–я`, `Ёё`, digits,
`–`, `-`, `:`, `×`, `●○`):

| Font | Cyrillic | Digits and signs |
|---|---|---|
| `F32M` (system) | complete except `Ё`, `ё` | `0–9`, `–`, `-`, `:`; no `×`, `·`, `●`, `○` |
| `F48M` | only `БИРСавгдзийклнрстуъыь` | `0–9`, `-`, `:`; no `–` |
| `F44L` | a few letters | `1–9`, `:`; no `0`, no `–` |
| `F36M` | partial (no `Д`, `Е`, `Ж`, `Ц`, `Ч`, `Щ` …) | `0–9`, `-`, `:`; no `–` |
| `F36N` | none | dot-matrix `0–9`, `:` (the AOD style) |

Choices:

- Every text in `F32M`. The Mac side replaces `Ё`/`ё` with `Е`/`е` when it encodes a program, so the watch never
  gets a letter the font lacks.
- Big numbers (rest time, reps on the set screen) in `F48M`, with an ASCII `-` between reps (`5-8`); `F32M` text
  may use `–`.
- Set dots are drawn as small LVGL circles, not glyphs.

## Usage

The program lives in Markdown notes, one file per day, sorted by name (`Day 01.md`, `Day 02.md`, …):

```markdown
## День 1: Жим и присед

- [x] Жим лёжа: 4×5–8 (90 кг)
- [ ] Тяга штанги в наклоне: 3×8–10 (50–60 кг) отдых 2:00
- [ ] Скручивания на полу 3х20
```

- The day's name is its first `##` heading without a leading `День N:`.
- An exercise is a list item (checkbox or not): `<name>[:] <sets>×<reps>[–<reps>] [(<weight>)] [отдых m:ss]`; `×`,
  `x` and `х` all work. Rest defaults to 90 s. Other lines are ignored.
- A list item that does not match (for example a name without sets) stops `push` with `file:line`.
- Limits: 7 days, 12 exercises per day, names 32 B and weights 32 B (UTF-8, cut on a character), 2048 B in all.
  `Ё`/`ё` become `Е`/`е` (the system font lacks them).

```bash
uv run --locked wp2apps workout push ~/path/to/Gym --dry-run              # parse only, no Bluetooth
uv run --locked wp2apps workout push ~/path/to/Gym --address <UUID>       # or --hci-port P (emulator)
uv run --locked wp2apps workout history --address <UUID>                  # JSON lines, newest first
```

Start `push` or `history`, then open the workout app: the app asks (R) on start, on resume and every 3 s on its
start screen. Both reuse the cached session key like `wp2apps herdr` (no "Paired" popup) and give up after 120 s.
A new program finishes a workout in progress (it goes to the history) and replaces the old one.

## Protocol

Every message: `"w1"`, a kind, then for host → app a `seq` byte; at most 128 B. `seq` counts host messages of a
transfer from `B` (0) on.

| Kind | Direction | Body |
|---|---|---|
| `B` | host → app | `total: u16` program bytes (1..2048) |
| `C` | host → app | `offset: u16`, up to 120 data bytes |
| `E` | host → app | `crc32: u32` of the program (zlib), its version |
| `Q` | host → app | – (send the history); not part of a transfer |
| `R` | app → host | `version: u32` stored (0 = none), `flags: u8` (bit 0: a workout is in progress; a host holds a new program back until it ends, `push` refuses without `--force`). Sent on start, on resume, every 3 s on the start screen and right after a workout ends. Hosts accept a 7-byte `R` (older app) as flags 0 |
| `A` | app → host | `version: u32` just stored |
| `Y` | app → host | `index: u8, count: u8, start: u32, dur_s: u16, day: u8, done: u8, planned: u8`; an empty history is one `Y` with count 0 |

A gap in `seq`, a `C` at another offset than the bytes received so far, more bytes than `total`, or an `E` whose CRC
or program does not check drops the transfer and the app sends `R` again. The program format is in
`docs/plans/2026-11-13-workout-app.md`, Task 4 (`sdk/examples/workout/src/model.rs`, `src/wp2apps/workout.py`).

Plain data: exercise and day names, weights, rest times and the history (dates, durations, counts) travel unencrypted
over a channel whose pairing secret any BLE client can read (`docs/app-guide.md`, section 4). The spec accepts this
for workout data (`docs/specs/2026-11-13-workout-app-design.md`); nothing else personal goes over it. The owner's real
program stays out of commits and logs: the tests use `tests/fixtures/workout/`.

## iPhone app

`ios/`: the program lives in the app (Программа), the history is kept forever (История), the watch link,
`workout` install/update and the signing expiry are under Часы. Design: `docs/specs/2026-11-13-workout-ios-design.md`.

- Talks over the stub's own channel key K (`docs/abi-1.4.md`): no bind, no popup, next to Nothing X on the
  connection iOS already holds. Needs the new stub (OTA) and loader; with the old ones it gets no answer.
- Syncs on every `R` of the workout app: history first, then the program unless the watch has it or a workout runs
  (then it waits for the `R` the app sends when the workout ends).
- Storage: one JSON file in Application Support; the pairing secret in the Keychain.
- `scripts/ios-install` builds `workout.wp2app` into the app and installs it on the connected iPhone (`WP2_TEAM`,
  `WP2_BUNDLE_ID`); free Personal Team: re-run every 7 days. `scripts/ios-install --sim -demo` runs it in the iOS
  Simulator with made-up data; `--sim -tcp 7700` talks to `wp2apps bridge --hci-port P` (emulator).
- Import of the old Markdown program: `wp2apps workout push <dir> --dry-run > program.json`, then Программа → import.
- Tests: `(cd ios/WireKit && swift test)` (vectors from `tests/ios_vectors.py`), `tests/test_ios_e2e.py` (WireKit
  against the emulator over K).
