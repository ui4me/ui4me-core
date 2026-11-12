#!/usr/bin/env bash
# owner: 008-layout-engine (L-FLOAT-r10) - patch register UP-06 (engine/upstream-patches.md).
#
# Checks that patches/taffy/taffy/ is exactly DioxusLabs/taffy @ fca167cf659b8bf48019d0dea90624109a78103f
# (the files the vendored copy keeps: Cargo.toml, README.md, LICENSE, src/, examples/ and
# benches/dummy_benchmark.rs) with P-008-1.patch … P-008-6.patch applied (`patch -p1`), so the patch file, the vendored tree that
# `[patch."https://github.com/DioxusLabs/taffy"]` in Cargo.toml builds, and the upstream base cannot drift.
#
# The pristine tree comes from the cargo git checkout of the locked revision (or UI4ME_UPSTREAM_SOURCE;
# the revision is recorded in the patch header).
#
# Usage: patches/taffy/verify.sh      exit 0 = identical, 1 = drift, 2 = pristine source not found
set -u
SHA=fca167cf659b8bf48019d0dea90624109a78103f
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
src="${UI4ME_UPSTREAM_SOURCE:-}"
if [ -n "$src" ] && [ ! -f "$src/Cargo.toml" ]; then
  echo "verify.sh: UI4ME_UPSTREAM_SOURCE is not an upstream source checkout" >&2
  exit 2
fi
for d in "${CARGO_HOME:-$HOME/.cargo}"/git/checkouts/taffy-*/"${SHA:0:7}"; do
  [ -n "$src" ] && break
  [ -f "$d/Cargo.toml" ] && src="$d" && break
done
if [ -z "$src" ]; then
  echo "verify.sh: no cargo checkout of DioxusLabs/taffy @ $SHA (set UI4ME_UPSTREAM_SOURCE to a pristine checkout at this revision)" >&2
  exit 2
fi
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
mkdir -p "$work/taffy/benches"
cp -r "$src/src" "$src/examples" "$work/taffy/"
cp "$src/Cargo.toml" "$src/README.md" "$src/LICENSE" "$work/taffy/"
cp "$src/benches/dummy_benchmark.rs" "$work/taffy/benches/"
for p in P-008-1 P-008-2 P-008-3 P-008-4 P-008-5 P-008-6; do (cd "$work/taffy" && patch -s -p1 -i "$here/$p.patch") || { echo "verify.sh: $p.patch does not apply to $SHA" >&2; exit 1; }; done
if diff -r "$work/taffy" "$here/taffy" >"$work/diff.txt"; then
  echo "verify.sh: OK - patches/taffy/taffy == DioxusLabs/taffy@$SHA + P-008-1 + P-008-2 + P-008-3 + P-008-4 + P-008-5 + P-008-6"
else
  head -40 "$work/diff.txt" >&2
  echo "verify.sh: DRIFT between the vendored tree and base+patch" >&2
  exit 1
fi
