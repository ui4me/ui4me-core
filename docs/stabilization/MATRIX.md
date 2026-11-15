# Stabilization implementation matrix

Generated from [tracker.json](tracker.json) by `python3 tools/stabilization_tracker.py render`. Do not edit by hand.

## Consolidated findings

| ID | Sev. | Kind | WS | Status | Tests | Linux | Windows | Commits | Remaining risk |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| F-01 | Critical | runtime | B | FIXED_VERIFIED | crates/ui4me-renderer/tests/stack_depth.rs (T1), fuzz_gate::harness_depth_chains | T1 at half engine stack (16 MiB); host-level nested kwin: depth 40 @1 MiB ulimit, fieldset/table 250 pass (baseline abort) | Windows Server 2022 interactive RDP, release host b2a31a4: see docs/stabilization/evidence/lead/windows-integration-b2a31a4.md: PE reserve 32 MiB; depth 40/250 div and 250 fieldset pass; T1 passes natively | 96c549e 554b58d 5cb8e90 | inline-block/buttons/multicol exponential layout excluded from T1 until F-14 (ws H). File dialogs/DnD/UIA off the main thread need a native smoke. |
| F-02 | Critical | runtime | A | FIXED_VERIFIED | transport::t2_burst_to_a_slow_reading_peer_is_delivered_in_order_without_disconnect, peer_that_stops_reading_is_refused_at_the_byte_cap, peer_that_stops_reading_is_disconnected_by_the_write_deadline, each_frame_is_written_with_a_single_write_call, host::tests::refused_output_disconnects_once_and_counts_the_rest, document::event_queue_tests | Linux windowed (nested kwin): burst2 N=20 3/3, N=100 3/3 pass (baseline 1/3, 0/3); T2 passes, fails on baseline | Windows Server 2022 interactive RDP, release host b2a31a4: see docs/stabilization/evidence/lead/windows-integration-b2a31a4.md: burst2 N=20 and N=100 PROBE_PASS 6/6, no host warnings (baseline: disconnect at 40/100); host tests incl. T2 pass natively | c90062f 78df633 8c8cab3 eae0ad9 a664fb8 | Reader >8 MiB behind is still disconnected by design; Windows pipe quota unchanged. |
| F-03 | High | runtime | B | FIXED_VERIFIED | crates/ui4me-assets/tests/svg_depth.rs, fuzz_resources::harness_svg_depth, svg_depth::self_references_usvg_resolves_are_accepted, svg_depth::paint_inherited_through_use_is_counted_per_instantiated_shape | svg_depth tests; svgdepth 700 and 2,000 clip-path chain refused cleanly windowed (baseline abort) | Windows Server 2022 interactive RDP, release host b2a31a4: see docs/stabilization/evidence/lead/windows-integration-b2a31a4.md: svgdepth 500/700 pass; svg_depth passes natively | c8b9832 8806f21 0836c0c b420e8b | A pattern that references itself through an explicit fill is still refused although usvg rewrites it to none. Other cycles follow usvg 0.49 semantics. |
| F-04 | High | runtime | C | FIXED_VERIFIED | text::fonts::environment::font_identity_tests, tests/font_identity_soak.rs (R4a) | soak +78.3 MiB baseline -> +95 KiB fixed; windowed nested kwin 900 s: baseline 58->296 MiB, fixed flat 44-55 MiB | native unit+soak pass; windowed replay: scenario does not reproduce on Windows baseline (flat), fixed flat (evidence/lead/windows-f04-a02d21a.md) | 7c111c8 | One heap copy per system font file remains (FontBlob/FontTable take bytes); content-number map never shrinks (one entry per distinct file). 30-min windowed soak R4b not yet automated. |
| F-06 | Medium | runtime | B | FIXED_VERIFIED | move_element_rejects_depth_over_limit, move_chains_cannot_exceed_the_limit, insert_html_counts_insertion_depth, a_rejected_move_rolls_back_its_batch, parser_corpus::capped_and_uncapped_parses_agree_at_the_node_limit, parse::sink::tests::comments_count_toward_the_node_stop | 6 unit tests (fail on baseline) | n/a | 7587eb3 8b7c343 225ad5c | set_text under an element at depth 256 now refused (small behaviour change, documented). |
| F-07 | High | runtime | D | FIXED_VERIFIED | document::geometry_tests (T5 incl. proptest) | Native KDE Plasma 6.6 Wayland on linuxDEV (ydotool input, AT-SPI, screenshots): PASS (evidence/n-native-linux/README.md): AT-SPI extents within 2 px of pixels for position:fixed, translate and scale(1.5) ancestors; caret hit-testing; fcitx5 candidate window at the caret | Renderer geometry tests pass on winDEV (553/553); caret click placement exercised natively only on Linux. Windows IME candidate placement and UIA caret bounds NOT exercised natively. | c712525 136a214 | Selection extents exact only for translations under scale/rotate. Windows IME/UIA caret bounds and rotation/non-1.0 scale factors not verified natively; ibus not tested. |
| F-08 | High | runtime | D | FIXED_VERIFIED | document::interact_tests (focus reveal) | Native KDE Plasma 6.6 Wayland on linuxDEV (ydotool input, AT-SPI, screenshots): PASS (evidence/n-native-linux/README.md): Tab/AT-SPI grabFocus/backend focus scroll off-screen targets into view, extents inside viewport | Native Windows (SendInput scan codes + UIA, interactive session): fails on 1.0.1, passes on integration (evidence/lead/windows-native-interaction.md) | c4ba3ad 0d1f6d0 | Focus reveal inside a position:fixed scroll container not exercised natively. Deferred (renderer review #6): scroll_into_view under a scaled or rotated scroll container overshoots (2x at scale(2)) or scrolls the wrong axis. |
| F-09 | Medium | process | F | FIXED_VERIFIED | tools/test_ci_signal.py, test_workflows.py::test_lint_runs_even_when_the_verifier_fails | verify.py default stages green on non-release tree (ws F and integration python suite) | n/a | 099f238 026d68f |  |
| F-10 | High | process | F | FIXED_VERIFIED | tools/test_ci_gate.py (recorded API: d053e09 refused, c6fd71d accepted, missing refused) | unit tests + live read-only API run | n/a | 9240d35 126075e | Workflow wiring unrun on GitHub. |
| F-11 | Medium | process | F | BLOCKED | test_release_artifacts.py | qualify_windows_rc --allow-non-windows tool tests (tools/test_qualification_tools.py) PASS | Local 1.0.2 RC wheel (sha256 2c2dca51...e926c1, from bba9e6d sdist) qualified natively: first frame, depth 250, svgdepth 700, burst 100x6 PASS (evidence/lead/windows-rc-1.0.2.md) | 5d1dc25 8aa7f4a 126075e | The CI-built wheel will have a different hash unless the MSVC build is reproducible; it must be qualified again. |
| F-12 | Medium | runtime | G | FIXED_VERIFIED | host::headless::tests, tools/test_headless_clock.py (T3) | T3 burst passes (baseline -32601); RSS growth 10k writes 2,460 -> 136 KiB | T3 transition burst and R5(c) pass natively (evidence/lead/windows-lifecycle-81b8c85.md) | 85c817f | Unsubscribed-type assertion needs host-side subscription (3.1, not done). Headless CPU use higher while transitions run. |
| F-13 | Medium | runtime | E | FIXED_VERIFIED | tools/test_binding_robustness.py CallbackRobustnessTests (T9), real-host sys.exit() in close policy test | Linux: verify.py all stages PASS at 29f9f8d (verify-integ6); Pester 72/72 real host on pwsh 7.7; Python suites on 3.10/3.13/3.14/3.14t | Python suites incl. CallbackRobustnessTests pass on 3.10 and 3.13 (evidence/lead/windows-bindings-29f9f8d.md) | 0922dcf | Policy fail-safe: exception or None return keeps the window open (warned). |
| F-14 | Medium | runtime | H | FIXED_VERIFIED | crates/ui4me-renderer/tests/content_stalls.rs, crates/ui4me-assets/tests/svg_filter_cost.rs, fuzz_resources::harness_svg_filters (T6), chunked_parsing_builds_the_same_tree (html5lib corpus), stack_depth inline-block/button (T1), document::paged_tests::paginating_huge_or_deep_flows_is_bounded | verify.py all stages PASS (verify-integ5, cd4a12e); release renderer 546 tests; fuzz depth 120 s/151 cases and svg_filters 90 s/1476 cases 0 findings. Before/after: 26 nested inline-blocks 7.7 s->0.2 s; 40k unclosed div 5.0 s->2 ms; 4,000 inline inputs 17.0 s->0.95 s; feTurbulence 46.8 s->0.2 s (evidence/h-stalls/) | Windows release (winDEV, cd4a12e): content_stalls 13/13, svg_filter_cost 5/5, stack_depth 8/8 incl. inline-block/button, renderer lib 553/553 (evidence/lead/windows-integration-cd4a12e.md) | 467bb7d b20efc5 268de65 c526a94 ecf3504 ec4a764 082a27b | Layout still O(atomic depth x subtree) in places (~53 ms at 250 deep); final-layout reuse relies on all layout runs going through cache_store (new invariant). SVG budget is per raster, not per frame. CSS filter estimate assumes scale 1 (HiDPI cost x scale^2). Parse just under the cap up to the node limit ~0.27 s. |
| F-15 | Medium | runtime | H | FIXED_VERIFIED | content_stalls F-15 tests (gradient stops, filter functions, mask layers, scope depth) | four F-15 tests fail on baseline with the audit's errors, pass after; 64 x blur(50px) 6.2 s->0.19 s; verify PASS | Windows release (winDEV, cd4a12e): content_stalls 13/13, svg_filter_cost 5/5, stack_depth 8/8 incl. inline-block/button, renderer lib 553/553 (evidence/lead/windows-integration-cd4a12e.md) | ecf3504 | CSS filter estimate underestimates many small filtered layers (~3x for 50 shadowed cards). |
| F-18 | Medium | runtime | G | FIXED_VERIFIED | platform::linux::tests::supervised_environment_skips_what_systemd_rejects, test_host_lifecycle::test_t15_supervised_start_with_rejected_environment | real systemd-run; baseline: Cannot assign environment variable BASH_FUNC_x%% | n/a | 3dc57a7 |  |
| F-19 | Medium | runtime | E | FIXED_VERIFIED | Unit.Tests.ps1 BIND-PS-11 (PosixOnly), evidence/e-bindings/ps-f19-allusers-container.sh | Docker mcr.microsoft.com/powershell: AllUsers install as root, run as unprivileged user: baseline throws, fixed runs headless app (evidence/e-bindings/ps-f19-allusers-container.md) | n/a (Not applicable on Windows (no execute bit); verified the chmod/cache path is unreachable on Windows (Get-UI4MEHostPath guard) and the test is excluded there) | 9827c3d | Cache under XDG_CACHE_HOME grows one copy per version/hash. |
| F-20 | Medium | process | F | NOT_REPRODUCED |  | gh api: visibility=public; PublicEvent 2026-11-12 by owner; 18/19 metadata URLs 200 (CHANGELOG 404 until on main) | pending | 37c0399 |  |
| F-21 | Medium | process | F | FIXED_VERIFIED | tools/test_version_source.py, tools/test_release_metadata.py | bump_version.py 1.0.2 rewrote the 8 forced locations; version.py --check --strict PASS; test_version_source PASS | RC wheel ui4me_core-1.0.2 built from the 1.0.2 sdist on winDEV and qualified (evidence/lead/windows-rc-1.0.2.md) | d24fbe0 805e59e bba9e6d 76a094b |  |
| F-22 | Medium | process | F | BLOCKED | T11, T14 | local equivalents PASS | local equivalents PASS (evidence/lead/windows-bindings-29f9f8d.md) | 026d68f 73e99d6 |  |
| F-23 | Medium | process | F | FIXED_VERIFIED | test_workflows.py, zizmor 0 / actionlint 0, cargo deny check | local zizmor, actionlint, cargo deny | n/a | 14b1ae3 026d68f 126075e | Windows binary unsigned (owner: certificate needed); attestations unrun. |
| F-24 | Medium | process | F | FIXED_VERIFIED | test_release_metadata.py | python suite | n/a | 37c0399 61c381b | Changelog 1.0.2 section to finalise; qualification records not yet generated from tools. |
| F-25 | Low-Medium | runtime | G | FIXED_VERIFIED | AcceptLoopTests, TerminationSignalTests, RelaunchTests, PolicyAnswerTests, host::tests::error_answers_to_policy_requests_are_no_decision | all T15 tests pass; each fails on baseline; launcher SIGINT/SIGTERM exit 130/143 with on_shutdown | host lifecycle tests pass; Ctrl+Break direct start: Python exit 130 + on_shutdown; PowerShell 5.1/7 exit 130, UntilShutdown returns, no debugger prompt (evidence/lead/windows-bindings-29f9f8d.md) | 7813ef8 92977d2 1d2753a 2c1233a 0adab8c f5b3fb7 92a7ea1 334ede8 | WM_ENDSESSION (logoff/shutdown) not exercised natively; KillMode=mixed and HOST-12 SID check deferred (G). Ctrl+C typed in a real console not exercised (CTRL_BREAK via API only). Deferred (host review #6, #7): named-pipe server recreate after a failed instance; Win32 posted-message starvation under a message flood. |
| F-26 | Low-Medium | runtime | E | FIXED_VERIFIED | tools/test_bootstrap.py, tools/test_launcher.py, tools/test_application.py | Linux: verify.py all stages PASS at 29f9f8d (verify-integ6); Pester 72/72 real host on pwsh 7.7; Python suites on 3.10/3.13/3.14/3.14t | BIND-04 on Windows: real host True, pip shims False; bootstrap/launcher tests pass on 3.10 and 3.13 (evidence/lead/windows-bindings-29f9f8d.md) | c3e1cca 0922dcf | BIND-05 (lossy print relay through the 512-line log ring) DEFERRED: needs a host option (stdio=inherit) or a lossless relay. |
| F-27 | Low-Medium | runtime | F | DEFERRED |  | pending | pending | 14b1ae3 |  |
| F-28 | Medium | runtime | E | FIXED_VERIFIED | powershell/tests/Callbacks.Tests.ps1, Unit.Tests.ps1, DirectStart.Tests.ps1 | Linux: verify.py all stages PASS at 29f9f8d (verify-integ6); Pester 72/72 real host on pwsh 7.7; Python suites on 3.10/3.13/3.14/3.14t | Pester real host 70/70 on 5.1 and 7.4.6 (evidence/lead/windows-bindings-29f9f8d.md) | 916b1d7 df14e52 334ede8 | PS-09 strict [bool] policy is a documented behaviour change from 1.0.1. Launcher ends with [Environment]::Exit (skips PowerShell exit handlers such as transcripts). |
| F-29 | High | runtime | E | FIXED_VERIFIED | tools/test_binding_robustness.py ControlNotificationTests (T16), powershell/tests/Delivery.Tests.ps1 | Linux: verify.py all stages PASS at 29f9f8d (verify-integ6); Pester 72/72 real host on pwsh 7.7; Python suites on 3.10/3.13/3.14/3.14t; e6 overflow probe: 191 dropped + closed lost -> 0 control dropped, window retired (evidence/e-bindings/py-e6-overflow.md) | PS 5.1 and 7 windowed burst N=100 x 6 rounds: PROBE_PASS, droppedControl=0, droppedDiscrete=0; Delivery.Tests pass on both; Python suites pass (evidence/lead/windows-bindings-29f9f8d.md) | 0922dcf b6b2d86 df14e52 | Control messages unbounded by design (thousands of document.error possible). Interest filtering drops events that arrive before their handler is registered; replacing the document.event handler via rpc.on_notification disables the filter. PowerShell delivers only ~8-20% of a 10,800-event transition burst to callbacks (coalescible, counted). |
| F-16a | High | runtime | C | FIXED_VERIFIED | regression::append_cliff::append_after_twenty_unrelated_updates_shapes_at_most_two_paragraphs (T13) | 2,000 rows append after 20 updates: 691-750 ms -> 54-55 ms; shaped 2,001 -> 1 | append_cliff test passes natively | 3335e26 |  |
| F-16b | Medium | runtime | C+D | FIXED_VERIFIED | family_resolution_tests, truncated_generic_expansion_selects_the_same_fonts, a11y T13 counters (ws D) | cold render 828-930 ms -> 273-334 ms; a11y builds/100 ticks no AT 100 -> 0 | renderer lib 553/553 and host tests on winDEV; UIA tree correct in the engine-thread smoke (evidence/lead/windows-engine-thread-smoke.md) | c955e44 98bc678 | RENDER-07: O(document) set_text (~27 ms at 2,000 rows) remains; RENDER-09 insert_html validation O(n) (deferred). |
| F-17 | High | runtime | D | FIXED_VERIFIED | document::interact_tests (link Enter, T10 matrix) | Native KDE Plasma 6.6 Wayland on linuxDEV (ydotool input, AT-SPI, screenshots): PASS (evidence/n-native-linux/README.md): Enter on link gives keydown, click, link_activated | Native Windows (SendInput scan codes + UIA, interactive session): fails on 1.0.1, passes on integration (evidence/lead/windows-native-interaction.md) | 4b186e7 |  |
| F-17b | Medium | runtime | D | FIXED_VERIFIED | keyboard::shortcut_key tests, click common-ancestor tests, interact_tests::prevent_default_shortcut_rules_ignore_altgr_characters, interact_tests::a_drag_between_children_of_a_focusable_container_produces_no_click | Native KDE Plasma 6.6 Wayland on linuxDEV (ydotool input, AT-SPI, screenshots): PASS (evidence/n-native-linux/README.md): Russian layout Ctrl+A/C/V/Z by physical key; icon-press/padding-release clicks; found and fixed prevent_default physical-key mismatch (658d8f7) | Native Windows (SendInput scan codes + UIA, interactive session): fails on 1.0.1, passes on integration (evidence/lead/windows-native-interaction.md) | efee6a2 a25f757 658d8f7 a7fecec 191c3c9 | Docs examples now test event code (KeyS); apps that match event key for shortcuts still fail on non-Latin layouts, as in browsers. |

## Specialist findings

| ID | Sev. | Consolidated | WS | Status | Commits | Notes |
| --- | --- | --- | --- | --- | --- | --- |
| HOST-01 | High→Critical (F-02) | F-02 | A | FIXED_VERIFIED | c90062f 78df633 8c8cab3 eae0ad9 |  |
| HOST-02 | High→advisory (§2.3) |  | G | FIXED_VERIFIED | c90062f | PR #3 not merged; F-02 fixed without dropping. Advisory closed. |
| HOST-03 | Medium | F-18 | G | FIXED_VERIFIED | 3dc57a7 |  |
| HOST-04 | Low-Med | F-25 | G | FIXED_VERIFIED | 7813ef8 0adab8c |  |
| HOST-05 | Low-Med |  | G | FIXED_VERIFIED | eb4bb24 |  |
| HOST-06 | Low | F-14 | H | FIXED_VERIFIED | c30e1e0 ( 268de65 | request_submit linear in release (24/31/44 ms at 1k/2k/4k fields). Superlinear cost is first layout of many inline inputs (0.52/2.33/17.3 s) - handed to ws H. |
| HOST-07 | Low-Med | F-25 | G | FIXED_VERIFIED | 92977d2 f5b3fb7 |  |
| HOST-08 | Low | F-25 | G | FIXED_VERIFIED | 8c8cab3 |  |
| HOST-09 | Low | F-02 | A | FIXED_VERIFIED | c90062f 78df633 8c8cab3 eae0ad9 |  |
| HOST-10 | Low | F-25 | G | FIXED_VERIFIED | 1d2753a |  |
| HOST-11 | Low |  | G | FIXED_VERIFIED | 203a8fb |  |
| HOST-12 | Low |  | G | DEFERRED | 203a8fb | SQOS identification + session check done (203a8fb); server SID ownership check deferred: false-refusal risk across elevated/non-elevated pairs needs a dedicated Windows test matrix. |
| HOST-13 | Low |  | G | FIXED_VERIFIED | 203a8fb |  |
| HOST-14 | Low |  | G | FIXED_VERIFIED | eb210dc |  |
| HOST-15 | Low-Med | F-25 | G | FIXED_VERIFIED | 2c1233a |  |
| HOST-16 | Low |  | G | FIXED_VERIFIED | dfa29a1 | Panic hook only; catch_unwind around document commands deferred (changes failure semantics across renderer boundary). |
| HOST-17 | Low |  | G | FIXED_VERIFIED | 859955d |  |
| RENDER-01 | High | F-04 | C | FIXED_VERIFIED | 7c111c8 |  |
| RENDER-02 | High→Critical (F-01) | F-01 | B | FIXED_VERIFIED | 96c549e 554b58d |  |
| RENDER-03 | Medium | F-06 | B | FIXED_VERIFIED | 7587eb3 |  |
| RENDER-04 | Low | F-12 | G | FIXED_VERIFIED | 85c817f |  |
| RENDER-05 | Medium→High (F-16a) | F-16a | C | FIXED_VERIFIED | 3335e26 |  |
| RENDER-06 | Medium | F-16b | C+D | FIXED_VERIFIED | c955e44 |  |
| RENDER-07 | Medium | F-16b | C+D | DEFERRED |  | Measured 26-27 ms per set_text at 2,000 rows (layout ~17, scene ~5, raster ~4-5). O(dirty) pipeline is roadmap 8.1, optional; postponed per 08 until measured need beyond R5(b) 100 ms budget, which is met. |
| RENDER-08 | Low-Med |  | C | DEFERRED |  | Full frame 8 ms @1280x800, ~20 ms @4K; damage-limited present is roadmap 8.3 (optional). |
| RENDER-09 | Low |  | C | DEFERRED |  | insert_html validation is ~95% of cost on 40k-node docs (3.85 ms/insert). Exact fragment-only check needs an id index and reachable-node counter in ui4me-document; plan in evidence/h-stalls/inputsec-10-11-render-09.md. |
| RENDER-10 | Low (test gap) |  | C | FIXED_VERIFIED | 8806f21 | Fuzz workers on half production stack + depth/SVG generators; nightly scheduled by ws F. |
| RENDER-11 | Low |  | C | DEFERRED |  | crate-wide allow(dead_code) cleanup; maintenance, post-1.0.2. |
| RENDER-12 | Low-Med | F-27 | F | DEFERRED | 14b1ae3 | verify_patches.sh (Stylo/Taffy patch-stack drift) scheduled nightly (14b1ae3); reducing/upstreaming the Taffy delta postponed per 08-non-recommendations (large rebase risk for a stabilization release). |
| RENDER-13 | Low |  | C | DEFERRED |  | raw text_ptr aliasing in HostTree: sound today; refactor post-1.0.2. |
| INTERACT-01 | High | F-08 | D | FIXED_VERIFIED | see F-08 |  |
| INTERACT-02 | High | F-07 | D | FIXED_VERIFIED | see F-07 |  |
| INTERACT-03 | High | F-07 | D | FIXED_VERIFIED | see F-07 |  |
| INTERACT-04 | Medium | F-16b | C+D | FIXED_VERIFIED | 98bc678 |  |
| INTERACT-05 | Medium→High (F-17) | F-17 | D | FIXED_VERIFIED | see F-17 |  |
| INTERACT-06 | Medium | F-17b | D | FIXED_VERIFIED | see F-17b 658d8f7 |  |
| INTERACT-07 | Medium | F-17b | D | FIXED_VERIFIED | see F-17b |  |
| INTERACT-08 | Medium | F-02 | A | DEFERRED |  | Host-side pointermove coalescing per frame not implemented. Mitigated: F-02 (8 MiB byte-bounded queue, no message cap) and F-29 (bindings evict coalescible events first). Post-1.0.2. |
| INTERACT-09 | Low (Windows IME in password fields, not run) |  | D | DEFERRED |  | Deferred: patched winit-win32 ImeRequest::Disable semantics unverified; needs native Windows IME test (proposal: disable IME when ime.password on cfg(windows)). |
| INTERACT-10 | Low |  | D | FIXED_VERIFIED | 4738155 |  |
| INTERACT-11 | Low |  | D | DEFERRED |  | drag-region tag list and fixed double-click timing; low, post-1.0.2. |
| INTERACT-12 | Low |  | D | DEFERRED |  | Deferred: depends on native IM event ordering (Wayland text-input, IMM32); removing quiet window could reintroduce stale-commit bug. |
| INTERACT-13 | Low |  | D | DEFERRED |  | Deferred: SPI_GETWHEELSCROLLLINES changes Windows scroll speed; needs native verification. |
| INTERACT-14 | Low |  | D | DEFERRED |  | vendored patch maintenance notes; low, post-1.0.2. |
| INTERACT-15 | Low |  | D | DEFERRED |  | platform divergence disclosure: partially addressed in docs/concepts; remaining items post-1.0.2. |
| BIND-01 | Medium | F-13 | E | FIXED_VERIFIED | 0922dcf |  |
| BIND-02 | Medium→High (F-29) | F-29 | E | FIXED_VERIFIED | 0922dcf |  |
| BIND-03 | Medium | F-13 | E | FIXED_VERIFIED | 0922dcf |  |
| BIND-04 | Medium | F-26 | E | FIXED_VERIFIED | c3e1cca |  |
| BIND-05 | Medium | F-26 | E | DEFERRED |  | Needs a host change: [backend] stdio = "inherit" or a lossless backpressured relay instead of the 512-line ring. Post-1.0.2. |
| BIND-06 | Low-Med | F-26 | E | FIXED_VERIFIED | 0922dcf |  |
| BIND-07 | Low-Med |  | E | DEFERRED | 0922dcf | Partial: Application.on_activated added; forwarding a second launch's script arguments needs host/protocol work. Post-1.0.2. |
| BIND-08 | Low-Med |  | E | DEFERRED |  | Default identity collisions (zipapps/__main__, scaffold app ids): DX; post-1.0.2 backlog. |
| BIND-09 | Low-Med | F-26 | E | FIXED_VERIFIED | c3e1cca |  |
| BIND-10 | Low |  | E | FIXED_VERIFIED | c3e1cca 92a7ea1 |  |
| BIND-11 | Low |  | E | FIXED_VERIFIED | 0922dcf |  |
| BIND-12 | Low | F-22 | F | FIXED_VERIFIED | c3e1cca 0922dcf |  |
| BIND-13 | Low |  | E | FIXED_VERIFIED | abd85f8 |  |
| BIND-14 | Low |  | E | DEFERRED |  | Linux startup DX messages; post-1.0.2 backlog. |
| BIND-15 | not confirmed (coordinator test) |  | E | NOT_REPRODUCED |  | Coordinator E12 on Windows Server 2022 (uv Python 3.13): no console window, no conhost. Evidence docs/audit/evidence/logs/win-pyw5-result.txt. |
| BIND-16 | Low |  | E | DEFERRED |  | Python API surface gaps; post-1.0.2 backlog (no defect). |
| BIND-17 | Medium | F-02 | A | FIXED_VERIFIED | see F-02 | Host side fixed by ws A (F-02); binding side by ws E (F-29). |
| BIND-18 | Low |  | E | DEFERRED |  | sdist fallback on unsupported platforms; post-1.0.2 backlog. |
| BIND-19 | High (=F-02) | F-02 | A | FIXED_VERIFIED | see F-02 | Host side fixed by ws A (F-02); binding side by ws E (F-29). |
| BIND-PS-01 | Medium | F-28 | E | FIXED_VERIFIED | 916b1d7 |  |
| BIND-PS-02 | Medium (=F-29) | F-02, F-29 | A | FIXED_VERIFIED | b6b2d86 |  |
| BIND-PS-03 | Medium-Low (=F-29) | F-29 | E | FIXED_VERIFIED | b6b2d86 df14e52 |  |
| BIND-PS-04 | Low-Med | F-28 | E | FIXED_VERIFIED | b6b2d86 |  |
| BIND-PS-05 | Low-Med | F-28 | E | FIXED_VERIFIED | 916b1d7 df14e52 |  |
| BIND-PS-06 | Low-Med | F-28 | E | FIXED_VERIFIED | 916b1d7 |  |
| BIND-PS-07 | Low | F-28 | E | FIXED_VERIFIED | 916b1d7 92a7ea1 334ede8 |  |
| BIND-PS-08 | Low | F-28 | E | FIXED_VERIFIED | 916b1d7 |  |
| BIND-PS-09 | Low | F-28 | E | FIXED_VERIFIED | 916b1d7 |  |
| BIND-PS-10 | Low | F-28 | E | DEFERRED |  | Writes synchronous on the runspace thread, not covered by RPC timeouts; low risk (8 MiB bounded host queue); post-1.0.2. |
| BIND-PS-11 | Low | F-19 | E | FIXED_VERIFIED | 9827c3d |  |
| BIND-PS-12 | Low |  | E | DEFERRED |  | JSON serializer edge cases; post-1.0.2 backlog. |
| BIND-PS-13 | Low |  | E | DEFERRED |  | Idiomatic pipeline/error records; API design, post-1.0.2. |
| BIND-PS-14 | Low |  | E | DEFERRED |  | Naming consistency; API design, post-1.0.2. |
| BIND-PS-15 | Low |  | E | DEFERRED |  | Runspace/thread-affinity guard; post-1.0.2. |
| BIND-PS-16 | Low |  | E | DEFERRED |  | Add-Type cost; post-1.0.2. |
| BIND-PS-17 | Low |  | E | DEFERRED |  | Callback variable scoping; documented behaviour, post-1.0.2. |
| BIND-PS-18 | Low |  | E | DEFERRED |  | Non-ASCII launcher output on Windows unverified; post-1.0.2. |
| BIND-PS-19 | Low |  | E | DEFERRED |  | Start-UI4MEHost wait mode prints raw JSON records; post-1.0.2. |
| INPUTSEC-01 | High (=F-01) | F-01 | B | FIXED_VERIFIED | 96c549e 554b58d |  |
| INPUTSEC-02 | High (=F-03) | F-03 | B | FIXED_VERIFIED | c8b9832 |  |
| INPUTSEC-03 | Medium | F-14 | H | FIXED_VERIFIED | c526a94 |  |
| INPUTSEC-04 | Medium | F-15 | H | FIXED_VERIFIED | ecf3504 |  |
| INPUTSEC-05 | Low-Med | F-14 | H | FIXED_VERIFIED | b20efc5 ec4a764 |  |
| INPUTSEC-06 | Medium | F-14 | H | FIXED_VERIFIED | 467bb7d |  |
| INPUTSEC-07 | High (=F-01) | F-01 | B | FIXED_VERIFIED | 554b58d c8b9832 |  |
| INPUTSEC-08 | Low | F-23, F-27 | F | FIXED_VERIFIED | 14b1ae3 |  |
| INPUTSEC-09 | Low-Med | F-27 | F | DEFERRED | 14b1ae3 | verify_patches.sh (Stylo/Taffy patch-stack drift) scheduled nightly (14b1ae3); reducing/upstreaming the Taffy delta postponed per 08-non-recommendations (large rebase risk for a stabilization release). |
| INPUTSEC-10 | Low |  | H | DEFERRED |  | Windows DLL search-order hardening (+crt-static, /DEPENDENTLOADFLAG:0x800, SetDefaultDllDirectories) belongs in host build/main; recommendations in evidence/h-stalls/inputsec-10-11-render-09.md. Not a regression; post-1.0.2. |
| INPUTSEC-11 | Low |  | H | FIXED_VERIFIED | 79b9bec | checked_add in Reader::need and command length; unit test fails on baseline |
| RELEASE-01 | High | F-10 | F | FIXED_VERIFIED | 9240d35 |  |
| RELEASE-02 | High→Medium (F-09) | F-09 | F | FIXED_VERIFIED | 099f238 |  |
| RELEASE-03 | Medium | F-11 | F | BLOCKED |  | Blocked with F-11: the fix is in the workflows and needs a GitHub Actions run (first push of the integration branch; publishing not authorized). |
| RELEASE-04 | Medium | F-11 | F | BLOCKED |  | Blocked with F-11: the fix is in the workflows and needs a GitHub Actions run (first push of the integration branch; publishing not authorized). |
| RELEASE-05 | Medium | F-20 | F | NOT_REPRODUCED |  | Repository public since 2026-11-12; links resolve (see F-20). |
| RELEASE-06 | Medium | F-19 | E | FIXED_VERIFIED | 9827c3d |  |
| RELEASE-07 | Medium | F-21 | F | FIXED_VERIFIED | d24fbe0 805e59e bba9e6d 76a094b | Fixed with F-21: tools/release/version.py and bump_version.py; the 1.0.2 bump touched no workflow. |
| RELEASE-08 | Medium | F-22 | F | BLOCKED |  | Blocked with F-22: the fix is in the workflows and needs a GitHub Actions run (first push of the integration branch; publishing not authorized). |
| RELEASE-09 | Medium | F-23 | F | FIXED_VERIFIED | 14b1ae3 |  |
| RELEASE-10 | Medium | F-24 | F | FIXED_VERIFIED | 37c0399 |  |
| RELEASE-11 | Medium | F-24 | F | FIXED_VERIFIED | 37c0399 |  |
| RELEASE-12 | Low-Med | F-10 | F | BLOCKED |  | Owner decision: branch protection / PR-only main / CODEOWNERS are repository settings outside the code; needs the owner and push access. Recommendations unchanged from the audit. |
| RELEASE-13 | Low |  | F | FIXED_VERIFIED | 126075e |  |
| RELEASE-14 | Low |  | F | BLOCKED | 026d68f | Workflow change verified statically (tools/test_workflows.py, actionlint-equivalent checks, ci_gate tests); NOT executed on GitHub Actions: pushing the branch is not authorized. Concurrency/cancellation, caching and timeouts wired in ci.yml. |
| RELEASE-15 | Low |  | F | FIXED_VERIFIED | 37c0399 126075e d24fbe0 |  |
| RELEASE-16 | Low |  | F | FIXED_VERIFIED | 824353c |  |
| RELEASE-17 | Low |  | F | FIXED_VERIFIED | 5d1dc25 |  |
| RELEASE-18 | Low |  | F | FIXED_VERIFIED | 126075e |  |
| RELEASE-19 | Low-Med |  | F | BLOCKED | 026d68f | Workflow change verified statically (tools/test_workflows.py, actionlint-equivalent checks, ci_gate tests); NOT executed on GitHub Actions: pushing the branch is not authorized. nightly.yml added with schedule, concurrency and timeouts. |

## Tests T1–T16

| ID | Status | Location | CI | Notes |
| --- | --- | --- | --- | --- |
| T1 | FIXED_VERIFIED | crates/ui4me-renderer/tests/stack_depth.rs, crates/ui4me-assets/tests/svg_depth.rs | cargo test (default); release in nightly | Native Windows pass incl. inline-block and button at depth 256 (cd4a12e). |
| T2 | FIXED_VERIFIED | crates/ui4me-host/src/transport.rs tests | cargo test (default) | Fails on baseline (message 236 refused). |
| T3 | FIXED_VERIFIED | tools/test_headless_clock.py | python stage | Linux + Windows |
| T4 | FIXED_VERIFIED | renderer font_identity_tests, tests/font_identity_soak.rs | cargo test (default) | Baseline: +78.3 MiB; fixed +95 KiB; native Windows pass |
| T5 | FIXED_VERIFIED | renderer document::geometry_tests |  | Unit tests pass Linux+Windows; native checks: evidence/n-native-linux (Linux) and evidence/lead/windows-native-interaction.md (Windows) |
| T6 | FIXED_VERIFIED | fuzz_gate.rs, fuzz_resources.rs (#[ignore]) | nightly (ws F) | Small-stack workers; depth (incl. inline-block/button/multicol units), SVG depth and SVG filter-cost generators. Linux: depth 120 s/151 cases, svg_filters 90 s/1,476 cases, 0 findings. Nightly via ws F. |
| T7 | BLOCKED | tools/test_ci_signal.py, test_workflows.py |  | Local tests pass; the CI part needs a GitHub Actions run (not run in this stabilization). |
| T8 | BLOCKED | tools/test_ci_gate.py |  | Local tests pass; the CI part needs a GitHub Actions run (not run in this stabilization). |
| T9 | FIXED_VERIFIED | tools/test_binding_robustness.py CallbackRobustnessTests; Callbacks.Tests.ps1 | python stage; Pester | Fails on baseline; Windows 3.10/3.13 and 5.1/7 pass |
| T10 | FIXED_VERIFIED | renderer document::interact_tests |  | Unit tests pass Linux+Windows; native checks: evidence/n-native-linux (Linux) and evidence/lead/windows-native-interaction.md (Windows) |
| T11 | BLOCKED | ci.yml python matrix 3.10-3.14 + Pester (unrun on GitHub) |  | Local tests pass; the CI part needs a GitHub Actions run (not run in this stabilization). |
| T12 | BLOCKED | ci.yml clean-wheel + allusers jobs, verify-published.yml |  | Local tests pass; the CI part needs a GitHub Actions run (not run in this stabilization). |
| T13 | FIXED_VERIFIED | regression::append_cliff, a11y presence counters | cargo test (default) | append shapes 1 paragraph; 0 a11y builds per 100 ticks without AT |
| T14 | BLOCKED | tools/windowed_smoke.py + nightly |  | tools/windowed_smoke.py 4/4 on the live KDE Wayland session (linuxDEV) and under nested kwin --virtual; nightly job not yet run on GitHub Local tests pass; the CI part needs a GitHub Actions run (not run in this stabilization). |
| T15 | FIXED_VERIFIED | tools/test_host_lifecycle.py, host unit tests | python stage | Linux full; Windows: host lifecycle subset + Ctrl+Break direct start (Python, PS 5.1, PS 7) |
| T16 | FIXED_VERIFIED | tools/test_binding_robustness.py ControlNotificationTests; powershell/tests/Delivery.Tests.ps1 | python stage; Pester | Fails on baseline; Windows pass incl. windowed N=100 burst probe |

## Release gates

| Gate | Requirement | Baseline (1.0.1) | Current | Evidence |
| --- | --- | --- | --- | --- |
| R1 | No open Critical or High runtime defect | FAILING | MET | No Critical or High runtime finding is open. Remaining runtime items are Medium or Low and deferred (RENDER-07, INTERACT-08, BIND-05, ...). |
| R2 | Documented limits render without abort on production stacks | FAILING | MET | T1 Linux+Windows; Windows windowed depth 250 div/fieldset, SVG 700 |
| R3 | 10,000-event bursts / 2,000 events/s for 60 s: no disconnect, no dropped control notification | FAILING | PARTIAL | Bursts 100 controls x 6 rounds: droppedControl 0, no disconnect on Linux and Windows (Python, PS 5.1, PS 7). The 10,000-event / 2,000 events/s for 60 s soak was not run (deferred). |
| R4 | Memory: counting-allocator soak <1 MiB; 30-min windowed soak <5 MiB/5% | FAILING | PARTIAL | font_identity_soak; 900 s windowed Linux flat; Windows flat |
| R5 | Stalls bounded (pathological <250 ms or rejected; reference docs <100 ms; frame gap <50 ms draining) | FAILING | PARTIAL | R5(c) headless 19 ms; F-16a append 55 ms; pathological inputs pending ws H |
| R6 | Keyboard reachability/operability; focus scrolled into view | FAILING | MET | F-08, F-17, F-17b verified with real input on Windows and Linux (evidence/lead/windows-native-interaction.md). |
| R7 | Geometry consistency across caret/selection/IME/AT/get_rect vs hit testing | FAILING | MET | F-07 geometry tests incl. proptest. Deferred: scroll-into-view under scale/rotate (renderer review #6). |
| R8 | AT contract checks automated + manual Orca/NVDA checklist | UNVERIFIED | PARTIAL | UIA and AT-SPI tree checks automated; Orca checked manually on linuxDEV. NVDA/Narrator not checked. |
| R9 | Backend crash/freeze/veto, signals, session end handled as documented | PARTIAL | PARTIAL | SIGTERM/SIGINT/SIGHUP deliver app.shutdown (Linux); Windows Ctrl+Break delivers app.shutdown, launcher still dies; session end unverified |
| R10 | Bindings never die silently on user-code errors | FAILING | MET | F-13, F-28 and the 9 bindings review fixes; Python 3.10/3.13 and Pester 5.1/7.4 on Windows. |
| P1 | Release commit has green required CI on every OS, enforced by publish workflow | FAILING | BLOCKED | Required checks and ci_gate implemented (F-09/F-10); needs GitHub Actions runs and branch protection. |
| P2 | Ordinary changes can be green; lint always runs | FAILING | BLOCKED | CI fixed for ordinary changes (F-09); first GitHub run pending. |
| P3 | Publish requires green CI for the SHA plus manual dispatch | PARTIAL | MET | Publish workflows refuse a SHA without green required checks (tools/release/ci_gate.py, test_ci_gate.py). |
| P4 | Published artifacts are exactly the qualified artifacts | MET Linux / FAILING Windows | PARTIAL | Workflow can publish the qualified Windows wheel; the verifier checks stack reserve and release build. Needs a CI-built wheel (F-11). |
| P5 | Every package version traceable to its tag | MET PyPI / FAILING PSGallery 1.0.0 | PARTIAL | Workflows tie both packages to the tag. PSGallery 1.0.0 history cannot be changed. |
| P6 | Every claimed runtime tested in CI against the real host | FAILING | BLOCKED | Python 3.10-3.14 and real-host Pester (Linux, Windows, PS 5.1) configured in CI; run locally and on winDEV, not on GitHub (F-22). |
| P7 | Every documented install mode works | FAILING | PARTIAL | AllUsers on Linux fixed (F-19); clean-wheel and AllUsers CI jobs not yet run on GitHub. |
| P8 | Dependency advisories and licences checked in CI | FAILING | BLOCKED | cargo deny configured in CI (F-23); first GitHub run pending. |
| P9 | Workflows hardened (SHA-pinned, no template injection) | FAILING | MET | Actions SHA-pinned, no template injection (F-23, tools/test_workflows.py). |
| P10 | Provenance: PyPI attestations, build provenance, SBOM | PARTIAL | PARTIAL | Attestations, SBOM and provenance steps in the publish workflows; exercised only at the next release. |
| P11 | Version single-sourced; a release does not edit CI | FAILING | MET | F-21: single version source; the 1.0.2 bump edited no workflow. |
| P12 | Changelog, release notes, compatibility/deprecation policy, SECURITY.md, yank procedure | FAILING | MET | CHANGELOG.md, docs/release/RELEASE_NOTES_1.0.2.md, versioning policy, SECURITY.md (F-24). |
| P13 | Public metadata links resolve | FAILING | MET | Repository is public since 2026-11-12; F-20 not reproduced. |
| P14 | MPL-2.0 source availability for modified Stylo | MET | MET |  |
| P15 | Fresh contributor can build and test from docs | MET | MET |  |
| P16 | Published artifacts work on clean machines | MET (audit) | MET (audit) |  |
| P17 | Vendored dependencies verifiably equal upstream plus patches | MET manually | MET manually |  |
| P18 | Secrets least-privilege | MET | MET |  |

Consolidated status counts: BLOCKED 2, DEFERRED 1, FIXED_VERIFIED 26, NOT_REPRODUCED 1
