#!/usr/bin/env python3
"""Fail closed unless exactly one canonical UI4ME Core release triple is present.

Produces a source/wheel hash manifest for the GitHub release; does not upload.
"""
from __future__ import annotations

import argparse
import hashlib
from pathlib import Path

try:
    from .verify_linux_artifact import validate_sdist, validate_wheel as validate_linux
    from .verify_windows_artifact import validate_wheel as validate_windows
except ImportError:
    from verify_linux_artifact import validate_sdist, validate_wheel as validate_linux
    from verify_windows_artifact import validate_wheel as validate_windows

VERSION = '1.0.0'
EXPECTED = (
    f'ui4me_core-{VERSION}.tar.gz',
    f'ui4me_core-{VERSION}-py3-none-manylinux_2_34_x86_64.whl',
    f'ui4me_core-{VERSION}-py3-none-win_amd64.whl',
)


def validate(directory: Path) -> list[tuple[str, str]]:
    directory = directory.resolve(strict=True)
    files = [p for p in directory.iterdir() if p.is_file()]
    names = [p.name for p in files]
    if sorted(names) != sorted(EXPECTED):
        raise ValueError(f'Expected exactly {EXPECTED}; found {names}')
    reports = [
        validate_sdist(directory / EXPECTED[0]),
        validate_linux(directory / EXPECTED[1]),
        validate_windows(directory / EXPECTED[2]),
    ]
    if any(r['result'] != 'PASS' for r in reports):
        raise ValueError('A release artifact failed verification')
    return [(n, hashlib.sha256((directory / n).read_bytes()).hexdigest()) for n in EXPECTED]


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('directory', type=Path)
    parser.add_argument('--write-hashes', type=Path)
    args = parser.parse_args()
    try:
        result = validate(args.directory)
    except (ValueError, OSError) as exc:
        parser.exit(2, f'INVALID RELEASE: {exc}\n')
    manifest = ''.join(f'{digest}  {name}\n' for name, digest in result)
    if args.write_hashes:
        args.write_hashes.write_text(manifest, encoding='ascii')
    print(manifest, end='')
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
