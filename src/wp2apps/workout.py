"""The workout app's Mac side (docs/workout.md): parse the owner's Markdown notes, encode the program for the watch,
push it over the "w1" protocol and read the history back."""
import asyncio
from dataclasses import dataclass
import json
from pathlib import Path
import re
import struct
import sys
import time
import zlib

from . import channel
from .herdr import clip

NAME_MAX, WEIGHT_MAX, PROGRAM_MAX, DAYS_MAX, EX_MAX, REST_DEFAULT = 32, 32, 2048, 7, 12, 90
CHUNK = 120
MAGIC = b"w1"
WAIT = 120.0  # seconds push and history wait for the app to ask
RUNNING = 1  # R flags: a workout is in progress

ITEM = re.compile(r"^\s*[-*+]\s+(?:\[[ xX]\]\s*)?(?P<body>.*?)\s*$")
BODY = re.compile(r"^(?P<name>.+?):?\s+(?P<sets>\d+)\s*[×xXхХ]\s*(?P<lo>\d+)(?:\s*[–-]\s*(?P<hi>\d+))?"
                  r"(?:\s*\((?P<weight>[^)]*)\))?(?:\s*отдых\s+(?P<m>\d+):(?P<s>[0-5]\d))?$")
HEADING = re.compile(r"^##\s+(?P<title>.+?)\s*$")
DAY_PREFIX = re.compile(r"^День\s*\d+\s*[:.]\s*")


@dataclass(frozen=True)
class Exercise:
    name: str
    sets: int
    reps: tuple[int, int]
    weight: str
    rest_s: int


@dataclass(frozen=True)
class Day:
    name: str
    exercises: tuple[Exercise, ...]


@dataclass(frozen=True)
class Entry:
    start: int  # seconds since 2000-01-01 local (or since boot if the watch had no clock)
    dur_s: int
    day: int
    done: int
    planned: int


class ParseError(ValueError):
    """"<file>:<line>: <reason>"."""


def parse_day(text: str, file: str) -> Day:
    name, exercises = None, []
    for n, line in enumerate(text.splitlines(), 1):
        if name is None:
            if m := HEADING.match(line):
                name = DAY_PREFIX.sub("", m["title"]) or m["title"]
            continue
        item = ITEM.match(line)
        if not item:
            continue
        m = BODY.match(item["body"])
        if not m:
            raise ParseError(f"{file}:{n}: expected '<name>: <sets>×<reps>[–<reps>] [(<weight>)] [отдых m:ss]', "
                             f"got {item['body']!r}")
        lo = int(m["lo"])
        rest = int(m["m"]) * 60 + int(m["s"]) if m["m"] else REST_DEFAULT
        exercises.append(Exercise(m["name"].strip(), int(m["sets"]), (lo, int(m["hi"] or lo)),
                                  (m["weight"] or "").strip(), rest))
    if name is None:
        raise ParseError(f"{file}: no '## ' heading")
    if not exercises:
        raise ParseError(f"{file}: no exercises under '## {name}'")
    return Day(name, tuple(exercises))


def parse_dir(path: Path) -> list[Day]:
    """Every *.md with a '## ' heading, sorted by file name."""
    days = []
    for f in sorted(Path(path).glob("*.md")):
        text = f.read_text(encoding="utf-8")
        if any(HEADING.match(line) for line in text.splitlines()):
            days.append(parse_day(text, f.name))
    return days


def _text(s: str, size: int) -> bytes:
    # the system font has no Ё/ё (docs/workout.md, Fonts)
    b = clip(s.replace("Ё", "Е").replace("ё", "е"), size)
    return bytes([len(b)]) + b


def encode(days: list[Day]) -> bytes:
    """The watch's program format (docs/plans/2026-11-13-workout-app.md, Task 4)."""
    if not 1 <= len(days) <= DAYS_MAX:
        raise ValueError(f"{len(days)} days: 1..{DAYS_MAX}")
    out = bytearray([len(days)])
    for d in days:
        if not 1 <= len(d.exercises) <= EX_MAX:
            raise ValueError(f"day {d.name!r}: {len(d.exercises)} exercises, 1..{EX_MAX}")
        out += _text(d.name, NAME_MAX) + bytes([len(d.exercises)])
        for e in d.exercises:
            lo, hi = e.reps
            if not (1 <= e.sets <= 255 and 0 <= lo <= hi <= 255 and 0 <= e.rest_s <= 0xFFFF):
                raise ValueError(f"{e.name!r}: sets {e.sets}, reps {lo}–{hi}, rest {e.rest_s} s out of range")
            out += _text(e.name, NAME_MAX) + bytes([e.sets, lo, hi]) + struct.pack("<H", e.rest_s)
            out += _text(e.weight, WEIGHT_MAX)
    if len(out) > PROGRAM_MAX:
        raise ValueError(f"program is {len(out)} B, the watch takes {PROGRAM_MAX}")
    return bytes(out)


def version(program: bytes) -> int:
    return zlib.crc32(program)


def messages(program: bytes) -> list[bytes]:
    """B, C…, E with seq 0, 1, …"""
    out = [MAGIC + b"B" + bytes([0]) + struct.pack("<H", len(program))]
    for off in range(0, len(program), CHUNK):
        out.append(MAGIC + b"C" + bytes([len(out)]) + struct.pack("<H", off) + program[off:off + CHUNK])
    out.append(MAGIC + b"E" + bytes([len(out)]) + struct.pack("<I", version(program)))
    return out


def set_msg(start: int, day: int, program_version: int, ex: int, t: int) -> bytes:
    """D: a set done on the phone (ex 0xFF: the workout only runs); docs/workout.md."""
    return MAGIC + b"D\x00" + struct.pack("<IBIBH", start, day, program_version, ex, t)


