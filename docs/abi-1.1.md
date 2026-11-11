# App ABI 1.1: host data channel, storage, system info, sensors

Status: design, 2026-11-11. JX402 `1.0.0.73` only. Stock entries come from `docs/recon.md` ("ABI 1.1" section);
each one is marked with how it was checked. Verification results and emulator numbers are in that section and
in `tests/test_api_e2e.py`.

## Compatibility

- `Api.abi` stays 1, `Api.abi_minor` becomes 1. The table only grows: the ABI 1.0 fields keep their offsets, the
  new function pointers follow `exit`. An ABI 1.0 app never reads past `exit`, so it runs unchanged.
- An app built against the 1.1 SDK refuses to start on a 1.0 loader (`__start` returns `-2`, the menu shows the
  error). The package header carries the app's `abi_minor`; the stub refuses to install, and the loader to load, a
  package whose `abi_minor` is above the one they know (`LOADER_ABI_MINOR`, now 1).
- New stub, new loader. The stub gets a second patch point (below) and new `StubRam` fields. A 1.1 loader on an
  old stub sees `StubRam.api == 0` and runs 1.1 apps without the channel (`send` returns `-107`, nothing
  arrives). A 1.0 loader on the new stub ignores the new fields.
- ABI 2 (LVGL apps, `docs/recon-lvgl.md`) is separate. The services here do not depend on how an app draws: the
  wire protocol, the `StubRam` queues, the storage format and the stock entries are reused as they are by ABI 2.
  The event code `MESSAGE = 7` and the function signatures below are reserved for that reuse.

## App side (SDK `cmf-wp2-app`)

New `Api` fields, in this order after `exit`:

| Field | Signature | Returns |
|---|---|---|
| `send` | `(data: *const u8, len: usize) -> i32` | `0` queued; `-1` `len` not in `1..=128`; `-11` outbox full; `-107` no host listening |
| `recv` | `(buf: *mut u8, cap: usize) -> i32` | length `1..=128` of the oldest message (removed); `0` none; `-1` `cap` too small (message kept) |
| `load` | `(buf: *mut u8, cap: usize) -> i32` | stored length `0..=4096` (`0` = nothing stored); `-1` `cap` too small; `-5` I/O or both copies corrupt |
| `save` | `(data: *const u8, len: usize) -> i32` | `0`; `-1` `len > 4096`; `-5` I/O. `len == 0` deletes the data |
| `time` | `(out: *mut Time) -> i32` | `0`; `-5` RTC unavailable |
| `battery` | `() -> i32` | bits 0–7 percent, bit 8 charging, bit 9 full, bit 10 charger connected; `-5` driver error |
| `keep_screen_on` | `(on: bool) -> i32` | `0` |
| `accel` | `(out: *mut [i16; 3]) -> i32` | `0` and x, y, z in mg; `-5` no sample (only if phase 5 proves it, see Sensors) |

```rust
#[repr(C)]
pub struct Time { year: u16, month: u8, day: u8, hour: u8, minute: u8, second: u8, weekday: u8,
                  millis: u16, utc_offset_min: i16 }   // 12 B, local time as the watch shows it
```

New event `MESSAGE = 7`: one per message that reached the app's inbox, `x` = its length. The app reads it with
`recv`. Messages arrive in order; the event is a hint, `recv` until `0` is also correct.

Every new call is safe from the app thread, also while paused. Calls that touch hardware state (`keep_screen_on`)
only record the wish; the loader applies it on the UI thread.

## Data channel

### Wire protocol

Same transport, frame format, crypto and gate as install (`0x90A0..0x90A4`): system frame `c1 = 0xFFFF`,
AES-CBC with the session key, CRC32 tail. The stub answers only while the stock authkey **and** session key are
non-zero.

| Opcode | Direction | Plaintext | Reply |
|---|---|---|---|
| `0x90A5` MSG | host → app | `slot: u8` (`0..7`, or `0xFF` = whatever runs), `data[1..=128]` | `0xB0A5`: `status: u8` |
| `0xB0A6` EVENT | app → host, unsolicited | `seq: u8`, `slot: u8`, `data[1..=128]` | none |

