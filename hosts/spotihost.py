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

    async def seek(self, pos):
        self.started = time.monotonic() - pos
        self.st["pos"] = pos

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
    """The Spotify Web API with a user token: the same calls and order as CMFX (ios/App/Hub/Spotify.swift), for
    Spotify's February 2026 API (one library by URI, /playlists/{id}/items with `item`, search ≤ 10, no `product`
    for development-mode apps)."""

    API = "https://api.spotify.com/v1"

    def __init__(self, token):
        self.token, self.st, self.view, self.playlists, self.devs, self.track = token, {}, [], [], [], None
        self.art_url, self.context, self.shown, self.specials = None, None, "tracks", 0

    def call(self, method, path, body=None, query=None):
        url = self.API + path + ("?" + urllib.parse.urlencode(query) if query else "")
        data = json.dumps(body).encode() if body is not None else (b"" if method in ("PUT", "POST") else None)
        req = urllib.request.Request(url, data=data, method=method, headers={
            "Authorization": f"Bearer {self.token}", "Content-Type": "application/json"})
        try:
            with urllib.request.urlopen(req, timeout=10) as r:
                raw = r.read()
                try:
                    return json.loads(raw) if raw else {}
                except ValueError:
                    return {}  # player commands answer 200 with a non-JSON body
        except urllib.error.HTTPError as e:
            try:
                err = json.loads(e.read()).get("error", {})
            except ValueError:
                err = {}
            msg, reason = str(err.get("message", "")), str(err.get("reason", ""))
            if e.code == 404 and reason == "NO_ACTIVE_DEVICE":
                raise LookupError("Open Spotify on a device") from e
            if e.code == 403 and reason == "PREMIUM_REQUIRED":
                raise PermissionError("Premium needed") from e
            if e.code == 403 and ("restriction violated" in msg.lower() or "cannot control device volume" in msg.lower()):
                raise InterruptedError(msg) from e
            raise RuntimeError(f"Spotify: {msg or e.code}") from e

    def call_again(self, method, path, body=None, query=None):
        """Some networks drop TLS connections now and then: twice more (POST only if it never connected)."""
        for attempt in range(3):
            try:
                return self.call(method, path, body, query)
            except urllib.error.URLError as e:
                if isinstance(e, urllib.error.HTTPError) or attempt == 2:
                    raise
                if method == "POST" and "EOF" not in str(e) and "refused" not in str(e):
                    raise
                time.sleep(0.3 * (attempt + 1))

    async def run(self, *a, **k):
        return await asyncio.get_running_loop().run_in_executor(None, lambda: self.call_again(*a, **k))

    def state(self):
        me = self.call_again("GET", "/me")
        p = self.call_again("GET", "/me/player") or {}
        item = p.get("item") or {}
        premium = me.get("product") not in ("free", "open")
        flags = (PLAYING if p.get("is_playing") else 0) | (PREMIUM if premium else 0)
        flags |= SHUFFLE if p.get("shuffle_state") else 0
        if not item:
            flags |= IDLE
        self.track = item
        if item.get("id") and self.call("GET", "/me/library/contains", query={"uris": f"spotify:track:{item['id']}"}) == [True]:
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

    async def play_on_a_device(self, body=None):
        devs = (await self.run("GET", "/me/player/devices")).get("devices", [])
        if not devs:
            raise LookupError("Open Spotify on a device")
        d = next((d for d in devs if d["type"] == "Smartphone"), devs[0])
        await self.run("PUT", "/me/player/play", body=body or {}, query={"device_id": d["id"]})

    async def settle(self, playing):
        """Spotify's player state lags a command by up to a second: wait until it shows `playing`."""
        for _ in range(8):
            await asyncio.sleep(0.25)
            if bool((await self.run("GET", "/me/player") or {}).get("is_playing")) == playing:
                return

    async def key(self, k):
        if k == "p":
            now = await self.run("GET", "/me/player") or {}
            playing = bool(now.get("is_playing")) if now else bool(self.st.get("flags", 0) & PLAYING)
            want = not playing
            try:
                await self.run("PUT", "/me/player/pause" if playing else "/me/player/play")
            except InterruptedError:
                want = playing
                await self.run("PUT", "/me/player/play" if playing else "/me/player/pause")
            except LookupError:
                want = True
                await self.play_on_a_device()
            await self.settle(want)
        else:
            try:
                await self.run("POST", "/me/player/next" if k == "n" else "/me/player/previous")
            except LookupError:
                await self.play_on_a_device()
            await asyncio.sleep(0.6)
        return True

    async def seek(self, pos):
        await self.run("PUT", "/me/player/seek", query={"position_ms": pos * 1000})

    async def volume(self, v):
        dev = (await self.run("GET", "/me/player") or {}).get("device") or {}
        if dev.get("supports_volume") is False:
            return f"Set the volume on {dev.get('name') or 'the device'}"
        try:
            await self.run("PUT", "/me/player/volume", query={"volume_percent": v})
        except InterruptedError:
            return "This device's volume is set on it"

    async def like(self):
        tid = (self.track or {}).get("id")
        if not tid:
            return "Nothing playing"
        uri = f"spotify:track:{tid}"
        liked = await self.run("GET", "/me/library/contains", query={"uris": uri}) == [True]
        await self.run("DELETE" if liked else "PUT", "/me/library", query={"uris": uri})
        return "Removed from Liked" if liked else "Liked"

    async def rows(self, kind):
        if kind in "PA":
            me = await self.run("GET", "/me")
            items = (await self.run("GET", "/me/playlists", query={"limit": 24})).get("items", [])
            if kind == "A":
                items = [p for p in items if (p.get("owner") or {}).get("id") == me.get("id") or p.get("collaborative")]
            self.playlists = items
            self.specials = 2 if kind == "P" else 0
            lead = [("Up next", "Your queue", False), ("Liked Songs", "Your saved songs", False)] if kind == "P" else []

            def sub(p):
                total = (p.get("items") or p.get("tracks") or {}).get("total")
                owner = (p.get("owner") or {}).get("display_name") or ""
                return f"{total} songs" if total is not None else (f"by {owner}" if owner else "Playlist")
            return lead + [(p["name"], sub(p), False) for p in items]
        if kind == "D":
            self.devs = (await self.run("GET", "/me/player/devices")).get("devices", [])
            return [(d["name"], d["type"].title(), d["is_active"]) for d in self.devs]
        return []

    def _rows(self, mark=True):
        now = (self.track or {}).get("id")
        return [(t["name"], ", ".join(a["name"] for a in t.get("artists", [])), mark and t.get("id") == now) for t in self.view]

    async def tracks(self, i):
        if self.specials == 2 and i == 0:
            self.view, self.context, self.shown = (await self.run("GET", "/me/player/queue")).get("queue", [])[:24], None, "queue"
            return self._rows(False)
        if self.specials == 2 and i == 1:
            me = await self.run("GET", "/me")
            items = (await self.run("GET", "/me/tracks", query={"limit": 24})).get("items", [])
            self.view = [it.get("item") or it.get("track") for it in items if it.get("item") or it.get("track")]
            self.context, self.shown = f"spotify:user:{me['id']}:collection", "tracks"
            return self._rows()
        p = self.playlists[i - self.specials]
        self.context = p["uri"]
        try:
            items = (await self.run("GET", f'/playlists/{p["id"]}/items', query={"limit": 24})).get("items", [])
        except RuntimeError:
            self.view, self.shown = [], "whole"
            owner = (p.get("owner") or {}).get("display_name") or ""
            return [("Play all", f"by {owner}" if owner else "Songs hidden by Spotify", False)]
        self.shown = "tracks"
        self.view = [it.get("item") or it.get("track") for it in items if it.get("item") or it.get("track")]
        return self._rows()

    async def search(self, q):
        self.context, self.shown = None, "tracks"
        r = await self.run("GET", "/search", query={"q": q, "type": "track", "limit": 10})
        self.view = (r.get("tracks") or {}).get("items", [])
        return self._rows(False)

    async def play(self, i):
        if self.shown == "whole":
            body = {"context_uri": self.context}
        elif self.shown == "queue":
            for _ in range(i + 1):
                await self.run("POST", "/me/player/next")
                await asyncio.sleep(0.25)
            await asyncio.sleep(0.4)
            return
        else:
            t = self.view[i]
            body = {"context_uri": self.context, "offset": {"uri": t["uri"]}} if self.context else {"uris": [t["uri"]]}
        try:
            await self.run("PUT", "/me/player/play", body=body)
        except LookupError:
            await self.play_on_a_device(body)
        await self.settle(True)

    async def device(self, i):
        await self.run("PUT", "/me/player", body={"device_ids": [self.devs[i]["id"]], "play": True})
        await asyncio.sleep(0.8)

    async def add(self, i):
        p = self.playlists[i]
        uri = (self.track or {}).get("uri")
        if not uri:
            return "Nothing playing"
        await self.run("POST", f'/playlists/{p["id"]}/items', body={"uris": [uri]})
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
                elif m[:1] == b"j" and len(m) >= 3:
                    await src.seek(int.from_bytes(m[1:3], "little"))
                    await ch.send(now_message(src.state()))
                elif m[:1] == b"v":
                    said = await src.volume(m[1])
                    if said:
                        await note(said)
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
            except (PermissionError, LookupError) as e:
                await note(str(e))
            except Exception as e:  # a failed call must not end the session
                print("error:", e, flush=True)
                await note(str(e)[:60] or "Spotify error")


if __name__ == "__main__":
    asyncio.run(main())
