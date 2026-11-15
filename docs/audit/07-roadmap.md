# 7. Prioritised roadmap

Each item lists priority (P0 = before the next release, P1 = next minor, P2 = when the project grows), rationale, dependencies, effort and objective acceptance criteria. Effort is a rough size for one experienced engineer: **S** ≈ up to a few days, **M** ≈ up to two weeks, **L** ≈ more. Treat these as orders of magnitude, not estimates. Finding IDs point to the register and the area files, which hold implementation-level remediation detail.

The project has one maintainer (all 34 commits are from one account). The plan is sized for that, and it stays deliberately conservative about new machinery.

## Release plan at a glance

| Release | Content | Rough size |
| --- | --- | --- |
| **1.0.2** | 1.1–1.4, 6.1, the Windows artifact checks in 6.0, the release-notes and classifier change in 7.1, tests T1, T2, T4, T7, T8, T9, T16; if capacity allows 1.5 | about three engineer-weeks including fixes and tests |
| **1.1** (plan as two minor releases for one maintainer) | Phases 2–3 and 5, plus 6.3–6.5 and 7.1 policies | about three to five engineer-months |
| **Stable claim** | the gates in [05-release-readiness.md](05-release-readiness.md) §5.4 pass on one release | — |

## Phase 1 — Critical correctness and safety (1.0.2)

### 1.1 Stack safety for content depth — P0, M (F-01, F-03, F-06)
- **Do:**
  - (a) **Run the event loop on an explicitly sized thread on both OSes** (preferred). `main` spawns one thread with `std::thread::Builder::stack_size(32 << 20)` that builds the event loop with `with_any_thread(true)` (available in the vendored winit-win32 `event_loop.rs:172-231` and winit-wayland `lib.rs:83`) and runs the host; `main` joins it and propagates the exit code and panics. Re-check COM/OLE initialisation (file dialogs, drag and drop) and AccessKit adapter creation on that thread. This removes the dependence on the PE reserve and on `ulimit -s`.
  - (b) Belt and braces on Windows: also emit `/STACK:33554432` from `crates/ui4me-host/build.rs` under `CARGO_CFG_TARGET_ENV == "msvc"` (`cargo:rustc-link-arg-bins`), so any code that still runs on the process main thread has the same budget. On x64 this reserves address space only. Assert `SizeOfStackReserve` in `tools/release/verify_windows_artifact.py`.
  - (c) Spawn asset decode workers with an explicit `stack_size` (e.g. 8 MiB) in `ui4me-assets/src/worker.rs`.
  - (d) In `svg/prescan.rs`, reject image SVG whose element nesting exceeds a bound (e.g. 128, generous for real icons) with `DECODE_FAILED`, and bound `clip-path`/`mask`/`use` reference chains.
  - (e) Check depth in `MoveElement` (`depth(target) + height(subtree)`) and in `InsertHtml` (`depth(parent) + height(fragment)`) in `document.rs::apply_op`.
  - (f) Shrink the hottest frames: pass `&BoxStyle` (the style maps already hold `Arc<BoxStyle>`) instead of by value in `layout/box_tree/build.rs`, and move large temporaries into `Box`. Measure `div`, `fieldset`, `table`, flex and `display: contents`.
- **Dependencies:** none.
- **Acceptance:** T1 passes in release profile on Windows and Linux CI: the documented maximum depth (256) for `div`, `fieldset`, `table`, flex and `display: contents`, on a thread with **half** the configured engine stack (16 MiB), without abort; SVG at the pre-scan cap on half the worker stack. `inline-block` is excluded until 2.5 removes the exponential layout (T1 is about aborts, not time). `move_element`/`insert_html` beyond the limit return `SIZE_LIMIT`. On the **published** Windows wheel, `depth.py 250` renders and `svgdepth.py 700` yields a clean decode failure.

