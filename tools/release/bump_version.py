#!/usr/bin/env python3
"""Set the UI4ME Core release version in every format-forced location.

    python tools/release/bump_version.py 1.0.2            # rewrite in place
    python tools/release/bump_version.py 1.0.2 --dry-run  # show what would change
    python tools/release/bump_version.py --list           # inventory of literals

Only the locations in ``version.FORCED`` and the workspace crates' Cargo.lock
entries are rewritten; everything else (workflows, verifiers) reads
``version.py``. ``--list`` prints every remaining tracked literal of the
current version by category, which is the work list for making docs, tests and
examples version-agnostic. Never commits, tags or publishes.
"""
from __future__ import annotations

import argparse
from collections import Counter
from pathlib import Path
import re
import sys

sys.path.insert(0, str(Path(__file__).resolve().parent))
import version as versioning  # noqa: E402


def replace_group(text: str, pattern: str, value: str) -> tuple[str, int]:
    count = 0

    def substitute(match: re.Match) -> str:
        nonlocal count
        count += 1
        start, end = match.span(1)
        whole = match.group(0)
        offset = match.start(0)
        return whole[:start - offset] + value + whole[end - offset:]
    return re.sub(pattern, substitute, text, count=1), count


def planned_changes(new: str, root: Path) -> dict[Path, tuple[str, str]]:
    if not versioning.SEMVER.match(new):
        raise ValueError(f'Version must be X.Y.Z without leading zeros: {new!r}')
    changes: dict[Path, tuple[str, str]] = {}
    for relative, (pattern, style) in versioning.FORCED.items():
        path = root / relative
        before = path.read_text(encoding='utf-8')
        after, count = replace_group(before, pattern, versioning.render(new, style))
        if count != 1:
            raise ValueError(f'{relative}: version pattern not found')
        if after != before:
            changes[path] = (before, after)
    lock = root / 'Cargo.lock'
    before = lock.read_text(encoding='utf-8')
    after = before
    for crate in versioning.WORKSPACE_CRATES:
        after, count = replace_group(after, r'(?m)^name = "' + re.escape(crate) + r'"\nversion = "([^"]+)"', new)
        if count != 1:
            raise ValueError(f'Cargo.lock: no entry for workspace crate {crate}')
    if after != before:
        changes[lock] = (before, after)
    return changes


def bump(new: str, root: Path = versioning.ROOT, dry_run: bool = False) -> list[str]:
    changes = planned_changes(new, root)
    if not dry_run:
        for path, (_, after) in changes.items():
            path.write_text(after, encoding='utf-8', newline='\n')
        problems = versioning.check_consistency(root, strict=True)
        if problems:
            raise RuntimeError(f'Version locations still disagree after the bump: {problems}')
    return sorted(str(p.relative_to(root)).replace('\\', '/') for p in changes)


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.split('\n\n', 1)[0])
    parser.add_argument('version', nargs='?')
    parser.add_argument('--dry-run', action='store_true')
    parser.add_argument('--list', action='store_true', help='list every tracked literal of the current version')
    args = parser.parse_args(argv)
    if args.list:
        rows = versioning.literal_occurrences()
        for row in rows:
            print(f"{row['category']:18} {row['path']}:{row['line']}: {row['text']}")
        counts = Counter(row['category'] for row in rows)
        files = len({row['path'] for row in rows})
        print(f'# {len(rows)} lines in {files} files: ' + ', '.join(f'{k} {v}' for k, v in sorted(counts.items())))
        return 0
    if not args.version:
        parser.error('a version is required (or --list)')
    try:
        changed = bump(args.version, dry_run=args.dry_run)
    except (OSError, ValueError, RuntimeError) as error:
        print(f'bump_version: {error}', file=sys.stderr)
        return 2
    verb = 'Would change' if args.dry_run else 'Changed'
    print(f'{verb} {len(changed)} files to {args.version}:')
    for name in changed:
        print('  ' + name)
    if not args.dry_run:
        print('Next: run tools/release/version.py --check --strict, review the diff, and update '
              'CHANGELOG.md and the release notes. Nothing was committed.')
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
