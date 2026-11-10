from pathlib import Path
import shutil
import struct
import subprocess
import unittest

from wp2apps import package

ROOT = Path(__file__).resolve().parents[1]


@unittest.skipUnless(shutil.which("cargo") and shutil.which("llvm-objcopy"), "needs cargo and llvm-objcopy")
class SdkTest(unittest.TestCase):
    def test_hello_builds_and_packs(self):
        subprocess.run([str(ROOT / "scripts" / "build-app"), "hello", "1"], check=True, capture_output=True)
        pkg = package.parse((ROOT / "target" / "hello.wp2app").read_bytes())
        self.assertEqual((pkg.name, pkg.entry, pkg.load_addr), ("hello", 0, package.APP_BASE))
        self.assertTrue(0 < len(pkg.image) < 16 * 1024)
        elf = ROOT / "sdk/target/thumbv7em-none-eabi/release/hello"
        self.assertEqual(len(pkg.image), alloc_end(elf.read_bytes()) - package.APP_BASE)


def alloc_end(elf):
    shoff, = struct.unpack_from("<I", elf, 0x20)
    shentsize, shnum = struct.unpack_from("<HH", elf, 0x2E)
    end = 0
    for i in range(shnum):
        _, _, flags, addr, _, size = struct.unpack_from("<IIIIII", elf, shoff + i * shentsize)
        if flags & 2:
            end = max(end, addr + size)
    return end


if __name__ == "__main__":
    unittest.main()
