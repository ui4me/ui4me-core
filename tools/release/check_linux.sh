#!/usr/bin/env bash
# Full source gate on the local KDE workstation; never skips socket/process tests.
set -euo pipefail
cd "$(dirname "$0")/../.."
if [[ "$(uname -s)" != "Linux" ]]; then
  echo 'Linux release gate must run on Linux' >&2; exit 2
fi
export CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-2}"
export RUST_TEST_THREADS="${RUST_TEST_THREADS:-2}"
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked --workspace -- --test-threads="$RUST_TEST_THREADS"
python3 -m unittest discover -s tools -p 'test_*.py' -v
cargo build --locked --release -p ui4me-host
"$(pwd)/target/release/ui4me-host" --version
python3 tools/release/verify_source.py
printf 'Source gate passed. Native KDE and installed wheel tests are still separate requirements.\n'
