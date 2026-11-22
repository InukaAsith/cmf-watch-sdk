"""Phone side of the YouTube Music app (sdk/examples/ytmusic), run on the Mac; CMFX does the same on the iPhone.
sdk/examples/ytmusic/src/model.rs has the protocol.

  uv run python hosts/ytmhost.py --music ~/Music/some-folder          # songs from files (search by file name)
  uv run python hosts/ytmhost.py --yt                                  # YouTube Music through yt-dlp

The watch searches, picks and skips; this finds the song, decodes it, shapes it for the watch's speaker (wp2apps
tune) and streams it as ADPCM under the watch's credit (`F next room gen`). Audio is streamed only: a song is
decoded into memory while it plays, never kept. Up next: the rest of the search results, in order.
"""
import argparse, asyncio, json, pathlib, shutil, subprocess, sys, tempfile, time

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parents[1] / "src"))
from wp2apps import adpcm, channel, play, tune  # noqa: E402

MSG_MAX = 128
AUDIO_EXT = {".mp3", ".m4a", ".aac", ".wav", ".aiff", ".aif", ".flac", ".caf"}


def field(s: str, n: int) -> bytes:
    b = s.encode()[:n]
    while b:
        try:
            b.decode()
            break
        except UnicodeDecodeError:
            b = b[:-1]
    return bytes([len(b)]) + b


def song_message(pos: int, song: dict) -> bytes:
    m = b"N" + pos.to_bytes(2, "little") + min(song["dur"], 0xFFFF).to_bytes(2, "little") + field(song["title"], 48)
    return (m + song["artist"].encode()[:40])[:MSG_MAX]


def list_messages(kind: bytes, rows) -> list[bytes]:
    out = [b"L" + kind + bytes([min(len(rows), 24)])]
    for i, (title, sub, active) in enumerate(rows[:24]):
        out.append((b"l" + bytes([i, 1 if active else 0]) + field(title, 40) + sub.encode())[:MSG_MAX])
    return out


def mmss(s: int) -> str:
    return f"{s // 60}:{s % 60:02d}"


class Files:
    """Songs from a folder: "Artist - Title.mp3" or just "Title.mp3"."""

    def __init__(self, root: pathlib.Path):
        self.songs = []
        for p in sorted(root.rglob("*")):
            if p.suffix.lower() in AUDIO_EXT:
                artist, _, title = p.stem.rpartition(" - ")
                self.songs.append({"path": p, "title": title or p.stem, "artist": artist, "dur": _duration(p)})

    def search(self, q: str) -> list[dict]:
        words = q.lower().split()
        return [s for s in self.songs if all(w in f'{s["title"]} {s["artist"]}'.lower() for w in words)]

    def pcm(self, song: dict, pos: int) -> list[int]:
        return play.pcm16k(song["path"], start=pos)


def _duration(p: pathlib.Path) -> int:
    try:
        out = subprocess.run(["afinfo", str(p)], capture_output=True, text=True, timeout=10).stdout
        for line in out.splitlines():
            if "estimated duration" in line:
                return int(float(line.split(":")[1].split()[0]))
    except (OSError, ValueError, subprocess.TimeoutExpired):
        pass
    return 0


class YouTube:
    """YouTube Music through yt-dlp: search, then the song's AAC stream (itag 140) into a temporary file that is
    decoded and deleted at once."""

    def __init__(self):
        self.bin = shutil.which("yt-dlp") or sys.exit("yt-dlp is needed for --yt")

    def search(self, q: str) -> list[dict]:
        out = subprocess.run([self.bin, "--ignore-config", "--flat-playlist", "-j", f"ytsearch12:{q}"],
                             capture_output=True, text=True, timeout=60).stdout
        songs = []
        for line in out.splitlines():
            v = json.loads(line)
            if v.get("duration") and v["duration"] < 20 * 60:
                songs.append({"id": v["id"], "title": v.get("title", "?"), "artist": v.get("channel", "") or "",
                              "dur": int(v["duration"])})
        return songs

    def pcm(self, song: dict, pos: int) -> list[int]:
        with tempfile.TemporaryDirectory() as d:
            f = pathlib.Path(d) / "a.m4a"
            subprocess.run([self.bin, "--ignore-config", "-q", "-f", "140/bestaudio[ext=m4a]", "-o", str(f),
                            f"https://music.youtube.com/watch?v={song['id']}"], check=True, timeout=300)
            return play.pcm16k(f, start=pos)


