# UI4ME Core stabilization (post-audit, toward 1.0.2 and a stable claim)

Input: the engineering audit of 1.0.1 (`c6fd71d`) in [../audit/](../audit/README.md).

| File | Purpose |
| --- | --- |
| [tracker.json](tracker.json) | machine-readable tracker: 30 consolidated findings, 113 specialist findings, tests T1–T16, gates R1–R10 and P1–P18 |
| [MATRIX.md](MATRIX.md) | generated finding-by-finding matrix (`python3 tools/stabilization_tracker.py render`) |
| [JOURNAL.md](JOURNAL.md) | engineering journal: changes, root causes, trade-offs, verification commands and results, integrated commits |
| `RELEASE-READINESS.md` | gate-by-gate assessment of the integrated build (written at the end of each checkpoint) |
| `evidence/<workstream>/` | measurements and logs produced by each workstream |

`tools/test_stabilization_tracker.py` runs in the default Python suite. It refuses `FIXED_VERIFIED` without a
commit, a named regression test and the required Linux/Windows results, and refuses `DEFERRED`, `BLOCKED`,
`ALREADY_FIXED` and `NOT_REPRODUCED` without a written justification.

## Status vocabulary

`OPEN`, `IN_PROGRESS`, `FIXED_VERIFIED`, `ALREADY_FIXED`, `BLOCKED`, `DEFERRED`, `NOT_REPRODUCED`.
`classification` separates confirmed defects, process defects, inferred risks and improvements;
`owner_decision: true` marks items only the repository owner can decide (visibility, plan, signing certificate).

## Workstreams and file ownership

Integration branch: `stabilization/integration`. Each workstream works in its own worktree and branch
(`stab/<ws>`) and only edits the files it owns; cross-workstream edits are reported and reviewed at merge.

| WS | Branch | Scope | Owned files |
| --- | --- | --- | --- |
| A | `stab/a-transport` | F-02, HOST-08/09, T2 | `crates/ui4me-host/src/transport.rs` (+ tests), protocol limits docs |
| B | `stab/b-stack` | F-01, F-03, F-06, T1, T6 generators | host `main.rs`/`build.rs` (event-loop thread), `crates/ui4me-assets`, `crates/ui4me-document`, renderer `layout/box_tree` |
| C | `stab/c-fonts` | F-04, F-16a, F-16b (fonts/cache), T4, T13 (shaping) | renderer `text/**`, content cache |
| D | `stab/d-interact` | F-07, F-08, F-17, F-17b, INTERACT-04, T5, T10, T13 (a11y) | renderer interaction/focus/IME/geometry/a11y, host a11y/IME glue |
| E | `stab/e-bindings` | F-29, F-13, F-28, F-19, F-26, BIND-*, T9, T16 | `python/**`, `powershell/**`, binding tool tests |
| F | `stab/f-release` | F-09, F-10, F-11 (6.0), F-20..F-24, T7, T8, T11, T12, nightly | `.github/**`, `tools/release/**`, verifiers, packaging metadata, policies |
| G | wave 2 | F-12, F-18, F-25, HOST-05/06 | host lifecycle, headless loop (after A and B merge) |
| H | wave 2 | F-14, F-15 | renderer SVG filters, intrinsic sizing, parser, paint budgets (after B merges) |

Contracts fixed up front: the msvc host links with `/STACK:33554432` and the artifact check asserts that value;
the protocol stays v1 (no host-side dropping; any new capability is negotiated in `host.hello`); the version is
bumped once, after integration.

Windows verification is run by the lead only (one workspace on `winDEV`, see the journal).
