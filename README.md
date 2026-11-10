# wp2-apps

Installable apps for the CMF Watch Pro 2 (board JX402, stock firmware `1.0.0.73`):
a small loader added to the stock firmware, a Rust SDK, and apps installed over BLE
without reflashing the watch.

Status: runs in the emulator only. Nothing here runs on a watch yet.

- Design: [docs/specs/2026-11-10-wp2-app-loader-design.md](docs/specs/2026-11-10-wp2-app-loader-design.md)
- Plan, part 1: [docs/plans/2026-11-10-wp2-app-loader-foundations.md](docs/plans/2026-11-10-wp2-app-loader-foundations.md)

## Relation to cmf-watch-utils

The emulator, the firmware tooling and the reference firmware come from a local checkout of
[cmf-watch-utils](https://github.com/arnav-kr/cmf-watch-utils), expected next to this repository
at `../cmf-watch-utils` with its emulator set up (`runtime/`, `fixtures/`). No code is copied from
it. Emulator changes are contributed there and pinned here by commit.

## Firmware and loader in the emulator

Everything runs in the emulator only. Host tools (`llvm-objcopy`, `tk`, `gdb`) come from `nix-shell -p …`.

Build the patched `app.bin` (stub, heap reservation) and the loader package, then an app package:

```bash
scripts/build-fw                      # target/app-wp2.bin, target/loader.wp2app
scripts/build-app vibration-lab 1     # target/vibration-lab.wp2app (name, version)
```

Put `target/app-wp2.bin` into a NOR image with `wp2apps nor <stock-nor> --app target/app-wp2.bin --output <out>`
and boot it with `scripts/emu --nor <out>` (`scripts/emu` runs `cmf-watch-utils emu run` under `nix-shell -p tk`).

Install over the stub's own BLE channel through Netsim. Start `netsimd` (`$WP2_NETSIMD`, default
`~/Library/Android/sdk/emulator/netsimd`) with an H4 port, start the emulator with the same `--hci-port`,
then:

```bash
uv run --locked wp2apps install target/loader.wp2app --hci-port P
uv run --locked wp2apps install target/vibration-lab.wp2app --hci-port P   # optional --slot N
uv run --locked wp2apps list --hci-port P
uv run --locked wp2apps remove --slot N --hci-port P
```

The loader opens from the "Apps" row at the end of the stock app list (list style). Tests:

```bash
uv run --locked python -m unittest discover -s tests -v     # e2e skipped without WP2_E2E=1
(cd sdk && cargo test --target aarch64-apple-darwin)
(cd fw && cargo test -p wp2-core --target aarch64-apple-darwin)
WP2_E2E=1 nix-shell -p gdb llvm --run 'uv run --locked python -m unittest tests.test_e2e -v'
```

One e2e run takes minutes; logs go to `runs/e2e/`.

Limits:

- Evidence is from the emulator only. A physical watch is untested and is never written without the owner's approval.
- The "Apps" row exists only in the list style, not in the waterfall style.
- Packages live in `/NAND:/wp2apps` on the stock FAT volume.
- Addresses are valid for JX402 `1.0.0.73` only.

## License

MIT, see [LICENSE](LICENSE).
