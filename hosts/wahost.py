"""Mock phone for the chats app (sdk/examples/chats/src/model.rs has the protocol): fake chats, a drawn sticker,
an auto-reply. Stands in for the iPhone module until the real WhatsApp bridge.

  uv run --with pillow python hosts/wahost.py --sim 7703
"""
import argparse, asyncio, pathlib, struct, sys

from PIL import Image, ImageDraw

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parents[1] / "src"))
from wp2apps.channel import Channel, SimChannel, send_when_open  # noqa: E402
from wp2apps.adpcm import Encoder  # noqa: E402

import math  # noqa: E402

VOICE = 5

MAX_MSGS = 12

CHATS = {
    1: ("Alex", [(0, 0, "hey are we still on for tonight?"), (1, 0, "yes! 7pm at the usual place"), (0, 1, "")]),
    2: ("Mum", [(0, 0, "Did you eat?"), (1, 0, "Yes mum"), (0, VOICE, "0:04"), (0, 0, "Call me when you are free")]),
    3: ("Project team", [(0, 0, "Build is green"), (0, 0, "Demo moved to Friday 10am"), (1, 0, "Works for me")]),
    4: ("Sam", [(0, 2, "beach today")]),
    5: ("Gym buddies", [(0, 0, "Leg day tomorrow 6am, no excuses")]),
    6: ("Priya", [(0, 0, "Thanks for the notes!")]),
}
UNREAD = {1: 2, 2: 1, 4: 1}
# under the name: last seen, online (P), or nothing (groups); and a made-up profile picture's colours
SEEN = {2: "last seen 16:42", 4: "last seen yesterday 21:05", 6: "last seen 2 Nov"}
ONLINE = {1}
DP = {1: ((40, 90, 200), (240, 200, 170)), 2: ((200, 80, 120), (220, 170, 140)), 4: ((30, 150, 140), (150, 110, 80)),
      6: ((150, 90, 200), (190, 140, 110))}
# whom a new chat can go to (the + button's picker)
CONTACTS = {20: "Dad", 21: "Nimal", 22: "Ayesha", 23: "Kasun", 24: "Tharushi"}
NEW = 200
# the chats whose list picture the watch has
FACES_SENT = set()
QUICK = ["OK", "On my way", "Call you later", "Yes", "No", "Thanks!"]


def rgb565(r, g, b):
    return (r >> 3) << 11 | (g >> 2) << 5 | b >> 3


def dp_parts(bg, skin):
    """A head and shoulders on a colour, drawn at 48 px and shrunk to the watch's 12 x 12 (`G` parts of 4 rows)."""
    im = Image.new("RGB", (48, 48), bg)
    d = ImageDraw.Draw(im)
    d.ellipse((6, 30, 42, 66), fill=(30, 30, 40))
    d.ellipse((14, 8, 34, 30), fill=skin)
    d.chord((13, 6, 35, 24), 180, 360, fill=(40, 30, 25))
    px = im.resize((12, 12), Image.LANCZOS).load()
    out = []
    for part in range(3):
        b = bytearray(b"G" + bytes([part]))
        for y in range(part * 4, part * 4 + 4):
            for x in range(12):
                b += struct.pack("<H", rgb565(*px[x, y]))
        out.append(bytes(b))
    return out


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
    if not CHATS[cid][1]:
        return ""
    me, kind, text = CHATS[cid][1][-1]
    if kind == VOICE:
        return ("You: " if me else "") + "Voice message"
    return ("You: " if me else "") + (text or ("Sticker" if kind == 1 else "Photo"))


def row(i, cid, unread, name, prev):
    n, p = name.encode()[:24], prev.encode()[:40]
    return b"C" + bytes([i, cid, unread, len(n)]) + n + bytes([len(p)]) + p


async def send_list(ch, syncing=False):
    """The chats; first the + button's row (unread 254) and, while syncing, the line the watch draws as a bar."""
    rows = [row(0, NEW, 254, "New chat", "Start one")]
    if syncing:
        rows.append(row(1, 250, 255, "Syncing...", "Saved chats below"))
    for cid, (name, _) in CHATS.items():
        rows.append(row(len(rows), cid, UNREAD.get(cid, 0), name, preview(cid)))
    rows = rows[:12]
    await ch.send(b"L" + bytes([len(rows)]))
    for r in rows:
        await ch.send(r)
    # the rows' pictures (`g id part px`), once a session
    for cid in CHATS:
        if cid in DP and cid not in FACES_SENT:
            FACES_SENT.add(cid)
            for part in dp_parts(*DP[cid]):
                await ch.send(b"g" + bytes([cid]) + part[1:])


async def send_picker(ch):
    await ch.send(b"KNew chat")
    await ch.send(b"L" + bytes([len(CONTACTS)]))
    for i, (cid, name) in enumerate(CONTACTS.items()):
        await ch.send(row(i, cid, 0, name, "Hey there! I am using WhatsApp."))


