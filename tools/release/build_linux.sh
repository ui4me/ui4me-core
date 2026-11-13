#!/usr/bin/env bash
# Build release source and binary wheel. This script never publishes packages.
set -euo pipefail
cd "$(dirname "$0")/../.."
command -v cargo >/dev/null || { echo 'Rust toolchain is required for compiled wheels' >&2; exit 2; }
python3 -m pip install --upgrade 'build>=1.2' 'setuptools>=77' 'wheel>=0.44' 'twine>=6' 'packaging>=24'
# Build the sdist, then the wheel FROM that sdist: proves the source archive
# alone is a complete build input (no checkout-only files).
python3 -m build --outdir dist
python3 -m twine check dist/*
python3 tools/release/verify_linux_artifact.py dist
cat <<'OUT'
Local wheels and source artifacts are in dist/; nothing was uploaded.
A locally tagged linux_x86_64 wheel is NOT a publishable manylinux wheel.
For PyPI, build using .github/workflows/release-linux.yml and upload the
repaired manylinux artifact after Linux release qualification succeeds.
OUT
