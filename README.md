# wp2-apps

Installable apps for the CMF Watch Pro 2 (board JX402, stock firmware `1.0.0.73`):
a small loader added to the stock firmware, a Rust SDK, and apps installed over BLE
without reflashing the watch.

Status: design and planning. Nothing here runs on a watch yet.

- Design: [docs/specs/2026-11-10-wp2-app-loader-design.md](docs/specs/2026-11-10-wp2-app-loader-design.md)
- Plan, part 1: [docs/plans/2026-11-10-wp2-app-loader-foundations.md](docs/plans/2026-11-10-wp2-app-loader-foundations.md)

## Relation to cmf-watch-utils

The emulator, the firmware tooling and the reference firmware come from a local checkout of
[cmf-watch-utils](https://github.com/arnav-kr/cmf-watch-utils), expected next to this repository
at `../cmf-watch-utils` with its emulator set up (`runtime/`, `fixtures/`). No code is copied from
it. Emulator changes are contributed there and pinned here by commit.

## License

MIT, see [LICENSE](LICENSE).
