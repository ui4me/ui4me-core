"""Validate the publication guard and canonical release artifact inventory."""
import importlib.util
import sys
import tempfile
from pathlib import Path
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'tools' / 'release'))
import verify_publish_dist as publishing


class PublishWorkflowTests(unittest.TestCase):
    def test_publishing_never_runs_on_push_or_tag(self):
        workflow=(ROOT/'.github/workflows/publish.yaml').read_text()
        header=workflow.split('\npermissions:',1)[0]
        self.assertIn('workflow_dispatch:',header)
        self.assertNotIn('  push:',header)
        self.assertNotIn('  release:',header)
        self.assertIn("github.ref == 'refs/heads/main'",workflow)
        self.assertIn("'publish-' + version",workflow)
        self.assertIn('environment:\n      name: pypi',workflow)
        self.assertIn('pypa/gh-action-pypi-publish@release/v1',workflow)
        self.assertNotIn('secrets.PYPI_TOKEN',workflow)
        self.assertIn('id-token: write',workflow)
        self.assertIn('needs: [preflight, source, linux-wheel, windows-wheel]',workflow)
        self.assertIn('needs: [preflight, pypi]',workflow)
        self.assertIn('SOURCE_COMMIT.txt',workflow)
        self.assertIn('sha256sum -c SHA256SUMS',workflow)
        for p in ('.github/workflows/release-linux.yml','.github/workflows/release-windows.yml'):
            head=(ROOT/p).read_text().split('permissions:',1)[0]
            self.assertNotIn('  push:',head)
            self.assertIn('  workflow_dispatch:',head)

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
