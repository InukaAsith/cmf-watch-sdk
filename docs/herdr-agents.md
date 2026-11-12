# herdr agents on the watch

`sdk/examples/herdr-agents` shows the [herdr](https://herdr.dev) agents of the Mac (Claude Code, Codex sessions)
and buzzes when one turns blocked or done. `wp2apps herdr` (`src/wp2apps/herdr.py`) is its Mac daemon. Needs the
ABI 1.1 loader (`docs/abi-1.1.md`).

## Run it

```bash
nix-shell -p llvm --run 'scripts/build-app herdr-agents 1'
uv run --locked wp2apps install target/herdr-agents.wp2app --address <UUID>
# close the "Paired" popup on the watch with the button
uv run --locked wp2apps herdr --address <UUID>      # leave it running; -v also logs titles
uv run --locked wp2apps herdr --chat --address <UUID>  # also a chat: typed lines go to the running app (api-demo)
# now open Apps → herdr-agents on the watch
```

- **Start the daemon before you open the app.** Without a cached session it binds, and every bind shows the stock
  "Paired" popup; closing it while the app runs ends the app. The daemon keeps the session key in
  `~/.cache/wp2apps/sessions.json` and resumes it on every reconnect, so once bound it shows no popup again (until the
  watch reboots or another client binds).
- **Phone Bluetooth off.** The watch takes one central. If the Nothing X app connects, it binds with a new key and
  the daemon loses the link.
- **macOS can keep a stale LE link** to the watch after a crash or sleep: the daemon then retries without
  connecting. Toggle the Mac's Bluetooth off and on.
- The app lives only while the screen is on: when the stock auto-sleep blanks the panel, the stock closes the app
  view. After a buzz the app holds the screen on for 20 s.
- Emulator: `--hci-port P` instead of `--address`.

## Watch app

- Header: blocked, done and working counts. Below it "waiting for Mac..." until the first message, or
  "stale N s" when nothing came for 25 s.
- An LVGL app (ABI 1.2) in the stock look: system font, a focus page (`stock::focus_page`) with the stock app list's
  fisheye: the agent nearest the centre line sits on a pill, the others shift along the edge, fade and their dots
  shrink; the crown scrolls with the round scroll bar. Order: blocked (red), done (green), working (blue, pulsing),
  idle (grey), unknown; then by workspace and title. Each row: status dot, title, workspace label under it, one line
  each ending in "..." (`LONG_MODE` DOT). Not in the simulator (no LVGL there yet).
- Blocked transition: three 110 ms pulses; done: one 220 ms pulse. Snapshots never buzz.
- Tap a row: the Mac focuses that agent's pane (`herdr agent focus`, else `herdr tab focus`). Button exits.
- The last list is saved in app storage after each snapshot and at exit, and fills the screen at the next start.

## Protocol

Every message: `"h1"` (magic and version 1), kind, then for host → app a `seq` byte, then the body. At most 128 B.

| Kind | Direction | Body |
|---|---|---|
| `B` begin snapshot | Mac → app | – |
| `U` upsert | Mac → app | `id`, `status`, `ws_len`, `ws`, `title` (UTF-8, ws ≤ 24 B, title ≤ 64 B) |
| `D` delete | Mac → app | `id` |
| `E` end snapshot | Mac → app | number of agents in the snapshot |
| `H` heartbeat | Mac → app | – (every 10 s without changes) |
| `R` request snapshot | app → Mac | – (no seq) |
| `F` focus | app → Mac | `id` (no seq) |

Status: 0 idle, 1 working, 2 blocked, 3 done, 4 unknown. `id` is a short per-pane number the daemon assigns; a
freed id is not reused at once. `seq` counts host messages modulo 256. A `B` resets it; any other message whose
`seq` is not the last one + 1 makes the app ask for a snapshot. The app sends `R` on start, on resume, after a gap,
after a snapshot whose `E` count does not match, and every 3 s while it waits or is stale. The daemon sends nothing
until an `R` arrives, then follows the app's slot. `NO_APP` or a send that stays `FULL`/`PAUSED` for 5 s marks the
app gone until its next `R`. The daemon sends the 32 most urgent agents (blocked and done first); the app keeps 32.

Daemon power: it PINGs every 0.25 s while the app is open and every 1 s otherwise, and polls `herdr agent list`
and `herdr workspace list` every second. Titles appear only in `-v` logs.

## Tests

```bash
uv run --locked python -m unittest tests.test_herdr -v
(cd sdk && cargo test -p herdr-agents --target aarch64-apple-darwin)
WP2_E2E=1 WP2_NETSIM_INSTANCE=87 nix-shell -p gdb llvm --run 'uv run --locked python -m unittest tests.test_herdr_e2e -v'
```

The e2e uses a fake `herdr` (`tests/fixtures/fake_herdr.py`) and saves frames to `runs/herdr-agents/`.
