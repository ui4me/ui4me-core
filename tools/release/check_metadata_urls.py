#!/usr/bin/env python3
"""Every URL in the published package metadata must resolve without credentials (F-20 / RELEASE-05).

Collects ``[project.urls]`` from pyproject.toml, the ``*Uri`` fields of the
PowerShell manifest, and the absolute links that the PyPI description
(docs/publish/pypi.md) contains. Without ``--network`` it only lists them
(and checks that they are well-formed https URLs). With ``--network`` every URL
must answer 200 to an unauthenticated request; the publish preflight runs this
when ``check_metadata_urls`` is set.

Today this fails by design: the repository is private, so every github.com
link returns 404 to users. Whether to make it public, or to point metadata at
a public documentation site, is an owner decision (roadmap 6.2); this check is
the acceptance test for either choice.
"""
from __future__ import annotations

import argparse
import json
from pathlib import Path
import re
import sys
import urllib.error
import urllib.request

ROOT = Path(__file__).resolve().parents[2]
URL = re.compile(r'https?://[^\s)\]>"\'`]+')


def pyproject_urls(root: Path = ROOT) -> dict[str, str]:
    text = (root / 'pyproject.toml').read_text(encoding='utf-8')
    section = re.search(r'(?ms)^\[project\.urls\]\s*$(.*?)(?=^\[)', text + '\n[')
    found = {}
    for line in (section.group(1) if section else '').splitlines():
        match = re.match(r'\s*([A-Za-z][\w -]*)\s*=\s*"([^"]+)"', line)
        if match:
            found[f'pyproject {match.group(1).strip()}'] = match.group(2)
    return found


def psd1_urls(root: Path = ROOT) -> dict[str, str]:
    text = (root / 'powershell/UI4ME.Core/UI4ME.Core.psd1').read_text(encoding='utf-8')
    return {f'psd1 {name}': value for name, value in re.findall(r"(?m)^\s*(\w+Uri)\s*=\s*'([^']+)'", text)}


def description_urls(root: Path = ROOT) -> dict[str, str]:
    text = (root / 'docs/publish/pypi.md').read_text(encoding='utf-8')
    return {f'pypi.md link {i}': url.rstrip('.,;') for i, url in enumerate(sorted(set(URL.findall(text))), 1)}


def collect(root: Path = ROOT) -> dict[str, str]:
    return {**pyproject_urls(root), **psd1_urls(root), **description_urls(root)}


def status(url: str) -> int | str:
    request = urllib.request.Request(url, method='GET', headers={'User-Agent': 'ui4me-release-metadata-check'})
    try:
        with urllib.request.urlopen(request, timeout=20) as response:
            return response.status
    except urllib.error.HTTPError as error:
        return error.code
    except (urllib.error.URLError, TimeoutError, OSError) as error:
        return f'error: {error}'


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.split('\n\n', 1)[0])
    parser.add_argument('--network', action='store_true', help='fetch every URL without credentials')
    args = parser.parse_args(argv)
    urls = collect()
    bad_form = {k: v for k, v in urls.items() if not v.startswith('https://')}
    rows, failed = [], dict(bad_form)
    for name, url in sorted(urls.items()):
        result = status(url) if args.network else 'not fetched'
        if args.network and result != 200:
            failed[name] = url
        rows.append({'field': name, 'url': url, 'status': result})
    print(json.dumps(rows, indent=2))
    if failed:
        print(f'FAIL: {len(failed)} metadata URLs do not resolve publicly or are not https:', file=sys.stderr)
        for name, url in sorted(failed.items()):
            print(f'  {name}: {url}', file=sys.stderr)
        return 1
    print(f"PASS: {len(urls)} metadata URLs {'resolve' if args.network else 'are well-formed'}")
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
