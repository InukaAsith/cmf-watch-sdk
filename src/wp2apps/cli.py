import argparse
import asyncio
import json
import os
from pathlib import Path
import struct
import sys
import zlib

from bleak.exc import BleakError

from . import blegatt, channel, firmware, h4, icon, install, link, package, paths


def cmd_pack(args):
    image = args.image.read_bytes()
    elf = args.elf.read_bytes()
    entry = package.check_elf(image, elf)
    kind = package.KIND_LOADER if args.kind == "loader" else package.KIND_APP
    data = package.pack(image, name=args.name, version=args.version, kind=kind, entry=entry,
                        relocs=package.relocations(elf), abi_minor=package.abi_minor(image, elf))
    with args.output.open("xb") as out:
        out.write(data)


def cmd_info(args):
    pkg = package.parse(args.package.read_bytes())
    print(json.dumps({
        "name": pkg.name, "version": pkg.version, "kind": pkg.kind, "abi": pkg.abi, "abi_minor": pkg.abi_minor,
        "relocs": len(pkg.relocs), "entry": pkg.entry,
        "image_size": len(pkg.image), "crc32": f"{zlib.crc32(pkg.image):#010x}",
        "icon": icon.parse(pkg.image) is not None,
    }, indent=2))


def cmd_firmware(args):
    stub = args.stub.read_bytes() if args.stub else None
    with args.output.open("xb") as out:
        out.write(firmware.patch(paths.stock_app(), stub))


def cmd_nor(args):
    data = firmware.stage_nor(args.nor.read_bytes(), args.app.read_bytes())
    with args.output.open("xb") as out:
        out.write(data)


def slot_arg(text):
    if text.upper() == "L":
        return install.LOADER_SLOT
    if not text.isdigit() or int(text) >= install.SLOTS:
        raise argparse.ArgumentTypeError("slot must be 0..7 or L")
    return int(text)


def app_slot_arg(text):
    if not text.isdigit() or int(text) >= install.SLOTS:
        raise argparse.ArgumentTypeError("slot must be 0..7")
    return int(text)


def slot_name(slot):
    return "L" if slot == install.LOADER_SLOT else str(slot)


SESSIONS = Path(os.environ.get("WP2APPS_SESSIONS") or Path.home() / ".cache/wp2apps/sessions.json")


def cached_key(target: str) -> bytes | None:
    try:
        return bytes.fromhex(json.loads(SESSIONS.read_text())[target])
    except (OSError, ValueError, KeyError):
        return None


def cache_key(target: str, key: bytes) -> None:
    # the session key is personal data (AGENTS.md): user-only file outside the repository
    try:
        known = json.loads(SESSIONS.read_text())
    except (OSError, ValueError):
        known = {}
    known[target] = key.hex()
    SESSIONS.parent.mkdir(parents=True, exist_ok=True)
    fd = os.open(SESSIONS, os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o600)
    with os.fdopen(fd, "w") as out:
        json.dump(known, out)


async def with_watch(args, action, resume: bool = False):
    """resume: reuse the cached session key of this watch (send, listen), so a running app sees no bind popup."""
    transport = blegatt.gatt_over_ble(args.address) if args.address else h4.gatt_over_h4(args.hci_port)
    target = f"ble:{args.address}" if args.address else f"hci:{args.hci_port}"
    async with transport as gatt:
        session = link.Session()
        await session.handshake(gatt, key=cached_key(target) if resume else None)
        if resume:
            cache_key(target, session.key)
        return await action(session)


def cmd_install(args):
    data = args.package.read_bytes()
    slot = asyncio.run(with_watch(args, lambda s: install.install(s, data, args.slot)))
    print(json.dumps({"installed": slot_name(slot)}))


def cmd_remove(args):
    asyncio.run(with_watch(args, lambda s: install.remove(s, args.slot)))
    print(json.dumps({"removed": slot_name(args.slot)}))


def cmd_list(args):
    found = asyncio.run(with_watch(args, install.listing))
    print(json.dumps({"slots": {slot_name(n): {"name": h.name, "version": h.version, "kind": h.kind,
                                               "abi_minor": h.abi_minor, "icon": h.icon}
                                for n, h in found.items()}}))


def cmd_send(args):
    data = bytes.fromhex(args.data) if args.hex else args.data.encode()
    asyncio.run(with_watch(args, lambda s: channel.send(s, data, args.slot, args.wait), resume=True))
    print(json.dumps({"sent": len(data)}))


def cmd_listen(args):
    async def listen(s):
        state = await channel.ping(s)
        print(json.dumps({"running": slot_name(state.running_slot) if state.running_slot != 0xFF else None,
                          "abi_1_1": bool(state.flags & 1), "paused": bool(state.flags & 2)}), flush=True)
        loop = asyncio.get_running_loop()
        end = None if args.seconds is None else loop.time() + args.seconds
        count = 0
        while args.count is None or count < args.count:
            left = None if end is None else end - loop.time()
            if left is not None and left <= 0:
                break
            try:
                m = await channel.recv(s, 5.0 if left is None else min(left, 5.0))
            except TimeoutError:
                continue
            if args.slot not in (channel.ANY, m.slot):
                continue
            count += 1
            print(json.dumps({"seq": m.seq, "slot": m.slot, "data": m.data.hex(), "text": channel.text(m.data)}),
                  flush=True)

    asyncio.run(with_watch(args, listen, resume=True))


