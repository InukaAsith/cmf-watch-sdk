"""Offline map packs for the maps app (sdk/examples/maps/src/pack.rs has the format).

  build: OSM tiles round a point → Nothing dark style (fixed 16 colours, STYLE), run-length coded per tile
  send:  the pack to the watch app over the channel (F/f/c per file, acked; P progress; Z done)

  uv run --with pillow python hosts/mapbuild.py build --lat 51.5080 --lon -0.1281 --radius 1.5 --out runs/mappack/london
  uv run --with pillow python hosts/mapbuild.py send runs/mappack/london --sim 7706

Tiles come from tile.openstreetmap.org through the same cache as pixview (runs/tiles), at most --max-tiles, one at a
time (the OSM tile policy: no bulk downloads). The phone app does the same from its own tile source.
"""
import argparse, asyncio, math, pathlib, struct, sys, time

from PIL import Image, ImageFilter

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
sys.path.insert(0, str(pathlib.Path(__file__).resolve().parents[1] / "src"))
from pixhost import rgb565, tile, world  # noqa: E402
from wp2apps.channel import Channel, SimChannel  # noqa: E402

TILE, CAP, WORLD_Z, MSG_MAX = 256, 32 * 1024, 20, 128


# OSM carto colour → Nothing dark map colour. Every pixel takes the nearest OSM colour, so the palette is fixed and
# the same in every pack.
STYLE = [
    ((0xF2, 0xEF, 0xE9), (0x0E, 0x0E, 0x0E)),  # land
    ((0xE0, 0xDF, 0xDF), (0x13, 0x13, 0x13)),  # residential
    ((0xD9, 0xD0, 0xC9), (0x24, 0x24, 0x24)),  # building
    ((0xC4, 0xB6, 0xAB), (0x2E, 0x2E, 0x2E)),  # building edge
    ((0xFF, 0xFF, 0xFF), (0x6A, 0x6A, 0x6A)),  # minor road, label halo
    ((0xF7, 0xFA, 0xBF), (0x9C, 0x9C, 0x9C)),  # secondary
    ((0xFC, 0xD6, 0xA4), (0xC4, 0xC4, 0xC4)),  # primary
    ((0xF9, 0xB2, 0x9C), (0xE8, 0xE8, 0xE8)),  # trunk
    ((0xE8, 0x92, 0xA2), (0xD7, 0x19, 0x21)),  # motorway: the red accent
    ((0xAA, 0xD3, 0xDF), (0x15, 0x25, 0x2E)),  # water
    ((0xC8, 0xFA, 0xCC), (0x12, 0x21, 0x18)),  # park
    ((0xAD, 0xD1, 0x9E), (0x16, 0x27, 0x18)),  # grass, wood
    ((0x33, 0x33, 0x33), (0xF0, 0xF0, 0xF0)),  # label text
    ((0x77, 0x77, 0x77), (0xA8, 0xA8, 0xA8)),  # grey labels, paths
    ((0xDD, 0xDD, 0xE8), (0x1B, 0x1B, 0x22)),  # pedestrian areas
    ((0xBB, 0xBB, 0xBB), (0x48, 0x48, 0x48)),  # rail, casings
]
# more OSM colours, each drawn as one of the 16 above (shops, borders, brown labels would else match the red)
ALIAS = [
    ((0xFF, 0xD6, 0xD1), 1), ((0xF2, 0xDA, 0xD9), 1), ((0xEB, 0xDB, 0xE8), 1), ((0xE5, 0xDD, 0xD5), 2),
    ((0xD0, 0x8F, 0x55), 13), ((0x73, 0x4A, 0x08), 13), ((0xC7, 0x7C, 0x7C), 13), ((0xE4, 0xA8, 0xA0), 3),
    ((0xAC, 0x39, 0xAC), 13), ((0x8D, 0x8D, 0x8D), 13), ((0xF5, 0xE9, 0xC6), 1), ((0xDE, 0xF6, 0xC0), 10),
]
REF = Image.new("P", (1, 1))
REF.putpalette([v for src, _ in STYLE for v in src] + [v for src, _ in ALIAS for v in src])
TO = list(range(16)) + [i for _, i in ALIAS]
PALETTE = [dst for _, dst in STYLE]


def styled(img):
    """OSM tile → palette indices in STYLE order, speckle merged (anti-aliasing would cost a run per pixel)."""
    q = img.convert("RGB").quantize(palette=REF, dither=Image.Dither.NONE)
    q = q.point(TO + [0] * (256 - len(TO)))
    return q.filter(ImageFilter.ModeFilter(3))


def tokens(px, w, h):
    out = bytearray()
    for y in range(h):
        row = px[y * w:(y + 1) * w]
        x = 0
        while x < w:
            c, n = row[x], 1
            while x + n < w and row[x + n] == c and n < 271:
                n += 1
            out += bytes([c << 4 | (n - 1)]) if n <= 15 else bytes([c << 4 | 15, n - 16])
            x += n
    return bytes(out)


