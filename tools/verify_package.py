#!/usr/bin/env python3
"""Verify the delivered source manifest before importing or debugging the package.

The manifest excludes itself. Local Git metadata, build products and verification
logs are not package files. After source fixes the original manifest is expected
to differ; generate a new final record instead of weakening verification.
"""
import argparse
import hashlib
from pathlib import Path, PurePosixPath

ROOT = Path(__file__).resolve().parents[1]


def verify(root, manifest):
    failures, seen = [], set()
    for index, line in enumerate(manifest.read_text(encoding="utf-8").splitlines(), 1):
        if not line or line.startswith("#"):
            continue
        try:
            digest, name = line.split("  ", 1)
            relative = PurePosixPath(name)
            if (len(digest) != 64 or any(c not in "0123456789abcdef" for c in digest)
                    or not name or relative.is_absolute() or ".." in relative.parts
                    or "\\" in name or ":" in name or name in seen):
                raise ValueError("invalid digest, unsafe path or duplicate entry")
            seen.add(name)
            path = root.joinpath(*relative.parts)
            if not path.resolve().is_relative_to(root.resolve()):
                raise ValueError("path escapes package root")
            actual = hashlib.sha256(path.read_bytes()).hexdigest()
            if actual != digest:
                failures.append(f"CHANGED {name}")
        except (OSError, ValueError) as error:
            failures.append(f"line {index}: {error}")
    if not seen:
        failures.append("Manifest contains no files")
    return seen, failures


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=ROOT)
    parser.add_argument("--manifest", type=Path)
    args = parser.parse_args()
    manifest = args.manifest or args.root / "docs/SOURCE_FILES.sha256"
    try:
        files, failures = verify(args.root, manifest)
    except OSError as error:
        parser.exit(2, f"Cannot read source manifest: {error}\n")
    for failure in failures:
        print(failure)
    print(f"{'FAIL' if failures else 'PASS'}: {len(files)} source files checked")
    return 1 if failures else 0


if __name__ == "__main__":
    raise SystemExit(main())
