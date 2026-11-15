#!/usr/bin/env python3
"""The single source of the UI4ME Core release version (F-21 / RELEASE-07).

``Cargo.toml [workspace.package].version`` is the source of truth. Release
machinery (workflows, verifiers, staging scripts) reads it through this module
instead of carrying its own literal, so a release does not edit CI.

A few files must contain the literal because their format requires it
(``FORCED``); ``bump_version.py`` rewrites exactly those, and
``check_consistency`` (used by ``verify_source.py``) proves they agree.

Works on Python 3.10 (no ``tomllib``), because the installed-wheel smoke test
runs it on every supported interpreter.

CLI: ``python tools/release/version.py`` prints the version;
``--check`` verifies the forced locations; ``--artifacts`` prints file names.
"""
from __future__ import annotations

import argparse
import json
from pathlib import Path
import re
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[2]
SEMVER = re.compile(r'^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$')

# Path -> (regex with exactly one group around the version, how the version is written).
# 'plain' is X.Y.Z; 'win4' is the four-part Windows assembly version X.Y.Z.0.
FORCED: dict[str, tuple[str, str]] = {
    'Cargo.toml': (r'(?ms)^\[workspace\.package\]\s*$.*?^version\s*=\s*"([^"]+)"', 'plain'),
    'pyproject.toml': (r'(?ms)^\[project\]\s*$.*?^version\s*=\s*"([^"]+)"', 'plain'),
    'python/ui4me_core/__init__.py': (r'(?m)^__version__ = "([^"]+)"', 'plain'),
    'python/ui4me_core/client.py': (r'(?m)^CLIENT_VERSION = "([^"]+)"', 'plain'),
    'powershell/UI4ME.Core/UI4ME.Core.psd1': (r"(?m)^\s*ModuleVersion = '([^']+)'", 'plain'),
    'powershell/UI4ME.Core/UI4ME.Core.psm1': (r"(?m)^\$script:UI4MEVersion = '([^']+)'", 'plain'),
    'crates/ui4me-host/host.manifest': (r'(?m)<assemblyIdentity version="([^"]+)"', 'win4'),
}
# Workspace crates whose Cargo.lock entries carry the workspace version.
WORKSPACE_CRATES = ('ui4me-host', 'ui4me-document', 'ui4me-renderer', 'ui4me-assets', 'ui4me-engine-types')
# Forced locations that are known to disagree until the lead runs the first
# bump (F-21): host.manifest has said 0.1.0.0 since 0.1. Strict mode
# (UI4ME_STRICT_VERSION_SOURCE=1, or check_consistency(strict=True)) includes them.
PENDING_FIRST_BUMP = frozenset({'crates/ui4me-host/host.manifest'})


def render(version: str, style: str) -> str:
    return f'{version}.0' if style == 'win4' else version


def source_version(root: Path = ROOT) -> str:
    text = (root / 'Cargo.toml').read_text(encoding='utf-8')
    match = re.search(FORCED['Cargo.toml'][0], text)
    if not match:
        raise ValueError('Cargo.toml has no [workspace.package] version')
    version = match.group(1)
    if not SEMVER.match(version):
        raise ValueError(f'Workspace version is not X.Y.Z: {version!r}')
    return version


def artifact_names(version: str | None = None) -> dict[str, str]:
    version = version or source_version()
    return {
        'sdist': f'ui4me_core-{version}.tar.gz',
        'linux_wheel': f'ui4me_core-{version}-py3-none-manylinux_2_34_x86_64.whl',
        'windows_wheel': f'ui4me_core-{version}-py3-none-win_amd64.whl',
        'sdist_root': f'ui4me_core-{version}',
        'psgallery_zip': f'UI4ME.Core-{version}.zip',
        'tag': f'v{version}',
    }


def declared(root: Path = ROOT) -> dict[str, str]:
    """Every forced location's declared value (or 'missing')."""
    found = {}
    for path, (pattern, _) in FORCED.items():
        try:
            match = re.search(pattern, (root / path).read_text(encoding='utf-8'))
        except OSError:
            match = None
        found[path] = match.group(1) if match else 'missing'
    lock = root / 'Cargo.lock'
    if lock.is_file():
        text = lock.read_text(encoding='utf-8')
        for crate in WORKSPACE_CRATES:
            match = re.search(r'(?m)^name = "' + re.escape(crate) + r'"\nversion = "([^"]+)"', text)
            found[f'Cargo.lock:{crate}'] = match.group(1) if match else 'missing'
    return found


