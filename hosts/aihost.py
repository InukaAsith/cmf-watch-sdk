"""Phone side of the assistant app: asks Gemini and streams the answer to the watch as it arrives.

  GEMINI_API_KEY=... uv run python hosts/aihost.py --sim 7704      # real Gemini (free key: aistudio.google.com)
  uv run python hosts/aihost.py --sim 7704                         # no key: canned demo answers

Watch protocol: examples/chats/src/model.rs (`A` a new bubble, `T` more text for it, `Q` suggested prompts).
"""
import argparse, asyncio, json, os, pathlib, re, sys, unicodedata, urllib.request

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parents[1] / "src"))
from wp2apps.channel import Channel, SimChannel, send_when_open  # noqa: E402

MODEL = os.environ.get("GEMINI_MODEL", "gemini-3.5-flash")
SYSTEM = ("You answer on a small round smartwatch screen. Reply in plain text, at most 60 words, no markdown, "
          "no lists, no emoji.")
PROMPTS = ["Fun fact", "Quick workout idea", "Explain black holes", "Dinner idea", "Motivate me"]
BUBBLE = 90  # bytes per bubble: 6 lines of 16 cells, with room for wrapping
DEMO = {
    "Fun fact": "Octopuses have three hearts and blue blood. Two hearts pump blood to the gills, the third to the rest "
                "of the body, and it stops beating while they swim, which is why they prefer crawling.",
    "Quick workout idea": "Try 4 rounds: 15 squats, 10 push ups, 20 second plank, 30 jumping jacks. Rest 45 seconds "
                          "between rounds. Takes about 12 minutes.",
}


def clean(t: str) -> str:
    """The watch font is ASCII (and Cyrillic): no markdown, no emoji, plain quotes."""
    t = re.sub(r"[*`#>]+|(?<![A-Za-z0-9])_+|_+(?![A-Za-z0-9])", "", t)
    t = t.replace("’", "'").replace("‘", "'").replace("“", '"').replace("”", '"')
    t = t.replace("—", "-").replace("–", "-").replace("…", "...")
    t = unicodedata.normalize("NFKD", t)
    return "".join(c for c in t if c.isascii() and (c.isprintable() or c == " ")).replace("\n", " ")


async def gemini_stream(history):
    """Yields text pieces from Gemini's SSE stream (blocking HTTP in a thread)."""
    key = os.environ["GEMINI_API_KEY"]
    url = f"https://generativelanguage.googleapis.com/v1beta/models/{MODEL}:streamGenerateContent?alt=sse"
    body = json.dumps({"systemInstruction": {"parts": [{"text": SYSTEM}]}, "contents": history}).encode()
    req = urllib.request.Request(url, body, {"Content-Type": "application/json", "x-goog-api-key": key})
    q: asyncio.Queue = asyncio.Queue()
    loop = asyncio.get_running_loop()

    def pump():
        try:
            with urllib.request.urlopen(req, timeout=60) as r:
                for line in r:
                    if line.startswith(b"data: "):
                        d = json.loads(line[6:])
                        for c in d.get("candidates", []):
                            for p in c.get("content", {}).get("parts", []):
                                if p.get("text"):
                                    loop.call_soon_threadsafe(q.put_nowait, p["text"])
        except Exception as e:
            loop.call_soon_threadsafe(q.put_nowait, f" (error: {e})")
        loop.call_soon_threadsafe(q.put_nowait, None)

    loop.run_in_executor(None, pump)
    while (piece := await q.get()) is not None:
        yield piece


async def demo_stream(prompt):
    text = DEMO.get(prompt, f"Demo mode: set GEMINI_API_KEY for real answers. You asked: {prompt}")
    for word in re.findall(r"\S+\s*", text):
        await asyncio.sleep(0.06)
        yield word


