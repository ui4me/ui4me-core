#!/usr/bin/env bash
# owner: 006-style-engine (T048) - patch register UP-01 / UP-03 / UP-04 / UP-05 / UP-09 / UP-11 / UP-12 / UP-13 / UP-14 (engine/upstream-patches.md).
#
# Checks that patches/stylo/stylo/ is exactly servo/stylo @ 37c1ed3b707c8f333304de2107c13b68fbc1057c
# with P-006-1.patch, P-006-2.patch, P-006-3.patch, P-006-4.patch, P-006-5.patch, P-006-6.patch, P-006-7.patch, P-006-8.patch, P-006-9.patch and P-006-10.patch applied (`patch -p1`), so the patch files, the vendored tree that
# `[patch."https://github.com/servo/stylo"]` in Cargo.toml builds, and the upstream base cannot drift.
#
# The pristine tree comes from the cargo git checkout of the locked revision (or UI4ME_UPSTREAM_SOURCE;
# the revision is recorded in each patch header). Only files cargo does not ship to the
# vendored copy are ignored: .cargo-ok, .github and the upstream ignore file.
#
# Usage: patches/stylo/verify.sh      exit 0 = identical, 1 = drift, 2 = pristine source not found
set -u
SHA=37c1ed3b707c8f333304de2107c13b68fbc1057c
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
src="${UI4ME_UPSTREAM_SOURCE:-}"
if [ -n "$src" ] && [ ! -f "$src/Cargo.toml" ]; then
  echo "verify.sh: UI4ME_UPSTREAM_SOURCE is not an upstream source checkout" >&2
  exit 2
fi
for d in "${CARGO_HOME:-$HOME/.cargo}"/git/checkouts/stylo-*/"${SHA:0:7}"; do
  [ -n "$src" ] && break
  [ -f "$d/Cargo.toml" ] && src="$d" && break
done
if [ -z "$src" ]; then
  echo "verify.sh: no cargo checkout of servo/stylo @ $SHA (set UI4ME_UPSTREAM_SOURCE to a pristine checkout at this revision)" >&2
  exit 2
fi
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
cp -r "$src" "$work/stylo"
rm -rf "$work/stylo/.cargo-ok" "$work/stylo/.github"
rm -rf "$work"/stylo/.g*
for p in P-006-1 P-006-2 P-006-3 P-006-4 P-006-5 P-006-6 P-006-7 P-006-8 P-006-9 P-006-10 P-006-12 P-006-13 P-006-14 P-006-15 P-006-16 P-006-17 P-006-18 P-006-19; do
  (cd "$work/stylo" && patch -s -p1 -i "$here/$p.patch") || { echo "verify.sh: $p.patch does not apply to $SHA" >&2; exit 1; }
done
if diff -r "$work/stylo" "$here/stylo" >"$work/diff.txt"; then
  echo "verify.sh: OK - patches/stylo/stylo == servo/stylo@$SHA + P-006-1 + P-006-2 + P-006-3 + P-006-4 + P-006-5 + P-006-6 + P-006-7 + P-006-8 + P-006-9 + P-006-10 + P-006-12 + P-006-13 + P-006-14 + P-006-15 + P-006-16 + P-006-17 + P-006-18 + P-006-19"
else
  head -40 "$work/diff.txt" >&2
  echo "verify.sh: DRIFT between the vendored tree and base+patches" >&2
  exit 1
fi
