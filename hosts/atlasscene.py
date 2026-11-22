"""Scenes for the Maps app (sdk/examples/atlas/src/scene.rs has the format): OpenStreetMap's streets, water, parks,
places and street names round a point, cut from OpenFreeMap's vector tiles (OpenMapTiles schema, zoom 14), in
metres of a frame, sized to the watch's store. CMFX does the same in Swift (HubKit AtlasScene).

  uv run --with mapbox-vector-tile python hosts/atlasscene.py 6.9130 79.8619 --r 1500 --out runs/scene.bin

Tiles are cached in runs/vtiles (OpenFreeMap: no key, no limit; still one at a time).
"""
import argparse, gzip, json, math, pathlib, struct, urllib.request

CAP = 30 * 1024
MAX_RING = 512
AREA, LINE, NAME, LABEL = 0x10, 0x20, 0x30, 0x40
WATER, PARK = 0, 1
MOTORWAY, PRIMARY, SECONDARY, TERTIARY, MINOR, PATH, RAIL, WATERWAY = range(8)
CARRIER = 8
PLACE, POI, WATER_NAME = 0, 1, 2
# kinds of places of interest (scene.rs poi_kind): label class 3 + kind - 1
FOOD, SHOP, GREEN_POI, TRANSIT, HEALTH, LEARN, STAY, CULTURE, MONEY, CAR, SEARCHED = range(3, 14)
POI_KIND = {
    **dict.fromkeys(["restaurant", "cafe", "fast_food", "bar", "beer", "ice_cream", "bakery", "food_court", "pub"], FOOD),
    **dict.fromkeys(["shop", "grocery", "clothing_store", "supermarket", "jewelry", "mobile_phone", "books",
                     "hardware", "furniture", "electronics", "alcohol_shop", "convenience", "department_store",
                     "gift", "shoes", "marketplace", "florist", "laundry", "hairdresser"], SHOP),
    **dict.fromkeys(["park", "garden", "playground", "pitch", "sports", "stadium", "swimming", "golf", "zoo",
                     "dog_park", "campsite"], GREEN_POI),
    **dict.fromkeys(["bus", "railway", "ferry_terminal", "airport", "aerialway", "harbor", "bicycle_rental",
                     "taxi"], TRANSIT),
    **dict.fromkeys(["hospital", "doctors", "pharmacy", "dentist", "veterinary", "clinic"], HEALTH),
    **dict.fromkeys(["school", "college", "university", "library", "kindergarten", "place_of_worship",
                     "town_hall", "police", "fire_station", "post", "embassy", "townhall"], LEARN),
    **dict.fromkeys(["lodging", "hotel", "hostel", "motel", "guest_house"], STAY),
    **dict.fromkeys(["museum", "attraction", "art_gallery", "theatre", "cinema", "monument", "castle",
                     "music", "entertainment"], CULTURE),
    **dict.fromkeys(["bank", "atm", "money"], MONEY),
    **dict.fromkeys(["fuel", "parking", "car", "car_repair", "charging_station"], CAR),
}
Z = 14
CACHE = pathlib.Path(__file__).resolve().parents[1] / "runs" / "vtiles"
TILEJSON = "https://tiles.openfreemap.org/planet"

ROAD = {"motorway": MOTORWAY, "trunk": MOTORWAY, "primary": PRIMARY, "secondary": SECONDARY, "tertiary": TERTIARY,
        "minor": MINOR, "service": MINOR, "residential": MINOR, "unclassified": MINOR, "living_street": MINOR,
        "raceway": MINOR, "busway": MINOR, "path": PATH, "track": PATH, "pedestrian": PATH, "footway": PATH,
        "cycleway": PATH, "steps": PATH, "bridleway": PATH, "rail": RAIL, "transit": RAIL}
GREEN = {"grass", "wood", "park", "garden", "forest", "meadow", "recreation_ground", "cemetery", "golf_course"}


# --- the frame: metres east / north of an origin (as NavService.local)

def to_xy(lat, lon, lat0, lon0):
    return ((lon - lon0) * math.cos(math.radians(lat0)) * 111_320, (lat - lat0) * 110_540)


