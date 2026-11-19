"""Mock phone for the chats app (sdk/examples/chats/src/model.rs has the protocol): fake chats, a drawn sticker,
an auto-reply. Stands in for the iPhone module until the real WhatsApp bridge.

  uv run --with pillow python hosts/wahost.py --sim 7703
"""
import argparse, asyncio, pathlib, struct, sys

from PIL import Image, ImageDraw

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parents[1] / "src"))
from wp2apps.channel import Channel, SimChannel, send_when_open  # noqa: E402

MAX_MSGS = 12

CHATS = {
    1: ("Alex", [(0, 0, "hey are we still on for tonight?"), (1, 0, "yes! 7pm at the usual place"), (0, 1, "")]),
    2: ("Mum", [(0, 0, "Did you eat?"), (1, 0, "Yes mum"), (0, 0, "Call me when you are free")]),
    3: ("Project team", [(0, 0, "Build is green"), (0, 0, "Demo moved to Friday 10am"), (1, 0, "Works for me")]),
    4: ("Sam", [(0, 2, "beach today")]),
    5: ("Gym buddies", [(0, 0, "Leg day tomorrow 6am, no excuses")]),
    6: ("Priya", [(0, 0, "Thanks for the notes!")]),
}
UNREAD = {1: 2, 2: 1, 4: 1}
QUICK = ["OK", "On my way", "Call you later", "Yes", "No", "Thanks!"]


def rgb565(r, g, b):
    return (r >> 3) << 11 | (g >> 2) << 5 | b >> 3


def encode(img, w, h, scale):
    """Run tokens for a w×h picture drawn at `scale` (pixview's format; runs never cross a row)."""
    img = img.convert("RGBA")
    bg = Image.new("RGBA", img.size, (0, 0, 0, 255))
    img = Image.alpha_composite(bg, img).convert("RGB").resize((w, h), Image.LANCZOS)
    q = img.quantize(16, method=Image.Quantize.MEDIANCUT, dither=Image.Dither.NONE)
    pal = (q.getpalette()[:48] + [0] * 48)[:48]
    px, out = q.load(), bytearray()
    for y in range(h):
        x = 0
        while x < w:
            c, run = px[x, y], 1
            while x + run < w and px[x + run, y] == c and run < 271:
                run += 1
            out += bytes([c << 4 | (run - 1)]) if run <= 15 else bytes([c << 4 | 15, run - 16])
            x += run
    hdr = b"I" + struct.pack("<HHBhh", w, h, scale, 0, 0)
    hdr += b"".join(struct.pack("<H", rgb565(*pal[3 * i : 3 * i + 3])) for i in range(16))
    return hdr, bytes(out)


def sticker():
    im = Image.new("RGBA", (256, 256), (0, 0, 0, 0))
    d = ImageDraw.Draw(im)
    d.ellipse((16, 16, 240, 240), fill=(255, 204, 51), outline=(255, 255, 255), width=10)
    d.ellipse((76, 80, 108, 124), fill=(40, 30, 20))
    d.ellipse((148, 80, 180, 124), fill=(40, 30, 20))
    d.arc((64, 100, 192, 200), 20, 160, fill=(40, 30, 20), width=12)
    d.ellipse((50, 130, 86, 158), fill=(255, 120, 120))
    d.ellipse((170, 130, 206, 158), fill=(255, 120, 120))
    return im


def photo():
    tiles = sorted((pathlib.Path(__file__).resolve().parents[1] / "runs" / "tiles" / "16").rglob("*.png"))
    if tiles:
        return Image.open(tiles[0])
    im = Image.new("RGB", (200, 200))
    d = ImageDraw.Draw(im)
    for y in range(200):
        d.line((0, y, 200, y), fill=(80 + y // 3, 160 - y // 4, 230 - y // 2))
    d.ellipse((120, 30, 170, 80), fill=(255, 230, 120))
    d.polygon([(0, 200), (0, 140), (60, 100), (120, 150), (200, 120), (200, 200)], fill=(230, 210, 160))
    return im


def text_msg(op, *head, text=""):
    return bytes([ord(op), *head]) + text.encode()[: 128 - 1 - len(head)]


async def send_picture(ch, idx, kind):
    hdr, data = encode(sticker(), 120, 120, 1) if kind == 1 else encode(photo(), 96, 96, 2)
    await ch.send(b"i" + bytes([idx]) + hdr)
    for seq, i in enumerate(range(0, len(data), 125)):
        await ch.send(b"d" + bytes([idx, seq & 0xFF]) + data[i : i + 125])
    await ch.send(b"e" + bytes([idx]))


def preview(cid):
    me, kind, text = CHATS[cid][1][-1]
    return ("You: " if me else "") + (text or ("Sticker" if kind == 1 else "Photo"))


async def send_list(ch):
    await ch.send(b"L" + bytes([len(CHATS)]))
    for i, (cid, (name, _)) in enumerate(CHATS.items()):
        n, p = name.encode()[:24], preview(cid).encode()[:40]
        await ch.send(b"C" + bytes([i, cid, UNREAD.get(cid, 0), len(n)]) + n + bytes([len(p)]) + p)


async def open_chat(ch, cid):
    name, msgs = CHATS[cid]
    UNREAD.pop(cid, None)
    await ch.send(b"H" + bytes([cid]) + name.encode())
    shown = msgs[-MAX_MSGS:]
    for i, (me, kind, text) in enumerate(shown):
        await ch.send(text_msg("M", i, me, kind, text=text))
    for i, (me, kind, text) in enumerate(shown):
        if kind:
            await send_picture(ch, i, kind)


async def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--sim", type=int)
    ap.add_argument("--address", help="a real watch (K channel)")
    a = ap.parse_args()
    opened = None
    async with (SimChannel(a.sim) if a.sim else Channel(address=a.address, channel_key=True)) as ch:
        await send_when_open(ch, b"?")  # waits till the app is open
        await send_list(ch)
        for i, q in enumerate(QUICK):
            await ch.send(b"Q" + bytes([i]) + q.encode())
        while True:
            m = (await ch.recv()).data
            print("watch:", m[:1].decode(), m[1:], flush=True)
            if m == b"R":
                await send_list(ch)
                for i, q in enumerate(QUICK):
                    await ch.send(b"Q" + bytes([i]) + q.encode())
            elif m[:1] == b"o" and m[1] in CHATS:
                opened = m[1]
                await open_chat(ch, opened)
            elif m[:1] == b"s" and m[1] in CHATS:
                cid, text = m[1], m[2:].decode(errors="replace")
                CHATS[cid][1].append((1, 0, text))
                await send_list(ch)
                # the other side answers with a sticker after 2 s
                await asyncio.sleep(2)
                CHATS[cid][1].append((0, 1, ""))
                await ch.send(b"N" + bytes([cid]))
                await send_list(ch)
                if opened == cid:
                    n = min(len(CHATS[cid][1]), MAX_MSGS) - 1
                    await ch.send(text_msg("A", 0, 1))
                    await send_picture(ch, n, 1)


if __name__ == "__main__":
    asyncio.run(main())
