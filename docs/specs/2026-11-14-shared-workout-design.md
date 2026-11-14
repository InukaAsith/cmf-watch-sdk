# One workout on the watch and the phone

## Why

The owner wants to run a workout from the iPhone too, without the watch, and to see and drive the same workout from
either side when both are there: "Сделал" on the phone or on the watch counts once in the same workout, the next day
stays the same on both.

## Goals

- Start, walk (sets, rest timer) and finish a workout on the phone alone.
- With the watch app open, one workout on both: a set done on either side shows on the other.
- The history and the next day agree on both after a sync; nothing a side recorded is lost.
- The phone's active workout looks like the watch: the day's exercises with set dots that fill in, the open set or
  the rest countdown, heart rate when the watch gives one.

Non-goals: Live Activity / lock screen widget, Apple Health, a workout without a program, editing past workouts.
Anti-goals: a set counted twice or lost after a merge; the watch losing its workout because the phone sent something.

## Constraint

The phone only talks to the open `workout` app on the watch. Closed app or no link: each side runs on its own, so a
merge happens on every meeting, not only as a fallback.

## Approaches

- **A, event log (picked, my call; the owner did not object).** A workout is `start`, `day`, the program version and
  a grow-only set of events "set done: exercise, seconds from start". Each side adds its own; on a meeting they
  exchange the missing ones, the union is the workout. Counts and the rest are derived. Offline presses on both
  sides both count. The watch already keeps exactly this list (the trace's sets).
- B, one owner, the other a remote: simpler while linked, needs an ownership handoff when the link drops, and two
  offline starts lose one workout.

## Design

**Identity.** A workout is identified by its `start` (unix seconds, each side's clock; the watch's clock is set by the
phone's stock app). An event is `(ex, t)`; equal pairs are the same event.

**Protocol** (`docs/workout.md`; host → app messages keep their `seq` byte; an older app ignores unknown kinds, so the
phone falls back to independent workouts):

| Kind | Direction | Body |
|---|---|---|
| `D` | host → app | `start: u32, day: u8, version: u32, ex: u8, t: u16`; `ex` 0xFF = only "this workout runs". The app adopts the workout if it has none (and `start` is not stale), adds the event if `start` is its workout's, ignores it if `version` is not its program's |
| `F` | host → app | `start: u32, end: u32`: finish that workout (history entry, rotation), as "Завершить" on the watch |
| `H` | host → app | `start: u32, dur_s: u16, day: u8, done: u8, planned: u8` (the `Y` body): a workout done on the phone alone; added to the history once (by `start`), the rotation follows it when it is the newest |
| `P` | app → host | as now; the phone fetches the sets (`T` → `X`, now allowed during a workout) when `P`'s `start` is its own and `done` differs from its count |

**Who decides.** The phone holds the merge rules; the watch only applies `D`, `F`, `H`. On a meeting (first `R` or
`P` of the session):

- same `start`: exchange missing events (phone sends `D` per event the watch lacks, fetches `X` for the rest);
- one side has no open workout: the other's is adopted (`D` per event, or the phone takes the watch's from `X`);
- two different open workouts: the one with more sets stays open, the other is finished into the history as its own
  entry (`F` to the watch, or the phone finishes its own locally and sends `H`). Tie: the earlier start stays;
- different program versions: no sharing; each finishes its own, the phone sends its entry with `H`.

**Rest.** Derived on both sides: the newest event's time plus that exercise's rest. A `D` that lands on the watch's
open set screen moves it to the rest screen; the rest end buzzes as now.

**Watch** (`sdk/examples/workout`): `Model::merge_set(start, day, version, ex, t, now)`, `Model::finish_at(start,
end)`, `Model::add_entry(entry)`; trace sets stay the event list (96 max). No ABI change, no loader change.

**Phone** (`ios/`): `Store.open: OpenWorkout? {start, day, version, events, finished}` persisted with the rest;
a pure `mergePlan(phone, watch) -> [action]` in WireKit (tested); SyncController sends `D`/`F`/`H` and fetches `X`.
UI: Программа → a day → "Начать"; the active workout (section + sheet, as now) gets the day's exercises with dots
filled per set (at most 5, as on the watch, grey name when complete), a tap opens the set screen with "Сделал",
the rest as a countdown ring with a time-sensitive local notification and a haptic at the end, "Завершить" at the
bottom. Heart rate shows only while the watch sends it.

**History.** Phone-only workouts enter the phone's history at finish (no heart rate, sets with times) and the
watch's with `H` on the next meeting.

## Left to the builder

Message chunking details, notification wording, the exact layout of the set screen, how stale an adoptable `start`
may be (the watch's `STALE_S`, 3 h, is the default).

## Tests

Watch: host unit tests for `merge_set` (adopt, dedupe, wrong version, stale), `finish_at`, `add_entry` + rotation.
WireKit: `mergePlan` for each case above, parse/encode vectors for `D`/`F`/`H`. e2e (`tests/test_workout_e2e.py`,
Python as the phone): `D` from the host fills a dot and starts the rest on the watch; a watch set reaches the host
through `X`; two different workouts merge per the rule; `H` moves the rotation.
