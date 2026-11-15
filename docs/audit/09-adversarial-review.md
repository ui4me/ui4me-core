# 9. Adversarial review and disposition

After the first complete draft, an independent review (Opus, extra-high effort, read-only, briefed to challenge rather than confirm) went through every document, spot-checked code claims and evidence logs, and produced 16 must-fix objections, 13 suggestions and 7 recorded disagreements. The full text is retained in [findings/REVIEW-full.md](findings/REVIEW-full.md). This page records what was done with each item. Where the review could be settled with evidence rather than argument, the coordinator ran the check (E12, E13).

## 9.1 Must-fix objections

| # | Objection (short) | Disposition |
| --- | --- | --- |
| M1 | Missing files, placeholder, absolute paths, "two people" | **Fixed.** BIND and this page added; placeholder removed and the burst matrix retained as [evidence](evidence/logs/bind-burst-matrix-linux-windowed.txt); paths normalised; wording corrected. |
| M2 | F-01 stack figure "~6–24 KiB per element" cannot explain an overflow at 40 | **Accepted.** ~24 KiB per `<div>` level, ~32 KiB per `<fieldset>` level (6 KiB is per frame). Fixed in 02, 03, README and the INPUTSEC note. |
| M3 | A 16 MiB PE reserve fails the proposed invariant for `<fieldset>` and does nothing on Linux | **Accepted.** Roadmap 1.1 now runs the event loop on an explicitly sized 32 MiB thread on both OSes, keeps a matching Windows reserve as a backstop, and makes frame-shrinking part of 1.1. Effort raised to M. |
| M4 | "winit and AccessKit are main-thread bound" is false | **Accepted.** The vendored winit has `with_any_thread` on Win32 and Wayland. The postponed row in 08 is removed; the large-stack thread is now the recommended fix. RENDER note corrected. AccessKit on a non-main thread must still be checked during implementation. |
| M5 | T1/R2 cannot pass with `inline-block` while its layout is exponential | **Accepted.** `inline-block` is excluded from T1/R2 until roadmap 2.5; R2 is explicitly about aborts, not time. |
| M6 | Fixing F-02 moves the failure into the bindings' drop policies | **Accepted.** New finding F-29 (High), roadmap 1.3 (P0), test T16, gate R3 now includes "no dropped control notification". |
| M7 | The "stopgap" was the full redesign; removing the count cap meets R3 | **Accepted.** Roadmap 1.2 is now: byte-bounded queue (the 8 MiB cap exists), one syscall per frame, log once. Lanes, sequence numbers and loss markers are deferred until a measured need (see D4). 03 and 08 updated. |
| M8 | PR #3 contradiction between areas; a draft PR is not a product defect | **Accepted.** F-05 removed from the ranked list and turned into an advisory PR review (register §2.3); BIND wording aligned with HOST-02. |
| M9 | 1.0.2 test list differs across documents; T3 depends on Phase 2 | **Accepted.** One list everywhere: T1, T2, T4, T7, T8, T9, T16; T3 with roadmap 2.1; ~three engineer-weeks. |
| M10 | R5 (250 ms for any command) unattainable; evidence was a debug build | **Accepted.** R5 split into pathological inputs (< 250 ms or rejected), reference documents (< 100 ms per update), and frame gap while draining (< 50 ms). `request_submit` marked for release re-measurement. |
| M11 | R4 tighter than the audit's own non-leaking control | **Accepted.** R4 is now a deterministic counting-allocator soak plus a nightly windowed soak with a 5 MiB / 5 % threshold. |
| M12 | Second-person gates impossible for a solo maintainer; plan facts uncertain; going public is an owner decision | **Accepted.** P3 rewritten around the CI gate plus manual dispatch; roadmap 6.2 is an owner decision with options and an instruction to check current GitHub plan documentation; "make the repository public" listed in 08 as not recommended by the audit. |
| M13 | Fuzzing claims contradict `fuzz_gate.rs` | **Accepted.** The harness already generates nesting bombs to 600; it would very likely have found F-01 at a 1 MiB stack, but F-03 and F-06 need new generators. Fixed in 04, 08 and the RENDER note. |
| M14 | Index and R1 gaps | **Fixed.** BIND-19 and PS findings indexed; F-28 (PowerShell defects) and F-29 added; R1 now explicitly covers runtime findings, process findings are P-gates. |
| M15 | Windows claims stronger than retained evidence; identical PS/Python logs | **Addressed with new evidence.** Remaining Windows logs retained (depth 20/60/120/250, SVG 500/700, Application Error events). The PowerShell run was repeated with a version stamp (E13: 5.1.20348.5773 Desktop, N=10 passes, N=20 fails); the earlier identical logs were genuine, because both probes print the same deterministic lines for the same document. BIND-15 (`pythonw` console window) was **run** (E12) and not confirmed. README Q6 now tags each item by evidence level. |
| M16 | Rubric mixes likelihood into severity; F-03's "untrusted per the docs" unsupported | **Accepted.** Rubric now distinguishes benign content (Critical) from crafted input (High); F-03 High severity, Low likelihood; INPUTSEC wording corrected. |