MSG status: `0` OK (in the inbox), `1` BAD_REQUEST (length), `7` NO_APP (nothing runs, another slot runs, or the
running app is ABI 1.0), `8` FULL (inbox full, retry), `9` PAUSED (a stock popup covers the app, retry).

`seq` counts EVENT frames per stub boot, modulo 256; a gap means the host lost frames (a disconnect, or the outbox
was dropped).

**No collision with stock.** The stock dispatcher `0x100D343C` takes `c1 == 0xFFFF && c2 > 0x908B` straight to its
`default` branch (`movw r2, #0x908b; cmp; bhi 0x100D34EA`), the branch the stub hooks. No stock handler sees
`0x90A0..0x90A5`. Watch → host frames of stock system commands are `request | 0xA000`, so at most `0xB08B`;
`0xB0A0..0xB0A6` are ours. The host parser needs no plaintext exception for them (both are encrypted).

### Watch side

`StubRam` (≤ `0xE00`) gets:

- `api: u8` (was padding): 1 once the new stub booted. The loader uses the queues only when it is 1.
- `app_flags: u8`, written by the loader: bit 0 an ABI ≥ 1.1 app runs, bit 1 it is paused.
- inbox: 4 × `{len: u16, data[128]}`, head/tail indices. Producer: the stub (thread `wewear_read`, the one
  that runs the stock dispatcher). Consumer: the app thread (`recv`), and the loader (UI thread) when it resets
  the inbox at launch and stop, while the app thread is not running.
- outbox: 4 × `{len: u16, slot: u8, data[128]}`, head/tail. Producer: the app thread (`send`). Consumer: the stub.
- `host_key: u32`: first word of the session key at the last authenticated wp2 frame (any of `0x90A0..0x90A5`).
  "A host listens" = the stream is connected (`*(u8 *)(*0x3801A658 + 4) & 1`, the bit `wewear_read` itself
  polls) and the current session key starts with `host_key`. A new bind (another client, a new handshake)
  changes the session key, so a phone that binds after us never gets app frames.

The stock encrypt/send helpers use static buffers (`0x100CCB34`), so every frame we send goes out from
`wewear_read`, like the install replies. That thread idles in `os_sleep(10)` (`0x100CBFC6..0x100CBFCC`) whenever
no frame is pending. The stub replaces that `bl 0x101BE1EC` at **`0x100CBFC8`** with `bl wp2_ble_idle`, which
sends at most one EVENT from the outbox and then sleeps the same 10 ms. If no host listens, it drops the outbox.
The patcher checks the old bytes, as for the other patch points. The new stub header field (the former reserved
word) carries the hook; zero means no hook.

Next to `running_slot`, the loader keeps `app_flags` current: set at launch from the package's `abi_minor`, bit 1
in the pause tick, cleared at stop. The stub accepts a MSG only with bit 0 set, bit 1 clear and a matching slot.

### Limits

| | |
|---|---|
| message | 1..=128 B, both directions (the install `DATA` size, proven on this path) |
| inbox / outbox | 4 messages each |
| delivery host → app | acked: the MSG reply says whether it reached the inbox |
| delivery app → host | best effort: at most one EVENT per `wewear_read` idle pass (~10 ms), dropped when no host listens; `seq` shows gaps |

### Host side

- `wp2apps send [--slot N] [--hex] [--wait S] DATA (--hci-port P | --address A)`: one MSG, retried on FULL
  and PAUSED until `--wait` (default 10 s), exit code 1 with the status name otherwise.
- `wp2apps listen [--slot N] [--count N] [--seconds S] (--hci-port P | --address A)`: prints one JSON line per
  EVENT (`seq`, `slot`, `data` hex, `text` if UTF-8).
