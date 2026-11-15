"""Publish CI gate (F-10 / RELEASE-01) against recorded GitHub API responses.

The recordings are real, trimmed responses for this repository: d053e09 is the
commit 1.0.0 was published from while its Windows CI failed; c6fd71d is 1.0.1
with green CI. The gate must refuse the first, accept the second, and refuse a
commit with no CI run at all.
"""
from __future__ import annotations

import copy
import io
import json
from contextlib import redirect_stderr, redirect_stdout
from pathlib import Path
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'tools' / 'release'))
import ci_gate  # noqa: E402

DATA = ROOT / 'tools' / 'release' / 'testdata' / 'ci_gate'
RECORDED = DATA / 'recorded-1.0.x.json'
CONFIG_1_0 = DATA / 'required-1.0.x.json'
REPO = 'ui4me/ui4me-core'
RED = 'd053e09478a081a48b68782f2204c4f649ae0e3b'
GREEN = 'c6fd71dc5c15dcdfdcdc1500dc156c1b929c216c'
MISSING = '0' * 40


def run_cli(*args: str) -> tuple[int, str, str]:
    out, err = io.StringIO(), io.StringIO()
    with redirect_stdout(out), redirect_stderr(err):
        code = ci_gate.main(list(args))
    return code, out.getvalue(), err.getvalue()


class RecordedGateTests(unittest.TestCase):
    def gate(self, sha: str, recorded: Path = RECORDED, config: Path = CONFIG_1_0):
        return run_cli('--repo', REPO, '--sha', sha, '--config', str(config), '--recorded', str(recorded))

    def test_red_commit_is_refused(self):
        code, out, err = self.gate(RED)
        self.assertEqual(code, 1, out + err)
        self.assertIn('REFUSED', err)
        self.assertIn('concluded failure', err)

    def test_green_commit_is_accepted(self):
        code, out, err = self.gate(GREEN)
        self.assertEqual(code, 0, out + err)
        report = json.loads(out)
        self.assertEqual(report['result'], 'ACCEPTED')
        self.assertEqual(report['workflows'][0]['run_id'], 43551153042)

    def test_commit_without_ci_run_is_refused(self):
        code, out, err = self.gate(MISSING)
        self.assertEqual(code, 1, out + err)
        self.assertIn('no push run', err)

    def mutated(self, change) -> Path:
        data = json.loads(RECORDED.read_text(encoding='utf-8'))
        change(data)
        folder = tempfile.TemporaryDirectory()
        self.addCleanup(folder.cleanup)
        path = Path(folder.name) / 'recorded.json'
        path.write_text(json.dumps(data), encoding='utf-8')
        return path

    def green_runs_key(self) -> str:
        return ci_gate.runs_path(REPO, 'ci.yml', GREEN, 'push', 1)

    def test_in_progress_run_is_refused(self):
        def change(data):
            run = data[self.green_runs_key()]['workflow_runs'][0]
            run['status'], run['conclusion'] = 'in_progress', None
        code, _, err = self.gate(GREEN, self.mutated(change))
        self.assertEqual(code, 1)
        self.assertIn('still in_progress', err)

    def test_required_job_that_was_skipped_or_removed_is_refused(self):
        def skipped(data):
            jobs = data[ci_gate.jobs_path(REPO, 43551153042, 1)]['jobs']
            jobs[1]['conclusion'] = 'skipped'
        code, _, err = self.gate(GREEN, self.mutated(skipped))
        self.assertEqual(code, 1)
        self.assertIn('verify (windows-2022): completed/skipped', err)

        def removed(data):
            body = data[ci_gate.jobs_path(REPO, 43551153042, 1)]
            body['jobs'] = body['jobs'][:1]
            body['total_count'] = 1
        code, _, err = self.gate(GREEN, self.mutated(removed))
        self.assertEqual(code, 1)
        self.assertIn('verify (windows-2022): missing', err)

    def test_run_for_another_commit_is_never_accepted(self):
        # A response that (wrongly) lists a green run for a different SHA.
        def change(data):
            data[ci_gate.runs_path(REPO, 'ci.yml', MISSING, 'push', 1)] = copy.deepcopy(data[self.green_runs_key()])
        code, _, err = self.gate(MISSING, self.mutated(change))
        self.assertEqual(code, 1)
        self.assertIn('no push run', err)

    def test_run_from_another_branch_is_refused(self):
        def change(data):
            data[self.green_runs_key()]['workflow_runs'][0]['head_branch'] = 'feature'
        code, _, err = self.gate(GREEN, self.mutated(change))
        self.assertEqual(code, 1)

    def test_newest_rerun_decides(self):
        def change(data):
            runs = data[self.green_runs_key()]['workflow_runs']
            older = copy.deepcopy(runs[0])
            older.update(id=1, conclusion='failure', created_at='2026-11-14T01:00:00Z', run_number=12)
            runs.append(older)
            data[self.green_runs_key()]['total_count'] = 2
        self.assertEqual(self.gate(GREEN, self.mutated(change))[0], 0)

        def newer_red(data):
            runs = data[self.green_runs_key()]['workflow_runs']
            newer = copy.deepcopy(runs[0])
            newer.update(id=2, conclusion='failure', created_at='2026-11-15T01:00:00Z', run_number=14)
            runs.append(newer)
            data[self.green_runs_key()]['total_count'] = 2
        self.assertEqual(self.gate(GREEN, self.mutated(newer_red))[0], 1)

    def test_short_sha_and_bad_repo_are_usage_errors(self):
        self.assertEqual(run_cli('--repo', REPO, '--sha', 'c6fd71d', '--config', str(CONFIG_1_0),
                                 '--recorded', str(RECORDED))[0], 2)
        self.assertEqual(run_cli('--repo', 'bad repo', '--sha', GREEN, '--config', str(CONFIG_1_0),
                                 '--recorded', str(RECORDED))[0], 2)

    def test_current_required_checks_config_is_valid(self):
        workflows = ci_gate.load_config()
        self.assertEqual([w['file'] for w in workflows], ['ci.yml'])
        # The recorded 1.0.x green run predates the widened CI; under the
        # current requirements it must be refused (jobs missing), not accepted.
        code, _, err = run_cli('--repo', REPO, '--sha', GREEN, '--recorded', str(RECORDED))
        self.assertEqual(code, 1)
        self.assertIn('missing', err)


if __name__ == '__main__':
    unittest.main()
