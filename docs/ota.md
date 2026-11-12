# Firmware image for OTA

`make fw-ota VERSION_CODE=0x10003` builds `target/fw-wp2.bin`. It is the stock `1.0.0.73` image with the patched
`app.bin` (stub, heap reservation, a fourth vibration level at 35 %; see `wp2apps firmware --help`). All other
partitions (`res`, `fonts`, `res_e`, `sdfs_k`, AGPS trailer) are stock. `make fw-motor` uses the same builder for
the motor-only image.

## How it is built (`wp2apps ota-image`)

- The stub grows `app.bin` from `0x25A080` to `0x25CDE8` bytes, so `fwkit edit-app` (in-place patches) cannot carry
  it. Instead the builder unpacks the outer AOTA and the inner `TEMP.bin` AOTA with `fwkit unpack`, replaces
  `app.bin` and its manifest entry (`file_size`, `orig_size`, `checksum` = CRC32), then repacks both with `fwkit pack`.
- `TEMP.bin` is stored as `LZMA` chunks: `b"LZMA"`, `u32 16`, `u32 packed`, `u32 size`, then an XZ stream (preset 6,
  2 MiB chunks). Re-encoding the stock TEMP gives the stock bytes exactly (`tests/test_firmware.py`).
- In the outer manifest, `TEMP.bin`'s `file_size`, `orig_size` and `checksum` all describe the stored bytes, as in stock.
- **AOTA data CRC.** Header `+0x18` is the CRC32 of `[u16 +0x12, u32 +0x14)`, and the header CRC at `+0x04` covers it.
  `fwkit pack` and `edit-app` leave `+0x18` stale. Before `ota-image` fixed it (`firmware.fix_aota_crc`), every
  image we built had a stale one, including `fw-motor.bin` / `fw-motor-v10001.bin` from 2026-11-11. The emulator's
  OTA client rejects such an image. Whether the watch checks this field is unknown.
- `--version-code` sets the outer manifest's `version_code` (stock `0x10000`).
- `--temp-only` keeps only `ota.xml` and `TEMP.bin` and drops the AGPS trailer. `cmf-watch-utils emu ota` accepts
  only this form. Its `TEMP.bin` is byte-identical to the one in the full image.

## Emulator proof (2026-11-12)

```bash
uv run --locked wp2apps ota-image --app target/app-wp2.bin --temp-only --version-code 0x10002 --output target/fw-wp2-temp.bin
scripts/emu-ota target/fw-wp2-temp.bin runs/ota      # stock OTA over Netsim, then the emulator's install
WP2_NOR=runs/ota/install/nor.bin WP2_E2E=1 nix-shell -p gdb llvm --run \
  'uv run --locked python -m unittest tests.test_lv_e2e tests.test_e2e.HelloTest tests.test_e2e.MotorTest -v'
```

- The virtual stock watch took the whole transfer and answered `watch_reported_image_valid`.
- `emu recover` installed the staged update. The `app.bin` in the resulting NOR is byte-identical to
  `target/app-wp2.bin`.
- The e2e runs above passed 14/14 on that NOR.
- Limits:
  - The install step is the emulator's host model, not the watch's bootloader.
  - The full image's other partitions were not transferred in the emulator. They are stock bytes.

## Real watch

Not done yet. It needs the owner's yes for the exact image (`AGENTS.md`, rule 1), sent with the stock OTA sequence
(rule 3).
