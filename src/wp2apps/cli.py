import argparse
import json
from pathlib import Path
import sys
import zlib

from . import firmware, package, paths


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
    return top


def main(argv=None):
    args = parser().parse_args(argv)
    try:
        return args.handler(args) or 0
    except (OSError, ValueError) as error:
        print(f"Error: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
