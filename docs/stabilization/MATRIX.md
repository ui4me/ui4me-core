# Stabilization implementation matrix

Generated from [tracker.json](tracker.json) by `python3 tools/stabilization_tracker.py render`. Do not edit by hand.

## Consolidated findings

| ID | Sev. | Kind | WS | Status | Tests | Linux | Windows | Commits | Remaining risk |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| F-01 | Critical | runtime | B | IN_PROGRESS | T1, T6 | pending | pending |  |  |
| F-02 | Critical | runtime | A | IN_PROGRESS | T2, T3, T14 | pending | pending |  |  |
| F-03 | High | runtime | B | IN_PROGRESS | T1, T6 | pending | pending |  |  |
| F-04 | High | runtime | C | IN_PROGRESS | T4 | pending | pending |  |  |
| F-06 | Medium | runtime | B | IN_PROGRESS | T1, T6 | pending | pending |  |  |
| F-07 | High | runtime | D | IN_PROGRESS | document::geometry_tests (T5 incl. proptest) | Linux headless: cargo test renderer/host + verify.py (ws D); integration Python suite | pending | c712525 136a214 | Selection extents exact only for translations under scale/rotate. |
| F-08 | High | runtime | D | IN_PROGRESS | document::interact_tests (focus reveal) | Linux headless: cargo test renderer/host + verify.py (ws D); integration Python suite | pending | c4ba3ad 0d1f6d0 |  |
| F-09 | Medium | process | F | FIXED_VERIFIED | tools/test_ci_signal.py, test_workflows.py::test_lint_runs_even_when_the_verifier_fails | verify.py default stages green on non-release tree (ws F and integration python suite) | n/a | 099f238 026d68f |  |
| F-10 | High | process | F | FIXED_VERIFIED | tools/test_ci_gate.py (recorded API: d053e09 refused, c6fd71d accepted, missing refused) | unit tests + live read-only API run | n/a | 9240d35 126075e | Workflow wiring unrun on GitHub. |
| F-11 | Medium | process | F | IN_PROGRESS | test_release_artifacts.py | pending | pending | 5d1dc25 8aa7f4a 126075e |  |
| F-12 | Medium | runtime | G | OPEN | T3 | pending | pending |  |  |
| F-13 | Medium | runtime | E | IN_PROGRESS | T9 | pending | pending |  |  |
| F-14 | Medium | runtime | H | OPEN | T6 | pending | pending |  |  |
| F-15 | Medium | runtime | H | OPEN | T6 | pending | pending |  |  |
| F-18 | Medium | runtime | G | OPEN | T15 | pending | pending |  |  |
| F-19 | Medium | runtime | E | IN_PROGRESS | T12 | pending | pending |  |  |
| F-20 | Medium | process | F | NOT_REPRODUCED |  | gh api: visibility=public; PublicEvent 2026-11-12 by owner; 18/19 metadata URLs 200 (CHANGELOG 404 until on main) | pending | 37c0399 |  |
| F-21 | Medium | process | F | IN_PROGRESS | tools/test_version_source.py | pending | pending | d24fbe0 805e59e |  |
| F-22 | Medium | process | F | IN_PROGRESS | T11, T14 | pending | pending | 026d68f |  |
| F-23 | Medium | process | F | FIXED_VERIFIED | test_workflows.py, zizmor 0 / actionlint 0, cargo deny check | local zizmor, actionlint, cargo deny | n/a | 14b1ae3 026d68f 126075e | Windows binary unsigned (owner: certificate needed); attestations unrun. |
| F-24 | Medium | process | F | FIXED_VERIFIED | test_release_metadata.py | python suite | n/a | 37c0399 61c381b | Changelog 1.0.2 section to finalise; qualification records not yet generated from tools. |
| F-25 | Low-Medium | runtime | G | OPEN | T15 | pending | pending |  |  |
| F-26 | Low-Medium | runtime | E | IN_PROGRESS | T9 | pending | pending |  |  |
| F-27 | Low-Medium | runtime | F | IN_PROGRESS |  | pending | pending |  |  |
| F-28 | Medium | runtime | E | IN_PROGRESS | T9, T16 | pending | pending |  |  |
| F-29 | High | runtime | E | IN_PROGRESS | T16 | pending | pending |  |  |
| F-16a | High | runtime | C | IN_PROGRESS | T13 | pending | pending |  |  |
| F-16b | Medium | runtime | C+D | IN_PROGRESS | T13 | pending | pending |  |  |
| F-17 | High | runtime | D | IN_PROGRESS | document::interact_tests (link Enter, T10 matrix) | Linux headless: cargo test renderer/host + verify.py (ws D); integration Python suite | pending | 4b186e7 |  |
| F-17b | Medium | runtime | D | IN_PROGRESS | keyboard::shortcut_key tests, click common-ancestor tests | Linux headless: cargo test renderer/host + verify.py (ws D); integration Python suite | pending | efee6a2 a25f757 |  |

