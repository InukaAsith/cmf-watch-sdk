# Keyboards and voice typing

Text input for any app, from the UI kit (`cmf-wp2-ui`). The demo is `sdk/examples/keyboards`:

```bash
scripts/sim keyboards                                  # on the Mac
scripts/build-app keyboards 1 && uv run wp2apps install target/keyboards.wp2app   # on a watch
```

| Module | What | Needs |
|---|---|---|
| `keyboard` | QWERTY laid out for the round panel (Wear OS style): letters, `?12` symbols, shift, delete, space, send; a capital after the end of a sentence; the typed text with a caret | nothing |
| `emoji` | the watch's own 52 emoji (the stock `emoji28.font`), in text and as the keyboard's second page | loader 6 (`emoji_read`); older: no emoji key, emoji draw as boxes |
| `dial` | the rotary keyboard: letters round the rim, the crown picks, a tap types, held a capital | nothing |
| `dictate` | voice typing: the mic as IMA ADPCM to the phone, the words back as they come | loader 6 (mic) and a phone app that answers (below) |

## In an app

The app keeps its loop; the modules draw and say what to do next.

```rust
use cmf_wp2_ui::keyboard::{Act, Keyboard};
use cmf_wp2_ui::dictate::{Dictate, Out};

static mut EMOJI: [u8; cmf_wp2_ui::emoji::ram_for(18)] = [0; cmf_wp2_ui::emoji::ram_for(18)];

cmf_wp2_ui::emoji::init(api, unsafe { &mut *core::ptr::addr_of_mut!(EMOJI) });
let mut kb = Keyboard::new(ACCENT);
kb.draw(&mut surface, "Title");
// TOUCH_DOWN
match kb.tap(x, y) {
    Act::Typed(k) => { /* kb.draw_text, kb.draw_key(k, true), plain again after FLASH_MS */ }
    Act::Redraw => kb.draw(&mut surface, "Title"),
    Act::Send => use_the_text(kb.text()),
    Act::Voice => dict.start(api, false),   // then dict.draw
    Act::Dial => { /* the rotary keyboard */ }
    _ => {}
}
```

- **Voice typing**: while `dict.active()`, wait at most `dict.wait_ms(...)` for events and call `dict.tick(api)`.
  Give every message to `dict.message(m)` first, touches to `dict.tap`, the button to `dict.cancel`. On
  `Out::Done`, `kb.add_words(dict.text())`.
- **Rotary keyboard**: `dial.down` on TOUCH_DOWN, `dial.up` on TOUCH_UP, `dial.crown` on CROWN, `dial.tick` with
  `dial.hold_wait` as a timeout (held letters become capitals). Its letters come in their case: set `kb.shift =
  false` before `kb.type_key`, and `dial.shift = kb.shift` after.
- **Emoji memory**: `ram_for(n)` keeps `n` emoji at once (18 = a whole page, about 20 KB). An app short of room
  can give the icon's memory once it runs (`app_icon_ram()`), as Chats does.
- **PAUSE**: the loader stops the mic, so cancel dictation (`dict.cancel`).

## The phone side of voice typing

The watch sends `~` messages on the app's own channel, so one phone feature serves every app:

| Direction | Message | Meaning |
|---|---|---|
| watch → phone | `~1` | start dictating (`~3`: a recording the phone keeps) |
| watch → phone | `~a seq:u16 block` | 240 samples at 8 kHz as one IMA ADPCM block (`cmf_wp2_app::adpcm`) |
| watch → phone | `~0` / `~2` | the end / cancel |
| phone → watch | `~ kind:u8 text`, kind 0 | the words so far (kind is a byte 0, 1 or 2, not a digit) |
| phone → watch | kind 1 | the final words |
| phone → watch | kind 2 | a problem, shown on the watch |

The mic is `cmf_wp2_app::mic::Voice`: 1 or 2 channels detected, 8 kHz, an automatic gain (the real watch's mic is
quiet). With no phone connected the screen says so; with no answer for 10 s it gives up.

Phone apps: CMFX (iOS speech recognition, on the iPhone when it can) answers today. The FMC Companion does not yet.
On the Mac, `hosts/voicehost.py` stands in for the phone in the simulator:

```bash
scripts/sim keyboards --tcp 7798 &
uv run python hosts/voicehost.py --sim 7798 --words "coffee at ten" heard.wav
```

## Limits

- At most `TEXT_MAX` (100) bytes of text; emoji take 4 each.
- The rotary keyboard's top letter sits under the loader's status bar (loader 7) while it is selected.
