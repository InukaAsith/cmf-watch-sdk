import io
from pathlib import Path
import struct
import subprocess
import tempfile
import unittest
import zlib

from wp2apps import install, package


class InstallChunksTest(unittest.TestCase):
    def test_chunks_cover_file(self):
        data = bytes(range(256)) * 3
        parts = list(install.chunks(data))
        self.assertEqual(b"".join(p for _, p in parts), data)
        self.assertTrue(all(len(p) <= 128 for _, p in parts))
        self.assertEqual([o for o, _ in parts], list(range(0, len(data), 128)))


class FakeSession:
    def __init__(self, slots=()):
        self.calls, self.slots = [], dict(slots)

    async def request(self, c1, c2, payload, expect, timeout=6.0):
        self.calls.append((c2, payload))
        if c2 == install.INFO:
            return b"\0" + self.slots.get(payload[0], b"")
        if c2 == install.DATA:
            return b"\0" + struct.pack("<I", struct.unpack_from("<I", payload)[0] + len(payload) - 4)
        return b"\0"


def sample(name="hello", kind=package.KIND_APP):
    return package.pack(bytes(64), name=name, version=3, kind=kind)


class InstallFlowTest(unittest.IsolatedAsyncioTestCase):
    async def test_first_free_slot(self):
        s = FakeSession({0: sample("other")[:64]})
        self.assertEqual(await install.install(s, sample()), 1)
        begin = [p for c, p in s.calls if c == install.BEGIN][0]
        self.assertEqual(struct.unpack("<BII", begin), (1, len(sample()), zlib.crc32(sample())))
        self.assertEqual(s.calls[-1][0], install.END)

    async def test_same_name_replaced(self):
        s = FakeSession({0: sample("other")[:64], 3: sample()[:64]})
        self.assertEqual(await install.install(s, sample()), 3)

    async def test_loader_goes_to_loader_slot(self):
        self.assertEqual(await install.install(FakeSession(), sample("l", package.KIND_LOADER)), 0xFF)

    async def test_kind_and_slot_checked_before_begin(self):
        for data, slot in ((sample("l", package.KIND_LOADER), 2), (sample(), install.LOADER_SLOT)):
            s = FakeSession()
            with self.assertRaisesRegex(ValueError, "slot L"):
                await install.install(s, data, slot)
            self.assertEqual(s.calls, [])

    async def test_status_error(self):
        class Busy(FakeSession):
            async def request(self, *a, **k):
                return bytes([install.Status.BUSY])
        with self.assertRaisesRegex(ValueError, "BUSY"):
            await install.remove(Busy(), 0)

    async def test_info_empty_and_full(self):
        s = FakeSession({2: sample()[:64]})
        self.assertIsNone(await install.info(s, 0))
        self.assertEqual(await install.info(s, 2), install.Header("hello", 3, 0, 0))


class CliTest(unittest.TestCase):
    def test_slot_arg(self):
        from wp2apps import cli
        self.assertEqual(cli.slot_arg("L"), 0xFF)
        self.assertEqual(cli.slot_arg("7"), 7)
        with self.assertRaises(Exception):
            cli.slot_arg("8")

    def test_stock_app_failure_is_error_with_stderr(self):
        from unittest import mock
        from wp2apps import cli, paths
        failed = subprocess.CalledProcessError(2, "fwkit", stderr="no such file\n")
        with mock.patch.object(paths, "firmware", return_value=Path("x")), \
                mock.patch.object(paths.subprocess, "run", side_effect=failed), \
                tempfile.TemporaryDirectory() as tmp, mock.patch("sys.stderr", new=io.StringIO()) as err:
            self.assertEqual(cli.main(["firmware", "--output", f"{tmp}/out.bin"]), 1)
        self.assertIn("no such file", err.getvalue())

    def test_connection_refused_is_error_not_traceback(self):
        from wp2apps import cli
        self.assertEqual(cli.main(["list", "--hci-port", "1"]), 1)
