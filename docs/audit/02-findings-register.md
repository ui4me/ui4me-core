# 2. Findings register

This register consolidates the six area reports. Each consolidated finding (`F-nn`) names the area findings it merges. Full evidence, reproduction, remediation detail, alternatives and test designs live in the area files under [findings/](findings/); this page is the ranked index plus the coordinator's own verification. It was revised after the adversarial review ([09-adversarial-review.md](09-adversarial-review.md)).

**Severity rubric.** *Critical* = process crash or application failure triggered by benign content or ordinary use within documented limits, on a supported platform. *High* = the same consequence but requiring crafted input, or a serious functional, accessibility or data defect. *Medium* = real defect with a workaround or limited reach. *Low* = minor or hardening. **Likelihood** is rated separately and does not change severity. **Status** says how the finding was established; *reproduced (coordinator)* means re-run by the coordinating session on the published 1.0.1 artifacts on the named platform, with logs in [evidence/logs](evidence/logs/).

## 2.1 Runtime findings, ranked

| ID | Finding | Area refs | Sev. | Likelihood | Status | Phase |
| --- | --- | --- | --- | --- | --- | --- |
| F-01 | **Windows host aborts on ~40 levels of nested HTML.** The executable has a 1 MiB main-thread stack and the recursive box-tree builder uses ~24 KiB of stack per nesting level for `<div>` and ~32 KiB for `<fieldset>` (~4 frames of ~6 KiB per element). Documented limit: 256. `<fieldset>` also overflows Linux's 8 MiB main stack at ~252. | RENDER-02, INPUTSEC-01, INPUTSEC-07 | Critical | Low-Medium (the bundled showcase reaches depth 14; complex or generated layouts and tables reach 40) | Reproduced (coordinator, Windows: 20/30 pass, 40/60/120/250 abort) | 1 |
| F-02 | **Healthy backend disconnected on event bursts.** Every DOM event is forwarded; the per-connection outbound queue holds 128 messages; a full queue is treated as a dead peer. Kernel slack is ~104 frames on Linux because each frame is written with two syscalls. Each colour-scheme switch emits 18 events per button with 3 transitioned colours (6 longhands × 3 event types). | HOST-01, HOST-09, INTERACT-08, BIND-17, BIND-19, BIND-PS-02 | Critical | High: ≥ 20 transitioned controls plus a colour-scheme change, **including an OS dark-mode switch, which documents follow by default** (code path, not run end to end) | Reproduced (coordinator: Windows Python 3.13 and PowerShell 5.1.20348, Linux windowed; BIND: matrix in [log](evidence/logs/bind-burst-matrix-linux-windowed.txt)) | 1 |
| F-29 | **Bindings silently drop control notifications under bursts.** Python's 128-slot queues drop anything, including window `closed` and `document.error` (191 dropped, `closed` never seen; 22–28 % drops under GIL contention). PowerShell's first-come admission dropped 173 of 300 including the following `click`, and its deferral cap can drop `closed`. Once F-02 is fixed, these drops become the next failure. | BIND-02, BIND-PS-02, BIND-PS-03 | High | High once F-02 is fixed | Reproduced (BIND, headless and mock peer) | 1 |
| F-03 | **Crafted SVG aborts the host.** 700 nested `<g>` (or 2,000 chained `clip-path`) in a ~10 KB `data:` URL or a user-chosen image overflows the 2 MiB asset-worker stack. Image SVG has no nesting cap (inline SVG is capped at 48). | INPUTSEC-02 | High | Low (needs a crafted file; reachable in apps that display user-chosen SVG or set image URLs from user data) | Reproduced (coordinator, Linux and Windows: 500 pass, 700 abort) | 1 |
| F-04 | **Unbounded host memory growth (issue #2).** Each time parley prunes and reloads a font face, the renderer copies the whole font file (~0.5 MiB) into `system_blobs`, which is never pruned. | RENDER-01 | High | High for long-running apps that re-create content in a face not continuously in use | Reproduced (coordinator windowed; RENDER in-process with heaptrack) | 1 |
| F-06 | **The depth limit is bypassable** (`move_element` unchecked; `insert_html` measures the fragment as if at the root) → unbounded depth → abort on any platform. | RENDER-03 | Medium | Low (needs backend calls) | Reproduced (RENDER) | 1 |
| F-07 | **Geometry ignores `position: fixed` ancestors and CSS transforms** outside painting and hit testing: caret placement, `<select>` option picking, IME candidate position, `get_rect` and AT bounds are wrong in scrolled modal dialogs (`<dialog>` is fixed) and in translated containers (hand-rolled centred modals). PR #4 fixes one of five paths. | INTERACT-02, INTERACT-03 | High | Medium-High | Reproduced (INTERACT headless probe; PR #4 branch tested) | 3 |
| F-08 | **Keyboard focus never scrolls into view** (Tab, backend `focus`, AT `Focus`). | INTERACT-01 | High | High | Reproduced (INTERACT) | 2 |
| F-17 | **Enter on a focused link does nothing** (no `click`, no `link_activated`): keyboard users cannot follow links (WCAG 2.1.1, Level A). | INTERACT-05 | High | High for apps with links | Reproduced (INTERACT) | 2 |
| F-13 | **Python callback worker dies silently** on `BaseException` (`sys.exit()` in a Quit button) or an unencodable result; events and close decisions stop forever. Callback errors print one line without traceback; one failing handler skips the others. | BIND-01, BIND-03 | Medium | Medium-High | Reproduced (BIND) | 2 |
| F-28 | **PowerShell binding defects**: a host JSON member named `psbase`/`psobject`/… (e.g. an element attribute read with `get_attributes`) throws on the C# reader thread and loses the connection; `break`/`continue` in a callback escapes into module loops (policy → veto); `Start-UI4MEScript` from a helper re-runs the wrong file; Ctrl+C exits 0 instead of 130; a `-NoWait` dialog is cancelled by the default 15 s `Wait-UI4MERpc`. | BIND-PS-01, -04…-10 | Medium (PS-01) / Low | Low-Medium | Confirmed (BIND, pwsh 7.7 Linux with mock peer and real host; Windows not run) | 2 |
| F-14 | **Content-driven GUI-thread stalls**: uncapped SVG filters (`feTurbulence numOctaves=10000` ≈ 47 s), exponential shrink-to-fit inline-block layout (depth 26 ≈ 11 s), quadratic parse of deep unclosed blocks, superlinear `request_submit` (12 s at 1,000 fields, debug build; not re-measured in release). | INPUTSEC-03, -05, -06, HOST-06 | Medium | Low-Medium | Reproduced (INPUTSEC, HOST) | 2 |
| F-15 | **Paint-budget overflow is fatal to the document** (gradient > 1,024 stops, > 64 filter functions, scope nesting > 128 fail the whole frame; live the host drops the document). | INPUTSEC-04 | Medium | Medium | Reproduced (INPUTSEC) | 2 |
| F-16a | **Append cliff in long lists**: after 8 unrelated updates, appending a row reshapes every row (1.0–1.8 s at 2,000 rows) because replayed paragraphs are not touched in the content cache. Makes the documented long-log use case freeze per line. | RENDER-05 | High (for that use case) | High for log/chat apps with any other ticking text | Measured (RENDER, in-process harness) | 3 |
| F-16b | **O(document) incremental updates**: ~27–31 ms per `set_text` at 2,000 rows; cold shaping dominated by expanding `sans-serif` to 165 fontconfig families; full a11y tree rebuilt on every animation tick and caret blink (18 ms at 3k nodes), even with no AT connected. | RENDER-06, -07, INTERACT-04 | Medium | High for large documents | Measured (RENDER, INTERACT) | 3 / 8 |
| F-17b | **Shortcut and click semantics**: editing/clipboard shortcuts and Ctrl+Shift+Q match only the layout character (fail on non-Latin layouts; Windows behaviour inferred); no click when press and release land on different children of one button. | INTERACT-06, -07 | Medium | Medium | Reproduced headless (07); 06 inferred | 3 |
| F-12 | **Headless mode diverges from native**: no animation/transition clock (0 transition events), no flush-driven updates (journal growth ~0.38 KiB/op until a read). The only automated end-to-end mode cannot see F-02 or the windowed half of F-04. | coordinator E1, RENDER-04 | Medium | Certain | Reproduced | 2 |
| F-18 | **Linux launch fails on some environments**: any env var name systemd rejects (`BASH_FUNC_x%%` from `export -f`, Lmod) or a non-UTF-8 value aborts supervised start. | HOST-03 | Medium | Medium (environment-dependent) | Reproduced (HOST) | 2 |
| F-19 | **PowerShell AllUsers install on Linux unusable by non-root users** (nupkg drops the exec bit; `chmod u+x` at first use). | RELEASE-06, BIND-PS-11 | Medium | High on shared machines/containers | Reproduced (RELEASE, container) | 4 |
| F-25 | **Host lifecycle edge cases**: listener dies permanently on any accept error (EMFILE reproduced); SIGTERM/session end skips `app.shutdown`; relaunch during shutdown fails; error answer to close/quit (incl. "queue full") counts as veto; log flood after disconnect. | HOST-04, -07, -08, -10, -15 | Low-Medium | Low-Medium | Reproduced (HOST) | 2 |
| F-26 | **Direct-start DX defects**: backend `print()` lost under load (155 of 20,000 lines reached the console, 512-line lossy ring); first `open_html` failure cancels the app; a second `Application.start()` recursively spawns hosts; `host_binary()` can exec its own console script in a loop. | BIND-04, -05, -06, -09 | Low-Medium | Medium | Reproduced (BIND) | 3 |
| F-27 | **Vendored Taffy fork carries ~1,800 added lines** (subgrid ~1,400); patch verification and dependency scans not in CI; 4 unmaintained advisories, 0 vulnerabilities. | RENDER-12, INPUTSEC-08, -09 | Low-Medium (liability) | Medium over time | Proven; `verify.sh` passes | 8 |

## 2.2 Process and release findings

Rated on their own scale: consequence for release integrity and maintainability, not for end users directly.

| ID | Finding | Area refs | Sev. | Status | Phase |
| --- | --- | --- | --- | --- | --- |
| F-10 | **Publishing is not gated on CI.** 1.0.0 was published 21 s after its CI run started, without waiting; Windows CI later failed (test-only failures, artifacts probably unaffected). No workflow check, no branch protection (unavailable on the current plan for a private repo), no reviewers on publish environments. | RELEASE-01, RELEASE-12 | High | Proven from run history | 6 |
| F-09 | **CI is red for every non-release change**, as a side effect of a committed source-manifest check, and the red step causes clippy to be skipped: no PR has ever been linted. | RELEASE-02 | Medium | Reproduced (CI runs on PR #3/#4) | 6 |
| F-11 | **Published Windows binary ≠ qualified Windows binary** (built from identical source, different build); PSGallery 1.0.0 built from a commit other than its tag. | RELEASE-03, RELEASE-04 | Medium | Proven (hashes, run history) | 6 |
| F-20 | **All public package links 404** while the repository is private. | RELEASE-05 | Medium | Reproduced | 7 |
| F-21 | **Version hard-coded in ~61 files** including workflows, verifiers and `host.manifest` (`0.1.0.0`). | RELEASE-07 | Medium | Proven | 6 |
| F-22 | **CI narrower than claims**: Python 3.12 only (3.10 claimed; `test_bootstrap` cannot import there); real-host Pester skipped in all CI; no GUI test of any kind. | RELEASE-08, BIND-12 | Medium | Proven | 5 |
| F-23 | **Supply-chain hygiene**: 42 unpinned actions (publisher action on a branch), no dependency scan, no SBOM, no build provenance for GitHub assets, unsigned Windows binary, mutable releases. | RELEASE-09, INPUTSEC-08 | Medium | Proven (zizmor, `cargo deny`) | 6 |
| F-24 | **Release evidence not durable; no policies**: records are hand-written summaries referencing deleted files; "Production/Stable" without compatibility, changelog, security or yank policy. | RELEASE-10, RELEASE-11 | Medium | Proven | 6 / 7 |

## 2.3 Review of open pull requests (advisory, not a defect)

- **PR #3 (`fix/drop-saturated-notifications`) — do not merge as written.** It routes `app.shutdown`, window `closed`, `request.cancelled`, `document.error` and click/input events through a silent drop path, while responses and close/quit requests still disconnect on a full queue (HOST-02). The 1.0.2 fix in [07-roadmap.md](07-roadmap.md) 1.2 removes the count cap instead of dropping anything.
- **PR #4 (`fix/fixed-ancestor-viewport-rect`) — correct but incomplete.** It fixes AT/element bounds; caret, select, IME and text-line geometry inside fixed boxes remain wrong on the branch (INTERACT-02). Rebase it onto the shared geometry service (roadmap 3.2) or extend it to all five paths.

## 2.4 All area findings (index)

Severity as assessed by the area specialist; where the coordinator re-rated, the consolidated rows above govern.

| Area | ID → severity |
| --- | --- |
| HOST | 01 High→**Critical (F-02)** · 02 High→advisory (§2.3) · 03 Medium · 04 Low-Med · 05 Low-Med · 06 Low · 07 Low-Med · 08 Low · 09 Low · 10 Low · 11 Low · 12 Low · 13 Low · 14 Low · 15 Low-Med · 16 Low · 17 Low |
| RENDER | 01 High · 02 High→**Critical (F-01)** · 03 Medium · 04 Low · 05 Medium→**High (F-16a)** · 06 Medium · 07 Medium · 08 Low-Med · 09 Low · 10 Low (test gap) · 11 Low · 12 Low-Med · 13 Low |
| INTERACT | 01 High · 02 High · 03 High · 04 Medium · 05 Medium→**High (F-17)** · 06 Medium · 07 Medium · 08 Medium · 09 Low (Windows IME in password fields, not run) · 10 Low · 11 Low · 12 Low · 13 Low · 14 Low · 15 Low |
| BIND | 01 Medium · 02 Medium→**High (F-29)** · 03 Medium · 04 Medium · 05 Medium · 06 Low-Med · 07 Low-Med · 08 Low-Med · 09 Low-Med · 10 Low · 11 Low · 12 Low · 13 Low · 14 Low · 15 **not confirmed** (coordinator test) · 16 Low · 17 Medium · 18 Low · 19 High (=F-02) · PS-01 Medium · PS-02 Medium (=F-29) · PS-03 Medium-Low (=F-29) · PS-04…06 Low-Med · PS-07…19 Low |
| INPUTSEC | 01 High (=F-01) · 02 High (=F-03) · 03 Medium · 04 Medium · 05 Low-Med · 06 Medium · 07 High (=F-01) · 08 Low · 09 Low-Med · 10 Low · 11 Low |
| RELEASE | 01 High · 02 High→Medium (F-09) · 03 Medium · 04 Medium · 05 Medium · 06 Medium · 07 Medium · 08 Medium · 09 Medium · 10 Medium · 11 Medium · 12 Low-Med · 13 Low · 14 Low · 15 Low · 16 Low · 17 Low · 18 Low · 19 Low-Med |

## 2.5 Coordinator verification and corrections

Claims were re-run on published 1.0.1 artifacts where a platform was needed. Corrections to specialist, issue or coordinator claims:

1. **Issue #1 says Linux survived.** It does not: in a windowed Linux session (nested KWin) 20 transitioned buttons fail on the third switch and 100 on the first. Linux only "survives" headless, where no transitions run (E1–E2).
2. **INPUTSEC-02 says the Windows asset workers overflow "far shallower".** They do not: Rust gives spawned threads a 2 MiB stack on every OS; only the main thread inherits the 1 MiB PE reserve. The SVG threshold is the same on both platforms (E8–E9).
3. **RENDER-02 was a "reproducible risk" for lack of a Windows machine.** It is a confirmed defect on the published binary (E7).
4. **BIND-15 (`pythonw` opens a console window) was reasoned from code.** Tested on Windows Server 2022 with uv-managed Python 3.13: the host runs, no console window and no new `conhost` process appear (E12). Not confirmed.
5. **The coordinator's early hypothesis that the paint path causes issue #2 was wrong**: the growth is in text shaping during layout, which windowed frames trigger and headless skips (F-04).
6. **Two coordinator observations were retracted after measurement**: a host apparently outliving a killed launcher (teardown takes 0.28 s, E11), and `pythonw` apparently not starting the app (the probe script read `/proc`, which does not exist on Windows; a portable probe started normally, E12).
7. **Stack cost figure.** An early draft quoted "~6 KiB per element"; that is per stack frame. The per-level costs are ~24 KiB (`div`) and ~32 KiB (`fieldset`).

## 2.6 Reproduction quick reference

| Finding | Minimal reproduction |
| --- | --- |
| F-01 | `python depth.py 40` on Windows with the 1.0.1 wheel ([script](evidence/scripts/depth.py)); stderr `thread 'main' … has overflowed its stack` |
| F-02 | `python burst2.py 20 6 env` in a desktop session ([script](evidence/scripts/burst2.py)); PowerShell: `powershell -File burst.ps1 -N 20` ([script](evidence/scripts/burst.ps1)) |
| F-29 | BIND experiments `e*_events.py` / PowerShell `burst_client.ps1` in [evidence/scripts/bind](evidence/scripts/bind/) (see BIND-02, PS-02) |
| F-03 | `python svgdepth.py 700` ([script](evidence/scripts/svgdepth.py)) |
| F-04 | `LEAK_CSS=custom LEAK_CSSTEXT='.u4-badge { font-weight: 600; }' python issue2_replay.py` with `doc.html` and `region.html` alongside ([scripts](evidence/scripts/)) |
| F-07, F-08, F-17 | `interact-probe` cases in [evidence/scripts/interact-probe](evidence/scripts/interact-probe/) |
| F-09 | any PR's CI run, e.g. GitHub run 43625326001 |
| others | see the area file |
