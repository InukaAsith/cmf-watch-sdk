"""Mock phone for the Mail and Notes watch apps: both are the chats app (sdk/examples/chats/src/model.rs has the
protocol) in another mode. Demo data only; the iPhone app (CMFX) does Gmail and Notes + Reminders.

  uv run python hosts/inboxhost.py mail --sim 7760
  uv run python hosts/inboxhost.py notes --address <watch>

Mail: tap a thread to read it; `• Archive` / `• Mark unread` quick replies are actions, any other text is a reply.
Notes: row 0 starts a note; reminder lists tick with a tap (`t idx`); typing adds a line or a reminder.
"""
import argparse, asyncio, pathlib, sys

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parents[1] / "src"))
from wp2apps.channel import ANY, Channel, NoApp, SimChannel, send_when_open  # noqa: E402

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


# `C`'s unread for the new-chat row: the watch's + button
NEW_ROW = 254


class Mail:
    QUICK = ["• Archive", "• Mark unread", "Thanks!", "Got it, will reply soon", "Sounds good", "On it"]
    NEW = 99
    # whom a new mail can go to: the people written with
    PEOPLE = {100: ("Priya Perera", "priya@example.com"), 101: ("Sam", "sam@example.com"),
              102: ("Dilan Silva", "dilan@example.com"), 103: ("Amaya", "amaya@example.com")}

    def __init__(self):
        self.threads = {
            1: dict(name="GitHub", subject="[cmfx] Build passed", unread=1, body=[(0, "All jobs have passed. CMFX-iOS artifact is ready to download from the run page.")]),
            2: dict(name="Priya Perera", subject="Lunch on Friday?", unread=1, body=[(0, "Hey! Are you free for lunch this Friday around 12:30? There is a new place near campus that does great kottu, my treat if you bring the slides.")]),
            3: dict(name="UCSC Exams", subject="Timetable: semester 2", unread=0, body=[(0, "Dear students, the semester 2 examination timetable is now published on the LMS. Please check your venues and report clashes before the 25th.")]),
            4: dict(name="Nothing", subject="Your order has shipped", unread=0, body=[(0, "Good news. Your order is on its way and should arrive in 3 to 5 working days. Track it from your account.")]),
            5: dict(name="Sam", subject="Re: weekend hike", unread=0, body=[(1, "Saturday 6am works for me."), (0, "Perfect, I will bring snacks. Meet at the station?")]),
        }

    def rows(self):
        return [(self.NEW, "New mail", NEW_ROW, "Write one")] + [
            (i, t["name"], t["unread"], t["subject"]) for i, t in self.threads.items()]

    def picker(self):
        return [(i, n, 0, a) for i, (n, a) in self.PEOPLE.items()]

    def title(self, i):
        return "To " + self.PEOPLE[i][0] if i in self.PEOPLE else self.threads[i]["name"]

    def lines(self, i):
        if i in self.PEOPLE:
            return []
        out = []
        for me, text in self.threads[i]["body"]:
            out += [(me, 0, c) for c in chunks(text)]
        return out

    def opened(self, i):
        if i in self.threads:
            self.threads[i]["unread"] = 0

    def send(self, i, text):
        """→ (messages to append, list changed, chat closed)"""
        if i in self.PEOPLE:
            # a new mail: its first words the subject
            n = max(self.threads) + 1
            self.threads = {n: dict(name=self.PEOPLE[i][0], subject=text[:30], unread=0, body=[(1, text)]), **self.threads}
            return [], True
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
        out = [(self.NEW, "New note", NEW_ROW, "Write one on the watch")]
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


class Inbox:
    """One app's mock state and its answers; `slot` is the app's slot on the watch (ANY in the sim)."""

    def __init__(self, box, slot):
        self.box, self.slot, self.opened = box, slot, None

    async def out(self, ch, data):
        ch.slot = self.slot
        try:
            await ch.send(data)
        finally:
            ch.slot = ANY

    async def send_list(self, ch):
        rows = self.box.rows()[:MAX_ROWS]
        await self.out(ch, b"L" + bytes([len(rows)]))
        for idx, (i, name, unread, preview) in enumerate(rows):
            n, p = cut(name, 24), cut(preview, 40)
            await self.out(ch, b"C" + bytes([idx, i, unread, len(n)]) + n + bytes([len(p)]) + p)
        for q, text in enumerate(self.box.QUICK):
            await self.out(ch, msg("Q", q, text=text))

    async def open_(self, ch, i):
        self.opened = i
        self.box.opened(i)
        await self.out(ch, b"H" + bytes([i]) + cut(self.box.title(i), 24))
        for idx, (me, kind, text) in enumerate(self.box.lines(i)[-MAX_MSGS:]):
            await self.out(ch, msg("M", idx, me, kind, text=text))

    async def handle(self, ch, m):
        box = self.box
        if m == b"R":
            await self.send_list(ch)
        elif m[:1] == b"o" and len(m) == 2 and hasattr(box, "picker") and m[1] == box.NEW:
            # + : whom to write to, as a list under its title
            rows = box.picker()
            await self.out(ch, b"K" + b"New mail")
            await self.out(ch, b"L" + bytes([len(rows)]))
            for idx, (i, name, unread, preview) in enumerate(rows):
                n, p = cut(name, 24), cut(preview, 40)
                await self.out(ch, b"C" + bytes([idx, i, unread, len(n)]) + n + bytes([len(p)]) + p)
        elif m[:1] == b"o" and len(m) == 2:
            await self.open_(ch, m[1])
        elif m[:1] == b"s" and len(m) > 2:
            i, text = m[1], m[2:].decode(errors="replace")
            try:
                add, changed = box.send(i, text)
            except KeyError:
                return
            if add == "reopen":
                await self.open_(ch, max(box.items))
            elif add == "reload":
                await self.open_(ch, i)
            for me, kind, t in add if isinstance(add, list) else []:
                await self.out(ch, msg("A", me, kind, text=t))
            if changed:
                await self.send_list(ch)
        elif m[:1] == b"t" and len(m) == 2 and self.opened is not None:
            got = box.tap(self.opened, m[1])
            if got:
                idx, (kind, text) = got
                await self.out(ch, msg("M", idx, 0, kind, text=text))


async def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("mode", choices=["mail", "notes", "both"])
    ap.add_argument("--sim", type=int)
    ap.add_argument("--address", help="a real watch (K channel)")
    ap.add_argument("--mail-slot", type=int, default=6)
    ap.add_argument("--notes-slot", type=int, default=7)
    a = ap.parse_args()
    if a.mode == "both":
        apps = {a.mail_slot: Inbox(Mail(), a.mail_slot), a.notes_slot: Inbox(Notes(), a.notes_slot)}
    else:
        apps = {None: Inbox(Mail() if a.mode == "mail" else Notes(), ANY)}
    async with (SimChannel(a.sim) if a.sim else Channel(address=a.address, channel_key=True)) as ch:
        print("waiting for the app", flush=True)
        if None in apps:
            await send_when_open(ch, b"?")
            await apps[None].send_list(ch)
        else:
            # whichever of the two is open now gets its list (the other answers NoApp); later opens ask with R
            for app in apps.values():
                try:
                    await app.send_list(ch)
                except NoApp:
                    pass
        while True:
            m = await ch.recv()
            app = apps.get(m.slot) or apps.get(None)
            print(f"slot {m.slot}:", m.data[:1].decode(errors="replace"), m.data[1:], "" if app else "(not ours)", flush=True)
            if app:
                await app.handle(ch, m.data)


if __name__ == "__main__":
    asyncio.run(main())
