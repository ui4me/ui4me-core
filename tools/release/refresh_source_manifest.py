#!/usr/bin/env python3
"""Regenerate the complete source manifest for a reviewed Git checkout.

This is an explicit release-preparation action, NOT something the verifier runs
on failure. Recompute it only after source review, before committing the final
published tree, and then verify with ``tools/verify_package.py``.
"""
from __future__ import annotations

import argparse
import hashlib
from pathlib import Path
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[2]
MANIFEST = ROOT / 'docs' / 'SOURCE_FILES.sha256'
EXCLUDED_DIRS = {'.git', '.venv', '__pycache__', '.pytest_cache',
                 'node_modules', '.mypy_cache', '.ruff_cache'}
EXCLUDED_TOP_LEVEL = {'.local', 'target', 'build', 'dist', 'dist-manylinux', 'verification'}
EXCLUDED_SUFFIXES = {'.pyc', '.pyo', '.pem', '.key', '.secret'}
EXCLUDED_NAMES = {'.DS_Store', '.env'}


def git_visible_files(root: Path) -> set[str] | None:
    """Tracked plus untracked-but-not-ignored files, or None outside a Git work tree.

    This honours .gitignore and .git/info/exclude, so maintainer-local files
    (agent instructions, lab notes) never enter a manifest that a clean
    checkout of the committed tree must satisfy.
    """
    if not (root / '.git').exists():
        return None
    try:
        top = subprocess.run(['git', '-C', str(root), 'rev-parse', '--show-toplevel'],
                             capture_output=True, text=True, check=True).stdout.strip()
        if Path(top).resolve() != root.resolve():
            return None
        listed = subprocess.run(['git', '-C', str(root), 'ls-files', '-co', '--exclude-standard', '-z'],
                                capture_output=True, check=True).stdout
    except (OSError, subprocess.CalledProcessError):
        return None
    return {name for name in listed.decode('utf-8').split('\0') if name}


def included_source_files(root: Path = ROOT):
    visible = git_visible_files(root)
    for path in sorted(root.rglob('*')):
        if not path.is_file() and not path.is_symlink():
            continue
        rel = path.relative_to(root)
        if rel.as_posix() == 'docs/SOURCE_FILES.sha256':
            continue
        if visible is not None and rel.as_posix() not in visible:
            continue
        if rel.parts[0] in EXCLUDED_TOP_LEVEL:
            continue
        if any(x in EXCLUDED_DIRS or x.endswith('.egg-info') for x in rel.parts):
            continue
        if path.name in EXCLUDED_NAMES or path.suffix.lower() in EXCLUDED_SUFFIXES:
            continue
        if path.name.endswith(('.credentials.json', '.local.json', '.log')):
            continue
        if path.name.startswith('.env.') and path.name != '.env.example':
            continue
        if path.is_symlink():
            raise ValueError(f'Refusing symlinked source: {rel}')
        yield path


def build_manifest(root: Path = ROOT):
    rows = ['# UI4ME Core 1.0.0 public release source. This manifest excludes itself.']
    for file in included_source_files(root):
        rows.append(f'{hashlib.sha256(file.read_bytes()).hexdigest()}  {file.relative_to(root).as_posix()}')
    return '\n'.join(rows) + '\n'


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--check', action='store_true', help='Fail if the source manifest needs refreshing; never modify it')
    args = parser.parse_args(argv)
    current = build_manifest()
    if args.check:
        if not MANIFEST.is_file() or MANIFEST.read_text(encoding='utf-8') != current:
            print('Source manifest does not match the current reviewed tree', file=sys.stderr)
            return 1
        print(f'PASS: Source manifest matches ({current.count(chr(10)) - 1} files)')
        return 0
    MANIFEST.write_text(current, encoding='utf-8')
    print(f'Updated {MANIFEST.relative_to(ROOT)} ({current.count(chr(10)) - 1} files)')
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
