"""A stand-in phone for the Maps app (examples/atlas, proto.rs): made-up places, a made-up route and the map as data
(scene.rs), so the watch side runs without CMFX. The map is a made-up town (streets with names, a park, a lake,
places with pins) or, with --real LAT LON, OpenStreetMap's streets there (hosts/atlasscene.py, OpenFreeMap tiles).
CMFX does the real thing (cmfx ios/App/Hub/Atlas.swift).

  scripts/sim atlas --tcp 7706 &  uv run python hosts/atlashost.py --sim 7706
  uv run --with mapbox-vector-tile python hosts/atlashost.py --sim 7706 --real 6.9130 79.8619
  uv run python hosts/atlashost.py --address AA:BB:CC:DD:EE:FF

The scenes' data chunks go shuffled (they carry their place: scene.rs `d`), over the sliding window on a watch.
"""
import argparse, asyncio, pathlib, random, struct, sys

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parents[1] / "src"))
sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
from wp2apps.channel import Channel, Pipe, SimChannel  # noqa: E402
import atlasscene as sc  # noqa: E402

CHUNK = 124
# (name, sub, metres from us, where in the frame)
PLACES = [("Viharamahadevi Park", "Colombo 07", 1840, (-680, 1160)), ("Park Street Mews", "Colombo 02", 2410, (420, 900)),
          ("Galle Face Green", "Colombo 03", 3020, (-900, -300)), ("Beira Lake", "Colombo 02", 2900, (500, -450)),
          ("Independence Square", "Colombo 07", 3500, (-200, 1400)), ("Diyatha Uyana", "Battaramulla", 8800, (1300, 200))]
INFO = ["Park", "Ananda Coomaraswamy Mawatha, Colombo 07", "+94 11 000 0000"]
# the ways there to pick, by car: (secs, metres, via)
ALTS = [(540, 1840, "Galle Road"), (660, 2100, "Duplication Road")]
# (at metres, kind, dir, street): nav state.rs kinds and dirs
STEPS = [(0, 3, 0, "Galle Road"), (420, 0, 2, "Dharmapala Mawatha"), (1100, 0, 5, "Green Path"),
         (1840, 2, 0, "Arrive")]
SECS = {0: 540, 1: 660, 2: 1500}
# the route, frame metres (east, north): its legs as the steps say
ROUTE = [(0, 0), (0, 420), (-680, 420), (-680, 1160)]
R = 1500  # half a scene's side, metres


def town(centre, r):
    """A made-up town as atlasscene features: streets every 200 m (main ones every 600 m, named), a park, a lake,
    places of interest."""
    feats = []
    cx, cy = centre
    lo, hi = (cx - r) // 200 * 200, (cx + r) // 200 * 200 + 200
    for k in range(lo, hi + 1, 200):
        main = k % 600 == 0
        cls = sc.PRIMARY if k == 0 else sc.SECONDARY if main else sc.MINOR
        ns = [(k, cy - r), (k, cy + r)]
        ew = [(cx - r, k + 30), (cx + r, k + 30)]
        nn = "Galle Road" if k == 0 else f"{abs(k) // 200} {'East' if k > 0 else 'West'} Lane"
        en = "Dharmapala Mawatha" if k == 400 else f"{abs(k) // 200} {'North' if k > 0 else 'South'} Road"
        for pts, name in ((ns, nn), (ew, en)):
            feats.append(("line", cls, pts, None, 0))
            feats.append(("carrier", cls, pts, name, 0))
    feats.append(("area", sc.PARK, [(-900, 900), (-500, 900), (-500, 1300), (-900, 1300)], None, 0))
    feats.append(("area", sc.WATER, [(300, -700), (700, -620), (760, -300), (420, -200), (260, -420)], None, 0))
    feats.append(("label", sc.WATER_NAME, (500, -450), "Beira Lake", 1))
    feats.append(("label", sc.PLACE, (-700, 600), "Cinnamon Gardens", 1))
    for i, (x, y, n, k) in enumerate([(120, 90, "Cafe Kumbuk", sc.FOOD), (-240, 360, "Odel", sc.SHOP),
                                      (-700, 1100, "Town Hall", sc.LEARN), (260, 620, "Majestic City", sc.SHOP),
                                      (-120, -260, "Barefoot", sc.CULTURE)]):
        feats.append(("label", k, (x, y), n, i))
    return feats


