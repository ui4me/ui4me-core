"""Structural tests of the GitHub workflows (RELEASE-15: assert structure, not substrings).

Each workflow is parsed with ``yaml.safe_load`` and the properties the release
process relies on are asserted on the parsed structure:

* CI signal (F-09, T7): clippy runs even when the verifier fails; ordinary CI
  never opts into the release-freeze manifest check; the publish workflows do.
* Publish gate (F-10, T8): the preflight runs the CI gate for the exact SHA
  with ``actions: read``; every publishing job depends on the preflight; PyPI
  upload happens only after the draft release; only manual dispatch on main.
* CI coverage (F-22): every claimed Python version, real-host Pester on pwsh 7
  (Linux, Windows) and Windows PowerShell 5.1, no path filter, clean-machine jobs.
* Hygiene (F-23, RELEASE-14): SHA-pinned actions with a version comment,
  digest-pinned images, no ``${{ }}`` inside shell scripts, timeouts,
  concurrency, no persisted checkout credentials.
* The required-check names in ``tools/release/required_checks.json`` are
  exactly names that ``ci.yml`` produces.

PyYAML is required; set ``UI4ME_REQUIRE_WORKFLOW_TESTS=1`` (CI does) to turn
its absence into a failure instead of a skip.
"""
from __future__ import annotations

import itertools
import json
import os
from pathlib import Path
import re
import tomllib
import unittest

ROOT = Path(__file__).resolve().parents[1]
WORKFLOWS = ROOT / '.github' / 'workflows'
try:
    import yaml
except ImportError:  # pragma: no cover - exercised only without PyYAML
    yaml = None
    if os.environ.get('UI4ME_REQUIRE_WORKFLOW_TESTS') == '1':
        raise

SHA_PIN = re.compile(r'^[\w.-]+/[\w./-]+@[0-9a-f]{40}$')
EXPRESSION = re.compile(r'\$\{\{\s*matrix\.(\w+)\s*\}\}')


def load(name: str) -> dict:
    return yaml.safe_load((WORKFLOWS / name).read_text(encoding='utf-8'))


def triggers(workflow: dict) -> dict:
    # YAML 1.1 reads the bare key `on` as boolean True.
    value = workflow.get('on', workflow.get(True))
    return value if isinstance(value, dict) else {key: None for key in ([value] if isinstance(value, str) else value)}


def steps(job: dict) -> list[dict]:
    return job.get('steps', [])


def step_index(job: dict, predicate) -> int:
    for index, step in enumerate(steps(job)):
        if predicate(step):
            return index
    raise AssertionError('step not found')


def needs(job: dict) -> list[str]:
    value = job.get('needs', [])
    return [value] if isinstance(value, str) else list(value)


def ancestors(jobs: dict, name: str) -> set[str]:
    seen: set[str] = set()
    pending = needs(jobs[name])
    while pending:
        current = pending.pop()
        if current not in seen:
            seen.add(current)
            pending.extend(needs(jobs[current]))
    return seen


def matrix_rows(job: dict) -> list[dict]:
    matrix = (job.get('strategy') or {}).get('matrix') or {}
    if not matrix:
        return [{}]
    axes = {k: v for k, v in matrix.items() if k not in ('include', 'exclude') and isinstance(v, list)}
    rows = [dict(zip(axes, combo)) for combo in itertools.product(*axes.values())] if axes else []
    for extra in matrix.get('include', []):
        rows.append(dict(extra))
    return rows or [{}]


def job_display_names(job_id: str, job: dict) -> list[str]:
    template = job.get('name', job_id)
    return [EXPRESSION.sub(lambda m, r=row: str(r[m.group(1)]), template) for row in matrix_rows(job)]


def all_workflows() -> dict[str, dict]:
    return {p.name: load(p.name) for p in sorted(WORKFLOWS.glob('*.y*ml'))}


