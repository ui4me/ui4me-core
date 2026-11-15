# UI4ME Core engineering audit (1.0.1, commit `c6fd71d`)

Independent, adversarial engineering audit of UI4ME Core at tag `v1.0.1` (commit `c6fd71dc5c15dcdfdcdc1500dc156c1b929c216c`), carried out on 2026-11-15. The audit evaluates the project; it does not change it. No production code, build configuration or release artifact was modified.

| Document | Contents |
| --- | --- |
| [01-scope-and-method.md](01-scope-and-method.md) | revision, environments, organisation, commands, experiments E1–E13, limitations, dimensions not covered, lab assets left |
| [02-findings-register.md](02-findings-register.md) | ranked runtime findings, process findings, review of open PRs, index of all area IDs, coordinator corrections, reproduction quick reference |
| [03-architecture.md](03-architecture.md) | what to keep, what creates risk, what to redesign, what to simplify |
| [04-testing-and-quality.md](04-testing-and-quality.md) | test inventory, why the suite missed the defects, ranked tests T1–T16 |
| [05-release-readiness.md](05-release-readiness.md) | objective runtime (R) and process (P) gates with status |
| [06-technical-debt.md](06-technical-debt.md) | compounding liabilities vs harmless imperfections |
| [07-roadmap.md](07-roadmap.md) | phased roadmap with priority, dependencies, effort and acceptance criteria |
| [08-non-recommendations.md](08-non-recommendations.md) | changes considered and rejected or postponed |
| [09-adversarial-review.md](09-adversarial-review.md) | independent challenge of the draft, what changed, and remaining disagreements |
| [findings/](findings/) | area reports: HOST, RENDER, INTERACT, BIND (+ BIND-powershell), INPUTSEC, RELEASE |
| [evidence/](evidence/) | reproduction scripts and logs |

## Executive assessment

**UI4ME Core is a well-designed engine with a careful host and a weak safety net.** Its central architecture — a Rust host that owns the event loop, renders a documented HTML/CSS profile on the CPU and exposes native accessibility, driven by a separate Python or PowerShell process over a strict, authenticated IPC channel — is sound and worth keeping. Much of the implementation is careful: resources are bounded almost everywhere, the lifecycle logic is a pure and tested state machine, the platform `unsafe` code showed no soundness defects, the incremental pipeline is tested against a cold oracle, and vendored patches are small, documented and byte-verifiable.

**It is not yet a professionally stable library.** The audit confirmed on the published 1.0.1 artifacts two ways that ordinary use terminates an application, one way that a crafted file does, and an unbounded memory growth:

