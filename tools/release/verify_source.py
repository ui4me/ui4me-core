"""Lightweight source integrity check before and after local qualification.

Every format-forced version location must equal the single source
(``Cargo.toml [workspace.package].version``, see ``version.py``), and the files
a release build needs must exist.
"""
from __future__ import annotations
from pathlib import Path
import hashlib
import json
import sys

sys.path.insert(0, str(Path(__file__).resolve().parent))
from version import ROOT, check_consistency, declared, source_version  # noqa: E402

REQUIRED = ['Cargo.lock', 'pyproject.toml', 'setup.py', 'MANIFEST.in', 'LICENSE',
            'third_party/README.md', 'crates/ui4me-host/src/host/bitmap.rs',
            'tools/release/build_linux.sh', 'tools/release/verify_linux_artifact.py',
            'python/ui4me_core/launcher.py', 'python/ui4me_core/live_bitmap.py',
            'examples/python/ui/live_bitmap.html', 'python/ui4me_core/_bootstrap.py',
            'tools/release/SOURCE_FILES.sha256', 'tools/release/version.py',
            'tools/release/collect_licenses.py', 'tools/release/smoke_installed_wheel.py',
            'CHANGELOG.md', 'SECURITY.md', 'docs/release/versioning.md']


def main() -> int:
    version = source_version()
    problems = check_consistency()
    if problems:
        raise SystemExit(f'Inconsistent final version (source {version}): {problems}')
    missing = [f for f in REQUIRED if not (ROOT / f).is_file()]
    if missing:
        raise SystemExit(f'Missing source files: {missing}')
    print(json.dumps({'result': 'PASS', 'version': version, 'versions': declared(),
                      'source_lock_sha256': hashlib.sha256((ROOT / 'Cargo.lock').read_bytes()).hexdigest(),
                      'required_files': len(REQUIRED)}, indent=2))
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