### 1.2 Never disconnect a reading backend — P0, S (F-02)
- **Do:** Do **not** merge PR #3 as written (§2.3 of the register). Instead, the smallest change that meets R3:
  - (a) Bound the outbound queue by **bytes only**. The 8 MiB per-connection byte cap already exists (`transport.rs:18`, enforced at `:211-214`); the binding limit is the 128-message `sync_channel` (`:17`, `:273`). Replace it with an unbounded channel guarded by the existing byte counter (a very large bounded `sync_channel` preallocates its slots). At the measured 360–600 B per event, 8 MiB holds ~14k–23k events.
  - (b) Write each frame with one syscall (header and body in one buffer or `write_vectored`), on both platforms (HOST-09). This raises Linux kernel slack from ~104 to ~167 frames.
  - (c) After the first send failure, mark the connection dead and log once with a counter (HOST-08).
  - A backend that stops reading is still disconnected by the existing 10 s write deadline or by the byte cap; nothing is dropped.
- **Dependencies:** none. 3.1 builds on it.
- **Acceptance:** T2 passes (10,000 events plus interleaved responses at a reader of 1 frame/ms: no disconnect, every message delivered in order). `burst2.py 100 6 env` passes on Windows (Python and PowerShell 5.1) and Linux windowed. The existing "peer stops reading" test still disconnects.

### 1.3 Bindings never drop control notifications — P0, M (F-29)
- **Do:** In both bindings, classify incoming notifications. Control notifications (`window.event` `closed`/`close_requested`, `document.error`, `app.shutdown`, `app.activated`, `request.cancelled`) bypass the bounded queues and their bookkeeping (retiring windows/documents) is applied eagerly on the reader thread. Coalescible events (pointer moves, wheel, scroll, resize) are dropped first when a queue is full; discrete DOM events are dropped last; every drop is counted and the first drop per burst is logged. PowerShell: admit events by subscription interest on the reader thread instead of first-come (BIND-PS-02), so unsubscribed events never occupy admission slots.
- **Dependencies:** 1.2 (otherwise the host disconnects first).
- **Acceptance:** T16: during a 1,000-event burst, `request_close()` retires the window and `dropped_notifications` counts no control notification, in Python and Windows PowerShell 5.1; a `click` after a 300-event burst is delivered in both bindings.

### 1.4 Font identity / issue #2 — P0, S (F-04)
- **Do:** In `text/fonts/environment.rs` use `SourceCache::new_shared()` instead of `SourceCache::default()` (`:269`), keep the fontique blob alive instead of copying it into `system_blobs` (`:323-328`), and bound `system_blobs` by last-used pass.
- **Acceptance:** T4 passes (stable blob id across a forced source-cache prune; counting-allocator soak grows < 1 MiB after warm-up). The issue #2 bold-variant replay stays flat (gate R4) windowed on Linux and Windows.

### 1.5 Cheap high-value fixes, if capacity allows — P0-optional, S each (F-08, F-17, F-13)
- **Do:** scroll the focused element into view (nearest) on keyboard, programmatic and AT focus, not on pointer focus; Enter on a focused `<a href>` activates it; in the Python binding catch `BaseException` per callback, turn `SystemExit` into `request_quit()` with the exit code, print full tracebacks, and continue with the remaining handlers.
- **Acceptance:** the corresponding cases of T9 and T10 pass.

## Phase 2 — Reliability and regression prevention

### 2.1 Headless as a faithful test backend — P1, M (F-12)
- **Do:** run the normal frame pump in headless mode against an off-screen surface, driven by a virtual clock; add a test-mode-only `test.advance_time {ms}` request; flush pending updates on each pump as native mode does.
- **Acceptance:** in headless, `burst2.py` receives transition events; 10,000 `set_text` without reads keeps RSS bounded (RENDER-04); T3 runs in CI.

### 2.2 Binding robustness, remaining items — P1, S each (BIND-03, -06, -11, F-28)
- **Do:** Python: map host `-32004` to `StaleDocumentError`; let a failed first `open_html` raise without cancelling the app. PowerShell: build objects so reserved member names (`psbase`, `psobject`, `psadapted`, `psextended`, `pstypenames`, empty) cannot throw on the reader thread (PS-01); contain `break`/`continue` and require an explicit boolean from policies (PS-08/09); resolve the script path in `Start-UI4MEScript` from the calling script (PS-06); exit 130 on Ctrl+C (PS-07). Write the shared rules into `protocol.md` as a "writing a binding" checklist.
- **Acceptance:** T9 complete; an element with a `psbase` attribute read through `Get-UI4MEAttributes` works.

