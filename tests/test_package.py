import contextlib
import io
import json
from pathlib import Path
import tempfile
import unittest

from wp2apps import cli, package


class PackageTest(unittest.TestCase):
    def test_round_trip(self):
        image = bytes(range(256)) * 4
        data = package.pack(image, name="Vibration Lab", version=3)
        self.assertEqual(len(data), package.HEADER_SIZE + len(image))
        pkg = package.parse(data)
        self.assertEqual((pkg.name, pkg.version, pkg.kind, pkg.abi, pkg.entry, pkg.load_addr, pkg.image),
                         ("Vibration Lab", 3, package.KIND_APP, 1, 0, package.APP_BASE, image))

    def test_recon_values(self):
        self.assertEqual((package.APP_BASE, package.APP_SIZE), (0x386FF000, 0x20000))

    def test_utf8_name_fits(self):
        self.assertEqual(package.parse(package.pack(b"\0" * 4, name="Вибро", version=1)).name, "Вибро")

    def test_bad_names_rejected(self):
        for name in ("Я" * 12, ""):
            with self.assertRaises(ValueError):
                package.pack(b"\0" * 4, name=name, version=1)

    def test_corrupt_rejected(self):
        data = package.pack(b"\1" * 64, name="x", version=1)
        flipped = data[:-1] + bytes([data[-1] ^ 1])
        for broken in (b"XXXX" + data[4:], data[:-1], flipped, data[:package.HEADER_SIZE - 1]):
            with self.assertRaises(ValueError):
                package.parse(broken)

    def test_entry_size_and_address_bounds(self):
        with self.assertRaises(ValueError):
            package.pack(b"\0" * 16, name="x", version=1, entry=16)
        with self.assertRaises(ValueError):
            package.pack(b"\0" * (package.APP_SIZE + 1), name="x", version=1)
        with self.assertRaises(ValueError):
            package.pack(b"\0" * 16, name="x", version=1, load_addr=package.APP_BASE + 4)

    def test_cli_pack_info(self):
        with tempfile.TemporaryDirectory() as d:
            image, out = Path(d) / "hello.bin", Path(d) / "hello.wp2app"
            image.write_bytes(b"\x70\x47" * 20)
            self.assertEqual(cli.main(["pack", str(image), "--name", "hello", "--version", "2", "--output", str(out)]), 0)
            stdout = io.StringIO()
            with contextlib.redirect_stdout(stdout):
                self.assertEqual(cli.main(["info", str(out)]), 0)
            info = json.loads(stdout.getvalue())
            self.assertEqual((info["name"], info["version"], info["image_size"]), ("hello", 2, 40))

    def test_cli_error_exit(self):
        with tempfile.TemporaryDirectory() as d:
            bad = Path(d) / "bad.wp2app"
            bad.write_bytes(b"nope")
            with contextlib.redirect_stderr(io.StringIO()) as stderr:
                self.assertEqual(cli.main(["info", str(bad)]), 1)
            self.assertIn("Error:", stderr.getvalue())


if __name__ == "__main__":
    unittest.main()
