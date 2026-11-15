#!/usr/bin/env python3
"""Refuse to publish unless the required CI run for the exact commit succeeded.

F-10 / RELEASE-01: 1.0.0 was published from a commit whose Windows CI later
failed, because nothing in the publish workflows looked at CI. Branch
protection is not available on the current GitHub plan, so the publish
preflight calls this gate instead. It works on any plan: it only reads the
Actions REST API with the workflow token (``actions: read``).

For every workflow listed in ``required_checks.json`` the gate requires:

* at least one run of that workflow for exactly ``--sha`` (and the configured
  event, normally ``push`` to ``main``);
* the newest such run is ``completed`` with conclusion ``success``;
* every required job name exists in that run's latest attempt and concluded
  ``success`` (a skipped or missing job is a refusal, so dropping a job from CI
  cannot silently weaken the gate).

``--recorded FILE`` replays recorded API responses instead of calling GitHub
(dry-run mode, used by ``tools/test_ci_gate.py``). The file maps request
paths, exactly as built by this module, to JSON bodies.

Exit status: 0 accepted, 1 refused, 2 usage or API error.
"""
from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import re
import sys
import time
from typing import Callable
import urllib.error
import urllib.parse
import urllib.request

HERE = Path(__file__).resolve().parent
DEFAULT_CONFIG = HERE / 'required_checks.json'
SHA_RE = re.compile(r'^[0-9a-f]{40}$')
REPO_RE = re.compile(r'^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+$')

Fetch = Callable[[str], dict]


class GateRefused(Exception):
    """The commit does not have the required green CI."""


def load_config(path: Path = DEFAULT_CONFIG) -> list[dict]:
    data = json.loads(path.read_text(encoding='utf-8'))
    if data.get('schema') != 1 or not isinstance(data.get('workflows'), list) or not data['workflows']:
        raise ValueError(f'{path}: expected schema 1 with a non-empty "workflows" list')
    for item in data['workflows']:
        if not isinstance(item.get('file'), str) or not item['file'].endswith(('.yml', '.yaml')):
            raise ValueError(f'{path}: every workflow needs a "file" (e.g. ci.yml)')
        if '/' in item['file']:
            raise ValueError(f'{path}: "file" is the workflow file name under .github/workflows')
        jobs = item.get('jobs')
        if not isinstance(jobs, list) or not jobs or not all(isinstance(j, str) and j for j in jobs):
            raise ValueError(f'{path}: workflow {item["file"]} needs a non-empty "jobs" list')
        if len(set(jobs)) != len(jobs):
            raise ValueError(f'{path}: duplicate job names for {item["file"]}')
    return data['workflows']


def runs_path(repo: str, workflow_file: str, sha: str, event: str | None, page: int) -> str:
    query = {'head_sha': sha, 'per_page': '100', 'page': str(page)}
    if event:
        query['event'] = event
    return (f'/repos/{repo}/actions/workflows/{urllib.parse.quote(workflow_file)}/runs?'
            + urllib.parse.urlencode(query))


def jobs_path(repo: str, run_id: int, page: int) -> str:
    return (f'/repos/{repo}/actions/runs/{int(run_id)}/jobs?'
            + urllib.parse.urlencode({'filter': 'latest', 'per_page': '100', 'page': str(page)}))


def _collect(fetch: Fetch, make_path: Callable[[int], str], key: str) -> list[dict]:
    items: list[dict] = []
    page = 1
    while True:
        body = fetch(make_path(page))
        batch = body.get(key)
        if not isinstance(batch, list):
            raise ValueError(f'Unexpected API response for {make_path(page)}: no "{key}" list')
        items.extend(batch)
        total = body.get('total_count', len(items))
        if not batch or len(items) >= total or page >= 50:
            return items
        page += 1