@unittest.skipIf(yaml is None, 'PyYAML is not installed (pip install -r tools/release/requirements-ci.txt)')
class CiSignalStructureTests(unittest.TestCase):
    def setUp(self):
        self.ci = load('ci.yml')
        self.verify = self.ci['jobs']['verify']

    def test_lint_runs_even_when_the_verifier_fails(self):
        verify_at = step_index(self.verify, lambda s: 'tools/verify.py' in s.get('run', ''))
        clippy_at = step_index(self.verify, lambda s: 'cargo clippy' in s.get('run', ''))
        self.assertGreater(clippy_at, verify_at)
        clippy = steps(self.verify)[clippy_at]
        self.assertIn('!cancelled()', str(clippy.get('if', '')), 'clippy must not be skipped after a failure')
        self.assertIn('-D warnings', clippy['run'])
        self.assertFalse(steps(self.verify)[verify_at].get('continue-on-error'), 'a verifier failure must fail the job')

    def test_ordinary_ci_does_not_opt_into_the_release_freeze_check(self):
        text = (WORKFLOWS / 'ci.yml').read_text(encoding='utf-8')
        self.assertNotIn('UI4ME_RELEASE_MANIFEST_CHECK', text)
        run = steps(self.verify)[step_index(self.verify, lambda s: 'tools/verify.py' in s.get('run', ''))]['run']
        self.assertNotIn('--stage', run, 'CI runs the default verifier stages')

    def test_publish_workflows_run_the_release_freeze_check(self):
        for name, job_id in (('publish.yaml', 'preflight'), ('publish-powershell.yaml', 'build-and-check')):
            job = load(name)['jobs'][job_id]
            checks = [s for s in steps(job) if 'refresh_source_manifest.py --check' in s.get('run', '')]
            self.assertTrue(checks, name)
            self.assertTrue(any((s.get('env') or {}).get('UI4ME_RELEASE_MANIFEST_CHECK') == '1' for s in checks), name)
            self.assertTrue(any('verify_package.py' in s.get('run', '') for s in steps(job)), name)

    def test_manifest_check_runs_in_ci_when_release_tooling_changes(self):
        workflow = load('release-manifest.yml')
        on = triggers(workflow)
        self.assertEqual(on['pull_request']['paths'], ['tools/release/**'])
        self.assertEqual(on['push']['paths'], ['tools/release/**'])
        job = workflow['jobs']['manifest']
        self.assertTrue(any((s.get('env') or {}).get('UI4ME_RELEASE_MANIFEST_CHECK') == '1' for s in steps(job)))

    def test_ci_triggers_concurrency_and_timeouts(self):
        on = triggers(self.ci)
        self.assertEqual(on['push'], {'branches': ['main']})
        self.assertIn('pull_request', on)
        self.assertNotIn('paths', on['pull_request'] or {}, 'CI must not be path-filtered')
        concurrency = self.ci['concurrency']
        self.assertIn('pull_request', str(concurrency['cancel-in-progress']))
        for job_id, job in self.ci['jobs'].items():
            self.assertIsInstance(job.get('timeout-minutes'), int, job_id)
        for job_id in ('verify', 'linux-host'):
            self.assertTrue(any(s.get('uses', '').startswith('Swatinem/rust-cache@') for s in steps(self.ci['jobs'][job_id])),
                            job_id)


