"""Mac side of the herdr-agents watch app: polls `herdr agent list`, sends the list as deltas over the ABI 1.1 channel
and focuses the pane the watch asks for. Protocol: docs/herdr-agents.md."""
import asyncio
from dataclasses import dataclass
import json
import sys
import time

from . import channel

MAGIC = b"h1"
REQUEST, FOCUS = b"R", b"F"
STATUS = {"idle": 0, "working": 1, "blocked": 2, "done": 3}
WS_MAX, TITLE_MAX = 24, 64
SHOWN = 32  # the app keeps this many; blocked and done go first
RANK = {2: 0, 3: 1, 1: 2, 0: 3}


def clip(text: str, size: int) -> bytes:
    """At most `size` bytes of UTF-8, cut on a char boundary."""
    return text.encode()[:size].decode("utf-8", "ignore").encode()


@dataclass(frozen=True)
class Agent:
    status: int
    ws: bytes
    title: bytes


def top(agents: dict[str, "Agent"]) -> dict[str, "Agent"]:
    return dict(sorted(agents.items(), key=lambda kv: RANK.get(kv[1].status, 4))[:SHOWN])


def parse(agents_json: str, workspaces_json: str) -> tuple[dict[str, Agent], dict[str, str]]:
    """{pane_id: Agent}, {pane_id: tab_id} from `herdr agent list` and `herdr workspace list`."""
    labels = {w["workspace_id"]: w.get("label") or "" for w in json.loads(workspaces_json)["result"]["workspaces"]}
    agents, tabs = {}, {}
    for a in json.loads(agents_json)["result"]["agents"]:
        title = a.get("terminal_title_stripped") or a.get("name") or a.get("agent") or "?"
        agents[a["pane_id"]] = Agent(STATUS.get(a.get("agent_status"), 4),
                                     clip(labels.get(a.get("workspace_id"), ""), WS_MAX), clip(title, TITLE_MAX))
        tabs[a["pane_id"]] = a.get("tab_id")
    return agents, tabs


class Sync:
    """Short ids for panes, the list the watch has, and the message seq."""

    def __init__(self):
        self.ids: dict[str, int] = {}
        self.sent: dict[str, Agent] = {}
        self.seq = 0
        self._next = 0

    def _msg(self, kind: bytes, body: bytes = b"") -> bytes:
        self.seq = (self.seq + 1) & 0xFF
        return MAGIC + kind + bytes([self.seq]) + body

    def _id(self, pane: str) -> int:
        if pane not in self.ids:
            used = set(self.ids.values())
            while self._next in used:
                self._next = (self._next + 1) & 0xFF
            self.ids[pane], self._next = self._next, (self._next + 1) & 0xFF
        return self.ids[pane]

    def _upsert(self, pane: str, a: Agent) -> bytes:
        return self._msg(b"U", bytes([self._id(pane), a.status, len(a.ws)]) + a.ws + a.title)

    def snapshot(self, agents: dict[str, Agent]) -> list[bytes]:
        agents = top(agents)
        for pane in set(self.ids) - set(agents):
            del self.ids[pane]
        self.sent = dict(agents)
        return [self._msg(b"B"), *(self._upsert(p, a) for p, a in agents.items()), self._msg(b"E", bytes([len(agents)]))]

    def delta(self, agents: dict[str, Agent]) -> list[bytes]:
        agents = top(agents)
        out = [self._msg(b"D", bytes([self.ids.pop(p)])) for p in self.sent if p not in agents]
        out += [self._upsert(p, a) for p, a in agents.items() if self.sent.get(p) != a]
        self.sent = dict(agents)
        return out

    def beat(self) -> bytes:
        return self._msg(b"H")

    def pane(self, ident: int) -> str | None:
        return next((p for p, i in self.ids.items() if i == ident), None)


async def run_herdr(herdr: str, *args: str) -> str:
    proc = await asyncio.create_subprocess_exec(herdr, *args, stdout=asyncio.subprocess.PIPE,
                                                stderr=asyncio.subprocess.DEVNULL)
    out, _ = await proc.communicate()
    if proc.returncode:
        raise OSError(f"herdr {args[0]} {args[1]} exited with {proc.returncode}")
    return out.decode()


async def read_state(herdr: str):
    return parse(await run_herdr(herdr, "agent", "list"), await run_herdr(herdr, "workspace", "list"))


async def focus(herdr: str, pane: str, tab: str | None) -> bool:
    for args in (("agent", "focus", pane), ("tab", "focus", tab)):
        if args[-1]:
            try:
                await run_herdr(herdr, *args)
                return True
            except OSError:
                continue
    return False


