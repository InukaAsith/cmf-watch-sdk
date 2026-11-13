# Workout iPhone companion: design

Status: approved by the owner (2026-11-13). Project 2 of 3 (watch app done on `workout-app`; heart rate is project 3).
Project 1: `docs/specs/2026-11-13-workout-app-design.md`, protocol and program format: `docs/workout.md`.

## Why

The program lives in Markdown notes in Comma today, and `wp2apps workout push` on the Mac sends it to the watch.
The owner wants the iPhone to own the program: edit days and exercises on the phone, have the watch get them
without thinking about it, and see every workout ever done. The notes are retired.

Success: the owner edits the program on the phone, opens `workout` on the watch and it already has the new
program; the phone's history tab lists every finished workout since the app was installed, including those done
while the phone was in a pocket. Nothing X keeps working next to it.

## Goals

- SwiftUI iPhone app, three tabs: **Программа** (days → exercises → editor), **История**, **Часы** (link state,
  last sync, `workout` version on the watch, install/update).
- The program is stored only in the app. Exercise fields: name, sets, reps min–max, weight (free text: `90 кг`,
  `свой вес`, `50–60 кг`), rest (default 90 s). Days have a name and an ordered exercise list.
- Automatic sync: when `workout` opens on the watch, the phone (in the background) pulls new history and pushes
  the program if it changed, unless a workout is in progress.
- History kept on the phone forever, merged from the watch's ring (~30 entries) on every sync.
- Install and update the `workout` watch app from the phone (the package ships inside the iPhone app).
- One-time import of the current program from the JSON of `wp2apps workout push <dir> --dry-run` (Files / share
  sheet).
- Works next to Nothing X on the same phone: no bind, no "Paired" popup, no fight over the session key.

## Non-goals

- Per-set reps or weight; heart rate (project 3); stats beyond a per-month count in the history list.
- Editing the program on the watch. Installing the loader or firmware from the phone.
- Android, iPad layouts, iCloud sync between devices, App Store distribution.

## Anti-goals

- A workout in progress cut short by a sync (a new program finishes the running workout today).
- A bind popup while `workout` is open (it closes the app).
- Breaking Nothing X (its session, its notifications, its sync).
- Battery drain from constant polling in the background.
- Writes to the physical watch without the owner's yes (AGENTS.md rule 1); the owner's program or history, the
  watch address or the pairing secret in commits or logs (rule 2).

## Constraints found

- One LE link: on iOS all apps share the system's connection to a peripheral (`retrieveConnectedPeripherals`), so
  the app talks over the link Nothing X holds; no second central.
- Today the stub accepts app messages only under the **stock** session key, which comes from a bind
  (`fw/wp2-stub/src/ble.rs`, `docs/recon.md` "Bind popup"). Nothing X holds its own key; a second bind replaces it
  and shows the popup. → stub change 1.
- The stub sends app messages (`EVENT`) only right after its reply to a host frame, from `wewear_read`; hosts poll
  with `PING` every 0.25 s. An iOS app in the background has no timers, so it cannot poll. → stub change 2.
- `R` carries only the stored program version; the phone cannot tell a workout is running. → watch app change.
- The stub lives in `app.bin`: changes 1 and 2 need one full OTA (`docs/ota.md`, done once on 2026-11-12). The
  owner accepted this. Everything after that (watch app, its updates) installs over BLE from the phone.
- iOS Simulator has no Bluetooth: the phone's BLE path is testable only on hardware. The protocol layers are
  tested against the host simulator (`scripts/sim workout --tcp`) and shared test vectors.

## Approaches

1. **Native: SwiftUI + CoreBluetooth, protocols ported to Swift** (chosen). Frame crypto (`link.py`: AES-128-CBC
   with CommonCrypto, SHA-256 with CryptoKit, CRC32), channel (`channel.py`), `w1` and the program encoder
   (`workout.py`), install (`install.py`). Apple frameworks only, no third-party packages.
2. Share the Rust code (`wp2-core`, the watch model) with iOS as a static library over FFI: one encoder for both
   sides, but cargo for iOS targets, a C bridge and an Xcode build phase for ~50 lines of encoder. Dropped.
