import asyncio
import os
from pathlib import Path
import struct
import tempfile
import unittest
import zlib

from wp2apps import channel, workout
from wp2apps.workout import Day, Entry, Exercise, ParseError

FIXTURES = Path(__file__).parent / "fixtures" / "workout"
PROGRAM_BIN = Path(__file__).parent / "fixtures" / "workout-program.bin"


def ex(line: str) -> Exercise:
    return workout.parse_day(f"## D\n{line}\n", "D.md").exercises[0]


class ParseTest(unittest.TestCase):
    def test_lines(self):
        self.assertEqual(ex("- [x] Жим лёжа: 4×5–8 (90 кг)"), Exercise("Жим лёжа", 4, (5, 8), "90 кг", 90))
        self.assertEqual(ex("- [ ] Скручивания на полу 3х20"), Exercise("Скручивания на полу", 3, (20, 20), "", 90))
        self.assertEqual(ex("- Тяга: 3x8-10 (50–60 кг) отдых 2:00"), Exercise("Тяга", 3, (8, 10), "50–60 кг", 120))
        self.assertEqual(ex("- Подтягивания: 4×6–10 (свой вес)").weight, "свой вес")
        self.assertEqual(ex("* Жим 2 руками: 3 × 10").name, "Жим 2 руками")

    def test_no_sets_is_an_error(self):
        with self.assertRaises(ParseError) as caught:
            workout.parse_day("## День 2: Тяга\n\n- [x] Сгибание на бицепс \n", "D.md")
        self.assertTrue(str(caught.exception).startswith("D.md:3:"), str(caught.exception))

    def test_day_name_and_other_lines(self):
        day = workout.parse_day("## День 1: Жим и присед\n\n- [x] Жим: 4×5\n\n&nbsp;\nтекст\n", "D.md")
        self.assertEqual(day, Day("Жим и присед", (Exercise("Жим", 4, (5, 5), "", 90),)))
        self.assertEqual(workout.parse_day("## Ноги\n- Присед: 5×5\n", "D.md").name, "Ноги")
        with self.assertRaises(ParseError):
            workout.parse_day("## Пусто\n\nничего\n", "D.md")

    def test_parse_dir(self):
        days = workout.parse_dir(FIXTURES)
        self.assertEqual([d.name for d in days], ["Верх", "Ноги"])
        self.assertEqual(days[0].exercises[0].rest_s, 120)
        with tempfile.TemporaryDirectory() as tmp:
            (Path(tmp) / "notes.md").write_text("без заголовка\n")
            (Path(tmp) / "b.md").write_text("## B\n- x: 1×1\n")
            (Path(tmp) / "a.md").write_text("## A\n- y: 1×1\n")
            self.assertEqual([d.name for d in workout.parse_dir(Path(tmp))], ["A", "B"])


class EncodeTest(unittest.TestCase):
    def test_layout(self):
        p = workout.encode([Day("Д", (Exercise("Жим", 4, (5, 8), "90 кг", 120),))])
        name, weight = "Жим".encode(), "90 кг".encode()
        want = bytes([1, 2]) + "Д".encode() + bytes([1, len(name)]) + name + bytes([4, 5, 8]) + struct.pack("<H", 120) \
            + bytes([len(weight)]) + weight
        self.assertEqual(p, want)

    def test_yo_and_clip(self):
        long = "Очень длинное название упражнения"
        p = workout.encode([Day("Ёлка", (Exercise("Жим лёжа", 1, (1, 1), "", 90), Exercise(long, 1, (1, 1), "", 90)))])
        self.assertIn("Елка".encode(), p)
        self.assertIn("Жим лежа".encode(), p)
        self.assertNotIn("ё".encode(), p)
        clipped = long.encode()[:workout.NAME_MAX].decode("utf-8", "ignore").encode()
        self.assertIn(bytes([len(clipped)]) + clipped + bytes([1]), p)
        self.assertLessEqual(len(clipped), workout.NAME_MAX)

    def test_weight_fits_32_bytes(self):
        w = "гантели по 8–10 кг"  # 31 B
        p = workout.encode([Day("d", (Exercise("x", 1, (1, 1), w, 90),))])
        self.assertTrue(p.endswith(bytes([len(w.encode())]) + w.encode()))
        self.assertEqual(workout.WEIGHT_MAX, 32)

    def test_limits(self):
        one = (Exercise("x", 1, (1, 1), "", 90),)
        with self.assertRaises(ValueError):
            workout.encode([Day("d", one)] * 8)
        with self.assertRaises(ValueError):
            workout.encode([Day("d", one * 13)])
        with self.assertRaises(ValueError):
            workout.encode([Day("d", (Exercise("x", 1, (9, 5), "", 90),))])
        with self.assertRaises(ValueError):
            workout.encode([Day("x" * 32, (Exercise("я" * 16, 9, (9, 9), "я" * 8, 90),) * 12)] * 3)  # > 2048 B
        with self.assertRaises(ValueError):
            workout.encode([])

    def test_messages(self):
        program = bytes(range(256)) + bytes(44)
        msgs = workout.messages(program)
        self.assertEqual([m[:3] for m in msgs], [b"w1B", b"w1C", b"w1C", b"w1C", b"w1E"])
        self.assertEqual([m[3] for m in msgs], [0, 1, 2, 3, 4])
        self.assertEqual(msgs[0][4:], struct.pack("<H", 300))
        self.assertEqual([struct.unpack_from("<H", m, 4)[0] for m in msgs[1:4]], [0, 120, 240])
        self.assertEqual([len(m) - 6 for m in msgs[1:4]], [120, 120, 60])
        self.assertEqual(b"".join(m[6:] for m in msgs[1:4]), program)
        self.assertEqual(msgs[4][4:], struct.pack("<I", zlib.crc32(program)))
        self.assertTrue(all(len(m) <= channel.MSG_MAX for m in msgs))
        self.assertEqual(workout.version(program), zlib.crc32(program))

    def test_fixture(self):
        program = workout.encode(workout.parse_dir(FIXTURES))
        if os.environ.get("WP2_REGEN"):
            PROGRAM_BIN.write_bytes(program)
        self.assertEqual(program, PROGRAM_BIN.read_bytes())

    def test_decode_entry(self):
        msg = b"w1Y\x01\x03" + struct.pack("<IHBBB", 847879200, 3600, 2, 7, 9)
        self.assertEqual(workout.decode_entry(msg), Entry(847879200, 3600, 2, 7, 9))
        self.assertIsNone(workout.decode_entry(b"w1Y\x00\x00"))
        self.assertIsNone(workout.decode_entry(b"h1Y" + bytes(11)))