class Daemon:
    """One task: wait for app messages (Channel.recv polls PING), poll herdr every `period` s, send deltas.
    The app asks for a snapshot (R) on start, on resume and after a gap; until then nothing is sent."""

    def __init__(self, ch: channel.Channel, herdr: str = "herdr", period: float = 1.0, beat: float = 10.0,
                 verbose: bool = False, log=None, on_key=None, chat: bool = False):
        self.ch, self.herdr, self.period, self.beat_every, self.verbose = ch, herdr, period, beat, verbose
        # chat: stdin lines go to whatever app runs, its non-herdr messages are printed (api-demo's log)
        self.chat = chat
        self.lines: asyncio.Queue[str] = asyncio.Queue()
        self.log = log or (lambda text: print(f"{time.strftime('%H:%M:%S')} {text}", file=sys.stderr, flush=True))
        self.on_key = on_key
        self.sync = Sync()
        self.agents: dict[str, Agent] = {}
        self.tabs: dict[str, str] = {}
        self.synced = False
        self.focused: list[str] = []
        self.last_sent = 0.0
        self._key = None

    async def deliver(self, messages: list[bytes]) -> None:
        try:
            for m in messages:
                await self.ch.send(m, timeout=5)
        except (channel.NoApp, TimeoutError, ValueError) as error:
            self.log(f"app gone ({type(error).__name__}); waiting for it to ask again")
            self.synced, self.ch.slot = False, channel.ANY
            return
        self.last_sent = time.monotonic()

    async def say(self, line: str) -> None:
        try:
            session = await self.ch.connect()
            await channel.send(session, line.encode()[:channel.MSG_MAX], channel.ANY, wait=5.0)
        except (channel.NoApp, TimeoutError, ValueError, *channel.LOST) as error:
            self.log(f"chat: not sent ({type(error).__name__}: {error})")

    async def message(self, m: channel.Message) -> None:
        if m.data[:2] != MAGIC or len(m.data) < 3:
            if self.chat:
                print(f"< {channel.text(m.data) or m.data.hex()}", flush=True)
            return
        kind = m.data[2:3]
        if kind == REQUEST:
            self.ch.slot = m.slot
            self.synced = True
            self.log(f"app asked for a snapshot (slot {m.slot}): {len(self.agents)} agents")
            await self.deliver(self.sync.snapshot(self.agents))
        elif kind == FOCUS and len(m.data) == 4:
            pane = self.sync.pane(m.data[3])
            ok = pane is not None and await focus(self.herdr, pane, self.tabs.get(pane))
            if ok:
                self.focused.append(pane)
            self.log(f"focus {pane or m.data[3]}: {'ok' if ok else 'failed'}")

    async def poll(self) -> None:
        try:
            self.agents, self.tabs = await read_state(self.herdr)
        except (OSError, ValueError, KeyError) as error:
            self.log(f"herdr: {error}")
            return
        if not self.synced:
            return
        out = self.sync.delta(self.agents)
        if self.verbose:
            for m in out:
                self.log(f"-> {m[2:3].decode()} {m[4:].decode('utf-8', 'replace')}")
        if not out and time.monotonic() - self.last_sent >= self.beat_every:
            out = [self.sync.beat()]
        if out:
            await self.deliver(out)

    async def run(self) -> None:
        loop = asyncio.get_running_loop()
        next_poll = 0.0
        if self.chat:
            loop.add_reader(sys.stdin, lambda: self.lines.put_nowait(sys.stdin.readline()))
        while True:
            while not self.lines.empty():
                line = self.lines.get_nowait().rstrip("\n")
                if line:
                    await self.say(line)
            self.ch.poll = 0.25 if self.synced else 1.0  # PING cost on the watch: slow until the app is open
            try:
                wait = max(next_poll - loop.time(), 0.05)
                m = await self.ch.recv(timeout=min(wait, 0.3) if self.chat else wait)
            except TimeoutError:
                m = None
            if self.ch.key != self._key:
                self._key = self.ch.key
                self.log("connected")
                if self.on_key:
                    self.on_key(self.ch.key)
            if m:
                await self.message(m)
            if loop.time() >= next_poll:
                next_poll = loop.time() + self.period
                await self.poll()


def main(args) -> int:
    from .cli import cache_key, cached_key
    if getattr(args, "sim", None):
        ch, on_key = channel.SimChannel(args.sim), None
    else:
        target = f"ble:{args.address}" if args.address else f"hci:{args.hci_port}"
        ch = channel.Channel(address=args.address, hci_port=args.hci_port)
        ch.key = cached_key(target)  # resume the bound session: a new bind shows a popup that ends the running app
        on_key = lambda key: cache_key(target, key)  # noqa: E731
    daemon = Daemon(ch, herdr=args.herdr, period=args.period, verbose=args.verbose, on_key=on_key,
                    chat=getattr(args, "chat", False))
    daemon.log("connecting" + ("" if on_key is None else " (cached session)" if ch.key
                               else " (no cached session: binds, open the app after)"))
    try:
        asyncio.run(daemon.run())
    except KeyboardInterrupt:
        pass
    return 0