3. Phone as a remote for the Mac CLI: keeps the Mac in the loop, which is what the owner wants to drop. Dropped.

Sync: **automatic** (owner's choice) over manual "Синхронизировать". Fallback if stub change 2 fails in the
emulator: sync when the phone app comes to the foreground, plus a manual button (same protocol).

## Part 1: stub (firmware), one OTA

Both changes keep today's behaviour for the Mac tools (`wp2apps install`, `herdr`, `workout push`): the stock
session key keeps working.

**1. Own channel key.** The stub also accepts frames `0x90A0..0x90A7` encrypted with
`K = sha256("wp2-chan" ‖ pairing secret)[:16]` (builder may pick the exact derivation), with the same frame format
and CRC as today. Replies and `EVENT`s go out under the key the request used. No bind is needed: the phone reads
the secret once with `AT GETSECRET` on the shell characteristic and caches it in the Keychain (re-read when frames
stop being answered). Security is unchanged: any BLE client can read the secret already (`docs/recon.md`, threat
model, accepted by the owner). The stub can no longer use the stock decrypt/encrypt helpers for `K` (they use the
stock key buffers); it needs its own AES-CBC (software AES in `wp2-core`, host-tested against Python vectors, or a
stock AES primitive if recon finds one that takes a key argument).

**2. Unsolicited app messages.** When the app's outbox has a message and a host has used `K` since the link came
up, the stub sends the `EVENT` notification without waiting for a host frame. Recon decides from which thread
that is safe (the stream write is owned by `wewear_read` today; the loader's UI thread or a stock work queue are
candidates). `PING` keeps working for the Mac. If no safe path exists, stop and bring it to the owner: the fallback
above applies and change 1 still ships.

Proof in the emulator: an e2e test where a host binds the stock way (stands in for Nothing X), a second host uses
`K` only, the app sends a message with no host frame pending, and the `K` host receives it; the stock host's
session keeps working. Then the owner's yes, one OTA, and the hardware check below.

**Shared characteristic (owner: go ahead without asking more).** `K` frames use the same stock characteristic
that Nothing X listens on. Each frame is one ATT write (long write when over MTU-3), so frames of the two apps do
not interleave; Nothing X is expected to drop frames it cannot decrypt. Not provable before hardware: if Nothing X
breaks on them, a second OTA moves the channel to an own GATT characteristic.

Docs: a new `docs/abi-1.4.md` (or a section in `docs/abi-1.1.md`) for the channel key and unsolicited events.

## Part 2: `workout` on the watch and the Mac

- `R` gets one more byte: `flags: u8`, bit 0 = a workout is in progress (≥ 1 set done or started, not finished).
  `R` becomes 8 B. The app still sends `R` on start, on `RESUME` and every 3 s on the start screen, and also once
  right after a workout finishes, so a held-back program goes out at once.
- `workout.py`: accept 7 or 8 B `R`; `push` refuses to send while bit 0 is set ("finish the workout first") unless
  `--force`.
- `docs/workout.md` protocol table updated. The owner is told about this watch change (they approved it).

## Part 3: the iPhone app

### Screens (mockup: brainstorm session, three tabs chosen)

- **Программа:** list of days (`День 1 · Жим и присед`, "5 упражнений"), add, delete, reorder. Day screen: name,
  exercises with a one-line summary (`4×5–8 · 90 кг · отдых 1:30`), add, delete, drag to reorder. Exercise screen:
  name, sets stepper, reps min and max, weight text, rest (m:ss). Limits are checked while typing (7 days, 12
  exercises per day, names and weight ≤ 32 B UTF-8, encoded program ≤ 2048 B), with the remaining room shown when
  it gets tight. A badge on the tab while the watch has an older program ("ждёт отправки").
- **История:** grouped by month with a count ("Ноябрь · 6 тренировок"), newest first. Row: day name, date, duration,
  sets done / planned.
- **Часы:** link state (connected / not found / no `workout`), last sync time, program on the watch (same /
  older / held back because a workout is running), `workout` version on the watch vs the bundled one, buttons
  "Установить" / "Обновить", "Синхронизировать сейчас" (fallback and debugging), import of the `--dry-run` JSON.

Stock iOS look (List, Form, NavigationStack), system font, Russian UI.

### Storage

SwiftData (builder's call; a JSON file in Application Support is fine too). Program: days → exercises. History
entries: `start` (watch seconds since 2000-01-01, its local time), `durS`, `done`, `planned`, `dayIndex` and
`dayName` (resolved at sync time and never changed afterwards). Unique on `start`. Pushed programs: `version`
(CRC32 of the encoded bytes) → day names, kept so history can be resolved against the program the watch had.

### Sync

On every `R` from the watch:

1. Send `Q`, collect `Y`s, insert entries not yet stored. Resolve `dayName` from the day names of the program
   whose version is in this `R` (unknown version, e.g. pushed from the Mac: `День N`).
2. If the encoded program's version ≠ `R.version` and bit 0 is clear: push (`B`, `C`…, `E`), wait for `A`.
   Bit 0 set: hold it, show "ждёт конца тренировки".
3. Record the sync time.

Background: `UIBackgroundModes = bluetooth-central`, CoreBluetooth state restoration. The app subscribes to the
stub's notify characteristic over the shared link (or keeps a pending `connect` when nobody is connected, which
iOS completes when the watch is in range). A notification wakes the app; the sync above runs within the
background budget (a push is ~20 messages × ~15 ms). Before the stub change, or if change 2 fails: sync runs only
while the app is in the foreground, polling `PING` every 0.25 s while the "Часы" tab or a sync is active.

### Install from the phone

The iPhone app bundles `workout.wp2app`, built by an Xcode build phase from `sdk/examples/workout` and
`wp2apps` packaging (never committed). The "Часы" tab reads the installed header (`INFO` over the slots) and
compares name and version. Install follows `install.py`: slot with the same name, else a free one; `BEGIN`,
`DATA`…, `END`; `BUSY` while `workout` runs ("закройте приложение на часах"). Uses key `K`: no bind, no popup.
The user's tap on "Установить" is the owner's action; hardware testing of it still needs the owner's yes.

### Code layout (proposal)

`ios/Workout.xcodeproj` (or a Swift package plus a thin app target): `Wire/` (frame crypto, channel, `w1`, program
encoder, install, no UIKit, unit-tested on the Mac with `swift test`), `BLE/` (CoreBluetooth transport), `Sim/`
(TCP transport for `scripts/sim workout --tcp`, debug builds only), `UI/`, `Store/`.

## Testing

- Swift unit tests: program encoding and `w1` messages byte-for-byte equal to `workout.py` on the fixtures in
  `tests/fixtures/workout/` (vectors generated by Python into a JSON committed with the tests); frame
  encrypt/decrypt and `K` derivation against Python vectors; history merge and day-name resolution; limits.
- Simulator: the iOS app in the iOS Simulator against `scripts/sim workout --tcp` (sync, held-back push, history).
- Stub: `wp2-core` host tests for AES and the key path; emulator e2e as in Part 1.
- Watch app: model test for the `R` flag; emulator e2e push refused while a workout runs.
- Hardware, each step with the owner's yes: OTA of the new stub; Nothing X connected + the iPhone app: sync,
  install, Nothing X still syncs; background wake from a pocket.

## Order

1. Stub recon + changes 1 and 2 in the emulator (riskiest; decides automatic vs fallback sync).
2. `R` flag in `workout` and `workout.py`.
3. Swift `Wire` layer with vectors; iOS app against the simulator.
4. Owner's yes → OTA + install, hardware checks, results in `docs/workout.md`.

## Builder's calls (made without the owner)

- The phone's program always wins over one pushed from the Mac; an empty program (0 days) is never pushed.
- A held-back program cannot hang forever: the watch already auto-finishes a workout untouched for 3 hours.
- Signing: the owner's free Personal Team (personal use only): the profile expires every 7 days. The "Часы" tab
  shows the expiry and warns a day ahead; a script (`xcodebuild` + install to the connected iPhone) re-signs
  without opening Xcode. Deployment target iOS 17 (SwiftData, `@Observable`).

## Left to the builder

Exact `K` derivation and AES implementation; the thread for unsolicited events; SwiftData vs JSON; layouts within
the stock iOS look; how the build phase finds cargo and `uv`; signing setup (the owner's Apple ID in Xcode, kept
out of the repository).
