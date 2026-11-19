"""The phone's half of the Photos app (sdk/examples/photos, proto.rs): a folder of pictures as the watch's gallery.

  uv run --with pillow python hosts/photohost.py --sim 7742 [DIR]
  uv run --with pillow python hosts/photohost.py --address <watch> [DIR]

Without DIR it makes a demo gallery in runs/mock/gallery.
"""
import argparse, asyncio, colorsys, math, pathlib, struct, sys, zlib

from PIL import Image, ImageDraw

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent.parent / "src"))
sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
from wp2apps.channel import Channel, NoApp, SimChannel, send_when_open  # noqa: E402
from pixhost import messages, rgb565  # noqa: E402
from mapbuild import tokens  # noqa: E402

THUMB = 104
THUMB_CAP = 12 * 1024
MSG_MAX = 128
EXT = {".png", ".jpg", ".jpeg", ".webp", ".bmp", ".gif"}


def demo(root: pathlib.Path):
    root.mkdir(parents=True, exist_ok=True)
    names = ["Sunset", "Ocean", "Forest", "City lights", "Mountains", "Desert", "Aurora", "Lake", "Night sky",
             "Flowers", "Snow", "Neon", "Beach day", "Autumn"]
    for k, n in enumerate(names):
        p = root / f"{n}.png"
        if p.exists():
            continue
        im = Image.new("RGB", (466, 466))
        d = ImageDraw.Draw(im)
        h0 = k / len(names)
        for y in range(466):
            r, g, b = colorsys.hsv_to_rgb((h0 + y / 1400) % 1, 0.75, 0.35 + 0.6 * y / 466)
            d.line((0, y, 466, y), fill=(int(r * 255), int(g * 255), int(b * 255)))
        for j in range(5):
            a = (k * 7 + j * 72) * math.pi / 180
            x, y, r = 233 + 120 * math.cos(a), 233 + 120 * math.sin(a), 30 + 12 * j
            rr, gg, bb = colorsys.hsv_to_rgb((h0 + 0.5 + j * 0.05) % 1, 0.6, 1)
            d.ellipse((x - r, y - r, x + r, y + r), fill=(int(rr * 255), int(gg * 255), int(bb * 255)))
        d.text((40, 400), n, fill=(255, 255, 255))
        im.save(p)