- `wp2apps.channel.Channel` for daemons:

  ```python
  ch = Channel(address=None, hci_port=None, slot=0xFF)
  await ch.send(b"...", timeout=10)   # reconnects, retries FULL/PAUSED, raises NoApp / TimeoutError
  msg = await ch.recv(timeout=None)   # Message(seq, slot, data); reconnects on link loss
  await ch.close()
  ```

  A lost link (BLE error, request timeout) drops the session; the next call reconnects with backoff
  (1, 2, 4 … 30 s) and redoes the handshake.

### Security and power

- Gate: the same as install. The pairing secret is readable by any BLE client (`AT GETSECRET`, see the threat model
  in `docs/recon.md`), so **any client in range can send messages to the running app and read what it sends**.
  Apps must treat messages as untrusted input and must not send secrets or personal data.
- The channel adds no radio activity of its own: the host decides when to talk; the idle hook only checks two
  words before it sleeps (the thread already wakes every 10 ms).

## Storage

- One blob per slot: `/NAND:/wp2apps/N.dat`, at most 4096 B of app data. File format: `"WP2D"`,
  `u32 len`, `u32 crc32(data)`, `data`.
- `save`: write `N.new`, close, unlink `N.dat`, rename `N.new` → `N.dat` (the stock FAT rename does not
  overwrite). `load` reads `N.dat`, falls back to `N.new` (a cut between unlink and rename), checks magic, length
  and CRC; a half-written `N.new` fails the CRC.
- The data belongs to the slot's app: `wp2apps remove --slot N` (stub `DELETE`) also unlinks `N.dat` and `N.new`;
  an install into a slot that held a package with another name unlinks them too. Reinstalling the same name
  (an update) keeps them.
- Runs in the app thread (stack 8 KiB), only for the running app's slot. NAND wear is the app's responsibility:
  save on change, not per frame.

## System info

| Call | Stock entry | Check |
|---|---|---|
| `time` | `0x100C14B5(struct rtc_time *)`: the RTC driver's get_time, then year + 1900 and month + 1 (`{u8 sec, min, hour, mday, mon, wday, u16 year, u16 ms}`, read from the `rtc_time_d`/`rtc_time_t` log in `0x100C14F4`); zone `0x100C1459() → h·3600 + m·60` s (`gSysTimeZone`) | emulator with `QEMU_WP2_HOST_RTC=1` against the host clock |
| `battery` | power supply device, `0x101CE6FD` → capacity (prop 3), `0x101CE703` → status (prop 0: 3 charging, 4 full), `0x101CE70F` → DC5V (prop 4); the getters behind ux2sys property `0x0A`/`0x0B`, without its low-battery bookkeeping | emulator input `p mV charger percent` |
| `keep_screen_on` | stock per-view wake locks: `0x101B59A9(id)` takes, `0x100C1859(id)` + `0x100C1739()` release (the `AT SETWAKELOCK` pair); stock views lock their own view id (`0x1C`, `0x63`, `0xF6` …), the loader uses `0xF0` | emulator: the panel stays on past the BLE wake-lock release that blanks it otherwise |

`keep_screen_on(true)` holds the lock only while the app runs and is not paused; the loader releases it on
pause, exit, crash, force stop (button) and view delete, and takes it again on resume. Each held minute is
display power; apps should hold it only while they show live data.

## Sensors

Phase 5 decides, by emulator proof, which of these get an `Api` call; the rest stay open in this section.

| Sensor | Stock path found | Emulator model |
|---|---|---|
| accelerometer | ux2sys property `0x2B` getter `0x100E2EB1` copies the latest sample (24 B; `AT GETGSENSOR` prints words 0–2 as floats X, Y, Z) | STK8321, input `a x y z` (mg) |
| GPS | `AT SETGPSSWITCH` sets property `0x63` (mode 0..5), `AT GETGPSSNR` reads property `0x62` | UART NMEA, input `g on lat_e6 lon_e6` |
| heart rate | property `0x02` "change_hr_mode" | no PPG model; only a firmware hook (`health_hook`) |

Rules for any sensor that ships: the loader owns start and stop (off at exit, pause, crash, force stop and view
delete); GPS and heart-rate values are personal data and are never logged, committed or reported.
