"""Single-source version tooling (F-21 / RELEASE-07).

* every format-forced location agrees with ``Cargo.toml`` (``version.py``);
* ``bump_version.py`` rewrites all of them and nothing else;
* the release machinery (workflows and ``tools/release``) carries no literal
  version, so a release does not edit CI;
* the inventory of all remaining literals is complete (it is the lead's work
  list for the mass change after integration).

``UI4ME_STRICT_VERSION_SOURCE=1`` additionally enforces the locations that are
known to disagree until the first bump (``version.PENDING_FIRST_BUMP``).
"""
from __future__ import annotations

import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'tools' / 'release'))
import bump_version  # noqa: E402
import version as versioning  # noqa: E402

STRICT = os.environ.get('UI4ME_STRICT_VERSION_SOURCE') == '1'


def git_available() -> bool:
    try:
        return subprocess.run(['git', '-C', str(ROOT), 'rev-parse', '--git-dir'],
                              capture_output=True).returncode == 0
    except OSError:
        return False


class VersionSourceTests(unittest.TestCase):
    def test_source_version_is_semver_and_forced_locations_agree(self):
        version = versioning.source_version()
        self.assertRegex(version, versioning.SEMVER)
        self.assertEqual(versioning.check_consistency(strict=STRICT), {})

    def test_artifact_names_derive_from_one_version(self):
        names = versioning.artifact_names('9.8.7')
        self.assertEqual(names['sdist'], 'ui4me_core-9.8.7.tar.gz')
        self.assertEqual(names['windows_wheel'], 'ui4me_core-9.8.7-py3-none-win_amd64.whl')
        self.assertEqual(names['linux_wheel'], 'ui4me_core-9.8.7-py3-none-manylinux_2_34_x86_64.whl')
        self.assertEqual(names['tag'], 'v9.8.7')

    def copy_forced_tree(self) -> Path:
        folder = tempfile.TemporaryDirectory()
        self.addCleanup(folder.cleanup)
        root = Path(folder.name)
        for relative in list(versioning.FORCED) + ['Cargo.lock']:
            (root / relative).parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(ROOT / relative, root / relative)
        return root

    def test_bump_rewrites_every_forced_location_consistently(self):
        root = self.copy_forced_tree()
        before = {p: (root / p).read_text(encoding='utf-8') for p in list(versioning.FORCED) + ['Cargo.lock']}
        changed = bump_version.bump('9.8.7', root=root)
        self.assertEqual(versioning.source_version(root), '9.8.7')
        self.assertEqual(versioning.check_consistency(root, strict=True), {})
        self.assertEqual(set(changed), set(versioning.FORCED) | {'Cargo.lock'})
        old = versioning.source_version()
        for relative, text in before.items():
            after = (root / relative).read_text(encoding='utf-8')
            # Only the version declaration lines change.
            diff = [(a, b) for a, b in zip(text.splitlines(), after.splitlines()) if a != b]
            self.assertTrue(diff, relative)
            for a, b in diff:
                self.assertEqual(a.replace(old, '9.8.7').replace(versioning.render(old, 'win4'), '9.8.7.0')
                                 if relative != 'crates/ui4me-host/host.manifest' else
                                 re.sub(r'version="[^"]+"', 'version="9.8.7.0"', a, count=1), b, relative)
            self.assertEqual(len(text.splitlines()), len(after.splitlines()), relative)

    def test_bump_dry_run_changes_nothing_and_rejects_bad_versions(self):
        root = self.copy_forced_tree()
        snapshot = {p: (root / p).read_bytes() for p in list(versioning.FORCED) + ['Cargo.lock']}
        self.assertTrue(bump_version.bump('9.8.7', root=root, dry_run=True))
        self.assertEqual(snapshot, {p: (root / p).read_bytes() for p in snapshot})
        for bad in ('1.0', '01.0.0', '1.0.2-rc.1', 'v1.0.2', ''):
            with self.assertRaises(ValueError, msg=bad):
                bump_version.planned_changes(bad, root)

    @unittest.skipUnless(git_available(), 'needs a Git checkout')
    def test_release_machinery_has_no_literal_version(self):
        rows = [r for r in versioning.literal_occurrences() if r['category'] == 'release-machinery'
                and r['path'].startswith('tools/release/')]
        self.assertEqual(rows, [], 'Workflows and tools/release must read tools/release/version.py; '
                                   'found literals:\n' + '\n'.join(f"{r['path']}:{r['line']}" for r in rows))

    @unittest.skipUnless(git_available(), 'needs a Git checkout')
    def test_inventory_lists_every_tracked_literal(self):
        version = versioning.source_version()
        rows = versioning.literal_occurrences(version)
        listed = {(r['path'], r['line']) for r in rows}
        for relative in versioning.FORCED:
            if relative not in versioning.PENDING_FIRST_BUMP or STRICT:
                self.assertIn(relative, {p for p, _ in listed}, relative)
        # Completeness against an independent scan of the tracked files.
        tracked = subprocess.run(['git', '-C', str(ROOT), 'ls-files', '-z'], capture_output=True,
                                 check=True).stdout.decode('utf-8').split('\0')
        word = re.compile(r'(?<![\w.])' + re.escape(version) + r'(?![\w]|\.\d)')
        expected = set()
        for relative in filter(None, tracked):
            if relative.startswith(versioning.EXCLUDED_PREFIXES):
                continue
            path = ROOT / relative
            try:
                data = path.read_bytes()
            except OSError:
                continue
            if b'\0' in data[:8000]:
                continue
            for number, line in enumerate(data.decode('utf-8', 'replace').splitlines(), 1):
                if word.search(line):
                    expected.add((relative, number))
        self.assertEqual(listed, expected)
        self.assertTrue(all(r['category'] in {'forced', 'code', 'historical', 'release-machinery',
                                              'test', 'example', 'docs'} for r in rows))


if __name__ == '__main__':
    unittest.main()