class Host:
    def __init__(self, ch, log=print, real=None, blob=None):
        self.ch, self.log, self.real, self.blob = ch, log, real, blob
        self.last: tuple[int, list[bytes]] | None = None
        self.mode = 0
        self.navigating = False

    async def send(self, msgs: list[bytes], shuffle=False):
        if shuffle:
            head = [m for m in msgs if m[:1] != b"d"]
            tail = [m for m in msgs if m[:1] == b"d"]
            random.shuffle(tail)
            msgs = head + tail
        if isinstance(self.ch, Channel):
            await Pipe(await self.ch.connect()).stream(msgs)
        else:
            for m in msgs:
                await self.ch.send(m)

    async def scene(self, sid: int, centre, me=(0, 0), r=R):
        if self.blob:
            blob, r = self.blob, 1500
        elif self.real:
            blob, r = sc.scene_round(*self.real, centre)
        else:
            blob = sc.build(town(centre, r))
        msgs = [b"S" + bytes([sid]) + struct.pack("<HhhhhH", len(blob), *me, *centre, r)]
        msgs += [b"d" + bytes([sid]) + struct.pack("<H", k) + blob[i:i + CHUNK]
                 for k, i in enumerate(range(0, len(blob), CHUNK))]
        self.last = (sid, msgs)
        self.log(f"scene {sid} round {centre}: {len(msgs)} messages, {len(blob)} B")
        await self.send(msgs, shuffle=True)

    async def command(self, m: bytes):
        op = m[:1]
        if op == b"R":
            await self.ch.send(b"A")
        elif op == b"H" and len(m) == 7:
            here = m[2] == 1
            x, y = struct.unpack("<hh", m[3:7])
            self.log("map here" if here else f"map round {x} {y}")
            await self.scene(m[1], (0, 0) if here else (x, y))
        elif op in (b"Q", b"C") and len(m) >= 3:
            what = m[2:].decode(errors="replace") if op == b"Q" else f"category {m[2]}"
            self.log(f"search {what!r}")
            if m[2:] == b"pa":
                # a mixed search: two places round us, then a town far away (Matara) and its places, unplaced
                self.group = 0
                await self.send(self.mixed())
                await self.scene(m[1], (0, 400))
            else:
                await self.send(self.results(PLACES))
                await self.scene(m[1], (0, 400))
        elif op == b"F" and len(m) == 3:
            self.group = 0 if m[2] < 2 else 1
            self.log(f"group {self.group} for row {m[2]}")
            await self.send(self.mixed())
            await self.scene(m[1], (0, 400) if self.group == 0 else (0, 0))
        elif op == b"Z" and len(m) >= 3:
            self.log(f"area {m[2:].decode(errors='replace')!r}")
            await self.send(self.results([("Cinnamon Gardens", "Colombo 07", 2200, (-700, 600))]))
            await self.scene(m[1], (-700, 600))
        elif op == b"P" and len(m) >= 5:
            x, y = struct.unpack("<hh", m[1:5])
            name = m[5:].decode(errors="replace") or "Dropped pin"
            self.log(f"picked {name!r} at {x} {y}")
            await self.send([b"l\x08" + struct.pack("<Ihh", 900, x, y) + name.encode() + b"\0" + b"Galle Road, Colombo 03"]
                            + self.info(8))
        elif op == b"K":
            self.log("search cleared")
        elif op == b"I" and len(m) == 2:
            await self.send(self.info(m[1]))
        elif op == b"U" and len(m) == 2:
            self.log(f"ways to {m[1]}")
            # car and foot have a route, the bike none (as Wear OS shows it)
            await self.send([b"u" + bytes([mode, ok]) + struct.pack("<II", SECS.get(mode, 600), 1840) + bytes([14, 32])
                             for mode, ok in ((0, 1), (1, 0), (2, 1))])
        elif op == b"D" and len(m) == 4 and m[1] == 0:
            self.log("ways there to pick")
            await self.send([b"O" + bytes([len(ALTS)])] + [b"o" + bytes([k]) + struct.pack("<II", t, d) + bytes([14, 32])
                                                          + via.encode() for k, (t, d, via) in enumerate(ALTS)])
            # each way's line: the route, and one round the other side of the block (w k part unit points)
            lines = [ROUTE, [(0, 0), (600, 0), (600, 1160), (-680, 1160)], [(0, 0), (-680, 0), (-680, 1160)]]
            await self.send([b"w" + bytes([k, 0]) + struct.pack("<H", 1) + b"".join(struct.pack("<hh", x, y) for x, y in pts)
                             for k, pts in enumerate(lines[:len(ALTS)])])
        elif op == b"D" and len(m) in (4, 5):
            self.mode = m[1]
            self.log(f"route mode {m[1]} to {m[2]}" + (f", way {m[4]}" if len(m) == 5 else ""))
            await self.send([b"T" + bytes([m[1]]) + struct.pack("<II", SECS.get(m[1], 600), 1840) + bytes([len(STEPS), 14, 32])]
                            + [b"t" + bytes([k, kind, d]) + struct.pack("<I", at) + name.encode()
                               for k, (at, kind, d, name) in enumerate(STEPS)] + self.route())
            await self.scene(m[3], (-340, 580))
        elif op == b"S":
            self.log("start")
            await self.send([b"N", b"V" + bytes([1, 0, 2]) + struct.pack("<HHH", 380, 9, 184) + bytes([14, 32])
                             + b"Dharmapala Mawatha"] + self.route() + [b"X" + struct.pack("<hhH", 0, 40, 0)])
        elif op == b"X":
            self.log("stop")
        elif op == b"g" and len(m) == 2 and self.last and self.last[0] == m[1]:
            self.log(f"again {m[1]}")
            await self.send(self.last[1], shuffle=True)

    MIXED = [("Matara Bath Kade", "Galle Road, Colombo 06", 2600, (300, 900), 0),
             ("Matara Hotel", "Wellawatte", 3100, (-200, 1300), 0),
             ("Matara", "Southern Province", 140_000, (0, 0), 1),
             ("Matara Bus Stand", "Matara", 140_300, (300, -200), 1),
             ("Matara Fort", "Matara", 140_600, (-500, 300), 1)]

    def mixed(self):
        rows = [b"L" + bytes([len(self.MIXED), 2])]
        for i, (n, s, d, at, g) in enumerate(self.MIXED):
            at = at if g == getattr(self, "group", 0) else (-32768, -32768)
            rows.append(b"l" + bytes([i]) + struct.pack("<Ihh", d, *at) + n.encode() + b"\0" + s.encode())
        return rows

    def results(self, places):
        return [b"L" + bytes([len(places)])] + [
            b"l" + bytes([i]) + struct.pack("<Ihh", d, *at) + n.encode() + b"\0" + s.encode()
            for i, (n, s, d, at) in enumerate(places)]

    def info(self, idx):
        about = ("A green in the middle of the town, with a pond, old trees and a path round it. Open from sunrise "
                 "to sunset; the café by the gate serves tea and short eats.")
        return [b"i" + bytes([idx, k]) + t.encode() for k, t in enumerate(INFO)] + \
            [b"i" + bytes([idx, 4 + j // 100]) + about[j:j + 100].encode() for j in range(0, len(about), 100)]

    def route(self):
        return [b"G" + struct.pack("<H", len(ROUTE)),
                b"g" + struct.pack("<H", 0) + b"".join(struct.pack("<hh", *p) for p in ROUTE)]

    async def run(self):
        while True:
            m = (await self.ch.recv()).data
            await self.command(m)


async def main():
    p = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    g = p.add_mutually_exclusive_group(required=True)
    g.add_argument("--sim", type=int, help="scripts/sim atlas --tcp PORT")
    g.add_argument("--address", help="the watch's BLE address")
    p.add_argument("--real", nargs=2, type=float, metavar=("LAT", "LON"), help="OpenStreetMap's streets round there")
    p.add_argument("--blob", type=pathlib.Path, help="a scene's bytes to send as every map (CMFX's AtlasScene output)")
    a = p.parse_args()
    async with (SimChannel(a.sim) if a.sim else Channel(address=a.address, channel_key=True)) as ch:
        await Host(ch, real=tuple(a.real) if a.real else None, blob=a.blob.read_bytes() if a.blob else None).run()


if __name__ == "__main__":
    asyncio.run(main())