async def open_chat(ch, cid):
    name, msgs = CHATS[cid]
    UNREAD.pop(cid, None)
    await ch.send(b"H" + bytes([cid]) + name.encode())
    if cid in ONLINE:
        await ch.send(b"P\x01")
    elif cid in SEEN:
        await ch.send(b"W" + SEEN[cid].encode())
    if cid in DP:
        for part in dp_parts(*DP[cid]):
            await ch.send(part)
    shown = msgs[-MAX_MSGS:]
    for i, (me, kind, text) in enumerate(shown):
        await ch.send(text_msg("M", i, me, kind, text=text))
    for i, (me, kind, text) in enumerate(shown):
        if kind in (1, 2):
            await send_picture(ch, i, kind)


def voice_blocks(secs):
    """A made-up voice note at 8 kHz: a gliding hum in syllables, as `B seq block` messages."""
    n = int(secs * 8000)
    out, enc, ph = [], Encoder(), 0.0
    pcm = []
    for i in range(n):
        t = i / 8000
        f = 180 + 60 * math.sin(2 * math.pi * 0.7 * t)
        ph += 2 * math.pi * f / 8000
        env = max(0.0, math.sin(2 * math.pi * 2.5 * t)) ** 0.5
        pcm.append(int(9000 * env * (math.sin(ph) + 0.4 * math.sin(2 * ph) + 0.2 * math.sin(3 * ph))))
    pcm += [0] * (-len(pcm) % 244)
    for seq, k in enumerate(range(0, len(pcm), 244)):
        out.append(b"B" + struct.pack("<H", seq) + enc.block(pcm[k:k + 244]))
    return out


async def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--sim", type=int)
    ap.add_argument("--address", help="a real watch (K channel)")
    a = ap.parse_args()
    opened = None
    async with (SimChannel(a.sim) if a.sim else Channel(address=a.address, channel_key=True)) as ch:
        await send_when_open(ch, b"?")  # waits till the app is open
        # a catch-up first, as the phone does: the bar fills while it syncs
        await send_list(ch, syncing=True)
        for i, q in enumerate(QUICK):
            await ch.send(b"Q" + bytes([i]) + q.encode())
        for pct in (15, 30, 45, 60, 75, 90):
            await ch.send(b"Y" + bytes([pct]))
            await asyncio.sleep(1.2)
        await ch.send(b"Y" + bytes([100]))
        await asyncio.sleep(0.6)
        await send_list(ch)
        # the voice note playing on the watch (blocks, how many went), and a recording's blocks
        voice, rec = None, 0
        while True:
            m = (await ch.recv()).data
            if m[:1] not in (b"F", b"~"):
                print("watch:", m[:1].decode(errors="replace"), m[1:], flush=True)
            if m[:1] == b"t" and opened in CHATS and m[1] < len(CHATS[opened][1][-MAX_MSGS:]) and \
                    CHATS[opened][1][-MAX_MSGS:][m[1]][1] == VOICE:
                text = CHATS[opened][1][-MAX_MSGS:][m[1]][2]
                mins, secs = text.split(":")
                voice = [voice_blocks(int(mins) * 60 + int(secs)), 0]
                await ch.send(b"O")
                continue
            if m[:1] == b"F" and len(m) == 4 and voice:
                nxt, room = struct.unpack("<H", m[1:3])[0], m[3]
                blocks, sent = voice
                sent = max(sent, nxt)
                while sent < len(blocks) and sent < nxt + room:
                    await ch.send(blocks[sent])
                    sent += 1
                voice[1] = sent
                if sent == len(blocks):
                    await ch.send(b"E")
                    voice = None
                continue
            if m == b"x":
                voice = None
                continue
            # a voice note recorded on the watch: sent when it ends
            if m == b"~3":
                rec = 0
                continue
            if m[:2] == b"~a":
                rec += 1
                continue
            if m == b"~0" and opened in CHATS:
                secs = max(1, round(rec * 240 / 8000))
                CHATS[opened][1].append((1, VOICE, f"{secs // 60}:{secs % 60:02d}"))
                await ch.send(b"~\x01Sent")
                await open_chat(ch, opened)
                continue
            if m == b"R":
                FACES_SENT.clear()
                await send_list(ch)
                for i, q in enumerate(QUICK):
                    await ch.send(b"Q" + bytes([i]) + q.encode())
            elif m == b"o" + bytes([NEW]):
                await send_picker(ch)
            elif m[:1] == b"o" and m[1] in CONTACTS:
                # a new chat with them: empty till something is sent
                CHATS[m[1]] = (CONTACTS[m[1]], [])
                opened = m[1]
                await open_chat(ch, opened)
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