### 2.3 Host lifecycle hardening — P1, S each (F-18, F-25)
- **Do:** filter env names/values that systemd rejects in `bootstrap_supervised`; make the accept loop retry transient errors (EMFILE, ENFILE, ECONNABORTED; Windows `ERROR_NO_DATA`, `ERROR_PIPE_BUSY`); handle SIGTERM/SIGINT/SIGHUP and Windows `WM_QUERYENDSESSION`/`WM_ENDSESSION` by starting an orderly quit with a short deadline; treat error answers to `before_close`/`before_quit` as "no decision" rather than a veto; let a relaunch during shutdown wait for the instance lock; strip host-private `UI4ME_*` variables from the backend environment; re-wake the loop when the pump budget is exhausted (HOST-05); add a panic hook. When making zero-parameter methods strict (HOST-17), keep `host.hello` open to unknown keys (or accept an explicit `capabilities` object) so newer bindings can negotiate with older hosts.
- **Acceptance:** one test per item (T15); `BASH_FUNC_x%%` in the environment no longer prevents start.

### 2.4 GUI-thread time budget — P1, S (HOST-05, HOST-06)
- **Do:** process backend messages until ~8 ms have elapsed, then yield and re-wake; log any single command over 100 ms with its op name; re-measure `request_submit` on many required fields in a release build and fix the superlinear path.
- **Acceptance:** gate R5(c): frame gap < 50 ms while 1,000 queued commands drain.

### 2.5 Remaining content-stall and paint-budget paths — P1, S–M (F-14, F-15)
- **Do:** memoise shrink-to-fit intrinsic sizing per box per pass (removes the exponential inline-block case; then add `inline-block` to T1); cap SVG filter cost (`numOctaves`, primitives × area); make paint-budget overflow skip the offending item instead of failing the document; fix the quadratic unclosed-block parse.
- **Acceptance:** gate R5(a): each INPUTSEC-03…06 reproduction finishes in < 250 ms (release) or is rejected cleanly; no document is dropped for exceeding a paint budget.

## Phase 3 — Architecture and API improvements

### 3.1 Host-side event subscription — P1, M (F-02 long-term)
- **Do:** client capability in `host.hello`; `set_event_types` command (document-wide, by event type); the renderer installs root listeners only for subscribed types; bindings send their handler set on `on()`/unsubscribe. This saves GUI-thread time and matters most for PowerShell, whose pump costs ~0.65 ms per unsubscribed event (~1.2 s per theme switch at 100 controls). Per-element subscription, separate reliable/event lanes, sequence numbers and an `events_lost` marker are **not** needed while the host never drops anything (1.2); add them only if a measured case requires the host to drop discrete events.
- **Dependencies:** 1.2, 1.3.
- **Acceptance:** an app subscribed only to `click` receives zero transition and pointer events (counter); a client without the capability behaves as in v1.

### 3.2 One content→window geometry service — P1, M–L (F-07)
- **Do:** one function maps fragment-local coordinates to window coordinates (scroll chain to the nearest fixed ancestor, transforms); caret hit, select option hit, IME cursor area, a11y bounds and `get_rect` all use it. Rebase PR #4 onto it.
- **Acceptance:** T5 passes for fixed, sticky, nested scrollers, translate/scale and a centred modal after scrolling.

### 3.3 Remaining keyboard and pointer semantics — P1, S–M (F-17b, rest of F-08)
- **Do:** match shortcuts on the physical key code as a fallback for non-Latin layouts; click target = nearest common activatable ancestor of press and release; complete the keyboard activation matrix.
- **Acceptance:** T10 passes.

### 3.4 Cheap large-document fixes — P1, S each (F-16a, part of F-16b)
- **Do:** touch content-cache entries of replayed/seeded inline containers each pass (`ContentCache::touch` from `seed`/`adopt_results`); memoise `resolve_families` per (families, face-table generation) and truncate system generic expansion; rebuild the accessibility tree only when an AT adapter is active and only for semantic changes.
- **Acceptance:** T13 counters: an append after 20 unrelated updates shapes ≤ 2 paragraphs; 100 animation ticks with no AT connected rebuild the a11y tree 0 times; gate R5(b) on the reference documents.

