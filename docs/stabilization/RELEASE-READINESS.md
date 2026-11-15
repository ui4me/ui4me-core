# UI4ME Core 1.0.2 release readiness

Status at the head of `stabilization/integration`. Per-finding detail is in [MATRIX.md](MATRIX.md) (generated from
[tracker.json](tracker.json)); evidence is under [evidence/](evidence/).

## Verdict

**The core library is fit for a 1.0.2 beta release candidate.** Every Critical and High finding from the 1.0.1 audit is
fixed, has a regression test that failed on the baseline, and passes on Linux and on Windows. **It is not yet fit to
publish.** Publishing requires green GitHub Actions runs for the release commit (gates P1, P2, P6, P8). Those runs have
not happened yet, because CI runs on `main` and pull requests and nothing was merged or published.

Scope was narrowed during the work to core-library correctness: the host, the renderer, document and assets crates, and
the in-repo Python and PowerShell bindings. The rest is deferred and listed below.

## Findings

| Status | Consolidated (F-xx) | Specialist |
| --- | --- | --- |
| Fixed and verified | 26 | 74 |
| Blocked on a GitHub Actions run | 2 (F-11, F-22) | 6 |
| Deferred | 1 (F-27) | 31 |
| Not reproduced | 1 (F-20, repository now public) | 2 |

No open finding is Critical or High. The largest deferred runtime items are Medium: RENDER-07 (O(document) incremental
update, about 27 ms at 2,000 rows), INTERACT-08 (input event amplification) and BIND-05 (direct-start print relay).

## Adversarial review

After integration, independent reviewers read the host, bindings and renderer diffs without the authors' conclusions.

- **Host:** 7 findings. #1–#5 fixed (merge `ed7e474`). #6 (pipe server recreate) and #7 (Win32 posted-message
  starvation) are deferred.
- **Bindings:** 9 findings, all fixed (merge `a8550a0`).
- **Renderer and assets:** 7 findings. Six fixed; #6 (scroll into view under scale/rotate) is deferred. In #7, the
  per-use pattern count was already correct, and the fix covers an under-count through `use`.
- **Fuzz on the integration head:** found two `paginate` hangs. Both were also present in the 1.0.1 baseline, and both
  are now fixed (`082a27b`).

## Gates

| Gate | Status | Gate | Status |
| --- | --- | --- | --- |
| R1 no open Critical/High | MET | P1 green required CI | BLOCKED (GitHub run) |
| R2 limits without abort | MET | P2 ordinary changes green | BLOCKED (GitHub run) |
| R3 burst soak | PARTIAL (100×6 bursts pass; 60 s soak not run) | P3 publish gated | MET |
| R4 memory | PARTIAL (15-min windowed replay flat) | P4 qualified = published | PARTIAL (F-11) |
| R5 stalls | PARTIAL (one case 0.95 s; RENDER-07) | P5 traceable to tag | PARTIAL (PSGallery 1.0.0 history) |
| R6 keyboard | MET | P6 runtimes in CI | BLOCKED (GitHub run) |
| R7 geometry | MET | P7 install modes | PARTIAL |
| R8 assistive tech | PARTIAL (no NVDA/Narrator) | P8 advisories in CI | BLOCKED (GitHub run) |
| R9 lifecycle | PARTIAL (WM_ENDSESSION not exercised) | P9 workflows hardened | MET |
| R10 bindings never silent | MET | P10 provenance | PARTIAL (exercised at release) |
| | | P11 single version source | MET |
| | | P12 changelog, notes, policy | MET |
| | | P13 public links | MET |
| | | P14–P18 | MET (unchanged) |

## Final verification

The head of `stabilization/integration` was verified on Linux and on Windows Server 2022. Details are in
[evidence/lead/final-f7f0ab8](evidence/lead/final-f7f0ab8/README.md).

- **Linux:** `tools/verify.py` passed all default stages, and clippy passed. The 7 fuzz harnesses (2,000 cases each)
  found nothing.
- **Windows:** the release host was built with a 32 MiB stack reserve. These passed:
  - host tests;
  - the stack, stall, SVG and renderer suites;
  - Pester with the real host on PowerShell 5.1 and 7.4: 74 passed, 0 skipped;
  - Python 3.10 and 3.13: 107 tests each;
  - bursts: no control or discrete event dropped;
  - Ctrl+Break: exit 130.
- **Earlier native evidence, still current:**
  - real-input interaction on Windows and Linux;
  - fcitx5 IME and Orca on Linux;
  - local RC qualification of the 1.0.2 wheel on Windows (`evidence/lead/windows-rc-1.0.2.md`).

The changes since then do not affect the paths they covered. The fixes to the click common ancestor and to AltGr
were verified by unit tests only, not by a new native run.

## Before and after

See the table in [RELEASE_NOTES_1.0.2.md](../release/RELEASE_NOTES_1.0.2.md). In summary:

- Windows deep documents went from an abort at about 40 levels to rendering at 250.
- Issue #2 memory went from 58→296 MiB in 15 minutes to a flat 44–55 MiB.
- A long-list append went from 691–750 ms to 54–55 ms.
- The worst content stalls went from 47 s to 0.2 s.

## Before publishing

1. Open a pull request from `stabilization/integration` to `main`. CI then runs every required job on Linux and
   Windows. Fix what fails, and record the runs against F-22, P1, P2, P6 and P8.
2. Build the Windows wheel in CI, qualify that exact wheel natively, and publish it (F-11, P4).
3. Merge, tag `v1.0.2`, and publish through the gated workflows. Each step needs the owner's authorization.

## Deferred

All rows are in [MATRIX.md](MATRIX.md) with reasons. In short:

- the SDK and non-core tooling;
- RENDER-07, RENDER-08, RENDER-09 (incremental performance);
- INTERACT-08, -09, -11 to -15;
- BIND-05, -07, -08, -14, -16, -18 and BIND-PS-10 to -19 (ergonomics and diagnostics);
- HOST-12 (pipe SID check, `KillMode=mixed`);
- INPUTSEC-09, -10;
- F-27 (Taffy fork size);
- the review items listed above;
- the 60-second burst soak (R3), the 30-minute windowed soak in CI (R4) and NVDA/Narrator checks (R8).
