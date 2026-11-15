"""Validate the canonical release artifact inventory.

The publication guard itself (manual dispatch only, main only, trusted
publishing, job ordering, CI gate) is asserted structurally on the parsed
workflows in test_workflows.py (RELEASE-15); the former substring checks lived here.
"""
import sys
import tempfile
from pathlib import Path
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'tools' / 'release'))
import verify_publish_dist as publishing


class PublishWorkflowTests(unittest.TestCase):
    def test_exact_release_triple_and_hash_list(self):
        with tempfile.TemporaryDirectory() as tmp:
            directory=Path(tmp)
            for name in publishing.EXPECTED:
                (directory/name).write_bytes(name.encode())
            mocks=(patch.object(publishing,'validate_sdist',return_value={'result':'PASS'}),
                   patch.object(publishing,'validate_linux',return_value={'result':'PASS'}),
                   patch.object(publishing,'validate_windows',return_value={'result':'PASS'}))
            with mocks[0], mocks[1], mocks[2]:
                rows=publishing.validate(directory)
                self.assertEqual([name for name,_ in rows],list(publishing.EXPECTED))
                (directory/'unexpected.whl').write_bytes(b'bad')
                with self.assertRaises(ValueError):
                    publishing.validate(directory)
                (directory/'unexpected.whl').unlink()
                (directory/publishing.EXPECTED[0]).unlink()
                with self.assertRaises(ValueError):
                    publishing.validate(directory)


if __name__ == '__main__':
    unittest.main()