## 9.2 Suggestions

| # | Suggestion | Disposition |
| --- | --- | --- |
| S1 | OS theme switches trigger F-02 with no app involvement | **Adopted** as a likelihood note (code path verified by the reviewer; not run end to end). The 18 events per button are 6 longhands × 3 event types, not duplicates. |
| S2 | Pull focus-into-view, Enter-on-link and `sys.exit()` fixes into 1.0.2; raise Enter-on-link to High | **Adopted**: roadmap 1.5 ("if capacity allows"); Enter-on-link is F-17 (High). |
| S3 | Minimal manifest change for 1.0.2 | **Adopted**: 6.1 now runs the check only in publish workflows; generation at publish time moves to 6.3. |
| S4 | Test the exact published Windows wheel for 1.0.2 | **Adopted** as roadmap 6.0. |
| S5 | 1.1 effort is three to five engineer-months, not one to two | **Adopted.** |
| S6 | Phase labels contradict P-levels; RENDER-05 deserves High | **Adopted**: content-stall work moved to 2.5; cheap large-document fixes are 3.4 (P1); RENDER-05 is F-16a (High for the long-log use case). |
| S7 | Making `host.hello` strict would break forward negotiation | **Adopted** in roadmap 2.3 and 03 §3.4. |
| S8 | Make R3 and R8 testable | **Adopted.** |
| S9 | Do not gate the stable claim on supply-chain hygiene | **Adopted**: the stable claim requires P1–P7, P12, P13; P8–P11 recommended. |
| S10 | Recommend the Beta classifier directly | **Adopted** (roadmap 7.1). |
| S11 | Uncovered dimensions (forced colours, bidi editing, locked-down Windows, non-ASCII paths, portal-less dialogs, telemetry, bus factor, `host.manifest` version) | **Adopted**: listed in 01 "dimensions not covered"; forced colours added as roadmap 8.7; locked-down Windows as 4.3; bus factor in 06; `host.manifest` in F-21. |
| S12 | Tone and accuracy of the executive text | **Adopted**: removed the unsupported "three days of public history" remark; 1.0.0 publication described precisely; "by design" replaced with "as a side effect"; "built from identical source" added; headless claim narrowed; model-routing paragraph shortened. |
| S13 | Rate process findings on their own scale | **Adopted**: separate process table; F-09 Medium, F-10 High. |

## 9.3 Disagreements recorded

| # | Point | Outcome |
| --- | --- | --- |
| D1 | F-01 stays Critical but likelihood should be Low-Medium (showcase depth 14) | **Adopted.** |
| D2 | F-02 Critical, if anything understated | Agreed; no change. |
| D3 | The reviewer would rate F-03 Medium under a trusted-backend model | **Not adopted.** It stays High (severity reflects a whole-process abort from a ~10 KB file; likelihood Low carries the realism). Both views agree it belongs in 1.0.2 because the fix is trivial. |
| D4 | The full event-contract redesign may be more than a 1.x project needs | **Adopted.** Type-level subscription is the only 1.1 protocol change; lanes, sequence numbers and loss markers are conditional on measured need. The HOST area report still describes the full design, as a reference if that need appears. |
| D5 | F-07 likelihood Medium-High rather than High | **Adopted.** |
| D6 | `/STACK` mechanics are correct and harmless on x64 | Agreed; retained as the Windows backstop. |
| D7 | Dropping the committed manifest long-term is fine; only the timing differs | Agreed; reflected in 6.1/6.3. |

## 9.4 What the review confirmed

The review independently confirmed the issue #2 root cause and fix (`environment.rs:269, 323-328`), the issue #1 mechanism (`document.rs:347-405`, `transport.rs:17, 215, 273, 305-306`, `host/mod.rs:442-447`), the asymmetry between the inline-SVG (depth 48) and image-SVG (none) caps, the PR #3 analysis, the release-process facts, and the non-recommendations.
