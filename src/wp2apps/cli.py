import argparse
import json
from pathlib import Path
import sys
import zlib

from . import package


def cmd_pack(args):
    data = package.pack(args.image.read_bytes(), name=args.name, version=args.version)
    with args.output.open("xb") as out:
        out.write(data)


def cmd_info(args):
    pkg = package.parse(args.package.read_bytes())
    print(json.dumps({
        "name": pkg.name, "version": pkg.version, "kind": pkg.kind, "abi": pkg.abi,
        "load_addr": f"{pkg.load_addr:#x}", "entry": pkg.entry,
        "image_size": len(pkg.image), "crc32": f"{zlib.crc32(pkg.image):#010x}",
    }, indent=2))


def parser():
    top = argparse.ArgumentParser(prog="wp2apps")
    commands = top.add_subparsers(dest="command", required=True)
    pack = commands.add_parser("pack", help="Wrap a raw app image in a WP2 app package")
    pack.add_argument("image", type=Path)
    pack.add_argument("--name", required=True)
    pack.add_argument("--version", type=int, required=True)
    pack.add_argument("--output", type=Path, required=True)
    pack.set_defaults(handler=cmd_pack)
    info = commands.add_parser("info", help="Validate a package and print its header")
    info.add_argument("package", type=Path)
    info.set_defaults(handler=cmd_info)
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
