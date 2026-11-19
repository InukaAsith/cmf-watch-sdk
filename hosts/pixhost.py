"""Phone side of pixview, run on the Mac: renders a map or a photo, cuts it to 16 colours, run-length encodes it and
streams it to the app (sdk/examples/pixview/src/rle.rs has the format). Crown on the watch zooms, a tap recentres.

  uv run --with pillow python hosts/pixhost.py --sim 7701 map --lat 51.5080 --lon -0.1281 --zoom 16
  uv run --with pillow python hosts/pixhost.py --sim 7701 photo some.jpg
"""
import argparse, asyncio, io, math, pathlib, struct, sys, threading, time, urllib.request

from PIL import Image, ImageDraw

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parents[1] / "src"))
from wp2apps.channel import Channel, SimChannel  # noqa: E402

SIZE, MSG_MAX = 466, 128
TILES = pathlib.Path(__file__).resolve().parents[1] / "runs" / "tiles"
UA = "pixview-dev/0.1"


def rgb565(r, g, b):
    return (r >> 3) << 11 | (g >> 2) << 5 | b >> 3


def encode(img: Image.Image, scale: int = 2, colours: int = 16):
    """Image → (header, token stream). The image is fitted to (466/scale)² and masked to the round panel."""
    n = SIZE // scale
    img = img.convert("RGB").resize((n, n), Image.LANCZOS)
    q = img.quantize(colours, method=Image.Quantize.MEDIANCUT, dither=Image.Dither.NONE)
    pal = q.getpalette()[: 3 * colours]
    pal += [0] * (48 - len(pal))
    # black outside the circle: the darkest palette entry, so masked corners cost one long run per row
    dark = min(range(16), key=lambda i: sum(pal[3 * i : 3 * i + 3]))
    mask = Image.new("L", (n, n), 0)
    ImageDraw.Draw(mask).ellipse((0, 0, n - 1, n - 1), fill=255)
    q.paste(dark, mask=Image.eval(mask, lambda v: 255 - v))
    px = q.load()
    out = bytearray()
    for y in range(n):
        x = 0
        while x < n:
            c, run = px[x, y], 1
            while x + run < n and px[x + run, y] == c and run < 271:
                run += 1
            out += bytes([c << 4 | (run - 1)]) if run <= 15 else bytes([c << 4 | 15, run - 16])
            x += run
    off = (SIZE - n * scale) // 2
    header = b"I" + struct.pack("<HHBhh", n, n, scale, off, off)
    header += b"".join(struct.pack("<H", rgb565(*pal[3 * i : 3 * i + 3])) for i in range(16))
    return header, bytes(out)


def messages(header: bytes, stream: bytes):
    yield header
    step = MSG_MAX - 2
    for seq, i in enumerate(range(0, len(stream), step)):
        yield b"D" + bytes([seq & 0xFF]) + stream[i : i + step]
    yield b"E"


# --- maps (OpenStreetMap tiles; Google Static Maps with --google-key) ---

def world(lat, lon, z):
    s = 256 * 2**z
    x = (lon + 180) / 360 * s
    y = (1 - math.log(math.tan(math.radians(lat)) + 1 / math.cos(math.radians(lat))) / math.pi) / 2 * s
    return x, y


def unworld(x, y, z):
    s = 256 * 2**z
    lon = x / s * 360 - 180
    lat = math.degrees(math.atan(math.sinh(math.pi * (1 - 2 * y / s))))
    return lat, lon


def tile(z, x, y):
    """One OSM tile from the cache or the network; written atomically (the prefetch thread fetches too), grey if it
    cannot be had."""
    p = TILES / f"{z}/{x}/{y}.png"
    for _ in range(2):
        try:
            if not p.exists():
                p.parent.mkdir(parents=True, exist_ok=True)
                req = urllib.request.Request(f"https://tile.openstreetmap.org/{z}/{x}/{y}.png", headers={"User-Agent": UA})
                data = urllib.request.urlopen(req, timeout=15).read()
                tmp = p.with_suffix(f".{threading.get_ident()}.part")
                tmp.write_bytes(data)
                tmp.replace(p)
            return Image.open(p).convert("RGB")
        except Exception:
            p.unlink(missing_ok=True)
    return Image.new("RGB", (256, 256), (200, 200, 200))