- **F-01:** the Windows host aborts with a stack overflow on **40 levels of nested `<div>`**, far inside the documented limit of 256. The executable has a 1 MiB main-thread stack and the box-tree builder uses ~24 KiB per level (~32 KiB for `<fieldset>`, which also overflows Linux's 8 MiB stack near 252). The bundled showcase app reaches depth 14, so typical apps have headroom; complex or generated layouts do not.
- **F-02:** **twenty buttons with colour transitions and one colour-scheme change disconnect the backend** and fail the application, on Windows (Python and PowerShell 5.1) and on Linux. Documents follow the OS colour scheme by default, so a user switching dark mode can trigger it without the app doing anything (code path; not run end to end). The host forwards every DOM event and treats a momentarily full 128-message queue as a dead peer. Once that is fixed, both bindings silently drop control notifications such as window `closed` under the same bursts (F-29).
- **F-03:** a **~10 KB SVG with 700 nested groups**, in a `data:` URL or a user-chosen image, aborts the host on both platforms. This needs a crafted file, but the fix is a depth counter.
- **F-04:** **issue #2's memory growth** is a font-identity bug: each time parley evicts and reloads a face, the renderer copies the whole font file again and never frees it (~0.3 MiB/s in the reported scenario).

None of these was caught by the 1,094 Rust tests or the headless suites. No test runs at the documented limits on a production-sized stack, headless mode runs no transitions, and the fuzz harnesses (which already generate nesting bombs) never run in CI and use 8 MiB stacks. Keyboard and accessibility behaviour has real gaps: focus never scrolls into view, Enter does not follow links, and caret, `<select>` and IME geometry are wrong inside scrolled modal dialogs and transformed containers. A long log freezes for about a second per appended line once something else on the page has updated a few times.

The release process produces artifacts that install and run on clean machines (verified), with good foundations: manual confirmed dispatch, OIDC Trusted Publishing with attestations, wheels built from the sdist, and a bit-for-bit reproducible Linux host. But nothing enforces CI before publishing (1.0.0 was published 21 s after its CI run started, without waiting; Windows CI later failed on test-only issues). A committed source-manifest check makes every non-release pull request fail CI and skip linting. The published Windows binary is a different build (from identical source) from the one qualified. Stability, compatibility and security policies do not exist yet.

**Maturity by dimension** (coordinator's judgement):

| Dimension | Rating | Basis |
| --- | --- | --- |
| Architecture | Good | sound process model; four specific weaknesses (event contract, recursion budget, geometry, headless fidelity) |
| Host / IPC / supervision | Good, one critical defect | strong framing, auth, lifecycle, process control; F-02 |
| Renderer correctness | Fair | honest incremental design; crash-depth, memory and geometry defects |
| Performance | Fair | fine for forms; O(document) updates and an append cliff for long lists |
| Accessibility | Fair | real native trees on both platforms; keyboard/focus, bounds and forced-colour gaps; no retained screen-reader evidence |
| Python binding | Good, with sharp edges | non-blocking transport, lifecycle workers; silent worker death, lossy control notifications |
| PowerShell binding | Fair | works on 5.1 and 7; same drop problems, reserved-name crash, AllUsers install defect |
| Security (realistic threats) | Fair | local-only, authenticated, bounded; crafted SVG/HTML depth and filter DoS |
| Testing | Fair | good unit tests; blind to limits, bursts, soak, geometry and GUI |
| CI / release engineering | Weak | red-by-default CI, ungated publish, rebuild-after-qualify, version literals, no policies |

**Production readiness:** usable today for internal tools and controlled deployments that keep Windows layouts shallow (well under ~30 levels), avoid transitions on colour properties of many elements (or pin the colour scheme with `set_environment`), do not display user-supplied SVG, and are restarted periodically. Not yet suitable for general distribution under a "Production/Stable" claim; the roadmap recommends `4 - Beta` until the stable gates pass.

## Answers to the completion questions

**1. How good is UI4ME Core today?** Architecturally good, operationally immature. The evidence is the confirmed defects F-01–F-04 and F-29 on the published artifacts against a large passing test suite, plus the release-process facts F-09–F-11.

**2. What are the most important actual or potential defects?** F-01 (Windows stack overflow at depth 40), F-02 (backend disconnect on event bursts, including OS theme switches), F-29 (bindings drop control notifications), F-03 (crafted-SVG abort), F-04 (font memory growth), F-07 (geometry in fixed/transformed boxes), F-08 (focus not scrolled into view), F-17 (Enter on links), F-16a (long-log append cliff). Draft PR #3 should not be merged as written (register §2.3).

**3. Which architectural decisions are strong, and which create risk?** Strong: the two-process model with a host-owned loop, declarative default actions, strict bounded protocol, generation-bound handles, process supervision, cold-oracle incremental pipeline, deterministic CPU raster, vendoring discipline. Risky: every DOM event sent to the backend with a count-based disconnect; native recursion not budgeted against stacks; five separate geometry computations; a headless mode that is not the native loop; font identity borrowed from a dependency's cache. See [03-architecture.md](03-architecture.md).

**4. What tests and verification are missing?** Depth-limit survival on production stacks, transport burst tests, binding control-notification tests, a headless virtual clock, allocation soaks, a geometry consistency property test, fuzzing in CI with small stacks and image-SVG generators, callback-robustness tests, a keyboard matrix, a Python 3.10–3.14 matrix, real-host Pester in CI, clean-machine install tests (including AllUsers), and a windowed GUI smoke in CI. Ranked in [04-testing-and-quality.md](04-testing-and-quality.md) §4.3.

**5. How reliable are the Python and PowerShell interfaces?** The Python transport is well designed (the reader never blocks on user code; lifecycle requests have their own workers) but a `sys.exit()` in a callback silently stops event delivery, control notifications are dropped under load, callback errors lack tracebacks, and Python 3.10 is claimed but untested (`test_bootstrap` cannot even import there). The PowerShell module works on 5.1 and 7 and fails F-02 at the same threshold. It loses its connection if a host message contains a member named `psbase` (for example an element attribute), drops events first-come, and is unusable by non-root users after an AllUsers install on Linux. Its real-host tests never run in CI. See [findings/BIND.md](findings/BIND.md).

**6. What cross-platform problems remain?** Confirmed: the Windows-only stack overflow (F-01); Linux supervised start failing on some environment variables (F-18); the AllUsers install problem on Linux (F-19). Documented, not re-verified: AT-SPI extents in physical pixels at fractional scale; Wayland focus and attention policies. From code, not run: Windows IME left enabled in password fields (INTERACT-09); shortcuts failing on non-Latin layouts (F-17b). Not covered: Windows 10/11 desktops, GNOME and other compositors, forced colours, bidi editing, locked-down Windows policies. A suspected `pythonw` console window (BIND-15) was tested and **not** confirmed. See INTERACT-15 and roadmap Phase 4.

**7. How professional and reproducible is the release process?** Partly. Strengths: manual confirmed dispatch, OIDC Trusted Publishing with PEP 740 attestations, wheels built from the sdist in CI, a reproducible Linux host, and PowerShell module promotion of released binaries. Failures: no CI gate on publishing; CI red for every non-release change and therefore no linting; Windows rebuilt after qualification; PSGallery 1.0.0 built from a different commit than its tag; the version hard-coded in ~61 files; hand-written evidence records citing deleted files; no changelog, compatibility or security policy; package links 404 because the repository is private. See [findings/RELEASE.md](findings/RELEASE.md).

**8. What should be fixed immediately?** Roadmap Phase 1 and items 6.0, 6.1 and 7.1 for 1.0.2:
- stack safety (run the event loop on an explicitly sized thread, a Windows reserve backstop, SVG and depth caps);
- a byte-bounded outbound queue with single-syscall frames, so a reading backend is never disconnected;
- bindings that never drop control notifications;
- the font identity fix;
- a CI-status gate on publishing, the manifest check moved out of PR CI, and clippy run unconditionally;
- artifact checks on the exact Windows wheel;
- honest release notes and a Beta classifier.

If capacity allows, add the focus-into-view, Enter-on-link and `sys.exit()`-in-callback fixes.

**9. What should be redesigned before the project grows?** Recursion safety as a tested invariant; one geometry service; headless as a faithful test backend; host-side event subscription (type-level, negotiated); and the release pipeline as build-once, qualify, promote with a single-sourced version. Reliable/event lanes, sequence numbers and loss markers should wait until a measured case needs them. See [03-architecture.md](03-architecture.md) §3.3 and roadmap Phases 2, 3 and 6.

**10. What should deliberately remain unchanged?** The two-process architecture, declarative default actions, strict protocol handling, generation-bound handles, the supervision design, cold-oracle testing, pixel-probe tests, the CPU raster, vendoring discipline, dual-role direct start, manual confirmed publishing with OIDC, and the commit history. See [08-non-recommendations.md](08-non-recommendations.md).

**11. What measurable criteria should define a professional stable release?** Gates R1–R10 and P1–P7, P12 and P13 in [05-release-readiness.md](05-release-readiness.md):
- no open Critical or High runtime defect;
- documented limits survivable without abort on production stacks;
- no disconnect and no lost control notification for bursts up to 10,000 events;
- bounded memory in a counting-allocator soak and a 30-minute windowed soak;
- bounded stalls for pathological inputs and reference documents;
- keyboard and geometry consistency;
- automated AT-API contract checks plus a manual screen-reader checklist;
- enforced green CI before publishing;
- published artifacts identical to the qualified ones;
- every claimed runtime and install mode tested;
- published policies and changelog.

**12. What is the shortest sensible roadmap to that standard?**
- **1.0.2**, about three engineer-weeks: Phase 1, items 6.0, 6.1 and 7.1, and tests T1, T2, T4, T7, T8, T9 and T16.
- **1.1**, about three to five engineer-months, which a single maintainer should ship as two minor releases: headless virtual clock, binding and lifecycle hardening, stall budgets, event subscription, the geometry service, keyboard fixes, the cheap large-document fixes, the CI matrix with nightly fuzz and soak runs, build-once-promote, a single-sourced version, and policies.
- **Stable claim** after the gates pass on one release, with a windowed GUI smoke in CI.

Larger performance work and reducing the Taffy fork (Phase 8) stay optional until measurements demand them. See [07-roadmap.md](07-roadmap.md).