@unittest.skipIf(yaml is None, 'PyYAML is not installed (pip install -r tools/release/requirements-ci.txt)')
class PublishGateStructureTests(unittest.TestCase):
    def setUp(self):
        self.publish = load('publish.yaml')
        self.jobs = self.publish['jobs']

    def test_only_manual_dispatch_on_main(self):
        for name in ('publish.yaml', 'publish-powershell.yaml'):
            workflow = load(name)
            self.assertEqual(set(triggers(workflow)), {'workflow_dispatch'}, name)
            first = next(iter(workflow['jobs'].values()))
            self.assertIn("github.ref == 'refs/heads/main'", first['if'], name)
            self.assertEqual(workflow['permissions'], {'contents': 'read'}, name)

    def test_preflight_requires_green_ci_for_the_exact_sha(self):
        for name, job_id in (('publish.yaml', 'preflight'), ('publish-powershell.yaml', 'build-and-check')):
            job = load(name)['jobs'][job_id]
            self.assertEqual(job['permissions'].get('actions'), 'read', name)
            gate = [s for s in steps(job) if 'tools/release/ci_gate.py' in s.get('run', '')]
            self.assertEqual(len(gate), 1, name)
            self.assertIn('--sha "$SOURCE_SHA"', gate[0]['run'])
            self.assertIn('github.token', gate[0]['env']['GH_TOKEN'])
            self.assertNotIn('continue-on-error', gate[0])
            self.assertNotIn('if', gate[0], 'the gate must never be conditional')

    def test_every_publishing_job_depends_on_the_preflight(self):
        for job_id in ('build', 'stage', 'attest', 'draft-release', 'pypi', 'publish-release'):
            self.assertIn('preflight', ancestors(self.jobs, job_id) | set(needs(self.jobs[job_id])), job_id)
        self.assertIn('draft-release', needs(self.jobs['pypi']))
        self.assertIn('pypi', needs(self.jobs['publish-release']))
        self.assertIn('attest', ancestors(self.jobs, 'pypi'))
        for job_id in ('stage', 'attest', 'draft-release', 'pypi', 'publish-release'):
            condition = str(self.jobs[job_id].get('if', ''))
            self.assertIn('!cancelled()', condition, job_id)
        for job_id in ('attest', 'draft-release', 'pypi', 'publish-release'):
            self.assertIn('!failure()', str(self.jobs[job_id]['if']), job_id)
        self.assertIn('!inputs.dry_run', self.jobs['attest']['if'])

    def test_pypi_upload_uses_trusted_publishing_only(self):
        pypi = self.jobs['pypi']
        self.assertEqual(pypi['environment']['name'], 'pypi')
        self.assertEqual(pypi['permissions'], {'id-token': 'write', 'contents': 'read'})
        uses = [s['uses'] for s in steps(pypi) if 'uses' in s]
        self.assertTrue(any(u.startswith('pypa/gh-action-pypi-publish@') for u in uses))
        text = (WORKFLOWS / 'publish.yaml').read_text(encoding='utf-8')
        self.assertNotIn('secrets.', text)
        holders = {j for j, job in self.jobs.items() if (job.get('permissions') or {}).get('id-token') == 'write'}
        self.assertEqual(holders, {'pypi', 'attest'})
        self.assertEqual(self.jobs['build']['uses'], './.github/workflows/build-dist.yml')

    def test_candidate_promotion_requires_the_qualified_windows_hash(self):
        run = steps(self.jobs['preflight'])[step_index(self.jobs['preflight'], lambda s: s.get('id') == 'source')]['run']
        self.assertIn('Promoting a candidate requires expected_windows_wheel_sha256', run)
        stage = self.jobs['stage']
        verify = [s for s in steps(stage) if 'verify_publish_dist.py' in s.get('run', '')]
        self.assertEqual(len(verify), 1)
        self.assertIn('EXPECTED_WINDOWS_SHA256', verify[0]['run'])

    def test_powershell_publish_checks_the_tag_commit(self):
        job = load('publish-powershell.yaml')['jobs']['build-and-check']
        run = steps(job)[step_index(job, lambda s: s.get('id') == 'verify')]['run']
        self.assertIn('git/ref/tags/v$VERSION', run)
        self.assertIn('"$tag_sha" != "$head_sha"', run)

    def test_required_checks_are_produced_by_ci(self):
        config = json.loads((ROOT / 'tools/release/required_checks.json').read_text(encoding='utf-8'))
        for workflow in config['workflows']:
            parsed = load(workflow['file'])
            produced = {n for job_id, job in parsed['jobs'].items() for n in job_display_names(job_id, job)}
            missing = set(workflow['jobs']) - produced
            self.assertEqual(missing, set(), f'{workflow["file"]} does not produce {sorted(missing)}')
            self.assertIn(workflow.get('event', 'push'), triggers(parsed))


@unittest.skipIf(yaml is None, 'PyYAML is not installed (pip install -r tools/release/requirements-ci.txt)')
class CoverageStructureTests(unittest.TestCase):
    def setUp(self):
        self.ci = load('ci.yml')

    def test_python_matrix_covers_every_claimed_version(self):
        classifiers = tomllib.loads((ROOT / 'pyproject.toml').read_text(encoding='utf-8'))['project']['classifiers']
        claimed = {c.rsplit(' :: ', 1)[1] for c in classifiers
                   if re.fullmatch(r'Programming Language :: Python :: 3\.\d+', c)}
        tested = set(self.ci['jobs']['python']['strategy']['matrix']['python'])
        self.assertEqual(tested, claimed)
        run = ' '.join(s.get('run', '') for s in steps(self.ci['jobs']['python']))
        for module in ('test_bootstrap', 'test_application', 'test_reference_client'):
            self.assertIn(module, run)
        self.assertIn('linux-host', needs(self.ci['jobs']['python']))

    def test_real_host_pester_on_every_edition(self):
        verify = self.ci['jobs']['verify']
        pester = [s for s in steps(verify) if 'run_pester.ps1' in s.get('run', '')]
        shells = {s['shell'] for s in pester}
        self.assertEqual(shells, {'pwsh', 'powershell'})
        for step in pester:
            self.assertIn('-HostBinary', step['run'])
            self.assertIn('!cancelled()', str(step.get('if', '')))
        windows_only = [s for s in pester if s['shell'] == 'powershell']
        self.assertIn("runner.os == 'Windows'", windows_only[0]['if'])
        self.assertEqual(set(verify['strategy']['matrix']['os']), {'ubuntu-24.04', 'windows-2022'})
        self.assertIn('MaxSkipped', (ROOT / 'tools/release/run_pester.ps1').read_text(encoding='utf-8'))

    def test_clean_machine_jobs(self):
        images = {row['image'] for row in matrix_rows(self.ci['jobs']['clean-wheel'])}
        self.assertEqual(images, {'python:3.10-slim', 'python:3.14-slim'})
        self.assertEqual(self.ci['jobs']['clean-wheel']['container']['image'], '${{ matrix.ref }}')
        allusers = ' '.join(s.get('run', '') for s in steps(self.ci['jobs']['powershell-allusers']))
        self.assertIn('smoke_psmodule_allusers.sh', allusers)

    def test_nightly(self):
        nightly = load('nightly.yml')
        self.assertIn('schedule', triggers(nightly))
        self.assertIn('workflow_dispatch', triggers(nightly))
        runs = {job_id: ' '.join(s.get('run', '') for s in steps(job)) for job_id, job in nightly['jobs'].items()}
        self.assertIn('-- --ignored', runs['ignored-tests'])
        self.assertIn('--release', runs['ignored-tests'])
        self.assertIn('verify_patches.sh', runs['patch-stacks'])
        self.assertIn('check advisories', runs['advisories'])
        self.assertIn('windowed_smoke.py', runs['windowed-smoke'])
        report = nightly['jobs']['report-failure']
        self.assertIn('failure()', report['if'])
        self.assertEqual(report['permissions'], {'issues': 'write'})
        self.assertEqual(set(needs(report)), set(nightly['jobs']) - {'report-failure'})