def cmd_herdr(args):
    from . import herdr
    return herdr.main(args)


def cmd_scan(args):
    found = asyncio.run(blegatt.scan(args.timeout))
    print(json.dumps({"devices": [
        {"name": name, "rssi": rssi, "address": address if args.show_address else "<redacted>"}
        for name, rssi, address in found]}))


def parser():
    top = argparse.ArgumentParser(prog="wp2apps")
    commands = top.add_subparsers(dest="command", required=True)
    pack = commands.add_parser("pack", help="Wrap a raw app image in a WP2 app package")
    pack.add_argument("image", type=Path)
    pack.add_argument("--name", required=True)
    pack.add_argument("--version", type=int, required=True)
    pack.add_argument("--output", type=Path, required=True)
    pack.add_argument("--elf", type=Path, required=True, help="Linked ELF: entry/length check and relocation table")
    pack.add_argument("--kind", choices=("app", "loader"), default="app")
    pack.set_defaults(handler=cmd_pack)
    info = commands.add_parser("info", help="Validate a package and print its header")
    info.add_argument("package", type=Path)
    info.set_defaults(handler=cmd_info)
    fw = commands.add_parser("firmware", help="Patch the stock app.bin (heap reservation, stub)")
    fw.add_argument("--stub", type=Path)
    fw.add_argument("--output", type=Path, required=True)
    fw.set_defaults(handler=cmd_firmware)
    nor = commands.add_parser("nor", help="Put a patched app.bin into a NOR image")
    nor.add_argument("nor", type=Path)
    nor.add_argument("--app", type=Path, required=True)
    nor.add_argument("--output", type=Path, required=True)
    nor.set_defaults(handler=cmd_nor)
    for name, handler, help_ in (("install", cmd_install, "Install a package over BLE"),
                                 ("remove", cmd_remove, "Delete a slot and its app data"),
                                 ("list", cmd_list, "List installed packages"),
                                 ("send", cmd_send, "Send one message to the running app (ABI 1.1)"),
                                 ("listen", cmd_listen, "Print the running app's messages as JSON lines")):
        sub = commands.add_parser(name, help=help_)
        if name == "install":
            sub.add_argument("package", type=Path)
            sub.add_argument("--slot", type=slot_arg)
        if name == "remove":
            sub.add_argument("--slot", type=slot_arg, required=True)
        if name in ("send", "listen"):
            sub.add_argument("--slot", type=app_slot_arg, default=channel.ANY,
                             help="only this app slot (default: whatever runs)")
        if name == "send":
            sub.add_argument("data")
            sub.add_argument("--hex", action="store_true", help="DATA is hex, not UTF-8 text")
            sub.add_argument("--wait", type=float, default=10.0, help="seconds to retry a full or paused inbox")
        if name == "listen":
            sub.add_argument("--count", type=int)
            sub.add_argument("--seconds", type=float)
        where = sub.add_mutually_exclusive_group(required=True)
        where.add_argument("--hci-port", type=int, help="netsimd H4 TCP port (emulator)")
        where.add_argument("--address", help="BLE address (CoreBluetooth UUID on macOS) of a real watch")
        sub.set_defaults(handler=handler)
    herdr = commands.add_parser("herdr", help="Daemon for the herdr-agents app (docs/herdr-agents.md)")
    where = herdr.add_mutually_exclusive_group(required=True)
    where.add_argument("--hci-port", type=int, help="netsimd H4 TCP port (emulator)")
    where.add_argument("--address", help="BLE address (CoreBluetooth UUID on macOS) of a real watch")
    where.add_argument("--sim", type=int, metavar="PORT", help="app in the host simulator (scripts/sim --tcp PORT)")
    herdr.add_argument("--herdr", default="herdr", help="herdr executable")
    herdr.add_argument("--period", type=float, default=1.0, help="seconds between `herdr agent list` polls")
    herdr.add_argument("-v", "--verbose", action="store_true", help="log what is sent, titles included")
    herdr.set_defaults(handler=cmd_herdr)
    scan = commands.add_parser("scan", help="List nearby CMF watches (read-only; address redacted by default)")
    scan.add_argument("--timeout", type=float, default=5.0)
    scan.add_argument("--show-address", action="store_true")
    scan.set_defaults(handler=cmd_scan)
    return top


def main(argv=None):
    args = parser().parse_args(argv)
    try:
        return args.handler(args) or 0
    except channel.NoApp as error:
        print(f"Error: {error}", file=sys.stderr)
        return 1
    except (OSError, ValueError, ConnectionError, BleakError, struct.error) as error:
        print(f"Error: {error or type(error).__name__}", file=sys.stderr)
        return 1
    except KeyError as error:
        print(f"Error: missing {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
