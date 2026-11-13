# Workout app: design

Status: draft for the owner's review. Project 1 of 3 (watch app, then the iPhone companion, then heart rate).

## Why

The owner trains on a three-day split kept as notes in Comma (`Day 01.md` … `Day 03.md`, one checkbox per
exercise). In the gym they tick a checkbox on the phone and start a rest timer by hand. The goal is a custom
"harness" for workouts on the CMF Watch Pro 2: the plan lives on the watch, the watch walks through the sets and
times the rest. Later an iPhone app (SwiftUI) edits the program and reads the history, and heart rate shows
during the workout.

Success: a whole workout is done from the wrist, without the phone in hand, and the next time the watch
offers the next day.

## Goals (project 1)

- A watch app `workout` that shows the program, walks through sets, counts rest down and buzzes at its end.
- Progress survives the app being closed (screen-off, button, crash) and comes back where it was.
- A day rotation: the start screen offers the next day after the last workout.
- A short history of finished workouts on the watch, for the companion to read later.
- Until the companion exists, the program comes from the Mac: `wp2apps workout push` reads the owner's Markdown
  notes and sends them to the running app.
- A loader change so an app can handle the button ("back") itself and ask before it exits.
- A loader/firmware change so an app survives the screen going off and is still there after a wake.

## Non-goals (project 1)

- Recording actual reps or weight per set. The owner only ticks a set as done (their decision).
- Heart rate, accelerometer, rep counting: project 3. The SDK has no sensor today (`docs/abi-1.1.md`, Sensors).
- The iPhone app: project 2. Its wire protocol is this project's protocol, so design it to be reused.
- Editing the program on the watch.

## Anti-goals

- Losing a workout's progress because the screen went off or the button was pressed.
- Anything outside the stock look (AGENTS.md: system font, `cmf_wp2_app::stock`).
- Writing to the physical watch without the owner's approval (AGENTS.md rule 1). Everything is shown in the
  emulator first; the owner flashes once at the end (memory: emulator first, then flash).
- Personal data in commits: the owner's actual program and notes stay out of the repository; tests use made-up
  exercises.

## The watch app

LVGL app (ABI 1.2) in the stock look. Input: tap (`TOUCH_DOWN`), crown, button. No swipes or long press exist.
Mockups (brainstorm session): `.superpowers/brainstorm/*/content/workout-flow.html` (not committed).

### Screens

1. **Start.** Title "Тренировка". A focus list (`stock::focus_page`, as in herdr-agents) of the days:
   "День 1 · Жим и присед", subtitle "N упражнений". Focus starts on the next day in the rotation, which is marked
   "Следующая". Crown picks another day, tap starts it. If a workout is in progress, the app opens straight on its
   day list instead. With no program stored: a single line "Нет программы" and a hint to push it from the Mac.
2. **Day.** Title: day name and elapsed workout time (`День 1 · 0:18:42`). A focus list of exercises; the subtitle
   is set dots (done in accent, open in grey), the reps and the weight when the program has one
   (`●●○○ · 5–8 · 60 кг`). Tap an exercise opens its set screen. The last row is "Завершить".
3. **Set.** Exercise name, "Подход 3 из 4", the reps target large (`5–8`), the weight under it if any, one
   accent button "Сделал". Tapping it marks the set done, saves, and starts the rest.
