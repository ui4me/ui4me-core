"""Release manifest generation must be deliberate and exclude build/private material.

The completeness check of the committed ``tools/release/SOURCE_FILES.sha256``
against the whole tree is a release-freeze check, not a unit test: on any
ordinary change the committed manifest is (correctly) out of date, which made
every pull request fail CI (finding F-09 / RELEASE-02). It therefore runs only
when explicitly requested with ``UI4ME_RELEASE_MANIFEST_CHECK=1``: by the publish
workflows, by the ``release-manifest`` CI workflow when ``tools/release/**``
changes, and by ``tools/verify.py --stage release-manifest``.
"""
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

from release.refresh_source_manifest import included_source_files

ROOT = Path(__file__).resolve().parents[1]
RELEASE_CHECK_ENV = 'UI4ME_RELEASE_MANIFEST_CHECK'


def release_manifest_check_requested() -> bool:
    return os.environ.get(RELEASE_CHECK_ENV) == '1'


class SourceManifestTests(unittest.TestCase):
    @unittest.skipUnless(release_manifest_check_requested(),
                         f'release-freeze check: set {RELEASE_CHECK_ENV}=1 (publish workflows run it)')
    def test_checked_manifest_matches_entire_source(self):
        result = subprocess.run([sys.executable, 'tools/release/refresh_source_manifest.py', '--check'],
                                cwd=ROOT, capture_output=True, text=True)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)

    def test_no_generated_private_or_build_material(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / 'safe.py').write_text('pass')
            (root / 'target').mkdir()
            (root / 'target' / 'binary').write_bytes(b'ignored')
            (root / '.git').mkdir()
            (root / '.git' / 'config').write_text('not included')
            (root / 'project.local.json').write_text('private')
            (root / 'tools' / 'release').mkdir(parents=True)
            (root / 'tools' / 'release' / 'SOURCE_FILES.sha256').write_text('recursive')
            included = [p.relative_to(root).as_posix() for p in included_source_files(root)]
            self.assertEqual(included, ['safe.py'])

    def test_git_excluded_local_files_are_not_manifested(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            subprocess.run(['git', 'init', '-q', str(root)], check=True)
            (root / '.git' / 'info').mkdir(exist_ok=True)
            (root / '.git' / 'info' / 'exclude').write_text('AGENTS.md\nLOCAL_NOTES.md\n')
            (root / 'safe.py').write_text('pass')
            (root / 'AGENTS.md').write_text('local maintainer instructions')
            (root / 'LOCAL_NOTES.md').write_text('lab credentials location')
            included = [p.relative_to(root).as_posix() for p in included_source_files(root)]
            self.assertEqual(included, ['safe.py'])

    def test_order_is_case_sensitive_on_every_platform(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            for name in ('b.txt', 'C.txt', 'a.txt', 'D.txt'):
                (root / name).write_text(name)
            included = [p.name for p in included_source_files(root)]
            self.assertEqual(included, ['C.txt', 'D.txt', 'a.txt', 'b.txt'])


if __name__ == '__main__':
    unittest.main()
