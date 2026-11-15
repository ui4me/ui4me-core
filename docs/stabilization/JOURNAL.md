# Stabilization engineering journal

Newest entries last. Each entry: what changed, why, how it was verified, commits.

## 2026-11-15 — Checkpoint 0: baseline and organisation

- Base: `main` @ `c6fd71d` (tag v1.0.1) — identical to the audited commit; no newer code to reconcile. Open draft
  PRs #3 and #4 are unmerged (PR #3 is not to be merged as written; PR #4 is reference material for F-07).
- Integration branch `stabilization/integration`; audit committed as `1e49a6a`.
- Baseline `python3 tools/verify.py --keep-going --jobs 8`: fmt, check, test (1,094 Rust tests), headless,
  renderer, faults PASS; python FAIL with exactly two failures:
  - `test_checked_manifest_matches_entire_source` — expected on any non-release tree (F-09);
  - `test_no_references_to_retired_documents` — caused by committing the audit, which quotes retired document
    names as history. Fixed in `135a224` by excluding `docs/audit/` and `docs/stabilization/` from that check only
    (link checks still apply).
- Tracker created (`tracker.json`, `MATRIX.md`, `tools/stabilization_tracker.py`, `tools/test_stabilization_tracker.py`).
- Workstreams A–F launched in parallel worktrees (see README). G and H wait for A/B to merge because they share
  host and renderer files.
