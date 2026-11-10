import os
from pathlib import Path
import subprocess
import tempfile
import unittest

from tests.e2e import harness


class FastQemuTest(unittest.TestCase):
    def test_wrapper_adds_sleep_off_and_keeps_other_args(self):
        with tempfile.TemporaryDirectory() as tmp:
            fake = Path(tmp) / "fake qemu"
            fake.write_text('#!/bin/sh\nfor a; do echo "[$a]"; done\n')
            fake.chmod(0o755)
            wrapper = harness.fast_qemu(Path(tmp), qemu=fake)
            self.assertTrue(os.access(wrapper, os.X_OK))
            out = subprocess.run([wrapper, "-M", "x y", "-icount", "shift=3,align=off", "-S"],
                                 capture_output=True, text=True, check=True).stdout
        self.assertEqual(out.split("\n")[:-1], ["[-M]", "[x y]", "[-icount]", "[shift=3,align=off,sleep=off]", "[-S]"])


class GuestClockTest(unittest.TestCase):
    def test_parses_xp_output_in_eighths(self):
        self.assertEqual(harness.guest_seconds("0000000040004118: 0xfc000029\r\n"), 41 / 8)


if __name__ == "__main__":
    unittest.main()
