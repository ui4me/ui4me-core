# 4. Testing and quality assessment

## 4.1 What exists

| Suite | Size | What it really proves | In CI |
| --- | --- | --- | --- |
| Rust unit/integration, renderer | 501 + 6 ignored | Rendering semantics through targeted pixel and semantic probes; incremental-vs-cold oracle; layout, text, scene, damage raster | yes |
| Rust, document / assets / engine-types | ~470 incl. 7 proptest files | DOM mutation, parsing, journal, resource policy, decoders, bomb bounds, geometry | yes |
| Rust, host | 94 unit + 8 headless process integration | protocol, lifecycle state machine, transport, platform helpers | yes |
| Python tool tests (`tools/test_*.py`) | 120 | reference client, direct start against the **real host in headless mode**, docs examples, release tooling | yes, Python 3.12 only |
| Headless stages (`verify.py headless/renderer/faults`) | 3 stages, 7 fault modes | protocol/lifecycle end to end, crash/freeze/veto handling, process-tree cleanup | yes |
| Pester | 54 `It` (19 unit, 24 mock-host transport, 11 direct start) | module logic; **4 real-host tests are skipped in every CI run** | separate, path-filtered workflow |
| Fuzz harnesses (`fuzz_gate.rs`, `fuzz_resources.rs`) | 4 seeded mutation harnesses; the HTML harness already generates nesting bombs up to 600 levels (`fuzz_gate.rs:172-178`) | run on 8 MiB worker threads, which hides stack exhaustion | **no** (`#[ignore]`) |
| Native ledgers (`native_acceptance.py` N01–N15, `renderer_acceptance.py` R01–R12, `completion_acceptance.py` C01–C12, `Qualify.ps1` 28 checks) | 39 rows + 28 checks | real windows, input, IME, clipboard, AT-SPI/UIA | **no**; private lab only (root uinput daemon, KWin scripting, RDP via Xvfb), not reproducible by a third party |

Local run at the audited commit: all default verifier stages pass (1,094 Rust tests, 120 Python tests). See [01-scope-and-method.md](01-scope-and-method.md).

**Quality of the existing tests is better than average.** Renderer tests assert specific pixels and semantics instead of opaque golden images; the cold pipeline is used as an oracle; the verifier records exact commands and refuses to call headless evidence "native". Do not convert these to snapshots.

## 4.2 What the tests did not catch, and why

Every confirmed Critical/High runtime defect in this audit passed the existing suite. The reasons are structural, not carelessness:

| Defect | Why the suite was blind |
| --- | --- |
| F-01 Windows stack overflow at depth 40 | Tests run layouts on Linux (8 MiB main stack) or on 8 MiB fuzz workers; nothing runs the documented maximum depth on a production-sized stack; fuzzers not in CI. Run at a 1 MiB worker stack, the existing HTML harness would very likely have found it. |
| F-02 burst disconnect, F-29 binding drops | Headless runs no transitions; no transport test with a rate-limited reader; no event-volume budget; no binding test that a control notification survives a burst. |
| F-03 SVG nesting abort | SVG pre-scan tests cover counts and specific constructs, not generic nesting. The fuzz nesting bomb uses inline SVG (capped at depth 48), not image SVG, so even a CI fuzz run would need a new generator. |
| F-04 font memory growth | No soak test with allocation accounting; headless skips the layout that triggers it. |
| F-07 geometry in fixed/transformed boxes | Each geometry consumer is tested in isolation, never cross-checked against hit testing. |
| F-08 focus not scrolled into view | No test covers focusing an off-screen element. |
| F-13 callback worker death | No test raises `BaseException` from a callback. |
| F-19 AllUsers install | Only CurrentUser installs are exercised. |

The lesson: **add tests that assert invariants across components and run at the documented limits**, not more example-based tests of single functions.

## 4.3 Highest-value tests to add (ranked by risk reduction per cost)

