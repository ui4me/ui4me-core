"""Release metadata and policy documents (F-24 / RELEASE-11, F-20 / RELEASE-05, roadmap 7.1)."""
from __future__ import annotations

import io
from contextlib import redirect_stderr, redirect_stdout
from pathlib import Path
import re
import sys
import tomllib
import unittest
from unittest import mock

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'tools' / 'release'))
import check_metadata_urls  # noqa: E402
import version as versioning  # noqa: E402


class ReleaseMetadataTests(unittest.TestCase):
    def setUp(self):
        self.project = tomllib.loads((ROOT / 'pyproject.toml').read_text(encoding='utf-8'))['project']

    def test_pypi_status_is_beta_until_the_stable_gates_pass(self):
        statuses = [c for c in self.project['classifiers'] if c.startswith('Development Status ::')]
        self.assertEqual(statuses, ['Development Status :: 4 - Beta'])
        psd1 = (ROOT / 'powershell/UI4ME.Core/UI4ME.Core.psd1').read_text(encoding='utf-8')
        self.assertRegex(psd1, r"Description = '[^']*beta")
        self.assertIn("'Beta'", psd1)

    def test_changelog_is_cumulative_and_linked(self):
        self.assertTrue(self.project['urls']['Changelog'].endswith('/CHANGELOG.md'))
        changelog = (ROOT / 'CHANGELOG.md').read_text(encoding='utf-8')
        headings = re.findall(r'(?m)^## \[([^\]]+)\]', changelog)
        self.assertEqual(headings[0], 'Unreleased')
        for released in ('1.0.1', '1.0.0'):
            self.assertIn(released, headings)
        self.assertIn(f'[{versioning.source_version()}]', changelog)

    def test_policy_documents_exist(self):
        security = (ROOT / 'SECURITY.md').read_text(encoding='utf-8')
        self.assertIn('Report a vulnerability', security)
        policy = (ROOT / 'docs/release/versioning.md').read_text(encoding='utf-8')
        for topic in ('Semantic Versioning', 'Deprecation', 'Protocol and capability policy',
                      'Support policy', 'Yanking and rollback'):
            self.assertIn(topic, policy)

    def test_metadata_urls_are_collected_from_every_channel(self):
        urls = check_metadata_urls.collect()
        self.assertIn('pyproject Changelog', urls)
        self.assertIn('psd1 LicenseUri', urls)
        self.assertIn('psd1 ProjectUri', urls)
        self.assertTrue(any(k.startswith('pypi.md link') for k in urls))
        self.assertTrue(all(u.startswith('https://') for u in urls.values()), urls)

    def run_check(self, statuses) -> tuple[int, str]:
        out, err = io.StringIO(), io.StringIO()
        with mock.patch.object(check_metadata_urls, 'status', side_effect=statuses), \
                redirect_stdout(out), redirect_stderr(err):
            code = check_metadata_urls.main(['--network'])
        return code, err.getvalue()

    def test_network_check_requires_every_url_to_answer_200(self):
        count = len(check_metadata_urls.collect())
        self.assertEqual(self.run_check(lambda url: 200)[0], 0)
        code, err = self.run_check(lambda url: 404 if url.endswith('/LICENSE') else 200)
        self.assertEqual(code, 1)
        self.assertIn('psd1 LicenseUri', err)
        self.assertGreater(count, 3)

    def test_offline_mode_never_fetches(self):
        with mock.patch.object(check_metadata_urls, 'status', side_effect=AssertionError('fetched')), \
                redirect_stdout(io.StringIO()):
            self.assertEqual(check_metadata_urls.main([]), 0)


if __name__ == '__main__':
    unittest.main()