@unittest.skipIf(yaml is None, 'PyYAML is not installed (pip install -r tools/release/requirements-ci.txt)')
class HygieneStructureTests(unittest.TestCase):
    def test_actions_are_pinned_by_sha_with_a_version_comment(self):
        for path in sorted(WORKFLOWS.glob('*.y*ml')):
            for number, line in enumerate(path.read_text(encoding='utf-8').splitlines(), 1):
                match = re.match(r'^\s*(?:-\s+)?uses:\s*(\S+)(.*)$', line)
                if not match or match.group(1).startswith('./'):
                    continue
                self.assertRegex(match.group(1), SHA_PIN, f'{path.name}:{number}')
                self.assertRegex(match.group(2), r'#\s*v\d', f'{path.name}:{number} needs a version comment')

    def test_no_expressions_inside_shell_scripts(self):
        # Template injection (zizmor): values reach scripts only through env.
        for name, workflow in all_workflows().items():
            for job_id, job in workflow['jobs'].items():
                for step in steps(job):
                    self.assertNotIn('${{', step.get('run', ''), f'{name} {job_id}: {step.get("name")}')

    def test_images_are_pinned_by_digest(self):
        for name, workflow in all_workflows().items():
            for job_id, job in workflow['jobs'].items():
                container = job.get('container')
                if container:
                    image = container['image'] if isinstance(container, dict) else container
                    refs = [row.get('ref') for row in matrix_rows(job)] if 'matrix.' in image else [image]
                    for ref in refs:
                        self.assertRegex(ref or '', r'@sha256:[0-9a-f]{64}$', f'{name} {job_id}')
                for step in steps(job):
                    for image in re.findall(r'(?m)^\s*((?:[\w.-]+/)*[\w.-]+:[\w.-]+(?:@\S+)?) \\?$', step.get('run', '')):
                        if '/' in image or image.startswith(('python:', 'rhysd')):
                            self.assertIn('@sha256:', image, f'{name} {job_id}: {image}')

    def test_no_privileged_triggers_and_no_persisted_credentials(self):
        for name, workflow in all_workflows().items():
            self.assertNotIn('pull_request_target', triggers(workflow), name)
            self.assertNotIn('workflow_run', triggers(workflow), name)
            self.assertIn('permissions', workflow, f'{name}: top-level permissions required')
            for job_id, job in workflow['jobs'].items():
                if 'uses' in job:
                    continue
                self.assertIsInstance(job.get('timeout-minutes'), int, f'{name} {job_id}')
                for step in steps(job):
                    if step.get('uses', '').startswith('actions/checkout@'):
                        self.assertIs((step.get('with') or {}).get('persist-credentials'), False, f'{name} {job_id}')

    def test_dependabot_covers_actions_cargo_and_pip(self):
        config = yaml.safe_load((ROOT / '.github/dependabot.yml').read_text(encoding='utf-8'))
        ecosystems = {u['package-ecosystem'] for u in config['updates']}
        self.assertEqual(ecosystems, {'github-actions', 'cargo', 'pip'})

    def test_retired_workflows_are_gone(self):
        # RELEASE-18: one build definition (build-dist.yml) instead of three copies.
        for name in ('release-linux.yml', 'release-windows.yml', 'test-powershell.yaml'):
            self.assertFalse((WORKFLOWS / name).exists(), name)
        self.assertIn('workflow_call', triggers(load('build-dist.yml')))


if __name__ == '__main__':
    unittest.main()
