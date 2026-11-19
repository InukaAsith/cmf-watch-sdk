"""Phone side of the Spotify app, run on the Mac (the iPhone hub app does the same with its own login).
sdk/examples/spotify/src/model.rs has the protocol.

  uv run --with pillow python hosts/spotihost.py --sim 7707              # demo library
  SPOTIFY_TOKEN=... uv run --with pillow python hosts/spotihost.py --sim 7707   # your account (Web API)

The token needs the scopes user-read-playback-state user-modify-playback-state playlist-read-private
playlist-modify-private playlist-modify-public user-library-read user-library-modify. Playback control needs
Premium; without it the watch shows "Premium needed" and everything else still works.
"""
import argparse, asyncio, colorsys, io, json, os, pathlib, struct, sys, time, urllib.parse, urllib.request

from PIL import Image, ImageDraw

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
sys.path.insert(0, str(pathlib.Path(__file__).resolve().parents[1] / "src"))
from mapbuild import tokens  # noqa: E402
from pixhost import rgb565  # noqa: E402
from wp2apps.channel import Channel, SimChannel, send_when_open  # noqa: E402

ART, MSG_MAX = 152, 128
PLAYING, PREMIUM, LIKED, SHUFFLE, IDLE = 1, 2, 4, 8, 16


def art_messages(img: Image.Image):
    img = img.convert("RGB").resize((ART, ART), Image.LANCZOS)
    # rounded corners, black outside
    mask = Image.new("L", (ART, ART), 0)
    ImageDraw.Draw(mask).rounded_rectangle((0, 0, ART - 1, ART - 1), 18, fill=255)
    img = Image.composite(img, Image.new("RGB", img.size), mask)
    q = img.quantize(16, method=Image.Quantize.MEDIANCUT, dither=Image.Dither.NONE)
    pal = (q.getpalette()[:48] + [0] * 48)[:48]
    hdr = b"I" + struct.pack("<HHBhh", ART, ART, 1, 0, 0) + b"".join(struct.pack("<H", rgb565(*pal[3 * i:3 * i + 3])) for i in range(16))
    data = tokens(q.tobytes(), ART, ART)
    msgs = [hdr] + [b"D" + bytes([s & 0xFF]) + data[i:i + 126] for s, i in enumerate(range(0, len(data), 126))] + [b"E"]
    return msgs


def field(s: str, n: int) -> bytes:
    b = s.encode()[:n]
    while b:
        try:
            b.decode()
            break
        except UnicodeDecodeError:
            b = b[:-1]
    return bytes([len(b)]) + b


def now_message(st) -> bytes:
    m = b"N" + bytes([st["flags"]]) + struct.pack("<HH", st["pos"], st["dur"]) + bytes([st["vol"]])
    m += field(st["title"], 48) + field(st["artist"], 40)
    return (m + st["device"].encode())[:MSG_MAX]


def list_messages(kind: bytes, rows):
    out = [b"L" + kind + bytes([min(len(rows), 24)])]
    for i, (title, sub, active) in enumerate(rows[:24]):
        m = b"l" + bytes([i, 1 if active else 0]) + field(title, 40)
        out.append((m + sub.encode())[:MSG_MAX])
    return out


def cover(title, hue):
    """Demo album art: a gradient and the title's initials."""
    img = Image.new("RGB", (ART, ART))
    d = ImageDraw.Draw(img)
    for y in range(ART):
        r, g, b = colorsys.hsv_to_rgb((hue + y / ART * 0.12) % 1, 0.75, 0.95 - y / ART * 0.55)
        d.line([(0, y), (ART, y)], fill=(int(r * 255), int(g * 255), int(b * 255)))
    d.ellipse((36, 36, 116, 116), outline=(255, 255, 255), width=5)
    d.ellipse((66, 66, 86, 86), fill=(255, 255, 255))
    return img


