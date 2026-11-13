import unittest

from tests import ios_vectors


class IosVectorsTest(unittest.TestCase):
    def test_committed_vectors_are_current(self):
        self.assertEqual(ios_vectors.VECTORS.read_text(encoding="utf-8"), ios_vectors.text(),
                         "regenerate: uv run --locked python -m tests.ios_vectors > " + str(ios_vectors.VECTORS))
