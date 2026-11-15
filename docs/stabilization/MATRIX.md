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
| F-07 | High | runtime | D | IN_PROGRESS | T5 | pending | pending |  |  |
| F-08 | High | runtime | D | IN_PROGRESS | T10 | pending | pending |  |  |
| F-09 | Medium | process | F | IN_PROGRESS | T7 | pending | pending |  |  |
| F-10 | High | process | F | IN_PROGRESS | T8 | pending | pending |  |  |
| F-11 | Medium | process | F | IN_PROGRESS | T12 | pending | pending |  |  |
| F-12 | Medium | runtime | G | OPEN | T3 | pending | pending |  |  |
| F-13 | Medium | runtime | E | IN_PROGRESS | T9 | pending | pending |  |  |
| F-14 | Medium | runtime | H | OPEN | T6 | pending | pending |  |  |
| F-15 | Medium | runtime | H | OPEN | T6 | pending | pending |  |  |
| F-18 | Medium | runtime | G | OPEN | T15 | pending | pending |  |  |
| F-19 | Medium | runtime | E | IN_PROGRESS | T12 | pending | pending |  |  |
| F-20 | Medium | process | F | IN_PROGRESS |  | pending | pending |  |  |
| F-21 | Medium | process | F | IN_PROGRESS |  | pending | pending |  |  |
| F-22 | Medium | process | F | IN_PROGRESS | T11, T14 | pending | pending |  |  |
| F-23 | Medium | process | F | IN_PROGRESS |  | pending | pending |  |  |
| F-24 | Medium | process | F | IN_PROGRESS |  | pending | pending |  |  |
| F-25 | Low-Medium | runtime | G | OPEN | T15 | pending | pending |  |  |
| F-26 | Low-Medium | runtime | E | IN_PROGRESS | T9 | pending | pending |  |  |
| F-27 | Low-Medium | runtime | F | IN_PROGRESS |  | pending | pending |  |  |
| F-28 | Medium | runtime | E | IN_PROGRESS | T9, T16 | pending | pending |  |  |
| F-29 | High | runtime | E | IN_PROGRESS | T16 | pending | pending |  |  |
| F-16a | High | runtime | C | IN_PROGRESS | T13 | pending | pending |  |  |
| F-16b | Medium | runtime | C+D | IN_PROGRESS | T13 | pending | pending |  |  |
| F-17 | High | runtime | D | IN_PROGRESS | T10 | pending | pending |  |  |
| F-17b | Medium | runtime | D | IN_PROGRESS | T10 | pending | pending |  |  |

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
| RENDER-10 | Low (test gap) |  | C | OPEN |  |  |
| RENDER-11 | Low |  | C | OPEN |  |  |
| RENDER-12 | Low-Med | F-27 | F | OPEN |  |  |
| RENDER-13 | Low |  | C | OPEN |  |  |
| INTERACT-01 | High | F-08 | D | OPEN |  |  |
| INTERACT-02 | High | F-07 | D | OPEN |  |  |
| INTERACT-03 | High | F-07 | D | OPEN |  |  |
| INTERACT-04 | Medium | F-16b | C+D | OPEN |  |  |
| INTERACT-05 | Medium→High (F-17) | F-17 | D | OPEN |  |  |
| INTERACT-06 | Medium | F-17b | D | OPEN |  |  |
| INTERACT-07 | Medium | F-17b | D | OPEN |  |  |
| INTERACT-08 | Medium | F-02 | A | OPEN |  |  |
| INTERACT-09 | Low (Windows IME in password fields, not run) |  | D | OPEN |  |  |
| INTERACT-10 | Low |  | D | OPEN |  |  |
| INTERACT-11 | Low |  | D | OPEN |  |  |
| INTERACT-12 | Low |  | D | OPEN |  |  |
| INTERACT-13 | Low |  | D | OPEN |  |  |
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
| INPUTSEC-08 | Low | F-23, F-27 | F | OPEN |  |  |
| INPUTSEC-09 | Low-Med | F-27 | F | OPEN |  |  |
| INPUTSEC-10 | Low |  | H | OPEN |  |  |
| INPUTSEC-11 | Low |  | H | OPEN |  |  |
| RELEASE-01 | High | F-10 | F | OPEN |  |  |
| RELEASE-02 | High→Medium (F-09) | F-09 | F | OPEN |  |  |
| RELEASE-03 | Medium | F-11 | F | OPEN |  |  |
| RELEASE-04 | Medium | F-11 | F | OPEN |  |  |
| RELEASE-05 | Medium | F-20 | F | OPEN |  |  |
| RELEASE-06 | Medium | F-19 | E | OPEN |  |  |
| RELEASE-07 | Medium | F-21 | F | OPEN |  |  |
| RELEASE-08 | Medium | F-22 | F | OPEN |  |  |
| RELEASE-09 | Medium | F-23 | F | OPEN |  |  |
| RELEASE-10 | Medium | F-24 | F | OPEN |  |  |
| RELEASE-11 | Medium | F-24 | F | OPEN |  |  |
| RELEASE-12 | Low-Med | F-10 | F | OPEN |  |  |
| RELEASE-13 | Low |  | F | OPEN |  |  |
| RELEASE-14 | Low |  | F | OPEN |  |  |
| RELEASE-15 | Low |  | F | OPEN |  |  |
| RELEASE-16 | Low |  | F | OPEN |  |  |
| RELEASE-17 | Low |  | F | OPEN |  |  |
| RELEASE-18 | Low |  | F | OPEN |  |  |
| RELEASE-19 | Low-Med |  | F | OPEN |  |  |

## Tests T1–T16

| ID | Status | Location | CI | Notes |
| --- | --- | --- | --- | --- |
| T1 | OPEN |  |  |  |
| T2 | OPEN |  |  |  |
| T3 | OPEN |  |  |  |
| T4 | OPEN |  |  |  |
| T5 | OPEN |  |  |  |
| T6 | OPEN |  |  |  |
| T7 | OPEN |  |  |  |
| T8 | OPEN |  |  |  |
| T9 | OPEN |  |  |  |
| T10 | OPEN |  |  |  |
| T11 | OPEN |  |  |  |
| T12 | OPEN |  |  |  |
| T13 | OPEN |  |  |  |
| T14 | OPEN |  |  |  |
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

Consolidated status counts: IN_PROGRESS 25, OPEN 5