def osm_map(lat, lon, z):
    cx, cy = world(lat, lon, z)
    left, top = int(cx - SIZE / 2), int(cy - SIZE / 2)
    img = Image.new("RGB", (SIZE, SIZE))
    for tx in range(left // 256, (left + SIZE) // 256 + 1):
        for ty in range(top // 256, (top + SIZE) // 256 + 1):
            img.paste(tile(z, tx, ty), (tx * 256 - left, ty * 256 - top))
    return img


def google_map(lat, lon, z, key):
    url = (f"https://maps.googleapis.com/maps/api/staticmap?center={lat},{lon}&zoom={z}&size={SIZE // 2}x{SIZE // 2}"
           f"&scale=2&maptype=roadmap&key={key}")
    return Image.open(io.BytesIO(urllib.request.urlopen(url, timeout=15).read())).convert("RGB")


def pin(img):
    d, c = ImageDraw.Draw(img), SIZE // 2
    d.ellipse((c - 9, c - 9, c + 9, c + 9), fill=(255, 255, 255))
    d.ellipse((c - 6, c - 6, c + 6, c + 6), fill=(30, 110, 255))
    return img


async def stream(ch, img, scale, rate=0, coarse=True):
    """Coarse first (scale 4, ~¼ the bytes) so something sharp-ish lands fast, then the full image. `rate` (B/s)
    paces the sends like the real BLE link; 0 = as fast as the channel takes them."""
    passes = [4, scale] if coarse and scale < 4 else [scale]
    for k in passes:
        t0 = time.monotonic()
        header, data = encode(img, k)
        msgs = list(messages(header, data))
        for m in msgs:
            await ch.send(m)
            if rate:
                await asyncio.sleep(len(m) / rate)
        print(f"scale {k}: {len(data)} B in {len(msgs)} messages, {time.monotonic() - t0:.2f}s", flush=True)


def prefetch(lat, lon, z):
    """Tiles for zoom ±1 around the centre, in the background, so the next crown turn renders from the cache."""
    for dz in (1, -1):
        try:
            osm_map(lat, lon, z + dz)
        except Exception:
            pass


async def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--sim", type=int, help="simulator TCP port (scripts/sim pixview --tcp PORT)")
    ap.add_argument("--hci-port", type=int, help="emulator over Netsim (H4 port)")
    ap.add_argument("--address", help="a real watch (wp2apps scan)")
    ap.add_argument("--scale", type=int, default=2)
    ap.add_argument("--once", action="store_true", help="exit after the first image is shown")
    ap.add_argument("--rate", type=int, default=0, help="pace sends at this many B/s (real BLE: ~8000)")
    ap.add_argument("--no-coarse", action="store_true")
    sub = ap.add_subparsers(dest="cmd", required=True)
    m = sub.add_parser("map")
    m.add_argument("--lat", type=float, required=True)
    m.add_argument("--lon", type=float, required=True)
    m.add_argument("--zoom", type=int, default=16)
    m.add_argument("--google-key")
    p = sub.add_parser("photo")
    p.add_argument("path")
    a = ap.parse_args()

    def render():
        if a.cmd == "photo":
            img = Image.open(a.path)
            s = min(img.size)
            return img.crop(((img.width - s) // 2, (img.height - s) // 2, (img.width + s) // 2, (img.height + s) // 2))
        img = google_map(a.lat, a.lon, a.zoom, a.google_key) if a.google_key else osm_map(a.lat, a.lon, a.zoom)
        return pin(img)

    job = None

    async def show():
        # a newer request cancels the image still streaming: the watch drops it at the next `I`
        nonlocal job
        if job and not job.done():
            job.cancel()
        try:
            img = await asyncio.to_thread(render)
        except Exception as e:
            print("render failed:", e, flush=True)
            return
        job = asyncio.create_task(stream(ch, img, a.scale, a.rate, not a.no_coarse))
        if a.cmd == "map" and not a.google_key:
            asyncio.get_running_loop().run_in_executor(None, prefetch, a.lat, a.lon, a.zoom)

    ch_ = SimChannel(a.sim) if a.sim else Channel(address=a.address, hci_port=a.hci_port)
    async with ch_ as ch:
        await show()
        while True:
            msg = (await ch.recv()).data.decode(errors="replace")
            print("watch:", msg, flush=True)
            if msg.startswith("K") and a.once and job.done():
                return
            if msg == "R" or msg.startswith("G"):
                await show()
            elif a.cmd == "map" and msg.startswith("Z"):
                a.zoom = max(3, min(19, a.zoom + int(msg[1:])))
                await show()
            elif a.cmd == "map" and msg.startswith("T"):
                _, x, y = msg.split()
                cx, cy = world(a.lat, a.lon, a.zoom)
                a.lat, a.lon = unworld(cx + int(x) - SIZE // 2, cy + int(y) - SIZE // 2, a.zoom)
                await show()


if __name__ == "__main__":
    asyncio.run(main())
