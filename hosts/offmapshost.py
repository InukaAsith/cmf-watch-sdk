"""A stand-in phone for Offline Maps' directions (examples/maps trip.rs), so the simulator gets real routes: each
`D mode …` is asked of OpenStreetMap's routing (FOSSGIS OSRM, car / bike / foot, with alternatives) from this Mac,
and answered as CMFX does (`O`, `o`); `p mode k` sends that route's turns (`S`, `s`) and points (`G`, `g`). The map
itself is saved in the simulator's storage beforehand (CMFX's pack code, or the sim store of a test). CMFX does the
real thing (cmfx ios/App/Hub/Maps.swift).

  scripts/sim maps --tcp 7795 --store DIR &  uv run python hosts/offmapshost.py --sim 7795
"""
import argparse, asyncio, json, math, pathlib, struct, sys, urllib.request

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parents[1] / "src"))
from wp2apps.channel import Channel, SimChannel  # noqa: E402

PROFILES = ["car", "bike", "foot"]
ROUTE_MAX, PER_MSG = 1500, 30
# OSRM modifier → nav state.rs dir
DIRS = {"straight": 0, "slight left": 1, "left": 2, "sharp left": 3, "slight right": 4, "right": 5, "sharp right": 6,
        "uturn": 7}


def xy(lat, lon, o):
    return ((lon - o[1]) * math.cos(math.radians(o[0])) * 111_320, (lat - o[0]) * 110_540)


def i16(v):
    return max(-32000, min(32000, round(v)))


def kind_dir(m):
    """OSRM's manoeuvre → (kind, dir) as nav state.rs: 0 turn, 1 roundabout, 2 arrive, 3 straight on, 4 fork."""
    t, d = m.get("type", "turn"), DIRS.get(m.get("modifier", "straight"), 0)
    if t == "arrive":
        return 2, 0
    if t in ("roundabout", "rotary", "roundabout turn", "exit roundabout", "exit rotary"):
        return 1, d
    if t in ("fork", "on ramp", "off ramp", "merge"):
        return 4, d
    if t in ("continue", "new name") and d == 0:
        return 3, 0
    return 0, d


def osrm(profile, a, b):
    url = (f"https://routing.openstreetmap.de/routed-{profile}/route/v1/driving/{a[1]:.6f},{a[0]:.6f};{b[1]:.6f},"
           f"{b[0]:.6f}?alternatives=3&overview=full&geometries=geojson&steps=true")
    req = urllib.request.Request(url, headers={"User-Agent": "wp2-sim offline maps host"})
    with urllib.request.urlopen(req, timeout=15) as r:
        j = json.load(r)
    return j.get("routes", []) if j.get("code") == "Ok" else []


def messages(route, o):
    """`S`, `s` each turn, then `G`, `g`: the route in the frame round `o`."""
    out, steps = [], []
    for leg in route["legs"]:
        for st in leg["steps"]:
            m = st["maneuver"]
            if m.get("type") == "depart":
                continue
            kind, d = kind_dir(m)
            x, y = xy(m["location"][1], m["location"][0], o)
            steps.append((kind, d, x, y, "" if kind == 2 else st.get("name", "")))
    steps = steps[:64]
    out.append(b"S" + bytes([len(steps)]))
    for k, (kind, d, x, y, name) in enumerate(steps):
        out.append(b"s" + bytes([k, kind, d]) + struct.pack("<hh", i16(x), i16(y)) + name.encode()[:32])
    pts = [xy(lat, lon, o) for lon, lat in route["geometry"]["coordinates"]]
    step = max(1, math.ceil(len(pts) / ROUTE_MAX))
    pts = pts[::step] + ([pts[-1]] if (len(pts) - 1) % step else [])
    out.append(b"G" + struct.pack("<HII", len(pts), int(route["distance"]), int(route["duration"])))
    for k, i in enumerate(range(0, len(pts), PER_MSG)):
        out.append(b"g" + bytes([k]) + b"".join(struct.pack("<hh", i16(x), i16(y)) for x, y in pts[i:i + PER_MSG]))
    return out


async def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--sim", type=int, required=True)
    a = ap.parse_args()
    found = {}
    async with SimChannel(a.sim) as ch:
        while True:
            m = (await ch.recv()).data
            if m[:1] == b"D" and len(m) == 18:
                mode = m[1]
                lat0, lon0 = struct.unpack_from("<ii", m, 2)
                o = (lat0 / 1e7, lon0 / 1e7)
                fx, fy, tx, ty = struct.unpack_from("<hhhh", m, 10)
                to_ll = (o[0] + ty / 110_540, o[1] + tx / (math.cos(math.radians(o[0])) * 111_320))
                fr_ll = o if fx == -32768 else (o[0] + fy / 110_540, o[1] + fx / (math.cos(math.radians(o[0])) * 111_320))
                print(f"D {PROFILES[min(mode, 2)]}", flush=True)
                try:
                    routes = await asyncio.to_thread(osrm, PROFILES[min(mode, 2)], fr_ll, to_ll)
                except Exception as e:
                    print("osrm:", e, flush=True)
                    routes = []
                found[mode] = (routes[:3], o)
                await ch.send(b"O" + bytes([mode, len(routes[:3])]))
                for k, r in enumerate(routes[:3]):
                    await ch.send(b"o" + bytes([mode, k]) + struct.pack("<II", int(r["duration"]), int(r["distance"])))
            elif m[:1] == b"p" and len(m) == 3 and m[1] in found and m[2] < len(found[m[1]][0]):
                routes, o = found[m[1]]
                print(f"p {m[1]} {m[2]}", flush=True)
                for x in messages(routes[m[2]], o):
                    await ch.send(x)


if __name__ == "__main__":
    asyncio.run(main())
