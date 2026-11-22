"""Phone side of the Browser app for the simulator (the real one is CMFX BrowserService): a reader page, page view as
a drawn picture of a page, and (--video) a playing video's frames.

  uv run --with pillow python hosts/browserhost.py --sim 7793 [--video]

--web: the real web on this Mac in place of the phone (hosts/browsertool: CMFX's Reader and picture code with
WebKit), for trying the watch app on a real watch without a CMFX build:
  uv run --with pillow python hosts/browserhost.py --address <watch> --web
Reader view, Google search, links, page view with its tappable places, scrolling, typing into fields, back, and a
playing video's frames on the watch (its sound on this Mac) work. Nothing is kept: recents live while it runs.
"""
import argparse, asyncio, json, math, pathlib, subprocess, sys, urllib.parse

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parents[1] / "src"))
sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
from PIL import Image, ImageDraw  # noqa: E402
from wp2apps.channel import Channel, NoApp, SimChannel, send_when_open  # noqa: E402
import pixhost  # noqa: E402

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
    srcs = [ROOT / "hosts" / "browsertool" / "main.swift"] + [HUBKIT / f"{n}.swift" for n in ("Reader", "Rle", "Quantize", "Proto", "Emoji")]
    if not out.exists() or any(p.stat().st_mtime > out.stat().st_mtime for p in srcs):
        out.parent.mkdir(exist_ok=True)
        subprocess.run(["swiftc", "-O", "-swift-version", "5", "-o", str(out), *map(str, srcs)], check=True)
    return out


class Tool:
    lock = asyncio.Lock()

    async def start(self):
        self.p = await asyncio.create_subprocess_exec(str(build_tool()), stdin=asyncio.subprocess.PIPE,
                                                      stdout=asyncio.subprocess.PIPE, limit=1 << 24)

    async def ask(self, line):
        async with self.lock:
            self.p.stdin.write(line.encode() + b"\n")
            await self.p.stdin.drain()
            return json.loads(await self.p.stdout.readline() or b"{}")


def url_for(typed):
    t = typed.strip()
    if t.lower().startswith(("http://", "https://")):
        return t
    if " " not in t and "." in t:
        return "https://" + t
    return "https://www.google.com/search?q=" + urllib.parse.quote_plus(t)


async def web(ch, tool):
    st = {"url": None, "links": [], "history": [], "page": False, "recent": [], "video": None}

    async def frames():
        # as long as it plays and page view is open: no timer stops it
        await ch.send(b"M\x01")
        n = 0
        try:
            while st["page"]:
                r = await tool.ask("frame")
                if r.get("none"):
                    break
                await send(r["msgs"])
                n += 1
                if n % 8 == 0 and not (await tool.ask("playing")).get("playing"):
                    break
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
        for h in msgs:
            await ch.send(bytes.fromhex(h))

    async def answer(r):
        if r.get("url"):
            st["url"] = r["url"]
        if "links" in r:
            st["links"] = r["links"]
        await send(r.get("msgs", []))

    async def open_(u, push=True):
        if push and st["url"]:
            st["history"].append(st["url"])
        st["url"] = u
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
        await answer(r)
        host = urllib.parse.urlsplit(st["url"]).hostname or ""
        if "google." not in host and "duckduckgo" not in host and st["url"] not in st["recent"]:
            st["recent"] = ([st["url"]] + st["recent"])[:4]

    while True:
        m = (await ch.recv()).data
        print(m[:1].decode(errors="replace"), m[1:40], flush=True)
        c = m[:1]
        if st["video"] and st["video"].done():
            st["video"] = None
        if st["video"] and c != b"R":
            await stop_video()
            if c in (b"X", b"B") or m == b"V\x00":
                await tool.ask("stop")
        try:
            if m == b"R":
                for i, u in enumerate(st["recent"]):
                    await ch.send(b"H" + bytes([i]) + (urllib.parse.urlsplit(u).hostname or u).encode()[:60])
            elif c == b"Q":
                st["page"] = False
                await open_(url_for(m[1:].decode(errors="replace")))
            elif c == b"O" and len(m) == 2 and m[1] < len(st["recent"]):
                st["page"] = False
                await open_(st["recent"][m[1]])
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
                st["page"] = m[1] == 1
                if st["url"]:
                    await open_(st["url"], push=False)
            elif c == b"T" and len(m) == 5:
                x, y = int.from_bytes(m[1:3], "little"), int.from_bytes(m[3:5], "little")
                r = await tool.ask(f"tap {x} {y}")
                if r.get("field"):
                    await ch.send(b"K")
                else:
                    await answer(r)
            elif c == b"S" and len(m) == 3:
                await answer(await tool.ask(f"scroll {int.from_bytes(m[1:3], 'little', signed=True)}"))
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
    async with (SimChannel(a.sim) if a.sim else Channel(address=a.address, channel_key=True)) as ch:
        await send_when_open(ch, b"!")
        if a.web:
            tool = Tool()
            await tool.start()
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
