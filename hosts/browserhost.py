"""Phone side of the Browser app for the simulator (the real one is CMFX BrowserService): a reader page, page view as
a drawn picture of a page, and (--video) a playing video's frames.

  uv run --with pillow python hosts/browserhost.py --sim 7793 [--video]

--web: the real web on this Mac in place of the phone (hosts/browsertool: CMFX's Reader and picture code with
WebKit), for trying the watch app on a real watch without a CMFX build:
  uv run --with pillow python hosts/browserhost.py --address <watch> --web
Reader view, Google search, links, page view with its tappable places, scrolling, typing into fields, back, and a
playing video's frames on the watch (its sound on this Mac) work. Nothing is kept: recents live while it runs.
"""
import argparse, asyncio, json, math, os, pathlib, subprocess, sys, urllib.parse

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parents[1] / "src"))
sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
from PIL import Image, ImageDraw  # noqa: E402
from wp2apps import channel  # noqa: E402
from wp2apps.channel import Channel, NoApp, SimChannel, send_when_open  # noqa: E402


class Fast:
    """The watch link at full speed: every message through one sender, 4 in flight (channel.Pipe, about 7 KB/s on
    the watch against 2 KB/s one at a time). One sender, so two never take each other's replies."""

    def __init__(self, ch):
        self.ch, self.lock = ch, asyncio.Lock()

    async def send_many(self, msgs):
        if not msgs:
            return
        async with self.lock:
            if isinstance(self.ch, SimChannel):
                for m in msgs:
                    await self.ch.send(m)
                return
            for attempt in (0, 1):
                session = await self.ch.connect()
                try:
                    return await channel.Pipe(session).stream(msgs)
                except channel.LOST:
                    # the link dropped: once more on a fresh one
                    await self.ch.drop()
                    if attempt:
                        raise

    async def send(self, m):
        await self.send_many([m])

    async def recv(self, *a, **k):
        return await self.ch.recv(*a, **k)
import pixhost  # noqa: E402

# favourites: kept between runs (the owner's own file, never in the repo); BROWSER_FAVOURITES moves it
FAVS = pathlib.Path(os.environ.get("BROWSER_FAVOURITES", "~/.cmf-browser-favourites.json")).expanduser()


def load_favs():
    try:
        return [u for u in json.loads(FAVS.read_text()) if isinstance(u, str)]
    except (OSError, ValueError):
        return []


def save_favs(favs):
    FAVS.write_text(json.dumps(favs))
    FAVS.chmod(0o600)

PAGE = [
    (0, 255, "Watches get a browser"),
    (1, 255, "The phone loads the web and the watch reads it: the text first, a picture of the page when it needs one."),
    (2, 0, "How reader view works"),
    (1, 255, "Scripts don't run, so it works with the phone in a pocket. Links are tapped like any other line."),
    (3, 255, "Crown or drag to scroll"),
    (3, 255, "Tap a link to open it"),
    (3, 255, "The button goes back"),
    (2, 1, "Page view and video"),
] + [(1, 255, f"Paragraph {k}: " + "more words to scroll through on the round screen " * 2) for k in range(8)]


def reader(title="Example news"):
    out = [b"N" + title.encode()]
    for kind, link, text in PAGE:
        b = text.encode()
        for k, i in enumerate(range(0, len(b), 120)):
            out.append(b"t" + bytes([kind if k == 0 else 4, link]) + b[i:i + 120])
    return out + [b"F"]


def page_picture(scroll=0):
    img = Image.new("RGB", (466, 466), "white")
    d = ImageDraw.Draw(img)
    d.rectangle((0, 0, 466, 70), fill=(30, 90, 200))
    d.text((150, 30), "example.com", fill="white")
    for k in range(14):
        y = 90 + k * 26 - scroll % 26
        d.rectangle((60, y, 60 + 300 - (k % 4) * 50, y + 12), fill=(90, 90, 90))
    d.rectangle((60, 300, 400, 420), fill=(0, 0, 0))
    d.polygon([(210, 330), (210, 390), (260, 360)], fill="white")
    hdr, toks = pixhost.encode(img, scale=2)
    return [b"P\x01"] + list(pixhost.messages(hdr, toks))


