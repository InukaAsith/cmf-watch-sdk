import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

from wp2apps import paths


class PathsTest(unittest.TestCase):
    def test_env_override(self):
        with tempfile.TemporaryDirectory() as d, patch.dict(os.environ, {"CMF_WATCH_UTILS": d}):
            self.assertEqual(paths.cmf_watch_utils(), Path(d).resolve())

    def test_missing_checkout(self):
        with patch.dict(os.environ, {"CMF_WATCH_UTILS": "/nonexistent/cmf"}):
            with self.assertRaises(FileNotFoundError):
                paths.cmf_watch_utils()

    def test_lfs_pointer_rejected(self):
        with tempfile.TemporaryDirectory() as d, patch.dict(os.environ, {"CMF_WATCH_UTILS": d}):
            fw = Path(d) / "dumps/binaries/firmware/JX402_V1.0.0.73.bin"
            fw.parent.mkdir(parents=True)
            fw.write_bytes(b"version https://git-lfs.github.com/spec/v1\n")
            with self.assertRaises(FileNotFoundError):
                paths.firmware()


if __name__ == "__main__":
    unittest.main()
