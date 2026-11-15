#!/usr/bin/env bash
# Verify every vendored patch stack (patches/*/verify.sh) against a fresh
# upstream clone at the pinned revision (P17 / RELEASE-19). Used by the nightly
# workflow; runs locally too. Never modifies the repository.
#
# Each patches/<name>/verify.sh names its upstream as "is exactly <owner>/<repo> @ <sha>"
# and pins SHA=<sha>; this script clones https://github.com/<owner>/<repo>
# (blobless), checks out that SHA and runs the script with UI4ME_UPSTREAM_SOURCE.
# Exit 0 only if every stack verifies.
set -euo pipefail
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
status=0
for script in "$root"/patches/*/verify.sh; do
  name="$(basename "$(dirname "$script")")"
  sha="$(sed -n 's/^SHA=\([0-9a-f]\{40\}\)$/\1/p' "$script")"
  repo="$(sed -n 's/^# Checks that .* is exactly \([A-Za-z0-9_.-]*\/[A-Za-z0-9_.-]*\) @ [0-9a-f]\{40\}$/\1/p' "$script")"
  if [[ -z "$sha" || -z "$repo" ]]; then
    echo "verify_patches: $script does not declare its upstream repository and SHA" >&2
    status=1
    continue
  fi
  echo "== $name: $repo @ $sha"
  git clone --quiet --filter=blob:none --no-checkout "https://github.com/$repo.git" "$work/$name"
  git -C "$work/$name" -c advice.detachedHead=false checkout --quiet "$sha"
  if UI4ME_UPSTREAM_SOURCE="$work/$name" bash "$script"; then
    echo "== $name: OK"
  else
    echo "== $name: FAILED" >&2
    status=1
  fi
done
exit "$status"