def frame(t):
    img = Image.new("RGB", (155, 155), "black")
    d = ImageDraw.Draw(img)
    for k in range(6):
        x = 77 + 50 * math.cos(t + k)
        y = 77 + 30 * math.sin(1.3 * t + k)
        d.ellipse((x - 14, y - 14, x + 14, y + 14), fill=((k * 40) % 255, 200 - k * 30, 80 + k * 25))
    hdr, toks = pixhost.encode(img.resize((233, 233)), scale=3)
    return list(pixhost.messages(hdr, toks))


ROOT = pathlib.Path(__file__).resolve().parents[1]
HUBKIT = ROOT.parent / "cmfx" / "ios" / "HubKit" / "Sources" / "HubKit"


def build_tool():
    out = ROOT / "target" / "browsertool"
    srcs = [ROOT / "hosts" / "browsertool" / "main.swift"] + [HUBKIT / f"{n}.swift" for n in ("Reader", "Rle", "Quantize", "Proto", "Emoji", "Layout", "Simple", "Video", "Pictures")]
    if not out.exists() or any(p.stat().st_mtime > out.stat().st_mtime for p in srcs):
        out.parent.mkdir(exist_ok=True)
        # built beside it, then swapped in: macOS won't start a signed binary rewritten in place while it runs
        tmp = out.with_name("browsertool.tmp")
        subprocess.run(["swiftc", "-O", "-swift-version", "5", "-o", str(tmp), *map(str, srcs)], check=True)
        tmp.replace(out)
    return out


class Tool:
    lock = asyncio.Lock()
    # where parts of an answer go as they come ({"part": [...]} lines before it): the watch, at once
    on_part = None

    async def start(self):
        self.p = await asyncio.create_subprocess_exec(str(build_tool()), stdin=asyncio.subprocess.PIPE,
                                                      stdout=asyncio.subprocess.PIPE, limit=1 << 24)

    async def ask(self, line):
        async with self.lock:
            if self.p.returncode is not None:
                # it died: a fresh one (its cookies are on disk)
                print("browser tool restarted", flush=True)
                await self.start()
            if os.environ.get("BROWSERHOST_DEBUG"):
                print("ask", line[:60], file=sys.stderr, flush=True)
            self.p.stdin.write(line.encode() + b"\n")
            await self.p.stdin.drain()
            while True:
                r = json.loads(await self.p.stdout.readline() or b"{}")
                if "part" not in r:
                    return r
                if self.on_part:
                    await self.on_part(r["part"])


def url_for(typed):
    t = typed.strip()
    if t.lower().startswith(("http://", "https://")):
        return t
    if " " not in t and "." in t:
        return "https://" + t
    return "https://www.google.com/search?q=" + urllib.parse.quote_plus(t)