| # | Test | Catches | Cost | Where |
| --- | --- | --- | --- | --- |
| T1 | **Depth-limit survival test**: render a document at the documented maximum depth for each expensive element type (`div`, `fieldset`, `table`, flex, `display:contents`; `inline-block` after roadmap 2.5 removes its exponential layout) on a thread whose stack is half the configured engine stack; same for image SVG at the pre-scan cap on half the asset-worker stack. Abort-free, not timed. Release profile, Windows and Linux CI. | F-01, F-03, F-06 | S | renderer + assets tests |
| T2 | **Transport burst test**: a reader consuming at a fixed rate (1 frame/ms) while the host enqueues 10,000 events plus interleaved responses and lifecycle notifications in one burst; assert no disconnect and every message delivered in order; a peer that stops reading is still disconnected at the write deadline. | F-02 | S | `ui4me-host` transport tests |
| T3 | **Headless virtual clock** plus a headless version of `burst2.py` (100 transitioned controls, colour-scheme toggle) asserting the backend survives and unsubscribed types are not sent. | F-02, F-12 | M (needs the clock) | `tools/` headless stage |
| T4 | **Font identity unit test and allocation soak**: shape a bold paragraph, force parley's source-cache prune, shape again, assert stable blob id and bounded `system_blobs`; plus a counting-allocator soak of the issue #2 op stream (growth < 1 MiB after warm-up). | F-04 | S | renderer |
| T5 | **Geometry consistency property test**: for fixtures with fixed boxes, sticky, nested scrollers, transforms, before and after scrolling: `hit_test(centre(rect(n))) ∈ {n, descendants}`; select-option rows hit their option; caret from a click at glyph k is k; IME area and a11y text bounds lie inside the control. | F-07 | M | renderer |
| T6 | **Fuzz in CI** (nightly, 90–120 s per harness, seeded, **1 MiB worker stack**). The existing nesting bomb covers F-01; add generators for nested `<g>`/`clip-path` chains in *image* SVG (F-03), deep `move_element` chains (F-06) and SVG filter cost (F-14). | F-01, F-03, F-06, F-14, F-15 | S | nightly workflow |
| T7 | **Comment-only PR is green**: CI self-test after removing the manifest change-detector; clippy runs even when tests fail. | F-09 | S | CI |
| T8 | **Publish preflight refuses a commit with red CI** (dry-run dispatch). | F-10 | S | publish workflow |
| T9 | **Python callback robustness**: `sys.exit()`, `KeyboardInterrupt`, unencodable return value and an exception in one of several handlers; assert the worker survives and the error is visible with a traceback. | F-13 | S | `tools/test_application.py` |
| T10 | **Keyboard matrix**: Enter/Space on button, link, summary, checkbox, radio, submit, `role=button`; Tab to an off-screen control scrolls it into view; shortcuts on a non-Latin `key` with Latin `code`. | F-08, F-17, F-17b | S–M | renderer interaction tests |
| T11 | **Python version matrix** (3.10–3.14) for `test_bootstrap`, `test_application`, `test_reference_client` against one built host; **Pester with the real host** on pwsh 7 (Linux, Windows) and PS 5.1, no path filter. | F-22 | S | CI |
| T12 | **Clean-machine smoke of published artifacts** incl. PowerShell AllUsers-as-root then run as user. | F-19, F-20 | S | post-publish workflow |
| T13 | **Long-document work-counter gates**: "append one row after 20 unrelated updates shapes ≤ 2 paragraphs"; "N animation ticks with no AT connected rebuild the a11y tree 0 times". Counters, not wall time. | F-16a, F-16b | M | renderer + host |
| T14 | **Windowed GUI smoke in CI**: Linux under a nested/headless compositor (this audit ran a `kwin_wayland --virtual` session successfully; `weston --backend=headless` is the lighter option) — open a window, wait for the first frame, run `burst2.py` and `depth.py`, query AT-SPI on a private bus. Windows on `windows-2022` runners (interactive desktop available on hosted runners; needs a feasibility run). | F-02, F-01, platform regressions | M–L | CI |
| T15 | **Lifecycle edge tests**: accept-loop EMFILE, SIGTERM delivers `app.shutdown`, relaunch during shutdown, env vars systemd rejects. | F-18, F-25 | S each | host / tools |
| T16 | **Binding control-notification test**: during a 1,000-event burst, `request_close()` retires the window and the drop counter shows no control notification, in Python and PowerShell 5.1; a `click` after a 300-event burst is delivered. | F-29 | S–M | `tools/`, Pester |

**1.0.2 set:** T1, T2, T4, T7, T8, T9 and T16 (T3 needs the headless clock and ships with roadmap 2.1). Together with the fixes they cover, this is roughly three engineer-weeks. This set would have caught F-01, F-02, F-03, F-04, F-13 and F-29; F-07, F-08 and F-17 need T5 and T10.

## 4.4 Native GUI verification strategy

Native behaviour (input, IME, AT-SPI/UIA, DPI, clipboard, dialogs) is currently proven only by private-lab runs whose raw evidence was later deleted from the tree. Two changes make it trustworthy without making it heavy:

1. **Automate the cheap native layer in CI** (T14): first frame, window lifecycle, event bursts, depth limits, AT tree presence. This catches regressions on every PR.
2. **Keep a small, scripted manual/lab layer for what cannot be automated** (screen-reader journeys, IME composition, fractional scaling, Wayland focus policies), run it against the **exact artifacts to be published** (build once, qualify, promote: F-11), and attach the raw evidence to the release as an asset. The existing BLOCKED-by-default acceptance tools are the right base; their output should generate the qualification record (RELEASE-10).

Screen-reader journeys worth scripting first (from INTERACT): Orca and NVDA — initial focus, Tab to an off-screen control, live-region announcement on `set_text`, combobox active descendant, a modal dialog after scrolling.

## 4.5 Static analysis and hygiene

| Check | State | Recommendation |
| --- | --- | --- |
| `cargo fmt` | enforced for the five crates | keep |
| `cargo clippy -D warnings` | configured, but **skipped on every PR** because the preceding step fails (F-09) | run it unconditionally |
| `unsafe_op_in_unsafe_fn = deny` | set | keep |
| `cargo deny` / advisories | not in CI; locally 0 vulnerabilities, 4 unmaintained (`ttf-parser` via `fontdue`, three zbus wire crates) | add with written ignores |
| `cargo miri` | not used | not recommended broadly (Stylo/FFI-heavy); optional for `ui4me-document` arena and journal |
| Crate-wide `#[allow(dead_code)]` in the renderer | hides ~1,500 lines of unreachable paged-media code | remove the blanket allow |
| Workflow linting (actionlint, zizmor) | none; zizmor reports 42 unpinned actions, 2 template injections | add |