def to_ll(x, y, lat0, lon0):
    return (lat0 + y / 110_540, lon0 + x / (math.cos(math.radians(lat0)) * 111_320))


def tile_of(lat, lon, z=Z):
    n = 2 ** z
    return (int((lon + 180) / 360 * n), int((1 - math.asinh(math.tan(math.radians(lat))) / math.pi) / 2 * n))


def tile_ll(tx, ty, u, v, z=Z, extent=4096):
    """A tile point (u right, v down in 0..extent) as lat, lon."""
    n = 2 ** z
    lon = (tx + u / extent) / n * 360 - 180
    lat = math.degrees(math.atan(math.sinh(math.pi * (1 - 2 * (ty + v / extent) / n))))
    return lat, lon


def fetch(tx, ty, z=Z) -> bytes:
    p = CACHE / f"{z}-{tx}-{ty}.pbf"
    if not p.exists():
        CACHE.mkdir(parents=True, exist_ok=True)
        tj = CACHE / "tilejson.json"
        if not tj.exists():
            tj.write_bytes(urllib.request.urlopen(urllib.request.Request(TILEJSON, headers={"User-Agent": "wp2-atlas-dev"}), timeout=30).read())
        url = json.loads(tj.read_text())["tiles"][0].format(z=z, x=tx, y=ty)
        p.write_bytes(urllib.request.urlopen(urllib.request.Request(url, headers={"User-Agent": "wp2-atlas-dev"}), timeout=30).read())
    raw = p.read_bytes()
    return gzip.decompress(raw) if raw[:2] == b"\x1f\x8b" else raw


# --- shapes

def simplify(pts, tol):
    """Douglas–Peucker, ends kept."""
    if len(pts) < 3:
        return list(pts)
    keep = [False] * len(pts)
    keep[0] = keep[-1] = True
    stack = [(0, len(pts) - 1)]
    while stack:
        a, b = stack.pop()
        (ax, ay), (bx, by) = pts[a], pts[b]
        dx, dy = bx - ax, by - ay
        l2 = dx * dx + dy * dy
        best, at = -1.0, -1
        for i in range(a + 1, b):
            px, py = pts[i]
            if l2 == 0:
                d = math.hypot(px - ax, py - ay)
            else:
                d = abs(dy * (px - ax) - dx * (py - ay)) / math.sqrt(l2)
            if d > best:
                best, at = d, i
        if best > tol:
            keep[at] = True
            stack += [(a, at), (at, b)]
    return [p for p, k in zip(pts, keep) if k]


def clip_line(pts, lo, hi):
    """The pieces of a line near the square [lo, hi]: runs of segments with an end inside."""
    inside = lambda p: lo[0] <= p[0] <= hi[0] and lo[1] <= p[1] <= hi[1]
    out, cur = [], []
    for a, b in zip(pts, pts[1:]):
        if inside(a) or inside(b):
            if not cur:
                cur = [a]
            cur.append(b)
        elif cur:
            out.append(cur)
            cur = []
    if cur:
        out.append(cur)
    return out


def enc_pts(pts) -> bytes:
    pts = [(max(-32000, min(32000, round(x))), max(-32000, min(32000, round(y)))) for x, y in pts]
    out = bytearray(struct.pack("<hh", *pts[0]))
    for (ax, ay), (bx, by) in zip(pts, pts[1:]):
        dx, dy = bx - ax, by - ay
        if -127 <= dx <= 127 and -127 <= dy <= 127:
            out += struct.pack("<bb", dx, dy)
        else:
            out += b"\x80" + struct.pack("<hh", dx, dy)
    return bytes(out)


SHORT = [("Mawatha", "Mw"), ("Road", "Rd"), ("Street", "St"), ("Avenue", "Ave"), ("Place", "Pl"), ("Lane", "Ln"),
         ("Drive", "Dr"), ("Terrace", "Terr"), ("Gardens", "Gdns"), ("Boulevard", "Blvd"), ("Highway", "Hwy"),
         ("Junction", "Jn"), ("Roundabout", "Rbt"), ("Expressway", "Expy"), ("Mawatta", "Mw")]


