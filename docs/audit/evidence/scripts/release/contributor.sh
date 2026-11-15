#!/bin/sh
set -eu
start=$(date +%s)
export DEBIAN_FRONTEND=noninteractive
apt-get update -qq >/dev/null
apt-get install -y -qq build-essential pkg-config curl ca-certificates python3 libwayland-dev libxkbcommon-dev libdbus-1-dev git >/dev/null
echo "deps installed after $(( $(date +%s) - start ))s"
cp -a /src /work
cd /work
export CARGO_BUILD_JOBS=4 RUST_TEST_THREADS=4
bash tools/bootstrap_linux.sh 2>&1 | tail -5
echo "host built after $(( $(date +%s) - start ))s"
. "$HOME/.cargo/env"
./target/debug/ui4me-host --version
echo "=== python tool tests (no .git, as from an archive)"
python3 -m unittest discover -s tools -p 'test_*.py' 2>&1 | tail -25 || true
echo "=== git init + commit then source manifest check"
git init -q && git add -A && git -c user.email=a@b -c user.name=audit commit -qm import && python3 tools/release/refresh_source_manifest.py --check || true
echo "=== headless example"
timeout 120 python3 tools/run_example.py --headless --timeout 40 --app-id org.ui4me.AuditHeadless --evidence /tmp/ev.json 2>&1 | tail -3 || true
echo "total $(( $(date +%s) - start ))s"
