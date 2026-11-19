"""Mock phone for the Mail and Notes watch apps: both are the chats app (sdk/examples/chats/src/model.rs has the
protocol) in another mode. Demo data only; the iPhone app (CMFX) does Gmail and Notes + Reminders.

  uv run python hosts/inboxhost.py mail --sim 7760
  uv run python hosts/inboxhost.py notes --address <watch>

Mail: tap a thread to read it; `• Archive` / `• Mark unread` quick replies are actions, any other text is a reply.
Notes: row 0 starts a note; reminder lists tick with a tap (`t idx`); typing adds a line or a reminder.
"""
import argparse, asyncio, pathlib, sys

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parents[1] / "src"))
from wp2apps.channel import Channel, SimChannel, send_when_open  # noqa: E402

MAX_ROWS, MAX_MSGS, TEXT = 12, 12, 120


def cut(s, n):
    b = s.encode()[:n]
    return b.decode(errors="ignore").encode()


def chunks(text):
    """Text → ≤ 120-byte pieces on word boundaries."""
    out, cur = [], ""
    for w in text.split():
        nxt = f"{cur} {w}".strip()
        if len(nxt.encode()) > TEXT and cur:
            out.append(cur)
            cur = w
        else:
            cur = nxt
    return out + ([cur] if cur else [])


def msg(op, *head, text=""):
    return op.encode() + bytes(head) + cut(text, 128 - 1 - len(head))


class Mail:
    QUICK = ["• Archive", "• Mark unread", "Thanks!", "Got it, will reply soon", "Sounds good", "On it"]

    def __init__(self):
        self.threads = {
            1: dict(name="GitHub", subject="[cmfx] Build passed", unread=1, body=[(0, "All jobs have passed. CMFX-iOS artifact is ready to download from the run page.")]),
            2: dict(name="Priya Perera", subject="Lunch on Friday?", unread=1, body=[(0, "Hey! Are you free for lunch this Friday around 12:30? There is a new place near campus that does great kottu, my treat if you bring the slides.")]),
            3: dict(name="UCSC Exams", subject="Timetable: semester 2", unread=0, body=[(0, "Dear students, the semester 2 examination timetable is now published on the LMS. Please check your venues and report clashes before the 25th.")]),
            4: dict(name="Nothing", subject="Your order has shipped", unread=0, body=[(0, "Good news. Your order is on its way and should arrive in 3 to 5 working days. Track it from your account.")]),
            5: dict(name="Sam", subject="Re: weekend hike", unread=0, body=[(1, "Saturday 6am works for me."), (0, "Perfect, I will bring snacks. Meet at the station?")]),
        }

    def rows(self):
        return [(i, t["name"], t["unread"], t["subject"]) for i, t in self.threads.items()]

    def title(self, i):
        return self.threads[i]["name"]

    def lines(self, i):
        out = []
        for me, text in self.threads[i]["body"]:
            out += [(me, 0, c) for c in chunks(text)]
        return out

    def opened(self, i):
        self.threads[i]["unread"] = 0

    def send(self, i, text):
        """→ (messages to append, list changed, chat closed)"""
        if text == "• Archive":
            del self.threads[i]
            return [], True
        if text == "• Mark unread":
            self.threads[i]["unread"] = 1
            return [], True
        # the watch already shows its own reply: no echo
        self.threads[i]["body"].append((1, text))
        return [], True

    def tap(self, i, idx):
        return None