4. **Rest.** A ring counting down the exercise's rest, the time large (`1:07`), "Дальше: подход 4 из 4" and a grey
   "Пропустить". At zero: a buzz (the kit's `haptic`, a short pattern noticeable in a gym) and back to the set
   screen of the next set.

### Flow

- After the last set of an exercise, rest runs, then the app opens the next exercise that still has open sets, in
  program order (owner's choice B). If none is left, it goes to a finish screen: "Готово", duration, sets done.
- The button is "back": set or rest screen → day list; day list → a dialog with three choices: "Выйти"
  (leave the app, the workout stays in progress and reopens where it was), "Завершить" (finish it) and "Отмена"
  (builder's call: the owner asked for an exit confirmation so nothing is lost; leaving without finishing must
  stay possible); start screen → exit. The "Завершить" row of the day list finishes too. Finishing ends the
  workout, writes it to the history and advances the rotation. The loader's escape for a hung app must not fire
  on a normal press inside this dialog.
- A workout with at least one set done counts as done and advances the rotation (owner: an unfinished Day 1 still
  leads to Day 2). A workout ended with zero sets done is dropped and does not advance it.
- A workout in progress that was not touched for 3 hours is finished automatically at the next start
  (builder's call; same rules as above).
- Rest that ran out while the app was closed: on reopen the set screen of the next set shows; no late buzz.

### Screen and app lifetime

The docs say the stock closes the app view when auto-sleep blanks the panel (`docs/app-guide.md:303`); the
owner believes that on the real watch with AOD the app stays. Neither is proven on hardware. The design does not
depend on it:

- State is saved on every change ("Сделал", finish, rotation), never per frame.
- The rest deadline is stored as wall-clock time (`api.time()`), so the timer is right after a reopen.
- During rest the app holds `keep_screen_on(true)`, and for 20 s after the end-of-rest buzz, then releases it.
- The app must survive the screen going off: after a wake the owner is back in the workout, not on the watch face.
  If the stock deletes the app view on auto-sleep (emulator says it does), that is fixed in the loader/firmware,
  not worked around (owner: "надо тогда загрузчик и прошивку доделать"). Find the stock path that closes the view
  on blank and what brings a view back on wake, keep the app view (paused while the panel is off, `RESUME` on
  wake), and prove it in the emulator; then confirm on hardware with AOD. Prefer a fix in the loader (NAND,
  `wp2apps install`). If the loader cannot do it, patch the stock code in `app.bin` and ship it as a full OTA
  (`docs/ota.md`): the owner accepts that, though OTA is not yet proven on hardware, and the flash itself still
  waits for their yes at that time. A "screen on for the whole workout" setting is a fallback only if both fail.

### Storage (one 4 KiB blob, slot storage)

Magic and version first (`"W1"`), unparsable blob = empty. Compact binary, sized for the owner's program
(3 days, ≤ 6 exercises each, names ≤ 32 B) with room to spare:

- Program: version id (from the host), days (name, exercises), each exercise: name, sets, reps min/max,
  weight text (optional, e.g. `90 кг`, `свой вес`, `50–60 кг`), rest seconds (default 90).
- Progress: day index, start time, per-exercise done-set counts, rest deadline.
- Rotation: index of the last finished day.
- History: a ring of the last N finished workouts (date, day index, duration, sets done / planned). N is whatever
  fits; at least 30.

Limits enforced on the host: the encoded program must leave room for progress and the history; `push` fails with
a clear message otherwise. The watch truncates names on a UTF-8 char boundary.

## Getting the program onto the watch (Mac, until the companion)

`uv run --locked wp2apps workout push <dir> --address <UUID>` (or `--hci-port P`):

- Reads every `*.md` in `<dir>` sorted by name. A day = the first `##` heading (text after "День N:" is the day
  name). An exercise = a list item (checkbox or not) of the form
  `<name>: <sets>×<reps>[–<reps>] [(<weight>)] [отдых <m:ss>]`. Both `×` and `x`/`х` are accepted. A line it
  cannot parse is reported with its file and line, and `push` stops. Example lines from the owner's notes:
  `Жим лёжа: 4×5–8 (90 кг)`, `Скручивания на полу 3х20` (no colon: name is the text before the sets),
  `Сгибание на бицепс` (no sets: report it; the builder decides whether a default like 3×10 is used or it fails).
- Encodes the program, then waits for the app to ask (the app pulls on start, on `RESUME` and every few seconds
  while it has no program or the host's version differs), sends it in ≤ 128 B messages with the
  begin/upsert/end framing of `docs/app-guide.md` "Protocol design", and exits once the app acks the version.
- A new program replaces the old one. A workout in progress is finished first (history keeps it).
- `--dry-run` prints the parsed program without Bluetooth.

Exercise and day names, weights, rest times and the history (dates, durations, counts) go over the channel in
plain form (builder's call): they are not in AGENTS.md's personal-data list, and the channel's weak pairing
secret is accepted for them. Record this exception in `docs/workout.md`. Still keep the owner's real program out
of commits and logs.

The protocol (`"w1"`, kinds and bodies) is documented in `docs/workout.md` and is what the iPhone app will speak.
Reading the history back (`wp2apps workout history`) is in scope only as the protocol message the companion
needs; a CLI printout of it is cheap and useful for testing.

## Loader change: the app handles "back"

Today `BUTTON` force-stops the app 1000 ms later (`ESCAPE_MS`, `fw/wp2-loader/src/main.rs:38`). New ABI call
(ABI 1.3, documented like `docs/abi-1.2.md`): an app declares it handles back itself. Then `BUTTON` is only an
event and the app exits by returning. A hung app must still be killable: the builder picks the escape (for
example a force stop if the app has not called `event()` for the escape time after a `BUTTON`, or after a second
press). Apps that do not opt in behave as today. The loader on the watch changes, so this needs a loader install,
approved by the owner, after the emulator proves it.

## Risks to settle first, in the emulator

1. **Cyrillic in the system font.** Stock fonts are subset to the glyphs their strings use; `font48m` lacks most
   Cyrillic (`docs/recon.md`, vibration settings row). Whether `font32M` (the system font) has all of `А–я Ёё` is
   unknown. Check first by rendering the full alphabet. If letters are missing, stop and bring options to the owner
   (own font on NAND, or a fallback) before building screens.
2. **Large digits.** The set and rest screens want a big numeral font. Check which stock font sizes have digits
   and `–`, `:`; otherwise use the kit's scaled text.
3. **App lifetime across screen-off**: the loader/firmware fix above, emulator first, then AOD on hardware.

## Projects 2 and 3 (for context, not designed here)

- **iPhone companion (SwiftUI).** Edit days and exercises (incl. rest per exercise and the screen setting), push
  to the watch, show the history. Speaks the `"w1"` protocol over the stub's BLE protocol. Main risk: the watch
  takes one central, and Nothing X binds with a new key (`docs/app-guide.md`, Operating rules); short
  connect-push-disconnect sessions or sharing iOS's system connection need a hardware check.
- **Heart rate.** Reverse the stock HR path (ux2sys property `0x02`), add it to the loader/SDK, test on hardware
  only (no PPG model in the emulator), then show it on the set and rest screens.

## Left to the builder

- Exact layouts, sizes and the buzz pattern within the stock look; the binary encoding; the protocol's kinds.
- Crate layout of the app (`sdk/examples/workout`), host tests for the model (rotation, progress, auto-finish,
  encoding) per `docs/app-guide.md`, and an e2e test in the emulator like `tests/test_api_e2e.py`.