## Specialist findings

| ID | Sev. | Consolidated | WS | Status | Commits | Notes |
| --- | --- | --- | --- | --- | --- | --- |
| HOST-01 | High→Critical (F-02) | F-02 | A | OPEN |  |  |
| HOST-02 | High→advisory (§2.3) |  | G | IN_PROGRESS |  | Advisory about draft PR #3: resolved by not merging PR #3 and fixing F-02 without dropping (see F-02). |
| HOST-03 | Medium | F-18 | G | OPEN |  |  |
| HOST-04 | Low-Med | F-25 | G | OPEN |  |  |
| HOST-05 | Low-Med |  | G | OPEN |  |  |
| HOST-06 | Low | F-14 | H | OPEN |  |  |
| HOST-07 | Low-Med | F-25 | G | OPEN |  |  |
| HOST-08 | Low | F-25 | G | OPEN |  |  |
| HOST-09 | Low | F-02 | A | OPEN |  |  |
| HOST-10 | Low | F-25 | G | OPEN |  |  |
| HOST-11 | Low |  | G | OPEN |  |  |
| HOST-12 | Low |  | G | OPEN |  |  |
| HOST-13 | Low |  | G | OPEN |  |  |
| HOST-14 | Low |  | G | OPEN |  |  |
| HOST-15 | Low-Med | F-25 | G | OPEN |  |  |
| HOST-16 | Low |  | G | OPEN |  |  |
| HOST-17 | Low |  | G | OPEN |  |  |
| RENDER-01 | High | F-04 | C | OPEN |  |  |
| RENDER-02 | High→Critical (F-01) | F-01 | B | OPEN |  |  |
| RENDER-03 | Medium | F-06 | B | OPEN |  |  |
| RENDER-04 | Low | F-12 | G | OPEN |  |  |
| RENDER-05 | Medium→High (F-16a) | F-16a | C | OPEN |  |  |
| RENDER-06 | Medium | F-16b | C+D | OPEN |  |  |
| RENDER-07 | Medium | F-16b | C+D | OPEN |  |  |
| RENDER-08 | Low-Med |  | C | OPEN |  |  |
| RENDER-09 | Low |  | C | OPEN |  |  |
| RENDER-10 | Low (test gap) |  | C | IN_PROGRESS |  |  |
| RENDER-11 | Low |  | C | OPEN |  |  |
| RENDER-12 | Low-Med | F-27 | F | OPEN |  |  |
| RENDER-13 | Low |  | C | OPEN |  |  |
| INTERACT-01 | High | F-08 | D | IN_PROGRESS |  |  |
| INTERACT-02 | High | F-07 | D | IN_PROGRESS |  |  |
| INTERACT-03 | High | F-07 | D | IN_PROGRESS |  |  |
| INTERACT-04 | Medium | F-16b | C+D | IN_PROGRESS |  |  |
| INTERACT-05 | Medium→High (F-17) | F-17 | D | IN_PROGRESS |  |  |
| INTERACT-06 | Medium | F-17b | D | IN_PROGRESS |  |  |
| INTERACT-07 | Medium | F-17b | D | IN_PROGRESS |  |  |
| INTERACT-08 | Medium | F-02 | A | OPEN |  |  |
| INTERACT-09 | Low (Windows IME in password fields, not run) |  | D | DEFERRED |  | Deferred: patched winit-win32 ImeRequest::Disable semantics unverified; needs native Windows IME test (proposal: disable IME when ime.password on cfg(windows)). |
| INTERACT-10 | Low |  | D | IN_PROGRESS | 4738155 |  |
| INTERACT-11 | Low |  | D | OPEN |  |  |
| INTERACT-12 | Low |  | D | DEFERRED |  | Deferred: depends on native IM event ordering (Wayland text-input, IMM32); removing quiet window could reintroduce stale-commit bug. |
| INTERACT-13 | Low |  | D | DEFERRED |  | Deferred: SPI_GETWHEELSCROLLLINES changes Windows scroll speed; needs native verification. |
| INTERACT-14 | Low |  | D | OPEN |  |  |
| INTERACT-15 | Low |  | D | OPEN |  |  |
| BIND-01 | Medium | F-13 | E | OPEN |  |  |
| BIND-02 | Medium→High (F-29) | F-29 | E | OPEN |  |  |
| BIND-03 | Medium | F-13 | E | OPEN |  |  |
| BIND-04 | Medium | F-26 | E | OPEN |  |  |
| BIND-05 | Medium | F-26 | E | OPEN |  |  |
| BIND-06 | Low-Med | F-26 | E | OPEN |  |  |
| BIND-07 | Low-Med |  | E | OPEN |  |  |
| BIND-08 | Low-Med |  | E | OPEN |  |  |
| BIND-09 | Low-Med | F-26 | E | OPEN |  |  |
| BIND-10 | Low |  | E | OPEN |  |  |
| BIND-11 | Low |  | E | OPEN |  |  |
| BIND-12 | Low | F-22 | F | OPEN |  |  |
| BIND-13 | Low |  | E | OPEN |  |  |
| BIND-14 | Low |  | E | OPEN |  |  |
| BIND-15 | not confirmed (coordinator test) |  | E | NOT_REPRODUCED |  | Coordinator E12 on Windows Server 2022 (uv Python 3.13): no console window, no conhost. Evidence docs/audit/evidence/logs/win-pyw5-result.txt. |
| BIND-16 | Low |  | E | OPEN |  |  |
| BIND-17 | Medium | F-02 | A | OPEN |  |  |
| BIND-18 | Low |  | E | OPEN |  |  |
| BIND-19 | High (=F-02) | F-02 | A | OPEN |  |  |
| BIND-PS-01 | Medium | F-28 | E | OPEN |  |  |
| BIND-PS-02 | Medium (=F-29) | F-02, F-29 | A | OPEN |  |  |
| BIND-PS-03 | Medium-Low (=F-29) | F-29 | E | OPEN |  |  |
| BIND-PS-04 | Low-Med | F-28 | E | OPEN |  |  |
| BIND-PS-05 | Low-Med | F-28 | E | OPEN |  |  |
| BIND-PS-06 | Low-Med | F-28 | E | OPEN |  |  |
| BIND-PS-07 | Low | F-28 | E | OPEN |  |  |
| BIND-PS-08 | Low | F-28 | E | OPEN |  |  |
| BIND-PS-09 | Low | F-28 | E | OPEN |  |  |
| BIND-PS-10 | Low | F-28 | E | OPEN |  |  |
| BIND-PS-11 | Low | F-19 | E | OPEN |  |  |
| BIND-PS-12 | Low |  | E | OPEN |  |  |
| BIND-PS-13 | Low |  | E | OPEN |  |  |
| BIND-PS-14 | Low |  | E | OPEN |  |  |
| BIND-PS-15 | Low |  | E | OPEN |  |  |
| BIND-PS-16 | Low |  | E | OPEN |  |  |
| BIND-PS-17 | Low |  | E | OPEN |  |  |
| BIND-PS-18 | Low |  | E | OPEN |  |  |
| BIND-PS-19 | Low |  | E | OPEN |  |  |
| INPUTSEC-01 | High (=F-01) | F-01 | B | OPEN |  |  |
| INPUTSEC-02 | High (=F-03) | F-03 | B | OPEN |  |  |
| INPUTSEC-03 | Medium | F-14 | H | OPEN |  |  |
| INPUTSEC-04 | Medium | F-15 | H | OPEN |  |  |
| INPUTSEC-05 | Low-Med | F-14 | H | OPEN |  |  |
| INPUTSEC-06 | Medium | F-14 | H | OPEN |  |  |
| INPUTSEC-07 | High (=F-01) | F-01 | B | OPEN |  |  |
| INPUTSEC-08 | Low | F-23, F-27 | F | IN_PROGRESS |  |  |
| INPUTSEC-09 | Low-Med | F-27 | F | IN_PROGRESS |  |  |
| INPUTSEC-10 | Low |  | H | OPEN |  |  |
| INPUTSEC-11 | Low |  | H | OPEN |  |  |
| RELEASE-01 | High | F-10 | F | IN_PROGRESS |  |  |
| RELEASE-02 | High→Medium (F-09) | F-09 | F | IN_PROGRESS |  |  |
| RELEASE-03 | Medium | F-11 | F | IN_PROGRESS |  |  |
| RELEASE-04 | Medium | F-11 | F | IN_PROGRESS |  |  |
| RELEASE-05 | Medium | F-20 | F | NOT_REPRODUCED |  | Repository public since 2026-11-12; links resolve (see F-20). |
| RELEASE-06 | Medium | F-19 | E | OPEN |  |  |
| RELEASE-07 | Medium | F-21 | F | IN_PROGRESS |  |  |
| RELEASE-08 | Medium | F-22 | F | IN_PROGRESS |  |  |
| RELEASE-09 | Medium | F-23 | F | IN_PROGRESS |  |  |
| RELEASE-10 | Medium | F-24 | F | IN_PROGRESS |  |  |
| RELEASE-11 | Medium | F-24 | F | IN_PROGRESS |  |  |
| RELEASE-12 | Low-Med | F-10 | F | IN_PROGRESS |  |  |
| RELEASE-13 | Low |  | F | IN_PROGRESS |  |  |
| RELEASE-14 | Low |  | F | IN_PROGRESS |  |  |
| RELEASE-15 | Low |  | F | IN_PROGRESS |  |  |
| RELEASE-16 | Low |  | F | IN_PROGRESS |  |  |
| RELEASE-17 | Low |  | F | IN_PROGRESS |  |  |
| RELEASE-18 | Low |  | F | IN_PROGRESS |  |  |
| RELEASE-19 | Low-Med |  | F | IN_PROGRESS |  |  |

