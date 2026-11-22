"""Phone side of the nav app: a route (OSRM's public demo server for testing; Google Routes later), then a drive
along it, sending the next manoeuvre and where we are every tick (examples/nav/src/state.rs has the protocol).

  uv run python hosts/navhost.py --sim 7705                       # Trafalgar Square → British Museum, 12 m/s
  uv run python hosts/navhost.py --sim 7705 --speed 25 --from 51.5007,-0.1246 --to 51.5194,-0.1270
  uv run python hosts/navhost.py --sim 7705 --wait     # the watch starts it: recent places, a search, a pick
"""
import argparse, asyncio, datetime, json, math, pathlib, struct, sys, urllib.request

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parents[1] / "src"))
from wp2apps.channel import Channel, SimChannel, send_when_open  # noqa: E402

CACHE = pathlib.Path(__file__).resolve().parents[1] / "runs" / "routes"
DIRS = {"straight": 0, "slight left": 1, "left": 2, "sharp left": 3, "slight right": 4, "right": 5,
        "sharp right": 6, "uturn": 7}


def osrm(a, b):
    key = f"{a[0]:.5f},{a[1]:.5f}_{b[0]:.5f},{b[1]:.5f}"
    p = CACHE / f"{key}.json"
    if not p.exists():
        url = (f"https://router.project-osrm.org/route/v1/driving/{a[1]},{a[0]};{b[1]},{b[0]}"
               "?overview=full&geometries=geojson&steps=true")
        req = urllib.request.Request(url, headers={"User-Agent": "wp2-nav-dev/0.1"})
        data = urllib.request.urlopen(req, timeout=20).read()
        CACHE.mkdir(parents=True, exist_ok=True)
        p.write_bytes(data)
    return json.loads(p.read_text())["routes"][0]


def kind_of(step):
    t = step["maneuver"]["type"]
    if t == "arrive":
        return 2
    if t in ("roundabout", "rotary", "roundabout turn", "exit roundabout", "exit rotary"):
        return 1
    if t in ("fork", "merge", "on ramp", "off ramp"):
        return 4
    if t in ("depart", "continue", "new name") and step["maneuver"].get("modifier", "straight") == "straight":
        return 3
    return 0


def simplify(pts, tol):
    """Douglas-Peucker on (x, y) metres."""
    if len(pts) < 3:
        return pts
    (x0, y0), (x1, y1) = pts[0], pts[-1]
    dx, dy = x1 - x0, y1 - y0
    n = math.hypot(dx, dy) or 1e-9
    i, d = max(((i, abs(dy * (x - x0) - dx * (y - y0)) / n) for i, (x, y) in enumerate(pts[1:-1], 1)),
               key=lambda t: t[1])
    if d <= tol:
        return [pts[0], pts[-1]]
    return simplify(pts[: i + 1], tol)[:-1] + simplify(pts[i:], tol)


class Route:
    def __init__(self, r):
        coords = r["geometry"]["coordinates"]  # lon, lat
        self.lat0, self.lon0 = coords[0][1], coords[0][0]
        self.pts = [self.local(lat, lon) for lon, lat in coords]
        self.cum = [0.0]
        for (ax, ay), (bx, by) in zip(self.pts, self.pts[1:]):
            self.cum.append(self.cum[-1] + math.hypot(bx - ax, by - ay))
        self.length = self.cum[-1]
        self.steps = []
        for st in r["legs"][0]["steps"]:
            lon, lat = st["maneuver"]["location"]
            x, y = self.local(lat, lon)
            i = min(range(len(self.pts)), key=lambda i: (self.pts[i][0] - x) ** 2 + (self.pts[i][1] - y) ** 2)
            name = st.get("name") or st.get("ref") or ""
            if kind_of(st) == 1 and st["maneuver"].get("exit"):
                name = f"Exit {st['maneuver']['exit']} {name}".strip()
            self.steps.append((self.cum[i], kind_of(st), DIRS.get(st["maneuver"].get("modifier", "straight"), 0),
                               name or "Continue"))
        tol = 2.0
        self.shape = simplify(self.pts, tol)
        while len(self.shape) > 400:
            tol *= 1.6
            self.shape = simplify(self.pts, tol)

    def local(self, lat, lon):
        return ((lon - self.lon0) * math.cos(math.radians(self.lat0)) * 111320, (lat - self.lat0) * 110540)

    def at(self, s):
        """(x, y, heading°) at s metres along."""
        s = max(0.0, min(s, self.length))
        i = max(0, min(len(self.cum) - 2, next((k for k, c in enumerate(self.cum) if c > s), len(self.cum) - 1) - 1))
        (ax, ay), (bx, by) = self.pts[i], self.pts[i + 1]
        seg = (self.cum[i + 1] - self.cum[i]) or 1e-9
        t = (s - self.cum[i]) / seg
        return ax + (bx - ax) * t, ay + (by - ay) * t, math.degrees(math.atan2(bx - ax, by - ay)) % 360

    def next_step(self, s):
        for idx, st in enumerate(self.steps[1:], 1):
            if st[0] > s + 3:
                return idx, st
        return len(self.steps) - 1, self.steps[-1]