class FakeChannel:
    def __init__(self):
        self.sent, self.slot, self.key, self.poll = [], channel.ANY, b"k", 0.25
        self.inbox = asyncio.Queue()

    async def send(self, data, timeout=10):
        self.sent.append(data)

    async def recv(self, timeout=None):
        async with asyncio.timeout(timeout):
            return await self.inbox.get()

    def app(self, data: bytes):
        self.inbox.put_nowait(channel.Message(seq=0, slot=2, data=data))


class FlowTest(unittest.IsolatedAsyncioTestCase):
    async def test_push(self):
        program = PROGRAM_BIN.read_bytes()
        v = struct.pack("<I", zlib.crc32(program))
        ch = FakeChannel()
        task = asyncio.create_task(workout.push(ch, program, log=lambda *_: None))
        ch.app(b"h1R")  # another app's request: ignored
        ch.app(b"w1R" + bytes(4))
        while len(ch.sent) < len(workout.messages(program)):
            await asyncio.sleep(0.01)
        self.assertEqual(ch.sent, workout.messages(program))
        self.assertEqual(ch.slot, 2)
        ch.app(b"w1A" + v)
        await asyncio.wait_for(task, 2)

    async def test_push_same_version(self):
        program = PROGRAM_BIN.read_bytes()
        ch = FakeChannel()
        ch.app(b"w1R" + struct.pack("<I", zlib.crc32(program)))
        await asyncio.wait_for(workout.push(ch, program, log=lambda *_: None), 2)
        self.assertEqual(ch.sent, [])

    async def test_push_refuses_running(self):
        ch = FakeChannel()
        ch.app(b"w1R" + bytes(4) + b"\x01")  # a workout runs on the watch
        with self.assertRaisesRegex(RuntimeError, "finish it first"):
            await asyncio.wait_for(workout.push(ch, PROGRAM_BIN.read_bytes(), log=lambda *_: None), 2)
        self.assertEqual(ch.sent, [])

    async def test_push_force(self):
        program = PROGRAM_BIN.read_bytes()
        ch = FakeChannel()
        ch.app(b"w1R" + bytes(4) + b"\x01")
        task = asyncio.create_task(workout.push(ch, program, log=lambda *_: None, force=True))
        while len(ch.sent) < len(workout.messages(program)):
            await asyncio.sleep(0.01)
        ch.app(b"w1A" + struct.pack("<I", zlib.crc32(program)))
        await asyncio.wait_for(task, 2)

    async def test_request_accepts_7_and_8_bytes(self):
        program = PROGRAM_BIN.read_bytes()
        for tail in (b"", b"\x00"):
            ch = FakeChannel()
            ch.app(b"w1R" + struct.pack("<I", zlib.crc32(program)) + tail)
            await asyncio.wait_for(workout.push(ch, program, log=lambda *_: None), 2)
            self.assertEqual(ch.sent, [])

    async def test_push_gives_up(self):
        ch = FakeChannel()
        with self.assertRaises(TimeoutError):
            await workout.push(ch, PROGRAM_BIN.read_bytes(), log=lambda *_: None, wait=0.2)

    async def test_history(self):
        ch = FakeChannel()
        task = asyncio.create_task(workout.history(ch))
        ch.app(b"w1R" + bytes(4))
        while not ch.sent:
            await asyncio.sleep(0.01)
        self.assertEqual(ch.sent, [b"w1Q\x00"])
        ch.app(b"w1Y\x00\x02" + struct.pack("<IHBBB", 2000, 60, 1, 3, 9))
        ch.app(b"w1Y\x01\x02" + struct.pack("<IHBBB", 1000, 60, 0, 4, 9))
        self.assertEqual(await asyncio.wait_for(task, 2), [Entry(2000, 60, 1, 3, 9), Entry(1000, 60, 0, 4, 9)])

    async def test_history_empty(self):
        ch = FakeChannel()
        task = asyncio.create_task(workout.history(ch))
        ch.app(b"w1R" + bytes(4))
        ch.app(b"w1Y\x00\x00" + bytes(9))
        self.assertEqual(await asyncio.wait_for(task, 2), [])


if __name__ == "__main__":
    unittest.main()
