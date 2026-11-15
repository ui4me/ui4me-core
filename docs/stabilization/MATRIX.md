# Stabilization implementation matrix

Generated from [tracker.json](tracker.json) by `python3 tools/stabilization_tracker.py render`. Do not edit by hand.

## Consolidated findings

| ID | Sev. | Kind | WS | Status | Tests | Linux | Windows | Commits | Remaining risk |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| F-01 | Critical | runtime | B | FIXED_VERIFIED | crates/ui4me-renderer/tests/stack_depth.rs (T1), fuzz_gate::harness_depth_chains | T1 at half engine stack (16 MiB); host-level nested kwin: depth 40 @1 MiB ulimit, fieldset/table 250 pass (baseline abort) | Windows Server 2022 interactive RDP, release host b2a31a4: see docs/stabilization/evidence/lead/windows-integration-b2a31a4.md: PE reserve 32 MiB; depth 40/250 div and 250 fieldset pass; T1 passes natively | 96c549e 554b58d 5cb8e90 | inline-block/buttons/multicol exponential layout excluded from T1 until F-14 (ws H). File dialogs/DnD/UIA off the main thread need a native smoke. |
| F-02 | Critical | runtime | A | FIXED_VERIFIED | transport::t2_burst_to_a_slow_reading_peer_is_delivered_in_order_without_disconnect, peer_that_stops_reading_is_refused_at_the_byte_cap, peer_that_stops_reading_is_disconnected_by_the_write_deadline, each_frame_is_written_with_a_single_write_call, host::tests::refused_output_disconnects_once_and_counts_the_rest, document::event_queue_tests | Linux windowed (nested kwin): burst2 N=20 3/3, N=100 3/3 pass (baseline 1/3, 0/3); T2 passes, fails on baseline | Windows Server 2022 interactive RDP, release host b2a31a4: see docs/stabilization/evidence/lead/windows-integration-b2a31a4.md: burst2 N=20 and N=100 PROBE_PASS 6/6, no host warnings (baseline: disconnect at 40/100); host tests incl. T2 pass natively | c90062f 78df633 8c8cab3 eae0ad9 a664fb8 | Reader >8 MiB behind is still disconnected by design; Windows pipe quota unchanged. |
| F-03 | High | runtime | B | FIXED_VERIFIED | crates/ui4me-assets/tests/svg_depth.rs, fuzz_resources::harness_svg_depth | svg_depth tests; svgdepth 700 and 2,000 clip-path chain refused cleanly windowed (baseline abort) | Windows Server 2022 interactive RDP, release host b2a31a4: see docs/stabilization/evidence/lead/windows-integration-b2a31a4.md: svgdepth 500/700 pass; svg_depth passes natively | c8b9832 8806f21 | Conservative model may reject unusual valid SVGs (reference hop = one level; cycles rejected). |
| F-04 | High | runtime | C | FIXED_VERIFIED | text::fonts::environment::font_identity_tests, tests/font_identity_soak.rs (R4a) | soak +78.3 MiB baseline -> +95 KiB fixed; windowed nested kwin 900 s: baseline 58->296 MiB, fixed flat 44-55 MiB | native unit+soak pass; windowed replay: scenario does not reproduce on Windows baseline (flat), fixed flat (evidence/lead/windows-f04-a02d21a.md) | 7c111c8 | One heap copy per system font file remains (FontBlob/FontTable take bytes); content-number map never shrinks (one entry per distinct file). 30-min windowed soak R4b not yet automated. |
| F-06 | Medium | runtime | B | FIXED_VERIFIED | move_element_rejects_depth_over_limit, move_chains_cannot_exceed_the_limit, insert_html_counts_insertion_depth, a_rejected_move_rolls_back_its_batch | 6 unit tests (fail on baseline) | n/a | 7587eb3 | set_text under an element at depth 256 now refused (small behaviour change, documented). |
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
| F-16a | High | runtime | C | FIXED_VERIFIED | regression::append_cliff::append_after_twenty_unrelated_updates_shapes_at_most_two_paragraphs (T13) | 2,000 rows append after 20 updates: 691-750 ms -> 54-55 ms; shaped 2,001 -> 1 | append_cliff test passes natively | 3335e26 |  |
| F-16b | Medium | runtime | C+D | IN_PROGRESS | family_resolution_tests, truncated_generic_expansion_selects_the_same_fonts, a11y T13 counters (ws D) | cold render 828-930 ms -> 273-334 ms; a11y builds/100 ticks no AT 100 -> 0 | pending | c955e44 98bc678 |  |
| F-17 | High | runtime | D | IN_PROGRESS | document::interact_tests (link Enter, T10 matrix) | Linux headless: cargo test renderer/host + verify.py (ws D); integration Python suite | pending | 4b186e7 |  |
| F-17b | Medium | runtime | D | IN_PROGRESS | keyboard::shortcut_key tests, click common-ancestor tests | Linux headless: cargo test renderer/host + verify.py (ws D); integration Python suite | pending | efee6a2 a25f757 |  |

