#!/usr/bin/env python3
"""Refuse a duplicate upload: exit 1 if PyPI already has ui4me-core VERSION, 0 if it does not."""
from __future__ import annotations

import sys
import urllib.error
import urllib.request


def main(argv: list[str]) -> int:
    if len(argv) != 1:
        print('usage: check_pypi_absent.py VERSION', file=sys.stderr)
        return 2
    version = argv[0]
    url = f'https://pypi.org/pypi/ui4me-core/{version}/json'
    try:
        with urllib.request.urlopen(url, timeout=20):
            print(f'PyPI already has ui4me-core {version}; refusing duplicate upload', file=sys.stderr)
            return 1
    except urllib.error.HTTPError as error:
        if error.code == 404:
            print(f'PyPI has no ui4me-core {version} yet')
            return 0
        raise


if __name__ == '__main__':
    raise SystemExit(main(sys.argv[1:]))
