# Shared workout Implementation Plan

> Executed inline (the owner: "да, делай"). Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** one workout on the watch and the iPhone, runnable on the phone alone.

**Architecture:** a workout is `start` + `day` + a grow-only set of set events `(ex, t)`; the phone holds the merge
rules and drives the watch with `D`/`F`/`H`; the watch reports with `P` and `X`.

**Tech Stack:** Rust no_std (watch app), Swift 6 / SwiftUI iOS 17 (WireKit, app), Python e2e.

**Spec:** `docs/specs/2026-11-14-shared-workout-design.md`

## Global Constraints

- Times are the watch's: seconds since 2000-01-01 00:00 **local** (`model::epoch_s`); the phone converts with the
  same base (WireKit `Entry.date` already does the forward direction).
- No ABI or loader change; host → app messages keep the `seq` byte at index 3; at most 128 B per message.
- App stack 8 KiB without a guard: no big temporaries in `main`'s frame.
- Heart-rate values never in logs or tests (synthetic only).
- An older watch app ignores `D`/`F`/`H`: the phone must then keep its workout to itself and still work.

## Review Focus

- A `D` for a workout the watch already finished (same `start` in its history): ignored, no second entry.
- The phone finishes while the watch app is closed: `H` arrives later and must not duplicate an entry the watch made.
- Events beyond an exercise's planned sets (both sides pressed offline): dropped, not overflowing `done`.
- A `D` with a `t` older than the newest event: no rest restart.
- Phone clock vs watch clock a few seconds apart: same events still dedupe (the phone never re-stamps watch events).

---

### Task 1: watch model

**Files:** `sdk/examples/workout/src/model.rs`

**Produces:** `Model::merge_set(start: u32, day: u8, ex: u8, t: u16, now: u32) -> bool` (adopts when no workout and
`start` is not stale and not in the history; ex 0xFF only adopts; adds `(ex, t)` if new and the exercise has sets
left; newest event sets `rest_until`/`rest_ex`; returns changed), `Model::finish_at(start: u32, end: u32) -> bool`,
`Model::add_entry(e: Entry) -> bool` (dedupe by start; `last_day` follows when newest).

- [ ] tests: adopt, dedupe, over planned, stale, already in history, older t keeps rest, finish_at wrong start,
      add_entry dedupe + rotation
- [ ] implement, `cargo test -p workout --target aarch64-apple-darwin`
- [ ] commit

### Task 2: watch protocol and loop

**Files:** `proto.rs` (`Got::Set{start,day,version,ex,t}`, `Got::Finish{start,end}`, `Got::Entry(Entry)`),
`main.rs` (apply; a merged set on the open set screen of that exercise → Rest; finish → Done; save), `docs/workout.md`.

- [ ] proto tests for the three parsers, implement, commit

### Task 3: e2e (Python as the phone)

**Files:** `src/wp2apps/workout.py` (`set_msg`, `finish_msg`, `entry_msg`), `tests/test_workout_e2e.py`
(`test_4_phone_drives`: D adopts + fills a dot + rest; watch set reaches X; F finishes; H moves the rotation).

### Task 4: WireKit

**Files:** `W1.swift` (encoders, `watchSeconds(Date)`), new `Shared.swift` (`OpenWorkout`, `mergePlan`), tests.

**Produces:** `struct OpenWorkout { start: UInt32; day: Int; version: UInt32; sets: [SetMark] }`,
`enum MergeAction { send(Data), adopt(OpenWorkout), finishOwn, fetchSets }`,
`func mergePlan(phone: OpenWorkout?, watch: (start: UInt32, day: Int, done: Int)?, watchSets: [SetMark]?, watchVersion: UInt32) -> [MergeAction]`.

### Task 5: app logic

**Files:** `Store.swift` (`open: OpenWorkout?`, `startWorkout`, `doneSet`, `finishWorkout` → history entry,
`pendingEntries` for `H`), `SyncController.swift` (run `mergePlan` on `R`/`P`, send, fetch X).

### Task 6: app UI

**Files:** `PulseViews.swift` → active workout: day list with filled dots, set screen with "Сделал", rest ring +
local notification + haptic, "Завершить"; Программа → day → "Начать".

### Task 7: verify and ship

All suites, e2e, `scripts/ios-install` with a new workout version.