def street(name: str) -> str:
    """A street's name as a map writes it: no bracketed other name, the usual short words."""
    import re
    name = re.sub(r"\s*\(.*?\)", "", name).strip()
    words = name.split()
    return " ".join(dict(SHORT).get(w, w) if i > 0 else w for i, w in enumerate(words))


def text(s: str, n=60) -> bytes:
    b = s.encode()[:n]
    while b:
        try:
            b.decode()
            return b
        except UnicodeDecodeError:
            b = b[:-1]
    return b


# --- features from tiles

def features(lat0, lon0, centre, r, tiles):
    """(kind, class, pts or point, name, rank) round `centre` (frame metres), half side `r`, from decoded tiles."""
    lo, hi = (centre[0] - r, centre[1] - r), (centre[0] + r, centre[1] + r)
    out = []

    def geo(tx, ty, ring, extent):
        return [to_xy(*tile_ll(tx, ty, u, extent - v, extent=extent), lat0, lon0) for u, v in ring]

    for (tx, ty), t in tiles.items():
        for lname, layer in t.items():
            extent = layer.get("extent", 4096)
            for f in layer["features"]:
                p, g = f["properties"], f["geometry"]
                kind, coords = g["type"], g["coordinates"]
                if lname in ("water", "park", "landcover"):
                    if lname == "water" and p.get("class") == "swimming_pool":
                        continue
                    if lname == "landcover" and p.get("class") not in GREEN and p.get("subclass") not in GREEN:
                        continue
                    cls = WATER if lname == "water" else PARK
                    polys = [coords] if kind == "Polygon" else coords if kind == "MultiPolygon" else []
                    for poly in polys:
                        ring = geo(tx, ty, poly[0], extent)
                        xs, ys = [q[0] for q in ring], [q[1] for q in ring]
                        if max(xs) < lo[0] or min(xs) > hi[0] or max(ys) < lo[1] or min(ys) > hi[1]:
                            continue
                        out.append(("area", cls, ring, None, 0))
                elif lname in ("transportation", "transportation_name", "waterway"):
                    if lname == "waterway":
                        if p.get("class") not in ("river", "canal", "stream"):
                            continue
                        cls = WATERWAY
                    else:
                        cls = ROAD.get(p.get("class"), ROAD.get(p.get("subclass")))
                        if cls is None:
                            continue
                    lines = [coords] if kind == "LineString" else coords if kind == "MultiLineString" else []
                    name = p.get("name") if lname == "transportation_name" else None
                    if lname == "transportation_name" and not name:
                        continue
                    for l in lines:
                        for piece in clip_line(geo(tx, ty, l, extent), lo, hi):
                            out.append(("carrier" if name else "line", cls, piece, name, 0))
                elif lname in ("poi", "place", "water_name"):
                    name = p.get("name")
                    if not name or kind != "Point":
                        continue
                    x, y = geo(tx, ty, [coords], extent)[0]
                    if not (lo[0] <= x <= hi[0] and lo[1] <= y <= hi[1]):
                        continue
                    if lname == "place" and p.get("class") not in ("suburb", "neighbourhood", "quarter", "village", "town", "hamlet", "city"):
                        continue
                    if lname == "poi":
                        cls = POI_KIND.get(p.get("class"), POI_KIND.get(p.get("subclass"), POI))
                    else:
                        cls = PLACE if lname == "place" else WATER_NAME
                    rank = int(p.get("rank", 50) or 50)
                    out.append(("label", cls, (x, y), name, rank))
    return out


def tiles_round(lat0, lon0, centre, r):
    a = to_ll(centre[0] - r, centre[1] - r, lat0, lon0)
    b = to_ll(centre[0] + r, centre[1] + r, lat0, lon0)
    (x0, y1), (x1, y0) = tile_of(*a), tile_of(*b)
    return [(x, y) for x in range(x0, x1 + 1) for y in range(y0, y1 + 1)]


# --- the blob