def finish_msg(start: int, end: int) -> bytes:
    """F: the phone finished the workout that started at `start`."""
    return MAGIC + b"F\x00" + struct.pack("<II", start, end)


def entry_msg(e: Entry) -> bytes:
    """H: a workout done on the phone alone."""
    return MAGIC + b"H\x00" + struct.pack("<IHBBB", e.start, e.dur_s, e.day, e.done, e.planned)


def zones_msg(lows: list[int]) -> bytes:
    """Z: the lowest bpm of heart-rate zones 1..5."""
    return MAGIC + b"Z\x00" + bytes(lows)


def decode_entry(msg: bytes) -> Entry | None:
    if len(msg) != 14 or msg[:3] != MAGIC + b"Y" or msg[4] == 0:
        return None
    return Entry(*struct.unpack_from("<IHBBB", msg, 5))


async def _request(ch, wait: float) -> tuple[int, int]:
    """(version, flags) of the app's next R (flags bit 0: a workout is in progress; 7-byte R from older apps = 0);
    sets ch.slot to the app's slot."""
    deadline = time.monotonic() + wait
    while (left := deadline - time.monotonic()) > 0:
        try:
            m = await ch.recv(timeout=left)
        except TimeoutError:
            break
        if m.data[:3] == MAGIC + b"R" and len(m.data) in (7, 8):
            ch.slot = m.slot
            return struct.unpack_from("<I", m.data, 3)[0], m.data[7] if len(m.data) == 8 else 0
    raise TimeoutError(f"the workout app did not ask within {wait:.0f} s: open it on the watch")


async def _expect(ch, kind: bytes, wait: float) -> bytes:
    deadline = time.monotonic() + wait
    while (left := deadline - time.monotonic()) > 0:
        try:
            m = await ch.recv(timeout=left)
        except TimeoutError:
            break
        if m.data[:3] == MAGIC + kind:
            return m.data
    raise TimeoutError(f"no {kind.decode()} from the app within {wait:.0f} s")


async def push(ch, program: bytes, log, wait: float = WAIT, force: bool = False) -> None:
    """Waits for the app's R, sends the program unless it is already there, waits for its A. A new program finishes
    a workout in progress, so that is refused unless `force`."""
    v = version(program)
    have, flags = await _request(ch, wait)
    if have == v:
        log(f"already on the watch (version {v:08x})")
        return
    if flags & RUNNING and not force:
        raise RuntimeError("a workout is running on the watch: finish it first, or --force")
    for m in messages(program):
        await ch.send(m)
    while True:
        a = await _expect(ch, b"A", 30)
        if len(a) == 7 and struct.unpack_from("<I", a, 3)[0] == v:
            log(f"stored on the watch (version {v:08x}, {len(program)} B)")
            return


async def history(ch, wait: float = WAIT) -> list[Entry]:
    """Newest first."""
    await _request(ch, wait)
    await ch.send(MAGIC + b"Q" + bytes([0]))
    got: dict[int, Entry] = {}
    while True:
        y = await _expect(ch, b"Y", 30)
        if len(y) < 5 or y[4] == 0:
            return []
        if e := decode_entry(y):
            got[y[3]] = e
        if len(got) == y[4]:
            return [got[i] for i in sorted(got)]


async def trace(ch, wait: float = WAIT) -> tuple[int, list[tuple[int, int]], bytes]:
    """The last workout's trace: (start, [(exercise, seconds to "Сделал")], bpm per 10 s; 0 = none). `wait` 0: the
    slot is known (ch.slot), no R to wait for (the app asks only on its start screen)."""
    if wait:
        await _request(ch, wait)
    await ch.send(MAGIC + b"T" + bytes([0]))
    parts: dict[int, bytes] = {}
    while True:
        x = await _expect(ch, b"X", 30)
        parts[x[3]], start = x[9:], int.from_bytes(x[5:9], "little")
        if len(parts) == x[4]:
            break
    b = b"".join(parts[i] for i in sorted(parts))
    n = b[0]
    sets = [(b[1 + 3 * i], int.from_bytes(b[2 + 3 * i:4 + 3 * i], "little")) for i in range(n)]
    at = 1 + 3 * n
    return start, sets, b[at + 2:at + 2 + int.from_bytes(b[at:at + 2], "little")]


def main(args) -> int:
    from .cli import cache_key, cached_key
    log = lambda text: print(f"{time.strftime('%H:%M:%S')} {text}", file=sys.stderr, flush=True)  # noqa: E731
    if args.action == "push":
        days = parse_dir(Path(args.dir).expanduser())
        program = encode(days)
        if args.dry_run:
            print(json.dumps({"days": [{"name": d.name, "exercises": [e.__dict__ for e in d.exercises]} for d in days],
                              "bytes": len(program), "version": f"{version(program):08x}"}, ensure_ascii=False))
            return 0
    if not (args.address or args.hci_port):
        raise ValueError("give --address (watch) or --hci-port (emulator), or --dry-run")
    target = f"ble:{args.address}" if args.address else f"hci:{args.hci_port}"
    ch = channel.Channel(address=args.address, hci_port=args.hci_port)
    ch.key = cached_key(target)  # resume: a new bind shows a popup that ends the running app

    async def run():
        log("waiting for the workout app: open it on the watch")
        try:
            if args.action == "push":
                await push(ch, program, log, force=args.force)
            else:
                for e in await history(ch):
                    print(json.dumps(e.__dict__))
        finally:
            if ch.key:
                cache_key(target, ch.key)
            await ch.drop()

    asyncio.run(run())
    return 0