def encode(q):
    """One tile file: scale byte + tokens, at full size if it fits CAP, else 128² drawn doubled."""
    for scale in (1, 2):
        n = TILE // scale
        t = q.resize((n, n), Image.NEAREST) if scale > 1 else q
        data = bytes([scale]) + tokens(t.tobytes(), n, n)
        if len(data) <= CAP:
            return data
    raise ValueError("tile too big")


def build(a):
    out = pathlib.Path(a.out)
    out.mkdir(parents=True, exist_ok=True)
    zooms = list(range(a.min_zoom, a.max_zoom + 1))
    grids, total = [], 0
    for z in zooms:
        cx, cy = world(a.lat, a.lon, z)
        r = a.radius * 1000 / (156543.03 * math.cos(math.radians(a.lat)) / 2**z)  # metres → pixels
        x0, y0 = int((cx - r) // TILE), int((cy - r) // TILE)
        x1, y1 = int((cx + r) // TILE), int((cy + r) // TILE)
        w, h = min(x1 - x0 + 1, 255), min(y1 - y0 + 1, 255)
        grids.append((z, x0, y0, w, h))
        total += w * h
    if total > a.max_tiles:
        sys.exit(f"{total} tiles > --max-tiles {a.max_tiles}: smaller --radius or fewer zooms")
    print(f"{total} tiles, zooms {zooms[0]}..{zooms[-1]}", flush=True)
    imgs, done = {}, 0
    for li, (z, x0, y0, w, h) in enumerate(grids):
        for r in range(h):
            for c in range(w):
                cached = (pathlib.Path(__file__).resolve().parents[1] / "runs/tiles" / f"{z}/{x0 + c}/{y0 + r}.png").exists()
                imgs[li, c, r] = styled(tile(z, x0 + c, y0 + r))
                done += 1
                if not cached:
                    time.sleep(0.3)  # gentle on the OSM servers
        print(f"  z{z}: {w}x{h}", flush=True)
    pal = [v for c in PALETTE for v in c]
    sizes = []
    for (li, c, r), im in imgs.items():
        data = encode(im)
        (out / f"t{li}_{c}_{r}").write_bytes(data)
        sizes.append(len(data))
    idx = bytearray(b"WMP1") + bytes([len(grids)])
    for i in range(16):
        idx += struct.pack("<H", rgb565(*pal[3 * i:3 * i + 3]))
    for z, x0, y0, w, h in grids:
        mm = 156543.03 * math.cos(math.radians(a.lat)) / 2**z * 1000
        idx += struct.pack("<BIIBBI", z, x0, y0, w, h, round(mm))
    cx, cy = world(a.lat, a.lon, WORLD_Z)
    idx += struct.pack("<II", int(cx), int(cy))
    (out / "idx").write_bytes(idx)
    kb = (sum(sizes) + len(idx)) / 1024
    print(f"pack {out}: {len(sizes)} tiles, {kb:.0f} KB (tile avg {sum(sizes) // len(sizes)} B, max {max(sizes)} B)")


async def send_pack(ch, pack):
    """Every tile, then idx (the watch only uses a pack once idx is there); each file acked. → (files, bytes)."""
    pack = pathlib.Path(pack)
    files = sorted(p for p in pack.iterdir() if p.name.startswith("t")) + [pack / "idx"]
    for i, p in enumerate(files):
        if i % 4 == 0:
            await ch.send(b"P" + struct.pack("<HH", i, len(files)))
        await ch.send(b"F" + p.name.encode())
        data = p.read_bytes()
        for k in range(0, len(data), MSG_MAX - 1):
            await ch.send(b"f" + data[k:k + MSG_MAX - 1])
        await ch.send(b"c")
        while (await ch.recv(timeout=60)).data != b"a":
            pass
    await ch.send(b"Z")
    while (await ch.recv(timeout=60)).data != b"k":
        pass
    return len(files), sum(p.stat().st_size for p in files)


async def send(a):
    ch_ = SimChannel(a.sim) if a.sim else Channel(address=a.address, hci_port=a.hci_port, channel_key=True)
    t0 = time.monotonic()
    async with ch_ as ch:
        n, total = await send_pack(ch, a.pack)
    dt = time.monotonic() - t0
    print(f"sent {n} files, {total // 1024} KB in {dt:.1f} s ({total / dt / 1024:.1f} KB/s)")


def main():
    p = argparse.ArgumentParser()
    sub = p.add_subparsers(dest="cmd", required=True)
    b = sub.add_parser("build")
    b.add_argument("--lat", type=float, required=True)
    b.add_argument("--lon", type=float, required=True)
    b.add_argument("--radius", type=float, default=1.5, help="km round the point")
    b.add_argument("--min-zoom", type=int, default=13)
    b.add_argument("--max-zoom", type=int, default=17)
    b.add_argument("--max-tiles", type=int, default=300)
    b.add_argument("--out", required=True)
    s = sub.add_parser("send")
    s.add_argument("pack")
    s.add_argument("--sim", type=int)
    s.add_argument("--address")
    s.add_argument("--hci-port", type=int)
    a = p.parse_args()
    build(a) if a.cmd == "build" else asyncio.run(send(a))


if __name__ == "__main__":
    main()