async def send_route(ch, route):
    pts = route.shape
    await ch.send(b"G" + struct.pack("<H", len(pts)))
    for off in range(0, len(pts), 29):
        chunk = pts[off : off + 29]
        await ch.send(b"g" + struct.pack("<H", off) + b"".join(struct.pack("<hh", round(x), round(y)) for x, y in chunk))


async def drive(ch, route, speed, tick):
    s = 0.0
    while s < route.length:
        x, y, hdg = route.at(s)
        idx, (at, kind, d, name) = route.next_step(s)
        left = route.length - s
        secs = left / speed
        eta = datetime.datetime.now() + datetime.timedelta(seconds=secs)
        await ch.send(b"X" + struct.pack("<hhH", round(x), round(y), round(hdg)))
        await ch.send(b"V" + bytes([idx & 0xFF, kind, d]) + struct.pack("<HHH", min(65535, round(at - s)),
                      round(secs / 60), min(65535, round(left / 10))) + bytes([eta.hour, eta.minute])
                      + name.encode()[:110])
        await asyncio.sleep(tick)
        s += speed * tick
    await ch.send(b"W")


RECENT = [("British Museum", "Great Russell St, London"), ("Covent Garden", "London WC2"),
          ("King's Cross", "Euston Rd, London")]
FOUND = [("Russell Square", "Bloomsbury, London"), ("Russell Square Gardens", "London WC1"),
         ("Russell Hotel", "Russell Sq, London")]


def places(rows, walking=True):
    return [b"P" + bytes([len(rows), walking])] + [b"p" + bytes([i]) + n.encode() + b"\0" + d.encode()
                                                    for i, (n, d) in enumerate(rows)]


async def start_screen(ch, route, a):
    """The watch's start screen (examples/nav/src/start.rs): recent places, a search, a pick starts the drive."""
    driving = None
    for m in places(RECENT):
        await ch.send(m)
    while True:
        m = (await ch.recv()).data
        if m == b"R":
            for x in ([] if driving else places(RECENT)):
                await ch.send(x)
            if driving:
                await send_route(ch, route)
        elif m[:1] == b"Q":
            print(f"search {m[1:].decode(errors='replace')!r}", flush=True)
            await ch.send(b"E" + "Searching\u2026".encode())
            for x in places(FOUND):
                await ch.send(x)
        elif m[:1] == b"S" and len(m) == 3 and not driving:
            print(f"start {m[1]} {'walking' if m[2] else 'roads'}", flush=True)
            await ch.send(b"E" + "Finding a route\u2026".encode())
            await send_route(ch, route)
            driving = asyncio.create_task(drive(ch, route, a.speed, a.tick))


async def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--sim", type=int)
    ap.add_argument("--address", help="a real watch (K channel)")
    ap.add_argument("--from", dest="a", default="51.50797,-0.12804")  # Trafalgar Square
    ap.add_argument("--to", dest="b", default="51.51939,-0.12697")    # British Museum
    ap.add_argument("--speed", type=float, default=12.0, help="m/s")
    ap.add_argument("--tick", type=float, default=1.0)
    ap.add_argument("--wait", action="store_true", help="drive once the watch picks a place (its start screen)")
    a = ap.parse_args()
    route = Route(osrm(tuple(map(float, a.a.split(","))), tuple(map(float, a.b.split(",")))))
    print(f"route {route.length:.0f} m, {len(route.steps)} steps, {len(route.shape)} points", flush=True)
    async with (SimChannel(a.sim) if a.sim else Channel(address=a.address, channel_key=True)) as ch:
        await send_when_open(ch, b"?")  # waits till the app is open
        if a.wait:
            return await start_screen(ch, route, a)
        await send_route(ch, route)
        task = asyncio.create_task(drive(ch, route, a.speed, a.tick))
        while True:
            m = (await ch.recv()).data
            if m == b"R":
                await send_route(ch, route)


if __name__ == "__main__":
    asyncio.run(main())