class Demo:
    """An offline library, so the watch app can be tried without an account."""

    def __init__(self):
        self.lib = {
            "Night drive": [("Midnight City", "M83", 243), ("Nightcall", "Kavinsky", 258), ("Tears", "Cut Copy", 221)],
            "Focus": [("Intro", "The xx", 128), ("Kiara", "Bonobo", 230), ("Avril 14th", "Aphex Twin", 125)],
            "Gym": [("Power", "Kanye West", 292), ("Lose Yourself", "Eminem", 326)],
        }
        self.devices = [["iPhone", "Phone", True], ["MacBook", "Computer", False], ["Living room", "Speaker", False]]
        self.queue = list(self.lib["Night drive"])
        self.i = 0
        self.st = {"flags": PLAYING | PREMIUM, "pos": 61, "dur": 243, "vol": 60, "title": "", "artist": "",
                   "device": "iPhone"}
        self.liked = set()
        self.started = time.monotonic()
        self.load(0)

    def load(self, i, pos=0):
        self.i = i % len(self.queue)
        t, a, d = self.queue[self.i]
        self.st.update(title=t, artist=a, dur=d, pos=pos)
        self.st["flags"] = (self.st["flags"] | PLAYING) & ~IDLE
        self.started = time.monotonic() - pos

    def state(self):
        if self.st["flags"] & PLAYING:
            self.st["pos"] = min(self.st["dur"], int(time.monotonic() - self.started))
        liked = self.st["title"] in self.liked
        self.st["flags"] = (self.st["flags"] & ~LIKED) | (LIKED if liked else 0)
        return self.st

    def art(self):
        return cover(self.st["title"], (hash(self.st["title"]) % 100) / 100)

    async def key(self, k):
        if k == "p":
            self.state()
            self.st["flags"] ^= PLAYING
            self.started = time.monotonic() - self.st["pos"]
        elif k in "nb":
            self.load(self.i + (1 if k == "n" else -1))
            return True
        return False

    async def volume(self, v):
        self.st["vol"] = v

    async def like(self):
        t = self.st["title"]
        self.liked ^= {t}
        return "Liked" if t in self.liked else "Removed from Liked"

    async def rows(self, kind):
        if kind in "PA":
            return [(n, f"{len(s)} songs", False) for n, s in self.lib.items()]
        if kind == "D":
            return [(n, sub, on) for n, sub, on in self.devices]
        return []

    async def tracks(self, i):
        name = list(self.lib)[i]
        self.view = self.lib[name]
        return [(t, a, t == self.st["title"]) for t, a, _ in self.view]

    async def search(self, q):
        hits = [s for songs in self.lib.values() for s in songs if q.lower() in (s[0] + " " + s[1]).lower()]
        self.view = hits or [(q.title(), "Search result", 200)]
        return [(t, a, False) for t, a, _ in self.view]

    async def play(self, i):
        self.queue = list(self.view)
        self.load(i)

    async def device(self, i):
        for k, d in enumerate(self.devices):
            d[2] = k == i
        self.st["device"] = self.devices[i][0]

    async def add(self, i):
        name = list(self.lib)[i]
        song = self.queue[self.i]
        if song not in self.lib[name]:
            self.lib[name].append(song)
        return f"Added to {name}"


class Web:
    """The Spotify Web API with a user token."""

    API = "https://api.spotify.com/v1"

    def __init__(self, token):
        self.token, self.st, self.view, self.playlists, self.devs, self.track = token, {}, [], [], [], None
        self.art_url = None

    def call(self, method, path, body=None, query=None):
        url = self.API + path + ("?" + urllib.parse.urlencode(query) if query else "")
        data = json.dumps(body).encode() if body is not None else (b"" if method in ("PUT", "POST") else None)
        req = urllib.request.Request(url, data=data, method=method, headers={
            "Authorization": f"Bearer {self.token}", "Content-Type": "application/json"})
        try:
            with urllib.request.urlopen(req, timeout=10) as r:
                raw = r.read()
                return json.loads(raw) if raw else {}
        except urllib.error.HTTPError as e:
            if e.code == 403:
                raise PermissionError("Premium needed") from e
            raise

    async def run(self, *a, **k):
        return await asyncio.get_running_loop().run_in_executor(None, lambda: self.call(*a, **k))

    def state(self):
        p = self.call("GET", "/me/player") or {}
        item = p.get("item") or {}
        me = self.call("GET", "/me")
        flags = (PLAYING if p.get("is_playing") else 0) | (PREMIUM if me.get("product") == "premium" else 0)
        flags |= SHUFFLE if p.get("shuffle_state") else 0
        if not item:
            flags |= IDLE
        self.track = item
        if item.get("id"):
            if self.call("GET", "/me/tracks/contains", query={"ids": item["id"]}) == [True]:
                flags |= LIKED
        imgs = (item.get("album") or {}).get("images") or []
        self.art_url = imgs[-2]["url"] if len(imgs) > 1 else (imgs[0]["url"] if imgs else None)
        dev = p.get("device") or {}
        self.st = {"flags": flags, "pos": (p.get("progress_ms") or 0) // 1000, "dur": (item.get("duration_ms") or 0) // 1000,
                   "vol": dev.get("volume_percent") or 0, "title": item.get("name", ""),
                   "artist": ", ".join(a["name"] for a in item.get("artists", [])), "device": dev.get("name", "")}
        return self.st

    def art(self):
        if not self.art_url:
            return None
        return Image.open(io.BytesIO(urllib.request.urlopen(self.art_url, timeout=10).read()))

    async def key(self, k):
        if k == "p":
            await self.run("PUT", "/me/player/pause" if self.st.get("flags", 0) & PLAYING else "/me/player/play")
        else:
            await self.run("POST", "/me/player/next" if k == "n" else "/me/player/previous")
        await asyncio.sleep(0.4)
        return True

    async def volume(self, v):
        await self.run("PUT", "/me/player/volume", query={"volume_percent": v})

    async def like(self):
        tid = (self.track or {}).get("id")
        if not tid:
            return "Nothing playing"
        liked = await self.run("GET", "/me/tracks/contains", query={"ids": tid}) == [True]
        await self.run("DELETE" if liked else "PUT", "/me/tracks", query={"ids": tid})
        return "Removed from Liked" if liked else "Liked"

    async def rows(self, kind):
        if kind in "PA":
            me = await self.run("GET", "/me")
            items = (await self.run("GET", "/me/playlists", query={"limit": 24}))["items"]
            if kind == "A":
                items = [p for p in items if p["owner"]["id"] == me["id"] or p.get("collaborative")]
            self.playlists = items
            return [(p["name"], f'{p["tracks"]["total"]} songs', False) for p in items]
        if kind == "D":
            self.devs = (await self.run("GET", "/me/player/devices"))["devices"]
            return [(d["name"], d["type"].title(), d["is_active"]) for d in self.devs]
        return []

    async def tracks(self, i):
        self.context = self.playlists[i]["uri"]
        items = (await self.run("GET", f'/playlists/{self.playlists[i]["id"]}/tracks', query={"limit": 24}))["items"]
        self.view = [it["track"] for it in items if it.get("track")]
        now = (self.track or {}).get("id")
        return [(t["name"], ", ".join(a["name"] for a in t["artists"]), t["id"] == now) for t in self.view]

    async def search(self, q):
        self.context = None
        r = await self.run("GET", "/search", query={"q": q, "type": "track", "limit": 20})
        self.view = r["tracks"]["items"]
        return [(t["name"], ", ".join(a["name"] for a in t["artists"]), False) for t in self.view]

    async def play(self, i):
        t = self.view[i]
        body = {"context_uri": self.context, "offset": {"uri": t["uri"]}} if self.context else {"uris": [t["uri"]]}
        await self.run("PUT", "/me/player/play", body=body)
        await asyncio.sleep(0.5)

    async def device(self, i):
        await self.run("PUT", "/me/player", body={"device_ids": [self.devs[i]["id"]], "play": True})
        await asyncio.sleep(0.8)

    async def add(self, i):
        p = self.playlists[i]
        uri = (self.track or {}).get("uri")
        if not uri:
            return "Nothing playing"
        await self.run("POST", f'/playlists/{p["id"]}/tracks', body={"uris": [uri]})
        return f'Added to {p["name"]}'