def build(feats, cap=CAP - 512, report=False):
    """Records in order of importance, as many as fit in `cap`: water, main roads, parks, streets, names, places,
    places of interest (by rank), waterways, paths, railways."""
    tiers = [
        [f for f in feats if f[0] == "area" and f[1] == WATER],
        [f for f in feats if f[0] == "line" and f[1] <= TERTIARY],
        [f for f in feats if f[0] == "area" and f[1] == PARK],
        [f for f in feats if f[0] == "line" and f[1] == MINOR],
        [f for f in feats if f[0] == "carrier" and f[1] <= TERTIARY],
        [f for f in feats if f[0] == "label" and f[1] in (PLACE, WATER_NAME)],
        [f for f in feats if f[0] == "label" and f[1] == SEARCHED],
        sorted([f for f in feats if f[0] == "label" and (f[1] == POI or FOOD <= f[1] < SEARCHED)], key=lambda f: f[4])[:48],
        [f for f in feats if f[0] == "carrier" and f[1] == MINOR],
        [f for f in feats if f[0] == "line" and f[1] == WATERWAY],
        [f for f in feats if f[0] == "line" and f[1] == PATH],
        [f for f in feats if f[0] == "line" and f[1] == RAIL],
    ]
    out = bytearray()
    names: dict[str, int] = {}
    full = False
    for tier in tiers:
        for kind, cls, g, name, rank in tier:
            rec = bytearray()
            new: list[str] = []
            if kind == "area":
                tol = 3.0
                ring = simplify(g, tol)
                while len(ring) > MAX_RING:
                    tol *= 1.6
                    ring = simplify(g, tol)
                if len(ring) < 3:
                    continue
                rec += bytes([AREA | cls]) + struct.pack("<H", len(ring)) + enc_pts(ring)
            elif kind in ("line", "carrier"):
                pts = simplify(g, 6.0 if kind == "carrier" else 1.5 if cls >= MINOR else 2.5)
                for i in range(0, len(pts) - 1, 254):
                    piece = pts[i:i + 255]
                    if len(piece) < 2:
                        continue
                    idx = 0xFF
                    if kind == "carrier":
                        if name not in names and name not in new:
                            if len(names) + len(new) >= 255:
                                continue
                            t = text(street(name))
                            rec += bytes([NAME, len(t)]) + t
                            new.append(name)
                        idx = names[name] if name in names else len(names) + new.index(name)
                    c = cls + (CARRIER if kind == "carrier" else 0)
                    rec += bytes([LINE | c, idx, len(piece)]) + enc_pts(piece)
            else:
                t = text(name)
                x, y = (max(-32000, min(32000, round(v))) for v in g)
                rec += bytes([LABEL | cls]) + struct.pack("<hhBB", x, y, min(rank, 255), len(t)) + t
            if len(out) + len(rec) > cap:
                full = True
                continue
            out += rec
            for n in new:
                names[n] = len(names)
    return (bytes(out), full) if report else bytes(out)


RADII = (1500, 1250, 1000, 800)


def scene_round(lat0, lon0, centre, r=None, cap=CAP - 512):
    """(blob, r): the square round `centre` (frame metres of lat0, lon0), as wide as fits whole (a city's centre
    is smaller than a village), `r` half its side."""
    import mapbox_vector_tile as mvt
    radii = (r,) if r else RADII
    tiles = {t: mvt.decode(fetch(*t)) for t in tiles_round(lat0, lon0, centre, radii[0])}
    for k, rr in enumerate(radii):
        blob, full = build(features(lat0, lon0, centre, rr, tiles), cap, report=True)
        if not full or k == len(radii) - 1:
            return blob, rr


def main():
    a = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    a.add_argument("lat", type=float)
    a.add_argument("lon", type=float)
    a.add_argument("--r", type=int, help="half the side, metres (default: as wide as fits)")
    a.add_argument("--out", required=True)
    o = a.parse_args()
    blob, r = scene_round(o.lat, o.lon, (0, 0), o.r)
    pathlib.Path(o.out).write_bytes(blob)
    print(f"{len(blob)} B, r {r} m")


if __name__ == "__main__":
    main()
