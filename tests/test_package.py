import contextlib
import io
import json
import struct
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

    def test_header_bit_flip_rejected(self):
        data = bytearray(package.pack(b"\0" * 64, name="x", version=1))
        data[0x28] ^= 0x01
        with self.assertRaises(ValueError):
            package.parse(bytes(data))

    def test_nonzero_reserved_rejected(self):
        data = bytearray(package.pack(b"\0" * 64, name="x", version=1))
        data[0x3F] = 1
        with self.assertRaises(ValueError):
            package.parse(bytes(data))

    def test_abi_minor_round_trip(self):
        pkg = package.parse(package.pack(b"\0" * 4, name="x", version=1))
        self.assertEqual(pkg.abi_minor, package.ABI_MINOR)
        self.assertEqual(package.HEADER.unpack_from(package.pack(b"\0" * 4, name="x", version=1))[3],
                         package.ABI_MINOR)

    def test_elf_matching_image_accepted(self):
        package.check_elf(b"\0" * 0x30, fake_elf(package.APP_BASE | 1, [(package.APP_BASE, 0x30)]))

    def test_elf_linked_elsewhere_rejected(self):
        with self.assertRaises(ValueError):
            package.check_elf(b"\0" * 0x30, fake_elf(0x10000001, [(0x10000000, 0x30)]))

    def test_elf_image_missing_bss_rejected(self):
        elf = fake_elf(package.APP_BASE | 1, [(package.APP_BASE, 0x30), (package.APP_BASE + 0x30, 0x8)])
        with self.assertRaises(ValueError):
            package.check_elf(b"\0" * 0x30, elf)

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


def fake_elf(entry, sections):
    header = bytearray(0x34)
    header[:4] = b"\x7fELF"
    struct.pack_into("<I", header, 0x18, entry)
    struct.pack_into("<I", header, 0x20, 0x34)
    struct.pack_into("<HH", header, 0x2E, 40, len(sections) + 1)
    table = bytes(40) + b"".join(struct.pack("<IIIIII", 1, 1, 2, addr, 0, size) + bytes(16)
                                 for addr, size in sections)
    return bytes(header) + table


if __name__ == "__main__":
    unittest.main()