def check_workflow(fetch: Fetch, repo: str, sha: str, workflow: dict, branch: str | None) -> dict:
    """Return a report for one workflow, or raise GateRefused."""
    name = workflow['file']
    event = workflow.get('event', 'push')
    runs = _collect(fetch, lambda page: runs_path(repo, name, sha, event, page), 'workflow_runs')
    # The API filters by head_sha already; check again so a recorded or
    # proxied response for another commit can never be accepted.
    runs = [r for r in runs if r.get('head_sha') == sha]
    if branch:
        runs = [r for r in runs if r.get('head_branch') == branch]
    if not runs:
        raise GateRefused(f'{name}: no {event} run for {sha}'
                          + (f' on {branch}' if branch else '')
                          + '. Push the commit and wait for CI before publishing.')
    newest = max(runs, key=lambda r: (r.get('created_at') or '', r.get('run_number') or 0,
                                      r.get('run_attempt') or 0))
    where = newest.get('html_url') or f'run {newest.get("id")}'
    if newest.get('status') != 'completed':
        raise GateRefused(f'{name}: the CI run for {sha} is still {newest.get("status")} ({where}); '
                          'wait for it to finish')
    if newest.get('conclusion') != 'success':
        raise GateRefused(f'{name}: the CI run for {sha} concluded {newest.get("conclusion")} ({where})')
    jobs = _collect(fetch, lambda page: jobs_path(repo, newest['id'], page), 'jobs')
    by_name: dict[str, dict] = {}
    for job in jobs:
        by_name[job.get('name')] = job
    problems = []
    for required in workflow['jobs']:
        job = by_name.get(required)
        if job is None:
            problems.append(f'{required}: missing')
        elif job.get('conclusion') != 'success':
            problems.append(f'{required}: {job.get("status")}/{job.get("conclusion")}')
    if problems:
        raise GateRefused(f'{name}: required jobs did not succeed in {where}: ' + '; '.join(problems))
    return {'workflow': name, 'run_id': newest['id'], 'run_attempt': newest.get('run_attempt'),
            'url': newest.get('html_url'), 'jobs': sorted(workflow['jobs'])}


def evaluate(fetch: Fetch, repo: str, sha: str, workflows: list[dict], branch: str | None = 'main') -> list[dict]:
    if not REPO_RE.match(repo):
        raise ValueError(f'Invalid repository name: {repo!r}')
    if not SHA_RE.match(sha):
        raise ValueError(f'The gate needs a full 40-character lower-case commit SHA, got {sha!r}')
    return [check_workflow(fetch, repo, sha, w, branch) for w in workflows]


def github_fetcher(token: str, api_url: str) -> Fetch:
    base = api_url.rstrip('/')

    def fetch(path: str) -> dict:
        request = urllib.request.Request(base + path, headers={
            'Accept': 'application/vnd.github+json',
            'Authorization': f'Bearer {token}',
            'X-GitHub-Api-Version': '2022-11-28',
            'User-Agent': 'ui4me-release-ci-gate',
        })
        try:
            with urllib.request.urlopen(request, timeout=30) as response:
                return json.load(response)
        except urllib.error.HTTPError as error:
            raise ValueError(f'GitHub API {path} returned HTTP {error.code}') from error
    return fetch


def recorded_fetcher(path: Path) -> Fetch:
    responses = json.loads(path.read_text(encoding='utf-8'))
    if not isinstance(responses, dict):
        raise ValueError('Recorded responses must be a JSON object of path -> body')

    def fetch(request_path: str) -> dict:
        if request_path not in responses:
            # An unrecorded request behaves like an empty listing, which the
            # gate treats as "no run" (refusal), never as success.
            return {'total_count': 0, 'workflow_runs': [], 'jobs': []}
        return responses[request_path]
    return fetch


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.split('\n\n', 1)[0])
    parser.add_argument('--repo', default=os.environ.get('GITHUB_REPOSITORY'))
    parser.add_argument('--sha', required=True)
    parser.add_argument('--branch', default='main', help='required head branch of the CI run ("" for any)')
    parser.add_argument('--config', type=Path, default=DEFAULT_CONFIG)
    parser.add_argument('--recorded', type=Path, help='replay recorded API responses (dry run, no network)')
    parser.add_argument('--wait-seconds', type=int, default=0,
                        help='keep polling while the run is still in progress, up to this long')
    args = parser.parse_args(argv)
    if not args.repo:
        parser.error('--repo or GITHUB_REPOSITORY is required')
    try:
        workflows = load_config(args.config)
        if args.recorded:
            fetch = recorded_fetcher(args.recorded)
        else:
            token = os.environ.get('GH_TOKEN') or os.environ.get('GITHUB_TOKEN')
            if not token:
                parser.exit(2, 'CI gate: GH_TOKEN (workflow token with actions: read) is required\n')
            fetch = github_fetcher(token, os.environ.get('GITHUB_API_URL', 'https://api.github.com'))
        deadline = time.monotonic() + max(0, args.wait_seconds)
        while True:
            try:
                reports = evaluate(fetch, args.repo, args.sha, workflows, args.branch or None)
                break
            except GateRefused as refusal:
                if 'still' in str(refusal) and time.monotonic() < deadline:
                    print(f'CI gate: waiting: {refusal}', file=sys.stderr)
                    time.sleep(30)
                    continue
                raise
    except GateRefused as refusal:
        print(f'CI gate REFUSED: {refusal}', file=sys.stderr)
        return 1
    except (OSError, ValueError) as error:
        print(f'CI gate ERROR: {error}', file=sys.stderr)
        return 2
    print(json.dumps({'result': 'ACCEPTED', 'sha': args.sha, 'workflows': reports}, indent=2))
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
