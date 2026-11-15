# F-09 / T7: an ordinary change can be green and lint always runs (2026-11-15)

## Baseline (1e49a6a)

* `python3 tools/release/refresh_source_manifest.py --check` on the clean base tree: exit 1, "Source manifest does not match the current reviewed tree" (the base commit only added `docs/audit/`; any change makes the committed manifest stale).
* Hence `tools/verify.py`'s default `python` stage failed on every non-release tree (`test_checked_manifest_matches_entire_source`), and in `ci.yml` the following `Lints` step had no `if:`, so clippy was skipped whenever the verifier failed (audit: PR #3/#4 runs show `Lints skipped`).
* `tools/test_public_docs.py` also failed on the base (6 retired-name hits, all in `docs/audit/findings/`).

## This branch

* The freeze check runs only with `UI4ME_RELEASE_MANIFEST_CHECK=1`: the publish preflights, `release-manifest.yml` (when `tools/release/**` changes) and `tools/verify.py --stage release-manifest`. `tools/test_ci_signal.py` proves it is skipped by default and executes when requested.
* `ci.yml` runs clippy with `if: ${{ !cancelled() }}` after the verifier; `tools/test_workflows.py::test_lint_runs_even_when_the_verifier_fails` asserts it structurally.
* The default `tools/verify.py --keep-going` passes on this branch, which is itself a non-release tree with a stale manifest (see the final report for the stage table).

## Final verifier run on this branch

`CARGO_BUILD_JOBS=4 python3 tools/verify.py --keep-going --jobs 4` → exit 0: fmt PASS 1 s, check PASS 60 s, test PASS 294 s (1094 Rust tests passed, 11 ignored), python PASS 26 s (189 tool tests, OK, 4 skipped), headless PASS, renderer PASS, faults PASS.
