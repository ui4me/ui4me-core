# 5. Release-readiness assessment

## 5.1 Verdict

**1.0.1 does not meet the bar its "Production/Stable" classifier implies.** It works for small and medium apps on the qualified desktops, and its artifacts install and run on clean machines. But content- or usage-triggered failures terminate applications on supported platforms (F-01, F-02; F-03 with crafted input), memory grows without bound in a realistic long session (F-04), and the release process cannot guarantee that a published version passed its own CI (F-10) or that the published Windows binary is the build that was qualified (F-11).

The gates below are objective: each can be checked by a script or a retained artefact. They were recalibrated after the adversarial review so that a correctly fixed build can actually pass them.

## 5.2 Runtime gates

| # | Gate | Status at 1.0.1 | Evidence |
| --- | --- | --- | --- |
| R1 | No open Critical or High **runtime** defect (§2.1 of the register), triaged with labels and a milestone. Process findings are covered by the P-gates. | **FAILING** | F-01, F-02, F-29, F-03, F-04, F-07, F-08, F-17, F-16a open; issues have no labels or milestones |
| R2 | Documents at the documented limits (depth 256 for `div`, `fieldset`, `table`, flex, `display: contents`; 65,536 nodes; image SVG at the pre-scan caps) render **without aborting** on the production stack on every platform. Abort-free, not time-bounded (time is R5). `inline-block` joins once 2.5 lands. | **FAILING** | Windows aborts at depth 40 (E7); `fieldset` aborts at ~252 even on Linux (INPUTSEC-01); SVG at 700 nesting aborts both (E8–E9) |
| R3 | Bursts of up to 10,000 events (or 8 MiB) in one GUI turn, and a sustained 2,000 events/s for 60 s, cause no disconnect of a reading backend and no dropped **control** notification in either binding | **FAILING** | 20 transitioned controls kill the app (E2–E4); bindings drop `closed` (F-29) |
| R4 | Memory: (a) in-process counting-allocator soak — live heap grows < 1 MiB over 20,000 mixed operations after the first 2,000 (deterministic, in CI); (b) nightly 30-minute windowed soak — host RSS over the last 20 minutes grows < 5 MiB or < 5 %, whichever is larger. Warm-up is defined by operation count. | **FAILING** | issue #2: ~18 MiB per minute (E5); the non-leaking control grows ~1.3 MiB per 10 min, which is why (b) is not tighter |
| R5 | Stalls, release build: (a) every pathological input class (SVG filters, nested shrink-to-fit, unclosed-block parse, `request_submit` on many fields) is bounded to < 250 ms or rejected cleanly; (b) on reference documents (a 200-control form, a 2,000-row log) any single update, and an append after 20 unrelated updates, stalls < 100 ms; (c) frame gap < 50 ms while 1,000 queued commands drain | **FAILING** | SVG filter ≈ 47 s, shrink-to-fit 11 s at depth 26 (F-14); append cliff 1.0–1.8 s (F-16a); `request_submit` to be re-measured in release |
| R6 | Keyboard: every interactive element reachable and operable; focus scrolled into view | **FAILING** | F-08, F-17 |
| R7 | Geometry consistency: caret, selection, IME area, AT bounds and `get_rect` agree with hit testing under scroll, fixed positioning and transforms | **FAILING** | F-07 |
| R8 | Assistive technology: automated AT-API contract checks on the release artifacts (AT-SPI and UIA trees: roles, names, states, focus-changed and live-region events), plus a documented 15-minute manual Orca and NVDA checklist per minor release with notes attached | **UNVERIFIED** | lab ledgers exist for earlier binaries; raw evidence not retained (RELEASE-10) |
| R9 | Backend crash/freeze/veto, host signals and session end handled as documented | **PARTIAL** | faults stage passes; SIGTERM skips `app.shutdown` (HOST-07) |
| R10 | Bindings never die silently on user-code errors | **FAILING** | F-13, F-28 |

## 5.3 Process and artifact gates

| # | Gate | Status | Evidence |
| --- | --- | --- | --- |
| P1 | Release commit has green required CI on every supported OS, enforced by the publish workflow | **FAILING** | F-10 |
| P2 | Ordinary changes can be green; lint always runs | **FAILING** | F-09 |
| P3 | Publish requires green required CI for the SHA (P1) and an explicit manual dispatch. Branch protection and reviewer gates are adopted when the plan or visibility allows and a second maintainer exists. | **PARTIAL** | manual typed-phrase dispatch exists; P1 missing |
| P4 | Published artifacts are exactly the qualified artifacts (build once, qualify, promote) | **MET Linux / FAILING Windows** | F-11 |
| P5 | Every package version traceable to its tag (all channels) | **MET PyPI / FAILING PSGallery 1.0.0** | RELEASE-04 |
| P6 | Every claimed runtime (Python 3.10–3.14, pwsh 7.2+, PS 5.1) tested in CI against the real host before publishing | **FAILING** | F-22 |
| P7 | Every documented install mode works | **FAILING** | F-19 |
| P8 | Dependency advisories and licences checked in CI | **FAILING (not in CI) / content OK** | 0 vulnerabilities, 4 unmaintained |
| P9 | Workflows hardened (SHA-pinned actions, no template injection, current runtimes) | **FAILING** | zizmor: 42 unpinned, 2 injections |
| P10 | Provenance: PyPI attestations, build provenance for GitHub assets, SBOM | **PARTIAL** | PEP 740 present; no build provenance, no SBOM |
| P11 | Version single-sourced; a release does not edit CI | **FAILING** | F-21 |
| P12 | Cumulative changelog, accurate release notes, compatibility/deprecation policy, SECURITY.md, yank procedure | **FAILING** | F-24 |
| P13 | Public metadata links resolve | **FAILING** | F-20 |
| P14 | MPL-2.0 source availability for modified Stylo | **MET** | sdist contains `patches/stylo/**` and `MPL-2.0.txt`; wheel `THIRD_PARTY_LICENSES.txt` points to it (coordinator check) |
| P15 | Fresh contributor can build and test from the docs | **MET** (one hermeticity bug) | Debian 13 container: host built in 4 min 41 s, 119/120 tool tests pass (RELEASE-16) |
| P16 | Published artifacts work on clean machines | **MET (by this audit, not by process)** | wheels on Python 3.10/3.14 containers; module CurrentUser on pwsh 7.4 |
| P17 | Vendored dependencies verifiably equal upstream plus documented patches | **MET manually / not in CI** | `verify.sh` OK; `third_party` pristine commits equal crates.io |
| P18 | Secrets least-privilege | **MET** | OIDC for PyPI; Gallery key scoped to one environment |

## 5.4 What each release should require

**1.0.2** (roadmap Phase 1, 6.0, 6.1, 7.1 notes): R2 and R3 met; R4 met for the issue #2 scenario; tests T1, T2, T4, T7, T8, T9, T16 in CI; P1 and P2 met; the stack-reserve and depth/SVG/burst checks run against the exact Windows wheel to be published (6.0); release notes state the open High items (F-07, F-08, F-17, F-16a unless fixed); PyPI classifier `4 - Beta`.

**A "Production/Stable" claim** requires R1–R10 (R8 in its automated-plus-checklist form) and P1–P7, P12 and P13. P8–P11 are strongly recommended good practice but do not change what users experience, so they are not preconditions for the claim.
