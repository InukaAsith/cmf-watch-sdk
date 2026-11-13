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
| `R` | app → host | `version: u32` stored (0 = none) |
| `A` | app → host | `version: u32` just stored |
| `Y` | app → host | `index: u8, count: u8, start: u32, dur_s: u16, day: u8, done: u8, planned: u8`; an empty history is one `Y` with count 0 |

A gap in `seq`, a `C` at another offset than the bytes received so far, more bytes than `total`, or an `E` whose CRC
or program does not check drops the transfer and the app sends `R` again. The program format is in
`docs/plans/2026-11-13-workout-app.md`, Task 4 (`sdk/examples/workout/src/model.rs`, `src/wp2apps/workout.py`).

Plain data: exercise and day names, weights, rest times and the history (dates, durations, counts) travel unencrypted
over a channel whose pairing secret any BLE client can read (`docs/app-guide.md`, section 4). The spec accepts this
for workout data (`docs/specs/2026-11-13-workout-app-design.md`); nothing else personal goes over it. The owner's real
program stays out of commits and logs: the tests use `tests/fixtures/workout/`.

## Hardware

2026-11-13, owner's approval: only the loader was written. `loader.wp2app` v3 (ABI 1.3, `own_back`) replaced loader
v1 (ABI 1.2) in slot `L` with `wp2apps install --address`; `wp2apps list` confirmed it. No OTA: the screen-off
behaviour needs no `app.bin` change (commit 99f0e3a). The `workout` app was **not** installed and no program was
pushed: the owner moves the workout side to an iOS app.

Checked by the owner with the older apps in slots 0–6 (abi_minor 1 and 2) on the new loader:

- The crown press and the right swipe leave an app to the "Custom Apps" menu, as before.
- The screen going off (auto-sleep and AOD) keeps the app; a button press wakes the watch straight into the app
  and does not end it. A longer wait (deep sleep) gave the same result.

Not checked on hardware: the `workout` back dialog and the rest buzz while the screen is dark.
Install notes: as in `docs/ota.md`, macOS held the LE link after `install`; toggling the Mac's Bluetooth freed it.