def check_consistency(root: Path = ROOT, strict: bool = False) -> dict[str, tuple[str, str]]:
    """Locations whose declared version differs from the source: {location: (found, expected)}."""
    version = source_version(root)
    styles = {path: style for path, (_, style) in FORCED.items()}
    problems = {}
    for location, value in declared(root).items():
        expected = render(version, styles.get(location, 'plain'))
        if value != expected and (strict or location not in PENDING_FIRST_BUMP):
            problems[location] = (value, expected)
    return problems


HISTORICAL_PREFIXES = ('docs/audit/', 'docs/stabilization/', 'tools/release/records/',
                       'tools/release/testdata/', 'docs/release/RELEASE_NOTES_', 'CHANGELOG.md')
EXCLUDED_PREFIXES = ('patches/', 'third_party/', 'tools/release/SOURCE_FILES.sha256')


def category(path: str, text: str = '') -> str:
    if path in FORCED or path == 'Cargo.lock':
        # The version declaration itself; other mentions in the same file
        # (messages, comments) are ordinary code literals.
        return 'forced' if re.search(r'(?i)version', text) else 'code'
    if path.startswith(HISTORICAL_PREFIXES):
        return 'historical'
    if path.startswith('.github/') or (path.startswith('tools/release/') and not path.startswith('tools/release/qualification/')):
        return 'release-machinery'
    if '/tests/' in path or Path(path).name.startswith('test_'):
        return 'test'
    if path.startswith(('examples/', 'powershell/examples/')) or '/examples/' in path:
        return 'example'
    if path.endswith('.md'):
        return 'docs'
    return 'code'


def literal_occurrences(version: str | None = None, root: Path = ROOT) -> list[dict]:
    """Every tracked line containing the literal version, with a category.

    The inventory the lead uses for the version single-sourcing change. Uses
    ``git grep`` on tracked files, so build trees and local files never appear.
    """
    version = version or source_version(root)
    result = subprocess.run(['git', '-C', str(root), 'grep', '-n', '-I', '-F', '-e', version, '--', '.'],
                            capture_output=True, text=True, encoding='utf-8', errors='replace')
    if result.returncode not in (0, 1):
        raise RuntimeError(result.stderr.strip() or 'git grep failed')
    rows = []
    word = re.compile(r'(?<![\w.])' + re.escape(version) + r'(?![\w]|\.\d)')
    for line in result.stdout.splitlines():
        path, number, text = line.split(':', 2)
        if path.startswith(EXCLUDED_PREFIXES) or not word.search(text):
            continue
        rows.append({'path': path, 'line': int(number), 'category': category(path, text), 'text': text.strip()[:160]})
    return rows


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.split('\n\n', 1)[0])
    group = parser.add_mutually_exclusive_group()
    group.add_argument('--check', action='store_true', help='verify every forced location agrees')
    group.add_argument('--artifacts', action='store_true', help='print artifact file names as JSON')
    group.add_argument('--expect', metavar='X.Y.Z', help='exit 1 unless the source version equals this')
    parser.add_argument('--strict', action='store_true', help='with --check: include pending locations')
    args = parser.parse_args(argv)
    version = source_version()
    if args.check:
        problems = check_consistency(strict=args.strict)
        for location, (found, expected) in sorted(problems.items()):
            print(f'MISMATCH {location}: {found} (expected {expected})', file=sys.stderr)
        print(json.dumps({'version': version, 'result': 'FAIL' if problems else 'PASS'}))
        return 1 if problems else 0
    if args.artifacts:
        print(json.dumps(artifact_names(version), indent=2))
        return 0
    if args.expect is not None:
        if args.expect != version:
            print(f'Requested version {args.expect!r} does not equal the source version {version!r} '
                  '(Cargo.toml [workspace.package])', file=sys.stderr)
            return 1
        print(version)
        return 0
    print(version)
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