async def web(ch, tool):
    st = {"url": None, "links": [], "history": [], "page": False, "recent": [], "video": None,
          # the start screen's rows: 0 recent, 1 history (this run's pages, newest first), 2 favourites (kept on file)
          "list": 0, "visited": [], "favs": load_favs()}

    def rows():
        return [st["recent"], st["visited"], st["favs"]][st["list"]]

    async def part(ms):
        if os.environ.get("BROWSERHOST_DEBUG"):
            for h in ms:
                if h[:2] in ("49", "4a", "6a", "79"):
                    print("part", bytes.fromhex(h)[:1], h[2:20], file=sys.stderr, flush=True)
        await send(ms)

    tool.on_part = part

    async def bar_label(u):
        # the bar's words: what was searched on Google, else the site
        p = urllib.parse.urlsplit(u or "")
        q = urllib.parse.parse_qs(p.query).get("q", [""])[0] if "google." in (p.hostname or "") else ""
        t = q or (p.hostname or "").removeprefix("www.").removeprefix("m.")
        if t:
            await ch.send(b"b" + t.encode()[:64])

    async def frames():
        # the watch's player: frames while it plays, its place while paused; as long as the video is there and page
        # view is open (no timer stops it)
        await ch.send(b"M\x01")
        rate = {"t": asyncio.get_running_loop().time(), "n": 0, "b": 0}
        v = await tool.ask("vstate")
        if v.get("title"):
            await send(v["title"])
        n = 0
        paused = False
        title = None
        loop = asyncio.get_running_loop()
        try:
            while st["page"]:
                t0 = loop.time()
                if n % 6 == 0:
                    v = await tool.ask("vstate")
                    if not v.get("exists") or v.get("ended"):
                        break
                    await send(v["msgs"])
                    if v.get("title") and v["title"] != title:
                        # the title, or "Ad" while one plays
                        title = v["title"]
                        await send(title)
                if v.get("paused"):
                    await asyncio.sleep(0.4)
                    n = 0
                    paused = True
                    continue
                if paused:
                    # the pause sign is over the picture: a whole frame
                    await tool.ask("vkey")
                    paused = False
                r = await tool.ask("frame")
                n += 1
                if r.get("none") or not r["msgs"]:
                    # between an ad and the video, buffering, or nothing changed: wait (the video is still there)
                    await asyncio.sleep(0.03)
                    continue
                await send(r["msgs"])
                # at most 24 frames a second
                await asyncio.sleep(max(0, 1 / 24 - (loop.time() - t0)))
                # how fast it goes (frames a second, bytes a frame), every 2 s
                rate["n"] += 1
                rate["b"] += sum(len(h) // 2 for h in r["msgs"])
                now = asyncio.get_running_loop().time()
                if now - rate["t"] >= 2:
                    print(f"video: {rate['n'] / (now - rate['t']):.1f} fps, {rate['b'] // max(rate['n'], 1)} B a frame", file=sys.stderr, flush=True)
                    rate.update(t=now, n=0, b=0)
        finally:
            await ch.send(b"M\x00")
        await answer(await tool.ask("page -"))

    async def watch_video():
        # page view: a video that plays (now or once a tap starts it) goes to the watch
        if st["page"] and not st["video"] and (await tool.ask("playing")).get("playing"):
            st["video"] = asyncio.create_task(frames())

    async def stop_video():
        t, st["video"] = st["video"], None
        if t:
            t.cancel()
            try:
                await t
            except (asyncio.CancelledError, Exception):
                pass

    async def send(msgs):
        await ch.send_many([bytes.fromhex(h) for h in msgs])

    async def answer(r):
        if r.get("url"):
            st["url"] = r["url"]
        if "links" in r:
            st["links"] = r["links"]
        if r.get("url"):
            u = r["url"]
            if u not in st["visited"][:1] and not u.startswith("about:"):
                st["visited"] = ([u] + [x for x in st["visited"] if x != u])[:50]
        ms = r.get("msgs", [])
        print(f"answer: {r.get("mode", "")} {r.get("url", "")[:60]} {len(ms)} messages, {sum(len(x) for x in ms) // 2} B", file=sys.stderr, flush=True)
        await send(ms)
        if ms or r.get("mode"):
            await bar_label(r.get("url") or st["url"])
        if r.get("mode") in ("drawn", "picture"):
            # pictures that weren't in yet (striped on the watch): sent as they come, 20 s at most
            if st.get("late") and not st["late"].done():
                st["late"].cancel()
            st["late"] = asyncio.create_task(late(st["url"]))

    async def late(url):
        for _ in range(14):
            await asyncio.sleep(1.5)
            if st["url"] != url or (st["video"] and not st["video"].done()):
                return
            r = await tool.ask("late")
            if st["url"] != url:
                return
            if r.get("msgs"):
                print(f"late pictures: {len(r['msgs'])} messages", file=sys.stderr, flush=True)
                await send(r["msgs"])
            if not r.get("left"):
                return

    async def to_page(u):
        # page view by itself: the watch is told, the page sent whole
        st["page"] = True
        await ch.send(b"G")
        await answer(await tool.ask("page " + u))

    async def open_(u, push=True):
        if push and st["url"]:
            st["history"].append(st["url"])
        st["url"] = u
        host = urllib.parse.urlsplit(u).hostname or ""
        if "google." not in host and "duckduckgo" not in host and u not in st["recent"]:
            st["recent"] = ([u] + st["recent"])[:4]
        if (await tool.ask("isvideo " + u)).get("video"):
            # a video: page view, played, straight to its frames (the page's picture once it stops)
            st["page"] = True
            await ch.send(b"G")
            await tool.ask("load " + u)
            await tool.ask("play")
            if (await tool.ask("playing")).get("playing"):
                st["video"] = asyncio.create_task(frames())
            else:
                await answer(await tool.ask("page -"))
            return
        if st["page"]:
            await answer(await tool.ask("page " + u))
            return
        if "google." in u and "/search" in u:
            await ch.send(b"!Searching Google\xe2\x80\xa6")
            r = await tool.ask("google " + u)
            if "fallback" in r:
                st["url"] = r["fallback"]
                r = await tool.ask("reader " + r["fallback"])
            await answer(r)
            return
        await ch.send(("!Loading " + (urllib.parse.urlsplit(u).hostname or "the page") + "\u2026").encode())
        r = await tool.ask("reader " + u)
        if r.get("empty"):
            # no text here: page view
            await to_page(r.get("url") or u)
            return
        await answer(r)
        host = urllib.parse.urlsplit(st["url"]).hostname or ""
        if "google." not in host and "duckduckgo" not in host and st["url"] not in st["recent"]:
            st["recent"] = ([st["url"]] + st["recent"])[:4]

    if os.environ.get("BROWSERHOST_START"):
        # testing: this page at once, in page view
        st["page"] = True
        await ch.send(b"G")
        await open_(os.environ["BROWSERHOST_START"])
    while True:
        m = (await ch.recv()).data
        print(m[:1].decode(errors="replace"), m[1:40], flush=True)
        c = m[:1]
        if st["video"] and st["video"].done():
            st["video"] = None
        if st["video"] and not st["video"].done() and c in (b"P", b"k"):
            # the player's own: pause / play, skip
            if c == b"P":
                await tool.ask("toggle")
            elif len(m) == 3:
                await tool.ask(f"seek {int.from_bytes(m[1:3], 'little', signed=True)}")
            continue
        if st["video"] and c != b"R":
            await stop_video()
            if c in (b"X", b"B") or m == b"V\x00":
                await tool.ask("stop")
        try:
            if c == b"R":
                st["list"] = m[1] if len(m) == 2 and m[1] < 3 else 0
                for i, u in enumerate(rows()[:4]):
                    await ch.send(b"H" + bytes([i]) + (urllib.parse.urlsplit(u).hostname or u).encode()[:60])
            elif c == b"Q":
                # page view by default (the watch's reader button asks for text)
                st["page"] = True
                await open_(url_for(m[1:].decode(errors="replace")))
            elif c == b"O" and len(m) == 2 and m[1] < len(rows()):
                st["page"] = True
                await open_(rows()[m[1]])
            elif c == b"*" and len(m) > 1:
                # a favourite typed on the start screen (an address, or words: their search), then the list again
                u = url_for(m[1:].decode(errors="replace"))
                st["favs"] = [u] + [x for x in st["favs"] if x != u]
                save_favs(st["favs"])
                st["list"] = 2
                for i, f in enumerate(rows()[:3]):
                    await ch.send(b"H" + bytes([i]) + (urllib.parse.urlsplit(f).hostname or f).encode()[:60])
            elif c == b"L" and len(m) == 2 and m[1] < len(st["links"]):
                await open_(st["links"][m[1]])
            elif m == b"B":
                if st["page"]:
                    r = await tool.ask("back")
                    if not r.get("none"):
                        await answer(r)
                        continue
                if st["history"]:
                    await open_(st["history"].pop(), push=False)
                else:
                    st["url"] = None
                    await ch.send(b"Z")
            elif c == b"V" and len(m) == 2:
                # 0 text, 1 page view drawn, 2 page view as a picture
                st["page"] = m[1] >= 1
                await tool.ask("picture " + ("1" if m[1] == 2 else "0"))
                if st["url"]:
                    await open_(st["url"], push=False)
            elif c == b"T" and len(m) == 5:
                x, y = int.from_bytes(m[1:3], "little"), int.from_bytes(m[3:5], "little")
                r = await tool.ask(f"tap {x} {y}")
                if r.get("open"):
                    # a row of a simple screen: its video (the player) or page
                    await open_(r["open"])
                elif r.get("field"):
                    await ch.send(b"K")
                else:
                    await answer(r)
            elif c == b"S" and len(m) == 3:
                # the end of what the watch has (1 down, -1 up): the real page's next part, or the feed's next posts
                await answer(await tool.ask(f"more {1 if int.from_bytes(m[1:3], 'little', signed=True) > 0 else -1}"))
            elif c == b"W":
                await answer(await tool.ask("type " + m[1:].decode(errors="replace").replace("\n", " ")))
            elif m == b"X":
                await tool.ask("stop")
            if c in (b"T", b"V", b"L", b"S"):
                await asyncio.sleep(1.5 if c == b"T" else 0)
                await watch_video()
        except NoApp:
            raise
        except Exception as e:  # keep serving: one bad page must not end the test
            print("error:", e, flush=True)
            await ch.send(b"!Couldn't load that")


async def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--sim", type=int)
    ap.add_argument("--address")
    ap.add_argument("--video", action="store_true")
    ap.add_argument("--web", action="store_true", help="the real web, from this Mac")
    a = ap.parse_args()
    tool = None
    if a.web:
        # up before the watch app opens: its window can be shown to sign in (kill -USR1)
        tool = Tool()
        await tool.start()
    async with (SimChannel(a.sim) if a.sim else Channel(address=a.address, channel_key=True)) as ch:
        await send_when_open(ch, b"!")
        if a.web:
            ch = Fast(ch)
            # the app closed: wait for it to open again, a fresh start
            while True:
                try:
                    await web(ch, tool)
                except NoApp:
                    print("app closed: waiting for it", flush=True)
                    await send_when_open(ch, b"!")
        video = None
        while True:
            m = (await ch.recv()).data
            print(m[:1].decode(errors="replace"), m[1:30], flush=True)
            if m == b"R":
                for i, t in enumerate(["The Verge", "BBC News"]):
                    await ch.send(b"H" + bytes([i]) + t.encode())
            elif m[:1] in (b"Q", b"O", b"L"):
                await ch.send(b"!Loading example.com\xe2\x80\xa6")
                for x in reader():
                    await ch.send(x)
            elif m == b"V\x01":
                for x in page_picture():
                    await ch.send(x)
                if a.video:
                    async def play():
                        await ch.send(b"M\x01")
                        t = 0.0
                        while True:
                            for x in frame(t):
                                await ch.send(x)
                            t += 0.4
                    video = asyncio.create_task(play())
            elif m in (b"V\x00", b"X", b"B"):
                if video:
                    video.cancel()
                    video = None
                if m == b"V\x00":
                    for x in reader():
                        await ch.send(x)
                elif m == b"B":
                    await ch.send(b"Z")
            elif m[:1] == b"S":
                for x in page_picture(scroll=40):
                    await ch.send(x)
            elif m[:1] == b"T":
                await ch.send(b"K")


if __name__ == "__main__":
    asyncio.run(main())