### 3.5 Python API polish — P2, S (BIND-13, -16, -07, -09)
- **Do:** ship `py.typed`; add `Application.on_activated`; fill the parity gaps in BIND §4; detect a second `Application.start()` in a backend and raise.
- **Acceptance:** `mypy --strict` on the examples passes against the installed wheel.

## Phase 4 — Cross-platform consistency

### 4.1 Platform behaviour table — P1, S (INTERACT-15)
- **Do:** extend `docs/guides/platforms.md` with every behaviour that differs (focus stealing, attention, clipboard serial rules, GNOME title-bar fallback, AT-SPI physical-pixel extents at scale ≠ 1, forced colours not applied, the stack-depth limits until 1.1 ships). State that Windows qualification is one Windows Server 2022 VM over RDP.
- **Acceptance:** each row links to a test or a documented lab check.

### 4.2 Windows and packaging fixes — P1, S each (INTERACT-09, F-19)
- **Do:** verify natively, then disable IME composition in password fields if confirmed; in `Get-UI4MEHostPath`, copy the host to a per-user cache with hash verification when it cannot be made executable.
- **Acceptance:** an AllUsers-as-root then run-as-user container test passes.

### 4.3 Locked-down Windows environments — P2, S (reasoned, not run)
- **Do:** document behaviour under AppLocker/WDAC (unsigned executables in user-writable paths) and PowerShell Constrained Language Mode (the module uses `Add-Type`); test once on the lab VM.
- **Acceptance:** documented outcome for each policy.

## Phase 5 — Testing and verification hardening

### 5.1 CI that runs the tests that matter — P0/P1, S–M (F-22)
- **Do:** P0 (1.0.2): T1, T2, T4, T7, T8, T9, T16. P1: Python 3.10–3.14 matrix; real-host Pester on pwsh 7 (Linux, Windows) and PS 5.1 without a path filter; nightly job with the fuzz harnesses at a 1 MiB worker stack, `--ignored` tests, `patches/*/verify.sh`, and a 30-minute windowed soak (R4b).
- **Acceptance:** every test T1–T16 in [04-testing-and-quality.md](04-testing-and-quality.md) runs automatically on PRs or nightly; nightly failures open an issue.

### 5.2 Windowed GUI smoke in CI — P1, M–L (T14)
- **Do:** Linux job with `weston --backend=headless` (or `kwin_wayland --virtual`, which this audit used successfully) and a private session bus; Windows job on hosted runners after a feasibility run; run `burst2.py`, `depth.py`, `svgdepth.py`, window churn and an AT-tree presence check.
- **Acceptance:** a PR that reintroduces F-01 or F-02 fails CI.

## Phase 6 — Packaging and release engineering

### 6.0 Release-artifact checks for 1.0.2 — P0, S (F-11 interim)
- **Do:** `verify_windows_artifact.py` asserts the stack reserve; after building the release candidates in CI, run `depth.py 250`, `svgdepth.py 700` and `burst2.py 100` against the **exact** wheel to be published, on the Windows lab VM, before dispatching the PyPI upload.
- **Acceptance:** recorded hashes of the tested wheel equal the published ones.

### 6.1 CI signal and publish gate — P0, S (F-09, F-10)
- **Do (1.0.2, minimal):** run `test_checked_manifest_matches_entire_source` only in the publish workflows (which already run `verify_package.py`) or when `tools/release/**` changes; make clippy run unconditionally (`if: ${{ !cancelled() }}` or a verifier stage); add a publish-preflight step that refuses unless the required CI check runs for the SHA concluded `success` (works on any GitHub plan); push CI only on `main`, PR CI on PRs, `concurrency` with cancel-in-progress, job timeouts, rust-cache.
- **Do (later, with 6.3):** stop committing `SOURCE_FILES.sha256`; generate it from tracked files in the publish preflight and inject it into the sdist, which keeps its real purpose (proving that builds outside Git used the reviewed file set).
- **Acceptance:** a comment-only PR is green with clippy run; a dry-run publish of a red commit is refused.