class Notes:
    QUICK = ["• Delete note", "• Clear done", "Buy ", "Call ", "Remember to "]
    NEW = 1

    def __init__(self):
        self.items = {
            2: dict(kind="list", name="Groceries", lines=[[3, "Milk"], [3, "Bread"], [3, "Coconut"], [4, "Eggs"]]),
            3: dict(kind="list", name="Today", lines=[[3, "Submit lab report"], [3, "Call the bank"], [4, "Gym"]]),
            4: dict(kind="note", name="Watch ideas", lines=[[0, "Crown scroll speed feels right now"], [0, "Maps: Apple tiles are much clearer"]]),
            5: dict(kind="note", name="Wi-Fi at home", lines=[[0, "Network: HomeNet-5G"], [0, "Router is behind the TV"]]),
        }
        self.next = 6

    def rows(self):
        # new note, notes newest first, then reminder lists
        out = [(self.NEW, "New note", 0, "Write one on the watch")]
        for i, it in reversed(self.items.items()):
            if it["kind"] == "note":
                out.append((i, it["name"], 0, it["lines"][0][1] if it["lines"] else "No text yet"))
        for i, it in self.items.items():
            if it["kind"] == "list":
                out.append((i, it["name"], 0, f"{sum(k == 3 for k, _ in it['lines'])} to do"))
        return out

    def title(self, i):
        return "New note" if i == self.NEW else self.items[i]["name"]

    def lines(self, i):
        if i == self.NEW:
            return []
        it = self.items[i]
        return [(0, k, c) for k, t in it["lines"] for c in (chunks(t) if k == 0 else [t])]

    def opened(self, i):
        pass

    def send(self, i, text):
        if i == self.NEW:
            # the first line written is the title; what follows is the body
            self.items[self.next] = dict(kind="note", name=text[:40], lines=[])
            self.next += 1
            return "reopen", True
        it = self.items[i]
        if text == "• Delete note":
            del self.items[i]
            return [], True
        if text == "• Clear done":
            it["lines"] = [l for l in it["lines"] if l[0] != 4]
            return "reload", True
        kind = 3 if it["kind"] == "list" else 0
        it["lines"].append([kind, text])
        return [(0, kind, text)], True

    def tap(self, i, idx):
        it = self.items.get(i)
        if not it or it["kind"] != "list" or idx >= len(it["lines"]):
            return None
        it["lines"][idx][0] = 4 if it["lines"][idx][0] == 3 else 3
        return idx, it["lines"][idx]


async def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("mode", choices=["mail", "notes"])
    ap.add_argument("--sim", type=int)
    ap.add_argument("--address", help="a real watch (K channel)")
    a = ap.parse_args()
    box = Mail() if a.mode == "mail" else Notes()
    opened = None

    async def send_list(ch):
        rows = box.rows()[:MAX_ROWS]
        await ch.send(b"L" + bytes([len(rows)]))
        for idx, (i, name, unread, preview) in enumerate(rows):
            n, p = cut(name, 24), cut(preview, 40)
            await ch.send(b"C" + bytes([idx, i, unread, len(n)]) + n + bytes([len(p)]) + p)
        for q, text in enumerate(box.QUICK):
            await ch.send(msg("Q", q, text=text))

    async def open_(ch, i):
        box.opened(i)
        await ch.send(b"H" + bytes([i]) + cut(box.title(i), 24))
        for idx, (me, kind, text) in enumerate(box.lines(i)[-MAX_MSGS:]):
            await ch.send(msg("M", idx, me, kind, text=text))

    async with (SimChannel(a.sim) if a.sim else Channel(address=a.address, channel_key=True)) as ch:
        await send_when_open(ch, b"?")  # waits till the app is open
        await send_list(ch)
        while True:
            m = (await ch.recv()).data
            print("watch:", m[:1].decode(errors="replace"), m[1:], flush=True)
            if m == b"R":
                await send_list(ch)
            elif m[:1] == b"o" and len(m) == 2:
                opened = m[1]
                await open_(ch, opened)
            elif m[:1] == b"s" and len(m) > 2:
                i, text = m[1], m[2:].decode(errors="replace")
                try:
                    add, changed = box.send(i, text)
                except KeyError:
                    continue
                if add == "reopen":
                    opened = max(box.items)
                    await open_(ch, opened)
                elif add == "reload":
                    await open_(ch, i)
                for me, kind, t in add if isinstance(add, list) else []:
                    await ch.send(msg("A", me, kind, text=t))
                if changed:
                    await send_list(ch)
            elif m[:1] == b"t" and len(m) == 2 and opened is not None:
                got = box.tap(opened, m[1])
                if got:
                    idx, (kind, text) = got
                    await ch.send(msg("M", idx, 0, kind, text=text))


if __name__ == "__main__":
    asyncio.run(main())
