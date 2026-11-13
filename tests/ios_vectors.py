"""Test vectors for the iPhone app's WireKit (ios/WireKit/Tests/WireKitTests/vectors.json), made by the Python host
so both sides encode byte for byte the same. Regenerate: uv run --locked python -m tests.ios_vectors > <that file>.
tests/test_ios_vectors.py fails when the committed file is stale."""
import json
from pathlib import Path
import struct
import zlib

from wp2apps import install, link, package, workout

FIXTURE = Path(__file__).parent / "fixtures" / "workout"
VECTORS = Path(__file__).parents[1] / "ios" / "WireKit" / "Tests" / "WireKitTests" / "vectors.json"


def day_json(d: workout.Day) -> dict:
    return {"name": d.name, "exercises": [{"name": e.name, "sets": e.sets, "repsMin": e.reps[0], "repsMax": e.reps[1],
                                           "weight": e.weight, "restS": e.rest_s} for e in d.exercises]}


def vectors() -> dict:
    days = workout.parse_dir(FIXTURE) + [workout.Day("Ёлка и очень длинное имя дня!", (
        workout.Exercise("Жим ногами в тренажёре стоя (33 Б)", 2, (8, 8), "Ёмкость 50–60 кг", 75),))]
    program = workout.encode(days)
    key = bytes([7] * 16)
    secret = "00112233445566778899aabbccddeeff"
    entry = workout.Entry(start=0x2F00_1234, dur_s=3725, day=1, done=17, planned=19)
    y = workout.MAGIC + b"Y" + bytes([2, 5]) + struct.pack("<IHBBB", entry.start, entry.dur_s, entry.day, entry.done,
                                                          entry.planned)
    dry = {"days": [{"name": d.name, "exercises": [e.__dict__ for e in d.exercises]} for d in days],
           "bytes": len(program), "version": f"{workout.version(program):08x}"}
    return {
        "days": [day_json(d) for d in days],
        "program_hex": program.hex(),
        "version": workout.version(program),
        "messages_hex": [m.hex() for m in workout.messages(program)],
        "query_hex": (workout.MAGIC + b"Q" + bytes([0])).hex(),
        "seal": {"key": key.hex(), "plain": b"hello".hex(), "cipher": link.encrypt(key, b"hello").hex()},
        "channel_key": {"secret": secret, "key": link.channel_key(secret).hex()},
        "frame": {"c2": link.STUB_INFO, "payload": "ff", "key": key.hex(),
                  "hex": link.frame(link.SYSTEM, link.STUB_INFO, b"\xff", key).hex()},
        "entry": {"hex": y.hex(), "index": 2, "count": 5, "start": entry.start, "durS": entry.dur_s, "day": entry.day,
                  "done": entry.done, "planned": entry.planned},
        "empty_history_hex": (workout.MAGIC + b"Y" + bytes([0, 0])).hex(),
        "requests": [{"hex": (workout.MAGIC + b"R" + struct.pack("<I", 0x01020304) + tail).hex(), "version": 0x01020304,
                      "running": tail == b"\x01"} for tail in (b"", b"\x00", b"\x01")],
        "ack_hex": (workout.MAGIC + b"A" + struct.pack("<I", 0x0A0B0C0D)).hex(),
        "dry_run_json": json.dumps(dry, ensure_ascii=False),
        "package": package_vectors(),
    }


def package_vectors() -> dict:
    """A tiny app package and the install requests for it in slot 3 (install.upload)."""
    data = package.pack(bytes(range(256)) * 2, name="workout", version=3)
    return {"hex": data.hex(), "name": "workout", "version": 3, "kind": package.KIND_APP,
            "info_reply_hex": (bytes([0]) + data[:package.HEADER_SIZE]).hex(),
            "begin_hex": struct.pack("<BII", 3, len(data), zlib.crc32(data)).hex(),
            "data_hex": [(struct.pack("<I", off) + part).hex() for off, part in install.chunks(data)],
            "end_hex": "00"}


def text() -> str:
    return json.dumps(vectors(), ensure_ascii=False, indent=1) + "\n"


if __name__ == "__main__":
    print(text(), end="")