### 6.2 Repository protection — owner decision, not P0
- **Options:** keep the repository private on the current plan (no branch protection; rely on 6.1's CI gate and the manual typed-phrase dispatch); move to a paid plan (branch protection and rulesets; check current GitHub documentation for which plan enables required reviewers on deployment environments of private repositories); or make the repository public (free protection, reachable links for F-20). Visibility is an ownership and licensing decision, not an engineering one.
- **When a second maintainer exists:** require PRs, status checks and a reviewer on the publish environments.

### 6.3 Build once, qualify, promote — P1, M (F-11, RELEASE-04, RELEASE-13)
- **Do:** split publishing into build (draft release with sdist, wheels, nupkg, `SHA256SUMS`, build-provenance attestations, SBOM) and promote (verifies hashes against a machine-generated qualification record, uploads exactly those files, publishes the draft). The PowerShell workflow checks that HEAD equals the tag.
- **Acceptance:** every file on PyPI, the Gallery and the GitHub release has a hash listed in the qualification record.

### 6.4 Single-source version and generated records — P1, M (F-21, F-24)
- **Do:** the `Cargo.toml` workspace version as the single source; `tools/release/version.py` and `bump_version.py`; version-agnostic examples; `host.manifest` version generated; qualification records generated from `native_acceptance.py`/`Qualify.ps1`/`verify.py` output with raw evidence attached as a release asset.
- **Acceptance:** `git grep` finds no literal version in workflows or verifiers; a version bump touches ≤ 6 files.

### 6.5 Supply chain — P1/P2, S–M (F-23)
- **Do:** SHA-pin actions with Dependabot for actions, cargo and pip; `deny.toml` + `cargo deny` in CI with written ignores; hash-pinned release tooling; manylinux image by digest; fix the two template injections; replace `fontdue` (unmaintained `ttf-parser`) with the `skrifa`/parley stack already in the tree.
- **Acceptance:** zizmor and `cargo deny` clean in CI.

## Phase 7 — Documentation and developer experience

### 7.1 Honest stability and policy docs — P0 (notes, classifier) / P1 (policies), S (F-24, F-20)
- **Do (1.0.2):** release notes list the known open High items; change the PyPI classifier to `Development Status :: 4 - Beta` (and the Gallery description accordingly) until the stable gates pass.
- **Do (1.1):** `CHANGELOG.md`, `SECURITY.md`, `docs/release/versioning.md` (SemVer for the Python/PowerShell APIs and manifest, protocol capability policy, support policy, yank procedure); package metadata pointing at reachable URLs (checked in the preflight).
- **Acceptance:** every URL in package metadata returns 200 unauthenticated.

### 7.2 Startup and error DX — P2, S (BIND-05, -14, -04)
- **Do:** relay backend output losslessly in direct start (pipe it to the launcher instead of through the 512-line ring); human-readable startup errors when the systemd user session, Wayland or the portal are missing; prevent `host_binary()` from resolving to the console-script wrapper.
- **Acceptance:** 10,000 lines printed by a backend all reach the terminal; `env -i python app.py` prints an actionable one-paragraph message.

## Phase 8 — Longer-term, optional

| Item | Priority | Effort | Trigger |
| --- | --- | --- | --- |
| 8.1 Incremental pipeline O(dirty): skip clean subtrees in style bridge, host-tree, fragments and scene (F-16b) | P2 | L | apps with > 1,000 rows or 50 Hz updates after 3.4 |
| 8.2 Shrink the Taffy fork: upstream subgrid or isolate it behind extension points (F-27) | P2 | M–L | next Taffy upgrade |
| 8.3 Damage-limited surface conversion and `present_with_damage` (RENDER-08) | P2 | M | 4K/HiDPI performance complaints |
| 8.4 Remove crate-wide `#[allow(dead_code)]`; delete or feature-gate paged media (RENDER-11) | P2 | S–M | any renderer refactor |
| 8.5 Convert box-tree recursion to an explicit work stack | P2 | M | if new element types keep raising stack cost after 1.1(f) |
| 8.6 Upgrade winit once a non-beta 0.31 exists; re-validate the four platform patches natively | P2 | M | upstream release |
| 8.7 Forced-colours / Windows High Contrast palette (`forced-colors`, `prefers-contrast`) | P1–P2 | M | before claiming accessibility conformance |
