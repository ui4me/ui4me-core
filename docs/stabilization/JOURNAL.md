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

## 2026-11-15 — Integration of workstreams D, F and A

- **D merged** (`379ec1f`): geometry service (F-07), focus into view (F-08), Enter on links (F-17), physical-key
  shortcuts and common-ancestor click (F-17b), a11y rebuild gating (INTERACT-04), pointer cancel on focus loss
  (INTERACT-10). INTERACT-09/12/13 deferred (native-only evidence needed). Event-order changes documented in
  `docs/reference/protocol.md`.
- **F merged**: manifest check opt-in (F-09), CI gate for publishing (F-10), stack-reserve artifact check and RC
  qualification tooling (F-11 interim), version tooling (F-21, bump pending), CI matrix/Pester/clean-machine/
  nightly (F-22, unrun on GitHub), pinning/deny/SBOM/attestations (F-23), Beta classifier and policies (F-24).
  Python tool suite on integration: 193 tests OK, 4 skipped.
- **Correction to the audit (F-20):** the repository is public (owner `PublicEvent` 2026-11-12, before the audit);
  18/19 metadata URLs return 200. F-20 recorded NOT_REPRODUCED.
- **A merged**: byte-bounded outbound queue, single-write frames, log once (F-02, HOST-01/08/09). Linux windowed
  `burst2.py` N=100: 3/3 pass (baseline 0/3).
- **New defect found during integration:** at N=100 the backend received exactly 1,024 events per scheme switch.
  The renderer's per-turn event queue (`MAX_EVENTS = 1024`) refused later events silently, including
  `link_activated`. Fixed in `a664fb8` (16,384 per turn — R3 asks for 10,000 — refusals counted and logged by the
  host). Test `document::event_queue_tests` fails with the old cap.
