import argparse
import asyncio
import json
from pathlib import Path
import sys
import zlib

from bleak.exc import BleakError

from . import blegatt, firmware, h4, install, link, package, paths


def cmd_pack(args):
    image = args.image.read_bytes()
    elf = args.elf.read_bytes()
    package.check_elf(image, elf)
    kind = package.KIND_LOADER if args.kind == "loader" else package.KIND_APP
    data = package.pack(image, name=args.name, version=args.version, kind=kind,
                        relocs=package.relocations(elf))
    with args.output.open("xb") as out:
        out.write(data)


def cmd_info(args):
    pkg = package.parse(args.package.read_bytes())
    print(json.dumps({
        "name": pkg.name, "version": pkg.version, "kind": pkg.kind, "abi": pkg.abi, "abi_minor": pkg.abi_minor,
        "relocs": len(pkg.relocs), "entry": pkg.entry,
        "image_size": len(pkg.image), "crc32": f"{zlib.crc32(pkg.image):#010x}",
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


def slot_name(slot):
    return "L" if slot == install.LOADER_SLOT else str(slot)


async def with_watch(args, action):
    transport = blegatt.gatt_over_ble(args.address) if args.address else h4.gatt_over_h4(args.hci_port)
    async with transport as gatt:
        session = link.Session()
        await session.handshake(gatt)
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
                                               "abi_minor": h.abi_minor} for n, h in found.items()}}))


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
                                 ("remove", cmd_remove, "Delete a slot"),
                                 ("list", cmd_list, "List installed packages")):
        sub = commands.add_parser(name, help=help_)
        if name == "install":
            sub.add_argument("package", type=Path)
            sub.add_argument("--slot", type=slot_arg)
        if name == "remove":
            sub.add_argument("--slot", type=slot_arg, required=True)
        where = sub.add_mutually_exclusive_group(required=True)
        where.add_argument("--hci-port", type=int, help="netsimd H4 TCP port (emulator)")
        where.add_argument("--address", help="BLE address (CoreBluetooth UUID on macOS) of a real watch")
        sub.set_defaults(handler=handler)
    scan = commands.add_parser("scan", help="List nearby CMF watches (read-only; address redacted by default)")
    scan.add_argument("--timeout", type=float, default=5.0)
    scan.add_argument("--show-address", action="store_true")
    scan.set_defaults(handler=cmd_scan)
    return top


def main(argv=None):
    args = parser().parse_args(argv)
    try:
        return args.handler(args) or 0
    except (OSError, ValueError, ConnectionError, BleakError) as error:
        print(f"Error: {error or type(error).__name__}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