async def answer(ch, history, prompt):
    stream = gemini_stream(history) if os.environ.get("GEMINI_API_KEY") else demo_stream(prompt)
    full, used = "", None
    async for piece in stream:
        full += piece
        for word in re.findall(r"\S+\s*|\s+", clean(piece)):
            # a new bubble when this one is full
            if used is None or used + len(word) > BUBBLE:
                await ch.send(b"A\x00\x00")
                used = 0
                word = word.lstrip()
            if word:
                await ch.send(b"T" + word.encode()[:120])
                used += len(word)
    if used is None:
        await ch.send(b"A\x00\x00" + b"(no answer)")
    history.append({"role": "model", "parts": [{"text": full}]})


def chunks(text, n=BUBBLE):
    out, cur = [], ""
    for w in clean(text).split():
        if len((cur + " " + w).strip()) > n and cur:
            out.append(cur)
            cur = w
        else:
            cur = (cur + " " + w).strip()
    return out + ([cur] if cur else [])


async def main():
    """The watch lists New chat (id 1) and the last 3 conversations; a question to New chat makes a conversation
    (the phone opens it with `H`). Every question is acknowledged at once (`Q`) so the watch does not ask again."""
    ap = argparse.ArgumentParser()
    ap.add_argument("--sim", type=int)
    ap.add_argument("--address", help="a real watch (K channel)")
    ap.add_argument("--delay", type=float, default=0, help="seconds before an answer starts (a slow model)")
    ap.add_argument("--deaf", type=int, default=0, help="ignore the first N questions (a phone that was asleep)")
    a = ap.parse_args()
    print("Gemini" if os.environ.get("GEMINI_API_KEY") else "demo mode (no GEMINI_API_KEY)", flush=True)
    convos = {}  # id -> history, newest last in order
    current = [None]
    deaf = [a.deaf]
    async with (SimChannel(a.sim) if a.sim else Channel(address=a.address, channel_key=True)) as ch:

        async def quick():
            for i, p in enumerate(PROMPTS):
                await ch.send(b"Q" + bytes([i]) + p.encode())

        async def send_list():
            rows = [(1, "New chat", "Ask anything")] + [
                (i, h[0]["parts"][0]["text"], next((t["parts"][0]["text"] for t in reversed(h) if t["role"] == "model"), ""))
                for i, h in reversed(list(convos.items()))]
            await ch.send(b"L" + bytes([len(rows)]))
            for k, (i, name, prev) in enumerate(rows):
                n, pv = clean(name).encode()[:24], clean(prev).encode()[:40]
                await ch.send(b"C" + bytes([k, i, 0, len(n)]) + n + bytes([len(pv)]) + pv)
            await quick()

        async def open_chat(i):
            bubbles = [(t["role"] == "user", c) for t in convos[i] for c in chunks(t["parts"][0]["text"])][-10:]
            await ch.send(b"H" + bytes([i]) + b"Gemini")
            for k, (me, c) in enumerate(bubbles):
                await ch.send(b"M" + bytes([k, int(me), 0]) + c.encode()[:120])

        await send_when_open(ch, b"?")  # waits till the app is open
        await send_list()
        while True:
            m = (await ch.recv()).data
            print("watch:", m, flush=True)
            if m == b"R":
                await send_list()
            elif m[:1] == b"o" and len(m) == 2:
                current[0] = None if m[1] == 1 else m[1]
                if m[1] in convos:
                    await open_chat(m[1])
                else:
                    await ch.send(b"H\x01Gemini")
            elif m[:1] == b"s":
                if deaf[0] > 0:
                    deaf[0] -= 1
                    continue
                prompt = m[2:].decode(errors="replace")
                if current[0] is None:
                    current[0] = next(i for i in range(2, 250) if i not in convos)
                    convos[current[0]] = []
                    fresh = True
                else:
                    fresh = False
                h = convos.pop(current[0])
                convos[current[0]] = h
                while len(convos) > 3:
                    convos.pop(next(iter(convos)))
                h.append({"role": "user", "parts": [{"text": prompt}]})
                await (open_chat(current[0]) if fresh else quick())
                await asyncio.sleep(a.delay)
                await answer(ch, h, prompt)
                await send_list()  # the conversation now heads the list


if __name__ == "__main__":
    asyncio.run(main())