def thumb_file(img: Image.Image) -> bytes:
    img = img.convert("RGB")
    w, h = img.size
    s = min(w, h)
    img = img.crop(((w - s) // 2, (h - s) // 2, (w - s) // 2 + s, (h - s) // 2 + s))
    for side, scale in ((THUMB, 1), (THUMB // 2, 2)):
        t = img.resize((side, side), Image.LANCZOS)
        mask = Image.new("L", (side, side), 0)
        ImageDraw.Draw(mask).rounded_rectangle((0, 0, side - 1, side - 1), 14 // scale, fill=255)
        t = Image.composite(t, Image.new("RGB", t.size), mask)
        q = t.quantize(16, method=Image.Quantize.MEDIANCUT, dither=Image.Dither.NONE)
        pal = (q.getpalette()[:48] + [0] * 48)[:48]
        hdr = b"I" + struct.pack("<HHBhh", side, side, scale, 0, 0)
        hdr += b"".join(struct.pack("<H", rgb565(*pal[3 * i:3 * i + 3])) for i in range(16))
        data = hdr + tokens(q.tobytes(), side, side)
        if len(data) <= THUMB_CAP:
            return data
    return data


def file_messages(name: str, data: bytes):
    yield b"F" + name.encode()
    for k in range(0, len(data), MSG_MAX - 1):
        yield b"f" + data[k:k + MSG_MAX - 1]
    yield b"c"


BANDS = 8
PANEL = 466


def framed(img: Image.Image, round_: bool, scale: int = 2):
    """→ (picture at its pixel size, its origin on the panel). round_: a square filling the panel, masked to the
    circle; else the whole picture, as big as fits inside the circle, its corners rounded."""
    img = img.convert("RGB")
    w, h = img.size
    if round_:
        n = PANEL // scale
        side = min(w, h)
        img = img.crop(((w - side) // 2, (h - side) // 2, (w + side) // 2, (h + side) // 2)).resize((n, n), Image.LANCZOS)
        mask = Image.new("L", (n, n), 0)
        ImageDraw.Draw(mask).ellipse((0, 0, n - 1, n - 1), fill=255)
    else:
        a = w / h
        R = 226
        pw, ph = int(2 * R * a / math.sqrt(1 + a * a)) // scale, int(2 * R / math.sqrt(1 + a * a)) // scale
        img = img.resize((pw, ph), Image.LANCZOS)
        mask = Image.new("L", (pw, ph), 0)
        ImageDraw.Draw(mask).rounded_rectangle((0, 0, pw - 1, ph - 1), radius=12, fill=255)
    img.paste((0, 0, 0), mask=Image.eval(mask, lambda v: 255 - v))
    pw, ph = img.size
    return img, ((PANEL - pw * scale) // 2, (PANEL - ph * scale) // 2)


def picture_messages(img: Image.Image, round_: bool, scale: int = 2):
    """The opened picture: `P n`, then n horizontal bands, each its own 16 colours (median cut, refined by k-means),
    `I` (its row as y0) / `D`… / `E`."""
    img, (ox, oy) = framed(img, round_, scale)
    n, h = img.size
    rows = [h * k // BANDS for k in range(BANDS + 1)]
    out = [b"P" + bytes([BANDS])]
    for k in range(BANDS):
        y0, y1 = rows[k], rows[k + 1]
        q = img.crop((0, y0, n, y1)).quantize(16, method=Image.Quantize.MEDIANCUT, kmeans=4, dither=Image.Dither.NONE)
        pal = (q.getpalette()[:48] + [0] * 48)[:48]
        hdr = b"I" + struct.pack("<HHBhh", n, y1 - y0, scale, ox, oy + y0 * scale)
        hdr += b"".join(struct.pack("<H", rgb565(*pal[3 * i:3 * i + 3])) for i in range(16))
        out += list(messages(hdr, tokens(q.tobytes(), n, y1 - y0)))
    return out


def rle8(px: bytes, w: int, h: int) -> bytes:
    """A byte < 255: one pixel of that colour; 255, c, n: n (3..255) pixels of colour c; runs stay in a row."""
    out = bytearray()
    for y in range(h):
        row = px[y * w:(y + 1) * w]
        x = 0
        while x < w:
            c, k = row[x], 1
            while x + k < w and row[x + k] == c and k < 255:
                k += 1
            out += bytes([255, c, k]) if k >= 3 else bytes([c]) * k
            x += k
    return bytes(out)


def sharp_messages(img: Image.Image, round_: bool, scale: int = 2, cap: int = 40 * 1024):
    """The sharp version: up to 255 colours (fewer if it would not fit the watch's 40 KB), `S`, `K`…, `Y`…, `Z`."""
    img, (ox, oy) = framed(img, round_, scale)
    w, h = img.size
    for colours in (255, 192, 128, 96, 64):
        q = img.quantize(colours, method=Image.Quantize.MEDIANCUT, kmeans=2, dither=Image.Dither.NONE)
        data = rle8(q.tobytes(), w, h)
        if len(data) <= cap:
            break
    pal = q.getpalette()[:3 * colours]
    out = [b"S" + struct.pack("<HHHBhh", len(data), w, h, scale, ox, oy)]
    for first in range(0, colours, 60):
        cs = range(first, min(first + 60, colours))
        out.append(b"K" + bytes([first, len(cs)]) + b"".join(struct.pack("<H", rgb565(*pal[3 * i:3 * i + 3])) for i in cs))
    for seq, i in enumerate(range(0, len(data), MSG_MAX - 2)):
        out.append(b"Y" + bytes([seq & 0xFF]) + data[i:i + MSG_MAX - 2])
    out.append(b"Z")
    return out


async def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--sim", type=int)
    ap.add_argument("--address")
    ap.add_argument("dir", nargs="?")
    a = ap.parse_args()
    root = pathlib.Path(a.dir) if a.dir else pathlib.Path(__file__).resolve().parent.parent / "runs/mock/gallery"
    if not a.dir:
        demo(root)
    pics = sorted(p for p in root.iterdir() if p.suffix.lower() in EXT)[:64]
    names = [p.stem for p in pics]
    key = zlib.crc32("\n".join(names).encode())
    print(f"{len(pics)} pictures, key {key:08x}", flush=True)
    ch_ = SimChannel(a.sim) if a.sim else Channel(address=a.address, channel_key=True)
    async with ch_ as ch:

        async def gallery():
            await ch.send(b"L" + bytes([len(names)]) + struct.pack("<I", key))
            for i, n in enumerate(names):
                await ch.send(b"l" + bytes([i]) + n.encode()[:40])

        shown = [None, True, False]  # index, round, sharp

        async def handle(m):
            if m == b"R":
                await gallery()
            elif m[:1] == b"q" and len(m) == 2 and m[1] < len(pics):
                for msg in file_messages(f"th{m[1]}", thumb_file(Image.open(pics[m[1]]))):
                    await ch.send(msg)
            elif m[:1] == b"o" and len(m) in (2, 3) and m[1] < len(pics):
                # o idx [mode]: mode 1 round, 0 (or none: round, as before) the whole picture
                shown[0], shown[1], shown[2] = m[1], len(m) == 2 or m[2] == 1, False
                for msg in picture_messages(Image.open(pics[m[1]]), shown[1]):
                    await ch.send(msg)
            elif m == b"h" and shown[0] is not None:
                shown[2] = True
                for msg in sharp_messages(Image.open(pics[shown[0]]), shown[1]):
                    await ch.send(msg)
            elif m[:1] == b"G" and shown[0] is not None:
                # a chunk went missing: that picture again, whole
                make = sharp_messages if shown[2] else picture_messages
                for msg in make(Image.open(pics[shown[0]]), shown[1]):
                    await ch.send(msg)

        await send_when_open(ch, b"?")
        while True:
            m = (await ch.recv()).data
            print("watch:", m[:20], flush=True)
            try:
                await handle(m)
            except NoApp:
                print("the app was closed", flush=True)


if __name__ == "__main__":
    asyncio.run(main())