## Specialist findings

| ID | Sev. | Consolidated | WS | Status | Commits | Notes |
| --- | --- | --- | --- | --- | --- | --- |
| HOST-01 | High→Critical (F-02) | F-02 | A | FIXED_VERIFIED | c90062f 78df633 8c8cab3 eae0ad9 |  |
| HOST-02 | High→advisory (§2.3) |  | G | FIXED_VERIFIED | c90062f | PR #3 not merged; F-02 fixed without dropping. Advisory closed. |
| HOST-03 | Medium | F-18 | G | OPEN |  |  |
| HOST-04 | Low-Med | F-25 | G | OPEN |  |  |
| HOST-05 | Low-Med |  | G | OPEN |  |  |
| HOST-06 | Low | F-14 | H | OPEN |  |  |
| HOST-07 | Low-Med | F-25 | G | OPEN |  |  |
| HOST-08 | Low | F-25 | G | IN_PROGRESS | c90062f 78df633 8c8cab3 eae0ad9 |  |
| HOST-09 | Low | F-02 | A | FIXED_VERIFIED | c90062f 78df633 8c8cab3 eae0ad9 |  |
| HOST-10 | Low | F-25 | G | OPEN |  |  |
| HOST-11 | Low |  | G | OPEN |  |  |
| HOST-12 | Low |  | G | OPEN |  |  |
| HOST-13 | Low |  | G | OPEN |  |  |
| HOST-14 | Low |  | G | OPEN |  |  |
| HOST-15 | Low-Med | F-25 | G | OPEN |  |  |
| HOST-16 | Low |  | G | OPEN |  |  |
| HOST-17 | Low |  | G | OPEN |  |  |
| RENDER-01 | High | F-04 | C | FIXED_VERIFIED | 7c111c8 |  |
| RENDER-02 | High→Critical (F-01) | F-01 | B | FIXED_VERIFIED | 96c549e 554b58d |  |
| RENDER-03 | Medium | F-06 | B | FIXED_VERIFIED | 7587eb3 |  |
| RENDER-04 | Low | F-12 | G | OPEN |  |  |
| RENDER-05 | Medium→High (F-16a) | F-16a | C | FIXED_VERIFIED | 3335e26 |  |
| RENDER-06 | Medium | F-16b | C+D | FIXED_VERIFIED | c955e44 |  |
| RENDER-07 | Medium | F-16b | C+D | DEFERRED |  | Measured 26-27 ms per set_text at 2,000 rows (layout ~17, scene ~5, raster ~4-5). O(dirty) pipeline is roadmap 8.1, optional; postponed per 08 until measured need beyond R5(b) 100 ms budget, which is met. |
| RENDER-08 | Low-Med |  | C | DEFERRED |  | Full frame 8 ms @1280x800, ~20 ms @4K; damage-limited present is roadmap 8.3 (optional). |
| RENDER-09 | Low |  | C | OPEN |  |  |
| RENDER-10 | Low (test gap) |  | C | FIXED_VERIFIED | 8806f21 | Fuzz workers on half production stack + depth/SVG generators; nightly scheduled by ws F. |
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
| INPUTSEC-01 | High (=F-01) | F-01 | B | FIXED_VERIFIED | 96c549e 554b58d |  |
| INPUTSEC-02 | High (=F-03) | F-03 | B | FIXED_VERIFIED | c8b9832 |  |
| INPUTSEC-03 | Medium | F-14 | H | OPEN |  |  |
| INPUTSEC-04 | Medium | F-15 | H | OPEN |  |  |
| INPUTSEC-05 | Low-Med | F-14 | H | OPEN |  |  |
| INPUTSEC-06 | Medium | F-14 | H | OPEN |  |  |
| INPUTSEC-07 | High (=F-01) | F-01 | B | FIXED_VERIFIED | 554b58d c8b9832 |  |
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
| T1 | FIXED_VERIFIED | crates/ui4me-renderer/tests/stack_depth.rs, crates/ui4me-assets/tests/svg_depth.rs | cargo test (default); release in nightly | Native Windows pass. inline-block pending F-14. |
| T2 | FIXED_VERIFIED | crates/ui4me-host/src/transport.rs tests | cargo test (default) | Fails on baseline (message 236 refused). |
| T3 | OPEN |  |  |  |
| T4 | FIXED_VERIFIED | renderer font_identity_tests, tests/font_identity_soak.rs | cargo test (default) | Baseline: +78.3 MiB; fixed +95 KiB; native Windows pass |
| T5 | IN_PROGRESS | renderer document::geometry_tests |  |  |
| T6 | IN_PROGRESS | fuzz_gate.rs, fuzz_resources.rs (#[ignore]) | nightly (ws F) | Small-stack workers, depth/SVG generators; filter-cost generator pending ws H. |
| T7 | IN_PROGRESS | tools/test_ci_signal.py, test_workflows.py |  |  |
| T8 | IN_PROGRESS | tools/test_ci_gate.py |  |  |
| T9 | OPEN |  |  |  |
| T10 | IN_PROGRESS | renderer document::interact_tests |  |  |
| T11 | IN_PROGRESS | ci.yml python matrix 3.10-3.14 + Pester (unrun on GitHub) |  |  |
| T12 | IN_PROGRESS | ci.yml clean-wheel + allusers jobs, verify-published.yml |  |  |
| T13 | FIXED_VERIFIED | regression::append_cliff, a11y presence counters | cargo test (default) | append shapes 1 paragraph; 0 a11y builds per 100 ticks without AT |
| T14 | IN_PROGRESS | tools/windowed_smoke.py + nightly |  |  |
| T15 | OPEN |  |  |  |
| T16 | OPEN |  |  |  |

## Release gates

| Gate | Requirement | Baseline (1.0.1) | Current | Evidence |
| --- | --- | --- | --- | --- |
| R1 | No open Critical or High runtime defect | FAILING | FAILING |  |
| R2 | Documented limits render without abort on production stacks | FAILING | MET (pending inline-block, F-14) | T1 Linux+Windows; Windows windowed depth 250 div/fieldset, SVG 700 |
| R3 | 10,000-event bursts / 2,000 events/s for 60 s: no disconnect, no dropped control notification | FAILING | FAILING |  |
| R4 | Memory: counting-allocator soak <1 MiB; 30-min windowed soak <5 MiB/5% | FAILING | PARTIAL: R4a MET; R4b (30-min windowed soak) not automated | font_identity_soak; 900 s windowed Linux flat; Windows flat |
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

Consolidated status counts: FIXED_VERIFIED 10, IN_PROGRESS 14, NOT_REPRODUCED 1, OPEN 5