## Tests T1–T16

| ID | Status | Location | CI | Notes |
| --- | --- | --- | --- | --- |
| T1 | OPEN |  |  |  |
| T2 | OPEN |  |  |  |
| T3 | OPEN |  |  |  |
| T4 | OPEN |  |  |  |
| T5 | IN_PROGRESS | renderer document::geometry_tests |  |  |
| T6 | OPEN |  |  |  |
| T7 | IN_PROGRESS | tools/test_ci_signal.py, test_workflows.py |  |  |
| T8 | IN_PROGRESS | tools/test_ci_gate.py |  |  |
| T9 | OPEN |  |  |  |
| T10 | IN_PROGRESS | renderer document::interact_tests |  |  |
| T11 | IN_PROGRESS | ci.yml python matrix 3.10-3.14 + Pester (unrun on GitHub) |  |  |
| T12 | IN_PROGRESS | ci.yml clean-wheel + allusers jobs, verify-published.yml |  |  |
| T13 | OPEN |  |  |  |
| T14 | IN_PROGRESS | tools/windowed_smoke.py + nightly |  |  |
| T15 | OPEN |  |  |  |
| T16 | OPEN |  |  |  |

## Release gates

| Gate | Requirement | Baseline (1.0.1) | Current | Evidence |
| --- | --- | --- | --- | --- |
| R1 | No open Critical or High runtime defect | FAILING | FAILING |  |
| R2 | Documented limits render without abort on production stacks | FAILING | FAILING |  |
| R3 | 10,000-event bursts / 2,000 events/s for 60 s: no disconnect, no dropped control notification | FAILING | FAILING |  |
| R4 | Memory: counting-allocator soak <1 MiB; 30-min windowed soak <5 MiB/5% | FAILING | FAILING |  |
| R5 | Stalls bounded (pathological <250 ms or rejected; reference docs <100 ms; frame gap <50 ms draining) | FAILING | FAILING |  |
| R6 | Keyboard reachability/operability; focus scrolled into view | FAILING | FAILING |  |
| R7 | Geometry consistency across caret/selection/IME/AT/get_rect vs hit testing | FAILING | FAILING |  |
| R8 | AT contract checks automated + manual Orca/NVDA checklist | UNVERIFIED | UNVERIFIED |  |
| R9 | Backend crash/freeze/veto, signals, session end handled as documented | PARTIAL | PARTIAL |  |
| R10 | Bindings never die silently on user-code errors | FAILING | FAILING |  |
| P1 | Release commit has green required CI on every OS, enforced by publish workflow | FAILING | FAILING |  |
| P2 | Ordinary changes can be green; lint always runs | FAILING | FAILING |  |
| P3 | Publish requires green CI for the SHA plus manual dispatch | PARTIAL | PARTIAL |  |
| P4 | Published artifacts are exactly the qualified artifacts | MET Linux / FAILING Windows | MET Linux / FAILING Windows |  |
| P5 | Every package version traceable to its tag | MET PyPI / FAILING PSGallery 1.0.0 | MET PyPI / FAILING PSGallery 1.0.0 |  |
| P6 | Every claimed runtime tested in CI against the real host | FAILING | FAILING |  |
| P7 | Every documented install mode works | FAILING | FAILING |  |
| P8 | Dependency advisories and licences checked in CI | FAILING | FAILING |  |
| P9 | Workflows hardened (SHA-pinned, no template injection) | FAILING | FAILING |  |
| P10 | Provenance: PyPI attestations, build provenance, SBOM | PARTIAL | PARTIAL |  |
| P11 | Version single-sourced; a release does not edit CI | FAILING | FAILING |  |
| P12 | Changelog, release notes, compatibility/deprecation policy, SECURITY.md, yank procedure | FAILING | FAILING |  |
| P13 | Public metadata links resolve | FAILING | FAILING |  |
| P14 | MPL-2.0 source availability for modified Stylo | MET | MET |  |
| P15 | Fresh contributor can build and test from docs | MET | MET |  |
| P16 | Published artifacts work on clean machines | MET (audit) | MET (audit) |  |
| P17 | Vendored dependencies verifiably equal upstream plus patches | MET manually | MET manually |  |
| P18 | Secrets least-privilege | MET | MET |  |

Consolidated status counts: FIXED_VERIFIED 4, IN_PROGRESS 20, NOT_REPRODUCED 1, OPEN 5