class Host:
    def __init__(self, session, source, log=print, seconds: float | None = None, enhance: bool = False,
                 smooth: bool = True):
        self.session, self.src, self.log, self.seconds = session, source, log, seconds
        # loudness levelling and the trellis encoder (off: the owner preferred the sound without them)
        self.enhance = enhance
        self.smooth = smooth  # volume smoothing: tune's compressor
        self.pipe = channel.Pipe(session)
        self.results: list[dict] = []
        self.queue: list[dict] = []
        self.index = 0
        self.gen = None
        self.out: list[bytes] = []  # this stream's messages, in order
        self.sent = 0
        self.seq = 0  # the next block's number in this stream
        self.credit = {"next": 0, "room": 8, "gen": None}
        self.control: list[bytes] = []  # lists and notes, ahead of the stream
        self.job: asyncio.Task | None = None
        self.starts: list[tuple[int, int]] = []  # (first block's seq, queue index) of each song in this stream
        self.stopped = False

    # --- songs

    async def _render(self, song: dict, pos: int, gen: int, told: bool):
        """The song's blocks in parts of ~10 s as they are encoded (the trellis encoder is the slow part: the first
        part goes out while the rest is encoded). `told`: the watch waits for it, so it hears how far preparing
        got (`r gen pct`: its yellow circle)."""
        run = asyncio.get_running_loop().run_in_executor

        def say(pct: int):
            if told and gen == self.gen:
                self.control.append(b"r" + bytes([gen, pct]))

        say(5)
        t0 = time.monotonic()
        s = await run(None, self.src.pcm, song, pos)
        if self.seconds:
            s = s[: int(self.seconds * play.RATE)]
        say(35)
        s = await run(None, lambda: tune.for_speaker(s, play.RATE, level=int(self.enhance), smooth=int(self.smooth))) if s else s
        say(75)
        self.log(f"  decoded and tuned {song['title']!r} ({len(s) / play.RATE:.0f} s) in {time.monotonic() - t0:.1f} s")
        enc = adpcm.Encoder(play.PATHS if self.enhance else 0)
        part = 41 * play.SAMPLES  # ~0.94 s
        for k in range(0, len(s), part):
            if gen != self.gen:
                return
            yield await run(None, play.messages, s[k:k + part], enc)
            if k == 0:
                say(100)

    def _renumber(self, msgs: list[bytes]) -> list[bytes]:
        out = []
        for m in msgs:
            out.append(b"a" + (self.seq & 0xFFFF).to_bytes(2, "little") + m[3:])
            self.seq += 1
        return out

    async def start(self, gen: int, pos: int = 0):
        """A new stream for the watch's `gen`: the song at index from `pos`, then the rest of the queue."""
        if self.job:
            self.job.cancel()
        self.gen, self.out, self.sent, self.seq, self.starts = gen, [], 0, 0, []
        self.credit = {"next": 0, "room": 8, "gen": gen}
        if not 0 <= self.index < len(self.queue):
            self.out = [b"o" + bytes([play.OUT_KHZ, 1, gen]), b"z"]
            return
        self.job = asyncio.create_task(self._feed(gen, pos))

    def current(self) -> int:
        """The song the watch plays: the last one whose blocks it has heard (credit next - held)."""
        heard = self.credit.get("heard", self.credit["next"])
        at = [i for seq, i in self.starts if seq <= heard]
        return at[-1] if at else (self.starts[0][1] if self.starts else self.index)

    async def _feed(self, gen: int, pos: int):
        try:
            # the open first: the watch shows the song being prepared at once
            self.out += [b"o" + bytes([play.OUT_KHZ, 1, gen]), b"x" + bytes([40])]
            first = True
            i = self.index
            while i < len(self.queue):
                song = self.queue[i]
                marked = False
                async for blocks in self._render(song, pos, gen, told=first):
                    if gen != self.gen:
                        return
                    if not marked:
                        self.out.append(song_message(pos, song))
                        self.starts.append((self.seq, i))
                        marked = True
                    self.out += self._renumber(blocks)
                first = False
                pos = 0
                # the next song is fetched while this one plays: start when ~20 s of it are left to send
                while self.sent < len(self.out) - int(20 * play.RATE / play.SAMPLES):
                    await asyncio.sleep(0.2)
                    if gen != self.gen:
                        return
                i += 1
            self.out.append(b"z")
        except asyncio.CancelledError:
            pass
        except Exception as e:  # a song that fails must not end the session
            self.log(f"error: {e}")
            self.control.append(b"X" + str(e).encode()[:60])
            self.out.append(b"z")

    # --- the watch

    async def command(self, m: bytes):
        k = m[:1]
        if m == b"R" or m == b"q":
            self.gen, self.out, self.sent = None, [], 0
            if self.job:
                self.job.cancel()
            self.stopped = m == b"q"
        elif k == b"F" and len(m) in (5, 7):
            if m[4] == self.gen:
                nxt = int.from_bytes(m[1:3], "little")
                held = int.from_bytes(m[5:7], "little") if len(m) == 7 else 0
                self.credit = {"next": nxt, "room": m[3], "gen": m[4], "heard": max(0, nxt - held)}
        elif k == b"s":
            q = m[1:].decode(errors="replace")
            self.log(f"search {q!r}")
            self.results = await asyncio.get_running_loop().run_in_executor(None, self.src.search, q)
            self.control += list_messages(b"S", [(s["title"], f'{s["artist"]} · {mmss(s["dur"])}'.strip(" ·"), False)
                                                 for s in self.results])
        elif k == b"o" and m[1:2] == b"Q":
            self.index = self.current()
            rows = [(s["title"], s["artist"], i == self.index) for i, s in enumerate(self.queue)][self.index:]
            self.control += list_messages(b"Q", [(t, a, i == 0) for i, (t, a, _) in enumerate(rows)])
            self._upnext_from = self.index
        elif k == b"o" and m[1:2] == b"H":
            self.history = getattr(self, "history", [])
            self.control += list_messages(b"H", [(t["title"], t["artist"], False) for t in self.history])
        elif k == b"p" and len(m) == 4:
            gen, kind, i = m[1], m[2:3], m[3]
            if kind == b"S" and i < len(self.results):
                self.queue, self.index = self.results[:], i
            elif kind == b"H" and i < len(getattr(self, "history", [])):
                self.queue, self.index = [self.history[i]], 0
            elif kind == b"Q":
                self.index = getattr(self, "_upnext_from", self.index) + i
            self.log(f"play {self.queue[self.index]['title']!r}" if self.index < len(self.queue) else "play: none")
            if self.index < len(self.queue):
                song = self.queue[self.index]
                self.history = [song] + [t for t in getattr(self, "history", []) if t is not song][:23]
            await self.start(gen)
        elif k in (b"n", b"b") and len(m) == 2:
            self.index = self.current() + (1 if k == b"n" else -1)
            self.index = max(self.index, 0)
            self.log(f"{'next' if k == b'n' else 'previous'}: {self.queue[self.index]['title']!r}" if self.index < len(self.queue) else "next: end of the queue")
            await self.start(m[1])
        elif k == b"j" and len(m) == 4:
            self.index = self.current()
            await self.start(m[1], int.from_bytes(m[2:4], "little"))

    def _batch(self, most: int = 16) -> list[bytes]:
        batch = self.control[:most]
        del self.control[: len(batch)]
        limit = self.credit["next"] + self.credit["room"]
        while len(batch) < most and self.sent < len(self.out):
            m = self.out[self.sent]
            if m[:1] == b"a" and int.from_bytes(m[1:3], "little") >= limit:
                break
            batch.append(m)
            self.sent += 1
        return batch

    async def run(self, once: bool = True):
        """Serves the app while it is open; `once` False: waits for it to open, and again after it closes."""
        while True:
            try:
                while not self.session.events.empty():
                    self.session.events.get_nowait()
                await self.pipe.send([b"?"])  # the app says R
            except channel.NoApp:
                if once:
                    return
                await asyncio.sleep(2)
                continue
            self.log("YouTube Music is open on the watch")
            await self._serve()
            if once:
                return
            self.stopped = False
            self.log("closed: waiting for it to open again")

    async def _serve(self):
        while not self.stopped:
            while not self.session.events.empty():
                await self._event(self.session.events.get_nowait())
            batch = self._batch()
            if batch:
                try:
                    # a sliding window (4 in flight) through the batch: stop-and-wait per 4 only just kept up
                    await self.pipe.stream(batch)
                except channel.NoApp:
                    return
                self._speed(sum(1 for m in batch if m[:1] == b"a"))
                continue
            try:
                body = await asyncio.wait_for(self.session.events.get(), 0.03)
                await self._event(body)
            except TimeoutError:
                await channel.ping(self.session)  # the watch sends its EVENTs after a host frame

    def _speed(self, blocks: int):
        now = time.monotonic()
        t0, n = getattr(self, "_rate", (now, 0))
        n += blocks
        if now - t0 >= 10:
            need = play.RATE / play.SAMPLES
            self.log(f"  link {n / (now - t0):.1f} blocks/s ({n / (now - t0) / need:.2f}x the song), watch room {self.credit['room']}")
            t0, n = now, 0
        self._rate = (t0, n)

    async def _event(self, body):
        try:
            m = channel.message(self.session.key, body).data
        except ValueError:
            return
        if m[:1] != b"F":
            self.log(f"watch: {m[:40]!r}")
        await self.command(m)


async def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--music", type=pathlib.Path, help="a folder of songs")
    ap.add_argument("--yt", action="store_true", help="YouTube Music through yt-dlp")
    ap.add_argument("--address")
    ap.add_argument("--hci-port", type=int)
    ap.add_argument("--seconds", type=float, help="only this much of each song (tests)")
    ap.add_argument("--enhance", action="store_true", help="loudness levelling and the trellis encoder")
    ap.add_argument("--no-smooth", action="store_true", help="no volume smoothing (the compressor)")
    a = ap.parse_args()
    src = YouTube() if a.yt else Files(a.music or sys.exit("--music DIR or --yt"))
    async with channel.Channel(address=a.address, hci_port=a.hci_port, channel_key=True) as ch:
        session = await ch.connect()
        print("waiting for YouTube Music to open on the watch", flush=True)
        await Host(session, src, seconds=a.seconds, enhance=a.enhance, smooth=not a.no_smooth).run(once=False)


if __name__ == "__main__":
    asyncio.run(main())
