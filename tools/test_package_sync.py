"""The packaged `ui4me_core` client must not drift from the example copies.

Applications copy `examples/python/{client,application,live_bitmap}.py`;
the wheel ships `python/ui4me_core/`. Apart from package-relative imports
the sources must be identical, so both expose the same window options.
"""
from pathlib import Path
import re
import unittest

ROOT = Path(__file__).resolve().parents[1]


def normalized(path: Path) -> str:
    text = path.read_text(encoding="utf-8")
    return re.sub(r"^(\s*)from \.(\w+) import", r"\1from \2 import", text, flags=re.M)


class PackageSync(unittest.TestCase):
    def test_packaged_modules_match_example_copies(self):
        for name in ("client.py", "application.py", "live_bitmap.py"):
            with self.subTest(module=name):
                self.assertEqual(normalized(ROOT / "python" / "ui4me_core" / name),
                                 normalized(ROOT / "examples" / "python" / name))


if __name__ == "__main__":
    unittest.main()