async def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--sim", type=int)
    ap.add_argument("--address")
    ap.add_argument("--hci-port", type=int)
    a = ap.parse_args()
    src = Web(os.environ["SPOTIFY_TOKEN"]) if os.environ.get("SPOTIFY_TOKEN") else Demo()
    ch_ = SimChannel(a.sim) if a.sim else Channel(address=a.address, hci_port=a.hci_port, channel_key=True)
    last_title = None
    async with ch_ as ch:

        async def push(art=False):
            nonlocal last_title
            st = await asyncio.get_running_loop().run_in_executor(None, src.state)
            await ch.send(now_message(st))
            if art or st["title"] != last_title:
                last_title = st["title"]
                img = await asyncio.get_running_loop().run_in_executor(None, src.art)
                if img is not None:
                    for m in art_messages(img):
                        await ch.send(m)

        async def rows(kind, items):
            for m in list_messages(kind, items):
                await ch.send(m)

        async def note(text):
            await ch.send(b"X" + text.encode()[:60])

        await send_when_open(ch, b"?")  # the app answers R: its hello may have gone out before this host connected
        while True:
            m = (await ch.recv()).data
            print("watch:", m[:40], flush=True)
            try:
                if m == b"R":
                    await push(art=True)
                elif m[:1] == b"k":
                    if await src.key(chr(m[1])):
                        await push()
                    else:
                        await ch.send(now_message(src.state()))
                elif m[:1] == b"v":
                    await src.volume(m[1])
                elif m == b"h":
                    await note(await src.like())
                    await ch.send(now_message(src.state()))
                elif m[:1] == b"o":
                    kind = m[1:2]
                    await rows(kind, await src.rows(kind.decode()))
                elif m[:1] == b"s":
                    await rows(b"S", await src.search(m[1:].decode(errors="replace")))
                elif m[:1] == b"p":
                    kind, i = chr(m[1]), m[2]
                    if kind == "P":
                        await rows(b"T", await src.tracks(i))
                    elif kind in "TS":
                        await src.play(i)
                        await push()
                    elif kind == "D":
                        await src.device(i)
                        await push()
                    elif kind == "A":
                        await note(await src.add(i))
            except PermissionError as e:
                await note(str(e))
            except Exception as e:  # a failed call must not end the session
                print("error:", e, flush=True)
                await note("Spotify error")


if __name__ == "__main__":
    asyncio.run(main())
