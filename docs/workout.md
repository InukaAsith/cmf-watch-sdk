# Workout app

The `workout` watch app and its Mac side. Design: `docs/specs/2026-11-13-workout-app-design.md`; plan:
`docs/plans/2026-11-13-workout-app.md`.

## Fonts

Probed 2026-11-13 in the emulator (a throwaway app, one stock font per screen with `А–Я`, `а–я`, `Ёё`, digits,
`–`, `-`, `:`, `×`, `●○`):

| Font | Cyrillic | Digits and signs |
|---|---|---|
| `F32M` (system) | complete except `Ё`, `ё` | `0–9`, `–`, `-`, `:`; no `×`, `●`, `○` |
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
