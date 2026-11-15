"""CI signal (F-09 / RELEASE-02): an ordinary change can be green and lint always runs.

These tests pin the two halves of the fix that do not need a workflow parser:
the release-freeze manifest check is opt-in, and the default verifier stages
never include it. The workflow side (clippy runs even when tests fail, the
manifest check runs in the publish workflows) is asserted structurally in
``test_workflows.py``.
"""
from __future__ import annotations

import os
from pathlib import Path
import subprocess
import sys
import unittest

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'tools'))
import verify  # noqa: E402
from test_source_manifest import RELEASE_CHECK_ENV  # noqa: E402


def run_manifest_suite(extra_env: dict[str, str]) -> subprocess.CompletedProcess:
    env = {k: v for k, v in os.environ.items() if k != RELEASE_CHECK_ENV}
    env.update(extra_env)
    return subprocess.run([sys.executable, '-m', 'unittest', '-v', 'test_source_manifest'],
                          cwd=ROOT / 'tools', env=env, capture_output=True, text=True, timeout=300)


class CiSignalTests(unittest.TestCase):
    def test_default_stages_exclude_the_release_freeze_check(self):
        self.assertNotIn('release-manifest', verify.DEFAULT_STAGES)
        self.assertTrue(set(verify.DEFAULT_STAGES) <= set(verify.STAGES))
        # The default set is what CI and contributors run; it must keep the
        # whole functional surface.
        for stage in ('fmt', 'check', 'test', 'python', 'headless', 'renderer', 'faults'):
            self.assertIn(stage, verify.DEFAULT_STAGES)
        self.assertIn('release-manifest', verify.STAGES)
        self.assertIn('clippy', verify.STAGES)

    def test_manifest_completeness_check_is_skipped_unless_requested(self):
        result = run_manifest_suite({})
        output = result.stdout + result.stderr
        self.assertEqual(result.returncode, 0, output)
        self.assertRegex(output, r'test_checked_manifest_matches_entire_source .*skipped')

    def test_manifest_completeness_check_runs_when_requested(self):
        result = run_manifest_suite({RELEASE_CHECK_ENV: '1'})
        output = result.stdout + result.stderr
        # Pass or fail depends on whether this tree is a frozen release tree;
        # what matters is that the check actually executed.
        self.assertNotRegex(output, r'test_checked_manifest_matches_entire_source .*skipped')
        self.assertIn('test_checked_manifest_matches_entire_source', output)


if __name__ == '__main__':
    unittest.main()
